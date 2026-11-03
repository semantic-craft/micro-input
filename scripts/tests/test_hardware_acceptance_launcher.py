"""Preparation must never turn imported bytes or a dry run into hardware acceptance."""
import contextlib
import importlib.machinery
import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
loader = importlib.machinery.SourceFileLoader('hardware_kit', str(ROOT / 'scripts/prepare-hardware-acceptance'))
spec = importlib.util.spec_from_loader(loader.name, loader)
kit = importlib.util.module_from_spec(spec)
loader.exec_module(kit)


class PreparationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.output = self.root / 'kit'

    def call(self, *args):
        out = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(io.StringIO()):
            code = kit.main(list(map(str, args)))
        return code, json.loads(out.getvalue()) if out.getvalue() else None

    def prepare(self, *extra):
        code, result = self.call('prepare', '--output', self.output, *extra)
        self.assertEqual(code, 0)
        self.assertEqual(result['device_io'], 'NOT_RUN')

    def source(self, name, data):
        path = self.root / name
        path.write_bytes(data)
        return path

    def test_plan_has_one_new_temp_target_and_no_config_or_lighting_operation(self):
        # Inherited old probe authorization cannot acquire execution authority here.
        with patch.dict(os.environ, {'MI_PROBE_A_OPERATOR': 'I_AUTHORIZE_EXACT_TEMPFILE_A'}), patch('subprocess.Popen', side_effect=AssertionError('no child process')), patch('socket.socket', side_effect=AssertionError('no network')):
            self.prepare()
            manifest = kit.verify(self.output)
        plan = json.loads((self.output / 'plan.json').read_text())
        self.assertEqual(manifest['baseline_origin'], 'NOT_CAPTURED')
        self.assertIsNone(manifest['baseline'])
        self.assertEqual(plan['configuration_writes'], [])
        self.assertEqual(plan['lighting_requests'], [])
        self.assertEqual(plan['execution'], 'NOT_AUTHORIZED')
        self.assertEqual(plan['write_policy']['payload_bytes_total'], 2 * len(kit.payloads()['long.bin']) + len(kit.payloads()['short.bin']))
        self.assertEqual(len(kit.payloads()['long.bin']), 3078)
        self.assertEqual(len(kit.payloads()['short.bin']), 64)
        self.assertEqual(kit.payloads()['long.bin'][3071:3074], '中'.encode())
        self.assertEqual(self.call('verify', self.output)[1]['ready_to_execute'], False)
        self.assertEqual(self.call('compare', self.output)[0], 2)

    def test_backup_roundtrip_preserves_raw_bytes_and_does_not_claim_device_read(self):
        raw = b'{ "private": "value", "unknown":1.2300e+04 }\n'
        keymap = self.source('input.json', raw)
        smart = self.source('smart.json', b'\x00raw\xff')
        self.prepare('--keymap', keymap, '--smart-actions', smart)
        self.assertEqual((self.output / 'keymap.bin').read_bytes(), raw)
        manifest = kit.verify(self.output)
        self.assertEqual(manifest['baseline_origin'], 'IMPORTED_UNVERIFIED')
        self.assertNotIn('private', json.dumps(manifest))
        code, result = self.call('compare', self.output, '--keymap', keymap, '--smart-actions', smart)
        self.assertEqual(code, 0)
        self.assertEqual(result['status'], 'LOCAL_BYTES_EQUAL')
        self.assertEqual(result['device_identity_and_freshness'], 'NOT_VERIFIED')
        self.assertFalse(result['ready_to_execute'])
        self.assertEqual(result['runtime_lighting_restore'], 'NOT_VERIFIED')
        keymap.write_bytes(raw + b' ')
        self.assertEqual(self.call('compare', self.output, '--keymap', keymap, '--smart-actions', smart)[0], 1)
        self.assertEqual((self.output / 'keymap.bin').read_bytes(), raw)

    def test_missing_empty_and_present_smart_actions_are_not_interchangeable(self):
        keymap = self.source('input', b'{}')
        self.assertEqual(self.call('prepare', '--output', self.output, '--keymap', keymap)[0], 2)
        self.assertFalse(self.output.exists())
        self.prepare('--keymap', keymap, '--smart-actions-absent')
        self.assertEqual(self.call('compare', self.output, '--keymap', keymap, '--smart-actions-absent')[0], 0)
        empty = self.source('empty', b'')
        self.assertEqual(self.call('compare', self.output, '--keymap', keymap, '--smart-actions', empty)[0], 1)

    def test_tampering_permission_changes_and_missing_files_fail_closed(self):
        self.prepare()
        original = (self.output / 'long.bin').read_bytes()
        (self.output / 'long.bin').write_bytes(b'changed')
        self.assertEqual(self.call('verify', self.output)[0], 2)
        (self.output / 'long.bin').write_bytes(original)
        (self.output / 'long.bin').chmod(0o644)
        self.assertEqual(self.call('verify', self.output)[0], 2)
        (self.output / 'long.bin').chmod(0o600)
        (self.output / 'short.bin').unlink()
        self.assertEqual(self.call('verify', self.output)[0], 2)

    def test_modified_plan_cannot_expand_scope_even_with_recomputed_digest(self):
        self.prepare()
        path = self.output / 'plan.json'
        plan = json.loads(path.read_text())
        plan['target'] = 'keymap.json'
        path.write_bytes(kit.encoded(plan))
        manifest = json.loads((self.output / 'manifest.json').read_text())
        manifest['plan_sha256'] = kit.digest(path.read_bytes())['sha256']
        (self.output / 'manifest.json').write_bytes(kit.encoded(manifest))
        self.assertEqual(self.call('verify', self.output)[0], 2)

    def test_never_overwrites_existing_kit_and_rejects_symlink_sources(self):
        self.prepare()
        before = (self.output / 'manifest.json').read_bytes()
        self.assertEqual(self.call('prepare', '--output', self.output)[0], 2)
        self.assertEqual((self.output / 'manifest.json').read_bytes(), before)
        link = self.root / 'link'
        link.symlink_to(self.source('source', b'{}'))
        self.assertEqual(self.call('prepare', '--output', self.root / 'other', '--keymap', link, '--smart-actions-absent')[0], 2)
        (self.output / 'long.bin').unlink()
        (self.output / 'long.bin').symlink_to(self.root / 'source')
        self.assertEqual(self.call('verify', self.output)[0], 2)

    def test_malformed_metadata_and_nonregular_sources_fail_without_hanging(self):
        fifo = self.root / 'fifo'
        os.mkfifo(fifo)
        self.assertEqual(self.call('prepare', '--output', self.output, '--keymap', fifo, '--smart-actions-absent')[0], 2)
        self.prepare()
        (self.output / 'manifest.json').write_text('[]')
        self.assertEqual(self.call('verify', self.output)[0], 2)

    def test_no_execution_flag_or_resume_command_exists(self):
        for args in [('execute',), ('prepare', '--execute'), ('resume',)]:
            with self.assertRaises(SystemExit) as error, contextlib.redirect_stderr(io.StringIO()):
                kit.main(args)
            self.assertEqual(error.exception.code, 2)


if __name__ == '__main__':
    unittest.main()
