use crate::karabiner::{self, ConfigPatch, Readiness, ReloadBaseline};
use crate::store::session_navigation::{
    NavigationCommand, NavigationDispatch, PhysicalNavigationIntent,
};
use mi_model::apply::AppliedSixLayerPlan;
use mi_model::{
    direct_controls::{CONTROL_BINDINGS, DIRECT_CONTROLS},
    host_router::ResolvedHostAction,
};
use mi_proto::lighting_candidate::OaiKeyStatusEntry;
use mi_session::DeviceSession;
use mi_transport::{HidCandidate, HidTransport, MacTransport, TargetKeyboard, UserCommandReceiver};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum InstallationStatus {
    Applying,
    Applied,
    RecoveryRequired,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Installation {
    pub status: InstallationStatus,
    pub plan: AppliedSixLayerPlan,
    pub target: TargetKeyboard,
    pub profile_index: u32,
    pub backup: PathBuf,
    pub config: ConfigPatch,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedSixLayerState {
    pub version: u8,
    #[serde(
        default,
        rename = "cindy_native_paused_confirmed",
        skip_serializing_if = "Option::is_none"
    )]
    legacy_native_paused_confirmation: Option<bool>,
    pub installation: Option<Installation>,
}

impl Default for SavedSixLayerState {
    fn default() -> Self {
        Self {
            version: 1,
            legacy_native_paused_confirmation: None,
            installation: None,
        }
    }
}

pub fn load(path: &Path) -> Result<SavedSixLayerState, String> {
    match fs::read(path) {
        Ok(bytes) => {
            let saved: SavedSixLayerState =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if saved.version != 1 {
                return Err("不支持此六层运行记录版本".into());
            }
            Ok(saved)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(SavedSixLayerState::default())
        }
        Err(error) => Err(error.to_string()),
    }
}

