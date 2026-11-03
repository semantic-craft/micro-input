mod support;
use async_trait::async_trait;
use base64::Engine;
use mi_proto::{Framer, NativeFramer};
use mi_transport::{HidCandidate, HidLink, HidTransport, LinkBackend, TransportError};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::{
    collections::BTreeMap,
    fs,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use support::{fixture, hardware::*, temp_name};
use tokio::sync::{mpsc, Mutex as AsyncMutex};

#[derive(Clone, Copy, PartialEq)]
enum Fault {
    None,
    NoTruncate,
    AckOnly,
    WrongId,
    ChangeConfig,
    DeleteTimeout,
    Firmware,
    MissingSmart,
    EmptySmart,
    MissingActivity,
    Foreign,
    Huge,
    Collision,
    MutationError(usize),
}
struct Fake {
    inner: fixture::Fixture,
    fault: Fault,
    requests: Arc<Mutex<Vec<Value>>>,
}
struct Link {
    rx: AsyncMutex<mpsc::UnboundedReceiver<Vec<u8>>>,
    tx: mpsc::UnboundedSender<Vec<u8>>,
    files: Mutex<BTreeMap<String, Vec<u8>>>,
    framer: Mutex<NativeFramer>,
    requests: Arc<Mutex<Vec<Value>>>,
    fault: Fault,
    closed: Arc<AtomicBool>,
}
impl Fake {
    fn new(fault: Fault) -> Self {
        Self {
            inner: fixture::Fixture::new(fixture::Fault::None),
            fault,
            requests: Arc::new(Mutex::new(vec![])),
        }
    }
}
impl HidTransport for Fake {
    fn enumerate(&self) -> std::result::Result<Vec<HidCandidate>, TransportError> {
        self.inner.enumerate()
    }
    fn subscribe(&self) -> tokio::sync::broadcast::Receiver<()> {
        self.inner.subscribe()
    }
    fn open(&self, _: &str) -> std::result::Result<Arc<dyn HidLink>, TransportError> {
        self.inner.opens.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::unbounded_channel();
        let mut files = BTreeMap::from([
            ("keymap.json".into(), b"raw unparsed configuration".to_vec()),
            (
                "smart_actions.json".into(),
                b"private smart actions".to_vec(),
            ),
        ]);
        if self.fault == Fault::MissingSmart {
            files.remove("smart_actions.json");
        }
        if self.fault == Fault::EmptySmart {
            files.insert("smart_actions.json".into(), vec![]);
        }
        if self.fault == Fault::Collision {
            files.insert("mi_123456789abc.txt".into(), b"existing".to_vec());
        }
        Ok(Arc::new(Link {
            rx: AsyncMutex::new(rx),
            tx,
            files: Mutex::new(files),
            framer: Mutex::new(NativeFramer::default()),
            requests: self.requests.clone(),
            fault: self.fault,
            closed: self.inner.closed.clone(),
        }))
    }
}
#[async_trait]
impl HidLink for Link {
    fn backend(&self) -> LinkBackend {
        LinkBackend::Unverified
    }
    async fn write(&self, report: &[u8]) -> std::result::Result<(), TransportError> {
        let lines = self.framer.lock().unwrap().feed(report).unwrap();
        for line in lines {
            let req: Value = serde_json::from_slice(&line).unwrap();
            self.requests.lock().unwrap().push(req.clone());
            let method = req["method"].as_str().unwrap();
            let file = req["params"]["file"].as_str().unwrap_or("");
            let count = self
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|v| ["fs.writebin", "fs.delete"].contains(&v["method"].as_str().unwrap()))
                .count();
            if self.fault == Fault::MutationError(count)
                && ["fs.writebin", "fs.delete"].contains(&method)
            {
                let reply = json!({"id":req["id"],"error":{"code":500}});
                for report in NativeFramer::default()
                    .encode(reply.to_string().as_bytes())
                    .unwrap()
                {
                    self.tx.send(report).unwrap();
                }
                continue;
            }
            let mut files = self.files.lock().unwrap();
            let result=match method {
                "device.status"=>json!({"version":if self.fault==Fault::Firmware {"0.7"}else{"0.6.3"},"profile_index":0,"layer_index":if self.fault==Fault::MissingActivity {Value::Null}else{json!(1)}}),
                "fs.list"=>Value::Array(files.iter().map(|(name,bytes)|json!({"name":name,"size":if self.fault==Fault::Huge {1000000}else{bytes.len()},"checksum":format!("{:x}",Sha1::digest(bytes))})).collect()),
                "fs.readbin"=>{let offset=req["params"]["offset"].as_u64().unwrap() as usize;let len=req["params"]["len"].as_u64().unwrap() as usize;let bytes=&files[file];json!({"total_size":bytes.len(),"data":base64::engine::general_purpose::STANDARD.encode(&bytes[offset..bytes.len().min(offset+len)])})},
                "fs.writebin"=>{
                    assert!(temp_name(file).is_ok());
                    assert_eq!(req["params"]["append"],true);
                    let bytes=base64::engine::general_purpose::STANDARD.decode(req["params"]["data"].as_str().unwrap()).unwrap();
                    assert!(bytes.len()<=3072);
                    let offset=req["params"]["offset"].as_u64().unwrap() as usize;
                    if self.fault!=Fault::AckOnly {
                        let data=files.entry(file.into()).or_default();
                        if offset==0 && self.fault!=Fault::NoTruncate {data.clear();}
                        data.resize(data.len().max(offset+bytes.len()),0);
                        data[offset..offset+bytes.len()].copy_from_slice(&bytes);
                    }
                    if self.fault==Fault::ChangeConfig {files.get_mut("keymap.json").unwrap().push(b' ');}
                    Value::Null
                },
                "fs.delete"=>{assert!(temp_name(file).is_ok());files.remove(file);if self.fault==Fault::DeleteTimeout {continue;}Value::Null},
                _=>panic!("unexpected method {method}"),
            };
            let id = if (self.fault == Fault::WrongId && method == "fs.writebin")
                || self.fault == Fault::Foreign
            {
                json!(998)
            } else {
                req["id"].clone()
            };
            let reply = json!({"id":id,"method":method,"result":result});
            for report in NativeFramer::default()
                .encode(reply.to_string().as_bytes())
                .unwrap()
            {
                self.tx.send(report).unwrap();
            }
        }
        Ok(())
    }
    async fn read(&self) -> std::result::Result<Vec<u8>, TransportError> {
        self.rx
            .lock()
            .await
            .recv()
            .await
            .ok_or(TransportError::Disconnected)
    }
    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }
}
fn options(h1: bool) -> Options {
    Options {
        h1,
        authorization: if h1 { H1 } else { H0 }.into(),
        path: "synthetic-no-native-open".into(),
        target: "mi_123456789abc.txt".into(),
        expected_keymap: Some(hash(b"raw unparsed configuration")),
        unresolved_reviewed: true,
    }
}
fn mutations(fake: &Fake) -> Vec<Value> {
    fake.requests
        .lock()
        .unwrap()
        .iter()
        .filter(|v| ["fs.writebin", "fs.delete"].contains(&v["method"].as_str().unwrap()))
        .cloned()
        .collect()
}
#[tokio::test]
async fn h0_backs_up_raw_bytes_without_mutations_and_distinguishes_absence() {
    for fault in [Fault::None, Fault::MissingSmart, Fault::EmptySmart] {
        let fake = Fake::new(fault);
        let out = run(&fake, &options(false), Duration::ZERO).await.unwrap();
        assert!(out.passed, "{}", out.reason);
        assert!(mutations(&fake).is_empty());
        assert_eq!(
            fs::read(out.directory.join("keymap.json")).unwrap(),
            b"raw unparsed configuration"
        );
        assert_eq!(
            out.directory.join("smart_actions.json").exists(),
            fault != Fault::MissingSmart
        );
        if fault == Fault::EmptySmart {
            assert!(fs::read(out.directory.join("smart_actions.json"))
                .unwrap()
                .is_empty());
        }
        assert!(fake.inner.closed.load(Ordering::SeqCst));
        fs::remove_dir_all(out.directory).unwrap();
    }
}
#[tokio::test]
async fn h1_exact_wire_sequence_and_configuration_preservation() {
    let fake = Fake::new(Fault::None);
    let out = run(&fake, &options(true), Duration::ZERO).await.unwrap();
    assert!(out.passed, "{}", out.reason);
    assert!(out.cleanup_confirmed);
    let requests = mutations(&fake);
    assert_eq!(requests.len(), 6);
    let offsets: Vec<_> = requests[..5]
        .iter()
        .map(|v| v["params"]["offset"].as_u64().unwrap())
        .collect();
    assert_eq!(offsets, [0, 3072, 0, 0, 3072]);
    let flags: Vec<_> = requests[..5]
        .iter()
        .map(|v| v["params"]["completed"].as_bool().unwrap())
        .collect();
    assert_eq!(flags, [false, true, true, false, true]);
    let data: Vec<Vec<u8>> = requests[..5]
        .iter()
        .map(|v| {
            base64::engine::general_purpose::STANDARD
                .decode(v["params"]["data"].as_str().unwrap())
                .unwrap()
        })
        .collect();
    assert_eq!(data.iter().map(Vec::len).sum::<usize>(), 6220);
    assert_eq!([data[0].clone(), data[1].clone()].concat(), long());
    assert_eq!(data[2], short());
    assert_eq!(
        fs::read(out.directory.join("keymap.json")).unwrap(),
        b"raw unparsed configuration"
    );
    assert!(fake.inner.closed.load(Ordering::SeqCst));
    fs::remove_dir_all(out.directory).unwrap();
}
#[tokio::test]
async fn h1_stops_without_any_followup_request_after_uncertain_mutation() {
    for (fault, count) in [
        (Fault::NoTruncate, 3),
        (Fault::AckOnly, 1),
        (Fault::WrongId, 1),
        (Fault::ChangeConfig, 1),
        (Fault::DeleteTimeout, 6),
    ] {
        let fake = Fake::new(fault);
        let out = run(&fake, &options(true), Duration::ZERO).await.unwrap();
        assert!(!out.passed);
        assert!(!out.cleanup_confirmed);
        assert_eq!(mutations(&fake).len(), count);
        assert!(fake.inner.closed.load(Ordering::SeqCst));
        if [Fault::WrongId, Fault::DeleteTimeout].contains(&fault) {
            assert!(["fs.writebin", "fs.delete"].contains(
                &fake.requests.lock().unwrap().last().unwrap()["method"]
                    .as_str()
                    .unwrap()
            ));
        }
        fs::remove_dir_all(out.directory).unwrap();
    }
}
#[tokio::test]
async fn incomplete_identity_foreign_traffic_and_budget_fail_before_writes() {
    for fault in [
        Fault::Firmware,
        Fault::MissingActivity,
        Fault::Foreign,
        Fault::Huge,
        Fault::Collision,
    ] {
        let fake = Fake::new(fault);
        let out = run(&fake, &options(true), Duration::ZERO).await.unwrap();
        assert!(!out.passed);
        assert!(mutations(&fake).is_empty());
        fs::remove_dir_all(out.directory).unwrap();
    }
}
#[tokio::test]
async fn invalid_authority_target_review_or_hash_never_enumerates() {
    for n in 0..4 {
        let fake = Fake::new(Fault::None);
        let mut opts = options(true);
        match n {
            0 => opts.authorization = H0.into(),
            1 => opts.target = "keymap.json".into(),
            2 => opts.unresolved_reviewed = false,
            _ => opts.expected_keymap = None,
        };
        assert!(run(&fake, &opts, Duration::ZERO).await.is_err());
        assert_eq!(fake.inner.enumerations.load(Ordering::SeqCst), 0);
    }
}
#[tokio::test]
async fn stale_baseline_and_wrong_path_never_write() {
    let fake = Fake::new(Fault::None);
    let mut opts = options(true);
    opts.path = "other".into();
    assert!(run(&fake, &opts, Duration::ZERO).await.is_err());
    assert_eq!(fake.inner.opens.load(Ordering::SeqCst), 0);
    opts.path = "synthetic-no-native-open".into();
    opts.expected_keymap = Some("0".repeat(64));
    let out = run(&fake, &opts, Duration::ZERO).await.unwrap();
    assert!(!out.passed);
    assert!(mutations(&fake).is_empty());
    fs::remove_dir_all(out.directory).unwrap();
}
#[tokio::test]
#[ignore = "physical read-only backup; explicit H0 launcher only"]
async fn native_h0() {
    native(false).await.unwrap();
}
#[tokio::test]
#[ignore = "physical tempfile mutations; explicit H1 launcher only"]
async fn native_h1() {
    native(true).await.unwrap();
}

#[tokio::test]
async fn each_mutation_failure_is_terminal_without_replay_or_cleanup() {
    for count in 1..=6 {
        let fake = Fake::new(Fault::MutationError(count));
        let out = run(&fake, &options(true), Duration::ZERO).await.unwrap();
        assert!(!out.passed);
        assert!(!out.cleanup_confirmed);
        assert_eq!(mutations(&fake).len(), count);
        assert!(["fs.writebin", "fs.delete"].contains(
            &fake.requests.lock().unwrap().last().unwrap()["method"]
                .as_str()
                .unwrap()
        ));
        assert!(fake.inner.closed.load(Ordering::SeqCst));
        fs::remove_dir_all(out.directory).unwrap();
    }
}
