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

fn managed_layer(index: usize, retired: bool) -> Value {
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

fn offset(input: &[u8], raw: &RawValue) -> Result<usize, String> {
    let start = (raw.get().as_ptr() as usize)
        .checked_sub(input.as_ptr() as usize)
        .ok_or("原字节范围无效")?;
    if input.get(start..start + raw.get().len()) != Some(raw.get().as_bytes()) {
        return Err("原字节范围不一致".into());
    }
    Ok(start)
}

pub fn recognize_six_layer_profile(
    keymap: &[u8],
    smart: Option<&[u8]>,
) -> Result<Option<u32>, String> {
    recognize_profile(keymap, smart, false)
}

pub fn recognize_six_layer_recovery_candidate(
    keymap: &[u8],
    smart: Option<&[u8]>,
) -> Result<Option<u32>, String> {
    recognize_profile(keymap, smart, true)
}

fn recognize_profile(
    keymap: &[u8],
    smart: Option<&[u8]>,
    allow_retired: bool,
) -> Result<Option<u32>, String> {
    let config = parse_device_files(keymap, smart).map_err(|e| e.to_string())?;
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
            || (1..6).any(|i| {
                layers[i] != managed_layer(i, false)
                    && !(allow_retired && i == 3 && layers[i] == managed_layer(i, true))
            })
        {
            return Err("六层专用方案同名、重复或已修改，原映射保留".into());
        }
        found = Some(index as u32);
    }
    Ok(found)
}

pub fn compile_six_layer_profile(
    keymap: &[u8],
    smart: Option<&[u8]>,
) -> Result<CompiledSixLayerProfile, String> {
    let known = recognize_six_layer_profile(keymap, smart)?;
    let config = parse_device_files(keymap, smart).map_err(|e| e.to_string())?;
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
    let index = if let Some(index) = known {
        index
    } else {
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
        layers.extend((1..6).map(|i| managed_layer(i, false).to_string()));
        let profile = format!(
            "{{\"id\":{index},\"name\":\"{SIX_LAYER_PROFILE_NAME}\",\"layers\":[{}]}}",
            layers.join(",")
        );
        let end = offset(keymap, profiles_raw)? + profiles_raw.get().len() - 1;
        edits.push((end, end, format!(",{profile}").into_bytes()));
        index
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
