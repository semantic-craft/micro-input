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
            },
        )
        .unwrap();
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
            },
        )
        .unwrap_err();
    assert!(error.contains("活动扇区数"));
    assert_eq!(serde_json::to_value(&reopened.document).unwrap(), before);
}
