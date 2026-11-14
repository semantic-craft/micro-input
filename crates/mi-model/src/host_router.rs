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

fn default_true() -> bool {
    true
}

pub const CANONICAL_DESKTOP_AGENT_PROFILES: [&str; 5] = [
    "Antigravity",
    "Codex",
    "DeepSeek Coder (DSH)",
    "Herdr",
    "其他软件",
];

pub fn canonical_desktop_agent_rank(name: &str) -> Option<usize> {
    match name.trim() {
        "Antigravity" => Some(0),
        "Codex" => Some(1),
        "DeepSeek Coder (DSH)" | "DSH" | "DeepSeek Harness" | "Deepseek Harness" => Some(2),
        "Herdr" => Some(3),
        "其他软件" => Some(4),
        _ => None,
    }
}

pub fn desktop_agent_for_profile_name(name: &str) -> Option<(&'static str, &'static str)> {
    match name.trim() {
        "Antigravity" => Some(("antigravity", "com.google.antigravity")),
        "Codex" => Some(("codex", "com.openai.codex")),
        "DeepSeek Coder (DSH)" | "DSH" | "DeepSeek Harness" | "Deepseek Harness" => {
            Some(("dsh-desktop", "com.deepseek.dsh"))
        }
        _ => None,
    }
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_bundle_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        adapter: Option<String>,
        #[serde(default = "default_true")]
        no_newline: bool,
        #[serde(default)]
        submit: bool,
    },
    OpenApp {
        bundle_id: String,
    },
    ShellCommand {
        command: String,
        working_directory: String,
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
    MacroSequence {
        events: Vec<ResolvedMacroEvent>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "event_type", rename_all = "snake_case")]
