//! Own-format interchange only. No firmware serialization or device authority.
use crate::{
    document::{new_id, LocalDocumentV2},
    draft::{ActionKind, Draft},
    multi::ActionReference,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const LIMIT: usize = 4 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    pub format: String,
    pub version: u32,
    pub name: String,
    pub scope: String,
    pub metadata: Value,
    pub document: Value,
}
fn checked_document(value: &Value) -> Result<LocalDocumentV2, String> {
    let draft = Draft::from_bytes(&serde_json::to_vec(value).map_err(|e| e.to_string())?)?;
    if draft.loaded_format != 2 || draft.read_only {
        return Err("预设含未知字段或动作格式，未导入；原文件保留".into());
    }
    draft.document.view()?;
    Ok(draft.document)
}
pub fn parse(text: &str) -> Result<Preset, String> {
    if text.len() > LIMIT {
        return Err("预设超过 4 MiB，未导入".into());
    }
    let p: Preset =
        serde_json::from_str(text).map_err(|e| format!("不是有效的 Micro Input 自有预设：{e}"))?;
    if p.format != "micro-input-preset"
        || p.version != 1
        || !["document", "profile", "layer"].contains(&p.scope.as_str())
    {
        return Err("预设格式或版本不支持；不支持厂商格式".into());
    }
    if p.name.trim().is_empty() || p.name.len() > 256 {
        return Err("预设名称无效".into());
    }
    checked_document(&p.document)?;
    Ok(p)
}
pub fn inspect(text: &str) -> Result<String, String> {
    let p = parse(text)?;
    let d = checked_document(&p.document)?;
    serde_json::to_string(&json!({"name":p.name,"scope":p.scope,
        "profiles":d.editable.graph.profiles.len(),"layers":d.editable.graph.layers.len(),
        "actions":d.editable.graph.actions.len()}))
    .map_err(|e| e.to_string())
}
pub fn export(draft: &Draft, pid: Option<&str>, lid: Option<&str>) -> Result<String, String> {
    if draft.read_only {
        return Err("文档含未知字段，不能正规化导出；请保留原文件".into());
    }
    if lid.is_some() && pid.is_none() {
        return Err("导出层必须指定所属配置".into());
    }
    let mut doc = draft.document.clone();
    let mut scope = "document";
    let mut name = "Micro Input 本地配置".to_owned();
    if let Some(pid) = pid {
        let profile = doc.editable.graph.profiles.get(pid).ok_or("配置不存在")?;
        name = profile.name.clone();
        scope = "profile";
        let layers: BTreeSet<_> = if let Some(lid) = lid {
            if !profile.layer_order.iter().any(|id| id == lid) {
                return Err("层不属于所选配置".into());
            }
            name = doc.editable.graph.layers[lid].name.clone();
            scope = "layer";
            [lid.to_owned()].into()
        } else {
            profile.layer_order.iter().cloned().collect()
        };
        doc.editable.graph.profile_order.retain(|id| id == pid);
        doc.editable.graph.profiles.retain(|id, _| id == pid);
        doc.editable
            .graph
            .profiles
            .get_mut(pid)
            .unwrap()
            .layer_order
            .retain(|id| layers.contains(id));
        doc.editable
            .graph
            .layers
            .retain(|id, _| layers.contains(id));
        let mut used = BTreeSet::new();
        let mut pending: Vec<_> = doc
            .editable
            .graph
            .layers
            .values()
            .flat_map(|l| l.controls.values())
            .filter_map(|b| b.action_id.clone())
            .collect();
        while let Some(id) = pending.pop() {
            if !used.insert(id.clone()) {
                continue;
            }
            let action = doc
                .editable
                .graph
                .actions
                .get(&id)
                .ok_or("动作引用不存在")?;
            if let Some(m) = crate::multi::local_multi(action) {
                pending.extend(
                    m.branches()
                        .iter()
                        .filter_map(|(_, r)| r.map(|r| r.id().to_owned())),
                );
            }
        }
        doc.editable.graph.actions.retain(|id, _| used.contains(id));
        let ids = object_ids(&doc);
        doc.editable.source_refs.retain(|id, _| ids.contains(id));
    }
    doc.validate()?;
    // The immutable source archive is deliberately preserved, including unknown raw values.
    // UI must disclose that scoped exports still contain this complete archive.
    let p = Preset {
        format: "micro-input-preset".into(),
        version: 1,
        name,
        scope: scope.into(),
        metadata: json!({"source_archive":"complete","device_compatibility":"unverified"}),
        document: serde_json::to_value(doc).map_err(|e| e.to_string())?,
    };
    let text = serde_json::to_string_pretty(&p).map_err(|e| e.to_string())?;
    if text.len() > LIMIT {
        return Err("导出预设超过 4 MiB".into());
    }
    Ok(text)
}
fn object_ids(doc: &LocalDocumentV2) -> BTreeSet<String> {
    let g = &doc.editable.graph;
    g.profiles
        .keys()
        .chain(g.layers.keys())
        .chain(g.actions.keys())
        .cloned()
        .chain(g.layers.values().flat_map(|l| l.controls.keys().cloned()))
        .collect()
}
pub fn import(text: &str) -> Result<Draft, String> {
    let p = parse(text)?;
    let mut doc = checked_document(&p.document)?;
    let map: BTreeMap<_, _> = object_ids(&doc)
        .into_iter()
        .map(|id| (id, new_id()))
        .collect();
    let g = &mut doc.editable.graph;
    g.profile_order = g.profile_order.iter().map(|id| map[id].clone()).collect();
    g.profiles = std::mem::take(&mut g.profiles)
        .into_iter()
        .map(|(id, mut v)| {
            v.layer_order = v.layer_order.iter().map(|id| map[id].clone()).collect();
            (map[&id].clone(), v)
        })
        .collect();
    g.layers = std::mem::take(&mut g.layers)
        .into_iter()
        .map(|(id, mut v)| {
            v.control_order = v.control_order.iter().map(|id| map[id].clone()).collect();
            v.controls = v
                .controls
                .into_iter()
                .map(|(id, mut b)| {
                    b.action_id = b.action_id.map(|a| map[&a].clone());
                    (map[&id].clone(), b)
                })
                .collect();
            (map[&id].clone(), v)
        })
        .collect();
    g.actions = std::mem::take(&mut g.actions)
        .into_iter()
        .map(|(id, mut a)| {
            if let Some(mut m) = crate::multi::local_multi(&a) {
                for r in [&mut m.tap, &mut m.double_tap, &mut m.hold, &mut m.tap_hold]
                    .into_iter()
                    .flatten()
                {
                    let (ActionReference::Macro { action_id }
                    | ActionReference::Action { action_id }) = r;
                    *action_id = map[action_id].clone();
                }
                a.payload = serde_json::to_value(ActionKind::LocalMulti { multi: m })
                    .expect("serializable action");
            }
            (map[&id].clone(), a)
        })
        .collect();
    doc.editable.source_refs = std::mem::take(&mut doc.editable.source_refs)
        .into_iter()
        .map(|(id, v)| (map[&id].clone(), v))
        .collect();
    // An imported baseline is history, never authority for the connected device.
    let mut old_origin = doc.origin.clone();
    // Store the source payload once: duplicating it in the history wrapper would make
    // otherwise valid exported files exceed the document budget when imported.
    let config = old_origin
        .as_object_mut()
        .ok_or("来源记录格式无效")?
        .remove("config")
        .ok_or("来源配置缺失")?;
    doc.origin = json!({"kind":"imported-local","config":config,
        "imported_origin":old_origin,"imported_baseline":doc.baseline,
        "preset_metadata":p.metadata,"preset_name":p.name,"preset_scope":p.scope,
        "imported_document_id":doc.document_id});
    doc.baseline = None;
    doc.document_id = new_id();
    doc.revision = 0;
    doc.validate()?;
    Draft::from_document(doc, 2, false)
}
pub fn starter() -> Result<String, String> {
    let mut d = Draft::default();
    // Build a fresh original preset, without carrying the bundled demo's unknown bindings.
    let ids = d.document.editable.graph.profile_order.clone();
    for id in ids {
        d.delete_profile(&id)?;
    }
    let pid = d.create_profile("Coding 起步")?;
    let lid = d.document.editable.graph.profiles[&pid].layer_order[0].clone();
    for (slot, id) in [(0, "search"), (1, "save")] {
        let a = crate::draft::actions()
            .into_iter()
            .find(|a| a.id == id)
            .unwrap();
        d.assign(&json!(pid), &json!(lid), slot, a)?;
    }
    let (mut config, _) = d.document.view()?;
    config.source = crate::Source::LocalDraft;
    config.read_at = 0;
    config.firmware = None;
    config.device_files = None;
    config.active_profile = crate::ActiveProfile::Unknown(Value::Null);
    config.macros.clear();
    config.multi_actions.clear();
    config.smart_actions = Value::Null;
    config.warnings.clear();
    config.raw = json!({"kind":"original-local-preset"});
    d.document.origin = json!({"kind":"original-local-preset","config":config});
    d.document.editable.source_refs.clear();
    let mut preset = parse(&export(&d, None, None)?)?;
    preset.name = "Coding 起步".into();
    preset.metadata["author"] = json!("Micro Input");
    serde_json::to_string_pretty(&preset).map_err(|e| e.to_string())
}

