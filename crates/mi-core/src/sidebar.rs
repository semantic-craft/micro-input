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

    pub const fn is_native_oai_default(self) -> bool {
        matches!(self, Self::Codex)
    }

    pub fn from_profile_name(name: &str) -> Option<Self> {
        if name == mi_model::preset::LOCAL_CODEX_PROFILE_NAME {
            return Some(Self::Codex);
        }
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardProfileChoice {
    pub profile_id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardLayerProjection {
    pub layer: u8,
    pub leds: [bool; 3],
    pub profile_id: Option<String>,
    pub display_name: String,
    pub owner: String,
    pub is_preview_selected: bool,
    pub assignment_issue: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardLayersProjection {
    pub layers: [BoardLayerProjection; 6],
    pub profile_choices: Vec<BoardProfileChoice>,
    pub usb_startup_profile_id: Option<String>,
    pub usb_startup_display_name: String,
    pub usb_preview_selected: bool,
    pub observed_layer: Option<u8>,
    pub observed_profile_kind: Option<String>,
    pub observed_status: String,
    pub apply_block_reason: String,
    pub can_apply: bool,
    pub configuration_observed: bool,
    pub keyboard_grabbed: bool,
    pub first_event_verified: bool,
    pub runtime_ready: bool,
    pub restore_available: bool,
}

impl Default for BoardLayersProjection {
    fn default() -> Self {
        Self {
            layers: std::array::from_fn(|i| BoardLayerProjection {
                layer: mi_model::board_layers::BoardLayer::ALL[i].number(),
                leds: mi_model::board_layers::BoardLayer::ALL[i].leds(),
                profile_id: None,
                display_name: "未分配".into(),
                owner: "Unassigned".into(),
                is_preview_selected: false,
                assignment_issue: None,
            }),
            profile_choices: Vec::new(),
            usb_startup_profile_id: None,
            usb_startup_display_name: "未分配".into(),
            usb_preview_selected: true,
            observed_layer: None,
            observed_profile_kind: None,
            observed_status: "设备层未确认".into(),
            apply_block_reason: "六层输入与写入协议尚未验证，当前仅保存本地方案".into(),
            can_apply: false,
            configuration_observed: false,
            keyboard_grabbed: false,
            first_event_verified: false,
            runtime_ready: false,
            restore_available: false,
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidebarProjection {
    pub section_title: String,
    pub rows: Vec<SidebarProfileRow>,
    pub board_layers: BoardLayersProjection,
    pub selected_profile_id: Option<String>,
    pub selected_display_name: Option<String>,
    pub active_host_profile_id: Option<String>,
    #[serde(default)]
    pub is_profile_locked: bool,
    #[serde(default)]
    pub locked_profile_id: Option<String>,
    #[serde(default = "default_true")]
    pub accessibility_trusted: bool,
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
            board_layers: BoardLayersProjection::default(),
            selected_profile_id: None,
            selected_display_name: None,
            active_host_profile_id: None,
            is_profile_locked: false,
            locked_profile_id: None,
            accessibility_trusted: true,
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
    BrowseProfile(String),
    PreviewBoardLayer(mi_model::board_layers::BoardLayer),
    PreviewUsbStartup,
    ProvisionBoardLayers,

    RestoreBoardLayers,
    SetBoardLayerProfile {
        layer: mi_model::board_layers::BoardLayer,
        profile_id: Option<String>,
    },
    SetUsbStartupProfile(Option<String>),
    ResetBoardLayerPlan,
    SetProfileLocked(bool),
    SetAutoFollow(bool),
    ToggleAutoFollowPause,
    SetShowOtherProfiles(bool),
    Profile(ProfileCommand),
    Backup(BackupCommand),
}
