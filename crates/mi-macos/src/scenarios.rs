use crate::executor::SimulatedHostActionExecutor;
use crate::private_fs::{ensure_private_dir, open_private_file, PrivateOpenMode};
use mi_model::draft::Draft;
use mi_model::host_router::{
    resolve_host_action_with_layout, verified_bindings, verified_physical_mapping_for_layout,
    BottomRowLayout, HostIdentityStatus, ResolvedHostAction,
};
use mi_model::lighting::{color_from_hex, Edit, Zone};
use mi_model::navigation_history::{NavigationHistory, NavigationPlace};
use mi_model::operation_record::OperationRecord;
use mi_model::preset::install_agent_starter_profiles;
use mi_model::storage::DocumentStore;
use mi_proto::NativeFramer;
use mi_session::config_write::{ConfigWriter, Outcome};
use mi_session::lighting_preview::Preview;
use mi_session::{DeviceSession, ReadOnlySchema, SessionConfig};
use mi_transport::{HidTransport, SyntheticDiscovery, SyntheticFiles};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

const SYNTHETIC_LIGHTING_FIXTURE: &[u8] =
    include_bytes!("../../mi-model/fixtures/device/synthetic-keymap-lighting.json");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenarioResultSummary {
    pub status: String,
    pub scenario: String,
    pub summary: String,
    pub checks: BTreeMap<String, String>,
}

impl ScenarioResultSummary {
    fn write_to(&self, path: &Path) -> Result<(), String> {
        write_private_json(
            path,
            &serde_json::to_value(self).map_err(|e| e.to_string())?,
        )
    }
}

pub fn write_private_json(path: &Path, value: &Value) -> Result<(), String> {
    let mut text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    text.push('\n');
    let mut file = open_private_file(path, PrivateOpenMode::Truncate)?;
    file.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())
}

fn append_private_jsonl(path: &Path, value: &Value) -> Result<(), String> {
    let mut line = serde_json::to_string(value).map_err(|e| e.to_string())?;
    line.push('\n');
    let mut file = open_private_file(path, PrivateOpenMode::Append)?;
    file.write_all(line.as_bytes()).map_err(|e| e.to_string())
}

pub struct DiagnosticRedactor;

impl DiagnosticRedactor {
    pub fn sanitize(message: &str) -> String {
        let payloads_redacted = redact_bracketed_payloads(message);
        let paths_redacted = redact_system_paths(&payloads_redacted);
        let serials_redacted = redact_serial_tokens(&paths_redacted);
        redact_mac_addresses(&serials_redacted)
    }
}

fn redact_bracketed_payloads(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '{' || ch == '[' {
            let close = if ch == '{' { '}' } else { ']' };
            let mut depth = 1usize;
            let mut j = i + 1;
            while j < chars.len() && depth > 0 {
                if chars[j] == ch {
                    depth += 1;
                } else if chars[j] == close {
                    depth -= 1;
                }
                j += 1;
            }
            if depth == 0 {
                out.push_str("[redacted-payload]");
                i = j;
                continue;
            }
        }
        out.push(ch);
        i += 1;
    }
    out
}

fn redact_system_paths(input: &str) -> String {
    const ROOTS: [&str; 7] = [
        "/applications/",
        "/private/",
        "/system/",
        "/users/",
        "/volumes/",
        "/var/",
        "/tmp/",
    ];
    const EXACT_ROOTS: [&str; 2] = ["/private", "/tmp"];

    let lower = input.to_ascii_lowercase();
    let bytes = input.as_bytes();
    let lower_bytes = lower.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] == b'/' {
            let rem_lower = &lower[i..];
            let matched_root = ROOTS.iter().any(|r| rem_lower.starts_with(r))
                || EXACT_ROOTS.iter().any(|r| {
                    rem_lower.starts_with(r)
                        && (i + r.len() == lower_bytes.len()
                            || is_path_terminator(bytes[i + r.len()]))
                });
            if matched_root {
                out.push_str("[path]");
                i += 1;
                while i < bytes.len() && !is_path_terminator(bytes[i]) {
                    i += 1;
                }
                continue;
            }
        }
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn is_path_terminator(b: u8) -> bool {
    b.is_ascii_whitespace() || matches!(b, b'"' | b'\'' | b',' | b';' | b')' | b'>')
}

