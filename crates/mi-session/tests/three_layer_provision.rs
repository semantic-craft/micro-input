use base64::{engine::general_purpose::STANDARD, Engine};
use mi_model::apply::{
    compile_three_layer_profile, recognize_three_layer_profile, ThreeLayerBoard,
    ThreeLayerProvision,
};
use mi_proto::{Framer, NativeFramer};
use mi_session::{DeviceSession, ReadOnlySchema, SessionConfig};
use mi_transport::{HidLink, LinkBackend, MockLink, TransportError};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::{path::Path, sync::Arc, time::Duration};

const KEYMAP: &[u8] =
    include_bytes!("../../mi-model/fixtures/device/synthetic-keymap-numeric.json");
const SMART: &[u8] =
    include_bytes!("../../mi-model/fixtures/device/synthetic-smart-actions-object.json");

#[test]
fn adding_a_profile_only_splices_the_active_index_and_one_array_entry() {
    let ThreeLayerProvision::Write {
        profile_index,
        keymap,
    } = compile_three_layer_profile(KEYMAP, Some(SMART)).unwrap()
    else {
        panic!("new profile")
    };
    assert_eq!(profile_index, 2);
    let candidate = mi_model::parse_device_files(&keymap, Some(SMART)).unwrap();
    assert_eq!(recognize_three_layer_profile(&candidate).unwrap(), Some(2));
    let added = &candidate.raw["profiles"][2];
    let added_bytes = serde_json::to_string(added).unwrap();
    let without_added = String::from_utf8(keymap.clone())
        .unwrap()
        .replace(&format!(",{added_bytes}"), "");
    let exact_original =
        without_added.replacen("\"activeProfileId\": 2", "\"activeProfileId\": 1", 1);
    assert_eq!(exact_original.as_bytes(), KEYMAP);
    assert!(matches!(
        compile_three_layer_profile(&keymap, Some(SMART)).unwrap(),
        ThreeLayerProvision::AlreadySelected { profile_index: 2 }
    ));
    let drifted = String::from_utf8(keymap)
        .unwrap()
        .replace("\"DeepSeek Harness\"", "\"User DSH\"");
    assert!(compile_three_layer_profile(drifted.as_bytes(), Some(SMART)).is_err());
}

struct ExclusiveFixture(Arc<MockLink>);

#[async_trait::async_trait]
impl HidLink for ExclusiveFixture {
    fn backend(&self) -> LinkBackend {
        LinkBackend::MacHidapi267Numbered
    }

    fn is_exclusive_vendor(&self) -> bool {
        true
    }

    async fn write(&self, report: &[u8]) -> Result<(), TransportError> {
        self.0.write(report).await
    }

    async fn read(&self) -> Result<Vec<u8>, TransportError> {
        self.0.read().await
    }

    fn close(&self) {
        self.0.close();
    }
}

fn metadata(name: &str, bytes: &[u8]) -> Value {
    json!({"name":name,"size":bytes.len(),"checksum":format!("{:x}",Sha1::digest(bytes))})
}

#[derive(Clone, Copy)]
enum Commit {
    ConfirmLayerOne,
    StayInLayerTwo,
    Ignore,
}

