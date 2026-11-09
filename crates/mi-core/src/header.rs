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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeaderProjection {
    pub ready_state: HeaderReadyState,
    pub banner: OwnershipBannerState,
    pub show_connect_button: bool,
    pub show_return_to_host_button: bool,
    pub host_execution_enabled: bool,
    pub can_edit_draft: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub busy: bool,
}

impl Default for HeaderProjection {
    fn default() -> Self {
        Self {
            ready_state: HeaderReadyState::Ready {
                text: "已就绪".to_string(),
            },
            banner: OwnershipBannerState::Ready {
                summary: "就绪".to_string(),
            },
            show_connect_button: true,
            show_return_to_host_button: false,
            host_execution_enabled: false,
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
    Undo,
    Redo,
    OpenSettings,
}
