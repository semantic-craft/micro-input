use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mi_session::{DeviceSession, ReadOnlySchema, SessionConfig};
use serde_json::{json, Value};

use super::tests::{poll_until, spawn_mock_v070_session, MockV070Link, MockWireEvent, TestFramer};
use super::*;
use crate::canvas::CanvasAction;
use crate::firmware_update::FirmwareAction;
use crate::session_strip::FourAppSessionKind;
use crate::six_layer_runtime::RunningSixLayer;
use crate::store::WorkbenchCommand;

fn make_live_candidate(
    idx: usize,
    phase: FourAppSessionKind,
    updated_at_ms: u64,
) -> crate::session_strip::FourAppLiveCandidate {
    crate::session_strip::FourAppLiveCandidate {
        place: mi_model::navigation_history::NavigationPlace::session(
            "codex",
            format!("sess-{idx}"),
        ),
        title: format!("Session {idx}"),
        phase,
        watermark: format!("wm-{idx}-{updated_at_ms}"),
        last_modified_ms: 10_000 + updated_at_ms,
        last_user_view_ms: 1_000,
        updated_at_ms,
    }
}

fn select_editable_antigravity_profile(store: &mut WorkbenchStore) -> (String, String) {
    let pid = store
        .document_store
        .draft
        .document
        .editable
        .graph
        .profiles
        .iter()
        .find(|(_, p)| p.name == "Antigravity")
        .expect("Antigravity preset profile")
        .0
        .clone();
    store
        .select_and_activate_profile_at(&pid, Instant::now(), false)
        .unwrap();
    let lid = store.selected_layer_id.clone().unwrap();
    (pid, lid)
}

fn bind_sa_tokens_on_layer(store: &mut WorkbenchStore, layer_id: &str, bindings: &[(u32, &str)]) {
    store
        .document_store
        .update(|draft| {
            draft.mutate(|doc| {
                let layer = doc
                    .editable
                    .graph
                    .layers
                    .get_mut(layer_id)
                    .ok_or_else(|| "missing layer".to_string())?;
                for &(slot, token) in bindings {
                    let logical = format!("logical-{slot}");
                    if let Some(ctrl) = layer
                        .controls
                        .values_mut()
                        .find(|c| c.logical_control == logical)
                    {
                        ctrl.original = serde_json::to_value(mi_model::Binding::Basic(
                            mi_model::Token(token.to_string()),
                        ))
                        .unwrap();
                        ctrl.action_id = None;
                    }
                }
                Ok(())
            })
        })
        .unwrap();
}

