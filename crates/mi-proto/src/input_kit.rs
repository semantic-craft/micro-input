use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{json, Value};

use crate::v070_capabilities::{SaRgbEffect, SaRgbUpdate};
use crate::ProtoError;

pub const INPUT_KIT_DEFAULT_PORT: u16 = 19147;
pub const INPUT_KIT_WS_PATH: &str = "/input";
pub const MAX_INPUT_KIT_FRAME_BYTES: usize = 512 * 1024;
pub const INPUT_KIT_DEVICE_RATE_LIMIT_CODE: i32 = -32017;
pub const INPUT_KIT_WS_RATE_LIMIT_CLOSE_CODE: u16 = 1008;
pub const INPUT_KIT_WS_BINARY_CLOSE_CODE: u16 = 4003;
pub const INPUT_KIT_HANDSHAKE_TIMEOUT_MS: u64 = 5_000;
pub const INPUT_KIT_UNAUTH_TIMEOUT_MS: u64 = 30_000;
pub const INPUT_KIT_AUTH_TIMEOUT_MS: u64 = 29_000;
pub const INPUT_KIT_REQUEST_TIMEOUT_MS: u64 = 10_000;
pub const INPUT_KIT_ADD_LAYER_TIMEOUT_MS: u64 = 120_000;
pub const INPUT_KIT_MAX_APPLICATION_NAME_CHARS: usize = 80;
pub const INPUT_KIT_MAX_IDENTIFIER_UTF16_LEN: usize = 128;
pub const INPUT_KIT_MAX_IDENTIFIER_PATTERN_UTF16_LEN: usize = 256;
pub const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LoopbackHost {
    #[serde(rename = "127.0.0.1")]
    Ipv4Loopback,
    #[serde(rename = "::1")]
    Ipv6Loopback,
    #[serde(rename = "localhost")]
    Localhost,
}

impl LoopbackHost {
    pub fn parse(raw: &str) -> Result<Self, ProtoError> {
        match raw {
            "127.0.0.1" => Ok(Self::Ipv4Loopback),
            "::1" => Ok(Self::Ipv6Loopback),
            "localhost" => Ok(Self::Localhost),
            other => Err(ProtoError::Invalid(format!(
                "unsupported non-loopback host: {other}"
            ))),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ipv4Loopback => "127.0.0.1",
            Self::Ipv6Loopback => "::1",
            Self::Localhost => "localhost",
        }
    }

