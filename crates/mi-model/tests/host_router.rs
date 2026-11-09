use mi_model::{
    draft::{Action, ActionKind, Draft},
    host_router::{
        binding_to_logical_slot, check_passthrough_health, resolve_host_action,
        resolve_host_action_with_command_consent, HostIdentityStatus, PassThroughHealth,
        ResolvedHostAction,
    },
    preset::install_agent_starter_profiles,
    storage::DocumentStore,
    Binding, Token,
};
use serde_json::json;

#[test]
fn binding_to_logical_slot_handles_observed_row0_order_and_joystick_sectors() {
    assert_eq!(binding_to_logical_slot("AG00"), Some("logical-0"));
    assert_eq!(binding_to_logical_slot("AG01"), Some("logical-1"));
    assert_eq!(binding_to_logical_slot("AG02"), Some("logical-2"));
    assert_eq!(binding_to_logical_slot("AG10"), Some("logical-10"));
    assert_eq!(binding_to_logical_slot("AG11"), Some("logical-11"));
    assert_eq!(binding_to_logical_slot("AG12"), Some("logical-12"));
    assert_eq!(binding_to_logical_slot("AG13"), Some("logical-13"));
    assert_eq!(binding_to_logical_slot("AG14"), Some("logical-14"));
    assert_eq!(binding_to_logical_slot("AG15"), Some("logical-16"));
    assert_eq!(binding_to_logical_slot("AG16"), Some("logical-17"));
    assert_eq!(binding_to_logical_slot("AG17"), Some("logical-18"));
    assert_eq!(binding_to_logical_slot("AG18"), Some("logical-19"));
    assert_eq!(binding_to_logical_slot("ACT06"), Some("logical-6"));
    assert_eq!(binding_to_logical_slot("ACT10"), Some("logical-10"));
    assert_eq!(binding_to_logical_slot("ACT11"), Some("logical-11"));
    assert_eq!(binding_to_logical_slot("ACT12"), Some("logical-12"));
    assert_eq!(binding_to_logical_slot("ENC_CC"), Some("logical-13"));
    assert_eq!(binding_to_logical_slot("ENC_CW"), Some("logical-14"));
    assert_eq!(binding_to_logical_slot("ENC_CLK"), Some("logical-15"));
    assert_eq!(binding_to_logical_slot("AG19"), None);
    assert_eq!(binding_to_logical_slot("invalid"), None);
}

#[test]
fn passthrough_health_inspects_connection_snapshot_and_layer_drift() {
    assert_eq!(
        check_passthrough_health(None, None),
        PassThroughHealth::Disconnected
    );

    let identity = HostIdentityStatus {
        version: Some("0.6.3".into()),
        profile_index: Some(0),
        layer_index: Some(0),
    };
    assert_eq!(
        check_passthrough_health(None, Some(&identity)),
        PassThroughHealth::SnapshotMissing
    );

    let mut cfg = Draft::default().config;
    cfg.profiles[0].layers[0].keys =
        std::array::from_fn(|i| Binding::Basic(Token(format!("KV_OAI_AG{i:02}"))));
    assert_eq!(
        check_passthrough_health(Some(&cfg), Some(&identity)),
        PassThroughHealth::Ready
    );

    // Stock Input 1.1.0 Codex layer on firmware 0.6.3: 1-based layer_index=1 + AG00..AG05 + ACT06..ACT12
    let identity_one_based = HostIdentityStatus {
        version: Some("0.6.3".into()),
        profile_index: Some(0),
        layer_index: Some(1),
    };
    cfg.profiles[0].layers.truncate(1);
    cfg.profiles[0].layers[0].keys = std::array::from_fn(|i| {
        if i <= 5 {
            Binding::Unknown(json!(format!("KV_OAI_AG{i:02}")))
        } else {
            Binding::Unknown(json!(format!("KV_OAI_ACT{i:02}")))
        }
    });
    assert_eq!(
        check_passthrough_health(Some(&cfg), Some(&identity_one_based)),
        PassThroughHealth::Ready
    );

    cfg.profiles[0].layers[0].keys[12] = Binding::Empty;
    assert_eq!(
        check_passthrough_health(Some(&cfg), Some(&identity)),
        PassThroughHealth::LayerDriftWarning {
            active_profile: 0,
            active_layer: 0,
            bound_ag_count: 12,
        }
    );

    // Multi-layer drift must not be masked by an adjacent passthrough layer:
    // layers[0] = 13 AG bindings, layers[1] = 0 AG bindings.
    let mut multi_cfg = Draft::default().config;
    multi_cfg.profiles[0].layers[0].keys =
        std::array::from_fn(|i| Binding::Basic(Token(format!("KV_OAI_AG{i:02}"))));
    multi_cfg.profiles[0].layers[1].keys = std::array::from_fn(|_| Binding::Empty);

    // On real firmware 0.6.3 (1-based layer_index), layer_index=1 -> layers[0] (Ready),
    // layer_index=2 -> layers[1] (LayerDriftWarning with bound_ag_count=0).
    assert_eq!(
        check_passthrough_health(
            Some(&multi_cfg),
            Some(&HostIdentityStatus {
                version: Some("0.6.3".into()),
                profile_index: Some(0),
                layer_index: Some(1),
            })
        ),
        PassThroughHealth::Ready
    );
    assert_eq!(
        check_passthrough_health(
            Some(&multi_cfg),
            Some(&HostIdentityStatus {
                version: Some("0.6.3".into()),
                profile_index: Some(0),
                layer_index: Some(2),
            })
        ),
        PassThroughHealth::LayerDriftWarning {
            active_profile: 0,
            active_layer: 2,
            bound_ag_count: 0,
        }
    );

    // On synthetic-0.6.3 (0-based layer_index), layer_index=1 -> layers[1] (LayerDriftWarning).
    assert_eq!(
        check_passthrough_health(
            Some(&multi_cfg),
            Some(&HostIdentityStatus {
                version: Some("synthetic-0.6.3".into()),
                profile_index: Some(0),
                layer_index: Some(1),
            })
        ),
        PassThroughHealth::LayerDriftWarning {
            active_profile: 0,
            active_layer: 1,
            bound_ag_count: 0,
        }
    );
}

