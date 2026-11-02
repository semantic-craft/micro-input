use mi_model::draft::{binding_key, Action, Draft};
use serde_json::{json, Value};

fn action(events: Value) -> Action {
    Action {
        id: "local-macro".into(),
        name: "Local sequence".into(),
        category: "普通宏".into(),
        detail: "自有本地格式 · 不执行".into(),
        payload: json!({"type":"local_macro","macro_sequence":{"format_version":1,"events":events}}),
    }
}
#[test]
fn local_press_delay_release_round_trips_with_whole_graph_undo() {
    let mut draft = Draft::default();
    let p = draft.config.profiles[0].id.clone();
    let l = draft.config.profiles[0].layers[0].id.clone();
    let original = draft.document.origin.clone();
    let before = draft.document.editable.clone();
    let a = action(
        json!([{"type":"press","key":"Command"},{"type":"press","key":"a"},{"type":"delay","ms":125},{"type":"release","key":"a"},{"type":"release","key":"Command"}]),
    );
    draft.assign(&p, &l, 2, a.clone()).unwrap();
    let after = draft.document.editable.clone();
    let bytes = serde_json::to_vec(&draft.document).unwrap();
    let reopened = Draft::from_bytes(&bytes).unwrap();
    assert!(!reopened.read_only);
    assert_eq!(reopened.edits[&binding_key(&p, &l, 2)], a);
    assert_eq!(reopened.document.origin, original);
    draft.undo(false).unwrap();
    assert!(draft.document.editable == before);
    draft.undo(true).unwrap();
    assert!(draft.document.editable == after);
}

#[test]
fn invalid_macro_reports_the_held_press_step() {
    let invalid = action(json!([{"type":"press","key":"a"},{"type":"delay","ms":1}]));
    assert!(invalid.validate().unwrap_err().contains("第 1 步"));
}

#[test]
fn invalid_or_future_macro_never_replaces_the_saved_sequence_or_source() {
    use mi_model::storage::DocumentStore;
    let directory = std::env::temp_dir().join(format!("mi-macro-{}", mi_model::document::new_id()));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("workbench.json");
    let mut store = DocumentStore::default();
    store.open(path.clone()).unwrap();
    let p = store.draft.config.profiles[0].id.clone();
    let l = store.draft.config.profiles[0].layers[0].id.clone();
    let valid = action(json!([{"type":"press","key":"a"},{"type":"release","key":"a"}]));
    store
        .update(|d| d.assign(&p, &l, 0, valid.clone()))
        .unwrap();
    let saved = std::fs::read(&path).unwrap();
    for events in [
        json!([]),
        json!([{"type":"release","key":"a"}]),
        json!([{"type":"press","key":"a"},{"type":"press","key":"a"}]),
        json!([{"type":"delay","ms":0}]),
        json!([{"type":"delay","ms":-1}]),
        json!([{"type":"delay","ms":4294967296u64}]),
        json!([{"type":"delay","ms":1,"future":true}]),
    ] {
        assert!(store
            .update(|d| d.assign(&p, &l, 0, action(events)))
            .is_err());
        assert_eq!(store.draft.revision, 1);
        assert_eq!(std::fs::read(&path).unwrap(), saved);
    }
    let mut future: Value = serde_json::from_slice(&saved).unwrap();
    let aid = future["graph"]["actions"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    future["graph"]["actions"][&aid]["payload"]["macro_sequence"]["format_version"] = json!(2);
    let future_bytes = serde_json::to_vec(&future).unwrap();
    std::fs::write(&path, &future_bytes).unwrap();
    store.open(path.clone()).unwrap();
    assert!(store.draft.read_only);
    assert!(store.update(|d| d.assign(&p, &l, 0, valid)).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), future_bytes);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn shared_macro_references_survive_restart_and_guard_deletion() {
    let mut draft = Draft::default();
    let p = draft.config.profiles[0].id.clone();
    let l = draft.config.profiles[0].layers[0].id.clone();
    let id = draft
        .save_macro(
            None,
            action(json!([{"type":"press","key":"a"},{"type":"release","key":"a"}])),
        )
        .unwrap();
    draft.bind_macro(&p, &l, 0, Some(&id)).unwrap();
    draft.bind_macro(&p, &l, 1, Some(&id)).unwrap();
    assert_eq!(draft.macros()[0].references.len(), 2);
    assert!(draft.delete_macro(&id).unwrap_err().contains("logical-0"));
    let revision = draft.revision;
    let edited = action(json!([{"type":"press","key":"b"},{"type":"release","key":"b"}]));
    draft.save_macro(Some(&id), edited.clone()).unwrap();
    assert_eq!(draft.revision, revision + 1);
    assert_eq!(draft.edits[&binding_key(&p, &l, 0)], edited);
    assert_eq!(draft.edits[&binding_key(&p, &l, 1)], edited);
    let mut re = Draft::from_bytes(&serde_json::to_vec(&draft.document).unwrap()).unwrap();
    assert_eq!(re.macros()[0].id, id);
    assert_eq!(re.macros()[0].references.len(), 2);
    re.bind_macro(&p, &l, 0, None).unwrap();
    re.bind_macro(&p, &l, 1, None).unwrap();
    re.delete_macro(&id).unwrap();
    assert!(re.macros().is_empty());
    re.undo(false).unwrap();
    assert_eq!(re.macros()[0].id, id);
    re.undo(false).unwrap();
    assert_eq!(re.macros()[0].references.len(), 1);
}
