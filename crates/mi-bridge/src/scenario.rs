use crate::lighting_arbiter::{FourAppAgentPhase, FourAppLightingArbiter, LightingUpdateResult};
use crate::ownership::{
    NativeOwnershipController, NativeOwnershipReply, NativeOwnershipRequest,
    UnixNativeOwnershipTransport,
};
use crate::voice::{FourAppVoiceManager, PushToTalkController};
use mi_model::control_settings::ControlSettings;
use mi_model::draft::{actions, Draft};
use mi_model::host_router::{resolve_host_action_with_layout, BottomRowLayout, ResolvedHostAction};
use mi_model::lighting::{color_from_hex, Edit, Zone};
use mi_model::navigation_history::NavigationPlace;
use mi_model::preset::{export, install_agent_starter_profiles};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostActionReceipt {
    pub binding_id: String,
    pub profile_id: String,
    pub action: ResolvedHostAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeOwnershipScenarioReport {
    pub status: String,
    pub checks: BTreeMap<String, bool>,
    pub receipt_count: usize,
    pub receipts: Vec<HostActionReceipt>,
    pub native_app: String,
    pub physical_key: String,
    pub lighting_visual: String,
}

fn find_profile_id(draft: &Draft, name: &str) -> Result<String, String> {
    draft
        .document
        .editable
        .graph
        .profiles
        .iter()
        .find(|(_, p)| p.name == name)
        .map(|(id, _)| id.clone())
        .ok_or_else(|| format!("missing profile: {name}"))
}

fn set_host_active_profile(
    draft: &Draft,
    ownership: &mut NativeOwnershipController,
    profile_id: &str,
) -> Result<String, String> {
    let profile = draft
        .document
        .editable
        .graph
        .profiles
        .get(profile_id)
        .ok_or_else(|| format!("missing profile id: {profile_id}"))?;
    let desired = if matches!(profile.name.as_str(), "Cindy" | "Codex") {
        "native"
    } else {
        "micro-input"
    };
    if ownership.desired_owner() != desired {
        ownership.set_desired_owner(desired);
    } else {
        ownership.invalidate("切换配置重新确认", desired == "micro-input");
    }
    Ok(profile_id.to_owned())
}

fn synthetic_reply(
    request: &NativeOwnershipRequest,
    instance: Uuid,
    state: &str,
) -> NativeOwnershipReply {
    NativeOwnershipReply {
        version: 1,
        request_id: request.request_id,
        client_instance: request.client_instance,
        generation: request.generation,
        native_instance: instance,
        native_generation: 1,
        state: state.to_owned(),
        reason: "synthetic peer".into(),
        input_drained: true,
        ptt_drained: true,
        lighting_drained: true,
        capabilities: Some(vec!["fenced-release-v1".into()]),
    }
}

fn emit_synthetic_control(
    draft: &Draft,
    active_profile_id: &str,
    ownership: &NativeOwnershipController,
    kind: &str,
    receipts: &mut Vec<HostActionReceipt>,
) {
    if !ownership.allows_execution() {
        return;
    }
    let events: &[(&str, bool)] = if kind == "wide" {
        &[
            ("AG10", true),
            ("AG11", true),
            ("AG10", false),
            ("AG11", false),
        ]
    } else {
        &[("AG14", true), ("AG14", false)]
    };
    let mut wide_latched = false;
    for &(binding_id, pressed) in events {
        if kind == "wide" && matches!(binding_id, "AG10" | "AG11") {
            if pressed {
                if wide_latched {
                    continue;
                }
                wide_latched = true;
            } else {
                wide_latched = false;
                continue;
            }
        }
        if let Some(action) = resolve_host_action_with_layout(
            &draft.document,
            Some(active_profile_id),
            binding_id,
            pressed,
            true,
            BottomRowLayout::Merged,
        ) {
            receipts.push(HostActionReceipt {
                binding_id: binding_id.to_owned(),
                profile_id: active_profile_id.to_owned(),
                action,
            });
        }
    }
}

pub async fn run_native_ownership_scenario(
    peer_socket: Option<&Path>,
    audit_log_path: Option<&Path>,
) -> Result<NativeOwnershipScenarioReport, String> {
    let mut draft = Draft::default();
    install_agent_starter_profiles(&mut draft)?;

    let herdr_id = find_profile_id(&draft, "Herdr")?;
    let antigravity_id = find_profile_id(&draft, "Antigravity")?;
    let cindy_id = find_profile_id(&draft, "Cindy")?;
    let codex_id = find_profile_id(&draft, "Codex")?;
    let dsh_id = find_profile_id(&draft, "DeepSeek Coder (DSH)")?;

    let herdr_layer_id = draft.document.editable.graph.profiles[&herdr_id].layer_order[0].clone();
    let search_action = actions()
        .into_iter()
        .find(|a| a.id == "search")
        .ok_or_else(|| "missing search action".to_owned())?;
    draft.assign(
        &json!(herdr_id),
        &json!(herdr_layer_id),
        14,
        search_action.clone(),
    )?;

    let transport = peer_socket.map(UnixNativeOwnershipTransport::new);
    let mut ownership = NativeOwnershipController::new(transport.clone(), false);
    let mut voice = FourAppVoiceManager::new();
    let mut ptt = PushToTalkController::new_simulated();
    let mut lighting = FourAppLightingArbiter::new();
    let peer_instance = Uuid::new_v4();
    let mut receipts = Vec::new();
    let mut checks = BTreeMap::new();
    let mut paired_events: Vec<Value> = Vec::new();

    let control_exchange = |payload: Value| -> Result<Value, String> {
        let Some(sock) = peer_socket else {
            return Ok(json!({}));
        };
        let mut ctrl = sock.as_os_str().to_os_string();
        ctrl.push(".control");
        let ctrl_transport = UnixNativeOwnershipTransport::new(ctrl);
        let bytes = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;
        let raw = ctrl_transport
            .exchange_blocking(&bytes)
            .map_err(|e| e.to_string())?;
        serde_json::from_slice(&raw).map_err(|e| e.to_string())
    };

    let mut active_profile = set_host_active_profile(&draft, &mut ownership, &herdr_id)?;

    let exchange_state =
        |ownership: &mut NativeOwnershipController, state: &str| -> Result<(), String> {
            ownership.set_desired_owner(if state == "native" {
                "native"
            } else {
                "micro-input"
            });
            let req = ownership.begin_request(true);
            let reply = if let Some(ref t) = transport {
                t.exchange(&req).map_err(|e| e.to_string())?
            } else {
                synthetic_reply(&req, peer_instance, state)
            };
            ownership.receive(reply, &req);
            Ok(())
        };

    let record_phase_emit = |phase: &str,
                             wide: bool,
                             active_pid: &str,
                             ownership_ref: &NativeOwnershipController,
                             receipts: &mut Vec<HostActionReceipt>,
                             paired_events: &mut Vec<Value>|
     -> Result<(), String> {
        let kinds: &[&str] = if wide {
            &["wide", "encoder"]
        } else {
            &["encoder"]
        };
        for &kind in kinds {
            let before = receipts.len();
            let event_id = format!("{phase}-{kind}");
            let native = control_exchange(json!({"eventId": event_id, "control": kind}))?;
            emit_synthetic_control(&draft, active_pid, ownership_ref, kind, receipts);
            if peer_socket.is_some() {
                let native_count = native
                    .get("nativeBusinessActions")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| "native event receipt missing".to_owned())?;
                paired_events.push(json!({
                    "event_id": event_id,
                    "native_actions": native_count,
                    "micro_actions": (receipts.len() - before) as i64,
                }));
            }
        }
        Ok(())
    };

    exchange_state(&mut ownership, "native")?;
    record_phase_emit(
        "native",
        true,
        &active_profile,
        &ownership,
        &mut receipts,
        &mut paired_events,
    )?;
    checks.insert("native_blocks_wide_and_encoder".into(), receipts.is_empty());

    exchange_state(&mut ownership, "released")?;
    record_phase_emit(
        "released",
        true,
        &active_profile,
        &ownership,
        &mut receipts,
        &mut paired_events,
    )?;
    checks.insert(
        "released_wide_once_encoder_once".into(),
        receipts.len() == 2
            && receipts
                .iter()
                .filter(|r| r.binding_id == "AG10" || r.binding_id == "AG11")
                .count()
                == 1
            && receipts.iter().filter(|r| r.binding_id == "AG14").count() == 1,
    );

    let old_req = ownership.begin_request(true);
    let delayed_reply = if let Some(ref t) = transport {
        t.exchange(&old_req).map_err(|e| e.to_string())?
    } else {
        synthetic_reply(&old_req, peer_instance, "released")
    };
    active_profile = set_host_active_profile(&draft, &mut ownership, &antigravity_id)?;
    ownership.receive(delayed_reply, &old_req);
    record_phase_emit(
        "stale-profile",
        false,
        &active_profile,
        &ownership,
        &mut receipts,
        &mut paired_events,
    )?;
    checks.insert(
        "profile_switch_rejects_old_release".into(),
        receipts.len() == 2 && !ownership.allows_execution(),
    );

    exchange_state(&mut ownership, "released")?;
    record_phase_emit(
        "fresh",
        false,
        &active_profile,
        &ownership,
        &mut receipts,
        &mut paired_events,
    )?;
    checks.insert(
        "fresh_release_routes_current_profile".into(),
        receipts.len() == 3
            && receipts.last().map(|r| r.profile_id.as_str()) == Some(antigravity_id.as_str()),
    );

    let lost_req = ownership.begin_request(false);
    if let Some(ref t) = transport {
        let _ = control_exchange(json!({"cmd": "disconnect"}))?;
        match t.exchange(&lost_req) {
            Ok(_) => return Err("peer did not disconnect".into()),
            Err(_) => ownership.failed(&lost_req),
        }
    } else {
        ownership.failed(&lost_req);
    }
    record_phase_emit(
        "lost",
        true,
        &active_profile,
        &ownership,
        &mut receipts,
        &mut paired_events,
    )?;
    checks.insert(
        "lost_peer_blocks_input".into(),
        receipts.len() == 3 && !ownership.allows_execution(),
    );

    let direct_blocked = !ownership.allows_execution();
    checks.insert(
        "unknown_blocks_direct_action".into(),
        direct_blocked && receipts.len() == 3,
    );

    let (ptt_started, _) = voice
        .start_dictation(
            "cindy",
            NavigationPlace::cindy("/tmp/ws", "sess-1"),
            "AG10",
            &antigravity_id,
            &ownership,
            true,
            &mut ptt,
        )
        .await;
    let lighting_res = lighting.update_feedback(
        FourAppAgentPhase::Idle,
        &[],
        false,
        &ownership,
        false,
        false,
        false,
        Some("synthetic-device"),
        |_| Ok("{\"synced\":true}".into()),
    );
    checks.insert(
        "unknown_blocks_ptt_and_lighting".into(),
        !ptt_started && !ptt.is_active && lighting_res == LightingUpdateResult::YieldedToNative,
    );

    let cindy_layer_id = draft.document.editable.graph.profiles[&cindy_id].layer_order[0].clone();
    let codex_layer_id = draft.document.editable.graph.profiles[&codex_id].layer_order[0].clone();
    let protected_selected = draft.document.is_protected_oai_layer(&cindy_layer_id)
        && draft.document.is_protected_oai_layer(&codex_layer_id);
    let save_rejected = draft
        .assign(&json!(codex_id), &json!(codex_layer_id), 14, search_action)
        .is_err();
    let settings_rejected = draft
        .set_control_settings(
            &codex_id,
            &codex_layer_id,
            ControlSettings {
                joystick_mode: "logical_sectors".into(),
                active_sectors: 4,
            },
        )
        .is_err();
    let lighting_edit_rejected = draft
        .edit_lighting(
            &codex_id,
            &codex_layer_id,
            Zone::Backlight,
            Edit::Color(color_from_hex("#3377CC")?),
            false,
        )
        .is_err();
    let export_layer_blocked = export(&draft, Some(&codex_id), Some(&codex_layer_id)).is_err();
    let duplicate_blocked = draft
        .duplicate_layer(&codex_id, &codex_layer_id, "ChatGPT Copy")
        .is_err();
    let compact_bytes = draft.compile_compact_host_keymap(Some(&codex_id))?;
    let compact_str = String::from_utf8(compact_bytes).map_err(|e| e.to_string())?;
    let compact_valid = compact_str.len() < 1024
        && compact_str.contains("KV_OAI_AG00")
        && compact_str.contains("VENDOR");
    checks.insert(
        "protected_oai_layer_locks_ui_and_lighting".into(),
        protected_selected
            && save_rejected
            && settings_rejected
            && lighting_edit_rejected
            && export_layer_blocked
            && duplicate_blocked
            && compact_valid,
    );

    set_host_active_profile(&draft, &mut ownership, &cindy_id)?;
    let cindy_yields = ownership.desired_owner() == "native" && !ownership.allows_execution();
    set_host_active_profile(&draft, &mut ownership, &codex_id)?;
    let codex_yields = ownership.desired_owner() == "native" && !ownership.allows_execution();
    set_host_active_profile(&draft, &mut ownership, &dsh_id)?;
    let dsh_takeover = ownership.desired_owner() == "micro-input" && !ownership.allows_execution();
    set_host_active_profile(&draft, &mut ownership, &antigravity_id)?;
    let ag_takeover = ownership.desired_owner() == "micro-input" && !ownership.allows_execution();
    let reclaim_req = ownership.begin_request(true);
    ownership.receive(
        synthetic_reply(&reclaim_req, peer_instance, "released"),
        &reclaim_req,
    );
    let reclaimed = ownership.allows_execution() && receipts.len() == 3;
    ownership.invalidate("scenario complete", false);
    checks.insert(
        "cindy_codex_to_workbench_handoff".into(),
        cindy_yields && codex_yields && dsh_takeover && ag_takeover && reclaimed,
    );

    let all_passed = checks.len() == 9 && checks.values().all(|&v| v);
    let mut status = if all_passed { "PASS" } else { "FAIL" }.to_owned();

    let mut paired_status: Option<String> = None;
    if peer_socket.is_some() {
        let expected = BTreeMap::from([
            ("native-wide", 1i64),
            ("native-encoder", 1),
            ("released-wide", 1),
            ("released-encoder", 1),
            ("stale-profile-encoder", 0),
            ("fresh-encoder", 1),
            ("lost-wide", 0),
            ("lost-encoder", 0),
        ]);
        let paired_ok = paired_events.len() == expected.len()
            && paired_events.iter().all(|event| {
                let Some(id) = event.get("event_id").and_then(Value::as_str) else {
                    return false;
                };
                let Some(native) = event.get("native_actions").and_then(Value::as_i64) else {
                    return false;
                };
                let Some(micro) = event.get("micro_actions").and_then(Value::as_i64) else {
                    return false;
                };
                expected.get(id).copied() == Some(native + micro)
            });
        paired_status = Some(if paired_ok { "PASS" } else { "FAIL" }.into());
        if !paired_ok {
            status = "FAIL".into();
        }
    }

    if let Some(audit_path) = audit_log_path {
        if let Some(parent) = audit_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            let mut audit_file = std::fs::File::create(audit_path).map_err(|e| e.to_string())?;
            for r in &receipts {
                let line = serde_json::to_string(r).map_err(|e| e.to_string())?;
                writeln!(audit_file, "{line}").map_err(|e| e.to_string())?;
            }
            let ownership_json_path = parent.join("ownership.json");
            let mut payload = json!({
                "status": status,
                "checks": checks,
                "receipt_count": receipts.len(),
                "native_app": "NOT_RUN",
                "physical_key": "NOT_RUN",
                "lighting_visual": "NOT_RUN",
            });
            if let Some(ps) = paired_status {
                payload["paired_events"] = Value::Array(paired_events);
                payload["paired_native_source"] = json!(ps);
            }
            let pretty = serde_json::to_vec_pretty(&payload).map_err(|e| e.to_string())?;
            std::fs::write(ownership_json_path, pretty).map_err(|e| e.to_string())?;
        }
    }

    Ok(NativeOwnershipScenarioReport {
        status,
        checks,
        receipt_count: receipts.len(),
        receipts,
        native_app: "NOT_RUN".into(),
        physical_key: "NOT_RUN".into(),
        lighting_visual: "NOT_RUN".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_native_ownership_scenario_passes_all_nine_checks_and_three_receipts() {
        let dir = tempfile::tempdir().unwrap();
        let audit_path = dir.path().join("audit.jsonl");
        let report = run_native_ownership_scenario(None, Some(&audit_path))
            .await
            .unwrap();
        assert_eq!(report.status, "PASS");
        assert_eq!(report.checks.len(), 9);
        for (name, passed) in &report.checks {
            assert!(passed, "check {name} failed");
        }
        assert_eq!(report.receipt_count, 3);
        let audit_text = std::fs::read_to_string(&audit_path).unwrap();
        assert_eq!(audit_text.lines().count(), 3);
        let ownership_json: Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("ownership.json")).unwrap())
                .unwrap();
        assert_eq!(ownership_json["status"], "PASS");
        assert_eq!(ownership_json["receipt_count"], 3);
    }
}
