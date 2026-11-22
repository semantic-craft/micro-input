#[derive(Debug, Clone, Copy)]
pub struct BilingualLabelSet {
    pub key: &'static str,
    pub en: &'static [&'static str],
    pub zh: &'static [&'static str],
}

impl BilingualLabelSet {
    pub fn matches_exact(&self, label: &str) -> bool {
        let trimmed = label.trim();
        !trimmed.is_empty()
            && self
                .en
                .iter()
                .chain(self.zh.iter())
                .any(|candidate| trimmed == *candidate)
    }

    pub fn matches_ascii_case_insensitive(&self, label: &str) -> bool {
        let trimmed = label.trim();
        !trimmed.is_empty()
            && (self
                .en
                .iter()
                .any(|candidate| trimmed.eq_ignore_ascii_case(candidate))
                || self.zh.iter().any(|candidate| trimmed == *candidate))
    }

    pub fn matches_prefix(&self, label: &str) -> bool {
        let trimmed = label.trim();
        if trimmed.is_empty() {
            return false;
        }
        let lower = trimmed.to_ascii_lowercase();
        self.en
            .iter()
            .any(|candidate| lower.starts_with(&candidate.to_ascii_lowercase()))
            || self
                .zh
                .iter()
                .any(|candidate| trimmed.starts_with(candidate))
    }

    pub fn matches_contains(&self, label: &str) -> bool {
        let trimmed = label.trim();
        if trimmed.is_empty() {
            return false;
        }
        let lower = trimmed.to_ascii_lowercase();
        self.en
            .iter()
            .any(|candidate| lower.contains(&candidate.to_ascii_lowercase()))
            || self.zh.iter().any(|candidate| trimmed.contains(candidate))
    }
}

pub(crate) const SIDE_QUESTION_INPUT_SYNTHETIC_LABEL: &str = "Side Question Input";

pub const DICTATION_START: BilingualLabelSet = BilingualLabelSet {
    key: "dictation_start",
    en: &["Record Audio", "Record voice memo"],
    zh: &[
        "录制语音输入",
        "录制语音",
        "开始语音录制",
        "开始录音",
        "开始听写",
    ],
};

pub const DICTATION_STOP: BilingualLabelSet = BilingualLabelSet {
    key: "dictation_stop",
    en: &["Stop Recording", "Recording", "Stop recording"],
    zh: &[
        "停止录制",
        "停止语音录制",
        "停止录音",
        "停止听写",
        "正在录音",
        "正在录制",
    ],
};

pub const LIVE_VOICE_START: BilingualLabelSet = BilingualLabelSet {
    key: "live_voice_start",
    en: &["Start Voice mode", "Start Live Chat"],
    zh: &["开启语音模式", "启动语音模式", "开启实时语音"],
};

pub const LIVE_VOICE_END: BilingualLabelSet = BilingualLabelSet {
    key: "live_voice_end",
    en: &["End Voice mode", "End Live chat"],
    zh: &[
        "结束语音模式",
        "关闭语音模式",
        "退出语音模式",
        "结束实时语音",
    ],
};

pub const MODEL_PICKER_EXACT: BilingualLabelSet = BilingualLabelSet {
    key: "model_picker_exact",
    en: &["model"],
    zh: &["模型"],
};

pub const MODEL_PICKER_PREFIX: BilingualLabelSet = BilingualLabelSet {
    key: "model_picker_prefix",
    en: &["select model", "choose model", "switch model"],
    zh: &["模型，", "模型：", "选择模型", "切换模型"],
};

pub const MODEL_PICKER_EXCLUDED: BilingualLabelSet = BilingualLabelSet {
    key: "model_picker_excluded",
    en: &[
        "actions",
        "options",
        "menu",
        "switch mode",
        "agent permissions",
        "add context",
        "customize sections",
        "notifications",
        "extensions",
        "subagent",
    ],
    zh: &[
        "操作",
        "选项",
        "菜单",
        "权限",
        "添加上下文",
        "自定义显示模块",
        "系统通知",
        "扩展",
        "子代理",
        "子智能体",
    ],
};

