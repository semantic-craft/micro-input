//! R-D07/R-C01/R-N01: one serialized queue per connection, cancellation and atomic snapshots.
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
use tokio::sync::{broadcast, Mutex as AsyncMutex, Notify};
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
            timeout: Duration::from_secs(2),
            interval: Duration::from_millis(50),
        }
    }
}
// TODO(UNKNOWN-04): fs parameter names, chunks, checksums and response schemas must be supplied by evidence.
pub trait DeviceSchema: Send + Sync {
    fn identity_request(&self) -> Result<(Method, Value), ProtoError>;
    fn list_request(&self) -> Result<(Method, Value), ProtoError>;
    fn files(&self, response: &Value) -> Result<Vec<String>, ProtoError>;
    fn read_request(&self, name: &str) -> Result<(Method, Value), ProtoError>;
    fn bytes(&self, response: &Value) -> Result<Vec<u8>, ProtoError>;
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
    fn files(&self, _: &Value) -> Result<Vec<String>, ProtoError> {
        Err(ProtoError::Unverified("UNKNOWN-04 fs schema"))
    }
    fn read_request(&self, _: &str) -> Result<(Method, Value), ProtoError> {
        Err(ProtoError::Unverified("UNKNOWN-04 fs schema"))
    }
    fn bytes(&self, _: &Value) -> Result<Vec<u8>, ProtoError> {
        Err(ProtoError::Unverified("UNKNOWN-04 fs schema"))
    }
    fn parse(&self, _: &[u8], _: Option<&[u8]>) -> Result<DeviceConfig, SessionError> {
        Err(ProtoError::Unverified("UNKNOWN-06 device config schema").into())
    }
}
struct Queue {
    framer: Box<dyn Framer>,
    next_id: u16,
    last: Option<tokio::time::Instant>,
}
#[derive(Clone)]
pub struct Snapshot {
    pub config: DeviceConfig,
    pub stale: bool,
}
pub struct DeviceSession {
    pub id: u64,
    link: Arc<dyn HidLink>,
    queue: AsyncMutex<Queue>,
    invalid: AtomicBool,
    changed: Notify,
    config: SessionConfig,
    schema: Arc<dyn DeviceSchema>,
    snapshot: Mutex<Option<Snapshot>>,
    notifications: broadcast::Sender<HostNotification>,
}
impl DeviceSession {
    pub fn new(
        link: Arc<dyn HidLink>,
        framer: Box<dyn Framer>,
        schema: Arc<dyn DeviceSchema>,
        config: SessionConfig,
    ) -> Arc<Self> {
        let (notifications, _) = broadcast::channel(32);
        Arc::new(Self {
            id: SESSION_ID.fetch_add(1, Ordering::SeqCst),
            link,
            queue: AsyncMutex::new(Queue {
                framer,
                next_id: 0,
                last: None,
            }),
            invalid: AtomicBool::new(false),
            changed: Notify::new(),
            config,
            schema,
            snapshot: Mutex::new(None),
            notifications,
        })
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
        if let Ok(mut q) = self.queue.try_lock() {
            q.framer.clear();
        }
    }
    async fn exchange(
        &self,
        q: &mut Queue,
        method: Method,
        params: Value,
    ) -> Result<Value, SessionError> {
        if self.invalid.load(Ordering::SeqCst) {
            q.framer.clear();
            return Err(SessionError::Disconnected);
        }
        // Do not wrap IDs in a connection: a late timed-out response must never satisfy a later request.
        if q.next_id >= 999 {
            return Err(ProtoError::Invalid("request id space exhausted; reconnect".into()).into());
        }
        let id = q.next_id;
        q.next_id += 1;
        let request = RpcRequest::new(method, params, id)?;
        let reports = q.framer.encode(&JsonCodec.encode(&request)?)?;
        if let Some(last) = q.last {
            tokio::time::sleep_until(last + self.config.interval).await;
        }
        for report in reports {
            self.link.write(&report).await?;
        }
        q.last = Some(tokio::time::Instant::now());
        let receive = async {
            loop {
                let report = self.link.read().await?;
                let mut response = None;
                for line in q.framer.feed(&report)? {
                    match JsonCodec.decode(&line)? {
                        Incoming::Response(r) if r.id == id => response = Some(r.raw),
                        Incoming::Response(_) => {}
                        Incoming::Notification(n) => {
                            let _ = self.notifications.send(n.into());
                        }
                    }
                }
                if let Some(value) = response {
                    return Ok(value);
                }
            }
        };
        tokio::time::timeout(self.config.timeout, receive)
            .await
            .map_err(|_| SessionError::Timeout)?
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
        tokio::select! {biased;_=n=>{self.queue.lock().await.framer.clear();Err(SessionError::Disconnected)},r=f=>{if matches!(r,Err(SessionError::Transport(TransportError::Disconnected))){self.disconnect();}r}}
    }
    pub async fn request(&self, method: Method, params: Value) -> Result<Value, SessionError> {
        self.guarded(async {
            let mut q = self.queue.lock().await;
            self.exchange(&mut q, method, params).await
        })
        .await
    }
    pub async fn read_identity(&self) -> Result<Value, SessionError> {
        let (m, p) = self.schema.identity_request()?;
        self.request(m, p).await
    }
    pub async fn read_config_snapshot(&self) -> Result<DeviceConfig, SessionError> {
        let result = self
            .guarded(async {
                let mut q = self.queue.lock().await;
                let (m, p) = self.schema.list_request()?;
                let files = self.schema.files(&self.exchange(&mut q, m, p).await?)?;
                if !files.iter().any(|n| n == "keymap.json") {
                    return Err(SessionError::Config("缺少 keymap.json".into()));
                }
                let (m, p) = self.schema.read_request("keymap.json")?;
                let keymap = self.schema.bytes(&self.exchange(&mut q, m, p).await?)?;
                let smart = if files.iter().any(|n| n == "smart_actions.json") {
                    let (m, p) = self.schema.read_request("smart_actions.json")?;
                    Some(self.schema.bytes(&self.exchange(&mut q, m, p).await?)?)
                } else {
                    None
                };
                let mut parsed = self.schema.parse(&keymap, smart.as_deref())?;
                parsed.source = Source::DeviceSnapshot;
                let mut snapshot = self.snapshot.lock().unwrap();
                if self.invalid.load(Ordering::SeqCst) {
                    return Err(SessionError::Disconnected);
                }
                *snapshot = Some(Snapshot {
                    config: parsed.clone(),
                    stale: false,
                });
                Ok(parsed)
            })
            .await;
        if result.is_err() {
            if let Some(s) = self.snapshot.lock().unwrap().as_mut() {
                s.stale = true;
            }
        }
        result
    }
}
impl Drop for DeviceSession {
    fn drop(&mut self) {
        self.link.close();
    }
}
#[cfg(test)]
mod tests;
