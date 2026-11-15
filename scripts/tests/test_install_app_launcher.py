"""Deterministic unit tests for scripts/install-app Developer ID and TCC continuity gates."""
from __future__ import annotations

import importlib.machinery
import importlib.util
import io
import json
from pathlib import Path
import plistlib
import shutil
import signal
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import zipfile

SCRIPT_PATH = Path(__file__).resolve().parents[1] / 'install-app'
LOADER = importlib.machinery.SourceFileLoader('install_app', str(SCRIPT_PATH))
SPEC = importlib.util.spec_from_loader('install_app', LOADER)
assert SPEC is not None and SPEC.loader is not None
install_mod = importlib.util.module_from_spec(SPEC)
sys.modules['install_app'] = install_mod
SPEC.loader.exec_module(install_mod)

DEV_ID_DR = (
    'identifier "craft.semantic.micro-input.dev" and anchor apple generic and '
    'certificate leaf[subject.OU] = TEAM123456'
)
APPLE_DEV_DR = (
    'identifier "craft.semantic.micro-input.dev" and anchor apple generic and '
    'certificate leaf[subject.CN] = "Apple Development: dev@example.com (DEV111)"'
)
ADHOC_DR = 'cdhash H"0123456789abcdef0123456789abcdef01234567"'


def _make_fake_bundle(
    bundle_path: Path,
    *,
    bundle_id: str = 'craft.semantic.micro-input.dev',
    version: str = '10',
    authority: str = 'Authority=Developer ID Application: Example Corp (TEAM123456)',
    designated_req: str = DEV_ID_DR,
    verify_rc: int = 0,
    spctl_rc: int = 0,
) -> Path:
    contents = bundle_path / 'Contents'
    (contents / 'MacOS').mkdir(parents=True, exist_ok=True)
    (contents / 'MacOS/MicroInput').write_text('#!/bin/sh\n', encoding='utf-8')
    with (contents / 'Info.plist').open('wb') as handle:
        plistlib.dump(
            {
                'CFBundleIdentifier': bundle_id,
                'CFBundleExecutable': 'MicroInput',
                'CFBundleVersion': version,
                'TestAuthority': authority,
                'TestDesignatedReq': designated_req,
                'TestVerifyRc': verify_rc,
                'TestSpctlRc': spctl_rc,
            },
            handle,
        )
    return bundle_path


class FakeCommandRunner:
    def __init__(self, *, ps_lines: list[str] | None = None) -> None:
        self.calls: list[list[str]] = []
        self.ps_lines = list(ps_lines or [])
        self.killed: list[tuple[int, int]] = []

    def kill(self, pid: int, sig: int) -> None:
        self.killed.append((pid, sig))
        self.ps_lines = [line for line in self.ps_lines if not line.strip().startswith(f'{pid} ')]

    def _read_meta(self, bundle_path: Path) -> dict:
        with (bundle_path / 'Contents/Info.plist').open('rb') as handle:
            return plistlib.load(handle)

    def __call__(self, cmd: list[str]) -> subprocess.CompletedProcess[str]:
        self.calls.append(cmd)
        if cmd[:4] == ['codesign', '--verify', '--deep', '--strict']:
            meta = self._read_meta(Path(cmd[4]))
            rc = int(meta.get('TestVerifyRc', 0))
            return subprocess.CompletedProcess(cmd, rc, '', '' if rc == 0 else 'invalid signature')

        if cmd[:2] == ['codesign', '-dvv']:
            meta = self._read_meta(Path(cmd[2]))
            authority = str(meta.get('TestAuthority', ''))
            return subprocess.CompletedProcess(cmd, 0, '', f'Executable={cmd[2]}/Contents/MacOS/MicroInput\n{authority}\n')

        if cmd[:3] == ['spctl', '--assess', '--type']:
            meta = self._read_meta(Path(cmd[4]))
            rc = int(meta.get('TestSpctlRc', 0))
            return subprocess.CompletedProcess(cmd, rc, '', '' if rc == 0 else 'rejected by Gatekeeper')

        if cmd[:4] == ['codesign', '-d', '-r', '-']:
            meta = self._read_meta(Path(cmd[4]))
            dr = str(meta.get('TestDesignatedReq', ''))
            return subprocess.CompletedProcess(
                cmd,
                0,
                f'designated => {dr}\n' if dr else '',
                f'Executable={cmd[4]}/Contents/MacOS/MicroInput\n',
            )

        if cmd[:3] == ['/usr/bin/ditto', '-x', '-k']:
            with zipfile.ZipFile(cmd[3], 'r') as zf:
                zf.extractall(cmd[4])
            return subprocess.CompletedProcess(cmd, 0, '', '')

        if cmd[0] == '/usr/bin/ditto':
            shutil.copytree(cmd[1], cmd[2], symlinks=True)
            return subprocess.CompletedProcess(cmd, 0, '', '')

        if cmd[:2] == ['ps', '-ax']:
            return subprocess.CompletedProcess(cmd, 0, '\n'.join(self.ps_lines) + '\n', '')

        return subprocess.CompletedProcess(cmd, 0, '', '')


