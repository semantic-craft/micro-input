//! R-D01/R-D02/R-D07/R-N02: discovery, non-exclusive HID, and synthetic transport.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tokio::sync::{broadcast, mpsc};
#[derive(Debug, Clone, thiserror::Error)]
pub enum TransportError {
    #[error("设备已断开")]
    Disconnected,
    #[error("设备 I/O 失败（原因未确认）：{0}")]
    Unavailable(String),
    #[error("仅支持 USB；连接类型待确认")]
    Unsupported,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransportHint {
    Usb,
    Bluetooth,
    Unknown,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UsagePair {
    pub page: u16,
    pub usage: u16,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HidCandidate {
    pub vid: u16,
    pub pid: u16,
    pub usage_page: u16,
    pub usage: u16,
    pub usage_pairs: Vec<UsagePair>,
    pub transport_hint: TransportHint,
    pub product: String,
    pub path: String,
}
pub fn is_candidate(d: &HidCandidate) -> bool {
    d.vid == 0x303a
        && matches!(d.pid, 0x8297 | 0x8298)
        && d.transport_hint == TransportHint::Usb
        && (d
            .usage_pairs
            .iter()
            .any(|p| p.page == 0xff00 && p.usage == 1)
            || (d.usage_pairs.is_empty() && d.usage_page == 0xff00 && d.usage == 1))
}
/// R-D13/E-075: selected by compiled adapters, never by environment variables or RPC input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkBackend {
    Unverified,
    MacHidapi267Numbered,
    #[cfg(feature = "synthetic-backend")]
    SyntheticNumbered,
}
#[async_trait::async_trait]
pub trait HidLink: Send + Sync {
    fn backend(&self) -> LinkBackend {
        LinkBackend::Unverified
    }
    async fn write(&self, report: &[u8]) -> Result<(), TransportError>;
    async fn read(&self) -> Result<Vec<u8>, TransportError>;
    fn close(&self);
}
pub trait HidTransport: Send + Sync {
    fn enumerate(&self) -> Result<Vec<HidCandidate>, TransportError>;
    #[cfg(feature = "synthetic-backend")]
    fn set_synthetic_discovery(&self, _state: u8) -> Result<(), TransportError> {
        Err(TransportError::Unsupported)
    }
    #[cfg(feature = "synthetic-backend")]
    fn set_synthetic_identity(&self, _scenario: u8) -> Result<(), TransportError> {
        Err(TransportError::Unsupported)
    }
    #[cfg(feature = "synthetic-backend")]
    fn set_synthetic_config(&self, _scenario: u8) -> Result<(), TransportError> {
        Err(TransportError::Unsupported)
    }
    #[cfg(feature = "synthetic-backend")]
    fn synthetic_requests(&self) -> Vec<String> {
        vec![]
    }
    #[cfg(feature = "synthetic-backend")]
    fn emit_synthetic_input(&self, _count: u16) -> Result<(), TransportError> {
        Err(TransportError::Unsupported)
    }
    #[cfg(feature = "synthetic-backend")]
    fn emit_synthetic_input_case(&self, _input_case: u8) -> Result<(), TransportError> {
        Err(TransportError::Unsupported)
    }
    #[cfg(feature = "synthetic-backend")]
    fn emit_synthetic_binding(
        &self,
        _binding_id: &str,
        _pressed: bool,
    ) -> Result<(), TransportError> {
        Err(TransportError::Unsupported)
    }
    fn subscribe(&self) -> broadcast::Receiver<()>;
    fn stop_discovery(&self) {}
    fn open(&self, id: &str) -> Result<Arc<dyn HidLink>, TransportError>;
}
#[cfg(target_os = "macos")]
extern "C" {
    fn mi_enumerate(
        callback: extern "C" fn(
            *mut std::ffi::c_void,
            u16,
            u16,
            u16,
            u16,
            *const u16,
            usize,
            *const std::ffi::c_char,
            *const std::ffi::c_char,
            *const std::ffi::c_char,
        ),
        context: *mut std::ffi::c_void,
    ) -> i32;
    fn mi_permission() -> i32;
    fn mi_watch(
        stop: *const AtomicBool,
        changed: extern "C" fn(*mut std::ffi::c_void),
        context: *mut std::ffi::c_void,
    );
}
pub fn input_monitoring_allowed() -> bool {
    #[cfg(target_os = "macos")]
    {
        unsafe { mi_permission() != 0 }
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}
pub struct MacTransport {
    events: broadcast::Sender<()>,
    watcher: Mutex<Option<(Arc<AtomicBool>, std::thread::JoinHandle<()>)>>,
}
impl Default for MacTransport {
    fn default() -> Self {
        Self::new()
    }
}
impl MacTransport {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(16);
        Self {
            events,
            watcher: Mutex::new(None),
        }
    }
    fn start_watching(&self) {
        let mut watcher = self.watcher.lock().unwrap();
        if watcher.is_some() {
            return;
        }
        #[cfg(target_os = "macos")]
        {
            let stop = Arc::new(AtomicBool::new(false));
            let thread_stop = stop.clone();
            let events = self.events.clone();
            let thread = std::thread::spawn(move || unsafe {
                // The callback only borrows this sender while mi_watch owns the run loop.
                mi_watch(
                    Arc::as_ptr(&thread_stop),
                    discovery_changed,
                    &events as *const _ as *mut std::ffi::c_void,
                );
            });
            *watcher = Some((stop, thread));
        }
    }
    fn stop_watching(&self) {
        let watcher = self.watcher.lock().unwrap().take();
        if let Some((stop, thread)) = watcher {
            stop.store(true, Ordering::SeqCst);
            let _ = thread.join();
        }
    }
}
impl Drop for MacTransport {
    fn drop(&mut self) {
        self.stop_watching();
    }
}
#[cfg(target_os = "macos")]
extern "C" fn discovery_changed(context: *mut std::ffi::c_void) {
    let sender = unsafe { &*(context as *const broadcast::Sender<()>) };
    let _ = sender.send(());
}

#[cfg(target_os = "macos")]
extern "C" fn discovered(
    context: *mut std::ffi::c_void,
    vid: u16,
    pid: u16,
    usage_page: u16,
    usage: u16,
    pairs: *const u16,
    count: usize,
    transport: *const std::ffi::c_char,
    product: *const std::ffi::c_char,
    path: *const std::ffi::c_char,
) {
    // The native adapter owns these values for the duration of this synchronous callback.
    unsafe {
        let string = |p| std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned();
        let candidate = HidCandidate {
            vid,
            pid,
            usage_page,
            usage,
            usage_pairs: std::slice::from_raw_parts(pairs, count * 2)
                .chunks_exact(2)
                .map(|p| UsagePair {
                    page: p[0],
                    usage: p[1],
                })
                .collect(),
            transport_hint: match string(transport).as_str() {
                "USB" => TransportHint::Usb,
                "Bluetooth" | "Bluetooth Low Energy" => TransportHint::Bluetooth,
                _ => TransportHint::Unknown,
            },
            product: string(product),
            path: string(path),
        };
        if is_candidate(&candidate) {
            (*(context as *mut Vec<HidCandidate>)).push(candidate);
        }
    }
}
// TODO(UNKNOWN-05): USB source is OS-derived; BLE and cross-interface physical identity remain unverified.
impl HidTransport for MacTransport {
    fn enumerate(&self) -> Result<Vec<HidCandidate>, TransportError> {
        #[cfg(target_os = "macos")]
        {
            let mut devices: Vec<HidCandidate> = Vec::new();
            let result = unsafe {
                mi_enumerate(discovered, &mut devices as *mut _ as *mut std::ffi::c_void)
            };
            if result != 0 {
                return Err(TransportError::Unavailable("系统 HID 枚举失败".into()));
            }
            devices.sort_by(|a, b| a.path.cmp(&b.path));
            Ok(devices)
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err(TransportError::Unsupported)
        }
    }

    fn subscribe(&self) -> broadcast::Receiver<()> {
        let receiver = self.events.subscribe();
        self.start_watching();
        receiver
    }
    fn stop_discovery(&self) {
        self.stop_watching();
    }
    fn open(&self, id: &str) -> Result<Arc<dyn HidLink>, TransportError> {
        let d = self
            .enumerate()?
            .into_iter()
            .find(|d| d.path == id)
            .ok_or(TransportError::Disconnected)?;
        if d.transport_hint != TransportHint::Usb || d.pid != 0x8298 {
            return Err(TransportError::Unsupported);
        }
        let api = hidapi::HidApi::new().map_err(|e| TransportError::Unavailable(e.to_string()))?;
        let path =
            std::ffi::CString::new(id).map_err(|e| TransportError::Unavailable(e.to_string()))?;
        // macos-shared-device feature sets hid_darwin_set_open_exclusive(0).
        let device = api
            .open_path(&path)
            .map_err(|e| TransportError::Unavailable(e.to_string()))?;
        Ok(Arc::new(MacLink {
            device: Arc::new(Mutex::new(Some(device))),
        }))
    }
}
struct MacLink {
    device: Arc<Mutex<Option<hidapi::HidDevice>>>,
}
#[async_trait::async_trait]
impl HidLink for MacLink {
    fn backend(&self) -> LinkBackend {
        if cfg!(target_os = "macos") {
            LinkBackend::MacHidapi267Numbered
        } else {
            LinkBackend::Unverified
        }
    }
    async fn write(&self, report: &[u8]) -> Result<(), TransportError> {
        let d = self.device.clone();
        let bytes = report.to_vec();
        tokio::task::spawn_blocking(move || {
            let guard = d.lock().unwrap();
            let dev = guard.as_ref().ok_or(TransportError::Disconnected)?;
            let sent = dev
                .write(&bytes)
                .map_err(|e| TransportError::Unavailable(e.to_string()))?;
            if sent != bytes.len() {
                return Err(TransportError::Unavailable(
                    "short numbered report send".into(),
                ));
            }
            Ok(())
        })
        .await
        .map_err(|e| TransportError::Unavailable(e.to_string()))?
    }
    async fn read(&self) -> Result<Vec<u8>, TransportError> {
        loop {
            let d = self.device.clone();
            let bytes = tokio::task::spawn_blocking(move || {
                let guard = d.lock().unwrap();
                let dev = guard.as_ref().ok_or(TransportError::Disconnected)?;
                let mut b = [0; 256];
                let n = dev
                    .read_timeout(&mut b, 20)
                    .map_err(|e| TransportError::Unavailable(e.to_string()))?;
                Ok::<_, TransportError>(b[..n].to_vec())
            })
            .await
            .map_err(|e| TransportError::Unavailable(e.to_string()))??;
            if !bytes.is_empty() {
                return Ok(bytes);
            }
        }
    }
    fn close(&self) {
        self.device.lock().unwrap().take();
    }
}
/// R-N04: programmable mock, never an alternate physical wire implementation.
pub struct MockLink {
    incoming: tokio::sync::Mutex<mpsc::UnboundedReceiver<Vec<u8>>>,
    pub inject: mpsc::UnboundedSender<Vec<u8>>,
    pub outgoing: mpsc::UnboundedSender<Vec<u8>>,
    closed: AtomicBool,
    changed: tokio::sync::Notify,
    pub writes: Mutex<Vec<(std::time::Instant, Vec<u8>)>>,
}
impl MockLink {
    pub fn new() -> (Arc<Self>, mpsc::UnboundedReceiver<Vec<u8>>) {
        let (inject, incoming) = mpsc::unbounded_channel();
        let (outgoing, rx) = mpsc::unbounded_channel();
        (
            Arc::new(Self {
                incoming: tokio::sync::Mutex::new(incoming),
                inject,
                outgoing,
                closed: AtomicBool::new(false),
                changed: tokio::sync::Notify::new(),
                writes: Mutex::new(Vec::new()),
            }),
            rx,
        )
    }
}
#[async_trait::async_trait]
impl HidLink for MockLink {
    async fn write(&self, r: &[u8]) -> Result<(), TransportError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(TransportError::Disconnected);
        }
        self.writes
            .lock()
            .unwrap()
            .push((std::time::Instant::now(), r.to_vec()));
        self.outgoing
            .send(r.to_vec())
            .map_err(|_| TransportError::Disconnected)
    }
    async fn read(&self) -> Result<Vec<u8>, TransportError> {
        let notified = self.changed.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if self.closed.load(Ordering::SeqCst) {
            return Err(TransportError::Disconnected);
        }
        tokio::select! {_=notified=>Err(TransportError::Disconnected),v=async{self.incoming.lock().await.recv().await}=>v.ok_or(TransportError::Disconnected)}
    }
    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.changed.notify_waiters();
    }
}
pub struct MockTransport {
    pub devices: Mutex<Vec<HidCandidate>>,
    pub files: Mutex<BTreeMap<String, Vec<u8>>>,
    pub link: Arc<MockLink>,
    events: broadcast::Sender<()>,
}
impl MockTransport {
    pub fn new(link: Arc<MockLink>) -> Self {
        let (events, _) = broadcast::channel(16);
        Self {
            devices: Mutex::new(Vec::new()),
            files: Mutex::new(BTreeMap::new()),
            link,
            events,
        }
    }
    pub fn set_devices(&self, ds: Vec<HidCandidate>) {
        *self.devices.lock().unwrap() = ds;
        let _ = self.events.send(());
    }
}
impl HidTransport for MockTransport {
    fn enumerate(&self) -> Result<Vec<HidCandidate>, TransportError> {
        Ok(self
            .devices
            .lock()
            .unwrap()
            .iter()
            .filter(|d| is_candidate(d))
            .cloned()
            .collect())
    }
    fn subscribe(&self) -> broadcast::Receiver<()> {
        self.events.subscribe()
    }
    fn open(&self, _: &str) -> Result<Arc<dyn HidLink>, TransportError> {
        Ok(self.link.clone())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vendor_collection_is_found_when_primary_usage_is_keyboard() {
        let d = HidCandidate {
            vid: 0x303a,
            pid: 0x8298,
            usage_page: 1,
            usage: 6,
            usage_pairs: vec![
                UsagePair { page: 1, usage: 6 },
                UsagePair {
                    page: 0xff00,
                    usage: 1,
                },
            ],
            transport_hint: TransportHint::Usb,
            product: "Synthetic".into(),
            path: "interface-a".into(),
        };
        assert!(is_candidate(&d));
    }
    #[test]
    fn filtering() {
        let mut d = HidCandidate {
            vid: 0x303a,
            pid: 0x8297,
            usage_page: 0xff00,
            usage: 1,
            usage_pairs: vec![],
            transport_hint: TransportHint::Usb,
            product: "synthetic".into(),
            path: "mock".into(),
        };
        assert!(is_candidate(&d));
        // R-D01: a vendor page alone cannot identify the RPC interface.
        d.usage = 2;
        assert!(!is_candidate(&d));
        d.usage = 1;
        d.transport_hint = TransportHint::Bluetooth;
        assert!(!is_candidate(&d));
        d.transport_hint = TransportHint::Usb;
        d.pid = 0x8360;
        assert!(!is_candidate(&d));
        d.pid = 0x8298;
        d.usage_page = 1;
        assert!(!is_candidate(&d));
    }
}

// R-D13/D05: explicit test-only OS discovery boundary; no physical open is possible.
#[cfg(feature = "synthetic-backend")]
pub struct SyntheticDiscovery {
    files: Arc<Mutex<Option<SyntheticFiles>>>,
    state: Mutex<u8>,
    identity: std::sync::atomic::AtomicU8,
    configuration: Arc<std::sync::atomic::AtomicU8>,
    input_inject: Mutex<Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>>,
    requests: Arc<Mutex<Vec<String>>>,
    events: broadcast::Sender<()>,
}
#[cfg(feature = "synthetic-backend")]
impl Default for SyntheticDiscovery {
    fn default() -> Self {
        let (events, _) = broadcast::channel(16);
        Self {
            state: Mutex::new(0),
            files: Arc::new(Mutex::new(None)),
            identity: std::sync::atomic::AtomicU8::new(0),
            configuration: Arc::new(std::sync::atomic::AtomicU8::new(0)),
            input_inject: Mutex::new(None),
            requests: Arc::new(Mutex::new(vec![])),
            events,
        }
    }
}
#[cfg(feature = "synthetic-backend")]
impl HidTransport for SyntheticDiscovery {
    fn enumerate(&self) -> Result<Vec<HidCandidate>, TransportError> {
        let state = *self.state.lock().unwrap();
        match state {
            0 => Ok(vec![]),
            1 | 3 => Ok((0..if state == 3 { 1 } else { 2 })
                .map(|i| HidCandidate {
                    vid: 0x303a,
                    pid: if i == 0 { 0x8298 } else { 0x8297 },
                    usage_page: 1,
                    usage: 6,
                    usage_pairs: vec![
                        UsagePair { page: 1, usage: 6 },
                        UsagePair {
                            page: 0xff00,
                            usage: 1,
                        },
                    ],
                    transport_hint: TransportHint::Usb,
                    product: "合成 Creator Micro".into(),
                    path: if state == 3 {
                        "synthetic-reconnected-interface-0".into()
                    } else {
                        format!("synthetic-interface-{i}")
                    },
                })
                .collect()),
            _ => Err(TransportError::Unavailable("合成枚举失败".into())),
        }
    }
    fn subscribe(&self) -> broadcast::Receiver<()> {
        self.events.subscribe()
    }
    fn open(&self, id: &str) -> Result<Arc<dyn HidLink>, TransportError> {
        let state = *self.state.lock().unwrap();
        if !((state == 1 && id == "synthetic-interface-0")
            || (state == 3 && id == "synthetic-reconnected-interface-0"))
        {
            return Err(TransportError::Unsupported);
        }
        let (link, mut incoming) = MockLink::new();
        let scenario = self.identity.load(Ordering::SeqCst);
        let configuration = self.configuration.clone();
        let files = self.files.clone();
        let requests = self.requests.clone();
        let inject = link.inject.clone();
        *self.input_inject.lock().unwrap() = Some(inject.clone());
        tokio::spawn(async move {
            let mut bytes = vec![];
            let mut lists = 0;
            while let Some(report) = incoming.recv().await {
                if report.len() != 64 || report[0] != 6 || report[1] != 2 || report[2] > 61 {
                    break;
                }
                bytes.extend_from_slice(&report[3..3 + report[2] as usize]);
                let Ok(request) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                    continue;
                };
                bytes.clear();
                let method = request["method"].as_str().unwrap_or("");
                requests.lock().unwrap().push(method.to_owned());
                let config = configuration.load(Ordering::SeqCst);
                let mutable_reply = if [
                    "fs.list",
                    "fs.readbin",
                    "fs.writebin",
                    "synthetic.lights.read",
                    "synthetic.lights.set",
                ]
                .contains(&method)
                {
                    files.lock().unwrap().as_mut().map(|f| f.respond(&request))
                } else {
                    None
                };
                let response = if let Some(reply) = mutable_reply {
                    let Some(reply) = reply else { continue };
                    reply
                } else if method == "fs.writebin" {
                    serde_json::json!({"id":request["id"],"error":{"code":403,"message":"synthetic writes not enabled"}})
                } else if method == "fs.list" || method == "fs.readbin" {
                    use base64::Engine;
                    let keymap: &[u8] = if config == 4 {
                        include_bytes!(
                            "../../mi-model/fixtures/device/synthetic-keymap-reconnected.json"
                        )
                    } else {
                        include_bytes!(
                            "../../mi-model/fixtures/device/synthetic-keymap-multichunk.json"
                        )
                    };
                    let smart = include_bytes!(
                        "../../mi-model/fixtures/device/synthetic-smart-actions-object.json"
                    );
                    let result = if method == "fs.list" {
                        lists += 1;
                        serde_json::json!([
                            {"name":"keymap.json","size":if config==2 && lists%2==0 {1} else {12466},"checksum":if config==4 {"8629249fa26f62ab5cbe37dbbacf8b6813e6560a"}else{"29982c129d7250b624d483d1bf2a29544271507e"}},
                            {"name":"smart_actions.json","size":738,"checksum":"e60e82aba8a5f71ba7726febac9310e4007ed2ab"}
                        ])
                    } else {
                        let file: &[u8] = if request["params"]["file"] == "keymap.json" {
                            keymap
                        } else {
                            smart
                        };
                        let offset = request["params"]["offset"].as_u64().unwrap_or(0) as usize;
                        let len = request["params"]["len"].as_u64().unwrap_or(0) as usize;
                        let chunk = file
                            .get(offset..offset.saturating_add(len))
                            .unwrap_or_default();
                        serde_json::json!({"total_size":file.len(),"data":if config==3 {"!invalid".into()} else {base64::engine::general_purpose::STANDARD.encode(chunk)}})
                    };
                    if config == 0 {
                        serde_json::json!({"id":request["id"],"error":{"code":503,"message":"synthetic configuration unavailable"}})
                    } else {
                        serde_json::json!({"id":request["id"],"result":result})
                    }
                } else if scenario == 3 {
                    serde_json::json!({"id":request["id"],"result":{"version":"synthetic-0.6.3","profile_index":0,"layer_index":0}})
                } else if scenario == 2 {
                    serde_json::json!({"id":request["id"],"error":{"code":500,"message":"synthetic identity failure"}})
                } else if method == "device.status" && scenario == 1 {
                    serde_json::json!({"id":request["id"],"result":{"profile_index":-1,"layer_index":1.5}})
                } else {
                    serde_json::json!({"id":request["id"],"result":{"version":"synthetic-0.6.3","profile_index":2,"layer_index":1}})
                };
                for part in response.to_string().as_bytes().chunks(61) {
                    let mut r = vec![0; 64];
                    r[0] = 6;
                    r[1] = 2;
                    r[2] = part.len() as u8;
                    r[3..3 + part.len()].copy_from_slice(part);
                    if inject.send(r).is_err() {
                        return;
                    }
                }
            }
        });
        Ok(Arc::new(SyntheticLink(link)))
    }
    fn emit_synthetic_input(&self, count: u16) -> Result<(), TransportError> {
        if count > 1024 {
            return Err(TransportError::Unsupported);
        }
        let inject = self.input_inject.lock().unwrap();
        let inject = inject.as_ref().ok_or(TransportError::Disconnected)?;
        // Test-only unknown notification. This does not specify a physical control schema.
        let bytes = br#"{"m":"test.synthetic","p":"private payload must not cross FFI"}"#;
        for _ in 0..count {
            for part in bytes.chunks(61) {
                let mut report = vec![0; 64];
                report[0] = 6;
                report[1] = 2;
                report[2] = part.len() as u8;
                report[3..3 + part.len()].copy_from_slice(part);
                inject
                    .send(report)
                    .map_err(|_| TransportError::Disconnected)?;
            }
        }
        Ok(())
    }
    fn emit_synthetic_input_case(&self, input_case: u8) -> Result<(), TransportError> {
        // Original synthetic protocol facts, not captured device or manufacturer payloads.
        let messages = match input_case {
            0 => vec![
                serde_json::json!({"m":"v.oai.hid","p":{"k":"AG00","act":1}}),
                serde_json::json!({"m":"v.oai.hid","p":{"k":"AG00","act":0}}),
                serde_json::json!({"m":"v.oai.hid","p":{"k":"AG13","act":1}}),
                serde_json::json!({"m":"v.oai.hid","p":{"k":"AG13","act":0}}),
                serde_json::json!({"m":"v.oai.hid","p":{"k":"AG11","act":1}}),
                serde_json::json!({"m":"v.oai.hid","p":{"k":"AG10","act":1}}),
                serde_json::json!({"m":"v.oai.hid","p":{"k":"AG10","act":0}}),
                serde_json::json!({"m":"v.oai.hid","p":{"k":"AG11","act":0}}),
            ],
            1 => vec![
                serde_json::json!({"m":"v.oai.hid","p":{"k":"AG00","act":2}}),
                serde_json::json!({"m":"v.oai.hid","p":{"k":"AG19","act":1}}),
                serde_json::json!({"m":"v.oai.hid","p":{"k":"ENC_CW","act":2}}),
                serde_json::json!({"m":"v.oai.hid","p":{"k":"AG00","act":true}}),
                serde_json::json!({"m":"test.synthetic","p":"private payload must not cross FFI"}),
            ],
            _ => return Err(TransportError::Unsupported),
        };
        let inject = self.input_inject.lock().unwrap();
        let inject = inject.as_ref().ok_or(TransportError::Disconnected)?;
        for message in messages {
            for part in message.to_string().as_bytes().chunks(61) {
                let mut report = vec![0; 64];
                report[0] = 6;
                report[1] = 2;
                report[2] = part.len() as u8;
                report[3..3 + part.len()].copy_from_slice(part);
                inject
                    .send(report)
                    .map_err(|_| TransportError::Disconnected)?;
            }
        }
        Ok(())
    }
    fn emit_synthetic_binding(
        &self,
        binding_id: &str,
        pressed: bool,
    ) -> Result<(), TransportError> {
        let digits = binding_id
            .strip_prefix("AG")
            .ok_or(TransportError::Unsupported)?;
        if digits.len() != 2
            || !digits.bytes().all(|b| b.is_ascii_digit())
            || digits
                .parse::<u8>()
                .map_err(|_| TransportError::Unsupported)?
                > 18
        {
            return Err(TransportError::Unsupported);
        }
        let message = serde_json::json!({"m":"v.oai.hid","p":{"k":binding_id,"act":if pressed { 1 } else { 0 }}});
        let inject = self.input_inject.lock().unwrap();
        let inject = inject.as_ref().ok_or(TransportError::Disconnected)?;
        for part in message.to_string().as_bytes().chunks(61) {
            let mut report = vec![0; 64];
            report[0] = 6;
            report[1] = 2;
            report[2] = part.len() as u8;
            report[3..3 + part.len()].copy_from_slice(part);
            inject
                .send(report)
                .map_err(|_| TransportError::Disconnected)?;
        }
        Ok(())
    }
    fn set_synthetic_identity(&self, scenario: u8) -> Result<(), TransportError> {
        self.identity.store(scenario, Ordering::SeqCst);
        Ok(())
    }
    fn set_synthetic_config(&self, scenario: u8) -> Result<(), TransportError> {
        self.configuration.store(scenario, Ordering::SeqCst);
        Ok(())
    }
    fn synthetic_requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
    fn set_synthetic_discovery(&self, state: u8) -> Result<(), TransportError> {
        *self.state.lock().unwrap() = state;
        let _ = self.events.send(());
        Ok(())
    }
}

