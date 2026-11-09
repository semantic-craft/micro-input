use mi_bridge::*;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::{fs::PermissionsExt, net::UnixListener};

#[test]
fn dsh_navigation_scenario_satisfies_all_offline_gate_checks_and_audit_counts() {
    let dir = tempfile::Builder::new()
        .prefix("mi-dsh-qa-")
        .tempdir_in("/tmp")
        .unwrap();
    let audit_path = dir.path().join("audit.jsonl");
    let report = run_dsh_navigation_scenario(Some(&audit_path)).unwrap();
    assert_eq!(
        (
            report.status.as_str(),
            report.body_reads,
            report.native_app.as_str(),
            report.physical_key.as_str()
        ),
        ("PASS", 0, "NOT_RUN", "NOT_RUN")
    );
    let expected = [
        "disabled_zero_reads",
        "same_name_exact_identity",
        "cold_target_unavailable",
        "pending_only_transitions_retained",
        "selected_id_readback",
        "duplicate_request_no_double_send",
        "queue_and_steer_confirmed_separately",
        "switch_session_no_misstop",
        "same_session_stale_stop_zero_cancel",
        "cancel_queue_remaining_not_all_stopped",
        "unprovable_turn_atomicity_refused",
        "create_workspace_bound_readback",
        "resolve_approval_recorded",
        "already_resolved_noop_zero_side_effect",
        "interaction_type_mismatch_blocked",
        "resolve_question_answered",
        "select_model_readback_confirmed",
        "unsupported_thought_level_locally_refused",
        "wrong_selected_unconfirmed",
        "out_of_scope_zero_reads",
        "deleted_target_no_redirect",
        "host_generation_rejects_old_selection",
        "journal_gap_clears_targets",
        "bridge_subscription_closed",
    ];
    assert_eq!(report.checks.len(), expected.len());
    for k in expected {
        assert_eq!(report.checks.get(k), Some(&true), "failed check: {k}");
    }

    let summary: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join("dsh-navigation.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        (summary["status"].as_str(), summary["body_reads"].as_u64()),
        (Some("PASS"), Some(0))
    );

    let records: Vec<Value> = std::fs::read_to_string(&audit_path)
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(records.len(), 24);
    assert!(records
        .iter()
        .all(|r| r["target"] == "dsh" && r.get("text_byte_len").is_none_or(Value::is_null)));
    for (act, out, exact) in [
        ("dsh_open", "confirmed", 6),
        ("dsh_open", "unconfirmed", 1),
        ("dsh_open", "stale", 2),
        ("dsh_open", "denied", 1),
        ("dsh_prompt", "steered", 2),
        ("dsh_prompt", "queued", 1),
        ("dsh_cancel", "unconfirmed", 1),
        ("dsh_cancel", "noop", 1),
        ("dsh_cancel", "cancelledQueueRemaining", 1),
        ("dsh_cancel", "denied", 1),
        ("dsh_create", "confirmed", 1),
        ("dsh_resolve", "resolved", 3),
        ("dsh_resolve", "interactionTypeMismatch", 1),
        ("dsh_switch_model", "modelSwitched", 1),
        ("dsh_switch_model", "unsupportedThought", 1),
    ] {
        assert_eq!(
            records
                .iter()
                .filter(|r| r["action_type"] == act && r["outcome"] == out)
                .count(),
            exact,
            "mismatched count for {act}:{out}"
        );
    }
}

