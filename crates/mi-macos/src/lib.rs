pub mod app_cache;
pub mod ax_dictation;
pub mod codex_sessions;
pub mod executor;
pub mod instance_lock;
pub mod managed_controls;
mod native_ax;
pub mod passive_panel;
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
pub use executor::{
    is_antigravity_new_conversation_control, is_bundle_running, parse_pid_from_lsappinfo_output,
    select_antigravity_new_conversation_target, submit_modifiers_for_adapter,
    AntigravityNewConversationTarget, AntigravitySidebarAxNode, ClipboardItemSnapshot,
    ClipboardSnapshot, DialScrollRequest, HeldInputId, HostActionReceipt, HostOsDriver,
    LiveHostActionExecutor, MacOsSystemDriver, RecordedOsEvent, RecordingOsDriver,
    SimulatedHostActionExecutor,
};
pub use instance_lock::DocumentInstanceLock;
pub use managed_controls::CodexReasoningAction;
pub use native_ax::observe_codex_window_state;
pub use passive_panel::{
    PassivePanelEvent, PassivePanelFrameSpec, PassivePanelHost, PassivePanelRole,
    PassivePanelStyleSpec, RecordingPassivePanelHost, RecordingPassivePanelState, ScreenGeometry,
    ScreenPoint, ScreenRect, ScreenSize,
};
pub use scenarios::{
    run_config_write_scenario, run_diagnostic_export_scenario, run_host_execution_scenario,
    run_lighting_control_scenario, DiagnosticRedactor, ScenarioResultSummary,
};
pub use session_navigation::{
    codex_process_id, default_codex_home, navigate_codex_session,
    observe_antigravity_sidebar_conversations, observe_codex_threads_live,
    SidebarConversationObservation, SidebarRowVisualState,
};
pub use status_item::{
    PresetFileEnvelope, RecordingStatusItemHost, StatusIconPrimitive, StatusItemHost,
    StatusItemIconSpec, StatusItemSnapshot, MAX_PRESET_FILE_BYTES,
};