#[test]
fn shell_commands_route_only_after_an_independent_consent_gate() {
    let mut draft = Draft::default();
    let pid = draft.document.editable.graph.profile_order[0].clone();
    let lid = draft.document.editable.graph.profiles[&pid].layer_order[0].clone();
    let action = Action {
        id: "safe-command".into(),
        name: "写入命令标记".into(),
        category: "命令".into(),
        detail: "默认关闭 · /bin/sh · 10 秒上限".into(),
        payload: serde_json::to_value(ActionKind::ShellCommand {
            command: "printf command-marker > marker".into(),
            working_directory: "/tmp".into(),
        })
        .unwrap(),
    };
    draft.assign(&json!(pid), &json!(lid), 0, action).unwrap();

    assert!(
        resolve_host_action_with_command_consent(&draft.document, Some(&pid), "AG00", true, false)
            .is_none(),
        "command consent defaults to off and must not resolve a shell action"
    );
    assert_eq!(
        resolve_host_action_with_command_consent(&draft.document, Some(&pid), "AG00", true, true),
        Some(ResolvedHostAction::ShellCommand {
            command: "printf command-marker > marker".into(),
            working_directory: "/tmp".into(),
        })
    );
}

#[test]
fn resolve_host_action_routes_pressed_events_and_supports_wide_key_fallback() {
    let mut draft = Draft::default();
    assert_eq!(draft.revision, 0);
    assert_eq!(draft.config.profiles.len(), 2);
    let created = install_agent_starter_profiles(&mut draft).unwrap();
    assert_eq!(created.len(), 6);
    assert_eq!(
        draft.revision, 1,
        "install_agent_starter_profiles must commit as a single atomic revision"
    );
    draft.undo(false).unwrap();
    assert_eq!(
        draft.config.profiles.len(),
        2,
        "single undo must revert all 6 starter profiles"
    );
    draft.undo(true).unwrap();
    assert_eq!(draft.config.profiles.len(), 6);

    let ag_pid = &created[0];
    let herdr_pid = &created[1];
    let dsh_pid = &created[2];
    // Released events never trigger host actions
    assert!(resolve_host_action(&draft.document, Some(ag_pid), "AG00", false).is_none());

    // AG00 maps to logical-0 (Antigravity: OpenApp com.google.antigravity)
    let resolved_0 = resolve_host_action(&draft.document, Some(ag_pid), "AG00", true).unwrap();
    assert_eq!(
        resolved_0,
        ResolvedHostAction::OpenApp {
            bundle_id: "com.google.antigravity".into(),
        }
    );

    assert_eq!(
        resolve_host_action(&draft.document, Some(ag_pid), "AG01", true).unwrap(),
        ResolvedHostAction::AgentCommand {
            adapter: "antigravity".into(),
            command: "focus-input".into()
        }
    );
    for (binding, command) in [
        ("AG02", "new-chat"),
        ("AG03", "project"),
        ("AG04", "model"),
        ("AG06", "live-voice"),
        ("AG07", "focus-input"),
        ("AG08", "terminal"),
        ("AG09", "split-right"),
        ("AG10", "push-to-talk"),
        ("AG11", "push-to-talk"),
        ("AG12", "submit"),
        ("AG13", "prev-conversation"),
        ("ENC_CC", "prev-conversation"),
        ("AG14", "next-conversation"),
        ("ENC_CW", "next-conversation"),
        ("ENC_CLK", "search"),
        ("AG15", "scroll-up"),
        ("AG17", "scroll-down"),
    ] {
        assert_eq!(
            resolve_host_action(&draft.document, Some(ag_pid), binding, true),
            Some(ResolvedHostAction::AgentCommand {
                adapter: "antigravity".into(),
                command: command.into(),
            }),
            "Antigravity {binding} must resolve to {command}"
        );
    }
    for binding in ["AG05", "AG16", "AG18"] {
        assert!(
            matches!(
                resolve_host_action(&draft.document, Some(ag_pid), binding, true),
                Some(ResolvedHostAction::Shortcut { target_bundle_id: Some(ref target), .. })
                    if target == "com.google.antigravity"
            ),
            "Antigravity {binding} must resolve to com.google.antigravity shortcut"
        );
    }
    for binding in ["AG10", "AG15"] {
        assert!(resolve_host_action(&draft.document, Some(dsh_pid), binding, true).is_none());
    }
    for (binding, command) in [
        ("AG13", "scroll-up"),
        ("ENC_CC", "scroll-up"),
        ("AG14", "scroll-down"),
        ("ENC_CW", "scroll-down"),
        ("ENC_CLK", "scroll-to-bottom"),
    ] {
        assert_eq!(
            resolve_host_action(&draft.document, Some(dsh_pid), binding, true),
            Some(ResolvedHostAction::AgentCommand {
                adapter: "dsh-desktop".into(),
                command: command.into(),
            })
        );
    }
    assert_eq!(
        resolve_host_action(&draft.document, Some(herdr_pid), "AG00", true).unwrap(),
        ResolvedHostAction::OpenApp {
            bundle_id: "dev.herdr.app".into()
        }
    );
    for binding in ["AG01", "AG02", "AG10", "ENC_CLK"] {
        assert!(
            matches!(resolve_host_action(&draft.document, Some(herdr_pid), binding, true),
            Some(ResolvedHostAction::Shortcut { target_bundle_id: Some(ref target), .. }) if target == "dev.herdr.app")
        );
    }

    assert_eq!(
        resolve_host_action(&draft.document, Some(dsh_pid), "AG00", true).unwrap(),
        ResolvedHostAction::OpenApp {
            bundle_id: "com.deepseek.dsh".into()
        }
    );
    assert_eq!(
        resolve_host_action(&draft.document, Some(dsh_pid), "AG02", true).unwrap(),
        ResolvedHostAction::AgentCommand {
            adapter: "dsh-desktop".into(),
            command: "search".into()
        }
    );

    // Test Wide-Key fallback: if logical-10 has no action and logical-11 has an action, AG10 falls back to logical-11
    let lid = draft.document.editable.graph.profiles[ag_pid].layer_order[0].clone();
    draft
        .document
        .editable
        .graph
        .layers
        .get_mut(&lid)
        .unwrap()
        .controls
        .values_mut()
        .find(|b| b.logical_control == "logical-10")
        .unwrap()
        .action_id = None;
    let fallback_action = Action {
        id: "wide-fallback".into(),
        name: "宽键右触点回退".into(),
        category: "Coding".into(),
        detail: "Space".into(),
        payload: serde_json::to_value(ActionKind::Shortcut {
            modifiers: vec![],
            key: "Space".into(),
            target: "com.google.antigravity".into(),
        })
        .unwrap(),
    };
    draft
        .assign(&json!(ag_pid), &json!(lid), 11, fallback_action.clone())
        .unwrap();
    let resolved_fallback =
        resolve_host_action(&draft.document, Some(ag_pid), "AG10", true).unwrap();
    assert_eq!(
        resolved_fallback,
        ResolvedHostAction::Shortcut {
            key: "Space".into(),
            key_code: 0x31,
            modifiers: vec![],
            target_bundle_id: Some("com.google.antigravity".into()),
        }
    );
    // Raw ACT10 must also fall back to logical-11 symmetrically
    assert_eq!(
        resolve_host_action(&draft.document, Some(ag_pid), "ACT10", true),
        Some(resolved_fallback.clone())
    );
    // And when logical-11 is cleared and logical-10 is bound, ACT11 / AG11 fall back to logical-10
    draft
        .document
        .editable
        .graph
        .layers
        .get_mut(&lid)
        .unwrap()
        .controls
        .values_mut()
        .find(|b| b.logical_control == "logical-11")
        .unwrap()
        .action_id = None;
    draft
        .assign(&json!(ag_pid), &json!(lid), 10, fallback_action)
        .unwrap();
    assert_eq!(
        resolve_host_action(&draft.document, Some(ag_pid), "ACT11", true),
        Some(resolved_fallback.clone())
    );
    assert_eq!(
        resolve_host_action(&draft.document, Some(ag_pid), "AG11", true),
        Some(resolved_fallback)
    );

    for slot in 16..20 {
        draft
            .assign(
                &json!(ag_pid),
                &json!(lid),
                slot,
                mi_model::draft::Action {
                    id: format!("sector-{slot}"),
                    name: "Sector fixture".into(),
                    category: "应用".into(),
                    detail: "test".into(),
                    payload: serde_json::to_value(mi_model::draft::ActionKind::OpenApp {
                        bundle_id: "com.google.antigravity".into(),
                    })
                    .unwrap(),
                },
            )
            .unwrap();
    }
    // Joystick sectors (logical-16..19) must only resolve when joystick_mode == "logical_sectors"
    // and sector_idx < active_sectors.
    draft
        .set_control_settings(
            ag_pid,
            &lid,
            mi_model::control_settings::ControlSettings {
                joystick_mode: "logical_sectors".into(),
                active_sectors: 2,
            },
        )
        .unwrap();
    // Sector 0 (AG15 -> logical-16) and sector 1 (AG16 -> logical-17) remain active;
    // sector 2 (AG17 -> logical-18) and sector 3 (AG18 -> logical-19) are inactive.
    assert!(resolve_host_action(&draft.document, Some(ag_pid), "AG15", true).is_some());
    assert!(resolve_host_action(&draft.document, Some(ag_pid), "AG16", true).is_some());
    assert!(resolve_host_action(&draft.document, Some(ag_pid), "AG17", true).is_none());
    assert!(resolve_host_action(&draft.document, Some(ag_pid), "AG18", true).is_none());

    draft
        .set_control_settings(
            ag_pid,
            &lid,
            mi_model::control_settings::ControlSettings {
                joystick_mode: "source".into(),
                active_sectors: 4,
            },
        )
        .unwrap();
    assert!(resolve_host_action(&draft.document, Some(ag_pid), "AG15", true).is_none());
}

