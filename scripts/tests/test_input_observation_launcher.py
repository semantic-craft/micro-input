"""Run the real launcher with only process/signal boundaries replaced; never USB."""
import contextlib
import io
import os
from pathlib import Path
import runpy
import shutil
import signal
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

LAUNCHER = Path(__file__).resolve().parents[1] / 'observe-input'

class Child:
    pid = 987650
    def __init__(self, outcomes):
        self.outcomes = iter(outcomes)
        self.returncode = None
    def poll(self):
        return self.returncode
    def wait(self, timeout):
        value = next(self.outcomes)
        if value == 'timeout':
            raise subprocess.TimeoutExpired('fake', timeout)
        self.returncode = value
        return value

class ObservationLauncherTests(unittest.TestCase):
    def launch(self, arguments, outcomes):
        with tempfile.TemporaryDirectory() as name:
            root = Path(name)
            script = root / 'scripts/observe-input'
            script.parent.mkdir()
            shutil.copyfile(LAUNCHER, script)
            child = Child(outcomes)
            def spawn(command, **kwargs):
                self.assertEqual(command[-6:], ['input_observation', 'native_input_observation', '--', '--ignored', '--exact', '--nocapture'])
                self.assertTrue(kwargs['start_new_session'])
                self.assertEqual(kwargs['env']['MI_INPUT_OBSERVATION_OPERATOR'], 'I_AUTHORIZE_ONE_READONLY_INPUT_OBSERVATION')
                self.assertEqual(kwargs['env']['MI_INPUT_OBSERVATION_SECONDS'], '45')
                return child
            with patch.object(sys, 'argv', [str(script)] + arguments), patch('subprocess.Popen', side_effect=spawn) as popen, patch('subprocess.run') as run, patch('os.killpg') as kill, patch('signal.signal'), contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                run.return_value.returncode = 0
                with self.assertRaises(SystemExit) as result:
                    runpy.run_path(str(script), run_name='__main__')
            if popen.called:
                private = list(root.glob('.scratch/input-study/input-observation-*'))[0]
                self.assertEqual(private.stat().st_mode & 0o777, 0o700)
                self.assertTrue(all(p.stat().st_mode & 0o777 == 0o600 for p in private.iterdir()))
            return result.exception.code, popen.call_count, run, kill

    def test_one_opt_in_child_is_joined_without_signalling_other_apps(self):
        code, count, run, kill = self.launch(['--observe-once'], [0])
        self.assertEqual((code, count), (0, 1))
        run.assert_not_called()
        kill.assert_not_called()

    def test_timeout_terminates_only_own_process_group_without_retry(self):
        code, count, _, kill = self.launch(['--observe-once'], ['timeout', -15])
        self.assertEqual((code, count), (124, 1))
        kill.assert_called_once_with(Child.pid, signal.SIGTERM)

    def test_unconfirmed_exit_escalates_only_own_group_and_does_not_respawn(self):
        code, count, _, kill = self.launch(['--observe-once'], ['timeout'] * 3)
        self.assertEqual((code, count), (124, 1))
        self.assertEqual(kill.call_count, 2)

    def test_default_uses_synthetic_and_removes_inherited_native_opt_in(self):
        with patch.dict(os.environ, {'MI_INPUT_OBSERVATION_OPERATOR': 'inherited'}):
            code, count, run, kill = self.launch([], [])
        self.assertEqual((code, count), (0, 0))
        self.assertIn('--features', run.call_args.args[0])
        self.assertNotIn('MI_INPUT_OBSERVATION_OPERATOR', run.call_args.kwargs['env'])
        kill.assert_not_called()