#[cfg(feature = "synthetic-backend")]
struct SyntheticLink(Arc<MockLink>);
#[cfg(feature = "synthetic-backend")]
#[async_trait::async_trait]
impl HidLink for SyntheticLink {
    fn backend(&self) -> LinkBackend {
        LinkBackend::SyntheticNumbered
    }
    async fn write(&self, r: &[u8]) -> Result<(), TransportError> {
        self.0.write(r).await
    }
    async fn read(&self) -> Result<Vec<u8>, TransportError> {
        self.0.read().await
    }
    fn close(&self) {
        self.0.close();
    }
}

/// Mutable, explicitly injected synthetic files; never populated from user configuration.
#[cfg(feature = "synthetic-backend")]
#[derive(Clone)]
pub struct SyntheticFiles {
    pub keymap: Vec<u8>,
    pub smart_actions: Option<Vec<u8>>,
    /// Reject the chunk at this byte offset before storing it.
    pub reject_offset: Option<usize>,
    /// Store the chunk but lose its response, modeling uncertain completion.
    pub lose_ack_offset: Option<usize>,
    pub corrupt_completed: bool,
    pub missing_keymap: bool,
    pub bad_checksum: bool,
    pub fail_read: bool,
    pub preview_target: u64,
    pub preview_lights: serde_json::Value,
    pub fail_preview: bool,
    next_offset: usize,
}
#[cfg(feature = "synthetic-backend")]
impl SyntheticFiles {
    pub fn new(keymap: Vec<u8>, smart_actions: Option<Vec<u8>>) -> Self {
        Self {
            keymap,
            smart_actions,
            reject_offset: None,
            lose_ack_offset: None,
            corrupt_completed: false,
            missing_keymap: false,
            bad_checksum: false,
            fail_read: false,
            preview_target: 0,
            preview_lights: serde_json::Value::Null,
            fail_preview: false,
            next_offset: 0,
        }
    }
    fn respond(&mut self, request: &serde_json::Value) -> Option<serde_json::Value> {
        use base64::{engine::general_purpose::STANDARD, Engine};
        use serde_json::json;
        use sha1::{Digest, Sha1};
        let params = &request["params"];
        let result: Result<serde_json::Value, ()> = (|| {
            match request["method"].as_str() {
                Some("synthetic.lights.read" | "synthetic.lights.set") => {
                    if self.fail_preview {
                        return Err(());
                    }
                    if self.preview_lights.is_null() {
                        let source: serde_json::Value =
                            serde_json::from_slice(&self.keymap).map_err(|_| ())?;
                        self.preview_lights = source["profiles"][0]["layers"][0]["lights"].clone();
                    }
                    if request["method"] == "synthetic.lights.set" {
                        if params["target"].as_u64() != Some(self.preview_target)
                            || params.get("expected") != Some(&self.preview_lights)
                        {
                            return Err(());
                        }
                        self.preview_lights = params["lights"].clone();
                    }
                    Ok(json!({"target":self.preview_target,"lights":self.preview_lights}))
                }

                Some("fs.list") => {
                    let mut files = vec![
                        json!({"name":"keymap.json","size":self.keymap.len(),"checksum":format!("{:x}",Sha1::digest(&self.keymap))}),
                    ];
                    if self.missing_keymap {
                        files.clear();
                    } else if self.bad_checksum {
                        files[0]["checksum"] = json!("0000000000000000000000000000000000000000");
                    }
                    if let Some(smart) = &self.smart_actions {
                        files.push(json!({"name":"smart_actions.json","size":smart.len(),"checksum":format!("{:x}",Sha1::digest(smart))}));
                    }
                    Ok(json!(files))
                }
                Some("fs.readbin") => {
                    if self.fail_read {
                        return Err(());
                    }
                    let bytes = match params["file"].as_str() {
                        Some("keymap.json") => &self.keymap,
                        Some("smart_actions.json") => self.smart_actions.as_ref().ok_or(())?,
                        _ => return Err(()),
                    };
                    let offset =
                        usize::try_from(params["offset"].as_u64().ok_or(())?).map_err(|_| ())?;
                    let len = usize::try_from(params["len"].as_u64().ok_or(())?).map_err(|_| ())?;
                    if len == 0 || len > 3072 || offset > bytes.len() {
                        return Err(());
                    }
                    Ok(
                        json!({"total_size":bytes.len(),"data":STANDARD.encode(&bytes[offset..offset.saturating_add(len).min(bytes.len())])}),
                    )
                }
                Some("fs.writebin") => {
                    if params.as_object().is_none_or(|p| p.len() != 5)
                        || params["file"] != "keymap.json"
                        || params["append"] != true
                    {
                        return Err(());
                    }
                    let offset =
                        usize::try_from(params["offset"].as_u64().ok_or(())?).map_err(|_| ())?;
                    let complete = params["completed"].as_bool().ok_or(())?;
                    let data = STANDARD
                        .decode(params["data"].as_str().ok_or(())?)
                        .map_err(|_| ())?;
                    if self.reject_offset == Some(offset)
                        || data.is_empty()
                        || data.len() > 3072
                        || offset.saturating_add(data.len()) > 4 * 1024 * 1024
                        || (offset != 0 && offset != self.next_offset)
                    {
                        return Err(());
                    }
                    // Original synthetic failure model: visible partial overwrite, no atomicity promise.
                    if offset == 0 {
                        self.keymap.clear();
                        self.next_offset = 0;
                    }
                    self.keymap.extend_from_slice(&data);
                    self.next_offset = self.keymap.len();
                    if complete {
                        self.next_offset = 0;
                        if self.corrupt_completed {
                            self.keymap.push(b'!');
                        }
                    }
                    Ok(json!({"synthetic_written":true}))
                }
                _ => Err(()),
            }
        })();
        if request["method"] == "fs.writebin"
            && result.is_ok()
            && self.lose_ack_offset == params["offset"].as_u64().map(|n| n as usize)
        {
            return None;
        }
        Some(match result {
            Ok(result) => json!({"id":request["id"],"result":result}),
            Err(()) => {
                json!({"id":request["id"],"error":{"code":500,"message":"synthetic file fault"}})
            }
        })
    }
}
#[cfg(feature = "synthetic-backend")]
impl SyntheticDiscovery {
    pub fn set_files(&self, files: SyntheticFiles) {
        *self.files.lock().unwrap() = Some(files);
    }
    pub fn files(&self) -> Option<SyntheticFiles> {
        self.files.lock().unwrap().clone()
    }
}
