//! R-E09: local identities and editable ownership graph; never a device payload.
use crate::{
    draft::{binding_key, Action, LegacyDocumentV1},
    *,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct ProfileNode {
    pub name: String,
    pub layer_order: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct LayerNode {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_settings: Option<crate::control_settings::ControlSettings>,
    pub name: String,
    pub control_order: Vec<String>,
    pub controls: BTreeMap<String, BindingNode>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct BindingNode {
    pub logical_control: String,
    pub original: Value,
    pub action_id: Option<String>,
}
#[derive(Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Graph {
    pub profile_order: Vec<String>,
    pub profiles: BTreeMap<String, ProfileNode>,
    pub layers: BTreeMap<String, LayerNode>,
    pub actions: BTreeMap<String, Action>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct EditableState {
    pub graph: Graph,
    pub source_refs: BTreeMap<String, Value>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct LocalDocumentV2 {
    pub schema_version: u32,
    pub document_id: String,
    pub revision: u64,
    pub baseline: Option<Value>,
    pub origin: Value,
    #[serde(flatten)]
    pub editable: EditableState,
}
fn name_valid(name: &str) -> Result<(), String> {
    if name.trim().is_empty() || name.len() > 256 {
        Err("名称应为 1–256 字节，且不能全为空白".into())
    } else {
        Ok(())
    }
}
fn exact_order(order: &[String], keys: impl Iterator<Item = String>) -> bool {
    let expected: BTreeSet<_> = keys.collect();
    order.len() == expected.len() && order.iter().cloned().collect::<BTreeSet<_>>() == expected
}
impl LocalDocumentV2 {
    pub fn from_legacy(legacy: &LegacyDocumentV1, digest: Option<String>) -> Result<Self, String> {
        let config = &legacy.config;
        let mut doc = Self::from_config(config, None, "legacy-local")?;
        doc.revision = legacy.revision;
        doc.origin["legacy_revision"] = json!(legacy.revision);
        doc.origin["legacy_document_sha256"] = json!(digest);
        for (pi, p) in config.profiles.iter().enumerate() {
            let pid = doc.editable.graph.profile_order[pi].clone();
            for (li, l) in p.layers.iter().enumerate() {
                let lid = doc.editable.graph.profiles[&pid].layer_order[li].clone();
                let layer = doc.editable.graph.layers.get_mut(&lid).unwrap();
                for (slot, bid) in layer.control_order.clone().iter().enumerate() {
                    let key = binding_key(&p.id, &l.id, slot as u32);
                    if let Some(action) = legacy.edits.get(&key) {
                        let aid = new_id();
                        doc.editable
                            .graph
                            .actions
                            .insert(aid.clone(), action.clone());
                        layer.controls.get_mut(bid).unwrap().action_id = Some(aid.clone());
                        doc.editable
                            .source_refs
                            .insert(aid, json!({"kind":"legacy-edit","binding_key":key}));
                    }
                }
            }
        }
        doc.validate()?;
        Ok(doc)
    }
    pub fn from_config(
        config: &DeviceConfig,
        baseline: Option<Value>,
        kind: &str,
    ) -> Result<Self, String> {
        // The established v1 identity check rejects ambiguous source IDs. Never infer from names/indexes.
        LegacyDocumentV1::new(config.clone())?;
        let mut doc = Self {
            schema_version: 2,
            document_id: new_id(),
            revision: 0,
            baseline,
            origin: json!({"kind":kind,"config":config}),
            editable: EditableState {
                graph: Graph::default(),
                source_refs: BTreeMap::new(),
            },
        };
        for p in &config.profiles {
            let pid = new_id();
            let mut profile = ProfileNode {
                name: p.name.clone(),
                layer_order: vec![],
            };
            doc.editable.source_refs.insert(
                pid.clone(),
                json!({"kind":"source-profile","source_id":p.id}),
            );
            for l in &p.layers {
                let lid = new_id();
                let mut layer = LayerNode {
                    control_settings: None,
                    name: l.name.clone(),
                    control_order: vec![],
                    controls: BTreeMap::new(),
                };
                doc.editable.source_refs.insert(
                    lid.clone(),
                    json!({"kind":"source-layer","profile_source_id":p.id,"source_id":l.id}),
                );
                for (slot, original) in l
                    .keys
                    .iter()
                    .chain(l.encoders.iter())
                    .chain(l.joystick.sectors.iter())
                    .enumerate()
                {
                    let bid = new_id();
                    layer.control_order.push(bid.clone());
                    layer.controls.insert(
                        bid.clone(),
                        BindingNode {
                            logical_control: format!("logical-{slot}"),
                            original: serde_json::to_value(original).map_err(|e| e.to_string())?,
                            action_id: None,
                        },
                    );
                    doc.editable.source_refs.insert(bid,json!({"kind":"source-binding","profile_source_id":p.id,"layer_source_id":l.id,"logical_control":format!("logical-{slot}")}));
                }
                profile.layer_order.push(lid.clone());
                doc.editable.graph.layers.insert(lid, layer);
            }
            doc.editable.graph.profile_order.push(pid.clone());
            doc.editable.graph.profiles.insert(pid, profile);
        }
        doc.validate()?;
        Ok(doc)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 2 || Uuid::parse_str(&self.document_id).is_err() {
            return Err("本地文档版本或身份不合法".into());
        }
        let g = &self.editable.graph;
        let mut ids = BTreeSet::new();
        ids.insert(self.document_id.clone());
        let mut add = |id: &String| -> Result<(), String> {
            if Uuid::parse_str(id).is_err() || !ids.insert(id.clone()) {
                Err("文档对象 UUID 缺失或重复".into())
            } else {
                Ok(())
            }
        };
        if !exact_order(&g.profile_order, g.profiles.keys().cloned()) {
            return Err("配置顺序引用不完整或重复".into());
        }
        let mut layer_order = vec![];
        for (id, p) in &g.profiles {
            add(id)?;
            name_valid(&p.name)?;
            layer_order.extend(p.layer_order.clone());
        }
        if !exact_order(&layer_order, g.layers.keys().cloned()) {
            return Err("层归属引用不完整或重复".into());
        }
        for (id, l) in &g.layers {
            add(id)?;
            name_valid(&l.name)?;
            if !exact_order(&l.control_order, l.controls.keys().cloned()) {
                return Err("控件顺序引用不完整或重复".into());
            }
            let mut logical = BTreeSet::new();
            for (id, b) in &l.controls {
                add(id)?;
                if !logical.insert(&b.logical_control)
                    || b.logical_control
                        .strip_prefix("logical-")
                        .and_then(|s| s.parse::<usize>().ok())
                        .is_none()
                {
                    return Err("逻辑控件身份无效或重复".into());
                }
                serde_json::from_value::<Binding>(b.original.clone())
                    .map_err(|_| "原始绑定结构不合法")?;
                if b.action_id
                    .as_ref()
                    .is_some_and(|a| !g.actions.contains_key(a))
                {
                    return Err(format!("控件 {id} 引用了不存在的动作"));
                }
            }
        }
        for id in g.actions.keys() {
            add(id)?;
        }
        if self
            .editable
            .source_refs
            .keys()
            .any(|id| !ids.contains(id) || id == &self.document_id)
        {
            return Err("来源引用指向不存在的对象".into());
        }
        Ok(())
    }
    pub fn actions_known(&self) -> bool {
        self.editable
            .graph
            .actions
            .values()
            .all(|a| a.validate().is_ok())
    }
    pub fn create_profile(&mut self, name: &str) -> Result<String, String> {
        name_valid(name)?;
        let id = new_id();
        self.editable.graph.profile_order.push(id.clone());
        self.editable.graph.profiles.insert(
            id.clone(),
            ProfileNode {
                name: name.into(),
                layer_order: vec![],
            },
        );
        self.create_layer(&id, "基础层")?;
        Ok(id)
    }
    pub fn create_layer(&mut self, pid: &str, name: &str) -> Result<String, String> {
        name_valid(name)?;
        let p = self
            .editable
            .graph
            .profiles
            .get_mut(pid)
            .ok_or("配置不存在")?;
        let id = new_id();
        let mut layer = LayerNode {
            control_settings: None,
            name: name.into(),
            control_order: vec![],
            controls: BTreeMap::new(),
        };
        for slot in 0..16 {
            let bid = new_id();
            layer.control_order.push(bid.clone());
            layer.controls.insert(
                bid,
                BindingNode {
                    logical_control: format!("logical-{slot}"),
                    original: json!("Empty"),
                    action_id: None,
                },
            );
        }
        p.layer_order.push(id.clone());
        self.editable.graph.layers.insert(id.clone(), layer);
        Ok(id)
    }
    fn copy_layer(
        &mut self,
        lid: &str,
        name: &str,
        action_map: &mut BTreeMap<String, String>,
    ) -> Result<String, String> {
        name_valid(name)?;
        let old = self
            .editable
            .graph
            .layers
            .get(lid)
            .ok_or("层不存在")?
            .clone();
        let id = new_id();
        let mut layer = LayerNode {
            control_settings: old.control_settings.clone(),
            name: name.into(),
            control_order: vec![],
            controls: BTreeMap::new(),
        };
        self.copy_source(lid, &id);
        for bid in old.control_order {
            let mut b = old.controls[&bid].clone();
            let new_bid = new_id();
            if let Some(aid) = &b.action_id {
                let mapped = if let Some(id) = action_map.get(aid) {
                    id.clone()
                } else {
                    let id = new_id();
                    self.editable
                        .graph
                        .actions
                        .insert(id.clone(), self.editable.graph.actions[aid].clone());
                    self.copy_source(aid, &id);
                    action_map.insert(aid.clone(), id.clone());
                    id
                };
                b.action_id = Some(mapped);
            }
            self.copy_source(&bid, &new_bid);
            layer.control_order.push(new_bid.clone());
            layer.controls.insert(new_bid, b);
        }
        self.editable.graph.layers.insert(id.clone(), layer);
        Ok(id)
    }
    fn copy_source(&mut self, old: &str, new: &str) {
        if let Some(mut source) = self.editable.source_refs.get(old).cloned() {
            if let Some(fields) = source.as_object_mut() {
                fields.insert("kind".into(), json!("derived"));
                fields.insert("copied_from".into(), json!(old));
            }
            self.editable.source_refs.insert(new.into(), source);
        }
    }
    pub fn duplicate_profile(&mut self, pid: &str, name: &str) -> Result<String, String> {
        name_valid(name)?;
        let old = self
            .editable
            .graph
            .profiles
            .get(pid)
            .ok_or("配置不存在")?
            .clone();
        let id = new_id();
        let mut layers = vec![];
        let mut actions = BTreeMap::new();
        self.copy_source(pid, &id);
        for lid in old.layer_order {
            let name = self.editable.graph.layers[&lid].name.clone();
            layers.push(self.copy_layer(&lid, &name, &mut actions)?);
        }
        self.editable.graph.profiles.insert(
            id.clone(),
            ProfileNode {
                name: name.into(),
                layer_order: layers,
            },
        );
        self.editable.graph.profile_order.push(id.clone());
        Ok(id)
    }
    pub fn duplicate_layer(&mut self, pid: &str, lid: &str, name: &str) -> Result<String, String> {
        if !self
            .editable
            .graph
            .profiles
            .get(pid)
            .ok_or("配置不存在")?
            .layer_order
            .iter()
            .any(|id| id == lid)
        {
            return Err("层不属于所选配置".into());
        }
        let id = self.copy_layer(lid, name, &mut BTreeMap::new())?;
        self.editable
            .graph
            .profiles
            .get_mut(pid)
            .unwrap()
            .layer_order
            .push(id.clone());
        Ok(id)
    }
    pub fn rename_profile(&mut self, id: &str, name: &str) -> Result<(), String> {
        name_valid(name)?;
        self.editable
            .graph
            .profiles
            .get_mut(id)
            .ok_or("配置不存在")?
            .name = name.into();
        Ok(())
    }
    pub fn rename_layer(&mut self, id: &str, name: &str) -> Result<(), String> {
        name_valid(name)?;
        self.editable
            .graph
            .layers
            .get_mut(id)
            .ok_or("层不存在")?
            .name = name.into();
        Ok(())
    }
    pub fn move_profile(&mut self, id: &str, index: usize) -> Result<(), String> {
        move_item(&mut self.editable.graph.profile_order, id, index)
    }
    pub fn move_layer(&mut self, pid: &str, id: &str, index: usize) -> Result<(), String> {
        move_item(
            &mut self
                .editable
                .graph
                .profiles
                .get_mut(pid)
                .ok_or("配置不存在")?
                .layer_order,
            id,
            index,
        )
    }
    pub fn delete_layer(&mut self, pid: &str, lid: &str) -> Result<(), String> {
        let p = self
            .editable
            .graph
            .profiles
            .get_mut(pid)
            .ok_or("配置不存在")?;
        let index = p
            .layer_order
            .iter()
            .position(|v| v == lid)
            .ok_or("层不属于所选配置")?;
        p.layer_order.remove(index);
        self.remove_layer(lid)?;
        self.remove_orphan_actions();
        Ok(())
    }
    pub fn delete_profile(&mut self, pid: &str) -> Result<(), String> {
        let p = self
            .editable
            .graph
            .profiles
            .remove(pid)
            .ok_or("配置不存在")?;
        self.editable.graph.profile_order.retain(|v| v != pid);
        self.editable.source_refs.remove(pid);
        for lid in p.layer_order {
            self.remove_layer(&lid)?;
        }
        self.remove_orphan_actions();
        Ok(())
    }
    fn remove_layer(&mut self, lid: &str) -> Result<(), String> {
        let l = self.editable.graph.layers.remove(lid).ok_or("层不存在")?;
        self.editable.source_refs.remove(lid);
        for bid in l.controls.keys() {
            self.editable.source_refs.remove(bid);
        }
        Ok(())
    }
    fn remove_orphan_actions(&mut self) {
        let used: BTreeSet<_> = self
            .editable
            .graph
            .layers
            .values()
            .flat_map(|l| l.controls.values())
            .filter_map(|b| b.action_id.clone())
            .collect();
        self.editable
            .graph
            .actions
            .retain(|id, _| used.contains(id));
        let valid: BTreeSet<_> = self
            .editable
            .graph
            .profiles
            .keys()
            .chain(self.editable.graph.layers.keys())
            .chain(self.editable.graph.actions.keys())
            .cloned()
            .chain(
                self.editable
                    .graph
                    .layers
                    .values()
                    .flat_map(|l| l.controls.keys().cloned()),
            )
            .collect();
        self.editable.source_refs.retain(|id, _| valid.contains(id));
    }
    pub fn view(&self) -> Result<(DeviceConfig, BTreeMap<String, Action>), String> {
        let mut config: DeviceConfig = serde_json::from_value(self.origin["config"].clone())
            .map_err(|_| "来源配置不能显示")?;
        let source = config.clone();
        if self.baseline.is_some() || self.origin["kind"] == "legacy-local" {
            config.source = Source::LocalDraft;
        }
        config.profiles.clear();
        let mut edits = BTreeMap::new();
        for (pi, pid) in self.editable.graph.profile_order.iter().enumerate() {
            let p = &self.editable.graph.profiles[pid];
            let mut profile = Profile {
                id: json!(pid),
                index: pi,
                name: p.name.clone(),
                layers: vec![],
            };
            for lid in &p.layer_order {
                let l = &self.editable.graph.layers[lid];
                let sr = self.editable.source_refs.get(lid);
                let original = sr.and_then(|r| {
                    source
                        .profiles
                        .iter()
                        .find(|p| p.id == r["profile_source_id"])
                        .and_then(|p| p.layers.iter().find(|l| l.id == r["source_id"]))
                });
                let mut layer = original.cloned().unwrap_or_else(|| Layer {
                    id: json!(lid),
                    name: l.name.clone(),
                    color: Value::Null,
                    keys: std::array::from_fn(|_| Binding::Empty),
                    encoders: std::array::from_fn(|_| Binding::Empty),
                    joystick: Joystick {
                        kind: Value::Null,
                        sectors: vec![],
                    },
                    lights: Lights {
                        backlight: Value::Null,
                        underglow: Value::Null,
                    },
                });
                layer.id = json!(lid);
                layer.name = l.name.clone();
                let mut values = vec![];
                for bid in &l.control_order {
                    let b = &l.controls[bid];
                    let slot = b
                        .logical_control
                        .strip_prefix("logical-")
                        .and_then(|s| s.parse::<usize>().ok())
                        .ok_or("无效逻辑控件")?;
                    if slot != values.len() {
                        return Err("逻辑控件顺序无法显示".into());
                    }
                    values.push(
                        serde_json::from_value::<Binding>(b.original.clone())
                            .map_err(|_| "原始绑定不能显示")?,
                    );
                    if let Some(aid) = &b.action_id {
                        edits.insert(
                            binding_key(&json!(pid), &json!(lid), slot as u32),
                            self.editable.graph.actions[aid].clone(),
                        );
                    }
                }
                if values.len() < 16 {
                    return Err("层缺少基础逻辑控件".into());
                }
                layer.keys = values[0..13].to_vec().try_into().unwrap();
                layer.encoders = values[13..16].to_vec().try_into().unwrap();
                layer.joystick.sectors = values[16..].to_vec();
                profile.layers.push(layer);
            }
            config.profiles.push(profile);
        }
        Ok((config, edits))
    }
    pub fn structure_changes(&self) -> Vec<String> {
        let Ok(source) = serde_json::from_value::<DeviceConfig>(self.origin["config"].clone())
        else {
            return vec![];
        };
        let mut changes = vec![];
        let g = &self.editable.graph;
        let refs = &self.editable.source_refs;
        for old in &source.profiles {
            if !g.profile_order.iter().any(|id| {
                refs.get(id)
                    .is_some_and(|r| r["source_id"] == old.id && r["kind"] != "derived")
            }) {
                changes.push(format!("删除配置：{}", old.name));
            }
        }
        for (pi, pid) in g.profile_order.iter().enumerate() {
            let p = &g.profiles[pid];
            let sr = refs.get(pid);
            let original = sr
                .filter(|r| r["kind"] != "derived")
                .and_then(|r| source.profiles.iter().find(|old| old.id == r["source_id"]));
            if let Some(old) = original {
                if p.name != old.name {
                    changes.push(format!("配置改名：{} → {}", old.name, p.name));
                }
                if pi != old.index {
                    changes.push(format!("配置排序：{} → 第 {} 项", p.name, pi + 1));
                }
                for l in &old.layers {
                    if !p.layer_order.iter().any(|id| {
                        refs.get(id)
                            .is_some_and(|r| r["source_id"] == l.id && r["kind"] != "derived")
                    }) {
                        changes.push(format!("删除层：{} / {}", p.name, l.name));
                    }
                }
            } else {
                changes.push(format!("新增配置：{}", p.name));
            }
            for (li, lid) in p.layer_order.iter().enumerate() {
                let l = &g.layers[lid];
                if let Some(settings) = &l.control_settings {
                    let mode = match settings.joystick_mode.as_str() {
                        "source" => "保留来源模式",
                        "logical_sectors" => "逻辑扇区意图",
                        other => other,
                    };
                    changes.push(format!(
                        "控件意图：{} / {} · {} · 活动扇区 {}（旧槽位保留）",
                        p.name, l.name, mode, settings.active_sectors
                    ));
                }
                let original_layer =
                    refs.get(lid)
                        .filter(|r| r["kind"] != "derived")
                        .and_then(|r| {
                            original.and_then(|p| {
                                p.layers
                                    .iter()
                                    .enumerate()
                                    .find(|(_, l)| l.id == r["source_id"])
                            })
                        });
                if let Some((index, old)) = original_layer {
                    if old.name != l.name {
                        changes.push(format!("层改名：{} / {} → {}", p.name, old.name, l.name));
                    }
                    if index != li {
                        changes.push(format!(
                            "层排序：{} / {} → 第 {} 项",
                            p.name,
                            l.name,
                            li + 1
                        ));
                    }
                } else {
                    changes.push(format!("新增层：{} / {}", p.name, l.name));
                }
            }
        }
        changes
    }
}
fn move_item(order: &mut Vec<String>, id: &str, index: usize) -> Result<(), String> {
    if index >= order.len() {
        return Err("目标排序位置不存在".into());
    }
    let from = order
        .iter()
        .position(|v| v == id)
        .ok_or("对象不属于此列表")?;
    let value = order.remove(from);
    order.insert(index, value);
    Ok(())
}
