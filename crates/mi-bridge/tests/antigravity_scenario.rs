use mi_bridge::*;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::{fs::PermissionsExt, net::UnixListener};

#[test]
fn antigravity_status_scenario_satisfies_all_offline_gate_checks_and_audit_counts() {
    let dir = tempfile::Builder::new()
        .prefix("mi-ag-qa-")
        .tempdir_in("/tmp")
        .unwrap();
    let audit_path = dir.path().join("audit.jsonl");
    let report = run_antigravity_status_scenario(Some(&audit_path)).unwrap();
    assert_eq!(
        (
            report.status.as_str(),
            report.installed_version.as_str(),
            report.language_server_cl.as_str(),
            report.transcript_reads,
            report.token_reads,
            report.native_app.as_str(),
            report.physical_key.as_str(),
        ),
        ("PASS", "2.29.0", "1013060670", 0, 0, "NOT_RUN", "NOT_RUN")
    );

    let expected = [
        "version_and_capabilities_recorded",
        "disabled_zero_reads",
        "existing_two_sessions_catalog_and_selected_readback_confirmed",
        "same_name_cross_workspace_no_collision",
        "wrong_selected_readback_unconfirmed",
        "two_sessions_isolated",
        "duplicate_events_deduplicated",
        "stop_fully_idle_false_not_completed",
        "stop_fully_idle_true_completed",
        "create_binds_project_id_and_confirms_selected",
        "two_sessions_send_only_modifies_target",
        "running_session_send_steering_verified_separately",
        "missing_or_deleted_target_send_zero_redirect",
        "timeout_unknown_reconciles_without_resend",
        "stop_selected_session_only_and_stale_noop",
        "resolve_interaction_and_question_guard",
        "switch_model_and_thinking_guard",
        "deleted_target_no_redirect",
        "missing_schema_or_auth_fails_closed",
        "l3_schema_or_csrf_invalidation_fails_closed",
        "stream_disconnect_marks_stale",
        "generation_change_marks_stale",
        "journal_gap_marks_unavailable",
        "zero_transcript_reads",
        "zero_token_reads",
        "bridge_subscription_closed",
    ];
    assert_eq!(report.checks.len(), expected.len());
    for name in expected {
        assert_eq!(
            report.checks.get(name),
            Some(&true),
            "check {name} did not pass"
        );
    }

    for doc_name in [
        "antigravity-status.json",
        "antigravity-navigation.json",
        "antigravity-agentapi.json",
        "antigravity-l3-control.json",
    ] {
        let doc: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.path().join(doc_name)).unwrap())
                .unwrap();
        assert_eq!(
            (
                doc["status"].as_str(),
                doc["installed_version"].as_str(),
                doc["language_server_cl"].as_str(),
                doc["transcript_reads"].as_u64(),
                doc["token_reads"].as_u64(),
                doc["native_app"].as_str(),
                doc["physical_key"].as_str(),
            ),
            (
                Some("PASS"),
                Some("2.29.0"),
                Some("1013060670"),
                Some(0),
                Some(0),
                Some("NOT_RUN"),
                Some("NOT_RUN"),
            ),
            "unexpected report payload in {doc_name}"
        );
        let doc_checks = doc["checks"].as_object().unwrap();
        assert_eq!(doc_checks.len(), expected.len());
        for name in expected {
            assert_eq!(
                doc_checks.get(name).and_then(Value::as_bool),
                Some(true),
                "missing or failed check {name} in {doc_name}"
            );
        }
    }

    let records: Vec<Value> = std::fs::read_to_string(&audit_path)
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(records.iter().all(|r| {
        r["target"] == "antigravity" && r.get("text_byte_len").is_none_or(Value::is_null)
    }));

    for (act, out, exact) in [
        ("antigravity_open", "confirmed", 5),
        ("antigravity_open", "unconfirmed", 2),
        ("antigravity_open", "denied", 1),
        ("antigravity_create", "created", 1),
        ("antigravity_create", "unconfirmed", 1),
        ("antigravity_send", "started", 1),
        ("antigravity_send", "queued", 1),
        ("antigravity_send", "steered", 1),
        ("antigravity_send", "unconfirmed", 2),
        ("antigravity_send", "unknown", 1),
        ("antigravity_query", "queued", 1),
        ("antigravity_stop", "stopped", 1),
        ("antigravity_stop", "noop", 1),
        ("antigravity_stop", "unconfirmed", 1),
        ("antigravity_stop", "denied", 1),
        ("antigravity_resolve", "resolved", 3),
        ("antigravity_resolve", "unconfirmed", 1),
        ("antigravity_resolve", "noop", 1),
        ("antigravity_switch_model", "modelSwitched", 2),
        ("antigravity_switch_model", "unconfirmed", 2),
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
    assert_eq!(records.len(), 30);
}