fn wait_for_sa_rgb_count(link: &MockV070Link, expected_min: usize) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while link.sa_rgb_params().len() < expected_min {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {expected_min} kb.sa.rgb frames; got {:?}",
            link.sa_rgb_params()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn assert_is_clean_reset_frame(params: &Value, expected_sa: &[u64]) {
    assert_eq!(params.get("e").and_then(Value::as_u64), Some(0));
    assert!(
        params.get("c").is_none(),
        "reset frame must omit c: {params}"
    );
    assert!(
        params.get("b").is_none(),
        "reset frame must omit b: {params}"
    );
    assert!(
        params.get("s").is_none(),
        "reset frame must omit s: {params}"
    );
    let actual_sa: Vec<u64> = params["sa"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    assert_eq!(actual_sa, expected_sa);
}

#[test]
fn v070_session_sa_rgb_end_to_end_states_dedup_coalescing_and_rc06_isolation() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = crate::store::WorkbenchStoreConfig::isolated(dir.path());
    cfg.isolated = false;
    let mut store = WorkbenchStore::open(cfg.clone()).unwrap();
    assert!(!store.projection().firmware.v070.session_sa_rgb_sync_enabled);

    let (active_pid, active_lid) = select_editable_antigravity_profile(&mut store);
    bind_sa_tokens_on_layer(
        &mut store,
        &active_lid,
        &[
            (0, "SA_101"),
            (1, "SA_102"),
            (2, "SA_103"),
            (3, "SA_104"),
            (4, "SA_105"),
            (6, "SA_199"),
        ],
    );

    let sent_methods = Arc::new(Mutex::new(Vec::new()));
    let (session, link, responder) =
        spawn_mock_v070_session(&store.tokio_runtime, "0.7.0-rc.16", sent_methods);
    store.device_session = Some(session.clone());
    store.trigger_v070_session_snapshot_if_supported(&session);
    poll_until(&mut store, |s| s.projection().firmware.v070.synced);

    let initial_candidates = vec![
        make_live_candidate(0, FourAppSessionKind::Working, 100),
        make_live_candidate(1, FourAppSessionKind::NeedsAttention, 90),
        make_live_candidate(2, FourAppSessionKind::BlueDotCompleted, 80),
        make_live_candidate(3, FourAppSessionKind::Error, 70),
        make_live_candidate(4, FourAppSessionKind::Idle, 60),
    ];
    store.ingest_four_app_candidates(initial_candidates.clone());
    std::thread::sleep(Duration::from_millis(40));
    assert!(
        link.sa_rgb_params().is_empty(),
        "opt-in disabled by default must emit 0 kb.sa.rgb frames"
    );

    let _ = active_pid;
    store
        .dispatch_command(WorkbenchCommand::Firmware(FirmwareAction::V070Hardware(
            V070HardwareAction::SetSessionSaRgbSync { enabled: true },
        )))
        .unwrap();
    assert!(store.projection().firmware.v070.session_sa_rgb_sync_enabled);
    assert!(store.document_store.v070_session_sa_rgb_sync_enabled());
    wait_for_sa_rgb_count(&link, 4);

    let first_batch = link.sa_rgb_params();
    assert_eq!(first_batch.len(), 4);
    let mut lit_ids = std::collections::BTreeSet::new();
    for frame in &first_batch {
        assert!(frame.get("e").and_then(Value::as_u64).unwrap_or(0) > 0);
        assert!(frame.get("c").and_then(Value::as_u64).is_some());
        for id in frame["sa"].as_array().unwrap() {
            lit_ids.insert(id.as_u64().unwrap());
        }
    }
    assert_eq!(
        lit_ids,
        std::collections::BTreeSet::from([101, 102, 103, 104, 199]),
        "Idle slot SA_105 must stay unlit while SA_199 aggregates highest urgency"
    );
    let attention_frame = first_batch
        .iter()
        .find(|f| {
            f["sa"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v.as_u64() == Some(102))
        })
        .expect("NeedsAttention frame for SA_102");
    let attention_ids: Vec<u64> = attention_frame["sa"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    assert_eq!(
        attention_ids,
        vec![102, 199],
        "non-slot SA_199 must share aggregate NeedsAttention style"
    );

    link.clear_wire_events();
    store.ingest_four_app_candidates(initial_candidates);
    std::thread::sleep(Duration::from_millis(140));
    assert!(
        link.sa_rgb_params().is_empty(),
        "same-color deduplication must emit 0 frames on unchanged session states"
    );

    std::thread::sleep(Duration::from_millis(20));
    store.update_now = Instant::now();
    store.ingest_four_app_candidates(vec![make_live_candidate(0, FourAppSessionKind::Error, 200)]);
    wait_for_sa_rgb_count(&link, 2);
    link.clear_wire_events();

    let burst_start = Instant::now();
    store.update_now = burst_start;
    store.ingest_four_app_candidates(vec![make_live_candidate(
        0,
        FourAppSessionKind::Working,
        210,
    )]);
    store.update_now = burst_start + Duration::from_millis(15);
    store.ingest_four_app_candidates(vec![make_live_candidate(
        0,
        FourAppSessionKind::NeedsAttention,
        220,
    )]);
    let gen_after_ingest = store.projection_generation();
    assert!(!store.projection().firmware.v070.busy);
    std::thread::sleep(Duration::from_millis(30));
    assert!(
        link.sa_rgb_params().is_empty(),
        "sub-100ms burst must not flush before 100ms window elapses"
    );
    wait_for_sa_rgb_count(&link, 1);
    let coalesced = link.sa_rgb_params();
    assert_eq!(
        coalesced.len(),
        1,
        "rapid state burst inside 100ms window must coalesce to 1 trailing frame"
    );
    assert_eq!(
        store.projection_generation(),
        gen_after_ingest,
        "background sa.rgb timer flush must never increment projection_generation (R-C06)"
    );
    assert!(
        !store.projection().firmware.v070.busy,
        "background sa.rgb timer flush must never set v070.busy"
    );

    link.clear_wire_events();
    let roundtrip_start = Instant::now();
    store.update_now = roundtrip_start;
    store.ingest_four_app_candidates(vec![make_live_candidate(
        0,
        FourAppSessionKind::Working,
        230,
    )]);
    store.update_now = roundtrip_start + Duration::from_millis(10);
    store.ingest_four_app_candidates(vec![make_live_candidate(
        0,
        FourAppSessionKind::NeedsAttention,
        240,
    )]);
    std::thread::sleep(Duration::from_millis(150));
    assert!(
        link.sa_rgb_params().is_empty(),
        "roundtrip back to confirmed style during cooldown must emit 0 frames"
    );

    std::thread::sleep(Duration::from_millis(20));
    store.update_now = Instant::now();
    store.ingest_four_app_candidates(vec![make_live_candidate(0, FourAppSessionKind::Idle, 300)]);
    wait_for_sa_rgb_count(&link, 1);
    let idle_reset = link.sa_rgb_params();
    assert_eq!(idle_reset.len(), 1);
    assert_is_clean_reset_frame(&idle_reset[0], &[101, 199]);

    link.clear_wire_events();
    store.system_will_terminate();
    responder.abort();
    drop(store);

    let reopened = WorkbenchStore::open(cfg).unwrap();
    assert!(
        reopened
            .projection()
            .firmware
            .v070
            .session_sa_rgb_sync_enabled
    );
}

#[test]
fn v070_session_sa_rgb_resets_before_close_on_opt_out_protected_layer_sleep_terminate_and_unplug() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = crate::store::WorkbenchStoreConfig::isolated(dir.path());
    cfg.isolated = false;
    let mut store = WorkbenchStore::open(cfg).unwrap();

    let (active_pid, custom_lid) = select_editable_antigravity_profile(&mut store);
    bind_sa_tokens_on_layer(&mut store, &custom_lid, &[(0, "SA_201"), (1, "SA_202")]);

    let mut protected_lid = String::new();
    store
        .document_store
        .update(|draft| {
            protected_lid = draft.create_layer(&active_pid, "OAI Protected")?;
            draft.mutate(|doc| {
                let layer = doc.editable.graph.layers.get_mut(&protected_lid).unwrap();
                if let Some(ctrl) = layer.controls.values_mut().next() {
                    ctrl.original = serde_json::to_value(mi_model::Binding::Basic(
                        mi_model::Token("KV_OAI_PTT".to_string()),
                    ))
                    .unwrap();
                    ctrl.action_id = None;
                }
                Ok(())
            })
        })
        .unwrap();

    let sent_methods = Arc::new(Mutex::new(Vec::new()));
    let (session, link, responder) =
        spawn_mock_v070_session(&store.tokio_runtime, "0.7.0-rc.16", sent_methods);
    store.device_session = Some(session.clone());
    store.trigger_v070_session_snapshot_if_supported(&session);
    poll_until(&mut store, |s| s.projection().firmware.v070.synced);

    store.ingest_four_app_candidates(vec![
        make_live_candidate(0, FourAppSessionKind::Working, 100),
        make_live_candidate(1, FourAppSessionKind::Error, 90),
    ]);
    store
        .dispatch_command(WorkbenchCommand::Firmware(FirmwareAction::V070Hardware(
            V070HardwareAction::SetSessionSaRgbSync { enabled: true },
        )))
        .unwrap();
    wait_for_sa_rgb_count(&link, 2);

    link.clear_wire_events();
    store
        .dispatch_command(WorkbenchCommand::Firmware(FirmwareAction::V070Hardware(
            V070HardwareAction::SetSessionSaRgbSync { enabled: false },
        )))
        .unwrap();
    wait_for_sa_rgb_count(&link, 1);
    let opt_out_frames = link.sa_rgb_params();
    assert_eq!(opt_out_frames.len(), 1);
    assert_is_clean_reset_frame(&opt_out_frames[0], &[201, 202]);

    link.clear_wire_events();
    store
        .dispatch_command(WorkbenchCommand::Firmware(FirmwareAction::V070Hardware(
            V070HardwareAction::SetSessionSaRgbSync { enabled: true },
        )))
        .unwrap();
    wait_for_sa_rgb_count(&link, 2);

    link.clear_wire_events();
    store
        .dispatch_command(WorkbenchCommand::Canvas(CanvasAction::SelectLayer(
            protected_lid,
        )))
        .unwrap();
    wait_for_sa_rgb_count(&link, 1);
    let protected_reset = link.sa_rgb_params();
    assert_eq!(protected_reset.len(), 1);
    assert_is_clean_reset_frame(&protected_reset[0], &[201, 202]);

    link.clear_wire_events();
    store.ingest_four_app_candidates(vec![make_live_candidate(
        0,
        FourAppSessionKind::NeedsAttention,
        200,
    )]);
    std::thread::sleep(Duration::from_millis(140));
    assert!(
        link.sa_rgb_params().is_empty(),
        "protected Mg1/KV_OAI layer must emit 0 Override frames"
    );

    store
        .dispatch_command(WorkbenchCommand::Canvas(CanvasAction::SelectLayer(
            custom_lid,
        )))
        .unwrap();
    wait_for_sa_rgb_count(&link, 1);

    link.clear_wire_events();
    store.handle_v070_profile_layer_changed(session.id, 3, 2);
    wait_for_sa_rgb_count(&link, 1);
    assert_is_clean_reset_frame(&link.sa_rgb_params()[0], &[201]);

    link.clear_wire_events();
    store.handle_v070_profile_layer_changed(session.id, 3, 1);
    wait_for_sa_rgb_count(&link, 1);
    link.clear_wire_events();

    store.system_will_sleep();
    let sleep_events = link.wire_events.lock().unwrap().clone();
    let reset_pos = sleep_events
        .iter()
        .position(|ev| matches!(ev, MockWireEvent::SaRgb(_)))
        .expect("system_will_sleep must emit e=0 reset");
    let close_pos = sleep_events
        .iter()
        .position(|ev| matches!(ev, MockWireEvent::LinkClosed))
        .expect("system_will_sleep must close link");
    assert!(
        reset_pos < close_pos,
        "e=0 reset must occur BEFORE link close on system_will_sleep: {sleep_events:?}"
    );
    if let MockWireEvent::SaRgb(ref params) = sleep_events[reset_pos] {
        assert_is_clean_reset_frame(params, &[201]);
    }
    responder.abort();

    let dir_term = tempfile::tempdir().unwrap();
    let mut cfg_term = crate::store::WorkbenchStoreConfig::isolated(dir_term.path());
    cfg_term.isolated = false;
    let mut store_term = WorkbenchStore::open(cfg_term).unwrap();
    let (_, term_lid) = select_editable_antigravity_profile(&mut store_term);
    bind_sa_tokens_on_layer(&mut store_term, &term_lid, &[(0, "SA_301")]);
    let (session_term, link_term, responder_term) = spawn_mock_v070_session(
        &store_term.tokio_runtime,
        "0.7.0-rc.16",
        Arc::new(Mutex::new(Vec::new())),
    );
    store_term.device_session = Some(session_term.clone());
    store_term.trigger_v070_session_snapshot_if_supported(&session_term);
    poll_until(&mut store_term, |s| s.projection().firmware.v070.synced);
    store_term.ingest_four_app_candidates(vec![make_live_candidate(
        0,
        FourAppSessionKind::Working,
        100,
    )]);
    store_term
        .dispatch_command(WorkbenchCommand::Firmware(FirmwareAction::V070Hardware(
            V070HardwareAction::SetSessionSaRgbSync { enabled: true },
        )))
        .unwrap();
    wait_for_sa_rgb_count(&link_term, 1);
    link_term.clear_wire_events();

    let raw_cmd: WorkbenchCommand =
        serde_json::from_value(json!({"domain": "system_will_terminate"})).unwrap();
    store_term.dispatch_command(raw_cmd).unwrap();
    let term_events = link_term.wire_events.lock().unwrap().clone();
    let term_reset_pos = term_events
        .iter()
        .position(|ev| matches!(ev, MockWireEvent::SaRgb(_)))
        .expect("SystemWillTerminate must emit e=0 reset");
    let term_close_pos = term_events
        .iter()
        .position(|ev| matches!(ev, MockWireEvent::LinkClosed))
        .expect("SystemWillTerminate must close link");
    assert!(
        term_reset_pos < term_close_pos,
        "e=0 reset must occur BEFORE link close on SystemWillTerminate: {term_events:?}"
    );
    if let MockWireEvent::SaRgb(ref params) = term_events[term_reset_pos] {
        assert_is_clean_reset_frame(params, &[301]);
    }
    responder_term.abort();

    let dir_unplug = tempfile::tempdir().unwrap();
    let mut cfg_unplug = crate::store::WorkbenchStoreConfig::isolated(dir_unplug.path());
    cfg_unplug.isolated = false;
    let mut store_unplug = WorkbenchStore::open(cfg_unplug).unwrap();
    let (_, unplug_lid) = select_editable_antigravity_profile(&mut store_unplug);
    bind_sa_tokens_on_layer(&mut store_unplug, &unplug_lid, &[(0, "SA_401")]);
    let (session_unplug, link_unplug, responder_unplug) = spawn_mock_v070_session(
        &store_unplug.tokio_runtime,
        "0.7.0-rc.16",
        Arc::new(Mutex::new(Vec::new())),
    );
    store_unplug.device_session = Some(session_unplug.clone());
    store_unplug.trigger_v070_session_snapshot_if_supported(&session_unplug);
    poll_until(&mut store_unplug, |s| s.projection().firmware.v070.synced);
    store_unplug.ingest_four_app_candidates(vec![make_live_candidate(
        0,
        FourAppSessionKind::Working,
        100,
    )]);
    store_unplug
        .dispatch_command(WorkbenchCommand::Firmware(FirmwareAction::V070Hardware(
            V070HardwareAction::SetSessionSaRgbSync { enabled: true },
        )))
        .unwrap();
    wait_for_sa_rgb_count(&link_unplug, 1);
    link_unplug.clear_wire_events();

    store_unplug.handle_hardware_loop_event(
        HardwareLoopEvent::Disconnected {
            session_id: session_unplug.id,
        },
        Instant::now(),
    );
    assert!(
        link_unplug.sa_rgb_params().is_empty(),
        "physical unplug (Disconnected) must emit 0 wire frames"
    );
    assert!(!store_unplug.sa_rgb_coordinator.has_dirty_keys());
    responder_unplug.abort();
}

#[test]
fn v070_session_sa_rgb_drop_inside_tokio_runtime_resets_before_close_without_panic_and_gates_older_firmware(
) {
    for old_fw in ["0.6.3", "0.7.0-rc.15"] {
        let dir_old = tempfile::tempdir().unwrap();
        let mut cfg_old = crate::store::WorkbenchStoreConfig::isolated(dir_old.path());
        cfg_old.isolated = false;
        let mut store_old = WorkbenchStore::open(cfg_old).unwrap();
        let (_, old_lid) = select_editable_antigravity_profile(&mut store_old);
        bind_sa_tokens_on_layer(&mut store_old, &old_lid, &[(0, "SA_501")]);
        let (session_old, link_old, responder_old) = spawn_mock_v070_session(
            &store_old.tokio_runtime,
            old_fw,
            Arc::new(Mutex::new(Vec::new())),
        );
        store_old.device_session = Some(session_old.clone());
        store_old.trigger_v070_session_snapshot_if_supported(&session_old);
        store_old
            .dispatch_command(WorkbenchCommand::Firmware(FirmwareAction::V070Hardware(
                V070HardwareAction::SetSessionSaRgbSync { enabled: true },
            )))
            .unwrap();
        store_old.ingest_four_app_candidates(vec![make_live_candidate(
            0,
            FourAppSessionKind::Working,
            100,
        )]);
        std::thread::sleep(Duration::from_millis(60));
        assert!(
            link_old.sa_rgb_params().is_empty(),
            "firmware {old_fw} (< 0.7.0-rc.16) must never emit kb.sa.rgb"
        );
        drop(store_old);
        responder_old.abort();
    }

    let dir = tempfile::tempdir().unwrap();
    let mut cfg = crate::store::WorkbenchStoreConfig::isolated(dir.path());
    cfg.isolated = false;
    let mut store = WorkbenchStore::open(cfg).unwrap();
    let (_, lid) = select_editable_antigravity_profile(&mut store);
    bind_sa_tokens_on_layer(&mut store, &lid, &[(0, "SA_601"), (1, "SA_602")]);
    let (session, link, responder) = spawn_mock_v070_session(
        &store.tokio_runtime,
        "0.7.0-rc.16",
        Arc::new(Mutex::new(Vec::new())),
    );
    store.device_session = Some(session.clone());
    store.trigger_v070_session_snapshot_if_supported(&session);
    poll_until(&mut store, |s| s.projection().firmware.v070.synced);
    store.ingest_four_app_candidates(vec![
        make_live_candidate(0, FourAppSessionKind::Working, 100),
        make_live_candidate(1, FourAppSessionKind::BlueDotCompleted, 90),
    ]);
    store
        .dispatch_command(WorkbenchCommand::Firmware(FirmwareAction::V070Hardware(
            V070HardwareAction::SetSessionSaRgbSync { enabled: true },
        )))
        .unwrap();
    wait_for_sa_rgb_count(&link, 2);
    link.clear_wire_events();

    let outer_rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    outer_rt.block_on(async {
        assert!(tokio::runtime::Handle::try_current().is_ok());
        store.system_will_terminate();
    });
    let rt_events = link.wire_events.lock().unwrap().clone();
    let rt_reset_pos = rt_events
        .iter()
        .position(|ev| matches!(ev, MockWireEvent::SaRgb(_)))
        .expect("teardown inside active Tokio runtime must emit e=0 reset");
    let rt_close_pos = rt_events
        .iter()
        .position(|ev| matches!(ev, MockWireEvent::LinkClosed))
        .expect("system_will_terminate must close link");
    assert!(
        rt_reset_pos < rt_close_pos,
        "e=0 reset must occur BEFORE link close inside Tokio runtime: {rt_events:?}"
    );
    if let MockWireEvent::SaRgb(ref params) = rt_events[rt_reset_pos] {
        assert_is_clean_reset_frame(params, &[601, 602]);
    }
    drop(store);
    responder.abort();

    let dir_drop = tempfile::tempdir().unwrap();
    let mut cfg_drop = crate::store::WorkbenchStoreConfig::isolated(dir_drop.path());
    cfg_drop.isolated = false;
    let mut store_drop = WorkbenchStore::open(cfg_drop).unwrap();
    let drop_lid = select_editable_antigravity_profile(&mut store_drop).1;
    bind_sa_tokens_on_layer(&mut store_drop, &drop_lid, &[(0, "SA_603"), (1, "SA_604")]);
    let (session_drop, link_drop, responder_drop) = spawn_mock_v070_session(
        &store_drop.tokio_runtime,
        "0.7.0-rc.16",
        Arc::new(Mutex::new(Vec::new())),
    );
    store_drop.device_session = Some(session_drop.clone());
    store_drop.trigger_v070_session_snapshot_if_supported(&session_drop);
    poll_until(&mut store_drop, |s| s.projection().firmware.v070.synced);
    store_drop.ingest_four_app_candidates(vec![
        make_live_candidate(0, FourAppSessionKind::Working, 100),
        make_live_candidate(1, FourAppSessionKind::Error, 90),
    ]);
    store_drop
        .dispatch_command(WorkbenchCommand::Firmware(FirmwareAction::V070Hardware(
            V070HardwareAction::SetSessionSaRgbSync { enabled: true },
        )))
        .unwrap();
    wait_for_sa_rgb_count(&link_drop, 2);
    link_drop.clear_wire_events();

    drop(store_drop);

    let drop_events = link_drop.wire_events.lock().unwrap().clone();
    let reset_pos = drop_events
        .iter()
        .position(|ev| matches!(ev, MockWireEvent::SaRgb(_)))
        .expect("Drop of WorkbenchStore must emit e=0 reset");
    let close_pos = drop_events
        .iter()
        .position(|ev| matches!(ev, MockWireEvent::LinkClosed))
        .expect("Drop must close link");
    assert!(
        reset_pos < close_pos,
        "e=0 reset must occur BEFORE link close on Drop: {drop_events:?}"
    );
    if let MockWireEvent::SaRgb(ref params) = drop_events[reset_pos] {
        assert_is_clean_reset_frame(params, &[603, 604]);
    }
    responder_drop.abort();
}

#[test]
fn reassigned_slot_ignores_stale_original_sa_and_hardware_layer_2_does_not_alias_host_layer_1() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = crate::store::WorkbenchStoreConfig::isolated(dir.path());
    cfg.isolated = false;
    let mut store = WorkbenchStore::open(cfg).unwrap();

    let (active_pid, host_layer_1) = select_editable_antigravity_profile(&mut store);
    assert_eq!(
        store.document_store.draft.document.editable.graph.profiles[&active_pid]
            .layer_order
            .len(),
        1
    );
    bind_sa_tokens_on_layer(&mut store, &host_layer_1, &[(0, "SA_701"), (1, "SA_702")]);

    let (session, link, responder) = spawn_mock_v070_session(
        &store.tokio_runtime,
        "0.7.0-rc.16",
        Arc::new(Mutex::new(Vec::new())),
    );
    store.device_session = Some(session.clone());
    store.trigger_v070_session_snapshot_if_supported(&session);
    poll_until(&mut store, |s| s.projection().firmware.v070.synced);

    store.ingest_four_app_candidates(vec![
        make_live_candidate(0, FourAppSessionKind::Working, 100),
        make_live_candidate(1, FourAppSessionKind::Error, 90),
    ]);
    store
        .dispatch_command(WorkbenchCommand::Firmware(FirmwareAction::V070Hardware(
            V070HardwareAction::SetSessionSaRgbSync { enabled: true },
        )))
        .unwrap();
    wait_for_sa_rgb_count(&link, 2);

    std::thread::sleep(Duration::from_millis(110));
    link.clear_wire_events();
    store
        .dispatch_command(WorkbenchCommand::Canvas(CanvasAction::AssignSlotAction {
            slot: 0,
            action_id: "slot-open-0".into(),
        }))
        .unwrap();
    let slot0_node = store.document_store.draft.document.editable.graph.layers[&host_layer_1]
        .controls
        .values()
        .find(|c| c.logical_control == "logical-0")
        .unwrap();
    assert!(slot0_node.action_id.is_some());
    assert_eq!(slot0_node.original, json!({"Basic": "SA_701"}));

    wait_for_sa_rgb_count(&link, 1);
    let reassign_frames = link.sa_rgb_params();
    assert_eq!(reassign_frames.len(), 1);
    assert_is_clean_reset_frame(&reassign_frames[0], &[701]);

    std::thread::sleep(Duration::from_millis(110));
    link.clear_wire_events();
    store.handle_v070_profile_layer_changed(session.id, 3, 2);
    wait_for_sa_rgb_count(&link, 1);
    let hw_layer_2_frames = link.sa_rgb_params();
    assert_eq!(hw_layer_2_frames.len(), 1);
    assert_is_clean_reset_frame(&hw_layer_2_frames[0], &[702]);

    link.clear_wire_events();
    store.ingest_four_app_candidates(vec![make_live_candidate(
        1,
        FourAppSessionKind::NeedsAttention,
        200,
    )]);
    std::thread::sleep(Duration::from_millis(130));
    assert!(
        link.sa_rgb_params().is_empty(),
        "hardware layer 2 missing in editable.graph must never alias Host Layer 1"
    );

    store.v070_hardware.projection.sa_rgb.sa = Some(999);
    let mut running = RunningSixLayer::isolated(session.clone(), dir.path());
    running.readiness.configuration_observed = true;
    running.readiness.keyboard_grabbed = true;
    running.set_v070_inbox(store.hardware_event_tx.clone());
    running.attach_test_vendor_session(&store.tokio_runtime, session.clone());
    store.six_layer_running = Some(running);
    store.handle_v070_profile_layer_changed(session.id, 3, 2);
    std::thread::sleep(Duration::from_millis(130));
    assert!(
        link.sa_rgb_params().is_empty(),
        "Six-Layer HostProfile layer without SA_* bindings must ignore UI spinner fallback_sa"
    );

    if let Some(running) = store.six_layer_running.take() {
        running.shutdown(&store.tokio_runtime).unwrap();
    }
    responder.abort();
}

