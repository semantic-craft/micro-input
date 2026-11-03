//! Local edits use the model's validation and one atomic document update.
use super::*;
use mi_model::lighting::{Edit, Zone};
use serde_json::json;
fn edit(zone: &str, field: &str, value: &str) -> Result<(Zone, Edit), BridgeError> {
    let zone = match zone {
        "backlight" => Zone::Backlight,
        "underglow" => Zone::Underglow,
        _ => return Err(failure("未知灯区")),
    };
    let edit = match field {
        "effect" => Edit::Effect(serde_json::from_value(json!(value)).map_err(failure)?),
        "color" => Edit::Color(mi_model::lighting::color_from_hex(value).map_err(failure)?),
        "brightness" => Edit::Brightness(value.parse().map_err(failure)?),
        "speed" => Edit::Speed(value.parse().map_err(failure)?),
        "magic" => Edit::Magic(value.parse().map_err(failure)?),
        _ => return Err(failure("未知灯光参数")),
    };
    Ok((zone, edit))
}
#[uniffi::export(async_runtime = "tokio")]
impl MicroInput {
    pub fn layer_lighting(&self, layer_id: String) -> Result<String, BridgeError> {
        #[cfg(feature = "synthetic-backend")]
        let preview = self
            .operations
            .lock()
            .unwrap()
            .preview
            .as_ref()
            .and_then(|(_, p)| {
                if p.status()["phase"] == "Active" {
                    p.lighting()
                } else {
                    None
                }
            });
        #[cfg(not(feature = "synthetic-backend"))]
        let preview = None;
        let lights = match preview {
            Some(lights) => lights,
            None => self
                .draft
                .lock()
                .unwrap()
                .draft
                .document
                .lighting(&layer_id)
                .map_err(failure)?,
        };
        let mut zones = serde_json::Map::new();
        for zone in [Zone::Backlight, Zone::Underglow] {
            let raw = lights.zone(zone);
            let mut fields = serde_json::Map::new();
            for field in ["effect", "color", "brightness", "speed", "magic"] {
                let value = match field {
                    "effect" => raw[field].as_str().map(str::to_owned),
                    "color" => raw[field]
                        .as_u64()
                        .and_then(|n| u32::try_from(n).ok())
                        .and_then(|n| mi_model::lighting::color_to_hex(n).ok()),
                    _ => raw[field]
                        .as_f64()
                        .filter(|n| n.is_finite() && (0.0..=1.0).contains(n))
                        .map(|n| format!("{:.0}", n * 100.0)),
                }
                .unwrap_or_else(|| raw[field].to_string());
                let editable = edit(zone.name(), field, &value)
                    .is_ok_and(|(_, e)| lights.clone().edit(zone, e).is_ok());
                fields.insert(field.into(), json!({"value":value,"editable":editable}));
            }
            zones.insert(zone.name().into(), json!(fields));
        }
        Ok(
            json!({"zones":zones,"raw":serde_json::to_string(&lights).map_err(failure)?})
                .to_string(),
        )
    }
    pub async fn edit_lighting(
        &self,
        profile_id: String,
        layer_id: String,
        zone: String,
        field: String,
        value: String,
        all_layers: bool,
    ) -> Result<String, BridgeError> {
        let (zone, edit) = edit(&zone, &field, &value)?;
        let mut store = self.draft.lock().unwrap();
        store
            .update(|draft| draft.edit_lighting(&profile_id, &layer_id, zone, edit, all_layers))
            .map_err(failure)?;
        document_response(&store, None)
    }
    pub async fn begin_lighting_preview(&self) -> Result<String, BridgeError> {
        #[cfg(feature = "synthetic-backend")]
        {
            if !self.write_capable() {
                return Err(failure("原生即时预览未验收"));
            }
            let _inspection = self.reserve_inspection()?;
            let session = self.current()?;
            let identity = session
                .read_identity_info()
                .await
                .map_err(session_failure)?;
            if identity.version.as_deref() != Some("synthetic-0.6.3")
                || identity.profile_index != Some(0)
                || identity.layer_index != Some(0)
            {
                return Err(failure("活动来源未核实；不开始预览"));
            }
            let preview = mi_session::lighting_preview::Preview::begin(session)
                .await
                .map_err(failure)?;
            let token = mi_model::document::new_id();
            self.operations.lock().unwrap().preview = Some((token.clone(), preview));
            Ok(token)
        }
        #[cfg(not(feature = "synthetic-backend"))]
        {
            Err(failure("原生即时预览未验收"))
        }
    }
    pub fn update_lighting_preview(
        &self,
        token: String,
        zone: String,
        field: String,
        value: String,
    ) -> Result<(), BridgeError> {
        let (zone, edit) = edit(&zone, &field, &value)?;
        #[cfg(feature = "synthetic-backend")]
        {
            let operations = self.operations.lock().unwrap();
            let (_, preview) = operations
                .preview
                .as_ref()
                .filter(|(t, _)| t == &token)
                .ok_or_else(|| failure("预览已失效"))?;
            preview.update(zone, edit).map_err(failure)
        }
        #[cfg(not(feature = "synthetic-backend"))]
        {
            let _ = (token, zone, edit);
            Err(failure("原生即时预览未验收"))
        }
    }
    pub fn lighting_preview_status(&self) -> String {
        #[cfg(feature = "synthetic-backend")]
        if let Some((_, preview)) = &self.operations.lock().unwrap().preview {
            return preview.status().to_string();
        }
        json!({"phase":"Idle","physical_visual":"NOT_RUN"}).to_string()
    }
    pub async fn end_lighting_preview(&self, token: String) -> Result<String, BridgeError> {
        #[cfg(feature = "synthetic-backend")]
        {
            let preview = self
                .operations
                .lock()
                .unwrap()
                .preview
                .as_ref()
                .filter(|(t, _)| t == &token)
                .map(|(_, p)| p.clone())
                .ok_or_else(|| failure("预览已失效"))?;
            let result = preview.end().await;
            if result["phase"] == "Restored" {
                let mut operations = self.operations.lock().unwrap();
                if operations
                    .preview
                    .as_ref()
                    .is_some_and(|(t, _)| t == &token)
                {
                    operations.preview = None;
                }
            }
            // Unknown keeps the operation gate closed. Reconnection never restores or replays.
            Ok(result.to_string())
        }
        #[cfg(not(feature = "synthetic-backend"))]
        {
            let _ = token;
            Err(failure("原生即时预览未验收"))
        }
    }
}