pub const TOGGLE_SIDE_PANEL: BilingualLabelSet = BilingualLabelSet {
    key: "toggle_side_panel",
    en: &["Toggle Side Panel"],
    zh: &["切换侧边面板"],
};

pub const TOGGLE_SIDEBAR: BilingualLabelSet = BilingualLabelSet {
    key: "toggle_sidebar",
    en: &["Toggle sidebar", "Toggle Sidebar"],
    zh: &["切换侧边栏"],
};

pub const OPEN_SIDEBAR: BilingualLabelSet = BilingualLabelSet {
    key: "open_sidebar",
    en: &["Open sidebar", "Open Sidebar"],
    zh: &["打开侧边栏", "展开侧边栏"],
};

pub const NEW_TAB: BilingualLabelSet = BilingualLabelSet {
    key: "new_tab",
    en: &["New tab", "New Tab"],
    zh: &["新标签页"],
};

pub const SIDE_QUESTION: BilingualLabelSet = BilingualLabelSet {
    key: "side_question",
    en: &["Side Question"],
    zh: &["旁路提问"],
};

pub const NEW_SIDE_QUESTION: BilingualLabelSet = BilingualLabelSet {
    key: "new_side_question",
    en: &["New Side Question"],
    zh: &["新建支线提问", "新建旁路提问"],
};

pub const BROWSER: BilingualLabelSet = BilingualLabelSet {
    key: "browser",
    en: &["Browser"],
    zh: &["内置浏览器"],
};

pub const PREVIEW: BilingualLabelSet = BilingualLabelSet {
    key: "preview",
    en: &["Preview"],
    zh: &["预览"],
};

pub const OPEN_IN_EXTERNAL_BROWSER: BilingualLabelSet = BilingualLabelSet {
    key: "open_in_external_browser",
    en: &["Open in external browser"],
    zh: &["在外部浏览器中打开"],
};

pub const CLEAR_SEARCH: BilingualLabelSet = BilingualLabelSet {
    key: "clear_search",
    en: &["Clear search", "Clear Search"],
    zh: &["清除搜索"],
};

pub const ADDRESS_BAR: BilingualLabelSet = BilingualLabelSet {
    key: "address_bar",
    en: &["Address bar"],
    zh: &["地址栏"],
};

pub const SEARCH_FOR_CONTENT_TO_OPEN: BilingualLabelSet = BilingualLabelSet {
    key: "search_for_content_to_open",
    en: &["Search for content to open"],
    zh: &["搜索要打开的内容"],
};

pub const SEARCH_CONVERSATIONS: BilingualLabelSet = BilingualLabelSet {
    key: "search_conversations",
    en: &["Search Conversations"],
    zh: &["搜索会话"],
};

pub const NEW_CONVERSATION_IN_PROJECT: BilingualLabelSet = BilingualLabelSet {
    key: "new_conversation_in_project",
    en: &[
        "New Conversation in Project",
        "New Conversation in Workspace",
    ],
    zh: &["在项目中新建对话", "在工作区中新建对话"],
};

pub const NEW_CONVERSATION_CONTAINS: BilingualLabelSet = BilingualLabelSet {
    key: "new_conversation_contains",
    en: &["New Conversation"],
    zh: &["新建对话"],
};

pub const CONVERSATION_LOG_HEADING: BilingualLabelSet = BilingualLabelSet {
    key: "conversation_log_heading",
    en: &["Conversation Log"],
    zh: &["会话日志"],
};

pub const CHAT_TRANSCRIPT_HEADINGS: BilingualLabelSet = BilingualLabelSet {
    key: "chat_transcript_headings",
    en: &[
        "Conversation Log",
        "Agent said",
        "Agent working",
        "You said",
    ],
    zh: &["会话日志", "智能体回复", "智能体正在执行", "您说"],
};

pub const CHAT_TRANSCRIPT_REGIONS: BilingualLabelSet = BilingualLabelSet {
    key: "chat_transcript_regions",
    en: &["Conversation turns", "Agent response", "User message"],
    zh: &["对话轮次导航", "智能体回复", "用户消息"],
};

pub const SETTINGS_CONTAINER: BilingualLabelSet = BilingualLabelSet {
    key: "settings_container",
    en: &["Settings"],
    zh: &["系统设置", "设置"],
};

