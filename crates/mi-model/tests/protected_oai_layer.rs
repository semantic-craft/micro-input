use mi_model::{
    control_settings::ControlSettings,
    draft::{actions, Action, Draft},
    host_router::{
        check_passthrough_health, resolve_host_action, HostIdentityStatus, PassThroughHealth,
    },
    is_protected_oai_layer,
    lighting::{Edit, Effect, Lighting, Zone},
    official_chatgpt_layer, parse_device_files, preset, Binding, Joystick, Layer, Lights, Token,
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

#[test]
fn compact_host_keymap_stays_under_1024_bytes_while_preserving_all_profiles_and_30_macros_locally()
{
    let mut draft = Draft::default();
    let profile_ids = preset::install_agent_starter_profiles(&mut draft).unwrap();
    assert_eq!(profile_ids.len(), 8);

    let cindy_pid = draft
        .document
        .editable
        .graph
        .profiles
        .iter()
        .find(|(_, p)| p.name == "Cindy")
        .map(|(id, _)| id.clone())
        .unwrap();
    let codex_pid = draft
        .document
        .editable
        .graph
        .profiles
        .iter()
        .find(|(_, p)| p.name == "Codex")
        .map(|(id, _)| id.clone())
        .unwrap();
    let ag_pid = draft
        .document
        .editable
        .graph
        .profiles
        .iter()
        .find(|(_, p)| p.name == "Antigravity")
        .map(|(id, _)| id.clone())
        .unwrap();

    let cindy_protected_lid =
        draft.document.editable.graph.profiles[&cindy_pid].layer_order[0].clone();
    let codex_protected_lid =
        draft.document.editable.graph.profiles[&codex_pid].layer_order[0].clone();
    assert!(draft.document.is_protected_oai_layer(&cindy_protected_lid));
    assert!(draft.document.is_protected_oai_layer(&codex_protected_lid));

    let ag_lid = draft.document.editable.graph.profiles[&ag_pid].layer_order[0].clone();
    let mut macro_ids = Vec::new();
    for idx in 1..=30 {
        let action = Action {
            id: format!("macro-{idx}"),
            name: format!("宏动作 #{idx:02}"),
            category: "普通宏".into(),
            detail: "30 宏容量保持测试".into(),
            payload: json!({
                "type": "local_macro",
                "macro_sequence": {
                    "format_version": 1,
                    "events": [
                        {"type": "press", "key": "a"},
                        {"type": "release", "key": "a"}
                    ]
                }
            }),
        };
        let mid = if idx <= 3 {
            draft
                .save_macro_for_binding(
                    None,
                    action,
                    &json!(ag_pid),
                    &json!(ag_lid),
                    (idx + 1) as u32,
                )
                .unwrap()
        } else {
            draft.save_macro(None, action).unwrap()
        };
        macro_ids.push(mid);
    }
    assert_eq!(draft.macros().len(), 30);

    for (pid, expected_name) in [(&cindy_pid, "Cindy"), (&codex_pid, "Codex")] {
        let compact = draft
            .compile_compact_host_keymap(Some(pid))
            .expect("compile compact host keymap");
        assert!(
            compact.len() < 1024,
            "compact host keymap.json must be < 1024 bytes to avoid firmware 0.6.3 single-packet / 4KB overflow, got {} bytes",
            compact.len()
        );
        let parsed = parse_device_files(&compact, None).expect("firmware-valid keymap.json");
        assert!(
            !parsed
                .warnings
                .iter()
                .any(|w| w.contains("缺失") || w.contains("未知") || w.contains("重复")),
            "{:?}",
            parsed.warnings
        );
        assert_eq!(parsed.profiles.len(), 1);
        assert_eq!(parsed.profiles[0].name, expected_name);
        assert_eq!(parsed.profiles[0].layers.len(), 1);
        assert!(is_protected_oai_layer(&parsed.profiles[0].layers[0]));
        assert_eq!(
            check_passthrough_health(
                Some(&parsed),
                Some(&HostIdentityStatus {
                    version: Some("0.6.3".into()),
                    profile_index: Some(0),
                    layer_index: Some(1),
                }),
            ),
            PassThroughHealth::Ready
        );
    }

    let serialized = serde_json::to_vec(&draft.document).unwrap();
    let reopened = Draft::from_bytes(&serialized).expect("reopen local v2 document");
    assert!(!reopened.read_only);
    assert_eq!(reopened.config.profiles.len(), 8);
    assert_eq!(reopened.macros().len(), 30);
    assert!(reopened
        .document
        .is_protected_oai_layer(&cindy_protected_lid));
    assert!(reopened
        .document
        .is_protected_oai_layer(&codex_protected_lid));
    assert!(
        resolve_host_action(&reopened.document, Some(&ag_pid), "AG00", true).is_some(),
        "Antigravity workbench routing must remain intact after compact host keymap compilation"
    );
}
