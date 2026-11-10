use mi_model::{
    apply::{compile_six_layer_profile, recognize_six_layer_profile, AppliedSixLayerPlan},
    board_layers::{BoardLayer, BoardLayerPlan, BoardLayerRoute},
    direct_controls::{CONTROL_BINDINGS, DIRECT_CONTROLS},
    host_router::{BottomRowLayout, ResolvedHostAction},
};
use serde_json::{json, Value};
const ORIGINAL: &[u8] = include_bytes!("../fixtures/device/synthetic-six-layer-source.json");

#[test]
fn six_layer_compiler_preserves_original_bytes_and_covers_every_control() {
    let compiled = compile_six_layer_profile(ORIGINAL, None).unwrap();
    assert_eq!(compiled.profile_index, 1);
    assert_eq!(
        recognize_six_layer_profile(&compiled.keymap, None).unwrap(),
        Some(1)
    );
    let parsed: Value = serde_json::from_slice(&compiled.keymap).unwrap();
    let original: Value = serde_json::from_slice(ORIGINAL).unwrap();
    assert_eq!(parsed["profiles"][0], original["profiles"][0]);
    let text = std::str::from_utf8(&compiled.keymap).unwrap();
    let source = std::str::from_utf8(ORIGINAL).unwrap();
    let original_profile = source
        .split("\"profiles\": [")
        .nth(1)
        .unwrap()
        .split("],\n  \"macros\"")
        .next()
        .unwrap();
    assert!(text.contains(original_profile));
    assert!(text.contains("\"unknownNativeField\": 1.00"));
    assert!(text.contains("\"keep\": [true, 9.00]"));
    let layers = parsed["profiles"][1]["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 6);
    assert_eq!(
        layers[0]["layout"],
        original["profiles"][0]["layers"][0]["layout"]
    );
    let mut usages = std::collections::BTreeSet::new();
    let mut key_codes = std::collections::BTreeSet::new();
    for layer in 2..=4 {
        let layout = &layers[layer - 1]["layout"];
        let tokens: Vec<_> = layout["keymap"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|row| row.as_array().unwrap())
            .chain(layout["encoders"][0].as_array().unwrap())
            .chain(
                layout["joystick"]["sectors"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|sector| &sector["k"]),
            )
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(tokens.len(), 20);
        for (slot, token) in tokens.into_iter().enumerate() {
            let control = DIRECT_CONTROLS[(layer - 2) * 20 + slot];
            assert_eq!(control.token, token);
            assert!(usages.insert(control.usage));
            assert!(key_codes.insert(control.key_code));
        }
        assert!(!layout.to_string().contains("KV_OAI"));
        assert_eq!(layout["joystick"]["type"], "RADIAL");
        assert_eq!(
            layout["joystick"]["sectors"][3],
            json!({"k":DIRECT_CONTROLS[(layer-2)*20+19].token,"a1":0.875,"a2":0.125})
        );
    }
    for layer in &layers[4..6] {
        assert!(!layer.to_string().contains("KV_OAI"));
        assert_eq!(
            layer["layout"]["encoders"],
            json!([["KC_NONE", "KC_NONE", "KC_NONE"]])
        );
        assert!(layer["layout"]["joystick"]["sectors"]
            .as_array()
            .unwrap()
            .iter()
            .all(|sector| sector["k"] == "KC_NONE"));
        assert!(layer["layout"]["keymap"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|row| row.as_array().unwrap())
            .all(|token| token == "KC_NONE"));
    }
    assert_eq!(
        compile_six_layer_profile(&compiled.keymap, None)
            .unwrap()
            .keymap,
        compiled.keymap
    );
}

#[test]
fn ambiguous_or_modified_native_and_dedicated_profiles_are_rejected() {
    let mut value: Value = serde_json::from_slice(ORIGINAL).unwrap();
    let mut duplicate = value["profiles"][0].clone();
    duplicate["id"] = json!(1);
    value["profiles"].as_array_mut().unwrap().push(duplicate);
    assert!(compile_six_layer_profile(&serde_json::to_vec(&value).unwrap(), None).is_err());
    let mut value: Value = serde_json::from_slice(ORIGINAL).unwrap();
    value["profiles"][0]["layers"][0]["layout"]["keymap"][0][0] = json!("KV_OAI_ACT00");
    assert!(compile_six_layer_profile(&serde_json::to_vec(&value).unwrap(), None).is_err());
    let mut value: Value =
        serde_json::from_slice(&compile_six_layer_profile(ORIGINAL, None).unwrap().keymap).unwrap();
    value["profiles"][1]["layers"][3]["layout"]["encoders"][0][0] = json!("KV_OAI_ENC_CC");
    assert!(compile_six_layer_profile(&serde_json::to_vec(&value).unwrap(), None).is_err());
}

#[test]
fn applied_plan_freezes_routes_and_converts_only_dictation_to_click() {
    let mut draft = mi_model::draft::Draft::default();
    mi_model::preset::install_agent_starter_profiles(&mut draft).unwrap();
    let id = |name: &str| {
        draft
            .document
            .editable
            .graph
            .profiles
            .iter()
            .find(|(_, p)| p.name == name)
            .unwrap()
            .0
            .clone()
    };
    let mut desired = BoardLayerPlan::recommended(
        id("Codex"),
        id("Antigravity"),
        id("DeepSeek Coder (DSH)"),
        id(mi_model::preset::MANAGED_CINDY_PROFILE_NAME),
    );
    let applied =
        AppliedSixLayerPlan::compile(&desired, &draft.document, BottomRowLayout::Split, true)
            .unwrap();
    assert_eq!(CONTROL_BINDINGS.len(), 20);
    for host in &applied.hosts[0..2] {
        assert!(host.actions.iter().flatten().any(|a| matches!(a,ResolvedHostAction::AgentCommand{command,..} if command=="toggle-dictation")));
        assert!(!host.actions.iter().flatten().any(
            |a| matches!(a,ResolvedHostAction::AgentCommand{command,..} if command=="push-to-talk")
        ));
    }
    desired.replace(BoardLayer::ALL[1], BoardLayerRoute::Unassigned);
    assert_ne!(applied.desired, desired);
    assert!(
        AppliedSixLayerPlan::compile(&desired, &draft.document, BottomRowLayout::Split, true)
            .is_err()
    );
    desired = applied.desired.clone();
    desired.replace(
        BoardLayer::ALL[5],
        desired.route(BoardLayer::ALL[1]).clone(),
    );
    assert!(
        AppliedSixLayerPlan::compile(&desired, &draft.document, BottomRowLayout::Split, true)
            .is_err()
    );
}
