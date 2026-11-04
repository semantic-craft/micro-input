use mi_model::storage::DocumentStore;
use mi_model::{action_library, draft::Draft};
use serde_json::json;
use std::collections::BTreeSet;

#[test]
fn favorite_is_a_host_preference_and_survives_restart_without_editing_the_draft() {
    let directory = std::env::temp_dir().join(format!("mi-library-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("workbench.json");
    let mut store = DocumentStore::default();
    store.open(path.clone()).unwrap();
    let before = serde_json::to_value(&store.draft).unwrap();
    store.set_action_favorite("app", true).unwrap();
    assert_eq!(serde_json::to_value(&store.draft).unwrap(), before);
    assert!(!path.exists());
    let mut reopened = DocumentStore::default();
    reopened.open(path.clone()).unwrap();
    let favorites = reopened.favorite_actions().unwrap();
    let found = action_library::search("TEXTEDIT", "应用", true, favorites, false);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].action.id, "app");
    assert!(action_library::search("", "文本", true, favorites, false).is_empty());
    reopened.set_action_favorite("app", false).unwrap();
    assert!(store.set_action_favorite("url", true).is_err());
    assert!(store.favorite_actions().unwrap().contains("app"));
    assert!(reopened.favorite_actions().unwrap().is_empty());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn corrupt_or_incomplete_preferences_never_overwrite_files_or_disable_draft_editing() {
    let directory = std::env::temp_dir().join(format!("mi-library-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("workbench.json");
    let preferences = directory.join("workbench.preferences.json");
    for original in [
        b"{broken".to_vec(),
        br#"{"schema_version":2,"favorites":["app"]}"#.to_vec(),
        br#"{"schema_version":1,"favorites":[],"future":true}"#.to_vec(),
    ] {
        std::fs::write(&preferences, &original).unwrap();
        let mut store = DocumentStore::default();
        store.open(path.clone()).unwrap();
        assert!(store.favorite_actions().is_err());
        assert!(store.set_action_favorite("app", true).is_err());
        assert_eq!(std::fs::read(&preferences).unwrap(), original);
        store
            .update(|d| {
                let id = d.config.profiles[0].id.as_str().unwrap().to_owned();
                d.rename_profile(&id, "Local editing still works")
            })
            .unwrap();
    }
    std::fs::remove_file(&preferences).unwrap();
    std::fs::write(
        directory.join("workbench.preferences.pending"),
        b"retain incomplete save",
    )
    .unwrap();
    let mut store = DocumentStore::default();
    store.open(path).unwrap();
    assert!(store.set_action_favorite("app", true).is_err());
    assert!(store.favorite_actions().unwrap().is_empty());
    assert!(!preferences.exists());
    assert_eq!(
        std::fs::read(directory.join("workbench.preferences.pending")).unwrap(),
        b"retain incomplete save"
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn library_explains_local_app_intent_and_rust_validates_assignment() {
    let entries = action_library::search("", "全部", false, &BTreeSet::new(), false);
    let app = entries.iter().find(|e| e.action.id == "app").unwrap();
    assert_eq!(
        app.action.payload,
        json!({"type":"open_app","bundle_id":"com.apple.TextEdit"})
    );
    assert!(app.capability.host_required);
    assert!(app.capability.execution_available);
    assert_eq!(app.capability.target, "com.apple.TextEdit");
    assert!(app.capability.unavailable_reason.is_empty());
    let skill = entries.iter().find(|e| e.action.id == "skill").unwrap();
    assert!(skill.capability.host_required);
    assert!(!skill.capability.execution_available);
    assert!(skill.capability.unavailable_reason.contains("未接入"));
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

#[test]
fn shell_command_capability_hides_execution_until_consent_is_enabled() {
    let entries = action_library::search("", "命令", false, &BTreeSet::new(), false);
    let command = entries
        .iter()
        .find(|entry| entry.action.id == "shell-command")
        .unwrap();
    assert!(!command.capability.execution_available);
    assert!(command.capability.unavailable_reason.contains("同意"));
    assert_eq!(command.capability.target, "/tmp");

    let enabled = action_library::search("", "命令", false, &BTreeSet::new(), true);
    let command = enabled
        .iter()
        .find(|entry| entry.action.id == "shell-command")
        .unwrap();
    assert!(command.capability.execution_available);
    assert!(command.capability.unavailable_reason.is_empty());
    assert!(command.capability.result_semantics.contains("退出状态"));
}

#[test]
fn cross_agent_navigation_actions_are_executable_and_assignable_to_any_slot() {
    let entries = action_library::search("", "导航", false, &BTreeSet::new(), false);
    assert_eq!(entries.len(), 2);
    let back = entries
        .iter()
        .find(|entry| entry.action.id == "navigate-back")
        .unwrap();
    let forward = entries
        .iter()
        .find(|entry| entry.action.id == "navigate-forward")
        .unwrap();
    for entry in [back, forward] {
        assert!(entry.capability.host_required);
        assert!(entry.capability.execution_available);
        assert!(entry.capability.unavailable_reason.is_empty());
    }

    let mut draft = Draft::default();
    let p = draft.config.profiles[0].id.clone();
    let l = draft.config.profiles[0].layers[0].id.clone();
    draft.assign(&p, &l, 0, back.action.clone()).unwrap();
    draft.assign(&p, &l, 1, forward.action.clone()).unwrap();
}

#[test]
fn prompt_slots_and_submit_control_actions_report_paste_only_vs_explicit_submit_semantics() {
    let text_entries = action_library::search("工程指令", "文本", false, &BTreeSet::new(), false);
    assert_eq!(text_entries.len(), 5);
    for entry in &text_entries {
        assert!(entry.capability.host_required);
        assert!(entry.capability.execution_available);
        assert!(entry
            .capability
            .result_semantics
            .contains("no_newline=true"));
        assert!(entry.capability.result_semantics.contains("不自动回车"));
    }

    let submit_entries = action_library::search("", "提交控制", false, &BTreeSet::new(), false);
    assert_eq!(submit_entries.len(), 2);
    for entry in &submit_entries {
        assert!(entry.capability.host_required);
        assert!(entry.capability.execution_available);
        assert!(entry.capability.unavailable_reason.is_empty());
        assert!(entry.capability.result_semantics.contains("先停"));
    }
}
