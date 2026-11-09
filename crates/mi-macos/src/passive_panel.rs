use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ScreenPoint {
    pub x: f64,
    pub y: f64,
}

impl ScreenPoint {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

impl Default for ScreenPoint {
    fn default() -> Self {
        Self::new(720.0, 450.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ScreenSize {
    pub width: f64,
    pub height: f64,
}

impl ScreenSize {
    pub const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ScreenRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl ScreenRect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub const fn min_x(self) -> f64 {
        self.x
    }

    pub const fn min_y(self) -> f64 {
        self.y
    }

    pub fn max_x(self) -> f64 {
        self.x + self.width
    }

    pub fn max_y(self) -> f64 {
        self.y + self.height
    }

    pub fn contains(self, point: ScreenPoint) -> bool {
        point.x >= self.min_x()
            && point.x <= self.max_x()
            && point.y >= self.min_y()
            && point.y <= self.max_y()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScreenGeometry {
    pub screens: Vec<ScreenRect>,
    pub main_visible_frame: ScreenRect,
}

impl Default for ScreenGeometry {
    fn default() -> Self {
        Self::single(ScreenRect::new(0.0, 0.0, 1440.0, 900.0))
    }
}

impl ScreenGeometry {
    pub fn single(visible_frame: ScreenRect) -> Self {
        Self {
            screens: vec![visible_frame],
            main_visible_frame: visible_frame,
        }
    }

    pub fn visible_frame_for_cursor(&self, cursor: ScreenPoint) -> ScreenRect {
        self.screens
            .iter()
            .copied()
            .find(|rect| rect.contains(cursor))
            .unwrap_or(self.main_visible_frame)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PassivePanelRole {
    RadialMenu,
    PassiveNotice,
    CheatSheet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassivePanelStyleSpec {
    pub borderless: bool,
    pub nonactivating_panel: bool,
    pub can_become_key: bool,
    pub can_become_main: bool,
    pub floating_level: bool,
    pub hides_on_deactivate: bool,
    pub can_join_all_spaces: bool,
    pub full_screen_auxiliary: bool,
    pub is_opaque: bool,
    pub has_shadow: bool,
    pub ignores_mouse_events: bool,
}

impl PassivePanelStyleSpec {
    pub const fn for_role(role: PassivePanelRole) -> Self {
        Self {
            borderless: true,
            nonactivating_panel: true,
            can_become_key: false,
            can_become_main: false,
            floating_level: true,
            hides_on_deactivate: false,
            can_join_all_spaces: true,
            full_screen_auxiliary: true,
            is_opaque: false,
            has_shadow: true,
            ignores_mouse_events: match role {
                PassivePanelRole::RadialMenu | PassivePanelRole::PassiveNotice => true,
                PassivePanelRole::CheatSheet => false,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PassivePanelFrameSpec {
    pub role: PassivePanelRole,
    pub title: String,
    pub frame: ScreenRect,
    pub style: PassivePanelStyleSpec,
}

impl PassivePanelFrameSpec {
    pub const RADIAL_SIZE: ScreenSize = ScreenSize::new(500.0, 500.0);
    pub const NOTICE_SIZE: ScreenSize = ScreenSize::new(280.0, 76.0);
    pub const CHEAT_SHEET_SIZE: ScreenSize = ScreenSize::new(400.0, 610.0);
    pub const HUD_TOP_RIGHT_INSET: f64 = 20.0;

    pub fn top_right_hud_frame(size: ScreenSize, screen: ScreenRect) -> ScreenRect {
        let width = size.width.min((screen.width - 40.0).max(0.0));
        let height = size.height.min((screen.height - 40.0).max(0.0));
        let x = screen
            .min_x()
            .max(screen.max_x() - width - Self::HUD_TOP_RIGHT_INSET);
        let y = screen
            .min_y()
            .max(screen.max_y() - height - Self::HUD_TOP_RIGHT_INSET);
        ScreenRect::new(x, y, width, height)
    }

    pub fn cursor_centered_radial_frame(
        size: ScreenSize,
        cursor: ScreenPoint,
        screen: ScreenRect,
    ) -> ScreenRect {
        let width = size.width.min(screen.width.max(0.0));
        let height = size.height.min(screen.height.max(0.0));
        let x = (cursor.x - width / 2.0)
            .max(screen.min_x())
            .min((screen.max_x() - width).max(screen.min_x()));
        let y = (cursor.y - height / 2.0)
            .max(screen.min_y())
            .min((screen.max_y() - height).max(screen.min_y()));
        ScreenRect::new(x, y, width, height)
    }

    pub fn for_radial(cursor: ScreenPoint, screen: &ScreenGeometry) -> Self {
        let visible = screen.visible_frame_for_cursor(cursor);
        Self {
            role: PassivePanelRole::RadialMenu,
            title: "Micro Input 径向菜单".to_string(),
            frame: Self::cursor_centered_radial_frame(Self::RADIAL_SIZE, cursor, visible),
            style: PassivePanelStyleSpec::for_role(PassivePanelRole::RadialMenu),
        }
    }

    pub fn for_notice(cursor: ScreenPoint, screen: &ScreenGeometry) -> Self {
        let visible = screen.visible_frame_for_cursor(cursor);
        Self {
            role: PassivePanelRole::PassiveNotice,
            title: "Micro Input 方案切换提示".to_string(),
            frame: Self::top_right_hud_frame(Self::NOTICE_SIZE, visible),
            style: PassivePanelStyleSpec::for_role(PassivePanelRole::PassiveNotice),
        }
    }

    pub fn for_cheat_sheet(cursor: ScreenPoint, screen: &ScreenGeometry) -> Self {
        let visible = screen.visible_frame_for_cursor(cursor);
        Self {
            role: PassivePanelRole::CheatSheet,
            title: "Micro Input 按键速查".to_string(),
            frame: Self::top_right_hud_frame(Self::CHEAT_SHEET_SIZE, visible),
            style: PassivePanelStyleSpec::for_role(PassivePanelRole::CheatSheet),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PassivePanelEvent {
    Presented(PassivePanelFrameSpec),
    Hidden(PassivePanelRole),
}

pub trait PassivePanelHost: Send + Sync {
    fn present_panel(&self, spec: PassivePanelFrameSpec);
    fn hide_panel(&self, role: PassivePanelRole);
    fn visible_panel(&self, role: PassivePanelRole) -> Option<PassivePanelFrameSpec>;
    fn events(&self) -> Vec<PassivePanelEvent>;
}

#[derive(Debug, Default, Clone)]
pub struct RecordingPassivePanelHost {
    state: Arc<Mutex<RecordingPassivePanelState>>,
}

#[derive(Debug, Default, Clone)]
pub struct RecordingPassivePanelState {
    pub active_radial: Option<PassivePanelFrameSpec>,
    pub active_hud: Option<PassivePanelFrameSpec>,
    pub history: Vec<PassivePanelEvent>,
}

impl RecordingPassivePanelHost {
    pub fn new() -> Self {
        Self::default()
    }
}

impl PassivePanelHost for RecordingPassivePanelHost {
    fn present_panel(&self, spec: PassivePanelFrameSpec) {
        if let Ok(mut guard) = self.state.lock() {
            match spec.role {
                PassivePanelRole::RadialMenu => {
                    if guard.active_radial.as_ref() != Some(&spec) {
                        guard.active_radial = Some(spec.clone());
                        guard.history.push(PassivePanelEvent::Presented(spec));
                    }
                }
                PassivePanelRole::PassiveNotice | PassivePanelRole::CheatSheet => {
                    if guard.active_hud.as_ref() != Some(&spec) {
                        guard.active_hud = Some(spec.clone());
                        guard.history.push(PassivePanelEvent::Presented(spec));
                    }
                }
            }
        }
    }

    fn hide_panel(&self, role: PassivePanelRole) {
        if let Ok(mut guard) = self.state.lock() {
            match role {
                PassivePanelRole::RadialMenu => {
                    if guard.active_radial.take().is_some() {
                        guard.history.push(PassivePanelEvent::Hidden(role));
                    }
                }
                PassivePanelRole::PassiveNotice | PassivePanelRole::CheatSheet => {
                    if guard.active_hud.as_ref().is_some_and(|p| p.role == role) {
                        guard.active_hud = None;
                        guard.history.push(PassivePanelEvent::Hidden(role));
                    }
                }
            }
        }
    }

    fn visible_panel(&self, role: PassivePanelRole) -> Option<PassivePanelFrameSpec> {
        let guard = self.state.lock().ok()?;
        match role {
            PassivePanelRole::RadialMenu => guard.active_radial.clone(),
            PassivePanelRole::PassiveNotice | PassivePanelRole::CheatSheet => guard
                .active_hud
                .as_ref()
                .filter(|p| p.role == role)
                .cloned(),
        }
    }

    fn events(&self) -> Vec<PassivePanelEvent> {
        self.state
            .lock()
            .map(|guard| guard.history.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radial_and_hud_frames_clamp_and_inset_correctly() {
        let screen = ScreenGeometry::single(ScreenRect::new(0.0, 0.0, 1440.0, 900.0));
        let radial = PassivePanelFrameSpec::for_radial(ScreenPoint::new(50.0, 60.0), &screen);
        assert_eq!(radial.frame, ScreenRect::new(0.0, 0.0, 500.0, 500.0));
        assert!(radial.style.nonactivating_panel);
        assert!(radial.style.ignores_mouse_events);

        let notice = PassivePanelFrameSpec::for_notice(ScreenPoint::new(720.0, 450.0), &screen);
        assert_eq!(notice.frame, ScreenRect::new(1140.0, 804.0, 280.0, 76.0));
        assert!(notice.style.ignores_mouse_events);

        let sheet = PassivePanelFrameSpec::for_cheat_sheet(ScreenPoint::new(720.0, 450.0), &screen);
        assert_eq!(sheet.frame, ScreenRect::new(1020.0, 270.0, 400.0, 610.0));
        assert!(!sheet.style.ignores_mouse_events);
    }
}
