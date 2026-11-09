use mi_bridge::*;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::{fs::PermissionsExt, net::UnixListener};

#[test]
fn cindy_navigation_scenario_satisfies_all_offline_gate_checks_and_audit_counts() {
    let dir = tempfile::Builder::new()
        .prefix("mi-cindy-qa-")
        .tempdir_in("/tmp")
        .unwrap();
    let audit_path = dir.path().join("audit.jsonl");
    let report = run_cindy_navigation_scenario(Some(&audit_path)).unwrap();
    assert_eq!(
        (
            report.status.as_str(),
            report.unauthorized_metadata_reads,
            report.local_agent_api_relaxed,
            report.native_app.as_str(),
            report.physical_key.as_str(),
        ),
        ("PASS", 0, false, "NOT_RUN", "NOT_RUN")
    );
    let expected = [
        "disabled_zero_reads",
        "scoped_catalog_only",
        "delivered_vs_confirmed_separated",
        "same_name_exact_identity",
        "selected_readback_matches",
        "wrong_selected_unconfirmed",
        "out_of_scope_zero_reads",
        "deleted_target_no_redirect",
        "account_or_generation_change_blocks_old_selection",
        "stop_bound_to_run_and_stale_noop",
        "resolve_bound_and_question_guard",
        "workspace_create_and_target_prompt_steering",
        "unread_tristate_and_pending_round",
    ];
    assert_eq!(report.checks.len(), expected.len());
    for name in expected {
        let item = report
            .checks
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("missing check: {name}"));
        assert_eq!(item.status, "PASS", "failed check {name}: {}", item.detail);
    }

    let summary: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join("cindy-navigation.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        (
            summary["scenario"].as_str(),
            summary["status"].as_str(),
            summary["unauthorized_metadata_reads"].as_u64(),
            summary["local_agent_api_relaxed"].as_bool(),
            summary["native_app"].as_str(),
            summary["physical_key"].as_str(),
        ),
        (
            Some("cindy-navigation"),
            Some("PASS"),
            Some(0),
            Some(false),
            Some("NOT_RUN"),
            Some("NOT_RUN"),
        )
    );
    let summary_checks = summary["checks"].as_array().unwrap();
    assert_eq!(summary_checks.len(), expected.len());
    for name in expected {
        let found = summary_checks
            .iter()
            .find(|c| c["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("missing summary check: {name}"));
        assert_eq!(found["status"].as_str(), Some("PASS"));
    }

    let records: Vec<Value> = std::fs::read_to_string(&audit_path)
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(records.len(), 27);
    assert!(records
        .iter()
        .all(|r| r["target"] == "cindy" && r.get("text_byte_len").is_none_or(Value::is_null)));

    for (act, out, exact) in [
        ("cindy_open", "delivered", 1),
        ("cindy_open", "confirmed", 5),
        ("cindy_open", "unconfirmed", 1),
        ("cindy_open", "stale", 2),
        ("cindy_stop", "stopped", 1),
        ("cindy_stop", "noop", 2),
        ("cindy_resolve", "resolved", 1),
        ("cindy_resolve", "interactionTypeMismatch", 1),
        ("cindy_resolve", "noop", 2),
        ("cindy_create", "confirmed", 1),
        ("cindy_send", "denied", 1),
        ("cindy_send", "started", 1),
        ("cindy_send", "steered", 1),
        ("cindy_send", "unknown", 2),
        ("cindy_query", "queued", 1),
        ("cindy_next_attention", "confirmed", 3),
        ("cindy_next_attention", "noop", 1),
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
fn cindy_grant_and_unix_client_enforce_permissions_scope_and_whitelists() {
    let dir = tempfile::Builder::new()
        .prefix("mi-cy-")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let (sock_path, grant_path) = (
        dir.path().join("cindy.sock"),
        dir.path().join("cindy-grant.json"),
    );
    let scope = CindyScope::new("/qa/ws");
    let g_val = json!({
        "version": 1,
        "socketPath": sock_path.to_str().unwrap(),
        "token": "0123456789abcdef0123456789abcdef",
        "instanceId": "cindy-inst-1",
        "generation": 3,
        "ownerEpoch": "owner-1:3",
        "scopes": [scope],
    });
    std::fs::write(&grant_path, serde_json::to_vec(&g_val).unwrap()).unwrap();
    std::fs::set_permissions(&grant_path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(CindyGrant::load(&grant_path), Err(CindyFailure::Grant));
    std::fs::set_permissions(&grant_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let loaded = CindyGrant::load(&grant_path).unwrap();
    assert_eq!(loaded.generation, 3);
    assert_eq!(loaded.owner_epoch, "owner-1:3");

    let listener = UnixListener::bind(&sock_path).unwrap();
    std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = std::thread::spawn(move || {
        for idx in 0..8 {
            let (mut s, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&mut s).read_line(&mut line).unwrap();
            let req: Value = serde_json::from_str(&line).unwrap();
            let id = req["id"].as_str().unwrap().to_owned();
            let addr = json!({"workspacePath": "/qa/ws", "sessionId": "sess-1"});
            let reply = match idx {
                0 => json!({
                    "version": 1,
                    "id": id,
                    "instanceId": "cindy-inst-1",
                    "generation": 3,
                    "ownerEpoch": "owner-1:3",
                    "status": "ok",
                    "selected": addr,
                    "rows": [{
                        "address": addr,
                        "title": "Task 1",
                        "phase": "needs-interaction",
                        "attention": true,
                        "pinned": false,
                        "runId": "run-1",
                        "unread": true,
                        "eventWatermark": 12,
                        "queuedCount": 0,
                        "pendingInteraction": {
                            "interactionId": "perm-1",
                            "interactionType": "permission",
                            "toolName": "bash"
                        }
                    }]
                }),
                1 => json!({
                    "version": 1,
                    "id": id,
                    "instanceId": "cindy-inst-1",
                    "generation": 3,
                    "ownerEpoch": "owner-1:3",
                    "status": "delivered",
                    "delivered": addr,
                    "deepLink": "cindy://session/sess-1"
                }),
                2 => json!({
                    "version": 1,
                    "id": id,
                    "instanceId": "cindy-inst-1",
                    "generation": 3,
                    "ownerEpoch": "owner-1:3",
                    "status": "confirmed",
                    "delivered": addr,
                    "selected": addr,
                    "deepLink": "cindy://session/sess-1"
                }),
                3 => json!({
                    "version": 1,
                    "id": id,
                    "instanceId": "cindy-inst-1",
                    "generation": 3,
                    "ownerEpoch": "owner-1:3",
                    "status": "confirmed",
                    "commandId": "cmd-create",
                    "delivered": addr,
                    "selected": addr,
                    "deepLink": "cindy://session/sess-1"
                }),
                4 => json!({
                    "version": 1,
                    "id": id,
                    "instanceId": "cindy-inst-1",
                    "generation": 3,
                    "ownerEpoch": "owner-1:3",
                    "status": "steered",
                    "commandId": "cmd-send",
                    "selected": addr,
                    "delivery": "steer",
                    "runId": "run-1",
                    "queuedCount": 0
                }),
                5 => json!({
                    "version": 1,
                    "id": id,
                    "instanceId": "cindy-inst-1",
                    "generation": 3,
                    "ownerEpoch": "owner-1:3",
                    "status": "stopped",
                    "commandId": "cmd-stop",
                    "selected": addr,
                    "runId": "run-1"
                }),
                6 => json!({
                    "version": 1,
                    "id": id,
                    "instanceId": "cindy-inst-1",
                    "generation": 3,
                    "ownerEpoch": "owner-1:3",
                    "status": "resolved",
                    "commandId": "cmd-resolve",
                    "selected": addr,
                    "interactionId": "perm-1",
                    "interactionType": "permission"
                }),
                _ => json!({
                    "version": 1,
                    "id": id,
                    "instanceId": "cindy-inst-1",
                    "generation": 3,
                    "ownerEpoch": "owner-1:3",
                    "status": "queued",
                    "commandId": "cmd-query",
                    "unexpectedKey": true
                }),
            };
            s.write_all(format!("{reply}\n").as_bytes()).unwrap();
        }
    });

    let client = CindySessionClient::new(&grant_path);
    let snap = client.connect().unwrap();
    assert_eq!(snap.rows.len(), 1);
    let sel = CindySelection {
        address: CindyTaskAddress::new("/qa/ws", "sess-1"),
        instance_id: "cindy-inst-1".into(),
        generation: 3,
        owner_epoch: "owner-1:3".into(),
        local_epoch: 1,
    };
    assert_eq!(client.open(&sel).status, CindyOpenResult::Delivered);
    assert_eq!(client.open(&sel).status, CindyOpenResult::Confirmed);
    assert_eq!(
        client
            .create(&CindyCreateIntent {
                command_id: "cmd-create".into(),
                scope: scope.clone(),
                text: None,
                instance_id: "cindy-inst-1".into(),
                generation: 3,
                owner_epoch: "owner-1:3".into(),
                local_epoch: 1,
            })
            .status,
        CindyCommandResult::Confirmed
    );
    assert_eq!(
        client
            .send(&CindySendIntent {
                command_id: "cmd-send".into(),
                selection: sel.clone(),
                text: "hello".into(),
                delivery_mode: CindyDeliveryMode::Steer,
            })
            .status,
        CindyCommandResult::Steered
    );
    assert_eq!(
        client
            .stop(&CindyStopIntent {
                command_id: "cmd-stop".into(),
                selection: sel.clone(),
                expected_run_id: "run-1".into(),
            })
            .status,
        CindyCommandResult::Stopped
    );
    assert_eq!(
        client
            .resolve(&CindyResolveIntent {
                command_id: "cmd-resolve".into(),
                selection: sel.clone(),
                interaction_id: "perm-1".into(),
                interaction_type: CindyInteractionType::Permission,
                decision: CindyInteractionDecision::Approve,
                option_id: None,
            })
            .status,
        CindyCommandResult::Resolved
    );
    let bad_reply = client.query(&CindyQueryIntent {
        command_id: "cmd-query".into(),
        scope,
        target: Some(sel.address),
        instance_id: "cindy-inst-1".into(),
        generation: 3,
        owner_epoch: "owner-1:3".into(),
        local_epoch: 1,
    });
    assert_eq!(bad_reply.status, CindyCommandResult::Unavailable);
    server.join().unwrap();
}
