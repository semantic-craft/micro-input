#![cfg(feature = "synthetic-backend")]
use mi_model::{
    document::LocalDocumentV2,
    lighting::{Edit, Zone},
    storage::DocumentStore,
};
use mi_session::{
    config_write::{ConfigWriter, Outcome},
    DeviceSession, ReadOnlySchema, SessionConfig,
};
use mi_transport::{
    HidLink, HidTransport, LinkBackend, SyntheticDiscovery, SyntheticFiles, SyntheticWriteResponse,
    TransportError,
};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::{sync::Arc, time::Duration};
const RAW: &[u8] = include_bytes!("../../mi-model/fixtures/device/synthetic-keymap-lighting.json");
fn baseline(raw: &[u8], id: u64) -> Value {
    json!({"device_identity":{"confidence":"verified-serial","serial":"synthetic","session_id":id},"firmware":"synthetic-0.6.3","keymap":{"state":"present","size":raw.len(),"sha1":format!("{:x}",Sha1::digest(raw)),"bytes":raw},"smart_actions":{"state":"missing"}})
}
fn plan(
    raw: &[u8],
    id: u64,
    color: u32,
) -> (tempfile::TempDir, mi_model::apply::PreparedCandidate) {
    let config = mi_model::parse_device_files(raw, None).unwrap();
    let base = baseline(raw, id);
    let doc = LocalDocumentV2::from_config(&config, Some(base.clone()), "device-snapshot").unwrap();
    let p = doc.editable.graph.profile_order[0].clone();
    let l = doc.editable.graph.profiles[&p].layer_order[0].clone();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("draft.json");
    std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
    let mut store = DocumentStore::default();
    store.open(path).unwrap();
    store
        .update(|d| d.edit_lighting(&p, &l, Zone::Backlight, Edit::Color(color), false))
        .unwrap();
    let preview = store.preview_apply(&base).unwrap();
    store.prepare_apply_backup(&preview.token, &base).unwrap();
    let candidate = store
        .take_prepared_candidate(&preview.token, &base)
        .unwrap();
    (dir, candidate)
}
async fn setup(files: SyntheticFiles) -> (Arc<SyntheticDiscovery>, Arc<DeviceSession>) {
    let transport = Arc::new(SyntheticDiscovery::default());
    transport.set_synthetic_discovery(1).unwrap();
    transport.set_files(files);
    let session = DeviceSession::new(
        transport.open("synthetic-interface-0").unwrap(),
        Box::<mi_proto::NativeFramer>::default(),
        Arc::new(ReadOnlySchema),
        SessionConfig {
            timeout: Duration::from_millis(60),
            interval: Duration::ZERO,
        },
    );
    session.read_identity_info().await.unwrap();
    session.read_config_snapshot().await.unwrap();
    (transport, session)
}
#[tokio::test]
async fn compiled_lighting_goes_through_real_codec_reader_and_explicit_restore() {
    let (device, session) = setup(SyntheticFiles::new(RAW.to_vec(), None)).await;
    let writer = ConfigWriter::new(session.clone()).unwrap();
    let (_dir, plan) = plan(RAW, session.id, 0xabcdef);
    assert_eq!(writer.apply(&plan).await, Outcome::Verified);
    assert_eq!(device.files().unwrap().keymap, plan.candidate());
    let writes = device
        .synthetic_requests()
        .iter()
        .filter(|m| *m == "fs.writebin")
        .count();
    assert_eq!(writer.apply(&plan).await, Outcome::NotSent);
    assert_eq!(
        device
            .synthetic_requests()
            .iter()
            .filter(|m| *m == "fs.writebin")
            .count(),
        writes
    );
    let (_dir2, restore) = self::plan(plan.candidate(), session.id, 1193046);
    assert_eq!(writer.apply(&restore).await, Outcome::Verified);
    assert_eq!(device.files().unwrap().keymap, RAW);
    session.shutdown().await;
}

#[tokio::test]
async fn candidate_result_payloads_still_require_independent_readback() {
    for result in [Value::Null, json!(true), json!(false), json!({"opaque":42})] {
        let mut files = SyntheticFiles::new(RAW.to_vec(), None);
        files.write_response = SyntheticWriteResponse::Result(result.clone());
        let (device, session) = setup(files).await;
        let (_dir, plan) = plan(RAW, session.id, 0xabcdef);
        let writer = ConfigWriter::new(session.clone()).unwrap();
        let mut requests_after_ack = 0;
        assert_eq!(
            writer
                .apply_observed(&plan, |at, _| {
                    if let Checkpoint::AfterAck(_) = at {
                        requests_after_ack = device.synthetic_requests().len();
                    }
                })
                .await,
            Outcome::Verified,
            "{result}"
        );
        let requests = device.synthetic_requests();
        assert!(requests[requests_after_ack..]
            .iter()
            .any(|m| m == "fs.readbin"));
        assert_eq!(device.files().unwrap().keymap, plan.candidate());
        session.shutdown().await;
    }
}

