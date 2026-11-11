use mi_macos::ax_dictation::{
    is_valid_ax_leaf_on_no_value, AXDictationController, AxElementNode, AxTreeProvider,
    AxWindowSnapshot, DictationAdapter, DictationOutcome,
};
use mi_macos::executor::{
    ClipboardItemSnapshot, ClipboardSnapshot, HeldInputId, LiveHostActionExecutor, RecordedOsEvent,
    RecordingOsDriver, APPLE_FN_KEY_CODE, CG_EVENT_FLAG_MASK_SECONDARY_FN,
    CG_EVENT_TYPE_FLAGS_CHANGED,
};
use mi_macos::{
    run_config_write_scenario, run_diagnostic_export_scenario, run_host_execution_scenario,
    run_lighting_control_scenario, AppBundleCache, AppBundleInfo, DocumentInstanceLock,
    OTHER_APPS_WILDCARD_BUNDLE_ID,
};
use mi_model::host_router::{ResolvedHostAction, ResolvedMacroEvent};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

#[test]
fn unsupported_session_navigation_does_not_report_success() {
    for action in [
        ResolvedHostAction::AgentCommand {
            adapter: "four-app".into(),
            command: "slot-open:0".into(),
        },
        ResolvedHostAction::FocusSession {
            adapter: "antigravity".into(),
            session: "session-1".into(),
        },
    ] {
        let driver = RecordingOsDriver::new(true);
        let executor = LiveHostActionExecutor::new(driver.clone());
        let receipt = executor.execute("Antigravity", "AG00", &action);
        assert_eq!(receipt.outcome, "blocked", "{action:?}");
        assert_eq!(receipt.detail, "session_navigation_unavailable");
        assert!(driver.events().is_empty());
    }
}

#[test]
fn unsupported_agent_command_does_not_activate_an_application() {
    let driver = RecordingOsDriver::new(true);
    let executor = LiveHostActionExecutor::new(driver.clone());
    let receipt = executor.execute(
        "Antigravity",
        "AG00",
        &ResolvedHostAction::AgentCommand {
            adapter: "antigravity".into(),
            command: "unknown-command".into(),
        },
    );
    assert_eq!(receipt.outcome, "blocked");
    assert_eq!(receipt.detail, "unsupported_agent_command");
    assert!(driver.events().is_empty());
}

#[test]
fn supported_agent_command_performs_the_requested_action() {
    let driver = RecordingOsDriver::new(true);
    let executor = LiveHostActionExecutor::new(driver.clone());
    let receipt = executor.execute(
        "Antigravity",
        "AG07",
        &ResolvedHostAction::AgentCommand {
            adapter: "antigravity".into(),
            command: "focus-input".into(),
        },
    );
    assert_eq!(receipt.outcome, "executed");
    assert_eq!(
        driver.events(),
        vec![
            RecordedOsEvent::CheckAccessibilityTrusted,
            RecordedOsEvent::FocusAgentInput("antigravity".into()),
        ]
    );
}

#[test]
fn test_document_instance_lock_rejects_duplicate_owner_until_released() {
    let temp = TempDir::new().expect("temp dir");
    let doc_dir = temp.path().join("micro-input-doc");

    assert_eq!(DocumentInstanceLock::owner_pid(&doc_dir), None);

    let mut first = DocumentInstanceLock::acquire_with_pid(&doc_dir, 4242)
        .expect("first acquire io")
        .expect("first lock acquired");
    assert!(first.is_held());
    assert_eq!(DocumentInstanceLock::owner_pid(&doc_dir), Some(4242));

    let duplicate =
        DocumentInstanceLock::acquire_with_pid(&doc_dir, 9999).expect("duplicate acquire io");
    assert!(
        duplicate.is_none(),
        "second lock on same document_dir must be rejected while first is held"
    );
    assert_eq!(DocumentInstanceLock::owner_pid(&doc_dir), Some(4242));

    first.release();
    assert!(!first.is_held());
    assert_eq!(DocumentInstanceLock::owner_pid(&doc_dir), None);

    {
        let _reacquired = DocumentInstanceLock::acquire_with_pid(&doc_dir, 7777)
            .expect("reacquire io")
            .expect("lock reacquired after release");
        assert_eq!(DocumentInstanceLock::owner_pid(&doc_dir), Some(7777));
    }
    assert_eq!(DocumentInstanceLock::owner_pid(&doc_dir), None);
}

