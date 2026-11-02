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

SCRIPTS = Path(__file__).resolve().parents[1]


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
    probe = 'a'

    def run_launcher(self, results):
        sandbox = tempfile.TemporaryDirectory(prefix=f'mi-probe-{self.probe}-launcher-')
        self.addCleanup(sandbox.cleanup)
        root = Path(sandbox.name).resolve()
        script = root / 'scripts' / f'probe-{self.probe}'
        script.parent.mkdir()
        shutil.copyfile(SCRIPTS / f'probe-{self.probe}', script)
        shutil.copyfile(SCRIPTS / 'probe_launcher.py', script.parent / 'probe_launcher.py')
        child = FakeChild(results)

        def fake_popen(command, **kwargs):
            # The real prepared journal must be private and durable enough to
            # reopen before the launcher is allowed to create its child.
            prepared = list(root.glob(f'.scratch/input-study/probe-{self.probe}-supervisor-*/prepared.json'))
            self.assertEqual(len(prepared), 1)
            self.assertIsNone(json.loads(prepared[0].read_text())['pid'])
            self.assertEqual(stat.S_IMODE(prepared[0].stat().st_mode), 0o600)
            self.assertEqual(kwargs['cwd'], root)
            self.assertTrue(kwargs['start_new_session'])
            self.assertEqual(kwargs['env'][f'MI_PROBE_{self.probe.upper()}_OPERATOR'], f'I_AUTHORIZE_EXACT_TEMPFILE_{self.probe.upper()}')
            self.assertEqual(command[-6:], [f'probe_{self.probe}', f'native_tempfile_{self.probe}', '--', '--ignored', '--exact', '--nocapture'])
            return child

        stderr = io.StringIO()
        with patch.object(sys, 'path', [str(script.parent)] + sys.path), \
                patch.object(sys, 'argv', [str(script), f'--execute-exact-tempfile-{self.probe}']), \
                patch('subprocess.Popen', side_effect=fake_popen) as popen, \
                patch('subprocess.run', side_effect=AssertionError('unexpected real subprocess')), \
                patch('os.killpg') as killpg, patch('signal.signal'), \
                contextlib.redirect_stderr(stderr):
            with self.assertRaises(SystemExit) as exit_result:
                runpy.run_path(str(script), run_name='__main__')
        directories = list(root.glob(f'.scratch/input-study/probe-{self.probe}-supervisor-*'))
        self.assertEqual(len(directories), 1)
        self.assertEqual(stat.S_IMODE(directories[0].stat().st_mode), 0o700)
        journals = {}
        for path in directories[0].iterdir():
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
            journals[path.stem] = json.loads(path.read_text())
        self.assertEqual(popen.call_count, 1)
        return exit_result.exception.code, child, journals, killpg, stderr.getvalue()

    def test_default_and_dry_run_cannot_inherit_native_authorization(self):
        for mode in ([], ['--dry-run']):
            with tempfile.TemporaryDirectory(prefix='mi-probe-launcher-dry-') as directory:
                root = Path(directory).resolve()
                script = root / 'scripts' / f'probe-{self.probe}'
                script.parent.mkdir()
                shutil.copyfile(SCRIPTS / f'probe-{self.probe}', script)
                shutil.copyfile(SCRIPTS / 'probe_launcher.py', script.parent / 'probe_launcher.py')
                with patch.object(sys, 'path', [str(script.parent)] + sys.path), \
                        patch.object(sys, 'argv', [str(script)] + mode), \
                        patch.dict('os.environ', {'MI_PROBE_A_OPERATOR': 'I_AUTHORIZE_EXACT_TEMPFILE_A',
                                                  'MI_PROBE_B_OPERATOR': 'I_AUTHORIZE_EXACT_TEMPFILE_B'}), \
                        patch('subprocess.run') as run, patch('subprocess.Popen') as popen, \
                        contextlib.redirect_stdout(io.StringIO()):
                    run.return_value.returncode = 0
                    with self.assertRaises(SystemExit) as result:
                        runpy.run_path(str(script), run_name='__main__')
                    self.assertEqual(result.exception.code, 0)
                    command = run.call_args.args[0]
                    self.assertEqual(command[-1], f'probe_{self.probe}')
                    self.assertNotIn('--ignored', command)
                    self.assertNotIn('MI_PROBE_A_OPERATOR', run.call_args.kwargs['env'])
                    self.assertNotIn('MI_PROBE_B_OPERATOR', run.call_args.kwargs['env'])
                    popen.assert_not_called()
                    self.assertFalse((root / '.scratch').exists())

    def test_other_probe_flag_cannot_start_child(self):
        script = SCRIPTS / f'probe-{self.probe}'
        other = 'b' if self.probe == 'a' else 'a'
        with patch.object(sys, 'path', [str(SCRIPTS)] + sys.path), \
                patch.object(sys, 'argv', [str(script), f'--execute-exact-tempfile-{other}']), \
                patch('subprocess.run') as run, patch('subprocess.Popen') as popen, \
                contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit) as result:
                runpy.run_path(str(script), run_name='__main__')
            self.assertEqual(result.exception.code, 2)
            run.assert_not_called()
            popen.assert_not_called()

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


class ProbeBLauncherTests(ProbeALauncherTests):
    probe = 'b'


if __name__ == '__main__':
    unittest.main()
