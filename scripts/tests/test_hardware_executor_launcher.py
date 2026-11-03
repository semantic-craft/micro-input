"""Run the real supervisor with process/signal boundaries replaced. No device I/O."""
import contextlib
import importlib.machinery
import importlib.util
import io
import json
import os
from pathlib import Path
import runpy
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]

class HardwareExecutorTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / 'scripts').mkdir()
        for name in ('hardware-acceptance', 'prepare-hardware-acceptance'):
            shutil.copyfile(SCRIPTS / name, self.root / 'scripts' / name)
        self.script = self.root / 'scripts' / 'hardware-acceptance'
        self.kit = self.root / 'kit'
        loader = importlib.machinery.SourceFileLoader('fixture_kit', str(self.root / 'scripts' / 'prepare-hardware-acceptance'))
        spec = importlib.util.spec_from_loader(loader.name, loader)
        mod = importlib.util.module_from_spec(spec)
        loader.exec_module(mod)
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(mod.main(['prepare', '--output', str(self.kit)]), 0)

    def invoke(self, args, popen=None, run=None, kill=None):
        with patch.object(sys, 'argv', [str(self.script)] + args), \
             patch('subprocess.Popen', side_effect=popen or AssertionError('unexpected process')), \
             patch('subprocess.run', side_effect=run or AssertionError('unexpected cargo')), \
             patch('os.killpg', side_effect=kill) as kills, patch('signal.signal'), \
             contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit) as result:
                runpy.run_path(str(self.script), run_name='__main__')
        return result.exception.code, kills

    def args(self, scope):
        args = ['--execute', scope, '--kit', str(self.kit), '--device-path', 'mock-only-path']
        if scope == 'h1': args += ['--expected-keymap-sha256', 'a' * 64, '--unresolved-operations-reviewed']
        return args

    def test_default_scrubs_authority_and_selects_only_nonignored_tests(self):
        def run(command, **kwargs):
            self.assertNotIn('--ignored', command)
            self.assertFalse(any(k.startswith(('MI_HARDWARE_', 'MI_PROBE_')) for k in kwargs['env']))
            return subprocess.CompletedProcess(command, 0)
        with patch.dict(os.environ, {'MI_HARDWARE_OPERATOR':'danger', 'MI_PROBE_A_OPERATOR':'danger'}):
            self.assertEqual(self.invoke([], run=run)[0], 0)

    def test_invalid_scope_or_kit_never_launches(self):
        for args in (['--execute','h1'], ['--device-path','x'], self.args('h1')[:-1], self.args('h0')+['--unresolved-operations-reviewed']):
            self.assertEqual(self.invoke(args)[0], 2)
        (self.kit / 'long.bin').write_bytes(b'corrupt')
        self.assertEqual(self.invoke(self.args('h1'))[0], 2)

    def test_exact_native_selection_private_journal_and_one_shot_claim(self):
        class Child:
            pid = 999999
            def poll(self): return 0
            def wait(self, timeout): return 0
        def popen(command, **kwargs):
            scope = command[-5].removeprefix('native_')
            self.assertIn(scope, ('h0', 'h1'))
            self.assertEqual(command[-4:], ['--','--ignored','--exact','--nocapture'])
            self.assertTrue(kwargs['start_new_session'])
            self.assertEqual(kwargs['env']['MI_HARDWARE_PATH'], 'mock-only-path')
            self.assertEqual(kwargs['env']['MI_HARDWARE_OPERATOR'], 'I_AUTHORIZE_H0_READONLY_BACKUP' if scope=='h0' else 'I_AUTHORIZE_H1_EXACT_TEMPFILE')
            journals=list(self.root.glob('.scratch/input-study/hardware-supervisor-*/prepared.json'))
            self.assertTrue(journals)
            for p in journals:
                self.assertEqual(p.stat().st_mode & 0o777, 0o600)
                self.assertEqual(p.parent.stat().st_mode & 0o777, 0o700)
            if scope=='h1': self.assertEqual(len(list(self.root.glob('*.h1-claimed.json'))),1)
            return Child()
        self.assertEqual(self.invoke(self.args('h0'), popen=popen)[0], 0)
        self.assertEqual(self.invoke(self.args('h1'), popen=popen)[0], 0)
        self.assertEqual(self.invoke(self.args('h1'))[0], 2)

    def test_timeout_stops_only_own_group_and_keeps_claim(self):
        class Child:
            pid = 999999
            calls = 0
            def poll(self): return None
            def wait(self, timeout):
                self.calls += 1
                if self.calls == 1: raise subprocess.TimeoutExpired('mock',timeout)
                return -15
        code, kills = self.invoke(self.args('h1'), popen=lambda *a, **k: Child())
        self.assertEqual(code, 124)
        self.assertEqual(kills.call_count,1)
        self.assertEqual(kills.call_args.args[0],999999)
        self.assertEqual(len(list(self.root.glob('*.h1-claimed.json'))),1)
        self.assertEqual(len(list(self.root.glob('.scratch/input-study/*/stopped-unknown.json'))),1)
