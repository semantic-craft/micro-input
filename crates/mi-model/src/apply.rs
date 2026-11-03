//! T13 software preparation only. No transport, writer, activation or recovery execution.
use crate::{comparison, document::LocalDocumentV2, draft::ActionKind};
use serde::Serialize;
use serde_json::{json, value::RawValue, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, DirBuilder, OpenOptions},
    io::Write,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};

pub const UNAVAILABLE: &str =
    "UNKNOWN-08/09：配置写入、读回和设备恢复尚未实机验证；当前仅准备本机副本，未发送";
// TODO(UNKNOWN-08): implement a writer only after configuration commit/readback evidence.
// TODO(UNKNOWN-09): implement device restore only after recovery evidence.
// Both require explicit authorization. Temporary-file probes do not establish these capabilities.
#[derive(Clone, Serialize)]
pub struct Preview {
    pub kind: String,
    pub token: String,
    pub document_id: String,
    pub revision: u64,
    pub document_sha256: String,
    pub device_identity: Value,
    pub firmware: Value,
    pub files: Value,
    pub active_indices: Value,
    pub active_source: String,
    pub profile_source_id: Value,
    pub layer_source_id: Value,
    pub logical_control: String,
    pub physical_position: String,
    pub before: String,
    pub after: String,
    pub planned_keymap_sha256: String,
    pub apply_available: bool,
    pub restore_available: bool,
    pub unavailable_reason: String,
}
#[derive(Serialize)]
pub struct Prepared {
    pub token: String,
    pub backup_path: String,
    pub manifest_sha256: String,
    pub status: String,
    pub unavailable_reason: String,
}
pub(crate) struct Pending {
    pub prepared: bool,
    pub preview: Preview,
    baseline: Value,
    keymap: Vec<u8>,
    planned_keymap: Vec<u8>,
    smart: Option<Vec<u8>>,
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn bytes(baseline: &Value, key: &str) -> Result<Option<Vec<u8>>, String> {
    let file = &baseline[key];
    if key == "smart_actions" && file["state"] == "missing" {
        return Ok(None);
    }
    let bytes: Vec<u8> =
        serde_json::from_value(file["bytes"].clone()).map_err(|_| "缺少核验文件原字节")?;
    if file["state"] != "present"
        || file["size"].as_u64() != Some(bytes.len() as u64)
        || file["sha1"].as_str() != Some(format!("{:x}", sha1::Sha1::digest(&bytes)).as_str())
    {
        return Err("文件原字节、大小与摘要不一致".into());
    }
    Ok(Some(bytes))
}
fn metadata(baseline: &Value) -> Value {
    let mut value = baseline.clone();
    for field in ["keymap", "smart_actions"] {
        if let Some(file) = value[field].as_object_mut() {
            file.remove("bytes");
        }
    }
    value
}
type Object<'a> = BTreeMap<String, &'a RawValue>;
fn object(raw: &RawValue) -> Result<Object<'_>, String> {
    serde_json::from_str(raw.get()).map_err(|_| "目标 JSON 对象结构未知".into())
}
fn field<'a>(object: &Object<'a>, name: &str) -> Result<&'a RawValue, String> {
    object
        .get(name)
        .copied()
        .ok_or_else(|| format!("目标缺少 {name}"))
}
fn array(raw: &RawValue) -> Result<Vec<&RawValue>, String> {
    serde_json::from_str(raw.get()).map_err(|_| "目标 JSON 数组结构未知".into())
}
fn unique<'a>(items: Vec<&'a RawValue>, id: &Value) -> Result<&'a RawValue, String> {
    let mut matching = Vec::new();
    for raw in items {
        if serde_json::from_str::<Value>(field(&object(raw)?, "id")?.get())
            .map_err(|e| e.to_string())?
            == *id
        {
            matching.push(raw);
        }
    }
    if matching.len() != 1 {
        return Err("来源 ID 缺失或存在歧义".into());
    }
    Ok(matching[0])
}
/// RawValue borrows ranges directly from `input` (serde_json 1.0.154, raw_value feature).
/// Strict device parsing has already rejected duplicate members and invalid row shapes.
fn splice(
    input: &[u8],
    pid: &Value,
    lid: &Value,
    slot: usize,
    token: &str,
) -> Result<Vec<u8>, String> {
    let root: &RawValue = serde_json::from_slice(input).map_err(|e| e.to_string())?;
    let p = unique(array(field(&object(root)?, "profiles")?)?, pid)?;
    let l = unique(array(field(&object(p)?, "layers")?)?, lid)?;
    let layout = object(field(&object(l)?, "layout")?)?;
    let rows = array(field(&layout, "keymap")?)?;
    let raw = rows
        .into_iter()
        .map(array)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .nth(slot)
        .ok_or("普通键逻辑槽不存在")?;
    let value = raw.get().as_bytes();
    let start = (value.as_ptr() as usize)
        .checked_sub(input.as_ptr() as usize)
        .ok_or("原字节范围无效")?;
    let end = start
        .checked_add(value.len())
        .filter(|end| *end <= input.len())
        .ok_or("原字节范围越界")?;
    if input.get(start..end) != Some(value) {
        return Err("原字节范围不一致".into());
    }
    let replacement = serde_json::to_vec(token).map_err(|e| e.to_string())?;
    let mut result = Vec::with_capacity(input.len() - value.len() + replacement.len());
    result.extend_from_slice(&input[..start]);
    result.extend_from_slice(&replacement);
    result.extend_from_slice(&input[end..]);
    Ok(result)
}
impl Pending {
    pub fn new(
        doc: &LocalDocumentV2,
        document_bytes: &[u8],
        current: &Value,
    ) -> Result<Self, String> {
        doc.validate()?;
        if serde_json::from_slice::<Value>(document_bytes).map_err(|_| "本地文档原字节无效")?
            != serde_json::to_value(doc).map_err(|e| e.to_string())?
        {
            return Err("内存文档与磁盘原字节不符；请重新打开".into());
        }
        comparison::ensure_apply_baseline(doc, current)?;
        if !matches!(
            current["firmware"].as_str(),
            Some("0.6.3" | "synthetic-0.6.3")
        ) || current["device_identity"]["session_id"].as_u64().is_none()
        {
            return Err("当前会话或固件未核验".into());
        }
        let keymap = bytes(current, "keymap")?.ok_or("没有 keymap 文件")?;
        let smart = bytes(current, "smart_actions")?;
        // Require exact baseline bytes as well as matching list digests.
        let baseline = doc.baseline.as_ref().ok_or("没有设备基线")?;
        if bytes(baseline, "keymap")?.as_ref() != Some(&keymap)
            || bytes(baseline, "smart_actions")? != smart
        {
            return Err("基线原字节已变化；请重新比较".into());
        }
        let config =
            crate::parse_device_files(&keymap, smart.as_deref()).map_err(|e| e.to_string())?;
        if config.raw["version"].as_u64() != Some(1) {
            return Err("设备配置版本未知，拒绝准备".into());
        }
        let mut reference = LocalDocumentV2::from_config(&config, None, "apply-reference")?;
        comparison::align(&mut reference, &doc.editable)?;
        if reference.editable.source_refs != doc.editable.source_refs {
            return Err("来源引用或结构变化不在单绑定范围内".into());
        }
        if doc.editable.graph.layers.iter().any(|(id, layer)| {
            layer.lighting.as_ref().is_some_and(|target| {
                reference
                    .lighting(id)
                    .map_or(true, |original| original != *target)
            })
        }) {
            return Self::lighting(doc, current, document_bytes, &reference, keymap, smart);
        }
        let mut normalized = doc.editable.graph.clone();
        for layer in normalized.layers.values_mut() {
            layer.lighting = None;
        }
        if normalized.actions.len() != 1 {
            return Err("仅支持一个普通设备按键意图；高级动作或多项改动尚不可用".into());
        }
        let mut target = None;
        for (lid, layer) in &mut normalized.layers {
            for (bid, binding) in &mut layer.controls {
                if let Some(aid) = binding.action_id.take() {
                    if target.replace((lid.clone(), bid.clone(), aid)).is_some() {
                        return Err("仅支持单个普通键绑定".into());
                    }
                }
            }
        }
        normalized.actions.clear();
        if normalized != reference.editable.graph {
            return Err("结构、模式或其他字段变化不在单绑定范围内".into());
        }
        let (local_lid, bid, aid) = target.ok_or("没有普通键变更")?;
        let binding = &doc.editable.graph.layers[&local_lid].controls[&bid];
        let slot = binding
            .logical_control
            .strip_prefix("logical-")
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|s| *s < 13)
            .ok_or("只支持普通键，不支持旋钮或摇杆")?;
        let action = &doc.editable.graph.actions[&aid];
        action.validate()?;
        let ActionKind::DeviceKey { token } =
            serde_json::from_value(action.payload.clone()).map_err(|_| "动作类型未知")?
        else {
            return Err("只有 DeviceKey 普通设备按键可准备；宿主快捷键不可转换".into());
        };
        let source = &doc.editable.source_refs[&bid];
        if source["kind"] != "source-binding"
            || source["logical_control"] != binding.logical_control
        {
            return Err("绑定来源无效")?;
        }
        let pid = &source["profile_source_id"];
        let lid = &source["layer_source_id"];
        let p = config
            .profiles
            .iter()
            .find(|p| p.id == *pid)
            .ok_or("来源配置不存在")?;
        let l = p
            .layers
            .iter()
            .find(|l| l.id == *lid)
            .ok_or("来源层不存在")?;
        let before = match &l.keys[slot] {
            crate::Binding::Basic(t) => t.0.clone(),
            crate::Binding::Empty => "空绑定".into(),
            crate::Binding::Unknown(Value::String(t)) if known_passthrough(l, slot, t) => t.clone(),
            _ => return Err("原绑定未知或高级类型；不能准备覆盖".into()),
        };
        if before == token {
            return Err("普通键未发生变化".into());
        }
        let planned = splice(&keymap, pid, lid, slot, &token)?;
        let preview = Preview {
            kind: "single_device_key".into(),
            token: crate::document::new_id(),
            document_id: doc.document_id.clone(),
            revision: doc.revision,
            document_sha256: digest(document_bytes),
            device_identity: current["device_identity"].clone(),
            firmware: current["firmware"].clone(),
            files: json!({"keymap":metadata(current)["keymap"],"smart_actions":metadata(current)["smart_actions"]}),
            active_indices: current
                .get("active_indices")
                .cloned()
                .unwrap_or(Value::Null),
            active_source: "未知；活动索引未用于选择目标".into(),
            profile_source_id: pid.clone(),
            layer_source_id: lid.clone(),
            logical_control: binding.logical_control.clone(),
            physical_position: "未核实；仅来源逻辑槽位".into(),
            before,
            after: token,
            planned_keymap_sha256: digest(&planned),
            apply_available: false,
            restore_available: false,
            unavailable_reason: UNAVAILABLE.into(),
        };
        Ok(Self {
            prepared: false,
            preview,
            baseline: current.clone(),
            keymap,
            planned_keymap: planned,
            smart,
        })
    }
    pub fn ensure_current(
        &self,
        doc: &LocalDocumentV2,
        document_bytes: &[u8],
        current: &Value,
    ) -> Result<(), String> {
        if self.preview.document_id != doc.document_id
            || self.preview.revision != doc.revision
            || self.preview.document_sha256 != digest(document_bytes)
        {
            return Err("文档或修订已变化；请重新预览".into());
        }
        comparison::ensure_apply_baseline(doc, current)?;
        if self.baseline != *current {
            return Err("会话快照已变化；请重新读取并预览".into());
        }
        Ok(())
    }
    pub fn backup(&self, parent: &Path) -> Result<Prepared, String> {
        let root = parent.join("device-backups");
        match DirBuilder::new().mode(0o700).create(&root) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.to_string()),
        }
        let meta = fs::symlink_metadata(&root).map_err(|e| e.to_string())?;
        if !meta.is_dir()
            || meta.permissions().mode() & 0o777 != 0o700
            || meta.uid() != fs::metadata(parent).map_err(|e| e.to_string())?.uid()
        {
            return Err("备份目录不是当前文档所属的私有目录".into());
        }
        let path = root.join(&self.preview.token);
        DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .map_err(|_| "备份已存在或无法创建；没有覆盖旧包")?;
        let save = |name: &str, bytes: &[u8]| -> Result<(), String> {
            let file = path.join(name);
            let mut f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&file)
                .map_err(|e| e.to_string())?;
            f.write_all(bytes)
                .and_then(|_| f.sync_all())
                .map_err(|e| e.to_string())?;
            if fs::read(&file).map_err(|e| e.to_string())? != bytes {
                return Err("备份读回不符；未标为准备完成".into());
            }
            Ok(())
        };
        save("keymap.json", &self.keymap)?;
        save("planned-keymap.json", &self.planned_keymap)?;
        if let Some(smart) = &self.smart {
            save("smart_actions.json", smart)?;
        }
        let manifest=serde_json::to_vec_pretty(&json!({"format":"micro-input-device-backup","format_version":1,"status":"prepared-not-sent","plan":self.preview,"baseline":metadata(&self.baseline),"keymap_sha256":digest(&self.keymap),"smart_actions_sha256":self.smart.as_ref().map(|s| digest(s))})).map_err(|e| e.to_string())?;
        save("manifest.json", &manifest)?;
        for dir in [&path, &root, parent] {
            fs::File::open(dir)
                .and_then(|f| f.sync_all())
                .map_err(|e| e.to_string())?;
        }
        Ok(Prepared {
            token: self.preview.token.clone(),
            backup_path: path.to_string_lossy().into(),
            manifest_sha256: digest(&manifest),
            status: "prepared-not-sent".into(),
            unavailable_reason: UNAVAILABLE.into(),
        })
    }
}

