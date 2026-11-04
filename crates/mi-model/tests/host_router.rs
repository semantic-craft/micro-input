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
    assert_eq!(created.len(), 8);
    assert_eq!(
        draft.revision, 1,
        "install_agent_starter_profiles must commit as a single atomic revision"
    );
    draft.undo(false).unwrap();
    assert_eq!(
        draft.config.profiles.len(),
        2,
        "single undo must revert all 8 starter profiles"
    );
    draft.undo(true).unwrap();
    assert_eq!(draft.config.profiles.len(), 8);

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
    for pid in [ag_pid, dsh_pid] {
        for binding in ["AG10", "AG15", "ENC_CLK"] {
            assert!(resolve_host_action(&draft.document, Some(pid), binding, true).is_none());
        }
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
    assert_eq!(draft.config.profiles.len(), 8);
    assert!(!draft
        .config
        .profiles
        .iter()
        .any(|p| p.name.contains("演示") || p.name.contains("示例")));
    for (name, bundle) in [
        ("Cindy", "com.xd.cindy"),
        ("Codex", "com.openai.codex"),
        ("千问办公", "cn.qwenwork.desktop.mac"),
        ("ZCode", "dev.zcode.app"),
        ("Qoder CN", "com.qodercn.app"),
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
    assert_eq!(edited.config.profiles.len(), 9);
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
