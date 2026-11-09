use crate::sidebar::CanonicalAgent;
use mi_macos::{PassivePanelFrameSpec, ScreenRect};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

pub const RADIAL_SECTOR_COUNT: usize = 8;
pub const RADIAL_CANVAS_SIZE_PT: f64 = 500.0;
pub const RADIAL_ORBIT_RADIUS_FACTOR: f64 = 0.34;
pub const RADIAL_CARD_WIDTH_FACTOR: f64 = 0.23;
pub const RADIAL_CARD_HEIGHT_PT: f64 = 62.0;
pub const ESCAPE_KEY_CODE: u16 = 53;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SectorIndex(u8);

impl SectorIndex {
    pub const EAST: Self = Self(0);
    pub const SOUTH_EAST: Self = Self(1);
    pub const SOUTH: Self = Self(2);
    pub const SOUTH_WEST: Self = Self(3);
    pub const WEST: Self = Self(4);
    pub const NORTH_WEST: Self = Self(5);
    pub const NORTH: Self = Self(6);
    pub const NORTH_EAST: Self = Self(7);

    pub const ALL: [Self; RADIAL_SECTOR_COUNT] = [
        Self::EAST,
        Self::SOUTH_EAST,
        Self::SOUTH,
        Self::SOUTH_WEST,
        Self::WEST,
        Self::NORTH_WEST,
        Self::NORTH,
        Self::NORTH_EAST,
    ];

    pub const fn new(raw: u8) -> Option<Self> {
        if raw < RADIAL_SECTOR_COUNT as u8 {
            Some(Self(raw))
        } else {
            None
        }
    }

    pub const fn from_usize(index: usize) -> Option<Self> {
        if index < RADIAL_SECTOR_COUNT {
            Some(Self(index as u8))
        } else {
            None
        }
    }

