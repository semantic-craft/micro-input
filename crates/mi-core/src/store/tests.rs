mod startup_tests {
    use super::super::*;

    #[test]

    fn automatic_host_startup_preserves_saved_v1_state_without_migrating() {
        for active_agent in [None, Some(CanonicalAgent::DeepseekHarness)] {
            let temp = tempfile::TempDir::new().unwrap();
            let seed = WorkbenchStore::open_isolated(temp.path()).unwrap();
            let active_id = active_agent.and_then(|agent| seed.find_canonical_profile_id(agent));
            fs::write(
                temp.path().join("workbench.json"),
                serde_json::to_vec(&seed.document_store.draft.document).unwrap(),
            )
            .unwrap();
            let original = serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version": 1, "favorites": [],
                "host_active_profile_id": active_id,
                "host_execution_enabled": false, "command_execution_enabled": false
            }))
            .unwrap();
            let path = temp.path().join("workbench.preferences.json");
            fs::write(&path, &original).unwrap();
            drop(seed);

            let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();
            let preserve_saved_host_state = store.document_store.has_saved_host_preferences();
            assert!(preserve_saved_host_state);
            let before = store.document_store.routing_state();
            store.initialize_auto_host_ready(preserve_saved_host_state);

            assert_eq!(store.document_store.routing_state(), before);
            assert_eq!(store.active_host_profile_id(), active_id);
            assert!(!store.document_store.host_execution_enabled());
            assert_eq!(fs::read(path).unwrap(), original);
            assert!(!temp
                .path()
                .join("workbench.preferences.v1.backup.json")
                .exists());
        }
    }

    #[test]
    fn automatic_host_startup_initializes_a_new_install() {
        let temp = tempfile::TempDir::new().unwrap();
        let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();
        store.initialize_auto_host_ready(false);
        assert_eq!(
            store.active_host_profile_id(),
            store.find_canonical_profile_id(CanonicalAgent::Antigravity)
        );
        assert!(store.document_store.host_execution_enabled());
    }
}

mod tests {
    use super::super::antigravity_daemon::{
        build_antigravity_trajectories_request, find_freshest_antigravity_hub_in,
        parse_antigravity_http_200_body, CsrfToken, ANTIGRAVITY_HUB_MAX_BYTES,
    };
    use super::super::*;

    #[test]
    fn status_menu_cheat_sheet_retains_host_profile_during_device_editing_and_clears_when_unselected(
    ) {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut cfg = WorkbenchStoreConfig::isolated(dir.path());
        cfg.isolated = false;
        let mut store = WorkbenchStore::open(cfg).expect("open store");
        let codex_id = store
            .find_canonical_profile_id(CanonicalAgent::Codex)
            .expect("Codex profile");
        store
            .select_and_activate_profile(&codex_id)
            .expect("activate Codex");

        let host_sheet = store
            .projection()
            .status_menu
            .cheat_sheet_preview
            .clone()
            .expect("host cheat sheet preview");
        let host_favorites = store.projection().status_menu.favorites_2x2.clone();
        assert_eq!(store.projection().status_menu.active_profile_name, "Codex");
        assert_eq!(host_sheet.profile_name, "Codex");
        assert!(host_sheet
            .key_cells
            .iter()
            .any(|c| c.available && c.short_label != "未绑定"));

        let mut raw_demo = mi_model::demo().expect("demo config").raw;
        raw_demo["version"] = json!(1);
        let keymap_bytes = serde_json::to_vec(&raw_demo).expect("keymap bytes");
        let keymap_file = mi_model::operation_record::file_value(&keymap_bytes);
        let sha1 = keymap_file["sha1"].as_str().unwrap_or_default().to_string();
        let baseline = json!({
            "firmware": "0.6.3",
            "device_identity": {
                "confidence": "verified-serial",
                "serial": "creator-micro-2-local",
                "session_id": 1
            },
            "keymap": keymap_file,
            "smart_actions": { "state": "missing" },
            "files": [{
                "name": "keymap",
                "state": "present",
                "size": keymap_bytes.len(),
                "sha1": sha1
            }]
        });
        let device_cfg =
            mi_model::parse_device_files(&keymap_bytes, None).expect("parse device keymap");
        store
            .document_store
            .begin_device_editing(device_cfg, baseline)
            .expect("enter device editing");
        assert!(store.document_store.editing_device());
        store.commit_runtime_update();

        let device_editing_menu = &store.projection().status_menu;
        assert_eq!(device_editing_menu.active_profile_name, "Codex");
        assert_eq!(device_editing_menu.favorites_2x2, host_favorites);
        assert_eq!(
            device_editing_menu.cheat_sheet_preview.as_ref(),
            Some(&host_sheet)
        );

        store.document_store.return_to_host();
        store
            .document_store
            .set_host_active_profile(None)
            .expect("clear active host profile");
        store.commit_runtime_update();

        assert_eq!(
            store.projection().status_menu.active_profile_name,
            "未选择方案"
        );
        assert!(store.projection().status_menu.cheat_sheet_preview.is_none());
    }