def _read_bundle_version(bundle_path: Path) -> str:
    with (bundle_path / 'Contents/Info.plist').open('rb') as handle:
        return str(plistlib.load(handle)['CFBundleVersion'])


class InstallAppTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory(prefix='mi-install-test-')
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name).resolve()
        self.fake_lsregister = self.root / 'lsregister'
        self.fake_lsregister.write_text('#!/bin/sh\n', encoding='utf-8')

    def test_rejects_dev_and_adhoc_bundles_without_allow_dev_leaving_dest_and_tcc_untouched(self):
        dest = _make_fake_bundle(self.root / 'Applications/Micro Input.app', version='1')
        dev_src = _make_fake_bundle(
            self.root / 'src-dev/MicroInput.app',
            version='2',
            authority='Authority=Apple Development: dev@example.com (DEV111)',
            designated_req=APPLE_DEV_DR,
        )
        adhoc_src = _make_fake_bundle(
            self.root / 'src-adhoc/MicroInput.app',
            version='3',
            authority='Signature=adhoc',
            designated_req=ADHOC_DR,
        )

        for src in (dev_src, adhoc_src):
            runner = FakeCommandRunner()
            with self.assertRaises(install_mod.InstallerError) as ctx:
                install_mod.install_app(
                    source=src,
                    dest=dest,
                    allow_dev=False,
                    runner=runner,
                    kill_fn=runner.kill,
                    sleep_fn=lambda _: None,
                    lsregister_path=self.fake_lsregister,
                )
            self.assertIn('--allow-dev', str(ctx.exception))
            self.assertEqual(_read_bundle_version(dest), '1')
            self.assertFalse(any(c[0] == 'tccutil' for c in runner.calls))
            self.assertFalse(any(c[0] == '/usr/bin/ditto' for c in runner.calls))

    def test_preserves_tcc_when_developer_id_designated_requirements_match(self):
        dest = _make_fake_bundle(self.root / 'Applications/Micro Input.app', version='1', designated_req=DEV_ID_DR)
        src = _make_fake_bundle(self.root / 'stage/Micro Input.app', version='2', designated_req=DEV_ID_DR)
        runner = FakeCommandRunner(
            ps_lines=[
                f'101 {dest}/Contents/MacOS/MicroInput',
                f'202 {self.root}/target/MicroInput.app/Contents/MacOS/MicroInput',
            ]
        )

        report = install_mod.install_app(
            source=src,
            dest=dest,
            unregister_source=True,
            launch=True,
            runner=runner,
            kill_fn=runner.kill,
            sleep_fn=lambda _: None,
            lsregister_path=self.fake_lsregister,
        )
        self.assertEqual(report.status, 'installed')
        self.assertTrue(report.designated_requirement_matched)
        self.assertFalse(report.tcc_reset_required)
        self.assertFalse(report.tcc_reset_performed)
        self.assertEqual(report.terminated_pids, [101])
        self.assertEqual(runner.killed, [(101, signal.SIGTERM)])
        self.assertEqual(_read_bundle_version(dest), '2')
        self.assertFalse(any(c[0] == 'tccutil' for c in runner.calls))
        self.assertIn([str(self.fake_lsregister), '-f', str(dest)], runner.calls)
        self.assertTrue(any('--unregister-only' in c and str(src) in c for c in runner.calls))
        self.assertIn(['/usr/bin/open', str(dest)], runner.calls)
        leftover_siblings = [p for p in dest.parent.iterdir() if p.name.startswith(f'.{dest.name}.')]
        self.assertEqual(leftover_siblings, [])

    def test_resets_tcc_when_designated_requirements_mismatch_in_either_direction(self):
        dest_dev = _make_fake_bundle(
            self.root / 'Applications/Micro Input.app',
            version='1',
            authority='Authority=Apple Development: dev@example.com (DEV111)',
            designated_req=APPLE_DEV_DR,
        )
        src_rel = _make_fake_bundle(self.root / 'stage/Micro Input.app', version='2', designated_req=DEV_ID_DR)
        runner_up = FakeCommandRunner()
        report_up = install_mod.install_app(
            source=src_rel,
            dest=dest_dev,
            runner=runner_up,
            kill_fn=runner_up.kill,
            sleep_fn=lambda _: None,
            lsregister_path=self.fake_lsregister,
        )
        self.assertFalse(report_up.designated_requirement_matched)
        self.assertTrue(report_up.tcc_reset_required)
        self.assertTrue(report_up.tcc_reset_performed)
        self.assertIn(
            ['tccutil', 'reset', 'Accessibility', 'craft.semantic.micro-input.dev'],
            runner_up.calls,
        )
        self.assertEqual(_read_bundle_version(dest_dev), '2')

        src_dev = _make_fake_bundle(
            self.root / 'target/MicroInput.app',
            version='3',
            authority='Authority=Apple Development: dev@example.com (DEV111)',
            designated_req=APPLE_DEV_DR,
        )
        runner_down = FakeCommandRunner()
        report_down = install_mod.install_app(
            source=src_dev,
            dest=dest_dev,
            allow_dev=True,
            runner=runner_down,
            kill_fn=runner_down.kill,
            sleep_fn=lambda _: None,
            lsregister_path=self.fake_lsregister,
        )
        self.assertFalse(report_down.designated_requirement_matched)
        self.assertTrue(report_down.tcc_reset_required)
        self.assertTrue(report_down.tcc_reset_performed)
        self.assertIn(
            ['tccutil', 'reset', 'Accessibility', 'craft.semantic.micro-input.dev'],
            runner_down.calls,
        )
        self.assertEqual(_read_bundle_version(dest_dev), '3')

    def test_zip_archive_install_and_dry_run_mode(self):
        src_for_zip = _make_fake_bundle(self.root / 'zip-src/Micro Input.app', version='9', designated_req=DEV_ID_DR)
        archive_path = self.root / 'Micro-Input-v0.9.0-macos-arm64.zip'
        with zipfile.ZipFile(archive_path, 'w') as zf:
            for item in src_for_zip.rglob('*'):
                if item.is_file():
                    zf.write(item, item.relative_to(src_for_zip.parent))

        dest = _make_fake_bundle(
            self.root / 'Applications/Micro Input.app',
            version='1',
            authority='Signature=adhoc',
            designated_req=ADHOC_DR,
        )

        dry_runner = FakeCommandRunner(ps_lines=[f'999 {dest}/Contents/MacOS/MicroInput'])
        dry_report = install_mod.install_app(
            zip_path=archive_path,
            dest=dest,
            dry_run=True,
            runner=dry_runner,
            kill_fn=dry_runner.kill,
            sleep_fn=lambda _: None,
            lsregister_path=self.fake_lsregister,
        )
        self.assertEqual(dry_report.status, 'dry_run')
        self.assertTrue(dry_report.tcc_reset_required)
        self.assertFalse(dry_report.tcc_reset_performed)
        self.assertEqual(dry_runner.killed, [])
        self.assertFalse(any(c[0] == 'tccutil' for c in dry_runner.calls))
        self.assertEqual(_read_bundle_version(dest), '1')

        real_runner = FakeCommandRunner()
        real_report = install_mod.install_app(
            zip_path=archive_path,
            dest=dest,
            runner=real_runner,
            kill_fn=real_runner.kill,
            sleep_fn=lambda _: None,
            lsregister_path=self.fake_lsregister,
        )
        self.assertEqual(real_report.status, 'installed')
        self.assertTrue(real_report.tcc_reset_performed)
        self.assertEqual(_read_bundle_version(dest), '9')

    def test_rejects_wrong_bundle_id_and_spctl_failure(self):
        dest = self.root / 'Applications/Micro Input.app'
        bad_id = _make_fake_bundle(self.root / 'bad-id/MicroInput.app', bundle_id='com.example.other')
        runner = FakeCommandRunner()
        with self.assertRaises(install_mod.InstallerError):
            install_mod.install_app(source=bad_id, dest=dest, runner=runner, lsregister_path=self.fake_lsregister)
        self.assertFalse(dest.exists())

        bad_spctl = _make_fake_bundle(self.root / 'bad-spctl/MicroInput.app', spctl_rc=1)
        with self.assertRaises(install_mod.InstallerError):
            install_mod.install_app(source=bad_spctl, dest=dest, runner=runner, lsregister_path=self.fake_lsregister)
        self.assertFalse(dest.exists())

    def test_cli_json_output_and_exit_codes(self):
        src = _make_fake_bundle(self.root / 'src/Micro Input.app', version='5')
        dest = self.root / 'Applications/Micro Input.app'
        runner = FakeCommandRunner()
        stdout_buf = io.StringIO()
        with patch.object(install_mod, 'default_runner', runner), \
             patch.object(install_mod, 'LSREGISTER', self.fake_lsregister), \
             patch.object(sys, 'stdout', stdout_buf):
            rc = install_mod.main([str(src), '--dest', str(dest), '--dry-run', '--json'])
        self.assertEqual(rc, 0)
        payload = json.loads(stdout_buf.getvalue())
        self.assertEqual(payload['status'], 'dry_run')
        self.assertFalse(payload['tcc_reset_required'])

    def test_tccutil_failure_before_swap_leaves_dest_unchanged_and_retry_is_idempotent(self):
        dest = _make_fake_bundle(
            self.root / 'Applications/Micro Input.app',
            version='1',
            authority='Authority=Apple Development: dev@example.com (DEV111)',
            designated_req=APPLE_DEV_DR,
        )
        src = _make_fake_bundle(self.root / 'stage/Micro Input.app', version='2', designated_req=DEV_ID_DR)

        fail_tcc = {'fail': True}

        class RetryRunner(FakeCommandRunner):
            def __call__(self, cmd: list[str]) -> subprocess.CompletedProcess[str]:
                if cmd[0] == 'tccutil' and fail_tcc['fail']:
                    self.calls.append(cmd)
                    return subprocess.CompletedProcess(cmd, 1, '', 'transient tccutil error')
                return super().__call__(cmd)

        runner1 = RetryRunner()
        with self.assertRaises(install_mod.InstallerError):
            install_mod.install_app(
                source=src,
                dest=dest,
                runner=runner1,
                kill_fn=runner1.kill,
                sleep_fn=lambda _: None,
                lsregister_path=self.fake_lsregister,
            )
        self.assertEqual(_read_bundle_version(dest), '1')
        self.assertFalse(any(c[0] == '/usr/bin/ditto' for c in runner1.calls))

        fail_tcc['fail'] = False
        runner2 = RetryRunner()
        report = install_mod.install_app(
            source=src,
            dest=dest,
            runner=runner2,
            kill_fn=runner2.kill,
            sleep_fn=lambda _: None,
            lsregister_path=self.fake_lsregister,
        )
        self.assertTrue(report.tcc_reset_performed)
        self.assertEqual(_read_bundle_version(dest), '2')
        tcc_idx = next(i for i, c in enumerate(runner2.calls) if c[0] == 'tccutil')
        ditto_idx = next(i for i, c in enumerate(runner2.calls) if c[0] == '/usr/bin/ditto')
        self.assertLess(tcc_idx, ditto_idx)

    def test_backup_cleanup_resilience_and_json_errors_on_corrupt_plist_or_oserror(self):
        dest = _make_fake_bundle(self.root / 'Applications/Micro Input.app', version='1', designated_req=DEV_ID_DR)
        src = _make_fake_bundle(self.root / 'stage/Micro Input.app', version='2', designated_req=DEV_ID_DR)

        rmtree_flags: list[bool] = []
        real_rmtree = shutil.rmtree

        def checked_rmtree(path, ignore_errors=False, *args, **kwargs):
            if Path(path).name.startswith(f'.{dest.name}.backup.'):
                rmtree_flags.append(bool(ignore_errors))
            return real_rmtree(path, ignore_errors=ignore_errors, *args, **kwargs)

        runner = FakeCommandRunner()
        with patch.object(install_mod.shutil, 'rmtree', side_effect=checked_rmtree):
            report = install_mod.install_app(
                source=src,
                dest=dest,
                runner=runner,
                kill_fn=runner.kill,
                sleep_fn=lambda _: None,
                lsregister_path=self.fake_lsregister,
            )
        self.assertEqual(report.status, 'installed')
        self.assertEqual(rmtree_flags, [True])
        self.assertIn([str(self.fake_lsregister), '-f', str(dest)], runner.calls)

        corrupt_src = self.root / 'corrupt/MicroInput.app'
        (corrupt_src / 'Contents').mkdir(parents=True)
        (corrupt_src / 'Contents/Info.plist').write_bytes(b'not a valid plist')
        out_corrupt = io.StringIO()
        with patch.object(install_mod, 'default_runner', runner), \
             patch.object(sys, 'stdout', out_corrupt):
            rc_corrupt = install_mod.main([str(corrupt_src), '--dest', str(dest), '--json'])
        self.assertEqual(rc_corrupt, 1)
        corrupt_payload = json.loads(out_corrupt.getvalue())
        self.assertEqual(corrupt_payload['status'], 'error')
        self.assertIn('Info.plist', corrupt_payload['error'])

        out_perm = io.StringIO()
        with patch.object(install_mod, 'default_runner', runner), \
             patch.object(install_mod, 'LSREGISTER', self.fake_lsregister), \
             patch.object(install_mod.os, 'rename', side_effect=PermissionError('read-only /Applications')), \
             patch.object(sys, 'stdout', out_perm):
            rc_perm = install_mod.main([str(src), '--dest', str(dest), '--json'])
        self.assertEqual(rc_perm, 1)
        perm_payload = json.loads(out_perm.getvalue())
        self.assertEqual(perm_payload['status'], 'error')
        self.assertIn('read-only', perm_payload['error'])

    def test_terminates_nested_helpers_inside_dest_without_touching_sibling_apps(self):
        dest = _make_fake_bundle(self.root / 'Applications/Micro Input.app', version='1', designated_req=DEV_ID_DR)
        src = _make_fake_bundle(self.root / 'stage/Micro Input.app', version='2', designated_req=DEV_ID_DR)
        runner = FakeCommandRunner(
            ps_lines=[
                f'101 {dest}/Contents/MacOS/MicroInput',
                f'102 {dest}/Contents/Frameworks/Sparkle.framework/Versions/B/Updater.app/Contents/MacOS/Updater',
                f'202 {self.root}/Applications/Micro Input Extra.app/Contents/MacOS/MicroInput',
                f'303 {self.root}/target/MicroInput.app/Contents/MacOS/MicroInput',
            ]
        )
        report = install_mod.install_app(
            source=src,
            dest=dest,
            runner=runner,
            kill_fn=runner.kill,
            sleep_fn=lambda _: None,
            lsregister_path=self.fake_lsregister,
        )
        self.assertEqual(report.terminated_pids, [101, 102])
        self.assertEqual(runner.killed, [(101, signal.SIGTERM), (102, signal.SIGTERM)])


if __name__ == '__main__':
    unittest.main()
