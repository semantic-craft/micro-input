//! Validated `0.7.0-rc.16+` hardware capability codecs (candidate-only; never read-only exit-chain).
//!
//! All requests target firmware RPC methods exposed only over an exclusive
//! macOS 267-byte numbered HID vendor link on verified `>= 0.7.0-rc.16` firmware.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{alias, ProtoError};

pub const MOUSE_CONFIG_METHOD: &str = "mouse.config";
pub const JOYSTICK_DEADZONE_METHOD: &str = "joystick.deadzone";
pub const PROFILE_LAYER_SET_METHOD: &str = "kb.pl.set";
pub const PROFILE_LAYER_NOTIFY_METHOD: &str = "kb.pl";
pub const PM_CONFIG_METHOD: &str = "pm.config";
pub const PM_STATE_METHOD: &str = "pm.state";
pub const DEVICE_NAME_METHOD: &str = "device.name";
pub const SA_RGB_METHOD: &str = "kb.sa.rgb";

pub const MAX_DEVICE_NAME_LEN: usize = 15;
pub const SA_RGB_SUPPORTED_EFFECT_MASK: u8 = 0x5b;

#[derive(Serialize)]
struct CandidateRequest<'a, P: Serialize> {
    id: u16,
    method: &'a str,
    params: &'a P,
}

fn encode_v070_request<P: Serialize>(
    method: &str,
    params: &P,
    id: u16,
) -> Result<Vec<u8>, ProtoError> {
    if id >= 999 {
        return Err(ProtoError::Invalid("id must be < 999".into()));
    }
    serde_json::to_vec(&CandidateRequest { id, method, params })
        .map_err(|error| ProtoError::Invalid(error.to_string()))
}

fn validate_v070_response<'a>(
    reply: &'a Value,
    expected_id: u16,
    expected_method: &str,
) -> Result<&'a Value, ProtoError> {
    if !reply.is_object() || expected_id >= 999 {
        return Err(ProtoError::Invalid(format!(
            "invalid {expected_method} reply envelope"
        )));
    }
    if alias(reply, "id", "i")?.and_then(Value::as_u64) != Some(u64::from(expected_id)) {
        return Err(ProtoError::Invalid(format!(
            "mismatched reply id for {expected_method}"
        )));
    }
    if let Some(method_value) = alias(reply, "method", "m")? {
        let Some(method) = method_value.as_str() else {
            return Err(ProtoError::Invalid(format!(
                "non-string reply method for {expected_method}"
            )));
        };
        if method != expected_method {
            return Err(ProtoError::Invalid(format!(
                "unexpected reply method {method} for {expected_method}"
            )));
        }
    }
    if let Some(error) = reply.get("error").filter(|v| !v.is_null()) {
        let detail = error
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| error.to_string());
        return Err(ProtoError::Invalid(format!(
            "{expected_method} failed: {detail}"
        )));
    }
    reply
        .get("result")
        .ok_or_else(|| ProtoError::Invalid(format!("missing result in {expected_method} reply")))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseSpeedPreset {
    Slow,
    Standard,
    Fast,
}

impl Default for MouseSpeedPreset {
    fn default() -> Self {
        Self::Standard
    }
}

