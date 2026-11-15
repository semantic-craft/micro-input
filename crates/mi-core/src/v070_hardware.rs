use std::ops::{Deref, DerefMut};

use mi_model::SaId;
use mi_proto::v070_capabilities::{
    validate_device_name, DeviceNameInfo, JoystickDeadzone, MouseConfig, MouseSpeedPreset,
    PmConfig, PmState, SaRgbEffect, SaRgbUpdate,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct V070MouseConfigProjection {
    pub preset: String,
    #[serde(flatten)]
    pub config: MouseConfig,
    pub deadzone: f64,
}

impl Deref for V070MouseConfigProjection {
    type Target = MouseConfig;

    fn deref(&self) -> &Self::Target {
        &self.config
    }
}

impl DerefMut for V070MouseConfigProjection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.config
    }
}

impl Default for V070MouseConfigProjection {
    fn default() -> Self {
        Self::from_mouse_config(&MouseConfig::default(), JoystickDeadzone::DEFAULT)
    }
}

impl V070MouseConfigProjection {
    pub fn from_mouse_config(cfg: &MouseConfig, deadzone: f64) -> Self {
        let preset = cfg
            .matching_preset()
            .map(MouseSpeedPreset::as_str)
            .unwrap_or("custom")
            .to_string();
        let safe_deadzone = if deadzone.is_finite() {
            deadzone
        } else {
            JoystickDeadzone::DEFAULT
        };
        Self {
            preset,
            config: *cfg,
            deadzone: safe_deadzone,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct V070PmStateProjection {
    #[serde(flatten)]
    pub inner: PmState,
    pub usb_host_hold: bool,
}

impl Deref for V070PmStateProjection {
    type Target = PmState;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for V070PmStateProjection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl V070PmStateProjection {
    pub fn from_pm_state(state: &PmState) -> Self {
        Self {
            inner: state.clone(),
            usb_host_hold: state.usb_host_hold().is_some(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct V070SaRgbProjection {
    pub sa: Option<u32>,
    pub color_hex: String,
    pub brightness: f64,
    pub speed: f64,
    pub effect: u8,
}

impl Default for V070SaRgbProjection {
    fn default() -> Self {
        Self {
            sa: None,
            color_hex: "#FFFFFF".into(),
            brightness: 1.0,
            speed: 0.5,
            effect: SaRgbEffect::Solid.as_u8(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct V070HardwareProjection {
    pub supported: bool,
    pub busy: bool,
    pub synced: bool,
    pub active_profile: Option<u8>,
    pub active_layer: Option<u8>,
    pub battery_percent: Option<u8>,
    pub is_charging: Option<bool>,
    pub host_sleep_status: String,
    pub status_message: String,
    pub last_error: Option<String>,
    #[serde(default)]
    pub auto_layer_sync_enabled: bool,
    #[serde(default)]
    pub session_sa_rgb_sync_enabled: bool,
    #[serde(default)]
    pub preferred_device_serial: Option<String>,
    pub mouse: V070MouseConfigProjection,
    pub pm_config: PmConfig,
    pub pm_state: V070PmStateProjection,
    pub device_name: DeviceNameInfo,
    pub sa_rgb: V070SaRgbProjection,
}

impl Default for V070HardwareProjection {
    fn default() -> Self {
        Self {
            supported: false,
            busy: false,
            synced: false,
            active_profile: None,
            active_layer: None,
            battery_percent: None,
            is_charging: None,
            host_sleep_status: "awake".into(),
            status_message: "需连接 0.7.0-rc.16+ 独占固件设备".into(),
            last_error: None,
            auto_layer_sync_enabled: false,
            session_sa_rgb_sync_enabled: false,
            preferred_device_serial: None,
            mouse: V070MouseConfigProjection::default(),
            pm_config: PmConfig::default(),
            pm_state: V070PmStateProjection::default(),
            device_name: DeviceNameInfo::default(),
            sa_rgb: V070SaRgbProjection::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct V070SnapshotPayload {
    pub session_id: u64,
    pub version: String,
    pub active_profile: Option<u8>,
    pub active_layer: Option<u8>,
    pub battery_percent: Option<u8>,
    pub is_charging: Option<bool>,
    pub mouse: MouseConfig,
    pub deadzone: f64,
    pub pm_config: PmConfig,
    pub pm_state: PmState,
    pub device_name: DeviceNameInfo,
}

#[derive(Debug, Clone)]
pub enum V070BridgeOutcome {
    Snapshot(V070SnapshotPayload),
    ProfileLayer {
        profile_index: Option<u8>,
        layer_index: u8,
    },
    MouseConfigUpdated {
        mouse: MouseConfig,
    },
    DeadzoneUpdated {
        deadzone: f64,
    },
    PmConfigUpdated {
        pm_config: PmConfig,
    },
    DeviceRenamed {
        device_name: DeviceNameInfo,
    },
    SaRgbApplied {
        sa: SaId,
        rgb: u32,
        brightness: f64,
        speed: f64,
        effect: SaRgbEffect,
    },
    SaRgbReset {
        sa: SaId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum V070HardwareAction {
    Refresh,
    SetAutoLayerSync {
        enabled: bool,
    },
    SetSessionSaRgbSync {
        enabled: bool,
    },
    SelectLayer {
        layer: u8,
    },
    ApplyMousePreset {
        preset: String,
    },
    ApplyMouseConfig {
        move_delta: u16,
        max_speed: u16,
        wheel_delta: u16,
        wheel_max_speed: u16,
        stick_speed: u16,
        stick_scroll: u16,
    },
    ApplyJoystickDeadzone {
        deadzone: f64,
    },
    ApplyPmConfig {
        auto_sleep: bool,
        host_sleep: bool,
        sleep_after_min: u16,
    },
    RenameDevice {
        name: String,
    },
    ApplySaRgb {
        sa: u32,
        color_hex: String,
        brightness: f64,
        speed: f64,
        effect: u8,
    },
    ResetSaRgb {
        sa: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum V070HostPreference {
    AutoLayerSync { enabled: bool },
    SessionSaRgbSync { enabled: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ValidatedV070DeviceCommand {
    Refresh,
    SelectLayer {
        layer: u8,
    },
    ApplyMousePreset {
        config: MouseConfig,
    },
    ApplyMouseConfig {
        config: MouseConfig,
    },
    ApplyJoystickDeadzone {
        deadzone: JoystickDeadzone,
    },
    ApplyPmConfig(PmConfig),
    RenameDevice(String),
    ApplySaRgb {
        sa: SaId,
        rgb: u32,
        brightness: f64,
        speed: f64,
        effect: SaRgbEffect,
        update: SaRgbUpdate,
    },
    ResetSaRgb {
        sa: SaId,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ParsedV070Action {
    HostPreference(V070HostPreference),
    DeviceCommand(ValidatedV070DeviceCommand),
}

pub(crate) fn parse_v070_action(
    action: V070HardwareAction,
    base_mouse: &MouseConfig,
    max_name_length: usize,
) -> Result<ParsedV070Action, String> {
    match action {
        V070HardwareAction::SetAutoLayerSync { enabled } => Ok(ParsedV070Action::HostPreference(
            V070HostPreference::AutoLayerSync { enabled },
        )),
        V070HardwareAction::SetSessionSaRgbSync { enabled } => Ok(
            ParsedV070Action::HostPreference(V070HostPreference::SessionSaRgbSync { enabled }),
        ),
        V070HardwareAction::Refresh => Ok(ParsedV070Action::DeviceCommand(
            ValidatedV070DeviceCommand::Refresh,
        )),
        V070HardwareAction::SelectLayer { layer } => {
            if layer == 0 || layer > 6 {
                return Err(format!(
                    "板载层索引 {layer} 超出有效范围（仅支持第 1–6 层）"
                ));
            }
            Ok(ParsedV070Action::DeviceCommand(
                ValidatedV070DeviceCommand::SelectLayer { layer },
            ))
        }
        V070HardwareAction::ApplyMousePreset { preset } => {
            let parsed = MouseSpeedPreset::parse(&preset).map_err(|e| e.to_string())?;
            Ok(ParsedV070Action::DeviceCommand(
                ValidatedV070DeviceCommand::ApplyMousePreset {
                    config: parsed.to_mouse_config(),
                },
            ))
        }
        V070HardwareAction::ApplyMouseConfig {
            move_delta,
            max_speed,
            wheel_delta,
            wheel_max_speed,
            stick_speed,
            stick_scroll,
        } => {
            let mut config = *base_mouse;
            config.move_delta = move_delta;
            config.max_speed = max_speed;
            config.wheel_delta = wheel_delta;
            config.wheel_max_speed = wheel_max_speed;
            config.stick_speed = stick_speed;
            config.stick_scroll = stick_scroll;
            config.validate().map_err(|e| e.to_string())?;
            Ok(ParsedV070Action::DeviceCommand(
                ValidatedV070DeviceCommand::ApplyMouseConfig { config },
            ))
        }
        V070HardwareAction::ApplyJoystickDeadzone { deadzone } => {
            let deadzone = JoystickDeadzone::new(deadzone).map_err(|e| e.to_string())?;
            Ok(ParsedV070Action::DeviceCommand(
                ValidatedV070DeviceCommand::ApplyJoystickDeadzone { deadzone },
            ))
        }
        V070HardwareAction::ApplyPmConfig {
            auto_sleep,
            host_sleep,
            sleep_after_min,
        } => {
            let cfg = PmConfig {
                auto_sleep,
                host_sleep,
                sleep_after_min,
            };
            cfg.validate().map_err(|e| e.to_string())?;
            Ok(ParsedV070Action::DeviceCommand(
                ValidatedV070DeviceCommand::ApplyPmConfig(cfg),
            ))
        }
        V070HardwareAction::RenameDevice { name } => {
            let validated =
                validate_device_name(&name, max_name_length).map_err(|e| e.to_string())?;
            Ok(ParsedV070Action::DeviceCommand(
                ValidatedV070DeviceCommand::RenameDevice(validated),
            ))
        }
        V070HardwareAction::ApplySaRgb {
            sa,
            color_hex,
            brightness,
            speed,
            effect,
        } => {
            let rgb = parse_hex_color(&color_hex)?;
            if !brightness.is_finite() || !speed.is_finite() {
                return Err("亮度和速度必须为有限数值".into());
            }
            let parsed_effect = SaRgbEffect::from_u8(effect).map_err(|e| e.to_string())?;
            let update = SaRgbUpdate {
                sa: vec![sa],
                c: Some(rgb),
                b: Some(brightness as f32),
                s: Some(speed as f32),
                e: Some(parsed_effect.as_u8()),
            };
            update.validate().map_err(|e| e.to_string())?;
            let sa_id =
                SaId::new(sa).ok_or_else(|| format!("sa id {sa} must be in 1..=0xFFFFFF"))?;
            Ok(ParsedV070Action::DeviceCommand(
                ValidatedV070DeviceCommand::ApplySaRgb {
                    sa: sa_id,
                    rgb,
                    brightness,
                    speed,
                    effect: parsed_effect,
                    update,
                },
            ))
        }
        V070HardwareAction::ResetSaRgb { sa } => {
            let update = SaRgbUpdate::reset(vec![sa]);
            update.validate().map_err(|e| e.to_string())?;
            let sa_id =
                SaId::new(sa).ok_or_else(|| format!("sa id {sa} must be in 1..=0xFFFFFF"))?;
            Ok(ParsedV070Action::DeviceCommand(
                ValidatedV070DeviceCommand::ResetSaRgb { sa: sa_id },
            ))
        }
    }
}

pub(crate) fn parse_hex_color(raw: &str) -> Result<u32, String> {
    let trimmed = raw
        .trim()
        .strip_prefix('#')
        .or_else(|| raw.trim().strip_prefix("0x"))
        .or_else(|| raw.trim().strip_prefix("0X"))
        .unwrap_or(raw.trim());
    if trimmed.len() != 6 || !trimmed.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("RGB 颜色必须为 6 位十六进制（例如 #33AAFF）".into());
    }
    u32::from_str_radix(trimmed, 16).map_err(|_| "RGB 颜色格式无效".into())
}

pub(crate) fn format_hex_color(rgb: u32) -> String {
    format!("#{:06X}", rgb & 0xFF_FFFF)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mi_proto::v070_capabilities::{PmHold, MAX_DEVICE_NAME_LEN};
    use serde_json::{json, Value};

    #[test]
    fn flattened_projections_preserve_flat_json_and_deref_ergonomics() {
        let mut mouse = V070MouseConfigProjection::default();
        assert_eq!(mouse.move_delta, MouseConfig::default().move_delta);
        mouse.move_delta = 12;
        assert_eq!(mouse.config.move_delta, 12);

        let mouse_json = serde_json::to_value(&mouse).unwrap();
        let obj = mouse_json.as_object().unwrap();
        assert!(obj.get("config").is_none());
        assert_eq!(obj["preset"], "standard");
        assert_eq!(obj["move_delta"], 12);
        assert_eq!(obj["deadzone"], JoystickDeadzone::DEFAULT);
        let decoded_mouse: V070MouseConfigProjection = serde_json::from_value(json!({
            "preset": "fast",
            "delay_ms": 80,
            "interval_ms": 15,
            "move_delta": 12,
            "max_speed": 20,
            "time_to_max": 25,
            "wheel_delay_ms": 90,
            "wheel_interval_ms": 60,
            "wheel_delta": 2,
            "wheel_max_speed": 12,
            "wheel_time_to_max": 35,
            "stick_speed": 4,
            "stick_regulator": 18,
            "stick_scroll": 120,
            "deadzone": 0.25,
        }))
        .unwrap();
        assert_eq!(decoded_mouse.move_delta, 12);
        assert_eq!(decoded_mouse.deadzone, 0.25);

        let pm_state = PmState {
            state: "active".into(),
            floor: "usb".into(),
            base_floor: "idle".into(),
            line_powered: true,
            auto_sleep: true,
            sleep_after_s: 2700,
            idle_s: 12,
            standby_s: 0,
            credit_s: 180,
            holds: vec![PmHold {
                owner: "usb.host".into(),
                floor: "usb".into(),
                age_ms: 3400,
            }],
        };
        let pm_proj = V070PmStateProjection::from_pm_state(&pm_state);
        assert_eq!(pm_proj.state, "active");
        assert!(pm_proj.usb_host_hold);

        let pm_json = serde_json::to_value(&pm_proj).unwrap();
        let pm_obj = pm_json.as_object().unwrap();
        assert!(pm_obj.get("inner").is_none());
        assert_eq!(pm_obj["state"], "active");
        assert_eq!(pm_obj["usb_host_hold"], true);
        let decoded_pm: V070PmStateProjection =
            serde_json::from_value(Value::Object(pm_obj.clone())).unwrap();
        assert_eq!(decoded_pm, pm_proj);

        assert_eq!(
            V070HardwareProjection::default().device_name.max_length,
            MAX_DEVICE_NAME_LEN
        );
    }

    #[test]
    fn parse_v070_action_splits_preferences_and_parses_device_commands_once() {
        let base_mouse = MouseConfig::default();
        assert_eq!(
            parse_v070_action(
                V070HardwareAction::SetAutoLayerSync { enabled: true },
                &base_mouse,
                MAX_DEVICE_NAME_LEN,
            ),
            Ok(ParsedV070Action::HostPreference(
                V070HostPreference::AutoLayerSync { enabled: true }
            ))
        );

        let parsed_sa = parse_v070_action(
            V070HardwareAction::ApplySaRgb {
                sa: 7057,
                color_hex: "0x33aaff".into(),
                brightness: 0.75,
                speed: 0.25,
                effect: 4,
            },
            &base_mouse,
            MAX_DEVICE_NAME_LEN,
        )
        .unwrap();
        match parsed_sa {
            ParsedV070Action::DeviceCommand(ValidatedV070DeviceCommand::ApplySaRgb {
                sa,
                rgb,
                effect,
                ..
            }) => {
                assert_eq!(sa.get(), 7057);
                assert_eq!(rgb, 0x33AAFF);
                assert_eq!(effect, SaRgbEffect::Breath);
            }
            other => panic!("unexpected parsed action: {other:?}"),
        }

        assert!(parse_v070_action(
            V070HardwareAction::ApplySaRgb {
                sa: 0,
                color_hex: "#33AAFF".into(),
                brightness: 0.5,
                speed: 0.5,
                effect: 1,
            },
            &base_mouse,
            MAX_DEVICE_NAME_LEN,
        )
        .is_err());

        assert!(parse_v070_action(
            V070HardwareAction::ApplyMouseConfig {
                move_delta: 0,
                max_speed: 20,
                wheel_delta: 2,
                wheel_max_speed: 10,
                stick_speed: 4,
                stick_scroll: 80,
            },
            &base_mouse,
            MAX_DEVICE_NAME_LEN,
        )
        .is_err());

        assert!(parse_v070_action(
            V070HardwareAction::ApplyMouseConfig {
                move_delta: 128,
                max_speed: 20,
                wheel_delta: 2,
                wheel_max_speed: 10,
                stick_speed: 4,
                stick_scroll: 80,
            },
            &base_mouse,
            MAX_DEVICE_NAME_LEN,
        )
        .is_err());

        let parsed_mouse = parse_v070_action(
            V070HardwareAction::ApplyMouseConfig {
                move_delta: 12,
                max_speed: 100,
                wheel_delta: 127,
                wheel_max_speed: 10,
                stick_speed: 4,
                stick_scroll: 255,
            },
            &base_mouse,
            MAX_DEVICE_NAME_LEN,
        )
        .unwrap();
        assert!(matches!(
            parsed_mouse,
            ParsedV070Action::DeviceCommand(ValidatedV070DeviceCommand::ApplyMouseConfig { .. })
        ));

        assert!(parse_v070_action(
            V070HardwareAction::ApplyJoystickDeadzone { deadzone: 0.0024 },
            &base_mouse,
            MAX_DEVICE_NAME_LEN,
        )
        .is_err());
        assert!(parse_v070_action(
            V070HardwareAction::ApplyJoystickDeadzone { deadzone: 0.96 },
            &base_mouse,
            MAX_DEVICE_NAME_LEN,
        )
        .is_err());
        let parsed_dz = parse_v070_action(
            V070HardwareAction::ApplyJoystickDeadzone { deadzone: 0.25 },
            &base_mouse,
            MAX_DEVICE_NAME_LEN,
        )
        .unwrap();
        assert_eq!(
            parsed_dz,
            ParsedV070Action::DeviceCommand(ValidatedV070DeviceCommand::ApplyJoystickDeadzone {
                deadzone: JoystickDeadzone { v: 0.25 }
            })
        );
    }
}
