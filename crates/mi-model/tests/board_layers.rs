use mi_model::{
    board_layers::{
        BoardLayer, BoardLayerPlan, BoardLayerRoute, BoardPreviewSelection, NativeLayerOwner,
    },
    storage::DocumentStore,
};
use serde_json::{json, Value};
use tempfile::TempDir;

#[test]
fn board_numbers_and_led_patterns_are_strict() {
    let patterns = [
        [true, false, false],
        [false, true, false],
        [false, false, true],
        [true, true, false],
        [false, true, true],
        [true, true, true],
    ];
    for (i, layer) in BoardLayer::ALL.into_iter().enumerate() {
        assert_eq!(layer.number() as usize, i + 1);
        assert_eq!(layer.offset(), i);
        assert_eq!(layer.leds(), patterns[i]);
        assert_eq!(
            serde_json::from_value::<BoardLayer>(json!(i + 1)).unwrap(),
            layer
        );
    }
    for bad in [
        json!(0),
        json!(7),
        json!(255),
        json!(-1),
        json!(1.5),
        json!("1"),
    ] {
        assert!(serde_json::from_value::<BoardLayer>(bad).is_err());
    }
    assert!(serde_json::from_value::<BoardLayerPlan>(json!({"layers": []})).is_err());
    assert!(serde_json::from_value::<BoardLayerRoute>(
        json!({"kind":"Native", "owner":"Other", "profile_id":"x"})
    )
    .is_err());
}

fn preferences() -> Value {
    json!({"schema_version":1, "favorites":["custom-favorite"], "host_active_profile_id":"existing-active",
        "host_execution_enabled":true, "command_execution_enabled":true, "auto_follow_enabled":true,
        "manual_override":true, "bottom_row_layout":"merged", "app_profile_links":{"test.app":"missing"},
        "hardware_profile_slots":["usb-id","third-id",null,"first-id"], "active_hardware_slot":3})
}

fn open_fixture(temp: &TempDir, value: &Value) -> (DocumentStore, Vec<u8>) {
    let original = serde_json::to_vec_pretty(value).unwrap();
    std::fs::write(temp.path().join("workbench.preferences.json"), &original).unwrap();
    let mut store = DocumentStore::default();
    store.open(temp.path().join("workbench.json")).unwrap();
    (store, original)
}

fn save(store: &mut DocumentStore) -> Result<(), String> {
    store.save_board_configuration(
        store.board_layer_plan().cloned().unwrap_or_default(),
        store.board_preview_selection(),
        store.usb_startup_profile_id().map(str::to_owned),
    )
}

#[test]
fn v1_migration_preserves_all_preferences_missing_ids_order_and_recovery_bytes() {
    let temp = TempDir::new().unwrap();
    let value = preferences();
    let (mut store, original) = open_fixture(&temp, &value);
    let path = temp.path().join("workbench.preferences.json");
    let backup = path.with_extension("v1.backup.json");
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert!(!backup.exists());
    assert_eq!(store.usb_startup_profile_id(), Some("usb-id"));
    assert_eq!(
        store.legacy_three_bindings().unwrap().0,
        [Some("third-id".into()), None, Some("first-id".into())]
    );
    let plan = store.board_layer_plan().unwrap().clone();
    assert_eq!(
        plan.route(BoardLayer::ALL[0]).profile_id(),
        Some("third-id")
    );
    assert_eq!(plan.route(BoardLayer::ALL[1]), &BoardLayerRoute::Unassigned);
    for layer in &BoardLayer::ALL[3..] {
        assert_eq!(plan.route(*layer), &BoardLayerRoute::Unassigned);
    }
    assert_eq!(
        store.board_preview_selection(),
        BoardPreviewSelection::Layer(BoardLayer::ALL[2])
    );
    save(&mut store).unwrap();
    assert_eq!(std::fs::read(&backup).unwrap(), original);
    let saved_bytes = std::fs::read(&path).unwrap();
    let saved: Value = serde_json::from_slice(&saved_bytes).unwrap();
    for key in [
        "favorites",
        "host_active_profile_id",
        "host_execution_enabled",
        "command_execution_enabled",
        "auto_follow_enabled",
        "manual_override",
        "bottom_row_layout",
        "app_profile_links",
    ] {
        assert_eq!(saved[key], value[key], "{key}");
    }
    let mut reopened = DocumentStore::default();
    reopened.open(temp.path().join("workbench.json")).unwrap();
    assert_eq!(reopened.board_layer_plan(), Some(&plan));
    assert_eq!(
        reopened.legacy_three_bindings(),
        store.legacy_three_bindings()
    );
    save(&mut reopened).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), saved_bytes);
    assert_eq!(std::fs::read(&backup).unwrap(), original);
}

