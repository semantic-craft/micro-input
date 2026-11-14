#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DictationAdapter {
    Antigravity,
    Dsh,
    Codex,
}

impl DictationAdapter {
    pub fn parse(adapter: &str) -> Option<Self> {
        match adapter.trim().to_ascii_lowercase().as_str() {
            "antigravity" => Some(Self::Antigravity),
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
            Self::Dsh => matches!(
                trimmed,
                "开始听写" | "Voice Dictation" | "开始录音" | "Start recording"
            ),
            Self::Codex => matches!(trimmed, "Dictate" | "听写"),
        }
    }

    pub fn matches_finishing_label(&self, label: &str) -> bool {
        *self == Self::Codex && matches!(label.trim(), "Finishing dictation" | "正在完成听写")
    }

    pub fn matches_stop_label(&self, label: &str) -> bool {
        let trimmed = label.trim();
        match self {
            Self::Antigravity => {
                matches!(trimmed, "Stop Recording" | "Recording" | "Stop recording")
            }
            Self::Dsh => matches!(
                trimmed,
                "停止听写" | "Stop Dictation" | "停止并识别" | "Stop and transcribe"
            ),
            Self::Codex => matches!(trimmed, "Stop dictation" | "停止听写"),
        }
    }
}
