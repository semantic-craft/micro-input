use crate::{
    canvas::{
        ControlSurfaceId, JoystickDirection8, KnobJoystickPreset, ProtectedLayerLockProjection,
    },
    editors::EditorAction,
};
use mi_model::{action_library::ActionCapability, draft::Action, host_router::BottomRowLayout};
use serde::{Deserialize, Serialize};

pub const INSPECTOR_WIDTH: f32 = 332.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KeycapRole {
    TaskKey,
    ActionKey,
}

impl KeycapRole {
    pub const fn label(self) -> &'static str {
        match self {
            Self::TaskKey => "会话键",
            Self::ActionKey => "动作键",
        }
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CodexKeycapId {
    Blank = 0,
    Fast = 1,
    Approve = 2,
    Decline = 3,
    Split = 4,
    Mic = 5,
    Codex = 6,
    Bug = 7,
    OpenAi = 8,
    Term = 9,
    Download = 10,
    Delete = 11,
    New = 12,
    Nav = 13,
    Magic = 14,
    Diff = 15,
    Play = 16,
    Git = 17,
    BranchAlt = 18,
    Branch = 19,
    Merge = 20,
    Pr = 21,
    Paint = 22,
    Lab = 23,
    Party = 24,
    Time = 25,
    MindPlus = 26,
    MindMinus = 27,
    Setup = 28,
    Folder = 29,
    Upload = 30,
    Apps = 31,
    Yolo = 32,
    Yeet = 33,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeycapDescriptor {
    pub id: CodexKeycapId,
    pub silk_code: &'static str,
    pub short_label: &'static str,
    pub chinese_hint: &'static str,
    pub glyph_symbol: &'static str,
    pub search_keywords: &'static [&'static str],
    pub default_action_id: Option<&'static str>,
}

const KEYCAP_DESCRIPTORS: [KeycapDescriptor; 34] = [
    KeycapDescriptor {
        id: CodexKeycapId::Blank,
        silk_code: "BLANK",
        short_label: "Blank",
        chinese_hint: "无丝印空白键",
        glyph_symbol: "□",
        search_keywords: &["blank", "empty", "空白", "无"],
        default_action_id: None,
    },
    KeycapDescriptor {
        id: CodexKeycapId::Fast,
        silk_code: "FAST",
        short_label: "Fast",
        chinese_hint: "极速执行 / 快速跳转",
        glyph_symbol: "⚡",
        search_keywords: &["fast", "bolt", "lightning", "极速", "闪电", "快"],
        default_action_id: Some("ag-cycle-completed-next"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::Approve,
        silk_code: "APPROVE",
        short_label: "Approve",
        chinese_hint: "批准 / 同意",
        glyph_symbol: "✓",
        search_keywords: &["approve", "accept", "yes", "check", "批准", "同意", "确认"],
        default_action_id: Some("agent-approve"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::Decline,
        silk_code: "DECLINE",
        short_label: "Decline",
        chinese_hint: "拒绝 / 驳回",
        glyph_symbol: "✕",
        search_keywords: &["decline", "reject", "no", "cancel", "拒绝", "驳回", "取消"],
        default_action_id: Some("agent-deny"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::Split,
        silk_code: "SPLIT",
        short_label: "Split",
        chinese_hint: "分屏 / 对比视图",
        glyph_symbol: "◫",
        search_keywords: &["split", "pane", "分屏", "并排"],
        default_action_id: Some("ag-split-right"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::Mic,
        silk_code: "MIC",
        short_label: "Mic",
        chinese_hint: "语音听写 / 按住说话",
        glyph_symbol: "🎤",
        search_keywords: &[
            "mic",
            "voice",
            "ptt",
            "dictation",
            "fn",
            "语音",
            "麦克风",
            "说话",
            "听写",
        ],
        default_action_id: Some("apple-fn-key"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::Codex,
        silk_code: "CODEX",
        short_label: "Codex",
        chinese_hint: "唤醒 Codex 助手",
        glyph_symbol: "◈",
        search_keywords: &["codex", "agent", "ai", "助手", "智能体"],
        default_action_id: Some("four-app-attention"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::Bug,
        silk_code: "BUG",
        short_label: "Bug",
        chinese_hint: "诊断报错 / 修复缺陷",
        glyph_symbol: "🐞",
        search_keywords: &["bug", "fix", "debug", "error", "修复", "调试", "报错"],
        default_action_id: Some("prompt-test"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::OpenAi,
        silk_code: "OPENAI",
        short_label: "OpenAI",
        chinese_hint: "切换或聚焦 AI 会话",
        glyph_symbol: "◎",
        search_keywords: &["openai", "chatgpt", "ai", "待处理", "模型"],
        default_action_id: Some("four-app-attention"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::Term,
        silk_code: "TERM",
        short_label: "Term",
        chinese_hint: "切换终端面板",
        glyph_symbol: "⌘",
        search_keywords: &["term", "terminal", "shell", "console", "终端", "命令行"],
        default_action_id: Some("pane"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::Download,
        silk_code: "DOWNLOAD",
        short_label: "Pull",
        chinese_hint: "拉取更新 / 保存",
        glyph_symbol: "↓",
        search_keywords: &["download", "pull", "save", "下载", "拉取", "保存"],
        default_action_id: Some("save"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::Delete,
        silk_code: "DELETE",
        short_label: "Stop",
        chinese_hint: "停止生成 / 清空",
        glyph_symbol: "⌫",
        search_keywords: &["delete", "stop", "clear", "trash", "删除", "停止", "中断"],
        default_action_id: Some("agent-stop"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::New,
        silk_code: "NEW",
        short_label: "New",
        chinese_hint: "新建对话 / 新建任务",
        glyph_symbol: "＋",
        search_keywords: &["new", "plus", "add", "create", "新建", "新对话", "添加"],
        default_action_id: Some("agent-new-or-stop"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::Nav,
        silk_code: "NAV",
        short_label: "Nav",
        chinese_hint: "全局搜索与导航",
        glyph_symbol: "🧭",
        search_keywords: &["nav", "search", "find", "goto", "导航", "搜索", "查找"],
        default_action_id: Some("search"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::Magic,
        silk_code: "MAGIC",
        short_label: "Magic",
        chinese_hint: "自动润色 / 智能重构",
        glyph_symbol: "✦",
        search_keywords: &[
            "magic", "sparkle", "wand", "refactor", "魔法", "重构", "优化",
        ],
        default_action_id: Some("prompt-continue"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::Diff,
        silk_code: "DIFF",
        short_label: "Diff",
        chinese_hint: "查看代码改动与审阅",
        glyph_symbol: "±",
        search_keywords: &["diff", "review", "changes", "差异", "改动", "审阅"],
        default_action_id: Some("prompt-review"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::Play,
        silk_code: "PLAY",
        short_label: "Run",
        chinese_hint: "运行 / 发送执行",
        glyph_symbol: "▶",
        search_keywords: &["play", "run", "submit", "send", "运行", "发送", "执行"],
        default_action_id: Some("agent-submit"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::Git,
        silk_code: "GIT",
        short_label: "Git",
        chinese_hint: "版本控制状态",
        glyph_symbol: "⌥",
        search_keywords: &["git", "vcs", "repo", "版本", "仓库"],
        default_action_id: Some("prompt-summary"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::BranchAlt,
        silk_code: "BRANCH_ALT",
        short_label: "Fork",
        chinese_hint: "派生分支 / 备选路线",
        glyph_symbol: "⑂",
        search_keywords: &["branch_alt", "fork", "派生", "分叉"],
        default_action_id: Some("ag-prev-conversation"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::Branch,
        silk_code: "BRANCH",
        short_label: "Branch",
        chinese_hint: "切换工作分支",
        glyph_symbol: "⑃",
        search_keywords: &["branch", "switch", "分支", "切分支"],
        default_action_id: Some("ag-next-conversation"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::Merge,
        silk_code: "MERGE",
        short_label: "Merge",
        chinese_hint: "合并改动",
        glyph_symbol: "⑄",
        search_keywords: &["merge", "join", "合并", "合入"],
        default_action_id: Some("agent-approve"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::Pr,
        silk_code: "PR",
        short_label: "PR",
        chinese_hint: "提交 Pull Request",
        glyph_symbol: "⇄",
        search_keywords: &["pr", "pull request", "review", "合并请求", "提交"],
        default_action_id: Some("prompt-commit"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::Paint,
        silk_code: "PAINT",
        short_label: "Theme",
        chinese_hint: "界面与视觉微调",
        glyph_symbol: "🎨",
        search_keywords: &["paint", "color", "theme", "design", "配色", "主题", "设计"],
        default_action_id: Some("review"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::Lab,
        silk_code: "LAB",
        short_label: "Test",
        chinese_hint: "运行实验与单元验证",
        glyph_symbol: "🧪",
        search_keywords: &["lab", "test", "verify", "check", "实验", "测试", "验证"],
        default_action_id: Some("prompt-test"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::Party,
        silk_code: "PARTY",
        short_label: "Ship",
        chinese_hint: "确认完成并庆祝交付",
        glyph_symbol: "🎉",
        search_keywords: &["party", "celebrate", "done", "ship", "完成", "庆祝", "交付"],
        default_action_id: Some("agent-submit"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::Time,
        silk_code: "TIME",
        short_label: "History",
        chinese_hint: "回溯最近会话历史",
        glyph_symbol: "⏱",
        search_keywords: &["time", "history", "recent", "back", "历史", "回退", "时间"],
        default_action_id: Some("navigate-back"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::MindPlus,
        silk_code: "MIND_PLUS",
        short_label: "Deep+",
        chinese_hint: "加深推理 / 下一个已完成",
        glyph_symbol: "⊕",
        search_keywords: &[
            "mind_plus",
            "think",
            "deep",
            "next",
            "加深",
            "推理",
            "下一个",
        ],
        default_action_id: Some("ag-cycle-completed-next"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::MindMinus,
        silk_code: "MIND_MINUS",
        short_label: "Fast-",
        chinese_hint: "简化输出 / 上一个已完成",
        glyph_symbol: "⊖",
        search_keywords: &["mind_minus", "concise", "prev", "简化", "精简", "上一个"],
        default_action_id: Some("ag-cycle-completed-prev"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::Setup,
        silk_code: "SETUP",
        short_label: "Setup",
        chinese_hint: "打开设置与命令面板",
        glyph_symbol: "⚙",
        search_keywords: &["setup", "settings", "config", "gear", "设置", "配置"],
        default_action_id: Some("search"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::Folder,
        silk_code: "FOLDER",
        short_label: "Project",
        chinese_hint: "切换工作区项目",
        glyph_symbol: "📁",
        search_keywords: &[
            "folder",
            "project",
            "workspace",
            "dir",
            "文件夹",
            "项目",
            "工作区",
        ],
        default_action_id: Some("ag-next-project"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::Upload,
        silk_code: "UPLOAD",
        short_label: "Push",
        chinese_hint: "推送远端 / 上传上下文",
        glyph_symbol: "↑",
        search_keywords: &["upload", "push", "export", "上传", "推送", "导出"],
        default_action_id: Some("agent-prompt-submit"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::Apps,
        silk_code: "APPS",
        short_label: "Apps",
        chinese_hint: "四应用总览与切换",
        glyph_symbol: "⊞",
        search_keywords: &[
            "apps",
            "grid",
            "overview",
            "switch",
            "应用",
            "总览",
            "四应用",
        ],
        default_action_id: Some("four-app-attention"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::Yolo,
        silk_code: "YOLO",
        short_label: "YOLO",
        chinese_hint: "全自动放行执行",
        glyph_symbol: "🚀",
        search_keywords: &[
            "yolo",
            "auto",
            "rocket",
            "autonomous",
            "自动",
            "全自动",
            "火箭",
        ],
        default_action_id: Some("agent-approve"),
    },
    KeycapDescriptor {
        id: CodexKeycapId::Yeet,
        silk_code: "YEET",
        short_label: "Yeet",
        chinese_hint: "一键丢弃改动 / 强力撤销",
        glyph_symbol: "💨",
        search_keywords: &["yeet", "discard", "revert", "undo", "丢弃", "撤销", "重来"],
        default_action_id: Some("agent-stop"),
    },
];

impl CodexKeycapId {
    pub const ALL_34: [Self; 34] = [
        Self::Blank,
        Self::Fast,
        Self::Approve,
        Self::Decline,
        Self::Split,
        Self::Mic,
        Self::Codex,
        Self::Bug,
        Self::OpenAi,
        Self::Term,
        Self::Download,
        Self::Delete,
        Self::New,
        Self::Nav,
        Self::Magic,
        Self::Diff,
        Self::Play,
        Self::Git,
        Self::BranchAlt,
        Self::Branch,
        Self::Merge,
        Self::Pr,
        Self::Paint,
        Self::Lab,
        Self::Party,
        Self::Time,
        Self::MindPlus,
        Self::MindMinus,
        Self::Setup,
        Self::Folder,
        Self::Upload,
        Self::Apps,
        Self::Yolo,
        Self::Yeet,
    ];

    pub const fn descriptor(self) -> KeycapDescriptor {
        KEYCAP_DESCRIPTORS[self as usize]
    }

    pub const fn silk_code(self) -> &'static str {
        self.descriptor().silk_code
    }

    pub const fn short_label(self) -> &'static str {
        self.descriptor().short_label
    }

    pub const fn chinese_hint(self) -> &'static str {
        self.descriptor().chinese_hint
    }

    pub const fn glyph_symbol(self) -> &'static str {
        self.descriptor().glyph_symbol
    }

    pub const fn default_action_id(self) -> Option<&'static str> {
        self.descriptor().default_action_id
    }

    pub fn matches_query(self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return true;
        }
        let d = self.descriptor();
        d.silk_code.to_lowercase().contains(&q)
            || d.short_label.to_lowercase().contains(&q)
            || d.chinese_hint.to_lowercase().contains(&q)
            || d.glyph_symbol.to_lowercase().contains(&q)
            || d.search_keywords
                .iter()
                .any(|kw| kw.to_lowercase().contains(&q))
    }

    pub fn default_action(self) -> Option<Action> {
        let target_id = self.default_action_id()?;
        mi_model::draft::actions()
            .into_iter()
            .find(|a| a.id == target_id)
    }

    pub fn infer_for_slot(slot: u32, action: Option<&Action>) -> Self {
        if let Some(a) = action {
            if a.id == "apple-fn-key"
                || a.id.contains("ptt")
                || a.id.contains("dictation")
                || a.id == "ag-live-voice"
                || a.payload.get("command").and_then(|v| v.as_str()) == Some("push-to-talk")
            {
                return Self::Mic;
            }
            if a.id.contains("slot-open") || a.id.contains("cockpit-slot") {
                return Self::Apps;
            }
            if a.id.contains("approve") {
                return Self::Approve;
            }
            if a.id.contains("deny") || a.id.contains("reject") {
                return Self::Decline;
            }
            if a.id.contains("submit") {
                return Self::Play;
            }
            if a.id.contains("stop") {
                return Self::Delete;
            }
            if a.id.contains("new") {
                return Self::New;
            }
            if a.id.contains("split") {
                return Self::Split;
            }
            if a.id.contains("project") {
                return Self::Folder;
            }
            if a.id.contains("search") {
                return Self::Nav;
            }
            if a.id.contains("save") {
                return Self::Download;
            }
            if a.id.contains("review") {
                return Self::Diff;
            }
            if a.id.contains("test") {
                return Self::Lab;
            }
            if a.id.contains("commit") {
                return Self::Pr;
            }
            if a.id.contains("back") {
                return Self::Time;
            }
            if a.id.contains("completed-next") {
                return Self::MindPlus;
            }
            if a.id.contains("completed-prev") {
                return Self::MindMinus;
            }
            if let Some(found) = Self::ALL_34
                .into_iter()
                .find(|k| k.default_action_id() == Some(a.id.as_str()))
            {
                return found;
            }
        }
        match slot {
            0..=5 => Self::Apps,
            10 | 11 => Self::Mic,
            12 => Self::Play,
            _ => Self::Term,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeycapCatalogItemProjection {
    pub id: CodexKeycapId,
    pub silk_code: String,
    pub short_label: String,
    pub chinese_hint: String,
    pub glyph_symbol: String,
    pub default_action_id: Option<String>,
    pub is_selected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ShortcutModifier {
    Command,
    Shift,
    Option,
    Control,
}

impl ShortcutModifier {
    pub const ALL: [Self; 4] = [Self::Command, Self::Shift, Self::Option, Self::Control];

    pub const fn as_draft_str(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::Shift => "shift",
            Self::Option => "option",
            Self::Control => "control",
        }
    }

    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Command => "⌘",
            Self::Shift => "⇧",
            Self::Option => "⌥",
            Self::Control => "⌃",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ShortcutRecorderState {
    pub is_recording: bool,
    pub key: String,
    pub modifiers: Vec<ShortcutModifier>,
    pub target: String,
    pub preview_badge: String,
    pub validation_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum InspectorDrawer {
    KeycapCatalog,
    MoreActions,
    MacroLibrary,
    LayerAndLighting,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionLibraryRowProjection {
    pub action_id: String,
    pub name: String,
    pub category: String,
    pub detail: String,
    pub target_summary: String,
    pub execution_available: bool,
    pub unavailable_reason: String,
    pub is_favorite: bool,
    pub is_currently_bound: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MacroLibraryRowProjection {
    pub id: String,
    pub name: String,
    pub is_multi: bool,
    pub step_or_branch_summary: String,
    pub reference_count: usize,
    pub reference_summary: String,
    pub is_currently_bound: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditableSlotInspectorProjection {
    pub selected_surface: ControlSurfaceId,
    pub selected_logical_slot: u32,
    pub selected_binding_id: String,
    pub surface_display_name: String,
    pub joystick_direction: Option<JoystickDirection8>,
    pub bottom_row_layout: BottomRowLayout,
    pub supports_task_role: bool,
    pub slot_role: KeycapRole,
    pub bound_action_id: Option<String>,
    pub bound_action_name: String,
    pub bound_action_category: String,
    pub bound_action_detail: String,
    pub bound_capability: Option<ActionCapability>,
    pub is_apple_fn_bound: bool,
    pub selected_keycap: CodexKeycapId,
    pub keycap_search_query: String,
    pub filtered_keycaps: Vec<KeycapCatalogItemProjection>,
    pub bind_keycap_default_action_on_select: bool,
    pub shortcut_recorder: ShortcutRecorderState,
    pub action_search_query: String,
    pub action_category_filter: String,
    pub available_categories: Vec<String>,
    pub favorites_only: bool,
    pub filtered_actions: Vec<ActionLibraryRowProjection>,
    pub macros: Vec<MacroLibraryRowProjection>,
    pub multis: Vec<MacroLibraryRowProjection>,
    pub open_drawers: Vec<InspectorDrawer>,
    pub can_add_lighting: bool,
    pub has_layer_lighting: bool,
    pub command_execution_enabled: bool,
}

impl PartialEq for EditableSlotInspectorProjection {
    fn eq(&self, other: &Self) -> bool {
        let cap_eq = match (&self.bound_capability, &other.bound_capability) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                a.target == b.target
                    && a.prerequisites == b.prerequisites
                    && a.host_required == b.host_required
                    && a.execution_available == b.execution_available
                    && a.unavailable_reason == b.unavailable_reason
                    && a.source == b.source
                    && a.result_semantics == b.result_semantics
            }
            _ => false,
        };
        cap_eq
            && self.selected_surface == other.selected_surface
            && self.selected_logical_slot == other.selected_logical_slot
            && self.selected_binding_id == other.selected_binding_id
            && self.surface_display_name == other.surface_display_name
            && self.joystick_direction == other.joystick_direction
            && self.bottom_row_layout == other.bottom_row_layout
            && self.supports_task_role == other.supports_task_role
            && self.slot_role == other.slot_role
            && self.bound_action_id == other.bound_action_id
            && self.bound_action_name == other.bound_action_name
            && self.bound_action_category == other.bound_action_category
            && self.bound_action_detail == other.bound_action_detail
            && self.is_apple_fn_bound == other.is_apple_fn_bound
            && self.selected_keycap == other.selected_keycap
            && self.keycap_search_query == other.keycap_search_query
            && self.filtered_keycaps == other.filtered_keycaps
            && self.bind_keycap_default_action_on_select
                == other.bind_keycap_default_action_on_select
            && self.shortcut_recorder == other.shortcut_recorder
            && self.action_search_query == other.action_search_query
            && self.action_category_filter == other.action_category_filter
            && self.available_categories == other.available_categories
            && self.favorites_only == other.favorites_only
            && self.filtered_actions == other.filtered_actions
            && self.macros == other.macros
            && self.multis == other.multis
            && self.open_drawers == other.open_drawers
            && self.can_add_lighting == other.can_add_lighting
            && self.has_layer_lighting == other.has_layer_lighting
            && self.command_execution_enabled == other.command_execution_enabled
    }
}

impl Default for EditableSlotInspectorProjection {
    fn default() -> Self {
        Self {
            selected_surface: ControlSurfaceId::Key0,
            selected_logical_slot: 0,
            selected_binding_id: "AG00".to_string(),
            surface_display_name: "按键 AG00".to_string(),
            joystick_direction: None,
            bottom_row_layout: BottomRowLayout::Split,
            supports_task_role: true,
            slot_role: KeycapRole::TaskKey,
            bound_action_id: None,
            bound_action_name: "未绑定".to_string(),
            bound_action_category: String::new(),
            bound_action_detail: String::new(),
            bound_capability: None,
            is_apple_fn_bound: false,
            selected_keycap: CodexKeycapId::Blank,
            keycap_search_query: String::new(),
            filtered_keycaps: Vec::new(),
            bind_keycap_default_action_on_select: true,
            shortcut_recorder: ShortcutRecorderState::default(),
            action_search_query: String::new(),
            action_category_filter: "全部".to_string(),
            available_categories: Vec::new(),
            favorites_only: false,
            filtered_actions: Vec::new(),
            macros: Vec::new(),
            multis: Vec::new(),
            open_drawers: Vec::new(),
            can_add_lighting: false,
            has_layer_lighting: false,
            command_execution_enabled: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum InspectorBodyProjection {
    ProtectedLocked(ProtectedLayerLockProjection),
    Editable(Box<EditableSlotInspectorProjection>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InspectorProjection {
    pub width_px: f32,
    pub profile_id: Option<String>,
    pub profile_display_name: String,
    pub layer_id: Option<String>,
    pub layer_name: String,
    pub is_protected_layer: bool,
    pub can_edit_draft: bool,
    pub body: InspectorBodyProjection,
}

impl Default for InspectorProjection {
    fn default() -> Self {
        Self {
            width_px: INSPECTOR_WIDTH,
            profile_id: None,
            profile_display_name: String::new(),
            layer_id: None,
            layer_name: String::new(),
            is_protected_layer: false,
            can_edit_draft: true,
            body: InspectorBodyProjection::Editable(Box::default()),
        }
    }
}

impl InspectorProjection {
    pub fn editable(&self) -> Option<&EditableSlotInspectorProjection> {
        match &self.body {
            InspectorBodyProjection::Editable(inner) => Some(inner),
            InspectorBodyProjection::ProtectedLocked(_) => None,
        }
    }

    pub fn protected_lock(&self) -> Option<&ProtectedLayerLockProjection> {
        match &self.body {
            InspectorBodyProjection::ProtectedLocked(lock) => Some(lock),
            InspectorBodyProjection::Editable(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum InspectorAction {
    UnlockProtectedLayer,
    SelectSlot(u32),
    SelectJoystickDirection(JoystickDirection8),
    SetSlotRole(KeycapRole),
    ApplyKnobJoystickPreset(KnobJoystickPreset),
    SetBottomRowLayout(BottomRowLayout),
    MergeBottomRowAcrossLayers,
    UnassignSelectedSlot,
    SetKeycapSearchQuery(String),
    SetBindKeycapDefaultAction(bool),
    ApplyKeycapIcon {
        keycap: CodexKeycapId,
        bind_default_action: bool,
    },
    BindAppleFnKey,
    StartShortcutRecording,
    CancelShortcutRecording,
    CaptureShortcutChord {
        key: String,
        modifiers: Vec<ShortcutModifier>,
    },
    SetShortcutTarget(String),
    CommitRecordedShortcut,
    AssignCatalogAction(String),
    AssignCustomText {
        name: String,
        text: String,
        submit_on_paste: bool,
    },
    BindExistingAction(String),
    ToggleActionFavorite {
        action_id: String,
        favorite: bool,
    },
    ToggleDrawer(InspectorDrawer),
    SetActionSearchQuery(String),
    SetActionCategoryFilter(String),
    SetFavoritesOnly(bool),
    SetCommandExecutionEnabled(bool),
    CreateCustomLayer {
        name: String,
    },
    RenameSelectedLayer {
        name: String,
    },
    DuplicateSelectedLayer {
        name: String,
    },
    DeleteSelectedLayer,
    OpenEditor(EditorAction),
}
