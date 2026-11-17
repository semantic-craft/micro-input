"""Unit tests for scripts/release-macos metadata validation, Mach-O path hygiene, and appcast checks."""
from __future__ import annotations

import importlib.machinery
import importlib.util
import os
from pathlib import Path
import plistlib
import stat
import tempfile
import unittest
from unittest.mock import patch

SCRIPT_PATH = Path(__file__).resolve().parents[1] / 'release-macos'
LOADER = importlib.machinery.SourceFileLoader('release_macos', str(SCRIPT_PATH))
SPEC = importlib.util.spec_from_loader('release_macos', LOADER)
assert SPEC is not None and SPEC.loader is not None
release_mod = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release_mod)


class ReleaseMacOSTests(unittest.TestCase):
    def _make_repo(self, root: Path, short_version: str = '0.5.0', build_version: str = '5') -> None:
        (root / 'LICENSE').write_text('MIT License\n', encoding='utf-8')
        notes = root / 'docs/releases/v0.5.0.md'
        notes.parent.mkdir(parents=True, exist_ok=True)
        notes.write_text('# Micro Input v0.5.0\n\n发行说明。\n', encoding='utf-8')
        plist_path = root / 'app/MicroInput/Info.plist'
        plist_path.parent.mkdir(parents=True, exist_ok=True)
        with plist_path.open('wb') as handle:
            plistlib.dump(
                {
                    'CFBundleIdentifier': 'craft.semantic.micro-input.dev',
                    'CFBundleShortVersionString': short_version,
                    'CFBundleVersion': build_version,
                    'SUFeedURL': 'https://github.com/semantic-craft/micro-input/releases/latest/download/appcast.xml',
                    'SUPublicEDKey': 'lwr8Uv2vLk5q4IOem6pjd3V5/EF9aHb2kx/4pzIfKJc=',
                },
                handle,
            )

    def test_release_auto_select_still_uses_the_first_developer_id(self):
        first = 'Developer ID Application: First Match (TEAMFIRST01)'
        later = 'Developer ID Application: Example (LQAVR62TK2)'
        output = f'  1) AAAA "{first}"\n  2) BBBB "{later}"\n     2 valid identities found\n'
        with patch.dict(os.environ, {'MI_RELEASE_SIGN_IDENTITY': ''}, clear=False), \
                patch.object(release_mod.subprocess, 'check_output', return_value=output):
            self.assertEqual(release_mod.resolve_release_signing_identity(), first)
        with patch.dict(os.environ, {'MI_RELEASE_SIGN_IDENTITY': later}, clear=False):
            self.assertEqual(release_mod.resolve_release_signing_identity(), later)

    def test_parse_args_enforces_tag_format_and_rejects_unknown_flags(self):
        args = release_mod.parse_args(['v0.5.0', '--publish', '--install'])
        self.assertEqual(args.tag, 'v0.5.0')
        self.assertTrue(args.publish)
        self.assertTrue(args.install)
        default_args = release_mod.parse_args(['v0.5.0'])
        self.assertFalse(default_args.publish)
        self.assertFalse(default_args.install)
        for bad in (['0.5.0'], ['v0.5'], ['v0.5.0', '--bogus']):
            with self.assertRaises(SystemExit):
                release_mod.parse_args(bad)

    def test_validate_release_metadata_matches_worktree_and_rejects_version_drift(self):
        repo_root = Path(__file__).resolve().parents[2]
        with (repo_root / 'app/MicroInput/Info.plist').open('rb') as handle:
            plist = plistlib.load(handle)
        expected_short = str(plist['CFBundleShortVersionString']).strip()
        expected_build = str(plist['CFBundleVersion']).strip()
        short_ver, build_ver = release_mod.validate_release_metadata(
            repo_root, f'v{expected_short}'
        )
        self.assertEqual(short_ver, expected_short)
        self.assertEqual(build_ver, expected_build)

        with tempfile.TemporaryDirectory(prefix='mi-release-meta-') as tmp:
            fake_root = Path(tmp).resolve()
            self._make_repo(fake_root, short_version='0.4.0', build_version='4')
            with self.assertRaises(ValueError) as ctx:
                release_mod.validate_release_metadata(fake_root, 'v0.5.0')
            self.assertIn('CFBundleShortVersionString=0.5.0', str(ctx.exception))

            self._make_repo(fake_root, short_version='0.5.0', build_version='five')
            with self.assertRaises(ValueError) as build_ctx:
                release_mod.validate_release_metadata(fake_root, 'v0.5.0')
            self.assertIn('CFBundleVersion must be a numeric build string', str(build_ctx.exception))

    def test_extract_unsafe_rpaths_and_user_path_detector(self):
        prefix = '/Us' + 'ers/builder/work/app/.build/out/Products/Release/PackageFrameworks'
        cryptexd_rpath = '/var/run/com.apple.security.cryptexd/codex.system/bootstrap/usr/lib/swift'
        xcode_rpath = '/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/macosx'
        sample_otool = f"""
Load command 18
          cmd LC_RPATH
      cmdsize 32
         path /usr/lib/swift (offset 12)
Load command 19
          cmd LC_RPATH
      cmdsize 128
         path {prefix} (offset 12)
Load command 20
          cmd LC_RPATH
      cmdsize 96
         path {cryptexd_rpath} (offset 12)
Load command 21
          cmd LC_RPATH
      cmdsize 128
         path {xcode_rpath} (offset 12)
Load command 22
          cmd LC_RPATH
      cmdsize 32
         path @loader_path (offset 12)
Load command 23
          cmd LC_RPATH
      cmdsize 48
         path @executable_path/../Frameworks (offset 12)
"""
        self.assertEqual(
            release_mod.extract_unsafe_rpaths(sample_otool),
            [prefix, cryptexd_rpath, xcode_rpath, '@loader_path'],
        )

        with tempfile.TemporaryDirectory(prefix='mi-release-bin-') as tmp:
            root = Path(tmp).resolve()
            clean_bin = root / 'MicroInputClean'
            clean_bin.write_bytes(b'\xcf\xfa\xed\xfe/workspace/src/main.rs\x00/Users/Shared/data\x00')
            release_mod.assert_binary_free_of_user_paths(clean_bin)

            dirty_bin = root / 'MicroInputDirty'
            dirty_bin.write_bytes(b'\xcf\xfa\xed\xfe' + b'/Us' + b'ers/builder/cargo/registry\x00')
            with self.assertRaises(RuntimeError):
                release_mod.assert_binary_free_of_user_paths(dirty_bin)

    def test_validate_appcast_xml_and_redact_sensitive(self):
        with tempfile.TemporaryDirectory(prefix='mi-release-appcast-') as tmp:
            appcast = Path(tmp).resolve() / 'appcast.xml'
            dl_url = 'https://github.com/semantic-craft/micro-input/releases/download/v0.5.0/Micro-Input-v0.5.0-macos-arm64.zip'
            appcast.write_text(
                f"""<?xml version="1.0" standalone="yes"?>
<rss xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle" version="2.0">
  <channel>
    <title>Micro Input</title>
    <item>
      <title>0.5.0</title>
      <sparkle:version>5</sparkle:version>
      <sparkle:shortVersionString>0.5.0</sparkle:shortVersionString>
      <enclosure url="{dl_url}" length="123456" type="application/octet-stream" sparkle:edSignature="base64sig=="/>
    </item>
  </channel>
</rss>
""",
                encoding='utf-8',
            )
            release_mod.validate_appcast_xml(appcast, '0.5.0', '5', dl_url)

            with self.assertRaises(ValueError):
                release_mod.validate_appcast_xml(appcast, '0.5.1', '5', dl_url)

        redacted = release_mod.redact_sensitive(
            'Signed by Developer ID Application: Jane Example (TEAM998877) 0123456789ABCDEF0123456789ABCDEF01234567 jane@example.com'
        )
        self.assertNotIn('Jane Example', redacted)
        self.assertNotIn('TEAM998877', redacted)
        self.assertNotIn('0123456789ABCDEF0123456789ABCDEF01234567', redacted)
        self.assertNotIn('jane@example.com', redacted)

    def test_redact_sensitive_masks_team_ids_key_paths_and_uuid_issuer_ids(self):
        sample = '\n'.join([
            '{"teamId": "TEAM123456", "team_id": "TEAM654321", "issuerId": "57246542-96fe-1a63-e053-0824d011072a", "keyId": "2X9R4HXF34"}',
            '<key>TeamIdentifier</key><string>TEAM778899</string>',
            'xcrun notarytool submit app.zip --team-id TEAM001122 --issuer 11223344-5566-7788-99aa-bbccddeeff00 --key-id 9A8B7C6D5E --key ~/.private_keys/AuthKey_9A8B7C6D5E.p8',
            'pkcs12=/Users/example/.private_keys/cert.p12',
        ])
        redacted = release_mod.redact_sensitive(sample)
        for secret in (
            'TEAM123456',
            'TEAM654321',
            'TEAM778899',
            'TEAM001122',
            '57246542-96fe-1a63-e053-0824d011072a',
            '11223344-5566-7788-99aa-bbccddeeff00',
            '2X9R4HXF34',
            '9A8B7C6D5E',
            '.private_keys',
            'AuthKey_9A8B7C6D5E.p8',
            'cert.p12',
            '/Users/example',
        ):
            self.assertNotIn(secret, redacted, f'Leaked {secret!r} in:\n{redacted}')

    def test_format_notary_failure_log_redacts_before_truncating_at_2000_chars(self):
        prefix = 'A' * 1990
        secret_tail = 'TEAM889900 SECRET_TOKEN_AFTER_2000_CHARS'
        formatted = release_mod.format_notary_failure_log(
            f'{prefix} --team-id {secret_tail}',
            None,
            ['SECRET_TOKEN_AFTER_2000_CHARS'],
        )
        self.assertLessEqual(len(formatted), 2000)
        self.assertNotIn('TEAM889900', formatted)
        self.assertNotIn('SECRET_TOKEN', formatted)

    def _write_notary_stub(self, path: Path, body: str) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(f'#!/bin/sh\n{body}\n', encoding='utf-8')
        path.chmod(path.stat().st_mode | stat.S_IXUSR)

    def test_resolve_notary_profile_requires_env_or_helper_without_legacy_default(self):
        legacy_default = 'micro-input' + '-notary'
        with tempfile.TemporaryDirectory(prefix='mi-notary-empty-') as tmp:
            fake_home = Path(tmp).resolve()
            with self.assertRaises(RuntimeError) as ctx:
                release_mod.resolve_notary_profile(env={'PATH': ''}, home=fake_home)
            msg = str(ctx.exception)
            self.assertIn('MI_NOTARY_PROFILE', msg)
            self.assertIn('NOTARY_PROFILE', msg)
            self.assertIn('macos-notary-profile', msg)
            self.assertNotIn(legacy_default, msg)

            blank_stub = fake_home / '.local/bin/macos-notary-profile'
            self._write_notary_stub(blank_stub, 'printf "   \\n"')
            with self.assertRaises(RuntimeError):
                release_mod.resolve_notary_profile(env={'PATH': ''}, home=fake_home)

            failing_stub = fake_home / '.local/bin/macos-notary-profile'
            self._write_notary_stub(failing_stub, 'echo "missing keychain item" >&2; exit 1')
            with self.assertRaises(RuntimeError) as fail_ctx:
                release_mod.resolve_notary_profile(env={'PATH': ''}, home=fake_home)
            self.assertIn('macos-notary-profile failed', str(fail_ctx.exception))

    def test_resolve_notary_profile_precedence_and_helper_stubs(self):
        with tempfile.TemporaryDirectory(prefix='mi-notary-stub-') as tmp:
            root = Path(tmp).resolve()
            path_bin = root / 'bin'
            fake_home = root / 'home'
            self._write_notary_stub(path_bin / 'macos-notary-profile', 'echo "stub-path-profile"')
            self._write_notary_stub(
                fake_home / '.local/bin/macos-notary-profile',
                'echo "stub-home-profile"',
            )

            self.assertEqual(
                release_mod.resolve_notary_profile(
                    env={
                        'MI_NOTARY_PROFILE': '  env-mi-profile  ',
                        'NOTARY_PROFILE': 'env-shared-profile',
                        'PATH': str(path_bin),
                    },
                    home=fake_home,
                ),
                'env-mi-profile',
            )
            self.assertEqual(
                release_mod.resolve_notary_profile(
                    env={
                        'MI_NOTARY_PROFILE': '   ',
                        'NOTARY_PROFILE': 'env-shared-profile',
                        'PATH': str(path_bin),
                    },
                    home=fake_home,
                ),
                'env-shared-profile',
            )
            self.assertEqual(
                release_mod.resolve_notary_profile(
                    env={'PATH': str(path_bin)},
                    home=fake_home,
                ),
                'stub-path-profile',
            )
            self.assertEqual(
                release_mod.resolve_notary_profile(
                    env={'PATH': ''},
                    home=fake_home,
                ),
                'stub-home-profile',
            )


if __name__ == '__main__':
    unittest.main()
