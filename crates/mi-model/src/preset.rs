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
            if doc.is_protected_oai_layer(lid) {
                return Err("受保护宿主层（ChatGPT / Codex / Cindy）不支持导出".into());
            }
            name = doc.editable.graph.layers[lid].name.clone();
            scope = "layer";
            [lid.to_owned()].into()
        } else {
            let non_protected: BTreeSet<_> = profile
                .layer_order
                .iter()
                .filter(|id| !doc.is_protected_oai_layer(id))
                .cloned()
                .collect();
            if non_protected.is_empty() {
                return Err("仅含受保护宿主层的配置不支持导出".into());
            }
            non_protected
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
        if ["cindy", "zcode", "codex"].contains(&adapter) {
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
    legacy_specs.extend(
        specs
            .iter()
            .filter(|(name, _)| matches!(*name, "Cindy" | "Codex" | "千问办公" | "Antigravity"))
            .cloned(),
    );
    for (_, assignments) in &mut specs {
        for (_, _, label, _, _, kind) in assignments {
            if let ActionKind::AgentCommand { adapter, command } = kind {
                if command == "dictation"
                    && matches!(
                        adapter.as_str(),
                        "cindy" | "codex" | "qwenwork" | "antigravity" | "zcode" | "dsh-desktop"
                    )
                {
                    *command = "push-to-talk".into();
                    *label = "按住说话 · 松开停止";
                }
            }
        }
    }
    legacy_specs.extend(
        specs
            .iter()
            .filter(|(name, _)| {
                matches!(
                    *name,
                    "Cindy"
                        | "Codex"
                        | "千问办公"
                        | "ZCode"
                        | "Antigravity"
                        | "DeepSeek Coder (DSH)"
                )
            })
            .cloned(),
    );
    for (name, assignments) in &mut specs {
        let adapter = match *name {
            "Cindy" => "cindy",
            "Codex" => "codex",
            "千问办公" => "qwenwork",
            "ZCode" => "zcode",
            "Antigravity" => "antigravity",
            "DeepSeek Coder (DSH)" => "dsh-desktop",
            _ => continue,
        };
        for (slot, id, label, command) in [
            (13, "desktop-scroll-up", "向上滚动对话", "scroll-up"),
            (14, "desktop-scroll-down", "向下滚动对话", "scroll-down"),
            (
                15,
                "desktop-scroll-bottom",
                "跳到底部并聚焦输入",
                "scroll-to-bottom",
            ),
        ] {
            assignments.push((
                slot,
                id,
                label,
                "Agent",
                "目标应用的原生对话滚动与输入框定位；不抢占非目标窗口焦点",
                ActionKind::AgentCommand {
                    adapter: adapter.into(),
                    command: command.into(),
                },
            ));
        }
    }
    legacy_specs.extend(
        specs
            .iter()
            .filter(|(name, _)| *name == "Antigravity")
            .cloned(),
    );
    let antigravity = &mut specs
        .iter_mut()
        .find(|(name, _)| *name == "Antigravity")
        .unwrap()
        .1;
    antigravity.clear();
    antigravity.extend([
        (
            0,
            "desktop-open",
            "打开 / 聚焦应用",
            "应用",
            "com.google.antigravity",
            ActionKind::OpenApp {
                bundle_id: "com.google.antigravity".into(),
            },
        ),
        (
            1,
            "desktop-input",
            "聚焦当前输入框",
            "Agent",
            "com.google.antigravity",
            ActionKind::AgentCommand {
                adapter: "antigravity".into(),
                command: "focus-input".into(),
            },
        ),
        (
            2,
            "desktop-new",
            "新建会话",
            "Agent",
            "com.google.antigravity",
            ActionKind::AgentCommand {
                adapter: "antigravity".into(),
                command: "new-chat".into(),
            },
        ),
        (
            3,
            "desktop-project",
            "选项目 / 新建项目",
            "Agent",
            "切换侧栏项目或打开新项目目录（Cmd+;）",
            ActionKind::AgentCommand {
                adapter: "antigravity".into(),
                command: "project".into(),
            },
        ),
        (
            4,
            "desktop-model",
            "选择模型",
            "Agent",
            "打开模型与思考档位选择器（Cmd+/ 或原生控件）",
            ActionKind::AgentCommand {
                adapter: "antigravity".into(),
                command: "model".into(),
            },
        ),
        (
            5,
            "desktop-workspace",
            "项目工作区快切",
            "快捷键",
            "com.google.antigravity",
            ActionKind::Shortcut {
                key: "K".into(),
                modifiers: vec!["command".into(), "shift".into()],
                target: "com.google.antigravity".into(),
            },
        ),
        (
            6,
            "desktop-live-voice",
            "Gemini Live 实时语音",
            "Agent",
            "启动或结束 Antigravity Live 实时语音对话并确认状态",
            ActionKind::AgentCommand {
                adapter: "antigravity".into(),
                command: "live-voice".into(),
            },
        ),
        (
            7,
            "desktop-browser",
            "打开内置浏览器",
            "Agent",
            "切换右侧内置浏览器与预览面板（Cmd+Shift+B 或原生控件）",
            ActionKind::AgentCommand {
                adapter: "antigravity".into(),
                command: "browser".into(),
            },
        ),
        (
            8,
            "desktop-terminal",
            "打开终端面板",
            "Agent",
            "切换底部终端面板（Cmd+J 或原生控件）",
            ActionKind::AgentCommand {
                adapter: "antigravity".into(),
                command: "terminal".into(),
            },
        ),
        (
            9,
            "desktop-split",
            "快速向右分栏",
            "Agent",
            "将当前会话向右分栏（Cmd+D）",
            ActionKind::AgentCommand {
                adapter: "antigravity".into(),
                command: "split-right".into(),
            },
        ),
        (
            10,
            "desktop-dictation",
            "按住说话 · 松开停止",
            "Agent",
            "2U 宽键按住启动听写（Record Audio / Ctrl+M），松开自动停止",
            ActionKind::AgentCommand {
                adapter: "antigravity".into(),
                command: "push-to-talk".into(),
            },
        ),
        (
            11,
            "desktop-dictation-r",
            "按住说话 · 松开停止",
            "Agent",
            "2U 宽键按住启动听写（Record Audio / Ctrl+M），松开自动停止",
            ActionKind::AgentCommand {
                adapter: "antigravity".into(),
                command: "push-to-talk".into(),
            },
        ),
        (
            12,
            "desktop-submit",
            "发送消息（先停听写）",
            "提交控制",
            "若听写激活则先停并确认停止，再聚焦输入区回车提交",
            ActionKind::AgentCommand {
                adapter: "antigravity".into(),
                command: "submit".into(),
            },
        ),
        (
            13,
            "desktop-prev-chat",
            "上一会话（侧栏轮转）",
            "Agent",
            "在左侧栏会话列表中向上切换一条会话（Alt+Up）",
            ActionKind::AgentCommand {
                adapter: "antigravity".into(),
                command: "prev-conversation".into(),
            },
        ),
        (
            14,
            "desktop-next-chat",
            "下一会话（侧栏轮转）",
            "Agent",
            "在左侧栏会话列表中向下切换一条会话（Alt+Down）",
            ActionKind::AgentCommand {
                adapter: "antigravity".into(),
                command: "next-conversation".into(),
            },
        ),
        (
            15,
            "desktop-search",
            "搜索会话",
            "Agent",
            "打开会话搜索面板（Search Conversations / Cmd+K）",
            ActionKind::AgentCommand {
                adapter: "antigravity".into(),
                command: "search".into(),
            },
        ),
        (
            16,
            "desktop-scroll-up",
            "向上滚动对话",
            "Agent",
            "目标应用的原生对话滚动与输入框定位；不抢占非目标窗口焦点",
            ActionKind::AgentCommand {
                adapter: "antigravity".into(),
                command: "scroll-up".into(),
            },
        ),
        (
            17,
            "desktop-nav-back",
            "历史后退",
            "快捷键",
            "com.google.antigravity",
            ActionKind::Shortcut {
                key: "[".into(),
                modifiers: vec!["command".into()],
                target: "com.google.antigravity".into(),
            },
        ),
        (
            18,
            "desktop-scroll-down",
            "向下滚动对话",
            "Agent",
            "目标应用的原生对话滚动与输入框定位；不抢占非目标窗口焦点",
            ActionKind::AgentCommand {
                adapter: "antigravity".into(),
                command: "scroll-down".into(),
            },
        ),
        (
            19,
            "desktop-nav-forward",
            "历史前进",
            "快捷键",
            "com.google.antigravity",
            ActionKind::Shortcut {
                key: "]".into(),
                modifiers: vec!["command".into()],
                target: "com.google.antigravity".into(),
            },
        ),
    ]);
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
    let mut seed = crate::demo().map_err(|e| format!("{e:?}"))?;
    if let Some(read_at) = draft.document.origin["config"]["read_at"].as_u64() {
        seed.read_at = read_at;
    }
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
                l.lighting.is_some()
                    || l.control_settings.is_some()
                    || l.controls.values().any(|b| b.action_id.is_some())
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
    let retired_qoder_ids: Vec<String> = work
        .document
        .editable
        .graph
        .profiles
        .iter()
        .filter_map(|(pid, p)| {
            if p.name != "Qoder CN" || p.layer_order.len() != 1 {
                return None;
            }
            let layer = work.document.editable.graph.layers.get(&p.layer_order[0])?;
            let all_qoder = layer
                .controls
                .values()
                .filter_map(|b| b.action_id.as_ref())
                .all(|aid| {
                    work.document
                        .editable
                        .graph
                        .actions
                        .get(aid)
                        .is_some_and(|a| {
                            a.payload.get("bundle_id").and_then(Value::as_str)
                                == Some("com.qodercn.app")
                                || a.payload.get("adapter").and_then(Value::as_str)
                                    == Some("qoder-cn")
                        })
                });
            all_qoder.then(|| pid.clone())
        })
        .collect();
    for pid in retired_qoder_ids {
        work.delete_profile(&pid)?;
    }
    for (profile_name, assignments) in specs {
        let is_native_oai_profile = matches!(profile_name, "Cindy" | "Codex");
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
            if is_native_oai_profile
                && p.layer_order.len() == 1
                && work.document.is_protected_oai_layer(&p.layer_order[0])
            {
                created_ids.push(pid);
                continue;
            }
            let can_upgrade = p.layer_order.len() == 1
                && legacy_specs
                    .iter()
                    .filter(|(name, _)| *name == profile_name)
                    .any(|(_, old)| {
                        let layer = &work.document.editable.graph.layers[&p.layer_order[0]];
                        layer.lighting.is_none()
                            && layer.control_settings.as_ref().is_some_and(|settings| {
                                settings.joystick_mode == "logical_sectors"
                                    && settings.active_sectors == 4
                            })
                            && layer
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
        if is_native_oai_profile {
            work.mutate(|doc| {
                let layer = doc
                    .editable
                    .graph
                    .layers
                    .get_mut(&lid)
                    .ok_or_else(|| "层不存在".to_owned())?;
                layer.name = "ChatGPT".into();
                layer.lighting = None;
                layer.control_settings = None;
                let old_control_ids = std::mem::take(&mut layer.control_order);
                for cid in old_control_ids {
                    doc.editable.source_refs.remove(&cid);
                }
                layer.controls.clear();
                for (slot, token) in crate::OFFICIAL_OAI_KEY_TOKENS
                    .iter()
                    .chain(crate::OFFICIAL_OAI_ENCODER_TOKENS.iter())
                    .enumerate()
                {
                    let bid = crate::document::new_id();
                    layer.control_order.push(bid.clone());
                    layer.controls.insert(
                        bid,
                        crate::document::BindingNode {
                            logical_control: format!("logical-{slot}"),
                            original: serde_json::to_value(crate::Binding::Unknown(Value::String(
                                (*token).into(),
                            )))
                            .map_err(|e| e.to_string())?,
                            action_id: None,
                        },
                    );
                }
                Ok(())
            })?;
            created_ids.push(pid);
            continue;
        }
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

pub fn install_four_app_cockpit_preset(
    draft: &mut Draft,
    profile_id: &str,
) -> Result<(String, String), String> {
    let mut work = draft.clone();
    let pid = if profile_id.trim().is_empty() {
        work.document
            .editable
            .graph
            .profile_order
            .first()
            .cloned()
            .ok_or_else(|| "不存在可用的 Profile".to_owned())?
    } else if work
        .document
        .editable
        .graph
        .profiles
        .contains_key(profile_id)
    {
        profile_id.to_owned()
    } else {
        return Err("指定的 Profile 不存在".into());
    };
    let profile_name = work.document.editable.graph.profiles[&pid].name.clone();
    let adapter = match profile_name.as_str() {
        "Antigravity" => "antigravity",
        "DeepSeek Coder (DSH)" => "dsh-desktop",
        "ZCode" => "zcode",
        "Cindy" => "cindy",
        "Codex" => "codex",
        "千问办公" => "qwenwork",
        _ => "antigravity",
    };
    let existing_layer = work.document.editable.graph.profiles[&pid]
        .layer_order
        .iter()
        .find(|lid| {
            work.document
                .editable
                .graph
                .layers
                .get(*lid)
                .is_some_and(|l| l.name == "四应用总控")
        })
        .cloned();
    let lid = match existing_layer {
        Some(lid) => lid,
        None => {
            let first_lid = work.document.editable.graph.profiles[&pid].layer_order[0].clone();
            if work.document.is_protected_oai_layer(&first_lid) {
                work.create_layer(&pid, "四应用总控")?
            } else {
                first_lid
            }
        }
    };
    work.set_control_settings(
        &pid,
        &lid,
        crate::control_settings::ControlSettings {
            joystick_mode: "logical_sectors".into(),
            active_sectors: 4,
        },
    )?;
    let assignments = [
        (
            0u32,
            "cockpit-slot-0",
            "四应用槽位 1 · AG00",
            "四应用总控",
            "直跳或长按绑定第 1 个跨应用活跃会话槽位",
            ActionKind::AgentCommand {
                adapter: "four-app".into(),
                command: "slot-open:0".into(),
            },
        ),
        (
            1,
            "cockpit-slot-1",
            "四应用槽位 2 · AG01",
            "四应用总控",
            "直跳或长按绑定第 2 个跨应用活跃会话槽位",
            ActionKind::AgentCommand {
                adapter: "four-app".into(),
                command: "slot-open:1".into(),
            },
        ),
        (
            2,
            "cockpit-slot-2",
            "四应用槽位 3 · AG02",
            "四应用总控",
            "直跳或长按绑定第 3 个跨应用活跃会话槽位",
            ActionKind::AgentCommand {
                adapter: "four-app".into(),
                command: "slot-open:2".into(),
            },
        ),
        (
            3,
            "cockpit-slot-3",
            "四应用槽位 4 · AG03",
            "四应用总控",
            "直跳或长按绑定第 4 个跨应用活跃会话槽位",
            ActionKind::AgentCommand {
                adapter: "four-app".into(),
                command: "slot-open:3".into(),
            },
        ),
        (
            4,
            "cockpit-slot-4",
            "四应用槽位 5 · AG04",
            "四应用总控",
            "直跳或长按绑定第 5 个跨应用活跃会话槽位",
            ActionKind::AgentCommand {
                adapter: "four-app".into(),
                command: "slot-open:4".into(),
            },
        ),
        (
            5,
            "cockpit-slot-5",
            "四应用槽位 6 · AG05",
            "四应用总控",
            "直跳或长按绑定第 6 个跨应用活跃会话槽位",
            ActionKind::AgentCommand {
                adapter: "four-app".into(),
                command: "slot-open:5".into(),
            },
        ),
        (
            6,
            "cockpit-attention",
            "四应用全局待办跳转 · AG06",
            "四应用总控",
            "一键切到最高优先级待关注会话",
            ActionKind::AgentCommand {
                adapter: "four-app".into(),
                command: "four-app-attention".into(),
            },
        ),
        (
            7,
            "cockpit-approve",
            "审批通过 · AG07",
            "四应用总控",
            "批准当前或最高优先级待决交互请求",
            ActionKind::AgentCommand {
                adapter: "host".into(),
                command: "approve".into(),
            },
        ),
        (
            8,
            "cockpit-deny",
            "审批拒绝 · AG08",
            "四应用总控",
            "拒绝当前或最高优先级待决交互请求",
            ActionKind::AgentCommand {
                adapter: "host".into(),
                command: "deny".into(),
            },
        ),
        (
            9,
            "cockpit-new-or-stop",
            "新建或停止会话 · AG09",
            "四应用总控",
            "运行中则停止当前生成，空闲或冷启动则新建会话",
            ActionKind::AgentCommand {
                adapter: "host".into(),
                command: "new-or-stop".into(),
            },
        ),
        (
            10,
            "cockpit-ptt-left",
            "按住语音 · AG10 (2U 左)",
            "四应用总控",
            "2U 宽键按住说话，松开自动停止听写",
            ActionKind::AgentCommand {
                adapter: adapter.into(),
                command: "push-to-talk".into(),
            },
        ),
        (
            11,
            "cockpit-ptt-right",
            "按住语音 · AG11 (2U 右)",
            "四应用总控",
            "2U 宽键按住说话，松开自动停止听写",
            ActionKind::AgentCommand {
                adapter: adapter.into(),
                command: "push-to-talk".into(),
            },
        ),
        (
            12,
            "cockpit-submit",
            "停止听写并提交 · AG12",
            "四应用总控",
            "先停活跃听写再聚焦输入区回车提交",
            ActionKind::AgentCommand {
                adapter: "host".into(),
                command: "submit".into(),
            },
        ),
        (
            13,
            "cockpit-dial-ccw",
            "旋钮逆时针 · 向上滚屏",
            "四应用总控",
            "平滑向上滚动当前对话消息区",
            ActionKind::AgentCommand {
                adapter: adapter.into(),
                command: "scroll-up".into(),
            },
        ),
        (
            14,
            "cockpit-dial-cw",
            "旋钮顺时针 · 向下滚屏",
            "四应用总控",
            "平滑向下滚动当前对话消息区",
            ActionKind::AgentCommand {
                adapter: adapter.into(),
                command: "scroll-down".into(),
            },
        ),
        (
            15,
            "cockpit-dial-click",
            "旋钮按压 · 跳到底部",
            "四应用总控",
            "一键跳回当前对话流最底部",
            ActionKind::AgentCommand {
                adapter: adapter.into(),
                command: "scroll-to-bottom".into(),
            },
        ),
    ];
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
    draft.mutate(|doc| {
        doc.editable = work.document.editable;
        Ok((pid, lid))
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
                    20
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

    #[test]
    fn upgrades_pre_dial_conversational_presets_while_preserving_customized_profiles() {
        let mut draft = Draft::default();
        let ids = install_agent_starter_profiles(&mut draft).unwrap();
        assert_eq!(ids.len(), 7);

        for native_name in ["Cindy", "Codex"] {
            let pid = draft
                .document
                .editable
                .graph
                .profiles
                .iter()
                .find(|(_, p)| p.name == native_name)
                .unwrap()
                .0
                .clone();
            let lid = draft.document.editable.graph.profiles[&pid].layer_order[0].clone();
            assert!(
                draft.document.is_protected_oai_layer(&lid),
                "{native_name} default layer must be a protected KV_OAI_* layer"
            );
        }

        // Downgrade DeepSeek Coder (DSH) and 千问办公 to pre-Slice-B 9-slot push-to-talk state (remove slots 13, 14, 15),
        // and customize ZCode by adding a joystick prompt slot (slot 16 -> prompt-continue).
        for profile_name in ["DeepSeek Coder (DSH)", "千问办公"] {
            let pid = draft
                .document
                .editable
                .graph
                .profiles
                .iter()
                .find(|(_, p)| p.name == profile_name)
                .unwrap()
                .0
                .clone();
            let lid = draft.document.editable.graph.profiles[&pid].layer_order[0].clone();
            draft
                .mutate(|doc| {
                    let layer = doc.editable.graph.layers.get_mut(&lid).unwrap();
                    let mut removed = std::collections::BTreeSet::new();
                    for slot in [13, 14, 15] {
                        if let Some(aid) = layer
                            .controls
                            .values_mut()
                            .find(|b| b.logical_control == format!("logical-{slot}"))
                            .and_then(|b| b.action_id.take())
                        {
                            removed.insert(aid);
                        }
                    }
                    for aid in removed {
                        doc.editable.graph.actions.remove(&aid);
                    }
                    Ok(())
                })
                .unwrap();
        }

        let zcode_pid = draft
            .document
            .editable
            .graph
            .profiles
            .iter()
            .find(|(_, p)| p.name == "ZCode")
            .unwrap()
            .0
            .clone();
        let zcode_lid = draft.document.editable.graph.profiles[&zcode_pid].layer_order[0].clone();
        let prompt_continue = crate::draft::actions()
            .into_iter()
            .find(|a| a.id == "prompt-continue")
            .unwrap();
        draft
            .assign(&json!(zcode_pid), &json!(zcode_lid), 16, prompt_continue)
            .unwrap();
        let zcode_before = draft.document.editable.graph.layers[&zcode_lid].clone();

        let upgraded_ids = install_agent_starter_profiles(&mut draft).unwrap();
        assert_eq!(
            upgraded_ids, ids,
            "Profile UUIDs must be preserved on upgrade"
        );

        for profile_name in ["Antigravity", "DeepSeek Coder (DSH)", "千问办公"] {
            let pid = draft
                .document
                .editable
                .graph
                .profiles
                .iter()
                .find(|(_, p)| p.name == profile_name)
                .unwrap()
                .0
                .clone();
            assert!(
                crate::host_router::resolve_host_action(&draft.document, Some(&pid), "AG13", true)
                    .is_some(),
                "{profile_name} AG13 dial action must be present after lossless upgrade"
            );
            assert!(
                crate::host_router::resolve_host_action(
                    &draft.document,
                    Some(&pid),
                    "ENC_CLK",
                    true
                )
                .is_some(),
                "{profile_name} ENC_CLK action must be present after lossless upgrade"
            );
        }

        assert!(
            draft.document.editable.graph.layers[&zcode_lid] == zcode_before,
            "User-customized ZCode profile (with joystick prompt) must remain untouched"
        );
    }
}