impl MouseSpeedPreset {
    pub fn parse(raw: &str) -> Result<Self, ProtoError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "slow" => Ok(Self::Slow),
            "standard" | "default" => Ok(Self::Standard),
            "fast" => Ok(Self::Fast),
            other => Err(ProtoError::Invalid(format!(
                "unknown mouse speed preset: {other}"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Slow => "slow",
            Self::Standard => "standard",
            Self::Fast => "fast",
        }
    }

    pub fn to_mouse_config(self) -> MouseConfig {
        match self {
            Self::Slow => MouseConfig {
                delay_ms: 120,
                interval_ms: 24,
                move_delta: 4,
                max_speed: 6,
                time_to_max: 35,
                wheel_delay_ms: 120,
                wheel_interval_ms: 90,
                wheel_delta: 1,
                wheel_max_speed: 4,
                wheel_time_to_max: 45,
                stick_speed: 1,
                stick_regulator: 25,
                stick_scroll: 60,
            },
            Self::Standard => MouseConfig {
                delay_ms: 100,
                interval_ms: 20,
                move_delta: 8,
                max_speed: 10,
                time_to_max: 30,
                wheel_delay_ms: 100,
                wheel_interval_ms: 80,
                wheel_delta: 1,
                wheel_max_speed: 8,
                wheel_time_to_max: 40,
                stick_speed: 2,
                stick_regulator: 20,
                stick_scroll: 40,
            },
            Self::Fast => MouseConfig {
                delay_ms: 80,
                interval_ms: 16,
                move_delta: 14,
                max_speed: 18,
                time_to_max: 24,
                wheel_delay_ms: 80,
                wheel_interval_ms: 60,
                wheel_delta: 2,
                wheel_max_speed: 14,
                wheel_time_to_max: 30,
                stick_speed: 4,
                stick_regulator: 16,
                stick_scroll: 25,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MouseConfig {
    pub delay_ms: u16,
    pub interval_ms: u16,
    pub move_delta: u16,
    pub max_speed: u16,
    pub time_to_max: u16,
    pub wheel_delay_ms: u16,
    pub wheel_interval_ms: u16,
    pub wheel_delta: u16,
    pub wheel_max_speed: u16,
    pub wheel_time_to_max: u16,
    pub stick_speed: u16,
    pub stick_regulator: u16,
    pub stick_scroll: u16,
}

impl Default for MouseConfig {
    fn default() -> Self {
        MouseSpeedPreset::Standard.to_mouse_config()
    }
}

impl MouseConfig {
    pub fn validate(&self) -> Result<(), ProtoError> {
        let checks = [
            ("delay_ms", self.delay_ms, 1..=5000),
            ("interval_ms", self.interval_ms, 1..=5000),
            ("move_delta", self.move_delta, 1..=255),
            ("max_speed", self.max_speed, 1..=255),
            ("time_to_max", self.time_to_max, 1..=2000),
            ("wheel_delay_ms", self.wheel_delay_ms, 1..=5000),
            ("wheel_interval_ms", self.wheel_interval_ms, 1..=5000),
            ("wheel_delta", self.wheel_delta, 1..=255),
            ("wheel_max_speed", self.wheel_max_speed, 1..=255),
            ("wheel_time_to_max", self.wheel_time_to_max, 1..=2000),
            ("stick_speed", self.stick_speed, 1..=255),
            ("stick_regulator", self.stick_regulator, 1..=2000),
            ("stick_scroll", self.stick_scroll, 1..=2000),
        ];
        for (name, val, range) in checks {
            if !range.contains(&val) {
                return Err(ProtoError::Invalid(format!(
                    "mouse.config.{name} ({val}) must be in {}..={}",
                    range.start(),
                    range.end()
                )));
            }
        }
        Ok(())
    }

    pub fn matching_preset(&self) -> Option<MouseSpeedPreset> {
        [
            MouseSpeedPreset::Slow,
            MouseSpeedPreset::Standard,
            MouseSpeedPreset::Fast,
        ]
        .into_iter()
        .find(|preset| preset.to_mouse_config() == *self)
    }
}

pub fn encode_read_mouse_config(id: u16) -> Result<Vec<u8>, ProtoError> {
    encode_v070_request(MOUSE_CONFIG_METHOD, &Value::Null, id)
}

pub fn encode_write_mouse_config(cfg: &MouseConfig, id: u16) -> Result<Vec<u8>, ProtoError> {
    cfg.validate()?;
    encode_v070_request(MOUSE_CONFIG_METHOD, cfg, id)
}

pub fn decode_mouse_config_response(
    reply: &Value,
    expected_id: u16,
) -> Result<MouseConfig, ProtoError> {
    let result = validate_v070_response(reply, expected_id, MOUSE_CONFIG_METHOD)?;
    let cfg: MouseConfig = serde_json::from_value(result.clone())
        .map_err(|error| ProtoError::Invalid(format!("invalid mouse.config result: {error}")))?;
    cfg.validate()?;
    Ok(cfg)
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct JoystickDeadzone {
    pub v: f64,
}

impl Default for JoystickDeadzone {
    fn default() -> Self {
        Self { v: Self::DEFAULT }
    }
}

impl JoystickDeadzone {
    pub const MIN: f64 = 0.0025;
    pub const MAX: f64 = 0.95;
    pub const UI_MIN: f64 = 0.02;
    pub const UI_MAX: f64 = 0.45;
    pub const DEFAULT: f64 = 0.15;

    pub fn new(v: f64) -> Result<Self, ProtoError> {
        let dz = Self { v };
        dz.validate()?;
        Ok(dz)
    }

    pub fn validate(&self) -> Result<(), ProtoError> {
        if !self.v.is_finite() || !(Self::MIN..=Self::MAX).contains(&self.v) {
            return Err(ProtoError::Invalid(format!(
                "joystick.deadzone v ({}) must be finite in {}..={}",
                self.v,
                Self::MIN,
                Self::MAX
            )));
        }
        Ok(())
    }
}

pub fn encode_read_joystick_deadzone(id: u16) -> Result<Vec<u8>, ProtoError> {
    encode_v070_request(JOYSTICK_DEADZONE_METHOD, &Value::Null, id)
}

pub fn encode_write_joystick_deadzone(
    dz: JoystickDeadzone,
    id: u16,
) -> Result<Vec<u8>, ProtoError> {
    dz.validate()?;
    encode_v070_request(JOYSTICK_DEADZONE_METHOD, &dz, id)
}

pub fn decode_joystick_deadzone_response(
    reply: &Value,
    expected_id: u16,
) -> Result<JoystickDeadzone, ProtoError> {
    let result = validate_v070_response(reply, expected_id, JOYSTICK_DEADZONE_METHOD)?;
    let dz: JoystickDeadzone = serde_json::from_value(result.clone()).map_err(|error| {
        ProtoError::Invalid(format!("invalid joystick.deadzone result: {error}"))
    })?;
    dz.validate()?;
    Ok(dz)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileLayerTarget {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub l: Option<u8>,
}

impl ProfileLayerTarget {
    pub fn validate(&self) -> Result<(), ProtoError> {
        if self.p.is_none() && self.l.is_none() {
            return Err(ProtoError::Invalid("missing p and l".into()));
        }
        Ok(())
    }
}

pub fn encode_set_profile_layer(
    target: ProfileLayerTarget,
    id: u16,
) -> Result<Vec<u8>, ProtoError> {
    target.validate()?;
    encode_v070_request(PROFILE_LAYER_SET_METHOD, &target, id)
}

pub fn decode_set_profile_layer_response(
    reply: &Value,
    expected_id: u16,
) -> Result<(), ProtoError> {
    let result = validate_v070_response(reply, expected_id, PROFILE_LAYER_SET_METHOD)?;
    if !result.is_object() {
        return Err(ProtoError::Invalid(
            "kb.pl.set result must be an object".into(),
        ));
    }
    Ok(())
}

/// Parses a `kb.pl` notification (`{"m":"kb.pl","p":{"p":3,"l":1}}` or long keys).
pub fn parse_profile_layer_notification(raw: &Value) -> Option<(u8, u8)> {
    let method = alias(raw, "method", "m").ok().flatten()?.as_str()?;
    if method != PROFILE_LAYER_NOTIFY_METHOD {
        return None;
    }
    let params = alias(raw, "params", "p").ok().flatten()?;
    let p = params
        .get("p")
        .and_then(Value::as_u64)
        .and_then(|v| u8::try_from(v).ok())?;
    let l = params
        .get("l")
        .and_then(Value::as_u64)
        .and_then(|v| u8::try_from(v).ok())?;
    Some((p, l))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PmConfig {
    pub auto_sleep: bool,
    pub host_sleep: bool,
    pub sleep_after_min: u16,
}

impl Default for PmConfig {
    fn default() -> Self {
        Self {
            auto_sleep: true,
            host_sleep: true,
            sleep_after_min: 15,
        }
    }
}

impl PmConfig {
    pub fn validate(&self) -> Result<(), ProtoError> {
        if !(1..=1440).contains(&self.sleep_after_min) {
            return Err(ProtoError::Invalid(format!(
                "pm.config.sleep_after_min ({}) must be in 1..=1440",
                self.sleep_after_min
            )));
        }
        Ok(())
    }
}

pub fn encode_read_pm_config(id: u16) -> Result<Vec<u8>, ProtoError> {
    encode_v070_request(PM_CONFIG_METHOD, &Value::Null, id)
}

pub fn encode_write_pm_config(cfg: &PmConfig, id: u16) -> Result<Vec<u8>, ProtoError> {
    cfg.validate()?;
    encode_v070_request(PM_CONFIG_METHOD, cfg, id)
}

pub fn decode_pm_config_response(reply: &Value, expected_id: u16) -> Result<PmConfig, ProtoError> {
    let result = validate_v070_response(reply, expected_id, PM_CONFIG_METHOD)?;
    let cfg: PmConfig = serde_json::from_value(result.clone())
        .map_err(|error| ProtoError::Invalid(format!("invalid pm.config result: {error}")))?;
    cfg.validate()?;
    Ok(cfg)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PmHold {
    pub owner: String,
    pub floor: String,
    pub age_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PmState {
    pub state: String,
    pub floor: String,
    pub base_floor: String,
    pub line_powered: bool,
    pub auto_sleep: bool,
    pub sleep_after_s: u32,
    pub idle_s: u32,
    pub standby_s: u32,
    pub credit_s: u32,
    #[serde(default)]
    pub holds: Vec<PmHold>,
}

impl PmState {
    pub fn usb_host_hold(&self) -> Option<&PmHold> {
        self.holds.iter().find(|h| h.owner == "usb.host")
    }
}

pub fn encode_read_pm_state(id: u16) -> Result<Vec<u8>, ProtoError> {
    encode_v070_request(PM_STATE_METHOD, &Value::Null, id)
}

pub fn decode_pm_state_response(reply: &Value, expected_id: u16) -> Result<PmState, ProtoError> {
    let result = validate_v070_response(reply, expected_id, PM_STATE_METHOD)?;
    serde_json::from_value(result.clone())
        .map_err(|error| ProtoError::Invalid(format!("invalid pm.state result: {error}")))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceNameInfo {
    pub name: String,
    pub factory_name: String,
    pub max_length: usize,
}

pub fn validate_device_name(name: &str, max_length: usize) -> Result<String, ProtoError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ProtoError::Invalid("device name cannot be empty".into()));
    }
    let limit = max_length.min(MAX_DEVICE_NAME_LEN);
    if limit == 0 || trimmed.len() > limit {
        return Err(ProtoError::Invalid(format!(
            "device name UTF-8 byte length {} exceeds max_length {limit}",
            trimmed.len()
        )));
    }
    Ok(trimmed.to_string())
}

pub fn encode_read_device_name(id: u16) -> Result<Vec<u8>, ProtoError> {
    encode_v070_request(DEVICE_NAME_METHOD, &Value::Null, id)
}

#[derive(Serialize)]
struct DeviceNameWriteParams<'a> {
    name: &'a str,
}

pub fn encode_set_device_name(
    name: &str,
    max_length: usize,
    id: u16,
) -> Result<Vec<u8>, ProtoError> {
    let validated = validate_device_name(name, max_length)?;
    encode_v070_request(
        DEVICE_NAME_METHOD,
        &DeviceNameWriteParams { name: &validated },
        id,
    )
}

pub fn decode_device_name_response(
    reply: &Value,
    expected_id: u16,
) -> Result<DeviceNameInfo, ProtoError> {
    let result = validate_v070_response(reply, expected_id, DEVICE_NAME_METHOD)?;
    serde_json::from_value(result.clone())
        .map_err(|error| ProtoError::Invalid(format!("invalid device.name result: {error}")))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum SaRgbEffect {
    Off = 0,
    Solid = 1,
    Rainbow = 3,
    Breath = 4,
    ShallowBreath = 6,
}

impl SaRgbEffect {
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    pub fn from_u8(raw: u8) -> Result<Self, ProtoError> {
        if raw < 8 && (SA_RGB_SUPPORTED_EFFECT_MASK & (1 << raw)) != 0 {
            return match raw {
                0 => Ok(Self::Off),
                1 => Ok(Self::Solid),
                3 => Ok(Self::Rainbow),
                4 => Ok(Self::Breath),
                6 => Ok(Self::ShallowBreath),
                _ => unreachable!(),
            };
        }
        Err(ProtoError::Invalid(format!(
            "unsupported kb.sa.rgb effect id: {raw}"
        )))
    }
}

impl TryFrom<u8> for SaRgbEffect {
    type Error = ProtoError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::from_u8(value)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SaRgbUpdate {
    pub sa: Vec<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub c: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub b: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub s: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub e: Option<u8>,
}

impl SaRgbUpdate {
    pub fn reset(sa: Vec<u32>) -> Self {
        Self {
            sa,
            c: None,
            b: None,
            s: None,
            e: Some(SaRgbEffect::Off.as_u8()),
        }
    }

    pub fn validate(&self) -> Result<(), ProtoError> {
        if self.sa.is_empty() {
            return Err(ProtoError::Invalid("missing or empty sa".into()));
        }
        for &sa_id in &self.sa {
            if sa_id == 0 || sa_id > 0xFF_FFFF {
                return Err(ProtoError::Invalid(format!(
                    "sa id {sa_id} must be in 1..=0xFFFFFF"
                )));
            }
        }
        if let Some(c) = self.c {
            if c > 0xFF_FFFF {
                return Err(ProtoError::Invalid(format!(
                    "kb.sa.rgb color {c} must be in 0..=0xFFFFFF"
                )));
            }
        }
        if let Some(b) = self.b {
            if !b.is_finite() || !(0.0..=1.0).contains(&b) {
                return Err(ProtoError::Invalid(format!(
                    "kb.sa.rgb brightness {b} must be finite in 0.0..=1.0"
                )));
            }
        }
        if let Some(s) = self.s {
            if !s.is_finite() || !(0.0..=1.0).contains(&s) {
                return Err(ProtoError::Invalid(format!(
                    "kb.sa.rgb speed {s} must be finite in 0.0..=1.0"
                )));
            }
        }
        if let Some(e) = self.e {
            SaRgbEffect::from_u8(e)?;
        }
        if self.e == Some(SaRgbEffect::Off.as_u8())
            && (self.c.is_some() || self.b.is_some() || self.s.is_some())
        {
            return Err(ProtoError::Invalid("reset takes no other value".into()));
        }
        if self.c.is_none() && self.b.is_none() && self.s.is_none() && self.e.is_none() {
            return Err(ProtoError::Invalid("empty update".into()));
        }
        Ok(())
    }
}

pub fn encode_set_sa_rgb(update: &SaRgbUpdate, id: u16) -> Result<Vec<u8>, ProtoError> {
    update.validate()?;
    encode_v070_request(SA_RGB_METHOD, update, id)
}

pub fn decode_set_sa_rgb_response(reply: &Value, expected_id: u16) -> Result<(), ProtoError> {
    validate_v070_response(reply, expected_id, SA_RGB_METHOD)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        validate_read_only, Framer, HostNotification, Incoming, JsonCodec, Method, NativeFramer,
        RpcCodec, RpcRequest,
    };
    use serde_json::json;

    #[test]
    fn read_only_exit_boundary_still_rejects_v070_candidates() {
        let frames = [
            encode_write_mouse_config(&MouseConfig::default(), 1).unwrap(),
            encode_write_joystick_deadzone(JoystickDeadzone::default(), 2).unwrap(),
            encode_set_profile_layer(
                ProfileLayerTarget {
                    p: Some(3),
                    l: Some(1),
                },
                3,
            )
            .unwrap(),
            encode_write_pm_config(&PmConfig::default(), 4).unwrap(),
            encode_read_pm_state(5).unwrap(),
            encode_set_device_name("Micro-Desk", 15, 6).unwrap(),
            encode_set_sa_rgb(&SaRgbUpdate::reset(vec![7057]), 7).unwrap(),
        ];
        for bytes in frames {
            let v: Value = serde_json::from_slice(&bytes).unwrap();
            if !v["params"].is_null() {
                let borrowed =
                    RpcRequest::new(Method::DeviceStatus, v["params"].clone(), 1).unwrap();
                assert!(validate_read_only(&borrowed).is_err());
            }
        }
        let read_only = RpcRequest::new(Method::DeviceStatus, Value::Null, 1).unwrap();
        assert!(validate_read_only(&read_only).is_ok());
        let encoded = JsonCodec.encode(&read_only).unwrap();
        assert!(NativeFramer::default().encode(&encoded).is_ok());
    }

    #[test]
    fn mouse_config_presets_and_validation_match_firmware_defaults() {
        let standard = MouseConfig::default();
        assert_eq!(standard.delay_ms, 100);
        assert_eq!(standard.interval_ms, 20);
        assert_eq!(standard.move_delta, 8);
        assert_eq!(standard.max_speed, 10);
        assert_eq!(standard.time_to_max, 30);
        assert_eq!(standard.wheel_delay_ms, 100);
        assert_eq!(standard.wheel_interval_ms, 80);
        assert_eq!(standard.wheel_delta, 1);
        assert_eq!(standard.wheel_max_speed, 8);
        assert_eq!(standard.wheel_time_to_max, 40);
        assert_eq!(standard.stick_speed, 2);
        assert_eq!(standard.stick_regulator, 20);
        assert_eq!(standard.stick_scroll, 40);
        assert_eq!(standard.matching_preset(), Some(MouseSpeedPreset::Standard));
        assert_eq!(
            MouseSpeedPreset::parse("FAST").unwrap(),
            MouseSpeedPreset::Fast
        );
        assert!(MouseSpeedPreset::parse("turbo").is_err());

        let wire = encode_write_mouse_config(&standard, 10).unwrap();
        let v: Value = serde_json::from_slice(&wire).unwrap();
        assert_eq!(v["method"], "mouse.config");
        assert_eq!(v["params"]["stick_scroll"], 40);

        let reply = json!({"id": 10, "method": "mouse.config", "result": v["params"]});
        assert_eq!(decode_mouse_config_response(&reply, 10).unwrap(), standard);

        let mut bad = standard;
        bad.move_delta = 0;
        assert!(encode_write_mouse_config(&bad, 11).is_err());
        assert!(encode_write_mouse_config(&standard, 999).is_err());
    }

    #[test]
    fn joystick_deadzone_enforces_firmware_range_and_finiteness() {
        assert!(encode_write_joystick_deadzone(JoystickDeadzone { v: 0.0025 }, 1).is_ok());
        assert!(encode_write_joystick_deadzone(JoystickDeadzone { v: 0.95 }, 2).is_ok());
        assert!(encode_write_joystick_deadzone(JoystickDeadzone { v: 0.0024 }, 3).is_err());
        assert!(encode_write_joystick_deadzone(JoystickDeadzone { v: 0.96 }, 4).is_err());
        assert!(encode_write_joystick_deadzone(JoystickDeadzone { v: f64::NAN }, 5).is_err());

        let reply = json!({"id": 2, "method": "joystick.deadzone", "result": {"v": 0.15}});
        assert_eq!(
            decode_joystick_deadzone_response(&reply, 2).unwrap().v,
            0.15
        );
    }

    #[test]
    fn profile_layer_target_and_kb_pl_notification_parse_cleanly() {
        assert!(encode_set_profile_layer(ProfileLayerTarget { p: None, l: None }, 1).is_err());
        let wire = encode_set_profile_layer(
            ProfileLayerTarget {
                p: None,
                l: Some(2),
            },
            7,
        )
        .unwrap();
        let v: Value = serde_json::from_slice(&wire).unwrap();
        assert_eq!(v["method"], "kb.pl.set");
        assert_eq!(v["params"], json!({"l": 2}));
        assert!(decode_set_profile_layer_response(
            &json!({"id": 7, "method": "kb.pl.set", "result": {}}),
            7
        )
        .is_ok());

        let raw_notify = br#"{"m":"kb.pl","p":{"p":3,"l":1}}"#;
        let Incoming::Notification(notif) = JsonCodec.decode(raw_notify).unwrap() else {
            panic!("expected notification");
        };
        match HostNotification::from(notif) {
            HostNotification::ProfileLayer(v) => {
                assert_eq!(parse_profile_layer_notification(&v), Some((3, 1)));
            }
            other => panic!("expected ProfileLayer notification, got {other:?}"),
        }
    }

    #[test]
    fn pm_config_pm_state_and_device_name_round_trip_and_validate() {
        let mut pm = PmConfig::default();
        pm.sleep_after_min = 0;
        assert!(encode_write_pm_config(&pm, 1).is_err());
        pm.sleep_after_min = 1441;
        assert!(encode_write_pm_config(&pm, 1).is_err());
        pm.sleep_after_min = 30;
        assert!(encode_write_pm_config(&pm, 1).is_ok());

        let pm_state_reply = json!({
            "id": 2,
            "method": "pm.state",
            "result": {
                "auto_sleep": true,
                "base_floor": "host_sleep",
                "credit_s": 0,
                "floor": "usb",
                "holds": [{"age_ms": 9626, "floor": "usb", "owner": "usb.host"}],
                "idle_s": 9,
                "line_powered": true,
                "sleep_after_s": 900,
                "standby_s": 3600,
                "state": "active"
            }
        });
        let state = decode_pm_state_response(&pm_state_reply, 2).unwrap();
        assert_eq!(state.usb_host_hold().map(|h| h.floor.as_str()), Some("usb"));

        assert!(encode_set_device_name("   ", 15, 3).is_err());
        assert!(encode_set_device_name("1234567890123456", 15, 3).is_err());
        assert!(encode_set_device_name("1234567890", 8, 3).is_err());
        let name_req = encode_set_device_name("  MicroDesk  ", 15, 3).unwrap();
        let name_v: Value = serde_json::from_slice(&name_req).unwrap();
        assert_eq!(name_v["params"]["name"], "MicroDesk");

        let name_reply = json!({
            "id": 4,
            "method": "device.name",
            "result": {"factory_name": "Creator Micro 2", "max_length": 15, "name": "MicroDesk"}
        });
        let info = decode_device_name_response(&name_reply, 4).unwrap();
        assert_eq!(info.max_length, 15);
        assert_eq!(info.name, "MicroDesk");
    }

    #[test]
    fn sa_rgb_validates_mask_bounds_reset_and_empty_updates() {
        for e in [0u8, 1, 3, 4, 6] {
            assert!(SaRgbEffect::from_u8(e).is_ok());
        }
        for e in [2u8, 5, 7, 9] {
            assert!(SaRgbEffect::from_u8(e).is_err());
        }

        let reset = SaRgbUpdate::reset(vec![7057, 7058]);
        assert!(encode_set_sa_rgb(&reset, 1).is_ok());
        let reset_with_color = SaRgbUpdate {
            c: Some(0xFF0000),
            ..SaRgbUpdate::reset(vec![7057])
        };
        assert!(encode_set_sa_rgb(&reset_with_color, 2).is_err());

        let empty = SaRgbUpdate {
            sa: vec![7057],
            c: None,
            b: None,
            s: None,
            e: None,
        };
        assert!(encode_set_sa_rgb(&empty, 3).is_err());

        let no_sa = SaRgbUpdate::reset(vec![]);
        assert!(encode_set_sa_rgb(&no_sa, 4).is_err());

        let bad_b = SaRgbUpdate {
            sa: vec![7057],
            c: Some(0x112233),
            b: Some(1.2),
            s: None,
            e: Some(1),
        };
        assert!(encode_set_sa_rgb(&bad_b, 5).is_err());

        let valid = SaRgbUpdate {
            sa: vec![7057],
            c: Some(0x33AAFF),
            b: Some(0.8),
            s: Some(0.5),
            e: Some(SaRgbEffect::Breath.as_u8()),
        };
        let wire = encode_set_sa_rgb(&valid, 6).unwrap();
        let v: Value = serde_json::from_slice(&wire).unwrap();
        assert_eq!(v["method"], "kb.sa.rgb");
        assert_eq!(v["params"]["e"], 4);
    }

    #[test]
    fn json_codec_allows_v070_method_echoes_and_rejects_rpc_errors() {
        for method in [
            "mouse.config",
            "joystick.deadzone",
            "kb.pl.set",
            "pm.config",
            "pm.state",
            "device.name",
            "kb.sa.rgb",
        ] {
            let wire = json!({"id": 1, "method": method, "result": {}}).to_string();
            assert!(JsonCodec.decode(wire.as_bytes()).is_ok());
        }

        let err_reply = json!({
            "id": 1,
            "method": "kb.pl.set",
            "error": {"code": -32602, "message": "missing p and l"}
        });
        assert!(decode_set_profile_layer_response(&err_reply, 1).is_err());
    }
}