#[test]
fn antigravity_grant_and_unix_client_enforce_permissions_whitelists_and_zero_reads() {
    let dir = tempfile::Builder::new()
        .prefix("mi-ag-")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let (sock_path, grant_path) = (
        dir.path().join("antigravity.sock"),
        dir.path().join("antigravity-grant.json"),
    );
    let caps = AntigravityCapabilities::installed_2_29_0();
    let g_val = json!({
        "version": 1,
        "socketPath": sock_path.to_str().unwrap(),
        "token": "0123456789abcdef0123456789abcdef",
        "instanceId": "ag-inst-1",
        "generation": 1,
        "allowedWorkspaces": ["/workspace/ag-alpha"],
        "capabilities": caps,
    });
    std::fs::write(&grant_path, serde_json::to_vec(&g_val).unwrap()).unwrap();
    std::fs::set_permissions(&grant_path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        AntigravityGrant::load(&grant_path),
        Err(AntigravityFailure::Grant)
    );
    std::fs::set_permissions(&grant_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let loaded = AntigravityGrant::load(&grant_path).unwrap();
    assert_eq!(loaded.generation, 1);
    assert_eq!(loaded.capabilities.installed_version, "2.29.0");
    assert_eq!(loaded.capabilities.language_server_cl, "1013060670");

    let listener = UnixListener::bind(&sock_path).unwrap();
    std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = std::thread::spawn(move || {
        for idx in 0..9 {
            let (mut s, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&mut s).read_line(&mut line).unwrap();
            let req: Value = serde_json::from_str(&line).unwrap();
            let id = req["id"].as_str().unwrap().to_owned();
            let sub_id = req["subscriptionId"].as_str().unwrap().to_owned();
            let sel =
                json!({"conversationId": "conv-alpha", "workspacePath": "/workspace/ag-alpha"});
            let row = json!({
                "conversationId": "conv-alpha",
                "workspacePath": "/workspace/ag-alpha",
                "title": "Alpha Session",
                "projectId": "proj-alpha",
                "modelName": "gemini-2.5-pro",
                "thinkingLevel": "high",
                "phase": "running",
                "lastEventType": "PreInvocation",
                "invocationNum": 1,
                "stepIdx": 0,
                "executionNum": 1,
                "fullyIdle": false,
                "transitionCount": 1
            });
            let reply = match idx {
                0 => {
                    assert_eq!(req["op"].as_str(), Some("subscribe"));
                    assert_eq!(req["activate"].as_bool(), Some(false));
                    json!({
                        "version": 1,
                        "subscriptionId": sub_id,
                        "id": id,
                        "instanceId": "ag-inst-1",
                        "generation": 1,
                        "status": "ok",
                        "capabilities": AntigravityCapabilities::installed_2_29_0(),
                        "cursor": 1,
                        "rows": [row],
                        "changes": [],
                        "selected": sel,
                        "transcriptReads": 0,
                        "tokenReads": 0
                    })
                }
                1 => {
                    assert_eq!(req["op"].as_str(), Some("open"));
                    assert_eq!(
                        req["schemaVersion"].as_str(),
                        Some("antigravity-catalog-nav-v1")
                    );
                    json!({
                        "version": 1,
                        "subscriptionId": sub_id,
                        "id": id,
                        "instanceId": "ag-inst-1",
                        "generation": 1,
                        "status": "ok",
                        "selected": sel,
                        "transcriptReads": 0,
                        "tokenReads": 0
                    })
                }
                2 => {
                    assert_eq!(req["op"].as_str(), Some("create"));
                    assert_eq!(
                        req["schemaVersion"].as_str(),
                        Some("antigravity-agentapi-v1")
                    );
                    json!({
                        "version": 1,
                        "subscriptionId": sub_id,
                        "id": id,
                        "instanceId": "ag-inst-1",
                        "generation": 1,
                        "status": "created",
                        "commandOp": "create",
                        "requestId": "req-create-1",
                        "conversationId": "conv-alpha",
                        "workspacePath": "/workspace/ag-alpha",
                        "projectId": "proj-alpha",
                        "delivery": "started",
                        "inFlightSteering": false,
                        "reasonCode": "agentapi.newConversationConfirmed",
                        "duplicate": false,
                        "reconciled": false,
                        "cursor": 2,
                        "rows": [row],
                        "selected": sel,
                        "transcriptReads": 0,
                        "tokenReads": 0
                    })
                }
                3 => {
                    assert_eq!(req["op"].as_str(), Some("send"));
                    assert_eq!(
                        req["schemaVersion"].as_str(),
                        Some("antigravity-agentapi-v1")
                    );
                    json!({
                        "version": 1,
                        "subscriptionId": sub_id,
                        "id": id,
                        "instanceId": "ag-inst-1",
                        "generation": 1,
                        "status": "steered",
                        "commandOp": "send",
                        "requestId": "req-send-1",
                        "conversationId": "conv-alpha",
                        "workspacePath": "/workspace/ag-alpha",
                        "projectId": "proj-alpha",
                        "delivery": "steered",
                        "inFlightSteering": true,
                        "reasonCode": "agentapi.sendMessageSteered",
                        "duplicate": false,
                        "reconciled": false,
                        "cursor": 3,
                        "rows": [row],
                        "selected": sel,
                        "transcriptReads": 0,
                        "tokenReads": 0
                    })
                }
                4 => {
                    assert_eq!(req["op"].as_str(), Some("stop"));
                    assert_eq!(req["schemaVersion"].as_str(), Some("antigravity-stop-v1"));
                    json!({
                        "version": 1,
                        "subscriptionId": sub_id,
                        "id": id,
                        "instanceId": "ag-inst-1",
                        "generation": 1,
                        "status": "stopped",
                        "commandOp": "stop",
                        "requestId": "req-stop-1",
                        "conversationId": "conv-alpha",
                        "workspacePath": "/workspace/ag-alpha",
                        "projectId": "proj-alpha",
                        "delivery": "canceled",
                        "inFlightSteering": false,
                        "reasonCode": "languageServer.cancelCascadeInvocationConfirmed",
                        "duplicate": false,
                        "reconciled": false,
                        "cursor": 4,
                        "rows": [row],
                        "selected": sel,
                        "transcriptReads": 0,
                        "tokenReads": 0
                    })
                }
                5 => {
                    assert_eq!(req["op"].as_str(), Some("resolve"));
                    assert_eq!(
                        req["schemaVersion"].as_str(),
                        Some("antigravity-interaction-v1")
                    );
                    json!({
                        "version": 1,
                        "subscriptionId": sub_id,
                        "id": id,
                        "instanceId": "ag-inst-1",
                        "generation": 1,
                        "status": "resolved",
                        "commandOp": "resolve",
                        "requestId": "req-resolve-1",
                        "conversationId": "conv-alpha",
                        "workspacePath": "/workspace/ag-alpha",
                        "projectId": "proj-alpha",
                        "delivery": "allow",
                        "inFlightSteering": false,
                        "reasonCode": "languageServer.handleCascadeUserInteractionConfirmed",
                        "duplicate": false,
                        "reconciled": false,
                        "cursor": 5,
                        "rows": [row],
                        "selected": sel,
                        "transcriptReads": 0,
                        "tokenReads": 0
                    })
                }
                6 => {
                    assert_eq!(req["op"].as_str(), Some("switch-model"));
                    assert_eq!(
                        req["schemaVersion"].as_str(),
                        Some("antigravity-model-config-v1")
                    );
                    json!({
                        "version": 1,
                        "subscriptionId": sub_id,
                        "id": id,
                        "instanceId": "ag-inst-1",
                        "generation": 1,
                        "status": "modelSwitched",
                        "commandOp": "switch-model",
                        "requestId": "req-switch-1",
                        "conversationId": "conv-alpha",
                        "workspacePath": "/workspace/ag-alpha",
                        "projectId": "proj-alpha",
                        "delivery": "modelSwitched",
                        "inFlightSteering": false,
                        "reasonCode": "languageServer.cascadeModelConfigConfirmed",
                        "duplicate": false,
                        "reconciled": false,
                        "cursor": 6,
                        "rows": [row],
                        "selected": sel,
                        "transcriptReads": 0,
                        "tokenReads": 0
                    })
                }
                7 => json!({
                    "version": 1,
                    "subscriptionId": sub_id,
                    "id": id,
                    "instanceId": "ag-inst-1",
                    "generation": 1,
                    "status": "queued",
                    "commandOp": "query",
                    "requestId": "req-query-bad",
                    "unexpectedKey": true
                }),
                _ => json!({
                    "version": 1,
                    "subscriptionId": sub_id,
                    "id": id,
                    "instanceId": "ag-inst-1",
                    "generation": 1,
                    "status": "ok"
                }),
            };
            s.write_all(format!("{reply}\n").as_bytes()).unwrap();
        }
    });

    let client = AntigravityStatusClient::new(&grant_path);
    let snap = client.connect().unwrap();
    assert_eq!(snap.rows.len(), 1);
    assert_eq!(snap.transcript_reads, 0);
    assert_eq!(snap.token_reads, 0);

    let key = AntigravitySessionKey::new("conv-alpha", "/workspace/ag-alpha");
    let open_reply = client.open(&key).unwrap();
    assert_eq!(open_reply.status, "ok");
    assert_eq!(open_reply.selected, Some(key.clone()));

    let create_reply = client
        .create(&AntigravityCreateIntent {
            request_id: "req-create-1".into(),
            workspace_path: "/workspace/ag-alpha".into(),
            project_id: "proj-alpha".into(),
            prompt: "hello".into(),
            title: Some("Alpha Session".into()),
            model_name: Some("gemini-2.5-pro".into()),
            conversation_id: Some("conv-alpha".into()),
        })
        .unwrap();
    assert_eq!(create_reply.status, "created");

    let send_reply = client
        .send(&AntigravitySendIntent {
            request_id: "req-send-1".into(),
            target: key.clone(),
            project_id: Some("proj-alpha".into()),
            message: "steer msg".into(),
            delivery_mode: AntigravityDeliveryMode::Steer,
        })
        .unwrap();
    assert_eq!(send_reply.status, "steered");
    assert!(send_reply.in_flight_steering);

    let stop_reply = client
        .stop(&AntigravityStopIntent {
            request_id: "req-stop-1".into(),
            target: key.clone(),
            expected_invocation_num: 1,
            expected_execution_num: Some(1),
            kill_background_tasks: true,
        })
        .unwrap();
    assert_eq!(stop_reply.status, "stopped");

    let resolve_reply = client
        .resolve(&AntigravityResolveIntent {
            request_id: "req-resolve-1".into(),
            target: key.clone(),
            interaction_id: "int-1".into(),
            interaction_type: AntigravityInteractionType::Permission,
            decision: AntigravityInteractionDecision::Allow,
            selected_option_ids: Vec::new(),
        })
        .unwrap();
    assert_eq!(resolve_reply.status, "resolved");

    let switch_reply = client
        .switch_model(&AntigravitySwitchModelIntent {
            request_id: "req-switch-1".into(),
            target: key.clone(),
            model_name: "gemini-2.5-pro".into(),
            thinking_level: "high".into(),
        })
        .unwrap();
    assert_eq!(switch_reply.status, "modelSwitched");

    let bad_query = client.query(&AntigravityQueryIntent {
        request_id: "req-query-bad".into(),
        expected_op: Some("send".into()),
        conversation_id: Some("conv-alpha".into()),
        workspace_path: Some("/workspace/ag-alpha".into()),
        project_id: Some("proj-alpha".into()),
    });
    assert_eq!(bad_query, Err(AntigravityFailure::InvalidReply));
    client.close();
    server.join().unwrap();
}
