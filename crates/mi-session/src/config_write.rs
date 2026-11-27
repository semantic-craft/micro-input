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
pub struct ConfigWriter {
    session: Arc<DeviceSession>,
    native_interface: Option<String>,
    consumed: AsyncMutex<BTreeSet<String>>,
    records: Mutex<std::collections::BTreeMap<String, OperationRecord>>,
}
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
                failure = Failure::RequestBudget;
                self.session
                    .wait_for_write_capacity("fs.writebin", WritePriority::Normal)
                    .await?;
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
                self.session.charge_logical_config_write("fs.writebin")?;
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

use mi_model::apply::{back_up_three_layer_write, recognize_three_layer_profile, ThreeLayerBoard};
use std::path::{Path, PathBuf};

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
    pub original_verified: bool,
    pub backup: Option<PathBuf>,
}

impl DeviceSession {
    pub async fn prepare_six_layer_write(
        &self,
        backup_parent: &Path,
    ) -> Result<PreparedSixLayerWrite, SessionError> {
        self.prepare_six_layer_write_with_layer_modes(backup_parent, None)
            .await
    }

    pub async fn prepare_six_layer_write_with_layer_modes(
        &self,
        backup_parent: &Path,
        layer_joystick_modes: Option<[Option<&str>; 6]>,
    ) -> Result<PreparedSixLayerWrite, SessionError> {
        self.guarded(async {
            let mut queue = self.queue.lock().await;
            let before = self.three_layer_position(&mut queue).await?;
            let original = self.read_files_locked(&mut queue).await?;
            let fw = self.observed_firmware_version();
            let compiled = if let Some(layer_modes) = layer_joystick_modes {
                mi_model::apply::compile_six_layer_profile_for_firmware_layers(
                    &original.keymap.bytes,
                    smart_bytes(&original),
                    fw.as_deref(),
                    layer_modes,
                )
            } else {
                let default_joystick_mode = fw
                    .as_deref()
                    .filter(|v| mi_model::supports_v070_mouse_tokens(v))
                    .map(|_| "mouse");
                mi_model::apply::compile_six_layer_profile_for_firmware(
                    &original.keymap.bytes,
                    smart_bytes(&original),
                    fw.as_deref(),
                    default_joystick_mode,
                )
            }
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
                self.wait_for_write_capacity("fs.txbegin", WritePriority::Normal)
                    .await?;
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
                    self.charge_logical_config_write("fs.txbegin")?;
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

    fn record_status_firmware<'a>(&self, response: &'a Value) -> Result<&'a Value, SessionError> {
        let result = identity_result(response)?;
        *self.firmware.lock().unwrap() = firmware(result);
        Ok(result)
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
            let result = self.record_status_firmware(&response)?;
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
            let result = self.record_status_firmware(&response)?;
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
        let verified_backup = mi_model::private_fs::verify_existing_private_dir(backup)
            .map_err(|err| SessionError::Config(format!("六层备份目录不安全或不可用：{err:?}")))?;
        let original =
            read_verified_backup_file(&verified_backup.join("keymap.json"), MAX_BACKUP_FILE_BYTES)?;
        let candidate = read_verified_backup_file(
            &verified_backup.join("planned-keymap.json"),
            MAX_BACKUP_FILE_BYTES,
        )?;
        let previous_candidate = read_verified_optional_backup_file(
            &verified_backup.join("previous-planned-keymap.json"),
            MAX_BACKUP_FILE_BYTES,
        )?;
        let pre_write_candidate = read_verified_optional_backup_file(
            &verified_backup.join("pre-write-keymap.json"),
            MAX_BACKUP_FILE_BYTES,
        )?;
        let smart = read_verified_optional_backup_file(
            &verified_backup.join("smart_actions.json"),
            MAX_BACKUP_FILE_BYTES,
        )?;
        verify_backup_manifest(
            &verified_backup,
            &original,
            &candidate,
            previous_candidate.as_deref(),
            pre_write_candidate.as_deref(),
            smart.as_deref(),
        )?;
        mi_model::parse_device_files(&original, smart.as_deref())
            .map_err(|e| SessionError::Config(format!("原始恢复键位校验失败：{e}")))?;
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
            self.wait_for_write_capacity("fs.txbegin", WritePriority::Normal)
                .await?;
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
            self.charge_logical_config_write("fs.txbegin")?;
            self.stage_three_layer_keymap(&mut queue, &original, smart.as_deref())
                .await
        })
        .await
    }

    pub async fn restore_three_layer_backup(&self, backup: &Path) -> Result<(), SessionError> {
        let verified_backup = mi_model::private_fs::verify_existing_private_dir(backup)
            .map_err(|err| SessionError::Config(format!("三层备份目录不安全或不可用：{err:?}")))?;
        let original =
            read_verified_backup_file(&verified_backup.join("keymap.json"), MAX_BACKUP_FILE_BYTES)?;
        let candidate = read_verified_backup_file(
            &verified_backup.join("planned-keymap.json"),
            MAX_BACKUP_FILE_BYTES,
        )?;
        let smart = read_verified_optional_backup_file(
            &verified_backup.join("smart_actions.json"),
            MAX_BACKUP_FILE_BYTES,
        )?;
        verify_backup_manifest(
            &verified_backup,
            &original,
            &candidate,
            None,
            None,
            smart.as_deref(),
        )?;
        mi_model::parse_device_files(&original, smart.as_deref())
            .map_err(|e| SessionError::Config(format!("原始恢复键位校验失败：{e}")))?;
        let parsed = mi_model::parse_device_files(&candidate, smart.as_deref())
            .map_err(|e| SessionError::Config(e.to_string()))?;
        recognize_three_layer_profile(&parsed)
            .map_err(SessionError::Config)?
            .ok_or_else(|| SessionError::Config("恢复副本不是三层方案".into()))?;
        self.guarded(async {
            self.wait_for_write_capacity("fs.txbegin", WritePriority::Normal)
                .await?;
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
            self.charge_logical_config_write("fs.txbegin")?;
            self.stage_three_layer_keymap(&mut queue, &original, smart.as_deref())
                .await
        })
        .await
    }
}