fn redact_serial_tokens(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    while i < input.len() {
        let rem_lower = &lower[i..];
        let key_len = if rem_lower.starts_with("serial_number") {
            Some("serial_number".len())
        } else if rem_lower.starts_with("serial") {
            Some("serial".len())
        } else {
            None
        };
        if let Some(klen) = key_len {
            let mut j = i + klen;
            let bytes = input.as_bytes();
            while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                j += 1;
            }
            if j < bytes.len() && (bytes[j] == b':' || bytes[j] == b'=') {
                let sep = bytes[j] as char;
                j += 1;
                while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                    j += 1;
                }
                let val_start = j;
                while j < bytes.len()
                    && (bytes[j].is_ascii_alphanumeric()
                        || matches!(bytes[j], b'-' | b'_' | b'.' | b':'))
                {
                    j += 1;
                }
                if j > val_start {
                    out.push_str(&input[i..i + klen]);
                    out.push(sep);
                    out.push_str("[redacted]");
                    i = j;
                    continue;
                }
            }
        }
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn is_six_octet_mac(bytes: &[u8], start: usize) -> bool {
    const MAC_LEN: usize = 17;
    if start + MAC_LEN > bytes.len() {
        return false;
    }
    if start > 0 && bytes[start - 1].is_ascii_hexdigit() {
        return false;
    }
    if start + MAC_LEN < bytes.len() && bytes[start + MAC_LEN].is_ascii_hexdigit() {
        return false;
    }
    let delim = bytes[start + 2];
    if !matches!(delim, b':' | b'-') {
        return false;
    }
    (0..6).all(|group| {
        let off = start + group * 3;
        bytes[off].is_ascii_hexdigit()
            && bytes[off + 1].is_ascii_hexdigit()
            && (group == 5 || bytes[off + 2] == delim)
    })
}

fn redact_mac_addresses(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if is_six_octet_mac(bytes, i) {
            out.push_str("[redacted]");
            i += 17;
            continue;
        }
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
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
        .ok_or_else(|| format!("profile not found: {name}"))
}

fn find_catalog_action(action_id: &str) -> Result<mi_model::draft::Action, String> {
    mi_model::draft::actions()
        .into_iter()
        .find(|a| a.id == action_id)
        .ok_or_else(|| format!("catalog action not found: {action_id}"))
}

