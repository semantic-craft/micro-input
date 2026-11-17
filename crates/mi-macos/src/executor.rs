use crate::managed_controls::{
    CodexReasoningAction, ManagedAxControls, ManagedControlReadiness, ModelPickerConfirmResult,
    ObservedDictationState, SubagentOpenResult,
};
use crate::native_ax::NativeAxProvider;
use mi_model::host_router::{ResolvedHostAction, ResolvedMacroEvent};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const APPLE_FN_KEY_CODE: u16 = 0x3F;
pub const KEY_CODE_V: u16 = 0x09;
pub const KEY_CODE_RETURN: u16 = 0x24;
pub const KEY_CODE_N: u16 = 0x2D;
pub const CG_EVENT_TYPE_FLAGS_CHANGED: u32 = 12;
pub const CG_EVENT_FLAG_MASK_SECONDARY_FN: u64 = 0x0080_0000;
pub const MAX_CLIPBOARD_STASH_BYTES: usize = 4 * 1024 * 1024;
pub const DEFAULT_SHELL_COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
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

pub(crate) fn is_antigravity_live_voice_start_label(label: &str) -> bool {
    let trimmed = label.trim();
    trimmed.eq_ignore_ascii_case("Start Voice mode")
        || trimmed.eq_ignore_ascii_case("Start Live Chat")
}

