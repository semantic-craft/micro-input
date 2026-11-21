use mi_model::navigation_history::NavigationPlace;
use mi_proto::lighting_candidate::OaiKeyStatusEntry;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn parse_hex_rgb(raw: &str) -> Option<u32> {
    let hex = raw.trim().strip_prefix('#').unwrap_or(raw.trim());
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(hex, 16).ok()
}

const SIX_LAYER_SESSION_KEYS: [&str; 18] = [
    "AG00", "AG01", "AG02", "AG03", "AG04", "AG05", "AG06", "AG07", "AG08", "AG09", "AG10", "AG11",
    "AG12", "AG13", "AG14", "AG15", "AG16", "AG17",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FourAppAgentPhase {
    Idle,
    Thinking,
    AwaitingInput,
    Completed,
    Error,
    BackgroundActive,
}

impl FourAppAgentPhase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "空闲/就绪",
            Self::Thinking => "生成中/思考中",
            Self::AwaitingInput => "待输入/待审批",
            Self::Completed => "会话已完成",
            Self::Error => "异常/失败",
            Self::BackgroundActive => "后台任务运行中",
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Self::Idle => "checkmark.circle",
            Self::Thinking => "ellipsis.circle",
            Self::AwaitingInput => "exclamationmark.circle",
            Self::Completed => "sparkles",
            Self::Error => "xmark.octagon",
            Self::BackgroundActive => "arrow.triangle.2.circlepath",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DualZoneLightingFeedback {
    pub backlight_color: String,
    pub backlight_effect: String,
    pub backlight_brightness: f64,
    pub backlight_speed: f64,
    pub underglow_color: String,
    pub underglow_effect: String,
    pub underglow_brightness: f64,
    pub underglow_speed: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerKeyLightingFeedback {
    pub binding_id: String,
    pub color: String,
    pub phase: FourAppAgentPhase,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FourAppSlotSnapshot {
    pub app: String,
    pub title: String,
    pub status: String,
    pub place: Option<NavigationPlace>,
    pub watermark: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LightingUpdateResult {
    Applied,
    SuspendedByProtectedLayer,
    DeviceBusy,
    Released,
    NoDevice,
}

pub struct FourAppLightingArbiter {
    pub active_phase: FourAppAgentPhase,
    pub dual_zone_feedback: Option<DualZoneLightingFeedback>,
    pub per_key_feedback: Vec<PerKeyLightingFeedback>,
    pub full_keypad_feedback: Vec<PerKeyLightingFeedback>,
    pub last_update_result: LightingUpdateResult,
    pub lighting_enabled: bool,
    pub session_lighting_linked: bool,
    pub only_glow_on_attention_or_completed: bool,
    pub glow_on_working_sessions: bool,
    pub brightness_scale: f64,
    pub idle_color_hex: String,
    pub working_color_hex: String,
    pub manual_preview_phase: Option<FourAppAgentPhase>,
    pub acknowledged_watermarks: BTreeMap<String, String>,
    pub hardware_synced: bool,
    pub last_hardware_sync_json: Option<String>,
    pub last_synced_payload_digest: Option<String>,
    pub last_synced_device_id: Option<String>,
    pub hardware_write_count: usize,
    last_recomputed_slots: Vec<FourAppSlotSnapshot>,
    last_six_layer_entries: Option<Vec<OaiKeyStatusEntry>>,
}

impl Default for FourAppLightingArbiter {
    fn default() -> Self {
        Self::new()
    }
}

impl FourAppLightingArbiter {
    pub fn new() -> Self {
        Self {
            active_phase: FourAppAgentPhase::Idle,
            dual_zone_feedback: None,
            per_key_feedback: Vec::new(),
            full_keypad_feedback: Vec::new(),
            last_update_result: LightingUpdateResult::Released,
            lighting_enabled: true,
            session_lighting_linked: true,
            only_glow_on_attention_or_completed: true,
            glow_on_working_sessions: false,
            brightness_scale: 0.85,
            idle_color_hex: "#0088FF".into(),
            working_color_hex: "#00FF66".into(),
            manual_preview_phase: None,
            acknowledged_watermarks: BTreeMap::new(),
            hardware_synced: false,
            last_hardware_sync_json: None,
            last_synced_payload_digest: None,
            last_synced_device_id: None,
            hardware_write_count: 0,
            last_recomputed_slots: Vec::new(),
            last_six_layer_entries: None,
        }
    }

    pub fn is_slot_attention_needed(&self, slot: &FourAppSlotSnapshot) -> bool {
        let Some(place) = &slot.place else {
            return false;
        };
        if matches!(
            slot.status.as_str(),
            "waiting" | "waiting_approval" | "needs-interaction" | "needs_interaction"
        ) {
            return true;
        }
        if matches!(slot.status.as_str(), "completed" | "error" | "failed") {
            let key = place.target_id();
            return slot.watermark.is_empty()
                || self.acknowledged_watermarks.get(&key) != Some(&slot.watermark);
        }
        false
    }

    pub fn is_slot_working(&self, slot: &FourAppSlotSnapshot) -> bool {
        slot.place.is_some()
            && matches!(
                slot.status.as_str(),
                "running"
                    | "active"
                    | "runningTool"
                    | "generating"
                    | "busy"
                    | "background_active"
                    | "backgroundActive"
            )
    }

    fn should_light_slot(&self, slot: &FourAppSlotSnapshot) -> bool {
        if slot.place.is_none() {
            return false;
        }
        if self.is_slot_working(slot) {
            self.glow_on_working_sessions
        } else if self.only_glow_on_attention_or_completed {
            self.is_slot_attention_needed(slot)
        } else {
            true
        }
    }

    pub fn acknowledge_slot(&mut self, place: &NavigationPlace, watermark: impl Into<String>) {
        self.acknowledged_watermarks
            .insert(place.target_id(), watermark.into());
    }

    pub fn derive_dual_zone_feedback(
        &self,
        phase: FourAppAgentPhase,
        dictation_active: bool,
    ) -> DualZoneLightingFeedback {
        let idle_hex = self.idle_color_hex.as_str();
        let working_hex = self.working_color_hex.as_str();
        let (bl_color, bl_effect, bl_brightness, bl_speed) = match phase {
            FourAppAgentPhase::Idle => (idle_hex, "solid", 0.72, 0.5),
            FourAppAgentPhase::Thinking => (working_hex, "breath", 0.85, 0.7),
            FourAppAgentPhase::AwaitingInput => ("#FFAA00", "breath", 1.0, 0.8),
            FourAppAgentPhase::Completed => (idle_hex, "solid", 0.90, 0.5),
            FourAppAgentPhase::Error => ("#FF2200", "breath", 1.0, 0.9),
            FourAppAgentPhase::BackgroundActive => ("#AA44FF", "breath", 0.7, 0.5),
        };
        let (ug_color, ug_effect, ug_brightness, ug_speed) = if dictation_active {
            ("#8800FF", "solid", 1.0, 0.5)
        } else if phase == FourAppAgentPhase::Completed {
            (idle_hex, "solid", 0.90, 0.5)
        } else if phase == FourAppAgentPhase::Thinking {
            (working_hex, "breath", 0.85, 0.65)
        } else if phase == FourAppAgentPhase::AwaitingInput {
            ("#FFAA00", "breath", 0.95, 0.75)
        } else if phase == FourAppAgentPhase::Error {
            ("#FF2200", "breath", 1.0, 0.85)
        } else {
            (idle_hex, "solid", 0.78, 0.5)
        };
        DualZoneLightingFeedback {
            backlight_color: bl_color.into(),
            backlight_effect: bl_effect.into(),
            backlight_brightness: bl_brightness,
            backlight_speed: bl_speed,
            underglow_color: ug_color.into(),
            underglow_effect: ug_effect.into(),
            underglow_brightness: ug_brightness,
            underglow_speed: ug_speed,
        }
    }

    pub fn derive_per_key_feedback(
        &self,
        slots: &[FourAppSlotSnapshot],
        dictation_active: bool,
    ) -> Vec<PerKeyLightingFeedback> {
        let mut result = Vec::with_capacity(8);
        let key_ids = ["AG00", "AG01", "AG02", "AG03", "AG04", "AG05"];
        let idle_hex = self.idle_color_hex.as_str();
        let working_hex = self.working_color_hex.as_str();
        for (i, key_id) in key_ids.iter().enumerate() {
            if let Some(slot) = slots.get(i).filter(|s| s.place.is_some()) {
                let (phase, color) = if !self.session_lighting_linked {
                    (FourAppAgentPhase::Idle, idle_hex)
                } else {
                    match slot.status.as_str() {
                        "running" | "runningTool" | "generating" | "busy" | "active" => {
                            (FourAppAgentPhase::Thinking, working_hex)
                        }
                        "waiting" | "waiting_approval" | "needs-interaction"
                        | "needs_interaction" => (FourAppAgentPhase::AwaitingInput, "#FFAA00"),
                        "error" | "failed" if self.is_slot_attention_needed(slot) => {
                            (FourAppAgentPhase::Error, "#FF2200")
                        }
                        "background_active" | "backgroundActive" => {
                            (FourAppAgentPhase::BackgroundActive, "#AA44FF")
                        }
                        "completed" if self.is_slot_attention_needed(slot) => {
                            (FourAppAgentPhase::Completed, idle_hex)
                        }
                        _ => (FourAppAgentPhase::Idle, idle_hex),
                    }
                };
                let label = if slot.title.is_empty() {
                    format!("{} slot {i}", slot.app)
                } else {
                    slot.title.clone()
                };
                result.push(PerKeyLightingFeedback {
                    binding_id: (*key_id).into(),
                    color: color.into(),
                    phase,
                    label,
                });
            } else {
                result.push(PerKeyLightingFeedback {
                    binding_id: (*key_id).into(),
                    color: "#222222".into(),
                    phase: FourAppAgentPhase::Idle,
                    label: format!("空槽位 {i}"),
                });
            }
        }
        let wide_color = if dictation_active {
            "#FF00FF"
        } else {
            "#00CCAA"
        };
        result.push(PerKeyLightingFeedback {
            binding_id: "AG10".into(),
            color: wide_color.into(),
            phase: if dictation_active {
                FourAppAgentPhase::Thinking
            } else {
                FourAppAgentPhase::Idle
            },
            label: if dictation_active {
                "听写中".into()
            } else {
                "听写就绪".into()
            },
        });
        result
    }

    pub fn derive_full_keypad_feedback(
        &self,
        slots: &[FourAppSlotSnapshot],
        dictation_active: bool,
        active_phase: FourAppAgentPhase,
        has_attention: bool,
        is_bottom_row_split: bool,
    ) -> Vec<PerKeyLightingFeedback> {
        let base = self.derive_per_key_feedback(slots, dictation_active);
        let mut entries: Vec<PerKeyLightingFeedback> = base.into_iter().take(6).collect();
        entries.push(PerKeyLightingFeedback {
            binding_id: "AG06".into(),
            color: if has_attention { "#FFB800" } else { "#334455" }.into(),
            phase: if has_attention {
                FourAppAgentPhase::AwaitingInput
            } else {
                FourAppAgentPhase::Idle
            },
            label: "全局待办跳转".into(),
        });
        let approval_ready = active_phase == FourAppAgentPhase::AwaitingInput || has_attention;
        entries.push(PerKeyLightingFeedback {
            binding_id: "AG07".into(),
            color: if approval_ready { "#00FF66" } else { "#224433" }.into(),
            phase: if approval_ready {
                FourAppAgentPhase::AwaitingInput
            } else {
                FourAppAgentPhase::Idle
            },
            label: "审批通过".into(),
        });
        entries.push(PerKeyLightingFeedback {
            binding_id: "AG08".into(),
            color: if approval_ready { "#FF3B30" } else { "#442222" }.into(),
            phase: if approval_ready {
                FourAppAgentPhase::AwaitingInput
            } else {
                FourAppAgentPhase::Idle
            },
            label: "审批拒绝".into(),
        });
        let running = active_phase == FourAppAgentPhase::Thinking;
        entries.push(PerKeyLightingFeedback {
            binding_id: "AG09".into(),
            color: if running { "#FF3B30" } else { "#00AAFF" }.into(),
            phase: if running {
                FourAppAgentPhase::Thinking
            } else {
                FourAppAgentPhase::Idle
            },
            label: if running {
                "停止生成"
            } else {
                "新建会话"
            }
            .into(),
        });
        let wide_color = if dictation_active {
            "#FF00FF"
        } else {
            "#00CCAA"
        };
        if is_bottom_row_split {
            entries.push(PerKeyLightingFeedback {
                binding_id: "AG10".into(),
                color: "#00AAFF".into(),
                phase: FourAppAgentPhase::Idle,
                label: "聚焦输入框".into(),
            });
            entries.push(PerKeyLightingFeedback {
                binding_id: "AG11".into(),
                color: wide_color.into(),
                phase: if dictation_active {
                    FourAppAgentPhase::Thinking
                } else {
                    FourAppAgentPhase::Idle
                },
                label: if dictation_active {
                    "听写中"
                } else {
                    "按住说话"
                }
                .into(),
            });
        } else {
            entries.push(PerKeyLightingFeedback {
                binding_id: "AG10".into(),
                color: wide_color.into(),
                phase: if dictation_active {
                    FourAppAgentPhase::Thinking
                } else {
                    FourAppAgentPhase::Idle
                },
                label: if dictation_active {
                    "听写中 (2U 左)"
                } else {
                    "按住说话 (2U 左)"
                }
                .into(),
            });
            entries.push(PerKeyLightingFeedback {
                binding_id: "AG11".into(),
                color: wide_color.into(),
                phase: if dictation_active {
                    FourAppAgentPhase::Thinking
                } else {
                    FourAppAgentPhase::Idle
                },
                label: if dictation_active {
                    "听写中 (2U 右)"
                } else {
                    "按住说话 (2U 右)"
                }
                .into(),
            });
        }
        entries.push(PerKeyLightingFeedback {
            binding_id: "AG12".into(),
            color: "#0066FF".into(),
            phase: active_phase,
            label: "停止听写并提交".into(),
        });
        entries
    }

    pub fn recompute_feedback(
        &mut self,
        live_phase: FourAppAgentPhase,
        slots: &[FourAppSlotSnapshot],
        dictation_active: bool,
        is_protected_layer: bool,
        preview_busy: bool,
        is_bottom_row_split: bool,
    ) -> LightingUpdateResult {
        let phase = if let Some(manual) = self.manual_preview_phase {
            manual
        } else if !self.session_lighting_linked {
            FourAppAgentPhase::Idle
        } else {
            live_phase
        };
        self.active_phase = phase;

        if is_protected_layer {
            self.release();
            self.last_update_result = LightingUpdateResult::SuspendedByProtectedLayer;
            return LightingUpdateResult::SuspendedByProtectedLayer;
        }
        if preview_busy {
            self.last_update_result = LightingUpdateResult::DeviceBusy;
            return LightingUpdateResult::DeviceBusy;
        }

        let clamped_scale = self.brightness_scale.clamp(0.15, 1.0);
        self.last_recomputed_slots = slots.to_vec();
        let attention_slots: Vec<&FourAppSlotSnapshot> = slots
            .iter()
            .filter(|s| self.is_slot_attention_needed(s))
            .collect();
        let working_slots: Vec<&FourAppSlotSnapshot> =
            slots.iter().filter(|s| self.is_slot_working(s)).collect();
        let has_awaiting_slot = slots.iter().any(|s| {
            s.place.is_some()
                && matches!(
                    s.status.as_str(),
                    "waiting" | "waiting_approval" | "needs-interaction" | "needs_interaction"
                )
        });
        let has_error_slot = slots.iter().any(|s| {
            matches!(s.status.as_str(), "error" | "failed") && self.is_slot_attention_needed(s)
        });
        let has_any_attention = !attention_slots.is_empty();

        let mut dual_zone = self.derive_dual_zone_feedback(phase, dictation_active);
        if !self.lighting_enabled {
            dual_zone = DualZoneLightingFeedback {
                backlight_color: "#000000".into(),
                backlight_effect: "off".into(),
                backlight_brightness: 0.0,
                backlight_speed: 0.0,
                underglow_color: "#000000".into(),
                underglow_effect: "off".into(),
                underglow_brightness: 0.0,
                underglow_speed: 0.0,
            };
        } else if self.only_glow_on_attention_or_completed && self.manual_preview_phase.is_none() {
            dual_zone.backlight_color = "#000000".into();
            dual_zone.backlight_effect = "off".into();
            dual_zone.backlight_brightness = 0.0;
            dual_zone.backlight_speed = 0.0;
            dual_zone.underglow_color = "#000000".into();
            dual_zone.underglow_effect = "off".into();
            dual_zone.underglow_brightness = 0.0;
            dual_zone.underglow_speed = 0.0;
            if dictation_active {
                dual_zone.underglow_color = "#8800FF".into();
                dual_zone.underglow_effect = "solid".into();
                dual_zone.underglow_brightness = (1.0 * clamped_scale).min(1.0);
                dual_zone.underglow_speed = 0.5;
            } else if phase == FourAppAgentPhase::AwaitingInput || has_awaiting_slot {
                let color = if self.session_lighting_linked {
                    "#FFAA00".into()
                } else {
                    self.idle_color_hex.clone()
                };
                let effect = if self.session_lighting_linked {
                    "breath"
                } else {
                    "solid"
                };
                dual_zone.backlight_color = color.clone();
                dual_zone.backlight_effect = effect.into();
                dual_zone.backlight_brightness = (0.85 * clamped_scale).min(1.0);
                dual_zone.backlight_speed = if self.session_lighting_linked {
                    0.75
                } else {
                    0.5
                };
                dual_zone.underglow_color = color;
                dual_zone.underglow_effect = effect.into();
                dual_zone.underglow_brightness = (0.95 * clamped_scale).min(1.0);
                dual_zone.underglow_speed = if self.session_lighting_linked {
                    0.75
                } else {
                    0.5
                };
            } else if phase == FourAppAgentPhase::Error || has_error_slot {
                let color = if self.session_lighting_linked {
                    "#FF2200".into()
                } else {
                    self.idle_color_hex.clone()
                };
                let effect = if self.session_lighting_linked {
                    "breath"
                } else {
                    "solid"
                };
                dual_zone.backlight_color = color.clone();
                dual_zone.backlight_effect = effect.into();
                dual_zone.backlight_brightness = (0.90 * clamped_scale).min(1.0);
                dual_zone.backlight_speed = if self.session_lighting_linked {
                    0.85
                } else {
                    0.5
                };
                dual_zone.underglow_color = color;
                dual_zone.underglow_effect = effect.into();
                dual_zone.underglow_brightness = (1.0 * clamped_scale).min(1.0);
                dual_zone.underglow_speed = if self.session_lighting_linked {
                    0.85
                } else {
                    0.5
                };
            } else if self.glow_on_working_sessions
                && (phase == FourAppAgentPhase::Thinking || !working_slots.is_empty())
            {
                dual_zone.backlight_color = if self.session_lighting_linked {
                    self.working_color_hex.clone()
                } else {
                    self.idle_color_hex.clone()
                };
                dual_zone.backlight_effect = if self.session_lighting_linked {
                    "breath".into()
                } else {
                    "solid".into()
                };
                dual_zone.backlight_brightness = (0.80 * clamped_scale).min(1.0);
                dual_zone.backlight_speed = if self.session_lighting_linked {
                    0.65
                } else {
                    0.5
                };
            }
        } else {
            if phase == FourAppAgentPhase::Idle && !dictation_active {
                dual_zone.backlight_color = self.idle_color_hex.clone();
                dual_zone.underglow_color = self.idle_color_hex.clone();
                dual_zone.backlight_brightness = 0.72;
                dual_zone.underglow_brightness = 0.78;
            }
            dual_zone.backlight_brightness =
                (dual_zone.backlight_brightness * clamped_scale).clamp(0.0, 1.0);
            dual_zone.underglow_brightness =
                (dual_zone.underglow_brightness * clamped_scale).clamp(0.0, 1.0);
        }

        self.per_key_feedback = self.derive_per_key_feedback(slots, dictation_active);
        let mut full_keys = self.derive_full_keypad_feedback(
            slots,
            dictation_active,
            phase,
            has_any_attention,
            is_bottom_row_split,
        );
        if self.manual_preview_phase.is_none() {
            let slot_ids = ["AG00", "AG01", "AG02", "AG03", "AG04", "AG05"];
            if self.only_glow_on_attention_or_completed {
                full_keys = full_keys
                    .into_iter()
                    .map(|item| {
                        if let Some(idx) = slot_ids.iter().position(|k| *k == item.binding_id) {
                            if slots.get(idx).is_some_and(|s| self.should_light_slot(s)) {
                                return item;
                            }
                            return PerKeyLightingFeedback {
                                binding_id: item.binding_id,
                                color: "#000000".into(),
                                phase: FourAppAgentPhase::Idle,
                                label: item.label,
                            };
                        }
                        if item.binding_id == "AG06" && has_any_attention {
                            let attn_color = if !self.session_lighting_linked {
                                self.idle_color_hex.as_str()
                            } else if phase == FourAppAgentPhase::AwaitingInput || has_awaiting_slot
                            {
                                "#FFAA00"
                            } else if phase == FourAppAgentPhase::Error || has_error_slot {
                                "#FF2200"
                            } else {
                                self.idle_color_hex.as_str()
                            };
                            return PerKeyLightingFeedback {
                                binding_id: item.binding_id,
                                color: attn_color.into(),
                                phase,
                                label: item.label,
                            };
                        }
                        if (item.binding_id == "AG10" || item.binding_id == "AG11")
                            && dictation_active
                        {
                            return item;
                        }
                        PerKeyLightingFeedback {
                            binding_id: item.binding_id,
                            color: "#000000".into(),
                            phase: FourAppAgentPhase::Idle,
                            label: item.label,
                        }
                    })
                    .collect();
            } else {
                full_keys = full_keys
                    .into_iter()
                    .map(|item| {
                        if let Some(idx) = slot_ids.iter().position(|k| *k == item.binding_id) {
                            if slots.get(idx).is_some_and(|s| !self.should_light_slot(s)) {
                                return PerKeyLightingFeedback {
                                    binding_id: item.binding_id,
                                    color: "#000000".into(),
                                    phase: FourAppAgentPhase::Idle,
                                    label: item.label,
                                };
                            }
                        }
                        item
                    })
                    .collect();
            }
        }

        if self.dual_zone_feedback.as_ref() == Some(&dual_zone)
            && self.full_keypad_feedback == full_keys
            && self.last_hardware_sync_json.is_some()
        {
            self.last_update_result = LightingUpdateResult::Applied;
            return LightingUpdateResult::Applied;
        }

        let per_key_json: Vec<serde_json::Value> = full_keys
            .iter()
            .map(|item| {
                let hex = if !self.lighting_enabled {
                    "#000000"
                } else {
                    item.color.as_str()
                };
                let is_lit = self.lighting_enabled && hex != "#000000" && hex != "#222222";
                let key_brightness = if is_lit {
                    (0.85 * clamped_scale).max(0.35)
                } else {
                    0.0
                };
                let (key_effect, key_speed) = if !is_lit {
                    (0, 0.65)
                } else {
                    Self::phase_effect_and_speed(item.phase)
                };
                json!({
                    "k": item.binding_id,
                    "color": if is_lit { hex } else { "#000000" },
                    "brightness": key_brightness,
                    "e": key_effect,
                    "s": key_speed,
                })
            })
            .collect();

        let payload = json!({
            "backlight": {
                "effect": dual_zone.backlight_effect,
                "color": dual_zone.backlight_color,
                "brightness": dual_zone.backlight_brightness,
                "speed": dual_zone.backlight_speed,
                "magic": 0.5,
            },
            "underglow": {
                "effect": dual_zone.underglow_effect,
                "color": dual_zone.underglow_color,
                "brightness": dual_zone.underglow_brightness,
                "speed": dual_zone.underglow_speed,
                "magic": 0.5,
            },
            "per_key": per_key_json,
        });
        let payload_str = serde_json::to_string(&payload).unwrap_or_default();
        self.dual_zone_feedback = Some(dual_zone);
        self.full_keypad_feedback = full_keys;
        self.last_hardware_sync_json = Some(payload_str);
        self.last_update_result = LightingUpdateResult::Applied;
        LightingUpdateResult::Applied
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_feedback<F>(
        &mut self,
        live_phase: FourAppAgentPhase,
        slots: &[FourAppSlotSnapshot],
        dictation_active: bool,
        is_protected_layer: bool,
        preview_busy: bool,
        is_bottom_row_split: bool,
        connected_device_id: Option<&str>,
        mut write_hardware: F,
    ) -> LightingUpdateResult
    where
        F: FnMut(&str) -> Result<String, String>,
    {
        let result = self.recompute_feedback(
            live_phase,
            slots,
            dictation_active,
            is_protected_layer,
            preview_busy,
            is_bottom_row_split,
        );
        if result != LightingUpdateResult::Applied {
            return result;
        }
        if let Some(device_id) = connected_device_id {
            let payload_str = self.last_hardware_sync_json.as_deref().unwrap_or_default();
            let digest = format!("{:x}", Sha256::digest(payload_str.as_bytes()));
            let needs_sync = !self.hardware_synced
                || self.last_synced_payload_digest.as_deref() != Some(digest.as_str())
                || self.last_synced_device_id.as_deref() != Some(device_id);
            if needs_sync {
                match write_hardware(payload_str) {
                    Ok(resp) if resp.contains("\"synced\":true") => {
                        self.last_synced_payload_digest = Some(digest);
                        self.last_synced_device_id = Some(device_id.to_owned());
                        self.hardware_synced = true;
                        self.hardware_write_count += 1;
                    }
                    _ => {
                        self.hardware_synced = false;
                        self.last_synced_payload_digest = None;
                    }
                }
            }
        } else {
            self.last_synced_payload_digest = None;
            self.last_synced_device_id = None;
            self.hardware_synced = false;
        }

        LightingUpdateResult::Applied
    }

    fn six_layer_off_entries() -> Vec<OaiKeyStatusEntry> {
        SIX_LAYER_SESSION_KEYS
            .into_iter()
            .map(|k| OaiKeyStatusEntry::with_style(k, 0, 0.0, 0, 0.65))
            .collect()
    }

    const fn phase_effect_and_speed(phase: FourAppAgentPhase) -> (u8, f64) {
        match phase {
            FourAppAgentPhase::AwaitingInput => (4, 0.65),
            FourAppAgentPhase::Error => (4, 0.75),
            FourAppAgentPhase::Completed => (1, 0.50),
            FourAppAgentPhase::Thinking => (4, 0.65),
            FourAppAgentPhase::BackgroundActive => (4, 0.50),
            FourAppAgentPhase::Idle => (1, 0.50),
        }
    }

    fn six_layer_slot_entry(
        &self,
        key: &str,
        slot: Option<&FourAppSlotSnapshot>,
        lit_brightness: f64,
    ) -> (OaiKeyStatusEntry, Option<FourAppAgentPhase>) {
        let Some(slot) = slot.filter(|s| s.place.is_some()) else {
            return (OaiKeyStatusEntry::with_style(key, 0, 0.0, 0, 0.65), None);
        };
        let (phase, hex) = if !self.session_lighting_linked {
            (FourAppAgentPhase::Idle, self.idle_color_hex.as_str())
        } else {
            match slot.status.as_str() {
                "waiting" | "waiting_approval" | "needs-interaction" | "needs_interaction" => {
                    (FourAppAgentPhase::AwaitingInput, "#FFAA00")
                }
                "error" | "failed" if self.is_slot_attention_needed(slot) => {
                    (FourAppAgentPhase::Error, "#FF2200")
                }
                "completed" if self.is_slot_attention_needed(slot) => {
                    (FourAppAgentPhase::Completed, self.idle_color_hex.as_str())
                }
                "running" | "runningTool" | "generating" | "busy" | "active" => {
                    (FourAppAgentPhase::Thinking, self.working_color_hex.as_str())
                }
                "background_active" | "backgroundActive" => {
                    (FourAppAgentPhase::BackgroundActive, "#AA44FF")
                }
                _ => (FourAppAgentPhase::Idle, self.idle_color_hex.as_str()),
            }
        };
        if !self.should_light_slot(slot) {
            return (OaiKeyStatusEntry::with_style(key, 0, 0.0, 0, 0.65), None);
        }
        let color = parse_hex_rgb(hex).unwrap_or(0);
        if color == 0 {
            return (OaiKeyStatusEntry::with_style(key, 0, 0.0, 0, 0.65), None);
        }
        let (effect, speed) = Self::phase_effect_and_speed(phase);
        (
            OaiKeyStatusEntry::with_style(key, color, lit_brightness, effect, speed),
            Some(phase),
        )
    }

    pub fn six_layer_thstatus_entries_for_layers(
        &self,
        layer1_slots: &[FourAppSlotSnapshot],
        layer2_slots: &[FourAppSlotSnapshot],
        layer3_slots: &[FourAppSlotSnapshot],
        preferred_layer: u8,
    ) -> Vec<OaiKeyStatusEntry> {
        if !self.lighting_enabled
            || self.last_update_result == LightingUpdateResult::SuspendedByProtectedLayer
            || self.brightness_scale <= 0.0
        {
            return Self::six_layer_off_entries();
        }
        let clamped_scale = self.brightness_scale.clamp(0.15, 1.0);
        let lit_brightness = (0.85 * clamped_scale).max(0.35);
        if let Some(preview_phase) = self.manual_preview_phase {
            let hex = match preview_phase {
                FourAppAgentPhase::Thinking => self.working_color_hex.as_str(),
                FourAppAgentPhase::AwaitingInput => "#FFAA00",
                FourAppAgentPhase::Error => "#FF2200",
                FourAppAgentPhase::BackgroundActive => "#AA44FF",
                FourAppAgentPhase::Completed | FourAppAgentPhase::Idle => {
                    self.idle_color_hex.as_str()
                }
            };
            let color = parse_hex_rgb(hex).unwrap_or(0);
            if color == 0 {
                return Self::six_layer_off_entries();
            }
            let (effect, speed) = Self::phase_effect_and_speed(preview_phase);
            let sync_ambient = matches!(
                preview_phase,
                FourAppAgentPhase::AwaitingInput | FourAppAgentPhase::Error
            );
            return SIX_LAYER_SESSION_KEYS
                .into_iter()
                .enumerate()
                .map(|(idx, key)| {
                    OaiKeyStatusEntry::with_style(key, color, lit_brightness, effect, speed)
                        .with_ambient_sync(sync_ambient && idx == 0)
                })
                .collect();
        }

        let mut entries = Vec::with_capacity(SIX_LAYER_SESSION_KEYS.len());
        let mut best_ambient: Option<((u8, u8, usize), usize)> = None;
        for (layer_num, layer_offset, layer_slots) in [
            (1u8, 0usize, layer1_slots),
            (2u8, 6usize, layer2_slots),
            (3u8, 12usize, layer3_slots),
        ] {
            for slot_idx in 0..6usize {
                let entry_idx = layer_offset + slot_idx;
                let key = SIX_LAYER_SESSION_KEYS[entry_idx];
                let (entry, lit_phase) =
                    self.six_layer_slot_entry(key, layer_slots.get(slot_idx), lit_brightness);
                if entry.c != 0 && entry.b > 0.0 && entry.e != 0 {
                    let urgency = match lit_phase {
                        Some(FourAppAgentPhase::AwaitingInput) => Some(0u8),
                        Some(FourAppAgentPhase::Error) => Some(1u8),
                        _ => None,
                    };
                    if let Some(u) = urgency {
                        let rank = (u, u8::from(layer_num != preferred_layer), entry_idx);
                        if best_ambient.is_none_or(|(best_rank, _)| rank < best_rank) {
                            best_ambient = Some((rank, entry_idx));
                        }
                    }
                }
                entries.push(entry);
            }
        }
        if let Some((_, ambient_idx)) = best_ambient {
            entries[ambient_idx].sa = true;
        }
        entries
    }

    pub fn six_layer_thstatus_entries(&self) -> Vec<OaiKeyStatusEntry> {
        self.six_layer_thstatus_entries_for_layers(
            &[],
            &self.last_recomputed_slots,
            &self.last_recomputed_slots,
            2,
        )
    }

    fn apply_six_layer_confirmation(
        &mut self,
        digest: &str,
        confirmed_digest: Option<&str>,
    ) -> bool {
        let should_be_lit_active = self.lighting_enabled
            && self.brightness_scale > 0.0
            && self.last_update_result != LightingUpdateResult::SuspendedByProtectedLayer;

        if !should_be_lit_active {
            let mut changed = false;
            if self.hardware_synced {
                self.hardware_synced = false;
                changed = true;
            }
            if confirmed_digest == Some(digest)
                && self.last_synced_payload_digest.as_deref() != Some(digest)
            {
                if self.last_synced_payload_digest.is_some() {
                    self.hardware_write_count += 1;
                }
                self.last_synced_payload_digest = Some(digest.to_owned());
                changed = true;
            }
            return changed;
        }

        if confirmed_digest == Some(digest) {
            if !self.hardware_synced || self.last_synced_payload_digest.as_deref() != Some(digest) {
                self.hardware_write_count += 1;
                self.hardware_synced = true;
                self.last_synced_payload_digest = Some(digest.to_owned());
                return true;
            }
            false
        } else if self.hardware_synced || self.last_synced_payload_digest.is_some() {
            self.hardware_synced = false;
            self.last_synced_payload_digest = None;
            true
        } else {
            false
        }
    }

    pub fn sync_six_layer_hardware_for_layers<F>(
        &mut self,
        layer1_slots: &[FourAppSlotSnapshot],
        layer2_slots: &[FourAppSlotSnapshot],
        layer3_slots: &[FourAppSlotSnapshot],
        preferred_layer: u8,
        queue_and_read_confirmed: F,
    ) -> bool
    where
        F: FnOnce(&str, &[OaiKeyStatusEntry]) -> Result<Option<String>, String>,
    {
        let entries = self.six_layer_thstatus_entries_for_layers(
            layer1_slots,
            layer2_slots,
            layer3_slots,
            preferred_layer,
        );
        self.last_six_layer_entries = Some(entries.clone());
        let digest = sha256_hex(&serde_json::to_vec(&entries).unwrap_or_default());
        let confirmed = queue_and_read_confirmed(&digest, &entries).ok().flatten();
        self.apply_six_layer_confirmation(&digest, confirmed.as_deref());
        self.hardware_synced
    }

    pub fn sync_six_layer_hardware<F>(&mut self, queue_and_read_confirmed: F) -> bool
    where
        F: FnOnce(&str, &[OaiKeyStatusEntry]) -> Result<Option<String>, String>,
    {
        let entries = self.six_layer_thstatus_entries();
        self.last_six_layer_entries = Some(entries.clone());
        let digest = sha256_hex(&serde_json::to_vec(&entries).unwrap_or_default());
        let confirmed = queue_and_read_confirmed(&digest, &entries).ok().flatten();
        self.apply_six_layer_confirmation(&digest, confirmed.as_deref());
        self.hardware_synced
    }

    pub fn reconcile_six_layer_confirmed(&mut self, confirmed_digest: Option<&str>) -> bool {
        let entries = if !self.lighting_enabled
            || self.last_update_result == LightingUpdateResult::SuspendedByProtectedLayer
            || self.brightness_scale <= 0.0
        {
            Self::six_layer_off_entries()
        } else {
            self.last_six_layer_entries
                .clone()
                .unwrap_or_else(|| self.six_layer_thstatus_entries())
        };
        let digest = sha256_hex(&serde_json::to_vec(&entries).unwrap_or_default());
        self.apply_six_layer_confirmation(&digest, confirmed_digest)
    }

    pub fn last_six_layer_entries(&self) -> Option<&[OaiKeyStatusEntry]> {
        self.last_six_layer_entries.as_deref()
    }

    pub fn release(&mut self) {
        self.dual_zone_feedback = None;
        self.last_recomputed_slots.clear();
        self.last_six_layer_entries = None;
        self.last_synced_payload_digest = None;
        self.hardware_synced = false;
        self.last_update_result = LightingUpdateResult::Released;
    }

    pub fn invalidate_hardware_cache(&mut self) {
        self.last_synced_payload_digest = None;
        self.hardware_synced = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lighting_arbiter_deduplicates_identical_hardware_payload_digest() {
        let mut arbiter = FourAppLightingArbiter::new();
        let slots = vec![FourAppSlotSnapshot {
            app: "antigravity".into(),
            title: "Task 1".into(),
            status: "waiting_approval".into(),
            place: Some(NavigationPlace::antigravity("conv-1", "/tmp/ws")),
            watermark: "1:1:waiting".into(),
            status_detail: None,
        }];

        let r1 = arbiter.update_feedback(
            FourAppAgentPhase::AwaitingInput,
            &slots,
            false,
            false,
            false,
            false,
            Some("dev-1"),
            |_| Ok("{\"synced\":true}".into()),
        );
        assert_eq!(r1, LightingUpdateResult::Applied);
        assert_eq!(arbiter.hardware_write_count, 1);
        assert!(arbiter.hardware_synced);

        let r2 = arbiter.update_feedback(
            FourAppAgentPhase::AwaitingInput,
            &slots,
            false,
            false,
            false,
            false,
            Some("dev-1"),
            |_| Ok("{\"synced\":true}".into()),
        );
        assert_eq!(r2, LightingUpdateResult::Applied);
        assert_eq!(arbiter.hardware_write_count, 1);

        let done_slots = vec![FourAppSlotSnapshot {
            app: "antigravity".into(),
            title: "Task 1".into(),
            status: "completed".into(),
            place: Some(NavigationPlace::antigravity("conv-1", "/tmp/ws")),
            watermark: "1:2:done".into(),
            status_detail: None,
        }];
        let r3 = arbiter.update_feedback(
            FourAppAgentPhase::Completed,
            &done_slots,
            false,
            false,
            false,
            false,
            Some("dev-1"),
            |_| Ok("{\"synced\":true}".into()),
        );
        assert_eq!(r3, LightingUpdateResult::Applied);
        assert_eq!(arbiter.hardware_write_count, 2);

        arbiter.invalidate_hardware_cache();
        let r4 = arbiter.update_feedback(
            FourAppAgentPhase::Completed,
            &done_slots,
            false,
            false,
            false,
            false,
            Some("dev-1"),
            |_| Ok("{\"synced\":true}".into()),
        );
        assert_eq!(r4, LightingUpdateResult::Applied);
        assert_eq!(arbiter.hardware_write_count, 3);
    }

    #[test]
    fn test_response_driven_defaults_keep_running_dark_completed_solid_without_underglow() {
        let mut arbiter = FourAppLightingArbiter::new();
        assert!(!arbiter.glow_on_working_sessions);
        assert_eq!(arbiter.idle_color_hex, "#0088FF");
        assert_eq!(arbiter.working_color_hex, "#00FF66");

        let running_slot = FourAppSlotSnapshot {
            app: "antigravity".into(),
            title: "In Progress Task".into(),
            status: "running".into(),
            place: Some(NavigationPlace::antigravity("conv-run", "/tmp/ws")),
            watermark: "running:1".into(),
            status_detail: None,
        };
        let _ = arbiter.update_feedback(
            FourAppAgentPhase::Thinking,
            std::slice::from_ref(&running_slot),
            false,
            false,
            false,
            false,
            Some("dev-1"),
            |_| Ok("{\"synced\":true}".into()),
        );
        let dz_running = arbiter.dual_zone_feedback.as_ref().unwrap();
        assert_eq!(dz_running.underglow_color, "#000000");
        assert_eq!(dz_running.underglow_effect, "off");
        assert_eq!(dz_running.backlight_color, "#000000");
        assert_eq!(arbiter.full_keypad_feedback[0].color, "#000000");

        arbiter.glow_on_working_sessions = true;
        let _ = arbiter.update_feedback(
            FourAppAgentPhase::Thinking,
            &[running_slot],
            false,
            false,
            false,
            false,
            Some("dev-1"),
            |_| Ok("{\"synced\":true}".into()),
        );
        let dz_opt_in = arbiter.dual_zone_feedback.as_ref().unwrap();
        assert_eq!(dz_opt_in.backlight_color, "#00FF66");
        assert_eq!(dz_opt_in.underglow_color, "#000000");
        assert_eq!(dz_opt_in.underglow_effect, "off");
        assert_eq!(arbiter.full_keypad_feedback[0].color, "#00FF66");

        arbiter.glow_on_working_sessions = false;
        let completed_slot = FourAppSlotSnapshot {
            app: "antigravity".into(),
            title: "Completed Unread Task".into(),
            status: "completed".into(),
            place: Some(NavigationPlace::antigravity("conv-run", "/tmp/ws")),
            watermark: "idle:2".into(),
            status_detail: None,
        };
        let _ = arbiter.update_feedback(
            FourAppAgentPhase::Completed,
            &[completed_slot],
            false,
            false,
            false,
            false,
            Some("dev-1"),
            |_| Ok("{\"synced\":true}".into()),
        );
        let dz_done = arbiter.dual_zone_feedback.as_ref().unwrap();
        assert_eq!(dz_done.underglow_color, "#000000");
        assert_eq!(dz_done.underglow_effect, "off");
        assert_eq!(dz_done.backlight_color, "#000000");
        assert_eq!(arbiter.full_keypad_feedback[0].color, "#0088FF");
        let parsed: serde_json::Value =
            serde_json::from_str(arbiter.last_hardware_sync_json.as_deref().unwrap()).unwrap();
        assert_eq!(parsed["per_key"][0]["e"], 1);
        assert_eq!(parsed["per_key"][0]["s"], 0.5);
    }

    #[test]
    fn six_layer_thstatus_entries_maps_running_green_completed_solid_blue_and_all_eighteen_keys() {
        let mut arbiter = FourAppLightingArbiter::new();
        arbiter.only_glow_on_attention_or_completed = false;
        arbiter.glow_on_working_sessions = true;
        let slots = vec![
            FourAppSlotSnapshot {
                app: "antigravity".into(),
                title: "Running".into(),
                status: "running".into(),
                place: Some(NavigationPlace::antigravity("conv-run", "/tmp/ws")),
                watermark: "run:1".into(),
                status_detail: None,
            },
            FourAppSlotSnapshot {
                app: "antigravity".into(),
                title: "Completed".into(),
                status: "completed".into(),
                place: Some(NavigationPlace::antigravity("conv-done", "/tmp/ws")),
                watermark: "done:1".into(),
                status_detail: None,
            },
            FourAppSlotSnapshot {
                app: "dsh-desktop".into(),
                title: "Idle".into(),
                status: "idle".into(),
                place: Some(NavigationPlace::session("dsh-desktop", "sess-idle")),
                watermark: "idle:1".into(),
                status_detail: None,
            },
        ];

        assert_eq!(
            arbiter.recompute_feedback(
                FourAppAgentPhase::Thinking,
                &slots,
                false,
                false,
                false,
                false,
            ),
            LightingUpdateResult::Applied
        );

        assert!(arbiter
            .full_keypad_feedback
            .iter()
            .all(|fb| !fb.binding_id.starts_with("ENC_")));
        assert!(arbiter
            .per_key_feedback
            .iter()
            .all(|fb| !fb.binding_id.starts_with("ENC_")));

        let entries = arbiter.six_layer_thstatus_entries();
        let keys: Vec<&str> = entries.iter().map(|e| e.k.as_str()).collect();
        assert_eq!(keys, SIX_LAYER_SESSION_KEYS.to_vec());
        for idx in 0..6 {
            assert_eq!(entries[idx].c, 0);
            assert_eq!(entries[idx].e, 0);
            assert!(!entries[idx].sa);
        }
        assert_eq!(entries[6].c, 0x00FF66);
        assert_eq!(entries[6].e, 4);
        assert_eq!(entries[12].c, 0x00FF66);
        assert_eq!(entries[7].c, 0x0088FF);
        assert_eq!(entries[7].e, 1);
        assert_eq!(entries[7].s, 0.5);
        assert_eq!(entries[13].c, 0x0088FF);
        assert_eq!(entries[13].e, 1);
        assert_eq!(entries[8].c, 0x0088FF);
        assert_eq!(entries[8].e, 1);
        assert_eq!(entries[14].c, 0x0088FF);
        assert!(entries.iter().all(|e| !e.sa));
        for empty_idx in [9, 10, 11, 15, 16, 17] {
            assert_eq!(entries[empty_idx].c, 0);
            assert_eq!(entries[empty_idx].b, 0.0);
            assert_eq!(entries[empty_idx].e, 0);
        }
    }

    #[test]
    fn six_layer_thstatus_for_layers_selects_single_ambient_sync_by_urgency_preferred_layer_and_index(
    ) {
        let arbiter = FourAppLightingArbiter::new();
        let l1 = [
            FourAppSlotSnapshot {
                app: "com.openai.codex".into(),
                title: "Codex Error".into(),
                status: "error".into(),
                place: Some(NavigationPlace::codex("c-err", "local", "/tmp/c")),
                watermark: "w1".into(),
                status_detail: None,
            },
            FourAppSlotSnapshot {
                app: "com.openai.codex".into(),
                title: "Codex Waiting".into(),
                status: "waiting_approval".into(),
                place: Some(NavigationPlace::codex("c-wait", "local", "/tmp/c")),
                watermark: "w2".into(),
                status_detail: None,
            },
        ];
        let l2 = [
            FourAppSlotSnapshot {
                app: "com.google.antigravity".into(),
                title: "AG Waiting".into(),
                status: "waiting".into(),
                place: Some(NavigationPlace::antigravity("ag-wait", "/tmp/ag")),
                watermark: "w3".into(),
                status_detail: None,
            },
            FourAppSlotSnapshot {
                app: "com.google.antigravity".into(),
                title: "AG Done".into(),
                status: "completed".into(),
                place: Some(NavigationPlace::antigravity("ag-done", "/tmp/ag")),
                watermark: "w4".into(),
                status_detail: None,
            },
        ];

        let prefer_l2 = arbiter.six_layer_thstatus_entries_for_layers(&l1, &l2, &[], 2);
        assert_eq!(prefer_l2.len(), 18);
        assert_eq!(prefer_l2.iter().filter(|e| e.sa).count(), 1);
        assert!(prefer_l2[6].sa);
        assert_eq!(prefer_l2[0].c, 0xFF2200);
        assert_eq!(prefer_l2[0].e, 4);
        assert_eq!(prefer_l2[0].s, 0.75);
        assert_eq!(prefer_l2[1].c, 0xFFAA00);
        assert_eq!(prefer_l2[1].e, 4);
        assert_eq!(prefer_l2[7].c, 0x0088FF);
        assert_eq!(prefer_l2[7].e, 1);
        assert!(!prefer_l2[7].sa);

        let prefer_l1 = arbiter.six_layer_thstatus_entries_for_layers(&l1, &l2, &[], 1);
        assert_eq!(prefer_l1.iter().filter(|e| e.sa).count(), 1);
        assert!(prefer_l1[1].sa);

        let completed_only = arbiter.six_layer_thstatus_entries_for_layers(&[], &l2[1..2], &[], 2);
        assert!(completed_only.iter().all(|e| !e.sa));
        assert_eq!(completed_only[6].c, 0x0088FF);
        assert_eq!(completed_only[6].e, 1);
    }

    #[test]
    fn sync_six_layer_hardware_waits_for_wire_ack_and_prevents_stale_replay_on_offline_disable() {
        let mut arbiter = FourAppLightingArbiter::new();
        arbiter.manual_preview_phase = Some(FourAppAgentPhase::AwaitingInput);
        arbiter.recompute_feedback(FourAppAgentPhase::Idle, &[], false, false, false, false);
        let preview_entries = arbiter.six_layer_thstatus_entries();
        assert_eq!(preview_entries.len(), 18);
        assert!(preview_entries
            .iter()
            .all(|e| e.c == 0xFFAA00 && e.e == 4 && e.b > 0.0));
        assert!(preview_entries[0].sa);
        assert!(preview_entries[1..].iter().all(|e| !e.sa));

        arbiter.manual_preview_phase = Some(FourAppAgentPhase::Completed);
        let completed_preview = arbiter.six_layer_thstatus_entries();
        assert!(completed_preview.iter().all(|e| e.e == 1 && !e.sa));
        arbiter.manual_preview_phase = None;

        arbiter.glow_on_working_sessions = true;
        let slots = vec![FourAppSlotSnapshot {
            app: "antigravity".into(),
            title: "Running".into(),
            status: "running".into(),
            place: Some(NavigationPlace::antigravity("conv-run", "/tmp/ws")),
            watermark: "run:1".into(),
            status_detail: None,
        }];
        arbiter.recompute_feedback(
            FourAppAgentPhase::Thinking,
            &slots,
            false,
            false,
            false,
            false,
        );

        let mut queued: Option<(String, Vec<OaiKeyStatusEntry>)> = None;
        assert!(!arbiter.sync_six_layer_hardware(|digest, entries| {
            queued = Some((digest.to_owned(), entries.to_vec()));
            Ok(None)
        }));
        assert!(!arbiter.hardware_synced);
        assert!(arbiter.last_synced_payload_digest.is_none());
        assert_eq!(arbiter.hardware_write_count, 0);
        let (lit_digest, lit_entries) = queued.clone().unwrap();
        assert_eq!(lit_entries[6].c, 0x00FF66);
        assert_eq!(lit_entries[12].c, 0x00FF66);

        assert!(arbiter.reconcile_six_layer_confirmed(Some(&lit_digest)));
        assert!(arbiter.hardware_synced);
        assert_eq!(arbiter.hardware_write_count, 1);
        assert!(!arbiter.reconcile_six_layer_confirmed(Some(&lit_digest)));
        assert_eq!(arbiter.hardware_write_count, 1);

        assert!(arbiter.reconcile_six_layer_confirmed(None));
        assert!(!arbiter.hardware_synced);
        assert!(arbiter.last_synced_payload_digest.is_none());

        arbiter.lighting_enabled = false;
        arbiter.recompute_feedback(
            FourAppAgentPhase::Thinking,
            &slots,
            false,
            false,
            false,
            false,
        );
        assert!(!arbiter.sync_six_layer_hardware(|digest, entries| {
            queued = Some((digest.to_owned(), entries.to_vec()));
            Ok(None)
        }));
        let (off_digest, off_entries) = queued.unwrap();
        assert_ne!(off_digest, lit_digest);
        assert!(off_entries
            .iter()
            .all(|e| e.c == 0 && e.b == 0.0 && e.e == 0 && !e.sa));
        assert!(!arbiter.hardware_synced);

        assert!(arbiter.reconcile_six_layer_confirmed(Some(&off_digest)));
        assert!(!arbiter.hardware_synced);
        assert_eq!(
            arbiter.last_synced_payload_digest.as_deref(),
            Some(off_digest.as_str())
        );
    }

    #[test]
    fn unlinked_session_lighting_and_error_watermark_acknowledgement_align_ui_and_hardware() {
        let mut arbiter = FourAppLightingArbiter::new();
        arbiter.session_lighting_linked = false;
        let err_place = NavigationPlace::antigravity("conv-err", "/tmp/ws");
        let run_place = NavigationPlace::antigravity("conv-run", "/tmp/ws");
        let slots = vec![
            FourAppSlotSnapshot {
                app: "antigravity".into(),
                title: "Errored".into(),
                status: "error".into(),
                place: Some(err_place.clone()),
                watermark: "err:1".into(),
                status_detail: None,
            },
            FourAppSlotSnapshot {
                app: "antigravity".into(),
                title: "Running".into(),
                status: "running".into(),
                place: Some(run_place),
                watermark: "run:1".into(),
                status_detail: None,
            },
        ];

        arbiter.recompute_feedback(FourAppAgentPhase::Error, &slots, false, false, false, false);
        assert_eq!(arbiter.full_keypad_feedback[0].color, "#0088FF");
        assert_eq!(arbiter.full_keypad_feedback[1].color, "#000000");
        assert_eq!(
            arbiter.dual_zone_feedback.as_ref().unwrap().underglow_color,
            "#0088FF"
        );
        let unlinked_entries = arbiter.six_layer_thstatus_entries_for_layers(&[], &slots, &[], 2);
        assert_eq!(unlinked_entries[6].c, 0x0088FF);
        assert_eq!(unlinked_entries[7].c, 0);

        arbiter.session_lighting_linked = true;
        arbiter.acknowledge_slot(&err_place, "err:1");
        assert!(!arbiter.is_slot_attention_needed(&slots[0]));
        arbiter.recompute_feedback(FourAppAgentPhase::Idle, &slots, false, false, false, false);
        assert_eq!(arbiter.full_keypad_feedback[0].color, "#000000");
        assert_eq!(
            arbiter.dual_zone_feedback.as_ref().unwrap().underglow_color,
            "#000000"
        );
        let acked_entries = arbiter.six_layer_thstatus_entries_for_layers(&[], &slots, &[], 2);
        assert_eq!(acked_entries[6].c, 0);
        assert!(acked_entries.iter().all(|e| !e.sa));
    }
}
