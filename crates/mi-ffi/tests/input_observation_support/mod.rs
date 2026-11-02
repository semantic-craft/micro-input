//! Test-only acceptance driver. Never exposes raw notifications or configuration.
use mi_ffi::{InputKind, InputSubscription};
use serde_json::{json, Value};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub async fn capture(sub: &InputSubscription, duration: Duration) -> Value {
    let started = now_ms();
    let deadline = tokio::time::Instant::now() + duration.min(Duration::from_secs(45));
    let mut events = Vec::new();
    let terminal = loop {
        match tokio::time::timeout_at(deadline, sub.next_event()).await {
            Err(_) => break "window_elapsed",
            Ok(None) => break "stream_closed",
            Ok(Some(event)) => {
                let class = match event.kind {
                    InputKind::CandidatePressed => "candidate_pressed",
                    InputKind::CandidateReleased => "candidate_released",
                    InputKind::UnknownNotification => "unknown_notification",
                    InputKind::InputStreamGap => "input_stream_gap",
                    // A future decoder must not silently promote this observation tool.
                    _ => "unverified_input",
                };
                let gap = event.kind == InputKind::InputStreamGap;
                events.push(json!({"class":class,"binding_id":event.binding_id,
                    "session_id":event.session_id,"sequence":event.sequence,
                    "timestamp_ms":event.timestamp_ms,"lost_count":event.lost_count}));
                if gap {
                    break "input_stream_gap";
                }
                if events.len() == 2048 {
                    break "event_limit";
                }
            }
        }
    };
    sub.close();
    let count = events.len();
    json!({"capture_started_ms":started,"capture_finished_ms":now_ms(),
        "terminal":terminal,"event_count":count,"events":events,
        "outcome":if count == 0 {"silence_unconfirmed"} else {"notifications_observed_mapping_unverified"},
        "physical_mapping_verified":false,"action_eligible":false})
}

pub const OPT_IN: &str = "I_AUTHORIZE_ONE_READONLY_INPUT_OBSERVATION";
pub fn options(
    operator: Option<&str>,
    experiment: Option<&str>,
    seconds: Option<&str>,
) -> Result<(String, u64), &'static str> {
    if operator != Some(OPT_IN) {
        return Err("explicit observation opt-in absent");
    }
    let id = experiment.ok_or("experiment absent")?;
    if id.is_empty() || id.len() > 64 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err("invalid experiment identifier");
    }
    let seconds = seconds
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| (1..=45).contains(v))
        .ok_or("capture window must be 1..45 seconds")?;
    Ok((id.into(), seconds))
}
