//! Independent fake USB system boundary; no native device is opened.
use super::*;
use base64::Engine;
use mi_transport::{TransportHint, UsagePair};
use sha1::{Digest, Sha1};
use tokio::sync::broadcast;
#[derive(Clone, Copy, PartialEq)]
pub enum Fault {
    None,
    Firmware,
    Multiple,
    Foreign,
    ForeignAfterCreate,
    WrongEcho,
    WriteTimeout,
    WriteError,
    ReadMismatch,
    DeleteTimeout,
    ConfigChanged,
    MetadataChanged,
    Disconnect,
}
pub struct Fixture {
    pub opens: AtomicUsize,
    pub enumerations: AtomicUsize,
    pub mutations: Arc<Mutex<Vec<String>>>,
    pub closed: Arc<AtomicBool>,
    fault: Fault,
}
impl Fixture {
    pub fn new(fault: Fault) -> Self {
        Self {
            opens: AtomicUsize::new(0),
            enumerations: AtomicUsize::new(0),
            mutations: Arc::new(Mutex::new(vec![])),
            closed: Arc::new(AtomicBool::new(false)),
            fault,
        }
    }
}
fn device() -> HidCandidate {
    HidCandidate {
        vid: 0x303a,
        pid: 0x8298,
        usage_page: 0xff00,
        usage: 1,
        usage_pairs: vec![UsagePair {
            page: 0xff00,
            usage: 1,
        }],
        transport_hint: TransportHint::Usb,
        product: "Independent synthetic USB".into(),
        path: "synthetic-no-native-open".into(),
    }
}
impl HidTransport for Fixture {
    fn enumerate(&self) -> std::result::Result<Vec<HidCandidate>, TransportError> {
        self.enumerations.fetch_add(1, Ordering::SeqCst);
        Ok(if self.fault == Fault::Multiple {
            vec![device(), device()]
        } else {
            vec![device()]
        })
    }
    fn subscribe(&self) -> broadcast::Receiver<()> {
        broadcast::channel(1).1
    }
    fn open(&self, _: &str) -> std::result::Result<Arc<dyn HidLink>, TransportError> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::unbounded_channel();
        let key = include_bytes!("../../../mi-model/fixtures/device/synthetic-keymap-numeric.json")
            .to_vec();
        let files = BTreeMap::from([
            ("keymap.json".to_string(), key),
            (
                "smart_actions.json".to_string(),
                br#"{"smartActions":{}}"#.to_vec(),
            ),
        ]);
        Ok(Arc::new(FakeLink {
            framer: Mutex::new(NativeFramer::default()),
            files: Mutex::new(files),
            incoming: AsyncMutex::new(rx),
            tx,
            fault: self.fault,
            mutations: self.mutations.clone(),
            closed: self.closed.clone(),
            list_count: AtomicUsize::new(0),
        }))
    }
}
struct FakeLink {
    framer: Mutex<NativeFramer>,
    files: Mutex<BTreeMap<String, Vec<u8>>>,
    incoming: AsyncMutex<mpsc::UnboundedReceiver<Vec<u8>>>,
    tx: mpsc::UnboundedSender<Vec<u8>>,
    fault: Fault,
    mutations: Arc<Mutex<Vec<String>>>,
    closed: Arc<AtomicBool>,
    list_count: AtomicUsize,
}
impl FakeLink {
    fn reply(&self, value: Value) {
        for frame in NativeFramer::default()
            .encode(value.to_string().as_bytes())
            .unwrap()
        {
            let _ = self.tx.send(frame);
        }
    }
}
#[async_trait]
impl HidLink for FakeLink {
    fn backend(&self) -> LinkBackend {
        LinkBackend::Unverified
    }
    async fn write(&self, report: &[u8]) -> std::result::Result<(), TransportError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(TransportError::Disconnected);
        }
        let lines = self.framer.lock().unwrap().feed(report).unwrap();
        for line in lines {
            let req: Value = serde_json::from_slice(&line).unwrap();
            let method = req["method"].as_str().unwrap();
            let diagnostic = ["fs.write", "fs.delete"].contains(&method)
                || (method == "fs.readbin"
                    && req["params"]["file"]
                        .as_str()
                        .is_some_and(|v| v.starts_with("mi_")));
            if diagnostic {
                assert!((900..999).contains(&req["id"].as_u64().unwrap()));
            } else {
                assert!(req["id"].as_u64().unwrap() < 900);
            }
            let file = req["params"]["file"].as_str().unwrap_or("");
            if ["fs.write", "fs.delete"].contains(&method) {
                self.mutations.lock().unwrap().push(method.into());
            }
            let result = match method {
                "device.status" => {
                    json!({"version":if self.fault==Fault::Firmware {"0.6.3-rc.10"} else {"0.6.3"},"profile_index":0,"layer_index":0})
                }
                "sys.version" => json!({"version":"0.6.3"}),
                "fs.list" => {
                    let n = self.list_count.fetch_add(1, Ordering::SeqCst);
                    if (self.fault == Fault::Foreign && n == 0)
                        || (self.fault == Fault::ForeignAfterCreate && n == 4)
                    {
                        self.reply(json!({"id":1000,"result":null}));
                    }
                    if self.fault == Fault::MetadataChanged && n == 3 {
                        self.files
                            .lock()
                            .unwrap()
                            .insert("external.txt".into(), b"external".to_vec());
                    }
                    Value::Array(self.files.lock().unwrap().iter().map(|(name,data)|json!({"name":name,"size":data.len(),"checksum":format!("{:x}",Sha1::digest(data))})).collect())
                }
                "fs.write" => {
                    assert!(temp_name(file).is_ok());
                    assert_eq!(req["params"], json!({"file":file,"data":"{\"ok\":true}"}));
                    self.files
                        .lock()
                        .unwrap()
                        .insert(file.into(), PAYLOAD.to_vec());
                    if self.fault == Fault::ConfigChanged {
                        self.files
                            .lock()
                            .unwrap()
                            .get_mut("keymap.json")
                            .unwrap()
                            .push(b' ');
                    }
                    if self.fault == Fault::WriteTimeout {
                        continue;
                    }
                    if self.fault == Fault::Disconnect {
                        self.close();
                        return Err(TransportError::Disconnected);
                    }
                    if self.fault == Fault::WrongEcho {
                        self.reply(json!({"id":req["id"],"method":"fs.delete","result":null}));
                        continue;
                    }
                    if self.fault == Fault::WriteError {
                        self.reply(json!({"id":req["id"],"error":{"code":500,"message":"synthetic failure"}}));
                        continue;
                    }
                    Value::Null
                }
                "fs.readbin" => {
                    let offset = req["params"]["offset"].as_u64().unwrap() as usize;
                    let len = req["params"]["len"].as_u64().unwrap() as usize;
                    let held = self.files.lock().unwrap();
                    let data = held.get(file).unwrap();
                    let bytes = if file.starts_with("mi_") && self.fault == Fault::ReadMismatch {
                        b"{\"ok\":fals}".as_slice()
                    } else {
                        &data[offset..data.len().min(offset + len)]
                    };
                    json!({"total_size":data.len(),"data":base64::engine::general_purpose::STANDARD.encode(bytes)})
                }
                "fs.delete" => {
                    assert!(temp_name(file).is_ok());
                    assert_eq!(req["params"], json!({"file":file}));
                    self.files.lock().unwrap().remove(file);
                    if self.fault == Fault::DeleteTimeout {
                        continue;
                    }
                    Value::Null
                }
                _ => panic!("unexpected fake boundary method"),
            };
            self.reply(json!({"id":req["id"],"method":method,"result":result}));
        }
        Ok(())
    }
    async fn read(&self) -> std::result::Result<Vec<u8>, TransportError> {
        loop {
            if self.closed.load(Ordering::SeqCst) {
                return Err(TransportError::Disconnected);
            }
            let mut incoming = self.incoming.lock().await;
            if let Ok(Some(report)) =
                tokio::time::timeout(Duration::from_millis(5), incoming.recv()).await
            {
                return Ok(report);
            }
        }
    }
    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }
}
