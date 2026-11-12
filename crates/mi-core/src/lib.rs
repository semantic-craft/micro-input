pub mod canvas;
pub mod editors;
pub mod header;
pub mod inspector;
pub mod karabiner;
pub mod overlay;
pub mod qa_runner;
pub mod radial;
pub mod session_strip;
pub mod sidebar;
pub mod six_layer_runtime;
pub mod status_menu;
pub mod store;

pub use canvas::{
    format_concise_keycap_caption, CanvasAction, CanvasLayerTab, CanvasProjection, CanvasRect,
    ControlCellProjection, ControlSurfaceId, EncoderDialProjection, JoystickDirection8,
    JoystickDirectionBadge, JoystickPadProjection, KnobJoystickPreset,
    ProtectedLayerLockProjection, CANVAS_HEIGHT, CANVAS_PADDING, CANVAS_WIDTH, KEY_1U_SIZE,
    KEY_2U_WIDTH, KEY_GAP,
};
pub use editors::{
    ApplyChangeRowProjection, ApplyPreparationModalProjection, ComparisonRowProjection,
    EditorAction, EditorModalProjection, LightingEditorModalProjection, LightingZoneProjection,
    MacroEditorModalProjection, MacroStepDraft, MultiBranchKind, MultiBranchSlotProjection,
    MultiBranchTarget, MultiEditorModalProjection, ReconnectComparisonModalProjection,
};
pub use header::{HeaderAction, HeaderProjection, HeaderReadyState};
pub use inspector::{
    ActionLibraryRowProjection, CodexKeycapId, EditableSlotInspectorProjection, InspectorAction,
    InspectorBodyProjection, InspectorDrawer, InspectorProjection, KeycapCatalogItemProjection,
    KeycapDescriptor, KeycapRole, MacroLibraryRowProjection, ShortcutModifier,
    ShortcutRecorderState, INSPECTOR_WIDTH,
};
pub use mi_model::host_router::{BottomRowLayout, PopoverPreviewMode};
pub use overlay::{
    ActiveHudContent, CheatSheetKeyCellProjection, CheatSheetProjection, HostOverlayAction,
    HostOverlayProjection, HudVisibilityState, OverlayController, TargetAgentStatusCardProjection,
    PASSIVE_NOTICE_DURATION,
};
pub use qa_runner::{run_synthetic_scenario, QaLaunchEnv, SyntheticScenario9};
pub use radial::{
    octant_center_for_sector, resolve_radial_slots, strip_trailing_newlines, RadialConfirmMode,
    RadialConsumeOutcome, RadialInputEvent, RadialInputReadiness, RadialIntent, RadialMenuSession,
    RadialMenuState, RadialOctantPoint, RadialOverlayAction, RadialOverlayProjection, RadialPhase,
    RadialPositionSample, RadialPresetKind, RadialRouteStamp, RadialSlot, RadialSlotConfig,
    RadialSlotKind, RadialSlotResolutionContext, SectorIndex, ESCAPE_KEY_CODE,
    RADIAL_CANVAS_SIZE_PT, RADIAL_CARD_HEIGHT_PT, RADIAL_CARD_WIDTH_FACTOR,
    RADIAL_ORBIT_RADIUS_FACTOR, RADIAL_SECTOR_COUNT,
};
pub use session_strip::{
    AmbientColorPreset, FourAppLiveCandidate, FourAppSessionKind, FourAppSlotTable,
    LightingBrightnessPreset, LightingTimeoutPreset, SessionCapsulePillProjection,
    SessionSlotCardProjection, SessionStripAction, SessionStripProjection,
    SimpleLightingPopoverProjection,
};
pub use sidebar::{
    AutoSwitchState, BackupCommand, BoardLayerProjection, BoardLayersProjection,
    BoardProfileChoice, CanonicalAgent, OwnershipBannerState, ProfileCommand, SidebarAction,
    SidebarProfileRow, SidebarProjection,
};
pub use status_menu::{
    FavoriteSlotCardProjection, FavoriteSlotIndex, PendingPresetImportPreview, PinnedFavoritesGrid,
    PresetExportScope, StatusMenuAction, StatusMenuProjection, FAVORITE_SLOT_COUNT,
};
pub use store::{
    format_popover_cheat_label, DeviceWorkflowAction, HardwareAutoAcceptanceReceipt,
    HardwareLoopEvent, WorkbenchCommand, WorkbenchError, WorkbenchProjection, WorkbenchStore,
    WorkbenchStoreConfig,
};