pub fn save(path: &Path, state: &SavedSixLayerState) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?;
    if let Ok(expected) = fs::read(path) {
        return karabiner::replace_if_unchanged(path, &expected, &bytes);
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        fs::File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[derive(Default)]
struct ExecutionPolicy {
    state: Mutex<ExecutionPolicyState>,
}

#[derive(Default)]
struct ExecutionPolicyState {
    enabled: bool,
    commands: bool,
    epoch: u64,
    admit_after: Option<Instant>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ExecutionAdmission {
    epoch: u64,
    received_at: Instant,
}

impl ExecutionPolicy {
    fn set(&self, enabled: bool, commands: bool, now: Instant) {
        let mut state = self.state.lock().unwrap();
        if state.enabled != enabled || state.commands != commands {
            state.enabled = enabled;
            state.commands = commands;
            state.epoch += 1;
            state.admit_after = Some(now);
        }
    }

    fn capture(&self, received_at: Instant) -> ExecutionAdmission {
        ExecutionAdmission {
            epoch: self.state.lock().unwrap().epoch,
            received_at,
        }
    }

    fn admits(&self, admission: ExecutionAdmission, action: &ResolvedHostAction) -> bool {
        let state = self.state.lock().unwrap();
        state.epoch == admission.epoch
            && state.enabled
            && state
                .admit_after
                .is_none_or(|after| admission.received_at >= after)
            && (!matches!(action, ResolvedHostAction::ShellCommand { .. }) || state.commands)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum WireSyncState {
    #[default]
    Unsynced,
    InFlight(String),
    Confirmed(String),
}

async fn open_vendor_session(
    transport: Arc<MacTransport>,
    target: TargetKeyboard,
) -> Option<Arc<DeviceSession>> {
    tokio::task::spawn_blocking(move || {
        mi_transport::discover_target_keyboard()
            .ok()
            .filter(|d| d.location_id == target.location_id)
            .and_then(|d| connect(transport.as_ref(), &d).ok())
            .map(|(session, _)| session)
    })
    .await
    .ok()
    .flatten()
}

const THSTATUS_RETRY_BACKOFF: Duration = Duration::from_secs(2);

async fn send_thstatus_bounded(
    session: &DeviceSession,
    sync_state: &Mutex<WireSyncState>,
    retry_not_before: &Mutex<Option<Instant>>,
    digest: String,
    entries: &[OaiKeyStatusEntry],
    session_ok: &mut bool,
) {
    *sync_state.lock().unwrap() = WireSyncState::InFlight(digest.clone());
    match tokio::time::timeout(
        Duration::from_millis(750),
        session.send_oai_thstatus(entries),
    )
    .await
    {
        Ok(Ok(_)) => {
            *sync_state.lock().unwrap() = WireSyncState::Confirmed(digest);
            *retry_not_before.lock().unwrap() = None;
        }
        Ok(Err(_)) | Err(_) => {
            *sync_state.lock().unwrap() = WireSyncState::Unsynced;
            if session.is_disconnected() {
                *session_ok = false;
                *retry_not_before.lock().unwrap() = None;
            } else {
                *retry_not_before.lock().unwrap() = Some(Instant::now() + THSTATUS_RETRY_BACKOFF);
            }
        }
    }
}

pub const SLOTS_PER_LAYER: usize = 20;
pub type HostAction = ResolvedHostAction;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedProfilePlan {
    pub profile_id: String,
    pub bundle_id: Option<String>,
    pub actions: [Option<HostAction>; SLOTS_PER_LAYER],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SixLayerRoutingMode {
    Locked(PinnedProfilePlan),
    ManualPinned(PinnedProfilePlan),
    AutoFollow {
        dynamic_hosts: Vec<PinnedProfilePlan>,
    },
}

impl Default for SixLayerRoutingMode {
    fn default() -> Self {
        Self::AutoFollow {
            dynamic_hosts: Vec::new(),
        }
    }
}

enum VendorSlotDecode {
    Edge {
        phys_slot: u8,
        control: Option<u8>,
        pressed: bool,
    },
    Step(u8),
    RadialSector(u8),
    RadialRelease,
}

fn handle_vendor_slot_notification(
    received: &mi_session::ReceivedNotification,
    pressed_mask: &mut u32,
    receiver: &UserCommandReceiver,
    layer1_fallback: bool,
) {
    match decode_six_layer_vendor_slot(&received.notification, layer1_fallback) {
        Some(VendorSlotDecode::Edge {
            phys_slot,
            control,
            pressed,
        }) => {
            let mask_bit = 1u32 << phys_slot;
            if pressed {
                if (*pressed_mask & mask_bit) == 0 {
                    *pressed_mask |= mask_bit;
                    if let Some(control) = control {
                        let _ = receiver.push_vendor_activation(control);
                    }
                }
            } else {
                *pressed_mask &= !mask_bit;
            }
        }
        Some(VendorSlotDecode::Step(control)) => {
            let _ = receiver.push_vendor_activation(control);
        }
        Some(VendorSlotDecode::RadialSector(slot)) => {
            let radial_bits = 0b1111u32 << 16;
            let bit = 1u32 << slot;
            if (*pressed_mask & radial_bits) == 0 {
                *pressed_mask |= bit;
                let _ = receiver.push_vendor_activation(slot);
            }
        }
        Some(VendorSlotDecode::RadialRelease) => {
            *pressed_mask &= !(0b1111u32 << 16);
        }
        None => {}
    }
}

struct VendorBridge {
    led_tx: tokio::sync::watch::Sender<Option<(String, Vec<OaiKeyStatusEntry>)>>,
    stop_tx: tokio::sync::watch::Sender<bool>,
    connected: Arc<AtomicBool>,
    sync_state: Arc<Mutex<WireSyncState>>,
    retry_not_before: Arc<Mutex<Option<Instant>>>,
    active_session: Arc<Mutex<Option<Arc<DeviceSession>>>>,
    retired_session: Mutex<Option<Arc<DeviceSession>>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl VendorBridge {
    fn new(initial_session: Option<Arc<DeviceSession>>) -> Self {
        let (led_tx, _) = tokio::sync::watch::channel(None);
        let (stop_tx, _) = tokio::sync::watch::channel(false);
        Self {
            led_tx,
            stop_tx,
            connected: Arc::new(AtomicBool::new(false)),
            sync_state: Arc::new(Mutex::new(WireSyncState::Unsynced)),
            retry_not_before: Arc::new(Mutex::new(None)),
            active_session: Arc::new(Mutex::new(initial_session)),
            retired_session: Mutex::new(None),
            task: None,
        }
    }

    fn confirmed_digest(&self) -> Option<String> {
        match &*self.sync_state.lock().unwrap() {
            WireSyncState::Confirmed(digest) => Some(digest.clone()),
            WireSyncState::Unsynced | WireSyncState::InFlight(_) => None,
        }
    }

    fn retry_ready(&self) -> bool {
        self.retry_not_before
            .lock()
            .unwrap()
            .is_none_or(|deadline| Instant::now() >= deadline)
    }

    fn needs_lighting_sync(&self) -> bool {
        if !self.connected.load(Ordering::Acquire) {
            return false;
        }
        let sync_state = self.sync_state.lock().unwrap().clone();
        let queued = self.led_tx.borrow().as_ref().map(|(d, _)| d.clone());
        match (queued, sync_state) {
            (None, _) => false,
            (Some(_), WireSyncState::Unsynced) => self.retry_ready(),
            (Some(_), WireSyncState::InFlight(_)) => false,
            (Some(wanted), WireSyncState::Confirmed(confirmed)) => wanted != confirmed,
        }
    }

    fn retire(&self) {
        let _ = self.stop_tx.send(true);
        self.connected.store(false, Ordering::Release);
        *self.sync_state.lock().unwrap() = WireSyncState::Unsynced;
        *self.retry_not_before.lock().unwrap() = None;
        if let Some(session) = self.active_session.lock().unwrap().take() {
            session.disconnect();
            *self.retired_session.lock().unwrap() = Some(session);
        }
    }

    fn shutdown(&mut self, runtime: &tokio::runtime::Runtime) {
        self.retire();
        if let Some(task) = self.task.take() {
            let _ = runtime.block_on(task);
        }
        if let Some(session) = self.retired_session.lock().unwrap().take() {
            runtime.block_on(session.shutdown());
        }
        if let Some(session) = self.active_session.lock().unwrap().take() {
            runtime.block_on(session.shutdown());
        }
    }

    fn spawn(
        &mut self,
        runtime: &tokio::runtime::Runtime,
        initial_session: Option<Arc<DeviceSession>>,
        receiver: Arc<UserCommandReceiver>,
        routing: Arc<Mutex<RoutingContext>>,
        target: TargetKeyboard,
        transport: Option<Arc<MacTransport>>,
        allow_reconnect: bool,
    ) {
        self.spawn_with_reopen(
            runtime,
            initial_session,
            receiver,
            routing,
            allow_reconnect,
            move || {
                let transport = transport.clone();
                let target = target.clone();
                async move { open_vendor_session(transport?, target).await }
            },
        );
    }

    fn spawn_with_reopen<F, Fut>(
        &mut self,
        runtime: &tokio::runtime::Runtime,
        initial_session: Option<Arc<DeviceSession>>,
        receiver: Arc<UserCommandReceiver>,
        routing: Arc<Mutex<RoutingContext>>,
        allow_reconnect: bool,
        mut reopen: F,
    ) where
        F: FnMut() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Option<Arc<DeviceSession>>> + Send,
    {
        if self.task.is_some() {
            self.shutdown(runtime);
        }
        if let Some(session) = self.retired_session.lock().unwrap().take() {
            runtime.block_on(session.shutdown());
        }
        let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
        self.stop_tx = stop_tx;
        *self.retry_not_before.lock().unwrap() = None;
        let mut led_rx = self.led_tx.subscribe();
        let initial_session = initial_session.filter(|s| !s.is_disconnected());
        let initial_notif_rx = if let Some(ref s) = initial_session {
            let rx = s.notifications();
            *self.active_session.lock().unwrap() = Some(Arc::clone(s));
            self.connected.store(true, Ordering::Release);
            Some(rx)
        } else {
            *self.active_session.lock().unwrap() = None;
            self.connected.store(false, Ordering::Release);
            None
        };
        let connected = Arc::clone(&self.connected);
        let sync_state = Arc::clone(&self.sync_state);
        let retry_not_before = Arc::clone(&self.retry_not_before);
        let active_session = Arc::clone(&self.active_session);
        self.task = Some(runtime.spawn(async move {
            let mut current_session = initial_session;
            let mut pending_notif_rx = initial_notif_rx;
            let mut pressed_mask: u32 = 0;
            let mut rotated_from_live = false;
            while !*stop_rx.borrow() {
                if current_session.is_none() {
                    pressed_mask = 0;
                    rotated_from_live = false;
                    connected.store(false, Ordering::Release);
                    *sync_state.lock().unwrap() = WireSyncState::Unsynced;
                    *retry_not_before.lock().unwrap() = None;
                    if !allow_reconnect {
                        return;
                    }
                    tokio::select! {
                        _ = stop_rx.changed() => return,
                        _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                    }
                    if *stop_rx.borrow() || receiver.is_retired() {
                        return;
                    }
                    let Some(session) = reopen().await else {
                        continue;
                    };
                    if *stop_rx.borrow() || receiver.is_retired() {
                        session.shutdown().await;
                        return;
                    }
                    pending_notif_rx = Some(session.notifications());
                    *active_session.lock().unwrap() = Some(Arc::clone(&session));
                    connected.store(true, Ordering::Release);
                    current_session = Some(session);
                }
                let session = current_session.take().unwrap();
                let mut notif_rx = pending_notif_rx
                    .take()
                    .unwrap_or_else(|| session.notifications());
                connected.store(true, Ordering::Release);
                let mut session_ok = !session.is_disconnected();
                let needs_initial_flush = !rotated_from_live
                    || matches!(*sync_state.lock().unwrap(), WireSyncState::Unsynced);
                if session_ok && needs_initial_flush {
                    let initial_payload = { led_rx.borrow_and_update().clone() };
                    if let Some((digest, entries)) = initial_payload {
                        send_thstatus_bounded(
                            &session,
                            &sync_state,
                            &retry_not_before,
                            digest,
                            &entries,
                            &mut session_ok,
                        )
                        .await;
                    }
                }
                let mut next_rotated = None;
                let mut already_shut_down = false;
                while session_ok && !*stop_rx.borrow() {
                    if allow_reconnect && session.request_count() >= 900 {
                        while let Ok(received) = notif_rx.try_recv() {
                            let layer1_fallback =
                                routing.lock().unwrap().accepts_layer1_vendor_fallback();
                            handle_vendor_slot_notification(
                                &received,
                                &mut pressed_mask,
                                &receiver,
                                layer1_fallback,
                            );
                        }
                        pressed_mask = 0;
                        let _ = active_session.lock().unwrap().take();
                        session.shutdown().await;
                        already_shut_down = true;
                        if *stop_rx.borrow() || receiver.is_retired() {
                            break;
                        }
                        if let Some(new_session) = reopen().await {
                            if *stop_rx.borrow() || receiver.is_retired() {
                                new_session.shutdown().await;
                                break;
                            }
                            let new_notif_rx = new_session.notifications();
                            *active_session.lock().unwrap() = Some(Arc::clone(&new_session));
                            connected.store(true, Ordering::Release);
                            next_rotated = Some((new_session, new_notif_rx));
                        }
                        break;
                    }
                    tokio::select! {
                        _ = stop_rx.changed() => break,
                        _ = session.wait_disconnected() => {
                            session_ok = false;
                        }
                        notif = notif_rx.recv() => {
                            match notif {
                                Ok(received) => {
                                    let layer1_fallback =
                                        routing.lock().unwrap().accepts_layer1_vendor_fallback();
                                    handle_vendor_slot_notification(
                                        &received,
                                        &mut pressed_mask,
                                        &receiver,
                                        layer1_fallback,
                                    );
                                }
                                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                    pressed_mask = 0;
                                }
                                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                                    session_ok = false;
                                }
                            }
                        }
                        changed = led_rx.changed() => {
                            if changed.is_err() {
                                break;
                            }
                            let next_payload = { led_rx.borrow_and_update().clone() };
                            if let Some((digest, entries)) = next_payload {
                                send_thstatus_bounded(
                                    &session,
                                    &sync_state,
                                    &retry_not_before,
                                    digest,
                                    &entries,
                                    &mut session_ok,
                                )
                                .await;
                            }
                        }
                    }
                }
                if let Some((new_session, new_notif_rx)) = next_rotated {
                    current_session = Some(new_session);
                    pending_notif_rx = Some(new_notif_rx);
                    rotated_from_live = true;
                    continue;
                }
                connected.store(false, Ordering::Release);
                *sync_state.lock().unwrap() = WireSyncState::Unsynced;
                *retry_not_before.lock().unwrap() = None;
                let _ = active_session.lock().unwrap().take();
                if !already_shut_down {
                    session.shutdown().await;
                }
                if !*stop_rx.borrow() && allow_reconnect {
                    receiver.invalidate("USB 独占会话已失效，输入已暂停");
                }
                return;
            }
        }));
    }
}

fn decode_six_layer_vendor_slot(
    notification: &mi_proto::HostNotification,
    layer1_fallback: bool,
) -> Option<VendorSlotDecode> {
    let mi_proto::HostNotification::Unknown(raw) = notification else {
        return None;
    };
    if ["id", "i", "result", "error"]
        .iter()
        .any(|key| raw.get(key).is_some())
    {
        return None;
    }
    if raw.get("method").is_some() && raw.get("m").is_some() {
        return None;
    }
    if raw.get("params").is_some() && raw.get("p").is_some() {
        return None;
    }
    let method = raw.get("method").or_else(|| raw.get("m"))?.as_str()?;
    let params = raw.get("params").or_else(|| raw.get("p"))?.as_object()?;
    match method {
        "v.oai.hid" => {
            let key = params.get("k")?.as_str()?;
            let act = params.get("act")?.as_i64()?;
            if let Some(digits) = key.strip_prefix("AG") {
                if digits.len() != 2 || !digits.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                let pressed = match act {
                    1 => true,
                    0 => false,
                    _ => return None,
                };
                let ag = digits.parse::<u8>().ok()?;
                return match ag {
                    0..=5 => Some(VendorSlotDecode::Edge {
                        phys_slot: ag,
                        control: layer1_fallback.then_some(ag),
                        pressed,
                    }),
                    6..=11 => Some(VendorSlotDecode::Edge {
                        phys_slot: ag - 6,
                        control: Some(ag - 6),
                        pressed,
                    }),
                    12..=17 => Some(VendorSlotDecode::Edge {
                        phys_slot: ag - 12,
                        control: Some(20 + (ag - 12)),
                        pressed,
                    }),
                    _ => None,
                };
            }
            if !layer1_fallback {
                return None;
            }
            if let Some(digits) = key.strip_prefix("ACT") {
                if digits.len() == 2 && digits.bytes().all(|b| b.is_ascii_digit()) {
                    let slot = digits.parse::<u8>().ok()?;
                    if (6..=12).contains(&slot) {
                        let pressed = match act {
                            1 => true,
                            0 => false,
                            _ => return None,
                        };
                        return Some(VendorSlotDecode::Edge {
                            phys_slot: slot,
                            control: Some(slot),
                            pressed,
                        });
                    }
                }
                return None;
            }
            match (key, act) {
                ("ENC_CC", 2) => Some(VendorSlotDecode::Step(13)),
                ("ENC_CW", 2) => Some(VendorSlotDecode::Step(14)),
                ("ENC_CC", 1) => Some(VendorSlotDecode::Edge {
                    phys_slot: 13,
                    control: Some(13),
                    pressed: true,
                }),
                ("ENC_CC", 0) => Some(VendorSlotDecode::Edge {
                    phys_slot: 13,
                    control: Some(13),
                    pressed: false,
                }),
                ("ENC_CW", 1) => Some(VendorSlotDecode::Edge {
                    phys_slot: 14,
                    control: Some(14),
                    pressed: true,
                }),
                ("ENC_CW", 0) => Some(VendorSlotDecode::Edge {
                    phys_slot: 14,
                    control: Some(14),
                    pressed: false,
                }),
                ("ENC_CLK", 1) => Some(VendorSlotDecode::Edge {
                    phys_slot: 15,
                    control: Some(15),
                    pressed: true,
                }),
                ("ENC_CLK", 0) => Some(VendorSlotDecode::Edge {
                    phys_slot: 15,
                    control: Some(15),
                    pressed: false,
                }),
                _ => None,
            }
        }
        "v.oai.rad" if layer1_fallback => {
            let angle = params.get("a")?.as_f64()?;
            let distance = params.get("d")?.as_f64()?;
            if !angle.is_finite()
                || !distance.is_finite()
                || !(0.0..=1.0).contains(&angle)
                || !(0.0..=1.0).contains(&distance)
            {
                return None;
            }
            if distance >= 0.5 {
                let norm = angle.rem_euclid(1.0);
                let slot = if (0.625..0.875).contains(&norm) {
                    16
                } else if (0.375..0.625).contains(&norm) {
                    17
                } else if (0.125..0.375).contains(&norm) {
                    18
                } else {
                    19
                };
                Some(VendorSlotDecode::RadialSector(slot))
            } else if distance < 0.35 {
                Some(VendorSlotDecode::RadialRelease)
            } else {
                None
            }
        }
        _ => None,
    }
}

pub struct RunningSixLayer {
    pub receiver: Arc<UserCommandReceiver>,
    pub readiness: Readiness,
    pub candidate: HidCandidate,
    pub installation: Installation,
    pub observed_layer: Arc<AtomicU8>,
    pub first_event_verified: Arc<AtomicBool>,
    pub armed: bool,
    execution_policy: Arc<ExecutionPolicy>,
    pub receipts: Option<std::sync::mpsc::Receiver<mi_macos::HostActionReceipt>>,
    routing: Arc<Mutex<RoutingContext>>,
    pub(crate) navigation: Option<NavigationDispatch>,
    vendor_bridge: VendorBridge,
    transport: Option<Arc<MacTransport>>,
    stop: Arc<AtomicBool>,
    monitor: Option<JoinHandle<()>>,
    executor: Option<JoinHandle<Result<(), String>>>,
}

#[derive(Clone, Debug)]
pub(crate) struct RoutingContext {
    pub mode: SixLayerRoutingMode,
    pub bundle_id: Option<String>,
    pub generation: u64,
    pub changed_at: Instant,
}

impl Default for RoutingContext {
    fn default() -> Self {
        Self {
            mode: SixLayerRoutingMode::default(),
            bundle_id: None,
            generation: 0,
            changed_at: Instant::now(),
        }
    }
}

impl RoutingContext {
    fn update(&mut self, bundle_id: Option<&str>) -> bool {
        if self.bundle_id.as_deref() == bundle_id {
            return false;
        }
        self.bundle_id = bundle_id.map(str::to_owned);
        match self.mode {
            SixLayerRoutingMode::Locked(_) | SixLayerRoutingMode::ManualPinned(_) => false,
            SixLayerRoutingMode::AutoFollow { .. } => {
                self.generation += 1;
                self.changed_at = Instant::now();
                true
            }
        }
    }

    fn set_routing(&mut self, mode: SixLayerRoutingMode, bundle_id: Option<&str>) -> bool {
        let mut changed = false;
        if self.bundle_id.as_deref() != bundle_id {
            self.bundle_id = bundle_id.map(str::to_owned);
            if matches!(mode, SixLayerRoutingMode::AutoFollow { .. }) {
                self.generation += 1;
                self.changed_at = Instant::now();
                changed = true;
            }
        }
        if self.mode != mode {
            if matches!(mode, SixLayerRoutingMode::AutoFollow { .. })
                && !matches!(self.mode, SixLayerRoutingMode::AutoFollow { .. })
            {
                self.generation += 1;
                self.changed_at = Instant::now();
            }
            self.mode = mode;
            changed = true;
        }
        changed
    }

    fn accepts_layer1_vendor_fallback(&self) -> bool {
        match &self.mode {
            SixLayerRoutingMode::Locked(pinned) | SixLayerRoutingMode::ManualPinned(pinned) => {
                pinned.actions.iter().any(Option::is_some)
            }
            SixLayerRoutingMode::AutoFollow { dynamic_hosts } => dynamic_hosts.iter().any(|host| {
                host.bundle_id.as_deref() == self.bundle_id.as_deref()
                    && host.actions.iter().any(Option::is_some)
            }),
        }
    }

    fn effective_profile_id(
        &self,
        plan: &AppliedSixLayerPlan,
        physical_layer: u8,
    ) -> Option<String> {
        match &self.mode {
            SixLayerRoutingMode::Locked(pinned) => Some(pinned.profile_id.clone()),
            SixLayerRoutingMode::ManualPinned(pinned) => {
                if pinned.bundle_id.as_deref() == Some("com.openai.codex")
                    && pinned.actions.iter().all(Option::is_none)
                {
                    None
                } else {
                    Some(pinned.profile_id.clone())
                }
            }
            SixLayerRoutingMode::AutoFollow { dynamic_hosts } => {
                if !(1..=3).contains(&physical_layer) {
                    return None;
                }
                if let Some(bundle) = self.bundle_id.as_deref() {
                    if let Some(host) = plan.hosts.iter().find(|host| {
                        mi_macos::executor::bundle_id_for_adapter(&host.adapter) == Some(bundle)
                    }) {
                        return Some(host.profile_id.clone());
                    }
                    if let Some(dyn_host) = dynamic_hosts
                        .iter()
                        .find(|h| h.bundle_id.as_deref() == Some(bundle))
                    {
                        return Some(dyn_host.profile_id.clone());
                    }
                }
                plan.hosts
                    .get(physical_layer.checked_sub(2)? as usize)
                    .map(|h| h.profile_id.clone())
            }
        }
    }

    fn foreground_priority_adapter(
        &self,
        plan: &AppliedSixLayerPlan,
        physical_layer: u8,
    ) -> Option<String> {
        if !matches!(self.mode, SixLayerRoutingMode::AutoFollow { .. })
            || !(1..=3).contains(&physical_layer)
        {
            return None;
        }
        let bundle = self.bundle_id.as_deref()?;
        if let Some(host) = plan
            .hosts
            .iter()
            .find(|host| mi_macos::executor::bundle_id_for_adapter(&host.adapter) == Some(bundle))
        {
            return Some(host.adapter.clone());
        }
        if let SixLayerRoutingMode::AutoFollow { dynamic_hosts } = &self.mode {
            if dynamic_hosts
                .iter()
                .any(|h| h.bundle_id.as_deref() == Some(bundle))
            {
                return match bundle {
                    "com.openai.codex" => Some("codex".into()),
                    "com.google.antigravity" => Some("antigravity".into()),
                    "com.deepseek.dsh" => Some("dsh-desktop".into()),
                    "dev.herdr.app" => Some("herdr".into()),
                    _ => None,
                };
            }
        }
        None
    }

    fn resolve_control<'a>(
        &'a self,
        plan: &'a AppliedSixLayerPlan,
        control_layer: u8,
        slot: usize,
    ) -> Option<(&'a str, &'a ResolvedHostAction)> {
        match &self.mode {
            SixLayerRoutingMode::Locked(pinned) | SixLayerRoutingMode::ManualPinned(pinned) => {
                if !(1..=4).contains(&control_layer) {
                    return None;
                }
                let action = pinned.actions.get(slot)?.as_ref()?;
                Some((pinned.profile_id.as_str(), action))
            }
            SixLayerRoutingMode::AutoFollow { dynamic_hosts } => {
                if !(1..=3).contains(&control_layer) {
                    return None;
                }
                if let Some(bundle) = self.bundle_id.as_deref() {
                    if let Some(host) = plan.hosts.iter().find(|host| {
                        mi_macos::executor::bundle_id_for_adapter(&host.adapter) == Some(bundle)
                    }) {
                        let action = host.actions.get(slot)?.as_ref()?;
                        return Some((host.profile_id.as_str(), action));
                    }
                    if let Some(dyn_host) = dynamic_hosts
                        .iter()
                        .find(|h| h.bundle_id.as_deref() == Some(bundle))
                    {
                        let action = dyn_host.actions.get(slot)?.as_ref()?;
                        return Some((dyn_host.profile_id.as_str(), action));
                    }
                }
                let host = plan.hosts.get(control_layer.checked_sub(2)? as usize)?;
                let action = host.actions.get(slot)?.as_ref()?;
                Some((host.profile_id.as_str(), action))
            }
        }
    }

    #[cfg(test)]
    fn host_index(&self, plan: &AppliedSixLayerPlan, physical_layer: u8) -> usize {
        if let Some(bundle) = self.bundle_id.as_deref() {
            if let Some(index) = plan.hosts.iter().position(|host| {
                mi_macos::executor::bundle_id_for_adapter(&host.adapter) == Some(bundle)
            }) {
                return index;
            }
        }
        (physical_layer.saturating_sub(2)) as usize
    }
}

impl RunningSixLayer {
    pub fn foreground_changed(&self, bundle_id: Option<&str>) -> bool {
        self.routing.lock().unwrap().update(bundle_id)
    }

    pub fn set_routing_context(&self, mode: SixLayerRoutingMode, bundle_id: Option<&str>) -> bool {
        self.routing.lock().unwrap().set_routing(mode, bundle_id)
    }

    pub fn routing_mode(&self) -> SixLayerRoutingMode {
        self.routing.lock().unwrap().mode.clone()
    }

    pub fn foreground_priority_adapter(&self) -> Option<String> {
        let layer = self.observed_layer.load(Ordering::Acquire);
        self.routing
            .lock()
            .unwrap()
            .foreground_priority_adapter(&self.installation.plan, layer)
    }

    pub fn effective_profile_id(&self) -> Option<String> {
        let layer = self.observed_layer.load(Ordering::Acquire);
        self.routing
            .lock()
            .unwrap()
            .effective_profile_id(&self.installation.plan, layer)
    }

    #[cfg(test)]
    pub(crate) fn isolated(session: Arc<DeviceSession>, directory: &Path) -> Self {
        let mut draft = mi_model::draft::Draft::default();
        mi_model::preset::install_agent_starter_profiles(&mut draft).unwrap();
        let id = |name: &str| {
            draft
                .document
                .editable
                .graph
                .profiles
                .iter()
                .find(|(_, p)| p.name == name)
                .unwrap()
                .0
                .clone()
        };
        let desired = mi_model::board_layers::BoardLayerPlan::recommended(
            id("Codex"),
            id("Antigravity"),
            id("DeepSeek Coder (DSH)"),
        );
        let plan = AppliedSixLayerPlan::compile(
            &desired,
            &draft.document,
            mi_model::host_router::BottomRowLayout::Split,
            true,
        )
        .unwrap();
        let candidate = HidCandidate {
            path: "isolated".into(),
            vid: 0x303a,
            pid: 0x8298,
            product: "Synthetic".into(),
            usage_page: 0xff60,
            usage: 0x61,
            usage_pairs: vec![],
            transport_hint: mi_transport::TransportHint::Usb,
        };
        new_running(
            Arc::new(UserCommandReceiver::bind().unwrap()),
            Readiness::isolated(),
            Some(session),
            candidate,
            Installation {
                status: InstallationStatus::Applied,
                plan,
                target: TargetKeyboard {
                    vendor_id: 0x303a,
                    product_id: 0x8298,
                    location_id: 0,
                    usb_parent: 0,
                    vendor_path: "isolated".into(),
                },
                profile_index: 0,
                backup: directory.join("backup"),
                config: ConfigPatch {
                    path: directory.join("karabiner.json"),
                    original: vec![],
                    installed: vec![],
                    profile_name: "isolated".into(),
                    original_device: None,
                    installed_device: serde_json::Value::Null,
                },
            },
            1,
        )
    }

    pub fn set_execution_policy(&self, enabled: bool, commands: bool, now: Instant) {
        self.execution_policy.set(enabled, commands, now);
    }

    pub(crate) fn admits_navigation(&self, admission: ExecutionAdmission) -> bool {
        let state = self.execution_policy.state.lock().unwrap();
        state.epoch == admission.epoch
            && state.enabled
            && state
                .admit_after
                .is_none_or(|after| admission.received_at >= after)
    }

    pub fn vendor_bridge_connected(&self) -> bool {
        self.vendor_bridge.connected.load(Ordering::Acquire)
    }

    pub fn confirmed_lighting_digest(&self) -> Option<String> {
        self.vendor_bridge.confirmed_digest()
    }

    pub fn vendor_bridge_needs_lighting_sync(&self) -> bool {
        self.vendor_bridge.needs_lighting_sync()
    }

    pub fn queue_session_lighting(
        &self,
        digest: &str,
        entries: &[OaiKeyStatusEntry],
    ) -> Option<String> {
        let connected = self.vendor_bridge.connected.load(Ordering::Acquire);
        let sync_state = self.vendor_bridge.sync_state.lock().unwrap().clone();
        let retry_ready = self.vendor_bridge.retry_ready();
        let confirmed = match &sync_state {
            WireSyncState::Confirmed(d) => Some(d.clone()),
            WireSyncState::Unsynced | WireSyncState::InFlight(_) => None,
        };
        self.vendor_bridge.led_tx.send_if_modified(|current| {
            if current.as_ref().map(|(d, _)| d.as_str()) != Some(digest) {
                *current = Some((digest.to_owned(), entries.to_vec()));
                true
            } else {
                connected && sync_state == WireSyncState::Unsynced && retry_ready
            }
        });
        confirmed
    }

    pub fn start_vendor_bridge(&mut self, runtime: &tokio::runtime::Runtime) {
        let initial = self.vendor_bridge.active_session.lock().unwrap().take();
        self.vendor_bridge.spawn(
            runtime,
            initial,
            Arc::clone(&self.receiver),
            Arc::clone(&self.routing),
            self.installation.target.clone(),
            self.transport.clone(),
            true,
        );
    }

    pub fn attach_test_vendor_session(
        &mut self,
        runtime: &tokio::runtime::Runtime,
        session: Arc<DeviceSession>,
    ) {
        self.vendor_bridge.spawn(
            runtime,
            Some(session),
            Arc::clone(&self.receiver),
            Arc::clone(&self.routing),
            self.installation.target.clone(),
            None,
            false,
        );
    }

    #[cfg(test)]
    pub(crate) fn is_execution_enabled(&self) -> bool {
        self.execution_policy.state.lock().unwrap().enabled
    }

    pub fn start_executor<D: mi_macos::HostOsDriver + 'static>(
        &mut self,
        executor: Arc<mi_macos::LiveHostActionExecutor<D>>,
    ) {
        let receiver = Arc::clone(&self.receiver);
        let plan = self.installation.plan.clone();
        let stop = Arc::clone(&self.stop);
        let policy = Arc::clone(&self.execution_policy);
        let observed = Arc::clone(&self.observed_layer);
        let first = Arc::clone(&self.first_event_verified);
        let routing = Arc::clone(&self.routing);
        let navigation = self.navigation.clone();
        let transport = self.transport.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(32);
        self.receipts = Some(rx);
        self.executor = Some(thread::spawn(move || {
            execute_clicks(
                &receiver,
                &plan,
                &stop,
                &policy,
                &observed,
                &first,
                &routing,
                &tx,
                navigation.as_ref(),
                || receiver.next(),
                || {
                    if transport
                        .as_ref()
                        .is_some_and(|transport| !transport.has_vendor_lease())
                    {
                        return Err("USB 专用接口独占已丢失".into());
                    }
                    receiver.failure().map_or(Ok(()), Err)
                },
                |profile, binding, action, admission| {
                    executor.execute_when(profile, binding, action, || {
                        !stop.load(Ordering::Acquire)
                            && receiver.failure().is_none()
                            && transport
                                .as_ref()
                                .is_none_or(|transport| transport.has_vendor_lease())
                            && policy.admits(admission, action)
                    })
                },
            );
            Ok(())
        }));
    }

    pub fn retire(&self) {
        self.receiver.invalidate("运行代次已结束");
        self.stop.store(true, Ordering::Release);
        self.vendor_bridge.retire();
    }

    pub fn is_retired(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }

    pub fn shutdown(mut self, runtime: &tokio::runtime::Runtime) -> Result<(), String> {
        self.quiesce(runtime)
    }

    pub fn quiesce(&mut self, runtime: &tokio::runtime::Runtime) -> Result<(), String> {
        self.retire();
        self.vendor_bridge.shutdown(runtime);
        let monitor_result = self.monitor.take().map_or(Ok(()), |monitor| {
            monitor.join().map_err(|_| "设备监视清理失败".to_string())
        });
        let executor_result = self.executor.take().map_or(Ok(()), |executor| {
            executor
                .join()
                .map_err(|_| "宿主动作清理失败".to_string())
                .and_then(|result| result)
        });
        monitor_result.and(executor_result)
    }

    pub fn start_monitor(&mut self) {
        let receiver = Arc::clone(&self.receiver);
        let stop = Arc::clone(&self.stop);
        let patch_path = self.installation.config.path.clone();
        let endpoint_marker = self.receiver.path().to_string_lossy().into_owned();
        let target = self.installation.target.clone();
        self.monitor = Some(thread::spawn(move || {
            let mut wall = SystemTime::now();
            let mut monotonic = Instant::now();
            let mut last_patch_stamp: Option<(Option<SystemTime>, u64)> = None;
            while !stop.load(Ordering::Acquire) {
                for _ in 0..10 {
                    if stop.load(Ordering::Acquire) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(200));
                }
                let now_wall = SystemTime::now();
                let now_monotonic = Instant::now();
                let slept = now_wall
                    .duration_since(wall)
                    .unwrap_or_default()
                    .saturating_sub(now_monotonic.duration_since(monotonic))
                    > Duration::from_secs(1);
                wall = now_wall;
                monotonic = now_monotonic;
                if !check_input_connection(&receiver, slept, || {
                    steady_state_recheck(
                        &target,
                        receiver.path(),
                        &patch_path,
                        &endpoint_marker,
                        &mut last_patch_stamp,
                    )
                }) {
                    return;
                }
            }
        }));
    }
}

fn steady_state_recheck(
    target: &TargetKeyboard,
    receiver_path: &Path,
    patch_path: &Path,
    endpoint_marker: &str,
    last_patch_stamp: &mut Option<(Option<SystemTime>, u64)>,
) -> Result<(), String> {
    match mi_transport::discover_target_keyboard() {
        Ok(found) => {
            if found.location_id != target.location_id {
                return Err("键盘 USB 端口位置已变更，等待自动重连".into());
            }
            if found != *target {
                return Err("目标 USB 设备已变化".into());
            }
        }
        Err(_) => return Err("目标 USB 设备已变化".into()),
    }
    if !receiver_path.exists() {
        return Err("输入接收器已断开".into());
    }
    let meta = fs::metadata(patch_path).map_err(|_| "Karabiner 配置已变化".to_string())?;
    let stamp = (meta.modified().ok(), meta.len());
    if last_patch_stamp.as_ref() != Some(&stamp) {
        let bytes = fs::read(patch_path).map_err(|_| "Karabiner 配置已变化".to_string())?;
        let text = String::from_utf8_lossy(&bytes);
        if !text.contains(endpoint_marker) {
            return Err("Karabiner 配置已变化".into());
        }
        *last_patch_stamp = Some(stamp);
    }
    Ok(())
}

impl Drop for RunningSixLayer {
    fn drop(&mut self) {
        self.retire();
        // The monitor only owns immutable evidence and its old receiver, so it
        // may finish a bounded CLI check without delaying the workbench thread.
        self.monitor.take();
        self.executor.take();
    }
}

fn check_input_connection(
    receiver: &UserCommandReceiver,
    slept: bool,
    recheck: impl FnOnce() -> Result<(), String>,
) -> bool {
    let result = if slept {
        Err("检测到系统睡眠，六层输入已暂停".into())
    } else {
        recheck()
    };
    if let Err(error) = result {
        receiver.invalidate(&error);
        false
    } else {
        true
    }
}

fn execute_clicks(
    receiver: &UserCommandReceiver,
    plan: &AppliedSixLayerPlan,
    stop: &AtomicBool,
    policy: &ExecutionPolicy,
    observed: &AtomicU8,
    first: &AtomicBool,
    routing: &Mutex<RoutingContext>,
    receipts: &std::sync::mpsc::SyncSender<mi_macos::HostActionReceipt>,
    navigation: Option<&NavigationDispatch>,
    mut next: impl FnMut() -> Result<Option<mi_transport::Activation>, String>,
    mut check_connection: impl FnMut() -> Result<(), String>,
    mut execute: impl FnMut(
        &str,
        &str,
        &ResolvedHostAction,
        ExecutionAdmission,
    ) -> mi_macos::HostActionReceipt,
) {
    while !stop.load(Ordering::Acquire) {
        let event = match next() {
            Ok(Some(event)) => event,
            Ok(None) => {
                thread::sleep(Duration::from_millis(5));
                continue;
            }
            Err(_) => return,
        };
        let admission = policy.capture(event.received_at);
        if event.generation != receiver.generation() {
            continue;
        }
        let control = &DIRECT_CONTROLS[event.control as usize];
        observed.store(control.layer, Ordering::Release);
        first.store(true, Ordering::Release);
        let context = routing.lock().unwrap().clone();
        let is_auto_follow = matches!(context.mode, SixLayerRoutingMode::AutoFollow { .. });
        if is_auto_follow && context.generation != 0 && event.received_at < context.changed_at {
            continue;
        }
        let Some((profile_id, action)) =
            context.resolve_control(plan, control.layer, control.slot as usize)
        else {
            continue;
        };
        if !policy.admits(admission, action) {
            continue;
        }
        if let Err(error) = check_connection() {
            receiver.invalidate(&error);
            return;
        }
        if stop.load(Ordering::Acquire) || receiver.failure().is_some() {
            return;
        }
        if event.is_expired_at(Instant::now()) {
            continue;
        }
        if is_auto_follow && routing.lock().unwrap().generation != context.generation {
            continue;
        }
        if let (Some(command), Some(dispatch)) =
            (NavigationCommand::from_action(action), navigation)
        {
            if !policy.admits(admission, action) {
                continue;
            }
            let snapshot = dispatch.slots.lock().unwrap().clone();
            if dispatch
                .tx
                .try_send(PhysicalNavigationIntent {
                    command,
                    revision: snapshot.revision,
                    input_generation: event.generation.clone(),
                    admission,
                    received_at: event.received_at,
                    profile: profile_id.to_owned(),
                    binding: CONTROL_BINDINGS[control.slot as usize].into(),
                })
                .is_err()
            {
                receiver.invalidate("会话导航积压，输入已暂停");
                return;
            }
            continue;
        }
        let receipt = execute(
            profile_id,
            CONTROL_BINDINGS[control.slot as usize],
            action,
            admission,
        );
        if receipts.try_send(receipt).is_err() {
            receiver.invalidate("执行回执积压，输入已暂停");
            return;
        }
    }
}

fn connect(
    transport: &dyn HidTransport,
    target: &TargetKeyboard,
) -> Result<(Arc<DeviceSession>, HidCandidate), String> {
    let candidates = transport.enumerate().map_err(|e| e.to_string())?;
    let candidate = candidates
        .into_iter()
        .find(|candidate| {
            candidate.path == target.vendor_path
                && mi_transport::is_exclusive_vendor_candidate(candidate)
        })
        .ok_or("已确认的专用接口已消失")?;
    let link = transport.open(&candidate.path).map_err(|e| e.to_string())?;
    if !link.is_exclusive_vendor() {
        link.close();
        return Err("未取得 USB 专用接口独占".into());
    }
    let session = DeviceSession::new(
        link,
        Box::new(mi_proto::NativeFramer::default()),
        Arc::new(mi_session::ReadOnlySchema),
        mi_session::SessionConfig::default(),
    );
    Ok((session, candidate))
}

fn prepare_apply_karabiner_patch(
    saved: &SavedSixLayerState,
    target: &TargetKeyboard,
    receiver: &UserCommandReceiver,
    layout: mi_model::host_router::BottomRowLayout,
) -> Result<ConfigPatch, String> {
    let rule = karabiner::rules(target, receiver, layout)?;
    let path = match &saved.installation {
        Some(prev) => prev.config.path.clone(),
        None => karabiner::config_path()?,
    };
    let patch = karabiner::prepare_config(
        path.clone(),
        fs::read(&path).map_err(|e| e.to_string())?,
        target,
        rule,
    )?;
    if let Some(prev) = &saved.installation {
        if patch.profile_name != prev.config.profile_name {
            return Err("原 Karabiner 方案身份已变化；请先恢复原键盘配置后再应用".into());
        }
    }
    Ok(patch)
}

pub async fn apply(
    document_dir: &Path,
    plan: AppliedSixLayerPlan,
    mut saved: SavedSixLayerState,
    transport: Arc<MacTransport>,
) -> Result<RunningSixLayer, String> {
    let state_path = document_dir.join("six-layer-runtime.json");
    plan.validate_current()?;
    let target = mi_transport::discover_target_keyboard().map_err(|e| e.to_string())?;
    let receiver = Arc::new(UserCommandReceiver::bind_default().map_err(|e| e.to_string())?);
    let patch = prepare_apply_karabiner_patch(&saved, &target, &receiver, plan.bottom_row_layout)?;
    let (session, candidate) = connect(transport.as_ref(), &target)?;
    let outcome: Result<RunningSixLayer, String> = async {
        let prepared = session
            .prepare_six_layer_write(document_dir)
            .await
            .map_err(|e| e.to_string())?;
        let candidate_bytes = prepared.candidate.clone();
        let smart = match fs::read(prepared.backup.join("smart_actions.json")) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.to_string()),
        };
        let baseline = ReloadBaseline::capture()?;
        patch.install(&document_dir.join("karabiner-backups"))?;
        let readiness = match wait_ready(&baseline, &patch, &target) {
            Ok(ready) => ready,
            Err(error) => {
                let rollback = patch.rollback_exact();
                return Err(format!(
                    "{error}；规则恢复：{}",
                    rollback.map(|_| "已确认".into()).unwrap_or_else(|e| e)
                ));
            }
        };
        let previous = saved.clone();
        let mut restoration_config = patch.clone();
        if let Some(prev) = &previous.installation {
            restoration_config.original = prev.config.original.clone();
            restoration_config.original_device = prev.config.original_device.clone();
            if let Err(error) = merge_previous_original_into_backup(&prev.backup, &prepared.backup)
            {
                let _ = patch.rollback_exact();
                let _ = fs::remove_dir_all(&prepared.backup);
                return Err(error);
            }
        }
        let backup = prepared.backup.clone();
        let mut installation = Installation {
            status: InstallationStatus::Applying,
            plan,
            target: target.clone(),
            profile_index: prepared.profile_index,
            backup,
            config: restoration_config,
        };
        saved.installation = Some(installation.clone());
        if let Err(error) = save(&state_path, &saved) {
            let _ = patch.rollback_exact();
            if previous
                .installation
                .as_ref()
                .is_none_or(|prev| prev.backup != installation.backup)
            {
                let _ = fs::remove_dir_all(&installation.backup);
            }
            return Err(error);
        }
        let result = session.apply_six_layer_write(prepared).await;
        let receipt = match result {
            Ok(receipt) => receipt,
            Err(error) => {
                if error.original_verified {
                    let rollback = patch.rollback_exact();
                    if rollback.is_ok() && save(&state_path, &previous).is_ok() {
                        if previous
                            .installation
                            .as_ref()
                            .is_none_or(|prev| prev.backup != installation.backup)
                        {
                            let _ = fs::remove_dir_all(&installation.backup);
                        }
                    }
                    return Err(format!(
                        "{}；设备原件已确认；规则恢复：{}",
                        error.detail,
                        rollback.map(|_| "已确认".into()).unwrap_or_else(|e| e)
                    ));
                }
                if let Some(err_backup) = error.backup {
                    let merged_ok = previous.installation.as_ref().map_or(true, |prev| {
                        merge_previous_original_into_backup(&prev.backup, &err_backup).is_ok()
                    });
                    if merged_ok {
                        installation.backup = err_backup;
                    }
                }
                installation.status = InstallationStatus::RecoveryRequired;
                saved.installation = Some(installation);
                let _ = save(&state_path, &saved);
                return Err(format!(
                    "{}；设备恢复未确认，保留吞键规则，停止派发",
                    error.detail
                ));
            }
        };
        installation.backup = receipt.backup.clone();
        installation.status = InstallationStatus::RecoveryRequired;
        saved.installation = Some(installation.clone());
        save(&state_path, &saved)?;
        let layer = session
            .verify_six_layer_files(&candidate_bytes, smart.as_deref(), receipt.profile_index)
            .await
            .map_err(|e| format!("{e}；保留吞键规则，请恢复设备"))?;
        readiness.recheck(&patch, &target)?;
        installation.status = InstallationStatus::Applied;
        saved.installation = Some(installation.clone());
        save(&state_path, &saved)?;
        if let Some(prev) = &previous.installation {
            if prev.backup != installation.backup {
                let _ = fs::remove_dir_all(&prev.backup);
            }
        }
        let mut running = new_running(
            receiver,
            readiness,
            Some(session.clone()),
            candidate,
            installation,
            layer as u8,
        );
        running.transport = Some(transport);
        Ok(running)
    }
    .await;
    if outcome.is_err() {
        session.shutdown().await;
    }
    outcome
}

