use crate::overlay::CheatSheetProjection;
use crate::radial::{RadialConfirmMode, RadialPresetKind};
use crate::sidebar::CanonicalAgent;
use mi_macos::StatusItemIconSpec;
use serde::{Deserialize, Serialize};

pub const FAVORITE_SLOT_COUNT: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct FavoriteSlotIndex(u8);

impl FavoriteSlotIndex {
    pub const SLOT_0: Self = Self(0);
    pub const SLOT_1: Self = Self(1);
    pub const SLOT_2: Self = Self(2);
    pub const SLOT_3: Self = Self(3);

    pub const ALL: [Self; FAVORITE_SLOT_COUNT] =
        [Self::SLOT_0, Self::SLOT_1, Self::SLOT_2, Self::SLOT_3];

    pub const fn new(raw: u8) -> Option<Self> {
        if raw < FAVORITE_SLOT_COUNT as u8 {
            Some(Self(raw))
        } else {
            None
        }
    }

    pub const fn from_usize(index: usize) -> Option<Self> {
        if index < FAVORITE_SLOT_COUNT {
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PresetExportScope {
    Document,
    Profile,
    Layer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingPresetImportPreview {
    pub raw_text: String,
    pub name: String,
    pub profile_count: usize,
    pub layer_count: usize,
    pub action_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PinnedFavoritesGrid {
    pub slots: [Option<String>; FAVORITE_SLOT_COUNT],
}

impl PinnedFavoritesGrid {
    pub const DEFAULT_AGENTS: [CanonicalAgent; FAVORITE_SLOT_COUNT] = [
        CanonicalAgent::Antigravity,
        CanonicalAgent::DeepseekHarness,
        CanonicalAgent::Herdr,
        CanonicalAgent::Codex,
    ];

    pub fn slot(&self, index: FavoriteSlotIndex) -> Option<&str> {
        self.slots[index.as_usize()].as_deref()
    }

    pub fn assign_or_swap(&mut self, slot: FavoriteSlotIndex, profile_id: String) -> bool {
        let target_idx = slot.as_usize();
        if let Some(existing_idx) = self
            .slots
            .iter()
            .position(|entry| entry.as_deref() == Some(profile_id.as_str()))
        {
            if existing_idx != target_idx {
                self.slots.swap(target_idx, existing_idx);
                return true;
            }
            return false;
        }
        self.slots[target_idx] = Some(profile_id);
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FavoriteSlotCardProjection {
    pub slot: FavoriteSlotIndex,
    pub profile_id: Option<String>,
    pub title: String,
    pub is_active: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusMenuProjection {
    pub icon: StatusItemIconSpec,
    pub active_profile_id: Option<String>,
    pub active_profile_name: String,
    pub routing_mode_summary: String,
    #[serde(default)]
    pub is_profile_locked: bool,
    #[serde(default)]
    pub locked_profile_id: Option<String>,
    #[serde(default = "default_true")]
    pub accessibility_trusted: bool,
    #[serde(default)]
    pub competing_host_notice: Option<String>,
    pub favorites_2x2: [FavoriteSlotCardProjection; FAVORITE_SLOT_COUNT],
    pub host_execution_enabled: bool,
    pub auto_follow_enabled: bool,
    pub manual_override: bool,
    pub radial_menu_enabled: bool,
    pub radial_confirm_mode: RadialConfirmMode,
    pub lighting_enabled: bool,
    pub cheat_sheet_preview: Option<CheatSheetProjection>,
    pub cheat_sheet_visible: bool,
    pub can_export_profile: bool,
    pub can_export_layer: bool,
    pub pending_import: Option<PendingPresetImportPreview>,
    pub pending_export_scope: Option<PresetExportScope>,
    pub preset_error: Option<String>,
    #[serde(default)]
    pub power_status: Option<crate::header::PowerStatusBadgeProjection>,
    #[serde(default)]
    pub candidate_devices: Vec<crate::header::CandidateDevice>,
    #[serde(default)]
    pub preferred_device_serial: Option<String>,
}

impl Default for StatusMenuProjection {
    fn default() -> Self {
        Self {
            icon: StatusItemIconSpec::default(),
            active_profile_id: None,
            active_profile_name: "未选择方案".to_string(),
            routing_mode_summary: "手动模式".to_string(),
            is_profile_locked: false,
            locked_profile_id: None,
            accessibility_trusted: true,
            competing_host_notice: None,
            favorites_2x2: std::array::from_fn(|idx| FavoriteSlotCardProjection {
                slot: FavoriteSlotIndex::ALL[idx],
                profile_id: None,
                title: "未绑定".to_string(),
                is_active: false,
            }),
            host_execution_enabled: false,
            auto_follow_enabled: false,
            manual_override: false,
            radial_menu_enabled: false,
            radial_confirm_mode: RadialConfirmMode::EncoderPress,
            lighting_enabled: true,
            cheat_sheet_preview: None,
            cheat_sheet_visible: false,
            can_export_profile: false,
            can_export_layer: false,
            pending_import: None,
            pending_export_scope: None,
            preset_error: None,
            power_status: None,
            candidate_devices: Vec::new(),
            preferred_device_serial: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatusMenuAction {
    ActivateFavoriteSlot(FavoriteSlotIndex),
    PinFavoriteSlot {
        slot: FavoriteSlotIndex,
        profile_id: String,
    },
    SelectProfile(String),
    SelectCandidateDevice {
        selector: String,
    },
    SetProfileLocked(bool),
    SetHostExecutionEnabled(bool),
    OpenAccessibilitySettings,
    SetAutoFollowEnabled(bool),
    ResumeAutoFollow,
    SetRadialMenuEnabled(bool),
    SetRadialConfirmMode(RadialConfirmMode),
    ApplyRadialPreset(RadialPresetKind),
    ToggleCheatSheet,
    StagePresetImportBytes(Vec<u8>),
    ConfirmStagedPresetImport,
    CancelStagedPresetImport,
    RequestPresetExport(PresetExportScope),
    ConfirmPresetExport,
    CancelPresetExport,
    OpenMainWorkbenchWindow,
}
