//! R-C03/R-C05/R-N03: owned display projection retaining raw values; exact device bytes remain in the session.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Source {
    Demo,
    DeviceSnapshot,
    Imported,
    LocalDraft,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_files: Option<Vec<FileVerification>>,
    pub source: Source,
    pub read_at: u64,
    pub firmware: Option<String>,
    pub profiles: Vec<Profile>,
    pub raw: Value,
    pub warnings: Vec<String>,
    pub macros: Vec<Macro>,
    pub multi_actions: Vec<MultiAction>,
    pub smart_actions: Value,
    pub active_profile: ActiveProfile,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileVerification {
    pub name: String,
    pub size: u64,
    pub sha1: String,
}
// TODO(UNKNOWN-06): device/cache schema equivalence and active profile semantics need device evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActiveProfile {
    Unknown(Value),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: Value,
    pub index: usize,
    pub name: String,
    pub layers: Vec<Layer>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer {
    pub id: Value,
    pub name: String,
    pub color: Value,
    pub keys: [Binding; 13],
    pub encoders: [Binding; 3],
    pub joystick: Joystick,
    pub lights: Lights,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Joystick {
    pub kind: Value,
    pub sectors: Vec<Binding>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lights {
    pub backlight: Value,
    pub underglow: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Token(pub String);
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Binding {
    Basic(Token),
    Macro(Value),
    Multi(Value),
    Smart(Value),
    Empty,
    Unknown(Value),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Macro {
    pub id: Value,
    pub name: String,
    pub color: Value,
    pub events: Vec<MacroEvent>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacroEvent {
    pub token: Token,
    pub delay_ms: i64,
    pub act: Act,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Act {
    Release,
    Press,
    Click,
    Unknown(i64),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiAction {
    pub id: Value,
    pub tap: Value,
    pub double_tap: Value,
    pub hold: Value,
    pub tap_hold: Value,
    pub tapping_term_ms: i64,
}
#[derive(Debug, thiserror::Error)]
#[error("配置无法解析：{0}")]
pub struct ParseError(pub String);
fn error(s: &str) -> ParseError {
    ParseError(s.into())
}
fn array<'a>(v: &'a Value, key: &str) -> Result<&'a Vec<Value>, ParseError> {
    v.get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| error(&format!("{key} 应为数组")))
}
fn string(v: &Value, key: &str) -> Result<String, ParseError> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| error(&format!("{key} 应为字符串")))
}
fn integer(v: &Value, key: &str) -> Result<i64, ParseError> {
    v.get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| error(&format!("{key} 应为整数")))
}
fn binding(v: &Value) -> Binding {
    // Only our synthetic schema has a proven mapping. Unknown tokens never become Empty.
    if v.is_null() {
        return Binding::Empty;
    }
    if let Some(t) = v.as_str() {
        return if matches!(t, "KC_A" | "KC_B") {
            Binding::Basic(Token(t.into()))
        } else {
            Binding::Unknown(v.clone())
        };
    }
    match v.get("kind").and_then(Value::as_str) {
        Some("macro") if v.get("ref").is_some() => Binding::Macro(v["ref"].clone()),
        Some("multi") if v.get("ref").is_some() => Binding::Multi(v["ref"].clone()),
        Some("smart") if v.get("ref").is_some() => Binding::Smart(v["ref"].clone()),
        _ => Binding::Unknown(v.clone()),
    }
}
/// Parses our documented synthetic interchange schema, NOT a verified device schema (R-N03).
pub fn parse_config(bytes: &[u8]) -> Result<DeviceConfig, ParseError> {
    let raw: Value = serde_json::from_slice(bytes).map_err(|e| error(&e.to_string()))?;
    let mut profiles = Vec::new();
    let mut warnings = vec!["设备 schema 与活动 Profile 语义待确认".into()];
    for (index, p) in array(&raw, "profiles")?.iter().enumerate() {
        if p.get("id").is_none() {
            return Err(error("Profile 缺少 id"));
        }
        let mut layers = Vec::new();
        for l in array(p, "layers")? {
            let rows = array(&l["layout"], "keymap")?;
            if rows.len() != 4
                || rows
                    .iter()
                    .zip([2, 4, 4, 3])
                    .any(|(r, n)| r.as_array().map(Vec::len) != Some(n))
            {
                return Err(error("keymap 行长必须为 2/4/4/3"));
            }
            let keys: Vec<_> = rows
                .iter()
                .flat_map(|r| r.as_array().into_iter().flatten())
                .map(binding)
                .collect();
            let raw_encoders = array(&l["layout"], "encoders")?;
            let enc: Vec<_> = if raw_encoders.len() == 1
                && raw_encoders[0]
                    .as_array()
                    .is_some_and(|inner| inner.len() == 3)
            {
                raw_encoders[0]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(binding)
                    .collect()
            } else {
                raw_encoders.iter().map(binding).collect()
            };
            let sectors = array(&l["layout"]["joystick"], "sectors")?
                .iter()
                .map(binding)
                .collect();
            layers.push(Layer {
                id: l["id"].clone(),
                name: string(l, "name")?,
                color: l["color"].clone(),
                keys: keys.try_into().map_err(|_| error("应有 13 键"))?,
                encoders: enc.try_into().map_err(|_| error("应有 3 个旋钮槽"))?,
                joystick: Joystick {
                    kind: l["layout"]["joystick"]["kind"].clone(),
                    sectors,
                },
                lights: Lights {
                    backlight: l["lights"]["backlight"].clone(),
                    underglow: l["lights"]["underglow"].clone(),
                },
            });
        }
        profiles.push(Profile {
            id: p["id"].clone(),
            index,
            name: string(p, "name")?,
            layers,
        });
    }
    let mut macros = Vec::new();
    for m in array(&raw, "macros")? {
        let mut events = Vec::new();
        for e in array(m, "events")? {
            // The 0/1/2 mapping belongs to our synthetic format, not firmware evidence.
            let a = integer(e, "act")?;
            events.push(MacroEvent {
                token: Token(string(e, "token")?),
                delay_ms: integer(e, "delay_ms")?,
                act: match a {
                    0 => Act::Release,
                    1 => Act::Press,
                    2 => Act::Click,
                    n => Act::Unknown(n),
                },
            });
        }
        macros.push(Macro {
            id: m["id"].clone(),
            name: string(m, "name")?,
            color: m["color"].clone(),
            events,
        });
    }
    let mut multi_actions = Vec::new();
    for m in array(&raw, "multiActions")? {
        multi_actions.push(MultiAction {
            id: m["id"].clone(),
            tap: m["tap"].clone(),
            double_tap: m["double_tap"].clone(),
            hold: m["hold"].clone(),
            tap_hold: m["tap_hold"].clone(),
            tapping_term_ms: integer(m, "tapping_term_ms")?,
        });
    }
    let smart_actions = raw.get("smartActions").cloned().unwrap_or(Value::Null);
    for b in profiles
        .iter()
        .flat_map(|p| &p.layers)
        .flat_map(|l| l.keys.iter().chain(&l.encoders).chain(&l.joystick.sectors))
    {
        let missing = match b {
            Binding::Macro(id) => !macros.iter().any(|m| &m.id == id),
            Binding::Multi(id) => !multi_actions.iter().any(|m| &m.id == id),
            Binding::Smart(id) => !smart_actions
                .as_array()
                .is_some_and(|a| a.iter().any(|m| &m["id"] == id)),
            _ => false,
        };
        if missing {
            warnings.push(format!("缺失引用：{b:?}"));
        }
    }
    warnings.sort();
    warnings.dedup();
    Ok(DeviceConfig {
        device_files: None,
        source: Source::Imported,
        read_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        firmware: None,
        profiles,
        active_profile: ActiveProfile::Unknown(raw["activeProfileId"].clone()),
        raw,
        warnings,
        macros,
        multi_actions,
        smart_actions,
    })
}
pub fn demo() -> Result<DeviceConfig, ParseError> {
    let mut c = parse_config(include_bytes!("../fixtures/demo.json"))?;
    c.source = Source::Demo;
    Ok(c)
}

pub const OFFICIAL_OAI_KEY_TOKENS: [&str; 13] = [
    "KV_OAI_AG00",
    "KV_OAI_AG01",
    "KV_OAI_AG02",
    "KV_OAI_AG03",
    "KV_OAI_AG04",
    "KV_OAI_AG05",
    "KV_OAI_ACT06",
    "KV_OAI_ACT07",
    "KV_OAI_ACT08",
    "KV_OAI_ACT09",
    "KV_OAI_ACT10",
    "KV_OAI_ACT11",
    "KV_OAI_ACT12",
];

pub const ALL_AG_OAI_KEY_TOKENS: [&str; 13] = [
    "KV_OAI_AG00",
    "KV_OAI_AG01",
    "KV_OAI_AG02",
    "KV_OAI_AG03",
    "KV_OAI_AG04",
    "KV_OAI_AG05",
    "KV_OAI_AG06",
    "KV_OAI_AG07",
    "KV_OAI_AG08",
    "KV_OAI_AG09",
    "KV_OAI_AG10",
    "KV_OAI_AG11",
    "KV_OAI_AG12",
];

pub const OFFICIAL_OAI_ENCODER_TOKENS: [&str; 3] =
    ["KV_OAI_ENC_CC", "KV_OAI_ENC_CW", "KV_OAI_ENC_CLK"];

pub(crate) fn binding_has_oai_prefix(binding: &Binding) -> bool {
    match binding {
        Binding::Basic(Token(s)) => s.starts_with("KV_OAI_"),
        Binding::Unknown(Value::String(s)) => s.starts_with("KV_OAI_"),
        Binding::Unknown(Value::Object(map)) => map
            .get("k")
            .and_then(Value::as_str)
            .is_some_and(|s| s.starts_with("KV_OAI_")),
        _ => false,
    }
}

pub fn is_protected_oai_layer(layer: &Layer) -> bool {
    layer.keys.iter().any(binding_has_oai_prefix)
        || layer.encoders.iter().any(binding_has_oai_prefix)
        || layer.joystick.sectors.iter().any(binding_has_oai_prefix)
}

pub fn official_chatgpt_layer(id: Value, name: &str) -> Layer {
    Layer {
        id,
        name: if name.trim().is_empty() {
            "ChatGPT".into()
        } else {
            name.trim().into()
        },
        color: Value::Number(0x00FFA6u64.into()),
        keys: std::array::from_fn(|i| {
            Binding::Unknown(Value::String(OFFICIAL_OAI_KEY_TOKENS[i].into()))
        }),
        encoders: std::array::from_fn(|i| {
            Binding::Unknown(Value::String(OFFICIAL_OAI_ENCODER_TOKENS[i].into()))
        }),
        joystick: Joystick {
            kind: Value::String("VENDOR".into()),
            sectors: Vec::new(),
        },
        lights: Lights {
            backlight: Value::Null,
            underglow: Value::Null,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn non_contiguous_ids_and_unknowns() {
        let d = demo().unwrap();
        assert_eq!(d.profiles[1].id, 42);
        assert_eq!(d.profiles[1].index, 1);
        assert!(matches!(
            d.profiles[0].layers[0].keys[1],
            Binding::Unknown(_)
        ));
        assert_eq!(d.raw["futureField"]["preserved"], true);
        assert!(d.warnings.iter().any(|w| w.contains("缺失引用")));
    }
    #[test]
    fn malformed_and_numeric_types() {
        assert!(parse_config(b"{").is_err());
        let mut v = demo().unwrap().raw;
        v["macros"][0]["events"][0]["delay_ms"] = (-500).into();
        assert!(parse_config(&serde_json::to_vec(&v).unwrap()).is_ok());
        v["macros"][0]["events"][0]["delay_ms"] = "bad".into();
        assert!(parse_config(&serde_json::to_vec(&v).unwrap()).is_err());
    }
}

pub mod draft;

pub mod storage;

#[cfg(test)]
mod document_tests;

#[cfg(test)]
mod device_tests;

mod device;
pub use device::parse_device_files;

pub mod action_library;
pub mod document;

pub mod comparison;
#[cfg(test)]
mod comparison_tests;
pub mod control_settings;
pub mod macro_sequence;

pub mod macro_library;

pub mod multi;

pub mod preset;

pub mod apply;

pub mod host_router;
pub use host_router::{
    resolve_host_action_with_layout, verified_physical_mapping_for_layout, BottomRowLayout,
};

pub mod lighting;

pub mod navigation_history;

pub mod operation_record;

pub mod slot_table;

pub mod board_layers;
pub mod direct_controls;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StrictSemver<'a> {
    major: u32,
    minor: u32,
    patch: u32,
    prerelease: Option<&'a str>,
}

fn parse_numeric_part(part: &str) -> Option<u32> {
    if part.is_empty()
        || !part.bytes().all(|b| b.is_ascii_digit())
        || (part.len() > 1 && part.starts_with('0'))
    {
        return None;
    }
    part.parse::<u32>().ok()
}

fn parse_strict_semver(version: &str) -> Option<StrictSemver<'_>> {
    let (core, prerelease) = match version.split_once('-') {
        Some((core, pre)) => {
            if pre.is_empty()
                || pre.split('.').any(|id| {
                    id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                })
            {
                return None;
            }
            (core, Some(pre))
        }
        None => (version, None),
    };
    let mut parts = core.split('.');
    let major = parse_numeric_part(parts.next()?)?;
    let minor = parse_numeric_part(parts.next()?)?;
    let patch = parse_numeric_part(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some(StrictSemver {
        major,
        minor,
        patch,
        prerelease,
    })
}

fn parse_rc_number(prerelease: &str) -> Option<u32> {
    let num = prerelease.strip_prefix("rc.")?;
    parse_numeric_part(num)
}

/// Returns `true` when `version` is a natively supported Creator Micro 2 firmware release.
pub fn is_supported_native_firmware(version: &str) -> bool {
    let Some(v) = parse_strict_semver(version) else {
        return false;
    };
    match (v.major, v.minor, v.patch, v.prerelease) {
        (0, 6, 3, None) | (0, 7, 0, None) => true,
        (0, 7, 0, Some(pre)) => parse_rc_number(pre).is_some(),
        _ => false,
    }
}

/// Returns `true` when `version` is valid for reading/planning device configs (native or synthetic).
pub fn is_supported_config_firmware(version: &str) -> bool {
    is_supported_native_firmware(version) || version == "synthetic-0.6.3"
}

/// Returns `true` when `version` is allowed to enter the USB bootloader (`0.6.3` or strict semver `>= 0.7.0-0`).
pub fn is_supported_bootloader_firmware(version: &str) -> bool {
    if version == "0.6.3" {
        return true;
    }
    let Some(v) = parse_strict_semver(version) else {
        return false;
    };
    (v.major == 0 && v.minor >= 7) || v.major >= 1
}

/// Returns `true` when `version` supports `0.7.0-rc.16` `MS_*` hardware tokens and `MOUSE`/`SCROLL` joystick modes.
pub fn supports_v070_mouse_tokens(version: &str) -> bool {
    let Some(v) = parse_strict_semver(version) else {
        return false;
    };
    if v.major >= 1 || (v.major == 0 && v.minor >= 8) {
        return true;
    }
    if v.major == 0 && v.minor == 7 {
        match v.prerelease {
            None => true,
            Some(_) if v.patch > 0 => true,
            Some(pre) => parse_rc_number(pre).is_some_and(|n| n >= 16),
        }
    } else {
        false
    }
}

/// Returns `true` when `command` is one of the 15 host CoreGraphics cursor/click/scroll/speed commands.
pub fn is_host_mouse_command(command: &str) -> bool {
    matches!(
        command,
        "mouse-move:up"
            | "mouse-move:down"
            | "mouse-move:left"
            | "mouse-move:right"
            | "mouse-click:left"
            | "mouse-click:right"
            | "mouse-click:middle"
            | "mouse-scroll:up"
            | "mouse-scroll:down"
            | "mouse-scroll:left"
            | "mouse-scroll:right"
            | "mouse-speed:0"
            | "mouse-speed:1"
            | "mouse-speed:2"
            | "mouse-speed:cycle"
    )
}

#[cfg(test)]
mod firmware_and_mouse_tests {
    use super::*;

    #[test]
    fn firmware_version_helpers_accept_expected_versions_and_reject_invalid_ones() {
        for v in ["0.6.3", "0.7.0", "0.7.0-rc.1", "0.7.0-rc.16", "0.7.0-rc.17"] {
            assert!(is_supported_native_firmware(v), "native should accept {v}");
            assert!(is_supported_config_firmware(v), "config should accept {v}");
        }
        assert!(!is_supported_native_firmware("synthetic-0.6.3"));
        assert!(is_supported_config_firmware("synthetic-0.6.3"));

        for v in [
            "0.7",
            "0.6.3-rc.10",
            "0.5.0",
            "",
            "0.7.0-rc.",
            "0.7.0-rc.a",
            "0.7.0-rc.01",
        ] {
            assert!(!is_supported_native_firmware(v), "native should reject {v}");
            assert!(!is_supported_config_firmware(v), "config should reject {v}");
        }

        for v in [
            "0.6.3",
            "0.7.0-rc.16",
            "0.7.0-rc.17",
            "0.7.0",
            "0.8.0",
            "1.0.0",
        ] {
            assert!(
                is_supported_bootloader_firmware(v),
                "bootloader should accept {v}"
            );
        }
        for v in [
            "0.7",
            "0.6.2",
            "0.6.3-rc.10",
            "synthetic-0.6.3",
            "",
            "01.0.0",
            "0.7.0+build1",
            "0.7.0-",
        ] {
            assert!(
                !is_supported_bootloader_firmware(v),
                "bootloader should reject {v}"
            );
        }

        for v in [
            "0.7.0-rc.16",
            "0.7.0-rc.17",
            "0.7.0",
            "0.7.1-rc.1",
            "0.7.1",
            "0.8.0",
            "1.0.0",
        ] {
            assert!(
                supports_v070_mouse_tokens(v),
                "v070 mouse tokens should be supported on {v}"
            );
        }
        for v in [
            "0.6.3",
            "0.7.0-rc.15",
            "0.7.0-rc.1",
            "0.7.0-rc.016",
            "0.7",
            "synthetic-0.6.3",
            "0.7.0-alpha.1",
        ] {
            assert!(
                !supports_v070_mouse_tokens(v),
                "v070 mouse tokens should be rejected on {v}"
            );
        }
    }

    #[test]
    fn host_mouse_command_matches_exact_fifteen_commands() {
        let valid = [
            "mouse-move:up",
            "mouse-move:down",
            "mouse-move:left",
            "mouse-move:right",
            "mouse-click:left",
            "mouse-click:right",
            "mouse-click:middle",
            "mouse-scroll:up",
            "mouse-scroll:down",
            "mouse-scroll:left",
            "mouse-scroll:right",
            "mouse-speed:0",
            "mouse-speed:1",
            "mouse-speed:2",
            "mouse-speed:cycle",
        ];
        for cmd in valid {
            assert!(is_host_mouse_command(cmd), "expected valid: {cmd}");
        }
        for bad in ["mouse-move:diagonal", "mouse-speed:3", "submit", ""] {
            assert!(!is_host_mouse_command(bad), "expected invalid: {bad}");
        }
    }
}
