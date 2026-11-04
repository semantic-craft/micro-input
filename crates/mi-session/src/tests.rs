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
        notices.recv().await.unwrap().notification,
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
        crate::ReceivedNotification {
            notification: HostNotification::Radial(_),
            ..
        }
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
        crate::ReceivedNotification {
            notification: HostNotification::Radial(_),
            ..
        }
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

#[tokio::test]
async fn vendor_binding_candidate_is_readable_without_claiming_physical_input() {
    let (s, link, mut requests) = setup(500);
    let mut monitor = s.subscribe_input();
    for (act, expected) in [
        (1, crate::InputEventKind::CandidatePressed),
        (0, crate::InputEventKind::CandidateReleased),
    ] {
        link.inject
            .send(
                json!({"m":"v.oai.hid","p":{"k":"AG00","act":act}})
                    .to_string()
                    .into_bytes(),
            )
            .unwrap();
        let event = monitor.next().await.unwrap();
        assert_eq!(event.kind, expected);
        assert_eq!(event.binding_id.as_deref(), Some("AG00"));
        assert_eq!(event.logical_switch, None);
        assert!(!event.action_eligible);
        assert!(monitor.pressed_switches().is_empty());
    }
    assert!(requests.try_recv().is_err());
    s.shutdown().await;
}

#[tokio::test]
async fn candidate_variants_preserve_binding_identity_and_do_not_merge_or_count_actions() {
    let (s, link, mut requests) = setup(500);
    let mut monitor = s.subscribe_input();
    // The public source's wide-key sequence remains four separate binding events.
    // Release-before-press, duplicate presses, and AG13/14's release remain diagnostics.
    let inputs = [
        ("AG11", 1),
        ("AG10", 1),
        ("AG10", 0),
        ("AG11", 0),
        ("AG13", 0),
        ("AG13", 1),
        ("AG13", 1),
        ("AG13", 0),
        ("AG14", 1),
        ("AG15", 1),
        ("AG16", 1),
        ("AG17", 1),
        ("AG18", 1),
        ("AG12", 1),
    ];
    for (index, (binding, act)) in inputs.into_iter().enumerate() {
        link.inject
            .send(
                json!({"method":"v.oai.hid","params":{"k":binding,"act":act,"future":"private"}})
                    .to_string()
                    .into_bytes(),
            )
            .unwrap();
        let event = monitor.next().await.unwrap();
        assert_eq!(event.sequence, index as u64 + 1);
        assert_eq!(event.binding_id.as_deref(), Some(binding));
        assert_eq!(
            event.kind,
            if act == 1 {
                crate::InputEventKind::CandidatePressed
            } else {
                crate::InputEventKind::CandidateReleased
            }
        );
        assert_eq!(event.logical_switch, None);
        assert!(!event.action_eligible);
    }
    assert!(monitor.pressed_switches().is_empty());
    assert!(requests.try_recv().is_err());
    s.shutdown().await;
}