pub const MARKDOWN_VIEW_MODE_CONTAINER: BilingualLabelSet = BilingualLabelSet {
    key: "markdown_view_mode_container",
    en: &["Markdown view mode"],
    zh: &["Markdown 视图模式", "Markdown 预览模式"],
};

pub const SIDEBAR_SECTION_HEADERS: BilingualLabelSet = BilingualLabelSet {
    key: "sidebar_section_headers",
    en: &["Projects", "Pinned Conversations", "Automations"],
    zh: &["项目管理", "置顶对话", "自动化任务"],
};

pub const PROJECT_OPTIONS: BilingualLabelSet = BilingualLabelSet {
    key: "project_options",
    en: &["Project options"],
    zh: &["项目选项"],
};

pub const SEE_ALL_PREFIX: BilingualLabelSet = BilingualLabelSet {
    key: "see_all_prefix",
    en: &["See all ("],
    zh: &["查看全部 (", "查看全部(", "查看全部（"],
};

pub const SHOW_LESS: BilingualLabelSet = BilingualLabelSet {
    key: "show_less",
    en: &["Show less"],
    zh: &["收起"],
};

pub const STOP_EXECUTION: BilingualLabelSet = BilingualLabelSet {
    key: "stop_execution",
    en: &["Stop execution"],
    zh: &["停止执行"],
};

pub const ARCHIVE_CONVERSATION: BilingualLabelSet = BilingualLabelSet {
    key: "archive_conversation",
    en: &["Archive conversation"],
    zh: &["归档会话"],
};

pub const ROW_ACTIONS_OTHER: BilingualLabelSet = BilingualLabelSet {
    key: "row_actions_other",
    en: &[
        "Pin conversation",
        "Unpin conversation",
        "More options",
        "Conversation options",
    ],
    zh: &["置顶会话", "取消置顶会话", "更多选项", "会话选项"],
};

pub const UTILITY_SIDEBAR_OTHER: BilingualLabelSet = BilingualLabelSet {
    key: "utility_sidebar_other",
    en: &["Delete conversation", "Rename conversation"],
    zh: &["删除会话", "重命名对话"],
};

pub const SUBAGENT_OVERVIEW: BilingualLabelSet = BilingualLabelSet {
    key: "subagent_overview",
    en: &["subagents"],
    zh: &["子智能体", "子代理"],
};

pub const SUBAGENT_EXCLUDED: BilingualLabelSet = BilingualLabelSet {
    key: "subagent_excluded",
    en: &[
        "stop subagent",
        "cancel subagent",
        "open subagent:",
        "close ",
    ],
    zh: &[
        "打开子智能体",
        "停止子智能体",
        "停止子 Agent",
        "打开子代理",
        "停止子代理",
        "取消子代理",
    ],
};

pub const SUBAGENT_KEYWORD: BilingualLabelSet = BilingualLabelSet {
    key: "subagent_keyword",
    en: &["subagent"],
    zh: &["子智能体", "子代理", "子 Agent"],
};

pub const OPEN_SUBAGENT_PREFIX: BilingualLabelSet = BilingualLabelSet {
    key: "open_subagent_prefix",
    en: &["open subagent:"],
    zh: &["打开子智能体", "打开子代理"],
};

pub const SCROLL_TO_BOTTOM: BilingualLabelSet = BilingualLabelSet {
    key: "scroll_to_bottom",
    en: &["Jump to bottom", "Scroll to bottom"],
    zh: &["跳到底部", "滚动到底部"],
};

pub const SEND_MESSAGE: BilingualLabelSet = BilingualLabelSet {
    key: "send_message",
    en: &["Send message"],
    zh: &["发送消息", "傳送訊息"],
};

pub const SUBMIT_MESSAGE: BilingualLabelSet = BilingualLabelSet {
    key: "submit_message",
    en: &["Send", "Send message", "Submit"],
    zh: &["发送", "发送消息", "傳送訊息", "提交"],
};

