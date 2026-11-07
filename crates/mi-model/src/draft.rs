//! R-E07/R-E08/R-K05: versioned local intents; never firmware payloads.
use crate::DeviceConfig;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const FIRMWARE_V063_TOKENS: [&str; 304] = [
    "ENC_CC",
    "ENC_CLK",
    "ENC_CW",
    "KC_0",
    "KC_1",
    "KC_2",
    "KC_3",
    "KC_4",
    "KC_5",
    "KC_6",
    "KC_7",
    "KC_8",
    "KC_9",
    "KC_A",
    "KC_AGAIN",
    "KC_APP",
    "KC_B",
    "KC_BRID",
    "KC_BRIU",
    "KC_BSLS",
    "KC_BSPC",
    "KC_C",
    "KC_CANCEL",
    "KC_CAPS",
    "KC_CLEAR",
    "KC_CLEAR_AGAIN",
    "KC_COMM",
    "KC_COPY",
    "KC_CRSEL",
    "KC_CUT",
    "KC_D",
    "KC_DEL",
    "KC_DOT",
    "KC_DOWN",
    "KC_E",
    "KC_END",
    "KC_ENT",
    "KC_EQL",
    "KC_ERAS",
    "KC_ESC",
    "KC_EXECUTE",
    "KC_EXSEL",
    "KC_F",
    "KC_F1",
    "KC_F10",
    "KC_F11",
    "KC_F12",
    "KC_F13",
    "KC_F14",
    "KC_F15",
    "KC_F16",
    "KC_F17",
    "KC_F18",
    "KC_F19",
    "KC_F2",
    "KC_F20",
    "KC_F21",
    "KC_F22",
    "KC_F23",
    "KC_F24",
    "KC_F3",
    "KC_F4",
    "KC_F5",
    "KC_F6",
    "KC_F7",
    "KC_F8",
    "KC_F9",
    "KC_FIND",
    "KC_G",
    "KC_GRV",
    "KC_H",
    "KC_HAEN",
    "KC_HANJ",
    "KC_HELP",
    "KC_HENK",
    "KC_HOME",
    "KC_I",
    "KC_INS",
    "KC_INT6",
    "KC_INT7",
    "KC_INT8",
    "KC_INT9",
    "KC_J",
    "KC_JYEN",
    "KC_K",
    "KC_KANA",
    "KC_KP_EQUAL_AS400",
    "KC_L",
    "KC_LALT",
    "KC_LANG3",
    "KC_LANG4",
    "KC_LANG5",
    "KC_LANG6",
    "KC_LANG7",
    "KC_LANG8",
    "KC_LANG9",
    "KC_LBRC",
    "KC_LCAP",
    "KC_LCTL",
    "KC_LEFT",
    "KC_LGUI",
    "KC_LNUM",
    "KC_LPAD",
    "KC_LSCR",
    "KC_LSFT",
    "KC_M",
    "KC_MCTL",
    "KC_MENU",
    "KC_MFFD",
    "KC_MHEN",
    "KC_MINS",
    "KC_MNXT",
    "KC_MPLY",
    "KC_MPRV",
    "KC_MRWD",
    "KC_MSTP",
    "KC_MUTE",
    "KC_N",
    "KC_NLCK",
    "KC_NONE",
    "KC_NUBS",
    "KC_NUHS",
    "KC_O",
    "KC_OPER",
    "KC_OUT",
    "KC_P",
    "KC_P0",
    "KC_P1",
    "KC_P2",
    "KC_P3",
    "KC_P4",
    "KC_P5",
    "KC_P6",
    "KC_P7",
    "KC_P8",
    "KC_P9",
    "KC_PAST",
    "KC_PASTE",
    "KC_PAUS",
    "KC_PCMM",
    "KC_PDOT",
    "KC_PENT",
    "KC_PEQL",
    "KC_PGDN",
    "KC_PGUP",
    "KC_PMNS",
    "KC_POWER",
    "KC_PPLS",
    "KC_PRIOR",
    "KC_PSCR",
    "KC_PSLS",
    "KC_PWR",
    "KC_Q",
    "KC_QUOT",
    "KC_R",
    "KC_RALT",
    "KC_RBRC",
    "KC_RCTL",
    "KC_RETURN",
    "KC_RGHT",
    "KC_RGUI",
    "KC_RO",
    "KC_RSFT",
    "KC_S",
    "KC_SCLN",
    "KC_SELECT",
    "KC_SEPAR",
    "KC_SLCK",
    "KC_SLEP",
    "KC_SLSH",
    "KC_SPC",
    "KC_STOP",
    "KC_SYSREQ",
    "KC_T",
    "KC_TAB",
    "KC_U",
    "KC_UNDO",
    "KC_UP",
    "KC_V",
    "KC_VOLD",
    "KC_VOLU",
    "KC_W",
    "KC_X",
    "KC_Y",
    "KC_Z",
    "KI_BLDW",
    "KI_BLUP",
    "KI_CBT1",
    "KI_CBT2",
    "KI_CBT3",
    "KI_CBT4",
    "KI_CBT5",
    "KI_CBT6",
    "KI_CBT7",
    "KI_CBT8",
    "KI_CBTP1",
    "KI_CBTP2",
    "KI_CBTP3",
    "KI_CBTP4",
    "KI_CBTP5",
    "KI_CBTP6",
    "KI_CBTP7",
    "KI_CBTP8",
    "KI_CS_HIDE",
    "KI_CS_SHOW",
    "KI_CS_SHOW_TMP",
    "KI_CS_TOGGLE",
    "KI_CUSB",
    "KI_FP",
    "KI_LM1",
    "KI_LM10",
    "KI_LM11",
    "KI_LM12",
    "KI_LM13",
    "KI_LM14",
    "KI_LM15",
    "KI_LM2",
    "KI_LM3",
    "KI_LM4",
    "KI_LM5",
    "KI_LM6",
    "KI_LM7",
    "KI_LM8",
    "KI_LM9",
    "KI_LMNEXT",
    "KI_LMPREV",
    "KI_LS1",
    "KI_LS10",
    "KI_LS11",
    "KI_LS12",
    "KI_LS13",
    "KI_LS14",
    "KI_LS15",
    "KI_LS2",
    "KI_LS3",
    "KI_LS4",
    "KI_LS5",
    "KI_LS6",
    "KI_LS7",
    "KI_LS8",
    "KI_LS9",
    "KI_LSNEXT",
    "KI_LSPREV",
    "KI_PS1",
    "KI_PS10",
    "KI_PS11",
    "KI_PS12",
    "KI_PS13",
    "KI_PS14",
    "KI_PS15",
    "KI_PS2",
    "KI_PS3",
    "KI_PS4",
    "KI_PS5",
    "KI_PS6",
    "KI_PS7",
    "KI_PS8",
    "KI_PS9",
    "KV_FRAMER_AI",
    "KV_FRAMER_PUBLISH",
    "KV_OAI_ACT00",
    "KV_OAI_ACT01",
    "KV_OAI_ACT02",
    "KV_OAI_ACT03",
    "KV_OAI_ACT04",
    "KV_OAI_ACT05",
    "KV_OAI_ACT06",
    "KV_OAI_ACT07",
    "KV_OAI_ACT08",
    "KV_OAI_ACT09",
    "KV_OAI_ACT10",
    "KV_OAI_ACT11",
    "KV_OAI_ACT12",
    "KV_OAI_ACT13",
    "KV_OAI_ACT14",
    "KV_OAI_ACT15",
    "KV_OAI_ACT16",
    "KV_OAI_ACT17",
    "KV_OAI_ACT18",
    "KV_OAI_ACT19",
    "KV_OAI_ACT20",
    "KV_OAI_AG00",
    "KV_OAI_AG01",
    "KV_OAI_AG02",
    "KV_OAI_AG03",
    "KV_OAI_AG04",
    "KV_OAI_AG05",
    "KV_OAI_AG06",
    "KV_OAI_AG07",
    "KV_OAI_AG08",
    "KV_OAI_AG09",
    "KV_OAI_AG10",
    "KV_OAI_AG11",
    "KV_OAI_AG12",
    "KV_OAI_AG13",
    "KV_OAI_AG14",
    "KV_OAI_AG15",
    "KV_OAI_AG16",
    "KV_OAI_AG17",
    "KV_OAI_AG18",
    "KV_OAI_AG19",
    "KV_OAI_ENC_CC",
    "KV_OAI_ENC_CLK",
    "KV_OAI_ENC_CW",
];

