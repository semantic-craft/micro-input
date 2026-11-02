//! Input schemas remain UNKNOWN-07/10. Unknown traffic is counted, never exposed or executed.
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
    pub lost_count: u64,
    pub action_eligible: bool,
}
/// An evidence-backed decoder may supply logical switches. Physical control, keycap coverage
/// and action role must be mapped separately after physical verification (UNKNOWN-10).
/// This seam is not an outbound request or a firmware schema.
pub trait InputDecoder: Send + Sync {
    fn decode(&self, notification: &HostNotification) -> Option<(InputEventKind, String)>;
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
        self.subscribe_input_with_decoder(Box::new(UnverifiedInputDecoder))
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
        let (kind, logical_switch, lost_count, action_eligible) = match result.unwrap() {
            Ok(notification) => {
                self.sequence += 1;
                match self.decoder.decode(&notification) {
                    Some((InputEventKind::Pressed, logical)) => {
                        self.pressed.insert(logical.clone());
                        (InputEventKind::Pressed, Some(logical), 0, true)
                    }
                    Some((InputEventKind::Released, logical)) => {
                        self.pressed.remove(&logical);
                        (InputEventKind::Released, Some(logical), 0, true)
                    }
                    _ => (InputEventKind::UnknownNotification, None, 0, false),
                }
            }
            Err(broadcast::error::RecvError::Lagged(count)) => {
                self.sequence += count;
                self.stopped = true;
                self.pressed.clear();
                (InputEventKind::InputStreamGap, None, count, false)
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
            lost_count,
            action_eligible,
        })
    }
}