pub const ALL_BILINGUAL_LABEL_SETS: &[&BilingualLabelSet] = &[
    &DICTATION_START,
    &DICTATION_STOP,
    &LIVE_VOICE_START,
    &LIVE_VOICE_END,
    &MODEL_PICKER_EXACT,
    &MODEL_PICKER_PREFIX,
    &MODEL_PICKER_EXCLUDED,
    &TOGGLE_SIDE_PANEL,
    &TOGGLE_SIDEBAR,
    &OPEN_SIDEBAR,
    &NEW_TAB,
    &SIDE_QUESTION,
    &NEW_SIDE_QUESTION,
    &BROWSER,
    &PREVIEW,
    &OPEN_IN_EXTERNAL_BROWSER,
    &CLEAR_SEARCH,
    &ADDRESS_BAR,
    &SEARCH_FOR_CONTENT_TO_OPEN,
    &SEARCH_CONVERSATIONS,
    &NEW_CONVERSATION_IN_PROJECT,
    &NEW_CONVERSATION_CONTAINS,
    &CONVERSATION_LOG_HEADING,
    &CHAT_TRANSCRIPT_HEADINGS,
    &CHAT_TRANSCRIPT_REGIONS,
    &SETTINGS_CONTAINER,
    &MARKDOWN_VIEW_MODE_CONTAINER,
    &SIDEBAR_SECTION_HEADERS,
    &PROJECT_OPTIONS,
    &SEE_ALL_PREFIX,
    &SHOW_LESS,
    &STOP_EXECUTION,
    &ARCHIVE_CONVERSATION,
    &ROW_ACTIONS_OTHER,
    &UTILITY_SIDEBAR_OTHER,
    &SUBAGENT_OVERVIEW,
    &SUBAGENT_EXCLUDED,
    &SUBAGENT_KEYWORD,
    &OPEN_SUBAGENT_PREFIX,
    &SCROLL_TO_BOTTOM,
    &SEND_MESSAGE,
    &SUBMIT_MESSAGE,
];

pub fn is_dictation_start(label: &str) -> bool {
    DICTATION_START.matches_exact(label)
}

pub fn is_dictation_stop(label: &str) -> bool {
    DICTATION_STOP.matches_exact(label)
}

pub fn is_live_voice_start(label: &str) -> bool {
    LIVE_VOICE_START.matches_ascii_case_insensitive(label)
}

pub fn is_live_voice_end(label: &str) -> bool {
    LIVE_VOICE_END.matches_ascii_case_insensitive(label)
}

pub fn is_model_picker_trigger(label: &str) -> bool {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    let excluded = MODEL_PICKER_EXCLUDED.en.iter().any(|candidate| {
        let lower_candidate = candidate.to_ascii_lowercase();
        if lower_candidate == "switch mode" {
            lower.contains("switch mode") && !lower.contains("switch model")
        } else {
            lower.contains(&lower_candidate)
        }
    }) || MODEL_PICKER_EXCLUDED
        .zh
        .iter()
        .any(|candidate| trimmed.contains(candidate));
    if excluded {
        return false;
    }
    MODEL_PICKER_EXACT.matches_ascii_case_insensitive(trimmed)
        || MODEL_PICKER_PREFIX.matches_prefix(trimmed)
}

pub fn is_toggle_side_panel(label: &str) -> bool {
    TOGGLE_SIDE_PANEL.matches_exact(label)
}

pub fn is_toggle_sidebar(label: &str) -> bool {
    TOGGLE_SIDEBAR.matches_ascii_case_insensitive(label)
}

pub fn is_open_sidebar(label: &str) -> bool {
    OPEN_SIDEBAR.matches_ascii_case_insensitive(label)
}

pub fn is_new_tab(label: &str) -> bool {
    NEW_TAB.matches_ascii_case_insensitive(label)
}

pub fn is_side_question(label: &str) -> bool {
    SIDE_QUESTION.matches_exact(label)
}

pub fn is_new_side_question(label: &str) -> bool {
    NEW_SIDE_QUESTION.matches_exact(label)
}

pub fn is_browser(label: &str) -> bool {
    BROWSER.matches_exact(label)
}

pub fn is_preview(label: &str) -> bool {
    PREVIEW.matches_exact(label)
}

pub fn is_open_in_external_browser(label: &str) -> bool {
    OPEN_IN_EXTERNAL_BROWSER.matches_exact(label)
}