#[test]
fn test_app_bundle_cache_caches_lookups_and_handles_wildcard() {
    let cache = AppBundleCache::with_resolver(Arc::new(|bundle_id| AppBundleInfo {
        bundle_id: bundle_id.to_string(),
        installed: bundle_id == "com.google.antigravity",
        app_path: Some(PathBuf::from("/Applications/Antigravity.app")),
        display_name: Some("Antigravity".to_string()),
        icon_cached: true,
    }));

    let wildcard = cache.resolve(OTHER_APPS_WILDCARD_BUNDLE_ID);
    assert_eq!(wildcard.bundle_id, "*");
    assert!(wildcard.installed);
    assert_eq!(wildcard.app_path, None);
    assert_eq!(wildcard.display_name.as_deref(), Some("其他软件"));
    assert!(!wildcard.icon_cached);
    assert_eq!(cache.lookup_count(), 0, "wildcard must bypass resolver I/O");

    let first = cache.resolve("com.google.antigravity");
    assert!(first.installed);
    assert_eq!(first.display_name.as_deref(), Some("Antigravity"));
    assert_eq!(cache.lookup_count(), 1);
    assert_eq!(cache.cache_hits(), 0);

    let second = cache.resolve("com.google.antigravity");
    assert_eq!(second, first);
    assert_eq!(
        cache.lookup_count(),
        1,
        "repeated lookup must not invoke resolver"
    );
    assert_eq!(cache.cache_hits(), 1);
}