/// Non-OAI firmware v0.6.3 tokens (260 entries) plus legacy aliases ("KC_ENTER", "KC_SPACE").
pub const DEVICE_KEY_TOKENS: [&str; 262] = [
    "ENC_CC",
    "ENC_CLK",
    "ENC_CW",
    "KC_0",
    "KC_1",
    "KC_2",
    "KC_3",
    "KC_4",
    "KC_5",
    "KC_6",
    "KC_7",
    "KC_8",
    "KC_9",
    "KC_A",
    "KC_AGAIN",
    "KC_APP",
    "KC_B",
    "KC_BRID",
    "KC_BRIU",
    "KC_BSLS",
    "KC_BSPC",
    "KC_C",
    "KC_CANCEL",
    "KC_CAPS",
    "KC_CLEAR",
    "KC_CLEAR_AGAIN",
    "KC_COMM",
    "KC_COPY",
    "KC_CRSEL",
    "KC_CUT",
    "KC_D",
    "KC_DEL",
    "KC_DOT",
    "KC_DOWN",
    "KC_E",
    "KC_END",
    "KC_ENT",
    "KC_ENTER",
    "KC_EQL",
    "KC_ERAS",
    "KC_ESC",
    "KC_EXECUTE",
    "KC_EXSEL",
    "KC_F",
    "KC_F1",
    "KC_F10",
    "KC_F11",
    "KC_F12",
    "KC_F13",
    "KC_F14",
    "KC_F15",
    "KC_F16",
    "KC_F17",
    "KC_F18",
    "KC_F19",
    "KC_F2",
    "KC_F20",
    "KC_F21",
    "KC_F22",
    "KC_F23",
    "KC_F24",
    "KC_F3",
    "KC_F4",
    "KC_F5",
    "KC_F6",
    "KC_F7",
    "KC_F8",
    "KC_F9",
    "KC_FIND",
    "KC_G",
    "KC_GRV",
    "KC_H",
    "KC_HAEN",
    "KC_HANJ",
    "KC_HELP",
    "KC_HENK",
    "KC_HOME",
    "KC_I",
    "KC_INS",
    "KC_INT6",
    "KC_INT7",
    "KC_INT8",
    "KC_INT9",
    "KC_J",
    "KC_JYEN",
    "KC_K",
    "KC_KANA",
    "KC_KP_EQUAL_AS400",
    "KC_L",
    "KC_LALT",
    "KC_LANG3",
    "KC_LANG4",
    "KC_LANG5",
    "KC_LANG6",
    "KC_LANG7",
    "KC_LANG8",
    "KC_LANG9",
    "KC_LBRC",
    "KC_LCAP",
    "KC_LCTL",
    "KC_LEFT",
    "KC_LGUI",
    "KC_LNUM",
    "KC_LPAD",
    "KC_LSCR",
    "KC_LSFT",
    "KC_M",
    "KC_MCTL",
    "KC_MENU",
    "KC_MFFD",
    "KC_MHEN",
    "KC_MINS",
    "KC_MNXT",
    "KC_MPLY",
    "KC_MPRV",
    "KC_MRWD",
    "KC_MSTP",
    "KC_MUTE",
    "KC_N",
    "KC_NLCK",
    "KC_NONE",
    "KC_NUBS",
    "KC_NUHS",
    "KC_O",
    "KC_OPER",
    "KC_OUT",
    "KC_P",
    "KC_P0",
    "KC_P1",
    "KC_P2",
    "KC_P3",
    "KC_P4",
    "KC_P5",
    "KC_P6",
    "KC_P7",
    "KC_P8",
    "KC_P9",
    "KC_PAST",
    "KC_PASTE",
    "KC_PAUS",
    "KC_PCMM",
    "KC_PDOT",
    "KC_PENT",
    "KC_PEQL",
    "KC_PGDN",
    "KC_PGUP",
    "KC_PMNS",
    "KC_POWER",
    "KC_PPLS",
    "KC_PRIOR",
    "KC_PSCR",
    "KC_PSLS",
    "KC_PWR",
    "KC_Q",
    "KC_QUOT",
    "KC_R",
    "KC_RALT",
    "KC_RBRC",
    "KC_RCTL",
    "KC_RETURN",
    "KC_RGHT",
    "KC_RGUI",
    "KC_RO",
    "KC_RSFT",
    "KC_S",
    "KC_SCLN",
    "KC_SELECT",
    "KC_SEPAR",
    "KC_SLCK",
    "KC_SLEP",
    "KC_SLSH",
    "KC_SPACE",
    "KC_SPC",
    "KC_STOP",
    "KC_SYSREQ",
    "KC_T",
    "KC_TAB",
    "KC_U",
    "KC_UNDO",
    "KC_UP",
    "KC_V",
    "KC_VOLD",
    "KC_VOLU",
    "KC_W",
    "KC_X",
    "KC_Y",
    "KC_Z",
    "KI_BLDW",
    "KI_BLUP",
    "KI_CBT1",
    "KI_CBT2",
    "KI_CBT3",
    "KI_CBT4",
    "KI_CBT5",
    "KI_CBT6",
    "KI_CBT7",
    "KI_CBT8",
    "KI_CBTP1",
    "KI_CBTP2",
    "KI_CBTP3",
    "KI_CBTP4",
    "KI_CBTP5",
    "KI_CBTP6",
    "KI_CBTP7",
    "KI_CBTP8",
    "KI_CS_HIDE",
    "KI_CS_SHOW",
    "KI_CS_SHOW_TMP",
    "KI_CS_TOGGLE",
    "KI_CUSB",
    "KI_FP",
    "KI_LM1",
    "KI_LM10",
    "KI_LM11",
    "KI_LM12",
    "KI_LM13",
    "KI_LM14",
    "KI_LM15",
    "KI_LM2",
    "KI_LM3",
    "KI_LM4",
    "KI_LM5",
    "KI_LM6",
    "KI_LM7",
    "KI_LM8",
    "KI_LM9",
    "KI_LMNEXT",
    "KI_LMPREV",
    "KI_LS1",
    "KI_LS10",
    "KI_LS11",
    "KI_LS12",
    "KI_LS13",
    "KI_LS14",
    "KI_LS15",
    "KI_LS2",
    "KI_LS3",
    "KI_LS4",
    "KI_LS5",
    "KI_LS6",
    "KI_LS7",
    "KI_LS8",
    "KI_LS9",
    "KI_LSNEXT",
    "KI_LSPREV",
    "KI_PS1",
    "KI_PS10",
    "KI_PS11",
    "KI_PS12",
    "KI_PS13",
    "KI_PS14",
    "KI_PS15",
    "KI_PS2",
    "KI_PS3",
    "KI_PS4",
    "KI_PS5",
    "KI_PS6",
    "KI_PS7",
    "KI_PS8",
    "KI_PS9",
    "KV_FRAMER_AI",
    "KV_FRAMER_PUBLISH",
];