pub fn is_clear_search(label: &str) -> bool {
    CLEAR_SEARCH.matches_ascii_case_insensitive(label)
}

pub fn is_address_bar(label: &str) -> bool {
    ADDRESS_BAR.matches_exact(label)
}

pub fn is_search_for_content_to_open(label: &str) -> bool {
    SEARCH_FOR_CONTENT_TO_OPEN.matches_exact(label)
}

pub fn is_search_conversations(label: &str) -> bool {
    SEARCH_CONVERSATIONS.matches_exact(label)
}

pub fn is_new_conversation_in_project(label: &str) -> bool {
    NEW_CONVERSATION_IN_PROJECT.matches_exact(label)
}

pub fn contains_new_conversation(label: &str) -> bool {
    NEW_CONVERSATION_CONTAINS.matches_contains(label)
}

pub fn is_conversation_log_heading(label: &str) -> bool {
    CONVERSATION_LOG_HEADING.matches_exact(label)
}

pub fn is_chat_transcript_heading(label: &str) -> bool {
    CHAT_TRANSCRIPT_HEADINGS.matches_exact(label)
}

pub fn is_chat_transcript_region(label: &str) -> bool {
    CHAT_TRANSCRIPT_REGIONS.matches_exact(label)
}

pub fn is_settings_container(label: &str) -> bool {
    SETTINGS_CONTAINER.matches_exact(label)
}

pub fn is_markdown_view_mode_container(label: &str) -> bool {
    MARKDOWN_VIEW_MODE_CONTAINER.matches_exact(label)
}

pub fn is_sidebar_section_header(label: &str) -> bool {
    SIDEBAR_SECTION_HEADERS.matches_exact(label)
}

pub fn is_project_options(label: &str) -> bool {
    PROJECT_OPTIONS.matches_exact(label)
}

pub fn is_see_all(label: &str) -> bool {
    SEE_ALL_PREFIX.matches_prefix(label)
}

pub fn is_show_less(label: &str) -> bool {
    SHOW_LESS.matches_exact(label)
}

pub fn is_stop_execution(label: &str) -> bool {
    STOP_EXECUTION.matches_exact(label)
}

pub fn is_archive_conversation(label: &str) -> bool {
    ARCHIVE_CONVERSATION.matches_exact(label)
}

pub fn is_row_action(label: &str) -> bool {
    is_stop_execution(label)
        || is_archive_conversation(label)
        || ROW_ACTIONS_OTHER.matches_exact(label)
}

pub fn is_sidebar_boundary(label: &str) -> bool {
    is_see_all(label)
        || is_show_less(label)
        || contains_new_conversation(label)
        || is_toggle_sidebar(label)
        || is_search_conversations(label)
        || is_sidebar_section_header(label)
        || is_project_options(label)
}

pub fn is_utility_sidebar_button(label: &str) -> bool {
    label.trim().is_empty()
        || is_sidebar_boundary(label)
        || is_row_action(label)
        || UTILITY_SIDEBAR_OTHER.matches_exact(label)
}

pub fn is_side_panel_button(label: &str) -> bool {
    is_toggle_side_panel(label)
        || is_new_tab(label)
        || is_side_question(label)
        || is_new_side_question(label)
        || is_browser(label)
        || is_preview(label)
        || is_open_in_external_browser(label)
        || is_clear_search(label)
}

pub fn is_browser_filter_button(label: &str) -> bool {
    is_browser(label)
        || is_preview(label)
        || is_open_in_external_browser(label)
        || is_clear_search(label)
}

pub fn is_subagent_popup_trigger(label: &str) -> bool {
    let trimmed = label.trim();
    if trimmed.is_empty() || SUBAGENT_EXCLUDED.matches_contains(trimmed) {
        return false;
    }
    SUBAGENT_KEYWORD.matches_contains(trimmed)
}

pub fn is_overview_subagents_button(label: &str) -> bool {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    SUBAGENT_OVERVIEW.en.iter().any(|candidate| {
        let lower_candidate = candidate.to_ascii_lowercase();
        lower == lower_candidate
            || lower
                .strip_prefix(&lower_candidate)
                .is_some_and(|rest| rest.starts_with(' '))
    }) || SUBAGENT_OVERVIEW.zh.iter().any(|candidate| {
        trimmed == *candidate
            || trimmed.strip_prefix(candidate).is_some_and(|rest| {
                rest.starts_with(' ') || rest.starts_with('(') || rest.starts_with('（')
            })
    })
}