pub(crate) fn merge_previous_original_into_backup(
    previous_backup: &Path,
    new_backup: &Path,
) -> Result<(), String> {
    if previous_backup == new_backup {
        return Ok(());
    }
    let prev_keymap = previous_backup.join("keymap.json");
    if !prev_keymap.exists() {
        return Ok(());
    }
    let pre_write_path = new_backup.join("pre-write-keymap.json");
    let new_keymap_path = new_backup.join("keymap.json");
    let pre_write_bytes = if pre_write_path.exists() {
        Some(fs::read(&pre_write_path).map_err(|e| e.to_string())?)
    } else if new_keymap_path.exists() {
        let bytes = fs::read(&new_keymap_path).map_err(|e| e.to_string())?;
        write_private_backup_file(&pre_write_path, &bytes)?;
        Some(bytes)
    } else {
        None
    };
    let original_keymap = fs::read(&prev_keymap).map_err(|e| e.to_string())?;
    write_private_backup_file(&new_keymap_path, &original_keymap)?;
    let prev_planned_path = previous_backup.join("planned-keymap.json");
    let prev_planned = if prev_planned_path.exists() {
        let bytes = fs::read(&prev_planned_path).map_err(|e| e.to_string())?;
        write_private_backup_file(&new_backup.join("previous-planned-keymap.json"), &bytes)?;
        Some(bytes)
    } else {
        None
    };
    let prev_manifest = fs::read(previous_backup.join("manifest.json"))
        .ok()
        .and_then(|raw| serde_json::from_slice::<serde_json::Value>(&raw).ok());
    let manifest_path = new_backup.join("manifest.json");
    if manifest_path.exists() {
        let raw_manifest = fs::read(&manifest_path).map_err(|e| e.to_string())?;
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&raw_manifest).map_err(|e| e.to_string())?;
        manifest["keymap_sha256"] =
            serde_json::Value::String(mi_bridge::lighting_arbiter::sha256_hex(&original_keymap));
        if let Some(pre_write) = &pre_write_bytes {
            manifest["pre_write_keymap_sha256"] =
                serde_json::Value::String(mi_bridge::lighting_arbiter::sha256_hex(pre_write));
        }
        if let Some(prev_manifest) = &prev_manifest {
            if let Some(prev_profile) = prev_manifest.get("previous_profile_index") {
                manifest["previous_profile_index"] = prev_profile.clone();
            }
            if let Some(prev_layer) = prev_manifest.get("previous_layer_index") {
                manifest["previous_layer_index"] = prev_layer.clone();
            }
        }
        if let Some(prev_planned) = &prev_planned {
            manifest["previous_planned_keymap_sha256"] =
                serde_json::Value::String(mi_bridge::lighting_arbiter::sha256_hex(prev_planned));
        }
        let updated = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
        write_private_backup_file(&manifest_path, &updated)?;
    }
    if let Ok(dir) = fs::File::open(new_backup) {
        let _ = dir.sync_all();
    }
    Ok(())
}

