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
        Ok((Method::FsList, Value::Null))
    }
    fn files(&self, v: &Value) -> Result<Vec<String>, ProtoError> {
        serde_json::from_value(v["test_files"].clone())
            .map_err(|e| ProtoError::Invalid(e.to_string()))
    }
    fn read_request(&self, n: &str) -> Result<(Method, Value), ProtoError> {
        Ok((Method::FsRead, json!({"test_name":n})))
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
        .send(json!({"id":998}).to_string().into_bytes())
        .unwrap();
    l.inject
        .send(br#"{"m":"kb.sa.exec","params":"never execute"}"#.to_vec())
        .unwrap();
    l.inject
        .send(
            json!({"id":req["id"],"test_ok":true})
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
        .send(json!({"id":req["id"]}).to_string().into_bytes())
        .unwrap();
    assert!(second.await.unwrap().is_ok());
    let w = l.writes.lock().unwrap();
    assert!(w[1].0.duration_since(w[0].0) >= Duration::from_millis(49));
}
#[tokio::test]
async fn malformed_timeout_disconnect_and_new_connection() {
    let (s, l, mut rx) = setup(25);
    let a = s.clone();
    let f = tokio::spawn(async move { a.request(Method::FsList, Value::Null).await });
    rx.recv().await;
    l.inject.send(b"invalid".to_vec()).unwrap();
    assert!(matches!(f.await.unwrap(), Err(SessionError::Protocol(_))));
    assert!(matches!(
        s.request(Method::FsList, Value::Null).await,
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
    let _ = l
        .inject
        .send(json!({"id":req["id"],"old":true}).to_string().into_bytes());
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert!(!f.is_finished());
    new_link
        .inject
        .send(json!({"id":req["id"],"new":true}).to_string().into_bytes())
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
                json!({"id":r["id"],"test_files":files.keys().collect::<Vec<_>>()})
            } else {
                json!({"id":r["id"],"test_content":String::from_utf8_lossy(&files[r["params"]["test_name"].as_str().unwrap()])})
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
                json!({"id":r["id"]}),
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
