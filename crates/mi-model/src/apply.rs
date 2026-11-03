//! Compile reviewed configuration changes without transport or device side effects.
use crate::{comparison, document::LocalDocumentV2, draft::ActionKind};
use serde::{Deserialize, Serialize};
use serde_json::{json, value::RawValue, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, DirBuilder, OpenOptions},
    io::Write,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub const UNAVAILABLE: &str = "请核验当前连接并保存原配置副本后应用";
#[derive(Clone, Serialize, Deserialize)]
pub struct Preview {
    pub kind: String,
    #[serde(default)]
    pub changes: Vec<Value>,
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
#[derive(Clone, Serialize, Deserialize)]
pub struct Prepared {
    pub token: String,
    pub backup_path: String,
    pub manifest_sha256: String,
    pub status: String,
    pub unavailable_reason: String,
}
pub(crate) struct Pending {
    pub prepared: Option<Prepared>,
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
            return Err("来源引用或结构变化不在配置应用范围内".into());
        }
        let mut normalized = doc.editable.graph.clone();
        for layer in normalized.layers.values_mut() {
            layer.lighting = None;
        }
        let mut targets = Vec::new();
        let mut used_actions = std::collections::BTreeSet::new();
        for (lid, layer) in &mut normalized.layers {
            for (bid, binding) in &mut layer.controls {
                if let Some(aid) = binding.action_id.take() {
                    used_actions.insert(aid.clone());
                    targets.push((lid.clone(), bid.clone(), aid));
                }
            }
        }
        if used_actions.len() != normalized.actions.len() {
            return Err("存在未绑定的动作；不能应用".into());
        }
        normalized.actions.clear();
        if normalized != reference.editable.graph {
            return Err("结构、模式或其他字段变化不在配置应用范围内".into());
        }
        let mut planned = keymap.clone();
        let mut changes = Vec::new();
        for (local_lid, bid, aid) in targets {
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
                crate::Binding::Unknown(Value::String(t)) if known_passthrough(l, slot, t) => {
                    t.clone()
                }
                _ => return Err("原绑定未知或高级类型；不能准备覆盖".into()),
            };
            if before == token {
                continue;
            }
            planned = splice(&planned, pid, lid, slot, &token)?;
            changes.push(json!({"kind":"key","profile":pid,"layer":lid,"control":binding.logical_control,"before":before,"after":token}));
        }
        let key_count = changes.len();
        let (lighting_patches, lighting_changes) = Self::lighting_patches(doc, &planned)?;
        for (start, end, bytes) in lighting_patches.into_iter().rev() {
            planned.splice(start..end, bytes);
        }
        changes.extend(lighting_changes);
        if changes.is_empty() {
            return Err("没有键位或灯光变化".into());
        }
        crate::parse_device_files(&planned, smart.as_deref()).map_err(|e| e.to_string())?;
        let single_key = key_count == 1 && changes.len() == 1;
        let first = &changes[0];
        let preview = Preview {
            kind: if single_key {
                "single_device_key"
            } else if key_count == 0 {
                "lighting"
            } else {
                "configuration"
            }
            .into(),
            changes: changes.clone(),
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
            profile_source_id: if single_key {
                first["profile"].clone()
            } else {
                Value::Null
            },
            layer_source_id: if single_key {
                first["layer"].clone()
            } else {
                Value::Null
            },
            logical_control: first["control"].as_str().unwrap_or("").into(),
            physical_position: "未核实；仅来源逻辑槽位".into(),
            before: if single_key {
                first["before"].as_str().unwrap_or("").into()
            } else {
                "当前设备配置".into()
            },
            after: if single_key {
                first["after"].as_str().unwrap_or("").into()
            } else {
                format!(
                    "{} 个键位、{} 个灯光字段",
                    key_count,
                    changes.len() - key_count
                )
            },
            planned_keymap_sha256: digest(&planned),
            apply_available: false,
            restore_available: false,
            unavailable_reason: UNAVAILABLE.into(),
        };
        Ok(Self {
            prepared: None,
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
            backup_path: fs::canonicalize(&path)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .into(),
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
    pub(crate) operation_id: String,
    pub(crate) session_id: u64,
    pub(crate) original: Vec<u8>,
    pub(crate) candidate: Vec<u8>,
    pub(crate) smart: Option<Vec<u8>>,
    pub(crate) backup: Prepared,
    pub(crate) document_path: PathBuf,
    pub(crate) document_bytes: Vec<u8>,
    pub(crate) baseline: Value,
    pub(crate) restores: Option<String>,
}
impl PreparedCandidate {
    pub fn record_path(&self) -> PathBuf {
        PathBuf::from(&self.backup.backup_path).join("operation")
    }
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
    pub(crate) fn candidate(
        &self,
        document_path: PathBuf,
        document_bytes: Vec<u8>,
    ) -> Result<PreparedCandidate, String> {
        let backup = self.prepared.clone().ok_or("先保存并核验原始副本")?;
        Ok(PreparedCandidate {
            backup,
            document_path: fs::canonicalize(document_path).map_err(|e| e.to_string())?,
            document_bytes,
            baseline: self.baseline.clone(),
            restores: None,
            operation_id: self.preview.token.clone(),
            session_id: self.preview.device_identity["session_id"]
                .as_u64()
                .ok_or("会话缺失")?,
            original: self.keymap.clone(),
            candidate: self.planned_keymap.clone(),
            smart: self.smart.clone(),
        })
    }
    fn lighting_patches(
        doc: &LocalDocumentV2,
        keymap: &[u8],
    ) -> Result<(Vec<(usize, usize, Vec<u8>)>, Vec<Value>), String> {
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
            let layer_fields = object(raw_layer)?;
            if !layer_fields.contains_key("lights") {
                if !target.complete_known() {
                    return Err("新增灯光必须为完整已知字段".into());
                }
                let end = raw_layer.get().as_ptr() as usize - keymap.as_ptr() as usize
                    + raw_layer.get().len()
                    - 1;
                let bytes = format!(
                    ",\"lights\":{}",
                    serde_json::to_string(target).map_err(|e| e.to_string())?
                )
                .into_bytes();
                patches.push((end, end, bytes));
                changes.push(json!({"kind":"lighting","profile":pid,"layer":lid,"field":"lights","before":null,"after":target}));
                continue;
            }
            let lights = object(field(&layer_fields, "lights")?)?;
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
                    changes.push(json!({"kind":"lighting","profile":pid,"layer":lid,"zone":zone.name(),"field":key,"before":b[key],"after":value}));
                }
            }
        }
        patches.sort_by_key(|p| p.0);
        if patches.windows(2).any(|w| w[0].1 > w[1].0) {
            return Err("目标范围重叠".into());
        }
        Ok((patches, changes))
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

