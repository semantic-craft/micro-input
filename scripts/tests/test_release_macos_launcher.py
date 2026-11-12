"""Unit tests for scripts/release-macos metadata validation, Mach-O path hygiene, and appcast checks."""
from __future__ import annotations

import importlib.machinery
import importlib.util
from pathlib import Path
import plistlib
import tempfile
import unittest

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

    def test_parse_args_enforces_tag_format_and_rejects_unknown_flags(self):
        args = release_mod.parse_args(['v0.5.0', '--publish'])
        self.assertEqual(args.tag, 'v0.5.0')
        self.assertTrue(args.publish)
        for bad in (['0.5.0'], ['v0.5'], ['v0.5.0', '--bogus']):
            with self.assertRaises(SystemExit):
                release_mod.parse_args(bad)

    def test_validate_release_metadata_matches_worktree_and_rejects_version_drift(self):
        repo_root = Path(__file__).resolve().parents[2]
        short_ver, build_ver = release_mod.validate_release_metadata(repo_root, 'v0.5.0')
        self.assertEqual(short_ver, '0.5.0')
        self.assertEqual(build_ver, '5')

        with tempfile.TemporaryDirectory(prefix='mi-release-meta-') as tmp:
            fake_root = Path(tmp).resolve()
            self._make_repo(fake_root, short_version='0.4.0', build_version='4')
            with self.assertRaises(ValueError) as ctx:
                release_mod.validate_release_metadata(fake_root, 'v0.5.0')
            self.assertIn('CFBundleShortVersionString=0.5.0', str(ctx.exception))

    def test_extract_unsafe_rpaths_and_user_path_detector(self):
        prefix = '/Us' + 'ers/builder/work/app/.build/out/Products/Release/PackageFrameworks'
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
      cmdsize 48
         path @executable_path/../Frameworks (offset 12)
"""
        self.assertEqual(release_mod.extract_unsafe_rpaths(sample_otool), [prefix])

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


if __name__ == '__main__':
    unittest.main()