type AgentAssignment = (
    u32,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    ActionKind,
);
fn legacy_agent_specs() -> Vec<(&'static str, Vec<AgentAssignment>)> {
    vec![
        (
            "Antigravity",
            vec![
                (
                    0,
                    "ag-focus",
                    "聚焦 Antigravity",
                    "应用",
                    "com.google.antigravity",
                    ActionKind::OpenApp {
                        bundle_id: "com.google.antigravity".into(),
                    },
                ),
                (
                    1,
                    "ag-panel",
                    "切换 Agent 面板",
                    "Coding",
                    "Cmd+L",
                    ActionKind::Shortcut {
                        modifiers: vec!["command".into()],
                        key: "L".into(),
                        target: "frontmost-editor".into(),
                    },
                ),
                (
                    2,
                    "ag-review",
                    "注入审查提示词",
                    "文本",
                    "请检查边界条件与回归测试",
                    ActionKind::Text {
                        text: "请检查边界条件与回归测试".into(),
                    },
                ),
                (
                    3,
                    "ag-plan",
                    "注入计划提示词",
                    "文本",
                    "请先给出分阶段实施与验证计划",
                    ActionKind::Text {
                        text: "请先给出分阶段实施与验证计划".into(),
                    },
                ),
                (
                    4,
                    "ag-test",
                    "注入验证提示词",
                    "文本",
                    "请运行 check 并验证当前改动",
                    ActionKind::Text {
                        text: "请运行 check 并验证当前改动".into(),
                    },
                ),
                (
                    5,
                    "ag-refactor",
                    "注入重构提示词",
                    "文本",
                    "请在不改变外部行为的前提下简化当前实现",
                    ActionKind::Text {
                        text: "请在不改变外部行为的前提下简化当前实现".into(),
                    },
                ),
                (
                    6,
                    "ag-term",
                    "切换终端",
                    "Coding",
                    "Ctrl+`",
                    ActionKind::Shortcut {
                        modifiers: vec!["control".into()],
                        key: "grave".into(),
                        target: "frontmost-editor".into(),
                    },
                ),
                (
                    7,
                    "ag-new-chat",
                    "新建对话",
                    "Coding",
                    "Cmd+N",
                    ActionKind::Shortcut {
                        modifiers: vec!["command".into()],
                        key: "N".into(),
                        target: "frontmost-editor".into(),
                    },
                ),
                (
                    8,
                    "ag-open",
                    "快速搜索文件",
                    "Coding",
                    "Cmd+P",
                    ActionKind::Shortcut {
                        modifiers: vec!["command".into()],
                        key: "P".into(),
                        target: "frontmost-editor".into(),
                    },
                ),
                (
                    9,
                    "ag-save",
                    "保存当前文件",
                    "Coding",
                    "Cmd+S",
                    ActionKind::Shortcut {
                        modifiers: vec!["command".into()],
                        key: "S".into(),
                        target: "frontmost-editor".into(),
                    },
                ),
                (
                    10,
                    "ag-accept",
                    "同意继续",
                    "Coding",
                    "Cmd+Return",
                    ActionKind::Shortcut {
                        modifiers: vec!["command".into()],
                        key: "Return".into(),
                        target: "frontmost-editor".into(),
                    },
                ),
                (
                    12,
                    "ag-stop",
                    "取消 / 拒绝",
                    "Coding",
                    "Escape",
                    ActionKind::Shortcut {
                        modifiers: vec![],
                        key: "Escape".into(),
                        target: "frontmost-editor".into(),
                    },
                ),
                (
                    13,
                    "ag-prev-tab",
                    "上一标签页",
                    "Coding",
                    "Cmd+Shift+[",
                    ActionKind::Shortcut {
                        modifiers: vec!["command".into(), "shift".into()],
                        key: "[".into(),
                        target: "frontmost-editor".into(),
                    },
                ),
                (
                    14,
                    "ag-next-tab",
                    "下一标签页",
                    "Coding",
                    "Cmd+Shift+]",
                    ActionKind::Shortcut {
                        modifiers: vec!["command".into(), "shift".into()],
                        key: "]".into(),
                        target: "frontmost-editor".into(),
                    },
                ),
                (
                    15,
                    "ag-enc-click",
                    "切换侧边栏",
                    "Coding",
                    "Cmd+B",
                    ActionKind::Shortcut {
                        modifiers: vec!["command".into()],
                        key: "B".into(),
                        target: "frontmost-editor".into(),
                    },
                ),
                (
                    16,
                    "ag-north",
                    "向上切分屏焦点",
                    "Coding",
                    "Cmd+Opt+Up",
                    ActionKind::Shortcut {
                        modifiers: vec!["command".into(), "option".into()],
                        key: "Up".into(),
                        target: "frontmost-editor".into(),
                    },
                ),
                (
                    17,
                    "ag-west",
                    "向左切分屏焦点",
                    "Coding",
                    "Cmd+Opt+Left",
                    ActionKind::Shortcut {
                        modifiers: vec!["command".into(), "option".into()],
                        key: "Left".into(),
                        target: "frontmost-editor".into(),
                    },
                ),
                (
                    18,
                    "ag-south",
                    "向下切分屏焦点",
                    "Coding",
                    "Cmd+Opt+Down",
                    ActionKind::Shortcut {
                        modifiers: vec!["command".into(), "option".into()],
                        key: "Down".into(),
                        target: "frontmost-editor".into(),
                    },
                ),
                (
                    19,
                    "ag-east",
                    "向右切分屏焦点",
                    "Coding",
                    "Cmd+Opt+Right",
                    ActionKind::Shortcut {
                        modifiers: vec!["command".into(), "option".into()],
                        key: "Right".into(),
                        target: "frontmost-editor".into(),
                    },
                ),
            ],
        ),
        (
            "Herdr",
            vec![
                (
                    0,
                    "hd-s1",
                    "聚焦 Session 1",
                    "Agent",
                    "herdr / session-1",
                    ActionKind::FocusSession {
                        adapter: "herdr".into(),
                        session: "session-1".into(),
                    },
                ),
                (
                    1,
                    "hd-s2",
                    "聚焦 Session 2",
                    "Agent",
                    "herdr / session-2",
                    ActionKind::FocusSession {
                        adapter: "herdr".into(),
                        session: "session-2".into(),
                    },
                ),
                (
                    2,
                    "hd-s3",
                    "聚焦 Session 3",
                    "Agent",
                    "herdr / session-3",
                    ActionKind::FocusSession {
                        adapter: "herdr".into(),
                        session: "session-3".into(),
                    },
                ),
                (
                    3,
                    "hd-s4",
                    "聚焦 Session 4",
                    "Agent",
                    "herdr / session-4",
                    ActionKind::FocusSession {
                        adapter: "herdr".into(),
                        session: "session-4".into(),
                    },
                ),
                (
                    4,
                    "hd-s5",
                    "聚焦 Session 5",
                    "Agent",
                    "herdr / session-5",
                    ActionKind::FocusSession {
                        adapter: "herdr".into(),
                        session: "session-5".into(),
                    },
                ),
                (
                    5,
                    "hd-s6",
                    "聚焦 Session 6",
                    "Agent",
                    "herdr / session-6",
                    ActionKind::FocusSession {
                        adapter: "herdr".into(),
                        session: "session-6".into(),
                    },
                ),
                (
                    6,
                    "hd-new-ws",
                    "新建工作区",
                    "Agent",
                    "herdr / new-workspace",
                    ActionKind::AgentCommand {
                        adapter: "herdr".into(),
                        command: "new-workspace".into(),
                    },
                ),
                (
                    7,
                    "hd-split-r",
                    "向右分屏",
                    "Agent",
                    "herdr / split-right",
                    ActionKind::AgentCommand {
                        adapter: "herdr".into(),
                        command: "split-right".into(),
                    },
                ),
                (
                    8,
                    "hd-stack",
                    "切换栈视图",
                    "Agent",
                    "herdr / toggle-stack",
                    ActionKind::AgentCommand {
                        adapter: "herdr".into(),
                        command: "toggle-stack".into(),
                    },
                ),
                (
                    9,
                    "hd-close-pane",
                    "关闭当前窗格",
                    "Agent",
                    "herdr / close-pane",
                    ActionKind::AgentCommand {
                        adapter: "herdr".into(),
                        command: "close-pane".into(),
                    },
                ),
                (
                    10,
                    "hd-approve",
                    "同意当前会话请求",
                    "Agent",
                    "herdr / approve-current",
                    ActionKind::AgentCommand {
                        adapter: "herdr".into(),
                        command: "approve-current".into(),
                    },
                ),
                (
                    12,
                    "hd-deny",
                    "拒绝当前会话请求",
                    "Agent",
                    "herdr / deny-current",
                    ActionKind::AgentCommand {
                        adapter: "herdr".into(),
                        command: "deny-current".into(),
                    },
                ),
                (
                    13,
                    "hd-prev-session",
                    "上一会话",
                    "Agent",
                    "herdr / prev-session",
                    ActionKind::AgentCommand {
                        adapter: "herdr".into(),
                        command: "prev-session".into(),
                    },
                ),
                (
                    14,
                    "hd-next-session",
                    "下一会话",
                    "Agent",
                    "herdr / next-session",
                    ActionKind::AgentCommand {
                        adapter: "herdr".into(),
                        command: "next-session".into(),
                    },
                ),
                (
                    15,
                    "hd-enc-click",
                    "切换窗格缩放",
                    "Agent",
                    "herdr / toggle-zoom",
                    ActionKind::AgentCommand {
                        adapter: "herdr".into(),
                        command: "toggle-zoom".into(),
                    },
                ),
                (
                    16,
                    "hd-pane-north",
                    "北向分屏焦点",
                    "Agent",
                    "herdr / pane-north",
                    ActionKind::AgentCommand {
                        adapter: "herdr".into(),
                        command: "pane-north".into(),
                    },
                ),
                (
                    17,
                    "hd-pane-west",
                    "西向分屏焦点",
                    "Agent",
                    "herdr / pane-west",
                    ActionKind::AgentCommand {
                        adapter: "herdr".into(),
                        command: "pane-west".into(),
                    },
                ),
                (
                    18,
                    "hd-pane-south",
                    "南向分屏焦点",
                    "Agent",
                    "herdr / pane-south",
                    ActionKind::AgentCommand {
                        adapter: "herdr".into(),
                        command: "pane-south".into(),
                    },
                ),
                (
                    19,
                    "hd-pane-east",
                    "东向分屏焦点",
                    "Agent",
                    "herdr / pane-east",
                    ActionKind::AgentCommand {
                        adapter: "herdr".into(),
                        command: "pane-east".into(),
                    },
                ),
            ],
        ),
        (
            "DeepSeek Coder (DSH)",
            vec![
                (
                    0,
                    "dsh-s1",
                    "聚焦 DSH 会话 1",
                    "Agent",
                    "dsh / session-1",
                    ActionKind::FocusSession {
                        adapter: "dsh".into(),
                        session: "session-1".into(),
                    },
                ),
                (
                    1,
                    "dsh-s2",
                    "聚焦 DSH 会话 2",
                    "Agent",
                    "dsh / session-2",
                    ActionKind::FocusSession {
                        adapter: "dsh".into(),
                        session: "session-2".into(),
                    },
                ),
                (
                    2,
                    "dsh-debug",
                    "注入调试提示词",
                    "文本",
                    "请定位根因并给出最小修复方案",
                    ActionKind::Text {
                        text: "请定位根因并给出最小修复方案".into(),
                    },
                ),
                (
                    3,
                    "dsh-refactor",
                    "注入重构提示词",
                    "文本",
                    "请保持公共接口不变并重构此模块",
                    ActionKind::Text {
                        text: "请保持公共接口不变并重构此模块".into(),
                    },
                ),
                (
                    4,
                    "dsh-test",
                    "注入测试提示词",
                    "文本",
                    "请补充覆盖异常分支的单元测试",
                    ActionKind::Text {
                        text: "请补充覆盖异常分支的单元测试".into(),
                    },
                ),
                (
                    5,
                    "dsh-explain",
                    "注入解释提示词",
                    "文本",
                    "请说明这段代码的状态流转与边界条件",
                    ActionKind::Text {
                        text: "请说明这段代码的状态流转与边界条件".into(),
                    },
                ),
                (
                    6,
                    "dsh-interrupt",
                    "发送中断",
                    "Agent",
                    "dsh / interrupt",
                    ActionKind::AgentCommand {
                        adapter: "dsh".into(),
                        command: "interrupt".into(),
                    },
                ),
                (
                    7,
                    "dsh-clear",
                    "清屏",
                    "Agent",
                    "dsh / clear",
                    ActionKind::AgentCommand {
                        adapter: "dsh".into(),
                        command: "clear".into(),
                    },
                ),
                (
                    8,
                    "dsh-context",
                    "注入文件上下文",
                    "Agent",
                    "dsh / inject-context",
                    ActionKind::AgentCommand {
                        adapter: "dsh".into(),
                        command: "inject-context".into(),
                    },
                ),
                (
                    9,
                    "dsh-history",
                    "历史搜索",
                    "Agent",
                    "dsh / history-search",
                    ActionKind::AgentCommand {
                        adapter: "dsh".into(),
                        command: "history-search".into(),
                    },
                ),
                (
                    10,
                    "dsh-approve",
                    "同意执行当前工具调用",
                    "Agent",
                    "dsh / approve-current",
                    ActionKind::AgentCommand {
                        adapter: "dsh".into(),
                        command: "approve-current".into(),
                    },
                ),
                (
                    12,
                    "dsh-deny",
                    "拒绝 / 取消当前操作",
                    "Agent",
                    "dsh / deny-current",
                    ActionKind::AgentCommand {
                        adapter: "dsh".into(),
                        command: "deny-current".into(),
                    },
                ),
                (
                    13,
                    "dsh-prev-session",
                    "上一会话",
                    "Agent",
                    "dsh / prev-session",
                    ActionKind::AgentCommand {
                        adapter: "dsh".into(),
                        command: "prev-session".into(),
                    },
                ),
                (
                    14,
                    "dsh-next-session",
                    "下一会话",
                    "Agent",
                    "dsh / next-session",
                    ActionKind::AgentCommand {
                        adapter: "dsh".into(),
                        command: "next-session".into(),
                    },
                ),
                (
                    15,
                    "dsh-enc-click",
                    "查看会话状态",
                    "Agent",
                    "dsh / status",
                    ActionKind::AgentCommand {
                        adapter: "dsh".into(),
                        command: "status".into(),
                    },
                ),
                (
                    16,
                    "dsh-pane-north",
                    "北向分屏焦点",
                    "Agent",
                    "dsh / pane-north",
                    ActionKind::AgentCommand {
                        adapter: "dsh".into(),
                        command: "pane-north".into(),
                    },
                ),
                (
                    17,
                    "dsh-pane-west",
                    "西向分屏焦点",
                    "Agent",
                    "dsh / pane-west",
                    ActionKind::AgentCommand {
                        adapter: "dsh".into(),
                        command: "pane-west".into(),
                    },
                ),
                (
                    18,
                    "dsh-pane-south",
                    "南向分屏焦点",
                    "Agent",
                    "dsh / pane-south",
                    ActionKind::AgentCommand {
                        adapter: "dsh".into(),
                        command: "pane-south".into(),
                    },
                ),
                (
                    19,
                    "dsh-pane-east",
                    "东向分屏焦点",
                    "Agent",
                    "dsh / pane-east",
                    ActionKind::AgentCommand {
                        adapter: "dsh".into(),
                        command: "pane-east".into(),
                    },
                ),
            ],
        ),
    ]
}

