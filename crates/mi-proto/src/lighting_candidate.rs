//! Fixed `lights.preview` encoding; sending authority belongs to the session preview.
//!
//! Input 1.1.0 passes the two-zone DTO directly as params (IA-17). Optional
//! fields stay omitted; no defaults, target binding, readback, or restoration
//! semantics are inferred. The known effects and numeric bounds below are
//! client policy, not evidence of firmware acceptance or visual behavior.
use crate::ProtoError;
use serde::{Deserialize, Serialize};

/// Only the known DTO fields can enter a candidate. Unknown source fields must
/// remain in their original model; this type is not a lossless config editor.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LightingZoneCandidate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effect: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brightness: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub magic: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LightingPreviewCandidate {
    pub backlight: LightingZoneCandidate,
    pub underglow: LightingZoneCandidate,
}

impl LightingZoneCandidate {
    fn validate(&self) -> Result<(), ProtoError> {
        if self.effect.as_deref().is_some_and(|effect| {
            !matches!(
                effect,
                "off" | "solid" | "snake" | "rainbow" | "breath" | "gradient" | "shallow_breath"
            )
        }) || [self.brightness, self.speed, self.magic]
            .into_iter()
            .flatten()
            .any(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
            || self.color.is_some_and(|color| color > 0xff_ffff)
        {
            return Err(ProtoError::Invalid(
                "lighting candidate exceeds known client parameter policy".into(),
            ));
        }
        Ok(())
    }
}

/// Produce candidate bytes only. This does not grant a `Method` variant, a
/// transport permit, or evidence that the request can be safely applied.
///
/// The production request constructor still accepts only its closed read-only
/// enum, so candidate methods cannot enter it as strings:
/// ```compile_fail
/// use mi_proto::RpcRequest;
/// let request = RpcRequest::new("lights.preview", serde_json::Value::Null, 7);
/// ```
pub fn encode_lighting_preview_candidate(
    params: &LightingPreviewCandidate,
    id: u16,
) -> Result<Vec<u8>, ProtoError> {
    if id >= 999 {
        return Err(ProtoError::Invalid("id must be < 999".into()));
    }
    params.backlight.validate()?;
    params.underglow.validate()?;
    #[derive(Serialize)]
    struct Request<'a> {
        id: u16,
        method: &'static str,
        params: &'a LightingPreviewCandidate,
    }
    serde_json::to_vec(&Request {
        id,
        method: "lights.preview",
        params,
    })
    .map_err(|error| ProtoError::Invalid(error.to_string()))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OaiKeyStatusEntry {
    pub k: String,
    pub c: u32,
    #[serde(default = "default_key_brightness")]
    pub b: f64,
    #[serde(default = "default_key_effect")]
    pub e: u8,
    #[serde(default = "default_key_speed")]
    pub s: f64,
}

fn default_key_brightness() -> f64 {
    0.85
}

fn default_key_effect() -> u8 {
    1
}

fn default_key_speed() -> f64 {
    0.5
}

impl OaiKeyStatusEntry {
    pub fn new(k: impl Into<String>, c: u32) -> Self {
        let b = if c == 0 { 0.0 } else { 0.85 };
        let e = if c == 0 { 0 } else { 1 };
        Self {
            k: k.into(),
            c,
            b,
            e,
            s: 0.5,
        }
    }

    pub fn with_style(k: impl Into<String>, c: u32, b: f64, e: u8, s: f64) -> Self {
        Self {
            k: k.into(),
            c,
            b,
            e,
            s,
        }
    }

    pub fn firmware_key_id(&self) -> Option<u8> {
        self.k
            .strip_prefix("AG")
            .filter(|digits| digits.len() == 2 && digits.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|digits| digits.parse::<u8>().ok())
            .filter(|&id| id < 20)
    }

