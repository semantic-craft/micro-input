//! R-A02/R-A05: only high-level asynchronous operations cross the in-process UniFFI bridge.
mod input_monitor;
pub use input_monitor::*;
use mi_session::{DeviceSession, ReadOnlySchema, SessionConfig, UnverifiedSchema};
use mi_transport::{HidCandidate, HidTransport, MacTransport};
use std::sync::{Arc, Mutex};
uniffi::setup_scaffolding!();
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum BridgeError {
    #[error("设备已断开；请明确重新连接")]
    Disconnected,
    #[error("请求超时；可以手动重试")]
    Timeout,
    #[error("设备 I/O 失败，原因未确认；请检查连接后重试")]
    Unavailable,
    #[error("{message}")]
    Failed { message: String },
}
fn failure(e: impl std::fmt::Display) -> BridgeError {
    BridgeError::Failed {
        message: e.to_string(),
    }
}
fn document_response(
    draft: &mi_model::draft::Draft,
    selection: Option<(String, Option<String>)>,
) -> Result<String, BridgeError> {
    let hint = selection.map(|(p, l)| serde_json::json!({"profile_id":p,"layer_id":l}));
    let capabilities: std::collections::BTreeMap<_, _> = draft
        .edits
        .iter()
        .map(|(key, action)| (key, mi_model::action_library::capability(action)))
        .collect();
    serde_json::to_string(
        &serde_json::json!({"draft":draft,"config":draft.config,"selection_hint":hint,"macro_library":draft.macros(),"action_capabilities":capabilities}),
    )
    .map_err(failure)
}
fn session_failure(error: mi_session::SessionError) -> BridgeError {
    match error {
        mi_session::SessionError::Disconnected
        | mi_session::SessionError::Transport(mi_transport::TransportError::Disconnected) => {
            BridgeError::Disconnected
        }
        mi_session::SessionError::Timeout => BridgeError::Timeout,
        mi_session::SessionError::Transport(mi_transport::TransportError::Unavailable(_)) => {
            BridgeError::Unavailable
        }
        mi_session::SessionError::Config(_) => failure("设备配置读取失败；旧快照已过期，请重试"),
        error => failure(error),
    }
}
#[derive(Clone, uniffi::Record)]
pub struct DeviceUsage {
    pub page: u16,
    pub usage: u16,
}
#[cfg(feature = "synthetic-backend")]
#[derive(uniffi::Enum)]
pub enum SyntheticDiscoveryState {
    NoDevices,
    TwoDevices,
    Failure,
}
#[derive(Clone, uniffi::Record)]
pub struct DeviceInfo {
    pub id: String,
    pub product: String,
    pub vid: u16,
    pub pid: u16,
    pub usage_page: u16,
    pub usage: u16,
    pub usage_pairs: Vec<DeviceUsage>,
    pub verified_model: bool,
    pub transport: String,
}
impl From<HidCandidate> for DeviceInfo {
    fn from(c: HidCandidate) -> Self {
        Self {
            id: c.path,
            product: c.product,
            vid: c.vid,
            pid: c.pid,
            usage_page: c.usage_page,
            usage: c.usage,
            usage_pairs: c
                .usage_pairs
                .into_iter()
                .map(|p| DeviceUsage {
                    page: p.page,
                    usage: p.usage,
                })
                .collect(),
            verified_model: c.pid == 0x8298,
            transport: format!("{:?}", c.transport_hint),
        }
    }
}
#[derive(Clone, uniffi::Record)]
pub struct IdentityInfo {
    pub session_id: u64,
    pub version: Option<String>,
    pub profile_index: Option<u32>,
    pub layer_index: Option<u32>,
    pub source_method: String,
    pub verification: String,
    pub backend: String,
    pub raw_json: String,
}
#[uniffi::export(callback_interface)]
pub trait DeviceEvents: Send + Sync {
    fn changed(&self, devices: Vec<DeviceInfo>);
    fn failed(&self, message: String);
}
#[derive(Default)]
struct ConnectionState {
    generation: u64,
    pending: Option<String>,
    session: Option<(String, Arc<DeviceSession>)>,
}
// An OS open cannot be cancelled once spawn_blocking starts. Keep its result
// under RAII until the current generation has adopted it as a session.
struct OpenedLink(Option<Arc<dyn mi_transport::HidLink>>);
impl Drop for OpenedLink {
    fn drop(&mut self) {
        if let Some(link) = self.0.take() {
            link.close();
        }
    }
}
struct ConnectAttempt<'a> {
    state: &'a Mutex<ConnectionState>,
    generation: u64,
}
impl Drop for ConnectAttempt<'_> {
    fn drop(&mut self) {
        let mut state = self.state.lock().unwrap();
        if state.generation == self.generation && state.pending.is_some() {
            state.generation += 1;
            state.pending = None;
        }
    }
}
#[derive(uniffi::Object)]
pub struct MicroInput {
    draft: Mutex<mi_model::storage::DocumentStore>,
    transport: Arc<dyn HidTransport>,
    state: Mutex<ConnectionState>,
    connect_gate: tokio::sync::Mutex<()>,
    stopped: tokio::sync::watch::Sender<bool>,
}
#[uniffi::export(async_runtime = "tokio")]
impl MicroInput {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            draft: Mutex::new(mi_model::storage::DocumentStore::default()),
            transport: Arc::new(MacTransport::new()),
            state: Mutex::new(ConnectionState::default()),
            connect_gate: tokio::sync::Mutex::new(()),
            stopped: tokio::sync::watch::channel(false).0,
        })
    }
    // R-P03/R-E02: local intents only, no session or transport access.
    pub async fn draft_state(&self) -> Result<String, BridgeError> {
        serde_json::to_string(&self.draft.lock().unwrap().draft).map_err(failure)
    }
    pub async fn action_catalog(&self) -> Result<String, BridgeError> {
        serde_json::to_string(&mi_model::draft::actions()).map_err(failure)
    }
    pub async fn action_library(
        &self,
        query: String,
        category: String,
        favorites_only: bool,
    ) -> Result<String, BridgeError> {
        let store = self.draft.lock().unwrap();
        let (favorites, preferences_error) = match store.favorite_actions() {
            Ok(values) => (values.clone(), None),
            Err(error) => (Default::default(), Some(error)),
        };
        let categories: std::collections::BTreeSet<_> = mi_model::draft::actions()
            .into_iter()
            .map(|a| a.category)
            .collect();
        serde_json::to_string(&serde_json::json!({
            "entries":mi_model::action_library::search(&query, &category, favorites_only, &favorites),
            "categories":categories,"preferences_error":preferences_error
        })).map_err(failure)
    }
    pub async fn set_action_favorite(
        &self,
        action_id: String,
        favorite: bool,
    ) -> Result<(), BridgeError> {
        self.draft
            .lock()
            .unwrap()
            .set_action_favorite(&action_id, favorite)
            .map_err(failure)
    }
    pub async fn open_document(&self, path: String) -> Result<String, BridgeError> {
        let mut store = self.draft.lock().unwrap();
        store.open(path.into()).map_err(failure)?;
        document_response(&store.draft, None)
    }
    pub async fn document_view(&self) -> Result<String, BridgeError> {
        document_response(&self.draft.lock().unwrap().draft, None)
    }
    pub async fn assign_action(
        &self,
        profile_id: String,
        layer_id: String,
        slot: u32,
        action_json: String,
    ) -> Result<String, BridgeError> {
        let profile = serde_json::from_str(&profile_id).map_err(failure)?;
        let layer = serde_json::from_str(&layer_id).map_err(failure)?;
        let action = serde_json::from_str(&action_json).map_err(failure)?;
        let mut store = self.draft.lock().unwrap();
        store
            .update(|d| d.assign(&profile, &layer, slot, action))
            .map_err(failure)?;
        document_response(&store.draft, None)
    }
    pub async fn save_macro_action(
        &self,
        macro_id: Option<String>,
        profile_id: String,
        layer_id: String,
        slot: u32,
        action_json: String,
    ) -> Result<String, BridgeError> {
        let profile = serde_json::from_str(&profile_id).map_err(failure)?;
        let layer = serde_json::from_str(&layer_id).map_err(failure)?;
        let action = serde_json::from_str(&action_json).map_err(failure)?;
        let mut store = self.draft.lock().unwrap();
        store
            .update(|d| {
                d.save_macro_for_binding(macro_id.as_deref(), action, &profile, &layer, slot)
                    .map(|_| ())
            })
            .map_err(failure)?;
        document_response(&store.draft, None)
    }
    pub async fn bind_macro_action(
        &self,
        macro_id: Option<String>,
        profile_id: String,
        layer_id: String,
        slot: u32,
    ) -> Result<String, BridgeError> {
        let profile = serde_json::from_str(&profile_id).map_err(failure)?;
        let layer = serde_json::from_str(&layer_id).map_err(failure)?;
        let mut store = self.draft.lock().unwrap();
        store
            .update(|d| d.bind_macro(&profile, &layer, slot, macro_id.as_deref()))
            .map_err(failure)?;
        document_response(&store.draft, None)
    }
    pub async fn delete_macro_action(&self, macro_id: String) -> Result<String, BridgeError> {
        let mut store = self.draft.lock().unwrap();
        store
            .update(|d| d.delete_macro(&macro_id))
            .map_err(failure)?;
        document_response(&store.draft, None)
    }
    pub async fn undo_draft(&self, redo: bool) -> Result<String, BridgeError> {
        let mut store = self.draft.lock().unwrap();
        store.update(|d| d.undo(redo)).map_err(failure)?;
        document_response(&store.draft, None)
    }
    pub async fn create_profile(&self, name: String) -> Result<String, BridgeError> {
        let mut store = self.draft.lock().unwrap();
        let mut id = String::new();
        store
            .update(|d| {
                id = d.create_profile(&name)?;
                Ok(())
            })
            .map_err(failure)?;
        let lid = store.draft.document.editable.graph.profiles[&id]
            .layer_order
            .first()
            .cloned();
        document_response(&store.draft, Some((id, lid)))
    }
    pub async fn rename_profile(
        &self,
        profile_id: String,
        name: String,
    ) -> Result<String, BridgeError> {
        let mut store = self.draft.lock().unwrap();
        store
            .update(|d| d.rename_profile(&profile_id, &name))
            .map_err(failure)?;
        document_response(&store.draft, None)
    }
    pub async fn duplicate_profile(
        &self,
        profile_id: String,
        name: String,
    ) -> Result<String, BridgeError> {
        let mut store = self.draft.lock().unwrap();
        let mut id = String::new();
        store
            .update(|d| {
                id = d.duplicate_profile(&profile_id, &name)?;
                Ok(())
            })
            .map_err(failure)?;
        let lid = store.draft.document.editable.graph.profiles[&id]
            .layer_order
            .first()
            .cloned();
        document_response(&store.draft, Some((id, lid)))
    }
    pub async fn delete_profile(&self, profile_id: String) -> Result<String, BridgeError> {
        let mut store = self.draft.lock().unwrap();
        store
            .update(|d| d.delete_profile(&profile_id))
            .map_err(failure)?;
        document_response(&store.draft, None)
    }
    pub async fn move_profile(
        &self,
        profile_id: String,
        to_index: u32,
    ) -> Result<String, BridgeError> {
        let mut store = self.draft.lock().unwrap();
        store
            .update(|d| d.move_profile(&profile_id, to_index as usize))
            .map_err(failure)?;
        document_response(&store.draft, None)
    }
    pub async fn create_layer(
        &self,
        profile_id: String,
        name: String,
    ) -> Result<String, BridgeError> {
        let mut store = self.draft.lock().unwrap();
        let mut id = String::new();
        store
            .update(|d| {
                id = d.create_layer(&profile_id, &name)?;
                Ok(())
            })
            .map_err(failure)?;
        document_response(&store.draft, Some((profile_id, Some(id))))
    }
    pub async fn rename_layer(
        &self,
        layer_id: String,
        name: String,
    ) -> Result<String, BridgeError> {
        let mut store = self.draft.lock().unwrap();
        store
            .update(|d| d.rename_layer(&layer_id, &name))
            .map_err(failure)?;
        document_response(&store.draft, None)
    }
    pub async fn duplicate_layer(
        &self,
        profile_id: String,
        layer_id: String,
        name: String,
    ) -> Result<String, BridgeError> {
        let mut store = self.draft.lock().unwrap();
        let mut id = String::new();
        store
            .update(|d| {
                id = d.duplicate_layer(&profile_id, &layer_id, &name)?;
                Ok(())
            })
            .map_err(failure)?;
        document_response(&store.draft, Some((profile_id, Some(id))))
    }
    pub async fn delete_layer(
        &self,
        profile_id: String,
        layer_id: String,
    ) -> Result<String, BridgeError> {
        let mut store = self.draft.lock().unwrap();
        store
            .update(|d| d.delete_layer(&profile_id, &layer_id))
            .map_err(failure)?;
        document_response(&store.draft, None)
    }
    pub async fn move_layer(
        &self,
        profile_id: String,
        layer_id: String,
        to_index: u32,
    ) -> Result<String, BridgeError> {
        let mut store = self.draft.lock().unwrap();
        store
            .update(|d| d.move_layer(&profile_id, &layer_id, to_index as usize))
            .map_err(failure)?;
        document_response(&store.draft, None)
    }
    /// Copy only the current session's completed, checksum-verified snapshot; never caller supplied JSON.
    pub async fn create_draft_from_snapshot(&self) -> Result<String, BridgeError> {
        let state = self.state.lock().unwrap();
        let (id, session) = state
            .session
            .as_ref()
            .ok_or_else(|| failure("设备会话已断开"))?;
        let snapshot = session
            .snapshot()
            .filter(|s| !s.stale)
            .ok_or_else(|| failure("请先读取完整且未过期的设备快照"))?;
        let file = |f: &mi_session::VerifiedFile| serde_json::json!({"state":"present","name":f.meta.file.name(),"size":f.meta.size,"sha1":f.meta.checksum_hex(),"bytes":f.bytes.as_ref()});
        let baseline = serde_json::json!({"device_identity":{"interface":id,"session_id":session.id,"confidence":"observed-interface","backend":session.backend_name()},"firmware":snapshot.config.firmware,"read_at":snapshot.config.read_at,"keymap":file(&snapshot.files.keymap),"smart_actions":snapshot.files.smart_actions.as_ref().map(&file).unwrap_or(serde_json::json!({"state":"missing"}))});
        let mut store = self.draft.lock().unwrap();
        store
            .create_independent(snapshot.config, baseline)
            .map_err(failure)?;
        document_response(&store.draft, None)
    }
    pub async fn list_devices(&self) -> Result<Vec<DeviceInfo>, BridgeError> {
        let t = self.transport.clone();
        tokio::task::spawn_blocking(move || {
            t.enumerate()
                .map(|ds| ds.into_iter().map(Into::into).collect())
        })
        .await
        .map_err(failure)?
        .map_err(failure)
    }
    pub async fn subscribe_device_events(self: Arc<Self>, cb: Box<dyn DeviceEvents>) {
        let mut stopped = self.stopped.subscribe();
        let mut rx = {
            // Serialize watcher creation with shutdown's state invalidation.
            let _state = self.state.lock().unwrap();
            if *stopped.borrow() {
                return;
            }
            self.transport.subscribe()
        };
        let devices = self.list_devices().await;
        if *stopped.borrow() {
            return;
        }
        match devices {
            Ok(devices) => cb.changed(devices),
            Err(e) => cb.failed(e.to_string()),
        }
        let weak = Arc::downgrade(&self);
        tokio::spawn(async move {
            loop {
                let result = tokio::select! {
                    biased;
                    _ = stopped.changed() => break,
                    result = rx.recv() => result,
                };
                let Some(this) = weak.upgrade() else { break };
                if *stopped.borrow() {
                    break;
                }
                match result {
                    Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        let devices = this.list_devices().await;
                        if *stopped.borrow() {
                            break;
                        }
                        match devices {
                            Ok(devices) => {
                                this.devices_changed(&devices);
                                cb.changed(devices);
                            }
                            Err(e) => cb.failed(e.to_string()),
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }
    pub async fn connect(&self, id: String) -> Result<(), BridgeError> {
        if *self.stopped.borrow() {
            return Err(BridgeError::Disconnected);
        }
        // Invalidate before awaiting the gate, so disconnect also cancels queued connects.
        let (generation, old) = {
            let mut state = self.state.lock().unwrap();
            if *self.stopped.borrow() {
                return Err(BridgeError::Disconnected);
            }
            state.generation += 1;
            state.pending = Some(id.clone());
            (state.generation, state.session.take())
        };
        let _attempt = ConnectAttempt {
            state: &self.state,
            generation,
        };
        if let Some((_, s)) = old {
            s.shutdown().await;
        }
        let _gate = self.connect_gate.lock().await;
        if *self.stopped.borrow() || self.state.lock().unwrap().generation != generation {
            return Err(failure("连接已取消"));
        }
        let t = self.transport.clone();
        let path = id.clone();
        let mut opened =
            tokio::task::spawn_blocking(move || t.open(&path).map(|link| OpenedLink(Some(link))))
                .await
                .map_err(failure)?
                .map_err(|e| session_failure(e.into()))?;
        let mut state = self.state.lock().unwrap();
        if *self.stopped.borrow() || state.generation != generation {
            return Err(failure("连接已取消"));
        }
        let link = opened.0.take().expect("native open returned a link");
        // E-075: only the compiled, inspected native adapter can select numbered reports.
        let supported = matches!(
            link.backend(),
            mi_transport::LinkBackend::MacHidapi267Numbered
        );
        #[cfg(feature = "synthetic-backend")]
        let supported =
            supported || matches!(link.backend(), mi_transport::LinkBackend::SyntheticNumbered);
        let framer: Box<dyn mi_proto::Framer> = if supported {
            Box::new(mi_proto::NativeFramer::default())
        } else {
            Box::new(mi_proto::UnimplementedFramer)
        };
        let schema: Arc<dyn mi_session::DeviceSchema> = if supported {
            Arc::new(ReadOnlySchema)
        } else {
            Arc::new(UnverifiedSchema)
        };
        let s = DeviceSession::new(link, framer, schema, SessionConfig::default());
        state.pending = None;
        state.session = Some((id, s));
        Ok(())
    }
    pub async fn shutdown(&self) {
        self.stopped.send_replace(true);
        self.disconnect().await;
        let _connecting = self.connect_gate.lock().await;
        let transport = self.transport.clone();
        // Native discovery joins off the UI executor after callbacks are unscheduled.
        let _ = tokio::task::spawn_blocking(move || transport.stop_discovery()).await;
    }
    pub async fn disconnect(&self) {
        let (_, old) = self.invalidate();
        if let Some((_, s)) = old {
            s.shutdown().await;
        }
    }
    /// Wait on the current generation without opening hardware or sending any request.
    pub async fn wait_for_disconnect(&self) -> String {
        let Ok(session) = self.current() else {
            return "设备已断开".into();
        };
        session.wait_disconnected().await;
        session
            .terminal_failure()
            .map(|e| session_failure(e).to_string())
            .unwrap_or_else(|| "设备已断开".into())
    }
    pub async fn read_identity(&self) -> Result<IdentityInfo, BridgeError> {
        let s = self.current()?;
        let info = s
            .read_identity_info()
            .await
            .map_err(|e| session_failure(s.terminal_failure().unwrap_or(e)))?;
        // A reply verifies this identity query only, not configuration or broad compatibility.
        Ok(IdentityInfo {
            session_id: info.session_id,
            version: info.version,
            profile_index: info.profile_index,
            layer_index: info.layer_index,
            source_method: info.source_method,
            verification: "identity-query-observed".into(),
            backend: s.backend_name().into(),
            raw_json: info.raw.to_string(),
        })
    }
    pub async fn read_config(&self) -> Result<String, BridgeError> {
        let s = self.current()?;
        let c = s.read_config_snapshot().await.map_err(session_failure)?;
        serde_json::to_string(&c).map_err(failure)
    }
    pub async fn load_demo(&self) -> Result<String, BridgeError> {
        let c = self.draft.lock().unwrap().draft.config.clone();
        serde_json::to_string(&c).map_err(failure)
    }
    pub async fn input_monitoring_status(&self) -> bool {
        tokio::task::spawn_blocking(mi_transport::input_monitoring_allowed)
            .await
            .unwrap_or(false)
    }
}
impl MicroInput {
    fn invalidate(&self) -> (u64, Option<(String, Arc<DeviceSession>)>) {
        let mut state = self.state.lock().unwrap();
        state.generation += 1;
        state.pending = None;
        (state.generation, state.session.take())
    }
    fn devices_changed(&self, devices: &[DeviceInfo]) {
        let old = {
            let mut state = self.state.lock().unwrap();
            let id = state
                .pending
                .as_ref()
                .or_else(|| state.session.as_ref().map(|(id, _)| id));
            if id.is_some_and(|id| !devices.iter().any(|d| &d.id == id)) {
                state.generation += 1;
                state.pending = None;
                state.session.take()
            } else {
                None
            }
        };
        if let Some((_, s)) = old {
            s.disconnect();
        }
    }
    fn current(&self) -> Result<Arc<DeviceSession>, BridgeError> {
        self.state
            .lock()
            .unwrap()
            .session
            .as_ref()
            .map(|(_, s)| s.clone())
            .ok_or(BridgeError::Disconnected)
    }
}
impl Drop for MicroInput {
    fn drop(&mut self) {
        if let Some((_, s)) = self.state.lock().unwrap().session.take() {
            s.disconnect();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use mi_transport::{HidLink, MockLink, TransportError};
    struct DelayedTransport {
        entered: Arc<tokio::sync::Notify>,
        release: Arc<(Mutex<bool>, std::sync::Condvar)>,
        link: Arc<MockLink>,
    }
    impl HidTransport for DelayedTransport {
        fn enumerate(&self) -> Result<Vec<HidCandidate>, TransportError> {
            Ok(vec![])
        }
        fn subscribe(&self) -> tokio::sync::broadcast::Receiver<()> {
            let (_, r) = tokio::sync::broadcast::channel(1);
            r
        }
        fn open(&self, _: &str) -> Result<Arc<dyn HidLink>, TransportError> {
            self.entered.notify_one();
            let (lock, cv) = &*self.release;
            let mut released = lock.lock().unwrap();
            while !*released {
                released = cv.wait(released).unwrap();
            }
            Ok(self.link.clone())
        }
    }
    struct SlowDiscoveryTransport {
        entered: Arc<tokio::sync::Notify>,
        release: Arc<(Mutex<bool>, std::sync::Condvar)>,
        events: tokio::sync::broadcast::Sender<()>,
    }
    impl HidTransport for SlowDiscoveryTransport {
        fn enumerate(&self) -> Result<Vec<HidCandidate>, TransportError> {
            self.entered.notify_one();
            let mut released = self.release.0.lock().unwrap();
            while !*released {
                released = self.release.1.wait(released).unwrap();
            }
            Ok(vec![])
        }
        fn subscribe(&self) -> tokio::sync::broadcast::Receiver<()> {
            self.events.subscribe()
        }
        fn open(&self, _: &str) -> Result<Arc<dyn HidLink>, TransportError> {
            Err(TransportError::Unsupported)
        }
    }
    struct CountEvents(Arc<std::sync::atomic::AtomicUsize>);
    impl DeviceEvents for CountEvents {
        fn changed(&self, _: Vec<DeviceInfo>) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        fn failed(&self, _: String) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
    #[tokio::test]
    async fn shutdown_discards_discovery_result_that_was_already_in_flight() {
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let core = Arc::new(MicroInput {
            draft: Mutex::new(mi_model::storage::DocumentStore::default()),
            transport: Arc::new(SlowDiscoveryTransport {
                entered: entered.clone(),
                release: release.clone(),
                events: tokio::sync::broadcast::channel(1).0,
            }),
            state: Mutex::new(ConnectionState::default()),
            connect_gate: tokio::sync::Mutex::new(()),
            stopped: tokio::sync::watch::channel(false).0,
        });
        let callbacks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let c = core.clone();
        let count = callbacks.clone();
        let subscribed = tokio::spawn(async move {
            c.subscribe_device_events(Box::new(CountEvents(count)))
                .await
        });
        entered.notified().await;
        core.shutdown().await;
        *release.0.lock().unwrap() = true;
        release.1.notify_one();
        subscribed.await.unwrap();
        assert_eq!(callbacks.load(std::sync::atomic::Ordering::SeqCst), 0);
    }
    struct LifecycleTransport {
        events: tokio::sync::broadcast::Sender<()>,
        stopped: std::sync::atomic::AtomicBool,
    }
    impl HidTransport for LifecycleTransport {
        fn enumerate(&self) -> Result<Vec<HidCandidate>, TransportError> {
            Ok(vec![])
        }
        fn subscribe(&self) -> tokio::sync::broadcast::Receiver<()> {
            self.events.subscribe()
        }
        fn stop_discovery(&self) {
            self.stopped
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
        fn open(&self, _: &str) -> Result<Arc<dyn HidLink>, TransportError> {
            Ok(MockLink::new().0)
        }
    }
    struct EventsLifetime(Arc<tokio::sync::Notify>);
    impl DeviceEvents for EventsLifetime {
        fn changed(&self, _: Vec<DeviceInfo>) {}
        fn failed(&self, _: String) {}
    }
    impl Drop for EventsLifetime {
        fn drop(&mut self) {
            self.0.notify_one();
        }
    }
    #[tokio::test]
    async fn shutdown_releases_discovery_subscription_and_rejects_new_opens() {
        let transport = Arc::new(LifecycleTransport {
            events: tokio::sync::broadcast::channel(1).0,
            stopped: std::sync::atomic::AtomicBool::new(false),
        });
        let core = Arc::new(MicroInput {
            draft: Mutex::new(mi_model::storage::DocumentStore::default()),
            transport: transport.clone(),
            state: Mutex::new(ConnectionState::default()),
            connect_gate: tokio::sync::Mutex::new(()),
            stopped: tokio::sync::watch::channel(false).0,
        });
        let released = Arc::new(tokio::sync::Notify::new());
        core.clone()
            .subscribe_device_events(Box::new(EventsLifetime(released.clone())))
            .await;
        core.connect("mock".into()).await.unwrap();
        core.shutdown().await;
        tokio::time::timeout(std::time::Duration::from_millis(200), released.notified())
            .await
            .unwrap();
        assert!(transport.stopped.load(std::sync::atomic::Ordering::SeqCst));
        assert!(matches!(
            core.connect("mock".into()).await,
            Err(BridgeError::Disconnected)
        ));
        assert!(matches!(
            core.read_identity().await,
            Err(BridgeError::Disconnected)
        ));
    }
    struct UnavailableTransport;
    impl HidTransport for UnavailableTransport {
        fn enumerate(&self) -> Result<Vec<HidCandidate>, TransportError> {
            Ok(vec![])
        }
        fn subscribe(&self) -> tokio::sync::broadcast::Receiver<()> {
            tokio::sync::broadcast::channel(1).1
        }
        fn open(&self, _: &str) -> Result<Arc<dyn HidLink>, TransportError> {
            Err(TransportError::Unavailable(
                "native detail must stay local".into(),
            ))
        }
    }
    #[tokio::test]
    async fn nonexclusive_open_failure_is_unconfirmed_and_manual_retry_is_available() {
        let core = MicroInput {
            draft: Mutex::new(mi_model::storage::DocumentStore::default()),
            transport: Arc::new(UnavailableTransport),
            state: Mutex::new(ConnectionState::default()),
            connect_gate: tokio::sync::Mutex::new(()),
            stopped: tokio::sync::watch::channel(false).0,
        };
        for _ in 0..2 {
            let error = core.connect("mock".into()).await.unwrap_err();
            assert!(matches!(error, BridgeError::Unavailable));
            assert!(!error.to_string().contains("native detail"));
            assert!(matches!(
                core.read_identity().await,
                Err(BridgeError::Disconnected)
            ));
        }
    }
    #[tokio::test]
    async fn disconnect_cancels_in_flight_open() {
        cancellation_during_open(false).await;
    }
    #[tokio::test]
    async fn removal_cancels_in_flight_open() {
        cancellation_during_open(true).await;
    }
    #[tokio::test]
    async fn dropped_open_future_closes_late_native_handle() {
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let (link, _) = MockLink::new();
        let core = Arc::new(MicroInput {
            draft: Mutex::new(mi_model::storage::DocumentStore::default()),
            transport: Arc::new(DelayedTransport {
                entered: entered.clone(),
                release: release.clone(),
                link: link.clone(),
            }),
            state: Mutex::new(ConnectionState::default()),
            connect_gate: tokio::sync::Mutex::new(()),
            stopped: tokio::sync::watch::channel(false).0,
        });
        let c = core.clone();
        let task = tokio::spawn(async move { c.connect("mock".into()).await });
        entered.notified().await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        *release.0.lock().unwrap() = true;
        release.1.notify_one();
        // The native open is not abortable; its late result must still close.
        let result = tokio::time::timeout(std::time::Duration::from_millis(500), link.read()).await;
        assert!(matches!(result, Ok(Err(TransportError::Disconnected))));
        assert!(core.read_identity().await.is_err());
    }
    async fn cancellation_during_open(removal: bool) {
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let core = Arc::new(MicroInput {
            draft: Mutex::new(mi_model::storage::DocumentStore::default()),
            transport: Arc::new(DelayedTransport {
                entered: entered.clone(),
                release: release.clone(),
                link: MockLink::new().0,
            }),
            state: Mutex::new(ConnectionState::default()),
            connect_gate: tokio::sync::Mutex::new(()),
            stopped: tokio::sync::watch::channel(false).0,
        });
        let c = core.clone();
        let task = tokio::spawn(async move { c.connect("mock".into()).await });
        entered.notified().await;
        if removal {
            core.devices_changed(&[]);
        } else {
            core.disconnect().await;
        }
        *release.0.lock().unwrap() = true;
        release.1.notify_one();
        assert!(task.await.unwrap().is_err());
        assert!(core.current().is_err());
    }
}

#[cfg(feature = "synthetic-backend")]
#[uniffi::export(async_runtime = "tokio")]
impl MicroInput {
    #[uniffi::constructor]
    pub fn synthetic_discovery() -> Arc<Self> {
        Arc::new(Self {
            draft: Mutex::new(mi_model::storage::DocumentStore::default()),
            transport: Arc::new(mi_transport::SyntheticDiscovery::default()),
            state: Mutex::new(ConnectionState::default()),
            connect_gate: tokio::sync::Mutex::new(()),
            stopped: tokio::sync::watch::channel(false).0,
        })
    }
    pub async fn set_synthetic_identity(&self, scenario: u8) -> Result<(), BridgeError> {
        self.transport
            .set_synthetic_identity(scenario)
            .map_err(failure)
    }
    pub async fn set_synthetic_config(&self, scenario: u8) -> Result<(), BridgeError> {
        self.transport
            .set_synthetic_config(scenario)
            .map_err(failure)
    }
    pub async fn synthetic_requests(&self) -> Vec<String> {
        self.transport.synthetic_requests()
    }
    pub async fn set_synthetic_discovery(
        &self,
        state: SyntheticDiscoveryState,
    ) -> Result<(), BridgeError> {
        self.transport
            .set_synthetic_discovery(match state {
                SyntheticDiscoveryState::NoDevices => 0,
                SyntheticDiscoveryState::TwoDevices => 1,
                SyntheticDiscoveryState::Failure => 2,
            })
            .map_err(failure)
    }
}

#[cfg(test)]
mod hardware_tests {
    use super::*;
    #[tokio::test]
    #[ignore = "requires explicitly authorized physical read-only snapshot and isolated local draft"]
    async fn native_snapshot_to_independent_local_draft() {
        struct Isolated(std::path::PathBuf);
        impl Drop for Isolated {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let directory =
            Isolated(std::env::temp_dir().join(format!("mi-t06-{}", mi_model::document::new_id())));
        std::fs::create_dir(&directory.0).unwrap();
        let path = directory.0.join("workbench.json");
        let core = MicroInput::new();
        let outcome = tokio::time::timeout(std::time::Duration::from_secs(90), async {
            let ensure = |ok: bool, message: &str| if ok { Ok(()) } else { Err(failure(message)) };
            core.open_document(path.to_string_lossy().into_owned())
                .await?;
            core.create_profile("Existing local work".into()).await?;
            let old = std::fs::read(&path).map_err(failure)?;
            let devices: Vec<_> = core
                .list_devices()
                .await?
                .into_iter()
                .filter(|d| d.pid == 0x8298 && d.transport == "Usb")
                .collect();
            ensure(
                devices.len() == 1,
                "expected exactly one supported USB candidate",
            )?;
            core.connect(devices[0].id.clone()).await?;
            let identity = core.read_identity().await?;
            ensure(
                identity.version.as_deref() == Some("0.6.3"),
                "unexpected firmware",
            )?;
            let read: serde_json::Value =
                serde_json::from_str(&core.read_config().await?).map_err(failure)?;
            ensure(
                read["source"] == "DeviceSnapshot",
                "not a physical snapshot",
            )?;
            let created: serde_json::Value =
                serde_json::from_str(&core.create_draft_from_snapshot().await?).map_err(failure)?;
            let doc_id = created["draft"]["document_id"]
                .as_str()
                .ok_or_else(|| failure("new document identity missing"))?;
            let pid = created["config"]["profiles"][0]["id"]
                .as_str()
                .ok_or_else(|| failure("source has no profile"))?;
            core.rename_profile(pid.into(), "Acceptance local copy".into())
                .await?;
            ensure(
                std::fs::read(&path).map_err(failure)? == old,
                "previous local file changed",
            )?;
            let selected: serde_json::Value = serde_json::from_slice(
                &std::fs::read(directory.0.join(format!("draft-{doc_id}.json")))
                    .map_err(failure)?,
            )
            .map_err(failure)?;
            ensure(
                selected["baseline"]["keymap"]["state"] == "present",
                "baseline file missing",
            )?;
            let bytes = selected["baseline"]["keymap"]["bytes"]
                .as_array()
                .ok_or_else(|| failure("exact baseline bytes missing"))?;
            ensure(!bytes.is_empty(), "empty baseline bytes")?;
            let re = MicroInput::new();
            let reopened: serde_json::Value = serde_json::from_str(
                &re.open_document(path.to_string_lossy().into_owned())
                    .await?,
            )
            .map_err(failure)?;
            ensure(
                reopened["draft"]["document_id"] == doc_id,
                "selection not restored",
            )?;
            ensure(
                reopened["config"]["source"] == "LocalDraft",
                "new local source not restored",
            )?;
            ensure(
                reopened["config"]["profiles"][0]["name"] == "Acceptance local copy",
                "saved name not restored",
            )?;
            ensure(
                reopened["draft"]["can_undo"] == false,
                "history restored across restart",
            )?;
            re.shutdown().await;
            Ok::<(), BridgeError>(())
        })
        .await;
        core.shutdown().await;
        outcome
            .expect("bounded acceptance timed out")
            .expect("read-only local-draft acceptance failed");
        eprintln!("T06 physical read-only snapshot -> independent local document -> reopen PASS; firmware=0.6.3; no device mutation");
    }
    #[tokio::test]
    #[ignore = "requires an explicitly authorized native read-only lifecycle session"]
    async fn native_identity_shutdown_and_reopen() {
        let mut previous = None;
        for _ in 0..2 {
            let core = MicroInput::new();
            let candidates: Vec<_> = core
                .list_devices()
                .await
                .unwrap()
                .into_iter()
                .filter(|d| d.pid == 0x8298 && d.transport == "Usb")
                .collect();
            assert_eq!(candidates.len(), 1, "explicit selection required");
            // Retain a real descriptor-only event subscription until shutdown.
            struct Events;
            impl DeviceEvents for Events {
                fn changed(&self, _: Vec<DeviceInfo>) {}
                fn failed(&self, _: String) {}
            }
            core.clone().subscribe_device_events(Box::new(Events)).await;
            core.connect(candidates[0].id.clone()).await.unwrap();
            let identity =
                tokio::time::timeout(std::time::Duration::from_secs(25), core.read_identity())
                    .await
                    .expect("bounded query")
                    .expect("identity query");
            assert_eq!(identity.version.as_deref(), Some("0.6.3"));
            assert_ne!(previous, Some(identity.session_id));
            previous = Some(identity.session_id);
            tokio::time::timeout(std::time::Duration::from_secs(3), core.shutdown())
                .await
                .expect("bounded reader and discovery shutdown");
            assert!(matches!(
                core.read_identity().await,
                Err(BridgeError::Disconnected)
            ));
        }
        eprintln!("native read-only lifecycle PASS: two fresh sessions, identity 0.6.3, reader/discovery shutdown, no configuration requests");
    }
    #[tokio::test]
    #[ignore = "requires explicitly authorized physical read-only configuration session"]
    async fn native_configuration_snapshot_read_only() {
        let core = MicroInput::new();
        let devices: Vec<_> = core
            .list_devices()
            .await
            .unwrap()
            .into_iter()
            .filter(|d| d.pid == 0x8298 && d.transport == "Usb")
            .collect();
        assert_eq!(devices.len(), 1, "explicit selection required");
        core.connect(devices[0].id.clone()).await.unwrap();
        let identity = core.read_identity().await.unwrap();
        assert_eq!(identity.version.as_deref(), Some("0.6.3"));
        let response =
            tokio::time::timeout(std::time::Duration::from_secs(65), core.read_config()).await;
        core.disconnect().await;
        let text = response
            .expect("bounded read")
            .expect("verified configuration read");
        let snapshot: serde_json::Value = serde_json::from_str(&text).unwrap();
        let profiles = snapshot["profiles"].as_array().unwrap();
        assert!(!profiles.is_empty());
        assert!(profiles
            .iter()
            .any(|p| p["layers"].as_array().is_some_and(|a| !a.is_empty())));
        assert_eq!(snapshot["source"], "DeviceSnapshot");
        assert_eq!(snapshot["firmware"], "0.6.3");
        let files = snapshot["device_files"].as_array().unwrap();
        assert!(!files.is_empty());
        eprintln!(
            "configuration read PASS: firmware=0.6.3 profiles={} files={} total_bytes={}",
            profiles.len(),
            files.len(),
            files
                .iter()
                .map(|f| f["size"].as_u64().unwrap())
                .sum::<u64>()
        );
    }
    /// Opt-in only. R-D09: same core as UniFFI, exactly one supported USB candidate,
    /// status/version only, bounded lifetime, no file reads or device mutations.
    #[tokio::test]
    #[ignore = "requires an explicitly authorized physical read-only device session"]
    async fn native_identity_query_only() {
        let core = MicroInput::new();
        let devices = core.list_devices().await.unwrap();
        let supported: Vec<_> = devices
            .into_iter()
            .filter(|d| d.pid == 0x8298 && d.transport == "Usb")
            .collect();
        assert_eq!(
            supported.len(),
            1,
            "explicit selection required for zero/multiple candidates"
        );
        core.connect(supported[0].id.clone()).await.unwrap();
        let result =
            tokio::time::timeout(std::time::Duration::from_secs(25), core.read_identity()).await;
        core.disconnect().await;
        let identity = result
            .expect("bounded identity query")
            .expect("native identity response");
        eprintln!(
            "identity version={:?} backend={} source={}",
            identity.version, identity.backend, identity.source_method
        );
        assert!(identity.version.is_some());
    }
}