pub(crate) fn is_antigravity_live_voice_end_label(label: &str) -> bool {
    let trimmed = label.trim();
    trimmed.eq_ignore_ascii_case("End Voice mode") || trimmed.eq_ignore_ascii_case("End Live chat")
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

pub fn is_bundle_running(bundle_id: &str) -> bool {
    let trimmed = bundle_id.trim();
    if trimmed.is_empty() || trimmed == "*" {
        return false;
    }
    let Ok(output) = Command::new("/usr/bin/lsappinfo")
        .args(["info", "-only", "pid", trimmed])
        .output()
    else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    parse_pid_from_lsappinfo_output(&String::from_utf8_lossy(&output.stdout)).is_some()
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
type CFDictionaryRef = *const std::ffi::c_void;
#[cfg(target_os = "macos")]
type CFArrayRef = *const std::ffi::c_void;
#[cfg(target_os = "macos")]
type CFMachPortRef = *mut std::ffi::c_void;
#[cfg(target_os = "macos")]
type CFRunLoopRef = *mut std::ffi::c_void;
#[cfg(target_os = "macos")]
type CFRunLoopSourceRef = *mut std::ffi::c_void;
#[cfg(target_os = "macos")]
type CGEventTapProxy = *mut std::ffi::c_void;
#[cfg(target_os = "macos")]
type CGEventMask = u64;
#[cfg(target_os = "macos")]
type CGEventTapCallBack = unsafe extern "C" fn(
    proxy: CGEventTapProxy,
    event_type: u32,
    event: CGEventRef,
    user_info: *mut std::ffi::c_void,
) -> CGEventRef;
#[cfg(target_os = "macos")]
type CFIndex = isize;
#[cfg(target_os = "macos")]
type AXError = i32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButtonKind {
    Left,
    Right,
    Middle,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct CGSize {
    width: f64,
    height: f64,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

#[cfg(target_os = "macos")]
const K_CG_MOUSE_EVENT_CLICK_STATE: u32 = 1;
#[cfg(target_os = "macos")]
const CG_EVENT_LEFT_MOUSE_DOWN: u32 = 1;
#[cfg(target_os = "macos")]
const CG_EVENT_LEFT_MOUSE_UP: u32 = 2;
#[cfg(target_os = "macos")]
const CG_EVENT_RIGHT_MOUSE_DOWN: u32 = 3;
#[cfg(target_os = "macos")]
const CG_EVENT_RIGHT_MOUSE_UP: u32 = 4;
#[cfg(target_os = "macos")]
const CG_EVENT_MOUSE_MOVED: u32 = 5;
#[cfg(target_os = "macos")]
const CG_EVENT_OTHER_MOUSE_DOWN: u32 = 25;
#[cfg(target_os = "macos")]
const CG_EVENT_OTHER_MOUSE_UP: u32 = 26;
#[cfg(target_os = "macos")]
const CG_MOUSE_BUTTON_LEFT: u32 = 0;
#[cfg(target_os = "macos")]
const CG_MOUSE_BUTTON_RIGHT: u32 = 1;
#[cfg(target_os = "macos")]
const CG_MOUSE_BUTTON_CENTER: u32 = 2;

#[cfg(target_os = "macos")]
const CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE: i32 = 1;
#[cfg(target_os = "macos")]
const CG_HID_EVENT_TAP: u32 = 0;
#[cfg(target_os = "macos")]
const CG_SESSION_EVENT_TAP: u32 = 1;
#[cfg(target_os = "macos")]
const CG_HEAD_INSERT_EVENT_TAP: u32 = 0;
#[cfg(target_os = "macos")]
const CG_EVENT_TAP_OPTION_DEFAULT: u32 = 0;
#[cfg(target_os = "macos")]
const CG_EVENT_TAP_OPTION_LISTEN_ONLY: u32 = 1;
#[cfg(target_os = "macos")]
const CG_EVENT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
#[cfg(target_os = "macos")]
const CG_EVENT_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;
#[cfg(target_os = "macos")]
const K_CG_MOUSE_EVENT_DELTA_X: u32 = 4;
#[cfg(target_os = "macos")]
const K_CG_MOUSE_EVENT_DELTA_Y: u32 = 5;
#[cfg(target_os = "macos")]
const MAX_MODEL_PICKER_MOUSE_DELTAS: usize = 256;
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
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
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
    fn AXUIElementGetTypeID() -> usize;
    fn CFGetTypeID(cf: CFTypeRef) -> usize;
    fn CFStringGetTypeID() -> usize;
    fn CFArrayGetTypeID() -> usize;
    static kCFBooleanTrue: CFTypeRef;
    static kCFBooleanFalse: CFTypeRef;
    static kCFTypeDictionaryKeyCallBacks: u8;
    static kCFTypeDictionaryValueCallBacks: u8;
    static kCFRunLoopCommonModes: CFStringRef;
    static kCFRunLoopDefaultMode: CFStringRef;
    fn CGEventSourceCreate(state_id: i32) -> CGEventSourceRef;
    fn CGEventCreate(source: CGEventSourceRef) -> CGEventRef;
    fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    fn CGEventGetDoubleValueField(event: CGEventRef, field: u32) -> f64;
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: CGEventMask,
        callback: CGEventTapCallBack,
        user_info: *mut std::ffi::c_void,
    ) -> CFMachPortRef;
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    fn CFMachPortCreateRunLoopSource(
        allocator: CFTypeRef,
        port: CFMachPortRef,
        order: CFIndex,
    ) -> CFRunLoopSourceRef;
    fn CFMachPortInvalidate(port: CFMachPortRef);
    fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    fn CFRunLoopRemoveSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    fn CFRunLoopRunInMode(mode: CFStringRef, seconds: f64, return_after_source_handled: u8) -> i32;
    fn CFRunLoopStop(rl: CFRunLoopRef);
    fn CGEventCreateMouseEvent(
        source: CGEventSourceRef,
        mouse_type: u32,
        mouse_cursor_position: CGPoint,
        mouse_button: u32,
    ) -> CGEventRef;
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
    fn CGEventSetIntegerValueField(event: CGEventRef, field: u32, value: i64);
    fn CGEventPost(tap: u32, event: CGEventRef);
    fn CGEventPostToPid(pid: i32, event: CGEventRef);
    fn CGGetActiveDisplayList(
        max_displays: u32,
        active_displays: *mut u32,
        display_count: *mut u32,
    ) -> i32;
    fn CGDisplayBounds(display: u32) -> CGRect;
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
    fn CFDictionaryCreate(
        allocator: CFTypeRef,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        num_values: CFIndex,
        key_call_backs: CFTypeRef,
        value_call_backs: CFTypeRef,
    ) -> CFDictionaryRef;
    fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
    fn CFRelease(cf: CFTypeRef);
}

#[cfg(target_os = "macos")]
unsafe fn with_accessibility_prompt_options<R>(
    prompt: bool,
    f: impl FnOnce(CFDictionaryRef) -> R,
) -> Option<R> {
    let key = cf_string_from_str("AXTrustedCheckOptionPrompt");
    if key.is_null() {
        return None;
    }
    let value = if prompt {
        kCFBooleanTrue
    } else {
        kCFBooleanFalse
    };
    let keys = [key as CFTypeRef];
    let values = [value];
    let dict = CFDictionaryCreate(
        std::ptr::null(),
        keys.as_ptr(),
        values.as_ptr(),
        1,
        (&raw const kCFTypeDictionaryKeyCallBacks).cast(),
        (&raw const kCFTypeDictionaryValueCallBacks).cast(),
    );
    let out = if !dict.is_null() {
        let res = f(dict);
        CFRelease(dict);
        Some(res)
    } else {
        None
    };
    CFRelease(key);
    out
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
    if cf.is_null() || CFGetTypeID(cf) != CFStringGetTypeID() {
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
unsafe fn ax_copy_raw_attr(elem: AXUIElementRef, attr: &str) -> Option<CFTypeRef> {
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
unsafe fn cf_array_copy_ax_elements(owned_val: CFTypeRef, limit: isize) -> Vec<AXUIElementRef> {
    if owned_val.is_null() {
        return Vec::new();
    }
    if CFGetTypeID(owned_val) != CFArrayGetTypeID() {
        CFRelease(owned_val);
        return Vec::new();
    }
    let ax_elem_type = AXUIElementGetTypeID();
    let count = CFArrayGetCount(owned_val);
    let mut out = Vec::new();
    if count > 0 {
        for i in 0..count.min(limit) {
            let item = CFArrayGetValueAtIndex(owned_val, i);
            if !item.is_null() && CFGetTypeID(item) == ax_elem_type {
                CFRetain(item);
                out.push(item);
            }
        }
    }
    CFRelease(owned_val);
    out
}

#[cfg(target_os = "macos")]
unsafe fn ax_copy_string_attr(elem: AXUIElementRef, attr: &str) -> Option<String> {
    let val = ax_copy_raw_attr(elem, attr)?;
    let out = cf_string_to_rust(val);
    CFRelease(val);
    out
}

#[cfg(target_os = "macos")]
unsafe fn ax_copy_element_attr(elem: AXUIElementRef, attr: &str) -> Option<AXUIElementRef> {
    let val = ax_copy_raw_attr(elem, attr)?;
    if CFGetTypeID(val) != AXUIElementGetTypeID() {
        CFRelease(val);
        return None;
    }
    Some(val)
}

#[cfg(target_os = "macos")]
unsafe fn ax_copy_children(elem: AXUIElementRef) -> Vec<AXUIElementRef> {
    match ax_copy_raw_attr(elem, "AXChildren") {
        Some(val) => cf_array_copy_ax_elements(val, 256),
        None => Vec::new(),
    }
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
        "codex" => Some("com.openai.codex"),
        "chatgpt" => Some("com.openai.atlas"),
        "dsh-desktop" | "dsh" | "deepseek" => Some("com.deepseek.dsh"),
        "herdr" => Some("dev.herdr.app"),
        _ => None,
    }
}

pub fn model_picker_navigation_key_code(command: &str) -> Option<u16> {
    match command.trim() {
        "model-picker-prev" | "search-prev" | "subagent-prev" => Some(0x7E),
        "model-picker-next" | "search-next" | "subagent-next" => Some(0x7D),
        "model-picker-effort-right" => Some(0x7C),
        "model-picker-effort-left" => Some(0x7B),
        "model-picker-confirm" | "search-confirm" | "subagent-confirm" => Some(KEY_CODE_RETURN),
        "model-picker-cancel" | "subagent-cancel" => Some(0x35),
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
        ("antigravity", "model" | "effort" | "model-picker-open") => {
            Some((0x2C, mods(&["command"])))
        }
        ("antigravity", "project" | "prev-project" | "next-project") => {
            Some((0x29, mods(&["command"])))
        }
        ("antigravity", "browser") => Some((0x0B, mods(&["command", "shift"]))),
        ("antigravity", "terminal") => Some((0x26, mods(&["command"]))),
        ("antigravity", "dictation" | "push-to-talk") => Some((0x2E, mods(&["control"]))),
        ("antigravity", "split-right") => Some((0x02, mods(&["command"]))),
        (
            "antigravity" | "dsh-desktop",
            "prev-conversation" | "working-prev" | "completed-prev" | "attention-prev",
        ) => Some((0x7E, mods(&["option"]))),
        (
            "antigravity" | "dsh-desktop",
            "next-conversation" | "working-next" | "completed-next" | "attention-next",
        ) => Some((0x7D, mods(&["option"]))),
        ("dsh-desktop", "new-chat" | "new-or-stop") => Some((0x2D, mods(&["command"]))),
        ("dsh-desktop", "search" | "focus-input") => Some((0x28, mods(&["command"]))),
        ("dsh-desktop", "model" | "effort" | "model-picker-open") => {
            Some((0x2C, mods(&["command"])))
        }
        ("dsh-desktop", "project" | "prev-project" | "next-project") => {
            Some((0x1F, mods(&["command"])))
        }
        ("dsh-desktop", "terminal") => Some((0x32, mods(&["control"]))),
        ("dsh-desktop", "browser") => Some((0x11, mods(&["command"]))),
        ("codex", "new-chat" | "new-or-stop") => Some((0x2D, mods(&["command"]))),
        ("codex", "search") => Some((0x28, mods(&["command"]))),
        ("codex", "model" | "effort" | "model-picker-open") => {
            Some((0x2E, mods(&["control", "shift"])))
        }
        ("codex", "live-voice") => Some((0x09, mods(&["control", "shift"]))),
        ("codex", "new-tab") => Some((0x11, mods(&["command"]))),
        ("codex", "terminal") => Some((0x32, mods(&["control"]))),
        ("codex", "cycle-workspace-layout") => Some((0x0B, mods(&["command", "shift"]))),
        ("chatgpt", "new-chat") => Some((0x2D, mods(&["command"]))),
        ("chatgpt", "search") => Some((0x03, mods(&["command"]))),
        ("chatgpt", "model") => Some((0x30, mods(&["shift"]))),
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
        let mut line = serde_json::to_string(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        line.push('\n');
        let mut file =
            crate::private_fs::open_private_file(path, crate::private_fs::PrivateOpenMode::Append)
                .map_err(io::Error::other)?;
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
    fn is_accessibility_trusted(&self) -> bool {
        true
    }
    fn open_accessibility_settings(&self) -> Result<(), String> {
        Ok(())
    }
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
    fn write_clipboard_plain_text_checked(&self, text: &str) -> Result<u64, String> {
        Ok(self.write_clipboard_plain_text(text))
    }
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
    fn move_cursor_by(&self, dx: f64, dy: f64) -> Result<(), String>;
    fn click_mouse_button(&self, button: MouseButtonKind) -> Result<(), String>;
    fn scroll_cursor_wheel(&self, dx_lines: i32, dy_lines: i32) -> Result<(), String>;
    fn new_conversation_in_project(&self, adapter: &str) -> Result<(), String>;
    fn toggle_live_voice(&self, adapter: &str) -> Result<(), String>;
    fn codex_reasoning(&self, _action: CodexReasoningAction) -> Result<(), String> {
        Err("blocked:codex_reasoning_unavailable".into())
    }
    fn open_model_picker(&self, adapter: &str) -> Result<(), String> {
        let Some((key_code, modifiers)) =
            native_shortcut_for_agent_command(adapter, "model-picker-open")
                .or_else(|| native_shortcut_for_agent_command(adapter, "model"))
        else {
            return Err("blocked:unsupported_agent_command".into());
        };
        self.agent_shortcut(adapter, key_code, &modifiers)
    }
    fn open_side_question(&self, _adapter: &str) -> Result<(), String> {
        Err("blocked:unsupported_agent_command".into())
    }
    fn open_side_panel(&self, _adapter: &str) -> Result<(), String> {
        Err("blocked:unsupported_side_panel_adapter".into())
    }
    fn close_side_panel(&self, _adapter: &str) -> Result<(), String> {
        Err("blocked:unsupported_side_panel_adapter".into())
    }
    fn send_model_picker_key(&self, adapter: &str, key_code: u16) -> Result<(), String> {
        Err(format!(
            "blocked:model_picker_key_unavailable:{adapter}:{key_code:x}"
        ))
    }
    fn step_model_picker(&self, adapter: &str, key_code: u16) -> Result<(), String> {
        self.send_model_picker_key(adapter, key_code)
    }
    fn confirm_model_picker(&self, adapter: &str) -> Result<ModelPickerConfirmResult, String> {
        self.send_model_picker_key(adapter, KEY_CODE_RETURN)?;
        Ok(ModelPickerConfirmResult::Confirmed)
    }
    fn open_subagent_picker(&self, _adapter: &str) -> Result<SubagentOpenResult, String> {
        Err("blocked:subagent_picker_unavailable".into())
    }
    fn step_subagent_picker(&self, adapter: &str, key_code: u16) -> Result<(), String> {
        self.send_model_picker_key(adapter, key_code)
    }
    fn confirm_subagent_picker(&self, adapter: &str) -> Result<ModelPickerConfirmResult, String> {
        self.send_model_picker_key(adapter, KEY_CODE_RETURN)?;
        Ok(ModelPickerConfirmResult::Confirmed)
    }
    fn resolve_default_managed_adapter(&self, profile_id: &str) -> &'static str {
        infer_managed_adapter_from_profile_id(profile_id).unwrap_or("antigravity")
    }
    fn set_isolated_mode(&self, _isolated: bool) {}
    fn set_model_picker_mouse_capture(&self, _enabled: bool) {}
    fn drain_model_picker_mouse_deltas(&self) -> Vec<(f64, f64)> {
        Vec::new()
    }
}

fn infer_managed_adapter_from_profile_id(profile_id: &str) -> Option<&'static str> {
    let lower = profile_id.trim().to_ascii_lowercase();
    if lower.contains("codex") {
        Some("codex")
    } else if lower.contains("dsh") || lower.contains("deepseek") {
        Some("dsh-desktop")
    } else if lower.contains("antigravity") {
        Some("antigravity")
    } else {
        None
    }
}

fn clamp_point_to_rects(point: (f64, f64), rects: &[(f64, f64, f64, f64)]) -> (f64, f64) {
    let mut best: Option<((f64, f64), f64)> = None;
    for &(ox, oy, width, height) in rects {
        if !ox.is_finite()
            || !oy.is_finite()
            || !width.is_finite()
            || !height.is_finite()
            || width <= 0.0
            || height <= 0.0
        {
            continue;
        }
        let max_x = (ox + width - 1.0).max(ox);
        let max_y = (oy + height - 1.0).max(oy);
        let cx = point.0.clamp(ox, max_x);
        let cy = point.1.clamp(oy, max_y);
        let dx = cx - point.0;
        let dy = cy - point.1;
        let dist_sq = dx * dx + dy * dy;
        if dist_sq == 0.0 {
            return (cx, cy);
        }
        if best.is_none_or(|(_, best_dist)| dist_sq < best_dist) {
            best = Some(((cx, cy), dist_sq));
        }
    }
    best.map_or(point, |(clamped, _)| clamped)
}

#[cfg(target_os = "macos")]
struct TapCallbackContext {
    active: AtomicBool,
    suppress_mouse_moved: AtomicBool,
    tap_port: AtomicUsize,
    run_loop_addr: AtomicUsize,
    deltas: Arc<Mutex<Vec<(f64, f64)>>>,
}

#[cfg(target_os = "macos")]
unsafe extern "C" fn model_picker_mouse_tap_callback(
    _proxy: CGEventTapProxy,
    event_type: u32,
    event: CGEventRef,
    user_info: *mut std::ffi::c_void,
) -> CGEventRef {
    if user_info.is_null() || event.is_null() {
        return event;
    }
    let ctx = &*(user_info as *const TapCallbackContext);
    if !ctx.active.load(Ordering::Acquire) {
        return event;
    }
    if event_type == CG_EVENT_TAP_DISABLED_BY_TIMEOUT {
        let port = ctx.tap_port.load(Ordering::Acquire) as CFMachPortRef;
        if !port.is_null() {
            CGEventTapEnable(port, true);
        }
        return event;
    }
    if event_type == CG_EVENT_TAP_DISABLED_BY_USER_INPUT {
        return event;
    }
    if event_type == CG_EVENT_MOUSE_MOVED {
        let dx = CGEventGetDoubleValueField(event, K_CG_MOUSE_EVENT_DELTA_X);
        let dy = CGEventGetDoubleValueField(event, K_CG_MOUSE_EVENT_DELTA_Y);
        if (dx != 0.0 || dy != 0.0) && dx.is_finite() && dy.is_finite() {
            if let Ok(mut queue) = ctx.deltas.lock() {
                if queue.len() >= MAX_MODEL_PICKER_MOUSE_DELTAS {
                    let drop_count = queue.len() + 1 - MAX_MODEL_PICKER_MOUSE_DELTAS;
                    queue.drain(0..drop_count);
                }
                queue.push((dx, dy));
            }
        }
        if ctx.suppress_mouse_moved.load(Ordering::Acquire) {
            return std::ptr::null_mut();
        }
    }
    event
}

#[cfg(target_os = "macos")]
trait TapRuntime: Send + Sync + 'static {
    unsafe fn create_tap(&self, options: u32, ctx: *mut std::ffi::c_void) -> CFMachPortRef;
    unsafe fn create_source(&self, tap: CFMachPortRef) -> CFRunLoopSourceRef;
    unsafe fn invalidate_port(&self, tap: CFMachPortRef);
    unsafe fn current_run_loop(&self) -> CFRunLoopRef;
    unsafe fn add_source(&self, rl: CFRunLoopRef, source: CFRunLoopSourceRef);
    unsafe fn remove_source(&self, rl: CFRunLoopRef, source: CFRunLoopSourceRef);
    unsafe fn enable_tap(&self, tap: CFMachPortRef, enable: bool);
    unsafe fn run_loop_once(&self);
    unsafe fn stop_run_loop(&self, rl: CFRunLoopRef);
    unsafe fn retain_cf(&self, cf: CFTypeRef) -> CFTypeRef;
    unsafe fn release_cf(&self, cf: CFTypeRef);
}

#[cfg(target_os = "macos")]
struct LiveTapRuntime;

#[cfg(target_os = "macos")]
impl TapRuntime for LiveTapRuntime {
    unsafe fn create_tap(&self, options: u32, ctx: *mut std::ffi::c_void) -> CFMachPortRef {
        CGEventTapCreate(
            CG_SESSION_EVENT_TAP,
            CG_HEAD_INSERT_EVENT_TAP,
            options,
            1u64 << CG_EVENT_MOUSE_MOVED,
            model_picker_mouse_tap_callback,
            ctx,
        )
    }
    unsafe fn create_source(&self, tap: CFMachPortRef) -> CFRunLoopSourceRef {
        CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0)
    }
    unsafe fn invalidate_port(&self, tap: CFMachPortRef) {
        CFMachPortInvalidate(tap);
    }
    unsafe fn current_run_loop(&self) -> CFRunLoopRef {
        CFRunLoopGetCurrent()
    }
    unsafe fn add_source(&self, rl: CFRunLoopRef, source: CFRunLoopSourceRef) {
        CFRunLoopAddSource(rl, source, kCFRunLoopCommonModes);
    }
    unsafe fn remove_source(&self, rl: CFRunLoopRef, source: CFRunLoopSourceRef) {
        CFRunLoopRemoveSource(rl, source, kCFRunLoopCommonModes);
    }
    unsafe fn enable_tap(&self, tap: CFMachPortRef, enable: bool) {
        CGEventTapEnable(tap, enable);
    }
    unsafe fn run_loop_once(&self) {
        let _ = CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.05, 0);
    }
    unsafe fn stop_run_loop(&self, rl: CFRunLoopRef) {
        CFRunLoopStop(rl);
    }
    unsafe fn retain_cf(&self, cf: CFTypeRef) -> CFTypeRef {
        CFRetain(cf)
    }
    unsafe fn release_cf(&self, cf: CFTypeRef) {
        CFRelease(cf);
    }
}

#[cfg(target_os = "macos")]
fn cleanup_mouse_tap(
    ctx_ptr: *mut TapCallbackContext,
    join_handle: Option<std::thread::JoinHandle<()>>,
    runtime: &dyn TapRuntime,
) {
    unsafe {
        (*ctx_ptr).active.store(false, Ordering::SeqCst);
        let rl_addr = (*ctx_ptr).run_loop_addr.load(Ordering::SeqCst);
        if rl_addr != 0 {
            runtime.stop_run_loop(rl_addr as CFRunLoopRef);
        }
        if let Some(handle) = join_handle {
            let _ = handle.join();
        }
        let retained_rl = (*ctx_ptr).run_loop_addr.swap(0, Ordering::AcqRel);
        if retained_rl != 0 {
            runtime.release_cf(retained_rl as CFTypeRef);
        }
        drop(Box::from_raw(ctx_ptr));
    }
}

#[cfg(target_os = "macos")]
struct ModelPickerMouseTap {
    ctx_ptr: *mut TapCallbackContext,
    join_handle: Option<std::thread::JoinHandle<()>>,
    runtime: Arc<dyn TapRuntime>,
}

#[cfg(target_os = "macos")]
unsafe impl Send for ModelPickerMouseTap {}

#[cfg(target_os = "macos")]
impl ModelPickerMouseTap {
    fn start(deltas: Arc<Mutex<Vec<(f64, f64)>>>) -> Option<Self> {
        Self::start_with_runtime(deltas, Arc::new(LiveTapRuntime), Duration::from_millis(500))
    }

    fn start_with_runtime(
        deltas: Arc<Mutex<Vec<(f64, f64)>>>,
        runtime: Arc<dyn TapRuntime>,
        timeout: Duration,
    ) -> Option<Self> {
        if let Ok(mut queue) = deltas.lock() {
            queue.clear();
        }
        let ctx_ptr = Box::into_raw(Box::new(TapCallbackContext {
            active: AtomicBool::new(true),
            suppress_mouse_moved: AtomicBool::new(true),
            tap_port: AtomicUsize::new(0),
            run_loop_addr: AtomicUsize::new(0),
            deltas,
        }));
        let ctx_addr = ctx_ptr as usize;
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<bool>(1);
        let worker_runtime = Arc::clone(&runtime);
        let handle = std::thread::Builder::new()
            .name("mi-model-picker-mouse-tap".into())
            .spawn(move || unsafe {
                let ctx = &*(ctx_addr as *const TapCallbackContext);
                if !ctx.active.load(Ordering::SeqCst) {
                    let _ = ready_tx.send(false);
                    return;
                }
                let mut tap = worker_runtime.create_tap(
                    CG_EVENT_TAP_OPTION_DEFAULT,
                    ctx_addr as *mut std::ffi::c_void,
                );
                if tap.is_null() {
                    ctx.suppress_mouse_moved.store(false, Ordering::Release);
                    tap = worker_runtime.create_tap(
                        CG_EVENT_TAP_OPTION_LISTEN_ONLY,
                        ctx_addr as *mut std::ffi::c_void,
                    );
                }
                if tap.is_null() {
                    let _ = ready_tx.send(false);
                    return;
                }
                let source = worker_runtime.create_source(tap);
                if source.is_null() {
                    worker_runtime.invalidate_port(tap);
                    worker_runtime.release_cf(tap as CFTypeRef);
                    let _ = ready_tx.send(false);
                    return;
                }
                ctx.tap_port.store(tap as usize, Ordering::Release);
                let rl = worker_runtime.current_run_loop();
                if rl.is_null() {
                    worker_runtime.invalidate_port(tap);
                    worker_runtime.release_cf(source as CFTypeRef);
                    worker_runtime.release_cf(tap as CFTypeRef);
                    let _ = ready_tx.send(false);
                    return;
                }
                worker_runtime.retain_cf(rl as CFTypeRef);
                ctx.run_loop_addr.store(rl as usize, Ordering::SeqCst);
                if !ctx.active.load(Ordering::SeqCst) {
                    worker_runtime.invalidate_port(tap);
                    worker_runtime.release_cf(source as CFTypeRef);
                    worker_runtime.release_cf(tap as CFTypeRef);
                    let _ = ready_tx.send(false);
                    return;
                }
                worker_runtime.add_source(rl, source);
                worker_runtime.enable_tap(tap, true);
                if ready_tx.send(true).is_ok() {
                    while ctx.active.load(Ordering::Acquire) {
                        worker_runtime.run_loop_once();
                    }
                }
                worker_runtime.enable_tap(tap, false);
                worker_runtime.remove_source(rl, source);
                worker_runtime.invalidate_port(tap);
                worker_runtime.release_cf(source as CFTypeRef);
                worker_runtime.release_cf(tap as CFTypeRef);
            })
            .ok();
        let Some(handle) = handle else {
            cleanup_mouse_tap(ctx_ptr, None, runtime.as_ref());
            return None;
        };
        match ready_rx.recv_timeout(timeout) {
            Ok(true) => Some(Self {
                ctx_ptr,
                join_handle: Some(handle),
                runtime,
            }),
            Ok(false) | Err(_) => {
                cleanup_mouse_tap(ctx_ptr, Some(handle), runtime.as_ref());
                None
            }
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for ModelPickerMouseTap {
    fn drop(&mut self) {
        cleanup_mouse_tap(self.ctx_ptr, self.join_handle.take(), self.runtime.as_ref());
    }
}

#[cfg(not(target_os = "macos"))]
struct ModelPickerMouseTap;

pub(crate) fn is_sensitive_pasteboard_type(uti: &str) -> bool {
    uti.eq_ignore_ascii_case("org.nspasteboard.ConcealedType")
        || uti.eq_ignore_ascii_case("org.nspasteboard.TransientType")
}

#[cfg(target_os = "macos")]
mod macos_pasteboard {
    use super::{ClipboardItemSnapshot, ClipboardSnapshot};
    use std::ffi::{c_char, c_void, CStr};

    pub type Id = *mut c_void;
    type Sel = *mut c_void;

    #[link(name = "AppKit", kind = "framework")]
    #[link(name = "Foundation", kind = "framework")]
    #[link(name = "objc")]
    unsafe extern "C" {
        fn objc_getClass(name: *const c_char) -> Id;
        fn sel_registerName(name: *const c_char) -> Sel;
        fn objc_msgSend();
        fn objc_autoreleasePoolPush() -> *mut c_void;
        fn objc_autoreleasePoolPop(pool: *mut c_void);
    }

    struct AutoreleasePool(*mut c_void);

    impl AutoreleasePool {
        unsafe fn push() -> Self {
            Self(unsafe { objc_autoreleasePoolPush() })
        }
    }

    impl Drop for AutoreleasePool {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { objc_autoreleasePoolPop(self.0) };
            }
        }
    }

    unsafe fn cls(name: &[u8]) -> Id {
        unsafe { objc_getClass(name.as_ptr().cast::<c_char>()) }
    }

    unsafe fn sel(name: &[u8]) -> Sel {
        unsafe { sel_registerName(name.as_ptr().cast::<c_char>()) }
    }

    unsafe fn msg_0(obj: Id, s: Sel) -> Id {
        let f: unsafe extern "C" fn(Id, Sel) -> Id =
            unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(obj, s) }
    }

    unsafe fn msg_0_void(obj: Id, s: Sel) {
        let f: unsafe extern "C" fn(Id, Sel) =
            unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(obj, s) }
    }

    unsafe fn msg_0_isize(obj: Id, s: Sel) -> isize {
        let f: unsafe extern "C" fn(Id, Sel) -> isize =
            unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(obj, s) }
    }

    unsafe fn msg_0_usize(obj: Id, s: Sel) -> usize {
        let f: unsafe extern "C" fn(Id, Sel) -> usize =
            unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(obj, s) }
    }

    unsafe fn msg_0_cstr(obj: Id, s: Sel) -> *const c_char {
        let f: unsafe extern "C" fn(Id, Sel) -> *const c_char =
            unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(obj, s) }
    }

    unsafe fn msg_0_bytes(obj: Id, s: Sel) -> *const u8 {
        let f: unsafe extern "C" fn(Id, Sel) -> *const u8 =
            unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(obj, s) }
    }

    unsafe fn msg_1_usize(obj: Id, s: Sel, arg: usize) -> Id {
        let f: unsafe extern "C" fn(Id, Sel, usize) -> Id =
            unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(obj, s, arg) }
    }

    unsafe fn msg_1_id(obj: Id, s: Sel, arg: Id) -> Id {
        let f: unsafe extern "C" fn(Id, Sel, Id) -> Id =
            unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(obj, s, arg) }
    }

    unsafe fn msg_1_id_void(obj: Id, s: Sel, arg: Id) {
        let f: unsafe extern "C" fn(Id, Sel, Id) =
            unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(obj, s, arg) }
    }

    unsafe fn msg_1_id_bool(obj: Id, s: Sel, arg: Id) -> i8 {
        let f: unsafe extern "C" fn(Id, Sel, Id) -> i8 =
            unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(obj, s, arg) }
    }

    unsafe fn msg_2_id_id_bool(obj: Id, s: Sel, a: Id, b: Id) -> i8 {
        let f: unsafe extern "C" fn(Id, Sel, Id, Id) -> i8 =
            unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(obj, s, a, b) }
    }

    unsafe fn msg_2_ptr_usize(obj: Id, s: Sel, p: *const u8, n: usize) -> Id {
        let f: unsafe extern "C" fn(Id, Sel, *const u8, usize) -> Id =
            unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(obj, s, p, n) }
    }

    unsafe fn msg_3_ptr_usize_usize(obj: Id, s: Sel, p: *const u8, n: usize, enc: usize) -> Id {
        let f: unsafe extern "C" fn(Id, Sel, *const u8, usize, usize) -> Id =
            unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(obj, s, p, n, enc) }
    }

    static PASTEBOARD_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[derive(Debug)]
    pub struct PasteboardRef {
        unique_pb: Option<Id>,
    }

    unsafe impl Send for PasteboardRef {}
    unsafe impl Sync for PasteboardRef {}

    impl Drop for PasteboardRef {
        fn drop(&mut self) {
            if let Some(pb) = self.unique_pb.take() {
                let _guard = PASTEBOARD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
                unsafe {
                    let _pool = AutoreleasePool::push();
                    msg_0_void(pb, sel(b"releaseGlobally\0"));
                }
            }
        }
    }

    impl PasteboardRef {
        pub fn general() -> Self {
            Self { unique_pb: None }
        }

        #[allow(dead_code)]
        pub fn unique_for_test() -> Option<Self> {
            let _guard = PASTEBOARD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            unsafe {
                let _pool = AutoreleasePool::push();
                let pb = msg_0(cls(b"NSPasteboard\0"), sel(b"pasteboardWithUniqueName\0"));
                if pb.is_null() {
                    return None;
                }
                let retained = msg_0(pb, sel(b"retain\0"));
                Some(Self {
                    unique_pb: Some(retained),
                })
            }
        }

        pub fn is_general(&self) -> bool {
            self.unique_pb.is_none()
        }

        unsafe fn target(&self) -> Id {
            match self.unique_pb {
                Some(pb) => pb,
                None => unsafe { msg_0(cls(b"NSPasteboard\0"), sel(b"generalPasteboard\0")) },
            }
        }

        pub fn change_count(&self) -> u64 {
            let _guard = PASTEBOARD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            unsafe {
                let _pool = AutoreleasePool::push();
                let pb = self.target();
                if pb.is_null() {
                    return 0;
                }
                msg_0_isize(pb, sel(b"changeCount\0")).max(0) as u64
            }
        }

        pub fn snapshot(&self, max_bytes: usize) -> (ClipboardSnapshot, bool) {
            let _guard = PASTEBOARD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            unsafe {
                let _pool = AutoreleasePool::push();
                let pb = self.target();
                if pb.is_null() {
                    return (
                        ClipboardSnapshot {
                            change_count: 0,
                            items: Vec::new(),
                        },
                        true,
                    );
                }
                let cc = msg_0_isize(pb, sel(b"changeCount\0")).max(0) as u64;
                let items_arr = msg_0(pb, sel(b"pasteboardItems\0"));
                if items_arr.is_null() {
                    return (
                        ClipboardSnapshot {
                            change_count: cc,
                            items: Vec::new(),
                        },
                        true,
                    );
                }
                let item_count = msg_0_usize(items_arr, sel(b"count\0"));
                let was_empty = item_count == 0;
                let mut total = 0usize;
                let mut items = Vec::new();
                let sel_obj_at = sel(b"objectAtIndex:\0");
                let sel_types = sel(b"types\0");
                let sel_count = sel(b"count\0");
                let sel_utf8 = sel(b"UTF8String\0");
                let sel_data_for_type = sel(b"dataForType:\0");
                let sel_length = sel(b"length\0");
                let sel_bytes = sel(b"bytes\0");

                for i in 0..item_count {
                    let item = msg_1_usize(items_arr, sel_obj_at, i);
                    if item.is_null() {
                        continue;
                    }
                    let types_arr = msg_0(item, sel_types);
                    if types_arr.is_null() {
                        continue;
                    }
                    let type_count = msg_0_usize(types_arr, sel_count);
                    for j in 0..type_count {
                        let ns_type = msg_1_usize(types_arr, sel_obj_at, j);
                        if ns_type.is_null() {
                            continue;
                        }
                        let cstr = msg_0_cstr(ns_type, sel_utf8);
                        if cstr.is_null() {
                            continue;
                        }
                        let uti = CStr::from_ptr(cstr).to_string_lossy();
                        if super::is_sensitive_pasteboard_type(&uti) {
                            return (
                                ClipboardSnapshot {
                                    change_count: cc,
                                    items: Vec::new(),
                                },
                                true,
                            );
                        }
                    }
                }

                for i in 0..item_count {
                    let item = msg_1_usize(items_arr, sel_obj_at, i);
                    if item.is_null() {
                        continue;
                    }
                    let types_arr = msg_0(item, sel_types);
                    if types_arr.is_null() {
                        continue;
                    }
                    let type_count = msg_0_usize(types_arr, sel_count);
                    let mut item_bytes = 0usize;
                    let mut types_and_bytes = Vec::new();
                    let mut exceeded = false;
                    for j in 0..type_count {
                        let ns_type = msg_1_usize(types_arr, sel_obj_at, j);
                        if ns_type.is_null() {
                            continue;
                        }
                        let cstr = msg_0_cstr(ns_type, sel_utf8);
                        if cstr.is_null() {
                            continue;
                        }
                        let uti = CStr::from_ptr(cstr).to_string_lossy().into_owned();
                        let ns_data = msg_1_id(item, sel_data_for_type, ns_type);
                        if ns_data.is_null() {
                            continue;
                        }
                        let len = msg_0_usize(ns_data, sel_length);
                        if total.saturating_add(item_bytes).saturating_add(len) > max_bytes {
                            exceeded = true;
                            break;
                        }
                        let ptr = msg_0_bytes(ns_data, sel_bytes);
                        let bytes = if len == 0 || ptr.is_null() {
                            Vec::new()
                        } else {
                            std::slice::from_raw_parts(ptr, len).to_vec()
                        };
                        item_bytes += len;
                        types_and_bytes.push((uti, bytes));
                    }
                    if exceeded {
                        break;
                    }
                    if !types_and_bytes.is_empty() {
                        total += item_bytes;
                        items.push(ClipboardItemSnapshot { types_and_bytes });
                    }
                }
                (
                    ClipboardSnapshot {
                        change_count: cc,
                        items,
                    },
                    was_empty,
                )
            }
        }

        pub(crate) fn require_objc_object(
            ptr: *mut std::ffi::c_void,
            what: &str,
        ) -> Result<*mut std::ffi::c_void, String> {
            if ptr.is_null() {
                Err(format!("{what} allocation failed"))
            } else {
                Ok(ptr)
            }
        }

        pub fn clear_contents(&self) -> Result<(), String> {
            let _guard = PASTEBOARD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            unsafe {
                let _pool = AutoreleasePool::push();
                let pb = self.target();
                if pb.is_null() {
                    return Err("NSPasteboard unavailable".into());
                }
                let _ = msg_0_isize(pb, sel(b"clearContents\0"));
                Ok(())
            }
        }

        pub fn write_items(&self, items: &[ClipboardItemSnapshot]) -> Result<u64, String> {
            let _guard = PASTEBOARD_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            unsafe {
                let _pool = AutoreleasePool::push();
                let pb = self.target();
                if pb.is_null() {
                    return Err("NSPasteboard unavailable".into());
                }
                let _ = msg_0_isize(pb, sel(b"clearContents\0"));
                if items.is_empty() {
                    return Ok(msg_0_isize(pb, sel(b"changeCount\0")).max(0) as u64);
                }
                let arr = Self::require_objc_object(
                    msg_1_usize(
                        cls(b"NSMutableArray\0"),
                        sel(b"arrayWithCapacity:\0"),
                        items.len(),
                    ),
                    "NSMutableArray",
                )?;
                let sel_alloc = sel(b"alloc\0");
                let sel_new = sel(b"new\0");
                let sel_autorelease = sel(b"autorelease\0");
                let sel_init_utf8 = sel(b"initWithBytes:length:encoding:\0");
                let sel_data_bytes = sel(b"dataWithBytes:length:\0");
                let sel_set_data = sel(b"setData:forType:\0");
                let sel_add_object = sel(b"addObject:\0");
                let cls_pb_item = cls(b"NSPasteboardItem\0");
                let cls_nsstring = cls(b"NSString\0");
                let cls_nsdata = cls(b"NSData\0");

                for snap_item in items {
                    let pb_item =
                        Self::require_objc_object(msg_0(cls_pb_item, sel_new), "NSPasteboardItem")?;
                    let _ = msg_0(pb_item, sel_autorelease);
                    for (uti, bytes) in &snap_item.types_and_bytes {
                        let ns_type_alloc = msg_0(cls_nsstring, sel_alloc);
                        let ns_type = msg_3_ptr_usize_usize(
                            ns_type_alloc,
                            sel_init_utf8,
                            if uti.is_empty() {
                                std::ptr::null()
                            } else {
                                uti.as_ptr()
                            },
                            uti.len(),
                            4, // NSUTF8StringEncoding
                        );
                        if ns_type.is_null() {
                            continue;
                        }
                        let _ = msg_0(ns_type, sel_autorelease);
                        let ns_data = msg_2_ptr_usize(
                            cls_nsdata,
                            sel_data_bytes,
                            if bytes.is_empty() {
                                std::ptr::null()
                            } else {
                                bytes.as_ptr()
                            },
                            bytes.len(),
                        );
                        if ns_data.is_null() {
                            continue;
                        }
                        let _ = msg_2_id_id_bool(pb_item, sel_set_data, ns_data, ns_type);
                    }
                    msg_1_id_void(arr, sel_add_object, pb_item);
                }
                let wrote = msg_1_id_bool(pb, sel(b"writeObjects:\0"), arr);
                if wrote == 0 {
                    return Err("NSPasteboard writeObjects failed".into());
                }
                Ok(msg_0_isize(pb, sel(b"changeCount\0")).max(0) as u64)
            }
        }
    }
}