/// Frozen candidate issued only after the private backup has been verified.
/// It carries no transport authority. Production execution is not implemented.
#[derive(Clone)]
pub struct PreparedCandidate {
    operation_id: String,
    session_id: u64,
    original: Vec<u8>,
    candidate: Vec<u8>,
    smart: Option<Vec<u8>>,
}
impl PreparedCandidate {
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }
    pub fn session_id(&self) -> u64 {
        self.session_id
    }
    pub fn original(&self) -> &[u8] {
        &self.original
    }
    pub fn candidate(&self) -> &[u8] {
        &self.candidate
    }
    pub fn smart_actions(&self) -> Option<&[u8]> {
        self.smart.as_deref()
    }
}
impl Pending {
    pub(crate) fn candidate(&self) -> Result<PreparedCandidate, String> {
        if !self.prepared {
            return Err("先保存并核验原始副本".into());
        }
        Ok(PreparedCandidate {
            operation_id: self.preview.token.clone(),
            session_id: self.preview.device_identity["session_id"]
                .as_u64()
                .ok_or("会话缺失")?,
            original: self.keymap.clone(),
            candidate: self.planned_keymap.clone(),
            smart: self.smart.clone(),
        })
    }
    fn lighting(
        doc: &LocalDocumentV2,
        current: &Value,
        document_bytes: &[u8],
        reference: &LocalDocumentV2,
        keymap: Vec<u8>,
        smart: Option<Vec<u8>>,
    ) -> Result<Self, String> {
        let mut normalized = doc.editable.graph.clone();
        for layer in normalized.layers.values_mut() {
            layer.lighting = None;
        }
        if normalized != reference.editable.graph {
            return Err("灯光提交不能混合键位、动作或结构修改".into());
        }
        let root: &RawValue = serde_json::from_slice(&keymap).map_err(|e| e.to_string())?;
        let mut patches = vec![];
        let mut changes = vec![];
        for (id, layer) in &doc.editable.graph.layers {
            let Some(target) = &layer.lighting else {
                continue;
            };
            let source = &doc.editable.source_refs[id];
            if source["kind"] != "source-layer" {
                return Err("灯光层来源不明确".into());
            }
            let pid = &source["profile_source_id"];
            let lid = &source["source_id"];
            let profile = unique(array(field(&object(root)?, "profiles")?)?, pid)?;
            let raw_layer = unique(array(field(&object(profile)?, "layers")?)?, lid)?;
            let lights = object(field(&object(raw_layer)?, "lights")?)?;
            for zone in [
                crate::lighting::Zone::Backlight,
                crate::lighting::Zone::Underglow,
            ] {
                let raw = field(&lights, zone.name())?;
                let before: Value = serde_json::from_str(raw.get()).map_err(|e| e.to_string())?;
                let after = target.zone(zone);
                if &before == after {
                    continue;
                }
                let a = after.as_object().ok_or("目标灯区结构未知")?;
                let b = before.as_object().ok_or("原灯区结构未知")?;
                if a.keys().ne(b.keys()) {
                    return Err("不能创建或删除灯光字段".into());
                }
                let _effect: crate::lighting::Effect =
                    serde_json::from_value(before["effect"].clone())
                        .map_err(|_| "原灯效未知，不可修改")?;
                let new_effect: crate::lighting::Effect =
                    serde_json::from_value(after["effect"].clone()).map_err(|_| "目标灯效未知")?;
                let fields = object(raw)?;
                for (key, value) in a {
                    if b.get(key) == Some(value) {
                        continue;
                    }
                    let valid = match key.as_str() {
                        "effect" => true,
                        "color" => value.as_u64().is_some_and(|n| n <= 0xffffff),
                        "brightness" | "speed" | "magic" => value
                            .as_f64()
                            .is_some_and(|n| n.is_finite() && (0.0..=1.0).contains(&n)),
                        _ => false,
                    };
                    use crate::lighting::Effect::*;
                    let enabled = key == "effect"
                        || match new_effect {
                            Off => false,
                            Solid => key != "speed" && key != "magic",
                            Gradient => key != "speed",
                            Snake => key != "magic",
                            Rainbow => key != "color",
                            Breath => true,
                        };
                    // Unknown original effects were rejected above.
                    if !valid || !enabled || !crate::lighting::known_value(key, &b[key]) {
                        return Err("未知字段、超界参数或禁用参数发生变化".into());
                    }
                    let token = field(&fields, key)?;
                    let start = (token.get().as_ptr() as usize)
                        .checked_sub(keymap.as_ptr() as usize)
                        .ok_or("字节范围无效")?;
                    let end = start
                        .checked_add(token.get().len())
                        .filter(|end| *end <= keymap.len())
                        .ok_or("范围越界")?;
                    patches.push((
                        start,
                        end,
                        serde_json::to_vec(value).map_err(|e| e.to_string())?,
                    ));
                    changes.push(json!({"profile":pid,"layer":lid,"zone":zone.name(),"field":key,"before":b[key],"after":value}));
                }
            }
        }
        if patches.is_empty() {
            return Err("灯光未发生变化，无候选".into());
        }
        patches.sort_by_key(|p| p.0);
        if patches.windows(2).any(|w| w[0].1 > w[1].0) {
            return Err("目标范围重叠".into());
        }
        let mut planned = keymap.clone();
        for (start, end, bytes) in patches.into_iter().rev() {
            planned.splice(start..end, bytes);
        }
        crate::parse_device_files(&planned, smart.as_deref()).map_err(|e| e.to_string())?;
        let preview = Preview {
            kind: "lighting".into(),
            token: crate::document::new_id(),
            document_id: doc.document_id.clone(),
            revision: doc.revision,
            document_sha256: digest(document_bytes),
            device_identity: current["device_identity"].clone(),
            firmware: current["firmware"].clone(),
            files: metadata(current),
            active_indices: current["active_indices"].clone(),
            active_source: "不按活动索引选择编译目标".into(),
            profile_source_id: Value::Null,
            layer_source_id: Value::Null,
            logical_control: String::new(),
            physical_position: "灯光字段".into(),
            before: "原始灯光字段".into(),
            after: serde_json::to_string(&changes).map_err(|e| e.to_string())?,
            planned_keymap_sha256: digest(&planned),
            apply_available: false,
            restore_available: false,
            unavailable_reason: UNAVAILABLE.into(),
        };
        Ok(Self {
            prepared: false,
            preview,
            baseline: current.clone(),
            keymap,
            planned_keymap: planned,
            smart,
        })
    }
}
fn known_passthrough(layer: &crate::Layer, slot: usize, token: &str) -> bool {
    let Some(binding) = token.strip_prefix("KV_OAI_") else {
        return false;
    };
    let logical = if matches!(binding, "AG11" | "ACT11") {
        Some("logical-11")
    } else {
        crate::host_router::verified_physical_mapping(binding).map(|s| s.logical_switch)
    };
    if logical != Some(format!("logical-{slot}").as_str()) {
        return false;
    }
    let suffix = binding
        .strip_prefix("AG")
        .or_else(|| binding.strip_prefix("ACT"));
    layer
        .keys
        .iter()
        .filter(|b| match b {
            crate::Binding::Unknown(Value::String(s)) => {
                s.strip_prefix("KV_OAI_AG")
                    .or_else(|| s.strip_prefix("KV_OAI_ACT"))
                    == suffix
            }
            _ => false,
        })
        .count()
        == 1
}
