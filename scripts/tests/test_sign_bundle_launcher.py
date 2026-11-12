"""Unit and integration tests for scripts/sign_bundle.py inside-out bundle signing."""
from __future__ import annotations

import importlib.util
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
    shutil.copyfile('/usr/bin/true', dest)
    dest.chmod(0o755)


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

    def test_real_codesign_verify_deep_strict_survives_qa_copytree_and_outer_resign(self):
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
            subprocess.run(['codesign', '--force', '--sign', '-', '--timestamp=none', str(qa_app)], check=True)
            subprocess.run(['codesign', '--verify', '--deep', '--strict', str(qa_app)], check=True)


if __name__ == '__main__':
    unittest.main()