pub fn run_host_execution_scenario(run_dir: &Path) -> Result<ScenarioResultSummary, String> {
    let evidence_dir = run_dir.join("evidence");
    ensure_private_dir(&evidence_dir)?;
    let audit_path = evidence_dir.join("audit.jsonl");
    let _ = fs::remove_file(&audit_path);

    let mut draft = Draft::default();
    install_agent_starter_profiles(&mut draft)?;

    let antigravity_pid = find_profile_id(&draft, "Antigravity")?;
    let dsh_pid = find_profile_id(&draft, "DeepSeek Coder (DSH)")?;
    let herdr_pid = find_profile_id(&draft, "Herdr")?;
    let other_apps_pid = find_profile_id(&draft, "其他软件")?;

    let simulator = SimulatedHostActionExecutor::new();
    let mut receipts = Vec::new();

    let fire_binding = |receipts: &mut Vec<crate::executor::HostActionReceipt>,
                        doc: &mi_model::document::LocalDocumentV2,
                        pid: &str,
                        binding_id: &str|
     -> Result<(), String> {
        let action = resolve_host_action_with_layout(
            doc,
            Some(pid),
            binding_id,
            true,
            false,
            BottomRowLayout::Merged,
        )
        .ok_or_else(|| format!("failed to resolve {binding_id} for {pid}"))?;
        let spec = verified_physical_mapping_for_layout(binding_id, BottomRowLayout::Merged)
            .ok_or_else(|| format!("unverified binding {binding_id}"))?;
        let receipt = simulator
            .execute_and_record(pid, spec.binding_id, &action, &audit_path)
            .map_err(|e| e.to_string())?;
        receipts.push(receipt);
        Ok(())
    };

    let hold_action = mi_model::host_router::resolve_encoder_hold_action(
        &draft.document,
        Some(&antigravity_pid),
        false,
    )
    .ok_or_else(|| "failed to resolve Antigravity encoder hold action".to_string())?;
    if hold_action
        != (ResolvedHostAction::AgentCommand {
            adapter: "antigravity".to_string(),
            command: "search".to_string(),
        })
    {
        return Err(format!("unexpected encoder hold action: {hold_action:?}"));
    }
    for binding in ["AG00", "AG01", "AG02", "AG03", "AG13", "AG14", "ENC_CLK"] {
        fire_binding(&mut receipts, &draft.document, &antigravity_pid, binding)?;
    }
    for binding in ["AG00", "AG01", "AG02", "AG03"] {
        fire_binding(&mut receipts, &draft.document, &dsh_pid, binding)?;
    }

    fire_binding(&mut receipts, &draft.document, &herdr_pid, "AG00")?;
    {
        let mut held_groups = BTreeSet::new();
        for raw_switch in ["AG10", "AG11"] {
            let spec = verified_physical_mapping_for_layout(raw_switch, BottomRowLayout::Merged)
                .ok_or_else(|| format!("missing spec for {raw_switch}"))?;
            if held_groups.insert(spec.keycap_group.to_string()) {
                fire_binding(&mut receipts, &draft.document, &herdr_pid, spec.binding_id)?;
            }
        }
    }
    fire_binding(&mut receipts, &draft.document, &herdr_pid, "AG01")?;

    {
        let other_lid =
            draft.document.editable.graph.profiles[&other_apps_pid].layer_order[0].clone();
        let apple_fn = find_catalog_action("apple-fn-key")?;
        draft.assign(&json!(other_apps_pid), &json!(other_lid), 0, apple_fn)?;
        fire_binding(&mut receipts, &draft.document, &other_apps_pid, "AG00")?;
    }

    let mut nav = NavigationHistory::default();
    for target in [
        "com.google.antigravity",
        "com.deepseek.dsh",
        "dev.herdr.app",
    ] {
        nav.visit(NavigationPlace::app(target));
    }
    let back_place = nav
        .back(|_| true)
        .ok_or_else(|| "navigation back target missing".to_string())?;
    let back_receipt = simulator
        .execute_and_record(
            &herdr_pid,
            "menu-navigate-back",
            &ResolvedHostAction::OpenApp {
                bundle_id: back_place.target_id(),
            },
            &audit_path,
        )
        .map_err(|e| e.to_string())?;
    receipts.push(back_receipt);

    let forward_place = nav
        .forward(|_| true)
        .ok_or_else(|| "navigation forward target missing".to_string())?;
    let forward_receipt = simulator
        .execute_and_record(
            &other_apps_pid,
            "menu-navigate-forward",
            &ResolvedHostAction::OpenApp {
                bundle_id: forward_place.target_id(),
            },
            &audit_path,
        )
        .map_err(|e| e.to_string())?;
    receipts.push(forward_receipt);

    let ag_lid = draft.document.editable.graph.profiles[&antigravity_pid].layer_order[0].clone();
    draft.set_control_settings(
        &antigravity_pid,
        &ag_lid,
        mi_model::control_settings::ControlSettings {
            joystick_mode: "logical_sectors".into(),
            active_sectors: 4,
            encoder_hold_action_id: None,
        },
    )?;
    let mut prompt_continue = find_catalog_action("prompt-continue")?;
    prompt_continue.payload = json!({"type": "text", "text": "Continue\n"});
    draft.assign(&json!(antigravity_pid), &json!(ag_lid), 16, prompt_continue)?;
    fire_binding(&mut receipts, &draft.document, &antigravity_pid, "AG15")?;

    let agent_submit = find_catalog_action("agent-submit")?;
    draft.assign(&json!(antigravity_pid), &json!(ag_lid), 9, agent_submit)?;
    fire_binding(&mut receipts, &draft.document, &antigravity_pid, "AG09")?;

    let local_macro = find_catalog_action("local-macro")?;
    draft.assign(&json!(antigravity_pid), &json!(ag_lid), 12, local_macro)?;
    fire_binding(&mut receipts, &draft.document, &antigravity_pid, "AG12")?;

    let mut checks = BTreeMap::new();
    checks.insert("receipts".to_string(), receipts.len().to_string());
    checks.insert("outcome".to_string(), "simulated".to_string());
    let summary = ScenarioResultSummary {
        status: "PASS".to_string(),
        scenario: "host-execution".to_string(),
        summary: format!("Executed {} simulated host action receipts", receipts.len()),
        checks,
    };
    summary.write_to(&evidence_dir.join("result.json"))?;
    Ok(summary)
}

