use mi_model::host_router::canonical_desktop_agent_rank;
use serde::{Deserialize, Serialize};

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CanonicalAgent {
    Antigravity = 0,
    Codex = 1,
    DeepseekHarness = 2,
    Herdr = 3,
    OtherApps = 4,
}

impl CanonicalAgent {
    pub const ALL: [Self; 5] = [
        Self::Antigravity,
        Self::Codex,
        Self::DeepseekHarness,
        Self::Herdr,
        Self::OtherApps,
    ];

    pub const fn rank(self) -> usize {
        self as usize
    }

    pub const fn profile_name(self) -> &'static str {
        match self {
            Self::Antigravity => "Antigravity",
            Self::Codex => "Codex",
            Self::DeepseekHarness => "DeepSeek Coder (DSH)",
            Self::Herdr => "Herdr",
            Self::OtherApps => "其他软件",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Antigravity => "Antigravity",
            Self::Codex => "Codex",
            Self::DeepseekHarness => "Deepseek Harness",
            Self::Herdr => "Herdr",
            Self::OtherApps => "其他软件",
        }
    }

    pub const fn bundle_id(self) -> &'static str {
        match self {
            Self::Antigravity => "com.google.antigravity",
            Self::Codex => "com.openai.codex",
            Self::DeepseekHarness => "com.deepseek.dsh",
            Self::Herdr => "dev.herdr.app",
            Self::OtherApps => "*",
        }
    }

    pub const fn badge_monogram(self) -> &'static str {
        match self {
            Self::Antigravity => "AG",
            Self::Codex => "CX",
            Self::DeepseekHarness => "DS",
            Self::Herdr => "HD",
            Self::OtherApps => "通用",
        }
    }

    pub const fn is_manual_fallback(self) -> bool {
        matches!(self, Self::OtherApps)
    }

    pub fn from_profile_name(name: &str) -> Option<Self> {
        let rank = canonical_desktop_agent_rank(name)?;
        Self::ALL.get(rank).copied()
    }

    pub fn from_bundle_id(bundle_id: &str) -> Option<Self> {
        let trimmed = bundle_id.trim();
        Self::ALL
            .into_iter()
            .find(|agent| agent.bundle_id() == trimmed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AutoSwitchState {
    Unloaded,
    Disabled,
    Active,
    Paused,
}

impl AutoSwitchState {
    pub const fn from_flags(
        loaded: bool,
        auto_follow_enabled: bool,
        manual_override: bool,
    ) -> Self {
        if !loaded {
            Self::Unloaded
        } else if !auto_follow_enabled {
            Self::Disabled
        } else if manual_override {
            Self::Paused
        } else {
            Self::Active
        }
    }

    pub const fn is_enabled(self) -> bool {
        matches!(self, Self::Active | Self::Paused)
    }

    pub const fn status_label(self) -> &'static str {
        match self {
            Self::Unloaded => "文档未载入",
            Self::Disabled => "手动选择软件",
            Self::Active => "前台切到已安装软件时自动切换",
            Self::Paused => "已手动固定当前软件，点右侧恢复自动切换",
        }
    }

    pub const fn action_button_title(self) -> Option<&'static str> {
        match self {
            Self::Paused => Some("恢复自动"),
            Self::Active => Some("固定当前"),
            Self::Unloaded | Self::Disabled => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OwnershipBannerState {
    Ready { summary: String },
    NativeHeld { summary: String },
    Caution { reason: String },
}

impl OwnershipBannerState {
    pub fn evaluate(is_protected_active_profile: bool) -> Self {
        if is_protected_active_profile {
            Self::NativeHeld {
                summary: "受保护宿主层 · 按键编辑与灯光不可用".into(),
            }
        } else {
            Self::Ready {
                summary: "本机按键控制".into(),
            }
        }
    }

    pub fn is_caution(&self) -> bool {
        matches!(self, Self::Caution { .. })
    }

    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }

    pub fn text(&self) -> &str {
        match self {
            Self::Ready { summary } | Self::NativeHeld { summary } => summary,
            Self::Caution { reason } => reason,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidebarProfileRow {
    pub profile_id: String,
    pub raw_name: String,
    pub display_name: String,
    pub badge_monogram: String,
    pub canonical_agent: Option<CanonicalAgent>,
    pub is_selected: bool,
    pub is_active_host: bool,
    pub is_protected_oai: bool,
    pub app_installed: bool,
    pub is_manual_fallback: bool,
}

pub const HARDWARE_PROFILE_SLOT_COUNT: usize = 4;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum HardwareProfileSlotKind {
    UsbPlug = 0,
    TouchSlot1 = 1,
    TouchSlot2 = 2,
    TouchSlot3 = 3,
}

impl HardwareProfileSlotKind {
    pub const ALL: [Self; HARDWARE_PROFILE_SLOT_COUNT] = [
        Self::UsbPlug,
        Self::TouchSlot1,
        Self::TouchSlot2,
        Self::TouchSlot3,
    ];

    pub const DEFAULT_AGENTS: [CanonicalAgent; HARDWARE_PROFILE_SLOT_COUNT] = [
        CanonicalAgent::Antigravity,
        CanonicalAgent::Antigravity,
        CanonicalAgent::Codex,
        CanonicalAgent::DeepseekHarness,
    ];

    pub const fn from_index(index: u8) -> Option<Self> {
        match index {
            0 => Some(Self::UsbPlug),
            1 => Some(Self::TouchSlot1),
            2 => Some(Self::TouchSlot2),
            3 => Some(Self::TouchSlot3),
            _ => None,
        }
    }

    pub const fn index(self) -> u8 {
        self as u8
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::UsbPlug => "插入 USB",
            Self::TouchSlot1 => "左下触控 1",
            Self::TouchSlot2 => "左下触控 2",
            Self::TouchSlot3 => "左下触控 3",
        }
    }

    pub const fn trigger_badge(self) -> &'static str {
        match self {
            Self::UsbPlug => "USB",
            Self::TouchSlot1 => "LED 1 · ●○○",
            Self::TouchSlot2 => "LED 2 · ○●○",
            Self::TouchSlot3 => "LED 3 · ○○●",
        }
    }

    pub const fn default_agent(self) -> CanonicalAgent {
        Self::DEFAULT_AGENTS[self as usize]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareProfileSlotProjection {
    pub slot_index: u8,
    pub slot_kind: HardwareProfileSlotKind,
    pub title: String,
    pub trigger_badge: String,
    pub profile_id: String,
    pub display_name: String,
    pub monogram: String,
    pub bundle_id: Option<String>,
    pub is_active_slot: bool,
    pub is_active_profile: bool,
}

impl Default for HardwareProfileSlotProjection {
    fn default() -> Self {
        Self::for_kind(HardwareProfileSlotKind::UsbPlug)
    }
}

impl HardwareProfileSlotProjection {
    pub fn for_kind(kind: HardwareProfileSlotKind) -> Self {
        let agent = kind.default_agent();
        Self {
            slot_index: kind.index(),
            slot_kind: kind,
            title: kind.title().to_string(),
            trigger_badge: kind.trigger_badge().to_string(),
            profile_id: String::new(),
            display_name: agent.display_name().to_string(),
            monogram: agent.badge_monogram().to_string(),
            bundle_id: Some(agent.bundle_id().to_string()),
            is_active_slot: kind == HardwareProfileSlotKind::UsbPlug,
            is_active_profile: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidebarProjection {
    pub section_title: String,
    pub rows: Vec<SidebarProfileRow>,
    pub hardware_slots: [HardwareProfileSlotProjection; HARDWARE_PROFILE_SLOT_COUNT],
    pub active_hardware_slot: u8,
    pub selected_profile_id: Option<String>,
    pub selected_display_name: Option<String>,
    pub active_host_profile_id: Option<String>,
    pub auto_switch: AutoSwitchState,
    pub show_other_profiles: bool,
    pub can_edit_draft: bool,
    pub editing_device: bool,
    pub can_move_up: bool,
    pub can_move_down: bool,
    pub can_delete: bool,
    pub busy: bool,
    pub ownership_banner: OwnershipBannerState,
    pub footer_status_text: String,
    pub footer_limit_text: String,
}

impl Default for SidebarProjection {
    fn default() -> Self {
        Self {
            section_title: "常用软件方案".to_string(),
            rows: Vec::new(),
            hardware_slots: std::array::from_fn(|idx| {
                HardwareProfileSlotProjection::for_kind(HardwareProfileSlotKind::ALL[idx])
            }),
            active_hardware_slot: 0,
            selected_profile_id: None,
            selected_display_name: None,
            active_host_profile_id: None,
            auto_switch: AutoSwitchState::Unloaded,
            show_other_profiles: false,
            can_edit_draft: false,
            editing_device: false,
            can_move_up: false,
            can_move_down: false,
            can_delete: false,
            busy: false,
            ownership_banner: OwnershipBannerState::Ready {
                summary: "就绪".to_string(),
            },
            footer_status_text: "就绪".to_string(),
            footer_limit_text: "长按说话上限 120 秒，响应超时 8 秒".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProfileCommand {
    Create { name: String },
    Rename { profile_id: String, name: String },
    Duplicate { profile_id: String, name: String },
    MoveUp { profile_id: String },
    MoveDown { profile_id: String },
    Delete { profile_id: String },
    InstallCodingAgentPresets,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackupCommand {
    ExportPreset {
        profile_id: Option<String>,
        layer_id: Option<String>,
    },
    ImportPreset {
        text: String,
    },
    ReturnToHostDocument,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SidebarAction {
    SelectProfile(String),
    ActivateHardwareSlot(u8),
    CycleTouchProfileSlot,
    SetHardwareSlotProfile { slot_index: u8, profile_id: String },
    ResetHardwareSlotsToDefault,
    SetAutoFollow(bool),
    ToggleAutoFollowPause,
    SetShowOtherProfiles(bool),
    Profile(ProfileCommand),
    Backup(BackupCommand),
}
