use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mi_model::slot_table::SLOT_COUNT;
use mi_model::{Binding, Joystick, Layer, Lights, SaId, Token};
use mi_proto::v070_capabilities::{SaRgbEffect, SaRgbUpdate};
use mi_proto::{Framer, ProtoError};
use mi_session::{DeviceSession, ReadOnlySchema, SessionConfig};
use mi_transport::{HidLink, LinkBackend, TransportError};
use serde_json::{json, Value};

use super::*;

fn sample_custom_layer(tokens: &[&str]) -> Layer {
    let keys = std::array::from_fn(|idx| {
        let tok = tokens.get(idx).copied().unwrap_or("KC_NO");
        Binding::Basic(Token(tok.to_string()))
    });
    let encoders = std::array::from_fn(|_| Binding::Empty);
    Layer {
        id: json!(1),
        name: "Custom".into(),
        color: json!(0),
        keys,
        encoders,
        joystick: Joystick {
            kind: json!("4way"),
            sectors: vec![Binding::Empty; 4],
        },
        lights: Lights {
            backlight: Value::Null,
            underglow: Value::Null,
        },
    }
}

fn sa(raw: u32) -> SaId {
    SaId::new(raw).unwrap()
}

fn working_style() -> SaRgbLitStyle {
    match map_session_kind_to_sa_lighting(FourAppSessionKind::Working) {
        DesiredSaLighting::Override(s) => s,
        DesiredSaLighting::DefaultOnboard => unreachable!(),
    }
}

fn attention_style() -> SaRgbLitStyle {
    match map_session_kind_to_sa_lighting(FourAppSessionKind::NeedsAttention) {
        DesiredSaLighting::Override(s) => s,
        DesiredSaLighting::DefaultOnboard => unreachable!(),
    }
}

fn completed_style() -> SaRgbLitStyle {
    match map_session_kind_to_sa_lighting(FourAppSessionKind::BlueDotCompleted) {
        DesiredSaLighting::Override(s) => s,
        DesiredSaLighting::DefaultOnboard => unreachable!(),
    }
}

fn error_style() -> SaRgbLitStyle {
    match map_session_kind_to_sa_lighting(FourAppSessionKind::Error) {
        DesiredSaLighting::Override(s) => s,
        DesiredSaLighting::DefaultOnboard => unreachable!(),
    }
}

struct TestFramer;

impl Framer for TestFramer {
    fn encode(&self, payload: &[u8]) -> Result<Vec<Vec<u8>>, ProtoError> {
        Ok(vec![payload.to_vec()])
    }

    fn feed(&mut self, report: &[u8]) -> Result<Vec<Vec<u8>>, ProtoError> {
        Ok(vec![report.to_vec()])
    }

    fn clear(&mut self) {}
}

struct GatedMockLink {
    in_tx: Mutex<Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>>,
    in_rx: tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>>,
    out_tx: tokio::sync::mpsc::UnboundedSender<Value>,
    sa_rgb_frames: Mutex<Vec<Value>>,
}

impl GatedMockLink {
    fn new() -> (Arc<Self>, tokio::sync::mpsc::UnboundedReceiver<Value>) {
        let (in_tx, in_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel();
        (
            Arc::new(Self {
                in_tx: Mutex::new(Some(in_tx)),
                in_rx: tokio::sync::Mutex::new(in_rx),
                out_tx,
                sa_rgb_frames: Mutex::new(Vec::new()),
            }),
            out_rx,
        )
    }

    fn reply_ok(&self, req: &Value) {
        if let Some(tx) = self.in_tx.lock().unwrap().as_ref() {
            let reply = json!({
                "id": req["id"],
                "method": req["method"],
                "result": {}
            });
            let _ = tx.send(serde_json::to_vec(&reply).unwrap());
        }
    }

    fn frames(&self) -> Vec<Value> {
        self.sa_rgb_frames.lock().unwrap().clone()
    }
}

impl HidLink for GatedMockLink {
    fn is_exclusive_vendor(&self) -> bool {
        true
    }

    fn backend(&self) -> LinkBackend {
        LinkBackend::MacHidapi267Numbered
    }

