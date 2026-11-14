//! Durable offline operation evidence. No transport or automatic recovery.
use crate::{
    apply::{Prepared, PreparedCandidate},
    document::LocalDocumentV2,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, DirBuilder, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};
const LIMIT: u64 = 16 * 1024 * 1024;
pub(crate) fn digest(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    Prepared,
    Revalidating,
    Writing,
    Verifying,
    Verified,
    VerifiedLocalSaveFailed,
    NotSent,
    DeviceStateUnknown,
    OriginalVerified,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Failure {
    Persistence,
    Identity,
    Baseline,
    RequestBudget,
    Transport,
    Readback,
    LocalSave,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Status {
    pub operation_id: String,
    pub phase: Phase,
    pub confirmed_chunks: usize,
    pub material_sha256: String,
    pub target_document_sha256: String,
    pub device_verified: bool,
    pub local_saved: bool,
    pub restores: Option<String>,
    pub resolved_by: Option<String>,
    pub error_category: Option<Failure>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    sha256: String,
    status: Status,
}
// Private decoding, not a public way to construct a sendable candidate.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Frozen {
    operation_id: String,
    session_id: u64,
    backup: Prepared,
    document_path: PathBuf,
    document_sha256: String,
    baseline: Value,
    restores: Option<String>,
}
impl Frozen {
    fn from_plan(p: &PreparedCandidate) -> Self {
        let mut baseline = p.baseline.clone();
        if let Some(o) = baseline.as_object_mut() {
            o.remove("comparison_config");
        }
        for key in ["keymap", "smart_actions"] {
            if let Some(o) = baseline[key].as_object_mut() {
                o.remove("bytes");
            }
        }
        Self {
            operation_id: p.operation_id.clone(),
            session_id: p.session_id,
            backup: p.backup.clone(),
            document_path: p.document_path.clone(),
            document_sha256: digest(&p.document_bytes),
            baseline,
            restores: p.restores.clone(),
        }
    }
    fn plan(self, path: &Path) -> Result<PreparedCandidate, String> {
        let root = Path::new(&self.backup.backup_path);
        let original = read(&root.join("keymap.json"))?;
        let candidate = read(&root.join("planned-keymap.json"))?;
        let smart = if self.baseline["smart_actions"]["state"] == "missing" {
            None
        } else {
            Some(read(&root.join("smart_actions.json"))?)
        };
        let document_bytes = read(&path.join("document-before.json"))?;
        if digest(&document_bytes) != self.document_sha256 {
            return Err("原文档材料摘要不符".into());
        }
        let mut baseline = self.baseline;
        baseline["keymap"]["bytes"] = serde_json::json!(original);
        if let Some(b) = &smart {
            baseline["smart_actions"]["bytes"] = serde_json::json!(b);
        }
        Ok(PreparedCandidate {
            operation_id: self.operation_id,
            session_id: self.session_id,
            original,
            candidate,
            smart,
            backup: self.backup,
            document_path: self.document_path,
            document_bytes,
            baseline,
            restores: self.restores,
        })
    }
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.permissions().mode() & 0o777 != 0o600 || meta.len() > LIMIT {
        return Err("操作材料不是有界私有文件".into());
    }
    let mut b = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(LIMIT + 1)
        .read_to_end(&mut b)
        .map_err(|e| e.to_string())?;
    if b.len() as u64 > LIMIT {
        return Err("操作材料过大".into());
    }
    Ok(b)
}
fn sync_dir(path: &Path) -> Result<(), String> {
    fs::File::open(path)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())
}
fn create(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() as u64 > LIMIT {
        return Err("操作材料过大".into());
    }
    let mut f = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| e.to_string())?;
    f.write_all(bytes)
        .and_then(|_| f.sync_all())
        .map_err(|e| e.to_string())?;
    if read(path)? != bytes {
        return Err("操作材料读回不符".into());
    }
    Ok(())
}
fn atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension(format!("{}.pending", crate::document::new_id()));
    let result = (|| {
        create(&tmp, bytes)?;
        fs::rename(&tmp, path).map_err(|e| e.to_string())?;
        sync_dir(path.parent().ok_or("无父目录")?)
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}
pub(crate) fn same_device(a: &Value, b: &Value) -> Result<(), String> {
    let x = &a["device_identity"];
    let y = &b["device_identity"];
    // A live native connection may restore its own exact backup. Interface paths
    // never authorize restoration after reconnect/restart or to another session.
    let a_fw = a["firmware"].as_str();
    let b_fw = b["firmware"].as_str();
    if a_fw.is_some_and(crate::is_supported_native_firmware)
        && a_fw == b_fw
        && x["confidence"] == "observed-interface"
        && y == x
        && x["session_id"].as_u64().is_some()
        && x["connection_nonce"]
            .as_str()
            .is_some_and(|v| !v.is_empty())
        && x["interface"].as_str().is_some_and(|v| !v.is_empty())
        && x["backend"] == "hidapi-2.6.7/macOS/numbered-0x06"
    {
        return Ok(());
    }
    if a_fw != b_fw
        || !b_fw.is_some_and(crate::is_supported_config_firmware)
        || x["confidence"] != "verified-serial"
        || y["confidence"] != "verified-serial"
        || x["serial"].as_str().is_none_or(str::is_empty)
        || x["serial"] != y["serial"]
        || y["session_id"].as_u64().is_none()
    {
        return Err("恢复要求明确相同的设备身份和固件".into());
    }
    Ok(())
}
fn validate_plan(p: &PreparedCandidate) -> Result<(), String> {
    let root = Path::new(&p.backup.backup_path);
    let m = read(&root.join("manifest.json"))?;
    if digest(&m) != p.backup.manifest_sha256 {
        return Err("副本 manifest 摘要不符".into());
    }
    let m: Value = serde_json::from_slice(&m).map_err(|e| e.to_string())?;
    if m["plan"]["token"] != p.operation_id
        || m["keymap_sha256"] != digest(&p.original)
        || m["plan"]["planned_keymap_sha256"] != digest(&p.candidate)
        || m["plan"]["document_sha256"] != digest(&p.document_bytes)
        || m["plan"]["device_identity"] != p.baseline["device_identity"]
        || m["plan"]["firmware"] != p.baseline["firmware"]
        || p.baseline["device_identity"]["session_id"].as_u64() != Some(p.session_id)
    {
        return Err("候选与副本关联不符".into());
    }
    if read(&root.join("keymap.json"))? != p.original
        || read(&root.join("planned-keymap.json"))? != p.candidate
    {
        return Err("副本原字节不符".into());
    }
    match &p.smart {
        Some(b)
            if read(&root.join("smart_actions.json"))? != *b
                || m["smart_actions_sha256"] != digest(b) =>
        {
            return Err("smart 副本不符".into())
        }
        None if root.join("smart_actions.json").exists()
            || !m["smart_actions_sha256"].is_null() =>
        {
            return Err("smart 有无不符".into())
        }
        _ => {}
    }
    let d: LocalDocumentV2 =
        serde_json::from_slice(&p.document_bytes).map_err(|e| e.to_string())?;
    d.validate()?;
    if m["plan"]["document_id"] != d.document_id || m["plan"]["revision"] != d.revision {
        return Err("文档身份不符".into());
    }
    Ok(())
}
/// One directory is the durable consumption marker. Reopening it never sends anything.
pub struct OperationRecord {
    path: PathBuf,
    status: Status,
    chunk_count: usize,
}
impl OperationRecord {
    pub fn begin(plan: &PreparedCandidate) -> Result<Self, String> {
        validate_plan(plan)?;
        let _lock = crate::storage::Lock::acquire(plan.document_path.with_extension("lock"))?;
        ensure_available(&plan.document_path, plan.restores.as_deref())?;
        if crate::storage::read(&plan.document_path)?.ok_or("文档缺失")? != plan.document_bytes
        {
            return Err("文档已变化".into());
        }
        let doc: LocalDocumentV2 =
            serde_json::from_slice(&plan.document_bytes).map_err(|e| e.to_string())?;
        let config = crate::parse_device_files(&plan.candidate, plan.smart.as_deref())
            .map_err(|e| e.to_string())?;
        let mut baseline = plan.baseline.clone();
        baseline["keymap"] = file_value(&plan.candidate);
        let next = crate::comparison::Comparison::new(&doc, &config, baseline)?
            .resolve(&doc, crate::comparison::Choice::KeepLocal)?
            .ok_or("无本地更新")?;
        let target = serde_json::to_vec_pretty(&next).map_err(|e| e.to_string())?;
        if target.len() > 4 * 1024 * 1024 {
            return Err("本地目标超过文档大小预算".into());
        }
        let material = serde_json::to_vec(&Frozen::from_plan(plan)).map_err(|e| e.to_string())?;
        let path = plan.record_path();
        DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .map_err(|_| "操作已占用或记录目录不可创建")?;
        create(&path.join("document-before.json"), &plan.document_bytes)?;
        create(&path.join("material.json"), &material)?;
        create(&path.join("document-target.json"), &target)?;
        let record = Self {
            path,
            chunk_count: plan.candidate.len().div_ceil(3072),
            status: Status {
                operation_id: plan.operation_id.clone(),
                phase: Phase::Prepared,
                confirmed_chunks: 0,
                material_sha256: digest(&material),
                target_document_sha256: digest(&target),
                device_verified: false,
                local_saved: false,
                restores: plan.restores.clone(),
                resolved_by: None,
                error_category: None,
            },
        };
        record.save()?;
        sync_dir(record.path.parent().unwrap())?;
        Ok(record)
    }
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if !meta.is_dir() || meta.permissions().mode() & 0o777 != 0o700 {
            return Err("操作目录不私有".into());
        }
        let env: Envelope =
            serde_json::from_slice(&read(&path.join("status.json"))?).map_err(|e| e.to_string())?;
        if digest(&serde_json::to_vec(&env.status).map_err(|e| e.to_string())?) != env.sha256 {
            return Err("操作记录校验失败".into());
        }
        let mut record = Self {
            path,
            status: env.status,
            chunk_count: 0,
        };
        let plan = record.plan()?;
        record.chunk_count = plan.candidate.len().div_ceil(3072);
        if record.status.operation_id != plan.operation_id
            || record.status.restores != plan.restores
            || record.status.confirmed_chunks > plan.candidate.len().div_ceil(3072)
            || (record.status.local_saved && !record.status.device_verified)
            || (record.status.device_verified
                && !matches!(
                    record.status.phase,
                    Phase::Verified | Phase::VerifiedLocalSaveFailed
                ))
        {
            return Err("操作记录状态不一致".into());
        }
        record.target()?;
        if let Some(id) = &record.status.resolved_by {
            if uuid::Uuid::parse_str(id).is_err() || id == &record.status.operation_id {
                return Err("恢复关联无效".into());
            }
            let linked_path = Path::new(&plan.backup.backup_path)
                .parent()
                .ok_or("无备份目录")?
                .join(id)
                .join("operation");
            let env: Envelope = serde_json::from_slice(&read(&linked_path.join("status.json"))?)
                .map_err(|e| e.to_string())?;
            if digest(&serde_json::to_vec(&env.status).map_err(|e| e.to_string())?) != env.sha256
                || !env.status.device_verified
                || (env.status.restores.as_ref() != Some(&record.status.operation_id)
                    && (record.status.restores.is_none()
                        || env.status.restores != record.status.restores))
            {
                return Err("恢复成功证据缺失".into());
            }
        }
        Ok(record)
    }
    pub fn status(&self) -> &Status {
        &self.status
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn restart_phase(&self) -> Phase {
        match self.status.phase {
            Phase::Prepared | Phase::Revalidating => Phase::NotSent,
            Phase::Writing | Phase::Verifying => Phase::DeviceStateUnknown,
            p => p,
        }
    }
    pub(crate) fn plan(&self) -> Result<PreparedCandidate, String> {
        let material = read(&self.path.join("material.json"))?;
        if digest(&material) != self.status.material_sha256 {
            return Err("冻结材料摘要不符".into());
        }
        let p = serde_json::from_slice::<Frozen>(&material)
            .map_err(|e| e.to_string())?
            .plan(&self.path)?;
        validate_plan(&p)?;
        if p.record_path() != self.path {
            return Err("操作路径不符".into());
        }
        Ok(p)
    }
    fn target(&self) -> Result<Vec<u8>, String> {
        let b = read(&self.path.join("document-target.json"))?;
        if digest(&b) != self.status.target_document_sha256 {
            return Err("本地修复材料损坏".into());
        }
        Ok(b)
    }
    fn save(&self) -> Result<(), String> {
        let b = serde_json::to_vec(&self.status).map_err(|e| e.to_string())?;
        atomic(
            &self.path.join("status.json"),
            &serde_json::to_vec(&Envelope {
                sha256: digest(&b),
                status: self.status.clone(),
            })
            .map_err(|e| e.to_string())?,
        )
    }
    pub fn advance(&mut self, phase: Phase, confirmed: usize) -> Result<(), String> {
        use Phase::*;
        let allowed = matches!(
            (self.status.phase, phase),
            (Prepared, Revalidating | NotSent)
                | (Revalidating, Writing | NotSent)
                | (Writing, Writing | Verifying | DeviceStateUnknown)
                | (Verifying, Verified | DeviceStateUnknown)
                | (Verified, VerifiedLocalSaveFailed)
                | (VerifiedLocalSaveFailed, Verified)
        );
        if !allowed
            || confirmed < self.status.confirmed_chunks
            || confirmed > self.chunk_count
            || (matches!(phase, Phase::Verifying | Phase::Verified)
                && confirmed != self.chunk_count)
        {
            return Err("无效操作状态转换".into());
        }
        let old = self.status.clone();
        self.status.phase = phase;
        self.status.confirmed_chunks = confirmed;
        if phase == Verified {
            self.status.device_verified = true;
        }
        if let Err(e) = self.save() {
            if phase != Phase::Verified {
                self.status = old;
            }
            return Err(e);
        }
        Ok(())
    }
    pub fn fail(&mut self, phase: Phase, category: Failure) -> Result<(), String> {
        self.status.error_category = Some(category);
        self.advance(phase, self.status.confirmed_chunks)
    }
    /// Never opens a device. A process restart requires persisted verification evidence.
    pub fn retry_local_save(&mut self) -> Result<(), String> {
        let result = self.repair_local();
        if result.is_err() && self.status.device_verified && !self.status.local_saved {
            self.status.phase = Phase::VerifiedLocalSaveFailed;
            self.status.error_category = Some(Failure::LocalSave);
            let _ = self.save();
        }
        result
    }
    fn repair_local(&mut self) -> Result<(), String> {
        if self.status.local_saved {
            return Ok(());
        }
        if !self.status.device_verified {
            return Err("没有持久核验结果；先独立读回核对".into());
        }
        self.save()?; // Persist known verification before touching the document.
        let plan = self.plan()?;
        let target = self.target()?;
        let _lock = crate::storage::Lock::acquire(plan.document_path.with_extension("lock"))?;
        let current = crate::storage::read(&plan.document_path)?.ok_or("文档缺失")?;
        if current != target && current != plan.document_bytes {
            return Err("文档修订或原字节已变化；拒绝覆盖".into());
        }
        if current != target {
            atomic(&plan.document_path, &target)?;
        }
        if let Some(parent) = &plan.restores {
            let root = Path::new(&plan.backup.backup_path)
                .parent()
                .ok_or("无备份目录")?;
            let mut previous = Self::open(root.join(parent).join("operation"))?;
            let old = previous.plan()?;
            same_device(&old.baseline, &plan.baseline)?;
            if old.original != plan.candidate {
                return Err("恢复关联目标不符".into());
            }
            previous.status.resolved_by = Some(plan.operation_id.clone());
            previous.save()?;
            for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
                let path = entry.map_err(|e| e.to_string())?.path().join("operation");
                if path == self.path || !path.exists() {
                    continue;
                }
                let mut sibling = Self::open(path)?;
                if sibling.status.restores.as_ref() == Some(parent)
                    && matches!(
                        sibling.status.phase,
                        Phase::DeviceStateUnknown | Phase::NotSent | Phase::OriginalVerified
                    )
                {
                    if sibling.plan()?.candidate != plan.candidate {
                        return Err("恢复链目标不一致".into());
                    }
                    sibling.status.resolved_by = Some(plan.operation_id.clone());
                    sibling.save()?;
                }
            }
        }
        self.status.phase = Phase::Verified;
        self.status.local_saved = true;
        self.status.error_category = None;
        if let Err(e) = self.save() {
            self.status.local_saved = false;
            self.status.phase = Phase::VerifiedLocalSaveFailed;
            return Err(e);
        }
        Ok(())
    }
    pub fn prepare_restore(&self, current: Value) -> Result<PreparedCandidate, String> {
        PreparedCandidate::restore(self, current)
    }
    /// Explicit readback classification is read-only; it does not restart an operation.
    pub fn classify(&self, current: &Value) -> Result<&'static str, String> {
        let p = self.plan()?;
        same_device(&p.baseline, current)?;
        let key = checked_file(current, "keymap")?.ok_or("缺少 keymap")?;
        let smart = checked_file(current, "smart_actions")?;
        Ok(if smart == p.smart && key == p.candidate {
            "candidate"
        } else if smart == p.smart && key == p.original {
            "original"
        } else {
            "other"
        })
    }
    pub fn confirm_readback(&mut self, current: &Value) -> Result<(), String> {
        let classification = self.classify(current)?;
        self.status.resolved_by = None;
        self.status.error_category = None;
        match classification {
            "candidate" => {
                self.status.phase = Phase::Verified;
                self.status.device_verified = true;
                self.status.local_saved = false;
            }
            "original" => {
                self.status.phase = Phase::OriginalVerified;
                self.status.device_verified = false;
                self.status.local_saved = false;
            }
            _ => {
                self.status.phase = Phase::DeviceStateUnknown;
                self.status.device_verified = false;
                self.status.local_saved = false;
            }
        }
        self.save()
    }
}
pub fn file_value(bytes: &[u8]) -> Value {
    use sha1::Sha1;
    serde_json::json!({"state":"present","size":bytes.len(),"sha1":format!("{:x}",Sha1::digest(bytes)),"bytes":bytes})
}
fn checked_file(v: &Value, key: &str) -> Result<Option<Vec<u8>>, String> {
    if key == "smart_actions" && v[key]["state"] == "missing" {
        return Ok(None);
    }
    let b: Vec<u8> = serde_json::from_value(v[key]["bytes"].clone()).map_err(|_| "缺少文件字节")?;
    if v[key] != file_value(&b) {
        return Err("文件摘要或大小不符".into());
    }
    Ok(Some(b))
}
/// A failed/corrupt record cannot silently unlock an affected document.
pub fn ensure_document_editable(path: &Path) -> Result<(), String> {
    ensure_available(path, None)
}
fn ensure_available(path: &Path, permitted: Option<&str>) -> Result<(), String> {
    let canonical = if path.exists() {
        fs::canonicalize(path).map_err(|e| e.to_string())?
    } else {
        path.to_path_buf()
    };
    let path = canonical.as_path();
    let root = path.parent().ok_or("无文档目录")?.join("device-backups");
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let dir = entry.map_err(|e| e.to_string())?.path().join("operation");
        if !dir.exists() {
            continue;
        }
        let record = OperationRecord::open(&dir)?;
        let p = record.plan()?;
        let replaced_restore = permitted.is_some()
            && p.restores.as_deref() == permitted
            && matches!(
                record.status.phase,
                Phase::DeviceStateUnknown | Phase::NotSent | Phase::OriginalVerified
            );
        if p.document_path == path
            && permitted != Some(p.operation_id())
            && !replaced_restore
            && record.status.resolved_by.is_none()
            && !record.status.local_saved
            && !matches!(
                record.status.phase,
                Phase::NotSent | Phase::OriginalVerified
            )
        {
            return Err("设备操作待核对或本地修复；当前文档只读".into());
        }
    }
    Ok(())
}

impl OperationRecord {
    pub fn document_path(&self) -> Result<PathBuf, String> {
        Ok(self.plan()?.document_path)
    }
}
