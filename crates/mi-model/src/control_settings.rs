use crate::{
    document::{new_id, BindingNode},
    draft::Draft,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const LOCAL_SECTOR_LIMIT: u32 = 32;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ControlSettings {
    pub joystick_mode: String,
    pub active_sectors: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoder_hold_action_id: Option<String>,
}
impl ControlSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !["source", "logical_sectors", "mouse", "scroll"].contains(&self.joystick_mode.as_str())
        {
            return Err("摇杆模式：未知本地意图；原值保留".into());
        }
        if self.active_sectors > LOCAL_SECTOR_LIMIT {
            return Err("活动扇区数：应为 0–32（本机编辑器限制，非设备容量）".into());
        }
        Ok(())
    }
}
impl Draft {
    pub fn set_control_settings(
        &mut self,
        pid: &str,
        lid: &str,
        mut settings: ControlSettings,
    ) -> Result<(), String> {
        settings.validate()?;
        self.mutate(|d| {
            if !d
                .editable
                .graph
                .profiles
                .get(pid)
                .ok_or("配置不存在")?
                .layer_order
                .iter()
                .any(|id| id == lid)
            {
                return Err("层不属于所选配置".into());
            }
            if d.is_protected_oai_layer(lid) {
                return Err("受保护宿主层（原生 OAI）不支持修改控件设置".into());
            }
            if let Some(ref hold_id) = settings.encoder_hold_action_id {
                if !d.editable.graph.actions.contains_key(hold_id) {
                    return Err("旋钮长按动作引用不存在".into());
                }
            }
            let layer = d.editable.graph.layers.get_mut(lid).ok_or("层不存在")?;
            if let Some(old) = &layer.control_settings {
                old.validate()?;
                if settings.encoder_hold_action_id.is_none() {
                    settings.encoder_hold_action_id = old.encoder_hold_action_id.clone();
                }
            }
            let required_slots = if settings.joystick_mode == "logical_sectors" {
                16 + settings.active_sectors as usize
            } else if matches!(settings.joystick_mode.as_str(), "mouse" | "scroll") {
                layer.control_order.len().max(20)
            } else {
                layer.control_order.len()
            };
            for slot in layer.control_order.len()..required_slots {
                let id = new_id();
                layer.control_order.push(id.clone());
                layer.controls.insert(
                    id,
                    BindingNode {
                        logical_control: format!("logical-{slot}"),
                        original: json!("Empty"),
                        action_id: None,
                    },
                );
            }
            layer.control_settings = Some(settings);
            Ok(())
        })
    }
}
