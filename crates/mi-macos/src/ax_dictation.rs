use crate::executor::{ClipboardSnapshot, MAX_CLIPBOARD_STASH_BYTES};
use std::sync::Mutex;

pub const MAX_PARENT_CHAIN_HOPS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DictationAdapter {
    Antigravity,
    QwenWork,
    Cindy,
    Dsh,
    Codex,
}

impl DictationAdapter {
    pub fn parse(adapter: &str) -> Option<Self> {
        match adapter.trim().to_ascii_lowercase().as_str() {
            "antigravity" => Some(Self::Antigravity),
            "qwen" | "qwenwork" | "qwen-work" | "千问办公" => Some(Self::QwenWork),
            "cindy" => Some(Self::Cindy),
            "dsh" | "dsh-desktop" | "deepseek" => Some(Self::Dsh),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }

    pub fn matches_start_label(&self, label: &str) -> bool {
        let trimmed = label.trim();
        match self {
            Self::Antigravity => {
                matches!(trimmed, "Record Audio" | "Record voice memo")
            }
            Self::QwenWork => trimmed.starts_with("语音输入"),
            Self::Cindy => trimmed == "开始语音输入",
            Self::Dsh => matches!(trimmed, "开始听写" | "Voice Dictation"),
            Self::Codex => matches!(trimmed, "Dictate" | "听写"),
        }
    }

    pub fn matches_stop_label(&self, label: &str) -> bool {
        let trimmed = label.trim();
        match self {
            Self::Antigravity => {
                matches!(trimmed, "Stop Recording" | "Recording" | "Stop recording")
            }
            Self::QwenWork => trimmed.starts_with("停止录音"),
            Self::Cindy => matches!(trimmed, "停止语音输入" | "结束语音输入"),
            Self::Dsh => matches!(trimmed, "停止听写" | "Stop Dictation"),
            Self::Codex => matches!(trimmed, "Stop dictation" | "停止听写"),
        }
    }
}

pub fn is_valid_ax_leaf_on_no_value(
    role: Option<&str>,
    attribute_names: Option<&[String]>,
) -> bool {
    let Some(r) = role.map(str::trim).filter(|r| !r.is_empty()) else {
        return false;
    };
    let _ = r;
    let Some(names) = attribute_names else {
        return false;
    };
    !names.iter().any(|attr| attr == "AXChildren")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedDictationTarget {
    pub pid: i32,
    pub window_id: u64,
    pub window_title: String,
    pub control_id: String,
    pub adapter: DictationAdapter,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxElementNode {
    pub control_id: String,
    pub role: String,
    pub label: String,
    pub parent_chain_to_window: Vec<String>,
    pub attribute_names: Vec<String>,
}

impl AxElementNode {
    pub fn reaches_window_within_hop_limit(&self, window_id: u64) -> bool {
        if self.parent_chain_to_window.is_empty()
            || self.parent_chain_to_window.len() > MAX_PARENT_CHAIN_HOPS
        {
            return false;
        }
        let expected_window_token = window_id.to_string();
        self.parent_chain_to_window
            .iter()
            .any(|hop| hop == &expected_window_token)
    }

    pub fn is_valid_leaf_for_window(&self, window_id: u64) -> bool {
        is_valid_ax_leaf_on_no_value(Some(&self.role), Some(&self.attribute_names))
            && self.reaches_window_within_hop_limit(window_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxWindowSnapshot {
    pub pid: i32,
    pub window_id: u64,
    pub window_title: String,
    pub controls: Vec<AxElementNode>,
}

pub trait AxTreeProvider: Send + Sync {
    fn frontmost_window(&self, adapter: DictationAdapter) -> Option<AxWindowSnapshot>;
    fn window_by_identity(
        &self,
        pid: i32,
        window_id: u64,
        window_title: &str,
    ) -> Option<AxWindowSnapshot>;
    fn ax_press(&self, pid: i32, window_id: u64, control_id: &str) -> Result<(), String>;
    fn snapshot_clipboard(&self, max_bytes: usize) -> ClipboardSnapshot;
    fn restore_clipboard(&self, snapshot: &ClipboardSnapshot, expected_change_count: u64) -> bool;
}

impl<T: AxTreeProvider + ?Sized> AxTreeProvider for std::sync::Arc<T> {
    fn frontmost_window(&self, adapter: DictationAdapter) -> Option<AxWindowSnapshot> {
        (**self).frontmost_window(adapter)
    }

    fn window_by_identity(
        &self,
        pid: i32,
        window_id: u64,
        window_title: &str,
    ) -> Option<AxWindowSnapshot> {
        (**self).window_by_identity(pid, window_id, window_title)
    }

    fn ax_press(&self, pid: i32, window_id: u64, control_id: &str) -> Result<(), String> {
        (**self).ax_press(pid, window_id, control_id)
    }

    fn snapshot_clipboard(&self, max_bytes: usize) -> ClipboardSnapshot {
        (**self).snapshot_clipboard(max_bytes)
    }

    fn restore_clipboard(&self, snapshot: &ClipboardSnapshot, expected_change_count: u64) -> bool {
        (**self).restore_clipboard(snapshot, expected_change_count)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DictationOutcome {
    Started(LockedDictationTarget),
    Stopped {
        used_replacement_idle_readback: bool,
        clipboard_restored: bool,
    },
    Failed(String),
}

pub struct AXDictationController<P: AxTreeProvider> {
    provider: P,
    active_target: Mutex<Option<LockedDictationTarget>>,
    stashed_clipboard: Mutex<Option<ClipboardSnapshot>>,
}

impl<P: AxTreeProvider> AXDictationController<P> {
    pub fn new(provider: P) -> Self {
        Self {
            provider,
            active_target: Mutex::new(None),
            stashed_clipboard: Mutex::new(None),
        }
    }

    pub fn provider(&self) -> &P {
        &self.provider
    }

    pub fn active_target(&self) -> Option<LockedDictationTarget> {
        self.active_target.lock().unwrap().clone()
    }

    pub fn start_dictation(&self, adapter: DictationAdapter) -> DictationOutcome {
        let Some(window) = self.provider.frontmost_window(adapter) else {
            return DictationOutcome::Failed("frontmost_window_not_found".to_string());
        };
        let candidates: Vec<&AxElementNode> = window
            .controls
            .iter()
            .filter(|c| {
                c.is_valid_leaf_for_window(window.window_id)
                    && adapter.matches_start_label(&c.label)
            })
            .collect();
        if candidates.len() != 1 {
            return DictationOutcome::Failed(format!(
                "ambiguous_or_missing_start_control:{}",
                candidates.len()
            ));
        }
        let target_control_id = candidates[0].control_id.clone();

        let clip = self.provider.snapshot_clipboard(MAX_CLIPBOARD_STASH_BYTES);
        *self.stashed_clipboard.lock().unwrap() = Some(clip);

        if let Err(err) = self
            .provider
            .ax_press(window.pid, window.window_id, &target_control_id)
        {
            return DictationOutcome::Failed(err);
        }

        let Some(readback_window) =
            self.provider
                .window_by_identity(window.pid, window.window_id, &window.window_title)
        else {
            return DictationOutcome::Failed("window_identity_lost_after_press".to_string());
        };

        let confirmed = readback_window.controls.iter().any(|c| {
            c.control_id == target_control_id
                && c.is_valid_leaf_for_window(readback_window.window_id)
                && adapter.matches_stop_label(&c.label)
        });
        if !confirmed {
            return DictationOutcome::Failed("start_readback_unconfirmed".to_string());
        }

        let locked = LockedDictationTarget {
            pid: window.pid,
            window_id: window.window_id,
            window_title: window.window_title,
            control_id: target_control_id,
            adapter,
        };
        *self.active_target.lock().unwrap() = Some(locked.clone());
        DictationOutcome::Started(locked)
    }

    pub fn stop_dictation(&self) -> DictationOutcome {
        let Some(locked) = self.active_target.lock().unwrap().clone() else {
            return DictationOutcome::Failed("no_active_dictation".to_string());
        };

        let Some(window) =
            self.provider
                .window_by_identity(locked.pid, locked.window_id, &locked.window_title)
        else {
            return DictationOutcome::Failed("locked_window_not_found".to_string());
        };

        let active_matches = window.controls.iter().any(|c| {
            c.control_id == locked.control_id
                && c.is_valid_leaf_for_window(window.window_id)
                && locked.adapter.matches_stop_label(&c.label)
        });
        if !active_matches {
            return DictationOutcome::Failed("locked_control_not_in_recording_state".to_string());
        }

        if let Err(err) = self
            .provider
            .ax_press(locked.pid, locked.window_id, &locked.control_id)
        {
            return DictationOutcome::Failed(err);
        }

        let Some(readback_window) =
            self.provider
                .window_by_identity(locked.pid, locked.window_id, &locked.window_title)
        else {
            return DictationOutcome::Failed("window_identity_lost_on_stop".to_string());
        };

        let same_control_idle = readback_window.controls.iter().any(|c| {
            c.control_id == locked.control_id
                && c.is_valid_leaf_for_window(readback_window.window_id)
                && locked.adapter.matches_start_label(&c.label)
        });

        let used_replacement_idle_readback = if same_control_idle {
            false
        } else {
            let original_still_present = readback_window
                .controls
                .iter()
                .any(|c| c.control_id == locked.control_id);
            if original_still_present {
                return DictationOutcome::Failed("stop_readback_still_recording".to_string());
            }
            let idle_replacements: Vec<&AxElementNode> = readback_window
                .controls
                .iter()
                .filter(|c| {
                    c.is_valid_leaf_for_window(readback_window.window_id)
                        && locked.adapter.matches_start_label(&c.label)
                })
                .collect();
            if idle_replacements.len() != 1 {
                return DictationOutcome::Failed("replacement_idle_control_not_unique".to_string());
            }
            true
        };

        let clipboard_restored = if let Some(stash) = self.stashed_clipboard.lock().unwrap().take()
        {
            self.provider.restore_clipboard(&stash, stash.change_count)
        } else {
            true
        };

        *self.active_target.lock().unwrap() = None;
        DictationOutcome::Stopped {
            used_replacement_idle_readback,
            clipboard_restored,
        }
    }
}
