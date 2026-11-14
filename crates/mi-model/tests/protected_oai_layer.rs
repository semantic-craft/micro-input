use mi_model::{
    control_settings::ControlSettings,
    draft::{actions, Action, Draft},
    is_protected_oai_layer,
    lighting::{Edit, Effect, Lighting, Zone},
    official_chatgpt_layer, preset, Binding, Joystick, Layer, Lights, Token,
};
use serde_json::{json, Value};

fn empty_layer() -> Layer {
    Layer {
        id: json!("layer-1"),
        name: "Layer 1".into(),
        color: json!("#477DAD"),
        keys: std::array::from_fn(|_| Binding::Basic(Token("KC_A".into()))),
        encoders: std::array::from_fn(|_| Binding::Basic(Token("KC_B".into()))),
        joystick: Joystick {
            kind: json!("sectors"),
            sectors: vec![Binding::Basic(Token("KC_C".into()))],
        },
        lights: Lights {
            backlight: Value::Null,
            underglow: Value::Null,
        },
    }
}

#[test]
fn is_protected_oai_layer_replicates_official_mg1_across_keys_encoders_and_joystick_sectors() {
    let base = empty_layer();
    assert!(!is_protected_oai_layer(&base));

    let mut with_key = base.clone();
    with_key.keys[6] = Binding::Unknown(json!("KV_OAI_ACT06"));
    assert!(is_protected_oai_layer(&with_key));

    let mut with_basic_key = base.clone();
    with_basic_key.keys[0] = Binding::Basic(Token("KV_OAI_AG00".into()));
    assert!(is_protected_oai_layer(&with_basic_key));

    let mut with_encoder = base.clone();
    with_encoder.encoders[2] = Binding::Unknown(json!("KV_OAI_ENC_CLK"));
    assert!(is_protected_oai_layer(&with_encoder));

    let mut with_sector_str = base.clone();
    with_sector_str.joystick.sectors = vec![Binding::Unknown(json!("KV_OAI_SEC01"))];
    assert!(is_protected_oai_layer(&with_sector_str));

    let mut with_sector_obj = base.clone();
    with_sector_obj.joystick.sectors = vec![Binding::Unknown(json!({"k": "KV_OAI_SEC02"}))];
    assert!(is_protected_oai_layer(&with_sector_obj));

    let official = official_chatgpt_layer(json!("chatgpt"), "");
    assert_eq!(official.name, "ChatGPT");
    assert_eq!(official.color, json!(0x00FFA6u64));
    assert_eq!(official.joystick.kind, json!("VENDOR"));
    assert!(official.joystick.sectors.is_empty());
    assert!(is_protected_oai_layer(&official));
}

