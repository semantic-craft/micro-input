use crate::lighting_arbiter::{FourAppAgentPhase, FourAppLightingArbiter, LightingUpdateResult};
use crate::{
    canvas::{
        format_concise_keycap_caption, CanvasAction, CanvasLayerTab, CanvasProjection, CanvasRect,
        ControlCellProjection, ControlSurfaceId, EncoderDialProjection, JoystickDirection8,
        JoystickDirectionBadge, JoystickPadProjection, KnobJoystickPreset,
        ProtectedLayerLockProjection, CANVAS_PADDING, KEY_1U_SIZE, KEY_2U_WIDTH, KEY_GAP,
    },
    editors::{
        ApplyChangeRowProjection, ApplyPreparationModalProjection, ComparisonRowProjection,
        EditorAction, EditorModalProjection, LightingEditorModalProjection, LightingZoneProjection,
        MacroEditorModalProjection, MacroStepDraft, MultiBranchKind, MultiBranchSlotProjection,
        MultiBranchTarget, MultiEditorModalProjection, MultiTargetOptionProjection,
        ReconnectComparisonModalProjection,
    },
    header::{
        CandidateDevice, HeaderAction, HeaderProjection, HeaderReadyState,
        PowerStatusBadgeProjection,
    },
    inspector::{
        ActionLibraryRowProjection, CodexKeycapId, EditableSlotInspectorProjection,
        InspectorAction, InspectorBodyProjection, InspectorDrawer, InspectorProjection,
        JoystickControlMode, KeycapCatalogItemProjection, KeycapRole, MacroLibraryRowProjection,
        ShortcutRecorderState, INSPECTOR_WIDTH,
    },
    overlay::{
        CheatSheetKeyCellProjection, CheatSheetNotificationKind, CheatSheetProjection,
        HostOverlayAction, HostOverlayProjection, HudVisibilityState, OverlayController,
        TargetAgentStatusCardProjection,
    },
    radial::{
        RadialConfirmMode, RadialConsumeOutcome, RadialInputEvent, RadialInputReadiness,
        RadialIntent, RadialMenuSession, RadialMenuState, RadialOverlayAction,
        RadialOverlayProjection, RadialPositionSample, RadialPresetKind, RadialRouteStamp,
        RadialSlot, RadialSlotConfig, RadialSlotResolutionContext, SectorIndex, ESCAPE_KEY_CODE,
        RADIAL_SECTOR_COUNT,
    },
    session_strip::{
        agent_for_place, FourAppLiveCandidate, FourAppSessionKind, FourAppSlotTable,
        SessionCapsulePillProjection, SessionSlotCardProjection, SessionStripAction,
        SessionStripProjection, SimpleLightingPopoverProjection,
    },
    sidebar::{
        BackupCommand, BoardLayerProjection, BoardLayersProjection, BoardProfileChoice,
        CanonicalAgent, OwnershipBannerState, ProfileCommand, SidebarAction, SidebarProfileRow,
        SidebarProjection,
    },
    status_menu::{
        FavoriteSlotCardProjection, FavoriteSlotIndex, PendingPresetImportPreview,
        PinnedFavoritesGrid, PresetExportScope, StatusMenuAction, StatusMenuProjection,
        FAVORITE_SLOT_COUNT,
    },
};
use mi_macos::{
    executor::APPLE_FN_KEY_CODE, AppBundleCache, AppBundleInfo, HeldInputId, HostActionReceipt,
    LiveHostActionExecutor, MacOsSystemDriver, NoopPassivePanelHost, NoopStatusItemHost,
    PassivePanelFrameSpec, PassivePanelHost, PassivePanelRole, PresetFileEnvelope, ScreenGeometry,
    ScreenPoint, StatusItemHost, StatusItemIconSpec, StatusItemSnapshot,
};
use mi_model::{
    action_library,
    board_layers::{
        BoardLayer, BoardLayerPlan, BoardLayerRoute, BoardPreviewSelection, NativeLayerOwner,
    },
    document::{LayerNode, LocalDocumentV2},
    draft::{Action, ActionKind, Draft},
    host_router::{
        resolve_host_action_with_layout, BottomRowLayout, ResolvedHostAction,
        CANONICAL_DESKTOP_AGENT_PROFILES,
    },
    lighting::{Edit as LightingEdit, Effect as LightingEffect, Lighting, Zone as LightingZone},
    macro_sequence::{LocalMacro, MacroEvent},
    multi::{ActionReference, LocalMulti},
    navigation_history::{NavigationHistory, NavigationPlace},
    slot_table::SLOT_COUNT,
    storage::{
        routing::{RoutingOperation, RoutingRequest},
        DocumentStore,
    },
};
use mi_session::{DeviceSession, InputEvent, InputEventKind};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};

mod firmware_store;
pub(crate) mod session_navigation;
mod six_layer_store;
pub(crate) mod v070_hardware_store;
mod workspace_lifecycle;
use session_navigation::{
    AntigravityInstance, CatalogInstance, NavigationCatalog, NavigationCommand,
    NavigationInputLease, SessionNavigation,
};
pub(crate) use workspace_lifecycle::SystemLifecycle;
mod projection;
pub use projection::format_popover_cheat_label;
use projection::{ProjectionInputs, ReusableInspectorCatalogs, SixLayerView};
pub(crate) mod runtime_inbox;
use runtime_inbox::RuntimeInbox;
pub(crate) mod runtime_stamp;
use runtime_stamp::RuntimeCommitStamp;

