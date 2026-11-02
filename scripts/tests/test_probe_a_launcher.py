"""Exercise the actual live launcher with the process/OS boundaries replaced.

The copied launcher computes ROOT inside a fresh TemporaryDirectory. No Cargo,
process, signal, HID transport or native query is executed by these tests.
"""
import contextlib
import io
import json
from pathlib import Path
import runpy
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

LAUNCHER = Path(__file__).resolve().parents[1] / 'probe-a'


class FakeChild:
    pid = 987654

    def __init__(self, results):
        self.results = iter(results)
        self.returncode = None
        self.wait_budgets = []

    def poll(self):
        return self.returncode

    def wait(self, timeout):
        self.wait_budgets.append(timeout)
        result = next(self.results)
        if result == 'timeout':
            raise subprocess.TimeoutExpired('fake-no-process', timeout)
        self.returncode = result
        return result


class ProbeALauncherTests(unittest.TestCase):
    def run_launcher(self, results):
        sandbox = tempfile.TemporaryDirectory(prefix='mi-probe-a-launcher-')
        self.addCleanup(sandbox.cleanup)
        root = Path(sandbox.name).resolve()
        script = root / 'scripts' / 'probe-a'
        script.parent.mkdir()
        shutil.copyfile(LAUNCHER, script)
        child = FakeChild(results)

        def fake_popen(command, **kwargs):
            # The real prepared journal must be private and durable enough to
            # reopen before the launcher is allowed to create its child.
            prepared = list(root.glob('.scratch/input-study/probe-a-supervisor-*/prepared.json'))
            self.assertEqual(len(prepared), 1)
            self.assertIsNone(json.loads(prepared[0].read_text())['pid'])
            self.assertEqual(stat.S_IMODE(prepared[0].stat().st_mode), 0o600)
            self.assertEqual(kwargs['cwd'], root)
            self.assertTrue(kwargs['start_new_session'])
            self.assertEqual(kwargs['env']['MI_PROBE_A_OPERATOR'], 'I_AUTHORIZE_EXACT_TEMPFILE_A')
            self.assertEqual(command[-6:], ['probe_a', 'native_tempfile_a', '--', '--ignored', '--exact', '--nocapture'])
            return child

        stderr = io.StringIO()
        with patch.object(sys, 'argv', [str(script), '--execute-exact-tempfile-a']), \
                patch('subprocess.Popen', side_effect=fake_popen) as popen, \
                patch('subprocess.run', side_effect=AssertionError('unexpected real subprocess')), \
                patch('os.killpg') as killpg, patch('signal.signal'), \
                contextlib.redirect_stderr(stderr):
            with self.assertRaises(SystemExit) as exit_result:
                runpy.run_path(str(script), run_name='__main__')
        directories = list(root.glob('.scratch/input-study/probe-a-supervisor-*'))
        self.assertEqual(len(directories), 1)
        self.assertEqual(stat.S_IMODE(directories[0].stat().st_mode), 0o700)
        journals = {}
        for path in directories[0].iterdir():
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
            journals[path.stem] = json.loads(path.read_text())
        self.assertEqual(popen.call_count, 1)
        return exit_result.exception.code, child, journals, killpg, stderr.getvalue()

    def test_live_entry_prepares_private_journals_before_one_fake_child(self):
        code, child, journals, killpg, stderr = self.run_launcher([0])
        self.assertEqual(code, 0)
        self.assertEqual(set(journals), {'prepared', 'started', 'child-exited'})
        self.assertEqual(journals['child-exited']['returncode'], 0)
        self.assertEqual(child.wait_budgets, [240])
        killpg.assert_not_called()
        self.assertEqual(stderr, '')

    def test_watchdog_stops_only_its_fake_child_and_records_unknown(self):
        code, child, journals, killpg, stderr = self.run_launcher(['timeout', -15])
        self.assertEqual(code, 124)
        self.assertEqual(child.wait_budgets, [240, 10])
        killpg.assert_called_once_with(child.pid, signal.SIGTERM)
        self.assertIn('child-stopped-state-unknown', journals)
        self.assertEqual(journals['child-stopped-state-unknown']['pid'], child.pid)
        self.assertIn('no automatic retry', stderr)

    def test_watchdog_unconfirmed_exit_keeps_pid_and_never_respawns(self):
        code, child, journals, killpg, stderr = self.run_launcher(['timeout'] * 3)
        self.assertEqual(code, 124)
        self.assertEqual(child.wait_budgets, [240, 10, 10])
        self.assertEqual(killpg.call_args_list, [unittest.mock.call(child.pid, signal.SIGTERM),
                                               unittest.mock.call(child.pid, signal.SIGKILL)])
        self.assertIn('exit-unconfirmed-unknown', journals)
        self.assertIsNone(journals['exit-unconfirmed-unknown']['returncode'])
        self.assertIn('exit_confirmed=False', stderr)


if __name__ == '__main__':
    unittest.main()
