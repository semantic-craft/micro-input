//! Offline `lights.preview` candidate encoding, without native sending authority.
//!
//! Input 1.1.0 passes the two-zone DTO directly as params (IA-17). Optional
//! fields stay omitted; no defaults, target binding, readback, or restoration
//! semantics are inferred. The known effects and numeric bounds below are
//! client policy, not evidence of firmware acceptance or visual behavior.
use crate::ProtoError;
use serde::Serialize;

/// Only the known DTO fields can enter a candidate. Unknown source fields must
/// remain in their original model; this type is not a lossless config editor.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
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

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct LightingPreviewCandidate {
    pub backlight: LightingZoneCandidate,
    pub underglow: LightingZoneCandidate,
}

impl LightingZoneCandidate {
    fn validate(&self) -> Result<(), ProtoError> {
        if self.effect.as_deref().is_some_and(|effect| {
            !matches!(
                effect,
                "off" | "solid" | "snake" | "rainbow" | "breath" | "gradient"
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

        // Borrowing a read-only method's variant cannot smuggle the two-zone
        // params through the production parameter validator either.
        let borrowed_method = RpcRequest::new(
            Method::DeviceStatus,
            serde_json::to_value(params).unwrap(),
            7,
        )
        .unwrap();
        assert!(validate_read_only(&borrowed_method).is_err());
    }
}
