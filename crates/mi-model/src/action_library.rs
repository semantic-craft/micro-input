//! Local action intents and honest prerequisites. No adapter discovery or execution.
use crate::draft::{actions, Action, ActionKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionCapability {
    pub target: String,
    pub prerequisites: Vec<String>,
    pub host_required: bool,
    pub execution_available: bool,
    pub unavailable_reason: String,
    pub source: String,
    pub result_semantics: String,
}

pub fn capability(action: &Action, command_execution_enabled: bool) -> ActionCapability {
    let mut prerequisites = vec!["可分配到本地逻辑槽位；宿主执行基于 0.6.3 物理映射验证".into()];
    let (target, host_required, execution_available, reason, source, result_semantics) =
        match serde_json::from_value::<ActionKind>(action.payload.clone()) {
            Ok(ActionKind::DeviceKey { token }) if token.starts_with("MS_") => {
                prerequisites.push(
                    "需固件 0.7.0-rc.16+ 并应用到 /fs/keymap.json；写入后跨 USB/蓝牙主机脱机直接生效"
                        .into(),
                );
                (
                    format!("硬件 HID 鼠标 · {token}"),
                    false,
                    false,
                    "板载硬件鼠标键码需审阅并应用到设备后生效；宿主运行时推荐使用「鼠标控制」动作或摇杆切换",
                    "Creator Micro 2 板载固件动作 (0.7.0-rc.16+)",
                    "保存只改变本地草稿；应用到设备后跨主机脱机生效",
                )
            }
            Ok(ActionKind::DeviceKey { token }) if token.starts_with("KI_CS_") => {
                prerequisites.push(
                    "需固件 0.6.3+ 应用到 /fs/keymap.json 且当前 Mac 保持 Vendor HID 连接".into(),
                );
                (
                    format!("板载速查通知 · {token}"),
                    false,
                    false,
                    "按键按下后由固件经 Vendor 通道发送 kb.cs.* 通知唤醒桌面速查浮窗",
                    "Creator Micro 2 板载速查触发器 (0.6.3+)",
                    "应用到设备后按下触发 Vendor 通知并控制桌面速查浮窗",
                )
            }
            Ok(ActionKind::DeviceKey { token }) => (
                token,
                false,
                false,
                "设备按键需审阅并应用配置后生效；不是 Mac 宿主动作",
                "Micro Input 自有离线意图；不是已验证的设备或工具能力",
                "保存只改变本地草稿；没有执行结果",
            ),
            Ok(ActionKind::Shortcut { target, .. }) => {
                prerequisites.push("目标应用须支持此快捷键；辅助功能权限由宿主按需调用".into());
                (
                    target,
                    true,
                    true,
                    "",
                    "Mac 宿主动作执行器（CGEvent / NSWorkspace）",
                    "宿主激活对应 Profile 时按键可实时触发此动作",
                )
            }
            Ok(ActionKind::LocalMulti { .. }) => (
                "本地 Multi 四分支".into(),
                false,
                false,
                "Multi 执行未接入；时序、容量和设备格式未知",
                "Micro Input 自有离线意图；不是已验证的设备或工具能力",
                "保存只改变本地草稿；没有执行结果",
            ),
            Ok(ActionKind::LocalMacro { macro_sequence }) => {
                let supported = macro_sequence.validate().is_ok()
                    && macro_sequence.events.iter().all(|event| match event {
                        crate::macro_sequence::MacroEvent::Press { key }
                        | crate::macro_sequence::MacroEvent::Release { key } => {
                            crate::host_router::macos_virtual_key_code(key).is_some()
                        }
                        crate::macro_sequence::MacroEvent::Delay { .. } => true,
                    });
                if supported {
                    prerequisites
                        .push("序列只能使用已接入的 macOS 键名；辅助功能权限由宿主按需调用".into());
                    (
                        "本地普通宏序列".into(),
                        true,
                        true,
                        "",
                        "Mac 宿主动作执行器（CGEvent 逐步按下 / 释放）",
                        "宿主激活对应 Profile 时按键可按顺序执行；Multi 与设备驻留不在本能力内",
                    )
                } else {
                    prerequisites
                        .push("自有本地序列；按键名称不是固件 token，未知键名只保留为草稿".into());
                    (
                        "本地普通宏序列".into(),
                        false,
                        false,
                        "存在未知键名或无效序列；仅支持已验证的 macOS 键名执行",
                        "Micro Input 自有离线意图；不是已验证的设备或工具能力",
                        "保存只改变本地草稿；没有执行结果",
                    )
                }
            }
            Ok(ActionKind::Text { .. }) => {
                prerequisites.push(
                    "需要 Mac 宿主常驻与明确的文本输入目标；桌面 Agent 下自动聚焦输入区并暂存恢复原剪贴板"
                        .into(),
                );
                (
                    "当前文本输入目标".into(),
                    true,
                    true,
                    "",
                    "Mac 宿主动作执行器（输入区聚焦 + 剪贴板暂存恢复 + Cmd+V）",
                    "默认仅粘贴不自动回车（no_newline=true）；仅在显式配置或按提交键时停止活跃听写并提交",
                )
            }
            Ok(ActionKind::OpenUrl { url }) => {
                prerequisites.push("需要 Mac 宿主常驻与可处理 HTTP / HTTPS 的应用".into());
                (
                    url,
                    true,
                    true,
                    "",
                    "Mac 宿主动作执行器（NSWorkspace URL 打开）",
                    "宿主激活对应 Profile 时按键可实时触发此动作",
                )
            }
            Ok(ActionKind::OpenApp { bundle_id }) => {
                prerequisites.push("需要 Mac 宿主常驻；目标应用须已安装".into());
                (
                    bundle_id,
                    true,
                    true,
                    "",
                    "Mac 宿主动作执行器（NSWorkspace 应用激活）",
                    "宿主激活对应 Profile 时按键可实时触发此动作",
                )
            }
            Ok(ActionKind::AgentCommand { adapter, command })
                if matches!(
                    command.as_str(),
                    "back" | "navigate-back" | "forward" | "navigate-forward"
                ) && matches!(
                    adapter.as_str(),
                    "navigation" | "host" | "codex" | "antigravity" | "dsh-desktop" | "herdr"
                ) =>
            {
                prerequisites.push("需要 Mac 宿主常驻及至少一条小键盘打开历史".into());
                let direction = if matches!(command.as_str(), "back" | "navigate-back") {
                    "后退"
                } else {
                    "前进"
                };
                (
                    format!("跨 Agent 导航 / {direction}"),
                    true,
                    true,
                    "",
                    "Mac 宿主跨 Agent 导航历史栈（最多 50 条）",
                    "回放小键盘打开历史中的上一/下一可用目标；后退保留前进栈，新打开截断前进栈",
                )
            }
            Ok(ActionKind::ShellCommand {
                command: _,
                working_directory,
            }) => {
                prerequisites.push("固定使用 /bin/sh -c；工作目录必须是绝对路径".into());
                prerequisites.push("10 秒超时；回执只含退出状态、路径和命令字节数".into());
                if command_execution_enabled {
                    (
                        working_directory,
                        true,
                        true,
                        "",
                        "Mac 宿主命令执行器（独立同意开关）",
                        "成功或失败表示子进程退出状态；命令文本和输出不进入审计",
                    )
                } else {
                    (
                        working_directory,
                        true,
                        false,
                        "命令执行同意未开启；默认不会路由或启动子进程",
                        "Micro Input 自有离线意图；不是已验证的设备或工具能力",
                        "保存只改变本地草稿；没有执行结果",
                    )
                }
            }
            Ok(ActionKind::AgentCommand { adapter, command })
                if matches!(
                    adapter.as_str(),
                    "four-app" | "host" | "codex" | "antigravity" | "dsh-desktop"
                ) && (command.starts_with("slot-open:")
                    || matches!(
                        command.as_str(),
                        "four-app-attention"
                            | "approve"
                            | "deny"
                            | "new-or-stop"
                            | "stop"
                            | "next-workspace"
                            | "previous-workspace"
                    )) =>
            {
                prerequisites.push(
                    "需要 Mac 宿主常驻及对应桌面应用（Antigravity / DSH）会话桥或辅助功能权限"
                        .into(),
                );
                (
                    format!("{adapter} / {command}"),
                    true,
                    true,
                    "",
                    "Mac 宿主四应用总控路由器（FourAppSlots / 会话审批 / 停止与冷会话物化）",
                    "宿主激活对应 Profile 时按键可直达跨应用槽位、审批、停止或新建会话",
                )
            }
            Ok(ActionKind::AgentCommand { adapter, command })
                if matches!(
                    adapter.as_str(),
                    "host" | "codex" | "antigravity" | "dsh-desktop"
                ) && (command == "submit"
                    || command
                        .strip_prefix("prompt:")
                        .is_some_and(|s| !s.trim_end_matches(['\r', '\n']).trim().is_empty())
                    || command
                        .strip_prefix("submit-prompt:")
                        .is_some_and(|s| !s.trim_end_matches(['\r', '\n']).trim().is_empty())) =>
            {
                prerequisites.push(
                    "需要目标桌面 Agent 运行、辅助功能权限及唯一可访问输入框；活跃听写未确认停止时自动阻断提交"
                        .into(),
                );
                let semantics = if command.starts_with("prompt:") {
                    "聚焦目标输入区、暂存并恢复原剪贴板后仅粘贴文本（no_newline=true，不自动回车）"
                } else {
                    "若听写激活则先停并确认停止，随后聚焦目标输入区（粘贴后）回车提交"
                };
                (
                    format!("{adapter} / {command}"),
                    true,
                    true,
                    "",
                    "Mac 宿主防冲突提交与 Prompt 派发器（AX 输入区聚焦 + 听写清理门禁）",
                    semantics,
                )
            }
            Ok(ActionKind::AgentCommand { adapter, command })
                if matches!(
                    adapter.as_str(),
                    "host" | "codex" | "antigravity" | "dsh-desktop"
                ) && matches!(
                    command.as_str(),
                    "model-picker-open"
                        | "model-picker-prev"
                        | "model-picker-next"
                        | "model-picker-confirm"
                        | "model-picker-effort-right"
                        | "model-picker-effort-left"
                        | "model-picker-cancel"
                ) =>
            {
                prerequisites.push(
                    "需要目标桌面 Agent（Antigravity / Codex / DSH）运行及辅助功能权限；弹层内按键直投目标窗口不重聚焦输入框"
                        .into(),
                );
                (
                    format!("{adapter} / {command}"),
                    true,
                    true,
                    "",
                    "Mac 宿主目标应用模型选择与推理力度控制器（AX / 窗口直投按键）",
                    "打开目标 Agent 模型选择页，或在不关闭浮层的前提下切换模型、调整推理力度并确认",
                )
            }
            Ok(ActionKind::AgentCommand { adapter, command })
                if adapter == "host" && crate::is_host_mouse_command(&command) =>
            {
                prerequisites.push(
                    "需要 Mac 宿主常驻与辅助功能（CGEvent）权限；无焦点抢占移动指针、点击与四向滚轮"
                        .into(),
                );
                (
                    format!("host / {command}"),
                    true,
                    true,
                    "",
                    "Mac 宿主 CoreGraphics 指针、点击、四向滚轮与三档速度控制器",
                    "按当前宿主速度档位（0/1/2）实时移动光标、发送左/右/中键点击或四向滚轮事件",
                )
            }
            Ok(ActionKind::AgentCommand { adapter, command }) => {
                let desktop = (matches!(adapter.as_str(), "codex" | "antigravity" | "dsh-desktop")
                    && (matches!(
                        command.as_str(),
                        "model"
                            | "effort"
                            | "dictation"
                            | "browser"
                            | "terminal"
                            | "scroll-up"
                            | "scroll-down"
                            | "scroll-up-fast"
                            | "scroll-down-fast"
                            | "scroll-to-bottom"
                            | "sidebar-scroll-up"
                            | "sidebar-scroll-down"
                            | "prev-project"
                            | "next-project"
                            | "cycle-working-prev"
                            | "cycle-working-next"
                            | "cycle-completed-prev"
                            | "cycle-completed-next"
                            | "cycle-attention-prev"
                            | "cycle-attention-next"
                    ) || (command == "push-to-talk"
                        && matches!(adapter.as_str(), "codex" | "antigravity" | "dsh-desktop"))
                        || (matches!(command.as_str(), "voice-reply" | "toggle-voice-reply")
                            && matches!(
                                adapter.as_str(),
                                "codex" | "antigravity" | "dsh-desktop"
                            ))
                        || command == "focus-input"
                        || command == "search"
                        || command == "new-chat"
                        || (adapter == "codex"
                            && matches!(
                                command.as_str(),
                                "live-voice"
                                    | "new-tab"
                                    | "cycle-workspace-layout"
                                    | "toggle-dictation"
                                    | "effort-down"
                                    | "effort-up"
                                    | "toggle-fast-mode"
                            ))
                        || (adapter == "antigravity"
                            && matches!(
                                command.as_str(),
                                "live-voice"
                                    | "prev-conversation"
                                    | "next-conversation"
                                    | "project"
                                    | "new-conversation-in-project"
                                    | "split-right"
                            ))))
                    || (matches!(
                        adapter.as_str(),
                        "host" | "codex" | "antigravity" | "dsh-desktop"
                    ) && command == "search");
                let verified = matches!(adapter.as_str(), "herdr") || desktop;
                if verified {
                    prerequisites.push(if desktop {
                        "需要目标应用、辅助功能权限及唯一可访问控件".into()
                    } else {
                        format!("需要本机已安装并配置 {adapter} CLI 适配器")
                    });
                    (
                        format!("{adapter} / {command}"),
                        true,
                        true,
                        "",
                        "Mac 宿主适配器（CLI / 辅助功能）",
                        "宿主激活对应 Profile 时按键可实时触发此动作",
                    )
                } else {
                    (
                        format!("{adapter} / {command}"),
                        true,
                        false,
                        "工具适配器未验证、未接入；引用仅为本地意图",
                        "Micro Input 自有离线意图；不是已验证的设备或工具能力",
                        "保存只改变本地草稿；没有执行结果",
                    )
                }
            }
            Ok(ActionKind::FocusSession { adapter, session }) => {
                let verified = matches!(adapter.as_str(), "herdr") && session != "unselected";
                if session == "unselected" {
                    prerequisites.push("缺失具体会话引用；请先填写本地意图".into());
                }
                if verified {
                    prerequisites.push(format!("需要本机已安装并配置 {adapter} CLI 适配器"));
                    (
                        format!("{adapter} / {session}"),
                        true,
                        true,
                        "",
                        "Mac 宿主 CLI 适配器（herdr）",
                        "宿主激活对应 Profile 时按键可实时触发此动作",
                    )
                } else {
                    (
                        format!("{adapter} / {session}"),
                        true,
                        false,
                        "工具适配器未验证或缺失具体会话引用；无法确认目标会话",
                        "Micro Input 自有离线意图；不是已验证的设备或工具能力",
                        "保存只改变本地草稿；没有执行结果",
                    )
                }
            }
            Ok(ActionKind::SkillReference { adapter, skill }) => {
                if skill == "unselected" {
                    prerequisites.push("缺失具体 Skill 引用；请先填写本地意图".into());
                }
                (
                    format!("{adapter} / {skill}"),
                    true,
                    false,
                    "工具适配器未验证、未接入；无法解析 Skill 引用",
                    "Micro Input 自有离线意图；不是已验证的设备或工具能力",
                    "保存只改变本地草稿；没有执行结果",
                )
            }
            Err(_) => (
                "未知".into(),
                false,
                false,
                "未知动作或参数；原始值保留，不可执行",
                "Micro Input 自有离线意图；不是已验证的设备或工具能力",
                "保存只改变本地草稿；没有执行结果",
            ),
        };
    ActionCapability {
        target,
        prerequisites,
        host_required,
        execution_available,
        unavailable_reason: reason.into(),
        source: source.into(),
        result_semantics: result_semantics.into(),
    }
}

pub fn capability_localized(
    action: &Action,
    command_execution_enabled: bool,
    locale: crate::ResolvedLocale,
) -> ActionCapability {
    let base = capability(action, command_execution_enabled);
    if locale == crate::ResolvedLocale::ZhHans {
        return base;
    }
    let localize_target = |t: &str| -> String {
        if let Some(tok) = t.strip_prefix("硬件 HID 鼠标 · ") {
            format!("Hardware HID Mouse · {tok}")
        } else if let Some(tok) = t.strip_prefix("板载速查通知 · ") {
            format!("Onboard Cheat Sheet · {tok}")
        } else if t == "跨 Agent 导航 / 后退" {
            "Cross-Agent Navigation / Back".into()
        } else if t == "跨 Agent 导航 / 前进" {
            "Cross-Agent Navigation / Forward".into()
        } else {
            crate::i18n::tr(t, locale).to_string()
        }
    };
    let localize_prereq = |p: &str| -> String {
        if let Some(rest) = p.strip_prefix("需要本机已安装并配置 ") {
            if let Some(adapter) = rest.strip_suffix(" CLI 适配器") {
                return format!("Requires {adapter} CLI adapter installed and configured locally");
            }
        }
        crate::i18n::tr(p, locale).to_string()
    };
    ActionCapability {
        target: localize_target(&base.target),
        prerequisites: base
            .prerequisites
            .iter()
            .map(|p| localize_prereq(p))
            .collect(),
        host_required: base.host_required,
        execution_available: base.execution_available,
        unavailable_reason: crate::i18n::tr(&base.unavailable_reason, locale).to_string(),
        source: crate::i18n::tr(&base.source, locale).to_string(),
        result_semantics: crate::i18n::tr(&base.result_semantics, locale).to_string(),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LibraryEntry {
    pub action: Action,
    pub capability: ActionCapability,
    pub favorite: bool,
}

struct CachedCatalogEntry {
    action: Action,
    cap_enabled: [ActionCapability; 2],
    cap_disabled: [ActionCapability; 2],
    search_text_enabled: String,
    search_text_disabled: String,
}

fn cached_catalog() -> &'static [CachedCatalogEntry] {
    static INDEX: std::sync::OnceLock<Vec<CachedCatalogEntry>> = std::sync::OnceLock::new();
    INDEX.get_or_init(|| {
        actions()
            .into_iter()
            .map(|action| {
                let cap_enabled_zh =
                    capability_localized(&action, true, crate::ResolvedLocale::ZhHans);
                let cap_enabled_en = capability_localized(&action, true, crate::ResolvedLocale::En);
                let cap_disabled_zh =
                    capability_localized(&action, false, crate::ResolvedLocale::ZhHans);
                let cap_disabled_en =
                    capability_localized(&action, false, crate::ResolvedLocale::En);
                let en_pres = crate::i18n::localize_action(&action, crate::ResolvedLocale::En);
                let search_text_enabled = format!(
                    "{} {} {} {} {} {} {} {} {}",
                    action.name,
                    action.category,
                    action.detail,
                    cap_enabled_zh.target,
                    en_pres.name,
                    en_pres.category,
                    en_pres.summary,
                    cap_enabled_en.target,
                    action.payload
                )
                .to_lowercase();
                let search_text_disabled = format!(
                    "{} {} {} {} {} {} {} {} {}",
                    action.name,
                    action.category,
                    action.detail,
                    cap_disabled_zh.target,
                    en_pres.name,
                    en_pres.category,
                    en_pres.summary,
                    cap_disabled_en.target,
                    action.payload
                )
                .to_lowercase();
                CachedCatalogEntry {
                    action,
                    cap_enabled: [cap_enabled_zh, cap_enabled_en],
                    cap_disabled: [cap_disabled_zh, cap_disabled_en],
                    search_text_enabled,
                    search_text_disabled,
                }
            })
            .collect()
    })
}

pub fn search(
    query: &str,
    category: &str,
    favorites_only: bool,
    favorites: &BTreeSet<String>,
    command_execution_enabled: bool,
) -> Vec<LibraryEntry> {
    search_localized(
        query,
        category,
        favorites_only,
        favorites,
        command_execution_enabled,
        crate::ResolvedLocale::ZhHans,
    )
}

pub fn search_localized(
    query: &str,
    category: &str,
    favorites_only: bool,
    favorites: &BTreeSet<String>,
    command_execution_enabled: bool,
    locale: crate::ResolvedLocale,
) -> Vec<LibraryEntry> {
    let query = query.trim().to_lowercase();
    let is_all = crate::i18n::is_all_category_filter(category);
    let canonical_cat = crate::i18n::canonical_category_key(category);
    let locale_idx = locale.index();
    cached_catalog()
        .iter()
        .filter_map(|entry| {
            let favorite = favorites.contains(&entry.action.id);
            let (info, text) = if command_execution_enabled {
                (&entry.cap_enabled[locale_idx], &entry.search_text_enabled)
            } else {
                (&entry.cap_disabled[locale_idx], &entry.search_text_disabled)
            };
            if (is_all || canonical_cat == entry.action.category)
                && (!favorites_only || favorite)
                && (query.is_empty() || text.contains(&query))
            {
                let mut localized_action = entry.action.clone();
                if locale == crate::ResolvedLocale::En {
                    let pres = crate::i18n::localize_action(&localized_action, locale);
                    localized_action.name = pres.name;
                    localized_action.category = pres.category;
                    localized_action.detail = pres.summary;
                }
                Some(LibraryEntry {
                    action: localized_action,
                    capability: info.clone(),
                    favorite,
                })
            } else {
                None
            }
        })
        .collect()
}
