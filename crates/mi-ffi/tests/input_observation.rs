//! Opt-in native observation and synthetic checks at the production MicroInput seam.
use mi_ffi::MicroInput;
#[cfg(feature = "synthetic-backend")]
use mi_ffi::SyntheticInputCase;
use std::time::Duration;
mod input_observation_support;
use input_observation_support::*;

#[cfg(feature = "synthetic-backend")]
async fn connected() -> std::sync::Arc<MicroInput> {
    let core = MicroInput::synthetic_discovery();
    core.set_synthetic_discovery(mi_ffi::SyntheticDiscoveryState::ReconnectedDevice)
        .await
        .unwrap();
    let device = core.list_devices().await.unwrap().remove(0);
    core.connect(device.id).await.unwrap();
    core
}

#[cfg(feature = "synthetic-backend")]
#[tokio::test]
async fn candidate_observation_retains_binding_and_never_exposes_payload_or_mapping() {
    let core = connected().await;
    let sub = core.subscribe_input().await.unwrap();
    core.emit_synthetic_input_case(SyntheticInputCase::CandidateBindings)
        .await
        .unwrap();
    let report = capture(&sub, Duration::from_millis(30)).await;
    let events = report["events"].as_array().unwrap();
    assert_eq!(events.len(), 8);
    assert_eq!(events[0]["class"], "candidate_pressed");
    assert_eq!(events[0]["binding_id"], "AG00");
    assert_eq!(events[1]["class"], "candidate_released");
    assert_eq!(report["physical_mapping_verified"], false);
    assert_eq!(report["action_eligible"], false);
    assert!(!report.to_string().contains("raw"));
    core.shutdown().await;
}

#[cfg(feature = "synthetic-backend")]
#[tokio::test]
async fn unknown_silence_close_and_disconnect_are_distinct_unconfirmed_terminals() {
    let core = connected().await;
    let sub = core.subscribe_input().await.unwrap();
    core.emit_synthetic_input_case(SyntheticInputCase::UnsupportedVariants)
        .await
        .unwrap();
    let report = capture(&sub, Duration::from_millis(30)).await;
    assert_eq!(report["event_count"], 5);
    assert!(report["events"]
        .as_array()
        .unwrap()
        .iter()
        .all(|e| e["class"] == "unknown_notification" && e["binding_id"].is_null()));
    assert!(!report.to_string().contains("private payload"));
    assert!(sub.next_event().await.is_none());
    let silent = core.subscribe_input().await.unwrap();
    let report = capture(&silent, Duration::from_millis(5)).await;
    assert_eq!(report["terminal"], "window_elapsed");
    assert_eq!(report["outcome"], "silence_unconfirmed");
    let closed = core.subscribe_input().await.unwrap();
    closed.close();
    assert_eq!(
        capture(&closed, Duration::from_secs(1)).await["terminal"],
        "stream_closed"
    );
    let disconnected = core.subscribe_input().await.unwrap();
    core.disconnect().await;
    assert_eq!(
        capture(&disconnected, Duration::from_secs(1)).await["terminal"],
        "stream_closed"
    );
    core.shutdown().await;
}

#[cfg(feature = "synthetic-backend")]
#[tokio::test]
async fn overflow_stops_capture_and_does_not_replay_backlog() {
    let core = connected().await;
    let sub = core.subscribe_input().await.unwrap();
    core.emit_synthetic_input(1024).await.unwrap();
    // Let the unique production reader drain into the bounded broadcast queue.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let report = capture(&sub, Duration::from_secs(1)).await;
    assert_eq!(report["terminal"], "input_stream_gap");
    assert_eq!(report["events"][0]["class"], "input_stream_gap");
    assert!(report["events"][0]["lost_count"].as_u64().unwrap() > 0);
    assert!(sub.next_event().await.is_none());
    core.shutdown().await;
}

#[test]
fn native_options_require_exact_opt_in_and_a_bounded_window() {
    assert!(options(None, Some("test-1"), Some("45")).is_err());
    assert!(options(Some("yes"), Some("test-1"), Some("45")).is_err());
    assert!(options(Some(OPT_IN), Some("test-1"), Some("46")).is_err());
    assert!(options(Some(OPT_IN), Some("../private"), Some("30")).is_err());
    assert_eq!(
        options(Some(OPT_IN), Some("test-1"), Some("45")).unwrap().1,
        45
    );
}