#[tokio::test]
async fn successful_response_without_expected_bytes_cannot_verify_or_save_locally() {
    for ignore_writes in [true, false] {
        let mut files = SyntheticFiles::new(RAW.to_vec(), None);
        files.ignore_writes = ignore_writes;
        files.corrupt_completed = !ignore_writes;
        files.write_response = SyntheticWriteResponse::Result(json!(true));
        let (device, session) = setup(files).await;
        let (dir, plan) = plan(RAW, session.id, 0xabcdef);
        let local_before = std::fs::read(dir.path().join("draft.json")).unwrap();
        let writer = ConfigWriter::new(session.clone()).unwrap();
        let mut acknowledged = false;
        assert_eq!(
            writer
                .apply_observed(&plan, |at, _| {
                    if let Checkpoint::AfterAck(_) = at {
                        acknowledged = true;
                    }
                })
                .await,
            Outcome::DeviceStateUnknown
        );
        assert!(acknowledged);
        assert_ne!(device.files().unwrap().keymap, plan.candidate());
        if ignore_writes {
            assert_eq!(device.files().unwrap().keymap, RAW);
        }
        let record = OperationRecord::open(plan.record_path()).unwrap();
        assert!(!record.status().device_verified);
        assert!(!record.status().local_saved);
        assert_eq!(
            record.status().error_category,
            Some(mi_model::operation_record::Failure::Readback)
        );
        assert_eq!(
            std::fs::read(dir.path().join("draft.json")).unwrap(),
            local_before
        );
        let count = mutations(&device);
        assert_eq!(writer.apply(&plan).await, Outcome::NotSent);
        assert_eq!(mutations(&device), count);
        session.shutdown().await;
    }
}