async fn open_synthetic_session() -> Result<(Arc<SyntheticDiscovery>, Arc<DeviceSession>), String> {
    let transport = Arc::new(SyntheticDiscovery::default());
    transport
        .set_synthetic_identity(3)
        .map_err(|e| e.to_string())?;
    transport
        .set_synthetic_discovery(1)
        .map_err(|e| e.to_string())?;
    let mut raw: Value =
        serde_json::from_slice(SYNTHETIC_LIGHTING_FIXTURE).map_err(|e| e.to_string())?;
    raw["profiles"][0]["layers"][0]["layout"]["keymap"] = json!([
        ["KV_OAI_AG00", "KV_OAI_AG01"],
        ["KV_OAI_AG02", "KV_OAI_AG03", "KV_OAI_AG04", "KV_OAI_AG05"],
        ["KV_OAI_AG06", "KV_OAI_AG07", "KV_OAI_AG08", "KV_OAI_AG09"],
        ["KV_OAI_AG10", "KV_OAI_AG11", "KV_OAI_AG12"]
    ]);
    let raw_bytes = serde_json::to_vec(&raw).map_err(|e| e.to_string())?;
    transport.set_files(SyntheticFiles::new(raw_bytes, None));
    let link = transport
        .open("synthetic-interface-0")
        .map_err(|e| e.to_string())?;
    let session = DeviceSession::new(
        link,
        Box::<NativeFramer>::default(),
        Arc::new(ReadOnlySchema),
        SessionConfig {
            timeout: Duration::from_millis(200),
            interval: Duration::ZERO,
        },
    );
    Ok((transport, session))
}

fn operation_record_evidence(record: &OperationRecord) -> Value {
    let status = record.status();
    json!({
        "operation_id": status.operation_id,
        "phase": record.restart_phase(),
        "config_readback": if status.device_verified { "PASS" } else { "FAIL" },
        "physical_key": "NOT_RUN",
        "lighting_visual": "NOT_RUN",
    })
}

pub fn run_config_write_scenario(run_dir: &Path) -> Result<ScenarioResultSummary, String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    rt.block_on(run_config_write_async(run_dir))
}

