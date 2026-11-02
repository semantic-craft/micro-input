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

pub fn capability(action: &Action) -> ActionCapability {
    let mut prerequisites = vec!["仅可分配到本地逻辑槽位；物理映射待验证".into()];
    let (target, host_required, reason) =
        match serde_json::from_value::<ActionKind>(action.payload.clone()) {
            Ok(ActionKind::Shortcut { target, .. }) => {
                prerequisites.push("目标应用须在前台并支持此快捷键；当前不检测焦点".into());
                (target, false, "快捷键发送未接入；设备独立执行能力未验证")
            }
            Ok(ActionKind::LocalMulti { .. }) => (
                "本地 Multi 四分支".into(),
                false,
                "Multi 执行未接入；时序、容量和设备格式未知",
            ),
            Ok(ActionKind::LocalMacro { .. }) => {
                prerequisites.push(
                    "自有本地序列；按键名称不是固件 token，设备与宿主执行能力均未核实".into(),
                );
                (
                    "本地普通宏序列".into(),
                    false,
                    "宏执行未接入；设备驻留与宿主要求未核实",
                )
            }
            Ok(ActionKind::Text { .. }) => {
                prerequisites.push("需要 Mac 宿主常驻与明确的文本输入目标".into());
                ("当前文本输入目标（未检测）".into(), true, "文本执行未接入")
            }
            Ok(ActionKind::OpenUrl { url }) => {
                prerequisites.push("需要 Mac 宿主常驻与可处理 HTTP / HTTPS 的应用".into());
                (url, true, "网址执行未接入")
            }
            Ok(ActionKind::OpenApp { bundle_id }) => {
                prerequisites.push("需要 Mac 宿主常驻；目标应用须已安装（未检测）".into());
                (bundle_id, true, "应用执行未接入")
            }
            Ok(ActionKind::AgentCommand { adapter, command }) => (
                format!("{adapter} / {command}"),
                true,
                "工具适配器未验证、未接入；引用仅为本地意图",
            ),
            Ok(ActionKind::FocusSession { adapter, session }) => {
                if session == "unselected" {
                    prerequisites.push("缺失具体会话引用；请先填写本地意图".into());
                }
                (
                    format!("{adapter} / {session}"),
                    true,
                    "工具适配器未验证、未接入；无法确认目标会话",
                )
            }
            Ok(ActionKind::SkillReference { adapter, skill }) => {
                if skill == "unselected" {
                    prerequisites.push("缺失具体 Skill 引用；请先填写本地意图".into());
                }
                (
                    format!("{adapter} / {skill}"),
                    true,
                    "工具适配器未验证、未接入；无法解析 Skill 引用",
                )
            }
            Err(_) => ("未知".into(), false, "未知动作或参数；原始值保留，不可执行"),
        };
    ActionCapability {
        target,
        prerequisites,
        host_required,
        execution_available: false,
        unavailable_reason: reason.into(),
        source: "Micro Input 自有离线意图；不是已验证的设备或工具能力".into(),
        result_semantics: "保存只改变本地草稿；没有执行结果".into(),
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
) -> Vec<LibraryEntry> {
    let query = query.trim().to_lowercase();
    actions()
        .into_iter()
        .filter_map(|action| {
            let favorite = favorites.contains(&action.id);
            let info = capability(&action);
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
