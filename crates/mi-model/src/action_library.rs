//! Local action intents and honest prerequisites. No adapter discovery or execution.
use crate::draft::{actions, Action, ActionKind};
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize)]
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
                prerequisites.push("需要 Mac 宿主常驻与明确的文本输入目标".into());
                (
                    "当前文本输入目标".into(),
                    true,
                    true,
                    "",
                    "Mac 宿主动作执行器（剪贴板恢复 + Cmd+V）",
                    "宿主激活对应 Profile 时按键可实时触发此动作",
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
                if adapter == "t3" && command == "next-attention" =>
            {
                prerequisites
                    .push("需要已安装的 T3 Code、显式开启只读连接及新鲜的待处理目标".into());
                (
                    "T3 Code / 下一个需要我".into(),
                    true,
                    true,
                    "",
                    "T3 Code v0.1.6 只读 shell 与身份深链",
                    "交付打开请求与目标访问确认分别报告；不发送或批准会话内容",
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
            Ok(ActionKind::AgentCommand { adapter, command }) => {
                let desktop = matches!(
                    adapter.as_str(),
                    "cindy"
                        | "zcode"
                        | "qwenwork"
                        | "codex"
                        | "antigravity"
                        | "dsh-desktop"
                        | "qoder-cn"
                ) && (matches!(
                    command.as_str(),
                    "model" | "effort" | "dictation" | "browser" | "terminal"
                ) || (command == "push-to-talk"
                    && matches!(
                        adapter.as_str(),
                        "cindy" | "codex" | "qwenwork" | "antigravity" | "qoder-cn"
                    ))
                    || command == "focus-input"
                    || (command == "search" && adapter != "qwenwork")
                    || (command == "new-chat"
                        && matches!(adapter.as_str(), "antigravity" | "dsh-desktop" | "qoder-cn")));
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

#[derive(Debug, Clone, Serialize)]
pub struct LibraryEntry {
    pub action: Action,
    pub capability: ActionCapability,
    pub favorite: bool,
}

pub fn search(
    query: &str,
    category: &str,
    favorites_only: bool,
    favorites: &BTreeSet<String>,
    command_execution_enabled: bool,
) -> Vec<LibraryEntry> {
    let query = query.trim().to_lowercase();
    actions()
        .into_iter()
        .filter_map(|action| {
            let favorite = favorites.contains(&action.id);
            let info = capability(&action, command_execution_enabled);
            let text = format!(
                "{} {} {} {} {}",
                action.name, action.category, action.detail, info.target, action.payload
            )
            .to_lowercase();
            if (category == "全部" || category == action.category)
                && (!favorites_only || favorite)
                && (query.is_empty() || text.contains(&query))
            {
                Some(LibraryEntry {
                    action,
                    capability: info,
                    favorite,
                })
            } else {
                None
            }
        })
        .collect()
}
