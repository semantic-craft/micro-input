use mi_core::qa_runner::{run_synthetic_scenario, QaLaunchEnv, SyntheticScenario9};
use mi_macos::DocumentInstanceLock;
use serde_json::Value;
use std::fs;
use tempfile::TempDir;

#[test]
fn synthetic_scenario_9_parses_all_canonical_names_and_rejects_unknown() {
    let expected = [
        ("host-execution", SyntheticScenario9::HostExecution),
        ("native-ownership", SyntheticScenario9::NativeOwnership),
        ("dsh-navigation", SyntheticScenario9::DshNavigation),
        ("cindy-navigation", SyntheticScenario9::CindyNavigation),
        ("antigravity-status", SyntheticScenario9::AntigravityStatus),
        ("config-write", SyntheticScenario9::ConfigWrite),
        ("lighting-control", SyntheticScenario9::LightingControl),
        ("diagnostic-export", SyntheticScenario9::DiagnosticExport),
    ];
    assert_eq!(SyntheticScenario9::ALL.len(), 8);
    for (raw, variant) in expected {
        let parsed = SyntheticScenario9::parse(raw).expect("canonical scenario must parse");
        assert_eq!(parsed, variant);
        assert_eq!(parsed.as_str(), raw);
    }
    assert!(SyntheticScenario9::parse("unknown-scenario").is_err());
}

#[test]
fn document_instance_lock_rejects_duplicate_owner_until_released() {
    let temp = TempDir::new().expect("tempdir");
    let doc_dir = temp.path().join("documents");

    let first = DocumentInstanceLock::acquire_with_pid(&doc_dir, 4242)
        .expect("first lock io")
        .expect("first lock acquired");
    assert_eq!(DocumentInstanceLock::owner_pid(&doc_dir), Some(4242));

    let duplicate = DocumentInstanceLock::acquire_with_pid(&doc_dir, 4243).expect("dup lock io");
    assert!(duplicate.is_none());

    drop(first);
    assert_eq!(DocumentInstanceLock::owner_pid(&doc_dir), None);

    let reacquired = DocumentInstanceLock::acquire_with_pid(&doc_dir, 4243)
        .expect("reacquire lock io")
        .expect("reacquired after release");
    assert!(reacquired.is_held());
}

#[test]
fn qa_runner_executes_all_nine_synthetic_scenarios_with_isolated_evidence() {
    for scenario in SyntheticScenario9::ALL {
        let temp = TempDir::new().expect("tempdir");
        let run_dir = temp.path().join(scenario.as_str());
        let doc_dir = run_dir.join("documents");
        let evidence_dir = run_dir.join("evidence");
        let audit_path = evidence_dir.join("audit.jsonl");
        let diag_report_path = evidence_dir.join("diagnostic-report.json");

        let env = QaLaunchEnv::from_pairs(
            Some(doc_dir.to_str().unwrap()),
            Some(audit_path.to_str().unwrap()),
            Some(run_dir.to_str().unwrap()),
            Some(diag_report_path.to_str().unwrap()),
            Some(scenario.as_str()),
        )
        .expect("valid QaLaunchEnv");
        assert_eq!(env.scenario, Some(scenario));
        assert_eq!(env.resolved_run_dir().unwrap(), run_dir);

        run_synthetic_scenario(&env, scenario)
            .unwrap_or_else(|e| panic!("scenario {} failed: {e}", scenario.as_str()));

        match scenario {
            SyntheticScenario9::HostExecution => {
                let lines = fs::read_to_string(&audit_path).expect("audit.jsonl");
                assert_eq!(lines.lines().count(), 20);
            }
            SyntheticScenario9::NativeOwnership => {
                let ownership: Value =
                    serde_json::from_slice(&fs::read(evidence_dir.join("ownership.json")).unwrap())
                        .unwrap();
                assert_eq!(ownership["status"], "PASS");
                assert_eq!(ownership["receipt_count"], 3);
                let lines = fs::read_to_string(&audit_path).expect("audit.jsonl");
                let rows: Vec<Value> = lines
                    .lines()
                    .map(|l| serde_json::from_str(l).unwrap())
                    .collect();
                assert_eq!(rows.len(), 3);
                assert!(rows.iter().all(|r| r["outcome"] == "simulated"));
            }
            SyntheticScenario9::DshNavigation => {
                let doc: Value = serde_json::from_slice(
                    &fs::read(evidence_dir.join("dsh-navigation.json")).unwrap(),
                )
                .unwrap();
                assert_eq!(doc["status"], "PASS");
                assert!(audit_path.is_file());
            }
            SyntheticScenario9::CindyNavigation => {
                let doc: Value = serde_json::from_slice(
                    &fs::read(evidence_dir.join("cindy-navigation.json")).unwrap(),
                )
                .unwrap();
                assert_eq!(doc["status"], "PASS");
                assert!(audit_path.is_file());
            }
            SyntheticScenario9::AntigravityStatus => {
                let doc: Value = serde_json::from_slice(
                    &fs::read(evidence_dir.join("antigravity-status.json")).unwrap(),
                )
                .unwrap();
                assert_eq!(doc["status"], "PASS");
                assert!(audit_path.is_file());
            }
            SyntheticScenario9::ConfigWrite => {
                let doc: Value = serde_json::from_slice(
                    &fs::read(evidence_dir.join("device-editor.json")).unwrap(),
                )
                .unwrap();
                assert_eq!(doc["status"], "PASS");
                assert!(!run_dir.join("workspace").exists());
            }
            SyntheticScenario9::LightingControl => {
                let doc: Value =
                    serde_json::from_slice(&fs::read(evidence_dir.join("lighting.json")).unwrap())
                        .unwrap();
                assert_eq!(doc["status"], "PASS");
                assert!(!run_dir.join("workspace").exists());
            }
            SyntheticScenario9::DiagnosticExport => {
                let doc: Value = serde_json::from_slice(
                    &fs::read(evidence_dir.join("diagnostics.json")).unwrap(),
                )
                .unwrap();
                assert_eq!(doc["status"], "PASS");
                assert!(diag_report_path.is_file());
            }
        }
    }
}
