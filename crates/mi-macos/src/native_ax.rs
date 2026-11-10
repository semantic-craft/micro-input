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
pub use macos::NativeAxProvider;

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use crate::managed_controls::ManagedControl;
    use std::ffi::c_void;
    use std::time::{Duration, Instant};

    type Ref = *const c_void;
    const MAX_NODES: usize = 4096;
    const MAX_DEPTH: usize = 48;
    const UTF8: u32 = 0x0800_0100;

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
        fn AXUIElementCopyAttributeValue(element: Ref, attribute: Ref, value: *mut Ref) -> i32;
        fn AXUIElementCopyActionNames(element: Ref, value: *mut Ref) -> i32;
        fn AXUIElementIsAttributeSettable(element: Ref, attribute: Ref, value: *mut u8) -> i32;
        fn AXUIElementSetAttributeValue(element: Ref, attribute: Ref, value: Ref) -> i32;
        fn AXUIElementPerformAction(element: Ref, action: Ref) -> i32;
        fn AXValueGetValue(value: Ref, kind: u32, output: *mut c_void) -> bool;
        fn AXValueGetTypeID() -> usize;
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
        fn CFArrayGetTypeID() -> usize;
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
        fn attr(&self, name: &str) -> Option<Self> {
            let name = Self::text(name);
            let mut value = std::ptr::null();
            let status = unsafe { AXUIElementCopyAttributeValue(self.0, name.0, &mut value) };
            if status != 0 {
                return None;
            }
            unsafe { Self::from_create(value) }
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
        fn string_attr(&self, name: &str) -> Option<String> {
            self.attr(name)?.string()
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
        fn bool_attr(&self, name: &str) -> Option<bool> {
            let value = self.attr(name)?;
            if unsafe { CFGetTypeID(value.0) == CFBooleanGetTypeID() } {
                return Some(unsafe { CFBooleanGetValue(value.0) } != 0);
            }
            value.number().map(|n| n != 0.0)
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
                if !value.is_null() {
                    items.push(unsafe { Self(CFRetain(value)) });
                }
            }
            Ok(items)
        }
        fn children(&self) -> Result<Vec<Self>, String> {
            self.attr("AXChildren")
                .map_or_else(|| Ok(Vec::new()), |v| v.array())
        }
        fn actions(&self) -> Vec<String> {
            let mut value = std::ptr::null();
            if unsafe { AXUIElementCopyActionNames(self.0, &mut value) } != 0 {
                return Vec::new();
            }
            let Some(value) = (unsafe { Self::from_create(value) }) else {
                return Vec::new();
            };
            value
                .array()
                .unwrap_or_default()
                .iter()
                .filter_map(Self::string)
                .collect()
        }
        fn settable(&self, name: &str) -> bool {
            let name = Self::text(name);
            let mut value = 0u8;
            unsafe { AXUIElementIsAttributeSettable(self.0, name.0, &mut value) == 0 && value != 0 }
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
        fn rect(&self) -> Option<Rect> {
            let origin = self.attr("AXPosition")?;
            let size = self.attr("AXSize")?;
            if unsafe {
                CFGetTypeID(origin.0) != AXValueGetTypeID()
                    || CFGetTypeID(size.0) != AXValueGetTypeID()
            } {
                return None;
            }
            let mut point = Point::default();
            let mut dimensions = Size::default();
            if !unsafe {
                AXValueGetValue(origin.0, 1, (&mut point as *mut Point).cast())
                    && AXValueGetValue(size.0, 2, (&mut dimensions as *mut Size).cast())
            } {
                return None;
            }
            (point.x.is_finite()
                && point.y.is_finite()
                && dimensions.width.is_finite()
                && dimensions.height.is_finite()
                && dimensions.width > 0.0
                && dimensions.height > 0.0)
                .then_some(Rect {
                    origin: point,
                    size: dimensions,
                })
        }
    }

    #[derive(Debug, Clone)]
    struct Target {
        identity: ManagedWindowIdentity,
        application: Owned,
        window: Owned,
    }

    #[derive(Debug, Default)]
    pub struct NativeAxProvider {
        target: Option<Target>,
        retained_targets: Vec<Target>,
        elements: Vec<Owned>,
        windows: Vec<(String, Owned, u64)>,
        next_window_id: u64,
        cached_controls: Vec<ManagedControl>,
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
            if !unsafe { AXIsProcessTrusted() } {
                return Err(blocked("accessibility_untrusted"));
            }
            let target = self
                .target_for(identity)
                .ok_or_else(|| blocked("ax_target_missing"))?;
            if target.identity != *identity
                || target.window.pid() != Some(identity.pid)
                || target.application.pid() != Some(identity.pid)
            {
                return Err(blocked("target_pid_mismatch"));
            }
            let windows = target
                .application
                .attr("AXWindows")
                .ok_or_else(|| blocked("target_window_lost"))?
                .array()?;
            if !windows
                .iter()
                .any(|window| unsafe { CFEqual(window.0, target.window.0) } != 0)
                || target.window.string_attr("AXTitle").unwrap_or_default() != identity.title
                || target.window.bool_attr("AXMinimized") == Some(true)
            {
                return Err(blocked("target_window_changed"));
            }
            Ok((target.application.clone(), target.window.clone()))
        }
        fn metadata(&self, element: &Owned, id: u64, role: String) -> Option<ManagedControl> {
            let editable = matches!(role.as_str(), "AXTextArea" | "AXTextField")
                && element.settable("AXValue");
            let button = matches!(
                role.as_str(),
                "AXButton" | "AXCheckBox" | "AXRadioButton" | "AXSwitch" | "AXToggleButton"
            );
            let label = if button || role == "AXHeading" {
                element
                    .string_attr("AXDescription")
                    .filter(|v| !v.trim().is_empty())
                    .or_else(|| element.string_attr("AXTitle"))
                    .unwrap_or_default()
            } else {
                String::new()
            };
            let relevant_button = button
                && ([
                    crate::ax_dictation::DictationAdapter::Cindy,
                    crate::ax_dictation::DictationAdapter::Antigravity,
                    crate::ax_dictation::DictationAdapter::Dsh,
                ]
                .iter()
                .any(|adapter| {
                    adapter.matches_start_label(&label) || adapter.matches_stop_label(&label)
                }) || matches!(
                    label.trim(),
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
            let chat_heading = role == "AXHeading"
                && label == "Conversation Log"
                && self
                    .target
                    .as_ref()
                    .is_some_and(|target| target.identity.bundle_id == "com.google.antigravity");
            if !editable && !relevant_button && !chat_heading && role != "AXScrollArea" {
                return None;
            }
            Some(ManagedControl {
                id,
                role,
                label,
                enabled: element.bool_attr("AXEnabled").unwrap_or(true),
                pressable: relevant_button
                    && element.actions().iter().any(|action| action == "AXPress"),
                editable,
                focused: editable && element.bool_attr("AXFocused").unwrap_or(false),
            })
        }
        fn walk(
            &mut self,
            element: Owned,
            depth: usize,
            visited: &mut usize,
            controls: &mut Vec<ManagedControl>,
            deadline: Instant,
        ) -> Result<(), String> {
            if Instant::now() >= deadline {
                return Err(blocked("ax_snapshot_timeout"));
            }
            *visited += 1;
            if *visited > MAX_NODES || depth > MAX_DEPTH {
                return Err(blocked("ax_tree_limit"));
            }
            let role = element
                .string_attr("AXRole")
                .ok_or_else(|| blocked("ax_role_unavailable"))?;
            if role == "AXStaticText" {
                return Ok(());
            }
            if element.bool_attr("AXHidden") == Some(true) {
                return Ok(());
            }
            if let Some(mut control) = self.metadata(&element, 0, role.clone()) {
                control.id = self.register(&element)?;
                controls.push(control);
            }
            if !matches!(role.as_str(), "AXTextArea" | "AXTextField") {
                for child in element.children()? {
                    self.walk(child, depth + 1, visited, controls, deadline)?;
                }
            }
            Ok(())
        }
        fn refresh_controls(&self, window: &Owned) -> Option<Vec<ManagedControl>> {
            if self.cached_controls.is_empty() {
                return None;
            }
            self.cached_controls
                .iter()
                .map(|control| {
                    let element = self.element(control.id).ok()?;
                    let owner = element.attr("AXWindow")?;
                    if element.pid() != window.pid()
                        || unsafe { CFEqual(owner.0, window.0) } == 0
                        || element.bool_attr("AXHidden") == Some(true)
                    {
                        return None;
                    }
                    self.metadata(&element, control.id, element.string_attr("AXRole")?)
                })
                .collect()
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
    }

    impl NativeAxProvider {
        fn capture_window(
            &mut self,
            bundle_id: &str,
            reuse_controls: bool,
        ) -> Result<ManagedWindow, String> {
            if !unsafe { AXIsProcessTrusted() } {
                return Err(blocked("accessibility_untrusted"));
            }
            let pid = crate::executor::find_pid_for_bundle_id(bundle_id)
                .ok_or_else(|| blocked("target_application_missing"))?;
            let application = unsafe { Owned::from_create(AXUIElementCreateApplication(pid)) }
                .ok_or_else(|| blocked("ax_application_unavailable"))?;
            unsafe {
                AXUIElementSetMessagingTimeout(application.0, 0.2);
            }
            if application.pid() != Some(pid) {
                return Err(blocked("target_pid_mismatch"));
            }
            if application.settable("AXManualAccessibility") {
                application.set("AXManualAccessibility", unsafe { kCFBooleanTrue })?;
            }
            let windows = application
                .attr("AXWindows")
                .ok_or_else(|| blocked("target_window_missing"))?
                .array()?;
            let windows: Vec<_> = windows
                .into_iter()
                .filter(|window| {
                    window.bool_attr("AXMinimized") != Some(true)
                        && window.string_attr("AXRole").as_deref() == Some("AXWindow")
                })
                .collect();
            let [window] = windows.as_slice() else {
                return Err(blocked("ambiguous_or_missing_target_window"));
            };
            if window.pid() != Some(pid) {
                return Err(blocked("target_pid_mismatch"));
            }
            let same_window = self.target.as_ref().is_some_and(|old| {
                old.identity.pid == pid
                    && old.identity.bundle_id == bundle_id
                    && unsafe { CFEqual(old.window.0, window.0) } != 0
            });
            if !reuse_controls || !same_window {
                self.cached_controls.clear();
                self.elements.clear();
            }
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
                title: window.string_attr("AXTitle").unwrap_or_default(),
            };
            self.target = Some(Target {
                identity: identity.clone(),
                application,
                window: window.clone(),
            });
            if reuse_controls {
                self.read_for_scroll(&identity)
            } else {
                self.read(&identity)
            }
        }

        fn read_for_scroll(
            &mut self,
            target: &ManagedWindowIdentity,
        ) -> Result<ManagedWindow, String> {
            let (_, window) = self.validate(target)?;
            if let Some(controls) = self.refresh_controls(&window) {
                self.validate(target)?;
                return Ok(ManagedWindow {
                    identity: target.clone(),
                    controls,
                });
            }
            self.read(target)
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
                let Some(role) = hit.string_attr("AXRole") else {
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
                let Some(parent) = hit.attr("AXParent") else {
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
                .rect()
                .ok_or_else(|| blocked("window_geometry_unavailable"))?;
            let headings: Vec<_> = controls
                .iter()
                .filter(|control| {
                    control.role == "AXHeading" && control.label == "Conversation Log"
                })
                .collect();
            let preferred = if let [heading] = headings.as_slice() {
                self.element(heading.id)?
                    .attr("AXParent")
                    .and_then(|parent| parent.rect())
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
            self.capture_window(bundle_id, false)
        }
        fn capture_for_scroll(&mut self, bundle_id: &str) -> Result<ManagedWindow, String> {
            self.capture_window(bundle_id, true)
        }
        fn read(&mut self, target: &ManagedWindowIdentity) -> Result<ManagedWindow, String> {
            let (_, window) = self.validate(target)?;
            if self
                .target
                .as_ref()
                .is_none_or(|current| current.identity != *target)
            {
                self.target = self.target_for(target).cloned();
                self.elements.clear();
                self.cached_controls.clear();
            }
            let mut controls = Vec::new();
            self.walk(
                window,
                0,
                &mut 0,
                &mut controls,
                Instant::now() + Duration::from_millis(500),
            )?;
            self.cached_controls = controls.clone();
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
            self.cached_controls.clear();
            element.action("AXPress")
        }
        fn focus(&mut self, target: &ManagedWindowIdentity, control: u64) -> Result<(), String> {
            let (metadata, element) = self.checked_control(target, control)?;
            if !metadata.enabled || !metadata.editable || !element.settable("AXFocused") {
                return Err(blocked("ax_input_not_focusable"));
            }
            let (application, window) = self.validate(target)?;
            self.cached_controls.clear();
            application.set("AXFrontmost", unsafe { kCFBooleanTrue })?;
            window.action("AXRaise")?;
            self.validate(target)?;
            element.set("AXFocused", unsafe { kCFBooleanTrue })
        }
        fn scroll(
            &mut self,
            target: &ManagedWindowIdentity,
            request: &DialScrollRequest,
        ) -> Result<(), String> {
            let snapshot = self.read_for_scroll(target)?;
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
                .rect()
                .ok_or_else(|| blocked("input_geometry_unavailable"))?;
            let mut candidates: Vec<Owned> = Vec::new();
            for control in snapshot
                .controls
                .iter()
                .filter(|control| control.role == "AXScrollArea")
            {
                let area = self.element(control.id)?;
                let Some(rect) = area.rect() else {
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
                let Some(bar) = area.attr("AXVerticalScrollBar") else {
                    continue;
                };
                if bar.settable("AXValue")
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
                .attr("AXValue")
                .and_then(|v| v.number())
                .ok_or_else(|| blocked("scroll_position_unavailable"))?;
            let min = bar
                .attr("AXMinValue")
                .and_then(|v| v.number())
                .ok_or_else(|| blocked("scroll_range_unavailable"))?;
            let max = bar
                .attr("AXMaxValue")
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
            bar.set("AXValue", value.0)?;
            let deadline = Instant::now() + Duration::from_millis(400);
            loop {
                self.validate(target)
                    .map_err(|e| format!("unconfirmed:{e}"))?;
                if bar
                    .attr("AXValue")
                    .and_then(|v| v.number())
                    .is_some_and(|value| (value - next).abs() <= 0.001 * (max - min))
                {
                    return Ok(());
                }
                if Instant::now() >= deadline {
                    return Err("unconfirmed:scroll_readback".into());
                }
                std::thread::sleep(Duration::from_millis(25));
            }
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
                .string_attr("AXValue")
                .ok_or_else(|| blocked("input_value_unavailable"))?;
            let range = element
                .attr("AXSelectedTextRange")
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
            self.cached_controls.clear();
            element.set("AXValue", value.0)?;
            let deadline = Instant::now() + Duration::from_millis(400);
            loop {
                self.validate(target)
                    .map_err(|e| format!("unconfirmed:{e}"))?;
                if element.string_attr("AXValue").as_deref() == Some(&replacement) {
                    return Ok(());
                }
                if Instant::now() >= deadline {
                    return Err("unconfirmed:input_value_readback".into());
                }
                std::thread::sleep(Duration::from_millis(25));
            }
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
                .attr("AXFocusedWindow")
                .ok_or_else(|| blocked("target_window_not_focused"))?;
            if app.bool_attr("AXFrontmost") != Some(true)
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
            self.cached_controls.clear();
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

        #[test]
        fn owned_window_handles_survive_other_app_and_replacement_captures() {
            let target = |bundle: &str, id: u64| Target {
                identity: ManagedWindowIdentity {
                    bundle_id: bundle.into(),
                    pid: 123,
                    window_id: id,
                    title: format!("window-{id}"),
                },
                application: Owned::text(&format!("app-{bundle}")),
                window: Owned::text(&format!("window-{id}")),
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
                provider.target_for(&identity).unwrap().window.0,
                original_window.0
            );
            provider.target = Some(target("com.google.antigravity", 3));
            assert_eq!(
                provider.target_for(&identity).unwrap().window.0,
                original_window.0
            );
            provider.release_target(&identity);
            assert!(provider.target_for(&identity).is_none());
            assert!(provider.retained_targets.is_empty());
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
    }
}
