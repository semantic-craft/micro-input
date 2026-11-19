mod encoder_dispatch;
mod lifecycle;
mod state;
mod vendor_bridge;

#[cfg(test)]
mod tests;

pub use lifecycle::{apply, restore, resume};
#[cfg(test)]
pub(crate) use lifecycle::{
    inspect_resume_snapshot, merge_previous_original_into_backup, ResumeSnapshotAction,
};
pub use state::{
    load, save, HostAction, Installation, InstallationStatus, PinnedProfilePlan,
    SavedSixLayerState, SixLayerRoutingMode, SLOTS_PER_LAYER,
};
pub(crate) use state::{ExecutionAdmission, ExecutionPolicy};
#[cfg(test)]
pub(crate) use tests::{test_b64_decode, test_b64_encode};
pub use vendor_bridge::activation_to_input_event;
pub(crate) use vendor_bridge::{VendorBridge, VendorBridgePreviewLease, WireSyncState};

#[cfg(test)]
use crate::karabiner::ConfigPatch;
use crate::karabiner::Readiness;
use crate::store::session_navigation::NavigationDispatch;
use encoder_dispatch::execute_clicks_with_encoder_picker;
#[cfg(test)]
use lifecycle::new_running;
use mi_model::apply::AppliedSixLayerPlan;
use mi_model::host_router::ResolvedHostAction;
use mi_proto::lighting_candidate::OaiKeyStatusEntry;
use mi_session::DeviceSession;
use mi_transport::{HidCandidate, MacTransport, TargetKeyboard, UserCommandReceiver};
use std::{
    fs,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime},
};
use vendor_bridge::VendorBridgeSpawnConfig;

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
    firmware_version: Option<String>,
    transport: Option<Arc<MacTransport>>,
    stop: Arc<AtomicBool>,
    monitor: Option<JoinHandle<()>>,
    executor: Option<JoinHandle<Result<(), String>>>,
}

static CODEX_SLOT_15_FALLBACK: std::sync::LazyLock<ResolvedHostAction> =
    std::sync::LazyLock::new(|| ResolvedHostAction::AgentCommand {
        adapter: "codex".into(),
        command: "model-picker-open".into(),
    });

#[derive(Clone, Debug, Default)]
pub(crate) struct RoutingContext {
    pub mode: SixLayerRoutingMode,
    pub bundle_id: Option<String>,
}

impl RoutingContext {
    fn update(&mut self, bundle_id: Option<&str>) -> bool {
        if self.bundle_id.as_deref() == bundle_id {
            return false;
        }
        self.bundle_id = bundle_id.map(str::to_owned);
        matches!(self.mode, SixLayerRoutingMode::BoardLayers { .. })
    }

    fn set_routing(&mut self, mode: SixLayerRoutingMode, bundle_id: Option<&str>) -> bool {
        let mut changed = false;
        if self.bundle_id.as_deref() != bundle_id {
            self.bundle_id = bundle_id.map(str::to_owned);
            changed = true;
        }
        if self.mode != mode {
            self.mode = mode;
            changed = true;
        }
        changed
    }

    fn accepts_layer1_vendor_fallback(&self) -> bool {
        match &self.mode {
            SixLayerRoutingMode::Locked(pinned) | SixLayerRoutingMode::ManualPinned(pinned) => {
                pinned.has_any_action()
            }
            SixLayerRoutingMode::BoardLayers { layer_one, .. } => layer_one
                .as_deref()
                .is_some_and(PinnedProfilePlan::has_any_action),
        }
    }

