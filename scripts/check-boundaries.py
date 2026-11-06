"""Deterministic guard for UNKNOWN inventory and the physical framing barrier."""
from pathlib import Path
import re
import subprocess
root = Path(__file__).resolve().parent.parent
sources = list((root / 'crates').rglob('*.rs'))
found = set()
for path in sources:
    found.update(re.findall(r'TODO\((UNKNOWN-\d+)\)', path.read_text()))
expected = {f'UNKNOWN-{n:02}' for n in range(1, 10)}
doc = (root / 'docs/spec/UNKNOWNS.md').read_text()
documented = set(re.findall(r'^\| (UNKNOWN-\d+) \|', doc, re.MULTILINE))
assert expected <= documented, (expected - documented)
assert found <= documented, (found - documented)
# R-D13: guard behavior rather than requiring one placeholder implementation.
# The tests capture the actual session出口 and assert zero reports for rejected requests.
for package, behavior in [
    ('mi-proto', 'production_exit_is_strictly_read_only'),
    ('mi-session', 'production_exit_rejects_forbidden_parameters_without_sending'),
    ('mi-session', 'real_framer_never_writes'),
    ('mi-session', 'encoded_mutation_candidates_cannot_bypass_production_exit'),
    ('mi-session', 'encoded_readonly_requests_never_forward_ambiguous_original_bytes'),
]:
    subprocess.run(['cargo', 'test', '--locked', '-p', package, behavior], cwd=root, check=True)
ffi = '\n'.join(p.read_text() for p in (root / 'crates/mi-ffi/src').rglob('*.rs'))
assert not re.search(r'pub async fn (?:request|rpc|send_report)\b', ffi), 'raw RPC exposed through UniFFI'
subprocess.run(['cargo', 'test', '--locked', '-p', 'mi-session', '--features', 'synthetic-backend', 'synthetic_writer_rejects_native_backend_even_when_feature_is_built'], cwd=root, check=True)

# R-C01: forbid hardcoded host home paths outside synthetic placeholders (/Users/example, /Users/test, /Users/private).
home_pattern = re.compile(r'/Users/(?!(?:example|test|private|Shared)(?:/|["\'\s),]|$))[A-Za-z0-9._-]+')
tracked = subprocess.check_output(['git', 'ls-files'], cwd=root, text=True).splitlines()
home_violations = []
for rel in tracked:
    if rel == 'lessons.md':
        continue
    path = root / rel
    if not path.is_file():
        continue
    try:
        text = path.read_text(encoding='utf-8')
    except UnicodeDecodeError:
        continue
    for lineno, line in enumerate(text.splitlines(), 1):
        if home_pattern.search(line):
            home_violations.append(f'{rel}:{lineno}: {line.strip()}')
assert not home_violations, (
    'Hardcoded user home path found; resolve via Path.home(), os.environ, or config discovery instead:\n  '
    + '\n  '.join(home_violations)
)

# R-C02: integration patches must include all new bridge/fixture files (never drop untracked files via bare `git diff HEAD`).
required_patch_files = {
    'integrations/cindy/native-ownership.patch': [
        'apps/desktop/src/main/worklouder-codex/WorkLouderOwnershipFence.ts',
        'apps/desktop/src/main/worklouder-codex/microInputNavigation.ts',
        'apps/desktop/src/main/worklouder-codex/microInputOwnership.ts',
        'apps/desktop/src/renderer/lib/workLouderCodexOwnership.ts',
        'scripts/micro-input-navigation-fixture.ts',
        'scripts/micro-input-ownership-fixture.ts',
    ],
    'integrations/dsh/navigation.patch': [
        'apps/desktop-host/src/micro-input-host.ts',
        'apps/desktop-host/src/micro-input.ts',
        'scripts/micro-input-fixture.ts',
    ],
    'integrations/zcode/navigation.patch': [
        'packages/desktop/src/main/micro-input/socket.ts',
        'scripts/micro-input-fixture.ts',
    ],
}
missing_patch_entries = []
for patch_rel, expected_files in required_patch_files.items():
    patch_text = (root / patch_rel).read_text(encoding='utf-8', errors='replace')
    present = set(re.findall(r'^\+\+\+ b/(.+)$', patch_text, re.MULTILINE))
    for req in expected_files:
        if req not in present:
            missing_patch_entries.append(f'{patch_rel} missing {req}')
assert not missing_patch_entries, (
    'Integration patch is missing new files (run `git add -N .` in the target checkout before `git diff HEAD`):\n  '
    + '\n  '.join(missing_patch_entries)
)

# R-C03: NativeOwnershipController.isUnbridgedDesktopActive must derive solely from current socket presence,
# never from sticky session latches (bridgeObserved / nativeIdentity) that permanently lock out desktop control after Cindy exits.
ownership_src = (root / 'app/MicroInput/Sources/MicroInput/NativeOwnership.swift').read_text(encoding='utf-8')
unbridged_match = re.search(r'private var isUnbridgedDesktopActive:\s*Bool\s*\{([^}]+)\}', ownership_src)
assert unbridged_match, 'NativeOwnershipController.isUnbridgedDesktopActive definition missing in NativeOwnership.swift'
unbridged_body = unbridged_match.group(1)
assert 'bridgeObserved' not in unbridged_body and 'nativeIdentity' not in unbridged_body, (
    'R-C03 violation in app/MicroInput/Sources/MicroInput/NativeOwnership.swift: '
    'NativeOwnershipController.isUnbridgedDesktopActive must not gate fallback on sticky bridgeObserved or nativeIdentity state'
)

# R-C04: FourAppVoiceManager.startDictation must await PushToTalkController.waitForStartOutcome() before entering .recording state.
voice_src = (root / 'app/MicroInput/Sources/MicroInput/FourAppVoice.swift').read_text(encoding='utf-8')
assert 'store.pushToTalk.waitForStartOutcome()' in voice_src, (
    'R-C04 violation in app/MicroInput/Sources/MicroInput/FourAppVoice.swift: '
    'FourAppVoiceManager.startDictation must await store.pushToTalk.waitForStartOutcome() before setting dictationState = .recording'
)

# R-C05: scripts/run must use strict argparse (supporting --list/--kill without spawning and stripping __CFBundleIdentifier),
# and MicroInputApp.init() must acquire DocumentInstanceLock before creating StatusMenu so duplicate menu bar icons cannot occur.
run_src = (root / 'scripts/run').read_text(encoding='utf-8')
assert 'argparse.ArgumentParser' in run_src and '__CFBundleIdentifier' in run_src and "'--verify' in sys.argv" not in run_src, (
    'R-C05 violation in scripts/run: use argparse.ArgumentParser(allow_abbrev=False) with --list/--kill and strip __CFBundleIdentifier '
    'instead of ad-hoc sys.argv substring checks that spawn duplicate MicroInput instances on unknown flags'
)
app_src = (root / 'app/MicroInput/Sources/MicroInput/MicroInputApp.swift').read_text(encoding='utf-8')
lock_pos = app_src.find('DocumentInstanceLock.acquire(')
menu_pos = app_src.find('statusMenu = StatusMenu(')
assert lock_pos != -1 and menu_pos != -1 and lock_pos < menu_pos, (
    'R-C05 violation in app/MicroInput/Sources/MicroInput/MicroInputApp.swift: '
    'MicroInputApp.init() must acquire DocumentInstanceLock.acquire(directory:) before creating StatusMenu(store:)'
)

print('UNKNOWN inventory and read-only/no-send boundaries: PASS')
