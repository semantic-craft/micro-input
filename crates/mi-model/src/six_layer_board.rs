use super::{array, field, object};
use crate::{
    board_layers::{BoardLayer, BoardLayerPlan, BoardLayerRoute, NativeLayerOwner},
    direct_controls::{CONTROL_BINDINGS, DIRECT_CONTROLS},
    document::LocalDocumentV2,
    host_router::{resolve_host_action_with_layout, BottomRowLayout, ResolvedHostAction},
    parse_device_files,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, value::RawValue, Value};

pub const SIX_LAYER_PROFILE_NAME: &str = "Micro Input Six Layers";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AppliedHostLayer {
    pub profile_id: String,
    pub adapter: String,
    pub actions: [Option<ResolvedHostAction>; 20],
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AppliedSixLayerPlan {
    pub desired: BoardLayerPlan,
    pub bottom_row_layout: BottomRowLayout,
    pub hosts: Vec<AppliedHostLayer>,
}

impl AppliedSixLayerPlan {
    pub fn validate_current(&self) -> Result<(), String> {
        if !matches!(
            self.desired.route(BoardLayer::ALL[0]),
            BoardLayerRoute::Native {
                owner: NativeLayerOwner::Codex,
                ..
            }
        ) || self.hosts.len() != 2
            || [3, 4, 5].iter().any(|&index| {
                self.desired.route(BoardLayer::ALL[index]) != &BoardLayerRoute::Unassigned
            })
        {
            return Err("旧六层安装已暂停，请先恢复原设备配置".into());
        }
        for (index, adapter) in ["antigravity", "dsh-desktop"].iter().enumerate() {
            let host = &self.hosts[index];
            if host.adapter != *adapter
                || self.desired.route(BoardLayer::ALL[index + 1])
                    != &(BoardLayerRoute::HostProfile {
                        profile_id: host.profile_id.clone(),
                    })
            {
                return Err("六层托管目标不受支持，请先恢复原设备配置".into());
            }
        }
        Ok(())
    }

    pub fn compiled_board_layer(&self, board_layer: BoardLayer) -> crate::Layer {
        let index = usize::from(board_layer.number().saturating_sub(1));
        if index == 0
            || matches!(
                self.desired.route(board_layer),
                BoardLayerRoute::Native { .. }
            )
        {
            return crate::official_chatgpt_layer(json!(0), "Codex");
        }
        let tokens: Vec<&str> = match index {
            1 => LAYER2_VENDOR_SLOTS
                .into_iter()
                .chain(DIRECT_CONTROLS[6..20].iter().map(|control| control.token))
                .collect(),
            2 => LAYER3_VENDOR_SLOTS
                .into_iter()
                .chain(DIRECT_CONTROLS[26..40].iter().map(|control| control.token))
                .collect(),
            _ => vec!["KC_NONE"; 20],
        };
        let name = [
            "Codex",
            "Antigravity",
            "DeepSeek Harness",
            "Unassigned 4",
            "Unassigned 5",
            "Unassigned 6",
        ]
        .get(index)
        .copied()
        .unwrap_or("Unassigned");
        crate::Layer {
            id: json!(index),
            name: name.into(),
            color: json!(0),
            keys: std::array::from_fn(|i| crate::Binding::Basic(crate::Token(tokens[i].into()))),
            encoders: std::array::from_fn(|i| {
                crate::Binding::Basic(crate::Token(tokens[13 + i].into()))
            }),
            joystick: crate::Joystick {
                kind: json!("RADIAL"),
                sectors: (16..20)
                    .map(|i| crate::Binding::Basic(crate::Token(tokens[i].into())))
                    .collect(),
            },
            lights: crate::Lights {
                backlight: Value::Null,
                underglow: Value::Null,
            },
        }
    }

    pub fn compile(
        plan: &BoardLayerPlan,
        doc: &LocalDocumentV2,
        layout: BottomRowLayout,
        command_execution: bool,
    ) -> Result<Self, String> {
        let BoardLayerRoute::Native {
            owner: NativeLayerOwner::Codex,
            profile_id,
        } = plan.route(BoardLayer::ALL[0])
        else {
            return Err("第 1 层必须为 Codex 原生方案".into());
        };
        if crate::board_layers::native_owner(doc, profile_id) != Some(NativeLayerOwner::Codex) {
            return Err("第 1 层未引用受保护的 Codex 原生方案".into());
        }
        if [3, 4, 5]
            .iter()
            .any(|&index| plan.route(BoardLayer::ALL[index]) != &BoardLayerRoute::Unassigned)
        {
            return Err("当前第 4、5、6 层必须留空，尚无额外输入编码".into());
        }
        let names = ["Antigravity", "DeepSeek Coder (DSH)"];
        let adapters = ["antigravity", "dsh-desktop"];
        let bundles = ["com.google.antigravity", "com.deepseek.dsh"];
        let mut hosts = Vec::new();
        for index in 0..2 {
            let BoardLayerRoute::HostProfile { profile_id } =
                plan.route(BoardLayer::ALL[index + 1])
            else {
                return Err(format!("第 {} 层必须由 Micro Input 控制", index + 2));
            };
            let profile = doc
                .editable
                .graph
                .profiles
                .get(profile_id)
                .ok_or("托管方案已缺失")?;
            if profile.name != names[index]
                || profile
                    .layer_order
                    .iter()
                    .any(|id| doc.is_protected_oai_layer(id))
            {
                return Err(format!(
                    "第 {} 层必须为 {} 的托管方案",
                    index + 2,
                    names[index]
                ));
            }
            let actions = std::array::from_fn(|slot| {
                let mut action = resolve_host_action_with_layout(
                    doc,
                    Some(profile_id),
                    CONTROL_BINDINGS[slot],
                    true,
                    command_execution,
                    layout,
                )?;
                if let ResolvedHostAction::AgentCommand { adapter, command } = &mut action {
                    if adapter == "host" && !crate::is_host_mouse_command(command) {
                        *adapter = adapters[index].into();
                    }
                    if matches!(command.as_str(), "push-to-talk" | "dictation") {
                        *command = "toggle-dictation".into();
                    }
                }
                match &mut action {
                    ResolvedHostAction::Shortcut {
                        target_bundle_id, ..
                    } => {
                        if target_bundle_id.is_none() {
                            *target_bundle_id = Some(bundles[index].into());
                        }
                    }
                    ResolvedHostAction::PasteText {
                        target_bundle_id,
                        adapter,
                        ..
                    } => {
                        if target_bundle_id.is_none() {
                            *target_bundle_id = Some(bundles[index].into());
                        }
                        if adapter.is_none() {
                            *adapter = Some(adapters[index].into());
                        }
                    }
                    _ => {}
                }
                Some(action)
            });
            if actions.iter().flatten().any(|action| {
                matches!(
                    action,
                    ResolvedHostAction::MacroSequence { .. }
                        | ResolvedHostAction::Shortcut { key_code: 0x3f, .. }
                )
            }) {
                return Err(format!(
                    "{} 含需要全局按住或宏的动作，六层点击模式不支持",
                    names[index]
                ));
            }
            hosts.push(AppliedHostLayer {
                profile_id: profile_id.clone(),
                adapter: adapters[index].into(),
                actions,
            });
        }
        Ok(Self {
            desired: plan.clone(),
            bottom_row_layout: layout,
            hosts,
        })
    }
}

pub struct CompiledSixLayerProfile {
    pub profile_index: u32,
    pub keymap: Vec<u8>,
}

fn native_layout(layout: &Value) -> bool {
    let keys: Vec<_> = layout["keymap"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|row| row.as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .collect();
    let expected: Vec<_> = (0..13)
        .map(|slot| format!("KV_OAI_{}{:02}", if slot < 6 { "AG" } else { "ACT" }, slot))
        .collect();
    keys == expected
        && layout["encoders"] == json!([["KV_OAI_ENC_CC", "KV_OAI_ENC_CW", "KV_OAI_ENC_CLK"]])
        && layout["joystick"]["type"] == "VENDOR"
}

const LAYER2_VENDOR_SLOTS: [&str; 6] = [
    "KV_OAI_AG06",
    "KV_OAI_AG07",
    "KV_OAI_AG08",
    "KV_OAI_AG09",
    "KV_OAI_AG10",
    "KV_OAI_AG11",
];

const LAYER3_VENDOR_SLOTS: [&str; 6] = [
    "KV_OAI_AG12",
    "KV_OAI_AG13",
    "KV_OAI_AG14",
    "KV_OAI_AG15",
    "KV_OAI_AG16",
    "KV_OAI_AG17",
];

fn managed_lights_off() -> Value {
    json!({
        "backlight": {
            "effect": "off",
            "brightness": 0.0,
            "speed": 0.5,
            "magic": 1.0,
            "color": 0
        },
        "underglow": {
            "effect": "off",
            "brightness": 0.0,
            "speed": 0.55,
            "magic": 1.0,
            "color": 0
        }
    })
}

fn managed_layer(index: usize) -> Value {
    let tokens: Vec<&str> = match index {
        1 => LAYER2_VENDOR_SLOTS
            .into_iter()
            .chain(DIRECT_CONTROLS[6..20].iter().map(|control| control.token))
            .collect(),
        2 => LAYER3_VENDOR_SLOTS
            .into_iter()
            .chain(DIRECT_CONTROLS[26..40].iter().map(|control| control.token))
            .collect(),
        _ => vec!["KC_NONE"; 20],
    };
    let rows = [
        &tokens[0..2],
        &tokens[2..6],
        &tokens[6..10],
        &tokens[10..13],
    ];
    let name = [
        "Codex",
        "Antigravity",
        "DeepSeek Harness",
        "Unassigned 4",
        "Unassigned 5",
        "Unassigned 6",
    ][index];
    json!({
        "id": index,
        "name": name,
        "color": 0,
        "lights": managed_lights_off(),
        "layout": {
            "base": rows,
            "keymap": rows,
            "encoders": [&tokens[13..16]],
            "joystick": {
                "type": "RADIAL",
                "sectors": [
                    {"k": tokens[16], "a1": 0.625, "a2": 0.875},
                    {"k": tokens[17], "a1": 0.375, "a2": 0.625},
                    {"k": tokens[18], "a1": 0.125, "a2": 0.375},
                    {"k": tokens[19], "a1": 0.875, "a2": 0.125}
                ]
            }
        }
    })
}

fn offset(input: &[u8], raw: &RawValue) -> Result<usize, String> {
    let start = (raw.get().as_ptr() as usize)
        .checked_sub(input.as_ptr() as usize)
        .ok_or("原字节范围无效")?;
    if input.get(start..start + raw.get().len()) != Some(raw.get().as_bytes()) {
        return Err("原字节范围不一致".into());
    }
    Ok(start)
}

fn is_allowed_managed_vendor_token(
    layer_index: usize,
    row_index: usize,
    col_index: usize,
    token: &str,
) -> bool {
    let in_top_six = (row_index == 0 && col_index < 2) || (row_index == 1 && col_index < 4);
    in_top_six
        && ((layer_index == 1 && LAYER2_VENDOR_SLOTS.contains(&token))
            || (layer_index == 2 && LAYER3_VENDOR_SLOTS.contains(&token)))
}

fn grid_has_conflicting_vendor_token(grid: &Value, allow_top_six_for_layer: Option<usize>) -> bool {
    let Some(rows) = grid.as_array() else {
        return false;
    };
    for (row_index, row) in rows.iter().enumerate() {
        if let Some(cols) = row.as_array() {
            for (col_index, cell) in cols.iter().enumerate() {
                if let Some(token) = cell.as_str() {
                    if token.starts_with("KV_OAI_")
                        && !allow_top_six_for_layer.is_some_and(|layer_index| {
                            is_allowed_managed_vendor_token(
                                layer_index,
                                row_index,
                                col_index,
                                token,
                            )
                        })
                    {
                        return true;
                    }
                }
            }
        } else if row
            .as_str()
            .is_some_and(|token| token.starts_with("KV_OAI_"))
        {
            return true;
        }
    }
    false
}

fn managed_layer_has_conflict(layer_index: usize, layer: &Value) -> bool {
    if layer["name"] == "Cindy" {
        return true;
    }
    let layout = &layer["layout"];
    if layout["joystick"]["type"] == "VENDOR" {
        return true;
    }
    if grid_has_conflicting_vendor_token(&layout["keymap"], Some(layer_index))
        || grid_has_conflicting_vendor_token(&layout["base"], Some(layer_index))
        || grid_has_conflicting_vendor_token(&layout["encoders"], None)
    {
        return true;
    }
    layout["joystick"]["sectors"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|sector| sector["k"].as_str())
        .any(|token| token.starts_with("KV_OAI_"))
}

fn recognize_six_layer_recovery_candidate_in_config(
    config: &crate::DeviceConfig,
) -> Result<Option<u32>, String> {
    let profiles = config.raw["profiles"]
        .as_array()
        .ok_or("Profile 列表缺失")?;
    let mut found = None;
    for (index, profile) in profiles.iter().enumerate() {
        if profile["name"] != SIX_LAYER_PROFILE_NAME {
            continue;
        }
        let layers = profile["layers"]
            .as_array()
            .ok_or("六层专用方案同名、重复或已修改，原映射保留")?;
        if found.is_some()
            || profile["id"] != index
            || profiles.iter().filter(|p| p["id"] == index).count() != 1
            || layers.len() != 6
            || !native_layout(&layers[0]["layout"])
        {
            return Err("六层专用方案同名、重复或已修改，原映射保留".into());
        }
        found = Some(index as u32);
    }
    Ok(found)
}

fn recognize_six_layer_profile_in_config(
    config: &crate::DeviceConfig,
) -> Result<Option<u32>, String> {
    let Some(index) = recognize_six_layer_recovery_candidate_in_config(config)? else {
        return Ok(None);
    };
    let layers = config.raw["profiles"][index as usize]["layers"]
        .as_array()
        .ok_or("六层专用方案同名、重复或已修改，原映射保留")?;
    if (1..6).all(|i| layers[i] == managed_layer(i)) {
        Ok(Some(index))
    } else {
        Err("六层专用方案同名、重复或已修改，原映射保留".into())
    }
}

pub fn recognize_six_layer_profile(
    keymap: &[u8],
    smart: Option<&[u8]>,
) -> Result<Option<u32>, String> {
    let config = parse_device_files(keymap, smart).map_err(|e| e.to_string())?;
    recognize_six_layer_profile_in_config(&config)
}

pub fn verify_active_six_layer_profile(
    keymap: &[u8],
    smart: Option<&[u8]>,
    profile_index: u32,
) -> Result<(), String> {
    let config = parse_device_files(keymap, smart).map_err(|e| e.to_string())?;
    let recognized_index = recognize_six_layer_profile_in_config(&config)?
        .ok_or_else(|| "六层专用方案缺失".to_string())?;
    if recognized_index != profile_index
        || config.raw["activeProfileId"].as_u64() != Some(u64::from(profile_index))
    {
        return Err("六层活动方案索引不符".into());
    }
    Ok(())
}

pub fn recognize_six_layer_recovery_candidate(
    keymap: &[u8],
    smart: Option<&[u8]>,
) -> Result<Option<u32>, String> {
    let config = parse_device_files(keymap, smart).map_err(|e| e.to_string())?;
    recognize_six_layer_recovery_candidate_in_config(&config)
}

pub fn compile_six_layer_profile(
    keymap: &[u8],
    smart: Option<&[u8]>,
) -> Result<CompiledSixLayerProfile, String> {
    let config = parse_device_files(keymap, smart).map_err(|e| e.to_string())?;
    let existing = recognize_six_layer_recovery_candidate_in_config(&config)?;
    let profiles = config.raw["profiles"]
        .as_array()
        .ok_or("Profile 列表缺失")?;
    if !config.raw["activeProfileId"]
        .as_u64()
        .is_some_and(|index| (index as usize) < profiles.len())
    {
        return Err("原活动 Profile 不可识别".into());
    }
    let root: &RawValue = serde_json::from_slice(keymap).map_err(|e| e.to_string())?;
    let fields = object(root)?;
    let profiles_raw = field(&fields, "profiles")?;
    let originals = array(profiles_raw)?;
    let mut edits = Vec::new();
    let index = match existing {
        Some(index) => {
            let layers = profiles[index as usize]["layers"]
                .as_array()
                .ok_or("六层数组缺失")?;
            let has_conflict = (1..6)
                .any(|layer_index| managed_layer_has_conflict(layer_index, &layers[layer_index]));
            if has_conflict {
                return Err("六层专用方案同名、重复或已修改，原映射保留".into());
            }
            if !(1..6).all(|i| layers[i] == managed_layer(i)) {
                let target_profile = originals
                    .get(index as usize)
                    .copied()
                    .ok_or("六层 Profile 原字节缺失")?;
                let profile_fields = object(target_profile)?;
                let layers_raw = field(&profile_fields, "layers")?;
                let raw_layers = array(layers_raw)?;
                let codex_raw = raw_layers.first().copied().ok_or("第 1 层原字节缺失")?;
                let mut upgraded_layers = vec![codex_raw.get().to_owned()];
                upgraded_layers.extend((1..6).map(|i| managed_layer(i).to_string()));
                let start = offset(keymap, layers_raw)?;
                edits.push((
                    start,
                    start + layers_raw.get().len(),
                    format!("[{}]", upgraded_layers.join(",")).into_bytes(),
                ));
            }
            index
        }
        None => {
            let mut native = Vec::new();
            for (profile, raw) in profiles.iter().zip(&originals) {
                let profile_fields = object(raw)?;
                let raw_layers = array(field(&profile_fields, "layers")?)?;
                for (layer, raw_layer) in profile["layers"]
                    .as_array()
                    .ok_or("Layer 列表缺失")?
                    .iter()
                    .zip(raw_layers)
                {
                    if native_layout(&layer["layout"]) {
                        native.push(raw_layer);
                    }
                }
            }
            if native.len() != 1 {
                return Err("未找到唯一已核验的 Codex 原生布局，原映射保留".into());
            }
            let index = u32::try_from(profiles.len()).map_err(|_| "Profile 数量超界")?;
            if profiles.iter().any(|profile| profile["id"] == index) {
                return Err("新增 Profile ID 与原方案重复".into());
            }
            let mut layers = vec![native[0].get().to_owned()];
            layers.extend((1..6).map(|i| managed_layer(i).to_string()));
            let profile = format!(
                "{{\"id\":{index},\"name\":\"{SIX_LAYER_PROFILE_NAME}\",\"layers\":[{}]}}",
                layers.join(",")
            );
            let end = offset(keymap, profiles_raw)? + profiles_raw.get().len() - 1;
            edits.push((end, end, format!(",{profile}").into_bytes()));
            index
        }
    };
    let active = field(&fields, "activeProfileId")?;
    let start = offset(keymap, active)?;
    edits.push((
        start,
        start + active.get().len(),
        index.to_string().into_bytes(),
    ));
    edits.sort_by_key(|edit| edit.0);
    let mut next = Vec::new();
    let mut cursor = 0;
    for (start, end, replacement) in edits {
        next.extend_from_slice(&keymap[cursor..start]);
        next.extend(replacement);
        cursor = end;
    }
    next.extend_from_slice(&keymap[cursor..]);
    if recognize_six_layer_profile(&next, smart)? != Some(index) {
        return Err("六层候选自检失败".into());
    }
    Ok(CompiledSixLayerProfile {
        profile_index: index,
        keymap: next,
    })
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    const SOURCE: &[u8] = include_bytes!("../fixtures/device/synthetic-six-layer-source.json");

    fn build_outdated_keymap(
        source: &[u8],
        customize_layer: impl Fn(usize, &mut Value),
    ) -> Vec<u8> {
        let root: &RawValue = serde_json::from_slice(source).unwrap();
        let fields = object(root).unwrap();
        let profiles_raw = field(&fields, "profiles").unwrap();
        let originals = array(profiles_raw).unwrap();
        let profile0_fields = object(originals[0]).unwrap();
        let raw_layers = array(field(&profile0_fields, "layers").unwrap()).unwrap();
        let mut layers = vec![raw_layers[0].get().to_owned()];
        for i in 1..6 {
            let mut layer = managed_layer(i);
            customize_layer(i, &mut layer);
            layers.push(layer.to_string());
        }
        let profile = format!(
            "{{\"id\":1,\"name\":\"{SIX_LAYER_PROFILE_NAME}\",\"layers\":[{}]}}",
            layers.join(",")
        );
        let end = offset(source, profiles_raw).unwrap() + profiles_raw.get().len() - 1;
        let active = field(&fields, "activeProfileId").unwrap();
        let active_start = offset(source, active).unwrap();
        let mut edits = vec![
            (end, end, format!(",{profile}").into_bytes()),
            (
                active_start,
                active_start + active.get().len(),
                b"1".to_vec(),
            ),
        ];
        edits.sort_by_key(|e| e.0);
        let mut out = Vec::new();
        let mut cursor = 0;
        for (s, e, r) in edits {
            out.extend_from_slice(&source[cursor..s]);
            out.extend(r);
            cursor = e;
        }
        out.extend_from_slice(&source[cursor..]);
        out
    }

    #[test]
    fn managed_layers_encode_hybrid_vendor_slots_and_lights_off() {
        assert_eq!(recognize_six_layer_profile(SOURCE, None).unwrap(), None);
        assert_eq!(
            verify_active_six_layer_profile(SOURCE, None, 1).unwrap_err(),
            "六层专用方案缺失"
        );
        let compiled = compile_six_layer_profile(SOURCE, None).unwrap();
        assert_eq!(
            recognize_six_layer_profile(&compiled.keymap, None).unwrap(),
            Some(1)
        );
        verify_active_six_layer_profile(&compiled.keymap, None, 1).unwrap();
        assert_eq!(
            verify_active_six_layer_profile(&compiled.keymap, None, 0).unwrap_err(),
            "六层活动方案索引不符"
        );
        let parsed: Value = serde_json::from_slice(&compiled.keymap).unwrap();
        let layers = parsed["profiles"][1]["layers"].as_array().unwrap();
        for layer in &layers[1..6] {
            assert_eq!(layer["lights"]["backlight"]["effect"], "off");
            assert_eq!(layer["lights"]["backlight"]["brightness"], 0.0);
            assert_eq!(layer["lights"]["underglow"]["effect"], "off");
            assert_eq!(layer["lights"]["underglow"]["brightness"], 0.0);
        }
        assert_eq!(
            layers[1]["layout"]["keymap"][0],
            json!(["KV_OAI_AG06", "KV_OAI_AG07"])
        );
        assert_eq!(
            layers[1]["layout"]["keymap"][1],
            json!(["KV_OAI_AG08", "KV_OAI_AG09", "KV_OAI_AG10", "KV_OAI_AG11"])
        );
        assert_eq!(
            layers[2]["layout"]["keymap"][0],
            json!(["KV_OAI_AG12", "KV_OAI_AG13"])
        );
        assert_eq!(
            layers[2]["layout"]["keymap"][1],
            json!(["KV_OAI_AG14", "KV_OAI_AG15", "KV_OAI_AG16", "KV_OAI_AG17"])
        );
    }

    #[test]
    fn outdated_managed_layers_destructively_upgrade_in_place_preserving_codex_layer_zero_bytes() {
        let outdated = build_outdated_keymap(SOURCE, |index, layer| {
            layer.as_object_mut().unwrap().remove("lights");
            if index == 1 {
                layer["layout"]["keymap"][0] = json!(["KC_F13", "KC_F14"]);
            } else if index == 4 {
                layer["name"] = json!("Old Slot 5");
                layer["layout"]["keymap"][0] = json!(["KC_A", "KC_B"]);
            }
        });
        assert!(recognize_six_layer_profile(&outdated, None).is_err());
        assert!(verify_active_six_layer_profile(&outdated, None, 1).is_err());
        assert_eq!(
            recognize_six_layer_recovery_candidate(&outdated, None).unwrap(),
            Some(1)
        );

        let upgraded = compile_six_layer_profile(&outdated, None).unwrap();
        assert_eq!(upgraded.profile_index, 1);
        let parsed: Value = serde_json::from_slice(&upgraded.keymap).unwrap();
        assert_eq!(parsed["profiles"].as_array().unwrap().len(), 2);
        assert_eq!(parsed["activeProfileId"], 1);
        assert_eq!(
            recognize_six_layer_profile(&upgraded.keymap, None).unwrap(),
            Some(1)
        );
        verify_active_six_layer_profile(&upgraded.keymap, None, 1).unwrap();

        let old_root: &RawValue = serde_json::from_slice(&outdated).unwrap();
        let old_profiles = array(field(&object(old_root).unwrap(), "profiles").unwrap()).unwrap();
        let old_l0 = array(field(&object(old_profiles[1]).unwrap(), "layers").unwrap()).unwrap()[0];

        let new_root: &RawValue = serde_json::from_slice(&upgraded.keymap).unwrap();
        let new_profiles = array(field(&object(new_root).unwrap(), "profiles").unwrap()).unwrap();
        let new_l0 = array(field(&object(new_profiles[1]).unwrap(), "layers").unwrap()).unwrap()[0];

        assert_eq!(new_l0.get(), old_l0.get());
        assert!(new_l0.get().contains("\"unknownNativeField\": 1.00"));
    }

    #[test]
    fn retired_cindy_and_conflicting_native_tokens_are_recoverable_but_rejected_by_compiler() {
        let cindy_bytes = build_outdated_keymap(SOURCE, |index, layer| {
            if index == 3 {
                layer["name"] = json!("Cindy");
            }
        });
        assert!(recognize_six_layer_profile(&cindy_bytes, None).is_err());
        assert!(compile_six_layer_profile(&cindy_bytes, None).is_err());
        assert_eq!(
            recognize_six_layer_recovery_candidate(&cindy_bytes, None).unwrap(),
            Some(1)
        );

        let act_conflict = build_outdated_keymap(SOURCE, |index, layer| {
            if index == 2 {
                layer["layout"]["keymap"][2][0] = json!("KV_OAI_ACT06");
            }
        });
        assert!(recognize_six_layer_profile(&act_conflict, None).is_err());
        assert!(compile_six_layer_profile(&act_conflict, None).is_err());
        assert_eq!(
            recognize_six_layer_recovery_candidate(&act_conflict, None).unwrap(),
            Some(1)
        );

        let ag00_conflict = build_outdated_keymap(SOURCE, |index, layer| {
            if index == 2 {
                layer["layout"]["keymap"][0][0] = json!("KV_OAI_AG00");
            }
        });
        assert!(recognize_six_layer_profile(&ag00_conflict, None).is_err());
        assert!(compile_six_layer_profile(&ag00_conflict, None).is_err());
        assert_eq!(
            recognize_six_layer_recovery_candidate(&ag00_conflict, None).unwrap(),
            Some(1)
        );

        let vendor_joystick_conflict = build_outdated_keymap(SOURCE, |index, layer| {
            if index == 2 {
                layer["layout"]["joystick"]["type"] = json!("VENDOR");
            }
        });
        assert!(recognize_six_layer_profile(&vendor_joystick_conflict, None).is_err());
        assert!(compile_six_layer_profile(&vendor_joystick_conflict, None).is_err());
        assert_eq!(
            recognize_six_layer_recovery_candidate(&vendor_joystick_conflict, None).unwrap(),
            Some(1)
        );

        let mut dup_id_doc: Value =
            serde_json::from_slice(&compile_six_layer_profile(SOURCE, None).unwrap().keymap)
                .unwrap();
        dup_id_doc["profiles"][0]["id"] = json!(1);
        let dup_id_bytes = serde_json::to_vec(&dup_id_doc).unwrap();
        assert!(recognize_six_layer_recovery_candidate(&dup_id_bytes, None).is_err());
        assert!(recognize_six_layer_profile(&dup_id_bytes, None).is_err());
        assert!(compile_six_layer_profile(&dup_id_bytes, None).is_err());

        let benign_name = build_outdated_keymap(SOURCE, |index, layer| {
            if index == 2 {
                layer["name"] = json!("My KV_OAI_ACT Layer");
            }
        });
        assert_eq!(
            recognize_six_layer_recovery_candidate(&benign_name, None).unwrap(),
            Some(1)
        );
        let upgraded_benign = compile_six_layer_profile(&benign_name, None).unwrap();
        assert_eq!(
            recognize_six_layer_profile(&upgraded_benign.keymap, None).unwrap(),
            Some(1)
        );
    }
}