    pub const fn as_u8(self) -> u8 {
        self.0
    }

    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }

    pub fn from_polar(angle_turns: f64, distance: f64) -> Option<Self> {
        if !angle_turns.is_finite()
            || !distance.is_finite()
            || !(0.0..=1.0).contains(&angle_turns)
            || !(0.0..=1.0).contains(&distance)
            || distance < RadialMenuState::OPEN_DISTANCE
        {
            return None;
        }
        let idx = ((angle_turns * 8.0 + 0.5).floor() as usize) % RADIAL_SECTOR_COUNT;
        Some(Self(idx as u8))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RadialConfirmMode {
    #[default]
    #[serde(rename = "encoder_press")]
    EncoderPress,
    #[serde(rename = "spring_back")]
    SpringBack,
}

impl RadialConfirmMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::EncoderPress => "旋钮按下确认",
            Self::SpringBack => "摇杆回弹确认",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RadialInputReadiness {
    #[default]
    Ready,
    Disconnected,
    Stale,
    UnverifiedMapping,
    Paused,
}

impl RadialInputReadiness {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ready => "摇杆选择 · 旋钮按下或快速回弹确认 · 慢回中取消",
            Self::Disconnected => "尚未连接已核验的键盘",
            Self::Stale => "请读取当前设备身份与配置",
            Self::UnverifiedMapping => "当前摇杆或旋钮映射不可用于菜单",
            Self::Paused => "设备操作期间或映射重验前暂停菜单",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RadialPresetKind {
    #[default]
    DefaultAppLauncher,
    PaseoEngineering,
    FourAppEngineering,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RadialSlotKind {
    CheatSheet,
    OpenApp,
    Prompt,
    Submit,
    Stop,
    NavigateBack,
    NavigateForward,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RadialSlotConfig {
    pub kind: RadialSlotKind,
    pub label: String,
    pub text: Option<String>,
    pub bundle_id: Option<String>,
    pub adapter: Option<String>,
    pub submit: bool,
    pub symbol: Option<String>,
    pub color_hex: Option<String>,
}

impl RadialSlotConfig {
    pub fn prompt(
        label: impl Into<String>,
        text: impl Into<String>,
        adapter: Option<String>,
        submit: bool,
        symbol: Option<&str>,
        color_hex: Option<&str>,
    ) -> Self {
        Self {
            kind: RadialSlotKind::Prompt,
            label: label.into(),
            text: Some(text.into()),
            bundle_id: None,
            adapter,
            submit,
            symbol: symbol.map(str::to_string),
            color_hex: color_hex.map(str::to_string),
        }
    }

    pub fn submit(
        label: impl Into<String>,
        adapter: Option<String>,
        symbol: Option<&str>,
        color_hex: Option<&str>,
    ) -> Self {
        Self {
            kind: RadialSlotKind::Submit,
            label: label.into(),
            text: None,
            bundle_id: None,
            adapter,
            submit: true,
            symbol: symbol.map(str::to_string),
            color_hex: color_hex.map(str::to_string),
        }
    }

    pub fn stop(
        label: impl Into<String>,
        adapter: Option<String>,
        symbol: Option<&str>,
        color_hex: Option<&str>,
    ) -> Self {
        Self {
            kind: RadialSlotKind::Stop,
            label: label.into(),
            text: None,
            bundle_id: None,
            adapter,
            submit: false,
            symbol: symbol.map(str::to_string),
            color_hex: color_hex.map(str::to_string),
        }
    }

    pub fn navigate_back(
        label: impl Into<String>,
        symbol: Option<&str>,
        color_hex: Option<&str>,
    ) -> Self {
        Self {
            kind: RadialSlotKind::NavigateBack,
            label: label.into(),
            text: None,
            bundle_id: None,
            adapter: None,
            submit: false,
            symbol: symbol.map(str::to_string),
            color_hex: color_hex.map(str::to_string),
        }
    }

    pub fn navigate_forward(
        label: impl Into<String>,
        symbol: Option<&str>,
        color_hex: Option<&str>,
    ) -> Self {
        Self {
            kind: RadialSlotKind::NavigateForward,
            label: label.into(),
            text: None,
            bundle_id: None,
            adapter: None,
            submit: false,
            symbol: symbol.map(str::to_string),
            color_hex: color_hex.map(str::to_string),
        }
    }

    pub fn cheat_sheet(
        label: impl Into<String>,
        symbol: Option<&str>,
        color_hex: Option<&str>,
    ) -> Self {
        Self {
            kind: RadialSlotKind::CheatSheet,
            label: label.into(),
            text: None,
            bundle_id: None,
            adapter: None,
            submit: false,
            symbol: symbol.map(str::to_string),
            color_hex: color_hex.map(str::to_string),
        }
    }

    pub fn open_app(
        label: impl Into<String>,
        bundle_id: impl Into<String>,
        symbol: Option<&str>,
        color_hex: Option<&str>,
    ) -> Self {
        Self {
            kind: RadialSlotKind::OpenApp,
            label: label.into(),
            text: None,
            bundle_id: Some(bundle_id.into()),
            adapter: None,
            submit: false,
            symbol: symbol.map(str::to_string),
            color_hex: color_hex.map(str::to_string),
        }
    }

    pub fn default_app_launcher_slots() -> [Self; RADIAL_SECTOR_COUNT] {
        let agents = [
            CanonicalAgent::Cindy,
            CanonicalAgent::Codex,
            CanonicalAgent::Herdr,
            CanonicalAgent::Antigravity,
            CanonicalAgent::DeepseekHarness,
        ];
        [
            Self::navigate_back("后退", Some("chevron.backward"), Some("#00CCAA")),
            Self::cheat_sheet("按键速查", Some("rectangle.grid.3x2"), Some("#FFFFFF")),
            Self::open_app(
                agents[0].display_name(),
                agents[0].bundle_id(),
                Some("app.badge"),
                None,
            ),
            Self::open_app(
                agents[1].display_name(),
                agents[1].bundle_id(),
                Some("app.badge"),
                None,
            ),
            Self::navigate_forward("前进", Some("chevron.forward"), Some("#00CCAA")),
            Self::open_app(
                agents[2].display_name(),
                agents[2].bundle_id(),
                Some("app.badge"),
                None,
            ),
            Self::open_app(
                agents[3].display_name(),
                agents[3].bundle_id(),
                Some("app.badge"),
                None,
            ),
            Self::open_app(
                agents[4].display_name(),
                agents[4].bundle_id(),
                Some("app.badge"),
                None,
            ),
        ]
    }

    pub fn paseo_engineering_slots() -> [Self; RADIAL_SECTOR_COUNT] {
        [
            Self::navigate_back("Back · 后退", Some("chevron.backward"), Some("#00CCAA")),
            Self::prompt(
                "Test",
                "Run the tests and fix any failures",
                None,
                false,
                Some("checkmark.shield"),
                Some("#00FF66"),
            ),
            Self::prompt(
                "Continue",
                "Continue",
                None,
                false,
                Some("play.fill"),
                Some("#00AAFF"),
            ),
            Self::prompt(
                "Commit",
                "Commit this work",
                None,
                false,
                Some("arrow.triangle.branch"),
                Some("#FFB800"),
            ),
            Self::navigate_forward("Forward · 前进", Some("chevron.forward"), Some("#00CCAA")),
            Self::prompt(
                "Review",
                "Review your diff for bugs",
                None,
                false,
                Some("magnifyingglass"),
                Some("#AA44FF"),
            ),
            Self::submit(
                "Submit · 提交",
                None,
                Some("paperplane.fill"),
                Some("#0066FF"),
            ),
            Self::prompt(
                "Summary",
                "Summarize where things stand and what's left",
                None,
                false,
                Some("list.bullet.rectangle"),
                Some("#00CCAA"),
            ),
        ]
    }

    pub fn four_app_engineering_slots() -> [Self; RADIAL_SECTOR_COUNT] {
        [
            Self::navigate_back("Back · 后退", Some("chevron.backward"), Some("#00CCAA")),
            Self::prompt(
                "Test",
                "Run the tests and fix any failures",
                None,
                false,
                Some("checkmark.shield"),
                Some("#00FF66"),
            ),
            Self::prompt(
                "Continue",
                "Continue",
                None,
                false,
                Some("play.fill"),
                Some("#00AAFF"),
            ),
            Self::prompt(
                "Commit",
                "Commit this work",
                None,
                false,
                Some("arrow.triangle.branch"),
                Some("#FFB800"),
            ),
            Self::navigate_forward("Forward · 前进", Some("chevron.forward"), Some("#00CCAA")),
            Self::prompt(
                "Review",
                "Review your diff for bugs",
                None,
                false,
                Some("magnifyingglass"),
                Some("#AA44FF"),
            ),
            Self::stop(
                "Stop · 停止",
                None,
                Some("stop.circle.fill"),
                Some("#FF3B30"),
            ),
            Self::prompt(
                "Summary",
                "Summarize where things stand and what's left",
                None,
                false,
                Some("list.bullet.rectangle"),
                Some("#00CCAA"),
            ),
        ]
    }

    pub fn for_preset(kind: RadialPresetKind) -> [Self; RADIAL_SECTOR_COUNT] {
        match kind {
            RadialPresetKind::DefaultAppLauncher => Self::default_app_launcher_slots(),
            RadialPresetKind::PaseoEngineering => Self::paseo_engineering_slots(),
            RadialPresetKind::FourAppEngineering => Self::four_app_engineering_slots(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RadialIntent {
    OpenApp {
        bundle_id: String,
    },
    CheatSheet,
    Prompt {
        text: String,
        adapter: Option<String>,
        submit: bool,
    },
    Submit {
        adapter: Option<String>,
    },
    Stop {
        adapter: Option<String>,
    },
    NavigateBack,
    NavigateForward,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RadialSlotResolutionContext {
    pub has_active_profile: bool,
    pub active_adapter: Option<String>,
    pub active_agent_display_name: Option<String>,
    pub active_session_preview: Option<String>,
    pub can_navigate_back: bool,
    pub can_navigate_forward: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RadialOctantPoint {
    pub sector: SectorIndex,
    pub center_x: f64,
    pub center_y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RadialSlot {
    pub sector: SectorIndex,
    pub label: String,
    pub target_preview: Option<String>,
    pub intent: Option<RadialIntent>,
    pub unavailable_reason: Option<String>,
    pub symbol: Option<String>,
    pub color_hex: Option<String>,
    pub octant: RadialOctantPoint,
}

impl RadialSlot {
    pub fn accessibility_label(&self, selected: bool) -> String {
        let prefix = if selected { "已选择，" } else { "" };
        match &self.unavailable_reason {
            Some(reason) => format!("{prefix}{}，不可用，{reason}", self.label),
            None => format!("{prefix}{}", self.label),
        }
    }
}

pub fn octant_center_for_sector(sector: SectorIndex, canvas_size: f64) -> RadialOctantPoint {
    let center = canvas_size * 0.5;
    let radius = canvas_size * RADIAL_ORBIT_RADIUS_FACTOR;
    let angle = sector.as_usize() as f64 * std::f64::consts::FRAC_PI_4;
    RadialOctantPoint {
        sector,
        center_x: center + angle.cos() * radius,
        center_y: center + angle.sin() * radius,
    }
}

pub fn strip_trailing_newlines(raw: &str) -> String {
    raw.trim_end_matches(['\r', '\n']).to_string()
}

pub fn resolve_radial_slots(
    configs: &[RadialSlotConfig; RADIAL_SECTOR_COUNT],
    ctx: &RadialSlotResolutionContext,
    is_app_installed: impl Fn(&str) -> bool,
) -> [RadialSlot; RADIAL_SECTOR_COUNT] {
    std::array::from_fn(|idx| {
        let sector = SectorIndex::ALL[idx];
        let cfg = &configs[idx];
        let octant = octant_center_for_sector(sector, RADIAL_CANVAS_SIZE_PT);
        let default_target_preview =
            match (&ctx.active_agent_display_name, &ctx.active_session_preview) {
                (Some(agent), Some(session)) if !session.is_empty() => {
                    Some(format!("{agent} · {session}"))
                }
                (Some(agent), _) => Some(agent.clone()),
                _ => None,
            };

        let (intent, unavailable_reason, target_preview) = match cfg.kind {
            RadialSlotKind::CheatSheet => {
                if ctx.has_active_profile {
                    (Some(RadialIntent::CheatSheet), None, None)
                } else {
                    (None, Some("当前配置不可用".to_string()), None)
                }
            }
            RadialSlotKind::OpenApp => {
                let bundle = cfg.bundle_id.as_deref().unwrap_or("");
                let installed = !bundle.is_empty() && is_app_installed(bundle);
                if installed {
                    (
                        Some(RadialIntent::OpenApp {
                            bundle_id: bundle.to_string(),
                        }),
                        None,
                        None,
                    )
                } else {
                    (None, Some("应用未安装".to_string()), None)
                }
            }
            RadialSlotKind::Prompt => {
                let sanitized = strip_trailing_newlines(cfg.text.as_deref().unwrap_or(""));
                if sanitized.trim().is_empty() {
                    (None, Some("未设置指令文本".to_string()), None)
                } else {
                    let adapter = cfg.adapter.clone().or_else(|| ctx.active_adapter.clone());
                    let available = adapter.is_some() || ctx.has_active_profile;
                    if available {
                        (
                            Some(RadialIntent::Prompt {
                                text: sanitized,
                                adapter,
                                submit: cfg.submit,
                            }),
                            None,
                            default_target_preview,
                        )
                    } else {
                        (None, Some("没有可用的目标配置".to_string()), None)
                    }
                }
            }
            RadialSlotKind::Submit => {
                let adapter = cfg.adapter.clone().or_else(|| ctx.active_adapter.clone());
                if adapter.is_some() {
                    (
                        Some(RadialIntent::Submit { adapter }),
                        None,
                        default_target_preview,
                    )
                } else {
                    (None, Some("当前配置不是桌面 Agent".to_string()), None)
                }
            }
            RadialSlotKind::Stop => {
                let adapter = cfg.adapter.clone().or_else(|| ctx.active_adapter.clone());
                if adapter.is_some() {
                    (
                        Some(RadialIntent::Stop { adapter }),
                        None,
                        default_target_preview,
                    )
                } else {
                    (None, Some("当前配置不是桌面 Agent".to_string()), None)
                }
            }
            RadialSlotKind::NavigateBack => {
                if ctx.can_navigate_back {
                    (Some(RadialIntent::NavigateBack), None, None)
                } else {
                    (None, Some("没有可后退的打开历史".to_string()), None)
                }
            }
            RadialSlotKind::NavigateForward => {
                if ctx.can_navigate_forward {
                    (Some(RadialIntent::NavigateForward), None, None)
                } else {
                    (None, Some("没有可前进的打开历史".to_string()), None)
                }
            }
        };

        RadialSlot {
            sector,
            label: cfg.label.clone(),
            target_preview,
            intent,
            unavailable_reason,
            symbol: cfg.symbol.clone(),
            color_hex: cfg.color_hex.clone(),
            octant,
        }
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RadialRouteStamp {
    pub document_revision: u64,
    pub host_revision: u64,
    pub profile_id: Option<String>,
    pub profile_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RadialMenuSession {
    pub input_session: u64,
    pub route: RadialRouteStamp,
    pub slots: [RadialSlot; RADIAL_SECTOR_COUNT],
    pub selected: Option<SectorIndex>,
    pub anchor_frame: ScreenRect,
}

impl RadialMenuSession {
    pub fn selected_intent(&self) -> Option<RadialIntent> {
        self.selected
            .and_then(|sector| self.slots.get(sector.as_usize())?.intent.clone())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RadialPositionSample {
    pub angle_turns: f64,
    pub distance: f64,
}

impl RadialPositionSample {
    pub const fn new(angle_turns: f64, distance: f64) -> Self {
        Self {
            angle_turns,
            distance,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RadialPhase {
    Disabled,
    WaitingForCenter,
    Idle,
    Showing(Box<RadialMenuSession>),
    Settling,
}

#[derive(Debug, Clone)]
pub struct RadialMenuState {
    phase: RadialPhase,
    confirm_mode: RadialConfirmMode,
    sector_entered_at: Option<Instant>,
    armed_at_perimeter: bool,
    last_armed_sector: Option<SectorIndex>,
    perimeter_exited_at: Option<Instant>,
}

impl Default for RadialMenuState {
    fn default() -> Self {
        Self::new()
    }
}

impl RadialMenuState {
    pub const REST_DISTANCE: f64 = 0.15;
    pub const CENTER_RETURN_THRESHOLD: f64 = 0.35;
    pub const OPEN_DISTANCE: f64 = 0.50;
    pub const SELECT_DISTANCE: f64 = 0.75;
    pub const SPRING_BACK_MAX_DURATION: Duration = Duration::from_millis(500);
    pub const PERIMETER_SNAP_BACK_MAX_DURATION: Duration = Duration::from_millis(250);

    pub fn new() -> Self {
        Self {
            phase: RadialPhase::Disabled,
            confirm_mode: RadialConfirmMode::EncoderPress,
            sector_entered_at: None,
            armed_at_perimeter: false,
            last_armed_sector: None,
            perimeter_exited_at: None,
        }
    }

    pub fn enabled(&self) -> bool {
        !matches!(self.phase, RadialPhase::Disabled)
    }

    pub fn confirm_mode(&self) -> RadialConfirmMode {
        self.confirm_mode
    }

    pub fn phase(&self) -> &RadialPhase {
        &self.phase
    }

    pub fn armed_at_perimeter(&self) -> bool {
        self.armed_at_perimeter
    }

    pub fn session(&self) -> Option<&RadialMenuSession> {
        match &self.phase {
            RadialPhase::Showing(s) => Some(s),
            _ => None,
        }
    }

    pub fn captures_confirm(&self) -> bool {
        matches!(self.phase, RadialPhase::Showing(_) | RadialPhase::Settling)
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.phase = if enabled {
            RadialPhase::WaitingForCenter
        } else {
            RadialPhase::Disabled
        };
        self.clear_spring_arm();
    }

    pub fn set_confirm_mode(&mut self, mode: RadialConfirmMode) {
        self.confirm_mode = mode;
    }

    pub fn cancel(&mut self) {
        if self.enabled() {
            self.phase = RadialPhase::WaitingForCenter;
            self.clear_spring_arm();
        }
    }

    pub fn select_sector_in_place(&mut self, sector: SectorIndex, now: Instant) {
        if let RadialPhase::Showing(session) = &mut self.phase {
            session.selected = Some(sector);
            self.sector_entered_at = Some(now);
        }
    }

    pub fn sector(sample: RadialPositionSample) -> Option<SectorIndex> {
        SectorIndex::from_polar(sample.angle_turns, sample.distance)
    }

    pub fn update(
        &mut self,
        sample: RadialPositionSample,
        now: Instant,
        make_session: impl FnOnce() -> Option<RadialMenuSession>,
    ) -> Option<(RadialMenuSession, RadialIntent)> {
        if !self.enabled()
            || !sample.distance.is_finite()
            || !(0.0..=1.0).contains(&sample.distance)
        {
            return None;
        }

        if sample.distance < Self::CENTER_RETURN_THRESHOLD {
            if self.confirm_mode == RadialConfirmMode::SpringBack {
                if let RadialPhase::Showing(mut session) = self.phase.clone() {
                    let entered_ok = self.sector_entered_at.is_some_and(|entered| {
                        now.saturating_duration_since(entered) <= Self::SPRING_BACK_MAX_DURATION
                    });
                    let exited_ok = self.perimeter_exited_at.is_none_or(|exited| {
                        now.saturating_duration_since(exited)
                            <= Self::PERIMETER_SNAP_BACK_MAX_DURATION
                    });
                    if self.armed_at_perimeter && entered_ok && exited_ok {
                        if let Some(target_sector) = session.selected.or(self.last_armed_sector) {
                            if let Some(intent) =
                                session.slots[target_sector.as_usize()].intent.clone()
                            {
                                session.selected = Some(target_sector);
                                self.phase = RadialPhase::Settling;
                                self.clear_spring_arm();
                                return Some((*session, intent));
                            }
                        }
                    }
                }
            }
            self.phase = RadialPhase::Idle;
            self.clear_spring_arm();
            return None;
        }

        match &mut self.phase {
            RadialPhase::Idle => {
                let sector = Self::sector(sample)?;
                let mut session = make_session()?;
                session.selected = Some(sector);
                self.sector_entered_at = Some(now);
                if sample.distance >= Self::SELECT_DISTANCE {
                    self.armed_at_perimeter = true;
                    self.last_armed_sector = Some(sector);
                    self.perimeter_exited_at = None;
                } else {
                    self.armed_at_perimeter = false;
                    self.last_armed_sector = None;
                    self.perimeter_exited_at = None;
                }
                self.phase = RadialPhase::Showing(Box::new(session));
            }
            RadialPhase::Showing(session) => {
                let new_sector = Self::sector(sample);
                if new_sector != session.selected {
                    session.selected = new_sector;
                    if new_sector.is_some() {
                        self.sector_entered_at = Some(now);
                        self.armed_at_perimeter = sample.distance >= Self::SELECT_DISTANCE;
                        self.last_armed_sector = if self.armed_at_perimeter {
                            new_sector
                        } else {
                            None
                        };
                        self.perimeter_exited_at = None;
                    } else if self.armed_at_perimeter && self.perimeter_exited_at.is_none() {
                        self.perimeter_exited_at = Some(now);
                    }
                } else if let Some(current_sector) = new_sector {
                    if sample.distance >= Self::SELECT_DISTANCE {
                        self.armed_at_perimeter = true;
                        self.last_armed_sector = Some(current_sector);
                        self.perimeter_exited_at = None;
                    } else if self.armed_at_perimeter && self.perimeter_exited_at.is_none() {
                        self.perimeter_exited_at = Some(now);
                    }
                }
            }
            RadialPhase::Disabled | RadialPhase::WaitingForCenter | RadialPhase::Settling => {}
        }

        None
    }

    pub fn confirm(&mut self) -> Option<(RadialMenuSession, RadialIntent)> {
        let session = self.session()?.clone();
        let intent = session.selected_intent()?;
        self.phase = RadialPhase::Settling;
        self.clear_spring_arm();
        Some((session, intent))
    }

    fn clear_spring_arm(&mut self) {
        self.sector_entered_at = None;
        self.armed_at_perimeter = false;
        self.last_armed_sector = None;
        self.perimeter_exited_at = None;
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RadialInputEvent {
    Position {
        session_id: u64,
        ingress_seq: u64,
        sample: RadialPositionSample,
        admitted: bool,
    },
    KeyPress {
        session_id: u64,
        ingress_seq: u64,
        binding_id: String,
        admitted: bool,
    },
    EscapeKeyDown {
        key_code: u16,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct RadialConsumeOutcome {
    pub consumed: bool,
    pub confirmed_intent: Option<RadialIntent>,
    pub dispatched_receipt: Option<mi_macos::HostActionReceipt>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RadialOverlayProjection {
    pub enabled: bool,
    pub readiness: RadialInputReadiness,
    pub confirm_mode: RadialConfirmMode,
    pub visible_session: Option<RadialMenuSession>,
    pub panel_spec: Option<PassivePanelFrameSpec>,
}

impl Default for RadialOverlayProjection {
    fn default() -> Self {
        Self {
            enabled: false,
            readiness: RadialInputReadiness::Ready,
            confirm_mode: RadialConfirmMode::EncoderPress,
            visible_session: None,
            panel_spec: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RadialOverlayAction {
    CancelEscape,
    SelectSector(SectorIndex),
    ConfirmSelected,
}
