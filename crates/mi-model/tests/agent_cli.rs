use mi_model::{draft::Draft, storage::DocumentStore};
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("mi-agent-{}", mi_model::document::new_id()));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn cli(command: &str, request: Value) -> (bool, Value) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mi"))
        .args(["draft", command])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(serde_json::to_string(&request).unwrap().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    (
        output.status.success(),
        serde_json::from_slice(&output.stdout).unwrap(),
    )
}
#[test]
fn real_client_prepares_independent_proposal_without_changing_open_workbench() {
    let t = Directory::new();
    let path = t.0.join("workbench.json");
    let draft = Draft::default();
    fs::write(&path, serde_json::to_vec(&draft.document).unwrap()).unwrap();
    let original = fs::read(&path).unwrap();
    let (ok, read) = cli("read", json!({"api_version":1,"workbench":path}));
    assert!(ok, "{read}");
    let pid = read["result"]["document"]["graph"]["profile_order"][0]
        .as_str()
        .unwrap();
    let output = t.0.join("proposal.json");
    let request = json!({"api_version":1,"workbench":path,"expected":read["result"]["stamp"],"operations":[{"op":"rename_profile","profile_id":pid,"name":"Agent review"}],"output":output});
    let (ok, result) = cli("patch", request);
    assert!(ok, "{result}");
    assert_eq!(fs::read(&path).unwrap(), original);
    assert!(!path.with_extension("active.json").exists());
    let mut store = DocumentStore::default();
    store.open(path.clone()).unwrap();
    let preview = store
        .inspect_agent_proposal(&fs::read_to_string(&output).unwrap())
        .unwrap();
    assert!(preview.changes.iter().any(|c| c.contains("Agent review")));
    store.reject_agent_proposal();
    assert_eq!(store.draft.document_id, draft.document_id);
    let preview = store
        .inspect_agent_proposal(&fs::read_to_string(&output).unwrap())
        .unwrap();
    store.accept_agent_proposal(&preview.token).unwrap();
    assert_eq!(
        serde_json::to_value(&store.draft.document).unwrap(),
        result["result"]["document"]
    );
    let mut reopened = DocumentStore::default();
    reopened.open(path.clone()).unwrap();
    assert_eq!(reopened.draft.document_id, store.draft.document_id);
    assert_eq!(fs::read(path).unwrap(), original);
}
#[test]
fn stale_revisions_illegal_fields_and_dangling_references_cannot_produce_or_replace_proposals() {
    let t = Directory::new();
    let path = t.0.join("workbench.json");
    let draft = Draft::default();
    fs::write(&path, serde_json::to_vec(&draft.document).unwrap()).unwrap();
    let (_, read) = cli("read", json!({"api_version":1,"workbench":path}));
    let pid = read["result"]["document"]["graph"]["profile_order"][0].clone();
    let output = t.0.join("proposal.json");
    let valid = json!({"api_version":1,"workbench":path,"expected":read["result"]["stamp"],"operations":[{"op":"rename_profile","profile_id":pid,"name":"Candidate"}],"output":output});
    for (code, request) in [
        ("stale_revision", {
            let mut r = valid.clone();
            r["expected"]["revision"] = json!(999);
            r
        }),
        ("invalid_request", {
            let mut r = valid.clone();
            r["operations"][0]["raw"] = json!("forbidden");
            r
        }),
        ("invalid_operation", {
            let mut r = valid.clone();
            r["operations"][0]["profile_id"] = json!(mi_model::document::new_id());
            r
        }),
    ] {
        let (ok, result) = cli("patch", request);
        assert!(!ok);
        assert_eq!(result["error"]["code"], code);
        assert!(!output.exists());
    }
    let original = fs::read(&path).unwrap();
    let (ok, _) = cli("patch", valid.clone());
    assert!(ok);
    let proposal = fs::read(&output).unwrap();
    let (ok, result) = cli("patch", valid.clone());
    assert!(!ok);
    assert_eq!(result["error"]["code"], "conflict");
    assert_eq!(fs::read(output).unwrap(), proposal);
    assert_eq!(fs::read(&path).unwrap(), original);
    let mut store = DocumentStore::default();
    store.open(path.clone()).unwrap();
    let preview = store
        .inspect_agent_proposal(std::str::from_utf8(&proposal).unwrap())
        .unwrap();
    store
        .update(|d| d.rename_profile(pid.as_str().unwrap(), "UI changed"))
        .unwrap();
    assert_eq!(
        store
            .accept_agent_proposal(&preview.token)
            .unwrap_err()
            .code,
        "stale_revision"
    );
    assert!(!path.with_extension("active.json").exists());
}
#[test]
fn unknown_raw_is_retained_but_unknown_editable_fields_and_forged_proposals_are_rejected() {
    let t = Directory::new();
    let path = t.0.join("workbench.json");
    let mut draft = Draft::default();
    draft.document.origin["future_device_raw"] = json!({"unknown":[7,"retain"]});
    fs::write(&path, serde_json::to_vec(&draft.document).unwrap()).unwrap();
    let (_, read) = cli("read", json!({"api_version":1,"workbench":path}));
    let pid = read["result"]["document"]["graph"]["profile_order"][0].clone();
    let output = t.0.join("proposal.json");
    let request = json!({"api_version":1,"workbench":path,"expected":read["result"]["stamp"],"operations":[{"op":"rename_profile","profile_id":pid,"name":"Retain"}],"output":output});
    let (ok, result) = cli("patch", request.clone());
    assert!(ok);
    assert_eq!(
        result["result"]["document"]["origin"]["future_device_raw"],
        json!({"unknown":[7,"retain"]})
    );
    let mut proposal: Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
    proposal["document"]["origin"]["future_device_raw"] = json!("forged");
    let mut store = DocumentStore::default();
    store.open(path.clone()).unwrap();
    assert_eq!(
        store
            .inspect_agent_proposal(&proposal.to_string())
            .err()
            .unwrap()
            .code,
        "invalid_operation"
    );
    let mut raw: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    raw["graph"]["future_editable"] = json!(42);
    fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();
    let (_, read) = cli("read", json!({"api_version":1,"workbench":path}));
    assert_eq!(read["result"]["read_only"], true);
    assert_eq!(read["result"]["document"]["graph"]["future_editable"], 42);
    let mut r = request;
    r["expected"] = read["result"]["stamp"].clone();
    let (ok, result) = cli("patch", r);
    assert!(!ok);
    assert_eq!(result["error"]["code"], "read_only");
}
#[test]
fn binding_requires_existing_action_and_correct_profile_layer_control_ownership() {
    let t = Directory::new();
    let path = t.0.join("workbench.json");
    let mut draft = Draft::default();
    let p = draft.document.editable.graph.profile_order[0].clone();
    let l = draft.document.editable.graph.profiles[&p].layer_order[0].clone();
    draft
        .assign(
            &json!(p),
            &json!(l),
            0,
            mi_model::draft::actions()[0].clone(),
        )
        .unwrap();
    let layer = &draft.document.editable.graph.layers[&l];
    let binding = layer.control_order[1].clone();
    let action = layer.controls[&layer.control_order[0]]
        .action_id
        .clone()
        .unwrap();
    fs::write(&path, serde_json::to_vec(&draft.document).unwrap()).unwrap();
    let (_, read) = cli("read", json!({"api_version":1,"workbench":path}));
    let request = json!({"api_version":1,"workbench":path,"expected":read["result"]["stamp"],"operations":[{"op":"bind_action","profile_id":p,"layer_id":l,"binding_id":binding,"action_id":action}]});
    let (ok, result) = cli("validate", request.clone());
    assert!(ok);
    assert_eq!(
        result["result"]["document"]["graph"]["layers"][&l]["controls"][&binding]["action_id"],
        action
    );
    for key in ["profile_id", "layer_id", "binding_id", "action_id"] {
        let mut r = request.clone();
        r["operations"][0][key] = json!(mi_model::document::new_id());
        let (ok, result) = cli("validate", r);
        assert!(!ok);
        assert_eq!(result["error"]["code"], "invalid_operation");
    }
    assert_eq!(
        fs::read(&path).unwrap(),
        serde_json::to_vec(&draft.document).unwrap()
    );
}
#[test]
fn interrupted_source_and_missing_binding_action_are_explicit_failures() {
    let t = Directory::new();
    let path = t.0.join("workbench.json");
    let draft = Draft::default();
    fs::write(&path, serde_json::to_vec(&draft.document).unwrap()).unwrap();
    let (_, read) = cli("read", json!({"api_version":1,"workbench":path}));
    let p = draft.document.editable.graph.profile_order[0].clone();
    let l = draft.document.editable.graph.profiles[&p].layer_order[0].clone();
    let b = draft.document.editable.graph.layers[&l].control_order[0].clone();
    let missing = json!({"api_version":1,"workbench":path,"expected":read["result"]["stamp"],"operations":[{"op":"bind_action","profile_id":p,"layer_id":l,"binding_id":b}]});
    let (ok, result) = cli("validate", missing);
    assert!(!ok);
    assert_eq!(result["error"]["code"], "invalid_request");
    fs::write(path.with_extension("pending"), b"interrupted save").unwrap();
    let request = json!({"api_version":1,"workbench":path,"expected":read["result"]["stamp"],"operations":[{"op":"rename_profile","profile_id":p,"name":"Blocked"}],"output":t.0.join("proposal.json")});
    let (ok, result) = cli("patch", request);
    assert!(!ok);
    assert_eq!(result["error"]["code"], "conflict");
    assert!(!t.0.join("proposal.json").exists());
}
#[test]
fn unavailable_capabilities_versions_missing_files_and_locks_are_structured() {
    let t = Directory::new();
    let path = t.0.join("workbench.json");
    for (command, request, code) in [
        (
            "apply",
            json!({"api_version":1,"workbench":path}),
            "capability_unavailable",
        ),
        (
            "read",
            json!({"api_version":2,"workbench":path}),
            "unsupported_version",
        ),
        (
            "read",
            json!({"api_version":1,"workbench":path}),
            "not_found",
        ),
    ] {
        let (ok, result) = cli(command, request);
        assert!(!ok);
        assert_eq!(result["error"]["code"], code);
    }
    fs::write(
        &path,
        serde_json::to_vec(&Draft::default().document).unwrap(),
    )
    .unwrap();
    let (_, read) = cli("read", json!({"api_version":1,"workbench":path}));
    assert_eq!(read["result"]["capabilities"]["device_apply"], false);
    assert_eq!(read["result"]["capabilities"]["host_execute"], false);
    for marker in [
        path.with_extension("lock"),
        path.with_extension("active.lock"),
    ] {
        fs::write(&marker, b"existing").unwrap();
        let (ok, result) = cli("read", json!({"api_version":1,"workbench":path}));
        assert!(!ok);
        assert_eq!(result["error"]["code"], "conflict");
        assert_eq!(fs::read(&marker).unwrap(), b"existing");
        fs::remove_file(marker).unwrap();
    }
}