#[tokio::test]
async fn malformed_and_other_generation_inputs_remain_unknown_without_false_release() {
    let (s, link, mut requests) = setup(500);
    let mut monitor = s.subscribe_input();
    let mut inputs = vec![];
    for binding in [
        "AG19",
        "AG0",
        "AG000",
        "AG-1",
        "AG００",
        "ACT06",
        "ENC_CW",
        "ENC_CC",
        "ENC_CLK",
        "KV_OAI_AG00",
    ] {
        inputs.push(json!({"m":"v.oai.hid","p":{"k":binding,"act":1}}));
    }
    for act in [
        json!(2),
        json!(-1),
        json!(1.0),
        json!(0.0),
        json!("1"),
        json!(true),
        json!(null),
        json!([]),
    ] {
        inputs.push(json!({"m":"v.oai.hid","p":{"k":"AG00","act":act}}));
    }
    inputs.extend([
        json!({"m":"v.oai.hid","p":{"act":1}}),
        json!({"m":"v.oai.hid","p":{"k":0,"act":1}}),
        json!({"m":"v.oai.hid","p":{"k":"AG00"}}),
        json!({"m":"v.oai.hid","p":[]}),
        json!({"m":"v.oai.hid","p":{"k":"AG00","act":1},"params":{"k":"AG00","act":0}}),
        json!({"m":"v.oai.hid","p":{"k":"AG00","act":1},"id":7}),
        json!({"m":"v.oai.hid","p":{"k":"AG00","act":1},"i":null}),
        json!({"m":"v.oai.rad","p":{"a":0.75,"d":1}}),
        json!({"m":"kb.radial","p":{"k":"AG00","act":1}}),
        json!({"m":"kb.sa.exec","p":{"k":"AG00","act":1}}),
    ]);
    for raw in inputs {
        link.inject.send(raw.to_string().into_bytes()).unwrap();
        let event = monitor.next().await.unwrap();
        assert_eq!(event.kind, crate::InputEventKind::UnknownNotification);
        assert_eq!(event.binding_id, None);
        assert_eq!(event.logical_switch, None);
        assert!(!event.action_eligible);
    }
    assert!(requests.try_recv().is_err());
    s.shutdown().await;
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

#[tokio::test]
async fn foreign_response_echo_does_not_kill_reader_or_satisfy_current_request() {
    let (link, mut requests) = MockLink::new();
    let s = DeviceSession::new(
        link.clone(),
        Box::new(mi_proto::NativeFramer::default()),
        Arc::new(ReadOnlySchema),
        SessionConfig::default(),
    );
    let mut notices = s.notifications();
    for value in [
        json!({"id":1000,"method":"synthetic.foreign","params":{},"result":{}}),
        json!({"method":"kb.radial"}),
    ] {
        for report in mi_proto::NativeFramer::default()
            .encode(value.to_string().as_bytes())
            .unwrap()
        {
            link.inject.send(report).unwrap();
        }
    }
    tokio::time::timeout(Duration::from_secs(1), notices.recv())
        .await
        .unwrap()
        .unwrap();
    let session = s.clone();
    let task =
        tokio::spawn(async move { session.request(Method::DeviceStatus, Value::Null).await });
    let report = requests.recv().await.unwrap();
    let request: Value = serde_json::from_slice(&report[3..3 + report[2] as usize]).unwrap();
    for value in [
        json!({"id":1000,"method":"synthetic.foreign","params":{},"result":{}}),
        json!({"id":request["id"],"method":"sys.version","result":{"version":"wrong-method"}}),
        json!({"id":request["id"],"method":"device.status","result":{"version":"owned"}}),
    ] {
        for report in mi_proto::NativeFramer::default()
            .encode(value.to_string().as_bytes())
            .unwrap()
        {
            link.inject.send(report).unwrap();
        }
    }
    assert_eq!(task.await.unwrap().unwrap()["result"]["version"], "owned");
    assert!(s.terminal_failure().is_none());
    s.shutdown().await;
}

#[tokio::test]
async fn owned_malformed_response_remains_an_error_after_routing() {
    let (link, mut requests) = MockLink::new();
    let s = DeviceSession::new(
        link.clone(),
        Box::new(mi_proto::NativeFramer::default()),
        Arc::new(ReadOnlySchema),
        SessionConfig::default(),
    );
    let session = s.clone();
    let task =
        tokio::spawn(async move { session.request(Method::DeviceStatus, Value::Null).await });
    let report = requests.recv().await.unwrap();
    let request: Value = serde_json::from_slice(&report[3..3 + report[2] as usize]).unwrap();
    let value = json!({"id":request["id"],"method":"device.status","params":{},"result":{}});
    for report in mi_proto::NativeFramer::default()
        .encode(value.to_string().as_bytes())
        .unwrap()
    {
        link.inject.send(report).unwrap();
    }
    assert!(matches!(
        task.await.unwrap(),
        Err(SessionError::Protocol(_))
    ));
    assert!(s.terminal_failure().is_some());
    s.shutdown().await;
}

#[tokio::test]
async fn waiting_for_session_end_observes_disconnect_without_polling() {
    let (session, _, _) = setup(100);
    let waiter = session.clone();
    let ended = tokio::spawn(async move { waiter.wait_disconnected().await });
    tokio::task::yield_now().await;
    assert!(!ended.is_finished());
    session.shutdown().await;
    tokio::time::timeout(Duration::from_millis(100), ended)
        .await
        .unwrap()
        .unwrap();
    // Calling after shutdown must not miss the earlier notification.
    tokio::time::timeout(Duration::from_millis(100), session.wait_disconnected())
        .await
        .unwrap();
}

#[tokio::test]
async fn cancelled_read_retries_with_fresh_request_and_rejects_late_response() {
    let (session, link, mut requests) = setup(500);
    let first = session.clone();
    let cancelled = tokio::spawn(async move { first.read_identity_info().await });
    let old: Value = serde_json::from_slice(&requests.recv().await.unwrap()).unwrap();
    cancelled.abort();
    assert!(cancelled.await.unwrap_err().is_cancelled());
    let next = session.clone();
    let retry = tokio::spawn(async move { next.read_identity_info().await });
    let current: Value = serde_json::from_slice(&requests.recv().await.unwrap()).unwrap();
    assert_ne!(old["id"], current["id"]);
    link.inject
        .send(
            json!({"id": old["id"], "result": {"version": "old"}})
                .to_string()
                .into_bytes(),
        )
        .unwrap();
    tokio::task::yield_now().await;
    assert!(!retry.is_finished());
    link.inject
        .send(
            json!({"id": current["id"], "result": {"version": "new"}})
                .to_string()
                .into_bytes(),
        )
        .unwrap();
    assert_eq!(
        retry.await.unwrap().unwrap().version.as_deref(),
        Some("new")
    );
    session.shutdown().await;
}

struct StalledWriteLink(Arc<MockLink>, Arc<Notify>);
#[async_trait::async_trait]
impl HidLink for StalledWriteLink {
    async fn write(&self, _: &[u8]) -> Result<(), TransportError> {
        self.1.notify_one();
        std::future::pending().await
    }
    async fn read(&self) -> Result<Vec<u8>, TransportError> {
        self.0.read().await
    }
    fn close(&self) {
        self.0.close();
    }
}
#[tokio::test]
async fn stalled_report_write_is_bounded_and_requires_a_new_session() {
    let (link, _) = MockLink::new();
    let session = DeviceSession::new(
        Arc::new(StalledWriteLink(link, Arc::new(Notify::new()))),
        Box::new(TestFramer),
        Arc::new(ReadOnlySchema),
        SessionConfig {
            timeout: Duration::from_millis(20),
            interval: Duration::ZERO,
        },
    );
    let result = tokio::time::timeout(
        Duration::from_millis(200),
        session.request(Method::SysVersion, Value::Null),
    )
    .await;
    assert!(matches!(result, Ok(Err(SessionError::Timeout))));
    assert!(matches!(
        session.request(Method::SysVersion, Value::Null).await,
        Err(SessionError::Disconnected)
    ));
    session.shutdown().await;
}

#[tokio::test]
async fn cancelling_incomplete_send_invalidates_session() {
    let (link, _) = MockLink::new();
    let started = Arc::new(Notify::new());
    let session = DeviceSession::new(
        Arc::new(StalledWriteLink(link, started.clone())),
        Box::new(TestFramer),
        Arc::new(ReadOnlySchema),
        SessionConfig::default(),
    );
    let held = session.clone();
    let request = tokio::spawn(async move { held.request(Method::SysVersion, Value::Null).await });
    started.notified().await;
    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());
    assert!(matches!(
        session.request(Method::SysVersion, Value::Null).await,
        Err(SessionError::Disconnected)
    ));
    session.shutdown().await;
}

