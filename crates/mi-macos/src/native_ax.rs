use crate::executor::DialScrollRequest;
use crate::managed_controls::{blocked, ManagedAxProvider, ManagedWindow, ManagedWindowIdentity};

#[cfg(not(target_os = "macos"))]
#[derive(Debug, Default)]
pub struct NativeAxProvider;

#[cfg(not(target_os = "macos"))]
impl ManagedAxProvider for NativeAxProvider {
    fn activate_window(&mut self, _: &ManagedWindowIdentity) -> Result<(), String> {
        Err(blocked("macos_required"))
    }
    fn capture(&mut self, _: &str) -> Result<ManagedWindow, String> {
        Err(blocked("macos_required"))
    }
    fn read(&mut self, _: &ManagedWindowIdentity) -> Result<ManagedWindow, String> {
        Err(blocked("macos_required"))
    }
    fn press(&mut self, _: &ManagedWindowIdentity, _: u64) -> Result<(), String> {
        Err(blocked("macos_required"))
    }
    fn focus(&mut self, _: &ManagedWindowIdentity, _: u64) -> Result<(), String> {
        Err(blocked("macos_required"))
    }
    fn scroll(&mut self, _: &ManagedWindowIdentity, _: &DialScrollRequest) -> Result<(), String> {
        Err(blocked("macos_required"))
    }
    fn insert_text(&mut self, _: &ManagedWindowIdentity, _: u64, _: &str) -> Result<(), String> {
        Err(blocked("macos_required"))
    }
    fn shortcut(
        &mut self,
        _: &ManagedWindowIdentity,
        _: u64,
        _: u16,
        _: &[String],
    ) -> Result<(), String> {
        Err(blocked("macos_required"))
    }
    fn submit(&mut self, _: &ManagedWindowIdentity, _: u64) -> Result<(), String> {
        Err(blocked("macos_required"))
    }
    fn send_window_key(
        &mut self,
        _: &ManagedWindowIdentity,
        _: u16,
        _: &[String],
    ) -> Result<(), String> {
        Err(blocked("macos_required"))
    }
}

#[cfg(not(target_os = "macos"))]
pub fn observe_codex_window_state() -> Result<crate::codex_sessions::CodexWindowSnapshot, String> {
    Err(blocked("macos_required"))
}

#[cfg(not(target_os = "macos"))]
pub fn observe_codex_live_window_signals() -> Option<crate::codex_sessions::CodexLiveWindowSignals>
{
    None
}

