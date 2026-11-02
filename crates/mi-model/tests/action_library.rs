use mi_model::{action_library, draft::Draft};
use serde_json::json;
use std::collections::BTreeSet;

#[test]
fn library_explains_local_app_intent_and_rust_validates_assignment() {
    let entries = action_library::search("应用", "全部", false, &BTreeSet::new());
    let app = entries.iter().find(|e| e.action.id == "app").unwrap();
    assert_eq!(
        app.action.payload,
        json!({"type":"open_app","bundle_id":"com.apple.TextEdit"})
    );
    assert!(app.capability.host_required);
    assert!(!app.capability.execution_available);
    assert_eq!(app.capability.target, "com.apple.TextEdit");
    assert!(app.capability.unavailable_reason.contains("未接入"));
    let mut draft = Draft::default();
    let p = draft.config.profiles[0].id.clone();
    let l = draft.config.profiles[0].layers[0].id.clone();
    draft.assign(&p, &l, 0, app.action.clone()).unwrap();
    let revision = draft.revision;
    let mut invalid = app.action.clone();
    invalid.payload["bundle_id"] = json!("not an app");
    assert!(draft.assign(&p, &l, 0, invalid).is_err());
    assert_eq!(draft.revision, revision);
    draft.undo(false).unwrap();
    assert!(draft.edits.is_empty());
}
