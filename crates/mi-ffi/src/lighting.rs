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
        if self
            .draft
            .lock()
            .unwrap()
            .draft
            .document
            .is_protected_oai_layer(&layer_id)
        {
            return Ok(
                json!({"zones":{},"can_add":false,"protected":true,"raw":"null"}).to_string(),
            );
        }
        let preview = self
            .operations
            .lock()
            .unwrap()
            .preview
            .as_ref()
            .and_then(|owned| {
                if owned.layer_id == layer_id && owned.preview.status()["phase"] == "Active" {
                    owned.preview.lighting()
                } else {
                    None
                }
            });
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
        let is_protected = self
            .draft
            .lock()
            .unwrap()
            .draft
            .document
            .is_protected_oai_layer(&layer_id);
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
                let editable = !is_protected
                    && edit(zone.name(), field, &value)
                        .is_ok_and(|(_, e)| lights.clone().edit(zone, e).is_ok());
                fields.insert(field.into(), json!({"value":value,"editable":editable}));
            }
            zones.insert(zone.name().into(), json!(fields));
        }
        Ok(
            json!({
                "zones": zones,
                "can_add": !is_protected && self.draft.lock().unwrap().draft.document.can_add_lighting(&layer_id),
                "is_protected": is_protected,
                "raw": serde_json::to_string(&lights).map_err(failure)?
            })
            .to_string(),
        )
    }
    pub fn add_layer_lighting(&self, layer_id: String) -> Result<String, BridgeError> {
        let _inspection = self.reserve_inspection()?;
        let mut store = self.draft.lock().unwrap();
        store
            .update(|draft| draft.add_lighting(&layer_id))
            .map_err(failure)?;
        document_response(&store, None)
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
    pub async fn begin_lighting_preview(&self, layer_id: String) -> Result<String, BridgeError> {
        if !self.write_capable() {
            return Err(failure("当前连接不支持即时预览"));
        }
        let _inspection = self.reserve_inspection()?;
        let session = self.current()?;
        let session_id = session.id;
        let (lights, document_id, revision) = {
            let store = self.draft.lock().unwrap();
            let doc = &store.draft.document;
            if doc.is_protected_oai_layer(&layer_id) {
                return Err(failure("受保护宿主层不支持静态 RGB 灯光即时预览"));
            }
            (
                doc.lighting(&layer_id).map_err(failure)?,
                doc.document_id.clone(),
                doc.revision,
            )
        };
        #[cfg(feature = "synthetic-backend")]
        let preview = if session.backend_name() == "synthetic/numbered-0x06" {
            mi_session::lighting_preview::Preview::begin(session).await
        } else {
            mi_session::lighting_preview::Preview::begin_native(session, lights).await
        }
        .map_err(failure)?;
        #[cfg(not(feature = "synthetic-backend"))]
        let preview = mi_session::lighting_preview::Preview::begin_native(session, lights)
            .await
            .map_err(failure)?;
        if !self.current().is_ok_and(|s| s.id == session_id) {
            preview.end().await;
            return Err(failure("预览连接已失效"));
        }
        let token = mi_model::document::new_id();
        self.operations.lock().unwrap().preview = Some(device_operations::OwnedPreview {
            token: token.clone(),
            layer_id,
            document_id,
            revision,
            preview,
        });
        Ok(token)
    }
    pub fn update_lighting_preview(
        &self,
        token: String,
        zone: String,
        field: String,
        value: String,
    ) -> Result<(), BridgeError> {
        let (zone, edit) = edit(&zone, &field, &value)?;
        let operations = self.operations.lock().unwrap();
        let owned = operations
            .preview
            .as_ref()
            .filter(|p| p.token == token)
            .ok_or_else(|| failure("预览已失效"))?;
        owned.preview.update(zone, edit).map_err(failure)
    }
    pub fn lighting_preview_status(&self) -> String {
        if let Some(owned) = &self.operations.lock().unwrap().preview {
            return owned.preview.status().to_string();
        }
        json!({"phase":"Idle","physical_visual":"NOT_RUN"}).to_string()
    }
    pub fn save_lighting_preview(&self, token: String) -> Result<String, BridgeError> {
        let operations = self.operations.lock().unwrap();
        let owned = operations
            .preview
            .as_ref()
            .filter(|p| p.token == token)
            .ok_or_else(|| failure("预览已失效"))?;
        let lights = owned
            .preview
            .lighting()
            .ok_or_else(|| failure("预览已结束"))?;
        let mut store = self.draft.lock().unwrap();
        if store.draft.document.document_id != owned.document_id
            || store.draft.document.revision != owned.revision
        {
            return Err(failure("草稿已变化，不能采纳旧预览"));
        }
        store
            .update(|draft| {
                draft.mutate(|doc| {
                    doc.editable
                        .graph
                        .layers
                        .get_mut(&owned.layer_id)
                        .ok_or("层已不存在")?
                        .lighting = Some(lights);
                    Ok(())
                })
            })
            .map_err(failure)?;
        document_response(&store, None)
    }
    pub async fn end_lighting_preview(&self, token: String) -> Result<String, BridgeError> {
        let preview = self
            .operations
            .lock()
            .unwrap()
            .preview
            .as_ref()
            .filter(|p| p.token == token)
            .map(|p| p.preview.clone())
            .ok_or_else(|| failure("预览已失效"))?;
        let result = preview.end().await;
        if result["phase"] == "Restored" || result["phase"] == "Stopped" {
            let mut operations = self.operations.lock().unwrap();
            if operations
                .preview
                .as_ref()
                .is_some_and(|p| p.token == token)
            {
                operations.preview = None;
            }
        }
        Ok(result.to_string())
    }
    pub async fn sync_four_app_hardware_lighting(
        &self,
        payload_json: String,
    ) -> Result<String, BridgeError> {
        let parsed: serde_json::Value = serde_json::from_str(&payload_json).map_err(failure)?;
        let parse_zone = |zone_val: &serde_json::Value| {
            let effect = zone_val["effect"].as_str().map(str::to_owned);
            let color = zone_val["color"]
                .as_str()
                .and_then(|hex| mi_model::lighting::color_from_hex(hex).ok())
                .or_else(|| {
                    zone_val["color"]
                        .as_u64()
                        .and_then(|n| u32::try_from(n).ok())
                });
            let brightness = zone_val["brightness"].as_f64();
            let speed = zone_val["speed"].as_f64();
            let magic = zone_val["magic"].as_f64();
            mi_proto::lighting_candidate::LightingZoneCandidate {
                effect,
                brightness,
                speed,
                magic,
                color,
            }
        };
        let rgb = mi_proto::lighting_candidate::LightingPreviewCandidate {
            backlight: parse_zone(&parsed["backlight"]),
            underglow: parse_zone(&parsed["underglow"]),
        };
        let mut entries = Vec::new();
        if let Some(items) = parsed["per_key"].as_array() {
            for item in items {
                let Some(k) = item["k"]
                    .as_str()
                    .or_else(|| item["binding_id"].as_str())
                    .map(str::to_owned)
                else {
                    continue;
                };
                let c = item["c"]
                    .as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .or_else(|| {
                        item["color"]
                            .as_str()
                            .and_then(|hex| mi_model::lighting::color_from_hex(hex).ok())
                    })
                    .unwrap_or(0);
                entries.push(mi_proto::lighting_candidate::OaiKeyStatusEntry { k, c });
            }
        }
        if !self.write_capable() {
            return Ok(json!({
                "synced": false,
                "reason": "write_incapable_or_offline",
                "keys": entries.len()
            })
            .to_string());
        }
        let Ok(session) = self.current() else {
            return Ok(json!({
                "synced": false,
                "reason": "no_active_session",
                "keys": entries.len()
            })
            .to_string());
        };
        let rgb_ok = session.send_oai_rgbcfg(&rgb).await.is_ok();
        let th_ok = if entries.is_empty() {
            false
        } else {
            session.send_oai_thstatus(&entries).await.is_ok()
        };
        Ok(json!({
            "synced": rgb_ok || th_ok,
            "rgbcfg": rgb_ok,
            "thstatus": th_ok,
            "keys": entries.len()
        })
        .to_string())
    }
}
