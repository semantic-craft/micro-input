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