    fn effective_profile_id_str<'a>(
        &'a self,
        plan: &'a AppliedSixLayerPlan,
        physical_layer: u8,
    ) -> Option<&'a str> {
        match &self.mode {
            SixLayerRoutingMode::Locked(pinned) => Some(pinned.profile_id.as_str()),
            SixLayerRoutingMode::ManualPinned(pinned) => {
                if pinned.bundle_id.as_deref() == Some("com.openai.codex")
                    && !pinned.has_any_action()
                {
                    None
                } else {
                    Some(pinned.profile_id.as_str())
                }
            }
            SixLayerRoutingMode::BoardLayers {
                layer_one,
                layer_two,
                layer_three,
            } => match physical_layer {
                1 => layer_one.as_ref().map(|p| p.profile_id.as_str()),
                2 => layer_two
                    .as_ref()
                    .map(|p| p.profile_id.as_str())
                    .or_else(|| plan.hosts.first().map(|h| h.profile_id.as_str())),
                3 => layer_three
                    .as_ref()
                    .map(|p| p.profile_id.as_str())
                    .or_else(|| plan.hosts.get(1).map(|h| h.profile_id.as_str())),
                _ => None,
            },
        }
    }

    fn effective_profile_id(
        &self,
        plan: &AppliedSixLayerPlan,
        physical_layer: u8,
    ) -> Option<String> {
        self.effective_profile_id_str(plan, physical_layer)
            .map(str::to_owned)
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
            SixLayerRoutingMode::BoardLayers {
                layer_one,
                layer_two,
                layer_three,
            } => match control_layer {
                1 => {
                    if let Some(layer1) = layer_one.as_ref() {
                        let action = layer1.actions.get(slot)?.as_ref()?;
                        Some((layer1.profile_id.as_str(), action))
                    } else if slot == 15 {
                        Some(("codex", &*CODEX_SLOT_15_FALLBACK))
                    } else {
                        None
                    }
                }
                2 => {
                    if let Some(layer2) = layer_two.as_ref() {
                        let action = layer2.actions.get(slot)?.as_ref()?;
                        return Some((layer2.profile_id.as_str(), action));
                    }
                    let host = plan.hosts.first()?;
                    let action = host.actions.get(slot)?.as_ref()?;
                    Some((host.profile_id.as_str(), action))
                }
                3 => {
                    if let Some(layer3) = layer_three.as_ref() {
                        let action = layer3.actions.get(slot)?.as_ref()?;
                        return Some((layer3.profile_id.as_str(), action));
                    }
                    let host = plan.hosts.get(1)?;
                    let action = host.actions.get(slot)?.as_ref()?;
                    Some((host.profile_id.as_str(), action))
                }
                _ => None,
            },
        }
    }

    fn resolve_encoder_hold(
        &self,
        plan: &AppliedSixLayerPlan,
        control_layer: u8,
    ) -> Option<(String, ResolvedHostAction)> {
        match &self.mode {
            SixLayerRoutingMode::Locked(pinned) | SixLayerRoutingMode::ManualPinned(pinned) => {
                if !(1..=4).contains(&control_layer) {
                    return None;
                }
                if matches!(self.mode, SixLayerRoutingMode::ManualPinned(_))
                    && pinned.bundle_id.as_deref() == Some("com.openai.codex")
                    && !pinned.has_any_action()
                {
                    return None;
                }
                pinned
                    .encoder_hold_action
                    .as_ref()
                    .map(|a| (pinned.profile_id.clone(), a.clone()))
            }
            SixLayerRoutingMode::BoardLayers {
                layer_one,
                layer_two,
                layer_three,
            } => match control_layer {
                1 => {
                    if let Some(layer1) = layer_one.as_ref() {
                        layer1
                            .encoder_hold_action
                            .as_ref()
                            .map(|a| (layer1.profile_id.clone(), a.clone()))
                    } else {
                        Some((
                            "codex".to_string(),
                            ResolvedHostAction::AgentCommand {
                                adapter: "codex".into(),
                                command: "search".into(),
                            },
                        ))
                    }
                }
                2 => {
                    if let Some(layer2) = layer_two.as_ref() {
                        return layer2
                            .encoder_hold_action
                            .as_ref()
                            .map(|a| (layer2.profile_id.clone(), a.clone()));
                    }
                    plan.hosts.first().and_then(|h| {
                        h.encoder_hold_action
                            .as_ref()
                            .map(|a| (h.profile_id.clone(), a.clone()))
                    })
                }
                3 => {
                    if let Some(layer3) = layer_three.as_ref() {
                        return layer3
                            .encoder_hold_action
                            .as_ref()
                            .map(|a| (layer3.profile_id.clone(), a.clone()));
                    }
                    plan.hosts.get(1).and_then(|h| {
                        h.encoder_hold_action
                            .as_ref()
                            .map(|a| (h.profile_id.clone(), a.clone()))
                    })
                }
                _ => None,
            },
        }
    }

    fn resolve_layer_adapter<'a>(
        &'a self,
        plan: &'a AppliedSixLayerPlan,
        control_layer: u8,
    ) -> Option<(&'a str, &'static str)> {
        fn adapter_from_bundle_id(bundle_id: Option<&str>) -> Option<&'static str> {
            match bundle_id.map(str::trim) {
                Some("com.google.antigravity") => Some("antigravity"),
                Some("com.openai.codex") => Some("codex"),
                Some("com.deepseek.dsh" | "com.deepseek.dsh.desktop") => Some("dsh-desktop"),
                _ => None,
            }
        }
        fn adapter_from_name(adapter: &str) -> Option<&'static str> {
            match adapter.trim() {
                "antigravity" => Some("antigravity"),
                "codex" => Some("codex"),
                "dsh-desktop" => Some("dsh-desktop"),
                _ => None,
            }
        }
        match &self.mode {
            SixLayerRoutingMode::Locked(pinned) | SixLayerRoutingMode::ManualPinned(pinned) => {
                if !(1..=4).contains(&control_layer) {
                    return None;
                }
                if matches!(self.mode, SixLayerRoutingMode::ManualPinned(_))
                    && pinned.bundle_id.as_deref() == Some("com.openai.codex")
                    && !pinned.has_any_action()
                {
                    return None;
                }
                let adapter = adapter_from_bundle_id(pinned.bundle_id.as_deref())
                    .or_else(|| {
                        pinned
                            .encoder_hold_action
                            .iter()
                            .chain(pinned.actions.iter().flatten())
                            .find_map(|action| match action {
                                ResolvedHostAction::AgentCommand { adapter, .. } => {
                                    adapter_from_name(adapter)
                                }
                                _ => None,
                            })
                    })
                    .or_else(|| adapter_from_bundle_id(self.bundle_id.as_deref()))?;
                Some((pinned.profile_id.as_str(), adapter))
            }
            SixLayerRoutingMode::BoardLayers {
                layer_one,
                layer_two,
                layer_three,
            } => match control_layer {
                1 => {
                    let pid = layer_one
                        .as_ref()
                        .map(|l| l.profile_id.as_str())
                        .unwrap_or("codex");
                    Some((pid, "codex"))
                }
                2 => {
                    if let Some(layer2) = layer_two.as_ref() {
                        let adapter = adapter_from_bundle_id(layer2.bundle_id.as_deref())
                            .or_else(|| {
                                plan.hosts
                                    .first()
                                    .and_then(|h| adapter_from_name(h.adapter.as_str()))
                            })
                            .unwrap_or("antigravity");
                        return Some((layer2.profile_id.as_str(), adapter));
                    }
                    let host = plan.hosts.first()?;
                    let adapter = adapter_from_name(host.adapter.as_str())?;
                    Some((host.profile_id.as_str(), adapter))
                }
                3 => {
                    if let Some(layer3) = layer_three.as_ref() {
                        let adapter = adapter_from_bundle_id(layer3.bundle_id.as_deref())
                            .or_else(|| {
                                plan.hosts
                                    .get(1)
                                    .and_then(|h| adapter_from_name(h.adapter.as_str()))
                            })
                            .unwrap_or("dsh-desktop");
                        return Some((layer3.profile_id.as_str(), adapter));
                    }
                    let host = plan.hosts.get(1)?;
                    let adapter = adapter_from_name(host.adapter.as_str())?;
                    Some((host.profile_id.as_str(), adapter))
                }
                _ => None,
            },
        }
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

    pub fn effective_profile_id(&self) -> Option<String> {
        let layer = self.observed_layer.load(Ordering::Acquire);
        self.routing
            .lock()
            .unwrap()
            .effective_profile_id(&self.installation.plan, layer)
    }

    pub(crate) fn effective_profile_id_eq(&self, expected: Option<&str>) -> bool {
        let layer = self.observed_layer.load(Ordering::Acquire);
        self.routing
            .lock()
            .unwrap()
            .effective_profile_id_str(&self.installation.plan, layer)
            == expected
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
            serial_number: None,
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

    pub fn observed_firmware_version(&self) -> Option<String> {
        if self.is_retired() || self.receiver.failure().is_some() {
            return None;
        }
        self.vendor_bridge
            .active_session
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|session| session.observed_firmware_version())
            .or_else(|| self.firmware_version.clone())
    }

    pub(crate) fn observed_firmware_version_eq(&self, expected: Option<&str>) -> bool {
        if self.is_retired() || self.receiver.failure().is_some() {
            return expected.is_none();
        }
        let guard = self.vendor_bridge.active_session.lock().unwrap();
        if let Some(version) = guard
            .as_ref()
            .and_then(|session| session.observed_firmware_version())
        {
            Some(version.as_str()) == expected
        } else {
            self.firmware_version.as_deref() == expected
        }
    }

    pub fn vendor_bridge_connected(&self) -> bool {
        self.vendor_bridge.connected.load(Ordering::Acquire)
    }

    pub fn live_vendor_session(&self) -> Option<Arc<DeviceSession>> {
        if self.is_retired() || !self.vendor_bridge_connected() {
            return None;
        }
        self.vendor_bridge
            .active_session
            .lock()
            .unwrap()
            .as_ref()
            .filter(|session| !session.is_disconnected() && session.has_exclusive_vendor_link())
            .cloned()
    }

    pub(crate) fn acquire_preview_session(
        &self,
    ) -> Result<(Arc<DeviceSession>, VendorBridgePreviewLease), String> {
        if self.is_retired() || self.receiver.failure().is_some() {
            return Err("六层运行代次已失效，无法启动灯光预览".to_string());
        }
        self.vendor_bridge.acquire_preview_lease()
    }

    pub(crate) fn set_v070_inbox(&self, inbox: crate::store::runtime_inbox::RuntimeInbox) {
        *self.vendor_bridge.v070_inbox.lock().unwrap() = Some(inbox);
    }

    pub(crate) fn vendor_session_id(&self) -> Option<u64> {
        if let Some(id) = self
            .vendor_bridge
            .active_session
            .lock()
            .unwrap()
            .as_ref()
            .map(|session| session.id)
        {
            return Some(id);
        }
        self.vendor_bridge
            .retired_session
            .lock()
            .unwrap()
            .as_ref()
            .map(|session| session.id)
    }

    pub(crate) fn send_v070_command(
        &self,
        cmd: crate::store::v070_hardware_store::V070BridgeCommand,
    ) -> Result<(), String> {
        let tx = self
            .vendor_bridge
            .v070_cmd_tx
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| "USB 专用桥接尚未就绪".to_string())?;
        tx.try_send(cmd)
            .map_err(|_| "USB 专用桥接指令队列繁忙或已关闭".to_string())
    }

    pub(crate) fn observed_v070_profile_layer(&self) -> Option<(u8, u8)> {
        *self
            .vendor_bridge
            .v070_observed_profile_layer
            .lock()
            .unwrap()
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
            VendorBridgeSpawnConfig {
                initial_session: initial,
                receiver: Arc::clone(&self.receiver),
                routing: Arc::clone(&self.routing),
                target: self.installation.target.clone(),
                preferred_serial: self.candidate.serial_number.clone(),
                transport: self.transport.clone(),
                allow_reconnect: true,
            },
        );
    }

    pub fn attach_test_vendor_session(
        &mut self,
        runtime: &tokio::runtime::Runtime,
        session: Arc<DeviceSession>,
    ) {
        if self.firmware_version.is_none() {
            self.firmware_version = session.observed_firmware_version();
        }
        self.vendor_bridge.spawn(
            runtime,
            VendorBridgeSpawnConfig {
                initial_session: Some(session),
                receiver: Arc::clone(&self.receiver),
                routing: Arc::clone(&self.routing),
                target: self.installation.target.clone(),
                preferred_serial: self.candidate.serial_number.clone(),
                transport: None,
                allow_reconnect: false,
            },
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
            execute_clicks_with_encoder_picker(
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
                |enabled| executor.set_model_picker_mouse_capture(enabled),
                || executor.drain_model_picker_mouse_deltas(),
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

    #[cfg(test)]
    pub(crate) fn set_executor_handle_for_test(&mut self, handle: JoinHandle<Result<(), String>>) {
        self.executor = Some(handle);
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
        let preferred_serial = self.candidate.serial_number.clone();
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
                        preferred_serial.as_deref(),
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
    preferred_serial: Option<&str>,
    receiver_path: &Path,
    patch_path: &Path,
    endpoint_marker: &str,
    last_patch_stamp: &mut Option<(Option<SystemTime>, u64)>,
) -> Result<(), String> {
    match mi_transport::discover_target_keyboard_preferred(preferred_serial) {
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
