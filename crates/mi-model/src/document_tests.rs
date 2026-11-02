//! R-E07/R-E08/R-K05 regression checks against data loss and wrong-target edits.
use crate::{draft::*, storage::DocumentStore};
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "mi-document-{}-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn file(&self) -> PathBuf {
        self.0.join("workbench.json")
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn assign(d: &mut Draft) -> Result<(), String> {
    let p = d.config.profiles[0].id.clone();
    let l = d.config.profiles[0].layers[0].id.clone();
    d.assign(&p, &l, 1, actions()[1].clone())
}
#[test]
fn stable_ids_survive_reorder_and_rename() {
    let t = Temp::new();
    let mut s = DocumentStore::default();
    s.open(t.file()).unwrap();
    s.update(assign).unwrap();
    let pid = s.draft.config.profiles[0].id.as_str().unwrap().to_owned();
    let lid = s.draft.config.profiles[0].layers[0]
        .id
        .as_str()
        .unwrap()
        .to_owned();
    s.update(|d| d.rename_profile(&pid, "Renamed")).unwrap();
    s.update(|d| d.rename_layer(&lid, "Renamed layer")).unwrap();
    s.update(|d| d.move_profile(&pid, 1)).unwrap();
    s.update(|d| d.move_layer(&pid, &lid, 1)).unwrap();
    let mut reopened = DocumentStore::default();
    reopened.open(t.file()).unwrap();
    let p = &reopened.draft.config.profiles[1];
    let l = &p.layers[1];
    assert_eq!(p.name, "Renamed");
    assert_eq!(p.id, pid);
    assert_eq!(l.name, "Renamed layer");
    assert_eq!(l.id, lid);
    assert!(reopened
        .draft
        .structure_changes
        .iter()
        .any(|v| v.contains("Renamed")));
    assert_eq!(
        reopened.draft.edits[&binding_key(&p.id, &l.id, 1)].id,
        "save"
    );
    assert!(reopened
        .update(|d| d.move_layer(&pid, "missing", 0))
        .is_err());
}
#[test]
fn restart_preserves_parameters_baseline_and_undo_save() {
    let t = Temp::new();
    let mut s = DocumentStore::default();
    s.open(t.file()).unwrap();
    let baseline = s.draft.config.raw.clone();
    s.update(|d| {
        let p = d.config.profiles[0].id.clone();
        let l = d.config.profiles[0].layers[0].id.clone();
        let mut a = actions()[2].clone();
        a.payload = json!({"type":"text","text":"自有提示\n第二行"});
        d.assign(&p, &l, 2, a)
    })
    .unwrap();
    let mut re = DocumentStore::default();
    re.open(t.file()).unwrap();
    assert_eq!(re.draft.edits, s.draft.edits);
    assert_eq!(re.draft.config.raw, baseline);
    assert!(!re.draft.can_undo);
    s.update(|d| d.undo(false)).unwrap();
    re.open(t.file()).unwrap();
    assert!(re.draft.edits.is_empty());
    s.update(|d| d.undo(true)).unwrap();
    re.open(t.file()).unwrap();
    assert_eq!(re.draft.edits.len(), 1);
}
#[test]
fn corrupt_future_and_unknown_files_are_never_overwritten() {
    let t = Temp::new();
    for bytes in [
        b"{broken".to_vec(),
        serde_json::to_vec(&json!({"schema_version":99})).unwrap(),
    ] {
        fs::write(t.file(), &bytes).unwrap();
        let mut s = DocumentStore::default();
        assert!(s.open(t.file()).is_err());
        assert!(s.update(assign).is_err());
        assert_eq!(fs::read(t.file()).unwrap(), bytes);
    }
    let mut d = LegacyDocumentV1::default();
    let p = d.config.profiles[0].id.clone();
    let l = d.config.profiles[0].layers[0].id.clone();
    d.assign(&p, &l, 1, actions()[1].clone()).unwrap();
    let mut raw = serde_json::to_value(&d).unwrap();
    raw["config"]["future"] = json!({"keep":true});
    let bytes = serde_json::to_vec(&raw).unwrap();
    fs::write(t.file(), &bytes).unwrap();
    let mut s = DocumentStore::default();
    s.open(t.file()).unwrap();
    assert!(s.draft.read_only);
    assert!(s.update(assign).is_err());
    assert_eq!(fs::read(t.file()).unwrap(), bytes);
    raw["config"].as_object_mut().unwrap().remove("future");
    for a in raw["edits"].as_object_mut().unwrap().values_mut() {
        a["payload"] = json!({"type":"future_action","private":"preserve"});
    }
    let bytes = serde_json::to_vec(&raw).unwrap();
    fs::write(t.file(), &bytes).unwrap();
    s.open(t.file()).unwrap();
    assert!(s.draft.read_only);
    assert!(s.update(assign).is_err());
    assert_eq!(fs::read(t.file()).unwrap(), bytes);
}
#[test]
fn conflicting_instances_and_failed_save_preserve_memory() {
    let t = Temp::new();
    let mut a = DocumentStore::default();
    let mut b = DocumentStore::default();
    a.open(t.file()).unwrap();
    b.open(t.file()).unwrap();
    a.update(assign).unwrap();
    let bytes = fs::read(t.file()).unwrap();
    assert!(b.update(assign).is_err());
    assert!(b.draft.edits.is_empty());
    assert_eq!(fs::read(t.file()).unwrap(), bytes);
    fs::write(t.file().with_extension("pending"), b"interrupted").unwrap();
    let before = a.draft.revision;
    assert!(a.update(|d| d.undo(false)).is_err());
    assert_eq!(a.draft.revision, before);
    assert_eq!(fs::read(t.file()).unwrap(), bytes);
}
#[test]
fn typed_actions_reject_invalid_parameters() {
    for a in actions() {
        a.validate().unwrap();
    }
    let mut a = actions()[3].clone();
    for url in ["javascript:alert(1)", "https://", "https://a b"] {
        a.payload = json!({"type":"open_url","url":url});
        assert!(a.validate().is_err());
    }
    a.payload = json!({"type":"text","text":""});
    assert!(a.validate().is_err());
    a.payload = json!({"type":"shortcut","key":"p","modifiers":["guess"],"target":"editor"});
    assert!(a.validate().is_err());
}