pub fn is_open_subagent_button(label: &str) -> bool {
    OPEN_SUBAGENT_PREFIX.matches_prefix(label)
}

pub fn is_scroll_to_bottom(label: &str) -> bool {
    SCROLL_TO_BOTTOM.matches_ascii_case_insensitive(label)
}

pub fn is_send_message(label: &str) -> bool {
    SEND_MESSAGE.matches_exact(label)
}

pub fn is_submit_message(label: &str) -> bool {
    SUBMIT_MESSAGE.matches_ascii_case_insensitive(label)
}

pub fn is_scroll_or_send_button(label: &str) -> bool {
    is_scroll_to_bottom(label) || is_submit_message(label)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn all_bilingual_label_sets_have_non_empty_en_and_zh_and_unique_keys() {
        assert!(!ALL_BILINGUAL_LABEL_SETS.is_empty());
        let mut keys = HashSet::new();
        for set in ALL_BILINGUAL_LABEL_SETS {
            assert!(
                !set.key.trim().is_empty(),
                "BilingualLabelSet key must be non-empty"
            );
            assert!(
                keys.insert(set.key),
                "duplicate BilingualLabelSet key: {}",
                set.key
            );
            assert!(
                !set.en.is_empty(),
                "BilingualLabelSet '{}' must declare non-empty en labels",
                set.key
            );
            assert!(
                !set.zh.is_empty(),
                "BilingualLabelSet '{}' must declare non-empty zh labels",
                set.key
            );
            for label in set.en.iter().chain(set.zh.iter()) {
                assert!(
                    !label.trim().is_empty(),
                    "BilingualLabelSet '{}' must not contain blank labels",
                    set.key
                );
            }
        }
    }

    #[test]
    fn matches_en_and_zh_dictation_and_live_voice() {
        assert!(is_dictation_start("Record Audio"));
        assert!(is_dictation_start(" Record voice memo "));
        assert!(is_dictation_start("录制语音输入"));
        assert!(is_dictation_start("开始听写"));
        assert!(!is_dictation_start("Stop Recording"));
        assert!(!is_dictation_start("停止录制"));

        assert!(is_dictation_stop("Stop Recording"));
        assert!(is_dictation_stop("Recording"));
        assert!(is_dictation_stop("停止录制"));
        assert!(is_dictation_stop("正在录音"));
        assert!(!is_dictation_stop("Record Audio"));

        assert!(is_live_voice_start("Start Voice mode"));
        assert!(is_live_voice_start("start live chat"));
        assert!(is_live_voice_start("开启语音模式"));
        assert!(is_live_voice_start("开启实时语音"));
        assert!(!is_live_voice_start("结束语音模式"));

        assert!(is_live_voice_end("End Voice mode"));
        assert!(is_live_voice_end("end live chat"));
        assert!(is_live_voice_end("结束语音模式"));
        assert!(is_live_voice_end("退出语音模式"));
        assert!(!is_live_voice_end("开启语音模式"));
    }

    #[test]
    fn matches_en_and_zh_model_picker_and_excludes_permissions() {
        assert!(is_model_picker_trigger("Model"));
        assert!(is_model_picker_trigger("Select model"));
        assert!(is_model_picker_trigger("Switch model: Gemini"));
        assert!(is_model_picker_trigger("模型"));
        assert!(is_model_picker_trigger("模型，当前：Gemini 4 Neon High"));
        assert!(is_model_picker_trigger("模型：Gemini 4"));
        assert!(is_model_picker_trigger("选择模型"));
        assert!(is_model_picker_trigger("切换模型"));

        assert!(!is_model_picker_trigger("Switch mode"));
        assert!(!is_model_picker_trigger(
            "权限模式，当前：全自动放行 (Turbo)"
        ));
        assert!(!is_model_picker_trigger("Agent permissions"));
        assert!(!is_model_picker_trigger("模型选项"));
        assert!(!is_model_picker_trigger("子智能体模型"));
        assert!(!is_model_picker_trigger(""));
    }

    #[test]
    fn matches_en_and_zh_side_panel_browser_sidebar_and_subagent() {
        assert!(is_toggle_side_panel("Toggle Side Panel"));
        assert!(is_toggle_side_panel("切换侧边面板"));
        assert!(is_side_panel_button("切换侧边面板"));
        assert!(is_side_panel_button("新标签页"));
        assert!(is_side_panel_button("旁路提问"));
        assert!(is_side_panel_button("新建支线提问"));
        assert!(is_side_panel_button("内置浏览器"));
        assert!(is_side_panel_button("预览"));
        assert!(is_side_panel_button("Open in external browser"));
        assert!(!is_open_in_external_browser("Open in External Browser"));
        assert!(is_side_panel_button("在外部浏览器中打开"));
        assert!(is_side_panel_button("清除搜索"));

        assert!(is_address_bar("Address bar"));
        assert!(is_address_bar("地址栏"));
        assert!(is_search_for_content_to_open("Search for content to open"));
        assert!(is_search_for_content_to_open("搜索要打开的内容"));

        assert!(is_toggle_sidebar("Toggle sidebar"));
        assert!(is_toggle_sidebar("Toggle Sidebar"));
        assert!(is_toggle_sidebar("切换侧边栏"));
        assert!(is_open_sidebar("Open sidebar"));
        assert!(is_open_sidebar("Open Sidebar"));
        assert!(is_open_sidebar("打开侧边栏"));
        assert!(is_open_sidebar("展开侧边栏"));

        assert!(is_sidebar_boundary("查看全部 (8)"));
        assert!(is_sidebar_boundary("See all (8)"));
        assert!(is_sidebar_boundary("收起"));
        assert!(is_sidebar_boundary("在项目中新建对话"));
        assert!(is_sidebar_boundary("项目管理"));
        assert!(is_sidebar_boundary("项目选项"));
        assert!(is_row_action("停止执行"));
        assert!(is_row_action("归档会话"));
        assert!(is_row_action("置顶会话"));
        assert!(is_utility_sidebar_button("删除会话"));
        assert!(is_utility_sidebar_button("重命名对话"));

        assert!(is_subagent_popup_trigger("Subagent"));
        assert!(is_subagent_popup_trigger("子智能体"));
        assert!(is_subagent_popup_trigger("子代理"));
        assert!(!is_subagent_popup_trigger("Stop subagent"));
        assert!(!is_subagent_popup_trigger("Open subagent: Research"));
        assert!(!is_subagent_popup_trigger("打开子智能体：Research"));
        assert!(!is_subagent_popup_trigger("停止子智能体"));
        assert!(!is_subagent_popup_trigger("停止子 Agent"));
        assert!(!is_subagent_popup_trigger("取消子代理"));

        assert!(is_overview_subagents_button("Subagents"));
        assert!(is_overview_subagents_button("Subagents 2"));
        assert!(is_overview_subagents_button("子智能体"));
        assert!(is_overview_subagents_button("子智能体 2"));
        assert!(is_overview_subagents_button("子代理"));
        assert!(is_open_subagent_button("Open subagent: Planner"));
        assert!(is_open_subagent_button("打开子智能体：Planner"));
        assert!(is_open_subagent_button("打开子代理：Planner"));
    }

    #[test]
    fn matches_scroll_to_bottom_and_submit_message_case_insensitively_and_bilingually() {
        for label in [
            "Jump to bottom",
            "Scroll to bottom",
            "Scroll to Bottom",
            "SCROLL TO BOTTOM",
            "跳到底部",
            "滚动到底部",
        ] {
            assert!(is_scroll_to_bottom(label), "expected scroll match: {label}");
            assert!(is_scroll_or_send_button(label));
            assert!(!is_submit_message(label));
        }

        for label in [
            "Send",
            "Send message",
            "send message",
            "Submit",
            "发送",
            "发送消息",
            "傳送訊息",
            "提交",
        ] {
            assert!(is_submit_message(label), "expected submit match: {label}");
            assert!(is_scroll_or_send_button(label));
            assert!(!is_scroll_to_bottom(label));
        }
    }
}
