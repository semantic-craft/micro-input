import base64
from pathlib import Path
import plistlib
import re
import subprocess
from urllib.parse import urlparse
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
    'SoftwareUpdate.swift',
    'UITheme.swift',
    'WorkbenchRootView.swift',
]
assert swift_ui_files == expected_swift_ui_files, (
    f'app/MicroInput/Sources/MicroInputUI must only contain canonical thin SwiftUI files {expected_swift_ui_files}, found: {swift_ui_files}'
)
for ui_file in (root / 'app/MicroInput/Sources/MicroInputUI').glob('*.swift'):
    ui_text = ui_file.read_text(encoding='utf-8')
    assert not re.search(r'^\s*import\s+Sparkle\b', ui_text, re.MULTILINE), (
        f'{ui_file.relative_to(root)} must not import Sparkle; keep Sparkle confined to host target MicroInput'
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

run_src = (root / 'scripts/run').read_text(encoding='utf-8')
assert (
    'argparse.ArgumentParser' in run_src
    and '__CFBundleIdentifier' in run_src
    and "'--verify' in sys.argv" not in run_src
    and "'-u', str(app)" in run_src
    and 'symlinks=True' in run_src
), (
    'R-C05 violation in scripts/run: use argparse.ArgumentParser(allow_abbrev=False) with --list/--kill, strip __CFBundleIdentifier, '
    'copy app bundles with symlinks=True, and unregister QA app bundles from LaunchServices on cleanup'
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

core_bridge_src = (root / 'app/MicroInput/Sources/MicroInputUI/CoreBridge.swift').read_text(encoding='utf-8')
assert (
    '@Observable' in core_bridge_src
    and '@ObservationIgnored public private(set) var model: ProjectionModel' in core_bridge_src
    and 'ObservableObject' not in core_bridge_src
    and '@Published' not in core_bridge_src
), (
    'R-C06 violation in app/MicroInput/Sources/MicroInputUI/CoreBridge.swift: '
    'CoreStore must use @Observable with @ObservationIgnored model and Equatable-gated domain slices; '
    'do not use ObservableObject or @Published var model'
)
for view_name in (
    'WorkbenchRootView.swift',
    'ClassicWorkbenchView.swift',
    'CreatorMicroBoardView.swift',
    'InlineInspectorView.swift',
    'ModalsAndStatusMenuView.swift',
    'SoftwareUpdate.swift',
):
    view_src = (root / 'app/MicroInput/Sources/MicroInputUI' / view_name).read_text(encoding='utf-8')
    assert (
        '@ObservedObject' not in view_src
        and 'store.model' not in view_src
        and 'let model: ProjectionModel' not in view_src
    ), (
        f'R-C06 violation in app/MicroInput/Sources/MicroInputUI/{view_name}: '
        'SwiftUI views must read fine-grained domain slices (store.header, store.sidebar, store.canvas, '
        'store.sessionStrip, store.inspector, store.statusMenu, store.overlay, store.footer) instead of '
        '@ObservedObject, store.model, or passing monolithic ProjectionModel'
    )
ffi_lib_src = (root / 'crates/mi-ffi/src/lib.rs').read_text(encoding='utf-8')
assert 'store.projection_generation() == before_gen' in ffi_lib_src, (
    'R-C06 violation in crates/mi-ffi/src/lib.rs: '
    'mi_core_dispatch_json must short-circuit with null when store.projection_generation() is unchanged'
)

for view_path in sorted((root / 'app/MicroInput/Sources/MicroInputUI').glob('*.swift')):
    lines = view_path.read_text(encoding='utf-8').splitlines()
    for idx, line in enumerate(lines):
        if '.buttonStyle(.plain)' in line or '.onTapGesture' in line:
            window_start = max(0, idx - 25)
            window_end = min(len(lines), idx + 6)
            window_text = '\n'.join(lines[window_start:window_end])
            assert '.contentShape(Rectangle())' in window_text, (
                f'R-C07 violation in {view_path.relative_to(root)}:{idx + 1}: '
                'every .buttonStyle(.plain) and .onTapGesture control must attach .contentShape(Rectangle()) '
                'so edge/spacer clicks within the control bounds are never dead zones'
            )

inspector_src = (root / 'app/MicroInput/Sources/MicroInputUI/InlineInspectorView.swift').read_text(encoding='utf-8')
assert (
    'ScrollViewReader' in inspector_src
    and 'scrollTo("inspector-scroll-top"' in inspector_src
    and inspector_src.count('.id(') >= 2
    and inspector_src.count('.onSubmit') >= 2
), (
    'R-C07 violation in app/MicroInput/Sources/MicroInputUI/InlineInspectorView.swift: '
    'CreatorMicroInlineInspectorView must bind .id(...) on slot/layer headers, reset scroll via ScrollViewReader, '
    'and attach .onSubmit handlers to search TextFields'
)

store_src = (root / 'crates/mi-core/src/store.rs').read_text(encoding='utf-8')
assert 'inspector_search_active' in store_src, (
    'R-C07 violation in crates/mi-core/src/store.rs: '
    'consume_physical_input_event must guard selected_surface auto-follow with !inspector_search_active '
    'so hardware key events do not hijack InlineInspectorView focus during active search/recording'
)

assert (
    'func applicationDockMenu(' in swift_app_src
    and 'win.isMiniaturized' in swift_app_src
    and 'win.deminiaturize(' in swift_app_src
    and 'applicationShouldHandleReopen(' in swift_app_src
    and 'applicationIconImage' in swift_app_src
), (
    'R-C07 violation in app/MicroInput/Sources/MicroInput/MicroInputApp.swift: '
    'AppDelegate must implement applicationDockMenu, deminiaturize minimized windows in openMainWindow/reopen, '
    'and set NSApp.applicationIconImage from AppIcon.icns'
)

info_plist_src = (root / 'app/MicroInput/Info.plist').read_text(encoding='utf-8')
build_src = (root / 'scripts/build').read_text(encoding='utf-8')
assert '<key>CFBundleIconFile</key>' in info_plist_src and '<string>AppIcon</string>' in info_plist_src and 'AppIcon.icns' in build_src, (
    'R-C07 violation: app/MicroInput/Info.plist and scripts/build must package and declare AppIcon.icns'
)
info_plist_data = plistlib.loads(info_plist_src.encode('utf-8'))
assert info_plist_data.get('CFBundleIdentifier') == 'craft.semantic.micro-input.dev', (
    'Info.plist CFBundleIdentifier must remain craft.semantic.micro-input.dev to preserve macOS TCC grants'
)
parsed_feed = urlparse(str(info_plist_data.get('SUFeedURL', '')))
assert parsed_feed.scheme == 'https' and bool(parsed_feed.netloc), (
    'Info.plist SUFeedURL must declare an HTTPS appcast feed URL with non-empty host'
)
decoded_ed_key = base64.b64decode(str(info_plist_data.get('SUPublicEDKey', '')), validate=True)
assert len(decoded_ed_key) == 32, (
    'Info.plist SUPublicEDKey must be a valid 32-byte base64 Ed25519 public key'
)
assert (root / 'LICENSE').is_file() and 'MIT License' in (root / 'LICENSE').read_text(encoding='utf-8'), (
    'Repository root must contain MIT LICENSE file'
)

projection_src = (root / 'crates/mi-core/src/projection.rs').read_text(encoding='utf-8')
for forbidden in (
    'WorkbenchStore', 'NativeOwnershipController', 'PushToTalkController',
    'AppBundleCache', 'DeviceSession', 'RunningSixLayer', 'Instant::now',
    '.elapsed(', '.load(Ordering', 'std::fs', 'self.panel_host', 'self.status_item_host',
):
    assert forbidden not in projection_src, (
        f'R-C08 violation in crates/mi-core/src/projection.rs: {forbidden} is a runtime capability; '
        'projection must read captured value inputs only'
    )
assert 'UnboundedSender<HardwareLoopEvent>' not in store_src and 'UnboundedReceiver<HardwareLoopEvent>' not in store_src, (
    'R-C08 violation: Workbench hardware ingress must remain bounded with independent loss/control delivery'
)

six_layer_runtime_src = (root / 'crates/mi-core/src/six_layer_runtime.rs').read_text(encoding='utf-8').split('#[cfg(test)]\nmod tests {', 1)[0]
assert 'LiveHostActionExecutor::new' not in six_layer_runtime_src and '.driver()' not in store_src, (
    'R-C08 violation: six-layer and UI host actions must share the Store executor admission/cleanup boundary'
)
assert 'executor.execute_when(' in six_layer_runtime_src and 'executor.execute(' not in six_layer_runtime_src, (
    'R-C08 violation: six-layer admission must be checked inside the executor effect lock'
)

assert (root / 'docs/designs/DESIGN.md').is_file(), (
    'R-C09 violation: docs/designs/DESIGN.md must exist as the canonical visual design contract'
)
for view_path in sorted((root / 'app/MicroInput/Sources/MicroInputUI').glob('*.swift')):
    rel_view = view_path.relative_to(root)
    lines = view_path.read_text(encoding='utf-8').splitlines()
    for idx, line in enumerate(lines):
        assert '.controlSize(.mini)' not in line, (
            f'R-C09 violation in {rel_view}:{idx + 1}: .controlSize(.mini) is forbidden; use SurfaceButtonStyle or SelectDropdown'
        )
        assert '.buttonStyle(.borderedProminent)' not in line and '.buttonStyle(.bordered)' not in line, (
            f'R-C09 violation in {rel_view}:{idx + 1}: system .bordered/.borderedProminent button styles are forbidden; use SurfaceButtonStyle'
        )
        if re.search(r'\bPicker\s*\(', line):
            picker_window = '\n'.join(lines[idx : min(len(lines), idx + 21)])
            assert '.pickerStyle(.segmented)' in picker_window, (
                f'R-C09 violation in {rel_view}:{idx + 1}: non-segmented Picker is forbidden; use SelectDropdown'
            )
        if '.buttonStyle(.plain)' in line:
            focus_window = '\n'.join(lines[max(0, idx - 5) : min(len(lines), idx + 6)])
            assert '.focusEffectDisabled(' in focus_window, (
                f'R-C09 violation in {rel_view}:{idx + 1}: every .buttonStyle(.plain) control must attach .focusEffectDisabled()'
            )

forbidden_radial_copy = ('径向菜单', '摇杆菜单', '摇杆径向菜单')
ui_and_core_paths = (
    sorted((root / 'app/MicroInput/Sources/MicroInputUI').glob('*.swift'))
    + sorted((root / 'crates/mi-macos/src').rglob('*.rs'))
    + sorted((root / 'crates/mi-core/src').rglob('*.rs'))
)
for src_path in ui_and_core_paths:
    src_text = src_path.read_text(encoding='utf-8')
    for term in forbidden_radial_copy:
        assert term not in src_text, (
            f'R-C10 violation in {src_path.relative_to(root)}: forbidden legacy term {term!r}; '
            'use single-source 轮盘菜单 naming (WorkbenchDestination.radial / PassivePanelFrameSpec::RADIAL_TITLE)'
        )
classic_workbench_src = (root / 'app/MicroInput/Sources/MicroInputUI/ClassicWorkbenchView.swift').read_text(encoding='utf-8')
status_menu_src = (root / 'app/MicroInput/Sources/MicroInputUI/ModalsAndStatusMenuView.swift').read_text(encoding='utf-8')
assert 'case radial = "轮盘菜单"' in classic_workbench_src, (
    'R-C10 violation in app/MicroInput/Sources/MicroInputUI/ClassicWorkbenchView.swift: '
    'WorkbenchDestination.radial must declare rawValue "轮盘菜单"'
)
assert 'WorkbenchDestination.radial.rawValue' in status_menu_src, (
    'R-C10 violation in app/MicroInput/Sources/MicroInputUI/ModalsAndStatusMenuView.swift: '
    'StatusMenuPopoverView must reference WorkbenchDestination.radial.rawValue as single source of truth'
)

print('UNKNOWN inventory and read-only/no-send boundaries: PASS')