#[test]
fn legacy_edits_survive_first_structure_save_with_exact_backup() {
    let t = Temp::new();
    let mut config = crate::demo().unwrap();
    config.profiles[0].id = json!(7);
    config.profiles[1].id = json!("7");
    config.raw["future_large"] = json!(u64::MAX);
    let key = binding_key(&config.profiles[0].id, &config.profiles[0].layers[0].id, 1);
    let second_key = binding_key(&config.profiles[1].id, &config.profiles[1].layers[0].id, 2);
    let mut action = actions()[2].clone();
    action.payload = json!({"type":"text","text":"retained\nsecond line"});
    let mut shortcut = actions()[0].clone();
    shortcut.payload = json!({"type":"shortcut","key":"k","modifiers":["control","shift"],"target":"specific-editor"});
    let bytes = serde_json::to_vec_pretty(&json!({
        "schema_version":1,"config":config,"edits":{key:action,second_key:shortcut},"revision":7
    }))
    .unwrap();
    fs::write(t.file(), &bytes).unwrap();
    let mut store = DocumentStore::default();
    store.open(t.file()).unwrap();
    assert_eq!(
        fs::read(t.file()).unwrap(),
        bytes,
        "browsing does not migrate"
    );
    store
        .update(|d| d.create_profile("New profile").map(|_| ()))
        .unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(t.file()).unwrap()).unwrap();
    assert_eq!(saved["schema_version"], 2);
    assert_eq!(saved["baseline"], serde_json::Value::Null);
    assert_eq!(saved["revision"], 8);
    assert_eq!(
        saved["origin"]["config"]["raw"]["future_large"],
        json!(u64::MAX)
    );
    let backup = fs::read_dir(&t.0)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "backup"))
        .unwrap();
    assert_eq!(fs::read(backup).unwrap(), bytes);
    let mut reopened = DocumentStore::default();
    reopened.open(t.file()).unwrap();
    assert!(reopened
        .draft
        .edits
        .values()
        .any(|a| a.payload["text"] == "retained\nsecond line"));
    assert!(reopened.draft.edits.values().any(|a|a.payload==json!({"type":"shortcut","key":"k","modifiers":["control","shift"],"target":"specific-editor"})));
    assert_eq!(reopened.draft.config.profiles.len(), 3);
    assert!(!reopened.draft.can_undo);
}

#[test]
fn copied_profile_is_independent_and_one_undo_restores_its_exact_ids() {
    let mut d = Draft::default();
    assign(&mut d).unwrap();
    let original = d.config.profiles[0].id.as_str().unwrap().to_owned();
    let copied = d.duplicate_profile(&original, "Copy").unwrap();
    let copied_layer = d.config.profiles[2].layers[0].id.clone();
    assert_eq!(d.config.profiles.len(), 3);
    d.undo(false).unwrap();
    assert_eq!(d.config.profiles.len(), 2);
    d.undo(true).unwrap();
    assert_eq!(d.config.profiles[2].id, copied);
    assert_eq!(d.config.profiles[2].layers[0].id, copied_layer);
    let mut a = actions()[2].clone();
    a.payload = json!({"type":"text","text":"copy only"});
    d.assign(&json!(copied), &copied_layer, 1, a).unwrap();
    assert_eq!(
        d.edits[&binding_key(&json!(original), &d.config.profiles[0].layers[0].id, 1)].id,
        "save"
    );
    assert_eq!(
        d.edits[&binding_key(&json!(copied), &copied_layer, 1)].payload["text"],
        "copy only"
    );
}