fn fixture(
    parent: &Path,
    commit: Commit,
) -> (
    Arc<DeviceSession>,
    tokio::task::JoinHandle<(Vec<u8>, usize)>,
) {
    let (link, mut requests) = MockLink::new();
    let inject = link.inject.clone();
    let parent = parent.to_owned();
    let session = DeviceSession::new(
        Arc::new(ExclusiveFixture(link)),
        Box::<NativeFramer>::default(),
        Arc::new(ReadOnlySchema),
        SessionConfig {
            timeout: Duration::from_secs(2),
            interval: Duration::ZERO,
        },
    );
    let worker = tokio::spawn(async move {
        let mut incoming = NativeFramer::default();
        let outbound = NativeFramer::default();
        let mut active = 1;
        let mut keymap = KEYMAP.to_vec();
        let mut staged = vec![];
        let mut writes = 0;
        while let Some(frame) = requests.recv().await {
            for bytes in incoming.feed(&frame).unwrap() {
                let request: Value = serde_json::from_slice(&bytes).unwrap();
                let params = &request["params"];
                let result = match request["method"].as_str().unwrap() {
                    "device.status" => {
                        let layer = if active == 2 && matches!(commit, Commit::StayInLayerTwo) {
                            2
                        } else {
                            1
                        };
                        json!({"version":"0.6.3","profile_index":active,"layer_index":layer})
                    }
                    "fs.list" => json!([
                        metadata("keymap.json", &keymap),
                        metadata("smart_actions.json", SMART)
                    ]),
                    "fs.readbin" => {
                        let bytes = match params["file"].as_str().unwrap() {
                            "keymap.json" => keymap.as_slice(),
                            "smart_actions.json" => SMART,
                            file => panic!("unexpected read {file}"),
                        };
                        let offset = params["offset"].as_u64().unwrap() as usize;
                        let limit = params["len"].as_u64().unwrap() as usize;
                        json!({"total_size":bytes.len(),"data":STANDARD.encode(&bytes[offset..(offset+limit).min(bytes.len())])})
                    }
                    "fs.txbegin" => {
                        let mut backups =
                            std::fs::read_dir(parent.join("three-layer-device-backups")).unwrap();
                        let backup = backups.next().unwrap().unwrap().path();
                        assert!(backups.next().is_none());
                        assert_eq!(std::fs::read(backup.join("keymap.json")).unwrap(), KEYMAP);
                        assert_eq!(
                            std::fs::read(backup.join("smart_actions.json")).unwrap(),
                            SMART
                        );
                        assert!(backup.join("manifest.json").is_file());
                        json!({"tx":17})
                    }
                    "fs.writebin" => {
                        assert_eq!(params["file"], "keymap.json");
                        assert_eq!(params["tx"], 17);
                        assert_eq!(params["offset"], staged.len());
                        staged.extend(STANDARD.decode(params["data"].as_str().unwrap()).unwrap());
                        writes += 1;
                        Value::Null
                    }
                    "fs.txcommit" => {
                        assert_eq!(params["tx"], 17);
                        if !matches!(commit, Commit::Ignore) {
                            keymap = std::mem::take(&mut staged);
                            let applied: Value = serde_json::from_slice(&keymap).unwrap();
                            active = applied["activeProfileId"].as_u64().unwrap();
                        }
                        Value::Null
                    }
                    method => panic!("unexpected method {method}"),
                };
                let response = json!({"id":request["id"],"result":result});
                let mut wire = response.to_string();
                wire.push('\n');
                for frame in outbound.encode(wire.as_bytes()).unwrap() {
                    inject.send(frame).unwrap();
                }
            }
        }
        (keymap, writes)
    });
    (session, worker)
}

#[tokio::test]
async fn provisioning_preserves_original_data_and_is_idempotent_after_confirmed_readback() {
    let dir = tempfile::tempdir().unwrap();
    let (session, worker) = fixture(dir.path(), Commit::ConfirmLayerOne);
    assert_eq!(session.inspect_three_layer_profile().await.unwrap(), None);
    let expected = ThreeLayerBoard {
        profile_index: 2,
        layer_index: 1,
    };
    assert_eq!(
        session
            .provision_three_layer_profile(dir.path())
            .await
            .unwrap(),
        expected
    );
    assert_eq!(
        session.inspect_three_layer_profile().await.unwrap(),
        Some(expected)
    );
    assert_eq!(
        session
            .provision_three_layer_profile(dir.path())
            .await
            .unwrap(),
        expected
    );
    session.shutdown().await;
    drop(session);
    let (keymap, writes) = worker.await.unwrap();
    assert!(writes > 1, "fixture should exercise more than one chunk");
    assert_eq!(
        writes,
        keymap.len().div_ceil(mi_proto::mutation::CHUNK_BYTES)
    );
}

