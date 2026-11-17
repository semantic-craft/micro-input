pub mod app_cache;
pub mod ax_dictation;
pub mod codex_sessions;
pub mod diagnostics;
pub mod executor;
pub mod instance_lock;
pub mod managed_controls;
pub mod native_ax;
pub mod passive_panel;
pub mod private_fs;
#[cfg(feature = "qa-runner")]
pub mod scenarios;
pub mod session_navigation;
pub mod status_item;

pub use app_cache::{AppBundleCache, AppBundleInfo, OTHER_APPS_WILDCARD_BUNDLE_ID};
pub use ax_dictation::DictationAdapter;
pub use codex_sessions::{
    build_codex_thread_deeplink, observe_codex_threads, read_codex_thread_fingerprint,
    verify_codex_window_readback, CodexObservedStatus, CodexThreadFingerprint,
    CodexThreadObservation, CodexWindowSnapshot,
};
pub use diagnostics::{write_private_json, DiagnosticRedactor};
pub use executor::{
    is_antigravity_new_conversation_control, is_bundle_running, parse_pid_from_lsappinfo_output,
    redact_home_path, select_antigravity_new_conversation_target, submit_modifiers_for_adapter,
    AntigravityNewConversationTarget, AntigravitySidebarAxNode, ClipboardItemSnapshot,
    ClipboardSnapshot, DialScrollRequest, HeldInputId, HostActionReceipt, HostOsDriver,
    LiveHostActionExecutor, MacOsSystemDriver, MouseButtonKind, RecordedOsEvent, RecordingOsDriver,
    SimulatedHostActionExecutor,
};
pub use instance_lock::DocumentInstanceLock;
pub use managed_controls::{CodexReasoningAction, ModelPickerConfirmResult, SubagentOpenResult};
pub use native_ax::observe_codex_window_state;
pub use passive_panel::{
    NoopPassivePanelHost, PassivePanelEvent, PassivePanelFrameSpec, PassivePanelHost,
    PassivePanelRole, PassivePanelStyleSpec, ScreenGeometry, ScreenPoint, ScreenRect, ScreenSize,
};
#[cfg(any(test, feature = "test-util"))]
pub use passive_panel::{RecordingPassivePanelHost, RecordingPassivePanelState};
pub use private_fs::{
    ensure_private_dir, open_private_file, open_verified_readonly_file,
    verify_existing_private_dir, PrivateDirError, PrivateOpenMode,
};
#[cfg(feature = "qa-runner")]
pub use scenarios::{
    run_config_write_scenario, run_diagnostic_export_scenario, run_host_execution_scenario,
    run_lighting_control_scenario, ScenarioResultSummary,
};
pub use session_navigation::{
    codex_process_id, default_codex_home, navigate_codex_session,
    observe_antigravity_sidebar_conversations, observe_codex_threads_live,
    SidebarConversationObservation, SidebarRowVisualState,
};
#[cfg(any(test, feature = "test-util"))]
pub use status_item::RecordingStatusItemHost;
pub use status_item::{
    NoopStatusItemHost, PresetFileEnvelope, StatusIconPrimitive, StatusItemHost,
    StatusItemIconSpec, StatusItemSnapshot, MAX_PRESET_FILE_BYTES,
};