#[tokio::test]
// Physical 0.6.3 capture, 2026-11-03: operator pressed left once, then right once.
// Candidate and mapped subscriptions both retain AG00 then AG01; only the old slot mapping was reversed.
async fn verified_input_maps_observed_row0_order_joystick_sectors_and_merges_wide_key() {
    let (s, link, _) = setup(500);
    let mut monitor = s.subscribe_verified_input();
    for raw in [
        json!({"m":"v.oai.hid","p":{"k":"AG00","act":1}}),
        json!({"m":"v.oai.hid","p":{"k":"AG00","act":0}}),
        json!({"m":"v.oai.hid","p":{"k":"AG01","act":1}}),
        json!({"m":"v.oai.hid","p":{"k":"AG01","act":0}}),
        json!({"m":"v.oai.hid","p":{"k":"AG11","act":1}}),
        json!({"m":"v.oai.hid","p":{"k":"AG10","act":1}}),
        json!({"m":"v.oai.hid","p":{"k":"AG11","act":0}}),
        json!({"m":"v.oai.hid","p":{"k":"AG10","act":0}}),
        json!({"m":"v.oai.hid","p":{"k":"AG15","act":1}}),
    ] {
        link.inject.send(raw.to_string().into_bytes()).unwrap();
    }
    let e1 = monitor.next().await.unwrap();
    assert_eq!(e1.kind, crate::InputEventKind::Pressed);
    assert_eq!(e1.binding_id.as_deref(), Some("AG00"));
    assert_eq!(e1.logical_switch.as_deref(), Some("logical-0"));
    assert_eq!(e1.physical_control.as_deref(), Some("row0-col0-left"));
    assert!(e1.action_eligible);

    let e2 = monitor.next().await.unwrap();
    assert_eq!(e2.kind, crate::InputEventKind::Released);
    assert_eq!(e2.logical_switch.as_deref(), Some("logical-0"));

    let e3 = monitor.next().await.unwrap();
    assert_eq!(e3.kind, crate::InputEventKind::Pressed);
    assert_eq!(e3.binding_id.as_deref(), Some("AG01"));
    assert_eq!(e3.logical_switch.as_deref(), Some("logical-1"));
    assert_eq!(e3.physical_control.as_deref(), Some("row0-col1-right"));

    let e4 = monitor.next().await.unwrap();
    assert_eq!(e4.kind, crate::InputEventKind::Released);
    assert_eq!(e4.logical_switch.as_deref(), Some("logical-1"));

    // AG11 press + AG10 press + AG11 release + AG10 release -> 1 Pressed + 1 Released on logical-10
    let wide_press = monitor.next().await.unwrap();
    assert_eq!(wide_press.kind, crate::InputEventKind::Pressed);
    assert_eq!(wide_press.binding_id.as_deref(), Some("AG10"));
    assert_eq!(wide_press.logical_switch.as_deref(), Some("logical-10"));
    assert_eq!(wide_press.keycap_group.as_deref(), Some("wide-bottom-2u"));
    assert!(wide_press.action_eligible);

    let wide_release = monitor.next().await.unwrap();
    assert_eq!(wide_release.kind, crate::InputEventKind::Released);
    assert_eq!(wide_release.binding_id.as_deref(), Some("AG10"));
    assert_eq!(wide_release.logical_switch.as_deref(), Some("logical-10"));
    assert!(monitor.active_wide_switches().is_empty());

    let joy_north = monitor.next().await.unwrap();
    assert_eq!(joy_north.kind, crate::InputEventKind::Pressed);
    assert_eq!(joy_north.binding_id.as_deref(), Some("AG15"));
    assert_eq!(joy_north.logical_switch.as_deref(), Some("logical-16"));
    assert_eq!(
        joy_north.physical_control.as_deref(),
        Some("joystick-sector-north")
    );
    s.shutdown().await;
}

