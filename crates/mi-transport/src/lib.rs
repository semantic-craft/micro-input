//! R-D01/R-D02/R-D07/R-N02: discovery, non-exclusive HID, and synthetic transport.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::sync::{broadcast, mpsc};
#[derive(Debug, Clone, thiserror::Error)]
pub enum TransportError {
    #[error("设备已断开")]
    Disconnected,
    #[error("设备不可用或被占用：{0}")]
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
pub struct HidCandidate {
    pub vid: u16,
    pub pid: u16,
    pub usage_page: u16,
    pub usage: u16,
    pub transport_hint: TransportHint,
    pub product: String,
    pub path: String,
}
pub fn is_candidate(d: &HidCandidate) -> bool {
    d.vid == 0x303a && matches!(d.pid, 0x8297 | 0x8298) && d.usage_page == 0xff00
}
#[async_trait::async_trait]
pub trait HidLink: Send + Sync {
    async fn write(&self, report: &[u8]) -> Result<(), TransportError>;
    async fn read(&self) -> Result<Vec<u8>, TransportError>;
    fn close(&self);
}
pub trait HidTransport: Send + Sync {
    fn enumerate(&self) -> Result<Vec<HidCandidate>, TransportError>;
    fn subscribe(&self) -> broadcast::Receiver<()>;
    fn open(&self, id: &str) -> Result<Arc<dyn HidLink>, TransportError>;
}
#[cfg(target_os = "macos")]
extern "C" {
    fn mi_changes() -> u64;
    fn mi_permission() -> i32;
    fn mi_watch(stop: *const AtomicBool);
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
    stop: Arc<AtomicBool>,
    started: AtomicBool,
}
impl Default for MacTransport {
    fn default() -> Self {
        Self::new()
    }
}
impl MacTransport {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(16);
        let stop = Arc::new(AtomicBool::new(false));
        Self {
            events,
            stop,
            started: AtomicBool::new(false),
        }
    }
    fn start_watching(&self) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        #[cfg(target_os = "macos")]
        {
            let stop = self.stop.clone();
            std::thread::spawn(move || unsafe { mi_watch(Arc::as_ptr(&stop)) });
        }
        let s = self.stop.clone();
        let e = self.events.clone();
        std::thread::spawn(move || {
            #[cfg(target_os = "macos")]
            let mut seen = unsafe { mi_changes() };
            while !s.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(25));
                #[cfg(target_os = "macos")]
                {
                    let now = unsafe { mi_changes() };
                    if now != seen {
                        std::thread::sleep(Duration::from_millis(175));
                        seen = unsafe { mi_changes() };
                        let _ = e.send(());
                    }
                }
            }
        });
    }
}
impl Drop for MacTransport {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
impl HidTransport for MacTransport {
    fn enumerate(&self) -> Result<Vec<HidCandidate>, TransportError> {
        let api = hidapi::HidApi::new().map_err(|e| TransportError::Unavailable(e.to_string()))?;
        Ok(api
            .device_list()
            .map(|d| HidCandidate {
                vid: d.vendor_id(),
                pid: d.product_id(),
                usage_page: d.usage_page(),
                usage: d.usage(),
                // TODO(UNKNOWN-05): only OS Transport (via hidapi bus_type); no release-bit guess or dual-channel dedupe.
                transport_hint: match d.bus_type() {
                    hidapi::BusType::Usb => TransportHint::Usb,
                    hidapi::BusType::Bluetooth => TransportHint::Bluetooth,
                    _ => TransportHint::Unknown,
                },
                product: d.product_string().unwrap_or("未命名 HID").into(),
                path: d.path().to_string_lossy().into_owned(),
            })
            .filter(is_candidate)
            .collect())
    }
    fn subscribe(&self) -> broadcast::Receiver<()> {
        let receiver = self.events.subscribe();
        self.start_watching();
        receiver
    }
    fn open(&self, id: &str) -> Result<Arc<dyn HidLink>, TransportError> {
        let d = self
            .enumerate()?
            .into_iter()
            .find(|d| d.path == id)
            .ok_or(TransportError::Disconnected)?;
        if d.transport_hint != TransportHint::Usb {
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
    async fn write(&self, report: &[u8]) -> Result<(), TransportError> {
        let d = self.device.clone();
        let bytes = report.to_vec();
        tokio::task::spawn_blocking(move || {
            let guard = d.lock().unwrap();
            let dev = guard.as_ref().ok_or(TransportError::Disconnected)?;
            dev.write(&bytes)
                .map(|_| ())
                .map_err(|e| TransportError::Unavailable(e.to_string()))
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
    fn filtering() {
        let mut d = HidCandidate {
            vid: 0x303a,
            pid: 0x8297,
            usage_page: 0xff00,
            usage: 1,
            transport_hint: TransportHint::Unknown,
            product: "synthetic".into(),
            path: "mock".into(),
        };
        assert!(is_candidate(&d));
        d.pid = 0x8360;
        assert!(!is_candidate(&d));
        d.pid = 0x8298;
        d.usage_page = 1;
        assert!(!is_candidate(&d));
    }
}
