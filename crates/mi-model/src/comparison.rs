//! D07: local three-way comparison. No transport and no device payload compilation.
use crate::{
    document::{EditableState, LocalDocumentV2},
    DeviceConfig,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, PartialEq)]
pub enum Choice {
    Merge,
    KeepLocal,
    UseDevice,
    Cancel,
}
#[derive(Clone, Serialize)]
pub struct ComparisonRow {
    pub path: String,
    pub label: String,
    pub base: Option<Value>,
    pub local: Option<Value>,
    pub device: Option<Value>,
    pub result: String,
}
#[derive(Clone, Serialize)]
pub struct Comparison {
    pub token: String,
    pub rows: Vec<ComparisonRow>,
    pub conflicts: Vec<String>,
    pub identity_matches: bool,
    pub revision: u64,
    pub identity_conflict: bool,
    pub identity_confirmation_required: bool,
    pub identity_user_confirmed: bool,
    #[serde(skip)]
    original: Value,
    #[serde(skip)]
    remote: LocalDocumentV2,
    #[serde(skip)]
    merged: EditableState,
}
/// New baselines carry their projection; old T06 documents retain it in immutable origin.
pub fn base_config(doc: &LocalDocumentV2) -> Result<DeviceConfig, String> {
    serde_json::from_value(
        doc.baseline
            .as_ref()
            .and_then(|b| b.get("comparison_config"))
            .unwrap_or(&doc.origin["config"])
            .clone(),
    )
    .map_err(|_| "基线配置不能显示".into())
}
fn source_key(value: &Value) -> Option<String> {
    let kind = value["kind"].as_str()?;
    let fields: &[&str] = match kind {
        "source-profile" => &["source_id"],
        "source-layer" => &["profile_source_id", "source_id"],
        "source-binding" => &["profile_source_id", "layer_source_id", "logical_control"],
        _ => return None,
    };
    Some(
        json!([
            kind,
            fields
                .iter()
                .map(|field| value[*field].clone())
                .collect::<Vec<_>>()
        ])
        .to_string(),
    )
}
fn align(doc: &mut LocalDocumentV2, reference: &EditableState) -> Result<(), String> {
    let mut keys = BTreeMap::new();
    for (id, source) in &reference.source_refs {
        if let Some(key) = source_key(source) {
            if keys.insert(key, id.clone()).is_some() {
                return Err("来源 ID 存在歧义，不能自动对齐".into());
            }
        }
    }
    let mapping: BTreeMap<_, _> = doc
        .editable
        .source_refs
        .iter()
        .filter_map(|(id, source)| {
            source_key(source)
                .and_then(|key| keys.get(&key).map(|target| (id.clone(), target.clone())))
        })
        .collect();
    let map = |id: String| mapping.get(&id).cloned().unwrap_or(id);
    let g = &mut doc.editable.graph;
    g.profile_order = std::mem::take(&mut g.profile_order)
        .into_iter()
        .map(&map)
        .collect();
    g.profiles = std::mem::take(&mut g.profiles)
        .into_iter()
        .map(|(id, mut p)| {
            p.layer_order = p.layer_order.into_iter().map(&map).collect();
            (map(id), p)
        })
        .collect();
    g.layers = std::mem::take(&mut g.layers)
        .into_iter()
        .map(|(id, mut l)| {
            l.control_order = l.control_order.into_iter().map(&map).collect();
            l.controls = l.controls.into_iter().map(|(id, b)| (map(id), b)).collect();
            (map(id), l)
        })
        .collect();
    doc.editable.source_refs = std::mem::take(&mut doc.editable.source_refs)
        .into_iter()
        .map(|(id, r)| (map(id), r))
        .collect();
    Ok(())
}
fn merge(
    path: &str,
    b: Option<&Value>,
    l: Option<&Value>,
    r: Option<&Value>,
    rows: &mut Vec<ComparisonRow>,
    conflicts: &mut Vec<String>,
) -> Option<Value> {
    if l == b && r == b {
        return b.cloned();
    }
    if let (Some(Value::Object(bo)), Some(Value::Object(lo)), Some(Value::Object(ro))) = (b, l, r) {
        // A binding's local intent and its original value refer to the same user-visible field.
        if !bo.contains_key("logical_control") {
            let keys: BTreeSet<_> = bo.keys().chain(lo.keys()).chain(ro.keys()).collect();
            let mut result = serde_json::Map::new();
            for key in keys {
                if let Some(v) = merge(
                    &format!("{path}/{key}"),
                    bo.get(key),
                    lo.get(key),
                    ro.get(key),
                    rows,
                    conflicts,
                ) {
                    result.insert(key.clone(), v);
                }
            }
            return Some(Value::Object(result));
        }
    }
    if path.ends_with("/original") && b != r && (has_unknown(b) || has_unknown(r)) {
        conflicts.push(format!("未知 raw 绑定变化：{path}"));
    }
    if let (Some(b), Some(r)) = (b, r) {
        if b.get("logical_control").is_some()
            && b["original"] != r["original"]
            && (has_unknown(b.get("original")) || has_unknown(r.get("original")))
        {
            conflicts.push(format!("未知 raw 绑定变化：{path}"));
        }
    }
    let (result, value) = if l == r {
        ("共同值", l)
    } else if l == b {
        ("设备", r)
    } else if r == b {
        ("本地", l)
    } else {
        conflicts.push(path.into());
        ("冲突", l)
    };
    rows.push(ComparisonRow {
        path: path.into(),
        label: String::new(),
        base: b.cloned(),
        local: l.cloned(),
        device: r.cloned(),
        result: result.into(),
    });
    value.cloned()
}
// Only fields represented by the editable graph are removed. Everything else is opaque.
fn opaque(config: &DeviceConfig, other: &DeviceConfig) -> Value {
    let mut root = config.raw.clone();
    let mut extras = serde_json::Map::new();
    if let Some(profiles) = root
        .as_object_mut()
        .and_then(|r| r.remove("profiles"))
        .and_then(|v| v.as_array().cloned())
    {
        for mut p in profiles {
            let Some(peer) = other.profiles.iter().find(|peer| peer.id == p["id"]) else {
                continue;
            };
            let key = p["id"].to_string();
            let Some(o) = p.as_object_mut() else { continue };
            o.remove("id");
            if o.get("name").is_some_and(Value::is_string) {
                o.remove("name");
            }
            let mut layers = serde_json::Map::new();
            if let Some(ls) = o.remove("layers").and_then(|v| v.as_array().cloned()) {
                for mut l in ls {
                    if !peer.layers.iter().any(|peer| peer.id == l["id"]) {
                        continue;
                    }
                    let lid = l["id"].to_string();
                    if let Some(lo) = l.as_object_mut() {
                        lo.remove("id");
                        if lo.get("name").is_some_and(Value::is_string) {
                            lo.remove("name");
                        }
                        if let Some(layout) = lo.get_mut("layout").and_then(Value::as_object_mut) {
                            layout.remove("keymap");
                            if layout
                                .get("encoders")
                                .and_then(Value::as_array)
                                .is_some_and(|a| a.len() == 3)
                            {
                                layout.remove("encoders");
                            }
                            // Joystick metadata remains opaque; bindings are separately compared.
                            if let Some(sectors) = layout
                                .get_mut("joystick")
                                .and_then(|j| j.get_mut("sectors"))
                                .and_then(Value::as_array_mut)
                            {
                                for sector in sectors {
                                    if let Some(s) = sector.as_object_mut() {
                                        s.remove("k");
                                    }
                                }
                            }
                            if layout.is_empty() {
                                lo.remove("layout");
                            }
                        }
                        if !lo.is_empty() {
                            layers.insert(lid, l);
                        }
                    }
                }
            }
            if !layers.is_empty() {
                o.insert("layers".into(), Value::Object(layers));
            }
            if !o.is_empty() {
                extras.insert(key, p);
            }
        }
    }
    let mut unknown = BTreeMap::new();
    for p in &config.profiles {
        let Some(peer) = other.profiles.iter().find(|peer| peer.id == p.id) else {
            continue;
        };
        for l in &p.layers {
            if !peer.layers.iter().any(|peer| peer.id == l.id) {
                continue;
            }
            for (slot, b) in l
                .keys
                .iter()
                .chain(l.encoders.iter())
                .chain(l.joystick.sectors.iter())
                .enumerate()
            {
                if matches!(b, crate::Binding::Unknown(_)) {
                    unknown.insert(format!("{} / {} / logical-{slot}", p.id, l.id), json!(b));
                }
            }
        }
    }
    json!({"root":root,"profiles":extras,"smart_actions":config.smart_actions,"unknown_bindings":unknown})
}
fn has_unknown(v: Option<&Value>) -> bool {
    v.is_some_and(|v| v.get("Unknown").is_some())
}
impl Comparison {
    pub fn new(
        local: &LocalDocumentV2,
        config: &DeviceConfig,
        mut baseline: Value,
    ) -> Result<Self, String> {
        if !baseline.is_object() || !local.baseline.as_ref().is_some_and(Value::is_object) {
            return Err("缺少有效的设备基线；不能比较".into());
        }
        let base_config = base_config(local)?;
        let mut base = LocalDocumentV2::from_config(&base_config, None, "comparison")?;
        align(&mut base, &local.editable)?;
        let mut remote = LocalDocumentV2::from_config(config, None, "comparison")?;
        align(&mut remote, &base.editable)?;
        baseline["comparison_config"] = json!(config);
        remote.baseline = Some(baseline);
        let mut rows = vec![];
        let mut conflicts = vec![];
        let old_identity = &local
            .baseline
            .as_ref()
            .ok_or("没有设备基线；请先从核验快照新建草稿")?["device_identity"];
        let new_identity = &remote.baseline.as_ref().unwrap()["device_identity"];
        let identity_matches = old_identity["confidence"] == "verified-serial"
            && new_identity["confidence"] == "verified-serial"
            && old_identity["serial"].as_str().is_some()
            && old_identity["serial"] == new_identity["serial"];
        let identity_conflict = !identity_matches
            && ((old_identity["serial"].as_str().is_some()
                && new_identity["serial"].as_str().is_some()
                && old_identity["serial"] != new_identity["serial"])
                || (old_identity["interface"].as_str().is_some()
                    && new_identity["interface"].as_str().is_some()
                    && old_identity["interface"] != new_identity["interface"]));
        let serial_conflict = old_identity["serial"].as_str().is_some()
            && new_identity["serial"].as_str().is_some()
            && old_identity["serial"] != new_identity["serial"];
        let identity_confirmation_required = identity_conflict && !serial_conflict;
        if !identity_matches {
            conflicts.push(
                if identity_conflict {
                    if identity_confirmation_required {
                        "设备接口已变化；请先核对并确认是同一设备"
                    } else {
                        "设备身份不同；不能接受错误基线"
                    }
                } else {
                    "跨重连设备身份未确认；只允许明确选择整份版本"
                }
                .into(),
            );
        }
        for kind in ["source-profile", "source-layer", "source-binding"] {
            let sources = |state: &EditableState| {
                state
                    .source_refs
                    .values()
                    .filter(|r| r["kind"] == kind)
                    .filter_map(source_key)
                    .collect::<BTreeSet<_>>()
            };
            let b = sources(&base.editable);
            let r = sources(&remote.editable);
            if b.difference(&r).next().is_some() && r.difference(&b).next().is_some() {
                let label = match kind {
                    "source-profile" => "配置",
                    "source-layer" => "层",
                    _ => "控件",
                };
                conflicts.push(format!("{label}源 ID 无法对照；新增与删除不能猜作同一对象"));
            }
        }
        let old_raw = opaque(&base_config, config);
        let new_raw = opaque(config, &base_config);
        if old_raw != new_raw {
            conflicts.push("未知 raw 改变；双方原值保留，禁止自动合并".into());
            rows.push(ComparisonRow {
                path: "未知 raw".into(),
                label: "未知原值".into(),
                base: Some(old_raw.clone()),
                local: Some(old_raw),
                device: Some(new_raw),
                result: "冲突".into(),
            });
        }
        let b = json!(base.editable);
        let l = json!(local.editable);
        let r = json!(remote.editable);
        let merged = merge("", Some(&b), Some(&l), Some(&r), &mut rows, &mut conflicts).unwrap();
        let merged: EditableState = serde_json::from_value(merged).map_err(|e| e.to_string())?;
        let mut candidate = local.clone();
        candidate.editable = merged.clone();
        if let Err(e) = candidate.validate() {
            conflicts.push(format!("引用校验：{e}"));
        }
        let mut labels = BTreeMap::new();
        for state in [&base.editable, &remote.editable, &local.editable] {
            for (id, p) in &state.graph.profiles {
                labels.insert(id.clone(), format!("配置 {}", p.name));
            }
            for (id, l) in &state.graph.layers {
                labels.insert(id.clone(), format!("层 {}", l.name));
                for (bid, b) in &l.controls {
                    labels.insert(bid.clone(), format!("{} / {}", l.name, b.logical_control));
                }
            }
            for (id, a) in &state.graph.actions {
                labels.insert(id.clone(), format!("动作 {}", a.name));
            }
        }
        for row in &mut rows {
            if !row.label.is_empty() {
                continue;
            }
            let context = row
                .path
                .split('/')
                .filter_map(|id| labels.get(id))
                .last()
                .cloned()
                .unwrap_or_else(|| "文档".into());
            let field = match row.path.rsplit('/').next().unwrap_or("") {
                "name" => "名称",
                "profile_order" => "配置顺序",
                "layer_order" => "层顺序",
                "control_order" => "控件顺序",
                _ => "对象或绑定",
            };
            row.label = format!("{context} · {field}");
        }
        for conflict in &mut conflicts {
            for row in &rows {
                if row.path.starts_with('/') && conflict.contains(&row.path) {
                    *conflict = if *conflict == row.path {
                        format!("{}：本地与设备变化不同", row.label)
                    } else {
                        conflict.replace(&row.path, &row.label)
                    };
                }
            }
        }
        Ok(Self {
            token: crate::document::new_id(),
            rows,
            conflicts,
            identity_matches,
            identity_conflict,
            identity_confirmation_required,
            identity_user_confirmed: false,
            revision: local.revision,
            original: json!(local),
            remote,
            merged,
        })
    }
    /// Explicit user resolution for a transient interface change, scoped to this exact snapshot.
    /// This does not upgrade observed identity or make automatic merge eligible.
    pub fn confirm_identity(
        &mut self,
        local: &LocalDocumentV2,
        current: &Value,
    ) -> Result<(), String> {
        if !self.identity_confirmation_required {
            return Err("此比较不能由人工确认绕过设备身份差异".into());
        }
        if json!(local) != self.original {
            return Err("草稿已变化；请重新比较".into());
        }
        self.ensure_snapshot(current)?;
        if self.identity_user_confirmed {
            return Ok(());
        }
        self.identity_user_confirmed = true;
        self.identity_conflict = false;
        self.conflicts
            .retain(|c| c != "设备接口已变化；请先核对并确认是同一设备");
        self.conflicts
            .push("设备身份已由用户核对；技术身份仍未确认，不自动合并".into());
        Ok(())
    }
    pub fn ensure_snapshot(&self, current: &Value) -> Result<(), String> {
        let mut expected = self.remote.baseline.clone().ok_or("缺少设备基线")?;
        expected
            .as_object_mut()
            .ok_or("设备基线无效")?
            .remove("comparison_config");
        let mut current = current.clone();
        if let Some(o) = current.as_object_mut() {
            o.remove("comparison_config");
        }
        if expected != current {
            return Err("设备快照或身份已变化；请重新读取并比较".into());
        }
        Ok(())
    }
    pub fn can_merge(&self) -> bool {
        self.conflicts.is_empty() && self.identity_matches
    }
    pub fn resolve(
        &self,
        local: &LocalDocumentV2,
        choice: Choice,
    ) -> Result<Option<LocalDocumentV2>, String> {
        if choice == Choice::Cancel {
            return Ok(None);
        }
        if json!(local) != self.original {
            return Err("草稿已变化；请重新比较".into());
        }
        if self.identity_conflict {
            return Err("设备身份不同；不能套用基线".into());
        }
        if choice == Choice::Merge && !self.can_merge() {
            return Err("存在冲突；不能合并".into());
        }
        let mut next = local.clone();
        next.baseline = self.remote.baseline.clone();
        if self.identity_user_confirmed {
            next.baseline.as_mut().unwrap()["identity_resolution"] = json!({"kind":"user-confirmed","comparison_token":self.token,"device_identity":self.remote.baseline.as_ref().unwrap()["device_identity"]});
        }
        next.editable = match choice {
            Choice::Merge => self.merged.clone(),
            Choice::KeepLocal => local.editable.clone(),
            Choice::UseDevice => self.remote.editable.clone(),
            Choice::Cancel => unreachable!(),
        };
        if choice == Choice::KeepLocal {
            for (lid, layer) in &mut next.editable.graph.layers {
                if let Some(remote) = self.remote.editable.graph.layers.get(lid) {
                    for (bid, binding) in &mut layer.controls {
                        if let Some(r) = remote.controls.get(bid) {
                            if has_unknown(Some(&binding.original))
                                || has_unknown(Some(&r.original))
                            {
                                binding.original = r.original.clone();
                            }
                        }
                    }
                }
            }
        }
        next.revision = next.revision.checked_add(1).ok_or("文档修订号已达上限")?;
        next.validate()?;
        Ok(Some(next))
    }
}

