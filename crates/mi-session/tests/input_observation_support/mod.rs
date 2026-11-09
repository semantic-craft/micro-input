use mi_session::{
    DeviceSchema, DeviceSession, InputEventKind, InputMonitor, ReadOnlySchema, SessionConfig,
    UnverifiedSchema,
};
use mi_transport::{HidCandidate, HidTransport, LinkBackend, MacTransport};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex as TokioMutex, Notify};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputKind {
    UnknownNotification,
    CandidatePressed,
    CandidateReleased,
    Pressed,
    Released,
    InputStreamGap,
}

#[derive(Clone, Debug)]
pub struct InputEventInfo {
    pub session_id: u64,
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub kind: InputKind,
    #[cfg(feature = "synthetic-backend")]
    pub logical_switch: Option<String>,
    pub binding_id: Option<String>,
    #[cfg(feature = "synthetic-backend")]
    pub physical_control: Option<String>,
    #[cfg(feature = "synthetic-backend")]
    pub keycap_group: Option<String>,
    pub lost_count: u64,
    #[cfg(feature = "synthetic-backend")]
    pub verification: String,
    #[cfg(feature = "synthetic-backend")]
    pub action_eligible: bool,
}

#[derive(Clone)]
pub struct DeviceUsage {
    pub page: u16,
    pub usage: u16,
}

#[derive(Clone)]
pub struct DeviceInfo {
    pub id: String,
    pub vid: u16,
    pub pid: u16,
    pub usage_pairs: Vec<DeviceUsage>,
    pub transport: String,
}

impl From<HidCandidate> for DeviceInfo {
    fn from(c: HidCandidate) -> Self {
        Self {
            id: c.path,
            vid: c.vid,
            pid: c.pid,
            usage_pairs: c
                .usage_pairs
                .into_iter()
                .map(|p| DeviceUsage {
                    page: p.page,
                    usage: p.usage,
                })
                .collect(),
            transport: format!("{:?}", c.transport_hint),
        }
    }
}

#[derive(Clone)]
pub struct IdentityInfo {
    pub session_id: u64,
    pub version: Option<String>,
    pub profile_index: Option<u32>,
    pub layer_index: Option<u32>,
    pub source_method: String,
    pub backend: String,
}

#[cfg(feature = "synthetic-backend")]
pub enum SyntheticDiscoveryState {
    ReconnectedDevice,
}

#[cfg(feature = "synthetic-backend")]
#[derive(Clone, Debug)]
pub enum SyntheticInputCase {
    CandidateBindings,
    UnsupportedVariants,
}

pub struct InputSubscription {
    monitor: TokioMutex<InputMonitor>,
    closed: AtomicBool,
    changed: Notify,
}

impl InputSubscription {
    pub async fn next_event(&self) -> Option<InputEventInfo> {
        if self.closed.load(Ordering::SeqCst) {
            return None;
        }
        let changed = self.changed.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        let mut monitor = tokio::select! {
            biased;
            _ = &mut changed => return None,
            guard = self.monitor.lock() => guard
        };
        if self.closed.load(Ordering::SeqCst) {
            monitor.close();
            return None;
        }
        let event = tokio::select! {
            biased;
            _ = changed => {
                monitor.close();
                return None;
            }
            event = monitor.next() => event
        }?;
        if self.closed.load(Ordering::SeqCst) {
            monitor.close();
            return None;
        }
        let kind = match event.kind {
            InputEventKind::UnknownNotification => InputKind::UnknownNotification,
            InputEventKind::CandidatePressed => InputKind::CandidatePressed,
            InputEventKind::CandidateReleased => InputKind::CandidateReleased,
            InputEventKind::Pressed => InputKind::Pressed,
            InputEventKind::Released => InputKind::Released,
            InputEventKind::InputStreamGap => InputKind::InputStreamGap,
        };
        #[cfg(feature = "synthetic-backend")]
        let verification = match kind {
            InputKind::Pressed | InputKind::Released if event.action_eligible => {
                "verified-host-routing-0.6.3"
            }
            InputKind::CandidatePressed | InputKind::CandidateReleased => {
                "candidate-dispatch-0.6.2-current-firmware-unverified"
            }
            _ => "input-schema-unverified",
        }
        .into();
        Some(InputEventInfo {
            session_id: event.session_id,
            sequence: event.sequence,
            timestamp_ms: event.timestamp_ms,
            kind,
            #[cfg(feature = "synthetic-backend")]
            logical_switch: event.logical_switch,
            binding_id: event.binding_id,
            #[cfg(feature = "synthetic-backend")]
            physical_control: event.physical_control,
            #[cfg(feature = "synthetic-backend")]
            keycap_group: event.keycap_group,
            lost_count: event.lost_count,
            #[cfg(feature = "synthetic-backend")]
            verification,
            #[cfg(feature = "synthetic-backend")]
            action_eligible: event.action_eligible,
        })
    }

    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.changed.notify_waiters();
        if let Ok(mut monitor) = self.monitor.try_lock() {
            monitor.close();
        }
    }
}