#[tokio::test]
async fn malformed_error_and_wrong_id_responses_leave_completed_writes_unknown() {
    for response in [
        SyntheticWriteResponse::MissingResult,
        SyntheticWriteResponse::ResultAndError,
        SyntheticWriteResponse::WrongId,
        SyntheticWriteResponse::Error,
    ] {
        let mut files = SyntheticFiles::new(RAW.to_vec(), None);
        files.write_response = response.clone();
        let (device, session) = setup(files).await;
        let (dir, plan) = plan(RAW, session.id, 0xabcdef);
        let local_before = std::fs::read(dir.path().join("draft.json")).unwrap();
        let writer = ConfigWriter::new(session.clone()).unwrap();
        let mut acknowledged = false;
        assert_eq!(
            writer
                .apply_observed(&plan, |at, _| {
                    if let Checkpoint::AfterAck(_) = at {
                        acknowledged = true;
                    }
                })
                .await,
            Outcome::DeviceStateUnknown,
            "{response:?}"
        );
        assert!(!acknowledged);
        assert_eq!(device.files().unwrap().keymap, plan.candidate());
        let record = OperationRecord::open(plan.record_path()).unwrap();
        assert!(!record.status().device_verified);
        assert!(!record.status().local_saved);
        assert_eq!(
            std::fs::read(dir.path().join("draft.json")).unwrap(),
            local_before
        );
        let count = mutations(&device);
        assert_eq!(writer.apply(&plan).await, Outcome::NotSent);
        assert_eq!(mutations(&device), count);
        assert!(session
            .request(mi_proto::Method::SysVersion, Value::Null)
            .await
            .is_err());
        session.shutdown().await;
    }
}
#[tokio::test]
async fn multichunk_partial_write_lost_ack_and_corrupt_readback_never_report_success() {
    let mut raw = RAW.to_vec();
    raw.extend(vec![b' '; 7000]);
    for fault in ["reject-middle", "lost-final", "corrupt"] {
        let mut files = SyntheticFiles::new(raw.clone(), None);
        if fault == "reject-middle" {
            files.reject_offset = Some(3072)
        }
        let (device, session) = setup(files).await;
        let (_dir, plan) = plan(&raw, session.id, 0xabcdef);
        let mut files = device.files().unwrap();
        if fault == "lost-final" {
            files.lose_ack_offset = Some((plan.candidate().len() - 1) / 3072 * 3072)
        }
        if fault == "corrupt" {
            files.corrupt_completed = true
        }
        device.set_files(files);
        let writer = ConfigWriter::new(session.clone()).unwrap();
        assert_eq!(
            writer.apply(&plan).await,
            Outcome::DeviceStateUnknown,
            "{fault}"
        );
        assert_eq!(writer.apply(&plan).await, Outcome::NotSent);
        assert_ne!(device.files().unwrap().keymap, raw);
        session.shutdown().await;
    }
}
#[tokio::test]
async fn changed_baseline_and_wrong_session_send_no_mutations() {
    let (device, session) = setup(SyntheticFiles::new(RAW.to_vec(), None)).await;
    let writer = ConfigWriter::new(session.clone()).unwrap();
    let (_dir, wrong) = plan(RAW, session.id + 1, 5);
    assert_eq!(writer.apply(&wrong).await, Outcome::NotSent);
    let (_dir2, valid) = plan(RAW, session.id, 5);
    let mut changed = device.files().unwrap();
    changed.keymap.push(b' ');
    device.set_files(changed);
    assert_eq!(writer.apply(&valid).await, Outcome::NotSent);
    assert!(!device
        .synthetic_requests()
        .iter()
        .any(|m| m == "fs.writebin"));
    session.shutdown().await;
}
struct NativeTrap;
#[async_trait::async_trait]
impl HidLink for NativeTrap {
    fn backend(&self) -> LinkBackend {
        LinkBackend::MacHidapi267Numbered
    }
    async fn write(&self, _: &[u8]) -> Result<(), TransportError> {
        panic!("native write reached")
    }
    async fn read(&self) -> Result<Vec<u8>, TransportError> {
        std::future::pending().await
    }
    fn close(&self) {}
}
#[tokio::test]
async fn synthetic_writer_rejects_native_backend_even_when_feature_is_built() {
    let session = DeviceSession::new(
        Arc::new(NativeTrap),
        Box::<mi_proto::NativeFramer>::default(),
        Arc::new(ReadOnlySchema),
        SessionConfig::default(),
    );
    assert!(ConfigWriter::new(session.clone()).is_err());
    session.disconnect();
}
#[test]
fn chunk_boundaries_and_tail_flags_are_exact() {
    for size in [1, 3072, 3073, 10000] {
        let bytes = vec![b'x'; size];
        let mut decoded = vec![];
        for offset in (0..size).step_by(3072) {
            let request: Value = serde_json::from_slice(
                &mi_proto::mutation::keymap_chunk(&bytes, offset, 7).unwrap(),
            )
            .unwrap();
            assert_eq!(request["params"]["completed"], offset + 3072 >= size);
            use base64::Engine;
            decoded.extend(
                base64::engine::general_purpose::STANDARD
                    .decode(request["params"]["data"].as_str().unwrap())
                    .unwrap(),
            );
        }
        assert_eq!(decoded, bytes);
    }
    assert!(mi_proto::mutation::keymap_chunk(b"abc", 1, 0).is_err());
    assert!(mi_proto::mutation::keymap_chunk(b"", 0, 0).is_err());
    assert!(mi_proto::mutation::keymap_chunk(b"a", 0, 999).is_err());
}

#[tokio::test]
async fn exhausted_budget_is_rejected_before_the_first_mutation() {
    let (device, session) = setup(SyntheticFiles::new(RAW.to_vec(), None)).await;
    // Identity + snapshot consumed 4 IDs. Consume 988 more, leaving 7.
    for _ in 0..988 {
        session
            .request(mi_proto::Method::SysVersion, Value::Null)
            .await
            .unwrap();
    }
    let (_dir, plan) = plan(RAW, session.id, 7);
    let writer = ConfigWriter::new(session.clone()).unwrap();
    // Revalidation uses three, then a write plus three readback calls still fits.
    // Consume one further ID so the write must be rejected rather than stranding readback.
    session
        .request(mi_proto::Method::SysVersion, Value::Null)
        .await
        .unwrap();
    assert_eq!(writer.apply(&plan).await, Outcome::NotSent);
    assert!(!device
        .synthetic_requests()
        .iter()
        .any(|m| m == "fs.writebin"));
    session.shutdown().await;
}
#[tokio::test]
async fn cancelling_after_send_closes_session_and_never_replays() {
    let mut files = SyntheticFiles::new(RAW.to_vec(), None);
    files.lose_ack_offset = Some(0);
    let (device, session) = setup(files).await;
    let (_dir, plan) = plan(RAW, session.id, 7);
    let writer = ConfigWriter::new(session.clone()).unwrap();
    let mut pending = Box::pin(writer.apply(&plan));
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            tokio::select! {
                result = &mut pending => panic!("completed before cancellation: {result:?}"),
                _ = tokio::time::sleep(Duration::from_millis(1)) => {
                    if device.synthetic_requests().iter().any(|m| m == "fs.writebin") { break; }
                }
            }
        }
    })
    .await
    .unwrap();
    drop(pending);
    assert_eq!(
        OperationRecord::open(plan.record_path())
            .unwrap()
            .restart_phase(),
        Phase::DeviceStateUnknown
    );
    assert!(device
        .synthetic_requests()
        .iter()
        .any(|m| m == "fs.writebin"));
    assert!(session
        .request(mi_proto::Method::SysVersion, Value::Null)
        .await
        .is_err());
    assert_eq!(writer.apply(&plan).await, Outcome::NotSent);
    session.shutdown().await;
}

