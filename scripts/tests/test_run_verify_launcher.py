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
        lock_fd = os.open(qa_root / 'qa.lock', os.O_RDWR | os.O_CREAT, 0o600)
        lock_file = os.fdopen(lock_fd, 'w', encoding='utf-8')
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

    def test_rejects_unknown_flags_without_spawning_app(self):
        root = self._prepare_checkout()
        script = root / 'scripts/run'
        for bad_args in (['--unknown-flag'], ['--verify', '--bogus'], ['positional']):
            stderr = io.StringIO()
            with patch.object(sys, 'argv', [str(script), *bad_args]), \
                    patch('subprocess.Popen') as popen, \
                    contextlib.redirect_stderr(stderr):
                with self.assertRaises(SystemExit) as result:
                    runpy.run_path(str(script), run_name='__main__')
                self.assertEqual(result.exception.code, 2)
                popen.assert_not_called()

    def test_list_and_kill_manage_checkout_and_qa_without_spawning_or_killing_installed_app(self):
        root = self._prepare_checkout()
        script = root / 'scripts/run'
        ps_output = (
            f"  78893 /Applications/Micro Input.app/Contents/MacOS/MicroInput\n"
            f"  78877 {root / 'target/MicroInput.app/Contents/MacOS/MicroInput'}\n"
            f"  78901 {root / '.scratch/qa-runs/123-456/Micro Input QA.app/Contents/MacOS/MicroInput'}\n"
        )
        stdout = io.StringIO()
        with patch.object(sys, 'argv', [str(script), '--list']), \
                patch('subprocess.check_output', return_value=ps_output), \
                patch('subprocess.Popen') as popen, \
                contextlib.redirect_stdout(stdout):
            with self.assertRaises(SystemExit) as result:
                runpy.run_path(str(script), run_name='__main__')
            self.assertEqual(result.exception.code, 0)
            popen.assert_not_called()
        listed = stdout.getvalue()
        self.assertIn('78877', listed)
        self.assertIn('78901', listed)
        self.assertNotIn('78893', listed)

        killed = []
        with patch.object(sys, 'argv', [str(script), '--kill']), \
                patch('subprocess.check_output', return_value=ps_output), \
                patch('os.kill', side_effect=lambda pid, sig: killed.append((pid, sig))), \
                patch('subprocess.Popen') as popen, \
                contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaises(SystemExit) as result:
                runpy.run_path(str(script), run_name='__main__')
            self.assertEqual(result.exception.code, 0)
            popen.assert_not_called()
        killed_pids = {pid for pid, _ in killed}
        self.assertEqual(killed_pids, {78877, 78901})
        self.assertNotIn(78893, killed_pids)

    def test_strips_polluted_bundle_env_and_refuses_duplicate_non_verify_instance(self):
        root = self._prepare_checkout()
        script = root / 'scripts/run'
        fake_home = root / 'fake_home'
        doc_dir = fake_home / 'Library/Application Support/MicroInput'
        doc_dir.mkdir(parents=True)
        lock_fd = os.open(doc_dir / 'instance.lock', os.O_RDWR | os.O_CREAT, 0o600)
        lock_file = os.fdopen(lock_fd, 'w', encoding='utf-8')
        self.addCleanup(lock_file.close)
        fcntl.flock(lock_file.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)

        stderr = io.StringIO()
        with patch.object(sys, 'argv', [str(script)]), \
                patch('pathlib.Path.home', return_value=fake_home), \
                patch.dict(os.environ, {'__CFBundleIdentifier': 'com.google.antigravity'}, clear=False), \
                patch('subprocess.Popen') as popen, \
                contextlib.redirect_stderr(stderr):
            with self.assertRaises(SystemExit) as result:
                runpy.run_path(str(script), run_name='__main__')
            self.assertEqual(result.exception.code, 2)
            self.assertIn('instance.lock', stderr.getvalue())
            popen.assert_not_called()

        fcntl.flock(lock_file.fileno(), fcntl.LOCK_UN)
        child = FakeVerifyChild([0])
        captured_env = {}

        def fake_spawn(command, **kwargs):
            captured_env.update(kwargs.get('env', {}))
            return child

        with patch.object(sys, 'argv', [str(script)]), \
                patch('pathlib.Path.home', return_value=fake_home), \
                patch.dict(os.environ, {'__CFBundleIdentifier': 'com.google.antigravity', 'XPC_SERVICE_NAME': '0'}, clear=False), \
                patch('subprocess.Popen', side_effect=fake_spawn) as popen:
            with self.assertRaises(SystemExit) as result:
                runpy.run_path(str(script), run_name='__main__')
            self.assertEqual(result.exception.code, 0)
            self.assertEqual(popen.call_count, 1)
        self.assertNotIn('__CFBundleIdentifier', captured_env)
        self.assertNotIn('XPC_SERVICE_NAME', captured_env)

    def test_verify_resigns_qa_bundle_inside_out_instead_of_adhoc_outer_only(self):
        root = self._prepare_checkout()
        script = root / 'scripts/run'
        child = FakeVerifyChild([0])
        run_calls = []

        def fake_run(cmd, **kwargs):
            run_calls.append([str(part) for part in cmd])
            return subprocess.CompletedProcess(args=cmd, returncode=0)

        with patch.object(sys, 'argv', [str(script), '--verify', '--timeout=10']), \
                patch('pathlib.Path.home', return_value=root / 'fake_home'), \
                patch('subprocess.Popen', return_value=child), \
                patch('subprocess.run', side_effect=fake_run), \
                contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaises(SystemExit) as result:
                runpy.run_path(str(script), run_name='__main__')
            self.assertEqual(result.exception.code, 0)

        sign_cmds = [cmd for cmd in run_calls if any(part.endswith('sign_bundle.py') for part in cmd)]
        self.assertEqual(len(sign_cmds), 1)
        cmd = sign_cmds[0]
        self.assertEqual(cmd[0], sys.executable)
        self.assertTrue(cmd[-4].endswith('sign_bundle.py'))
        self.assertTrue(cmd[-3].endswith('Micro Input QA.app'))
        self.assertEqual(cmd[-2:], ['--mode', 'dev'])
        self.assertFalse(any(call[:4] == ['codesign', '--force', '--sign', '-'] for call in run_calls))

    def test_verify_preserves_framework_symlinks_and_disables_updater(self):
        root = self._prepare_checkout()
        script = root / 'scripts/run'
        fw = root / 'target/MicroInput.app/Contents/Frameworks/Sparkle.framework'
        ver_b = fw / 'Versions/B'
        (ver_b / 'Updater.app/Contents/MacOS').mkdir(parents=True)
        (ver_b / 'Sparkle').write_text('macho', encoding='utf-8')
        (fw / 'Versions/Current').symlink_to('B')
        (fw / 'Sparkle').symlink_to('Versions/Current/Sparkle')
        (fw / 'Updater.app').symlink_to('Versions/Current/Updater.app')

        child = FakeVerifyChild([0])
        captured = {}

        def fake_spawn(command, **kwargs):
            app_root = Path(command[0]).parent.parent.parent
            copied_fw = app_root / 'Contents/Frameworks/Sparkle.framework'
            captured['current_is_symlink'] = (copied_fw / 'Versions/Current').is_symlink()
            captured['top_sparkle_is_symlink'] = (copied_fw / 'Sparkle').is_symlink()
            captured['env'] = kwargs.get('env', {})
            with (app_root / 'Contents/Info.plist').open('rb') as handle:
                captured['plist'] = plistlib.load(handle)
            return child

        run_calls = []

        def fake_run(cmd, **kwargs):
            run_calls.append([str(c) for c in cmd])
            return subprocess.CompletedProcess(args=cmd, returncode=0)

        with patch.object(sys, 'argv', [str(script), '--verify', '--timeout=10']), \
                patch('pathlib.Path.home', return_value=root / 'fake_home'), \
                patch('subprocess.Popen', side_effect=fake_spawn), \
                patch('subprocess.run', side_effect=fake_run), \
                contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaises(SystemExit) as result:
                runpy.run_path(str(script), run_name='__main__')
            self.assertEqual(result.exception.code, 0)

        self.assertTrue(captured['current_is_symlink'])
        self.assertTrue(captured['top_sparkle_is_symlink'])
        self.assertIs(captured['plist']['SUEnableAutomaticChecks'], False)
        self.assertEqual(captured['env'].get('MI_DISABLE_UPDATER'), '1')
        unregistered_targets = [cmd[-1] for cmd in run_calls if len(cmd) >= 3 and cmd[-2] == '-u']
        lsreg = Path('/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister')
        if lsreg.is_file():
            self.assertTrue(
                any(target.endswith('Sparkle.framework/Versions/B/Updater.app') for target in unregistered_targets),
                f'Expected nested Updater.app unregistration in {unregistered_targets}',
            )
        else:
            self.assertEqual(unregistered_targets, [])

    def test_verify_prunes_completed_default_qa_runs_beyond_retention_cap(self):
        root = self._prepare_checkout()
        script = root / 'scripts/run'
        qa_root = root / '.scratch/qa-runs'
        qa_root.mkdir(parents=True)
        for index in range(25):
            evidence_dir = qa_root / f'1700000000{index:03d}-100' / 'evidence'
            evidence_dir.mkdir(parents=True)
            (evidence_dir / 'launcher.json').write_text('{"ok":true}\n', encoding='utf-8')
        active_dir = qa_root / '9999999999999-111'
        (active_dir / 'Micro Input QA.app').mkdir(parents=True)

        fake_home = root / 'fake_home'
        (fake_home / 'Library/Preferences').mkdir(parents=True)

        with patch.object(sys, 'argv', [str(script), '--verify', '--timeout=15']), \
                patch('pathlib.Path.home', return_value=fake_home), \
                patch('subprocess.Popen', side_effect=lambda *args, **kwargs: FakeVerifyChild([0])), \
                patch('subprocess.run'), \
                contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaises(SystemExit) as result:
                runpy.run_path(str(script), run_name='__main__')
            self.assertEqual(result.exception.code, 0)

        for index in range(20):
            self.assertFalse((qa_root / f'1700000000{index:03d}-100').exists())
        for index in range(20, 25):
            self.assertTrue((qa_root / f'1700000000{index:03d}-100').exists())
        completed_dirs = [
            p for p in qa_root.iterdir()
            if p.is_dir() and p.name != '9999999999999-111'
        ]
        self.assertEqual(len(completed_dirs), 6)
        self.assertTrue((active_dir / 'Micro Input QA.app').exists())
        self.assertTrue((qa_root / 'qa.lock').exists())

        with patch.object(sys, 'argv', [str(script), '--verify', '--timeout=15']), \
                patch.dict(os.environ, {'MI_QA_MAX_RETAINED_RUNS': '3'}, clear=False), \
                patch('pathlib.Path.home', return_value=fake_home), \
                patch('subprocess.Popen', side_effect=lambda *args, **kwargs: FakeVerifyChild([0])), \
                patch('subprocess.run'), \
                contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaises(SystemExit) as result:
                runpy.run_path(str(script), run_name='__main__')
            self.assertEqual(result.exception.code, 0)

        remaining_completed = sorted(
            p.name for p in qa_root.iterdir()
            if p.is_dir() and p.name != '9999999999999-111'
        )
        self.assertEqual(len(remaining_completed), 3)
        self.assertIn('1700000000024-100', remaining_completed)
        self.assertTrue((active_dir / 'Micro Input QA.app').exists())
        self.assertTrue((qa_root / 'qa.lock').exists())

    def test_verify_prunes_collision_suffixed_and_nested_qa_run_dir_only(self):
        root = self._prepare_checkout()
        script = root / 'scripts/run'
        qa_root = root / '.scratch/qa-runs'
        qa_root.mkdir(parents=True)
        seeded = [
            '1700000000001-100',
            '1700000000001-100-1',
            '1700000000002-100',
            '1700000000003-100-2',
            '1700000000004-100',
            '1700000000005-100',
            '1700000000006-100-1',
            '1700000000007-100',
        ]
        for run_id in seeded:
            evidence_dir = qa_root / run_id / 'evidence'
            evidence_dir.mkdir(parents=True)
            (evidence_dir / 'launcher.json').write_text('{"ok":true}\n', encoding='utf-8')

        fake_home = root / 'fake_home'
        (fake_home / 'Library/Preferences').mkdir(parents=True)

        external_run_dir = root / 'external-qa-run'
        with patch.object(sys, 'argv', [str(script), '--verify', '--timeout=15']), \
                patch.dict(os.environ, {'MI_QA_RUN_DIR': str(external_run_dir)}, clear=False), \
                patch('pathlib.Path.home', return_value=fake_home), \
                patch('subprocess.Popen', side_effect=lambda *args, **kwargs: FakeVerifyChild([0])), \
                patch('subprocess.run'), \
                contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaises(SystemExit) as result:
                runpy.run_path(str(script), run_name='__main__')
            self.assertEqual(result.exception.code, 0)

        self.assertTrue((external_run_dir / 'evidence/launcher.json').exists())
        self.assertEqual(sorted(p.name for p in qa_root.iterdir() if p.is_dir()), seeded)

        nested_run_dir = qa_root / '1700000000008-200-1'
        with patch.object(sys, 'argv', [str(script), '--verify', '--timeout=15']), \
                patch.dict(os.environ, {'MI_QA_RUN_DIR': str(nested_run_dir)}, clear=False), \
                patch('pathlib.Path.home', return_value=fake_home), \
                patch('subprocess.Popen', side_effect=lambda *args, **kwargs: FakeVerifyChild([0])), \
                patch('subprocess.run'), \
                contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaises(SystemExit) as result:
                runpy.run_path(str(script), run_name='__main__')
            self.assertEqual(result.exception.code, 0)

        self.assertEqual(
            sorted(p.name for p in qa_root.iterdir() if p.is_dir()),
            [
                '1700000000003-100-2',
                '1700000000004-100',
                '1700000000005-100',
                '1700000000006-100-1',
                '1700000000007-100',
                '1700000000008-200-1',
            ],
        )

    def test_unique_bundle_id_cleanup_never_deletes_other_qa_session_plists(self):
        root = self._prepare_checkout()
        script = root / 'scripts/run'
        child = FakeVerifyChild([0])
        captured_plist = {}

        def fake_spawn(command, **kwargs):
            info_path = Path(command[0]).parent.parent / 'Info.plist'
            with info_path.open('rb') as handle:
                captured_plist.update(plistlib.load(handle))
            return child

        fake_home = root / 'fake_home'
        prefs_dir = fake_home / 'Library/Preferences'
        prefs_dir.mkdir(parents=True)
        other_unique = prefs_dir / 'com.semanticcraft.microinput.qa.other999.plist'
        fixed_qa = prefs_dir / 'com.semanticcraft.microinput.qa.plist'
        prod_plist = prefs_dir / 'com.semanticcraft.microinput.plist'
        for p in (other_unique, fixed_qa, prod_plist):
            p.write_text('plist', encoding='utf-8')

        with patch.object(sys, 'argv', [str(script), '--verify', '--unique-bundle-id']), \
                patch('pathlib.Path.home', return_value=fake_home), \
                patch('subprocess.Popen', side_effect=fake_spawn), \
                patch('subprocess.run'), \
                contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaises(SystemExit) as result:
                runpy.run_path(str(script), run_name='__main__')
            self.assertEqual(result.exception.code, 0)

        self.assertTrue(captured_plist['CFBundleIdentifier'].startswith('com.semanticcraft.microinput.qa.'))
        self.assertTrue(other_unique.exists(), 'Concurrent --unique-bundle-id session plist must not be deleted')
        self.assertTrue(fixed_qa.exists(), 'Fixed QA session plist must not be deleted by --unique-bundle-id run')
        self.assertTrue(prod_plist.exists(), 'Production preferences plist must not be deleted')

    def test_lock_hardening_rejects_fifo_symlink_user_symlink_parent_and_loose_mode(self):
        root = self._prepare_checkout()
        script = root / 'scripts/run'

        qa_root = root / '.scratch/qa-runs'
        qa_root.mkdir(parents=True)
        fifo_qa_lock = qa_root / 'qa.lock'
        os.mkfifo(fifo_qa_lock, 0o600)

        stderr = io.StringIO()
        with patch.object(sys, 'argv', [str(script), '--verify']), \
                patch('subprocess.Popen') as popen, \
                patch('subprocess.run'), \
                contextlib.redirect_stderr(stderr):
            result_box = []

            def run_launcher():
                try:
                    runpy.run_path(str(script), run_name='__main__')
                except SystemExit as exc:
                    result_box.append(('exit', exc.code))
                except BaseException as exc:
                    result_box.append(('err', exc))

            import threading
            t = threading.Thread(target=run_launcher, daemon=True)
            t.start()
            t.join(timeout=0.5)
            if t.is_alive():
                drain = os.open(fifo_qa_lock, os.O_RDONLY | os.O_NONBLOCK)
                t.join(timeout=1.0)
                os.close(drain)
                self.fail('scripts/run blocked opening FIFO qa.lock without O_NONBLOCK')
            self.assertEqual(result_box[0][0], 'exit')
            self.assertEqual(result_box[0][1], 2)
            popen.assert_not_called()

        fifo_qa_lock.unlink()
        victim = root / 'victim.txt'
        victim.write_text('KEEP', encoding='utf-8')
        fifo_qa_lock.symlink_to(victim)
        with patch.object(sys, 'argv', [str(script), '--verify']), \
                patch('subprocess.Popen') as popen, \
                patch('subprocess.run'), \
                contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit) as result:
                runpy.run_path(str(script), run_name='__main__')
            self.assertEqual(result.exception.code, 2)
            popen.assert_not_called()
        self.assertEqual(victim.read_text(encoding='utf-8'), 'KEEP')

        fifo_qa_lock.unlink()
        fifo_qa_lock.write_text('', encoding='utf-8')
        os.chmod(fifo_qa_lock, 0o644)
        with patch.object(sys, 'argv', [str(script), '--verify']), \
                patch('subprocess.Popen') as popen, \
                patch('subprocess.run'), \
                contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit) as result:
                runpy.run_path(str(script), run_name='__main__')
            self.assertEqual(result.exception.code, 2)
            popen.assert_not_called()

        fake_home = root / 'fake_home'
        real_target = root / 'real_doc_dir'
        real_target.mkdir(parents=True)
        os.chmod(real_target, 0o755)
        doc_parent = fake_home / 'Library/Application Support'
        doc_parent.mkdir(parents=True)
        (doc_parent / 'MicroInput').symlink_to(real_target)
        with patch.object(sys, 'argv', [str(script)]), \
                patch('pathlib.Path.home', return_value=fake_home), \
                patch('subprocess.Popen') as popen, \
                contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit) as result:
                runpy.run_path(str(script), run_name='__main__')
            self.assertEqual(result.exception.code, 2)
            popen.assert_not_called()
        self.assertEqual(stat.S_IMODE(real_target.stat().st_mode), 0o755)


if __name__ == '__main__':
    unittest.main()
