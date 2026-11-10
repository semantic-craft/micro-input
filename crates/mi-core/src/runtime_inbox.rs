use super::HardwareLoopEvent;
use mi_session::InputEventKind;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;

pub(super) const INPUT_CAPACITY: usize = 256;
pub(super) const INPUT_BATCH: usize = 32;
pub(super) const CONTROL_BATCH: usize = 6;

#[derive(Default)]
struct Pending {
    session: Option<u64>,
    input: VecDeque<HardwareLoopEvent>,
    source_closed: bool,
    closed: bool,
    gap: Option<HardwareLoopEvent>,
    disconnected: Option<HardwareLoopEvent>,
    discovery: Option<HardwareLoopEvent>,
    ownership: Option<HardwareLoopEvent>,
    acceptance: Option<HardwareLoopEvent>,
    candidates: Option<HardwareLoopEvent>,
}

#[derive(Clone, Default)]
pub(super) struct RuntimeInbox {
    pending: Arc<Mutex<Pending>>,
    ready: Arc<Notify>,
}

impl RuntimeInbox {
    pub(super) fn open_source(&self, session_id: u64) {
        let mut pending = self.pending.lock().unwrap();
        pending.session = Some(session_id);
        pending.input.clear();
        pending.gap = None;
        pending.disconnected = None;
        pending.acceptance = None;
        pending.source_closed = false;
    }

    pub(super) fn source_closed(&self, session_id: u64) -> bool {
        let pending = self.pending.lock().unwrap();
        pending.closed || (pending.session == Some(session_id) && pending.source_closed)
    }

    pub(super) fn close(&self) {
        let mut pending = self.pending.lock().unwrap();
        *pending = Pending {
            closed: true,
            ..Pending::default()
        };
        self.ready.notify_waiters();
    }

    pub(super) fn send(&self, event: HardwareLoopEvent) -> Result<(), ()> {
        let mut pending = self.pending.lock().unwrap();
        if pending.closed {
            return Err(());
        }
        match event {
            HardwareLoopEvent::Input(mut input) => {
                if pending.session != Some(input.session_id) || pending.source_closed {
                    return Ok(());
                }
                if input.kind == InputEventKind::InputStreamGap
                    || pending.input.len() == INPUT_CAPACITY
                {
                    input.lost_count = input.lost_count.saturating_add(
                        pending.input.len() as u64
                            + u64::from(input.kind != InputEventKind::InputStreamGap),
                    );
                    input.kind = InputEventKind::InputStreamGap;
                    pending.input.clear();
                    pending.source_closed = true;
                    pending.gap = Some(HardwareLoopEvent::Input(input));
                } else {
                    pending.input.push_back(HardwareLoopEvent::Input(input));
                }
            }
            HardwareLoopEvent::Disconnected { session_id } => {
                if pending.session != Some(session_id) {
                    return Ok(());
                }
                pending.source_closed = true;
                pending.input.clear();
                pending.disconnected = Some(HardwareLoopEvent::Disconnected { session_id });
            }
            event @ HardwareLoopEvent::HardwareAutoAccepted { session_id, .. } => {
                if pending.session == Some(session_id) {
                    pending.acceptance = Some(event);
                }
            }
            event @ HardwareLoopEvent::DiscoveryChanged { epoch } => {
                if !matches!(&pending.discovery, Some(HardwareLoopEvent::DiscoveryChanged { epoch: old }) if *old > epoch)
                {
                    pending.discovery = Some(event);
                }
            }
            event @ HardwareLoopEvent::LiveCandidatesUpdated { epoch, .. } => {
                if !matches!(&pending.candidates, Some(HardwareLoopEvent::LiveCandidatesUpdated { epoch: old, .. }) if *old > epoch)
                {
                    pending.candidates = Some(event);
                }
            }
            HardwareLoopEvent::OwnershipReply {
                epoch,
                request,
                reply,
            } => {
                let stale = matches!(&pending.ownership, Some(HardwareLoopEvent::OwnershipReply { epoch: old, request: previous, .. }) if (*old, previous.generation) > (epoch, request.generation));
                if !stale {
                    pending.ownership = Some(HardwareLoopEvent::OwnershipReply {
                        epoch,
                        request,
                        reply,
                    });
                }
            }
        }
        drop(pending);
        self.ready.notify_one();
        Ok(())
    }

    pub(super) fn take_batch(&self) -> Vec<HardwareLoopEvent> {
        let mut pending = self.pending.lock().unwrap();
        let mut batch = Vec::with_capacity(CONTROL_BATCH + INPUT_BATCH);
        for event in [
            pending.gap.take(),
            pending.disconnected.take(),
            pending.ownership.take(),
            pending.acceptance.take(),
            pending.discovery.take(),
            pending.candidates.take(),
        ]
        .into_iter()
        .flatten()
        {
            batch.push(event);
        }
        for _ in 0..INPUT_BATCH {
            let Some(event) = pending.input.pop_front() else {
                break;
            };
            batch.push(event);
        }
        batch
    }