#[cfg(target_os = "macos")]
pub(crate) use macos::activate_focused_codex_window;
#[cfg(target_os = "macos")]
pub use macos::{
    frontmost_selected_antigravity_conversation, observe_antigravity_sidebar_conversations,
    observe_codex_live_window_signals, observe_codex_window_state, NativeAxProvider,
    NativeSessionNavigation,
};

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use crate::managed_controls::ManagedControl;
    use std::ffi::c_void;
    use std::time::{Duration, Instant};

    type Ref = *const c_void;
    const MAX_NODES: usize = 16384;
    const MAX_DEPTH: usize = 48;
    const UTF8: u32 = 0x0800_0100;

    #[derive(Clone)]
    struct ControlNode<E> {
        pid: Option<i32>,
        role: Option<String>,
        hidden: bool,
        window: Option<E>,
        children: Vec<E>,
    }

    fn validate_control_membership<E>(
        path: &[E],
        scope: &E,
        window: &E,
        pid: i32,
        deadline: Instant,
        same: impl Fn(&E, &E) -> bool,
        mut read_node: impl FnMut(&E) -> Result<ControlNode<E>, String>,
    ) -> Result<(), String> {
        if path.first().is_none_or(|first| !same(first, scope)) {
            return Err(blocked("ax_control_scope_changed"));
        }
        if path.len() > MAX_DEPTH + 1 {
            return Err(blocked("ax_tree_limit"));
        }
        for (index, current) in path.iter().enumerate() {
            if Instant::now() >= deadline {
                return Err(blocked("ax_control_validation_timeout"));
            }
            if path[..index].iter().any(|old| same(old, current)) {
                return Err(blocked("ax_control_ancestry_cycle"));
            }
            let node = read_node(current)?;
            if Instant::now() >= deadline {
                return Err(blocked("ax_control_validation_timeout"));
            }
            if node.pid != Some(pid) {
                return Err(blocked("control_pid_mismatch"));
            }
            if node.role.is_none() {
                return Err(blocked("ax_role_unavailable"));
            }
            if node.hidden {
                return Err(blocked("ax_control_hidden"));
            }
            if node
                .window
                .as_ref()
                .is_some_and(|owner| !same(owner, window))
            {
                return Err(blocked("ax_control_window_changed"));
            }
            if path
                .get(index + 1)
                .is_some_and(|next| !node.children.iter().any(|child| same(child, next)))
            {
                return Err(blocked("ax_control_removed"));
            }
        }
        Ok(())
    }

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default)]
    struct Point {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default)]
    struct Size {
        width: f64,
        height: f64,
    }
    #[derive(Clone, Copy, Debug)]
    struct Rect {
        origin: Point,
        size: Size,
    }
    impl Rect {
        fn intersection(self, other: Self) -> Option<Self> {
            let x = self.origin.x.max(other.origin.x);
            let y = self.origin.y.max(other.origin.y);
            let width =
                (self.origin.x + self.size.width).min(other.origin.x + other.size.width) - x;
            let height =
                (self.origin.y + self.size.height).min(other.origin.y + other.size.height) - y;
            (width > 0.0 && height > 0.0).then_some(Self {
                origin: Point { x, y },
                size: Size { width, height },
            })
        }
    }

    fn scroll_candidate_points(window: Rect, input: Rect, preferred: Option<Rect>) -> Vec<Point> {
        if window.intersection(input).is_none() {
            return Vec::new();
        }
        let above_input = Rect {
            origin: Point {
                x: input.origin.x,
                y: window.origin.y,
            },
            size: Size {
                width: input.size.width,
                height: input.origin.y - window.origin.y,
            },
        };
        let Some(area) = window.intersection(above_input) else {
            return Vec::new();
        };
        let area = preferred
            .and_then(|preferred| area.intersection(preferred))
            .unwrap_or(area);
        [0.75, 0.5, 0.25]
            .into_iter()
            .flat_map(|y| {
                [0.5, 0.25, 0.75].into_iter().map(move |x| Point {
                    x: area.origin.x + area.size.width * x,
                    y: area.origin.y + area.size.height * y,
                })
            })
            .collect()
    }
    #[repr(C)]
    #[derive(Default)]
    struct Range {
        location: isize,
        length: isize,
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn CGEventCreateKeyboardEvent(source: *mut c_void, key: u16, down: bool) -> *mut c_void;
        fn CGEventSetFlags(event: *mut c_void, flags: u64);
        fn CGEventSetLocation(event: *mut c_void, point: Point);
        fn CGEventCreateScrollWheelEvent2(
            source: *mut c_void,
            units: u32,
            wheels: u32,
            wheel1: i32,
            wheel2: i32,
            wheel3: i32,
        ) -> *mut c_void;
        fn CGEventPostToPid(pid: i32, event: *mut c_void);
        fn AXIsProcessTrusted() -> bool;
        fn AXUIElementCreateApplication(pid: i32) -> Ref;
        fn AXUIElementGetPid(element: Ref, pid: *mut i32) -> i32;
        fn AXUIElementCopyElementAtPosition(element: Ref, x: f32, y: f32, result: *mut Ref) -> i32;
        fn AXUIElementSetMessagingTimeout(element: Ref, timeout: f32) -> i32;
        fn AXUIElementCopyMultipleAttributeValues(
            element: Ref,
            attributes: Ref,
            options: u32,
            values: *mut Ref,
        ) -> i32;
        fn AXUIElementCopyActionNames(element: Ref, value: *mut Ref) -> i32;
        fn AXUIElementIsAttributeSettable(element: Ref, attribute: Ref, value: *mut u8) -> i32;
        fn AXUIElementSetAttributeValue(element: Ref, attribute: Ref, value: Ref) -> i32;
        fn AXUIElementPerformAction(element: Ref, action: Ref) -> i32;
        fn AXValueGetValue(value: Ref, kind: u32, output: *mut c_void) -> bool;
        fn AXValueGetTypeID() -> usize;
        fn AXValueGetType(value: Ref) -> u32;
        #[cfg(test)]
        fn AXValueCreate(kind: u32, value: *const c_void) -> Ref;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithBytes(
            allocator: Ref,
            bytes: *const u8,
            count: isize,
            encoding: u32,
            external: u8,
        ) -> Ref;
        fn CFStringGetLength(value: Ref) -> isize;
        fn CFStringCreateWithSubstring(allocator: Ref, string: Ref, range: Range) -> Ref;
        fn CFStringGetMaximumSizeForEncoding(length: isize, encoding: u32) -> isize;
        fn CFStringGetCString(
            value: Ref,
            buffer: *mut std::ffi::c_char,
            size: isize,
            encoding: u32,
        ) -> u8;
        fn CFStringGetTypeID() -> usize;
        fn CFURLGetTypeID() -> usize;
        fn CFURLGetString(value: Ref) -> Ref;
        fn CFArrayGetTypeID() -> usize;
        fn CFArrayCreate(allocator: Ref, values: *const Ref, count: isize, callbacks: Ref) -> Ref;
        fn CFNullGetTypeID() -> usize;
        fn CFArrayGetCount(value: Ref) -> isize;
        fn CFArrayGetValueAtIndex(value: Ref, index: isize) -> Ref;
        fn CFBooleanGetTypeID() -> usize;
        fn CFBooleanGetValue(value: Ref) -> u8;
        fn CFNumberGetTypeID() -> usize;
        fn CFNumberGetValue(value: Ref, kind: i32, output: *mut c_void) -> u8;
        fn CFNumberCreate(allocator: Ref, kind: i32, value: *const c_void) -> Ref;
        fn CFGetTypeID(value: Ref) -> usize;
        fn CFEqual(left: Ref, right: Ref) -> u8;
        fn CFRetain(value: Ref) -> Ref;
        fn CFRelease(value: Ref);
        static kCFBooleanTrue: Ref;
        #[cfg(test)]
        static kCFBooleanFalse: Ref;
        #[cfg(test)]
        static kCFTypeArrayCallBacks: u8;
    }

    #[derive(Debug)]
    struct Owned(Ref);
    unsafe impl Send for Owned {}
    impl Drop for Owned {
        fn drop(&mut self) {
            unsafe { CFRelease(self.0) }
        }
    }
    impl Clone for Owned {
        fn clone(&self) -> Self {
            unsafe { Self(CFRetain(self.0)) }
        }
    }
    impl Owned {
        unsafe fn from_create(value: Ref) -> Option<Self> {
            if value.is_null() {
                None
            } else {
                Some(Self(value))
            }
        }
        fn text_with(
            value: &str,
            create: unsafe extern "C" fn(Ref, *const u8, isize, u32, u8) -> Ref,
        ) -> Option<Self> {
            let cf_str = unsafe {
                create(
                    std::ptr::null(),
                    value.as_ptr(),
                    value.len() as isize,
                    UTF8,
                    0,
                )
            };
            if cf_str.is_null() {
                None
            } else {
                unsafe { Self::from_create(cf_str) }
            }
        }
        fn text(value: &str) -> Option<Self> {
            Self::text_with(value, CFStringCreateWithBytes)
        }
        fn attributes(
            &self,
            names: &[&str],
            deadline: Instant,
        ) -> Result<Vec<Option<Self>>, String> {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| blocked("ax_snapshot_timeout"))?;
            let timeout = remaining.as_secs_f32().min(0.2);
            if unsafe { AXUIElementSetMessagingTimeout(self.0, timeout) } != 0 {
                return Err(blocked("ax_timeout_unavailable"));
            }
            let names: Vec<_> = names
                .iter()
                .map(|name| Self::text(name))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| blocked("ax_attribute_names_unavailable"))?;
            let refs: Vec<_> = names.iter().map(|name| name.0).collect();
            let attributes = unsafe {
                Self::from_create(CFArrayCreate(
                    std::ptr::null(),
                    refs.as_ptr(),
                    refs.len() as isize,
                    std::ptr::null(),
                ))
            }
            .ok_or_else(|| blocked("ax_attribute_names_unavailable"))?;
            let mut result = std::ptr::null();
            let status = unsafe {
                AXUIElementCopyMultipleAttributeValues(self.0, attributes.0, 0, &mut result)
            };
            let result = unsafe { Self::from_create(result) };
            if status != 0 {
                return Err(format!("blocked:ax_batch_{status}"));
            }
            let values = result.ok_or_else(|| blocked("ax_batch_missing"))?.array()?;
            if values.len() != names.len() {
                return Err(blocked("ax_batch_length"));
            }
            values.into_iter().map(decode_attribute_slot).collect()
        }
        fn attr(&self, name: &str) -> Result<Option<Self>, String> {
            Ok(self
                .attributes(&[name], Instant::now() + Duration::from_millis(200))?
                .pop()
                .flatten())
        }
        fn string(&self) -> Option<String> {
            if unsafe { CFGetTypeID(self.0) != CFStringGetTypeID() } {
                return None;
            }
            let mut buffer = [0u8; 4096];
            if unsafe {
                CFStringGetCString(
                    self.0,
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as isize,
                    UTF8,
                )
            } != 0
            {
                let end = buffer.iter().position(|byte| *byte == 0)?;
                return std::str::from_utf8(&buffer[..end]).ok().map(str::to_owned);
            }
            let len = unsafe { CFStringGetLength(self.0) };
            if len < 0 {
                return None;
            }
            let max_bytes =
                unsafe { CFStringGetMaximumSizeForEncoding(len, UTF8) }.checked_add(1)?;
            if max_bytes <= 0 || max_bytes > 65536 {
                return None;
            }
            let mut heap = vec![0u8; max_bytes as usize];
            if unsafe { CFStringGetCString(self.0, heap.as_mut_ptr().cast(), max_bytes, UTF8) } == 0
            {
                return None;
            }
            let end = heap.iter().position(|byte| *byte == 0)?;
            std::str::from_utf8(&heap[..end]).ok().map(str::to_owned)
        }
        fn string_attr(&self, name: &str) -> Result<Option<String>, String> {
            self.attr(name)?
                .map(|value| {
                    value
                        .string()
                        .ok_or_else(|| blocked("ax_string_type_or_limit"))
                })
                .transpose()
        }
        fn boolean(&self) -> Result<bool, String> {
            if unsafe { CFGetTypeID(self.0) == CFBooleanGetTypeID() } {
                return Ok(unsafe { CFBooleanGetValue(self.0) } != 0);
            }
            self.number()
                .map(|value| value != 0.0)
                .ok_or_else(|| blocked("ax_boolean_type"))
        }
        fn number(&self) -> Option<f64> {
            if unsafe { CFGetTypeID(self.0) != CFNumberGetTypeID() } {
                return None;
            }
            let mut value = 0f64;
            (unsafe { CFNumberGetValue(self.0, 13, (&mut value as *mut f64).cast()) } != 0
                && value.is_finite())
            .then_some(value)
        }
        fn bool_attr(&self, name: &str) -> Result<Option<bool>, String> {
            self.attr(name)?.map(|value| value.boolean()).transpose()
        }
        fn array(&self) -> Result<Vec<Self>, String> {
            if unsafe { CFGetTypeID(self.0) != CFArrayGetTypeID() } {
                return Err(blocked("ax_array_type"));
            }
            let count = unsafe { CFArrayGetCount(self.0) };
            if count < 0 || count as usize > MAX_NODES {
                return Err(blocked("ax_tree_limit"));
            }
            let mut items = Vec::new();
            for index in 0..count {
                let value = unsafe { CFArrayGetValueAtIndex(self.0, index) };
                if value.is_null() {
                    return Err(blocked("ax_array_null_element"));
                }
                items.push(unsafe { Self(CFRetain(value)) });
            }
            Ok(items)
        }
        fn actions(&self) -> Result<Vec<String>, String> {
            let mut value = std::ptr::null();
            let status = unsafe { AXUIElementCopyActionNames(self.0, &mut value) };
            let value = unsafe { Self::from_create(value) };
            if attribute_error(status)?.is_none() {
                return Ok(Vec::new());
            }
            value
                .ok_or_else(|| blocked("ax_actions_missing"))?
                .array()?
                .iter()
                .map(|item| item.string().ok_or_else(|| blocked("ax_action_type")))
                .collect()
        }
        fn settable(&self, name: &str) -> Result<bool, String> {
            let name = Self::text(name).ok_or_else(|| blocked("ax_attribute_name_unavailable"))?;
            let mut value = 0u8;
            let status = unsafe { AXUIElementIsAttributeSettable(self.0, name.0, &mut value) };
            Ok(attribute_error(status)?.is_some() && value != 0)
        }
        fn set(&self, name: &str, value: Ref) -> Result<(), String> {
            let name = Self::text(name)
                .ok_or_else(|| "unconfirmed:ax_set_attr_unavailable".to_string())?;
            let code = unsafe { AXUIElementSetAttributeValue(self.0, name.0, value) };
            if code == 0 {
                Ok(())
            } else {
                Err(format!("unconfirmed:ax_set_{code}"))
            }
        }
        fn action(&self, action: &str) -> Result<(), String> {
            let action = Self::text(action)
                .ok_or_else(|| "unconfirmed:ax_action_name_unavailable".to_string())?;
            let code = unsafe { AXUIElementPerformAction(self.0, action.0) };
            if code == 0 {
                Ok(())
            } else {
                Err(format!("unconfirmed:ax_action_{code}"))
            }
        }
        fn pid(&self) -> Option<i32> {
            let mut pid = 0;
            (unsafe { AXUIElementGetPid(self.0, &mut pid) } == 0 && pid > 0).then_some(pid)
        }
        fn rect(&self) -> Result<Option<Rect>, String> {
            let mut attrs = self
                .attributes(
                    &["AXPosition", "AXSize"],
                    Instant::now() + Duration::from_millis(200),
                )?
                .into_iter();
            let (Some(origin), Some(size)) = (attrs.next().flatten(), attrs.next().flatten())
            else {
                return Ok(None);
            };
            if unsafe {
                CFGetTypeID(origin.0) != AXValueGetTypeID()
                    || CFGetTypeID(size.0) != AXValueGetTypeID()
            } {
                return Err(blocked("ax_geometry_type"));
            }
            let mut point = Point::default();
            let mut dimensions = Size::default();
            if !unsafe {
                AXValueGetValue(origin.0, 1, (&mut point as *mut Point).cast())
                    && AXValueGetValue(size.0, 2, (&mut dimensions as *mut Size).cast())
            } {
                return Err(blocked("ax_geometry_type"));
            }
            Ok((point.x.is_finite()
                && point.y.is_finite()
                && dimensions.width.is_finite()
                && dimensions.height.is_finite()
                && dimensions.width > 0.0
                && dimensions.height > 0.0)
                .then_some(Rect {
                    origin: point,
                    size: dimensions,
                }))
        }
    }

    fn attribute_error(code: i32) -> Result<Option<()>, String> {
        match code {
            0 => Ok(Some(())),
            -25205 | -25212 => Ok(None),
            _ => Err(format!("blocked:ax_attribute_{code}")),
        }
    }

    fn decode_attribute_slot(value: Owned) -> Result<Option<Owned>, String> {
        if unsafe { CFGetTypeID(value.0) == CFNullGetTypeID() } {
            return Ok(None);
        }
        if unsafe { CFGetTypeID(value.0) == AXValueGetTypeID() && AXValueGetType(value.0) == 5 } {
            let mut code = 0i32;
            if !unsafe { AXValueGetValue(value.0, 5, (&mut code as *mut i32).cast()) } {
                return Err(blocked("ax_slot_error_unavailable"));
            }
            return attribute_error(code).map(|_| None);
        }
        Ok(Some(value))
    }

    fn text_attribute(value: Option<Owned>) -> Result<Option<String>, String> {
        value
            .map(|value| {
                value
                    .string()
                    .ok_or_else(|| blocked("ax_string_type_or_limit"))
            })
            .transpose()
    }
    fn lenient_text_attribute(value: Option<Owned>) -> Result<Option<String>, String> {
        value
            .map(|value| {
                if unsafe { CFGetTypeID(value.0) == CFStringGetTypeID() } {
                    Ok(value.string().unwrap_or_default())
                } else {
                    Err(blocked("ax_string_type_or_limit"))
                }
            })
            .transpose()
    }
    fn optional_codex_text_attribute(value: Option<Owned>) -> Option<String> {
        let value = value?;
        if unsafe { CFGetTypeID(value.0) != CFStringGetTypeID() } {
            return None;
        }
        let len = unsafe { CFStringGetLength(value.0) };
        if len <= 0 {
            return Some(String::new());
        }
        if len > 2048 {
            let sub = unsafe {
                Owned::from_create(CFStringCreateWithSubstring(
                    std::ptr::null(),
                    value.0,
                    Range {
                        location: 0,
                        length: 2048,
                    },
                ))
            };
            return sub.and_then(|s| s.string());
        }
        value.string()
    }
    fn lenient_url_attribute(value: Owned) -> Result<String, String> {
        if unsafe { CFGetTypeID(value.0) == CFURLGetTypeID() } {
            let text = unsafe { CFURLGetString(value.0) };
            if text.is_null() {
                return Err(blocked("ax_url_unavailable"));
            }
            Ok(unsafe { Owned(CFRetain(text)) }
                .string()
                .unwrap_or_default())
        } else if unsafe { CFGetTypeID(value.0) == CFStringGetTypeID() } {
            Ok(value.string().unwrap_or_default())
        } else {
            Err(blocked("ax_url_type_or_limit"))
        }
    }
    fn bool_attribute(value: Option<Owned>) -> Result<Option<bool>, String> {
        value.map(|value| value.boolean()).transpose()
    }

    fn unconfirmed(error: String) -> String {
        if error.starts_with("unconfirmed:") {
            error
        } else {
            format!("unconfirmed:{error}")
        }
    }

    fn write_with_readback(
        write: impl FnOnce() -> Result<(), String>,
        mut read: impl FnMut() -> Result<bool, String>,
        timeout_reason: &str,
    ) -> Result<(), String> {
        write()?;
        let deadline = Instant::now() + Duration::from_millis(400);
        loop {
            if read().map_err(unconfirmed)? {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(format!("unconfirmed:{timeout_reason}"));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum ObservationPurpose {
        Controls,
        AntigravityControls,
        Sessions,
        SessionDocuments,
    }

    fn is_antigravity_hub_web_area(url: Option<&str>, title: Option<&str>) -> bool {
        let Some(raw_url) = url.map(str::trim).filter(|u| !u.is_empty()) else {
            return true;
        };
        let is_loopback_https = raw_url
            .strip_prefix("https://")
            .is_some_and(|rest| rest.starts_with("127.0.0.1:") || rest.starts_with("localhost:"));
        if !is_loopback_https {
            return false;
        }
        match title.map(str::trim).filter(|t| !t.is_empty()) {
            None => true,
            Some(t) => t.contains("Antigravity") || t == "Jetski Web",
        }
    }

    #[derive(Clone, Default)]
    struct NodeData {
        role: String,
        children: Vec<Owned>,
        label: String,
        enabled: bool,
        pressable: bool,
        editable: bool,
        focused: bool,
        expanded: Option<bool>,
        url: Option<String>,
    }
    #[derive(Clone)]
    struct ObservedNode {
        element: Owned,
        parent: Option<usize>,
        data: NodeData,
    }
    #[derive(Clone)]
    struct WindowObservation {
        nodes: Vec<ObservedNode>,
    }

    fn observe_tree(
        root: Owned,
        deadline: Instant,
        mut read_node: impl FnMut(&Owned) -> Result<Option<NodeData>, String>,
    ) -> Result<WindowObservation, String> {
        loop {
            match observe_tree_once(root.clone(), deadline, &mut read_node) {
                Err(error) if invalid_ax_observation(&error) => {
                    let remaining = deadline
                        .checked_duration_since(Instant::now())
                        .ok_or_else(|| blocked("ax_snapshot_timeout"))?;
                    std::thread::sleep(remaining.min(Duration::from_millis(10)));
                }
                result => return result,
            }
        }
    }

    fn invalid_ax_observation(error: &str) -> bool {
        matches!(
            error,
            "blocked:ax_batch_-25202" | "blocked:ax_attribute_-25202"
        )
    }

    fn observe_tree_once(
        root: Owned,
        deadline: Instant,
        mut read_node: impl FnMut(&Owned) -> Result<Option<NodeData>, String>,
    ) -> Result<WindowObservation, String> {
        let mut nodes = Vec::new();
        let mut pending = vec![(root, None, 0)];
        let mut visited = 0;
        while let Some((element, parent, depth)) = pending.pop() {
            visited += 1;
            if visited > MAX_NODES || depth > MAX_DEPTH {
                return Err(blocked("ax_tree_limit"));
            }
            if Instant::now() >= deadline {
                return Err(blocked("ax_snapshot_timeout"));
            }
            let Some(mut data) = read_node(&element)? else {
                continue;
            };
            if Instant::now() >= deadline {
                return Err(blocked("ax_snapshot_timeout"));
            }
            if visited + pending.len() + data.children.len() > MAX_NODES {
                return Err(blocked("ax_tree_limit"));
            }
            let index = nodes.len();
            for child in std::mem::take(&mut data.children).into_iter().rev() {
                pending.push((child, Some(index), depth + 1));
            }
            nodes.push(ObservedNode {
                element,
                parent,
                data,
            });
        }
        Ok(WindowObservation { nodes })
    }

    fn observe_window(
        root: Owned,
        purpose: ObservationPurpose,
    ) -> Result<WindowObservation, String> {
        observe_window_until(root, purpose, Instant::now() + Duration::from_secs(2))
    }
    fn observe_window_until(
        root: Owned,
        purpose: ObservationPurpose,
        deadline: Instant,
    ) -> Result<WindowObservation, String> {
        observe_tree(root, deadline, |element| {
            read_observed_node(
                purpose,
                |names| element.attributes(names, deadline),
                |name| element.settable(name),
                || element.actions(),
            )
        })
    }

    fn read_observed_node(
        purpose: ObservationPurpose,
        mut attributes: impl FnMut(&[&str]) -> Result<Vec<Option<Owned>>, String>,
        mut settable: impl FnMut(&str) -> Result<bool, String>,
        mut actions: impl FnMut() -> Result<Vec<String>, String>,
    ) -> Result<Option<NodeData>, String> {
        let mut attrs = attributes(&["AXRole", "AXSubrole", "AXHidden", "AXChildren"])?.into_iter();
        let role = text_attribute(attrs.next().flatten())?
            .ok_or_else(|| blocked("ax_role_unavailable"))?;
        let subrole = text_attribute(attrs.next().flatten())?;
        let hidden = bool_attribute(attrs.next().flatten())?.unwrap_or(false);
        let raw_children = attrs.next().flatten();
        if hidden || (role == "AXStaticText" && purpose != ObservationPurpose::Sessions) {
            return Ok(None);
        }
        let input = matches!(
            role.as_str(),
            "AXTextArea" | "AXTextField" | "AXSecureTextField"
        );
        let combobox = role == "AXComboBox";
        let labeled_input = role == "AXTextField" || combobox;
        let button = matches!(
            role.as_str(),
            "AXButton"
                | "AXCheckBox"
                | "AXRadioButton"
                | "AXSwitch"
                | "AXToggleButton"
                | "AXLink"
                | "AXPopUpButton"
                | "AXMenuItem"
        );
        let children =
            if input || (purpose == ObservationPurpose::Sessions && role == "AXStaticText") {
                Vec::new()
            } else {
                raw_children
                    .map(|value| value.array())
                    .transpose()?
                    .unwrap_or_default()
            };
        let status_group = role == "AXGroup" && subrole.as_deref() == Some("AXApplicationStatus");
        let session_container = purpose == ObservationPurpose::Sessions
            && !children.is_empty()
            && matches!(role.as_str(), "AXGroup" | "AXScrollArea" | "AXList");
        let names: &[&str] = match purpose {
            ObservationPurpose::Controls | ObservationPurpose::AntigravityControls
                if labeled_input =>
            {
                &["AXDescription", "AXTitle", "AXEnabled", "AXFocused"]
            }
            ObservationPurpose::Controls | ObservationPurpose::AntigravityControls if input => {
                &["AXEnabled", "AXFocused"]
            }
            ObservationPurpose::Controls | ObservationPurpose::AntigravityControls
                if role == "AXPopUpButton" =>
            {
                &["AXDescription", "AXTitle", "AXEnabled", "AXExpanded"]
            }
            ObservationPurpose::Controls | ObservationPurpose::AntigravityControls
                if button || role == "AXHeading" || status_group =>
            {
                &["AXDescription", "AXTitle", "AXEnabled"]
            }
            ObservationPurpose::AntigravityControls if role == "AXWebArea" => &["AXURL", "AXTitle"],
            ObservationPurpose::Sessions if role == "AXLink" => {
                &["AXDescription", "AXTitle", "AXEnabled", "AXURL"]
            }
            ObservationPurpose::Sessions if role == "AXButton" => {
                &["AXDescription", "AXTitle", "AXEnabled", "AXExpanded"]
            }
            ObservationPurpose::Sessions if role == "AXStaticText" => &["AXValue", "AXEnabled"],
            ObservationPurpose::Sessions if session_container => &["AXDescription", "AXTitle"],
            ObservationPurpose::Sessions | ObservationPurpose::SessionDocuments
                if role == "AXWebArea" =>
            {
                &["AXURL"]
            }
            _ => &[],
        };
        let mut attrs = if names.is_empty() {
            Vec::new()
        } else {
            attributes(names)?
        }
        .into_iter();
        let mut data = NodeData {
            role,
            children,
            enabled: true,
            ..Default::default()
        };
        match purpose {
            ObservationPurpose::Controls | ObservationPurpose::AntigravityControls
                if labeled_input =>
            {
                let desc = text_attribute(attrs.next().flatten())?;
                let title = text_attribute(attrs.next().flatten())?;
                data.label = desc
                    .filter(|s| !s.trim().is_empty())
                    .or(title)
                    .unwrap_or_default();
                data.enabled = bool_attribute(attrs.next().flatten())?.unwrap_or(true);
                data.focused = bool_attribute(attrs.next().flatten())?.unwrap_or(false);
                data.editable =
                    subrole.as_deref() != Some("AXSecureTextField") && settable("AXValue")?;
            }
            ObservationPurpose::Controls | ObservationPurpose::AntigravityControls if input => {
                data.enabled = bool_attribute(attrs.next().flatten())?.unwrap_or(true);
                data.focused = bool_attribute(attrs.next().flatten())?.unwrap_or(false);
                data.editable = data.role != "AXSecureTextField"
                    && subrole.as_deref() != Some("AXSecureTextField")
                    && settable("AXValue")?;
            }
            ObservationPurpose::Controls | ObservationPurpose::AntigravityControls
                if button || data.role == "AXHeading" || status_group =>
            {
                let desc = text_attribute(attrs.next().flatten())?;
                let title = text_attribute(attrs.next().flatten())?;
                data.label = desc
                    .filter(|s| !s.trim().is_empty())
                    .or(title)
                    .unwrap_or_default();
                data.enabled = bool_attribute(attrs.next().flatten())?.unwrap_or(true);
                if data.role == "AXPopUpButton" {
                    data.expanded = bool_attribute(attrs.next().flatten())?;
                }
                data.pressable = button && actions()?.iter().any(|action| action == "AXPress");
            }
            ObservationPurpose::AntigravityControls if data.role == "AXWebArea" => {
                data.url = attrs
                    .next()
                    .flatten()
                    .map(lenient_url_attribute)
                    .transpose()?;
                let title = lenient_text_attribute(attrs.next().flatten())?;
                if !is_antigravity_hub_web_area(data.url.as_deref(), title.as_deref()) {
                    data.children.clear();
                }
            }
            ObservationPurpose::Sessions if data.role == "AXLink" => {
                let desc = lenient_text_attribute(attrs.next().flatten())?;
                let title = lenient_text_attribute(attrs.next().flatten())?;
                data.label = desc
                    .filter(|s| !s.trim().is_empty())
                    .or(title)
                    .unwrap_or_default();
                data.enabled = bool_attribute(attrs.next().flatten())?.unwrap_or(true);
                data.url = attrs
                    .next()
                    .flatten()
                    .map(lenient_url_attribute)
                    .transpose()?;
                data.pressable = data.enabled
                    && data
                        .url
                        .as_deref()
                        .is_some_and(|url| url.starts_with("https://127.0.0.1:"))
                    && actions()?.iter().any(|action| action == "AXPress");
            }
            ObservationPurpose::Sessions if data.role == "AXButton" => {
                let desc = lenient_text_attribute(attrs.next().flatten())?;
                let title = lenient_text_attribute(attrs.next().flatten())?;
                data.label = desc
                    .filter(|s| !s.trim().is_empty())
                    .or(title)
                    .unwrap_or_default();
                data.enabled = bool_attribute(attrs.next().flatten())?.unwrap_or(true);
                data.expanded = bool_attribute(attrs.next().flatten())?;
                data.pressable =
                    data.enabled && actions()?.iter().any(|action| action == "AXPress");
            }
            ObservationPurpose::Sessions if data.role == "AXStaticText" => {
                data.label = lenient_text_attribute(attrs.next().flatten())?.unwrap_or_default();
                data.enabled = bool_attribute(attrs.next().flatten())?.unwrap_or(true);
                data.pressable = false;
            }
            ObservationPurpose::Sessions if session_container => {
                let desc = lenient_text_attribute(attrs.next().flatten())?;
                let title = lenient_text_attribute(attrs.next().flatten())?;
                let is_chat_transcript_region = |text: Option<&str>| {
                    text.is_some_and(crate::antigravity_ax_labels::is_chat_transcript_region)
                };
                if is_chat_transcript_region(desc.as_deref())
                    || is_chat_transcript_region(title.as_deref())
                {
                    data.children.clear();
                }
            }
            ObservationPurpose::Sessions | ObservationPurpose::SessionDocuments
                if data.role == "AXWebArea" =>
            {
                data.url = attrs
                    .next()
                    .flatten()
                    .map(lenient_url_attribute)
                    .transpose()?;
                let should_prune = (purpose == ObservationPurpose::SessionDocuments
                    && data.url.as_deref().is_some_and(|url| !url.is_empty()))
                    || (purpose == ObservationPurpose::Sessions
                        && !is_antigravity_hub_web_area(data.url.as_deref(), None));
                if should_prune {
                    data.children.clear();
                }
            }
            _ => {}
        }
        Ok(Some(data))
    }

    fn enable_application_accessibility_with(
        settable: impl Fn(&str) -> Result<bool, String>,
        read_bool: impl Fn(&str) -> Result<Option<bool>, String>,
        mut set_true: impl FnMut(&str) -> i32,
    ) -> Result<(), String> {
        if !settable("AXManualAccessibility")? {
            return Ok(());
        }
        let manual_code = set_true("AXManualAccessibility");
        if manual_code != 0 {
            return Err(format!("unconfirmed:ax_set_{manual_code}"));
        }
        if read_bool("AXManualAccessibility")?.unwrap_or(false) {
            return Ok(());
        }
        if !read_bool("AXEnhancedUserInterface")?.unwrap_or(false) {
            let enhanced_code = set_true("AXEnhancedUserInterface");
            if enhanced_code != 0 && enhanced_code != -25208 {
                return Err(format!("unconfirmed:ax_set_{enhanced_code}"));
            }
        }
        Ok(())
    }

    fn enable_application_accessibility(application: &Owned) -> Result<(), String> {
        enable_application_accessibility_with(
            |name| application.settable(name),
            |name| application.bool_attr(name),
            |name| {
                let Some(attr) = Owned::text(name) else {
                    return -25200;
                };
                unsafe { AXUIElementSetAttributeValue(application.0, attr.0, kCFBooleanTrue) }
            },
        )
    }

    pub fn frontmost_selected_antigravity_conversation(pid: i32, port: u16) -> Option<String> {
        if port == 0 || pid <= 0 {
            return None;
        }
        if !unsafe { AXIsProcessTrusted() } {
            return None;
        }
        let application = unsafe { Owned::from_create(AXUIElementCreateApplication(pid)) }?;
        let deadline = Instant::now() + Duration::from_millis(400);
        if unsafe { AXUIElementSetMessagingTimeout(application.0, 0.2) } != 0 {
            return None;
        }
        enable_application_accessibility(&application).ok()?;
        let mut attrs = application
            .attributes(&["AXFrontmost", "AXFocusedWindow", "AXWindows"], deadline)
            .ok()?
            .into_iter();
        if bool_attribute(attrs.next().flatten()).ok()? != Some(true) {
            return None;
        }
        let focused = attrs.next().flatten();
        let windows = attrs.next().flatten()?.array().ok()?;
        let mut visible = Vec::new();
        for element in windows {
            let mut win_attrs = element
                .attributes(&["AXRole", "AXMinimized"], deadline)
                .ok()?
                .into_iter();
            let role = text_attribute(win_attrs.next().flatten()).ok()??;
            let minimized = bool_attribute(win_attrs.next().flatten())
                .ok()?
                .unwrap_or(false);
            if role == "AXWindow" && !minimized {
                visible.push(element);
            }
        }
        let index = focused
            .as_ref()
            .and_then(|focused| {
                visible
                    .iter()
                    .position(|window| unsafe { CFEqual(window.0, focused.0) } != 0)
            })
            .or_else(|| (visible.len() == 1).then_some(0))?;
        let window = visible.swap_remove(index);
        let observation =
            observe_window_until(window, ObservationPurpose::SessionDocuments, deadline).ok()?;
        let nav_window = crate::session_navigation::SessionNavigationWindow {
            pid,
            window: 1,
            selected_urls: selected_web_area_urls(&observation),
        };
        crate::session_navigation::selected_conversation(&nav_window, port).ok()
    }

    #[derive(Debug, Clone)]
    struct CapturedWindow {
        bundle_id: String,
        pid: i32,
        application: Owned,
        element: Owned,
        title: String,
    }
    impl CapturedWindow {
        fn capture(bundle: &str, prefer_focused: bool) -> Result<Self, String> {
            if !unsafe { AXIsProcessTrusted() } {
                return Err(blocked("accessibility_untrusted"));
            }
            let pid = crate::executor::find_pid_for_bundle_id(bundle)
                .ok_or_else(|| blocked("target_application_missing"))?;
            let application = unsafe { Owned::from_create(AXUIElementCreateApplication(pid)) }
                .ok_or_else(|| blocked("ax_application_unavailable"))?;
            let deadline = Instant::now() + Duration::from_secs(2);
            if unsafe { AXUIElementSetMessagingTimeout(application.0, 0.2) } != 0 {
                return Err(blocked("ax_timeout_unavailable"));
            }
            enable_application_accessibility(&application)?;
            let mut attrs = application
                .attributes(&["AXWindows", "AXFocusedWindow"], deadline)?
                .into_iter();
            let windows = attrs
                .next()
                .flatten()
                .ok_or_else(|| blocked("target_window_missing"))?
                .array()?;
            let focused = attrs.next().flatten();
            let mut visible = Vec::new();
            for element in windows {
                let mut attrs = element
                    .attributes(&["AXRole", "AXMinimized", "AXTitle"], deadline)?
                    .into_iter();
                let role = text_attribute(attrs.next().flatten())?
                    .ok_or_else(|| blocked("ax_role_unavailable"))?;
                let minimized = bool_attribute(attrs.next().flatten())?.unwrap_or(false);
                let title = text_attribute(attrs.next().flatten())?.unwrap_or_default();
                if role == "AXWindow" && !minimized {
                    visible.push((element, title));
                }
            }
            let selected = if prefer_focused {
                focused.as_ref().and_then(|focused| {
                    visible
                        .iter()
                        .position(|(window, _)| unsafe { CFEqual(window.0, focused.0) } != 0)
                })
            } else {
                None
            };
            let index = selected
                .or_else(|| (visible.len() == 1).then_some(0))
                .ok_or_else(|| blocked("ambiguous_or_missing_target_window"))?;
            let (element, title) = visible.swap_remove(index);
            let window = Self {
                bundle_id: bundle.into(),
                pid,
                application,
                element,
                title,
            };
            window.validate(None)?;
            Ok(window)
        }
        fn validate_container(&self) -> Result<String, String> {
            if !unsafe { AXIsProcessTrusted() } {
                return Err(blocked("accessibility_untrusted"));
            }
            if self.application.pid() != Some(self.pid)
                || self.element.pid() != Some(self.pid)
                || crate::executor::find_pid_for_bundle_id(&self.bundle_id) != Some(self.pid)
            {
                return Err(blocked("target_pid_mismatch"));
            }
            let windows = self
                .application
                .attr("AXWindows")?
                .ok_or_else(|| blocked("target_window_lost"))?
                .array()?;
            if !windows
                .iter()
                .any(|window| unsafe { CFEqual(window.0, self.element.0) } != 0)
            {
                return Err(blocked("target_window_changed"));
            }
            let mut attrs = self
                .element
                .attributes(
                    &["AXMinimized", "AXTitle"],
                    Instant::now() + Duration::from_millis(400),
                )?
                .into_iter();
            if bool_attribute(attrs.next().flatten())? == Some(true) {
                return Err(blocked("target_window_changed"));
            }
            Ok(text_attribute(attrs.next().flatten())?.unwrap_or_default())
        }
        fn validate(&self, expected_title: Option<&str>) -> Result<(), String> {
            let title = self.validate_container()?;
            if expected_title.is_some_and(|expected| expected != title) {
                return Err(blocked("target_window_changed"));
            }
            Ok(())
        }
    }

    pub(crate) fn activate_focused_codex_window() -> Result<i32, String> {
        let window = CapturedWindow::capture("com.openai.codex", true)?;
        window
            .application
            .set("AXFrontmost", unsafe { kCFBooleanTrue })?;
        window.element.action("AXRaise")?;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            window.validate(None)?;
            let focused = window.application.attr("AXFocusedWindow")?;
            if window.application.bool_attr("AXFrontmost")? == Some(true)
                && focused
                    .as_ref()
                    .is_some_and(|focused| unsafe { CFEqual(focused.0, window.element.0) != 0 })
            {
                return Ok(window.pid);
            }
            if Instant::now() >= deadline {
                return Err(blocked("target_window_not_focused"));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[derive(Debug, Default)]
    pub struct NativeSessionNavigation {
        window: Option<CapturedWindow>,
        port: u16,
        workspace_path: String,
        project_id: Option<String>,
    }

    impl NativeSessionNavigation {
        pub fn new(port: u16, workspace_path: impl Into<String>, project_id: Option<&str>) -> Self {
            Self {
                window: None,
                port,
                workspace_path: workspace_path.into(),
                project_id: project_id
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned),
            }
        }

        fn observe_until(&self, pid: i32, deadline: Instant) -> Result<WindowObservation, String> {
            self.observe_purpose_until(pid, ObservationPurpose::Sessions, deadline)
        }

        fn observe_purpose_until(
            &self,
            pid: i32,
            purpose: ObservationPurpose,
            deadline: Instant,
        ) -> Result<WindowObservation, String> {
            let window = self
                .window
                .as_ref()
                .ok_or_else(|| blocked("session_window_missing"))?;
            if window.pid != pid {
                return Err(blocked("session_process_changed"));
            }
            window.validate(None)?;
            let observation = observe_window_until(window.element.clone(), purpose, deadline)?;
            window.validate(None)?;
            Ok(observation)
        }

        fn inspect(
            &self,
            pid: i32,
        ) -> Result<crate::session_navigation::SessionNavigationWindow, String> {
            use crate::session_navigation::SessionNavigationWindow;
            let observation = self.observe_purpose_until(
                pid,
                ObservationPurpose::SessionDocuments,
                Instant::now() + Duration::from_secs(2),
            )?;
            Ok(SessionNavigationWindow {
                pid,
                window: 1,
                selected_urls: selected_web_area_urls(&observation),
            })
        }

        fn before_mutation(&self, is_current: &dyn Fn() -> bool) -> Result<(), String> {
            self.window
                .as_ref()
                .ok_or_else(|| blocked("session_window_missing"))?
                .validate(None)?;
            if !is_current() {
                return Err(blocked("session_navigation_retired"));
            }
            Ok(())
        }
    }

    fn selected_web_area_urls(observation: &WindowObservation) -> Vec<String> {
        let mut selected_urls = Vec::new();
        for node in &observation.nodes {
            let Some(url) = &node.data.url else {
                continue;
            };
            if node.data.role == "AXWebArea" {
                let mut ancestor = node.parent;
                let mut nested = false;
                while let Some(index) = ancestor {
                    let parent = &observation.nodes[index];
                    if parent.data.role == "AXWebArea"
                        && parent.data.url.as_ref().is_some_and(|url| !url.is_empty())
                    {
                        nested = true;
                        break;
                    }
                    ancestor = parent.parent;
                }
                if !nested && !url.is_empty() {
                    selected_urls.push(url.clone());
                }
            }
        }
        selected_urls
    }

    fn subtree_end(nodes: &[ObservedNode], root: usize) -> usize {
        let mut index = root + 1;
        while index < nodes.len() && nodes[index].parent.is_some_and(|parent| parent >= root) {
            index += 1;
        }
        index
    }

    fn is_sidebar_boundary_label(trimmed: &str) -> bool {
        crate::antigravity_ax_labels::is_sidebar_boundary(trimmed)
    }

    fn is_row_action_label(trimmed: &str) -> bool {
        crate::antigravity_ax_labels::is_row_action(trimmed)
    }

    fn is_utility_sidebar_button_label(label: &str) -> bool {
        crate::antigravity_ax_labels::is_utility_sidebar_button(label)
    }

    fn is_row_boundary_button(node: &ObservedNode) -> bool {
        let trimmed = node.data.label.trim();
        !is_row_action_label(trimmed)
            && (node.data.expanded.is_some() || is_sidebar_boundary_label(trimmed))
    }

    fn classify_sidebar_row(
        observation: &WindowObservation,
        link_idx: usize,
    ) -> Option<(String, crate::session_navigation::SidebarRowVisualState)> {
        use crate::session_navigation::SidebarRowVisualState;
        let nodes = &observation.nodes;
        let link = nodes.get(link_idx)?;
        let link_end = subtree_end(nodes, link_idx);
        let mut title = link.data.label.trim().to_string();
        if title.is_empty() {
            for child in &nodes[(link_idx + 1)..link_end] {
                if child.data.role == "AXStaticText" && !child.data.label.trim().is_empty() {
                    title = child.data.label.trim().to_string();
                    break;
                }
            }
        }
        let mut container = link.parent;
        for _ in 0..2 {
            if let Some(c) = container {
                if subtree_end(nodes, c) == link_end {
                    container = nodes[c].parent;
                } else {
                    break;
                }
            }
        }
        let container_idx = container?;
        let container_end = subtree_end(nodes, container_idx);
        let mut has_stop = false;
        let mut has_archive = false;
        let mut after_actions_idx = link_end;
        let mut idx = link_end;
        while idx < container_end {
            let node = &nodes[idx];
            if node.data.role == "AXLink" || node.data.role == "AXHeading" {
                break;
            }
            if node.data.role == "AXButton" {
                if is_row_boundary_button(node) {
                    break;
                }
                let trimmed = node.data.label.trim();
                if crate::antigravity_ax_labels::is_stop_execution(trimmed) {
                    has_stop = true;
                } else if crate::antigravity_ax_labels::is_archive_conversation(trimmed) {
                    has_archive = true;
                }
                if is_row_action_label(trimmed) {
                    after_actions_idx = subtree_end(nodes, idx);
                }
            }
            idx += 1;
        }
        let row_end = idx;
        if has_stop {
            Some((title, SidebarRowVisualState::InProgress))
        } else if has_archive {
            let has_trailing_timestamp = nodes[after_actions_idx..row_end]
                .iter()
                .any(|n| n.data.role == "AXStaticText" && !n.data.label.trim().is_empty());
            let state = if has_trailing_timestamp {
                SidebarRowVisualState::ReadIdle
            } else {
                SidebarRowVisualState::UnreadIdle
            };
            Some((title, state))
        } else {
            None
        }
    }

    fn observe_sidebar_conversations(
        observation: &WindowObservation,
        port: u16,
    ) -> Vec<crate::session_navigation::SidebarConversationObservation> {
        use crate::session_navigation::{conversation_id_from_url, SidebarConversationObservation};
        let mut rows = Vec::new();
        for (idx, node) in observation.nodes.iter().enumerate() {
            if node.data.role != "AXLink" {
                continue;
            }
            let Some(cid) = node
                .data
                .url
                .as_deref()
                .and_then(|url| conversation_id_from_url(url, port))
            else {
                continue;
            };
            if let Some((title, state)) = classify_sidebar_row(observation, idx) {
                rows.push(SidebarConversationObservation {
                    conversation_id: cid.to_string(),
                    title,
                    state,
                });
            }
        }
        rows
    }

    pub fn observe_antigravity_sidebar_conversations(
        pid: i32,
        port: u16,
    ) -> Result<Vec<crate::session_navigation::SidebarConversationObservation>, String> {
        let mut nav = NativeSessionNavigation::new(port, "", None);
        use crate::session_navigation::SessionNavigationProvider;
        nav.capture(pid)?;
        let observation = nav.observe_until(pid, Instant::now() + Duration::from_secs(2))?;
        Ok(observe_sidebar_conversations(&observation, port))
    }

    #[derive(Clone, Debug, Default)]
    struct CodexAxNodeData {
        role: String,
        title: String,
        description: String,
        value: String,
        aria_current: Option<String>,
        children: Vec<Owned>,
    }

    #[derive(Clone, Debug)]
    struct CodexObservedNode {
        parent: Option<usize>,
        role: String,
        title: String,
        description: String,
        value: String,
        aria_current: Option<String>,
    }

    fn read_codex_ax_node(
        mut attributes: impl FnMut(&[&str]) -> Result<Vec<Option<Owned>>, String>,
    ) -> Result<Option<CodexAxNodeData>, String> {
        let mut base = attributes(&["AXRole", "AXHidden", "AXChildren"])?.into_iter();
        let role =
            text_attribute(base.next().flatten())?.ok_or_else(|| blocked("ax_role_unavailable"))?;
        let hidden = bool_attribute(base.next().flatten())?.unwrap_or(false);
        let raw_children = base.next().flatten();
        if hidden {
            return Ok(None);
        }
        let children = if role == "AXStaticText" {
            Vec::new()
        } else {
            raw_children
                .map(|value| value.array())
                .transpose()?
                .unwrap_or_default()
        };
        let names: &[&str] = match role.as_str() {
            "AXToolbar" | "AXList" => &["AXDescription", "AXTitle"],
            "AXImage" => &["AXDescription", "AXTitle", "AXValue"],
            "AXButton" => &["AXDescription", "AXTitle", "AXARIACurrent"],
            "AXStaticText" | "AXHeading" | "AXLink" => &["AXValue", "AXTitle", "AXDescription"],
            _ => &[],
        };
        let mut extra = if names.is_empty() {
            Vec::new()
        } else {
            attributes(names)?
        }
        .into_iter();
        let mut data = CodexAxNodeData {
            role: role.clone(),
            children,
            ..Default::default()
        };
        match role.as_str() {
            "AXToolbar" | "AXList" => {
                data.description =
                    optional_codex_text_attribute(extra.next().flatten()).unwrap_or_default();
                data.title =
                    optional_codex_text_attribute(extra.next().flatten()).unwrap_or_default();
            }
            "AXImage" => {
                data.description =
                    optional_codex_text_attribute(extra.next().flatten()).unwrap_or_default();
                data.title =
                    optional_codex_text_attribute(extra.next().flatten()).unwrap_or_default();
                data.value =
                    optional_codex_text_attribute(extra.next().flatten()).unwrap_or_default();
            }
            "AXButton" => {
                data.description =
                    optional_codex_text_attribute(extra.next().flatten()).unwrap_or_default();
                data.title =
                    optional_codex_text_attribute(extra.next().flatten()).unwrap_or_default();
                data.aria_current = optional_codex_text_attribute(extra.next().flatten());
            }
            "AXStaticText" | "AXHeading" | "AXLink" => {
                data.value =
                    optional_codex_text_attribute(extra.next().flatten()).unwrap_or_default();
                data.title =
                    optional_codex_text_attribute(extra.next().flatten()).unwrap_or_default();
                data.description =
                    optional_codex_text_attribute(extra.next().flatten()).unwrap_or_default();
            }
            _ => {}
        }
        Ok(Some(data))
    }

    fn collect_codex_nodes(
        root: Owned,
        deadline: Instant,
        mut read_node: impl FnMut(&Owned) -> Result<Option<CodexAxNodeData>, String>,
    ) -> Result<Vec<CodexObservedNode>, String> {
        loop {
            let mut nodes = Vec::new();
            let mut pending = vec![(root.clone(), None, 0usize)];
            let mut visited = 0usize;
            let mut retry_invalid = false;
            while let Some((element, parent, depth)) = pending.pop() {
                visited += 1;
                if visited > MAX_NODES || depth > MAX_DEPTH {
                    return Err(blocked("ax_tree_limit"));
                }
                if Instant::now() >= deadline {
                    return Err(blocked("ax_snapshot_timeout"));
                }
                let mut data = match read_node(&element) {
                    Ok(Some(data)) => data,
                    Ok(None) => continue,
                    Err(error) if invalid_ax_observation(&error) => {
                        retry_invalid = true;
                        break;
                    }
                    Err(error) => return Err(error),
                };
                if Instant::now() >= deadline {
                    return Err(blocked("ax_snapshot_timeout"));
                }
                if visited + pending.len() + data.children.len() > MAX_NODES {
                    return Err(blocked("ax_tree_limit"));
                }
                let index = nodes.len();
                for child in std::mem::take(&mut data.children).into_iter().rev() {
                    pending.push((child, Some(index), depth + 1));
                }
                nodes.push(CodexObservedNode {
                    parent,
                    role: data.role,
                    title: data.title,
                    description: data.description,
                    value: data.value,
                    aria_current: data.aria_current,
                });
            }
            if retry_invalid {
                let remaining = deadline
                    .checked_duration_since(Instant::now())
                    .ok_or_else(|| blocked("ax_snapshot_timeout"))?;
                std::thread::sleep(remaining.min(Duration::from_millis(10)));
                continue;
            }
            return Ok(nodes);
        }
    }

    fn codex_subtree_end(nodes: &[CodexObservedNode], root: usize) -> usize {
        let mut index = root + 1;
        while index < nodes.len() && nodes[index].parent.is_some_and(|parent| parent >= root) {
            index += 1;
        }
        index
    }

    fn codex_node_text(node: &CodexObservedNode) -> Option<String> {
        [&node.value, &node.title, &node.description]
            .into_iter()
            .map(|s| s.trim())
            .find(|s| !s.is_empty())
            .map(str::to_owned)
    }

    fn is_codex_attention_badge(raw: &str) -> bool {
        let normalized = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        if matches!(
            normalized.as_str(),
            "需要批准" | "等待批准" | "待审批" | "需要输入"
        ) {
            return true;
        }
        matches!(
            normalized.to_ascii_lowercase().as_str(),
            "needs approval" | "awaiting approval" | "needs input"
        )
    }

    fn is_signed_diff_count(token: &str) -> bool {
        token.starts_with(['+', '-'])
            && token.len() > 1
            && token[1..].bytes().all(|b| b.is_ascii_digit())
    }

    fn is_codex_sidebar_metadata_token(raw: &str) -> bool {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return true;
        }
        let collapsed = trimmed.split_whitespace().collect::<Vec<_>>().join(" ");
        if collapsed.eq_ignore_ascii_case("now") || collapsed.eq_ignore_ascii_case("just now") {
            return true;
        }
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if matches!(parts.as_slice(), [one] if is_signed_diff_count(one))
            || matches!(parts.as_slice(), [a, b] if is_signed_diff_count(a) && is_signed_diff_count(b))
        {
            return true;
        }
        let digit_len = trimmed.bytes().take_while(|b| b.is_ascii_digit()).count();
        if digit_len > 0 {
            let unit = trimmed[digit_len..].trim_start().to_ascii_lowercase();
            if matches!(
                unit.as_str(),
                "s" | "ss"
                    | "sec"
                    | "secs"
                    | "m"
                    | "ms"
                    | "min"
                    | "mins"
                    | "h"
                    | "hs"
                    | "hr"
                    | "hrs"
                    | "d"
                    | "ds"
                    | "w"
                    | "ws"
                    | "mo"
                    | "mos"
                    | "y"
                    | "ys"
            ) {
                return true;
            }
        }
        false
    }

    struct CodexParsedWindow {
        toolbar_project: Option<String>,
        active_sidebar_texts: Vec<String>,
        attention_sidebar_titles: Vec<String>,
        main_pane_texts: Vec<String>,
        saw_failed_to_resume: bool,
        saw_no_rollout: bool,
    }

    fn parse_codex_window_nodes(nodes: &[CodexObservedNode]) -> CodexParsedWindow {
        let mut in_toolbar = vec![false; nodes.len()];
        let mut in_web_area = vec![false; nodes.len()];
        let mut is_sidebar_list = vec![false; nodes.len()];
        let mut in_sidebar_list = vec![false; nodes.len()];
        let mut is_sidebar_row_button = vec![false; nodes.len()];
        let mut in_sidebar_button = vec![false; nodes.len()];
        let mut in_active_sidebar_button = vec![false; nodes.len()];

        for i in 0..nodes.len() {
            let parent_toolbar = nodes[i].parent.is_some_and(|p| in_toolbar[p]);
            let parent_web = nodes[i].parent.is_some_and(|p| in_web_area[p]);
            in_toolbar[i] = parent_toolbar
                || (nodes[i].role == "AXToolbar"
                    && (nodes[i].description.trim() == "Chat toolbar"
                        || nodes[i].title.trim() == "Chat toolbar"));
            in_web_area[i] = parent_web || nodes[i].role == "AXWebArea";
        }

        for i in 0..nodes.len() {
            if in_web_area[i] && nodes[i].role == "AXList" {
                let end = codex_subtree_end(nodes, i);
                let has_list_marker = nodes[(i + 1)..end]
                    .iter()
                    .any(|child| child.role == "AXListMarker");
                if !has_list_marker {
                    is_sidebar_list[i] = true;
                }
            }
        }

        for i in 0..nodes.len() {
            let parent_sidebar = nodes[i].parent.is_some_and(|p| in_sidebar_list[p]);
            in_sidebar_list[i] = parent_sidebar || is_sidebar_list[i];

            let parent_sidebar_btn = nodes[i].parent.is_some_and(|p| in_sidebar_button[p]);
            let row_btn = in_sidebar_list[i] && nodes[i].role == "AXButton" && !parent_sidebar_btn;
            is_sidebar_row_button[i] = row_btn;
            in_sidebar_button[i] = parent_sidebar_btn || row_btn;

            let parent_active_btn = nodes[i].parent.is_some_and(|p| in_active_sidebar_button[p]);
            let is_active_btn = in_sidebar_list[i]
                && nodes[i].role == "AXButton"
                && nodes[i]
                    .aria_current
                    .as_deref()
                    .is_some_and(|v| v.trim() == "page");
            in_active_sidebar_button[i] = parent_active_btn || is_active_btn;
        }

        let mut attention_sidebar_titles = Vec::new();
        for (i, &is_row) in is_sidebar_row_button.iter().enumerate() {
            if !is_row {
                continue;
            }
            let end = codex_subtree_end(nodes, i);
            let mut row_texts = Vec::new();
            for node in &nodes[i..end] {
                if !matches!(
                    node.role.as_str(),
                    "AXStaticText" | "AXButton" | "AXHeading" | "AXLink"
                ) {
                    continue;
                }
                for raw in [&node.value, &node.title, &node.description] {
                    let trimmed = raw.trim();
                    if !trimmed.is_empty() && !row_texts.iter().any(|t: &String| t == trimmed) {
                        row_texts.push(trimmed.to_owned());
                    }
                }
            }
            if row_texts.iter().any(|t| is_codex_attention_badge(t)) {
                for text in row_texts {
                    if !is_codex_attention_badge(&text)
                        && !is_codex_sidebar_metadata_token(&text)
                        && !attention_sidebar_titles.contains(&text)
                    {
                        attention_sidebar_titles.push(text);
                    }
                }
            }
        }

        let mut toolbar_project = None;
        let mut active_sidebar_texts = Vec::new();
        let mut main_pane_texts = Vec::new();
        let mut saw_failed_to_resume = false;
        let mut saw_no_rollout = false;

        for (i, node) in nodes.iter().enumerate() {
            if toolbar_project.is_none() && in_toolbar[i] && node.role == "AXImage" {
                for raw in [&node.description, &node.title, &node.value] {
                    if let Some(proj) = raw.trim().strip_prefix("Project:") {
                        let proj = proj.trim();
                        if !proj.is_empty() {
                            toolbar_project = Some(proj.to_owned());
                            break;
                        }
                    }
                }
            }

            if !in_web_area[i] {
                continue;
            }
            let Some(text) = codex_node_text(node) else {
                continue;
            };
            if text.contains("Failed to resume chat") {
                saw_failed_to_resume = true;
            }
            if text
                .to_ascii_lowercase()
                .contains("no rollout found for thread id")
            {
                saw_no_rollout = true;
            }
            if in_active_sidebar_button[i] {
                if matches!(
                    node.role.as_str(),
                    "AXStaticText" | "AXButton" | "AXHeading" | "AXLink"
                ) && !active_sidebar_texts.contains(&text)
                {
                    active_sidebar_texts.push(text);
                }
            } else if !in_sidebar_list[i]
                && matches!(node.role.as_str(), "AXStaticText" | "AXHeading" | "AXLink")
            {
                main_pane_texts.push(text);
            }
        }

        CodexParsedWindow {
            toolbar_project,
            active_sidebar_texts,
            attention_sidebar_titles,
            main_pane_texts,
            saw_failed_to_resume,
            saw_no_rollout,
        }
    }

    fn summarize_codex_window(
        window_title: String,
        nodes: &[CodexObservedNode],
    ) -> crate::codex_sessions::CodexWindowSnapshot {
        let parsed = parse_codex_window_nodes(nodes);
        let has_resume_error = parsed.saw_failed_to_resume
            && (window_title.trim() == "ChatGPT"
                || (parsed.saw_no_rollout && parsed.main_pane_texts.len() <= 6));

        crate::codex_sessions::CodexWindowSnapshot {
            window_title,
            toolbar_project: parsed.toolbar_project,
            active_sidebar_texts: parsed.active_sidebar_texts,
            main_pane_texts: parsed.main_pane_texts,
            has_resume_error,
        }
    }

    fn summarize_codex_live_window_signals_multi(
        is_frontmost: bool,
        primary_nodes: &[CodexObservedNode],
        extra_windows: &[Vec<CodexObservedNode>],
    ) -> crate::codex_sessions::CodexLiveWindowSignals {
        let primary = parse_codex_window_nodes(primary_nodes);
        let mut attention_sidebar_titles = primary.attention_sidebar_titles.clone();
        let mut attention_windows = vec![(
            primary.toolbar_project.clone(),
            primary.attention_sidebar_titles,
        )];
        for nodes in extra_windows {
            let parsed = parse_codex_window_nodes(nodes);
            for title in &parsed.attention_sidebar_titles {
                if !attention_sidebar_titles.contains(title) {
                    attention_sidebar_titles.push(title.clone());
                }
            }
            attention_windows.push((parsed.toolbar_project, parsed.attention_sidebar_titles));
        }
        crate::codex_sessions::CodexLiveWindowSignals {
            is_frontmost,
            toolbar_project: primary.toolbar_project,
            active_sidebar_texts: primary.active_sidebar_texts,
            attention_sidebar_titles,
            attention_windows,
        }
    }

    pub fn observe_codex_window_state() -> Result<crate::codex_sessions::CodexWindowSnapshot, String>
    {
        let window = CapturedWindow::capture("com.openai.codex", true)?;
        let deadline = Instant::now() + Duration::from_secs(2);
        let nodes = collect_codex_nodes(window.element.clone(), deadline, |element| {
            read_codex_ax_node(|names| element.attributes(names, deadline))
        })?;
        window.validate(None)?;
        Ok(summarize_codex_window(window.title, &nodes))
    }

    pub fn observe_codex_live_window_signals(
    ) -> Option<crate::codex_sessions::CodexLiveWindowSignals> {
        if !unsafe { AXIsProcessTrusted() } {
            return None;
        }
        let pid = crate::executor::find_pid_for_bundle_id("com.openai.codex")?;
        if pid <= 0 {
            return None;
        }
        let application = unsafe { Owned::from_create(AXUIElementCreateApplication(pid)) }?;
        let deadline = Instant::now() + Duration::from_millis(400);
        if unsafe { AXUIElementSetMessagingTimeout(application.0, 0.2) } != 0 {
            return None;
        }
        enable_application_accessibility(&application).ok()?;
        let mut attrs = application
            .attributes(&["AXFrontmost", "AXFocusedWindow", "AXWindows"], deadline)
            .ok()?
            .into_iter();
        let is_frontmost = bool_attribute(attrs.next().flatten())
            .ok()?
            .unwrap_or(false);
        let focused = attrs.next().flatten();
        let windows = attrs.next().flatten()?.array().ok()?;
        let mut visible = Vec::new();
        for element in windows {
            if Instant::now() >= deadline {
                return None;
            }
            let Ok(win_attrs) = element.attributes(&["AXRole", "AXMinimized"], deadline) else {
                continue;
            };
            let mut win_attrs = win_attrs.into_iter();
            let Ok(Some(role)) = text_attribute(win_attrs.next().flatten()) else {
                continue;
            };
            let minimized = bool_attribute(win_attrs.next().flatten())
                .ok()
                .flatten()
                .unwrap_or(false);
            if role == "AXWindow" && !minimized {
                visible.push(element);
            }
        }
        if visible.is_empty() {
            return None;
        }
        let index = focused
            .as_ref()
            .and_then(|focused| {
                visible
                    .iter()
                    .position(|window| unsafe { CFEqual(window.0, focused.0) } != 0)
            })
            .unwrap_or(0);
        let primary_window = visible.remove(index);
        let primary_nodes = collect_codex_nodes(primary_window, deadline, |element| {
            read_codex_ax_node(|names| element.attributes(names, deadline))
        })
        .ok()?;
        let mut extra_windows = Vec::new();
        for window in visible {
            if Instant::now() >= deadline {
                break;
            }
            let Ok(nodes) = collect_codex_nodes(window, deadline, |element| {
                read_codex_ax_node(|names| element.attributes(names, deadline))
            }) else {
                break;
            };
            extra_windows.push(nodes);
        }
        Some(summarize_codex_live_window_signals_multi(
            is_frontmost,
            &primary_nodes,
            &extra_windows,
        ))
    }

    fn find_sidebar_conversation_link(
        observation: &WindowObservation,
        port: u16,
        conversation: &str,
    ) -> Option<usize> {
        for (idx, node) in observation.nodes.iter().enumerate() {
            if node.data.role != "AXLink" || !node.data.enabled || !node.data.pressable {
                continue;
            }
            if node
                .data
                .url
                .as_deref()
                .and_then(|url| crate::session_navigation::conversation_id_from_url(url, port))
                != Some(conversation)
            {
                continue;
            }
            if classify_sidebar_row(observation, idx).is_some() {
                return Some(idx);
            }
        }
        None
    }

    fn find_collapsed_toggle_sidebar_button(observation: &WindowObservation) -> Option<usize> {
        let mut matches = observation
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(idx, node)| {
                let trimmed = node.data.label.trim();
                let is_collapsed_toggle =
                    (crate::antigravity_ax_labels::is_toggle_sidebar(trimmed)
                        && node.data.expanded == Some(false))
                        || (crate::antigravity_ax_labels::is_open_sidebar(trimmed)
                            && node.data.expanded != Some(true));
                (node.data.role == "AXButton"
                    && node.data.enabled
                    && node.data.pressable
                    && is_collapsed_toggle)
                    .then_some(idx)
            });
        let first = matches.next()?;
        matches.next().is_none().then_some(first)
    }

    fn project_header_button_for_link(
        observation: &WindowObservation,
        link_idx: usize,
        port: u16,
    ) -> Option<usize> {
        let nodes = &observation.nodes;
        if let Some(parent_idx) = nodes[link_idx].parent {
            let mut siblings = nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| {
                    n.parent == Some(parent_idx)
                        && n.data.role == "AXButton"
                        && n.data.enabled
                        && n.data.pressable
                        && !is_utility_sidebar_button_label(&n.data.label)
                })
                .map(|(idx, _)| idx);
            if let Some(btn_idx) = siblings.next() {
                if siblings.next().is_none() {
                    return Some(btn_idx);
                }
            }
        }
        let lower = link_idx.saturating_sub(6);
        for idx in (lower..link_idx).rev() {
            let node = &nodes[idx];
            if node.data.role == "AXLink"
                && node
                    .data
                    .url
                    .as_deref()
                    .and_then(|u| crate::session_navigation::conversation_id_from_url(u, port))
                    .is_some()
            {
                break;
            }
            if node.data.role == "AXButton"
                && node.data.enabled
                && node.data.pressable
                && !is_utility_sidebar_button_label(&node.data.label)
            {
                return Some(idx);
            }
        }
        None
    }

    fn locate_target_project_header(
        observation: &WindowObservation,
        port: u16,
        workspace_path: &str,
        project_id: Option<&str>,
        collapsed_fallback: bool,
    ) -> Option<usize> {
        if let Some(pid_str) = project_id.filter(|s| !s.is_empty()) {
            let mut saw_project_header = false;
            let mut matched = Vec::new();
            for (idx, node) in observation.nodes.iter().enumerate() {
                if node.data.role == "AXLink"
                    && node.data.url.as_deref().is_some_and(|url| {
                        crate::session_navigation::url_matches_project(url, port, pid_str)
                    })
                {
                    if let Some(btn_idx) = project_header_button_for_link(observation, idx, port) {
                        saw_project_header = true;
                        let expanded_ok = if collapsed_fallback {
                            observation.nodes[btn_idx].data.expanded == Some(false)
                        } else {
                            observation.nodes[btn_idx].data.expanded != Some(false)
                        };
                        if expanded_ok && !matched.contains(&btn_idx) {
                            matched.push(btn_idx);
                        }
                    }
                }
            }
            if matched.len() == 1 {
                return Some(matched[0]);
            }
            if saw_project_header || matched.len() > 1 {
                return None;
            }
        }
        let basename = std::path::Path::new(workspace_path)
            .file_name()
            .and_then(|s| s.to_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())?;
        let matches: Vec<_> = observation
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(idx, node)| {
                let expanded_ok = if collapsed_fallback {
                    node.data.expanded == Some(false)
                } else {
                    node.data.expanded != Some(false)
                };
                (node.data.role == "AXButton"
                    && node.data.enabled
                    && node.data.pressable
                    && expanded_ok
                    && !is_utility_sidebar_button_label(&node.data.label)
                    && node.data.label.trim() == basename)
                    .then_some(idx)
            })
            .collect();
        if matches.len() == 1 {
            return Some(matches[0]);
        }
        let sidebar_matches: Vec<_> = matches
            .into_iter()
            .filter(|&idx| {
                let Some(parent) = observation.nodes[idx].parent else {
                    return false;
                };
                let end = subtree_end(&observation.nodes, parent);
                observation.nodes[(idx + 1)..end]
                    .iter()
                    .take_while(|next| next.data.role != "AXButton")
                    .any(|next| {
                        next.parent == Some(parent)
                            && next.data.role == "AXLink"
                            && (crate::antigravity_ax_labels::contains_new_conversation(
                                &next.data.label,
                            ) || next.data.url.as_deref().is_some_and(|u| {
                                crate::session_navigation::app_document_path(u, port)
                                    .is_some_and(|p| p.contains("project="))
                            }))
                    })
            })
            .collect();
        (sidebar_matches.len() == 1).then(|| sidebar_matches[0])
    }

    fn is_section_or_project_header_button(
        observation: &WindowObservation,
        port: u16,
        idx: usize,
    ) -> bool {
        let node = &observation.nodes[idx];
        if node.data.role != "AXButton" {
            return false;
        }
        let trimmed = node.data.label.trim();
        if crate::antigravity_ax_labels::is_sidebar_section_header(trimmed) {
            return true;
        }
        if is_utility_sidebar_button_label(trimmed) {
            return false;
        }
        if node.data.expanded.is_some() {
            return true;
        }
        let upper = (idx + 5).min(observation.nodes.len());
        observation.nodes[(idx + 1)..upper].iter().any(|next| {
            matches!(next.data.role.as_str(), "AXLink" | "AXButton")
                && (crate::antigravity_ax_labels::contains_new_conversation(&next.data.label)
                    || next.data.url.as_deref().is_some_and(|u| {
                        crate::session_navigation::app_document_path(u, port)
                            .is_some_and(|p| p.contains("project="))
                    }))
        })
    }

    fn find_project_see_all_button(
        observation: &WindowObservation,
        port: u16,
        workspace_path: &str,
        project_id: Option<&str>,
    ) -> Option<usize> {
        if let Some(proj_btn_idx) =
            locate_target_project_header(observation, port, workspace_path, project_id, false)
        {
            if observation.nodes[proj_btn_idx].data.expanded == Some(false) {
                return None;
            }
            for idx in (proj_btn_idx + 1)..observation.nodes.len() {
                if is_section_or_project_header_button(observation, port, idx) {
                    break;
                }
                let node = &observation.nodes[idx];
                if node.data.role == "AXButton"
                    && node.data.enabled
                    && node.data.pressable
                    && crate::antigravity_ax_labels::is_see_all(&node.data.label)
                {
                    return Some(idx);
                }
            }
            return None;
        }
        let any_project_headers = (0..observation.nodes.len()).any(|idx| {
            let trimmed = observation.nodes[idx].data.label.trim();
            !crate::antigravity_ax_labels::is_sidebar_section_header(trimmed)
                && is_section_or_project_header_button(observation, port, idx)
        });
        if any_project_headers {
            return None;
        }
        let matches: Vec<_> = observation
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(idx, node)| {
                (node.data.role == "AXButton"
                    && node.data.enabled
                    && node.data.pressable
                    && crate::antigravity_ax_labels::is_see_all(&node.data.label))
                .then_some(idx)
            })
            .collect();
        (matches.len() == 1).then(|| matches[0])
    }

    fn verify_sidebar_link_with(
        window: &Owned,
        link: &Owned,
        port: u16,
        conversation: &str,
        deadline: Instant,
        mut read: impl FnMut(&Owned, &[&str], Instant) -> Result<Vec<Option<Owned>>, String>,
    ) -> Result<(), String> {
        let deadline = deadline.min(Instant::now() + Duration::from_millis(400));
        let mut attrs = read(
            link,
            &["AXWindow", "AXRole", "AXEnabled", "AXURL"],
            deadline,
        )?
        .into_iter();
        let attached = attrs
            .next()
            .flatten()
            .ok_or_else(|| blocked("session_sidebar_link_window_missing"))?;
        if unsafe { CFEqual(attached.0, window.0) } == 0 {
            return Err(blocked("session_sidebar_link_window_changed"));
        }
        if text_attribute(attrs.next().flatten())?.as_deref() != Some("AXLink") {
            return Err(blocked("session_sidebar_link_changed"));
        }
        if bool_attribute(attrs.next().flatten())? == Some(false) {
            return Err(blocked("session_sidebar_link_disabled"));
        }
        let live_url = attrs
            .next()
            .flatten()
            .map(lenient_url_attribute)
            .transpose()?;
        if live_url
            .as_deref()
            .and_then(|url| crate::session_navigation::conversation_id_from_url(url, port))
            != Some(conversation)
        {
            return Err(blocked("session_sidebar_link_url_changed"));
        }
        Ok(())
    }

    fn verify_sidebar_button_with(
        window: &Owned,
        button: &Owned,
        expected_label: &str,
        expected_expanded: Option<bool>,
        deadline: Instant,
        mut read: impl FnMut(&Owned, &[&str], Instant) -> Result<Vec<Option<Owned>>, String>,
    ) -> Result<(), String> {
        let deadline = deadline.min(Instant::now() + Duration::from_millis(400));
        let mut attrs = read(
            button,
            &[
                "AXWindow",
                "AXRole",
                "AXEnabled",
                "AXDescription",
                "AXTitle",
                "AXExpanded",
            ],
            deadline,
        )?
        .into_iter();
        let attached = attrs
            .next()
            .flatten()
            .ok_or_else(|| blocked("session_sidebar_button_window_missing"))?;
        if unsafe { CFEqual(attached.0, window.0) } == 0 {
            return Err(blocked("session_sidebar_button_window_changed"));
        }
        if text_attribute(attrs.next().flatten())?.as_deref() != Some("AXButton") {
            return Err(blocked("session_sidebar_button_changed"));
        }
        if bool_attribute(attrs.next().flatten())? == Some(false) {
            return Err(blocked("session_sidebar_button_disabled"));
        }
        let desc = lenient_text_attribute(attrs.next().flatten())?;
        let title = lenient_text_attribute(attrs.next().flatten())?;
        let label = desc
            .filter(|s| !s.trim().is_empty())
            .or(title)
            .unwrap_or_default();
        if label.trim() != expected_label.trim() {
            return Err(blocked("session_sidebar_button_label_changed"));
        }
        let expanded = bool_attribute(attrs.next().flatten())?;
        if expected_expanded.is_some_and(|exp| expanded != Some(exp)) {
            return Err(blocked("session_sidebar_button_state_changed"));
        }
        Ok(())
    }

    trait SidebarSelectionBackend {
        fn observe_until(&mut self, deadline: Instant) -> Result<WindowObservation, String>;
        fn verify_and_press_link(
            &mut self,
            observation: &WindowObservation,
            link_idx: usize,
            port: u16,
            conversation: &str,
            validate_lease: &mut dyn FnMut() -> Result<(), String>,
            is_current: &dyn Fn() -> bool,
        ) -> Result<(), String>;
        fn verify_and_press_button(
            &mut self,
            observation: &WindowObservation,
            button_idx: usize,
            expected_expanded: Option<bool>,
            validate_lease: &mut dyn FnMut() -> Result<(), String>,
            is_current: &dyn Fn() -> bool,
        ) -> Result<(), String>;
        fn wait_for_reobservation(&mut self, deadline: Instant) -> bool {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            std::thread::sleep(remaining.min(Duration::from_millis(25)));
            Instant::now() < deadline
        }
    }

    fn poll_for_sidebar_link(
        backend: &mut impl SidebarSelectionBackend,
        port: u16,
        conversation: &str,
        deadline: Instant,
        is_current: &dyn Fn() -> bool,
        latest: &mut WindowObservation,
    ) -> Result<Option<usize>, String> {
        loop {
            if !is_current() {
                return Err(blocked("session_navigation_retired"));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            match backend.observe_until(deadline) {
                Ok(obs) => {
                    *latest = obs;
                    if let Some(link_idx) =
                        find_sidebar_conversation_link(latest, port, conversation)
                    {
                        return Ok(Some(link_idx));
                    }
                }
                Err(error)
                    if error == "blocked:ax_snapshot_timeout" && Instant::now() >= deadline =>
                {
                    return Ok(None);
                }
                Err(error) => return Err(unconfirmed(error)),
            }
            if !is_current() {
                return Err(blocked("session_navigation_retired"));
            }
            if !backend.wait_for_reobservation(deadline) {
                return Ok(None);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn drive_sidebar_selection(
        backend: &mut impl SidebarSelectionBackend,
        target: &crate::session_navigation::SessionNavigationWindow,
        port: u16,
        workspace_path: &str,
        project_id: Option<&str>,
        conversation: &str,
        validate_lease: &mut dyn FnMut() -> Result<(), String>,
        is_current: &dyn Fn() -> bool,
        before_already_selected: impl FnOnce(
            &mut dyn FnMut() -> Result<(), String>,
            &dyn Fn() -> bool,
        ) -> Result<(), String>,
    ) -> Result<(), String> {
        if crate::session_navigation::selected_conversation(target, port).as_deref()
            == Ok(conversation)
        {
            return before_already_selected(validate_lease, is_current);
        }
        validate_lease()?;
        let mut observation = backend.observe_until(Instant::now() + Duration::from_secs(2))?;
        if let Some(link_idx) = find_sidebar_conversation_link(&observation, port, conversation) {
            return backend.verify_and_press_link(
                &observation,
                link_idx,
                port,
                conversation,
                validate_lease,
                is_current,
            );
        }

        if let Some(toggle_idx) = find_collapsed_toggle_sidebar_button(&observation) {
            let expected_expanded = observation.nodes[toggle_idx].data.expanded;
            backend.verify_and_press_button(
                &observation,
                toggle_idx,
                expected_expanded,
                validate_lease,
                is_current,
            )?;
            if let Some(link_idx) = poll_for_sidebar_link(
                backend,
                port,
                conversation,
                Instant::now() + Duration::from_millis(400),
                is_current,
                &mut observation,
            )? {
                return backend.verify_and_press_link(
                    &observation,
                    link_idx,
                    port,
                    conversation,
                    validate_lease,
                    is_current,
                );
            }
        }

        if let Some(proj_btn_idx) =
            locate_target_project_header(&observation, port, workspace_path, project_id, true)
        {
            if observation.nodes[proj_btn_idx].data.expanded == Some(false) {
                backend.verify_and_press_button(
                    &observation,
                    proj_btn_idx,
                    Some(false),
                    validate_lease,
                    is_current,
                )?;
                if let Some(link_idx) = poll_for_sidebar_link(
                    backend,
                    port,
                    conversation,
                    Instant::now() + Duration::from_millis(500),
                    is_current,
                    &mut observation,
                )? {
                    return backend.verify_and_press_link(
                        &observation,
                        link_idx,
                        port,
                        conversation,
                        validate_lease,
                        is_current,
                    );
                }
            }
        }

        if let Some(see_all_idx) =
            find_project_see_all_button(&observation, port, workspace_path, project_id)
        {
            backend.verify_and_press_button(
                &observation,
                see_all_idx,
                None,
                validate_lease,
                is_current,
            )?;
            if let Some(link_idx) = poll_for_sidebar_link(
                backend,
                port,
                conversation,
                Instant::now() + Duration::from_millis(500),
                is_current,
                &mut observation,
            )? {
                return backend.verify_and_press_link(
                    &observation,
                    link_idx,
                    port,
                    conversation,
                    validate_lease,
                    is_current,
                );
            }
        }

        Err("unconfirmed:session_sidebar_link_unavailable".into())
    }

    struct LiveSidebarBackend<'a> {
        nav: &'a NativeSessionNavigation,
        pid: i32,
    }

    impl SidebarSelectionBackend for LiveSidebarBackend<'_> {
        fn observe_until(&mut self, deadline: Instant) -> Result<WindowObservation, String> {
            self.nav.observe_until(self.pid, deadline)
        }

        fn verify_and_press_link(
            &mut self,
            observation: &WindowObservation,
            link_idx: usize,
            port: u16,
            conversation: &str,
            validate_lease: &mut dyn FnMut() -> Result<(), String>,
            is_current: &dyn Fn() -> bool,
        ) -> Result<(), String> {
            let window = self
                .nav
                .window
                .as_ref()
                .ok_or_else(|| blocked("session_window_missing"))?
                .element
                .clone();
            let link = observation.nodes[link_idx].element.clone();
            validate_lease()?;
            verify_sidebar_link_with(
                &window,
                &link,
                port,
                conversation,
                Instant::now() + Duration::from_millis(400),
                |element, names, deadline| element.attributes(names, deadline),
            )?;
            self.nav.before_mutation(is_current)?;
            link.action("AXPress")
        }

        fn verify_and_press_button(
            &mut self,
            observation: &WindowObservation,
            button_idx: usize,
            expected_expanded: Option<bool>,
            validate_lease: &mut dyn FnMut() -> Result<(), String>,
            is_current: &dyn Fn() -> bool,
        ) -> Result<(), String> {
            let window = self
                .nav
                .window
                .as_ref()
                .ok_or_else(|| blocked("session_window_missing"))?
                .element
                .clone();
            let node = &observation.nodes[button_idx];
            let button = node.element.clone();
            validate_lease()?;
            verify_sidebar_button_with(
                &window,
                &button,
                &node.data.label,
                expected_expanded,
                Instant::now() + Duration::from_millis(400),
                |element, names, deadline| element.attributes(names, deadline),
            )?;
            self.nav.before_mutation(is_current)?;
            button.action("AXPress")
        }
    }

    impl crate::session_navigation::SessionNavigationProvider for NativeSessionNavigation {
        fn capture(
            &mut self,
            pid: i32,
        ) -> Result<crate::session_navigation::SessionNavigationWindow, String> {
            let window = CapturedWindow::capture("com.google.antigravity", true)?;
            if window.pid != pid {
                return Err(blocked("session_process_changed"));
            }
            self.window = Some(window);
            self.inspect(pid)
        }
        fn read(
            &mut self,
            target: &crate::session_navigation::SessionNavigationWindow,
        ) -> Result<crate::session_navigation::SessionNavigationWindow, String> {
            self.inspect(target.pid)
        }
        fn select_conversation(
            &mut self,
            target: &crate::session_navigation::SessionNavigationWindow,
            conversation: &str,
            validate_lease: &mut dyn FnMut() -> Result<(), String>,
            is_current: &dyn Fn() -> bool,
        ) -> Result<(), String> {
            let mut backend = LiveSidebarBackend {
                nav: self,
                pid: target.pid,
            };
            drive_sidebar_selection(
                &mut backend,
                target,
                self.port,
                &self.workspace_path,
                self.project_id.as_deref(),
                conversation,
                validate_lease,
                is_current,
                |validate_lease, is_current| {
                    validate_lease()?;
                    self.before_mutation(is_current)
                },
            )
        }
    }

    struct FilteredObservation {
        controls: Vec<ManagedControl>,
        control_paths: Vec<(u64, Vec<u64>)>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum TargetValidationMode {
        StrictWithTitle,
        ContainerOnly,
    }

    #[derive(Debug, Clone)]
    struct Target {
        identity: ManagedWindowIdentity,
        window: CapturedWindow,
        scope: Owned,
    }

    #[derive(Debug, Default)]
    pub struct NativeAxProvider {
        target: Option<Target>,
        retained_targets: Vec<Target>,
        elements: Vec<Owned>,
        windows: Vec<(String, Owned, u64)>,
        next_window_id: u64,
        control_paths: Vec<(u64, Vec<u64>)>,
        next_scope_id: u64,
    }

    impl NativeAxProvider {
        fn register(&mut self, element: &Owned) -> Result<u64, String> {
            if let Some(index) = self
                .elements
                .iter()
                .position(|old| unsafe { CFEqual(old.0, element.0) } != 0)
            {
                return Ok(index as u64 + 1);
            }
            if self.elements.len() >= MAX_NODES * 2 {
                return Err(blocked("ax_identity_limit"));
            }
            self.elements.push(element.clone());
            Ok(self.elements.len() as u64)
        }
        fn element(&self, id: u64) -> Result<Owned, String> {
            id.checked_sub(1)
                .and_then(|index| self.elements.get(index as usize))
                .cloned()
                .ok_or_else(|| blocked("ax_control_identity_lost"))
        }
        fn target_for(&self, identity: &ManagedWindowIdentity) -> Option<&Target> {
            self.retained_targets
                .iter()
                .find(|target| target.identity == *identity)
                .or_else(|| {
                    self.target
                        .as_ref()
                        .filter(|target| target.identity == *identity)
                })
        }
        fn validate_with_mode(
            &self,
            identity: &ManagedWindowIdentity,
            mode: TargetValidationMode,
        ) -> Result<(Owned, Owned, String), String> {
            let target = self
                .target_for(identity)
                .ok_or_else(|| blocked("ax_target_missing"))?;
            if target.identity != *identity || target.window.pid != identity.pid {
                return Err(blocked("target_pid_mismatch"));
            }
            let live_title = target.window.validate_container()?;
            if mode == TargetValidationMode::StrictWithTitle && live_title != identity.title {
                return Err(blocked("target_window_changed"));
            }
            if !self.is_descendant(&target.scope, &target.window.element) {
                return Err(blocked("target_scope_changed"));
            }
            Ok((
                target.window.application.clone(),
                target.window.element.clone(),
                live_title,
            ))
        }
        fn validate(&self, identity: &ManagedWindowIdentity) -> Result<(Owned, Owned), String> {
            let (application, window, _) =
                self.validate_with_mode(identity, TargetValidationMode::StrictWithTitle)?;
            Ok((application, window))
        }
        fn is_descendant(&self, element: &Owned, ancestor: &Owned) -> bool {
            let deadline = Instant::now() + Duration::from_millis(300);
            let mut current = element.clone();
            for _ in 0..MAX_DEPTH {
                if unsafe { CFEqual(current.0, ancestor.0) } != 0 {
                    return true;
                }
                if Instant::now() >= deadline {
                    return false;
                }
                let Ok(Some(parent)) = current.attr("AXParent") else {
                    return false;
                };
                current = parent;
            }
            false
        }
        fn control_from_node_data(&self, id: u64, data: NodeData) -> Option<ManagedControl> {
            let bundle_id = self
                .target
                .as_ref()
                .map(|target| target.identity.bundle_id.as_str());
            Self::control_from_node_data_for_bundle(bundle_id, id, data)
        }
        fn control_from_node_data_for_bundle(
            bundle_id: Option<&str>,
            id: u64,
            data: NodeData,
        ) -> Option<ManagedControl> {
            let is_antigravity = bundle_id == Some("com.google.antigravity");
            let is_codex = bundle_id == Some("com.openai.codex");
            let is_dsh = bundle_id == Some("com.deepseek.dsh");
            let is_grokbot = bundle_id == Some("com.anysphere.sand");
            let antigravity_model_trigger = is_antigravity
                && matches!(data.role.as_str(), "AXPopUpButton" | "AXButton")
                && data.pressable
                && crate::managed_controls::is_antigravity_model_picker_trigger(&data.label);
            let codex_reasoning_trigger = is_codex
                && data.role == "AXPopUpButton"
                && data.pressable
                && crate::managed_controls::is_codex_reasoning_trigger(&data.label);
            let codex_model_trigger = is_codex
                && matches!(data.role.as_str(), "AXPopUpButton" | "AXButton")
                && data.pressable
                && crate::managed_controls::is_codex_model_picker_trigger(&data.label);
            let codex_popover_item = is_codex
                && data.role == "AXMenuItem"
                && crate::managed_controls::is_codex_popover_menu_item(&data.label);
            let dsh_model_trigger = is_dsh
                && matches!(data.role.as_str(), "AXPopUpButton" | "AXButton")
                && data.pressable
                && crate::managed_controls::is_dsh_model_picker_trigger(&data.label);
            let antigravity_subagent_trigger = is_antigravity
                && data.pressable
                && ((data.role == "AXPopUpButton"
                    && crate::managed_controls::is_antigravity_subagent_popup_trigger(
                        &data.label,
                    ))
                    || (data.role == "AXButton"
                        && (crate::managed_controls::is_antigravity_overview_subagents_button(
                            &data.label,
                        ) || crate::managed_controls::is_antigravity_open_subagent_button(
                            &data.label,
                        ))));
            let codex_subagent_trigger = is_codex
                && matches!(data.role.as_str(), "AXPopUpButton" | "AXButton")
                && data.pressable
                && crate::managed_controls::is_codex_subagent_picker_trigger(&data.label);
            let dsh_subagent_trigger = is_dsh
                && matches!(data.role.as_str(), "AXPopUpButton" | "AXButton")
                && data.pressable
                && crate::managed_controls::is_dsh_subagent_picker_trigger(&data.label);
            let subagent_trigger =
                antigravity_subagent_trigger || codex_subagent_trigger || dsh_subagent_trigger;

            let is_antigravity_side_panel_btn = is_antigravity
                && data.role == "AXButton"
                && crate::antigravity_ax_labels::is_side_panel_button(&data.label);
            let is_antigravity_browser_address_bar = is_antigravity
                && matches!(data.role.as_str(), "AXComboBox" | "AXTextField")
                && crate::antigravity_ax_labels::is_address_bar(&data.label);
            let is_antigravity_new_tab_search = is_antigravity
                && data.role == "AXTextField"
                && crate::antigravity_ax_labels::is_search_for_content_to_open(&data.label);

            let relevant_button = data.pressable
                && ([
                    crate::ax_dictation::DictationAdapter::Antigravity,
                    crate::ax_dictation::DictationAdapter::Dsh,
                    crate::ax_dictation::DictationAdapter::Codex,
                    crate::ax_dictation::DictationAdapter::GrokBot,
                ]
                .iter()
                .any(|adapter| {
                    adapter.matches_start_label(&data.label)
                        || adapter.matches_stop_label(&data.label)
                }) || crate::executor::is_antigravity_live_voice_start_label(&data.label)
                    || crate::executor::is_antigravity_live_voice_end_label(&data.label)
                    || crate::grokbot_ax_labels::is_voice_chat_start(&data.label)
                    || crate::grokbot_ax_labels::is_voice_chat_end(&data.label)
                    || crate::grokbot_ax_labels::is_send_message(&data.label)
                    || is_antigravity_side_panel_btn
                    || crate::antigravity_ax_labels::is_scroll_or_send_button(&data.label));
            let chat_heading = data.role == "AXHeading"
                && crate::antigravity_ax_labels::is_conversation_log_heading(&data.label)
                && is_antigravity;
            let codex = crate::ax_dictation::DictationAdapter::Codex;
            let codex_dictation = codex.matches_start_label(&data.label)
                || codex.matches_stop_label(&data.label)
                || codex.matches_finishing_label(&data.label);
            let grokbot = crate::ax_dictation::DictationAdapter::GrokBot;
            let grokbot_dictation = grokbot.matches_start_label(&data.label)
                || grokbot.matches_stop_label(&data.label)
                || grokbot.matches_finishing_label(&data.label);
            let grokbot_voice_chat = is_grokbot
                && data.role == "AXButton"
                && (crate::grokbot_ax_labels::is_voice_chat_start(&data.label)
                    || crate::grokbot_ax_labels::is_voice_chat_end(&data.label));
            let editable_text_input = data.editable && data.role != "AXComboBox";
            if !editable_text_input
                && !relevant_button
                && !codex_dictation
                && !grokbot_dictation
                && !grokbot_voice_chat
                && !is_antigravity_side_panel_btn
                && !is_antigravity_browser_address_bar
                && !is_antigravity_new_tab_search
                && !chat_heading
                && data.role != "AXScrollArea"
                && !antigravity_model_trigger
                && !codex_reasoning_trigger
                && !codex_model_trigger
                && !codex_popover_item
                && !dsh_model_trigger
                && !subagent_trigger
            {
                return None;
            }
            let pressable = if codex_popover_item || grokbot_voice_chat {
                data.pressable
            } else if antigravity_model_trigger
                || codex_reasoning_trigger
                || codex_model_trigger
                || dsh_model_trigger
                || subagent_trigger
            {
                true
            } else if is_antigravity_side_panel_btn {
                data.pressable
            } else {
                relevant_button
            };
            Some(ManagedControl {
                id,
                role: data.role,
                label: data.label,
                enabled: data.enabled,
                pressable,
                editable: data.editable,
                focused: data.focused,
                expanded: data.expanded,
            })
        }
        fn read_single_control_node(
            element: &Owned,
            deadline: Instant,
        ) -> Result<Option<NodeData>, String> {
            let mut attrs = element
                .attributes(&["AXRole", "AXSubrole", "AXHidden"], deadline)?
                .into_iter();
            let role = text_attribute(attrs.next().flatten())?
                .ok_or_else(|| blocked("ax_role_unavailable"))?;
            let subrole = text_attribute(attrs.next().flatten())?;
            let hidden = bool_attribute(attrs.next().flatten())?.unwrap_or(false);
            if hidden || role == "AXStaticText" {
                return Ok(None);
            }
            let input = matches!(
                role.as_str(),
                "AXTextArea" | "AXTextField" | "AXSecureTextField"
            );
            let labeled_input = matches!(role.as_str(), "AXTextField" | "AXComboBox");
            let button = matches!(
                role.as_str(),
                "AXButton"
                    | "AXCheckBox"
                    | "AXRadioButton"
                    | "AXSwitch"
                    | "AXToggleButton"
                    | "AXLink"
                    | "AXPopUpButton"
                    | "AXMenuItem"
            );
            let mut data = NodeData {
                role,
                enabled: true,
                ..Default::default()
            };
            if labeled_input {
                let mut attrs = element
                    .attributes(
                        &["AXDescription", "AXTitle", "AXEnabled", "AXFocused"],
                        deadline,
                    )?
                    .into_iter();
                data.label = text_attribute(attrs.next().flatten())?
                    .filter(|s| !s.trim().is_empty())
                    .or(text_attribute(attrs.next().flatten())?)
                    .unwrap_or_default();
                data.enabled = bool_attribute(attrs.next().flatten())?.unwrap_or(true);
                data.focused = bool_attribute(attrs.next().flatten())?.unwrap_or(false);
                data.editable = subrole.as_deref() != Some("AXSecureTextField")
                    && element.settable("AXValue")?;
            } else if input {
                let mut attrs = element
                    .attributes(&["AXEnabled", "AXFocused"], deadline)?
                    .into_iter();
                data.enabled = bool_attribute(attrs.next().flatten())?.unwrap_or(true);
                data.focused = bool_attribute(attrs.next().flatten())?.unwrap_or(false);
                data.editable = data.role != "AXSecureTextField"
                    && subrole.as_deref() != Some("AXSecureTextField")
                    && element.settable("AXValue")?;
            } else if button || data.role == "AXHeading" {
                let names: &[&str] = if data.role == "AXPopUpButton" {
                    &["AXDescription", "AXTitle", "AXEnabled", "AXExpanded"]
                } else {
                    &["AXDescription", "AXTitle", "AXEnabled"]
                };
                let mut attrs = element.attributes(names, deadline)?.into_iter();
                data.label = text_attribute(attrs.next().flatten())?
                    .filter(|s| !s.trim().is_empty())
                    .or(text_attribute(attrs.next().flatten())?)
                    .unwrap_or_default();
                data.enabled = bool_attribute(attrs.next().flatten())?.unwrap_or(true);
                if data.role == "AXPopUpButton" {
                    data.expanded = bool_attribute(attrs.next().flatten())?;
                } else if data.role == "AXButton"
                    && crate::antigravity_ax_labels::is_toggle_side_panel(&data.label)
                {
                    if let Ok(vals) = element.attributes(&["AXExpanded"], deadline) {
                        if let Some(val) = vals.into_iter().next().flatten() {
                            data.expanded = bool_attribute(Some(val)).ok().flatten();
                        }
                    }
                }
                data.pressable =
                    button && element.actions()?.iter().any(|action| action == "AXPress");
            } else if data.role == "AXGroup" && subrole.as_deref() == Some("AXApplicationStatus") {
                let mut attrs = element
                    .attributes(&["AXDescription", "AXTitle", "AXEnabled"], deadline)?
                    .into_iter();
                data.label = text_attribute(attrs.next().flatten())?
                    .filter(|s| !s.trim().is_empty())
                    .or(text_attribute(attrs.next().flatten())?)
                    .unwrap_or_default();
                data.enabled = bool_attribute(attrs.next().flatten())?.unwrap_or(true);
            }
            Ok(Some(data))
        }
        fn ancestor_matches_container(
            observation: &WindowObservation,
            node_by_id: &std::collections::HashMap<u64, usize>,
            cache: &mut std::collections::HashMap<u64, bool>,
            ancestor_id: u64,
            allowed_roles: &[&str],
            matches_role_and_label: impl Fn(&str, &str) -> bool,
        ) -> bool {
            if let Some(&cached) = cache.get(&ancestor_id) {
                return cached;
            }
            let matched = match node_by_id
                .get(&ancestor_id)
                .map(|&idx| &observation.nodes[idx])
            {
                Some(node) if allowed_roles.contains(&node.data.role.as_str()) => {
                    let role = node.data.role.as_str();
                    let label = node.data.label.trim();
                    if !label.is_empty() {
                        matches_role_and_label(role, label)
                    } else {
                        let deadline = Instant::now() + Duration::from_millis(100);
                        if let Ok(attrs) = node
                            .element
                            .attributes(&["AXDescription", "AXTitle"], deadline)
                        {
                            let mut iter = attrs.into_iter();
                            let desc = iter.next().flatten().and_then(|v| v.string());
                            let title = iter.next().flatten().and_then(|v| v.string());
                            desc.as_deref()
                                .map(str::trim)
                                .is_some_and(|label| matches_role_and_label(role, label))
                                || title
                                    .as_deref()
                                    .map(str::trim)
                                    .is_some_and(|label| matches_role_and_label(role, label))
                        } else {
                            false
                        }
                    }
                }
                _ => false,
            };
            cache.insert(ancestor_id, matched);
            matched
        }
        fn is_excluded_browser_container(role: &str, label: &str) -> bool {
            (matches!(role, "AXGroup" | "AXSheet" | "AXDialog")
                && crate::antigravity_ax_labels::is_settings_container(label))
                || (role == "AXGroup"
                    && crate::antigravity_ax_labels::is_markdown_view_mode_container(label))
                || (matches!(role, "AXGroup" | "AXScrollArea" | "AXList")
                    && crate::antigravity_ax_labels::is_chat_transcript_region(label))
        }
        fn is_browser_filter_candidate(control: &ManagedControl) -> bool {
            (control.role == "AXButton"
                && crate::antigravity_ax_labels::is_browser_filter_button(&control.label))
                || (matches!(control.role.as_str(), "AXComboBox" | "AXTextField")
                    && crate::antigravity_ax_labels::is_address_bar(&control.label))
        }
        fn filter_controls_from_observation(
            observation: &WindowObservation,
            bundle_id: Option<&str>,
            mut assign_id: impl FnMut(usize, &Owned) -> Result<u64, String>,
        ) -> Result<FilteredObservation, String> {
            let mut controls = Vec::new();
            let mut paths: Vec<Vec<u64>> = Vec::new();
            let mut control_paths: Vec<(u64, Vec<u64>)> = Vec::new();
            let mut node_by_id = std::collections::HashMap::new();
            for (node_idx, node) in observation.nodes.iter().enumerate() {
                let id = assign_id(node_idx, &node.element)?;
                node_by_id.insert(id, node_idx);
                let mut path = node
                    .parent
                    .map(|index| paths[index].clone())
                    .unwrap_or_default();
                path.push(id);
                paths.push(path.clone());
                let Some(control) =
                    Self::control_from_node_data_for_bundle(bundle_id, id, node.data.clone())
                else {
                    continue;
                };
                control_paths.push((id, path));
                controls.push(control);
            }
            let is_antigravity = bundle_id == Some("com.google.antigravity");
            if is_antigravity {
                let multi_text_areas = controls
                    .iter()
                    .filter(|c| {
                        c.enabled
                            && c.editable
                            && matches!(c.role.as_str(), "AXTextArea" | "AXTextField")
                    })
                    .count()
                    > 1;
                let toggle_expanded = if let Some(toggle_idx) = controls
                    .iter()
                    .position(|c| crate::antigravity_ax_labels::is_toggle_side_panel(&c.label))
                {
                    let id = controls[toggle_idx].id;
                    let expanded = node_by_id.get(&id).and_then(|&idx| {
                        observation.nodes[idx].data.expanded.or_else(|| {
                            let elem = &observation.nodes[idx].element;
                            let deadline = Instant::now() + Duration::from_millis(100);
                            let val = elem
                                .attributes(&["AXExpanded"], deadline)
                                .ok()?
                                .into_iter()
                                .next()
                                .flatten();
                            bool_attribute(val).ok().flatten()
                        })
                    });
                    controls[toggle_idx].expanded = expanded;
                    expanded
                } else {
                    None
                };
                let side_panel_expanded = multi_text_areas || toggle_expanded == Some(true);

                if side_panel_expanded {
                    let mut side_question_ancestors: std::collections::HashMap<u64, bool> =
                        std::collections::HashMap::new();
                    for control in &mut controls {
                        if matches!(control.role.as_str(), "AXTextArea" | "AXTextField")
                            && control.editable
                        {
                            if let Some((_, path)) =
                                control_paths.iter().find(|(cid, _)| *cid == control.id)
                            {
                                for &ancestor_id in path.iter().rev().skip(1) {
                                    if Self::ancestor_matches_container(
                                        observation,
                                        &node_by_id,
                                        &mut side_question_ancestors,
                                        ancestor_id,
                                        &["AXGroup"],
                                        |_, label| {
                                            crate::antigravity_ax_labels::is_side_question(label)
                                        },
                                    ) {
                                        control.label = crate::antigravity_ax_labels::SIDE_QUESTION_INPUT_SYNTHETIC_LABEL.to_string();
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }

                if controls.iter().any(Self::is_browser_filter_candidate) {
                    let new_tab_search_info = controls
                        .iter()
                        .find(|c| {
                            c.role == "AXTextField"
                                && crate::antigravity_ax_labels::is_search_for_content_to_open(
                                    &c.label,
                                )
                        })
                        .and_then(|c| {
                            let search_idx = *node_by_id.get(&c.id)?;
                            let search_parent =
                                control_paths.iter().find(|(cid, _)| *cid == c.id).and_then(
                                    |(_, path)| path.len().checked_sub(2).map(|idx| path[idx]),
                                );
                            Some((search_idx, search_parent))
                        });
                    let chat_transcript_containers: std::collections::HashSet<usize> = observation
                        .nodes
                        .iter()
                        .filter_map(|node| {
                            if node.data.role == "AXHeading"
                                && crate::antigravity_ax_labels::is_chat_transcript_heading(
                                    &node.data.label,
                                )
                            {
                                node.parent.filter(|&parent_idx| {
                                    matches!(
                                        observation.nodes[parent_idx].data.role.as_str(),
                                        "AXGroup" | "AXScrollArea" | "AXList"
                                    )
                                })
                            } else {
                                None
                            }
                        })
                        .collect();
                    let mut excluded_ancestors: std::collections::HashMap<u64, bool> =
                        std::collections::HashMap::new();
                    let mut retained_ids = std::collections::HashSet::with_capacity(controls.len());
                    controls.retain(|c| {
                        let keep = if Self::is_browser_filter_candidate(c) {
                            let path_opt = control_paths
                                .iter()
                                .find(|(cid, _)| *cid == c.id)
                                .map(|(_, p)| p);
                            let in_excluded_container = path_opt.is_some_and(|path| {
                                path.iter().rev().skip(1).any(|&ancestor_id| {
                                    node_by_id
                                        .get(&ancestor_id)
                                        .is_some_and(|idx| chat_transcript_containers.contains(idx))
                                        || Self::ancestor_matches_container(
                                            observation,
                                            &node_by_id,
                                            &mut excluded_ancestors,
                                            ancestor_id,
                                            &[
                                                "AXGroup",
                                                "AXSheet",
                                                "AXDialog",
                                                "AXScrollArea",
                                                "AXList",
                                            ],
                                            Self::is_excluded_browser_container,
                                        )
                                })
                            });
                            if in_excluded_container {
                                false
                            } else if c.role == "AXButton" {
                                if crate::antigravity_ax_labels::is_browser(&c.label)
                                    || crate::antigravity_ax_labels::is_open_in_external_browser(
                                        &c.label,
                                    )
                                {
                                    true
                                } else if crate::antigravity_ax_labels::is_preview(&c.label) {
                                    new_tab_search_info.is_some_and(|(search_idx, _)| {
                                        node_by_id.get(&c.id).is_some_and(|&idx| idx > search_idx)
                                    })
                                } else if crate::antigravity_ax_labels::is_clear_search(&c.label) {
                                    new_tab_search_info.is_some_and(
                                        |(search_idx, search_parent)| {
                                            let Some(&idx) = node_by_id.get(&c.id) else {
                                                return false;
                                            };
                                            if idx <= search_idx {
                                                return false;
                                            }
                                            if idx - search_idx <= 6 {
                                                return true;
                                            }
                                            let parent_id = path_opt.and_then(|path| {
                                                path.len().checked_sub(2).map(|p| path[p])
                                            });
                                            parent_id.is_some()
                                                && parent_id == search_parent
                                                && parent_id
                                                    .and_then(|pid| node_by_id.get(&pid))
                                                    .is_some_and(|&pidx| {
                                                        observation.nodes[pidx].data.role
                                                            == "AXGroup"
                                                    })
                                        },
                                    )
                                } else {
                                    true
                                }
                            } else {
                                true
                            }
                        } else {
                            true
                        };
                        if keep {
                            retained_ids.insert(c.id);
                        }
                        keep
                    });
                    control_paths.retain(|(cid, _)| retained_ids.contains(cid));
                }
            }
            Ok(FilteredObservation {
                controls,
                control_paths,
            })
        }
        fn controls_from_observation_for_bundle(
            observation: &WindowObservation,
            bundle_id: &str,
        ) -> Vec<ManagedControl> {
            Self::filter_controls_from_observation(observation, Some(bundle_id), |node_idx, _| {
                Ok(node_idx as u64 + 1)
            })
            .expect("synthetic node index IDs never fail")
            .controls
        }
        fn controls_from_observation(
            &mut self,
            observation: WindowObservation,
        ) -> Result<Vec<ManagedControl>, String> {
            let bundle_id = self
                .target
                .as_ref()
                .map(|target| target.identity.bundle_id.clone());
            self.control_paths.clear();
            let filtered = Self::filter_controls_from_observation(
                &observation,
                bundle_id.as_deref(),
                |_, element| self.register(element),
            )?;
            self.control_paths = filtered.control_paths;
            Ok(filtered.controls)
        }
        fn checked_control(
            &mut self,
            target: &ManagedWindowIdentity,
            id: u64,
        ) -> Result<(ManagedControl, Owned), String> {
            let window = self.read(target)?;
            let control = window
                .controls
                .into_iter()
                .find(|control| control.id == id)
                .ok_or_else(|| blocked("ax_control_removed"))?;
            let element = self.element(id)?;
            if element.pid() != Some(target.pid) {
                return Err(blocked("control_pid_mismatch"));
            }
            Ok((control, element))
        }
        fn checked_exact_control(
            &self,
            target: &ManagedWindowIdentity,
            id: u64,
        ) -> Result<(ManagedControl, Owned), String> {
            let deadline = Instant::now() + Duration::from_millis(300);
            let current = self
                .target
                .as_ref()
                .filter(|current| current.identity == *target)
                .ok_or_else(|| blocked("target_identity_changed"))?;
            let (_, window) = self.validate(target)?;
            let element = self.element(id)?;
            let (_, path) = self
                .control_paths
                .iter()
                .find(|(control, path)| *control == id && path.last() == Some(&id))
                .ok_or_else(|| blocked("ax_control_path_missing"))?;
            let path = path
                .iter()
                .map(|id| self.element(*id))
                .collect::<Result<Vec<_>, _>>()?;
            let same = |left: &Owned, right: &Owned| unsafe { CFEqual(left.0, right.0) } != 0;
            validate_control_membership(
                &path,
                &current.scope,
                &window,
                target.pid,
                deadline,
                same,
                |node| {
                    let names: &[&str] = if same(node, &element) {
                        &["AXRole", "AXHidden", "AXWindow"]
                    } else {
                        &["AXRole", "AXHidden", "AXWindow", "AXChildren"]
                    };
                    let mut attrs = node.attributes(names, deadline)?.into_iter();
                    let role = text_attribute(attrs.next().flatten())?;
                    let hidden = bool_attribute(attrs.next().flatten())?.unwrap_or(false);
                    let window = attrs.next().flatten();
                    let children = if same(node, &element) {
                        Vec::new()
                    } else {
                        attrs
                            .next()
                            .flatten()
                            .ok_or_else(|| blocked("ax_control_removed"))?
                            .array()?
                    };
                    Ok(ControlNode {
                        pid: node.pid(),
                        role,
                        hidden,
                        window,
                        children,
                    })
                },
            )?;
            let node_data = Self::read_single_control_node(&element, deadline)?
                .ok_or_else(|| blocked("ax_control_removed"))?;
            let metadata = self
                .control_from_node_data(id, node_data)
                .ok_or_else(|| blocked("ax_control_removed"))?;
            self.validate(target)?;
            if Instant::now() >= deadline {
                return Err(blocked("ax_control_validation_timeout"));
            }
            Ok((metadata, element))
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ScopePolicy {
        RequireFocusedPane,
        AllowFallback,
        FullWindow,
    }

    impl NativeAxProvider {
        fn scope_identity(
            &mut self,
            pid: i32,
            window_id: u64,
            scope: &Owned,
            previous: Option<&Target>,
        ) -> Result<u64, String> {
            if let Some(target) = self.retained_targets.iter().chain(previous).find(|target| {
                target.identity.pid == pid
                    && target.identity.window_id == window_id
                    && unsafe { CFEqual(target.scope.0, scope.0) } != 0
            }) {
                return Ok(target.identity.scope_id);
            }
            self.next_scope_id = self
                .next_scope_id
                .checked_add(1)
                .ok_or_else(|| blocked("ax_scope_identity_exhausted"))?;
            Ok(self.next_scope_id)
        }

        fn capture_window_with_scope_policy(
            &mut self,
            bundle_id: &str,
            policy: ScopePolicy,
        ) -> Result<ManagedWindow, String> {
            let captured = CapturedWindow::capture(bundle_id, bundle_id == "com.openai.codex")?;
            let pid = captured.pid;
            let window = captured.element.clone();
            self.elements.clear();
            let window_id = if let Some((_, _, id)) =
                self.windows.iter().find(|(bundle, old, _)| {
                    bundle == bundle_id && unsafe { CFEqual(old.0, window.0) } != 0
                }) {
                *id
            } else {
                self.next_window_id += 1;
                self.windows.retain(|(bundle, _, _)| bundle != bundle_id);
                self.windows
                    .push((bundle_id.to_owned(), window.clone(), self.next_window_id));
                self.next_window_id
            };
            let identity = ManagedWindowIdentity {
                bundle_id: bundle_id.to_owned(),
                pid,
                window_id,
                scope_id: 0,
                title: captured.title.clone(),
            };
            let previous_target = self.target.clone();
            self.target = Some(Target {
                identity: identity.clone(),
                window: captured,
                scope: window.clone(),
            });
            let snapshot = self.read(&identity)?;
            if policy == ScopePolicy::FullWindow {
                return Ok(snapshot);
            }
            let require_focused_pane = policy == ScopePolicy::RequireFocusedPane;
            let input_controls: Vec<_> = snapshot
                .controls
                .iter()
                .filter(|control| {
                    control.enabled && control.editable && control.role == "AXTextArea"
                })
                .filter_map(|control| {
                    self.control_paths
                        .iter()
                        .find(|(id, _)| *id == control.id)
                        .map(|(_, path)| (control, path.clone()))
                })
                .collect();
            let input_paths: Vec<_> = input_controls
                .iter()
                .map(|(_, path)| path.clone())
                .collect();
            let mut focus_path = Vec::new();
            if input_paths.len() > 1 {
                let mut focused = self
                    .target
                    .as_ref()
                    .unwrap()
                    .window
                    .application
                    .attr("AXFocusedUIElement")?;
                let deadline = Instant::now() + Duration::from_millis(300);
                for _ in 0..MAX_DEPTH {
                    let Some(element) = focused else {
                        break;
                    };
                    if Instant::now() >= deadline {
                        break;
                    }
                    focus_path.push(self.register(&element)?);
                    if unsafe { CFEqual(element.0, window.0) } != 0 {
                        break;
                    }
                    focused = element.attr("AXParent")?;
                }
                focus_path.reverse();
            }
            let scope_selection =
                match crate::managed_controls::select_input_scope(&input_paths, &focus_path) {
                    Ok(selection) => Ok(selection),
                    Err(err) => {
                        let primary_paths: Vec<_> = input_controls
                            .iter()
                            .filter(|(control, _)| {
                                !crate::managed_controls::is_side_question_input(control)
                            })
                            .map(|(_, path)| path.clone())
                            .collect();
                        if primary_paths.len() < input_paths.len() {
                            if primary_paths.len() == 1 {
                                return Ok(snapshot);
                            } else if primary_paths.len() > 1 {
                                crate::managed_controls::select_input_scope(
                                    &primary_paths,
                                    &focus_path,
                                )
                            } else {
                                Err(err)
                            }
                        } else {
                            Err(err)
                        }
                    }
                };
            let scope_element_id = match scope_selection {
                Ok(Some(id)) => id,
                Ok(None) => return Ok(snapshot),
                Err(err) if require_focused_pane => return Err(err),
                Err(_) => return Ok(snapshot),
            };
            let scope = self.element(scope_element_id)?;
            let scope_id = self.scope_identity(pid, window_id, &scope, previous_target.as_ref())?;
            let mut identity = identity;
            identity.scope_id = scope_id;
            let target = self.target.as_mut().unwrap();
            target.identity = identity.clone();
            target.scope = scope;
            let controls: Vec<_> = snapshot
                .controls
                .into_iter()
                .filter(|control| {
                    self.control_paths
                        .iter()
                        .any(|(id, path)| *id == control.id && path.contains(&scope_element_id))
                })
                .collect();
            Ok(ManagedWindow { identity, controls })
        }

        fn hit_is_passive_target(
            &self,
            app: &Owned,
            window: &Owned,
            pid: i32,
            point: Point,
            deadline: Instant,
        ) -> bool {
            let mut hit = std::ptr::null();
            if unsafe {
                AXUIElementCopyElementAtPosition(app.0, point.x as f32, point.y as f32, &mut hit)
            } != 0
            {
                return false;
            }
            let Some(mut hit) = (unsafe { Owned::from_create(hit) }) else {
                return false;
            };
            for depth in 0..MAX_DEPTH {
                if Instant::now() >= deadline || hit.pid() != Some(pid) {
                    return false;
                }
                if unsafe { CFEqual(hit.0, window.0) } != 0 {
                    return depth > 0;
                }
                let Ok(Some(role)) = hit.string_attr("AXRole") else {
                    return false;
                };
                if matches!(
                    role.as_str(),
                    "AXButton"
                        | "AXCheckBox"
                        | "AXRadioButton"
                        | "AXSwitch"
                        | "AXToggleButton"
                        | "AXPopUpButton"
                        | "AXMenuButton"
                        | "AXMenuItem"
                        | "AXMenuBar"
                        | "AXMenu"
                        | "AXToolbar"
                        | "AXTextField"
                        | "AXTextArea"
                        | "AXComboBox"
                        | "AXLink"
                        | "AXSlider"
                        | "AXTabGroup"
                        | "AXApplication"
                ) {
                    return false;
                }
                let Ok(Some(parent)) = hit.attr("AXParent") else {
                    return false;
                };
                hit = parent;
            }
            false
        }

        fn scroll_with_targeted_event(
            &mut self,
            target: &ManagedWindowIdentity,
            input: Rect,
            controls: &[ManagedControl],
            request: &DialScrollRequest,
        ) -> Result<(), String> {
            if request.sidebar || request.jump_to_bottom {
                return Err(blocked("target_scrollbar_or_bottom_control_unavailable"));
            }
            let (app, window) = self.validate(target)?;
            let bounds = window
                .rect()?
                .ok_or_else(|| blocked("window_geometry_unavailable"))?;
            let headings: Vec<_> = controls
                .iter()
                .filter(|control| {
                    control.role == "AXHeading"
                        && crate::antigravity_ax_labels::is_conversation_log_heading(&control.label)
                })
                .collect();
            let preferred = if let [heading] = headings.as_slice() {
                self.element(heading.id)?
                    .attr("AXParent")?
                    .map(|parent| parent.rect())
                    .transpose()?
                    .flatten()
            } else {
                None
            };
            let deadline = Instant::now() + Duration::from_millis(400);
            let point = scroll_candidate_points(bounds, input, preferred)
                .into_iter()
                .find(|point| {
                    self.hit_is_passive_target(&app, &window, target.pid, *point, deadline)
                })
                .ok_or_else(|| blocked("passive_chat_region_unavailable"))?;
            let delta = request.clamped_delta_lines();
            if delta == 0 {
                return Ok(());
            }
            let event = unsafe {
                Owned::from_create(CGEventCreateScrollWheelEvent2(
                    std::ptr::null_mut(),
                    1,
                    1,
                    -delta,
                    0,
                    0,
                ))
            }
            .ok_or_else(|| blocked("scroll_event_unavailable"))?;
            unsafe {
                CGEventSetLocation(event.0.cast_mut(), point);
                CGEventSetFlags(event.0.cast_mut(), 0);
            }
            self.validate(target)?;
            if !self.hit_is_passive_target(
                &app,
                &window,
                target.pid,
                point,
                Instant::now() + Duration::from_millis(200),
            ) {
                return Err(blocked("scroll_target_changed"));
            }
            unsafe { CGEventPostToPid(target.pid, event.0.cast_mut()) };
            Err("unconfirmed:scroll_dispatched".into())
        }
    }

    impl ManagedAxProvider for NativeAxProvider {
        fn retain_target(&mut self, identity: &ManagedWindowIdentity) -> Result<(), String> {
            if self
                .retained_targets
                .iter()
                .any(|target| target.identity == *identity)
            {
                return Ok(());
            }
            let target = self
                .target_for(identity)
                .cloned()
                .ok_or_else(|| blocked("ax_target_missing"))?;
            self.retained_targets.push(target);
            Ok(())
        }
        fn release_target(&mut self, identity: &ManagedWindowIdentity) {
            self.retained_targets
                .retain(|target| target.identity != *identity);
        }
        fn activate_window(&mut self, target: &ManagedWindowIdentity) -> Result<(), String> {
            let (application, window) = self.validate(target)?;
            if application.bool_attr("AXFrontmost")? != Some(true) {
                application.set("AXFrontmost", unsafe { kCFBooleanTrue })?;
            }
            let _ = window.action("AXRaise");
            Ok(())
        }

        fn process_has_exited(&self, pid: i32) -> bool {
            pid > 0
                && unsafe { libc::kill(pid, 0) } == -1
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        }
        fn capture(&mut self, bundle_id: &str) -> Result<ManagedWindow, String> {
            self.capture_window_with_scope_policy(bundle_id, ScopePolicy::RequireFocusedPane)
        }
        fn capture_full_window(&mut self, bundle_id: &str) -> Result<ManagedWindow, String> {
            self.capture_window_with_scope_policy(bundle_id, ScopePolicy::FullWindow)
        }
        fn capture_for_scroll(&mut self, bundle_id: &str) -> Result<ManagedWindow, String> {
            self.capture_window_with_scope_policy(bundle_id, ScopePolicy::RequireFocusedPane)
        }
        fn capture_for_window_key(&mut self, bundle_id: &str) -> Result<ManagedWindow, String> {
            self.capture_window_with_scope_policy(bundle_id, ScopePolicy::AllowFallback)
        }
        fn read(&mut self, target: &ManagedWindowIdentity) -> Result<ManagedWindow, String> {
            self.validate(target)?;
            if self
                .target
                .as_ref()
                .is_none_or(|current| current.identity != *target)
            {
                self.target = self.target_for(target).cloned();
                self.elements.clear();
            }
            let scope = self
                .target_for(target)
                .ok_or_else(|| blocked("ax_target_missing"))?
                .scope
                .clone();
            let purpose = if target.bundle_id == "com.google.antigravity" {
                ObservationPurpose::AntigravityControls
            } else {
                ObservationPurpose::Controls
            };
            let observation = observe_window(scope, purpose)?;
            let controls = self.controls_from_observation(observation)?;
            self.validate(target)?;
            Ok(ManagedWindow {
                identity: target.clone(),
                controls,
            })
        }
        fn read_container(
            &mut self,
            target: &ManagedWindowIdentity,
        ) -> Result<ManagedWindow, String> {
            let (_, _, title_before) =
                self.validate_with_mode(target, TargetValidationMode::ContainerOnly)?;
            let scope = self
                .target_for(target)
                .ok_or_else(|| blocked("ax_target_missing"))?
                .scope
                .clone();
            let purpose = if target.bundle_id == "com.google.antigravity" {
                ObservationPurpose::AntigravityControls
            } else {
                ObservationPurpose::Controls
            };
            let observation = observe_window(scope, purpose)?;
            let controls =
                Self::controls_from_observation_for_bundle(&observation, &target.bundle_id);
            let (_, _, title_after) =
                self.validate_with_mode(target, TargetValidationMode::ContainerOnly)?;
            if title_before != title_after {
                return Err(blocked("target_window_changed"));
            }
            Ok(ManagedWindow {
                identity: ManagedWindowIdentity {
                    title: title_after,
                    ..target.clone()
                },
                controls,
            })
        }
        fn press(&mut self, target: &ManagedWindowIdentity, control: u64) -> Result<(), String> {
            let (control, element) = self.checked_control(target, control)?;
            if !control.enabled || !control.pressable {
                return Err(blocked("ax_control_not_pressable"));
            }
            element.action("AXPress")
        }
        fn read_control(
            &mut self,
            target: &ManagedWindowIdentity,
            control: u64,
        ) -> Result<ManagedControl, String> {
            self.checked_exact_control(target, control)
                .map(|(metadata, _)| metadata)
        }
        fn focus(&mut self, target: &ManagedWindowIdentity, control: u64) -> Result<(), String> {
            let (metadata, element) = self.checked_exact_control(target, control)?;
            if !metadata.enabled || !metadata.editable || !element.settable("AXFocused")? {
                return Err(blocked("ax_input_not_focusable"));
            }
            let (application, window) = self.validate(target)?;
            application.set("AXFrontmost", unsafe { kCFBooleanTrue })?;
            window.action("AXRaise")?;
            self.validate(target).map_err(unconfirmed)?;
            element.set("AXFocused", unsafe { kCFBooleanTrue })
        }
        fn scroll(
            &mut self,
            target: &ManagedWindowIdentity,
            request: &DialScrollRequest,
        ) -> Result<(), String> {
            let snapshot = self.read(target)?;
            if request.jump_to_bottom && !request.sidebar {
                let buttons: Vec<_> = snapshot
                    .controls
                    .iter()
                    .filter(|control| {
                        control.enabled
                            && control.pressable
                            && matches!(
                                control.label.trim(),
                                "Jump to bottom" | "Scroll to bottom" | "跳到底部" | "滚动到底部"
                            )
                    })
                    .collect();
                if let [button] = buttons.as_slice() {
                    self.press(target, button.id)?;
                    let deadline = Instant::now() + Duration::from_millis(400);
                    loop {
                        let current = self
                            .read(target)
                            .map_err(|error| format!("unconfirmed:{error}"))?;
                        if !current.controls.iter().any(|control| {
                            control.enabled && control.pressable && control.label == button.label
                        }) {
                            return Ok(());
                        }
                        if Instant::now() >= deadline {
                            return Err("unconfirmed:scroll_bottom_readback".into());
                        }
                        std::thread::sleep(Duration::from_millis(25));
                    }
                }
                if buttons.len() > 1 {
                    return Err(blocked("ambiguous_scroll_bottom_control"));
                }
            }
            let inputs: Vec<_> = snapshot
                .controls
                .iter()
                .filter(|control| {
                    control.role == "AXTextArea" && control.editable && control.enabled
                })
                .collect();
            let input = if let [input] = inputs.as_slice() {
                *input
            } else if inputs.len() > 1 {
                let focused: Vec<_> = inputs
                    .iter()
                    .copied()
                    .filter(|control| control.focused)
                    .collect();
                if let [input] = focused.as_slice() {
                    *input
                } else {
                    let primary: Vec<_> = inputs
                        .iter()
                        .copied()
                        .filter(|control| !crate::managed_controls::is_side_question_input(control))
                        .collect();
                    let [input] = primary.as_slice() else {
                        return Err(blocked("ambiguous_or_missing_input"));
                    };
                    *input
                }
            } else {
                return Err(blocked("ambiguous_or_missing_input"));
            };
            let input_rect = self
                .element(input.id)?
                .rect()?
                .ok_or_else(|| blocked("input_geometry_unavailable"))?;
            let mut candidates: Vec<Owned> = Vec::new();
            for control in snapshot
                .controls
                .iter()
                .filter(|control| control.role == "AXScrollArea")
            {
                let area = self.element(control.id)?;
                let Some(rect) = area.rect()? else {
                    continue;
                };
                let overlap = (rect.origin.x + rect.size.width)
                    .min(input_rect.origin.x + input_rect.size.width)
                    - rect.origin.x.max(input_rect.origin.x);
                let matches_region = if request.sidebar {
                    rect.origin.x + rect.size.width <= input_rect.origin.x
                        && rect.size.height > input_rect.size.height
                } else {
                    overlap >= input_rect.size.width * 0.75
                };
                if !matches_region {
                    continue;
                }
                let Some(bar) = area.attr("AXVerticalScrollBar")? else {
                    continue;
                };
                if bar.settable("AXValue")?
                    && !candidates
                        .iter()
                        .any(|old| unsafe { CFEqual(old.0, bar.0) } != 0)
                {
                    candidates.push(bar);
                }
            }
            if candidates.is_empty() {
                return self.scroll_with_targeted_event(
                    target,
                    input_rect,
                    &snapshot.controls,
                    request,
                );
            }
            let [bar] = candidates.as_slice() else {
                return Err(blocked("ambiguous_or_missing_target_scrollbar"));
            };
            if bar.pid() != Some(target.pid) {
                return Err(blocked("scrollbar_pid_mismatch"));
            }
            let current = bar
                .attr("AXValue")?
                .and_then(|v| v.number())
                .ok_or_else(|| blocked("scroll_position_unavailable"))?;
            let min = bar
                .attr("AXMinValue")?
                .and_then(|v| v.number())
                .ok_or_else(|| blocked("scroll_range_unavailable"))?;
            let max = bar
                .attr("AXMaxValue")?
                .and_then(|v| v.number())
                .ok_or_else(|| blocked("scroll_range_unavailable"))?;
            if max <= min {
                return Err(blocked("scroll_range_invalid"));
            }
            let next = if request.jump_to_bottom {
                max
            } else {
                (current + (max - min) * f64::from(request.clamped_delta_lines()) / 100.0)
                    .clamp(min, max)
            };
            self.validate(target)?;
            let value = unsafe {
                Owned::from_create(CFNumberCreate(
                    std::ptr::null(),
                    13,
                    (&next as *const f64).cast(),
                ))
            }
            .ok_or_else(|| blocked("scroll_value_unavailable"))?;
            write_with_readback(
                || bar.set("AXValue", value.0),
                || {
                    self.validate(target)?;
                    Ok(bar
                        .attr("AXValue")?
                        .and_then(|value| value.number())
                        .is_some_and(|value| (value - next).abs() <= 0.001 * (max - min)))
                },
                "scroll_readback",
            )
        }

        fn insert_text(
            &mut self,
            target: &ManagedWindowIdentity,
            control: u64,
            text: &str,
        ) -> Result<(), String> {
            let (input, element) = self.checked_control(target, control)?;
            if !input.enabled || !input.editable || !input.focused {
                return Err(blocked("input_focus_changed"));
            }
            let original = element
                .string_attr("AXValue")?
                .ok_or_else(|| blocked("input_value_unavailable"))?;
            let range = element
                .attr("AXSelectedTextRange")?
                .ok_or_else(|| blocked("input_selection_unavailable"))?;
            if unsafe { CFGetTypeID(range.0) != AXValueGetTypeID() } {
                return Err(blocked("input_selection_unavailable"));
            }
            let mut selection = Range::default();
            if !unsafe { AXValueGetValue(range.0, 4, (&mut selection as *mut Range).cast()) } {
                return Err(blocked("input_selection_unavailable"));
            }
            let replacement = crate::managed_controls::insert_at_utf16_selection(
                &original,
                selection.location,
                selection.length,
                text,
            )?;
            if replacement.len() >= 4096 {
                return Err(blocked("input_value_too_large"));
            }
            let value =
                Owned::text(&replacement).ok_or_else(|| blocked("input_value_unavailable"))?;
            self.validate(target)?;
            write_with_readback(
                || element.set("AXValue", value.0),
                || {
                    self.validate(target)?;
                    Ok(element.string_attr("AXValue")?.as_deref() == Some(&replacement))
                },
                "input_value_readback",
            )
        }

        fn shortcut(
            &mut self,
            target: &ManagedWindowIdentity,
            control: u64,
            key: u16,
            modifiers: &[String],
        ) -> Result<(), String> {
            let (input, _) = self.checked_control(target, control)?;
            if !input.enabled || !input.editable || !input.focused {
                return Err(blocked("input_focus_changed"));
            }
            let (app, window) = self.validate(target)?;
            let focused = app
                .attr("AXFocusedWindow")?
                .ok_or_else(|| blocked("target_window_not_focused"))?;
            if app.bool_attr("AXFrontmost")? != Some(true)
                || unsafe { CFEqual(focused.0, window.0) } == 0
            {
                return Err(blocked("target_window_not_focused"));
            }
            let down = unsafe {
                Owned::from_create(CGEventCreateKeyboardEvent(std::ptr::null_mut(), key, true))
            }
            .ok_or_else(|| blocked("keyboard_event_unavailable"))?;
            let up = unsafe {
                Owned::from_create(CGEventCreateKeyboardEvent(std::ptr::null_mut(), key, false))
            }
            .ok_or_else(|| blocked("keyboard_event_unavailable"))?;
            let flags = crate::executor::modifiers_to_cg_flags(modifiers);
            unsafe {
                CGEventSetFlags(down.0.cast_mut(), flags);
                CGEventSetFlags(up.0.cast_mut(), flags);
                CGEventPostToPid(target.pid, down.0.cast_mut());
                CGEventPostToPid(target.pid, up.0.cast_mut());
            }
            Ok(())
        }
        fn submit(&mut self, target: &ManagedWindowIdentity, control: u64) -> Result<(), String> {
            let (input, _) = self.checked_control(target, control)?;
            if !input.focused {
                return Err(blocked("input_focus_changed"));
            }
            let deadline = Instant::now() + Duration::from_millis(400);
            loop {
                let window = self.read(target)?;
                if !window
                    .controls
                    .iter()
                    .any(|input| input.id == control && input.focused && input.editable)
                {
                    return Err(blocked("input_focus_changed"));
                }
                crate::managed_controls::validate_codex_submit(&window)?;
                let buttons: Vec<_> = window
                    .controls
                    .iter()
                    .filter(|control| {
                        control.enabled
                            && control.pressable
                            && matches!(
                                control.label.trim(),
                                "Send"
                                    | "Send message"
                                    | "Submit"
                                    | "发送"
                                    | "发送消息"
                                    | "傳送訊息"
                                    | "提交"
                            )
                    })
                    .collect();
                if let [button] = buttons.as_slice() {
                    return self.press(target, button.id);
                }
                if buttons.len() > 1 || Instant::now() >= deadline {
                    return Err(blocked("ambiguous_or_missing_submit_control"));
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        }
        fn send_window_key(
            &mut self,
            target: &ManagedWindowIdentity,
            key_code: u16,
            modifiers: &[String],
        ) -> Result<(), String> {
            self.validate(target)?;
            let down = unsafe {
                Owned::from_create(CGEventCreateKeyboardEvent(
                    std::ptr::null_mut(),
                    key_code,
                    true,
                ))
            }
            .ok_or_else(|| blocked("keyboard_event_unavailable"))?;
            let up = unsafe {
                Owned::from_create(CGEventCreateKeyboardEvent(
                    std::ptr::null_mut(),
                    key_code,
                    false,
                ))
            }
            .ok_or_else(|| blocked("keyboard_event_unavailable"))?;
            let flags = crate::executor::modifiers_to_cg_flags(modifiers);
            unsafe {
                CGEventSetFlags(down.0.cast_mut(), flags);
                CGEventSetFlags(up.0.cast_mut(), flags);
                CGEventPostToPid(target.pid, down.0.cast_mut());
                CGEventPostToPid(target.pid, up.0.cast_mut());
            }
            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::session_navigation::{
            SessionNavigationWindow, SidebarConversationObservation, SidebarRowVisualState,
        };

        #[test]
        fn codex_dictation_controls_remain_observed_when_disabled() {
            let provider = NativeAxProvider::default();
            for label in [
                "Dictate",
                "听写",
                "Stop dictation",
                "停止听写",
                "Finishing dictation",
                "正在完成听写",
            ] {
                let control = provider
                    .control_from_node_data(
                        1,
                        NodeData {
                            role: "AXButton".into(),
                            label: label.into(),
                            enabled: false,
                            pressable: false,
                            ..Default::default()
                        },
                    )
                    .unwrap();
                assert_eq!(control.label, label);
                assert!(!control.enabled);
                assert!(!control.pressable);
            }
        }

        #[test]
        fn codex_native_submit_rejects_observed_recording_or_finishing_controls() {
            let mut window = ManagedWindow {
                identity: ManagedWindowIdentity {
                    bundle_id: "com.openai.codex".into(),
                    pid: 100,
                    window_id: 1,
                    scope_id: 0,
                    title: "Test".into(),
                },
                controls: vec![],
            };
            assert!(crate::managed_controls::validate_codex_submit(&window).is_ok());
            for (label, enabled, pressable, allowed) in [
                ("Dictate", true, true, true),
                ("听写", true, true, true),
                ("Dictate", false, true, false),
                ("Stop dictation", true, true, false),
                ("Finishing dictation", false, false, false),
                ("正在完成听写", false, false, false),
            ] {
                window.controls = vec![ManagedControl {
                    id: 2,
                    role: "AXButton".into(),
                    label: label.into(),
                    enabled,
                    pressable,
                    editable: false,
                    focused: false,
                    expanded: None,
                }];
                assert_eq!(
                    crate::managed_controls::validate_codex_submit(&window).is_ok(),
                    allowed,
                    "{label}"
                );
            }
            window.controls[0].label = "Dictate".into();
            window.controls[0].enabled = true;
            window.controls[0].pressable = true;
            window.controls.push(ManagedControl {
                id: 3,
                ..window.controls[0].clone()
            });
            assert!(crate::managed_controls::validate_codex_submit(&window).is_err());
        }

        #[test]
        fn grokbot_status_node_and_trilingual_controls_are_captured_and_guarded() {
            let status_data = read_observed_node(
                ObservationPurpose::Controls,
                |names| {
                    Ok(names
                        .iter()
                        .map(|name| match *name {
                            "AXRole" => Owned::text("AXGroup"),
                            "AXSubrole" => Owned::text("AXApplicationStatus"),
                            "AXHidden" => Some(unsafe { Owned(CFRetain(kCFBooleanFalse)) }),
                            "AXChildren" => None,
                            "AXDescription" => Owned::text("正在转写语音输入…"),
                            "AXTitle" => None,
                            "AXEnabled" => Some(unsafe { Owned(CFRetain(kCFBooleanTrue)) }),
                            _ => None,
                        })
                        .collect())
                },
                |_| Ok(false),
                || Ok(Vec::new()),
            )
            .unwrap()
            .expect("AXApplicationStatus node observed");
            assert_eq!(status_data.role, "AXGroup");
            assert_eq!(status_data.label, "正在转写语音输入…");
            assert!(!status_data.pressable);

            let status_control = NativeAxProvider::control_from_node_data_for_bundle(
                Some("com.anysphere.sand"),
                41,
                status_data,
            )
            .expect("Grok Bot transcribing status node retained");
            assert_eq!(status_control.label, "正在转写语音输入…");
            assert!(!status_control.pressable);

            for (label, pressable, enabled) in [
                ("Start voice input", true, true),
                ("开始语音输入", true, true),
                ("開始語音輸入", true, true),
                ("Stop dictation", true, true),
                ("停止听写", true, true),
                ("停止聽寫", true, true),
                ("Start voice chat", true, true),
                ("发起语音聊天", true, true),
                ("發起語音聊天", true, true),
                ("Hang up", true, true),
                ("挂断", false, false),
                ("掛斷", true, false),
                ("Send message", true, true),
                ("发送消息", true, true),
                ("傳送訊息", true, true),
            ] {
                let retained = NativeAxProvider::control_from_node_data_for_bundle(
                    Some("com.anysphere.sand"),
                    50,
                    NodeData {
                        role: "AXButton".into(),
                        label: label.into(),
                        enabled,
                        pressable,
                        ..Default::default()
                    },
                )
                .unwrap_or_else(|| panic!("Grok Bot control must be retained: {label}"));
                assert_eq!(retained.label, label);
                assert_eq!(retained.enabled, enabled);
            }

            let mut window = ManagedWindow {
                identity: ManagedWindowIdentity {
                    bundle_id: "com.anysphere.sand".into(),
                    pid: 200,
                    window_id: 2,
                    scope_id: 0,
                    title: "Grok Bot".into(),
                },
                controls: vec![],
            };
            assert!(crate::managed_controls::validate_codex_submit(&window).is_ok());
            for (role, label, enabled, pressable, allowed) in [
                ("AXButton", "Start voice input", true, true, true),
                ("AXButton", "开始语音输入", true, true, true),
                ("AXButton", "開始語音輸入", true, true, true),
                ("AXButton", "Stop dictation", true, true, false),
                ("AXButton", "停止听写", true, true, false),
                ("AXButton", "停止聽寫", true, true, false),
                ("AXGroup", "Transcribing voice input…", true, false, false),
                ("AXGroup", "正在转写语音输入…", true, false, false),
                ("AXGroup", "正在轉錄語音輸入…", true, false, false),
            ] {
                window.controls = vec![ManagedControl {
                    id: 7,
                    role: role.into(),
                    label: label.into(),
                    enabled,
                    pressable,
                    editable: false,
                    focused: false,
                    expanded: None,
                }];
                assert_eq!(
                    crate::managed_controls::validate_codex_submit(&window).is_ok(),
                    allowed,
                    "{label}"
                );
            }
        }

        fn node(
            id: &str,
            role: &str,
            label: &str,
            parent: Option<usize>,
            url: Option<&str>,
            expanded: Option<bool>,
            pressable: bool,
        ) -> ObservedNode {
            ObservedNode {
                element: Owned::text(id).unwrap(),
                parent,
                data: NodeData {
                    role: role.into(),
                    label: label.into(),
                    enabled: true,
                    pressable,
                    expanded,
                    url: url.map(str::to_owned),
                    ..Default::default()
                },
            }
        }

        #[test]
        fn observe_sidebar_conversations_classifies_spinner_unread_and_read_rows_across_layouts() {
            let grouped = WindowObservation {
                nodes: vec![
                    node("win", "AXWindow", "", None, None, None, false),
                    node("list", "AXList", "", Some(0), None, None, false),
                    node("row-run", "AXGroup", "", Some(1), None, None, false),
                    node(
                        "link-run",
                        "AXLink",
                        "Duplicate Title",
                        Some(2),
                        Some("https://127.0.0.1:4321/c/uuid-running"),
                        None,
                        true,
                    ),
                    node(
                        "txt-run",
                        "AXStaticText",
                        "Duplicate Title",
                        Some(3),
                        None,
                        None,
                        false,
                    ),
                    node(
                        "btn-stop",
                        "AXButton",
                        "Stop execution",
                        Some(2),
                        None,
                        None,
                        true,
                    ),
                    node("row-unread", "AXGroup", "", Some(1), None, None, false),
                    node(
                        "link-unread",
                        "AXLink",
                        "Duplicate Title",
                        Some(6),
                        Some("https://127.0.0.1:4321/c/uuid-unread"),
                        None,
                        true,
                    ),
                    node(
                        "btn-pin-u",
                        "AXButton",
                        "Pin conversation",
                        Some(6),
                        None,
                        None,
                        true,
                    ),
                    node(
                        "btn-arch-u",
                        "AXButton",
                        "Archive conversation",
                        Some(6),
                        None,
                        None,
                        true,
                    ),
                    node("row-read", "AXGroup", "", Some(1), None, None, false),
                    node(
                        "link-read",
                        "AXLink",
                        "Duplicate Title",
                        Some(10),
                        Some("https://127.0.0.1:4321/c/uuid-read"),
                        None,
                        true,
                    ),
                    node(
                        "btn-arch-r",
                        "AXButton",
                        "Archive conversation",
                        Some(10),
                        None,
                        None,
                        true,
                    ),
                    node("age-r", "AXStaticText", "4h", Some(10), None, None, false),
                    node("chat-pane", "AXGroup", "", Some(0), None, None, false),
                    node(
                        "body-link",
                        "AXLink",
                        "Spoofed Chat Link",
                        Some(14),
                        Some("https://127.0.0.1:4321/c/uuid-spoof"),
                        None,
                        true,
                    ),
                ],
            };
            assert_eq!(
                observe_sidebar_conversations(&grouped, 4321),
                vec![
                    SidebarConversationObservation {
                        conversation_id: "uuid-running".into(),
                        title: "Duplicate Title".into(),
                        state: SidebarRowVisualState::InProgress,
                    },
                    SidebarConversationObservation {
                        conversation_id: "uuid-unread".into(),
                        title: "Duplicate Title".into(),
                        state: SidebarRowVisualState::UnreadIdle,
                    },
                    SidebarConversationObservation {
                        conversation_id: "uuid-read".into(),
                        title: "Duplicate Title".into(),
                        state: SidebarRowVisualState::ReadIdle,
                    },
                ]
            );

            let flattened = WindowObservation {
                nodes: vec![
                    node("list", "AXGroup", "", None, None, None, false),
                    node(
                        "link-1",
                        "AXLink",
                        "",
                        Some(0),
                        Some("https://127.0.0.1:4321/c/flat-unread"),
                        None,
                        true,
                    ),
                    node(
                        "title-1",
                        "AXStaticText",
                        "Same Name",
                        Some(1),
                        None,
                        None,
                        false,
                    ),
                    node(
                        "arch-1",
                        "AXButton",
                        "Archive conversation",
                        Some(0),
                        None,
                        None,
                        true,
                    ),
                    node(
                        "link-2",
                        "AXLink",
                        "Same Name",
                        Some(0),
                        Some("https://127.0.0.1:4321/c/flat-read"),
                        None,
                        true,
                    ),
                    node(
                        "arch-2",
                        "AXButton",
                        "Archive conversation",
                        Some(0),
                        None,
                        None,
                        true,
                    ),
                    node("age-2", "AXStaticText", "10m", Some(0), None, None, false),
                    node(
                        "link-3",
                        "AXLink",
                        "Same Name",
                        Some(0),
                        Some("https://127.0.0.1:4321/c/flat-run"),
                        None,
                        true,
                    ),
                    node(
                        "stop-3",
                        "AXButton",
                        "Stop execution",
                        Some(0),
                        None,
                        None,
                        true,
                    ),
                    node(
                        "see-all",
                        "AXButton",
                        "See all (8)",
                        Some(0),
                        None,
                        None,
                        true,
                    ),
                ],
            };
            assert_eq!(
                observe_sidebar_conversations(&flattened, 4321),
                vec![
                    SidebarConversationObservation {
                        conversation_id: "flat-unread".into(),
                        title: "Same Name".into(),
                        state: SidebarRowVisualState::UnreadIdle,
                    },
                    SidebarConversationObservation {
                        conversation_id: "flat-read".into(),
                        title: "Same Name".into(),
                        state: SidebarRowVisualState::ReadIdle,
                    },
                    SidebarConversationObservation {
                        conversation_id: "flat-run".into(),
                        title: "Same Name".into(),
                        state: SidebarRowVisualState::InProgress,
                    },
                ]
            );
        }

        #[test]
        fn direct_sidebar_link_verification_rejects_spoofed_urls_and_window_changes() {
            let window = Owned::text("window-1").unwrap();
            let link = Owned::text("link-target").unwrap();
            for (scenario, win_id, role, enabled, live_url, should_succeed) in [
                (
                    "valid",
                    "window-1",
                    "AXLink",
                    true,
                    "https://127.0.0.1:4321/c/target-id",
                    true,
                ),
                (
                    "wrong-window",
                    "window-2",
                    "AXLink",
                    true,
                    "https://127.0.0.1:4321/c/target-id",
                    false,
                ),
                (
                    "wrong-role",
                    "window-1",
                    "AXButton",
                    true,
                    "https://127.0.0.1:4321/c/target-id",
                    false,
                ),
                (
                    "disabled",
                    "window-1",
                    "AXLink",
                    false,
                    "https://127.0.0.1:4321/c/target-id",
                    false,
                ),
                (
                    "spoof-origin",
                    "window-1",
                    "AXLink",
                    true,
                    "https://evil.example:4321/c/target-id",
                    false,
                ),
                (
                    "spoof-port",
                    "window-1",
                    "AXLink",
                    true,
                    "https://127.0.0.1:9999/c/target-id",
                    false,
                ),
                (
                    "spoof-prefix",
                    "window-1",
                    "AXLink",
                    true,
                    "https://127.0.0.1:4321/c/target-id-extra",
                    false,
                ),
                (
                    "spoof-nested",
                    "window-1",
                    "AXLink",
                    true,
                    "https://127.0.0.1:4321/c/target-id/sub",
                    false,
                ),
            ] {
                let outcome = verify_sidebar_link_with(
                    &window,
                    &link,
                    4321,
                    "target-id",
                    Instant::now() + Duration::from_millis(400),
                    |_, names, _| {
                        Ok(names
                            .iter()
                            .map(|name| match *name {
                                "AXWindow" => Owned::text(win_id),
                                "AXRole" => Owned::text(role),
                                "AXEnabled" => unsafe {
                                    Some(Owned(CFRetain(if enabled {
                                        kCFBooleanTrue
                                    } else {
                                        kCFBooleanFalse
                                    })))
                                },
                                "AXURL" => Owned::text(live_url),
                                _ => None,
                            })
                            .collect())
                    },
                );
                assert_eq!(outcome.is_ok(), should_succeed, "{scenario}");
            }
        }

        #[test]
        fn cancellation_while_query_is_applying_never_rewrites_the_query() {
            let writes = std::cell::Cell::new(0);
            let mut reads = 0;
            let result = write_with_readback(
                || {
                    writes.set(writes.get() + 1);
                    Ok(())
                },
                || {
                    reads += 1;
                    if reads == 1 {
                        Ok(false)
                    } else {
                        Err(blocked("session_navigation_retired"))
                    }
                },
                "value_not_applied",
            );
            assert_eq!(
                result.unwrap_err(),
                "unconfirmed:blocked:session_navigation_retired"
            );
            assert_eq!(writes.get(), 1);
            assert_eq!(reads, 2);
        }

        #[test]
        fn unsupported_attributes_are_absent_but_failed_reads_abort_observation() {
            for code in [-25205i32, -25212] {
                let value =
                    unsafe { Owned::from_create(AXValueCreate(5, (&code as *const i32).cast())) }
                        .unwrap();
                assert!(decode_attribute_slot(value).unwrap().is_none());
            }
            let code = -25204i32;
            let value =
                unsafe { Owned::from_create(AXValueCreate(5, (&code as *const i32).cast())) }
                    .unwrap();
            assert!(decode_attribute_slot(value).is_err());
            let value = decode_attribute_slot(Owned::text("AXLink").unwrap())
                .unwrap()
                .unwrap();
            assert_eq!(value.string().as_deref(), Some("AXLink"));
        }

        #[test]
        fn successful_write_with_failed_observation_is_unconfirmed_and_never_retried() {
            let writes = std::cell::Cell::new(0);
            let result = write_with_readback(
                || {
                    writes.set(writes.get() + 1);
                    Ok(())
                },
                || Err("blocked:ax_attribute_-25204".into()),
                "value_readback",
            );
            assert_eq!(writes.get(), 1);
            assert_eq!(
                result.unwrap_err(),
                "unconfirmed:blocked:ax_attribute_-25204"
            );
            assert!(write_with_readback(
                || Err("unconfirmed:ax_set_-25204".into()),
                || panic!("no read after failed write"),
                "value_readback"
            )
            .is_err());
        }

        #[test]
        fn a_partial_tree_never_publishes_a_unique_candidate() {
            let mut visited = 0;
            let observation = observe_tree(
                Owned::text("root").unwrap(),
                Instant::now() + Duration::from_secs(1),
                |element| {
                    visited += 1;
                    match element.string().as_deref() {
                        Some("root") => Ok(Some(NodeData {
                            children: vec![
                                Owned::text("candidate").unwrap(),
                                Owned::text("unreadable").unwrap(),
                            ],
                            ..Default::default()
                        })),
                        Some("candidate") => Ok(Some(NodeData {
                            role: "AXLink".into(),
                            url: Some("https://127.0.0.1:1/c/target".into()),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        })),
                        _ => Err("blocked:ax_attribute_-25204".into()),
                    }
                },
            );
            assert_eq!(visited, 3);
            assert!(observation.is_err());
            let complete = observe_tree(
                Owned::text("leaf").unwrap(),
                Instant::now() + Duration::from_secs(1),
                |_| Ok(Some(NodeData::default())),
            )
            .unwrap();
            assert_eq!(complete.nodes.len(), 1);
        }

        #[test]
        fn transient_rebuild_discards_the_entire_incomplete_attempt() {
            let mut attempt = 0;
            let observation = observe_tree(
                Owned::text("window").unwrap(),
                Instant::now() + Duration::from_millis(300),
                |element| match element.string().as_deref() {
                    Some("window") => {
                        attempt += 1;
                        let children = if attempt == 1 {
                            vec![
                                Owned::text("old-candidate").unwrap(),
                                Owned::text("retired-node").unwrap(),
                            ]
                        } else {
                            vec![Owned::text("new-candidate").unwrap()]
                        };
                        Ok(Some(NodeData {
                            children,
                            ..Default::default()
                        }))
                    }
                    Some("retired-node") => Err("blocked:ax_batch_-25202".into()),
                    _ => Ok(Some(NodeData {
                        role: "AXLink".into(),
                        ..Default::default()
                    })),
                },
            )
            .unwrap();
            assert_eq!(attempt, 2);
            assert_eq!(observation.nodes.len(), 2);
            assert_eq!(
                observation.nodes[1].element.string().as_deref(),
                Some("new-candidate")
            );
            assert!(observe_tree(
                Owned::text("retired-window").unwrap(),
                Instant::now() + Duration::from_millis(20),
                |_| Err("blocked:ax_batch_-25202".into())
            )
            .is_err());
        }

        #[test]
        fn traversal_refuses_cycles_deadlines_and_oversized_frontiers() {
            assert!(observe_tree(
                Owned::text("cycle").unwrap(),
                Instant::now() + Duration::from_secs(1),
                |element| Ok(Some(NodeData {
                    children: vec![element.clone()],
                    ..Default::default()
                }))
            )
            .is_err());
            assert!(observe_tree(
                Owned::text("expired").unwrap(),
                Instant::now() - Duration::from_millis(1),
                |_| panic!("expired reads must not start")
            )
            .is_err());
            let leaf = Owned::text("leaf").unwrap();
            assert!(observe_tree(
                Owned::text("wide").unwrap(),
                Instant::now() + Duration::from_secs(1),
                |_| Ok(Some(NodeData {
                    children: vec![leaf.clone(); MAX_NODES],
                    ..Default::default()
                }))
            )
            .is_err());
        }

        #[test]
        #[ignore = "read-only observation of the running Antigravity window"]
        fn native_window_observation_reads_both_purposes_without_input_values() {
            use crate::session_navigation::SessionNavigationProvider;
            let pid =
                crate::session_navigation::antigravity_process_id().expect("running Antigravity");
            let mut sessions = NativeSessionNavigation::default();
            let selected = sessions.capture(pid).unwrap();
            assert!(selected
                .selected_urls
                .iter()
                .any(|url| url.starts_with("https://127.0.0.1:")));
            let mut controls = NativeAxProvider::default();
            let snapshot = controls.capture("com.google.antigravity").unwrap();
            assert_eq!(snapshot.identity.pid, pid);
            assert!(snapshot.controls.iter().any(|control| control.editable));
            assert_eq!(
                controls.read(&snapshot.identity).unwrap().identity,
                snapshot.identity
            );
        }

        #[test]
        fn control_from_node_data_exposes_subagent_and_dsh_model_triggers_while_dropping_stop_buttons(
        ) {
            let make_provider = |bundle: &str| NativeAxProvider {
                target: Some(Target {
                    identity: ManagedWindowIdentity {
                        bundle_id: bundle.into(),
                        pid: 42,
                        window_id: 1,
                        scope_id: 0,
                        title: "window".into(),
                    },
                    window: CapturedWindow {
                        bundle_id: bundle.into(),
                        pid: 42,
                        application: Owned::text("app").unwrap(),
                        element: Owned::text("win").unwrap(),
                        title: "window".into(),
                    },
                    scope: Owned::text("win").unwrap(),
                }),
                ..Default::default()
            };

            let ag = make_provider("com.google.antigravity");
            assert!(ag
                .control_from_node_data(
                    1,
                    NodeData {
                        role: "AXPopUpButton".into(),
                        label: "1 subagent running".into(),
                        enabled: true,
                        pressable: true,
                        expanded: Some(false),
                        ..Default::default()
                    }
                )
                .is_some());
            assert!(ag
                .control_from_node_data(
                    2,
                    NodeData {
                        role: "AXButton".into(),
                        label: "Open Subagent: Code Quality Auditor & Release Engineer (DeepCoder)"
                            .into(),
                        enabled: true,
                        pressable: true,
                        ..Default::default()
                    }
                )
                .is_some());
            for stop_label in [
                "Stop subagent",
                "Improvement Coding Worker Stop subagent",
                "Close Layer 0 Coding Worker",
            ] {
                assert!(
                    ag.control_from_node_data(
                        3,
                        NodeData {
                            role: "AXButton".into(),
                            label: stop_label.into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        }
                    )
                    .is_none(),
                    "must drop {stop_label}"
                );
            }

            let dsh = make_provider("com.deepseek.dsh");
            assert!(dsh
                .control_from_node_data(
                    10,
                    NodeData {
                        role: "AXPopUpButton".into(),
                        label: "选择模型，当前 DeepSeek-V4.1-Flash，推理等级 High".into(),
                        enabled: true,
                        pressable: true,
                        expanded: Some(false),
                        ..Default::default()
                    }
                )
                .is_some());
            assert!(dsh
                .control_from_node_data(
                    11,
                    NodeData {
                        role: "AXButton".into(),
                        label: "3 个子智能体".into(),
                        enabled: true,
                        pressable: true,
                        ..Default::default()
                    }
                )
                .is_some());

            let codex = make_provider("com.openai.codex");
            assert!(codex
                .control_from_node_data(
                    20,
                    NodeData {
                        role: "AXPopUpButton".into(),
                        label: "2 subagents".into(),
                        enabled: true,
                        pressable: true,
                        expanded: Some(false),
                        ..Default::default()
                    }
                )
                .is_some());
        }

        #[test]
        #[ignore = "read-only observation of running Antigravity and DSH windows"]
        fn live_native_ax_provider_surfaces_subagent_and_model_controls_readonly() {
            let mut provider = NativeAxProvider::default();
            if let Ok(ag_win) = provider.capture_full_window("com.google.antigravity") {
                println!(
                    "live Antigravity filtered controls ({}): {:?}",
                    ag_win.controls.len(),
                    ag_win
                        .controls
                        .iter()
                        .map(|c| (&c.role, &c.label, c.expanded))
                        .collect::<Vec<_>>()
                );
                assert!(
                    !ag_win
                        .controls
                        .iter()
                        .any(|c| c.label.to_lowercase().contains("stop subagent")),
                    "filtered Antigravity controls must never include Stop subagent"
                );
                if let Some(toggle) = ag_win.controls.iter().find(|c| {
                    c.role == "AXButton"
                        && crate::antigravity_ax_labels::is_toggle_side_panel(&c.label)
                }) {
                    let reread = provider
                        .read_control(&ag_win.identity, toggle.id)
                        .expect("read_control on Toggle Side Panel");
                    assert_eq!(
                        reread.expanded, toggle.expanded,
                        "read_control must preserve AXExpanded on Toggle Side Panel"
                    );
                    assert!(
                        reread.expanded.is_some(),
                        "Toggle Side Panel must expose Some(bool) AXExpanded via read_control"
                    );
                }
            }
            if let Ok(dsh_win) = provider.capture_full_window("com.deepseek.dsh") {
                println!(
                    "live DSH filtered controls ({}): {:?}",
                    dsh_win.controls.len(),
                    dsh_win
                        .controls
                        .iter()
                        .map(|c| (&c.role, &c.label, c.expanded))
                        .collect::<Vec<_>>()
                );
                assert!(
                    dsh_win.controls.iter().any(|c| {
                        crate::managed_controls::is_dsh_model_picker_trigger(&c.label)
                    }),
                    "filtered DSH controls must surface the composer model picker popup"
                );
            }
        }

        #[test]
        fn owned_window_handles_survive_other_app_and_replacement_captures() {
            let target = |bundle: &str, id: u64| Target {
                identity: ManagedWindowIdentity {
                    bundle_id: bundle.into(),
                    pid: 123,
                    window_id: id,
                    scope_id: 0,
                    title: format!("window-{id}"),
                },
                window: CapturedWindow {
                    bundle_id: bundle.into(),
                    pid: 123,
                    application: Owned::text(&format!("app-{bundle}")).unwrap(),
                    element: Owned::text(&format!("window-{id}")).unwrap(),
                    title: format!("window-{id}"),
                },
                scope: Owned::text(&format!("window-{id}")).unwrap(),
            };
            let original = target("com.google.antigravity", 1);
            let identity = original.identity.clone();
            let original_window = original.window.clone();
            let mut provider = NativeAxProvider {
                target: Some(original),
                ..Default::default()
            };
            provider.retain_target(&identity).unwrap();
            provider.target = Some(target("com.deepseek.dsh", 2));
            assert_eq!(
                provider.checked_exact_control(&identity, 1).unwrap_err(),
                "blocked:target_identity_changed"
            );
            assert_eq!(
                provider.target_for(&identity).unwrap().window.element.0,
                original_window.element.0
            );
            provider.target = Some(target("com.google.antigravity", 3));
            assert_eq!(
                provider.target_for(&identity).unwrap().window.element.0,
                original_window.element.0
            );
            provider.release_target(&identity);
            assert!(provider.target_for(&identity).is_none());
            assert!(provider.retained_targets.is_empty());
        }

        #[test]
        fn retained_scope_survives_switching_panes_in_the_same_window() {
            let window = Owned::text("window").unwrap();
            let target = Target {
                identity: ManagedWindowIdentity {
                    bundle_id: "com.google.antigravity".into(),
                    pid: 123,
                    window_id: 1,
                    scope_id: 10,
                    title: "window".into(),
                },
                window: CapturedWindow {
                    bundle_id: "com.google.antigravity".into(),
                    pid: 123,
                    application: Owned::text("app").unwrap(),
                    element: window,
                    title: "window".into(),
                },
                scope: Owned::text("left-pane").unwrap(),
            };
            let original = target.identity.clone();
            let original_scope = target.scope.clone();
            let mut provider = NativeAxProvider {
                target: Some(target.clone()),
                ..Default::default()
            };
            provider.retain_target(&original).unwrap();
            let mut right = target;
            right.identity.scope_id = 11;
            right.scope = Owned::text("right-pane").unwrap();
            provider.target = Some(right);
            assert_eq!(
                provider.checked_exact_control(&original, 1).unwrap_err(),
                "blocked:target_identity_changed"
            );
            assert_eq!(
                provider.target_for(&original).unwrap().scope.0,
                original_scope.0
            );
            provider.release_target(&original);
            assert!(provider.target_for(&original).is_none());
        }

        #[test]
        fn rebuilt_panes_do_not_accumulate_scope_handles_or_exhaust_a_cache() {
            let mut provider = NativeAxProvider::default();
            let mut last = 0;
            for index in 0..256 {
                let scope = Owned::text(&format!("pane-{index}")).unwrap();
                let id = provider.scope_identity(123, 1, &scope, None).unwrap();
                assert!(id > last);
                last = id;
            }
            assert!(provider.retained_targets.is_empty());
            assert!(provider.target.is_none());
        }

        fn control_nodes(count: usize) -> Vec<ControlNode<usize>> {
            (0..count)
                .map(|index| ControlNode {
                    pid: Some(123),
                    role: Some(
                        if index + 1 == count {
                            "AXTextArea"
                        } else {
                            "AXGroup"
                        }
                        .into(),
                    ),
                    hidden: false,
                    window: Some(count),
                    children: if index + 1 < count {
                        vec![index + 1]
                    } else {
                        Vec::new()
                    },
                })
                .collect()
        }

        fn check_path(nodes: &[ControlNode<usize>], path: &[usize]) -> Result<(), String> {
            validate_control_membership(
                path,
                &0,
                &nodes.len(),
                123,
                Instant::now() + Duration::from_secs(1),
                |left, right| left == right,
                |node| {
                    nodes
                        .get(*node)
                        .cloned()
                        .ok_or_else(|| blocked("ax_control_removed"))
                },
            )
        }

        #[test]
        fn exact_control_membership_accepts_live_path_with_optional_window() {
            let mut nodes = control_nodes(3);
            check_path(&nodes, &[0, 1, 2]).unwrap();
            for node in &mut nodes {
                node.window = None;
            }
            check_path(&nodes, &[0, 1, 2]).unwrap();
        }

        #[test]
        fn exact_control_membership_rejects_removed_or_replaced_path_edges() {
            for index in 0..2 {
                for children in [vec![], vec![8]] {
                    let mut nodes = control_nodes(3);
                    nodes[index].children = children;
                    assert_eq!(
                        check_path(&nodes, &[0, 1, 2]).unwrap_err(),
                        "blocked:ax_control_removed"
                    );
                }
            }
        }

        #[test]
        fn exact_control_membership_requires_the_original_scope() {
            let nodes = control_nodes(3);
            for path in [vec![], vec![1, 2]] {
                assert_eq!(
                    check_path(&nodes, &path).unwrap_err(),
                    "blocked:ax_control_scope_changed"
                );
            }
        }

        #[test]
        fn exact_control_membership_rejects_hidden_or_invalid_path_nodes() {
            for index in 0..3 {
                let mut nodes = control_nodes(3);
                nodes[index].hidden = true;
                assert_eq!(
                    check_path(&nodes, &[0, 1, 2]).unwrap_err(),
                    "blocked:ax_control_hidden"
                );
                nodes[index].hidden = false;
                nodes[index].role = None;
                assert_eq!(
                    check_path(&nodes, &[0, 1, 2]).unwrap_err(),
                    "blocked:ax_role_unavailable"
                );
                nodes[index].role = Some("AXGroup".into());
                nodes[index].pid = Some(999);
                assert_eq!(
                    check_path(&nodes, &[0, 1, 2]).unwrap_err(),
                    "blocked:control_pid_mismatch"
                );
                nodes[index].pid = Some(123);
                nodes[index].window = Some(9);
                assert_eq!(
                    check_path(&nodes, &[0, 1, 2]).unwrap_err(),
                    "blocked:ax_control_window_changed"
                );
            }
        }

        #[test]
        fn exact_control_membership_rejects_cyclic_or_overlong_paths() {
            let mut nodes = control_nodes(3);
            nodes[1].children = vec![0];
            assert_eq!(
                check_path(&nodes, &[0, 1, 0]).unwrap_err(),
                "blocked:ax_control_ancestry_cycle"
            );
            let nodes = control_nodes(MAX_DEPTH + 2);
            let path: Vec<_> = (0..nodes.len()).collect();
            assert_eq!(
                check_path(&nodes, &path).unwrap_err(),
                "blocked:ax_tree_limit"
            );
        }

        #[test]
        fn exact_control_membership_rejects_read_errors_and_expired_budget() {
            let read_error = validate_control_membership(
                &[0, 1, 2],
                &0,
                &3,
                123,
                Instant::now() + Duration::from_secs(1),
                |left, right| left == right,
                |_| Err(blocked("ax_attribute_failed")),
            );
            assert_eq!(read_error.unwrap_err(), "blocked:ax_attribute_failed");
            let expired = validate_control_membership(
                &[0, 1, 2],
                &0,
                &3,
                123,
                Instant::now(),
                |left, right| left == right,
                |_| panic!("expired validation must not read AX"),
            );
            assert_eq!(
                expired.unwrap_err(),
                "blocked:ax_control_validation_timeout"
            );
        }

        fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
            Rect {
                origin: Point { x, y },
                size: Size { width, height },
            }
        }

        #[test]
        fn scroll_points_stay_inside_window_above_the_composer() {
            let window = rect(-1500.0, 100.0, 900.0, 800.0);
            let composer = rect(-1200.0, 730.0, 700.0, 120.0);
            let points = scroll_candidate_points(window, composer, None);
            assert!(!points.is_empty());
            assert!(points
                .iter()
                .all(|p| p.x > -1200.0 && p.x < -600.0 && p.y > 100.0 && p.y < 730.0));
            assert!(
                scroll_candidate_points(window, rect(-1200.0, 950.0, 300.0, 100.0), None)
                    .is_empty()
            );
            assert!(
                scroll_candidate_points(window, rect(-1200.0, 100.0, 300.0, 100.0), None)
                    .is_empty()
            );
        }

        #[test]
        fn known_chat_region_constrains_scroll_points() {
            let points = scroll_candidate_points(
                rect(0.0, 0.0, 1200.0, 900.0),
                rect(300.0, 700.0, 500.0, 100.0),
                Some(rect(250.0, 150.0, 600.0, 450.0)),
            );
            assert!(!points.is_empty());
            assert!(points
                .iter()
                .all(|p| p.x > 300.0 && p.x < 800.0 && p.y > 150.0 && p.y < 600.0));
        }

        #[test]
        fn process_exit_requires_esrch_and_never_treats_this_process_as_exited() {
            let provider = NativeAxProvider::default();
            assert!(!provider.process_has_exited(std::process::id() as i32));
            assert!(!provider.process_has_exited(0));
            let mut process = std::process::Command::new("/usr/bin/true").spawn().unwrap();
            let pid = process.id() as i32;
            process.wait().unwrap();
            assert!(provider.process_has_exited(pid));
        }

        fn ax_array(items: &[&str]) -> Option<Owned> {
            let owned: Vec<_> = items
                .iter()
                .map(|item| Owned::text(item).unwrap())
                .collect();
            let refs: Vec<_> = owned.iter().map(|item| item.0).collect();
            unsafe {
                Owned::from_create(CFArrayCreate(
                    std::ptr::null(),
                    refs.as_ptr(),
                    refs.len() as isize,
                    (&raw const kCFTypeArrayCallBacks).cast(),
                ))
            }
        }

        #[test]
        fn session_documents_prunes_non_empty_web_areas_after_traversing_wrappers() {
            let mut container_batches = 0;
            let observation = observe_tree_once(
                Owned::text("window").unwrap(),
                Instant::now() + Duration::from_secs(1),
                |element| {
                    let id = element.string().unwrap();
                    read_observed_node(
                        ObservationPurpose::SessionDocuments,
                        |names| match (id.as_str(), names) {
                            ("window", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => {
                                container_batches += 1;
                                Ok(vec![
                                    Owned::text("AXWindow"),
                                    None,
                                    None,
                                    ax_array(&["split"]),
                                ])
                            }
                            ("split", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => {
                                container_batches += 1;
                                Ok(vec![
                                    Owned::text("AXSplitGroup"),
                                    None,
                                    None,
                                    ax_array(&["empty-wrapper", "none-wrapper"]),
                                ])
                            }
                            (
                                "empty-wrapper",
                                ["AXRole", "AXSubrole", "AXHidden", "AXChildren"],
                            ) => Ok(vec![
                                Owned::text("AXWebArea"),
                                None,
                                None,
                                ax_array(&["conversation"]),
                            ]),
                            ("empty-wrapper", ["AXURL"]) => Ok(vec![Owned::text("")]),
                            ("conversation", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => {
                                Ok(vec![
                                    Owned::text("AXWebArea"),
                                    None,
                                    None,
                                    ax_array(&["conversation-dom"]),
                                ])
                            }
                            ("conversation", ["AXURL"]) => {
                                Ok(vec![Owned::text("https://127.0.0.1:4321/c/target")])
                            }
                            ("none-wrapper", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => {
                                Ok(vec![
                                    Owned::text("AXWebArea"),
                                    None,
                                    None,
                                    ax_array(&["background"]),
                                ])
                            }
                            ("none-wrapper", ["AXURL"]) => Ok(vec![None]),
                            ("background", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => {
                                Ok(vec![
                                    Owned::text("AXWebArea"),
                                    None,
                                    None,
                                    ax_array(&["background-dom"]),
                                ])
                            }
                            ("background", ["AXURL"]) => {
                                Ok(vec![Owned::text("data:text/html,background")])
                            }
                            (other, batch) => {
                                panic!("unexpected read on {other}: {batch:?}")
                            }
                        },
                        |_| panic!("SessionDocuments never checks settable"),
                        || panic!("SessionDocuments never queries actions"),
                    )
                },
            )
            .unwrap();
            assert_eq!(container_batches, 2);
            assert_eq!(
                selected_web_area_urls(&observation),
                vec![
                    "https://127.0.0.1:4321/c/target".to_string(),
                    "data:text/html,background".to_string(),
                ]
            );
        }

        #[test]
        fn sessions_purpose_batches_links_buttons_and_timestamps_without_extra_action_ipc() {
            let mut action_queries = Vec::new();
            let observation = observe_tree_once(
                Owned::text("window").unwrap(),
                Instant::now() + Duration::from_secs(1),
                |element| {
                    let id = element.string().unwrap();
                    read_observed_node(
                        ObservationPurpose::Sessions,
                        |names| match (id.as_str(), names) {
                            ("window", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => {
                                Ok(vec![
                                    Owned::text("AXWindow"),
                                    None,
                                    None,
                                    ax_array(&[
                                        "project-btn",
                                        "local-link",
                                        "external-link",
                                        "disabled-link",
                                        "age-text",
                                        "conv-turns",
                                    ]),
                                ])
                            }
                            ("project-btn", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => {
                                Ok(vec![Owned::text("AXButton"), None, None, ax_array(&[])])
                            }
                            (
                                "project-btn",
                                ["AXDescription", "AXTitle", "AXEnabled", "AXExpanded"],
                            ) => Ok(vec![
                                None,
                                Owned::text("micro-input"),
                                unsafe { Some(Owned(CFRetain(kCFBooleanTrue))) },
                                unsafe { Some(Owned(CFRetain(kCFBooleanFalse))) },
                            ]),
                            ("local-link", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => {
                                Ok(vec![Owned::text("AXLink"), None, None, ax_array(&[])])
                            }
                            ("local-link", ["AXDescription", "AXTitle", "AXEnabled", "AXURL"]) => {
                                Ok(vec![
                                    Owned::text("Fix sidebar"),
                                    Owned::text("Fallback"),
                                    unsafe { Some(Owned(CFRetain(kCFBooleanTrue))) },
                                    Owned::text("https://127.0.0.1:4321/c/c1"),
                                ])
                            }
                            (
                                "external-link",
                                ["AXRole", "AXSubrole", "AXHidden", "AXChildren"],
                            ) => Ok(vec![Owned::text("AXLink"), None, None, ax_array(&[])]),
                            (
                                "external-link",
                                ["AXDescription", "AXTitle", "AXEnabled", "AXURL"],
                            ) => Ok(vec![
                                Owned::text("Docs"),
                                None,
                                unsafe { Some(Owned(CFRetain(kCFBooleanTrue))) },
                                Owned::text("https://example.com/docs"),
                            ]),
                            (
                                "disabled-link",
                                ["AXRole", "AXSubrole", "AXHidden", "AXChildren"],
                            ) => Ok(vec![Owned::text("AXLink"), None, None, ax_array(&[])]),
                            (
                                "disabled-link",
                                ["AXDescription", "AXTitle", "AXEnabled", "AXURL"],
                            ) => Ok(vec![
                                Owned::text("Disabled"),
                                None,
                                unsafe { Some(Owned(CFRetain(kCFBooleanFalse))) },
                                Owned::text("https://127.0.0.1:4321/c/c2"),
                            ]),
                            ("age-text", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => {
                                Ok(vec![
                                    Owned::text("AXStaticText"),
                                    None,
                                    None,
                                    ax_array(&["inline-box-never-visited"]),
                                ])
                            }
                            ("age-text", ["AXValue", "AXEnabled"]) => {
                                Ok(vec![Owned::text("10m"), unsafe {
                                    Some(Owned(CFRetain(kCFBooleanTrue)))
                                }])
                            }
                            ("conv-turns", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => {
                                Ok(vec![
                                    Owned::text("AXGroup"),
                                    None,
                                    None,
                                    ax_array(&["turn-body-never-visited"]),
                                ])
                            }
                            ("conv-turns", ["AXDescription", "AXTitle"]) => {
                                Ok(vec![Owned::text("Conversation turns"), None])
                            }
                            (other, batch) => panic!("unexpected read on {other}: {batch:?}"),
                        },
                        |_| panic!("Sessions never queries settable"),
                        || {
                            action_queries.push(id.clone());
                            Ok(vec!["AXPress".into()])
                        },
                    )
                },
            )
            .unwrap();
            assert_eq!(
                action_queries,
                vec!["project-btn".to_string(), "local-link".to_string()]
            );
            assert_eq!(observation.nodes.len(), 7);
            assert_eq!(observation.nodes[1].data.expanded, Some(false));
            assert!(observation.nodes[1].data.pressable);
            assert_eq!(observation.nodes[2].data.label, "Fix sidebar");
            assert!(observation.nodes[2].data.pressable);
            assert!(!observation.nodes[3].data.pressable);
            assert!(!observation.nodes[4].data.pressable);
            assert_eq!(observation.nodes[5].data.label, "10m");
            assert!(!observation.nodes[5].data.pressable);
            assert!(observation.nodes[6].data.children.is_empty());
        }

        struct MockSidebarBackend {
            stages: Vec<WindowObservation>,
            current_stage: usize,
            pressed_buttons: Vec<(String, Option<bool>)>,
            pressed_links: Vec<String>,
        }

        impl MockSidebarBackend {
            fn new(stages: Vec<WindowObservation>) -> Self {
                Self {
                    stages,
                    current_stage: 0,
                    pressed_buttons: Vec::new(),
                    pressed_links: Vec::new(),
                }
            }
        }

        impl SidebarSelectionBackend for MockSidebarBackend {
            fn observe_until(&mut self, _: Instant) -> Result<WindowObservation, String> {
                self.stages
                    .get(self.current_stage)
                    .cloned()
                    .ok_or_else(|| "blocked:ax_snapshot_timeout".into())
            }

            fn verify_and_press_link(
                &mut self,
                observation: &WindowObservation,
                link_idx: usize,
                port: u16,
                conversation: &str,
                validate_lease: &mut dyn FnMut() -> Result<(), String>,
                is_current: &dyn Fn() -> bool,
            ) -> Result<(), String> {
                validate_lease()?;
                let n = &observation.nodes[link_idx];
                assert_eq!(
                    n.data
                        .url
                        .as_deref()
                        .and_then(|u| crate::session_navigation::conversation_id_from_url(u, port)),
                    Some(conversation)
                );
                if !is_current() {
                    return Err(blocked("session_navigation_retired"));
                }
                self.pressed_links
                    .push(n.element.string().unwrap_or_default());
                Ok(())
            }

            fn verify_and_press_button(
                &mut self,
                observation: &WindowObservation,
                button_idx: usize,
                expected_expanded: Option<bool>,
                validate_lease: &mut dyn FnMut() -> Result<(), String>,
                is_current: &dyn Fn() -> bool,
            ) -> Result<(), String> {
                validate_lease()?;
                let n = &observation.nodes[button_idx];
                if let Some(exp) = expected_expanded {
                    assert_eq!(n.data.expanded, Some(exp));
                }
                if !is_current() {
                    return Err(blocked("session_navigation_retired"));
                }
                self.pressed_buttons
                    .push((n.element.string().unwrap_or_default(), expected_expanded));
                self.current_stage = self.current_stage.saturating_add(1);
                Ok(())
            }

            fn wait_for_reobservation(&mut self, _: Instant) -> bool {
                false
            }
        }

        #[test]
        fn sidebar_selection_fast_paths_already_selected_and_expands_collapsed_sections_without_search(
        ) {
            let already_open = SessionNavigationWindow {
                pid: 42,
                window: 1,
                selected_urls: vec!["https://127.0.0.1:4321/c/active-cid".into()],
            };
            let mut backend = MockSidebarBackend::new(vec![]);
            drive_sidebar_selection(
                &mut backend,
                &already_open,
                4321,
                "/work/micro-input",
                Some("proj-mi"),
                "active-cid",
                &mut || Ok(()),
                &|| true,
                |validate, is_current| {
                    validate()?;
                    assert!(is_current());
                    Ok(())
                },
            )
            .unwrap();
            assert!(backend.pressed_buttons.is_empty());
            assert!(backend.pressed_links.is_empty());

            let other_open = SessionNavigationWindow {
                pid: 42,
                window: 1,
                selected_urls: vec!["https://127.0.0.1:4321/c/other-cid".into()],
            };

            let collapsed_by_project_id = WindowObservation {
                nodes: vec![
                    node("win", "AXWindow", "", None, None, None, false),
                    node("hdr-a", "AXGroup", "", Some(0), None, None, false),
                    node(
                        "btn-proj-a",
                        "AXButton",
                        "micro-input",
                        Some(1),
                        None,
                        Some(false),
                        true,
                    ),
                    node(
                        "new-proj-a",
                        "AXLink",
                        "New Conversation in Project",
                        Some(1),
                        Some("https://127.0.0.1:4321/?project=proj-a"),
                        None,
                        true,
                    ),
                    node("hdr-b", "AXGroup", "", Some(0), None, None, false),
                    node(
                        "btn-proj-b",
                        "AXButton",
                        "micro-input",
                        Some(4),
                        None,
                        Some(false),
                        true,
                    ),
                    node(
                        "new-proj-b",
                        "AXLink",
                        "New Conversation in Project",
                        Some(4),
                        Some("https://127.0.0.1:4321/?project=proj-b"),
                        None,
                        true,
                    ),
                ],
            };
            let expanded_with_see_all = WindowObservation {
                nodes: vec![
                    node("win", "AXWindow", "", None, None, None, false),
                    node(
                        "btn-proj-other",
                        "AXButton",
                        "other-repo",
                        Some(0),
                        None,
                        Some(true),
                        true,
                    ),
                    node(
                        "see-all-other",
                        "AXButton",
                        "See all (3)",
                        Some(0),
                        None,
                        None,
                        true,
                    ),
                    node("hdr-b", "AXGroup", "", Some(0), None, None, false),
                    node(
                        "btn-proj-b",
                        "AXButton",
                        "micro-input",
                        Some(3),
                        None,
                        Some(true),
                        true,
                    ),
                    node(
                        "new-proj-b",
                        "AXLink",
                        "New Conversation in Project",
                        Some(3),
                        Some("https://127.0.0.1:4321/?project=proj-b"),
                        None,
                        true,
                    ),
                    node(
                        "see-all-b",
                        "AXButton",
                        "See all (8)",
                        Some(0),
                        None,
                        None,
                        true,
                    ),
                ],
            };
            let mounted_target = WindowObservation {
                nodes: vec![
                    node("win", "AXWindow", "", None, None, None, false),
                    node("row", "AXGroup", "", Some(0), None, None, false),
                    node(
                        "link-wanted",
                        "AXLink",
                        "Target Session",
                        Some(1),
                        Some("https://127.0.0.1:4321/c/wanted-uuid"),
                        None,
                        true,
                    ),
                    node(
                        "arch",
                        "AXButton",
                        "Archive conversation",
                        Some(1),
                        None,
                        None,
                        true,
                    ),
                ],
            };
            let mut backend = MockSidebarBackend::new(vec![
                collapsed_by_project_id,
                expanded_with_see_all,
                mounted_target,
            ]);
            drive_sidebar_selection(
                &mut backend,
                &other_open,
                4321,
                "/work/second/micro-input",
                Some("proj-b"),
                "wanted-uuid",
                &mut || Ok(()),
                &|| true,
                |_, _| unreachable!(),
            )
            .unwrap();
            assert_eq!(
                backend.pressed_buttons,
                vec![
                    ("btn-proj-b".to_string(), Some(false)),
                    ("see-all-b".to_string(), None),
                ]
            );
            assert_eq!(backend.pressed_links, vec!["link-wanted".to_string()]);

            let collapsed_basename_only = WindowObservation {
                nodes: vec![
                    node("win", "AXWindow", "", None, None, None, false),
                    node(
                        "toggle-sidebar",
                        "AXButton",
                        "Toggle sidebar",
                        Some(0),
                        None,
                        Some(false),
                        true,
                    ),
                ],
            };
            let after_sidebar_open = WindowObservation {
                nodes: vec![
                    node("win", "AXWindow", "", None, None, None, false),
                    node(
                        "toggle-sidebar",
                        "AXButton",
                        "Toggle sidebar",
                        Some(0),
                        None,
                        Some(true),
                        true,
                    ),
                    node(
                        "btn-basename",
                        "AXButton",
                        "solo-repo",
                        Some(0),
                        None,
                        Some(false),
                        true,
                    ),
                ],
            };
            let after_basename_expand = WindowObservation {
                nodes: vec![
                    node("win", "AXWindow", "", None, None, None, false),
                    node("row-solo", "AXGroup", "", Some(0), None, None, false),
                    node(
                        "link-solo",
                        "AXLink",
                        "Solo Chat",
                        Some(1),
                        Some("https://127.0.0.1:4321/c/solo-uuid"),
                        None,
                        true,
                    ),
                    node(
                        "arch-solo",
                        "AXButton",
                        "Archive conversation",
                        Some(1),
                        None,
                        None,
                        true,
                    ),
                ],
            };
            let mut backend = MockSidebarBackend::new(vec![
                collapsed_basename_only,
                after_sidebar_open,
                after_basename_expand,
            ]);
            drive_sidebar_selection(
                &mut backend,
                &other_open,
                4321,
                "/work/solo-repo",
                None,
                "solo-uuid",
                &mut || Ok(()),
                &|| true,
                |_, _| unreachable!(),
            )
            .unwrap();
            assert_eq!(
                backend.pressed_buttons,
                vec![
                    ("toggle-sidebar".to_string(), Some(false)),
                    ("btn-basename".to_string(), Some(false)),
                ]
            );
            assert_eq!(backend.pressed_links, vec!["link-solo".to_string()]);

            let ambiguous_basenames = WindowObservation {
                nodes: vec![
                    node("win", "AXWindow", "", None, None, None, false),
                    node(
                        "btn-dup-1",
                        "AXButton",
                        "micro-input",
                        Some(0),
                        None,
                        Some(false),
                        true,
                    ),
                    node(
                        "btn-dup-2",
                        "AXButton",
                        "micro-input",
                        Some(0),
                        None,
                        Some(false),
                        true,
                    ),
                ],
            };
            let mut ambiguous_backend = MockSidebarBackend::new(vec![ambiguous_basenames]);
            let err = drive_sidebar_selection(
                &mut ambiguous_backend,
                &other_open,
                4321,
                "/work/dup/micro-input",
                None,
                "missing-uuid",
                &mut || Ok(()),
                &|| true,
                |_, _| unreachable!(),
            )
            .unwrap_err();
            assert_eq!(err, "unconfirmed:session_sidebar_link_unavailable");
            assert!(ambiguous_backend.pressed_buttons.is_empty());
            assert!(ambiguous_backend.pressed_links.is_empty());
        }

        #[test]
        fn sidebar_selection_ignores_unclassified_chat_body_links_and_honors_project_id_collapsed_filter(
        ) {
            let other_open = SessionNavigationWindow {
                pid: 42,
                window: 7,
                selected_urls: vec!["https://127.0.0.1:4321/c/current-uuid".into()],
            };
            let collapsed_with_body_link = WindowObservation {
                nodes: vec![
                    node("win", "AXWindow", "", None, None, None, false),
                    node(
                        "toggle-sidebar",
                        "AXButton",
                        "Toggle sidebar",
                        Some(0),
                        None,
                        Some(false),
                        true,
                    ),
                    node(
                        "chat-body-link",
                        "AXLink",
                        "Reference conversation://target-uuid",
                        Some(0),
                        Some("https://127.0.0.1:4321/c/target-uuid"),
                        None,
                        true,
                    ),
                ],
            };
            let expanded_sidebar = WindowObservation {
                nodes: vec![
                    node("win", "AXWindow", "", None, None, None, false),
                    node(
                        "chat-body-link",
                        "AXLink",
                        "Reference conversation://target-uuid",
                        Some(0),
                        Some("https://127.0.0.1:4321/c/target-uuid"),
                        None,
                        true,
                    ),
                    node("row-sidebar", "AXGroup", "", Some(0), None, None, false),
                    node(
                        "sidebar-link",
                        "AXLink",
                        "Target Chat",
                        Some(2),
                        Some("https://127.0.0.1:4321/c/target-uuid"),
                        None,
                        true,
                    ),
                    node(
                        "arch-btn",
                        "AXButton",
                        "Archive conversation",
                        Some(2),
                        None,
                        None,
                        true,
                    ),
                ],
            };
            let mut backend =
                MockSidebarBackend::new(vec![collapsed_with_body_link, expanded_sidebar]);
            drive_sidebar_selection(
                &mut backend,
                &other_open,
                4321,
                "/work/micro-input",
                Some("proj-1"),
                "target-uuid",
                &mut || Ok(()),
                &|| true,
                |_, _| unreachable!(),
            )
            .unwrap();
            assert_eq!(
                backend.pressed_buttons,
                vec![("toggle-sidebar".to_string(), Some(false))]
            );
            assert_eq!(backend.pressed_links, vec!["sidebar-link".to_string()]);

            let already_expanded_project_obs = WindowObservation {
                nodes: vec![
                    node("win", "AXWindow", "", None, None, None, false),
                    node("hdr-1", "AXGroup", "", Some(0), None, None, false),
                    node(
                        "btn-proj-1",
                        "AXButton",
                        "micro-input",
                        Some(1),
                        None,
                        Some(true),
                        true,
                    ),
                    node(
                        "new-conv-1",
                        "AXLink",
                        "New Conversation",
                        Some(1),
                        Some("https://127.0.0.1:4321/?project=proj-1"),
                        None,
                        true,
                    ),
                    node(
                        "btn-other-same-basename",
                        "AXButton",
                        "micro-input",
                        Some(0),
                        None,
                        Some(false),
                        true,
                    ),
                ],
            };
            assert_eq!(
                locate_target_project_header(
                    &already_expanded_project_obs,
                    4321,
                    "/work/micro-input",
                    Some("proj-1"),
                    true,
                ),
                None
            );
            assert_eq!(
                locate_target_project_header(
                    &already_expanded_project_obs,
                    4321,
                    "/work/micro-input",
                    Some("proj-1"),
                    false,
                ),
                Some(2)
            );

            let duplicate_basename_obs = WindowObservation {
                nodes: vec![
                    node("root", "AXGroup", "", None, None, None, false),
                    node(
                        "picker-btn",
                        "AXButton",
                        "agent-tasks",
                        Some(0),
                        None,
                        Some(false),
                        true,
                    ),
                    node(
                        "picker-show-all",
                        "AXButton",
                        "Show All",
                        Some(0),
                        None,
                        Some(false),
                        true,
                    ),
                    node(
                        "sidebar-project",
                        "AXButton",
                        "agent-tasks",
                        Some(0),
                        None,
                        Some(false),
                        true,
                    ),
                    node(
                        "sidebar-new-conv",
                        "AXLink",
                        "New Conversation in Project",
                        Some(0),
                        None,
                        None,
                        true,
                    ),
                ],
            };
            assert_eq!(
                locate_target_project_header(
                    &duplicate_basename_obs,
                    4321,
                    "/work/agent-tasks",
                    None,
                    true,
                ),
                Some(3)
            );
        }

        #[test]
        fn collapsed_open_sidebar_button_reveals_target_link() {
            let mut backend = MockSidebarBackend::new(vec![
                WindowObservation {
                    nodes: vec![
                        node("root", "AXGroup", "", None, None, None, false),
                        node(
                            "open-sidebar",
                            "AXButton",
                            "Open sidebar",
                            Some(0),
                            None,
                            None,
                            true,
                        ),
                    ],
                },
                WindowObservation {
                    nodes: vec![
                        node("root", "AXGroup", "", None, None, None, false),
                        node("row", "AXGroup", "", Some(0), None, None, false),
                        node(
                            "target-link",
                            "AXLink",
                            "Target",
                            Some(1),
                            Some("https://127.0.0.1:4321/c/target-1"),
                            None,
                            true,
                        ),
                        node(
                            "archive",
                            "AXButton",
                            "Archive conversation",
                            Some(1),
                            None,
                            Some(false),
                            true,
                        ),
                    ],
                },
            ]);
            let target = crate::session_navigation::SessionNavigationWindow {
                pid: 1,
                window: 1,
                selected_urls: vec!["https://127.0.0.1:4321/c/current".into()],
            };
            drive_sidebar_selection(
                &mut backend,
                &target,
                4321,
                "/work/agent-tasks",
                None,
                "target-1",
                &mut || Ok(()),
                &|| true,
                |_, _| Ok(()),
            )
            .unwrap();
            assert_eq!(
                backend.pressed_buttons,
                vec![("open-sidebar".to_string(), None)]
            );
            assert_eq!(backend.pressed_links, vec!["target-link".to_string()]);
        }

        #[test]
        fn accessibility_enablement_falls_back_to_enhanced_ui_when_manual_stays_false() {
            let mut written = Vec::new();
            enable_application_accessibility_with(
                |name| Ok(name == "AXManualAccessibility"),
                |_| Ok(Some(false)),
                |name| {
                    written.push(name.to_string());
                    if name == "AXEnhancedUserInterface" {
                        -25208
                    } else {
                        0
                    }
                },
            )
            .unwrap();
            assert_eq!(
                written,
                vec![
                    "AXManualAccessibility".to_string(),
                    "AXEnhancedUserInterface".to_string()
                ]
            );
        }

        #[test]
        fn codex_window_snapshot_separates_sidebar_active_row_main_pane_and_resume_errors() {
            let n = |parent: Option<usize>,
                     role: &str,
                     title: &str,
                     desc: &str,
                     val: &str,
                     aria: Option<&str>| CodexObservedNode {
                parent,
                role: role.into(),
                title: title.into(),
                description: desc.into(),
                value: val.into(),
                aria_current: aria.map(str::to_owned),
            };
            let nodes = vec![
                n(None, "AXWindow", "Same Title", "", "", None),
                n(Some(0), "AXToolbar", "", "Chat toolbar", "", None),
                n(Some(1), "AXImage", "", "Project: micro-input", "", None),
                n(Some(0), "AXWebArea", "", "", "", None),
                n(Some(3), "AXList", "", "Threads", "", None),
                n(Some(4), "AXButton", "Same Title", "", "", Some("page")),
                n(Some(5), "AXStaticText", "", "", "Active row subtitle", None),
                n(Some(4), "AXButton", "Same Title", "", "", None),
                n(
                    Some(7),
                    "AXStaticText",
                    "",
                    "",
                    "Inactive sibling row text",
                    None,
                ),
                n(Some(3), "AXGroup", "", "Main", "", None),
                n(
                    Some(9),
                    "AXStaticText",
                    "",
                    "",
                    "Main pane target message",
                    None,
                ),
                n(Some(9), "AXList", "", "", "", None),
                n(Some(11), "AXListMarker", "", "", "•", None),
                n(
                    Some(11),
                    "AXStaticText",
                    "",
                    "",
                    "Bullet item inside main pane",
                    None,
                ),
            ];
            let snap = summarize_codex_window("Same Title".into(), &nodes);
            assert_eq!(snap.window_title, "Same Title");
            assert_eq!(snap.toolbar_project.as_deref(), Some("micro-input"));
            assert_eq!(
                snap.active_sidebar_texts,
                vec!["Same Title".to_string(), "Active row subtitle".to_string()]
            );
            assert_eq!(
                snap.main_pane_texts,
                vec![
                    "Main pane target message".to_string(),
                    "Bullet item inside main pane".to_string()
                ]
            );
            assert!(!snap.has_resume_error);

            let err_nodes = vec![
                n(None, "AXWindow", "ChatGPT", "", "", None),
                n(Some(0), "AXWebArea", "", "", "", None),
                n(
                    Some(1),
                    "AXStaticText",
                    "",
                    "",
                    "Failed to resume chat: no rollout found for thread id 0000",
                    None,
                ),
            ];
            let err_snap = summarize_codex_window("ChatGPT".into(), &err_nodes);
            assert!(err_snap.has_resume_error);

            let short_err_snap = summarize_codex_window("Other Title".into(), &err_nodes);
            assert!(short_err_snap.has_resume_error);

            let normal_chat_nodes = vec![
                n(None, "AXWindow", "Debug chat", "", "", None),
                n(Some(0), "AXWebArea", "", "", "", None),
                n(
                    Some(1),
                    "AXStaticText",
                    "",
                    "",
                    "Why did it say Failed to resume chat?",
                    None,
                ),
            ];
            let normal_snap = summarize_codex_window("Debug chat".into(), &normal_chat_nodes);
            assert!(!normal_snap.has_resume_error);

            let attention_nodes = vec![
                n(None, "AXWindow", "Active Thread", "", "", None),
                n(Some(0), "AXToolbar", "", "Chat toolbar", "", None),
                n(Some(1), "AXImage", "", "Project: micro-input", "", None),
                n(Some(0), "AXWebArea", "", "", "", None),
                n(Some(3), "AXList", "", "Threads", "", None),
                n(Some(4), "AXButton", "Active Thread", "", "", Some("page")),
                n(Some(5), "AXStaticText", "", "", "2m", None),
                n(Some(4), "AXButton", "Deploy Staging", "", "", None),
                n(Some(7), "AXStaticText", "", "", "Needs approval", None),
                n(Some(7), "AXStaticText", "", "", "2m", None),
                n(Some(7), "AXStaticText", "", "", "+12 -4", None),
                n(Some(4), "AXButton", "", "", "", None),
                n(Some(11), "AXStaticText", "", "", "迁移数据库", None),
                n(Some(11), "AXStaticText", "", "", "需要批准", None),
                n(Some(11), "AXStaticText", "", "", "just now", None),
                n(Some(11), "AXStaticText", "", "", "+12", None),
                n(Some(4), "AXButton", "Fix needs approval copy", "", "", None),
                n(Some(16), "AXStaticText", "", "", "Running", None),
            ];
            let second_window_nodes = vec![
                n(None, "AXWindow", "Other Window", "", "", None),
                n(Some(0), "AXToolbar", "", "Chat toolbar", "", None),
                n(Some(1), "AXImage", "", "Project: sibling-repo", "", None),
                n(Some(0), "AXWebArea", "", "", "", None),
                n(Some(3), "AXList", "", "Threads", "", None),
                n(Some(4), "AXButton", "Audit Auth", "", "", None),
                n(Some(5), "AXStaticText", "", "", "Needs input", None),
                n(Some(5), "AXStaticText", "", "", "15mins", None),
            ];
            let live_signals = summarize_codex_live_window_signals_multi(
                true,
                &attention_nodes,
                &[second_window_nodes],
            );
            assert!(live_signals.is_frontmost);
            assert_eq!(live_signals.toolbar_project.as_deref(), Some("micro-input"));
            assert_eq!(
                live_signals.active_sidebar_texts,
                vec!["Active Thread".to_string(), "2m".to_string()]
            );
            assert_eq!(
                live_signals.attention_sidebar_titles,
                vec![
                    "Deploy Staging".to_string(),
                    "迁移数据库".to_string(),
                    "Audit Auth".to_string()
                ]
            );
            assert_eq!(
                live_signals.attention_windows,
                vec![
                    (
                        Some("micro-input".to_string()),
                        vec!["Deploy Staging".to_string(), "迁移数据库".to_string()]
                    ),
                    (
                        Some("sibling-repo".to_string()),
                        vec!["Audit Auth".to_string()]
                    )
                ]
            );
        }

        #[test]
        fn read_observed_node_extracts_popup_button_and_menu_item_labels_and_pressability() {
            for (role, label, expected_expanded) in [
                ("AXPopUpButton", "GPT-5.4 Standard", Some(false)),
                ("AXMenuItem", "Select model", None),
            ] {
                let mut pass = 0usize;
                let observed = read_observed_node(
                    ObservationPurpose::Controls,
                    |names| {
                        pass += 1;
                        if pass == 1 {
                            assert_eq!(names, &["AXRole", "AXSubrole", "AXHidden", "AXChildren"]);
                            Ok(vec![Owned::text(role), None, None, None])
                        } else if role == "AXPopUpButton" {
                            assert_eq!(
                                names,
                                &["AXDescription", "AXTitle", "AXEnabled", "AXExpanded"]
                            );
                            Ok(vec![None, Owned::text(label), None, unsafe {
                                Some(Owned(CFRetain(kCFBooleanFalse)))
                            }])
                        } else {
                            assert_eq!(names, &["AXDescription", "AXTitle", "AXEnabled"]);
                            Ok(vec![None, Owned::text(label), None])
                        }
                    },
                    |_| Ok(false),
                    || Ok(vec!["AXPress".to_string()]),
                )
                .unwrap()
                .expect("popup/menu control must be observed");
                assert_eq!(observed.role, role);
                assert_eq!(observed.label, label);
                assert!(observed.enabled);
                assert!(observed.pressable);
                assert_eq!(observed.expanded, expected_expanded);
            }

            let provider = NativeAxProvider {
                target: Some(Target {
                    identity: ManagedWindowIdentity {
                        bundle_id: "com.google.antigravity".into(),
                        pid: 42,
                        window_id: 1,
                        scope_id: 0,
                        title: "Antigravity".into(),
                    },
                    window: CapturedWindow {
                        bundle_id: "com.google.antigravity".into(),
                        pid: 42,
                        application: Owned::text("app").unwrap(),
                        element: Owned::text("win").unwrap(),
                        title: "Antigravity".into(),
                    },
                    scope: Owned::text("win").unwrap(),
                }),
                ..Default::default()
            };
            let trigger = provider
                .control_from_node_data(
                    7,
                    NodeData {
                        role: "AXPopUpButton".into(),
                        label: "Select model, current: Gemini 4 Carbon High".into(),
                        enabled: true,
                        pressable: true,
                        expanded: Some(false),
                        ..Default::default()
                    },
                )
                .expect("Antigravity Select model popup button must be observed");
            assert!(trigger.pressable);
            assert_eq!(trigger.expanded, Some(false));
            for excluded in [
                "Agent permissions",
                "Add context",
                "Display options",
                "Project options",
                "Customize sections",
                "Notifications",
                "Extensions",
            ] {
                assert!(provider
                    .control_from_node_data(
                        8,
                        NodeData {
                            role: "AXPopUpButton".into(),
                            label: excluded.into(),
                            enabled: true,
                            pressable: true,
                            expanded: Some(false),
                            ..Default::default()
                        },
                    )
                    .is_none());
            }
        }

        #[test]
        fn codex_ax_traversal_enforces_depth_and_node_caps() {
            let leaf = Owned::text("leaf").unwrap();
            assert!(collect_codex_nodes(
                Owned::text("wide").unwrap(),
                Instant::now() + Duration::from_secs(1),
                |_| Ok(Some(CodexAxNodeData {
                    role: "AXGroup".into(),
                    children: vec![leaf.clone(); MAX_NODES],
                    ..Default::default()
                }))
            )
            .is_err());
        }

        #[test]
        fn owned_text_returns_none_when_cfstring_create_returns_null() {
            unsafe extern "C" fn null_cfstring(
                _: Ref,
                _: *const u8,
                _: isize,
                _: u32,
                _: u8,
            ) -> Ref {
                std::ptr::null()
            }
            assert!(Owned::text_with("hello", null_cfstring).is_none());
            assert!(Owned::text("hello").is_some());
        }

        #[test]
        fn antigravity_controls_from_observation_identifies_side_question_input() {
            let mut provider = NativeAxProvider {
                target: Some(Target {
                    identity: ManagedWindowIdentity {
                        bundle_id: "com.google.antigravity".into(),
                        pid: 42,
                        window_id: 1,
                        scope_id: 0,
                        title: "Antigravity".into(),
                    },
                    window: CapturedWindow {
                        bundle_id: "com.google.antigravity".into(),
                        pid: 42,
                        application: Owned::text("app").unwrap(),
                        element: Owned::text("win").unwrap(),
                        title: "Antigravity".into(),
                    },
                    scope: Owned::text("win").unwrap(),
                }),
                ..Default::default()
            };

            let observation = WindowObservation {
                nodes: vec![
                    ObservedNode {
                        element: Owned::text("win").unwrap(),
                        parent: None,
                        data: NodeData {
                            role: "AXWindow".into(),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("toggle-btn").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Toggle Side Panel".into(),
                            enabled: true,
                            pressable: true,
                            expanded: Some(true),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("side-panel-group").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXGroup".into(),
                            label: "Side Question".into(),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("side-question-textarea").unwrap(),
                        parent: Some(2),
                        data: NodeData {
                            role: "AXTextArea".into(),
                            label: String::new(),
                            enabled: true,
                            editable: true,
                            ..Default::default()
                        },
                    },
                ],
            };

            let controls = provider.controls_from_observation(observation).unwrap();
            let sq_input = controls
                .iter()
                .find(|c| c.role == "AXTextArea")
                .expect("Side Question textarea must be observed");
            assert_eq!(sq_input.label, "Side Question Input");
        }

        #[test]
        fn read_observed_node_extracts_textfield_and_combobox_labels_for_antigravity_browser() {
            for (role, label, expected_editable, expect_children_kept) in [
                ("AXComboBox", "Address bar", false, true),
                ("AXTextField", "Search for content to open", true, false),
            ] {
                let mut pass = 0usize;
                let observed = read_observed_node(
                    ObservationPurpose::Controls,
                    |names| {
                        pass += 1;
                        if pass == 1 {
                            assert_eq!(names, &["AXRole", "AXSubrole", "AXHidden", "AXChildren"]);
                            Ok(vec![Owned::text(role), None, None, ax_array(&["child"])])
                        } else {
                            assert_eq!(
                                names,
                                &["AXDescription", "AXTitle", "AXEnabled", "AXFocused"]
                            );
                            Ok(vec![Owned::text(label), None, None, None])
                        }
                    },
                    |attr| {
                        assert_eq!(attr, "AXValue");
                        Ok(expected_editable)
                    },
                    || panic!("labeled inputs should not query AXActions"),
                )
                .unwrap()
                .expect("labeled input/combobox must be observed");
                assert_eq!(observed.role, role);
                assert_eq!(observed.label, label);
                assert_eq!(observed.editable, expected_editable);
                assert_eq!(!observed.children.is_empty(), expect_children_kept);
            }

            let make_provider = |bundle_id: &str| NativeAxProvider {
                target: Some(Target {
                    identity: ManagedWindowIdentity {
                        bundle_id: bundle_id.into(),
                        pid: 42,
                        window_id: 1,
                        scope_id: 0,
                        title: "App".into(),
                    },
                    window: CapturedWindow {
                        bundle_id: bundle_id.into(),
                        pid: 42,
                        application: Owned::text("app").unwrap(),
                        element: Owned::text("win").unwrap(),
                        title: "App".into(),
                    },
                    scope: Owned::text("win").unwrap(),
                }),
                ..Default::default()
            };

            let ag = make_provider("com.google.antigravity");
            let address_bar = ag
                .control_from_node_data(
                    1,
                    NodeData {
                        role: "AXComboBox".into(),
                        label: "Address bar".into(),
                        enabled: true,
                        editable: false,
                        ..Default::default()
                    },
                )
                .expect("Antigravity Address bar combobox must be retained even if editable=false");
            assert_eq!(address_bar.label, "Address bar");

            let codex = make_provider("com.openai.codex");
            assert!(
                codex
                    .control_from_node_data(
                        2,
                        NodeData {
                            role: "AXComboBox".into(),
                            label: "Address bar".into(),
                            enabled: true,
                            editable: true,
                            ..Default::default()
                        },
                    )
                    .is_none(),
                "non-Antigravity AXComboBox must not leak into Codex controls"
            );
        }

        #[test]
        fn antigravity_controls_from_observation_filters_settings_code_preview_and_unrelated_clear_search(
        ) {
            let mut provider = NativeAxProvider {
                target: Some(Target {
                    identity: ManagedWindowIdentity {
                        bundle_id: "com.google.antigravity".into(),
                        pid: 42,
                        window_id: 1,
                        scope_id: 0,
                        title: "Antigravity".into(),
                    },
                    window: CapturedWindow {
                        bundle_id: "com.google.antigravity".into(),
                        pid: 42,
                        application: Owned::text("app").unwrap(),
                        element: Owned::text("win").unwrap(),
                        title: "Antigravity".into(),
                    },
                    scope: Owned::text("win").unwrap(),
                }),
                ..Default::default()
            };

            let observation = WindowObservation {
                nodes: vec![
                    ObservedNode {
                        element: Owned::text("win").unwrap(),
                        parent: None,
                        data: NodeData {
                            role: "AXWindow".into(),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("sidebar-clear").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Clear search".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("code-preview").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Preview".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("chat-open-external").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Open in External Browser".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("settings-dialog").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXGroup".into(),
                            label: "Settings".into(),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("settings-browser").unwrap(),
                        parent: Some(4),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Browser".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("new-tab-search-group").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXGroup".into(),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("new-tab-search").unwrap(),
                        parent: Some(6),
                        data: NodeData {
                            role: "AXTextField".into(),
                            label: "Search for content to open".into(),
                            enabled: true,
                            editable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("new-tab-clear").unwrap(),
                        parent: Some(6),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Clear search".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("new-tab-browser").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Browser".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("new-tab-preview").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Preview".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("browser-open-external").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Open in external browser".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("browser-address-bar").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXComboBox".into(),
                            label: "Address bar".into(),
                            enabled: true,
                            editable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("composer-browser-menuitem").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXMenuItem".into(),
                            label: "Browser".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                ],
            };

            let controls = provider.controls_from_observation(observation).unwrap();
            let summary: Vec<(&str, &str)> = controls
                .iter()
                .map(|c| (c.role.as_str(), c.label.as_str()))
                .collect();
            assert_eq!(
                summary,
                vec![
                    ("AXTextField", "Search for content to open"),
                    ("AXButton", "Clear search"),
                    ("AXButton", "Browser"),
                    ("AXButton", "Preview"),
                    ("AXButton", "Open in external browser"),
                    ("AXComboBox", "Address bar"),
                ]
            );
            assert_eq!(
                provider.control_paths.len(),
                controls.len(),
                "control_paths must stay synchronized with filtered controls"
            );
        }

        #[test]
        fn antigravity_controls_observation_prunes_guest_browser_and_catcher_web_areas() {
            let mut visited_forbidden = Vec::new();
            let observation = observe_tree_once(
                Owned::text("window").unwrap(),
                Instant::now() + Duration::from_secs(1),
                |element| {
                    let id = element.string().unwrap();
                    if id.ends_with("-child") {
                        visited_forbidden.push(id.clone());
                    }
                    read_observed_node(
                        ObservationPurpose::AntigravityControls,
                        |names| match (id.as_str(), names) {
                            ("window", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => {
                                Ok(vec![
                                    Owned::text("AXWindow"),
                                    None,
                                    None,
                                    ax_array(&[
                                        "google-wcv",
                                        "catcher-wcv",
                                        "local-http-wcv",
                                        "local-https-foreign-wcv",
                                        "plugin-wcv",
                                        "hub-wcv",
                                        "jetski-bootstrap-wcv",
                                    ]),
                                ])
                            }
                            (
                                "google-wcv"
                                | "catcher-wcv"
                                | "local-http-wcv"
                                | "local-https-foreign-wcv"
                                | "plugin-wcv"
                                | "nested-iframe-wcv",
                                ["AXRole", "AXSubrole", "AXHidden", "AXChildren"],
                            ) => Ok(vec![
                                Owned::text("AXWebArea"),
                                None,
                                None,
                                ax_array(&[&format!("{id}-child")]),
                            ]),
                            ("google-wcv", ["AXURL", "AXTitle"]) => Ok(vec![
                                Owned::text("https://www.google.com/?zx=1"),
                                Owned::text("Google"),
                            ]),
                            ("catcher-wcv", ["AXURL", "AXTitle"]) => Ok(vec![
                                Owned::text("data:text/html,<!doctype html>"),
                                Owned::text(""),
                            ]),
                            ("local-http-wcv", ["AXURL", "AXTitle"]) => Ok(vec![
                                Owned::text("http://localhost:3000/"),
                                Owned::text("Local Dev"),
                            ]),
                            ("local-https-foreign-wcv", ["AXURL", "AXTitle"]) => Ok(vec![
                                Owned::text("https://127.0.0.1:8443/"),
                                Owned::text("User Local HTTPS App"),
                            ]),
                            ("plugin-wcv", ["AXURL", "AXTitle"]) => Ok(vec![
                                Owned::text("plugin://ext-authority/index.html"),
                                Owned::text("Extension"),
                            ]),
                            ("nested-iframe-wcv", ["AXURL", "AXTitle"]) => Ok(vec![
                                Owned::text("https://example.com/preview"),
                                Owned::text("Example"),
                            ]),
                            ("hub-wcv", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => {
                                Ok(vec![
                                    Owned::text("AXWebArea"),
                                    None,
                                    None,
                                    ax_array(&["hub-address-bar", "nested-iframe-wcv"]),
                                ])
                            }
                            ("hub-wcv", ["AXURL", "AXTitle"]) => Ok(vec![
                                Owned::text("https://127.0.0.1:61037/c/conv-1"),
                                Owned::text("My Chat - Antigravity"),
                            ]),
                            (
                                "jetski-bootstrap-wcv",
                                ["AXRole", "AXSubrole", "AXHidden", "AXChildren"],
                            ) => Ok(vec![
                                Owned::text("AXWebArea"),
                                None,
                                None,
                                ax_array(&["bootstrap-toggle"]),
                            ]),
                            ("jetski-bootstrap-wcv", ["AXURL", "AXTitle"]) => Ok(vec![
                                Owned::text("https://127.0.0.1:61037/"),
                                Owned::text("Jetski Web"),
                            ]),
                            (
                                "hub-address-bar",
                                ["AXRole", "AXSubrole", "AXHidden", "AXChildren"],
                            ) => Ok(vec![Owned::text("AXComboBox"), None, None, ax_array(&[])]),
                            (
                                "hub-address-bar",
                                ["AXDescription", "AXTitle", "AXEnabled", "AXFocused"],
                            ) => Ok(vec![
                                Owned::text("Address bar"),
                                None,
                                unsafe { Some(Owned(CFRetain(kCFBooleanTrue))) },
                                unsafe { Some(Owned(CFRetain(kCFBooleanFalse))) },
                            ]),
                            (
                                "bootstrap-toggle",
                                ["AXRole", "AXSubrole", "AXHidden", "AXChildren"],
                            ) => Ok(vec![Owned::text("AXButton"), None, None, ax_array(&[])]),
                            ("bootstrap-toggle", ["AXDescription", "AXTitle", "AXEnabled"]) => {
                                Ok(vec![Owned::text("Toggle Side Panel"), None, unsafe {
                                    Some(Owned(CFRetain(kCFBooleanTrue)))
                                }])
                            }
                            (other, batch) => panic!("unexpected AX read on {other}: {batch:?}"),
                        },
                        |_| Ok(true),
                        || Ok(vec!["AXPress".to_string()]),
                    )
                },
            )
            .unwrap();

            assert!(
                visited_forbidden.is_empty(),
                "guest/catcher/plugin/iframe AXWebArea children must never be traversed, saw: {visited_forbidden:?}"
            );
            let mut provider = NativeAxProvider {
                target: Some(Target {
                    identity: ManagedWindowIdentity {
                        bundle_id: "com.google.antigravity".into(),
                        pid: 42,
                        window_id: 1,
                        scope_id: 0,
                        title: "Antigravity".into(),
                    },
                    window: CapturedWindow {
                        bundle_id: "com.google.antigravity".into(),
                        pid: 42,
                        application: Owned::text("app").unwrap(),
                        element: Owned::text("window").unwrap(),
                        title: "Antigravity".into(),
                    },
                    scope: Owned::text("window").unwrap(),
                }),
                ..Default::default()
            };
            let controls = provider.controls_from_observation(observation).unwrap();
            let summary: Vec<(&str, &str)> = controls
                .iter()
                .map(|c| (c.role.as_str(), c.label.as_str()))
                .collect();
            assert_eq!(
                summary,
                vec![
                    ("AXComboBox", "Address bar"),
                    ("AXButton", "Toggle Side Panel"),
                ]
            );
        }

        #[test]
        fn antigravity_controls_filter_blocks_settings_browser_button_beyond_depth_16() {
            let mut provider = NativeAxProvider {
                target: Some(Target {
                    identity: ManagedWindowIdentity {
                        bundle_id: "com.google.antigravity".into(),
                        pid: 42,
                        window_id: 1,
                        scope_id: 0,
                        title: "Antigravity".into(),
                    },
                    window: CapturedWindow {
                        bundle_id: "com.google.antigravity".into(),
                        pid: 42,
                        application: Owned::text("app").unwrap(),
                        element: Owned::text("win").unwrap(),
                        title: "Antigravity".into(),
                    },
                    scope: Owned::text("win").unwrap(),
                }),
                ..Default::default()
            };

            let mut nodes = vec![
                ObservedNode {
                    element: Owned::text("win").unwrap(),
                    parent: None,
                    data: NodeData {
                        role: "AXWindow".into(),
                        ..Default::default()
                    },
                },
                ObservedNode {
                    element: Owned::text("settings-dialog").unwrap(),
                    parent: Some(0),
                    data: NodeData {
                        role: "AXGroup".into(),
                        label: "Settings".into(),
                        ..Default::default()
                    },
                },
            ];
            for depth in 0..20 {
                let parent = nodes.len() - 1;
                nodes.push(ObservedNode {
                    element: Owned::text(&format!("nest-{depth}")).unwrap(),
                    parent: Some(parent),
                    data: NodeData {
                        role: "AXGroup".into(),
                        ..Default::default()
                    },
                });
            }
            let deep_parent = nodes.len() - 1;
            nodes.push(ObservedNode {
                element: Owned::text("deep-settings-browser").unwrap(),
                parent: Some(deep_parent),
                data: NodeData {
                    role: "AXButton".into(),
                    label: "Browser".into(),
                    enabled: true,
                    pressable: true,
                    ..Default::default()
                },
            });

            let controls = provider
                .controls_from_observation(WindowObservation { nodes })
                .unwrap();
            assert!(
                controls.is_empty(),
                "Settings Browser button nested >16 levels deep must still be filtered out, got: {controls:?}"
            );
        }

        #[test]
        fn antigravity_controls_filter_blocks_chat_transcript_and_settings_browser_false_positives()
        {
            let mut provider = NativeAxProvider {
                target: Some(Target {
                    identity: ManagedWindowIdentity {
                        bundle_id: "com.google.antigravity".into(),
                        pid: 42,
                        window_id: 1,
                        scope_id: 0,
                        title: "Antigravity".into(),
                    },
                    window: CapturedWindow {
                        bundle_id: "com.google.antigravity".into(),
                        pid: 42,
                        application: Owned::text("app").unwrap(),
                        element: Owned::text("win").unwrap(),
                        title: "Antigravity".into(),
                    },
                    scope: Owned::text("win").unwrap(),
                }),
                ..Default::default()
            };

            let observation = WindowObservation {
                nodes: vec![
                    ObservedNode {
                        element: Owned::text("win").unwrap(),
                        parent: None,
                        data: NodeData {
                            role: "AXWindow".into(),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("new-tab-search").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXTextField".into(),
                            label: "Search for content to open".into(),
                            enabled: true,
                            editable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("user-msg").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXList".into(),
                            label: "User message".into(),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("user-clear-search").unwrap(),
                        parent: Some(2),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Clear search".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("user-browser").unwrap(),
                        parent: Some(2),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Browser".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("conv-turns").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXGroup".into(),
                            label: "Conversation turns".into(),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("turns-browser").unwrap(),
                        parent: Some(5),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Browser".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("turns-address-bar").unwrap(),
                        parent: Some(5),
                        data: NodeData {
                            role: "AXComboBox".into(),
                            label: "Address bar".into(),
                            enabled: true,
                            editable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("turns-subagent").unwrap(),
                        parent: Some(5),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Open subagent: worker-1".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("agent-resp").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXScrollArea".into(),
                            label: "Agent response".into(),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("agent-preview").unwrap(),
                        parent: Some(9),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Preview".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("agent-open-external").unwrap(),
                        parent: Some(9),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Open in external browser".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("agent-address-bar").unwrap(),
                        parent: Some(9),
                        data: NodeData {
                            role: "AXTextField".into(),
                            label: "Address bar".into(),
                            enabled: true,
                            editable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("settings-modal").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXDialog".into(),
                            label: "Settings".into(),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("settings-open-external").unwrap(),
                        parent: Some(13),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Open in external browser".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("settings-address-bar").unwrap(),
                        parent: Some(13),
                        data: NodeData {
                            role: "AXComboBox".into(),
                            label: "Address bar".into(),
                            enabled: true,
                            editable: false,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("side-panel-browser").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Browser".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("side-panel-open-external").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Open in external browser".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("side-panel-address-bar").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXComboBox".into(),
                            label: "Address bar".into(),
                            enabled: true,
                            editable: true,
                            ..Default::default()
                        },
                    },
                ],
            };

            let controls = provider.controls_from_observation(observation).unwrap();
            let summary: Vec<(&str, &str)> = controls
                .iter()
                .map(|c| (c.role.as_str(), c.label.as_str()))
                .collect();
            assert_eq!(
                summary,
                vec![
                    ("AXTextField", "Search for content to open"),
                    ("AXButton", "Open subagent: worker-1"),
                    ("AXScrollArea", "Agent response"),
                    ("AXButton", "Browser"),
                    ("AXButton", "Open in external browser"),
                    ("AXComboBox", "Address bar"),
                ]
            );
            assert_eq!(
                provider.control_paths.len(),
                controls.len(),
                "control_paths must remain synchronized after chat/settings filtering"
            );
        }

        #[test]
        fn antigravity_controls_filter_blocks_unlabeled_heading_anchored_chat_log_and_markdown_preview(
        ) {
            let mut provider = NativeAxProvider {
                target: Some(Target {
                    identity: ManagedWindowIdentity {
                        bundle_id: "com.google.antigravity".into(),
                        pid: 42,
                        window_id: 1,
                        scope_id: 0,
                        title: "Antigravity".into(),
                    },
                    window: CapturedWindow {
                        bundle_id: "com.google.antigravity".into(),
                        pid: 42,
                        application: Owned::text("app").unwrap(),
                        element: Owned::text("win").unwrap(),
                        title: "Antigravity".into(),
                    },
                    scope: Owned::text("win").unwrap(),
                }),
                ..Default::default()
            };

            let observation = WindowObservation {
                nodes: vec![
                    ObservedNode {
                        element: Owned::text("win").unwrap(),
                        parent: None,
                        data: NodeData {
                            role: "AXWindow".into(),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("hub-web").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXWebArea".into(),
                            url: Some("https://127.0.0.1:61037/c/test".into()),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("new-tab-search").unwrap(),
                        parent: Some(1),
                        data: NodeData {
                            role: "AXTextField".into(),
                            label: "Search for content to open".into(),
                            enabled: true,
                            editable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("chat-log-group").unwrap(),
                        parent: Some(1),
                        data: NodeData {
                            role: "AXGroup".into(),
                            label: String::new(),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("heading-conv-log").unwrap(),
                        parent: Some(3),
                        data: NodeData {
                            role: "AXHeading".into(),
                            label: "Conversation Log".into(),
                            enabled: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("heading-agent-said").unwrap(),
                        parent: Some(3),
                        data: NodeData {
                            role: "AXHeading".into(),
                            label: "Agent said".into(),
                            enabled: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("chat-clear-search").unwrap(),
                        parent: Some(3),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Clear search".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("chat-step-group").unwrap(),
                        parent: Some(3),
                        data: NodeData {
                            role: "AXGroup".into(),
                            label: String::new(),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("chat-browser").unwrap(),
                        parent: Some(7),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Browser".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("chat-open-external").unwrap(),
                        parent: Some(7),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Open in external browser".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("chat-address-bar").unwrap(),
                        parent: Some(7),
                        data: NodeData {
                            role: "AXComboBox".into(),
                            label: "Address bar".into(),
                            enabled: true,
                            editable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("chat-open-subagent").unwrap(),
                        parent: Some(7),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Open subagent: reviewer-1".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("subagent-turn-group").unwrap(),
                        parent: Some(1),
                        data: NodeData {
                            role: "AXGroup".into(),
                            label: String::new(),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("heading-agent-working").unwrap(),
                        parent: Some(12),
                        data: NodeData {
                            role: "AXHeading".into(),
                            label: "Agent working".into(),
                            enabled: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("subagent-browser").unwrap(),
                        parent: Some(12),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Browser".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("subagent-preview").unwrap(),
                        parent: Some(12),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Preview".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("markdown-view-mode").unwrap(),
                        parent: Some(1),
                        data: NodeData {
                            role: "AXGroup".into(),
                            label: "Markdown view mode".into(),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("code-block-preview").unwrap(),
                        parent: Some(16),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Preview".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("side-panel-preview").unwrap(),
                        parent: Some(1),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Preview".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("side-panel-browser").unwrap(),
                        parent: Some(1),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Browser".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                ],
            };

            let controls = provider.controls_from_observation(observation).unwrap();
            let summary: Vec<(&str, &str)> = controls
                .iter()
                .map(|c| (c.role.as_str(), c.label.as_str()))
                .collect();
            assert_eq!(
                summary,
                vec![
                    ("AXTextField", "Search for content to open"),
                    ("AXHeading", "Conversation Log"),
                    ("AXButton", "Open subagent: reviewer-1"),
                    ("AXButton", "Preview"),
                    ("AXButton", "Browser"),
                ]
            );
            assert_eq!(
                provider.control_paths.len(),
                controls.len(),
                "control_paths must remain synchronized after heading-anchored chat filtering"
            );

            let guarded_obs = WindowObservation {
                nodes: vec![
                    ObservedNode {
                        element: Owned::text("win").unwrap(),
                        parent: None,
                        data: NodeData {
                            role: "AXWindow".into(),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("hub-web").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXWebArea".into(),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("direct-heading").unwrap(),
                        parent: Some(1),
                        data: NodeData {
                            role: "AXHeading".into(),
                            label: "Conversation Log".into(),
                            enabled: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("legit-browser").unwrap(),
                        parent: Some(1),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Browser".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                ],
            };
            let guarded_controls = provider.controls_from_observation(guarded_obs).unwrap();
            assert!(
                guarded_controls
                    .iter()
                    .any(|c| c.role == "AXButton" && c.label == "Browser"),
                "AXWebArea must never be treated as a chat transcript container: {guarded_controls:?}"
            );
        }

        #[test]
        fn controls_from_observation_for_bundle_matches_antigravity_pruning_without_provider_state()
        {
            let ag_observation = WindowObservation {
                nodes: vec![
                    ObservedNode {
                        element: Owned::text("ag-win").unwrap(),
                        parent: None,
                        data: NodeData {
                            role: "AXWindow".into(),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("chat-container").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXGroup".into(),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("chat-heading").unwrap(),
                        parent: Some(1),
                        data: NodeData {
                            role: "AXHeading".into(),
                            label: "Conversation Log".into(),
                            enabled: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("chat-browser").unwrap(),
                        parent: Some(1),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Browser".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("settings-dialog").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXGroup".into(),
                            label: "Settings".into(),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("settings-browser").unwrap(),
                        parent: Some(4),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Browser".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("voice-idle").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Record voice memo".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("side-panel-browser").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Browser".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                ],
            };

            let pure_controls = NativeAxProvider::controls_from_observation_for_bundle(
                &ag_observation,
                "com.google.antigravity",
            );
            let pure_summary: Vec<(&str, &str)> = pure_controls
                .iter()
                .map(|c| (c.role.as_str(), c.label.as_str()))
                .collect();
            assert_eq!(
                pure_summary,
                vec![
                    ("AXHeading", "Conversation Log"),
                    ("AXButton", "Record voice memo"),
                    ("AXButton", "Browser"),
                ]
            );

            let mut ag_provider = NativeAxProvider {
                target: Some(Target {
                    identity: ManagedWindowIdentity {
                        bundle_id: "com.google.antigravity".into(),
                        pid: 42,
                        window_id: 1,
                        scope_id: 0,
                        title: "Antigravity".into(),
                    },
                    window: CapturedWindow {
                        bundle_id: "com.google.antigravity".into(),
                        pid: 42,
                        application: Owned::text("ag-app").unwrap(),
                        element: Owned::text("ag-win").unwrap(),
                        title: "Antigravity".into(),
                    },
                    scope: Owned::text("ag-win").unwrap(),
                }),
                ..Default::default()
            };
            let stateful_controls = ag_provider
                .controls_from_observation(ag_observation)
                .unwrap();
            let stateful_summary: Vec<(&str, &str)> = stateful_controls
                .iter()
                .map(|c| (c.role.as_str(), c.label.as_str()))
                .collect();
            assert_eq!(pure_summary, stateful_summary);

            let codex_observation = WindowObservation {
                nodes: vec![
                    ObservedNode {
                        element: Owned::text("codex-win").unwrap(),
                        parent: None,
                        data: NodeData {
                            role: "AXWindow".into(),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("codex-finishing").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "Finishing dictation".into(),
                            enabled: false,
                            pressable: false,
                            ..Default::default()
                        },
                    },
                ],
            };
            let codex_controls = NativeAxProvider::controls_from_observation_for_bundle(
                &codex_observation,
                "com.openai.codex",
            );
            assert_eq!(codex_controls.len(), 1);
            assert_eq!(codex_controls[0].label, "Finishing dictation");
        }

        #[test]
        fn antigravity_chinese_controls_filter_retains_controls_and_excludes_transcript_and_settings(
        ) {
            let observation = WindowObservation {
                nodes: vec![
                    ObservedNode {
                        element: Owned::text("win").unwrap(),
                        parent: None,
                        data: NodeData {
                            role: "AXWindow".into(),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("chat-group").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXGroup".into(),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("chat-heading").unwrap(),
                        parent: Some(1),
                        data: NodeData {
                            role: "AXHeading".into(),
                            label: "会话日志".into(),
                            enabled: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("chat-browser-false-pos").unwrap(),
                        parent: Some(1),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "内置浏览器".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("agent-reply-group").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXGroup".into(),
                            label: "智能体回复".into(),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("reply-preview-false-pos").unwrap(),
                        parent: Some(4),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "预览".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("settings-group").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXGroup".into(),
                            label: "系统设置".into(),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("settings-browser-false-pos").unwrap(),
                        parent: Some(6),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "内置浏览器".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("voice-start").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "录制语音输入".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("live-voice").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "开启语音模式".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("model-picker").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "模型，当前：Gemini 4 Neon High".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("toggle-panel").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "切换侧边面板".into(),
                            enabled: true,
                            pressable: true,
                            expanded: Some(true),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("side-browser").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "内置浏览器".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("address-bar").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXComboBox".into(),
                            label: "地址栏".into(),
                            enabled: true,
                            editable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("search-group").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXGroup".into(),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("new-tab-search").unwrap(),
                        parent: Some(14),
                        data: NodeData {
                            role: "AXTextField".into(),
                            label: "搜索要打开的内容".into(),
                            enabled: true,
                            editable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("clear-search").unwrap(),
                        parent: Some(14),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "清除搜索".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("new-tab-preview").unwrap(),
                        parent: Some(14),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "预览".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("side-question-btn").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXButton".into(),
                            label: "旁路提问".into(),
                            enabled: true,
                            pressable: true,
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("side-question-group").unwrap(),
                        parent: Some(0),
                        data: NodeData {
                            role: "AXGroup".into(),
                            label: "旁路提问".into(),
                            ..Default::default()
                        },
                    },
                    ObservedNode {
                        element: Owned::text("side-question-input").unwrap(),
                        parent: Some(19),
                        data: NodeData {
                            role: "AXTextArea".into(),
                            enabled: true,
                            editable: true,
                            ..Default::default()
                        },
                    },
                ],
            };

            let controls = NativeAxProvider::controls_from_observation_for_bundle(
                &observation,
                "com.google.antigravity",
            );
            let labels: Vec<(&str, &str)> = controls
                .iter()
                .map(|c| (c.role.as_str(), c.label.as_str()))
                .collect();
            assert_eq!(
                labels,
                vec![
                    ("AXHeading", "会话日志"),
                    ("AXButton", "录制语音输入"),
                    ("AXButton", "开启语音模式"),
                    ("AXButton", "模型，当前：Gemini 4 Neon High"),
                    ("AXButton", "切换侧边面板"),
                    ("AXButton", "内置浏览器"),
                    ("AXComboBox", "地址栏"),
                    ("AXTextField", "搜索要打开的内容"),
                    ("AXButton", "清除搜索"),
                    ("AXButton", "预览"),
                    ("AXButton", "旁路提问"),
                    ("AXTextArea", "Side Question Input"),
                ]
            );
        }

        #[test]
        fn antigravity_chinese_sidebar_observation_classifies_states_and_finds_controls() {
            let sidebar = WindowObservation {
                nodes: vec![
                    node("win", "AXWindow", "", None, None, None, false),
                    node(
                        "toggle-sb",
                        "AXButton",
                        "切换侧边栏",
                        Some(0),
                        None,
                        Some(false),
                        true,
                    ),
                    node(
                        "sec-proj",
                        "AXButton",
                        "项目管理",
                        Some(0),
                        None,
                        None,
                        true,
                    ),
                    node(
                        "proj-hdr",
                        "AXButton",
                        "micro-input",
                        Some(0),
                        None,
                        Some(true),
                        true,
                    ),
                    node(
                        "proj-opt",
                        "AXButton",
                        "项目选项",
                        Some(0),
                        None,
                        None,
                        true,
                    ),
                    node("list", "AXList", "", Some(0), None, None, false),
                    node("row-run", "AXGroup", "", Some(5), None, None, false),
                    node(
                        "link-run",
                        "AXLink",
                        "运行中会话",
                        Some(6),
                        Some("https://127.0.0.1:4321/c/uuid-zh-run"),
                        None,
                        true,
                    ),
                    node(
                        "btn-stop",
                        "AXButton",
                        "停止执行",
                        Some(6),
                        None,
                        None,
                        true,
                    ),
                    node("row-unread", "AXGroup", "", Some(5), None, None, false),
                    node(
                        "link-unread",
                        "AXLink",
                        "未读会话",
                        Some(9),
                        Some("https://127.0.0.1:4321/c/uuid-zh-unread"),
                        None,
                        true,
                    ),
                    node(
                        "btn-arch-u",
                        "AXButton",
                        "归档会话",
                        Some(9),
                        None,
                        None,
                        true,
                    ),
                    node("row-read", "AXGroup", "", Some(5), None, None, false),
                    node(
                        "link-read",
                        "AXLink",
                        "已读会话",
                        Some(12),
                        Some("https://127.0.0.1:4321/c/uuid-zh-read"),
                        None,
                        true,
                    ),
                    node(
                        "btn-arch-r",
                        "AXButton",
                        "归档会话",
                        Some(12),
                        None,
                        None,
                        true,
                    ),
                    node(
                        "ts-read",
                        "AXStaticText",
                        "2 分钟前",
                        Some(12),
                        None,
                        None,
                        false,
                    ),
                    node(
                        "see-all",
                        "AXButton",
                        "查看全部 (8)",
                        Some(0),
                        None,
                        None,
                        true,
                    ),
                ],
            };

            let rows = observe_sidebar_conversations(&sidebar, 4321);
            assert_eq!(rows.len(), 3);
            assert_eq!(rows[0].conversation_id, "uuid-zh-run");
            assert_eq!(
                rows[0].state,
                crate::session_navigation::SidebarRowVisualState::InProgress
            );
            assert_eq!(rows[1].conversation_id, "uuid-zh-unread");
            assert_eq!(
                rows[1].state,
                crate::session_navigation::SidebarRowVisualState::UnreadIdle
            );
            assert_eq!(rows[2].conversation_id, "uuid-zh-read");
            assert_eq!(
                rows[2].state,
                crate::session_navigation::SidebarRowVisualState::ReadIdle
            );

            assert_eq!(find_collapsed_toggle_sidebar_button(&sidebar), Some(1));
            assert_eq!(
                find_project_see_all_button(&sidebar, 4321, "/tmp/micro-input", None),
                Some(16)
            );
        }
    }
}

#[cfg(not(target_os = "macos"))]
#[derive(Default)]
pub struct NativeSessionNavigation;
#[cfg(not(target_os = "macos"))]
impl NativeSessionNavigation {
    pub fn new(_port: u16, _workspace_path: impl Into<String>, _project_id: Option<&str>) -> Self {
        Self
    }
}
#[cfg(not(target_os = "macos"))]
impl crate::session_navigation::SessionNavigationProvider for NativeSessionNavigation {
    fn capture(
        &mut self,
        _: i32,
    ) -> Result<crate::session_navigation::SessionNavigationWindow, String> {
        Err("blocked:macos_required".into())
    }
    fn read(
        &mut self,
        _: &crate::session_navigation::SessionNavigationWindow,
    ) -> Result<crate::session_navigation::SessionNavigationWindow, String> {
        Err("blocked:macos_required".into())
    }
    fn select_conversation(
        &mut self,
        _: &crate::session_navigation::SessionNavigationWindow,
        _: &str,
        _: &mut dyn FnMut() -> Result<(), String>,
        _: &dyn Fn() -> bool,
    ) -> Result<(), String> {
        Err("blocked:macos_required".into())
    }
}
