//! Local Layer target state. Device writes are compiled separately against original bytes.
use crate::{document::LocalDocumentV2, draft::Draft, Lights};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Lighting {
    pub backlight: Value,
    pub underglow: Value,
}
#[derive(Debug, Clone, Copy)]
pub enum Zone {
    Backlight,
    Underglow,
}
impl Zone {
    pub fn name(self) -> &'static str {
        match self {
            Self::Backlight => "backlight",
            Self::Underglow => "underglow",
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    Off,
    Solid,
    Snake,
    Rainbow,
    Breath,
    Gradient,
}
#[derive(Debug, Clone)]
pub enum Edit {
    Effect(Effect),
    Brightness(u8),
    Speed(u8),
    Magic(u8),
    Color(u32),
}
pub fn color_from_hex(text: &str) -> Result<u32, String> {
    let s = text.strip_prefix('#').unwrap_or(text);
    if s.len() != 6 || !s.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("颜色须为六位 RGB hex".into());
    }
    u32::from_str_radix(s, 16).map_err(|_| "颜色无效".into())
}
pub fn color_to_hex(value: u32) -> Result<String, String> {
    if value > 0xffffff {
        return Err("RGB 超出 24 位".into());
    }
    Ok(format!("#{value:06X}"))
}
pub(crate) fn known_value(key: &str, value: &Value) -> bool {
    match key {
        "effect" => serde_json::from_value::<Effect>(value.clone()).is_ok(),
        "color" => value.as_u64().is_some_and(|n| n <= 0xffffff),
        "brightness" | "speed" | "magic" => value
            .as_f64()
            .is_some_and(|n| n.is_finite() && (0.0..=1.0).contains(&n)),
        _ => false,
    }
}
impl Lighting {
    pub fn creator_micro_2_defaults() -> Self {
        Self {
            backlight: json!({"effect":"solid","brightness":1.0,"speed":0.5,"magic":1.0,"color":16777215}),
            underglow: json!({"effect":"rainbow","brightness":1.0,"speed":0.55,"magic":1.0,"color":16777215}),
        }
    }
    pub(crate) fn complete_known(&self) -> bool {
        [Zone::Backlight, Zone::Underglow].into_iter().all(|zone| {
            self.zone(zone).as_object().is_some_and(|fields| {
                fields.len() == 5
                    && ["effect", "brightness", "speed", "magic", "color"]
                        .into_iter()
                        .all(|key| fields.get(key).is_some_and(|v| known_value(key, v)))
            })
        })
    }

    pub fn from_lights(lights: &Lights) -> Self {
        Self {
            backlight: lights.backlight.clone(),
            underglow: lights.underglow.clone(),
        }
    }
    pub fn zone(&self, zone: Zone) -> &Value {
        match zone {
            Zone::Backlight => &self.backlight,
            Zone::Underglow => &self.underglow,
        }
    }
    pub fn edit(&mut self, zone: Zone, edit: Edit) -> Result<(), String> {
        let raw = match zone {
            Zone::Backlight => &mut self.backlight,
            Zone::Underglow => &mut self.underglow,
        };
        let old = raw.as_object().ok_or("灯区结构缺失或未知；不能补默认值")?;
        let effect: Effect =
            serde_json::from_value(old.get("effect").cloned().unwrap_or(Value::Null))
                .map_err(|_| "未知灯效仅保留，不可编辑")?;
        let (key, value) = match edit {
            Edit::Effect(v) => ("effect", json!(v)),
            Edit::Color(v)
                if effect != Effect::Off && effect != Effect::Rainbow && v <= 0xffffff =>
            {
                ("color", json!(v))
            }
            Edit::Brightness(v) if effect != Effect::Off && v <= 100 => {
                ("brightness", json!(f64::from(v) / 100.0))
            }
            Edit::Speed(v)
                if !matches!(effect, Effect::Off | Effect::Solid | Effect::Gradient)
                    && v <= 100 =>
            {
                ("speed", json!(f64::from(v) / 100.0))
            }
            Edit::Magic(v)
                if !matches!(effect, Effect::Off | Effect::Solid | Effect::Snake) && v <= 100 =>
            {
                ("magic", json!(f64::from(v) / 100.0))
            }
            _ => return Err("参数超界或当前效果不允许编辑该参数".into()),
        };
        if !old.get(key).is_some_and(|v| known_value(key, v)) {
            return Err("字段缺失或原值未知；不能隐式创建或覆盖灯光结构".into());
        }
        raw[key] = value;
        Ok(())
    }
}
impl LocalDocumentV2 {
    pub fn can_add_lighting(&self, lid: &str) -> bool {
        if self.is_protected_oai_layer(lid) {
            return false;
        }
        let Some(layer) = self.editable.graph.layers.get(lid) else {
            return false;
        };
        if layer.lighting.is_some() {
            return false;
        }
        let Some(source) = self.editable.source_refs.get(lid) else {
            return false;
        };
        let Ok(config) = crate::comparison::base_config(self) else {
            return false;
        };
        let Some(profiles) = config.raw["profiles"].as_array() else {
            return false;
        };
        let profiles: Vec<_> = profiles
            .iter()
            .filter(|p| p["id"] == source["profile_source_id"])
            .collect();
        if profiles.len() != 1 {
            return false;
        }
        let Some(layers) = profiles[0]["layers"].as_array() else {
            return false;
        };
        let layers: Vec<_> = layers
            .iter()
            .filter(|l| l["id"] == source["source_id"])
            .collect();
        layers.len() == 1
            && layers[0]
                .as_object()
                .is_some_and(|l| !l.contains_key("lights"))
    }

