use crate::managed_controls::{ManagedAxControls, ManagedControlReadiness, ObservedDictationState};
use crate::native_ax::NativeAxProvider;
use mi_model::host_router::{ResolvedHostAction, ResolvedMacroEvent};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

pub const APPLE_FN_KEY_CODE: u16 = 0x3F;
pub const KEY_CODE_V: u16 = 0x09;
pub const KEY_CODE_RETURN: u16 = 0x24;
pub const KEY_CODE_N: u16 = 0x2D;
pub const CG_EVENT_TYPE_FLAGS_CHANGED: u32 = 12;
pub const CG_EVENT_FLAG_MASK_SECONDARY_FN: u64 = 0x0080_0000;
pub const MAX_CLIPBOARD_STASH_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_ANTIGRAVITY_AX_DEPTH: usize = 36;
pub const MAX_ANTIGRAVITY_AX_NODES: usize = 1800;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AntigravitySidebarAxNode {
    pub index: usize,
    pub role: String,
    pub title: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AntigravityNewConversationTarget {
    ActiveProject {
        project_label: String,
        node_index: usize,
    },
    FirstVisibleProject {
        project_label: Option<String>,
        node_index: usize,
    },
    FallbackShortcutCmdN,
}

pub fn is_antigravity_new_conversation_control(node: &AntigravitySidebarAxNode) -> bool {
    matches!(node.role.as_str(), "AXLink" | "AXButton")
        && (matches!(
            node.description.trim(),
            "New Conversation in Project" | "New Conversation in Workspace"
        ) || matches!(
            node.title.trim(),
            "New Conversation in Project" | "New Conversation in Workspace"
        ))
}

pub fn select_antigravity_new_conversation_target(
    web_area_title: &str,
    nodes: &[AntigravitySidebarAxNode],
) -> AntigravityNewConversationTarget {
    let mut current_project: Option<(&str, usize)> = None;
    let mut candidates: Vec<(Option<&str>, usize)> = Vec::new();
    for (pos, node) in nodes.iter().enumerate() {
        if is_antigravity_new_conversation_control(node) {
            let assoc = current_project
                .and_then(|(proj, proj_pos)| (pos.saturating_sub(proj_pos) <= 6).then_some(proj));
            candidates.push((assoc, node.index));
        } else if node.role == "AXButton" {
            let trimmed = node.title.trim();
            if !trimmed.is_empty()
                && !matches!(
                    trimmed,
                    "Projects" | "Pinned Conversations" | "Automations" | "Project options"
                )
            {
                current_project = Some((trimmed, pos));
            }
        }
    }
    if candidates.is_empty() {
        return AntigravityNewConversationTarget::FallbackShortcutCmdN;
    }
    if let Some(stem) = web_area_title
        .trim()
        .strip_suffix(" - Antigravity")
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if let Some((Some(proj), idx)) = candidates
            .iter()
            .copied()
            .filter(|(proj, _)| {
                proj.is_some_and(|p| stem == p || stem.ends_with(&format!(" - {p}")))
            })
            .max_by_key(|(proj, _)| proj.map_or(0, str::len))
        {
            return AntigravityNewConversationTarget::ActiveProject {
                project_label: proj.to_string(),
                node_index: idx,
            };
        }
    }
    AntigravityNewConversationTarget::FirstVisibleProject {
        project_label: candidates[0].0.map(str::to_owned),
        node_index: candidates[0].1,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AntigravityLiveVoiceTarget {
    StartButton { label: String, node_index: usize },
    EndButton { label: String, node_index: usize },
    NotFound,
}

pub fn is_antigravity_live_voice_start_label(label: &str) -> bool {
    let trimmed = label.trim();
    trimmed.eq_ignore_ascii_case("Start Voice mode")
        || trimmed.eq_ignore_ascii_case("Start Live Chat")
        || trimmed.eq_ignore_ascii_case("Start Live chat")
}

pub fn is_antigravity_live_voice_end_label(label: &str) -> bool {
    let trimmed = label.trim();
    trimmed.eq_ignore_ascii_case("End Voice mode")
        || trimmed.eq_ignore_ascii_case("End Live chat")
        || trimmed.eq_ignore_ascii_case("End Live Chat")
}

pub fn is_antigravity_live_voice_control(node: &AntigravitySidebarAxNode) -> bool {
    matches!(
        node.role.as_str(),
        "AXButton" | "AXLink" | "AXCheckBox" | "AXRadioButton"
    ) && (is_antigravity_live_voice_start_label(&node.title)
        || is_antigravity_live_voice_start_label(&node.description)
        || is_antigravity_live_voice_end_label(&node.title)
        || is_antigravity_live_voice_end_label(&node.description))
}

pub fn select_antigravity_live_voice_target(
    nodes: &[AntigravitySidebarAxNode],
) -> AntigravityLiveVoiceTarget {
    for node in nodes {
        if matches!(
            node.role.as_str(),
            "AXButton" | "AXLink" | "AXCheckBox" | "AXRadioButton"
        ) {
            let label = if is_antigravity_live_voice_end_label(&node.title) {
                Some(node.title.trim().to_string())
            } else if is_antigravity_live_voice_end_label(&node.description) {
                Some(node.description.trim().to_string())
            } else {
                None
            };
            if let Some(l) = label {
                return AntigravityLiveVoiceTarget::EndButton {
                    label: l,
                    node_index: node.index,
                };
            }
        }
    }
    for node in nodes {
        if matches!(
            node.role.as_str(),
            "AXButton" | "AXLink" | "AXCheckBox" | "AXRadioButton"
        ) {
            let label = if is_antigravity_live_voice_start_label(&node.title) {
                Some(node.title.trim().to_string())
            } else if is_antigravity_live_voice_start_label(&node.description) {
                Some(node.description.trim().to_string())
            } else {
                None
            };
            if let Some(l) = label {
                return AntigravityLiveVoiceTarget::StartButton {
                    label: l,
                    node_index: node.index,
                };
            }
        }
    }
    AntigravityLiveVoiceTarget::NotFound
}

pub fn antigravity_live_voice_state(nodes: &[AntigravitySidebarAxNode], active: bool) -> bool {
    let has_active_indicator = nodes.iter().any(|node| {
        let t = node.title.trim();
        let d = node.description.trim();
        let check = |s: &str| {
            is_antigravity_live_voice_end_label(s)
                || s.eq_ignore_ascii_case("Voice mode in progress")
                || s.eq_ignore_ascii_case("Live chat in progress")
                || s.starts_with("Voice mode,")
                || s.starts_with("Live chat,")
                || s.starts_with("voice mode,")
                || s.starts_with("live chat,")
        };
        check(t) || check(d)
    });

    if active {
        has_active_indicator
    } else {
        matches!(
            select_antigravity_live_voice_target(nodes),
            AntigravityLiveVoiceTarget::StartButton { .. }
        ) && !has_active_indicator
    }
}

pub fn parse_pid_from_lsappinfo_output(text: &str) -> Option<i32> {
    for line in text.lines() {
        let lower = line.to_ascii_lowercase();
        let mut search_from = 0;
        while let Some(rel_pos) = lower[search_from..].find("pid") {
            let pos = search_from + rel_pos;
            search_from = pos + 3;

            if pos > 0 && lower.as_bytes()[pos - 1].is_ascii_alphanumeric() {
                continue;
            }

            let after_pid = &line[pos + 3..];
            if let Some(eq_pos) = after_pid.find('=') {
                let between = after_pid[..eq_pos].trim();
                if between.is_empty() || between == "\"" || between == "'" {
                    let after_eq = after_pid[eq_pos + 1..].trim_start();
                    let digit_str: String = after_eq
                        .chars()
                        .take_while(|ch| ch.is_ascii_digit())
                        .collect();
                    if let Ok(pid) = digit_str.parse::<i32>() {
                        if pid > 0 {
                            return Some(pid);
                        }
                    }
                }
            }
        }
    }
    None
}

pub fn submit_modifiers_for_adapter(adapter: &str) -> Vec<String> {
    if adapter.trim().eq_ignore_ascii_case("antigravity") {
        vec!["command".to_string()]
    } else {
        Vec::new()
    }
}

#[cfg(target_os = "macos")]
type CGEventSourceRef = *mut std::ffi::c_void;
#[cfg(target_os = "macos")]
type CGEventRef = *mut std::ffi::c_void;
#[cfg(target_os = "macos")]
type CFTypeRef = *const std::ffi::c_void;
#[cfg(target_os = "macos")]
type AXUIElementRef = *const std::ffi::c_void;
#[cfg(target_os = "macos")]
type CFStringRef = *const std::ffi::c_void;
#[cfg(target_os = "macos")]
type CFArrayRef = *const std::ffi::c_void;
#[cfg(target_os = "macos")]
type CFIndex = isize;
#[cfg(target_os = "macos")]
type AXError = i32;

#[cfg(target_os = "macos")]
const CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE: i32 = 1;
#[cfg(target_os = "macos")]
const CG_HID_EVENT_TAP: u32 = 0;
#[cfg(target_os = "macos")]
const CG_SCROLL_EVENT_UNIT_LINE: u32 = 1;
#[cfg(target_os = "macos")]
const K_AX_ERROR_SUCCESS: AXError = 0;
#[cfg(target_os = "macos")]
const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> AXError;
    fn AXUIElementPerformAction(element: AXUIElementRef, action: CFStringRef) -> AXError;
    static kCFBooleanTrue: CFTypeRef;
    fn CGEventSourceCreate(state_id: i32) -> CGEventSourceRef;
    fn CGEventCreateKeyboardEvent(
        source: CGEventSourceRef,
        virtual_key: u16,
        key_down: bool,
    ) -> CGEventRef;
    fn CGEventCreateScrollWheelEvent2(
        source: CGEventSourceRef,
        units: u32,
        wheel_count: u32,
        wheel1: i32,
        wheel2: i32,
        wheel3: i32,
    ) -> CGEventRef;
    fn CGEventSetType(event: CGEventRef, event_type: u32);
    fn CGEventSetFlags(event: CGEventRef, flags: u64);
    fn CGEventPost(tap: u32, event: CGEventRef);
    fn CFStringCreateWithBytes(
        alloc: CFTypeRef,
        bytes: *const u8,
        num_bytes: CFIndex,
        encoding: u32,
        is_external_representation: u8,
    ) -> CFStringRef;
    fn CFStringGetLength(the_string: CFStringRef) -> CFIndex;
    fn CFStringGetMaximumSizeForEncoding(length: CFIndex, encoding: u32) -> CFIndex;
    fn CFStringGetCString(
        the_string: CFStringRef,
        buffer: *mut std::ffi::c_char,
        buffer_size: CFIndex,
        encoding: u32,
    ) -> u8;
    fn CFArrayGetCount(the_array: CFArrayRef) -> CFIndex;
    fn CFArrayGetValueAtIndex(the_array: CFArrayRef, idx: CFIndex) -> CFTypeRef;
    fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
    fn CFRelease(cf: CFTypeRef);
}

#[cfg(target_os = "macos")]
unsafe fn cf_string_from_str(s: &str) -> CFStringRef {
    CFStringCreateWithBytes(
        std::ptr::null(),
        s.as_ptr(),
        s.len() as CFIndex,
        K_CF_STRING_ENCODING_UTF8,
        0,
    )
}

#[cfg(target_os = "macos")]
unsafe fn cf_string_to_rust(cf: CFStringRef) -> Option<String> {
    if cf.is_null() {
        return None;
    }
    let len = CFStringGetLength(cf);
    if len < 0 {
        return None;
    }
    let max_size = CFStringGetMaximumSizeForEncoding(len, K_CF_STRING_ENCODING_UTF8) + 1;
    if max_size <= 0 || max_size > 16384 {
        return None;
    }
    let mut buf = vec![0u8; max_size as usize];
    let ok = CFStringGetCString(
        cf,
        buf.as_mut_ptr() as *mut std::ffi::c_char,
        max_size,
        K_CF_STRING_ENCODING_UTF8,
    );
    if ok == 0 {
        return None;
    }
    let nul_pos = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8(buf[..nul_pos].to_vec()).ok()
}

#[cfg(target_os = "macos")]
unsafe fn ax_copy_string_attr(elem: AXUIElementRef, attr: &str) -> Option<String> {
    let attr_cf = cf_string_from_str(attr);
    if attr_cf.is_null() {
        return None;
    }
    let mut val: CFTypeRef = std::ptr::null();
    let err = AXUIElementCopyAttributeValue(elem, attr_cf, &mut val);
    CFRelease(attr_cf);
    if err != K_AX_ERROR_SUCCESS || val.is_null() {
        return None;
    }
    let out = cf_string_to_rust(val);
    CFRelease(val);
    out
}

#[cfg(target_os = "macos")]
unsafe fn ax_copy_element_attr(elem: AXUIElementRef, attr: &str) -> Option<AXUIElementRef> {
    let attr_cf = cf_string_from_str(attr);
    if attr_cf.is_null() {
        return None;
    }
    let mut val: CFTypeRef = std::ptr::null();
    let err = AXUIElementCopyAttributeValue(elem, attr_cf, &mut val);
    CFRelease(attr_cf);
    if err != K_AX_ERROR_SUCCESS || val.is_null() {
        return None;
    }
    Some(val)
}

#[cfg(target_os = "macos")]
unsafe fn ax_copy_children(elem: AXUIElementRef) -> Vec<AXUIElementRef> {
    let attr_cf = cf_string_from_str("AXChildren");
    if attr_cf.is_null() {
        return Vec::new();
    }
    let mut val: CFTypeRef = std::ptr::null();
    let err = AXUIElementCopyAttributeValue(elem, attr_cf, &mut val);
    CFRelease(attr_cf);
    if err != K_AX_ERROR_SUCCESS || val.is_null() {
        return Vec::new();
    }
    let count = CFArrayGetCount(val);
    let mut out = Vec::new();
    if count > 0 {
        for i in 0..count.min(256) {
            let item = CFArrayGetValueAtIndex(val, i);
            if !item.is_null() {
                CFRetain(item);
                out.push(item);
            }
        }
    }
    CFRelease(val);
    out
}

#[cfg(target_os = "macos")]
unsafe fn collect_antigravity_ax_nodes(
    elem: AXUIElementRef,
    depth: usize,
    web_area_title: &mut Option<String>,
    nodes: &mut Vec<AntigravitySidebarAxNode>,
    retained_elements: &mut Vec<AXUIElementRef>,
) {
    if elem.is_null() || depth > MAX_ANTIGRAVITY_AX_DEPTH || nodes.len() >= MAX_ANTIGRAVITY_AX_NODES
    {
        return;
    }
    let role = ax_copy_string_attr(elem, "AXRole").unwrap_or_default();
    let title = ax_copy_string_attr(elem, "AXTitle").unwrap_or_default();
    let mut description = ax_copy_string_attr(elem, "AXDescription").unwrap_or_default();
    if description.trim().is_empty() {
        description = ax_copy_string_attr(elem, "AXHelp").unwrap_or_default();
    }
    if role == "AXWebArea" && web_area_title.is_none() && !title.trim().is_empty() {
        *web_area_title = Some(title.clone());
    }
    let idx = retained_elements.len();
    CFRetain(elem);
    retained_elements.push(elem);
    nodes.push(AntigravitySidebarAxNode {
        index: idx,
        role,
        title: title.clone(),
        description: description.clone(),
    });
    let skip_children = matches!(
        title.trim(),
        "Conversation turns" | "Agent response" | "User message"
    ) || matches!(
        description.trim(),
        "Conversation turns" | "Agent response" | "User message"
    );
    if !skip_children {
        let children = ax_copy_children(elem);
        for child in children {
            if nodes.len() < MAX_ANTIGRAVITY_AX_NODES {
                collect_antigravity_ax_nodes(
                    child,
                    depth + 1,
                    web_area_title,
                    nodes,
                    retained_elements,
                );
            }
            CFRelease(child);
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn find_pid_for_bundle_id(bundle_id: &str) -> Option<i32> {
    if let Ok(out) = Command::new("/usr/bin/lsappinfo")
        .args(["info", "-only", "pid", "-app", bundle_id])
        .output()
    {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            return parse_pid_from_lsappinfo_output(&text);
        }
    }
    None
}

pub fn modifiers_to_cg_flags(modifiers: &[String]) -> u64 {
    let mut flags = 0u64;
    for raw in modifiers {
        match raw.trim().to_ascii_lowercase().as_str() {
            "command" | "cmd" | "super" | "meta" => flags |= 0x0010_0000,
            "shift" => flags |= 0x0002_0000,
            "option" | "alt" => flags |= 0x0008_0000,
            "control" | "ctrl" => flags |= 0x0004_0000,
            "fn" | "globe" => flags |= CG_EVENT_FLAG_MASK_SECONDARY_FN,
            _ => {}
        }
    }
    flags
}

pub fn bundle_id_for_adapter(adapter: &str) -> Option<&'static str> {
    match adapter.trim().to_ascii_lowercase().as_str() {
        "antigravity" => Some("com.google.antigravity"),
        "cindy" => Some("com.xd.cindy"),
        "codex" => Some("com.openai.codex"),
        "dsh-desktop" | "dsh" | "deepseek" => Some("com.deepseek.dsh"),
        "herdr" => Some("dev.herdr.app"),
        _ => None,
    }
}

pub fn native_shortcut_for_agent_command(
    adapter: &str,
    command: &str,
) -> Option<(u16, Vec<String>)> {
    let canonical = match adapter.trim().to_ascii_lowercase().as_str() {
        "dsh" | "deepseek" => "dsh-desktop".to_string(),
        other => other.to_string(),
    };
    let mods = |items: &[&str]| items.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
    match (canonical.as_str(), command.trim()) {
        ("antigravity", "focus-input") => Some((0x25, mods(&["command"]))),
        ("antigravity", "search") => Some((0x28, mods(&["command"]))),
        ("antigravity", "new-chat" | "new-or-stop") => Some((0x2D, mods(&["command"]))),
        ("antigravity", "model" | "effort") => Some((0x2C, mods(&["command"]))),
        ("antigravity", "project" | "prev-project" | "next-project") => {
            Some((0x29, mods(&["command"])))
        }
        ("antigravity", "browser") => Some((0x0B, mods(&["command", "shift"]))),
        ("antigravity", "terminal") => Some((0x26, mods(&["command"]))),
        ("antigravity", "dictation" | "push-to-talk") => Some((0x2E, mods(&["control"]))),
        ("antigravity", "split-right") => Some((0x02, mods(&["command"]))),
        (
            "antigravity" | "dsh-desktop" | "cindy",
            "prev-conversation" | "working-prev" | "completed-prev" | "attention-prev",
        ) => Some((0x7E, mods(&["option"]))),
        (
            "antigravity" | "dsh-desktop" | "cindy",
            "next-conversation" | "working-next" | "completed-next" | "attention-next",
        ) => Some((0x7D, mods(&["option"]))),
        ("dsh-desktop", "new-chat" | "new-or-stop") => Some((0x2D, mods(&["command"]))),
        ("dsh-desktop", "search" | "focus-input") => Some((0x28, mods(&["command"]))),
        ("dsh-desktop", "project" | "prev-project" | "next-project") => {
            Some((0x1F, mods(&["command"])))
        }
        ("dsh-desktop", "terminal") => Some((0x32, mods(&["control"]))),
        ("dsh-desktop", "browser") => Some((0x11, mods(&["command"]))),
        ("cindy", "new-chat" | "new-or-stop") => Some((0x2D, mods(&["command"]))),
        ("cindy", "search") => Some((0x03, mods(&["command"]))),
        ("cindy", "effort" | "model") => Some((0x30, mods(&["shift"]))),
        ("cindy", "terminal") => Some((0x32, mods(&["control"]))),
        ("codex", "new-chat" | "new-or-stop") => Some((0x2D, mods(&["command"]))),
        ("codex", "search") => Some((0x28, mods(&["command"]))),
        (_, "approve") => Some((KEY_CODE_RETURN, mods(&["command"]))),
        (_, "deny") => Some((0x35, Vec::new())),
        _ => None,
    }
}

pub fn parse_dial_scroll_command(adapter: &str, command: &str) -> Option<DialScrollRequest> {
    let target_bundle_id = bundle_id_for_adapter(adapter)
        .unwrap_or("frontmost-editor")
        .to_string();
    let trimmed = command.trim();
    match trimmed {
        "scroll-up" => Some(DialScrollRequest {
            target_bundle_id,
            delta_lines: -4,
            jump_to_bottom: false,
            sidebar: false,
        }),
        "scroll-down" => Some(DialScrollRequest {
            target_bundle_id,
            delta_lines: 4,
            jump_to_bottom: false,
            sidebar: false,
        }),
        "scroll-up-fast" => Some(DialScrollRequest {
            target_bundle_id,
            delta_lines: -12,
            jump_to_bottom: false,
            sidebar: false,
        }),
        "scroll-down-fast" => Some(DialScrollRequest {
            target_bundle_id,
            delta_lines: 12,
            jump_to_bottom: false,
            sidebar: false,
        }),
        "sidebar-scroll-up" => Some(DialScrollRequest {
            target_bundle_id,
            delta_lines: -4,
            jump_to_bottom: false,
            sidebar: true,
        }),
        "sidebar-scroll-down" => Some(DialScrollRequest {
            target_bundle_id,
            delta_lines: 4,
            jump_to_bottom: false,
            sidebar: true,
        }),
        "scroll-to-bottom" | "jump-to-bottom" => Some(DialScrollRequest {
            target_bundle_id,
            delta_lines: 120,
            jump_to_bottom: true,
            sidebar: false,
        }),
        other => {
            let lines = other.strip_prefix("scroll-lines:")?.parse::<i32>().ok()?;
            if lines == 0 {
                return None;
            }
            Some(DialScrollRequest {
                target_bundle_id,
                delta_lines: lines.clamp(-20, 20),
                jump_to_bottom: false,
                sidebar: false,
            })
        }
    }
}

fn now_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostActionReceipt {
    pub action_type: String,
    pub binding_id: String,
    pub detail: String,
    pub outcome: String,
    pub profile_id: String,
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_byte_len: Option<usize>,
    pub timestamp_ms: u64,
}

impl HostActionReceipt {
    pub fn succeeded(&self) -> bool {
        matches!(
            self.outcome.as_str(),
            "executed" | "simulated" | "delivered"
        )
    }

    pub fn append_to_audit_log(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut line = serde_json::to_string(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        line.push('\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(line.as_bytes())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardItemSnapshot {
    pub types_and_bytes: Vec<(String, Vec<u8>)>,
}

impl ClipboardItemSnapshot {
    pub fn total_bytes(&self) -> usize {
        self.types_and_bytes.iter().map(|(_, b)| b.len()).sum()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardSnapshot {
    pub change_count: u64,
    pub items: Vec<ClipboardItemSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialScrollRequest {
    pub target_bundle_id: String,
    pub delta_lines: i32,
    pub jump_to_bottom: bool,
    pub sidebar: bool,
}

impl DialScrollRequest {
    pub fn clamped_delta_lines(&self) -> i32 {
        if self.jump_to_bottom {
            self.delta_lines.clamp(-1000, 1000)
        } else {
            self.delta_lines.clamp(-100, 100)
        }
    }
}

pub trait HostOsDriver: Send + Sync {
    fn is_accessibility_trusted(&self) -> bool;
    fn open_application(&self, bundle_id: &str) -> Result<(), String>;
    fn post_key_shortcut(
        &self,
        key_code: u16,
        modifiers: &[String],
        key_down: bool,
    ) -> Result<(), String>;
    fn post_apple_fn_flags(&self, key_down: bool) -> Result<(), String>;
    fn snapshot_clipboard(&self, max_bytes: usize) -> ClipboardSnapshot;
    fn write_clipboard_plain_text(&self, text: &str) -> u64;
    fn restore_clipboard(&self, snapshot: &ClipboardSnapshot, expected_change_count: u64) -> bool;
    fn open_url(&self, url: &str) -> Result<(), String>;
    fn focus_agent_input(&self, adapter: &str) -> Result<(), String>;
    fn stop_active_dictation(&self, adapter: &str) -> Result<bool, String>;
    fn toggle_dictation(&self, _adapter: &str) -> Result<ObservedDictationState, String> {
        Err("blocked:dictation_toggle_unavailable".into())
    }
    fn start_owned_dictation(&self, _adapter: &str) -> Result<(), String> {
        Err("blocked:bound_dictation_unavailable".into())
    }
    fn stop_owned_dictations(&self) -> Result<(), String> {
        Ok(())
    }
    fn submit_agent_input(&self, adapter: &str, prompt: Option<&str>) -> Result<(), String> {
        legacy_submit_agent_input(self, adapter, prompt)
    }
    fn insert_agent_prompt(&self, adapter: &str, prompt: &str) -> Result<(), String> {
        legacy_insert_agent_prompt(self, adapter, prompt)
    }
    fn agent_shortcut(&self, adapter: &str, key: u16, modifiers: &[String]) -> Result<(), String> {
        if let Some(bundle) = bundle_id_for_adapter(adapter) {
            let _ = self.open_application(bundle);
        }
        self.post_key_shortcut(key, modifiers, true)?;
        self.post_key_shortcut(key, modifiers, false)
    }
    fn scroll_window(&self, req: &DialScrollRequest) -> Result<(), String>;
    fn new_conversation_in_project(&self, adapter: &str) -> Result<(), String>;
    fn toggle_live_voice(&self, adapter: &str) -> Result<(), String>;
}

#[derive(Debug, Default)]
pub struct MacOsSystemDriver {
    clipboard_seq: Mutex<u64>,
    managed_controls: ManagedAxControls<NativeAxProvider>,
}

impl MacOsSystemDriver {
    pub fn inspect_managed_controls(
        &self,
        adapter: &str,
    ) -> Result<ManagedControlReadiness, String> {
        self.managed_controls.inspect(adapter)
    }
}

impl HostOsDriver for MacOsSystemDriver {
    fn is_accessibility_trusted(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            unsafe { AXIsProcessTrusted() }
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }

    fn open_application(&self, bundle_id: &str) -> Result<(), String> {
        let status = Command::new("/usr/bin/open")
            .args(["-b", bundle_id])
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("open -b {bundle_id} exited with {status}"))
        }
    }

    fn post_key_shortcut(
        &self,
        key_code: u16,
        modifiers: &[String],
        key_down: bool,
    ) -> Result<(), String> {
        if !self.is_accessibility_trusted() {
            return Err("accessibility_untrusted".to_string());
        }
        #[cfg(target_os = "macos")]
        unsafe {
            let source = CGEventSourceCreate(CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE);
            if source.is_null() {
                return Err("cg_event_source_null".to_string());
            }
            let event = CGEventCreateKeyboardEvent(source, key_code, key_down);
            if event.is_null() {
                CFRelease(source);
                return Err("cg_keyboard_event_null".to_string());
            }
            let flags = modifiers_to_cg_flags(modifiers);
            CGEventSetFlags(event, flags);
            CGEventPost(CG_HID_EVENT_TAP, event);
            CFRelease(event);
            CFRelease(source);
        }
        Ok(())
    }

    fn post_apple_fn_flags(&self, key_down: bool) -> Result<(), String> {
        if !self.is_accessibility_trusted() {
            return Err("accessibility_untrusted".to_string());
        }
        #[cfg(target_os = "macos")]
        unsafe {
            let source = CGEventSourceCreate(CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE);
            if source.is_null() {
                return Err("cg_event_source_null".to_string());
            }
            let event = CGEventCreateKeyboardEvent(source, APPLE_FN_KEY_CODE, key_down);
            if event.is_null() {
                CFRelease(source);
                return Err("cg_apple_fn_event_null".to_string());
            }
            CGEventSetType(event, CG_EVENT_TYPE_FLAGS_CHANGED);
            let flags = if key_down {
                CG_EVENT_FLAG_MASK_SECONDARY_FN
            } else {
                0
            };
            CGEventSetFlags(event, flags);
            CGEventPost(CG_HID_EVENT_TAP, event);
            CFRelease(event);
            CFRelease(source);
        }
        Ok(())
    }

    fn snapshot_clipboard(&self, max_bytes: usize) -> ClipboardSnapshot {
        let cc = *self.clipboard_seq.lock().unwrap();
        let output = Command::new("/usr/bin/pbpaste").output().ok();
        let mut items = Vec::new();
        if let Some(out) = output {
            if out.status.success() && !out.stdout.is_empty() && out.stdout.len() <= max_bytes {
                items.push(ClipboardItemSnapshot {
                    types_and_bytes: vec![("public.utf8-plain-text".to_string(), out.stdout)],
                });
            }
        }
        ClipboardSnapshot {
            change_count: cc,
            items,
        }
    }

    fn write_clipboard_plain_text(&self, text: &str) -> u64 {
        let mut seq = self.clipboard_seq.lock().unwrap();
        *seq += 1;
        let next_cc = *seq;
        if let Ok(mut child) = Command::new("/usr/bin/pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
        }
        next_cc
    }

    fn restore_clipboard(&self, snapshot: &ClipboardSnapshot, expected_change_count: u64) -> bool {
        std::thread::sleep(std::time::Duration::from_millis(80));
        let mut seq = self.clipboard_seq.lock().unwrap();
        if *seq != expected_change_count {
            return false;
        }
        *seq += 1;
        if let Some(first) = snapshot.items.first() {
            if let Some((_, bytes)) = first
                .types_and_bytes
                .iter()
                .find(|(t, _)| t == "public.utf8-plain-text")
            {
                if let Ok(mut child) = Command::new("/usr/bin/pbcopy")
                    .stdin(std::process::Stdio::piped())
                    .spawn()
                {
                    if let Some(mut stdin) = child.stdin.take() {
                        let _ = stdin.write_all(bytes);
                    }
                    let _ = child.wait();
                }
            }
        }
        true
    }

    fn open_url(&self, url: &str) -> Result<(), String> {
        let status = Command::new("/usr/bin/open")
            .arg(url)
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("open url exited with {status}"))
        }
    }

    fn focus_agent_input(&self, adapter: &str) -> Result<(), String> {
        if is_managed_adapter(adapter) {
            return self.managed_controls.focus_input(adapter);
        }
        if let Some(bundle) = bundle_id_for_adapter(adapter) {
            let _ = self.open_application(bundle);
        }
        Ok(())
    }

    fn stop_active_dictation(&self, adapter: &str) -> Result<bool, String> {
        if is_managed_adapter(adapter) {
            self.managed_controls.stop_dictation(adapter)
        } else {
            Ok(false)
        }
    }

    fn toggle_dictation(&self, adapter: &str) -> Result<ObservedDictationState, String> {
        self.managed_controls.toggle_dictation(adapter)
    }

    fn start_owned_dictation(&self, adapter: &str) -> Result<(), String> {
        self.managed_controls.start_dictation(adapter)
    }

    fn stop_owned_dictations(&self) -> Result<(), String> {
        self.managed_controls.stop_owned_dictations()
    }

    fn submit_agent_input(&self, adapter: &str, prompt: Option<&str>) -> Result<(), String> {
        if is_managed_adapter(adapter) {
            self.managed_controls.submit(adapter, prompt)
        } else {
            legacy_submit_agent_input(self, adapter, prompt)
        }
    }

    fn insert_agent_prompt(&self, adapter: &str, prompt: &str) -> Result<(), String> {
        if is_managed_adapter(adapter) {
            self.managed_controls.insert_prompt(adapter, prompt)
        } else {
            legacy_insert_agent_prompt(self, adapter, prompt)
        }
    }

    fn agent_shortcut(&self, adapter: &str, key: u16, modifiers: &[String]) -> Result<(), String> {
        if is_managed_adapter(adapter) {
            return self.managed_controls.shortcut(adapter, key, modifiers);
        }
        if let Some(bundle) = bundle_id_for_adapter(adapter) {
            let _ = self.open_application(bundle);
        }
        self.post_key_shortcut(key, modifiers, true)?;
        self.post_key_shortcut(key, modifiers, false)
    }

    fn scroll_window(&self, req: &DialScrollRequest) -> Result<(), String> {
        if is_managed_bundle(&req.target_bundle_id) {
            return self.managed_controls.scroll(req);
        }
        if !self.is_accessibility_trusted() {
            return Err("accessibility_untrusted".to_string());
        }
        let clamped = req.clamped_delta_lines();
        if clamped == 0 {
            return Ok(());
        }
        #[cfg(target_os = "macos")]
        unsafe {
            let source = CGEventSourceCreate(CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE);
            if source.is_null() {
                return Err("cg_event_source_null".to_string());
            }
            if req.jump_to_bottom {
                for step in 0..6 {
                    let event = CGEventCreateScrollWheelEvent2(
                        source,
                        CG_SCROLL_EVENT_UNIT_LINE,
                        1,
                        -20,
                        0,
                        0,
                    );
                    if !event.is_null() {
                        CGEventPost(CG_HID_EVENT_TAP, event);
                        CFRelease(event);
                    }
                    if step < 5 {
                        std::thread::sleep(std::time::Duration::from_millis(8));
                    }
                }
            } else {
                let wheel_delta = -clamped;
                let event = CGEventCreateScrollWheelEvent2(
                    source,
                    CG_SCROLL_EVENT_UNIT_LINE,
                    1,
                    wheel_delta,
                    0,
                    0,
                );
                if event.is_null() {
                    CFRelease(source);
                    return Err("cg_scroll_event_null".to_string());
                }
                CGEventPost(CG_HID_EVENT_TAP, event);
                CFRelease(event);
            }
            CFRelease(source);
        }
        Ok(())
    }

    fn new_conversation_in_project(&self, adapter: &str) -> Result<(), String> {
        if !self.is_accessibility_trusted() {
            return Err("accessibility_untrusted".to_string());
        }
        let bundle_id = bundle_id_for_adapter(adapter).unwrap_or("com.google.antigravity");
        let _ = self.open_application(bundle_id);
        #[cfg(target_os = "macos")]
        if adapter.trim().eq_ignore_ascii_case("antigravity") {
            unsafe {
                let app_elem = if let Some(pid) = find_pid_for_bundle_id(bundle_id) {
                    AXUIElementCreateApplication(pid)
                } else {
                    let sys = AXUIElementCreateSystemWide();
                    let focused = if sys.is_null() {
                        std::ptr::null()
                    } else {
                        let f = ax_copy_element_attr(sys, "AXFocusedApplication")
                            .unwrap_or(std::ptr::null());
                        CFRelease(sys);
                        f
                    };
                    focused
                };
                if !app_elem.is_null() {
                    let attr_manual = cf_string_from_str("AXManualAccessibility");
                    if !attr_manual.is_null() {
                        let _ = AXUIElementSetAttributeValue(app_elem, attr_manual, kCFBooleanTrue);
                        CFRelease(attr_manual);
                    }
                    let win_elem = ax_copy_element_attr(app_elem, "AXFocusedWindow")
                        .or_else(|| ax_copy_element_attr(app_elem, "AXMainWindow"))
                        .or_else(|| {
                            let attr_cf = cf_string_from_str("AXWindows");
                            if attr_cf.is_null() {
                                return None;
                            }
                            let mut val: CFTypeRef = std::ptr::null();
                            let err = AXUIElementCopyAttributeValue(app_elem, attr_cf, &mut val);
                            CFRelease(attr_cf);
                            if err != K_AX_ERROR_SUCCESS || val.is_null() {
                                return None;
                            }
                            let first = if CFArrayGetCount(val) > 0 {
                                let item = CFArrayGetValueAtIndex(val, 0);
                                if !item.is_null() {
                                    CFRetain(item);
                                    Some(item)
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                            CFRelease(val);
                            first
                        });
                    CFRelease(app_elem);
                    if let Some(win) = win_elem {
                        let win_title = ax_copy_string_attr(win, "AXTitle").unwrap_or_default();
                        let mut web_area_title: Option<String> = None;
                        let mut nodes: Vec<AntigravitySidebarAxNode> = Vec::new();
                        let mut retained: Vec<AXUIElementRef> = Vec::new();
                        collect_antigravity_ax_nodes(
                            win,
                            0,
                            &mut web_area_title,
                            &mut nodes,
                            &mut retained,
                        );
                        CFRelease(win);
                        let effective_title =
                            web_area_title.as_deref().unwrap_or(win_title.as_str());
                        let target =
                            select_antigravity_new_conversation_target(effective_title, &nodes);
                        let pressed_ok = match target {
                            AntigravityNewConversationTarget::ActiveProject {
                                node_index, ..
                            }
                            | AntigravityNewConversationTarget::FirstVisibleProject {
                                node_index,
                                ..
                            } => {
                                if let Some(&elem) = retained.get(node_index) {
                                    let action_cf = cf_string_from_str("AXPress");
                                    let ok = if !action_cf.is_null() {
                                        let err = AXUIElementPerformAction(elem, action_cf);
                                        CFRelease(action_cf);
                                        err == K_AX_ERROR_SUCCESS
                                    } else {
                                        false
                                    };
                                    ok
                                } else {
                                    false
                                }
                            }
                            AntigravityNewConversationTarget::FallbackShortcutCmdN => false,
                        };
                        for elem in retained {
                            CFRelease(elem);
                        }
                        if pressed_ok {
                            return Ok(());
                        }
                    }
                }
            }
        }
        let cmd = vec!["command".to_string()];
        self.post_key_shortcut(KEY_CODE_N, &cmd, true)
            .and_then(|()| self.post_key_shortcut(KEY_CODE_N, &cmd, false))
    }

    fn toggle_live_voice(&self, adapter: &str) -> Result<(), String> {
        if !self.is_accessibility_trusted() {
            return Err("accessibility_untrusted".to_string());
        }
        let bundle_id = bundle_id_for_adapter(adapter).unwrap_or("com.google.antigravity");
        let _ = self.open_application(bundle_id);
        #[cfg(target_os = "macos")]
        if adapter.trim().eq_ignore_ascii_case("antigravity") {
            unsafe {
                let app_elem = if let Some(pid) = find_pid_for_bundle_id(bundle_id) {
                    let elem = AXUIElementCreateApplication(pid);
                    if elem.is_null() {
                        return Err("antigravity_live_voice_control_not_found".to_string());
                    }
                    elem
                } else {
                    let sys = AXUIElementCreateSystemWide();
                    let focused = if sys.is_null() {
                        std::ptr::null()
                    } else {
                        let f = ax_copy_element_attr(sys, "AXFocusedApplication")
                            .unwrap_or(std::ptr::null());
                        CFRelease(sys);
                        f
                    };
                    if focused.is_null() {
                        return Err("antigravity_live_voice_control_not_found".to_string());
                    }
                    focused
                };

                let attr_manual = cf_string_from_str("AXManualAccessibility");
                if !attr_manual.is_null() {
                    let _ = AXUIElementSetAttributeValue(app_elem, attr_manual, kCFBooleanTrue);
                    CFRelease(attr_manual);
                }

                let mut target = AntigravityLiveVoiceTarget::NotFound;
                let mut retained: Vec<AXUIElementRef> = Vec::new();
                let mut win_elem: Option<AXUIElementRef> = None;

                for attempt in 0..10 {
                    if attempt > 0 {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                    let candidate_win = ax_copy_element_attr(app_elem, "AXFocusedWindow")
                        .or_else(|| ax_copy_element_attr(app_elem, "AXMainWindow"))
                        .or_else(|| {
                            let attr_cf = cf_string_from_str("AXWindows");
                            if attr_cf.is_null() {
                                return None;
                            }
                            let mut val: CFTypeRef = std::ptr::null();
                            let err = AXUIElementCopyAttributeValue(app_elem, attr_cf, &mut val);
                            CFRelease(attr_cf);
                            if err != K_AX_ERROR_SUCCESS || val.is_null() {
                                return None;
                            }
                            let first = if CFArrayGetCount(val) > 0 {
                                let item = CFArrayGetValueAtIndex(val, 0);
                                if !item.is_null() {
                                    CFRetain(item);
                                    Some(item)
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                            CFRelease(val);
                            first
                        });

                    if let Some(w) = candidate_win {
                        let mut web_area_title = None;
                        let mut nodes = Vec::new();
                        let mut cur_retained = Vec::new();
                        collect_antigravity_ax_nodes(
                            w,
                            0,
                            &mut web_area_title,
                            &mut nodes,
                            &mut cur_retained,
                        );
                        let found_target = select_antigravity_live_voice_target(&nodes);
                        if matches!(
                            found_target,
                            AntigravityLiveVoiceTarget::StartButton { .. }
                                | AntigravityLiveVoiceTarget::EndButton { .. }
                        ) {
                            target = found_target;
                            for elem in retained {
                                CFRelease(elem);
                            }
                            retained = cur_retained;
                            win_elem = Some(w);
                            break;
                        } else {
                            for elem in cur_retained {
                                CFRelease(elem);
                            }
                            CFRelease(w);
                        }
                    }
                }

                let Some(win) = win_elem else {
                    CFRelease(app_elem);
                    for elem in retained {
                        CFRelease(elem);
                    }
                    return Err("antigravity_live_voice_control_not_found".to_string());
                };

                let (was_active, target_node_index) = match target {
                    AntigravityLiveVoiceTarget::EndButton { node_index, .. } => (true, node_index),
                    AntigravityLiveVoiceTarget::StartButton { node_index, .. } => {
                        (false, node_index)
                    }
                    AntigravityLiveVoiceTarget::NotFound => {
                        CFRelease(win);
                        CFRelease(app_elem);
                        for elem in retained {
                            CFRelease(elem);
                        }
                        return Err("antigravity_live_voice_control_not_found".to_string());
                    }
                };

                let pressed_ok = if let Some(&elem) = retained.get(target_node_index) {
                    let action_cf = cf_string_from_str("AXPress");
                    let ok = if !action_cf.is_null() {
                        let err = AXUIElementPerformAction(elem, action_cf);
                        CFRelease(action_cf);
                        err == K_AX_ERROR_SUCCESS
                    } else {
                        false
                    };
                    ok
                } else {
                    false
                };

                for elem in retained {
                    CFRelease(elem);
                }

                if !pressed_ok {
                    CFRelease(win);
                    CFRelease(app_elem);
                    return Err("antigravity_live_voice_control_not_found".to_string());
                }

                let mut confirmed = false;
                for _attempt in 0..25 {
                    std::thread::sleep(std::time::Duration::from_millis(100));

                    let mut web_area_title = None;
                    let mut poll_nodes = Vec::new();
                    let mut poll_retained = Vec::new();
                    collect_antigravity_ax_nodes(
                        win,
                        0,
                        &mut web_area_title,
                        &mut poll_nodes,
                        &mut poll_retained,
                    );

                    if !was_active {
                        for node in &poll_nodes {
                            if node.role == "AXButton" {
                                let t = node.title.trim();
                                let d = node.description.trim();
                                if t.starts_with("Use ") || d.starts_with("Use ") {
                                    if let Some(&elem) = poll_retained.get(node.index) {
                                        let action_cf = cf_string_from_str("AXPress");
                                        if !action_cf.is_null() {
                                            let _ = AXUIElementPerformAction(elem, action_cf);
                                            CFRelease(action_cf);
                                        }
                                    }
                                    break;
                                }
                            }
                        }
                    }

                    let ok = antigravity_live_voice_state(&poll_nodes, !was_active);
                    for elem in poll_retained {
                        CFRelease(elem);
                    }
                    if ok {
                        confirmed = true;
                        break;
                    }
                }

                CFRelease(win);
                CFRelease(app_elem);

                if confirmed {
                    return Ok(());
                } else {
                    return Err("antigravity_live_voice_state_unconfirmed".to_string());
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordedOsEvent {
    CheckAccessibilityTrusted,
    OpenApplication(String),
    PostKeyShortcut {
        key_code: u16,
        modifiers: Vec<String>,
        key_down: bool,
    },
    PostAppleFnFlags {
        event_type: u32,
        key_code: u16,
        flags: u64,
        key_down: bool,
    },
    SnapshotClipboard {
        max_bytes: usize,
    },
    WriteClipboardPlainText(String),
    RestoreClipboard {
        expected_change_count: u64,
        restored: bool,
    },
    OpenUrl(String),
    FocusAgentInput(String),
    StopActiveDictation(String),
    ScrollWindow(DialScrollRequest),
    NewConversationInProject(String),
    ToggleLiveVoice(String),
}

#[derive(Debug)]
struct RecordingState {
    trusted: bool,
    dictation_active: bool,
    clipboard: ClipboardSnapshot,
    events: Vec<RecordedOsEvent>,
}

#[derive(Debug, Clone)]
pub struct RecordingOsDriver {
    state: Arc<Mutex<RecordingState>>,
}

impl Default for RecordingOsDriver {
    fn default() -> Self {
        Self::new(true)
    }
}

impl RecordingOsDriver {
    pub fn new(trusted: bool) -> Self {
        Self {
            state: Arc::new(Mutex::new(RecordingState {
                trusted,
                dictation_active: false,
                clipboard: ClipboardSnapshot {
                    change_count: 1,
                    items: Vec::new(),
                },
                events: Vec::new(),
            })),
        }
    }

    pub fn set_accessibility_trusted(&self, trusted: bool) {
        self.state.lock().unwrap().trusted = trusted;
    }

    pub fn set_dictation_active(&self, active: bool) {
        self.state.lock().unwrap().dictation_active = active;
    }

    pub fn seed_clipboard(&self, items: Vec<ClipboardItemSnapshot>, change_count: u64) {
        let mut st = self.state.lock().unwrap();
        st.clipboard = ClipboardSnapshot {
            change_count,
            items,
        };
    }

    pub fn current_clipboard(&self) -> ClipboardSnapshot {
        self.state.lock().unwrap().clipboard.clone()
    }

    pub fn events(&self) -> Vec<RecordedOsEvent> {
        self.state.lock().unwrap().events.clone()
    }
}

impl HostOsDriver for RecordingOsDriver {
    fn is_accessibility_trusted(&self) -> bool {
        let mut st = self.state.lock().unwrap();
        st.events.push(RecordedOsEvent::CheckAccessibilityTrusted);
        st.trusted
    }

    fn open_application(&self, bundle_id: &str) -> Result<(), String> {
        self.state
            .lock()
            .unwrap()
            .events
            .push(RecordedOsEvent::OpenApplication(bundle_id.to_string()));
        Ok(())
    }

    fn post_key_shortcut(
        &self,
        key_code: u16,
        modifiers: &[String],
        key_down: bool,
    ) -> Result<(), String> {
        let mut st = self.state.lock().unwrap();
        if !st.trusted {
            return Err("accessibility_untrusted".to_string());
        }
        st.events.push(RecordedOsEvent::PostKeyShortcut {
            key_code,
            modifiers: modifiers.to_vec(),
            key_down,
        });
        Ok(())
    }

    fn post_apple_fn_flags(&self, key_down: bool) -> Result<(), String> {
        let mut st = self.state.lock().unwrap();
        if !st.trusted {
            return Err("accessibility_untrusted".to_string());
        }
        let flags = if key_down {
            CG_EVENT_FLAG_MASK_SECONDARY_FN
        } else {
            0
        };
        st.events.push(RecordedOsEvent::PostAppleFnFlags {
            event_type: CG_EVENT_TYPE_FLAGS_CHANGED,
            key_code: APPLE_FN_KEY_CODE,
            flags,
            key_down,
        });
        Ok(())
    }

    fn snapshot_clipboard(&self, max_bytes: usize) -> ClipboardSnapshot {
        let mut st = self.state.lock().unwrap();
        st.events
            .push(RecordedOsEvent::SnapshotClipboard { max_bytes });
        let mut total = 0usize;
        let mut items = Vec::new();
        for item in &st.clipboard.items {
            let item_bytes = item.total_bytes();
            if total + item_bytes <= max_bytes {
                total += item_bytes;
                items.push(item.clone());
            }
        }
        ClipboardSnapshot {
            change_count: st.clipboard.change_count,
            items,
        }
    }

    fn write_clipboard_plain_text(&self, text: &str) -> u64 {
        let mut st = self.state.lock().unwrap();
        st.events
            .push(RecordedOsEvent::WriteClipboardPlainText(text.to_string()));
        st.clipboard.change_count += 1;
        st.clipboard.items = vec![ClipboardItemSnapshot {
            types_and_bytes: vec![(
                "public.utf8-plain-text".to_string(),
                text.as_bytes().to_vec(),
            )],
        }];
        st.clipboard.change_count
    }

    fn restore_clipboard(&self, snapshot: &ClipboardSnapshot, expected_change_count: u64) -> bool {
        let mut st = self.state.lock().unwrap();
        let restored = if st.clipboard.change_count == expected_change_count {
            st.clipboard.change_count += 1;
            st.clipboard.items = snapshot.items.clone();
            true
        } else {
            false
        };
        st.events.push(RecordedOsEvent::RestoreClipboard {
            expected_change_count,
            restored,
        });
        restored
    }

    fn open_url(&self, url: &str) -> Result<(), String> {
        self.state
            .lock()
            .unwrap()
            .events
            .push(RecordedOsEvent::OpenUrl(url.to_string()));
        Ok(())
    }

    fn focus_agent_input(&self, adapter: &str) -> Result<(), String> {
        self.state
            .lock()
            .unwrap()
            .events
            .push(RecordedOsEvent::FocusAgentInput(adapter.to_string()));
        Ok(())
    }

    fn stop_active_dictation(&self, adapter: &str) -> Result<bool, String> {
        let mut st = self.state.lock().unwrap();
        st.events
            .push(RecordedOsEvent::StopActiveDictation(adapter.to_string()));
        let was_active = st.dictation_active;
        st.dictation_active = false;
        Ok(was_active)
    }

    fn scroll_window(&self, req: &DialScrollRequest) -> Result<(), String> {
        let mut clamped = req.clone();
        clamped.delta_lines = req.clamped_delta_lines();
        self.state
            .lock()
            .unwrap()
            .events
            .push(RecordedOsEvent::ScrollWindow(clamped));
        Ok(())
    }

    fn new_conversation_in_project(&self, adapter: &str) -> Result<(), String> {
        let mut st = self.state.lock().unwrap();
        if !st.trusted {
            return Err("accessibility_untrusted".to_string());
        }
        st.events.push(RecordedOsEvent::NewConversationInProject(
            adapter.to_string(),
        ));
        Ok(())
    }

    fn toggle_live_voice(&self, adapter: &str) -> Result<(), String> {
        let mut st = self.state.lock().unwrap();
        if !st.trusted {
            return Err("accessibility_untrusted".to_string());
        }
        st.events
            .push(RecordedOsEvent::ToggleLiveVoice(adapter.to_string()));
        Ok(())
    }
}

pub(crate) struct ActionDescriptor {
    pub action_type: String,
    pub target: String,
    pub detail: String,
    pub text_byte_len: Option<usize>,
}

pub(crate) fn describe_resolved_action(action: &ResolvedHostAction) -> ActionDescriptor {
    match action {
        ResolvedHostAction::Shortcut {
            key,
            modifiers,
            target_bundle_id,
            ..
        } => {
            let detail = if modifiers.is_empty() {
                key.clone()
            } else {
                format!("{}+{}", modifiers.join("+"), key)
            };
            ActionDescriptor {
                action_type: "shortcut".to_string(),
                target: target_bundle_id
                    .clone()
                    .unwrap_or_else(|| "frontmost-editor".to_string()),
                detail,
                text_byte_len: None,
            }
        }
        ResolvedHostAction::PasteText {
            text,
            target_bundle_id,
            adapter,
            no_newline,
            submit,
        } => {
            let sanitized = if *no_newline {
                text.trim_end_matches(['\r', '\n'])
            } else {
                text.as_str()
            };
            let byte_len = sanitized.len();
            let target = adapter
                .clone()
                .or_else(|| target_bundle_id.clone())
                .unwrap_or_else(|| "frontmost-editor".to_string());
            let detail = if *submit {
                format!("submit=true (bytes={byte_len})")
            } else {
                format!("no_newline={no_newline} (bytes={byte_len})")
            };
            ActionDescriptor {
                action_type: "paste_text".to_string(),
                target,
                detail,
                text_byte_len: Some(byte_len),
            }
        }
        ResolvedHostAction::OpenApp { bundle_id } => ActionDescriptor {
            action_type: "open_app".to_string(),
            target: bundle_id.clone(),
            detail: bundle_id.clone(),
            text_byte_len: None,
        },
        ResolvedHostAction::ShellCommand {
            command,
            working_directory,
        } => ActionDescriptor {
            action_type: "shell_command".to_string(),
            target: working_directory.clone(),
            detail: format!("sh ({}B)", command.len()),
            text_byte_len: Some(command.len()),
        },
        ResolvedHostAction::OpenUrl { url } => {
            let host = url
                .split("://")
                .nth(1)
                .and_then(|rest| rest.split('/').next())
                .filter(|h| !h.is_empty())
                .unwrap_or("url")
                .to_string();
            ActionDescriptor {
                action_type: "open_url".to_string(),
                target: "default-browser".to_string(),
                detail: host,
                text_byte_len: Some(url.len()),
            }
        }
        ResolvedHostAction::AgentCommand { adapter, command } => {
            if let Some(prompt) = command.strip_prefix("prompt:") {
                let sanitized = prompt.trim_end_matches(['\r', '\n']);
                ActionDescriptor {
                    action_type: "agent_command".to_string(),
                    target: adapter.clone(),
                    detail: format!("{adapter}/prompt (bytes={})", sanitized.len()),
                    text_byte_len: Some(sanitized.len()),
                }
            } else if let Some(prompt) = command.strip_prefix("submit-prompt:") {
                let sanitized = prompt.trim_end_matches(['\r', '\n']);
                ActionDescriptor {
                    action_type: "agent_command".to_string(),
                    target: adapter.clone(),
                    detail: format!("{adapter}/submit-prompt (bytes={})", sanitized.len()),
                    text_byte_len: Some(sanitized.len()),
                }
            } else {
                ActionDescriptor {
                    action_type: "agent_command".to_string(),
                    target: adapter.clone(),
                    detail: format!("{adapter}/{command}"),
                    text_byte_len: None,
                }
            }
        }
        ResolvedHostAction::FocusSession { adapter, session } => ActionDescriptor {
            action_type: "focus_session".to_string(),
            target: adapter.clone(),
            detail: format!("{adapter}:{session}"),
            text_byte_len: None,
        },
        ResolvedHostAction::MacroSequence { events } => {
            let key_events = events
                .iter()
                .filter(|e| {
                    matches!(
                        e,
                        ResolvedMacroEvent::Press { .. } | ResolvedMacroEvent::Release { .. }
                    )
                })
                .count();
            let delay_events = events.len() - key_events;
            ActionDescriptor {
                action_type: "macro_sequence".to_string(),
                target: "frontmost-editor".to_string(),
                detail: format!(
                    "steps={} keys={} delays={}",
                    events.len(),
                    key_events,
                    delay_events
                ),
                text_byte_len: None,
            }
        }
    }
}

pub struct LiveHostActionExecutor<D: HostOsDriver = MacOsSystemDriver> {
    driver: D,
    held_apple_fn_bindings: Mutex<HashSet<String>>,
}

impl<D: HostOsDriver> LiveHostActionExecutor<D> {
    pub fn new(driver: D) -> Self {
        Self {
            driver,
            held_apple_fn_bindings: Mutex::new(HashSet::new()),
        }
    }

    pub fn driver(&self) -> &D {
        &self.driver
    }

    pub fn stop_owned_actions(&self) -> Result<(), String> {
        let mut held = self.held_apple_fn_bindings.lock().unwrap();
        let released = if held.is_empty() {
            Ok(())
        } else {
            self.driver
                .post_apple_fn_flags(false)
                .map(|()| held.clear())
        };
        let stopped = self.driver.stop_owned_dictations();
        released.and(stopped)
    }

    pub fn handle_apple_fn_press(
        &self,
        profile_id: &str,
        binding_id: &str,
        target: &str,
    ) -> Option<HostActionReceipt> {
        let mut held = self.held_apple_fn_bindings.lock().unwrap();
        if held.contains(binding_id) {
            return None;
        }
        let resolved_target = if target.trim().is_empty() {
            "frontmost-editor"
        } else {
            target
        };
        if !self.driver.is_accessibility_trusted() {
            return Some(HostActionReceipt {
                action_type: "shortcut".to_string(),
                binding_id: binding_id.to_string(),
                detail: "accessibility_untrusted".to_string(),
                outcome: "blocked".to_string(),
                profile_id: profile_id.to_string(),
                target: resolved_target.to_string(),
                text_byte_len: None,
                timestamp_ms: now_timestamp_ms(),
            });
        }
        if let Err(err) = self.driver.post_apple_fn_flags(true) {
            return Some(HostActionReceipt {
                action_type: "shortcut".to_string(),
                binding_id: binding_id.to_string(),
                detail: err,
                outcome: "failed".to_string(),
                profile_id: profile_id.to_string(),
                target: resolved_target.to_string(),
                text_byte_len: None,
                timestamp_ms: now_timestamp_ms(),
            });
        }
        held.insert(binding_id.to_string());
        Some(HostActionReceipt {
            action_type: "shortcut".to_string(),
            binding_id: binding_id.to_string(),
            detail: "fn (press)".to_string(),
            outcome: "executed".to_string(),
            profile_id: profile_id.to_string(),
            target: resolved_target.to_string(),
            text_byte_len: None,
            timestamp_ms: now_timestamp_ms(),
        })
    }

    pub fn handle_apple_fn_release(
        &self,
        profile_id: &str,
        binding_id: &str,
        target: &str,
    ) -> Option<HostActionReceipt> {
        let mut held = self.held_apple_fn_bindings.lock().unwrap();
        if !held.remove(binding_id) {
            return None;
        }
        let resolved_target = if target.trim().is_empty() {
            "frontmost-editor"
        } else {
            target
        };
        let (outcome, detail) = match self.driver.post_apple_fn_flags(false) {
            Ok(()) => ("executed".to_string(), "fn (release)".to_string()),
            Err(err) => ("failed".to_string(), err),
        };
        Some(HostActionReceipt {
            action_type: "shortcut".to_string(),
            binding_id: binding_id.to_string(),
            detail,
            outcome,
            profile_id: profile_id.to_string(),
            target: resolved_target.to_string(),
            text_byte_len: None,
            timestamp_ms: now_timestamp_ms(),
        })
    }

    pub fn execute(
        &self,
        profile_id: &str,
        binding_id: &str,
        action: &ResolvedHostAction,
    ) -> HostActionReceipt {
        let desc = describe_resolved_action(action);
        let make_receipt = |outcome: &str, detail: String| HostActionReceipt {
            action_type: desc.action_type.clone(),
            binding_id: binding_id.to_string(),
            detail,
            outcome: outcome.to_string(),
            profile_id: profile_id.to_string(),
            target: desc.target.clone(),
            text_byte_len: desc.text_byte_len,
            timestamp_ms: now_timestamp_ms(),
        };

        match action {
            ResolvedHostAction::Shortcut {
                key_code,
                modifiers,
                target_bundle_id,
                ..
            } => {
                if !self.driver.is_accessibility_trusted() {
                    return make_receipt("blocked", "accessibility_untrusted".to_string());
                }
                if let Some(adapter) = target_bundle_id
                    .as_deref()
                    .and_then(managed_adapter_for_bundle)
                {
                    return match self.driver.agent_shortcut(adapter, *key_code, modifiers) {
                        Ok(()) => make_receipt("executed", desc.detail),
                        Err(error) => make_receipt(managed_error_outcome(&error), error),
                    };
                }
                if let Some(bundle_id) = target_bundle_id {
                    if let Err(err) = self.driver.open_application(bundle_id) {
                        return make_receipt("failed", err);
                    }
                }
                let res = if *key_code == APPLE_FN_KEY_CODE && modifiers.is_empty() {
                    self.driver
                        .post_apple_fn_flags(true)
                        .and_then(|()| self.driver.post_apple_fn_flags(false))
                } else {
                    self.driver
                        .post_key_shortcut(*key_code, modifiers, true)
                        .and_then(|()| self.driver.post_key_shortcut(*key_code, modifiers, false))
                };
                match res {
                    Ok(()) => make_receipt("executed", desc.detail),
                    Err(err) => make_receipt("failed", err),
                }
            }
            ResolvedHostAction::PasteText {
                text,
                target_bundle_id,
                adapter,
                no_newline,
                submit,
            } => {
                if !self.driver.is_accessibility_trusted() {
                    return make_receipt("blocked", "accessibility_untrusted".to_string());
                }
                let sanitized = if *no_newline {
                    text.trim_end_matches(['\r', '\n'])
                } else {
                    text.as_str()
                };
                if sanitized.trim().is_empty() {
                    return make_receipt("blocked", "empty_paste_text".to_string());
                }
                let managed = target_bundle_id
                    .as_deref()
                    .and_then(managed_adapter_for_bundle)
                    .or_else(|| {
                        adapter
                            .as_deref()
                            .filter(|adapter| is_managed_adapter(adapter))
                    });
                if let Some(managed) = managed {
                    let expected_bundle = bundle_id_for_adapter(managed);
                    if target_bundle_id
                        .as_deref()
                        .is_some_and(|bundle| Some(bundle) != expected_bundle)
                        || adapter.as_deref().is_some_and(|adapter| {
                            bundle_id_for_adapter(adapter) != expected_bundle
                        })
                    {
                        return make_receipt("blocked", "blocked:managed_target_mismatch".into());
                    }
                    let result = if *submit {
                        self.driver.submit_agent_input(managed, Some(sanitized))
                    } else {
                        self.driver.insert_agent_prompt(managed, sanitized)
                    };
                    return match result {
                        Ok(()) => make_receipt("executed", desc.detail),
                        Err(error) => make_receipt(managed_error_outcome(&error), error),
                    };
                }
                if let Some(bundle_id) = target_bundle_id {
                    let _ = self.driver.open_application(bundle_id);
                }
                let snapshot = self.driver.snapshot_clipboard(MAX_CLIPBOARD_STASH_BYTES);
                let written_cc = self.driver.write_clipboard_plain_text(sanitized);
                let cmd_mod = vec!["command".to_string()];
                let paste_res = self
                    .driver
                    .post_key_shortcut(KEY_CODE_V, &cmd_mod, true)
                    .and_then(|()| self.driver.post_key_shortcut(KEY_CODE_V, &cmd_mod, false));
                let submit_res = if *submit {
                    paste_res.and_then(|()| {
                        self.driver
                            .post_key_shortcut(KEY_CODE_RETURN, &[], true)
                            .and_then(|()| {
                                self.driver.post_key_shortcut(KEY_CODE_RETURN, &[], false)
                            })
                    })
                } else {
                    paste_res
                };
                let _ = self.driver.restore_clipboard(&snapshot, written_cc);
                match submit_res {
                    Ok(()) => make_receipt("executed", desc.detail),
                    Err(err) => make_receipt("failed", err),
                }
            }
            ResolvedHostAction::OpenApp { bundle_id } => {
                match self.driver.open_application(bundle_id) {
                    Ok(()) => make_receipt("executed", desc.detail),
                    Err(err) => make_receipt("failed", err),
                }
            }
            ResolvedHostAction::OpenUrl { url } => match self.driver.open_url(url) {
                Ok(()) => make_receipt("executed", desc.detail),
                Err(err) => make_receipt("failed", err),
            },
            ResolvedHostAction::ShellCommand {
                command,
                working_directory,
            } => {
                if !command.trim().is_empty()
                    && !working_directory.trim().is_empty()
                    && Path::new(working_directory).is_dir()
                {
                    let status = Command::new("/bin/sh")
                        .args(["-c", command])
                        .current_dir(working_directory)
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();
                    match status {
                        Ok(s) if s.success() => make_receipt("executed", desc.detail),
                        Ok(s) => make_receipt("failed", format!("shell_exit_{s}")),
                        Err(err) => make_receipt("failed", err.to_string()),
                    }
                } else {
                    make_receipt("executed", desc.detail)
                }
            }
            ResolvedHostAction::AgentCommand { adapter, command } => {
                if command == "toggle-dictation" {
                    if !self.driver.is_accessibility_trusted() {
                        return make_receipt("blocked", "accessibility_untrusted".to_string());
                    }
                    if !matches!(
                        crate::ax_dictation::DictationAdapter::parse(adapter),
                        Some(
                            crate::ax_dictation::DictationAdapter::Cindy
                                | crate::ax_dictation::DictationAdapter::Antigravity
                                | crate::ax_dictation::DictationAdapter::Dsh
                        )
                    ) {
                        return make_receipt("blocked", "unsupported_dictation_adapter".into());
                    }
                    match self.driver.toggle_dictation(adapter) {
                        Ok(state) => make_receipt(
                            "executed",
                            match state {
                                ObservedDictationState::Idle => "dictation_stopped",
                                ObservedDictationState::Recording => "dictation_started",
                            }
                            .into(),
                        ),
                        Err(error) => make_receipt(managed_error_outcome(&error), error),
                    }
                } else if command == "submit" || command.starts_with("submit-prompt:") {
                    if !self.driver.is_accessibility_trusted() {
                        return make_receipt("blocked", "accessibility_untrusted".to_string());
                    }
                    let prompt = command
                        .strip_prefix("submit-prompt:")
                        .map(|prompt| prompt.trim_end_matches(['\r', '\n']));
                    match self.driver.submit_agent_input(adapter, prompt) {
                        Ok(()) => make_receipt("executed", desc.detail),
                        Err(error) => make_receipt(managed_error_outcome(&error), error),
                    }
                } else if let Some(prompt) = command.strip_prefix("prompt:") {
                    if !self.driver.is_accessibility_trusted() {
                        return make_receipt("blocked", "accessibility_untrusted".to_string());
                    }
                    match self
                        .driver
                        .insert_agent_prompt(adapter, prompt.trim_end_matches(['\r', '\n']))
                    {
                        Ok(()) => make_receipt("executed", desc.detail),
                        Err(error) => make_receipt(managed_error_outcome(&error), error),
                    }
                } else if let Some(scroll_req) = parse_dial_scroll_command(adapter, command) {
                    if !self.driver.is_accessibility_trusted() {
                        return make_receipt("blocked", "accessibility_untrusted".to_string());
                    }
                    match self.driver.scroll_window(&scroll_req) {
                        Ok(()) => make_receipt("executed", desc.detail),
                        Err(error) => make_receipt(managed_error_outcome(&error), error),
                    }
                } else if command == "focus-input" {
                    if !self.driver.is_accessibility_trusted() {
                        return make_receipt("blocked", "accessibility_untrusted".to_string());
                    }
                    match self.driver.focus_agent_input(adapter) {
                        Ok(()) => make_receipt("executed", desc.detail),
                        Err(error) => make_receipt(managed_error_outcome(&error), error),
                    }
                } else if command == "new-conversation-in-project" {
                    if !self.driver.is_accessibility_trusted() {
                        return make_receipt("blocked", "accessibility_untrusted".to_string());
                    }
                    match self.driver.new_conversation_in_project(adapter) {
                        Ok(()) => make_receipt("executed", desc.detail),
                        Err(err) => make_receipt("failed", err),
                    }
                } else if command == "live-voice" {
                    if !self.driver.is_accessibility_trusted() {
                        return make_receipt("blocked", "accessibility_untrusted".to_string());
                    }
                    match self.driver.toggle_live_voice(adapter) {
                        Ok(()) => make_receipt("executed", desc.detail),
                        Err(err) => make_receipt("failed", err),
                    }
                } else if let Some((key_code, modifiers)) =
                    native_shortcut_for_agent_command(adapter, command)
                {
                    if !self.driver.is_accessibility_trusted() {
                        return make_receipt("blocked", "accessibility_untrusted".to_string());
                    }
                    match self.driver.agent_shortcut(adapter, key_code, &modifiers) {
                        Ok(()) => make_receipt("executed", desc.detail),
                        Err(err) => make_receipt(managed_error_outcome(&err), err),
                    }
                } else if matches!(command.as_str(), "dictation" | "push-to-talk") {
                    if !self.driver.is_accessibility_trusted() {
                        return make_receipt("blocked", "accessibility_untrusted".to_string());
                    }
                    if let Some(bundle_id) = bundle_id_for_adapter(adapter) {
                        let _ = self.driver.open_application(bundle_id);
                    }
                    match self
                        .driver
                        .post_apple_fn_flags(true)
                        .and_then(|()| self.driver.post_apple_fn_flags(false))
                    {
                        Ok(()) => make_receipt("executed", desc.detail),
                        Err(err) => make_receipt("failed", err),
                    }
                } else {
                    if let Some(bundle_id) = bundle_id_for_adapter(adapter) {
                        let _ = self.driver.open_application(bundle_id);
                    }
                    make_receipt("executed", desc.detail)
                }
            }
            ResolvedHostAction::FocusSession { .. } => make_receipt("executed", desc.detail),
            ResolvedHostAction::MacroSequence { events } => {
                if !self.driver.is_accessibility_trusted() {
                    return make_receipt("blocked", "accessibility_untrusted".to_string());
                }
                for event in events {
                    let res = match event {
                        ResolvedMacroEvent::Press { key_code, .. } => {
                            self.driver.post_key_shortcut(*key_code, &[], true)
                        }
                        ResolvedMacroEvent::Release { key_code, .. } => {
                            self.driver.post_key_shortcut(*key_code, &[], false)
                        }
                        ResolvedMacroEvent::Delay { .. } => Ok(()),
                    };
                    if let Err(err) = res {
                        return make_receipt("failed", err);
                    }
                }
                make_receipt("executed", desc.detail)
            }
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct SimulatedHostActionExecutor;

impl SimulatedHostActionExecutor {
    pub fn new() -> Self {
        Self
    }

    pub fn execute(
        &self,
        profile_id: &str,
        binding_id: &str,
        action: &ResolvedHostAction,
    ) -> HostActionReceipt {
        let desc = describe_resolved_action(action);
        HostActionReceipt {
            action_type: desc.action_type,
            binding_id: binding_id.to_string(),
            detail: desc.detail,
            outcome: "simulated".to_string(),
            profile_id: profile_id.to_string(),
            target: desc.target,
            text_byte_len: desc.text_byte_len,
            timestamp_ms: now_timestamp_ms(),
        }
    }

    pub fn execute_and_record(
        &self,
        profile_id: &str,
        binding_id: &str,
        action: &ResolvedHostAction,
        audit_log_path: &Path,
    ) -> io::Result<HostActionReceipt> {
        let receipt = self.execute(profile_id, binding_id, action);
        receipt.append_to_audit_log(audit_log_path)?;
        Ok(receipt)
    }
}

fn managed_error_outcome(error: &str) -> &'static str {
    if error.starts_with("blocked:") {
        "blocked"
    } else {
        "failed"
    }
}

fn legacy_submit_agent_input<D: HostOsDriver + ?Sized>(
    driver: &D,
    adapter: &str,
    prompt: Option<&str>,
) -> Result<(), String> {
    driver.stop_active_dictation(adapter)?;
    driver.focus_agent_input(adapter)?;
    let submit_modifiers = submit_modifiers_for_adapter(adapter);
    if let Some(prompt) = prompt {
        let snapshot = driver.snapshot_clipboard(MAX_CLIPBOARD_STASH_BYTES);
        let written = driver.write_clipboard_plain_text(prompt);
        let command = vec!["command".to_owned()];
        let result = driver
            .post_key_shortcut(KEY_CODE_V, &command, true)
            .and_then(|()| driver.post_key_shortcut(KEY_CODE_V, &command, false))
            .and_then(|()| driver.post_key_shortcut(KEY_CODE_RETURN, &submit_modifiers, true))
            .and_then(|()| driver.post_key_shortcut(KEY_CODE_RETURN, &submit_modifiers, false));
        driver.restore_clipboard(&snapshot, written);
        result
    } else {
        driver.post_key_shortcut(KEY_CODE_RETURN, &submit_modifiers, true)?;
        driver.post_key_shortcut(KEY_CODE_RETURN, &submit_modifiers, false)
    }
}

fn legacy_insert_agent_prompt<D: HostOsDriver + ?Sized>(
    driver: &D,
    adapter: &str,
    prompt: &str,
) -> Result<(), String> {
    driver.focus_agent_input(adapter)?;
    let snapshot = driver.snapshot_clipboard(MAX_CLIPBOARD_STASH_BYTES);
    let written = driver.write_clipboard_plain_text(prompt);
    let modifiers = vec!["command".to_owned()];
    let result = driver
        .post_key_shortcut(KEY_CODE_V, &modifiers, true)
        .and_then(|()| driver.post_key_shortcut(KEY_CODE_V, &modifiers, false));
    driver.restore_clipboard(&snapshot, written);
    result
}

fn is_managed_adapter(adapter: &str) -> bool {
    crate::managed_controls::managed_adapter(adapter).is_ok()
}

fn is_managed_bundle(bundle: &str) -> bool {
    managed_adapter_for_bundle(bundle).is_some()
}

fn managed_adapter_for_bundle(bundle: &str) -> Option<&'static str> {
    match bundle {
        "com.xd.cindy" => Some("cindy"),
        "com.google.antigravity" => Some("antigravity"),
        "com.deepseek.dsh" => Some("dsh-desktop"),
        _ => None,
    }
}
