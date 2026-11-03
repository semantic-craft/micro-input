use mi_model::draft::{Action, Draft};
use serde_json::json;
use std::collections::BTreeMap;
fn multi(target: &str) -> Action {
    Action {
        id: "local-multi".into(),
        name: "Editing gestures".into(),
        category: "Multi".into(),
        detail: "local only".into(),
        payload: json!({"type":"local_multi","multi":{"format_version":1,"term_ms":300,"tap":{"type":"macro","action_id":target},"double_tap":null,"hold":null,"tap_hold":null}}),
    }
}
#[test]
fn multi_shared_macro_branch_roundtrip_undo_and_delete_protection() {
    let mut d = Draft::default();
    let p = d.config.profiles[0].id.clone();
    let l = d.config.profiles[0].layers[0].id.clone();
    let raw = d.document.origin.clone();
    let macro_action = mi_model::draft::actions()
        .into_iter()
        .find(|a| a.id == "local-macro")
        .unwrap();
    let mid = d.save_macro(None, macro_action).unwrap();
    let before = d.document.editable.clone();
    let id = d
        .save_multi_for_binding(None, multi(&mid), BTreeMap::new(), &p, &l, 0)
        .unwrap();
    assert!(d
        .delete_macro(&mid)
        .unwrap_err()
        .contains("Editing gestures / 单击"));
    assert_eq!(
        d.macros()[0].references[0].parent_action_name.as_deref(),
        Some("Editing gestures")
    );
    let after = d.document.editable.clone();
    let mut re = Draft::from_bytes(&serde_json::to_vec(&d.document).unwrap()).unwrap();
    assert!(!re.read_only);
    assert_eq!(re.multis()[0].id, id);
    assert_eq!(re.document.origin, raw);
    d.undo(false).unwrap();
    assert!(d.document.editable == before);
    d.undo(true).unwrap();
    assert!(d.document.editable == after);
    let mut empty = multi(&mid);
    empty.payload["multi"]["tap"] = json!(null);
    re.save_multi_for_binding(Some(&id), empty, BTreeMap::new(), &p, &l, 0)
        .unwrap();
    re.delete_macro(&mid).unwrap();
}