#[test]
fn deleting_owned_nodes_keeps_shared_actions_and_can_reach_empty_document() {
    let mut d = Draft::default();
    assign(&mut d).unwrap();
    let pid = d.config.profiles[0].id.as_str().unwrap().to_owned();
    let first = d.config.profiles[0].layers[0]
        .id
        .as_str()
        .unwrap()
        .to_owned();
    let second = d.config.profiles[0].layers[1]
        .id
        .as_str()
        .unwrap()
        .to_owned();
    let source = d.document.origin.clone();
    let aid = d.document.editable.graph.layers[&first]
        .controls
        .values()
        .find_map(|b| b.action_id.clone())
        .unwrap();
    let target = d.document.editable.graph.layers.get_mut(&second).unwrap();
    target
        .controls
        .get_mut(&target.control_order[0])
        .unwrap()
        .action_id = Some(aid.clone());
    d.validate().unwrap();
    d.delete_layer(&pid, &first).unwrap();
    assert!(d.document.editable.graph.actions.contains_key(&aid));
    assert!(d.edits.values().any(|a| a.id == "save"));
    assert_eq!(d.document.origin, source);
    let ids = d
        .config
        .profiles
        .iter()
        .map(|p| p.id.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    for id in ids {
        d.delete_profile(&id).unwrap();
    }
    assert!(d.config.profiles.is_empty());
    assert!(d.document.editable.graph.actions.is_empty());
    d.undo(false).unwrap();
    assert_eq!(d.config.profiles.len(), 1);
    let mut bad = d.document.clone();
    let layer = bad.editable.graph.layers.values_mut().next().unwrap();
    layer.controls.values_mut().next().unwrap().action_id = Some(crate::document::new_id());
    assert!(bad.validate().is_err());
}

#[test]
fn independent_document_selection_survives_restart_without_replacing_previous_file() {
    let t = Temp::new();
    let mut store = DocumentStore::default();
    store.open(t.file()).unwrap();
    store.update(assign).unwrap();
    let old = fs::read(t.file()).unwrap();
    let mut config = crate::demo().unwrap();
    config.source = crate::Source::Imported;
    let baseline = json!({"device_identity":{"confidence":"synthetic"},"files":[{"bytes":[1,2,3],"sha1":"synthetic"}]});
    store.create_independent(config, baseline).unwrap();
    let new_id = store.draft.document_id.clone();
    store
        .update(|d| {
            d.rename_profile(
                &d.config.profiles[0].id.as_str().unwrap().to_owned(),
                "Selected draft",
            )
        })
        .unwrap();
    let mut re = DocumentStore::default();
    re.open(t.file()).unwrap();
    assert_eq!(re.draft.document_id, new_id);
    assert_eq!(re.draft.config.profiles[0].name, "Selected draft");
    assert_eq!(fs::read(t.file()).unwrap(), old);
    assert!(!re.draft.can_undo);
    assert_eq!(
        re.draft.document.baseline.as_ref().unwrap()["files"][0]["bytes"],
        json!([1, 2, 3])
    );
}

#[test]
fn failed_selection_and_migration_backup_preserve_files_and_memory() {
    let t = Temp::new();
    let mut store = DocumentStore::default();
    store.open(t.file()).unwrap();
    store.update(assign).unwrap();
    let old = fs::read(t.file()).unwrap();
    let id = store.draft.document_id.clone();
    let revision = store.draft.revision;
    fs::write(t.file().with_extension("active.pending"), b"interrupted").unwrap();
    assert!(store
        .create_independent(crate::demo().unwrap(), json!({"source":"synthetic"}))
        .is_err());
    assert_eq!(store.draft.document_id, id);
    assert_eq!(store.draft.revision, revision);
    assert_eq!(fs::read(t.file()).unwrap(), old);
    assert_eq!(
        fs::read_dir(&t.0)
            .unwrap()
            .filter(|e| e
                .as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("draft-"))
            .count(),
        1
    );
    let bytes = serde_json::to_vec(&LegacyDocumentV1::default()).unwrap();
    fs::write(t.file(), &bytes).unwrap();
    store.open(t.file()).unwrap();
    use sha2::{Digest, Sha256};
    let hash = format!("{:x}", Sha256::digest(&bytes));
    fs::write(
        t.0.join(format!("workbench.json.v1.{hash}.backup")),
        b"different",
    )
    .unwrap();
    let id = store.draft.document_id.clone();
    assert!(store
        .update(|d| d.create_profile("Failed").map(|_| ()))
        .is_err());
    assert_eq!(store.draft.document_id, id);
    assert_eq!(store.draft.revision, 0);
    assert_eq!(fs::read(t.file()).unwrap(), bytes);
}

#[test]
fn future_v2_fields_and_revision_exhaustion_never_replace_the_file() {
    let t = Temp::new();
    let base = serde_json::to_value(&Draft::default().document).unwrap();
    for kind in ["envelope", "graph", "profile", "action", "revision"] {
        let mut raw = base.clone();
        match kind {
            "envelope" => raw["future"] = json!({"keep":true}),
            "graph" => raw["graph"]["future"] = json!(17),
            "profile" => {
                let id = raw["graph"]["profile_order"][0]
                    .as_str()
                    .unwrap()
                    .to_owned();
                raw["graph"]["profiles"][id]["future"] = json!([1, 2]);
            }
            "action" => {
                let id = crate::document::new_id();
                raw["graph"]["actions"][id] = json!({"id":"future","name":"Future","category":"unknown","detail":"keep","payload":{"type":"future"}});
            }
            "revision" => raw["revision"] = json!(u64::MAX),
            _ => unreachable!(),
        }
        let bytes = serde_json::to_vec(&raw).unwrap();
        fs::write(t.file(), &bytes).unwrap();
        let mut store = DocumentStore::default();
        store.open(t.file()).unwrap();
        if kind != "revision" {
            assert!(store.draft.read_only);
        }
        assert!(store
            .update(|d| d.create_profile("Must fail").map(|_| ()))
            .is_err());
        assert_eq!(fs::read(t.file()).unwrap(), bytes);
    }
}

#[test]
fn no_op_keeps_redo_and_history_has_a_bounded_number_of_transactions() {
    let mut d = Draft::default();
    let pid = d.config.profiles[0].id.as_str().unwrap().to_owned();
    for n in 0..101 {
        d.rename_profile(&pid, &format!("Name {n}")).unwrap();
    }
    let rev = d.revision;
    for _ in 0..100 {
        d.undo(false).unwrap();
    }
    assert_eq!(d.config.profiles[0].name, "Name 0");
    assert!(!d.can_undo);
    assert!(d.can_redo);
    d.rename_profile(&pid, "Name 0").unwrap();
    assert_eq!(d.revision, rev + 100);
    assert!(d.can_redo);
    d.undo(true).unwrap();
    assert_eq!(d.config.profiles[0].name, "Name 1");
}

#[test]
fn stale_instance_cannot_replace_another_instances_document_selection() {
    let t = Temp::new();
    let mut first = DocumentStore::default();
    let mut stale = DocumentStore::default();
    first.open(t.file()).unwrap();
    stale.open(t.file()).unwrap();
    first
        .create_independent(crate::demo().unwrap(), json!({"synthetic":true}))
        .unwrap();
    let selected = fs::read(t.file().with_extension("active.json")).unwrap();
    let before = stale.draft.document_id.clone();
    assert!(stale
        .create_independent(crate::demo().unwrap(), json!({"synthetic":true}))
        .is_err());
    assert_eq!(stale.draft.document_id, before);
    assert_eq!(
        fs::read(t.file().with_extension("active.json")).unwrap(),
        selected
    );
    let mut reopened = DocumentStore::default();
    reopened.open(t.file()).unwrap();
    assert_eq!(reopened.draft.document_id, first.draft.document_id);
}

#[test]
fn legacy_device_source_is_historical_and_never_claims_a_current_snapshot() {
    let mut legacy = LegacyDocumentV1::default();
    legacy.config.source = crate::Source::DeviceSnapshot;
    let bytes = serde_json::to_vec(&legacy).unwrap();
    let mut d = Draft::from_bytes(&bytes).unwrap();
    assert_eq!(d.config.source, crate::Source::LocalDraft);
    assert!(d.document.baseline.is_none());
    assert_eq!(d.document.origin["config"]["source"], "DeviceSnapshot");
    let pid = d.config.profiles[0].id.as_str().unwrap().to_owned();
    d.rename_profile(&pid, "Local name").unwrap();
    assert_eq!(d.config.source, crate::Source::LocalDraft);
}
