use super::{array, field, object};
use crate::{
    board_layers::{BoardLayer, BoardLayerPlan, BoardLayerRoute, NativeLayerOwner},
    direct_controls::{CONTROL_BINDINGS, DIRECT_CONTROLS},
    document::LocalDocumentV2,
    host_router::{
        resolve_encoder_hold_action, resolve_host_action_with_layout, BottomRowLayout,
        ResolvedHostAction,
    },
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoder_hold_action: Option<ResolvedHostAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub joystick_mode: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AppliedSixLayerPlan {
    pub desired: BoardLayerPlan,
    pub bottom_row_layout: BottomRowLayout,
    pub hosts: Vec<AppliedHostLayer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_joystick_mode: Option<String>,
}

fn doc_firmware_version(doc: &LocalDocumentV2) -> Option<&str> {
    doc.baseline
        .as_ref()
        .and_then(|b| b.get("firmware"))
        .and_then(Value::as_str)
        .or_else(|| {
            doc.origin
                .get("config")
                .and_then(|c| c.get("firmware"))
                .and_then(Value::as_str)
        })
}

fn profile_hardware_joystick_mode(doc: &LocalDocumentV2, profile_id: &str) -> Option<String> {
    let lid = doc
        .editable
        .graph
        .profiles
        .get(profile_id)?
        .layer_order
        .first()?;
    let settings = doc
        .editable
        .graph
        .layers
        .get(lid)?
        .control_settings
        .as_ref()?;
    match settings.joystick_mode.as_str() {
        "mouse" => Some("MOUSE".into()),
        "scroll" => Some("SCROLL".into()),
        _ => None,
    }
}

fn codex_profile_hardware_joystick_mode(
    doc: &LocalDocumentV2,
    codex_profile_id: &str,
) -> Option<String> {
    if let Some(mode) = profile_hardware_joystick_mode(doc, codex_profile_id) {
        return Some(mode);
    }
    doc.editable
        .graph
        .profiles
        .iter()
        .find(|(_, profile)| {
            profile.name == crate::preset::LOCAL_CODEX_PROFILE_NAME
                && !profile
                    .layer_order
                    .iter()
                    .any(|lid| doc.is_protected_oai_layer(lid))
        })
        .and_then(|(pid, _)| profile_hardware_joystick_mode(doc, pid))
}

impl AppliedSixLayerPlan {
    pub fn board_layer_joystick_modes(&self) -> [Option<&str>; 6] {
        let codex = self
            .codex_joystick_mode
            .as_deref()
            .filter(|m| matches!(*m, "MOUSE" | "SCROLL"));
        let ag = self
            .hosts
            .first()
            .and_then(|h| h.joystick_mode.as_deref())
            .filter(|m| matches!(*m, "MOUSE" | "SCROLL"));
        let dsh = self
            .hosts
            .get(1)
            .and_then(|h| h.joystick_mode.as_deref())
            .filter(|m| matches!(*m, "MOUSE" | "SCROLL"));
        let unassigned = codex.or(ag).or(dsh);
        [codex, ag, dsh, unassigned, unassigned, unassigned]
    }

    pub fn uses_v070_joystick_hardware_mode(&self) -> bool {
        self.codex_joystick_mode
            .as_deref()
            .is_some_and(|m| matches!(m, "MOUSE" | "SCROLL"))
            || self.hosts.iter().any(|h| {
                h.joystick_mode
                    .as_deref()
                    .is_some_and(|m| matches!(m, "MOUSE" | "SCROLL"))
            })
    }

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
        if let Some(mode) = self.codex_joystick_mode.as_deref() {
            if !matches!(mode, "VENDOR" | "MOUSE" | "SCROLL") {
                return Err("六层摇杆硬件模式不受支持，请先恢复原设备配置".into());
            }
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
            if let Some(mode) = host.joystick_mode.as_deref() {
                if !matches!(mode, "RADIAL" | "MOUSE" | "SCROLL") {
                    return Err("六层摇杆硬件模式不受支持，请先恢复原设备配置".into());
                }
            }
        }
        Ok(())
    }

    pub fn validate_for_firmware(&self, firmware: Option<&str>) -> Result<(), String> {
        self.validate_current()?;
        if let Some(fw) = firmware {
            if self.uses_v070_joystick_hardware_mode() && !crate::supports_v070_mouse_tokens(fw) {
                return Err("原生摇杆 MOUSE/SCROLL 模式需要固件 0.7.0-rc.16 或更高版本".into());
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
            let mut layer = crate::official_chatgpt_layer(json!(0), "Codex");
            if let Some(mode) = self
                .codex_joystick_mode
                .as_deref()
                .filter(|m| matches!(*m, "MOUSE" | "SCROLL"))
            {
                layer.joystick = crate::Joystick {
                    kind: json!(mode),
                    sectors: vec![],
                };
            }
            return layer;
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
        let hardware_joystick_mode = if (1..=2).contains(&index) {
            self.hosts
                .get(index - 1)
                .and_then(|h| h.joystick_mode.as_deref())
                .filter(|m| matches!(*m, "MOUSE" | "SCROLL"))
        } else {
            self.codex_joystick_mode
                .as_deref()
                .or_else(|| self.hosts.iter().find_map(|h| h.joystick_mode.as_deref()))
                .filter(|m| matches!(*m, "MOUSE" | "SCROLL"))
        };
        let joystick = if let Some(mode) = hardware_joystick_mode {
            crate::Joystick {
                kind: json!(mode),
                sectors: vec![],
            }
        } else {
            crate::Joystick {
                kind: json!("RADIAL"),
                sectors: (16..20)
                    .map(|i| crate::Binding::Basic(crate::Token(tokens[i].into())))
                    .collect(),
            }
        };
        crate::Layer {
            id: json!(index),
            name: name.into(),
            color: json!(0),
            keys: std::array::from_fn(|i| crate::Binding::Basic(crate::Token(tokens[i].into()))),
            encoders: std::array::from_fn(|i| {
                crate::Binding::Basic(crate::Token(tokens[13 + i].into()))
            }),
            joystick,
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
            profile_id: codex_profile_id,
        } = plan.route(BoardLayer::ALL[0])
        else {
            return Err("第 1 层必须为 Codex 原生方案".into());
        };
        if crate::board_layers::native_owner(doc, codex_profile_id) != Some(NativeLayerOwner::Codex)
        {
            return Err("第 1 层未引用受保护的 Codex 原生方案".into());
        }
        if [3, 4, 5]
            .iter()
            .any(|&index| plan.route(BoardLayer::ALL[index]) != &BoardLayerRoute::Unassigned)
        {
            return Err("当前第 4、5、6 层必须留空，尚无额外输入编码".into());
        }
        let codex_joystick_mode = codex_profile_hardware_joystick_mode(doc, codex_profile_id);
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
            let joystick_mode = profile_hardware_joystick_mode(doc, profile_id);
            let normalize_action = |mut action: ResolvedHostAction| -> ResolvedHostAction {
                if let ResolvedHostAction::AgentCommand { adapter, command } = &mut action {
                    if matches!(adapter.as_str(), "host" | "active-agent")
                        && !crate::is_host_mouse_command(command)
                    {
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
                action
            };
            let actions = std::array::from_fn(|slot| {
                resolve_host_action_with_layout(
                    doc,
                    Some(profile_id),
                    CONTROL_BINDINGS[slot],
                    true,
                    command_execution,
                    layout,
                )
                .map(&normalize_action)
            });
            let encoder_hold_action =
                resolve_encoder_hold_action(doc, Some(profile_id), command_execution)
                    .map(&normalize_action);
            if actions
                .iter()
                .flatten()
                .chain(encoder_hold_action.iter())
                .any(|action| {
                    matches!(
                        action,
                        ResolvedHostAction::MacroSequence { .. }
                            | ResolvedHostAction::Shortcut { key_code: 0x3f, .. }
                    )
                })
            {
                return Err(format!(
                    "{} 含需要全局按住或宏的动作，六层点击模式不支持",
                    names[index]
                ));
            }
            hosts.push(AppliedHostLayer {
                profile_id: profile_id.clone(),
                adapter: adapters[index].into(),
                actions,
                encoder_hold_action,
                joystick_mode,
            });
        }
        let compiled = Self {
            desired: plan.clone(),
            bottom_row_layout: layout,
            hosts,
            codex_joystick_mode,
        };
        compiled.validate_for_firmware(doc_firmware_version(doc))?;
        Ok(compiled)
    }
}

#[derive(Debug)]
pub struct CompiledSixLayerProfile {
    pub profile_index: u32,
    pub keymap: Vec<u8>,
}

fn codex_native_keys_and_encoders(layout: &Value) -> bool {
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
}

fn joystick_is_v070_mouse_or_scroll(joystick: &Value, allowed_sectors: Option<&Value>) -> bool {
    let Some(obj) = joystick.as_object() else {
        return false;
    };
    let Some(kind) = obj.get("type").and_then(Value::as_str) else {
        return false;
    };
    if !matches!(kind, "MOUSE" | "SCROLL") {
        return false;
    }
    match obj.len() {
        1 => true,
        2 => match (obj.get("sectors"), allowed_sectors) {
            (Some(sectors), Some(expected)) => sectors == expected || sectors == &json!([]),
            (Some(sectors), None) => sectors == &json!([]),
            _ => false,
        },
        _ => false,
    }
}

fn native_layout_with_gate(layout: &Value, allow_v070_joystick: bool) -> bool {
    if !codex_native_keys_and_encoders(layout) {
        return false;
    }
    if layout["joystick"]["type"] == "VENDOR" {
        layout["joystick"]
            .get("sectors")
            .is_none_or(|sectors| sectors == &json!([]))
    } else if allow_v070_joystick {
        joystick_is_v070_mouse_or_scroll(&layout["joystick"], None)
    } else {
        false
    }
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

const SINGLE_LAYER_OAI_SECTORS: [&str; 4] =
    ["KV_OAI_AG15", "KV_OAI_AG16", "KV_OAI_AG17", "KV_OAI_AG18"];

fn four_sector_radial_joystick(tokens: [&str; 4]) -> Value {
    json!({
        "type": "RADIAL",
        "sectors": [
            {"k": tokens[0], "a1": 0.625, "a2": 0.875},
            {"k": tokens[1], "a1": 0.375, "a2": 0.625},
            {"k": tokens[2], "a1": 0.125, "a2": 0.375},
            {"k": tokens[3], "a1": 0.875, "a2": 0.125}
        ]
    })
}

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
    managed_layer_with_joystick_mode(index, None)
}

fn managed_layer_with_joystick_mode(index: usize, joystick_mode: Option<&str>) -> Value {
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
    let joystick = match joystick_mode {
        Some("MOUSE") => json!({"type": "MOUSE"}),
        Some("SCROLL") => json!({"type": "SCROLL"}),
        _ => four_sector_radial_joystick([tokens[16], tokens[17], tokens[18], tokens[19]]),
    };
    json!({
        "id": index,
        "name": name,
        "color": 0,
        "lights": managed_lights_off(),
        "layout": {
            "base": rows,
            "keymap": rows,
            "encoders": [&tokens[13..16]],
            "joystick": joystick
        }
    })
}

fn matches_canonical_managed_layer(index: usize, layer: &Value, allow_v070_joystick: bool) -> bool {
    let radial = managed_layer(index);
    if layer == &radial {
        return true;
    }
    if !allow_v070_joystick {
        return false;
    }
    if layer == &managed_layer_with_joystick_mode(index, Some("MOUSE"))
        || layer == &managed_layer_with_joystick_mode(index, Some("SCROLL"))
    {
        return true;
    }
    let allowed_sectors = &radial["layout"]["joystick"]["sectors"];
    if !joystick_is_v070_mouse_or_scroll(&layer["layout"]["joystick"], Some(allowed_sectors)) {
        return false;
    }
    let mut normalized = layer.clone();
    normalized["layout"]["joystick"] = radial["layout"]["joystick"].clone();
    normalized == radial
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
            || !native_layout_with_gate(&layers[0]["layout"], true)
        {
            return Err("六层专用方案同名、重复或已修改，原映射保留".into());
        }
        found = Some(index as u32);
    }
    Ok(found)
}

pub fn recognize_six_layer_profile_in_config(
    config: &crate::DeviceConfig,
) -> Result<Option<u32>, String> {
    let allow_v070_joystick = config
        .firmware
        .as_deref()
        .is_none_or(crate::supports_v070_mouse_tokens);
    let Some(index) = recognize_six_layer_recovery_candidate_in_config(config)? else {
        return Ok(None);
    };
    let layers = config.raw["profiles"][index as usize]["layers"]
        .as_array()
        .ok_or("六层专用方案同名、重复或已修改，原映射保留")?;
    let valid_for_gate = native_layout_with_gate(&layers[0]["layout"], allow_v070_joystick)
        && (1..6).all(|i| matches_canonical_managed_layer(i, &layers[i], allow_v070_joystick));
    if valid_for_gate {
        Ok(Some(index))
    } else if !allow_v070_joystick
        && native_layout_with_gate(&layers[0]["layout"], true)
        && (1..6).all(|i| matches_canonical_managed_layer(i, &layers[i], true))
    {
        Err("原生摇杆 MOUSE/SCROLL 模式需要固件 0.7.0-rc.16 或更高版本".into())
    } else {
        Err("六层专用方案同名、重复或已修改，原映射保留".into())
    }
}

pub fn recognize_six_layer_profile(
    keymap: &[u8],
    smart: Option<&[u8]>,
) -> Result<Option<u32>, String> {
    recognize_six_layer_profile_for_firmware(keymap, smart, None)
}

pub fn recognize_six_layer_profile_for_firmware(
    keymap: &[u8],
    smart: Option<&[u8]>,
    firmware: Option<&str>,
) -> Result<Option<u32>, String> {
    let mut config = parse_device_files(keymap, smart).map_err(|e| e.to_string())?;
    config.firmware = firmware.map(str::to_owned);
    recognize_six_layer_profile_in_config(&config)
}

pub fn verify_active_six_layer_profile(
    keymap: &[u8],
    smart: Option<&[u8]>,
    profile_index: u32,
) -> Result<(), String> {
    verify_active_six_layer_profile_for_firmware(keymap, smart, profile_index, None)
}

pub fn verify_active_six_layer_profile_for_firmware(
    keymap: &[u8],
    smart: Option<&[u8]>,
    profile_index: u32,
    firmware: Option<&str>,
) -> Result<(), String> {
    let mut config = parse_device_files(keymap, smart).map_err(|e| e.to_string())?;
    config.firmware = firmware.map(str::to_owned);
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

fn normalize_requested_six_layer_joystick_mode(
    mode: Option<&str>,
    firmware: Option<&str>,
) -> Result<Option<&'static str>, String> {
    let Some(raw) = mode else {
        return Ok(None);
    };
    let normalized = match raw.trim().to_ascii_lowercase().as_str() {
        "mouse" => Some("MOUSE"),
        "scroll" => Some("SCROLL"),
        "radial" | "sectors" | "logical_sectors" => None,
        other => return Err(format!("不支持的摇杆硬件模式：{other}")),
    };
    if normalized.is_some() && firmware.is_some_and(|fw| !crate::supports_v070_mouse_tokens(fw)) {
        return Err("原生摇杆 MOUSE/SCROLL 模式需要固件 0.7.0-rc.16 或更高版本".into());
    }
    Ok(normalized)
}

pub fn compile_six_layer_profile(
    keymap: &[u8],
    smart: Option<&[u8]>,
) -> Result<CompiledSixLayerProfile, String> {
    compile_six_layer_profile_for_firmware(keymap, smart, None, None)
}

pub fn compile_six_layer_profile_for_firmware(
    keymap: &[u8],
    smart: Option<&[u8]>,
    firmware: Option<&str>,
    joystick_mode: Option<&str>,
) -> Result<CompiledSixLayerProfile, String> {
    compile_six_layer_profile_for_firmware_inner(
        keymap,
        smart,
        firmware,
        joystick_mode.map(|mode| [Some(mode); 6]),
    )
}

pub fn compile_six_layer_profile_for_firmware_layers(
    keymap: &[u8],
    smart: Option<&[u8]>,
    firmware: Option<&str>,
    layer_joystick_modes: [Option<&str>; 6],
) -> Result<CompiledSixLayerProfile, String> {
    compile_six_layer_profile_for_firmware_inner(
        keymap,
        smart,
        firmware,
        Some(layer_joystick_modes),
    )
}

pub fn planned_keymap_matches_plan_joystick_modes(
    keymap: &[u8],
    profile_index: u32,
    plan: &AppliedSixLayerPlan,
    firmware: Option<&str>,
) -> bool {
    if firmware.is_some_and(|fw| !crate::supports_v070_mouse_tokens(fw)) {
        return true;
    }
    let Ok(config) = parse_device_files(keymap, None) else {
        return false;
    };
    let Some(layers) = config
        .raw
        .get("profiles")
        .and_then(Value::as_array)
        .and_then(|profiles| profiles.get(profile_index as usize))
        .and_then(|profile| profile.get("layers"))
        .and_then(Value::as_array)
    else {
        return false;
    };
    if layers.len() != 6 {
        return false;
    }
    let desired = plan.board_layer_joystick_modes();
    (0..3).all(|idx| {
        let expected_type = match desired[idx] {
            Some(mode) => mode,
            None if idx == 0 => "VENDOR",
            None => "RADIAL",
        };
        layers[idx]["layout"]["joystick"]["type"].as_str() == Some(expected_type)
    })
}

fn compile_six_layer_profile_for_firmware_inner(
    keymap: &[u8],
    smart: Option<&[u8]>,
    firmware: Option<&str>,
    layer_joystick_modes: Option<[Option<&str>; 6]>,
) -> Result<CompiledSixLayerProfile, String> {
    let requested_modes: Option<[Option<&'static str>; 6]> = match layer_joystick_modes {
        Some(modes) => Some([
            normalize_requested_six_layer_joystick_mode(modes[0], firmware)?,
            normalize_requested_six_layer_joystick_mode(modes[1], firmware)?,
            normalize_requested_six_layer_joystick_mode(modes[2], firmware)?,
            normalize_requested_six_layer_joystick_mode(modes[3], firmware)?,
            normalize_requested_six_layer_joystick_mode(modes[4], firmware)?,
            normalize_requested_six_layer_joystick_mode(modes[5], firmware)?,
        ]),
        None => None,
    };
    let allow_v070_joystick = firmware.is_none_or(crate::supports_v070_mouse_tokens);
    let mut config = parse_device_files(keymap, smart).map_err(|e| e.to_string())?;
    config.firmware = firmware.map(str::to_owned);
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
            if !native_layout_with_gate(&layers[0]["layout"], allow_v070_joystick) {
                if !allow_v070_joystick && native_layout_with_gate(&layers[0]["layout"], true) {
                    return Err("原生摇杆 MOUSE/SCROLL 模式需要固件 0.7.0-rc.16 或更高版本".into());
                }
                return Err("六层专用方案同名、重复或已修改，原映射保留".into());
            }
            let has_conflict = (1..6)
                .any(|layer_index| managed_layer_has_conflict(layer_index, &layers[layer_index]));
            if has_conflict {
                return Err("六层专用方案同名、重复或已修改，原映射保留".into());
            }
            if !allow_v070_joystick
                && (1..6).any(|i| {
                    matches!(
                        layers[i]["layout"]["joystick"]["type"].as_str(),
                        Some("MOUSE" | "SCROLL")
                    )
                })
            {
                return Err("原生摇杆 MOUSE/SCROLL 模式需要固件 0.7.0-rc.16 或更高版本".into());
            }
            let needs_managed_update = match requested_modes {
                Some(modes) => {
                    !(1..6).all(|i| layers[i] == managed_layer_with_joystick_mode(i, modes[i]))
                        || (modes[0].is_some()
                            && layers[0]["layout"]["joystick"]
                                != json!({"type": modes[0].unwrap()}))
                        || (modes[0].is_none()
                            && layers[0]["layout"]["joystick"]["type"] != "VENDOR")
                }
                None => !(1..6)
                    .all(|i| matches_canonical_managed_layer(i, &layers[i], allow_v070_joystick)),
            };
            if needs_managed_update {
                let target_profile = originals
                    .get(index as usize)
                    .copied()
                    .ok_or("六层 Profile 原字节缺失")?;
                let profile_fields = object(target_profile)?;
                let layers_raw = field(&profile_fields, "layers")?;
                let raw_layers = array(layers_raw)?;
                let codex_raw = raw_layers.first().copied().ok_or("第 1 层原字节缺失")?;
                let codex_layer_str = if let Some(modes) = requested_modes {
                    splice_raw_layer_joystick(
                        codex_raw,
                        &modes[0].map_or_else(
                            || json!({"type": "VENDOR", "sectors": []}),
                            |m| json!({"type": m}),
                        ),
                    )?
                } else {
                    codex_raw.get().to_owned()
                };
                let mut upgraded_layers = vec![codex_layer_str];
                for i in 1..6 {
                    let preserved_mode = if let Some(modes) = requested_modes {
                        modes[i]
                    } else {
                        layers[i]["layout"]["joystick"]["type"]
                            .as_str()
                            .filter(|m| allow_v070_joystick && matches!(*m, "MOUSE" | "SCROLL"))
                    };
                    upgraded_layers
                        .push(managed_layer_with_joystick_mode(i, preserved_mode).to_string());
                }
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
                    if native_layout_with_gate(&layer["layout"], allow_v070_joystick) {
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
            let codex_layer_str = if let Some(modes) = requested_modes {
                splice_raw_layer_joystick(
                    native[0],
                    &modes[0].map_or_else(
                        || json!({"type": "VENDOR", "sectors": []}),
                        |m| json!({"type": m}),
                    ),
                )?
            } else {
                native[0].get().to_owned()
            };
            let mut layers = vec![codex_layer_str];
            layers.extend((1..6).map(|i| {
                let mode = requested_modes.and_then(|modes| modes[i]);
                managed_layer_with_joystick_mode(i, mode).to_string()
            }));
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
    if recognize_six_layer_profile_for_firmware(&next, smart, firmware)? != Some(index) {
        return Err("六层候选自检失败".into());
    }
    Ok(CompiledSixLayerProfile {
        profile_index: index,
        keymap: next,
    })
}

fn splice_raw_layer_joystick(raw_layer: &RawValue, replacement: &Value) -> Result<String, String> {
    let bytes = raw_layer.get().as_bytes();
    let layer_fields = object(raw_layer)?;
    let layout_raw = field(&layer_fields, "layout")?;
    let layout_fields = object(layout_raw)?;
    let joystick_raw = field(&layout_fields, "joystick")?;
    let start = offset(bytes, joystick_raw)?;
    let end = start + joystick_raw.get().len();
    let current: Value = serde_json::from_str(joystick_raw.get()).map_err(|e| e.to_string())?;
    if &current == replacement {
        return Ok(raw_layer.get().to_owned());
    }
    let encoded = serde_json::to_string(replacement).map_err(|e| e.to_string())?;
    let mut out = String::with_capacity(bytes.len() - joystick_raw.get().len() + encoded.len());
    out.push_str(&raw_layer.get()[..start]);
    out.push_str(&encoded);
    out.push_str(&raw_layer.get()[end..]);
    Ok(out)
}

/// Inspects `(profile_index, layer_index)` in `/fs/keymap.json` (`layer_index` is 1-based when `>= 1`)
/// and returns normalized `"mouse" | "scroll" | "radial" | "vendor"`, or `None` if absent/unrecognized.
pub fn extract_keymap_joystick_hardware_mode(
    keymap_bytes: &[u8],
    active_profile: Option<u8>,
    active_layer: Option<u8>,
) -> Option<String> {
    let config = parse_device_files(keymap_bytes, None).ok()?;
    let profiles = config.raw.get("profiles")?.as_array()?;
    let profile_idx = if let Some(p) = active_profile {
        usize::from(p)
    } else {
        let active_id = config.raw.get("activeProfileId")?;
        let matching: Vec<usize> = profiles
            .iter()
            .enumerate()
            .filter_map(|(i, p)| (&p["id"] == active_id).then_some(i))
            .collect();
        if matching.len() == 1 {
            matching[0]
        } else {
            usize::try_from(active_id.as_u64()?).ok()?
        }
    };
    let layers = profiles.get(profile_idx)?.get("layers")?.as_array()?;
    let layer = match active_layer {
        Some(l) if l >= 1 => layers.get(usize::from(l - 1))?,
        _ => layers.first()?,
    };
    let raw_type = layer
        .get("layout")?
        .get("joystick")?
        .get("type")?
        .as_str()?;
    match raw_type.trim().to_ascii_uppercase().as_str() {
        "MOUSE" => Some("mouse".into()),
        "SCROLL" => Some("scroll".into()),
        "RADIAL" => Some("radial".into()),
        "VENDOR" => Some("vendor".into()),
        _ => None,
    }
}

fn grid_contains_any_token(grid: &Value, needles: &[&str]) -> bool {
    grid.as_array()
        .into_iter()
        .flatten()
        .flat_map(|row| row.as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .any(|token| needles.contains(&token))
}

/// Patches `layout.joystick` in `keymap_bytes` for the selected `profile_index` (defaults to
/// `activeProfileId`) and `layer_index` (0-based; `None` patches all layers in the target profile),
/// preserving all unknown JSON fields and exact bytes outside patched `joystick` object(s).
pub fn patch_keymap_joystick_hardware_mode(
    keymap_bytes: &[u8],
    firmware: &str,
    profile_index: Option<usize>,
    layer_index: Option<usize>,
    mode: &str,
) -> Result<(Vec<u8>, bool), String> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum TargetJoystickMode {
        Mouse,
        Scroll,
        Sectors,
    }

    let target_mode = match mode.trim().to_ascii_lowercase().as_str() {
        "mouse" => TargetJoystickMode::Mouse,
        "scroll" => TargetJoystickMode::Scroll,
        "sectors" | "logical_sectors" | "radial" => TargetJoystickMode::Sectors,
        other => return Err(format!("不支持的摇杆硬件模式：{other}")),
    };
    if matches!(
        target_mode,
        TargetJoystickMode::Mouse | TargetJoystickMode::Scroll
    ) && !crate::supports_v070_mouse_tokens(firmware)
    {
        return Err(format!(
            "原生摇杆 MOUSE/SCROLL 模式需要固件 0.7.0-rc.16 或更高版本（当前：{firmware}）"
        ));
    }

    let config = parse_device_files(keymap_bytes, None).map_err(|e| e.to_string())?;
    let profiles = config.raw["profiles"]
        .as_array()
        .ok_or("Profile 列表缺失")?;
    let target_pi = match profile_index {
        Some(pi) => {
            if pi >= profiles.len() {
                return Err("目标 Profile 索引超界".into());
            }
            pi
        }
        None => {
            let active_id = &config.raw["activeProfileId"];
            let matching: Vec<usize> = profiles
                .iter()
                .enumerate()
                .filter_map(|(i, p)| (&p["id"] == active_id).then_some(i))
                .collect();
            if matching.len() == 1 {
                matching[0]
            } else if let Some(idx) = active_id
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .filter(|&n| n < profiles.len())
            {
                idx
            } else {
                return Err("无法确定目标活动 Profile".into());
            }
        }
    };

    let layers = profiles[target_pi]["layers"]
        .as_array()
        .ok_or("目标 Profile 缺少 layers 数组")?;
    if layers.is_empty() {
        return Err("目标 Profile 没有可修改的层".into());
    }
    let layer_indices: Vec<usize> = match layer_index {
        Some(li) => {
            if li >= layers.len() {
                return Err("目标 Layer 索引超界".into());
            }
            vec![li]
        }
        None => (0..layers.len()).collect(),
    };

    let recognized_six_layer =
        recognize_six_layer_profile_for_firmware(keymap_bytes, None, Some("0.7.0-rc.16"))
            .ok()
            .flatten();
    let is_six_layer_target = recognized_six_layer == Some(target_pi as u32);
    let is_three_layer_target =
        profiles[target_pi]["name"].as_str() == Some(super::THREE_LAYER_PROFILE_NAME);

    let root: &RawValue = serde_json::from_slice(keymap_bytes).map_err(|e| e.to_string())?;
    let root_fields = object(root)?;
    let profiles_raw = array(field(&root_fields, "profiles")?)?;
    let profile_raw = profiles_raw
        .get(target_pi)
        .copied()
        .ok_or("目标 Profile 原字节缺失")?;
    let profile_fields = object(profile_raw)?;
    let layers_raw = array(field(&profile_fields, "layers")?)?;

    let mut edits: Vec<(usize, usize, Vec<u8>)> = Vec::new();
    for li in layer_indices {
        let raw_layer = layers_raw.get(li).copied().ok_or("目标 Layer 原字节缺失")?;
        let layer_fields = object(raw_layer)?;
        let layout_raw = field(&layer_fields, "layout")?;
        let layout_fields = object(layout_raw)?;
        let joystick_raw = field(&layout_fields, "joystick")?;
        let current_joystick: Value =
            serde_json::from_str(joystick_raw.get()).map_err(|e| e.to_string())?;

        let desired_joystick: Option<Value> = match target_mode {
            TargetJoystickMode::Mouse => {
                let target = json!({"type": "MOUSE"});
                (current_joystick != target).then_some(target)
            }
            TargetJoystickMode::Scroll => {
                let target = json!({"type": "SCROLL"});
                (current_joystick != target).then_some(target)
            }
            TargetJoystickMode::Sectors => {
                if is_six_layer_target {
                    if li == 0 {
                        let already_vendor = current_joystick["type"] == "VENDOR"
                            && current_joystick
                                .get("sectors")
                                .is_none_or(|s| s == &json!([]));
                        (!already_vendor).then(|| json!({"type": "VENDOR", "sectors": []}))
                    } else {
                        let expected = managed_layer(li)["layout"]["joystick"].clone();
                        (current_joystick != expected).then_some(expected)
                    }
                } else if is_three_layer_target {
                    let expected = json!({"type": "VENDOR", "sectors": []});
                    (current_joystick != expected).then_some(expected)
                } else if current_joystick["type"] == "RADIAL"
                    && current_joystick
                        .get("sectors")
                        .and_then(Value::as_array)
                        .is_some_and(|a| !a.is_empty())
                {
                    None
                } else if let Some(existing_sectors) = current_joystick
                    .get("sectors")
                    .and_then(Value::as_array)
                    .filter(|a| !a.is_empty())
                {
                    Some(json!({
                        "type": "RADIAL",
                        "sectors": existing_sectors
                    }))
                } else if grid_contains_any_token(
                    &layers[li]["layout"]["keymap"],
                    &SINGLE_LAYER_OAI_SECTORS,
                ) {
                    Some(four_sector_radial_joystick([
                        "KC_F16", "KC_F17", "KC_F18", "KC_F19",
                    ]))
                } else {
                    Some(four_sector_radial_joystick(SINGLE_LAYER_OAI_SECTORS))
                }
            }
        };

        if let Some(replacement_val) = desired_joystick {
            let start = offset(keymap_bytes, joystick_raw)?;
            let end = start + joystick_raw.get().len();
            let encoded = serde_json::to_vec(&replacement_val).map_err(|e| e.to_string())?;
            edits.push((start, end, encoded));
        }
    }

    if edits.is_empty() {
        return Ok((keymap_bytes.to_vec(), false));
    }

    edits.sort_by_key(|edit| edit.0);
    if edits.windows(2).any(|w| w[0].1 > w[1].0) {
        return Err("摇杆修改字节范围重叠".into());
    }

    let mut next = Vec::with_capacity(keymap_bytes.len());
    let mut cursor = 0;
    for (start, end, replacement) in edits {
        next.extend_from_slice(&keymap_bytes[cursor..start]);
        next.extend_from_slice(&replacement);
        cursor = end;
    }
    next.extend_from_slice(&keymap_bytes[cursor..]);

    parse_device_files(&next, None).map_err(|e| e.to_string())?;
    if is_six_layer_target {
        let verify_fw = (!firmware.trim().is_empty()).then_some(firmware);
        if recognize_six_layer_profile_for_firmware(&next, None, verify_fw)?
            != Some(target_pi as u32)
        {
            return Err("六层摇杆硬件模式补丁自检失败".into());
        }
    }
    Ok((next, true))
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    const SOURCE: &[u8] = include_bytes!("../../fixtures/device/synthetic-six-layer-source.json");

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
