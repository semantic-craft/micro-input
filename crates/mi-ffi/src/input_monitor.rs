//! High-level, payload-free input monitoring. No raw RPC/notification crosses this bridge.
use super::{failure, BridgeError, MicroInput};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::{Mutex, Notify};

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum InputKind {
    UnknownNotification,
    Pressed,
    Released,
    InputStreamGap,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct InputEventInfo {
    pub session_id: u64,
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub kind: InputKind,
    pub logical_switch: Option<String>,
    // Distinct axes; UNKNOWN-10 does not authorize guessed IDs or key counts.
    pub physical_control: Option<String>,
    pub keycap_group: Option<String>,
    pub action_role: Option<String>,
    pub lost_count: u64,
    pub verification: String,
    pub action_eligible: bool,
}
#[derive(uniffi::Object)]
pub struct InputSubscription {
    monitor: Mutex<mi_session::InputMonitor>,
    closed: AtomicBool,
    changed: Notify,
}
#[uniffi::export(async_runtime = "tokio")]
impl MicroInput {
    /// Fresh, bounded subscription. Rebuilding discards old backlog and pressed state;
    /// production still cannot provide action events while the input schema is UNKNOWN.
    pub async fn subscribe_input(&self) -> Result<Arc<InputSubscription>, BridgeError> {
        let session = self.current().map_err(failure)?;
        Ok(Arc::new(InputSubscription {
            monitor: Mutex::new(session.subscribe_input()),
            closed: AtomicBool::new(false),
            changed: Notify::new(),
        }))
    }
}
#[uniffi::export(async_runtime = "tokio")]
impl InputSubscription {
    /// Pull delivery keeps the Rust→Swift boundary bounded as well as the reader queue.
    /// Calls after close/gap/disconnect return None; a subscription never auto-restarts.
    pub async fn next_event(&self) -> Option<InputEventInfo> {
        if self.closed.load(Ordering::SeqCst) {
            return None;
        }
        let changed = self.changed.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        let mut monitor = tokio::select! { biased; _ = &mut changed => return None, guard = self.monitor.lock() => guard };
        if self.closed.load(Ordering::SeqCst) {
            monitor.close();
            return None;
        }
        let event = tokio::select! { biased; _ = changed => { monitor.close(); return None }, event = monitor.next() => event }?;
        if self.closed.load(Ordering::SeqCst) {
            monitor.close();
            return None;
        }
        let kind = match event.kind {
            mi_session::InputEventKind::UnknownNotification => InputKind::UnknownNotification,
            mi_session::InputEventKind::Pressed => InputKind::Pressed,
            mi_session::InputEventKind::Released => InputKind::Released,
            mi_session::InputEventKind::InputStreamGap => InputKind::InputStreamGap,
        };
        Some(InputEventInfo {
            session_id: event.session_id,
            sequence: event.sequence,
            timestamp_ms: event.timestamp_ms,
            kind,
            logical_switch: event.logical_switch,
            physical_control: None,
            keycap_group: None,
            action_role: None,
            lost_count: event.lost_count,
            verification: "input-schema-unverified".into(),
            action_eligible: false,
        })
    }
    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.changed.notify_waiters();
        if let Ok(mut monitor) = self.monitor.try_lock() {
            monitor.close();
        }
    }
}
impl Drop for InputSubscription {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(feature = "synthetic-backend")]
#[uniffi::export(async_runtime = "tokio")]
impl MicroInput {
    pub async fn emit_synthetic_input(&self, count: u16) -> Result<(), BridgeError> {
        self.current()?;
        self.transport.emit_synthetic_input(count).map_err(failure)
    }
}