mod antigravity_daemon;
mod device_sync;
mod host_execution;
mod inspector_editor_actions;
pub(crate) mod lighting_preview_store;
#[cfg(test)]
mod runtime_consistency_tests;
#[cfg(test)]
mod tests;
mod workbench_actions;
#[cfg(test)]
pub(crate) use antigravity_daemon::classify_antigravity_trajectory_summaries;
use antigravity_daemon::ensure_private_document_dir;
pub(crate) use antigravity_daemon::poll_antigravity_daemon_candidates_with_frontmost;
pub use antigravity_daemon::{
    codex_observations_to_catalog, codex_observations_to_catalog_with_ack,
};
use inspector_editor_actions::resolve_merged_bottom_slot;
pub use lighting_preview_store::{
    LightingPreviewBeginFuture, LightingPreviewEndFuture, LightingPreviewHandle,
    LightingPreviewPhase, LightingPreviewStarter, NativeLightingPreviewStarter,
};
use lighting_preview_store::{LightingPreviewState, LightingPreviewStopReason};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareAutoAcceptanceReceipt {
    pub schema_version: u32,
    pub status: String,
    pub session_id: u64,
    pub backend: String,
    #[serde(default)]
    pub exclusive_vendor: bool,
    pub product: String,
    pub vid_pid: String,
    pub device_path: String,
    pub firmware_version: String,
    pub profile_index: Option<u32>,
    pub layer_index: Option<u32>,
    pub keymap_size: u64,
    pub keymap_sha1: String,
    pub keymap_sha256: String,
    pub smart_actions_present: bool,
    pub smart_actions_size: Option<u64>,
    pub smart_actions_sha1: Option<String>,
    pub lighting_rgbcfg_synced: bool,
    pub lighting_preview_synced: bool,
    pub lighting_thstatus_synced: bool,
    pub timestamp_ms: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveSessionSource {
    Antigravity,
    Codex,
}

impl LiveSessionSource {
    pub fn agent(self) -> CanonicalAgent {
        match self {
            Self::Antigravity => CanonicalAgent::Antigravity,
            Self::Codex => CanonicalAgent::Codex,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LiveSessionSourceState {
    source: LiveSessionSource,
    generation: u64,
}

#[derive(Debug)]
pub enum HardwareLoopEvent {
    Input(InputEvent),
    Disconnected {
        session_id: u64,
    },
    DiscoveryChanged {
        epoch: u64,
    },

    HardwareAutoAccepted {
        session_id: u64,
        receipt: HardwareAutoAcceptanceReceipt,
    },
    LiveCandidatesUpdated {
        epoch: u64,
        source: LiveSessionSource,
        catalog: Option<NavigationCatalog>,
    },
    V070CapabilityOutcome {
        session_id: u64,
        seq: u64,
        result: Result<crate::v070_hardware::V070BridgeOutcome, String>,
    },
    V070ProfileLayerChanged {
        session_id: u64,
        profile_index: u8,
        layer_index: u8,
    },
    CheatSheetNotification {
        session_id: u64,
        ingress_sequence: u64,
        kind: CheatSheetNotificationKind,
    },
    EncoderHoldTimer {
        session_id: u64,
        hold_generation: u64,
    },
    LightingPreviewStarted {
        generation: u64,
        result: Result<Box<dyn LightingPreviewHandle>, String>,
    },
    LightingPreviewEnded {
        generation: u64,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BoardRouting {
    Legacy,
    Checking,
    Managed { layer: u8 },
    WaitingForFirstLayer,
    Unverified,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WorkbenchError {
    #[error("{0}")]
    Storage(String),
    #[error("{0}")]
    Routing(String),
    #[error("{0}")]
    Preset(String),
    #[error("受保护宿主层（原生 OAI）已锁定，不支持修改按键、预设或灯光")]
    ProtectedLayerLocked,
}

#[derive(Clone)]
pub struct WorkbenchStoreConfig {
    pub document_dir: PathBuf,
    pub auto_connect_usb: bool,
    pub isolated: bool,
    pub system_locale: mi_model::ResolvedLocale,
    pub app_cache: Arc<AppBundleCache>,
    pub panel_host: Arc<dyn PassivePanelHost>,
    pub status_item_host: Arc<dyn StatusItemHost>,
    pub lighting_preview_starter: Arc<dyn LightingPreviewStarter>,
}

impl WorkbenchStoreConfig {
    pub fn production_document_dir() -> PathBuf {
        env::var("MI_DOCUMENT_DIR")
            .ok()
            .map(|raw| raw.trim().to_string())
            .filter(|trimmed| !trimmed.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                env::var_os("HOME")
                    .map(|home| PathBuf::from(home).join("Library/Application Support/MicroInput"))
            })
            .unwrap_or_else(|| PathBuf::from(".micro-input"))
    }

    pub fn production() -> Self {
        let document_dir = Self::production_document_dir();
        Self {
            document_dir,
            auto_connect_usb: true,
            isolated: false,
            system_locale: mi_model::ResolvedLocale::ZhHans,
            app_cache: Arc::new(AppBundleCache::new()),
            panel_host: Arc::new(NoopPassivePanelHost::new()),
            status_item_host: Arc::new(NoopStatusItemHost::new()),
            lighting_preview_starter: Arc::new(NativeLightingPreviewStarter),
        }
    }

    pub fn isolated(dir: &Path) -> Self {
        let document_dir = dir.to_path_buf();
        let app_cache = Arc::new(AppBundleCache::with_resolver(Arc::new(|bundle_id| {
            let installed = CanonicalAgent::from_bundle_id(bundle_id).is_some();
            AppBundleInfo {
                bundle_id: bundle_id.to_string(),
                installed,
                app_path: None,
                display_name: CanonicalAgent::from_bundle_id(bundle_id)
                    .map(|a| a.display_name().to_string()),
                icon_cached: false,
            }
        })));
        Self {
            document_dir,
            auto_connect_usb: false,
            isolated: true,
            system_locale: mi_model::ResolvedLocale::ZhHans,
            app_cache,
            panel_host: Arc::new(NoopPassivePanelHost::new()),
            status_item_host: Arc::new(NoopStatusItemHost::new()),
            lighting_preview_starter: Arc::new(NativeLightingPreviewStarter),
        }
    }

    pub fn workbench_json_path(&self) -> PathBuf {
        self.document_dir.join("workbench.json")
    }

    pub fn hardware_auto_acceptance_path(&self) -> PathBuf {
        self.document_dir.join("hardware-auto-acceptance.json")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceWorkflowAction {
    ReadIdentity,
    ReadSnapshot,
    ExportDiagnostics,
    OpenReconnectComparison,
    ConfirmComparisonIdentity,
    ResolveComparison(mi_model::comparison::Choice),
    OpenApplyPreparation,
    PrepareApplyBackup,
    TakePreparedCandidate,
    CancelApplyPreview,
    SimulateKeyPress { binding_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "domain", content = "action", rename_all = "snake_case")]
pub enum WorkbenchCommand {
    Firmware(crate::firmware_update::FirmwareAction),
    #[serde(rename = "v070_hardware")]
    V070Hardware(crate::v070_hardware::V070HardwareAction),
    SystemWillSleep,
    SystemDidWake,
    SystemWillTerminate,
    Header(HeaderAction),
    Sidebar(SidebarAction),
    Canvas(CanvasAction),
    SessionStrip(SessionStripAction),
    Inspector(InspectorAction),
    Editor(EditorAction),
    Radial(RadialOverlayAction),
    Overlay(HostOverlayAction),
    StatusMenu(StatusMenuAction),
    Device(DeviceWorkflowAction),
    SetRadialSlotConfig {
        sector: u8,
        config: RadialSlotConfig,
    },
    ForegroundAppChanged {
        bundle_id: Option<String>,
    },
    MainWindowActiveChanged {
        active: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkbenchProjection {
    pub firmware: crate::firmware_update::FirmwareProjection,
    pub v070_hardware: crate::v070_hardware::V070HardwareProjection,
    pub header: HeaderProjection,
    pub sidebar: SidebarProjection,
    pub canvas: CanvasProjection,
    pub session_strip: SessionStripProjection,
    pub inspector: InspectorProjection,
    pub radial: RadialOverlayProjection,
    pub radial_slot_configs: [RadialSlotConfig; RADIAL_SECTOR_COUNT],
    pub overlay: HostOverlayProjection,
    pub status_menu: StatusMenuProjection,
    pub status_menu_open: bool,
    pub active_modal: Option<EditorModalProjection>,
    pub selected_profile_id: Option<String>,
    pub selected_layer_id: Option<String>,
    pub selected_binding_slot: Option<u32>,
    pub status_message: String,
    pub last_exported_preset: Option<String>,
    #[serde(default)]
    pub last_exported_diagnostics_path: Option<String>,
    pub device_connected: bool,
    pub device_summary: String,
    pub document_revision: u64,
    pub host_revision: u64,
    pub banner_flash_count: usize,
    pub last_switch_duration: Duration,
}

impl Default for WorkbenchProjection {
    fn default() -> Self {
        Self {
            firmware: Default::default(),
            v070_hardware: Default::default(),
            header: HeaderProjection::default(),
            sidebar: SidebarProjection::default(),
            canvas: CanvasProjection::default(),
            session_strip: SessionStripProjection::default(),
            inspector: InspectorProjection::default(),
            radial: RadialOverlayProjection::default(),
            radial_slot_configs: RadialSlotConfig::default_app_launcher_slots(),
            overlay: HostOverlayProjection::default(),
            status_menu: StatusMenuProjection::default(),
            status_menu_open: false,
            active_modal: None,
            selected_profile_id: None,
            selected_layer_id: None,
            selected_binding_slot: None,
            status_message: "已就绪".to_string(),
            last_exported_preset: None,
            last_exported_diagnostics_path: None,
            device_connected: false,
            device_summary: "Creator Micro 2 (303A:8298) · 离线草稿模式".to_string(),
            document_revision: 0,
            host_revision: 0,
            banner_flash_count: 0,
            last_switch_duration: Duration::ZERO,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DictationControllerState {
    pub is_active: bool,
    pub cleanup_confirmed: bool,
    #[cfg(test)]
    next_verify_stopped: bool,
}

impl Default for DictationControllerState {
    fn default() -> Self {
        Self {
            is_active: false,
            cleanup_confirmed: true,
            #[cfg(test)]
            next_verify_stopped: true,
        }
    }
}

impl DictationControllerState {
    pub fn cancel(&mut self) {
        self.is_active = false;
    }

    pub fn verify_stopped(&mut self) -> bool {
        #[cfg(test)]
        let stopped = self.next_verify_stopped;
        #[cfg(not(test))]
        let stopped = true;
        self.cleanup_confirmed = stopped;
        stopped
    }

    #[cfg(test)]
    pub fn set_next_verify_stopped(&mut self, stopped: bool) {
        self.next_verify_stopped = stopped;
    }
}

pub(crate) const HOST_CAUTION_TTL: Duration = Duration::from_secs(12);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HostActionCaution {
    pub detail_code: String,
    pub is_sticky_cleanup: bool,
    pub expires_at: Option<Instant>,
}

#[derive(Clone)]
struct HeldDictation {
    input: HeldInputId,
    profile_id: String,
    adapter: String,
}

pub struct WorkbenchStore {
    pub(crate) firmware_update: crate::firmware_update::FirmwareUpdater,
    pub config: WorkbenchStoreConfig,
    pub document_store: DocumentStore,
    pub ptt_controller: DictationControllerState,
    pub lighting_arbiter: FourAppLightingArbiter,
    pub slot_table: FourAppSlotTable,
    pub codex_slot_table: FourAppSlotTable,
    pub app_cache: Arc<AppBundleCache>,
    pub panel_host: Arc<dyn PassivePanelHost>,
    pub status_item_host: Arc<dyn StatusItemHost>,
    pub device_session: Option<Arc<DeviceSession>>,
    pub connected_candidate: Option<mi_transport::HidCandidate>,
    pub available_usb_candidates: Vec<mi_transport::HidCandidate>,
    pub hardware_rpc_verified: bool,
    pub last_hardware_auto_acceptance: Option<HardwareAutoAcceptanceReceipt>,
    pub merge_wide_key: Arc<AtomicBool>,
    board_routing: BoardRouting,
    board_decoder: Arc<AtomicBool>,
    board_admit_after: u64,
    six_layer_state: crate::six_layer_runtime::SavedSixLayerState,
    six_layer_running: Option<crate::six_layer_runtime::RunningSixLayer>,
    six_layer_job: Option<
        std::sync::mpsc::Receiver<
            Result<Option<crate::six_layer_runtime::RunningSixLayer>, String>,
        >,
    >,
    six_layer_auto_resume_attempts: u8,
    six_layer_next_auto_resume_at: Option<Instant>,
    six_layer_user_pinned: bool,
    last_synced_six_layer_revs: (u64, u64),
    v070_hardware: v070_hardware_store::V070HardwareController,
    sa_rgb_coordinator: crate::v070_session_sa_rgb::SaRgbRuntimeCoordinator,
    accessibility_trusted: bool,
    accessibility_trusted_override: Option<bool>,
    last_accessibility_check_at: Instant,
    last_competing_app_check_at: Instant,
    foreground_bundle_id: Option<String>,
    tokio_runtime: Arc<tokio::runtime::Runtime>,
    hardware_event_tx: RuntimeInbox,
    mac_watcher_transport: Option<Arc<mi_transport::MacTransport>>,
    system_lifecycle: SystemLifecycle,
    lifecycle_epoch: Arc<AtomicU64>,
    lifecycle_cleanup: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    discovery_enabled: bool,
    discovery_refresh_count: u64,
    resume_requires_explicit_connect: bool,
    pub navigation_history: NavigationHistory,
    session_navigation: SessionNavigation,
    live_session_source: Arc<(Mutex<LiveSessionSourceState>, Condvar)>,
    pub radial_state: RadialMenuState,
    pub radial_readiness: RadialInputReadiness,
    pub radial_slot_configs: [RadialSlotConfig; RADIAL_SECTOR_COUNT],
    pub overlay_controller: OverlayController,
    last_cheat_sheet_notif: Option<(u64, u64)>,
    pub pinned_favorites: PinnedFavoritesGrid,
    pub screen_geometry: ScreenGeometry,
    pub last_cursor_position: ScreenPoint,
    pub pending_preset_import: Option<PendingPresetImportPreview>,
    pub pending_preset_export_scope: Option<PresetExportScope>,
    pub preset_transfer_error: Option<String>,
    pub last_host_action_receipt: Option<HostActionReceipt>,
    pub dispatched_receipts: Vec<HostActionReceipt>,
    pub(crate) host_caution: Option<HostActionCaution>,
    pub live_executor: Arc<LiveHostActionExecutor<MacOsSystemDriver>>,
    pub encoder_model_picker: crate::encoder_model_picker::EncoderModelPicker,
    encoder_hold_profile_id: Option<String>,
    model_picker_mouse_capture_enabled: bool,
    dial_pressed: bool,
    dial_turned_while_pressed: bool,
    held_dictation: Option<HeldDictation>,
    selected_profile_id: Option<String>,
    selected_layer_id: Option<String>,
    selected_binding_slot: Option<u32>,
    selected_surface: ControlSurfaceId,
    selected_joystick_direction: JoystickDirection8,
    bottom_row_layout: BottomRowLayout,
    show_keycap_icons: bool,
    show_keycap_labels: bool,
    lighting_popover: SimpleLightingPopoverProjection,
    show_other_profiles: bool,
    busy: bool,
    status_message: String,
    last_exported_preset: Option<String>,
    last_exported_diagnostics_path: Option<String>,
    banner_flash_count: usize,
    last_switch_duration: Duration,
    keycap_overrides: BTreeMap<(String, u32), CodexKeycapId>,
    slot_role_overrides: BTreeMap<(String, u32), KeycapRole>,
    keycap_search_query: String,
    bind_keycap_default_action_on_select: bool,
    shortcut_recorder: ShortcutRecorderState,
    action_search_query: String,
    action_category_filter: String,
    favorites_only: bool,
    favorite_action_ids: BTreeSet<String>,
    open_drawers: Vec<InspectorDrawer>,
    command_execution_enabled: bool,
    lighting_apply_to_all_layers: bool,
    lighting_preview_active: bool,
    lighting_preview_remaining_secs: u8,
    lighting_preview_state: LightingPreviewState,
    lighting_preview_generation: u64,
    status_menu_open: bool,
    active_modal: Option<EditorModalProjection>,
    last_status_item_snapshot: Option<StatusItemSnapshot>,
    last_hud_panel: Option<Option<PassivePanelFrameSpec>>,
    last_radial_panel: Option<Option<PassivePanelFrameSpec>>,
    applications: BTreeMap<String, bool>,
    six_layer_view: Option<SixLayerView>,
    captured_app_cache_stamp: (usize, u64),
    last_committed_stamp: Option<RuntimeCommitStamp>,
    #[cfg(test)]
    pub(crate) commit_local_update_count: u64,
    update_now: Instant,
    input_lost_session: Option<u64>,
    projection_generation: u64,
    projection: WorkbenchProjection,
}

impl WorkbenchStore {
    pub fn open(config: WorkbenchStoreConfig) -> Result<Self, WorkbenchError> {
        let now = Instant::now();
        ensure_private_document_dir(&config.document_dir)?;
        let mut document_store = DocumentStore::default();
        let workbench_path = config.workbench_json_path();
        if !config.isolated || workbench_path.exists() {
            document_store
                .open(workbench_path)
                .map_err(WorkbenchError::Storage)?;
        }

        let preserve_saved_host_state = document_store.has_saved_host_preferences();
        let six_layer_state =
            crate::six_layer_runtime::load(&config.document_dir.join("six-layer-runtime.json"))
                .map_err(WorkbenchError::Storage)?;
        let app_cache = config.app_cache.clone();
        let panel_host = config.panel_host.clone();
        let status_item_host = config.status_item_host.clone();
        let tokio_runtime = if config.isolated {
            static ISOLATED_RUNTIME: std::sync::OnceLock<Arc<tokio::runtime::Runtime>> =
                std::sync::OnceLock::new();
            Arc::clone(ISOLATED_RUNTIME.get_or_init(|| {
                Arc::new(
                    tokio::runtime::Builder::new_multi_thread()
                        .worker_threads(4)
                        .enable_all()
                        .build()
                        .expect("isolated tokio runtime"),
                )
            }))
        } else {
            Arc::new(
                tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                    .map_err(|e| WorkbenchError::Storage(format!("创建异步运行时失败：{e}")))?,
            )
        };
        let hardware_event_tx = RuntimeInbox::default();
        let merge_wide_key = Arc::new(AtomicBool::new(false));

        let isolated = config.isolated;
        let mut store = Self {
            firmware_update: Default::default(),
            config,
            document_store,
            ptt_controller: DictationControllerState::default(),
            lighting_arbiter: FourAppLightingArbiter::new(),
            slot_table: FourAppSlotTable::new(),
            codex_slot_table: FourAppSlotTable::new(),
            app_cache,
            panel_host,
            status_item_host,
            device_session: None,
            connected_candidate: None,
            available_usb_candidates: Vec::new(),
            hardware_rpc_verified: false,
            last_hardware_auto_acceptance: None,
            merge_wide_key,
            tokio_runtime,
            hardware_event_tx,
            mac_watcher_transport: None,
            system_lifecycle: SystemLifecycle::Awake,
            lifecycle_epoch: Arc::new(AtomicU64::new(0)),
            lifecycle_cleanup: None,
            discovery_enabled: false,
            discovery_refresh_count: 0,
            resume_requires_explicit_connect: false,
            navigation_history: NavigationHistory::new(),
            session_navigation: SessionNavigation::default(),
            live_session_source: Arc::new((
                Mutex::new(LiveSessionSourceState {
                    source: LiveSessionSource::Antigravity,
                    generation: 0,
                }),
                Condvar::new(),
            )),
            radial_state: RadialMenuState::new(),
            radial_readiness: RadialInputReadiness::Ready,
            radial_slot_configs: RadialSlotConfig::default_app_launcher_slots(),
            overlay_controller: OverlayController::new(),
            last_cheat_sheet_notif: None,
            pinned_favorites: PinnedFavoritesGrid::default(),
            screen_geometry: ScreenGeometry::default(),
            last_cursor_position: ScreenPoint::default(),
            pending_preset_import: None,
            pending_preset_export_scope: None,
            preset_transfer_error: None,
            last_host_action_receipt: None,
            dispatched_receipts: Vec::new(),
            host_caution: None,
            live_executor: Arc::new(LiveHostActionExecutor::new(if isolated {
                MacOsSystemDriver::new_isolated()
            } else {
                MacOsSystemDriver::default()
            })),
            encoder_model_picker: crate::encoder_model_picker::EncoderModelPicker::new(),
            encoder_hold_profile_id: None,
            model_picker_mouse_capture_enabled: false,
            dial_pressed: false,
            dial_turned_while_pressed: false,
            held_dictation: None,
            board_routing: BoardRouting::Legacy,
            board_decoder: Arc::new(AtomicBool::new(false)),
            board_admit_after: 0,
            six_layer_state,
            six_layer_running: None,
            six_layer_job: None,
            six_layer_auto_resume_attempts: 0,
            six_layer_next_auto_resume_at: None,
            six_layer_user_pinned: false,
            last_synced_six_layer_revs: (0, 0),
            v070_hardware: v070_hardware_store::V070HardwareController::default(),
            sa_rgb_coordinator: crate::v070_session_sa_rgb::SaRgbRuntimeCoordinator::new(),
            accessibility_trusted: true,
            accessibility_trusted_override: None,
            last_accessibility_check_at: now,
            last_competing_app_check_at: now,
            foreground_bundle_id: None,
            selected_profile_id: None,
            selected_layer_id: None,
            selected_binding_slot: Some(0),
            selected_surface: ControlSurfaceId::Key0,
            selected_joystick_direction: JoystickDirection8::North,
            bottom_row_layout: BottomRowLayout::Split,
            show_keycap_icons: true,
            show_keycap_labels: true,
            lighting_popover: SimpleLightingPopoverProjection::default(),
            show_other_profiles: false,
            busy: false,
            status_message: "已就绪".to_string(),
            last_exported_preset: None,
            last_exported_diagnostics_path: None,
            banner_flash_count: 0,
            last_switch_duration: Duration::ZERO,
            keycap_overrides: BTreeMap::new(),
            slot_role_overrides: BTreeMap::new(),
            keycap_search_query: String::new(),
            bind_keycap_default_action_on_select: true,
            shortcut_recorder: ShortcutRecorderState::default(),
            action_search_query: String::new(),
            action_category_filter: "全部".to_string(),
            favorites_only: false,
            favorite_action_ids: BTreeSet::new(),
            open_drawers: Vec::new(),
            command_execution_enabled: false,
            lighting_apply_to_all_layers: false,
            lighting_preview_active: false,
            lighting_preview_remaining_secs: 0,
            lighting_preview_state: LightingPreviewState::Idle,
            lighting_preview_generation: 0,
            status_menu_open: false,
            active_modal: None,
            last_status_item_snapshot: None,
            last_hud_panel: None,
            last_radial_panel: None,
            applications: BTreeMap::new(),
            six_layer_view: None,
            captured_app_cache_stamp: (0, 0),
            last_committed_stamp: None,
            #[cfg(test)]
            commit_local_update_count: 0,
            update_now: now,
            input_lost_session: None,
            projection_generation: 0,
            projection: WorkbenchProjection::default(),
        };

        store.sync_routing_snapshot();
        store.prepare_desktop_profiles()?;
        let runtime_path = store.config.document_dir.join("six-layer-runtime.json");
        if store
            .six_layer_state
            .migrate_saved_encoder_defaults(&store.document_store.draft.document)
        {
            let _ = crate::six_layer_runtime::save(&runtime_path, &store.six_layer_state);
        }
        store.ensure_default_pinned_favorites();
        store.initialize_board_plan()?;
        store.initialize_selection(preserve_saved_host_state)?;
        store.status_message = "已就绪".to_string();
        if store.config.auto_connect_usb
            && env::var("MI_AUTO_HOST_READY").ok().as_deref() != Some("0")
        {
            store.lighting_popover.only_glow_on_attention_or_completed = false;
            store.lighting_arbiter.only_glow_on_attention_or_completed = false;
            store.initialize_auto_host_ready(preserve_saved_host_state);
            store.sync_live_session_source();
            store.start_mac_usb_discovery_watcher();
            store.start_antigravity_live_poller();
            let _ = store.connect_default_usb_with_execution(false);
        } else {
            store.sync_live_session_source();
        }
        if !store.config.isolated && !store.document_store.host_execution_enabled() {
            store.status_message = "已暂停软件按键映射".to_string();
        }
        store.last_synced_six_layer_revs = (
            store.document_store.draft.revision,
            store.document_store.host_revision(),
        );
        store.refresh_accessibility_trusted();
        store.refresh_competing_input_app_running(true);
        store.refresh_four_app_lighting();
        let initial_locale = store.effective_locale();
        if initial_locale != mi_model::ResolvedLocale::ZhHans {
            store.apply_locale_transition(initial_locale);
        }
        store.commit_runtime_update();
        Ok(store)
    }

    pub fn set_accessibility_trusted_for_test(&mut self, trusted: Option<bool>) {
        self.accessibility_trusted_override = trusted;
        let _ = self.refresh_accessibility_trusted();
        self.commit_runtime_update();
    }

    pub fn set_competing_input_app_running_for_test(&mut self, running: bool) {
        self.applications
            .insert("com.worklouder.input".to_string(), running);
        self.commit_runtime_update();
    }

    fn refresh_accessibility_trusted(&mut self) -> bool {
        let next = if let Some(override_val) = self.accessibility_trusted_override {
            override_val
        } else if !self.config.isolated && self.config.auto_connect_usb {
            self.live_executor.is_accessibility_trusted()
        } else {
            true
        };
        let changed = self.accessibility_trusted != next;
        self.accessibility_trusted = next;
        changed
    }

    fn refresh_competing_input_app_running(&mut self, force: bool) -> bool {
        if self.config.isolated || !self.config.auto_connect_usb {
            return false;
        }
        if !force
            && self
                .update_now
                .saturating_duration_since(self.last_competing_app_check_at)
                < Duration::from_secs(5)
        {
            return false;
        }
        self.last_competing_app_check_at = self.update_now;
        let running = mi_macos::is_bundle_running("com.worklouder.input");
        self.applications
            .insert("com.worklouder.input".to_string(), running)
            != Some(running)
    }

    pub fn open_accessibility_settings(&mut self) {
        let _ = self.refresh_accessibility_trusted();
        let (outcome, detail) = if !self.config.isolated && self.config.auto_connect_usb {
            match self.live_executor.open_accessibility_settings() {
                Ok(()) => ("executed", "已打开 macOS 辅助功能权限设置".to_string()),
                Err(error) => ("failed", format!("打开辅助功能设置失败 ({error})")),
            }
        } else {
            ("simulated", "已打开 macOS 辅助功能权限设置".to_string())
        };
        let profile_id = self.resolved_active_profile_id().unwrap_or_default();
        let receipt = HostActionReceipt {
            action_type: "open_accessibility_settings".to_string(),
            binding_id: "accessibility-settings".to_string(),
            profile_id,
            target: "com.apple.systempreferences.accessibility".to_string(),
            outcome: outcome.to_string(),
            detail: detail.clone(),
            text_byte_len: None,
            timestamp_ms: 0,
        };
        self.status_message = detail;
        self.record_host_action_receipt(receipt);
        self.commit_runtime_update();
    }

    pub fn set_profile_locked(
        &mut self,
        locked: bool,
        prefer_selected: bool,
    ) -> Result<(), WorkbenchError> {
        if locked {
            let profiles = &self
                .document_store
                .host_store()
                .draft
                .document
                .editable
                .graph
                .profiles;
            let selected_valid = self
                .selected_profile_id
                .clone()
                .filter(|pid| profiles.contains_key(pid));
            let active_valid = self
                .resolved_active_profile_id()
                .filter(|pid| profiles.contains_key(pid));
            let target_pid = if prefer_selected {
                selected_valid.or(active_valid)
            } else {
                active_valid.or(selected_valid)
            }
            .or_else(|| self.find_canonical_profile_id(CanonicalAgent::Antigravity))
            .or_else(|| {
                self.document_store
                    .host_store()
                    .draft
                    .document
                    .editable
                    .graph
                    .profile_order
                    .first()
                    .cloned()
            })
            .ok_or_else(|| WorkbenchError::Routing("没有可锁定的方案".to_string()))?;

            let first_layer_id = profiles
                .get(&target_pid)
                .and_then(|p| p.layer_order.first().cloned());

            self.apply_routing_operation(RoutingOperation::LockProfile {
                profile_id: Some(target_pid.clone()),
            })?;
            if prefer_selected || self.selected_profile_id.is_none() {
                let context_changed = self.selected_profile_id.as_deref()
                    != Some(target_pid.as_str())
                    || self.selected_layer_id != first_layer_id;
                self.selected_profile_id = Some(target_pid);
                self.selected_layer_id = first_layer_id;
                self.normalize_surface_for_current_layer();
                if context_changed {
                    self.sync_lighting_preview_after_document_change();
                }
            }
            let name = self
                .projection_inputs()
                .active_profile_canonical_display_name();
            self.status_message = format!("已锁定方案「{name}」");
        } else {
            self.apply_routing_operation(RoutingOperation::LockProfile { profile_id: None })?;
            self.six_layer_user_pinned = false;
            self.status_message = "已解除方案锁定".to_string();
        }
        self.sync_six_layer_routing_context();
        self.sync_live_session_source();
        self.refresh_four_app_lighting();
        if locked {
            let _ = self.maybe_sync_v070_active_profile_layer();
        } else {
            self.v070_hardware.pending_auto_layer_sync = None;
        }
        self.commit_runtime_update();
        Ok(())
    }

    pub fn poll_hardware_events(&mut self) -> bool {
        self.poll_at(Instant::now())
    }

    pub fn poll_at(&mut self, now: Instant) -> bool {
        self.update_now = now;
        let before_gen = self.projection_generation;
        let mut dirty = self.firmware_update.poll();
        if now.saturating_duration_since(self.last_accessibility_check_at) >= Duration::from_secs(1)
        {
            self.last_accessibility_check_at = now;
            dirty |= self.refresh_accessibility_trusted();
        }
        dirty |= self.refresh_competing_input_app_running(false);
        self.poll_workspace_lifecycle();
        if self.system_lifecycle == SystemLifecycle::Awake && !self.firmware_update.reserved() {
            self.poll_six_layer_runtime();
        }
        let _ = self.sync_live_session_source();
        let _ = self.drain_pending_hardware_events(now);
        dirty |= self.poll_lighting_preview(now);
        dirty |= self.poll_encoder_model_picker(now);
        dirty |= self.poll_session_navigation_with_commit(false);
        dirty |= self.overlay_controller.expire_notice_if_due(now);
        if self
            .host_caution
            .as_ref()
            .is_some_and(|c| !c.is_sticky_cleanup && c.expires_at.is_some_and(|exp| now >= exp))
        {
            self.host_caution = None;
            dirty = true;
        }
        if dirty || self.external_runtime_dirty() {
            self.commit_runtime_update();
        }
        self.projection_generation != before_gen
    }

    pub fn six_layer_state(&self) -> &crate::six_layer_runtime::SavedSixLayerState {
        &self.six_layer_state
    }

    pub fn is_model_picker_mouse_capture_enabled(&self) -> bool {
        self.model_picker_mouse_capture_enabled
    }

    pub fn inject_model_picker_mouse_delta_for_test(&self, dx: f64, dy: f64) {
        self.live_executor.inject_model_picker_mouse_delta(dx, dy);
    }

    pub(crate) fn set_model_picker_mouse_capture(&mut self, enabled: bool) {
        if self.model_picker_mouse_capture_enabled != enabled {
            self.model_picker_mouse_capture_enabled = enabled;
            self.live_executor.set_model_picker_mouse_capture(enabled);
        } else if !enabled {
            self.live_executor.set_model_picker_mouse_capture(false);
        }
    }

    pub(crate) fn cancel_encoder_model_picker(&mut self) {
        self.encoder_model_picker.cancel();
        self.encoder_hold_profile_id = None;
        self.model_picker_mouse_capture_enabled = false;
        self.live_executor.set_model_picker_mouse_capture(false);
    }

    fn poll_encoder_model_picker(&mut self, now: Instant) -> bool {
        if self.system_lifecycle != SystemLifecycle::Awake
            || !self.document_store.host_execution_enabled()
        {
            if self.encoder_model_picker.hold_state().is_some()
                || self.encoder_model_picker.active_session().is_some()
                || self.model_picker_mouse_capture_enabled
            {
                self.cancel_encoder_model_picker();
                return true;
            }
            return false;
        }

        let mut dirty = false;
        if self.encoder_model_picker.expire_if_idle(now).is_some() {
            self.set_model_picker_mouse_capture(false);
            dirty = true;
        }

        if let Some(hold) = self.encoder_model_picker.hold_state().cloned() {
            if !hold.hold_fired
                && !hold.turned_while_pressed
                && now.saturating_duration_since(hold.pressed_at)
                    >= crate::encoder_model_picker::ENCODER_HOLD_THRESHOLD
            {
                let (_, hold_dirty) = self.fire_due_encoder_hold_with_commit(
                    hold.session_id,
                    hold.hold_generation,
                    now,
                    false,
                );
                dirty |= hold_dirty;
            }
        }

        if self.encoder_model_picker.is_active(now) {
            let deltas = self.live_executor.drain_model_picker_mouse_deltas();
            for (dx, dy) in deltas {
                if let Some(cmd) = self.encoder_model_picker.on_mouse_delta(dx, dy, now) {
                    let Some(adapter) = self
                        .encoder_model_picker
                        .active_adapter(now)
                        .map(str::to_owned)
                    else {
                        continue;
                    };
                    let profile_id = self.resolved_active_profile_id().unwrap_or_default();
                    let action = cmd.into_host_action(adapter);
                    let _ = self.execute_resolved_physical_action_with_session_commit(
                        &profile_id,
                        "ENC_CLK",
                        0,
                        action,
                        false,
                    );
                    dirty = true;
                }
            }
        }
        dirty
    }

    pub fn projection_generation(&self) -> u64 {
        self.projection_generation
    }

    pub fn dispatch_command(&mut self, command: WorkbenchCommand) -> Result<(), WorkbenchError> {
        let started = Instant::now();
        let previous_profile = self.document_store.host_active_profile_id();
        let result = self.dispatch_at(command, started);
        if self.document_store.host_active_profile_id() != previous_profile {
            let elapsed = started.elapsed();
            self.last_switch_duration = elapsed;
            self.projection.last_switch_duration = elapsed;
        }
        result
    }

    pub fn dispatch_at(
        &mut self,
        command: WorkbenchCommand,
        now: Instant,
    ) -> Result<(), WorkbenchError> {
        self.update_now = now;
        if self.firmware_update.reserved()
            && !matches!(
                command,
                WorkbenchCommand::Firmware(_)
                    | WorkbenchCommand::SystemWillSleep
                    | WorkbenchCommand::SystemDidWake
                    | WorkbenchCommand::SystemWillTerminate
                    | WorkbenchCommand::ForegroundAppChanged { .. }
                    | WorkbenchCommand::MainWindowActiveChanged { .. }
            )
        {
            return Err(WorkbenchError::Routing(
                "固件更新期间设备控制已暂停；请先结束固件更新流程".into(),
            ));
        }
        match command {
            WorkbenchCommand::Firmware(action) => self.dispatch_firmware(action),
            WorkbenchCommand::V070Hardware(action) => self.dispatch_v070_hardware(action),
            WorkbenchCommand::SystemWillSleep => {
                self.system_will_sleep();
                Ok(())
            }
            WorkbenchCommand::SystemDidWake => {
                self.system_did_wake();
                Ok(())
            }
            WorkbenchCommand::SystemWillTerminate => {
                self.system_will_terminate();
                Ok(())
            }
            WorkbenchCommand::Header(a) => self.dispatch_header_action(a),
            WorkbenchCommand::Sidebar(a) => self.dispatch_sidebar_action(a),
            WorkbenchCommand::Canvas(a) => self.dispatch_canvas_action(a),
            WorkbenchCommand::SessionStrip(a) => self.dispatch_session_strip_action(a).map(|_| ()),
            WorkbenchCommand::Inspector(a) => self.dispatch_inspector_action(a).map(|_| ()),
            WorkbenchCommand::Editor(a) => self.dispatch_editor_action(a),
            WorkbenchCommand::Radial(a) => self.dispatch_radial_overlay_action(a).map(|_| ()),
            WorkbenchCommand::Overlay(a) => self.dispatch_overlay_action(a),
            WorkbenchCommand::StatusMenu(a) => self.dispatch_status_menu_action(a).map(|_| ()),
            WorkbenchCommand::Device(a) => self.dispatch_device_workflow_action(a),
            WorkbenchCommand::SetRadialSlotConfig { sector, config } => {
                if let Some(sec) = SectorIndex::new(sector) {
                    self.set_radial_slot_config(sec, config);
                }
                Ok(())
            }
            WorkbenchCommand::ForegroundAppChanged { bundle_id } => {
                self.foreground_changed(bundle_id.as_deref());
                Ok(())
            }
            WorkbenchCommand::MainWindowActiveChanged { active } => {
                self.set_main_workbench_window_active(active);
                Ok(())
            }
        }
    }

    pub fn open_isolated(dir: &Path) -> Result<Self, WorkbenchError> {
        Self::open(WorkbenchStoreConfig::isolated(dir))
    }

    pub fn projection(&self) -> &WorkbenchProjection {
        &self.projection
    }

    pub fn sidebar_rows(&self) -> &[SidebarProfileRow] {
        &self.projection.sidebar.rows
    }

    pub fn selected_profile_id(&self) -> Option<&str> {
        self.selected_profile_id.as_deref()
    }

    pub fn selected_layer_id(&self) -> Option<&str> {
        self.selected_layer_id.as_deref()
    }

    pub fn selected_binding_slot(&self) -> Option<u32> {
        self.selected_binding_slot
    }

    pub fn active_host_profile_id(&self) -> Option<String> {
        self.document_store.host_active_profile_id()
    }

    pub fn status_message(&self) -> &str {
        &self.status_message
    }

    pub fn set_status_message(&mut self, message: impl Into<String>) {
        self.status_message = message.into();
        self.commit_runtime_update();
    }

    pub fn banner_flash_count(&self) -> usize {
        self.banner_flash_count
    }

    pub fn last_switch_duration(&self) -> Duration {
        self.last_switch_duration
    }

    pub fn last_exported_preset(&self) -> Option<&str> {
        self.last_exported_preset.as_deref()
    }

    pub fn last_exported_diagnostics_path(&self) -> Option<&str> {
        self.last_exported_diagnostics_path.as_deref()
    }

    pub fn has_all_desktop_agent_presets(&self) -> bool {
        let doc = &self.document_store.draft.document;
        let profiles = &doc.editable.graph.profiles;
        let Some(local_codex_id) = self.local_codex_profile_id() else {
            return false;
        };
        if mi_model::preset::local_codex_profile_needs_starter_slot_migration(doc, &local_codex_id)
        {
            return false;
        }
        if profiles
            .get(&local_codex_id)
            .and_then(|p| p.layer_order.first())
            .is_some_and(|lid| {
                mi_model::preset::coding_agent_layer_needs_encoder_migration(
                    doc,
                    mi_model::preset::LOCAL_CODEX_PROFILE_NAME,
                    lid,
                )
            })
        {
            return false;
        }
        if profiles.keys().any(|pid| {
            mi_model::preset::starter_profile_needs_hardware_mouse_migration(doc, pid)
                || mi_model::preset::starter_profile_needs_legacy_spec_migration(doc, pid)
        }) {
            return false;
        }
        CANONICAL_DESKTOP_AGENT_PROFILES
            .iter()
            .all(|canonical_name| {
                profiles.iter().any(|(pid, p)| {
                    if p.name.trim() != *canonical_name {
                        return false;
                    }

                    if *canonical_name == "Codex" {
                        return p
                            .layer_order
                            .iter()
                            .any(|lid| doc.is_protected_oai_layer(lid));
                    }
                    if mi_model::preset::starter_profile_needs_hardware_mouse_migration(doc, pid)
                        || mi_model::preset::starter_profile_needs_legacy_spec_migration(doc, pid)
                    {
                        return false;
                    }
                    if *canonical_name == "Antigravity" {
                        return p.layer_order.first().is_some_and(|lid| {
                            !mi_model::preset::antigravity_layer_needs_starter_slot_migration(
                                doc, lid,
                            ) && !mi_model::preset::coding_agent_layer_needs_encoder_migration(
                                doc,
                                canonical_name,
                                lid,
                            )
                        });
                    }
                    if *canonical_name == "DeepSeek Coder (DSH)" {
                        return p.layer_order.first().is_some_and(|lid| {
                            !mi_model::preset::coding_agent_layer_needs_encoder_migration(
                                doc,
                                canonical_name,
                                lid,
                            )
                        });
                    }
                    true
                })
            })
    }

    pub fn prepare_desktop_profiles(&mut self) -> Result<(), WorkbenchError> {
        if self.has_all_desktop_agent_presets() {
            return Ok(());
        }
        self.document_store
            .update(|draft| mi_model::preset::install_agent_starter_profiles(draft).map(|_| ()))
            .map_err(WorkbenchError::Preset)?;
        self.commit_runtime_update();
        Ok(())
    }

    pub fn ensure_default_pinned_favorites(&mut self) {
        let profiles = &self
            .document_store
            .host_store()
            .draft
            .document
            .editable
            .graph
            .profiles;
        for (idx, agent) in PinnedFavoritesGrid::DEFAULT_AGENTS.into_iter().enumerate() {
            let valid_existing = self.pinned_favorites.slots[idx]
                .as_ref()
                .is_some_and(|pid| profiles.contains_key(pid));
            if !valid_existing {
                self.pinned_favorites.slots[idx] = self.default_host_profile_id(agent);
            }
        }
    }

    fn legacy_host_profile_id(&self, layer: u8) -> Option<String> {
        self.projection_inputs().legacy_host_profile_id(layer)
    }

    fn activate_legacy_board_layer(
        &mut self,
        layer: u8,
        now: Instant,
        notice: bool,
    ) -> Result<(), WorkbenchError> {
        let id = self.legacy_host_profile_id(layer).ok_or_else(|| {
            WorkbenchError::Routing("旧三层绑定缺失或属于原生应用，按键已暂停".into())
        })?;
        self.select_and_activate_profile_at(&id, now, notice)
    }

    fn confirm_board_routing(&mut self, receipt: &HardwareAutoAcceptanceReceipt) {
        let Some(snapshot) = self.device_session.as_ref().and_then(|s| s.snapshot()) else {
            return;
        };
        match mi_model::apply::recognize_three_layer_profile(&snapshot.config) {
            Ok(None) => self.board_routing = BoardRouting::Legacy,
            Ok(Some(index))
                if receipt.exclusive_vendor
                    && self.document_store.legacy_three_bindings().is_some()
                    && mi_model::is_supported_native_firmware(&receipt.firmware_version)
                    && receipt.profile_index == Some(index)
                    && receipt
                        .layer_index
                        .is_some_and(|layer| (1..=3).contains(&layer)) =>
            {
                if receipt.layer_index == Some(1) {
                    self.arm_board_routing();
                } else {
                    self.wait_for_board_first_layer();
                }
            }
            _ => {
                self.board_routing = BoardRouting::Unverified;
                self.status_message = "板载三层或启动层未确认，按键已暂停".into();
            }
        }
    }

    fn arm_board_routing(&mut self) {
        let Some(session) = self.device_session.as_ref() else {
            return;
        };
        self.board_admit_after = session.input_watermark();
        self.board_decoder.store(true, Ordering::SeqCst);
        self.enter_board_first_layer();
    }

    fn wait_for_board_first_layer(&mut self) {
        let Some(session) = self.device_session.as_ref() else {
            return;
        };
        self.board_admit_after = session.input_watermark();
        self.board_decoder.store(true, Ordering::SeqCst);
        self.board_routing = BoardRouting::WaitingForFirstLayer;
        self.status_message = "等待键盘返回第 1 层，其他层按键已暂停".into();
    }

    fn enter_board_first_layer(&mut self) {
        self.board_routing = BoardRouting::Managed { layer: 1 };
        self.status_message = if self.legacy_host_profile_id(1).is_some() {
            "板载三层已就绪 · 第 1 层"
        } else {
            "旧三层绑定缺失或属于原生应用，按键已暂停"
        }
        .into();
    }

    pub fn provision_board_layers(&mut self) -> Result<(), WorkbenchError> {
        self.start_six_layer_apply(None)
    }

    fn current_bottom_row_layout(&self) -> BottomRowLayout {
        self.projection_inputs().current_bottom_row_layout()
    }

    fn effective_slot_for_surface(&self, surface: ControlSurfaceId) -> u32 {
        if surface == ControlSurfaceId::Joystick {
            return self.selected_joystick_direction.logical_sector_slot();
        }
        if surface == ControlSurfaceId::Key10
            && self.current_bottom_row_layout() == BottomRowLayout::Merged
        {
            let doc = &self.document_store.draft.document;
            let layer = self
                .selected_layer_id
                .as_ref()
                .and_then(|lid| doc.editable.graph.layers.get(lid));
            return resolve_merged_bottom_slot(doc, layer);
        }
        surface.primary_logical_slot()
    }

    fn normalize_surface_for_current_layer(&mut self) {
        if self.selected_surface == ControlSurfaceId::Key11
            && self.current_bottom_row_layout() == BottomRowLayout::Merged
        {
            self.selected_surface = ControlSurfaceId::Key10;
            self.selected_binding_slot =
                Some(self.effective_slot_for_surface(ControlSurfaceId::Key10));
        }
    }

    fn require_selected_profile_and_layer(&self) -> Result<(String, String), WorkbenchError> {
        let pid = self
            .selected_profile_id
            .clone()
            .ok_or_else(|| WorkbenchError::Storage("未选择方案".to_string()))?;
        let lid = self
            .selected_layer_id
            .clone()
            .ok_or_else(|| WorkbenchError::Storage("未选择图层".to_string()))?;
        Ok((pid, lid))
    }

    fn resolve_action_by_id(&self, action_id: &str) -> Result<Action, WorkbenchError> {
        if let Some(existing) = self
            .document_store
            .draft
            .document
            .editable
            .graph
            .actions
            .get(action_id)
        {
            return Ok(existing.clone());
        }
        if let Some(by_inner_id) = self
            .document_store
            .draft
            .document
            .editable
            .graph
            .actions
            .values()
            .find(|a| a.id == action_id)
        {
            return Ok(by_inner_id.clone());
        }
        mi_model::draft::actions()
            .into_iter()
            .find(|a| a.id == action_id)
            .ok_or_else(|| WorkbenchError::Storage(format!("未知动作 ID: {action_id}")))
    }

    fn sync_routing_snapshot(&mut self) {
        self.bottom_row_layout = self.document_store.bottom_row_layout();
        self.merge_wide_key
            .store(self.bottom_row_layout.is_merged(), Ordering::SeqCst);
        self.command_execution_enabled = self.document_store.command_execution_enabled();
        if let Some(running) = &self.six_layer_running {
            running.set_execution_policy(
                self.document_store.host_execution_enabled(),
                self.command_execution_enabled,
                self.update_now,
            );
        }
        self.sync_six_layer_routing_context();
    }

    fn apply_routing_operation(
        &mut self,
        operation: RoutingOperation,
    ) -> Result<(), WorkbenchError> {
        let host = self.document_store.host_store();
        let request = RoutingRequest {
            document_id: host.draft.document_id.clone(),
            document_revision: host.draft.revision,
            host_state_revision: host.host_revision(),
            operation,
        };
        self.document_store
            .update_routing(request)
            .map_err(WorkbenchError::Routing)
    }

    fn initialize_auto_host_ready(&mut self, preserve_saved_host_state: bool) {
        if preserve_saved_host_state {
            return;
        }
        if let Some(ag_pid) = self.find_canonical_profile_id(CanonicalAgent::Antigravity) {
            let _ = self.select_and_activate_profile_at(&ag_pid, self.update_now, false);
            self.six_layer_user_pinned = false;
            self.sync_six_layer_routing_context();
        }
        if !self.document_store.host_execution_enabled() {
            let _ = self.apply_routing_operation(RoutingOperation::Execution { enabled: true });
        }
    }

    fn initialize_selection(
        &mut self,
        preserve_saved_host_state: bool,
    ) -> Result<(), WorkbenchError> {
        let existing_active = self.document_store.host_active_profile_id().filter(|pid| {
            self.document_store
                .draft
                .document
                .editable
                .graph
                .profiles
                .contains_key(pid)
        });
        let initial_pid = if let Some(pid) = existing_active {
            Some(pid)
        } else {
            let antigravity_id = self
                .document_store
                .draft
                .document
                .editable
                .graph
                .profiles
                .iter()
                .find(|(_, p)| {
                    CanonicalAgent::from_profile_name(&p.name) == Some(CanonicalAgent::Antigravity)
                })
                .map(|(id, _)| id.clone())
                .or_else(|| {
                    self.document_store
                        .draft
                        .document
                        .editable
                        .graph
                        .profile_order
                        .first()
                        .cloned()
                });
            if !preserve_saved_host_state {
                if let Some(pid) = &antigravity_id {
                    self.document_store
                        .set_host_active_profile(Some(pid.clone()))
                        .map_err(WorkbenchError::Routing)?;
                }
            }
            antigravity_id
        };

        self.selected_profile_id = initial_pid.clone();
        self.selected_layer_id = initial_pid.as_ref().and_then(|pid| {
            self.document_store
                .draft
                .document
                .editable
                .graph
                .profiles
                .get(pid)
                .and_then(|p| p.layer_order.first().cloned())
        });
        Ok(())
    }

    fn ensure_selected_profile_visible(&mut self) {
        let rows = self.compute_sidebar_rows();
        let prev_profile_id = self.selected_profile_id.clone();
        let prev_layer_id = self.selected_layer_id.clone();
        if self
            .selected_profile_id
            .as_ref()
            .is_some_and(|pid| rows.iter().any(|r| &r.profile_id == pid))
        {
            let layer_valid = self
                .selected_profile_id
                .as_ref()
                .and_then(|pid| {
                    self.document_store
                        .draft
                        .document
                        .editable
                        .graph
                        .profiles
                        .get(pid)
                })
                .is_some_and(|p| {
                    self.selected_layer_id
                        .as_ref()
                        .is_some_and(|lid| p.layer_order.iter().any(|id| id == lid))
                });
            if !layer_valid {
                self.selected_layer_id = self.selected_profile_id.as_ref().and_then(|pid| {
                    self.document_store
                        .draft
                        .document
                        .editable
                        .graph
                        .profiles
                        .get(pid)
                        .and_then(|p| p.layer_order.first().cloned())
                });
            }
        } else {
            self.selected_profile_id = rows.first().map(|r| r.profile_id.clone());
            self.selected_layer_id = self.selected_profile_id.as_ref().and_then(|pid| {
                self.document_store
                    .draft
                    .document
                    .editable
                    .graph
                    .profiles
                    .get(pid)
                    .and_then(|p| p.layer_order.first().cloned())
            });
        }
        if self.selected_profile_id != prev_profile_id || self.selected_layer_id != prev_layer_id {
            self.normalize_surface_for_current_layer();
            self.sync_lighting_preview_after_document_change();
        }
    }

    fn is_protected_oai_profile(&self, profile_id: &str) -> bool {
        self.projection_inputs()
            .is_protected_oai_profile(profile_id)
    }

    fn compute_sidebar_rows(&self) -> Vec<SidebarProfileRow> {
        self.projection_inputs().compute_sidebar_rows()
    }

    fn commit_runtime_update(&mut self) {
        self.capture_runtime_view();
        self.commit_local_update();
        self.reconcile_presentation();
        self.last_committed_stamp = Some(self.current_runtime_stamp());
    }

    fn observed_firmware_version(&self) -> Option<String> {
        self.last_hardware_auto_acceptance
            .as_ref()
            .filter(|_| self.hardware_rpc_verified && self.device_session_connected())
            .map(|receipt| receipt.firmware_version.clone())
            .or_else(|| {
                self.six_layer_running
                    .as_ref()
                    .and_then(|running| running.observed_firmware_version())
            })
            .or_else(|| {
                self.device_session
                    .as_ref()
                    .and_then(|session| session.observed_firmware_version())
            })
    }

    fn commit_local_update(&mut self) {
        #[cfg(test)]
        {
            self.commit_local_update_count = self.commit_local_update_count.wrapping_add(1);
        }
        self.normalize_view_state();
        let current_locale = self.effective_locale();
        let mut reused_keycaps = false;
        let mut reused_categories = false;
        let mut reused_actions = false;
        let mut reusable = ReusableInspectorCatalogs {
            keycaps: None,
            categories: None,
            actions: None,
        };
        if self.projection_generation > 0
            && self.projection.header.resolved_locale == current_locale
        {
            if let InspectorBodyProjection::Editable(prev) = &mut self.projection.inspector.body {
                if self.keycap_search_query.trim().is_empty()
                    && prev.keycap_search_query.trim().is_empty()
                    && prev.filtered_keycaps.len() == CodexKeycapId::ALL_34.len()
                {
                    reusable.keycaps = Some(std::mem::take(&mut prev.filtered_keycaps));
                    reused_keycaps = true;
                }
                if !prev.available_categories.is_empty() {
                    reusable.categories = Some(std::mem::take(&mut prev.available_categories));
                    reused_categories = true;
                }
                if self.action_search_query.trim().is_empty()
                    && prev.action_search_query.trim().is_empty()
                    && mi_model::i18n::is_all_category_filter(&self.action_category_filter)
                    && mi_model::i18n::is_all_category_filter(&prev.action_category_filter)
                    && !self.favorites_only
                    && !prev.favorites_only
                    && self.favorite_action_ids.is_empty()
                    && self.command_execution_enabled == prev.command_execution_enabled
                    && !prev.filtered_actions.is_empty()
                    && !prev.filtered_actions.iter().any(|r| r.is_favorite)
                {
                    reusable.actions = Some(std::mem::take(&mut prev.filtered_actions));
                    reused_actions = true;
                }
            }
        }
        let mut next = self.projection_inputs_with(reusable).project();
        next.firmware = self.firmware_update.projection.clone();
        next.firmware.current_version = self.observed_firmware_version();
        next.firmware.version_status =
            match (&next.firmware.current_version, &next.firmware.release) {
                (Some(current), Some(release)) => {
                    match crate::firmware_catalog::compare_versions(current, &release.version) {
                        Ok(std::cmp::Ordering::Less) => {
                            crate::i18n::tr("有较新的固件可用", current_locale).into()
                        }
                        Ok(std::cmp::Ordering::Equal) => crate::i18n::tr(
                            "当前已是此通道最新版本；再次安装将重刷相同版本",
                            current_locale,
                        )
                        .into(),
                        Ok(std::cmp::Ordering::Greater) => crate::i18n::tr(
                            "所选版本低于当前版本；安装将降级，配置兼容性未确认",
                            current_locale,
                        )
                        .into(),
                        Err(error) => {
                            crate::i18n::localize_firmware_message(&error, current_locale)
                        }
                    }
                }
                (None, _) => crate::i18n::tr("当前运行版本尚未读取", current_locale).into(),
                _ => crate::i18n::tr("尚未检查官方固件", current_locale).into(),
            };
        if current_locale == mi_model::ResolvedLocale::En {
            next.firmware.message =
                crate::i18n::localize_firmware_message(&next.firmware.message, current_locale);
        }
        let changed = if self.projection_generation == 0 {
            true
        } else if (reused_keycaps || reused_categories || reused_actions)
            && matches!(next.inspector.body, InspectorBodyProjection::Editable(_))
        {
            let (held_k, held_c, held_a) = match &mut next.inspector.body {
                InspectorBodyProjection::Editable(nb) => (
                    reused_keycaps.then(|| std::mem::take(&mut nb.filtered_keycaps)),
                    reused_categories.then(|| std::mem::take(&mut nb.available_categories)),
                    reused_actions.then(|| std::mem::take(&mut nb.filtered_actions)),
                ),
                InspectorBodyProjection::ProtectedLocked(_) => (None, None, None),
            };
            let diff = self.projection != next;
            if let InspectorBodyProjection::Editable(nb) = &mut next.inspector.body {
                if let Some(k) = held_k {
                    nb.filtered_keycaps = k;
                }
                if let Some(c) = held_c {
                    nb.available_categories = c;
                }
                if let Some(a) = held_a {
                    nb.filtered_actions = a;
                }
            }
            diff
        } else {
            self.projection != next
        };
        self.projection = next;
        if changed {
            self.projection_generation = self.projection_generation.wrapping_add(1);
        }
        self.last_committed_stamp = Some(self.current_runtime_stamp());
    }

    fn projection_inputs(&self) -> ProjectionInputs<'_> {
        self.projection_inputs_with(ReusableInspectorCatalogs {
            keycaps: None,
            categories: None,
            actions: None,
        })
    }

    fn projection_inputs_with(
        &self,
        reusable_catalogs: ReusableInspectorCatalogs,
    ) -> ProjectionInputs<'_> {
        ProjectionInputs {
            accessibility_trusted: self.accessibility_trusted,
            reusable_catalogs,
            action_category_filter: &self.action_category_filter,
            action_search_query: &self.action_search_query,
            active_modal: &self.active_modal,
            applications: &self.applications,
            available_usb_candidates: &self.available_usb_candidates,
            banner_flash_count: self.banner_flash_count,
            bind_keycap_default_action_on_select: self.bind_keycap_default_action_on_select,
            board_routing: self.board_routing,
            bottom_row_layout: self.bottom_row_layout,
            busy: self.busy,
            can_retry_host_cleanup: self.can_retry_host_cleanup(),
            command_execution_enabled: self.command_execution_enabled,
            competing_input_app_running: self
                .applications
                .get("com.worklouder.input")
                .copied()
                .unwrap_or(false),
            connected_candidate: &self.connected_candidate,
            device_connected: self.device_session_connected(),
            document_store: &self.document_store,
            favorite_action_ids: &self.favorite_action_ids,
            favorites_only: self.favorites_only,
            hardware_rpc_verified: self.hardware_rpc_verified,
            host_caution: &self.host_caution,
            isolated: self.config.isolated,
            keycap_overrides: &self.keycap_overrides,
            keycap_search_query: &self.keycap_search_query,
            last_cursor_position: self.last_cursor_position,
            last_exported_diagnostics_path: &self.last_exported_diagnostics_path,
            last_exported_preset: &self.last_exported_preset,
            last_switch_duration: self.last_switch_duration,
            acknowledged_watermarks: &self.lighting_arbiter.acknowledged_watermarks,
            full_keypad_feedback: &self.lighting_arbiter.full_keypad_feedback,
            dual_zone_feedback: &self.lighting_arbiter.dual_zone_feedback,
            lighting_popover: &self.lighting_popover,
            navigation_history: &self.navigation_history,
            open_drawers: &self.open_drawers,
            overlay_controller: &self.overlay_controller,
            pending_preset_export_scope: self.pending_preset_export_scope,
            pending_preset_import: &self.pending_preset_import,
            pinned_favorites: &self.pinned_favorites,
            preferred_device_serial: self.document_store.preferred_device_serial(),
            preset_transfer_error: &self.preset_transfer_error,
            ptt_active: self.ptt_controller.is_active,
            radial_readiness: &self.radial_readiness,
            radial_slot_configs: &self.radial_slot_configs,
            radial_state: &self.radial_state,
            resolved_locale: self.effective_locale(),
            screen_geometry: &self.screen_geometry,
            selected_binding_slot: self.selected_binding_slot,
            selected_joystick_direction: self.selected_joystick_direction,
            selected_layer_id: &self.selected_layer_id,
            selected_profile_id: &self.selected_profile_id,
            selected_surface: self.selected_surface,
            shortcut_recorder: &self.shortcut_recorder,
            show_keycap_icons: self.show_keycap_icons,
            show_keycap_labels: self.show_keycap_labels,
            show_other_profiles: self.show_other_profiles,
            silent_at_login: self.document_store.silent_at_login(),
            six_layer_busy: self.six_layer_job.is_some(),
            six_layer_running_present: self.six_layer_running.is_some(),
            six_layer_state: &self.six_layer_state,
            six_layer_user_pinned: self.six_layer_user_pinned,
            six_layer_view: &self.six_layer_view,
            slot_role_overrides: &self.slot_role_overrides,
            slot_table: self.active_slot_table(),
            status_menu_open: self.status_menu_open,
            status_message: &self.status_message,
            ui_language: self.document_store.ui_language(),
            v070_auto_layer_sync_enabled: self.document_store.v070_auto_layer_sync_enabled(),
            v070_hardware_mouse_supported: self
                .observed_firmware_version()
                .as_deref()
                .is_none_or(mi_model::supports_v070_mouse_tokens),
            v070_projection: &self.v070_hardware.projection,
            v070_session_sa_rgb_sync_enabled: self
                .document_store
                .v070_session_sa_rgb_sync_enabled(),
        }
    }

    fn capture_runtime_view(&mut self) {
        for agent in CanonicalAgent::ALL {
            let bundle = agent.bundle_id();
            let installed = self.app_cache.is_installed(bundle);
            if let Some(slot) = self.applications.get_mut(bundle) {
                *slot = installed;
            } else {
                self.applications.insert(bundle.to_owned(), installed);
            }
        }
        for slot_cfg in &self.radial_slot_configs {
            if let Some(bundle) = slot_cfg.bundle_id.as_deref() {
                let installed = self.app_cache.is_installed(bundle);
                if let Some(slot) = self.applications.get_mut(bundle) {
                    *slot = installed;
                } else {
                    self.applications.insert(bundle.to_owned(), installed);
                }
            }
        }
        self.captured_app_cache_stamp = (
            Arc::as_ptr(&self.app_cache) as usize,
            self.app_cache.generation(),
        );
        let current_revs = (
            self.document_store.draft.revision,
            self.document_store.host_revision(),
        );
        if self.last_synced_six_layer_revs != current_revs {
            self.last_synced_six_layer_revs = current_revs;
            if self.six_layer_running.is_some() {
                self.sync_six_layer_routing_context();
            }
        }
        self.six_layer_view = self.six_layer_running.as_ref().map(|running| {
            let observed_layer = running.observed_layer.load(Ordering::Acquire);
            SixLayerView {
                configuration_observed: running.readiness.configuration_observed,
                keyboard_grabbed: running.readiness.keyboard_grabbed,
                first_event_verified: running.first_event_verified.load(Ordering::Acquire),
                observed_layer,
                armed: running.armed,
                failure: running.receiver.failure(),
                effective_profile_id: running.effective_profile_id(),
            }
        });
    }

    fn device_session_connected(&self) -> bool {
        if let Some(running) = &self.six_layer_running {
            return running.readiness.configuration_observed
                && running.readiness.keyboard_grabbed
                && running.receiver.failure().is_none();
        }
        self.device_session
            .as_ref()
            .is_some_and(|session| !session.is_disconnected())
    }

    fn reconcile_presentation(&mut self) {
        let overlay = &self.projection.overlay;
        if self.last_hud_panel.as_ref() != Some(&overlay.panel_spec) {
            self.reconcile_hud_panel(overlay);
            self.last_hud_panel = Some(overlay.panel_spec.clone());
        }
        let radial = &self.projection.radial;
        if self.last_radial_panel.as_ref() != Some(&radial.panel_spec) {
            self.reconcile_radial_panel(radial);
            self.last_radial_panel = Some(radial.panel_spec.clone());
        }
        let menu = &self.projection.status_menu;
        let next = StatusItemSnapshot {
            icon: menu.icon.clone(),
            active_profile_name: menu.active_profile_name.clone(),
            routing_mode_label: menu.routing_mode_summary.clone(),
            host_execution_enabled: menu.host_execution_enabled,
            radial_menu_enabled: menu.radial_menu_enabled,
            cheat_sheet_visible: menu.cheat_sheet_visible,
            pinned_favorite_ids: self.pinned_favorites.slots.clone(),
        };
        if self.last_status_item_snapshot.as_ref() != Some(&next) {
            self.status_item_host.sync_status_item(next.clone());
            self.last_status_item_snapshot = Some(next);
        }
    }

    fn normalize_view_state(&mut self) {
        self.refresh_navigation_revision();
        self.sync_modal_for_layer_protection();
        self.ensure_default_pinned_favorites();
        if self.overlay_controller.is_cheat_sheet_visible() {
            let cheat_sheet = self.build_cheat_sheet_projection();
            self.overlay_controller.refresh_cheat_sheet_if_visible(
                cheat_sheet,
                &self.screen_geometry,
                self.last_cursor_position,
            );
        }
    }

    fn reconcile_hud_panel(&self, overlay: &HostOverlayProjection) {
        if let Some(spec) = &overlay.panel_spec {
            match spec.role {
                PassivePanelRole::PassiveNotice => {
                    self.panel_host.hide_panel(PassivePanelRole::CheatSheet);
                    self.panel_host.present_panel(spec.clone());
                }
                PassivePanelRole::CheatSheet => {
                    self.panel_host.hide_panel(PassivePanelRole::PassiveNotice);
                    self.panel_host.present_panel(spec.clone());
                }
                PassivePanelRole::RadialMenu => {}
            }
        } else {
            self.panel_host.hide_panel(PassivePanelRole::PassiveNotice);
            self.panel_host.hide_panel(PassivePanelRole::CheatSheet);
        }
    }

    fn reconcile_radial_panel(&self, radial: &RadialOverlayProjection) {
        if let Some(spec) = &radial.panel_spec {
            self.panel_host.present_panel(spec.clone());
        } else {
            self.panel_host.hide_panel(PassivePanelRole::RadialMenu);
        }
    }

    fn resolved_active_profile_id(&self) -> Option<String> {
        self.projection_inputs().resolved_active_profile_id()
    }

    fn active_desktop_agent_adapter(&self) -> Option<String> {
        self.projection_inputs().active_desktop_agent_adapter()
    }

    fn active_profile_display_name(&self) -> String {
        self.projection_inputs().active_profile_display_name()
    }

    fn routing_mode_summary_label(&self) -> String {
        self.projection_inputs().routing_mode_summary_label()
    }

    fn current_radial_route_stamp(&self) -> RadialRouteStamp {
        RadialRouteStamp {
            document_revision: self.document_store.draft.revision,
            host_revision: self.document_store.host_revision(),
            profile_id: self.resolved_active_profile_id(),
            profile_name: self.active_profile_display_name(),
        }
    }

    fn build_cheat_sheet_projection(&self) -> CheatSheetProjection {
        self.projection_inputs().build_cheat_sheet_projection()
    }

    pub fn active_modal(&self) -> Option<&EditorModalProjection> {
        self.projection.active_modal.as_ref()
    }

    pub fn selected_keycap(&self, slot: u32) -> CodexKeycapId {
        self.projection_inputs().selected_keycap(slot)
    }

    pub fn system_will_terminate(&mut self) {
        self.end_lighting_preview_blocking(LightingPreviewStopReason::SystemTerminate);
        self.teardown_v070_session_sa_rgb();
        self.hardware_event_tx.close();
        if let Some(session) = &self.device_session {
            session.disconnect();
        }
        if let Some(running) = self.six_layer_running.take() {
            if tokio::runtime::Handle::try_current().is_ok() {
                let rt = Arc::clone(&self.tokio_runtime);
                let _ = std::thread::spawn(move || running.shutdown(&rt)).join();
            } else {
                let _ = running.shutdown(&self.tokio_runtime);
            }
        }
    }
}

impl Drop for WorkbenchStore {
    fn drop(&mut self) {
        self.detach_lighting_preview_on_drop();
        self.teardown_v070_session_sa_rgb();
        self.hardware_event_tx.close();
        if let Some(session) = &self.device_session {
            session.disconnect();
        }
        if let Some(running) = &self.six_layer_running {
            running.retire();
        }
    }
}