    pub(super) async fn recv(&self) -> HardwareLoopEvent {
        loop {
            let ready = self.ready.notified();
            {
                let mut pending = self.pending.lock().unwrap();
                if let Some(event) = pending
                    .gap
                    .take()
                    .or_else(|| pending.disconnected.take())
                    .or_else(|| pending.ownership.take())
                    .or_else(|| pending.acceptance.take())
                    .or_else(|| pending.discovery.take())
                    .or_else(|| pending.candidates.take())
                    .or_else(|| pending.input.pop_front())
                {
                    return event;
                }
            }
            ready.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mi_session::InputEvent;

    fn input(session_id: u64) -> HardwareLoopEvent {
        HardwareLoopEvent::Input(InputEvent {
            session_id,
            board_layer: None,
            sequence: 1,
            ingress_sequence: 1,
            radial_center: None,
            radial_position: None,
            timestamp_ms: 0,
            kind: InputEventKind::Pressed,
            logical_switch: None,
            binding_id: Some("AG11".into()),
            physical_control: None,
            keycap_group: None,
            action_role: None,
            lost_count: 0,
            action_eligible: true,
        })
    }

    #[test]
    fn overflow_discards_backlog_and_delivers_loss_disconnect_and_control() {
        let inbox = RuntimeInbox::default();
        inbox.open_source(7);
        for _ in 0..INPUT_CAPACITY {
            inbox.send(input(7)).unwrap();
        }
        let mut owner = mi_bridge::NativeOwnershipController::new_isolated();
        let request = owner.begin_request(true);
        inbox
            .send(HardwareLoopEvent::OwnershipReply {
                epoch: 4,
                request: request.clone(),
                reply: None,
            })
            .unwrap();
        inbox.send(input(7)).unwrap();
        inbox
            .send(HardwareLoopEvent::Disconnected { session_id: 7 })
            .unwrap();
        let batch = inbox.take_batch();
        assert_eq!(batch.len(), 3);
        assert!(
            matches!(&batch[0], HardwareLoopEvent::Input(event) if event.kind == InputEventKind::InputStreamGap && event.lost_count == INPUT_CAPACITY as u64 + 1)
        );
        assert!(matches!(
            batch[1],
            HardwareLoopEvent::Disconnected { session_id: 7 }
        ));
        assert!(
            matches!(&batch[2], HardwareLoopEvent::OwnershipReply { request: received, .. } if *received == request)
        );
        inbox.send(input(7)).unwrap();
        assert!(
            inbox.take_batch().is_empty(),
            "consuming gap never reopens the lost source"
        );
    }

    #[test]
    fn input_batches_are_bounded_and_old_sources_cannot_close_a_replacement() {
        let inbox = RuntimeInbox::default();
        inbox.open_source(1);
        inbox.send(input(1)).unwrap();
        inbox.open_source(2);
        inbox
            .send(HardwareLoopEvent::Disconnected { session_id: 1 })
            .unwrap();
        inbox.send(input(1)).unwrap();
        for _ in 0..INPUT_CAPACITY {
            inbox.send(input(2)).unwrap();
        }
        for _ in 0..INPUT_CAPACITY / INPUT_BATCH {
            let batch = inbox.take_batch();
            assert_eq!(batch.len(), INPUT_BATCH);
            assert!(batch.iter().all(|event| matches!(event, HardwareLoopEvent::Input(input) if input.session_id == 2 && input.kind == InputEventKind::Pressed)));
        }
        assert!(inbox.take_batch().is_empty());
    }

    #[test]
    fn control_updates_coalesce_without_reviving_an_old_epoch() {
        let inbox = RuntimeInbox::default();
        for epoch in 0..1000 {
            inbox
                .send(HardwareLoopEvent::DiscoveryChanged { epoch })
                .unwrap();
            inbox
                .send(HardwareLoopEvent::LiveCandidatesUpdated {
                    epoch,
                    candidates: Vec::new(),
                })
                .unwrap();
        }
        inbox
            .send(HardwareLoopEvent::DiscoveryChanged { epoch: 0 })
            .unwrap();
        let batch = inbox.take_batch();
        assert_eq!(batch.len(), 2);
        assert!(matches!(
            batch[0],
            HardwareLoopEvent::DiscoveryChanged { epoch: 999 }
        ));
        assert!(matches!(
            batch[1],
            HardwareLoopEvent::LiveCandidatesUpdated { epoch: 999, .. }
        ));
        inbox.close();
        assert!(
            inbox.send(input(2)).is_err(),
            "background publishers terminate after owner teardown"
        );
    }
}
