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
pub struct HeaderProjection {
    pub engine_version: String,
    pub ready_state: HeaderReadyState,
    pub banner: OwnershipBannerState,
    pub show_connect_button: bool,
    pub show_return_to_host_button: bool,
    pub host_execution_enabled: bool,
    #[serde(default = "default_true")]
    pub accessibility_trusted: bool,
    pub can_edit_draft: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub busy: bool,
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
            can_edit_draft: false,
            can_undo: false,
            can_redo: false,
            busy: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HeaderAction {
    ConnectKeyboard,
    ReturnToHostDocument,
    SetExecutionEnabled(bool),
    OpenAccessibilitySettings,
    Undo,
    Redo,
    OpenSettings,
}
