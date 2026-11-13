use mi_model::host_router::BottomRowLayout;
use serde::{Deserialize, Serialize};

pub const KEY_1U_SIZE: f32 = 68.0;
pub const KEY_GAP: f32 = 8.0;
pub const KEY_2U_WIDTH: f32 = KEY_1U_SIZE * 2.0 + KEY_GAP;
pub const CANVAS_PADDING: f32 = 16.0;
pub const CANVAS_WIDTH: f32 = CANVAS_PADDING * 2.0 + KEY_1U_SIZE * 4.0 + KEY_GAP * 3.0;
pub const CANVAS_HEIGHT: f32 = CANVAS_WIDTH;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CanvasRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl CanvasRect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub const fn contains(self, px_x: f32, px_y: f32) -> bool {
        px_x >= self.x
            && px_x <= (self.x + self.width)
            && px_y >= self.y
            && px_y <= (self.y + self.height)
    }

    pub const fn center(self) -> (f32, f32) {
        (self.x + self.width * 0.5, self.y + self.height * 0.5)
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ControlSurfaceId {
    EncoderCcw = 0,
    EncoderCw = 1,
    EncoderPress = 2,
    Joystick = 3,
    Key0 = 4,
    Key1 = 5,
    Key2 = 6,
    Key3 = 7,
    Key4 = 8,
    Key5 = 9,
    Key6 = 10,
    Key7 = 11,
    Key8 = 12,
    Key9 = 13,
    Key10 = 14,
    Key11 = 15,
    Key12 = 16,
}

impl ControlSurfaceId {
    pub const ALL_17: [Self; 17] = [
        Self::EncoderCcw,
        Self::EncoderCw,
        Self::EncoderPress,
        Self::Joystick,
        Self::Key0,
        Self::Key1,
        Self::Key2,
        Self::Key3,
        Self::Key4,
        Self::Key5,
        Self::Key6,
        Self::Key7,
        Self::Key8,
        Self::Key9,
        Self::Key10,
        Self::Key11,
        Self::Key12,
    ];

    pub const fn primary_logical_slot(self) -> u32 {
        match self {
            Self::Key0 => 0,
            Self::Key1 => 1,
            Self::Key2 => 2,
            Self::Key3 => 3,
            Self::Key4 => 4,
            Self::Key5 => 5,
            Self::Key6 => 6,
            Self::Key7 => 7,
            Self::Key8 => 8,
            Self::Key9 => 9,
            Self::Key10 => 10,
            Self::Key11 => 11,
            Self::Key12 => 12,
            Self::EncoderCcw => 13,
            Self::EncoderCw => 14,
            Self::EncoderPress => 15,
            Self::Joystick => 16,
        }
    }

    pub const fn from_logical_slot(slot: u32, layout: BottomRowLayout) -> Option<Self> {
        match slot {
            0 => Some(Self::Key0),
            1 => Some(Self::Key1),
            2 => Some(Self::Key2),
            3 => Some(Self::Key3),
            4 => Some(Self::Key4),
            5 => Some(Self::Key5),
            6 => Some(Self::Key6),
            7 => Some(Self::Key7),
            8 => Some(Self::Key8),
            9 => Some(Self::Key9),
            10 => Some(Self::Key10),
            11 => match layout {
                BottomRowLayout::Split => Some(Self::Key11),
                BottomRowLayout::Merged => Some(Self::Key10),
            },
            12 => Some(Self::Key12),
            13 => Some(Self::EncoderCcw),
            14 => Some(Self::EncoderCw),
            15 => Some(Self::EncoderPress),
            16..=19 => Some(Self::Joystick),
            _ => None,
        }
    }

    pub const fn binding_id(self) -> &'static str {
        match self {
            Self::EncoderCcw => "AG13",
            Self::EncoderCw => "AG14",
            Self::EncoderPress => "ENC_CLK",
            Self::Joystick => "AG15",
            Self::Key0 => "AG00",
            Self::Key1 => "AG01",
            Self::Key2 => "AG02",
            Self::Key3 => "AG03",
            Self::Key4 => "AG04",
            Self::Key5 => "AG05",
            Self::Key6 => "AG06",
            Self::Key7 => "AG07",
            Self::Key8 => "AG08",
            Self::Key9 => "AG09",
            Self::Key10 => "AG10",
            Self::Key11 => "AG11",
            Self::Key12 => "AG12",
        }
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum JoystickDirection8 {
    North = 0,
    NorthEast = 1,
    East = 2,
    SouthEast = 3,
    South = 4,
    SouthWest = 5,
    West = 6,
    NorthWest = 7,
}

impl JoystickDirection8 {
    pub const ALL: [Self; 8] = [
        Self::North,
        Self::NorthEast,
        Self::East,
        Self::SouthEast,
        Self::South,
        Self::SouthWest,
        Self::West,
        Self::NorthWest,
    ];

    pub const fn logical_sector_slot(self) -> u32 {
        match self {
            Self::North | Self::NorthEast => 16,
            Self::South | Self::SouthWest => 17,
            Self::West | Self::NorthWest => 18,
            Self::East | Self::SouthEast => 19,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::North => "↑",
            Self::NorthEast => "↗",
            Self::East => "→",
            Self::SouthEast => "↘",
            Self::South => "↓",
            Self::SouthWest => "↙",
            Self::West => "←",
            Self::NorthWest => "↖",
        }
    }

    pub const fn unit_offset(self) -> (f32, f32) {
        let diag = std::f32::consts::FRAC_1_SQRT_2;
        match self {
            Self::North => (0.0, -1.0),
            Self::NorthEast => (diag, -diag),
            Self::East => (1.0, 0.0),
            Self::SouthEast => (diag, diag),
            Self::South => (0.0, 1.0),
            Self::SouthWest => (-diag, diag),
            Self::West => (-1.0, 0.0),
            Self::NorthWest => (-diag, -diag),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KnobJoystickPreset {
    CycleWorkingAndBlueDot,
    DualScrollProjectsAndChat,
}

impl KnobJoystickPreset {
    pub const ALL: [Self; 2] = [
        Self::CycleWorkingAndBlueDot,
        Self::DualScrollProjectsAndChat,
    ];

    pub const fn title(self) -> &'static str {
        match self {
            Self::CycleWorkingAndBlueDot => "旋钮轮询 Working/蓝点",
            Self::DualScrollProjectsAndChat => "旋钮+摇杆双滚轮",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EncoderDialProjection {
    pub bounds: CanvasRect,
    pub ccw_bounds: CanvasRect,
    pub cw_bounds: CanvasRect,
    pub press_bounds: CanvasRect,
    pub ccw_label: String,
    pub cw_label: String,
    pub press_label: String,
    pub selected_slot: Option<u32>,
    pub ring_glow_hex: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JoystickDirectionBadge {
    pub direction: JoystickDirection8,
    pub logical_slot: u32,
    pub bounds: CanvasRect,
    pub concise_label: String,
    pub icon_token: Option<String>,
    pub is_selected: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JoystickPadProjection {
    pub bounds: CanvasRect,
    pub center_deadzone_radius: f32,
    pub mode_label: String,
    pub active_direction: JoystickDirection8,
    pub sectors: Vec<JoystickDirectionBadge>,
    pub is_selected: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlCellProjection {
    pub surface: ControlSurfaceId,
    pub primary_slot: u32,
    pub effective_slot: u32,
    pub binding_id: String,
    pub row: u8,
    pub col: u8,
    pub span_cols: u8,
    pub bounds: CanvasRect,
    pub silkscreen_icon: Option<String>,
    pub keycap_id: String,
    pub slot_role: String,
    pub concise_caption: String,
    pub full_action_title: String,
    pub is_session_slot: Option<usize>,
    pub is_selected: bool,
    pub is_available: bool,
    pub per_key_led_hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtectedLayerLockProjection {
    pub is_locked: bool,
    pub layer_id: String,
    pub layer_name: String,
    pub owner_client_name: String,
    pub lock_badge_text: String,
    pub explanation_text: String,
    pub unlock_button_title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanvasLayerTab {
    pub layer_id: String,
    pub name: String,
    pub is_selected: bool,
    pub is_protected_oai: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanvasProjection {
    pub profile_id: Option<String>,
    pub profile_display_name: String,
    pub selected_layer_id: Option<String>,
    pub layer_tabs: Vec<CanvasLayerTab>,
    pub selected_surface: ControlSurfaceId,
    pub selected_logical_slot: u32,
    pub selected_joystick_direction: JoystickDirection8,
    pub bottom_row_layout: BottomRowLayout,
    pub show_keycap_icons: bool,
    pub show_keycap_labels: bool,
    pub encoder: EncoderDialProjection,
    pub joystick: JoystickPadProjection,
    pub keycap_cells: Vec<ControlCellProjection>,
    pub status_cluster_bounds: CanvasRect,
    pub status_led_hex: String,
    pub underglow_hex: String,
    pub board_preview_layer: Option<u8>,
    pub touch_led_states: [bool; 3],
    pub board_preview_label: String,
    pub protected_lock: Option<ProtectedLayerLockProjection>,
    pub can_edit_bindings: bool,
    pub can_add_lighting: bool,
    pub can_export_or_copy_layer: bool,
    pub inspector_expanded: bool,
}

impl Default for CanvasProjection {
    fn default() -> Self {
        let zero = CanvasRect::new(0.0, 0.0, 0.0, 0.0);
        Self {
            profile_id: None,
            profile_display_name: String::new(),
            selected_layer_id: None,
            layer_tabs: Vec::new(),
            selected_surface: ControlSurfaceId::Key0,
            selected_logical_slot: 0,
            selected_joystick_direction: JoystickDirection8::North,
            bottom_row_layout: BottomRowLayout::Split,
            show_keycap_icons: true,
            show_keycap_labels: true,
            encoder: EncoderDialProjection {
                bounds: zero,
                ccw_bounds: zero,
                cw_bounds: zero,
                press_bounds: zero,
                ccw_label: String::new(),
                cw_label: String::new(),
                press_label: String::new(),
                selected_slot: None,
                ring_glow_hex: String::new(),
            },
            joystick: JoystickPadProjection {
                bounds: zero,
                center_deadzone_radius: 0.0,
                mode_label: String::new(),
                active_direction: JoystickDirection8::North,
                sectors: Vec::new(),
                is_selected: false,
            },
            keycap_cells: Vec::new(),
            status_cluster_bounds: zero,
            status_led_hex: String::new(),
            underglow_hex: String::new(),
            board_preview_layer: None,
            touch_led_states: [true, false, false],
            board_preview_label: "USB 启动方案预览".to_string(),
            protected_lock: None,
            can_edit_bindings: true,
            can_add_lighting: true,
            can_export_or_copy_layer: true,
            inspector_expanded: true,
        }
    }
}

impl CanvasProjection {
    pub fn visible_keycap_cells(&self) -> &[ControlCellProjection] {
        &self.keycap_cells
    }

    pub fn cell_for_surface(&self, surface: ControlSurfaceId) -> Option<&ControlCellProjection> {
        self.keycap_cells
            .iter()
            .find(|cell| cell.surface == surface)
    }

    pub fn surface_bounds(&self, surface: ControlSurfaceId) -> Option<CanvasRect> {
        match surface {
            ControlSurfaceId::EncoderCcw => Some(self.encoder.ccw_bounds),
            ControlSurfaceId::EncoderCw => Some(self.encoder.cw_bounds),
            ControlSurfaceId::EncoderPress => Some(self.encoder.press_bounds),
            ControlSurfaceId::Joystick => Some(self.joystick.bounds),
            _ => self.cell_for_surface(surface).map(|cell| cell.bounds),
        }
    }

    pub fn hit_test_point(&self, px_x: f32, px_y: f32) -> Option<ControlSurfaceId> {
        if self.encoder.bounds.contains(px_x, px_y) {
            let (cx, cy) = self.encoder.bounds.center();
            let dx = px_x - cx;
            let dy = px_y - cy;
            let dist_sq = dx * dx + dy * dy;
            let press_r = self.encoder.bounds.width * 0.22;
            if self.encoder.press_bounds.contains(px_x, px_y) || dist_sq <= press_r * press_r {
                return Some(ControlSurfaceId::EncoderPress);
            }
            if px_x < cx {
                return Some(ControlSurfaceId::EncoderCcw);
            }
            return Some(ControlSurfaceId::EncoderCw);
        }

        if self.joystick.bounds.contains(px_x, px_y) {
            return Some(ControlSurfaceId::Joystick);
        }

        for cell in &self.keycap_cells {
            if cell.bounds.contains(px_x, px_y) {
                return Some(cell.surface);
            }
        }

        None
    }

    pub fn hit_test_joystick_direction(&self, px_x: f32, px_y: f32) -> Option<JoystickDirection8> {
        if !self.joystick.bounds.contains(px_x, px_y) {
            return None;
        }
        let (cx, cy) = self.joystick.bounds.center();
        let dx = px_x - cx;
        let dy = px_y - cy;
        let dist_sq = dx * dx + dy * dy;
        let deadzone = self.joystick.center_deadzone_radius;
        if dist_sq <= deadzone * deadzone {
            return None;
        }

        let angle_deg = dy.atan2(dx).to_degrees();
        let dir = if (-22.5..22.5).contains(&angle_deg) {
            JoystickDirection8::East
        } else if (22.5..67.5).contains(&angle_deg) {
            JoystickDirection8::SouthEast
        } else if (67.5..112.5).contains(&angle_deg) {
            JoystickDirection8::South
        } else if (112.5..157.5).contains(&angle_deg) {
            JoystickDirection8::SouthWest
        } else if (-67.5..-22.5).contains(&angle_deg) {
            JoystickDirection8::NorthEast
        } else if (-112.5..-67.5).contains(&angle_deg) {
            JoystickDirection8::North
        } else if (-157.5..-112.5).contains(&angle_deg) {
            JoystickDirection8::NorthWest
        } else {
            JoystickDirection8::West
        };
        Some(dir)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CanvasAction {
    SelectSurface(ControlSurfaceId),
    SelectLogicalSlot(u32),
    SelectJoystickDirection(JoystickDirection8),
    ClickCanvasPoint { x_milli_px: i32, y_milli_px: i32 },
    SelectLayer(String),
    SetBottomRowLayout(BottomRowLayout),
    MergeBottomRowAcrossLayers,
    SetShowKeycapIcons(bool),
    SetShowKeycapLabels(bool),
    ApplyKnobJoystickPreset(KnobJoystickPreset),
    AssignSlotAction { slot: u32, action_id: String },
    UnlockBySwitchingOrCreatingCustomLayer,
}

pub fn format_concise_keycap_caption(raw_label: &str) -> String {
    let trimmed = raw_label.trim();
    if trimmed.is_empty() {
        return "未绑定".to_string();
    }

    let without_paren = trimmed.split(['(', '（']).next().unwrap_or(trimmed).trim();

    if without_paren == "内置浏览器 · 布局" {
        return "浏览器".to_string();
    }

    let segment = if let Some((left, right)) = without_paren.split_once('·') {
        let left_trim = left.trim();
        let right_trim = right.trim();
        if left_trim.starts_with("会话") || left_trim.starts_with("槽位") {
            left_trim
        } else if !right_trim.is_empty() {
            right_trim
        } else {
            left_trim
        }
    } else {
        without_paren
    };

    if segment == "在项目中新建会话" {
        return "项目新建".to_string();
    }
    if segment == "聚焦当前输入框" {
        return "聚焦输入".to_string();
    }
    if segment == "新建标签页" {
        return "新标签页".to_string();
    }
    if segment == "Gemini Live 实时语音" || segment == "Live 实时语音" {
        return "实时语音".to_string();
    }

    let compact: String = segment
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '/')
        .take(4)
        .collect();

    if compact.is_empty() {
        trimmed.chars().take(4).collect()
    } else {
        compact
    }
}