#[tokio::test]
async fn successful_commit_reply_without_matching_bytes_never_confirms_three_layer_profile() {
    let dir = tempfile::tempdir().unwrap();
    let (session, worker) = fixture(dir.path(), Commit::Ignore);
    let err = session
        .provision_three_layer_profile(dir.path())
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("板载配置读回不一致"));
    assert!(err.contains("副本"));
    assert_eq!(session.inspect_three_layer_profile().await.unwrap(), None);
    assert!(session.has_exclusive_vendor_link());
    session.shutdown().await;
    drop(session);
    let (keymap, writes) = worker.await.unwrap();
    assert_eq!(keymap, KEYMAP);
    let ThreeLayerProvision::Write {
        keymap: candidate, ..
    } = compile_three_layer_profile(KEYMAP, Some(SMART)).unwrap()
    else {
        panic!("new profile")
    };
    assert_eq!(
        writes,
        candidate.len().div_ceil(mi_proto::mutation::CHUNK_BYTES)
    );
}

#[tokio::test]
async fn exact_candidate_with_unconfirmed_first_layer_restores_original_without_releasing_lease() {
    let dir = tempfile::tempdir().unwrap();
    let (session, worker) = fixture(dir.path(), Commit::StayInLayerTwo);
    let error = session
        .provision_three_layer_profile(dir.path())
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("固件未确认第 1 层"), "{error}");
    assert!(error.contains("已恢复原板载配置并核对两份文件"), "{error}");
    assert_eq!(session.inspect_three_layer_profile().await.unwrap(), None);
    assert!(session.has_exclusive_vendor_link());
    session.shutdown().await;
    drop(session);
    let (keymap, writes) = worker.await.unwrap();
    assert_eq!(keymap, KEYMAP);
    assert!(writes > keymap.len().div_ceil(mi_proto::mutation::CHUNK_BYTES));
}

#[tokio::test]
async fn explicit_recovery_is_bound_to_the_successful_write_and_current_lease() {
    let dir = tempfile::tempdir().unwrap();
    let (session, worker) = fixture(dir.path(), Commit::ConfirmLayerOne);
    let receipt = session
        .provision_three_layer_profile_with_recovery(dir.path())
        .await
        .unwrap();
    let (other_link, _other_frames) = MockLink::new();
    let other_session = DeviceSession::new(
        Arc::new(ExclusiveFixture(other_link.clone())),
        Box::<NativeFramer>::default(),
        Arc::new(ReadOnlySchema),
        SessionConfig {
            timeout: Duration::from_millis(60),
            interval: Duration::ZERO,
        },
    );
    let stale = other_session
        .restore_three_layer_profile(&receipt)
        .await
        .unwrap_err()
        .to_string();
    assert!(stale.contains("原独占 USB 会话已失效"), "{stale}");
    assert!(other_link.writes.lock().unwrap().is_empty());
    other_session.shutdown().await;
    let backup = receipt.backup_path().unwrap();
    assert_eq!(std::fs::read(backup.join("keymap.json")).unwrap(), KEYMAP);
    assert_eq!(
        std::fs::read(backup.join("smart_actions.json")).unwrap(),
        SMART
    );
    assert!(session.restore_three_layer_profile(&receipt).await.unwrap());
    let err = session
        .restore_three_layer_profile(&receipt)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("候选不完全相同，拒绝覆盖"), "{err}");
    assert!(session.has_exclusive_vendor_link());
    session.shutdown().await;
    drop(session);
    let (keymap, _) = worker.await.unwrap();
    assert_eq!(keymap, KEYMAP);
}

#[tokio::test]
async fn six_layer_prepare_apply_readonly_verify_and_restore_roundtrip() {
    six_layer_roundtrip(false).await;
    six_layer_roundtrip(true).await;
}