#[tokio::test]
async fn verified_input_decodes_stock_codex_act_enc_and_radial_joystick() {
    use crate::InputDecoder;
    let decoder = crate::VerifiedInputDecoder;
    assert_eq!(
        decoder.decode(&crate::HostNotification::Unknown(
            json!({"m":"v.oai.rad","p":{"a":0.75,"d":0.9}})
        )),
        Some((crate::InputEventKind::Pressed, "AG15".into()))
    );
    assert_eq!(
        decoder.decode(&crate::HostNotification::Unknown(
            json!({"m":"v.oai.rad","p":{"a":0.25,"d":0.9}})
        )),
        Some((crate::InputEventKind::Pressed, "AG17".into()))
    );
    assert_eq!(
        decoder.decode(&crate::HostNotification::Unknown(
            json!({"m":"v.oai.rad","p":{"a":5.0,"d":0.9}})
        )),
        None
    );

    let (s, link, _) = setup(500);
    let mut monitor = s.subscribe_verified_input();
    for raw in [
        // ACT10 + ACT11 wide key on stock Codex layer coalesces into AG10 (logical-10)
        json!({"m":"v.oai.hid","p":{"k":"ACT10","act":1}}),
        json!({"m":"v.oai.hid","p":{"k":"ACT11","act":1}}),
        json!({"m":"v.oai.hid","p":{"k":"ACT10","act":0}}),
        json!({"m":"v.oai.hid","p":{"k":"ACT11","act":0}}),
        // ENC_CC with act=2 (firmware 0.6.3 encoder step pulse) -> AG13 (logical-13)
        json!({"m":"v.oai.hid","p":{"k":"ENC_CC","act":2}}),
        // ENC_CLK press + release -> ENC_CLK (logical-15)
        json!({"m":"v.oai.hid","p":{"k":"ENC_CLK","act":1}}),
        json!({"m":"v.oai.hid","p":{"k":"ENC_CLK","act":0}}),
        // v.oai.rad continuous radial joystick in normalized turns [0.0, 1.0):
        // East (a=0.0, d=0.9), repeated East (a=0.05, d=0.95, ignored), center release (d=0.05)
        json!({"m":"v.oai.rad","p":{"a":0.0,"d":0.9}}),
        json!({"m":"v.oai.rad","p":{"a":0.05,"d":0.95}}),
        json!({"m":"v.oai.rad","p":{"a":0.0,"d":0.05}}),
        // South (a=0.25 -> AG17), West (a=0.5 -> AG16), North (a=0.75 -> AG15)
        json!({"m":"v.oai.rad","p":{"a":0.25,"d":0.9}}),
        json!({"m":"v.oai.rad","p":{"a":0.25,"d":0.05}}),
        json!({"m":"v.oai.rad","p":{"a":0.5,"d":0.9}}),
        json!({"m":"v.oai.rad","p":{"a":0.5,"d":0.05}}),
        json!({"m":"v.oai.rad","p":{"a":0.75,"d":0.9}}),
        json!({"m":"v.oai.rad","p":{"a":0.75,"d":0.05}}),
        // Out-of-range angle (a=5.0) must be rejected as UnknownNotification
        json!({"m":"v.oai.rad","p":{"a":5.0,"d":0.95}}),
    ] {
        link.inject.send(raw.to_string().into_bytes()).unwrap();
    }

    let wide_press = monitor.next().await.unwrap();
    assert_eq!(wide_press.kind, crate::InputEventKind::Pressed);
    assert_eq!(wide_press.binding_id.as_deref(), Some("AG10"));
    assert_eq!(wide_press.logical_switch.as_deref(), Some("logical-10"));
    assert_eq!(wide_press.keycap_group.as_deref(), Some("wide-bottom-2u"));

    let wide_release = monitor.next().await.unwrap();
    assert_eq!(wide_release.kind, crate::InputEventKind::Released);
    assert_eq!(wide_release.binding_id.as_deref(), Some("AG10"));
    assert_eq!(wide_release.logical_switch.as_deref(), Some("logical-10"));

    let enc_cc = monitor.next().await.unwrap();
    assert_eq!(enc_cc.kind, crate::InputEventKind::Pressed);
    assert_eq!(enc_cc.binding_id.as_deref(), Some("AG13"));
    assert_eq!(enc_cc.logical_switch.as_deref(), Some("logical-13"));
    assert!(enc_cc.action_eligible);
    assert!(
        monitor.pressed_switches().is_empty(),
        "momentary encoder step must not stay stuck in pressed_switches"
    );

    let enc_clk = monitor.next().await.unwrap();
    assert_eq!(enc_clk.kind, crate::InputEventKind::Pressed);
    assert_eq!(enc_clk.binding_id.as_deref(), Some("ENC_CLK"));
    assert_eq!(enc_clk.logical_switch.as_deref(), Some("logical-15"));
    assert!(enc_clk.action_eligible);
    assert_eq!(monitor.pressed_switches(), vec!["logical-15".to_string()]);

    let enc_clk_release = monitor.next().await.unwrap();
    assert_eq!(enc_clk_release.kind, crate::InputEventKind::Released);
    assert_eq!(enc_clk_release.binding_id.as_deref(), Some("ENC_CLK"));
    assert!(monitor.pressed_switches().is_empty());

    for (expected_ag, expected_switch) in [
        ("AG18", "logical-19"),
        ("AG17", "logical-18"),
        ("AG16", "logical-17"),
        ("AG15", "logical-16"),
    ] {
        let press = monitor.next().await.unwrap();
        assert_eq!(press.kind, crate::InputEventKind::Pressed);
        assert_eq!(press.binding_id.as_deref(), Some(expected_ag));
        assert_eq!(press.logical_switch.as_deref(), Some(expected_switch));
        assert!(press.action_eligible);

        if expected_ag == "AG18" {
            let held = monitor.next().await.unwrap();
            assert_eq!(held.radial_center, Some(false));
            assert!(!held.action_eligible);
        }
        let release = monitor.next().await.unwrap();
        assert_eq!(release.kind, crate::InputEventKind::Released);
        assert_eq!(release.binding_id.as_deref(), Some(expected_ag));
        assert_eq!(release.logical_switch.as_deref(), Some(expected_switch));
        assert!(release.action_eligible);
    }

    let invalid_rad = monitor.next().await.unwrap();
    assert_eq!(invalid_rad.kind, crate::InputEventKind::UnknownNotification);
    assert!(!invalid_rad.action_eligible);

    // Sector-to-sector roll without returning to center (North AG15 -> East AG18),
    // followed by release at d=0.25 (< 0.35 threshold).
    for raw in [
        json!({"m":"v.oai.rad","p":{"a":0.75,"d":0.9}}),
        json!({"m":"v.oai.rad","p":{"a":0.0,"d":0.9}}),
        json!({"m":"v.oai.rad","p":{"a":0.0,"d":0.25}}),
    ] {
        link.inject.send(raw.to_string().into_bytes()).unwrap();
    }
    let roll_north = monitor.next().await.unwrap();
    assert_eq!(roll_north.kind, crate::InputEventKind::Pressed);
    assert_eq!(roll_north.binding_id.as_deref(), Some("AG15"));
    assert_eq!(roll_north.logical_switch.as_deref(), Some("logical-16"));
    assert_eq!(monitor.pressed_switches(), vec!["logical-16".to_string()]);

    let roll_east = monitor.next().await.unwrap();
    assert_eq!(roll_east.kind, crate::InputEventKind::Pressed);
    assert_eq!(roll_east.binding_id.as_deref(), Some("AG18"));
    assert_eq!(roll_east.logical_switch.as_deref(), Some("logical-19"));
    assert_eq!(
        monitor.pressed_switches(),
        vec!["logical-19".to_string()],
        "rolling directly to a new radial sector must clear the previous sector from pressed_switches"
    );

    let release_east = monitor.next().await.unwrap();
    assert_eq!(release_east.kind, crate::InputEventKind::Released);
    assert_eq!(release_east.binding_id.as_deref(), Some("AG18"));
    assert_eq!(release_east.logical_switch.as_deref(), Some("logical-19"));
    assert!(monitor.pressed_switches().is_empty());

    s.shutdown().await;
}