impl Drop for InputSubscription {
    fn drop(&mut self) {
        self.close();
    }
}

pub struct MicroInput {
    #[cfg(feature = "synthetic-backend")]
    draft: Mutex<mi_model::storage::DocumentStore>,
    #[cfg(feature = "synthetic-backend")]
    merge_wide_key: Arc<AtomicBool>,
    transport: Arc<dyn HidTransport>,
    session: Mutex<Option<Arc<DeviceSession>>>,
}

impl MicroInput {
    pub fn new() -> Arc<Self> {
        #[cfg(feature = "synthetic-backend")]
        let store = mi_model::storage::DocumentStore::default();
        #[cfg(feature = "synthetic-backend")]
        let merge_wide_key = Arc::new(AtomicBool::new(store.bottom_row_layout().is_merged()));
        Arc::new(Self {
            #[cfg(feature = "synthetic-backend")]
            draft: Mutex::new(store),
            #[cfg(feature = "synthetic-backend")]
            merge_wide_key,
            transport: Arc::new(MacTransport::new()),
            session: Mutex::new(None),
        })
    }

    #[cfg(feature = "synthetic-backend")]
    pub fn synthetic_discovery() -> Arc<Self> {
        let store = mi_model::storage::DocumentStore::default();
        let merge_wide_key = Arc::new(AtomicBool::new(store.bottom_row_layout().is_merged()));
        Arc::new(Self {
            draft: Mutex::new(store),
            merge_wide_key,
            transport: Arc::new(mi_transport::SyntheticDiscovery::default()),
            session: Mutex::new(None),
        })
    }

