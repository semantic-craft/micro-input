use super::lifecycle::connect;
use super::RoutingContext;
use mi_model::direct_controls::{CONTROL_BINDINGS, DIRECT_CONTROLS};
use mi_proto::lighting_candidate::OaiKeyStatusEntry;
use mi_session::DeviceSession;
use mi_transport::{MacTransport, TargetKeyboard, UserCommandReceiver};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum WireSyncState {
    #[default]
    Unsynced,
    InFlight(String),
    Confirmed(String),
}

async fn open_vendor_session(
    transport: Arc<MacTransport>,
    target: TargetKeyboard,
    preferred_serial: Option<String>,
) -> Option<Arc<DeviceSession>> {
    let session = tokio::task::spawn_blocking(move || {
        mi_transport::discover_target_keyboard_preferred(preferred_serial.as_deref())
            .ok()
            .filter(|d| d.location_id == target.location_id)
            .and_then(|d| connect(transport.as_ref(), &d).ok())
            .map(|(session, _)| session)
    })
    .await
    .ok()
    .flatten()?;
    let _ = tokio::time::timeout(Duration::from_millis(750), session.read_identity_info()).await;
    Some(session)
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

enum VendorSlotDecode {
    Edge {
        phys_slot: u8,
        control: Option<u8>,
        layer1: bool,
        pressed: bool,
    },
    Step(u8),
    RadialSector(u8),
    RadialRelease,
}

pub(super) fn handle_vendor_slot_notification(
    received: &mi_session::ReceivedNotification,
    pressed_mask: &mut u64,
    receiver: &UserCommandReceiver,
    layer1_fallback: bool,
) {
    match decode_six_layer_vendor_slot(&received.notification, layer1_fallback) {
        Some(VendorSlotDecode::Edge {
            phys_slot,
            control,
            layer1,
            pressed,
        }) => {
            let bit = 1u64 << phys_slot;
            if pressed {
                if (*pressed_mask & bit) == 0 {
                    *pressed_mask |= bit;
                    if let Some(control) = control {
                        let _ = if layer1 {
                            receiver.push_layer1_vendor_activation(control)
                        } else {
                            receiver.push_vendor_activation(control)
                        };
                    }
                }
            } else {
                let was_pressed = (*pressed_mask & bit) != 0;
                *pressed_mask &= !bit;
                if was_pressed {
                    if let Some(control) = control {
                        let is_enc_clk = DIRECT_CONTROLS
                            .get(control as usize)
                            .and_then(|c| CONTROL_BINDINGS.get(c.slot as usize).copied())
                            == Some("ENC_CLK");
                        if is_enc_clk {
                            let _ = if layer1 {
                                receiver.push_layer1_vendor_release(control)
                            } else {
                                receiver.push_vendor_release(control)
                            };
                        }
                    }
                }
            }
        }
        Some(VendorSlotDecode::Step(control)) => {
            let _ = receiver.push_layer1_vendor_activation(control);
        }
        Some(VendorSlotDecode::RadialSector(slot)) => {
            let radial_bits = 0b1111u64 << 28;
            let bit = 1u64 << (28 + (slot - 16));
            if (*pressed_mask & radial_bits) == 0 {
                *pressed_mask |= bit;
                let _ = receiver.push_layer1_vendor_activation(slot);
            }
        }
        Some(VendorSlotDecode::RadialRelease) => {
            *pressed_mask &= !(0b1111u64 << 28);
        }
        None => {}
    }
}

pub fn activation_to_input_event(
    event: &mi_transport::Activation,
    session_id: u64,
) -> Option<mi_session::InputEvent> {
    let control = DIRECT_CONTROLS.get(event.control as usize)?;
    let binding = *CONTROL_BINDINGS.get(control.slot as usize)?;
    let board_layer = event.layer_override.unwrap_or(control.layer);
    let kind = if event.released {
        mi_session::InputEventKind::Released
    } else {
        mi_session::InputEventKind::Pressed
    };
    Some(mi_session::InputEvent {
        session_id,
        sequence: event.received_sequence,
        ingress_sequence: event.received_sequence,
        board_layer: Some(board_layer),
        radial_center: None,
        radial_position: None,
        timestamp_ms: 0,
        kind,
        logical_switch: Some(binding.to_string()),
        binding_id: Some(binding.to_string()),
        physical_control: None,
        keycap_group: None,
        action_role: None,
        lost_count: 0,
        action_eligible: true,
    })
}

fn forward_vendor_cheat_sheet_notification(
    received: &mi_session::ReceivedNotification,
    session_id: u64,
    v070_inbox: &Mutex<Option<crate::store::runtime_inbox::RuntimeInbox>>,
) {
    let kind = match received.notification {
        mi_proto::HostNotification::CheatSheetShow(_) => crate::CheatSheetNotificationKind::Show,
        mi_proto::HostNotification::CheatSheetHide(_) => crate::CheatSheetNotificationKind::Hide,
        mi_proto::HostNotification::CheatSheetToggle(_) => {
            crate::CheatSheetNotificationKind::Toggle
        }
        _ => return,
    };
    if let Some(inbox) = v070_inbox.lock().unwrap().as_ref().cloned() {
        let _ = inbox.send(crate::store::HardwareLoopEvent::CheatSheetNotification {
            session_id,
            ingress_sequence: received.ingress_sequence,
            kind,
        });
    }
}

fn forward_v070_profile_layer_notification(
    received: &mi_session::ReceivedNotification,
    session_id: u64,
    observed_layer: &AtomicU8,
    v070_observed_profile_layer: &Mutex<Option<(u8, u8)>>,
    v070_inbox: &Mutex<Option<crate::store::runtime_inbox::RuntimeInbox>>,
) {
    if let mi_proto::HostNotification::ProfileLayer(ref raw) = received.notification {
        if let Some((profile_index, layer_index)) =
            mi_proto::v070_capabilities::parse_profile_layer_notification(raw)
        {
            if (1..=6).contains(&layer_index) {
                observed_layer.store(layer_index, Ordering::Release);
            }
            *v070_observed_profile_layer.lock().unwrap() = Some((profile_index, layer_index));
            if let Some(inbox) = v070_inbox.lock().unwrap().as_ref().cloned() {
                let _ = inbox.send(crate::store::HardwareLoopEvent::V070ProfileLayerChanged {
                    session_id,
                    profile_index,
                    layer_index,
                });
            }
        }
    }
}

pub(super) fn record_v070_bridge_observation(
    outcome: &crate::v070_hardware::V070BridgeOutcome,
    observed_layer: &AtomicU8,
    v070_observed_profile_layer: &Mutex<Option<(u8, u8)>>,
) {
    match outcome {
        crate::v070_hardware::V070BridgeOutcome::Snapshot(snap) => {
            if let Some(layer) = snap.active_layer.filter(|l| (1..=6).contains(l)) {
                observed_layer.store(layer, Ordering::Release);
            }
            if let Some((p, l)) = snap.active_profile.zip(snap.active_layer) {
                *v070_observed_profile_layer.lock().unwrap() = Some((p, l));
            }
        }
        crate::v070_hardware::V070BridgeOutcome::ProfileLayer {
            profile_index,
            layer_index,
        } => {
            if (1..=6).contains(layer_index) {
                observed_layer.store(*layer_index, Ordering::Release);
            }
            let mut guard = v070_observed_profile_layer.lock().unwrap();
            if let Some(p) = profile_index.or_else(|| guard.map(|(p, _)| p)) {
                *guard = Some((p, *layer_index));
            }
        }
        _ => {}
    }
}

pub(crate) struct VendorBridge {
    pub(super) led_tx: tokio::sync::watch::Sender<Option<(String, Vec<OaiKeyStatusEntry>)>>,
    stop_tx: tokio::sync::watch::Sender<bool>,
    pub(super) connected: Arc<AtomicBool>,
    pub(super) sync_state: Arc<Mutex<WireSyncState>>,
    pub(super) retry_not_before: Arc<Mutex<Option<Instant>>>,
    pub(super) active_session: Arc<Mutex<Option<Arc<DeviceSession>>>>,
    pub(super) retired_session: Mutex<Option<Arc<DeviceSession>>>,
    pub(super) observed_layer: Arc<AtomicU8>,
    pub(super) v070_cmd_tx: Arc<
        Mutex<
            Option<tokio::sync::mpsc::Sender<crate::store::v070_hardware_store::V070BridgeCommand>>,
        >,
    >,
    pub(super) v070_inbox: Arc<Mutex<Option<crate::store::runtime_inbox::RuntimeInbox>>>,
    pub(super) v070_observed_profile_layer: Arc<Mutex<Option<(u8, u8)>>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

pub(super) struct VendorBridgeSpawnConfig {
    pub(super) initial_session: Option<Arc<DeviceSession>>,
    pub(super) receiver: Arc<UserCommandReceiver>,
    pub(super) routing: Arc<Mutex<RoutingContext>>,
    pub(super) target: TargetKeyboard,
    pub(super) preferred_serial: Option<String>,
    pub(super) transport: Option<Arc<MacTransport>>,
    pub(super) allow_reconnect: bool,
}

impl VendorBridge {
    pub(super) fn new(initial_session: Option<Arc<DeviceSession>>) -> Self {
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
            observed_layer: Arc::new(AtomicU8::new(1)),
            v070_cmd_tx: Arc::new(Mutex::new(None)),
            v070_inbox: Arc::new(Mutex::new(None)),
            v070_observed_profile_layer: Arc::new(Mutex::new(None)),
            task: None,
        }
    }

    pub(super) fn confirmed_digest(&self) -> Option<String> {
        match &*self.sync_state.lock().unwrap() {
            WireSyncState::Confirmed(digest) => Some(digest.clone()),
            WireSyncState::Unsynced | WireSyncState::InFlight(_) => None,
        }
    }

    pub(super) fn retry_ready(&self) -> bool {
        self.retry_not_before
            .lock()
            .unwrap()
            .is_none_or(|deadline| Instant::now() >= deadline)
    }

    pub(super) fn needs_lighting_sync(&self) -> bool {
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

    pub(super) fn retire(&self) {
        let _ = self.stop_tx.send(true);
        self.connected.store(false, Ordering::Release);
        *self.sync_state.lock().unwrap() = WireSyncState::Unsynced;
        *self.retry_not_before.lock().unwrap() = None;
        *self.v070_cmd_tx.lock().unwrap() = None;
        if let Some(session) = self.active_session.lock().unwrap().take() {
            session.disconnect();
            *self.retired_session.lock().unwrap() = Some(session);
        }
    }

    pub(super) fn shutdown(&mut self, runtime: &tokio::runtime::Runtime) {
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

    pub(super) fn spawn(
        &mut self,
        runtime: &tokio::runtime::Runtime,
        cfg: VendorBridgeSpawnConfig,
    ) {
        let VendorBridgeSpawnConfig {
            initial_session,
            receiver,
            routing,
            target,
            preferred_serial,
            transport,
            allow_reconnect,
        } = cfg;
        self.spawn_with_reopen(
            runtime,
            initial_session,
            receiver,
            routing,
            allow_reconnect,
            move || {
                let transport = transport.clone();
                let target = target.clone();
                let preferred_serial = preferred_serial.clone();
                async move { open_vendor_session(transport?, target, preferred_serial).await }
            },
        );
    }

    pub(super) fn spawn_with_reopen<F, Fut>(
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
        let observed_layer = Arc::clone(&self.observed_layer);
        let v070_inbox = Arc::clone(&self.v070_inbox);
        let v070_observed_profile_layer = Arc::clone(&self.v070_observed_profile_layer);
        let (v070_cmd_tx, mut v070_cmd_rx) =
            tokio::sync::mpsc::channel::<crate::store::v070_hardware_store::V070BridgeCommand>(16);
        *self.v070_cmd_tx.lock().unwrap() = Some(v070_cmd_tx);
        let v070_cmd_tx_slot = Arc::clone(&self.v070_cmd_tx);
        self.task = Some(runtime.spawn(async move {
            let mut current_session = initial_session;
            let mut pending_notif_rx = initial_notif_rx;
            let mut pressed_mask: u64 = 0;
            let mut rotated_from_live = false;
            let mut last_v070_snapshot_session: Option<u64> = None;
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
                if session_ok
                    && session.request_count() < 900
                    && session.has_exclusive_vendor_link()
                    && session
                        .observed_firmware_version()
                        .as_deref()
                        .is_some_and(mi_model::supports_v070_hardware_capabilities)
                    && last_v070_snapshot_session != Some(session.id)
                {
                    last_v070_snapshot_session = Some(session.id);
                    let snap_session = Arc::clone(&session);
                    let snap_observed_layer = Arc::clone(&observed_layer);
                    let snap_v070_pl = Arc::clone(&v070_observed_profile_layer);
                    let snap_inbox = Arc::clone(&v070_inbox);
                    tokio::spawn(async move {
                        let outcome =
                            crate::store::v070_hardware_store::fetch_v070_snapshot_on_session(
                                &snap_session,
                                false,
                            )
                            .await
                            .map(Box::new)
                            .map(crate::v070_hardware::V070BridgeOutcome::Snapshot);
                        if let Ok(ref bridge_outcome) = outcome {
                            record_v070_bridge_observation(
                                bridge_outcome,
                                &snap_observed_layer,
                                &snap_v070_pl,
                            );
                        }
                        if let Some(inbox) = snap_inbox.lock().unwrap().as_ref().cloned() {
                            let _ = inbox.send(
                                crate::store::HardwareLoopEvent::V070CapabilityOutcome {
                                    session_id: snap_session.id,
                                    seq: 0,
                                    result: outcome,
                                },
                            );
                        }
                    });
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
                            forward_v070_profile_layer_notification(
                                &received,
                                session.id,
                                &observed_layer,
                                &v070_observed_profile_layer,
                                &v070_inbox,
                            );
                            forward_vendor_cheat_sheet_notification(
                                &received,
                                session.id,
                                &v070_inbox,
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
                                    forward_v070_profile_layer_notification(
                                        &received,
                                        session.id,
                                        &observed_layer,
                                        &v070_observed_profile_layer,
                                        &v070_inbox,
                                    );
                                    forward_vendor_cheat_sheet_notification(
                                        &received,
                                        session.id,
                                        &v070_inbox,
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
                        cmd_opt = v070_cmd_rx.recv() => {
                            if let Some(cmd) = cmd_opt {
                                let seq = cmd.seq;
                                let result = crate::store::v070_hardware_store::execute_v070_command_on_session(
                                    &session, &cmd,
                                )
                                .await;
                                if let Ok(ref bridge_outcome) = result {
                                    record_v070_bridge_observation(
                                        bridge_outcome,
                                        &observed_layer,
                                        &v070_observed_profile_layer,
                                    );
                                }
                                if session.is_disconnected() {
                                    session_ok = false;
                                }
                                if let Some(inbox) = v070_inbox.lock().unwrap().as_ref().cloned() {
                                    let _ = inbox.send(crate::store::HardwareLoopEvent::V070CapabilityOutcome {
                                        session_id: session.id,
                                        seq,
                                        result,
                                    });
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
                *v070_cmd_tx_slot.lock().unwrap() = None;
                while let Ok(cmd) = v070_cmd_rx.try_recv() {
                    if let Some(inbox) = v070_inbox.lock().unwrap().as_ref().cloned() {
                        let _ = inbox.send(crate::store::HardwareLoopEvent::V070CapabilityOutcome {
                            session_id: session.id,
                            seq: cmd.seq,
                            result: Err("USB 会话已断开".into()),
                        });
                    }
                }
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
                        layer1: true,
                        pressed,
                    }),
                    6..=11 => Some(VendorSlotDecode::Edge {
                        phys_slot: ag,
                        control: Some(ag - 6),
                        layer1: false,
                        pressed,
                    }),
                    12..=17 => Some(VendorSlotDecode::Edge {
                        phys_slot: ag,
                        control: Some(20 + (ag - 12)),
                        layer1: false,
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
                            phys_slot: 18 + (slot - 6),
                            control: Some(slot),
                            layer1: true,
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
                    phys_slot: 25,
                    control: Some(13),
                    layer1: true,
                    pressed: true,
                }),
                ("ENC_CC", 0) => Some(VendorSlotDecode::Edge {
                    phys_slot: 25,
                    control: Some(13),
                    layer1: true,
                    pressed: false,
                }),
                ("ENC_CW", 1) => Some(VendorSlotDecode::Edge {
                    phys_slot: 26,
                    control: Some(14),
                    layer1: true,
                    pressed: true,
                }),
                ("ENC_CW", 0) => Some(VendorSlotDecode::Edge {
                    phys_slot: 26,
                    control: Some(14),
                    layer1: true,
                    pressed: false,
                }),
                ("ENC_CLK", 1) => Some(VendorSlotDecode::Edge {
                    phys_slot: 27,
                    control: Some(15),
                    layer1: true,
                    pressed: true,
                }),
                ("ENC_CLK", 0) => Some(VendorSlotDecode::Edge {
                    phys_slot: 27,
                    control: Some(15),
                    layer1: true,
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
