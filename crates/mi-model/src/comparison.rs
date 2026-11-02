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
    match value["kind"].as_str()? {
        "source-profile" | "source-layer" | "source-binding" => Some(value.to_string()),
        _ => None,
    }
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
    fn rewrite(v: &mut Value, mapping: &BTreeMap<String, String>) {
        match v {
            Value::String(s) => {
                if let Some(target) = mapping.get(s) {
                    *s = target.clone()
                }
            }
            Value::Array(a) => {
                for v in a {
                    rewrite(v, mapping)
                }
            }
            Value::Object(o) => {
                let old = std::mem::take(o);
                for (key, mut value) in old {
                    rewrite(&mut value, mapping);
                    o.insert(mapping.get(&key).cloned().unwrap_or(key), value);
                }
            }
            _ => {}
        }
    }
    let mut v = serde_json::to_value(&doc.editable).map_err(|e| e.to_string())?;
    rewrite(&mut v, &mapping);
    doc.editable = serde_json::from_value(v).map_err(|e| e.to_string())?;
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
        base: b.cloned(),
        local: l.cloned(),
        device: r.cloned(),
        result: result.into(),
    });
    value.cloned()
}
// Only fields represented by the editable graph are removed. Everything else is opaque.
fn opaque(config: &DeviceConfig) -> Value {
    let mut root = config.raw.clone();
    let mut extras = serde_json::Map::new();
    if let Some(profiles) = root
        .as_object_mut()
        .and_then(|r| r.remove("profiles"))
        .and_then(|v| v.as_array().cloned())
    {
        for mut p in profiles {
            let key = p["id"].to_string();
            let Some(o) = p.as_object_mut() else { continue };
            o.remove("id");
            o.remove("name");
            let mut layers = serde_json::Map::new();
            if let Some(ls) = o.remove("layers").and_then(|v| v.as_array().cloned()) {
                for mut l in ls {
                    let lid = l["id"].to_string();
                    if let Some(lo) = l.as_object_mut() {
                        lo.remove("id");
                        lo.remove("name");
                        if let Some(layout) = lo.get_mut("layout").and_then(Value::as_object_mut) {
                            layout.remove("keymap");
                            layout.remove("encoders");
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
    json!({"root":root,"profiles":extras,"smart_actions":config.smart_actions})
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
        let identity_conflict = (old_identity["serial"].as_str().is_some()
            && new_identity["serial"].as_str().is_some()
            && old_identity["serial"] != new_identity["serial"])
            || (old_identity["interface"].as_str().is_some()
                && new_identity["interface"].as_str().is_some()
                && old_identity["interface"] != new_identity["interface"]);
        if !identity_matches {
            conflicts.push(
                if identity_conflict {
                    "设备身份不同；不能接受错误基线"
                } else {
                    "跨重连设备身份未确认；只允许明确选择整份版本"
                }
                .into(),
            );
        }
        let old_raw = opaque(&base_config);
        let new_raw = opaque(config);
        if old_raw != new_raw {
            conflicts.push("未知 raw 改变；双方原值保留，禁止自动合并".into());
            rows.push(ComparisonRow {
                path: "未知 raw".into(),
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
        Ok(Self {
            token: crate::document::new_id(),
            rows,
            conflicts,
            identity_matches,
            identity_conflict,
            revision: local.revision,
            original: json!(local),
            remote,
            merged,
        })
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