const MAX_BACKUP_FILE_BYTES: u64 = mi_proto::mutation::MAX_FILE_BYTES as u64;
const MAX_BACKUP_MANIFEST_BYTES: u64 = 64 * 1024;

fn read_verified_backup_file_inner(
    path: &Path,
    max_bytes: u64,
    allow_missing: bool,
) -> Result<Option<Vec<u8>>, SessionError> {
    use std::io::Read;
    let file = if allow_missing {
        match mi_model::private_fs::open_verified_readonly_file_optional(path)
            .map_err(SessionError::Config)?
        {
            Some(file) => file,
            None => return Ok(None),
        }
    } else {
        mi_model::private_fs::open_verified_readonly_file(path).map_err(SessionError::Config)?
    };
    let meta = file
        .metadata()
        .map_err(|e| SessionError::Config(e.to_string()))?;
    if meta.len() > max_bytes {
        return Err(SessionError::Config(format!(
            "备份文件超过大小上限（{} > {} 字节）",
            meta.len(),
            max_bytes
        )));
    }
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| SessionError::Config(e.to_string()))?;
    if bytes.len() as u64 > max_bytes {
        return Err(SessionError::Config(format!(
            "备份文件超过大小上限（>{max_bytes} 字节）"
        )));
    }
    Ok(Some(bytes))
}

fn read_verified_backup_file(path: &Path, max_bytes: u64) -> Result<Vec<u8>, SessionError> {
    read_verified_backup_file_inner(path, max_bytes, false)?
        .ok_or_else(|| SessionError::Config("备份文件不存在".into()))
}

fn read_verified_optional_backup_file(
    path: &Path,
    max_bytes: u64,
) -> Result<Option<Vec<u8>>, SessionError> {
    read_verified_backup_file_inner(path, max_bytes, true)
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

fn verify_backup_manifest(
    backup_dir: &Path,
    original: &[u8],
    candidate: &[u8],
    previous_candidate: Option<&[u8]>,
    pre_write_candidate: Option<&[u8]>,
    smart: Option<&[u8]>,
) -> Result<(), SessionError> {
    let Some(raw_manifest) = read_verified_optional_backup_file(
        &backup_dir.join("manifest.json"),
        MAX_BACKUP_MANIFEST_BYTES,
    )?
    else {
        return Ok(());
    };
    let manifest: Value = serde_json::from_slice(&raw_manifest)
        .map_err(|e| SessionError::Config(format!("备份清单格式无效：{e}")))?;
    let expected_keymap = manifest
        .get("keymap_sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| SessionError::Config("备份清单缺少 keymap_sha256".into()))?;
    if expected_keymap != sha256_hex(original) {
        return Err(SessionError::Config(
            "备份清单 keymap_sha256 校验失败".into(),
        ));
    }
    let expected_planned = manifest
        .get("planned_keymap_sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| SessionError::Config("备份清单缺少 planned_keymap_sha256".into()))?;
    if expected_planned != sha256_hex(candidate) {
        return Err(SessionError::Config(
            "备份清单 planned_keymap_sha256 校验失败".into(),
        ));
    }
    match (smart, manifest.get("smart_actions_sha256")) {
        (Some(smart_bytes), Some(Value::String(expected_smart))) => {
            if expected_smart != &sha256_hex(smart_bytes) {
                return Err(SessionError::Config(
                    "备份清单 smart_actions_sha256 校验失败".into(),
                ));
            }
        }
        (Some(_), _) => {
            return Err(SessionError::Config(
                "备份清单缺少 smart_actions_sha256".into(),
            ));
        }
        (None, Some(Value::String(_))) => {
            return Err(SessionError::Config(
                "备份清单声明了 smart_actions_sha256，但文件缺失".into(),
            ));
        }
        (None, None | Some(Value::Null)) => {}
        (None, Some(_)) => {
            return Err(SessionError::Config(
                "备份清单 smart_actions_sha256 字段格式无效".into(),
            ));
        }
    }
    if let (Some(prev), Some(Value::String(expected_prev))) = (
        previous_candidate,
        manifest.get("previous_planned_keymap_sha256"),
    ) {
        if expected_prev != &sha256_hex(prev) {
            return Err(SessionError::Config(
                "备份清单 previous_planned_keymap_sha256 校验失败".into(),
            ));
        }
    }
    if let (Some(pre_write), Some(Value::String(expected_pre_write))) =
        (pre_write_candidate, manifest.get("pre_write_keymap_sha256"))
    {
        if expected_pre_write != &sha256_hex(pre_write) {
            return Err(SessionError::Config(
                "备份清单 pre_write_keymap_sha256 校验失败".into(),
            ));
        }
    }
    Ok(())
}