#[test]
fn local_multi_does_not_execute_tap_before_gesture_is_known() {
    use mi_model::multi::{ActionReference, LocalMulti};

    let mut draft = Draft::default();
    let profile_id = draft.document.editable.graph.profile_order[0].clone();
    let layer_id = draft.document.editable.graph.profiles[&profile_id].layer_order[0].clone();
    draft
        .assign(
            &json!(profile_id),
            &json!(layer_id),
            1,
            Action {
                id: uuid::Uuid::new_v4().to_string(),
                name: "Tap target".into(),
                category: "Test".into(),
                detail: "Open app".into(),
                payload: serde_json::to_value(ActionKind::OpenApp {
                    bundle_id: "com.example.test".into(),
                })
                .unwrap(),
            },
        )
        .unwrap();
    let tap_id = draft.document.editable.graph.layers[&layer_id]
        .controls
        .values()
        .find(|binding| binding.logical_control == "logical-1")
        .unwrap()
        .action_id
        .clone()
        .unwrap();
    draft
        .assign(
            &json!(profile_id),
            &json!(layer_id),
            0,
            Action {
                id: uuid::Uuid::new_v4().to_string(),
                name: "Four gestures".into(),
                category: "Test".into(),
                detail: "Multi".into(),
                payload: serde_json::to_value(ActionKind::LocalMulti {
                    multi: LocalMulti {
                        format_version: 1,
                        term_ms: 250,
                        tap: Some(ActionReference::Action { action_id: tap_id }),
                        double_tap: None,
                        hold: None,
                        tap_hold: None,
                    },
                })
                .unwrap(),
            },
        )
        .unwrap();

    assert!(resolve_host_action(&draft.document, Some(&profile_id), "AG00", true).is_none());
}

