use mi_model::{control_settings::ControlSettings, draft::Draft};
use serde_json::json;

#[test]
fn logical_sector_mode_preserves_independent_slots_history_and_source() {
    let mut draft = Draft::default();
    let p = draft.config.profiles[0].id.clone();
    let l = draft.config.profiles[0].layers[0].id.clone();
    let origin = draft.document.origin.clone();
    draft
        .set_control_settings(
            p.as_str().unwrap(),
            l.as_str().unwrap(),
            ControlSettings {
                joystick_mode: "logical_sectors".into(),
                active_sectors: 5,
                encoder_hold_action_id: None,
            },
        )
        .unwrap();
    assert!(draft
        .structure_changes
        .iter()
        .any(|s| s.contains("控件意图")));
    let a = mi_model::draft::actions()[0].clone();
    let b = mi_model::draft::actions()[1].clone();
    draft.assign(&p, &l, 13, a.clone()).unwrap();
    draft.assign(&p, &l, 14, b.clone()).unwrap();
    draft.assign(&p, &l, 20, b.clone()).unwrap();
    draft
        .set_control_settings(
            p.as_str().unwrap(),
            l.as_str().unwrap(),
            ControlSettings {
                joystick_mode: "source".into(),
                active_sectors: 1,
                encoder_hold_action_id: None,
            },
        )
        .unwrap();
    assert_eq!(draft.config.profiles[0].layers[0].joystick.sectors.len(), 5);
    assert_eq!(
        draft.edits[&mi_model::draft::binding_key(&p, &l, 13)].id,
        a.id
    );
    assert_eq!(
        draft.edits[&mi_model::draft::binding_key(&p, &l, 14)].id,
        b.id
    );
    let reopened = Draft::from_bytes(&serde_json::to_vec(&draft.document).unwrap()).unwrap();
    assert!(!reopened.read_only);
    assert_eq!(
        reopened.edits[&mi_model::draft::binding_key(&p, &l, 20)].id,
        b.id
    );
    assert_eq!(reopened.document.origin, origin);
    draft.undo(false).unwrap();
    assert_eq!(
        draft.document.editable.graph.layers[l.as_str().unwrap()]
            .control_settings
            .as_ref()
            .unwrap()
            .joystick_mode,
        "logical_sectors"
    );
    draft.undo(true).unwrap();
    assert_eq!(
        draft.document.editable.graph.layers[l.as_str().unwrap()]
            .control_settings
            .as_ref()
            .unwrap()
            .active_sectors,
        1
    );
    assert_eq!(draft.document.origin, origin);
    assert_ne!(origin["config"]["raw"], json!(null));
}

#[test]
fn unknown_mode_stays_visible_and_invalid_field_edits_leave_document_unchanged() {
    let draft = Draft::default();
    let pid = draft.config.profiles[0].id.as_str().unwrap().to_owned();
    let lid = draft.config.profiles[0].layers[0]
        .id
        .as_str()
        .unwrap()
        .to_owned();
    let mut raw = serde_json::to_value(&draft.document).unwrap();
    raw["graph"]["layers"][&lid]["control_settings"] =
        json!({"joystick_mode":"future-mode", "active_sectors":17});
    let mut reopened = Draft::from_bytes(&serde_json::to_vec(&raw).unwrap()).unwrap();
    assert!(!reopened.read_only);
    let before = serde_json::to_value(&reopened.document).unwrap();
    let error = reopened
        .set_control_settings(
            &pid,
            &lid,
            ControlSettings {
                joystick_mode: "source".into(),
                active_sectors: 2,
                encoder_hold_action_id: None,
            },
        )
        .unwrap_err();
    assert!(error.contains("摇杆模式"));
    assert_eq!(serde_json::to_value(&reopened.document).unwrap(), before);
    let error = reopened
        .set_control_settings(
            &pid,
            &lid,
            ControlSettings {
                joystick_mode: "source".into(),
                active_sectors: 33,
                encoder_hold_action_id: None,
            },
        )
        .unwrap_err();
    assert!(error.contains("活动扇区数"));
    assert_eq!(serde_json::to_value(&reopened.document).unwrap(), before);
}

#[test]
fn source_mode_does_not_materialize_inactive_sector_intent() {
    let mut draft = Draft::default();
    let pid = draft.config.profiles[0].id.as_str().unwrap().to_owned();
    let lid = draft.config.profiles[0].layers[0]
        .id
        .as_str()
        .unwrap()
        .to_owned();
    let original_controls = draft.document.editable.graph.layers[&lid].controls.clone();
    let original_order = draft.document.editable.graph.layers[&lid]
        .control_order
        .clone();
    draft
        .set_control_settings(
            &pid,
            &lid,
            ControlSettings {
                joystick_mode: "source".into(),
                active_sectors: 32,
                encoder_hold_action_id: None,
            },
        )
        .unwrap();
    let layer = &draft.document.editable.graph.layers[&lid];
    assert!(layer.controls == original_controls);
    assert_eq!(layer.control_order, original_order);
    draft
        .set_control_settings(
            &pid,
            &lid,
            ControlSettings {
                joystick_mode: "logical_sectors".into(),
                active_sectors: 32,
                encoder_hold_action_id: None,
            },
        )
        .unwrap();
    assert_eq!(
        draft.document.editable.graph.layers[&lid]
            .control_order
            .len(),
        48
    );
    assert!(original_controls
        .iter()
        .all(
            |(id, binding)| draft.document.editable.graph.layers[&lid].controls.get(id)
                == Some(binding)
        ));
}

