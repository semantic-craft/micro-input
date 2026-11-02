use crate::{comparison::*, document::LocalDocumentV2};
use serde_json::json;
fn fixture() -> (LocalDocumentV2, crate::DeviceConfig, serde_json::Value) {
    let config = crate::demo().unwrap();
    let baseline = json!({"device_identity":{"confidence":"verified-serial","serial":"synthetic-one"},"keymap":{"sha1":"base"}});
    let doc =
        LocalDocumentV2::from_config(&config, Some(baseline.clone()), "device-snapshot").unwrap();
    (doc, config, baseline)
}
#[test]
fn local_and_remote_disjoint_names_merge_against_new_base() {
    let (mut local, mut remote, baseline) = fixture();
    let pid = local.editable.graph.profile_order[0].clone();
    let lid = local.editable.graph.profiles[&pid].layer_order[0].clone();
    local.rename_profile(&pid, "Local profile").unwrap();
    remote.profiles[0].layers[0].name = "Device layer".into();
    let comparison = Comparison::new(&local, &remote, baseline).unwrap();
    assert!(comparison.can_merge(), "{:?}", comparison.conflicts);
    let accepted = comparison.resolve(&local, Choice::Merge).unwrap().unwrap();
    assert_eq!(accepted.editable.graph.profiles[&pid].name, "Local profile");
    assert_eq!(accepted.editable.graph.layers[&lid].name, "Device layer");
    assert_eq!(accepted.revision, 1);
    assert_eq!(
        accepted.structure_changes(),
        vec!["配置改名：通用 Coding · 演示 → Local profile"]
    );
}

#[test]
fn unknown_raw_and_uncertain_identity_block_only_automatic_merge() {
    let (local, mut remote, mut baseline) = fixture();
    remote.raw["future_mode"] = json!({"mode":"new"});
    baseline["device_identity"] =
        json!({"confidence":"observed-interface","interface":"ephemeral"});
    let comparison = Comparison::new(&local, &remote, baseline).unwrap();
    assert!(!comparison.can_merge());
    assert!(comparison.conflicts.iter().any(|c| c.contains("未知 raw")));
    assert!(!comparison.identity_matches);
    let kept = comparison
        .resolve(&local, Choice::KeepLocal)
        .unwrap()
        .unwrap();
    assert_eq!(
        kept.baseline.unwrap()["comparison_config"]["raw"]["future_mode"]["mode"],
        "new"
    );
    assert_eq!(local.origin["config"]["raw"].get("future_mode"), None);
}

#[test]
fn comparison_table_and_whole_choices_preserve_exact_semantics() {
    for (left, right, conflict, expected) in [
        ("Local", "通用 Coding · 演示", false, "Local"),
        ("通用 Coding · 演示", "Device", false, "Device"),
        ("Common", "Common", false, "Common"),
        ("Local", "Device", true, "Local"),
    ] {
        let (mut local, mut remote, baseline) = fixture();
        let pid = local.editable.graph.profile_order[0].clone();
        local.rename_profile(&pid, left).unwrap();
        remote.profiles[0].name = right.into();
        let before = json!(local);
        let comparison = Comparison::new(&local, &remote, baseline).unwrap();
        assert_eq!(!comparison.can_merge(), conflict);
        assert!(comparison
            .resolve(&local, Choice::Cancel)
            .unwrap()
            .is_none());
        assert_eq!(json!(local), before);
        let kept = comparison
            .resolve(&local, Choice::KeepLocal)
            .unwrap()
            .unwrap();
        assert_eq!(kept.editable.graph.profiles[&pid].name, left);
        let device = comparison
            .resolve(&local, Choice::UseDevice)
            .unwrap()
            .unwrap();
        assert_eq!(device.editable.graph.profiles[&pid].name, right);
        assert!(device.structure_changes().is_empty());
        if !conflict {
            let merged = comparison.resolve(&local, Choice::Merge).unwrap().unwrap();
            assert_eq!(merged.editable.graph.profiles[&pid].name, expected);
        } else {
            assert!(comparison.resolve(&local, Choice::Merge).is_err());
        }
    }
    let (mut local, mut remote, baseline) = fixture();
    let pid = local.editable.graph.profile_order[0].clone();
    local.delete_profile(&pid).unwrap();
    let compared = Comparison::new(&local, &remote, baseline.clone()).unwrap();
    assert!(compared.can_merge());
    assert_eq!(
        compared
            .resolve(&local, Choice::Merge)
            .unwrap()
            .unwrap()
            .editable
            .graph
            .profile_order
            .len(),
        1
    );
    remote.profiles[0].name = "Modified deleted".into();
    assert!(!Comparison::new(&local, &remote, baseline)
        .unwrap()
        .can_merge());
}
#[test]
fn different_device_ambiguous_sources_and_stale_local_cannot_be_accepted() {
    let (mut local, remote, mut baseline) = fixture();
    baseline["device_identity"]["serial"] = json!("other-device");
    let compared = Comparison::new(&local, &remote, baseline).unwrap();
    assert!(compared.resolve(&local, Choice::UseDevice).is_err());
    let (_, remote, baseline) = fixture();
    let compared = Comparison::new(&local, &remote, baseline.clone()).unwrap();
    let pid = local.editable.graph.profile_order[0].clone();
    local.rename_profile(&pid, "Later edit").unwrap();
    assert!(compared.resolve(&local, Choice::KeepLocal).is_err());
    let mut ambiguous = remote;
    ambiguous.profiles[1].id = ambiguous.profiles[0].id.clone();
    assert!(Comparison::new(&local, &ambiguous, baseline).is_err());
}

