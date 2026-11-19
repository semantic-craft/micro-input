use crate::sidebar::OwnershipBannerState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HeaderReadyState {
    Ready { text: String },
    ModeNotice { text: String },
    Caution { text: String },
}

impl HeaderReadyState {
    pub fn text(&self) -> &str {
        match self {
            Self::Ready { text } | Self::ModeNotice { text } | Self::Caution { text } => text,
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowerStatusBadgeProjection {
    pub battery_percent: Option<u8>,
    pub is_charging: Option<bool>,
    pub line_powered: bool,
    pub usb_host_hold: bool,
    pub usb_mode: String,
    pub pm_mode: String,
    pub host_sleep_status: String,
    pub summary_label: String,
}

impl PowerStatusBadgeProjection {
    pub fn from_v070(
        v070: &crate::v070_hardware::V070HardwareProjection,
        device_connected: bool,
    ) -> Option<Self> {
        Self::from_v070_localized(v070, device_connected, mi_model::ResolvedLocale::ZhHans)
    }

    pub fn from_v070_localized(
        v070: &crate::v070_hardware::V070HardwareProjection,
        device_connected: bool,
        locale: mi_model::ResolvedLocale,
    ) -> Option<Self> {
        if !(device_connected && v070.supported) {
            return None;
        }
        let battery_segment = if let Some(pct) = v070.battery_percent {
            if v070.is_charging == Some(true) {
                if locale == mi_model::ResolvedLocale::En {
                    format!("{pct}% Charging")
                } else {
                    format!("{pct}% 充电中")
                }
            } else {
                format!("{pct}%")
            }
        } else if v070.synced && v070.pm_state.line_powered {
            crate::i18n::tr("USB 供电", locale).to_string()
        } else {
            crate::i18n::tr("电量读取中", locale).to_string()
        };
        let usb_mode = if v070.synced {
            let mode_zh = if v070.pm_state.usb_host_hold {
                "usb.host 保持"
            } else if v070.pm_config.host_sleep {
                "跟随宿主"
            } else if !v070.pm_config.auto_sleep {
                "常醒"
            } else {
                "定时休眠"
            };
            crate::i18n::tr(mode_zh, locale).to_string()
        } else {
            String::new()
        };
        let pm_mode = if v070.synced {
            v070.pm_state.state.clone()
        } else {
            String::new()
        };
        let mut segments = vec![battery_segment];
        if !usb_mode.is_empty() {
            segments.push(usb_mode.clone());
        }
        if v070.synced && !pm_mode.is_empty() && pm_mode != "active" {
            let mapped = match pm_mode.as_str() {
                "idle" => crate::i18n::tr("待机", locale),
                "sleep" => crate::i18n::tr("休眠", locale),
                other => other,
            };
            segments.push(mapped.to_string());
        }
        let summary_label = segments.join(" · ");
        Some(Self {
            battery_percent: v070.battery_percent,
            is_charging: v070.is_charging,
            line_powered: v070.pm_state.line_powered,
            usb_host_hold: v070.pm_state.usb_host_hold,
            usb_mode,
            pm_mode,
            host_sleep_status: v070.host_sleep_status.clone(),
            summary_label,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateDevice {
    pub path: String,
    pub product: String,
    #[serde(default)]
    pub serial_number: Option<String>,
    #[serde(default)]
    pub selector_key: String,
    #[serde(default)]
    pub display_label: String,
    pub is_connected: bool,
    pub is_preferred: bool,
}

impl CandidateDevice {
    pub fn new(
        path: String,
        product: String,
        serial_number: Option<String>,
        display_label: String,
        is_connected: bool,
        is_preferred: bool,
    ) -> Self {
        let selector_key = serial_number
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&path)
            .to_string();
        Self {
            path,
            product,
            serial_number,
            selector_key,
            display_label,
            is_connected,
            is_preferred,
        }
    }

    pub fn selector_key(&self) -> &str {
        &self.selector_key
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeaderProjection {
    pub engine_version: String,
    pub ready_state: HeaderReadyState,
    pub banner: OwnershipBannerState,
    pub show_connect_button: bool,
    pub show_return_to_host_button: bool,
    pub host_execution_enabled: bool,
    #[serde(default = "default_true")]
    pub accessibility_trusted: bool,
    #[serde(default)]
    pub can_retry_host_cleanup: bool,
    pub can_edit_draft: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub busy: bool,
    #[serde(default)]
    pub power_status: Option<PowerStatusBadgeProjection>,
    #[serde(default)]
    pub candidate_devices: Vec<CandidateDevice>,
    #[serde(default)]
    pub preferred_device_serial: Option<String>,
    #[serde(default)]
    pub ui_language: mi_model::UiLanguagePreference,
    #[serde(default)]
    pub resolved_locale: mi_model::ResolvedLocale,
    #[serde(default = "default_true")]
    pub silent_at_login: bool,
}

impl Default for HeaderProjection {
    fn default() -> Self {
        Self {
            engine_version: env!("CARGO_PKG_VERSION").to_string(),
            ready_state: HeaderReadyState::Ready {
                text: "已就绪".to_string(),
            },
            banner: OwnershipBannerState::Ready {
                summary: "就绪".to_string(),
            },
            show_connect_button: true,
            show_return_to_host_button: false,
            host_execution_enabled: false,
            accessibility_trusted: true,
            can_retry_host_cleanup: false,
            can_edit_draft: false,
            can_undo: false,
            can_redo: false,
            busy: false,
            power_status: None,
            candidate_devices: Vec::new(),
            preferred_device_serial: None,
            ui_language: mi_model::UiLanguagePreference::System,
            resolved_locale: mi_model::ResolvedLocale::ZhHans,
            silent_at_login: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HeaderAction {
    ConnectKeyboard,
    ReturnToHostDocument,
    SetExecutionEnabled(bool),
    OpenAccessibilitySettings,
    RetryHostCleanup,
    Undo,
    Redo,
    OpenSettings,
    SelectCandidateDevice {
        selector: String,
    },
    SetUiLanguage {
        language: mi_model::UiLanguagePreference,
    },
    SyncSystemLocale {
        preferred_languages: Vec<String>,
    },
    SetSilentAtLogin {
        silent: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::CandidateDevice;

    #[test]
    fn candidate_device_computes_and_serializes_selector_key_once() {
        let with_serial = CandidateDevice::new(
            "/dev/hid1".to_string(),
            "Creator Micro 2".to_string(),
            Some("  SN-42  ".to_string()),
            "Creator Micro 2 (#SN-42)".to_string(),
            true,
            true,
        );
        assert_eq!(with_serial.selector_key(), "SN-42");
        let json = serde_json::to_value(&with_serial).unwrap();
        assert_eq!(json["selector_key"], "SN-42");

        let fallback = CandidateDevice::new(
            "IOService:/USB/9".to_string(),
            "Creator Micro 2".to_string(),
            Some("   ".to_string()),
            "Creator Micro 2 (#IOService:/USB/9)".to_string(),
            false,
            false,
        );
        assert_eq!(fallback.selector_key(), "IOService:/USB/9");
    }
}