pub fn canonical_firmware_token(token: &str) -> &str {
    match token {
        "KC_ENTER" => "KC_ENT",
        "KC_SPACE" => "KC_SPC",
        other => other,
    }
}

pub fn is_valid_firmware_v063_token(token: &str) -> bool {
    FIRMWARE_V063_TOKENS.binary_search(&token).is_ok()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActionKind {
    DeviceKey {
        token: String,
    },
    LocalMulti {
        multi: crate::multi::LocalMulti,
    },
    LocalMacro {
        macro_sequence: crate::macro_sequence::LocalMacro,
    },
    Shortcut {
        key: String,
        modifiers: Vec<String>,
        target: String,
    },
    Text {
        text: String,
    },
    OpenUrl {
        url: String,
    },
    OpenApp {
        bundle_id: String,
    },
    ShellCommand {
        command: String,
        working_directory: String,
    },
    AgentCommand {
        adapter: String,
        command: String,
    },
    FocusSession {
        adapter: String,
        session: String,
    },
    SkillReference {
        adapter: String,
        skill: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Action {
    pub id: String,
    pub name: String,
    pub category: String,
    pub detail: String,
    // Raw payload preserves future action types. Unsupported payloads lock editing.
    pub payload: Value,
}
impl Action {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() || self.name.len() > 256 {
            return Err("动作名称应为 1–256 字节".into());
        }
        let kind: ActionKind =
            serde_json::from_value(self.payload.clone()).map_err(|_| "动作参数未知或格式不正确")?;
        let bounded = |s: &str| !s.trim().is_empty() && s.len() <= 16384;
        let valid = match kind {
            ActionKind::DeviceKey { token } => DEVICE_KEY_TOKENS.contains(&token.as_str()),
            ActionKind::LocalMacro { macro_sequence } => return macro_sequence.validate(),
            ActionKind::LocalMulti { multi } => return multi.validate(),
            ActionKind::Shortcut {
                key,
                modifiers,
                target,
            } => {
                bounded(&key)
                    && bounded(&target)
                    && modifiers
                        .iter()
                        .all(|m| ["command", "shift", "option", "control"].contains(&m.as_str()))
            }
            ActionKind::Text { text } => bounded(&text),
            ActionKind::OpenUrl { url } => {
                url.len() <= 16384
                    && !url.chars().any(char::is_whitespace)
                    && url::Url::parse(&url).is_ok_and(|u| {
                        matches!(u.scheme(), "http" | "https") && u.host_str().is_some()
                    })
            }
            ActionKind::OpenApp { bundle_id } => {
                bundle_id.len() <= 256
                    && bundle_id.split('.').count() >= 2
                    && bundle_id.split('.').all(|part| {
                        !part.is_empty()
                            && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                    })
            }
            ActionKind::ShellCommand {
                command,
                working_directory,
            } => {
                bounded(&command)
                    && !command.bytes().any(|b| b == 0)
                    && bounded(&working_directory)
                    && !working_directory.bytes().any(|b| b == 0)
                    && working_directory.starts_with('/')
            }
            ActionKind::AgentCommand { adapter, command } => bounded(&adapter) && bounded(&command),
            ActionKind::FocusSession { adapter, session } => bounded(&adapter) && bounded(&session),
            ActionKind::SkillReference { adapter, skill } => bounded(&adapter) && bounded(&skill),
        };
        if valid {
            Ok(())
        } else {
            Err("动作参数为空、过长或不合法".into())
        }
    }
}
pub fn actions() -> Vec<Action> {
    use serde_json::json;
    [
        ("search", "搜索文件", "快捷键", "需要前台编辑器 · 仅草稿", json!({"type":"shortcut","key":"p","modifiers":["command"],"target":"frontmost-editor"})),
        ("save", "保存文件", "快捷键", "需要前台编辑器 · 仅草稿", json!({"type":"shortcut","key":"s","modifiers":["command"],"target":"frontmost-editor"})),
        ("review", "插入审查提示", "文本", "仅保存文本，不执行", json!({"type":"text","text":"请检查当前变更，优先报告可复现的问题。"})),
        ("prompt-continue", "工程指令 · Continue", "文本", "默认仅粘贴不回车（no_newline） · 自动暂存恢复剪贴板", json!({"type":"text","text":"Continue"})),
        ("prompt-test", "工程指令 · Test", "文本", "默认仅粘贴不回车（no_newline） · 自动暂存恢复剪贴板", json!({"type":"text","text":"Run the tests and fix any failures"})),
        ("prompt-review", "工程指令 · Review", "文本", "默认仅粘贴不回车（no_newline） · 自动暂存恢复剪贴板", json!({"type":"text","text":"Review your diff for bugs"})),
        ("prompt-commit", "工程指令 · Commit", "文本", "默认仅粘贴不回车（no_newline） · 自动暂存恢复剪贴板", json!({"type":"text","text":"Commit this work"})),
        ("prompt-summary", "工程指令 · Summary", "文本", "默认仅粘贴不回车（no_newline） · 自动暂存恢复剪贴板", json!({"type":"text","text":"Summarize where things stand and what's left"})),
        ("agent-submit", "停止听写并提交", "提交控制", "若听写激活则先停并确认停止，再聚焦目标输入区回车提交", json!({"type":"agent_command","adapter":"host","command":"submit"})),
        ("agent-prompt-submit", "粘贴指令并提交（显式回车）", "提交控制", "显式配置下先停听写、聚焦目标输入区、暂存剪贴板粘贴后再回车", json!({"type":"agent_command","adapter":"host","command":"submit-prompt:Continue"})),
        ("url", "打开网址", "URL", "仅保存网址，不打开", json!({"type":"open_url","url":"https://example.com"})),
        ("pane", "下一个 pane", "Herdr", "适配器未验证 · 不执行", json!({"type":"agent_command","adapter":"herdr","command":"next-pane"})),
        ("navigate-back", "后退到上一个打开目标", "导航", "跨 Agent 键盘打开历史后退（保留前进栈）", json!({"type":"agent_command","adapter":"navigation","command":"back"})),
        ("navigate-forward", "前进到下一个打开目标", "导航", "跨 Agent 键盘打开历史前进", json!({"type":"agent_command","adapter":"navigation","command":"forward"})),
        ("session", "切换会话", "DSH", "适配器未验证 · 不执行", json!({"type":"focus_session","adapter":"dsh","session":"unselected"})),
        ("skill", "Skill 引用", "Skill", "适配器未验证 · 不执行", json!({"type":"skill_reference","adapter":"unverified","skill":"unselected"})),
        ("app", "打开应用", "应用", "本地应用意图 · 宿主执行未接入", json!({"type":"open_app","bundle_id":"com.apple.TextEdit"})),
        ("shell-command", "命令动作", "命令", "默认关闭 · /bin/sh · 10 秒上限", json!({"type":"shell_command","command":"printf command-marker","working_directory":"/tmp"})),
        ("local-multi", "Multi 动作", "Multi", "本地四分支 · 不执行 · 固件范围未知", json!({"type":"local_multi","multi":{"format_version":1,"term_ms":250,"tap":null,"double_tap":null,"hold":null,"tap_hold":null}})),
        ("local-macro", "普通宏", "普通宏", "自有本地序列 · 已验证键名可宿主执行", json!({"type":"local_macro","macro_sequence":{"format_version":1,"events":[{"type":"press","key":"a"},{"type":"release","key":"a"}]}})),
        ("device-key", "设备普通按键", "设备按键", "来源明确的普通键意图 · 尚未应用", json!({"type":"device_key","token":"KC_B"})),
        ("slot-open-0", "四应用槽位 1 · AG00", "四应用总控", "直跳或长按绑定第 1 个跨应用活跃会话槽位", json!({"type":"agent_command","adapter":"four-app","command":"slot-open:0"})),
        ("slot-open-1", "四应用槽位 2 · AG01", "四应用总控", "直跳或长按绑定第 2 个跨应用活跃会话槽位", json!({"type":"agent_command","adapter":"four-app","command":"slot-open:1"})),
        ("slot-open-2", "四应用槽位 3 · AG02", "四应用总控", "直跳或长按绑定第 3 个跨应用活跃会话槽位", json!({"type":"agent_command","adapter":"four-app","command":"slot-open:2"})),
        ("slot-open-3", "四应用槽位 4 · AG03", "四应用总控", "直跳或长按绑定第 4 个跨应用活跃会话槽位", json!({"type":"agent_command","adapter":"four-app","command":"slot-open:3"})),
        ("slot-open-4", "四应用槽位 5 · AG04", "四应用总控", "直跳或长按绑定第 5 个跨应用活跃会话槽位", json!({"type":"agent_command","adapter":"four-app","command":"slot-open:4"})),
        ("slot-open-5", "四应用槽位 6 · AG05", "四应用总控", "直跳或长按绑定第 6 个跨应用活跃会话槽位", json!({"type":"agent_command","adapter":"four-app","command":"slot-open:5"})),
        ("four-app-attention", "四应用全局待办跳转", "四应用总控", "一键切到 Antigravity / DSH / ZCode / Cindy 中最高优先级待关注会话", json!({"type":"agent_command","adapter":"four-app","command":"four-app-attention"})),
        ("agent-approve", "审批通过 · Approve", "四应用总控", "批准当前或最高优先级待决交互请求", json!({"type":"agent_command","adapter":"host","command":"approve"})),
        ("agent-deny", "审批拒绝 · Deny", "四应用总控", "拒绝当前或最高优先级待决交互请求", json!({"type":"agent_command","adapter":"host","command":"deny"})),
        ("agent-new-or-stop", "新建或停止会话 · New/Stop", "四应用总控", "运行中则停止当前生成，空闲或冷启动则新建会话", json!({"type":"agent_command","adapter":"host","command":"new-or-stop"})),
        ("agent-stop", "停止当前生成 · Stop", "四应用总控", "立即中止当前会话生成流", json!({"type":"agent_command","adapter":"host","command":"stop"})),
        ("next-workspace", "下一工作区 · Next Workspace", "四应用总控", "切换到下一个活跃工作区会话", json!({"type":"agent_command","adapter":"four-app","command":"next-workspace"})),
        ("previous-workspace", "上一工作区 · Previous Workspace", "四应用总控", "切换到上一个活跃工作区会话", json!({"type":"agent_command","adapter":"four-app","command":"previous-workspace"})),
        ("ag-live-voice", "Antigravity · Gemini Live 实时语音", "Agent", "启动或结束 Antigravity Live 实时语音对话并确认状态", json!({"type":"agent_command","adapter":"antigravity","command":"live-voice"})),
        ("ag-prev-conversation", "Antigravity · 上一会话（侧栏轮转）", "Agent", "在左侧栏会话列表中向上切换一条会话（Alt+Up）", json!({"type":"agent_command","adapter":"antigravity","command":"prev-conversation"})),
        ("ag-next-conversation", "Antigravity · 下一会话（侧栏轮转）", "Agent", "在左侧栏会话列表中向下切换一条会话（Alt+Down）", json!({"type":"agent_command","adapter":"antigravity","command":"next-conversation"})),
        ("ag-project", "Antigravity · 选项目 / 新建项目", "Agent", "切换侧栏项目或打开新项目目录（Cmd+;）", json!({"type":"agent_command","adapter":"antigravity","command":"project"})),
        ("ag-sidebar-scroll-up", "在项目中向上滚动", "Agent", "在左侧项目与会话栏向上滚动并切换项目", json!({"type":"agent_command","adapter":"antigravity","command":"sidebar-scroll-up"})),
        ("ag-sidebar-scroll-down", "在项目中向下滚动", "Agent", "在左侧项目与会话栏向下滚动并切换项目", json!({"type":"agent_command","adapter":"antigravity","command":"sidebar-scroll-down"})),
        ("ag-prev-project", "切换到上一项目", "Agent", "在左侧项目列表中向上切换一个项目", json!({"type":"agent_command","adapter":"antigravity","command":"prev-project"})),
        ("ag-next-project", "切换到下一项目", "Agent", "在左侧项目列表中向下切换一个项目", json!({"type":"agent_command","adapter":"antigravity","command":"next-project"})),
        ("ag-cycle-working-prev", "上一个 Working 会话", "Agent", "轮询切换到上一个 Working 运行中的会话（::）", json!({"type":"agent_command","adapter":"antigravity","command":"cycle-working-prev"})),
        ("ag-cycle-working-next", "下一个 Working 会话", "Agent", "轮询切换到下一个 Working 运行中的会话（::）", json!({"type":"agent_command","adapter":"antigravity","command":"cycle-working-next"})),
        ("ag-cycle-completed-prev", "上一个蓝点已完成会话", "Agent", "轮询切换到上一个蓝点已完事待看的会话（🔵）", json!({"type":"agent_command","adapter":"antigravity","command":"cycle-completed-prev"})),
        ("ag-cycle-completed-next", "下一个蓝点已完成会话", "Agent", "轮询切换到下一个蓝点已完事待看的会话（🔵）", json!({"type":"agent_command","adapter":"antigravity","command":"cycle-completed-next"})),
        ("ag-cycle-attention-prev", "上一个 Working 或蓝点会话", "Agent", "轮询切换到上一个 Working 运行中或蓝点已完成的会话", json!({"type":"agent_command","adapter":"antigravity","command":"cycle-attention-prev"})),
        ("ag-cycle-attention-next", "下一个 Working 或蓝点会话", "Agent", "轮询切换到下一个 Working 运行中或蓝点已完成的会话", json!({"type":"agent_command","adapter":"antigravity","command":"cycle-attention-next"})),
        ("ag-split-right", "Antigravity · 快速向右分栏", "Agent", "将当前会话向右分栏（Cmd+D）", json!({"type":"agent_command","adapter":"antigravity","command":"split-right"})),
    ].into_iter().map(|(id,name,category,detail,payload)| Action {id:id.into(),name:name.into(),category:category.into(),detail:detail.into(),payload}).collect()
}

/// Document-scoped identity: source IDs, not array offsets or display names.
/// Logical positions remain schematic until UNKNOWN-10 is resolved.
pub fn binding_key(profile: &Value, layer: &Value, slot: u32) -> String {
    serde_json::to_string(&(profile, layer, format!("logical-{slot}"))).unwrap()
}
#[derive(Clone, Serialize, Deserialize)]
pub struct LegacyDocumentV1 {
    pub schema_version: u32,
    pub config: DeviceConfig,
    pub edits: BTreeMap<String, Action>,
    #[serde(skip)]
    undo: Vec<BTreeMap<String, Action>>,
    #[serde(skip)]
    redo: Vec<BTreeMap<String, Action>>,
    pub revision: u64,
    #[serde(skip_deserializing)]
    pub can_undo: bool,
    #[serde(skip_deserializing)]
    pub can_redo: bool,
    #[serde(skip_deserializing)]
    pub read_only: bool,
}
impl Default for LegacyDocumentV1 {
    fn default() -> Self {
        Self::new(crate::demo().expect("valid bundled fixture")).unwrap()
    }
}
impl LegacyDocumentV1 {
    pub fn new(config: DeviceConfig) -> Result<Self, String> {
        let d = Self {
            schema_version: 1,
            config,
            edits: BTreeMap::new(),
            undo: vec![],
            redo: vec![],
            revision: 0,
            can_undo: false,
            can_redo: false,
            read_only: false,
        };
        d.validate()?;
        Ok(d)
    }
    fn keys(&self) -> Result<BTreeSet<String>, String> {
        let mut profiles = BTreeSet::new();
        let mut keys = BTreeSet::new();
        for p in &self.config.profiles {
            if !(p.id.is_string() || p.id.is_i64() || p.id.is_u64())
                || !profiles.insert(p.id.to_string())
            {
                return Err("配置 ID 缺失或重复".into());
            }
            let mut layers = BTreeSet::new();
            for l in &p.layers {
                if !(l.id.is_string() || l.id.is_i64() || l.id.is_u64())
                    || !layers.insert(l.id.to_string())
                {
                    return Err("层 ID 缺失或重复".into());
                }
                for slot in 0..16 + l.joystick.sectors.len() {
                    keys.insert(binding_key(&p.id, &l.id, slot as u32));
                }
            }
        }
        Ok(keys)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("本地文档版本不支持；原文件保留".into());
        }
        let keys = self.keys()?;
        if self.edits.keys().any(|k| !keys.contains(k)) {
            return Err("草稿包含不存在的控件引用；原文件保留".into());
        }
        Ok(())
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let raw: Value = serde_json::from_slice(bytes).map_err(|_| "本地文档损坏；原文件保留")?;
        if raw["schema_version"] != 1 {
            return Err("本地文档版本不支持；原文件保留".into());
        }
        let mut d: Self =
            serde_json::from_value(raw.clone()).map_err(|_| "本地文档结构不合法；原文件保留")?;
        d.validate()?;
        let mut normalized = serde_json::to_value(&d).map_err(|e| e.to_string())?;
        let mut original = raw.clone();
        for key in ["can_undo", "can_redo", "read_only"] {
            normalized.as_object_mut().unwrap().remove(key);
            original.as_object_mut().unwrap().remove(key);
        }
        let future_fields = normalized != original;
        // Future fields/types must not be silently discarded by a subsequent save.
        d.read_only = future_fields
            || d.edits.values().any(|a| a.validate().is_err())
            || raw.as_object().unwrap().keys().any(|k| {
                ![
                    "schema_version",
                    "config",
                    "edits",
                    "revision",
                    "can_undo",
                    "can_redo",
                    "read_only",
                ]
                .contains(&k.as_str())
            });
        Ok(d)
    }
    pub fn assign(
        &mut self,
        profile: &Value,
        layer: &Value,
        slot: u32,
        action: Action,
    ) -> Result<(), String> {
        if self.read_only {
            return Err("文档含未知动作或字段，仅可浏览；原文件保留".into());
        }
        action.validate()?;
        let key = binding_key(profile, layer, slot);
        if !self.keys()?.contains(&key) {
            return Err("无效配置、层或控件".into());
        }
        if self.edits.get(&key) == Some(&action) {
            return Ok(());
        }
        self.undo.push(self.edits.clone());
        self.redo.clear();
        self.edits.insert(key, action);
        self.changed();
        Ok(())
    }
    pub fn undo(&mut self, redo: bool) -> Result<(), String> {
        if self.read_only {
            return Err("文档仅可浏览".into());
        }
        if redo {
            if let Some(next) = self.redo.pop() {
                self.undo.push(std::mem::replace(&mut self.edits, next));
                self.changed();
            }
        } else if let Some(old) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.edits, old));
            self.changed();
        }
        Ok(())
    }
    fn changed(&mut self) {
        self.revision += 1;
        self.can_undo = !self.undo.is_empty();
        self.can_redo = !self.redo.is_empty();
    }
}

