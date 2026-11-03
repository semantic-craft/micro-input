use mi_model::{
    apply::PreparedCandidate,
    document::LocalDocumentV2,
    lighting::{Edit, Zone},
    operation_record::{ensure_document_editable, file_value, OperationRecord, Phase},
    storage::DocumentStore,
};
use serde_json::json;
use std::{fs, os::unix::fs::MetadataExt};
const RAW: &[u8] = include_bytes!("../fixtures/device/synthetic-keymap-lighting.json");
fn candidate() -> (tempfile::TempDir, PreparedCandidate) {
    let dir = tempfile::tempdir().unwrap();
    let base = json!({"device_identity":{"confidence":"verified-serial","serial":"synthetic","session_id":8},"firmware":"synthetic-0.6.3","keymap":file_value(RAW),"smart_actions":{"state":"missing"}});
    let config = mi_model::parse_device_files(RAW, None).unwrap();
    let doc = LocalDocumentV2::from_config(&config, Some(base.clone()), "device-snapshot").unwrap();
    let p = doc.editable.graph.profile_order[0].clone();
    let l = doc.editable.graph.profiles[&p].layer_order[0].clone();
    let path = dir.path().join("draft.json");
    fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
    let mut store = DocumentStore::default();
    store.open(path).unwrap();
    store
        .update(|d| d.edit_lighting(&p, &l, Zone::Backlight, Edit::Color(99), false))
        .unwrap();
    let preview = store.preview_apply(&base).unwrap();
    store.prepare_apply_backup(&preview.token, &base).unwrap();
    let plan = store
        .take_prepared_candidate(&preview.token, &base)
        .unwrap();
    (dir, plan)
}
#[test]
fn durable_consumption_and_restart_classification_never_resume() {
    let (dir, p) = candidate();
    let mut r = OperationRecord::begin(&p).unwrap();
    assert!(OperationRecord::begin(&p).is_err());
    assert_eq!(r.restart_phase(), Phase::NotSent);
    assert!(ensure_document_editable(&dir.path().join("draft.json")).is_err());
    r.advance(Phase::Revalidating, 0).unwrap();
    r.advance(Phase::Writing, 0).unwrap();
    drop(r);
    let reopened = OperationRecord::open(p.record_path()).unwrap();
    assert_eq!(reopened.restart_phase(), Phase::DeviceStateUnknown);
    assert!(!reopened.status().device_verified);
    assert!(OperationRecord::open(p.record_path())
        .unwrap()
        .retry_local_save()
        .is_err());
    assert!(ensure_document_editable(&dir.path().join("draft.json")).is_err());
}
#[test]
fn local_target_already_saved_only_repairs_record_and_clears_old_undo_on_reopen() {
    let (dir, p) = candidate();
    let mut r = OperationRecord::begin(&p).unwrap();
    r.advance(Phase::Revalidating, 0).unwrap();
    r.advance(Phase::Writing, 0).unwrap();
    r.advance(Phase::Writing, 1).unwrap();
    r.advance(Phase::Verifying, 1).unwrap();
    r.advance(Phase::Verified, 1).unwrap();
    // Crash between document rename and completion record: only status needs repair.
    let path = dir.path().join("draft.json");
    fs::copy(p.record_path().join("document-target.json"), &path).unwrap();
    let inode = fs::metadata(&path).unwrap().ino();
    drop(r);
    let mut r = OperationRecord::open(p.record_path()).unwrap();
    r.retry_local_save().unwrap();
    r.retry_local_save().unwrap();
    assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
    assert!(r.status().local_saved);
    let mut store = DocumentStore::default();
    store.open(path).unwrap();
    let rev = store.draft.revision;
    store.draft.undo(false).unwrap();
    assert_eq!(store.draft.revision, rev);
    assert!(store.draft.document.structure_changes().is_empty());
}
#[test]
fn malformed_state_original_backup_and_future_local_document_are_rejected() {
    let (dir, p) = candidate();
    let r = OperationRecord::begin(&p).unwrap();
    let mut raw = json!(r.status());
    raw["phase"] = json!("Verified");
    // Updating a state label without its digest does not manufacture successful evidence.
    let status = p.record_path().join("status.json");
    let saved = fs::read(&status).unwrap();
    let mut env: serde_json::Value = serde_json::from_slice(&saved).unwrap();
    env["status"] = raw;
    fs::write(&status, serde_json::to_vec(&env).unwrap()).unwrap();
    assert!(OperationRecord::open(p.record_path()).is_err());
    fs::write(status, saved).unwrap();
    let current = json!({"device_identity":{"confidence":"verified-serial","serial":"synthetic","session_id":9},"firmware":"synthetic-0.6.3","keymap":file_value(RAW),"smart_actions":{"state":"missing"}});
    let path = dir.path().join("draft.json");
    let mut future: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    future["future_field"] = json!(true);
    fs::write(&path, serde_json::to_vec(&future).unwrap()).unwrap();
    assert!(r.prepare_restore(current).is_err());
    let backup = p.record_path().parent().unwrap().join("keymap.json");
    fs::write(backup, b"corrupt").unwrap();
    assert!(OperationRecord::open(p.record_path()).is_err());
}
#[test]
fn not_sent_record_releases_document_but_never_reuses_operation_id() {
    let (dir, p) = candidate();
    let mut r = OperationRecord::begin(&p).unwrap();
    r.advance(Phase::NotSent, 0).unwrap();
    assert!(ensure_document_editable(&dir.path().join("draft.json")).is_ok());
    assert!(OperationRecord::begin(&p).is_err());
    assert!(r.advance(Phase::Writing, 0).is_err());
}

#[test]
fn completed_local_retry_does_not_touch_a_later_revision() {
    let (dir, p) = candidate();
    let mut r = OperationRecord::begin(&p).unwrap();
    r.advance(Phase::Revalidating, 0).unwrap();
    r.advance(Phase::Writing, 0).unwrap();
    assert!(r.advance(Phase::Verifying, 0).is_err());
    r.advance(Phase::Writing, 1).unwrap();
    r.advance(Phase::Verifying, 1).unwrap();
    r.advance(Phase::Verified, 1).unwrap();
    r.retry_local_save().unwrap();
    let path = dir.path().join("draft.json");
    let mut store = DocumentStore::default();
    store.open(path.clone()).unwrap();
    let pid = store.draft.document.editable.graph.profile_order[0].clone();
    store
        .update(|d| d.rename_profile(&pid, "Later revision"))
        .unwrap();
    let later = fs::read(&path).unwrap();
    OperationRecord::open(p.record_path())
        .unwrap()
        .retry_local_save()
        .unwrap();
    assert_eq!(fs::read(path).unwrap(), later);
}