pub enum ResolvedMacroEvent {
    Press { key: String, key_code: u16 },
    Release { key: String, key_code: u16 },
    Delay { ms: u32 },
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
    let Some(version) = identity.version.as_deref() else {
        return PassThroughHealth::SnapshotMissing;
    };
    if !crate::is_supported_config_firmware(version) {
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
    // Native firmware device.status reports 1-based layer_index (1 for layers[0], 2 for layers[1]),
    // whereas synthetic-0.6.3 fixtures and 0-based unit tests use 0 for layers[0].
    let layer_idx = if crate::is_supported_native_firmware(version) {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BottomRowLayout {
    #[default]
    Split,
    Merged,
}

impl BottomRowLayout {
    pub fn is_merged(self) -> bool {
        matches!(self, Self::Merged)
    }

    pub fn is_split(self) -> bool {
        matches!(self, Self::Split)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Split => "split",
            Self::Merged => "merged",
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

pub fn verified_physical_mapping_for_layout(
    binding_id: &str,
    layout: BottomRowLayout,
) -> Option<PhysicalControlSpec> {
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
        "AG10" | "ACT10" if layout.is_split() => PhysicalControlSpec {
            binding_id: "AG10",
            logical_switch: "logical-10",
            physical_control: "row3-col0-left",
            keycap_group: "key-r3c0",
            action_role: "momentary_key",
        },
        "AG11" | "ACT11" if layout.is_split() => PhysicalControlSpec {
            binding_id: "AG11",
            logical_switch: "logical-11",
            physical_control: "row3-col1-middle",
            keycap_group: "key-r3c1",
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

pub fn verified_physical_mapping(binding_id: &str) -> Option<PhysicalControlSpec> {
    verified_physical_mapping_for_layout(binding_id, BottomRowLayout::Merged)
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
        "rightbracket" | "right_bracket" | "]" => Some(0x1E),
        "o" => Some(0x1F),
        "u" => Some(0x20),
        "leftbracket" | "left_bracket" | "[" => Some(0x21),
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
        "fn" | "globe" | "apple_fn" | "fn_globe" => Some(0x3F),
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
        "home" => Some(0x73),
        "page_up" | "pageup" => Some(0x74),
        "forward_delete" | "forwarddelete" => Some(0x75),
        "end" => Some(0x77),
        "page_down" | "pagedown" => Some(0x79),
        "left" => Some(0x7B),
        "right" => Some(0x7C),
        "down" => Some(0x7D),
        "up" => Some(0x7E),
        _ => None,
    }
}

fn profile_open_app_bundle_id(doc: &LocalDocumentV2, profile_id: &str) -> Option<String> {
    let profile = doc.editable.graph.profiles.get(profile_id)?;
    let lid = profile.layer_order.first()?;
    let layer = doc.editable.graph.layers.get(lid)?;
    for cid in &layer.control_order {
        let Some(action_id) = layer.controls.get(cid).and_then(|b| b.action_id.as_ref()) else {
            continue;
        };
        let Some(action) = doc.editable.graph.actions.get(action_id) else {
            continue;
        };
        if let Ok(ActionKind::OpenApp { bundle_id }) =
            serde_json::from_value(action.payload.clone())
        {
            let trimmed = bundle_id.trim();
            if !trimmed.is_empty() && trimmed.contains('.') {
                return Some(trimmed.to_owned());
            }
        }
    }
    None
}

fn convert_action(
    action: &Action,
    profile_name: Option<&str>,
    fallback_bundle_id: Option<&str>,
    command_execution_enabled: bool,
) -> Option<ResolvedHostAction> {
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
        ActionKind::Text { text } => {
            if text.trim_end_matches(['\r', '\n']).trim().is_empty() {
                return None;
            }
            let (adapter, target_bundle_id) = profile_name
                .and_then(desktop_agent_for_profile_name)
                .map(|(a, b)| (Some(a.to_owned()), Some(b.to_owned())))
                .unwrap_or_else(|| (None, fallback_bundle_id.map(str::to_owned)));
            Some(ResolvedHostAction::PasteText {
                text,
                target_bundle_id,
                adapter,
                no_newline: true,
                submit: false,
            })
        }
        ActionKind::OpenApp { bundle_id } => Some(ResolvedHostAction::OpenApp { bundle_id }),
        ActionKind::OpenUrl { url } => Some(ResolvedHostAction::OpenUrl { url }),
        ActionKind::ShellCommand {
            command,
            working_directory,
        } if command_execution_enabled => Some(ResolvedHostAction::ShellCommand {
            command,
            working_directory,
        }),
        ActionKind::ShellCommand { .. } => None,
        ActionKind::AgentCommand { adapter, command } => {
            if adapter.eq_ignore_ascii_case("cindy") {
                return None;
            }
            if let Some(prompt) = command
                .strip_prefix("prompt:")
                .or_else(|| command.strip_prefix("submit-prompt:"))
            {
                if prompt.trim_end_matches(['\r', '\n']).trim().is_empty() {
                    return None;
                }
            }
            let resolved_adapter = if adapter == "host" && crate::is_host_mouse_command(&command) {
                adapter
            } else if adapter == "host"
                && (command == "submit"
                    || command == "focus-input"
                    || command.starts_with("prompt:")
                    || command.starts_with("submit-prompt:"))
            {
                profile_name
                    .and_then(desktop_agent_for_profile_name)
                    .map(|(a, _)| a.to_owned())
                    .unwrap_or(adapter)
            } else {
                adapter
            };
            Some(ResolvedHostAction::AgentCommand {
                adapter: resolved_adapter,
                command,
            })
        }
        ActionKind::FocusSession { adapter, session } => {
            if session == "unselected" || adapter.eq_ignore_ascii_case("cindy") {
                None
            } else {
                Some(ResolvedHostAction::FocusSession { adapter, session })
            }
        }
        ActionKind::LocalMacro { macro_sequence } => {
            macro_sequence.validate().ok()?;
            let mut events = Vec::with_capacity(macro_sequence.events.len());
            for event in &macro_sequence.events {
                match event {
                    crate::macro_sequence::MacroEvent::Press { key } => {
                        let key_code = macos_virtual_key_code(key)?;
                        events.push(ResolvedMacroEvent::Press {
                            key: key.clone(),
                            key_code,
                        });
                    }
                    crate::macro_sequence::MacroEvent::Release { key } => {
                        let key_code = macos_virtual_key_code(key)?;
                        events.push(ResolvedMacroEvent::Release {
                            key: key.clone(),
                            key_code,
                        });
                    }
                    crate::macro_sequence::MacroEvent::Delay { ms } => {
                        events.push(ResolvedMacroEvent::Delay { ms: *ms });
                    }
                }
            }
            Some(ResolvedHostAction::MacroSequence { events })
        }
        ActionKind::LocalMulti { .. }
        | ActionKind::DeviceKey { .. }
        | ActionKind::SkillReference { .. } => None,
    }
}

pub(crate) fn resolve_layer_slot_action_id<'a>(
    doc: &'a LocalDocumentV2,
    layer: &'a crate::document::LayerNode,
    primary_slot: &str,
    layout: BottomRowLayout,
) -> Option<&'a String> {
    let find_action_id = |slot: &str| -> Option<&'a String> {
        layer
            .controls
            .values()
            .find(|b| b.logical_control == slot)
            .and_then(|b| b.action_id.as_ref())
    };
    if layout.is_split() {
        return find_action_id(primary_slot);
    }
    if matches!(primary_slot, "logical-10" | "logical-11") {
        let a10 = find_action_id("logical-10");
        let a11 = find_action_id("logical-11");
        let is_focus_input = |aid: &String| -> bool {
            doc.editable
                .graph
                .actions
                .get(aid)
                .and_then(|a| serde_json::from_value::<ActionKind>(a.payload.clone()).ok())
                .is_some_and(|k| {
                    matches!(
                        k,
                        ActionKind::AgentCommand { command, .. }
                            if matches!(command.as_str(), "focus-input" | "new-conversation-in-project")
                    )
                })
        };
        let is_voice_hold = |aid: &String| -> bool {
            doc.editable
                .graph
                .actions
                .get(aid)
                .and_then(|a| serde_json::from_value::<ActionKind>(a.payload.clone()).ok())
                .is_some_and(|k| {
                    matches!(
                        k,
                        ActionKind::AgentCommand { command, .. }
                            if command == "push-to-talk" || command == "dictation"
                    )
                })
        };
        if a10.is_some_and(is_focus_input) && a11.is_some_and(is_voice_hold) {
            return a11;
        }
        return find_action_id(primary_slot).or_else(|| match primary_slot {
            "logical-10" => a11,
            "logical-11" => a10,
            _ => None,
        });
    }
    find_action_id(primary_slot)
}

pub fn resolve_host_action_with_layout(
    doc: &LocalDocumentV2,
    host_active_profile_id: Option<&str>,
    binding_id: &str,
    is_pressed: bool,
    command_execution_enabled: bool,
    layout: BottomRowLayout,
) -> Option<ResolvedHostAction> {
    if !is_pressed {
        return None;
    }
    let _physical = verified_physical_mapping_for_layout(binding_id, layout)?;
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

    let action_id = resolve_layer_slot_action_id(doc, layer, primary_slot, layout)?;
    let action = doc.editable.graph.actions.get(action_id)?;
    let fallback_bundle = profile_open_app_bundle_id(doc, pid);
    convert_action(
        action,
        Some(&profile.name),
        fallback_bundle.as_deref(),
        command_execution_enabled,
    )
}

pub fn resolve_host_action_with_command_consent(
    doc: &LocalDocumentV2,
    host_active_profile_id: Option<&str>,
    binding_id: &str,
    is_pressed: bool,
    command_execution_enabled: bool,
) -> Option<ResolvedHostAction> {
    resolve_host_action_with_layout(
        doc,
        host_active_profile_id,
        binding_id,
        is_pressed,
        command_execution_enabled,
        BottomRowLayout::Merged,
    )
}

pub fn resolve_host_action(
    doc: &LocalDocumentV2,
    host_active_profile_id: Option<&str>,
    binding_id: &str,
    is_pressed: bool,
) -> Option<ResolvedHostAction> {
    resolve_host_action_with_command_consent(
        doc,
        host_active_profile_id,
        binding_id,
        is_pressed,
        false,
    )
}

/// Recheck each supported physical binding; counts alone do not establish position or uniqueness.
pub fn verified_bindings(config: &DeviceConfig, identity: &HostIdentityStatus) -> BTreeSet<String> {
    let mut allowed = BTreeSet::new();
    let (Some(p), Some(l)) = (identity.profile_index, identity.layer_index) else {
        return allowed;
    };
    let layer_index = match identity.version.as_deref() {
        Some(v) if crate::is_supported_native_firmware(v) && l > 0 => (l - 1) as usize,
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