#[test]
fn four_hard_locks_reject_edits_lighting_duplicate_export_and_skip_protected_on_cross_layer_merge()
{
    let mut draft = Draft::default();
    let pid = draft.document.editable.graph.profile_order[0].clone();
    let normal_lid = draft.document.editable.graph.profiles[&pid].layer_order[0].clone();
    let protected_lid = draft
        .create_protected_oai_layer(&pid, "ChatGPT")
        .expect("create protected layer");

    assert!(!draft.document.is_protected_oai_layer(&normal_lid));
    assert!(draft.document.is_protected_oai_layer(&protected_lid));
    assert_eq!(
        draft.document.protected_layer_ids(),
        vec![protected_lid.clone()]
    );

    let projected = draft
        .config
        .profiles
        .iter()
        .find(|p| p.id == json!(pid))
        .unwrap()
        .layers
        .iter()
        .find(|l| l.id == json!(protected_lid))
        .unwrap();
    assert!(is_protected_oai_layer(projected));
    assert_eq!(projected.color, json!(0x00FFA6u64));
    assert_eq!(projected.joystick.kind, json!("VENDOR"));

    let rev_before = draft.revision;
    let sample_action = actions().into_iter().find(|a| a.id == "search").unwrap();

    assert!(draft
        .assign(&json!(pid), &json!(protected_lid), 0, sample_action.clone())
        .is_err());
    let first_control =
        draft.document.editable.graph.layers[&protected_lid].control_order[0].clone();
    assert!(draft
        .bind_existing_action(&pid, &protected_lid, &first_control, None)
        .is_err());
    assert!(draft
        .set_control_settings(
            &pid,
            &protected_lid,
            ControlSettings {
                joystick_mode: "logical_sectors".into(),
                active_sectors: 4,
            },
        )
        .is_err());
    let macro_action = actions()
        .into_iter()
        .find(|a| a.id == "local-macro")
        .unwrap();
    assert!(draft
        .save_macro_for_binding(
            None,
            macro_action.clone(),
            &json!(pid),
            &json!(protected_lid),
            0,
        )
        .is_err());
    let saved_macro_id = draft.save_macro(None, macro_action).unwrap();
    assert!(draft
        .bind_macro(&json!(pid), &json!(protected_lid), 0, Some(&saved_macro_id),)
        .is_err());

    assert!(!draft.document.can_add_lighting(&protected_lid));
    assert!(draft.add_lighting(&protected_lid).is_err());
    assert!(draft
        .edit_lighting(
            &pid,
            &protected_lid,
            Zone::Backlight,
            Edit::Effect(Effect::Solid),
            false,
        )
        .is_err());
    assert!(draft.reset_lighting(&protected_lid).is_err());

    draft
        .mutate(|doc| {
            for lid in doc.editable.graph.profiles[&pid].layer_order.clone() {
                if !doc.is_protected_oai_layer(&lid) {
                    doc.editable.graph.layers.get_mut(&lid).unwrap().lighting =
                        Some(Lighting::creator_micro_2_defaults());
                }
            }
            Ok(())
        })
        .unwrap();
    draft
        .edit_lighting(
            &pid,
            &normal_lid,
            Zone::Backlight,
            Edit::Color(0x123456),
            true,
        )
        .unwrap();
    assert_eq!(
        draft.document.lighting(&normal_lid).unwrap().backlight["color"],
        json!(0x123456)
    );
    assert!(
        draft.document.editable.graph.layers[&protected_lid]
            .lighting
            .is_none(),
        "all_layers lighting edit must skip protected OAI layers"
    );

    assert!(draft
        .duplicate_layer(&pid, &protected_lid, "ChatGPT 副本")
        .is_err());
    assert!(preset::export(&draft, Some(&pid), Some(&protected_lid)).is_err());

    draft
        .assign(&json!(pid), &json!(normal_lid), 11, sample_action.clone())
        .unwrap();
    let updated = draft
        .merge_keys_across_layers(&pid, 10, 11, Some(sample_action.clone()))
        .unwrap();
    assert!(updated.contains(&normal_lid));
    assert!(!updated.contains(&protected_lid));

    let normal_layer = &draft.document.editable.graph.layers[&normal_lid];
    let normal_10 = normal_layer
        .controls
        .values()
        .find(|b| b.logical_control == "logical-10")
        .unwrap();
    let normal_11 = normal_layer
        .controls
        .values()
        .find(|b| b.logical_control == "logical-11")
        .unwrap();
    assert!(normal_10.action_id.is_some());
    assert!(normal_11.action_id.is_none());
    assert_eq!(normal_11.original, json!("Empty"));

    let protected_layer = &draft.document.editable.graph.layers[&protected_lid];
    let protected_10 = protected_layer
        .controls
        .values()
        .find(|b| b.logical_control == "logical-10")
        .unwrap();
    let protected_11 = protected_layer
        .controls
        .values()
        .find(|b| b.logical_control == "logical-11")
        .unwrap();
    assert!(protected_10.action_id.is_none());
    assert!(protected_11.action_id.is_none());
    assert_eq!(protected_10.original, json!({"Unknown": "KV_OAI_ACT10"}));
    assert_eq!(protected_11.original, json!({"Unknown": "KV_OAI_ACT11"}));
    assert!(draft.revision > rev_before);
}
