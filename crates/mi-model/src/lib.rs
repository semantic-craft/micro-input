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
            let enc: Vec<_> = array(&l["layout"], "encoders")?
                .iter()
                .map(binding)
                .collect();
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

pub mod document;