#[test]
fn unknown_control_fields_make_document_read_only() {
    let draft = Draft::default();
    let pid = draft.config.profiles[0].id.as_str().unwrap().to_owned();
    let lid = draft.config.profiles[0].layers[0]
        .id
        .as_str()
        .unwrap()
        .to_owned();
    let mut raw = serde_json::to_value(&draft.document).unwrap();
    raw["graph"]["layers"][&lid]["control_settings"] = json!({
        "joystick_mode":"logical_sectors", "active_sectors":2,
        "future_direction_map":{"a":17}
    });
    let mut reopened = Draft::from_bytes(&serde_json::to_vec(&raw).unwrap()).unwrap();
    assert!(reopened.read_only);
    assert!(reopened
        .set_control_settings(
            &pid,
            &lid,
            ControlSettings {
                joystick_mode: "source".into(),
                active_sectors: 0,
                encoder_hold_action_id: None,
            }
        )
        .is_err());
}

#[test]
fn mouse_and_scroll_joystick_modes_preserve_dormant_slots_and_reject_unknown_modes() {
    let mut draft = Draft::default();
    let pid = draft.config.profiles[0].id.as_str().unwrap().to_owned();
    let lid = draft.config.profiles[0].layers[0]
        .id
        .as_str()
        .unwrap()
        .to_owned();
    draft
        .set_control_settings(
            &pid,
            &lid,
            ControlSettings {
                joystick_mode: "logical_sectors".into(),
                active_sectors: 4,
                encoder_hold_action_id: None,
            },
        )
        .unwrap();
    let before_count = draft.document.editable.graph.layers[&lid]
        .control_order
        .len();
    assert_eq!(before_count, 20);

    for mode in ["mouse", "scroll"] {
        draft
            .set_control_settings(
                &pid,
                &lid,
                ControlSettings {
                    joystick_mode: mode.into(),
                    active_sectors: 0,
                    encoder_hold_action_id: None,
                },
            )
            .unwrap();
        assert_eq!(
            draft.document.editable.graph.layers[&lid]
                .control_order
                .len(),
            before_count
        );
    }

    assert!(draft
        .set_control_settings(
            &pid,
            &lid,
            ControlSettings {
                joystick_mode: "future-mode".into(),
                active_sectors: 0,
                encoder_hold_action_id: None,
            },
        )
        .is_err());
}

#[test]
fn set_control_settings_preserves_encoder_hold_and_orphan_gc_plus_copy_layer_respect_it() {
    let mut draft = Draft::default();
    let pid_val = draft.config.profiles[0].id.clone();
    let lid_val = draft.config.profiles[0].layers[0].id.clone();
    let pid = pid_val.as_str().unwrap().to_owned();
    let lid = lid_val.as_str().unwrap().to_owned();
    let open_action = mi_model::draft::actions()
        .into_iter()
        .find(|a| a.id == "agent-model-picker-open")
        .unwrap();

    draft
        .assign_encoder_hold(&pid_val, &lid_val, open_action)
        .unwrap();
    let hold_uuid = draft.document.editable.graph.layers[&lid]
        .control_settings
        .as_ref()
        .and_then(|cs| cs.encoder_hold_action_id.clone())
        .expect("encoder_hold_action_id must be set");
    assert!(draft
        .document
        .editable
        .graph
        .actions
        .contains_key(&hold_uuid));

    // Updating joystick_mode with encoder_hold_action_id: None must preserve existing encoder_hold_action_id.
    draft
        .set_control_settings(
            &pid,
            &lid,
            ControlSettings {
                joystick_mode: "mouse".into(),
                active_sectors: 0,
                encoder_hold_action_id: None,
            },
        )
        .unwrap();
    let cs_after = draft.document.editable.graph.layers[&lid]
        .control_settings
        .as_ref()
        .unwrap();
    assert_eq!(cs_after.joystick_mode, "mouse");
    assert_eq!(
        cs_after.encoder_hold_action_id.as_deref(),
        Some(hold_uuid.as_str())
    );

    // duplicate_layer must clone encoder_hold_action_id to an independent action UUID.
    let copied_lid = draft.duplicate_layer(&pid, &lid, "副本层").unwrap();
    let copied_hold_uuid = draft.document.editable.graph.layers[&copied_lid]
        .control_settings
        .as_ref()
        .and_then(|cs| cs.encoder_hold_action_id.clone())
        .expect("copied layer must retain encoder_hold_action_id");
    assert_ne!(copied_hold_uuid, hold_uuid);
    assert_eq!(
        draft.document.editable.graph.actions[&copied_hold_uuid].id,
        "agent-model-picker-open"
    );

    // Unassigning encoder hold on original layer removes orphan hold_uuid while keeping copied_hold_uuid.
    draft.unassign_encoder_hold(&pid_val, &lid_val).unwrap();
    assert!(!draft
        .document
        .editable
        .graph
        .actions
        .contains_key(&hold_uuid));
    assert!(draft
        .document
        .editable
        .graph
        .actions
        .contains_key(&copied_hold_uuid));
}