pub fn install_agent_starter_profiles(draft: &mut Draft) -> Result<Vec<String>, String> {
    let mut created_ids = Vec::new();
    // Retain the exact old built-ins only as migration fingerprints.
    let mut legacy_specs = legacy_agent_specs();
    let mut specs = legacy_specs.clone();
    for (name, adapter, bundle) in [
        ("Antigravity", "antigravity", "com.google.antigravity"),
        ("DeepSeek Coder (DSH)", "dsh-desktop", "com.deepseek.dsh"),
    ] {
        let assignments = &mut specs.iter_mut().find(|(n, _)| *n == name).unwrap().1;
        assignments.clear();
        assignments.push((
            0,
            "desktop-open",
            "打开 / 聚焦应用",
            "应用",
            bundle,
            ActionKind::OpenApp {
                bundle_id: bundle.into(),
            },
        ));
        for (slot, id, label, command) in [
            (1, "desktop-input", "聚焦当前输入框", "focus-input"),
            (2, "desktop-search", "搜索会话", "search"),
            (3, "desktop-new", "新建会话", "new-chat"),
        ] {
            assignments.push((
                slot,
                id,
                label,
                "Agent",
                bundle,
                ActionKind::AgentCommand {
                    adapter: adapter.into(),
                    command: command.into(),
                },
            ));
        }
    }

    // Desktop targets are bound explicitly; no inferred approval/send shortcuts.
    for (name, adapter, bundle) in [
        ("Cindy", "cindy", "com.xd.cindy"),
        ("Codex", "codex", "com.openai.codex"),
        ("千问办公", "qwenwork", "cn.qwenwork.desktop.mac"),
        ("ZCode", "zcode", "dev.zcode.app"),
        ("Qoder CN", "qoder-cn", "com.qodercn.app"),
    ] {
        let mut assignments = vec![(
            0,
            "desktop-open",
            "打开 / 聚焦应用",
            "应用",
            bundle,
            ActionKind::OpenApp {
                bundle_id: bundle.into(),
            },
        )];
        {
            assignments.push((
                1,
                "desktop-input",
                "聚焦当前输入框",
                "Agent",
                bundle,
                ActionKind::AgentCommand {
                    adapter: adapter.into(),
                    command: "focus-input".into(),
                },
            ));
        }
        if ["cindy", "zcode", "codex", "qoder-cn"].contains(&adapter) {
            assignments.push((
                2,
                "desktop-search",
                "搜索任务",
                "Agent",
                bundle,
                ActionKind::AgentCommand {
                    adapter: adapter.into(),
                    command: "search".into(),
                },
            ));
        }
        if adapter == "qoder-cn" {
            assignments.push((
                3,
                "desktop-new",
                "新建任务",
                "Agent",
                bundle,
                ActionKind::AgentCommand {
                    adapter: adapter.into(),
                    command: "new-chat".into(),
                },
            ));
        }
        if adapter == "qwenwork" || adapter == "zcode" {
            assignments.push((
                3,
                "desktop-new",
                "新建任务",
                "快捷键",
                bundle,
                ActionKind::Shortcut {
                    key: "n".into(),
                    modifiers: vec!["command".into()],
                    target: bundle.into(),
                },
            ));
        }
        if adapter == "qwenwork" {
            assignments.push((
                2,
                "desktop-search",
                "搜索全部任务",
                "快捷键",
                bundle,
                ActionKind::Shortcut {
                    key: "g".into(),
                    modifiers: vec!["command".into()],
                    target: bundle.into(),
                },
            ));
        }
        specs.push((name, assignments));
    }

    legacy_specs.extend(specs.clone());
    for (name, assignments) in &mut specs {
        let adapter = match *name {
            "Cindy" => "cindy",
            "Codex" => "codex",
            "千问办公" => "qwenwork",
            "ZCode" => "zcode",
            "Antigravity" => "antigravity",
            "DeepSeek Coder (DSH)" => "dsh-desktop",
            "Qoder CN" => "qoder-cn",
            _ => continue,
        };
        for (slot, id, label, command) in [
            (4, "desktop-model", "选择模型", "model"),
            (5, "desktop-effort", "调整思考力度", "effort"),
            (6, "desktop-dictation", "听写 / 语音输入", "dictation"),
            (7, "desktop-browser", "打开内置浏览器", "browser"),
            (8, "desktop-terminal", "打开终端面板", "terminal"),
        ] {
            let kind = if adapter == "codex"
                && matches!(command, "model" | "effort" | "browser" | "terminal")
            {
                let (key, modifiers) = match command {
                    "model" | "effort" => ("M", vec!["control".into(), "shift".into()]),
                    "browser" => ("T", vec!["command".into()]),
                    _ => ("`", vec!["control".into()]),
                };
                ActionKind::Shortcut {
                    key: key.into(),
                    modifiers,
                    target: "com.openai.codex".into(),
                }
            } else if adapter == "dsh-desktop" && command == "terminal" {
                ActionKind::Shortcut {
                    key: "`".into(),
                    modifiers: vec!["control".into()],
                    target: "com.deepseek.dsh".into(),
                }
            } else {
                ActionKind::AgentCommand {
                    adapter: adapter.into(),
                    command: command.into(),
                }
            };
            assignments.push((
                slot,
                id,
                label,
                "Agent",
                "目标应用的原生控件；未提供时明确提示",
                kind,
            ));
        }
    }
    let herdr = &mut specs
        .iter_mut()
        .find(|(name, _)| *name == "Herdr")
        .unwrap()
        .1;
    herdr.clear();
    herdr.push((
        0,
        "desktop-open",
        "打开 Herdr",
        "应用",
        "dev.herdr.app",
        ActionKind::OpenApp {
            bundle_id: "dev.herdr.app".into(),
        },
    ));
    for (slot, label, key, modifiers) in [
        (1, "选择 Space / Tab", "p", vec!["command"]),
        (2, "搜索命令", "p", vec!["command", "shift"]),
        (3, "新建 Space", "n", vec!["command"]),
        (4, "新建 Tab / 终端", "t", vec!["command"]),
        (5, "向右分屏", "d", vec!["command"]),
        (6, "向下分屏", "d", vec!["command", "shift"]),
        (7, "上一个 Space", "[", vec!["command", "option"]),
        (8, "下一个 Space", "]", vec!["command", "option"]),
        (9, "上一个 Tab", "Tab", vec!["control", "shift"]),
        (10, "下一个 Tab", "Tab", vec!["control"]),
        (12, "放大 / 还原面板", "Return", vec!["command", "shift"]),
        (15, "显示 / 隐藏侧栏", "b", vec!["command"]),
        (16, "聚焦上方面板", "Up", vec!["command", "option"]),
        (17, "聚焦左方面板", "Left", vec!["command", "option"]),
        (18, "聚焦下方面板", "Down", vec!["command", "option"]),
        (19, "聚焦右方面板", "Right", vec!["command", "option"]),
    ] {
        herdr.push((
            slot,
            "herdr-desktop",
            label,
            "快捷键",
            "dev.herdr.app",
            ActionKind::Shortcut {
                key: key.into(),
                modifiers: modifiers.into_iter().map(str::to_owned).collect(),
                target: "dev.herdr.app".into(),
            },
        ));
    }

    let mut work = draft.clone();
    // Retire only unchanged source profiles from the exact bundled fixture, even
    // when other profiles have since been added. Preserve edited and imported data.
    let seed = crate::demo().map_err(|e| format!("{e:?}"))?;
    if draft.document.origin["kind"] == "synthetic-local"
        && draft.document.origin["config"]
            == serde_json::to_value(&seed).map_err(|e| e.to_string())?
    {
        for mut current in draft.config.profiles.clone() {
            let Some(pid) = current.id.as_str().map(str::to_owned) else {
                continue;
            };
            let Some(source) = draft.document.editable.source_refs.get(&pid) else {
                continue;
            };
            if source["kind"] != "source-profile" {
                continue;
            }
            let Some(original) = seed.profiles.iter().find(|p| p.id == source["source_id"]) else {
                continue;
            };
            let node = &draft.document.editable.graph.profiles[&pid];
            if node.layer_order.iter().any(|lid| {
                let l = &draft.document.editable.graph.layers[lid];
                l.control_settings.is_some() || l.controls.values().any(|b| b.action_id.is_some())
            }) {
                continue;
            }
            current.id = original.id.clone();
            current.index = original.index;
            for (layer, lid) in current.layers.iter_mut().zip(&node.layer_order) {
                layer.id = draft
                    .document
                    .editable
                    .source_refs
                    .get(lid)
                    .map(|r| r["source_id"].clone())
                    .unwrap_or(Value::Null);
            }
            if serde_json::to_value(current).ok() == serde_json::to_value(original).ok() {
                work.delete_profile(&pid)?;
            }
        }
    }
    for (profile_name, assignments) in specs {
        let existing_id = work
            .document
            .editable
            .graph
            .profiles
            .iter()
            .find(|(_, p)| p.name == profile_name)
            .map(|(id, _)| id.clone());
        let pid = if let Some(pid) = existing_id {
            let p = &work.document.editable.graph.profiles[&pid];
            let can_upgrade = p.layer_order.len() == 1
                && legacy_specs
                    .iter()
                    .filter(|(name, _)| *name == profile_name)
                    .any(|(_, old)| {
                        let layer = &work.document.editable.graph.layers[&p.layer_order[0]];
                        layer.control_settings.as_ref().is_some_and(|settings| {
                            settings.joystick_mode == "logical_sectors"
                                && settings.active_sectors == 4
                        }) && layer
                            .controls
                            .values()
                            .filter(|b| b.action_id.is_some())
                            .count()
                            == old.len()
                            && old.iter().all(|(slot, id, name, category, detail, kind)| {
                                layer
                                    .controls
                                    .values()
                                    .find(|b| b.logical_control == format!("logical-{slot}"))
                                    .and_then(|b| b.action_id.as_ref())
                                    .and_then(|id| work.document.editable.graph.actions.get(id))
                                    .is_some_and(|a| {
                                        a.id == *id
                                            && a.name == *name
                                            && a.category == *category
                                            && a.detail == *detail
                                            && serde_json::to_value(kind).ok().as_ref()
                                                == Some(&a.payload)
                                    })
                            })
                    });
            if !can_upgrade {
                created_ids.push(pid);
                continue;
            }
            let lid = p.layer_order[0].clone();
            work.mutate(|doc| {
                let mut removed = std::collections::BTreeSet::new();
                for binding in doc
                    .editable
                    .graph
                    .layers
                    .get_mut(&lid)
                    .unwrap()
                    .controls
                    .values_mut()
                {
                    if let Some(id) = binding.action_id.take() {
                        removed.insert(id);
                    }
                }
                let used: std::collections::BTreeSet<_> = doc
                    .editable
                    .graph
                    .layers
                    .values()
                    .flat_map(|l| l.controls.values().filter_map(|b| b.action_id.clone()))
                    .collect();
                for id in removed {
                    if !used.contains(&id) && crate::multi::parent_references(doc, &id).is_empty() {
                        doc.editable.graph.actions.remove(&id);
                        doc.editable.source_refs.remove(&id);
                    }
                }
                Ok(())
            })?;
            pid
        } else {
            work.create_profile(profile_name)?
        };
        let lid = work.document.editable.graph.profiles[&pid].layer_order[0].clone();
        work.set_control_settings(
            &pid,
            &lid,
            crate::control_settings::ControlSettings {
                joystick_mode: "logical_sectors".into(),
                active_sectors: 4,
            },
        )?;
        for (slot, id, name, category, detail, kind) in assignments {
            let action = crate::draft::Action {
                id: id.into(),
                name: name.into(),
                category: category.into(),
                detail: detail.into(),
                payload: serde_json::to_value(kind).map_err(|e| e.to_string())?,
            };
            work.assign(&json!(pid), &json!(lid), slot, action)?;
        }
        created_ids.push(pid);
    }
    draft.mutate(|doc| {
        doc.editable = work.document.editable;
        Ok(created_ids)
    })
}