#[test]
fn invalid_multi_references_and_cycles_leave_document_and_disk_unchanged() {
    use mi_model::storage::DocumentStore;
    let dir = std::env::temp_dir().join(format!("mi-multi-{}", mi_model::document::new_id()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("workbench.json");
    let mut store = DocumentStore::default();
    store.open(path.clone()).unwrap();
    let p = store.draft.config.profiles[0].id.clone();
    let l = store.draft.config.profiles[0].layers[0].id.clone();
    let macro_action = mi_model::draft::actions()
        .into_iter()
        .find(|a| a.id == "local-macro")
        .unwrap();
    let mut mid = String::new();
    store
        .update(|d| {
            mid = d.save_macro(None, macro_action)?;
            Ok(())
        })
        .unwrap();
    let mut id = String::new();
    store
        .update(|d| {
            id = d.save_multi_for_binding(None, multi(&mid), BTreeMap::new(), &p, &l, 0)?;
            Ok(())
        })
        .unwrap();
    let saved = std::fs::read(&path).unwrap();
    let revision = store.draft.revision;
    let mut cycle = multi(&mid);
    cycle.payload["multi"]["hold"] = json!({"type":"action","action_id":id});
    let mut mismatch = multi(&id);
    mismatch.payload["multi"]["term_ms"] = json!(0);
    for a in [
        multi(&mi_model::document::new_id()),
        multi(&id),
        cycle,
        mismatch,
    ] {
        assert!(store
            .update(|d| d
                .save_multi_for_binding(Some(&id), a, BTreeMap::new(), &p, &l, 0)
                .map(|_| ()))
            .is_err());
        assert_eq!(store.draft.revision, revision);
        assert_eq!(std::fs::read(&path).unwrap(), saved);
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn four_branches_reuse_other_actions_atomically_and_replacing_control_preserves_references() {
    let mut d = Draft::default();
    let p = d.config.profiles[0].id.clone();
    let l = d.config.profiles[0].layers[0].id.clone();
    let a = mi_model::draft::actions()
        .into_iter()
        .find(|a| a.id == "save")
        .unwrap();
    d.assign(&p, &l, 1, a.clone()).unwrap();
    let aid = d
        .document
        .editable
        .graph
        .actions
        .keys()
        .next()
        .unwrap()
        .clone();
    let mut m = multi(&aid);
    for branch in ["tap", "double_tap", "hold", "tap_hold"] {
        m.payload["multi"][branch] = json!({"type":"action","action_id":aid});
    }
    let id = d
        .save_multi_for_binding(None, m, BTreeMap::new(), &p, &l, 0)
        .unwrap();
    d.assign(&p, &l, 1, mi_model::draft::actions()[0].clone())
        .unwrap();
    assert!(d.document.editable.graph.actions.contains_key(&aid));
    d.bind_multi(&p, &l, 2, Some(&id)).unwrap();
    assert_eq!(d.multis()[0].references.len(), 2);
    assert!(d.delete_multi(&id).is_err());
    let re = Draft::from_bytes(&serde_json::to_vec(&d.document).unwrap()).unwrap();
    assert!(!re.read_only);
    assert_eq!(
        re.document.editable.graph.actions[&id].payload["multi"]["tap_hold"]["action_id"],
        aid
    );
}

#[test]
fn deleting_a_layer_keeps_shared_multi_and_its_other_action_children() {
    let mut d = Draft::default();
    let p = d.config.profiles[0].id.clone();
    let l = d.config.profiles[0].layers[0].id.clone();
    let aid = mi_model::document::new_id();
    let a = mi_model::draft::actions()[0].clone();
    let mut m = multi(&aid);
    m.payload["multi"]["tap"]["type"] = json!("action");
    let id = d
        .save_multi_for_binding(None, m, BTreeMap::from([(aid.clone(), a)]), &p, &l, 0)
        .unwrap();
    d.delete_layer(p.as_str().unwrap(), l.as_str().unwrap())
        .unwrap();
    assert_eq!(d.multis().len(), 1);
    assert!(d.document.editable.graph.actions.contains_key(&id));
    assert!(d.document.editable.graph.actions.contains_key(&aid));
}

#[test]
fn editing_a_shared_multi_from_library_does_not_rebind_the_selected_control() {
    let mut d = Draft::default();
    let p = d.config.profiles[0].id.clone();
    let l = d.config.profiles[0].layers[0].id.clone();
    let mid = d
        .save_macro(
            None,
            mi_model::draft::actions()
                .into_iter()
                .find(|a| a.id == "local-macro")
                .unwrap(),
        )
        .unwrap();
    let id = d
        .save_multi_for_binding(None, multi(&mid), BTreeMap::new(), &p, &l, 0)
        .unwrap();
    let mut changed = multi(&mid);
    changed.name = "Shared edit".into();
    d.save_multi(&id, changed, BTreeMap::new()).unwrap();
    assert_eq!(d.multis()[0].references.len(), 1);
    assert_eq!(d.multis()[0].action.name, "Shared edit");
}

#[test]
fn future_multi_format_and_unknown_original_bindings_stay_read_only_and_preserved() {
    let mut d = Draft::default();
    let p = d.config.profiles[0].id.clone();
    let l = d.config.profiles[0].layers[0].id.clone();
    let mid = d
        .save_macro(
            None,
            mi_model::draft::actions()
                .into_iter()
                .find(|a| a.id == "local-macro")
                .unwrap(),
        )
        .unwrap();
    let id = d
        .save_multi_for_binding(None, multi(&mid), BTreeMap::new(), &p, &l, 0)
        .unwrap();
    let original = d.document.origin.clone();
    let mut raw = serde_json::to_value(&d.document).unwrap();
    raw["graph"]["actions"][&id]["payload"]["multi"]["format_version"] = json!(2);
    let mut re = Draft::from_bytes(&serde_json::to_vec(&raw).unwrap()).unwrap();
    assert!(re.read_only);
    assert_eq!(re.document.origin, original);
    assert!(re.bind_multi(&p, &l, 1, Some(&id)).is_err());
    assert_eq!(serde_json::to_value(&re.document).unwrap(), raw);
}
