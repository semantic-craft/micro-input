//! High-level offline workflow. Only the explicit synthetic constructor can mutate a simulator.
use super::*;
use mi_model::operation_record::OperationRecord;
use serde_json::{json, Value};
#[cfg(feature = "synthetic-backend")]
use std::collections::BTreeMap;
use std::{collections::BTreeSet, path::PathBuf};
#[derive(Default)]
pub(super) struct Operations {
    paused: bool,
    inspecting: bool,
    revalidate_routes: bool,
    next_lease: u64,
    leases: BTreeSet<u64>,
    drain: Option<(String, u64)>,
    running: Option<String>,
    cancel: Option<tokio::sync::watch::Sender<bool>>,
    last: Value,
    #[cfg(feature = "synthetic-backend")]
    local_save_fault: bool,
    #[cfg(feature = "synthetic-backend")]
    restores: BTreeMap<String, mi_model::apply::PreparedCandidate>,
    #[cfg(feature = "synthetic-backend")]
    writers: BTreeMap<String, Arc<mi_session::config_write::SyntheticWriter>>,
}
struct Inspection<'a>(&'a Mutex<Operations>);
impl Drop for Inspection<'_> {
    fn drop(&mut self) {
        self.0.lock().unwrap().inspecting = false;
    }
}
impl MicroInput {
    fn reserve_inspection(&self) -> Result<Inspection<'_>, BridgeError> {
        let mut operations = self.operations.lock().unwrap();
        if operations.inspecting || operations.running.is_some() || operations.drain.is_some() {
            return Err(failure("已有设备操作"));
        }
        operations.inspecting = true;
        Ok(Inspection(&self.operations))
    }
    #[cfg(feature = "synthetic-backend")]
    async fn refresh_operation_routes(&self, session: &Arc<mi_session::DeviceSession>) -> bool {
        let Ok(info) = session.read_identity_info().await else {
            return false;
        };
        if session.read_config_snapshot().await.is_err() {
            return false;
        }
        let mut state = self.state.lock().unwrap();
        if !state
            .session
            .as_ref()
            .is_some_and(|(_, s)| s.id == session.id)
        {
            return false;
        }
        state.identity = Some(IdentityInfo {
            session_id: info.session_id,
            version: info.version,
            profile_index: info.profile_index,
            layer_index: info.layer_index,
            source_method: info.source_method,
            verification: "identity-query-observed".into(),
            backend: session.backend_name().into(),
            raw_json: info.raw.to_string(),
        });
        true
    }
    pub(super) fn write_capable(&self) -> bool {
        #[cfg(feature = "synthetic-backend")]
        {
            let s = self.state.lock().unwrap();
            s.synthetic_write_enabled
                && s.session
                    .as_ref()
                    .is_some_and(|(_, s)| s.backend_name() == "synthetic/numbered-0x06")
        }
        #[cfg(not(feature = "synthetic-backend"))]
        {
            false
        }
    }
    pub(super) fn operation_allows_binding(&self, binding: &str) -> bool {
        let (paused, required) = {
            let o = self.operations.lock().unwrap();
            (o.paused, o.revalidate_routes)
        };
        if paused {
            return false;
        }
        if !required {
            return true;
        }
        let state = self.state.lock().unwrap();
        let Some((_, session)) = &state.session else {
            return false;
        };
        let Some(snapshot) = session.snapshot().filter(|s| !s.stale) else {
            return false;
        };
        let Some(i) = state
            .identity
            .as_ref()
            .filter(|i| i.session_id == session.id)
        else {
            return false;
        };
        let identity = mi_model::host_router::HostIdentityStatus {
            version: i.version.clone(),
            profile_index: i.profile_index,
            layer_index: i.layer_index,
        };
        mi_model::host_router::verified_bindings(&snapshot.config, &identity).contains(binding)
    }
    fn record_path(&self, id: &str) -> Result<PathBuf, BridgeError> {
        // Paths are Rust-owned and scoped to the currently opened document.
        let paths = self
            .draft
            .lock()
            .unwrap()
            .device_operation_paths()
            .map_err(failure)?;
        paths
            .into_iter()
            .find(|p| OperationRecord::open(p).is_ok_and(|r| r.status().operation_id == id))
            .ok_or_else(|| failure("操作不属于当前文档或材料不完整"))
    }
    fn record_view(record: &OperationRecord) -> Value {
        let mut v = serde_json::to_value(record.status()).unwrap();
        v["phase"] = json!(record.restart_phase());
        v["running"] = json!(false);
        v["can_retry_local"] =
            json!(record.status().device_verified && !record.status().local_saved);
        v
    }
}
#[uniffi::export(async_runtime = "tokio")]
impl MicroInput {
    pub fn device_operation_capabilities(&self) -> String {
        json!({"apply":self.write_capable(),"restore":self.write_capable(),"preview":false,"synthetic_only":true,"reason":"原生配置写入与恢复尚未验收"}).to_string()
    }
    pub fn begin_host_execution(&self) -> Result<u64, BridgeError> {
        let mut o = self.operations.lock().unwrap();
        if o.paused {
            return Err(failure("设备操作期间宿主执行已暂停"));
        }
        o.next_lease = o
            .next_lease
            .checked_add(1)
            .ok_or_else(|| failure("执行代次耗尽"))?;
        let lease = o.next_lease;
        o.leases.insert(lease);
        Ok(lease)
    }
    pub fn finish_host_execution(&self, lease: u64) {
        self.operations.lock().unwrap().leases.remove(&lease);
    }
    pub fn host_executions_pending(&self) -> u64 {
        self.operations.lock().unwrap().leases.len() as u64
    }
    pub fn begin_device_drain(&self) -> Result<String, BridgeError> {
        if !self.write_capable() {
            return Err(failure("原生设备变更能力关闭"));
        }
        let session = self.current()?;
        let mut o = self.operations.lock().unwrap();
        if o.inspecting || o.running.is_some() || o.drain.is_some() {
            return Err(failure("已有设备操作"));
        }
        o.paused = true;
        o.revalidate_routes = true;
        let token = mi_model::document::new_id();
        o.drain = Some((token.clone(), session.id));
        Ok(token)
    }
    pub fn abandon_device_drain(&self, token: String) {
        let mut o = self.operations.lock().unwrap();
        if o.drain.as_ref().is_some_and(|(t, _)| t == &token) {
            o.drain = None;
            o.paused = !o.leases.is_empty();
            o.last = json!({"phase":"NotSent","running":false,"reason":"排空取消或超时；未发送设备变更"});
        }
    }
    pub fn device_operation_status(&self, operation_id: String) -> Result<String, BridgeError> {
        let o = self.operations.lock().unwrap();
        if o.running.as_deref() == Some(&operation_id) {
            return Ok(
                json!({"operation_id":operation_id,"phase":"Running","running":true}).to_string(),
            );
        }
        if o.last["operation_id"] == operation_id {
            return Ok(o.last.to_string());
        }
        drop(o);
        Ok(Self::record_view(
            &OperationRecord::open(self.record_path(&operation_id)?).map_err(failure)?,
        )
        .to_string())
    }
    pub fn cancel_device_operation(&self, operation_id: String) {
        let o = self.operations.lock().unwrap();
        if o.running.as_deref() == Some(&operation_id) {
            if let Some(c) = &o.cancel {
                c.send_replace(true);
            }
        }
    }
    pub async fn device_operations(&self) -> Result<String, BridgeError> {
        if self.operations.lock().unwrap().running.is_some() {
            return Err(failure("设备操作进行中"));
        }
        let paths = self
            .draft
            .lock()
            .unwrap()
            .device_operation_paths()
            .map_err(failure)?;
        let mut rows = vec![];
        for p in paths {
            let record = OperationRecord::open(&p).map_err(failure)?;
            let operations = self.operations.lock().unwrap();
            // A durable status update can fail after successful verification. Keep the
            // in-process repair affordance without treating it as restart evidence.
            if operations.last["operation_id"] == record.status().operation_id
                && operations.last["can_retry_local"] == true
            {
                rows.push(operations.last.clone());
            } else {
                rows.push(Self::record_view(&record));
            }
        }
        Ok(json!(rows).to_string())
    }
    pub async fn retry_local_save(&self, operation_id: String) -> Result<String, BridgeError> {
        if self.operations.lock().unwrap().running.is_some() {
            return Err(failure("操作尚未结束"));
        }
        let _inspection = self.reserve_inspection()?;
        let path = self.record_path(&operation_id)?;
        #[cfg(feature = "synthetic-backend")]
        {
            let writer = self
                .operations
                .lock()
                .unwrap()
                .writers
                .get(&operation_id)
                .cloned();
            if let Some(w) = writer {
                w.retry_local_save(&operation_id).map_err(failure)?;
            } else {
                OperationRecord::open(&path)
                    .map_err(failure)?
                    .retry_local_save()
                    .map_err(failure)?;
            }
        }
        #[cfg(not(feature = "synthetic-backend"))]
        OperationRecord::open(&path)
            .map_err(failure)?
            .retry_local_save()
            .map_err(failure)?;
        let record = OperationRecord::open(path).map_err(failure)?;
        self.operations.lock().unwrap().last = Self::record_view(&record);
        let mut store = self.draft.lock().unwrap();
        store.reload_operation_document(&record).map_err(failure)?;
        document_response(&store, None)
    }
    pub fn discard_device_restore(&self) {
        #[cfg(feature = "synthetic-backend")]
        self.operations.lock().unwrap().restores.clear();
    }
    pub async fn prepare_device_restore(
        &self,
        operation_id: String,
    ) -> Result<String, BridgeError> {
        #[cfg(feature = "synthetic-backend")]
        {
            if !self.write_capable() {
                return Err(failure("原生恢复能力关闭"));
            }
            if self.operations.lock().unwrap().running.is_some() {
                return Err(failure("已有设备操作"));
            }
            let _inspection = self.reserve_inspection()?;
            let record =
                OperationRecord::open(self.record_path(&operation_id)?).map_err(failure)?;
            let writer =
                mi_session::config_write::SyntheticWriter::new(self.current()?).map_err(failure)?;
            let plan = writer.prepare_restore(&record).await.map_err(failure)?;
            let preview = plan.preview().map_err(failure)?;
            let token = plan.operation_id().to_string();
            let mut operations = self.operations.lock().unwrap();
            operations.restores.clear();
            operations.restores.insert(token, plan);
            return serde_json::to_string(&preview).map_err(failure);
        }
        #[cfg(not(feature = "synthetic-backend"))]
        {
            let _ = operation_id;
            Err(failure("原生恢复能力关闭"))
        }
    }
    pub async fn check_device_operation(
        &self,
        operation_id: String,
    ) -> Result<String, BridgeError> {
        #[cfg(feature = "synthetic-backend")]
        {
            if !self.write_capable() {
                return Err(failure("当前仅支持合成设备核对"));
            }
            if self.operations.lock().unwrap().running.is_some() {
                return Err(failure("已有设备操作"));
            }
            let _inspection = self.reserve_inspection()?;
            let mut record =
                OperationRecord::open(self.record_path(&operation_id)?).map_err(failure)?;
            let session = self.current()?;
            let writer =
                mi_session::config_write::SyntheticWriter::new(session.clone()).map_err(failure)?;
            writer
                .confirm_readback(&mut record)
                .await
                .map_err(failure)?;
            let ready = self.refresh_operation_routes(&session).await;
            let value = Self::record_view(&record);
            self.draft
                .lock()
                .unwrap()
                .reload_operation_document(&record)
                .map_err(failure)?;
            let mut operations = self.operations.lock().unwrap();
            operations.paused = !ready || !self.current().is_ok_and(|s| s.id == session.id);
            operations.last = value.clone();
            return Ok(value.to_string());
        }
        #[cfg(not(feature = "synthetic-backend"))]
        {
            let _ = operation_id;
            Err(failure("原生核对入口尚未接入"))
        }
    }
    pub async fn start_device_apply(
        self: Arc<Self>,
        token: String,
        drain_token: String,
        restore: bool,
    ) -> Result<String, BridgeError> {
        #[cfg(not(feature = "synthetic-backend"))]
        {
            let _ = (token, drain_token, restore);
            Err(failure("原生设备变更能力关闭"))
        }
        #[cfg(feature = "synthetic-backend")]
        {
            if !self.write_capable() {
                return Err(failure("原生设备变更能力关闭"));
            }
            let session = self.current()?;
            let mut o = self.operations.lock().unwrap();
            if o.inspecting
                || o.running.is_some()
                || !o.leases.is_empty()
                || o.drain.as_ref() != Some(&(drain_token, session.id))
            {
                return Err(failure("宿主动作尚未排空或操作代次失效"));
            }
            let plan = if restore {
                o.restores
                    .remove(&token)
                    .ok_or_else(|| failure("恢复计划不存在或已消费"))?
            } else {
                let state = self.state.lock().unwrap();
                let (_, baseline) = Self::comparison_snapshot(&state)?;
                self.draft
                    .lock()
                    .unwrap()
                    .take_prepared_candidate(&token, &baseline)
                    .map_err(failure)?
            };
            if plan.session_id() != session.id {
                return Err(failure("计划会话已失效"));
            }
            let id = plan.operation_id().to_owned();
            let path = plan.record_path();
            let writer = Arc::new(
                mi_session::config_write::SyntheticWriter::new(session.clone()).map_err(failure)?,
            );
            let (cancel, mut cancelled) = tokio::sync::watch::channel(false);
            o.cancel = Some(cancel);
            o.running = Some(id.clone());
            o.drain = None;
            o.writers.insert(id.clone(), writer.clone());
            let local_save_fault = o.local_save_fault;
            drop(o);
            let core = self.clone();
            let opid = id.clone();
            tokio::spawn(async move {
                let mut injected_lock = None;
                let outcome = tokio::select! {biased; _=cancelled.changed()=>None, r=tokio::time::timeout(std::time::Duration::from_secs(120),writer.apply_observed(&plan, |at, path| {
                    if local_save_fault && at == mi_session::config_write::Checkpoint::BeforeLocalSave {
                        if let Ok(document) = OperationRecord::open(path).and_then(|r| r.document_path()) {
                            let lock = document.with_extension("lock");
                            // Fault seam is limited to the dedicated simulator's own document.
                            if std::fs::OpenOptions::new().write(true).create_new(true).open(&lock).is_ok() {
                                injected_lock = Some(lock);
                            }
                        }
                    }
                }))=>r.ok()};
                if let Some(lock) = injected_lock {
                    let _ = std::fs::remove_file(lock);
                }
                let mut value=OperationRecord::open(&path).map(|r|Self::record_view(&r)).unwrap_or(json!({"operation_id":opid,"phase":"NotSent","running":false,"can_retry_local":false}));
                if outcome == Some(mi_session::config_write::Outcome::VerifiedLocalSaveFailed) {
                    value["phase"] = json!("VerifiedLocalSaveFailed");
                    value["can_retry_local"] = json!(true);
                }
                let verified = outcome == Some(mi_session::config_write::Outcome::Verified);
                let same_session = core.current().is_ok_and(|s| s.id == session.id);
                let mut routes_ready = false;
                if same_session
                    && (verified || outcome == Some(mi_session::config_write::Outcome::NotSent))
                {
                    routes_ready = core.refresh_operation_routes(&session).await;
                }
                if let Ok(record) = OperationRecord::open(&path) {
                    let _ = core
                        .draft
                        .lock()
                        .unwrap()
                        .reload_operation_document(&record);
                }
                let mut o = core.operations.lock().unwrap();
                if o.running.as_deref() == Some(&opid) {
                    if outcome != Some(mi_session::config_write::Outcome::VerifiedLocalSaveFailed) {
                        o.writers.remove(&opid);
                    }
                    o.running = None;
                    o.cancel = None;
                    o.last = value;
                    o.paused = !routes_ready || !core.current().is_ok_and(|s| s.id == session.id);
                }
            });
            Ok(id)
        }
    }
}
#[cfg(feature = "synthetic-backend")]
#[uniffi::export]
impl MicroInput {
    pub fn set_synthetic_local_save_failure(&self, enabled: bool) -> Result<(), BridgeError> {
        if !self.write_capable() {
            return Err(failure("仅合成写入器支持故障注入"));
        }
        self.operations.lock().unwrap().local_save_fault = enabled;
        Ok(())
    }
    #[uniffi::constructor]
    pub fn synthetic_config_writer() -> Arc<Self> {
        let transport = mi_transport::SyntheticDiscovery::default();
        transport.set_synthetic_identity(3).unwrap();
        let mut raw: Value = serde_json::from_slice(include_bytes!(
            "../../mi-model/fixtures/device/synthetic-keymap-lighting.json"
        ))
        .unwrap();
        raw["profiles"][0]["layers"][0]["layout"]["keymap"] = json!([
            ["KV_OAI_AG01", "KV_OAI_AG00"],
            ["KV_OAI_AG02", "KV_OAI_AG03", "KV_OAI_AG04", "KV_OAI_AG05"],
            ["KV_OAI_AG06", "KV_OAI_AG07", "KV_OAI_AG08", "KV_OAI_AG09"],
            ["KV_OAI_AG10", "KV_OAI_AG11", "KV_OAI_AG12"]
        ]);
        transport.set_files(mi_transport::SyntheticFiles::new(
            serde_json::to_vec(&raw).unwrap(),
            None,
        ));
        Arc::new(Self {
            draft: Mutex::new(mi_model::storage::DocumentStore::default()),
            transport: Arc::new(transport),
            state: Mutex::new(ConnectionState {
                synthetic_write_enabled: true,
                ..Default::default()
            }),
            operations: Mutex::new(Operations::default()),
            connect_gate: tokio::sync::Mutex::new(()),
            stopped: tokio::sync::watch::channel(false).0,
        })
    }
}