trait ClipboardBackend: Send + Sync + std::fmt::Debug {
    fn change_count(&self) -> u64;
    fn snapshot(&self, max_bytes: usize) -> (ClipboardSnapshot, bool);
    fn write_plain_text(&self, text: &str) -> Result<u64, String>;
    fn restore_items(&self, snapshot: &ClipboardSnapshot) -> Result<bool, String>;
    fn clear_contents(&self) -> Result<(), String>;
    fn restore_delay(&self) -> Duration;
}

fn pbcopy_plain_text_with_binary(program: &Path, text: &str) -> Result<(), String> {
    let mut child = Command::new(program)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("clipboard write spawn failed: {e}"))?;
    let write_res = if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes())
    } else {
        Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "pbcopy stdin unavailable",
        ))
    };
    let output = child
        .wait_with_output()
        .map_err(|e| format!("clipboard write wait failed: {e}"))?;
    if let Err(err) = write_res {
        return Err(format!("clipboard write stdin failed: {err}"));
    }
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("clipboard write failed with status {}", output.status)
        } else {
            format!("clipboard write failed ({}): {stderr}", output.status)
        });
    }
    Ok(())
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
struct LiveMacOsClipboardBackend {
    pasteboard: macos_pasteboard::PasteboardRef,
}

#[cfg(target_os = "macos")]
impl LiveMacOsClipboardBackend {
    fn general() -> Self {
        Self {
            pasteboard: macos_pasteboard::PasteboardRef::general(),
        }
    }

    #[allow(dead_code)]
    fn unique_for_test() -> Option<Self> {
        macos_pasteboard::PasteboardRef::unique_for_test().map(|pasteboard| Self { pasteboard })
    }
}

#[cfg(target_os = "macos")]
impl ClipboardBackend for LiveMacOsClipboardBackend {
    fn change_count(&self) -> u64 {
        self.pasteboard.change_count()
    }

    fn snapshot(&self, max_bytes: usize) -> (ClipboardSnapshot, bool) {
        self.pasteboard.snapshot(max_bytes)
    }

    fn write_plain_text(&self, text: &str) -> Result<u64, String> {
        let before_cc = self.pasteboard.change_count();
        if self.pasteboard.is_general() {
            pbcopy_plain_text_with_binary(Path::new("/usr/bin/pbcopy"), text)?;
            let after_cc = self.pasteboard.change_count();
            if after_cc == before_cc {
                return Err("clipboard changeCount did not advance after write".into());
            }
            Ok(after_cc)
        } else {
            let item = ClipboardItemSnapshot {
                types_and_bytes: vec![(
                    "public.utf8-plain-text".to_string(),
                    text.as_bytes().to_vec(),
                )],
            };
            self.pasteboard.write_items(&[item])
        }
    }

    fn restore_items(&self, snapshot: &ClipboardSnapshot) -> Result<bool, String> {
        self.pasteboard.write_items(&snapshot.items)?;
        Ok(true)
    }

    fn clear_contents(&self) -> Result<(), String> {
        self.pasteboard.clear_contents()
    }

    fn restore_delay(&self) -> Duration {
        Duration::from_millis(80)
    }
}

#[derive(Debug)]
struct InMemoryClipboardBackend {
    state: Mutex<ClipboardSnapshot>,
}

impl Default for InMemoryClipboardBackend {
    fn default() -> Self {
        Self {
            state: Mutex::new(ClipboardSnapshot {
                change_count: 1,
                items: Vec::new(),
            }),
        }
    }
}

impl ClipboardBackend for InMemoryClipboardBackend {
    fn change_count(&self) -> u64 {
        self.state.lock().unwrap().change_count
    }

    fn snapshot(&self, max_bytes: usize) -> (ClipboardSnapshot, bool) {
        let st = self.state.lock().unwrap();
        if st.items.iter().any(|item| {
            item.types_and_bytes
                .iter()
                .any(|(uti, _)| is_sensitive_pasteboard_type(uti))
        }) {
            return (
                ClipboardSnapshot {
                    change_count: st.change_count,
                    items: Vec::new(),
                },
                true,
            );
        }
        let was_empty = st.items.is_empty();
        let mut total = 0usize;
        let mut items = Vec::new();
        for item in &st.items {
            let item_bytes = item.total_bytes();
            if total.saturating_add(item_bytes) <= max_bytes {
                total += item_bytes;
                items.push(item.clone());
            } else {
                break;
            }
        }
        (
            ClipboardSnapshot {
                change_count: st.change_count,
                items,
            },
            was_empty,
        )
    }

    fn write_plain_text(&self, text: &str) -> Result<u64, String> {
        let mut st = self.state.lock().unwrap();
        st.change_count += 1;
        st.items = vec![ClipboardItemSnapshot {
            types_and_bytes: vec![(
                "public.utf8-plain-text".to_string(),
                text.as_bytes().to_vec(),
            )],
        }];
        Ok(st.change_count)
    }

    fn restore_items(&self, snapshot: &ClipboardSnapshot) -> Result<bool, String> {
        let mut st = self.state.lock().unwrap();
        st.change_count += 1;
        st.items = snapshot.items.clone();
        Ok(true)
    }

    fn clear_contents(&self) -> Result<(), String> {
        let mut st = self.state.lock().unwrap();
        st.change_count += 1;
        st.items.clear();
        Ok(())
    }

    fn restore_delay(&self) -> Duration {
        Duration::ZERO
    }
}

#[derive(Debug, Clone, Copy)]
struct LastMouseClickState {
    button: MouseButtonKind,
    at: Instant,
    x: f64,
    y: f64,
    count: i64,
}

fn next_mouse_click_state(
    prev: Option<LastMouseClickState>,
    button: MouseButtonKind,
    now: Instant,
    x: f64,
    y: f64,
) -> LastMouseClickState {
    const MAX_INTERVAL: Duration = Duration::from_millis(500);
    const MAX_DISTANCE_PT: f64 = 8.0;
    let count = match prev {
        Some(last)
            if last.button == button
                && now.saturating_duration_since(last.at) <= MAX_INTERVAL
                && (x - last.x).hypot(y - last.y) <= MAX_DISTANCE_PT =>
        {
            (last.count % 3) + 1
        }
        _ => 1,
    };
    LastMouseClickState {
        button,
        at: now,
        x,
        y,
        count,
    }
}

pub struct MacOsSystemDriver {
    clipboard_backend: Arc<dyn ClipboardBackend>,
    snapshot_empty_flags: Mutex<HashMap<u64, bool>>,
    managed_controls: ManagedAxControls<NativeAxProvider>,
    isolated: AtomicBool,
    model_picker_mouse_deltas: Arc<Mutex<Vec<(f64, f64)>>>,
    model_picker_mouse_tap: Mutex<Option<ModelPickerMouseTap>>,
    last_mouse_click: Mutex<Option<LastMouseClickState>>,
}

impl std::fmt::Debug for MacOsSystemDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacOsSystemDriver")
            .field("isolated", &self.isolated.load(Ordering::Relaxed))
            .field("managed_controls", &self.managed_controls)
            .finish()
    }
}

impl Default for MacOsSystemDriver {
    fn default() -> Self {
        #[cfg(target_os = "macos")]
        let clipboard_backend: Arc<dyn ClipboardBackend> =
            Arc::new(LiveMacOsClipboardBackend::general());
        #[cfg(not(target_os = "macos"))]
        let clipboard_backend: Arc<dyn ClipboardBackend> =
            Arc::new(InMemoryClipboardBackend::default());
        Self {
            clipboard_backend,
            snapshot_empty_flags: Mutex::new(HashMap::new()),
            managed_controls: ManagedAxControls::default(),
            isolated: AtomicBool::new(false),
            model_picker_mouse_deltas: Arc::new(Mutex::new(Vec::new())),
            model_picker_mouse_tap: Mutex::new(None),
            last_mouse_click: Mutex::new(None),
        }
    }
}

impl Drop for MacOsSystemDriver {
    fn drop(&mut self) {
        if let Ok(mut tap) = self.model_picker_mouse_tap.lock() {
            let _ = tap.take();
        }
    }
}

impl MacOsSystemDriver {
    pub fn new_isolated() -> Self {
        Self {
            clipboard_backend: Arc::new(InMemoryClipboardBackend::default()),
            snapshot_empty_flags: Mutex::new(HashMap::new()),
            managed_controls: ManagedAxControls::default(),
            isolated: AtomicBool::new(true),
            model_picker_mouse_deltas: Arc::new(Mutex::new(Vec::new())),
            model_picker_mouse_tap: Mutex::new(None),
            last_mouse_click: Mutex::new(None),
        }
    }

    #[allow(dead_code)]
    fn with_clipboard_backend(clipboard_backend: Arc<dyn ClipboardBackend>) -> Self {
        Self {
            clipboard_backend,
            snapshot_empty_flags: Mutex::new(HashMap::new()),
            managed_controls: ManagedAxControls::default(),
            isolated: AtomicBool::new(true),
            model_picker_mouse_deltas: Arc::new(Mutex::new(Vec::new())),
            model_picker_mouse_tap: Mutex::new(None),
            last_mouse_click: Mutex::new(None),
        }
    }

    fn is_isolated_environment(&self) -> bool {
        self.isolated.load(Ordering::Acquire)
            || std::env::var_os("MI_VERIFY_ISOLATED").is_some()
            || std::env::var_os("MI_HOST_AUDIT_LOG").is_some()
            || std::env::var_os("MI_DISABLE_UPDATER").is_some()
    }

    pub fn inspect_managed_controls(
        &self,
        adapter: &str,
    ) -> Result<ManagedControlReadiness, String> {
        self.managed_controls.inspect(adapter)
    }