    fn write<'a, 'c, 'b>(
        &'a self,
        report: &'c [u8],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), TransportError>> + Send + 'b>>
    where
        'a: 'b,
        'c: 'b,
        Self: 'b,
    {
        Box::pin(async move {
            let req: Value = serde_json::from_slice(report).unwrap();
            if req.get("method").and_then(Value::as_str) == Some("kb.sa.rgb") {
                self.sa_rgb_frames
                    .lock()
                    .unwrap()
                    .push(req["params"].clone());
            }
            let _ = self.out_tx.send(req);
            Ok(())
        })
    }

    fn read<'a, 'b>(
        &'a self,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<u8>, TransportError>> + Send + 'b>,
    >
    where
        'a: 'b,
        Self: 'b,
    {
        Box::pin(async move {
            let mut rx = self.in_rx.lock().await;
            rx.recv().await.ok_or(TransportError::Disconnected)
        })
    }

    fn close(&self) {
        self.in_tx.lock().unwrap().take();
    }
}

fn build_session(
    rt: &tokio::runtime::Runtime,
    link: Arc<GatedMockLink>,
    out_rx: &mut tokio::sync::mpsc::UnboundedReceiver<Value>,
    timeout: Duration,
) -> Arc<DeviceSession> {
    rt.block_on(async {
        let session = DeviceSession::new(
            link.clone(),
            Box::new(TestFramer),
            Arc::new(ReadOnlySchema),
            SessionConfig {
                timeout,
                interval: Duration::ZERO,
            },
        );
        let reply_fut = async {
            let req = out_rx.recv().await.expect("device.status request");
            if let Some(tx) = link.in_tx.lock().unwrap().as_ref() {
                let reply = json!({
                    "id": req["id"],
                    "method": "device.status",
                    "result": {
                        "version": "0.7.0-rc.16",
                        "profile_index": 0,
                        "layer_index": 1,
                        "battery": 95,
                        "is_charging": false
                    }
                });
                let _ = tx.send(serde_json::to_vec(&reply).unwrap());
            }
        };
        let (res, _) = tokio::join!(session.read_identity_info(), reply_fut);
        res.unwrap();
        session
    })
}

#[test]
fn all_five_session_kinds_and_protected_layer_guard_map_deterministically() {
    assert_eq!(
        map_session_kind_to_sa_lighting(FourAppSessionKind::Idle),
        DesiredSaLighting::DefaultOnboard
    );
    let styles = [
        working_style(),
        attention_style(),
        completed_style(),
        error_style(),
    ];
    for style in styles {
        let update = style.to_sa_rgb_update(vec![101]);
        assert!(update.validate().is_ok());
        assert_ne!(style.effect(), SaRgbEffect::Off);
    }
    let unique_colors: BTreeSet<u32> = styles.iter().map(|s| s.rgb()).collect();
    assert_eq!(unique_colors.len(), 4);

    let oai_layer = mi_model::official_chatgpt_layer(json!(0), "Codex");
    let kinds = [
        FourAppSessionKind::Working,
        FourAppSessionKind::NeedsAttention,
        FourAppSessionKind::BlueDotCompleted,
        FourAppSessionKind::Error,
        FourAppSessionKind::Idle,
        FourAppSessionKind::Idle,
    ];
    assert_eq!(
        resolve_desired_for_device_layer(&oai_layer, &kinds, Some(sa(999))),
        LayerLightingResolution::ProtectedOai
    );

    let custom = sample_custom_layer(&[
        "SA_10", "SA_20", "SA_30", "SA_40", "SA_50", "KC_NO", "SA_99",
    ]);
    let LayerLightingResolution::Active(desired) =
        resolve_desired_for_device_layer(&custom, &kinds, None)
    else {
        panic!("expected Active resolution");
    };
    assert_eq!(desired.get(&sa(10)), Some(&working_style()));
    assert_eq!(desired.get(&sa(20)), Some(&attention_style()));
    assert_eq!(desired.get(&sa(30)), Some(&completed_style()));
    assert_eq!(desired.get(&sa(40)), Some(&error_style()));
    assert!(!desired.contains_key(&sa(50)));
    assert_eq!(desired.get(&sa(99)), Some(&attention_style()));

    let no_sa_layer = sample_custom_layer(&["KC_A", "KC_B"]);
    let LayerLightingResolution::Active(fallback_desired) =
        resolve_desired_for_device_layer(&no_sa_layer, &kinds, Some(sa(777)))
    else {
        panic!("expected Active resolution");
    };
    assert_eq!(
        fallback_desired,
        BTreeMap::from([(sa(777), attention_style())])
    );

    let all_idle = [FourAppSessionKind::Idle; SLOT_COUNT];
    assert_eq!(
        resolve_desired_for_device_layer(&custom, &all_idle, Some(sa(777))),
        LayerLightingResolution::Active(BTreeMap::new())
    );
}

#[test]
fn same_color_dedup_rapid_burst_coalescing_and_roundtrip_elimination() {
    let t0 = Instant::now();
    let mut rec = SessionSaRgbReconciler::new();
    let mut target = BTreeMap::new();
    target.insert(sa(1), working_style());
    target.insert(sa(2), working_style());

    let first = rec.reconcile(t0, DesiredTarget::Active(target.clone()), 0);
    let ReconcileAction::Dispatch(batch1) = first else {
        panic!("expected immediate initial dispatch, got {first:?}");
    };
    assert_eq!(batch1.chunks.len(), 1);
    assert_eq!(batch1.chunks[0].update.sa, vec![1, 2]);
    assert_eq!(
        rec.complete_batch(
            t0 + Duration::from_millis(5),
            batch1.batch_id,
            batch1.chunks.len(),
            1
        ),
        ReconcileAction::NoOp
    );

    assert_eq!(
        rec.reconcile(
            t0 + Duration::from_millis(150),
            DesiredTarget::Active(target.clone()),
            1
        ),
        ReconcileAction::NoOp
    );

    let t1 = t0 + Duration::from_millis(200);
    let mut t1_map = target.clone();
    t1_map.insert(sa(1), error_style());
    let ReconcileAction::Dispatch(batch2) = rec.reconcile(t1, DesiredTarget::Active(t1_map), 1)
    else {
        panic!("expected dispatch after cooldown");
    };
    assert_eq!(batch2.chunks.len(), 1);
    assert_eq!(batch2.chunks[0].update.sa, vec![1]);
    assert_eq!(
        rec.complete_batch(
            t1 + Duration::from_millis(4),
            batch2.batch_id,
            batch2.chunks.len(),
            2
        ),
        ReconcileAction::NoOp
    );

    for (offset_ms, style) in [
        (10, Some(working_style())),
        (25, None),
        (40, Some(completed_style())),
        (60, Some(attention_style())),
    ] {
        let mut burst = target.clone();
        match style {
            Some(s) => {
                burst.insert(sa(1), s);
            }
            None => {
                burst.remove(&sa(1));
            }
        }
        let action = rec.reconcile(
            t1 + Duration::from_millis(offset_ms),
            DesiredTarget::Active(burst),
            2,
        );
        assert_eq!(
            action,
            ReconcileAction::ScheduleWake(t1 + SESSION_SA_RGB_MIN_INTERVAL)
        );
    }

    let ReconcileAction::Dispatch(batch3) = rec.flush_due(t1 + SESSION_SA_RGB_MIN_INTERVAL, 2)
    else {
        panic!("expected coalesced trailing dispatch at 100ms mark");
    };
    assert_eq!(batch3.chunks.len(), 1);
    assert_eq!(
        batch3.chunks[0].update,
        attention_style().to_sa_rgb_update(vec![1])
    );
    assert_eq!(
        rec.complete_batch(
            t1 + Duration::from_millis(105),
            batch3.batch_id,
            batch3.chunks.len(),
            3
        ),
        ReconcileAction::NoOp
    );

    let mut transient = target.clone();
    transient.insert(sa(1), error_style());
    assert!(matches!(
        rec.reconcile(
            t1 + Duration::from_millis(120),
            DesiredTarget::Active(transient),
            3
        ),
        ReconcileAction::ScheduleWake(_)
    ));
    let mut back_to_confirmed = target;
    back_to_confirmed.insert(sa(1), attention_style());
    assert_eq!(
        rec.reconcile(
            t1 + Duration::from_millis(140),
            DesiredTarget::Active(back_to_confirmed),
            3
        ),
        ReconcileAction::NoOp
    );
}

#[test]
fn inflight_error_retries_dirty_ids_and_urgent_clear_bypasses_cooldown() {
    let t0 = Instant::now();
    let mut rec = SessionSaRgbReconciler::new();
    let target = BTreeMap::from([(sa(7), working_style()), (sa(8), error_style())]);

    let ReconcileAction::Dispatch(batch1) =
        rec.reconcile(t0, DesiredTarget::Active(target.clone()), 0)
    else {
        panic!("expected initial dispatch");
    };
    assert_eq!(
        rec.reconcile(
            t0 + Duration::from_millis(10),
            DesiredTarget::Active(BTreeMap::new()),
            0
        ),
        ReconcileAction::NoOp
    );

    let fail_time = t0 + Duration::from_millis(20);
    let after_fail = rec.complete_batch(fail_time, batch1.batch_id, 0, 1);
    assert_eq!(
        after_fail,
        ReconcileAction::ScheduleWake(fail_time + SESSION_SA_RGB_MIN_INTERVAL)
    );
    assert!(rec.has_dirty_keys());

    let ReconcileAction::Dispatch(reset_after_fail) =
        rec.flush_due(fail_time + SESSION_SA_RGB_MIN_INTERVAL, 1)
    else {
        panic!("expected reset retry for unconfirmed failed IDs");
    };
    assert_eq!(reset_after_fail.chunks.len(), 1);
    assert_eq!(
        reset_after_fail.chunks[0].update,
        SaRgbUpdate::reset(vec![7, 8])
    );
    assert_eq!(
        rec.complete_batch(
            fail_time + Duration::from_millis(105),
            reset_after_fail.batch_id,
            reset_after_fail.chunks.len(),
            2
        ),
        ReconcileAction::NoOp
    );
    assert!(!rec.has_dirty_keys());

    let t2 = t0 + Duration::from_millis(300);
    let ReconcileAction::Dispatch(batch2) = rec.reconcile(t2, DesiredTarget::Active(target), 2)
    else {
        panic!("expected active dispatch");
    };
    rec.complete_batch(
        t2 + Duration::from_millis(5),
        batch2.batch_id,
        batch2.chunks.len(),
        4,
    );

    let ReconcileAction::Dispatch(urgent) = rec.reconcile(
        t2 + Duration::from_millis(15),
        DesiredTarget::UrgentClear(UrgentClearReason::ProtectedLayer),
        4,
    ) else {
        panic!("urgent protected-layer clear must bypass 100ms cooldown");
    };
    assert_eq!(urgent.chunks.len(), 1);
    assert_eq!(urgent.chunks[0].update, SaRgbUpdate::reset(vec![7, 8]));
}

#[test]
fn manual_sa_rgb_writes_reconcile_without_losing_dirty_teardown_tracking() {
    let t0 = Instant::now();
    let mut rec = SessionSaRgbReconciler::new();
    let manual_style = SaRgbLitStyle::from_manual_apply(0x33AAFF, 0.75, 0.5, 4).unwrap();

    rec.note_manual_sa_rgb_write(
        ManualSaRgbWrite::Applied {
            sa: sa(55),
            style: Some(manual_style),
        },
        false,
    );
    assert!(rec.has_dirty_keys());
    let teardown = rec.take_dirty_for_teardown();
    assert_eq!(teardown, vec![SaRgbUpdate::reset(vec![55])]);
    assert!(!rec.has_dirty_keys());

    let target = BTreeMap::from([(sa(55), working_style())]);
    let ReconcileAction::Dispatch(b1) = rec.reconcile(t0, DesiredTarget::Active(target.clone()), 0)
    else {
        panic!("expected dispatch");
    };
    rec.complete_batch(
        t0 + Duration::from_millis(5),
        b1.batch_id,
        b1.chunks.len(),
        1,
    );

    rec.note_manual_sa_rgb_write(
        ManualSaRgbWrite::Applied {
            sa: sa(55),
            style: Some(manual_style),
        },
        true,
    );
    let ReconcileAction::Dispatch(b2) = rec.reconcile(
        t0 + Duration::from_millis(150),
        DesiredTarget::Active(target),
        2,
    ) else {
        panic!("manual overwrite of confirmed key must mark key out-of-sync with desired");
    };
    assert_eq!(
        b2.chunks[0].update,
        working_style().to_sa_rgb_update(vec![55])
    );
}

#[test]
fn urgent_protected_clear_while_initial_batch_in_flight_bypasses_cooldown_on_completion() {
    let rt = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap(),
    );
    let (link, mut out_rx) = GatedMockLink::new();
    let session = build_session(&rt, link.clone(), &mut out_rx, Duration::from_secs(2));
    let coord = SaRgbRuntimeCoordinator::new();

