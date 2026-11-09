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
for package, behavior in [
    ('mi-proto', 'production_exit_is_strictly_read_only'),
    ('mi-session', 'production_exit_rejects_forbidden_parameters_without_sending'),
    ('mi-session', 'real_framer_never_writes'),
    ('mi-session', 'encoded_mutation_candidates_cannot_bypass_production_exit'),
    ('mi-session', 'encoded_readonly_requests_never_forward_ambiguous_original_bytes'),
]:
    subprocess.run(['cargo', 'test', '--locked', '-p', package, '--lib', behavior], cwd=root, check=True)
assert not (root / 'crates/mi-gpui').exists(), 'Retired crates/mi-gpui directory still exists'
assert (root / 'crates/mi-core').exists(), 'Missing crates/mi-core pure Rust kernel'
assert (root / 'crates/mi-ffi').exists(), 'Missing crates/mi-ffi C-ABI facade'
assert (root / 'app/MicroInput').exists(), 'Missing app/MicroInput native SwiftUI shell'
ffi_cargo = (root / 'crates/mi-ffi/Cargo.toml').read_text(encoding='utf-8')
assert 'uniffi' not in ffi_cargo and not (root / 'crates/mi-ffi/src/bin/uniffi-bindgen.rs').exists(), (
    'crates/mi-ffi must remain a pure C-ABI staticlib without UniFFI dependencies'
)
swift_entry_files = sorted(p.name for p in (root / 'app/MicroInput/Sources/MicroInput').glob('*.swift'))
assert swift_entry_files == ['MicroInputApp.swift'], (
    f'app/MicroInput/Sources/MicroInput must only contain thin host entry MicroInputApp.swift, found: {swift_entry_files}'
)
swift_ui_files = sorted(p.name for p in (root / 'app/MicroInput/Sources/MicroInputUI').glob('*.swift'))
expected_swift_ui_files = [
    'ClassicWorkbenchView.swift',
    'CoreBridge.swift',
    'CreatorMicroBoardView.swift',
    'InlineInspectorView.swift',
    'KeycapGlyphView.swift',
    'ModalsAndStatusMenuView.swift',
    'ProjectionModels.swift',
    'UITheme.swift',
    'WorkbenchRootView.swift',
]
assert swift_ui_files == expected_swift_ui_files, (
    f'app/MicroInput/Sources/MicroInputUI must only contain canonical thin SwiftUI files {expected_swift_ui_files}, found: {swift_ui_files}'
)
for swift_file in (root / 'app/MicroInput/Sources').rglob('*.swift'):
    line_count = len(swift_file.read_text(encoding='utf-8').splitlines())
    assert line_count < 1000, f'{swift_file.relative_to(root)} exceeds 1000-line ceiling ({line_count} lines)'
ui_and_bridge = '\n'.join(
    p.read_text()
    for crate in ('crates/mi-bridge/src', 'crates/mi-macos/src', 'crates/mi-core/src', 'crates/mi-ffi/src')
    for p in (root / crate).rglob('*.rs')
)
assert not re.search(r'pub async fn (?:request|rpc|send_report)\b', ui_and_bridge), 'raw RPC exposed in UI/bridge crates'
subprocess.run(['cargo', 'test', '--locked', '-p', 'mi-session', '--features', 'synthetic-backend', '--test', 'config_write', 'synthetic_writer_rejects_native_backend_even_when_feature_is_built'], cwd=root, check=True)

home_pattern = re.compile(r'/Users/(?!(?:example|test|private|Shared)(?:/|["\'\s),]|$))[A-Za-z0-9._-]+')
tracked = subprocess.check_output(
    ['git', 'ls-files', '--cached', '--others', '--exclude-standard'],
    cwd=root,
    text=True,
).splitlines()
home_violations = []
for rel in tracked:
    if rel == 'lessons.md' or rel.startswith('.audit/'):
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

residual_patches = sorted(str(p.relative_to(root)) for p in (root / 'integrations').rglob('*.patch'))
assert not residual_patches, (
    'R-C02 violation: external agent apps must be adapted directly from Micro Input without third-party .patch files:\n  '
    + '\n  '.join(residual_patches)
)

rust_ownership_src = (root / 'crates/mi-bridge/src/ownership.rs').read_text(encoding='utf-8')
rust_unbridged_match = re.search(
    r'pub fn is_unbridged_desktop_active\(&self\)\s*->\s*bool\s*\{([^}]+)\}',
    rust_ownership_src,
)
assert rust_unbridged_match, 'NativeOwnershipController::is_unbridged_desktop_active definition missing in crates/mi-bridge/src/ownership.rs'
rust_unbridged_body = rust_unbridged_match.group(1)
assert 'bridge_observed' not in rust_unbridged_body and 'native_identity' not in rust_unbridged_body, (
    'R-C03 violation in crates/mi-bridge/src/ownership.rs: '
    'NativeOwnershipController::is_unbridged_desktop_active must not gate fallback on sticky bridge_observed or native_identity state'
)

rust_voice_src = (root / 'crates/mi-bridge/src/voice.rs').read_text(encoding='utf-8')
held_start_pos = rust_voice_src.find('ptt.start_held_dictation(')
wait_outcome_pos = rust_voice_src.find('ptt.wait_for_start_outcome().await')
recording_after_ptt_pos = rust_voice_src.find('FourAppDictationState::Recording', held_start_pos if held_start_pos != -1 else 0)
assert (
    held_start_pos != -1
    and wait_outcome_pos != -1
    and recording_after_ptt_pos != -1
    and held_start_pos < wait_outcome_pos < recording_after_ptt_pos
), (
    'R-C04 violation in crates/mi-bridge/src/voice.rs: '
    'FourAppVoiceManager::start_dictation must await ptt.wait_for_start_outcome().await before entering FourAppDictationState::Recording'
)

run_src = (root / 'scripts/run').read_text(encoding='utf-8')
assert (
    'argparse.ArgumentParser' in run_src
    and '__CFBundleIdentifier' in run_src
    and "'--verify' in sys.argv" not in run_src
    and "'-u', str(app)" in run_src
), (
    'R-C05 violation in scripts/run: use argparse.ArgumentParser(allow_abbrev=False) with --list/--kill, strip __CFBundleIdentifier, '
    'and unregister QA app bundles from LaunchServices on cleanup'
)
swift_app_src = (root / 'app/MicroInput/Sources/MicroInput/MicroInputApp.swift').read_text(encoding='utf-8')
swift_lock_pos = swift_app_src.find('mi_core_acquire_instance_lock(')
swift_menu_pos = swift_app_src.find('setupStatusItem()')
assert swift_lock_pos != -1 and swift_menu_pos != -1 and swift_lock_pos < swift_menu_pos, (
    'R-C05 violation in app/MicroInput/Sources/MicroInput/MicroInputApp.swift: '
    'AppDelegate.init() must acquire mi_core_acquire_instance_lock before setupStatusItem()'
)
rust_exec_src = (root / 'crates/mi-macos/src/executor.rs').read_text(encoding='utf-8')
assert rust_exec_src.count('AXIsProcessTrustedWithOptions') == 0, (
    'R-C05 violation in crates/mi-macos/src/executor.rs: '
    'do not call AXIsProcessTrustedWithOptions inside host execution paths'
)

print('UNKNOWN inventory and read-only/no-send boundaries: PASS')