#[test]
fn explicit_empty_slots_are_not_an_absent_plan() {
    let temp = TempDir::new().unwrap();
    let mut v = preferences();
    v["hardware_profile_slots"] = json!([null, null, null, null]);
    let (mut store, _) = open_fixture(&temp, &v);
    let empty = BoardLayerPlan::default();
    assert_eq!(store.board_layer_plan(), Some(&empty));
    store.initialize_board_plan_if_absent(BoardLayerPlan::recommended(
        "a".into(),
        "b".into(),
        "c".into(),
        "d".into(),
    ));
    assert_eq!(store.board_layer_plan(), Some(&empty));
    v.as_object_mut().unwrap().remove("hardware_profile_slots");
    let temp2 = TempDir::new().unwrap();
    let (store, _) = open_fixture(&temp2, &v);
    assert!(store.board_layer_plan().is_none());
    assert!(store.legacy_three_bindings().is_none());
}

#[test]
fn corrupt_or_unknown_preferences_remain_unmodified() {
    for modification in [
        json!({"schema_version":3}),
        json!({"unexpected":true}),
        json!({"active_hardware_slot":4}),
        json!({"hardware_profile_slots":[null]}),
        json!({"hardware_profile_slots":null}),
    ] {
        let temp = TempDir::new().unwrap();
        let mut v = preferences();
        v.as_object_mut()
            .unwrap()
            .extend(modification.as_object().unwrap().clone());
        let (mut store, bytes) = open_fixture(&temp, &v);
        assert!(save(&mut store).is_err());
        assert_eq!(
            std::fs::read(temp.path().join("workbench.preferences.json")).unwrap(),
            bytes
        );
    }
}

#[test]
fn failed_saves_do_not_publish_plan_preview_or_revision() {
    for conflict in [true, false] {
        let temp = TempDir::new().unwrap();
        let (mut store, original) = open_fixture(&temp, &preferences());
        let plan = store.board_layer_plan().unwrap().clone();
        let preview = store.board_preview_selection();
        let revision = store.host_revision();
        let path = temp.path().join("workbench.preferences.json");
        if conflict {
            std::fs::write(&path, b"external edit").unwrap();
        } else {
            std::fs::write(path.with_extension("pending"), b"occupied").unwrap();
        }
        assert!(store
            .save_board_configuration(
                BoardLayerPlan::default(),
                BoardPreviewSelection::Layer(BoardLayer::ALL[5]),
                None
            )
            .is_err());
        assert_eq!(store.board_layer_plan(), Some(&plan));
        assert_eq!(store.board_preview_selection(), preview);
        assert_eq!(store.host_revision(), revision);
        assert_eq!(
            std::fs::read(&path).unwrap(),
            if conflict {
                b"external edit".to_vec()
            } else {
                original
            }
        );
    }
}

#[test]
fn migration_classifies_native_ids_without_replacing_them() {
    let temp = TempDir::new().unwrap();
    let mut draft = mi_model::draft::Draft::default();
    mi_model::preset::install_agent_starter_profiles(&mut draft).unwrap();
    let id = draft
        .document
        .editable
        .graph
        .profiles
        .iter()
        .find(|(_, p)| p.name == "Codex")
        .unwrap()
        .0
        .clone();
    std::fs::write(
        temp.path().join("workbench.json"),
        serde_json::to_vec(&draft.document).unwrap(),
    )
    .unwrap();
    let mut v = preferences();
    v["hardware_profile_slots"][1] = json!(id);
    let (store, _) = open_fixture(&temp, &v);
    assert_eq!(
        store.board_layer_plan().unwrap().route(BoardLayer::ALL[0]),
        &BoardLayerRoute::Native {
            owner: NativeLayerOwner::Codex,
            profile_id: id.clone()
        }
    );
    assert_eq!(store.legacy_three_bindings().unwrap().0[0], Some(id));
}

#[test]
fn default_initialization_is_saved_even_when_the_first_edit_is_a_noop() {
    let temp = TempDir::new().unwrap();
    let mut store = DocumentStore::default();
    let path = temp.path().join("workbench.json");
    store.open(path.clone()).unwrap();
    let plan = BoardLayerPlan::recommended("a".into(), "b".into(), "c".into(), "d".into());
    store.initialize_board_plan_if_absent(plan.clone());
    assert!(!path.with_extension("preferences.json").exists());
    save(&mut store).unwrap();
    let mut reopened = DocumentStore::default();
    reopened.open(path).unwrap();
    assert_eq!(reopened.board_layer_plan(), Some(&plan));
}

#[test]
fn a_conflicting_recovery_backup_blocks_migration_without_publishing_changes() {
    let temp = TempDir::new().unwrap();
    let (mut store, original) = open_fixture(&temp, &preferences());
    let path = temp.path().join("workbench.preferences.json");
    let backup = path.with_extension("v1.backup.json");
    std::fs::write(&backup, b"unrelated recovery bytes").unwrap();
    let plan = store.board_layer_plan().cloned();
    let preview = store.board_preview_selection();
    assert!(store
        .save_board_configuration(
            BoardLayerPlan::default(),
            BoardPreviewSelection::UsbStartup,
            None
        )
        .is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(std::fs::read(backup).unwrap(), b"unrelated recovery bytes");
    assert_eq!(store.board_layer_plan(), plan.as_ref());
    assert_eq!(store.board_preview_selection(), preview);
    assert!(!path.with_extension("pending").exists());
}