#[tokio::test]
async fn ordinary_ag_key_candidate_preserves_smart_file_through_real_readback() {
    let smart =
        include_bytes!("../../mi-model/fixtures/device/synthetic-smart-actions-object.json");
    let (device, session) = setup(SyntheticFiles::new(RAW.to_vec(), Some(smart.to_vec()))).await;
    let mut base = baseline(RAW, session.id);
    base["smart_actions"] = json!({"state":"present","size":smart.len(),"sha1":format!("{:x}",Sha1::digest(smart)),"bytes":smart.as_slice()});
    let config = mi_model::parse_device_files(RAW, Some(smart)).unwrap();
    let doc = LocalDocumentV2::from_config(&config, Some(base.clone()), "device-snapshot").unwrap();
    let p = doc.editable.graph.profile_order[0].clone();
    let l = doc.editable.graph.profiles[&p].layer_order[0].clone();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("draft.json");
    std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
    let mut store = DocumentStore::default();
    store.open(path).unwrap();
    store
        .update(|d| {
            d.assign(
                &json!(p),
                &json!(l),
                0,
                mi_model::draft::Action {
                    id: "key".into(),
                    name: "Key".into(),
                    category: "Device".into(),
                    detail: "local".into(),
                    payload: json!({"type":"device_key","token":"KC_B"}),
                },
            )
        })
        .unwrap();
    let preview = store.preview_apply(&base).unwrap();
    store.prepare_apply_backup(&preview.token, &base).unwrap();
    let plan = store
        .take_prepared_candidate(&preview.token, &base)
        .unwrap();
    let writer = ConfigWriter::new(session.clone()).unwrap();
    assert_eq!(writer.apply(&plan).await, Outcome::Verified);
    assert_eq!(
        device.files().unwrap().smart_actions.as_deref(),
        Some(smart.as_slice())
    );
    assert_eq!(device.files().unwrap().keymap, plan.candidate());
    let mut reopened = DocumentStore::default();
    reopened.open(dir.path().join("draft.json")).unwrap();
    let current = writer.inspect().await.unwrap();
    assert!(
        reopened.preview_apply(&current).is_err(),
        "applied key must not compile twice"
    );
    session.shutdown().await;
}

