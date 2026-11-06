use mi_model::{draft::Action, storage::DocumentStore};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
const RAW: &str = r#"{ "version":1, "future":1.2300e+04, "label":"中文", "activeProfileId":9,
"profiles":[{"id":"p","name":"P","layers":[{"id":7,"name":"L","layout":{"keymap":[["KC_A","KC_A"],[null,null,null,null],[null,null,null,null],[null,null,null]],"encoders":[null,null,null]}}]}]}"#;
fn setup() -> (tempfile::TempDir, DocumentStore, Value, String, String) {
    setup_with_smart(None)
}
fn setup_with_smart(
    smart: Option<&[u8]>,
) -> (tempfile::TempDir, DocumentStore, Value, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("workbench.json");
    let config = mi_model::parse_device_files(RAW.as_bytes(), smart).unwrap();
    let baseline = json!({"device_identity":{"confidence":"observed-interface","interface":"synthetic","session_id":4},"firmware":"0.6.3","keymap":{"state":"present","size":RAW.len(),"sha1":format!("{:x}",sha1::Sha1::digest(RAW.as_bytes())),"bytes":RAW.as_bytes()},"smart_actions":smart.map(|b| json!({"state":"present","size":b.len(),"sha1":format!("{:x}",sha1::Sha1::digest(b)),"bytes":b})).unwrap_or(json!({"state":"missing"})),"active_indices":{"profile":9,"layer":null}});
    let doc = mi_model::document::LocalDocumentV2::from_config(
        &config,
        Some(baseline.clone()),
        "device-snapshot",
    )
    .unwrap();
    std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
    let mut store = DocumentStore::default();
    store.open(path).unwrap();
    let p = store.draft.document.editable.graph.profile_order[0].clone();
    let l = store.draft.document.editable.graph.profiles[&p].layer_order[0].clone();
    let action = Action {
        id: "device-key".into(),
        name: "设备 B".into(),
        category: "设备按键".into(),
        detail: "本地意图".into(),
        payload: json!({"type":"device_key","token":"KC_B"}),
    };
    store
        .update(|d| d.assign(&json!(p), &json!(l), 0, action))
        .unwrap();
    (dir, store, baseline, p, l)
}
#[test]
fn preview_splices_one_source_token_and_verified_private_backup_preserves_exact_bytes() {
    let (dir, mut store, baseline, _, _) = setup();
    let before = serde_json::to_value(&store.draft.document).unwrap();
    let preview = store.preview_apply(&baseline).unwrap();
    assert_eq!(preview.before, "KC_A");
    assert_eq!(preview.after, "KC_B");
    assert_eq!(preview.logical_control, "logical-0");
    assert_eq!(preview.profile_source_id, json!("p"));
    assert!(!preview.apply_available);
    assert!(!preview.restore_available);
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        1,
        "preview creates no files"
    );
    let prepared = store
        .prepare_apply_backup(&preview.token, &baseline)
        .unwrap();
    let package = std::path::Path::new(&prepared.backup_path);
    assert_eq!(
        std::fs::read(package.join("keymap.json")).unwrap(),
        RAW.as_bytes()
    );
    assert!(!package.join("smart_actions.json").exists());
    assert_eq!(
        std::fs::read(package.join("planned-keymap.json")).unwrap(),
        RAW.replacen("\"KC_A\"", "\"KC_B\"", 1).as_bytes()
    );
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(package.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["baseline"]["smart_actions"]["state"], "missing");
    assert_eq!(
        manifest["plan"]["planned_keymap_sha256"],
        format!(
            "{:x}",
            Sha256::digest(RAW.replacen("\"KC_A\"", "\"KC_B\"", 1).as_bytes())
        )
    );
    assert_eq!(serde_json::to_value(&store.draft.document).unwrap(), before);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(package).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(package.join("manifest.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(
        store
            .prepare_apply_backup(&preview.token, &baseline)
            .is_err(),
        "consumed preview cannot repeat"
    );
}
#[test]
fn stale_document_session_identity_firmware_and_exact_baseline_reject_without_mutation() {
    let (_dir, mut store, baseline, p, _) = setup();
    let preview = store.preview_apply(&baseline).unwrap();
    for path in ["session_id", "interface"] {
        let mut changed = baseline.clone();
        changed["device_identity"][path] = json!("changed");
        assert!(store
            .prepare_apply_backup(&preview.token, &changed)
            .is_err());
    }
    let mut changed = baseline.clone();
    changed["firmware"] = json!("0.7.0");
    assert!(store.preview_apply(&changed).is_err());
    let mut changed = baseline.clone();
    changed["keymap"]["bytes"][3] = json!(33);
    assert!(store.preview_apply(&changed).is_err());
    store
        .update(|d| d.rename_profile(&p, "new revision"))
        .unwrap();
    assert!(store
        .prepare_apply_backup(&preview.token, &baseline)
        .is_err());
    assert!(
        store.preview_apply(&baseline).is_err(),
        "structure change unsupported"
    );
    assert_eq!(store.draft.config.profiles[0].name, "new revision");
}
#[test]
fn backup_failure_cancel_readonly_and_external_bytes_cannot_claim_prepared() {
    let (dir, mut store, baseline, _, _) = setup();
    let preview = store.preview_apply(&baseline).unwrap();
    let before = serde_json::to_value(&store.draft.document).unwrap();
    let bad = dir.path().join("device-backups");
    std::fs::write(&bad, b"cannot be a directory").unwrap();
    assert!(store
        .prepare_apply_backup(&preview.token, &baseline)
        .is_err());
    assert_eq!(serde_json::to_value(&store.draft.document).unwrap(), before);
    std::fs::remove_file(&bad).unwrap();
    store.cancel_apply_preview();
    assert!(store
        .prepare_apply_backup(&preview.token, &baseline)
        .is_err());
    store.draft.read_only = true;
    assert!(store.preview_apply(&baseline).is_err());
    store.draft.read_only = false;
    let path = dir.path().join("workbench.json");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.push(b' ');
    std::fs::write(&path, &bytes).unwrap();
    assert!(store.preview_apply(&baseline).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}
#[test]
fn ambiguous_sources_and_host_shortcuts_reject_while_unchanged_keys_are_ignored() {
    let (_dir, mut store, baseline, p, l) = setup();
    let aid = store
        .draft
        .document
        .editable
        .graph
        .actions
        .keys()
        .next()
        .unwrap()
        .clone();
    let mut host = store.draft.document.editable.graph.actions[&aid].clone();
    host.payload = json!({"type":"shortcut","key":"b","modifiers":[],"target":"device"});
    store
        .update(|d| d.assign(&json!(p), &json!(l), 0, host))
        .unwrap();
    assert!(
        store.preview_apply(&baseline).is_err(),
        "never reinterpret Shortcut target"
    );
    let (_dir, mut store, baseline, p, l) = setup();
    let action = Action {
        id: "device-key".into(),
        name: "设备 A".into(),
        category: "设备按键".into(),
        detail: "意图".into(),
        payload: json!({"type":"device_key","token":"KC_A"}),
    };
    store
        .update(|d| d.assign(&json!(p), &json!(l), 1, action))
        .unwrap();
    assert_eq!(store.preview_apply(&baseline).unwrap().changes.len(), 1);
    let (dir, mut store, baseline, _, l) = setup();
    let bids = store.draft.document.editable.graph.layers[&l]
        .control_order
        .clone();
    let duplicate = store.draft.document.editable.source_refs[&bids[0]].clone();
    store
        .draft
        .document
        .editable
        .source_refs
        .insert(bids[1].clone(), duplicate);
    let path = dir.path().join("workbench.json");
    std::fs::write(&path, serde_json::to_vec(&store.draft.document).unwrap()).unwrap();
    store.open(path).unwrap();
    assert!(store.preview_apply(&baseline).is_err());
}
#[test]
fn future_document_and_duplicate_json_are_readonly_or_rejected() {
    let (dir, mut store, baseline, _, _) = setup();
    let path = dir.path().join("workbench.json");
    let mut doc: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    doc["future_field"] = json!({"opaque":true});
    std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
    store.open(path).unwrap();
    assert!(store.draft.read_only);
    assert!(store.preview_apply(&baseline).is_err());
    assert!(mi_model::parse_device_files(
        RAW.replacen("\"version\":1", "\"version\":1,\"version\":1", 1)
            .as_bytes(),
        None
    )
    .is_err());
}
#[test]
fn backup_keeps_present_smart_file_opaque_and_does_not_overwrite_old_package() {
    const SMART: &[u8] =
        "{ \"smartActions\": {}, \"unknown\":1.000e+03, \"文字\":\"保留\" }\n".as_bytes();
    let (_dir, mut store, baseline, _, _) = setup_with_smart(Some(SMART));
    let preview = store.preview_apply(&baseline).unwrap();
    let prepared = store
        .prepare_apply_backup(&preview.token, &baseline)
        .unwrap();
    let path = std::path::Path::new(&prepared.backup_path).join("smart_actions.json");
    assert_eq!(std::fs::read(&path).unwrap(), SMART);
    let manifest: Value = serde_json::from_slice(
        &std::fs::read(std::path::Path::new(&prepared.backup_path).join("manifest.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        manifest["smart_actions_sha256"],
        format!("{:x}", Sha256::digest(SMART))
    );
    let preview = store.preview_apply(&baseline).unwrap();
    let second = store
        .prepare_apply_backup(&preview.token, &baseline)
        .unwrap();
    assert_ne!(second.backup_path, prepared.backup_path);
    assert_eq!(std::fs::read(path).unwrap(), SMART);
}

#[test]
fn device_workspace_keeps_host_document_routing_and_undo_separate() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("workbench.json");
    let mut store = DocumentStore::default();
    store.open(path.clone()).unwrap();
    store
        .update(|d| mi_model::preset::install_agent_starter_profiles(d).map(|_| ()))
        .unwrap();
    let host_id = store.draft.document.editable.graph.profile_order[0].clone();
    store
        .set_host_active_profile(Some(host_id.clone()))
        .unwrap();
    store.set_host_execution_enabled(true).unwrap();
    let original = std::fs::read(&path).unwrap();
    let preferences = std::fs::read(path.with_extension("preferences.json")).unwrap();
    let host_doc = store.draft.document_id.clone();
    let host_undo = store.draft.can_undo;
    let (_, _, baseline, _, _) = setup();
    let config = mi_model::parse_device_files(RAW.as_bytes(), None).unwrap();
    store
        .begin_device_editing(config.clone(), baseline.clone())
        .unwrap();
    assert!(store.editing_device());
    assert_eq!(store.host_store().draft.document_id, host_doc);
    assert_eq!(store.host_active_profile_id(), Some(host_id));
    assert!(store.host_execution_enabled());
    assert_eq!(store.routing_state()["document_id"], host_doc);
    let device_doc = store.draft.document_id.clone();
    let pid = store.draft.document.editable.graph.profile_order[0].clone();
    store
        .update(|d| d.rename_profile(&pid, "Device work in progress"))
        .unwrap();
    store.return_to_host();
    assert_eq!(store.draft.document_id, host_doc);
    assert_eq!(store.draft.can_undo, host_undo);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(
        std::fs::read(path.with_extension("preferences.json")).unwrap(),
        preferences
    );
    assert!(!path.with_extension("active.json").exists());
    let mut reopened = DocumentStore::default();
    reopened.open(path).unwrap();
    assert_eq!(reopened.draft.document_id, host_doc);
    reopened.resume_device_editing().unwrap();
    assert_eq!(reopened.draft.document_id, device_doc);
    assert_eq!(
        reopened.draft.document.editable.graph.profiles[&pid].name,
        "Device work in progress"
    );
}

#[test]
fn firmware_v063_304_tokens_2d_encoders_and_joystick_sector_splicing_work_end_to_end() {
    assert_eq!(mi_model::draft::FIRMWARE_V063_TOKENS.len(), 304);
    assert!(mi_model::draft::FIRMWARE_V063_TOKENS
        .windows(2)
        .all(|w| w[0] < w[1]));
    assert!(mi_model::draft::DEVICE_KEY_TOKENS
        .windows(2)
        .all(|w| w[0] < w[1]));
    assert!(mi_model::draft::is_valid_firmware_v063_token("KC_ENT"));
    assert!(mi_model::draft::is_valid_firmware_v063_token("KC_SPC"));
    assert_eq!(
        mi_model::draft::canonical_firmware_token("KC_ENTER"),
        "KC_ENT"
    );
    assert_eq!(
        mi_model::draft::canonical_firmware_token("KC_SPACE"),
        "KC_SPC"
    );

    let raw_2d = r#"{"version":1,"activeProfileId":0,"profiles":[{"id":"p","name":"P","layers":[{"id":7,"name":"L","color":65446,"lights":null,"layout":{"keymap":[["KC_A","KC_B"],[null,null,null,null],[null,null,null,null],[null,null,"KC_SPC"]],"encoders":[["KC_VOLU","KC_VOLD","KC_MPLY"]],"joystick":{"type":"KEY","sectors":[{"k":"KC_UP"},{"k":"KC_DOWN"}]}}}]}]}"#;
    let config = mi_model::parse_device_files(raw_2d.as_bytes(), None).unwrap();
    assert!(
        !config
            .warnings
            .iter()
            .any(|w| w.contains("旋钮") || w.contains("摇杆")),
        "{:?}",
        config.warnings
    );
    assert_eq!(
        config.profiles[0].layers[0].encoders[0],
        mi_model::Binding::Basic(mi_model::Token("KC_VOLU".into()))
    );
    assert_eq!(
        config.profiles[0].layers[0].joystick.sectors[0],
        mi_model::Binding::Basic(mi_model::Token("KC_UP".into()))
    );

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("workbench.json");
    let baseline = json!({
        "device_identity":{"confidence":"observed-interface","interface":"synthetic","session_id":4},
        "firmware":"0.6.3",
        "keymap":{"state":"present","size":raw_2d.len(),"sha1":format!("{:x}",sha1::Sha1::digest(raw_2d.as_bytes())),"bytes":raw_2d.as_bytes()},
        "smart_actions":{"state":"missing"},
        "active_indices":{"profile":0,"layer":0}
    });
    let doc = mi_model::document::LocalDocumentV2::from_config(
        &config,
        Some(baseline.clone()),
        "device-snapshot",
    )
    .unwrap();
    std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
    let mut store = DocumentStore::default();
    store.open(path).unwrap();
    let p = store.draft.document.editable.graph.profile_order[0].clone();
    let l = store.draft.document.editable.graph.profiles[&p].layer_order[0].clone();

    // Assign encoder click (slot 15) -> KC_MUTE and joystick sector 0 (slot 16) -> KC_ENT
    store
        .update(|d| {
            d.assign(
                &json!(p),
                &json!(l),
                15,
                Action {
                    id: "enc-mute".into(),
                    name: "旋钮静音".into(),
                    category: "设备按键".into(),
                    detail: "2D 旋钮按下".into(),
                    payload: json!({"type":"device_key","token":"KC_MUTE"}),
                },
            )?;
            d.assign(
                &json!(p),
                &json!(l),
                16,
                Action {
                    id: "joy-ent".into(),
                    name: "摇杆回车".into(),
                    category: "设备按键".into(),
                    detail: "摇杆扇区".into(),
                    payload: json!({"type":"device_key","token":"KC_ENT"}),
                },
            )
        })
        .unwrap();

    let preview = store.preview_apply(&baseline).unwrap();
    assert_eq!(preview.changes.len(), 2);
    store
        .prepare_apply_backup(&preview.token, &baseline)
        .unwrap();
    let candidate = store
        .take_prepared_candidate(&preview.token, &baseline)
        .unwrap();
    let re_parsed = mi_model::parse_device_files(candidate.candidate(), None).unwrap();
    assert_eq!(
        re_parsed.profiles[0].layers[0].encoders[2],
        mi_model::Binding::Basic(mi_model::Token("KC_MUTE".into()))
    );
    assert_eq!(
        re_parsed.profiles[0].layers[0].joystick.sectors[0],
        mi_model::Binding::Basic(mi_model::Token("KC_ENT".into()))
    );

    let all_ag = store.draft.compile_all_ag_host_keymap(Some(&p)).unwrap();
    let all_ag_parsed = mi_model::parse_device_files(&all_ag, None).unwrap();
    assert_eq!(
        all_ag_parsed.profiles[0].layers[0].keys[12],
        mi_model::Binding::Unknown(json!("KV_OAI_AG12"))
    );
}