    fn current(&self) -> Result<Arc<DeviceSession>, String> {
        self.session
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| "disconnected".to_owned())
    }

    #[cfg(feature = "synthetic-backend")]
    pub async fn set_synthetic_discovery(
        &self,
        state: SyntheticDiscoveryState,
    ) -> Result<(), String> {
        let code = match state {
            SyntheticDiscoveryState::ReconnectedDevice => 3,
        };
        self.transport
            .set_synthetic_discovery(code)
            .map_err(|e| e.to_string())
    }

    pub async fn list_devices(&self) -> Result<Vec<DeviceInfo>, String> {
        let t = self.transport.clone();
        tokio::task::spawn_blocking(move || {
            t.enumerate()
                .map(|ds| ds.into_iter().map(Into::into).collect())
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
    }

    pub async fn connect(&self, id: String) -> Result<(), String> {
        if let Some(old) = self.session.lock().unwrap().take() {
            old.shutdown().await;
        }
        let t = self.transport.clone();
        let link = tokio::task::spawn_blocking(move || t.open(&id))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let supported = matches!(link.backend(), LinkBackend::MacHidapi267Numbered);
        #[cfg(feature = "synthetic-backend")]
        let supported = supported || matches!(link.backend(), LinkBackend::SyntheticNumbered);
        let framer: Box<dyn mi_proto::Framer> = if supported {
            Box::new(mi_proto::NativeFramer::default())
        } else {
            Box::new(mi_proto::UnimplementedFramer)
        };
        let schema: Arc<dyn DeviceSchema> = if supported {
            Arc::new(ReadOnlySchema)
        } else {
            Arc::new(UnverifiedSchema)
        };
        let s = DeviceSession::new(link, framer, schema, SessionConfig::default());
        *self.session.lock().unwrap() = Some(s);
        Ok(())
    }

    pub async fn disconnect(&self) {
        if let Some(s) = self.session.lock().unwrap().take() {
            s.shutdown().await;
        }
    }

    pub async fn shutdown(&self) {
        self.disconnect().await;
        let t = self.transport.clone();
        let _ = tokio::task::spawn_blocking(move || t.stop_discovery()).await;
    }

    pub async fn subscribe_input(&self) -> Result<Arc<InputSubscription>, String> {
        let session = self.current()?;
        Ok(Arc::new(InputSubscription {
            monitor: TokioMutex::new(session.subscribe_input()),
            closed: AtomicBool::new(false),
            changed: Notify::new(),
        }))
    }

    #[cfg(feature = "synthetic-backend")]
    pub async fn subscribe_verified_input(&self) -> Result<Arc<InputSubscription>, String> {
        let session = self.current()?;
        Ok(Arc::new(InputSubscription {
            monitor: TokioMutex::new(
                session.subscribe_verified_input_with_merge_flag(Arc::clone(&self.merge_wide_key)),
            ),
            closed: AtomicBool::new(false),
            changed: Notify::new(),
        }))
    }

    #[cfg(feature = "synthetic-backend")]
    pub async fn emit_synthetic_input(&self, count: u16) -> Result<(), String> {
        self.current()?;
        self.transport
            .emit_synthetic_input(count)
            .map_err(|e| e.to_string())
    }

    #[cfg(feature = "synthetic-backend")]
    pub async fn emit_synthetic_binding_event(
        &self,
        binding_id: String,
        pressed: bool,
    ) -> Result<(), String> {
        self.current()?;
        self.transport
            .emit_synthetic_binding(&binding_id, pressed)
            .map_err(|e| e.to_string())
    }

    #[cfg(feature = "synthetic-backend")]
    pub async fn emit_synthetic_input_case(
        &self,
        input_case: SyntheticInputCase,
    ) -> Result<(), String> {
        self.current()?;
        let code = match input_case {
            SyntheticInputCase::CandidateBindings => 0,
            SyntheticInputCase::UnsupportedVariants => 1,
        };
        self.transport
            .emit_synthetic_input_case(code)
            .map_err(|e| e.to_string())
    }

    pub async fn read_identity(&self) -> Result<IdentityInfo, String> {
        let s = self.current()?;
        let info = s.read_identity_info().await.map_err(|e| e.to_string())?;
        Ok(IdentityInfo {
            session_id: info.session_id,
            version: info.version,
            profile_index: info.profile_index,
            layer_index: info.layer_index,
            source_method: info.source_method,
            backend: s.backend_name().into(),
        })
    }

    pub async fn read_config(&self) -> Result<String, String> {
        let s = self.current()?;
        let c = s.read_config_snapshot().await.map_err(|e| e.to_string())?;
        serde_json::to_string(&c).map_err(|e| e.to_string())
    }

    #[cfg(feature = "synthetic-backend")]
    pub async fn install_coding_agent_presets(&self) -> Result<String, String> {
        let mut store = self.draft.lock().unwrap();
        let mut created = Vec::new();
        store
            .update(|d| {
                created = mi_model::preset::install_agent_starter_profiles(d)?;
                Ok(())
            })
            .map_err(|e| e.to_string())?;
        if store.host_active_profile_id().is_none() {
            let graph = &store.draft.document.editable.graph;
            let initial = graph
                .profile_order
                .iter()
                .find(|id| graph.profiles.get(*id).is_some_and(|p| p.name == "Cindy"))
                .cloned()
                .or_else(|| created.first().cloned());
            if let Some(initial) = initial {
                store
                    .set_host_active_profile(Some(initial))
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(json!({
            "config": store.draft.config,
            "host_active_profile_id": store.host_active_profile_id(),
        })
        .to_string())
    }

    #[cfg(feature = "synthetic-backend")]
    pub async fn set_host_active_profile(&self, profile_id: Option<String>) -> Result<(), String> {
        let mut store = self.draft.lock().unwrap();
        store
            .set_host_active_profile(profile_id)
            .map_err(|e| e.to_string())
    }

    #[cfg(feature = "synthetic-backend")]
    pub async fn set_host_execution_enabled(&self, enabled: bool) -> Result<(), String> {
        let mut store = self.draft.lock().unwrap();
        store
            .set_host_execution_enabled(enabled)
            .map_err(|e| e.to_string())
    }

    #[cfg(feature = "synthetic-backend")]
    pub fn resolve_input_action(&self, binding_id: String, is_pressed: bool) -> Option<String> {
        let store = self.draft.lock().unwrap();
        let store = store.host_store();
        if !store.host_execution_enabled() {
            return None;
        }
        let resolved = store.resolve_host_input(&binding_id, is_pressed)?;
        serde_json::to_string(&resolved).ok()
    }

    #[cfg(feature = "synthetic-backend")]
    pub fn host_routing_state(&self) -> String {
        self.draft.lock().unwrap().routing_state().to_string()
    }

    #[cfg(feature = "synthetic-backend")]
    pub async fn update_host_routing(&self, request_json: String) -> Result<String, String> {
        let request = serde_json::from_str(&request_json).map_err(|e| e.to_string())?;
        let response = {
            let mut store = self.draft.lock().unwrap();
            store.update_routing(request).map_err(|e| e.to_string())?;
            let merged = store.bottom_row_layout().is_merged();
            self.merge_wide_key.store(merged, Ordering::SeqCst);
            store.routing_state().to_string()
        };
        Ok(response)
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub async fn capture(sub: &InputSubscription, duration: Duration) -> Value {
    let started = now_ms();
    let deadline = tokio::time::Instant::now() + duration.min(Duration::from_secs(45));
    let mut events = Vec::new();
    let terminal = loop {
        match tokio::time::timeout_at(deadline, sub.next_event()).await {
            Err(_) => break "window_elapsed",
            Ok(None) => break "stream_closed",
            Ok(Some(event)) => {
                let class = match event.kind {
                    InputKind::CandidatePressed => "candidate_pressed",
                    InputKind::CandidateReleased => "candidate_released",
                    InputKind::UnknownNotification => "unknown_notification",
                    InputKind::InputStreamGap => "input_stream_gap",
                    _ => "unverified_input",
                };
                let gap = event.kind == InputKind::InputStreamGap;
                events.push(json!({"class":class,"binding_id":event.binding_id,
                    "session_id":event.session_id,"sequence":event.sequence,
                    "timestamp_ms":event.timestamp_ms,"lost_count":event.lost_count}));
                if gap {
                    break "input_stream_gap";
                }
                if events.len() == 2048 {
                    break "event_limit";
                }
            }
        }
    };
    sub.close();
    let count = events.len();
    json!({"capture_started_ms":started,"capture_finished_ms":now_ms(),
        "terminal":terminal,"event_count":count,"events":events,
        "outcome":if count == 0 {"silence_unconfirmed"} else {"notifications_observed_mapping_unverified"},
        "physical_mapping_verified":false,"action_eligible":false})
}

pub const OPT_IN: &str = "I_AUTHORIZE_ONE_READONLY_INPUT_OBSERVATION";
pub fn options(
    operator: Option<&str>,
    experiment: Option<&str>,
    seconds: Option<&str>,
) -> Result<(String, u64), &'static str> {
    if operator != Some(OPT_IN) {
        return Err("explicit observation opt-in absent");
    }
    let id = experiment.ok_or("experiment absent")?;
    if id.is_empty() || id.len() > 64 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err("invalid experiment identifier");
    }
    let seconds = seconds
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| (1..=45).contains(v))
        .ok_or("capture window must be 1..45 seconds")?;
    Ok((id.into(), seconds))
}
