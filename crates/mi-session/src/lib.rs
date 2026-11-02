//! R-D07/R-C01/R-N01: one serialized queue per connection, cancellation and atomic snapshots.
mod input;
pub use input::*;
use mi_model::{DeviceConfig, Source};
use mi_proto::{
    Framer, HostNotification, Incoming, JsonCodec, Method, ProtoError, RpcCodec, RpcRequest,
};
use mi_transport::{HidLink, TransportError};
use serde_json::Value;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::sync::{broadcast, oneshot, Mutex as AsyncMutex, Notify};
static SESSION_ID: AtomicU64 = AtomicU64::new(1);
#[derive(Debug, Clone, thiserror::Error)]
pub enum SessionError {
    #[error(transparent)]
    Protocol(#[from] ProtoError),
    #[error(transparent)]
    Transport(#[from] TransportError),
    #[error("设备已断开；会话已失效")]
    Disconnected,
    #[error("请求超时（本地策略，设备默认值待确认）")]
    Timeout,
    #[error("配置读取失败：{0}")]
    Config(String),
}
// TODO(UNKNOWN-02): explicit local timeout policy, no retries; firmware timeout defaults unverified.
#[derive(Clone)]
pub struct SessionConfig {
    pub timeout: Duration,
    pub interval: Duration,
}
impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(10),
            interval: Duration::from_millis(50),
        }
    }
}
mod files;
pub use files::*;
// TODO(UNKNOWN-04): current-firmware configuration semantics still require device acceptance.
pub trait DeviceSchema: Send + Sync {
    fn identity_request(&self) -> Result<(Method, Value), ProtoError>;
    fn list_request(&self) -> Result<(Method, Value), ProtoError>;
    fn files(&self, response: &Value) -> Result<Vec<FileMeta>, ProtoError>;
    fn read_request(
        &self,
        file: ConfigFile,
        offset: u64,
        len: u32,
    ) -> Result<(Method, Value), ProtoError>;
    fn chunk(&self, response: &Value) -> Result<ReadChunk, ProtoError>;
    fn parse(&self, keymap: &[u8], smart: Option<&[u8]>) -> Result<DeviceConfig, SessionError>;
}
pub struct UnverifiedSchema;
impl DeviceSchema for UnverifiedSchema {
    fn identity_request(&self) -> Result<(Method, Value), ProtoError> {
        Err(ProtoError::Unverified("UNKNOWN-03 identity schema"))
    }
    fn list_request(&self) -> Result<(Method, Value), ProtoError> {
        Err(ProtoError::Unverified("UNKNOWN-04 fs schema"))
    }
    fn files(&self, _: &Value) -> Result<Vec<FileMeta>, ProtoError> {
        Err(ProtoError::Unverified("UNKNOWN-04 fs schema"))
    }
    fn read_request(&self, _: ConfigFile, _: u64, _: u32) -> Result<(Method, Value), ProtoError> {
        Err(ProtoError::Unverified("UNKNOWN-04 fs schema"))
    }
    fn chunk(&self, _: &Value) -> Result<ReadChunk, ProtoError> {
        Err(ProtoError::Unverified("UNKNOWN-04 fs schema"))
    }
    fn parse(&self, _: &[u8], _: Option<&[u8]>) -> Result<DeviceConfig, SessionError> {
        Err(ProtoError::Unverified("UNKNOWN-06 device config schema").into())
    }
}
#[derive(Debug, Clone)]
pub struct IdentityInfo {
    pub session_id: u64,
    pub version: Option<String>,
    pub profile_index: Option<u32>,
    pub layer_index: Option<u32>,
    pub source_method: String,
    pub raw: Value,
}
fn identity_result(v: &Value) -> Result<&Value, SessionError> {
    if let Some(error) = v.get("error") {
        return Err(
            ProtoError::Invalid(format!("identity RPC error code {}", error["code"])).into(),
        );
    }
    v.get("result")
        .filter(|v| v.is_object())
        .ok_or_else(|| ProtoError::Invalid("identity result must be an object".into()).into())
}
fn firmware(v: &Value) -> Option<String> {
    v.get("version")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
}
struct Pending {
    id: u16,
    method: Method,
    sender: oneshot::Sender<Result<Value, SessionError>>,
}
struct PendingLease {
    id: u16,
    pending: Arc<Mutex<Option<Pending>>>,
}
impl Drop for PendingLease {
    fn drop(&mut self) {
        let mut p = self.pending.lock().unwrap();
        if p.as_ref().is_some_and(|p| p.id == self.id) {
            p.take();
        }
    }
}
struct Queue {
    framer: Arc<Mutex<Box<dyn Framer>>>,
    next_id: u16,
    last: Option<tokio::time::Instant>,
}
#[derive(Clone)]
pub struct Snapshot {
    pub config: DeviceConfig,
    pub stale: bool,
    pub files: DeviceFiles,
}
pub struct DeviceSession {
    pub id: u64,
    link: Arc<dyn HidLink>,
    queue: AsyncMutex<Queue>,
    invalid: Arc<AtomicBool>,
    changed: Arc<Notify>,
    pending: Arc<Mutex<Option<Pending>>>,
    terminal: Arc<Mutex<Option<SessionError>>>,
    reader: Mutex<Option<tokio::task::JoinHandle<()>>>,
    config: SessionConfig,
    read_gate: AsyncMutex<()>,
    firmware: Mutex<Option<String>>,
    schema: Arc<dyn DeviceSchema>,
    snapshot: Arc<Mutex<Option<Snapshot>>>,
    notifications: broadcast::Sender<HostNotification>,
}
impl DeviceSession {
    pub fn new(
        link: Arc<dyn HidLink>,
        framer: Box<dyn Framer>,
        schema: Arc<dyn DeviceSchema>,
        config: SessionConfig,
    ) -> Arc<Self> {
        let (notifications, _) = broadcast::channel(256);
        let framer = Arc::new(Mutex::new(framer));
        let invalid = Arc::new(AtomicBool::new(false));
        let changed = Arc::new(Notify::new());
        let pending = Arc::new(Mutex::new(None::<Pending>));
        let terminal = Arc::new(Mutex::new(None));
        let snapshot = Arc::new(Mutex::new(None::<Snapshot>));
        let reader = {
            let terminal = terminal.clone();
            let (link, framer, invalid, changed, pending, notifications, snapshot) = (
                link.clone(),
                framer.clone(),
                invalid.clone(),
                changed.clone(),
                pending.clone(),
                notifications.clone(),
                snapshot.clone(),
            );
            tokio::spawn(async move {
                let outcome: Result<(), SessionError> = async {
                    loop {
                        if invalid.load(Ordering::SeqCst) {
                            break;
                        }
                        let report = link.read().await?;
                        let lines = framer.lock().unwrap().feed(&report)?;
                        for line in lines {
                            if let Some(id) = mi_proto::response_id(&line)? {
                                if !pending.lock().unwrap().as_ref().is_some_and(|p| p.id == id) {
                                    continue;
                                }
                            }
                            match JsonCodec.decode(&line)? {
                                Incoming::Response(r) => {
                                    let mut held = pending.lock().unwrap();
                                    if held.as_ref().is_some_and(|p| {
                                        p.id == r.id
                                            && r.raw
                                                .get("method")
                                                .or_else(|| r.raw.get("m"))
                                                .and_then(Value::as_str)
                                                .is_none_or(|m| m == p.method.name())
                                    }) {
                                        if let Some(p) = held.take() {
                                            let _ = p.sender.send(Ok(r.raw));
                                        }
                                    }
                                }
                                Incoming::Notification(n) => {
                                    let _ = notifications.send(n.into());
                                }
                            }
                        }
                    }
                    Ok(())
                }
                .await;
                if !invalid.load(Ordering::SeqCst) {
                    *terminal.lock().unwrap() = outcome.as_ref().err().cloned();
                }
                invalid.store(true, Ordering::SeqCst);
                if let Some(p) = pending.lock().unwrap().take() {
                    let _ = p
                        .sender
                        .send(Err(outcome.err().unwrap_or(SessionError::Disconnected)));
                }
                framer.lock().unwrap().clear();
                if let Some(s) = snapshot.lock().unwrap().as_mut() {
                    s.stale = true;
                }
                link.close();
                changed.notify_waiters();
            })
        };
        Arc::new(Self {
            id: SESSION_ID.fetch_add(1, Ordering::SeqCst),
            link,
            queue: AsyncMutex::new(Queue {
                framer,
                next_id: 0,
                last: None,
            }),
            invalid,
            changed,
            pending,
            terminal,
            reader: Mutex::new(Some(reader)),
            config,
            read_gate: AsyncMutex::new(()),
            firmware: Mutex::new(None),
            schema,
            snapshot,
            notifications,
        })
    }
    /// R-D07/R-D13: close wakes read; await receiver termination before releasing the session.
    pub async fn shutdown(&self) {
        self.disconnect();
        let reader = self.reader.lock().unwrap().take();
        if let Some(reader) = reader {
            let _ = reader.await;
        }
    }