    let mut two_chunk_target = BTreeMap::new();
    for raw in 1..=20 {
        two_chunk_target.insert(sa(raw), working_style());
    }
    coord.sync(
        DesiredTarget::Active(two_chunk_target),
        Some(session.clone()),
        &rt,
    );

    let first_req = rt
        .block_on(async { tokio::time::timeout(Duration::from_secs(1), out_rx.recv()).await })
        .expect("timed out waiting for chunk 0")
        .expect("channel closed");
    let first_sa: Vec<u64> = first_req["params"]["sa"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    assert_eq!(first_sa, (1..=16).collect::<Vec<u64>>());

    coord.sync(
        DesiredTarget::UrgentClear(UrgentClearReason::ProtectedLayer),
        Some(session.clone()),
        &rt,
    );

    let release_at = Instant::now();
    link.reply_ok(&first_req);

    let second_req = rt
        .block_on(async { tokio::time::timeout(Duration::from_secs(1), out_rx.recv()).await })
        .expect("timed out waiting for immediate urgent reset")
        .expect("channel closed");
    let reset_latency = release_at.elapsed();
    assert!(
        reset_latency < Duration::from_millis(75),
        "urgent clear must preempt chunk 1 and bypass 100ms cooldown; took {reset_latency:?}"
    );
    assert_eq!(second_req["params"]["e"].as_u64(), Some(0));
    assert!(second_req["params"].get("c").is_none());
    let reset_sa: Vec<u64> = second_req["params"]["sa"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    assert_eq!(reset_sa, (1..=16).collect::<Vec<u64>>());

    link.reply_ok(&second_req);
    let extra =
        rt.block_on(async { tokio::time::timeout(Duration::from_millis(60), out_rx.recv()).await });
    assert!(
        extra.is_err(),
        "unsent Set chunk 1 (17..=20) must never be emitted or reset"
    );
    assert!(!coord.has_dirty_keys());
    assert!(!coord.is_in_flight());
}

#[test]
fn session_none_parks_without_orphaning_in_flight_and_session_id_change_resets_confirmed() {
    let rt = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap(),
    );
    let (link1, mut out_rx1) = GatedMockLink::new();
    let session1 = build_session(&rt, link1.clone(), &mut out_rx1, Duration::from_secs(2));
    let responder1 = rt.spawn({
        let link1 = link1.clone();
        async move {
            while let Some(req) = out_rx1.recv().await {
                link1.reply_ok(&req);
            }
        }
    });

    let coord = SaRgbRuntimeCoordinator::new();
    coord.sync(DesiredTarget::Inactive, None, &rt);
    assert!(!coord.is_in_flight());

    let target = BTreeMap::from([(sa(11), working_style()), (sa(12), error_style())]);
    coord.sync(
        DesiredTarget::Active(target.clone()),
        Some(session1.clone()),
        &rt,
    );

    let deadline = Instant::now() + Duration::from_secs(1);
    while link1.frames().len() < 2 {
        assert!(Instant::now() < deadline, "session1 did not emit 2 frames");
        std::thread::sleep(Duration::from_millis(5));
    }
    while coord.is_in_flight() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(coord.has_dirty_keys());

    coord.sync(DesiredTarget::Inactive, None, &rt);
    assert!(
        !coord.is_in_flight(),
        "sync(..., session: None) must park without setting in_flight"
    );
    assert!(coord.has_dirty_keys());

    std::thread::sleep(SESSION_SA_RGB_MIN_INTERVAL + Duration::from_millis(15));
    let frames_before_resume = link1.frames().len();
    coord.sync(
        DesiredTarget::Active(target.clone()),
        Some(session1.clone()),
        &rt,
    );
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(
        link1.frames().len(),
        frames_before_resume,
        "resuming same session1 with unchanged confirmed styles must deduplicate"
    );

    let (link2, mut out_rx2) = GatedMockLink::new();
    let session2 = build_session(&rt, link2.clone(), &mut out_rx2, Duration::from_secs(2));
    assert_ne!(session1.id, session2.id);
    let responder2 = rt.spawn({
        let link2 = link2.clone();
        async move {
            while let Some(req) = out_rx2.recv().await {
                link2.reply_ok(&req);
            }
        }
    });

    coord.sync(DesiredTarget::Active(target), Some(session2), &rt);
    let deadline2 = Instant::now() + Duration::from_secs(1);
    while link2.frames().len() < 2 {
        assert!(
            Instant::now() < deadline2,
            "new session2 must reset confirmed state and emit fresh kb.sa.rgb frames"
        );
        std::thread::sleep(Duration::from_millis(5));
    }

    responder1.abort();
    responder2.abort();
}

#[test]
fn partial_chunk_failure_commits_earlier_chunks_backs_off_halts_at_3_failures_and_caps_reset_budget(
) {
    let t0 = Instant::now();
    let mut rec = SessionSaRgbReconciler::new();
    let mut twenty_ids = BTreeMap::new();
    for raw in 1..=20 {
        twenty_ids.insert(sa(raw), working_style());
    }

    let ReconcileAction::Dispatch(batch1) =
        rec.reconcile(t0, DesiredTarget::Active(twenty_ids.clone()), 10)
    else {
        panic!("expected 2-chunk dispatch");
    };
    assert_eq!(batch1.chunks.len(), 2);

    let slow_fail_1 = t0 + Duration::from_millis(500);
    let action_1 = rec.complete_batch(slow_fail_1, batch1.batch_id, 1, 12);
    assert_eq!(rec.consecutive_failures(), 1);
    assert!(!rec.halted_on_error());
    for raw in 1..=16 {
        assert_eq!(rec.confirmed().get(&sa(raw)), Some(&working_style()));
    }
    for raw in 17..=20 {
        assert!(!rec.confirmed().contains_key(&sa(raw)));
    }
    assert_eq!(
        action_1,
        ReconcileAction::ScheduleWake(slow_fail_1 + Duration::from_millis(100)),
        "slow RPC failure must compute backoff from completion time, never 0ms"
    );

    let wake_1 = slow_fail_1 + Duration::from_millis(100);
    let ReconcileAction::Dispatch(retry_1) = rec.flush_due(wake_1, 12) else {
        panic!("expected retry 1 after 100ms backoff");
    };
    assert_eq!(retry_1.chunks.len(), 1);
    assert_eq!(retry_1.chunks[0].update.sa, vec![17, 18, 19, 20]);

    let slow_fail_2 = wake_1 + Duration::from_millis(250);
    let action_2 = rec.complete_batch(slow_fail_2, retry_1.batch_id, 0, 13);
    assert_eq!(rec.consecutive_failures(), 2);
    assert_eq!(
        action_2,
        ReconcileAction::ScheduleWake(slow_fail_2 + Duration::from_millis(200))
    );

    let wake_2 = slow_fail_2 + Duration::from_millis(200);
    let ReconcileAction::Dispatch(retry_2) = rec.flush_due(wake_2, 13) else {
        panic!("expected retry 2 after 200ms backoff");
    };
    let slow_fail_3 = wake_2 + Duration::from_millis(250);
    let action_3 = rec.complete_batch(slow_fail_3, retry_2.batch_id, 0, 14);
    assert_eq!(rec.consecutive_failures(), 3);
    assert!(rec.halted_on_error());
    assert_eq!(action_3, ReconcileAction::NoOp);
    assert_eq!(
        rec.reconcile(
            slow_fail_3 + Duration::from_millis(900),
            DesiredTarget::Active(twenty_ids.clone()),
            14
        ),
        ReconcileAction::NoOp,
        "circuit breaker must stay halted when desired has not changed"
    );

    let mut updated_desired = twenty_ids;
    updated_desired.insert(sa(21), attention_style());
    let resume_at = slow_fail_3 + Duration::from_secs(1);
    let ReconcileAction::Dispatch(resumed) =
        rec.reconcile(resume_at, DesiredTarget::Active(updated_desired), 14)
    else {
        panic!("desired change must reset circuit breaker");
    };
    assert!(!rec.halted_on_error());
    assert_eq!(rec.consecutive_failures(), 0);
    assert_eq!(resumed.chunks.len(), 2);
    assert_eq!(resumed.chunks[0].update.sa, vec![17, 18, 19, 20]);
    assert_eq!(resumed.chunks[1].update.sa, vec![21]);
    rec.complete_batch(
        resume_at + Duration::from_millis(5),
        resumed.batch_id,
        resumed.chunks.len(),
        16,
    );

    assert_eq!(
        rec.reconcile(
            resume_at + Duration::from_millis(200),
            DesiredTarget::Active(BTreeMap::from([(sa(99), error_style())])),
            980
        ),
        ReconcileAction::Dispatch(DispatchedSaRgbBatch {
            batch_id: 5,
            chunks: vec![
                SaRgbBatchChunk {
                    update: SaRgbUpdate::reset((1..=16).collect()),
                    kind: SaRgbChunkKind::Reset((1..=16).map(sa).collect()),
                },
                SaRgbBatchChunk {
                    update: SaRgbUpdate::reset(vec![17, 18, 19, 20, 21]),
                    kind: SaRgbChunkKind::Reset((17..=21).map(sa).collect()),
                },
            ],
        }),
        "at request_count=980, Set is suppressed while Reset still dispatches under 995 cap"
    );
    rec.reset_for_new_session();

    let mut rec_cap = SessionSaRgbReconciler::new();
    let mut thirty_two = BTreeMap::new();
    for raw in 1..=20 {
        thirty_two.insert(sa(raw), working_style());
    }
    let ReconcileAction::Dispatch(seed) =
        rec_cap.reconcile(t0, DesiredTarget::Active(thirty_two), 0)
    else {
        panic!("seed");
    };
    rec_cap.complete_batch(
        t0 + Duration::from_millis(5),
        seed.batch_id,
        seed.chunks.len(),
        2,
    );
    let ReconcileAction::Dispatch(capped_reset) = rec_cap.reconcile(
        t0 + Duration::from_millis(200),
        DesiredTarget::UrgentClear(UrgentClearReason::ProtectedLayer),
        994,
    ) else {
        panic!("expected 1 reset chunk at request_count=994");
    };
    assert_eq!(capped_reset.chunks.len(), 1);
    assert_eq!(
        rec_cap.complete_batch(
            t0 + Duration::from_millis(205),
            capped_reset.batch_id,
            1,
            995
        ),
        ReconcileAction::NoOp,
        "at request_count=995, remaining reset chunk must stop before RequestIdExhausted"
    );

    let rt = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap(),
    );
    let (link, mut out_rx) = GatedMockLink::new();
    let session = build_session(&rt, link.clone(), &mut out_rx, Duration::from_secs(1));
    let responder = rt.spawn({
        let link = link.clone();
        async move {
            while let Some(req) = out_rx.recv().await {
                link.reply_ok(&req);
            }
        }
    });
    rt.block_on(async {
        while session.request_count() < 997 {
            session.reset_sa_rgb(vec![1]).await.unwrap();
        }
    });
    assert_eq!(session.request_count(), 997);

    let coord = SaRgbRuntimeCoordinator::new();
    for raw in 1..=20 {
        coord.note_manual_sa_rgb_write(
            session.id,
            ManualSaRgbWrite::Applied {
                sa: sa(raw),
                style: Some(working_style()),
            },
            false,
        );
    }
    link.sa_rgb_frames.lock().unwrap().clear();
    coord.teardown_blocking(&rt, Some(session.clone()));
    assert!(
        !session.is_disconnected(),
        "teardown_blocking must stop at 998 without triggering RequestIdExhausted disconnect"
    );
    assert_eq!(session.request_count(), 998);
    assert_eq!(link.frames().len(), 1);
    assert!(
        coord.has_dirty_keys(),
        "unconfirmed teardown chunk #2 must be restored into uncertain_dirty on live session"
    );
    responder.abort();
}