#[test]
fn test_live_host_action_executor_apple_fn_paste_submit_and_macro_trust() {
    let driver = RecordingOsDriver::new(true);
    driver.seed_clipboard(
        vec![
            ClipboardItemSnapshot {
                types_and_bytes: vec![
                    ("public.utf8-plain-text".to_string(), b"original-1".to_vec()),
                    ("public.html".to_string(), b"<b>original-1</b>".to_vec()),
                ],
            },
            ClipboardItemSnapshot {
                types_and_bytes: vec![("public.png".to_string(), vec![0x89, 0x50, 0x4E, 0x47])],
            },
        ],
        10,
    );
    let executor = LiveHostActionExecutor::new(driver.clone());

    let input = HeldInputId {
        session_id: 1,
        binding_id: "AG04".into(),
    };
    let fn_press = executor
        .handle_apple_fn_press(&input, "Antigravity", "frontmost-editor")
        .expect("apple fn press receipt");
    assert_eq!(fn_press.outcome, "executed");
    assert_eq!(fn_press.detail, "fn (press)");
    assert!(
        executor
            .handle_apple_fn_press(&input, "Antigravity", "frontmost-editor")
            .is_none(),
        "duplicate press while held must be ignored"
    );
    let fn_release = executor
        .handle_apple_fn_release(&input)
        .expect("apple fn release receipt");
    assert_eq!(fn_release.outcome, "executed");
    assert_eq!(fn_release.detail, "fn (release)");

    let events = driver.events();
    assert!(events.contains(&RecordedOsEvent::PostAppleFnFlags {
        event_type: CG_EVENT_TYPE_FLAGS_CHANGED,
        key_code: APPLE_FN_KEY_CODE,
        flags: CG_EVENT_FLAG_MASK_SECONDARY_FN,
        key_down: true,
    }));
    assert!(events.contains(&RecordedOsEvent::PostAppleFnFlags {
        event_type: CG_EVENT_TYPE_FLAGS_CHANGED,
        key_code: APPLE_FN_KEY_CODE,
        flags: 0,
        key_down: false,
    }));

    let secret_prompt = "SuperSecretPromptText\r\n";
    let paste_action = ResolvedHostAction::PasteText {
        text: secret_prompt.to_string(),
        target_bundle_id: Some("com.google.antigravity".to_string()),
        adapter: Some("antigravity".to_string()),
        no_newline: true,
        submit: false,
    };
    let paste_receipt = executor.execute("Antigravity", "AG15", &paste_action);
    assert_eq!(paste_receipt.outcome, "executed");
    assert_eq!(paste_receipt.action_type, "paste_text");
    assert_eq!(paste_receipt.detail, "no_newline=true (bytes=21)");
    assert_eq!(paste_receipt.text_byte_len, Some(21));
    let serialized = serde_json::to_string(&paste_receipt).expect("serialize receipt");
    assert!(
        !serialized.contains("SuperSecretPromptText"),
        "receipt must never contain plaintext payload"
    );
    let restored_clip = driver.current_clipboard();
    assert_eq!(
        restored_clip.items.len(),
        2,
        "multi-item clipboard must be losslessly restored"
    );
    assert_eq!(restored_clip.items[0].types_and_bytes.len(), 2);

    driver.set_dictation_active(true);
    let before_submit_count = driver.events().len();
    let submit_action = ResolvedHostAction::AgentCommand {
        adapter: "antigravity".to_string(),
        command: "submit".to_string(),
    };
    let submit_receipt = executor.execute("Antigravity", "AG09", &submit_action);
    assert!(submit_receipt.succeeded());
    let post_submit_events: Vec<_> = driver
        .events()
        .into_iter()
        .skip(before_submit_count)
        .collect();
    let stop_idx = post_submit_events
        .iter()
        .position(|e| matches!(e, RecordedOsEvent::StopActiveDictation(a) if a == "antigravity"))
        .expect("must stop active dictation on submit");
    let focus_idx = post_submit_events
        .iter()
        .position(|e| matches!(e, RecordedOsEvent::FocusAgentInput(a) if a == "antigravity"))
        .expect("must focus agent input on submit");
    assert!(
        stop_idx < focus_idx,
        "submit must stop active dictation before focusing/sending Return"
    );
    assert!(
        post_submit_events.iter().any(|e| matches!(
            e,
            RecordedOsEvent::PostKeyShortcut {
                key_code: mi_macos::executor::KEY_CODE_RETURN,
                modifiers,
                key_down: true,
            } if modifiers == &vec!["command".to_string()]
        )),
        "antigravity submit must send Cmd+Enter"
    );

    let new_project_chat_action = ResolvedHostAction::AgentCommand {
        adapter: "antigravity".to_string(),
        command: "new-conversation-in-project".to_string(),
    };
    let new_project_chat_receipt =
        executor.execute("Antigravity", "AG10", &new_project_chat_action);
    assert_eq!(new_project_chat_receipt.outcome, "executed");
    assert_eq!(
        new_project_chat_receipt.detail,
        "antigravity/new-conversation-in-project"
    );
    assert!(
        driver.events().iter().any(
            |e| matches!(e, RecordedOsEvent::NewConversationInProject(a) if a == "antigravity")
        ),
        "new-conversation-in-project must invoke driver new_conversation_in_project"
    );

    driver.set_accessibility_trusted(false);
    let macro_action = ResolvedHostAction::MacroSequence {
        events: vec![
            ResolvedMacroEvent::Press {
                key: "a".to_string(),
                key_code: 0x00,
            },
            ResolvedMacroEvent::Release {
                key: "a".to_string(),
                key_code: 0x00,
            },
        ],
    };
    let macro_receipt = executor.execute("Antigravity", "AG12", &macro_action);
    assert_eq!(macro_receipt.outcome, "blocked");
    assert_eq!(macro_receipt.detail, "accessibility_untrusted");
}

struct MockAxProvider {
    window: Mutex<AxWindowSnapshot>,
    press_calls: Mutex<Vec<String>>,
    replace_control_on_stop: Mutex<bool>,
    clipboard: Mutex<ClipboardSnapshot>,
}

impl AxTreeProvider for MockAxProvider {
    fn frontmost_window(&self, _adapter: DictationAdapter) -> Option<AxWindowSnapshot> {
        Some(self.window.lock().unwrap().clone())
    }

    fn window_by_identity(
        &self,
        pid: i32,
        window_id: u64,
        window_title: &str,
    ) -> Option<AxWindowSnapshot> {
        let win = self.window.lock().unwrap().clone();
        if win.pid == pid && win.window_id == window_id && win.window_title == window_title {
            Some(win)
        } else {
            None
        }
    }

    fn ax_press(&self, _pid: i32, _window_id: u64, control_id: &str) -> Result<(), String> {
        self.press_calls
            .lock()
            .unwrap()
            .push(control_id.to_string());
        let replace = *self.replace_control_on_stop.lock().unwrap();
        let mut win = self.window.lock().unwrap();
        if let Some(ctrl) = win.controls.iter_mut().find(|c| c.control_id == control_id) {
            if ctrl.label == "开始听写" {
                ctrl.label = "停止听写".to_string();
            } else if ctrl.label == "停止听写" {
                if replace {
                    ctrl.control_id = "btn-voice-rebuilt-2".to_string();
                }
                ctrl.label = "开始听写".to_string();
            }
        }
        Ok(())
    }

