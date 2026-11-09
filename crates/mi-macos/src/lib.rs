pub mod app_cache;
pub mod ax_dictation;
pub mod executor;
pub mod instance_lock;
pub mod passive_panel;
pub mod scenarios;
pub mod status_item;

pub use app_cache::{AppBundleCache, AppBundleInfo, OTHER_APPS_WILDCARD_BUNDLE_ID};
pub use ax_dictation::{
    is_valid_ax_leaf_on_no_value, AXDictationController, AxElementNode, AxTreeProvider,
    DictationAdapter, DictationOutcome, LockedDictationTarget,
};
pub use executor::{
    ClipboardItemSnapshot, ClipboardSnapshot, DialScrollRequest, HostActionReceipt, HostOsDriver,
    LiveHostActionExecutor, MacOsSystemDriver, RecordingOsDriver, SimulatedHostActionExecutor,
};
pub use instance_lock::DocumentInstanceLock;
pub use passive_panel::{
    PassivePanelEvent, PassivePanelFrameSpec, PassivePanelHost, PassivePanelRole,
    PassivePanelStyleSpec, RecordingPassivePanelHost, RecordingPassivePanelState, ScreenGeometry,
    ScreenPoint, ScreenRect, ScreenSize,
};
pub use scenarios::{
    run_config_write_scenario, run_diagnostic_export_scenario, run_host_execution_scenario,
    run_lighting_control_scenario, DiagnosticRedactor, ScenarioResultSummary,
};
pub use status_item::{
    PresetFileEnvelope, RecordingStatusItemHost, StatusIconPrimitive, StatusItemHost,
    StatusItemIconSpec, StatusItemSnapshot, MAX_PRESET_FILE_BYTES,
};