fn write_private_backup_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())
}

fn new_running(
    receiver: Arc<UserCommandReceiver>,
    readiness: Readiness,
    initial_session: Option<Arc<DeviceSession>>,
    candidate: HidCandidate,
    installation: Installation,
    layer: u8,
) -> RunningSixLayer {
    RunningSixLayer {
        receiver,
        readiness,
        candidate,
        installation,
        observed_layer: Arc::new(AtomicU8::new(layer)),
        first_event_verified: Arc::new(AtomicBool::new(false)),
        armed: false,
        execution_policy: Arc::new(ExecutionPolicy::default()),
        receipts: None,
        routing: Arc::new(Mutex::new(RoutingContext::default())),
        navigation: None,
        vendor_bridge: VendorBridge::new(initial_session),
        transport: None,
        stop: Arc::new(AtomicBool::new(false)),
        monitor: None,
        executor: None,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ResumeSnapshotAction {
    VerifyReady { layer: u8 },
    RequiresExplicitApply,
}

pub(crate) async fn inspect_resume_snapshot(
    session: &DeviceSession,
    expected: &[u8],
    smart: Option<&[u8]>,
    profile_index: u32,
) -> Result<ResumeSnapshotAction, String> {
    if mi_model::apply::recognize_six_layer_profile(expected, smart) == Ok(Some(profile_index)) {
        let layer = session
            .verify_six_layer_files(expected, smart, profile_index)
            .await
            .map_err(|e| format!("{e}；未写设备，请显式应用或恢复"))?;
        Ok(ResumeSnapshotAction::VerifyReady { layer: layer as u8 })
    } else {
        let compiled = mi_model::apply::compile_six_layer_profile(expected, smart)
            .map_err(|e| format!("{e}；未写设备，请显式应用或恢复"))?;
        if compiled.profile_index != profile_index {
            return Err("六层活动方案索引不符；未写设备，请显式应用或恢复".into());
        }
        match session
            .matches_six_layer_snapshot(expected, smart, profile_index)
            .await
        {
            Ok(mi_session::SixLayerSnapshotCheck::Matched) => {
                Ok(ResumeSnapshotAction::RequiresExplicitApply)
            }
            Ok(mi_session::SixLayerSnapshotCheck::FilesDiverged) => Err(
                "旧版六层快照与当前设备不符，未自动覆盖；请显式点击「应用到键盘」或「恢复原键盘配置」"
                    .into(),
            ),
            Ok(mi_session::SixLayerSnapshotCheck::StatusNotReady) => {
                Err("六层固件、方案身份或层位未就绪；未写设备，请显式应用或恢复".into())
            }
            Err(e) => Err(format!("{e}；未写设备，请显式应用或恢复")),
        }
    }
}

pub async fn resume(
    document_dir: &Path,
    mut saved: SavedSixLayerState,
    transport: Arc<MacTransport>,
) -> Result<RunningSixLayer, String> {
    let mut installation = saved
        .installation
        .clone()
        .filter(|installation| installation.status == InstallationStatus::Applied)
        .ok_or("六层记录未完成，需要恢复")?;
    installation.plan.validate_current()?;
    let expected =
        fs::read(installation.backup.join("planned-keymap.json")).map_err(|e| e.to_string())?;
    let smart = match fs::read(installation.backup.join("smart_actions.json")) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.to_string()),
    };
    let target = mi_transport::discover_target_keyboard().map_err(|e| e.to_string())?;
    let (session, candidate) = connect(transport.as_ref(), &target)?;
    let layer = match inspect_resume_snapshot(
        &session,
        &expected,
        smart.as_deref(),
        installation.profile_index,
    )
    .await
    {
        Ok(ResumeSnapshotAction::VerifyReady { layer }) => layer,
        Ok(ResumeSnapshotAction::RequiresExplicitApply) => {
            session.shutdown().await;
            return Err("旧版六层布局需要显式应用；重连未写设备".into());
        }
        Err(error) => {
            session.shutdown().await;
            return Err(error);
        }
    };
    let outcome: Result<RunningSixLayer, String> = async {
        let receiver = Arc::new(UserCommandReceiver::bind_default().map_err(|e| e.to_string())?);
        let rule = karabiner::rules(&target, &receiver, installation.plan.bottom_row_layout)?;
        let path = installation.config.path.clone();
        let patch = karabiner::prepare_config(
            path.clone(),
            fs::read(&path).map_err(|e| e.to_string())?,
            &target,
            rule,
        )?;
        let baseline = ReloadBaseline::capture()?;
        patch.install(&document_dir.join("karabiner-backups"))?;
        installation.config.installed = patch.installed.clone();
        installation.config.installed_device = patch.installed_device.clone();
        installation.target = target.clone();
        saved.installation = Some(installation.clone());
        save(&document_dir.join("six-layer-runtime.json"), &saved)?;
        // The board is already direct-coded. A failed reload must leave swallowing
        // rules in place, including when the previous rule had been removed.
        let readiness = wait_ready(&baseline, &patch, &target)?;
        let mut running = new_running(
            receiver,
            readiness,
            Some(session.clone()),
            candidate,
            installation,
            layer,
        );
        running.transport = Some(transport);
        Ok(running)
    }
    .await;
    if outcome.is_err() {
        session.shutdown().await;
    }
    outcome
}