    fn validate(&self) -> Result<(), ProtoError> {
        if self.firmware_key_id().is_none()
            || self.c > 0xff_ffff
            || !self.b.is_finite()
            || !(0.0..=1.0).contains(&self.b)
            || self.e > 6
            || !self.s.is_finite()
            || !(0.0..=1.0).contains(&self.s)
        {
            return Err(ProtoError::Invalid(
                "v.oai.thstatus key or color exceeds firmware v0.6.3 policy".into(),
            ));
        }
        Ok(())
    }
}

pub fn encode_oai_thstatus_candidate(
    entries: &[OaiKeyStatusEntry],
    id: u16,
) -> Result<Vec<u8>, ProtoError> {
    if id >= 999 {
        return Err(ProtoError::Invalid("id must be < 999".into()));
    }
    if entries.is_empty() || entries.len() > 20 {
        return Err(ProtoError::Invalid(
            "v.oai.thstatus supports 1..=20 key status entries".into(),
        ));
    }
    #[derive(Serialize)]
    struct WireEntry<'a> {
        id: u8,
        k: &'a str,
        c: u32,
        b: f64,
        e: u8,
        s: f64,
        sk: bool,
        sa: bool,
    }
    let mut wire_entries = Vec::with_capacity(entries.len());
    for entry in entries {
        entry.validate()?;
        let key_id = entry.firmware_key_id().ok_or_else(|| {
            ProtoError::Invalid("v.oai.thstatus key exceeds firmware v0.6.3 policy".into())
        })?;
        wire_entries.push(WireEntry {
            id: key_id,
            k: &entry.k,
            c: entry.c,
            b: entry.b,
            e: entry.e,
            s: entry.s,
            sk: false,
            sa: false,
        });
    }
    #[derive(Serialize)]
    struct Request<'a> {
        id: u16,
        method: &'static str,
        params: Vec<WireEntry<'a>>,
    }
    serde_json::to_vec(&Request {
        id,
        method: "v.oai.thstatus",
        params: wire_entries,
    })
    .map_err(|error| ProtoError::Invalid(error.to_string()))
}

fn firmware_effect_index(effect: Option<&str>) -> u8 {
    match effect {
        Some("off") => 0,
        Some("solid") | None => 1,
        Some("snake") => 2,
        Some("rainbow") => 3,
        Some("breath") => 4,
        Some("gradient") => 5,
        Some("shallow_breath") => 6,
        _ => 1,
    }
}

