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
        MultiBranchTarget, MultiEditorModalProjection, ReconnectComparisonModalProjection,
    },
    header::{HeaderAction, HeaderProjection, HeaderReadyState},
    inspector::{
        ActionLibraryRowProjection, CodexKeycapId, EditableSlotInspectorProjection,
        InspectorAction, InspectorBodyProjection, InspectorDrawer, InspectorProjection,
        KeycapCatalogItemProjection, KeycapRole, MacroLibraryRowProjection, ShortcutRecorderState,
        INSPECTOR_WIDTH,
    },
    overlay::{
        CheatSheetKeyCellProjection, CheatSheetProjection, HostOverlayAction,
        HostOverlayProjection, HudVisibilityState, OverlayController,
        TargetAgentStatusCardProjection,
    },
    radial::{
        resolve_radial_slots, RadialConfirmMode, RadialConsumeOutcome, RadialInputEvent,
        RadialInputReadiness, RadialIntent, RadialMenuSession, RadialMenuState,
        RadialOverlayAction, RadialOverlayProjection, RadialPositionSample, RadialPresetKind,
        RadialRouteStamp, RadialSlot, RadialSlotConfig, RadialSlotResolutionContext, SectorIndex,
        ESCAPE_KEY_CODE, RADIAL_SECTOR_COUNT,
    },
    session_strip::{
        agent_for_place, FourAppLiveCandidate, FourAppSessionKind, FourAppSlotTable,
        SessionCapsulePillProjection, SessionSlotCardProjection, SessionStripAction,
        SessionStripProjection, SimpleLightingPopoverProjection,
    },
    sidebar::{
        AutoSwitchState, BackupCommand, BoardLayerProjection, BoardLayersProjection,
        BoardProfileChoice, CanonicalAgent, OwnershipBannerState, ProfileCommand, SidebarAction,
        SidebarProfileRow, SidebarProjection,
    },
    status_menu::{
        FavoriteSlotCardProjection, FavoriteSlotIndex, PendingPresetImportPreview,
        PinnedFavoritesGrid, PresetExportScope, StatusMenuAction, StatusMenuProjection,
        FAVORITE_SLOT_COUNT,
    },
};
use mi_bridge::{FourAppAgentPhase, FourAppLightingArbiter, PushToTalkController};
use mi_macos::{
    executor::APPLE_FN_KEY_CODE, AppBundleCache, AppBundleInfo, HeldInputId, HostActionReceipt,
    LiveHostActionExecutor, MacOsSystemDriver, PassivePanelFrameSpec, PassivePanelHost,
    PassivePanelRole, PresetFileEnvelope, RecordingPassivePanelHost, RecordingStatusItemHost,
    ScreenGeometry, ScreenPoint, StatusItemHost, StatusItemIconSpec, StatusItemSnapshot,
};
use mi_model::{
    action_library,
    board_layers::{
        BoardLayer, BoardLayerPlan, BoardLayerRoute, BoardPreviewSelection, NativeLayerOwner,
    },
    document::{LayerNode, LocalDocumentV2},
    draft::{Action, ActionKind, Draft},
    host_router::{
        resolve_host_action_with_layout, BottomRowLayout, PopoverPreviewMode, ResolvedHostAction,
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

#[path = "session_navigation.rs"]
pub(crate) mod session_navigation;
#[path = "six_layer_store.rs"]
mod six_layer_store;
#[path = "workspace_lifecycle.rs"]
mod workspace_lifecycle;
use session_navigation::{
    AntigravityInstance, CatalogInstance, NavigationCatalog, NavigationCommand,
    NavigationInputLease, SessionNavigation,
};
use workspace_lifecycle::SystemLifecycle;
#[path = "projection.rs"]
mod projection;
pub use projection::format_popover_cheat_label;
use projection::{ProjectionInputs, ReusableInspectorCatalogs, SixLayerView};
#[path = "runtime_inbox.rs"]
mod runtime_inbox;
use runtime_inbox::RuntimeInbox;

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
pub(crate) enum LiveSessionSource {
    Antigravity,
    Codex,
}

impl LiveSessionSource {
    fn agent(self) -> CanonicalAgent {
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

#[derive(Debug, Clone)]
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
        catalog: Option<NavigationCatalog>,
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

impl BoardRouting {
    fn is_managed(self) -> bool {
        matches!(self, Self::Managed { .. } | Self::WaitingForFirstLayer)
    }
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
    pub app_cache: Arc<AppBundleCache>,
    pub panel_host: Arc<dyn PassivePanelHost>,
    pub status_item_host: Arc<dyn StatusItemHost>,
}

impl WorkbenchStoreConfig {
    pub fn production() -> Self {
        let document_dir = env::var_os("MI_DOCUMENT_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                env::var_os("HOME")
                    .map(|home| PathBuf::from(home).join("Library/Application Support/MicroInput"))
            })
            .unwrap_or_else(|| PathBuf::from(".micro-input"));
        Self {
            document_dir,
            auto_connect_usb: true,
            isolated: false,
            app_cache: Arc::new(AppBundleCache::new()),
            panel_host: Arc::new(RecordingPassivePanelHost::new()),
            status_item_host: Arc::new(RecordingStatusItemHost::new()),
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
            app_cache,
            panel_host: Arc::new(RecordingPassivePanelHost::new()),
            status_item_host: Arc::new(RecordingStatusItemHost::new()),
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
    SystemWillSleep,
    SystemDidWake,
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
            device_connected: false,
            device_summary: "Creator Micro 2 (303A:8298) · 离线草稿模式".to_string(),
            document_revision: 0,
            host_revision: 0,
            banner_flash_count: 0,
            last_switch_duration: Duration::ZERO,
        }
    }
}

#[derive(Clone)]
struct HeldDictation {
    input: HeldInputId,
    profile_id: String,
    adapter: String,
}

pub struct WorkbenchStore {
    pub config: WorkbenchStoreConfig,
    pub document_store: DocumentStore,
    pub ptt_controller: PushToTalkController,
    pub lighting_arbiter: FourAppLightingArbiter,
    pub slot_table: FourAppSlotTable,
    pub app_cache: Arc<AppBundleCache>,
    pub panel_host: Arc<dyn PassivePanelHost>,
    pub status_item_host: Arc<dyn StatusItemHost>,
    pub device_session: Option<Arc<DeviceSession>>,
    pub connected_candidate: Option<mi_transport::HidCandidate>,
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
    last_synced_six_layer_revs: (u64, u64),
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
    pub pinned_favorites: PinnedFavoritesGrid,
    pub screen_geometry: ScreenGeometry,
    pub last_cursor_position: ScreenPoint,
    pub pending_preset_import: Option<PendingPresetImportPreview>,
    pub pending_preset_export_scope: Option<PresetExportScope>,
    pub preset_transfer_error: Option<String>,
    pub last_host_action_receipt: Option<HostActionReceipt>,
    pub dispatched_receipts: Vec<HostActionReceipt>,
    pub live_executor: Arc<LiveHostActionExecutor<MacOsSystemDriver>>,
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
    auto_follow_enabled: bool,
    manual_override: bool,
    app_profile_links: BTreeMap<String, String>,
    busy: bool,
    status_message: String,
    last_exported_preset: Option<String>,
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
    status_menu_open: bool,
    active_modal: Option<EditorModalProjection>,
    last_status_item_snapshot: Option<StatusItemSnapshot>,
    last_hud_panel: Option<Option<PassivePanelFrameSpec>>,
    last_radial_panel: Option<Option<PassivePanelFrameSpec>>,
    applications: BTreeMap<String, bool>,
    six_layer_view: Option<SixLayerView>,
    update_now: Instant,
    input_lost_session: Option<u64>,
    projection_generation: u64,
    projection: WorkbenchProjection,
}

impl WorkbenchStore {
    pub fn open(config: WorkbenchStoreConfig) -> Result<Self, WorkbenchError> {
        let now = Instant::now();
        fs::create_dir_all(&config.document_dir)
            .map_err(|e| WorkbenchError::Storage(e.to_string()))?;
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

        let mut store = Self {
            config,
            document_store,
            ptt_controller: PushToTalkController::new_simulated(),
            lighting_arbiter: FourAppLightingArbiter::new(),
            slot_table: FourAppSlotTable::new(),
            app_cache,
            panel_host,
            status_item_host,
            device_session: None,
            connected_candidate: None,
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
            pinned_favorites: PinnedFavoritesGrid::default(),
            screen_geometry: ScreenGeometry::default(),
            last_cursor_position: ScreenPoint::default(),
            pending_preset_import: None,
            pending_preset_export_scope: None,
            preset_transfer_error: None,
            last_host_action_receipt: None,
            dispatched_receipts: Vec::new(),
            live_executor: Arc::new(LiveHostActionExecutor::new(MacOsSystemDriver::default())),
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
            last_synced_six_layer_revs: (0, 0),
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
            auto_follow_enabled: false,
            manual_override: false,
            app_profile_links: BTreeMap::new(),
            busy: false,
            status_message: "已就绪".to_string(),
            last_exported_preset: None,
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
            command_execution_enabled: true,
            lighting_apply_to_all_layers: false,
            lighting_preview_active: false,
            lighting_preview_remaining_secs: 0,
            status_menu_open: false,
            active_modal: None,
            last_status_item_snapshot: None,
            last_hud_panel: None,
            last_radial_panel: None,
            applications: BTreeMap::new(),
            six_layer_view: None,
            update_now: now,
            input_lost_session: None,
            projection_generation: 0,
            projection: WorkbenchProjection::default(),
        };

        store.sync_routing_snapshot();
        store.prepare_desktop_profiles()?;
        store.ensure_default_agent_links()?;
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
        store.last_synced_six_layer_revs = (
            store.document_store.draft.revision,
            store.document_store.host_revision(),
        );
        store.refresh_accessibility_trusted();
        store.refresh_competing_input_app_running(true);
        store.refresh_four_app_lighting();
        store.commit_runtime_update();
        Ok(store)
    }

    pub fn set_accessibility_trusted_for_test(&mut self, trusted: Option<bool>) {
        self.accessibility_trusted_override = trusted;
        self.refresh_accessibility_trusted();
        self.commit_runtime_update();
    }

    pub fn set_competing_input_app_running_for_test(&mut self, running: bool) {
        self.applications
            .insert("com.worklouder.input".to_string(), running);
        self.commit_runtime_update();
    }

    fn refresh_accessibility_trusted(&mut self) {
        self.accessibility_trusted = if let Some(override_val) = self.accessibility_trusted_override
        {
            override_val
        } else if !self.config.isolated && self.config.auto_connect_usb {
            self.live_executor.is_accessibility_trusted()
        } else {
            true
        };
    }

    fn refresh_competing_input_app_running(&mut self, force: bool) {
        if self.config.isolated || !self.config.auto_connect_usb {
            return;
        }
        if !force
            && self
                .update_now
                .saturating_duration_since(self.last_competing_app_check_at)
                < Duration::from_secs(5)
        {
            return;
        }
        self.last_competing_app_check_at = self.update_now;
        let running = mi_macos::is_bundle_running("com.worklouder.input");
        self.applications
            .insert("com.worklouder.input".to_string(), running);
    }

    pub fn open_accessibility_settings(&mut self) {
        self.refresh_accessibility_trusted();
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
            self.manual_override = true;
            if prefer_selected || self.selected_profile_id.is_none() {
                self.selected_profile_id = Some(target_pid);
                self.selected_layer_id = first_layer_id;
                self.normalize_surface_for_current_layer();
            }
            let name = self.active_profile_display_name();
            self.status_message = format!("已锁定方案「{name}」");
        } else {
            self.apply_routing_operation(RoutingOperation::LockProfile { profile_id: None })?;
            self.manual_override = !self.auto_follow_enabled;
            self.status_message = "已解除方案锁定".to_string();
        }
        self.sync_six_layer_routing_context();
        self.sync_live_session_source();
        self.refresh_four_app_lighting();
        self.commit_runtime_update();
        Ok(())
    }

    pub fn poll_hardware_events(&mut self) -> bool {
        self.poll_at(Instant::now())
    }

    pub fn poll_at(&mut self, now: Instant) -> bool {
        self.update_now = now;
        let before_gen = self.projection_generation;
        if now.saturating_duration_since(self.last_accessibility_check_at) >= Duration::from_secs(1)
        {
            self.last_accessibility_check_at = now;
            self.refresh_accessibility_trusted();
        }
        self.refresh_competing_input_app_running(false);
        self.poll_workspace_lifecycle();
        if self.system_lifecycle == SystemLifecycle::Awake {
            self.poll_six_layer_runtime();
        }
        self.sync_live_session_source();
        self.drain_pending_hardware_events(now);
        self.poll_session_navigation();
        self.overlay_controller.expire_notice_if_due(now);
        self.commit_runtime_update();
        self.projection_generation != before_gen
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
        match command {
            WorkbenchCommand::SystemWillSleep => {
                self.system_will_sleep();
                Ok(())
            }
            WorkbenchCommand::SystemDidWake => {
                self.system_did_wake();
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

    pub fn current_or_synthetic_device_snapshot(&mut self) -> Value {
        let wb_path = self.config.workbench_json_path();
        if !wb_path.exists() {
            let _ = self.document_store.open(wb_path);
        }
        if let Some(snap) = &self.document_store.draft.document.baseline {
            return snap.clone();
        }
        let mut raw_demo = mi_model::demo()
            .map(|d| d.raw)
            .unwrap_or_else(|_| self.document_store.draft.config.raw.clone());
        raw_demo["version"] = json!(1);
        let keymap_bytes = serde_json::to_vec(&raw_demo).unwrap_or_default();
        let keymap_file = mi_model::operation_record::file_value(&keymap_bytes);
        let sha1 = keymap_file["sha1"].as_str().unwrap_or_default().to_string();
        let snap = json!({
            "firmware": "0.6.3",
            "device_identity": {
                "confidence": "verified-serial",
                "serial": "creator-micro-2-local",
                "session_id": 1
            },
            "keymap": keymap_file,
            "smart_actions": {
                "state": "missing"
            },
            "files": [
                {
                    "name": "keymap",
                    "state": "present",
                    "size": keymap_bytes.len(),
                    "sha1": sha1
                }
            ]
        });
        if let Ok(base_cfg) = mi_model::parse_device_files(&keymap_bytes, None) {
            let _ = self
                .document_store
                .create_independent(base_cfg, snap.clone());
            let pid = self.document_store.draft.config.profiles[0].id.clone();
            let lid = self.document_store.draft.config.profiles[0].layers[0]
                .id
                .clone();
            if let Some(device_key_action) = mi_model::draft::actions()
                .into_iter()
                .find(|a| a.id == "device-key")
            {
                let _ = self
                    .document_store
                    .update(|draft| draft.assign(&pid, &lid, 0, device_key_action));
            }
        }
        snap
    }

    pub fn dispatch_device_workflow_action(
        &mut self,
        action: DeviceWorkflowAction,
    ) -> Result<(), WorkbenchError> {
        self.require_awake()?;
        match action {
            DeviceWorkflowAction::ReadIdentity => {
                let msg = if let Some(cand) = &self.connected_candidate {
                    format!(
                        "设备身份已读取：{} ({:04X}:{:04X}) · 只读门禁生效",
                        cand.product, cand.vid, cand.pid
                    )
                } else {
                    "当前处于离线草稿模式 · 设备身份：Creator Micro 2 (303A:8298)".to_string()
                };
                self.status_message = msg;
                self.commit_runtime_update();
                Ok(())
            }
            DeviceWorkflowAction::ReadSnapshot => {
                let snap = self.current_or_synthetic_device_snapshot();
                let sha1 = snap["keymap"]["sha1"].as_str().unwrap_or("-");
                self.status_message = format!("设备快照已核验 · keymap sha1={sha1}");
                self.commit_runtime_update();
                Ok(())
            }
            DeviceWorkflowAction::ExportDiagnostics => {
                let diag_path = self.config.document_dir.join("diagnostic-report.json");
                let report = json!({
                    "schema_version": 1,
                    "product": "Creator Micro 2",
                    "vid_pid": "303A:8298",
                    "connected": self.device_session_connected(),
                    "document_revision": self.document_store.draft.revision,
                    "host_revision": self.document_store.host_revision(),
                    "execution_enabled": self.document_store.host_execution_enabled(),
                });
                let bytes = serde_json::to_vec_pretty(&report).map_err(|error| {
                    WorkbenchError::Storage(format!("无法生成诊断报告：{error}"))
                })?;
                fs::write(&diag_path, bytes).map_err(|error| {
                    WorkbenchError::Storage(format!("无法保存诊断报告：{error}"))
                })?;
                self.status_message = format!("已导出诊断报告：{}", diag_path.display());
                self.commit_runtime_update();
                Ok(())
            }
            DeviceWorkflowAction::OpenReconnectComparison => {
                let snap = self.current_or_synthetic_device_snapshot();
                let device_cfg = self.document_store.draft.config.clone();
                self.dispatch_editor_action(EditorAction::OpenReconnectComparison {
                    device_config: Box::new(device_cfg),
                    baseline: snap,
                })
            }
            DeviceWorkflowAction::ConfirmComparisonIdentity => {
                let snap = self.current_or_synthetic_device_snapshot();
                self.dispatch_editor_action(EditorAction::ComparisonConfirmIdentity {
                    current_snapshot: snap,
                })
            }
            DeviceWorkflowAction::ResolveComparison(choice) => {
                let snap = self.current_or_synthetic_device_snapshot();
                self.dispatch_editor_action(EditorAction::ComparisonResolve {
                    choice,
                    current_snapshot: snap,
                })
            }
            DeviceWorkflowAction::OpenApplyPreparation => {
                let snap = self.current_or_synthetic_device_snapshot();
                self.dispatch_editor_action(EditorAction::OpenApplyPreparation {
                    current_snapshot: snap,
                })
            }
            DeviceWorkflowAction::PrepareApplyBackup => {
                let snap = self.current_or_synthetic_device_snapshot();
                self.dispatch_editor_action(EditorAction::ApplyPrepareBackup {
                    current_snapshot: snap,
                })
            }
            DeviceWorkflowAction::TakePreparedCandidate => {
                let snap = self.current_or_synthetic_device_snapshot();
                self.dispatch_editor_action(EditorAction::ApplyTakePreparedCandidate {
                    current_snapshot: snap,
                })
            }
            DeviceWorkflowAction::CancelApplyPreview => {
                self.dispatch_editor_action(EditorAction::ApplyCancelPreview)
            }
            DeviceWorkflowAction::SimulateKeyPress { binding_id } => {
                let ev = InputEvent {
                    session_id: 1,
                    board_layer: None,
                    sequence: 1,
                    ingress_sequence: 1,
                    radial_center: None,
                    radial_position: None,
                    timestamp_ms: 0,
                    kind: InputEventKind::Pressed,
                    logical_switch: None,
                    binding_id: Some(binding_id),
                    physical_control: None,
                    keycap_group: None,
                    action_role: None,
                    lost_count: 0,
                    action_eligible: true,
                };
                let _ = self.consume_physical_input_event(ev, self.update_now);
                Ok(())
            }
        }
    }

    pub fn start_mac_usb_discovery_watcher(&mut self) {
        self.discovery_enabled = true;
        if self.system_lifecycle != SystemLifecycle::Awake {
            return;
        }
        if self.config.isolated || self.mac_watcher_transport.is_some() {
            return;
        }
        let transport = Arc::new(if self.discovery_is_descriptor_only() {
            mi_transport::MacTransport::new()
        } else {
            mi_transport::MacTransport::exclusive_vendor()
        });
        let mut rx = mi_transport::HidTransport::subscribe(transport.as_ref());
        let tx = self.hardware_event_tx.clone();
        let epoch = self.lifecycle_epoch.load(Ordering::Acquire);
        self.tokio_runtime.spawn(async move {
            while rx.recv().await.is_ok() {
                if tx
                    .send(HardwareLoopEvent::DiscoveryChanged { epoch })
                    .is_err()
                {
                    break;
                }
            }
        });
        self.mac_watcher_transport = Some(transport);
    }

    fn effective_resolved_active_profile_id(&self) -> Option<String> {
        let profiles = &self
            .document_store
            .host_store()
            .draft
            .document
            .editable
            .graph
            .profiles;
        if let Some(locked) = self
            .document_store
            .locked_profile_id()
            .filter(|pid| profiles.contains_key(pid))
        {
            return Some(locked);
        }
        if let Some(profile) = self
            .six_layer_running
            .as_ref()
            .and_then(|running| running.effective_profile_id())
            .or_else(|| {
                self.six_layer_view
                    .as_ref()
                    .and_then(|running| running.effective_profile_id.clone())
            })
        {
            return Some(profile);
        }
        self.document_store
            .host_active_profile_id()
            .filter(|pid| profiles.contains_key(pid))
    }

    pub(crate) fn desired_live_session_source(&self) -> LiveSessionSource {
        let profiles = &self
            .document_store
            .host_store()
            .draft
            .document
            .editable
            .graph
            .profiles;
        let profile_name = |pid: &str| profiles.get(pid).map(|p| p.name.as_str());
        let active_id = self
            .effective_resolved_active_profile_id()
            .or_else(|| self.selected_profile_id.clone());
        let active_name = active_id
            .as_deref()
            .and_then(|id| self.runtime_action_profile_id(id))
            .and_then(profile_name);
        if active_name == Some(mi_model::preset::LOCAL_CODEX_PROFILE_NAME) {
            LiveSessionSource::Codex
        } else {
            LiveSessionSource::Antigravity
        }
    }

    pub(crate) fn sync_live_session_source(&mut self) -> bool {
        let desired = self.desired_live_session_source();
        let changed = {
            let (lock, cvar) = &*self.live_session_source;
            let mut guard = lock.lock().unwrap();
            if guard.source == desired {
                false
            } else {
                guard.source = desired;
                guard.generation = guard.generation.wrapping_add(1);
                cvar.notify_all();
                true
            }
        };
        if !changed {
            return false;
        }
        let acks = self.lighting_arbiter.acknowledged_watermarks.clone();
        self.update_navigation_catalog(None);
        self.slot_table.retain_agent_slots(desired.agent(), &acks);
        self.refresh_navigation_revision();
        self.refresh_four_app_lighting();
        self.commit_runtime_update();
        true
    }

    pub fn start_antigravity_live_poller(&self) {
        if self.config.isolated {
            return;
        }
        let tx = self.hardware_event_tx.clone();
        let epoch_source = Arc::clone(&self.lifecycle_epoch);
        let live_source = Arc::clone(&self.live_session_source);
        std::thread::spawn(move || {
            let mut seen_running_ids = BTreeSet::new();
            let mut completed_transition_watermarks = BTreeMap::new();
            loop {
                let (source, source_gen) = {
                    let (lock, _) = &*live_source;
                    let guard = lock.lock().unwrap();
                    (guard.source, guard.generation)
                };
                let epoch = epoch_source.load(Ordering::Acquire);
                let catalog = match source {
                    LiveSessionSource::Antigravity => poll_antigravity_daemon_candidates(
                        &mut seen_running_ids,
                        &mut completed_transition_watermarks,
                    ),
                    LiveSessionSource::Codex => poll_codex_session_candidates(),
                };
                let still_current = {
                    let (lock, _) = &*live_source;
                    let guard = lock.lock().unwrap();
                    guard.source == source && guard.generation == source_gen
                };
                if !still_current {
                    continue;
                }
                if tx
                    .send(HardwareLoopEvent::LiveCandidatesUpdated { epoch, catalog })
                    .is_err()
                {
                    break;
                }
                let (lock, cvar) = &*live_source;
                let guard = lock.lock().unwrap();
                if guard.generation == source_gen {
                    let _ = cvar.wait_timeout(guard, Duration::from_millis(1500));
                }
            }
        });
    }

    pub fn connect_default_usb(&mut self) -> Result<bool, WorkbenchError> {
        self.require_awake()?;
        self.exclusive_usb_transport();
        self.connect_default_usb_with_execution(true)
    }

    fn exclusive_usb_transport(&mut self) -> Arc<mi_transport::MacTransport> {
        self.resume_requires_explicit_connect = false;
        if self
            .mac_watcher_transport
            .as_ref()
            .is_some_and(|transport| !transport.is_exclusive_vendor())
        {
            if let Some(transport) = self.mac_watcher_transport.take() {
                mi_transport::HidTransport::stop_discovery(transport.as_ref());
            }
        }
        self.start_mac_usb_discovery_watcher();
        self.mac_watcher_transport
            .get_or_insert_with(|| Arc::new(mi_transport::MacTransport::exclusive_vendor()))
            .clone()
    }

    fn connect_default_usb_with_execution(
        &mut self,
        enable_execution_on_connect: bool,
    ) -> Result<bool, WorkbenchError> {
        self.require_awake()?;
        if let Some(installation) = &self.six_layer_state.installation {
            let plan = installation.plan.clone();
            if self
                .six_layer_running
                .as_ref()
                .is_none_or(|running| running.receiver.failure().is_some())
                && self.six_layer_job.is_none()
            {
                self.start_six_layer_apply(Some(plan))?;
            }
            return Ok(self.six_layer_running.is_some());
        }
        let transport = self.exclusive_usb_transport();
        let result = self.connect_with_transport(transport.as_ref(), enable_execution_on_connect);
        if let Err(error) = &result {
            self.set_status_message(error.to_string());
        }
        result
    }

    pub fn connect_with_transport(
        &mut self,
        transport: &dyn mi_transport::HidTransport,
        enable_execution_on_connect: bool,
    ) -> Result<bool, WorkbenchError> {
        self.require_awake()?;
        if self.six_layer_state.installation.is_some()
            || self.six_layer_running.is_some()
            || self.six_layer_job.is_some()
        {
            return Err(WorkbenchError::Routing(
                "六层运行独占 USB，普通连接不能启动第二个读取器".into(),
            ));
        }
        if self
            .device_session
            .as_ref()
            .is_some_and(|s| s.is_disconnected() || self.input_lost_session == Some(s.id))
        {
            self.release_board_inputs(self.update_now);
            if let Some(session) = self.device_session.take() {
                self.tokio_runtime.block_on(session.shutdown());
            }
            self.connected_candidate = None;
        }
        if self.live_executor.has_unconfirmed_apple_fn_release()
            || !self.ptt_controller.cleanup_confirmed
        {
            let stopped = self.live_executor.stop_owned_actions();
            self.finish_host_cleanup(stopped)
                .map_err(WorkbenchError::Routing)?;
        }
        if let (Some(session), Some(cand)) = (&self.device_session, &self.connected_candidate) {
            self.status_message = format!(
                "已连接 {} ({:04X}:{:04X}) · {}",
                cand.product,
                cand.vid,
                cand.pid,
                if session.has_exclusive_vendor_link() {
                    "USB 专用接口已独占"
                } else {
                    "已建立 HID 会话"
                }
            );
            self.commit_runtime_update();
            return Ok(true);
        }

        let candidates = transport
            .enumerate()
            .map_err(|e| WorkbenchError::Storage(format!("检测小键盘连接失败：{e}")))?;
        let verified_usb: Vec<mi_transport::HidCandidate> = candidates
            .iter()
            .filter(|c| {
                mi_transport::is_candidate(c)
                    && c.transport_hint == mi_transport::TransportHint::Usb
                    && c.pid == 0x8298
            })
            .cloned()
            .collect();

        match verified_usb.len() {
            1 => {
                let target = verified_usb.into_iter().next().unwrap();
                let link = transport
                    .open(&target.path)
                    .map_err(|e| WorkbenchError::Storage(format!("打开 USB 小键盘失败：{e}")))?;
                if !self.config.isolated && !link.is_exclusive_vendor() {
                    link.close();
                    return Err(WorkbenchError::Storage("未取得 USB 专用接口独占".into()));
                }
                let session = {
                    let _guard = self.tokio_runtime.enter();
                    DeviceSession::new(
                        link,
                        Box::new(mi_proto::NativeFramer::default()),
                        Arc::new(mi_session::ReadOnlySchema),
                        mi_session::SessionConfig::default(),
                    )
                };
                self.merge_wide_key
                    .store(self.bottom_row_layout.is_merged(), Ordering::SeqCst);
                self.board_decoder = Arc::new(AtomicBool::new(false));
                self.board_routing = if session.has_exclusive_vendor_link() {
                    BoardRouting::Checking
                } else {
                    BoardRouting::Legacy
                };
                let mut monitor = session.subscribe_verified_input_with_board_mode(
                    Arc::clone(&self.merge_wide_key),
                    Arc::clone(&self.board_decoder),
                );
                let session_id = session.id;
                self.hardware_event_tx.open_source(session_id);
                self.input_lost_session = None;
                let tx = self.hardware_event_tx.clone();
                self.tokio_runtime.spawn(async move {
                    while let Some(event) = monitor.next().await {
                        if tx.send(HardwareLoopEvent::Input(event)).is_err() {
                            return;
                        }
                    }
                    let _ = tx.send(HardwareLoopEvent::Disconnected { session_id });
                });
                self.device_session = Some(Arc::clone(&session));
                self.connected_candidate = Some(target.clone());
                self.hardware_rpc_verified = false;
                if enable_execution_on_connect && !self.document_store.host_execution_enabled() {
                    let _ =
                        self.apply_routing_operation(RoutingOperation::Execution { enabled: true });
                }
                self.lighting_arbiter.invalidate_hardware_cache();
                self.refresh_four_app_lighting();
                let accept_session = Arc::clone(&session);
                let accept_target = target.clone();
                let accept_tx = self.hardware_event_tx.clone();
                let initial_lighting_json = self
                    .lighting_arbiter
                    .last_hardware_sync_json
                    .clone()
                    .unwrap_or_default();
                self.tokio_runtime.spawn(async move {
                    if let Some(receipt) = run_automatic_hardware_acceptance(
                        &accept_session,
                        &accept_target,
                        &initial_lighting_json,
                    )
                    .await
                    {
                        let _ = accept_tx.send(HardwareLoopEvent::HardwareAutoAccepted {
                            session_id: accept_session.id,
                            receipt,
                        });
                    }
                });
                self.status_message = format!(
                    "已连接 {} ({:04X}:{:04X}) · {}",
                    target.product,
                    target.vid,
                    target.pid,
                    if session.has_exclusive_vendor_link() {
                        "USB 专用接口已独占"
                    } else {
                        "已建立 HID 会话"
                    }
                );
                self.commit_runtime_update();
                Ok(true)
            }
            0 => {
                if let Some(first) = candidates.first() {
                    self.status_message = format!(
                        "已检测到 {} ({:04X}:{:04X})",
                        first.product, first.vid, first.pid
                    );
                } else {
                    self.status_message =
                        "未检测到 Creator Micro 2 USB 连接（离线工作台就绪）".to_string();
                }
                self.commit_runtime_update();
                Ok(false)
            }
            _ => {
                self.status_message =
                    "检测到多台 Creator Micro 2 USB 设备，请只保留一台连接".to_string();
                self.commit_runtime_update();
                Ok(false)
            }
        }
    }

    pub fn handle_hardware_loop_event(
        &mut self,
        event: HardwareLoopEvent,
        now: Instant,
    ) -> Option<HostActionReceipt> {
        self.update_now = now;
        if self.system_lifecycle != SystemLifecycle::Awake {
            return None;
        }
        match event {
            HardwareLoopEvent::Input(input_event) => {
                let session = self.device_session.as_ref()?;
                if session.id != input_event.session_id
                    || (session.is_disconnected() && input_event.kind != InputEventKind::Released)
                {
                    return None;
                }
                self.route_board_input(input_event, now)
            }
            HardwareLoopEvent::Disconnected { session_id } => {
                if self
                    .device_session
                    .as_ref()
                    .is_some_and(|s| s.id == session_id)
                {
                    if let Some(running) = &self.six_layer_running {
                        running.receiver.invalidate("USB 会话已断开");
                    }
                    self.release_board_inputs(now);
                    self.board_routing = BoardRouting::Checking;
                    self.device_session = None;
                    let previous = self.connected_candidate.take();
                    self.hardware_rpc_verified = false;
                    self.lighting_arbiter.invalidate_hardware_cache();
                    self.status_message = "小键盘已断开连接（等待重新插入）".to_string();
                    self.commit_runtime_update();
                    if self.config.auto_connect_usb && !self.config.isolated {
                        let transport = mi_transport::MacTransport::exclusive_vendor();
                        use mi_transport::HidTransport;
                        let reenumerated = transport.enumerate().is_ok_and(|candidates| {
                            candidates.iter().any(|candidate| {
                                mi_transport::is_exclusive_vendor_candidate(candidate)
                                    && previous
                                        .as_ref()
                                        .is_none_or(|old| old.path != candidate.path)
                            })
                        });
                        if reenumerated {
                            let _ = self.connect_default_usb_with_execution(false);
                        }
                    }
                }
                None
            }
            HardwareLoopEvent::DiscoveryChanged { epoch } => {
                if epoch != self.lifecycle_epoch.load(Ordering::Acquire) {
                    return None;
                }
                if !self.config.isolated
                    && self.config.auto_connect_usb
                    && self
                        .six_layer_state
                        .installation
                        .as_ref()
                        .is_some_and(|inst| {
                            inst.status == crate::six_layer_runtime::InstallationStatus::Applied
                                && inst.plan.validate_current().is_ok()
                        })
                {
                    self.resume_requires_explicit_connect = false;
                    self.six_layer_auto_resume_attempts = 0;
                    self.six_layer_next_auto_resume_at = None;
                }
                if self.resume_requires_explicit_connect {
                    return None;
                }
                let old_path_missing = self
                    .mac_watcher_transport
                    .as_ref()
                    .zip(self.connected_candidate.as_ref())
                    .is_some_and(|(transport, old)| {
                        use mi_transport::HidTransport;
                        transport.enumerate().is_ok_and(|candidates| {
                            !candidates
                                .iter()
                                .any(|candidate| candidate.path == old.path)
                        })
                    });
                if self
                    .device_session
                    .as_ref()
                    .is_some_and(|s| s.is_disconnected() || old_path_missing)
                {
                    if let Some(running) = &self.six_layer_running {
                        running.receiver.invalidate("USB 设备身份已变化");
                    }
                    self.release_board_inputs(now);
                    self.board_routing = BoardRouting::Checking;
                    if let Some(session) = self.device_session.take() {
                        session.disconnect();
                    }
                    self.connected_candidate = None;
                    self.hardware_rpc_verified = false;
                    self.lighting_arbiter.invalidate_hardware_cache();
                }
                if !self.config.isolated && !self.device_session_connected() {
                    let _ = self.connect_default_usb_with_execution(false);
                }
                None
            }
            HardwareLoopEvent::HardwareAutoAccepted {
                session_id,
                receipt,
            } => {
                if self
                    .device_session
                    .as_ref()
                    .is_some_and(|s| s.id == session_id)
                {
                    if self.board_routing == BoardRouting::Checking {
                        self.confirm_board_routing(&receipt);
                    }
                    self.hardware_rpc_verified = true;
                    if self.board_routing == BoardRouting::Legacy {
                        self.status_message = format!(
                            "已连接 {} ({}) · 固件 {} 自动验收通过",
                            receipt.product, receipt.vid_pid, receipt.firmware_version
                        );
                    }
                    if let Ok(bytes) = serde_json::to_vec_pretty(&receipt) {
                        let path = self
                            .config
                            .document_dir
                            .join("hardware-auto-acceptance.json");
                        let tmp = path.with_extension("json.tmp");
                        if fs::write(&tmp, bytes).is_ok() {
                            let _ = fs::rename(tmp, path);
                        }
                    }
                    self.last_hardware_auto_acceptance = Some(receipt);
                    self.lighting_arbiter.invalidate_hardware_cache();
                    self.refresh_four_app_lighting();
                    self.commit_runtime_update();
                }
                None
            }
            HardwareLoopEvent::LiveCandidatesUpdated { epoch, catalog } => {
                if epoch != self.lifecycle_epoch.load(Ordering::Acquire) {
                    return None;
                }
                let desired = self.desired_live_session_source();
                if catalog
                    .as_ref()
                    .is_some_and(|c| c.instance.source() != desired)
                {
                    return None;
                }
                self.update_navigation_catalog(catalog);
                None
            }
        }
    }

    pub fn drain_pending_hardware_events(&mut self, now: Instant) -> Vec<HostActionReceipt> {
        self.update_now = now;
        let events = self.hardware_event_tx.take_batch();
        let mut receipts = Vec::new();
        for ev in events {
            if let Some(receipt) = self.handle_hardware_loop_event(ev, now) {
                receipts.push(receipt);
            }
        }
        receipts
    }

    pub async fn recv_next_hardware_event(
        &mut self,
        now: Instant,
    ) -> Option<Option<HostActionReceipt>> {
        let event = self.hardware_event_tx.recv().await;
        Some(self.handle_hardware_loop_event(event, now))
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
        CANONICAL_DESKTOP_AGENT_PROFILES
            .iter()
            .all(|canonical_name| {
                profiles.values().any(|p| {
                    if p.name.trim() != *canonical_name {
                        return false;
                    }

                    if *canonical_name == "Codex" {
                        return p
                            .layer_order
                            .iter()
                            .any(|lid| doc.is_protected_oai_layer(lid));
                    }
                    if *canonical_name == "Antigravity" {
                        return p.layer_order.first().is_some_and(|lid| {
                            !mi_model::preset::antigravity_layer_needs_starter_slot_migration(
                                doc, lid,
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
        self.ensure_default_agent_links()?;
        self.commit_runtime_update();
        Ok(())
    }

    pub fn ensure_default_agent_links(&mut self) -> Result<(), WorkbenchError> {
        if self.document_store.has_saved_host_preferences() {
            return Ok(());
        }
        let mut canonical_ids: Vec<(CanonicalAgent, String)> = Vec::new();
        for agent in CanonicalAgent::ALL {
            if agent.is_manual_fallback() {
                continue;
            }
            if let Some(pid) = self.default_host_profile_id(agent) {
                canonical_ids.push((agent, pid));
            }
        }

        for (agent, pid) in canonical_ids {
            let bundle = agent.bundle_id();
            if self.app_profile_links.get(bundle) != Some(&pid) {
                self.apply_routing_operation(RoutingOperation::Upsert {
                    bundle_id: bundle.to_string(),
                    profile_id: pid.clone(),
                })?;
                self.app_profile_links.insert(bundle.to_string(), pid);
            }
        }

        if self.app_profile_links.contains_key("*") {
            self.apply_routing_operation(RoutingOperation::Remove {
                bundle_id: "*".to_string(),
            })?;
            self.app_profile_links.remove("*");
        }

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
                    && receipt.firmware_version == "0.6.3"
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

    fn route_board_input(&mut self, event: InputEvent, now: Instant) -> Option<HostActionReceipt> {
        if event.kind == InputEventKind::InputStreamGap {
            self.input_lost_session = Some(event.session_id);
            self.release_board_inputs(now);
            self.status_message = "输入流有缺口，已停止原目标动作，请重新连接".into();
            self.commit_runtime_update();
            return None;
        }
        if event.kind == InputEventKind::Released {
            return self.consume_physical_input_event(event, now);
        }
        if self.input_lost_session == Some(event.session_id)
            || self.hardware_event_tx.source_closed(event.session_id)
        {
            return None;
        }
        if self.six_layer_job.is_some() || self.six_layer_running.is_some() {
            return None;
        }
        if self.six_layer_state.installation.is_some() {
            if self.document_store.locked_profile_id().is_some() && event.action_eligible {
                return self.consume_physical_input_event(event, now);
            }
            return None;
        }
        if self.board_routing == BoardRouting::WaitingForFirstLayer {
            if event.ingress_sequence <= self.board_admit_after
                || !event.action_eligible
                || event.board_layer != Some(1)
                || event.kind != InputEventKind::Pressed
            {
                return None;
            }
            self.enter_board_first_layer();
        }
        match self.board_routing {
            BoardRouting::Legacy if event.board_layer.is_none() => {
                return self.consume_physical_input_event(event, now);
            }
            BoardRouting::Managed { layer } => {
                if event.ingress_sequence <= self.board_admit_after || !event.action_eligible {
                    return None;
                }
                let next = event.board_layer.filter(|value| (1..=3).contains(value))?;
                if event.kind == InputEventKind::Pressed {
                    if layer != next {
                        self.release_board_inputs(now);
                    }
                    self.board_routing = BoardRouting::Managed { layer: next };
                    let Some(target) = self
                        .legacy_host_profile_id(next)
                        .filter(|id| !self.is_protected_oai_profile(id))
                    else {
                        self.set_status_message("旧三层绑定缺失或属于原生应用，按键已暂停");
                        return None;
                    };
                    if layer != next
                        || self.resolved_active_profile_id().as_deref() != Some(target.as_str())
                    {
                        self.status_message = format!("板载三层已就绪 · 第 {next} 层");
                        if let Err(error) =
                            self.activate_legacy_board_layer(next, now, layer != next)
                        {
                            self.set_status_message(error.to_string());
                            return None;
                        }
                    }
                } else {
                    return None;
                }
                return self.consume_physical_input_event(event, now);
            }
            _ => {}
        }
        None
    }

    fn release_board_inputs(&mut self, now: Instant) {
        self.update_now = now;
        self.cancel_physical_session_navigation();
        self.dial_pressed = false;
        self.dial_turned_while_pressed = false;
        self.radial_state.cancel();
        let stopped = self.live_executor.stop_owned_actions();
        let _ = self.finish_host_cleanup(stopped);
    }

    fn finish_host_cleanup(&mut self, stopped: Result<(), String>) -> Result<(), String> {
        if self.held_dictation.is_some() {
            self.finish_dictation(stopped.clone());
        } else {
            self.ptt_controller.cancel();
            self.ptt_controller.cleanup_confirmed = stopped.is_ok();
        }
        if let Err(error) = &stopped {
            self.status_message = format!("原目标释放未确认 · {error}");
        }
        stopped
    }

    fn finish_dictation(&mut self, stopped: Result<(), String>) -> Option<HostActionReceipt> {
        let held = self.held_dictation.as_ref()?.clone();
        self.ptt_controller.cancel();
        self.ptt_controller.cleanup_confirmed = stopped.is_ok();
        let (outcome, detail) = match stopped {
            Ok(()) => {
                self.held_dictation = None;
                (
                    "executed",
                    format!(
                        "已结束原目标听写 ({}, {})",
                        held.input.binding_id, held.adapter
                    ),
                )
            }
            Err(error) => ("unconfirmed", format!("原目标听写停止未确认 · {error}")),
        };
        let receipt = HostActionReceipt {
            action_type: "agent_command".into(),
            binding_id: held.input.binding_id,
            profile_id: held.profile_id,
            target: held.adapter,
            outcome: outcome.into(),
            detail: detail.clone(),
            text_byte_len: None,
            timestamp_ms: 0,
        };
        self.status_message = detail;
        self.record_host_action_receipt(receipt.clone());
        self.commit_runtime_update();
        Some(receipt)
    }

    fn recommended_board_plan(&self) -> Result<BoardLayerPlan, WorkbenchError> {
        let required = |id: Option<String>| {
            id.ok_or_else(|| WorkbenchError::Preset("默认六层所需的软件方案未就绪".into()))
        };
        Ok(BoardLayerPlan::recommended(
            required(self.find_canonical_profile_id(CanonicalAgent::Codex))?,
            required(self.find_canonical_profile_id(CanonicalAgent::Antigravity))?,
            required(self.find_canonical_profile_id(CanonicalAgent::DeepseekHarness))?,
        ))
    }

    fn initialize_board_plan(&mut self) -> Result<(), WorkbenchError> {
        if self.document_store.board_layer_plan().is_none() {
            let plan = self.recommended_board_plan()?;
            self.document_store.initialize_board_plan_if_absent(plan);
        }
        Ok(())
    }

    fn route_for_profile(&self, profile_id: &str) -> Result<BoardLayerRoute, WorkbenchError> {
        self.projection_inputs().route_for_profile(profile_id)
    }

    fn save_board_plan(
        &mut self,
        plan: BoardLayerPlan,
        preview: BoardPreviewSelection,
        usb: Option<String>,
    ) -> Result<(), WorkbenchError> {
        self.document_store
            .save_board_configuration(plan, preview, usb)
            .map_err(WorkbenchError::Storage)?;
        self.commit_local_update();
        Ok(())
    }

    pub fn preview_board_layer(&mut self, layer: BoardLayer) -> Result<(), WorkbenchError> {
        if self.document_store.editing_device() {
            return Err(WorkbenchError::Storage(
                "请先返回软件方案，再查看键盘层".into(),
            ));
        }
        let profile_id = self
            .document_store
            .board_layer_plan()
            .and_then(|plan| plan.route(layer).profile_id())
            .filter(|id| {
                self.document_store
                    .draft
                    .document
                    .editable
                    .graph
                    .profiles
                    .contains_key(*id)
            })
            .map(str::to_owned);
        self.preview_board_selection(BoardPreviewSelection::Layer(layer))?;
        self.set_browsed_profile(profile_id);
        self.commit_local_update();
        Ok(())
    }

    fn preview_board_selection(
        &mut self,
        preview: BoardPreviewSelection,
    ) -> Result<(), WorkbenchError> {
        self.save_board_plan(
            self.document_store
                .board_layer_plan()
                .cloned()
                .unwrap_or_default(),
            preview,
            self.document_store
                .usb_startup_profile_id()
                .map(str::to_owned),
        )
    }

    pub fn assign_board_layer(
        &mut self,
        layer: BoardLayer,
        profile_id: Option<String>,
    ) -> Result<(), WorkbenchError> {
        let route = profile_id
            .as_deref()
            .map(|id| self.route_for_profile(id))
            .transpose()?
            .unwrap_or_default();
        let mut plan = self
            .document_store
            .board_layer_plan()
            .cloned()
            .unwrap_or_default();
        plan.replace(layer, route);
        self.save_board_plan(
            plan,
            self.document_store.board_preview_selection(),
            self.document_store
                .usb_startup_profile_id()
                .map(str::to_owned),
        )
    }

    pub fn set_usb_startup_profile(
        &mut self,
        profile_id: Option<String>,
    ) -> Result<(), WorkbenchError> {
        if let Some(id) = &profile_id {
            self.route_for_profile(id)?;
        }
        self.save_board_plan(
            self.document_store
                .board_layer_plan()
                .cloned()
                .unwrap_or_default(),
            self.document_store.board_preview_selection(),
            profile_id,
        )
    }

    pub fn reset_board_layer_plan(&mut self) -> Result<(), WorkbenchError> {
        let plan = self.recommended_board_plan()?;
        self.save_board_plan(
            plan,
            self.document_store.board_preview_selection(),
            self.document_store
                .usb_startup_profile_id()
                .map(str::to_owned),
        )
    }

    pub fn browse_profile(&mut self, profile_id: &str) -> Result<(), WorkbenchError> {
        if !self
            .document_store
            .draft
            .document
            .editable
            .graph
            .profiles
            .contains_key(profile_id)
        {
            return Err(WorkbenchError::Routing("方案不存在".into()));
        }
        self.set_browsed_profile(Some(profile_id.to_owned()));
        self.commit_local_update();
        Ok(())
    }

    fn set_browsed_profile(&mut self, profile_id: Option<String>) {
        if self.selected_profile_id == profile_id {
            return;
        }
        self.selected_layer_id = profile_id.as_ref().and_then(|id| {
            self.document_store
                .draft
                .document
                .editable
                .graph
                .profiles
                .get(id)
                .and_then(|profile| profile.layer_order.first().cloned())
        });
        self.selected_profile_id = profile_id;
        self.normalize_surface_for_current_layer();
    }

    pub fn select_and_activate_profile(&mut self, profile_id: &str) -> Result<(), WorkbenchError> {
        let started = Instant::now();
        let result = self.select_and_activate_profile_at(profile_id, started, true);
        let elapsed = started.elapsed();
        self.last_switch_duration = elapsed;
        self.projection.last_switch_duration = elapsed;
        result
    }

    pub fn select_and_activate_profile_at(
        &mut self,
        profile_id: &str,
        now: Instant,
        emit_hud_notice: bool,
    ) -> Result<(), WorkbenchError> {
        self.update_now = now;
        let first_layer_id = self
            .document_store
            .draft
            .document
            .editable
            .graph
            .profiles
            .get(profile_id)
            .ok_or_else(|| WorkbenchError::Routing("Profile 不存在".to_string()))?
            .layer_order
            .first()
            .cloned();

        self.apply_routing_operation(RoutingOperation::Select {
            profile_id: profile_id.to_string(),
        })?;
        self.manual_override = true;
        self.selected_profile_id = Some(profile_id.to_string());
        self.selected_layer_id = first_layer_id;
        self.normalize_surface_for_current_layer();
        self.sync_six_layer_routing_context();
        self.sync_live_session_source();
        self.refresh_four_app_lighting();
        if emit_hud_notice && !self.overlay_controller.is_cheat_sheet_visible() {
            let profile_name = self.active_profile_display_name();
            let mode_summary = self.routing_mode_summary_label();
            let empty_sheet = CheatSheetProjection::default();
            self.overlay_controller.request_profile_switch_notice(
                profile_name,
                mode_summary,
                &empty_sheet,
                &self.screen_geometry,
                self.last_cursor_position,
                now,
            );
        }
        self.commit_runtime_update();
        if self.projection.header.banner.is_caution() {
            self.banner_flash_count += 1;
        }
        Ok(())
    }

    pub fn foreground_changed(&mut self, bundle_id: Option<&str>) -> bool {
        let changed = self.apply_foreground_changed_routing(bundle_id);
        self.sync_live_session_source();
        changed
    }

    fn apply_foreground_changed_routing(&mut self, bundle_id: Option<&str>) -> bool {
        let bundle_id = bundle_id
            .map(str::trim)
            .filter(|bundle| !bundle.is_empty() && *bundle != "*");
        self.refresh_accessibility_trusted();
        if bundle_id == Some("com.worklouder.input") {
            self.applications
                .insert("com.worklouder.input".to_string(), true);
        } else {
            self.refresh_competing_input_app_running(false);
        }
        self.navigation_foreground_changed(bundle_id);
        self.foreground_bundle_id = bundle_id.map(str::to_owned);
        if let Some(running) = &self.six_layer_running {
            let changed = running.foreground_changed(bundle_id);
            if changed {
                self.commit_runtime_update();
            }
            return changed;
        }
        if self.document_store.locked_profile_id().is_some() {
            if bundle_id == Some("com.worklouder.input") {
                self.commit_runtime_update();
            }
            return false;
        }
        let Some(bundle) = bundle_id else {
            return false;
        };
        if self.board_routing.is_managed() || !self.auto_follow_enabled || self.manual_override {
            return false;
        }
        let Some(target_pid) = self.app_profile_links.get(bundle).cloned() else {
            return false;
        };
        if !self
            .document_store
            .draft
            .document
            .editable
            .graph
            .profiles
            .contains_key(&target_pid)
        {
            return false;
        }
        if self.document_store.host_active_profile_id().as_deref() == Some(target_pid.as_str())
            && self.selected_profile_id.as_deref() == Some(target_pid.as_str())
        {
            return false;
        }

        let started = self.update_now;
        if self
            .apply_routing_operation(RoutingOperation::Follow {
                bundle_id: bundle.to_string(),
            })
            .is_err()
        {
            return false;
        }
        self.selected_profile_id = Some(target_pid.clone());
        self.selected_layer_id = self
            .document_store
            .draft
            .document
            .editable
            .graph
            .profiles
            .get(&target_pid)
            .and_then(|p| p.layer_order.first().cloned());
        self.normalize_surface_for_current_layer();
        self.refresh_four_app_lighting();
        if !self.overlay_controller.is_cheat_sheet_visible() {
            let profile_name = self.active_profile_display_name();
            let mode_summary = self.routing_mode_summary_label();
            let empty_sheet = CheatSheetProjection::default();
            self.overlay_controller.request_profile_switch_notice(
                profile_name,
                mode_summary,
                &empty_sheet,
                &self.screen_geometry,
                self.last_cursor_position,
                started,
            );
        }
        self.commit_runtime_update();
        if self.projection.header.banner.is_caution() {
            self.banner_flash_count += 1;
        }
        true
    }

    pub fn set_host_execution_enabled(&mut self, enabled: bool) -> Result<(), WorkbenchError> {
        self.apply_routing_operation(RoutingOperation::Execution { enabled })?;
        if let Some(running) = &self.six_layer_running {
            running.set_execution_policy(enabled, self.command_execution_enabled, self.update_now);
        }
        if !enabled {
            self.release_board_inputs(self.update_now);
        }
        self.commit_runtime_update();
        Ok(())
    }

    pub fn set_auto_follow(&mut self, enabled: bool) -> Result<(), WorkbenchError> {
        self.apply_routing_operation(RoutingOperation::Enable { enabled })?;
        self.auto_follow_enabled = enabled;
        if enabled && self.manual_override && self.document_store.locked_profile_id().is_none() {
            self.apply_routing_operation(RoutingOperation::Resume)?;
            self.manual_override = false;
        }
        self.sync_six_layer_routing_context();
        self.sync_live_session_source();
        self.commit_runtime_update();
        Ok(())
    }

    pub fn toggle_auto_follow_pause(&mut self) -> Result<(), WorkbenchError> {
        if !self.auto_follow_enabled {
            return self.set_auto_follow(true);
        }
        if self.manual_override || self.document_store.locked_profile_id().is_some() {
            self.apply_routing_operation(RoutingOperation::Resume)?;
            self.manual_override = false;
            self.auto_follow_enabled = true;
        } else if let Some(pid) = self
            .selected_profile_id
            .clone()
            .or_else(|| self.document_store.host_active_profile_id())
        {
            self.apply_routing_operation(RoutingOperation::Select { profile_id: pid })?;
            self.manual_override = true;
        }
        self.sync_six_layer_routing_context();
        self.sync_live_session_source();
        self.commit_runtime_update();
        Ok(())
    }

    pub fn set_show_other_profiles(&mut self, show: bool) {
        self.show_other_profiles = show;
        self.ensure_selected_profile_visible();
        self.commit_runtime_update();
    }

    pub fn undo_draft(&mut self, redo: bool) -> Result<(), WorkbenchError> {
        self.document_store
            .update(|draft| draft.undo(redo))
            .map_err(WorkbenchError::Storage)?;
        self.ensure_selected_profile_visible();
        self.normalize_surface_for_current_layer();
        self.sync_six_layer_routing_context();
        self.commit_runtime_update();
        Ok(())
    }

    pub fn apply_profile_command(&mut self, cmd: ProfileCommand) -> Result<(), WorkbenchError> {
        match cmd {
            ProfileCommand::Create { name } => {
                let mut created_id = String::new();
                self.document_store
                    .update(|draft| {
                        created_id = draft.create_profile(&name)?;
                        Ok(())
                    })
                    .map_err(WorkbenchError::Storage)?;
                if CanonicalAgent::from_profile_name(&name).is_none() {
                    self.show_other_profiles = true;
                }
                self.selected_profile_id = Some(created_id.clone());
                self.selected_layer_id = self
                    .document_store
                    .draft
                    .document
                    .editable
                    .graph
                    .profiles
                    .get(&created_id)
                    .and_then(|p| p.layer_order.first().cloned());
                self.status_message = format!("已新建方案「{name}」");
            }
            ProfileCommand::Rename { profile_id, name } => {
                self.document_store
                    .update(|draft| draft.rename_profile(&profile_id, &name))
                    .map_err(WorkbenchError::Storage)?;
                self.status_message = format!("已重命名为「{name}」");
            }
            ProfileCommand::Duplicate { profile_id, name } => {
                let mut duplicated_id = String::new();
                self.document_store
                    .update(|draft| {
                        duplicated_id = draft.duplicate_profile(&profile_id, &name)?;
                        Ok(())
                    })
                    .map_err(WorkbenchError::Storage)?;
                if CanonicalAgent::from_profile_name(&name).is_none() {
                    self.show_other_profiles = true;
                }
                self.selected_profile_id = Some(duplicated_id.clone());
                self.selected_layer_id = self
                    .document_store
                    .draft
                    .document
                    .editable
                    .graph
                    .profiles
                    .get(&duplicated_id)
                    .and_then(|p| p.layer_order.first().cloned());
                self.status_message = format!("已复制方案「{name}」");
            }
            ProfileCommand::MoveUp { profile_id } => {
                let order = &self
                    .document_store
                    .draft
                    .document
                    .editable
                    .graph
                    .profile_order;
                if let Some(pos) = order.iter().position(|id| id == &profile_id) {
                    if pos > 0 {
                        self.document_store
                            .update(|draft| draft.move_profile(&profile_id, pos - 1))
                            .map_err(WorkbenchError::Storage)?;
                    }
                }
            }
            ProfileCommand::MoveDown { profile_id } => {
                let order = &self
                    .document_store
                    .draft
                    .document
                    .editable
                    .graph
                    .profile_order;
                let len = order.len();
                if let Some(pos) = order.iter().position(|id| id == &profile_id) {
                    if pos + 1 < len {
                        self.document_store
                            .update(|draft| draft.move_profile(&profile_id, pos + 1))
                            .map_err(WorkbenchError::Storage)?;
                    }
                }
            }
            ProfileCommand::Delete { profile_id } => {
                self.document_store
                    .update(|draft| draft.delete_profile(&profile_id))
                    .map_err(WorkbenchError::Storage)?;
                if self.selected_profile_id.as_deref() == Some(profile_id.as_str()) {
                    self.selected_profile_id = None;
                }
                self.sync_routing_snapshot();
                self.ensure_selected_profile_visible();
                self.status_message = "已删除方案".to_string();
            }
            ProfileCommand::InstallCodingAgentPresets => {
                self.document_store
                    .update(|draft| {
                        mi_model::preset::install_agent_starter_profiles(draft).map(|_| ())
                    })
                    .map_err(WorkbenchError::Preset)?;
                self.ensure_default_agent_links()?;
                self.status_message = "已恢复默认软件方案".to_string();
            }
        }
        self.sync_six_layer_routing_context();
        self.sync_live_session_source();
        self.commit_runtime_update();
        Ok(())
    }

    pub fn apply_backup_command(
        &mut self,
        cmd: BackupCommand,
    ) -> Result<Option<String>, WorkbenchError> {
        match cmd {
            BackupCommand::ExportPreset {
                profile_id,
                layer_id,
            } => {
                let exported = mi_model::preset::export(
                    &self.document_store.draft,
                    profile_id.as_deref(),
                    layer_id.as_deref(),
                )
                .map_err(WorkbenchError::Preset)?;
                self.last_exported_preset = Some(exported.clone());
                self.status_message = "已导出方案预设".to_string();
                self.commit_runtime_update();
                Ok(Some(exported))
            }
            BackupCommand::ImportPreset { text } => {
                let mut imported =
                    mi_model::preset::import(&text).map_err(WorkbenchError::Preset)?;
                imported.revision = self.document_store.draft.revision + 1;
                self.document_store
                    .update(|draft| {
                        *draft = imported;
                        Ok(())
                    })
                    .map_err(WorkbenchError::Storage)?;
                self.sync_routing_snapshot();
                self.prepare_desktop_profiles()?;
                self.ensure_default_agent_links()?;
                self.show_other_profiles = true;
                self.ensure_selected_profile_visible();
                self.status_message = "已导入方案预设".to_string();
                self.commit_runtime_update();
                Ok(None)
            }
            BackupCommand::ReturnToHostDocument => {
                self.document_store.return_to_host();
                self.sync_routing_snapshot();
                self.ensure_selected_profile_visible();
                self.status_message = "已返回常用软件方案".to_string();
                self.commit_runtime_update();
                Ok(None)
            }
        }
    }

    pub fn dispatch_header_action(&mut self, action: HeaderAction) -> Result<(), WorkbenchError> {
        match action {
            HeaderAction::ConnectKeyboard => {
                if self.config.isolated {
                    self.status_message = "正在检测小键盘连接…".to_string();
                    self.commit_runtime_update();
                } else {
                    self.start_mac_usb_discovery_watcher();
                    if let Err(err) = self.connect_default_usb() {
                        self.status_message = err.to_string();
                        self.commit_runtime_update();
                    }
                }
                Ok(())
            }
            HeaderAction::ReturnToHostDocument => self
                .apply_backup_command(BackupCommand::ReturnToHostDocument)
                .map(|_| ()),
            HeaderAction::SetExecutionEnabled(enabled) => self.set_host_execution_enabled(enabled),
            HeaderAction::Undo => self.undo_draft(false),
            HeaderAction::Redo => self.undo_draft(true),
            HeaderAction::OpenAccessibilitySettings => {
                self.open_accessibility_settings();
                Ok(())
            }
            HeaderAction::OpenSettings => {
                self.refresh_accessibility_trusted();
                self.refresh_competing_input_app_running(true);
                self.status_menu_open = !self.status_menu_open;
                self.status_message = if self.status_menu_open {
                    "已打开高级设置".to_string()
                } else {
                    "已关闭高级设置".to_string()
                };
                self.commit_runtime_update();
                Ok(())
            }
        }
    }

    pub fn dispatch_sidebar_action(&mut self, action: SidebarAction) -> Result<(), WorkbenchError> {
        match action {
            SidebarAction::SelectProfile(profile_id) => {
                self.select_and_activate_profile_at(&profile_id, self.update_now, true)
            }
            SidebarAction::BrowseProfile(profile_id) => self.browse_profile(&profile_id),
            SidebarAction::PreviewBoardLayer(layer) => self.preview_board_layer(layer),
            SidebarAction::PreviewUsbStartup => {
                self.preview_board_selection(BoardPreviewSelection::UsbStartup)
            }
            SidebarAction::ProvisionBoardLayers => self.provision_board_layers(),
            SidebarAction::RestoreBoardLayers => self.start_six_layer_restore(),
            SidebarAction::SetBoardLayerProfile { layer, profile_id } => {
                self.assign_board_layer(layer, profile_id)
            }
            SidebarAction::SetUsbStartupProfile(profile_id) => {
                self.set_usb_startup_profile(profile_id)
            }
            SidebarAction::ResetBoardLayerPlan => self.reset_board_layer_plan(),
            SidebarAction::SetAutoFollow(enabled) => self.set_auto_follow(enabled),
            SidebarAction::SetProfileLocked(locked) => self.set_profile_locked(locked, true),
            SidebarAction::ToggleAutoFollowPause => self.toggle_auto_follow_pause(),
            SidebarAction::SetShowOtherProfiles(show) => {
                self.set_show_other_profiles(show);
                Ok(())
            }
            SidebarAction::Profile(cmd) => self.apply_profile_command(cmd),
            SidebarAction::Backup(cmd) => self.apply_backup_command(cmd).map(|_| ()),
        }
    }

    pub fn navigation_history(&self) -> &NavigationHistory {
        &self.navigation_history
    }

    pub fn find_canonical_profile_id(&self, agent: CanonicalAgent) -> Option<String> {
        self.document_store
            .draft
            .document
            .editable
            .graph
            .profiles
            .iter()
            .find(|(_, p)| {
                p.name != mi_model::preset::LOCAL_CODEX_PROFILE_NAME
                    && CanonicalAgent::from_profile_name(&p.name) == Some(agent)
            })
            .map(|(pid, _)| pid.clone())
    }

    fn local_codex_profile_id(&self) -> Option<String> {
        self.document_store
            .draft
            .document
            .editable
            .graph
            .profiles
            .iter()
            .find(|(_, p)| p.name == mi_model::preset::LOCAL_CODEX_PROFILE_NAME)
            .map(|(pid, _)| pid.clone())
    }

    fn default_host_profile_id(&self, agent: CanonicalAgent) -> Option<String> {
        let profiles = &self
            .document_store
            .host_store()
            .draft
            .document
            .editable
            .graph
            .profiles;
        let canonical_match = || {
            profiles
                .iter()
                .find(|(_, p)| {
                    p.name != mi_model::preset::LOCAL_CODEX_PROFILE_NAME
                        && CanonicalAgent::from_profile_name(&p.name) == Some(agent)
                })
                .map(|(pid, _)| pid.clone())
        };
        if agent == CanonicalAgent::Codex {
            profiles
                .iter()
                .find(|(_, p)| p.name == mi_model::preset::LOCAL_CODEX_PROFILE_NAME)
                .map(|(pid, _)| pid.clone())
                .or_else(canonical_match)
        } else {
            canonical_match()
        }
    }

    pub fn is_selected_layer_protected(&self) -> bool {
        self.projection_inputs().is_selected_layer_protected()
    }

    pub fn is_active_lighting_protected(&self) -> bool {
        self.projection_inputs().is_active_lighting_protected()
    }

    pub fn ensure_layer_editable(&self) -> Result<(), WorkbenchError> {
        if self.is_selected_layer_protected() {
            return Err(WorkbenchError::ProtectedLayerLocked);
        }
        Ok(())
    }

    pub fn dispatch_canvas_action(&mut self, action: CanvasAction) -> Result<(), WorkbenchError> {
        match action {
            CanvasAction::SelectSurface(mut surface) => {
                let layout = self.current_bottom_row_layout();
                if surface == ControlSurfaceId::Key11 && layout == BottomRowLayout::Merged {
                    surface = ControlSurfaceId::Key10;
                }
                self.selected_surface = surface;
                let slot = self.effective_slot_for_surface(surface);
                self.selected_binding_slot = Some(slot);
                self.commit_runtime_update();
                Ok(())
            }
            CanvasAction::SelectLogicalSlot(slot) => {
                let layout = self.current_bottom_row_layout();
                if let Some(surface) = ControlSurfaceId::from_logical_slot(slot, layout) {
                    self.selected_surface = surface;
                    self.selected_binding_slot = Some(slot);
                    if (16..=19).contains(&slot)
                        && self.selected_joystick_direction.logical_sector_slot() != slot
                    {
                        self.selected_joystick_direction = match slot {
                            16 => JoystickDirection8::North,
                            17 => JoystickDirection8::South,
                            18 => JoystickDirection8::West,
                            _ => JoystickDirection8::East,
                        };
                    }
                    self.commit_runtime_update();
                }
                Ok(())
            }
            CanvasAction::SelectJoystickDirection(dir) => {
                self.selected_surface = ControlSurfaceId::Joystick;
                self.selected_joystick_direction = dir;
                self.selected_binding_slot = Some(dir.logical_sector_slot());
                self.commit_runtime_update();
                Ok(())
            }
            CanvasAction::ClickCanvasPoint {
                x_milli_px,
                y_milli_px,
            } => {
                let px_x = x_milli_px as f32 / 1000.0;
                let px_y = y_milli_px as f32 / 1000.0;
                if let Some(dir) = self
                    .projection
                    .canvas
                    .hit_test_joystick_direction(px_x, px_y)
                {
                    self.selected_surface = ControlSurfaceId::Joystick;
                    self.selected_joystick_direction = dir;
                    self.selected_binding_slot = Some(dir.logical_sector_slot());
                    self.commit_runtime_update();
                    return Ok(());
                }
                if let Some(surface) = self.projection.canvas.hit_test_point(px_x, px_y) {
                    self.selected_surface = surface;
                    let slot = self.effective_slot_for_surface(surface);
                    self.selected_binding_slot = Some(slot);
                    self.commit_runtime_update();
                }
                Ok(())
            }
            CanvasAction::SelectLayer(layer_id) => {
                if self
                    .document_store
                    .draft
                    .document
                    .editable
                    .graph
                    .layers
                    .contains_key(&layer_id)
                {
                    self.selected_layer_id = Some(layer_id);
                    self.normalize_surface_for_current_layer();
                    self.refresh_four_app_lighting();
                    self.commit_runtime_update();
                }
                Ok(())
            }
            CanvasAction::SetBottomRowLayout(layout) => {
                self.ensure_layer_editable()?;
                let _ = self.apply_routing_operation(RoutingOperation::BottomRowLayout { layout });
                self.bottom_row_layout = layout;
                self.merge_wide_key
                    .store(layout.is_merged(), Ordering::SeqCst);
                self.normalize_surface_for_current_layer();
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
                Ok(())
            }
            CanvasAction::MergeBottomRowAcrossLayers => {
                self.ensure_layer_editable()?;
                let pid = self
                    .selected_profile_id
                    .clone()
                    .ok_or_else(|| WorkbenchError::Storage("未选择方案".to_string()))?;
                self.document_store
                    .update(|draft| {
                        draft.mutate(|doc| {
                            doc.merge_keys_across_layers(&pid, 10, 11, None)?;
                            Ok(())
                        })
                    })
                    .map_err(WorkbenchError::Storage)?;
                let _ = self.apply_routing_operation(RoutingOperation::BottomRowLayout {
                    layout: BottomRowLayout::Merged,
                });
                self.bottom_row_layout = BottomRowLayout::Merged;
                self.merge_wide_key.store(true, Ordering::SeqCst);
                self.normalize_surface_for_current_layer();
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
                Ok(())
            }
            CanvasAction::SetShowKeycapIcons(show) => {
                self.show_keycap_icons = show;
                self.commit_runtime_update();
                Ok(())
            }
            CanvasAction::SetShowKeycapLabels(show) => {
                self.show_keycap_labels = show;
                self.commit_runtime_update();
                Ok(())
            }
            CanvasAction::ApplyKnobJoystickPreset(preset) => {
                self.ensure_layer_editable()?;
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                if let Some(profile) = self
                    .document_store
                    .draft
                    .document
                    .editable
                    .graph
                    .profiles
                    .get(&pid)
                {
                    let agent = CanonicalAgent::from_profile_name(&profile.name);
                    if agent == Some(CanonicalAgent::Codex) && !preset.is_codex() {
                        return Err(WorkbenchError::Preset(
                            "Codex 方案不支持 Antigravity 旋钮与摇杆预设".into(),
                        ));
                    }
                    if agent == Some(CanonicalAgent::Antigravity) && preset.is_codex() {
                        return Err(WorkbenchError::Preset(
                            "Antigravity 方案不支持 Codex 旋钮预设".into(),
                        ));
                    }
                }
                let pairs: &[(u32, &str)] = match preset {
                    KnobJoystickPreset::CycleWorkingAndBlueDot => &[
                        (13, "ag-cycle-attention-prev"),
                        (14, "ag-cycle-attention-next"),
                        (15, "four-app-attention"),
                        (16, "ag-cycle-working-prev"),
                        (17, "ag-cycle-working-next"),
                        (18, "ag-cycle-completed-prev"),
                        (19, "ag-cycle-completed-next"),
                    ],
                    KnobJoystickPreset::DualScrollProjectsAndChat => &[
                        (13, "ag-sidebar-scroll-up"),
                        (14, "ag-sidebar-scroll-down"),
                        (15, "agent-submit"),
                        (16, "ag-prev-conversation"),
                        (17, "ag-next-conversation"),
                        (18, "ag-prev-project"),
                        (19, "ag-next-project"),
                    ],
                    KnobJoystickPreset::CodexScrollChatAndBottom => &[
                        (13, "codex-scroll-up"),
                        (14, "codex-scroll-down"),
                        (15, "codex-scroll-to-bottom"),
                    ],
                    KnobJoystickPreset::CodexReasoningEffortAndSpeed => &[
                        (13, "codex-effort-down"),
                        (14, "codex-effort-up"),
                        (15, "codex-toggle-fast-mode"),
                    ],
                };
                let catalog = mi_model::draft::actions();
                self.document_store
                    .update(|draft| {
                        for &(slot, action_id) in pairs {
                            let action = catalog
                                .iter()
                                .find(|a| a.id == action_id)
                                .cloned()
                                .ok_or_else(|| format!("未找到内置动作 {action_id}"))?;
                            draft.assign(&json!(pid), &json!(lid), slot, action)?;
                        }
                        Ok(())
                    })
                    .map_err(WorkbenchError::Storage)?;
                self.commit_runtime_update();
                Ok(())
            }
            CanvasAction::AssignSlotAction { slot, action_id } => {
                self.ensure_layer_editable()?;
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                let action = self.resolve_action_by_id(&action_id)?;
                self.document_store
                    .update(|draft| draft.assign(&json!(pid), &json!(lid), slot, action))
                    .map_err(WorkbenchError::Storage)?;
                self.commit_runtime_update();
                Ok(())
            }
            CanvasAction::UnlockBySwitchingOrCreatingCustomLayer => {
                let pid = self
                    .selected_profile_id
                    .clone()
                    .ok_or_else(|| WorkbenchError::Storage("未选择方案".to_string()))?;
                let existing_editable = self
                    .document_store
                    .draft
                    .document
                    .editable
                    .graph
                    .profiles
                    .get(&pid)
                    .and_then(|p| {
                        p.layer_order
                            .iter()
                            .find(|lid| {
                                !self
                                    .document_store
                                    .draft
                                    .document
                                    .is_protected_oai_layer(lid)
                            })
                            .cloned()
                    });
                let target_lid = if let Some(lid) = existing_editable {
                    lid
                } else {
                    let mut created_lid = String::new();
                    self.document_store
                        .update(|draft| {
                            created_lid = draft.create_layer(&pid, "自定义层")?;
                            Ok(())
                        })
                        .map_err(WorkbenchError::Storage)?;
                    created_lid
                };
                self.selected_layer_id = Some(target_lid);
                self.normalize_surface_for_current_layer();
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
                Ok(())
            }
        }
    }

    pub fn dispatch_session_strip_action(
        &mut self,
        action: SessionStripAction,
    ) -> Result<(), WorkbenchError> {
        self.require_awake()?;
        match action {
            SessionStripAction::ActivateSlot(slot_index) => {
                self.request_navigation_command(
                    NavigationCommand::Slot(slot_index),
                    None,
                    None,
                    "workbench".into(),
                    "session-strip".into(),
                );
                Ok(())
            }
            SessionStripAction::TogglePinSlot(slot_index) => {
                if slot_index >= SLOT_COUNT {
                    return Ok(());
                }
                let slot = self.slot_table.slot_table().slots()[slot_index].clone();
                let acks = self.lighting_arbiter.acknowledged_watermarks.clone();
                if slot.mode == mi_model::slot_table::SlotMode::Pinned {
                    self.slot_table
                        .unpin_slot(slot_index, &acks)
                        .map_err(WorkbenchError::Storage)?;
                } else if let Some(place) = slot.place {
                    self.slot_table
                        .pin_slot(
                            slot_index,
                            place,
                            slot.title,
                            slot.updated_at.unwrap_or(0),
                            &acks,
                        )
                        .map_err(WorkbenchError::Storage)?;
                }
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
                Ok(())
            }
            SessionStripAction::JumpToSession(place) => {
                self.request_session_navigation(place);
                Ok(())
            }
            SessionStripAction::AcknowledgeAllCompleted => {
                let to_ack: Vec<_> = self
                    .slot_table
                    .candidates()
                    .iter()
                    .filter(|c| c.has_unviewed_completion_timestamp())
                    .map(|c| (c.place.clone(), c.watermark.clone()))
                    .collect();
                for (place, wm) in to_ack {
                    self.lighting_arbiter.acknowledge_slot(&place, wm);
                }
                let acks = self.lighting_arbiter.acknowledged_watermarks.clone();
                self.slot_table.refresh_with_watermarks(&acks);
                self.lighting_popover.manual_preview_phase = None;
                self.lighting_arbiter.manual_preview_phase = None;
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
                Ok(())
            }
            SessionStripAction::BindTopSixAsSessionKeys => {
                self.ensure_layer_editable()?;
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                let catalog = mi_model::draft::actions();
                self.document_store
                    .update(|draft| {
                        for slot in 0..6u32 {
                            let target_id = format!("slot-open-{slot}");
                            let action = catalog
                                .iter()
                                .find(|a| a.id == target_id)
                                .cloned()
                                .ok_or_else(|| format!("未找到动作 {target_id}"))?;
                            draft.assign(&json!(pid), &json!(lid), slot, action)?;
                        }
                        Ok(())
                    })
                    .map_err(WorkbenchError::Storage)?;
                self.commit_runtime_update();
                Ok(())
            }
            SessionStripAction::RestoreDefaultLayout => {
                self.ensure_layer_editable()?;
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                let profile_name = self
                    .document_store
                    .draft
                    .document
                    .editable
                    .graph
                    .profiles
                    .get(&pid)
                    .map(|p| p.name.clone())
                    .unwrap_or_default();

                let mut template = Draft::default();
                mi_model::preset::install_agent_starter_profiles(&mut template)
                    .map_err(WorkbenchError::Preset)?;

                let template_layer_and_actions = template
                    .document
                    .editable
                    .graph
                    .profiles
                    .values()
                    .find(|p| p.name == profile_name)
                    .and_then(|p| p.layer_order.first())
                    .and_then(|tlid| {
                        template
                            .document
                            .editable
                            .graph
                            .layers
                            .get(tlid)
                            .cloned()
                            .map(|layer| (layer, template.document.editable.graph.actions.clone()))
                    });

                self.document_store
                    .update(|draft| {
                        if let Some((tlayer, tactions)) = template_layer_and_actions {
                            for slot in 0..20u32 {
                                let logical = format!("logical-{slot}");
                                let maybe_action = tlayer
                                    .controls
                                    .values()
                                    .find(|b| b.logical_control == logical)
                                    .and_then(|b| b.action_id.as_ref())
                                    .and_then(|aid| tactions.get(aid))
                                    .cloned();
                                if let Some(action) = maybe_action {
                                    draft.assign(&json!(pid), &json!(lid), slot, action)?;
                                } else {
                                    draft.unassign(&json!(pid), &json!(lid), slot)?;
                                }
                            }
                        } else {
                            mi_model::preset::install_agent_starter_profiles(draft)?;
                        }
                        Ok(())
                    })
                    .map_err(WorkbenchError::Storage)?;
                let _ = self.apply_routing_operation(RoutingOperation::BottomRowLayout {
                    layout: BottomRowLayout::Split,
                });
                self.bottom_row_layout = BottomRowLayout::Split;
                self.merge_wide_key.store(false, Ordering::SeqCst);
                self.normalize_surface_for_current_layer();
                self.commit_runtime_update();
                Ok(())
            }
            SessionStripAction::ToggleLightingPopover => {
                self.lighting_popover.is_open = !self.lighting_popover.is_open;
                self.commit_runtime_update();
                Ok(())
            }
            SessionStripAction::SetLightingEnabled(enabled) => {
                self.lighting_popover.lighting_enabled = enabled;
                self.lighting_arbiter.lighting_enabled = enabled;
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
                Ok(())
            }
            SessionStripAction::SetAutoSessionColor(auto_col) => {
                self.lighting_popover.auto_session_color = auto_col;
                self.lighting_arbiter.session_lighting_linked = auto_col;
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
                Ok(())
            }
            SessionStripAction::SetOnlyGlowOnAttentionOrCompleted(only_done) => {
                self.lighting_popover.only_glow_on_attention_or_completed = only_done;
                self.lighting_arbiter.only_glow_on_attention_or_completed = only_done;
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
                Ok(())
            }
            SessionStripAction::SetGlowOnWorking(glow_working) => {
                self.lighting_popover.glow_on_working = glow_working;
                self.lighting_arbiter.glow_on_working_sessions = glow_working;
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
                Ok(())
            }
            SessionStripAction::SetAmbientColor(preset) => {
                self.lighting_popover.ambient_color = preset;
                self.lighting_arbiter.idle_color_hex = preset.hex().to_string();
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
                Ok(())
            }
            SessionStripAction::SetWorkingColor(preset) => {
                self.lighting_popover.working_color = preset;
                self.lighting_arbiter.working_color_hex = preset.hex().to_string();
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
                Ok(())
            }
            SessionStripAction::SetBrightnessPreset(preset) => {
                self.lighting_popover.brightness = preset;
                self.lighting_arbiter.brightness_scale = preset.scale();
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
                Ok(())
            }
            SessionStripAction::SetAutoOffTimeout(preset) => {
                self.lighting_popover.auto_off_timeout = preset;
                self.commit_runtime_update();
                Ok(())
            }
            SessionStripAction::TriggerTestLighting(phase) => {
                if self.is_active_lighting_protected() {
                    return Err(WorkbenchError::ProtectedLayerLocked);
                }
                self.lighting_popover.manual_preview_phase = phase;
                self.lighting_arbiter.manual_preview_phase = phase;
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
                Ok(())
            }
        }
    }

    pub fn ingest_four_app_candidates(&mut self, candidates: Vec<FourAppLiveCandidate>) {
        let acks = self.lighting_arbiter.acknowledged_watermarks.clone();
        self.slot_table.update_candidates(candidates, &acks);
        self.refresh_four_app_lighting();
        self.commit_runtime_update();
    }

    pub fn refresh_four_app_lighting(&mut self) {
        let acks = self.lighting_arbiter.acknowledged_watermarks.clone();
        let snapshots = self.slot_table.lighting_snapshots(&acks);
        let live_phase = self.derive_live_lighting_phase(&acks);
        let is_protected = self.is_active_lighting_protected();
        let is_split = self.current_bottom_row_layout() == BottomRowLayout::Split;
        let six_layer_mode =
            self.six_layer_state.installation.is_some() || self.six_layer_running.is_some();
        if six_layer_mode {
            let result = self.lighting_arbiter.recompute_feedback(
                live_phase,
                &snapshots,
                false,
                is_protected,
                false,
                is_split,
            );
            if result != mi_bridge::LightingUpdateResult::DeviceBusy {
                let running = &self.six_layer_running;
                let _ = self
                    .lighting_arbiter
                    .sync_six_layer_hardware(|digest, entries| {
                        Ok(running
                            .as_ref()
                            .and_then(|r| r.queue_session_lighting(digest, entries)))
                    });
            }
            return;
        }
        let verified_session = self
            .device_session
            .as_ref()
            .filter(|_| self.hardware_rpc_verified)
            .filter(|s| !s.is_disconnected())
            .cloned();
        let connected_device_id = verified_session
            .as_ref()
            .and_then(|_| self.connected_candidate.as_ref().map(|c| c.path.clone()));
        let runtime_for_lighting = Arc::clone(&self.tokio_runtime);
        let _ = self.lighting_arbiter.update_feedback(
            live_phase,
            &snapshots,
            false,
            is_protected,
            false,
            is_split,
            connected_device_id.as_deref(),
            |payload_str| {
                let Some(session) = verified_session.clone() else {
                    return Err("no_verified_device_session".to_string());
                };
                let (preview, entries) = parse_hardware_lighting_payload(payload_str)?;
                runtime_for_lighting.spawn(async move {
                    let _ = sync_hardware_lighting_to_session(&session, &preview, &entries).await;
                });
                Ok("{\"synced\":true}".to_string())
            },
        );
    }

    fn derive_live_lighting_phase(&self, acks: &BTreeMap<String, String>) -> FourAppAgentPhase {
        self.projection_inputs().derive_live_lighting_phase(acks)
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
        let state = self.document_store.routing_state();
        self.auto_follow_enabled = state
            .get("auto_follow_enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        self.manual_override = state
            .get("manual_override")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
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
        self.app_profile_links.clear();
        if let Some(map) = state.get("app_profile_links").and_then(|v| v.as_object()) {
            for (k, v) in map {
                if let Some(pid) = v.as_str() {
                    self.app_profile_links.insert(k.clone(), pid.to_string());
                }
            }
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
        if self
            .selected_profile_id
            .as_ref()
            .is_some_and(|pid| rows.iter().any(|r| &r.profile_id == pid))
        {
            return;
        }
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
    }

    fn commit_local_update(&mut self) {
        self.normalize_view_state();
        let mut reused_keycaps = false;
        let mut reused_categories = false;
        let mut reused_actions = false;
        let mut reusable = ReusableInspectorCatalogs {
            keycaps: None,
            categories: None,
            actions: None,
        };
        if self.projection_generation > 0 {
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
                    && self.action_category_filter == "全部"
                    && prev.action_category_filter == "全部"
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
            auto_follow_enabled: self.auto_follow_enabled,
            banner_flash_count: self.banner_flash_count,
            bind_keycap_default_action_on_select: self.bind_keycap_default_action_on_select,
            board_routing: self.board_routing,
            bottom_row_layout: self.bottom_row_layout,
            busy: self.busy,
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
            isolated: self.config.isolated,
            keycap_overrides: &self.keycap_overrides,
            keycap_search_query: &self.keycap_search_query,
            last_cursor_position: self.last_cursor_position,
            last_exported_preset: &self.last_exported_preset,
            last_switch_duration: self.last_switch_duration,
            acknowledged_watermarks: &self.lighting_arbiter.acknowledged_watermarks,
            full_keypad_feedback: &self.lighting_arbiter.full_keypad_feedback,
            dual_zone_feedback: &self.lighting_arbiter.dual_zone_feedback,
            lighting_popover: &self.lighting_popover,
            manual_override: self.manual_override,
            navigation_history: &self.navigation_history,
            open_drawers: &self.open_drawers,
            overlay_controller: &self.overlay_controller,
            pending_preset_export_scope: self.pending_preset_export_scope,
            pending_preset_import: &self.pending_preset_import,
            pinned_favorites: &self.pinned_favorites,
            preset_transfer_error: &self.preset_transfer_error,
            ptt_active: self.ptt_controller.is_active,
            radial_readiness: &self.radial_readiness,
            radial_slot_configs: &self.radial_slot_configs,
            radial_state: &self.radial_state,
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
            six_layer_busy: self.six_layer_job.is_some(),
            six_layer_running_present: self.six_layer_running.is_some(),
            six_layer_state: &self.six_layer_state,
            six_layer_view: &self.six_layer_view,
            slot_role_overrides: &self.slot_role_overrides,
            slot_table: &self.slot_table,
            status_menu_open: self.status_menu_open,
            status_message: &self.status_message,
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
                foreground_priority_adapter: running.foreground_priority_adapter(),
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
            auto_follow_enabled: menu.auto_follow_enabled,
            manual_override: menu.manual_override,
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

    pub fn set_hosts(
        &mut self,
        panel_host: Arc<dyn PassivePanelHost>,
        status_item_host: Arc<dyn StatusItemHost>,
    ) {
        self.panel_host = panel_host;
        self.status_item_host = status_item_host;
        self.last_status_item_snapshot = None;
        self.last_hud_panel = None;
        self.last_radial_panel = None;
        self.commit_runtime_update();
    }

    pub fn update_screen_context(
        &mut self,
        screen: ScreenGeometry,
        cursor: ScreenPoint,
        main_window_active: bool,
        now: Instant,
    ) {
        self.update_now = now;
        self.screen_geometry = screen;
        self.last_cursor_position = cursor;
        self.overlay_controller
            .set_main_workbench_window_active(main_window_active);
        self.overlay_controller.expire_notice_if_due(now);
        self.commit_runtime_update();
    }

    pub fn set_main_workbench_window_active(&mut self, active: bool) {
        let previous_visibility = self.overlay_controller.visibility().clone();
        self.overlay_controller
            .set_main_workbench_window_active(active);
        if self.overlay_controller.visibility() != &previous_visibility {
            self.commit_runtime_update();
        } else {
            self.commit_local_update();
        }
    }

    pub fn expire_overlay_if_due(&mut self, now: Instant) -> bool {
        self.update_now = now;
        let expired = self.overlay_controller.expire_notice_if_due(now);
        if expired {
            self.commit_runtime_update();
        }
        expired
    }

    pub fn set_radial_menu_enabled(&mut self, enabled: bool) {
        self.radial_state.set_enabled(enabled);
        self.commit_runtime_update();
    }

    pub fn set_radial_confirm_mode(&mut self, mode: RadialConfirmMode) {
        self.radial_state.set_confirm_mode(mode);
        self.commit_runtime_update();
    }

    pub fn set_radial_input_readiness(&mut self, readiness: RadialInputReadiness) {
        self.radial_readiness = readiness;
        if readiness != RadialInputReadiness::Ready {
            self.radial_state.cancel();
        }
        self.commit_runtime_update();
    }

    pub fn apply_radial_preset(&mut self, preset: RadialPresetKind) {
        self.radial_slot_configs = RadialSlotConfig::for_preset(preset);
        self.radial_state.cancel();
        self.commit_runtime_update();
    }

    pub fn set_radial_slot_configs(&mut self, configs: [RadialSlotConfig; RADIAL_SECTOR_COUNT]) {
        self.radial_slot_configs = configs;
        self.radial_state.cancel();
        self.commit_runtime_update();
    }

    pub fn set_radial_slot_config(&mut self, sector: SectorIndex, config: RadialSlotConfig) {
        self.radial_slot_configs[sector.as_usize()] = config;
        self.radial_state.cancel();
        self.commit_runtime_update();
    }

    pub fn resolve_current_radial_slots(&self) -> [RadialSlot; RADIAL_SECTOR_COUNT] {
        self.projection_inputs().resolve_current_radial_slots()
    }

    fn execute_radial_intent(
        &mut self,
        session: &RadialMenuSession,
        intent: &RadialIntent,
    ) -> HostActionReceipt {
        let executor = Arc::clone(&self.live_executor);
        let isolated = self.config.isolated;
        self.execute_radial_intent_using(session, intent, move |profile, action| {
            if isolated {
                mi_macos::SimulatedHostActionExecutor::new().execute(profile, "radial-menu", action)
            } else {
                executor.execute(profile, "radial-menu", action)
            }
        })
    }

    fn execute_radial_intent_using(
        &mut self,
        session: &RadialMenuSession,
        intent: &RadialIntent,
        execute: impl FnOnce(&str, &ResolvedHostAction) -> HostActionReceipt,
    ) -> HostActionReceipt {
        let profile_id = session.route.profile_id.clone().unwrap_or_default();
        let local_receipt = |outcome: &str, detail: &str| HostActionReceipt {
            action_type: "cheat_sheet".into(),
            binding_id: "radial-menu".into(),
            detail: detail.into(),
            outcome: outcome.into(),
            profile_id: profile_id.clone(),
            target: "micro-input".into(),
            text_byte_len: None,
            timestamp_ms: 0,
        };
        let previous_history = self.navigation_history.clone();
        let action = match intent {
            RadialIntent::CheatSheet => {
                let sheet = self.build_cheat_sheet_projection();
                self.overlay_controller.show_cheat_sheet(
                    sheet,
                    &self.screen_geometry,
                    self.last_cursor_position,
                );
                let receipt = local_receipt("executed", "已显示当前宿主配置的按键速查");
                self.record_host_action_receipt(receipt.clone());
                return receipt;
            }
            RadialIntent::OpenApp { bundle_id } => ResolvedHostAction::OpenApp {
                bundle_id: bundle_id.clone(),
            },
            RadialIntent::Prompt {
                text,
                adapter,
                submit,
            } => ResolvedHostAction::AgentCommand {
                adapter: adapter
                    .clone()
                    .or_else(|| self.active_desktop_agent_adapter())
                    .unwrap_or_else(|| "host".into()),
                command: format!(
                    "{}:{text}",
                    if *submit { "submit-prompt" } else { "prompt" }
                ),
            },
            RadialIntent::Submit { adapter } | RadialIntent::Stop { adapter } => {
                ResolvedHostAction::AgentCommand {
                    adapter: adapter
                        .clone()
                        .or_else(|| self.active_desktop_agent_adapter())
                        .unwrap_or_else(|| "host".into()),
                    command: if matches!(intent, RadialIntent::Submit { .. }) {
                        "submit"
                    } else {
                        "deny"
                    }
                    .into(),
                }
            }
            RadialIntent::NavigateBack | RadialIntent::NavigateForward => {
                let applications = &self.applications;
                let available = |place: &NavigationPlace| {
                    applications
                        .get(bundle_id_for_place(place))
                        .copied()
                        .unwrap_or(false)
                };
                let entry = if matches!(intent, RadialIntent::NavigateBack) {
                    self.navigation_history.back(available)
                } else {
                    self.navigation_history.forward(available)
                };
                let Some(entry) = entry else {
                    let mut receipt = local_receipt("blocked", "没有可打开的历史目标");
                    receipt.action_type = "open_app".into();
                    receipt.target = "navigation".into();
                    self.record_host_action_receipt(receipt.clone());
                    return receipt;
                };
                ResolvedHostAction::OpenApp {
                    bundle_id: bundle_id_for_place(&entry).into(),
                }
            }
        };
        let mut receipt = execute(&profile_id, &action);
        if matches!(intent, RadialIntent::Prompt { .. }) {
            receipt.action_type = "paste_text".into();
        }
        if !receipt.succeeded() {
            self.navigation_history = previous_history;
        }
        self.record_host_action_receipt(receipt.clone());
        receipt
    }

    pub fn consume_radial_input(
        &mut self,
        event: RadialInputEvent,
        now: Instant,
    ) -> RadialConsumeOutcome {
        self.update_now = now;
        if self.system_lifecycle != SystemLifecycle::Awake || !self.radial_state.enabled() {
            return RadialConsumeOutcome {
                consumed: false,
                confirmed_intent: None,
                dispatched_receipt: None,
            };
        }

        if let RadialInputEvent::EscapeKeyDown { key_code } = event {
            if key_code == ESCAPE_KEY_CODE && self.radial_state.session().is_some() {
                self.radial_state.cancel();
                self.commit_runtime_update();
                return RadialConsumeOutcome {
                    consumed: true,
                    confirmed_intent: None,
                    dispatched_receipt: None,
                };
            }
            return RadialConsumeOutcome {
                consumed: false,
                confirmed_intent: None,
                dispatched_receipt: None,
            };
        }

        let is_position = matches!(event, RadialInputEvent::Position { .. });
        let is_enc_clk = matches!(
            &event,
            RadialInputEvent::KeyPress { binding_id, .. } if binding_id == "ENC_CLK"
        );
        let captured_confirm = self.radial_state.captures_confirm() && is_enc_clk;

        let gates_ok = self.system_lifecycle == SystemLifecycle::Awake
            && self.document_store.host_execution_enabled()
            && !self.ptt_controller.is_active
            && self.ptt_controller.cleanup_confirmed
            && !self.live_executor.has_unconfirmed_apple_fn_release()
            && self.radial_readiness == RadialInputReadiness::Ready
            && self.resolved_active_profile_id().is_some();

        if !gates_ok {
            self.radial_state.cancel();
            self.commit_runtime_update();
            return RadialConsumeOutcome {
                consumed: is_position || captured_confirm,
                confirmed_intent: None,
                dispatched_receipt: None,
            };
        }

        let current_route = self.current_radial_route_stamp();
        let event_session_id = match &event {
            RadialInputEvent::Position { session_id, .. }
            | RadialInputEvent::KeyPress { session_id, .. } => *session_id,
            RadialInputEvent::EscapeKeyDown { .. } => 0,
        };

        if let Some(active_session) = self.radial_state.session() {
            if active_session.input_session != event_session_id
                || active_session.route != current_route
            {
                self.radial_state.cancel();
                self.commit_runtime_update();
                if captured_confirm {
                    return RadialConsumeOutcome {
                        consumed: true,
                        confirmed_intent: None,
                        dispatched_receipt: None,
                    };
                }
            }
        }

        match event {
            RadialInputEvent::Position {
                session_id,
                sample,
                admitted,
                ..
            } => {
                if !admitted {
                    self.radial_state.cancel();
                    self.commit_runtime_update();
                    return RadialConsumeOutcome {
                        consumed: true,
                        confirmed_intent: None,
                        dispatched_receipt: None,
                    };
                }
                let slots = self.resolve_current_radial_slots();
                let anchor_frame = PassivePanelFrameSpec::for_radial(
                    self.last_cursor_position,
                    &self.screen_geometry,
                )
                .frame;
                let confirmed = self.radial_state.update(sample, now, || {
                    Some(RadialMenuSession {
                        input_session: session_id,
                        route: current_route,
                        slots,
                        selected: None,
                        anchor_frame,
                    })
                });
                if let Some((confirmed_session, intent)) = confirmed {
                    let receipt = self.execute_radial_intent(&confirmed_session, &intent);
                    self.commit_runtime_update();
                    return RadialConsumeOutcome {
                        consumed: true,
                        confirmed_intent: Some(intent),
                        dispatched_receipt: Some(receipt),
                    };
                }
                self.commit_runtime_update();
                RadialConsumeOutcome {
                    consumed: true,
                    confirmed_intent: None,
                    dispatched_receipt: None,
                }
            }
            RadialInputEvent::KeyPress {
                binding_id,
                admitted,
                ..
            } => {
                if binding_id == "ENC_CLK" && self.radial_state.captures_confirm() {
                    if admitted {
                        if let Some((confirmed_session, intent)) = self.radial_state.confirm() {
                            let receipt = self.execute_radial_intent(&confirmed_session, &intent);
                            self.commit_runtime_update();
                            return RadialConsumeOutcome {
                                consumed: true,
                                confirmed_intent: Some(intent),
                                dispatched_receipt: Some(receipt),
                            };
                        }
                    }
                    self.commit_runtime_update();
                    return RadialConsumeOutcome {
                        consumed: true,
                        confirmed_intent: None,
                        dispatched_receipt: None,
                    };
                }
                if self.radial_state.session().is_some() {
                    self.radial_state.cancel();
                    self.commit_runtime_update();
                }
                RadialConsumeOutcome {
                    consumed: false,
                    confirmed_intent: None,
                    dispatched_receipt: None,
                }
            }
            RadialInputEvent::EscapeKeyDown { .. } => RadialConsumeOutcome {
                consumed: false,
                confirmed_intent: None,
                dispatched_receipt: None,
            },
        }
    }

    pub fn dispatch_radial_overlay_action(
        &mut self,
        action: RadialOverlayAction,
    ) -> Result<Option<HostActionReceipt>, WorkbenchError> {
        self.require_awake()?;
        match action {
            RadialOverlayAction::CancelEscape => {
                self.radial_state.cancel();
                self.commit_runtime_update();
                Ok(None)
            }
            RadialOverlayAction::SelectSector(sector) => {
                self.radial_state
                    .select_sector_in_place(sector, self.update_now);
                self.commit_runtime_update();
                Ok(None)
            }
            RadialOverlayAction::ConfirmSelected => {
                let outcome = self
                    .radial_state
                    .confirm()
                    .map(|(session, intent)| self.execute_radial_intent(&session, &intent));
                self.commit_runtime_update();
                Ok(outcome)
            }
        }
    }

    pub fn dispatch_overlay_action(
        &mut self,
        action: HostOverlayAction,
    ) -> Result<(), WorkbenchError> {
        match action {
            HostOverlayAction::CloseCheatSheet => {
                self.overlay_controller.hide();
                self.commit_runtime_update();
                Ok(())
            }
            HostOverlayAction::ToggleCheatSheet => {
                let sheet = self.build_cheat_sheet_projection();
                self.overlay_controller.toggle_cheat_sheet(
                    sheet,
                    &self.screen_geometry,
                    self.last_cursor_position,
                );
                self.commit_runtime_update();
                Ok(())
            }
        }
    }

    pub fn dispatch_status_menu_action(
        &mut self,
        action: StatusMenuAction,
    ) -> Result<(), WorkbenchError> {
        match action {
            StatusMenuAction::ActivateFavoriteSlot(slot) => {
                let pid = self
                    .pinned_favorites
                    .slot(slot)
                    .map(str::to_string)
                    .ok_or_else(|| WorkbenchError::Routing("该常用格未绑定方案".to_string()))?;
                self.select_and_activate_profile_at(&pid, self.update_now, true)
            }
            StatusMenuAction::PinFavoriteSlot { slot, profile_id } => {
                if !self
                    .document_store
                    .draft
                    .document
                    .editable
                    .graph
                    .profiles
                    .contains_key(&profile_id)
                {
                    return Err(WorkbenchError::Routing("Profile 不存在".to_string()));
                }
                self.pinned_favorites.assign_or_swap(slot, profile_id);
                self.commit_runtime_update();
                Ok(())
            }
            StatusMenuAction::SelectProfile(profile_id) => {
                self.select_and_activate_profile_at(&profile_id, self.update_now, true)
            }
            StatusMenuAction::SetHostExecutionEnabled(enabled) => {
                self.set_host_execution_enabled(enabled)
            }
            StatusMenuAction::SetProfileLocked(locked) => self.set_profile_locked(locked, false),
            StatusMenuAction::OpenAccessibilitySettings => {
                self.open_accessibility_settings();
                Ok(())
            }
            StatusMenuAction::SetAutoFollowEnabled(enabled) => self.set_auto_follow(enabled),
            StatusMenuAction::ResumeAutoFollow => {
                if self.manual_override || self.document_store.locked_profile_id().is_some() {
                    self.apply_routing_operation(RoutingOperation::Resume)?;
                    self.manual_override = false;
                    self.auto_follow_enabled = true;
                }
                if !self.auto_follow_enabled {
                    self.apply_routing_operation(RoutingOperation::Enable { enabled: true })?;
                    self.auto_follow_enabled = true;
                }
                self.sync_six_layer_routing_context();
                self.sync_live_session_source();
                self.commit_runtime_update();
                Ok(())
            }
            StatusMenuAction::SetRadialMenuEnabled(enabled) => {
                self.set_radial_menu_enabled(enabled);
                Ok(())
            }
            StatusMenuAction::SetRadialConfirmMode(mode) => {
                self.set_radial_confirm_mode(mode);
                Ok(())
            }
            StatusMenuAction::ApplyRadialPreset(preset) => {
                self.apply_radial_preset(preset);
                Ok(())
            }
            StatusMenuAction::SetPopoverPreviewMode(mode) => {
                self.apply_routing_operation(RoutingOperation::PopoverPreviewMode { mode })?;
                self.commit_runtime_update();
                Ok(())
            }
            StatusMenuAction::ToggleCheatSheet => {
                self.dispatch_overlay_action(HostOverlayAction::ToggleCheatSheet)
            }
            StatusMenuAction::StagePresetImportBytes(bytes) => {
                let envelope = match PresetFileEnvelope::from_bytes(&bytes) {
                    Ok(env) => env,
                    Err(msg) => {
                        self.preset_transfer_error = Some(msg.clone());
                        self.pending_preset_import = None;
                        self.commit_runtime_update();
                        return Err(WorkbenchError::Preset(msg));
                    }
                };
                let parsed = match mi_model::preset::import(&envelope.raw_utf8) {
                    Ok(draft) => draft,
                    Err(msg) => {
                        self.preset_transfer_error = Some(msg.clone());
                        self.pending_preset_import = None;
                        self.commit_runtime_update();
                        return Err(WorkbenchError::Preset(msg));
                    }
                };
                let graph = &parsed.document.editable.graph;
                let name = graph
                    .profile_order
                    .first()
                    .and_then(|pid| graph.profiles.get(pid))
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| "预设方案".to_string());
                self.pending_preset_import = Some(PendingPresetImportPreview {
                    raw_text: envelope.raw_utf8,
                    name,
                    profile_count: graph.profiles.len(),
                    layer_count: graph.layers.len(),
                    action_count: graph.actions.len(),
                });
                self.preset_transfer_error = None;
                self.commit_runtime_update();
                Ok(())
            }
            StatusMenuAction::ConfirmStagedPresetImport => {
                let Some(staged) = self.pending_preset_import.take() else {
                    let msg = "没有待确认的预设导入".to_string();
                    self.preset_transfer_error = Some(msg.clone());
                    self.commit_runtime_update();
                    return Err(WorkbenchError::Preset(msg));
                };
                self.preset_transfer_error = None;
                self.apply_backup_command(BackupCommand::ImportPreset {
                    text: staged.raw_text,
                })?;
                Ok(())
            }
            StatusMenuAction::CancelStagedPresetImport => {
                self.pending_preset_import = None;
                self.preset_transfer_error = None;
                self.commit_runtime_update();
                Ok(())
            }
            StatusMenuAction::RequestPresetExport(scope) => {
                self.pending_preset_export_scope = Some(scope);
                self.preset_transfer_error = None;
                self.commit_runtime_update();
                Ok(())
            }
            StatusMenuAction::ConfirmPresetExport => {
                let Some(scope) = self.pending_preset_export_scope.take() else {
                    let msg = "没有待确认的预设导出范围".to_string();
                    self.preset_transfer_error = Some(msg.clone());
                    self.commit_runtime_update();
                    return Err(WorkbenchError::Preset(msg));
                };
                let (profile_id, layer_id) = match scope {
                    PresetExportScope::Document => (None, None),
                    PresetExportScope::Profile => (self.selected_profile_id.clone(), None),
                    PresetExportScope::Layer => (
                        self.selected_profile_id.clone(),
                        self.selected_layer_id.clone(),
                    ),
                };
                self.preset_transfer_error = None;
                self.apply_backup_command(BackupCommand::ExportPreset {
                    profile_id,
                    layer_id,
                })?;
                Ok(())
            }
            StatusMenuAction::CancelPresetExport => {
                self.pending_preset_export_scope = None;
                self.preset_transfer_error = None;
                self.commit_runtime_update();
                Ok(())
            }
            StatusMenuAction::OpenMainWorkbenchWindow => {
                self.set_main_workbench_window_active(true);
                self.status_message = "已打开主工作台窗口".to_string();
                self.commit_runtime_update();
                Ok(())
            }
        }
    }

    pub fn hud_visibility(&self) -> &HudVisibilityState {
        self.overlay_controller.visibility()
    }

    pub fn favorites_grid(&self) -> &PinnedFavoritesGrid {
        &self.pinned_favorites
    }

    pub fn radial_state(&self) -> &RadialMenuState {
        &self.radial_state
    }

    pub fn last_host_action_receipt(&self) -> Option<&HostActionReceipt> {
        self.last_host_action_receipt.as_ref()
    }

    pub fn dispatched_receipts(&self) -> &[HostActionReceipt] {
        &self.dispatched_receipts
    }

    fn record_host_action_receipt(&mut self, receipt: HostActionReceipt) {
        if let Some(audit_path) = env::var_os("MI_HOST_AUDIT_LOG") {
            let path = PathBuf::from(audit_path);
            let _ = receipt.append_to_audit_log(&path);
        }
        self.last_host_action_receipt = Some(receipt.clone());
        self.dispatched_receipts.push(receipt);
    }

    fn surface_for_binding(&self, binding_id: &str) -> Option<ControlSurfaceId> {
        let layout = self.current_bottom_row_layout();
        match binding_id {
            "AG00" => Some(ControlSurfaceId::Key0),
            "AG01" => Some(ControlSurfaceId::Key1),
            "AG02" => Some(ControlSurfaceId::Key2),
            "AG03" => Some(ControlSurfaceId::Key3),
            "AG04" => Some(ControlSurfaceId::Key4),
            "AG05" => Some(ControlSurfaceId::Key5),
            "AG06" | "ACT06" => Some(ControlSurfaceId::Key6),
            "AG07" | "ACT07" => Some(ControlSurfaceId::Key7),
            "AG08" | "ACT08" => Some(ControlSurfaceId::Key8),
            "AG09" | "ACT09" => Some(ControlSurfaceId::Key9),
            "AG10" | "ACT10" => Some(ControlSurfaceId::Key10),
            "AG11" | "ACT11" => Some(if layout.is_merged() {
                ControlSurfaceId::Key10
            } else {
                ControlSurfaceId::Key11
            }),
            "AG12" | "ACT12" => Some(ControlSurfaceId::Key12),
            "AG13" | "ENC_CC" => Some(ControlSurfaceId::EncoderCcw),
            "AG14" | "ENC_CW" => Some(ControlSurfaceId::EncoderCw),
            "ENC_CLK" => Some(ControlSurfaceId::EncoderPress),
            "AG15" | "AG16" | "AG17" | "AG18" => Some(ControlSurfaceId::Joystick),
            _ => None,
        }
    }

    fn resolve_physical_action(
        &self,
        profile_id: &str,
        binding_id: &str,
        layout: BottomRowLayout,
    ) -> Option<ResolvedHostAction> {
        let source_id = self.runtime_action_profile_id(profile_id)?;
        let doc = &self.document_store.host_store().draft.document;
        if let Some(resolved) = resolve_host_action_with_layout(
            doc,
            Some(source_id),
            binding_id,
            true,
            self.command_execution_enabled,
            layout,
        ) {
            return Some(resolved);
        }
        if source_id != profile_id {
            return None;
        }
        let profile = doc.editable.graph.profiles.get(source_id)?;
        fallback_unbridged_oai_action(&profile.name, binding_id, layout)
    }

    pub fn consume_physical_input_event(
        &mut self,
        event: InputEvent,
        now: Instant,
    ) -> Option<HostActionReceipt> {
        if self.system_lifecycle != SystemLifecycle::Awake {
            return None;
        }
        self.update_now = now;
        if event.kind == InputEventKind::InputStreamGap {
            self.input_lost_session = Some(event.session_id);
            self.release_board_inputs(now);
            self.commit_runtime_update();
            return None;
        }
        if event.kind != InputEventKind::Released
            && (self.input_lost_session == Some(event.session_id)
                || self.hardware_event_tx.source_closed(event.session_id))
        {
            return None;
        }
        let profile_id = self.resolved_active_profile_id();
        if event.kind != InputEventKind::Released
            && profile_id
                .as_ref()
                .is_none_or(|id| self.runtime_action_profile_id(id).is_none())
        {
            return None;
        }
        if let Some(pos) = event.radial_position {
            if self.radial_state.enabled() {
                let outcome = self.consume_radial_input(
                    RadialInputEvent::Position {
                        session_id: event.session_id,
                        ingress_seq: event.ingress_sequence,
                        sample: RadialPositionSample {
                            angle_turns: pos.angle_turns,
                            distance: pos.distance,
                        },
                        admitted: true,
                    },
                    now,
                );
                if outcome.consumed && event.kind == InputEventKind::UnknownNotification {
                    return outcome.dispatched_receipt;
                }
                if let Some(receipt) = outcome.dispatched_receipt {
                    return Some(receipt);
                }
            }
        }

        let Some(binding_id) = event.binding_id.clone() else {
            return None;
        };
        if event.kind != InputEventKind::Released
            && (!event.action_eligible
                || self.input_lost_session == Some(event.session_id)
                || self.live_executor.has_unconfirmed_apple_fn_release()
                || !self.ptt_controller.cleanup_confirmed)
        {
            return None;
        }

        let profile_id = profile_id.unwrap_or_default();

        match event.kind {
            InputEventKind::Released => {
                if binding_id == "ENC_CLK" {
                    self.dial_pressed = false;
                    self.dial_turned_while_pressed = false;
                }
                let input = HeldInputId {
                    session_id: event.session_id,
                    binding_id: binding_id.clone(),
                };
                if let Some(receipt) = self.live_executor.handle_apple_fn_release(&input) {
                    self.status_message = format!("已释放 {} · {}", binding_id, receipt.detail);
                    self.record_host_action_receipt(receipt.clone());
                    self.commit_runtime_update();
                    return Some(receipt);
                }
                if self
                    .held_dictation
                    .as_ref()
                    .is_some_and(|held| held.input == input)
                {
                    let stopped = if self.config.isolated {
                        Ok(())
                    } else {
                        self.live_executor.stop_owned_dictations()
                    };
                    return self.finish_dictation(stopped);
                }
                None
            }
            InputEventKind::Pressed => {
                if self.radial_state.enabled() {
                    let radial_outcome = self.consume_radial_input(
                        RadialInputEvent::KeyPress {
                            session_id: event.session_id,
                            ingress_seq: event.ingress_sequence,
                            binding_id: binding_id.clone(),
                            admitted: true,
                        },
                        now,
                    );
                    if radial_outcome.consumed {
                        return radial_outcome.dispatched_receipt;
                    }
                }

                if binding_id == "ENC_CLK" {
                    self.dial_pressed = true;
                    self.dial_turned_while_pressed = false;
                }

                let inspector_search_active = !self.action_search_query.trim().is_empty()
                    || !self.keycap_search_query.trim().is_empty()
                    || self.shortcut_recorder.is_recording;
                if !inspector_search_active {
                    if let Some(surface) = self.surface_for_binding(&binding_id) {
                        self.selected_surface = surface;
                        let slot = self.effective_slot_for_surface(surface);
                        self.selected_binding_slot = Some(slot);
                    }
                    match binding_id.as_str() {
                        "AG15" => self.selected_joystick_direction = JoystickDirection8::North,
                        "AG16" => self.selected_joystick_direction = JoystickDirection8::West,
                        "AG17" => self.selected_joystick_direction = JoystickDirection8::South,
                        "AG18" => self.selected_joystick_direction = JoystickDirection8::East,
                        _ => {}
                    }
                }

                if !self.document_store.host_execution_enabled() {
                    self.status_message = format!("已按下 {binding_id}（快捷键映射已暂停）");
                    self.commit_runtime_update();
                    return None;
                }

                let layout = self.current_bottom_row_layout();
                let Some(mut action) =
                    self.resolve_physical_action(&profile_id, &binding_id, layout)
                else {
                    self.status_message = format!("已按下 {binding_id}（当前槽位未绑定动作）");
                    self.commit_runtime_update();
                    return None;
                };

                if matches!(binding_id.as_str(), "AG13" | "AG14") && self.dial_pressed {
                    self.dial_turned_while_pressed = true;
                    if let ResolvedHostAction::AgentCommand { command, .. } = &mut action {
                        if command == "scroll-up" {
                            *command = "scroll-up-fast".to_string();
                        } else if command == "scroll-down" {
                            *command = "scroll-down-fast".to_string();
                        }
                    }
                }

                if let ResolvedHostAction::Shortcut {
                    key_code,
                    modifiers,
                    target_bundle_id,
                    ..
                } = &action
                {
                    if *key_code == APPLE_FN_KEY_CODE && modifiers.is_empty() {
                        let target = target_bundle_id.as_deref().unwrap_or("frontmost-editor");
                        if let Some(receipt) = self.live_executor.handle_apple_fn_press(
                            &HeldInputId {
                                session_id: event.session_id,
                                binding_id: binding_id.clone(),
                            },
                            &profile_id,
                            target,
                        ) {
                            self.status_message =
                                format!("已触发 {} · {}", binding_id, receipt.detail);
                            self.record_host_action_receipt(receipt.clone());
                            self.commit_runtime_update();
                            return Some(receipt);
                        }
                        return None;
                    }
                }

                if let ResolvedHostAction::AgentCommand { adapter, command } = &mut action {
                    if adapter == "host" {
                        *adapter = self
                            .active_desktop_agent_adapter()
                            .unwrap_or_else(|| "antigravity".to_string());
                    }
                    if let Some(command) =
                        NavigationCommand::from_action(&ResolvedHostAction::AgentCommand {
                            adapter: adapter.clone(),
                            command: command.clone(),
                        })
                    {
                        self.request_navigation_command(
                            command,
                            None,
                            Some(NavigationInputLease::Vendor(event.session_id)),
                            profile_id,
                            binding_id,
                        );
                        return None;
                    }
                    if command == "push-to-talk" {
                        let resolved_adapter = adapter.clone();
                        if self.held_dictation.is_some() {
                            return None;
                        }
                        let started = if self.config.isolated {
                            Ok(())
                        } else {
                            self.live_executor.start_owned_dictation(&resolved_adapter)
                        };
                        let uncertain = started
                            .as_ref()
                            .is_err_and(|error| error.starts_with("unconfirmed:"));
                        self.ptt_controller.is_active = started.is_ok();
                        self.ptt_controller.cleanup_confirmed = !uncertain;
                        if started.is_ok() || uncertain {
                            self.held_dictation = Some(HeldDictation {
                                input: HeldInputId {
                                    session_id: event.session_id,
                                    binding_id: binding_id.clone(),
                                },
                                profile_id: profile_id.clone(),
                                adapter: resolved_adapter.clone(),
                            });
                        }
                        let (outcome, detail) = match started {
                            Ok(()) => (
                                if self.config.isolated {
                                    "simulated"
                                } else {
                                    "executed"
                                },
                                format!("{resolved_adapter}/push-to-talk"),
                            ),
                            Err(error) => {
                                (if uncertain { "unconfirmed" } else { "blocked" }, error)
                            }
                        };
                        let receipt = HostActionReceipt {
                            action_type: "agent_command".into(),
                            binding_id: binding_id.clone(),
                            detail: detail.clone(),
                            outcome: outcome.into(),
                            profile_id,
                            target: resolved_adapter,
                            text_byte_len: None,
                            timestamp_ms: 0,
                        };
                        self.status_message = if self.ptt_controller.is_active {
                            format!("正在按住语音 ({binding_id})…")
                        } else {
                            format!("听写未启动 · {detail}")
                        };
                        self.record_host_action_receipt(receipt.clone());
                        self.commit_runtime_update();
                        return Some(receipt);
                    }
                }

                let receipt = if self.config.isolated {
                    mi_macos::SimulatedHostActionExecutor::new().execute(
                        &profile_id,
                        &binding_id,
                        &action,
                    )
                } else {
                    self.live_executor
                        .execute(&profile_id, &binding_id, &action)
                };

                if let ResolvedHostAction::OpenApp { bundle_id } = &action {
                    if receipt.succeeded() {
                        self.navigation_history
                            .visit(NavigationPlace::app(bundle_id.clone()));
                    }
                }

                self.status_message = if receipt.succeeded() {
                    format!("已触发 {} · {}", binding_id, receipt.detail)
                } else {
                    format!("未执行 {} · {}", binding_id, receipt.detail)
                };
                self.record_host_action_receipt(receipt.clone());
                self.commit_runtime_update();
                Some(receipt)
            }
            _ => None,
        }
    }

    fn sync_modal_for_layer_protection(&mut self) {
        if self.is_selected_layer_protected() {
            if self
                .active_modal
                .as_ref()
                .is_some_and(EditorModalProjection::is_mutating_layer_editor)
            {
                self.active_modal = None;
            }
            self.shortcut_recorder.is_recording = false;
            self.lighting_preview_active = false;
            self.lighting_preview_remaining_secs = 0;
            self.lighting_popover.manual_preview_phase = None;
            self.lighting_arbiter.manual_preview_phase = None;
        }
    }

    fn current_selected_slot(&self) -> u32 {
        self.projection_inputs().current_selected_slot()
    }

    pub fn dispatch_inspector_action(
        &mut self,
        action: InspectorAction,
    ) -> Result<(), WorkbenchError> {
        match action {
            InspectorAction::UnlockProtectedLayer => {
                self.dispatch_canvas_action(CanvasAction::UnlockBySwitchingOrCreatingCustomLayer)
            }
            InspectorAction::SelectSlot(slot) => {
                self.dispatch_canvas_action(CanvasAction::SelectLogicalSlot(slot))
            }
            InspectorAction::SelectJoystickDirection(dir) => {
                self.dispatch_canvas_action(CanvasAction::SelectJoystickDirection(dir))
            }
            InspectorAction::SetKeycapSearchQuery(query) => {
                self.keycap_search_query = query;
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::SetBindKeycapDefaultAction(bind) => {
                self.bind_keycap_default_action_on_select = bind;
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::ToggleDrawer(drawer) => {
                if let Some(pos) = self.open_drawers.iter().position(|d| *d == drawer) {
                    self.open_drawers.remove(pos);
                } else {
                    self.open_drawers.push(drawer);
                }
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::SetActionSearchQuery(query) => {
                self.action_search_query = query;
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::SetActionCategoryFilter(category) => {
                self.action_category_filter = category;
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::SetFavoritesOnly(fav_only) => {
                self.favorites_only = fav_only;
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::ToggleActionFavorite {
                action_id,
                favorite,
            } => {
                if favorite {
                    self.favorite_action_ids.insert(action_id);
                } else {
                    self.favorite_action_ids.remove(&action_id);
                }
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::SetCommandExecutionEnabled(enabled) => {
                self.command_execution_enabled = enabled;
                if let Some(running) = &self.six_layer_running {
                    running.set_execution_policy(
                        self.document_store.host_execution_enabled(),
                        enabled,
                        self.update_now,
                    );
                }
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::CancelShortcutRecording => {
                self.shortcut_recorder.is_recording = false;
                self.shortcut_recorder.validation_error = None;
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::CreateCustomLayer { name } => {
                let pid = self
                    .selected_profile_id
                    .clone()
                    .ok_or_else(|| WorkbenchError::Storage("未选择方案".to_string()))?;
                let mut created_lid = String::new();
                self.document_store
                    .update(|draft| {
                        created_lid = draft.create_layer(&pid, &name)?;
                        Ok(())
                    })
                    .map_err(WorkbenchError::Storage)?;
                self.selected_layer_id = Some(created_lid);
                self.normalize_surface_for_current_layer();
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::OpenEditor(editor_action) => {
                self.dispatch_editor_action(editor_action)
            }
            InspectorAction::ApplyKnobJoystickPreset(preset) => {
                self.dispatch_canvas_action(CanvasAction::ApplyKnobJoystickPreset(preset))
            }
            InspectorAction::SetBottomRowLayout(layout) => {
                self.dispatch_canvas_action(CanvasAction::SetBottomRowLayout(layout))
            }
            InspectorAction::MergeBottomRowAcrossLayers => {
                self.dispatch_canvas_action(CanvasAction::MergeBottomRowAcrossLayers)
            }
            InspectorAction::SetSlotRole(role) => {
                self.ensure_layer_editable()?;
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                let slot = self.current_selected_slot();
                match role {
                    KeycapRole::TaskKey => {
                        if slot >= 6 {
                            return Err(WorkbenchError::Storage(
                                "仅顶部 6 键 (AG00..AG05) 支持设为会话键".to_string(),
                            ));
                        }
                        let target_id = format!("slot-open-{slot}");
                        let action = self.resolve_action_by_id(&target_id)?;
                        self.document_store
                            .update(|draft| draft.assign(&json!(pid), &json!(lid), slot, action))
                            .map_err(WorkbenchError::Storage)?;
                        self.slot_role_overrides
                            .insert((pid, slot), KeycapRole::TaskKey);
                    }
                    KeycapRole::ActionKey => {
                        self.slot_role_overrides
                            .insert((pid.clone(), slot), KeycapRole::ActionKey);
                        let doc = &self.document_store.draft.document;
                        let layer = doc.editable.graph.layers.get(&lid);
                        let is_session =
                            resolve_slot_action_info(doc, layer, slot)
                                .1
                                .is_some_and(|a| {
                                    a.id.starts_with("slot-open-")
                                        || a.id.starts_with("cockpit-slot-")
                                        || a.payload
                                            .get("command")
                                            .and_then(|v| v.as_str())
                                            .is_some_and(|c| c.starts_with("slot-open:"))
                                });
                        if is_session {
                            let keycap = self.selected_keycap(slot);
                            let fallback = keycap
                                .default_action()
                                .or_else(|| CodexKeycapId::Fast.default_action());
                            self.document_store
                                .update(|draft| {
                                    if let Some(act) = fallback {
                                        draft.assign(&json!(pid), &json!(lid), slot, act)
                                    } else {
                                        draft.unassign(&json!(pid), &json!(lid), slot)
                                    }
                                })
                                .map_err(WorkbenchError::Storage)?;
                        }
                    }
                }
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::UnassignSelectedSlot => {
                self.ensure_layer_editable()?;
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                let slot = self.current_selected_slot();
                self.document_store
                    .update(|draft| draft.unassign(&json!(pid), &json!(lid), slot))
                    .map_err(WorkbenchError::Storage)?;
                self.slot_role_overrides
                    .insert((pid, slot), KeycapRole::ActionKey);
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::ApplyKeycapIcon {
                keycap,
                bind_default_action,
            } => {
                self.ensure_layer_editable()?;
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                let slot = self.current_selected_slot();
                self.keycap_overrides.insert((pid.clone(), slot), keycap);
                if bind_default_action {
                    self.slot_role_overrides
                        .insert((pid.clone(), slot), KeycapRole::ActionKey);
                    if let Some(action) = keycap.default_action() {
                        self.document_store
                            .update(|draft| draft.assign(&json!(pid), &json!(lid), slot, action))
                            .map_err(WorkbenchError::Storage)?;
                    } else if keycap == CodexKeycapId::Blank {
                        self.document_store
                            .update(|draft| draft.unassign(&json!(pid), &json!(lid), slot))
                            .map_err(WorkbenchError::Storage)?;
                    }
                }
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::BindAppleFnKey => {
                self.ensure_layer_editable()?;
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                let slot = self.current_selected_slot();
                let action = self.resolve_action_by_id("apple-fn-key")?;
                self.document_store
                    .update(|draft| draft.assign(&json!(pid), &json!(lid), slot, action))
                    .map_err(WorkbenchError::Storage)?;
                self.keycap_overrides
                    .insert((pid.clone(), slot), CodexKeycapId::Mic);
                self.slot_role_overrides
                    .insert((pid, slot), KeycapRole::ActionKey);
                self.shortcut_recorder.is_recording = false;
                self.shortcut_recorder.validation_error = None;
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::StartShortcutRecording => {
                self.ensure_layer_editable()?;
                self.shortcut_recorder = ShortcutRecorderState {
                    is_recording: true,
                    key: String::new(),
                    modifiers: Vec::new(),
                    target: "frontmost-editor".to_string(),
                    preview_badge: "请按下快捷键…".to_string(),
                    validation_error: None,
                };
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::CaptureShortcutChord { key, mut modifiers } => {
                self.ensure_layer_editable()?;
                modifiers.sort();
                modifiers.dedup();
                let mut badge = String::new();
                for m in &modifiers {
                    badge.push_str(m.symbol());
                }
                badge.push_str(&key.trim().to_uppercase());
                self.shortcut_recorder.key = key.trim().to_string();
                self.shortcut_recorder.modifiers = modifiers;
                self.shortcut_recorder.preview_badge = badge;
                self.shortcut_recorder.validation_error = None;
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::SetShortcutTarget(target) => {
                self.ensure_layer_editable()?;
                self.shortcut_recorder.target = target;
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::CommitRecordedShortcut => {
                self.ensure_layer_editable()?;
                let raw_key = self.shortcut_recorder.key.trim().to_lowercase();
                if raw_key.is_empty() {
                    let msg = "请先录入快捷键按键".to_string();
                    self.shortcut_recorder.validation_error = Some(msg.clone());
                    self.commit_runtime_update();
                    return Err(WorkbenchError::Storage(msg));
                }
                if self.shortcut_recorder.modifiers.is_empty()
                    && matches!(raw_key.as_str(), "fn" | "globe" | "0x3f" | "apple_fn")
                {
                    return self.dispatch_inspector_action(InspectorAction::BindAppleFnKey);
                }
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                let slot = self.current_selected_slot();
                let mods: Vec<String> = self
                    .shortcut_recorder
                    .modifiers
                    .iter()
                    .map(|m| m.as_draft_str().to_string())
                    .collect();
                let target = if self.shortcut_recorder.target.trim().is_empty() {
                    "frontmost-editor".to_string()
                } else {
                    self.shortcut_recorder.target.trim().to_string()
                };
                let badge = if self.shortcut_recorder.preview_badge.is_empty() {
                    raw_key.to_uppercase()
                } else {
                    self.shortcut_recorder.preview_badge.clone()
                };
                let action = Action {
                    id: format!("custom-shortcut-{slot}"),
                    name: format!("快捷键 {badge}"),
                    category: "快捷键".to_string(),
                    detail: format!("自定义录制快捷键 · 目标 {target}"),
                    payload: json!({
                        "type": "shortcut",
                        "key": raw_key,
                        "modifiers": mods,
                        "target": target,
                    }),
                };
                action.validate().map_err(WorkbenchError::Storage)?;
                self.document_store
                    .update(|draft| draft.assign(&json!(pid), &json!(lid), slot, action))
                    .map_err(WorkbenchError::Storage)?;
                self.slot_role_overrides
                    .insert((pid, slot), KeycapRole::ActionKey);
                self.shortcut_recorder.is_recording = false;
                self.shortcut_recorder.validation_error = None;
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::AssignCatalogAction(action_id)
            | InspectorAction::BindExistingAction(action_id) => {
                self.ensure_layer_editable()?;
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                let slot = self.current_selected_slot();
                let action = self.resolve_action_by_id(&action_id)?;
                self.document_store
                    .update(|draft| draft.assign(&json!(pid), &json!(lid), slot, action))
                    .map_err(WorkbenchError::Storage)?;
                if action_id.starts_with("slot-open-") && slot < 6 {
                    self.slot_role_overrides
                        .insert((pid, slot), KeycapRole::TaskKey);
                } else {
                    self.slot_role_overrides
                        .insert((pid, slot), KeycapRole::ActionKey);
                }
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::AssignCustomText {
                name,
                text,
                submit_on_paste,
            } => {
                self.ensure_layer_editable()?;
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                let slot = self.current_selected_slot();
                let action = if submit_on_paste {
                    Action {
                        id: format!("custom-prompt-submit-{slot}"),
                        name,
                        category: "提交控制".to_string(),
                        detail: "显式配置下先停听写、聚焦目标输入区、暂存剪贴板粘贴后再回车"
                            .to_string(),
                        payload: json!({
                            "type": "agent_command",
                            "adapter": "host",
                            "command": format!("submit-prompt:{text}"),
                        }),
                    }
                } else {
                    Action {
                        id: format!("custom-text-{slot}"),
                        name,
                        category: "文本".to_string(),
                        detail: "默认仅粘贴不回车（no_newline） · 自动暂存恢复剪贴板".to_string(),
                        payload: json!({
                            "type": "text",
                            "text": text,
                        }),
                    }
                };
                action.validate().map_err(WorkbenchError::Storage)?;
                self.document_store
                    .update(|draft| draft.assign(&json!(pid), &json!(lid), slot, action))
                    .map_err(WorkbenchError::Storage)?;
                self.slot_role_overrides
                    .insert((pid, slot), KeycapRole::ActionKey);
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::RenameSelectedLayer { name } => {
                self.ensure_layer_editable()?;
                let (_, lid) = self.require_selected_profile_and_layer()?;
                self.document_store
                    .update(|draft| draft.rename_layer(&lid, &name))
                    .map_err(WorkbenchError::Storage)?;
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::DuplicateSelectedLayer { name } => {
                self.ensure_layer_editable()?;
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                let mut created = String::new();
                self.document_store
                    .update(|draft| {
                        created = draft.duplicate_layer(&pid, &lid, &name)?;
                        Ok(())
                    })
                    .map_err(WorkbenchError::Storage)?;
                self.selected_layer_id = Some(created);
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::DeleteSelectedLayer => {
                self.ensure_layer_editable()?;
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                self.document_store
                    .update(|draft| draft.delete_layer(&pid, &lid))
                    .map_err(WorkbenchError::Storage)?;
                self.selected_layer_id = self
                    .document_store
                    .draft
                    .document
                    .editable
                    .graph
                    .profiles
                    .get(&pid)
                    .and_then(|p| p.layer_order.first().cloned());
                self.commit_runtime_update();
                Ok(())
            }
        }
    }

    pub fn dispatch_editor_action(&mut self, action: EditorAction) -> Result<(), WorkbenchError> {
        if matches!(
            action,
            EditorAction::OpenReconnectComparison { .. }
                | EditorAction::ComparisonConfirmIdentity { .. }
                | EditorAction::ComparisonResolve { .. }
                | EditorAction::OpenApplyPreparation { .. }
                | EditorAction::ApplyPrepareBackup { .. }
                | EditorAction::ApplyTakePreparedCandidate { .. }
                | EditorAction::LightingBeginPreview
        ) {
            self.require_awake()?;
        }
        match action {
            EditorAction::CloseModal => {
                self.lighting_preview_active = false;
                self.lighting_preview_remaining_secs = 0;
                self.active_modal = None;
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::ApplyCancelPreview => {
                self.document_store.cancel_apply_preview();
                self.active_modal = None;
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::OpenReconnectComparison {
                device_config,
                baseline,
            } => {
                let cmp = self
                    .document_store
                    .compare(&device_config, baseline)
                    .map_err(WorkbenchError::Storage)?;
                self.active_modal = Some(EditorModalProjection::ReconnectComparison(
                    build_comparison_modal_projection(&cmp),
                ));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::ComparisonConfirmIdentity { current_snapshot } => {
                let token = match &self.active_modal {
                    Some(EditorModalProjection::ReconnectComparison(m)) => m.token.clone(),
                    _ => return Err(WorkbenchError::Storage("未打开重连比较窗口".to_string())),
                };
                let cmp = self
                    .document_store
                    .confirm_comparison_identity(&token, &current_snapshot)
                    .map_err(WorkbenchError::Storage)?;
                self.active_modal = Some(EditorModalProjection::ReconnectComparison(
                    build_comparison_modal_projection(&cmp),
                ));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::ComparisonResolve {
                choice,
                current_snapshot,
            } => {
                let token = match &self.active_modal {
                    Some(EditorModalProjection::ReconnectComparison(m)) => m.token.clone(),
                    _ => return Err(WorkbenchError::Storage("未打开重连比较窗口".to_string())),
                };
                self.document_store
                    .resolve_comparison(&token, choice, &current_snapshot)
                    .map_err(WorkbenchError::Storage)?;
                self.active_modal = None;
                self.ensure_selected_profile_visible();
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::OpenApplyPreparation { current_snapshot } => {
                let preview = self
                    .document_store
                    .preview_apply(&current_snapshot)
                    .map_err(WorkbenchError::Storage)?;
                self.active_modal = Some(EditorModalProjection::ApplyPreparation(
                    build_apply_modal_projection(&preview),
                ));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::ApplyPrepareBackup { current_snapshot } => {
                let mut modal = match &self.active_modal {
                    Some(EditorModalProjection::ApplyPreparation(m)) => m.clone(),
                    _ => {
                        return Err(WorkbenchError::Storage(
                            "未打开配置应用预检窗口".to_string(),
                        ))
                    }
                };
                let prepared = self
                    .document_store
                    .prepare_apply_backup(&modal.token, &current_snapshot)
                    .map_err(WorkbenchError::Storage)?;
                modal.backup_prepared = true;
                modal.backup_path = Some(prepared.backup_path);
                modal.manifest_sha256 = Some(prepared.manifest_sha256);
                modal.unavailable_reason = prepared.unavailable_reason;
                modal.error_message = None;
                self.active_modal = Some(EditorModalProjection::ApplyPreparation(modal));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::ApplyTakePreparedCandidate { current_snapshot } => {
                let mut modal = match &self.active_modal {
                    Some(EditorModalProjection::ApplyPreparation(m)) => m.clone(),
                    _ => {
                        return Err(WorkbenchError::Storage(
                            "未打开配置应用预检窗口".to_string(),
                        ))
                    }
                };
                let candidate = self
                    .document_store
                    .take_prepared_candidate(&modal.token, &current_snapshot)
                    .map_err(WorkbenchError::Storage)?;
                modal.candidate_taken = true;
                modal.candidate_operation_id = Some(candidate.operation_id().to_string());
                modal.error_message = None;
                self.active_modal = Some(EditorModalProjection::ApplyPreparation(modal));
                self.commit_runtime_update();
                Ok(())
            }
            mutating_action => {
                self.ensure_layer_editable()?;
                self.dispatch_mutating_editor_action(mutating_action)
            }
        }
    }

    fn dispatch_mutating_editor_action(
        &mut self,
        action: EditorAction,
    ) -> Result<(), WorkbenchError> {
        match action {
            EditorAction::OpenMacroEditor { macro_id } => {
                let target_slot = Some(self.current_selected_slot());
                let proj = if let Some(id) = macro_id {
                    let act = self
                        .document_store
                        .draft
                        .document
                        .editable
                        .graph
                        .actions
                        .get(&id)
                        .ok_or_else(|| WorkbenchError::Storage("宏不存在".to_string()))?;
                    let ActionKind::LocalMacro { macro_sequence } =
                        serde_json::from_value(act.payload.clone())
                            .map_err(|_| WorkbenchError::Storage("不是有效的本地宏".to_string()))?
                    else {
                        return Err(WorkbenchError::Storage("不是有效的本地宏".to_string()));
                    };
                    let reference_summaries = self
                        .document_store
                        .draft
                        .macros()
                        .into_iter()
                        .find(|m| m.id == id)
                        .map(|m| {
                            m.references
                                .iter()
                                .map(|r| {
                                    format!(
                                        "{} / {} · {}",
                                        r.profile_name, r.layer_name, r.logical_control
                                    )
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    build_macro_editor_projection(
                        Some(id),
                        act.name.clone(),
                        macro_sequence.events,
                        target_slot,
                        reference_summaries,
                    )
                } else {
                    build_macro_editor_projection(
                        None,
                        "新建宏".to_string(),
                        Vec::new(),
                        target_slot,
                        Vec::new(),
                    )
                };
                self.active_modal = Some(EditorModalProjection::MacroEditor(proj));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MacroSetName(name) => {
                let mut modal = self.take_macro_modal()?;
                modal.name = name;
                self.active_modal = Some(EditorModalProjection::MacroEditor(modal));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MacroSetDraftKeyInput(key) => {
                let mut modal = self.take_macro_modal()?;
                modal.draft_key_input = key;
                self.active_modal = Some(EditorModalProjection::MacroEditor(modal));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MacroSetDraftDelayMs(ms) => {
                let mut modal = self.take_macro_modal()?;
                modal.draft_delay_ms = ms;
                self.active_modal = Some(EditorModalProjection::MacroEditor(modal));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MacroAppendEvent(event) => {
                let modal = self.take_macro_modal()?;
                let mut events: Vec<MacroEvent> =
                    modal.steps.iter().map(|s| s.event.clone()).collect();
                events.push(event);
                let next = rebuild_macro_modal_with_events(modal, events);
                self.active_modal = Some(EditorModalProjection::MacroEditor(next));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MacroAddTapKey { key, delay_ms } => {
                let modal = self.take_macro_modal()?;
                let mut events: Vec<MacroEvent> =
                    modal.steps.iter().map(|s| s.event.clone()).collect();
                let normalized_key = key.trim().to_lowercase();
                events.push(MacroEvent::Press {
                    key: normalized_key.clone(),
                });
                if delay_ms > 0 {
                    events.push(MacroEvent::Delay { ms: delay_ms });
                }
                events.push(MacroEvent::Release {
                    key: normalized_key,
                });
                let next = rebuild_macro_modal_with_events(modal, events);
                self.active_modal = Some(EditorModalProjection::MacroEditor(next));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MacroRemoveStep(index) => {
                let modal = self.take_macro_modal()?;
                let mut events: Vec<MacroEvent> =
                    modal.steps.iter().map(|s| s.event.clone()).collect();
                if index < events.len() {
                    events.remove(index);
                }
                let next = rebuild_macro_modal_with_events(modal, events);
                self.active_modal = Some(EditorModalProjection::MacroEditor(next));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MacroMoveStep { from, to } => {
                let modal = self.take_macro_modal()?;
                let mut events: Vec<MacroEvent> =
                    modal.steps.iter().map(|s| s.event.clone()).collect();
                if from < events.len() && to < events.len() && from != to {
                    let ev = events.remove(from);
                    events.insert(to, ev);
                }
                let next = rebuild_macro_modal_with_events(modal, events);
                self.active_modal = Some(EditorModalProjection::MacroEditor(next));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MacroClearSteps => {
                let modal = self.take_macro_modal()?;
                let next = rebuild_macro_modal_with_events(modal, Vec::new());
                self.active_modal = Some(EditorModalProjection::MacroEditor(next));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MacroSave {
                bind_to_selected_slot,
            } => {
                let mut modal = self.take_macro_modal()?;
                if modal.macro_id.is_none() && self.document_store.draft.macros().len() >= 30 {
                    let msg = "宏库已达 30 个上限".to_string();
                    modal.validation_error = Some(msg.clone());
                    self.active_modal = Some(EditorModalProjection::MacroEditor(modal));
                    self.commit_runtime_update();
                    return Err(WorkbenchError::Storage(msg));
                }
                let events: Vec<MacroEvent> = modal.steps.iter().map(|s| s.event.clone()).collect();
                let local_macro = LocalMacro {
                    format_version: 1,
                    events,
                };
                if let Err(err) = local_macro.validate() {
                    modal.validation_error = Some(err.clone());
                    self.active_modal = Some(EditorModalProjection::MacroEditor(modal));
                    self.commit_runtime_update();
                    return Err(WorkbenchError::Storage(err));
                }
                let action = Action {
                    id: modal
                        .macro_id
                        .clone()
                        .unwrap_or_else(|| "local-macro".to_string()),
                    name: modal.name.trim().to_string(),
                    category: "普通宏".to_string(),
                    detail: "自有本地序列 · 已验证键名可宿主执行".to_string(),
                    payload: serde_json::to_value(ActionKind::LocalMacro {
                        macro_sequence: local_macro,
                    })
                    .map_err(|e| WorkbenchError::Storage(e.to_string()))?,
                };
                if bind_to_selected_slot {
                    let (pid, lid) = self.require_selected_profile_and_layer()?;
                    let slot = modal
                        .target_slot
                        .unwrap_or_else(|| self.current_selected_slot());
                    self.document_store
                        .update(|draft| {
                            draft
                                .save_macro_for_binding(
                                    modal.macro_id.as_deref(),
                                    action,
                                    &json!(pid),
                                    &json!(lid),
                                    slot,
                                )
                                .map(|_| ())
                        })
                        .map_err(WorkbenchError::Storage)?;
                    self.slot_role_overrides
                        .insert((pid, slot), KeycapRole::ActionKey);
                } else {
                    self.document_store
                        .update(|draft| {
                            draft
                                .save_macro(modal.macro_id.as_deref(), action)
                                .map(|_| ())
                        })
                        .map_err(WorkbenchError::Storage)?;
                }
                self.active_modal = None;
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MacroBindExisting { macro_id } => {
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                let slot = self.current_selected_slot();
                self.document_store
                    .update(|draft| {
                        draft.bind_macro(&json!(pid), &json!(lid), slot, macro_id.as_deref())
                    })
                    .map_err(WorkbenchError::Storage)?;
                self.slot_role_overrides
                    .insert((pid, slot), KeycapRole::ActionKey);
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MacroDelete { macro_id } => {
                self.document_store
                    .update(|draft| draft.delete_macro(&macro_id))
                    .map_err(WorkbenchError::Storage)?;
                if matches!(
                    &self.active_modal,
                    Some(EditorModalProjection::MacroEditor(m)) if m.macro_id.as_deref() == Some(macro_id.as_str())
                ) {
                    self.active_modal = None;
                }
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::OpenLightingEditor => {
                let proj = self.build_lighting_modal_projection(None)?;
                self.active_modal = Some(EditorModalProjection::LightingEditor(proj));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::LightingSetApplyToAllLayers(all_layers) => {
                self.lighting_apply_to_all_layers = all_layers;
                let proj = self.build_lighting_modal_projection(None)?;
                self.active_modal = Some(EditorModalProjection::LightingEditor(proj));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::LightingAddDefault => {
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                let all_layers = self.lighting_apply_to_all_layers;
                self.document_store
                    .update(|draft| ensure_layer_has_base_lighting(draft, &pid, &lid, all_layers))
                    .map_err(WorkbenchError::Storage)?;
                let proj = self.build_lighting_modal_projection(None)?;
                self.active_modal = Some(EditorModalProjection::LightingEditor(proj));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::LightingEditEffect {
                zone,
                effect,
                all_layers,
            } => {
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                self.lighting_apply_to_all_layers = all_layers;
                self.document_store
                    .update(|draft| {
                        ensure_layer_has_base_lighting(draft, &pid, &lid, all_layers)?;
                        draft.edit_lighting(
                            &pid,
                            &lid,
                            zone,
                            LightingEdit::Effect(effect),
                            all_layers,
                        )
                    })
                    .map_err(WorkbenchError::Storage)?;
                let proj = self.build_lighting_modal_projection(None)?;
                self.active_modal = Some(EditorModalProjection::LightingEditor(proj));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::LightingEditBrightness {
                zone,
                brightness_percent,
                all_layers,
            } => {
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                self.lighting_apply_to_all_layers = all_layers;
                let clamped = brightness_percent.min(100);
                self.document_store
                    .update(|draft| {
                        ensure_layer_has_base_lighting(draft, &pid, &lid, all_layers)?;
                        draft.edit_lighting(
                            &pid,
                            &lid,
                            zone,
                            LightingEdit::Brightness(clamped),
                            all_layers,
                        )
                    })
                    .map_err(WorkbenchError::Storage)?;
                let proj = self.build_lighting_modal_projection(None)?;
                self.active_modal = Some(EditorModalProjection::LightingEditor(proj));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::LightingEditSpeed {
                zone,
                speed_percent,
                all_layers,
            } => {
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                self.lighting_apply_to_all_layers = all_layers;
                let clamped = speed_percent.min(100);
                self.document_store
                    .update(|draft| {
                        ensure_layer_has_base_lighting(draft, &pid, &lid, all_layers)?;
                        draft.edit_lighting(
                            &pid,
                            &lid,
                            zone,
                            LightingEdit::Speed(clamped),
                            all_layers,
                        )
                    })
                    .map_err(WorkbenchError::Storage)?;
                let proj = self.build_lighting_modal_projection(None)?;
                self.active_modal = Some(EditorModalProjection::LightingEditor(proj));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::LightingEditMagic {
                zone,
                magic_percent,
                all_layers,
            } => {
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                self.lighting_apply_to_all_layers = all_layers;
                let clamped = magic_percent.min(100);
                self.document_store
                    .update(|draft| {
                        ensure_layer_has_base_lighting(draft, &pid, &lid, all_layers)?;
                        draft.edit_lighting(
                            &pid,
                            &lid,
                            zone,
                            LightingEdit::Magic(clamped),
                            all_layers,
                        )
                    })
                    .map_err(WorkbenchError::Storage)?;
                let proj = self.build_lighting_modal_projection(None)?;
                self.active_modal = Some(EditorModalProjection::LightingEditor(proj));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::LightingEditColorHex {
                zone,
                color_hex,
                all_layers,
            } => {
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                self.lighting_apply_to_all_layers = all_layers;
                let rgb_u32 = parse_hex_color(&color_hex).map_err(WorkbenchError::Storage)?;
                self.document_store
                    .update(|draft| {
                        ensure_layer_has_base_lighting(draft, &pid, &lid, all_layers)?;
                        draft.edit_lighting(
                            &pid,
                            &lid,
                            zone,
                            LightingEdit::Color(rgb_u32),
                            all_layers,
                        )
                    })
                    .map_err(WorkbenchError::Storage)?;
                let proj = self.build_lighting_modal_projection(None)?;
                self.active_modal = Some(EditorModalProjection::LightingEditor(proj));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::LightingResetLayer => {
                let (_, lid) = self.require_selected_profile_and_layer()?;
                let has_source = self
                    .document_store
                    .draft
                    .document
                    .editable
                    .source_refs
                    .contains_key(&lid);
                self.document_store
                    .update(|draft| {
                        if has_source {
                            draft.reset_lighting(&lid)
                        } else {
                            draft.mutate(|doc| {
                                if let Some(layer) = doc.editable.graph.layers.get_mut(&lid) {
                                    layer.lighting = None;
                                }
                                Ok(())
                            })
                        }
                    })
                    .map_err(WorkbenchError::Storage)?;
                let proj = self.build_lighting_modal_projection(None)?;
                self.active_modal = Some(EditorModalProjection::LightingEditor(proj));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::LightingBeginPreview => {
                self.lighting_preview_active = true;
                self.lighting_preview_remaining_secs = 30;
                let proj = self.build_lighting_modal_projection(None)?;
                self.active_modal = Some(EditorModalProjection::LightingEditor(proj));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::LightingEndPreview => {
                self.lighting_preview_active = false;
                self.lighting_preview_remaining_secs = 0;
                let proj = self.build_lighting_modal_projection(None)?;
                self.active_modal = Some(EditorModalProjection::LightingEditor(proj));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::OpenMultiEditor { multi_id } => {
                let proj = self.build_multi_modal_projection(multi_id)?;
                self.active_modal = Some(EditorModalProjection::MultiEditor(proj));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MultiSetName(name) => {
                let mut modal = self.take_multi_modal()?;
                modal.name = name;
                self.active_modal = Some(EditorModalProjection::MultiEditor(modal));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MultiSetTermMs(term_ms) => {
                let mut modal = self.take_multi_modal()?;
                modal.term_ms = term_ms;
                self.active_modal = Some(EditorModalProjection::MultiEditor(modal));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MultiSetBranch { branch, target } => {
                let mut modal = self.take_multi_modal()?;
                let display_name = target
                    .as_ref()
                    .map(|t| self.resolve_multi_target_display_name(t))
                    .unwrap_or_else(|| "未设置".to_string());
                modal.branches[branch as usize] = MultiBranchSlotProjection {
                    branch,
                    title: branch.title().to_string(),
                    target,
                    target_display_name: display_name,
                };
                self.active_modal = Some(EditorModalProjection::MultiEditor(modal));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MultiSave {
                bind_to_selected_slot,
            } => {
                let mut modal = self.take_multi_modal()?;
                let mut additions: BTreeMap<String, Action> = BTreeMap::new();
                let mut resolved_refs: [Option<ActionReference>; 4] = [None, None, None, None];
                for (idx, slot_proj) in modal.branches.iter().enumerate() {
                    if let Some(target) = &slot_proj.target {
                        match self.resolve_multi_branch_reference(target, &mut additions) {
                            Ok(reference) => resolved_refs[idx] = Some(reference),
                            Err(err) => {
                                modal.validation_error = Some(err.to_string());
                                self.active_modal = Some(EditorModalProjection::MultiEditor(modal));
                                self.commit_runtime_update();
                                return Err(err);
                            }
                        }
                    }
                }
                let local_multi = LocalMulti {
                    format_version: 1,
                    term_ms: modal.term_ms,
                    tap: resolved_refs[0].clone(),
                    double_tap: resolved_refs[1].clone(),
                    hold: resolved_refs[2].clone(),
                    tap_hold: resolved_refs[3].clone(),
                };
                if let Err(err) = local_multi.validate() {
                    modal.validation_error = Some(err.clone());
                    self.active_modal = Some(EditorModalProjection::MultiEditor(modal));
                    self.commit_runtime_update();
                    return Err(WorkbenchError::Storage(err));
                }
                let action = Action {
                    id: modal
                        .multi_id
                        .clone()
                        .unwrap_or_else(|| "local-multi".to_string()),
                    name: modal.name.trim().to_string(),
                    category: "Multi".to_string(),
                    detail: "本地四分支 · 不执行 · 固件范围未知".to_string(),
                    payload: serde_json::to_value(ActionKind::LocalMulti { multi: local_multi })
                        .map_err(|e| WorkbenchError::Storage(e.to_string()))?,
                };
                if bind_to_selected_slot {
                    let (pid, lid) = self.require_selected_profile_and_layer()?;
                    let slot = modal
                        .target_slot
                        .unwrap_or_else(|| self.current_selected_slot());
                    self.document_store
                        .update(|draft| {
                            draft
                                .save_multi_for_binding(
                                    modal.multi_id.as_deref(),
                                    action,
                                    additions,
                                    &json!(pid),
                                    &json!(lid),
                                    slot,
                                )
                                .map(|_| ())
                        })
                        .map_err(WorkbenchError::Storage)?;
                    self.slot_role_overrides
                        .insert((pid, slot), KeycapRole::ActionKey);
                } else if let Some(ref existing_id) = modal.multi_id {
                    self.document_store
                        .update(|draft| {
                            draft.save_multi(existing_id, action, additions).map(|_| ())
                        })
                        .map_err(WorkbenchError::Storage)?;
                } else {
                    let (pid, lid) = self.require_selected_profile_and_layer()?;
                    let slot = modal
                        .target_slot
                        .unwrap_or_else(|| self.current_selected_slot());
                    self.document_store
                        .update(|draft| {
                            draft
                                .save_multi_for_binding(
                                    None,
                                    action,
                                    additions,
                                    &json!(pid),
                                    &json!(lid),
                                    slot,
                                )
                                .map(|_| ())
                        })
                        .map_err(WorkbenchError::Storage)?;
                }
                self.active_modal = None;
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MultiBindExisting { multi_id } => {
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                let slot = self.current_selected_slot();
                self.document_store
                    .update(|draft| {
                        draft.bind_multi(&json!(pid), &json!(lid), slot, multi_id.as_deref())
                    })
                    .map_err(WorkbenchError::Storage)?;
                self.slot_role_overrides
                    .insert((pid, slot), KeycapRole::ActionKey);
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MultiDelete { multi_id } => {
                self.document_store
                    .update(|draft| draft.delete_multi(&multi_id))
                    .map_err(WorkbenchError::Storage)?;
                if matches!(
                    &self.active_modal,
                    Some(EditorModalProjection::MultiEditor(m)) if m.multi_id.as_deref() == Some(multi_id.as_str())
                ) {
                    self.active_modal = None;
                }
                self.commit_runtime_update();
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn take_macro_modal(&self) -> Result<MacroEditorModalProjection, WorkbenchError> {
        match &self.active_modal {
            Some(EditorModalProjection::MacroEditor(m)) => Ok(m.clone()),
            _ => Err(WorkbenchError::Storage("未打开宏编辑器".to_string())),
        }
    }

    fn take_multi_modal(&self) -> Result<MultiEditorModalProjection, WorkbenchError> {
        match &self.active_modal {
            Some(EditorModalProjection::MultiEditor(m)) => Ok(m.clone()),
            _ => Err(WorkbenchError::Storage("未打开 Multi 编辑器".to_string())),
        }
    }

    fn build_lighting_modal_projection(
        &self,
        validation_error: Option<String>,
    ) -> Result<LightingEditorModalProjection, WorkbenchError> {
        let (pid, lid) = self.require_selected_profile_and_layer()?;
        let doc = &self.document_store.draft.document;
        let layer_name = doc
            .editable
            .graph
            .layers
            .get(&lid)
            .map(|l| l.name.clone())
            .unwrap_or_else(|| "默认层".to_string());
        let lighting_opt = doc.lighting(&lid).ok();
        let has_lighting = lighting_opt.is_some();
        let backlight = lighting_opt.as_ref().map(|l| {
            build_zone_projection(
                LightingZone::Backlight,
                "按键背光 (Backlight)",
                &l.backlight,
            )
        });
        let underglow = lighting_opt.as_ref().map(|l| {
            build_zone_projection(
                LightingZone::Underglow,
                "底灯环绕 (Underglow)",
                &l.underglow,
            )
        });
        Ok(LightingEditorModalProjection {
            profile_id: pid,
            layer_id: lid.clone(),
            layer_name,
            apply_to_all_layers: self.lighting_apply_to_all_layers,
            can_add_lighting: !doc.is_protected_oai_layer(&lid) && !has_lighting,
            has_lighting,
            backlight,
            underglow,
            preview_active: self.lighting_preview_active,
            preview_remaining_secs: self.lighting_preview_remaining_secs,
            validation_error,
        })
    }

    fn build_multi_modal_projection(
        &self,
        multi_id: Option<String>,
    ) -> Result<MultiEditorModalProjection, WorkbenchError> {
        let target_slot = Some(self.current_selected_slot());
        let available_actions: Vec<(String, String)> = mi_model::draft::actions()
            .into_iter()
            .filter(|a| a.id != "local-macro" && a.id != "local-multi")
            .map(|a| (a.id, a.name))
            .collect();
        let available_macros: Vec<(String, String)> = self
            .document_store
            .draft
            .macros()
            .into_iter()
            .map(|m| (m.id, m.action.name))
            .collect();

        if let Some(id) = multi_id {
            let act = self
                .document_store
                .draft
                .document
                .editable
                .graph
                .actions
                .get(&id)
                .ok_or_else(|| WorkbenchError::Storage("Multi 动作不存在".to_string()))?;
            let ActionKind::LocalMulti { multi } = serde_json::from_value(act.payload.clone())
                .map_err(|_| WorkbenchError::Storage("不是有效的 Multi 动作".to_string()))?
            else {
                return Err(WorkbenchError::Storage("不是有效的 Multi 动作".to_string()));
            };
            let refs = [&multi.tap, &multi.double_tap, &multi.hold, &multi.tap_hold];
            let branches: [MultiBranchSlotProjection; 4] = std::array::from_fn(|idx| {
                let kind = MultiBranchKind::ALL[idx];
                let target = refs[idx].as_ref().map(|r| match r {
                    ActionReference::Action { action_id } => MultiBranchTarget::Action {
                        action_id: action_id.clone(),
                    },
                    ActionReference::Macro { action_id } => MultiBranchTarget::Macro {
                        action_id: action_id.clone(),
                    },
                });
                let display = target
                    .as_ref()
                    .map(|t| self.resolve_multi_target_display_name(t))
                    .unwrap_or_else(|| "未设置".to_string());
                MultiBranchSlotProjection {
                    branch: kind,
                    title: kind.title().to_string(),
                    target,
                    target_display_name: display,
                }
            });
            let reference_summaries = self
                .document_store
                .draft
                .multis()
                .into_iter()
                .find(|m| m.id == id)
                .map(|m| {
                    m.references
                        .iter()
                        .map(|r| {
                            format!(
                                "{} / {} · {}",
                                r.profile_name, r.layer_name, r.logical_control
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            Ok(MultiEditorModalProjection {
                multi_id: Some(id),
                name: act.name.clone(),
                term_ms: multi.term_ms,
                branches,
                bind_to_selected_slot: true,
                target_slot,
                available_actions,
                available_macros,
                reference_summaries,
                validation_error: None,
            })
        } else {
            let branches: [MultiBranchSlotProjection; 4] = std::array::from_fn(|idx| {
                let kind = MultiBranchKind::ALL[idx];
                MultiBranchSlotProjection {
                    branch: kind,
                    title: kind.title().to_string(),
                    target: None,
                    target_display_name: "未设置".to_string(),
                }
            });
            Ok(MultiEditorModalProjection {
                multi_id: None,
                name: "新建 Multi".to_string(),
                term_ms: 200,
                branches,
                bind_to_selected_slot: true,
                target_slot,
                available_actions,
                available_macros,
                reference_summaries: Vec::new(),
                validation_error: None,
            })
        }
    }

    fn resolve_multi_target_display_name(&self, target: &MultiBranchTarget) -> String {
        let action_id = match target {
            MultiBranchTarget::Action { action_id } | MultiBranchTarget::Macro { action_id } => {
                action_id
            }
        };
        if let Some(a) = self
            .document_store
            .draft
            .document
            .editable
            .graph
            .actions
            .get(action_id)
        {
            return a.name.clone();
        }
        if let Some(a) = self
            .document_store
            .draft
            .document
            .editable
            .graph
            .actions
            .values()
            .find(|a| a.id == *action_id)
        {
            return a.name.clone();
        }
        mi_model::draft::actions()
            .into_iter()
            .find(|a| a.id == *action_id)
            .map(|a| a.name)
            .unwrap_or_else(|| action_id.clone())
    }

    fn resolve_multi_branch_reference(
        &self,
        target: &MultiBranchTarget,
        additions: &mut BTreeMap<String, Action>,
    ) -> Result<ActionReference, WorkbenchError> {
        let graph_actions = &self.document_store.draft.document.editable.graph.actions;
        match target {
            MultiBranchTarget::Macro { action_id } => {
                if !graph_actions.contains_key(action_id) {
                    return Err(WorkbenchError::Storage(format!(
                        "未找到引用的宏 {action_id}"
                    )));
                }
                Ok(ActionReference::Macro {
                    action_id: action_id.clone(),
                })
            }
            MultiBranchTarget::Action { action_id } => {
                if graph_actions.contains_key(action_id) {
                    return Ok(ActionReference::Action {
                        action_id: action_id.clone(),
                    });
                }
                if let Some((existing_uuid, _)) =
                    graph_actions.iter().find(|(_, a)| a.id == *action_id)
                {
                    return Ok(ActionReference::Action {
                        action_id: existing_uuid.clone(),
                    });
                }
                if let Some((added_uuid, _)) = additions.iter().find(|(_, a)| a.id == *action_id) {
                    return Ok(ActionReference::Action {
                        action_id: added_uuid.clone(),
                    });
                }
                let catalog_action = self.resolve_action_by_id(action_id)?;
                let new_uuid = mi_model::document::new_id();
                additions.insert(new_uuid.clone(), catalog_action);
                Ok(ActionReference::Action {
                    action_id: new_uuid,
                })
            }
        }
    }

    pub fn active_modal(&self) -> Option<&EditorModalProjection> {
        self.projection.active_modal.as_ref()
    }

    pub fn selected_keycap(&self, slot: u32) -> CodexKeycapId {
        self.projection_inputs().selected_keycap(slot)
    }
}

impl Drop for WorkbenchStore {
    fn drop(&mut self) {
        self.hardware_event_tx.close();
        if let Some(session) = &self.device_session {
            session.disconnect();
        }
        if let Some(running) = &self.six_layer_running {
            running.retire();
        }
    }
}

fn matches_logical_slot(logical_control: &str, slot: u32) -> bool {
    logical_control
        .strip_prefix("logical-")
        .and_then(|s| s.parse::<u32>().ok())
        == Some(slot)
}

fn resolve_slot_graph_action_id(layer: Option<&LayerNode>, slot: u32) -> Option<String> {
    let layer = layer?;
    layer
        .controls
        .values()
        .find(|b| matches_logical_slot(&b.logical_control, slot))
        .and_then(|b| b.action_id.clone())
}

fn format_macro_references(refs: &[mi_model::macro_library::MacroReference]) -> String {
    if refs.is_empty() {
        return "未绑定".to_string();
    }
    refs.iter()
        .map(|r| {
            format!(
                "{} / {} · {}",
                r.profile_name, r.layer_name, r.logical_control
            )
        })
        .collect::<Vec<_>>()
        .join("、")
}

fn is_supported_macro_key(key: &str) -> bool {
    let trimmed = key.trim();
    !trimmed.is_empty() && trimmed.len() <= 256
}

fn build_macro_step_drafts(events: &[MacroEvent]) -> Vec<MacroStepDraft> {
    events
        .iter()
        .enumerate()
        .map(|(index, event)| {
            let (summary_label, macos_key_supported) = match event {
                MacroEvent::Press { key } => (
                    format!("按下 {}", key.to_uppercase()),
                    is_supported_macro_key(key),
                ),
                MacroEvent::Release { key } => (
                    format!("释放 {}", key.to_uppercase()),
                    is_supported_macro_key(key),
                ),
                MacroEvent::Delay { ms } => (format!("等待 {ms} ms"), *ms > 0),
            };
            MacroStepDraft {
                index,
                event: event.clone(),
                summary_label,
                macos_key_supported,
            }
        })
        .collect()
}

fn build_macro_editor_projection(
    macro_id: Option<String>,
    name: String,
    events: Vec<MacroEvent>,
    target_slot: Option<u32>,
    reference_summaries: Vec<String>,
) -> MacroEditorModalProjection {
    let steps = build_macro_step_drafts(&events);
    let host_execution_supported = steps.iter().all(|s| s.macos_key_supported);
    MacroEditorModalProjection {
        macro_id,
        name,
        steps,
        draft_key_input: "a".to_string(),
        draft_delay_ms: 30,
        bind_to_selected_slot: true,
        target_slot,
        reference_summaries,
        validation_error: None,
        host_execution_supported,
    }
}

fn rebuild_macro_modal_with_events(
    mut modal: MacroEditorModalProjection,
    events: Vec<MacroEvent>,
) -> MacroEditorModalProjection {
    let steps = build_macro_step_drafts(&events);
    modal.host_execution_supported = steps.iter().all(|s| s.macos_key_supported);
    modal.steps = steps;
    modal.validation_error = None;
    modal
}

fn ensure_layer_has_base_lighting(
    draft: &mut Draft,
    pid: &str,
    lid: &str,
    all_layers: bool,
) -> Result<(), String> {
    let target_lids: Vec<String> = if all_layers {
        draft
            .document
            .editable
            .graph
            .profiles
            .get(pid)
            .map(|p| {
                p.layer_order
                    .iter()
                    .filter(|id| !draft.document.is_protected_oai_layer(id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_else(|| vec![lid.to_string()])
    } else {
        vec![lid.to_string()]
    };

    for target_lid in target_lids {
        if draft.document.lighting(&target_lid).is_ok() {
            continue;
        }
        if draft.document.can_add_lighting(&target_lid) {
            draft.add_lighting(&target_lid)?;
        } else {
            draft.mutate(|doc| {
                if let Some(layer) = doc.editable.graph.layers.get_mut(&target_lid) {
                    layer.lighting = Some(Lighting::creator_micro_2_defaults());
                }
                Ok(())
            })?;
        }
    }
    Ok(())
}

fn build_zone_projection(
    zone: LightingZone,
    zone_title: &str,
    raw: &Value,
) -> LightingZoneProjection {
    let effect: Option<LightingEffect> = raw
        .get("effect")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let brightness_percent = raw
        .get("brightness")
        .and_then(|v| v.as_f64())
        .map(|f| (f.clamp(0.0, 1.0) * 100.0).round() as u8)
        .unwrap_or(100);
    let speed_percent = raw
        .get("speed")
        .and_then(|v| v.as_f64())
        .map(|f| (f.clamp(0.0, 1.0) * 100.0).round() as u8)
        .unwrap_or(50);
    let magic_percent = raw
        .get("magic")
        .and_then(|v| v.as_f64())
        .map(|f| (f.clamp(0.0, 1.0) * 100.0).round() as u8)
        .unwrap_or(50);
    let color_u32 = raw
        .get("color")
        .and_then(|v| v.as_u64())
        .map(|n| (n as u32) & 0x00FF_FFFF)
        .unwrap_or(0x00FF_FFFF);
    let color_hex = format!("#{color_u32:06X}");
    let (can_edit_color, can_edit_brightness, can_edit_speed, can_edit_magic) = match effect {
        Some(LightingEffect::Off) => (false, false, false, false),
        Some(LightingEffect::Solid) => (true, true, false, false),
        Some(LightingEffect::Gradient) => (true, true, false, true),
        Some(LightingEffect::Snake) => (true, true, true, false),
        Some(LightingEffect::Rainbow) => (false, true, true, true),
        Some(LightingEffect::Breath) => (true, true, true, true),
        None => (false, false, false, false),
    };
    LightingZoneProjection {
        zone,
        zone_title: zone_title.to_string(),
        effect,
        brightness_percent,
        speed_percent,
        magic_percent,
        color_hex,
        can_edit_color,
        can_edit_brightness,
        can_edit_speed,
        can_edit_magic,
    }
}

fn parse_hex_color(input: &str) -> Result<u32, String> {
    let trimmed = input.trim();
    let hex = trimmed.strip_prefix('#').unwrap_or(trimmed);
    if hex.len() != 6 {
        return Err(format!("颜色格式无效（需为 #RRGGBB）：{input}"));
    }
    u32::from_str_radix(hex, 16).map_err(|_| format!("颜色十六进制无效：{input}"))
}

fn build_comparison_modal_projection(
    cmp: &mi_model::comparison::Comparison,
) -> ReconnectComparisonModalProjection {
    let rows = cmp
        .rows
        .iter()
        .map(|r| ComparisonRowProjection {
            path: r.path.clone(),
            label: r.label.clone(),
            base_summary: r.base_summary.clone(),
            local_summary: r.local_summary.clone(),
            device_summary: r.device_summary.clone(),
            result_badge: r.result.clone(),
            is_conflict: r.result.contains("冲突"),
        })
        .collect();
    ReconnectComparisonModalProjection {
        token: cmp.token.clone(),
        revision: cmp.revision,
        identity_matches: cmp.identity_matches,
        identity_conflict: cmp.identity_conflict,
        identity_confirmation_required: cmp.identity_confirmation_required,
        identity_user_confirmed: cmp.identity_user_confirmed,
        can_merge: cmp.can_merge(),
        conflicts: cmp.conflicts.clone(),
        rows,
        error_message: None,
    }
}

fn build_apply_modal_projection(
    preview: &mi_model::apply::Preview,
) -> ApplyPreparationModalProjection {
    let changes = preview
        .changes
        .iter()
        .map(|c| {
            let kind = c
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("change")
                .to_string();
            let location_label = c
                .get("control")
                .or_else(|| c.get("zone"))
                .or_else(|| c.get("field"))
                .and_then(|v| v.as_str())
                .unwrap_or("layer")
                .to_string();
            let before = c
                .get("before")
                .map(Value::to_string)
                .unwrap_or_else(|| "null".to_string());
            let after = c
                .get("after")
                .map(Value::to_string)
                .unwrap_or_else(|| "null".to_string());
            ApplyChangeRowProjection {
                kind,
                location_label,
                before,
                after,
            }
        })
        .collect();
    ApplyPreparationModalProjection {
        token: preview.token.clone(),
        kind: preview.kind.clone(),
        document_id: preview.document_id.clone(),
        revision: preview.revision,
        document_sha256: preview.document_sha256.clone(),
        planned_keymap_sha256: preview.planned_keymap_sha256.clone(),
        before_summary: preview.before.clone(),
        after_summary: preview.after.clone(),
        changes,
        backup_prepared: false,
        backup_path: None,
        manifest_sha256: None,
        candidate_taken: false,
        candidate_operation_id: None,
        unavailable_reason: preview.unavailable_reason.clone(),
        error_message: None,
    }
}

fn resolve_slot_action_info<'a>(
    doc: &'a LocalDocumentV2,
    layer: Option<&'a LayerNode>,
    slot: u32,
) -> (String, Option<&'a Action>) {
    let Some(layer) = layer else {
        return ("未绑定".to_string(), None);
    };
    let Some(binding) = layer
        .controls
        .values()
        .find(|b| matches_logical_slot(&b.logical_control, slot))
    else {
        return ("未绑定".to_string(), None);
    };
    if let Some(aid) = &binding.action_id {
        if let Some(action) = doc.editable.graph.actions.get(aid) {
            return (action.name.clone(), Some(action));
        }
    }
    if let Some(raw_token) = binding.original.as_str() {
        if raw_token != "Empty" && !raw_token.is_empty() {
            return (raw_token.to_string(), None);
        }
    }
    ("未绑定".to_string(), None)
}

fn resolve_merged_bottom_slot(doc: &LocalDocumentV2, layer: Option<&LayerNode>) -> u32 {
    let (_, act10) = resolve_slot_action_info(doc, layer, 10);
    let (_, act11) = resolve_slot_action_info(doc, layer, 11);
    let cmd10 = act10.and_then(|a| a.payload.get("command").and_then(|v| v.as_str()));
    let cmd11 = act11.and_then(|a| a.payload.get("command").and_then(|v| v.as_str()));
    if matches!(cmd10, Some("focus-input" | "new-conversation-in-project"))
        && matches!(cmd11, Some("push-to-talk" | "dictation"))
    {
        return 11;
    }
    if act10.is_some() {
        return 10;
    }
    if act11.is_some() {
        return 11;
    }
    10
}

fn resolve_silkscreen_icon(slot: u32, action: Option<&Action>) -> String {
    if let Some(a) = action {
        if a.id == "ag-live-voice"
            || a.id == "desktop-live-voice"
            || a.payload.get("command").and_then(|v| v.as_str()) == Some("live-voice")
        {
            return "🎙".to_string();
        }
        if a.payload.get("command").and_then(|v| v.as_str()) == Some("new-conversation-in-project")
        {
            return "＋".to_string();
        }
        if a.id.contains("dictation")
            || a.id.contains("ptt")
            || a.id == "apple-fn-key"
            || a.payload.get("command").and_then(|v| v.as_str()) == Some("push-to-talk")
        {
            return "🎤".to_string();
        }
        if a.id.contains("submit") {
            return "⏎".to_string();
        }
        if a.id.contains("slot-open") || a.id.contains("cockpit-slot") {
            return "💬".to_string();
        }
    }
    match slot {
        0..=5 => "💬".to_string(),
        10 | 11 => "🎤".to_string(),
        12 => "⏎".to_string(),
        _ => "⌘".to_string(),
    }
}

fn custom_monogram(name: &str) -> String {
    let trimmed = name.trim();
    let mut chars = trimmed.chars();
    match (chars.next(), chars.next()) {
        (Some(a), Some(b)) => format!("{}{}", a.to_uppercase(), b.to_uppercase()),
        (Some(a), None) => a.to_uppercase().to_string(),
        _ => "方案".to_string(),
    }
}

fn bundle_id_for_place(place: &NavigationPlace) -> &str {
    match place {
        NavigationPlace::App { bundle_id } => bundle_id.as_str(),
        other => agent_for_place(other).bundle_id(),
    }
}

fn fallback_unbridged_oai_action(
    profile_name: &str,
    binding_id: &str,
    layout: BottomRowLayout,
) -> Option<ResolvedHostAction> {
    let normalized = binding_id.trim().to_ascii_uppercase();
    let effective = if layout == BottomRowLayout::Merged && normalized == "AG11" {
        "AG10"
    } else {
        normalized.as_str()
    };
    let (adapter, bundle_id) = if profile_name.eq_ignore_ascii_case("Codex") {
        ("codex", "com.openai.codex")
    } else if profile_name.eq_ignore_ascii_case("ChatGPT") {
        ("chatgpt", "com.openai.atlas")
    } else {
        return None;
    };
    match effective {
        "AG00" => Some(ResolvedHostAction::OpenApp {
            bundle_id: bundle_id.to_string(),
        }),
        "AG01" => Some(ResolvedHostAction::AgentCommand {
            adapter: adapter.to_string(),
            command: "new-chat".to_string(),
        }),
        "AG02" => Some(ResolvedHostAction::AgentCommand {
            adapter: adapter.to_string(),
            command: "search".to_string(),
        }),
        "AG03" => Some(ResolvedHostAction::AgentCommand {
            adapter: adapter.to_string(),
            command: if adapter == "codex" {
                "effort".to_string()
            } else {
                "model".to_string()
            },
        }),
        "AG05" if adapter == "codex" => Some(ResolvedHostAction::AgentCommand {
            adapter: "codex".to_string(),
            command: "terminal".to_string(),
        }),
        "AG10" => Some(ResolvedHostAction::AgentCommand {
            adapter: adapter.to_string(),
            command: if layout == BottomRowLayout::Merged {
                "push-to-talk".to_string()
            } else {
                "focus-input".to_string()
            },
        }),
        "AG11" => Some(ResolvedHostAction::AgentCommand {
            adapter: adapter.to_string(),
            command: "push-to-talk".to_string(),
        }),
        "AG12" | "JS_CLK" => Some(ResolvedHostAction::AgentCommand {
            adapter: adapter.to_string(),
            command: "submit".to_string(),
        }),
        "AG13" | "ENC_CCW" => Some(ResolvedHostAction::AgentCommand {
            adapter: adapter.to_string(),
            command: "scroll-up".to_string(),
        }),
        "AG14" | "ENC_CW" => Some(ResolvedHostAction::AgentCommand {
            adapter: adapter.to_string(),
            command: "scroll-down".to_string(),
        }),
        "ENC_CLK" => Some(ResolvedHostAction::AgentCommand {
            adapter: adapter.to_string(),
            command: "scroll-to-bottom".to_string(),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod startup_tests {
    use super::*;

    #[test]

    fn automatic_host_startup_preserves_saved_v1_state_without_migrating() {
        for active_agent in [None, Some(CanonicalAgent::DeepseekHarness)] {
            let temp = tempfile::TempDir::new().unwrap();
            let seed = WorkbenchStore::open_isolated(temp.path()).unwrap();
            let active_id = active_agent.and_then(|agent| seed.find_canonical_profile_id(agent));
            fs::write(
                temp.path().join("workbench.json"),
                serde_json::to_vec(&seed.document_store.draft.document).unwrap(),
            )
            .unwrap();
            let original = serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version": 1, "favorites": [],
                "host_active_profile_id": active_id,
                "host_execution_enabled": false, "command_execution_enabled": false,
                "manual_override": false, "auto_follow_enabled": true
            }))
            .unwrap();
            let path = temp.path().join("workbench.preferences.json");
            fs::write(&path, &original).unwrap();
            drop(seed);

            let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();
            let preserve_saved_host_state = store.document_store.has_saved_host_preferences();
            assert!(preserve_saved_host_state);
            let before = store.document_store.routing_state();
            store.initialize_auto_host_ready(preserve_saved_host_state);

            assert_eq!(store.document_store.routing_state(), before);
            assert_eq!(store.active_host_profile_id(), active_id);
            assert!(!store.manual_override);
            assert!(!store.document_store.host_execution_enabled());
            assert_eq!(fs::read(path).unwrap(), original);
            assert!(!temp
                .path()
                .join("workbench.preferences.v1.backup.json")
                .exists());
        }
    }

    #[test]
    fn automatic_host_startup_initializes_a_new_install() {
        let temp = tempfile::TempDir::new().unwrap();
        let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();
        store.initialize_auto_host_ready(false);
        assert_eq!(
            store.active_host_profile_id(),
            store.find_canonical_profile_id(CanonicalAgent::Antigravity)
        );
        assert!(store.document_store.host_execution_enabled());
    }
}

fn parse_hex_rgb(hex: &str) -> Option<u32> {
    let trimmed = hex.trim().strip_prefix('#').unwrap_or(hex.trim());
    if trimmed.len() != 6 {
        return None;
    }
    u32::from_str_radix(trimmed, 16).ok()
}

fn parse_zone_candidate(
    zone: Option<&Value>,
) -> mi_proto::lighting_candidate::LightingZoneCandidate {
    let Some(z) = zone else {
        return mi_proto::lighting_candidate::LightingZoneCandidate::default();
    };
    mi_proto::lighting_candidate::LightingZoneCandidate {
        effect: z
            .get("effect")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        brightness: z
            .get("brightness")
            .and_then(Value::as_f64)
            .map(|v| v.clamp(0.0, 1.0)),
        speed: z
            .get("speed")
            .and_then(Value::as_f64)
            .map(|v| v.clamp(0.0, 1.0)),
        magic: z
            .get("magic")
            .and_then(Value::as_f64)
            .map(|v| v.clamp(0.0, 1.0)),
        color: z
            .get("color")
            .and_then(Value::as_str)
            .and_then(parse_hex_rgb),
    }
}

fn parse_hardware_lighting_payload(
    payload_str: &str,
) -> Result<
    (
        mi_proto::lighting_candidate::LightingPreviewCandidate,
        Vec<mi_proto::lighting_candidate::OaiKeyStatusEntry>,
    ),
    String,
> {
    let parsed: Value = serde_json::from_str(payload_str).map_err(|e| e.to_string())?;
    let preview = mi_proto::lighting_candidate::LightingPreviewCandidate {
        backlight: parse_zone_candidate(parsed.get("backlight")),
        underglow: parse_zone_candidate(parsed.get("underglow")),
    };
    mi_proto::lighting_candidate::encode_oai_rgbcfg_candidate(&preview, 0)
        .map_err(|e| e.to_string())?;
    mi_proto::lighting_candidate::encode_lighting_preview_candidate(&preview, 0)
        .map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    if let Some(items) = parsed.get("per_key").and_then(Value::as_array) {
        for item in items {
            let Some(k) = item.get("k").and_then(Value::as_str) else {
                continue;
            };
            let color = item
                .get("color")
                .and_then(Value::as_str)
                .and_then(parse_hex_rgb)
                .unwrap_or(0);
            let brightness = item
                .get("brightness")
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            let effect = item
                .get("e")
                .and_then(Value::as_u64)
                .and_then(|v| u8::try_from(v).ok())
                .unwrap_or(1)
                .min(6);
            let speed = item
                .get("s")
                .and_then(Value::as_f64)
                .unwrap_or(0.65)
                .clamp(0.0, 1.0);
            entries.push(mi_proto::lighting_candidate::OaiKeyStatusEntry::with_style(
                k, color, brightness, effect, speed,
            ));
        }
    }
    if !entries.is_empty() {
        mi_proto::lighting_candidate::encode_oai_thstatus_candidate(&entries, 0)
            .map_err(|e| e.to_string())?;
    }
    Ok((preview, entries))
}

fn default_acceptance_lighting_payload() -> (
    mi_proto::lighting_candidate::LightingPreviewCandidate,
    Vec<mi_proto::lighting_candidate::OaiKeyStatusEntry>,
) {
    let off_zone = mi_proto::lighting_candidate::LightingZoneCandidate {
        effect: Some("off".to_string()),
        brightness: Some(0.0),
        speed: Some(0.0),
        magic: Some(0.5),
        color: Some(0),
    };
    let preview = mi_proto::lighting_candidate::LightingPreviewCandidate {
        backlight: off_zone.clone(),
        underglow: off_zone,
    };
    let keys = [
        "AG00", "AG01", "AG02", "AG03", "AG04", "AG05", "AG06", "AG07", "AG08", "AG09", "AG10",
        "AG11", "AG12",
    ];
    let entries = keys
        .into_iter()
        .map(|k| mi_proto::lighting_candidate::OaiKeyStatusEntry::with_style(k, 0, 0.0, 0, 0.65))
        .collect();
    (preview, entries)
}

async fn sync_hardware_lighting_to_session(
    session: &DeviceSession,
    preview: &mi_proto::lighting_candidate::LightingPreviewCandidate,
    entries: &[mi_proto::lighting_candidate::OaiKeyStatusEntry],
) -> (bool, bool, bool) {
    let rgbcfg_ok = session.send_oai_rgbcfg(preview).await.is_ok();
    let preview_ok = session.send_lights_preview(preview).await.is_ok();
    let thstatus_ok = if entries.is_empty() {
        true
    } else {
        session.send_oai_thstatus(entries).await.is_ok()
    };
    (rgbcfg_ok, preview_ok, thstatus_ok)
}

async fn run_automatic_hardware_acceptance(
    session: &DeviceSession,
    target: &mi_transport::HidCandidate,
    initial_lighting_json: &str,
) -> Option<HardwareAutoAcceptanceReceipt> {
    let identity = session.read_identity_info().await.ok()?;
    let _config = session.read_config_snapshot().await.ok()?;
    let snap = session.snapshot()?;
    let keymap_size = snap.files.keymap.meta.size;
    let keymap_sha1 = snap.files.keymap.meta.checksum_hex();
    let keymap_sha256 = mi_bridge::sha256_hex(&snap.files.keymap.bytes);
    let smart_actions_present = snap.files.smart_actions.is_some();
    let smart_actions_size = snap.files.smart_actions.as_ref().map(|f| f.meta.size);
    let smart_actions_sha1 = snap
        .files
        .smart_actions
        .as_ref()
        .map(|f| f.meta.checksum_hex());
    let (preview, entries) = if initial_lighting_json.trim().is_empty() {
        default_acceptance_lighting_payload()
    } else {
        parse_hardware_lighting_payload(initial_lighting_json)
            .unwrap_or_else(|_| default_acceptance_lighting_payload())
    };
    let (rgbcfg_ok, preview_ok, thstatus_ok) =
        sync_hardware_lighting_to_session(session, &preview, &entries).await;
    let receipt = HardwareAutoAcceptanceReceipt {
        schema_version: 1,
        status: "PASS".to_string(),
        session_id: session.id,
        backend: session.backend_name().to_string(),
        exclusive_vendor: session.has_exclusive_vendor_link(),
        product: target.product.clone(),
        vid_pid: format!("{:04X}:{:04X}", target.vid, target.pid),
        device_path: target.path.clone(),
        firmware_version: identity.version.unwrap_or_else(|| "unknown".to_string()),
        profile_index: identity.profile_index,
        layer_index: identity.layer_index,
        keymap_size,
        keymap_sha1,
        keymap_sha256,
        smart_actions_present,
        smart_actions_size,
        smart_actions_sha1,
        lighting_rgbcfg_synced: rgbcfg_ok,
        lighting_preview_synced: preview_ok,
        lighting_thstatus_synced: thstatus_ok,
        timestamp_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
    };
    Some(receipt)
}

fn parse_iso8601_utc_ms(raw: &str) -> u64 {
    let trimmed = raw.trim().strip_suffix('Z').unwrap_or(raw.trim());
    let Some((date_part, time_part)) = trimmed.split_once('T') else {
        return 0;
    };
    let mut date_it = date_part.split('-');
    let (Some(y_s), Some(m_s), Some(d_s)) = (date_it.next(), date_it.next(), date_it.next()) else {
        return 0;
    };
    let (Ok(mut y), Ok(m), Ok(d)) = (y_s.parse::<i64>(), m_s.parse::<i64>(), d_s.parse::<i64>())
    else {
        return 0;
    };
    let (hms_part, frac_part) = time_part.split_once('.').unwrap_or((time_part, ""));
    let mut hms_it = hms_part.split(':');
    let (Some(hh_s), Some(mm_s), Some(ss_s)) = (hms_it.next(), hms_it.next(), hms_it.next()) else {
        return 0;
    };
    let (Ok(hh), Ok(mm), Ok(ss)) = (
        hh_s.parse::<u64>(),
        mm_s.parse::<u64>(),
        ss_s.parse::<u64>(),
    ) else {
        return 0;
    };
    let mut frac_ms = 0u64;
    for (idx, ch) in frac_part.chars().take(3).enumerate() {
        if let Some(digit) = ch.to_digit(10) {
            let weight = match idx {
                0 => 100,
                1 => 10,
                _ => 1,
            };
            frac_ms += u64::from(digit) * weight;
        }
    }
    if m <= 2 {
        y -= 1;
    }
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = (if m > 2 { m - 3 } else { m + 9 }) as u64;
    let doy = (153 * mp + 2) / 5 + (d as u64) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + (doe as i64) - 719468;
    if days < 0 {
        return 0;
    }
    (days as u64) * 86_400_000 + (hh * 3600 + mm * 60 + ss) * 1000 + frac_ms
}

fn find_freshest_antigravity_hub() -> Option<(u16, String, AntigravityInstance)> {
    let home = env::var_os("HOME").map(PathBuf::from)?;
    let daemon_dir = home.join(".gemini/antigravity/daemon");
    let entries = fs::read_dir(daemon_dir).ok()?;
    let mut hubs: Vec<(std::time::SystemTime, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            let name = path.file_name()?.to_str()?;
            if !name.starts_with("hub_") || !name.ends_with(".json") {
                return None;
            }
            let modified = e.metadata().ok()?.modified().ok()?;
            Some((modified, path))
        })
        .collect();
    hubs.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, path) in hubs {
        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(val) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let Some(port_u64) = val.get("httpPort").and_then(Value::as_u64) else {
            continue;
        };
        let Ok(port) = u16::try_from(port_u64) else {
            continue;
        };
        let Some(csrf) = val
            .get("csrfToken")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        path.hash(&mut hash);
        raw.hash(&mut hash);
        let pid = mi_macos::session_navigation::antigravity_process_id()?;
        return Some((
            port,
            csrf.to_string(),
            AntigravityInstance {
                hub: format!("{:x}", hash.finish()),
                pid,
                ui_port: u16::try_from(val.get("httpsPort")?.as_u64()?).ok()?,
            },
        ));
    }
    None
}

pub(crate) fn classify_antigravity_trajectory_summaries(
    summaries: &serde_json::Map<String, Value>,
    active_frontmost_cid: Option<&str>,
    seen_running_ids: &mut BTreeSet<String>,
    completed_transition_watermarks: &mut BTreeMap<String, String>,
) -> (
    Vec<FourAppLiveCandidate>,
    BTreeMap<String, String>,
    Option<(NavigationPlace, String)>,
) {
    let mut candidates = Vec::new();
    let mut project_ids = BTreeMap::new();
    let mut frontmost_acknowledged = None;
    for (cid, item) in summaries {
        let Some(obj) = item.as_object() else {
            continue;
        };
        let ann = obj.get("annotations").and_then(Value::as_object);
        let meta = obj.get("trajectoryMetadata").and_then(Value::as_object);
        if ann.and_then(|a| a.get("archived")).and_then(Value::as_bool) == Some(true)
            || ann
                .and_then(|a| a.get("antigravity.isSubagent"))
                .is_some_and(|v| {
                    v.as_bool() == Some(true)
                        || v.as_str().is_some_and(|s| s.eq_ignore_ascii_case("true"))
                })
        {
            seen_running_ids.remove(cid);
            completed_transition_watermarks.remove(cid);
            continue;
        }
        if let Some(m) = meta {
            if m.get("parentConversationId")
                .and_then(Value::as_str)
                .is_some_and(|s| !s.trim().is_empty())
                || m.get("subagentSpec").is_some_and(|v| !v.is_null())
                || m.get("isBattleModeFork").and_then(Value::as_bool) == Some(true)
                || m.get("nestingDepth")
                    .and_then(Value::as_i64)
                    .is_some_and(|d| d > 0)
                || m.get("sourceMetadata")
                    .and_then(Value::as_object)
                    .and_then(|sm| sm.get("hideFromConversationList"))
                    .and_then(Value::as_bool)
                    == Some(true)
            {
                seen_running_ids.remove(cid);
                completed_transition_watermarks.remove(cid);
                continue;
            }
        }
        let trajectory_type = obj
            .get("trajectoryType")
            .and_then(Value::as_str)
            .or_else(|| {
                meta.and_then(|m| m.get("trajectoryType"))
                    .and_then(Value::as_str)
            })
            .unwrap_or_default();
        if trajectory_type.ends_with("SIDE_QUESTION")
            || trajectory_type.ends_with("COMMIT_MESSAGE_GENERATION")
        {
            seen_running_ids.remove(cid);
            completed_transition_watermarks.remove(cid);
            continue;
        }
        let summary_str = obj
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        let title_str = ann
            .and_then(|a| a.get("title"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        let has_user_input = obj
            .get("lastUserInputTime")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty());
        if summary_str.is_empty() && title_str.is_empty() && !has_user_input {
            continue;
        }
        let Some(workspace_path) = session_navigation::workspace_from_summary(item) else {
            continue;
        };
        if let Some(pid_str) = meta
            .and_then(|m| m.get("projectId"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            project_ids.insert(cid.clone(), pid_str.to_string());
        }
        let title = if !summary_str.is_empty() {
            summary_str.to_string()
        } else if !title_str.is_empty() {
            title_str.to_string()
        } else {
            cid.chars().take(8).collect()
        };

        let status_str = obj
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("CASCADE_RUN_STATUS_IDLE");
        let last_mod_str = obj
            .get("lastModifiedTime")
            .and_then(Value::as_str)
            .or_else(|| obj.get("createdTime").and_then(Value::as_str))
            .unwrap_or_default();
        let view_str = ann
            .and_then(|a| a.get("lastUserViewTime"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let term_str = obj
            .get("latestExecutorTerminationReason")
            .and_then(Value::as_str)
            .or_else(|| obj.get("terminationReason").and_then(Value::as_str))
            .unwrap_or_default();
        let killed = obj.get("killed").and_then(Value::as_bool).unwrap_or(false);
        let waiting_steps = obj
            .get("waitingSteps")
            .and_then(Value::as_array)
            .is_some_and(|a| !a.is_empty());
        let has_running_subagents = obj
            .get("hasRunningSubagents")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let not_fully_idle = obj
            .get("notFullyIdle")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let errored = obj.get("errored").and_then(Value::as_bool).unwrap_or(false);
        let marked_unread = ann
            .and_then(|a| a.get("markedAsUnread"))
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let last_modified_ms = parse_iso8601_utc_ms(last_mod_str);
        let raw_view_ms = parse_iso8601_utc_ms(view_str);
        let watermark = if marked_unread {
            format!("{status_str}:{last_mod_str}:{term_str}:unread")
        } else {
            format!("{status_str}:{last_mod_str}:{term_str}")
        };

        let (phase, effective_view_ms) = if !killed
            && (waiting_steps
                || matches!(
                    status_str,
                    "CASCADE_RUN_STATUS_WAITING_FOR_USER_INPUT"
                        | "CASCADE_RUN_STATUS_WAITING_FOR_PERMISSION"
                )) {
            seen_running_ids.insert(cid.clone());
            (FourAppSessionKind::NeedsAttention, raw_view_ms)
        } else if !killed
            && (matches!(
                status_str,
                "CASCADE_RUN_STATUS_RUNNING"
                    | "CASCADE_RUN_STATUS_BUSY"
                    | "CASCADE_RUN_STATUS_CANCELING"
                    | "CASCADE_RUN_STATUS_WAITING_FOR_MODEL"
            ) || has_running_subagents
                || not_fully_idle)
        {
            seen_running_ids.insert(cid.clone());
            (FourAppSessionKind::Working, raw_view_ms)
        } else if errored
            || matches!(
                status_str,
                "CASCADE_RUN_STATUS_ERROR" | "CASCADE_RUN_STATUS_FAILED"
            )
            || term_str == "ERROR"
            || term_str.ends_with("_ERROR")
        {
            seen_running_ids.remove(cid);
            (FourAppSessionKind::Error, raw_view_ms)
        } else {
            if seen_running_ids.remove(cid) {
                completed_transition_watermarks.insert(cid.clone(), watermark.clone());
            }
            let just_transitioned = completed_transition_watermarks.get(cid) == Some(&watermark);
            let has_unviewed = marked_unread || last_modified_ms > raw_view_ms.saturating_add(500);
            if active_frontmost_cid == Some(cid.as_str()) && !marked_unread {
                completed_transition_watermarks.remove(cid);
                if !watermark.is_empty() {
                    frontmost_acknowledged = Some((
                        NavigationPlace::antigravity(cid.clone(), workspace_path.clone()),
                        watermark.clone(),
                    ));
                }
                (FourAppSessionKind::Idle, last_modified_ms.max(raw_view_ms))
            } else if has_unviewed || just_transitioned {
                let adj_view_ms = if last_modified_ms > raw_view_ms.saturating_add(500) {
                    raw_view_ms
                } else {
                    last_modified_ms.saturating_sub(1000)
                };
                (FourAppSessionKind::BlueDotCompleted, adj_view_ms)
            } else {
                (FourAppSessionKind::Idle, raw_view_ms)
            }
        };

        candidates.push(FourAppLiveCandidate {
            place: NavigationPlace::antigravity(cid.clone(), workspace_path),
            title,
            phase,
            watermark,
            last_modified_ms,
            last_user_view_ms: effective_view_ms,
            updated_at_ms: last_modified_ms.max(effective_view_ms),
        });
    }

    candidates.sort_by(|a, b| {
        a.phase
            .cmp(&b.phase)
            .then_with(|| b.last_modified_ms.cmp(&a.last_modified_ms))
            .then_with(|| a.place.target_id().cmp(&b.place.target_id()))
    });
    (candidates, project_ids, frontmost_acknowledged)
}

fn poll_antigravity_daemon_candidates(
    seen_running_ids: &mut BTreeSet<String>,
    completed_transition_watermarks: &mut BTreeMap<String, String>,
) -> Option<NavigationCatalog> {
    poll_antigravity_daemon_candidates_with_frontmost(
        seen_running_ids,
        completed_transition_watermarks,
        true,
    )
}

fn poll_antigravity_daemon_candidates_with_frontmost(
    seen_running_ids: &mut BTreeSet<String>,
    completed_transition_watermarks: &mut BTreeMap<String, String>,
    suppress_frontmost: bool,
) -> Option<NavigationCatalog> {
    use std::io::{Read, Write};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};

    let (port, csrf, instance) = find_freshest_antigravity_hub()?;
    let body = b"{\"metadata\":{}}";
    let request = format!(
        "POST /exa.language_server_pb.LanguageServerService/GetAllCascadeTrajectories HTTP/1.0\r\n\
         Host: 127.0.0.1:{port}\r\n\
         Content-Type: application/json\r\n\
         Connect-Protocol-Version: 1\r\n\
         x-codeium-csrf-token: {csrf}\r\n\
         Connection: close\r\n\
         Content-Length: {}\r\n\r\n",
        body.len()
    );
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(2000)).ok()?;
    let _ = stream.set_write_timeout(Some(Duration::from_millis(2000)));
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    stream.write_all(request.as_bytes()).ok()?;
    stream.write_all(body).ok()?;
    let mut raw_resp = Vec::with_capacity(65536);
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if Instant::now() >= deadline || raw_resp.len() > 4 * 1024 * 1024 {
            return None;
        }
        let mut chunk = [0u8; 16384];
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => raw_resp.extend_from_slice(&chunk[..count]),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue
            }
            Err(_) => return None,
        }
    }
    if !raw_resp.starts_with(b"HTTP/1.1 200") && !raw_resp.starts_with(b"HTTP/1.0 200") {
        return None;
    }
    if find_freshest_antigravity_hub()?.2 != instance {
        return None;
    }
    let sep = b"\r\n\r\n";
    let body_start = raw_resp
        .windows(sep.len())
        .position(|w| w == sep)
        .map(|pos| pos + sep.len())?;
    let parsed: Value = serde_json::from_slice(&raw_resp[body_start..]).ok()?;
    let summaries = parsed.get("trajectorySummaries")?.as_object()?;
    let active_frontmost_cid = suppress_frontmost
        .then(|| {
            mi_macos::session_navigation::frontmost_selected_antigravity_conversation(
                instance.pid,
                instance.ui_port,
            )
        })
        .flatten();
    let (candidates, project_ids, frontmost_acknowledged) =
        classify_antigravity_trajectory_summaries(
            summaries,
            active_frontmost_cid.as_deref(),
            seen_running_ids,
            completed_transition_watermarks,
        );
    Some(NavigationCatalog {
        instance: CatalogInstance::Antigravity(instance),
        observed_at: Instant::now(),
        candidates,
        project_ids,
        frontmost_acknowledged,
    })
}

pub fn codex_observations_to_catalog(
    codex_home: std::path::PathBuf,
    observations: &[mi_macos::CodexThreadObservation],
) -> NavigationCatalog {
    let mut candidates = Vec::new();

    for obs in observations {
        let (phase, last_modified_ms, last_user_view_ms) = match &obs.status {
            mi_macos::CodexObservedStatus::Working => (
                FourAppSessionKind::Working,
                obs.updated_at_ms,
                obs.updated_at_ms,
            ),
            mi_macos::CodexObservedStatus::Error => (
                FourAppSessionKind::Error,
                obs.updated_at_ms,
                obs.updated_at_ms,
            ),
            mi_macos::CodexObservedStatus::Unread => (
                FourAppSessionKind::BlueDotCompleted,
                obs.updated_at_ms.max(1000),
                0,
            ),
            mi_macos::CodexObservedStatus::Idle => (
                FourAppSessionKind::Idle,
                obs.updated_at_ms,
                obs.updated_at_ms,
            ),
            mi_macos::CodexObservedStatus::Unknown(_) => continue,
        };

        let workspace_path = obs.workspace_path.trim().to_string();
        let title = if obs.display_title.trim().is_empty() {
            obs.thread_id.clone()
        } else {
            obs.display_title.trim().to_string()
        };
        let watermark = if obs.watermark.is_empty() {
            obs.updated_at_ms.to_string()
        } else {
            obs.watermark.clone()
        };

        candidates.push(FourAppLiveCandidate {
            place: NavigationPlace::codex(
                obs.thread_id.clone(),
                obs.host_id.clone(),
                workspace_path,
            ),
            title,
            phase,
            watermark,
            last_modified_ms,
            last_user_view_ms,
            updated_at_ms: obs.updated_at_ms,
        });
    }

    candidates.sort_by(|a, b| {
        a.phase
            .cmp(&b.phase)
            .then_with(|| b.updated_at_ms.cmp(&a.updated_at_ms))
            .then_with(|| a.place.target_id().cmp(&b.place.target_id()))
    });

    NavigationCatalog {
        instance: CatalogInstance::Codex { codex_home },
        observed_at: Instant::now(),
        candidates,
        project_ids: BTreeMap::new(),
        frontmost_acknowledged: None,
    }
}

fn poll_codex_session_candidates() -> Option<NavigationCatalog> {
    let codex_home = mi_macos::default_codex_home()?;
    let observations = mi_macos::observe_codex_threads(&codex_home, 64).ok()?;
    Some(codex_observations_to_catalog(codex_home, &observations))
}

#[cfg(test)]
#[path = "runtime_consistency_tests.rs"]
mod runtime_consistency_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_menu_cheat_sheet_retains_host_profile_during_device_editing_and_clears_when_unselected(
    ) {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut cfg = WorkbenchStoreConfig::isolated(dir.path());
        cfg.isolated = false;
        let mut store = WorkbenchStore::open(cfg).expect("open store");
        let antigravity_id = store
            .find_canonical_profile_id(CanonicalAgent::Antigravity)
            .expect("Antigravity profile");
        store
            .select_and_activate_profile(&antigravity_id)
            .expect("activate Antigravity");
        store
            .dispatch_status_menu_action(StatusMenuAction::SetPopoverPreviewMode(
                PopoverPreviewMode::CheatSheet,
            ))
            .expect("enable cheat sheet preview");

        let host_sheet = store
            .projection()
            .status_menu
            .cheat_sheet_preview
            .clone()
            .expect("host cheat sheet preview");
        assert_eq!(
            store.projection().status_menu.active_profile_name,
            "Antigravity"
        );
        assert_eq!(host_sheet.profile_name, "Antigravity");
        assert!(host_sheet
            .key_cells
            .iter()
            .any(|c| c.available && c.short_label != "未绑定"));

        let mut raw_demo = mi_model::demo().expect("demo config").raw;
        raw_demo["version"] = json!(1);
        let keymap_bytes = serde_json::to_vec(&raw_demo).expect("keymap bytes");
        let keymap_file = mi_model::operation_record::file_value(&keymap_bytes);
        let sha1 = keymap_file["sha1"].as_str().unwrap_or_default().to_string();
        let baseline = json!({
            "firmware": "0.6.3",
            "device_identity": {
                "confidence": "verified-serial",
                "serial": "creator-micro-2-local",
                "session_id": 1
            },
            "keymap": keymap_file,
            "smart_actions": { "state": "missing" },
            "files": [{
                "name": "keymap",
                "state": "present",
                "size": keymap_bytes.len(),
                "sha1": sha1
            }]
        });
        let device_cfg =
            mi_model::parse_device_files(&keymap_bytes, None).expect("parse device keymap");
        store
            .document_store
            .begin_device_editing(device_cfg, baseline)
            .expect("enter device editing");
        assert!(store.document_store.editing_device());
        store.commit_runtime_update();

        let device_editing_menu = &store.projection().status_menu;
        assert_eq!(device_editing_menu.active_profile_name, "Antigravity");
        assert_eq!(device_editing_menu.favorites_2x2[0].title, "Antigravity");
        assert!(device_editing_menu.favorites_2x2[0].is_active);
        assert_eq!(
            device_editing_menu.cheat_sheet_preview.as_ref(),
            Some(&host_sheet)
        );

        store.document_store.return_to_host();
        store
            .document_store
            .set_host_active_profile(None)
            .expect("clear active host profile");
        store.commit_runtime_update();

        assert_eq!(
            store.projection().status_menu.active_profile_name,
            "未选择方案"
        );
        assert!(store.projection().status_menu.cheat_sheet_preview.is_none());
    }
}