    pub fn terminal_failure(&self) -> Option<SessionError> {
        self.terminal.lock().unwrap().clone()
    }
    pub fn backend_name(&self) -> &'static str {
        match self.link.backend() {
            mi_transport::LinkBackend::MacHidapi267Numbered => "hidapi-2.6.7/macOS/numbered-0x06",
            #[cfg(feature = "synthetic-backend")]
            mi_transport::LinkBackend::SyntheticNumbered => "synthetic/numbered-0x06",
            _ => "unverified",
        }
    }
    pub fn notifications(&self) -> broadcast::Receiver<HostNotification> {
        self.notifications.subscribe()
    }
    pub fn snapshot(&self) -> Option<Snapshot> {
        self.snapshot.lock().unwrap().clone()
    }
    pub fn disconnect(&self) {
        self.invalid.store(true, Ordering::SeqCst);
        self.link.close();
        if let Some(s) = self.snapshot.lock().unwrap().as_mut() {
            s.stale = true;
        }
        self.changed.notify_waiters();
        if let Ok(q) = self.queue.try_lock() {
            q.framer.lock().unwrap().clear();
        }
    }
    async fn exchange(
        &self,
        q: &mut Queue,
        method: Method,
        params: Value,
    ) -> Result<Value, SessionError> {
        if self.invalid.load(Ordering::SeqCst) {
            q.framer.lock().unwrap().clear();
            return Err(SessionError::Disconnected);
        }
        // Do not wrap IDs in a connection: a late timed-out response must never satisfy a later request.
        if q.next_id >= 999 {
            let error: SessionError =
                ProtoError::Invalid("request id space exhausted; reconnect".into()).into();
            *self.terminal.lock().unwrap() = Some(error.clone());
            self.shutdown().await;
            return Err(error);
        }
        let id = q.next_id;
        q.next_id += 1;
        let request = RpcRequest::new(method, params, id)?;
        mi_proto::validate_read_only(&request)?;
        let reports = q
            .framer
            .lock()
            .unwrap()
            .encode(&JsonCodec.encode(&request)?)?;
        if let Some(last) = q.last {
            tokio::time::sleep_until(last + self.config.interval).await;
        }
        if self.invalid.load(Ordering::SeqCst) {
            return Err(SessionError::Disconnected);
        }
        let (sender, receiver) = oneshot::channel();
        *self.pending.lock().unwrap() = Some(Pending { id, method, sender });
        let _lease = PendingLease {
            pending: self.pending.clone(),
            id,
        };
        for report in reports {
            self.link.write(&report).await?;
        }
        q.last = Some(tokio::time::Instant::now());
        tokio::time::timeout(self.config.timeout, receiver)
            .await
            .map_err(|_| SessionError::Timeout)?
            .map_err(|_| SessionError::Disconnected)?
    }

    async fn guarded<T>(
        &self,
        f: impl std::future::Future<Output = Result<T, SessionError>>,
    ) -> Result<T, SessionError> {
        let n = self.changed.notified();
        tokio::pin!(n);
        n.as_mut().enable();
        if self.invalid.load(Ordering::SeqCst) {
            return Err(SessionError::Disconnected);
        }
        tokio::select! {biased;_=n=>{self.queue.lock().await.framer.lock().unwrap().clear();Err(self.terminal.lock().unwrap().clone().unwrap_or(SessionError::Disconnected))},r=f=>{if matches!(r,Err(SessionError::Transport(TransportError::Disconnected))){self.disconnect();}r}}
    }
    pub async fn request(&self, method: Method, params: Value) -> Result<Value, SessionError> {
        self.guarded(async {
            let mut q = self.queue.lock().await;
            self.exchange(&mut q, method, params).await
        })
        .await
    }
    /// E-053/R-D09: identity reads never invoke the configuration schema or file service.
    pub async fn read_identity_info(&self) -> Result<IdentityInfo, SessionError> {
        *self.firmware.lock().unwrap() = None;
        self.guarded(async {
            let mut q = self.queue.lock().await;
            let status = self
                .exchange(&mut q, Method::DeviceStatus, Value::Null)
                .await;
            let status_value = status
                .as_ref()
                .ok()
                .and_then(|v| identity_result(v).ok())
                .cloned();
            let mut version = status_value.as_ref().and_then(firmware);
            let mut raw = serde_json::json!({"status":status_value});
            let mut source_method = "device.status";
            if version.is_none() {
                let reply = self
                    .exchange(&mut q, Method::SysVersion, Value::Null)
                    .await?;
                let result = identity_result(&reply)?;
                version = firmware(result);
                if version.is_none() {
                    return Err(
                        ProtoError::Invalid("sys.version has no valid version".into()).into(),
                    );
                }
                raw["version"] = result.clone();
                source_method = "sys.version";
            }
            let index = |key: &str| {
                status_value
                    .as_ref()
                    .and_then(|v| v.get(key))
                    .and_then(Value::as_u64)
                    .and_then(|n| u32::try_from(n).ok())
            };
            *self.firmware.lock().unwrap() = version.clone();
            Ok(IdentityInfo {
                session_id: self.id,
                version,
                profile_index: index("profile_index"),
                layer_index: index("layer_index"),
                source_method: source_method.into(),
                raw,
            })
        })
        .await
    }
    pub async fn read_identity(&self) -> Result<Value, SessionError> {
        let (m, p) = self.schema.identity_request()?;
        self.request(m, p).await
    }
    async fn read_file(
        &self,
        q: &mut Queue,
        meta: &FileMeta,
    ) -> Result<VerifiedFile, SessionError> {
        use sha1::{Digest, Sha1};
        let mut bytes = Vec::with_capacity(meta.size as usize);
        while (bytes.len() as u64) < meta.size {
            let offset = bytes.len() as u64;
            let len = (meta.size - offset).min(3072) as u32;
            let (m, p) = self.schema.read_request(meta.file, offset, len)?;
            let chunk = self.schema.chunk(&self.exchange(q, m, p).await?)?;
            if chunk.total_size != meta.size
                || chunk.data.is_empty()
                || chunk.data.len() > len as usize
                || offset
                    .checked_add(chunk.data.len() as u64)
                    .is_none_or(|end| end > meta.size)
            {
                return Err(SessionError::Config(
                    "分块大小无效或读取期间发生变化".into(),
                ));
            }
            bytes.extend_from_slice(&chunk.data);
        }
        let actual: [u8; 20] = Sha1::digest(&bytes).into();
        if actual != meta.checksum {
            return Err(SessionError::Config("原始文件 SHA-1 校验失败".into()));
        }
        Ok(VerifiedFile {
            meta: meta.clone(),
            bytes: bytes.into(),
        })
    }
    /// Bounded read transaction. A failed/cancelled attempt never publishes a partial snapshot.
    pub async fn read_config_snapshot(&self) -> Result<DeviceConfig, SessionError> {
        let _lease = self
            .read_gate
            .try_lock()
            .map_err(|_| SessionError::Config("配置正在读取".into()))?;
        if let Some(old) = self.snapshot.lock().unwrap().as_mut() {
            old.stale = true;
        }
        let read_firmware = self.firmware.lock().unwrap().clone();
        let supported = read_firmware.as_deref() == Some("0.6.3")
            || (self.backend_name().starts_with("synthetic/")
                && read_firmware.as_deref() == Some("synthetic-0.6.3"));
        if !supported {
            return Err(ProtoError::Unverified(
                "UNKNOWN-04 configuration requires explicit supported identity read",
            )
            .into());
        }
        tokio::time::timeout(
            Duration::from_secs(60),
            self.guarded(async {
                let mut q = self.queue.lock().await;
                let (m, p) = self.schema.list_request()?;
                let first = self.schema.files(&self.exchange(&mut q, m, p).await?)?;
                let key = first
                    .iter()
                    .find(|m| m.file == ConfigFile::Keymap)
                    .ok_or_else(|| SessionError::Config("缺少 keymap.json".into()))?;
                let smart = first.iter().find(|m| m.file == ConfigFile::SmartActions);
                let total = first.iter().try_fold(0u64, |sum, m| {
                    if m.size > 4 * 1024 * 1024 {
                        None
                    } else {
                        sum.checked_add(m.size)
                    }
                });
                if total.is_none_or(|n| n > 8 * 1024 * 1024) {
                    return Err(SessionError::Config("配置超过本地读取预算".into()));
                }
                let smart_actions = match smart {
                    Some(m) => Some(self.read_file(&mut q, m).await?),
                    None => None,
                };
                let keymap = self.read_file(&mut q, key).await?;
                let (m, p) = self.schema.list_request()?;
                let second = self.schema.files(&self.exchange(&mut q, m, p).await?)?;
                if first != second {
                    return Err(SessionError::Config(
                        "前后文件元数据不同；本次快照未更新".into(),
                    ));
                }
                let mut parsed = self.schema.parse(
                    &keymap.bytes,
                    smart_actions.as_ref().map(|f| f.bytes.as_ref()),
                )?;
                parsed.source = if matches!(
                    self.link.backend(),
                    mi_transport::LinkBackend::MacHidapi267Numbered
                ) {
                    Source::DeviceSnapshot
                } else {
                    Source::Imported
                };
                parsed.firmware = read_firmware;
                parsed.device_files = Some(
                    std::iter::once(&keymap)
                        .chain(smart_actions.iter())
                        .map(|f| mi_model::FileVerification {
                            name: f.meta.file.name().into(),
                            size: f.meta.size,
                            sha1: f.meta.checksum_hex(),
                        })
                        .collect(),
                );
                let mut held = self.snapshot.lock().unwrap();
                if self.invalid.load(Ordering::SeqCst) {
                    return Err(SessionError::Disconnected);
                }
                *held = Some(Snapshot {
                    config: parsed.clone(),
                    stale: false,
                    files: DeviceFiles {
                        keymap,
                        smart_actions,
                    },
                });
                Ok(parsed)
            }),
        )
        .await
        .map_err(|_| SessionError::Timeout)?
    }
}
impl Drop for DeviceSession {
    fn drop(&mut self) {
        self.link.close();
    }
}
#[cfg(test)]
mod tests;

#[cfg(test)]
mod file_tests;
