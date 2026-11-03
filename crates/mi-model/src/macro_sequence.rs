//! Versioned local editing intent. Key labels and delays are not firmware tokens or limits.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LocalMacro {
    pub format_version: u32,
    pub events: Vec<MacroEvent>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum MacroEvent {
    Press { key: String },
    Release { key: String },
    Delay { ms: u32 },
}
impl LocalMacro {
    pub fn validate(&self) -> Result<(), String> {
        if self.format_version != 1 || self.events.is_empty() {
            return Err("本地宏格式不支持或步骤为空".into());
        }
        let mut held = BTreeMap::new();
        for (index, event) in self.events.iter().enumerate() {
            let valid = match event {
                MacroEvent::Press { key } => {
                    !key.trim().is_empty() && key.len() <= 256 && held.insert(key, index).is_none()
                }
                MacroEvent::Release { key } => held.remove(key).is_some(),
                MacroEvent::Delay { ms } => *ms > 0,
            };
            if !valid {
                return Err(format!(
                    "本地宏第 {} 步无效：按键需先按下再释放，延时需大于 0",
                    index + 1
                ));
            }
        }
        if let Some(index) = held.values().min() {
            return Err(format!("本地宏第 {} 步按下的按键尚未释放", index + 1));
        }
        Ok(())
    }
}
