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
