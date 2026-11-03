//! Deterministic, side-effect-free host routing and passthrough layer health check.
use crate::{
    document::LocalDocumentV2,
    draft::{Action, ActionKind},
    Binding, DeviceConfig,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostIdentityStatus {
    pub version: Option<String>,
    pub profile_index: Option<u32>,
    pub layer_index: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PassThroughHealth {
    Ready,
    LayerDriftWarning {
        active_profile: u32,
        active_layer: u32,
        bound_ag_count: u32,
    },
    SnapshotMissing,
    Disconnected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "action_type", rename_all = "snake_case")]
pub enum ResolvedHostAction {
    Shortcut {
        key: String,
        key_code: u16,
        modifiers: Vec<String>,
        target_bundle_id: Option<String>,
    },
    PasteText {
        text: String,
    },
    OpenApp {
        bundle_id: String,
    },
    OpenUrl {
        url: String,
    },
    AgentCommand {
        adapter: String,
        command: String,
    },
    FocusSession {
        adapter: String,
        session: String,
    },
}

fn layer_passthrough_key_count(layer: &crate::Layer) -> u32 {
    let mut found_slots = BTreeSet::new();
    for binding in &layer.keys {
        let raw_token = match binding {
            Binding::Basic(token) => Some(token.0.as_str()),
            Binding::Unknown(Value::String(s)) => Some(s.as_str()),
            _ => None,
        };
        if let Some(token) = raw_token {
            let suffix = token
                .strip_prefix("KV_OAI_AG")
                .or_else(|| token.strip_prefix("KV_OAI_ACT"));
            if let Some(suffix) = suffix {
                if suffix.len() == 2
                    && suffix.bytes().all(|b| b.is_ascii_digit())
                    && suffix.parse::<u8>().ok().is_some_and(|n| n <= 12)
                {
                    found_slots.insert(suffix.to_owned());
                }
            }
        }
    }
    found_slots.len() as u32
}

pub fn check_passthrough_health(
    snapshot: Option<&DeviceConfig>,
    identity: Option<&HostIdentityStatus>,
) -> PassThroughHealth {
    let Some(identity) = identity else {
        return PassThroughHealth::Disconnected;
    };
    if !matches!(
        identity.version.as_deref(),
        Some("0.6.3" | "synthetic-0.6.3")
    ) {
        return PassThroughHealth::SnapshotMissing;
    }
    let Some(config) = snapshot else {
        return PassThroughHealth::SnapshotMissing;
    };
    let active_profile = identity.profile_index.unwrap_or(0);
    let active_layer = identity.layer_index.unwrap_or(0);
    let Some(profile) = config.profiles.get(active_profile as usize) else {
        return PassThroughHealth::LayerDriftWarning {
            active_profile,
            active_layer,
            bound_ag_count: 0,
        };
    };
    // Firmware 0.6.3 device.status reports 1-based layer_index (1 for layers[0], 2 for layers[1]),
    // whereas synthetic-0.6.3 fixtures and 0-based unit tests use 0 for layers[0].
    let layer_idx = if identity.version.as_deref() == Some("0.6.3") {
        active_layer.saturating_sub(1) as usize
    } else {
        active_layer as usize
    };
    let Some(bound_ag_count) = profile
        .layers
        .get(layer_idx)
        .map(layer_passthrough_key_count)
    else {
        return PassThroughHealth::LayerDriftWarning {
            active_profile,
            active_layer,
            bound_ag_count: 0,
        };
    };
    if bound_ag_count == 13 {
        PassThroughHealth::Ready
    } else {
        PassThroughHealth::LayerDriftWarning {
            active_profile,
            active_layer,
            bound_ag_count,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalControlSpec {
    pub binding_id: &'static str,
    pub logical_switch: &'static str,
    pub physical_control: &'static str,
    pub keycap_group: &'static str,
    pub action_role: &'static str,
}

pub fn verified_physical_mapping(binding_id: &str) -> Option<PhysicalControlSpec> {
    let spec = match binding_id {
        "AG00" => PhysicalControlSpec {
            binding_id: "AG00",
            logical_switch: "logical-0",
            physical_control: "row0-col0-left",
            keycap_group: "key-r0c0",
            action_role: "momentary_key",
        },
        "AG01" => PhysicalControlSpec {
            binding_id: "AG01",
            logical_switch: "logical-1",
            physical_control: "row0-col1-right",
            keycap_group: "key-r0c1",
            action_role: "momentary_key",
        },
        "AG02" => PhysicalControlSpec {
            binding_id: "AG02",
            logical_switch: "logical-2",
            physical_control: "row1-col0",
            keycap_group: "key-r1c0",
            action_role: "momentary_key",
        },
        "AG03" => PhysicalControlSpec {
            binding_id: "AG03",
            logical_switch: "logical-3",
            physical_control: "row1-col1",
            keycap_group: "key-r1c1",
            action_role: "momentary_key",
        },
        "AG04" => PhysicalControlSpec {
            binding_id: "AG04",
            logical_switch: "logical-4",
            physical_control: "row1-col2",
            keycap_group: "key-r1c2",
            action_role: "momentary_key",
        },
        "AG05" => PhysicalControlSpec {
            binding_id: "AG05",
            logical_switch: "logical-5",
            physical_control: "row1-col3",
            keycap_group: "key-r1c3",
            action_role: "momentary_key",
        },
        "AG06" | "ACT06" => PhysicalControlSpec {
            binding_id: "AG06",
            logical_switch: "logical-6",
            physical_control: "row2-col0",
            keycap_group: "key-r2c0",
            action_role: "momentary_key",
        },
        "AG07" | "ACT07" => PhysicalControlSpec {
            binding_id: "AG07",
            logical_switch: "logical-7",
            physical_control: "row2-col1",
            keycap_group: "key-r2c1",
            action_role: "momentary_key",
        },
        "AG08" | "ACT08" => PhysicalControlSpec {
            binding_id: "AG08",
            logical_switch: "logical-8",
            physical_control: "row2-col2",
            keycap_group: "key-r2c2",
            action_role: "momentary_key",
        },
        "AG09" | "ACT09" => PhysicalControlSpec {
            binding_id: "AG09",
            logical_switch: "logical-9",
            physical_control: "row2-col3",
            keycap_group: "key-r2c3",
            action_role: "momentary_key",
        },
        "AG10" | "AG11" | "ACT10" | "ACT11" => PhysicalControlSpec {
            binding_id: "AG10",
            logical_switch: "logical-10",
            physical_control: "row3-wide-2u",
            keycap_group: "wide-bottom-2u",
            action_role: "wide_momentary_key",
        },
        "AG12" | "ACT12" => PhysicalControlSpec {
            binding_id: "AG12",
            logical_switch: "logical-12",
            physical_control: "row3-col2-right",
            keycap_group: "key-r3c2",
            action_role: "momentary_key",
        },
        "AG13" | "ENC_CC" => PhysicalControlSpec {
            binding_id: "AG13",
            logical_switch: "logical-13",
            physical_control: "encoder-top-left-ccw",
            keycap_group: "encoder-top-left",
            action_role: "encoder_step",
        },
        "AG14" | "ENC_CW" => PhysicalControlSpec {
            binding_id: "AG14",
            logical_switch: "logical-14",
            physical_control: "encoder-top-left-cw",
            keycap_group: "encoder-top-left",
            action_role: "encoder_step",
        },
        "ENC_CLK" => PhysicalControlSpec {
            binding_id: "ENC_CLK",
            logical_switch: "logical-15",
            physical_control: "encoder-top-left-click",
            keycap_group: "encoder-top-left",
            action_role: "momentary_key",
        },
        "AG15" => PhysicalControlSpec {
            binding_id: "AG15",
            logical_switch: "logical-16",
            physical_control: "joystick-sector-north",
            keycap_group: "joystick-top-right",
            action_role: "joystick_sector",
        },
        "AG16" => PhysicalControlSpec {
            binding_id: "AG16",
            logical_switch: "logical-17",
            physical_control: "joystick-sector-west",
            keycap_group: "joystick-top-right",
            action_role: "joystick_sector",
        },
        "AG17" => PhysicalControlSpec {
            binding_id: "AG17",
            logical_switch: "logical-18",
            physical_control: "joystick-sector-south",
            keycap_group: "joystick-top-right",
            action_role: "joystick_sector",
        },
        "AG18" => PhysicalControlSpec {
            binding_id: "AG18",
            logical_switch: "logical-19",
            physical_control: "joystick-sector-east",
            keycap_group: "joystick-top-right",
            action_role: "joystick_sector",
        },
        _ => return None,
    };
    Some(spec)
}

pub fn binding_to_logical_slot(binding_id: &str) -> Option<&'static str> {
    match binding_id {
        "AG11" | "ACT11" => Some("logical-11"),
        other => verified_physical_mapping(other).map(|spec| spec.logical_switch),
    }
}

pub fn macos_virtual_key_code(key: &str) -> Option<u16> {
    match key.trim().to_ascii_lowercase().as_str() {
        "a" => Some(0x00),
        "s" => Some(0x01),
        "d" => Some(0x02),
        "f" => Some(0x03),
        "h" => Some(0x04),
        "g" => Some(0x05),
        "z" => Some(0x06),
        "x" => Some(0x07),
        "c" => Some(0x08),
        "v" => Some(0x09),
        "b" => Some(0x0B),
        "q" => Some(0x0C),
        "w" => Some(0x0D),
        "e" => Some(0x0E),
        "r" => Some(0x0F),
        "y" => Some(0x10),
        "t" => Some(0x11),
        "1" => Some(0x12),
        "2" => Some(0x13),
        "3" => Some(0x14),
        "4" => Some(0x15),
        "6" => Some(0x16),
        "5" => Some(0x17),
        "equal" | "=" => Some(0x18),
        "9" => Some(0x19),
        "7" => Some(0x1A),
        "minus" | "-" => Some(0x1B),
        "8" => Some(0x1C),
        "0" => Some(0x1D),
        "rightbracket" | "]" => Some(0x1E),
        "o" => Some(0x1F),
        "u" => Some(0x20),
        "leftbracket" | "[" => Some(0x21),
        "i" => Some(0x22),
        "p" => Some(0x23),
        "return" | "enter" => Some(0x24),
        "l" => Some(0x25),
        "j" => Some(0x26),
        "quote" | "'" => Some(0x27),
        "k" => Some(0x28),
        "semicolon" | ";" => Some(0x29),
        "backslash" | "\\" => Some(0x2A),
        "comma" | "," => Some(0x2B),
        "slash" | "/" => Some(0x2C),
        "n" => Some(0x2D),
        "m" => Some(0x2E),
        "period" | "." => Some(0x2F),
        "tab" => Some(0x30),
        "space" => Some(0x31),
        "grave" | "backtick" | "`" => Some(0x32),
        "delete" | "backspace" => Some(0x33),
        "escape" | "esc" => Some(0x35),
        "f1" => Some(0x7A),
        "f2" => Some(0x78),
        "f3" => Some(0x63),
        "f4" => Some(0x76),
        "f5" => Some(0x60),
        "f6" => Some(0x61),
        "f7" => Some(0x62),
        "f8" => Some(0x64),
        "f9" => Some(0x65),
        "f10" => Some(0x6D),
        "f11" => Some(0x67),
        "f12" => Some(0x6F),
        "left" => Some(0x7B),
        "right" => Some(0x7C),
        "down" => Some(0x7D),
        "up" => Some(0x7E),
        _ => None,
    }
}

fn convert_action(action: &Action) -> Option<ResolvedHostAction> {
    let kind: ActionKind = serde_json::from_value(action.payload.clone()).ok()?;
    match kind {
        ActionKind::Shortcut {
            key,
            modifiers,
            target,
        } => {
            let key_code = macos_virtual_key_code(&key)?;
            let target_bundle_id = if target == "frontmost-editor" || target.trim().is_empty() {
                None
            } else if target.contains('.') {
                Some(target)
            } else {
                None
            };
            Some(ResolvedHostAction::Shortcut {
                key,
                key_code,
                modifiers,
                target_bundle_id,
            })
        }
        ActionKind::Text { text } => Some(ResolvedHostAction::PasteText { text }),
        ActionKind::OpenApp { bundle_id } => Some(ResolvedHostAction::OpenApp { bundle_id }),
        ActionKind::OpenUrl { url } => Some(ResolvedHostAction::OpenUrl { url }),
        ActionKind::AgentCommand { adapter, command } => {
            Some(ResolvedHostAction::AgentCommand { adapter, command })
        }
        ActionKind::FocusSession { adapter, session } => {
            if session == "unselected" {
                None
            } else {
                Some(ResolvedHostAction::FocusSession { adapter, session })
            }
        }
        ActionKind::LocalMulti { .. }
        | ActionKind::DeviceKey { .. }
        | ActionKind::LocalMacro { .. }
        | ActionKind::SkillReference { .. } => None,
    }
}

pub fn resolve_host_action(
    doc: &LocalDocumentV2,
    host_active_profile_id: Option<&str>,
    binding_id: &str,
    is_pressed: bool,
) -> Option<ResolvedHostAction> {
    if !is_pressed {
        return None;
    }
    let pid = host_active_profile_id?;
    let profile = doc.editable.graph.profiles.get(pid)?;
    let lid = profile.layer_order.first()?;
    let layer = doc.editable.graph.layers.get(lid)?;
    let primary_slot = binding_to_logical_slot(binding_id)?;

    if let Some(slot_num) = primary_slot
        .strip_prefix("logical-")
        .and_then(|s| s.parse::<u32>().ok())
    {
        if slot_num >= 16 {
            let sector_idx = slot_num - 16;
            let active = layer.control_settings.as_ref().is_some_and(|s| {
                s.joystick_mode == "logical_sectors" && sector_idx < s.active_sectors
            });
            if !active {
                return None;
            }
        }
    }

    let find_action_id = |slot: &str| -> Option<&String> {
        layer
            .controls
            .values()
            .find(|b| b.logical_control == slot)
            .and_then(|b| b.action_id.as_ref())
    };

    let action_id = find_action_id(primary_slot).or_else(|| match primary_slot {
        "logical-10" => find_action_id("logical-11"),
        "logical-11" => find_action_id("logical-10"),
        _ => None,
    })?;

    let action = doc.editable.graph.actions.get(action_id)?;
    convert_action(action)
}

/// Recheck each supported physical binding; counts alone do not establish position or uniqueness.
pub fn verified_bindings(config: &DeviceConfig, identity: &HostIdentityStatus) -> BTreeSet<String> {
    let mut allowed = BTreeSet::new();
    let (Some(p), Some(l)) = (identity.profile_index, identity.layer_index) else {
        return allowed;
    };
    let layer_index = match identity.version.as_deref() {
        Some("0.6.3") if l > 0 => (l - 1) as usize,
        Some("synthetic-0.6.3") => l as usize,
        _ => return allowed,
    };
    let Some(layer) = config
        .profiles
        .get(p as usize)
        .and_then(|p| p.layers.get(layer_index))
    else {
        return allowed;
    };
    let token = |b: &Binding| match b {
        Binding::Basic(t) => Some(t.0.clone()),
        Binding::Unknown(Value::String(s)) => Some(s.clone()),
        _ => None,
    };
    let normalize = |s: &str| -> Option<String> {
        let suffix = s
            .strip_prefix("KV_OAI_AG")
            .or_else(|| s.strip_prefix("KV_OAI_ACT"))?;
        if suffix.len() != 2 {
            return None;
        }
        let n = suffix.parse::<usize>().ok()?;
        (n <= 12).then(|| format!("AG{n:02}"))
    };
    let names: Vec<_> = layer
        .keys
        .iter()
        .map(|b| token(b).and_then(|s| normalize(&s)))
        .collect();
    for n in 0..13 {
        let id = format!("AG{n:02}");
        let slot = n;
        if names[slot].as_deref() == Some(&id)
            && names.iter().filter(|v| v.as_deref() == Some(&id)).count() == 1
        {
            allowed.insert(id);
        }
    }
    if !(allowed.contains("AG10") && allowed.contains("AG11")) {
        allowed.remove("AG10");
        allowed.remove("AG11");
    }
    for (index, name, alias) in [
        (0, "ENC_CC", "AG13"),
        (1, "ENC_CW", "AG14"),
        (2, "ENC_CLK", "ENC_CLK"),
    ] {
        if token(&layer.encoders[index]).as_deref() == Some(&format!("KV_OAI_{name}")) {
            allowed.insert(name.into());
            allowed.insert(alias.into());
        }
    }
    if layer.joystick.kind == "VENDOR" && layer.joystick.sectors.is_empty() {
        for n in 15..=18 {
            allowed.insert(format!("AG{n:02}"));
        }
    }
    // Retain factory ACT aliases without accepting movement or duplicate switches.
    for n in 0..13 {
        if allowed.contains(&format!("AG{n:02}")) {
            allowed.insert(format!("ACT{n:02}"));
        }
    }
    allowed
}
