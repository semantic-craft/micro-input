use super::*;
use mi_proto::{Line, Report};
use mi_transport::{MockLink, MockTransport};
use serde_json::json;
// TEST ONLY: one raw JSON object per report is a made-up format, NOT a device protocol.
struct TestFramer;
impl Framer for TestFramer {
    fn encode(&self, p: &[u8]) -> Result<Vec<Report>, ProtoError> {
        Ok(vec![p.to_vec()])
    }
    fn feed(&mut self, r: &[u8]) -> Result<Vec<Line>, ProtoError> {
        Ok(vec![r.to_vec()])
    }
    fn clear(&mut self) {}
}
struct TestSchema;
impl DeviceSchema for TestSchema {
    fn identity_request(&self) -> Result<(Method, Value), ProtoError> {
        Ok((Method::SysVersion, Value::Null))
    }
    fn list_request(&self) -> Result<(Method, Value), ProtoError> {
        Ok((Method::FsList, json!({"checksum":true})))
    }
    fn files(&self, v: &Value) -> Result<Vec<String>, ProtoError> {
        serde_json::from_value(v["test_files"].clone())
            .map_err(|e| ProtoError::Invalid(e.to_string()))
    }
    fn read_request(&self, n: &str) -> Result<(Method, Value), ProtoError> {
        Ok((Method::FsReadbin, json!({"file":n,"offset":0,"len":3072})))
    }
    fn bytes(&self, v: &Value) -> Result<Vec<u8>, ProtoError> {
        v["test_content"]
            .as_str()
            .map(|s| s.as_bytes().to_vec())
            .ok_or_else(|| ProtoError::Invalid("synthetic content missing".into()))
    }
    fn parse(&self, k: &[u8], s: Option<&[u8]>) -> Result<DeviceConfig, SessionError> {
        let mut c = mi_model::parse_config(k).map_err(|e| SessionError::Config(e.to_string()))?;
        if let Some(s) = s {
            c.smart_actions =
                serde_json::from_slice(s).map_err(|e| SessionError::Config(e.to_string()))?;
        }
        Ok(c)
    }
}
fn setup(
    timeout: u64,
) -> (
    Arc<DeviceSession>,
    Arc<MockLink>,
    tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
) {
    let (l, r) = MockLink::new();
    (
        DeviceSession::new(
            l.clone(),
            Box::new(TestFramer),
            Arc::new(TestSchema),
            SessionConfig {
                timeout: Duration::from_millis(timeout),
                ..SessionConfig::default()
            },
        ),
        l,
        r,
    )
}
#[tokio::test]
async fn queue_interval_notifications_and_id_matching() {
    let (s, l, mut rx) = setup(500);
    let mut notices = s.notifications();
    let a = s.clone();
    let first = tokio::spawn(async move { a.request(Method::SysVersion, Value::Null).await });
    let req: Value = serde_json::from_slice(&rx.recv().await.unwrap()).unwrap();
    let a = s.clone();
    let second = tokio::spawn(async move { a.request(Method::DeviceStatus, Value::Null).await });
    assert!(tokio::time::timeout(Duration::from_millis(15), rx.recv())
        .await
        .is_err());
    l.inject
        .send(json!({"result":null,"id":998}).to_string().into_bytes())
        .unwrap();
    l.inject
        .send(br#"{"m":"kb.sa.exec","params":"never execute"}"#.to_vec())
        .unwrap();
    l.inject
        .send(
            json!({"result":null,"id":req["id"],"test_ok":true})
                .to_string()
                .into_bytes(),
        )
        .unwrap();
    assert!(first.await.unwrap().is_ok());
    assert!(matches!(
        notices.recv().await.unwrap(),
        HostNotification::Exec(_)
    ));
    let req: Value = serde_json::from_slice(&rx.recv().await.unwrap()).unwrap();
    l.inject
        .send(
            json!({"result":null,"id":req["id"]})
                .to_string()
                .into_bytes(),
        )
        .unwrap();
    assert!(second.await.unwrap().is_ok());
    let w = l.writes.lock().unwrap();
    assert!(w[1].0.duration_since(w[0].0) >= Duration::from_millis(49));
}
#[tokio::test]
async fn malformed_timeout_disconnect_and_new_connection() {
    let (s, l, mut rx) = setup(25);
    let a = s.clone();
    let f = tokio::spawn(async move { a.request(Method::FsList, json!({"checksum":true})).await });
    rx.recv().await;
    l.inject.send(b"invalid".to_vec()).unwrap();
    assert!(matches!(f.await.unwrap(), Err(SessionError::Protocol(_))));
    assert!(matches!(
        s.request(Method::FsList, json!({"checksum":true})).await,
        Err(SessionError::Disconnected)
    ));
    let (s, l, _rx) = setup(25);
    assert!(matches!(
        s.request(Method::FsList, json!({"checksum":true})).await,
        Err(SessionError::Timeout)
    ));
    let a = s.clone();
    let f = tokio::spawn(async move { a.read_config_snapshot().await });
    tokio::time::sleep(Duration::from_millis(5)).await;
    let a = s.clone();
    let queued = tokio::spawn(async move { a.read_identity().await });
    s.disconnect();
    assert!(matches!(f.await.unwrap(), Err(SessionError::Disconnected)));
    assert!(matches!(
        queued.await.unwrap(),
        Err(SessionError::Disconnected)
    ));
    let (new, new_link, mut new_rx) = setup(100);
    assert_ne!(s.id, new.id);
    let a = new.clone();
    let f = tokio::spawn(async move { a.read_identity().await });
    let req: Value = serde_json::from_slice(&new_rx.recv().await.unwrap()).unwrap();
    let _ = l.inject.send(
        json!({"result":null,"id":req["id"],"old":true})
            .to_string()
            .into_bytes(),
    );
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert!(!f.is_finished());
    new_link
        .inject
        .send(
            json!({"result":null,"id":req["id"],"new":true})
                .to_string()
                .into_bytes(),
        )
        .unwrap();
    assert_eq!(f.await.unwrap().unwrap()["new"], true);
}
#[tokio::test]
async fn snapshot_commit_and_stale_preservation() {
    let (s, l, mut rx) = setup(100);
    let mock = Arc::new(MockTransport::new(l.clone()));
    mock.files.lock().unwrap().insert(
        "keymap.json".into(),
        serde_json::to_vec(&mi_model::demo().unwrap().raw).unwrap(),
    );
    mock.files
        .lock()
        .unwrap()
        .insert("smart_actions.json".into(), b"[]".to_vec());
    let m = mock.clone();
    let server = tokio::spawn(async move {
        while let Some(bytes) = rx.recv().await {
            let r: Value = serde_json::from_slice(&bytes).unwrap();
            let files = m.files.lock().unwrap();
            let result = if r["method"] == "fs.list" {
                json!({"result":null,"id":r["id"],"test_files":files.keys().collect::<Vec<_>>()})
            } else {
                json!({"result":null,"id":r["id"],"test_content":String::from_utf8_lossy(&files[r["params"]["file"].as_str().unwrap()])})
            };
            m.link.inject.send(result.to_string().into_bytes()).unwrap();
        }
    });
    assert_eq!(
        s.read_config_snapshot().await.unwrap().source,
        Source::DeviceSnapshot
    );
    mock.files
        .lock()
        .unwrap()
        .insert("smart_actions.json".into(), b"{".to_vec());
    assert!(s.read_config_snapshot().await.is_err());
    assert!(s.snapshot().unwrap().stale);
    assert_eq!(s.snapshot().unwrap().config.profiles.len(), 2);
    server.abort();
}
#[tokio::test]
async fn real_framer_never_writes() {
    let (l, _) = MockLink::new();
    let s = DeviceSession::new(
        l.clone(),
        Box::new(mi_proto::UnimplementedFramer),
        Arc::new(TestSchema),
        SessionConfig::default(),
    );
    assert!(matches!(
        s.read_identity().await,
        Err(SessionError::Protocol(ProtoError::Unverified(_)))
    ));
    assert!(matches!(
        s.read_identity_info().await,
        Err(SessionError::Protocol(ProtoError::Unverified(_)))
    ));
    assert!(l.writes.lock().unwrap().is_empty());
}
#[tokio::test]
async fn notification_after_response_in_same_report_is_not_lost() {
    struct BatchFramer;
    impl Framer for BatchFramer {
        fn encode(&self, p: &[u8]) -> Result<Vec<Report>, ProtoError> {
            Ok(vec![p.to_vec()])
        }
        fn feed(&mut self, r: &[u8]) -> Result<Vec<Line>, ProtoError> {
            Ok(r.split(|b| *b == b'\n').map(Vec::from).collect())
        }
        fn clear(&mut self) {}
    }
    let (l, mut rx) = MockLink::new();
    let s = DeviceSession::new(
        l.clone(),
        Box::new(BatchFramer),
        Arc::new(TestSchema),
        SessionConfig::default(),
    );
    let mut notices = s.notifications();
    let a = s.clone();
    let f = tokio::spawn(async move { a.read_identity().await });
    let r: Value = serde_json::from_slice(&rx.recv().await.unwrap()).unwrap();
    l.inject
        .send(
            format!(
                "{}\n{}",
                json!({"result":null,"id":r["id"]}),
                json!({"method":"kb.radial"})
            )
            .into_bytes(),
        )
        .unwrap();
    assert!(f.await.unwrap().is_ok());
    assert!(matches!(
        notices.try_recv().unwrap(),
        HostNotification::Radial(_)
    ));
}

// R-D13: notifications belong to the session, including between requests.
#[tokio::test]
async fn idle_notifications_are_received_without_a_request() {
    let (s, l, _) = setup(100);
    let mut notices = s.notifications();
    l.inject
        .send(br#"{"method":"kb.radial"}"#.to_vec())
        .unwrap();
    assert!(matches!(
        tokio::time::timeout(Duration::from_millis(100), notices.recv())
            .await
            .unwrap()
            .unwrap(),
        HostNotification::Radial(_)
    ));
    s.shutdown().await;
    assert!(matches!(
        s.request(Method::SysVersion, Value::Null).await,
        Err(SessionError::Disconnected)
    ));
}

#[tokio::test]
async fn production_exit_rejects_forbidden_parameters_without_sending() {
    let (s, l, _rx) = setup(100);
    for (method, params) in [
        (Method::FsRead, json!({"file":"keymap.json"})),
        (
            Method::FsReadbin,
            json!({"file":"unrelated.json","offset":0,"len":10}),
        ),
    ] {
        assert!(s.request(method, params).await.is_err());
    }
    assert!(l.writes.lock().unwrap().is_empty());
    s.shutdown().await;
}

// R-D09/D01: status success is sufficient; absent version triggers explicit fallback.
#[tokio::test]
async fn identity_preserves_unknown_indices_and_falls_back_only_when_needed() {
    let (l, mut requests) = MockLink::new();
    let s = DeviceSession::new(
        l.clone(),
        Box::new(mi_proto::NativeFramer::default()),
        Arc::new(UnverifiedSchema),
        SessionConfig::default(),
    );
    let a = s.clone();
    let read = tokio::spawn(async move { a.read_identity_info().await });
    let report = requests.recv().await.unwrap();
    let r: Value = serde_json::from_slice(&report[3..3 + report[2] as usize]).unwrap();
    assert_eq!(r["method"], "device.status");
    let response =
        json!({"id":r["id"],"result":{"profile_index":-1,"layer_index":1.5,"future":"retained"}});
    for r in mi_proto::NativeFramer::default()
        .encode(response.to_string().as_bytes())
        .unwrap()
    {
        l.inject.send(r).unwrap();
    }
    let report = requests.recv().await.unwrap();
    let r: Value = serde_json::from_slice(&report[3..3 + report[2] as usize]).unwrap();
    assert_eq!(r["method"], "sys.version");
    for r in mi_proto::NativeFramer::default()
        .encode(
            json!({"id":r["id"],"result":{"version":"0.6.3"}})
                .to_string()
                .as_bytes(),
        )
        .unwrap()
    {
        l.inject.send(r).unwrap();
    }
    let identity = read.await.unwrap().unwrap();
    assert_eq!(identity.version.as_deref(), Some("0.6.3"));
    assert_eq!(identity.profile_index, None);
    assert_eq!(identity.layer_index, None);
    assert_eq!(identity.raw["status"]["future"], "retained");
    assert_eq!(identity.source_method, "sys.version");
    assert!(requests.try_recv().is_err());
    s.shutdown().await;
}

#[tokio::test]
async fn exhausted_request_ids_end_the_session_and_require_reconnect() {
    let (link, mut requests) = MockLink::new();
    let s = DeviceSession::new(
        link.clone(),
        Box::new(TestFramer),
        Arc::new(TestSchema),
        SessionConfig {
            timeout: Duration::from_secs(1),
            interval: Duration::ZERO,
        },
    );
    let inject = link.inject.clone();
    let server = tokio::spawn(async move {
        while let Some(r) = requests.recv().await {
            let v: Value = serde_json::from_slice(&r).unwrap();
            inject
                .send(json!({"id":v["id"],"result":null}).to_string().into_bytes())
                .unwrap();
        }
    });
    for _ in 0..999 {
        s.request(Method::SysVersion, Value::Null).await.unwrap();
    }
    let exhausted = s
        .request(Method::SysVersion, Value::Null)
        .await
        .unwrap_err();
    assert!(exhausted.to_string().contains("exhausted"));
    assert!(matches!(
        s.request(Method::SysVersion, Value::Null).await,
        Err(SessionError::Disconnected)
    ));
    s.shutdown().await;
    server.abort();
}

#[tokio::test]
async fn input_monitor_observes_idle_notifications_without_requests_or_payloads() {
    let (s, l, mut requests) = setup(500);
    let mut monitor = s.subscribe_input();
    l.inject
        .send(br#"{"m":"kb.sa.inserttext","params":"private text"}"#.to_vec())
        .unwrap();
    let event = tokio::time::timeout(Duration::from_secs(1), monitor.next())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(event.session_id, s.id);
    assert_eq!(event.sequence, 1);
    assert_eq!(event.kind, crate::InputEventKind::UnknownNotification);
    assert_eq!(event.logical_switch, None);
    assert!(!event.action_eligible);
    assert!(requests.try_recv().is_err());
    s.shutdown().await;
    assert!(monitor.next().await.is_none());
}

// Test-only logical decoding; this is not a Creator Micro wire schema.
struct SyntheticInputDecoder;
impl crate::InputDecoder for SyntheticInputDecoder {
    fn decode(&self, _: &HostNotification) -> Option<(crate::InputEventKind, String)> {
        Some((crate::InputEventKind::Pressed, "synthetic-switch-a".into()))
    }
}
#[tokio::test]
async fn input_gap_clears_pressed_state_and_requires_new_subscription() {
    let (s, l, mut requests) = setup(1000);
    let mut monitor = s.subscribe_input_with_decoder(Box::new(SyntheticInputDecoder));
    l.inject
        .send(br#"{"m":"test.synthetic"}"#.to_vec())
        .unwrap();
    assert_eq!(
        monitor.next().await.unwrap().kind,
        crate::InputEventKind::Pressed
    );
    assert_eq!(monitor.pressed_switches(), ["synthetic-switch-a"]);
    for _ in 0..300 {
        l.inject
            .send(br#"{"m":"test.synthetic"}"#.to_vec())
            .unwrap();
    }
    // A response behind the flood is a public barrier that the single reader consumed it.
    let session = s.clone();
    let barrier =
        tokio::spawn(async move { session.request(Method::SysVersion, Value::Null).await });
    let request: Value = serde_json::from_slice(&requests.recv().await.unwrap()).unwrap();
    l.inject
        .send(
            json!({"id":request["id"],"result":{}})
                .to_string()
                .into_bytes(),
        )
        .unwrap();
    barrier.await.unwrap().unwrap();
    let gap = monitor.next().await.unwrap();
    assert_eq!(gap.kind, crate::InputEventKind::InputStreamGap);
    assert_eq!(gap.lost_count, 44);
    assert!(!gap.action_eligible);
    assert!(monitor.pressed_switches().is_empty());
    assert!(monitor.stopped());
    assert!(monitor.next().await.is_none());
    let mut rebuilt = s.subscribe_input();
    assert!(rebuilt.pressed_switches().is_empty());
    l.inject
        .send(br#"{"m":"unknown.current.firmware"}"#.to_vec())
        .unwrap();
    assert_eq!(
        rebuilt.next().await.unwrap().kind,
        crate::InputEventKind::UnknownNotification
    );
    s.shutdown().await;
}

#[tokio::test]
async fn disconnected_monitor_drops_buffered_old_session_events() {
    let (s, l, _) = setup(500);
    let mut monitor = s.subscribe_input();
    l.inject
        .send(br#"{"m":"test.synthetic"}"#.to_vec())
        .unwrap();
    s.shutdown().await;
    assert!(monitor.next().await.is_none());
    let (new_session, link, _) = setup(500);
    let mut fresh = new_session.subscribe_input();
    link.inject
        .send(br#"{"m":"test.synthetic"}"#.to_vec())
        .unwrap();
    assert_eq!(fresh.next().await.unwrap().session_id, new_session.id);
    assert_ne!(s.id, new_session.id);
    new_session.shutdown().await;
}
