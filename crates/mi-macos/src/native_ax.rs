use crate::executor::DialScrollRequest;
use crate::managed_controls::{blocked, ManagedAxProvider, ManagedWindow, ManagedWindowIdentity};

#[cfg(not(target_os = "macos"))]
#[derive(Debug, Default)]
pub struct NativeAxProvider;

#[cfg(not(target_os = "macos"))]
impl ManagedAxProvider for NativeAxProvider {
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
}

#[cfg(target_os = "macos")]
pub use macos::{NativeAxProvider, NativeSessionNavigation};

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
        static kCFTypeArrayCallBacks: u8;
    }

    #[derive(Debug)]
    struct Owned(Ref);
    // AX references are retained Core Foundation objects. The provider is only
    // accessed under ManagedAxControls' mutex, including when its thread changes.
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
        fn text(value: &str) -> Self {
            unsafe {
                Self(CFStringCreateWithBytes(
                    std::ptr::null(),
                    value.as_ptr(),
                    value.len() as isize,
                    UTF8,
                    0,
                ))
            }
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
            let names: Vec<_> = names.iter().map(|name| Self::text(name)).collect();
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
            } == 0
            {
                return None;
            }
            let end = buffer.iter().position(|byte| *byte == 0)?;
            std::str::from_utf8(&buffer[..end]).ok().map(str::to_owned)
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
        fn url(&self) -> Result<String, String> {
            if unsafe { CFGetTypeID(self.0) == CFURLGetTypeID() } {
                let text = unsafe { CFURLGetString(self.0) };
                if text.is_null() {
                    return Err(blocked("ax_url_unavailable"));
                }
                unsafe { Self(CFRetain(text)) }
                    .string()
                    .ok_or_else(|| blocked("ax_url_type_or_limit"))
            } else {
                self.string().ok_or_else(|| blocked("ax_url_type_or_limit"))
            }
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
            let name = Self::text(name);
            let mut value = 0u8;
            let status = unsafe { AXUIElementIsAttributeSettable(self.0, name.0, &mut value) };
            Ok(attribute_error(status)?.is_some() && value != 0)
        }
        fn set(&self, name: &str, value: Ref) -> Result<(), String> {
            let name = Self::text(name);
            let code = unsafe { AXUIElementSetAttributeValue(self.0, name.0, value) };
            if code == 0 {
                Ok(())
            } else {
                Err(format!("unconfirmed:ax_set_{code}"))
            }
        }
        fn action(&self, action: &str) -> Result<(), String> {
            let action = Self::text(action);
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
        Sessions,
        SessionDocuments,
        SessionSearch,
    }

    #[derive(Default)]
    struct NodeData {
        role: String,
        subrole: String,
        children: Vec<Owned>,
        label: String,
        enabled: bool,
        pressable: bool,
        editable: bool,
        focused: bool,
        url: Option<String>,
    }
    struct ObservedNode {
        element: Owned,
        parent: Option<usize>,
        data: NodeData,
    }
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
        if hidden || (role == "AXStaticText" && purpose != ObservationPurpose::SessionSearch) {
            return Ok(None);
        }
        let input = matches!(
            role.as_str(),
            "AXTextArea" | "AXTextField" | "AXSecureTextField"
        ) || (purpose != ObservationPurpose::Controls && role == "AXComboBox");
        let button = matches!(
            role.as_str(),
            "AXButton" | "AXCheckBox" | "AXRadioButton" | "AXSwitch" | "AXToggleButton" | "AXLink"
        );
        let children = if input {
            Vec::new()
        } else {
            raw_children
                .map(|value| value.array())
                .transpose()?
                .unwrap_or_default()
        };
        let names: &[&str] = match purpose {
            ObservationPurpose::Controls if input => &["AXEnabled", "AXFocused"],
            ObservationPurpose::Controls if button || role == "AXHeading" => {
                &["AXDescription", "AXTitle", "AXEnabled"]
            }
            ObservationPurpose::Sessions | ObservationPurpose::SessionSearch
                if role == "AXComboBox" =>
            {
                &["AXPlaceholderValue", "AXEnabled"]
            }
            ObservationPurpose::Sessions | ObservationPurpose::SessionDocuments
                if role == "AXWebArea" =>
            {
                &["AXURL"]
            }
            ObservationPurpose::Sessions if role == "AXButton" => {
                &["AXDescription", "AXTitle", "AXEnabled"]
            }
            ObservationPurpose::SessionSearch if role == "AXStaticText" => {
                &["AXValue", "AXEnabled"]
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
            subrole: subrole.clone().unwrap_or_default(),
            children,
            enabled: true,
            ..Default::default()
        };
        match purpose {
            ObservationPurpose::Controls if input => {
                data.enabled = bool_attribute(attrs.next().flatten())?.unwrap_or(true);
                data.focused = bool_attribute(attrs.next().flatten())?.unwrap_or(false);
                data.editable = data.role != "AXSecureTextField"
                    && subrole.as_deref() != Some("AXSecureTextField")
                    && settable("AXValue")?;
            }
            ObservationPurpose::Controls if button || data.role == "AXHeading" => {
                data.label = text_attribute(attrs.next().flatten())?
                    .filter(|s| !s.trim().is_empty())
                    .or(text_attribute(attrs.next().flatten())?)
                    .unwrap_or_default();
                data.enabled = bool_attribute(attrs.next().flatten())?.unwrap_or(true);
                data.pressable = button && actions()?.iter().any(|action| action == "AXPress");
            }
            ObservationPurpose::Sessions | ObservationPurpose::SessionSearch
                if data.role == "AXComboBox" =>
            {
                data.label = text_attribute(attrs.next().flatten())?.unwrap_or_default();
                data.enabled = bool_attribute(attrs.next().flatten())?.unwrap_or(true);
                data.editable = settable("AXValue")?;
            }
            ObservationPurpose::Sessions if data.role == "AXButton" => {
                data.label = text_attribute(attrs.next().flatten())?
                    .filter(|s| !s.trim().is_empty())
                    .or(text_attribute(attrs.next().flatten())?)
                    .unwrap_or_default();
                data.enabled = bool_attribute(attrs.next().flatten())?.unwrap_or(true);
                data.pressable = actions()?.iter().any(|action| action == "AXPress");
            }
            ObservationPurpose::SessionSearch if data.role == "AXStaticText" => {
                // This purpose is used only inside the identified search
                // dialog. Conversation-body text is never read here.
                data.label = text_attribute(attrs.next().flatten())?.unwrap_or_default();
                data.enabled = bool_attribute(attrs.next().flatten())?.unwrap_or(true);
                data.pressable = matches!(data.children.len(), 5 | 6)
                    && actions()?.iter().any(|action| action == "AXPress");
            }
            ObservationPurpose::Sessions | ObservationPurpose::SessionDocuments
                if data.role == "AXWebArea" =>
            {
                data.url = attrs
                    .next()
                    .flatten()
                    .map(|value| value.url())
                    .transpose()?;
                if purpose == ObservationPurpose::SessionDocuments
                    && data.url.as_deref().is_some_and(|url| !url.is_empty())
                {
                    data.children.clear();
                }
            }
            _ => {}
        }
        Ok(Some(data))
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
            if application.settable("AXManualAccessibility")? {
                application.set("AXManualAccessibility", unsafe { kCFBooleanTrue })?;
            }
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
        fn validate(&self, expected_title: Option<&str>) -> Result<(), String> {
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
            let title = text_attribute(attrs.next().flatten())?.unwrap_or_default();
            if expected_title.is_some_and(|expected| expected != title) {
                return Err(blocked("target_window_changed"));
            }
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    pub struct NativeSessionNavigation {
        window: Option<CapturedWindow>,
    }
    struct SessionSearchDialog {
        root: Owned,
        input: Owned,
    }
    struct SessionSearchSelection {
        dialog: SessionSearchDialog,
        window: Owned,
        row: Owned,
        fields: Vec<Owned>,
    }
    impl SessionSearchSelection {
        fn verify(&self, conversation: &str, deadline: Instant) -> Result<(), String> {
            self.verify_with(conversation, deadline, |element, names, deadline| {
                element.attributes(names, deadline)
            })
        }
        fn verify_with(
            &self,
            conversation: &str,
            deadline: Instant,
            mut read: impl FnMut(&Owned, &[&str], Instant) -> Result<Vec<Option<Owned>>, String>,
        ) -> Result<(), String> {
            // A slow catalog refresh must be followed only by bounded reads of
            // the retained selection, never another full window traversal.
            let deadline = deadline.min(Instant::now() + Duration::from_millis(400));
            for element in [&self.dialog.root, &self.dialog.input, &self.row] {
                let window = read(element, &["AXWindow"], deadline)?
                    .pop()
                    .flatten()
                    .ok_or_else(|| blocked("session_search_window_missing"))?;
                if unsafe { CFEqual(window.0, self.window.0) } == 0 {
                    return Err(blocked("session_search_window_changed"));
                }
            }
            let mut attrs =
                read(&self.row, &["AXRole", "AXEnabled", "AXChildren"], deadline)?.into_iter();
            if text_attribute(attrs.next().flatten())?.as_deref() != Some("AXStaticText")
                || bool_attribute(attrs.next().flatten())? == Some(false)
            {
                return Err(blocked("session_search_result_changed"));
            }
            let fields = attrs
                .next()
                .flatten()
                .ok_or_else(|| blocked("session_search_result_changed"))?
                .array()?;
            if fields.len() != self.fields.len()
                || fields
                    .iter()
                    .zip(&self.fields)
                    .any(|(a, b)| unsafe { CFEqual(a.0, b.0) } == 0)
            {
                return Err(blocked("session_search_result_changed"));
            }
            let query = read(&self.dialog.input, &["AXValue"], deadline)?
                .pop()
                .flatten();
            if text_attribute(query)?.as_deref() != Some(conversation) {
                return Err(blocked("session_search_query_changed_final_guard"));
            }
            let identity = read(&self.fields[3], &["AXValue"], deadline)?
                .pop()
                .flatten();
            if text_attribute(identity)?.as_deref() != Some(conversation) {
                return Err(blocked("session_search_identity_changed"));
            }
            Ok(())
        }
    }
    impl NativeSessionNavigation {
        fn observe(&self, pid: i32) -> Result<WindowObservation, String> {
            self.observe_until(pid, Instant::now() + Duration::from_secs(2))
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

        fn search_dialog(
            &self,
            pid: i32,
            deadline: Instant,
        ) -> Result<Option<SessionSearchDialog>, String> {
            let observation = self.observe_until(pid, deadline)?;
            search_controls(&observation).map(|found| {
                found.map(|(dialog, input)| SessionSearchDialog {
                    root: observation.nodes[dialog].element.clone(),
                    input: observation.nodes[input].element.clone(),
                })
            })
        }

        fn observe_search_dialog(
            &self,
            dialog: &SessionSearchDialog,
            deadline: Instant,
        ) -> Result<WindowObservation, String> {
            let window = self
                .window
                .as_ref()
                .ok_or_else(|| blocked("session_window_missing"))?;
            window.validate(None)?;
            for element in [&dialog.root, &dialog.input] {
                let attached = element
                    .attributes(&["AXWindow"], deadline)?
                    .pop()
                    .flatten()
                    .ok_or_else(|| blocked("session_search_closed"))?;
                if unsafe { CFEqual(attached.0, window.element.0) } == 0 {
                    return Err(blocked("session_search_changed"));
                }
            }
            let results = observe_window_until(
                dialog.root.clone(),
                ObservationPurpose::SessionSearch,
                deadline,
            )?;
            window.validate(None)?;
            match search_controls(&results)? {
                None => Err(blocked("session_search_closed")),
                Some((0, input))
                    if unsafe { CFEqual(results.nodes[input].element.0, dialog.input.0) } != 0 =>
                {
                    Ok(results)
                }
                Some(_) => Err(blocked("session_search_changed")),
            }
        }

        fn search_result(
            &self,
            current: &SessionSearchDialog,
            conversation: &str,
            deadline: Instant,
        ) -> Result<Option<SessionSearchSelection>, String> {
            if current.input.string_attr("AXValue")?.as_deref() != Some(conversation) {
                return Err(blocked("session_search_query_changed_before_results"));
            }
            let results = self.observe_search_dialog(current, deadline)?;
            // Re-read the query after observation so an older result snapshot
            // cannot authorize selection after the user edits the search.
            verified_search_result(
                &results,
                current.input.string_attr("AXValue")?.as_deref(),
                conversation,
            )
            .map(|row| {
                row.map(|i| SessionSearchSelection {
                    window: self.window.as_ref().unwrap().element.clone(),
                    dialog: SessionSearchDialog {
                        root: current.root.clone(),
                        input: current.input.clone(),
                    },
                    row: results.nodes[i].element.clone(),
                    fields: results
                        .nodes
                        .iter()
                        .filter(|node| node.parent == Some(i))
                        .map(|node| node.element.clone())
                        .collect(),
                })
            })
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

    fn search_controls(observation: &WindowObservation) -> Result<Option<(usize, usize)>, String> {
        let mut fields = observation.nodes.iter().enumerate().filter(|(_, node)| {
            node.data.role == "AXComboBox"
                && node.data.label == "Search for conversations..."
                && node.data.enabled
                && node.data.editable
        });
        let Some((input, node)) = fields.next() else {
            return Ok(None);
        };
        if fields.next().is_some() {
            return Err(blocked("session_search_input_ambiguous"));
        }
        let mut parent = node.parent;
        while let Some(index) = parent {
            let node = &observation.nodes[index];
            if node.data.role == "AXGroup" && node.data.subrole == "AXApplicationDialog" {
                return Ok(Some((index, input)));
            }
            parent = node.parent;
        }
        Err(blocked("session_search_dialog_missing"))
    }

    fn search_result_row(
        observation: &WindowObservation,
        conversation: &str,
    ) -> Result<Option<usize>, String> {
        let mut found = None;
        for (index, node) in observation.nodes.iter().enumerate() {
            if node.data.role != "AXStaticText" || !node.data.enabled || !node.data.pressable {
                continue;
            }
            let mut parent = node.parent;
            let mut in_list = false;
            while let Some(i) = parent {
                if observation.nodes[i].data.role == "AXList" {
                    in_list = true;
                    break;
                }
                parent = observation.nodes[i].parent;
            }
            if !in_list {
                continue;
            }
            let children: Vec<_> = observation
                .nodes
                .iter()
                .filter(|n| n.parent == Some(index))
                .collect();
            // Observed Antigravity search rows carry [title, icon, project,
            // uuid, age] or [title, icon, project, uuid, unread_badge, age].
            // Never mistake a title equal to a UUID for the dedicated identity
            // field; unknown row shapes fail closed.
            let valid_row = match children.as_slice() {
                [title, icon, project, uuid, age] => {
                    title.data.role == "AXStaticText"
                        && icon.data.role == "AXImage"
                        && project.data.role == "AXStaticText"
                        && uuid.data.role == "AXStaticText"
                        && age.data.role == "AXStaticText"
                        && uuid.data.label == conversation
                }
                [title, icon, project, uuid, badge, age] => {
                    title.data.role == "AXStaticText"
                        && icon.data.role == "AXImage"
                        && project.data.role == "AXStaticText"
                        && uuid.data.role == "AXStaticText"
                        && badge.data.label.is_empty()
                        && ((badge.data.role == "AXGroup" && badge.data.subrole == "AXEmptyGroup")
                            || badge.data.role == "AXImage")
                        && age.data.role == "AXStaticText"
                        && uuid.data.label == conversation
                }
                _ => false,
            };
            if !valid_row {
                continue;
            }
            if found.replace(index).is_some() {
                return Err(blocked("session_search_result_ambiguous"));
            }
        }
        Ok(found)
    }

    fn verified_search_result(
        observation: &WindowObservation,
        query: Option<&str>,
        conversation: &str,
    ) -> Result<Option<usize>, String> {
        if query != Some(conversation) {
            return Err(blocked("session_search_query_changed_after_results"));
        }
        if !matches!(search_controls(observation)?, Some((0, _))) {
            return Err(blocked("session_search_dialog_changed"));
        }
        search_result_row(observation, conversation)
    }
    fn press_search_selection(
        validate_lease: &mut dyn FnMut() -> Result<(), String>,
        verify: impl FnOnce() -> Result<(), String>,
        is_current: &dyn Fn() -> bool,
        press: impl FnOnce() -> Result<(), String>,
    ) -> Result<bool, String> {
        validate_lease()?;
        match verify() {
            Err(error) if invalid_ax_observation(&error) => return Ok(false),
            result => result.map_err(|error| format!("session_search_selection_check:{error}"))?,
        }
        if !is_current() {
            return Err(blocked("session_navigation_retired"));
        }
        press().map_err(|error| format!("session_search_press:{error}"))?;
        Ok(true)
    }

    fn select_search_result_until<T>(
        deadline: Instant,
        is_current: &dyn Fn() -> bool,
        mut find_result: impl FnMut() -> Result<Option<T>, String>,
        mut try_press: impl FnMut(T) -> Result<bool, String>,
    ) -> Result<(), String> {
        let mut saw_invalid = false;
        let timeout_error = |saw_invalid: bool, is_current: &dyn Fn() -> bool| {
            if saw_invalid && is_current() {
                "unconfirmed:session_search_result_unstable".to_string()
            } else {
                "unconfirmed:session_search_result_unavailable".to_string()
            }
        };
        loop {
            if !is_current() || Instant::now() >= deadline {
                return Err(timeout_error(saw_invalid, is_current));
            }
            let selection = match find_result() {
                Ok(selection) => selection,
                Err(error)
                    if error == "blocked:ax_snapshot_timeout" && Instant::now() >= deadline =>
                {
                    return Err(timeout_error(saw_invalid, is_current));
                }
                Err(error) => return Err(unconfirmed(error)),
            };
            if let Some(selection) = selection {
                // Reacquire only references invalidated before AXPress.
                // Query writes and AXPress itself are never replayed.
                if try_press(selection).map_err(unconfirmed)? {
                    return Ok(());
                }
                saw_invalid = true;
            }
            if !is_current() || Instant::now() >= deadline {
                return Err(timeout_error(saw_invalid, is_current));
            }
            std::thread::sleep(
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(25)),
            );
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
            validate_lease()?;
            let observation = self.observe(target.pid)?;
            if search_controls(&observation)?.is_none() {
                let buttons: Vec<_> = observation
                    .nodes
                    .iter()
                    .filter(|node| {
                        node.data.role == "AXButton"
                            && node.data.label == "Search Conversations"
                            && node.data.enabled
                            && node.data.pressable
                    })
                    .collect();
                let [button] = buttons.as_slice() else {
                    return Err(blocked("session_search_button_unavailable_or_ambiguous"));
                };
                self.before_mutation(is_current)?;
                button.element.action("AXPress")?;
            }
            let deadline = Instant::now() + Duration::from_millis(1500);
            let dialog = loop {
                if let Some(dialog) = self
                    .search_dialog(target.pid, deadline)
                    .map_err(unconfirmed)?
                {
                    break dialog.root;
                }
                if !is_current() || Instant::now() >= deadline {
                    return Err("unconfirmed:session_search_unavailable".into());
                }
                std::thread::sleep(Duration::from_millis(25));
            };
            validate_lease().map_err(unconfirmed)?;
            let current = self
                .search_dialog(target.pid, Instant::now() + Duration::from_secs(2))?
                .ok_or_else(|| blocked("session_search_closed"))?;
            if unsafe { CFEqual(dialog.0, current.root.0) } == 0 {
                return Err(blocked("session_search_changed"));
            }
            self.before_mutation(is_current)?;
            write_with_readback(
                || current.input.set("AXValue", Owned::text(conversation).0),
                || {
                    self.before_mutation(is_current)?;
                    let window = self
                        .window
                        .as_ref()
                        .ok_or_else(|| blocked("session_window_missing"))?;
                    let check_deadline = Instant::now() + Duration::from_millis(400);
                    for element in [&current.root, &current.input] {
                        let attached = element
                            .attributes(&["AXWindow"], check_deadline)?
                            .pop()
                            .flatten()
                            .ok_or_else(|| blocked("session_search_closed"))?;
                        if unsafe { CFEqual(attached.0, window.element.0) } == 0 {
                            return Err(blocked("session_search_changed_during_query_write"));
                        }
                    }
                    if current.input.string_attr("AXValue")?.as_deref() != Some(conversation) {
                        if !is_current() {
                            return Err(blocked("session_navigation_retired"));
                        }
                        return Ok(false);
                    }
                    self.observe_search_dialog(&current, check_deadline)
                        .map_err(|error| {
                            if error == "blocked:session_search_changed" {
                                blocked("session_search_changed_during_query_write")
                            } else {
                                error
                            }
                        })?;
                    if !is_current() {
                        return Err(blocked("session_navigation_retired"));
                    }
                    Ok(true)
                },
                "session_search_query_not_applied",
            )?;
            let deadline = Instant::now() + Duration::from_secs(2);
            select_search_result_until(
                deadline,
                is_current,
                || self.search_result(&current, conversation, deadline),
                |selection| {
                    press_search_selection(
                        validate_lease,
                        || {
                            self.before_mutation(is_current)?;
                            selection.verify(conversation, deadline)?;
                            if Instant::now() >= deadline {
                                return Err(blocked("session_search_timeout"));
                            }
                            Ok(())
                        },
                        is_current,
                        || selection.row.action("AXPress"),
                    )
                },
            )
        }
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
        fn validate(&self, identity: &ManagedWindowIdentity) -> Result<(Owned, Owned), String> {
            let target = self
                .target_for(identity)
                .ok_or_else(|| blocked("ax_target_missing"))?;
            if target.identity != *identity || target.window.pid != identity.pid {
                return Err(blocked("target_pid_mismatch"));
            }
            target.window.validate(Some(&identity.title))?;
            if !self.is_descendant(&target.scope, &target.window.element) {
                return Err(blocked("target_scope_changed"));
            }
            Ok((
                target.window.application.clone(),
                target.window.element.clone(),
            ))
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
            let relevant_button = data.pressable
                && ([
                    crate::ax_dictation::DictationAdapter::Antigravity,
                    crate::ax_dictation::DictationAdapter::Dsh,
                ]
                .iter()
                .any(|adapter| {
                    adapter.matches_start_label(&data.label)
                        || adapter.matches_stop_label(&data.label)
                }) || crate::executor::is_antigravity_live_voice_start_label(&data.label)
                    || crate::executor::is_antigravity_live_voice_end_label(&data.label)
                    || matches!(
                        data.label.trim(),
                        "Jump to bottom"
                            | "Scroll to bottom"
                            | "跳到底部"
                            | "滚动到底部"
                            | "Send"
                            | "Send message"
                            | "Submit"
                            | "发送"
                            | "发送消息"
                            | "提交"
                    ));
            let chat_heading = data.role == "AXHeading"
                && data.label == "Conversation Log"
                && self
                    .target
                    .as_ref()
                    .is_some_and(|target| target.identity.bundle_id == "com.google.antigravity");
            if !data.editable && !relevant_button && !chat_heading && data.role != "AXScrollArea" {
                return None;
            }
            Some(ManagedControl {
                id,
                role: data.role,
                label: data.label,
                enabled: data.enabled,
                pressable: relevant_button,
                editable: data.editable,
                focused: data.focused,
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
            let button = matches!(
                role.as_str(),
                "AXButton"
                    | "AXCheckBox"
                    | "AXRadioButton"
                    | "AXSwitch"
                    | "AXToggleButton"
                    | "AXLink"
            );
            let mut data = NodeData {
                role,
                enabled: true,
                ..Default::default()
            };
            if input {
                let mut attrs = element
                    .attributes(&["AXEnabled", "AXFocused"], deadline)?
                    .into_iter();
                data.enabled = bool_attribute(attrs.next().flatten())?.unwrap_or(true);
                data.focused = bool_attribute(attrs.next().flatten())?.unwrap_or(false);
                data.editable = data.role != "AXSecureTextField"
                    && subrole.as_deref() != Some("AXSecureTextField")
                    && element.settable("AXValue")?;
            } else if button || data.role == "AXHeading" {
                let mut attrs = element
                    .attributes(&["AXDescription", "AXTitle", "AXEnabled"], deadline)?
                    .into_iter();
                data.label = text_attribute(attrs.next().flatten())?
                    .filter(|s| !s.trim().is_empty())
                    .or(text_attribute(attrs.next().flatten())?)
                    .unwrap_or_default();
                data.enabled = bool_attribute(attrs.next().flatten())?.unwrap_or(true);
                data.pressable =
                    button && element.actions()?.iter().any(|action| action == "AXPress");
            }
            Ok(Some(data))
        }
        fn controls_from_observation(
            &mut self,
            observation: WindowObservation,
        ) -> Result<Vec<ManagedControl>, String> {
            let mut controls = Vec::new();
            let mut paths: Vec<Vec<u64>> = Vec::new();
            self.control_paths.clear();
            for node in observation.nodes {
                let id = self.register(&node.element)?;
                let mut path = node
                    .parent
                    .map(|index| paths[index].clone())
                    .unwrap_or_default();
                path.push(id);
                paths.push(path.clone());
                let Some(control) = self.control_from_node_data(id, node.data) else {
                    continue;
                };
                self.control_paths.push((id, path));
                controls.push(control);
            }
            Ok(controls)
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

        fn capture_window(&mut self, bundle_id: &str) -> Result<ManagedWindow, String> {
            let captured = CapturedWindow::capture(bundle_id, false)?;
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
            let input_paths: Vec<_> = snapshot
                .controls
                .iter()
                .filter(|control| {
                    control.enabled && control.editable && control.role == "AXTextArea"
                })
                .filter_map(|control| {
                    self.control_paths
                        .iter()
                        .find(|(id, _)| *id == control.id)
                        .map(|(_, path)| path.clone())
                })
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
            let Some(scope_element_id) =
                crate::managed_controls::select_input_scope(&input_paths, &focus_path)?
            else {
                return Ok(snapshot);
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
                    control.role == "AXHeading" && control.label == "Conversation Log"
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

        fn process_has_exited(&self, pid: i32) -> bool {
            pid > 0
                && unsafe { libc::kill(pid, 0) } == -1
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        }
        fn capture(&mut self, bundle_id: &str) -> Result<ManagedWindow, String> {
            self.capture_window(bundle_id)
        }
        fn capture_for_scroll(&mut self, bundle_id: &str) -> Result<ManagedWindow, String> {
            self.capture_window(bundle_id)
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
            let observation = observe_window(scope, ObservationPurpose::Controls)?;
            let controls = self.controls_from_observation(observation)?;
            self.validate(target)?;
            Ok(ManagedWindow {
                identity: target.clone(),
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
            let [input] = inputs.as_slice() else {
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
            let value = Owned::text(&replacement);
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
                let buttons: Vec<_> = window
                    .controls
                    .iter()
                    .filter(|control| {
                        control.enabled
                            && control.pressable
                            && matches!(
                                control.label.trim(),
                                "Send" | "Send message" | "Submit" | "发送" | "发送消息" | "提交"
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
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn search_fixture() -> WindowObservation {
            let fields = [
                ("AXGroup", "", None),
                ("AXComboBox", "Search for conversations...", Some(0)),
                ("AXList", "", Some(0)),
                ("AXGroup", "", Some(2)),
                ("AXStaticText", "result", Some(3)),
                ("AXStaticText", "title", Some(4)),
                ("AXImage", "", Some(4)),
                ("AXStaticText", "project", Some(4)),
                ("AXStaticText", "target-uuid", Some(4)),
                ("AXStaticText", "age", Some(4)),
            ];
            let mut nodes: Vec<_> = fields
                .into_iter()
                .enumerate()
                .map(|(index, (role, label, parent))| ObservedNode {
                    element: Owned::text(&index.to_string()),
                    parent,
                    data: NodeData {
                        role: role.into(),
                        label: label.into(),
                        enabled: true,
                        ..Default::default()
                    },
                })
                .collect();
            nodes[0].data.subrole = "AXApplicationDialog".into();
            nodes[1].data.editable = true;
            nodes[4].data.pressable = true;
            WindowObservation { nodes }
        }

        #[test]
        fn search_identity_uses_the_dedicated_uuid_field_and_rejects_unknown_rows() {
            let mut tree = search_fixture();
            assert_eq!(
                verified_search_result(&tree, Some("target-uuid"), "target-uuid").unwrap(),
                Some(4)
            );
            tree.nodes[8].data.label = "other-uuid".into();
            tree.nodes[5].data.label = "target-uuid".into();
            tree.nodes[7].data.label = "target-uuid".into();
            assert_eq!(
                verified_search_result(&tree, Some("target-uuid"), "target-uuid").unwrap(),
                None
            );
            tree.nodes[8].data.label = "target-uuid".into();
            tree.nodes.pop(); // Missing age changes the known field shape.
            assert_eq!(
                verified_search_result(&tree, Some("target-uuid"), "target-uuid").unwrap(),
                None
            );
            let empty = WindowObservation {
                nodes: search_fixture().nodes.into_iter().take(4).collect(),
            };
            assert_eq!(
                verified_search_result(&empty, Some("target-uuid"), "target-uuid").unwrap(),
                None
            );
        }

        #[test]
        fn search_selection_rejects_duplicate_results_query_changes_and_foreign_dialogs() {
            let mut tree = search_fixture();
            for query in [None, Some("old-query")] {
                assert!(verified_search_result(&tree, query, "target-uuid").is_err());
            }
            tree.nodes[0].data.subrole.clear();
            assert!(verified_search_result(&tree, Some("target-uuid"), "target-uuid").is_err());
            tree = search_fixture();
            tree.nodes[1].data.label = "Other search".into();
            assert!(verified_search_result(&tree, Some("target-uuid"), "target-uuid").is_err());
            tree = search_fixture();
            let offset = tree.nodes.len() - 4;
            tree.nodes
                .extend(search_fixture().nodes.into_iter().enumerate().skip(4).map(
                    |(index, mut node)| {
                        node.element = Owned::text(&(index + offset).to_string());
                        if index > 4 {
                            node.parent = node.parent.map(|parent| parent + offset);
                        }
                        node
                    },
                ));
            assert_eq!(
                verified_search_result(&tree, Some("target-uuid"), "target-uuid").unwrap_err(),
                "blocked:session_search_result_ambiguous"
            );
        }

        #[test]
        fn result_press_rechecks_lease_then_window_query_identity_and_cancellation() {
            use std::cell::Cell;
            for changed in ["none", "lease", "window", "query", "identity", "cancel"] {
                let tree = search_fixture();
                let selection = SessionSearchSelection {
                    dialog: SessionSearchDialog {
                        root: tree.nodes[0].element.clone(),
                        input: tree.nodes[1].element.clone(),
                    },
                    window: Owned::text("window"),
                    row: tree.nodes[4].element.clone(),
                    fields: tree.nodes[5..].iter().map(|n| n.element.clone()).collect(),
                };
                let validated = Cell::new(false);
                let current = Cell::new(true);
                let presses = Cell::new(0);
                let result = press_search_selection(
                    &mut || {
                        validated.set(true);
                        if changed == "lease" {
                            Err("instance replaced during search".into())
                        } else {
                            Ok(())
                        }
                    },
                    || {
                        selection.verify_with(
                            "target-uuid",
                            Instant::now() + Duration::from_secs(1),
                            |element, names, _| {
                                assert!(
                                    validated.get(),
                                    "short readback must follow fresh lease validation"
                                );
                                let id = element.string().unwrap();
                                Ok(names
                                    .iter()
                                    .map(|name| match *name {
                                        "AXWindow" => Some(Owned::text(if changed == "window" {
                                            "replacement"
                                        } else {
                                            "window"
                                        })),
                                        "AXRole" => Some(Owned::text("AXStaticText")),
                                        "AXChildren" => {
                                            let values: Vec<_> =
                                                selection.fields.iter().map(|f| f.0).collect();
                                            unsafe {
                                                Owned::from_create(CFArrayCreate(
                                                    std::ptr::null(),
                                                    values.as_ptr(),
                                                    values.len() as isize,
                                                    std::ptr::null(),
                                                ))
                                            }
                                        }
                                        "AXValue" => {
                                            if changed == "cancel" {
                                                current.set(false);
                                            }
                                            Some(Owned::text(
                                                if (changed == "query" && id == "1")
                                                    || (changed == "identity" && id == "8")
                                                {
                                                    "other-uuid"
                                                } else {
                                                    "target-uuid"
                                                },
                                            ))
                                        }
                                        _ => None,
                                    })
                                    .collect())
                            },
                        )
                    },
                    &|| current.get(),
                    || {
                        presses.set(presses.get() + 1);
                        Ok(())
                    },
                );
                assert_eq!(result.is_ok(), changed == "none", "{changed}");
                assert_eq!(presses.get(), usize::from(changed == "none"), "{changed}");
            }
        }

        #[test]
        fn only_invalid_observation_before_press_allows_candidate_reacquisition() {
            let presses = std::cell::Cell::new(0);
            let mut validate = || Ok(());
            let invalid = || Err("blocked:ax_batch_-25202".into());
            assert!(
                !press_search_selection(&mut validate, invalid, &|| true, || {
                    presses.set(presses.get() + 1);
                    Ok(())
                })
                .unwrap()
            );
            assert_eq!(presses.get(), 0);
            assert!(press_search_selection(
                &mut validate,
                || Ok(()),
                &|| true,
                || {
                    presses.set(presses.get() + 1);
                    Err("unconfirmed:ax_action_-25202".into())
                }
            )
            .is_err());
            assert_eq!(presses.get(), 1);
        }

        #[test]
        fn query_write_waits_for_application_once_then_rejects_later_query_changes() {
            let writes = std::cell::Cell::new(0);
            let mut readbacks = [None, Some(""), Some("target-uuid")].into_iter();
            write_with_readback(
                || {
                    writes.set(writes.get() + 1);
                    Ok(())
                },
                || Ok(readbacks.next().expect("bounded query readback") == Some("target-uuid")),
                "session_search_query_not_applied",
            )
            .unwrap();
            assert_eq!(writes.get(), 1);
            assert!(readbacks.next().is_none());

            let tree = search_fixture();
            let presses = std::cell::Cell::new(0);
            let changed = press_search_selection(
                &mut || Ok(()),
                || {
                    verified_search_result(&tree, Some("user-changed-query"), "target-uuid")
                        .map(|_| ())
                },
                &|| true,
                || {
                    presses.set(presses.get() + 1);
                    Ok(())
                },
            );
            assert!(changed
                .unwrap_err()
                .contains("session_search_query_changed_after_results"));
            assert_eq!(presses.get(), 0);
            assert_eq!(writes.get(), 1);
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
                "session_search_query_not_applied",
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
            let value = decode_attribute_slot(Owned::text("AXLink"))
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
                Owned::text("root"),
                Instant::now() + Duration::from_secs(1),
                |element| {
                    visited += 1;
                    match element.string().as_deref() {
                        Some("root") => Ok(Some(NodeData {
                            children: vec![Owned::text("candidate"), Owned::text("unreadable")],
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
                Owned::text("leaf"),
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
                Owned::text("window"),
                Instant::now() + Duration::from_millis(300),
                |element| match element.string().as_deref() {
                    Some("window") => {
                        attempt += 1;
                        let children = if attempt == 1 {
                            vec![Owned::text("old-candidate"), Owned::text("retired-node")]
                        } else {
                            vec![Owned::text("new-candidate")]
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
                Owned::text("retired-window"),
                Instant::now() + Duration::from_millis(20),
                |_| Err("blocked:ax_batch_-25202".into())
            )
            .is_err());
        }

        #[test]
        fn traversal_refuses_cycles_deadlines_and_oversized_frontiers() {
            assert!(observe_tree(
                Owned::text("cycle"),
                Instant::now() + Duration::from_secs(1),
                |element| Ok(Some(NodeData {
                    children: vec![element.clone()],
                    ..Default::default()
                }))
            )
            .is_err());
            assert!(observe_tree(
                Owned::text("expired"),
                Instant::now() - Duration::from_millis(1),
                |_| panic!("expired reads must not start")
            )
            .is_err());
            let leaf = Owned::text("leaf");
            assert!(observe_tree(
                Owned::text("wide"),
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
                    application: Owned::text(&format!("app-{bundle}")),
                    element: Owned::text(&format!("window-{id}")),
                    title: format!("window-{id}"),
                },
                scope: Owned::text(&format!("window-{id}")),
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
            let window = Owned::text("window");
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
                    application: Owned::text("app"),
                    element: window,
                    title: "window".into(),
                },
                scope: Owned::text("left-pane"),
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
            right.scope = Owned::text("right-pane");
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
                let scope = Owned::text(&format!("pane-{index}"));
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
            let owned: Vec<_> = items.iter().map(|item| Owned::text(item)).collect();
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
                Owned::text("window"),
                Instant::now() + Duration::from_secs(1),
                |element| {
                    let id = element.string().unwrap();
                    read_observed_node(
                        ObservationPurpose::SessionDocuments,
                        |names| match (id.as_str(), names) {
                            ("window", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => {
                                container_batches += 1;
                                Ok(vec![
                                    Some(Owned::text("AXWindow")),
                                    None,
                                    None,
                                    ax_array(&["split"]),
                                ])
                            }
                            ("split", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => {
                                container_batches += 1;
                                Ok(vec![
                                    Some(Owned::text("AXSplitGroup")),
                                    None,
                                    None,
                                    ax_array(&["empty-wrapper", "none-wrapper"]),
                                ])
                            }
                            (
                                "empty-wrapper",
                                ["AXRole", "AXSubrole", "AXHidden", "AXChildren"],
                            ) => Ok(vec![
                                Some(Owned::text("AXWebArea")),
                                None,
                                None,
                                ax_array(&["conversation"]),
                            ]),
                            ("empty-wrapper", ["AXURL"]) => Ok(vec![Some(Owned::text(""))]),
                            ("conversation", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => {
                                Ok(vec![
                                    Some(Owned::text("AXWebArea")),
                                    None,
                                    None,
                                    ax_array(&["conversation-dom"]),
                                ])
                            }
                            ("conversation", ["AXURL"]) => {
                                Ok(vec![Some(Owned::text("https://127.0.0.1:4321/c/target"))])
                            }
                            ("none-wrapper", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => {
                                Ok(vec![
                                    Some(Owned::text("AXWebArea")),
                                    None,
                                    None,
                                    ax_array(&["background"]),
                                ])
                            }
                            ("none-wrapper", ["AXURL"]) => Ok(vec![None]),
                            ("background", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => {
                                Ok(vec![
                                    Some(Owned::text("AXWebArea")),
                                    None,
                                    None,
                                    ax_array(&["background-dom"]),
                                ])
                            }
                            ("background", ["AXURL"]) => {
                                Ok(vec![Some(Owned::text("data:text/html,background"))])
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
        fn session_search_skips_leaf_actions_and_matches_five_child_rows() {
            let mut action_queries = 0;
            let observation = observe_tree_once(
                Owned::text("dialog"),
                Instant::now() + Duration::from_secs(1),
                |element| {
                    let id = element.string().unwrap();
                    read_observed_node(
                        ObservationPurpose::SessionSearch,
                        |names| match (id.as_str(), names) {
                            ("dialog", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => {
                                Ok(vec![
                                    Some(Owned::text("AXGroup")),
                                    Some(Owned::text("AXApplicationDialog")),
                                    None,
                                    ax_array(&["input", "list"]),
                                ])
                            }
                            ("input", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => {
                                Ok(vec![
                                    Some(Owned::text("AXComboBox")),
                                    None,
                                    None,
                                    ax_array(&["combo-internal"]),
                                ])
                            }
                            ("input", ["AXPlaceholderValue", "AXEnabled"]) => Ok(vec![
                                Some(Owned::text("Search for conversations...")),
                                unsafe { Some(Owned(CFRetain(kCFBooleanTrue))) },
                            ]),
                            ("list", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => {
                                Ok(vec![
                                    Some(Owned::text("AXList")),
                                    None,
                                    None,
                                    ax_array(&["group"]),
                                ])
                            }
                            ("group", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => {
                                Ok(vec![
                                    Some(Owned::text("AXGroup")),
                                    None,
                                    None,
                                    ax_array(&["row"]),
                                ])
                            }
                            ("row", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => Ok(vec![
                                Some(Owned::text("AXStaticText")),
                                None,
                                None,
                                ax_array(&["title", "icon", "project", "uuid", "age"]),
                            ]),
                            ("row", ["AXValue", "AXEnabled"]) => {
                                Ok(vec![Some(Owned::text("result")), unsafe {
                                    Some(Owned(CFRetain(kCFBooleanTrue)))
                                }])
                            }
                            ("icon", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => {
                                Ok(vec![
                                    Some(Owned::text("AXImage")),
                                    None,
                                    None,
                                    ax_array(&[]),
                                ])
                            }
                            (
                                "title" | "project" | "uuid" | "age",
                                ["AXRole", "AXSubrole", "AXHidden", "AXChildren"],
                            ) => Ok(vec![
                                Some(Owned::text("AXStaticText")),
                                None,
                                None,
                                ax_array(&[]),
                            ]),
                            (leaf, ["AXValue", "AXEnabled"]) => {
                                let text = match leaf {
                                    "title" => "Bidirectional CLI",
                                    "project" => "micro-input",
                                    "uuid" => "target-uuid",
                                    "age" => "2m",
                                    other => panic!("unexpected AXStaticText {other}"),
                                };
                                Ok(vec![Some(Owned::text(text)), unsafe {
                                    Some(Owned(CFRetain(kCFBooleanTrue)))
                                }])
                            }
                            (other, batch) => panic!("unexpected read on {other}: {batch:?}"),
                        },
                        |attr| {
                            assert_eq!(id, "input");
                            assert_eq!(attr, "AXValue");
                            Ok(true)
                        },
                        || {
                            assert_eq!(id, "row", "leaf AXStaticText must not query actions");
                            action_queries += 1;
                            Ok(vec!["AXPress".into()])
                        },
                    )
                },
            )
            .unwrap();
            assert_eq!(action_queries, 1);
            assert!(observation.nodes[4].data.pressable);
            assert!(observation.nodes[5..].iter().all(|n| !n.data.pressable));
            assert_eq!(
                verified_search_result(&observation, Some("target-uuid"), "target-uuid").unwrap(),
                Some(4)
            );
        }

        #[test]
        fn search_result_loop_distinguishes_unavailable_and_unstable_timeouts() {
            let unavailable = select_search_result_until(
                Instant::now() + Duration::from_millis(30),
                &|| true,
                || Ok(None::<()>),
                |()| unreachable!(),
            );
            assert_eq!(
                unavailable.unwrap_err(),
                "unconfirmed:session_search_result_unavailable"
            );

            let mut validate = || Ok(());
            let unstable = select_search_result_until(
                Instant::now() + Duration::from_millis(30),
                &|| true,
                || Ok(Some(())),
                |()| {
                    press_search_selection(
                        &mut validate,
                        || Err("blocked:ax_batch_-25202".into()),
                        &|| true,
                        || unreachable!(),
                    )
                },
            );
            assert_eq!(
                unstable.unwrap_err(),
                "unconfirmed:session_search_result_unstable"
            );

            let current = std::cell::Cell::new(true);
            let cancelled = select_search_result_until(
                Instant::now() + Duration::from_secs(1),
                &|| current.get(),
                || Ok(Some(())),
                |()| {
                    let keep = press_search_selection(
                        &mut validate,
                        || Err("blocked:ax_batch_-25202".into()),
                        &|| current.get(),
                        || unreachable!(),
                    )?;
                    current.set(false);
                    Ok(keep)
                },
            );
            assert_eq!(
                cancelled.unwrap_err(),
                "unconfirmed:session_search_result_unavailable"
            );

            let attempts = std::cell::Cell::new(0);
            let presses = std::cell::Cell::new(0);
            select_search_result_until(
                Instant::now() + Duration::from_secs(1),
                &|| true,
                || Ok(Some(())),
                |()| {
                    let attempt = attempts.get() + 1;
                    attempts.set(attempt);
                    press_search_selection(
                        &mut validate,
                        || {
                            if attempt == 1 {
                                Err("blocked:ax_batch_-25202".into())
                            } else {
                                Ok(())
                            }
                        },
                        &|| true,
                        || {
                            presses.set(presses.get() + 1);
                            Ok(())
                        },
                    )
                },
            )
            .unwrap();
            assert_eq!(attempts.get(), 2);
            assert_eq!(presses.get(), 1);
        }

        fn six_child_search_fixture() -> WindowObservation {
            let fields = [
                ("AXGroup", "AXApplicationDialog", "", None),
                ("AXComboBox", "", "Search for conversations...", Some(0)),
                ("AXList", "", "", Some(0)),
                ("AXGroup", "", "", Some(2)),
                ("AXStaticText", "", "result", Some(3)),
                ("AXStaticText", "", "title", Some(4)),
                ("AXImage", "", "", Some(4)),
                ("AXStaticText", "", "project", Some(4)),
                ("AXStaticText", "", "target-uuid", Some(4)),
                ("AXGroup", "AXEmptyGroup", "", Some(4)),
                ("AXGroup", "AXEmptyGroup", "", Some(9)),
                ("AXStaticText", "", "age", Some(4)),
            ];
            let mut nodes: Vec<_> = fields
                .into_iter()
                .enumerate()
                .map(|(index, (role, subrole, label, parent))| ObservedNode {
                    element: Owned::text(&index.to_string()),
                    parent,
                    data: NodeData {
                        role: role.into(),
                        subrole: subrole.into(),
                        label: label.into(),
                        enabled: true,
                        ..Default::default()
                    },
                })
                .collect();
            nodes[1].data.editable = true;
            nodes[4].data.pressable = true;
            WindowObservation { nodes }
        }

        #[test]
        fn six_child_unread_badge_row_matches_and_verifies_selection_fields() {
            let mut action_queries = 0;
            let observation = observe_tree_once(
                Owned::text("dialog"),
                Instant::now() + Duration::from_secs(1),
                |element| {
                    let id = element.string().unwrap();
                    read_observed_node(
                        ObservationPurpose::SessionSearch,
                        |names| match (id.as_str(), names) {
                            ("dialog", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => {
                                Ok(vec![
                                    Some(Owned::text("AXGroup")),
                                    Some(Owned::text("AXApplicationDialog")),
                                    None,
                                    ax_array(&["input", "list"]),
                                ])
                            }
                            ("input", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => {
                                Ok(vec![
                                    Some(Owned::text("AXComboBox")),
                                    None,
                                    None,
                                    ax_array(&[]),
                                ])
                            }
                            ("input", ["AXPlaceholderValue", "AXEnabled"]) => Ok(vec![
                                Some(Owned::text("Search for conversations...")),
                                unsafe { Some(Owned(CFRetain(kCFBooleanTrue))) },
                            ]),
                            ("list", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => {
                                Ok(vec![
                                    Some(Owned::text("AXList")),
                                    None,
                                    None,
                                    ax_array(&["group", "two-child-text"]),
                                ])
                            }
                            (
                                "two-child-text",
                                ["AXRole", "AXSubrole", "AXHidden", "AXChildren"],
                            ) => Ok(vec![
                                Some(Owned::text("AXStaticText")),
                                None,
                                None,
                                ax_array(&["extra-a", "extra-b"]),
                            ]),
                            ("two-child-text", ["AXValue", "AXEnabled"]) => {
                                Ok(vec![Some(Owned::text("header")), unsafe {
                                    Some(Owned(CFRetain(kCFBooleanTrue)))
                                }])
                            }
                            ("group", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => {
                                Ok(vec![
                                    Some(Owned::text("AXGroup")),
                                    None,
                                    None,
                                    ax_array(&["row"]),
                                ])
                            }
                            ("row", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => Ok(vec![
                                Some(Owned::text("AXStaticText")),
                                None,
                                None,
                                ax_array(&["title", "icon", "project", "uuid", "badge", "age"]),
                            ]),
                            ("row", ["AXValue", "AXEnabled"]) => {
                                Ok(vec![Some(Owned::text("result")), unsafe {
                                    Some(Owned(CFRetain(kCFBooleanTrue)))
                                }])
                            }
                            ("icon", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => {
                                Ok(vec![
                                    Some(Owned::text("AXImage")),
                                    None,
                                    None,
                                    ax_array(&[]),
                                ])
                            }
                            ("badge", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => {
                                Ok(vec![
                                    Some(Owned::text("AXGroup")),
                                    Some(Owned::text("AXEmptyGroup")),
                                    None,
                                    ax_array(&["badge-dot"]),
                                ])
                            }
                            ("badge-dot", ["AXRole", "AXSubrole", "AXHidden", "AXChildren"]) => {
                                Ok(vec![
                                    Some(Owned::text("AXGroup")),
                                    Some(Owned::text("AXEmptyGroup")),
                                    None,
                                    ax_array(&[]),
                                ])
                            }
                            (
                                "title" | "project" | "uuid" | "age" | "extra-a" | "extra-b",
                                ["AXRole", "AXSubrole", "AXHidden", "AXChildren"],
                            ) => Ok(vec![
                                Some(Owned::text("AXStaticText")),
                                None,
                                None,
                                ax_array(&[]),
                            ]),
                            (leaf, ["AXValue", "AXEnabled"]) => {
                                let text = match leaf {
                                    "title" => "Unread Conversation",
                                    "project" => "micro-input",
                                    "uuid" => "target-uuid",
                                    "age" => "1m",
                                    "extra-a" | "extra-b" => "meta",
                                    other => panic!("unexpected AXStaticText {other}"),
                                };
                                Ok(vec![Some(Owned::text(text)), unsafe {
                                    Some(Owned(CFRetain(kCFBooleanTrue)))
                                }])
                            }
                            (other, batch) => panic!("unexpected read on {other}: {batch:?}"),
                        },
                        |attr| {
                            assert_eq!(id, "input");
                            assert_eq!(attr, "AXValue");
                            Ok(true)
                        },
                        || {
                            assert_eq!(id, "row", "only 5- or 6-child rows may query AXActions");
                            action_queries += 1;
                            Ok(vec!["AXPress".into()])
                        },
                    )
                },
            )
            .unwrap();
            assert_eq!(action_queries, 1);
            let row_index =
                verified_search_result(&observation, Some("target-uuid"), "target-uuid")
                    .unwrap()
                    .expect("6-child unread row must match");
            let selection = SessionSearchSelection {
                dialog: SessionSearchDialog {
                    root: observation.nodes[0].element.clone(),
                    input: observation.nodes[1].element.clone(),
                },
                window: Owned::text("window"),
                row: observation.nodes[row_index].element.clone(),
                fields: observation
                    .nodes
                    .iter()
                    .filter(|node| node.parent == Some(row_index))
                    .map(|node| node.element.clone())
                    .collect(),
            };
            assert_eq!(selection.fields.len(), 6);
            assert_eq!(selection.fields[3].string().as_deref(), Some("uuid"));
            let mut value_reads = Vec::new();
            selection
                .verify_with(
                    "target-uuid",
                    Instant::now() + Duration::from_secs(1),
                    |element, names, _| {
                        let id = element.string().unwrap();
                        Ok(names
                            .iter()
                            .map(|name| match *name {
                                "AXWindow" => Some(Owned::text("window")),
                                "AXRole" => Some(Owned::text("AXStaticText")),
                                "AXEnabled" => unsafe { Some(Owned(CFRetain(kCFBooleanTrue))) },
                                "AXChildren" => {
                                    ax_array(&["title", "icon", "project", "uuid", "badge", "age"])
                                }
                                "AXValue" => {
                                    value_reads.push(id.clone());
                                    Some(Owned::text("target-uuid"))
                                }
                                _ => None,
                            })
                            .collect())
                    },
                )
                .unwrap();
            assert_eq!(value_reads, vec!["input".to_string(), "uuid".to_string()]);

            let mut image_badge = six_child_search_fixture();
            image_badge.nodes[9].data.role = "AXImage".into();
            image_badge.nodes[9].data.subrole.clear();
            assert_eq!(
                verified_search_result(&image_badge, Some("target-uuid"), "target-uuid").unwrap(),
                Some(4)
            );
        }

        #[test]
        fn six_child_search_row_rejects_spoofed_badges_trailing_roles_and_title_uuids() {
            let baseline = six_child_search_fixture();
            assert_eq!(
                verified_search_result(&baseline, Some("target-uuid"), "target-uuid").unwrap(),
                Some(4)
            );

            let mut text_badge = six_child_search_fixture();
            text_badge.nodes[9].data.role = "AXStaticText".into();
            text_badge.nodes[9].data.subrole.clear();
            assert_eq!(
                verified_search_result(&text_badge, Some("target-uuid"), "target-uuid").unwrap(),
                None
            );

            let mut labeled_group_badge = six_child_search_fixture();
            labeled_group_badge.nodes[9].data.label = "target-uuid".into();
            assert_eq!(
                verified_search_result(&labeled_group_badge, Some("target-uuid"), "target-uuid")
                    .unwrap(),
                None
            );

            let mut labeled_image_badge = six_child_search_fixture();
            labeled_image_badge.nodes[9].data.role = "AXImage".into();
            labeled_image_badge.nodes[9].data.subrole.clear();
            labeled_image_badge.nodes[9].data.label = "badge".into();
            assert_eq!(
                verified_search_result(&labeled_image_badge, Some("target-uuid"), "target-uuid")
                    .unwrap(),
                None
            );

            let mut non_empty_subrole_badge = six_child_search_fixture();
            non_empty_subrole_badge.nodes[9].data.subrole.clear();
            assert_eq!(
                verified_search_result(
                    &non_empty_subrole_badge,
                    Some("target-uuid"),
                    "target-uuid"
                )
                .unwrap(),
                None
            );

            let mut trailing_group = six_child_search_fixture();
            trailing_group.nodes[11].data.role = "AXGroup".into();
            trailing_group.nodes[11].data.subrole = "AXEmptyGroup".into();
            assert_eq!(
                verified_search_result(&trailing_group, Some("target-uuid"), "target-uuid")
                    .unwrap(),
                None
            );

            let mut spoofed_title = six_child_search_fixture();
            spoofed_title.nodes[5].data.label = "target-uuid".into();
            spoofed_title.nodes[7].data.label = "target-uuid".into();
            spoofed_title.nodes[8].data.label = "other-uuid".into();
            spoofed_title.nodes[11].data.label = "target-uuid".into();
            assert_eq!(
                verified_search_result(&spoofed_title, Some("target-uuid"), "target-uuid").unwrap(),
                None
            );
        }
    }
}

#[cfg(not(target_os = "macos"))]
#[derive(Default)]
pub struct NativeSessionNavigation;
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
