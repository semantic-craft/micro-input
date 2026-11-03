"""Unit tests for scripts/run --verify QA isolation, path guard, lock, and cleanup."""
import contextlib
import fcntl
import io
import json
import os
from pathlib import Path
import plistlib
import runpy
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

LAUNCHER = Path(__file__).resolve().parents[1] / 'run'


class FakeVerifyChild:
    pid = 987651

    def __init__(self, outcomes, audit_line=None):
        self.outcomes = iter(outcomes)
        self.returncode = None
        self.terminated = False
        self.killed = False
        self.audit_line = audit_line
        self.env = {}

    def poll(self):
        return self.returncode

    def terminate(self):
        self.terminated = True
        self.returncode = -15

    def kill(self):
        self.killed = True
        self.returncode = -9

    def wait(self, timeout=None):
        if self.audit_line and 'MI_HOST_AUDIT_LOG' in self.env:
            Path(self.env['MI_HOST_AUDIT_LOG']).write_text(self.audit_line + '\n', encoding='utf-8')
        outcome = next(self.outcomes, 0)
        if outcome == 'timeout':
            raise subprocess.TimeoutExpired('fake-qa', timeout or 600)
        self.returncode = outcome
        return outcome


class RunVerifyLauncherTests(unittest.TestCase):
    def _prepare_checkout(self) -> Path:
        sandbox = tempfile.TemporaryDirectory(prefix='mi-run-verify-test-')
        self.addCleanup(sandbox.cleanup)
        root = Path(sandbox.name).resolve()
        script = root / 'scripts/run'
        script.parent.mkdir(parents=True)
        shutil.copyfile(LAUNCHER, script)
        macos_dir = root / 'target/MicroInput.app/Contents/MacOS'
        macos_dir.mkdir(parents=True)
        binary = macos_dir / 'MicroInput'
        binary.write_text('#!/bin/sh\nexit 0\n', encoding='utf-8')
        binary.chmod(0o755)
        info_path = root / 'target/MicroInput.app/Contents/Info.plist'
        with info_path.open('wb') as handle:
            plistlib.dump({
                'CFBundleIdentifier': 'com.semanticcraft.microinput',
                'CFBundleName': 'Micro Input',
                'CFBundleDisplayName': 'Micro Input',
            }, handle)
        return root

    def test_rejects_tmp_and_private_tmp_qa_directories(self):
        root = self._prepare_checkout()
        script = root / 'scripts/run'
        for forbidden in ('/tmp/mi-qa-forbidden', '/private/tmp/mi-qa-forbidden'):
            stderr = io.StringIO()
            with patch.object(sys, 'argv', [str(script), '--verify']), \
                    patch.dict(os.environ, {'MI_QA_RUN_DIR': forbidden}, clear=False), \
                    patch('subprocess.Popen') as popen, \
                    patch('subprocess.run') as run_cmd, \
                    contextlib.redirect_stderr(stderr):
                with self.assertRaises(SystemExit) as result:
                    runpy.run_path(str(script), run_name='__main__')
                self.assertEqual(result.exception.code, 2)
                self.assertIn('/tmp', stderr.getvalue())
                popen.assert_not_called()
                run_cmd.assert_not_called()

    def test_fixed_bundle_id_and_evidence_preserved_after_cleanup(self):
        root = self._prepare_checkout()
        script = root / 'scripts/run'
        child = FakeVerifyChild([0], audit_line='{"action_type":"open_app","outcome":"executed"}')
        captured_plist = {}

        def fake_spawn(command, **kwargs):
            child.env = kwargs.get('env', {})
            app_binary = Path(command[0])
            info_path = app_binary.parent.parent / 'Info.plist'
            with info_path.open('rb') as handle:
                captured_plist.update(plistlib.load(handle))
            return child

        stdout = io.StringIO()
        fake_home = root / 'fake_home'
        prefs_dir = fake_home / 'Library/Preferences'
        prefs_dir.mkdir(parents=True)
        stale_plist_fixed = prefs_dir / 'com.semanticcraft.microinput.qa.plist'
        stale_plist_unique = prefs_dir / 'com.semanticcraft.microinput.qa.12345.plist'
        prod_plist = prefs_dir / 'com.semanticcraft.microinput.plist'
        for p in (stale_plist_fixed, stale_plist_unique, prod_plist):
            p.write_text('plist', encoding='utf-8')

        with patch.object(sys, 'argv', [str(script), '--verify', '--timeout=15']), \
                patch('pathlib.Path.home', return_value=fake_home), \
                patch('subprocess.Popen', side_effect=fake_spawn) as popen, \
                patch('subprocess.run') as codesign, \
                contextlib.redirect_stdout(stdout):
            codesign.return_value.returncode = 0
            with self.assertRaises(SystemExit) as result:
                runpy.run_path(str(script), run_name='__main__')
            self.assertEqual(result.exception.code, 0)
            self.assertEqual(popen.call_count, 1)

        self.assertEqual(captured_plist['CFBundleIdentifier'], 'com.semanticcraft.microinput.qa')
        self.assertFalse(stale_plist_fixed.exists())
        self.assertFalse(stale_plist_unique.exists())
        self.assertTrue(prod_plist.exists(), 'Production preferences plist must not be deleted by QA cleanup')
        run_dirs = [p for p in (root / '.scratch/qa-runs').iterdir() if p.is_dir()]
        self.assertEqual(len(run_dirs), 1)
        run_dir = run_dirs[0]
        self.assertEqual(stat.S_IMODE(run_dir.stat().st_mode), 0o700)
        self.assertFalse((run_dir / 'Micro Input QA.app').exists())
        self.assertFalse((run_dir / 'documents').exists())
        evidence_dir = run_dir / 'evidence'
        self.assertTrue(evidence_dir.exists())
        launcher_json = json.loads((evidence_dir / 'launcher.json').read_text(encoding='utf-8'))
        self.assertEqual(launcher_json['bundle_id'], 'com.semanticcraft.microinput.qa')
        self.assertTrue(launcher_json['app_bundle_removed'])
        self.assertTrue(launcher_json['documents_removed'])
        self.assertTrue(launcher_json['preferences_removed'])
        audit_text = (evidence_dir / 'audit.jsonl').read_text(encoding='utf-8')
        self.assertIn('"action_type":"open_app"', audit_text)

    def test_single_instance_lock_prevents_concurrent_fixed_bundle_collision(self):
        root = self._prepare_checkout()
        script = root / 'scripts/run'
        qa_root = root / '.scratch/qa-runs'
        qa_root.mkdir(parents=True)
        lock_file = (qa_root / 'qa.lock').open('w', encoding='utf-8')
        self.addCleanup(lock_file.close)
        fcntl.flock(lock_file.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)

        stderr = io.StringIO()
        with patch.object(sys, 'argv', [str(script), '--verify']), \
                patch('subprocess.Popen') as popen, \
                patch('subprocess.run'), \
                contextlib.redirect_stderr(stderr):
            with self.assertRaises(SystemExit) as result:
                runpy.run_path(str(script), run_name='__main__')
            self.assertEqual(result.exception.code, 2)
            self.assertIn('qa.lock', stderr.getvalue())
            popen.assert_not_called()

        # --unique-bundle-id bypasses the shared fixed-bundle lock.
        child = FakeVerifyChild([0])
        captured_plist = {}

        def fake_spawn(command, **kwargs):
            info_path = Path(command[0]).parent.parent / 'Info.plist'
            with info_path.open('rb') as handle:
                captured_plist.update(plistlib.load(handle))
            return child

        with patch.object(sys, 'argv', [str(script), '--verify', '--unique-bundle-id']), \
                patch('subprocess.Popen', side_effect=fake_spawn) as popen, \
                patch('subprocess.run'), \
                contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaises(SystemExit) as result:
                runpy.run_path(str(script), run_name='__main__')
            self.assertEqual(result.exception.code, 0)
            self.assertEqual(popen.call_count, 1)
            self.assertTrue(captured_plist['CFBundleIdentifier'].startswith('com.semanticcraft.microinput.qa.'))

    def test_timeout_terminates_child_and_cleans_up_bundle(self):
        root = self._prepare_checkout()
        script = root / 'scripts/run'
        child = FakeVerifyChild(['timeout', -15])

        with patch.object(sys, 'argv', [str(script), '--verify', '--timeout=2']), \
                patch('subprocess.Popen', return_value=child), \
                patch('subprocess.run'), \
                contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaises(SystemExit) as result:
                runpy.run_path(str(script), run_name='__main__')
            self.assertEqual(result.exception.code, 0)

        self.assertTrue(child.terminated)
        run_dirs = [p for p in (root / '.scratch/qa-runs').iterdir() if p.is_dir()]
        self.assertEqual(len(run_dirs), 1)
        launcher_json = json.loads((run_dirs[0] / 'evidence/launcher.json').read_text(encoding='utf-8'))
        self.assertTrue(launcher_json['app_bundle_removed'])


if __name__ == '__main__':
    unittest.main()