    fn snapshot_clipboard(&self, _max_bytes: usize) -> ClipboardSnapshot {
        self.clipboard.lock().unwrap().clone()
    }

    fn restore_clipboard(&self, snapshot: &ClipboardSnapshot, expected_change_count: u64) -> bool {
        let mut current = self.clipboard.lock().unwrap();
        if current.change_count == expected_change_count {
            *current = snapshot.clone();
            true
        } else {
            false
        }
    }
}

#[test]
fn test_ax_dictation_leaf_validation_4tuple_lock_and_replacement_readback() {
    assert!(DictationAdapter::Dsh.matches_start_label("开始听写"));
    assert!(DictationAdapter::Dsh.matches_stop_label("停止听写"));
    assert!(DictationAdapter::Dsh.matches_stop_label("停止并识别"));

    let leaf_attrs = vec!["AXRole".to_string(), "AXTitle".to_string()];
    let container_attrs = vec![
        "AXRole".to_string(),
        "AXTitle".to_string(),
        "AXChildren".to_string(),
    ];
    assert!(is_valid_ax_leaf_on_no_value(
        Some("AXButton"),
        Some(&leaf_attrs)
    ));
    assert!(!is_valid_ax_leaf_on_no_value(
        Some("AXGroup"),
        Some(&container_attrs)
    ));
    assert!(!is_valid_ax_leaf_on_no_value(None, Some(&leaf_attrs)));
    assert!(!is_valid_ax_leaf_on_no_value(Some(""), Some(&leaf_attrs)));
    assert!(!is_valid_ax_leaf_on_no_value(Some("AXButton"), None));

    let provider = Arc::new(MockAxProvider {
        window: Mutex::new(AxWindowSnapshot {
            pid: 5150,
            window_id: 9001,
            window_title: "DSH - 主窗口".to_string(),
            controls: vec![AxElementNode {
                control_id: "btn-voice-1".to_string(),
                role: "AXButton".to_string(),
                label: "开始听写".to_string(),
                parent_chain_to_window: vec![
                    "toolbar-1".to_string(),
                    "root-split".to_string(),
                    "9001".to_string(),
                ],
                attribute_names: leaf_attrs,
            }],
        }),
        press_calls: Mutex::new(Vec::new()),
        replace_control_on_stop: Mutex::new(true),
        clipboard: Mutex::new(ClipboardSnapshot {
            change_count: 7,
            items: vec![ClipboardItemSnapshot {
                types_and_bytes: vec![("public.utf8-plain-text".to_string(), b"keep-me".to_vec())],
            }],
        }),
    });

    let controller = AXDictationController::new(provider.clone());
    let start = controller.start_dictation(DictationAdapter::Dsh);
    let locked = match start {
        DictationOutcome::Started(t) => t,
        other => panic!("expected Started, got {:?}", other),
    };
    assert_eq!(locked.pid, 5150);
    assert_eq!(locked.window_id, 9001);
    assert_eq!(locked.window_title, "DSH - 主窗口");
    assert_eq!(locked.control_id, "btn-voice-1");

    let stop = controller.stop_dictation();
    assert_eq!(
        stop,
        DictationOutcome::Stopped {
            used_replacement_idle_readback: true,
            clipboard_restored: true,
        }
    );
    let presses = provider.press_calls.lock().unwrap().clone();
    assert_eq!(
        presses,
        vec!["btn-voice-1".to_string(), "btn-voice-1".to_string()],
        "replacement control btn-voice-rebuilt-2 must only be read-confirmed, never pressed"
    );
}

