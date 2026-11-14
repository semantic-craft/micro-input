use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use mi_transport::MockLink;
use serde_json::json;
use sha1::{Digest, Sha1};
fn meta(name: &str, bytes: &[u8]) -> Value {
    json!({"name":name,"size":bytes.len(),"checksum":format!("{:x}",Sha1::digest(bytes))})
}
// Public session API over real report codec, with only the OS transport replaced.
#[tokio::test]
async fn failed_file_transactions_retain_one_complete_stale_snapshot() {
    for fault in [
        "oversize",
        "float-size",
        "missing-checksum",
        "duplicate",
        "missing-keymap",
        "empty-block",
        "invalid-base64",
        "oversized-block",
        "total-drift",
        "bad-checksum",
        "changed-list",
        "smart-appears",
        "invalid-json",
        "smart-empty",
        "smart-wrong-root",
        "rpc-error",
    ] {
        let (link, mut requests) = MockLink::new();
        let session = DeviceSession::new(
            link.clone(),
            Box::new(mi_proto::NativeFramer::default()),
            Arc::new(ReadOnlySchema),
            SessionConfig {
                interval: Duration::ZERO,
                timeout: Duration::from_millis(250),
            },
        );
        let active = Arc::new(Mutex::new(false));
        let enabled = active.clone();
        let inject = link.inject.clone();
        let server = tokio::spawn(async move {
            let original =
                include_bytes!("../../mi-model/fixtures/device/synthetic-keymap-multichunk.json")
                    .to_vec();
            let mut framer = mi_proto::NativeFramer::default();
            let mut lists = 0;
            while let Some(report) = requests.recv().await {
                for line in framer.feed(&report).unwrap() {
                    let request: Value = serde_json::from_slice(&line).unwrap();
                    let m = request["method"].as_str().unwrap();
                    let enabled = *enabled.lock().unwrap();
                    let bytes = if enabled && fault == "invalid-json" {
                        b"{".to_vec()
                    } else {
                        original.clone()
                    };
                    let result = match m {
                        "device.status" => json!({"version":"0.6.3"}),
                        "fs.list" => {
                            lists += 1;
                            let mut file = meta("keymap.json", &bytes);
                            if enabled {
                                match fault {
                                    "oversize" => file["size"] = json!(4 * 1024 * 1024 + 1),
                                    "float-size" => file["size"] = json!(1.5),
                                    "missing-checksum" => {
                                        file.as_object_mut().unwrap().remove("checksum");
                                    }
                                    "bad-checksum" => {
                                        file["checksum"] =
                                            json!("0000000000000000000000000000000000000000")
                                    }
                                    "changed-list" if lists % 2 == 0 => file["size"] = json!(9),
                                    _ => {}
                                }
                            }
                            let mut files = vec![file];
                            if enabled && fault == "duplicate" {
                                files.push(files[0].clone());
                            }
                            if enabled && fault == "missing-keymap" {
                                files.clear();
                            }
                            if enabled
                                && (fault == "smart-empty"
                                    || fault == "smart-wrong-root"
                                    || (fault == "smart-appears" && lists % 2 == 0))
                            {
                                files.push(meta(
                                    "smart_actions.json",
                                    if fault == "smart-wrong-root" {
                                        b"[]"
                                    } else {
                                        b""
                                    },
                                ));
                            }
                            json!(files)
                        }
                        "fs.readbin" => {
                            assert!(matches!(
                                request["params"]["file"].as_str(),
                                Some("keymap.json" | "smart_actions.json")
                            ));
                            let offset = request["params"]["offset"].as_u64().unwrap() as usize;
                            let len = request["params"]["len"].as_u64().unwrap() as usize;
                            let file = if request["params"]["file"] == "smart_actions.json" {
                                b"[]".as_slice()
                            } else {
                                bytes.as_slice()
                            };
                            let data = if enabled && fault == "empty-block" {
                                String::new()
                            } else if enabled && fault == "invalid-base64" {
                                "!garbage".into()
                            } else if enabled && fault == "oversized-block" {
                                STANDARD.encode(vec![0; len + 1])
                            } else {
                                STANDARD.encode(&file[offset..offset + len])
                            };
                            json!({"total_size":if enabled && fault=="total-drift" {file.len()+1}else{file.len()},"data":data})
                        }
                        _ => panic!("forbidden method"),
                    };
                    let response = if enabled && fault == "rpc-error" {
                        json!({"id":request["id"],"error":{"code":500,"message":"synthetic file RPC failure"}})
                    } else {
                        json!({"id":request["id"],"result":result})
                    };
                    for report in mi_proto::NativeFramer::default()
                        .encode(response.to_string().as_bytes())
                        .unwrap()
                    {
                        if inject.send(report).is_err() {
                            return;
                        }
                    }
                }
            }
        });
        session.read_identity_info().await.unwrap();
        session.read_config_snapshot().await.unwrap();
        let old = session.snapshot().unwrap();
        *active.lock().unwrap() = true;
        assert!(session.read_config_snapshot().await.is_err(), "{fault}");
        let retained = session.snapshot().unwrap();
        assert!(retained.stale, "{fault}");
        assert_eq!(
            retained.files.keymap.bytes, old.files.keymap.bytes,
            "{fault}"
        );
        assert_eq!(retained.config.raw, old.config.raw, "{fault}");
        assert_eq!(retained.config.read_at, old.config.read_at, "{fault}");
        session.shutdown().await;
        server.abort();
    }
}
#[tokio::test]
async fn configuration_requires_explicit_supported_identity_and_unknown_sends_nothing() {
    let (link, _) = MockLink::new();
    let session = DeviceSession::new(
        link.clone(),
        Box::new(mi_proto::NativeFramer::default()),
        Arc::new(ReadOnlySchema),
        SessionConfig::default(),
    );
    assert!(session.read_config_snapshot().await.is_err());
    assert!(link.writes.lock().unwrap().is_empty());
    session.shutdown().await;
}