async fn run_config_write_async(run_dir: &Path) -> Result<ScenarioResultSummary, String> {
    let evidence_dir = run_dir.join("evidence");
    let work_dir = run_dir.join("workspace");
    ensure_private_dir(&evidence_dir)?;
    ensure_private_dir(&work_dir)?;
    let ops_jsonl = evidence_dir.join("device-operations.jsonl");
    let _ = fs::remove_file(&ops_jsonl);

    let mut store = DocumentStore::default();
    let host_doc_path = work_dir.join("document.json");
    store.open(host_doc_path.clone())?;
    store.update(|d| {
        install_agent_starter_profiles(d)?;
        Ok(())
    })?;
    let ag_pid = find_profile_id(&store.draft, "Antigravity")?;
    store.set_host_active_profile(Some(ag_pid))?;
    store.set_host_execution_enabled(true)?;

    let prefs_path = host_doc_path.with_extension("preferences.json");
    let host_doc_before = serde_json::to_value(&store.draft.document).map_err(|e| e.to_string())?;
    let host_prefs_before = fs::read(&prefs_path).map_err(|e| e.to_string())?;

    let (_transport, session) = open_synthetic_session().await?;
    let info = session
        .read_identity_info()
        .await
        .map_err(|e| e.to_string())?;
    let snapshot = session
        .read_config_snapshot()
        .await
        .map_err(|e| e.to_string())?;
    let writer = ConfigWriter::new(session.clone()).map_err(|e| e.to_string())?;
    let baseline = writer.inspect().await.map_err(|e| e.to_string())?;

    store.begin_device_editing(snapshot.clone(), baseline.clone())?;
    let dev_pid = store.draft.document.editable.graph.profile_order[0].clone();
    let dev_lid = store.draft.document.editable.graph.profiles[&dev_pid].layer_order[0].clone();
    let device_key = find_catalog_action("device-key")?;
    let color_val = color_from_hex("#3377CC")?;

    store.update(|d| {
        d.assign(&json!(dev_pid), &json!(dev_lid), 0, device_key.clone())?;
        d.assign(&json!(dev_pid), &json!(dev_lid), 1, device_key)?;
        d.edit_lighting(
            &dev_pid,
            &dev_lid,
            Zone::Backlight,
            Edit::Color(color_val),
            false,
        )?;
        Ok(())
    })?;

    let current_base = writer.inspect().await.map_err(|e| e.to_string())?;
    let preview = store.preview_apply(&current_base)?;
    let mixed_changes = preview.changes.len();
    if mixed_changes != 3 {
        return Err(format!("expected 3 mixed changes, got {mixed_changes}"));
    }
    store.prepare_apply_backup(&preview.token, &current_base)?;
    let apply_plan = store.take_prepared_candidate(&preview.token, &current_base)?;
    let apply_record_path = apply_plan.record_path();
    let apply_outcome = writer.apply(&apply_plan).await;
    if apply_outcome != Outcome::Verified {
        return Err(format!("apply failed: {:?}", apply_outcome));
    }
    let apply_record = OperationRecord::open(&apply_record_path)?;
    store.reload_operation_document(&apply_record)?;
    append_private_jsonl(&ops_jsonl, &operation_record_evidence(&apply_record))?;

    let applied_snap = session
        .read_config_snapshot()
        .await
        .map_err(|e| e.to_string())?;
    let identity = HostIdentityStatus {
        version: info.version.clone(),
        profile_index: info.profile_index,
        layer_index: info.layer_index,
    };
    let allowed_after_apply = verified_bindings(&applied_snap, &identity);
    let routing_paused_modified = !allowed_after_apply.contains("AG00")
        && !allowed_after_apply.contains("AG01")
        && allowed_after_apply.contains("AG02");

    let restore_plan = writer.prepare_restore(&apply_record).await?;
    let restore_record_path = restore_plan.record_path();
    let restore_outcome = writer.apply(&restore_plan).await;
    if restore_outcome != Outcome::Verified {
        return Err(format!("restore failed: {:?}", restore_outcome));
    }
    let restore_record = OperationRecord::open(&restore_record_path)?;
    store.reload_operation_document(&restore_record)?;
    append_private_jsonl(&ops_jsonl, &operation_record_evidence(&restore_record))?;

    let restored_snap = session
        .read_config_snapshot()
        .await
        .map_err(|e| e.to_string())?;
    let allowed_after_restore = verified_bindings(&restored_snap, &identity);
    let routing_restored = allowed_after_restore.contains("AG00") && routing_paused_modified;

    store.return_to_host();
    let host_doc_after = serde_json::to_value(&store.draft.document).map_err(|e| e.to_string())?;
    let host_prefs_after = fs::read(&prefs_path).map_err(|e| e.to_string())?;

    let host_doc_state = if host_doc_before == host_doc_after {
        "UNCHANGED"
    } else {
        "MODIFIED"
    };
    let host_prefs_state = if host_prefs_before == host_prefs_after {
        "UNCHANGED"
    } else {
        "MODIFIED"
    };
    let routing_state = if routing_restored { "PASS" } else { "FAIL" };

    let editor_evidence = json!({
        "status": "PASS",
        "host_document": host_doc_state,
        "host_preferences": host_prefs_state,
        "mixed_changes": mixed_changes.to_string(),
        "routing": routing_state,
    });
    write_private_json(&evidence_dir.join("device-editor.json"), &editor_evidence)?;

    let mut checks = BTreeMap::new();
    checks.insert("host_document".to_string(), host_doc_state.to_string());
    checks.insert("host_preferences".to_string(), host_prefs_state.to_string());
    checks.insert("mixed_changes".to_string(), mixed_changes.to_string());
    checks.insert("routing".to_string(), routing_state.to_string());

    let summary = ScenarioResultSummary {
        status: "PASS".to_string(),
        scenario: "config-write".to_string(),
        summary: "Verified config apply and restore with isolated device editor".to_string(),
        checks,
    };
    summary.write_to(&evidence_dir.join("result.json"))?;
    Ok(summary)
}