#[test]
fn dsh_grant_and_unix_client_enforce_permissions_passive_subscribe_and_whitelists() {
    let dir = tempfile::Builder::new()
        .prefix("mi-dsh-")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let (sock_path, grant_path) = (
        dir.path().join("dsh.sock"),
        dir.path().join("dsh-grant.json"),
    );
    let g_val = json!({
        "version": 1,
        "socketPath": sock_path.to_str().unwrap(),
        "token": "0123456789abcdef0123456789abcdef",
        "hostId": "host-1",
        "generation": 5,
        "sessionIds": ["s1"]
    });
    std::fs::write(&grant_path, serde_json::to_vec(&g_val).unwrap()).unwrap();
    std::fs::set_permissions(&grant_path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(DSHGrant::load(&grant_path), Err(DSHFailure::Grant));
    std::fs::set_permissions(&grant_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(DSHGrant::load(&grant_path).unwrap().generation, 5);

    let listener = UnixListener::bind(&sock_path).unwrap();
    std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = std::thread::spawn(move || {
        for idx in 0..7 {
            let (mut s, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&mut s).read_line(&mut line).unwrap();
            let req: Value = serde_json::from_str(&line).unwrap();
            let id = req["id"].as_str().unwrap().to_owned();
            let sub_id = req["subscriptionId"].as_str().unwrap().to_owned();
            let cfg = json!({"provider": "deepseek-official", "model": "deepseek-chat", "thought": "high"});
            let reply = match idx {
                0 => {
                    assert_eq!(req["op"], "subscribe");
                    assert_eq!(req["activate"], json!(false));
                    json!({
                        "version": 1,
                        "subscriptionId": sub_id,
                        "id": id,
                        "hostId": "host-1",
                        "generation": 5,
                        "status": "ok",
                        "cursor": 1,
                        "rows": [{
                            "sessionId": "s1",
                            "workspaceId": "ws-1",
                            "title": "S1",
                            "active": true,
                            "running": true,
                            "pendingInput": 1,
                            "currentTurn": 2,
                            "queuedCount": 0,
                            "pendingInteraction": {"interactionId": "i1", "interactionType": "question"},
                            "modelConfig": cfg
                        }],
                        "unavailableSessionIds": [],
                        "changes": [],
                        "selected": "s1"
                    })
                }
                1 => {
                    assert_eq!(req["op"], "open");
                    assert!(req.get("activate").is_none_or(Value::is_null));
                    json!({
                        "version": 1,
                        "subscriptionId": sub_id,
                        "id": id,
                        "hostId": "host-1",
                        "generation": 5,
                        "status": "confirmed",
                        "selected": "s1"
                    })
                }
                2 => {
                    assert_eq!(req["op"], "prompt");
                    assert_eq!(req["mode"], "steer");
                    json!({
                        "version": 1,
                        "subscriptionId": sub_id,
                        "id": id,
                        "hostId": "host-1",
                        "generation": 5,
                        "status": "steered",
                        "sessionId": "s1",
                        "workspaceId": "ws-1",
                        "selected": "s1",
                        "requestId": "r-p1",
                        "delivery": "steer",
                        "currentTurn": 2,
                        "queuedCount": 1
                    })
                }
                3 => {
                    assert_eq!(req["op"], "cancel");
                    assert_eq!(req["expectedTurn"], 2);
                    json!({
                        "version": 1,
                        "subscriptionId": sub_id,
                        "id": id,
                        "hostId": "host-1",
                        "generation": 5,
                        "status": "cancelledQueueRemaining",
                        "sessionId": "s1",
                        "workspaceId": "ws-1",
                        "selected": "s1",
                        "requestId": "r-c1",
                        "expectedTurn": 2,
                        "queuedCount": 1
                    })
                }
                4 => {
                    assert_eq!(req["op"], "resolve");
                    json!({
                        "version": 1,
                        "subscriptionId": sub_id,
                        "id": id,
                        "hostId": "host-1",
                        "generation": 5,
                        "status": "interactionTypeMismatch",
                        "sessionId": "s1",
                        "workspaceId": "ws-1",
                        "selected": "s1",
                        "requestId": "r-res1",
                        "reasonCode": "guard.questionCannotApproveAsPermission"
                    })
                }
                5 => {
                    assert_eq!(req["op"], "selectModel");
                    json!({
                        "version": 1,
                        "subscriptionId": sub_id,
                        "id": id,
                        "hostId": "host-1",
                        "generation": 5,
                        "status": "modelSwitched",
                        "sessionId": "s1",
                        "workspaceId": "ws-1",
                        "selected": "s1",
                        "requestId": "r-m1",
                        "modelConfig": cfg
                    })
                }
                _ => {
                    json!({
                        "version": 1,
                        "subscriptionId": sub_id,
                        "id": id,
                        "hostId": "host-1",
                        "generation": 5,
                        "status": "confirmed",
                        "unexpectedKey": true
                    })
                }
            };
            s.write_all(format!("{reply}\n").as_bytes()).unwrap();
        }
    });

    let client = DSHSessionClient::new(&grant_path);
    assert_eq!(client.connect().unwrap().rows.len(), 1);
    let sel = DSHSelection {
        session_id: "s1".into(),
        workspace_id: "ws-1".into(),
        host_id: "host-1".into(),
        generation: 5,
        local_epoch: 1,
    };
    assert_eq!(client.open(&sel).unwrap().status, DSHOpenResult::Confirmed);
    let pr = client
        .prompt(&DSHPromptIntent {
            session_id: "s1".into(),
            workspace_id: "ws-1".into(),
            request_id: "r-p1".into(),
            mode: DSHPromptMode::Steer,
            text: "steer".into(),
            host_id: "host-1".into(),
            generation: 5,
            local_epoch: 1,
        })
        .unwrap();
    assert_eq!(pr.status, "steered");
    let ca = client
        .cancel(&DSHCancelIntent {
            session_id: "s1".into(),
            workspace_id: "ws-1".into(),
            request_id: "r-c1".into(),
            expected_turn: 2,
            host_id: "host-1".into(),
            generation: 5,
            local_epoch: 1,
        })
        .unwrap();
    assert_eq!(ca.status, "cancelledQueueRemaining");
    let rs = client
        .resolve(&DSHResolveIntent {
            session_id: "s1".into(),
            workspace_id: "ws-1".into(),
            request_id: "r-res1".into(),
            interaction_id: "i1".into(),
            interaction_type: DSHInteractionType::Permission,
            decision: DSHInteractionDecision::Approve,
            option_id: None,
            host_id: "host-1".into(),
            generation: 5,
            local_epoch: 1,
        })
        .unwrap();
    assert_eq!(rs.status, "interactionTypeMismatch");
    let sm = client
        .switch_model(&DSHSwitchModelIntent {
            session_id: "s1".into(),
            workspace_id: "ws-1".into(),
            request_id: "r-m1".into(),
            provider: "deepseek-official".into(),
            model: "deepseek-chat".into(),
            thought: Some("high".into()),
            host_id: "host-1".into(),
            generation: 5,
            local_epoch: 1,
        })
        .unwrap();
    assert_eq!(sm.status, "modelSwitched");
    assert_eq!(client.open(&sel), Err(DSHFailure::InvalidReply));
    server.join().unwrap();
}