#[tokio::test]
async fn ingress_watermark_precedes_decode_and_preserves_unread_radial_state() {
    let (session, link, _rx) = setup(500);
    let mut monitor = session.subscribe_verified_input();
    for raw in [
        json!({"m":"v.oai.rad","p":{"a":0.0,"d":0.0}}),
        json!({"m":"v.oai.rad","p":{"a":0.75,"d":0.9}}),
    ] {
        link.inject.send(raw.to_string().into_bytes()).unwrap();
    }
    tokio::time::timeout(Duration::from_secs(1), async {
        while session.input_watermark() < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let cutoff = session.input_watermark();
    assert_eq!(cutoff, 2);
    assert_eq!(monitor.next().await.unwrap().radial_center, Some(true));
    let unread_north = monitor.next().await.unwrap();
    assert_eq!(unread_north.ingress_sequence, cutoff);
    assert_eq!(unread_north.radial_center, Some(false));
    link.inject
        .send(
            json!({"m":"v.oai.rad","p":{"a":0.0,"d":0.9}})
                .to_string()
                .into_bytes(),
        )
        .unwrap();
    let east = monitor.next().await.unwrap();
    assert!(east.ingress_sequence > cutoff);
    assert_eq!(east.radial_center, Some(false));
    session.shutdown().await;
}

// Candidate bytes can be inspected and framed offline, but cannot reach a native link.
#[tokio::test]
async fn encoded_mutation_candidates_cannot_bypass_production_exit() {
    use mi_proto::lighting_candidate::{
        encode_lighting_preview_candidate, LightingPreviewCandidate,
    };
    let (s, link, _) = setup(100);
    let mut queue = s.queue.lock().await;
    let id = queue.next_id;
    let lighting =
        encode_lighting_preview_candidate(&LightingPreviewCandidate::default(), id).unwrap();
    let keymap = mi_proto::mutation::keymap_chunk(b"{}", 0, id).unwrap();
    for (method, bytes) in [
        ("lights.preview", lighting.clone()),
        ("fs.writebin", keymap.clone()),
        ("device.status", lighting),
        ("fs.list", keymap),
    ] {
        assert!(s.exchange_encoded(&mut queue, method, bytes).await.is_err());
        assert_eq!(queue.next_id, id);
        assert!(link.writes.lock().unwrap().is_empty());
    }
    drop(queue);
    s.shutdown().await;
}

#[tokio::test]
async fn encoded_readonly_requests_never_forward_ambiguous_original_bytes() {
    let (session, link, mut requests) = setup(500);
    let task_session = session.clone();
    let task = tokio::spawn(async move {
        let mut queue = task_session.queue.lock().await;
        let bytes = format!(
            r#"{{"id":{},"method":"sys.bootloader","method":"device.status","params":null}}"#,
            queue.next_id
        )
        .into_bytes();
        task_session
            .exchange_encoded(&mut queue, "device.status", bytes)
            .await
    });
    let bytes = tokio::time::timeout(Duration::from_millis(500), requests.recv())
        .await
        .unwrap()
        .unwrap();
    let request: Value = serde_json::from_slice(&bytes).unwrap();
    let expected = JsonCodec
        .encode(
            &RpcRequest::new(
                Method::DeviceStatus,
                Value::Null,
                request["id"].as_u64().unwrap() as u16,
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(bytes, expected);
    assert!(!String::from_utf8(bytes).unwrap().contains("bootloader"));
    link.inject
        .send(
            json!({"id":request["id"],"result":{}})
                .to_string()
                .into_bytes(),
        )
        .unwrap();
    assert!(task.await.unwrap().is_ok());
    session.shutdown().await;
}

struct BatchOnlyLink {
    inner: Arc<MockLink>,
    batches: Arc<Mutex<Vec<usize>>>,
}
#[async_trait::async_trait]
impl HidLink for BatchOnlyLink {
    async fn write(&self, _: &[u8]) -> Result<(), TransportError> {
        panic!("RPC reports must use one batch")
    }
    async fn write_batch(&self, reports: &[Vec<u8>]) -> Result<(), TransportError> {
        self.batches.lock().unwrap().push(reports.len());
        for report in reports {
            self.inner.write(report).await?;
        }
        Ok(())
    }
    async fn read(&self) -> Result<Vec<u8>, TransportError> {
        self.inner.read().await
    }
    fn close(&self) {
        self.inner.close();
    }
}
#[tokio::test]
async fn multiframe_request_uses_one_transport_batch() {
    let (raw, mut requests) = MockLink::new();
    let batches = Arc::new(Mutex::new(Vec::new()));
    let link = Arc::new(BatchOnlyLink {
        inner: raw.clone(),
        batches: batches.clone(),
    });
    let s = DeviceSession::new(
        link,
        Box::<mi_proto::NativeFramer>::default(),
        Arc::new(ReadOnlySchema),
        SessionConfig::default(),
    );
    let task = {
        let s = s.clone();
        tokio::spawn(async move {
            s.request(
                Method::FsReadbin,
                json!({"file":"keymap.json","offset":0,"len":3072}),
            )
            .await
        })
    };
    let mut framer = mi_proto::NativeFramer::default();
    let request = loop {
        let frame = requests.recv().await.unwrap();
        if let Some(line) = framer.feed(&frame).unwrap().pop() {
            break serde_json::from_slice::<Value>(&line).unwrap();
        }
    };
    assert_eq!(batches.lock().unwrap().len(), 1);
    assert!(batches.lock().unwrap()[0] > 1);
    for report in mi_proto::NativeFramer::default()
        .encode(
            json!({"id":request["id"],"result":null})
                .to_string()
                .as_bytes(),
        )
        .unwrap()
    {
        raw.inject.send(report).unwrap();
    }
    assert!(task.await.unwrap().is_ok());
    s.shutdown().await;
}

#[tokio::test]
async fn radial_positions_preserve_continuous_motion_without_upgrading_candidate_sources() {
    let (session, link, _rx) = setup(500);
    let mut verified = session.subscribe_verified_input();
    let mut candidate = session.subscribe_input();
    for (angle, distance, kind) in [
        (0.0, 0.0, InputEventKind::UnknownNotification),
        (0.0, 0.9, InputEventKind::Pressed),
        (0.08, 0.9, InputEventKind::UnknownNotification),
        (0.125, 0.4, InputEventKind::UnknownNotification),
        (1.0, 0.0, InputEventKind::Released),
    ] {
        link.inject
            .send(
                json!({"m":"v.oai.rad","p":{"a":angle,"d":distance}})
                    .to_string()
                    .into_bytes(),
            )
            .unwrap();
        let event = verified.next().await.unwrap();
        let position = event.radial_position.unwrap();
        assert_eq!(position.angle_turns, angle % 1.0);
        assert_eq!(position.distance, distance);
        assert_eq!(event.kind, kind);
        assert!(candidate.next().await.unwrap().radial_position.is_none());
    }
    for raw in [
        json!({"m":"v.oai.rad","p":{"a":1.1,"d":0.9}}),
        json!({"m":"v.oai.rad","p":{"a":0.1,"d":-0.1}}),
        json!({"m":"v.oai.rad","p":{"a":null,"d":0.9}}),
        json!({"m":"kb.radial","p":{"a":0.1,"d":0.9}}),
    ] {
        link.inject.send(raw.to_string().into_bytes()).unwrap();
        let event = verified.next().await.unwrap();
        assert!(event.radial_position.is_none());
        assert!(!event.action_eligible);
    }
    for raw in [
        json!({"m":"v.oai.rad","method":"other","p":{"a":0.1,"d":0.9}}),
        json!({"m":"v.oai.rad","p":{"a":0.1,"d":0.9},"params":{"a":0.2,"d":0.9}}),
        json!({"m":"v.oai.rad","id":1,"p":{"a":0.1,"d":0.9}}),
    ] {
        assert!(VerifiedInputDecoder
            .decode(&HostNotification::Unknown(raw))
            .is_none());
    }
    session.shutdown().await;
}