async fn with_verification_session<T>(
    session: &DeviceSession,
    operation: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, String> {
    let result = operation.await;
    session.shutdown().await;
    result
}

fn wait_ready(
    baseline: &ReloadBaseline,
    patch: &ConfigPatch,
    target: &TargetKeyboard,
) -> Result<Readiness, String> {
    let start = Instant::now();
    loop {
        match baseline.check(patch, target) {
            Ok(ready) => return Ok(ready),
            Err(error) if start.elapsed() > Duration::from_secs(8) => return Err(error),
            Err(_) => {}
        }
        thread::sleep(Duration::from_millis(100));
    }
}

pub async fn restore(
    document_dir: &Path,
    mut saved: SavedSixLayerState,
    transport: Arc<MacTransport>,
) -> Result<(), String> {
    let installation = saved.installation.clone().ok_or("没有可恢复的六层记录")?;
    let target = mi_transport::discover_target_keyboard().map_err(|e| e.to_string())?;
    if target.location_id != installation.target.location_id {
        return Err("键盘 USB 位置已变化，未自动恢复".into());
    }
    let (session, _) = connect(transport.as_ref(), &target)?;
    with_verification_session(&session, async {
        session
            .restore_six_layer_backup(&installation.backup)
            .await
            .map_err(|e| format!("{e}；保留吞键规则"))?;
        installation.config.restore_owned(&target)?;
        saved.installation = None;
        save(&document_dir.join("six-layer-runtime.json"), &saved)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use mi_model::board_layers::BoardLayerPlan;
    use mi_model::host_router::BottomRowLayout;
    use mi_transport::ActivationPayload;
    use std::os::unix::net::UnixDatagram;

    #[test]
    fn verification_session_is_closed_on_success_and_failure() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        for outcome in [Ok(()), Err("readback mismatch".to_string())] {
            let session = {
                let _entered = runtime.enter();
                let (link, _) = mi_transport::MockLink::new();
                DeviceSession::new(
                    link,
                    Box::new(mi_proto::NativeFramer::default()),
                    Arc::new(mi_session::ReadOnlySchema),
                    mi_session::SessionConfig::default(),
                )
            };
            let expected = outcome.clone();
            let result = runtime.block_on(with_verification_session(&session, async { outcome }));
            assert_eq!(result, expected);
            assert!(session.is_disconnected());
        }
    }

    #[test]
    fn executor_runs_after_verification_rpc_closes_and_stops_on_input_loss() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let (link, _) = mi_transport::MockLink::new();
        let session = {
            let _entered = runtime.enter();
            DeviceSession::new(
                link.clone(),
                Box::new(mi_proto::NativeFramer::default()),
                Arc::new(mi_session::ReadOnlySchema),
                mi_session::SessionConfig::default(),
            )
        };
        let mut running = RunningSixLayer::isolated(session.clone(), directory.path());
        running.readiness.configuration_observed = true;
        running.readiness.keyboard_grabbed = true;
        runtime
            .block_on(with_verification_session(&session, async { Ok(()) }))
            .unwrap();
        assert!(session.is_disconnected());
        running.receiver.arm().unwrap();
        running.set_execution_policy(true, true, Instant::now() - Duration::from_secs(1));
        running.foreground_changed(Some("com.google.antigravity"));
        let driver = mi_macos::RecordingOsDriver::default();
        running.start_executor(Arc::new(mi_macos::LiveHostActionExecutor::new(
            driver.clone(),
        )));
        send(&running.receiver, 40);
        send(&running.receiver, 7);
        let receipt = running
            .receipts
            .as_ref()
            .unwrap()
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        assert_eq!(receipt.outcome, "executed");
        assert_eq!(receipt.detail, "antigravity/focus-input");
        assert_eq!(
            receipt.profile_id,
            running.installation.plan.hosts[0].profile_id
        );
        assert_eq!(
            driver.events(),
            vec![
                mi_macos::executor::RecordedOsEvent::CheckAccessibilityTrusted,
                mi_macos::executor::RecordedOsEvent::FocusAgentInput("antigravity".into()),
            ]
        );
        assert!(link.writes.lock().unwrap().is_empty());
        assert!(running.receiver.failure().is_none());
        assert!(!check_input_connection(&running.receiver, false, || Err(
            "USB target changed".into()
        )));
        send(&running.receiver, 7);
        running.executor.take().unwrap().join().unwrap().unwrap();
        assert!(running.receipts.as_ref().unwrap().try_recv().is_err());
        assert_eq!(driver.events().len(), 2);
        running.shutdown(&runtime).unwrap();
    }

    #[test]
    fn input_monitor_ticks_do_not_need_vendor_rpc_and_fail_closed_on_readiness_loss() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (link, _) = mi_transport::MockLink::new();
        let session = {
            let _entered = runtime.enter();
            DeviceSession::new(
                link.clone(),
                Box::new(mi_proto::NativeFramer::default()),
                Arc::new(mi_session::ReadOnlySchema),
                mi_session::SessionConfig::default(),
            )
        };
        runtime.block_on(session.shutdown());
        let receiver = UserCommandReceiver::bind().unwrap();
        receiver.arm().unwrap();
        for _ in 0..1000 {
            assert!(check_input_connection(&receiver, false, || Ok(())));
        }
        assert!(link.writes.lock().unwrap().is_empty());
        assert!(receiver.failure().is_none());
        assert!(!check_input_connection(&receiver, false, || Err(
            "USB target changed".into()
        )));
        assert_eq!(receiver.failure().as_deref(), Some("USB target changed"));
        assert!(receiver.next().is_err());
        let receiver = UserCommandReceiver::bind().unwrap();
        assert!(!check_input_connection(&receiver, true, || panic!(
            "sleep stops before recheck"
        )));
        assert!(receiver.failure().unwrap().contains("睡眠"));
    }

    fn send(receiver: &UserCommandReceiver, control: u8) {
        let payload =
            serde_json::to_vec(&ActivationPayload::new(receiver.generation(), control)).unwrap();
        UnixDatagram::unbound()
            .unwrap()
            .send_to(&payload, receiver.path())
            .unwrap();
    }

    fn test_plan() -> AppliedSixLayerPlan {
        let mut draft = mi_model::draft::Draft::default();
        mi_model::preset::install_agent_starter_profiles(&mut draft).unwrap();
        let id = |name: &str| {
            draft
                .document
                .editable
                .graph
                .profiles
                .iter()
                .find(|(_, profile)| profile.name == name)
                .unwrap()
                .0
                .clone()
        };
        let desired =
            BoardLayerPlan::recommended(id("Codex"), id("Antigravity"), id("DeepSeek Coder (DSH)"));
        AppliedSixLayerPlan::compile(&desired, &draft.document, BottomRowLayout::Split, true)
            .unwrap()
    }

    #[test]
    fn native_codex_lower_rows_dispatch_compiled_actions_and_pause_without_events() {
        let directory = tempfile::tempdir().unwrap();
        let store = crate::WorkbenchStore::open_isolated(directory.path()).unwrap();
        let native = store
            .find_canonical_profile_id(crate::CanonicalAgent::Codex)
            .unwrap();
        let pinned = store.compile_profile_twenty_slots(&native).unwrap();
        let plan = test_plan();
        for mode in [
            SixLayerRoutingMode::Locked(pinned.clone()),
            SixLayerRoutingMode::ManualPinned(pinned.clone()),
        ] {
            for enabled in [true, false] {
                let receiver = UserCommandReceiver::bind().unwrap();
                receiver.arm().unwrap();
                let routing = Mutex::new(RoutingContext::default());
                routing
                    .lock()
                    .unwrap()
                    .set_routing(mode.clone(), Some("com.openai.codex"));
                for control in 0..20 {
                    send(&receiver, control);
                }
                let stop = AtomicBool::new(false);
                let policy = ExecutionPolicy::default();
                policy.set(enabled, true, Instant::now() - Duration::from_secs(1));
                let (tx, rx) = std::sync::mpsc::sync_channel(20);
                let mut dispatched = Vec::new();
                let mut received = 0;
                let deadline = Instant::now() + Duration::from_secs(1);
                execute_clicks(
                    &receiver,
                    &plan,
                    &stop,
                    &policy,
                    &AtomicU8::new(1),
                    &AtomicBool::new(false),
                    &routing,
                    &tx,
                    None,
                    || {
                        let next = receiver.next();
                        match &next {
                            Ok(Some(_)) => received += 1,
                            Ok(None) if received == 20 => stop.store(true, Ordering::Release),
                            Ok(None) => {
                                assert!(Instant::now() < deadline, "input delivery timed out")
                            }
                            Err(error) => panic!("input delivery failed: {error}"),
                        }
                        next
                    },
                    || Ok(()),
                    |profile, binding, action, _| {
                        dispatched.push((profile.to_owned(), binding.to_owned()));
                        mi_macos::SimulatedHostActionExecutor.execute(profile, binding, action)
                    },
                );
                let expected = if enabled {
                    (6..=12)
                        .map(|slot| (native.clone(), format!("AG{slot:02}")))
                        .collect()
                } else {
                    Vec::new()
                };
                assert_eq!(dispatched, expected);
                let receipts: Vec<_> = rx.try_iter().map(|r| r.detail).collect();
                assert_eq!(
                    receipts,
                    if enabled {
                        [
                            "live-voice",
                            "focus-input",
                            "new-tab",
                            "cycle-workspace-layout",
                            "new-chat",
                            "toggle-dictation",
                            "submit",
                        ]
                        .map(|command| format!("codex/{command}"))
                        .to_vec()
                    } else {
                        Vec::new()
                    }
                );
                assert!(receiver.failure().is_none());
            }
        }
    }

    #[test]
    fn custom_codex_routes_layer_one_vendor_in_manual_and_auto_follow_modes() {
        let plan = test_plan();
        let custom = PinnedProfilePlan {
            profile_id: "local-codex".into(),
            bundle_id: Some("com.openai.codex".into()),
            actions: plan.hosts[0].actions.clone(),
        };
        for mode in [
            SixLayerRoutingMode::ManualPinned(custom.clone()),
            SixLayerRoutingMode::AutoFollow {
                dynamic_hosts: vec![custom.clone()],
            },
        ] {
            let mut context = RoutingContext::default();
            context.set_routing(mode, Some("com.openai.codex"));
            assert!(context.accepts_layer1_vendor_fallback());
            assert_eq!(
                context.effective_profile_id(&plan, 1).as_deref(),
                Some("local-codex")
            );
            assert_eq!(
                context.resolve_control(&plan, 1, 0).map(|(id, _)| id),
                Some("local-codex")
            );
        }
    }

    #[test]
    fn custom_codex_vendor_dispatches_in_each_mode_and_stops_when_exclusive_session_is_lost() {
        use mi_proto::Framer;
        use mi_transport::HidLink;

        let plan = test_plan();
        let custom = PinnedProfilePlan {
            profile_id: "local-codex".into(),
            bundle_id: Some("com.openai.codex".into()),
            actions: std::array::from_fn(|slot| {
                (slot == 0).then(|| ResolvedHostAction::AgentCommand {
                    adapter: "codex".into(),
                    command: "focus-input".into(),
                })
            }),
        };
        let runtime = tokio::runtime::Runtime::new().unwrap();
        for mode in [
            SixLayerRoutingMode::Locked(custom.clone()),
            SixLayerRoutingMode::ManualPinned(custom.clone()),
            SixLayerRoutingMode::AutoFollow {
                dynamic_hosts: vec![custom.clone()],
            },
        ] {
            let (link, _writes) = mi_transport::MockLink::new();
            let session = {
                let _entered = runtime.enter();
                DeviceSession::new(
                    Arc::new(MockVendorLink {
                        inner: link.clone(),
                        exclusive: true,
                    }),
                    Box::new(mi_proto::NativeFramer::default()),
                    Arc::new(mi_session::ReadOnlySchema),
                    mi_session::SessionConfig::default(),
                )
            };
            assert!(session.has_exclusive_vendor_link());
            let receiver = Arc::new(UserCommandReceiver::bind().unwrap());
            receiver.arm().unwrap();
            let routing = Arc::new(Mutex::new(RoutingContext::default()));
            routing
                .lock()
                .unwrap()
                .set_routing(mode, Some("com.openai.codex"));
            let mut bridge = VendorBridge::new(Some(session.clone()));
            bridge.spawn_with_reopen(
                &runtime,
                Some(session.clone()),
                receiver.clone(),
                routing.clone(),
                true,
                || async { None },
            );
            let wire = b"{\"m\":\"v.oai.hid\",\"p\":{\"k\":\"AG00\",\"act\":1}}\n";
            for frame in mi_proto::NativeFramer::default().encode(wire).unwrap() {
                link.inject.send(frame).unwrap();
            }
            let until = Instant::now() + Duration::from_secs(1);
            let mut event = loop {
                if let Some(event) = receiver.next().unwrap() {
                    break Some(event);
                }
                assert!(
                    Instant::now() < until,
                    "Codex vendor press was not delivered"
                );
                thread::yield_now();
            };
            let stop = AtomicBool::new(false);
            let policy = ExecutionPolicy::default();
            policy.set(true, true, Instant::now() - Duration::from_secs(1));
            let (tx, rx) = std::sync::mpsc::sync_channel(2);
            execute_clicks(
                &receiver,
                &plan,
                &stop,
                &policy,
                &AtomicU8::new(1),
                &AtomicBool::new(false),
                &routing,
                &tx,
                None,
                || {
                    let next = event.take();
                    if next.is_none() {
                        stop.store(true, Ordering::Release);
                    }
                    Ok(next)
                },
                || {
                    session
                        .has_exclusive_vendor_link()
                        .then_some(())
                        .ok_or_else(|| "exclusive ownership missing".into())
                },
                |profile, binding, action, _| {
                    mi_macos::SimulatedHostActionExecutor.execute(profile, binding, action)
                },
            );
            let receipt = rx.try_recv().unwrap();
            assert_eq!(receipt.profile_id, "local-codex");
            assert_eq!(receipt.detail, "codex/focus-input");
            assert_eq!(receipt.outcome, "simulated");
            link.close();
            let until = Instant::now() + Duration::from_secs(1);
            while receiver.failure().is_none() {
                assert!(Instant::now() < until, "lost owner must retire input");
                thread::yield_now();
            }
            assert!(receiver.next().is_err());
            assert!(rx.try_recv().is_err());
            bridge.shutdown(&runtime);
        }
        let mut context = RoutingContext::default();
        let protected = PinnedProfilePlan {
            actions: std::array::from_fn(|_| None),
            ..custom
        };
        for mode in [
            SixLayerRoutingMode::Locked(protected.clone()),
            SixLayerRoutingMode::ManualPinned(protected),
        ] {
            context.set_routing(mode, Some("com.openai.codex"));
            assert!(!context.accepts_layer1_vendor_fallback());
            assert!(context.resolve_control(&plan, 1, 0).is_none());
        }
    }

    #[test]
    fn six_layer_connection_rejects_a_shared_vendor_link() {
        let (link, _writes) = mi_transport::MockLink::new();
        let transport = mi_transport::MockTransport::new(link);
        let target = TargetKeyboard {
            vendor_id: 0x303a,
            product_id: 0x8298,
            location_id: 1,
            usb_parent: 1,
            vendor_path: "synthetic".into(),
        };
        transport.set_devices(vec![HidCandidate {
            path: target.vendor_path.clone(),
            vid: target.vendor_id,
            pid: target.product_id,
            product: "Synthetic".into(),
            usage_page: 0xff00,
            usage: 1,
            usage_pairs: vec![],
            transport_hint: mi_transport::TransportHint::Usb,
        }]);
        assert!(
            matches!(connect(&transport, &target), Err(error) if error.contains("未取得 USB 专用接口独占"))
        );
    }

    #[test]
    fn foreground_host_overrides_physical_host_without_changing_observed_layer() {
        let plan = test_plan();
        let receiver = UserCommandReceiver::bind().unwrap();
        receiver.arm().unwrap();
        let foreground = Mutex::new(RoutingContext::default());
        foreground
            .lock()
            .unwrap()
            .update(Some("com.google.antigravity"));
        send(&receiver, 27);
        let stop = AtomicBool::new(false);
        let policy = ExecutionPolicy::default();
        policy.set(true, true, Instant::now() - Duration::from_secs(1));
        let observed = AtomicU8::new(1);
        let (tx, rx) = std::sync::mpsc::sync_channel(8);
        thread::scope(|scope| {
            let (done, wait) = std::sync::mpsc::channel::<()>();
            let stop_ref = &stop;
            scope.spawn(move || {
                if wait.recv_timeout(Duration::from_secs(5)).is_err() {
                    stop_ref.store(true, Ordering::Release);
                }
            });
            execute_clicks(
                &receiver,
                &plan,
                &stop,
                &policy,
                &observed,
                &AtomicBool::new(false),
                &foreground,
                &tx,
                None,
                || receiver.next(),
                || Ok(()),
                |profile, binding, action, _| {
                    stop.store(true, Ordering::Release);
                    mi_macos::SimulatedHostActionExecutor.execute(profile, binding, action)
                },
            );
            let _ = done.send(());
        });
        let receipt = rx.try_recv().unwrap();
        assert_eq!(receipt.profile_id, plan.hosts[0].profile_id);
        assert_eq!(receipt.detail, "antigravity/focus-input");
        assert_eq!(observed.load(Ordering::Acquire), 3);
        let mut context = foreground.lock().unwrap();
        context.update(Some("com.openai.codex"));
        assert_eq!(context.host_index(&plan, 3), 1);
        context.update(Some("com.apple.finder"));
        assert_eq!(context.host_index(&plan, 4), 2);
    }

    #[test]
    fn foreground_switch_discards_queued_and_waiting_clicks_without_retargeting() {
        let plan = test_plan();
        let receiver = UserCommandReceiver::bind().unwrap();
        receiver.arm().unwrap();
        let foreground = Mutex::new(RoutingContext::default());
        send(&receiver, 7);
        thread::sleep(Duration::from_millis(30));
        foreground
            .lock()
            .unwrap()
            .update(Some("com.google.antigravity"));
        send(&receiver, 27);
        let stop = AtomicBool::new(false);
        let policy = ExecutionPolicy::default();
        policy.set(true, true, Instant::now() - Duration::from_secs(1));
        let (tx, rx) = std::sync::mpsc::sync_channel(8);
        let mut permits = 0;
        thread::scope(|scope| {
            let (done, wait) = std::sync::mpsc::channel::<()>();
            let stop_ref = &stop;
            scope.spawn(move || {
                if wait.recv_timeout(Duration::from_secs(5)).is_err() {
                    stop_ref.store(true, Ordering::Release);
                }
            });
            execute_clicks(
                &receiver,
                &plan,
                &stop,
                &policy,
                &AtomicU8::new(1),
                &AtomicBool::new(false),
                &foreground,
                &tx,
                None,
                || receiver.next(),
                || {
                    permits += 1;
                    if permits == 1 {
                        foreground.lock().unwrap().update(Some("com.deepseek.dsh"));
                        send(&receiver, 27);
                    }
                    Ok(())
                },
                |profile, binding, action, _| {
                    stop.store(true, Ordering::Release);
                    mi_macos::SimulatedHostActionExecutor.execute(profile, binding, action)
                },
            );
            let _ = done.send(());
        });
        let receipts: Vec<_> = rx.try_iter().collect();
        assert_eq!(permits, 2);
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].profile_id, plan.hosts[1].profile_id);
        assert_eq!(receipts[0].detail, "dsh-desktop/browser");
        assert!(receiver.failure().is_none());
    }

    #[test]
    fn execute_clicks_ignores_retired_layer_and_stops_on_connection_loss() {
        let plan = test_plan();
        let ag_id = plan.hosts[0].profile_id.clone();
        let dsh_id = plan.hosts[1].profile_id.clone();

        let receiver = Arc::new(UserCommandReceiver::bind().unwrap());
        receiver.arm().unwrap();
        send(&receiver, 0);
        send(&receiver, 20);
        send(&receiver, 40);
        send(&receiver, 21);
        thread::sleep(Duration::from_millis(30));

        let stop = AtomicBool::new(false);
        let policy = ExecutionPolicy::default();
        policy.set(true, true, Instant::now() - Duration::from_secs(1));
        let observed = AtomicU8::new(1);
        let first = AtomicBool::new(false);
        let (tx, rx) = std::sync::mpsc::sync_channel(8);
        let mut calls = Vec::new();
        let mut permit_calls = 0usize;

        execute_clicks(
            &receiver,
            &plan,
            &stop,
            &policy,
            &observed,
            &first,
            &Mutex::new(RoutingContext::default()),
            &tx,
            None,
            || receiver.next(),
            || {
                permit_calls += 1;
                if permit_calls <= 2 {
                    Ok(())
                } else {
                    Err("设备连接已结束".into())
                }
            },
            |profile, binding, _action, _epoch| {
                calls.push((profile.to_owned(), binding.to_owned()));
                mi_macos::HostActionReceipt {
                    action_type: "open_app".into(),
                    binding_id: binding.into(),
                    detail: "ok".into(),
                    outcome: "executed".into(),
                    profile_id: profile.into(),
                    target: "test".into(),
                    text_byte_len: None,
                    timestamp_ms: 1,
                }
            },
        );

        assert!(first.load(Ordering::Acquire));
        assert_eq!(observed.load(Ordering::Acquire), 3);
        assert_eq!(
            calls,
            vec![(ag_id, "AG00".into()), (dsh_id, "AG00".into()),]
        );
        assert_eq!(rx.try_iter().count(), 2);
        assert_eq!(receiver.failure().as_deref(), Some("设备连接已结束"));
    }

    #[test]
    fn a_click_expiring_during_connection_check_does_not_stop_fresh_clicks() {
        let plan = test_plan();
        let dsh_id = plan.hosts[1].profile_id.clone();

        let receiver = Arc::new(UserCommandReceiver::bind().unwrap());
        receiver.arm().unwrap();
        send(&receiver, 0);
        thread::sleep(Duration::from_millis(30));
        let stop = AtomicBool::new(false);
        let policy = ExecutionPolicy::default();
        policy.set(true, true, Instant::now() - Duration::from_secs(3));
        let observed = AtomicU8::new(1);
        let first = AtomicBool::new(false);
        let (tx, rx) = std::sync::mpsc::sync_channel(8);
        let mut calls = Vec::new();
        let mut permit_calls = 0;
        thread::scope(|scope| {
            let (cancel_tx, cancel_rx) = std::sync::mpsc::channel::<()>();
            let watchdog_stop = &stop;
            scope.spawn(move || {
                if cancel_rx.recv_timeout(Duration::from_secs(5)).is_err() {
                    watchdog_stop.store(true, Ordering::Release);
                }
            });
            execute_clicks(
                &receiver,
                &plan,
                &stop,
                &policy,
                &observed,
                &first,
                &Mutex::new(RoutingContext::default()),
                &tx,
                None,
                || receiver.next(),
                || {
                    permit_calls += 1;
                    if permit_calls == 1 {
                        thread::sleep(Duration::from_millis(2100));
                        send(&receiver, 20);
                        thread::sleep(Duration::from_millis(30));
                    }
                    Ok(())
                },
                |profile, binding, _, _| {
                    calls.push((profile.to_owned(), binding.to_owned()));
                    stop.store(true, Ordering::Release);
                    mi_macos::HostActionReceipt {
                        action_type: "open_app".into(),
                        binding_id: binding.into(),
                        detail: "ok".into(),
                        outcome: "executed".into(),
                        profile_id: profile.into(),
                        target: "test".into(),
                        text_byte_len: None,
                        timestamp_ms: 1,
                    }
                },
            );
            let _ = cancel_tx.send(());
        });
        assert_eq!(calls, vec![(dsh_id, "AG00".into())]);
        assert_eq!(permit_calls, 2);
        assert_eq!(rx.try_iter().count(), 1);
        assert!(receiver.failure().is_none());
    }

    #[test]
    fn disable_then_enable_rejects_in_flight_request_and_accepts_new_input() {
        use mi_macos::{LiveHostActionExecutor, RecordingOsDriver};
        use std::sync::mpsc;

        let receiver = Arc::new(UserCommandReceiver::bind().unwrap());
        let plan = test_plan();
        let now = Instant::now();
        let policy = Arc::new(ExecutionPolicy::default());
        policy.set(true, true, now);
        let old_admission = policy.capture(now);
        policy.set(true, true, now + Duration::from_millis(1));
        assert_eq!(policy.capture(now).epoch, old_admission.epoch);
        let driver = RecordingOsDriver::new(true);
        let executor = Arc::new(LiveHostActionExecutor::new(driver.clone()));
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (resume_tx, resume_rx) = mpsc::sync_channel(1);
        let (receipt_tx, receipt_rx) = mpsc::sync_channel(1);
        let worker_policy = Arc::clone(&policy);
        let worker_executor = Arc::clone(&executor);
        let worker = thread::spawn(move || {
            let stop = AtomicBool::new(false);
            let event = mi_transport::Activation {
                control: 0,
                generation: receiver.generation().into(),
                received_sequence: 1,
                received_at: now,
            };
            let mut pending = Some(event);
            execute_clicks(
                &receiver,
                &plan,
                &stop,
                &worker_policy,
                &AtomicU8::new(1),
                &AtomicBool::new(false),
                &Mutex::new(RoutingContext::default()),
                &receipt_tx,
                None,
                || {
                    pending
                        .take()
                        .map(Some)
                        .ok_or_else(|| "test input exhausted".into())
                },
                || {
                    entered_tx.send(()).unwrap();
                    resume_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                    Ok(())
                },
                |profile, binding, action, admission| {
                    let receipt = worker_executor.execute_when(profile, binding, action, || {
                        !stop.load(Ordering::Acquire)
                            && receiver.failure().is_none()
                            && worker_policy.admits(admission, action)
                    });
                    stop.store(true, Ordering::Release);
                    receipt
                },
            );
        });
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        policy.set(false, true, now + Duration::from_millis(2));
        executor.stop_owned_actions().unwrap();
        policy.set(true, true, now + Duration::from_millis(3));
        resume_tx.send(()).unwrap();
        worker.join().unwrap();
        let receipt = receipt_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(receipt.outcome, "blocked");
        assert_eq!(receipt.detail, "input_retired");
        assert!(driver.events().is_empty());
        let action = ResolvedHostAction::OpenApp {
            bundle_id: "com.example.editor".into(),
        };
        let new_admission = policy.capture(now + Duration::from_millis(4));
        let receipt = executor.execute_when("new", "AG00", &action, || {
            policy.admits(new_admission, &action)
        });
        assert_eq!(receipt.outcome, "executed");
        assert!(!driver.events().is_empty());
    }

    #[test]
    fn disable_then_enable_discards_queued_old_input_before_execution() {
        use mi_macos::{LiveHostActionExecutor, RecordingOsDriver};

        let receiver = UserCommandReceiver::bind().unwrap();
        let plan = test_plan();
        let now = Instant::now();
        let policy = ExecutionPolicy::default();
        policy.set(true, true, now);
        let mut pending = std::collections::VecDeque::from([
            mi_transport::Activation {
                control: 0,
                generation: receiver.generation().into(),
                received_sequence: 1,
                received_at: now,
            },
            mi_transport::Activation {
                control: 0,
                generation: receiver.generation().into(),
                received_sequence: 2,
                received_at: now + Duration::from_millis(3),
            },
        ]);
        let driver = RecordingOsDriver::new(true);
        let executor = LiveHostActionExecutor::new(driver.clone());
        policy.set(false, true, now + Duration::from_millis(1));
        executor.stop_owned_actions().unwrap();
        policy.set(true, true, now + Duration::from_millis(2));
        let stop = AtomicBool::new(false);
        let (tx, rx) = std::sync::mpsc::sync_channel(2);
        let mut executed = 0;
        execute_clicks(
            &receiver,
            &plan,
            &stop,
            &policy,
            &AtomicU8::new(1),
            &AtomicBool::new(false),
            &Mutex::new(RoutingContext::default()),
            &tx,
            None,
            || {
                pending
                    .pop_front()
                    .map(Some)
                    .ok_or_else(|| "test input exhausted".into())
            },
            || Ok(()),
            |profile, binding, action, admission| {
                assert_eq!(admission.received_at, now + Duration::from_millis(3));
                assert!(driver.events().is_empty());
                executed += 1;
                let receipt = executor.execute_when(profile, binding, action, || {
                    !stop.load(Ordering::Acquire)
                        && receiver.failure().is_none()
                        && policy.admits(admission, action)
                });
                stop.store(true, Ordering::Release);
                receipt
            },
        );
        assert_eq!(executed, 1);
        assert_eq!(rx.try_iter().count(), 1);
        assert!(!driver.events().is_empty());
        assert!(receiver.failure().is_none());
    }

    #[test]
    fn saved_state_roundtrips_and_rejects_unsupported_versions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("six-layer-runtime.json");
        assert!(!load(&path)
            .unwrap()
            .legacy_native_paused_confirmation
            .is_some());
        let mut state = SavedSixLayerState::default();
        state.legacy_native_paused_confirmation = Some(true);
        save(&path, &state).unwrap();
        assert!(load(&path)
            .unwrap()
            .legacy_native_paused_confirmation
            .is_some());
        fs::write(
            &path,
            br#"{"version":2,"cindy_native_paused_confirmed":true,"installation":null}"#,
        )
        .unwrap();
        assert!(load(&path).is_err());
    }

    #[test]
    fn cross_layer_physical_switch_release_clears_pressed_mask_and_codex_layer_never_emits_host_activation(
    ) {
        use mi_proto::Framer;
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (link, _) = mi_transport::MockLink::new();
        let session = {
            let _entered = runtime.enter();
            DeviceSession::new(
                link.clone(),
                Box::new(mi_proto::NativeFramer::default()),
                Arc::new(mi_session::ReadOnlySchema),
                mi_session::SessionConfig::default(),
            )
        };
        let mut running = RunningSixLayer::isolated(session.clone(), dir.path());
        running.receiver.arm().unwrap();
        running.attach_test_vendor_session(&runtime, session);

        let framer = mi_proto::NativeFramer::default();
        for packet in [
            r#"{"m":"v.oai.hid","p":{"k":"AG06","act":1}}"#,
            r#"{"m":"v.oai.hid","p":{"k":"AG12","act":0}}"#,
            r#"{"m":"v.oai.hid","p":{"k":"AG06","act":1}}"#,
            r#"{"m":"v.oai.hid","p":{"k":"AG00","act":0}}"#,
            r#"{"m":"v.oai.hid","p":{"k":"AG00","act":1}}"#,
            r#"{"m":"v.oai.hid","p":{"k":"AG00","act":0}}"#,
            r#"{"m":"v.oai.hid","p":{"k":"AG06","act":1}}"#,
            r#"{"m":"v.oai.hid","p":{"k":"AG06","act":0}}"#,
        ] {
            let mut line = packet.to_string();
            line.push('\n');
            for frame in framer.encode(line.as_bytes()).unwrap() {
                link.inject.send(frame).unwrap();
            }
        }

        let mut controls = Vec::new();
        let start = Instant::now();
        while controls.len() < 3 && start.elapsed() < Duration::from_secs(2) {
            if let Some(ev) = running.receiver.next().unwrap() {
                controls.push(ev.control);
            } else {
                thread::sleep(Duration::from_millis(5));
            }
        }
        thread::sleep(Duration::from_millis(25));
        while let Some(ev) = running.receiver.next().unwrap() {
            controls.push(ev.control);
        }
        assert_eq!(controls, vec![0, 0, 0]);
        running.shutdown(&runtime).unwrap();
    }

    #[test]
    fn shared_vendor_bridge_rotates_after_shutting_down_seized_session_without_sleep_and_retire_takes_session(
    ) {
        use mi_proto::Framer;
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let make_rpc_session = |rt: &tokio::runtime::Runtime| {
            let (link, mut writes) = mi_transport::MockLink::new();
            let inject = link.inject.clone();
            let session = {
                let _entered = rt.enter();
                DeviceSession::new(
                    link.clone(),
                    Box::new(mi_proto::NativeFramer::default()),
                    Arc::new(mi_session::ReadOnlySchema),
                    mi_session::SessionConfig {
                        timeout: Duration::from_secs(2),
                        interval: Duration::ZERO,
                    },
                )
            };
            rt.spawn(async move {
                let mut in_framer = mi_proto::NativeFramer::default();
                let out_framer = mi_proto::NativeFramer::default();
                while let Some(report) = writes.recv().await {
                    let Ok(messages) = in_framer.feed(&report) else {
                        break;
                    };
                    for msg in messages {
                        let Ok(req) = serde_json::from_slice::<serde_json::Value>(&msg) else {
                            continue;
                        };
                        let result_val = if req["method"].as_str() == Some("v.oai.thstatus") {
                            serde_json::Value::Null
                        } else {
                            serde_json::json!({"version": "0.6.3", "profile_index": 1, "layer_index": 1})
                        };
                        let reply = serde_json::json!({
                            "id": req["id"],
                            "result": result_val
                        });
                        let mut wire = reply.to_string();
                        wire.push('\n');
                        for frame in out_framer.encode(wire.as_bytes()).unwrap() {
                            let _ = inject.send(frame);
                        }
                    }
                }
            });
            (link, session)
        };

        let (link1, session1) = make_rpc_session(&runtime);
        runtime.block_on(async {
            for _ in 0..900 {
                session1
                    .request(mi_proto::Method::DeviceStatus, serde_json::Value::Null)
                    .await
                    .unwrap();
            }
        });
        assert_eq!(session1.request_count(), 900);

        let (link2, session2) = make_rpc_session(&runtime);
        let receiver = Arc::new(UserCommandReceiver::bind().unwrap());
        receiver.arm().unwrap();

        let framer = mi_proto::NativeFramer::default();
        let mut press_line = r#"{"m":"v.oai.hid","p":{"k":"AG06","act":1}}"#.to_string();
        press_line.push('\n');

        let mut bridge = VendorBridge::new(Some(session1.clone()));

        let old_for_check = session1.clone();
        let reopen_session = session2.clone();
        let reopened_flag = Arc::new(AtomicBool::new(false));
        let reopened_flag_inner = Arc::clone(&reopened_flag);
        let rotation_started = Instant::now();

        let routing = Arc::new(Mutex::new(RoutingContext::default()));
        bridge.spawn_with_reopen(
            &runtime,
            Some(session1.clone()),
            Arc::clone(&receiver),
            routing,
            true,
            move || {
                assert!(old_for_check.is_disconnected());
                reopened_flag_inner.store(true, Ordering::Release);
                let next = reopen_session.clone();
                async move { Some(next) }
            },
        );

        while !reopened_flag.load(Ordering::Acquire)
            && rotation_started.elapsed() < Duration::from_secs(2)
        {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(reopened_flag.load(Ordering::Acquire));
        assert!(rotation_started.elapsed() < Duration::from_millis(500));
        assert!(session1.is_disconnected());
        let _ = link1;

        for frame in framer.encode(press_line.as_bytes()).unwrap() {
            link2.inject.send(frame).unwrap();
        }
        let wait_press = Instant::now();
        let mut observed = Vec::new();
        while observed.is_empty() && wait_press.elapsed() < Duration::from_secs(1) {
            while let Some(ev) = receiver.next().unwrap() {
                observed.push(ev.control);
            }
            if observed.is_empty() {
                thread::sleep(Duration::from_millis(5));
            }
        }
        assert!(!observed.is_empty());

        let entries = vec![OaiKeyStatusEntry::new("AG06", 0x22c55e)];
        let _ = bridge.led_tx.send(Some(("digest-backoff".into(), entries)));
        let wait_backoff = Instant::now();
        while bridge.retry_not_before.lock().unwrap().is_none()
            && wait_backoff.elapsed() < Duration::from_secs(1)
        {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(bridge.retry_not_before.lock().unwrap().is_some());
        assert!(!bridge.needs_lighting_sync());

        bridge.retire();
        assert!(bridge.active_session.lock().unwrap().is_none());
        bridge.shutdown(&runtime);
    }

    #[test]
    fn locked_and_manual_pinned_modes_route_layers_two_to_four_and_layer_one_vendor_fallback() {
        use mi_proto::Framer;

        let plan = test_plan();
        let ag_id = plan.hosts[0].profile_id.clone();
        let pinned_actions = plan.hosts[0].actions.clone();
        assert!(pinned_actions[0].is_some());
        assert!(pinned_actions[7].is_some());
        assert!(pinned_actions[15].is_some());
        let expected_slot15_binding = CONTROL_BINDINGS[15].to_string();
        let pinned = PinnedProfilePlan {
            profile_id: ag_id.clone(),
            bundle_id: Some("com.google.antigravity".into()),
            actions: pinned_actions,
        };

        let routing = Arc::new(Mutex::new(RoutingContext::default()));
        {
            let mut ctx = routing.lock().unwrap();
            ctx.set_routing(
                SixLayerRoutingMode::Locked(pinned.clone()),
                Some("com.google.antigravity"),
            );
            assert!(!ctx.update(Some("com.openai.codex")));
            assert!(!ctx.update(Some("com.deepseek.dsh")));
            assert_eq!(
                ctx.effective_profile_id(&plan, 1).as_deref(),
                Some(ag_id.as_str())
            );
            assert_eq!(
                ctx.effective_profile_id(&plan, 3).as_deref(),
                Some(ag_id.as_str())
            );
        }

        let receiver = Arc::new(UserCommandReceiver::bind().unwrap());
        receiver.arm().unwrap();
        send(&receiver, 0);
        send(&receiver, 27);
        send(&receiver, 55);
        thread::sleep(Duration::from_millis(30));

        let stop = AtomicBool::new(false);
        let policy = ExecutionPolicy::default();
        policy.set(true, true, Instant::now() - Duration::from_secs(1));
        let observed = AtomicU8::new(1);
        let first = AtomicBool::new(false);
        let (tx, rx) = std::sync::mpsc::sync_channel(8);
        let mut dispatched = Vec::new();

        execute_clicks(
            &receiver,
            &plan,
            &stop,
            &policy,
            &observed,
            &first,
            &routing,
            &tx,
            None,
            || {
                let next = receiver.next();
                if matches!(&next, Ok(None)) {
                    stop.store(true, Ordering::Release);
                }
                next
            },
            || Ok(()),
            |profile, binding, _action, _epoch| {
                dispatched.push((profile.to_owned(), binding.to_owned()));
                mi_macos::HostActionReceipt {
                    action_type: "open_app".into(),
                    binding_id: binding.into(),
                    detail: "ok".into(),
                    outcome: "executed".into(),
                    profile_id: profile.into(),
                    target: "com.google.antigravity".into(),
                    text_byte_len: None,
                    timestamp_ms: 1,
                }
            },
        );

        assert_eq!(
            dispatched,
            vec![
                (ag_id.clone(), "AG00".into()),
                (ag_id.clone(), "AG07".into()),
                (ag_id.clone(), expected_slot15_binding),
            ]
        );
        assert_eq!(rx.try_iter().count(), 3);

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (link, _writes) = mi_transport::MockLink::new();
        let session = {
            let _entered = runtime.enter();
            DeviceSession::new(
                link.clone(),
                Box::new(mi_proto::NativeFramer::default()),
                Arc::new(mi_session::ReadOnlySchema),
                mi_session::SessionConfig {
                    timeout: Duration::from_secs(1),
                    interval: Duration::ZERO,
                },
            )
        };
        let mut bridge = VendorBridge::new(Some(session.clone()));
        bridge.spawn_with_reopen(
            &runtime,
            Some(session),
            Arc::clone(&receiver),
            Arc::clone(&routing),
            true,
            || async { None },
        );
        let framer = mi_proto::NativeFramer::default();
        let mut oai_press = r#"{"m":"v.oai.hid","p":{"k":"AG00","act":1}}"#.to_string();
        oai_press.push('\n');
        for frame in framer.encode(oai_press.as_bytes()).unwrap() {
            link.inject.send(frame).unwrap();
        }
        let wait = Instant::now();
        let mut vendor_controls = Vec::new();
        while vendor_controls.is_empty() && wait.elapsed() < Duration::from_secs(1) {
            while let Some(ev) = receiver.next().unwrap() {
                vendor_controls.push(ev.control);
            }
            if vendor_controls.is_empty() {
                thread::sleep(Duration::from_millis(5));
            }
        }
        assert_eq!(vendor_controls, vec![0]);
        bridge.retire();
        bridge.shutdown(&runtime);

        routing.lock().unwrap().set_routing(
            SixLayerRoutingMode::ManualPinned(pinned),
            Some("com.deepseek.dsh"),
        );
        send(&receiver, 20);
        thread::sleep(Duration::from_millis(30));
        stop.store(false, Ordering::Release);
        let mut manual_dispatched = Vec::new();
        execute_clicks(
            &receiver,
            &plan,
            &stop,
            &policy,
            &observed,
            &first,
            &routing,
            &tx,
            None,
            || {
                let next = receiver.next();
                if matches!(&next, Ok(None)) {
                    stop.store(true, Ordering::Release);
                }
                next
            },
            || Ok(()),
            |profile, binding, _action, _epoch| {
                manual_dispatched.push((profile.to_owned(), binding.to_owned()));
                mi_macos::HostActionReceipt {
                    action_type: "open_app".into(),
                    binding_id: binding.into(),
                    detail: "ok".into(),
                    outcome: "executed".into(),
                    profile_id: profile.into(),
                    target: "com.google.antigravity".into(),
                    text_byte_len: None,
                    timestamp_ms: 1,
                }
            },
        );
        assert_eq!(manual_dispatched, vec![(ag_id, "AG00".into())]);
    }

    struct MockVendorLink {
        inner: Arc<mi_transport::MockLink>,
        exclusive: bool,
    }

    impl mi_transport::HidLink for MockVendorLink {
        fn backend(&self) -> mi_transport::LinkBackend {
            mi_transport::LinkBackend::MacHidapi267Numbered
        }
        fn is_exclusive_vendor(&self) -> bool {
            self.exclusive
        }
        fn write<'life0, 'life1, 'async_trait>(
            &'life0 self,
            report: &'life1 [u8],
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<(), mi_transport::TransportError>>
                    + Send
                    + 'async_trait,
            >,
        >
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            Self: 'async_trait,
        {
            self.inner.write(report)
        }
        fn read<'life0, 'async_trait>(
            &'life0 self,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<Vec<u8>, mi_transport::TransportError>>
                    + Send
                    + 'async_trait,
            >,
        >
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            self.inner.read()
        }
        fn close(&self) {
            self.inner.close()
        }
    }

    use super::{test_b64_decode as b64_decode, test_b64_encode as b64_encode};

    #[test]
    fn outdated_snapshot_requires_explicit_apply_and_refuses_external_modifications() {
        const SIX_SOURCE: &[u8] =
            include_bytes!("../../mi-model/fixtures/device/synthetic-six-layer-source.json");
        const SMART: &[u8] =
            include_bytes!("../../mi-model/fixtures/device/synthetic-smart-actions-object.json");

        let canonical_template =
            mi_model::apply::compile_six_layer_profile(SIX_SOURCE, Some(SMART)).unwrap();
        let mut outdated_val: serde_json::Value =
            serde_json::from_slice(&canonical_template.keymap).unwrap();
        for layer_idx in 1..6 {
            let layer_obj = outdated_val["profiles"][1]["layers"][layer_idx]
                .as_object_mut()
                .unwrap();
            layer_obj.remove("lights");
            if layer_idx == 1 {
                layer_obj["layout"]["keymap"][0] = serde_json::json!(["KC_F13", "KC_F14"]);
            }
        }
        let raw_doc: std::collections::BTreeMap<&str, &serde_json::value::RawValue> =
            serde_json::from_slice(SIX_SOURCE).unwrap();
        let raw_profiles: Vec<std::collections::BTreeMap<&str, &serde_json::value::RawValue>> =
            serde_json::from_str(raw_doc["profiles"].get()).unwrap();
        let raw_layers: Vec<&serde_json::value::RawValue> =
            serde_json::from_str(raw_profiles[0]["layers"].get()).unwrap();
        let raw_layer0 = raw_layers[0].get();
        let outdated_layers: Vec<String> = (1..6)
            .map(|i| serde_json::to_string(&outdated_val["profiles"][1]["layers"][i]).unwrap())
            .collect();
        let outdated_profile = format!(
            "{{\"id\":1,\"name\":\"{}\",\"layers\":[{},{}]}}",
            mi_model::apply::SIX_LAYER_PROFILE_NAME,
            raw_layer0,
            outdated_layers.join(",")
        );
        let outdated_bytes = std::str::from_utf8(SIX_SOURCE)
            .unwrap()
            .replacen("\"activeProfileId\": 0", "\"activeProfileId\": 1", 1)
            .replacen(
                "  }] }],\n  \"macros\"",
                &format!("  }}] }},{outdated_profile}],\n  \"macros\""),
                1,
            )
            .into_bytes();
        assert!(
            mi_model::apply::recognize_six_layer_profile(&outdated_bytes, Some(SMART)).is_err()
        );

        let dir = tempfile::tempdir().unwrap();
        let old_backup = dir.path().join("six-layer-backup-old");
        fs::create_dir_all(&old_backup).unwrap();
        fs::write(old_backup.join("keymap.json"), SIX_SOURCE).unwrap();
        fs::write(old_backup.join("planned-keymap.json"), &outdated_bytes).unwrap();
        fs::write(old_backup.join("smart_actions.json"), SMART).unwrap();
        fs::write(
            old_backup.join("manifest.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "format": "micro-input-three-layer-recovery-v1",
                "previous_profile_index": 0,
                "previous_layer_index": 1,
                "planned_profile_index": 1,
                "keymap_sha256": mi_bridge::lighting_arbiter::sha256_hex(SIX_SOURCE),
                "planned_keymap_sha256": mi_bridge::lighting_arbiter::sha256_hex(&outdated_bytes),
            }))
            .unwrap(),
        )
        .unwrap();

        let device_state = Arc::new(Mutex::new((0u64, outdated_bytes.clone(), 0usize)));
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let open_mock_session = |exclusive: bool| {
            let state = Arc::clone(&device_state);
            let (link, mut requests) = mi_transport::MockLink::new();
            let inject = link.inject.clone();
            let session = {
                let _entered = runtime.enter();
                DeviceSession::new(
                    Arc::new(MockVendorLink {
                        inner: link,
                        exclusive,
                    }),
                    Box::new(mi_proto::NativeFramer::default()),
                    Arc::new(mi_session::ReadOnlySchema),
                    mi_session::SessionConfig {
                        timeout: Duration::from_secs(2),
                        interval: Duration::ZERO,
                    },
                )
            };
            runtime.spawn(async move {
                use mi_proto::Framer;
                let mut incoming = mi_proto::NativeFramer::default();
                let outbound = mi_proto::NativeFramer::default();
                let mut staged = Vec::new();
                while let Some(frame) = requests.recv().await {
                    let Ok(messages) = incoming.feed(&frame) else {
                        break;
                    };
                    for bytes in messages {
                        let req: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                        let params = &req["params"];
                        let meta = |name: &str, data: &[u8]| {
                            let sha1 = mi_model::operation_record::file_value(data)["sha1"]
                                .as_str()
                                .unwrap()
                                .to_owned();
                            serde_json::json!({"name": name, "size": data.len(), "checksum": sha1})
                        };
                        let result = match req["method"].as_str().unwrap() {
                            "device.status" => {
                                let guard = state.lock().unwrap();
                                serde_json::json!({
                                    "version": "0.6.3",
                                    "profile_index": guard.0,
                                    "layer_index": 2
                                })
                            }
                            "fs.list" => {
                                let guard = state.lock().unwrap();
                                serde_json::json!([
                                    meta("keymap.json", &guard.1),
                                    meta("smart_actions.json", SMART)
                                ])
                            }
                            "fs.readbin" => {
                                let guard = state.lock().unwrap();
                                let file_bytes = match params["file"].as_str().unwrap() {
                                    "keymap.json" => guard.1.as_slice(),
                                    "smart_actions.json" => SMART,
                                    other => panic!("unexpected file {other}"),
                                };
                                let offset = params["offset"].as_u64().unwrap() as usize;
                                let len = params["len"].as_u64().unwrap() as usize;
                                let end = (offset + len).min(file_bytes.len());
                                serde_json::json!({
                                    "total_size": file_bytes.len(),
                                    "data": b64_encode(&file_bytes[offset..end])
                                })
                            }
                            "fs.txbegin" => serde_json::json!({"tx": 91}),
                            "fs.writebin" => {
                                staged.extend(b64_decode(params["data"].as_str().unwrap()));
                                serde_json::Value::Null
                            }
                            "fs.txcommit" => {
                                let next_keymap = std::mem::take(&mut staged);
                                let parsed: serde_json::Value =
                                    serde_json::from_slice(&next_keymap).unwrap();
                                let mut guard = state.lock().unwrap();
                                guard.0 = parsed["activeProfileId"].as_u64().unwrap();
                                guard.1 = next_keymap;
                                guard.2 += 1;
                                serde_json::Value::Null
                            }
                            other => panic!("unexpected method {other}"),
                        };
                        let reply = serde_json::json!({"id": req["id"], "result": result});
                        let mut wire = reply.to_string();
                        wire.push('\n');
                        for out in outbound.encode(wire.as_bytes()).unwrap() {
                            let _ = inject.send(out);
                        }
                    }
                }
            });
            session
        };

        runtime.block_on(async {
            let mut cindy_val: serde_json::Value =
                serde_json::from_slice(&canonical_template.keymap).unwrap();
            cindy_val["profiles"][1]["layers"][3]["name"] = serde_json::json!("Cindy");
            let cindy_bytes = serde_json::to_vec(&cindy_val).unwrap();
            let cindy_session = open_mock_session(false);
            let cindy_err = inspect_resume_snapshot(&cindy_session, &cindy_bytes, Some(SMART), 1)
                .await
                .unwrap_err();
            assert_eq!(cindy_session.request_count(), 0);
            cindy_session.shutdown().await;
            assert!(
                cindy_err.contains("未写设备，请显式应用或恢复"),
                "{cindy_err}"
            );

            let mismatch_profile_session = open_mock_session(false);
            let mismatch_err =
                inspect_resume_snapshot(&mismatch_profile_session, &outdated_bytes, Some(SMART), 1)
                    .await
                    .unwrap_err();
            mismatch_profile_session.shutdown().await;
            assert!(
                mismatch_err.contains("六层固件、方案身份或层位未就绪；未写设备，请显式应用或恢复"),
                "{mismatch_err}"
            );
            assert_eq!(device_state.lock().unwrap().2, 0);

            {
                let mut guard = device_state.lock().unwrap();
                guard.0 = 1;
                guard.1 = SIX_SOURCE.to_vec();
            }
            let mismatch_bytes_session = open_mock_session(false);
            let mismatch_bytes_err =
                inspect_resume_snapshot(&mismatch_bytes_session, &outdated_bytes, Some(SMART), 1)
                    .await
                    .unwrap_err();
            mismatch_bytes_session.shutdown().await;
            assert!(mismatch_bytes_err.contains("旧版六层快照与当前设备不符，未自动覆盖"));
            assert_eq!(device_state.lock().unwrap().2, 0);

            {
                let mut guard = device_state.lock().unwrap();
                guard.0 = 1;
                guard.1 = outdated_bytes.clone();
            }
            let probe_session = open_mock_session(false);
            assert_eq!(
                inspect_resume_snapshot(&probe_session, &outdated_bytes, Some(SMART), 1)
                    .await
                    .unwrap(),
                ResumeSnapshotAction::RequiresExplicitApply
            );
            probe_session.shutdown().await;
            assert!(probe_session.is_disconnected());
            assert_eq!(device_state.lock().unwrap().2, 0);

            let exclusive_session = open_mock_session(true);
            let prepared = exclusive_session
                .prepare_six_layer_write(dir.path())
                .await
                .unwrap();
            merge_previous_original_into_backup(&old_backup, &prepared.backup).unwrap();
            let upgraded_candidate = prepared.candidate.clone();
            let receipt = exclusive_session
                .apply_six_layer_write(prepared)
                .await
                .unwrap();
            exclusive_session.shutdown().await;
            fs::remove_dir_all(&old_backup).unwrap();

            assert_eq!(device_state.lock().unwrap().2, 1);
            assert_eq!(
                mi_model::apply::recognize_six_layer_profile(&upgraded_candidate, Some(SMART))
                    .unwrap(),
                Some(1)
            );
            assert_eq!(
                fs::read(receipt.backup.join("keymap.json")).unwrap(),
                SIX_SOURCE
            );
            assert_eq!(
                fs::read(receipt.backup.join("planned-keymap.json")).unwrap(),
                upgraded_candidate
            );

            let fast_resume_session = open_mock_session(false);
            assert_eq!(
                inspect_resume_snapshot(&fast_resume_session, &upgraded_candidate, Some(SMART), 1)
                    .await
                    .unwrap(),
                ResumeSnapshotAction::VerifyReady { layer: 2 }
            );
            fast_resume_session.shutdown().await;
            assert_eq!(device_state.lock().unwrap().2, 1);

            let restore_session = open_mock_session(true);
            restore_session
                .restore_six_layer_backup(&receipt.backup)
                .await
                .unwrap();
            restore_session.shutdown().await;
            assert_eq!(device_state.lock().unwrap().2, 2);
            assert_eq!(device_state.lock().unwrap().1, SIX_SOURCE);
        });
    }

    #[test]
    fn external_edit_mid_write_abort_recovers_factory_original_via_pre_write_backup() {
        const SIX_SOURCE: &[u8] =
            include_bytes!("../../mi-model/fixtures/device/synthetic-six-layer-source.json");
        const SMART: &[u8] =
            include_bytes!("../../mi-model/fixtures/device/synthetic-smart-actions-object.json");

        let canonical =
            mi_model::apply::compile_six_layer_profile(SIX_SOURCE, Some(SMART)).unwrap();
        let mut external_edit_val: serde_json::Value =
            serde_json::from_slice(&canonical.keymap).unwrap();
        external_edit_val["profiles"][1]["layers"][1]["layout"]["keymap"][0][0] =
            serde_json::json!("KC_F13");
        let external_edit_bytes = serde_json::to_vec(&external_edit_val).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let prev_backup = dir.path().join("six-layer-backup-prev");
        fs::create_dir_all(&prev_backup).unwrap();
        fs::write(prev_backup.join("keymap.json"), SIX_SOURCE).unwrap();
        fs::write(prev_backup.join("planned-keymap.json"), &canonical.keymap).unwrap();
        fs::write(prev_backup.join("smart_actions.json"), SMART).unwrap();
        fs::write(
            prev_backup.join("manifest.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "format": "micro-input-three-layer-recovery-v1",
                "previous_profile_index": 0,
                "previous_layer_index": 1,
                "planned_profile_index": 1,
                "keymap_sha256": mi_bridge::lighting_arbiter::sha256_hex(SIX_SOURCE),
                "planned_keymap_sha256": mi_bridge::lighting_arbiter::sha256_hex(&canonical.keymap),
            }))
            .unwrap(),
        )
        .unwrap();

        let device_state = Arc::new(Mutex::new((1u64, external_edit_bytes.clone(), 0usize)));
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let open_exclusive = || {
            let state = Arc::clone(&device_state);
            let (link, mut requests) = mi_transport::MockLink::new();
            let inject = link.inject.clone();
            let session = {
                let _entered = runtime.enter();
                DeviceSession::new(
                    Arc::new(MockVendorLink {
                        inner: link,
                        exclusive: true,
                    }),
                    Box::new(mi_proto::NativeFramer::default()),
                    Arc::new(mi_session::ReadOnlySchema),
                    mi_session::SessionConfig {
                        timeout: Duration::from_secs(2),
                        interval: Duration::ZERO,
                    },
                )
            };
            runtime.spawn(async move {
                use mi_proto::Framer;
                let mut incoming = mi_proto::NativeFramer::default();
                let outbound = mi_proto::NativeFramer::default();
                let mut staged = Vec::new();
                while let Some(frame) = requests.recv().await {
                    let Ok(messages) = incoming.feed(&frame) else {
                        break;
                    };
                    for bytes in messages {
                        let req: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                        let params = &req["params"];
                        let meta = |name: &str, data: &[u8]| {
                            let sha1 = mi_model::operation_record::file_value(data)["sha1"]
                                .as_str()
                                .unwrap()
                                .to_owned();
                            serde_json::json!({"name": name, "size": data.len(), "checksum": sha1})
                        };
                        let result = match req["method"].as_str().unwrap() {
                            "device.status" => {
                                let guard = state.lock().unwrap();
                                serde_json::json!({
                                    "version": "0.6.3",
                                    "profile_index": guard.0,
                                    "layer_index": 2
                                })
                            }
                            "fs.list" => {
                                let guard = state.lock().unwrap();
                                serde_json::json!([
                                    meta("keymap.json", &guard.1),
                                    meta("smart_actions.json", SMART)
                                ])
                            }
                            "fs.readbin" => {
                                let guard = state.lock().unwrap();
                                let file_bytes = match params["file"].as_str().unwrap() {
                                    "keymap.json" => guard.1.as_slice(),
                                    "smart_actions.json" => SMART,
                                    other => panic!("unexpected file {other}"),
                                };
                                let offset = params["offset"].as_u64().unwrap() as usize;
                                let len = params["len"].as_u64().unwrap() as usize;
                                let end = (offset + len).min(file_bytes.len());
                                serde_json::json!({
                                    "total_size": file_bytes.len(),
                                    "data": b64_encode(&file_bytes[offset..end])
                                })
                            }
                            "fs.txbegin" => serde_json::json!({"tx": 99}),
                            "fs.writebin" => {
                                staged.extend(b64_decode(params["data"].as_str().unwrap()));
                                serde_json::Value::Null
                            }
                            "fs.txcommit" => {
                                let next_keymap = std::mem::take(&mut staged);
                                let parsed: serde_json::Value =
                                    serde_json::from_slice(&next_keymap).unwrap();
                                let mut guard = state.lock().unwrap();
                                guard.0 = parsed["activeProfileId"].as_u64().unwrap();
                                guard.1 = next_keymap;
                                guard.2 += 1;
                                serde_json::Value::Null
                            }
                            other => panic!("unexpected method {other}"),
                        };
                        let reply = serde_json::json!({"id": req["id"], "result": result});
                        let mut wire = reply.to_string();
                        wire.push('\n');
                        for out in outbound.encode(wire.as_bytes()).unwrap() {
                            let _ = inject.send(out);
                        }
                    }
                }
            });
            session
        };

        runtime.block_on(async {
            let prep_session = open_exclusive();
            let prepared = prep_session
                .prepare_six_layer_write(dir.path())
                .await
                .unwrap();
            merge_previous_original_into_backup(&prev_backup, &prepared.backup).unwrap();
            let aborted_backup = prepared.backup.clone();
            prep_session.shutdown().await;
            drop(prepared);

            let manifest: serde_json::Value =
                serde_json::from_slice(&fs::read(aborted_backup.join("manifest.json")).unwrap())
                    .unwrap();
            assert_eq!(
                manifest["pre_write_keymap_sha256"].as_str(),
                Some(mi_bridge::lighting_arbiter::sha256_hex(&external_edit_bytes).as_str())
            );
            assert_eq!(device_state.lock().unwrap().1, external_edit_bytes);

            let restore_session = open_exclusive();
            restore_session
                .restore_six_layer_backup(&aborted_backup)
                .await
                .unwrap();
            restore_session.shutdown().await;
            assert_eq!(device_state.lock().unwrap().1, SIX_SOURCE);
        });
    }

    #[test]
    fn reapply_uses_previous_karabiner_path_and_rejects_profile_name_drift() {
        let dir = tempfile::tempdir().unwrap();
        let karabiner_path = dir.path().join("isolated-karabiner.json");
        let target = TargetKeyboard {
            vendor_id: 0x303a,
            product_id: 0x8298,
            location_id: 0x0110_0000,
            usb_parent: 0x1000_0001,
            vendor_path: "isolated-vendor".into(),
        };
        let receiver = UserCommandReceiver::bind().unwrap();
        let initial_karabiner = serde_json::to_vec_pretty(&serde_json::json!({
            "profiles": [
                {"name": "Default", "selected": true, "complex_modifications": {"rules": []}, "devices": []},
                {"name": "Work", "selected": false, "complex_modifications": {"rules": []}, "devices": []}
            ]
        }))
        .unwrap();
        fs::write(&karabiner_path, &initial_karabiner).unwrap();
        let rule = karabiner::rules(&target, &receiver, BottomRowLayout::Split).unwrap();
        let prev_patch =
            karabiner::prepare_config(karabiner_path.clone(), initial_karabiner, &target, rule)
                .unwrap();
        fs::write(&karabiner_path, &prev_patch.installed).unwrap();

        let saved = SavedSixLayerState {
            installation: Some(Installation {
                status: InstallationStatus::Applied,
                plan: test_plan(),
                target: target.clone(),
                profile_index: 1,
                backup: dir.path().join("backup"),
                config: prev_patch,
            }),
            ..SavedSixLayerState::default()
        };

        let same_profile =
            prepare_apply_karabiner_patch(&saved, &target, &receiver, BottomRowLayout::Split)
                .unwrap();
        assert_eq!(same_profile.path, karabiner_path);
        assert_eq!(same_profile.profile_name, "Default");

        let drifted_karabiner = serde_json::to_vec_pretty(&serde_json::json!({
            "profiles": [
                {"name": "Default", "selected": false, "complex_modifications": {"rules": []}, "devices": []},
                {"name": "Work", "selected": true, "complex_modifications": {"rules": []}, "devices": []}
            ]
        }))
        .unwrap();
        fs::write(&karabiner_path, &drifted_karabiner).unwrap();
        let err = prepare_apply_karabiner_patch(&saved, &target, &receiver, BottomRowLayout::Split)
            .unwrap_err();
        assert_eq!(
            err,
            "原 Karabiner 方案身份已变化；请先恢复原键盘配置后再应用"
        );
    }
}

#[cfg(test)]
pub(crate) fn test_b64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(b2 & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
pub(crate) fn test_b64_decode(raw: &str) -> Vec<u8> {
    let val = |c: u8| -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };
    let clean: Vec<u8> = raw.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let mut out = Vec::new();
    for chunk in clean.chunks(4) {
        let a = val(chunk[0]).unwrap();
        let b = val(chunk[1]).unwrap();
        out.push((a << 2) | (b >> 4));
        if chunk.get(2).copied() != Some(b'=') {
            if let Some(c) = chunk.get(2).copied().and_then(val) {
                out.push((b << 4) | (c >> 2));
                if chunk.get(3).copied() != Some(b'=') {
                    if let Some(d) = chunk.get(3).copied().and_then(val) {
                        out.push((c << 6) | d);
                    }
                }
            }
        }
    }
    out
}
