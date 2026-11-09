use mi_model::{
    comparison::Choice as ComparisonChoice,
    lighting::{Effect as LightingEffect, Zone as LightingZone},
    macro_sequence::MacroEvent,
    multi::ActionReference,
    DeviceConfig,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MacroStepDraft {
    pub index: usize,
    pub event: MacroEvent,
    pub summary_label: String,
    pub macos_key_supported: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MacroEditorModalProjection {
    pub macro_id: Option<String>,
    pub name: String,
    pub steps: Vec<MacroStepDraft>,
    pub draft_key_input: String,
    pub draft_delay_ms: u32,
    pub bind_to_selected_slot: bool,
    pub target_slot: Option<u32>,
    pub reference_summaries: Vec<String>,
    pub validation_error: Option<String>,
    pub host_execution_supported: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightingZoneProjection {
    pub zone: LightingZone,
    pub zone_title: String,
    pub effect: Option<LightingEffect>,
    pub brightness_percent: u8,
    pub speed_percent: u8,
    pub magic_percent: u8,
    pub color_hex: String,
    pub can_edit_color: bool,
    pub can_edit_brightness: bool,
    pub can_edit_speed: bool,
    pub can_edit_magic: bool,
}

impl PartialEq for LightingZoneProjection {
    fn eq(&self, other: &Self) -> bool {
        self.zone.name() == other.zone.name()
            && self.zone_title == other.zone_title
            && self.effect == other.effect
            && self.brightness_percent == other.brightness_percent
            && self.speed_percent == other.speed_percent
            && self.magic_percent == other.magic_percent
            && self.color_hex == other.color_hex
            && self.can_edit_color == other.can_edit_color
            && self.can_edit_brightness == other.can_edit_brightness
            && self.can_edit_speed == other.can_edit_speed
            && self.can_edit_magic == other.can_edit_magic
    }
}

impl Eq for LightingZoneProjection {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LightingEditorModalProjection {
    pub profile_id: String,
    pub layer_id: String,
    pub layer_name: String,
    pub apply_to_all_layers: bool,
    pub can_add_lighting: bool,
    pub has_lighting: bool,
    pub backlight: Option<LightingZoneProjection>,
    pub underglow: Option<LightingZoneProjection>,
    pub preview_active: bool,
    pub preview_remaining_secs: u8,
    pub validation_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum MultiBranchKind {
    Tap,
    DoubleTap,
    Hold,
    TapHold,
}

impl MultiBranchKind {
    pub const ALL: [Self; 4] = [Self::Tap, Self::DoubleTap, Self::Hold, Self::TapHold];

    pub const fn title(self) -> &'static str {
        match self {
            Self::Tap => "单击 (Tap)",
            Self::DoubleTap => "双击 (Double Tap)",
            Self::Hold => "长按 (Hold)",
            Self::TapHold => "点击后长按 (Tap + Hold)",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MultiBranchTarget {
    Action { action_id: String },
    Macro { action_id: String },
}

impl MultiBranchTarget {
    pub fn to_action_reference(&self) -> ActionReference {
        match self {
            Self::Action { action_id } => ActionReference::Action {
                action_id: action_id.clone(),
            },
            Self::Macro { action_id } => ActionReference::Macro {
                action_id: action_id.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultiBranchSlotProjection {
    pub branch: MultiBranchKind,
    pub title: String,
    pub target: Option<MultiBranchTarget>,
    pub target_display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultiEditorModalProjection {
    pub multi_id: Option<String>,
    pub name: String,
    pub term_ms: u32,
    pub branches: [MultiBranchSlotProjection; 4],
    pub bind_to_selected_slot: bool,
    pub target_slot: Option<u32>,
    pub available_actions: Vec<(String, String)>,
    pub available_macros: Vec<(String, String)>,
    pub reference_summaries: Vec<String>,
    pub validation_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComparisonRowProjection {
    pub path: String,
    pub label: String,
    pub base_summary: String,
    pub local_summary: String,
    pub device_summary: String,
    pub result_badge: String,
    pub is_conflict: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconnectComparisonModalProjection {
    pub token: String,
    pub revision: u64,
    pub identity_matches: bool,
    pub identity_conflict: bool,
    pub identity_confirmation_required: bool,
    pub identity_user_confirmed: bool,
    pub can_merge: bool,
    pub conflicts: Vec<String>,
    pub rows: Vec<ComparisonRowProjection>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyChangeRowProjection {
    pub kind: String,
    pub location_label: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyPreparationModalProjection {
    pub token: String,
    pub kind: String,
    pub document_id: String,
    pub revision: u64,
    pub document_sha256: String,
    pub planned_keymap_sha256: String,
    pub before_summary: String,
    pub after_summary: String,
    pub changes: Vec<ApplyChangeRowProjection>,
    pub backup_prepared: bool,
    pub backup_path: Option<String>,
    pub manifest_sha256: Option<String>,
    pub candidate_taken: bool,
    pub candidate_operation_id: Option<String>,
    pub unavailable_reason: String,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EditorModalProjection {
    MacroEditor(MacroEditorModalProjection),
    LightingEditor(LightingEditorModalProjection),
    MultiEditor(MultiEditorModalProjection),
    ReconnectComparison(ReconnectComparisonModalProjection),
    ApplyPreparation(ApplyPreparationModalProjection),
}

impl EditorModalProjection {
    pub const fn is_mutating_layer_editor(&self) -> bool {
        matches!(
            self,
            Self::MacroEditor(_) | Self::LightingEditor(_) | Self::MultiEditor(_)
        )
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub enum EditorAction {
    CloseModal,
    OpenMacroEditor {
        macro_id: Option<String>,
    },
    MacroSetName(String),
    MacroSetDraftKeyInput(String),
    MacroSetDraftDelayMs(u32),
    MacroAppendEvent(MacroEvent),
    MacroAddTapKey {
        key: String,
        delay_ms: u32,
    },
    MacroRemoveStep(usize),
    MacroMoveStep {
        from: usize,
        to: usize,
    },
    MacroClearSteps,
    MacroSave {
        bind_to_selected_slot: bool,
    },
    MacroBindExisting {
        macro_id: Option<String>,
    },
    MacroDelete {
        macro_id: String,
    },
    OpenLightingEditor,
    LightingSetApplyToAllLayers(bool),
    LightingAddDefault,
    LightingEditEffect {
        zone: LightingZone,
        effect: LightingEffect,
        all_layers: bool,
    },
    LightingEditBrightness {
        zone: LightingZone,
        brightness_percent: u8,
        all_layers: bool,
    },
    LightingEditSpeed {
        zone: LightingZone,
        speed_percent: u8,
        all_layers: bool,
    },
    LightingEditMagic {
        zone: LightingZone,
        magic_percent: u8,
        all_layers: bool,
    },
    LightingEditColorHex {
        zone: LightingZone,
        color_hex: String,
        all_layers: bool,
    },
    LightingResetLayer,
    LightingBeginPreview,
    LightingEndPreview,
    OpenMultiEditor {
        multi_id: Option<String>,
    },
    MultiSetName(String),
    MultiSetTermMs(u32),
    MultiSetBranch {
        branch: MultiBranchKind,
        target: Option<MultiBranchTarget>,
    },
    MultiSave {
        bind_to_selected_slot: bool,
    },
    MultiBindExisting {
        multi_id: Option<String>,
    },
    MultiDelete {
        multi_id: String,
    },
    OpenReconnectComparison {
        device_config: Box<DeviceConfig>,
        baseline: Value,
    },
    ComparisonConfirmIdentity {
        current_snapshot: Value,
    },
    ComparisonResolve {
        choice: ComparisonChoice,
        current_snapshot: Value,
    },
    OpenApplyPreparation {
        current_snapshot: Value,
    },
    ApplyPrepareBackup {
        current_snapshot: Value,
    },
    ApplyTakePreparedCandidate {
        current_snapshot: Value,
    },
    ApplyCancelPreview,
}

fn choice_name(choice: ComparisonChoice) -> &'static str {
    match choice {
        ComparisonChoice::Merge => "Merge",
        ComparisonChoice::KeepLocal => "KeepLocal",
        ComparisonChoice::UseDevice => "UseDevice",
        ComparisonChoice::Cancel => "Cancel",
    }
}

impl std::fmt::Debug for EditorAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CloseModal => write!(f, "CloseModal"),
            Self::OpenMacroEditor { macro_id } => f
                .debug_struct("OpenMacroEditor")
                .field("macro_id", macro_id)
                .finish(),
            Self::MacroSetName(s) => f.debug_tuple("MacroSetName").field(s).finish(),
            Self::MacroSetDraftKeyInput(s) => {
                f.debug_tuple("MacroSetDraftKeyInput").field(s).finish()
            }
            Self::MacroSetDraftDelayMs(ms) => {
                f.debug_tuple("MacroSetDraftDelayMs").field(ms).finish()
            }
            Self::MacroAppendEvent(ev) => f.debug_tuple("MacroAppendEvent").field(ev).finish(),
            Self::MacroAddTapKey { key, delay_ms } => f
                .debug_struct("MacroAddTapKey")
                .field("key", key)
                .field("delay_ms", delay_ms)
                .finish(),
            Self::MacroRemoveStep(i) => f.debug_tuple("MacroRemoveStep").field(i).finish(),
            Self::MacroMoveStep { from, to } => f
                .debug_struct("MacroMoveStep")
                .field("from", from)
                .field("to", to)
                .finish(),
            Self::MacroClearSteps => write!(f, "MacroClearSteps"),
            Self::MacroSave {
                bind_to_selected_slot,
            } => f
                .debug_struct("MacroSave")
                .field("bind_to_selected_slot", bind_to_selected_slot)
                .finish(),
            Self::MacroBindExisting { macro_id } => f
                .debug_struct("MacroBindExisting")
                .field("macro_id", macro_id)
                .finish(),
            Self::MacroDelete { macro_id } => f
                .debug_struct("MacroDelete")
                .field("macro_id", macro_id)
                .finish(),
            Self::OpenLightingEditor => write!(f, "OpenLightingEditor"),
            Self::LightingSetApplyToAllLayers(v) => f
                .debug_tuple("LightingSetApplyToAllLayers")
                .field(v)
                .finish(),
            Self::LightingAddDefault => write!(f, "LightingAddDefault"),
            Self::LightingEditEffect {
                zone,
                effect,
                all_layers,
            } => f
                .debug_struct("LightingEditEffect")
                .field("zone", &zone.name())
                .field("effect", effect)
                .field("all_layers", all_layers)
                .finish(),
            Self::LightingEditBrightness {
                zone,
                brightness_percent,
                all_layers,
            } => f
                .debug_struct("LightingEditBrightness")
                .field("zone", &zone.name())
                .field("brightness_percent", brightness_percent)
                .field("all_layers", all_layers)
                .finish(),
            Self::LightingEditSpeed {
                zone,
                speed_percent,
                all_layers,
            } => f
                .debug_struct("LightingEditSpeed")
                .field("zone", &zone.name())
                .field("speed_percent", speed_percent)
                .field("all_layers", all_layers)
                .finish(),
            Self::LightingEditMagic {
                zone,
                magic_percent,
                all_layers,
            } => f
                .debug_struct("LightingEditMagic")
                .field("zone", &zone.name())
                .field("magic_percent", magic_percent)
                .field("all_layers", all_layers)
                .finish(),
            Self::LightingEditColorHex {
                zone,
                color_hex,
                all_layers,
            } => f
                .debug_struct("LightingEditColorHex")
                .field("zone", &zone.name())
                .field("color_hex", color_hex)
                .field("all_layers", all_layers)
                .finish(),
            Self::LightingResetLayer => write!(f, "LightingResetLayer"),
            Self::LightingBeginPreview => write!(f, "LightingBeginPreview"),
            Self::LightingEndPreview => write!(f, "LightingEndPreview"),
            Self::OpenMultiEditor { multi_id } => f
                .debug_struct("OpenMultiEditor")
                .field("multi_id", multi_id)
                .finish(),
            Self::MultiSetName(s) => f.debug_tuple("MultiSetName").field(s).finish(),
            Self::MultiSetTermMs(ms) => f.debug_tuple("MultiSetTermMs").field(ms).finish(),
            Self::MultiSetBranch { branch, target } => f
                .debug_struct("MultiSetBranch")
                .field("branch", branch)
                .field("target", target)
                .finish(),
            Self::MultiSave {
                bind_to_selected_slot,
            } => f
                .debug_struct("MultiSave")
                .field("bind_to_selected_slot", bind_to_selected_slot)
                .finish(),
            Self::MultiBindExisting { multi_id } => f
                .debug_struct("MultiBindExisting")
                .field("multi_id", multi_id)
                .finish(),
            Self::MultiDelete { multi_id } => f
                .debug_struct("MultiDelete")
                .field("multi_id", multi_id)
                .finish(),
            Self::OpenReconnectComparison {
                device_config,
                baseline,
            } => f
                .debug_struct("OpenReconnectComparison")
                .field("device_config", device_config)
                .field("baseline", baseline)
                .finish(),
            Self::ComparisonConfirmIdentity { current_snapshot } => f
                .debug_struct("ComparisonConfirmIdentity")
                .field("current_snapshot", current_snapshot)
                .finish(),
            Self::ComparisonResolve {
                choice,
                current_snapshot,
            } => f
                .debug_struct("ComparisonResolve")
                .field("choice", &choice_name(*choice))
                .field("current_snapshot", current_snapshot)
                .finish(),
            Self::OpenApplyPreparation { current_snapshot } => f
                .debug_struct("OpenApplyPreparation")
                .field("current_snapshot", current_snapshot)
                .finish(),
            Self::ApplyPrepareBackup { current_snapshot } => f
                .debug_struct("ApplyPrepareBackup")
                .field("current_snapshot", current_snapshot)
                .finish(),
            Self::ApplyTakePreparedCandidate { current_snapshot } => f
                .debug_struct("ApplyTakePreparedCandidate")
                .field("current_snapshot", current_snapshot)
                .finish(),
            Self::ApplyCancelPreview => write!(f, "ApplyCancelPreview"),
        }
    }
}

impl PartialEq for EditorAction {
    fn eq(&self, other: &Self) -> bool {
        format!("{self:?}") == format!("{other:?}")
    }
}