    pub fn ws_url(self, port: u16) -> Result<String, ProtoError> {
        if port == 0 {
            return Err(ProtoError::Invalid(
                "port must be an integer between 1 and 65535".into(),
            ));
        }
        let authority = match self {
            Self::Ipv4Loopback => "127.0.0.1",
            Self::Ipv6Loopback => "[::1]",
            Self::Localhost => "localhost",
        };
        Ok(format!("ws://{authority}:{port}{INPUT_KIT_WS_PATH}"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum InputKitErrorCode {
    InvalidRequest = -32600,
    MethodNotFound = -32601,
    InvalidParams = -32602,
    InternalError = -32603,
    DeviceRateLimited = INPUT_KIT_DEVICE_RATE_LIMIT_CODE,
}

impl InputKitErrorCode {
    pub const fn code(self) -> i32 {
        self as i32
    }

    pub const fn from_code(code: i32) -> Option<Self> {
        match code {
            -32600 => Some(Self::InvalidRequest),
            -32601 => Some(Self::MethodNotFound),
            -32602 => Some(Self::InvalidParams),
            -32603 => Some(Self::InternalError),
            INPUT_KIT_DEVICE_RATE_LIMIT_CODE => Some(Self::DeviceRateLimited),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum InputKitRequestId {
    Number(u64),
    String(String),
}

impl From<u64> for InputKitRequestId {
    fn from(value: u64) -> Self {
        Self::Number(value)
    }
}

impl From<String> for InputKitRequestId {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for InputKitRequestId {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl InputKitRequestId {
    pub fn validate(&self) -> Result<(), ProtoError> {
        match self {
            Self::Number(n) if *n <= MAX_SAFE_INTEGER => Ok(()),
            Self::Number(n) => Err(ProtoError::Invalid(format!(
                "invalid request id {n}: exceeds safe integer range"
            ))),
            Self::String(s) if !s.is_empty() => Ok(()),
            Self::String(_) => Err(ProtoError::Invalid(
                "invalid request id: string id must not be empty".into(),
            )),
        }
    }
}

const fn is_unicode_format_char(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{0600}'..='\u{0605}'
            | '\u{061C}'
            | '\u{06DD}'
            | '\u{070F}'
            | '\u{0890}'..='\u{0891}'
            | '\u{08E2}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{110BD}'
            | '\u{110CD}'
            | '\u{13430}'..='\u{1343F}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0001}'
            | '\u{E0020}'..='\u{E007F}'
    )
}

const fn is_disallowed_application_name_char(c: char) -> bool {
    c.is_control() || is_unicode_format_char(c) || matches!(c, '\u{2028}' | '\u{2029}')
}

const fn is_disallowed_identifier_char(c: char) -> bool {
    matches!(
        c,
        '\u{0000}'..='\u{001F}' | '\u{007F}' | '\u{2028}' | '\u{2029}'
    )
}

pub fn is_positive_safe_integer(value: u64) -> bool {
    (1..=MAX_SAFE_INTEGER).contains(&value)
}

pub fn validate_device_id(device_id: u64) -> Result<(), ProtoError> {
    if !is_positive_safe_integer(device_id) {
        return Err(ProtoError::Invalid(
            "deviceId must be a positive safe integer".into(),
        ));
    }
    Ok(())
}

pub fn validate_profile_index(profile_index: u64) -> Result<(), ProtoError> {
    if profile_index > MAX_SAFE_INTEGER {
        return Err(ProtoError::Invalid(
            "profileIndex must be a non-negative safe integer".into(),
        ));
    }
    Ok(())
}

pub fn validate_layer_index(layer_index: u64) -> Result<(), ProtoError> {
    if !is_positive_safe_integer(layer_index) {
        return Err(ProtoError::Invalid(
            "layerIndex must be a positive safe integer (one-based)".into(),
        ));
    }
    Ok(())
}

pub fn validate_application_name(raw: &str) -> Result<String, ProtoError> {
    if raw.chars().any(is_disallowed_application_name_char) {
        return Err(ProtoError::Invalid(
            "applicationName must be 1 to 80 printable characters".into(),
        ));
    }
    let trimmed = raw.trim();
    let char_count = trimmed.chars().count();
    let utf16_len = trimmed.encode_utf16().count();
    if char_count == 0
        || char_count > INPUT_KIT_MAX_APPLICATION_NAME_CHARS
        || utf16_len > INPUT_KIT_MAX_APPLICATION_NAME_CHARS
    {
        return Err(ProtoError::Invalid(
            "applicationName must be 1 to 80 printable characters".into(),
        ));
    }
    Ok(trimmed.to_owned())
}

pub fn validate_api_identifier(identifier: &str) -> Result<(), ProtoError> {
    let utf16_len = identifier.encode_utf16().count();
    if utf16_len == 0
        || utf16_len > INPUT_KIT_MAX_IDENTIFIER_UTF16_LEN
        || identifier.trim() != identifier
        || identifier.chars().any(is_disallowed_identifier_char)
    {
        return Err(ProtoError::Invalid(
            "identifier must be 1 to 128 UTF-16 code units without surrounding whitespace, ASCII control characters, or Unicode line and paragraph separators".into(),
        ));
    }
    Ok(())
}

pub fn validate_identifier_pattern(pattern: &str) -> Result<(), ProtoError> {
    let utf16_len = pattern.encode_utf16().count();
    if utf16_len == 0 || utf16_len > INPUT_KIT_MAX_IDENTIFIER_PATTERN_UTF16_LEN {
        return Err(ProtoError::Invalid(
            "identifier pattern must be 1 to 256 UTF-16 code units".into(),
        ));
    }
    Ok(())
}

mod option_sa_rgb_effect_u8 {
    use super::*;

    pub fn serialize<S>(effect: &Option<SaRgbEffect>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match effect {
            Some(e) => serializer.serialize_u8(e.as_u8()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<SaRgbEffect>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw: Option<u8> = Option::deserialize(deserializer)?;
        match raw {
            Some(value) => SaRgbEffect::from_u8(value)
                .map(Some)
                .map_err(serde::de::Error::custom),
            None => Ok(None),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestAuthorizationParams {
    pub application_name: String,
}

impl RequestAuthorizationParams {
    pub fn validate(&self) -> Result<(), ProtoError> {
        let normalized = validate_application_name(&self.application_name)?;
        if normalized != self.application_name {
            return Err(ProtoError::Invalid(
                "applicationName must not contain surrounding whitespace".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticateParams {
    pub application_id: String,
    pub token: String,
}

impl AuthenticateParams {
    pub fn validate(&self) -> Result<(), ProtoError> {
        if self.application_id.trim().is_empty() {
            return Err(ProtoError::Invalid(
                "applicationId must be a non-empty string".into(),
            ));
        }
        if self.token.trim().is_empty() {
            return Err(ProtoError::Invalid(
                "token must be a non-empty string".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetDeviceInfoParams {
    pub device_id: u64,
}

impl GetDeviceInfoParams {
    pub fn validate(&self) -> Result<(), ProtoError> {
        validate_device_id(self.device_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetActiveProfileLayerParams {
    pub device_id: u64,
    pub profile_index: u64,
    pub layer_index: u64,
}

impl SetActiveProfileLayerParams {
    pub fn validate(&self) -> Result<(), ProtoError> {
        validate_device_id(self.device_id)?;
        validate_profile_index(self.profile_index)?;
        validate_layer_index(self.layer_index)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetKeyLightingParams {
    pub device_id: u64,
    pub identifier: String,
    pub is_regex: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brightness: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "option_sa_rgb_effect_u8"
    )]
    pub effect: Option<SaRgbEffect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
}

impl SetKeyLightingParams {
    pub fn validate(&self) -> Result<(), ProtoError> {
        validate_device_id(self.device_id)?;
        if self.is_regex {
            validate_identifier_pattern(&self.identifier)?;
        } else {
            validate_api_identifier(&self.identifier)?;
        }
        if self.color.is_none()
            && self.brightness.is_none()
            && self.effect.is_none()
            && self.speed.is_none()
        {
            return Err(ProtoError::Invalid(
                "Provide at least one of color, brightness, effect, or speed".into(),
            ));
        }
        if let Some(color) = self.color {
            if color > 0xFF_FFFF {
                return Err(ProtoError::Invalid(
                    "color must be an integer between 0 and 0xFFFFFF".into(),
                ));
            }
        }
        if let Some(brightness) = self.brightness {
            if !brightness.is_finite() || !(0.0..=1.0).contains(&brightness) {
                return Err(ProtoError::Invalid(
                    "brightness must be a finite number between 0 and 1".into(),
                ));
            }
        }
        if let Some(speed) = self.speed {
            if !speed.is_finite() || !(0.0..=1.0).contains(&speed) {
                return Err(ProtoError::Invalid(
                    "speed must be a finite number between 0 and 1".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn to_sa_rgb_updates(
        &self,
        sa_ids: &[u32],
        chunk_size: usize,
    ) -> Result<Vec<SaRgbUpdate>, ProtoError> {
        self.validate()?;
        if chunk_size == 0 {
            return Err(ProtoError::Invalid("chunk_size must be at least 1".into()));
        }
        let mut updates = Vec::with_capacity(sa_ids.len().div_ceil(chunk_size));
        for chunk in sa_ids.chunks(chunk_size) {
            let update = if self.effect == Some(SaRgbEffect::Off) {
                SaRgbUpdate::reset(chunk.to_vec())
            } else {
                SaRgbUpdate {
                    sa: chunk.to_vec(),
                    c: self.color,
                    b: self.brightness.map(|v| v as f32),
                    s: self.speed.map(|v| v as f32),
                    e: self.effect.map(SaRgbEffect::as_u8),
                }
            };
            update.validate()?;
            updates.push(update);
        }
        Ok(updates)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddLayerParams {
    pub device_id: u64,
    pub profile_index: u64,
    pub definition: Value,
}

impl AddLayerParams {
    pub fn validate(&self) -> Result<(), ProtoError> {
        validate_device_id(self.device_id)?;
        validate_profile_index(self.profile_index)?;
        if !self.definition.is_object() {
            return Err(ProtoError::Invalid(
                "definition must be a JSON object".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputKitMethod {
    RequestAuthorization(RequestAuthorizationParams),
    Authenticate(AuthenticateParams),
    GetDevices,
    GetDeviceInfo(GetDeviceInfoParams),
    SetActiveProfileLayer(SetActiveProfileLayerParams),
    SetKeyLighting(SetKeyLightingParams),
    AddLayer(AddLayerParams),
}

impl InputKitMethod {
    pub const fn method_name(&self) -> &'static str {
        match self {
            Self::RequestAuthorization(_) => "requestAuthorization",
            Self::Authenticate(_) => "authenticate",
            Self::GetDevices => "getDevices",
            Self::GetDeviceInfo(_) => "getDeviceInfo",
            Self::SetActiveProfileLayer(_) => "setActiveProfileLayer",
            Self::SetKeyLighting(_) => "setKeyLighting",
            Self::AddLayer(_) => "addLayer",
        }
    }

    pub const fn is_device_write(&self) -> bool {
        matches!(
            self,
            Self::SetActiveProfileLayer(_) | Self::SetKeyLighting(_) | Self::AddLayer(_)
        )
    }

    pub const fn target_device_id(&self) -> Option<u64> {
        match self {
            Self::GetDeviceInfo(p) => Some(p.device_id),
            Self::SetActiveProfileLayer(p) => Some(p.device_id),
            Self::SetKeyLighting(p) => Some(p.device_id),
            Self::AddLayer(p) => Some(p.device_id),
            Self::RequestAuthorization(_) | Self::Authenticate(_) | Self::GetDevices => None,
        }
    }

    pub fn validate(&self) -> Result<(), ProtoError> {
        match self {
            Self::RequestAuthorization(p) => p.validate(),
            Self::Authenticate(p) => p.validate(),
            Self::GetDevices => Ok(()),
            Self::GetDeviceInfo(p) => p.validate(),
            Self::SetActiveProfileLayer(p) => p.validate(),
            Self::SetKeyLighting(p) => p.validate(),
            Self::AddLayer(p) => p.validate(),
        }
    }
}

fn require_object<'a>(
    value: &'a Value,
    label: &str,
) -> Result<&'a serde_json::Map<String, Value>, ProtoError> {
    value
        .as_object()
        .ok_or_else(|| ProtoError::Invalid(format!("{label} must be a JSON object")))
}

fn parse_request_id(value: &Value) -> Result<InputKitRequestId, ProtoError> {
    let id = match value {
        Value::Number(n) => {
            let Some(u) = n.as_u64() else {
                return Err(ProtoError::Invalid(
                    "invalid request id: must be a non-negative safe integer or non-empty string"
                        .into(),
                ));
            };
            InputKitRequestId::Number(u)
        }
        Value::String(s) => InputKitRequestId::String(s.clone()),
        _ => {
            return Err(ProtoError::Invalid(
                "invalid request id: must be a non-negative safe integer or non-empty string"
                    .into(),
            ));
        }
    };
    id.validate()?;
    Ok(id)
}

fn require_u64_field(obj: &serde_json::Map<String, Value>, key: &str) -> Result<u64, ProtoError> {
    obj.get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| ProtoError::Invalid(format!("{key} must be a non-negative integer")))
}

fn require_str_field<'a>(
    obj: &'a serde_json::Map<String, Value>,
    key: &str,
) -> Result<&'a str, ProtoError> {
    obj.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ProtoError::Invalid(format!("{key} must be a string")))
}

fn require_bool_field(obj: &serde_json::Map<String, Value>, key: &str) -> Result<bool, ProtoError> {
    obj.get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| ProtoError::Invalid(format!("{key} must be a boolean")))
}

fn parse_optional_u32_field(
    obj: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<u32>, ProtoError> {
    match obj.get(key) {
        None => Ok(None),
        Some(v) => {
            let Some(n) = v.as_u64().and_then(|n| u32::try_from(n).ok()) else {
                return Err(ProtoError::Invalid(format!(
                    "{key} must be an integer between 0 and 0xFFFFFF"
                )));
            };
            Ok(Some(n))
        }
    }
}

fn parse_optional_f64_field(
    obj: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<f64>, ProtoError> {
    match obj.get(key) {
        None => Ok(None),
        Some(v) => {
            let Some(n) = v.as_f64() else {
                return Err(ProtoError::Invalid(format!(
                    "{key} must be a finite number between 0 and 1"
                )));
            };
            Ok(Some(n))
        }
    }
}

fn parse_optional_effect_field(
    obj: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<SaRgbEffect>, ProtoError> {
    match obj.get(key) {
        None => Ok(None),
        Some(v) => {
            let Some(raw) = v.as_u64().and_then(|n| u8::try_from(n).ok()) else {
                return Err(ProtoError::Invalid(
                    "effect must be 0, 1, 3, 4, or 6".into(),
                ));
            };
            SaRgbEffect::from_u8(raw).map(Some)
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct InputKitRequest {
    id: InputKitRequestId,
    method: InputKitMethod,
}

impl InputKitRequest {
    pub fn new(id: InputKitRequestId, method: InputKitMethod) -> Result<Self, ProtoError> {
        id.validate()?;
        method.validate()?;
        Ok(Self { id, method })
    }

    pub fn id(&self) -> &InputKitRequestId {
        &self.id
    }

    pub fn method(&self) -> &InputKitMethod {
        &self.method
    }

    pub fn into_parts(self) -> (InputKitRequestId, InputKitMethod) {
        (self.id, self.method)
    }

    pub fn encode_frame(&self) -> Result<Vec<u8>, ProtoError> {
        self.id.validate()?;
        self.method.validate()?;
        let envelope = match &self.method {
            InputKitMethod::RequestAuthorization(params) => json!({
                "jsonrpc": "2.0",
                "id": &self.id,
                "method": self.method.method_name(),
                "params": params,
            }),
            InputKitMethod::Authenticate(params) => json!({
                "jsonrpc": "2.0",
                "id": &self.id,
                "method": self.method.method_name(),
                "params": params,
            }),
            InputKitMethod::GetDevices => json!({
                "jsonrpc": "2.0",
                "id": &self.id,
                "method": self.method.method_name(),
            }),
            InputKitMethod::GetDeviceInfo(params) => json!({
                "jsonrpc": "2.0",
                "id": &self.id,
                "method": self.method.method_name(),
                "params": params,
            }),
            InputKitMethod::SetActiveProfileLayer(params) => json!({
                "jsonrpc": "2.0",
                "id": &self.id,
                "method": self.method.method_name(),
                "params": params,
            }),
            InputKitMethod::SetKeyLighting(params) => json!({
                "jsonrpc": "2.0",
                "id": &self.id,
                "method": self.method.method_name(),
                "params": params,
            }),
            InputKitMethod::AddLayer(params) => json!({
                "jsonrpc": "2.0",
                "id": &self.id,
                "method": self.method.method_name(),
                "params": params,
            }),
        };
        let bytes = serde_json::to_vec(&envelope)
            .map_err(|e| ProtoError::Invalid(format!("failed to encode InputKit request: {e}")))?;
        if bytes.len() > MAX_INPUT_KIT_FRAME_BYTES {
            return Err(ProtoError::Invalid(
                "InputKit request exceeds the 512 KiB WebSocket message limit".into(),
            ));
        }
        Ok(bytes)
    }

    pub fn decode_frame(bytes: &[u8]) -> Result<Self, ProtoError> {
        if bytes.len() > MAX_INPUT_KIT_FRAME_BYTES {
            return Err(ProtoError::Invalid(
                "InputKit frame exceeds the 512 KiB WebSocket message limit".into(),
            ));
        }
        let value: Value = serde_json::from_slice(bytes)
            .map_err(|e| ProtoError::Invalid(format!("invalid JSON-RPC frame: {e}")))?;
        let obj = require_object(&value, "request")?;
        if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Err(ProtoError::Invalid(
                "invalid request: jsonrpc must be \"2.0\"".into(),
            ));
        }
        let id_value = obj
            .get("id")
            .ok_or_else(|| ProtoError::Invalid("invalid request: missing id".into()))?;
        let id = parse_request_id(id_value)?;
        let method_name = obj
            .get("method")
            .and_then(Value::as_str)
            .ok_or_else(|| ProtoError::Invalid("invalid request: missing method".into()))?;

        let method = match method_name {
            "requestAuthorization" => {
                let params_val = obj
                    .get("params")
                    .ok_or_else(|| ProtoError::Invalid("missing params".into()))?;
                let params = require_object(params_val, "params")?;
                let raw_name = require_str_field(params, "applicationName")?;
                let application_name = validate_application_name(raw_name)?;
                InputKitMethod::RequestAuthorization(RequestAuthorizationParams {
                    application_name,
                })
            }
            "authenticate" => {
                let params_val = obj
                    .get("params")
                    .ok_or_else(|| ProtoError::Invalid("missing params".into()))?;
                let params = require_object(params_val, "params")?;
                let auth = AuthenticateParams {
                    application_id: require_str_field(params, "applicationId")?.to_owned(),
                    token: require_str_field(params, "token")?.to_owned(),
                };
                auth.validate()?;
                InputKitMethod::Authenticate(auth)
            }
            "getDevices" => {
                match obj.get("params") {
                    None | Some(Value::Null) => {}
                    Some(Value::Object(map)) if map.is_empty() => {}
                    Some(_) => {
                        return Err(ProtoError::Invalid(
                            "getDevices does not accept parameters".into(),
                        ));
                    }
                }
                InputKitMethod::GetDevices
            }
            "getDeviceInfo" => {
                let params_val = obj
                    .get("params")
                    .ok_or_else(|| ProtoError::Invalid("missing params".into()))?;
                let params = require_object(params_val, "params")?;
                let parsed = GetDeviceInfoParams {
                    device_id: require_u64_field(params, "deviceId")?,
                };
                parsed.validate()?;
                InputKitMethod::GetDeviceInfo(parsed)
            }
            "setActiveProfileLayer" => {
                let params_val = obj
                    .get("params")
                    .ok_or_else(|| ProtoError::Invalid("missing params".into()))?;
                let params = require_object(params_val, "params")?;
                let parsed = SetActiveProfileLayerParams {
                    device_id: require_u64_field(params, "deviceId")?,
                    profile_index: require_u64_field(params, "profileIndex")?,
                    layer_index: require_u64_field(params, "layerIndex")?,
                };
                parsed.validate()?;
                InputKitMethod::SetActiveProfileLayer(parsed)
            }
            "setKeyLighting" => {
                let params_val = obj
                    .get("params")
                    .ok_or_else(|| ProtoError::Invalid("missing params".into()))?;
                let params = require_object(params_val, "params")?;
                let parsed = SetKeyLightingParams {
                    device_id: require_u64_field(params, "deviceId")?,
                    identifier: require_str_field(params, "identifier")?.to_owned(),
                    is_regex: require_bool_field(params, "isRegex")?,
                    color: parse_optional_u32_field(params, "color")?,
                    brightness: parse_optional_f64_field(params, "brightness")?,
                    effect: parse_optional_effect_field(params, "effect")?,
                    speed: parse_optional_f64_field(params, "speed")?,
                };
                parsed.validate()?;
                InputKitMethod::SetKeyLighting(parsed)
            }
            "addLayer" => {
                let params_val = obj
                    .get("params")
                    .ok_or_else(|| ProtoError::Invalid("missing params".into()))?;
                let params = require_object(params_val, "params")?;
                let definition = params
                    .get("definition")
                    .cloned()
                    .ok_or_else(|| ProtoError::Invalid("missing definition".into()))?;
                let parsed = AddLayerParams {
                    device_id: require_u64_field(params, "deviceId")?,
                    profile_index: require_u64_field(params, "profileIndex")?,
                    definition,
                };
                parsed.validate()?;
                InputKitMethod::AddLayer(parsed)
            }
            unknown => {
                return Err(ProtoError::Invalid(format!("method not found: {unknown}")));
            }
        };

        Ok(Self { id, method })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizationCredentials {
    pub application_id: String,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSummary {
    pub id: u64,
    pub device_type: String,
}

impl DeviceSummary {
    pub fn validate(&self) -> Result<(), ProtoError> {
        validate_device_id(self.id)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub firmware_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_profile_index: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_layer_index: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battery_percentage: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_charging: Option<bool>,
}

impl DeviceStatus {
    pub fn validate(&self) -> Result<(), ProtoError> {
        if let Some(p) = self.selected_profile_index {
            validate_profile_index(p)?;
        }
        if let Some(l) = self.selected_layer_index {
            validate_layer_index(l)?;
        }
        if let Some(b) = self.battery_percentage {
            if !b.is_finite() || !(0.0..=100.0).contains(&b) {
                return Err(ProtoError::Invalid(
                    "batteryPercentage must be a finite number between 0 and 100".into(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub port_path: String,
    pub device_pid: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial_number: Option<String>,
    pub connection_type: u32,
    pub device_type: String,
    pub layout_type: String,
    pub is_usb_connection: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_status: Option<DeviceStatus>,
}

impl DeviceInfo {
    pub fn validate(&self) -> Result<(), ProtoError> {
        if let Some(status) = &self.device_status {
            status.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputKitRpcError {
    pub code: i32,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputKitResponseBody {
    Result(Value),
    Error(InputKitRpcError),
}

#[derive(Debug, Clone, PartialEq)]
pub struct InputKitResponse {
    pub id: InputKitRequestId,
    pub body: InputKitResponseBody,
}

impl InputKitResponse {
    pub fn ok(id: InputKitRequestId, result: Value) -> Self {
        Self {
            id,
            body: InputKitResponseBody::Result(result),
        }
    }

    pub fn authorization(id: InputKitRequestId, credentials: &AuthorizationCredentials) -> Self {
        Self::ok(
            id,
            serde_json::to_value(credentials).expect("AuthorizationCredentials serializes"),
        )
    }

    pub fn authenticated(id: InputKitRequestId) -> Self {
        Self::ok(id, json!({ "authenticated": true }))
    }

    pub fn devices(id: InputKitRequestId, devices: &[DeviceSummary]) -> Self {
        Self::ok(
            id,
            serde_json::to_value(devices).expect("DeviceSummary slice serializes"),
        )
    }

    pub fn device_info(id: InputKitRequestId, info: &DeviceInfo) -> Self {
        Self::ok(
            id,
            serde_json::to_value(info).expect("DeviceInfo serializes"),
        )
    }

    pub fn changed(id: InputKitRequestId) -> Self {
        Self::ok(id, json!({ "changed": true }))
    }

    pub fn applied(id: InputKitRequestId) -> Self {
        Self::ok(id, json!({ "applied": true }))
    }

    pub fn added(id: InputKitRequestId) -> Self {
        Self::ok(id, json!({ "added": true }))
    }

    pub fn error(
        id: InputKitRequestId,
        code: InputKitErrorCode,
        message: impl Into<String>,
    ) -> Self {
        Self {
            id,
            body: InputKitResponseBody::Error(InputKitRpcError {
                code: code.code(),
                message: message.into(),
            }),
        }
    }

    pub fn device_rate_limited(id: InputKitRequestId, message: impl Into<String>) -> Self {
        Self::error(id, InputKitErrorCode::DeviceRateLimited, message)
    }

    pub fn from_proto_error(id: InputKitRequestId, err: &ProtoError) -> Self {
        let message = err.to_string();
        let code = match err {
            ProtoError::Unverified(_) => InputKitErrorCode::InternalError,
            ProtoError::Invalid(detail) if detail.starts_with("method not found:") => {
                InputKitErrorCode::MethodNotFound
            }
            ProtoError::Invalid(detail)
                if detail.starts_with("invalid request")
                    || detail.starts_with("invalid JSON-RPC")
                    || detail.starts_with("InputKit frame exceeds") =>
            {
                InputKitErrorCode::InvalidRequest
            }
            ProtoError::Invalid(_) => InputKitErrorCode::InvalidParams,
        };
        Self::error(id, code, message)
    }

    pub fn encode_frame(&self) -> Result<Vec<u8>, ProtoError> {
        self.id.validate()?;
        let envelope = match &self.body {
            InputKitResponseBody::Result(result) => json!({
                "jsonrpc": "2.0",
                "id": &self.id,
                "result": result,
            }),
            InputKitResponseBody::Error(error) => json!({
                "jsonrpc": "2.0",
                "id": &self.id,
                "error": error,
            }),
        };
        let bytes = serde_json::to_vec(&envelope)
            .map_err(|e| ProtoError::Invalid(format!("failed to encode InputKit response: {e}")))?;
        if bytes.len() > MAX_INPUT_KIT_FRAME_BYTES {
            return Err(ProtoError::Invalid(
                "InputKit response exceeds the 512 KiB WebSocket message limit".into(),
            ));
        }
        Ok(bytes)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyTrigger {
    Press,
    Release,
    Click,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputKitNotification {
    DevicesUpdated {
        devices: Vec<DeviceSummary>,
    },
    DeviceInfoChanged {
        device_id: u64,
        device_info: DeviceInfo,
    },
    KeyTriggered {
        device_id: u64,
        identifier: String,
        action: KeyTrigger,
    },
}

impl InputKitNotification {
    pub const fn method_name(&self) -> &'static str {
        match self {
            Self::DevicesUpdated { .. } => "devicesUpdated",
            Self::DeviceInfoChanged { .. } => "deviceInfoChanged",
            Self::KeyTriggered { .. } => "keyTriggered",
        }
    }

    pub fn validate(&self) -> Result<(), ProtoError> {
        match self {
            Self::DevicesUpdated { devices } => {
                for d in devices {
                    d.validate()?;
                }
                Ok(())
            }
            Self::DeviceInfoChanged {
                device_id,
                device_info,
            } => {
                validate_device_id(*device_id)?;
                device_info.validate()
            }
            Self::KeyTriggered {
                device_id,
                identifier,
                ..
            } => {
                validate_device_id(*device_id)?;
                validate_api_identifier(identifier)
            }
        }
    }

    pub fn encode_frame(&self) -> Result<Vec<u8>, ProtoError> {
        self.validate()?;
        let params = match self {
            Self::DevicesUpdated { devices } => json!({ "devices": devices }),
            Self::DeviceInfoChanged {
                device_id,
                device_info,
            } => json!({
                "deviceId": device_id,
                "deviceInfo": device_info,
            }),
            Self::KeyTriggered {
                device_id,
                identifier,
                action,
            } => json!({
                "deviceId": device_id,
                "identifier": identifier,
                "action": action,
            }),
        };
        let envelope = json!({
            "jsonrpc": "2.0",
            "method": self.method_name(),
            "params": params,
        });
        let bytes = serde_json::to_vec(&envelope).map_err(|e| {
            ProtoError::Invalid(format!("failed to encode InputKit notification: {e}"))
        })?;
        if bytes.len() > MAX_INPUT_KIT_FRAME_BYTES {
            return Err(ProtoError::Invalid(
                "InputKit notification exceeds the 512 KiB WebSocket message limit".into(),
            ));
        }
        Ok(bytes)
    }

    pub fn decode_frame(bytes: &[u8]) -> Result<Self, ProtoError> {
        if bytes.len() > MAX_INPUT_KIT_FRAME_BYTES {
            return Err(ProtoError::Invalid(
                "InputKit notification exceeds the 512 KiB WebSocket message limit".into(),
            ));
        }
        let value: Value = serde_json::from_slice(bytes)
            .map_err(|e| ProtoError::Invalid(format!("invalid JSON-RPC notification: {e}")))?;
        let obj = require_object(&value, "notification")?;
        if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Err(ProtoError::Invalid(
                "invalid notification: jsonrpc must be \"2.0\"".into(),
            ));
        }
        if obj.contains_key("id") {
            return Err(ProtoError::Invalid(
                "invalid notification: notifications must not include id".into(),
            ));
        }
        let method = obj
            .get("method")
            .and_then(Value::as_str)
            .ok_or_else(|| ProtoError::Invalid("invalid notification: missing method".into()))?;
        let params_val = obj
            .get("params")
            .ok_or_else(|| ProtoError::Invalid("invalid notification: missing params".into()))?;
        let params = require_object(params_val, "params")?;

        let notification = match method {
            "devicesUpdated" => {
                let raw_devices =
                    params
                        .get("devices")
                        .and_then(Value::as_array)
                        .ok_or_else(|| {
                            ProtoError::Invalid(
                                "devicesUpdated params.devices must be an array".into(),
                            )
                        })?;
                let mut devices = Vec::with_capacity(raw_devices.len());
                for item in raw_devices {
                    let summary: DeviceSummary = serde_json::from_value(item.clone())
                        .map_err(|e| ProtoError::Invalid(format!("invalid device summary: {e}")))?;
                    summary.validate()?;
                    devices.push(summary);
                }
                Self::DevicesUpdated { devices }
            }
            "deviceInfoChanged" => {
                let device_id = require_u64_field(params, "deviceId")?;
                let info_val = params.get("deviceInfo").ok_or_else(|| {
                    ProtoError::Invalid("deviceInfoChanged missing deviceInfo".into())
                })?;
                let device_info: DeviceInfo = serde_json::from_value(info_val.clone())
                    .map_err(|e| ProtoError::Invalid(format!("invalid deviceInfo payload: {e}")))?;
                Self::DeviceInfoChanged {
                    device_id,
                    device_info,
                }
            }
            "keyTriggered" => {
                let device_id = require_u64_field(params, "deviceId")?;
                let identifier = require_str_field(params, "identifier")?.to_owned();
                let action_val = params
                    .get("action")
                    .ok_or_else(|| ProtoError::Invalid("keyTriggered missing action".into()))?;
                let action: KeyTrigger =
                    serde_json::from_value(action_val.clone()).map_err(|e| {
                        ProtoError::Invalid(format!("invalid keyTriggered action: {e}"))
                    })?;
                Self::KeyTriggered {
                    device_id,
                    identifier,
                    action,
                }
            }
            other => {
                return Err(ProtoError::Invalid(format!(
                    "unknown InputKit notification method: {other}"
                )));
            }
        };
        notification.validate()?;
        Ok(notification)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_host_parses_allowlist_and_formats_ws_urls() {
        assert_eq!(
            LoopbackHost::parse("127.0.0.1")
                .unwrap()
                .ws_url(INPUT_KIT_DEFAULT_PORT)
                .unwrap(),
            "ws://127.0.0.1:19147/input"
        );
        assert_eq!(
            LoopbackHost::parse("::1").unwrap().ws_url(19147).unwrap(),
            "ws://[::1]:19147/input"
        );
        assert_eq!(
            LoopbackHost::parse("localhost")
                .unwrap()
                .ws_url(8080)
                .unwrap(),
            "ws://localhost:8080/input"
        );
        assert!(LoopbackHost::parse("0.0.0.0").is_err());
        assert!(LoopbackHost::parse("192.168.1.5").is_err());
        assert!(LoopbackHost::parse("example.com").is_err());
        assert!(LoopbackHost::Ipv4Loopback.ws_url(0).is_err());
    }

    #[test]
    fn decodes_and_classifies_all_seven_input_kit_methods() {
        let cases: Vec<(Value, &'static str, bool, Option<u64>)> = vec![
            (
                json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "requestAuthorization",
                    "params": { "applicationName": "  Claude Code  " }
                }),
                "requestAuthorization",
                false,
                None,
            ),
            (
                json!({
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "authenticate",
                    "params": { "applicationId": "app-1", "token": "tok-1" }
                }),
                "authenticate",
                false,
                None,
            ),
            (
                json!({
                    "jsonrpc": "2.0",
                    "id": 3,
                    "method": "getDevices"
                }),
                "getDevices",
                false,
                None,
            ),
            (
                json!({
                    "jsonrpc": "2.0",
                    "id": 4,
                    "method": "getDeviceInfo",
                    "params": { "deviceId": 7 }
                }),
                "getDeviceInfo",
                false,
                Some(7),
            ),
            (
                json!({
                    "jsonrpc": "2.0",
                    "id": 5,
                    "method": "setActiveProfileLayer",
                    "params": { "deviceId": 7, "profileIndex": 0, "layerIndex": 1 }
                }),
                "setActiveProfileLayer",
                true,
                Some(7),
            ),
            (
                json!({
                    "jsonrpc": "2.0",
                    "id": 6,
                    "method": "setKeyLighting",
                    "params": {
                        "deviceId": 7,
                        "identifier": "claude.approve",
                        "isRegex": false,
                        "color": 0xFFAA00,
                        "brightness": 0.9,
                        "effect": 4,
                        "speed": 0.75
                    }
                }),
                "setKeyLighting",
                true,
                Some(7),
            ),
            (
                json!({
                    "jsonrpc": "2.0",
                    "id": "req-7",
                    "method": "addLayer",
                    "params": {
                        "deviceId": 7,
                        "profileIndex": 2,
                        "definition": { "id": 100, "name": "Plugin Layer" }
                    }
                }),
                "addLayer",
                true,
                Some(7),
            ),
        ];

        for (raw, expected_name, expected_write, expected_device) in cases {
            let bytes = serde_json::to_vec(&raw).unwrap();
            let decoded = InputKitRequest::decode_frame(&bytes)
                .unwrap_or_else(|e| panic!("expected {expected_name} to decode, got {e}"));
            assert_eq!(decoded.method().method_name(), expected_name);
            assert_eq!(decoded.method().is_device_write(), expected_write);
            assert_eq!(decoded.method().target_device_id(), expected_device);

            let reencoded = decoded.encode_frame().unwrap();
            let roundtrip = InputKitRequest::decode_frame(&reencoded).unwrap();
            assert_eq!(roundtrip, decoded);
        }

        let auth_bytes = serde_json::to_vec(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "requestAuthorization",
            "params": { "applicationName": "  Claude Code  " }
        }))
        .unwrap();
        let (_, method) = InputKitRequest::decode_frame(&auth_bytes)
            .unwrap()
            .into_parts();
        assert_eq!(
            method,
            InputKitMethod::RequestAuthorization(RequestAuthorizationParams {
                application_name: "Claude Code".into()
            })
        );
    }

    #[test]
    fn enforces_512_kib_frame_limit_and_jsonrpc_envelope() {
        let oversized = vec![b' '; MAX_INPUT_KIT_FRAME_BYTES + 1];
        assert!(InputKitRequest::decode_frame(&oversized).is_err());

        for bad in [
            json!({ "id": 1, "method": "getDevices" }),
            json!({ "jsonrpc": "1.0", "id": 1, "method": "getDevices" }),
            json!({ "jsonrpc": "2.0", "method": "getDevices" }),
            json!({ "jsonrpc": "2.0", "id": null, "method": "getDevices" }),
            json!({ "jsonrpc": "2.0", "id": "", "method": "getDevices" }),
            json!({ "jsonrpc": "2.0", "id": MAX_SAFE_INTEGER + 1, "method": "getDevices" }),
            json!({ "jsonrpc": "2.0", "id": 1, "method": "unknownMethod" }),
        ] {
            let bytes = serde_json::to_vec(&bad).unwrap();
            assert!(
                InputKitRequest::decode_frame(&bytes).is_err(),
                "expected invalid envelope to fail: {bad}"
            );
        }
    }

    #[test]
    fn validates_application_name_utf16_identifiers_and_index_bounds() {
        assert_eq!(
            validate_application_name("  Micro Input  ").unwrap(),
            "Micro Input"
        );
        assert!(validate_application_name("").is_err());
        assert!(validate_application_name("   ").is_err());
        assert!(validate_application_name(&"a".repeat(80)).is_ok());
        assert!(validate_application_name(&"a".repeat(81)).is_err());
        assert!(validate_application_name("bad\nname").is_err());
        assert!(validate_application_name("bad\u{0000}name").is_err());
        assert!(validate_application_name("bad\u{200B}name").is_err());
        assert!(validate_application_name("bad\u{2028}name").is_err());
        assert!(validate_application_name("bad\u{2029}name").is_err());
        assert!(validate_application_name("bad\u{FEFF}name").is_err());

        let exact_128_ascii = "k".repeat(128);
        assert!(validate_api_identifier(&exact_128_ascii).is_ok());
        assert!(validate_api_identifier(&"k".repeat(129)).is_err());

        let emoji_64 = "😀".repeat(64);
        assert_eq!(emoji_64.encode_utf16().count(), 128);
        assert!(validate_api_identifier(&emoji_64).is_ok());
        let emoji_65 = "😀".repeat(65);
        assert_eq!(emoji_65.encode_utf16().count(), 130);
        assert!(validate_api_identifier(&emoji_65).is_err());

        assert!(validate_api_identifier(" leading").is_err());
        assert!(validate_api_identifier("trailing ").is_err());
        assert!(validate_api_identifier("ctrl\u{001F}key").is_err());
        assert!(validate_api_identifier("del\u{007F}key").is_err());
        assert!(validate_api_identifier("line\u{2028}sep").is_err());
        assert!(validate_api_identifier("para\u{2029}sep").is_err());

        let emoji_128 = "😀".repeat(128);
        assert_eq!(emoji_128.encode_utf16().count(), 256);
        assert!(validate_identifier_pattern(&emoji_128).is_ok());
        let emoji_129 = "😀".repeat(129);
        assert!(validate_identifier_pattern(&emoji_129).is_err());

        for bad_pl in [
            json!({ "deviceId": 0, "profileIndex": 0, "layerIndex": 1 }),
            json!({ "deviceId": 1, "profileIndex": -1, "layerIndex": 1 }),
            json!({ "deviceId": 1, "profileIndex": 0, "layerIndex": 0 }),
            json!({ "deviceId": MAX_SAFE_INTEGER + 1, "profileIndex": 0, "layerIndex": 1 }),
        ] {
            let frame = serde_json::to_vec(&json!({
                "jsonrpc": "2.0",
                "id": 9,
                "method": "setActiveProfileLayer",
                "params": bad_pl
            }))
            .unwrap();
            assert!(
                InputKitRequest::decode_frame(&frame).is_err(),
                "expected invalid profile/layer params to fail: {bad_pl}"
            );
        }
    }

    #[test]
    fn set_key_lighting_converts_to_sa_rgb_updates_and_normalizes_off_effect() {
        let lit = SetKeyLightingParams {
            device_id: 1,
            identifier: "slot.0".into(),
            is_regex: false,
            color: Some(0x0088FF),
            brightness: Some(0.9),
            effect: Some(SaRgbEffect::Solid),
            speed: Some(0.5),
        };
        let ids: Vec<u32> = (1001..=1020).collect();
        let chunks = lit.to_sa_rgb_updates(&ids, 16).unwrap();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].sa.len(), 16);
        assert_eq!(chunks[1].sa.len(), 4);
        assert_eq!(chunks[0].c, Some(0x0088FF));
        assert_eq!(chunks[0].e, Some(SaRgbEffect::Solid.as_u8()));
        assert!((chunks[0].b.unwrap() - 0.9).abs() < 1e-6);
        assert!((chunks[0].s.unwrap() - 0.5).abs() < 1e-6);

        let off_with_extra_fields = SetKeyLightingParams {
            device_id: 1,
            identifier: "^slot\\.".into(),
            is_regex: true,
            color: Some(0xFF0000),
            brightness: Some(1.0),
            effect: Some(SaRgbEffect::Off),
            speed: Some(0.5),
        };
        let reset_chunks = off_with_extra_fields
            .to_sa_rgb_updates(&[7057, 7058], 16)
            .unwrap();
        assert_eq!(reset_chunks, vec![SaRgbUpdate::reset(vec![7057, 7058])]);
        assert!(reset_chunks[0].c.is_none());
        assert!(reset_chunks[0].b.is_none());
        assert!(reset_chunks[0].s.is_none());
        assert_eq!(reset_chunks[0].e, Some(0));

        let empty = SetKeyLightingParams {
            device_id: 1,
            identifier: "slot.0".into(),
            is_regex: false,
            color: None,
            brightness: None,
            effect: None,
            speed: None,
        };
        assert!(empty.validate().is_err());

        for bad in [
            json!({ "deviceId": 1, "identifier": "a", "isRegex": false, "color": 0x100_0000 }),
            json!({ "deviceId": 1, "identifier": "a", "isRegex": false, "brightness": 1.1 }),
            json!({ "deviceId": 1, "identifier": "a", "isRegex": false, "speed": -0.1 }),
            json!({ "deviceId": 1, "identifier": "a", "isRegex": false, "effect": 2 }),
            json!({ "deviceId": 1, "identifier": "a", "isRegex": false, "effect": 5 }),
            json!({ "deviceId": 1, "identifier": "a", "isRegex": "false", "color": 0 }),
        ] {
            let frame = serde_json::to_vec(&json!({
                "jsonrpc": "2.0",
                "id": 12,
                "method": "setKeyLighting",
                "params": bad
            }))
            .unwrap();
            assert!(
                InputKitRequest::decode_frame(&frame).is_err(),
                "expected invalid lighting params to fail: {bad}"
            );
        }
    }

    #[test]
    fn encodes_responses_including_device_rate_limit_32017_and_notifications() {
        let rate_limited = InputKitResponse::device_rate_limited(
            InputKitRequestId::Number(42),
            "device write budget exhausted",
        );
        let parsed: Value = serde_json::from_slice(&rate_limited.encode_frame().unwrap()).unwrap();
        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["id"], 42);
        assert_eq!(parsed["error"]["code"], INPUT_KIT_DEVICE_RATE_LIMIT_CODE);
        assert_eq!(parsed["error"]["code"], -32017);
        assert_eq!(parsed["error"]["message"], "device write budget exhausted");

        let applied: Value = serde_json::from_slice(
            &InputKitResponse::applied(InputKitRequestId::Number(1))
                .encode_frame()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(applied["result"], json!({ "applied": true }));

        let changed: Value = serde_json::from_slice(
            &InputKitResponse::changed(InputKitRequestId::Number(2))
                .encode_frame()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(changed["result"], json!({ "changed": true }));

        let added: Value = serde_json::from_slice(
            &InputKitResponse::added(InputKitRequestId::Number(3))
                .encode_frame()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(added["result"], json!({ "added": true }));

        let authed: Value = serde_json::from_slice(
            &InputKitResponse::authenticated(InputKitRequestId::Number(4))
                .encode_frame()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(authed["result"], json!({ "authenticated": true }));

        let err_unknown = InputKitResponse::from_proto_error(
            InputKitRequestId::Number(5),
            &ProtoError::Invalid("method not found: bogus".into()),
        );
        assert_eq!(
            err_unknown.body,
            InputKitResponseBody::Error(InputKitRpcError {
                code: InputKitErrorCode::MethodNotFound.code(),
                message: "无效 RPC：method not found: bogus".into(),
            })
        );

        let notifications = [
            InputKitNotification::DevicesUpdated {
                devices: vec![DeviceSummary {
                    id: 1,
                    device_type: "Creator Micro 2".into(),
                }],
            },
            InputKitNotification::DeviceInfoChanged {
                device_id: 1,
                device_info: DeviceInfo {
                    port_path: "DevSrvsID:7".into(),
                    device_pid: "8298".into(),
                    serial_number: Some("SN-1".into()),
                    connection_type: 1,
                    device_type: "Creator Micro 2".into(),
                    layout_type: "default".into(),
                    is_usb_connection: true,
                    device_status: Some(DeviceStatus {
                        firmware_version: Some("0.7.0-rc.16".into()),
                        selected_profile_index: Some(0),
                        selected_layer_index: Some(1),
                        battery_percentage: Some(88.0),
                        is_charging: Some(true),
                    }),
                },
            },
            InputKitNotification::KeyTriggered {
                device_id: 1,
                identifier: "claude.approve".into(),
                action: KeyTrigger::Click,
            },
        ];

        for notif in notifications {
            let encoded = notif.encode_frame().unwrap();
            let raw: Value = serde_json::from_slice(&encoded).unwrap();
            assert_eq!(raw["jsonrpc"], "2.0");
            assert!(raw.get("id").is_none());
            let decoded = InputKitNotification::decode_frame(&encoded).unwrap();
            assert_eq!(decoded, notif);
        }

        let bad_notif = serde_json::to_vec(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "keyTriggered",
            "params": { "deviceId": 1, "identifier": "ok", "action": "press" }
        }))
        .unwrap();
        assert!(InputKitNotification::decode_frame(&bad_notif).is_err());
    }
}