#[tokio::test]
async fn cancelled_read_keeps_old_snapshot_and_releases_queue_for_a_later_attempt() {
    let (link, mut requests) = MockLink::new();
    let session = DeviceSession::new(
        link.clone(),
        Box::new(mi_proto::NativeFramer::default()),
        Arc::new(ReadOnlySchema),
        SessionConfig {
            interval: Duration::ZERO,
            ..SessionConfig::default()
        },
    );
    let stall = Arc::new(AtomicBool::new(false));
    let stalled = Arc::new(Notify::new());
    let (switch, entered, inject) = (stall.clone(), stalled.clone(), link.inject.clone());
    let server = tokio::spawn(async move {
        let bytes = include_bytes!("../../mi-model/fixtures/device/synthetic-keymap-numeric.json");
        let mut framer = mi_proto::NativeFramer::default();
        while let Some(report) = requests.recv().await {
            for line in framer.feed(&report).unwrap() {
                let r: Value = serde_json::from_slice(&line).unwrap();
                let result = match r["method"].as_str().unwrap() {
                    "device.status" => json!({"version":"0.6.3"}),
                    "fs.list" => json!([meta("keymap.json", bytes)]),
                    "fs.readbin" => {
                        if switch.load(Ordering::SeqCst) {
                            entered.notify_one();
                            continue;
                        }
                        let offset = r["params"]["offset"].as_u64().unwrap() as usize;
                        let len = r["params"]["len"].as_u64().unwrap() as usize;
                        json!({"total_size":bytes.len(),"data":STANDARD.encode(&bytes[offset..offset+len])})
                    }
                    _ => panic!("forbidden method"),
                };
                for report in mi_proto::NativeFramer::default()
                    .encode(json!({"id":r["id"],"result":result}).to_string().as_bytes())
                    .unwrap()
                {
                    if inject.send(report).is_err() {
                        return;
                    }
                }
            }
        }
    });
    session.read_identity_info().await.unwrap();
    session.read_config_snapshot().await.unwrap();
    let old = session.snapshot().unwrap();
    stall.store(true, Ordering::SeqCst);
    let s = session.clone();
    let attempt = tokio::spawn(async move { s.read_config_snapshot().await });
    tokio::time::timeout(Duration::from_secs(1), stalled.notified())
        .await
        .unwrap();
    assert!(
        session.read_config_snapshot().await.is_err(),
        "duplicate read must not queue"
    );
    attempt.abort();
    assert!(attempt.await.unwrap_err().is_cancelled());
    assert!(session.snapshot().unwrap().stale);
    assert_eq!(
        session.snapshot().unwrap().files.keymap.bytes,
        old.files.keymap.bytes
    );
    stall.store(false, Ordering::SeqCst);
    session.read_config_snapshot().await.unwrap();
    assert!(!session.snapshot().unwrap().stale);
    session.shutdown().await;
    assert!(session.read_config_snapshot().await.is_err());
    assert!(session.snapshot().unwrap().stale);
    server.abort();
}

#[tokio::test]
async fn config_snapshot_accepts_v070_rc16_identity_read() {
    let (link, mut requests) = MockLink::new();
    let session = DeviceSession::new(
        link.clone(),
        Box::<mi_proto::NativeFramer>::default(),
        Arc::new(ReadOnlySchema),
        SessionConfig {
            timeout: Duration::from_secs(1),
            interval: Duration::ZERO,
        },
    );
    let inject = link.inject.clone();
    let server = tokio::spawn(async move {
        let bytes = include_bytes!("../../mi-model/fixtures/device/synthetic-keymap-numeric.json");
        let mut framer = mi_proto::NativeFramer::default();
        while let Some(report) = requests.recv().await {
            for line in framer.feed(&report).unwrap() {
                let r: Value = serde_json::from_slice(&line).unwrap();
                let result = match r["method"].as_str().unwrap() {
                    "device.status" => json!({"version": "0.7.0-rc.16"}),
                    "fs.list" => json!([meta("keymap.json", bytes)]),
                    "fs.readbin" => {
                        let offset = r["params"]["offset"].as_u64().unwrap() as usize;
                        let len = r["params"]["len"].as_u64().unwrap() as usize;
                        json!({"total_size": bytes.len(), "data": STANDARD.encode(&bytes[offset..offset + len])})
                    }
                    _ => panic!("forbidden method"),
                };
                for report in mi_proto::NativeFramer::default()
                    .encode(
                        json!({"id": r["id"], "result": result})
                            .to_string()
                            .as_bytes(),
                    )
                    .unwrap()
                {
                    if inject.send(report).is_err() {
                        return;
                    }
                }
            }
        }
    });
    let identity = session.read_identity_info().await.unwrap();
    assert_eq!(identity.version.as_deref(), Some("0.7.0-rc.16"));
    session.read_config_snapshot().await.unwrap();
    assert!(!session.snapshot().unwrap().stale);
    session.shutdown().await;
    server.abort();
}
