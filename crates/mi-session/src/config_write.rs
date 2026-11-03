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
    fn firmware(&self) -> &str {
        if self.native_interface.is_some() {
            "0.6.3"
        } else {
            "synthetic-0.6.3"
        }
    }
    fn baseline(&self, files: &DeviceFiles) -> Value {
        let mut value = synthetic_baseline(self.session.id, files);
        if let Some(interface) = &self.native_interface {
            value["device_identity"] = serde_json::json!({"confidence":"observed-interface","connection_nonce":self.session.connection_nonce,"interface":interface,"session_id":self.session.id,"backend":self.session.backend_name()});
            value["firmware"] = serde_json::json!("0.6.3");
        }
        value
    }
    /// Revalidate the supported backend and independently read complete files.
    pub async fn inspect(&self) -> Result<serde_json::Value, SessionError> {
        let info = self.session.read_identity_info().await?;
        if info.version.as_deref() != Some(self.firmware()) {
            return Err(SessionError::Config("identity unverified".into()));
        }
        self.session
            .guarded(async {
                let mut q = self.session.queue.lock().await;
                let files = self.session.read_files_locked(&mut q).await?;
                let mut base = self.baseline(&files);
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
                if info.version.as_deref() != Some(self.firmware()) {
                    return Err(SessionError::Config("identity unverified".into()));
                }
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
                    .classify(&self.baseline(&current))
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
                        .exchange_keymap(&mut q, plan.candidate(), offset)
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