#[test]
fn accepting_comparison_saves_recovery_copy_and_reopens_new_base_without_history() {
    let dir = std::env::temp_dir().join(format!("mi-compare-{}", crate::document::new_id()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("workbench.json");
    let (local, mut remote, baseline) = fixture();
    let original = serde_json::to_vec_pretty(&local).unwrap();
    std::fs::write(&path, &original).unwrap();
    let mut store = crate::storage::DocumentStore::default();
    store.open(path.clone()).unwrap();
    let pid = store.draft.document.editable.graph.profile_order[0].clone();
    store
        .update(|d| d.rename_profile(&pid, "Saved local"))
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    let rev = store.draft.revision;
    remote.profiles[0].name = "Device version".into();
    let comparison = store.compare(&remote, baseline.clone()).unwrap();
    store
        .resolve_comparison(&comparison.token, Choice::Cancel, &baseline)
        .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(store.draft.revision, rev);
    assert!(store.draft.can_undo);
    assert!(store
        .resolve_comparison(&comparison.token, Choice::UseDevice, &baseline)
        .is_err());
    let comparison = store.compare(&remote, baseline.clone()).unwrap();
    store
        .resolve_comparison(&comparison.token, Choice::UseDevice, &baseline)
        .unwrap();
    assert_eq!(store.draft.config.profiles[0].name, "Device version");
    assert_eq!(store.draft.revision, rev + 1);
    assert!(!store.draft.can_undo);
    assert!(store.draft.structure_changes.is_empty());
    assert!(store.draft.edits.is_empty());
    let backups: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "backup"))
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(std::fs::read(&backups[0]).unwrap(), before);
    let mut reopen = crate::storage::DocumentStore::default();
    reopen.open(path).unwrap();
    assert_eq!(reopen.draft.config.profiles[0].name, "Device version");
    assert_eq!(reopen.draft.document.origin, local.origin);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn snapshot_change_backup_failure_and_atomic_save_failure_leave_document_unchanged() {
    for failure in ["snapshot", "backup", "pending"] {
        let dir =
            std::env::temp_dir().join(format!("mi-compare-fail-{}", crate::document::new_id()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("workbench.json");
        let (local, mut remote, baseline) = fixture();
        let bytes = serde_json::to_vec_pretty(&local).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let mut store = crate::storage::DocumentStore::default();
        store.open(path.clone()).unwrap();
        remote.profiles[0].name = "Device name".into();
        let c = store.compare(&remote, baseline.clone()).unwrap();
        let mut current = baseline;
        match failure {
            "snapshot" => current["keymap"]["sha1"] = json!("changed"),
            "backup" => {
                use sha2::{Digest, Sha256};
                let hash = format!("{:x}", Sha256::digest(&bytes));
                std::fs::write(
                    dir.join(format!("workbench.json.reconnect.{hash}.backup")),
                    b"wrong existing backup",
                )
                .unwrap();
            }
            _ => std::fs::write(path.with_extension("pending"), b"interrupted").unwrap(),
        }
        assert!(store
            .resolve_comparison(&c.token, Choice::UseDevice, &current)
            .is_err());
        assert_eq!(json!(store.draft.document), json!(local));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
#[test]
fn freshly_changed_identity_or_file_digest_rejects_future_application() {
    let (mut local, _, mut current) = fixture();
    current["keymap"] = json!({"state":"present","sha1":"base","size":128});
    current["smart_actions"] = json!({"state":"missing"});
    local.baseline = Some(current.clone());
    assert!(ensure_apply_baseline(&local, &current).is_ok());
    current["keymap"]["sha1"] = json!("changed");
    assert!(ensure_apply_baseline(&local, &current).is_err());
    current["keymap"]["sha1"] = json!("base");
    current["device_identity"]["serial"] = json!("other");
    assert!(ensure_apply_baseline(&local, &current).is_err());
}

#[test]
fn keeping_local_binding_recomputes_diff_against_remote_and_preserves_remote_opaque_binding() {
    let (local, mut remote, baseline) = fixture();
    remote.profiles[0].layers[0].keys[0] = crate::Binding::Basic(crate::Token("KC_B".into()));
    let compared = Comparison::new(&local, &remote, baseline.clone()).unwrap();
    assert!(compared.can_merge());
    let kept = compared
        .resolve(&local, Choice::KeepLocal)
        .unwrap()
        .unwrap();
    assert!(kept
        .structure_changes()
        .iter()
        .any(|c| c.contains("绑定变化")));
    remote.profiles[0].layers[0].keys[0] = crate::Binding::Unknown(json!({"future":"remote"}));
    let compared = Comparison::new(&local, &remote, baseline).unwrap();
    assert!(!compared.can_merge());
    let kept = compared
        .resolve(&local, Choice::KeepLocal)
        .unwrap()
        .unwrap();
    let (view, _) = kept.view().unwrap();
    assert_eq!(
        view.profiles[0].layers[0].keys[0],
        crate::Binding::Unknown(json!({"future":"remote"}))
    );
}

#[test]
fn replacement_source_id_is_manual_and_malformed_baseline_never_panics() {
    let (local, mut remote, baseline) = fixture();
    remote.profiles[0].id = json!(900);
    let compared = Comparison::new(&local, &remote, baseline).unwrap();
    assert!(!compared.can_merge());
    assert!(compared.conflicts.iter().any(|c| c.contains("源 ID")));
    assert!(Comparison::new(&local, &remote, json!("invalid baseline")).is_err());
}

#[test]
fn device_deleted_unchanged_profile_is_a_deletion_not_opaque_conflict() {
    let (local, mut remote, baseline) = fixture();
    remote.profiles.remove(0);
    remote.profiles[0].index = 0;
    remote.raw["profiles"].as_array_mut().unwrap().remove(0);
    let compared = Comparison::new(&local, &remote, baseline).unwrap();
    assert!(compared.can_merge(), "{:?}", compared.conflicts);
    let next = compared.resolve(&local, Choice::Merge).unwrap().unwrap();
    assert_eq!(next.editable.graph.profile_order.len(), 1);
    assert!(next.structure_changes().is_empty());
}

#[test]
fn source_id_ambiguity_is_not_hidden_by_extra_reference_metadata() {
    let (mut local, remote, baseline) = fixture();
    let first = local.editable.graph.profile_order[0].clone();
    let second = local.editable.graph.profile_order[1].clone();
    let mut duplicate = local.editable.source_refs[&first].clone();
    duplicate["future_note"] = json!("retained");
    local.editable.source_refs.insert(second, duplicate);
    assert!(Comparison::new(&local, &remote, baseline).is_err());
}

#[test]
fn changed_transient_interface_requires_snapshot_bound_manual_identity_confirmation() {
    let (mut local, mut remote, mut baseline) = fixture();
    local.baseline.as_mut().unwrap()["device_identity"] =
        json!({"confidence":"observed-interface","interface":"transient-old","session_id":1});
    baseline["device_identity"] =
        json!({"confidence":"observed-interface","interface":"transient-new","session_id":2});
    remote.profiles[0].name = "After physical reconnect".into();
    let mut compared = Comparison::new(&local, &remote, baseline.clone()).unwrap();
    assert!(compared.identity_confirmation_required);
    assert!(compared.resolve(&local, Choice::UseDevice).is_err());
    let mut newer = baseline.clone();
    newer["device_identity"]["session_id"] = json!(3);
    assert!(compared.confirm_identity(&local, &newer).is_err());
    compared.confirm_identity(&local, &baseline).unwrap();
    assert!(!compared.can_merge());
    assert!(compared.identity_user_confirmed);
    let kept = compared
        .resolve(&local, Choice::KeepLocal)
        .unwrap()
        .unwrap();
    assert_eq!(
        kept.baseline.as_ref().unwrap()["device_identity"]["confidence"],
        "observed-interface"
    );
    assert_eq!(
        kept.baseline.as_ref().unwrap()["identity_resolution"]["comparison_token"],
        compared.token
    );
    assert_eq!(
        compared
            .resolve(&local, Choice::UseDevice)
            .unwrap()
            .unwrap()
            .view()
            .unwrap()
            .0
            .profiles[0]
            .name,
        "After physical reconnect"
    );
    let (local, remote, mut baseline) = fixture();
    baseline["device_identity"]["serial"] = json!("known-other");
    let mut wrong = Comparison::new(&local, &remote, baseline.clone()).unwrap();
    assert!(wrong.confirm_identity(&local, &baseline).is_err());
}

#[test]
fn comparison_binding_summary_names_the_action_while_raw_reference_remains_available() {
    let (local, remote, baseline) = fixture();
    let mut draft = crate::draft::Draft::from_document(local, 2, false).unwrap();
    let p = draft.config.profiles[0].id.clone();
    let l = draft.config.profiles[0].layers[0].id.clone();
    draft
        .assign(&p, &l, 0, crate::draft::actions()[1].clone())
        .unwrap();
    let compared = Comparison::new(&draft.document, &remote, baseline).unwrap();
    let row = compared
        .rows
        .iter()
        .find(|r| {
            r.local
                .as_ref()
                .is_some_and(|v| v.get("logical_control").is_some())
        })
        .unwrap();
    assert!(row.local_summary.contains("保存文件"));
    assert!(!row
        .local_summary
        .contains(row.local.as_ref().unwrap()["action_id"].as_str().unwrap()));
}
