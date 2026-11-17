"""Unit and integration tests for scripts/sign_bundle.py inside-out bundle signing."""
from __future__ import annotations

import importlib.util
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPT_PATH = Path(__file__).resolve().parents[1] / 'sign_bundle.py'
SPEC = importlib.util.spec_from_file_location('sign_bundle', SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
sign_bundle_mod = importlib.util.module_from_spec(SPEC)
sys.modules['sign_bundle'] = sign_bundle_mod
SPEC.loader.exec_module(sign_bundle_mod)


def _write_plist(path: Path, bundle_id: str, executable: str, pkg_type: str = 'APPL') -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        'CFBundleIdentifier': bundle_id,
        'CFBundleName': executable,
        'CFBundleExecutable': executable,
        'CFBundlePackageType': pkg_type,
        'CFBundleShortVersionString': '0.5.0',
        'CFBundleVersion': '5',
    }
    with path.open('wb') as handle:
        plistlib.dump(payload, handle)


def _copy_macho(dest: Path) -> None:
    dest.parent.mkdir(parents=True, exist_ok=True)
    true_bin = Path('/usr/bin/true')
    magic = b''
    if true_bin.is_file():
        with true_bin.open('rb') as handle:
            magic = handle.read(4)
    if magic in sign_bundle_mod.MACHO_MAGICS:
        shutil.copyfile(true_bin, dest)
    else:
        # Host /usr/bin/true is not Mach-O (Linux). A magic header is enough for
        # collect_sign_targets; real codesign is skipped when the tool is absent.
        dest.write_bytes(b'\xcf\xfa\xed\xfe' + b'\x00' * 28)
    dest.chmod(0o755)


def _record_codesign(commands: list[list[str]]):
    def fake_run(cmd, **kwargs):
        commands.append([str(part) for part in cmd])
        return subprocess.CompletedProcess(args=cmd, returncode=0, stdout='', stderr='')

    return fake_run