pub fn encode_oai_rgbcfg_candidate(
    params: &LightingPreviewCandidate,
    id: u16,
) -> Result<Vec<u8>, ProtoError> {
    if id >= 999 {
        return Err(ProtoError::Invalid("id must be < 999".into()));
    }
    params.backlight.validate()?;
    params.underglow.validate()?;
    #[derive(Serialize)]
    struct WireZone {
        e: u8,
        b: f64,
        s: f64,
        m: f64,
        c: u32,
    }
    impl WireZone {
        fn from_candidate(z: &LightingZoneCandidate) -> Self {
            Self {
                e: firmware_effect_index(z.effect.as_deref()),
                b: z.brightness.unwrap_or(0.75),
                s: z.speed.unwrap_or(0.5),
                m: z.magic.unwrap_or(0.5),
                c: z.color.unwrap_or(0),
            }
        }
    }
    #[derive(Serialize)]
    struct WireParams {
        keys: WireZone,
        ambient: WireZone,
    }
    #[derive(Serialize)]
    struct Request {
        id: u16,
        method: &'static str,
        params: WireParams,
    }
    serde_json::to_vec(&Request {
        id,
        method: "v.oai.rgbcfg",
        params: WireParams {
            keys: WireZone::from_candidate(&params.backlight),
            ambient: WireZone::from_candidate(&params.underglow),
        },
    })
    .map_err(|error| ProtoError::Invalid(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{validate_read_only, Framer, Method, NativeFramer, RpcRequest};
    use serde_json::{json, Value};

    #[test]
    fn self_authored_two_zone_vector_has_only_the_observed_envelope() {
        let params = LightingPreviewCandidate {
            backlight: LightingZoneCandidate {
                effect: Some("solid".into()),
                brightness: Some(0.75),
                speed: Some(0.25),
                magic: Some(0.5),
                color: Some(0x12_3456),
            },
            underglow: LightingZoneCandidate {
                effect: Some("gradient".into()),
                brightness: Some(0.0),
                speed: Some(1.0),
                magic: Some(0.0),
                color: Some(0xff_ffff),
            },
        };
        let bytes = encode_lighting_preview_candidate(&params, 998).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap(),
            json!({"id":998,"method":"lights.preview","params":{
                "backlight":{"effect":"solid","brightness":0.75,"speed":0.25,"magic":0.5,"color":1193046},
                "underglow":{"effect":"gradient","brightness":0.0,"speed":1.0,"magic":0.0,"color":16777215}
            }})
        );
    }

    #[test]
    fn optional_fields_stay_absent_and_all_known_effects_are_client_candidates() {
        for effect in ["off", "solid", "snake", "rainbow", "breath", "gradient"] {
            let params = LightingPreviewCandidate {
                backlight: LightingZoneCandidate {
                    effect: Some(effect.into()),
                    color: Some(0),
                    ..Default::default()
                },
                ..Default::default()
            };
            let bytes = encode_lighting_preview_candidate(&params, 0).unwrap();
            assert_eq!(
                serde_json::from_slice::<Value>(&bytes).unwrap(),
                json!({"id":0,"method":"lights.preview","params":{
                    "backlight":{"effect":effect,"color":0},"underglow":{}
                }})
            );
        }
    }

    #[test]
    fn invalid_ids_effects_and_each_numeric_field_are_rejected_in_either_zone() {
        for id in [999, u16::MAX] {
            assert!(encode_lighting_preview_candidate(&Default::default(), id).is_err());
        }
        for effect in ["", "Solid", "unknown", "lights.preview"] {
            assert_invalid_zone(LightingZoneCandidate {
                effect: Some(effect.into()),
                ..Default::default()
            });
        }
        for value in [-0.01, 1.01, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for field in ["brightness", "speed", "magic"] {
                let mut zone = LightingZoneCandidate::default();
                match field {
                    "brightness" => zone.brightness = Some(value),
                    "speed" => zone.speed = Some(value),
                    _ => zone.magic = Some(value),
                }
                assert_invalid_zone(zone);
            }
        }
        for color in [0x100_0000, u32::MAX] {
            assert_invalid_zone(LightingZoneCandidate {
                color: Some(color),
                ..Default::default()
            });
        }
    }

    fn assert_invalid_zone(zone: LightingZoneCandidate) {
        for params in [
            LightingPreviewCandidate {
                backlight: zone.clone(),
                ..Default::default()
            },
            LightingPreviewCandidate {
                underglow: zone.clone(),
                ..Default::default()
            },
        ] {
            assert!(encode_lighting_preview_candidate(&params, 7).is_err());
        }
    }

    #[test]
    fn framing_candidate_bytes_does_not_authorize_production_request() {
        let params = LightingPreviewCandidate {
            backlight: LightingZoneCandidate {
                effect: Some("solid".into()),
                brightness: Some(0.5),
                color: Some(0x12_3456),
                ..Default::default()
            },
            ..Default::default()
        };
        let bytes = encode_lighting_preview_candidate(&params, 7).unwrap();
        let reports = NativeFramer::default().encode(&bytes).unwrap();
        assert!(reports.len() > 1);
        let mut reader = NativeFramer::default();
        let mut lines = Vec::new();
        for report in reports {
            lines.extend(reader.feed(&report).unwrap());
        }
        assert_eq!(lines, vec![bytes]);

        let borrowed_method = RpcRequest::new(
            Method::DeviceStatus,
            serde_json::to_value(params).unwrap(),
            7,
        )
        .unwrap();
        assert!(validate_read_only(&borrowed_method).is_err());
    }

    #[test]
    fn oai_thstatus_and_rgbcfg_candidates_encode_firmware_v063_envelope() {
        let entries = vec![
            OaiKeyStatusEntry::new("AG00", 0xFFB800),
            OaiKeyStatusEntry::new("AG07", 0x00FF66),
            OaiKeyStatusEntry::new("AG12", 0x0066FF),
        ];
        let th_bytes = encode_oai_thstatus_candidate(&entries, 12).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&th_bytes).unwrap(),
            json!({
                "id": 12,
                "method": "v.oai.thstatus",
                "params": [
                    {"id": 0, "k": "AG00", "c": 0xFFB800, "b": 0.85, "e": 1, "s": 0.5, "sk": false, "sa": false},
                    {"id": 7, "k": "AG07", "c": 0x00FF66, "b": 0.85, "e": 1, "s": 0.5, "sk": false, "sa": false},
                    {"id": 12, "k": "AG12", "c": 0x0066FF, "b": 0.85, "e": 1, "s": 0.5, "sk": false, "sa": false}
                ]
            })
        );

        let rgb = LightingPreviewCandidate {
            backlight: LightingZoneCandidate {
                effect: Some("breath".into()),
                brightness: Some(0.95),
                speed: Some(0.75),
                magic: Some(0.50),
                color: Some(0xFFB800),
            },
            underglow: LightingZoneCandidate {
                effect: Some("breath".into()),
                brightness: Some(0.95),
                speed: Some(0.75),
                magic: Some(0.50),
                color: Some(0xFFB800),
            },
        };
        let rgb_bytes = encode_oai_rgbcfg_candidate(&rgb, 13).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&rgb_bytes).unwrap(),
            json!({
                "id": 13,
                "method": "v.oai.rgbcfg",
                "params": {
                    "keys": {"e": 4, "b": 0.95, "s": 0.75, "m": 0.5, "c": 0xFFB800},
                    "ambient": {"e": 4, "b": 0.95, "s": 0.75, "m": 0.5, "c": 0xFFB800}
                }
            })
        );
    }

    #[test]
    fn oai_thstatus_accepts_ag00_through_ag19_and_rejects_non_led_or_overflow_entries() {
        assert_eq!(OaiKeyStatusEntry::new("AG00", 0).firmware_key_id(), Some(0));
        assert_eq!(
            OaiKeyStatusEntry::new("AG19", 0).firmware_key_id(),
            Some(19)
        );
        assert_eq!(OaiKeyStatusEntry::new("AG20", 0).firmware_key_id(), None);
        assert_eq!(OaiKeyStatusEntry::new("AG+1", 0).firmware_key_id(), None);
        assert_eq!(OaiKeyStatusEntry::new("ENC_CLK", 0).firmware_key_id(), None);
        assert_eq!(OaiKeyStatusEntry::new("ENC_CC", 0).firmware_key_id(), None);
        assert_eq!(OaiKeyStatusEntry::new("ENC_CW", 0).firmware_key_id(), None);

        let twenty: Vec<OaiKeyStatusEntry> = (0..20)
            .map(|id| OaiKeyStatusEntry::new(format!("AG{id:02}"), 0x00FF66))
            .collect();
        let bytes = encode_oai_thstatus_candidate(&twenty, 42).unwrap();
        let decoded: Value = serde_json::from_slice(&bytes).unwrap();
        let params = decoded["params"].as_array().unwrap();
        assert_eq!(params.len(), 20);
        assert_eq!(params[0]["id"], 0);
        assert_eq!(params[0]["k"], "AG00");
        assert_eq!(params[19]["id"], 19);
        assert_eq!(params[19]["k"], "AG19");

        let mut twenty_one = twenty.clone();
        twenty_one.push(OaiKeyStatusEntry::new("AG00", 0));
        let err = encode_oai_thstatus_candidate(&twenty_one, 42)
            .unwrap_err()
            .to_string();
        assert!(err.contains("1..=20"), "{err}");

        assert!(encode_oai_thstatus_candidate(&[], 42).is_err());
        assert!(
            encode_oai_thstatus_candidate(&[OaiKeyStatusEntry::new("ENC_CLK", 0xFFFFFF)], 42)
                .is_err()
        );
    }
}
