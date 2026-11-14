use super::HardwareLoopEvent;
use mi_session::InputEventKind;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;

pub(super) const INPUT_CAPACITY: usize = 256;
pub(super) const INPUT_BATCH: usize = 32;
pub(super) const CONTROL_BATCH: usize = 7;
pub(super) const CHEAT_SHEET_NOTIF_CAPACITY: usize = 16;

#[derive(Default)]
struct Pending {
    session: Option<u64>,
    input: VecDeque<HardwareLoopEvent>,
    cheat_sheet_notifications: VecDeque<HardwareLoopEvent>,
    source_closed: bool,
    closed: bool,
    gap: Option<HardwareLoopEvent>,
    disconnected: Option<HardwareLoopEvent>,
    discovery: Option<HardwareLoopEvent>,
    acceptance: Option<HardwareLoopEvent>,
    candidates: Option<HardwareLoopEvent>,
    v070_capability: Option<HardwareLoopEvent>,
    v070_profile_layer: Option<HardwareLoopEvent>,
}

#[derive(Clone, Default)]
pub(crate) struct RuntimeInbox {
    pending: Arc<Mutex<Pending>>,
    ready: Arc<Notify>,
}

impl RuntimeInbox {
    pub(super) fn open_source(&self, session_id: u64) {
        let mut pending = self.pending.lock().unwrap();
        pending.session = Some(session_id);
        pending.input.clear();
        pending.cheat_sheet_notifications.clear();
        pending.gap = None;
        pending.disconnected = None;
        pending.acceptance = None;
        pending.v070_capability = None;
        pending.v070_profile_layer = None;
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

    pub(crate) fn send(&self, event: HardwareLoopEvent) -> Result<(), ()> {
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
            event @ HardwareLoopEvent::V070CapabilityOutcome { seq, .. } => {
                let replace = match &pending.v070_capability {
                    Some(HardwareLoopEvent::V070CapabilityOutcome { seq: old, .. }) => {
                        *old == 0 || (seq != 0 && seq >= *old)
                    }
                    _ => true,
                };
                if replace {
                    pending.v070_capability = Some(event);
                }
            }
            event @ HardwareLoopEvent::V070ProfileLayerChanged { .. } => {
                pending.v070_profile_layer = Some(event);
            }
            event @ HardwareLoopEvent::CheatSheetNotification { .. } => {
                if pending.cheat_sheet_notifications.len() >= CHEAT_SHEET_NOTIF_CAPACITY {
                    pending.cheat_sheet_notifications.pop_front();
                }
                pending.cheat_sheet_notifications.push_back(event);
            }
        }
        drop(pending);
        self.ready.notify_one();
        Ok(())
    }

    pub(super) fn take_batch(&self) -> Vec<HardwareLoopEvent> {
        let mut pending = self.pending.lock().unwrap();
        let mut batch = Vec::with_capacity(
            CONTROL_BATCH + INPUT_BATCH + pending.cheat_sheet_notifications.len(),
        );
        for event in [
            pending.gap.take(),
            pending.disconnected.take(),
            pending.acceptance.take(),
            pending.discovery.take(),
            pending.candidates.take(),
            pending.v070_capability.take(),
            pending.v070_profile_layer.take(),
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
        batch.extend(pending.cheat_sheet_notifications.drain(..));
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
                    .or_else(|| pending.acceptance.take())
                    .or_else(|| pending.discovery.take())
                    .or_else(|| pending.candidates.take())
                    .or_else(|| pending.v070_capability.take())
                    .or_else(|| pending.v070_profile_layer.take())
                    .or_else(|| pending.input.pop_front())
                    .or_else(|| pending.cheat_sheet_notifications.pop_front())
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
        inbox
            .send(HardwareLoopEvent::DiscoveryChanged { epoch: 4 })
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
        assert!(matches!(
            batch[2],
            HardwareLoopEvent::DiscoveryChanged { epoch: 4 }
        ));
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
                    catalog: None,
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

    #[test]
    fn v070_capability_coalescing_preserves_command_outcomes_over_snapshots() {
        let inbox = RuntimeInbox::default();
        let outcome = |seq| HardwareLoopEvent::V070CapabilityOutcome {
            session_id: 1,
            seq,
            result: Ok(crate::firmware_update::V070BridgeOutcome::ProfileLayer {
                profile_index: Some(1),
                layer_index: 2,
            }),
        };

        inbox.send(outcome(1)).unwrap();
        inbox.send(outcome(0)).unwrap();
        let batch = inbox.take_batch();
        assert_eq!(batch.len(), 1);
        assert!(matches!(
            batch[0],
            HardwareLoopEvent::V070CapabilityOutcome { seq: 1, .. }
        ));

        inbox.send(outcome(1)).unwrap();
        inbox.send(outcome(2)).unwrap();
        let batch = inbox.take_batch();
        assert_eq!(batch.len(), 1);
        assert!(matches!(
            batch[0],
            HardwareLoopEvent::V070CapabilityOutcome { seq: 2, .. }
        ));

        inbox.send(outcome(0)).unwrap();
        inbox.send(outcome(1)).unwrap();
        let batch = inbox.take_batch();
        assert_eq!(batch.len(), 1);
        assert!(matches!(
            batch[0],
            HardwareLoopEvent::V070CapabilityOutcome { seq: 1, .. }
        ));
    }

    #[test]
    fn cheat_sheet_notifications_bypass_session_filter_preserve_fifo_and_bound_at_16() {
        let inbox = RuntimeInbox::default();
        inbox.open_source(1);

        for (seq, kind) in [
            (10, crate::CheatSheetNotificationKind::Show),
            (11, crate::CheatSheetNotificationKind::Hide),
            (12, crate::CheatSheetNotificationKind::Toggle),
        ] {
            inbox
                .send(HardwareLoopEvent::CheatSheetNotification {
                    session_id: 99,
                    ingress_sequence: seq,
                    kind,
                })
                .unwrap();
        }
        let batch = inbox.take_batch();
        assert_eq!(batch.len(), 3);
        let kinds: Vec<_> = batch
            .iter()
            .map(|e| match e {
                HardwareLoopEvent::CheatSheetNotification {
                    session_id,
                    ingress_sequence,
                    kind,
                } => (*session_id, *ingress_sequence, *kind),
                other => panic!("unexpected event: {other:?}"),
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                (99, 10, crate::CheatSheetNotificationKind::Show),
                (99, 11, crate::CheatSheetNotificationKind::Hide),
                (99, 12, crate::CheatSheetNotificationKind::Toggle),
            ]
        );

        for seq in 0..20_u64 {
            inbox
                .send(HardwareLoopEvent::CheatSheetNotification {
                    session_id: 7,
                    ingress_sequence: seq,
                    kind: crate::CheatSheetNotificationKind::Show,
                })
                .unwrap();
        }
        let bounded = inbox.take_batch();
        assert_eq!(bounded.len(), CHEAT_SHEET_NOTIF_CAPACITY);
        let seqs: Vec<u64> = bounded
            .iter()
            .map(|e| match e {
                HardwareLoopEvent::CheatSheetNotification {
                    ingress_sequence, ..
                } => *ingress_sequence,
                other => panic!("unexpected event: {other:?}"),
            })
            .collect();
        assert_eq!(seqs, (4..20).collect::<Vec<_>>());

        inbox
            .send(HardwareLoopEvent::CheatSheetNotification {
                session_id: 7,
                ingress_sequence: 100,
                kind: crate::CheatSheetNotificationKind::Show,
            })
            .unwrap();
        inbox.open_source(2);
        assert!(inbox.take_batch().is_empty());
    }
}