#[test]
fn manual_sa_rgb_apply_is_not_immediately_clobbered_and_opt_out_uses_nonblocking_urgent_clear() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = crate::store::WorkbenchStoreConfig::isolated(dir.path());
    cfg.isolated = false;
    let mut store = WorkbenchStore::open(cfg).unwrap();

    let (_, lid) = select_editable_antigravity_profile(&mut store);
    bind_sa_tokens_on_layer(&mut store, &lid, &[(0, "SA_801")]);

    let (session, link, responder) = spawn_mock_v070_session(
        &store.tokio_runtime,
        "0.7.0-rc.16",
        Arc::new(Mutex::new(Vec::new())),
    );
    store.device_session = Some(session.clone());
    store.trigger_v070_session_snapshot_if_supported(&session);
    poll_until(&mut store, |s| s.projection().firmware.v070.synced);

    store.ingest_four_app_candidates(vec![make_live_candidate(
        0,
        FourAppSessionKind::Working,
        100,
    )]);
    store
        .dispatch_command(WorkbenchCommand::Firmware(FirmwareAction::V070Hardware(
            V070HardwareAction::SetSessionSaRgbSync { enabled: true },
        )))
        .unwrap();
    wait_for_sa_rgb_count(&link, 1);
    assert_eq!(link.sa_rgb_params()[0]["c"].as_u64(), Some(0x00FF66));

    std::thread::sleep(Duration::from_millis(110));
    link.clear_wire_events();
    store
        .dispatch_command(WorkbenchCommand::Firmware(FirmwareAction::V070Hardware(
            V070HardwareAction::ApplySaRgb {
                sa: 801,
                color_hex: "#33AAFF".into(),
                brightness: 0.75,
                speed: 0.5,
                effect: 4,
            },
        )))
        .unwrap();
    poll_until(&mut store, |s| !s.projection().firmware.v070.busy);
    std::thread::sleep(Duration::from_millis(140));
    let manual_frames = link.sa_rgb_params();
    assert_eq!(
        manual_frames.len(),
        1,
        "manual ApplySaRgb outcome must not be immediately clobbered by auto sync"
    );
    assert_eq!(manual_frames[0]["c"].as_u64(), Some(0x33AAFF));
    assert_eq!(manual_frames[0]["e"].as_u64(), Some(4));

    responder.abort();
    let _ = store.tokio_runtime.block_on(responder);

    let (slow_link, mut slow_out_rx) = MockV070Link::new();
    let slow_session = store.tokio_runtime.block_on(async {
        DeviceSession::new(
            slow_link.clone(),
            Box::new(TestFramer),
            Arc::new(ReadOnlySchema),
            SessionConfig {
                timeout: Duration::from_millis(500),
                interval: Duration::ZERO,
            },
        )
    });
    let slow_responder = store.tokio_runtime.spawn({
        let slow_link = slow_link.clone();
        async move {
            while let Some(raw) = slow_out_rx.recv().await {
                let req: Value = serde_json::from_slice(&raw).unwrap();
                let id = req["id"].clone();
                let method = req["method"].as_str().unwrap().to_string();
                let result = if method == "device.status" {
                    json!({
                        "version": "0.7.0-rc.16",
                        "profile_index": 3,
                        "layer_index": 1,
                        "battery": 90,
                        "is_charging": false
                    })
                } else {
                    tokio::time::sleep(Duration::from_millis(120)).await;
                    json!({})
                };
                slow_link.push_wire(json!({
                    "id": id,
                    "method": method,
                    "result": result
                }));
            }
        }
    });
    store.tokio_runtime.block_on(async {
        slow_session.read_identity_info().await.unwrap();
    });
    store.device_session = Some(slow_session);
    store.sync_v070_session_sa_rgb();
    wait_for_sa_rgb_count(&slow_link, 1);
    while store.sa_rgb_coordinator.is_in_flight() {
        std::thread::sleep(Duration::from_millis(5));
    }
    slow_link.clear_wire_events();

    let toggle_start = Instant::now();
    store
        .dispatch_command(WorkbenchCommand::Firmware(FirmwareAction::V070Hardware(
            V070HardwareAction::SetSessionSaRgbSync { enabled: false },
        )))
        .unwrap();
    let ui_elapsed = toggle_start.elapsed();
    assert!(
        ui_elapsed < Duration::from_millis(50),
        "SetSessionSaRgbSync(false) must dispatch non-blocking UrgentClear instead of blocking 120ms: took {ui_elapsed:?}"
    );
    wait_for_sa_rgb_count(&slow_link, 1);
    let clear_frames = slow_link.sa_rgb_params();
    assert_eq!(clear_frames.len(), 1);
    assert_is_clean_reset_frame(&clear_frames[0], &[801]);
    while store.sa_rgb_coordinator.is_in_flight() {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!store.sa_rgb_coordinator.has_dirty_keys());
    slow_responder.abort();
}