/// #22 must invoke this after a fresh identity/file-list query, before any future write.
/// A cached comparison cannot authorize a write by itself.
pub fn ensure_apply_baseline(document: &LocalDocumentV2, fresh: &Value) -> Result<(), String> {
    let baseline = document
        .baseline
        .as_ref()
        .filter(|b| b.is_object())
        .ok_or("没有设备基线，拒绝应用")?;
    if !baseline["device_identity"].is_object() || !fresh["device_identity"].is_object() {
        return Err("设备身份未经核验，拒绝应用".into());
    }
    if !["observed-interface", "verified-serial"]
        .iter()
        .any(|c| baseline["device_identity"]["confidence"] == *c)
    {
        return Err("设备身份状态未知，拒绝应用".into());
    }
    for key in ["keymap", "smart_actions"] {
        let file = &fresh[key];
        if !(file["state"] == "missing" && key == "smart_actions")
            && !(file["state"] == "present"
                && file["sha1"].as_str().is_some_and(|s| !s.is_empty())
                && file["size"].as_u64().is_some())
        {
            return Err("缺少新读取的文件摘要，拒绝应用".into());
        }
    }
    for key in ["device_identity", "firmware"] {
        if baseline[key] != fresh[key] {
            return Err("设备身份或固件已变化；请重新比较".into());
        }
    }
    for key in ["keymap", "smart_actions"] {
        for field in ["state", "sha1", "size"] {
            if baseline[key][field] != fresh[key][field] {
                return Err("设备摘要已变化；请重新比较".into());
            }
        }
    }
    Ok(())
}
