use crate::radial::RadialConfirmMode;
use mi_macos::{PassivePanelFrameSpec, ScreenGeometry, ScreenPoint, ScreenRect};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

pub const PASSIVE_NOTICE_DURATION: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetAgentStatusCardProjection {
    pub agent_name: String,
    pub adapter: String,
    pub phase_label: String,
    pub phase_symbol: String,
    pub is_recording: bool,
    pub voice_summary: String,
    pub confirm_mode: RadialConfirmMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheatSheetKeyCellProjection {
    pub binding_id: String,
    pub control_name: String,
    pub label: String,
    pub short_label: String,
    pub available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CheatSheetProjection {
    pub profile_name: String,
    pub routing_summary: String,
    pub target_agent_card: Option<TargetAgentStatusCardProjection>,
    pub key_cells: Vec<CheatSheetKeyCellProjection>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HudVisibilityState {
    Hidden,
    SuppressedByActiveWorkbench,
    PassiveToast {
        profile_name: String,
        mode_summary: String,
        expires_at: Instant,
        frame: ScreenRect,
    },
    InteractiveCheatSheet {
        projection: CheatSheetProjection,
        frame: ScreenRect,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ActiveHudContent {
    #[default]
    Hidden,
    SuppressedByActiveWorkbench,
    PassiveToast {
        profile_name: String,
        mode_summary: String,
    },
    InteractiveCheatSheet(CheatSheetProjection),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct HostOverlayProjection {
    pub content: ActiveHudContent,
    pub panel_spec: Option<PassivePanelFrameSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostOverlayAction {
    CloseCheatSheet,
    ToggleCheatSheet,
}

#[derive(Debug, Clone)]
pub struct OverlayController {
    main_workbench_window_active: bool,
    visibility: HudVisibilityState,
}

impl Default for OverlayController {
    fn default() -> Self {
        Self::new()
    }
}

impl OverlayController {
    pub fn new() -> Self {
        Self {
            main_workbench_window_active: false,
            visibility: HudVisibilityState::Hidden,
        }
    }

    pub fn main_workbench_window_active(&self) -> bool {
        self.main_workbench_window_active
    }

    pub fn visibility(&self) -> &HudVisibilityState {
        &self.visibility
    }

    pub fn is_cheat_sheet_visible(&self) -> bool {
        matches!(
            self.visibility,
            HudVisibilityState::InteractiveCheatSheet { .. }
        )
    }

    pub fn set_main_workbench_window_active(&mut self, active: bool) {
        self.main_workbench_window_active = active;
        if active && matches!(self.visibility, HudVisibilityState::PassiveToast { .. }) {
            self.visibility = HudVisibilityState::SuppressedByActiveWorkbench;
        }
    }

    pub fn request_profile_switch_notice(
        &mut self,
        profile_name: String,
        mode_summary: String,
        cheat_sheet: &CheatSheetProjection,
        screen: &ScreenGeometry,
        cursor: ScreenPoint,
        now: Instant,
    ) {
        if self.is_cheat_sheet_visible() {
            let spec = PassivePanelFrameSpec::for_cheat_sheet(cursor, screen);
            self.visibility = HudVisibilityState::InteractiveCheatSheet {
                projection: cheat_sheet.clone(),
                frame: spec.frame,
            };
            return;
        }

        if self.main_workbench_window_active {
            self.visibility = HudVisibilityState::SuppressedByActiveWorkbench;
            return;
        }

        let spec = PassivePanelFrameSpec::for_notice(cursor, screen);
        self.visibility = HudVisibilityState::PassiveToast {
            profile_name,
            mode_summary,
            expires_at: now + PASSIVE_NOTICE_DURATION,
            frame: spec.frame,
        };
    }

    pub fn show_cheat_sheet(
        &mut self,
        cheat_sheet: CheatSheetProjection,
        screen: &ScreenGeometry,
        cursor: ScreenPoint,
    ) {
        let spec = PassivePanelFrameSpec::for_cheat_sheet(cursor, screen);
        self.visibility = HudVisibilityState::InteractiveCheatSheet {
            projection: cheat_sheet,
            frame: spec.frame,
        };
    }

    pub fn toggle_cheat_sheet(
        &mut self,
        cheat_sheet: CheatSheetProjection,
        screen: &ScreenGeometry,
        cursor: ScreenPoint,
    ) {
        if self.is_cheat_sheet_visible() {
            self.hide();
        } else {
            self.show_cheat_sheet(cheat_sheet, screen, cursor);
        }
    }

    pub fn refresh_cheat_sheet_if_visible(
        &mut self,
        cheat_sheet: CheatSheetProjection,
        screen: &ScreenGeometry,
        cursor: ScreenPoint,
    ) {
        if self.is_cheat_sheet_visible() {
            let spec = PassivePanelFrameSpec::for_cheat_sheet(cursor, screen);
            self.visibility = HudVisibilityState::InteractiveCheatSheet {
                projection: cheat_sheet,
                frame: spec.frame,
            };
        }
    }

    pub fn hide(&mut self) {
        self.visibility = HudVisibilityState::Hidden;
    }

    pub fn expire_notice_if_due(&mut self, now: Instant) -> bool {
        if let HudVisibilityState::PassiveToast { expires_at, .. } = self.visibility {
            if now >= expires_at {
                self.visibility = HudVisibilityState::Hidden;
                return true;
            }
        }
        false
    }

    pub fn project(&self, screen: &ScreenGeometry, cursor: ScreenPoint) -> HostOverlayProjection {
        match &self.visibility {
            HudVisibilityState::Hidden => HostOverlayProjection {
                content: ActiveHudContent::Hidden,
                panel_spec: None,
            },
            HudVisibilityState::SuppressedByActiveWorkbench => HostOverlayProjection {
                content: ActiveHudContent::SuppressedByActiveWorkbench,
                panel_spec: None,
            },
            HudVisibilityState::PassiveToast {
                profile_name,
                mode_summary,
                ..
            } => {
                if self.main_workbench_window_active {
                    HostOverlayProjection {
                        content: ActiveHudContent::SuppressedByActiveWorkbench,
                        panel_spec: None,
                    }
                } else {
                    HostOverlayProjection {
                        content: ActiveHudContent::PassiveToast {
                            profile_name: profile_name.clone(),
                            mode_summary: mode_summary.clone(),
                        },
                        panel_spec: Some(PassivePanelFrameSpec::for_notice(cursor, screen)),
                    }
                }
            }
            HudVisibilityState::InteractiveCheatSheet { projection, .. } => HostOverlayProjection {
                content: ActiveHudContent::InteractiveCheatSheet(projection.clone()),
                panel_spec: Some(PassivePanelFrameSpec::for_cheat_sheet(cursor, screen)),
            },
        }
    }
}
