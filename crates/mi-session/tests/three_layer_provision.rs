use base64::{engine::general_purpose::STANDARD, Engine};
use mi_model::apply::{
    compile_three_layer_profile, recognize_three_layer_profile, ThreeLayerProvision,
};
use mi_proto::{Framer, NativeFramer};
use mi_session::{DeviceSession, ReadOnlySchema, SessionConfig};
use mi_transport::{HidLink, LinkBackend, MockLink, TransportError};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::{sync::Arc, time::Duration};

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

fn spawn_mock_device(
    initial: Vec<u8>,
    commit_writes: bool,
    post_commit_layer: u64,
) -> (
    Arc<DeviceSession>,
    tokio::task::JoinHandle<(Vec<u8>, usize)>,
) {
    spawn_mock_device_with_version(initial, commit_writes, post_commit_layer, "0.6.3")
}

fn spawn_mock_device_with_version(
    initial: Vec<u8>,
    commit_writes: bool,
    post_commit_layer: u64,
    version: &str,
) -> (
    Arc<DeviceSession>,
    tokio::task::JoinHandle<(Vec<u8>, usize)>,
) {
    let version = version.to_string();
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
    let worker = tokio::spawn(async move {
        let mut incoming = NativeFramer::default();
        let outbound = NativeFramer::default();
        let mut active = serde_json::from_slice::<Value>(&initial)
            .ok()
            .and_then(|v| v["activeProfileId"].as_u64())
            .unwrap_or(0);
        let mut keymap = initial;
        let mut staged = Vec::new();
        let mut commits = 0usize;
        while let Some(frame) = requests.recv().await {
            for bytes in incoming.feed(&frame).unwrap() {
                let request: Value = serde_json::from_slice(&bytes).unwrap();
                let params = &request["params"];
                let result = match request["method"].as_str().unwrap() {
                    "device.status" => {
                        let layer = if commits > 0 { post_commit_layer } else { 1 };
                        json!({"version": version, "profile_index":active,"layer_index":layer})
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
                        staged.clear();
                        json!({"tx":51})
                    }
                    "fs.writebin" => {
                        assert_eq!(params["file"], "keymap.json");
                        assert_eq!(params["tx"], 51);
                        assert_eq!(params["offset"], staged.len());
                        staged.extend(STANDARD.decode(params["data"].as_str().unwrap()).unwrap());
                        Value::Null
                    }
                    "fs.txcommit" => {
                        assert_eq!(params["tx"], 51);
                        commits += 1;
                        if commit_writes {
                            keymap = std::mem::take(&mut staged);
                            let applied: Value = serde_json::from_slice(&keymap).unwrap();
                            active = applied["activeProfileId"].as_u64().unwrap();
                        } else {
                            staged.clear();
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
        (keymap, commits)
    });
    (session, worker)
}

#[tokio::test]
async fn six_layer_unconfirmed_commit_or_layer_restores_original_bytes() {
    const SIX_SOURCE: &[u8] =
        include_bytes!("../../mi-model/fixtures/device/synthetic-six-layer-source.json");

    let dir_a = tempfile::tempdir().unwrap();
    let (session_a, worker_a) = spawn_mock_device(SIX_SOURCE.to_vec(), false, 1);
    let prepared_a = session_a
        .prepare_six_layer_write(dir_a.path())
        .await
        .unwrap();
    let failure_a = session_a
        .apply_six_layer_write(prepared_a)
        .await
        .unwrap_err();
    assert!(
        failure_a.detail.contains("板载配置读回不一致"),
        "{}",
        failure_a.detail
    );
    assert!(failure_a.original_verified);
    session_a.shutdown().await;
    drop(session_a);
    let (final_a, commits_a) = worker_a.await.unwrap();
    assert_eq!(commits_a, 1);
    assert_eq!(final_a, SIX_SOURCE);

    let dir_b = tempfile::tempdir().unwrap();
    let (session_b, worker_b) = spawn_mock_device(SIX_SOURCE.to_vec(), true, 0);
    let prepared_b = session_b
        .prepare_six_layer_write(dir_b.path())
        .await
        .unwrap();
    let failure_b = session_b
        .apply_six_layer_write(prepared_b)
        .await
        .unwrap_err();
    assert!(
        failure_b
            .detail
            .contains("六层文件已读回，但活动方案与层未确认"),
        "{}",
        failure_b.detail
    );
    assert!(failure_b.original_verified);
    session_b.shutdown().await;
    drop(session_b);
    let (final_b, commits_b) = worker_b.await.unwrap();
    assert_eq!(commits_b, 2);
    assert_eq!(final_b, SIX_SOURCE);
}

#[tokio::test]
async fn three_layer_backup_restore_roundtrip_idempotence_and_mismatch_rejection() {
    let ThreeLayerProvision::Write {
        keymap: candidate, ..
    } = compile_three_layer_profile(KEYMAP, Some(SMART)).unwrap()
    else {
        panic!("expected new 3-layer write")
    };
    let backup = tempfile::tempdir().unwrap();
    std::fs::write(backup.path().join("keymap.json"), KEYMAP).unwrap();
    std::fs::write(backup.path().join("planned-keymap.json"), &candidate).unwrap();
    std::fs::write(backup.path().join("smart_actions.json"), SMART).unwrap();

    let (restore_session, restore_worker) = spawn_mock_device(candidate.clone(), true, 1);
    restore_session
        .restore_three_layer_backup(backup.path())
        .await
        .unwrap();
    restore_session.shutdown().await;
    drop(restore_session);
    let (restored_keymap, restore_commits) = restore_worker.await.unwrap();
    assert_eq!(restore_commits, 1);
    assert_eq!(restored_keymap, KEYMAP);

    let (idempotent_session, idempotent_worker) = spawn_mock_device(KEYMAP.to_vec(), true, 1);
    idempotent_session
        .restore_three_layer_backup(backup.path())
        .await
        .unwrap();
    idempotent_session.shutdown().await;
    drop(idempotent_session);
    let (idempotent_keymap, idempotent_commits) = idempotent_worker.await.unwrap();
    assert_eq!(idempotent_commits, 0);
    assert_eq!(idempotent_keymap, KEYMAP);

    let mut tampered = candidate;
    tampered.push(b' ');
    let (mismatch_session, mismatch_worker) = spawn_mock_device(tampered.clone(), true, 1);
    let err = mismatch_session
        .restore_three_layer_backup(backup.path())
        .await
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("设备与本次三层候选不符，拒绝覆盖恢复"),
        "{err}"
    );
    mismatch_session.shutdown().await;
    drop(mismatch_session);
    let (mismatch_keymap, mismatch_commits) = mismatch_worker.await.unwrap();
    assert_eq!(mismatch_commits, 0);
    assert_eq!(mismatch_keymap, tampered);
}

#[tokio::test]
async fn prepare_six_layer_write_with_layer_modes_compiles_mixed_radial_and_mouse_on_v070() {
    const SIX_SOURCE: &[u8] =
        include_bytes!("../../mi-model/fixtures/device/synthetic-six-layer-source.json");
    let dir = tempfile::tempdir().unwrap();
    let (session, worker) =
        spawn_mock_device_with_version(SIX_SOURCE.to_vec(), true, 1, "0.7.0-rc.16");
    session.read_identity_info().await.unwrap();
    let layer_modes = [
        Some("MOUSE"),
        None,
        Some("MOUSE"),
        Some("MOUSE"),
        Some("MOUSE"),
        Some("MOUSE"),
    ];
    let prepared = session
        .prepare_six_layer_write_with_layer_modes(dir.path(), Some(layer_modes))
        .await
        .unwrap();
    let parsed: Value = serde_json::from_slice(&prepared.candidate).unwrap();
    let layers = parsed["profiles"][prepared.profile_index as usize]["layers"]
        .as_array()
        .unwrap();
    assert_eq!(layers[0]["layout"]["joystick"], json!({"type": "MOUSE"}));
    assert_eq!(layers[1]["layout"]["joystick"]["type"], json!("RADIAL"));
    assert_eq!(layers[2]["layout"]["joystick"], json!({"type": "MOUSE"}));
    session.shutdown().await;
    drop(session);
    let _ = worker.await.unwrap();

    // 0.6.3 must fail closed rather than silently downgrading requested MOUSE layers.
    let dir_063 = tempfile::tempdir().unwrap();
    let (session_063, worker_063) =
        spawn_mock_device_with_version(SIX_SOURCE.to_vec(), true, 1, "0.6.3");
    session_063.read_identity_info().await.unwrap();
    let err = session_063
        .prepare_six_layer_write_with_layer_modes(dir_063.path(), Some(layer_modes))
        .await
        .err()
        .expect("0.6.3 must reject MOUSE layer mode")
        .to_string();
    assert!(err.contains("0.7.0-rc.16"), "{err}");
    session_063.shutdown().await;
    drop(session_063);
    let _ = worker_063.await.unwrap();
}
