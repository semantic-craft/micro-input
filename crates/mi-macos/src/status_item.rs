use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub const MAX_PRESET_FILE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum StatusIconPrimitive {
    KeyboardBodyRoundedRect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        radius: f64,
        stroke_width: f64,
    },
    KeycapCell {
        row: u8,
        col: u8,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        radius: f64,
    },
    Spacebar2U {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        radius: f64,
    },
    RotaryKnobCircle {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        stroke_width: f64,
    },
    JoystickRect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        radius: f64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusItemIconSpec {
    pub width_pt: f64,
    pub height_pt: f64,
    pub is_template: bool,
    pub accessibility_description: String,
    pub primitives: [StatusIconPrimitive; 10],
}

impl StatusItemIconSpec {
    pub fn canonical() -> Self {
        Self {
            width_pt: 20.0,
            height_pt: 18.0,
            is_template: true,
            accessibility_description: "Micro Input 键盘".to_string(),
            primitives: [
                StatusIconPrimitive::KeyboardBodyRoundedRect {
                    x: 1.0,
                    y: 2.5,
                    width: 18.0,
                    height: 13.0,
                    radius: 2.0,
                    stroke_width: 1.25,
                },
                StatusIconPrimitive::KeycapCell {
                    row: 0,
                    col: 0,
                    x: 3.5,
                    y: 10.5,
                    width: 2.0,
                    height: 2.0,
                    radius: 0.35,
                },
                StatusIconPrimitive::KeycapCell {
                    row: 0,
                    col: 1,
                    x: 7.0,
                    y: 10.5,
                    width: 2.0,
                    height: 2.0,
                    radius: 0.35,
                },
                StatusIconPrimitive::KeycapCell {
                    row: 0,
                    col: 2,
                    x: 10.5,
                    y: 10.5,
                    width: 2.0,
                    height: 2.0,
                    radius: 0.35,
                },
                StatusIconPrimitive::KeycapCell {
                    row: 1,
                    col: 0,
                    x: 3.5,
                    y: 7.0,
                    width: 2.0,
                    height: 2.0,
                    radius: 0.35,
                },
                StatusIconPrimitive::KeycapCell {
                    row: 1,
                    col: 1,
                    x: 7.0,
                    y: 7.0,
                    width: 2.0,
                    height: 2.0,
                    radius: 0.35,
                },
                StatusIconPrimitive::KeycapCell {
                    row: 1,
                    col: 2,
                    x: 10.5,
                    y: 7.0,
                    width: 2.0,
                    height: 2.0,
                    radius: 0.35,
                },
                StatusIconPrimitive::Spacebar2U {
                    x: 5.0,
                    y: 4.5,
                    width: 7.5,
                    height: 1.25,
                    radius: 0.4,
                },
                StatusIconPrimitive::RotaryKnobCircle {
                    x: 14.0,
                    y: 10.0,
                    width: 3.0,
                    height: 3.0,
                    stroke_width: 1.0,
                },
                StatusIconPrimitive::JoystickRect {
                    x: 14.5,
                    y: 5.0,
                    width: 2.0,
                    height: 3.0,
                    radius: 0.4,
                },
            ],
        }
    }
}

impl Default for StatusItemIconSpec {
    fn default() -> Self {
        Self::canonical()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetFileEnvelope {
    pub raw_utf8: String,
    pub byte_len: usize,
}

impl PresetFileEnvelope {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_PRESET_FILE_BYTES {
            return Err("预设文件超过 4 MiB 上限，拒绝读取".to_string());
        }
        let raw_utf8 = std::str::from_utf8(bytes)
            .map_err(|_| "预设文件不是合法的 UTF-8 JSON 文本".to_string())?
            .to_string();
        Ok(Self {
            byte_len: bytes.len(),
            raw_utf8,
        })
    }

    pub fn read_path(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|e| format!("读取预设文件失败：{e}"))?;
        let mut buf = Vec::new();
        file.take((MAX_PRESET_FILE_BYTES + 1) as u64)
            .read_to_end(&mut buf)
            .map_err(|e| format!("读取预设文件失败：{e}"))?;
        Self::from_bytes(&buf)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusItemSnapshot {
    pub icon: StatusItemIconSpec,
    pub active_profile_name: String,
    pub routing_mode_label: String,
    pub host_execution_enabled: bool,
    pub auto_follow_enabled: bool,
    pub manual_override: bool,
    pub radial_menu_enabled: bool,
    pub cheat_sheet_visible: bool,
    pub pinned_favorite_ids: [Option<String>; 4],
}

pub trait StatusItemHost: Send + Sync {
    fn sync_status_item(&self, snapshot: StatusItemSnapshot);
    fn latest_snapshot(&self) -> Option<StatusItemSnapshot>;
    fn snapshots(&self) -> Vec<StatusItemSnapshot>;
}

impl dyn StatusItemHost {
    pub fn new_default() -> Arc<dyn StatusItemHost> {
        Arc::new(RecordingStatusItemHost::new())
    }
}

#[derive(Debug, Default, Clone)]
pub struct RecordingStatusItemHost {
    snapshots: Arc<Mutex<Vec<StatusItemSnapshot>>>,
}

impl RecordingStatusItemHost {
    pub fn new() -> Self {
        Self::default()
    }
}

impl StatusItemHost for RecordingStatusItemHost {
    fn sync_status_item(&self, snapshot: StatusItemSnapshot) {
        if let Ok(mut guard) = self.snapshots.lock() {
            if guard.last() != Some(&snapshot) {
                guard.push(snapshot);
            }
        }
    }

    fn latest_snapshot(&self) -> Option<StatusItemSnapshot> {
        self.snapshots.lock().ok().and_then(|s| s.last().cloned())
    }

    fn snapshots(&self) -> Vec<StatusItemSnapshot> {
        self.snapshots.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_item_icon_and_preset_envelope_bounds() {
        let icon = StatusItemIconSpec::canonical();
        assert_eq!((icon.width_pt, icon.height_pt), (20.0, 18.0));
        assert!(icon.is_template);
        assert_eq!(icon.accessibility_description, "Micro Input 键盘");
        assert_eq!(icon.primitives.len(), 10);

        let valid = PresetFileEnvelope::from_bytes(b"{\"format\":2}").expect("valid json");
        assert_eq!(valid.byte_len, 12);

        let oversized = vec![b'a'; MAX_PRESET_FILE_BYTES + 1];
        assert!(PresetFileEnvelope::from_bytes(&oversized).is_err());
    }
}
