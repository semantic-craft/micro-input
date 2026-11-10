use serde::{Deserialize, Serialize};
use std::ffi::CString;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Notify;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeOwnershipRequest {
    pub version: u32,
    pub request_id: Uuid,
    pub client_instance: Uuid,
    pub generation: u64,
    pub desired_owner: String,
    pub micro_drained: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeOwnershipReply {
    pub version: u32,
    pub request_id: Uuid,
    pub client_instance: Uuid,
    pub generation: u64,
    pub native_instance: Uuid,
    pub native_generation: u64,
    pub state: String,
    pub reason: String,
    pub input_drained: bool,
    pub ptt_drained: bool,
    pub lighting_drained: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedNativeOwnershipFence {
    pub client_instance: Uuid,
    pub generation: u64,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum OwnershipTransportError {
    #[error("native ownership socket unavailable")]
    Unavailable,
    #[error("native ownership peer failed security check")]
    InvalidPeer,
    #[error("native ownership reply invalid or exceeded limits")]
    InvalidReply,
}

#[derive(Debug)]
pub struct OwnershipPathLockGuard {
    slot: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
}

impl Drop for OwnershipPathLockGuard {
    fn drop(&mut self) {
        let mut busy = self.slot.0.lock().unwrap_or_else(|e| e.into_inner());
        *busy = false;
        self.slot.1.notify_one();
    }
}

pub struct DeferredOwnershipStream {
    pub stream: UnixStream,
    _guard: OwnershipPathLockGuard,
}

#[derive(Debug, Clone)]
pub struct UnixNativeOwnershipTransport {
    pub path: PathBuf,
    pub request_limit: usize,
    pub response_limit: usize,
    pub timeout: Duration,
}

impl Default for UnixNativeOwnershipTransport {
    fn default() -> Self {
        let base = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            path: base.join("Library/Application Support/MicroInput/cindy-ownership.sock"),
            request_limit: 4096,
            response_limit: 4096,
            timeout: Duration::from_millis(950),
        }
    }
}

impl UnixNativeOwnershipTransport {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            ..Self::default()
        }
    }

    pub fn fence_path(&self) -> PathBuf {
        let mut os = self.path.as_os_str().to_os_string();
        os.push(".fence");
        PathBuf::from(os)
    }

    pub fn socket_exists(&self) -> bool {
        Path::new(&self.path).exists()
    }

    pub fn read_persisted_fence(&self) -> Option<PersistedNativeOwnershipFence> {
        let bytes = self.read_persisted_fence_bytes()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn read_persisted_fence_raw_client_instance(&self) -> Option<String> {
        let bytes = self.read_persisted_fence_bytes()?;
        let val: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
        val.get("clientInstance")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    }

    fn read_persisted_fence_bytes(&self) -> Option<Vec<u8>> {
        let fence_path = self.fence_path();
        let c_path = CString::new(fence_path.as_os_str().as_bytes()).ok()?;
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::lstat(c_path.as_ptr(), &mut st) };
        if rc != 0 {
            return None;
        }
        let uid = unsafe { libc::getuid() };
        if st.st_uid != uid
            || (st.st_mode & libc::S_IFMT) != libc::S_IFREG
            || (st.st_mode & 0o077) != 0
            || st.st_size < 0
            || st.st_size > 4096
        {
            return None;
        }
        std::fs::read(&fence_path).ok()
    }

    pub fn write_persisted_fence(
        &self,
        fence: &PersistedNativeOwnershipFence,
    ) -> std::io::Result<()> {
        let fence_path = self.fence_path();
        if let Some(parent) = fence_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec(fence).map_err(std::io::Error::other)?;
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(&fence_path)?;
        file.write_all(&bytes)?;
        file.sync_all()
    }

    pub fn lock_path(&self) -> OwnershipPathLockGuard {
        static PATH_LOCKS: std::sync::OnceLock<
            std::sync::Mutex<
                std::collections::BTreeMap<
                    PathBuf,
                    Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
                >,
            >,
        > = std::sync::OnceLock::new();
        let slot = {
            let mut map = PATH_LOCKS
                .get_or_init(|| std::sync::Mutex::new(std::collections::BTreeMap::new()))
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            map.entry(self.path.clone())
                .or_insert_with(|| {
                    Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()))
                })
                .clone()
        };
        let mut busy = slot.0.lock().unwrap_or_else(|e| e.into_inner());
        while *busy {
            busy = slot.1.wait(busy).unwrap_or_else(|e| e.into_inner());
        }
        *busy = true;
        drop(busy);
        OwnershipPathLockGuard { slot }
    }

    pub fn exchange_blocking(
        &self,
        request_bytes: &[u8],
    ) -> Result<Vec<u8>, OwnershipTransportError> {
        let _guard = self.lock_path();
        self.exchange_blocking_locked(request_bytes)
    }

    pub fn exchange_blocking_locked(
        &self,
        request_bytes: &[u8],
    ) -> Result<Vec<u8>, OwnershipTransportError> {
        let c_path = CString::new(self.path.as_os_str().as_bytes())
            .map_err(|_| OwnershipTransportError::InvalidPeer)?;
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::lstat(c_path.as_ptr(), &mut st) };
        let uid = unsafe { libc::getuid() };
        if rc != 0
            || st.st_uid != uid
            || (st.st_mode & libc::S_IFMT) != libc::S_IFSOCK
            || (st.st_mode & 0o077) != 0
        {
            return Err(OwnershipTransportError::InvalidPeer);
        }
        if request_bytes.len() > self.request_limit {
            return Err(OwnershipTransportError::InvalidReply);
        }
        let mut stream =
            UnixStream::connect(&self.path).map_err(|_| OwnershipTransportError::Unavailable)?;
        stream
            .set_read_timeout(Some(self.timeout))
            .map_err(|_| OwnershipTransportError::Unavailable)?;
        stream
            .set_write_timeout(Some(self.timeout))
            .map_err(|_| OwnershipTransportError::Unavailable)?;
        let mut peer_uid: libc::uid_t = 0;
        let mut peer_gid: libc::gid_t = 0;
        let peer_rc = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut peer_uid, &mut peer_gid) };
        if peer_rc != 0 || peer_uid != uid {
            return Err(OwnershipTransportError::InvalidPeer);
        }
        let mut frame = Vec::with_capacity(request_bytes.len() + 1);
        frame.extend_from_slice(request_bytes);
        frame.push(b'\n');
        stream
            .write_all(&frame)
            .map_err(|_| OwnershipTransportError::Unavailable)?;
        let mut response = Vec::new();
        let mut buf = [0u8; 1024];
        while response.len() <= self.response_limit {
            let n = stream
                .read(&mut buf)
                .map_err(|_| OwnershipTransportError::Unavailable)?;
            if n == 0 {
                return Err(OwnershipTransportError::Unavailable);
            }
            response.extend_from_slice(&buf[..n]);
            if let Some(nl) = response.iter().position(|&b| b == b'\n') {
                if nl > self.response_limit || response.len() != nl + 1 {
                    return Err(OwnershipTransportError::InvalidReply);
                }
                response.truncate(nl);
                return Ok(response);
            }
        }
        Err(OwnershipTransportError::InvalidReply)
    }

    pub fn serialize_request(
        &self,
        request: &NativeOwnershipRequest,
    ) -> Result<Vec<u8>, OwnershipTransportError> {
        let mut val =
            serde_json::to_value(request).map_err(|_| OwnershipTransportError::InvalidReply)?;
        if let Some(raw_inst) = self.read_persisted_fence_raw_client_instance() {
            if Uuid::parse_str(&raw_inst).ok() == Some(request.client_instance) {
                if let Some(obj) = val.as_object_mut() {
                    obj.insert(
                        "clientInstance".to_string(),
                        serde_json::Value::String(raw_inst),
                    );
                }
            }
        }
        serde_json::to_vec(&val).map_err(|_| OwnershipTransportError::InvalidReply)
    }

    pub fn exchange(
        &self,
        request: &NativeOwnershipRequest,
    ) -> Result<NativeOwnershipReply, OwnershipTransportError> {
        let _guard = self.lock_path();
        self.exchange_locked(request)
    }

    pub fn exchange_locked(
        &self,
        request: &NativeOwnershipRequest,
    ) -> Result<NativeOwnershipReply, OwnershipTransportError> {
        let bytes = self.serialize_request(request)?;
        let raw = self.exchange_blocking_locked(&bytes)?;
        serde_json::from_slice(&raw).map_err(|_| OwnershipTransportError::InvalidReply)
    }

    pub fn begin_stream_request_locked(
        &self,
        request: &NativeOwnershipRequest,
        fast_timeout: Duration,
        guard: OwnershipPathLockGuard,
    ) -> Result<Result<NativeOwnershipReply, DeferredOwnershipStream>, OwnershipTransportError>
    {
        let request_bytes = self.serialize_request(request)?;
        let c_path = CString::new(self.path.as_os_str().as_bytes())
            .map_err(|_| OwnershipTransportError::InvalidPeer)?;
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::lstat(c_path.as_ptr(), &mut st) };
        let uid = unsafe { libc::getuid() };
        if rc != 0
            || st.st_uid != uid
            || (st.st_mode & libc::S_IFMT) != libc::S_IFSOCK
            || (st.st_mode & 0o077) != 0
        {
            return Err(OwnershipTransportError::InvalidPeer);
        }
        if request_bytes.len() > self.request_limit {
            return Err(OwnershipTransportError::InvalidReply);
        }
        let mut stream =
            UnixStream::connect(&self.path).map_err(|_| OwnershipTransportError::Unavailable)?;
        stream
            .set_read_timeout(Some(fast_timeout))
            .map_err(|_| OwnershipTransportError::Unavailable)?;
        stream
            .set_write_timeout(Some(self.timeout))
            .map_err(|_| OwnershipTransportError::Unavailable)?;
        let mut peer_uid: libc::uid_t = 0;
        let mut peer_gid: libc::gid_t = 0;
        let peer_rc = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut peer_uid, &mut peer_gid) };
        if peer_rc != 0 || peer_uid != uid {
            return Err(OwnershipTransportError::InvalidPeer);
        }
        let mut frame = Vec::with_capacity(request_bytes.len() + 1);
        frame.extend_from_slice(&request_bytes);
        frame.push(b'\n');
        stream
            .write_all(&frame)
            .map_err(|_| OwnershipTransportError::Unavailable)?;
        let mut response = Vec::new();
        let mut buf = [0u8; 1024];
        while response.len() <= self.response_limit {
            match stream.read(&mut buf) {
                Ok(0) => return Err(OwnershipTransportError::Unavailable),
                Ok(n) => {
                    response.extend_from_slice(&buf[..n]);
                    if let Some(nl) = response.iter().position(|&b| b == b'\n') {
                        if nl > self.response_limit || response.len() != nl + 1 {
                            return Err(OwnershipTransportError::InvalidReply);
                        }
                        response.truncate(nl);
                        let reply = serde_json::from_slice(&response)
                            .map_err(|_| OwnershipTransportError::InvalidReply)?;
                        return Ok(Ok(reply));
                    }
                }
                Err(err)
                    if response.is_empty()
                        && matches!(
                            err.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) =>
                {
                    return Ok(Err(DeferredOwnershipStream {
                        stream,
                        _guard: guard,
                    }));
                }
                Err(_) => return Err(OwnershipTransportError::Unavailable),
            }
        }
        Err(OwnershipTransportError::InvalidReply)
    }

    pub fn finish_stream_reply(
        deferred: DeferredOwnershipStream,
        response_limit: usize,
        full_timeout: Duration,
    ) -> Result<NativeOwnershipReply, OwnershipTransportError> {
        let mut stream = deferred.stream;
        stream
            .set_read_timeout(Some(full_timeout))
            .map_err(|_| OwnershipTransportError::Unavailable)?;
        let mut response = Vec::new();
        let mut buf = [0u8; 1024];
        while response.len() <= response_limit {
            let n = stream
                .read(&mut buf)
                .map_err(|_| OwnershipTransportError::Unavailable)?;
            if n == 0 {
                return Err(OwnershipTransportError::Unavailable);
            }
            response.extend_from_slice(&buf[..n]);
            if let Some(nl) = response.iter().position(|&b| b == b'\n') {
                if nl > response_limit || response.len() != nl + 1 {
                    return Err(OwnershipTransportError::InvalidReply);
                }
                response.truncate(nl);
                return serde_json::from_slice(&response)
                    .map_err(|_| OwnershipTransportError::InvalidReply);
            }
        }
        Err(OwnershipTransportError::InvalidReply)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnershipState {
    Unknown(String),
    Native { instance: Uuid, generation: u64 },
    Released { instance: Uuid, generation: u64 },
}

#[derive(Debug, Clone, Default)]
pub struct InterruptibleSleeper {
    notify: Arc<Notify>,
}

impl InterruptibleSleeper {
    pub fn new() -> Self {
        Self {
            notify: Arc::new(Notify::new()),
        }
    }

    pub fn interrupt(&self) {
        self.notify.notify_waiters();
    }

    pub async fn sleep_interruptibly(&self, duration: Duration) -> bool {
        tokio::select! {
            _ = self.notify.notified() => true,
            _ = tokio::time::sleep(duration) => false,
        }
    }
}

pub struct NativeOwnershipController {
    state: OwnershipState,
    generation: u64,
    client_instance: Uuid,
    pending: Option<NativeOwnershipRequest>,
    request_started_at: Option<Instant>,
    expires_at: Option<Instant>,
    native_identity: Option<(Uuid, u64)>,
    last_released_at: Option<Instant>,
    transport: Option<UnixNativeOwnershipTransport>,
    isolated: bool,
    allow_unbridged_desktop: bool,
    bridge_observed: bool,
    desired_owner: String,
    sleeper: InterruptibleSleeper,
    changed_count: usize,
}

impl NativeOwnershipController {
    pub fn new(
        transport: Option<UnixNativeOwnershipTransport>,
        allow_unbridged_desktop: bool,
    ) -> Self {
        let (client_instance, generation) = transport
            .as_ref()
            .and_then(UnixNativeOwnershipTransport::read_persisted_fence)
            .map(|f| (f.client_instance, f.generation.wrapping_add(1)))
            .unwrap_or_else(|| (Uuid::new_v4(), 0));
        Self {
            state: OwnershipState::Unknown("尚未取得 Cindy 原生所有权回执".into()),
            generation,
            client_instance,
            pending: None,
            request_started_at: None,
            expires_at: None,
            native_identity: None,
            last_released_at: None,
            transport,
            isolated: false,
            allow_unbridged_desktop,
            bridge_observed: false,
            desired_owner: "native".into(),
            sleeper: InterruptibleSleeper::new(),
            changed_count: 0,
        }
    }

    pub fn new_isolated() -> Self {
        Self {
            state: OwnershipState::Unknown("尚未取得 Cindy 原生所有权回执".into()),
            generation: 0,
            client_instance: Uuid::new_v4(),
            pending: None,
            request_started_at: None,
            expires_at: None,
            native_identity: None,
            last_released_at: None,
            transport: None,
            isolated: true,
            allow_unbridged_desktop: false,
            bridge_observed: false,
            desired_owner: "native".into(),
            sleeper: InterruptibleSleeper::new(),
            changed_count: 0,
        }
    }

    pub fn state(&self) -> &OwnershipState {
        &self.state
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn client_instance(&self) -> Uuid {
        self.client_instance
    }

    pub fn desired_owner(&self) -> &str {
        &self.desired_owner
    }

    pub fn has_pending_request(&self) -> bool {
        self.pending.is_some()
    }

    pub fn sleeper(&self) -> InterruptibleSleeper {
        self.sleeper.clone()
    }

    pub fn changed_count(&self) -> usize {
        self.changed_count
    }

    pub fn transport(&self) -> Option<&UnixNativeOwnershipTransport> {
        self.transport.as_ref()
    }

    pub fn sync_persisted_fence_if_needed(&mut self) {
        let Some(fence) = self
            .transport
            .as_ref()
            .and_then(UnixNativeOwnershipTransport::read_persisted_fence)
        else {
            return;
        };
        self.client_instance = fence.client_instance;
        if self.generation <= fence.generation {
            self.generation = fence.generation.wrapping_add(1);
        }
    }

    pub fn is_unbridged_desktop_active(&self) -> bool {
        self.allow_unbridged_desktop
            && self
                .transport
                .as_ref()
                .is_some_and(|unix| !Path::new(&unix.path).exists())
    }

    pub fn reconcile_unbridged_transition(&mut self) -> bool {
        self.reconcile_unbridged_transition_at(Instant::now())
    }

    pub fn reconcile_unbridged_transition_at(&mut self, now: Instant) -> bool {
        if self.is_unbridged_desktop_active() {
            if self.bridge_observed || self.native_identity.is_some() {
                self.bridge_observed = false;
                self.native_identity = None;
                self.last_released_at = None;
                self.pending = None;
                self.request_started_at = None;
                self.expires_at = None;
                self.state = OwnershipState::Unknown("Cindy 原生桥已退出，恢复本地宿主控制".into());
                self.changed_count += 1;
                return true;
            }
            return false;
        }
        if self.allow_unbridged_desktop && !self.bridge_observed {
            self.bridge_observed = true;
            self.sync_persisted_fence_if_needed();
            self.invalidate_at("检测到 Cindy 原生所有权桥，等待确认", false, now);
            return true;
        }
        false
    }

    pub fn allows_execution(&self) -> bool {
        self.allows_execution_at(Instant::now())
    }

    pub fn allows_execution_at(&self, now: Instant) -> bool {
        if self.isolated || self.is_unbridged_desktop_active() {
            return true;
        }
        matches!(self.state, OwnershipState::Released { .. })
            && self.expires_at.is_some_and(|exp| now < exp)
    }

    pub fn is_transient_reconfirming(&self) -> bool {
        self.is_transient_reconfirming_at(Instant::now())
    }

    pub fn is_transient_reconfirming_at(&self, now: Instant) -> bool {
        if self.isolated
            || self.is_unbridged_desktop_active()
            || self.desired_owner != "micro-input"
        {
            return false;
        }
        if self.allow_unbridged_desktop
            && self.pending.is_some()
            && self.native_identity.is_some()
            && self
                .request_started_at
                .is_some_and(|t| now.saturating_duration_since(t) < Duration::from_secs(1))
        {
            return true;
        }
        matches!(self.state, OwnershipState::Unknown(_))
            && self
                .last_released_at
                .is_some_and(|t| now.saturating_duration_since(t) < Duration::from_millis(500))
    }

    pub fn summary(&self) -> String {
        self.summary_at(Instant::now())
    }

    pub fn summary_at(&self, now: Instant) -> String {
        if self.isolated {
            return "隔离合成所有权".into();
        }
        if self.is_unbridged_desktop_active() {
            return "本地宿主控制就绪（Cindy 原生桥未启用）".into();
        }
        match &self.state {
            OwnershipState::Unknown(reason) => {
                if self.is_transient_reconfirming_at(now) {
                    "Cindy 已确认释放".into()
                } else {
                    format!("原生所有权未知，按键、听写与灯光暂停 · {reason}")
                }
            }
            OwnershipState::Native { .. } => {
                "Cindy 原生持有，Micro Input 按键、听写与灯光暂停".into()
            }
            OwnershipState::Released { .. } => {
                if self.allows_execution_at(now) {
                    "Cindy 已确认释放".into()
                } else {
                    "原生回执已过期，输入暂停".into()
                }
            }
        }
    }

    pub fn set_desired_owner(&mut self, value: &str) {
        self.set_desired_owner_at(value, Instant::now());
    }

    pub fn set_desired_owner_at(&mut self, value: &str, now: Instant) {
        if value == self.desired_owner {
            return;
        }
        self.desired_owner = value.to_owned();
        self.invalidate_at("配置已切换，等待当前代回执", value == "micro-input", now);
    }

    pub fn invalidate(&mut self, reason: &str, preserve_reconfirmation: bool) {
        self.invalidate_at(reason, preserve_reconfirmation, Instant::now());
    }

    pub fn invalidate_at(&mut self, reason: &str, preserve_reconfirmation: bool, now: Instant) {
        if self.isolated || self.is_unbridged_desktop_active() {
            return;
        }
        if preserve_reconfirmation && self.native_identity.is_some() {
            self.last_released_at = Some(now);
        } else if !preserve_reconfirmation {
            self.last_released_at = None;
        }
        self.sleeper.interrupt();
        self.generation = self.generation.wrapping_add(1);
        self.pending = None;
        self.request_started_at = None;
        self.expires_at = None;
        self.state = OwnershipState::Unknown(reason.to_owned());
        self.changed_count += 1;
    }

    pub fn begin_request(&mut self, micro_drained: bool) -> NativeOwnershipRequest {
        self.begin_request_at(micro_drained, Instant::now())
    }

    pub fn begin_request_at(
        &mut self,
        micro_drained: bool,
        now: Instant,
    ) -> NativeOwnershipRequest {
        let request = NativeOwnershipRequest {
            version: 1,
            request_id: Uuid::new_v4(),
            client_instance: self.client_instance,
            generation: self.generation,
            desired_owner: self.desired_owner.clone(),
            micro_drained,
        };
        self.pending = Some(request.clone());
        self.request_started_at = Some(now);
        request
    }

    pub fn receive(&mut self, reply: NativeOwnershipReply, for_request: &NativeOwnershipRequest) {
        self.receive_at(reply, for_request, Instant::now());
    }

    pub fn receive_at(
        &mut self,
        reply: NativeOwnershipReply,
        for_request: &NativeOwnershipRequest,
        now: Instant,
    ) {
        if self.pending.as_ref() != Some(for_request) || self.generation != for_request.generation {
            return;
        }
        self.pending = None;
        let max_latency = if self.allow_unbridged_desktop {
            Duration::from_secs(2)
        } else {
            Duration::from_secs(1)
        };
        let Some(started) = self.request_started_at else {
            self.invalidate_at("握手身份或代次未确认", false, now);
            return;
        };
        if now.saturating_duration_since(started) >= max_latency
            || reply.version != 1
            || reply.request_id != for_request.request_id
            || reply.client_instance != self.client_instance
            || reply.generation != self.generation
        {
            self.invalidate_at("握手身份或代次未确认", false, now);
            return;
        }
        if let Some((prev_inst, prev_gen)) = self.native_identity {
            if prev_inst == reply.native_instance && reply.native_generation < prev_gen {
                self.invalidate_at("原生旧代回执已拒绝", false, now);
                return;
            }
            if prev_inst != reply.native_instance {
                self.native_identity = Some((reply.native_instance, reply.native_generation));
                self.invalidate_at("Cindy 实例已更换，等待重新确认", false, now);
                return;
            }
        }
        self.native_identity = Some((reply.native_instance, reply.native_generation));
        if reply.state == "unknown"
            && matches!(
                reply.reason.as_str(),
                "stale_generation" | "returned_generation" | "different_client"
            )
        {
            self.sync_persisted_fence_if_needed();
            self.invalidate_at("握手代次已同步，等待重新确认", false, now);
            return;
        }
        let has_fenced_cap = reply
            .capabilities
            .as_ref()
            .is_some_and(|caps| caps.iter().any(|c| c == "fenced-release-v1"));
        let next = match reply.state.as_str() {
            "native" => OwnershipState::Native {
                instance: reply.native_instance,
                generation: reply.native_generation,
            },
            "released" => {
                if for_request.desired_owner != "micro-input"
                    || !has_fenced_cap
                    || !reply.input_drained
                    || !reply.ptt_drained
                    || !reply.lighting_drained
                {
                    self.last_released_at = None;
                    self.invalidate_at("原生释放未完整确认", false, now);
                    return;
                }
                self.last_released_at = Some(now);
                OwnershipState::Released {
                    instance: reply.native_instance,
                    generation: reply.native_generation,
                }
            }
            "unknown"
                if self.allow_unbridged_desktop
                    && for_request.desired_owner == "native"
                    && has_fenced_cap
                    && reply.reason == "native_state_unconfirmed" =>
            {
                self.last_released_at = None;
                OwnershipState::Native {
                    instance: reply.native_instance,
                    generation: reply.native_generation,
                }
            }
            "unknown"
                if self.allow_unbridged_desktop
                    && for_request.desired_owner == "micro-input"
                    && has_fenced_cap
                    && reply.lighting_drained
                    && reply.reason == "native_drain_unconfirmed" =>
            {
                self.last_released_at = Some(now);
                OwnershipState::Released {
                    instance: reply.native_instance,
                    generation: reply.native_generation,
                }
            }
            _ => {
                self.last_released_at = None;
                OwnershipState::Unknown("Cindy 未确认释放".into())
            }
        };
        self.expires_at = Some(if self.allow_unbridged_desktop {
            now + Duration::from_secs(300)
        } else {
            started + Duration::from_secs(1)
        });
        let is_same_instance_renewal = match (&self.state, &next) {
            (
                OwnershipState::Released {
                    instance: prev_inst,
                    ..
                },
                OwnershipState::Released {
                    instance: next_inst,
                    ..
                },
            ) => prev_inst == next_inst,
            _ => false,
        };
        if next != self.state {
            self.state = next;
            if !is_same_instance_renewal {
                self.changed_count += 1;
            }
        }
    }

    pub fn failed(&mut self, request: &NativeOwnershipRequest) {
        self.failed_at(request, Instant::now());
    }

    pub fn failed_at(&mut self, request: &NativeOwnershipRequest, now: Instant) {
        if self.pending.as_ref() != Some(request) || self.generation != request.generation {
            return;
        }
        self.last_released_at = None;
        self.invalidate_at("Cindy 握手失联或不可用", false, now);
    }

    pub fn poll_once(&mut self, micro_drained: bool) {
        self.poll_once_at(micro_drained, Instant::now());
    }

    pub fn poll_fast_or_defer_at(
        &mut self,
        micro_drained: bool,
        now: Instant,
        fast_timeout: Duration,
    ) -> Option<(NativeOwnershipRequest, DeferredOwnershipStream)> {
        if self.isolated {
            return None;
        }
        self.reconcile_unbridged_transition_at(now);
        if self.is_unbridged_desktop_active() {
            return None;
        }
        let transport = self.transport.clone()?;
        for _ in 0..2 {
            let guard = transport.lock_path();
            self.sync_persisted_fence_if_needed();
            let req_now = now;
            let req = self.begin_request_at(micro_drained, req_now);
            match transport.begin_stream_request_locked(&req, fast_timeout, guard) {
                Ok(Ok(reply)) => {
                    let prev_inst = self.native_identity.map(|(id, _)| id);
                    let retry_needed = (reply.state == "unknown"
                        && matches!(
                            reply.reason.as_str(),
                            "stale_generation" | "returned_generation" | "different_client"
                        ))
                        || prev_inst.is_some_and(|id| id != reply.native_instance);
                    self.receive_at(reply, &req, now);
                    if !retry_needed {
                        break;
                    }
                }
                Ok(Err(deferred)) => {
                    return Some((req, deferred));
                }
                Err(_) => {
                    self.failed_at(&req, now);
                    break;
                }
            }
        }
        None
    }

    pub fn poll_once_at(&mut self, micro_drained: bool, now: Instant) {
        if self.isolated {
            return;
        }
        self.reconcile_unbridged_transition_at(now);
        if self.is_unbridged_desktop_active() {
            return;
        }
        let Some(transport) = self.transport.clone() else {
            return;
        };
        for _ in 0..2 {
            let _guard = transport.lock_path();
            self.sync_persisted_fence_if_needed();
            if self.pending.is_some() {
                self.generation = self.generation.wrapping_add(1);
                self.pending = None;
            }
            let req_now = now;
            let req = self.begin_request_at(micro_drained, req_now);
            match transport.exchange_locked(&req) {
                Ok(reply) => {
                    let prev_inst = self.native_identity.map(|(id, _)| id);
                    let retry_needed = (reply.state == "unknown"
                        && matches!(
                            reply.reason.as_str(),
                            "stale_generation" | "returned_generation" | "different_client"
                        ))
                        || prev_inst.is_some_and(|id| id != reply.native_instance);
                    self.receive_at(reply, &req, now);
                    if !retry_needed {
                        break;
                    }
                }
                Err(_) => {
                    self.failed_at(&req, now);
                    break;
                }
            }
        }
    }

    pub fn check_expired_at(&mut self, now: Instant) -> bool {
        if matches!(self.state, OwnershipState::Released { .. })
            && self.expires_at.is_some_and(|exp| now >= exp)
        {
            self.last_released_at = None;
            self.invalidate_at("原生回执已过期", false, now);
            return true;
        }
        false
    }

    pub fn stop(&mut self) {
        self.stop_at(Instant::now());
    }

    pub fn stop_at(&mut self, now: Instant) {
        self.sleeper.interrupt();
        self.invalidate_at("握手已停止", false, now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;

    #[test]
    fn test_restarted_controller_resumes_persisted_fence_identity_and_generation() {
        let dir = tempfile::tempdir().unwrap();
        let sock_path = dir.path().join("cindy-ownership.sock");
        let transport = UnixNativeOwnershipTransport::new(&sock_path);
        let expected_instance = Uuid::new_v4();
        transport
            .write_persisted_fence(&PersistedNativeOwnershipFence {
                client_instance: expected_instance,
                generation: 41,
            })
            .unwrap();

        let controller = NativeOwnershipController::new(Some(transport.clone()), false);
        assert_eq!(controller.client_instance(), expected_instance);
        assert_eq!(controller.generation(), 42);

        std::fs::set_permissions(
            transport.fence_path(),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        let fallback = NativeOwnershipController::new(Some(transport), false);
        assert_ne!(fallback.client_instance(), expected_instance);
        assert_eq!(fallback.generation(), 0);
    }

    #[test]
    fn test_unbridged_desktop_recovers_when_bridge_socket_disappears_and_refences_on_reappear() {
        let dir = tempfile::Builder::new()
            .prefix("mi-")
            .tempdir_in("/tmp")
            .unwrap();
        let sock_path = dir.path().join("cindy-ownership.sock");
        let transport = UnixNativeOwnershipTransport::new(&sock_path);
        let mut controller = NativeOwnershipController::new(Some(transport), true);

        assert!(controller.is_unbridged_desktop_active());
        assert!(controller.allows_execution());

        let listener = UnixListener::bind(&sock_path).unwrap();
        std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o600)).unwrap();

        assert!(!controller.is_unbridged_desktop_active());
        assert!(controller.reconcile_unbridged_transition());
        assert!(!controller.allows_execution());

        controller.set_desired_owner("micro-input");
        let req = controller.begin_request(true);
        let native_id = Uuid::new_v4();
        controller.receive(
            NativeOwnershipReply {
                version: 1,
                request_id: req.request_id,
                client_instance: req.client_instance,
                generation: req.generation,
                native_instance: native_id,
                native_generation: 1,
                state: "native".into(),
                reason: "held".into(),
                input_drained: true,
                ptt_drained: true,
                lighting_drained: true,
                capabilities: Some(vec!["fenced-release-v1".into()]),
            },
            &req,
        );
        assert!(!controller.allows_execution());

        drop(listener);
        std::fs::remove_file(&sock_path).unwrap();

        assert!(controller.is_unbridged_desktop_active());
        assert!(controller.allows_execution());
        assert!(controller.reconcile_unbridged_transition());
        assert!(controller.allows_execution());

        let _listener2 = UnixListener::bind(&sock_path).unwrap();
        std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(!controller.is_unbridged_desktop_active());
        assert!(controller.reconcile_unbridged_transition());
        assert!(!controller.allows_execution());
    }

    #[tokio::test]
    async fn test_interruptible_poll_sleep_wakes_on_invalidate() {
        let mut controller = NativeOwnershipController::new(None, false);
        let sleeper = controller.sleeper();
        let waiter =
            tokio::spawn(async move { sleeper.sleep_interruptibly(Duration::from_secs(5)).await });
        tokio::task::yield_now().await;
        controller.invalidate("切换配置唤醒", false);
        let interrupted = waiter.await.unwrap();
        assert!(interrupted);
    }

    #[test]
    fn test_transient_reconfirmation_window_expires_after_500ms() {
        let mut controller = NativeOwnershipController::new(None, false);
        let t0 = Instant::now();
        controller.set_desired_owner_at("micro-input", t0);
        let req = controller.begin_request_at(true, t0);
        controller.receive_at(
            NativeOwnershipReply {
                version: 1,
                request_id: req.request_id,
                client_instance: req.client_instance,
                generation: req.generation,
                native_instance: Uuid::new_v4(),
                native_generation: 1,
                state: "released".into(),
                reason: "ok".into(),
                input_drained: true,
                ptt_drained: true,
                lighting_drained: true,
                capabilities: Some(vec!["fenced-release-v1".into()]),
            },
            &req,
            t0 + Duration::from_millis(10),
        );
        assert!(controller.allows_execution_at(t0 + Duration::from_millis(20)));

        let t_switch = t0 + Duration::from_millis(50);
        controller.invalidate_at("切换工作台配置", true, t_switch);
        assert!(!controller.allows_execution_at(t_switch));
        assert!(controller.is_transient_reconfirming_at(t_switch + Duration::from_millis(200)));
        assert!(!controller.is_transient_reconfirming_at(t_switch + Duration::from_millis(500)));
    }

    #[test]
    fn test_exchange_preserves_uppercase_persisted_fence_client_instance_for_cindy_strict_equality()
    {
        use std::io::{BufRead, BufReader};

        let dir = tempfile::Builder::new()
            .prefix("mi-")
            .tempdir_in("/tmp")
            .unwrap();
        let sock_path = dir.path().join("cindy-ownership.sock");
        let transport = UnixNativeOwnershipTransport::new(&sock_path);
        let fence_path = transport.fence_path();
        let uppercase_uuid = "00508FEF-D751-4FEB-9F74-7876C422E553";
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(&fence_path)
            .unwrap();
        file.write_all(
            format!(r#"{{"clientInstance":"{uppercase_uuid}","generation":10}}"#).as_bytes(),
        )
        .unwrap();
        file.sync_all().unwrap();

        let listener = UnixListener::bind(&sock_path).unwrap();
        std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o600)).unwrap();

        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let req: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert_eq!(req["clientInstance"].as_str(), Some(uppercase_uuid));
            let reply = serde_json::json!({
                "version": 1,
                "requestId": req["requestId"],
                "clientInstance": uppercase_uuid,
                "generation": req["generation"],
                "nativeInstance": "b0055e38-7b32-430b-b662-adde776a832b",
                "nativeGeneration": 1,
                "state": "released",
                "reason": "native_drained_and_fenced",
                "inputDrained": true,
                "pttDrained": true,
                "lightingDrained": true,
                "capabilities": ["fenced-release-v1"]
            });
            let mut out = serde_json::to_vec(&reply).unwrap();
            out.push(b'\n');
            stream.write_all(&out).unwrap();
        });

        let mut controller = NativeOwnershipController::new(Some(transport), true);
        controller.set_desired_owner("micro-input");
        controller.poll_once(true);
        assert!(controller.allows_execution());
        assert_eq!(controller.summary(), "Cindy 已确认释放");
        server.join().unwrap();
    }

    #[test]
    fn test_deferred_stream_and_poll_once_serialize_per_socket_path() {
        use std::io::{BufRead, BufReader};
        use std::sync::atomic::{AtomicBool, Ordering};

        let dir = tempfile::Builder::new()
            .prefix("mi-serial-")
            .tempdir_in("/tmp")
            .unwrap();
        let sock_path = dir.path().join("cindy-ownership.sock");
        let transport = UnixNativeOwnershipTransport::new(&sock_path);
        let listener = UnixListener::bind(&sock_path).unwrap();
        std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o600)).unwrap();

        let first_in_flight = Arc::new(AtomicBool::new(false));
        let first_in_flight_server = Arc::clone(&first_in_flight);
        let server_transport = transport.clone();

        let server = std::thread::spawn(move || {
            for idx in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                assert!(!first_in_flight_server.swap(true, Ordering::SeqCst));
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                let req: serde_json::Value = serde_json::from_str(&line).unwrap();
                if idx == 0 {
                    std::thread::sleep(Duration::from_millis(80));
                }
                let client_inst: Uuid = req["clientInstance"].as_str().unwrap().parse().unwrap();
                let gen = req["generation"].as_u64().unwrap();
                server_transport
                    .write_persisted_fence(&PersistedNativeOwnershipFence {
                        client_instance: client_inst,
                        generation: gen,
                    })
                    .unwrap();
                let reply = serde_json::json!({
                    "version": 1,
                    "requestId": req["requestId"],
                    "clientInstance": req["clientInstance"],
                    "generation": gen,
                    "nativeInstance": "d1706f6a-dda3-4821-a862-e587e63cc25f",
                    "nativeGeneration": 200 + idx,
                    "state": "unknown",
                    "reason": "native_drain_unconfirmed",
                    "inputDrained": false,
                    "pttDrained": false,
                    "lightingDrained": true,
                    "capabilities": ["fenced-release-v1"]
                });
                let mut out = serde_json::to_vec(&reply).unwrap();
                out.push(b'\n');
                stream.write_all(&out).unwrap();
                first_in_flight_server.store(false, Ordering::SeqCst);
            }
        });

        let mut controller = NativeOwnershipController::new(Some(transport), true);
        controller.set_desired_owner("micro-input");
        let (_req, deferred) = controller
            .poll_fast_or_defer_at(true, Instant::now(), Duration::from_millis(10))
            .expect("first request should defer while server sleeps 80ms");

        let bg = std::thread::spawn(move || {
            UnixNativeOwnershipTransport::finish_stream_reply(
                deferred,
                4096,
                Duration::from_millis(950),
            )
            .unwrap()
        });

        controller.poll_once(true);
        assert!(controller.allows_execution());
        assert!(bg.join().is_ok());
        server.join().unwrap();
    }
}