async fn six_layer_roundtrip(retired: bool) {
    const SIX_SOURCE: &[u8] =
        include_bytes!("../../mi-model/fixtures/device/synthetic-six-layer-source.json");
    let dir = tempfile::tempdir().unwrap();
    let (link, mut requests) = MockLink::new();
    let inject = link.inject.clone();
    let session = DeviceSession::new(
        Arc::new(ExclusiveFixture(link)),
        Box::<NativeFramer>::default(),
        Arc::new(ReadOnlySchema),
        SessionConfig {
            timeout: Duration::from_secs(2),
            interval: Duration::ZERO,
        },
    );
    let mut old_candidate: Value = serde_json::from_slice(
        &mi_model::apply::compile_six_layer_profile(SIX_SOURCE, Some(SMART))
            .unwrap()
            .keymap,
    )
    .unwrap();
    let old_layer = &mut old_candidate["profiles"][1]["layers"][3];
    old_layer["name"] = json!("Cindy");
    let tokens: Vec<_> = mi_model::direct_controls::DIRECT_CONTROLS[40..60]
        .iter()
        .map(|c| c.token)
        .collect();
    let rows = json!([
        &tokens[0..2],
        &tokens[2..6],
        &tokens[6..10],
        &tokens[10..13]
    ]);
    old_layer["layout"]["base"] = rows.clone();
    old_layer["layout"]["keymap"] = rows;
    old_layer["layout"]["encoders"] = json!([&tokens[13..16]]);
    for slot in 0..4 {
        old_layer["layout"]["joystick"]["sectors"][slot]["k"] = json!(tokens[16 + slot]);
    }
    let old_candidate = serde_json::to_vec(&old_candidate).unwrap();
    assert!(mi_model::apply::recognize_six_layer_profile(&old_candidate, Some(SMART)).is_err());
    assert!(mi_model::apply::compile_six_layer_profile(&old_candidate, Some(SMART)).is_err());
    let initial = if retired {
        old_candidate.clone()
    } else {
        SIX_SOURCE.to_vec()
    };
    let worker = tokio::spawn(async move {
        let mut incoming = NativeFramer::default();
        let outbound = NativeFramer::default();
        let mut active = u64::from(retired);
        let mut keymap = initial;
        let mut staged = vec![];
        let mut commits = 0usize;
        while let Some(frame) = requests.recv().await {
            for bytes in incoming.feed(&frame).unwrap() {
                let request: Value = serde_json::from_slice(&bytes).unwrap();
                let params = &request["params"];
                let result = match request["method"].as_str().unwrap() {
                    "device.status" => {
                        json!({"version":"0.6.3","profile_index":active,"layer_index":1})
                    }
                    "fs.list" => json!([
                        metadata("keymap.json", &keymap),
                        metadata("smart_actions.json", SMART)
                    ]),
                    "fs.readbin" => {
                        let bytes = match params["file"].as_str().unwrap() {
                            "keymap.json" => keymap.as_slice(),
                            "smart_actions.json" => SMART,
                            file => panic!("unexpected read {file}"),
                        };
                        let offset = params["offset"].as_u64().unwrap() as usize;
                        let limit = params["len"].as_u64().unwrap() as usize;
                        json!({"total_size":bytes.len(),"data":STANDARD.encode(&bytes[offset..(offset+limit).min(bytes.len())])})
                    }
                    "fs.txbegin" => json!({"tx":23}),
                    "fs.writebin" => {
                        assert_eq!(params["file"], "keymap.json");
                        assert_eq!(params["tx"], 23);
                        assert_eq!(params["offset"], staged.len());
                        staged.extend(STANDARD.decode(params["data"].as_str().unwrap()).unwrap());
                        Value::Null
                    }
                    "fs.txcommit" => {
                        assert_eq!(params["tx"], 23);
                        keymap = std::mem::take(&mut staged);
                        let applied: Value = serde_json::from_slice(&keymap).unwrap();
                        active = applied["activeProfileId"].as_u64().unwrap();
                        commits += 1;
                        Value::Null
                    }
                    method => panic!("unexpected method {method}"),
                };
                let response = json!({"id":request["id"],"result":result});
                let mut wire = response.to_string();
                wire.push('\n');
                for frame in outbound.encode(wire.as_bytes()).unwrap() {
                    inject.send(frame).unwrap();
                }
            }
        }
        (keymap, commits)
    });

    let backup = if retired {
        std::fs::write(dir.path().join("keymap.json"), SIX_SOURCE).unwrap();
        std::fs::write(dir.path().join("planned-keymap.json"), &old_candidate).unwrap();
        std::fs::write(dir.path().join("smart_actions.json"), SMART).unwrap();
        dir.path().to_path_buf()
    } else {
        let prepared = session.prepare_six_layer_write(dir.path()).await.unwrap();
        let candidate = prepared.candidate.clone();
        let backup = prepared.backup.clone();
        let receipt = session.apply_six_layer_write(prepared).await.unwrap();
        assert_eq!(receipt.profile_index, 1);
        assert_eq!(receipt.layer_index, 1);
        let verified_layer = session
            .verify_six_layer_files(&candidate, Some(SMART), receipt.profile_index)
            .await
            .unwrap();
        assert_eq!(verified_layer, 1);
        backup
    };

    if retired {
        let mut altered: Value = serde_json::from_slice(&old_candidate).unwrap();
        altered["profiles"][1]["layers"][3]["layout"]["keymap"][0][0] = json!("KC_A");
        std::fs::write(
            backup.join("planned-keymap.json"),
            serde_json::to_vec(&altered).unwrap(),
        )
        .unwrap();
        assert!(session.restore_six_layer_backup(&backup).await.is_err());
        std::fs::write(backup.join("planned-keymap.json"), &old_candidate).unwrap();
    }
    session.restore_six_layer_backup(&backup).await.unwrap();
    session.shutdown().await;
    drop(session);
    let (final_keymap, commits) = worker.await.unwrap();
    assert_eq!(final_keymap, SIX_SOURCE);
    assert_eq!(commits, if retired { 1 } else { 2 });
}

