//! Explicit device preview. Native stop does not restore runtime state.
use super::*;
use mi_model::lighting::{Edit, Lighting, Zone};
use serde_json::json;
use tokio::sync::watch;
pub struct Preview {
    desired: Mutex<Option<Lighting>>,
    commands: watch::Sender<Option<Lighting>>,
    state: watch::Receiver<Value>,
}
#[cfg(feature = "synthetic-backend")]
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
    #[cfg(feature = "synthetic-backend")]
    pub async fn begin(session: Arc<DeviceSession>) -> Result<Arc<Self>, String> {
        Self::begin_bounded(session, Duration::from_secs(30)).await
    }
    #[cfg(feature = "synthetic-backend")]
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
            let mut last = original.clone();
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
                let mut request = target.clone();
                request["expected"] = last["lights"].clone();
                match call(&session, &mut q, "synthetic.lights.set", request).await {
                    Ok(v) if v == target => last = v,
                    _ => {
                        failed = true;
                        break;
                    }
                }
            }
            // Atomically compare both target and last verified runtime state. External
            // changes must survive both subsequent updates and restoration.
            if !failed {
                let mut restore = original.clone();
                restore["expected"] = last["lights"].clone();
                failed = call(&session, &mut q, "synthetic.lights.set", restore)
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
    pub async fn begin_native(
        session: Arc<DeviceSession>,
        lights: Lighting,
    ) -> Result<Arc<Self>, String> {
        if session.backend_name() != "hidapi-2.6.7/macOS/numbered-0x06" {
            return Err("unsupported preview connection".into());
        }
        let info = session
            .read_identity_info()
            .await
            .map_err(|e| e.to_string())?;
        if !info
            .version
            .as_deref()
            .is_some_and(mi_model::is_supported_native_firmware)
        {
            return Err("unsupported preview firmware".into());
        }
        Self::begin_native_task(session, lights).await
    }
    async fn begin_native_task(
        session: Arc<DeviceSession>,
        lights: Lighting,
    ) -> Result<Arc<Self>, String> {
        Self::native_params(&lights)?;
        let (commands, mut rx) = watch::channel(Some(lights.clone()));
        let (state_tx, state) =
            watch::channel(json!({"phase":"Starting","physical_visual":"NOT_RUN"}));
        let (ready_tx, ready_rx) = oneshot::channel();
        tokio::spawn(async move {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
            let mut ready = Some(ready_tx);
            let mut failed = false;
            'preview: loop {
                let mut q = loop {
                    tokio::select! { biased;
                        _ = session.wait_disconnected() => { failed = true; break 'preview; }
                        _ = tokio::time::sleep_until(deadline) => break 'preview,
                        _ = async { if let Some(tx) = ready.as_mut() { tx.closed().await; } else { std::future::pending::<()>().await; } } => break 'preview,
                        changed = rx.changed() => {
                            if changed.is_err() || rx.borrow().is_none() { break 'preview; }
                        }
                        q = session.queue.lock() => break q,
                    }
                };
                if rx.has_changed().is_err()
                    || ready.as_ref().is_some_and(|tx| tx.is_closed())
                    || tokio::time::Instant::now() >= deadline
                {
                    break;
                }
                let desired = rx.borrow_and_update().clone();
                let Some(desired) = desired else { break };
                let result = match Self::native_params(&desired) {
                    Ok(params) => session.exchange_lighting(&mut q, &params).await,
                    Err(error) => Err(SessionError::Config(error)),
                };
                drop(q);
                if let Err(error) = result {
                    if let Some(tx) = ready.take() {
                        let _ = tx.send(Err(error.to_string()));
                    }
                    failed = true;
                    break;
                }
                state_tx.send_replace(json!({"phase":"Active","physical_visual":"NOT_RUN","runtime_readback":"NOT_AVAILABLE"}));
                if let Some(tx) = ready.take() {
                    let _ = tx.send(Ok(()));
                }
                tokio::select! { biased;
                    _ = session.wait_disconnected() => { failed = true; break; }
                    _ = tokio::time::sleep_until(deadline) => break,
                    change = rx.changed() => if change.is_err() { break; }
                }
                // Coalesce pending edits without cancelling an exchange already in flight.
                tokio::select! { biased;
                    _ = session.wait_disconnected() => { failed = true; break; }
                    _ = tokio::time::sleep_until(deadline) => break,
                    _ = tokio::time::sleep(Duration::from_millis(80)) => {}
                }
            }
            if failed {
                session.disconnect();
            }
            state_tx.send_replace(json!({"phase":if failed {"Unknown"} else {"Stopped"},"runtime_readback":"NOT_AVAILABLE","physical_visual":"NOT_RUN"}));
        });
        ready_rx.await.map_err(|e| e.to_string())??;
        Ok(Arc::new(Self {
            desired: Mutex::new(Some(lights)),
            commands,
            state,
        }))
    }
    fn native_params(
        lights: &Lighting,
    ) -> Result<mi_proto::lighting_candidate::LightingPreviewCandidate, String> {
        let params = serde_json::from_value(serde_json::to_value(lights).unwrap())
            .map_err(|e| e.to_string())?;
        mi_proto::lighting_candidate::encode_lighting_preview_candidate(&params, 0)
            .map_err(|e| e.to_string())?;
        Ok(params)
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

#[cfg(all(test, feature = "synthetic-backend"))]
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
    async fn same_target_external_change_is_never_overwritten() {
        for update_again in [false, true] {
            let (device, session) = setup().await;
            let preview = Preview::begin(session.clone()).await.unwrap();
            preview.update(Zone::Backlight, Edit::Color(55)).unwrap();
            color(&device, 55).await;
            let mut files = device.files().unwrap();
            files.preview_lights["backlight"]["color"] = json!(99);
            let external = files.preview_lights.clone();
            device.set_files(files);
            if update_again {
                preview.update(Zone::Backlight, Edit::Color(66)).unwrap();
                tokio::time::timeout(Duration::from_secs(2), async {
                    while preview.status()["phase"] == "Active" {
                        tokio::time::sleep(Duration::from_millis(2)).await;
                    }
                })
                .await
                .unwrap();
            }
            assert_eq!(preview.end().await["phase"], "Unknown");
            assert_eq!(device.files().unwrap().preview_lights, external);
            assert_eq!(device.files().unwrap().keymap, RAW);
            session.shutdown().await;
        }
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

#[cfg(test)]
mod native_tests {
    use super::*;
    use mi_transport::{HidLink, LinkBackend, MockLink, TransportError};
    struct NativeLink(Arc<MockLink>);
    #[async_trait::async_trait]
    impl HidLink for NativeLink {
        fn backend(&self) -> LinkBackend {
            LinkBackend::MacHidapi267Numbered
        }
        async fn write(&self, report: &[u8]) -> Result<(), TransportError> {
            self.0.write(report).await
        }
        async fn read(&self) -> Result<Vec<u8>, TransportError> {
            self.0.read().await
        }
        fn close(&self) {
            self.0.close();
        }
    }
    async fn setup() -> (Arc<DeviceSession>, Arc<Mutex<Vec<Value>>>, Arc<AtomicBool>) {
        let (link, mut outgoing) = MockLink::new();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let fail = Arc::new(AtomicBool::new(false));
        let fault = fail.clone();
        let incoming = link.clone();
        tokio::spawn(async move {
            let mut framer = mi_proto::NativeFramer::default();
            while let Some(report) = outgoing.recv().await {
                for line in framer.feed(&report).unwrap() {
                    let request: Value = serde_json::from_slice(&line).unwrap();
                    recorded.lock().unwrap().push(request.clone());
                    let reply = if request["method"] == "sys.version"
                        || request["method"] == "device.status"
                    {
                        json!({"id":request["id"],"result":{"version":"0.6.3"}})
                    } else if fault.load(Ordering::SeqCst) {
                        json!({"id":request["id"],"error":{"code":400,"message":"fixture rejected"}})
                    } else {
                        json!({"id":request["id"],"result":null})
                    };
                    for report in framer.encode(reply.to_string().as_bytes()).unwrap() {
                        let _ = incoming.inject.send(report);
                    }
                }
            }
        });
        let session = DeviceSession::new(
            Arc::new(NativeLink(link)),
            Box::<mi_proto::NativeFramer>::default(),
            Arc::new(ReadOnlySchema),
            SessionConfig {
                timeout: Duration::from_millis(200),
                interval: Duration::ZERO,
            },
        );
        (session, requests, fail)
    }
    async fn wait_for(requests: &Mutex<Vec<Value>>, count: usize) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while requests.lock().unwrap().len() < count {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn native_preview_uses_selected_values_and_stop_never_restores_or_writes_files() {
        let (session, requests, _) = setup().await;
        let mut lights = Lighting::creator_micro_2_defaults();
        lights.backlight["color"] = json!(42);
        let preview = Preview::begin_native(session.clone(), lights)
            .await
            .unwrap();
        assert_eq!(
            requests.lock().unwrap()[1]["params"]["backlight"]["color"],
            42
        );
        for color in 1..=100 {
            preview.update(Zone::Backlight, Edit::Color(color)).unwrap();
        }
        wait_for(&requests, 3).await;
        assert_eq!(
            requests.lock().unwrap()[2]["params"]["backlight"]["color"],
            100
        );
        assert_eq!(preview.end().await["phase"], "Stopped");
        assert_eq!(requests.lock().unwrap().len(), 3);
        assert!(requests
            .lock()
            .unwrap()
            .iter()
            .all(|r| r["method"] == "sys.version"
                || r["method"] == "device.status"
                || r["method"] == "lights.preview"));
        session.shutdown().await;
    }
    #[tokio::test]
    async fn stopping_while_queue_busy_discards_pending_update() {
        let (session, requests, _) = setup().await;
        let preview = Preview::begin_native(session.clone(), Lighting::creator_micro_2_defaults())
            .await
            .unwrap();
        let queue = session.queue.lock().await;
        preview.update(Zone::Backlight, Edit::Color(42)).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), preview.end())
                .await
                .unwrap()["phase"],
            "Stopped"
        );
        drop(queue);
        assert_eq!(requests.lock().unwrap().len(), 2);
        session.shutdown().await;
    }
    #[tokio::test]
    async fn cancelling_begin_while_queued_never_sends_an_unowned_preview() {
        let (session, requests, _) = setup().await;
        let queue = session.queue.lock().await;
        let other = session.clone();
        let begin = tokio::spawn(async move {
            Preview::begin_native_task(other, Lighting::creator_micro_2_defaults()).await
        });
        tokio::task::yield_now().await;
        begin.abort();
        let _ = begin.await;
        drop(queue);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(requests.lock().unwrap().is_empty());
        session.shutdown().await;
    }
    #[tokio::test]
    async fn native_failure_stops_without_retry_and_drop_does_not_restore() {
        let (session, requests, fail) = setup().await;
        let preview = Preview::begin_native(session.clone(), Lighting::creator_micro_2_defaults())
            .await
            .unwrap();
        fail.store(true, Ordering::SeqCst);
        preview.update(Zone::Backlight, Edit::Color(42)).unwrap();
        wait_for(&requests, 3).await;
        assert_eq!(preview.end().await["phase"], "Unknown");
        assert_eq!(requests.lock().unwrap().len(), 3);
        session.shutdown().await;
        let (session, requests, _) = setup().await;
        let preview = Preview::begin_native(session.clone(), Lighting::creator_micro_2_defaults())
            .await
            .unwrap();
        drop(preview);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(requests.lock().unwrap().len(), 2);
        session.shutdown().await;
    }
}
