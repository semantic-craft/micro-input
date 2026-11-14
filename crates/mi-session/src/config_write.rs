//! Plan-bound keymap application and exact-byte restoration; never an arbitrary RPC API.
use super::*;
use mi_model::{
    apply::PreparedCandidate,
    operation_record::{file_value, Failure, OperationRecord, Phase},
};
use std::collections::BTreeSet;
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    NotSent,
    DeviceStateUnknown,
    Verified,
    VerifiedLocalSaveFailed,
}
/// Owns consumed plan IDs for one live connection. A failure is never retried automatically.
pub struct ConfigWriter {
    session: Arc<DeviceSession>,
    native_interface: Option<String>,
    consumed: AsyncMutex<BTreeSet<String>>,
    records: Mutex<std::collections::BTreeMap<String, OperationRecord>>,
}
// If the caller drops an in-flight verification future, do not leave a reusable
// connection with a possibly changed device and an unobserved response.
struct MutationGuard<'a> {
    session: &'a DeviceSession,
    sent: bool,
    verified: bool,
}
impl Drop for MutationGuard<'_> {
    fn drop(&mut self) {
        if self.sent && !self.verified {
            self.session.disconnect();
        }
    }
}
impl ConfigWriter {
    #[cfg(feature = "synthetic-backend")]
    pub fn new(session: Arc<DeviceSession>) -> Result<Self, SessionError> {
        if session.link.backend() != mi_transport::LinkBackend::SyntheticNumbered {
            return Err(
                ProtoError::Unverified("native configuration writing remains disabled").into(),
            );
        }
        Ok(Self {
            session,
            native_interface: None,
            consumed: AsyncMutex::new(BTreeSet::new()),
            records: Mutex::new(std::collections::BTreeMap::new()),
        })
    }
    pub fn native(session: Arc<DeviceSession>, interface: String) -> Result<Self, SessionError> {
        if interface.is_empty()
            || session.link.backend() != mi_transport::LinkBackend::MacHidapi267Numbered
        {
            return Err(ProtoError::Unverified("native numbered backend required").into());
        }
        Ok(Self {
            session,
            native_interface: Some(interface),
            consumed: AsyncMutex::new(BTreeSet::new()),
            records: Mutex::new(std::collections::BTreeMap::new()),
        })
    }
    fn is_supported_firmware(&self, version: Option<&str>) -> bool {
        let Some(version) = version else {
            return false;
        };
        if self.native_interface.is_some() {
            mi_model::is_supported_native_firmware(version)
        } else {
            version == "synthetic-0.6.3"
        }
    }
    fn baseline(&self, firmware: &str, files: &DeviceFiles) -> Value {
        let mut value = synthetic_baseline(self.session.id, files);
        if let Some(interface) = &self.native_interface {
            value["device_identity"] = serde_json::json!({"confidence":"observed-interface","connection_nonce":self.session.connection_nonce,"interface":interface,"session_id":self.session.id,"backend":self.session.backend_name()});
            value["firmware"] = serde_json::json!(firmware);
        }
        value
    }
    /// Revalidate the supported backend and independently read complete files.
    pub async fn inspect(&self) -> Result<serde_json::Value, SessionError> {
        let info = self.session.read_identity_info().await?;
        let Some(firmware) = info
            .version
            .clone()
            .filter(|v| self.is_supported_firmware(Some(v)))
        else {
            return Err(SessionError::Config("identity unverified".into()));
        };
        self.session
            .guarded(async {
                let mut q = self.session.queue.lock().await;
                let files = self.session.read_files_locked(&mut q).await?;
                let mut base = self.baseline(&firmware, &files);
                base["active_indices"] =
                    serde_json::json!({"profile":info.profile_index,"layer":info.layer_index});
                Ok(base)
            })
            .await
    }
    pub async fn prepare_restore(
        &self,
        record: &OperationRecord,
    ) -> Result<PreparedCandidate, String> {
        let current = self.inspect().await.map_err(|e| e.to_string())?;
        record.prepare_restore(current)
    }
    /// Independent explicit readback after restart; no automatic write or local update.
    pub async fn confirm_readback(&self, record: &mut OperationRecord) -> Result<(), String> {
        let current = self.inspect().await.map_err(|e| e.to_string())?;
        record.confirm_readback(&current)
    }
    pub fn retry_local_save(&self, id: &str) -> Result<(), String> {
        self.records
            .lock()
            .unwrap()
            .get_mut(id)
            .ok_or("本进程没有核验结果")?
            .retry_local_save()
    }
    pub async fn apply(&self, plan: &PreparedCandidate) -> Outcome {
        self.apply_observed(plan, |_, _| {}).await
    }
    /// Checkpoint observation used by offline fault tests and durable state tracking.
    pub async fn apply_observed(
        &self,
        plan: &PreparedCandidate,
        mut observe: impl FnMut(Checkpoint, &std::path::Path),
    ) -> Outcome {
        if plan.session_id() != self.session.id {
            return Outcome::NotSent;
        }
        let mut consumed = self.consumed.lock().await;
        if !consumed.insert(plan.operation_id().into()) {
            return Outcome::NotSent;
        }
        let mut record = match OperationRecord::begin(plan) {
            Ok(r) => r,
            Err(_) => return Outcome::NotSent,
        };
        let mut guard = MutationGuard {
            session: &self.session,
            sent: false,
            verified: false,
        };
        let mut failure = Failure::Persistence;
        let result = self
            .session
            .guarded(async {
                observe(Checkpoint::BeforeRevalidate, record.path());
                record
                    .advance(Phase::Revalidating, 0)
                    .map_err(SessionError::Config)?;
                failure = Failure::Identity;
                let info = self.session.read_identity_info().await?;
                let Some(firmware) = info
                    .version
                    .as_deref()
                    .filter(|v| self.is_supported_firmware(Some(v)))
                else {
                    return Err(SessionError::Config("identity unverified".into()));
                };
                if self.native_interface.is_some() {
                    let active = plan.preview().map_err(SessionError::Config)?.active_indices;
                    if info.profile_index.is_none()
                        || info.layer_index.is_none()
                        || active["profile"].as_u64() != info.profile_index.map(u64::from)
                        || active["layer"].as_u64() != info.layer_index.map(u64::from)
                    {
                        return Err(SessionError::Config("active indices changed".into()));
                    }
                }
                let mut q = self.session.queue.lock().await;
                failure = Failure::Transport;
                let current = self.session.read_files_locked(&mut q).await?;
                failure = Failure::Baseline;
                if current.keymap.bytes.as_ref() != plan.original()
                    || current.smart_actions.as_ref().map(|f| f.bytes.as_ref())
                        != plan.smart_actions()
                {
                    return Err(SessionError::Config("baseline changed".into()));
                }
                failure = Failure::Identity;
                record
                    .classify(&self.baseline(firmware, &current))
                    .map_err(SessionError::Config)?;
                let chunks = plan.candidate().len().div_ceil(3072);
                let reads = chunks
                    + plan.smart_actions().map_or(0, |b| b.len().div_ceil(3072))
                    + 2
                    + usize::from(self.native_interface.is_some());
                failure = Failure::RequestBudget;
                if chunks == 0 || usize::from(q.next_id) + chunks + reads > 999 {
                    return Err(SessionError::Config("request budget insufficient".into()));
                }
                if let Some(snapshot) = self.session.snapshot.lock().unwrap().as_mut() {
                    snapshot.stale = true;
                }
                for (n, offset) in (0..plan.candidate().len()).step_by(3072).enumerate() {
                    let expected_id = q.next_id;

                    observe(Checkpoint::BeforeWrite(n), record.path());
                    failure = Failure::Persistence;
                    record
                        .advance(Phase::Writing, n)
                        .map_err(SessionError::Config)?;
                    guard.sent = true;
                    failure = Failure::Transport;
                    let reply = self
                        .session
                        .exchange_keymap_with_tx(&mut q, plan.candidate(), offset, None)
                        .await?;
                    mi_proto::mutation::validate_mutation_response_candidate(&reply, expected_id)?;
                    observe(Checkpoint::AfterAck(n + 1), record.path());
                    failure = Failure::Persistence;
                    record
                        .advance(Phase::Writing, n + 1)
                        .map_err(SessionError::Config)?;
                }
                record
                    .advance(Phase::Verifying, chunks)
                    .map_err(SessionError::Config)?;
                failure = Failure::Transport;
                let actual = self.session.read_files_locked(&mut q).await?;
                failure = Failure::Readback;
                if actual.keymap.bytes.as_ref() != plan.candidate()
                    || actual.smart_actions.as_ref().map(|f| f.bytes.as_ref())
                        != plan.smart_actions()
                {
                    return Err(SessionError::Config("readback differs".into()));
                }
                if self.native_interface.is_some() {
                    let status = self
                        .session
                        .exchange(&mut q, Method::DeviceStatus, Value::Null)
                        .await?;
                    if status["result"]["profile_index"].as_u64()
                        != info.profile_index.map(u64::from)
                        || status["result"]["layer_index"].as_u64()
                            != info.layer_index.map(u64::from)
                    {
                        return Err(SessionError::Config(
                            "activity changed during application".into(),
                        ));
                    }
                }
                guard.verified = true;
                observe(Checkpoint::BeforeVerifiedSave, record.path());
                failure = Failure::Persistence;
                record
                    .advance(Phase::Verified, chunks)
                    .map_err(SessionError::Config)?;
                observe(Checkpoint::BeforeLocalSave, record.path());
                failure = Failure::LocalSave;
                record.retry_local_save().map_err(SessionError::Config)?;
                Ok(())
            })
            .await;
        let outcome = match result {
            Ok(()) => Outcome::Verified,
            Err(_) if guard.verified => {
                let _ = record.fail(Phase::VerifiedLocalSaveFailed, failure);
                Outcome::VerifiedLocalSaveFailed
            }
            Err(_) if guard.sent => {
                self.session.disconnect();
                let _ = record.fail(Phase::DeviceStateUnknown, failure);
                Outcome::DeviceStateUnknown
            }
            Err(_) => {
                let _ = record.fail(Phase::NotSent, failure);
                Outcome::NotSent
            }
        };
        self.records
            .lock()
            .unwrap()
            .insert(plan.operation_id().into(), record);
        outcome
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Checkpoint {
    BeforeRevalidate,
    BeforeWrite(usize),
    AfterAck(usize),
    BeforeVerifiedSave,
    BeforeLocalSave,
}

fn synthetic_baseline(session_id: u64, files: &DeviceFiles) -> serde_json::Value {
    serde_json::json!({
        "device_identity": {"confidence":"verified-serial", "serial":"synthetic", "session_id":session_id},
        "firmware":"synthetic-0.6.3",
        "keymap":file_value(&files.keymap.bytes),
        "smart_actions":files.smart_actions.as_ref().map_or(serde_json::json!({"state":"missing"}), |f|file_value(&f.bytes))
    })
}

use mi_model::apply::{
    back_up_three_layer_write, compile_three_layer_profile, recognize_three_layer_profile,
    ThreeLayerBoard, ThreeLayerProvision,
};
use std::path::{Path, PathBuf};

pub struct ThreeLayerProvisionReceipt {
    pub board: ThreeLayerBoard,
    recovery: Option<ThreeLayerRecovery>,
}

impl ThreeLayerProvisionReceipt {
    pub fn backup_path(&self) -> Option<&Path> {
        self.recovery
            .as_ref()
            .map(|recovery| recovery.backup.as_path())
    }
}

struct ThreeLayerRecovery {
    session_id: u64,
    connection_nonce: String,
    original: DeviceFiles,
    candidate: Vec<u8>,
    backup: PathBuf,
}

fn smart_bytes(files: &DeviceFiles) -> Option<&[u8]> {
    files.smart_actions.as_ref().map(|file| file.bytes.as_ref())
}

fn exact_files(files: &DeviceFiles, keymap: &[u8], smart_actions: Option<&[u8]>) -> bool {
    files.keymap.bytes.as_ref() == keymap && smart_bytes(files) == smart_actions
}

use mi_proto::mutation::{validate_mutation_response_candidate, CHUNK_BYTES, MAX_FILE_BYTES};

fn chunks(bytes: &[u8]) -> usize {
    bytes.len().div_ceil(CHUNK_BYTES)
}

fn read_cost(keymap: &[u8], smart: Option<&[u8]>) -> usize {
    chunks(keymap) + smart.map_or(0, chunks) + 2
}

fn mutation_cost(keymap: &[u8], smart: Option<&[u8]>) -> usize {
    chunks(keymap) + 2 + read_cost(keymap, smart)
}

fn require_budget(queue: &Queue, writes: &[&[u8]], calls: usize) -> Result<(), SessionError> {
    if writes
        .iter()
        .any(|bytes| bytes.is_empty() || bytes.len() > MAX_FILE_BYTES)
        || usize::from(queue.next_id) + calls > 999
    {
        return Err(SessionError::Config("三层板载候选超过本次写入预算".into()));
    }
    Ok(())
}

impl DeviceSession {
    fn validate_three_layer_link(&self) -> Result<bool, SessionError> {
        match self.link.backend() {
            mi_transport::LinkBackend::MacHidapi267Numbered if self.has_exclusive_vendor_link() => {
                Ok(true)
            }
            #[cfg(feature = "synthetic-backend")]
            mi_transport::LinkBackend::SyntheticNumbered => Ok(false),
            _ => Err(SessionError::Config(
                "三层板载 Profile 需要独占 USB 专用接口".into(),
            )),
        }
    }

    async fn three_layer_position(
        &self,
        queue: &mut Queue,
    ) -> Result<ThreeLayerBoard, SessionError> {
        let is_native = self.validate_three_layer_link()?;
        let response = self
            .exchange(queue, Method::DeviceStatus, Value::Null)
            .await?;
        let result = identity_result(&response)?;
        let supported = result["version"].as_str().is_some_and(|v| {
            if is_native {
                mi_model::is_supported_native_firmware(v)
            } else {
                v == "synthetic-0.6.3"
            }
        });
        if !supported {
            return Err(SessionError::Config(
                "三层板载 Profile 不支持当前固件".into(),
            ));
        }
        let index = |name: &str| {
            result[name]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| SessionError::Config("设备未返回有效的 Profile 和层位".into()))
        };
        let position = ThreeLayerBoard {
            profile_index: index("profile_index")?,
            layer_index: index("layer_index")?,
        };
        *self.firmware.lock().unwrap() = result["version"].as_str().map(str::to_owned);
        Ok(position)
    }

    /// Read-only detection. Bind routing to a verified active profile on this lease.
    pub async fn inspect_three_layer_profile(
        &self,
    ) -> Result<Option<ThreeLayerBoard>, SessionError> {
        self.guarded(async {
            let mut queue = self.queue.lock().await;
            let before = self.three_layer_position(&mut queue).await?;
            let files = self.read_files_locked(&mut queue).await?;
            let config = mi_model::parse_device_files(
                &files.keymap.bytes,
                files.smart_actions.as_ref().map(|file| file.bytes.as_ref()),
            )
            .map_err(|e| SessionError::Config(e.to_string()))?;
            let Some(index) =
                recognize_three_layer_profile(&config).map_err(SessionError::Config)?
            else {
                return Ok(None);
            };
            let after = self.three_layer_position(&mut queue).await?;
            if before.profile_index != after.profile_index {
                return Err(SessionError::Config("核验期间设备切换了 Profile".into()));
            }
            if after.profile_index == index && (1..=3).contains(&after.layer_index) {
                Ok(Some(after))
            } else {
                Ok(None)
            }
        })
        .await
    }

    async fn stage_three_layer_keymap(
        &self,
        queue: &mut Queue,
        keymap: &[u8],
        smart: Option<&[u8]>,
    ) -> Result<(), SessionError> {
        let tx = self.exchange_tx_begin(queue).await?;
        for offset in (0..keymap.len()).step_by(CHUNK_BYTES) {
            let id = queue.next_id;
            let reply = self
                .exchange_keymap_with_tx(queue, keymap, offset, Some(tx))
                .await?;
            validate_mutation_response_candidate(&reply, id)?;
        }
        self.exchange_tx_commit(queue, tx).await?;
        let actual = self.read_files_locked(queue).await?;
        if !exact_files(&actual, keymap, smart) {
            return Err(SessionError::Config("板载配置读回不一致".into()));
        }
        Ok(())
    }

    async fn restore_three_layer_locked(
        &self,
        queue: &mut Queue,
        recovery: &ThreeLayerRecovery,
    ) -> Result<(), SessionError> {
        if recovery.session_id != self.id || recovery.connection_nonce != self.connection_nonce {
            return Err(SessionError::Config(
                "原独占 USB 会话已失效，拒绝恢复".into(),
            ));
        }
        let smart = smart_bytes(&recovery.original);
        let original = &recovery.original.keymap.bytes;
        require_budget(
            queue,
            &[original],
            1 + read_cost(&recovery.candidate, smart) + mutation_cost(original, smart),
        )?;
        self.three_layer_position(queue).await?;
        let current = self.read_files_locked(queue).await?;
        if !exact_files(&current, &recovery.candidate, smart) {
            return Err(SessionError::Config(
                "现有文件与候选不完全相同，拒绝覆盖".into(),
            ));
        }
        if let Some(snapshot) = self.snapshot.lock().unwrap().as_mut() {
            snapshot.stale = true;
        }
        self.stage_three_layer_keymap(queue, original, smart).await
    }

    /// Run only from an explicit apply action. Old mappings are never edited or deleted.
    pub async fn provision_three_layer_profile(
        &self,
        backup_parent: &Path,
    ) -> Result<ThreeLayerBoard, SessionError> {
        Ok(self
            .provision_three_layer_profile_with_recovery(backup_parent)
            .await?
            .board)
    }

    /// A successful write carries an exact-byte recovery for this USB lease only.
    pub async fn provision_three_layer_profile_with_recovery(
        &self,
        backup_parent: &Path,
    ) -> Result<ThreeLayerProvisionReceipt, SessionError> {
        self.guarded(async {
            let mut queue = self.queue.lock().await;
            let before = self.three_layer_position(&mut queue).await?;
            let original = self.read_files_locked(&mut queue).await?;
            let smart = smart_bytes(&original);
            let compiled = compile_three_layer_profile(&original.keymap.bytes, smart)
                .map_err(SessionError::Config)?;
            if self.three_layer_position(&mut queue).await? != before {
                return Err(SessionError::Config(
                    "准备期间设备切换了 Profile 或层位".into(),
                ));
            }
            let (profile_index, candidate) = match compiled {
                ThreeLayerProvision::AlreadySelected { profile_index } => {
                    if before.profile_index == profile_index
                        && (1..=3).contains(&before.layer_index)
                    {
                        return Ok(ThreeLayerProvisionReceipt {
                            board: before,
                            recovery: None,
                        });
                    }
                    return Err(SessionError::Config(
                        "配置已写入，但固件未确认选中三层 Profile".into(),
                    ));
                }
                ThreeLayerProvision::Write {
                    profile_index,
                    keymap,
                } => (profile_index, keymap),
            };
            const POSITION_ATTEMPTS: usize = 5;
            require_budget(
                &queue,
                &[&candidate, &original.keymap.bytes],
                mutation_cost(&candidate, smart)
                    + POSITION_ATTEMPTS
                    + 1
                    + read_cost(&candidate, smart)
                    + mutation_cost(&original.keymap.bytes, smart),
            )?;
            let backup = back_up_three_layer_write(
                backup_parent,
                before,
                profile_index,
                &original.keymap.bytes,
                &candidate,
                smart,
            )
            .map_err(SessionError::Config)?;
            if let Some(snapshot) = self.snapshot.lock().unwrap().as_mut() {
                snapshot.stale = true;
            }
            if let Err(error) = self
                .stage_three_layer_keymap(&mut queue, &candidate, smart)
                .await
            {
                return Err(SessionError::Config(format!(
                    "{error}；写入未核验，未尝试再次写入；原板载配置副本：{}",
                    backup.display(),
                )));
            }
            let recovery = ThreeLayerRecovery {
                session_id: self.id,
                connection_nonce: self.connection_nonce.clone(),
                original,
                candidate,
                backup,
            };
            let board = ThreeLayerBoard {
                profile_index,
                layer_index: 1,
            };
            let mut detail = "板载文件已核验，但固件未确认第 1 层".to_owned();
            for attempt in 0..POSITION_ATTEMPTS {
                if attempt > 0 {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                match self.three_layer_position(&mut queue).await {
                    Ok(position) if position == board => {
                        return Ok(ThreeLayerProvisionReceipt {
                            board,
                            recovery: Some(recovery),
                        })
                    }
                    Ok(_) => {}
                    Err(error) => {
                        detail = format!("{detail}（{error}）");
                        break;
                    }
                }
            }
            let restore = self.restore_three_layer_locked(&mut queue, &recovery).await;
            let outcome = match restore {
                Ok(()) => "已恢复原板载配置并核对两份文件".to_owned(),
                Err(error) => format!("恢复未核验：{error}"),
            };
            Err(SessionError::Config(format!(
                "{detail}；{outcome}；原板载配置副本：{}",
                recovery.backup.display(),
            )))
        })
        .await
    }

    /// Restore only a candidate written by this process on the still-exclusive USB lease.
    /// Return false if the original apply was already selected and did not change bytes.
    pub async fn restore_three_layer_profile(
        &self,
        receipt: &ThreeLayerProvisionReceipt,
    ) -> Result<bool, SessionError> {
        let Some(recovery) = &receipt.recovery else {
            return Ok(false);
        };
        self.guarded(async {
            let mut queue = self.queue.lock().await;
            self.restore_three_layer_locked(&mut queue, recovery)
                .await
                .map_err(|error| {
                    SessionError::Config(format!(
                        "{error}；原板载配置副本：{}",
                        recovery.backup.display(),
                    ))
                })?;
            Ok(true)
        })
        .await
    }
}

pub struct PreparedSixLayerWrite {
    original: DeviceFiles,
    before: ThreeLayerBoard,
    pub profile_index: u32,
    pub candidate: Vec<u8>,
    pub backup: PathBuf,
}

#[derive(Clone, Debug)]
pub struct SixLayerWriteReceipt {
    pub profile_index: u32,
    pub layer_index: u32,
    pub backup: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SixLayerSnapshotCheck {
    Matched,
    FilesDiverged,
    StatusNotReady,
}

#[derive(Debug)]
pub struct SixLayerWriteFailure {
    pub detail: String,
    /// True only after reading exact original files, or before any mutation.
    pub original_verified: bool,
    pub backup: Option<PathBuf>,
}

impl DeviceSession {
    pub async fn prepare_six_layer_write(
        &self,
        backup_parent: &Path,
    ) -> Result<PreparedSixLayerWrite, SessionError> {
        self.guarded(async {
            let mut queue = self.queue.lock().await;
            let before = self.three_layer_position(&mut queue).await?;
            let original = self.read_files_locked(&mut queue).await?;
            let compiled = mi_model::apply::compile_six_layer_profile(
                &original.keymap.bytes,
                smart_bytes(&original),
            )
            .map_err(SessionError::Config)?;
            if self.three_layer_position(&mut queue).await? != before {
                return Err(SessionError::Config("准备期间设备切换了方案或层".into()));
            }
            let root = backup_parent.join("six-layer-recovery");
            std::fs::create_dir_all(&root).map_err(|e| SessionError::Config(e.to_string()))?;
            let backup = back_up_three_layer_write(
                &root,
                before,
                compiled.profile_index,
                &original.keymap.bytes,
                &compiled.keymap,
                smart_bytes(&original),
            )
            .map_err(SessionError::Config)?;
            Ok(PreparedSixLayerWrite {
                original,
                before,
                profile_index: compiled.profile_index,
                candidate: compiled.keymap,
                backup,
            })
        })
        .await
    }

    pub async fn apply_six_layer_write(
        &self,
        prepared: PreparedSixLayerWrite,
    ) -> Result<SixLayerWriteReceipt, SixLayerWriteFailure> {
        let backup = Some(prepared.backup.clone());
        let mut sent = false;
        let result = self
            .guarded(async {
                let mut queue = self.queue.lock().await;
                let smart = smart_bytes(&prepared.original);
                let current = self.read_files_locked(&mut queue).await?;
                if !exact_files(&current, &prepared.original.keymap.bytes, smart)
                    || self.three_layer_position(&mut queue).await? != prepared.before
                {
                    return Err(SessionError::Config("设备在准备后发生变化，未写入".into()));
                }
                require_budget(
                    &queue,
                    &[&prepared.candidate, &prepared.original.keymap.bytes],
                    mutation_cost(&prepared.candidate, smart)
                        + 6
                        + 2 * read_cost(&prepared.candidate, smart)
                        + mutation_cost(&prepared.original.keymap.bytes, smart),
                )?;
                let path = prepared.backup.clone();
                if prepared.candidate != prepared.original.keymap.bytes.as_ref() {
                    sent = true;
                    self.stage_three_layer_keymap(&mut queue, &prepared.candidate, smart)
                        .await?;
                }
                for attempt in 0..5 {
                    if attempt > 0 {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                    let position = self.three_layer_position(&mut queue).await?;
                    if position.profile_index == prepared.profile_index
                        && (1..=6).contains(&position.layer_index)
                    {
                        return Ok(SixLayerWriteReceipt {
                            profile_index: position.profile_index,
                            layer_index: position.layer_index,
                            backup: path,
                        });
                    }
                }
                Err(SessionError::Config(
                    "六层文件已读回，但活动方案与层未确认".into(),
                ))
            })
            .await;
        match result {
            Ok(receipt) => Ok(receipt),
            Err(error) => {
                let original_verified = if !sent {
                    true
                } else {
                    self.guarded(async {
                        let mut queue = self.queue.lock().await;
                        let smart = smart_bytes(&prepared.original);
                        let current = self.read_files_locked(&mut queue).await?;
                        if exact_files(&current, &prepared.original.keymap.bytes, smart) {
                            return Ok(());
                        }
                        if !exact_files(&current, &prepared.candidate, smart) {
                            return Err(SessionError::Config(
                                "当前文件既非原件也非候选，停止自动恢复".into(),
                            ));
                        }
                        self.stage_three_layer_keymap(
                            &mut queue,
                            &prepared.original.keymap.bytes,
                            smart,
                        )
                        .await
                    })
                    .await
                    .is_ok()
                };
                Err(SixLayerWriteFailure {
                    detail: error.to_string(),
                    original_verified,
                    backup,
                })
            }
        }
    }

    pub async fn verify_six_layer_files(
        &self,
        candidate: &[u8],
        smart: Option<&[u8]>,
        profile_index: u32,
    ) -> Result<u32, SessionError> {
        self.guarded(async {
            let mut queue = self.queue.lock().await;
            let current = self.read_files_locked(&mut queue).await?;
            if !exact_files(&current, candidate, smart) {
                return Err(SessionError::Config("六层读回与已应用快照不符".into()));
            }
            mi_model::apply::verify_active_six_layer_profile(
                &current.keymap.bytes,
                smart_bytes(&current),
                profile_index,
            )
            .map_err(SessionError::Config)?;
            let response = self
                .exchange(&mut queue, Method::DeviceStatus, Value::Null)
                .await?;
            let result = identity_result(&response)?;
            let layer = result["layer_index"]
                .as_u64()
                .filter(|n| (1..=6).contains(n))
                .ok_or_else(|| SessionError::Config("未确认六层层位".into()))?;
            if !result["version"]
                .as_str()
                .is_some_and(mi_model::is_supported_native_firmware)
                || result["profile_index"] != profile_index
            {
                return Err(SessionError::Config("六层固件或方案身份不符".into()));
            }
            Ok(layer as u32)
        })
        .await
    }

    pub async fn matches_six_layer_snapshot(
        &self,
        expected_keymap: &[u8],
        expected_smart: Option<&[u8]>,
        profile_index: u32,
    ) -> Result<SixLayerSnapshotCheck, SessionError> {
        self.guarded(async {
            let mut queue = self.queue.lock().await;
            let current = self.read_files_locked(&mut queue).await?;
            let response = self
                .exchange(&mut queue, Method::DeviceStatus, Value::Null)
                .await?;
            let result = identity_result(&response)?;
            if !exact_files(&current, expected_keymap, expected_smart) {
                return Ok(SixLayerSnapshotCheck::FilesDiverged);
            }
            let layer_ok = result["layer_index"]
                .as_u64()
                .is_some_and(|layer| (1..=6).contains(&layer));
            if !result["version"]
                .as_str()
                .is_some_and(mi_model::is_supported_native_firmware)
                || result["profile_index"] != profile_index
                || !layer_ok
            {
                return Ok(SixLayerSnapshotCheck::StatusNotReady);
            }
            Ok(SixLayerSnapshotCheck::Matched)
        })
        .await
    }

    pub async fn restore_six_layer_backup(&self, backup: &Path) -> Result<(), SessionError> {
        let original = std::fs::read(backup.join("keymap.json"))
            .map_err(|e| SessionError::Config(e.to_string()))?;
        let candidate = std::fs::read(backup.join("planned-keymap.json"))
            .map_err(|e| SessionError::Config(e.to_string()))?;
        let previous_candidate = match std::fs::read(backup.join("previous-planned-keymap.json")) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(SessionError::Config(e.to_string())),
        };
        let pre_write_candidate = match std::fs::read(backup.join("pre-write-keymap.json")) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(SessionError::Config(e.to_string())),
        };
        let smart = match std::fs::read(backup.join("smart_actions.json")) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(SessionError::Config(e.to_string())),
        };
        mi_model::apply::recognize_six_layer_recovery_candidate(&candidate, smart.as_deref())
            .map_err(SessionError::Config)?
            .ok_or_else(|| SessionError::Config("恢复副本不是六层方案".into()))?;
        if let Some(prev_cand) = previous_candidate.as_deref() {
            mi_model::apply::recognize_six_layer_recovery_candidate(prev_cand, smart.as_deref())
                .map_err(SessionError::Config)?
                .ok_or_else(|| SessionError::Config("旧版恢复副本不是六层方案".into()))?;
        }
        if let Some(pre_write) = pre_write_candidate
            .as_deref()
            .filter(|pw| *pw != original.as_slice())
        {
            mi_model::apply::recognize_six_layer_recovery_candidate(pre_write, smart.as_deref())
                .map_err(SessionError::Config)?
                .ok_or_else(|| SessionError::Config("写前恢复副本不是六层方案".into()))?;
        }
        self.guarded(async {
            let mut queue = self.queue.lock().await;
            self.three_layer_position(&mut queue).await?;
            let current = self.read_files_locked(&mut queue).await?;
            if exact_files(&current, &original, smart.as_deref()) {
                return Ok(());
            }
            let matches_known_candidate = exact_files(&current, &candidate, smart.as_deref())
                || previous_candidate
                    .as_deref()
                    .is_some_and(|prev| exact_files(&current, prev, smart.as_deref()))
                || pre_write_candidate
                    .as_deref()
                    .is_some_and(|pw| exact_files(&current, pw, smart.as_deref()));
            if !matches_known_candidate {
                return Err(SessionError::Config(
                    "设备与本次候选不符，拒绝覆盖恢复".into(),
                ));
            }
            self.stage_three_layer_keymap(&mut queue, &original, smart.as_deref())
                .await
        })
        .await
    }

    pub async fn restore_three_layer_backup(&self, backup: &Path) -> Result<(), SessionError> {
        let original = std::fs::read(backup.join("keymap.json"))
            .map_err(|e| SessionError::Config(e.to_string()))?;
        let candidate = std::fs::read(backup.join("planned-keymap.json"))
            .map_err(|e| SessionError::Config(e.to_string()))?;
        let smart = match std::fs::read(backup.join("smart_actions.json")) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(SessionError::Config(e.to_string())),
        };
        let parsed = mi_model::parse_device_files(&candidate, smart.as_deref())
            .map_err(|e| SessionError::Config(e.to_string()))?;
        recognize_three_layer_profile(&parsed)
            .map_err(SessionError::Config)?
            .ok_or_else(|| SessionError::Config("恢复副本不是三层方案".into()))?;
        self.guarded(async {
            let mut queue = self.queue.lock().await;
            self.three_layer_position(&mut queue).await?;
            let current = self.read_files_locked(&mut queue).await?;
            if exact_files(&current, &original, smart.as_deref()) {
                return Ok(());
            }
            if !exact_files(&current, &candidate, smart.as_deref()) {
                return Err(SessionError::Config(
                    "设备与本次三层候选不符，拒绝覆盖恢复".into(),
                ));
            }
            self.stage_three_layer_keymap(&mut queue, &original, smart.as_deref())
                .await
        })
        .await
    }
}