#[test]
fn test_four_synthetic_scenarios_match_verify_offline_contracts() {
    let temp = TempDir::new().expect("temp dir");

    let host_dir = temp.path().join("host-execution");
    let host_summary = run_host_execution_scenario(&host_dir).expect("run_host_execution_scenario");
    assert_eq!(host_summary.status, "PASS");
    let audit_raw =
        fs::read_to_string(host_dir.join("evidence/audit.jsonl")).expect("read audit.jsonl");
    let rows: Vec<Value> = audit_raw
        .lines()
        .map(|l| serde_json::from_str(l).expect("parse audit row"))
        .collect();
    assert_eq!(rows.len(), 20);
    assert!(rows.iter().all(|r| r["outcome"] == "simulated"));
    let details: BTreeSet<String> = rows
        .iter()
        .filter_map(|r| r["detail"].as_str().map(str::to_string))
        .collect();
    for expected in [
        "antigravity/prev-conversation",
        "antigravity/next-conversation",
        "antigravity/search",
        "no_newline=true (bytes=8)",
    ] {
        assert!(details.contains(expected), "missing detail {expected}");
    }
    assert!(details.iter().any(|d| d.ends_with("/submit")));
    let bindings: BTreeSet<String> = rows
        .iter()
        .filter_map(|r| r["binding_id"].as_str().map(str::to_string))
        .collect();
    assert!(bindings.contains("menu-navigate-back"));
    assert!(bindings.contains("menu-navigate-forward"));
    let profiles: BTreeSet<String> = rows
        .iter()
        .filter_map(|r| r["profile_id"].as_str().map(str::to_string))
        .collect();
    assert_eq!(profiles.len(), 4);
    let wide_count = rows
        .iter()
        .filter(|r| matches!(r["binding_id"].as_str(), Some("AG10" | "AG11")))
        .count();
    assert_eq!(wide_count, 1);
    let last_row = rows.last().expect("last row");
    assert_eq!(last_row["action_type"], "macro_sequence");
    assert_eq!(last_row["detail"], "steps=2 keys=2 delays=0");

    let cfg_dir = temp.path().join("config-write");
    let cfg_summary = run_config_write_scenario(&cfg_dir).expect("run_config_write_scenario");
    assert_eq!(cfg_summary.status, "PASS");
    let cfg_ops_raw = fs::read_to_string(cfg_dir.join("evidence/device-operations.jsonl"))
        .expect("read device-operations.jsonl");
    let cfg_ops: Vec<Value> = cfg_ops_raw
        .lines()
        .map(|l| serde_json::from_str(l).expect("parse op"))
        .collect();
    assert_eq!(cfg_ops.len(), 2);
    assert!(cfg_ops.iter().all(|r| {
        r["phase"] == "Verified"
            && r["config_readback"] == "PASS"
            && r["physical_key"] == "NOT_RUN"
            && r["lighting_visual"] == "NOT_RUN"
    }));
    let editor_ev: Value = serde_json::from_str(
        &fs::read_to_string(cfg_dir.join("evidence/device-editor.json"))
            .expect("read device-editor.json"),
    )
    .expect("parse device-editor.json");
    assert_eq!(editor_ev["status"], "PASS");
    assert_eq!(editor_ev["host_document"], "UNCHANGED");
    assert_eq!(editor_ev["host_preferences"], "UNCHANGED");
    assert_eq!(editor_ev["mixed_changes"], "3");
    assert_eq!(editor_ev["routing"], "PASS");

    let light_dir = temp.path().join("lighting-control");
    let light_summary =
        run_lighting_control_scenario(&light_dir).expect("run_lighting_control_scenario");
    assert_eq!(light_summary.status, "PASS");
    let light_ops_raw = fs::read_to_string(light_dir.join("evidence/device-operations.jsonl"))
        .expect("read lighting device-operations.jsonl");
    let light_ops: Vec<Value> = light_ops_raw
        .lines()
        .map(|l| serde_json::from_str(l).expect("parse op"))
        .collect();
    assert_eq!(light_ops.len(), 2);
    assert!(light_ops.iter().all(|r| r["phase"] == "Verified"));
    let light_ev: Value = serde_json::from_str(
        &fs::read_to_string(light_dir.join("evidence/lighting.json")).expect("read lighting.json"),
    )
    .expect("parse lighting.json");
    assert_eq!(light_ev["status"], "PASS");
    assert_eq!(light_ev["runtime_readback"], "SYNTHETIC_PASS");
    assert_eq!(light_ev["physical_key"], "NOT_RUN");
    assert_eq!(light_ev["lighting_visual"], "NOT_RUN");

    let diag_dir = temp.path().join("diagnostic-export");
    let report_path = diag_dir.join(" exported-diagnostic-report.json");
    let diag_summary = run_diagnostic_export_scenario(&diag_dir, &report_path)
        .expect("run_diagnostic_export_scenario");
    assert_eq!(diag_summary.status, "PASS");
    let report_raw = fs::read_to_string(&report_path).expect("read diagnostic report");
    let forbidden_user_prefix = format!("/{}s/", "User");
    for forbidden in [
        forbidden_user_prefix.as_str(),
        "/private",
        "/tmp",
        "SECRET-SERIAL",
        "KC_A",
    ] {
        assert!(
            !report_raw.contains(forbidden),
            "diagnostic report leaked forbidden token: {forbidden}"
        );
    }
    let report_mode = fs::metadata(&report_path)
        .expect("report metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(report_mode, 0o600);
    let diag_ev_path = diag_dir.join("evidence/diagnostics.json");
    let diag_ev_mode = fs::metadata(&diag_ev_path)
        .expect("diagnostics.json metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(diag_ev_mode, 0o600);
    let diag_ev: Value =
        serde_json::from_str(&fs::read_to_string(&diag_ev_path).expect("read diagnostics.json"))
            .expect("parse diagnostics.json");
    assert_eq!(diag_ev["status"], "PASS");
    assert_eq!(diag_ev["failure_stage"], "config_read");
    assert_eq!(diag_ev["physical_key"], "NOT_RUN");
    assert_eq!(diag_ev["lighting_visual"], "NOT_RUN");
}

#[test]
fn antigravity_new_conversation_target_selector_matches_active_project_or_falls_back() {
    use mi_macos::{
        select_antigravity_new_conversation_target, AntigravityNewConversationTarget,
        AntigravitySidebarAxNode,
    };

    let nodes = vec![
        AntigravitySidebarAxNode {
            index: 0,
            role: "AXButton".into(),
            title: "agent-fleet".into(),
            description: String::new(),
        },
        AntigravitySidebarAxNode {
            index: 1,
            role: "AXPopUpButton".into(),
            title: String::new(),
            description: "Project options".into(),
        },
        AntigravitySidebarAxNode {
            index: 2,
            role: "AXLink".into(),
            title: String::new(),
            description: "New Conversation in Project".into(),
        },
        AntigravitySidebarAxNode {
            index: 3,
            role: "AXButton".into(),
            title: "responsay-macos".into(),
            description: String::new(),
        },
        AntigravitySidebarAxNode {
            index: 4,
            role: "AXPopUpButton".into(),
            title: String::new(),
            description: "Project options".into(),
        },
        AntigravitySidebarAxNode {
            index: 5,
            role: "AXLink".into(),
            title: String::new(),
            description: "New Conversation in Project".into(),
        },
        AntigravitySidebarAxNode {
            index: 6,
            role: "AXButton".into(),
            title: "micro-input".into(),
            description: String::new(),
        },
        AntigravitySidebarAxNode {
            index: 7,
            role: "AXPopUpButton".into(),
            title: String::new(),
            description: "Project options".into(),
        },
        AntigravitySidebarAxNode {
            index: 8,
            role: "AXLink".into(),
            title: String::new(),
            description: "New Conversation in Workspace".into(),
        },
    ];

    assert_eq!(
        select_antigravity_new_conversation_target(
            "Product Architecture Alignment And Analysis - responsay-macos - Antigravity",
            &nodes
        ),
        AntigravityNewConversationTarget::ActiveProject {
            project_label: "responsay-macos".into(),
            node_index: 5,
        }
    );
    assert_eq!(
        select_antigravity_new_conversation_target("micro-input - Antigravity", &nodes),
        AntigravityNewConversationTarget::ActiveProject {
            project_label: "micro-input".into(),
            node_index: 8,
        }
    );
    assert_eq!(
        select_antigravity_new_conversation_target("Standalone Chat - Antigravity", &nodes),
        AntigravityNewConversationTarget::FirstVisibleProject {
            project_label: Some("agent-fleet".into()),
            node_index: 2,
        }
    );
    assert_eq!(
        select_antigravity_new_conversation_target("Antigravity", &[]),
        AntigravityNewConversationTarget::FallbackShortcutCmdN
    );
}

#[test]
fn antigravity_live_voice_does_not_map_to_dictation_shortcut_ctrl_m() {
    use mi_macos::executor::native_shortcut_for_agent_command;
    use mi_macos::parse_pid_from_lsappinfo_output;

    assert_eq!(
        native_shortcut_for_agent_command("antigravity", "dictation"),
        Some((0x2E, vec!["control".to_string()]))
    );
    assert_eq!(
        native_shortcut_for_agent_command("antigravity", "live-voice"),
        None,
        "live-voice must trigger Antigravity Start/End Voice mode via AXPress, never Ctrl+M dictation"
    );

    let driver = RecordingOsDriver::new(true);
    let executor = LiveHostActionExecutor::new(driver.clone());
    let live_voice_action = ResolvedHostAction::AgentCommand {
        adapter: "antigravity".to_string(),
        command: "live-voice".to_string(),
    };
    let receipt = executor.execute("Antigravity", "AG06", &live_voice_action);
    assert_eq!(receipt.outcome, "executed");
    assert_eq!(receipt.detail, "antigravity/live-voice");
    assert!(
        !driver
            .events()
            .iter()
            .any(|e| matches!(e, RecordedOsEvent::PostKeyShortcut { key_code: 0x2E, .. })),
        "executing antigravity/live-voice must never post Ctrl+M (0x2E) dictation shortcut"
    );
    assert!(
        driver
            .events()
            .contains(&RecordedOsEvent::ToggleLiveVoice("antigravity".to_string())),
        "must record ToggleLiveVoice event for antigravity"
    );

    assert_eq!(
        parse_pid_from_lsappinfo_output("\"pid\"=3582\n"),
        Some(3582)
    );
    assert_eq!(
        parse_pid_from_lsappinfo_output(
            "bundleID=[ NULL ] asn=[ 0x0,0x44044 ] ... pid = 3582 !cgsConnection\n"
        ),
        Some(3582)
    );
    assert_eq!(parse_pid_from_lsappinfo_output("no pid here"), None);
}

#[test]
fn held_fn_uses_original_owner_and_releases_only_the_last_input() {
    let driver = RecordingOsDriver::new(true);
    let executor = LiveHostActionExecutor::new(driver.clone());
    let first = HeldInputId {
        session_id: 7,
        binding_id: "AG04".into(),
    };
    let second = HeldInputId {
        session_id: 7,
        binding_id: "AG05".into(),
    };
    executor
        .handle_apple_fn_press(&first, "profile-a", "target-a")
        .unwrap();
    executor
        .handle_apple_fn_press(&second, "profile-b", "target-b")
        .unwrap();
    let stale = HeldInputId {
        session_id: 6,
        binding_id: "AG04".into(),
    };
    assert!(executor.handle_apple_fn_release(&stale).is_none());
    let released = executor.handle_apple_fn_release(&first).unwrap();
    assert_eq!(released.profile_id, "profile-a");
    assert_eq!(released.target, "target-a");
    assert!(executor.handle_apple_fn_release(&first).is_none());
    let flags = || {
        driver
            .events()
            .iter()
            .filter_map(|event| match event {
                RecordedOsEvent::PostAppleFnFlags { key_down, .. } => Some(*key_down),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(flags(), vec![true]);
    let pulse = ResolvedHostAction::Shortcut {
        key: "fn".into(),
        key_code: APPLE_FN_KEY_CODE,
        modifiers: Vec::new(),
        target_bundle_id: None,
    };
    let blocked = executor.execute("new-profile", "AG06", &pulse);
    assert_eq!(blocked.detail, "apple_fn_already_held");
    assert_eq!(flags(), vec![true]);
    let released = executor.handle_apple_fn_release(&second).unwrap();
    assert_eq!(released.profile_id, "profile-b");
    assert_eq!(released.target, "target-b");
    assert_eq!(flags(), vec![true, false]);
}

#[test]
fn failed_fn_release_preserves_owner_until_confirmed_cleanup() {
    let driver = RecordingOsDriver::new(true);
    let executor = LiveHostActionExecutor::new(driver.clone());
    let input = HeldInputId {
        session_id: 7,
        binding_id: "AG04".into(),
    };
    executor
        .handle_apple_fn_press(&input, "original", "original-target")
        .unwrap();
    driver.set_accessibility_trusted(false);
    let failed = executor.handle_apple_fn_release(&input).unwrap();
    assert_eq!(failed.outcome, "failed");
    assert!(executor.has_unconfirmed_apple_fn_release());
    assert_eq!(failed.profile_id, "original");
    assert!(executor.stop_owned_actions().is_err());
    driver.set_accessibility_trusted(true);
    let shortcut = ResolvedHostAction::Shortcut {
        key: "a".into(),
        key_code: 0,
        modifiers: vec!["command".into()],
        target_bundle_id: Some("com.example.editor".into()),
    };
    let events = driver.events();
    let refused = executor.execute("new", "AG01", &shortcut);
    assert_eq!(refused.outcome, "blocked");
    assert_eq!(refused.detail, "apple_fn_cleanup_unconfirmed");
    assert_eq!(
        driver.events(),
        events,
        "uncertain cleanup blocks all host effects"
    );
    let next = HeldInputId {
        session_id: 8,
        binding_id: "AG04".into(),
    };
    let blocked = executor
        .handle_apple_fn_press(&next, "new", "new-target")
        .unwrap();
    assert_eq!(blocked.outcome, "blocked");
    assert_eq!(blocked.detail, "apple_fn_cleanup_unconfirmed");
    let retried = executor.handle_apple_fn_release(&input).unwrap();
    assert_eq!(retried.outcome, "executed");
    assert!(!executor.has_unconfirmed_apple_fn_release());
    assert_eq!(retried.target, "original-target");
    assert!(executor.handle_apple_fn_release(&input).is_none());
    assert_eq!(
        executor
            .handle_apple_fn_press(&next, "new", "new-target")
            .unwrap()
            .outcome,
        "executed"
    );
    executor.stop_owned_actions().unwrap();
    assert!(executor.handle_apple_fn_release(&next).is_none());
    let flags: Vec<_> = driver
        .events()
        .iter()
        .filter_map(|event| match event {
            RecordedOsEvent::PostAppleFnFlags { key_down, .. } => Some(*key_down),
            _ => None,
        })
        .collect();
    assert_eq!(flags, vec![true, false, true, false]);
}

#[test]
fn dropping_executor_releases_its_remaining_fn_output() {
    let driver = RecordingOsDriver::new(true);
    {
        let executor = LiveHostActionExecutor::new(driver.clone());
        let input = HeldInputId {
            session_id: 9,
            binding_id: "AG04".into(),
        };
        executor
            .handle_apple_fn_press(&input, "profile", "target")
            .unwrap();
    }
    let flags: Vec<_> = driver
        .events()
        .iter()
        .filter_map(|event| match event {
            RecordedOsEvent::PostAppleFnFlags { key_down, .. } => Some(*key_down),
            _ => None,
        })
        .collect();
    assert_eq!(flags, vec![true, false]);
}

#[test]
fn retired_agent_actions_are_blocked_and_chatgpt_fallback_remains_targeted() {
    let driver = RecordingOsDriver::new(true);
    let executor = LiveHostActionExecutor::new(driver.clone());
    for command in [
        "focus-input",
        "search",
        "new-chat",
        "submit",
        "push-to-talk",
        "toggle-dictation",
        "approve",
        "scroll-down",
        "prompt:test",
    ] {
        let receipt = executor.execute(
            "saved",
            "AG00",
            &ResolvedHostAction::AgentCommand {
                adapter: "cindy".into(),
                command: command.into(),
            },
        );
        assert_eq!(receipt.outcome, "blocked");
        assert_eq!(receipt.detail, "unsupported_agent_adapter");
    }
    let receipt = executor.execute(
        "saved",
        "AG00",
        &ResolvedHostAction::PasteText {
            text: "test".into(),
            adapter: Some("cindy".into()),
            target_bundle_id: None,
            no_newline: true,
            submit: true,
        },
    );
    assert_eq!(receipt.outcome, "blocked");
    assert!(driver.events().is_empty());
    assert!(DictationAdapter::parse("cindy").is_none());
    for command in [
        "new-chat",
        "search",
        "model",
        "focus-input",
        "scroll-down",
        "submit",
    ] {
        let receipt = executor.execute(
            "ChatGPT",
            "AG00",
            &ResolvedHostAction::AgentCommand {
                adapter: "chatgpt".into(),
                command: command.into(),
            },
        );
        assert_eq!(receipt.outcome, "executed", "{command}: {}", receipt.detail);
    }
    assert_eq!(
        mi_macos::executor::bundle_id_for_adapter("chatgpt"),
        Some("com.openai.atlas")
    );
    assert!(!driver.events().is_empty());
}