class SignBundleTests(unittest.TestCase):
    def _build_nested_bundle(self, root: Path) -> Path:
        app = root / 'MicroInput.app'
        _write_plist(app / 'Contents/Info.plist', 'craft.semantic.micro-input.dev', 'MicroInput', 'APPL')
        _copy_macho(app / 'Contents/MacOS/MicroInput')

        fw = app / 'Contents/Frameworks/Sparkle.framework'
        ver_b = fw / 'Versions/B'
        ver_b.mkdir(parents=True)
        (fw / 'Versions/Current').symlink_to('B')

        _copy_macho(ver_b / 'Sparkle')
        _copy_macho(ver_b / 'Autoupdate')

        res_dir = ver_b / 'Resources'
        res_dir.mkdir(parents=True)
        with (res_dir / 'Info.plist').open('wb') as handle:
            plistlib.dump(
                {
                    'CFBundleIdentifier': 'org.sparkle-project.Sparkle',
                    'CFBundleName': 'Sparkle',
                    'CFBundleExecutable': 'Sparkle',
                    'CFBundlePackageType': 'FMWK',
                    'CFBundleShortVersionString': '2.9.4',
                    'CFBundleVersion': '2094',
                },
                handle,
            )

        updater_app = ver_b / 'Updater.app'
        _write_plist(updater_app / 'Contents/Info.plist', 'org.sparkle-project.Sparkle.Updater', 'Updater', 'APPL')
        _copy_macho(updater_app / 'Contents/MacOS/Updater')

        downloader_xpc = ver_b / 'XPCServices/Downloader.xpc'
        _write_plist(
            downloader_xpc / 'Contents/Info.plist',
            'org.sparkle-project.Sparkle.Downloader',
            'Downloader',
            'XPC!',
        )
        _copy_macho(downloader_xpc / 'Contents/MacOS/Downloader')

        installer_xpc = ver_b / 'XPCServices/Installer.xpc'
        _write_plist(
            installer_xpc / 'Contents/Info.plist',
            'org.sparkle-project.Sparkle.Installer',
            'Installer',
            'XPC!',
        )
        _copy_macho(installer_xpc / 'Contents/MacOS/Installer')

        (fw / 'Sparkle').symlink_to('Versions/Current/Sparkle')
        (fw / 'Resources').symlink_to('Versions/Current/Resources')
        (fw / 'Autoupdate').symlink_to('Versions/Current/Autoupdate')
        (fw / 'Updater.app').symlink_to('Versions/Current/Updater.app')
        (fw / 'XPCServices').symlink_to('Versions/Current/XPCServices')

        return app

    def test_collect_sign_targets_orders_deepest_nested_code_before_framework_and_outer_app(self):
        with tempfile.TemporaryDirectory(prefix='mi-sign-order-') as tmp:
            app = self._build_nested_bundle(Path(tmp).resolve())
            targets = sign_bundle_mod.collect_sign_targets(app)
            rels = [str(p.relative_to(app)) for p in targets]
            downloader_rel = 'Contents/Frameworks/Sparkle.framework/Versions/B/XPCServices/Downloader.xpc'
            installer_rel = 'Contents/Frameworks/Sparkle.framework/Versions/B/XPCServices/Installer.xpc'
            framework_rel = 'Contents/Frameworks/Sparkle.framework'
            self.assertEqual(
                rels,
                [
                    downloader_rel,
                    installer_rel,
                    'Contents/Frameworks/Sparkle.framework/Versions/B/Autoupdate',
                    'Contents/Frameworks/Sparkle.framework/Versions/B/Updater.app',
                    framework_rel,
                    '.',
                ],
            )
            for xpc_rel in (downloader_rel, installer_rel):
                self.assertLess(rels.index(xpc_rel), rels.index(framework_rel))
                self.assertLess(rels.index(xpc_rel), rels.index('.'))

    def test_rejects_materialized_or_broken_framework_symlinks(self):
        with tempfile.TemporaryDirectory(prefix='mi-sign-symlink-') as tmp:
            root = Path(tmp).resolve()
            app = self._build_nested_bundle(root)
            current = app / 'Contents/Frameworks/Sparkle.framework/Versions/Current'
            current.unlink()
            shutil.copytree(app / 'Contents/Frameworks/Sparkle.framework/Versions/B', current, symlinks=True)
            with self.assertRaises(ValueError) as ctx:
                sign_bundle_mod.collect_sign_targets(app)
            self.assertIn('Versions/Current', str(ctx.exception))

        with tempfile.TemporaryDirectory(prefix='mi-sign-broken-') as tmp:
            root = Path(tmp).resolve()
            app = self._build_nested_bundle(root)
            top_link = app / 'Contents/Frameworks/Sparkle.framework/Autoupdate'
            top_link.unlink()
            _copy_macho(top_link)
            with self.assertRaises(ValueError) as ctx:
                sign_bundle_mod.collect_sign_targets(app)
            self.assertIn('Autoupdate', str(ctx.exception))

    def test_release_mode_rejects_adhoc_and_redacts_identity_on_failure(self):
        with tempfile.TemporaryDirectory(prefix='mi-sign-redact-') as tmp:
            app = self._build_nested_bundle(Path(tmp).resolve())
            with self.assertRaises(ValueError):
                sign_bundle_mod.sign_bundle(app, mode='release', identity='-')

            fake_id = 'Developer ID Application: Example Corp (ABCDE12345)'
            failed = subprocess.CompletedProcess(
                args=['codesign'],
                returncode=1,
                stdout='',
                stderr=f'{fake_id} 0123456789ABCDEF0123456789ABCDEF01234567: errSecInternalComponent',
            )
            with patch('subprocess.run', return_value=failed):
                with self.assertRaises(RuntimeError) as ctx:
                    sign_bundle_mod.sign_bundle(app, mode='release', identity=fake_id)
            msg = str(ctx.exception)
            self.assertNotIn('Example Corp', msg)
            self.assertNotIn('ABCDE12345', msg)
            self.assertNotIn('0123456789ABCDEF0123456789ABCDEF01234567', msg)
            self.assertIn('<REDACTED_SIGNING_IDENTITY>', msg)

    def test_dev_discovery_prefers_developer_id_team_and_keeps_env_override(self):
        preferred = 'Developer ID Application: Example (LQAVR62TK2)'
        other_dev_id = 'Developer ID Application: Other Team (TEAMOTHER1)'
        apple_dev = 'Apple Development: Example (B6NUMVUKU7)'
        keychain = '\n'.join([
            f'  1) AAAA "{apple_dev}"',
            f'  2) BBBB "{other_dev_id}"',
            f'  3) CCCC "{preferred}"',
            '     3 valid identities found',
        ])
        self.assertEqual(
            sign_bundle_mod._select_dev_identity(sign_bundle_mod._identity_names(keychain)),
            preferred,
        )
        self.assertEqual(
            sign_bundle_mod._select_dev_identity([other_dev_id, apple_dev]),
            other_dev_id,
        )
        self.assertEqual(sign_bundle_mod._select_dev_identity([apple_dev]), apple_dev)
        self.assertEqual(sign_bundle_mod._select_dev_identity([]), '-')

        with patch.dict(os.environ, {'MI_CODESIGN_IDENTITY': 'Apple Development: Override (DEV1111111)'}, clear=False), \
                patch('subprocess.check_output', side_effect=AssertionError('keychain must not be read')):
            self.assertEqual(
                sign_bundle_mod._discover_dev_identity(),
                'Apple Development: Override (DEV1111111)',
            )
        with patch.dict(os.environ, {'MI_CODESIGN_IDENTITY': ''}, clear=False), \
                patch('subprocess.check_output', return_value=keychain):
            self.assertEqual(sign_bundle_mod._discover_dev_identity(), preferred)
        with patch.dict(os.environ, {'MI_CODESIGN_IDENTITY': ''}, clear=False), \
                patch('subprocess.check_output', side_effect=FileNotFoundError):
            self.assertEqual(sign_bundle_mod._discover_dev_identity(), '-')

    def test_developer_id_reuses_release_hardened_runtime_without_deep(self):
        identity = 'Developer ID Application: Example (LQAVR62TK2)'
        with tempfile.TemporaryDirectory(prefix='mi-sign-devid-') as tmp:
            app = self._build_nested_bundle(Path(tmp).resolve())
            dev_cmds: list[list[str]] = []
            release_cmds: list[list[str]] = []
            with patch('subprocess.run', side_effect=_record_codesign(dev_cmds)):
                dev_targets = sign_bundle_mod.sign_bundle(app, mode='dev', identity=identity)
            with patch('subprocess.run', side_effect=_record_codesign(release_cmds)):
                release_targets = sign_bundle_mod.sign_bundle(app, mode='release', identity=identity)

            self.assertEqual(dev_cmds, release_cmds)
            self.assertEqual(dev_targets, release_targets)
            self.assertGreater(len(dev_cmds), 1)
            signed = []
            for cmd in dev_cmds:
                self.assertNotIn('--deep', cmd)
                self.assertNotIn('--timestamp=none', cmd)
                self.assertEqual(cmd[cmd.index('--options') + 1], 'runtime')
                self.assertIn('--timestamp', cmd)
                self.assertEqual(cmd[cmd.index('--sign') + 1], identity)
                target_name = Path(cmd[-1]).name
                if target_name == 'Downloader.xpc':
                    self.assertIn('--preserve-metadata=entitlements', cmd)
                else:
                    self.assertNotIn('--preserve-metadata=entitlements', cmd)
                signed.append(target_name)
            self.assertLess(signed.index('Downloader.xpc'), signed.index('Sparkle.framework'))
            self.assertLess(signed.index('Installer.xpc'), signed.index('Sparkle.framework'))
            self.assertLess(signed.index('Autoupdate'), signed.index('Sparkle.framework'))
            self.assertLess(signed.index('Updater.app'), signed.index('Sparkle.framework'))
            self.assertEqual(signed[-1], app.name)
            self.assertLess(signed.index('Sparkle.framework'), len(signed) - 1)

    def test_apple_development_and_adhoc_keep_timestamp_none(self):
        with tempfile.TemporaryDirectory(prefix='mi-sign-devcert-') as tmp:
            app = self._build_nested_bundle(Path(tmp).resolve())
            for identity in ('Apple Development: Example (B6NUMVUKU7)', '-'):
                commands: list[list[str]] = []
                with patch('subprocess.run', side_effect=_record_codesign(commands)):
                    sign_bundle_mod.sign_bundle(app, mode='dev', identity=identity)
                self.assertGreater(len(commands), 1)
                for cmd in commands:
                    self.assertIn('--timestamp=none', cmd)
                    self.assertNotIn('--options', cmd)
                    self.assertNotIn('--deep', cmd)
                    self.assertNotIn('--preserve-metadata=entitlements', cmd)
                    self.assertEqual(cmd[cmd.index('--sign') + 1], identity)

    def test_qa_plist_rewrite_resigns_nested_sparkle_and_outer_app_with_one_identity(self):
        identity = 'Developer ID Application: Example (LQAVR62TK2)'
        with tempfile.TemporaryDirectory(prefix='mi-sign-qa-') as tmp:
            root = Path(tmp).resolve()
            source_app = self._build_nested_bundle(root)
            qa_app = root / 'Micro Input QA.app'
            shutil.copytree(source_app, qa_app, symlinks=True)
            info_path = qa_app / 'Contents/Info.plist'
            with info_path.open('rb') as handle:
                info = plistlib.load(handle)
            info['CFBundleIdentifier'] = 'com.semanticcraft.microinput.qa'
            info['SUEnableAutomaticChecks'] = False
            with info_path.open('wb') as handle:
                plistlib.dump(info, handle)

            commands: list[list[str]] = []
            with patch.dict(os.environ, {'MI_CODESIGN_IDENTITY': identity}, clear=False), \
                    patch('subprocess.run', side_effect=_record_codesign(commands)):
                sign_bundle_mod.sign_bundle(qa_app, mode='dev')

            identities = {cmd[cmd.index('--sign') + 1] for cmd in commands}
            self.assertEqual(identities, {identity})
            rels = []
            for cmd in commands:
                target = Path(cmd[-1])
                rels.append('.' if target == qa_app else str(target.relative_to(qa_app)))
            self.assertIn('.', rels)
            self.assertTrue(any(rel.endswith('Downloader.xpc') for rel in rels))
            self.assertTrue(any(rel.endswith('Installer.xpc') for rel in rels))
            self.assertTrue(any(rel.endswith('Autoupdate') for rel in rels))
            self.assertTrue(any(rel.endswith('Updater.app') for rel in rels))
            self.assertIn('Contents/Frameworks/Sparkle.framework', rels)
            self.assertNotIn('-', identities)

    @unittest.skipUnless(shutil.which('codesign'), 'requires macOS codesign')
    def test_real_codesign_verify_deep_strict_after_consistent_inside_out_resign(self):
        with tempfile.TemporaryDirectory(prefix='mi-sign-real-') as tmp:
            root = Path(tmp).resolve()
            source_app = self._build_nested_bundle(root)
            sign_bundle_mod.sign_bundle(source_app, mode='dev', identity='-')
            subprocess.run(['codesign', '--verify', '--deep', '--strict', str(source_app)], check=True)

            qa_app = root / 'Micro Input QA.app'
            shutil.copytree(source_app, qa_app, symlinks=True)
            info_path = qa_app / 'Contents/Info.plist'
            with info_path.open('rb') as handle:
                info = plistlib.load(handle)
            info['CFBundleIdentifier'] = 'com.semanticcraft.microinput.qa'
            info['SUEnableAutomaticChecks'] = False
            with info_path.open('wb') as handle:
                plistlib.dump(info, handle)
            sign_bundle_mod.sign_bundle(qa_app, mode='dev', identity='-')
            subprocess.run(['codesign', '--verify', '--deep', '--strict', str(qa_app)], check=True)


if __name__ == '__main__':
    unittest.main()