#[test]
fn verified_local_macro_resolves_as_host_sequence_and_unknown_keys_stay_draft_only() {
    use mi_model::host_router::ResolvedMacroEvent;
    use mi_model::macro_sequence::{LocalMacro, MacroEvent};

    let mut draft = Draft::default();
    let profile_id = draft.document.editable.graph.profile_order[0].clone();
    let layer_id = draft.document.editable.graph.profiles[&profile_id].layer_order[0].clone();
    let macro_action = |key: &str| Action {
        id: uuid::Uuid::new_v4().to_string(),
        name: "Local macro".into(),
        category: "Test".into(),
        detail: "Host sequence".into(),
        payload: serde_json::to_value(ActionKind::LocalMacro {
            macro_sequence: LocalMacro {
                format_version: 1,
                events: vec![
                    MacroEvent::Press { key: key.into() },
                    MacroEvent::Delay { ms: 20 },
                    MacroEvent::Release { key: key.into() },
                ],
            },
        })
        .unwrap(),
    };

    draft
        .assign(&json!(profile_id), &json!(layer_id), 0, macro_action("a"))
        .unwrap();
    assert_eq!(
        resolve_host_action(&draft.document, Some(&profile_id), "AG00", true),
        Some(ResolvedHostAction::MacroSequence {
            events: vec![
                ResolvedMacroEvent::Press {
                    key: "a".into(),
                    key_code: 0x00,
                },
                ResolvedMacroEvent::Delay { ms: 20 },
                ResolvedMacroEvent::Release {
                    key: "a".into(),
                    key_code: 0x00,
                },
            ],
        })
    );

    draft
        .assign(
            &json!(profile_id),
            &json!(layer_id),
            0,
            macro_action("unknown-key"),
        )
        .unwrap();
    assert!(resolve_host_action(&draft.document, Some(&profile_id), "AG00", true).is_none());
}