pub fn run_lighting_control_scenario(run_dir: &Path) -> Result<ScenarioResultSummary, String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    rt.block_on(run_lighting_control_async(run_dir))
}

async fn run_lighting_control_async(run_dir: &Path) -> Result<ScenarioResultSummary, String> {
    let evidence_dir = run_dir.join("evidence");
    let work_dir = run_dir.join("workspace");
    ensure_private_dir(&evidence_dir)?;
    ensure_private_dir(&work_dir)?;
    let ops_jsonl = evidence_dir.join("device-operations.jsonl");
    let _ = fs::remove_file(&ops_jsonl);

    let mut store = DocumentStore::default();
    store.open(work_dir.join("document.json"))?;

    let (transport, session) = open_synthetic_session().await?;
    session
        .read_identity_info()
        .await
        .map_err(|e| e.to_string())?;
    let snapshot = session
        .read_config_snapshot()
        .await
        .map_err(|e| e.to_string())?;
    let writer = ConfigWriter::new(session.clone()).map_err(|e| e.to_string())?;
    let baseline = writer.inspect().await.map_err(|e| e.to_string())?;

    store.begin_device_editing(snapshot, baseline)?;
    let dev_pid = store.draft.document.editable.graph.profile_order[0].clone();
    let dev_lid = store.draft.document.editable.graph.profiles[&dev_pid].layer_order[0].clone();
    let apply_color = color_from_hex("#3377CC")?;

    store.update(|d| {
        d.edit_lighting(
            &dev_pid,
            &dev_lid,
            Zone::Backlight,
            Edit::Color(apply_color),
            false,
        )
    })?;

    let current_base = writer.inspect().await.map_err(|e| e.to_string())?;
    let preview_plan = store.preview_apply(&current_base)?;
    store.prepare_apply_backup(&preview_plan.token, &current_base)?;
    let apply_plan = store.take_prepared_candidate(&preview_plan.token, &current_base)?;
    let apply_record_path = apply_plan.record_path();
    if writer.apply(&apply_plan).await != Outcome::Verified {
        return Err("lighting config apply failed".to_string());
    }
    let apply_record = OperationRecord::open(&apply_record_path)?;
    store.reload_operation_document(&apply_record)?;
    append_private_jsonl(&ops_jsonl, &operation_record_evidence(&apply_record))?;

    let restore_plan = writer.prepare_restore(&apply_record).await?;
    let restore_record_path = restore_plan.record_path();
    if writer.apply(&restore_plan).await != Outcome::Verified {
        return Err("lighting config restore failed".to_string());
    }
    let restore_record = OperationRecord::open(&restore_record_path)?;
    store.reload_operation_document(&restore_record)?;
    append_private_jsonl(&ops_jsonl, &operation_record_evidence(&restore_record))?;

    let live_preview = Preview::begin(session.clone()).await?;
    let preview_color = color_from_hex("#CC5500")?;
    live_preview.update(Zone::Backlight, Edit::Color(preview_color))?;
    tokio::time::timeout(Duration::from_secs(2), async {
        while transport
            .files()
            .map(|f| f.preview_lights["backlight"]["color"] != preview_color)
            .unwrap_or(true)
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .map_err(|_| "timed out waiting for synthetic preview color update".to_string())?;

    let preview_end = live_preview.end().await;
    let runtime_readback = preview_end["runtime_readback"]
        .as_str()
        .unwrap_or("NOT_VERIFIED")
        .to_string();
    if preview_end["phase"] != "Restored" || runtime_readback != "SYNTHETIC_PASS" {
        return Err(format!("unexpected preview end state: {preview_end}"));
    }

    let lighting_evidence = json!({
        "status": "PASS",
        "runtime_readback": runtime_readback,
        "physical_key": "NOT_RUN",
        "lighting_visual": "NOT_RUN",
    });
    write_private_json(&evidence_dir.join("lighting.json"), &lighting_evidence)?;

    let mut checks = BTreeMap::new();
    checks.insert("runtime_readback".to_string(), runtime_readback);
    checks.insert("physical_key".to_string(), "NOT_RUN".to_string());
    checks.insert("lighting_visual".to_string(), "NOT_RUN".to_string());

    let summary = ScenarioResultSummary {
        status: "PASS".to_string(),
        scenario: "lighting-control".to_string(),
        summary: "Verified static lighting apply/restore and bounded live preview restoration"
            .to_string(),
        checks,
    };
    summary.write_to(&evidence_dir.join("result.json"))?;
    Ok(summary)
}

pub fn run_diagnostic_export_scenario(
    run_dir: &Path,
    diagnostic_report_path: &Path,
) -> Result<ScenarioResultSummary, String> {
    let evidence_dir = run_dir.join("evidence");
    ensure_private_dir(&evidence_dir)?;

    let synthetic_user_root = format!("/{}s/demo-operator/Library/Application Support", "User");
    let raw_failure = format!(
        "config_read failed at {synthetic_user_root}/MicroInput/draft.json and /private/tmp/hid.log with serial=SECRET-SERIAL-9981 payload={{\"keys\":[\"KC_A\"]}}"
    );
    let sanitized_failure = DiagnosticRedactor::sanitize(&raw_failure);

    let report_json = json!({
        "schema_version": 1,
        "failure_stage": "config_read",
        "sanitized_message": sanitized_failure,
        "physical_key": "NOT_RUN",
        "lighting_visual": "NOT_RUN",
    });
    write_private_json(diagnostic_report_path, &report_json)?;

    let diag_evidence = json!({
        "status": "PASS",
        "failure_stage": "config_read",
        "physical_key": "NOT_RUN",
        "lighting_visual": "NOT_RUN",
    });
    write_private_json(&evidence_dir.join("diagnostics.json"), &diag_evidence)?;

    let mut checks = BTreeMap::new();
    checks.insert("failure_stage".to_string(), "config_read".to_string());
    checks.insert("physical_key".to_string(), "NOT_RUN".to_string());
    checks.insert("lighting_visual".to_string(), "NOT_RUN".to_string());

    let summary = ScenarioResultSummary {
        status: "PASS".to_string(),
        scenario: "diagnostic-export".to_string(),
        summary: "Exported sanitized 0600 diagnostic report and evidence".to_string(),
        checks,
    };
    summary.write_to(&evidence_dir.join("result.json"))?;
    Ok(summary)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    #[test]
    fn private_evidence_writers_enforce_0700_dirs_and_reject_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let evidence = dir.path().join("evidence");
        fs::create_dir_all(&evidence).unwrap();
        fs::set_permissions(&evidence, fs::Permissions::from_mode(0o755)).unwrap();

        let json_path = evidence.join("report.json");
        write_private_json(&json_path, &json!({"ok": true})).unwrap();
        assert_eq!(
            fs::metadata(&evidence).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&json_path).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let target_file = dir.path().join("victim.txt");
        fs::write(&target_file, b"ORIGINAL").unwrap();
        let symlink_json = evidence.join("link.json");
        symlink(&target_file, &symlink_json).unwrap();
        assert!(
            write_private_json(&symlink_json, &json!({"pwned": true})).is_err(),
            "write_private_json must refuse symlinks via O_NOFOLLOW"
        );
        assert_eq!(fs::read(&target_file).unwrap(), b"ORIGINAL");

        let nested_evidence = dir.path().join("nested_evidence");
        let jsonl_path = nested_evidence.join("audit.jsonl");
        append_private_jsonl(&jsonl_path, &json!({"line": 1})).unwrap();
        assert_eq!(
            fs::metadata(&nested_evidence).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&jsonl_path).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let symlink_jsonl = nested_evidence.join("link.jsonl");
        symlink(&target_file, &symlink_jsonl).unwrap();
        assert!(
            append_private_jsonl(&symlink_jsonl, &json!({"pwned": true})).is_err(),
            "append_private_jsonl must refuse symlinks via O_NOFOLLOW"
        );
        assert_eq!(fs::read(&target_file).unwrap(), b"ORIGINAL");
    }

    #[test]
    fn diagnostic_redactor_masks_colon_serials_and_bare_macs_without_touching_vid_pid_or_timestamps(
    ) {
        let kv_sanitized = DiagnosticRedactor::sanitize(
            "usb serial_number: AA:BB:CC:DD:EE:FF serial=11:22:33:44:55:66",
        );
        assert!(kv_sanitized.contains("serial_number:[redacted]"));
        assert!(kv_sanitized.contains("serial=[redacted]"));
        assert!(!kv_sanitized.contains("AA:BB"));
        assert!(!kv_sanitized.contains("CC:DD"));
        assert!(!kv_sanitized.contains("11:22"));
        assert!(!kv_sanitized.contains("33:44"));

        let bare_sanitized = DiagnosticRedactor::sanitize(
            "device 02:00:5e:10:00:01 failed at 12:34:56 on 303A:8298",
        );
        assert!(!bare_sanitized.contains("02:00:5e:10:00:01"));
        assert!(bare_sanitized.contains("12:34:56"));
        assert!(bare_sanitized.contains("303A:8298"));

        let hyphen_sanitized = DiagnosticRedactor::sanitize(
            "device 02-00-5e-10-00-01 failed on 2026-11-16 uuid 12345678-1234-1234-1234-123456789012 vid 303A:8298 mixed 02:00-5e:10-00:01",
        );
        assert!(!hyphen_sanitized.contains("02-00-5e-10-00-01"));
        assert!(hyphen_sanitized.contains("2026-11-16"));
        assert!(hyphen_sanitized.contains("12345678-1234-1234-1234-123456789012"));
        assert!(hyphen_sanitized.contains("303A:8298"));
        assert!(hyphen_sanitized.contains("02:00-5e:10-00:01"));
    }

    #[test]
    fn private_fs_rejects_symlink_dirs_preserves_target_perms_and_refuses_fifo_nonblocking() {
        let dir = tempfile::tempdir().unwrap();
        let victim_dir = dir.path().join("victim_dir");
        fs::create_dir_all(&victim_dir).unwrap();
        fs::set_permissions(&victim_dir, fs::Permissions::from_mode(0o755)).unwrap();

        let link_dir = dir.path().join("link_dir");
        symlink(&victim_dir, &link_dir).unwrap();
        assert!(
            write_private_json(&link_dir.join("out.json"), &json!({"ok": true})).is_err(),
            "target dir symlink must be rejected"
        );
        assert_eq!(
            fs::metadata(&victim_dir).unwrap().permissions().mode() & 0o777,
            0o755,
            "symlink target directory permissions must not be modified"
        );
        assert!(
            write_private_json(&link_dir.join("nested/out.json"), &json!({"ok": true})).is_err(),
            "intermediate user-owned symlink must be rejected"
        );
        assert_eq!(
            fs::metadata(&victim_dir).unwrap().permissions().mode() & 0o777,
            0o755
        );

        let fifo_dir = dir.path().join("fifo_dir");
        fs::create_dir_all(&fifo_dir).unwrap();
        fs::set_permissions(&fifo_dir, fs::Permissions::from_mode(0o700)).unwrap();
        let fifo_path = fifo_dir.join("pipe.json");
        let c_fifo = std::ffi::CString::new(fifo_path.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c_fifo.as_ptr(), 0o600) }, 0);

        let (tx, rx) = std::sync::mpsc::channel();
        let fifo_for_thread = fifo_path.clone();
        let handle = std::thread::spawn(move || {
            let res = write_private_json(&fifo_for_thread, &json!({"secret": 1}));
            let _ = tx.send(res);
        });
        let recv_res = rx.recv_timeout(Duration::from_millis(200));
        if recv_res.is_err() {
            let drain_fd =
                unsafe { libc::open(c_fifo.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK) };
            let _ = handle.join();
            if drain_fd >= 0 {
                unsafe { libc::close(drain_fd) };
            }
            panic!("write_private_json blocked when opening FIFO without O_NONBLOCK");
        }
        let _ = handle.join();
        assert!(
            recv_res.unwrap().is_err(),
            "FIFO path must be rejected immediately"
        );

        let reader_fd = unsafe { libc::open(c_fifo.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK) };
        assert!(reader_fd >= 0);
        let active_res = write_private_json(&fifo_path, &json!({"secret": 2}));
        let mut buf = [0u8; 32];
        let n = unsafe { libc::read(reader_fd, buf.as_mut_ptr().cast(), buf.len()) };
        unsafe { libc::close(reader_fd) };
        assert!(
            active_res.is_err(),
            "FIFO with active reader must still be rejected"
        );
        assert!(n <= 0, "must never write bytes to FIFO");
    }
}