#[tokio::test]
async fn six_layer_outdated_layout_upgrades_in_place_at_same_profile_index_preserving_layer_zero_bytes(
) {
    const SIX_SOURCE: &[u8] =
        include_bytes!("../../mi-model/fixtures/device/synthetic-six-layer-source.json");
    let v2_template = mi_model::apply::compile_six_layer_profile(SIX_SOURCE, Some(SMART)).unwrap();
    let mut v1_value: Value = serde_json::from_slice(&v2_template.keymap).unwrap();
    for layer_idx in 1..6 {
        let tokens: Vec<&str> = if (1..=2).contains(&layer_idx) {
            mi_model::direct_controls::DIRECT_CONTROLS[(layer_idx - 1) * 20..layer_idx * 20]
                .iter()
                .map(|c| c.token)
                .collect()
        } else {
            vec!["KC_NONE"; 20]
        };
        let rows = json!([
            &tokens[0..2],
            &tokens[2..6],
            &tokens[6..10],
            &tokens[10..13]
        ]);
        let layer_obj = v1_value["profiles"][1]["layers"][layer_idx]
            .as_object_mut()
            .unwrap();
        layer_obj.remove("lights");
        layer_obj["layout"]["base"] = rows.clone();
        layer_obj["layout"]["keymap"] = rows;
        layer_obj["layout"]["encoders"] = json!([&tokens[13..16]]);
        for slot in 0..4 {
            layer_obj["layout"]["joystick"]["sectors"][slot]["k"] = json!(tokens[16 + slot]);
        }
    }
    use serde_json::value::RawValue;
    use std::collections::BTreeMap;
    let raw_doc: BTreeMap<&str, &RawValue> = serde_json::from_slice(SIX_SOURCE).unwrap();
    let raw_profiles: Vec<BTreeMap<&str, &RawValue>> =
        serde_json::from_str(raw_doc["profiles"].get()).unwrap();
    let raw_layers: Vec<&RawValue> = serde_json::from_str(raw_profiles[0]["layers"].get()).unwrap();
    let raw_layer0 = raw_layers[0].get();
    let raw_source_text = std::str::from_utf8(SIX_SOURCE).unwrap();
    let v1_managed_strings: Vec<String> = (1..6)
        .map(|i| serde_json::to_string(&v1_value["profiles"][1]["layers"][i]).unwrap())
        .collect();
    let v1_profile = format!(
        "{{\"id\":1,\"name\":\"{}\",\"layers\":[{},{}]}}",
        mi_model::apply::SIX_LAYER_PROFILE_NAME,
        raw_layer0,
        v1_managed_strings.join(",")
    );
    let v1_bytes = raw_source_text
        .replacen("\"activeProfileId\": 0", "\"activeProfileId\": 1", 1)
        .replacen(
            "  }] }],\n  \"macros\"",
            &format!("  }}] }},{v1_profile}],\n  \"macros\""),
            1,
        )
        .into_bytes();

    assert!(mi_model::apply::recognize_six_layer_profile(&v1_bytes, Some(SMART)).is_err());
    assert_eq!(
        mi_model::apply::recognize_six_layer_recovery_candidate(&v1_bytes, Some(SMART)).unwrap(),
        Some(1)
    );

    let dir = tempfile::tempdir().unwrap();
    let (link, mut requests) = MockLink::new();
    let inject = link.inject.clone();
    let session = DeviceSession::new(
        Arc::new(ExclusiveFixture(link)),
        Box::<NativeFramer>::default(),
        Arc::new(ReadOnlySchema),
        SessionConfig {
            timeout: Duration::from_secs(2),
            interval: Duration::ZERO,
        },
    );
    let v1_initial = v1_bytes.clone();
    let worker = tokio::spawn(async move {
        let mut incoming = NativeFramer::default();
        let outbound = NativeFramer::default();
        let mut active = 1u64;
        let mut keymap = v1_initial;
        let mut staged = vec![];
        while let Some(frame) = requests.recv().await {
            for bytes in incoming.feed(&frame).unwrap() {
                let request: Value = serde_json::from_slice(&bytes).unwrap();
                let params = &request["params"];
                let result = match request["method"].as_str().unwrap() {
                    "device.status" => {
                        json!({"version":"0.6.3","profile_index":active,"layer_index":2})
                    }
                    "fs.list" => json!([
                        metadata("keymap.json", &keymap),
                        metadata("smart_actions.json", SMART)
                    ]),
                    "fs.readbin" => {
                        let bytes = match params["file"].as_str().unwrap() {
                            "keymap.json" => keymap.as_slice(),
                            "smart_actions.json" => SMART,
                            file => panic!("unexpected read {file}"),
                        };
                        let offset = params["offset"].as_u64().unwrap() as usize;
                        let limit = params["len"].as_u64().unwrap() as usize;
                        json!({"total_size":bytes.len(),"data":STANDARD.encode(&bytes[offset..(offset+limit).min(bytes.len())])})
                    }
                    "fs.txbegin" => json!({"tx":31}),
                    "fs.writebin" => {
                        staged.extend(STANDARD.decode(params["data"].as_str().unwrap()).unwrap());
                        Value::Null
                    }
                    "fs.txcommit" => {
                        keymap = std::mem::take(&mut staged);
                        let applied: Value = serde_json::from_slice(&keymap).unwrap();
                        active = applied["activeProfileId"].as_u64().unwrap();
                        Value::Null
                    }
                    method => panic!("unexpected method {method}"),
                };
                let response = json!({"id":request["id"],"result":result});
                let mut wire = response.to_string();
                wire.push('\n');
                for frame in outbound.encode(wire.as_bytes()).unwrap() {
                    inject.send(frame).unwrap();
                }
            }
        }
        keymap
    });

    assert_eq!(
        session
            .matches_six_layer_snapshot(&v1_bytes, Some(SMART), 1)
            .await
            .unwrap(),
        mi_session::SixLayerSnapshotCheck::Matched
    );
    assert_eq!(
        session
            .matches_six_layer_snapshot(&v1_bytes, Some(SMART), 0)
            .await
            .unwrap(),
        mi_session::SixLayerSnapshotCheck::StatusNotReady
    );
    assert_eq!(
        session
            .matches_six_layer_snapshot(SIX_SOURCE, Some(SMART), 1)
            .await
            .unwrap(),
        mi_session::SixLayerSnapshotCheck::FilesDiverged
    );
    assert!(session
        .verify_six_layer_files(&v1_bytes, Some(SMART), 1)
        .await
        .is_err());

    let prepared = session.prepare_six_layer_write(dir.path()).await.unwrap();
    assert_eq!(prepared.profile_index, 1);
    let candidate = prepared.candidate.clone();
    let receipt = session.apply_six_layer_write(prepared).await.unwrap();
    assert_eq!(receipt.profile_index, 1);
    assert_eq!(receipt.layer_index, 2);
    assert_eq!(
        session
            .verify_six_layer_files(&candidate, Some(SMART), 1)
            .await
            .unwrap(),
        2
    );

    session.shutdown().await;
    drop(session);
    let final_keymap = worker.await.unwrap();
    let final_text = std::str::from_utf8(&final_keymap).unwrap();
    assert!(final_text.contains(raw_layer0));
    let final_json: Value = serde_json::from_slice(&final_keymap).unwrap();
    assert_eq!(final_json["profiles"].as_array().unwrap().len(), 2);
    assert_eq!(
        final_json["profiles"][1]["layers"][1]["layout"]["keymap"][0][0],
        "KV_OAI_AG06"
    );
    assert_eq!(
        final_json["profiles"][1]["layers"][2]["layout"]["keymap"][0][0],
        "KV_OAI_AG12"
    );
}

