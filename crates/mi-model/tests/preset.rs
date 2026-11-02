use mi_model::{
    document::new_id,
    draft::{Action, Draft},
    preset,
    storage::DocumentStore,
    Source,
};
use serde_json::{json, Value};
use std::{collections::BTreeSet, fs};

fn fixture() -> Draft {
    let mut d = Draft::default();
    let p = d.config.profiles[0].id.clone();
    let l = d.config.profiles[0].layers[0].id.clone();
    let macro_id = new_id();
    let multi_id = new_id();
    let tool_id = new_id();
    let g = &mut d.document.editable.graph;
    let macro_action = mi_model::draft::actions()
        .into_iter()
        .find(|a| a.id == "local-macro")
        .unwrap();
    let tool = mi_model::draft::actions()
        .into_iter()
        .find(|a| a.id == "session")
        .unwrap();
    g.actions.insert(macro_id.clone(), macro_action);
    g.actions.insert(tool_id.clone(), tool);
    g.actions.insert(multi_id.clone(),Action {id:"local-multi".into(), name:"共享四分支".into(),category:"Multi".into(),detail:"本地".into(), payload:json!({"type":"local_multi","multi":{"format_version":1,"term_ms":250,"tap":{"type":"macro","action_id":macro_id},"hold":{"type":"action","action_id":tool_id},"double_tap":{"type":"macro","action_id":macro_id},"tap_hold":null}})});
    let layer = g.layers.get_mut(l.as_str().unwrap()).unwrap();
    for slot in [0, 1] {
        layer
            .controls
            .get_mut(&layer.control_order[slot])
            .unwrap()
            .action_id = Some(multi_id.clone());
    }
    d.document.origin["config"]["raw"]["future_metadata"] =
        json!({"counter":u64::MAX,"color":"未知"});
    d.document.baseline = Some(json!({"private_device_fact":"historical-only"}));
    d.document.validate().unwrap();
    let mut d = Draft::from_document(d.document, 2, false).unwrap();
    d.assign(&p, &l, 2, mi_model::draft::actions()[0].clone())
        .unwrap();
    d
}
fn ids(d: &Draft) -> BTreeSet<String> {
    let g = &d.document.editable.graph;
    g.profiles
        .keys()
        .chain(g.layers.keys())
        .chain(g.actions.keys())
        .cloned()
        .chain(g.layers.values().flat_map(|l| l.controls.keys().cloned()))
        .collect()
}
#[test]
fn round_trip_remaps_shared_multi_macro_and_tool_references_preserving_raw_and_metadata() {
    let d = fixture();
    let exported = preset::export(&d, None, None).unwrap();
    let imported = preset::import(&exported).unwrap();
    imported.document.validate().unwrap();
    assert!(ids(&d).is_disjoint(&ids(&imported)));
    assert_ne!(d.document_id, imported.document_id);
    assert!(imported.document.baseline.is_none());
    assert!(matches!(imported.config.source, Source::LocalDraft));
    assert_eq!(
        imported.document.origin["imported_baseline"],
        d.document.baseline.clone().unwrap()
    );
    assert_eq!(
        imported.document.origin["config"],
        d.document.origin["config"]
    );
    let p = &imported.document.editable.graph.profile_order[0];
    let g = &imported.document.editable.graph;
    let l = &g.layers[&g.profiles[p].layer_order[0]];
    let first = l.controls[&l.control_order[0]].action_id.as_ref().unwrap();
    assert_eq!(
        Some(first),
        l.controls[&l.control_order[1]].action_id.as_ref()
    );
    let m = mi_model::multi::local_multi(&g.actions[first]).unwrap();
    assert_eq!(
        m.tap.as_ref().unwrap().id(),
        m.double_tap.as_ref().unwrap().id()
    );
    assert_eq!(
        g.actions[m.tap.unwrap().id()].payload["type"],
        "local_macro"
    );
    assert_eq!(g.actions[m.hold.unwrap().id()].payload["adapter"], "dsh");
    assert_eq!(imported.revision, 0);
    assert!(!imported.can_undo);
    let twice = preset::import(&preset::export(&imported, None, None).unwrap()).unwrap();
    assert_eq!(twice.config.raw, imported.config.raw);
}
#[test]
fn scoped_exports_keep_transitive_dependencies_and_only_selected_editable_nodes() {
    let mut d = fixture();
    let unused = new_id();
    d.document
        .editable
        .graph
        .actions
        .insert(unused.clone(), mi_model::draft::actions()[3].clone());
    let p = d.document.editable.graph.profile_order[0].clone();
    let l = d.document.editable.graph.profiles[&p].layer_order[0].clone();
    for layer in [None, Some(l.as_str())] {
        let text = preset::export(&d, Some(&p), layer).unwrap();
        let parsed = preset::parse(&text).unwrap();
        assert!(parsed.document["graph"]["actions"].get(&unused).is_none());
        let i = preset::import(&text).unwrap();
        assert_eq!(i.document.editable.graph.profiles.len(), 1);
        assert_eq!(
            i.document.editable.graph.layers.len(),
            if layer.is_some() { 1 } else { 2 }
        );
        assert_eq!(i.document.editable.graph.actions.len(), 4);
        assert_eq!(i.document.origin["config"], d.document.origin["config"]);
    }
    assert!(preset::export(&d, None, Some(&l)).is_err());
}
#[test]
fn invalid_versions_unknown_fields_and_dangling_references_are_rejected() {
    let text = preset::export(&fixture(), None, None).unwrap();
    let raw: Value = serde_json::from_str(&text).unwrap();
    let mut bad = raw.clone();
    bad["version"] = json!(2);
    assert!(preset::import(&bad.to_string()).is_err());
    let mut bad = raw.clone();
    bad["document"]["future"] = json!("keep me");
    assert!(preset::import(&bad.to_string()).is_err());
    let mut bad = raw.clone();
    bad["document"]["graph"]["actions"] = json!({});
    assert!(preset::import(&bad.to_string()).is_err());
    assert!(preset::import(&" ".repeat(preset::LIMIT + 1)).is_err());
    assert!(preset::import("{broken").is_err());
}
#[test]
fn import_is_independent_persistent_and_conflicts_preserve_existing_selection() {
    let dir = std::env::temp_dir().join(format!("mi-preset-{}", new_id()));
    fs::create_dir(&dir).unwrap();
    let path = dir.join("workbench.json");
    fs::write(&path, serde_json::to_vec(&fixture().document).unwrap()).unwrap();
    let original = fs::read(&path).unwrap();
    let mut store = DocumentStore::default();
    store.open(path.clone()).unwrap();
    let mut stale = DocumentStore::default();
    stale.open(path.clone()).unwrap();
    let text = preset::starter().unwrap();
    assert_eq!(preset::parse(&text).unwrap().name, "Coding 起步");
    store.import_preset(&text).unwrap();
    assert_eq!(fs::read(&path).unwrap(), original);
    let current = store.draft.document_id.clone();
    let mut reopened = DocumentStore::default();
    reopened.open(path.clone()).unwrap();
    assert_eq!(reopened.draft.document_id, current);
    assert_eq!(reopened.draft.config.profiles[0].name, "Coding 起步");
    let selection = fs::read(path.with_extension("active.json")).unwrap();
    assert!(stale.import_preset(&text).is_err());
    assert!(store.import_preset("{}").is_err());
    assert_eq!(
        fs::read(path.with_extension("active.json")).unwrap(),
        selection
    );
    assert_eq!(store.draft.document_id, current);
    store.import_preset(&text).unwrap();
    assert_ne!(store.draft.document_id, current);
    assert_eq!(store.draft.config.profiles[0].name, "Coding 起步");
    assert_eq!(fs::read(&path).unwrap(), original);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn source_archive_is_not_duplicated_on_import() {
    let mut d = Draft::default();
    d.document.origin["config"]["raw"]["large_unknown"] = json!("x".repeat(2_200_000));
    let text = preset::export(&d, None, None).unwrap();
    let imported = preset::import(&text).unwrap();
    let saved = serde_json::to_vec_pretty(&imported.document).unwrap();
    assert!(saved.len() <= preset::LIMIT);
    assert_eq!(
        imported.document.origin["config"],
        d.document.origin["config"]
    );
    assert!(imported.document.origin["imported_origin"]
        .get("config")
        .is_none());
}

#[test]
fn starter_contains_only_original_local_content() {
    let d = preset::import(&preset::starter().unwrap()).unwrap();
    assert_eq!(d.document.editable.graph.actions.len(), 2);
    let config = &d.document.origin["config"];
    assert_eq!(config["macros"], json!([]));
    assert_eq!(config["multi_actions"], json!([]));
    assert_eq!(config["warnings"], json!([]));
    assert_eq!(config["read_at"], 0);
    assert!(config["firmware"].is_null());
    assert!(config["smart_actions"].is_null());
}
