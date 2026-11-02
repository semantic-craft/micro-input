//! R-A02/R-A05: only high-level asynchronous operations cross the in-process UniFFI bridge.
mod input_monitor;
pub use input_monitor::*;
use mi_session::{DeviceSession, ReadOnlySchema, SessionConfig, UnverifiedSchema};
use mi_transport::{HidCandidate, HidTransport, MacTransport};
use std::sync::{Arc, Mutex};
uniffi::setup_scaffolding!();
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum BridgeError {
    #[error("{message}")]
    Failed { message: String },
}
fn failure(e: impl std::fmt::Display) -> BridgeError {
    BridgeError::Failed {
        message: e.to_string(),
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
#[derive(uniffi::Object)]
pub struct MicroInput {
    draft: Mutex<mi_model::storage::DocumentStore>,
    transport: Arc<dyn HidTransport>,
    state: Mutex<ConnectionState>,
    connect_gate: tokio::sync::Mutex<()>,
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
        })
    }
    // R-P03/R-E02: local intents only, no session or transport access.
    pub async fn draft_state(&self) -> Result<String, BridgeError> {
        serde_json::to_string(&self.draft.lock().unwrap().draft).map_err(failure)
    }
    pub async fn action_catalog(&self) -> Result<String, BridgeError> {
        serde_json::to_string(&mi_model::draft::actions()).map_err(failure)
    }
    pub async fn open_document(&self, path: String) -> Result<String, BridgeError> {
        self.draft
            .lock()
            .unwrap()
            .open(path.into())
            .map_err(failure)?;
        self.draft_state().await
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
        self.draft
            .lock()
            .unwrap()
            .update(|d| d.assign(&profile, &layer, slot, action))
            .map_err(failure)?;
        self.draft_state().await
    }
    pub async fn undo_draft(&self, redo: bool) -> Result<String, BridgeError> {
        self.draft
            .lock()
            .unwrap()
            .update(|d| d.undo(redo))
            .map_err(failure)?;
        self.draft_state().await
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
        let mut rx = self.transport.subscribe();
        match self.list_devices().await {
            Ok(devices) => cb.changed(devices),
            Err(e) => cb.failed(e.to_string()),
        }
        let weak = Arc::downgrade(&self);
        tokio::spawn(async move {
            loop {
                let result =
                    tokio::time::timeout(std::time::Duration::from_millis(250), rx.recv()).await;
                let Some(this) = weak.upgrade() else { break };
                match result {
                    Ok(Ok(())) | Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {
                        match this.list_devices().await {
                            Ok(devices) => {
                                this.devices_changed(&devices);
                                cb.changed(devices);
                            }
                            Err(e) => cb.failed(e.to_string()),
                        }
                    }
                    Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => break,
                    Err(_) => {}
                }
            }
        });
    }
    pub async fn connect(&self, id: String) -> Result<(), BridgeError> {
        // Invalidate before awaiting the gate, so disconnect also cancels queued connects.
        let (generation, old) = {
            let mut state = self.state.lock().unwrap();
            state.generation += 1;
            state.pending = Some(id.clone());
            (state.generation, state.session.take())
        };
        if let Some((_, s)) = old {
            s.shutdown().await;
        }
        let _gate = self.connect_gate.lock().await;
        if self.state.lock().unwrap().generation != generation {
            return Err(failure("连接已取消"));
        }
        let t = self.transport.clone();
        let path = id.clone();
        let link = tokio::task::spawn_blocking(move || t.open(&path))
            .await
            .map_err(failure)?
            .map_err(failure)?;
        let mut state = self.state.lock().unwrap();
        if state.generation != generation {
            link.close();
            return Err(failure("连接已取消"));
        }
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
    pub async fn disconnect(&self) {
        let (_, old) = self.invalidate();
        if let Some((_, s)) = old {
            s.shutdown().await;
        }
    }
    pub async fn read_identity(&self) -> Result<IdentityInfo, BridgeError> {
        let s = self.current()?;
        let info = s
            .read_identity_info()
            .await
            .map_err(|e| failure(s.terminal_failure().unwrap_or(e)))?;
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
        let c = s.read_config_snapshot().await.map_err(failure)?;
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
            .ok_or_else(|| failure("未连接设备"))
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
            let (l, _) = MockLink::new();
            Ok(l)
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
    async fn cancellation_during_open(removal: bool) {
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let core = Arc::new(MicroInput {
            draft: Mutex::new(mi_model::storage::DocumentStore::default()),
            transport: Arc::new(DelayedTransport {
                entered: entered.clone(),
                release: release.clone(),
            }),
            state: Mutex::new(ConnectionState::default()),
            connect_gate: tokio::sync::Mutex::new(()),
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