#[tokio::test]
async fn six_layer_upgrade_mid_write_failure_recovers_original_via_previous_planned_keymap() {
    const SIX_SOURCE: &[u8] =
        include_bytes!("../../mi-model/fixtures/device/synthetic-six-layer-source.json");
    let v2_template = mi_model::apply::compile_six_layer_profile(SIX_SOURCE, Some(SMART)).unwrap();
    let mut v1_value: Value = serde_json::from_slice(&v2_template.keymap).unwrap();
    for layer_idx in 1..6 {
        let tokens: Vec<&str> = if (1..=2).contains(&layer_idx) {
            mi_model::direct_controls::DIRECT_CONTROLS[(layer_idx - 1) * 20..layer_idx * 20]
                .iter()
                .map(|c| c.token)
                .collect()
        } else {
            vec!["KC_NONE"; 20]
        };
        let rows = json!([
            &tokens[0..2],
            &tokens[2..6],
            &tokens[6..10],
            &tokens[10..13]
        ]);
        let layer_obj = v1_value["profiles"][1]["layers"][layer_idx]
            .as_object_mut()
            .unwrap();
        layer_obj.remove("lights");
        layer_obj["layout"]["base"] = rows.clone();
        layer_obj["layout"]["keymap"] = rows;
        layer_obj["layout"]["encoders"] = json!([&tokens[13..16]]);
        for slot in 0..4 {
            layer_obj["layout"]["joystick"]["sectors"][slot]["k"] = json!(tokens[16 + slot]);
        }
    }
    use serde_json::value::RawValue;
    use std::collections::BTreeMap;
    let raw_doc: BTreeMap<&str, &RawValue> = serde_json::from_slice(SIX_SOURCE).unwrap();
    let raw_profiles: Vec<BTreeMap<&str, &RawValue>> =
        serde_json::from_str(raw_doc["profiles"].get()).unwrap();
    let raw_layers: Vec<&RawValue> = serde_json::from_str(raw_profiles[0]["layers"].get()).unwrap();
    let raw_layer0 = raw_layers[0].get();
    let v1_managed_strings: Vec<String> = (1..6)
        .map(|i| serde_json::to_string(&v1_value["profiles"][1]["layers"][i]).unwrap())
        .collect();
    let v1_profile = format!(
        "{{\"id\":1,\"name\":\"{}\",\"layers\":[{},{}]}}",
        mi_model::apply::SIX_LAYER_PROFILE_NAME,
        raw_layer0,
        v1_managed_strings.join(",")
    );
    let v1_bytes = std::str::from_utf8(SIX_SOURCE)
        .unwrap()
        .replacen("\"activeProfileId\": 0", "\"activeProfileId\": 1", 1)
        .replacen(
            "  }] }],\n  \"macros\"",
            &format!("  }}] }},{v1_profile}],\n  \"macros\""),
            1,
        )
        .into_bytes();

    let dir = tempfile::tempdir().unwrap();
    let (prep_link, mut prep_reqs) = MockLink::new();
    let prep_inject = prep_link.inject.clone();
    let prep_session = DeviceSession::new(
        Arc::new(ExclusiveFixture(prep_link)),
        Box::<NativeFramer>::default(),
        Arc::new(ReadOnlySchema),
        SessionConfig {
            timeout: Duration::from_secs(2),
            interval: Duration::ZERO,
        },
    );
    let prep_v1 = v1_bytes.clone();
    let prep_worker = tokio::spawn(async move {
        let mut incoming = NativeFramer::default();
        let outbound = NativeFramer::default();
        while let Some(frame) = prep_reqs.recv().await {
            for bytes in incoming.feed(&frame).unwrap() {
                let request: Value = serde_json::from_slice(&bytes).unwrap();
                let params = &request["params"];
                let result = match request["method"].as_str().unwrap() {
                    "device.status" => json!({"version":"0.6.3","profile_index":1,"layer_index":1}),
                    "fs.list" => json!([
                        metadata("keymap.json", &prep_v1),
                        metadata("smart_actions.json", SMART)
                    ]),
                    "fs.readbin" => {
                        let bytes = match params["file"].as_str().unwrap() {
                            "keymap.json" => prep_v1.as_slice(),
                            "smart_actions.json" => SMART,
                            file => panic!("unexpected read {file}"),
                        };
                        let offset = params["offset"].as_u64().unwrap() as usize;
                        let limit = params["len"].as_u64().unwrap() as usize;
                        json!({"total_size":bytes.len(),"data":STANDARD.encode(&bytes[offset..(offset+limit).min(bytes.len())])})
                    }
                    method => panic!("unexpected method {method}"),
                };
                let response = json!({"id":request["id"],"result":result});
                let mut wire = response.to_string();
                wire.push('\n');
                for frame in outbound.encode(wire.as_bytes()).unwrap() {
                    prep_inject.send(frame).unwrap();
                }
            }
        }
    });

    let prepared = prep_session
        .prepare_six_layer_write(dir.path())
        .await
        .unwrap();
    let backup = prepared.backup.clone();
    prep_session.shutdown().await;
    drop(prep_session);
    prep_worker.await.unwrap();

    std::fs::write(backup.join("keymap.json"), SIX_SOURCE).unwrap();
    std::fs::write(backup.join("previous-planned-keymap.json"), &v1_bytes).unwrap();

    let (rec_link, mut rec_reqs) = MockLink::new();
    let rec_inject = rec_link.inject.clone();
    let rec_session = DeviceSession::new(
        Arc::new(ExclusiveFixture(rec_link)),
        Box::<NativeFramer>::default(),
        Arc::new(ReadOnlySchema),
        SessionConfig {
            timeout: Duration::from_secs(2),
            interval: Duration::ZERO,
        },
    );
    let still_v1 = v1_bytes.clone();
    let rec_worker = tokio::spawn(async move {
        let mut incoming = NativeFramer::default();
        let outbound = NativeFramer::default();
        let mut keymap = still_v1;
        let mut staged = Vec::new();
        while let Some(frame) = rec_reqs.recv().await {
            for bytes in incoming.feed(&frame).unwrap() {
                let request: Value = serde_json::from_slice(&bytes).unwrap();
                let params = &request["params"];
                let result = match request["method"].as_str().unwrap() {
                    "device.status" => json!({"version":"0.6.3","profile_index":1,"layer_index":1}),
                    "fs.list" => json!([
                        metadata("keymap.json", &keymap),
                        metadata("smart_actions.json", SMART)
                    ]),
                    "fs.readbin" => {
                        let bytes = match params["file"].as_str().unwrap() {
                            "keymap.json" => keymap.as_slice(),
                            "smart_actions.json" => SMART,
                            file => panic!("unexpected read {file}"),
                        };
                        let offset = params["offset"].as_u64().unwrap() as usize;
                        let limit = params["len"].as_u64().unwrap() as usize;
                        json!({"total_size":bytes.len(),"data":STANDARD.encode(&bytes[offset..(offset+limit).min(bytes.len())])})
                    }
                    "fs.txbegin" => json!({"tx":41}),
                    "fs.writebin" => {
                        staged.extend(STANDARD.decode(params["data"].as_str().unwrap()).unwrap());
                        Value::Null
                    }
                    "fs.txcommit" => {
                        keymap = std::mem::take(&mut staged);
                        Value::Null
                    }
                    method => panic!("unexpected method {method}"),
                };
                let response = json!({"id":request["id"],"result":result});
                let mut wire = response.to_string();
                wire.push('\n');
                for frame in outbound.encode(wire.as_bytes()).unwrap() {
                    rec_inject.send(frame).unwrap();
                }
            }
        }
        keymap
    });

    rec_session.restore_six_layer_backup(&backup).await.unwrap();
    rec_session.shutdown().await;
    drop(rec_session);
    let final_keymap = rec_worker.await.unwrap();
    assert_eq!(final_keymap, SIX_SOURCE);
}
