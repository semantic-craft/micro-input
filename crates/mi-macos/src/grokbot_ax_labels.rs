pub const DICTATION_START: [&str; 3] = ["Start voice input", "开始语音输入", "開始語音輸入"];

pub const DICTATION_STOP: [&str; 3] = ["Stop dictation", "停止听写", "停止聽寫"];

pub const DICTATION_FINISHING: [&str; 6] = [
    "Transcribing voice input…",
    "Transcribing voice input...",
    "正在转写语音输入…",
    "正在转写语音输入...",
    "正在轉錄語音輸入…",
    "正在轉錄語音輸入...",
];

pub const VOICE_CHAT_START: [&str; 3] = ["Start voice chat", "发起语音聊天", "發起語音聊天"];

pub const VOICE_CHAT_END: [&str; 3] = ["Hang up", "挂断", "掛斷"];

pub fn is_dictation_start(label: &str) -> bool {
    DICTATION_START.contains(&label.trim())
}

pub fn is_dictation_stop(label: &str) -> bool {
    DICTATION_STOP.contains(&label.trim())
}

pub fn is_dictation_finishing(label: &str) -> bool {
    DICTATION_FINISHING.contains(&label.trim())
}

pub fn is_voice_chat_start(label: &str) -> bool {
    VOICE_CHAT_START.contains(&label.trim())
}

pub fn is_voice_chat_end(label: &str) -> bool {
    VOICE_CHAT_END.contains(&label.trim())
}

pub fn is_send_message(label: &str) -> bool {
    crate::antigravity_ax_labels::is_send_message(label)
}