impl PreparedCandidate {
    pub(crate) fn restore(
        record: &crate::operation_record::OperationRecord,
        current: Value,
    ) -> Result<Self, String> {
        let prior = record.plan()?;
        crate::operation_record::same_device(&prior.baseline, &current)?;
        let keymap = bytes(&current, "keymap")?.ok_or("缺少现状文件")?;
        let smart = bytes(&current, "smart_actions")?;
        let target = prior.original.clone();
        crate::parse_device_files(&target, smart.as_deref()).map_err(|e| e.to_string())?;
        let document_bytes = crate::storage::read(&prior.document_path)?.ok_or("文档缺失")?;
        let draft = crate::draft::Draft::from_bytes(&document_bytes)?;
        if draft.read_only || draft.loaded_format != 2 {
            return Err("恢复不能覆盖未知本地格式".into());
        }
        let doc = draft.document;
        let prior_doc: LocalDocumentV2 =
            serde_json::from_slice(&prior.document_bytes).map_err(|e| e.to_string())?;
        if doc.document_id != prior_doc.document_id {
            return Err("恢复关联文档身份已变化".into());
        }
        doc.validate()?;
        if keymap.len() > 4 * 1024 * 1024
            || smart.as_ref().is_some_and(|b| b.len() > 4 * 1024 * 1024)
        {
            return Err("恢复现状超过读取预算".into());
        }
        let mut preview: Preview = serde_json::from_slice::<Value>(
            &fs::read(PathBuf::from(&prior.backup.backup_path).join("manifest.json"))
                .map_err(|e| e.to_string())?,
        )
        .and_then(|v| serde_json::from_value(v["plan"].clone()))
        .map_err(|e| e.to_string())?;
        preview.kind = "restore-keymap".into();
        preview.changes.clear();
        preview.token = crate::document::new_id();
        preview.document_id = doc.document_id.clone();
        preview.revision = doc.revision;
        preview.document_sha256 = digest(&document_bytes);
        preview.device_identity = current["device_identity"].clone();
        preview.firmware = current["firmware"].clone();
        preview.files = metadata(&current);
        preview.before = "完整现状 keymap；可能不可解析".into();
        preview.after = "关联原副本；保留现状 smart_actions".into();
        preview.planned_keymap_sha256 = digest(&target);
        let mut pending = Pending {
            prepared: None,
            preview,
            baseline: current,
            keymap,
            planned_keymap: target,
            smart,
        };
        let backup = pending.backup(prior.document_path.parent().ok_or("无效文档路径")?)?;
        pending.prepared = Some(backup);
        let mut result = pending.candidate(prior.document_path, document_bytes)?;
        result.restores = Some(prior.operation_id);
        Ok(result)
    }
}

impl PreparedCandidate {
    pub fn preview(&self) -> Result<Preview, String> {
        let bytes = fs::read(Path::new(&self.backup.backup_path).join("manifest.json"))
            .map_err(|e| e.to_string())?;
        if digest(&bytes) != self.backup.manifest_sha256 {
            return Err("副本校验失败".into());
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        serde_json::from_value(value["plan"].clone()).map_err(|e| e.to_string())
    }
}
