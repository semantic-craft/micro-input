#![cfg(feature = "synthetic-backend")]
use mi_model::{
    document::LocalDocumentV2,
    lighting::{Edit, Zone},
    storage::DocumentStore,
};
use mi_session::{
    config_write::{Outcome, SyntheticWriter},
    DeviceSession, ReadOnlySchema, SessionConfig,
};
use mi_transport::{
    HidLink, HidTransport, LinkBackend, SyntheticDiscovery, SyntheticFiles, TransportError,
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
    let writer = SyntheticWriter::new(session.clone()).unwrap();
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
        let writer = SyntheticWriter::new(session.clone()).unwrap();
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
    let writer = SyntheticWriter::new(session.clone()).unwrap();
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
    assert!(SyntheticWriter::new(session.clone()).is_err());
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
    let writer = SyntheticWriter::new(session.clone()).unwrap();
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
    let writer = SyntheticWriter::new(session.clone()).unwrap();
    let result = tokio::time::timeout(Duration::from_millis(10), writer.apply(&plan)).await;
    assert!(result.is_err());
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
    let writer = SyntheticWriter::new(session.clone()).unwrap();
    assert_eq!(writer.apply(&plan).await, Outcome::Verified);
    assert_eq!(
        device.files().unwrap().smart_actions.as_deref(),
        Some(smart.as_slice())
    );
    assert_eq!(device.files().unwrap().keymap, plan.candidate());
    session.shutdown().await;
}
