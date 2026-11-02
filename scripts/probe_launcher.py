"""Closed private tempfile launch supervisor shared by the fixed A/B entries.

No native path is selected by default; interruption never retries or auto-cleans.
"""
import json
import os
import signal
import subprocess
import sys
import time
import uuid


def main(probe, ROOT):
    assert probe in ('a', 'b')
    MODE = sys.argv[1:]
    COMMAND = ['cargo', 'test', '--locked', '-p', 'mi-session', '--test', f'probe_{probe}']
    if MODE in ([], ['--dry-run']):
        # Remove any inherited opt-in; the ignored native test is never selected.
        env = dict(os.environ)
        env.pop('MI_PROBE_A_OPERATOR', None)
        env.pop('MI_PROBE_B_OPERATOR', None)
        result = subprocess.run(COMMAND, cwd=ROOT, env=env, check=False)
        print('Dry-run: mock USB only; native test remains ignored.', flush=True)
        sys.exit(result.returncode)
    if MODE != [f'--execute-exact-tempfile-{probe}']:
        print(f'Usage: scripts/probe-{probe} [--dry-run | --execute-exact-tempfile-{probe}]', file=sys.stderr)
        sys.exit(2)

    # Persist supervisor metadata before the child can touch hardware. The native
    # harness journals the exact generated target and stage before every mutation.
    private = ROOT / '.scratch' / 'input-study' / (f'probe-{probe}-supervisor-' + str(uuid.uuid4()))
    private.mkdir(parents=True, mode=0o700)
    env = dict(os.environ)
    env.pop('MI_PROBE_A_OPERATOR', None)
    env.pop('MI_PROBE_B_OPERATOR', None)
    env[f'MI_PROBE_{probe.upper()}_OPERATOR'] = f'I_AUTHORIZE_EXACT_TEMPFILE_{probe.upper()}'
    child = None

    def journal(stage):
        path = private / (stage + '.json')
        with open(path, 'x', opener=lambda path, flags: os.open(path, flags, 0o600)) as f:
            json.dump({'stage': stage, 'pid': child.pid if child else None, 'returncode': child.poll() if child else None, 'time': time.time(),
                       'state_if_interrupted': 'UNKNOWN; do not retry or auto-clean',
                       'private_harness_journals': f'.scratch/input-study/probe-{probe}-*/stage-*.json'}, f)
            f.flush()
            os.fsync(f.fileno())

    def stop_own_child():
        if child is None or child.poll() is not None:
            return True
        for sig in (signal.SIGTERM, signal.SIGKILL):
            try:
                os.killpg(child.pid, sig)
            except ProcessLookupError:
                return child.poll() is not None
            try:
                child.wait(timeout=10)
                return True
            except subprocess.TimeoutExpired:
                pass
        return False

    def interrupted(_signal, _frame):
        raise KeyboardInterrupt

    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGHUP, interrupted)
    journal('prepared')
    try:
        child = subprocess.Popen(COMMAND + [f'native_tempfile_{probe}', '--', '--ignored', '--exact', '--nocapture'],
                                 cwd=ROOT, env=env, start_new_session=True)
        journal('started')
        code = child.wait(timeout=240)
        journal('child-exited')
    except (subprocess.TimeoutExpired, KeyboardInterrupt, OSError):
        stopped = stop_own_child()
        try:
            journal('child-stopped-state-unknown' if stopped else 'exit-unconfirmed-unknown')
        except OSError:
            pass
        pid = child.pid if child else 'not-started'
        print(f'{probe.upper()} UNKNOWN after interruption/watchdog, PID={pid}, exit_confirmed={stopped}; '
              'inspect exact target/last stage in private journals; no automatic retry.', file=sys.stderr)
        sys.exit(124)
    sys.exit(code)