use mi_model::operation_record::{OperationRecord, Phase};
use mi_session::config_write::Checkpoint;
fn mutations(d: &SyntheticDiscovery) -> usize {
    d.synthetic_requests()
        .iter()
        .filter(|m| *m == "fs.writebin")
        .count()
}
async fn reconnect(d: &SyntheticDiscovery) -> Arc<DeviceSession> {
    let s = DeviceSession::new(
        d.open("synthetic-interface-0").unwrap(),
        Box::<mi_proto::NativeFramer>::default(),
        Arc::new(ReadOnlySchema),
        SessionConfig {
            timeout: Duration::from_millis(100),
            interval: Duration::ZERO,
        },
    );
    s.read_identity_info().await.unwrap();
    s
}
fn block_status(path: &std::path::Path) {
    std::fs::rename(path.join("status.json"), path.join("status.saved")).unwrap();
    std::fs::create_dir(path.join("status.json")).unwrap();
}
fn unblock_status(path: &std::path::Path) {
    std::fs::remove_dir(path.join("status.json")).unwrap();
    std::fs::rename(path.join("status.saved"), path.join("status.json")).unwrap();
}
#[tokio::test]
async fn durable_phase_faults_distinguish_no_send_unknown_and_verified_local_failure() {
    for checkpoint in [
        Checkpoint::BeforeRevalidate,
        Checkpoint::BeforeWrite(0),
        Checkpoint::AfterAck(1),
        Checkpoint::BeforeVerifiedSave,
        Checkpoint::BeforeLocalSave,
    ] {
        let (device, session) = setup(SyntheticFiles::new(RAW.to_vec(), None)).await;
        let (dir, p) = plan(RAW, session.id, 55);
        let writer = ConfigWriter::new(session.clone()).unwrap();
        let result = writer
            .apply_observed(&p, |at, path| {
                if at == checkpoint {
                    if at == Checkpoint::BeforeLocalSave {
                        std::fs::write(dir.path().join("draft.lock"), b"fault").unwrap();
                    } else {
                        block_status(path);
                    }
                }
            })
            .await;
        if checkpoint == Checkpoint::BeforeLocalSave {
            std::fs::remove_file(dir.path().join("draft.lock")).unwrap();
        } else {
            unblock_status(&p.record_path());
        }
        let record = OperationRecord::open(p.record_path()).unwrap();
        match checkpoint {
            Checkpoint::BeforeRevalidate | Checkpoint::BeforeWrite(0) => {
                assert_eq!(result, Outcome::NotSent);
                assert_eq!(mutations(&device), 0);
                assert_eq!(record.restart_phase(), Phase::NotSent);
            }
            Checkpoint::AfterAck(1) => {
                assert_eq!(result, Outcome::DeviceStateUnknown);
                assert!(!record.status().device_verified);
                assert_eq!(record.restart_phase(), Phase::DeviceStateUnknown);
            }
            Checkpoint::BeforeVerifiedSave => {
                assert_eq!(result, Outcome::VerifiedLocalSaveFailed);
                assert!(!record.status().device_verified);
                assert!(OperationRecord::open(p.record_path())
                    .unwrap()
                    .retry_local_save()
                    .is_err());
            }
            Checkpoint::BeforeLocalSave => {
                assert_eq!(result, Outcome::VerifiedLocalSaveFailed);
                assert!(record.status().device_verified);
                assert_eq!(
                    record.status().error_category,
                    Some(mi_model::operation_record::Failure::LocalSave)
                );
            }
            _ => unreachable!(),
        }
        let count = mutations(&device);
        if matches!(
            checkpoint,
            Checkpoint::BeforeVerifiedSave | Checkpoint::BeforeLocalSave
        ) {
            writer.retry_local_save(p.operation_id()).unwrap();
            writer.retry_local_save(p.operation_id()).unwrap();
            let final_record = OperationRecord::open(p.record_path()).unwrap();
            assert!(final_record.status().local_saved);
            let mut reopened = DocumentStore::default();
            reopened.open(dir.path().join("draft.json")).unwrap();
            assert!(!reopened.draft.read_only);
            assert!(reopened.draft.document.structure_changes().is_empty());
        }
        assert_eq!(mutations(&device), count);
        // Persisted consumption survives constructing another writer.
        assert_eq!(
            ConfigWriter::new(session.clone()).unwrap().apply(&p).await,
            Outcome::NotSent
        );
        assert_eq!(mutations(&device), count);
        session.shutdown().await;
    }
}
#[tokio::test]
async fn restart_local_repair_requires_persisted_proof_and_refuses_changed_document() {
    let (device, session) = setup(SyntheticFiles::new(RAW.to_vec(), None)).await;
    let (dir, p) = plan(RAW, session.id, 44);
    let writer = ConfigWriter::new(session.clone()).unwrap();
    assert_eq!(
        writer
            .apply_observed(&p, |at, _| {
                if at == Checkpoint::BeforeLocalSave {
                    std::fs::write(dir.path().join("draft.lock"), b"fault").unwrap();
                }
            })
            .await,
        Outcome::VerifiedLocalSaveFailed
    );
    std::fs::remove_file(dir.path().join("draft.lock")).unwrap();
    let mut store = DocumentStore::default();
    store.open(dir.path().join("draft.json")).unwrap();
    assert!(store.draft.read_only);
    let original = std::fs::read(dir.path().join("draft.json")).unwrap();
    let mut changed = original.clone();
    changed.push(b' ');
    std::fs::write(dir.path().join("draft.json"), &changed).unwrap();
    let count = mutations(&device);
    let mut record = OperationRecord::open(p.record_path()).unwrap();
    assert!(record.retry_local_save().is_err());
    assert_eq!(
        std::fs::read(dir.path().join("draft.json")).unwrap(),
        changed
    );
    std::fs::write(dir.path().join("draft.json"), original).unwrap();
    record.retry_local_save().unwrap();
    let saved = std::fs::read(dir.path().join("draft.json")).unwrap();
    OperationRecord::open(p.record_path())
        .unwrap()
        .retry_local_save()
        .unwrap();
    assert_eq!(std::fs::read(dir.path().join("draft.json")).unwrap(), saved);
    assert_eq!(mutations(&device), count);
    session.shutdown().await;
}
#[tokio::test]
async fn lost_verification_record_needs_explicit_readback_after_restart() {
    let (device, session) = setup(SyntheticFiles::new(RAW.to_vec(), None)).await;
    let (_dir, p) = plan(RAW, session.id, 22);
    let writer = ConfigWriter::new(session.clone()).unwrap();
    assert_eq!(
        writer
            .apply_observed(&p, |at, path| {
                if at == Checkpoint::BeforeVerifiedSave {
                    block_status(path);
                }
            })
            .await,
        Outcome::VerifiedLocalSaveFailed
    );
    unblock_status(&p.record_path());
    drop(writer);
    session.shutdown().await;
    let mut record = OperationRecord::open(p.record_path()).unwrap();
    assert!(record.retry_local_save().is_err());
    let s = reconnect(&device).await;
    let w = ConfigWriter::new(s.clone()).unwrap();
    let count = mutations(&device);
    w.confirm_readback(&mut record).await.unwrap();
    record.retry_local_save().unwrap();
    assert_eq!(mutations(&device), count);
    s.shutdown().await;
}
#[tokio::test]
async fn corrupt_partial_file_restores_from_linked_backup_preserving_current_smart() {
    let mut v: Value = serde_json::from_slice(RAW).unwrap();
    v["padding"] = json!("x".repeat(7000));
    let raw = serde_json::to_vec(&v).unwrap();
    let mut files = SyntheticFiles::new(raw.clone(), None);
    files.reject_offset = Some(3072);
    let (device, session) = setup(files).await;
    let (dir, p) = plan(&raw, session.id, 23);
    assert_eq!(
        ConfigWriter::new(session.clone()).unwrap().apply(&p).await,
        Outcome::DeviceStateUnknown
    );
    assert!(mi_model::parse_device_files(&device.files().unwrap().keymap, None).is_err());
    let mut state = device.files().unwrap();
    state.reject_offset = None;
    state.smart_actions = Some(br#"{"smartActions":{}}"#.to_vec());
    device.set_files(state);
    session.shutdown().await;
    let s = reconnect(&device).await;
    let writer = ConfigWriter::new(s.clone()).unwrap();
    let record = OperationRecord::open(p.record_path()).unwrap();
    let before = mutations(&device);
    assert_eq!(
        record.classify(&writer.inspect().await.unwrap()).unwrap(),
        "other"
    );
    let restore = writer.prepare_restore(&record).await.unwrap();
    assert_eq!(mutations(&device), before);
    assert_eq!(restore.candidate(), raw);
    assert_eq!(
        restore.smart_actions(),
        Some(br#"{"smartActions":{}}"#.as_slice())
    );
    assert_eq!(writer.apply(&restore).await, Outcome::Verified);
    assert_eq!(device.files().unwrap().keymap, raw);
    assert_eq!(
        device.files().unwrap().smart_actions,
        Some(br#"{"smartActions":{}}"#.to_vec())
    );
    let recovered = OperationRecord::open(restore.record_path()).unwrap();
    assert_eq!(
        recovered.status().restores.as_deref(),
        Some(p.operation_id())
    );
    let original = OperationRecord::open(p.record_path()).unwrap();
    assert_eq!(
        original.status().resolved_by.as_deref(),
        Some(restore.operation_id())
    );
    let mut reopened = DocumentStore::default();
    reopened.open(dir.path().join("draft.json")).unwrap();
    assert!(!reopened.draft.read_only);
    s.shutdown().await;
}
#[tokio::test]
async fn stale_restore_refuses_changes_and_new_review_can_bind_new_bytes() {
    let (device, session) = setup(SyntheticFiles::new(RAW.to_vec(), None)).await;
    let (_dir, p) = plan(RAW, session.id, 12);
    let writer = ConfigWriter::new(session.clone()).unwrap();
    assert_eq!(writer.apply(&p).await, Outcome::Verified);
    let record = OperationRecord::open(p.record_path()).unwrap();
    let stale = writer.prepare_restore(&record).await.unwrap();
    let mut state = device.files().unwrap();
    state.keymap.push(b' ');
    device.set_files(state);
    let before = mutations(&device);
    assert_eq!(writer.apply(&stale).await, Outcome::NotSent);
    assert_eq!(mutations(&device), before);
    let fresh = writer.prepare_restore(&record).await.unwrap();
    assert_eq!(writer.apply(&fresh).await, Outcome::Verified);
    assert_eq!(device.files().unwrap().keymap, RAW);
    session.shutdown().await;
}
#[tokio::test]
async fn recovery_rejects_missing_bad_digest_failed_read_and_unverified_identity() {
    for fault in 0..4 {
        let (device, session) = setup(SyntheticFiles::new(RAW.to_vec(), None)).await;
        let (_dir, p) = plan(RAW, session.id, 14);
        let writer = ConfigWriter::new(session.clone()).unwrap();
        assert_eq!(writer.apply(&p).await, Outcome::Verified);
        let record = OperationRecord::open(p.record_path()).unwrap();
        let mut files = device.files().unwrap();
        match fault {
            0 => files.missing_keymap = true,
            1 => files.bad_checksum = true,
            2 => files.fail_read = true,
            _ => {}
        }
        device.set_files(files);
        let before = mutations(&device);
        if fault == 3 {
            let mut current = writer.inspect().await.unwrap();
            current["device_identity"]["confidence"] = json!("unknown");
            assert!(record.prepare_restore(current).is_err());
        } else {
            assert!(writer.prepare_restore(&record).await.is_err());
        }
        assert_eq!(mutations(&device), before);
        session.shutdown().await;
    }
}
#[tokio::test]
async fn records_are_private_checksummed_and_corruption_never_authorizes_repair() {
    use std::os::unix::fs::PermissionsExt;
    let (device, session) = setup(SyntheticFiles::new(RAW.to_vec(), None)).await;
    let (_dir, p) = plan(RAW, session.id, 18);
    let writer = ConfigWriter::new(session.clone()).unwrap();
    assert_eq!(writer.apply(&p).await, Outcome::Verified);
    assert_eq!(
        std::fs::metadata(p.record_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    for name in [
        "status.json",
        "material.json",
        "document-before.json",
        "document-target.json",
    ] {
        assert_eq!(
            std::fs::metadata(p.record_path().join(name))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    let count = mutations(&device);
    for name in [
        "status.json",
        "material.json",
        "document-before.json",
        "document-target.json",
    ] {
        let path = p.record_path().join(name);
        let original = std::fs::read(&path).unwrap();
        let mut corrupted = original.clone();
        corrupted.push(b'x');
        std::fs::write(&path, &corrupted).unwrap();
        assert!(OperationRecord::open(p.record_path()).is_err());
        std::fs::write(path, original).unwrap();
    }
    assert_eq!(mutations(&device), count);
    session.shutdown().await;
}

#[tokio::test]
async fn explicit_original_readback_unlocks_without_claiming_not_sent_or_rewriting() {
    let mut files = SyntheticFiles::new(RAW.to_vec(), None);
    files.reject_offset = Some(0);
    let (device, session) = setup(files).await;
    let (dir, p) = plan(RAW, session.id, 21);
    assert_eq!(
        ConfigWriter::new(session.clone()).unwrap().apply(&p).await,
        Outcome::DeviceStateUnknown
    );
    session.shutdown().await;
    let s = reconnect(&device).await;
    let writer = ConfigWriter::new(s.clone()).unwrap();
    let count = mutations(&device);
    let mut record = OperationRecord::open(p.record_path()).unwrap();
    writer.confirm_readback(&mut record).await.unwrap();
    assert_eq!(record.status().phase, Phase::OriginalVerified);
    assert!(!record.status().device_verified);
    assert!(record.retry_local_save().is_err());
    let mut store = DocumentStore::default();
    store.open(dir.path().join("draft.json")).unwrap();
    assert!(!store.draft.read_only);
    assert_eq!(mutations(&device), count);
    s.shutdown().await;
}
#[tokio::test]
async fn failed_restore_can_be_replaced_by_new_review_of_same_original_backup() {
    let (device, session) = setup(SyntheticFiles::new(RAW.to_vec(), None)).await;
    let (dir, p) = plan(RAW, session.id, 33);
    let writer = ConfigWriter::new(session.clone()).unwrap();
    assert_eq!(writer.apply(&p).await, Outcome::Verified);
    let record = OperationRecord::open(p.record_path()).unwrap();
    let first = writer.prepare_restore(&record).await.unwrap();
    let mut files = device.files().unwrap();
    files.reject_offset = Some(0);
    device.set_files(files);
    assert_eq!(writer.apply(&first).await, Outcome::DeviceStateUnknown);
    session.shutdown().await;
    let mut files = device.files().unwrap();
    files.reject_offset = None;
    device.set_files(files);
    let s = reconnect(&device).await;
    let writer = ConfigWriter::new(s.clone()).unwrap();
    let fresh = writer.prepare_restore(&record).await.unwrap();
    assert_eq!(writer.apply(&fresh).await, Outcome::Verified);
    assert_eq!(device.files().unwrap().keymap, RAW);
    assert!(OperationRecord::open(first.record_path())
        .unwrap()
        .status()
        .resolved_by
        .is_some());
    let mut store = DocumentStore::default();
    store.open(dir.path().join("draft.json")).unwrap();
    assert!(!store.draft.read_only);
    s.shutdown().await;
}

#[tokio::test]
async fn readback_failure_keeps_unknown_until_independent_fresh_check() {
    let (device, session) = setup(SyntheticFiles::new(RAW.to_vec(), None)).await;
    let (_dir, p) = plan(RAW, session.id, 29);
    let writer = ConfigWriter::new(session.clone()).unwrap();
    assert_eq!(
        writer
            .apply_observed(&p, |at, _| {
                if at == Checkpoint::AfterAck(1) {
                    let mut f = device.files().unwrap();
                    f.fail_read = true;
                    device.set_files(f);
                }
            })
            .await,
        Outcome::DeviceStateUnknown
    );
    assert_eq!(device.files().unwrap().keymap, p.candidate());
    let mut r = OperationRecord::open(p.record_path()).unwrap();
    assert!(!r.status().device_verified);
    assert!(r.retry_local_save().is_err());
    session.shutdown().await;
    let mut f = device.files().unwrap();
    f.fail_read = false;
    device.set_files(f);
    let s = reconnect(&device).await;
    let count = mutations(&device);
    ConfigWriter::new(s.clone())
        .unwrap()
        .confirm_readback(&mut r)
        .await
        .unwrap();
    r.retry_local_save().unwrap();
    assert_eq!(mutations(&device), count);
    s.shutdown().await;
}

// Exercise the actual native writer branch and production byte barrier without HID.
struct NativeFixtureLink {
    inner: Arc<dyn HidLink>,
    incoming: tokio::sync::Mutex<(mi_proto::NativeFramer, std::collections::VecDeque<Vec<u8>>)>,
}
#[async_trait::async_trait]
impl HidLink for NativeFixtureLink {
    fn backend(&self) -> LinkBackend {
        LinkBackend::MacHidapi267Numbered
    }
    async fn write(&self, report: &[u8]) -> Result<(), TransportError> {
        self.inner.write(report).await
    }
    async fn read(&self) -> Result<Vec<u8>, TransportError> {
        use mi_proto::Framer;
        let mut state = self.incoming.lock().await;
        loop {
            if let Some(frame) = state.1.pop_front() {
                return Ok(frame);
            }
            let report = self.inner.read().await?;
            for line in state.0.feed(&report).unwrap() {
                let mut value: Value = serde_json::from_slice(&line).unwrap();
                if value["result"]["version"] == "synthetic-0.6.3" {
                    value["result"]["version"] = json!("0.6.3");
                }
                state.1.extend(
                    mi_proto::NativeFramer::default()
                        .encode(value.to_string().as_bytes())
                        .unwrap(),
                );
            }
        }
    }
    fn close(&self) {
        self.inner.close();
    }
}
#[tokio::test]
async fn native_plan_bound_writer_applies_and_restores_but_rejects_another_connection() {
    let device = Arc::new(SyntheticDiscovery::default());
    device.set_synthetic_discovery(1).unwrap();
    device.set_files(SyntheticFiles::new(RAW.to_vec(), None));
    let link = Arc::new(NativeFixtureLink {
        inner: device.open("synthetic-interface-0").unwrap(),
        incoming: tokio::sync::Mutex::new(Default::default()),
    });
    let session = DeviceSession::new(
        link,
        Box::<mi_proto::NativeFramer>::default(),
        Arc::new(ReadOnlySchema),
        SessionConfig {
            timeout: Duration::from_millis(500),
            interval: Duration::ZERO,
        },
    );
    let writer = ConfigWriter::native(session.clone(), "fake-native".into()).unwrap();
    let base = writer.inspect().await.unwrap();
    let config = mi_model::parse_device_files(RAW, None).unwrap();
    let doc = LocalDocumentV2::from_config(&config, Some(base.clone()), "device-snapshot").unwrap();
    let p = doc.editable.graph.profile_order[0].clone();
    let l = doc.editable.graph.profiles[&p].layer_order[0].clone();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("draft.json");
    std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
    let mut store = DocumentStore::default();
    store.open(path).unwrap();
    store
        .update(|d| d.edit_lighting(&p, &l, Zone::Backlight, Edit::Color(0x123457), false))
        .unwrap();
    let preview = store.preview_apply(&base).unwrap();
    store.prepare_apply_backup(&preview.token, &base).unwrap();
    let plan = store
        .take_prepared_candidate(&preview.token, &base)
        .unwrap();
    assert_eq!(writer.apply(&plan).await, Outcome::Verified);
    let record = mi_model::operation_record::OperationRecord::open(plan.record_path()).unwrap();
    let mut different = writer.inspect().await.unwrap();
    different["device_identity"]["connection_nonce"] = json!("another-process");
    assert!(record.prepare_restore(different).is_err());
    let restore = writer.prepare_restore(&record).await.unwrap();
    assert_eq!(writer.apply(&restore).await, Outcome::Verified);
    assert_eq!(device.files().unwrap().keymap, RAW);
    session.shutdown().await;
}
