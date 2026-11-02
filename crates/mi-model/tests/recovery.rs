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
