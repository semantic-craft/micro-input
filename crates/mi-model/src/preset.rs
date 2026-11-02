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
