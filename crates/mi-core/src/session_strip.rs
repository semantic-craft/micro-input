use crate::sidebar::CanonicalAgent;
use mi_bridge::{FourAppAgentPhase, FourAppSlotSnapshot};
use mi_model::{
    navigation_history::NavigationPlace,
    slot_table::{RecentCandidate, SlotMode, SlotStatus, SlotTable, SLOT_COUNT},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum FourAppSessionKind {
    NeedsAttention = 0,
    BlueDotCompleted = 1,
    Working = 2,
    Error = 3,
    Idle = 4,
}

impl FourAppSessionKind {
    pub const fn capsule_hex(self) -> &'static str {
        match self {
            Self::NeedsAttention => "#FFAA00",
            Self::BlueDotCompleted => "#0088FF",
            Self::Working => "#00FF66",
            Self::Error => "#FF2200",
            Self::Idle => "#0088FF",
        }
    }

    pub const fn badge_prefix(self) -> &'static str {
        match self {
            Self::NeedsAttention => "!",
            Self::BlueDotCompleted => "●",
            Self::Working => "::",
            Self::Error => "×",
            Self::Idle => "·",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FourAppLiveCandidate {
    pub place: NavigationPlace,
    pub title: String,
    pub phase: FourAppSessionKind,
    pub watermark: String,
    pub last_modified_ms: u64,
    pub last_user_view_ms: u64,
    pub updated_at_ms: u64,
}

impl FourAppLiveCandidate {
    pub fn has_unviewed_completion_timestamp(&self) -> bool {
        self.phase == FourAppSessionKind::BlueDotCompleted
            && self.last_modified_ms > self.last_user_view_ms.saturating_add(500)
    }

    pub fn is_unread_blue_dot(&self, acknowledged_watermarks: &BTreeMap<String, String>) -> bool {
        if !self.has_unviewed_completion_timestamp() {
            return false;
        }
        let target_id = self.place.target_id();
        !matches!(
            acknowledged_watermarks.get(&target_id),
            Some(ack) if !self.watermark.is_empty() && ack == &self.watermark
        )
    }

    pub fn effective_kind(
        &self,
        acknowledged_watermarks: &BTreeMap<String, String>,
    ) -> FourAppSessionKind {
        match self.phase {
            FourAppSessionKind::BlueDotCompleted => {
                if self.is_unread_blue_dot(acknowledged_watermarks) {
                    FourAppSessionKind::BlueDotCompleted
                } else {
                    FourAppSessionKind::Idle
                }
            }
            other => other,
        }
    }

    pub fn effective_slot_status(
        &self,
        acknowledged_watermarks: &BTreeMap<String, String>,
    ) -> SlotStatus {
        match self.effective_kind(acknowledged_watermarks) {
            FourAppSessionKind::NeedsAttention => SlotStatus::NeedsInteraction,
            FourAppSessionKind::BlueDotCompleted => SlotStatus::Unread,
            FourAppSessionKind::Working => SlotStatus::Running,
            FourAppSessionKind::Error => SlotStatus::Error,
            FourAppSessionKind::Idle => SlotStatus::Idle,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FourAppSlotTable {
    inner: SlotTable,
    candidates: Vec<FourAppLiveCandidate>,
}

impl FourAppSlotTable {
    pub fn new() -> Self {
        Self {
            inner: SlotTable::new(),
            candidates: Vec::new(),
        }
    }

    pub fn slot_table(&self) -> &SlotTable {
        &self.inner
    }

    pub fn candidates(&self) -> &[FourAppLiveCandidate] {
        &self.candidates
    }

    pub fn find_candidate(&self, place: &NavigationPlace) -> Option<&FourAppLiveCandidate> {
        let target_id = place.target_id();
        self.candidates
            .iter()
            .find(|c| c.place.target_id() == target_id)
    }

    pub fn pin_slot(
        &mut self,
        slot_index: usize,
        place: NavigationPlace,
        title: impl Into<String>,
        timestamp_ms: u64,
        acknowledged_watermarks: &BTreeMap<String, String>,
    ) -> Result<(), String> {
        self.inner
            .pin(slot_index, place, title, timestamp_ms)
            .map_err(|e| format!("{e:?}"))?;
        self.refresh_with_watermarks(acknowledged_watermarks);
        Ok(())
    }

    pub fn unpin_slot(
        &mut self,
        slot_index: usize,
        acknowledged_watermarks: &BTreeMap<String, String>,
    ) -> Result<(), String> {
        self.inner.unpin(slot_index).map_err(|e| format!("{e:?}"))?;
        self.refresh_with_watermarks(acknowledged_watermarks);
        Ok(())
    }

    pub fn update_candidates(
        &mut self,
        candidates: Vec<FourAppLiveCandidate>,
        acknowledged_watermarks: &BTreeMap<String, String>,
    ) {
        self.candidates = candidates;
        self.refresh_with_watermarks(acknowledged_watermarks);
    }

    pub fn refresh_with_watermarks(&mut self, acknowledged_watermarks: &BTreeMap<String, String>) {
        let mut sorted = self.candidates.clone();
        sorted.sort_by(|a, b| {
            let a_prio = !matches!(
                a.effective_kind(acknowledged_watermarks),
                FourAppSessionKind::Idle
            );
            let b_prio = !matches!(
                b.effective_kind(acknowledged_watermarks),
                FourAppSessionKind::Idle
            );
            b_prio
                .cmp(&a_prio)
                .then_with(|| b.updated_at_ms.cmp(&a.updated_at_ms))
        });
        self.candidates = sorted;

        let recent_candidates: Vec<RecentCandidate> = self
            .candidates
            .iter()
            .map(|cand| RecentCandidate {
                place: cand.place.clone(),
                title: cand.title.clone(),
                status: cand.effective_slot_status(acknowledged_watermarks),
                updated_at: cand.updated_at_ms,
            })
            .collect();

        self.inner.sync_recent(&recent_candidates);

        let status_updates: Vec<(usize, SlotStatus)> = self
            .inner
            .slots()
            .iter()
            .enumerate()
            .filter_map(|(idx, s)| {
                let place = s.place.as_ref()?;
                let cand = self.find_candidate(place)?;
                Some((idx, cand.effective_slot_status(acknowledged_watermarks)))
            })
            .collect();
        for (idx, status) in status_updates {
            self.inner.slots_mut()[idx].status = status;
        }
    }

    pub fn lighting_snapshots(
        &self,
        acknowledged_watermarks: &BTreeMap<String, String>,
    ) -> [FourAppSlotSnapshot; SLOT_COUNT] {
        std::array::from_fn(|idx| {
            let slot = &self.inner.slots()[idx];
            if let Some(place) = &slot.place {
                let app_key = agent_for_place(place).bundle_id().to_string();
                if let Some(cand) = self.find_candidate(place) {
                    let status_str = match cand.effective_kind(acknowledged_watermarks) {
                        FourAppSessionKind::NeedsAttention => "waiting",
                        FourAppSessionKind::BlueDotCompleted => "completed",
                        FourAppSessionKind::Working => "running",
                        FourAppSessionKind::Error => "error",
                        FourAppSessionKind::Idle => "idle",
                    };
                    return FourAppSlotSnapshot {
                        app: app_key,
                        title: cand.title.clone(),
                        status: status_str.to_string(),
                        place: Some(place.clone()),
                        watermark: cand.watermark.clone(),
                    };
                }
                return FourAppSlotSnapshot {
                    app: app_key,
                    title: slot.title.clone(),
                    status: "idle".to_string(),
                    place: Some(place.clone()),
                    watermark: String::new(),
                };
            }
            FourAppSlotSnapshot {
                app: String::new(),
                title: String::new(),
                status: "idle".to_string(),
                place: None,
                watermark: String::new(),
            }
        })
    }
}

pub fn agent_for_place(place: &NavigationPlace) -> CanonicalAgent {
    match place {
        NavigationPlace::Antigravity { .. } => CanonicalAgent::Antigravity,
        NavigationPlace::Dsh { .. } => CanonicalAgent::DeepseekHarness,
        NavigationPlace::App { bundle_id } => {
            CanonicalAgent::from_bundle_id(bundle_id).unwrap_or(CanonicalAgent::Antigravity)
        }
        NavigationPlace::Session { adapter, .. } => match adapter.as_str() {
            "dsh" | "dsh-desktop" => CanonicalAgent::DeepseekHarness,
            "codex" => CanonicalAgent::Codex,
            "herdr" => CanonicalAgent::Herdr,
            _ => CanonicalAgent::Antigravity,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AmbientColorPreset {
    PureWhite,
    EmeraldGreen,
    OceanBlue,
    CyanTeal,
    AmberGold,
}

impl AmbientColorPreset {
    pub const ALL: [Self; 5] = [
        Self::PureWhite,
        Self::EmeraldGreen,
        Self::OceanBlue,
        Self::CyanTeal,
        Self::AmberGold,
    ];

    pub const fn hex(self) -> &'static str {
        match self {
            Self::PureWhite => "#FFFFFF",
            Self::EmeraldGreen => "#00FF66",
            Self::OceanBlue => "#0088FF",
            Self::CyanTeal => "#00CCAA",
            Self::AmberGold => "#FFAA00",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::PureWhite => "纯白",
            Self::EmeraldGreen => "翡翠绿",
            Self::OceanBlue => "海洋蓝",
            Self::CyanTeal => "青碧",
            Self::AmberGold => "琥珀金",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LightingBrightnessPreset {
    Soft,
    Standard,
    Bright,
}

impl LightingBrightnessPreset {
    pub const ALL: [Self; 3] = [Self::Soft, Self::Standard, Self::Bright];

    pub const fn scale(self) -> f64 {
        match self {
            Self::Soft => 0.35,
            Self::Standard => 0.65,
            Self::Bright => 0.95,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Soft => "柔和 (35%)",
            Self::Standard => "标准 (65%)",
            Self::Bright => "明亮 (95%)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LightingTimeoutPreset {
    OneMinute,
    ThreeMinutes,
    FiveMinutes,
    Never,
}

impl LightingTimeoutPreset {
    pub const ALL: [Self; 4] = [
        Self::OneMinute,
        Self::ThreeMinutes,
        Self::FiveMinutes,
        Self::Never,
    ];

    pub const fn seconds(self) -> Option<u64> {
        match self {
            Self::OneMinute => Some(60),
            Self::ThreeMinutes => Some(180),
            Self::FiveMinutes => Some(300),
            Self::Never => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::OneMinute => "1 分钟",
            Self::ThreeMinutes => "3 分钟",
            Self::FiveMinutes => "5 分钟",
            Self::Never => "常亮",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimpleLightingPopoverProjection {
    pub is_open: bool,
    pub lighting_enabled: bool,
    pub auto_session_color: bool,
    pub only_glow_on_attention_or_completed: bool,
    pub glow_on_working: bool,
    pub ambient_color: AmbientColorPreset,
    pub working_color: AmbientColorPreset,
    pub brightness: LightingBrightnessPreset,
    pub auto_off_timeout: LightingTimeoutPreset,
    pub manual_preview_phase: Option<FourAppAgentPhase>,
    pub suspended_by_protected_layer: bool,
}

impl Default for SimpleLightingPopoverProjection {
    fn default() -> Self {
        Self {
            is_open: false,
            lighting_enabled: true,
            auto_session_color: true,
            only_glow_on_attention_or_completed: true,
            glow_on_working: true,
            ambient_color: AmbientColorPreset::OceanBlue,
            working_color: AmbientColorPreset::EmeraldGreen,
            brightness: LightingBrightnessPreset::Standard,
            auto_off_timeout: LightingTimeoutPreset::ThreeMinutes,
            manual_preview_phase: None,
            suspended_by_protected_layer: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionCapsulePillProjection {
    pub place: NavigationPlace,
    pub agent: CanonicalAgent,
    pub agent_monogram: String,
    pub title: String,
    pub kind: FourAppSessionKind,
    pub badge_text: String,
    pub color_hex: String,
    pub watermark: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSlotCardProjection {
    pub slot_index: usize,
    pub key_binding_id: String,
    pub mode: SlotMode,
    pub status: SlotStatus,
    pub kind: FourAppSessionKind,
    pub agent: Option<CanonicalAgent>,
    pub agent_monogram: String,
    pub title: String,
    pub subtitle: String,
    pub place: Option<NavigationPlace>,
    pub watermark: String,
    pub is_unread_blue_dot: bool,
    pub is_working_animated: bool,
    pub is_bound_to_top_key: bool,
    pub led_color_hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionStripProjection {
    pub slots: [SessionSlotCardProjection; SLOT_COUNT],
    pub completed_blue_dot_pills: Vec<SessionCapsulePillProjection>,
    pub working_pills: Vec<SessionCapsulePillProjection>,
    pub attention_pills: Vec<SessionCapsulePillProjection>,
    pub unread_completed_count: usize,
    pub working_count: usize,
    pub top_six_bound_as_session_keys: bool,
    pub can_mutate_layout: bool,
    pub lighting_popover: SimpleLightingPopoverProjection,
}

impl Default for SessionStripProjection {
    fn default() -> Self {
        let binding_ids = ["AG00", "AG01", "AG02", "AG03", "AG04", "AG05"];
        Self {
            slots: std::array::from_fn(|idx| SessionSlotCardProjection {
                slot_index: idx,
                key_binding_id: binding_ids[idx].to_string(),
                mode: SlotMode::Recent,
                status: SlotStatus::Idle,
                kind: FourAppSessionKind::Idle,
                agent: None,
                agent_monogram: String::new(),
                title: String::new(),
                subtitle: String::new(),
                place: None,
                watermark: String::new(),
                is_unread_blue_dot: false,
                is_working_animated: false,
                is_bound_to_top_key: false,
                led_color_hex: "#222222".to_string(),
            }),
            completed_blue_dot_pills: Vec::new(),
            working_pills: Vec::new(),
            attention_pills: Vec::new(),
            unread_completed_count: 0,
            working_count: 0,
            top_six_bound_as_session_keys: false,
            can_mutate_layout: true,
            lighting_popover: SimpleLightingPopoverProjection::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionStripAction {
    ActivateSlot(usize),
    TogglePinSlot(usize),
    JumpToSession(NavigationPlace),
    AcknowledgeAllCompleted,
    BindTopSixAsSessionKeys,
    RestoreDefaultLayout,
    ToggleLightingPopover,
    SetLightingEnabled(bool),
    SetAutoSessionColor(bool),
    SetOnlyGlowOnAttentionOrCompleted(bool),
    SetGlowOnWorking(bool),
    SetAmbientColor(AmbientColorPreset),
    SetWorkingColor(AmbientColorPreset),
    SetBrightnessPreset(LightingBrightnessPreset),
    SetAutoOffTimeout(LightingTimeoutPreset),
    TriggerTestLighting(Option<FourAppAgentPhase>),
}