    #[test]
    fn antigravity_hub_discovery_rejects_crlf_csrf_zero_port_symlink_and_public_mode() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let dir = tempfile::tempdir().unwrap();
        let hub_path = dir.path().join("hub_bad.json");

        fs::write(
            &hub_path,
            r#"{"httpPort":9001,"httpsPort":9443,"csrfToken":"tok\r\nX-Injected: 1"}"#,
        )
        .unwrap();
        fs::set_permissions(&hub_path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(
            find_freshest_antigravity_hub_in(dir.path()).is_none(),
            "CRLF in csrfToken must be rejected"
        );

        fs::write(
            &hub_path,
            r#"{"httpPort":9001,"httpsPort":9443,"csrfToken":" valid-tok "}"#,
        )
        .unwrap();
        fs::set_permissions(&hub_path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(
            find_freshest_antigravity_hub_in(dir.path()).is_none(),
            "leading/trailing whitespace in csrfToken must be rejected"
        );

        fs::write(
            &hub_path,
            r#"{"httpPort":0,"httpsPort":9443,"csrfToken":"valid-token"}"#,
        )
        .unwrap();
        fs::set_permissions(&hub_path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(
            find_freshest_antigravity_hub_in(dir.path()).is_none(),
            "port == 0 must be rejected"
        );

        fs::write(
            &hub_path,
            r#"{"httpPort":9001,"httpsPort":9443,"csrfToken":"valid-token"}"#,
        )
        .unwrap();
        fs::set_permissions(&hub_path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            find_freshest_antigravity_hub_in(dir.path()).is_none(),
            "0o644 hub file must be rejected"
        );

        fs::remove_file(&hub_path).unwrap();
        let outside = dir.path().join("outside.json");
        fs::write(
            &outside,
            r#"{"httpPort":9001,"httpsPort":9443,"csrfToken":"valid-token"}"#,
        )
        .unwrap();
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o600)).unwrap();
        symlink(&outside, &hub_path).unwrap();
        assert!(
            find_freshest_antigravity_hub_in(dir.path()).is_none(),
            "symlinked hub file must be rejected"
        );
        fs::remove_file(&hub_path).unwrap();

        fs::write(&hub_path, "").unwrap();
        fs::set_permissions(&hub_path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(find_freshest_antigravity_hub_in(dir.path()).is_none());
        fs::remove_file(&hub_path).unwrap();

        {
            use std::ffi::CString;
            use std::os::unix::ffi::OsStrExt;
            extern "C" {
                #[cfg(target_os = "macos")]
                fn mkfifo(path: *const std::ffi::c_char, mode: u16) -> std::ffi::c_int;
                #[cfg(not(target_os = "macos"))]
                fn mkfifo(path: *const std::ffi::c_char, mode: u32) -> std::ffi::c_int;
            }
            let c_fifo = CString::new(hub_path.as_os_str().as_bytes()).unwrap();
            assert_eq!(unsafe { mkfifo(c_fifo.as_ptr(), 0o600) }, 0);
            assert!(find_freshest_antigravity_hub_in(dir.path()).is_none());
            fs::remove_file(&hub_path).unwrap();
        }

        let pad = "a".repeat(ANTIGRAVITY_HUB_MAX_BYTES + 64);
        fs::write(
            &hub_path,
            format!(
                r#"{{"httpPort":9001,"httpsPort":9443,"csrfToken":"valid-token","pad":"{pad}"}}"#
            ),
        )
        .unwrap();
        fs::set_permissions(&hub_path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(find_freshest_antigravity_hub_in(dir.path()).is_none());

        assert!(CsrfToken::parse("").is_none());
        assert!(CsrfToken::parse("tok\0en").is_none());
        assert!(CsrfToken::parse("tok\x07en").is_none());
        assert!(CsrfToken::parse("tok\ten").is_none());
        let valid_tok = CsrfToken::parse("valid-token-123").expect("valid csrf token");
        let req = build_antigravity_trajectories_request(9001, &valid_tok, 15);
        assert!(req.contains("x-codeium-csrf-token: valid-token-123\r\n"));

        fs::write(
            &hub_path,
            r#"{"httpPort":9001,"httpsPort":9443,"csrfToken":"valid-token-123"}"#,
        )
        .unwrap();
        fs::set_permissions(&hub_path, fs::Permissions::from_mode(0o600)).unwrap();
        let parsed = find_freshest_antigravity_hub_in(dir.path()).expect("valid 0o600 hub");
        assert_eq!(parsed.port, 9001);
        assert_eq!(parsed.ui_port, 9443);
        assert_eq!(parsed.csrf.as_str(), "valid-token-123");

        assert!(parse_antigravity_http_200_body(b"HTTP/1.1 2000 OK\r\n\r\n{}").is_none());
        assert!(parse_antigravity_http_200_body(b"HTTP/1.1 401 Unauthorized\r\n\r\n{}").is_none());
        assert_eq!(
            parse_antigravity_http_200_body(b"HTTP/1.0 200 OK\r\nContent-Length: 2\r\n\r\n{}"),
            Some(b"{}".as_slice())
        );
    }

    #[cfg(unix)]
    #[test]
    fn antigravity_hub_discovery_rejects_symlinked_and_world_or_group_writable_daemon_dir() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let parent = tempfile::tempdir().unwrap();
        let real_dir = parent.path().join("real_daemon");
        fs::create_dir_all(&real_dir).unwrap();
        fs::set_permissions(&real_dir, fs::Permissions::from_mode(0o700)).unwrap();
        let hub_path = real_dir.join("hub_live.json");
        fs::write(
            &hub_path,
            r#"{"httpPort":9001,"httpsPort":9443,"csrfToken":"valid-token-123"}"#,
        )
        .unwrap();
        fs::set_permissions(&hub_path, fs::Permissions::from_mode(0o600)).unwrap();

        let symlink_dir = parent.path().join("linked_daemon");
        symlink(&real_dir, &symlink_dir).unwrap();
        assert!(find_freshest_antigravity_hub_in(&symlink_dir).is_none());

        fs::set_permissions(&real_dir, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(find_freshest_antigravity_hub_in(&real_dir).is_none());

        fs::set_permissions(&real_dir, fs::Permissions::from_mode(0o775)).unwrap();
        assert!(find_freshest_antigravity_hub_in(&real_dir).is_none());

        fs::set_permissions(&real_dir, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(find_freshest_antigravity_hub_in(&real_dir).is_some());
    }

    #[cfg(unix)]
    #[test]
    fn antigravity_hub_discovery_rejects_symlinked_and_writable_ancestor_dir() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let root = tempfile::tempdir().unwrap();
        let ancestor = root.path().join("ancestor");
        let daemon_dir = ancestor.join("daemon");
        fs::create_dir_all(&daemon_dir).unwrap();
        fs::set_permissions(&daemon_dir, fs::Permissions::from_mode(0o700)).unwrap();

        let hub_path = daemon_dir.join("hub_live.json");
        fs::write(
            &hub_path,
            r#"{"httpPort":9001,"httpsPort":9443,"csrfToken":"valid-token-123"}"#,
        )
        .unwrap();
        fs::set_permissions(&hub_path, fs::Permissions::from_mode(0o600)).unwrap();

        let symlink_ancestor = root.path().join("linked_ancestor");
        symlink(&ancestor, &symlink_ancestor).unwrap();
        assert!(find_freshest_antigravity_hub_in(&symlink_ancestor.join("daemon")).is_none());

        fs::set_permissions(&ancestor, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(find_freshest_antigravity_hub_in(&daemon_dir).is_none());

        fs::set_permissions(&ancestor, fs::Permissions::from_mode(0o775)).unwrap();
        assert!(find_freshest_antigravity_hub_in(&daemon_dir).is_none());

        fs::set_permissions(&ancestor, fs::Permissions::from_mode(0o1777)).unwrap();
        assert!(find_freshest_antigravity_hub_in(&daemon_dir).is_some());

        fs::set_permissions(&ancestor, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(find_freshest_antigravity_hub_in(&daemon_dir).is_some());
    }

    #[test]
    fn workbench_store_open_tightens_document_dir_permissions_to_0700() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
        let doc_dir = dir.path().join("nested").join("micro-input-doc");
        let cfg = WorkbenchStoreConfig::isolated(&doc_dir);
        let _store = WorkbenchStore::open(cfg).expect("open store");
        let mode = fs::metadata(&doc_dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "document_dir must be tightened to 0o700");
        let parent_mode = fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            parent_mode, 0o755,
            "ancestor directory permissions must be untouched"
        );

        let loose_doc_dir = dir.path().join("existing-loose");
        fs::create_dir_all(&loose_doc_dir).unwrap();
        fs::set_permissions(&loose_doc_dir, fs::Permissions::from_mode(0o755)).unwrap();
        let _store2 =
            WorkbenchStore::open(WorkbenchStoreConfig::isolated(&loose_doc_dir)).expect("open");
        assert_eq!(
            fs::metadata(&loose_doc_dir).unwrap().permissions().mode() & 0o777,
            0o700
        );

        let link_doc_dir = dir.path().join("symlink-doc-dir");
        std::os::unix::fs::symlink(&loose_doc_dir, &link_doc_dir).unwrap();
        assert!(WorkbenchStore::open(WorkbenchStoreConfig::isolated(&link_doc_dir)).is_err());
    }

    #[test]
    fn workbench_store_already_closed_confirm_falls_through_to_slot_search_and_both_press_modes_search(
    ) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(dir.path()).expect("open store");
        store.set_host_execution_enabled(true).unwrap();
        let ag_id = store
            .find_canonical_profile_id(crate::CanonicalAgent::Antigravity)
            .unwrap();
        store.select_and_activate_profile(&ag_id).unwrap();

        let mk_ev = |seq: u64, binding: &str, kind: InputEventKind| InputEvent {
            session_id: 1,
            board_layer: None,
            sequence: seq,
            ingress_sequence: seq,
            radial_center: None,
            radial_position: None,
            timestamp_ms: seq,
            kind,
            logical_switch: None,
            binding_id: Some(binding.into()),
            physical_control: None,
            keycap_group: None,
            action_role: None,
            lost_count: 0,
            action_eligible: true,
        };

        let t0 = Instant::now();
        assert!(store
            .consume_physical_input_event(mk_ev(1, "ENC_CLK", InputEventKind::Pressed), t0)
            .is_none());
        let short_search = store
            .consume_physical_input_event(
                mk_ev(2, "ENC_CLK", InputEventKind::Released),
                t0 + Duration::from_millis(60),
            )
            .expect("idle short press opens search");
        assert_eq!(short_search.detail, "antigravity/search");
        assert!(!store.is_model_picker_mouse_capture_enabled());
        assert_eq!(
            store
                .encoder_model_picker
                .active_kind(t0 + Duration::from_millis(60)),
            Some(crate::encoder_model_picker::EncoderOverlayKind::SearchConversations)
        );
        store.cancel_encoder_model_picker();

        let before_hold_len = store.dispatched_receipts().len();
        assert!(store
            .consume_physical_input_event(
                mk_ev(3, "ENC_CLK", InputEventKind::Pressed),
                t0 + Duration::from_millis(100),
            )
            .is_none());
        let hold_gen = store
            .encoder_model_picker
            .hold_state()
            .unwrap()
            .hold_generation;
        let hold_search = store
            .fire_due_encoder_hold(1, hold_gen, t0 + Duration::from_millis(460))
            .expect("idle 350ms hold opens search");
        assert_eq!(hold_search.detail, "antigravity/search");
        assert!(store
            .consume_physical_input_event(
                mk_ev(4, "ENC_CLK", InputEventKind::Released),
                t0 + Duration::from_millis(500),
            )
            .is_none());
        assert_eq!(store.dispatched_receipts().len(), before_hold_len + 1);
        store.cancel_encoder_model_picker();

        let turn_receipt = store
            .consume_physical_input_event(
                mk_ev(5, "AG13", InputEventKind::Pressed),
                t0 + Duration::from_millis(600),
            )
            .expect("idle CCW turn enters model picker");
        assert_eq!(turn_receipt.detail, "antigravity/model-picker-prev");
        assert!(!store.is_model_picker_mouse_capture_enabled());

        let closed_receipt = HostActionReceipt {
            action_type: "agent_command".into(),
            binding_id: "ENC_CLK".into(),
            detail: "antigravity/model-picker-already-closed".into(),
            outcome: "executed".into(),
            profile_id: ag_id.clone(),
            target: "antigravity".into(),
            text_byte_len: None,
            timestamp_ms: 1,
        };
        assert_eq!(
            store.finalize_encoder_confirm_outcome(Some(&closed_receipt)),
            crate::encoder_model_picker::ConfirmExecutionDisposition::FallbackToShortPress
        );
        assert!(!store.is_model_picker_mouse_capture_enabled());

        let fallback_search = store
            .execute_encoder_short_press(&ag_id, 1, t0 + Duration::from_millis(680))
            .expect("already-closed confirm falls through to slot 15 search");
        assert_eq!(fallback_search.detail, "antigravity/search");
        assert_eq!(
            store
                .encoder_model_picker
                .active_kind(t0 + Duration::from_millis(680)),
            Some(crate::encoder_model_picker::EncoderOverlayKind::SearchConversations)
        );
    }

    #[test]
    fn workbench_store_press_and_turn_fast_scrolls_and_search_aligns_target_adapter() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(dir.path()).expect("open store");
        store.set_host_execution_enabled(true).unwrap();
        let ag_id = store
            .find_canonical_profile_id(crate::CanonicalAgent::Antigravity)
            .unwrap();
        store.select_and_activate_profile(&ag_id).unwrap();

        let mk_ev = |seq: u64, binding: &str, kind: InputEventKind| InputEvent {
            session_id: 1,
            board_layer: None,
            sequence: seq,
            ingress_sequence: seq,
            radial_center: None,
            radial_position: None,
            timestamp_ms: seq,
            kind,
            logical_switch: None,
            binding_id: Some(binding.into()),
            physical_control: None,
            keycap_group: None,
            action_role: None,
            lost_count: 0,
            action_eligible: true,
        };

        let t0 = Instant::now();
        assert!(store
            .consume_physical_input_event(mk_ev(1, "ENC_CLK", InputEventKind::Pressed), t0)
            .is_none());
        let fast_scroll = store
            .consume_physical_input_event(
                mk_ev(2, "AG14", InputEventKind::Pressed),
                t0 + Duration::from_millis(50),
            )
            .expect("press+turn dispatches fast scroll");
        assert_eq!(fast_scroll.detail, "antigravity/scroll-down-fast");
        assert!(!store.is_model_picker_mouse_capture_enabled());
        assert!(!store
            .encoder_model_picker
            .is_active(t0 + Duration::from_millis(50)));
        assert!(store
            .consume_physical_input_event(
                mk_ev(3, "ENC_CLK", InputEventKind::Released),
                t0 + Duration::from_millis(90),
            )
            .is_none());

        let dsh_id = store
            .find_canonical_profile_id(crate::CanonicalAgent::DeepseekHarness)
            .unwrap();
        let dsh_lid = store
            .document_store
            .host_store()
            .draft
            .document
            .editable
            .graph
            .profiles[&dsh_id]
            .layer_order[0]
            .clone();
        store
            .document_store
            .update(|draft| {
                draft.assign(
                    &serde_json::json!(dsh_id),
                    &serde_json::json!(dsh_lid),
                    0,
                    mi_model::preset::canonical_encoder_hold_action(),
                )
            })
            .unwrap();
        store.select_and_activate_profile(&dsh_id).unwrap();
        store.foreground_bundle_id = Some("com.google.antigravity".to_string());

        let t1 = t0 + Duration::from_millis(200);
        let direct_search = store
            .execute_resolved_physical_action_with_session(
                &dsh_id,
                "AG00",
                1,
                ResolvedHostAction::AgentCommand {
                    adapter: "host".into(),
                    command: "search".into(),
                },
            )
            .expect("host/search resolves via profile target adapter");
        assert_eq!(direct_search.detail, "dsh-desktop/search");

        let search_open = store
            .consume_physical_input_event(mk_ev(4, "AG00", InputEventKind::Pressed), t1)
            .expect("host/search slot opens search");
        assert_eq!(search_open.detail, "dsh-desktop/search");
        assert!(!store.is_model_picker_mouse_capture_enabled());

        let search_step = store
            .consume_physical_input_event(
                mk_ev(5, "AG14", InputEventKind::Pressed),
                t1 + Duration::from_millis(40),
            )
            .expect("encoder turn steps search");
        assert_eq!(search_step.detail, "dsh-desktop/search-next");
        assert_eq!(direct_search.target, search_step.target);
        assert_eq!(search_open.target, search_step.target);
    }
}
