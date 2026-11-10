use crate::ownership::{NativeOwnershipController, OwnershipState};
use mi_model::navigation_history::NavigationPlace;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LightingUpdateResult {
    Applied,
    YieldedToNative,
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
            glow_on_working_sessions: true,
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
        }
    }

    pub fn can_send_lighting(
        &self,
        ownership: &NativeOwnershipController,
        is_protected_layer: bool,
        preview_busy: bool,
    ) -> bool {
        if matches!(ownership.state(), OwnershipState::Native { .. }) {
            return false;
        }
        if !ownership.allows_execution() || is_protected_layer || preview_busy {
            return false;
        }
        true
    }

    pub fn is_slot_attention_needed(&self, slot: &FourAppSlotSnapshot) -> bool {
        let Some(place) = &slot.place else {
            return false;
        };
        if matches!(
            slot.status.as_str(),
            "waiting"
                | "waiting_approval"
                | "needs-interaction"
                | "needs_interaction"
                | "error"
                | "failed"
        ) {
            return true;
        }
        if slot.status == "completed" {
            let key = place.target_id();
            return self.acknowledged_watermarks.get(&key) != Some(&slot.watermark);
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

    pub fn acknowledge_slot(&mut self, place: &NavigationPlace, watermark: impl Into<String>) {
        self.acknowledged_watermarks
            .insert(place.target_id(), watermark.into());
    }

    pub fn derive_dual_zone_feedback(
        &self,
        phase: FourAppAgentPhase,
        dictation_active: bool,
        native_ownership_active: bool,
    ) -> DualZoneLightingFeedback {
        let idle_hex = self.idle_color_hex.as_str();
        let working_hex = self.working_color_hex.as_str();
        let (bl_color, bl_effect, bl_brightness, bl_speed) = match phase {
            FourAppAgentPhase::Idle => (idle_hex, "solid", 0.72, 0.5),
            FourAppAgentPhase::Thinking => (working_hex, "breath", 0.85, 0.7),
            FourAppAgentPhase::AwaitingInput => ("#FFAA00", "breath", 1.0, 0.8),
            FourAppAgentPhase::Completed => (idle_hex, "breath", 0.95, 0.65),
            FourAppAgentPhase::Error => ("#FF2200", "breath", 1.0, 0.9),
            FourAppAgentPhase::BackgroundActive => ("#AA44FF", "breath", 0.7, 0.5),
        };
        let (ug_color, ug_effect, ug_brightness, ug_speed) = if native_ownership_active {
            ("#000000", "off", 0.0, 0.0)
        } else if dictation_active {
            ("#8800FF", "solid", 1.0, 0.5)
        } else if phase == FourAppAgentPhase::Completed {
            (idle_hex, "breath", 0.95, 0.65)
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
                let (phase, color) = match slot.status.as_str() {
                    "running" | "runningTool" | "generating" | "busy" | "active" => {
                        (FourAppAgentPhase::Thinking, working_hex)
                    }
                    "waiting" | "waiting_approval" | "needs-interaction" | "needs_interaction" => {
                        (FourAppAgentPhase::AwaitingInput, "#FFAA00")
                    }
                    "error" => (FourAppAgentPhase::Error, "#FF2200"),
                    "background_active" => (FourAppAgentPhase::BackgroundActive, "#AA44FF"),
                    "completed" => (FourAppAgentPhase::Completed, idle_hex),
                    _ => (FourAppAgentPhase::Idle, idle_hex),
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
        result.push(PerKeyLightingFeedback {
            binding_id: "ENC_CLK".into(),
            color: "#FFFFFF".into(),
            phase: FourAppAgentPhase::Idle,
            label: "旋钮确认/跳底".into(),
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
        entries.push(PerKeyLightingFeedback {
            binding_id: "ENC_CLK".into(),
            color: "#FFFFFF".into(),
            phase: FourAppAgentPhase::Idle,
            label: "旋钮确认/跳底".into(),
        });
        entries
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_feedback<F>(
        &mut self,
        live_phase: FourAppAgentPhase,
        slots: &[FourAppSlotSnapshot],
        dictation_active: bool,
        ownership: &NativeOwnershipController,
        is_protected_layer: bool,
        preview_busy: bool,
        is_bottom_row_split: bool,
        connected_device_id: Option<&str>,
        write_hardware: F,
    ) -> LightingUpdateResult
    where
        F: FnMut(&str) -> Result<String, String>,
    {
        self.update_feedback_at(
            live_phase,
            slots,
            dictation_active,
            ownership,
            is_protected_layer,
            preview_busy,
            is_bottom_row_split,
            connected_device_id,
            std::time::Instant::now(),
            write_hardware,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_feedback_at<F>(
        &mut self,
        live_phase: FourAppAgentPhase,
        slots: &[FourAppSlotSnapshot],
        dictation_active: bool,
        ownership: &NativeOwnershipController,
        is_protected_layer: bool,
        preview_busy: bool,
        is_bottom_row_split: bool,
        connected_device_id: Option<&str>,
        now: std::time::Instant,
        mut write_hardware: F,
    ) -> LightingUpdateResult
    where
        F: FnMut(&str) -> Result<String, String>,
    {
        let phase = if let Some(manual) = self.manual_preview_phase {
            manual
        } else if !self.session_lighting_linked {
            FourAppAgentPhase::Idle
        } else {
            live_phase
        };
        self.active_phase = phase;

        if matches!(ownership.state(), OwnershipState::Native { .. })
            || !ownership.allows_execution_at(now)
        {
            self.release();
            self.last_update_result = LightingUpdateResult::YieldedToNative;
            return LightingUpdateResult::YieldedToNative;
        }
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
        let attention_slots: Vec<&FourAppSlotSnapshot> = slots
            .iter()
            .filter(|s| self.is_slot_attention_needed(s))
            .collect();
        let working_slots: Vec<&FourAppSlotSnapshot> =
            slots.iter().filter(|s| self.is_slot_working(s)).collect();
        let has_any_attention = !attention_slots.is_empty();

        let mut dual_zone = self.derive_dual_zone_feedback(phase, dictation_active, false);
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
        } else if self.only_glow_on_attention_or_completed
            && self.manual_preview_phase.is_none()
            && self.session_lighting_linked
        {
            dual_zone.backlight_color = "#000000".into();
            dual_zone.backlight_effect = "off".into();
            dual_zone.backlight_brightness = 0.0;
            dual_zone.backlight_speed = 0.0;
            if dictation_active {
                dual_zone.underglow_color = "#8800FF".into();
                dual_zone.underglow_effect = "solid".into();
                dual_zone.underglow_brightness = (1.0 * clamped_scale).min(1.0);
                dual_zone.underglow_speed = 0.5;
            } else if phase == FourAppAgentPhase::AwaitingInput
                || attention_slots.iter().any(|s| s.status != "completed")
            {
                dual_zone.backlight_color = "#FFAA00".into();
                dual_zone.backlight_effect = "breath".into();
                dual_zone.backlight_brightness = (0.85 * clamped_scale).min(1.0);
                dual_zone.backlight_speed = 0.75;
                dual_zone.underglow_color = "#FFAA00".into();
                dual_zone.underglow_effect = "breath".into();
                dual_zone.underglow_brightness = (0.95 * clamped_scale).min(1.0);
                dual_zone.underglow_speed = 0.75;
            } else if phase == FourAppAgentPhase::Completed || !attention_slots.is_empty() {
                dual_zone.backlight_color = self.idle_color_hex.clone();
                dual_zone.backlight_effect = "breath".into();
                dual_zone.backlight_brightness = (0.85 * clamped_scale).min(1.0);
                dual_zone.backlight_speed = 0.65;
                dual_zone.underglow_color = self.idle_color_hex.clone();
                dual_zone.underglow_effect = "breath".into();
                dual_zone.underglow_brightness = (0.95 * clamped_scale).min(1.0);
                dual_zone.underglow_speed = 0.65;
            } else if self.glow_on_working_sessions
                && (phase == FourAppAgentPhase::Thinking || !working_slots.is_empty())
            {
                dual_zone.backlight_color = self.working_color_hex.clone();
                dual_zone.backlight_effect = "breath".into();
                dual_zone.backlight_brightness = (0.80 * clamped_scale).min(1.0);
                dual_zone.backlight_speed = 0.65;
                dual_zone.underglow_color = self.working_color_hex.clone();
                dual_zone.underglow_effect = "breath".into();
                dual_zone.underglow_brightness = (0.85 * clamped_scale).min(1.0);
                dual_zone.underglow_speed = 0.65;
            } else {
                dual_zone.underglow_color = "#000000".into();
                dual_zone.underglow_effect = "off".into();
                dual_zone.underglow_brightness = 0.0;
                dual_zone.underglow_speed = 0.0;
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
        if self.only_glow_on_attention_or_completed
            && self.manual_preview_phase.is_none()
            && self.session_lighting_linked
        {
            let slot_ids = ["AG00", "AG01", "AG02", "AG03", "AG04", "AG05"];
            full_keys = full_keys
                .into_iter()
                .map(|item| {
                    if let Some(idx) = slot_ids.iter().position(|k| *k == item.binding_id) {
                        if let Some(slot) = slots.get(idx) {
                            if self.is_slot_attention_needed(slot)
                                || (self.glow_on_working_sessions && self.is_slot_working(slot))
                            {
                                return item;
                            }
                        }
                        return PerKeyLightingFeedback {
                            binding_id: item.binding_id,
                            color: "#000000".into(),
                            phase: FourAppAgentPhase::Idle,
                            label: item.label,
                        };
                    }
                    if item.binding_id == "AG06" && has_any_attention {
                        let attn_color = if phase == FourAppAgentPhase::AwaitingInput {
                            "#FFAA00"
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
                    if (item.binding_id == "AG10" || item.binding_id == "AG11") && dictation_active
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
                let key_effect = if !is_lit {
                    0
                } else if matches!(
                    item.phase,
                    FourAppAgentPhase::Completed
                        | FourAppAgentPhase::AwaitingInput
                        | FourAppAgentPhase::Error
                        | FourAppAgentPhase::Thinking
                ) {
                    4
                } else {
                    1
                };
                json!({
                    "k": item.binding_id,
                    "color": if is_lit { hex } else { "#000000" },
                    "brightness": key_brightness,
                    "e": key_effect,
                    "s": 0.65,
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
        let digest = format!("{:x}", Sha256::digest(payload_str.as_bytes()));
        self.dual_zone_feedback = Some(dual_zone);
        self.full_keypad_feedback = full_keys;
        self.last_hardware_sync_json = Some(payload_str.clone());

        if let Some(device_id) = connected_device_id {
            let needs_sync = !self.hardware_synced
                || self.last_synced_payload_digest.as_deref() != Some(digest.as_str())
                || self.last_synced_device_id.as_deref() != Some(device_id);
            if needs_sync {
                match write_hardware(&payload_str) {
                    Ok(resp) if resp.contains("\"synced\":true") => {
                        self.last_synced_payload_digest = Some(digest);
                        self.last_synced_device_id = Some(device_id.to_owned());
                        self.hardware_synced = true;
                        self.hardware_write_count += 1;
                    }
                    _ => {
                        self.hardware_synced = false;
                    }
                }
            }
        } else {
            self.last_synced_payload_digest = None;
            self.last_synced_device_id = None;
            self.hardware_synced = false;
        }

        self.last_update_result = LightingUpdateResult::Applied;
        LightingUpdateResult::Applied
    }

    pub fn release(&mut self) {
        self.dual_zone_feedback = None;
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
        let ownership = NativeOwnershipController::new_isolated();
        let slots = vec![FourAppSlotSnapshot {
            app: "antigravity".into(),
            title: "Task 1".into(),
            status: "waiting_approval".into(),
            place: Some(NavigationPlace::antigravity("conv-1", "/tmp/ws")),
            watermark: "1:1:waiting".into(),
        }];

        let r1 = arbiter.update_feedback(
            FourAppAgentPhase::AwaitingInput,
            &slots,
            false,
            &ownership,
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
            &ownership,
            false,
            false,
            false,
            Some("dev-1"),
            |_| Ok("{\"synced\":true}".into()),
        );
        assert_eq!(r2, LightingUpdateResult::Applied);
        assert_eq!(arbiter.hardware_write_count, 1);

        let r3 = arbiter.update_feedback(
            FourAppAgentPhase::Completed,
            &slots,
            false,
            &ownership,
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
            &slots,
            false,
            &ownership,
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
    fn test_antigravity_idle_is_blue_and_in_progress_is_distinct_color_by_default() {
        let mut arbiter = FourAppLightingArbiter::new();
        let ownership = NativeOwnershipController::new_isolated();
        assert!(arbiter.glow_on_working_sessions);
        assert_eq!(arbiter.idle_color_hex, "#0088FF");
        assert_eq!(arbiter.working_color_hex, "#00FF66");

        let running_slot = FourAppSlotSnapshot {
            app: "antigravity".into(),
            title: "In Progress Task".into(),
            status: "running".into(),
            place: Some(NavigationPlace::antigravity("conv-run", "/tmp/ws")),
            watermark: "running:1".into(),
        };
        let _ = arbiter.update_feedback(
            FourAppAgentPhase::Thinking,
            &[running_slot],
            false,
            &ownership,
            false,
            false,
            false,
            Some("dev-1"),
            |_| Ok("{\"synced\":true}".into()),
        );
        let dz_running = arbiter.dual_zone_feedback.as_ref().unwrap();
        assert_eq!(dz_running.underglow_color, "#00FF66");
        assert_eq!(dz_running.backlight_color, "#00FF66");
        assert_eq!(arbiter.full_keypad_feedback[0].color, "#00FF66");

        let completed_slot = FourAppSlotSnapshot {
            app: "antigravity".into(),
            title: "Idle Completed Task".into(),
            status: "completed".into(),
            place: Some(NavigationPlace::antigravity("conv-run", "/tmp/ws")),
            watermark: "idle:2".into(),
        };
        let _ = arbiter.update_feedback(
            FourAppAgentPhase::Completed,
            &[completed_slot],
            false,
            &ownership,
            false,
            false,
            false,
            Some("dev-1"),
            |_| Ok("{\"synced\":true}".into()),
        );
        let dz_idle = arbiter.dual_zone_feedback.as_ref().unwrap();
        assert_eq!(dz_idle.underglow_color, "#0088FF");
        assert_eq!(dz_idle.backlight_color, "#0088FF");
        assert_eq!(arbiter.full_keypad_feedback[0].color, "#0088FF");
    }
}