#[tokio::test]
#[ignore = "requires scripts/observe-input --observe-once; one supervised read-only hardware experiment"]
async fn native_input_observation() {
    use serde_json::json;
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let operator = std::env::var("MI_INPUT_OBSERVATION_OPERATOR").ok();
    let experiment = std::env::var("MI_INPUT_OBSERVATION_ID").ok();
    let seconds = std::env::var("MI_INPUT_OBSERVATION_SECONDS").ok();
    // Authorization validation precedes constructing a transport, enumeration or open.
    let (experiment, seconds) = options(
        operator.as_deref(),
        experiment.as_deref(),
        seconds.as_deref(),
    )
    .expect("native observation not authorized");
    let path = std::env::var("MI_INPUT_OBSERVATION_REPORT").expect("report path absent");
    // Reserve a private report before opening the device; never overwrite evidence.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .expect("cannot reserve private report");
    let core = MicroInput::new();
    let mut report = json!({"experiment":experiment,"started_ms":now_ms(),
        "evidence":"E-094-reserved-native-observation", "active_source_verified":false,
        "active_ag_binding_count":null,"active_source_reason":"status-index-to-source-semantics-unverified",
        "physical_mapping_verified":false,"action_eligible":false});
    let setup = tokio::time::timeout(Duration::from_secs(100), async {
        let devices = core.list_devices().await.map_err(|_| "descriptor_query_failed")?;
        if devices.len() != 1 { return Err("single_candidate_required"); }
        let device = &devices[0];
        if device.vid != 0x303a || device.pid != 0x8298 || device.transport != "Usb" {
            return Err("supported_usb_candidate_required");
        }
        report["descriptor"] = json!({"vid":device.vid,"pid":device.pid,
            "transport":"Usb", "usage_pairs":device.usage_pairs.iter().map(|p| json!([p.page,p.usage])).collect::<Vec<_>>()});
        core.connect(device.id.clone()).await.map_err(|_| "connect_failed")?;
        let identity = core.read_identity().await.map_err(|_| "identity_query_failed")?;
        if identity.version.as_deref() != Some("0.6.3") || identity.backend != "hidapi-2.6.7/macOS/numbered-0x06" {
            return Err("verified_backend_and_firmware_required");
        }
        report["identity"] = json!({"session_id":identity.session_id,"firmware":"0.6.3",
            "profile_index":identity.profile_index,"layer_index":identity.layer_index,
            "source_method":if identity.source_method == "device.status" {"device.status"} else {"sys.version"}});
        // Uses the production integrity-checked two-file path, in memory only. No config bytes,
        // names, serial, user binding values or source IDs enter the report.
        drop(core.read_config().await.map_err(|_| "verified_config_read_failed")?);
        report["config_integrity_read_verified"] = json!(true);
        let sub = core.subscribe_input().await.map_err(|_| "subscription_failed")?;
        Ok((sub, identity))
    }).await;
    match setup {
        Ok(Ok((sub, identity))) => {
            println!("READY_FOR_PHYSICAL_INPUT experiment={} start_ms={} window_seconds={} active_source_verified=false", experiment, now_ms(), seconds);
            if std::io::stdout().flush().is_ok() {
                report["capture"] = capture(&sub, Duration::from_secs(seconds)).await;
                // A bounded read-only recheck detects status changes, without claiming source semantics.
                report["status_indices_stable"] =
                    match tokio::time::timeout(Duration::from_secs(15), core.read_identity()).await
                    {
                        Ok(Ok(after)) => json!(
                            identity.session_id == after.session_id
                                && identity.version == after.version
                                && identity.profile_index.is_some()
                                && identity.layer_index.is_some()
                                && identity.profile_index == after.profile_index
                                && identity.layer_index == after.layer_index
                        ),
                        _ => serde_json::Value::Null,
                    };
                report["stage"] = json!("observation_finished");
            } else {
                sub.close();
                report["stage"] = json!("ready_flush_failed");
            }
        }
        Ok(Err(reason)) => {
            report["stage"] = json!("setup_blocked");
            report["blocker"] = json!(reason);
        }
        Err(_) => {
            report["stage"] = json!("setup_timeout");
        }
    }
    let joined = tokio::time::timeout(Duration::from_secs(5), core.shutdown())
        .await
        .is_ok();
    report["shutdown_join_confirmed"] = json!(joined);
    report["finished_ms"] = json!(now_ms());
    serde_json::to_writer_pretty(&mut file, &report).expect("sanitized report write failed");
    file.write_all(b"\n").expect("report newline failed");
    file.sync_all().expect("report sync failed");
    println!(
        "OBSERVATION_FINISHED experiment={} stage={} shutdown_join_confirmed={}",
        experiment, report["stage"], joined
    );
    assert!(joined, "reader shutdown unconfirmed; no automatic retry");
    assert_eq!(
        report["stage"], "observation_finished",
        "setup blocked; inspect sanitized report; no automatic retry"
    );
}