    pub fn lighting(&self, lid: &str) -> Result<Lighting, String> {
        let layer = self.editable.graph.layers.get(lid).ok_or("层不存在")?;
        if let Some(target) = &layer.lighting {
            return Ok(target.clone());
        }
        let source = self.editable.source_refs.get(lid).ok_or("层没有来源灯光")?;
        let config = crate::comparison::base_config(self)?;
        let original = config
            .profiles
            .iter()
            .find(|p| p.id == source["profile_source_id"])
            .and_then(|p| p.layers.iter().find(|l| l.id == source["source_id"]))
            .ok_or("层来源不存在")?;
        Ok(Lighting::from_lights(&original.lights))
    }
}
impl Draft {
    pub fn add_lighting(&mut self, lid: &str) -> Result<(), String> {
        self.mutate(|doc| {
            if doc.is_protected_oai_layer(lid) {
                return Err("受保护宿主层不支持添加或修改静态 RGB 灯光".into());
            }
            if !doc.can_add_lighting(lid) {
                return Err("只能为来源中缺少 lights 的层添加灯光；已有或未知结构保持原值".into());
            }
            doc.editable.graph.layers.get_mut(lid).unwrap().lighting =
                Some(Lighting::creator_micro_2_defaults());
            Ok(())
        })
    }

    /// One history entry, including all-layer edits. Any invalid target rejects the whole edit.
    pub fn edit_lighting(
        &mut self,
        pid: &str,
        lid: &str,
        zone: Zone,
        edit: Edit,
        all_layers: bool,
    ) -> Result<(), String> {
        self.mutate(|doc| {
            let order = &doc
                .editable
                .graph
                .profiles
                .get(pid)
                .ok_or("配置不存在")?
                .layer_order;
            if !order.iter().any(|id| id == lid) {
                return Err("层不属于所选配置".into());
            }
            if doc.is_protected_oai_layer(lid) {
                return Err("受保护宿主层不支持添加或修改静态 RGB 灯光".into());
            }
            let targets: Vec<String> = if all_layers {
                order
                    .iter()
                    .filter(|id| !doc.is_protected_oai_layer(id))
                    .cloned()
                    .collect()
            } else {
                vec![lid.to_owned()]
            };
            for id in targets {
                let mut target = doc.lighting(&id)?;
                let before = target.clone();
                target.edit(zone, edit.clone())?;
                if target != before {
                    doc.editable.graph.layers.get_mut(&id).unwrap().lighting = Some(target);
                }
            }
            Ok(())
        })
    }
    pub fn reset_lighting(&mut self, lid: &str) -> Result<(), String> {
        self.mutate(|doc| {
            if doc.is_protected_oai_layer(lid) {
                return Err("受保护宿主层不支持重置静态 RGB 灯光".into());
            }
            doc.editable
                .graph
                .layers
                .get_mut(lid)
                .ok_or("层不存在")?
                .lighting = None;
            Ok(())
        })
    }
}
