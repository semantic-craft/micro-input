use super::*;
use mi_proto::{Line, Report};
use mi_transport::MockLink;
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
            Arc::new(ReadOnlySchema),
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
    let f = tokio::spawn(async move { a.request(Method::FsList, json!({"checksum":true})).await });
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
async fn real_framer_never_writes() {
    let (l, _) = MockLink::new();
    let s = DeviceSession::new(
        l.clone(),
        Box::new(mi_proto::UnimplementedFramer),
        Arc::new(ReadOnlySchema),
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
        Arc::new(ReadOnlySchema),
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
        Arc::new(ReadOnlySchema),
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
async fn bounded_device_files_are_verified_before_snapshot_publication() {
    let (link, mut requests) = MockLink::new();
    let s = DeviceSession::new(
        link.clone(),
        Box::new(TestFramer),
        Arc::new(ReadOnlySchema),
        SessionConfig {
            interval: Duration::ZERO,
            ..SessionConfig::default()
        },
    );
    let bytes = include_bytes!("../../mi-model/fixtures/device/synthetic-keymap-multichunk.json");
    let inject = link.inject.clone();
    let server = tokio::spawn(async move {
        let mut offsets = vec![];
        while let Some(r) = requests.recv().await {
            let r: Value = serde_json::from_slice(&r).unwrap();
            let result = if r["method"] == "device.status" {
                json!({"version":"0.6.3"})
            } else if r["method"] == "fs.list" {
                json!([{"name":"keymap.json","size":12466,"checksum":"29982c129d7250b624d483d1bf2a29544271507e"}])
            } else {
                use base64::Engine;
                assert_eq!(r["method"], "fs.readbin");
                assert_eq!(r["params"]["file"], "keymap.json");
                let offset = r["params"]["offset"].as_u64().unwrap() as usize;
                let len = r["params"]["len"].as_u64().unwrap() as usize;
                assert!(len <= 3072);
                offsets.push(offset);
                json!({"total_size":12466,"data":base64::engine::general_purpose::STANDARD.encode(&bytes[offset..offset+len])})
            };
            inject
                .send(
                    json!({"id":r["id"],"result":result})
                        .to_string()
                        .into_bytes(),
                )
                .unwrap();
            if r["method"] == "fs.list" && !offsets.is_empty() {
                return offsets;
            }
        }
        offsets
    });
    s.read_identity_info().await.unwrap();
    let c = s.read_config_snapshot().await.unwrap();
    assert_eq!(c.profiles[1].id, 42);
    assert_eq!(server.await.unwrap(), [0, 3072, 6144, 9216, 12288]);
    assert_eq!(s.snapshot().unwrap().files.keymap.bytes.as_ref(), bytes);
    s.shutdown().await;
}
