use mi_model::{draft::LegacyDocumentV1, storage::DocumentStore};
use std::{fs, path::PathBuf};
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("mi-recovery-{}", mi_model::document::new_id()));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn corrupt_document_can_recover_verified_legacy_backup_as_separate_editable_copy() {
    let t = Directory::new();
    let path = t.0.join("workbench.json");
    fs::write(&path, b"{broken").unwrap();
    let legacy = LegacyDocumentV1::new(mi_model::demo().unwrap()).unwrap();
    let bytes = serde_json::to_vec_pretty(&legacy).unwrap();
    let source = t.0.join("workbench.json.v1.backup");
    fs::write(&source, &bytes).unwrap();
    let mut store = DocumentStore::default();
    assert!(store.open(path.clone()).is_err());
    assert!(store
        .recovery_files()
        .unwrap()
        .contains(&source.to_string_lossy().into_owned()));
    let preview = store.inspect_recovery(source.clone()).unwrap();
    assert_eq!(preview.schema_version, Some(1));
    assert!(preview.can_restore);
    assert_eq!(preview.source_kind, "legacy-local");
    store.recover_document(&preview.token).unwrap();
    store
        .update(|d| d.create_profile("Recovered".into()).map(|_| ()))
        .unwrap();
    let id = store.draft.document_id.clone();
    let mut reopened = DocumentStore::default();
    reopened.open(path.clone()).unwrap();
    assert_eq!(reopened.draft.document_id, id);
    assert!(reopened
        .draft
        .document
        .editable
        .graph
        .profiles
        .values()
        .any(|p| p.name == "Recovered"));
    assert_eq!(fs::read(&path).unwrap(), b"{broken");
    assert_eq!(fs::read(source).unwrap(), bytes);
}
#[test]
fn damaged_selection_record_is_preserved_before_recovery_switches_to_new_copy() {
    let t = Directory::new();
    let path = t.0.join("workbench.json");
    let active = path.with_extension("active.json");
    fs::write(&active, b"not a selection").unwrap();
    let legacy = LegacyDocumentV1::new(mi_model::demo().unwrap()).unwrap();
    fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    let mut store = DocumentStore::default();
    assert!(store.open(path.clone()).is_err());
    let preview = store.inspect_recovery(path).unwrap();
    store.recover_document(&preview.token).unwrap();
    let backups: Vec<_> = fs::read_dir(&t.0)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|p| p.path().extension().is_some_and(|e| e == "backup"))
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(fs::read(backups[0].path()).unwrap(), b"not a selection");
}
#[test]
fn future_and_unknown_documents_only_allow_exact_non_overwriting_copies() {
    for bytes in [
        br#"{"schema_version":999,"future":{"keep":true}}"#.as_slice(),
        b"{corrupt",
    ] {
        let t = Directory::new();
        let path = t.0.join("workbench.json");
        fs::write(&path, bytes).unwrap();
        let mut store = DocumentStore::default();
        assert!(store.open(path.clone()).is_err());
        let preview = store.inspect_recovery(path.clone()).unwrap();
        assert!(!preview.can_restore);
        let before = store.draft.document_id.clone();
        assert!(store.recover_document(&preview.token).is_err());
        assert!(store.save_document_as().is_err());
        let copy = t.0.join("exact.json");
        store
            .copy_recovery_original(&preview.token, copy.clone())
            .unwrap();
        assert_eq!(fs::read(&copy).unwrap(), bytes);
        assert!(store
            .copy_recovery_original(&preview.token, path.clone())
            .is_err());
        assert_eq!(fs::read(path).unwrap(), bytes);
        assert_eq!(store.draft.document_id, before);
        assert!(!t.0.join("workbench.active.json").exists());
    }
}
#[test]
fn save_as_preserves_concurrent_changes_and_reopens_in_memory_draft_copy() {
    let t = Directory::new();
    let path = t.0.join("workbench.json");
    let mut store = DocumentStore::default();
    store.open(path.clone()).unwrap();
    store
        .update(|d| d.create_profile("Mine".into()).map(|_| ()))
        .unwrap();
    let before = store.draft.document_id.clone();
    fs::write(&path, b"external version").unwrap();
    assert!(store
        .update(|d| d.create_profile("Lost".into()).map(|_| ()))
        .is_err());
    store.save_document_as().unwrap();
    assert!(matches!(
        store.draft.config.source,
        mi_model::Source::LocalDraft
    ));
    assert_ne!(store.draft.document_id, before);
    assert!(!store.draft.can_undo);
    assert_eq!(fs::read(&path).unwrap(), b"external version");
    let mut reopened = DocumentStore::default();
    reopened.open(path).unwrap();
    assert_eq!(reopened.draft.document_id, store.draft.document_id);
    assert!(reopened
        .draft
        .document
        .editable
        .graph
        .profiles
        .values()
        .any(|p| p.name == "Mine"));
    assert!(!reopened
        .draft
        .document
        .editable
        .graph
        .profiles
        .values()
        .any(|p| p.name == "Lost"));
}
#[test]
fn stale_preview_pending_and_pointer_conflicts_never_change_memory_or_selection() {
    for failure in ["source", "pending", "lock", "selection"] {
        let t = Directory::new();
        let path = t.0.join("workbench.json");
        let mut store = DocumentStore::default();
        store.open(path.clone()).unwrap();
        store
            .update(|d| d.create_profile("Original".into()).map(|_| ()))
            .unwrap();
        let original = fs::read(&path).unwrap();
        let id = store.draft.document_id.clone();
        let source = t.0.join("recover.backup");
        fs::write(&source, &original).unwrap();
        let preview = store.inspect_recovery(source.clone()).unwrap();
        let marker = match failure {
            "source" => source.clone(),
            "pending" => t.0.join("workbench.active.pending"),
            "lock" => t.0.join("workbench.active.lock"),
            _ => t.0.join("workbench.active.json"),
        };
        fs::write(&marker, b"external").unwrap();
        assert!(store.recover_document(&preview.token).is_err(), "{failure}");
        assert_eq!(store.draft.document_id, id);
        assert_eq!(fs::read(path).unwrap(), original);
        assert_eq!(fs::read(marker).unwrap(), b"external");
    }
}
#[test]
fn recovered_v2_is_a_local_draft_even_when_historical_config_claimed_device_snapshot() {
    let t = Directory::new();
    let path = t.0.join("workbench.json");
    let mut config = mi_model::demo().unwrap();
    config.source = mi_model::Source::DeviceSnapshot;
    let document =
        mi_model::document::LocalDocumentV2::from_config(&config, None, "device-snapshot").unwrap();
    fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
    let mut store = DocumentStore::default();
    store.open(path.clone()).unwrap();
    let preview = store.inspect_recovery(path.clone()).unwrap();
    store.recover_document(&preview.token).unwrap();
    assert!(matches!(
        store.draft.config.source,
        mi_model::Source::LocalDraft
    ));
    assert_eq!(
        store.draft.document.origin["config"]["source"],
        "DeviceSnapshot"
    );
    let mut reopened = DocumentStore::default();
    reopened.open(path).unwrap();
    assert!(matches!(
        reopened.draft.config.source,
        mi_model::Source::LocalDraft
    ));
}