    pub fn inject_model_picker_mouse_delta(&self, dx: f64, dy: f64) {
        if let Ok(mut queue) = self.model_picker_mouse_deltas.lock() {
            queue.push((dx, dy));
        }
    }
}

fn open_url_command_args(url: &str) -> Result<[&str; 2], String> {
    mi_model::host_router::validate_open_url(url)?;
    Ok(["--", url])
}

pub use mi_model::private_fs::redact_home_path;

fn wait_child_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Result<std::process::ExitStatus, String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {
                let now = std::time::Instant::now();
                if now >= deadline {
                    #[cfg(unix)]
                    {
                        let pid = child.id() as libc::pid_t;
                        if pid > 0 {
                            unsafe {
                                libc::kill(-pid, libc::SIGKILL);
                            }
                        }
                    }
                    #[cfg(not(unix))]
                    {
                        let _ = child.kill();
                    }
                    let _ = child.wait();
                    return Err(format!("shell_timeout_{}ms", timeout.as_millis()));
                }
                let remaining = deadline.saturating_duration_since(now);
                std::thread::sleep(Duration::from_millis(5).min(remaining));
            }
            Err(err) => return Err(err.to_string()),
        }
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

    fn open_accessibility_settings(&self) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        unsafe {
            let _ = with_accessibility_prompt_options(true, |options| {
                AXIsProcessTrustedWithOptions(options)
            });
        }
        let status = Command::new("/usr/bin/open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("open accessibility settings exited with {status}"))
        }
    }

    fn open_application(&self, bundle_id: &str) -> Result<(), String> {
        if !mi_model::host_router::is_valid_bundle_id(bundle_id) {
            return Err("invalid_bundle_id".to_string());
        }
        let status = Command::new("/usr/bin/open")
            .args(["-b", bundle_id])
            .stdin(std::process::Stdio::null())
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
        let (snapshot, was_empty) = self.clipboard_backend.snapshot(max_bytes);
        let mut flags = self.snapshot_empty_flags.lock().unwrap();
        if flags.len() > 32 {
            flags.clear();
        }
        flags.insert(snapshot.change_count, was_empty);
        snapshot
    }

    fn write_clipboard_plain_text(&self, text: &str) -> u64 {
        self.write_clipboard_plain_text_checked(text).unwrap_or(0)
    }

    fn write_clipboard_plain_text_checked(&self, text: &str) -> Result<u64, String> {
        self.clipboard_backend.write_plain_text(text)
    }

    fn restore_clipboard(&self, snapshot: &ClipboardSnapshot, expected_change_count: u64) -> bool {
        let delay = self.clipboard_backend.restore_delay();
        if !delay.is_zero() {
            std::thread::sleep(delay);
        }
        let was_empty = self
            .snapshot_empty_flags
            .lock()
            .unwrap()
            .remove(&snapshot.change_count)
            .unwrap_or(snapshot.items.is_empty());
        if self.clipboard_backend.change_count() != expected_change_count {
            return false;
        }
        if snapshot.items.is_empty() {
            if was_empty {
                self.clipboard_backend.clear_contents().is_ok()
            } else {
                false
            }
        } else {
            self.clipboard_backend
                .restore_items(snapshot)
                .unwrap_or(false)
        }
    }

    fn open_url(&self, url: &str) -> Result<(), String> {
        let args = open_url_command_args(url)?;
        let status = Command::new("/usr/bin/open")
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
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
        if is_composer_shortcut_adapter(adapter) {
            return self.managed_controls.shortcut(adapter, key, modifiers);
        }
        if !self.is_accessibility_trusted() {
            return Err("accessibility_untrusted".to_string());
        }
        let bundle = bundle_id_for_adapter(adapter)
            .ok_or_else(|| "blocked:unsupported_agent_adapter".to_string())?;
        #[cfg(not(target_os = "macos"))]
        self.open_application(bundle)?;
        #[cfg(target_os = "macos")]
        unsafe {
            let pid = if bundle == "com.openai.codex" && find_pid_for_bundle_id(bundle).is_some() {
                crate::native_ax::activate_focused_codex_window()?
            } else {
                self.open_application(bundle)?;
                find_pid_for_bundle_id(bundle)
                    .ok_or_else(|| "blocked:agent_process_not_found".to_string())?
            };
            let down = CGEventCreateKeyboardEvent(std::ptr::null_mut(), key, true);
            if down.is_null() {
                return Err("cg_keyboard_event_null".to_string());
            }
            let up = CGEventCreateKeyboardEvent(std::ptr::null_mut(), key, false);
            if up.is_null() {
                CFRelease(down);
                return Err("cg_keyboard_event_null".to_string());
            }
            let flags = modifiers_to_cg_flags(modifiers);
            CGEventSetFlags(down, flags);
            CGEventSetFlags(up, flags);
            CGEventPostToPid(pid, down);
            CGEventPostToPid(pid, up);
            CFRelease(down);
            CFRelease(up);
        }
        Ok(())
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

    fn move_cursor_by(&self, dx: f64, dy: f64) -> Result<(), String> {
        if !self.is_accessibility_trusted() {
            return Err("accessibility_untrusted".to_string());
        }
        if !dx.is_finite() || !dy.is_finite() {
            return Err("invalid_cursor_delta".to_string());
        }
        #[cfg(target_os = "macos")]
        unsafe {
            let source = CGEventSourceCreate(CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE);
            if source.is_null() {
                return Err("cg_event_source_null".to_string());
            }
            let probe = CGEventCreate(std::ptr::null_mut());
            if probe.is_null() {
                CFRelease(source);
                return Err("cg_cursor_probe_null".to_string());
            }
            let current = CGEventGetLocation(probe);
            CFRelease(probe);
            let raw_next = CGPoint {
                x: current.x + dx,
                y: current.y + dy,
            };
            let mut displays = [0u32; 32];
            let mut display_count: u32 = 0;
            let mut rects = Vec::new();
            if CGGetActiveDisplayList(
                displays.len() as u32,
                displays.as_mut_ptr(),
                &mut display_count,
            ) == 0
                && display_count > 0
            {
                let count = (display_count as usize).min(displays.len());
                for &display in &displays[..count] {
                    let bounds = CGDisplayBounds(display);
                    rects.push((
                        bounds.origin.x,
                        bounds.origin.y,
                        bounds.size.width,
                        bounds.size.height,
                    ));
                }
            }
            let (clamped_x, clamped_y) = clamp_point_to_rects((raw_next.x, raw_next.y), &rects);
            let next = CGPoint {
                x: clamped_x,
                y: clamped_y,
            };
            let event =
                CGEventCreateMouseEvent(source, CG_EVENT_MOUSE_MOVED, next, CG_MOUSE_BUTTON_LEFT);
            if event.is_null() {
                CFRelease(source);
                return Err("cg_mouse_move_event_null".to_string());
            }
            CGEventPost(CG_HID_EVENT_TAP, event);
            CFRelease(event);
            CFRelease(source);
        }
        Ok(())
    }

    fn click_mouse_button(&self, button: MouseButtonKind) -> Result<(), String> {
        if !self.is_accessibility_trusted() {
            return Err("accessibility_untrusted".to_string());
        }
        #[cfg(target_os = "macos")]
        unsafe {
            let source = CGEventSourceCreate(CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE);
            if source.is_null() {
                return Err("cg_event_source_null".to_string());
            }
            let probe = CGEventCreate(std::ptr::null_mut());
            if probe.is_null() {
                CFRelease(source);
                return Err("cg_cursor_probe_null".to_string());
            }
            let loc = CGEventGetLocation(probe);
            CFRelease(probe);
            let (down_type, up_type, cg_button) = match button {
                MouseButtonKind::Left => (
                    CG_EVENT_LEFT_MOUSE_DOWN,
                    CG_EVENT_LEFT_MOUSE_UP,
                    CG_MOUSE_BUTTON_LEFT,
                ),
                MouseButtonKind::Right => (
                    CG_EVENT_RIGHT_MOUSE_DOWN,
                    CG_EVENT_RIGHT_MOUSE_UP,
                    CG_MOUSE_BUTTON_RIGHT,
                ),
                MouseButtonKind::Middle => (
                    CG_EVENT_OTHER_MOUSE_DOWN,
                    CG_EVENT_OTHER_MOUSE_UP,
                    CG_MOUSE_BUTTON_CENTER,
                ),
            };
            let down = CGEventCreateMouseEvent(source, down_type, loc, cg_button);
            let up = CGEventCreateMouseEvent(source, up_type, loc, cg_button);
            if down.is_null() || up.is_null() {
                if !down.is_null() {
                    CFRelease(down);
                }
                if !up.is_null() {
                    CFRelease(up);
                }
                CFRelease(source);
                return Err("cg_mouse_click_event_null".to_string());
            }
            let click_state = if let Ok(mut guard) = self.last_mouse_click.lock() {
                let next = next_mouse_click_state(*guard, button, Instant::now(), loc.x, loc.y);
                *guard = Some(next);
                next.count
            } else {
                1
            };
            CGEventSetIntegerValueField(down, K_CG_MOUSE_EVENT_CLICK_STATE, click_state);
            CGEventSetIntegerValueField(up, K_CG_MOUSE_EVENT_CLICK_STATE, click_state);
            CGEventPost(CG_HID_EVENT_TAP, down);
            CGEventPost(CG_HID_EVENT_TAP, up);
            CFRelease(down);
            CFRelease(up);
            CFRelease(source);
        }
        Ok(())
    }

    fn scroll_cursor_wheel(&self, dx_lines: i32, dy_lines: i32) -> Result<(), String> {
        if !self.is_accessibility_trusted() {
            return Err("accessibility_untrusted".to_string());
        }
        if dx_lines == 0 && dy_lines == 0 {
            return Ok(());
        }
        #[cfg(target_os = "macos")]
        unsafe {
            let source = CGEventSourceCreate(CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE);
            if source.is_null() {
                return Err("cg_event_source_null".to_string());
            }
            // Per NSEvent.h, horizontal deltaX uses + for left and - for right.
            let event = CGEventCreateScrollWheelEvent2(
                source,
                CG_SCROLL_EVENT_UNIT_LINE,
                2,
                dy_lines,
                -dx_lines,
                0,
            );
            if event.is_null() {
                CFRelease(source);
                return Err("cg_mouse_scroll_event_null".to_string());
            }
            CGEventPost(CG_HID_EVENT_TAP, event);
            CFRelease(event);
            CFRelease(source);
        }
        Ok(())
    }

    fn new_conversation_in_project(&self, adapter: &str) -> Result<(), String> {
        if !self.is_accessibility_trusted() {
            return Err("accessibility_untrusted".to_string());
        }
        let bundle_id = bundle_id_for_adapter(adapter)
            .ok_or_else(|| "blocked:unsupported_agent_adapter".to_string())?;
        self.open_application(bundle_id)?;
        #[cfg(target_os = "macos")]
        if adapter.trim().eq_ignore_ascii_case("antigravity") {
            unsafe {
                let pid = find_pid_for_bundle_id(bundle_id)
                    .ok_or_else(|| "blocked:agent_process_not_found".to_string())?;
                let app_elem = AXUIElementCreateApplication(pid);
                if !app_elem.is_null() {
                    let attr_manual = cf_string_from_str("AXManualAccessibility");
                    if !attr_manual.is_null() {
                        let _ = AXUIElementSetAttributeValue(app_elem, attr_manual, kCFBooleanTrue);
                        CFRelease(attr_manual);
                    }
                    let win_elem = ax_copy_element_attr(app_elem, "AXFocusedWindow")
                        .or_else(|| ax_copy_element_attr(app_elem, "AXMainWindow"))
                        .or_else(|| {
                            ax_copy_raw_attr(app_elem, "AXWindows").and_then(|val| {
                                cf_array_copy_ax_elements(val, 1).into_iter().next()
                            })
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
        if !is_composer_shortcut_adapter(adapter) {
            return Err("blocked:unsupported_project_navigation".into());
        }
        self.managed_controls.shortcut(adapter, KEY_CODE_N, &cmd)
    }

    fn toggle_live_voice(&self, adapter: &str) -> Result<(), String> {
        if !self.is_accessibility_trusted() {
            return Err("accessibility_untrusted".to_string());
        }
        self.managed_controls.toggle_live_voice(adapter)
    }

    fn codex_reasoning(&self, action: CodexReasoningAction) -> Result<(), String> {
        self.managed_controls.codex_reasoning(action)
    }

    fn open_model_picker(&self, adapter: &str) -> Result<(), String> {
        if !self.is_accessibility_trusted() {
            return Err("accessibility_untrusted".to_string());
        }
        if is_managed_adapter(adapter) {
            return self.managed_controls.open_model_picker(adapter);
        }
        let Some((key_code, modifiers)) =
            native_shortcut_for_agent_command(adapter, "model-picker-open")
                .or_else(|| native_shortcut_for_agent_command(adapter, "model"))
        else {
            return Err("blocked:unsupported_agent_command".into());
        };
        self.agent_shortcut(adapter, key_code, &modifiers)
    }

    fn open_side_question(&self, adapter: &str) -> Result<(), String> {
        if adapter != "antigravity" {
            return Err("blocked:unsupported_side_question_adapter".into());
        }
        if !self.is_accessibility_trusted() {
            return Err("accessibility_untrusted".to_string());
        }
        self.managed_controls.open_side_question(adapter)
    }

    fn open_side_panel(&self, adapter: &str) -> Result<(), String> {
        if adapter != "antigravity" {
            return Err("blocked:unsupported_side_panel_adapter".into());
        }
        if !self.is_accessibility_trusted() {
            return Err("accessibility_untrusted".to_string());
        }
        self.managed_controls.set_side_panel_open(adapter, true)
    }

    fn close_side_panel(&self, adapter: &str) -> Result<(), String> {
        if adapter != "antigravity" {
            return Err("blocked:unsupported_side_panel_adapter".into());
        }
        if !self.is_accessibility_trusted() {
            return Err("accessibility_untrusted".to_string());
        }
        self.managed_controls.set_side_panel_open(adapter, false)
    }

    fn send_model_picker_key(&self, adapter: &str, key_code: u16) -> Result<(), String> {
        if !self.is_accessibility_trusted() {
            return Err("accessibility_untrusted".to_string());
        }
        if is_managed_adapter(adapter) {
            return self
                .managed_controls
                .send_model_picker_key(adapter, key_code);
        }
        self.agent_shortcut(adapter, key_code, &[])
    }

    fn step_model_picker(&self, adapter: &str, key_code: u16) -> Result<(), String> {
        if !self.is_accessibility_trusted() {
            return Err("accessibility_untrusted".to_string());
        }
        if is_managed_adapter(adapter) {
            return self.managed_controls.step_model_picker(adapter, key_code);
        }
        self.agent_shortcut(adapter, key_code, &[])
    }

    fn confirm_model_picker(&self, adapter: &str) -> Result<ModelPickerConfirmResult, String> {
        if !self.is_accessibility_trusted() {
            return Err("accessibility_untrusted".to_string());
        }
        if is_managed_adapter(adapter) {
            return self.managed_controls.confirm_model_picker(adapter);
        }
        self.agent_shortcut(adapter, KEY_CODE_RETURN, &[])?;
        Ok(ModelPickerConfirmResult::Confirmed)
    }

    fn open_subagent_picker(&self, adapter: &str) -> Result<SubagentOpenResult, String> {
        if !self.is_accessibility_trusted() {
            return Err("accessibility_untrusted".to_string());
        }
        if is_managed_adapter(adapter) {
            return self.managed_controls.open_subagent_picker(adapter);
        }
        Err("blocked:subagent_picker_unavailable".into())
    }

    fn step_subagent_picker(&self, adapter: &str, key_code: u16) -> Result<(), String> {
        if !self.is_accessibility_trusted() {
            return Err("accessibility_untrusted".to_string());
        }
        if is_managed_adapter(adapter) {
            return self
                .managed_controls
                .step_subagent_picker(adapter, key_code);
        }
        self.agent_shortcut(adapter, key_code, &[])
    }

    fn confirm_subagent_picker(&self, adapter: &str) -> Result<ModelPickerConfirmResult, String> {
        if !self.is_accessibility_trusted() {
            return Err("accessibility_untrusted".to_string());
        }
        if is_managed_adapter(adapter) {
            return self.managed_controls.confirm_subagent_picker(adapter);
        }
        self.agent_shortcut(adapter, KEY_CODE_RETURN, &[])?;
        Ok(ModelPickerConfirmResult::Confirmed)
    }

    fn resolve_default_managed_adapter(&self, profile_id: &str) -> &'static str {
        if let Some(inferred) = infer_managed_adapter_from_profile_id(profile_id) {
            return inferred;
        }
        if self.is_isolated_environment() {
            return "antigravity";
        }
        for (bundle, adapter) in [
            ("com.google.antigravity", "antigravity"),
            ("com.openai.codex", "codex"),
            ("com.deepseek.dsh", "dsh-desktop"),
        ] {
            if is_bundle_running(bundle) {
                return adapter;
            }
        }
        "antigravity"
    }

    fn set_isolated_mode(&self, isolated: bool) {
        self.isolated.store(isolated, Ordering::SeqCst);
        if isolated {
            self.set_model_picker_mouse_capture(false);
        }
    }

    fn set_model_picker_mouse_capture(&self, enabled: bool) {
        let Ok(mut slot) = self.model_picker_mouse_tap.lock() else {
            return;
        };
        if !enabled {
            let _ = slot.take();
            if let Ok(mut queue) = self.model_picker_mouse_deltas.lock() {
                queue.clear();
            }
            return;
        }
        if self.is_isolated_environment() || !self.is_accessibility_trusted() {
            let _ = slot.take();
            return;
        }
        #[cfg(target_os = "macos")]
        if slot.is_none() {
            *slot = ModelPickerMouseTap::start(Arc::clone(&self.model_picker_mouse_deltas));
        }
    }

    fn drain_model_picker_mouse_deltas(&self) -> Vec<(f64, f64)> {
        self.model_picker_mouse_deltas
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default()
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
    MoveCursorBy {
        dx: i32,
        dy: i32,
    },
    ClickMouseButton(MouseButtonKind),
    ScrollCursorWheel {
        dx_lines: i32,
        dy_lines: i32,
    },
    NewConversationInProject(String),
    ToggleLiveVoice(String),
    CodexReasoning(CodexReasoningAction),
    OpenModelPicker(String),
    OpenSubagentPicker(String),
    OpenSideQuestion(String),
    OpenSidePanel(String),
    CloseSidePanel(String),
    SendModelPickerKey {
        adapter: String,
        key_code: u16,
    },
}

#[derive(Debug)]
struct RecordingState {
    dictation_active: bool,
    clipboard: ClipboardSnapshot,
    clipboard_write_error: Option<String>,
    events: Vec<RecordedOsEvent>,
    model_picker_mouse_capture: bool,
    model_picker_mouse_deltas: Vec<(f64, f64)>,
}

#[derive(Debug, Clone)]
pub struct RecordingOsDriver {
    accessibility_trusted: Arc<AtomicBool>,
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
            accessibility_trusted: Arc::new(AtomicBool::new(trusted)),
            state: Arc::new(Mutex::new(RecordingState {
                dictation_active: false,
                clipboard: ClipboardSnapshot {
                    change_count: 1,
                    items: Vec::new(),
                },
                clipboard_write_error: None,
                events: Vec::new(),
                model_picker_mouse_capture: false,
                model_picker_mouse_deltas: Vec::new(),
            })),
        }
    }

    pub fn set_clipboard_write_error(&self, err: Option<String>) {
        self.state.lock().unwrap().clipboard_write_error = err;
    }

    pub fn push_model_picker_mouse_delta(&self, dx: f64, dy: f64) {
        let mut st = self.state.lock().unwrap();
        if st.model_picker_mouse_capture {
            st.model_picker_mouse_deltas.push((dx, dy));
        }
    }

    pub fn is_model_picker_mouse_capture_enabled(&self) -> bool {
        self.state.lock().unwrap().model_picker_mouse_capture
    }

    pub fn set_accessibility_trusted(&self, trusted: bool) {
        self.accessibility_trusted.store(trusted, Ordering::SeqCst);
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
        self.accessibility_trusted.load(Ordering::SeqCst)
    }

    fn open_accessibility_settings(&self) -> Result<(), String> {
        self.state
            .lock()
            .unwrap()
            .events
            .push(RecordedOsEvent::OpenApplication(
                "com.apple.systempreferences.accessibility".to_string(),
            ));
        Ok(())
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
        if !self.accessibility_trusted.load(Ordering::SeqCst) {
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
        if !self.accessibility_trusted.load(Ordering::SeqCst) {
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
        self.write_clipboard_plain_text_checked(text).unwrap_or(0)
    }

    fn write_clipboard_plain_text_checked(&self, text: &str) -> Result<u64, String> {
        let mut st = self.state.lock().unwrap();
        if let Some(err) = st.clipboard_write_error.clone() {
            return Err(err);
        }
        st.events
            .push(RecordedOsEvent::WriteClipboardPlainText(text.to_string()));
        st.clipboard.change_count += 1;
        st.clipboard.items = vec![ClipboardItemSnapshot {
            types_and_bytes: vec![(
                "public.utf8-plain-text".to_string(),
                text.as_bytes().to_vec(),
            )],
        }];
        Ok(st.clipboard.change_count)
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

    fn move_cursor_by(&self, dx: f64, dy: f64) -> Result<(), String> {
        let mut st = self.state.lock().unwrap();
        if !self.accessibility_trusted.load(Ordering::SeqCst) {
            return Err("accessibility_untrusted".to_string());
        }
        st.events.push(RecordedOsEvent::MoveCursorBy {
            dx: dx.round() as i32,
            dy: dy.round() as i32,
        });
        Ok(())
    }

    fn click_mouse_button(&self, button: MouseButtonKind) -> Result<(), String> {
        let mut st = self.state.lock().unwrap();
        if !self.accessibility_trusted.load(Ordering::SeqCst) {
            return Err("accessibility_untrusted".to_string());
        }
        st.events.push(RecordedOsEvent::ClickMouseButton(button));
        Ok(())
    }

    fn scroll_cursor_wheel(&self, dx_lines: i32, dy_lines: i32) -> Result<(), String> {
        let mut st = self.state.lock().unwrap();
        if !self.accessibility_trusted.load(Ordering::SeqCst) {
            return Err("accessibility_untrusted".to_string());
        }
        st.events
            .push(RecordedOsEvent::ScrollCursorWheel { dx_lines, dy_lines });
        Ok(())
    }

    fn new_conversation_in_project(&self, adapter: &str) -> Result<(), String> {
        let mut st = self.state.lock().unwrap();
        if !self.accessibility_trusted.load(Ordering::SeqCst) {
            return Err("accessibility_untrusted".to_string());
        }
        st.events.push(RecordedOsEvent::NewConversationInProject(
            adapter.to_string(),
        ));
        Ok(())
    }

    fn toggle_live_voice(&self, adapter: &str) -> Result<(), String> {
        let mut st = self.state.lock().unwrap();
        if !self.accessibility_trusted.load(Ordering::SeqCst) {
            return Err("accessibility_untrusted".to_string());
        }
        st.events
            .push(RecordedOsEvent::ToggleLiveVoice(adapter.to_string()));
        Ok(())
    }

    fn codex_reasoning(&self, action: CodexReasoningAction) -> Result<(), String> {
        let mut st = self.state.lock().unwrap();
        if !self.accessibility_trusted.load(Ordering::SeqCst) {
            return Err("accessibility_untrusted".to_string());
        }
        st.events.push(RecordedOsEvent::CodexReasoning(action));
        Ok(())
    }

    fn open_model_picker(&self, adapter: &str) -> Result<(), String> {
        let mut st = self.state.lock().unwrap();
        if !self.accessibility_trusted.load(Ordering::SeqCst) {
            return Err("accessibility_untrusted".to_string());
        }
        st.events
            .push(RecordedOsEvent::OpenModelPicker(adapter.to_string()));
        Ok(())
    }

    fn open_subagent_picker(&self, adapter: &str) -> Result<SubagentOpenResult, String> {
        let mut st = self.state.lock().unwrap();
        if !self.accessibility_trusted.load(Ordering::SeqCst) {
            return Err("accessibility_untrusted".to_string());
        }
        st.events
            .push(RecordedOsEvent::OpenSubagentPicker(adapter.to_string()));
        Ok(SubagentOpenResult::PopupOpened)
    }

    fn open_side_question(&self, adapter: &str) -> Result<(), String> {
        let mut st = self.state.lock().unwrap();
        if !self.accessibility_trusted.load(Ordering::SeqCst) {
            return Err("accessibility_untrusted".to_string());
        }
        st.events
            .push(RecordedOsEvent::OpenSideQuestion(adapter.to_string()));
        Ok(())
    }
    fn open_side_panel(&self, adapter: &str) -> Result<(), String> {
        let mut st = self.state.lock().unwrap();
        if !self.accessibility_trusted.load(Ordering::SeqCst) {
            return Err("accessibility_untrusted".to_string());
        }
        st.events
            .push(RecordedOsEvent::OpenSidePanel(adapter.to_string()));
        Ok(())
    }
    fn close_side_panel(&self, adapter: &str) -> Result<(), String> {
        let mut st = self.state.lock().unwrap();
        if !self.accessibility_trusted.load(Ordering::SeqCst) {
            return Err("accessibility_untrusted".to_string());
        }
        st.events
            .push(RecordedOsEvent::CloseSidePanel(adapter.to_string()));
        Ok(())
    }
    fn send_model_picker_key(&self, adapter: &str, key_code: u16) -> Result<(), String> {
        let mut st = self.state.lock().unwrap();
        if !self.accessibility_trusted.load(Ordering::SeqCst) {
            return Err("accessibility_untrusted".to_string());
        }
        st.events.push(RecordedOsEvent::SendModelPickerKey {
            adapter: adapter.to_string(),
            key_code,
        });
        Ok(())
    }

    fn step_model_picker(&self, adapter: &str, key_code: u16) -> Result<(), String> {
        self.send_model_picker_key(adapter, key_code)
    }

    fn confirm_model_picker(&self, adapter: &str) -> Result<ModelPickerConfirmResult, String> {
        self.send_model_picker_key(adapter, KEY_CODE_RETURN)?;
        Ok(ModelPickerConfirmResult::Confirmed)
    }

    fn step_subagent_picker(&self, adapter: &str, key_code: u16) -> Result<(), String> {
        self.send_model_picker_key(adapter, key_code)
    }

    fn confirm_subagent_picker(&self, adapter: &str) -> Result<ModelPickerConfirmResult, String> {
        self.send_model_picker_key(adapter, KEY_CODE_RETURN)?;
        Ok(ModelPickerConfirmResult::Confirmed)
    }

    fn set_model_picker_mouse_capture(&self, enabled: bool) {
        let mut st = self.state.lock().unwrap();
        st.model_picker_mouse_capture = enabled;
        if !enabled {
            st.model_picker_mouse_deltas.clear();
        }
    }

    fn drain_model_picker_mouse_deltas(&self) -> Vec<(f64, f64)> {
        let mut st = self.state.lock().unwrap();
        std::mem::take(&mut st.model_picker_mouse_deltas)
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
            target: redact_home_path(working_directory),
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HeldInputId {
    pub session_id: u64,
    pub binding_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum FnHoldKey {
    Input(HeldInputId),
    Pulse,
}

struct FnOwner {
    profile_id: String,
    target: String,
    release_failed: bool,
}

pub type MacHostExecutor = LiveHostActionExecutor<MacOsSystemDriver>;

pub struct LiveHostActionExecutor<D: HostOsDriver = MacOsSystemDriver> {
    driver: D,
    execution_lock: Mutex<()>,
    held_apple_fn_bindings: Mutex<HashMap<FnHoldKey, FnOwner>>,
    mouse_speed_tier: AtomicU8,
    shell_command_timeout: Duration,
}

impl<D: HostOsDriver> Drop for LiveHostActionExecutor<D> {
    fn drop(&mut self) {
        if let Err(error) = self.stop_owned_actions() {
            eprintln!("host action cleanup unconfirmed: {error}");
        }
    }
}

impl<D: HostOsDriver> LiveHostActionExecutor<D> {
    pub fn new(driver: D) -> Self {
        Self {
            driver,
            execution_lock: Mutex::new(()),
            held_apple_fn_bindings: Mutex::new(HashMap::new()),
            mouse_speed_tier: AtomicU8::new(1),
            shell_command_timeout: DEFAULT_SHELL_COMMAND_TIMEOUT,
        }
    }

    pub fn with_shell_command_timeout(mut self, timeout: Duration) -> Self {
        self.shell_command_timeout = timeout;
        self
    }

    fn mouse_step_and_scroll(tier: u8) -> (f64, i32) {
        match tier {
            0 => (8.0, 2),
            2 => (52.0, 9),
            _ => (22.0, 4),
        }
    }

    fn execute_host_mouse_command(
        &self,
        command: &str,
        make_receipt: &impl Fn(&str, String) -> HostActionReceipt,
    ) -> HostActionReceipt {
        match command {
            "mouse-speed:0" => {
                self.mouse_speed_tier.store(0, Ordering::SeqCst);
                return make_receipt("executed", "mouse-speed:0".into());
            }
            "mouse-speed:1" => {
                self.mouse_speed_tier.store(1, Ordering::SeqCst);
                return make_receipt("executed", "mouse-speed:1".into());
            }
            "mouse-speed:2" => {
                self.mouse_speed_tier.store(2, Ordering::SeqCst);
                return make_receipt("executed", "mouse-speed:2".into());
            }
            "mouse-speed:cycle" => {
                let next = self
                    .mouse_speed_tier
                    .try_update(Ordering::SeqCst, Ordering::SeqCst, |cur| {
                        Some((cur + 1) % 3)
                    })
                    .map(|prev| (prev + 1) % 3)
                    .unwrap_or(1);
                return make_receipt("executed", format!("mouse-speed:{next}"));
            }
            _ => {}
        }
        if !self.driver.is_accessibility_trusted() {
            return make_receipt("blocked", "accessibility_untrusted".to_string());
        }
        let (move_px, scroll_lines) =
            Self::mouse_step_and_scroll(self.mouse_speed_tier.load(Ordering::SeqCst));
        let result = match command {
            "mouse-move:up" => self.driver.move_cursor_by(0.0, -move_px),
            "mouse-move:down" => self.driver.move_cursor_by(0.0, move_px),
            "mouse-move:left" => self.driver.move_cursor_by(-move_px, 0.0),
            "mouse-move:right" => self.driver.move_cursor_by(move_px, 0.0),
            "mouse-click:left" => self.driver.click_mouse_button(MouseButtonKind::Left),
            "mouse-click:right" => self.driver.click_mouse_button(MouseButtonKind::Right),
            "mouse-click:middle" => self.driver.click_mouse_button(MouseButtonKind::Middle),
            "mouse-scroll:up" => self.driver.scroll_cursor_wheel(0, scroll_lines),
            "mouse-scroll:down" => self.driver.scroll_cursor_wheel(0, -scroll_lines),
            "mouse-scroll:left" => self.driver.scroll_cursor_wheel(-scroll_lines, 0),
            "mouse-scroll:right" => self.driver.scroll_cursor_wheel(scroll_lines, 0),
            _ => Err("unsupported_mouse_command".to_string()),
        };
        match result {
            Ok(()) => make_receipt("executed", format!("host/{command}")),
            Err(err) => make_receipt("failed", err),
        }
    }

    pub fn is_accessibility_trusted(&self) -> bool {
        self.driver.is_accessibility_trusted()
    }

    pub fn open_accessibility_settings(&self) -> Result<(), String> {
        self.driver.open_accessibility_settings()
    }

    pub fn has_unconfirmed_apple_fn_release(&self) -> bool {
        self.held_apple_fn_bindings
            .lock()
            .unwrap()
            .values()
            .any(|owner| owner.release_failed)
    }

    pub fn set_isolated_mode(&self, isolated: bool) {
        self.driver.set_isolated_mode(isolated);
    }

    pub fn set_model_picker_mouse_capture(&self, enabled: bool) {
        self.driver.set_model_picker_mouse_capture(enabled);
    }

    pub fn drain_model_picker_mouse_deltas(&self) -> Vec<(f64, f64)> {
        self.driver.drain_model_picker_mouse_deltas()
    }
}

impl LiveHostActionExecutor<MacOsSystemDriver> {
    pub fn inject_model_picker_mouse_delta(&self, dx: f64, dy: f64) {
        self.driver.inject_model_picker_mouse_delta(dx, dy);
    }
}

impl<D: HostOsDriver> LiveHostActionExecutor<D> {
    pub fn stop_owned_actions(&self) -> Result<(), String> {
        let _execution = self.execution_lock.lock().unwrap();
        self.driver.set_model_picker_mouse_capture(false);
        let mut held = self.held_apple_fn_bindings.lock().unwrap();
        let released = if held.is_empty() {
            Ok(())
        } else {
            match self.driver.post_apple_fn_flags(false) {
                Ok(()) => {
                    held.clear();
                    Ok(())
                }
                Err(error) => {
                    for owner in held.values_mut() {
                        owner.release_failed = true;
                    }
                    Err(error)
                }
            }
        };
        let stopped = self.driver.stop_owned_dictations();
        released.and(stopped)
    }

    pub fn start_owned_dictation(&self, adapter: &str) -> Result<(), String> {
        let _execution = self.execution_lock.lock().unwrap();
        if self.has_unconfirmed_apple_fn_release() {
            return Err("apple_fn_cleanup_unconfirmed".into());
        }
        self.driver.start_owned_dictation(adapter)
    }

    pub fn stop_owned_dictations(&self) -> Result<(), String> {
        let _execution = self.execution_lock.lock().unwrap();
        self.driver.stop_owned_dictations()
    }

    pub fn handle_apple_fn_press(
        &self,
        input: &HeldInputId,
        profile_id: &str,
        target: &str,
    ) -> Option<HostActionReceipt> {
        let _execution = self.execution_lock.lock().unwrap();
        let mut held = self.held_apple_fn_bindings.lock().unwrap();
        let key = FnHoldKey::Input(input.clone());
        if held.contains_key(&key) {
            return None;
        }
        let binding_id = &input.binding_id;
        let resolved_target = if target.trim().is_empty() {
            "frontmost-editor"
        } else {
            target
        };
        let blocked = if held.values().any(|owner| owner.release_failed) {
            Some("apple_fn_cleanup_unconfirmed")
        } else if !self.driver.is_accessibility_trusted() {
            Some("accessibility_untrusted")
        } else {
            None
        };
        if let Some(reason) = blocked {
            return Some(HostActionReceipt {
                action_type: "shortcut".to_string(),
                binding_id: binding_id.to_string(),
                detail: reason.to_string(),
                outcome: "blocked".to_string(),
                profile_id: profile_id.to_string(),
                target: resolved_target.to_string(),
                text_byte_len: None,
                timestamp_ms: now_timestamp_ms(),
            });
        }
        let pressed = if held.is_empty() {
            self.driver.post_apple_fn_flags(true)
        } else {
            Ok(())
        };
        if let Err(err) = pressed {
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
        held.insert(
            key,
            FnOwner {
                profile_id: profile_id.to_string(),
                target: resolved_target.to_string(),
                release_failed: false,
            },
        );
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

    pub fn handle_apple_fn_release(&self, input: &HeldInputId) -> Option<HostActionReceipt> {
        let _execution = self.execution_lock.lock().unwrap();
        let mut held = self.held_apple_fn_bindings.lock().unwrap();
        let key = FnHoldKey::Input(input.clone());
        let owner = held.get(&key)?;
        let profile_id = owner.profile_id.clone();
        let target = owner.target.clone();
        let released = if held.len() == 1 {
            self.driver.post_apple_fn_flags(false)
        } else {
            Ok(())
        };
        let (outcome, detail) = match released {
            Ok(()) => {
                held.remove(&key);
                ("executed".to_string(), "fn (release)".to_string())
            }
            Err(err) => {
                held.get_mut(&key).unwrap().release_failed = true;
                ("failed".to_string(), err)
            }
        };
        Some(HostActionReceipt {
            action_type: "shortcut".to_string(),
            binding_id: input.binding_id.clone(),
            detail,
            outcome,
            profile_id,
            target,
            text_byte_len: None,
            timestamp_ms: now_timestamp_ms(),
        })
    }

    fn pulse_apple_fn(&self, profile_id: &str, target: &str) -> Result<(), String> {
        let mut held = self.held_apple_fn_bindings.lock().unwrap();
        if !held.is_empty() {
            return Err("apple_fn_already_held".into());
        }
        self.driver.post_apple_fn_flags(true)?;
        held.insert(
            FnHoldKey::Pulse,
            FnOwner {
                profile_id: profile_id.to_string(),
                target: target.to_string(),
                release_failed: true,
            },
        );
        self.driver.post_apple_fn_flags(false)?;
        held.remove(&FnHoldKey::Pulse);
        Ok(())
    }

    pub fn execute(
        &self,
        profile_id: &str,
        binding_id: &str,
        action: &ResolvedHostAction,
    ) -> HostActionReceipt {
        self.execute_when(profile_id, binding_id, action, || true)
    }

    pub fn with_serialized_host_effect<T>(
        &self,
        admitted: impl FnOnce() -> bool,
        effect: impl FnOnce(&D) -> Result<T, String>,
    ) -> Result<T, String> {
        let _execution = self.execution_lock.lock().unwrap();
        if !admitted() {
            return Err("blocked:session_navigation_retired".to_string());
        }
        if self.has_unconfirmed_apple_fn_release() {
            return Err("blocked:unconfirmed_apple_fn_release".to_string());
        }
        effect(&self.driver)
    }

    pub fn execute_when(
        &self,
        profile_id: &str,
        binding_id: &str,
        action: &ResolvedHostAction,
        admitted: impl FnOnce() -> bool,
    ) -> HostActionReceipt {
        let execution = self.execution_lock.lock().unwrap();
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

        if !admitted() {
            return make_receipt("blocked", "input_retired".to_string());
        }
        if self.has_unconfirmed_apple_fn_release() {
            return make_receipt("blocked", "apple_fn_cleanup_unconfirmed".to_string());
        }

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
                    .filter(|adapter| is_composer_shortcut_adapter(adapter))
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
                    self.pulse_apple_fn(profile_id, &desc.target)
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
                if adapter
                    .as_deref()
                    .is_some_and(|adapter| adapter.eq_ignore_ascii_case("cindy"))
                {
                    return make_receipt("blocked", "unsupported_agent_adapter".into());
                }
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
                    if let Err(err) = self.driver.open_application(bundle_id) {
                        return make_receipt("failed", err);
                    }
                }
                let snapshot = self.driver.snapshot_clipboard(MAX_CLIPBOARD_STASH_BYTES);
                let written_cc = match self.driver.write_clipboard_plain_text_checked(sanitized) {
                    Ok(cc) => cc,
                    Err(err) => return make_receipt("failed", err),
                };
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
            ResolvedHostAction::OpenUrl { url } => {
                if let Err(err) = mi_model::host_router::validate_open_url(url) {
                    return make_receipt("failed", err);
                }
                match self.driver.open_url(url) {
                    Ok(()) => make_receipt("executed", desc.detail),
                    Err(err) => make_receipt("failed", err),
                }
            }
            ResolvedHostAction::ShellCommand {
                command,
                working_directory,
            } => {
                if !command.trim().is_empty()
                    && !working_directory.trim().is_empty()
                    && Path::new(working_directory).is_dir()
                {
                    let mut cmd = Command::new("/bin/sh");
                    cmd.args(["-c", command])
                        .current_dir(working_directory)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null());
                    #[cfg(unix)]
                    {
                        use std::os::unix::process::CommandExt;
                        cmd.process_group(0);
                    }
                    let spawn_res = cmd.spawn();
                    drop(execution);
                    match spawn_res {
                        Ok(mut child) => {
                            match wait_child_with_timeout(&mut child, self.shell_command_timeout) {
                                Ok(s) if s.success() => make_receipt("executed", desc.detail),
                                Ok(s) => make_receipt("failed", format!("shell_exit_{s}")),
                                Err(err) => make_receipt("failed", err),
                            }
                        }
                        Err(err) => make_receipt("failed", err.to_string()),
                    }
                } else {
                    make_receipt(
                        "failed",
                        if command.trim().is_empty() {
                            "empty_shell_command".into()
                        } else {
                            "invalid_working_directory".into()
                        },
                    )
                }
            }
            ResolvedHostAction::AgentCommand { adapter, command } => {
                if adapter == "host" && mi_model::is_host_mouse_command(command) {
                    return self.execute_host_mouse_command(command, &make_receipt);
                }
                if adapter == "four-app" && command.starts_with("slot-open:") {
                    return make_receipt("blocked", "session_navigation_unavailable".into());
                }
                let effective_adapter = if matches!(adapter.as_str(), "host" | "active-agent")
                    && (matches!(
                        command.as_str(),
                        "model"
                            | "effort"
                            | "model-picker-open"
                            | "search"
                            | "subagent"
                            | "subagent-open"
                    ) || model_picker_navigation_key_code(command).is_some())
                {
                    self.driver.resolve_default_managed_adapter(profile_id)
                } else {
                    adapter.as_str()
                };
                if bundle_id_for_adapter(effective_adapter).is_none() {
                    return make_receipt("blocked", "unsupported_agent_adapter".into());
                }
                if let Some(key_code) = model_picker_navigation_key_code(command) {
                    if !self.driver.is_accessibility_trusted() {
                        return make_receipt("blocked", "accessibility_untrusted".to_string());
                    }
                    return match command.trim() {
                        "model-picker-prev" | "model-picker-next" => {
                            match self.driver.step_model_picker(effective_adapter, key_code) {
                                Ok(()) => make_receipt("executed", desc.detail),
                                Err(error) => make_receipt(managed_error_outcome(&error), error),
                            }
                        }
                        "model-picker-confirm" => {
                            match self.driver.confirm_model_picker(effective_adapter) {
                                Ok(ModelPickerConfirmResult::Confirmed) => {
                                    make_receipt("executed", desc.detail)
                                }
                                Ok(ModelPickerConfirmResult::PopupAlreadyClosed) => make_receipt(
                                    "executed",
                                    format!("{effective_adapter}/model-picker-already-closed"),
                                ),
                                Err(error) => make_receipt(managed_error_outcome(&error), error),
                            }
                        }
                        "subagent-prev" | "subagent-next" => {
                            match self
                                .driver
                                .step_subagent_picker(effective_adapter, key_code)
                            {
                                Ok(()) => make_receipt("executed", desc.detail),
                                Err(error) => make_receipt(managed_error_outcome(&error), error),
                            }
                        }
                        "subagent-confirm" => {
                            match self.driver.confirm_subagent_picker(effective_adapter) {
                                Ok(ModelPickerConfirmResult::Confirmed) => {
                                    make_receipt("executed", desc.detail)
                                }
                                Ok(ModelPickerConfirmResult::PopupAlreadyClosed) => make_receipt(
                                    "executed",
                                    format!("{effective_adapter}/subagent-picker-already-closed"),
                                ),
                                Err(error) => make_receipt(managed_error_outcome(&error), error),
                            }
                        }
                        _ => match self
                            .driver
                            .send_model_picker_key(effective_adapter, key_code)
                        {
                            Ok(()) => make_receipt("executed", desc.detail),
                            Err(error) => make_receipt(managed_error_outcome(&error), error),
                        },
                    };
                }
                if matches!(command.as_str(), "model" | "model-picker-open")
                    && is_managed_adapter(effective_adapter)
                {
                    if !self.driver.is_accessibility_trusted() {
                        return make_receipt("blocked", "accessibility_untrusted".to_string());
                    }
                    return match self.driver.open_model_picker(effective_adapter) {
                        Ok(()) => make_receipt("executed", desc.detail),
                        Err(error) => make_receipt(managed_error_outcome(&error), error),
                    };
                }
                if matches!(command.as_str(), "subagent" | "subagent-open") {
                    if !self.driver.is_accessibility_trusted() {
                        return make_receipt("blocked", "accessibility_untrusted".to_string());
                    }
                    if !is_managed_adapter(effective_adapter) {
                        return make_receipt("blocked", "subagent_picker_unavailable".into());
                    }
                    return match self.driver.open_subagent_picker(effective_adapter) {
                        Ok(SubagentOpenResult::PopupOpened) => {
                            make_receipt("executed", desc.detail)
                        }
                        Ok(SubagentOpenResult::OpenedDirectly) => make_receipt(
                            "executed",
                            format!("{effective_adapter}/subagent-opened-direct"),
                        ),
                        Err(error) => make_receipt(managed_error_outcome(&error), error),
                    };
                }
                if command == "toggle-dictation" {
                    if !self.driver.is_accessibility_trusted() {
                        return make_receipt("blocked", "accessibility_untrusted".to_string());
                    }
                    if !is_managed_adapter(adapter) {
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
                } else if command == "side-question" && adapter == "antigravity" {
                    if !self.driver.is_accessibility_trusted() {
                        return make_receipt("blocked", "accessibility_untrusted".to_string());
                    }
                    match self.driver.open_side_question(adapter) {
                        Ok(()) => make_receipt("executed", desc.detail),
                        Err(error) => make_receipt(managed_error_outcome(&error), error),
                    }
                } else if command == "open-side-panel" && effective_adapter == "antigravity" {
                    if !self.driver.is_accessibility_trusted() {
                        return make_receipt("blocked", "accessibility_untrusted".to_string());
                    }
                    match self.driver.open_side_panel(effective_adapter) {
                        Ok(()) => make_receipt("executed", desc.detail),
                        Err(error) => make_receipt(managed_error_outcome(&error), error),
                    }
                } else if command == "close-side-panel" && effective_adapter == "antigravity" {
                    if !self.driver.is_accessibility_trusted() {
                        return make_receipt("blocked", "accessibility_untrusted".to_string());
                    }
                    match self.driver.close_side_panel(effective_adapter) {
                        Ok(()) => make_receipt("executed", desc.detail),
                        Err(error) => make_receipt(managed_error_outcome(&error), error),
                    }
                } else if command == "live-voice" && adapter != "codex" {
                    if !self.driver.is_accessibility_trusted() {
                        return make_receipt("blocked", "accessibility_untrusted".to_string());
                    }
                    match self.driver.toggle_live_voice(adapter) {
                        Ok(()) => make_receipt("executed", desc.detail),
                        Err(err) => make_receipt("failed", err),
                    }
                } else if adapter == "codex"
                    && matches!(
                        command.as_str(),
                        "effort-down" | "effort-up" | "toggle-fast-mode"
                    )
                {
                    if !self.driver.is_accessibility_trusted() {
                        return make_receipt("blocked", "accessibility_untrusted".to_string());
                    }
                    let reasoning_action = match command.as_str() {
                        "effort-down" => CodexReasoningAction::DecreaseEffort,
                        "effort-up" => CodexReasoningAction::IncreaseEffort,
                        "toggle-fast-mode" => CodexReasoningAction::ToggleFastMode,
                        _ => unreachable!(),
                    };
                    match self.driver.codex_reasoning(reasoning_action) {
                        Ok(()) => make_receipt("executed", desc.detail),
                        Err(error) => make_receipt(managed_error_outcome(&error), error),
                    }
                } else if let Some((key_code, modifiers)) =
                    native_shortcut_for_agent_command(effective_adapter, command)
                {
                    if !self.driver.is_accessibility_trusted() {
                        return make_receipt("blocked", "accessibility_untrusted".to_string());
                    }
                    match self
                        .driver
                        .agent_shortcut(effective_adapter, key_code, &modifiers)
                    {
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
                    match self.pulse_apple_fn(profile_id, &desc.target) {
                        Ok(()) => make_receipt("executed", desc.detail),
                        Err(err) => make_receipt("failed", err),
                    }
                } else {
                    make_receipt("blocked", "unsupported_agent_command".into())
                }
            }
            ResolvedHostAction::FocusSession { .. } => {
                make_receipt("blocked", "session_navigation_unavailable".into())
            }
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
        let written = driver.write_clipboard_plain_text_checked(prompt)?;
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
    let written = driver.write_clipboard_plain_text_checked(prompt)?;
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

fn is_composer_shortcut_adapter(adapter: &str) -> bool {
    matches!(
        crate::ax_dictation::DictationAdapter::parse(adapter),
        Some(
            crate::ax_dictation::DictationAdapter::Antigravity
                | crate::ax_dictation::DictationAdapter::Dsh
        )
    )
}

fn is_managed_bundle(bundle: &str) -> bool {
    managed_adapter_for_bundle(bundle).is_some()
}

fn managed_adapter_for_bundle(bundle: &str) -> Option<&'static str> {
    match bundle {
        "com.google.antigravity" => Some("antigravity"),
        "com.deepseek.dsh" => Some("dsh-desktop"),
        "com.openai.codex" => Some("codex"),
        _ => None,
    }
}

#[cfg(test)]
mod serialized_effect_tests {
    use super::*;
    use std::fs;

    #[test]
    fn codex_managed_controls_do_not_redirect_official_shortcuts_through_the_composer() {
        assert!(is_managed_adapter("codex"));
        assert!(!is_composer_shortcut_adapter("codex"));
        assert!(is_composer_shortcut_adapter("antigravity"));
        assert!(is_composer_shortcut_adapter("dsh-desktop"));
        let driver = RecordingOsDriver::new(true);
        let executor = LiveHostActionExecutor::new(driver.clone());
        let receipt = executor.execute(
            "test",
            "button",
            &ResolvedHostAction::Shortcut {
                key: "terminal".into(),
                key_code: 0x11,
                modifiers: vec!["command".into()],
                target_bundle_id: Some("com.openai.codex".into()),
            },
        );
        assert_eq!(receipt.outcome, "executed");
        assert!(driver.events().iter().any(|event| matches!(
            event,
            RecordedOsEvent::PostKeyShortcut {
                key_code: 0x11,
                key_down: true,
                ..
            }
        )));
        assert!(!driver
            .events()
            .iter()
            .any(|event| matches!(event, RecordedOsEvent::FocusAgentInput(_))));
    }

    #[test]
    fn with_serialized_host_effect_enforces_lock_admission_and_unconfirmed_apple_fn_gate() {
        let driver = RecordingOsDriver::new(true);
        let executor = Arc::new(LiveHostActionExecutor::new(driver.clone()));

        assert_eq!(
            executor.with_serialized_host_effect(|| false, |_| Ok(())),
            Err("blocked:session_navigation_retired".to_string())
        );

        let held_input = HeldInputId {
            session_id: 1,
            binding_id: "AG00".into(),
        };
        let press_receipt = executor
            .handle_apple_fn_press(&held_input, "prof", "frontmost-editor")
            .unwrap();
        assert_eq!(press_receipt.outcome, "executed");
        driver.set_accessibility_trusted(false);
        let release_receipt = executor.handle_apple_fn_release(&held_input).unwrap();
        assert_eq!(release_receipt.outcome, "failed");
        assert!(executor.has_unconfirmed_apple_fn_release());

        assert_eq!(
            executor
                .with_serialized_host_effect(|| true, |d| d.open_application("com.example.app")),
            Err("blocked:unconfirmed_apple_fn_release".to_string())
        );

        driver.set_accessibility_trusted(true);
        executor.stop_owned_actions().unwrap();
        assert!(!executor.has_unconfirmed_apple_fn_release());

        let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
        let exec_bg = Arc::clone(&executor);
        let bg = std::thread::spawn(move || {
            exec_bg
                .with_serialized_host_effect(
                    || true,
                    |d| {
                        entered_tx.send(()).unwrap();
                        release_rx
                            .recv_timeout(std::time::Duration::from_secs(2))
                            .unwrap();
                        d.open_application("com.google.antigravity")
                    },
                )
                .unwrap();
        });
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();

        let exec_fg = Arc::clone(&executor);
        let fg = std::thread::spawn(move || {
            exec_fg.execute(
                "prof",
                "AG01",
                &ResolvedHostAction::OpenApp {
                    bundle_id: "com.openai.codex".into(),
                },
            )
        });
        std::thread::sleep(std::time::Duration::from_millis(30));
        release_tx.send(()).unwrap();
        bg.join().unwrap();
        let fg_receipt = fg.join().unwrap();
        assert_eq!(fg_receipt.outcome, "executed");

        let app_opens: Vec<String> = driver
            .events()
            .into_iter()
            .filter_map(|ev| match ev {
                RecordedOsEvent::OpenApplication(bundle) => Some(bundle),
                _ => None,
            })
            .collect();
        assert_eq!(
            app_opens,
            vec![
                "com.google.antigravity".to_string(),
                "com.openai.codex".to_string()
            ]
        );
    }

    #[test]
    fn host_mouse_commands_move_click_scroll_and_respect_speed_gears_and_trust() {
        let driver = RecordingOsDriver::new(true);
        let executor = LiveHostActionExecutor::new(driver.clone());
        let run = |command: &str| {
            executor.execute(
                "prof",
                "btn",
                &ResolvedHostAction::AgentCommand {
                    adapter: "host".into(),
                    command: command.into(),
                },
            )
        };

        // Default speed tier 1 -> 22px move, 4 scroll lines.
        assert_eq!(run("mouse-move:up").outcome, "executed");
        assert_eq!(run("mouse-click:left").outcome, "executed");
        assert_eq!(run("mouse-click:right").outcome, "executed");
        assert_eq!(run("mouse-click:middle").outcome, "executed");
        assert_eq!(run("mouse-scroll:down").outcome, "executed");
        assert_eq!(run("mouse-scroll:left").outcome, "executed");

        // Switch to fine tier 0 (8px / 2 lines), then fast tier 2 (52px / 9 lines), then cycle -> 0.
        assert_eq!(run("mouse-speed:0").detail, "mouse-speed:0");
        assert_eq!(run("mouse-move:right").outcome, "executed");
        assert_eq!(run("mouse-speed:2").detail, "mouse-speed:2");
        assert_eq!(run("mouse-scroll:up").outcome, "executed");
        assert_eq!(run("mouse-speed:cycle").detail, "mouse-speed:0");
        assert_eq!(run("mouse-move:left").outcome, "executed");

        let mouse_events: Vec<_> = driver
            .events()
            .into_iter()
            .filter(|ev| !matches!(ev, RecordedOsEvent::CheckAccessibilityTrusted))
            .collect();
        assert_eq!(
            mouse_events,
            vec![
                RecordedOsEvent::MoveCursorBy { dx: 0, dy: -22 },
                RecordedOsEvent::ClickMouseButton(MouseButtonKind::Left),
                RecordedOsEvent::ClickMouseButton(MouseButtonKind::Right),
                RecordedOsEvent::ClickMouseButton(MouseButtonKind::Middle),
                RecordedOsEvent::ScrollCursorWheel {
                    dx_lines: 0,
                    dy_lines: -4
                },
                RecordedOsEvent::ScrollCursorWheel {
                    dx_lines: -4,
                    dy_lines: 0
                },
                RecordedOsEvent::MoveCursorBy { dx: 8, dy: 0 },
                RecordedOsEvent::ScrollCursorWheel {
                    dx_lines: 0,
                    dy_lines: 9
                },
                RecordedOsEvent::MoveCursorBy { dx: -8, dy: 0 },
            ]
        );

        driver.set_accessibility_trusted(false);
        let blocked = run("mouse-move:down");
        assert_eq!(blocked.outcome, "blocked");
        assert_eq!(blocked.detail, "accessibility_untrusted");
    }

    #[test]
    fn clamp_point_to_rects_handles_negative_multimonitor_and_offscreen_bounds() {
        let displays = [(-1920.0, 0.0, 1920.0, 1080.0), (0.0, 0.0, 2560.0, 1440.0)];

        // Points inside primary or negative-x secondary monitor stay unchanged.
        assert_eq!(
            clamp_point_to_rects((500.0, 400.0), &displays),
            (500.0, 400.0)
        );
        assert_eq!(
            clamp_point_to_rects((-100.0, 300.0), &displays),
            (-100.0, 300.0)
        );

        // Offscreen beyond secondary left edge clamps to [-1920, -1] x [0, 1079].
        assert_eq!(
            clamp_point_to_rects((-2400.0, 300.0), &displays),
            (-1920.0, 300.0)
        );
        assert_eq!(
            clamp_point_to_rects((-500.0, -40.0), &displays),
            (-500.0, 0.0)
        );
        assert_eq!(
            clamp_point_to_rects((-500.0, 1200.0), &displays),
            (-500.0, 1079.0)
        );

        // Offscreen beyond primary right/bottom clamps to [0, 2559] x [0, 1439].
        assert_eq!(
            clamp_point_to_rects((3000.0, 1600.0), &displays),
            (2559.0, 1439.0)
        );

        // Empty display list falls back to the input point unchanged.
        assert_eq!(clamp_point_to_rects((-30.0, 40.0), &[]), (-30.0, 40.0));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn accessibility_prompt_options_builds_single_entry_cf_dictionary() {
        #[link(name = "CoreFoundation", kind = "framework")]
        unsafe extern "C" {
            fn CFDictionaryGetCount(the_dict: CFDictionaryRef) -> CFIndex;
            fn CFDictionaryGetValue(the_dict: CFDictionaryRef, key: CFTypeRef) -> CFTypeRef;
        }
        unsafe {
            let result = with_accessibility_prompt_options(false, |dict| {
                let count = CFDictionaryGetCount(dict);
                let key = cf_string_from_str("AXTrustedCheckOptionPrompt");
                let value = CFDictionaryGetValue(dict, key as CFTypeRef);
                CFRelease(key);
                (count, value == kCFBooleanFalse)
            });
            assert_eq!(result, Some((1, true)));
        }
    }

    #[test]
    fn open_url_command_args_adds_double_dash_and_validates_scheme() {
        assert_eq!(
            open_url_command_args("https://example.com/a?b=1").unwrap(),
            ["--", "https://example.com/a?b=1"]
        );
        assert!(open_url_command_args("-open").is_err());
        assert!(open_url_command_args("file:///etc/passwd").is_err());
        assert!(open_url_command_args("javascript:alert(1)").is_err());
    }

    #[test]
    fn shell_command_enforces_timeout_kills_child_and_nulls_stdin() {
        let driver = RecordingOsDriver::new(true);
        let executor = LiveHostActionExecutor::new(driver)
            .with_shell_command_timeout(Duration::from_millis(80));
        let t0 = std::time::Instant::now();
        let receipt = executor.execute(
            "Antigravity",
            "AG00",
            &ResolvedHostAction::ShellCommand {
                command: "sleep 5".into(),
                working_directory: "/tmp".into(),
            },
        );
        let elapsed = t0.elapsed();
        assert_eq!(receipt.outcome, "failed");
        assert!(
            receipt.detail.contains("shell_timeout_80ms"),
            "unexpected timeout detail: {}",
            receipt.detail
        );
        assert!(
            elapsed < Duration::from_millis(600),
            "timeout wait took too long: {elapsed:?}"
        );

        let stdin_receipt = executor.execute(
            "Antigravity",
            "AG00",
            &ResolvedHostAction::ShellCommand {
                command: "test ! -t 0 && [ \"$(head -c 1 | wc -c | tr -d ' ')\" = 0 ]".into(),
                working_directory: "/tmp".into(),
            },
        );
        assert_eq!(stdin_receipt.outcome, "executed");
    }

    #[test]
    fn shell_command_timeout_kills_process_group_orphans() {
        struct PidCleanup(Option<libc::pid_t>);
        impl Drop for PidCleanup {
            fn drop(&mut self) {
                if let Some(pid) = self.0 {
                    unsafe {
                        libc::kill(pid, libc::SIGKILL);
                    }
                }
            }
        }

        let temp = tempfile::tempdir().unwrap();
        let pid_file = temp.path().join("grandchild.pid");
        let driver = RecordingOsDriver::new(true);
        let executor = LiveHostActionExecutor::new(driver)
            .with_shell_command_timeout(Duration::from_millis(250));
        let receipt = executor.execute(
            "Antigravity",
            "AG00",
            &ResolvedHostAction::ShellCommand {
                command: format!("sleep 10 & echo $! > {}; wait", pid_file.display()),
                working_directory: "/tmp".into(),
            },
        );
        assert_eq!(receipt.outcome, "failed");
        assert!(receipt.detail.contains("shell_timeout_250ms"));

        let grandchild_pid: libc::pid_t = fs::read_to_string(&pid_file)
            .expect("grandchild pid written")
            .trim()
            .parse()
            .expect("valid pid");
        let mut guard = PidCleanup(Some(grandchild_pid));

        let deadline = std::time::Instant::now() + Duration::from_millis(250);
        let mut exited = false;
        while std::time::Instant::now() < deadline {
            let rc = unsafe { libc::kill(grandchild_pid, 0) };
            if rc != 0 && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
                exited = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if exited {
            guard.0 = None;
        }
        assert!(
            exited,
            "timed-out ShellCommand must kill entire process group including grandchild PID {grandchild_pid}"
        );
    }

    #[test]
    fn shell_command_empty_command_or_invalid_working_dir_reports_failed() {
        let driver = RecordingOsDriver::new(true);
        let executor = LiveHostActionExecutor::new(driver);

        let empty_cmd = executor.execute(
            "Antigravity",
            "AG00",
            &ResolvedHostAction::ShellCommand {
                command: "   \t\n ".into(),
                working_directory: "/tmp".into(),
            },
        );
        assert_eq!(empty_cmd.outcome, "failed");
        assert_eq!(empty_cmd.detail, "empty_shell_command");

        let bad_dir = executor.execute(
            "Antigravity",
            "AG00",
            &ResolvedHostAction::ShellCommand {
                command: "echo ok".into(),
                working_directory: "/nonexistent/mi-shell-working-dir-404".into(),
            },
        );
        assert_eq!(bad_dir.outcome, "failed");
        assert_eq!(bad_dir.detail, "invalid_working_directory");
    }

    #[cfg(unix)]
    #[test]
    fn audit_log_append_refuses_symlinks_and_fifo_without_blocking() {
        let dir = tempfile::tempdir().unwrap();
        let victim = dir.path().join("victim.txt");
        fs::write(&victim, b"SAFE").unwrap();
        let link_log = dir.path().join("audit.jsonl");
        std::os::unix::fs::symlink(&victim, &link_log).unwrap();

        let receipt = HostActionReceipt {
            action_type: "open_app".into(),
            binding_id: "AG00".into(),
            detail: "ok".into(),
            outcome: "executed".into(),
            profile_id: "p1".into(),
            target: "t".into(),
            text_byte_len: None,
            timestamp_ms: 1,
        };
        assert!(
            receipt.append_to_audit_log(&link_log).is_err(),
            "append_to_audit_log must refuse symlink target"
        );
        assert_eq!(fs::read(&victim).unwrap(), b"SAFE");

        let fifo = dir.path().join("audit.fifo");
        let c_fifo = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c_fifo.as_ptr(), 0o600) }, 0);
        let (tx, rx) = std::sync::mpsc::channel();
        let fifo_clone = fifo.clone();
        let receipt_clone = receipt.clone();
        let handle = std::thread::spawn(move || {
            let _ = tx.send(receipt_clone.append_to_audit_log(&fifo_clone));
        });
        let recv_res = rx.recv_timeout(Duration::from_millis(200));
        if recv_res.is_err() {
            let drain_fd =
                unsafe { libc::open(c_fifo.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK) };
            let _ = handle.join();
            if drain_fd >= 0 {
                unsafe { libc::close(drain_fd) };
            }
            panic!("append_to_audit_log blocked on FIFO");
        }
        let _ = handle.join();
        assert!(recv_res.unwrap().is_err());
    }

    #[test]
    fn pbcopy_helper_propagates_non_zero_exit_and_spawn_errors() {
        let missing = Path::new("/nonexistent/mi-pbcopy-missing");
        assert!(pbcopy_plain_text_with_binary(missing, "hello").is_err());

        let false_bin = Path::new("/usr/bin/false");
        let err = pbcopy_plain_text_with_binary(false_bin, "hello").unwrap_err();
        assert!(err.contains("clipboard write failed"), "err={err}");
    }

    #[test]
    fn paste_text_aborts_before_paste_when_clipboard_write_fails() {
        let driver = RecordingOsDriver::new(true);
        driver.set_clipboard_write_error(Some("pbcopy exited 1".into()));
        let executor = LiveHostActionExecutor::new(driver.clone());
        let receipt = executor.execute(
            "Notes",
            "AG00",
            &ResolvedHostAction::PasteText {
                text: "secret-snippet".into(),
                target_bundle_id: None,
                adapter: None,
                no_newline: false,
                submit: false,
            },
        );
        assert_eq!(receipt.outcome, "failed");
        assert_eq!(receipt.detail, "pbcopy exited 1");
        assert!(
            !driver
                .events()
                .iter()
                .any(|e| matches!(e, RecordedOsEvent::PostKeyShortcut { .. })),
            "must never send Cmd+V after clipboard write failure"
        );
    }

    #[derive(Debug)]
    struct MockClipboardState {
        change_count: u64,
        items: Vec<ClipboardItemSnapshot>,
        clear_calls: usize,
        restore_calls: usize,
        fail_next_write: Option<String>,
    }

    #[derive(Debug)]
    struct MockClipboardBackend {
        state: Mutex<MockClipboardState>,
    }

    impl MockClipboardBackend {
        fn new(items: Vec<ClipboardItemSnapshot>, change_count: u64) -> Self {
            Self {
                state: Mutex::new(MockClipboardState {
                    change_count,
                    items,
                    clear_calls: 0,
                    restore_calls: 0,
                    fail_next_write: None,
                }),
            }
        }
    }

    impl ClipboardBackend for MockClipboardBackend {
        fn change_count(&self) -> u64 {
            self.state.lock().unwrap().change_count
        }

        fn snapshot(&self, max_bytes: usize) -> (ClipboardSnapshot, bool) {
            let st = self.state.lock().unwrap();
            let was_empty = st.items.is_empty();
            let mut total = 0usize;
            let mut items = Vec::new();
            for item in &st.items {
                if total + item.total_bytes() <= max_bytes {
                    total += item.total_bytes();
                    items.push(item.clone());
                } else {
                    break;
                }
            }
            (
                ClipboardSnapshot {
                    change_count: st.change_count,
                    items,
                },
                was_empty,
            )
        }

        fn write_plain_text(&self, text: &str) -> Result<u64, String> {
            let mut st = self.state.lock().unwrap();
            if let Some(err) = st.fail_next_write.take() {
                return Err(err);
            }
            st.change_count += 1;
            st.items = vec![ClipboardItemSnapshot {
                types_and_bytes: vec![("public.utf8-plain-text".into(), text.as_bytes().to_vec())],
            }];
            Ok(st.change_count)
        }

        fn restore_items(&self, snapshot: &ClipboardSnapshot) -> Result<bool, String> {
            let mut st = self.state.lock().unwrap();
            st.restore_calls += 1;
            st.change_count += 1;
            st.items = snapshot.items.clone();
            Ok(true)
        }

        fn clear_contents(&self) -> Result<(), String> {
            let mut st = self.state.lock().unwrap();
            st.clear_calls += 1;
            st.change_count += 1;
            st.items.clear();
            Ok(())
        }

        fn restore_delay(&self) -> Duration {
            Duration::ZERO
        }
    }

    #[test]
    fn macos_driver_preserves_newer_clipboard_and_clears_when_originally_empty() {
        let backend = Arc::new(MockClipboardBackend::new(Vec::new(), 5));
        let driver = MacOsSystemDriver::with_clipboard_backend(Arc::clone(&backend) as _);

        let snap = driver.snapshot_clipboard(MAX_CLIPBOARD_STASH_BYTES);
        let written_cc = driver.write_clipboard_plain_text_checked("temp").unwrap();
        assert_eq!(written_cc, 6);
        assert!(driver.restore_clipboard(&snap, written_cc));
        assert_eq!(backend.state.lock().unwrap().clear_calls, 1);
        assert!(backend.state.lock().unwrap().items.is_empty());

        let snap2 = driver.snapshot_clipboard(MAX_CLIPBOARD_STASH_BYTES);
        let written_cc2 = driver.write_clipboard_plain_text_checked("temp2").unwrap();
        {
            let mut st = backend.state.lock().unwrap();
            st.change_count += 1;
            st.items = vec![ClipboardItemSnapshot {
                types_and_bytes: vec![("public.utf8-plain-text".into(), b"user-newer".to_vec())],
            }];
        }
        assert!(!driver.restore_clipboard(&snap2, written_cc2));
        assert_eq!(
            backend.state.lock().unwrap().items[0].types_and_bytes[0].1,
            b"user-newer"
        );
        assert_eq!(backend.state.lock().unwrap().restore_calls, 0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn live_appkit_pasteboard_snapshots_and_restores_multiple_items_and_types() {
        let Some(backend) = LiveMacOsClipboardBackend::unique_for_test() else {
            panic!("pasteboardWithUniqueName should succeed on macOS");
        };
        let backend = Arc::new(backend);
        let original = vec![
            ClipboardItemSnapshot {
                types_and_bytes: vec![
                    ("public.utf8-plain-text".into(), b"alpha".to_vec()),
                    ("public.html".into(), b"<b>alpha</b>".to_vec()),
                ],
            },
            ClipboardItemSnapshot {
                types_and_bytes: vec![("public.png".into(), vec![0x89, 0x50, 0x4E, 0x47])],
            },
        ];
        backend.pasteboard.write_items(&original).unwrap();
        let driver = MacOsSystemDriver::with_clipboard_backend(Arc::clone(&backend) as _);

        let snap = driver.snapshot_clipboard(MAX_CLIPBOARD_STASH_BYTES);
        assert_eq!(snap.items.len(), 2);
        let cc_after_write = driver
            .write_clipboard_plain_text_checked("ephemeral")
            .unwrap();
        assert!(cc_after_write > snap.change_count);
        assert!(driver.restore_clipboard(&snap, cc_after_write));

        let (restored, was_empty) = backend.snapshot(MAX_CLIPBOARD_STASH_BYTES);
        assert!(!was_empty);
        assert_eq!(restored.items.len(), 2);
        assert!(restored.items[0]
            .types_and_bytes
            .iter()
            .any(|(t, b)| t == "public.html" && b == b"<b>alpha</b>"));
        assert!(restored.items[1]
            .types_and_bytes
            .iter()
            .any(|(t, b)| t == "public.png" && b == &[0x89, 0x50, 0x4E, 0x47]));

        backend.clear_contents().unwrap();
        let (empty_snap, is_empty) = backend.snapshot(MAX_CLIPBOARD_STASH_BYTES);
        assert!(is_empty);
        assert!(empty_snap.items.is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn live_appkit_pasteboard_skips_concealed_and_transient_types_and_clears_injected_text() {
        let Some(backend) = LiveMacOsClipboardBackend::unique_for_test() else {
            panic!("pasteboardWithUniqueName should succeed on macOS");
        };
        let backend = Arc::new(backend);
        backend
            .pasteboard
            .write_items(&[ClipboardItemSnapshot {
                types_and_bytes: vec![
                    ("public.utf8-plain-text".into(), b"s3cr3t-pass".to_vec()),
                    ("org.nspasteboard.ConcealedType".into(), b"1".to_vec()),
                ],
            }])
            .unwrap();
        let driver = MacOsSystemDriver::with_clipboard_backend(Arc::clone(&backend) as _);

        let snapshot = driver.snapshot_clipboard(65536);
        assert!(snapshot.items.is_empty());
        let new_cc = driver
            .write_clipboard_plain_text_checked("injected")
            .unwrap();
        assert!(driver.restore_clipboard(&snapshot, new_cc));

        let (after, was_empty) = backend.snapshot(65536);
        assert!(was_empty);
        assert!(after.items.is_empty());

        backend
            .pasteboard
            .write_items(&[
                ClipboardItemSnapshot {
                    types_and_bytes: vec![(
                        "public.utf8-plain-text".into(),
                        b"first-item".to_vec(),
                    )],
                },
                ClipboardItemSnapshot {
                    types_and_bytes: vec![("ORG.NSPASTEBOARD.TRANSIENTTYPE".into(), b"1".to_vec())],
                },
            ])
            .unwrap();
        let (transient_snap, transient_empty) = backend.snapshot(65536);
        assert!(transient_empty);
        assert!(transient_snap.items.is_empty());

        let snap2 = driver.snapshot_clipboard(65536);
        let written_cc2 = driver
            .write_clipboard_plain_text_checked("injected-2")
            .unwrap();
        backend
            .pasteboard
            .write_items(&[ClipboardItemSnapshot {
                types_and_bytes: vec![("public.utf8-plain-text".into(), b"user-newer".to_vec())],
            }])
            .unwrap();
        assert!(!driver.restore_clipboard(&snap2, written_cc2));
        let (newer, newer_empty) = backend.snapshot(65536);
        assert!(!newer_empty);
        assert_eq!(newer.items[0].types_and_bytes[0].1, b"user-newer");
    }

    #[test]
    fn in_memory_clipboard_skips_transient_and_concealed_types_case_insensitively() {
        let backend = Arc::new(InMemoryClipboardBackend::default());
        {
            let mut st = backend.state.lock().unwrap();
            st.change_count = 10;
            st.items = vec![ClipboardItemSnapshot {
                types_and_bytes: vec![
                    ("public.utf8-plain-text".into(), b"secret".to_vec()),
                    ("ORG.NSPASTEBOARD.TRANSIENTTYPE".into(), b"1".to_vec()),
                ],
            }];
        }
        let (snap, was_empty) = backend.snapshot(65536);
        assert!(was_empty);
        assert!(snap.items.is_empty());
        assert_eq!(snap.change_count, 10);

        let driver = MacOsSystemDriver::with_clipboard_backend(Arc::clone(&backend) as _);
        let driver_snap = driver.snapshot_clipboard(65536);
        assert!(driver_snap.items.is_empty());
        let written_cc = driver
            .write_clipboard_plain_text_checked("injected")
            .unwrap();
        {
            let mut st = backend.state.lock().unwrap();
            st.change_count += 1;
            st.items = vec![ClipboardItemSnapshot {
                types_and_bytes: vec![("public.utf8-plain-text".into(), b"newer-user".to_vec())],
            }];
        }
        assert!(!driver.restore_clipboard(&driver_snap, written_cc));
        assert_eq!(
            backend.state.lock().unwrap().items[0].types_and_bytes[0].1,
            b"newer-user"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cf_string_to_rust_rejects_mismatched_cf_type() {
        #[link(name = "CoreFoundation", kind = "framework")]
        unsafe extern "C" {
            fn CFArrayCreate(
                allocator: CFTypeRef,
                values: *const CFTypeRef,
                num_values: CFIndex,
                call_backs: CFTypeRef,
            ) -> CFArrayRef;
        }
        if std::env::var_os("MI_MACOS_CF_PROBE").is_some() {
            unsafe {
                let empty = CFArrayCreate(std::ptr::null(), std::ptr::null(), 0, std::ptr::null());
                assert!(!empty.is_null());
                let decoded = cf_string_to_rust(empty);
                CFRelease(empty);
                assert!(decoded.is_none());
            }
            return;
        }
        let exe = std::env::current_exe().unwrap();
        let output = std::process::Command::new(exe)
            .args([
                "--exact",
                "executor::serialized_effect_tests::cf_string_to_rust_rejects_mismatched_cf_type",
            ])
            .env("MI_MACOS_CF_PROBE", "1")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && stdout.contains("1 passed"),
            "cf_string_to_rust on mismatched CFArray crashed or returned Some (status={:?}, stdout={stdout})",
            output.status
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cf_array_copy_ax_elements_releases_mismatched_ref_and_skips_non_ax_items() {
        #[link(name = "CoreFoundation", kind = "framework")]
        unsafe extern "C" {
            fn CFArrayCreate(
                allocator: CFTypeRef,
                values: *const CFTypeRef,
                num_values: CFIndex,
                call_backs: CFTypeRef,
            ) -> CFArrayRef;
            fn CFGetRetainCount(cf: CFTypeRef) -> CFIndex;
        }
        unsafe {
            let cf_str = cf_string_from_str("not-an-array");
            assert!(!cf_str.is_null());
            CFRetain(cf_str);
            let before = CFGetRetainCount(cf_str);
            let elems = cf_array_copy_ax_elements(cf_str, 16);
            assert!(elems.is_empty());
            assert_eq!(CFGetRetainCount(cf_str), before - 1);

            let values = [cf_str];
            let arr = CFArrayCreate(std::ptr::null(), values.as_ptr(), 1, std::ptr::null());
            assert!(!arr.is_null());
            let items = cf_array_copy_ax_elements(arr, 16);
            assert!(
                items.is_empty(),
                "non-AXUIElement CFString item inside CFArray must be skipped"
            );
            CFRelease(cf_str);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn pasteboard_require_objc_object_rejects_null_mutable_array() {
        let err = macos_pasteboard::PasteboardRef::require_objc_object(
            std::ptr::null_mut(),
            "NSMutableArray",
        )
        .unwrap_err();
        assert!(err.contains("NSMutableArray"));
    }

    #[test]
    fn open_application_rejects_malformed_and_option_injection_bundle_ids() {
        let driver = MacOsSystemDriver::new_isolated();
        for bad in [
            "",
            "-a.evil",
            "com.apple.Safari' || kMDItemFSName == '*'",
            "singleword",
            "com.example..app",
        ] {
            assert_eq!(
                driver.open_application(bad).unwrap_err(),
                "invalid_bundle_id"
            );
        }
    }

    #[test]
    fn paste_text_with_invalid_target_bundle_id_fails_without_touching_clipboard_or_cmd_v() {
        struct TrustedMacOpenDriver {
            system: MacOsSystemDriver,
            recording: RecordingOsDriver,
        }

        impl HostOsDriver for TrustedMacOpenDriver {
            fn is_accessibility_trusted(&self) -> bool {
                self.recording.is_accessibility_trusted()
            }
            fn open_application(&self, bundle_id: &str) -> Result<(), String> {
                self.system.open_application(bundle_id)
            }
            fn post_key_shortcut(
                &self,
                key_code: u16,
                modifiers: &[String],
                key_down: bool,
            ) -> Result<(), String> {
                self.recording
                    .post_key_shortcut(key_code, modifiers, key_down)
            }
            fn post_apple_fn_flags(&self, key_down: bool) -> Result<(), String> {
                self.recording.post_apple_fn_flags(key_down)
            }
            fn snapshot_clipboard(&self, max_bytes: usize) -> ClipboardSnapshot {
                self.recording.snapshot_clipboard(max_bytes)
            }
            fn write_clipboard_plain_text(&self, text: &str) -> u64 {
                self.recording.write_clipboard_plain_text(text)
            }
            fn write_clipboard_plain_text_checked(&self, text: &str) -> Result<u64, String> {
                self.recording.write_clipboard_plain_text_checked(text)
            }
            fn restore_clipboard(&self, snapshot: &ClipboardSnapshot, expected: u64) -> bool {
                self.recording.restore_clipboard(snapshot, expected)
            }
            fn open_url(&self, url: &str) -> Result<(), String> {
                self.recording.open_url(url)
            }
            fn focus_agent_input(&self, adapter: &str) -> Result<(), String> {
                self.recording.focus_agent_input(adapter)
            }
            fn stop_active_dictation(&self, adapter: &str) -> Result<bool, String> {
                self.recording.stop_active_dictation(adapter)
            }
            fn scroll_window(&self, req: &DialScrollRequest) -> Result<(), String> {
                self.recording.scroll_window(req)
            }
            fn move_cursor_by(&self, dx: f64, dy: f64) -> Result<(), String> {
                self.recording.move_cursor_by(dx, dy)
            }
            fn click_mouse_button(&self, button: MouseButtonKind) -> Result<(), String> {
                self.recording.click_mouse_button(button)
            }
            fn scroll_cursor_wheel(&self, dx: i32, dy: i32) -> Result<(), String> {
                self.recording.scroll_cursor_wheel(dx, dy)
            }
            fn new_conversation_in_project(&self, adapter: &str) -> Result<(), String> {
                self.recording.new_conversation_in_project(adapter)
            }
            fn toggle_live_voice(&self, adapter: &str) -> Result<(), String> {
                self.recording.toggle_live_voice(adapter)
            }
        }

        let recording = RecordingOsDriver::new(true);
        recording.seed_clipboard(
            vec![ClipboardItemSnapshot {
                types_and_bytes: vec![("public.utf8-plain-text".into(), b"keep-me".to_vec())],
            }],
            7,
        );
        let driver = TrustedMacOpenDriver {
            system: MacOsSystemDriver::new_isolated(),
            recording: recording.clone(),
        };
        let executor = LiveHostActionExecutor::new(driver);
        let receipt = executor.execute(
            "Notes",
            "AG00",
            &ResolvedHostAction::PasteText {
                text: "secret-snippet".into(),
                target_bundle_id: Some("-a.evil".into()),
                adapter: None,
                no_newline: false,
                submit: false,
            },
        );
        assert_eq!(receipt.outcome, "failed");
        assert_eq!(receipt.detail, "invalid_bundle_id");
        assert_eq!(recording.current_clipboard().change_count, 7);
        assert_eq!(
            recording.current_clipboard().items,
            vec![ClipboardItemSnapshot {
                types_and_bytes: vec![("public.utf8-plain-text".into(), b"keep-me".to_vec())],
            }]
        );
        assert!(!recording.events().iter().any(|e| matches!(
            e,
            RecordedOsEvent::SnapshotClipboard { .. }
                | RecordedOsEvent::WriteClipboardPlainText(_)
                | RecordedOsEvent::RestoreClipboard { .. }
                | RecordedOsEvent::PostKeyShortcut { .. }
        )));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn model_picker_mouse_tap_timeout_and_drop_balance_run_loop_retains() {
        struct MockTapRuntime {
            delay_before_run_loop: Duration,
            retains: AtomicUsize,
            releases: AtomicUsize,
            stops: AtomicUsize,
            tap_releases: AtomicUsize,
            source_releases: AtomicUsize,
        }

        impl MockTapRuntime {
            fn new(delay_before_run_loop: Duration) -> Self {
                Self {
                    delay_before_run_loop,
                    retains: AtomicUsize::new(0),
                    releases: AtomicUsize::new(0),
                    stops: AtomicUsize::new(0),
                    tap_releases: AtomicUsize::new(0),
                    source_releases: AtomicUsize::new(0),
                }
            }
        }

        impl TapRuntime for MockTapRuntime {
            unsafe fn create_tap(
                &self,
                _options: u32,
                _ctx: *mut std::ffi::c_void,
            ) -> CFMachPortRef {
                0x1001 as CFMachPortRef
            }
            unsafe fn create_source(&self, _tap: CFMachPortRef) -> CFRunLoopSourceRef {
                0x2002 as CFRunLoopSourceRef
            }
            unsafe fn invalidate_port(&self, _tap: CFMachPortRef) {}
            unsafe fn current_run_loop(&self) -> CFRunLoopRef {
                if !self.delay_before_run_loop.is_zero() {
                    std::thread::sleep(self.delay_before_run_loop);
                }
                0x3003 as CFRunLoopRef
            }
            unsafe fn add_source(&self, _rl: CFRunLoopRef, _source: CFRunLoopSourceRef) {}
            unsafe fn remove_source(&self, _rl: CFRunLoopRef, _source: CFRunLoopSourceRef) {}
            unsafe fn enable_tap(&self, _tap: CFMachPortRef, _enable: bool) {}
            unsafe fn run_loop_once(&self) {
                std::thread::sleep(Duration::from_millis(5));
            }
            unsafe fn stop_run_loop(&self, _rl: CFRunLoopRef) {
                self.stops.fetch_add(1, Ordering::SeqCst);
            }
            unsafe fn retain_cf(&self, cf: CFTypeRef) -> CFTypeRef {
                self.retains.fetch_add(1, Ordering::SeqCst);
                cf
            }
            unsafe fn release_cf(&self, cf: CFTypeRef) {
                match cf as usize {
                    0x1001 => {
                        self.tap_releases.fetch_add(1, Ordering::SeqCst);
                    }
                    0x2002 => {
                        self.source_releases.fetch_add(1, Ordering::SeqCst);
                    }
                    0x3003 => {
                        self.releases.fetch_add(1, Ordering::SeqCst);
                    }
                    _ => {}
                }
            }
        }

        let deltas = Arc::new(Mutex::new(Vec::new()));
        let timeout_rt = Arc::new(MockTapRuntime::new(Duration::from_millis(30)));
        assert!(ModelPickerMouseTap::start_with_runtime(
            Arc::clone(&deltas),
            Arc::clone(&timeout_rt) as Arc<dyn TapRuntime>,
            Duration::from_millis(1),
        )
        .is_none());
        assert_eq!(
            timeout_rt.retains.load(Ordering::SeqCst),
            timeout_rt.releases.load(Ordering::SeqCst),
            "timed-out tap setup must balance CFRetain/CFRelease on CFRunLoopRef"
        );
        assert_eq!(timeout_rt.tap_releases.load(Ordering::SeqCst), 1);
        assert_eq!(timeout_rt.source_releases.load(Ordering::SeqCst), 1);

        let live_rt = Arc::new(MockTapRuntime::new(Duration::ZERO));
        let tap = ModelPickerMouseTap::start_with_runtime(
            Arc::clone(&deltas),
            Arc::clone(&live_rt) as Arc<dyn TapRuntime>,
            Duration::from_millis(200),
        )
        .expect("live mock tap should start");
        assert_eq!(live_rt.retains.load(Ordering::SeqCst), 1);
        drop(tap);
        assert_eq!(live_rt.stops.load(Ordering::SeqCst), 1);
        assert_eq!(live_rt.releases.load(Ordering::SeqCst), 1);
        assert_eq!(live_rt.tap_releases.load(Ordering::SeqCst), 1);
        assert_eq!(live_rt.source_releases.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn mouse_click_state_progresses_to_double_and_triple_click_then_resets_on_delay_or_motion() {
        let t0 = Instant::now();
        let c1 = next_mouse_click_state(None, MouseButtonKind::Left, t0, 100.0, 200.0);
        assert_eq!(c1.count, 1);

        let c2 = next_mouse_click_state(
            Some(c1),
            MouseButtonKind::Left,
            t0 + Duration::from_millis(180),
            101.5,
            201.0,
        );
        assert_eq!(c2.count, 2);

        let c3 = next_mouse_click_state(
            Some(c2),
            MouseButtonKind::Left,
            t0 + Duration::from_millis(320),
            102.0,
            201.0,
        );
        assert_eq!(c3.count, 3);

        // Fourth rapid click wraps cleanly to 1
        let c4 = next_mouse_click_state(
            Some(c3),
            MouseButtonKind::Left,
            t0 + Duration::from_millis(420),
            102.0,
            201.0,
        );
        assert_eq!(c4.count, 1);

        // Slight stick tremor (<= 8pt) within macOS 500ms window still counts as double-click
        let c_slight_jitter = next_mouse_click_state(
            Some(c1),
            MouseButtonKind::Left,
            t0 + Duration::from_millis(480),
            106.0,
            200.0,
        );
        assert_eq!(c_slight_jitter.count, 2);

        // Exceeding distance threshold (> 8pt) resets to 1
        let c_moved = next_mouse_click_state(
            Some(c1),
            MouseButtonKind::Left,
            t0 + Duration::from_millis(120),
            112.0,
            200.0,
        );
        assert_eq!(c_moved.count, 1);

        // Exceeding interval threshold (> 500ms) resets to 1
        let c_slow = next_mouse_click_state(
            Some(c1),
            MouseButtonKind::Left,
            t0 + Duration::from_millis(550),
            100.0,
            200.0,
        );
        assert_eq!(c_slow.count, 1);
    }
}