#[test]
fn host_preferences_persist_active_profile_and_execution_enabled() {
    let dir = std::env::temp_dir().join(format!("mi-host-prefs-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let doc_path = dir.join("workbench.json");

    let mut store = DocumentStore::default();
    store.open(doc_path.clone()).unwrap();
    let mut created = Vec::new();
    store
        .update(|d| {
            created = install_agent_starter_profiles(d)?;
            Ok(())
        })
        .unwrap();

    store
        .set_host_active_profile(Some(created[1].clone()))
        .unwrap();
    store.set_host_execution_enabled(true).unwrap();

    let mut reopened = DocumentStore::default();
    reopened.open(doc_path).unwrap();
    assert_eq!(
        reopened.host_active_profile_id().as_deref(),
        Some(created[1].as_str())
    );
    assert!(reopened.host_execution_enabled());

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn desktop_profiles_are_targeted_idempotent_and_preserve_edits() {
    let mut draft = Draft::default();
    let ids = install_agent_starter_profiles(&mut draft).unwrap();
    assert_eq!(draft.config.profiles.len(), 6);
    assert!(!draft
        .config
        .profiles
        .iter()
        .any(|p| p.name.contains("演示") || p.name.contains("示例")));
    for name in ["Cindy", "Codex"] {
        let pid = draft
            .document
            .editable
            .graph
            .profiles
            .iter()
            .find(|(_, p)| p.name == name)
            .unwrap()
            .0
            .clone();
        let lid = draft.document.editable.graph.profiles[&pid].layer_order[0].clone();
        assert!(
            draft.document.is_protected_oai_layer(&lid),
            "{name} default layer must be a protected KV_OAI_* layer"
        );
        assert!(
            resolve_host_action(&draft.document, Some(&pid), "AG00", true).is_none(),
            "{name} is controlled natively by its client app, not Micro Input host actions"
        );
    }
    for (name, bundle) in [("DeepSeek Coder (DSH)", "com.deepseek.dsh")] {
        let pid = draft
            .document
            .editable
            .graph
            .profiles
            .iter()
            .find(|(_, p)| p.name == name)
            .unwrap()
            .0
            .clone();
        assert_eq!(
            resolve_host_action(&draft.document, Some(&pid), "AG00", true),
            Some(ResolvedHostAction::OpenApp {
                bundle_id: bundle.into()
            })
        );
        assert!(resolve_host_action(&draft.document, Some(&pid), "AG00", false).is_none());
        assert!(
            resolve_host_action(&draft.document, Some(&pid), "AG10", true).is_none(),
            "no unverified submit/approval action"
        );
    }
    for (name, adapter) in [
        ("Antigravity", "antigravity"),
        ("DeepSeek Coder (DSH)", "dsh-desktop"),
    ] {
        let pid = draft
            .document
            .editable
            .graph
            .profiles
            .iter()
            .find(|(_, p)| p.name == name)
            .unwrap()
            .0
            .clone();
        let dial_expectations = if adapter == "antigravity" {
            [
                ("AG13", "prev-conversation"),
                ("ENC_CC", "prev-conversation"),
                ("AG14", "next-conversation"),
                ("ENC_CW", "next-conversation"),
                ("ENC_CLK", "search"),
            ]
        } else {
            [
                ("AG13", "scroll-up"),
                ("ENC_CC", "scroll-up"),
                ("AG14", "scroll-down"),
                ("ENC_CW", "scroll-down"),
                ("ENC_CLK", "scroll-to-bottom"),
            ]
        };
        for (binding, expected_cmd) in dial_expectations {
            assert_eq!(
                resolve_host_action(&draft.document, Some(&pid), binding, true),
                Some(ResolvedHostAction::AgentCommand {
                    adapter: adapter.into(),
                    command: expected_cmd.into(),
                }),
                "{name} {binding} must resolve to {adapter}/{expected_cmd}"
            );
        }
        let lid = draft.document.editable.graph.profiles[&pid].layer_order[0].clone();
        for slot in [13, 14, 15] {
            let key = mi_model::draft::binding_key(&json!(pid), &json!(lid), slot);
            let action = &draft.edits[&key];
            assert!(
                mi_model::action_library::capability(action, false).execution_available,
                "{name} slot {slot} dial action must be execution_available"
            );
        }
    }
    let before = serde_json::to_value(&draft.document.editable).unwrap();
    assert_eq!(install_agent_starter_profiles(&mut draft).unwrap(), ids);
    assert_eq!(
        serde_json::to_value(&draft.document.editable).unwrap(),
        before
    );
    let mut edited = Draft::default();
    let pid = edited.document.editable.graph.profile_order[0].clone();
    edited.rename_profile(&pid, "My existing mapping").unwrap();
    install_agent_starter_profiles(&mut edited).unwrap();
    assert_eq!(
        edited.document.editable.graph.profiles[&pid].name,
        "My existing mapping"
    );
    assert_eq!(edited.config.profiles.len(), 7);
}

#[test]
fn verified_routes_require_positions_unique_aliases_and_complete_wide_pair() {
    use mi_model::host_router::verified_bindings;
    let mut cfg = Draft::default().config;
    cfg.profiles[0].layers[0].keys =
        std::array::from_fn(|i| Binding::Unknown(json!(format!("KV_OAI_AG{i:02}"))));
    let mut identity = HostIdentityStatus {
        version: Some("0.6.3".into()),
        profile_index: Some(0),
        layer_index: Some(1),
    };
    let allowed = verified_bindings(&cfg, &identity);
    assert!(allowed.contains("AG00") && allowed.contains("AG01") && allowed.contains("ACT10"));
    cfg.profiles[0].layers[0].keys.swap(0, 1);
    let allowed = verified_bindings(&cfg, &identity);
    assert!(!allowed.contains("AG00") && !allowed.contains("AG01"));
    assert!(allowed.contains("AG02"));
    cfg.profiles[0].layers[0].keys[11] = Binding::Unknown(json!("KV_OAI_ACT02"));
    let allowed = verified_bindings(&cfg, &identity);
    assert!(!allowed.contains("AG02") && !allowed.contains("AG10") && !allowed.contains("AG11"));
    assert!(allowed.contains("AG12"));
    identity.layer_index = None;
    assert!(verified_bindings(&cfg, &identity).is_empty());
}

#[test]
fn joystick_prompt_slots_and_submit_control_route_to_target_agent_without_auto_enter_by_default() {
    let mut draft = Draft::default();
    install_agent_starter_profiles(&mut draft).unwrap();

    let catalog = mi_model::draft::actions();
    let prompt_continue = catalog.iter().find(|a| a.id == "prompt-continue").unwrap();
    let prompt_test = catalog.iter().find(|a| a.id == "prompt-test").unwrap();
    let prompt_review = catalog.iter().find(|a| a.id == "prompt-review").unwrap();
    let prompt_commit = catalog.iter().find(|a| a.id == "prompt-commit").unwrap();
    let prompt_summary = catalog.iter().find(|a| a.id == "prompt-summary").unwrap();
    let agent_submit = catalog.iter().find(|a| a.id == "agent-submit").unwrap();
    let agent_prompt_submit = catalog
        .iter()
        .find(|a| a.id == "agent-prompt-submit")
        .unwrap();

    for (profile_name, expected_adapter, expected_bundle) in [
        ("Antigravity", "antigravity", "com.google.antigravity"),
        ("DeepSeek Coder (DSH)", "dsh-desktop", "com.deepseek.dsh"),
    ] {
        let pid = draft
            .document
            .editable
            .graph
            .profiles
            .iter()
            .find(|(_, p)| p.name == profile_name)
            .unwrap()
            .0
            .clone();
        let lid = draft.document.editable.graph.profiles[&pid].layer_order[0].clone();

        // Bind 4-way joystick sectors (slots 16..19 -> AG15..AG18) to Continue, Test, Review, Summary
        draft
            .assign(&json!(pid), &json!(lid), 16, prompt_continue.clone())
            .unwrap();
        draft
            .assign(&json!(pid), &json!(lid), 17, prompt_test.clone())
            .unwrap();
        draft
            .assign(&json!(pid), &json!(lid), 18, prompt_review.clone())
            .unwrap();
        draft
            .assign(&json!(pid), &json!(lid), 19, prompt_summary.clone())
            .unwrap();
        // Bind slot 9 (AG09) to Commit prompt and slot 12 (AG12) to explicit Submit control
        draft
            .assign(&json!(pid), &json!(lid), 9, prompt_commit.clone())
            .unwrap();
        draft
            .assign(&json!(pid), &json!(lid), 12, agent_submit.clone())
            .unwrap();

        for (binding, expected_text) in [
            ("AG15", "Continue"),
            ("AG16", "Run the tests and fix any failures"),
            ("AG17", "Review your diff for bugs"),
            ("AG18", "Summarize where things stand and what's left"),
            ("AG09", "Commit this work"),
        ] {
            let resolved = resolve_host_action(&draft.document, Some(&pid), binding, true)
                .unwrap_or_else(|| panic!("{profile_name} {binding} should resolve"));
            assert_eq!(
                resolved,
                ResolvedHostAction::PasteText {
                    text: expected_text.into(),
                    target_bundle_id: Some(expected_bundle.into()),
                    adapter: Some(expected_adapter.into()),
                    no_newline: true,
                    submit: false,
                },
                "{profile_name} {binding} must default to paste-only (no_newline=true, submit=false)"
            );
        }

        // Explicit submit key resolves to the active Desktop Agent's submit command
        assert_eq!(
            resolve_host_action(&draft.document, Some(&pid), "AG12", true),
            Some(ResolvedHostAction::AgentCommand {
                adapter: expected_adapter.into(),
                command: "submit".into(),
            })
        );

        // Explicit submit-prompt resolves only when explicitly bound
        draft
            .assign(&json!(pid), &json!(lid), 8, agent_prompt_submit.clone())
            .unwrap();
        assert_eq!(
            resolve_host_action(&draft.document, Some(&pid), "AG08", true),
            Some(ResolvedHostAction::AgentCommand {
                adapter: expected_adapter.into(),
                command: "submit-prompt:Continue".into(),
            })
        );
    }
}

#[test]
fn custom_app_profile_routes_open_app_targeted_shortcut_and_paste_text_to_bundle_id() {
    let mut draft = Draft::default();
    let custom_pid = draft.create_profile("Safari Workspace").unwrap();
    let custom_lid = draft.document.editable.graph.profiles[&custom_pid].layer_order[0].clone();

    draft
        .assign(
            &json!(custom_pid),
            &json!(custom_lid),
            0,
            Action {
                id: "custom-open-safari".into(),
                name: "打开 Safari".into(),
                category: "应用".into(),
                detail: "com.apple.Safari".into(),
                payload: serde_json::to_value(ActionKind::OpenApp {
                    bundle_id: "com.apple.Safari".into(),
                })
                .unwrap(),
            },
        )
        .unwrap();
    draft
        .assign(
            &json!(custom_pid),
            &json!(custom_lid),
            1,
            Action {
                id: "custom-safari-tab".into(),
                name: "新建标签页".into(),
                category: "快捷键".into(),
                detail: "com.apple.Safari".into(),
                payload: serde_json::to_value(ActionKind::Shortcut {
                    key: "t".into(),
                    modifiers: vec!["command".into()],
                    target: "com.apple.Safari".into(),
                })
                .unwrap(),
            },
        )
        .unwrap();
    draft
        .assign(
            &json!(custom_pid),
            &json!(custom_lid),
            2,
            Action {
                id: "custom-safari-snippet".into(),
                name: "粘贴地址".into(),
                category: "文本".into(),
                detail: "通用文本".into(),
                payload: serde_json::to_value(ActionKind::Text {
                    text: "https://developer.apple.com".into(),
                })
                .unwrap(),
            },
        )
        .unwrap();

    assert_eq!(
        resolve_host_action(&draft.document, Some(&custom_pid), "AG00", true),
        Some(ResolvedHostAction::OpenApp {
            bundle_id: "com.apple.Safari".into(),
        })
    );
    assert_eq!(
        resolve_host_action(&draft.document, Some(&custom_pid), "AG01", true),
        Some(ResolvedHostAction::Shortcut {
            key: "t".into(),
            key_code: 0x11,
            modifiers: vec!["command".into()],
            target_bundle_id: Some("com.apple.Safari".into()),
        })
    );
    assert_eq!(
        resolve_host_action(&draft.document, Some(&custom_pid), "AG02", true),
        Some(ResolvedHostAction::PasteText {
            text: "https://developer.apple.com".into(),
            target_bundle_id: Some("com.apple.Safari".into()),
            adapter: None,
            no_newline: true,
            submit: false,
        })
    );
}

#[test]
fn bottom_row_split_and_merge_layout_routing_and_persistence() {
    use mi_model::{
        resolve_host_action_with_layout, verified_physical_mapping_for_layout, BottomRowLayout,
    };

    let dir = std::env::temp_dir().join(format!("mi-bottom-row-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let doc_path = dir.join("workbench.json");

    let mut store = mi_model::storage::DocumentStore::default();
    store.open(doc_path.clone()).unwrap();
    let ids = mi_model::preset::install_agent_starter_profiles(&mut store.draft).unwrap();
    let ag_pid = ids
        .iter()
        .find(|pid| store.draft.document.editable.graph.profiles[*pid].name == "Antigravity")
        .unwrap()
        .clone();
    let herdr_pid = ids
        .iter()
        .find(|pid| store.draft.document.editable.graph.profiles[*pid].name == "Herdr")
        .unwrap()
        .clone();

    assert_eq!(store.bottom_row_layout(), BottomRowLayout::Split);
    store.set_host_active_profile(Some(ag_pid.clone())).unwrap();

    let ag10_split = verified_physical_mapping_for_layout("AG10", BottomRowLayout::Split).unwrap();
    assert_eq!(ag10_split.binding_id, "AG10");
    assert_eq!(ag10_split.logical_switch, "logical-10");
    assert_eq!(ag10_split.physical_control, "row3-col0-left");
    assert_eq!(ag10_split.keycap_group, "key-r3c0");
    assert_eq!(ag10_split.action_role, "momentary_key");

    let ag11_split = verified_physical_mapping_for_layout("AG11", BottomRowLayout::Split).unwrap();
    assert_eq!(ag11_split.binding_id, "AG11");
    assert_eq!(ag11_split.logical_switch, "logical-11");
    assert_eq!(ag11_split.physical_control, "row3-col1-middle");
    assert_eq!(ag11_split.keycap_group, "key-r3c1");
    assert_eq!(ag11_split.action_role, "momentary_key");

    let ag11_merged =
        verified_physical_mapping_for_layout("AG11", BottomRowLayout::Merged).unwrap();
    assert_eq!(ag11_merged.binding_id, "AG10");
    assert_eq!(ag11_merged.logical_switch, "logical-10");
    assert_eq!(ag11_merged.physical_control, "row3-wide-2u");
    assert_eq!(ag11_merged.keycap_group, "wide-bottom-2u");
    assert_eq!(ag11_merged.action_role, "wide_momentary_key");

    assert_eq!(
        store.resolve_host_input("AG07", true),
        Some(ResolvedHostAction::AgentCommand {
            adapter: "antigravity".into(),
            command: "focus-input".into(),
        })
    );
    assert_eq!(
        store.resolve_host_input("AG10", true),
        Some(ResolvedHostAction::AgentCommand {
            adapter: "antigravity".into(),
            command: "new-conversation-in-project".into(),
        })
    );
    assert_eq!(
        store.resolve_host_input("AG11", true),
        Some(ResolvedHostAction::AgentCommand {
            adapter: "antigravity".into(),
            command: "push-to-talk".into(),
        })
    );
    assert_eq!(
        store.resolve_host_input("AG12", true),
        Some(ResolvedHostAction::AgentCommand {
            adapter: "antigravity".into(),
            command: "submit".into(),
        })
    );

    assert!(resolve_host_action_with_layout(
        &store.draft.document,
        Some(&herdr_pid),
        "AG10",
        true,
        false,
        BottomRowLayout::Split
    )
    .is_some());
    assert!(resolve_host_action_with_layout(
        &store.draft.document,
        Some(&herdr_pid),
        "AG11",
        true,
        false,
        BottomRowLayout::Split
    )
    .is_none());

    let state_split = store.routing_state();
    assert_eq!(state_split["bottom_row_layout"], "split");
    assert_eq!(state_split["keys"].as_array().unwrap().len(), 20);

    let req: mi_model::storage::routing::RoutingRequest = serde_json::from_value(json!({
        "document_id": store.draft.document_id,
        "document_revision": store.draft.revision,
        "host_state_revision": store.host_revision(),
        "operation": {
            "kind": "bottom_row_layout",
            "layout": "merged"
        }
    }))
    .unwrap();
    store.update_routing(req).unwrap();
    assert_eq!(store.bottom_row_layout(), BottomRowLayout::Merged);

    let state_merged = store.routing_state();
    assert_eq!(state_merged["bottom_row_layout"], "merged");
    assert_eq!(state_merged["keys"].as_array().unwrap().len(), 19);
    assert_eq!(
        store.resolve_host_input("AG10", true),
        Some(ResolvedHostAction::AgentCommand {
            adapter: "antigravity".into(),
            command: "push-to-talk".into(),
        })
    );
    assert_eq!(
        store.resolve_host_input("AG11", true),
        Some(ResolvedHostAction::AgentCommand {
            adapter: "antigravity".into(),
            command: "push-to-talk".into(),
        })
    );
    assert!(resolve_host_action_with_layout(
        &store.draft.document,
        Some(&herdr_pid),
        "AG11",
        true,
        false,
        BottomRowLayout::Merged
    )
    .is_some());

    let mut reopened = mi_model::storage::DocumentStore::default();
    reopened.open(doc_path).unwrap();
    assert_eq!(reopened.bottom_row_layout(), BottomRowLayout::Merged);

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn canonical_recorded_shortcut_keys_resolve_through_host_router() {
    let mut draft = Draft::default();
    let pid = draft.document.editable.graph.profile_order[0].clone();
    let lid = draft.document.editable.graph.profiles[&pid].layer_order[0].clone();

    for (key, expected_code) in [
        ("left_bracket", 0x21),
        ("right_bracket", 0x1E),
        ("home", 0x73),
        ("page_up", 0x74),
        ("forward_delete", 0x75),
        ("end", 0x77),
        ("page_down", 0x79),
        ("fn", 0x3F),
    ] {
        draft
            .assign(
                &json!(pid),
                &json!(lid),
                0,
                Action {
                    id: format!("recorded-{key}"),
                    name: format!("Recorded {key}"),
                    category: "快捷键".into(),
                    detail: "test".into(),
                    payload: serde_json::to_value(ActionKind::Shortcut {
                        key: key.into(),
                        modifiers: vec!["command".into()],
                        target: "frontmost-editor".into(),
                    })
                    .unwrap(),
                },
            )
            .unwrap();

        assert_eq!(
            resolve_host_action(&draft.document, Some(&pid), "AG00", true),
            Some(ResolvedHostAction::Shortcut {
                key: key.into(),
                key_code: expected_code,
                modifiers: vec!["command".into()],
                target_bundle_id: None,
            })
        );
    }
}

#[test]
fn other_apps_general_profile_binds_only_ag11_apple_fn_and_serves_as_wildcard_follow_fallback() {
    use mi_model::{resolve_host_action_with_layout, BottomRowLayout};

    let mut store = DocumentStore::default();
    let ids = install_agent_starter_profiles(&mut store.draft).unwrap();
    let other_pid = ids
        .iter()
        .find(|pid| store.draft.document.editable.graph.profiles[*pid].name == "其他软件")
        .expect("其他软件 starter profile must be installed")
        .clone();
    let ag_pid = ids
        .iter()
        .find(|pid| store.draft.document.editable.graph.profiles[*pid].name == "Antigravity")
        .unwrap()
        .clone();

    let expected_fn = ResolvedHostAction::Shortcut {
        key: "fn".into(),
        key_code: 0x3F,
        modifiers: vec![],
        target_bundle_id: None,
    };

    // In default Split mode, ONLY AG11 (logical-11, bottom row middle/second microphone key) is bound.
    assert_eq!(
        resolve_host_action_with_layout(
            &store.draft.document,
            Some(&other_pid),
            "AG11",
            true,
            false,
            BottomRowLayout::Split
        ),
        Some(expected_fn.clone())
    );
    for unbound in [
        "AG00", "AG01", "AG02", "AG03", "AG04", "AG05", "AG06", "AG07", "AG08", "AG09", "AG10",
        "AG12", "AG13", "AG14", "ENC_CLK", "AG15", "AG16", "AG17", "AG18",
    ] {
        assert_eq!(
            resolve_host_action_with_layout(
                &store.draft.document,
                Some(&other_pid),
                unbound,
                true,
                false,
                BottomRowLayout::Split
            ),
            None,
            "其他软件 {unbound} must start unassigned so the user can freely map shortcuts"
        );
    }

    // In Merged 2U mode, both AG10 and AG11 fall back to logical-11 (Apple Fn).
    for wide_binding in ["AG10", "AG11"] {
        assert_eq!(
            resolve_host_action_with_layout(
                &store.draft.document,
                Some(&other_pid),
                wide_binding,
                true,
                false,
                BottomRowLayout::Merged
            ),
            Some(expected_fn.clone())
        );
    }

    // Wildcard "*" link routes any non-coding-agent app to "其他软件" while preserving explicit agent links.
    for op in [
        json!({"kind": "upsert", "bundle_id": "com.google.antigravity", "profile_id": ag_pid}),
        json!({"kind": "upsert", "bundle_id": "*", "profile_id": other_pid}),
        json!({"kind": "enable", "enabled": true}),
        json!({"kind": "follow", "bundle_id": "com.google.antigravity"}),
    ] {
        let req = serde_json::from_value(json!({
            "document_id": store.draft.document_id,
            "document_revision": store.draft.revision,
            "host_state_revision": store.host_revision(),
            "operation": op
        }))
        .unwrap();
        store.update_routing(req).unwrap();
    }
    assert_eq!(
        store.host_active_profile_id().as_deref(),
        Some(ag_pid.as_str())
    );

    let follow_safari = serde_json::from_value(json!({
        "document_id": store.draft.document_id,
        "document_revision": store.draft.revision,
        "host_state_revision": store.host_revision(),
        "operation": {"kind": "follow", "bundle_id": "com.apple.Safari"}
    }))
    .unwrap();
    store.update_routing(follow_safari).unwrap();
    assert_eq!(
        store.host_active_profile_id().as_deref(),
        Some(other_pid.as_str()),
        "Switching to an app outside the coding agents must follow into 其他软件"
    );
}

#[test]
fn generic_profile_routes_user_shortcut_after_reopen() {
    let mut draft = Draft::default();
    install_agent_starter_profiles(&mut draft).unwrap();
    let (profile, data) = draft
        .document
        .editable
        .graph
        .profiles
        .iter()
        .find(|(_, p)| p.name == "其他软件")
        .unwrap();
    let profile = profile.clone();
    let layer = data.layer_order[0].clone();
    let target = "com.example.editor";
    let modifiers = vec!["command".to_string()];
    draft
        .assign(
            &json!(profile),
            &json!(layer),
            0,
            Action {
                id: "custom-shortcut".into(),
                name: "Custom shortcut".into(),
                category: "test".into(),
                detail: String::new(),
                payload: serde_json::to_value(ActionKind::Shortcut {
                    key: "k".into(),
                    modifiers: modifiers.clone(),
                    target: target.into(),
                })
                .unwrap(),
            },
        )
        .unwrap();

    let bytes = serde_json::to_vec(&draft.document).unwrap();
    let reopened = Draft::from_bytes(&bytes).unwrap();
    assert_eq!(
        resolve_host_action(&reopened.document, Some(&profile), "AG00", true),
        Some(ResolvedHostAction::Shortcut {
            key: "k".into(),
            key_code: 0x28,
            modifiers,
            target_bundle_id: Some(target.into()),
        })
    );
}