/// Runtime projection plus one authoritative v2 document. History is session-local.
#[derive(Clone, Serialize)]
pub struct Draft {
    pub config: DeviceConfig,
    pub edits: BTreeMap<String, Action>,
    pub revision: u64,
    pub can_undo: bool,
    pub can_redo: bool,
    pub read_only: bool,
    pub loaded_format: u32,
    pub document_id: String,
    pub structure_changes: Vec<String>,
    #[serde(skip)]
    pub document: crate::document::LocalDocumentV2,
    #[serde(skip)]
    undo: Vec<crate::document::EditableState>,
    #[serde(skip)]
    redo: Vec<crate::document::EditableState>,
}
impl Default for Draft {
    fn default() -> Self {
        Self::new(crate::demo().expect("valid fixture")).unwrap()
    }
}
impl Draft {
    pub fn new(config: DeviceConfig) -> Result<Self, String> {
        let doc = crate::document::LocalDocumentV2::from_config(&config, None, "synthetic-local")?;
        Self::from_document(doc, 2, false)
    }
    pub fn from_document(
        document: crate::document::LocalDocumentV2,
        loaded_format: u32,
        read_only: bool,
    ) -> Result<Self, String> {
        let (config, edits) = document.view()?;
        Ok(Self {
            structure_changes: document.structure_changes(),
            revision: document.revision,
            document_id: document.document_id.clone(),
            config,
            edits,
            can_undo: false,
            can_redo: false,
            read_only,
            loaded_format,
            document,
            undo: vec![],
            redo: vec![],
        })
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        use sha2::{Digest, Sha256};
        let raw: Value = serde_json::from_slice(bytes).map_err(|_| "本地文档损坏；原文件保留")?;
        match raw["schema_version"].as_u64() {
            Some(1) => {
                let legacy = LegacyDocumentV1::from_bytes(bytes)?;
                let doc = crate::document::LocalDocumentV2::from_legacy(
                    &legacy,
                    Some(format!("{:x}", Sha256::digest(bytes))),
                )?;
                Self::from_document(doc, 1, legacy.read_only)
            }
            Some(2) => {
                let doc: crate::document::LocalDocumentV2 = serde_json::from_value(raw.clone())
                    .map_err(|_| "本地文档结构不合法；原文件保留")?;
                doc.validate()?;
                let read_only = serde_json::to_value(&doc).map_err(|e| e.to_string())? != raw
                    || !doc.actions_known();
                Self::from_document(doc, 2, read_only)
            }
            _ => Err("本地文档版本不支持；原文件保留".into()),
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        self.document.validate()
    }
    pub fn mutate<T>(
        &mut self,
        edit: impl FnOnce(&mut crate::document::LocalDocumentV2) -> Result<T, String>,
    ) -> Result<T, String> {
        if self.read_only {
            return Err("文档含未知动作或字段，仅可浏览；原文件保留".into());
        }
        let mut next = self.document.clone();
        let result = edit(&mut next)?;
        next.validate()?;
        if next.editable == self.document.editable {
            return Ok(result);
        }
        next.revision = self.revision.checked_add(1).ok_or("文档修订号已达上限")?;
        let (config, edits) = next.view()?;
        self.undo.push(self.document.editable.clone());
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.document = next;
        self.config = config;
        self.edits = edits;
        self.refresh();
        Ok(result)
    }
    fn refresh(&mut self) {
        self.structure_changes = self.document.structure_changes();
        self.revision = self.document.revision;
        self.can_undo = !self.undo.is_empty();
        self.can_redo = !self.redo.is_empty();
    }
    pub fn create_profile(&mut self, name: &str) -> Result<String, String> {
        self.mutate(|d| d.create_profile(name))
    }
    pub fn create_layer(&mut self, pid: &str, name: &str) -> Result<String, String> {
        self.mutate(|d| d.create_layer(pid, name))
    }
    pub fn create_protected_oai_layer(&mut self, pid: &str, name: &str) -> Result<String, String> {
        self.mutate(|d| d.create_protected_oai_layer(pid, name))
    }
    pub fn merge_keys_across_layers(
        &mut self,
        pid: &str,
        primary_slot: u32,
        secondary_slot: u32,
        action: Option<Action>,
    ) -> Result<Vec<String>, String> {
        self.mutate(|d| d.merge_keys_across_layers(pid, primary_slot, secondary_slot, action))
    }
    pub fn compile_compact_host_keymap(
        &self,
        active_profile_id: Option<&str>,
    ) -> Result<Vec<u8>, String> {
        crate::compile_compact_host_keymap(&self.document, active_profile_id)
    }
    pub fn compile_all_ag_host_keymap(
        &self,
        active_profile_id: Option<&str>,
    ) -> Result<Vec<u8>, String> {
        crate::compile_all_ag_host_keymap(&self.document, active_profile_id)
    }
    pub fn duplicate_profile(&mut self, pid: &str, name: &str) -> Result<String, String> {
        self.mutate(|d| d.duplicate_profile(pid, name))
    }
    pub fn duplicate_layer(&mut self, pid: &str, lid: &str, name: &str) -> Result<String, String> {
        self.mutate(|d| d.duplicate_layer(pid, lid, name))
    }
    pub fn rename_profile(&mut self, id: &str, name: &str) -> Result<(), String> {
        self.mutate(|d| d.rename_profile(id, name))
    }
    pub fn rename_layer(&mut self, id: &str, name: &str) -> Result<(), String> {
        self.mutate(|d| d.rename_layer(id, name))
    }
    pub fn move_profile(&mut self, id: &str, index: usize) -> Result<(), String> {
        self.mutate(|d| d.move_profile(id, index))
    }
    pub fn move_layer(&mut self, pid: &str, id: &str, index: usize) -> Result<(), String> {
        self.mutate(|d| d.move_layer(pid, id, index))
    }
    pub fn delete_profile(&mut self, id: &str) -> Result<(), String> {
        self.mutate(|d| d.delete_profile(id))
    }
    pub fn delete_layer(&mut self, pid: &str, id: &str) -> Result<(), String> {
        self.mutate(|d| d.delete_layer(pid, id))
    }
    pub fn assign(
        &mut self,
        profile: &Value,
        layer: &Value,
        slot: u32,
        action: Action,
    ) -> Result<(), String> {
        action.validate()?;
        let pid = profile.as_str().ok_or("配置必须是本地 UUID")?;
        let lid = layer.as_str().ok_or("层必须是本地 UUID")?;
        self.mutate(|d| {
            let p = d.editable.graph.profiles.get(pid).ok_or("配置不存在")?;
            if !p.layer_order.iter().any(|id| id == lid) {
                return Err("层不属于所选配置".into());
            }
            if d.is_protected_oai_layer(lid) {
                return Err("受保护宿主层（ChatGPT / Codex / Cindy）不支持修改按键映射".into());
            }
            let l = d.editable.graph.layers.get_mut(lid).ok_or("层不存在")?;
            let binding = l
                .controls
                .values_mut()
                .find(|b| b.logical_control == format!("logical-{slot}"))
                .ok_or("控件不存在")?;
            if binding
                .action_id
                .as_ref()
                .and_then(|id| d.editable.graph.actions.get(id))
                == Some(&action)
            {
                return Ok(());
            }
            let old = binding.action_id.replace(crate::document::new_id());
            d.editable
                .graph
                .actions
                .insert(binding.action_id.clone().unwrap(), action);
            if let Some(old) = old {
                if !d.editable.graph.actions.get(&old).is_some_and(|a| {
                    crate::macro_library::is_local_macro(a) || crate::multi::is_local_multi(a)
                }) && crate::multi::parent_references(d, &old).is_empty()
                    && !d.editable.graph.layers.values().any(|l| {
                        l.controls
                            .values()
                            .any(|b| b.action_id.as_ref() == Some(&old))
                    })
                {
                    d.editable.graph.actions.remove(&old);
                    d.editable.source_refs.remove(&old);
                }
            }
            Ok(())
        })
    }
    /// Bind an existing local action using stable ownership IDs; no action is executed.
    pub fn bind_existing_action(
        &mut self,
        profile_id: &str,
        layer_id: &str,
        binding_id: &str,
        action_id: Option<&str>,
    ) -> Result<(), String> {
        self.mutate(|d| {
            if !d
                .editable
                .graph
                .profiles
                .get(profile_id)
                .is_some_and(|p| p.layer_order.iter().any(|id| id == layer_id))
            {
                return Err("层不属于配置".into());
            }
            if d.is_protected_oai_layer(layer_id) {
                return Err("受保护宿主层（ChatGPT / Codex / Cindy）不支持绑定动作".into());
            }
            let graph = &mut d.editable.graph;
            if let Some(id) = action_id {
                graph.actions.get(id).ok_or("动作引用不存在")?.validate()?;
            }
            let binding = graph
                .layers
                .get_mut(layer_id)
                .ok_or("层不存在")?
                .controls
                .get_mut(binding_id)
                .ok_or("控件不属于层")?;
            binding.action_id = action_id.map(str::to_owned);
            Ok(())
        })
    }
    pub fn undo(&mut self, redo: bool) -> Result<(), String> {
        if self.read_only {
            return Err("文档仅可浏览".into());
        }
        let stack = if redo { &self.redo } else { &self.undo };
        let Some(state) = stack.last().cloned() else {
            return Ok(());
        };
        let mut next = self.document.clone();
        next.editable = state;
        next.revision = self.revision.checked_add(1).ok_or("文档修订号已达上限")?;
        next.validate()?;
        let (config, edits) = next.view()?;
        if redo {
            self.redo.pop();
            self.undo.push(self.document.editable.clone());
        } else {
            self.undo.pop();
            self.redo.push(self.document.editable.clone());
        }
        self.document = next;
        self.config = config;
        self.edits = edits;
        self.refresh();
        Ok(())
    }
}
