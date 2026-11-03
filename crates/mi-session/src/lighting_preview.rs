//! Simulator-only lifecycle, not the vendor wire protocol. Targeted set/read semantics
//! exist only in SyntheticFiles; native lights.preview and its recovery remain unverified.
use super::*;
use mi_model::lighting::{Edit, Lighting, Zone};
use serde_json::json;
use tokio::sync::watch;
pub struct Preview {
    desired: Mutex<Option<Lighting>>,
    commands: watch::Sender<Option<Lighting>>,
    state: watch::Receiver<Value>,
}
async fn call(
    s: &DeviceSession,
    q: &mut Queue,
    method: &'static str,
    params: Value,
) -> Result<Value, SessionError> {
    let bytes =
        serde_json::to_vec(&json!({"id":q.next_id,"method":method,"params":params})).unwrap();
    let reply = s.exchange_encoded(q, method, bytes).await?;
    reply
        .get("result")
        .cloned()
        .filter(|v| v.get("target").is_some() && v.get("lights").is_some())
        .ok_or_else(|| SessionError::Config("synthetic preview rejected".into()))
}
impl Preview {
    pub async fn begin(session: Arc<DeviceSession>) -> Result<Arc<Self>, String> {
        Self::begin_bounded(session, Duration::from_secs(30)).await
    }
    async fn begin_bounded(
        session: Arc<DeviceSession>,
        lifetime: Duration,
    ) -> Result<Arc<Self>, String> {
        if session.backend_name() != "synthetic/numbered-0x06" {
            return Err("native preview remains disabled".into());
        }
        let (ready_tx, ready_rx) = oneshot::channel();
        let (commands, mut rx) = watch::channel(None::<Lighting>);
        let (state_tx, state) =
            watch::channel(json!({"phase":"Starting","physical_visual":"NOT_RUN"}));
        tokio::spawn(async move {
            // Owning the queue for this bounded preview reserves IDs against every read/write.
            let mut q = session.queue.lock().await;
            if q.next_id > 995 {
                let _ = ready_tx.send(Err("request budget insufficient".to_string()));
                return;
            }
            let original = match call(&session, &mut q, "synthetic.lights.read", Value::Null).await
            {
                Ok(v) => v,
                Err(e) => {
                    let _ = ready_tx.send(Err(e.to_string()));
                    return;
                }
            };
            if original["target"] != 0 {
                let _ = ready_tx.send(Err("active source not verified".into()));
                return;
            }
            let lights: Lighting = match serde_json::from_value(original["lights"].clone()) {
                Ok(v) => v,
                Err(e) => {
                    let _ = ready_tx.send(Err(e.to_string()));
                    return;
                }
            };
            state_tx.send_replace(json!({"phase":"Active","physical_visual":"NOT_RUN"}));
            if ready_tx.send(Ok(lights)).is_err() {
                return;
            }
            let deadline = tokio::time::Instant::now() + lifetime;
            let mut failed = false;
            loop {
                tokio::select! {biased; _=session.changed.notified()=>{failed=true;break;}, _=tokio::time::sleep_until(deadline)=>break, changed=rx.changed()=>{if changed.is_err(){break;}} }
                // Latest-only coalescing; never cancel an in-flight transport exchange.
                tokio::time::sleep(Duration::from_millis(80)).await;
                let desired = rx.borrow_and_update().clone();
                let Some(desired) = desired else { break };
                if q.next_id >= 997 {
                    break;
                } // two IDs reserved for restoration and readback
                let mut target = original.clone();
                target["lights"]["backlight"] = desired.backlight;
                target["lights"]["underglow"] = desired.underglow;
                match call(&session, &mut q, "synthetic.lights.set", target).await {
                    Ok(v) if v["target"] == original["target"] => {}
                    _ => {
                        failed = true;
                        break;
                    }
                }
            }
            // Target comparison and update are atomic in the simulator. A layer switch
            // rejects restoration instead of sending an old layer's values to a new target.
            if !failed {
                failed = call(&session, &mut q, "synthetic.lights.set", original.clone())
                    .await
                    .is_err();
                if !failed {
                    failed = call(&session, &mut q, "synthetic.lights.read", Value::Null)
                        .await
                        .ok()
                        != Some(original);
                }
            }
            if failed {
                session.disconnect();
            }
            state_tx.send_replace(json!({"phase":if failed {"Unknown"}else{"Restored"},"runtime_readback":if failed {"NOT_VERIFIED"}else{"SYNTHETIC_PASS"},"physical_visual":"NOT_RUN"}));
        });
        let original = ready_rx.await.map_err(|e| e.to_string())??;
        Ok(Arc::new(Self {
            desired: Mutex::new(Some(original)),
            commands,
            state,
        }))
    }
    pub fn lighting(&self) -> Option<Lighting> {
        self.desired.lock().unwrap().clone()
    }
    pub fn status(&self) -> Value {
        self.state.borrow().clone()
    }
    pub fn update(&self, zone: Zone, edit: Edit) -> Result<(), String> {
        if self.status()["phase"] != "Active" {
            return Err("preview has ended".into());
        }
        let mut guard = self.desired.lock().unwrap();
        let mut desired = guard.clone().ok_or("preview is ending")?;
        desired.edit(zone, edit)?;
        *guard = Some(desired.clone());
        self.commands.send_replace(Some(desired));
        Ok(())
    }
    pub async fn end(&self) -> Value {
        *self.desired.lock().unwrap() = None;
        self.commands.send_replace(None);
        let mut state = self.state.clone();
        loop {
            let value = state.borrow_and_update().clone();
            if value["phase"] != "Active" && value["phase"] != "Starting" {
                return value;
            }
            if state.changed().await.is_err() {
                return json!({"phase":"Unknown","physical_visual":"NOT_RUN"});
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mi_transport::{HidTransport, SyntheticDiscovery, SyntheticFiles};
    const RAW: &[u8] =
        include_bytes!("../../mi-model/fixtures/device/synthetic-keymap-lighting.json");
    async fn setup() -> (Arc<SyntheticDiscovery>, Arc<DeviceSession>) {
        let transport = Arc::new(SyntheticDiscovery::default());
        transport.set_synthetic_discovery(1).unwrap();
        transport.set_files(SyntheticFiles::new(RAW.to_vec(), None));
        let session = DeviceSession::new(
            transport.open("synthetic-interface-0").unwrap(),
            Box::<mi_proto::NativeFramer>::default(),
            Arc::new(ReadOnlySchema),
            SessionConfig {
                timeout: Duration::from_millis(50),
                interval: Duration::ZERO,
            },
        );
        (transport, session)
    }
    async fn color(device: &SyntheticDiscovery, expected: u32) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while device.files().unwrap().preview_lights["backlight"]["color"] != expected {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn latest_only_and_restoration_preserve_files_and_unknown_runtime_fields() {
        let (device, session) = setup().await;
        let preview = Preview::begin(session.clone()).await.unwrap();
        let original = device.files().unwrap().preview_lights;
        for n in 1..=100 {
            preview.update(Zone::Backlight, Edit::Color(n)).unwrap();
        }
        color(&device, 100).await;
        assert_eq!(
            device
                .synthetic_requests()
                .iter()
                .filter(|m| *m == "synthetic.lights.set")
                .count(),
            1
        );
        assert_eq!(device.files().unwrap().keymap, RAW);
        assert_eq!(
            device.files().unwrap().preview_lights["unknown_region"],
            original["unknown_region"]
        );
        assert_eq!(preview.end().await["phase"], "Restored");
        assert_eq!(device.files().unwrap().preview_lights, original);
        assert_eq!(device.files().unwrap().keymap, RAW);
        assert!(preview.update(Zone::Backlight, Edit::Color(5)).is_err());
        assert_eq!(preview.end().await["phase"], "Restored");
        session.shutdown().await;
    }
    #[tokio::test]
    async fn changed_target_is_not_given_old_restoration_values() {
        let (device, session) = setup().await;
        let preview = Preview::begin(session.clone()).await.unwrap();
        preview.update(Zone::Backlight, Edit::Color(55)).unwrap();
        color(&device, 55).await;
        let mut files = device.files().unwrap();
        files.preview_target = 1;
        files.preview_lights = json!({"different":"layer"});
        device.set_files(files);
        assert_eq!(preview.end().await["phase"], "Unknown");
        assert_eq!(
            device.files().unwrap().preview_lights,
            json!({"different":"layer"})
        );
        session.shutdown().await;
    }
    #[tokio::test]
    async fn response_failure_never_claims_restoration_and_keeps_files_untouched() {
        let (device, session) = setup().await;
        let preview = Preview::begin(session.clone()).await.unwrap();
        preview.update(Zone::Backlight, Edit::Color(42)).unwrap();
        color(&device, 42).await;
        let mut files = device.files().unwrap();
        files.fail_preview = true;
        device.set_files(files);
        assert_eq!(preview.end().await["phase"], "Unknown");
        assert_eq!(device.files().unwrap().keymap, RAW);
        assert_eq!(
            device.files().unwrap().preview_lights["backlight"]["color"],
            42
        );
        session.shutdown().await;
    }
    #[tokio::test]
    async fn reserved_ids_survive_competing_read_and_exhaustion_ends_preview() {
        let (device, session) = setup().await;
        session.queue.lock().await.next_id = 995;
        let preview = Preview::begin(session.clone()).await.unwrap();
        let other = session.clone();
        let read =
            tokio::spawn(async move { other.request(Method::DeviceStatus, Value::Null).await });
        preview.update(Zone::Backlight, Edit::Color(21)).unwrap();
        color(&device, 21).await;
        assert!(!read.is_finished());
        preview.update(Zone::Backlight, Edit::Color(22)).unwrap();
        let result = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if preview.status()["phase"] == "Restored" {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await;
        assert!(result.is_ok());
        assert!(read.await.unwrap().is_err());
        assert_eq!(
            device.files().unwrap().preview_lights["backlight"]["color"],
            1193046
        );
        session.shutdown().await;
    }
    #[tokio::test]
    async fn insufficient_budget_and_unverified_target_send_no_mutations() {
        let (device, session) = setup().await;
        session.queue.lock().await.next_id = 996;
        assert!(Preview::begin(session.clone()).await.is_err());
        assert!(device.synthetic_requests().is_empty());
        session.queue.lock().await.next_id = 1;
        let mut files = device.files().unwrap();
        files.preview_target = 7;
        device.set_files(files);
        assert!(Preview::begin(session.clone()).await.is_err());
        assert!(!device
            .synthetic_requests()
            .iter()
            .any(|m| m == "synthetic.lights.set"));
        session.shutdown().await;
    }
    #[tokio::test]
    async fn bounded_lifetime_restores_and_disconnect_reports_unknown() {
        let (device, session) = setup().await;
        let preview = Preview::begin_bounded(session.clone(), Duration::from_millis(120))
            .await
            .unwrap();
        preview.update(Zone::Backlight, Edit::Color(123)).unwrap();
        color(&device, 123).await;
        tokio::time::timeout(Duration::from_secs(2), async {
            while preview.status()["phase"] == "Active" {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(preview.status()["phase"], "Restored");
        let preview = Preview::begin(session.clone()).await.unwrap();
        preview.update(Zone::Backlight, Edit::Color(124)).unwrap();
        color(&device, 124).await;
        session.disconnect();
        assert_eq!(preview.end().await["phase"], "Unknown");
        assert_eq!(device.files().unwrap().keymap, RAW);
        session.shutdown().await;
    }
    #[tokio::test]
    async fn dropped_owner_restores_without_a_second_reader() {
        let (device, session) = setup().await;
        let preview = Preview::begin(session.clone()).await.unwrap();
        preview.update(Zone::Backlight, Edit::Color(64)).unwrap();
        color(&device, 64).await;
        drop(preview);
        color(&device, 1193046).await;
        assert!(session
            .request(Method::DeviceStatus, Value::Null)
            .await
            .is_ok());
        session.shutdown().await;
    }
}