#[cfg(test)]
mod agent_migration_tests {
    use super::*;
    #[test]
    fn upgrades_only_unchanged_legacy_agents_atomically() {
        for customized in [false, true] {
            let mut draft = Draft::default();
            let mut old_ids = Vec::new();
            for (name, assignments) in legacy_agent_specs() {
                let pid = draft.create_profile(name).unwrap();
                let lid = draft.document.editable.graph.profiles[&pid].layer_order[0].clone();
                draft
                    .set_control_settings(
                        &pid,
                        &lid,
                        crate::control_settings::ControlSettings {
                            joystick_mode: "logical_sectors".into(),
                            active_sectors: 4,
                        },
                    )
                    .unwrap();
                for (slot, id, name, category, detail, kind) in assignments {
                    draft
                        .assign(
                            &json!(pid),
                            &json!(lid),
                            slot,
                            crate::draft::Action {
                                id: id.into(),
                                name: name.into(),
                                category: category.into(),
                                detail: detail.into(),
                                payload: serde_json::to_value(kind).unwrap(),
                            },
                        )
                        .unwrap();
                }
                old_ids.push(pid);
            }
            let ag = old_ids[0].clone();
            let lid = draft.document.editable.graph.profiles[&ag].layer_order[0].clone();
            if customized {
                let id = draft.document.editable.graph.layers[&lid]
                    .controls
                    .values()
                    .find_map(|b| b.action_id.clone())
                    .unwrap();
                draft
                    .document
                    .editable
                    .graph
                    .actions
                    .get_mut(&id)
                    .unwrap()
                    .name = "My action".into();
            }
            let before = draft.document.editable.clone();
            let revision = draft.revision;
            let ids = install_agent_starter_profiles(&mut draft).unwrap();
            assert_eq!(&ids[..3], &old_ids);
            assert_eq!(draft.revision, revision + 1);
            let count = draft.document.editable.graph.layers[&lid]
                .controls
                .values()
                .filter(|b| b.action_id.is_some())
                .count();
            assert_eq!(
                count,
                if customized {
                    legacy_agent_specs()[0].1.len()
                } else {
                    9
                }
            );
            if customized {
                assert!(draft.document.editable.graph.layers[&lid] == before.graph.layers[&lid]);
            }
            let after = serde_json::to_value(&draft.document.editable).unwrap();
            install_agent_starter_profiles(&mut draft).unwrap();
            assert_eq!(
                serde_json::to_value(&draft.document.editable).unwrap(),
                after
            );
            draft.undo(false).unwrap();
            assert_eq!(
                serde_json::to_value(&draft.document.editable).unwrap(),
                serde_json::to_value(before).unwrap()
            );
        }
    }
}
