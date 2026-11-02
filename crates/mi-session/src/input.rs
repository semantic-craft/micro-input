//! Community input candidates are diagnostic only (UNKNOWN-07/10). Raw payloads never leave this module.
use crate::DeviceSession;
use mi_proto::HostNotification;
use std::{
    collections::BTreeSet,
    sync::{atomic::Ordering, Arc},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::broadcast;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEventKind {
    UnknownNotification,
    CandidatePressed,
    CandidateReleased,
    Pressed,
    Released,
    InputStreamGap,
}
#[derive(Debug, Clone)]
pub struct InputEvent {
    pub session_id: u64,
    /// Local subscription order, including skipped items; not a firmware sequence.
    pub sequence: u64,
    /// Local receipt time, not a device timestamp.
    pub timestamp_ms: u64,
    pub kind: InputEventKind,
    pub logical_switch: Option<String>,
    /// Community vendor binding name; never an immutable physical scan position.
    pub binding_id: Option<String>,
    pub lost_count: u64,
    pub action_eligible: bool,
}
/// An evidence-backed decoder may supply logical switches. Physical control, keycap coverage
/// and action role must be mapped separately after physical verification (UNKNOWN-10).
/// This seam is not an outbound request or a firmware schema.
pub trait InputDecoder: Send + Sync {
    fn decode(&self, notification: &HostNotification) -> Option<(InputEventKind, String)>;
}
/// Dispatch's public 0.6.2 observation, not current-device or physical-mapping evidence.
/// This accepts only the AG00–AG18 variant; historical ACT/ENC/act=2 remain unknown.
pub struct CandidateInputDecoder;
impl InputDecoder for CandidateInputDecoder {
    fn decode(&self, notification: &HostNotification) -> Option<(InputEventKind, String)> {
        let HostNotification::Unknown(raw) = notification else {
            return None;
        };
        if ["id", "i", "result", "error"]
            .iter()
            .any(|key| raw.get(key).is_some())
        {
            return None;
        }
        let method = candidate_alias(raw, "method", "m")?.as_str()?;
        if method != "v.oai.hid" {
            return None;
        }
        let params = candidate_alias(raw, "params", "p")?.as_object()?;
        let binding = params.get("k")?.as_str()?;
        let digits = binding.strip_prefix("AG")?;
        if digits.len() != 2
            || !digits.bytes().all(|b| b.is_ascii_digit())
            || digits.parse::<u8>().ok()? > 18
        {
            return None;
        }
        let kind = match params.get("act")?.as_i64()? {
            1 => InputEventKind::CandidatePressed,
            0 => InputEventKind::CandidateReleased,
            _ => return None,
        };
        Some((kind, binding.to_owned()))
    }
}
fn candidate_alias<'a>(
    raw: &'a serde_json::Value,
    long: &str,
    short: &str,
) -> Option<&'a serde_json::Value> {
    match (raw.get(long), raw.get(short)) {
        (Some(a), Some(b)) if a != b => None,
        (Some(value), _) | (_, Some(value)) => Some(value),
        _ => None,
    }
}

pub struct UnverifiedInputDecoder;
impl InputDecoder for UnverifiedInputDecoder {
    fn decode(&self, _: &HostNotification) -> Option<(InputEventKind, String)> {
        None
    }
}
pub struct InputMonitor {
    session: Arc<DeviceSession>,
    receiver: broadcast::Receiver<HostNotification>,
    decoder: Box<dyn InputDecoder>,
    sequence: u64,
    stopped: bool,
    pressed: BTreeSet<String>,
}
impl DeviceSession {
    pub fn subscribe_input(self: &Arc<Self>) -> InputMonitor {
        self.subscribe_input_with_decoder(Box::new(CandidateInputDecoder))
    }
    pub fn subscribe_input_with_decoder(
        self: &Arc<Self>,
        decoder: Box<dyn InputDecoder>,
    ) -> InputMonitor {
        InputMonitor {
            session: self.clone(),
            receiver: self.notifications(),
            decoder,
            sequence: 0,
            stopped: false,
            pressed: BTreeSet::new(),
        }
    }
}
impl InputMonitor {
    pub fn pressed_switches(&self) -> Vec<String> {
        self.pressed.iter().cloned().collect()
    }
    pub fn stopped(&self) -> bool {
        self.stopped
    }
    pub fn close(&mut self) {
        self.stopped = true;
        self.pressed.clear();
    }
    /// Overflow is terminal for this subscription. Recovery creates a new subscription and
    /// empty state; no old buffered notification can resume delivery after a gap.
    pub async fn next(&mut self) -> Option<InputEvent> {
        if self.stopped || self.session.invalid.load(Ordering::SeqCst) {
            self.close();
            return None;
        }
        let changed = self.session.changed.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        if self.session.invalid.load(Ordering::SeqCst) {
            self.stopped = true;
            self.pressed.clear();
            return None;
        }
        let result = tokio::select! { biased; _ = changed => None, value = self.receiver.recv() => Some(value) };
        if self.session.invalid.load(Ordering::SeqCst) || result.is_none() {
            self.stopped = true;
            self.pressed.clear();
            return None;
        }
        let (kind, logical_switch, binding_id, lost_count) = match result.unwrap() {
            Ok(notification) => {
                self.sequence += 1;
                match self.decoder.decode(&notification) {
                    Some((InputEventKind::Pressed, logical)) => {
                        self.pressed.insert(logical.clone());
                        (InputEventKind::Pressed, Some(logical), None, 0)
                    }
                    Some((InputEventKind::Released, logical)) => {
                        self.pressed.remove(&logical);
                        (InputEventKind::Released, Some(logical), None, 0)
                    }
                    Some((
                        kind @ (InputEventKind::CandidatePressed
                        | InputEventKind::CandidateReleased),
                        binding,
                    )) => (kind, None, Some(binding), 0),
                    _ => (InputEventKind::UnknownNotification, None, None, 0),
                }
            }
            Err(broadcast::error::RecvError::Lagged(count)) => {
                self.sequence += count;
                self.stopped = true;
                self.pressed.clear();
                (InputEventKind::InputStreamGap, None, None, count)
            }
            Err(broadcast::error::RecvError::Closed) => {
                self.stopped = true;
                self.pressed.clear();
                return None;
            }
        };
        Some(InputEvent {
            session_id: self.session.id,
            sequence: self.sequence,
            timestamp_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .min(u64::MAX as u128) as u64,
            kind,
            logical_switch,
            binding_id,
            lost_count,
            // Structural recognition alone never authorizes actions.
            action_eligible: false,
        })
    }
}
