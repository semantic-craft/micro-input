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
    let mut vendor_slots = std::collections::BTreeSet::new();
    for layer in 2..=3 {
        assert_eq!(layers[layer - 1]["lights"]["backlight"]["effect"], "off");
        assert_eq!(layers[layer - 1]["lights"]["underglow"]["effect"], "off");
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
            if slot < 6 {
                let expected_ag = format!("KV_OAI_AG{:02}", 6 + (layer - 2) * 6 + slot);
                assert_eq!(token, expected_ag);
                assert!(vendor_slots.insert(token.to_owned()));
            } else {
                let control = DIRECT_CONTROLS[(layer - 2) * 20 + slot];
                assert_eq!(control.token, token);
                assert!(usages.insert(control.usage));
                assert!(key_codes.insert(control.key_code));
            }
        }
        assert_eq!(layout["joystick"]["type"], "RADIAL");
        assert_eq!(
            layout["joystick"]["sectors"][3],
            json!({"k":DIRECT_CONTROLS[(layer-2)*20+19].token,"a1":0.875,"a2":0.125})
        );
    }
    assert_eq!(vendor_slots.len(), 12);
    for layer in &layers[3..6] {
        assert_eq!(layer["lights"]["backlight"]["effect"], "off");
        assert_eq!(layer["lights"]["underglow"]["effect"], "off");
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
fn ambiguous_or_conflicting_profiles_are_rejected_while_outdated_managed_layers_upgrade_in_place() {
    let mut value: Value = serde_json::from_slice(ORIGINAL).unwrap();
    let mut duplicate = value["profiles"][0].clone();
    duplicate["id"] = json!(1);
    value["profiles"].as_array_mut().unwrap().push(duplicate);
    assert!(compile_six_layer_profile(&serde_json::to_vec(&value).unwrap(), None).is_err());

    let mut value: Value = serde_json::from_slice(ORIGINAL).unwrap();
    value["profiles"][0]["layers"][0]["layout"]["keymap"][0][0] = json!("KV_OAI_ACT00");
    assert!(compile_six_layer_profile(&serde_json::to_vec(&value).unwrap(), None).is_err());

    let canonical = compile_six_layer_profile(ORIGINAL, None).unwrap().keymap;
    let mut enc_conflict: Value = serde_json::from_slice(&canonical).unwrap();
    enc_conflict["profiles"][1]["layers"][3]["layout"]["encoders"][0][0] = json!("KV_OAI_ENC_CC");
    assert!(compile_six_layer_profile(&serde_json::to_vec(&enc_conflict).unwrap(), None).is_err());

    let mut outdated: Value = serde_json::from_slice(&canonical).unwrap();
    outdated["profiles"][1]["layers"][1]["layout"]["keymap"][0][0] = json!("KC_F13");
    let outdated_bytes = serde_json::to_vec(&outdated).unwrap();
    assert!(recognize_six_layer_profile(&outdated_bytes, None).is_err());
    let upgraded = compile_six_layer_profile(&outdated_bytes, None).unwrap();
    assert_eq!(upgraded.profile_index, 1);
    assert_eq!(
        recognize_six_layer_profile(&upgraded.keymap, None).unwrap(),
        Some(1)
    );
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
    let mut desired =
        BoardLayerPlan::recommended(id("Codex"), id("Antigravity"), id("DeepSeek Coder (DSH)"));
    let applied =
        AppliedSixLayerPlan::compile(&desired, &draft.document, BottomRowLayout::Split, true)
            .unwrap();
    assert_eq!(CONTROL_BINDINGS.len(), 20);
    assert_eq!(applied.hosts.len(), 2);
    assert!(applied.validate_current().is_ok());
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

#[test]
fn host_mouse_commands_preserve_host_adapter_in_applied_six_layer_plan() {
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
    let ag_id = id("Antigravity");
    let codex_id = id("Codex");
    let dsh_id = id("DeepSeek Coder (DSH)");
    let ag_layer = draft.document.editable.graph.profiles[&ag_id].layer_order[0].clone();
    let catalog = mi_model::draft::actions();
    let cycle_action = catalog
        .iter()
        .find(|a| a.id == "host-mouse-speed-cycle")
        .unwrap()
        .clone();
    let move_up_action = catalog
        .iter()
        .find(|a| a.id == "host-mouse-move-up")
        .unwrap()
        .clone();
    draft
        .assign(&json!(ag_id), &json!(ag_layer), 15, cycle_action)
        .unwrap();
    draft
        .assign(&json!(ag_id), &json!(ag_layer), 16, move_up_action)
        .unwrap();

    let desired = BoardLayerPlan::recommended(codex_id, ag_id, dsh_id);
    let applied =
        AppliedSixLayerPlan::compile(&desired, &draft.document, BottomRowLayout::Split, true)
            .unwrap();
    assert_eq!(
        applied.hosts[0].actions[15],
        Some(ResolvedHostAction::AgentCommand {
            adapter: "host".into(),
            command: "mouse-speed:cycle".into(),
        })
    );
    assert_eq!(
        applied.hosts[0].actions[16],
        Some(ResolvedHostAction::AgentCommand {
            adapter: "host".into(),
            command: "mouse-move:up".into(),
        })
    );
}

#[test]
fn six_layer_v070_mouse_and_scroll_joystick_modes_gate_by_firmware_and_keep_strict_key_invariants()
{
    use mi_model::apply::{
        compile_six_layer_profile_for_firmware, extract_keymap_joystick_hardware_mode,
        patch_keymap_joystick_hardware_mode, recognize_six_layer_profile_for_firmware,
        recognize_six_layer_profile_in_config, verify_active_six_layer_profile_for_firmware,
    };

    assert!(
        compile_six_layer_profile_for_firmware(ORIGINAL, None, Some("0.6.3"), Some("mouse"))
            .unwrap_err()
            .contains("0.7.0-rc.16")
    );
    assert!(compile_six_layer_profile_for_firmware(
        ORIGINAL,
        None,
        Some("0.7.0-rc.15"),
        Some("scroll")
    )
    .unwrap_err()
    .contains("0.7.0-rc.16"));

    let compiled_mouse =
        compile_six_layer_profile_for_firmware(ORIGINAL, None, Some("0.7.0-rc.16"), Some("mouse"))
            .unwrap();
    assert_eq!(compiled_mouse.profile_index, 1);
    let text = std::str::from_utf8(&compiled_mouse.keymap).unwrap();
    assert!(text.contains("\"unknownNativeField\": 1.00"));
    assert!(text.contains("\"keep\": [true, 9.00]"));

    for layer_1_based in 1..=6_u8 {
        assert_eq!(
            extract_keymap_joystick_hardware_mode(
                &compiled_mouse.keymap,
                Some(1),
                Some(layer_1_based)
            )
            .as_deref(),
            Some("mouse")
        );
    }

    assert_eq!(
        recognize_six_layer_profile_for_firmware(&compiled_mouse.keymap, None, Some("0.7.0-rc.16"))
            .unwrap(),
        Some(1)
    );
    verify_active_six_layer_profile_for_firmware(
        &compiled_mouse.keymap,
        None,
        1,
        Some("0.7.0-rc.16"),
    )
    .unwrap();

    let mut cfg_v070 = mi_model::parse_device_files(&compiled_mouse.keymap, None).unwrap();
    cfg_v070.firmware = Some("0.7.0".into());
    assert_eq!(
        recognize_six_layer_profile_in_config(&cfg_v070).unwrap(),
        Some(1)
    );

    let mut cfg_v063 = cfg_v070.clone();
    cfg_v063.firmware = Some("0.6.3".into());
    assert!(recognize_six_layer_profile_in_config(&cfg_v063)
        .unwrap_err()
        .contains("0.7.0-rc.16"));
    assert!(verify_active_six_layer_profile_for_firmware(
        &compiled_mouse.keymap,
        None,
        1,
        Some("0.6.3"),
    )
    .unwrap_err()
    .contains("0.7.0-rc.16"));

    // Strict key/encoder KV_OAI_* and DIRECT_CONTROLS invariants must still reject manual edits!
    let mut tampered_key: Value = serde_json::from_slice(&compiled_mouse.keymap).unwrap();
    tampered_key["profiles"][1]["layers"][1]["layout"]["keymap"][2][0] = json!("KC_A");
    assert!(recognize_six_layer_profile_for_firmware(
        &serde_json::to_vec(&tampered_key).unwrap(),
        None,
        Some("0.7.0-rc.16")
    )
    .is_err());

    let mut tampered_enc: Value = serde_json::from_slice(&compiled_mouse.keymap).unwrap();
    tampered_enc["profiles"][1]["layers"][2]["layout"]["encoders"][0][0] = json!("KV_OAI_ENC_CC");
    let tampered_enc_bytes = serde_json::to_vec(&tampered_enc).unwrap();
    assert!(recognize_six_layer_profile_for_firmware(
        &tampered_enc_bytes,
        None,
        Some("0.7.0-rc.16")
    )
    .is_err());
    assert!(compile_six_layer_profile_for_firmware(
        &tampered_enc_bytes,
        None,
        Some("0.7.0-rc.16"),
        None
    )
    .is_err());

    let mut tampered_codex: Value = serde_json::from_slice(&compiled_mouse.keymap).unwrap();
    tampered_codex["profiles"][1]["layers"][0]["layout"]["keymap"][0][0] = json!("KC_F13");
    assert!(recognize_six_layer_profile_for_firmware(
        &serde_json::to_vec(&tampered_codex).unwrap(),
        None,
        Some("0.7.0-rc.16")
    )
    .is_err());

    // Patching six-layer profile between MOUSE -> SCROLL -> logical_sectors preserves exact outer bytes
    let (_, unchanged) = patch_keymap_joystick_hardware_mode(
        &compiled_mouse.keymap,
        "0.7.0-rc.16",
        Some(1),
        None,
        "mouse",
    )
    .unwrap();
    assert!(!unchanged);

    let (patched_scroll, changed_scroll) = patch_keymap_joystick_hardware_mode(
        &compiled_mouse.keymap,
        "0.7.0-rc.16",
        Some(1),
        None,
        "scroll",
    )
    .unwrap();
    assert!(changed_scroll);
    assert_eq!(
        extract_keymap_joystick_hardware_mode(&patched_scroll, Some(1), Some(2)).as_deref(),
        Some("scroll")
    );
    assert_eq!(
        recognize_six_layer_profile_for_firmware(&patched_scroll, None, Some("0.7.0-rc.16"))
            .unwrap(),
        Some(1)
    );

    let (restored_sectors, changed_sectors) = patch_keymap_joystick_hardware_mode(
        &patched_scroll,
        "0.7.0-rc.16",
        Some(1),
        None,
        "logical_sectors",
    )
    .unwrap();
    assert!(changed_sectors);
    assert_eq!(
        extract_keymap_joystick_hardware_mode(&restored_sectors, Some(1), Some(1)).as_deref(),
        Some("vendor")
    );
    for layer_1_based in 2..=6_u8 {
        assert_eq!(
            extract_keymap_joystick_hardware_mode(&restored_sectors, Some(1), Some(layer_1_based))
                .as_deref(),
            Some("radial")
        );
    }
    assert_eq!(
        recognize_six_layer_profile_for_firmware(&restored_sectors, None, Some("0.6.3")).unwrap(),
        Some(1)
    );
    let restored_text = std::str::from_utf8(&restored_sectors).unwrap();
    assert!(restored_text.contains("\"unknownNativeField\": 1.00"));
    assert!(restored_text.contains("\"keep\": [true, 9.00]"));
}

#[test]
fn applied_six_layer_plan_tracks_and_gates_v070_joystick_hardware_modes() {
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
    let ag_id = id("Antigravity");
    let codex_id = id("Codex");
    let dsh_id = id("DeepSeek Coder (DSH)");
    let ag_layer = draft.document.editable.graph.profiles[&ag_id].layer_order[0].clone();
    draft
        .set_control_settings(
            &ag_id,
            &ag_layer,
            mi_model::control_settings::ControlSettings {
                joystick_mode: "mouse".into(),
                active_sectors: 0,
                encoder_hold_action_id: None,
            },
        )
        .unwrap();

    let (view_cfg, _) = draft.document.view().unwrap();
    let ag_view = view_cfg
        .profiles
        .iter()
        .find(|p| p.name == "Antigravity")
        .unwrap();
    assert_eq!(ag_view.layers[0].joystick.kind, json!({"type": "MOUSE"}));

    let desired = BoardLayerPlan::recommended(codex_id, ag_id, dsh_id);

    // Offline document (no firmware baseline yet) compiles and records joystick_mode, then gates on live firmware
    let plan =
        AppliedSixLayerPlan::compile(&desired, &draft.document, BottomRowLayout::Split, true)
            .unwrap();
    assert!(plan.uses_v070_joystick_hardware_mode());
    assert_eq!(plan.hosts[0].joystick_mode.as_deref(), Some("MOUSE"));
    assert_eq!(plan.hosts[1].joystick_mode, None);
    assert!(plan.validate_current().is_ok());
    assert!(plan.validate_for_firmware(Some("0.7.0-rc.16")).is_ok());
    assert!(plan
        .validate_for_firmware(Some("0.6.3"))
        .unwrap_err()
        .contains("0.7.0-rc.16"));
    assert_eq!(
        plan.compiled_board_layer(BoardLayer::ALL[1]).joystick.kind,
        json!("MOUSE")
    );
    assert_eq!(
        plan.compiled_board_layer(BoardLayer::ALL[2]).joystick.kind,
        json!("RADIAL")
    );

    // Document with known 0.6.3 baseline rejects compile directly
    draft.document.baseline = Some(json!({"firmware": "0.6.3"}));
    assert!(
        AppliedSixLayerPlan::compile(&desired, &draft.document, BottomRowLayout::Split, true)
            .unwrap_err()
            .contains("0.7.0-rc.16")
    );
    draft.document.baseline = Some(json!({"firmware": "0.7.0-rc.16"}));
    assert!(
        AppliedSixLayerPlan::compile(&desired, &draft.document, BottomRowLayout::Split, true)
            .is_ok()
    );
}
