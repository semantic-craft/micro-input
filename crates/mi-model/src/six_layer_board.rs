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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SixLayerKeymapRevision {
    #[default]
    V2HybridVendorSlots,
    V1StandardKeys,
    RetiredCindy,
}

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
                    if adapter == "host" {
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

const V2_LAYER2_VENDOR_SLOTS: [&str; 6] = [
    "KV_OAI_AG06",
    "KV_OAI_AG07",
    "KV_OAI_AG08",
    "KV_OAI_AG09",
    "KV_OAI_AG10",
    "KV_OAI_AG11",
];

const V2_LAYER3_VENDOR_SLOTS: [&str; 6] = [
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
        1 => V2_LAYER2_VENDOR_SLOTS
            .into_iter()
            .chain(DIRECT_CONTROLS[6..20].iter().map(|control| control.token))
            .collect(),
        2 => V2_LAYER3_VENDOR_SLOTS
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

fn managed_layer_v1(index: usize, retired: bool) -> Value {
    let tokens: Vec<_> = if (1..=2).contains(&index) || (retired && index == 3) {
        DIRECT_CONTROLS[(index - 1) * 20..index * 20]
            .iter()
            .map(|control| control.token)
            .collect()
    } else {
        vec!["KC_NONE"; 20]
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
    let name = if retired && index == 3 { "Cindy" } else { name };
    json!({"id":index,"name":name, "color":0,
        "layout":{"base":rows,"keymap":rows,"encoders":[&tokens[13..16]],"joystick":{"type":"RADIAL","sectors":[{"k":tokens[16],"a1":0.625,"a2":0.875},{"k":tokens[17],"a1":0.375,"a2":0.625},{"k":tokens[18],"a1":0.125,"a2":0.375},{"k":tokens[19],"a1":0.875,"a2":0.125}]}}})
}

fn is_retired_cindy_layer_3(layer: &Value) -> bool {
    let v1_cindy = managed_layer_v1(3, true);
    if layer == &v1_cindy {
        return true;
    }
    let mut v2_cindy = v1_cindy;
    v2_cindy["lights"] = managed_lights_off();
    layer == &v2_cindy
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

pub fn recognize_six_layer_profile_revision(
    keymap: &[u8],
    smart: Option<&[u8]>,
) -> Result<Option<(u32, SixLayerKeymapRevision)>, String> {
    let config = parse_device_files(keymap, smart).map_err(|e| e.to_string())?;
    recognize_profile_revision_in_config(&config, false)
}

pub fn verify_active_six_layer_profile_revision(
    keymap: &[u8],
    smart: Option<&[u8]>,
    profile_index: u32,
) -> Result<SixLayerKeymapRevision, String> {
    let config = parse_device_files(keymap, smart).map_err(|e| e.to_string())?;
    let (recognized_index, revision) = recognize_profile_revision_in_config(&config, false)?
        .ok_or_else(|| "六层专用方案缺失".to_string())?;
    if recognized_index != profile_index
        || config.raw["activeProfileId"].as_u64() != Some(u64::from(profile_index))
    {
        return Err("六层活动方案索引不符".into());
    }
    Ok(revision)
}

pub fn recognize_six_layer_profile(
    keymap: &[u8],
    smart: Option<&[u8]>,
) -> Result<Option<u32>, String> {
    match recognize_six_layer_profile_revision(keymap, smart)? {
        Some((index, SixLayerKeymapRevision::V2HybridVendorSlots)) => Ok(Some(index)),
        Some(_) => Err("六层专用方案同名、重复或已修改，原映射保留".into()),
        None => Ok(None),
    }
}

pub fn recognize_six_layer_recovery_revision(
    keymap: &[u8],
    smart: Option<&[u8]>,
) -> Result<Option<(u32, SixLayerKeymapRevision)>, String> {
    let config = parse_device_files(keymap, smart).map_err(|e| e.to_string())?;
    recognize_profile_revision_in_config(&config, true)
}

pub fn recognize_six_layer_recovery_candidate(
    keymap: &[u8],
    smart: Option<&[u8]>,
) -> Result<Option<u32>, String> {
    Ok(recognize_six_layer_recovery_revision(keymap, smart)?.map(|(index, _)| index))
}

fn recognize_profile_revision_in_config(
    config: &crate::DeviceConfig,
    allow_retired: bool,
) -> Result<Option<(u32, SixLayerKeymapRevision)>, String> {
    let profiles = config.raw["profiles"]
        .as_array()
        .ok_or("Profile 列表缺失")?;
    let mut found = None;
    for (index, profile) in profiles.iter().enumerate() {
        if profile["name"] != SIX_LAYER_PROFILE_NAME {
            continue;
        }
        let layers = profile["layers"].as_array().ok_or("六层数组缺失")?;
        if found.is_some()
            || profile["id"] != index
            || layers.len() != 6
            || !native_layout(&layers[0]["layout"])
        {
            return Err("六层专用方案同名、重复或已修改，原映射保留".into());
        }
        let revision = if (1..6).all(|i| layers[i] == managed_layer(i)) {
            SixLayerKeymapRevision::V2HybridVendorSlots
        } else if (1..6).all(|i| layers[i] == managed_layer_v1(i, false)) {
            SixLayerKeymapRevision::V1StandardKeys
        } else if allow_retired
            && is_retired_cindy_layer_3(&layers[3])
            && [1, 2, 4, 5]
                .into_iter()
                .all(|i| layers[i] == managed_layer_v1(i, false) || layers[i] == managed_layer(i))
        {
            SixLayerKeymapRevision::RetiredCindy
        } else {
            return Err("六层专用方案同名、重复或已修改，原映射保留".into());
        };
        found = Some((index as u32, revision));
    }
    Ok(found)
}

pub fn compile_six_layer_profile(
    keymap: &[u8],
    smart: Option<&[u8]>,
) -> Result<CompiledSixLayerProfile, String> {
    let config = parse_device_files(keymap, smart).map_err(|e| e.to_string())?;
    let known = recognize_profile_revision_in_config(&config, false)?;
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
    let index = match known {
        Some((index, SixLayerKeymapRevision::V2HybridVendorSlots)) => index,
        Some((index, SixLayerKeymapRevision::V1StandardKeys)) => {
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
            index
        }
        Some((_, SixLayerKeymapRevision::RetiredCindy)) => {
            return Err("六层专用方案同名、重复或已修改，原映射保留".into());
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

    fn build_v1_keymap(source: &[u8]) -> Vec<u8> {
        let root: &RawValue = serde_json::from_slice(source).unwrap();
        let fields = object(root).unwrap();
        let profiles_raw = field(&fields, "profiles").unwrap();
        let originals = array(profiles_raw).unwrap();
        let profile0_fields = object(originals[0]).unwrap();
        let raw_layers = array(field(&profile0_fields, "layers").unwrap()).unwrap();
        let mut layers = vec![raw_layers[0].get().to_owned()];
        layers.extend((1..6).map(|i| managed_layer_v1(i, false).to_string()));
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
    fn v2_managed_layers_encode_hybrid_vendor_slots_and_lights_off() {
        let compiled = compile_six_layer_profile(SOURCE, None).unwrap();
        assert_eq!(
            recognize_six_layer_profile_revision(&compiled.keymap, None).unwrap(),
            Some((1, SixLayerKeymapRevision::V2HybridVendorSlots))
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
    fn v1_standard_keys_upgrades_in_place_to_v2_preserving_codex_layer_zero_bytes() {
        let v1 = build_v1_keymap(SOURCE);
        assert_eq!(
            recognize_six_layer_profile_revision(&v1, None).unwrap(),
            Some((1, SixLayerKeymapRevision::V1StandardKeys))
        );
        assert!(recognize_six_layer_profile(&v1, None).is_err());
        assert_eq!(
            recognize_six_layer_recovery_revision(&v1, None).unwrap(),
            Some((1, SixLayerKeymapRevision::V1StandardKeys))
        );

        let upgraded = compile_six_layer_profile(&v1, None).unwrap();
        assert_eq!(upgraded.profile_index, 1);
        let parsed: Value = serde_json::from_slice(&upgraded.keymap).unwrap();
        assert_eq!(parsed["profiles"].as_array().unwrap().len(), 2);
        assert_eq!(parsed["activeProfileId"], 1);
        assert_eq!(
            recognize_six_layer_profile_revision(&upgraded.keymap, None).unwrap(),
            Some((1, SixLayerKeymapRevision::V2HybridVendorSlots))
        );

        let v1_root: &RawValue = serde_json::from_slice(&v1).unwrap();
        let v1_profiles = array(field(&object(v1_root).unwrap(), "profiles").unwrap()).unwrap();
        let v1_l0 = array(field(&object(v1_profiles[1]).unwrap(), "layers").unwrap()).unwrap()[0];

        let v2_root: &RawValue = serde_json::from_slice(&upgraded.keymap).unwrap();
        let v2_profiles = array(field(&object(v2_root).unwrap(), "profiles").unwrap()).unwrap();
        let v2_l0 = array(field(&object(v2_profiles[1]).unwrap(), "layers").unwrap()).unwrap()[0];

        assert_eq!(v2_l0.get(), v1_l0.get());
        assert!(v2_l0.get().contains("\"unknownNativeField\": 1.00"));
    }

    #[test]
    fn retired_cindy_is_recognized_for_recovery_only_and_rejected_by_compiler() {
        let v1 = build_v1_keymap(SOURCE);
        let mut cindy_val: Value = serde_json::from_slice(&v1).unwrap();
        cindy_val["profiles"][1]["layers"][3] = managed_layer_v1(3, true);
        let cindy_bytes = serde_json::to_vec(&cindy_val).unwrap();

        assert!(recognize_six_layer_profile_revision(&cindy_bytes, None).is_err());
        assert!(recognize_six_layer_profile(&cindy_bytes, None).is_err());
        assert!(compile_six_layer_profile(&cindy_bytes, None).is_err());
        assert_eq!(
            recognize_six_layer_recovery_revision(&cindy_bytes, None).unwrap(),
            Some((1, SixLayerKeymapRevision::RetiredCindy))
        );
        assert_eq!(
            recognize_six_layer_recovery_candidate(&cindy_bytes, None).unwrap(),
            Some(1)
        );
    }
}
