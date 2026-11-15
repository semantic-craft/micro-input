use serde::{Deserialize, Serialize};

use crate::draft::Action;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UiLanguagePreference {
    #[default]
    #[serde(rename = "system")]
    System,
    #[serde(rename = "zh-Hans")]
    ZhHans,
    #[serde(rename = "en")]
    En,
}

impl UiLanguagePreference {
    pub const fn is_default(&self) -> bool {
        matches!(self, Self::System)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::ZhHans => "zh-Hans",
            Self::En => "en",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResolvedLocale {
    #[default]
    #[serde(rename = "zh-Hans")]
    ZhHans,
    #[serde(rename = "en")]
    En,
}

impl ResolvedLocale {
    pub const fn index(self) -> usize {
        match self {
            Self::ZhHans => 0,
            Self::En => 1,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ZhHans => "zh-Hans",
            Self::En => "en",
        }
    }

    pub const fn resolve(pref: UiLanguagePreference, system_locale: Self) -> Self {
        match pref {
            UiLanguagePreference::System => system_locale,
            UiLanguagePreference::ZhHans => Self::ZhHans,
            UiLanguagePreference::En => Self::En,
        }
    }

    pub fn from_bcp47_tags<S: AsRef<str>>(tags: &[S]) -> Self {
        for tag in tags {
            let normalized = tag.as_ref().trim().to_ascii_lowercase();
            if normalized.is_empty() {
                continue;
            }
            if normalized == "zh" || normalized.starts_with("zh-") || normalized.starts_with("zh_")
            {
                return Self::ZhHans;
            }
            if normalized == "en" || normalized.starts_with("en-") || normalized.starts_with("en_")
            {
                return Self::En;
            }
        }
        Self::ZhHans
    }

    pub const fn select<'a>(self, zh: &'a str, en: &'a str) -> &'a str {
        match self {
            Self::ZhHans => zh,
            Self::En => en,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionPresentation {
    pub name: String,
    pub short_label: String,
    pub cheat_short: String,
    pub summary: String,
    pub category: String,
}

pub fn en_lookup(zh: &str) -> Option<&'static str> {
    MODEL_EN_TABLE
        .iter()
        .find(|(k, _)| *k == zh)
        .map(|(_, v)| *v)
}

pub fn tr<'a>(zh: &'a str, locale: ResolvedLocale) -> &'a str {
    match locale {
        ResolvedLocale::ZhHans => zh,
        ResolvedLocale::En => en_lookup(zh).unwrap_or(zh),
    }
}

pub fn is_all_category_filter(filter: &str) -> bool {
    let trimmed = filter.trim();
    trimmed.is_empty() || trimmed == "全部" || trimmed.eq_ignore_ascii_case("all")
}

pub fn all_category_label(locale: ResolvedLocale) -> &'static str {
    locale.select("全部", "All")
}

pub fn localize_category(cat: &str, locale: ResolvedLocale) -> String {
    let trimmed = cat.trim();
    if is_all_category_filter(trimmed) {
        return all_category_label(locale).to_string();
    }
    match locale {
        ResolvedLocale::ZhHans => canonical_category_key(trimmed).to_string(),
        ResolvedLocale::En => match canonical_category_key(trimmed) {
            "快捷键" => "Shortcuts".to_string(),
            "文本" => "Text".to_string(),
            "提交控制" => "Submit".to_string(),
            "导航" => "Navigation".to_string(),
            "应用" => "Apps".to_string(),
            "命令" => "Commands".to_string(),
            "普通宏" => "Macros".to_string(),
            "设备按键" => "Device Keys".to_string(),
            "四应用总控" => "Four-App".to_string(),
            "鼠标控制" => "Mouse".to_string(),
            "Codex 会话" => "Codex Sessions".to_string(),
            "会话" => "Sessions".to_string(),
            other => other.to_string(),
        },
    }
}

pub fn canonical_category_key(cat: &str) -> &str {
    match cat.trim() {
        "Shortcuts" => "快捷键",
        "Text" => "文本",
        "Submit" => "提交控制",
        "Navigation" => "导航",
        "Apps" => "应用",
        "Commands" => "命令",
        "Macros" => "普通宏",
        "Device Keys" => "设备按键",
        "Four-App" => "四应用总控",
        "Mouse" => "鼠标控制",
        "Codex Sessions" => "Codex 会话",
        "Sessions" => "会话",
        other => other,
    }
}

pub fn localize_profile_name(name: &str, locale: ResolvedLocale) -> String {
    match locale {
        ResolvedLocale::ZhHans => name.to_string(),
        ResolvedLocale::En => {
            if let Some(base) = name.strip_suffix(" 副本") {
                return format!("{} Copy", localize_profile_name(base, locale));
            }
            match name {
                "其他软件" => "Other Apps".to_string(),
                "Coding 起步" => "Coding Starter".to_string(),
                "Codex · 原生" => "Codex · Native".to_string(),
                "通用 Coding · 演示" => "General Coding · Demo".to_string(),
                "Herdr · 示例" => "Herdr · Sample".to_string(),
                "新建方案" => "New Profile".to_string(),
                "自定义方案" => "Custom Profile".to_string(),
                "预设方案" => "Preset Profile".to_string(),
                _ => name.to_string(),
            }
        }
    }
}

pub fn localize_layer_name(name: &str, locale: ResolvedLocale) -> String {
    match locale {
        ResolvedLocale::ZhHans => name.to_string(),
        ResolvedLocale::En => {
            if let Some(base) = name.strip_suffix(" 副本") {
                return format!("{} Copy", localize_layer_name(base, locale));
            }
            match name {
                "默认层" => "Default Layer".to_string(),
                "基础层" => "Base Layer".to_string(),
                "辅助层" => "Secondary Layer".to_string(),
                "画布层" => "Canvas Layer".to_string(),
                "自定义层" => "Custom Layer".to_string(),
                "四应用总控" => "Four-App Cockpit".to_string(),
                _ => {
                    if let Some(inner) = name.strip_prefix('第').and_then(|s| s.strip_suffix('层'))
                    {
                        if let Ok(num) = inner.trim().parse::<u32>() {
                            return format!("Layer {num}");
                        }
                    }
                    name.to_string()
                }
            }
        }
    }
}

pub fn localize_action_label(name: &str, locale: ResolvedLocale) -> String {
    match locale {
        ResolvedLocale::ZhHans => name.to_string(),
        ResolvedLocale::En => {
            let (localized, _, _) = localize_action_name_en(name);
            if localized != name {
                return localized;
            }
            en_lookup(name).unwrap_or(name).to_string()
        }
    }
}

pub fn localize_action(action: &Action, locale: ResolvedLocale) -> ActionPresentation {
    let category = localize_category(&action.category, locale);
    match locale {
        ResolvedLocale::ZhHans => ActionPresentation {
            name: action.name.clone(),
            short_label: compact_zh_label(&action.name),
            cheat_short: compact_zh_cheat(&action.name),
            summary: action.detail.clone(),
            category,
        },
        ResolvedLocale::En => {
            let (name, short_label, cheat_short) = localize_action_name_en(&action.name);
            let summary = localize_action_detail_en(&action.detail);
            ActionPresentation {
                name,
                short_label,
                cheat_short,
                summary,
                category,
            }
        }
    }
}

pub fn cheat_short_for_binding(
    raw_cheat_short: &str,
    binding_id: &str,
    locale: ResolvedLocale,
) -> String {
    if locale == ResolvedLocale::ZhHans {
        return raw_cheat_short.to_string();
    }
    let normalized = binding_id.trim().to_ascii_uppercase();
    if matches!(normalized.as_str(), "AG15" | "AG16" | "AG17" | "AG18") {
        if let Some(quad) = QUADRANT_OVERRIDES
            .iter()
            .find(|(k, _)| *k == raw_cheat_short)
            .map(|(_, v)| *v)
        {
            return quad.to_string();
        }
        return fit_quadrant_budget(raw_cheat_short);
    }
    if matches!(normalized.as_str(), "AG13" | "AG14" | "ENC_CLK") {
        return fit_ascii_budget(raw_cheat_short, 12);
    }
    fit_ascii_budget(raw_cheat_short, 14)
}

fn fit_ascii_budget(text: &str, max_chars: usize) -> String {
    let cleaned = text.replace('…', "").trim().to_string();
    if cleaned.chars().count() <= max_chars {
        return cleaned;
    }
    let mut words = Vec::new();
    let mut len = 0usize;
    for w in cleaned.split_whitespace() {
        let wlen = w.chars().count();
        let next_len = if words.is_empty() {
            wlen
        } else {
            len + 1 + wlen
        };
        if next_len <= max_chars {
            words.push(w);
            len = next_len;
        } else {
            break;
        }
    }
    if !words.is_empty() {
        words.join(" ")
    } else {
        cleaned.chars().take(max_chars).collect()
    }
}

fn fit_quadrant_budget(text: &str) -> String {
    let cleaned = text.replace('…', "").trim().to_string();
    let mut out: Vec<String> = Vec::new();
    let mut total = 0usize;
    for raw_word in cleaned.split_whitespace() {
        if out.len() >= 2 {
            break;
        }
        let word: String = raw_word.chars().take(6).collect();
        if word.is_empty() {
            continue;
        }
        let next_total = if out.is_empty() {
            word.chars().count()
        } else {
            total + 1 + word.chars().count()
        };
        if next_total <= 12 {
            total = next_total;
            out.push(word);
        } else {
            break;
        }
    }
    if out.is_empty() {
        cleaned.chars().take(6).collect()
    } else {
        out.join(" ")
    }
}

fn compact_zh_label(raw: &str) -> String {
    let base = raw.split(" · ").next().unwrap_or(raw).trim();
    if base.is_empty() {
        "未绑定".to_string()
    } else {
        base.to_string()
    }
}

fn compact_zh_cheat(raw: &str) -> String {
    let base = compact_zh_label(raw);
    match base.as_str() {
        "打开 / 聚焦应用" => "聚焦应用".into(),
        "选项目 / 新建项目" => "选/建项目".into(),
        "项目工作区快切" => "工作区快切".into(),
        "Gemini Live 实时语音" => "实时语音".into(),
        "打开终端面板" => "终端面板".into(),
        "快速向右分栏" => "向右分栏".into(),
        "内置浏览器 · 布局" | "打开内置浏览器" => "内置浏览器".into(),
        "聚焦当前输入框" => "聚焦输入框".into(),
        "按住说话" | "按住说话 · 松开停止" => "按住说话".into(),
        "停止听写并提交" => "停止提交".into(),
        "向上滚动对话" | "在项目中向上滚动" => "向上滚动".into(),
        "向下滚动对话" | "在项目中向下滚动" => "向下滚动".into(),
        "跳到底部并聚焦输入" => "跳到底部".into(),
        "显示 / 隐藏侧栏" => "显隐侧栏".into(),
        "新建 Tab / 终端" => "新建 Tab".into(),
        "聚焦上方面板" | "北向分屏焦点" | "向上切分屏焦点" => "上方面板".into(),
        "聚焦左方面板" | "西向分屏焦点" | "向左切分屏焦点" => "左方面板".into(),
        "聚焦下方面板" | "南向分屏焦点" | "向下切分屏焦点" => "下方面板".into(),
        "聚焦右方面板" | "东向分屏焦点" | "向右切分屏焦点" => "右方面板".into(),
        "放大 / 还原面板" => "缩放面板".into(),
        other => {
            if other.chars().count() <= 6 {
                other.to_string()
            } else {
                let prefix: String = other.chars().take(5).collect();
                format!("{prefix}…")
            }
        }
    }
}

fn localize_action_name_en(zh_name: &str) -> (String, String, String) {
    if let Some((en_full, en_short, en_cheat)) = ACTION_NAME_EN_TABLE
        .iter()
        .find(|(zh, _, _, _)| *zh == zh_name)
        .map(|(_, full, short, cheat)| (*full, *short, *cheat))
    {
        return (
            en_full.to_string(),
            en_short.to_string(),
            en_cheat.to_string(),
        );
    }
    if let Some(badge) = zh_name.strip_prefix("快捷键 ") {
        let full = format!("Shortcut {badge}");
        let cheat = fit_ascii_budget(&badge.replace('…', ""), 12);
        let cheat_non_empty = if cheat.is_empty() {
            "Shortcut".to_string()
        } else {
            cheat
        };
        return (full.clone(), full, cheat_non_empty);
    }
    let preserved = zh_name.to_string();
    let cheat = fit_ascii_budget(&preserved, 14);
    (preserved.clone(), preserved, cheat)
}

fn localize_action_detail_en(zh_detail: &str) -> String {
    if let Some(en) = ACTION_DETAIL_EN_TABLE
        .iter()
        .find(|(zh, _)| *zh == zh_detail)
        .map(|(_, en)| *en)
    {
        return en.to_string();
    }
    if let Some(target) = zh_detail.strip_prefix("自定义录制快捷键 · 目标 ") {
        return format!("Custom recorded shortcut · Target {target}");
    }
    zh_detail.to_string()
}

const QUADRANT_OVERRIDES: &[(&str, &str)] = &[
    ("Session 1", "Slot 1"),
    ("Session 2", "Slot 2"),
    ("Session 3", "Slot 3"),
    ("Session 4", "Slot 4"),
    ("Session 5", "Slot 5"),
    ("Session 6", "Slot 6"),
    ("Focus Input", "Input"),
    ("Browser", "Browse"),
    ("Terminal", "Term"),
    ("Workspaces", "Spaces"),
    ("Select Model", "Model"),
    ("Pick Model", "Model"),
    ("Projects", "Proj"),
    ("Search Chat", "Search"),
    ("Toggle Side", "Side"),
    ("Sidebar", "Side"),
    ("Jump Bottom", "Jump Down"),
    ("Unbound", "Empty"),
    ("Approve", "Accept"),
    ("Decline", "Reject"),
    ("Interrupt", "Abort"),
    ("Confirm", "Select"),
    ("Dictate", "Voice"),
    ("Fn Dictate", "Fn Mic"),
    ("Commands", "Cmds"),
    ("History", "Hist"),
    ("Explain", "Detail"),
    ("Refactor", "Clean"),
    ("Continue", "Resume"),
    ("Summary", "Recap"),
    ("Shortcut", "Key"),
    ("Command", "Shell"),
];

const ACTION_NAME_EN_TABLE: &[(&str, &str, &str, &str)] = &[
    ("未绑定", "Unbound", "Unbound", "Unbound"),
    ("未分配", "Unassigned", "Unassigned", "Empty"),
    ("新建宏", "New Macro", "New Macro", "New Macro"),
    ("新建 Multi", "New Multi", "New Multi", "New Multi"),
    ("普通宏", "Local Macro", "Macro", "Macro"),
    ("Multi 动作", "Multi Action", "Multi", "Multi"),
    ("设备普通按键", "Device Key", "Device Key", "Device Key"),
    ("合成动作", "Synthetic Action", "Synthetic", "Synthetic"),
    (
        "粘贴指令并提交（显式回车）",
        "Paste Prompt & Submit",
        "Paste & Send",
        "Paste Send",
    ),
    (
        "停止听写并提交",
        "Stop Dictation & Submit",
        "Stop & Send",
        "Stop & Send",
    ),
    (
        "停止听写并提交 · AG12",
        "Stop Dictation & Submit · AG12",
        "Stop & Send",
        "Stop & Send",
    ),
    (
        "发送消息（先停听写）",
        "Send Message (Stop Dictation First)",
        "Send Message",
        "Send Msg",
    ),
    ("提交当前输入", "Submit Current Input", "Submit", "Submit"),
    ("同意继续", "Approve & Continue", "Approve", "Approve"),
    (
        "同意当前会话请求",
        "Approve Session Request",
        "Approve",
        "Approve",
    ),
    (
        "批准当前或最高优先级待决交互请求",
        "Approve Pending Request",
        "Approve",
        "Approve",
    ),
    ("审批通过 · AG07", "Approve · AG07", "Approve", "Approve"),
    ("审批通过 · Approve", "Approve", "Approve", "Approve"),
    ("取消 / 拒绝", "Cancel / Decline", "Decline", "Decline"),
    (
        "拒绝当前会话请求",
        "Decline Session Request",
        "Decline",
        "Decline",
    ),
    (
        "拒绝 / 取消当前操作",
        "Cancel Current Operation",
        "Cancel",
        "Cancel",
    ),
    ("审批拒绝 · AG08", "Decline · AG08", "Decline", "Decline"),
    ("审批拒绝 · Deny", "Decline · Deny", "Decline", "Decline"),
    ("中断当前生成", "Stop Generation", "Stop", "Stop"),
    ("停止当前生成 · Stop", "Stop Generation", "Stop", "Stop"),
    ("发送中断", "Send Interrupt", "Interrupt", "Interrupt"),
    (
        "新建或停止会话 · AG09",
        "New or Stop Session · AG09",
        "New / Stop",
        "New / Stop",
    ),
    (
        "新建或停止会话 · New/Stop",
        "New or Stop Session",
        "New / Stop",
        "New / Stop",
    ),
    ("新建聊天", "New Chat", "New Chat", "New Chat"),
    ("新建会话", "New Session", "New Session", "New Chat"),
    ("新建对话", "New Conversation", "New Chat", "New Chat"),
    (
        "在项目中新建会话",
        "New Chat in Project",
        "Project Chat",
        "Proj Chat",
    ),
    ("新建标签页", "New Tab", "New Tab", "New Tab"),
    (
        "新建 Tab / 终端",
        "New Tab / Terminal",
        "New Tab",
        "New Tab",
    ),
    ("新建工作区", "New Workspace", "New Space", "New Space"),
    ("新建 Space", "New Space", "New Space", "New Space"),
    (
        "打开 / 聚焦应用",
        "Open / Focus App",
        "Focus App",
        "Focus App",
    ),
    ("打开应用", "Open Application", "Open App", "Open App"),
    ("打开 Herdr", "Open Herdr", "Open Herdr", "Open Herdr"),
    (
        "打开 / 聚焦 Antigravity",
        "Open / Focus Antigravity",
        "Antigravity",
        "Focus App",
    ),
    (
        "聚焦 Antigravity",
        "Focus Antigravity",
        "Antigravity",
        "Focus App",
    ),
    (
        "聚焦当前输入框",
        "Focus Current Input",
        "Focus Input",
        "Focus Input",
    ),
    (
        "选项目 / 新建项目",
        "Select / New Project",
        "Projects",
        "Projects",
    ),
    (
        "Antigravity · 选项目 / 新建项目",
        "Antigravity · Select / New Project",
        "Projects",
        "Projects",
    ),
    (
        "项目工作区快切",
        "Switch Workspace",
        "Workspaces",
        "Workspaces",
    ),
    ("选择模型", "Select Model", "Select Model", "Pick Model"),
    (
        "弹出模型选择页",
        "Open Model Picker",
        "Pick Model",
        "Pick Model",
    ),
    (
        "选择上一模型 / 档位",
        "Previous Model / Effort",
        "Prev Model",
        "Prev Model",
    ),
    (
        "选择下一模型 / 档位",
        "Next Model / Effort",
        "Next Model",
        "Next Model",
    ),
    (
        "返回模型列表 / 调低模型力度",
        "Back / Lower Effort",
        "Model Left",
        "Model Left",
    ),
    (
        "展开 / 调高模型力度",
        "Expand / Raise Effort",
        "Model Right",
        "Model Right",
    ),
    (
        "确认所选模型",
        "Confirm Selected Model",
        "Confirm",
        "Confirm",
    ),
    ("调整思考力度", "Adjust Thinking Effort", "Effort", "Effort"),
    (
        "Gemini Live 实时语音",
        "Gemini Live Voice",
        "Live Voice",
        "Live Voice",
    ),
    (
        "Antigravity · Gemini Live 实时语音",
        "Antigravity · Gemini Live Voice",
        "Live Voice",
        "Live Voice",
    ),
    ("实时语音", "Live Voice", "Live Voice", "Live Voice"),
    (
        "按住说话 · 松开停止",
        "Hold to Talk · Release to Stop",
        "Hold to Talk",
        "Hold Talk",
    ),
    (
        "按住语音 · AG10 (2U 左)",
        "Hold to Talk · AG10 (2U Left)",
        "Hold to Talk",
        "Hold Talk",
    ),
    (
        "按住语音 · AG11 (2U 右)",
        "Hold to Talk · AG11 (2U Right)",
        "Hold to Talk",
        "Hold Talk",
    ),
    (
        "听写 / 语音输入",
        "Dictation / Voice Input",
        "Dictation",
        "Dictate",
    ),
    ("点击听写", "Toggle Dictation", "Dictation", "Dictate"),
    (
        "苹果 Fn 键（语音输入）",
        "Apple Fn Key (Dictation)",
        "Fn Dictate",
        "Fn Dictate",
    ),
    (
        "打开终端面板",
        "Toggle Terminal Panel",
        "Terminal",
        "Terminal",
    ),
    ("切换终端", "Toggle Terminal", "Terminal", "Terminal"),
    (
        "显示 / 隐藏终端",
        "Show / Hide Terminal",
        "Terminal",
        "Terminal",
    ),
    ("快速向右分栏", "Split Right", "Split Right", "Split Right"),
    (
        "Antigravity · 快速向右分栏",
        "Antigravity · Split Right",
        "Split Right",
        "Split Right",
    ),
    ("向右分屏", "Split Right", "Split Right", "Split Right"),
    ("向下分屏", "Split Down", "Split Down", "Split Down"),
    (
        "内置浏览器 · 布局",
        "Built-in Browser · Layout",
        "Browser",
        "Browser",
    ),
    (
        "打开内置浏览器",
        "Open Built-in Browser",
        "Browser",
        "Browser",
    ),
    (
        "Antigravity · 打开内置浏览器",
        "Antigravity · Open Browser",
        "Browser",
        "Browser",
    ),
    (
        "轮换工作区布局",
        "Cycle Workspace Layout",
        "Layout",
        "Layout",
    ),
    (
        "切换栈视图",
        "Toggle Stack View",
        "Stack View",
        "Stack View",
    ),
    ("切换窗格缩放", "Toggle Pane Zoom", "Zoom Pane", "Zoom Pane"),
    (
        "放大 / 还原面板",
        "Zoom / Restore Pane",
        "Zoom Pane",
        "Zoom Pane",
    ),
    (
        "关闭当前窗格",
        "Close Current Pane",
        "Close Pane",
        "Close Pane",
    ),
    (
        "显示 / 隐藏侧栏",
        "Toggle Sidebar",
        "Toggle Side",
        "Sidebar",
    ),
    ("切换侧边栏", "Toggle Sidebar", "Toggle Side", "Sidebar"),
    (
        "切换 Agent 面板",
        "Toggle Agent Panel",
        "Agent Panel",
        "Agent",
    ),
    (
        "搜索会话",
        "Search Conversations",
        "Search Chat",
        "Search Chat",
    ),
    ("搜索任务", "Search Tasks", "Search Task", "Search"),
    ("搜索命令", "Command Palette", "Commands", "Commands"),
    ("快速搜索文件", "Quick Open File", "Find File", "Find File"),
    ("搜索文件", "Search Files", "Find File", "Find File"),
    ("历史搜索", "Search History", "History", "History"),
    ("清屏", "Clear Screen", "Clear", "Clear"),
    (
        "保存当前文件",
        "Save Current File",
        "Save File",
        "Save File",
    ),
    ("保存文件", "Save File", "Save File", "Save File"),
    ("插入审查提示", "Insert Review Prompt", "Review", "Review"),
    ("注入计划提示词", "Insert Plan Prompt", "Plan", "Plan"),
    ("注入审查提示词", "Insert Review Prompt", "Review", "Review"),
    ("注入调试提示词", "Insert Debug Prompt", "Debug", "Debug"),
    ("注入测试提示词", "Insert Test Prompt", "Test", "Test"),
    (
        "注入解释提示词",
        "Insert Explain Prompt",
        "Explain",
        "Explain",
    ),
    (
        "注入重构提示词",
        "Insert Refactor Prompt",
        "Refactor",
        "Refactor",
    ),
    ("注入验证提示词", "Insert Verify Prompt", "Verify", "Verify"),
    (
        "注入文件上下文",
        "Insert File Context",
        "File Ctx",
        "File Ctx",
    ),
    ("工程指令 · Review", "Prompt · Review", "Review", "Review"),
    ("工程指令 · Test", "Prompt · Test", "Test", "Test"),
    ("工程指令 · Commit", "Prompt · Commit", "Commit", "Commit"),
    (
        "工程指令 · Continue",
        "Prompt · Continue",
        "Continue",
        "Continue",
    ),
    (
        "工程指令 · Summary",
        "Prompt · Summary",
        "Summary",
        "Summary",
    ),
    ("打开网址", "Open URL", "Open URL", "Open URL"),
    ("Skill 引用", "Skill Reference", "Skill", "Skill"),
    ("命令动作", "Shell Command", "Command", "Command"),
    ("向上滚动对话", "Scroll Chat Up", "Scroll Up", "Scroll Up"),
    (
        "向下滚动对话",
        "Scroll Chat Down",
        "Scroll Down",
        "Scroll Down",
    ),
    (
        "在项目中向上滚动",
        "Scroll Up in Project",
        "Scroll Up",
        "Scroll Up",
    ),
    (
        "在项目中向下滚动",
        "Scroll Down in Project",
        "Scroll Down",
        "Scroll Down",
    ),
    (
        "Codex · 向上滚动对话",
        "Codex · Scroll Chat Up",
        "Scroll Up",
        "Scroll Up",
    ),
    (
        "Codex · 向下滚动对话",
        "Codex · Scroll Chat Down",
        "Scroll Down",
        "Scroll Down",
    ),
    (
        "跳到底部并聚焦输入",
        "Jump to Bottom & Focus Input",
        "Jump Bottom",
        "Jump Bottom",
    ),
    (
        "Codex · 跳到底部并聚焦输入",
        "Codex · Jump to Bottom",
        "Jump Bottom",
        "Jump Bottom",
    ),
    (
        "旋钮逆时针 · 向上滚屏",
        "Dial CCW · Scroll Up",
        "Scroll Up",
        "Scroll Up",
    ),
    (
        "旋钮顺时针 · 向下滚屏",
        "Dial CW · Scroll Down",
        "Scroll Down",
        "Scroll Down",
    ),
    (
        "旋钮按压 · 跳到底部",
        "Dial Press · Jump to Bottom",
        "Jump Bottom",
        "Jump Bottom",
    ),
    (
        "Codex · 降低推理档位",
        "Codex · Lower Effort",
        "Effort -",
        "Effort -",
    ),
    (
        "Codex · 提高推理档位",
        "Codex · Raise Effort",
        "Effort +",
        "Effort +",
    ),
    (
        "Codex · 切换 Fast 模式",
        "Codex · Toggle Fast Mode",
        "Fast Mode",
        "Fast Mode",
    ),
    (
        "降低推理档位",
        "Lower Reasoning Effort",
        "Effort -",
        "Effort -",
    ),
    (
        "提高推理档位",
        "Raise Reasoning Effort",
        "Effort +",
        "Effort +",
    ),
    (
        "切换 Fast 模式",
        "Toggle Fast Mode",
        "Fast Mode",
        "Fast Mode",
    ),
    ("历史后退", "History Back", "Back", "Back"),
    ("历史前进", "History Forward", "Forward", "Go Fwd"),
    (
        "后退到上一个打开目标",
        "Back to Previous Target",
        "Back",
        "Back",
    ),
    (
        "前进到下一个打开目标",
        "Forward to Next Target",
        "Forward",
        "Go Fwd",
    ),
    (
        "上一个蓝点已完成会话",
        "Previous Completed Session",
        "Prev Done",
        "Prev Done",
    ),
    (
        "下一个蓝点已完成会话",
        "Next Completed Session",
        "Next Done",
        "Next Done",
    ),
    (
        "上一个 Working 会话",
        "Previous Working Session",
        "Prev Work",
        "Prev Work",
    ),
    (
        "下一个 Working 会话",
        "Next Working Session",
        "Next Work",
        "Next Work",
    ),
    (
        "上一个 Working 或蓝点会话",
        "Previous Active/Done Session",
        "Prev Active",
        "Prev Active",
    ),
    (
        "下一个 Working 或蓝点会话",
        "Next Active/Done Session",
        "Next Active",
        "Next Active",
    ),
    (
        "上一会话",
        "Previous Conversation",
        "Prev Chat",
        "Prev Chat",
    ),
    ("下一会话", "Next Conversation", "Next Chat", "Next Chat"),
    (
        "上一会话（侧栏轮转）",
        "Previous Conversation (Cycle)",
        "Prev Chat",
        "Prev Chat",
    ),
    (
        "下一会话（侧栏轮转）",
        "Next Conversation (Cycle)",
        "Next Chat",
        "Next Chat",
    ),
    (
        "Antigravity · 上一会话（侧栏轮转）",
        "Antigravity · Previous Chat",
        "Prev Chat",
        "Prev Chat",
    ),
    (
        "Antigravity · 下一会话（侧栏轮转）",
        "Antigravity · Next Chat",
        "Next Chat",
        "Next Chat",
    ),
    (
        "切换到上一项目",
        "Previous Project",
        "Prev Proj",
        "Prev Proj",
    ),
    ("切换到下一项目", "Next Project", "Next Proj", "Next Proj"),
    (
        "上一工作区 · Previous Workspace",
        "Previous Workspace",
        "Prev Space",
        "Prev Space",
    ),
    (
        "下一工作区 · Next Workspace",
        "Next Workspace",
        "Next Space",
        "Next Space",
    ),
    ("上一个 Space", "Previous Space", "Prev Space", "Prev Space"),
    ("下一个 Space", "Next Space", "Next Space", "Next Space"),
    ("上一个 Tab", "Previous Tab", "Prev Tab", "Prev Tab"),
    ("下一个 Tab", "Next Tab", "Next Tab", "Next Tab"),
    ("上一标签页", "Previous Tab", "Prev Tab", "Prev Tab"),
    ("下一标签页", "Next Tab", "Next Tab", "Next Tab"),
    ("下一个 pane", "Next Pane", "Next Pane", "Next Pane"),
    (
        "选择 Space / Tab",
        "Select Space / Tab",
        "Space / Tab",
        "Space / Tab",
    ),
    ("切换会话", "Switch Session", "Switch Chat", "Switch"),
    (
        "查看会话状态",
        "View Session Status",
        "Chat Status",
        "Status",
    ),
    (
        "四应用全局待办跳转",
        "Jump to Pending Attention",
        "Next Alert",
        "Next Alert",
    ),
    (
        "四应用全局待办跳转 · AG06",
        "Jump to Pending Attention · AG06",
        "Next Alert",
        "Next Alert",
    ),
    ("会话 1", "Session 1", "Session 1", "Session 1"),
    ("会话 2", "Session 2", "Session 2", "Session 2"),
    ("会话 3", "Session 3", "Session 3", "Session 3"),
    ("会话 4", "Session 4", "Session 4", "Session 4"),
    ("会话 5", "Session 5", "Session 5", "Session 5"),
    ("会话 6", "Session 6", "Session 6", "Session 6"),
    (
        "四应用槽位 1 · AG00",
        "Cockpit Slot 1 · AG00",
        "Slot 1",
        "Slot 1",
    ),
    (
        "四应用槽位 2 · AG01",
        "Cockpit Slot 2 · AG01",
        "Slot 2",
        "Slot 2",
    ),
    (
        "四应用槽位 3 · AG02",
        "Cockpit Slot 3 · AG02",
        "Slot 3",
        "Slot 3",
    ),
    (
        "四应用槽位 4 · AG03",
        "Cockpit Slot 4 · AG03",
        "Slot 4",
        "Slot 4",
    ),
    (
        "四应用槽位 5 · AG04",
        "Cockpit Slot 5 · AG04",
        "Slot 5",
        "Slot 5",
    ),
    (
        "四应用槽位 6 · AG05",
        "Cockpit Slot 6 · AG05",
        "Slot 6",
        "Slot 6",
    ),
    (
        "聚焦 Session 1",
        "Focus Session 1",
        "Session 1",
        "Session 1",
    ),
    (
        "聚焦 Session 2",
        "Focus Session 2",
        "Session 2",
        "Session 2",
    ),
    (
        "聚焦 Session 3",
        "Focus Session 3",
        "Session 3",
        "Session 3",
    ),
    (
        "聚焦 Session 4",
        "Focus Session 4",
        "Session 4",
        "Session 4",
    ),
    (
        "聚焦 Session 5",
        "Focus Session 5",
        "Session 5",
        "Session 5",
    ),
    (
        "聚焦 Session 6",
        "Focus Session 6",
        "Session 6",
        "Session 6",
    ),
    ("聚焦 DSH 会话 1", "Focus DSH Session 1", "DSH 1", "DSH 1"),
    ("聚焦 DSH 会话 2", "Focus DSH Session 2", "DSH 2", "DSH 2"),
    ("北向分屏焦点", "Focus North Pane", "Pane Up", "Pane Up"),
    ("南向分屏焦点", "Focus South Pane", "Pane Down", "Pane Down"),
    ("西向分屏焦点", "Focus West Pane", "Pane Left", "Pane Left"),
    (
        "东向分屏焦点",
        "Focus East Pane",
        "Pane Right",
        "Pane Right",
    ),
    ("聚焦上方面板", "Focus Upper Pane", "Pane Up", "Pane Up"),
    ("聚焦下方面板", "Focus Lower Pane", "Pane Down", "Pane Down"),
    ("聚焦左方面板", "Focus Left Pane", "Pane Left", "Pane Left"),
    (
        "聚焦右方面板",
        "Focus Right Pane",
        "Pane Right",
        "Pane Right",
    ),
    ("向上切分屏焦点", "Focus Pane Up", "Pane Up", "Pane Up"),
    (
        "向下切分屏焦点",
        "Focus Pane Down",
        "Pane Down",
        "Pane Down",
    ),
    (
        "向左切分屏焦点",
        "Focus Pane Left",
        "Pane Left",
        "Pane Left",
    ),
    (
        "向右切分屏焦点",
        "Focus Pane Right",
        "Pane Right",
        "Pane Right",
    ),
    ("鼠标指针上移", "Move Cursor Up", "Move Up", "Move Up"),
    ("鼠标指针下移", "Move Cursor Down", "Move Down", "Move Down"),
    ("鼠标指针左移", "Move Cursor Left", "Move Left", "Move Left"),
    (
        "鼠标指针右移",
        "Move Cursor Right",
        "Move Right",
        "Move Right",
    ),
    ("鼠标左键点击", "Left Click", "Left Click", "Left Click"),
    ("鼠标右键点击", "Right Click", "Right Click", "Right Click"),
    ("鼠标中键点击", "Middle Click", "Mid Click", "Mid Click"),
    ("鼠标滚轮向上", "Scroll Wheel Up", "Scroll Up", "Scroll Up"),
    (
        "鼠标滚轮向下",
        "Scroll Wheel Down",
        "Scroll Down",
        "Scroll Down",
    ),
    (
        "鼠标滚轮向左",
        "Scroll Wheel Left",
        "Scroll Left",
        "Scroll Left",
    ),
    (
        "鼠标滚轮向右",
        "Scroll Wheel Right",
        "Scroll Right",
        "Scroll Right",
    ),
    (
        "鼠标速度档位 0 · 精细",
        "Mouse Speed 0 · Precision",
        "Speed 0",
        "Speed 0",
    ),
    (
        "鼠标速度档位 1 · 标准",
        "Mouse Speed 1 · Standard",
        "Speed 1",
        "Speed 1",
    ),
    (
        "鼠标速度档位 2 · 快速",
        "Mouse Speed 2 · Fast",
        "Speed 2",
        "Speed 2",
    ),
    (
        "循环切换鼠标速度档位",
        "Cycle Mouse Speed",
        "Mouse Speed",
        "Mouse Speed",
    ),
    (
        "板载速查切换 · KI_CS_TOGGLE",
        "Onboard Cheat Sheet Toggle · KI_CS_TOGGLE",
        "Cheat Sheet",
        "Cheat Sheet",
    ),
    (
        "板载速查按住显示 · KI_CS_SHOW_TMP",
        "Onboard Hold Cheat Sheet · KI_CS_SHOW_TMP",
        "Hold Sheet",
        "Hold Sheet",
    ),
    (
        "板载速查显示 · KI_CS_SHOW",
        "Onboard Show Cheat Sheet · KI_CS_SHOW",
        "Show Sheet",
        "Show Sheet",
    ),
    (
        "板载速查隐藏 · KI_CS_HIDE",
        "Onboard Hide Cheat Sheet · KI_CS_HIDE",
        "Hide Sheet",
        "Hide Sheet",
    ),
    (
        "板载鼠标上移 · MS_UP",
        "Onboard Cursor Up · MS_UP",
        "Move Up",
        "Move Up",
    ),
    (
        "板载鼠标下移 · MS_DOWN",
        "Onboard Cursor Down · MS_DOWN",
        "Move Down",
        "Move Down",
    ),
    (
        "板载鼠标左移 · MS_LEFT",
        "Onboard Cursor Left · MS_LEFT",
        "Move Left",
        "Move Left",
    ),
    (
        "板载鼠标右移 · MS_RGHT",
        "Onboard Cursor Right · MS_RGHT",
        "Move Right",
        "Move Right",
    ),
    (
        "板载鼠标左键 · MS_BTN1",
        "Onboard Left Click · MS_BTN1",
        "Left Click",
        "Left Click",
    ),
    (
        "板载鼠标右键 · MS_BTN2",
        "Onboard Right Click · MS_BTN2",
        "Right Click",
        "Right Click",
    ),
    (
        "板载鼠标中键 · MS_BTN3",
        "Onboard Middle Click · MS_BTN3",
        "Mid Click",
        "Mid Click",
    ),
    (
        "板载鼠标按键 4 · MS_BTN4",
        "Onboard Mouse Button 4 · MS_BTN4",
        "Button 4",
        "Btn 4",
    ),
    (
        "板载鼠标按键 5 · MS_BTN5",
        "Onboard Mouse Button 5 · MS_BTN5",
        "Button 5",
        "Btn 5",
    ),
    (
        "板载鼠标按键 6 · MS_BTN6",
        "Onboard Mouse Button 6 · MS_BTN6",
        "Button 6",
        "Btn 6",
    ),
    (
        "板载鼠标按键 7 · MS_BTN7",
        "Onboard Mouse Button 7 · MS_BTN7",
        "Button 7",
        "Btn 7",
    ),
    (
        "板载鼠标按键 8 · MS_BTN8",
        "Onboard Mouse Button 8 · MS_BTN8",
        "Button 8",
        "Btn 8",
    ),
    (
        "板载滚轮向上 · MS_WHLU",
        "Onboard Wheel Up · MS_WHLU",
        "Scroll Up",
        "Scroll Up",
    ),
    (
        "板载滚轮向下 · MS_WHLD",
        "Onboard Wheel Down · MS_WHLD",
        "Scroll Down",
        "Scroll Down",
    ),
    (
        "板载滚轮向左 · MS_WHLL",
        "Onboard Wheel Left · MS_WHLL",
        "Scroll Left",
        "Scroll Left",
    ),
    (
        "板载滚轮向右 · MS_WHLR",
        "Onboard Wheel Right · MS_WHLR",
        "Scroll Right",
        "Scroll Right",
    ),
    (
        "板载鼠标速度档 0 · MS_ACL0",
        "Onboard Speed 0 · MS_ACL0",
        "Speed 0",
        "Speed 0",
    ),
    (
        "板载鼠标速度档 1 · MS_ACL1",
        "Onboard Speed 1 · MS_ACL1",
        "Speed 1",
        "Speed 1",
    ),
    (
        "板载鼠标速度档 2 · MS_ACL2",
        "Onboard Speed 2 · MS_ACL2",
        "Speed 2",
        "Speed 2",
    ),
];

const ACTION_DETAIL_EN_TABLE: &[(&str, &str)] = &[
    (
        "需要前台编辑器 · 仅草稿",
        "Requires frontmost editor · Draft only",
    ),
    (
        "默认仅粘贴不回车（no_newline） · 自动暂存恢复剪贴板",
        "Pastes without Enter (no_newline) · Preserves clipboard",
    ),
    (
        "显式配置下先停听写、聚焦目标输入区、暂存剪贴板粘贴后再回车",
        "Stops dictation first, focuses input, pastes via staged clipboard, then submits",
    ),
    (
        "先停活跃听写再聚焦输入区回车提交",
        "Stops active dictation first, focuses input, then presses Enter",
    ),
    (
        "若听写激活则先停并确认停止，再聚焦目标输入区回车提交",
        "Stops active dictation if running, focuses target input, then submits",
    ),
    (
        "若听写激活则先停并确认停止，再聚焦输入区回车提交",
        "Stops active dictation if running, focuses input, then presses Enter",
    ),
    (
        "若听写激活则先停并确认停止，再聚焦输入区按 Cmd+Enter 提交",
        "Stops active dictation if running, focuses input, then presses Cmd+Enter",
    ),
    (
        "若听写激活则先停并确认停止，再聚焦输入区提交",
        "Stops active dictation if running, focuses input, then submits",
    ),
    (
        "合成 Apple Magic Keyboard 专属 🌐 Fn (0x3F flagsChanged) · 支持点按与长按语音输入",
        "Synthesizes Apple 🌐 Fn (0x3F flagsChanged) · Tap or hold for dictation",
    ),
    (
        "2U 宽键按住说话，松开自动停止听写",
        "Hold 2U wide key to talk, release to stop dictation",
    ),
    (
        "2U 宽键按住启动听写（Record Audio / Ctrl+M），松开自动停止",
        "Hold 2U wide key to dictate (Record Audio / Ctrl+M), release to stop",
    ),
    (
        "按住启动听写（Record Audio / Ctrl+M），松开自动停止",
        "Hold to start dictation (Record Audio / Ctrl+M), release to stop",
    ),
    (
        "按一次开始听写，再按一次停止",
        "Press once to start dictation, press again to stop",
    ),
    (
        "启动或结束 Antigravity Live 实时语音对话并确认状态",
        "Starts or stops Antigravity Gemini Live voice and verifies state",
    ),
    (
        "启动语音聊天（Ctrl+Shift+V），需应用提供语音功能",
        "Starts voice chat (Ctrl+Shift+V) when supported by target app",
    ),
    (
        "目标应用的原生控件；未提供时明确提示",
        "Uses target app native control; reports clearly when unavailable",
    ),
    (
        "目标应用的原生对话滚动与输入框定位；不抢占非目标窗口焦点",
        "Scrolls chat and focuses input natively without stealing unrelated window focus",
    ),
    (
        "跨 Agent 键盘打开历史后退（保留前进栈）",
        "Navigates back in cross-agent history (preserving forward stack)",
    ),
    (
        "跨 Agent 键盘打开历史前进",
        "Navigates forward in cross-agent history",
    ),
    (
        "自有本地序列 · 已验证键名可宿主执行",
        "Local macro sequence · Verified keys execute on macOS host",
    ),
    (
        "本地四分支 · 不执行 · 固件范围未知",
        "Local 4-branch Multi · Draft only · Firmware scope unverified",
    ),
    (
        "来源明确的普通键意图 · 尚未应用",
        "Onboard key token intent · Pending device apply",
    ),
    (
        "默认关闭 · /bin/sh · 10 秒上限",
        "Disabled by default · /bin/sh · 10s timeout",
    ),
    (
        "本地应用意图 · 宿主执行未接入",
        "Local app intent · Host execution not wired",
    ),
    ("适配器未验证 · 不执行", "Unverified adapter · Not executed"),
    (
        "仅保存文本，不执行",
        "Saves reference text only; not executed",
    ),
    ("仅保存网址，不打开", "Saves URL only; does not open"),
    (
        "激活 Antigravity 并聚焦当前对话输入框（Cmd+L）",
        "Activates Antigravity and focuses chat input (Cmd+L)",
    ),
    (
        "在当前项目中新建对话（Cmd+N）",
        "Creates a new conversation in current project (Cmd+N)",
    ),
    (
        "在当前或最近项目中新建会话（New Conversation in Project）",
        "Creates a new conversation in current or recent project",
    ),
    (
        "在当前窗口新建标签页（Cmd+T）",
        "Opens a new tab in current window (Cmd+T)",
    ),
    ("新建聊天（Cmd+N）", "Starts a new chat (Cmd+N)"),
    (
        "切换侧栏项目或打开新项目目录（Cmd+;）",
        "Switches project or opens project folder (Cmd+;)",
    ),
    (
        "打开模型与思考档位选择器（Cmd+/ 或原生控件）",
        "Opens model and thinking effort picker (Cmd+/ or native control)",
    ),
    (
        "打开当前目标 Agent 的模型选择弹层并进入旋钮选模型与推理力度会话",
        "Opens target Agent model picker for dial selection and effort tuning",
    ),
    (
        "在已打开的模型选择弹层中向上移动高亮项（直接向窗口投递按键，不重聚焦输入框）",
        "Moves selection up in open model picker without refocusing input",
    ),
    (
        "在已打开的模型选择弹层中向下移动高亮项（直接向窗口投递按键，不重聚焦输入框）",
        "Moves selection down in open model picker without refocusing input",
    ),
    (
        "在已打开的模型选择弹层中向左返回模型列表或调低模型推理力度档位",
        "Returns to model list or lowers reasoning effort in model picker",
    ),
    (
        "在已打开的模型选择弹层中向右展开子菜单或调高模型推理力度档位",
        "Expands submenu or raises reasoning effort in model picker",
    ),
    (
        "在已打开的模型选择弹层中回车确认当前高亮项（直接向窗口投递按键，不重聚焦输入框）",
        "Confirms highlighted item in open model picker",
    ),
    (
        "切换底部终端面板（Cmd+J 或原生控件）",
        "Toggles bottom terminal panel (Cmd+J or native control)",
    ),
    (
        "显示或隐藏终端（Ctrl+`）",
        "Shows or hides terminal (Ctrl+`)",
    ),
    (
        "将当前会话向右分栏（Cmd+D）",
        "Splits current session to the right (Cmd+D)",
    ),
    (
        "切换右侧内置浏览器与预览面板（Cmd+Shift+B 或原生控件）",
        "Toggles built-in browser and preview panel (Cmd+Shift+B or native)",
    ),
    (
        "切换内置浏览器与工作区布局（Cmd+Shift+B）",
        "Cycles built-in browser and workspace layout (Cmd+Shift+B)",
    ),
    (
        "轮换完整视图、分屏与隐藏标签页（Cmd+Shift+B）",
        "Cycles full view, split view, and hidden tabs (Cmd+Shift+B)",
    ),
    (
        "打开会话搜索面板（Search Conversations / Cmd+K）",
        "Opens conversation search (Cmd+K)",
    ),
    (
        "平滑向上滚动当前对话消息区",
        "Smoothly scrolls current conversation up",
    ),
    (
        "平滑向下滚动当前对话消息区",
        "Smoothly scrolls current conversation down",
    ),
    (
        "一键跳回当前对话流最底部",
        "Jumps to the bottom of the current conversation",
    ),
    (
        "向上滚动 Codex 当前对话内容（不抢占窗口焦点）",
        "Scrolls Codex conversation up without stealing window focus",
    ),
    (
        "向下滚动 Codex 当前对话内容（不抢占窗口焦点）",
        "Scrolls Codex conversation down without stealing window focus",
    ),
    (
        "跳回 Codex 对话底部（存在 Scroll to bottom 按钮时优先点击，否则发送向下滚动）",
        "Jumps to bottom of Codex chat",
    ),
    (
        "后台打开 Codex 推理档位浮层并向左降低一档，停手约 1.5 秒自动收起",
        "Lowers Codex reasoning effort by one step",
    ),
    (
        "后台打开 Codex 推理档位浮层并向右提高一档，停手约 1.5 秒自动收起",
        "Raises Codex reasoning effort by one step",
    ),
    (
        "后台打开 Codex 推理浮层并切换 Fast / Standard 响应速度",
        "Toggles Codex Fast / Standard response mode",
    ),
    (
        "切换并回读对应状态的 Codex 会话",
        "Switches to matching Codex session",
    ),
    (
        "轮询切换到上一个蓝点已完事待看的会话（🔵）",
        "Cycles to previous unread completed session (🔵)",
    ),
    (
        "轮询切换到下一个蓝点已完事待看的会话（🔵）",
        "Cycles to next unread completed session (🔵)",
    ),
    (
        "轮询切换到上一个 Working 运行中的会话（::）",
        "Cycles to previous working session (::)",
    ),
    (
        "轮询切换到下一个 Working 运行中的会话（::）",
        "Cycles to next working session (::)",
    ),
    (
        "轮询切换到上一个 Working 运行中或蓝点已完成的会话",
        "Cycles to previous working or unread completed session",
    ),
    (
        "轮询切换到下一个 Working 运行中或蓝点已完成的会话",
        "Cycles to next working or unread completed session",
    ),
    (
        "在左侧栏会话列表中向上切换一条会话（Alt+Up）",
        "Selects previous conversation in sidebar (Alt+Up)",
    ),
    (
        "在左侧栏会话列表中向下切换一条会话（Alt+Down）",
        "Selects next conversation in sidebar (Alt+Down)",
    ),
    (
        "在左侧项目列表中向上切换一个项目",
        "Switches to previous project in sidebar",
    ),
    (
        "在左侧项目列表中向下切换一个项目",
        "Switches to next project in sidebar",
    ),
    (
        "在左侧项目与会话栏向上滚动并切换项目",
        "Scrolls up in project and session sidebar",
    ),
    (
        "在左侧项目与会话栏向下滚动并切换项目",
        "Scrolls down in project and session sidebar",
    ),
    (
        "切换到上一个活跃工作区会话",
        "Switches to previous active workspace",
    ),
    (
        "切换到下一个活跃工作区会话",
        "Switches to next active workspace",
    ),
    (
        "一键切到最高优先级待关注会话",
        "Jumps to highest-priority session needing attention",
    ),
    (
        "一键切到 Antigravity / DSH 中最高优先级待关注会话",
        "Jumps to highest-priority session needing attention",
    ),
    (
        "直跳或长按绑定第 1 个跨应用活跃会话槽位",
        "Jumps to or pins cross-app session slot 1",
    ),
    (
        "直跳或长按绑定第 2 个跨应用活跃会话槽位",
        "Jumps to or pins cross-app session slot 2",
    ),
    (
        "直跳或长按绑定第 3 个跨应用活跃会话槽位",
        "Jumps to or pins cross-app session slot 3",
    ),
    (
        "直跳或长按绑定第 4 个跨应用活跃会话槽位",
        "Jumps to or pins cross-app session slot 4",
    ),
    (
        "直跳或长按绑定第 5 个跨应用活跃会话槽位",
        "Jumps to or pins cross-app session slot 5",
    ),
    (
        "直跳或长按绑定第 6 个跨应用活跃会话槽位",
        "Jumps to or pins cross-app session slot 6",
    ),
    ("同意执行当前工具调用", "Approves current tool execution"),
    (
        "批准当前或最高优先级待决交互请求",
        "Approves current or top-priority pending request",
    ),
    (
        "拒绝当前或最高优先级待决交互请求",
        "Declines current or top-priority pending request",
    ),
    (
        "立即中止当前会话生成流",
        "Immediately stops active generation",
    ),
    (
        "运行中则停止当前生成，空闲或冷启动则新建会话",
        "Stops generation if running, or starts a new session when idle",
    ),
    (
        "按当前速度档位向上移动系统鼠标指针（CGEvent）",
        "Moves macOS cursor up at current speed preset (CGEvent)",
    ),
    (
        "按当前速度档位向下移动系统鼠标指针（CGEvent）",
        "Moves macOS cursor down at current speed preset (CGEvent)",
    ),
    (
        "按当前速度档位向左移动系统鼠标指针（CGEvent）",
        "Moves macOS cursor left at current speed preset (CGEvent)",
    ),
    (
        "按当前速度档位向右移动系统鼠标指针（CGEvent）",
        "Moves macOS cursor right at current speed preset (CGEvent)",
    ),
    (
        "在当前光标位置发送左键按下与抬起（CGEvent）",
        "Clicks left mouse button at cursor (CGEvent)",
    ),
    (
        "在当前光标位置发送右键按下与抬起（CGEvent）",
        "Clicks right mouse button at cursor (CGEvent)",
    ),
    (
        "在当前光标位置发送中键按下与抬起（CGEvent）",
        "Clicks middle mouse button at cursor (CGEvent)",
    ),
    (
        "在当前光标处垂直向上滚动（CGEvent）",
        "Scrolls vertically up at cursor (CGEvent)",
    ),
    (
        "在当前光标处垂直向下滚动（CGEvent）",
        "Scrolls vertically down at cursor (CGEvent)",
    ),
    (
        "在当前光标处水平向左滚动（CGEvent）",
        "Scrolls horizontally left at cursor (CGEvent)",
    ),
    (
        "在当前光标处水平向右滚动（CGEvent）",
        "Scrolls horizontally right at cursor (CGEvent)",
    ),
    (
        "切换鼠标移动与滚轮精细低速档（8px / 2行）",
        "Switches host mouse/wheel to Precision speed (8px / 2 lines)",
    ),
    (
        "切换鼠标移动与滚轮标准中速档（22px / 4行）",
        "Switches host mouse/wheel to Standard speed (22px / 4 lines)",
    ),
    (
        "切换鼠标移动与滚轮快速高速档（52px / 9行）",
        "Switches host mouse/wheel to Fast speed (52px / 9 lines)",
    ),
    (
        "在精细(0)、标准(1)、快速(2) 之间循环切换鼠标速度档位",
        "Cycles host mouse speed between Precision (0), Standard (1), and Fast (2)",
    ),
    (
        "固件 0.6.3+ 原生板载按键速查切换触发器（经 Vendor 通知唤醒）",
        "Firmware 0.6.3+ onboard Cheat Sheet toggle token",
    ),
    (
        "固件 0.6.3+ 原生按住显示 / 松开隐藏按键速查浮窗触发器",
        "Firmware 0.6.3+ onboard hold-to-show Cheat Sheet token",
    ),
    (
        "固件 0.6.3+ 原生显式打开桌面按键速查浮窗触发器",
        "Firmware 0.6.3+ onboard show Cheat Sheet token",
    ),
    (
        "固件 0.6.3+ 原生显式隐藏桌面按键速查浮窗触发器",
        "Firmware 0.6.3+ onboard hide Cheat Sheet token",
    ),
    (
        "固件 0.7.0-rc.16+ 原生板载鼠标指针上移硬件令牌",
        "Firmware 0.7.0-rc.16+ onboard mouse cursor up token",
    ),
    (
        "固件 0.7.0-rc.16+ 原生板载鼠标指针下移硬件令牌",
        "Firmware 0.7.0-rc.16+ onboard mouse cursor down token",
    ),
    (
        "固件 0.7.0-rc.16+ 原生板载鼠标指针左移硬件令牌",
        "Firmware 0.7.0-rc.16+ onboard mouse cursor left token",
    ),
    (
        "固件 0.7.0-rc.16+ 原生板载鼠标指针右移硬件令牌",
        "Firmware 0.7.0-rc.16+ onboard mouse cursor right token",
    ),
    (
        "固件 0.7.0-rc.16+ 原生板载鼠标左键硬件令牌",
        "Firmware 0.7.0-rc.16+ onboard left mouse button token",
    ),
    (
        "固件 0.7.0-rc.16+ 原生板载鼠标右键硬件令牌",
        "Firmware 0.7.0-rc.16+ onboard right mouse button token",
    ),
    (
        "固件 0.7.0-rc.16+ 原生板载鼠标中键硬件令牌",
        "Firmware 0.7.0-rc.16+ onboard middle mouse button token",
    ),
    (
        "固件 0.7.0-rc.16+ 原生板载鼠标后退侧键硬件令牌",
        "Firmware 0.7.0-rc.16+ onboard mouse button 4 (Back) token",
    ),
    (
        "固件 0.7.0-rc.16+ 原生板载鼠标前进侧键硬件令牌",
        "Firmware 0.7.0-rc.16+ onboard mouse button 5 (Forward) token",
    ),
    (
        "固件 0.7.0-rc.16+ 原生板载鼠标扩展键 6 硬件令牌",
        "Firmware 0.7.0-rc.16+ onboard mouse button 6 token",
    ),
    (
        "固件 0.7.0-rc.16+ 原生板载鼠标扩展键 7 硬件令牌",
        "Firmware 0.7.0-rc.16+ onboard mouse button 7 token",
    ),
    (
        "固件 0.7.0-rc.16+ 原生板载鼠标扩展键 8 硬件令牌",
        "Firmware 0.7.0-rc.16+ onboard mouse button 8 token",
    ),
    (
        "固件 0.7.0-rc.16+ 原生板载垂直滚轮向上硬件令牌",
        "Firmware 0.7.0-rc.16+ onboard vertical wheel up token",
    ),
    (
        "固件 0.7.0-rc.16+ 原生板载垂直滚轮向下硬件令牌",
        "Firmware 0.7.0-rc.16+ onboard vertical wheel down token",
    ),
    (
        "固件 0.7.0-rc.16+ 原生板载水平滚轮向左硬件令牌",
        "Firmware 0.7.0-rc.16+ onboard horizontal wheel left token",
    ),
    (
        "固件 0.7.0-rc.16+ 原生板载水平滚轮向右硬件令牌",
        "Firmware 0.7.0-rc.16+ onboard horizontal wheel right token",
    ),
    (
        "固件 0.7.0-rc.16+ 原生板载鼠标低速加速档硬件令牌",
        "Firmware 0.7.0-rc.16+ onboard low mouse acceleration token",
    ),
    (
        "固件 0.7.0-rc.16+ 原生板载鼠标中速加速档硬件令牌",
        "Firmware 0.7.0-rc.16+ onboard medium mouse acceleration token",
    ),
    (
        "固件 0.7.0-rc.16+ 原生板载鼠标高速加速档硬件令牌",
        "Firmware 0.7.0-rc.16+ onboard high mouse acceleration token",
    ),
    (
        "请检查边界条件与回归测试",
        "Check boundary conditions and regression tests",
    ),
    (
        "请先给出分阶段实施与验证计划",
        "Provide a phased implementation and verification plan",
    ),
    (
        "请运行 check 并验证当前改动",
        "Run check and verify current changes",
    ),
    (
        "请在不改变外部行为的前提下简化当前实现",
        "Simplify current implementation without changing external behavior",
    ),
    (
        "请保持公共接口不变并重构此模块",
        "Refactor this module while preserving its public interface",
    ),
    (
        "请定位根因并给出最小修复方案",
        "Identify the root cause and propose a minimal fix",
    ),
    (
        "请补充覆盖异常分支的单元测试",
        "Add unit tests covering error branches",
    ),
    (
        "请说明这段代码的状态流转与边界条件",
        "Explain state transitions and boundary conditions of this code",
    ),
    (
        "请检查当前变更，优先报告可复现的问题。",
        "Review current changes and prioritize reproducible issues.",
    ),
];

const MODEL_EN_TABLE: &[(&str, &str)] = &[
    ("通用", "App"),
    ("其他软件", "Other Apps"),
    ("默认层", "Default Layer"),
    ("基础层", "Base Layer"),
    ("自定义层", "Custom Layer"),
    ("未选择方案", "No Profile Selected"),
    ("缺失的方案", "Missing Profile"),
    ("已锁定", "Locked"),
    ("手动选择方案", "Manual Profile"),
    ("已暂停软件按键映射", "Key Mapping Paused"),
    ("前台目标", "Frontmost Target"),
    ("系统剪贴板与前台输入焦点", "System Clipboard & Frontmost Input"),
    ("系统默认浏览器", "System Default Browser"),
    ("目标 Agent 窗口", "Target Agent Window"),
    ("打开会话历史", "Session History"),
    ("本机活跃 Agent 会话槽位", "Active Agent Session Slots"),
    ("系统鼠标与光标位置", "System Cursor Position"),
    ("本机 Shell", "Local Shell"),
    ("当前工作台", "Workbench"),
    ("键盘硬件", "Keyboard Hardware"),
    ("辅助功能权限", "Accessibility Permission"),
    ("剪贴板可读写", "Clipboard Access"),
    ("已打开可编辑输入区", "Editable Input Focused"),
    ("无需辅助功能权限", "No Accessibility Permission Needed"),
    ("已连接并选择可验证的目标应用", "Target App Installed & Reachable"),
    ("存在可切换的上一或下一打开目标", "Previous or Next History Target Available"),
    ("存在运行中或已完成的活跃 Agent 会话", "Active or Completed Agent Session Present"),
    ("命令执行开关已开启", "Shell Command Execution Enabled"),
    ("命令内容已配置", "Command Configured"),
    ("宏步骤为空或含未知按键", "Macro is empty or contains unsupported keys"),
    ("宏已配置且键名受 macOS 支持", "Macro keys supported on macOS"),
    ("固件协议与行为范围未确认", "Firmware protocol scope unverified"),
    ("宿主应用动作尚未实现", "Host app action not implemented"),
    ("设备写入门禁尚未开放", "Device write gate not enabled"),
    ("宿主执行已开启", "Host Execution Enabled"),
    ("仅保存引用，不在宿主执行", "Saved as reference only; not executed on host"),
    ("宿主按键事件已发送，不代表目标软件内部命令一定完成", "Key event posted to macOS; target app handling depends on focus"),
    ("剪贴板会尽力恢复；目标输入区未聚焦时可能粘贴失败", "Clipboard restored best-effort; requires focused text input"),
    ("交由 macOS 打开，不验证网页加载结果", "Handed off to macOS default browser"),
    ("通过 Accessibility 或已验证快捷方式触发目标控件，失败时返回明确原因", "Triggers native control via Accessibility or verified shortcut"),
    ("切换已记录的已安装应用窗口，失败时不污染历史游标", "Switches recorded app windows without corrupting history on failure"),
    ("解析实时会话槽位并切换对应 Agent 会话窗口", "Resolves live session slot and activates target Agent session"),
    ("通过 macOS CGEvent 直接驱动系统鼠标移动、点击或滚轮", "Drives macOS cursor movement, clicks, or scroll wheel via CGEvent"),
    ("按超时上限运行 /bin/sh -c，返回退出状态摘要", "Runs /bin/sh -c with timeout cap and reports exit status"),
    ("当前版本仅保存在本地配置，不在宿主执行", "Stored in local configuration only; not executed on host"),
    ("按顺序发送按键按下、抬起与毫秒等待", "Replays key press, release, and millisecond delays in order"),
    ("四分支本地模型已保存；设备固件能力仍属未知项，不在宿主模拟", "4-branch Multi saved locally; not simulated on host"),
    ("仅保存在本地草稿，不向设备写入", "Saved in local draft only; not written to device"),
    ("需要打开软件按键映射", "Enable Key Mapping first"),
    ("需要在检查器开启命令执行", "Enable shell command execution in Inspector"),
    ("动作参数为空或格式无效", "Action parameters are empty or invalid"),
    ("未绑定有效动作", "No valid action bound"),
    ("内置目录", "Built-in Catalog"),
    ("可分配到本地逻辑槽位；宿主执行基于 0.6.3 物理映射验证", "Assignable to local logical slots; host execution verified on 0.6.3 physical layout"),
    ("需固件 0.7.0-rc.16+ 并应用到 /fs/keymap.json；写入后跨 USB/蓝牙主机脱机直接生效", "Requires firmware 0.7.0-rc.16+ applied to /fs/keymap.json; works standalone across USB/BLE hosts"),
    ("板载硬件鼠标键码需审阅并应用到设备后生效；宿主运行时推荐使用「鼠标控制」动作或摇杆切换", "Onboard hardware mouse tokens require applying to device; prefer Mouse actions or joystick mode on host"),
    ("Creator Micro 2 板载固件动作 (0.7.0-rc.16+)", "Creator Micro 2 Onboard Firmware Action (0.7.0-rc.16+)"),
    ("保存只改变本地草稿；应用到设备后跨主机脱机生效", "Saving updates local draft only; takes effect across hosts once applied to device"),
    ("需固件 0.6.3+ 应用到 /fs/keymap.json 且当前 Mac 保持 Vendor HID 连接", "Requires firmware 0.6.3+ applied to /fs/keymap.json with active macOS Vendor HID link"),
    ("按键按下后由固件经 Vendor 通道发送 kb.cs.* 通知唤醒桌面速查浮窗", "Firmware sends kb.cs.* Vendor notification on press to toggle desktop Cheat Sheet"),
    ("Creator Micro 2 板载速查触发器 (0.6.3+)", "Creator Micro 2 Onboard Cheat Sheet Trigger (0.6.3+)"),
    ("应用到设备后按下触发 Vendor 通知并控制桌面速查浮窗", "Triggers Vendor notification and controls desktop Cheat Sheet once applied to device"),
    ("设备按键需审阅并应用配置后生效；不是 Mac 宿主动作", "Device key tokens require applying to hardware; not executed by macOS host"),
    ("Micro Input 自有离线意图；不是已验证的设备或工具能力", "Micro Input local draft intent; not a verified device or host capability"),
    ("保存只改变本地草稿；没有执行结果", "Saving updates local draft only; no host execution"),
    ("目标应用须支持此快捷键；辅助功能权限由宿主按需调用", "Target app must support this shortcut; Accessibility permission requested by host as needed"),
    ("Mac 宿主动作执行器（CGEvent / NSWorkspace）", "macOS Host Executor (CGEvent / NSWorkspace)"),
    ("宿主激活对应 Profile 时按键可实时触发此动作", "Triggers live on macOS when the owning Profile is active"),
    ("本地 Multi 四分支", "Local 4-Branch Multi"),
    ("Multi 执行未接入；时序、容量和设备格式未知", "Multi execution not wired; firmware timing, capacity, and format unverified"),
    ("序列只能使用已接入的 macOS 键名；辅助功能权限由宿主按需调用", "Sequence must use supported macOS key names; Accessibility permission used on demand"),
    ("本地普通宏序列", "Local Macro Sequence"),
    ("Mac 宿主动作执行器（CGEvent 逐步按下 / 释放）", "macOS Host Executor (stepwise CGEvent press / release)"),
    ("宿主激活对应 Profile 时按键可按顺序执行；Multi 与设备驻留不在本能力内", "Replays in order when owning Profile is active; excludes Multi and onboard residency"),
    ("自有本地序列；按键名称不是固件 token，未知键名只保留为草稿", "Local sequence; key names are not firmware tokens—unsupported keys remain draft-only"),
    ("存在未知键名或无效序列；仅支持已验证的 macOS 键名执行", "Contains unsupported key names or invalid sequence; only verified macOS keys execute"),
    ("需要 Mac 宿主常驻与明确的文本输入目标；桌面 Agent 下自动聚焦输入区并暂存恢复原剪贴板", "Requires running macOS host and focused text input; focuses desktop Agent input and preserves clipboard"),
    ("当前文本输入目标", "Focused Text Input"),
    ("Mac 宿主动作执行器（输入区聚焦 + 剪贴板暂存恢复 + Cmd+V）", "macOS Host Executor (input focus + staged clipboard + Cmd+V)"),
    ("默认仅粘贴不自动回车（no_newline=true）；仅在显式配置或按提交键时停止活跃听写并提交", "Pastes without Enter by default (no_newline=true); stops active dictation and submits only when explicit"),
    ("需要 Mac 宿主常驻与可处理 HTTP / HTTPS 的应用", "Requires running macOS host and an HTTP/HTTPS handler app"),
    ("Mac 宿主动作执行器（NSWorkspace URL 打开）", "macOS Host Executor (NSWorkspace URL open)"),
    ("需要 Mac 宿主常驻；目标应用须已安装", "Requires running macOS host; target app must be installed"),
    ("Mac 宿主动作执行器（NSWorkspace 应用激活）", "macOS Host Executor (NSWorkspace app activation)"),
    ("需要 Mac 宿主常驻及至少一条小键盘打开历史", "Requires running macOS host and at least one keypad navigation history entry"),
    ("Mac 宿主跨 Agent 导航历史栈（最多 50 条）", "macOS Host Cross-Agent Navigation History (up to 50 entries)"),
    ("回放小键盘打开历史中的上一/下一可用目标；后退保留前进栈，新打开截断前进栈", "Replays previous/next target in keypad history; Back preserves forward stack"),
    ("固定使用 /bin/sh -c；工作目录必须是绝对路径", "Runs via /bin/sh -c; working directory must be an absolute path"),
    ("10 秒超时；回执只含退出状态、路径和命令字节数", "10s timeout; receipt logs exit code, path, and byte length only"),
    ("Mac 宿主命令执行器（独立同意开关）", "macOS Host Shell Executor (explicit opt-in)"),
    ("成功或失败表示子进程退出状态；命令文本和输出不进入审计", "Reports subprocess exit status; command text and stdout/stderr are never logged"),
    ("命令执行同意未开启；默认不会路由或启动子进程", "Shell command execution is disabled; no subprocess will spawn by default"),
    ("需要 Mac 宿主常驻及对应桌面应用（Antigravity / DSH）会话桥或辅助功能权限", "Requires running macOS host plus desktop Agent session bridge or Accessibility permission"),
    ("Mac 宿主四应用总控路由器（FourAppSlots / 会话审批 / 停止与冷会话物化）", "macOS Host Four-App Router (slots / approval / stop & cold session materialization)"),
    ("宿主激活对应 Profile 时按键可直达跨应用槽位、审批、停止或新建会话", "Jumps to cross-app slots, approves, stops, or creates sessions when Profile is active"),
    ("需要目标桌面 Agent 运行、辅助功能权限及唯一可访问输入框；活跃听写未确认停止时自动阻断提交", "Requires running desktop Agent, Accessibility permission, and unique input; blocks submit until dictation stops"),
    ("聚焦目标输入区、暂存并恢复原剪贴板后仅粘贴文本（no_newline=true，不自动回车）", "Focuses target input, stages/restores clipboard, and pastes without pressing Enter"),
    ("若听写激活则先停并确认停止，随后聚焦目标输入区（粘贴后）回车提交", "Stops active dictation first if running, focuses target input, and submits"),
    ("Mac 宿主防冲突提交与 Prompt 派发器（AX 输入区聚焦 + 听写清理门禁）", "macOS Host Conflict-Safe Submit & Prompt Dispatcher (AX input focus + dictation gate)"),
    ("需要目标桌面 Agent（Antigravity / Codex / DSH）运行及辅助功能权限；弹层内按键直投目标窗口不重聚焦输入框", "Requires running desktop Agent and Accessibility permission; posts keys directly to picker window"),
    ("Mac 宿主目标应用模型选择与推理力度控制器（AX / 窗口直投按键）", "macOS Host Model Picker & Effort Controller (AX / direct window key post)"),
    ("打开目标 Agent 模型选择页，或在不关闭浮层的前提下切换模型、调整推理力度并确认", "Opens target Agent model picker or cycles models, tunes reasoning effort, and confirms"),
    ("需要 Mac 宿主常驻与辅助功能（CGEvent）权限；无焦点抢占移动指针、点击与四向滚轮", "Requires running macOS host and Accessibility (CGEvent) permission; moves cursor/clicks/scrolls without stealing focus"),
    ("Mac 宿主 CoreGraphics 指针、点击、四向滚轮与三档速度控制器", "macOS Host CoreGraphics Cursor, Click, 4-Way Scroll & 3-Speed Controller"),
    ("按当前宿主速度档位（0/1/2）实时移动光标、发送左/右/中键点击或四向滚轮事件", "Moves cursor, clicks left/right/middle, or scrolls 4 ways at current host speed (0/1/2)"),
    ("需要目标应用、辅助功能权限及唯一可访问控件", "Requires target app, Accessibility permission, and a uniquely addressable control"),
    ("Mac 宿主适配器（CLI / 辅助功能）", "macOS Host Adapter (CLI / Accessibility)"),
    ("工具适配器未验证、未接入；引用仅为本地意图", "Adapter unverified and not wired; stored as local draft intent only"),
    ("缺失具体会话引用；请先填写本地意图", "Session reference missing; fill in target session first"),
    ("Mac 宿主 CLI 适配器（herdr）", "macOS Host CLI Adapter (herdr)"),
    ("工具适配器未验证或缺失具体会话引用；无法确认目标会话", "Adapter unverified or session reference missing; cannot resolve target session"),
    ("缺失具体 Skill 引用；请先填写本地意图", "Skill reference missing; fill in skill name first"),
    ("工具适配器未验证、未接入；无法解析 Skill 引用", "Adapter unverified and not wired; cannot resolve Skill reference"),
    ("未知", "Unknown"),
    ("未知动作或参数；原始值保留，不可执行", "Unknown action or parameters; raw payload preserved, not executable"),
    ("粘贴文本 · Continue", "Paste Text · Continue"),
    ("粘贴文本 · Test", "Paste Text · Test"),
    ("粘贴文本 · Review", "Paste Text · Review"),
    ("粘贴文本 · Commit", "Paste Text · Commit"),
    ("粘贴文本 · Summary", "Paste Text · Summary"),
    ("粘贴文本", "Paste Text"),
    ("打开网址", "Open URL"),
    ("聚焦输入框", "Focus Input"),
    ("搜索会话", "Search Conversations"),
    ("新建会话", "New Session"),
    ("在项目中新建会话", "New Chat in Project"),
    ("选择模型", "Select Model"),
    ("弹出模型选择页", "Open Model Picker"),
    ("选择上一模型 / 档位", "Previous Model / Effort"),
    ("选择下一模型 / 档位", "Next Model / Effort"),
    ("确认所选模型", "Confirm Selected Model"),
    ("降低模型力度（左）", "Lower Model Effort (Left)"),
    ("选择 / 提高模型力度（右）", "Select / Raise Model Effort (Right)"),
    ("取消模型选择", "Cancel Model Picker"),
    ("长按动作", "Hold Action"),
    ("思考力度", "Thinking Effort"),
    ("听写 / 语音输入", "Dictation / Voice Input"),
    ("按住说话", "Hold to Talk"),
    ("Gemini Live 实时语音", "Gemini Live Voice"),
    ("选项目 / 新建项目", "Select / New Project"),
    ("快速向右分栏", "Split Right"),
    ("上一会话（侧栏轮转）", "Previous Conversation (Cycle)"),
    ("下一会话（侧栏轮转）", "Next Conversation (Cycle)"),
    ("切换语音回复", "Toggle Voice Reply"),
    ("打开内置浏览器", "Open Built-in Browser"),
    ("打开终端面板", "Toggle Terminal Panel"),
    ("向上滚动对话", "Scroll Chat Up"),
    ("向下滚动对话", "Scroll Chat Down"),
    ("快速向上滚动", "Fast Scroll Up"),
    ("快速向下滚动", "Fast Scroll Down"),
    ("跳到底部并聚焦输入", "Jump to Bottom & Focus Input"),
    ("在项目中向上滚动", "Scroll Up in Project"),
    ("在项目中向下滚动", "Scroll Down in Project"),
    ("上一项目", "Previous Project"),
    ("下一项目", "Next Project"),
    ("上一个 Working 会话", "Previous Working Session"),
    ("下一个 Working 会话", "Next Working Session"),
    ("上一个蓝点已完成会话", "Previous Completed Session"),
    ("下一个蓝点已完成会话", "Next Completed Session"),
    ("上一个 Working 或蓝点会话", "Previous Active/Done Session"),
    ("下一个 Working 或蓝点会话", "Next Active/Done Session"),
    ("后退到上一个打开目标", "Back to Previous Target"),
    ("前进到下一个打开目标", "Forward to Next Target"),
    ("下一个需要我", "Next Needing Attention"),
    ("停止听写并提交", "Stop Dictation & Submit"),
    ("会话 1", "Session 1"),
    ("会话 2", "Session 2"),
    ("会话 3", "Session 3"),
    ("会话 4", "Session 4"),
    ("会话 5", "Session 5"),
    ("会话 6", "Session 6"),
    ("新建标签页", "New Tab"),
    ("内置浏览器 · 布局", "Built-in Browser · Layout"),
    ("点击听写", "Toggle Dictation"),
    ("降低推理档位", "Lower Reasoning Effort"),
    ("提高推理档位", "Raise Reasoning Effort"),
    ("切换 Fast 模式", "Toggle Fast Mode"),
    ("指针上移", "Cursor Up"),
    ("指针下移", "Cursor Down"),
    ("指针左移", "Cursor Left"),
    ("指针右移", "Cursor Right"),
    ("左键点击", "Left Click"),
    ("右键点击", "Right Click"),
    ("中键点击", "Middle Click"),
    ("滚轮向上", "Wheel Up"),
    ("滚轮向下", "Wheel Down"),
    ("滚轮向左", "Wheel Left"),
    ("滚轮向右", "Wheel Right"),
    ("指针精细档 (0)", "Precision Speed (0)"),
    ("指针标准档 (1)", "Standard Speed (1)"),
    ("指针快速档 (2)", "Fast Speed (2)"),
    ("循环指针速度档", "Cycle Cursor Speed"),
    ("粘贴指令并提交", "Paste Prompt & Submit"),
    ("粘贴工程指令", "Paste Prompt"),
    ("Agent 动作", "Agent Action"),
    ("聚焦会话", "Focus Session"),
    ("不可用 / 未配置", "Unavailable / Unbound"),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn contains_cjk(text: &str) -> bool {
        text.chars().any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c))
    }

    fn assert_binding_budgets(action: &Action) {
        let pres = localize_action(action, ResolvedLocale::En);
        assert!(
            !contains_cjk(&pres.name),
            "action '{}' localized name contains CJK: {}",
            action.name,
            pres.name
        );
        assert!(
            !contains_cjk(&pres.short_label),
            "action '{}' localized short_label contains CJK: {}",
            action.name,
            pres.short_label
        );
        assert!(
            !contains_cjk(&pres.cheat_short),
            "action '{}' localized cheat_short contains CJK: {}",
            action.name,
            pres.cheat_short
        );
        assert!(
            !contains_cjk(&pres.summary),
            "action '{}' detail '{}' localized summary contains CJK: {}",
            action.name,
            action.detail,
            pres.summary
        );
        assert!(
            !contains_cjk(&pres.category),
            "action '{}' category '{}' localized category contains CJK: {}",
            action.name,
            action.category,
            pres.category
        );

        for rotary in ["AG13", "AG14", "ENC_CLK"] {
            let label = cheat_short_for_binding(&pres.cheat_short, rotary, ResolvedLocale::En);
            assert!(
                !label.contains('…') && label.chars().count() <= 12,
                "rotary {rotary} budget exceeded for '{}': '{}' ({} chars)",
                action.name,
                label,
                label.chars().count()
            );
        }

        for quadrant in ["AG15", "AG16", "AG17", "AG18"] {
            let label = cheat_short_for_binding(&pres.cheat_short, quadrant, ResolvedLocale::En);
            let words: Vec<&str> = label.split_whitespace().collect();
            assert!(
                !label.contains('…')
                    && label.chars().count() <= 12
                    && words.len() <= 2
                    && words.iter().all(|w| w.chars().count() <= 6),
                "quadrant {quadrant} budget exceeded for '{}': '{}' ({} chars, words={words:?})",
                action.name,
                label,
                label.chars().count()
            );
        }

        for key_1u in ["AG00", "AG06", "AG12"] {
            let label = cheat_short_for_binding(&pres.cheat_short, key_1u, ResolvedLocale::En);
            assert!(
                !label.contains('…') && label.chars().count() <= 14,
                "1U {key_1u} budget exceeded for '{}': '{}' ({} chars)",
                action.name,
                label,
                label.chars().count()
            );
        }
    }

    #[test]
    fn bcp47_resolution_and_preference_override() {
        assert_eq!(
            ResolvedLocale::from_bcp47_tags(&["en-US", "zh-Hans-CN"]),
            ResolvedLocale::En
        );
        assert_eq!(
            ResolvedLocale::from_bcp47_tags(&["zh-Hant-HK", "en"]),
            ResolvedLocale::ZhHans
        );
        assert_eq!(
            ResolvedLocale::from_bcp47_tags(&["ja-JP"]),
            ResolvedLocale::ZhHans
        );
        assert_eq!(
            ResolvedLocale::resolve(UiLanguagePreference::System, ResolvedLocale::En),
            ResolvedLocale::En
        );
        assert_eq!(
            ResolvedLocale::resolve(UiLanguagePreference::ZhHans, ResolvedLocale::En),
            ResolvedLocale::ZhHans
        );
        assert_eq!(
            ResolvedLocale::resolve(UiLanguagePreference::En, ResolvedLocale::ZhHans),
            ResolvedLocale::En
        );
    }

    #[test]
    fn catalog_and_starter_preset_actions_translate_within_spatial_budgets() {
        for action in crate::draft::actions() {
            assert_binding_budgets(&action);
        }

        let catalog_en = crate::action_library::search_localized(
            "",
            "All",
            false,
            &std::collections::BTreeSet::new(),
            true,
            ResolvedLocale::En,
        );
        assert!(!catalog_en.is_empty());
        for entry in &catalog_en {
            assert!(
                !contains_cjk(&entry.capability.target)
                    && !contains_cjk(&entry.capability.unavailable_reason)
                    && !contains_cjk(&entry.capability.source)
                    && !contains_cjk(&entry.capability.result_semantics)
                    && entry
                        .capability
                        .prerequisites
                        .iter()
                        .all(|p| !contains_cjk(p)),
                "catalog capability contains CJK for '{}': {:?}",
                entry.action.id,
                entry.capability
            );
        }

        let mut draft = crate::draft::Draft::default();
        let ids = crate::preset::install_agent_starter_profiles(&mut draft).unwrap();
        assert!(!ids.is_empty());
        assert_binding_budgets(&crate::preset::canonical_encoder_hold_action());
        let first_pid = ids[0].clone();
        crate::preset::install_four_app_cockpit_preset(&mut draft, &first_pid).unwrap();
        for action in draft.document.editable.graph.actions.values() {
            assert_binding_budgets(action);
        }
    }

    #[test]
    fn canonical_names_translate_while_preserving_custom_renames() {
        assert_eq!(
            localize_profile_name("其他软件", ResolvedLocale::En),
            "Other Apps"
        );
        assert_eq!(
            localize_profile_name("其他软件 副本", ResolvedLocale::En),
            "Other Apps Copy"
        );
        assert_eq!(
            localize_profile_name("My Custom Rig", ResolvedLocale::En),
            "My Custom Rig"
        );
        assert_eq!(
            localize_layer_name("默认层", ResolvedLocale::En),
            "Default Layer"
        );
        assert_eq!(
            localize_layer_name("第 3 层", ResolvedLocale::En),
            "Layer 3"
        );
        assert_eq!(
            localize_layer_name("Custom Editing", ResolvedLocale::En),
            "Custom Editing"
        );
    }
}
