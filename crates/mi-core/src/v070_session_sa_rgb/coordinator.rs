use std::sync::{Arc, Mutex};
use std::time::Instant;

use mi_session::{DeviceSession, SessionError};
use mi_transport::budget::WritePriority;

use super::{
    DesiredTarget, DispatchedSaRgbBatch, ManualSaRgbWrite, ReconcileAction, SaRgbChunkKind,
    SessionSaRgbReconciler, MAX_TEARDOWN_REQUEST_ID, SESSION_SA_RGB_TEARDOWN_TIMEOUT,
};

struct CoordinatorInner {
    reconciler: SessionSaRgbReconciler,
    scheduled_wake: Option<(Instant, tokio::task::AbortHandle)>,
    epoch: u64,
    bound_session_id: Option<u64>,
}

impl CoordinatorInner {
    fn cancel_wake(&mut self) {
        if let Some((_, handle)) = self.scheduled_wake.take() {
            handle.abort();
        }
    }
}

pub(crate) struct SaRgbRuntimeCoordinator {
    inner: Arc<Mutex<CoordinatorInner>>,
}

impl Default for SaRgbRuntimeCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl SaRgbRuntimeCoordinator {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(CoordinatorInner {
                reconciler: SessionSaRgbReconciler::new(),
                scheduled_wake: None,
                epoch: 0,
                bound_session_id: None,
            })),
        }
    }

    #[cfg(test)]
    pub fn has_dirty_keys(&self) -> bool {
        self.inner.lock().unwrap().reconciler.has_dirty_keys()
    }

    #[cfg(test)]
    pub fn is_in_flight(&self) -> bool {
        self.inner.lock().unwrap().reconciler.in_flight.is_some()
    }

    pub fn note_manual_sa_rgb_write(
        &self,
        session_id: u64,
        write: ManualSaRgbWrite,
        sync_enabled: bool,
    ) {
        let mut guard = self.inner.lock().unwrap();
        if guard.bound_session_id != Some(session_id) {
            if guard.bound_session_id.is_some() {
                guard.epoch = guard.epoch.wrapping_add(1);
                guard.cancel_wake();
                guard.reconciler.reset_for_new_session();
            }
            guard.bound_session_id = Some(session_id);
        }
        guard
            .reconciler
            .note_manual_sa_rgb_write(write, sync_enabled);
    }

    pub fn clear_on_disconnect(&self) {
        let mut guard = self.inner.lock().unwrap();
        guard.epoch = guard.epoch.wrapping_add(1);
        guard.bound_session_id = None;
        guard.cancel_wake();
        guard.reconciler.clear_on_disconnect();
    }

    pub fn sync(
        &self,
        target: DesiredTarget,
        session: Option<Arc<DeviceSession>>,
        runtime: &Arc<tokio::runtime::Runtime>,
    ) {
        let mut guard = self.inner.lock().unwrap();
        let Some(session) = session else {
            guard.epoch = guard.epoch.wrapping_add(1);
            guard.cancel_wake();
            guard.reconciler.park_without_session();
            return;
        };
        if guard.bound_session_id != Some(session.id) {
            guard.epoch = guard.epoch.wrapping_add(1);
            guard.cancel_wake();
            guard.reconciler.reset_for_new_session();
            guard.bound_session_id = Some(session.id);
        }
        let now = Instant::now();
        let priority = match &target {
            DesiredTarget::UrgentClear(_) => WritePriority::UrgentClear,
            DesiredTarget::Active(_) | DesiredTarget::Inactive => WritePriority::BackgroundSync,
        };
        let budget_ready_at = session.write_budget().next_ready_at(now, priority);
        let action = guard.reconciler.reconcile_with_budget(
            now,
            target,
            session.request_count(),
            budget_ready_at,
        );
        Self::apply_action_locked(&self.inner, &mut guard, action, session, runtime.handle());
    }

    pub fn teardown_blocking(
        &self,
        runtime: &Arc<tokio::runtime::Runtime>,
        session: Option<Arc<DeviceSession>>,
    ) {
        let (teardown_epoch, resets) = {
            let mut guard = self.inner.lock().unwrap();
            guard.epoch = guard.epoch.wrapping_add(1);
            guard.cancel_wake();
            (guard.epoch, guard.reconciler.take_dirty_for_teardown())
        };
        if resets.is_empty() {
            return;
        }
        let Some(session) =
            session.filter(|s| !s.is_disconnected() && s.has_exclusive_vendor_link())
        else {
            return;
        };
        let session_ref = Arc::clone(&session);
        let resets_for_task = resets.clone();
        let fut = async move {
            let deadline = tokio::time::Instant::now() + SESSION_SA_RGB_TEARDOWN_TIMEOUT;
            let mut done = 0usize;
            for reset in &resets_for_task {
                if session_ref.is_disconnected()
                    || session_ref.request_count() >= MAX_TEARDOWN_REQUEST_ID
                {
                    break;
                }
                match tokio::time::timeout_at(
                    deadline,
                    session_ref.set_sa_rgb_with_priority(reset, WritePriority::UrgentClear),
                )
                .await
                {
                    Ok(Ok(_)) => done += 1,
                    Ok(Err(_)) | Err(_) => break,
                }
            }
            done
        };
        let confirmed_count = if tokio::runtime::Handle::try_current().is_ok() {
            let rt = Arc::clone(runtime);
            std::thread::spawn(move || rt.block_on(fut))
                .join()
                .unwrap_or(0)
        } else {
            runtime.block_on(fut)
        };
        if confirmed_count < resets.len() && !session.is_disconnected() {
            let mut guard = self.inner.lock().unwrap();
            if guard.epoch == teardown_epoch
                && guard.bound_session_id.is_none_or(|id| id == session.id)
            {
                guard.bound_session_id = Some(session.id);
                guard
                    .reconciler
                    .restore_unconfirmed_teardown(&resets[confirmed_count..]);
            }
        }
    }

    fn apply_action_locked(
        inner_arc: &Arc<Mutex<CoordinatorInner>>,
        guard: &mut CoordinatorInner,
        action: ReconcileAction,
        session: Arc<DeviceSession>,
        handle: &tokio::runtime::Handle,
    ) {
        match action {
            ReconcileAction::NoOp => {
                if guard.reconciler.in_flight.is_none() {
                    guard.cancel_wake();
                }
            }
            ReconcileAction::ScheduleWake(wake_at) => {
                Self::arm_wake_locked(inner_arc, guard, wake_at, session, handle);
            }
            ReconcileAction::Dispatch(batch) => {
                guard.cancel_wake();
                let epoch = guard.epoch;
                let inner_clone = Arc::clone(inner_arc);
                let handle_clone = handle.clone();
                handle.spawn(async move {
                    Self::drive_batch_loop(inner_clone, epoch, session, batch, handle_clone).await;
                });
            }
        }
    }

    fn arm_wake_locked(
        inner_arc: &Arc<Mutex<CoordinatorInner>>,
        guard: &mut CoordinatorInner,
        wake_at: Instant,
        session: Arc<DeviceSession>,
        handle: &tokio::runtime::Handle,
    ) {
        if guard
            .scheduled_wake
            .as_ref()
            .is_some_and(|(existing, _)| *existing == wake_at)
        {
            return;
        }
        guard.cancel_wake();
        let epoch = guard.epoch;
        let inner_clone = Arc::clone(inner_arc);
        let handle_clone = handle.clone();
        let join = handle.spawn(async move {
            tokio::time::sleep_until(tokio::time::Instant::from_std(wake_at)).await;
            if session.is_disconnected() {
                return;
            }
            let mut g = inner_clone.lock().unwrap();
            if g.epoch != epoch {
                return;
            }
            g.scheduled_wake = None;
            let now = Instant::now();
            let priority = g.reconciler.pending_write_priority();
            let budget_ready_at = session.write_budget().next_ready_at(now, priority);
            let next_action =
                g.reconciler
                    .flush_due_with_budget(now, session.request_count(), budget_ready_at);
            Self::apply_action_locked(&inner_clone, &mut g, next_action, session, &handle_clone);
        });
        guard.scheduled_wake = Some((wake_at, join.abort_handle()));
    }

    async fn drive_batch_loop(
        inner: Arc<Mutex<CoordinatorInner>>,
        epoch: u64,
        session: Arc<DeviceSession>,
        mut batch: DispatchedSaRgbBatch,
        handle: tokio::runtime::Handle,
    ) {
        loop {
            let mut completed_chunks = 0usize;
            let mut preempted = false;
            let mut rate_limited = false;
            let batch_priority = {
                let guard = inner.lock().unwrap();
                if guard.reconciler.last_target_was_urgent_clear
                    && batch
                        .chunks
                        .iter()
                        .all(|chunk| matches!(chunk.kind, SaRgbChunkKind::Reset(_)))
                {
                    WritePriority::UrgentClear
                } else {
                    WritePriority::BackgroundSync
                }
            };
            for chunk in &batch.chunks {
                {
                    let guard = inner.lock().unwrap();
                    if guard.epoch != epoch {
                        return;
                    }
                    if guard.reconciler.urgent_clear_pending
                        && matches!(chunk.kind, SaRgbChunkKind::Set { .. })
                    {
                        preempted = true;
                        break;
                    }
                }
                if session.is_disconnected() {
                    break;
                }
                match session
                    .set_sa_rgb_with_priority(&chunk.update, batch_priority)
                    .await
                {
                    Ok(()) => completed_chunks += 1,
                    Err(SessionError::DeviceRateLimited { .. }) => {
                        rate_limited = true;
                        break;
                    }
                    Err(_) => break,
                }
            }
            let mut guard = inner.lock().unwrap();
            if guard.epoch != epoch {
                return;
            }
            let now = Instant::now();
            let next_priority = guard.reconciler.pending_write_priority();
            let budget_ready_at = session.write_budget().next_ready_at(now, next_priority);
            let follow_up = guard.reconciler.finish_batch_with_budget(
                now,
                batch.batch_id,
                completed_chunks,
                session.request_count(),
                preempted,
                rate_limited,
                budget_ready_at,
            );
            match follow_up {
                ReconcileAction::Dispatch(next_batch) => {
                    guard.cancel_wake();
                    batch = next_batch;
                }
                other => {
                    Self::apply_action_locked(&inner, &mut guard, other, session, &handle);
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::future::Future;
    use std::pin::Pin;
    use std::time::Duration;

    use mi_model::SaId;
    use mi_proto::{Framer, ProtoError};
    use mi_session::{ReadOnlySchema, SessionConfig};
    use mi_transport::{HidLink, LinkBackend, TransportError};
    use serde_json::{json, Value};

    use super::*;
    use crate::session_strip::FourAppSessionKind;
    use crate::v070_session_sa_rgb::{
        map_session_kind_to_sa_lighting, DesiredSaLighting, SESSION_SA_RGB_MIN_INTERVAL,
    };

    struct IdentityFramer;

    impl Framer for IdentityFramer {
        fn encode(&self, payload: &[u8]) -> Result<Vec<Vec<u8>>, ProtoError> {
            Ok(vec![payload.to_vec()])
        }

        fn feed(&mut self, report: &[u8]) -> Result<Vec<Vec<u8>>, ProtoError> {
            Ok(vec![report.to_vec()])
        }

        fn clear(&mut self) {}
    }

    struct GatedSaRgbLink {
        in_tx: Mutex<Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>>,
        in_rx: tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>>,
        out_tx: tokio::sync::mpsc::UnboundedSender<Value>,
        sa_rgb_count: Mutex<usize>,
    }

    impl GatedSaRgbLink {
        fn new() -> (Arc<Self>, tokio::sync::mpsc::UnboundedReceiver<Value>) {
            let (in_tx, in_rx) = tokio::sync::mpsc::unbounded_channel();
            let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel();
            (
                Arc::new(Self {
                    in_tx: Mutex::new(Some(in_tx)),
                    in_rx: tokio::sync::Mutex::new(in_rx),
                    out_tx,
                    sa_rgb_count: Mutex::new(0),
                }),
                out_rx,
            )
        }

        fn reply(&self, req: &Value, result: Value) {
            if let Some(tx) = self.in_tx.lock().unwrap().as_ref() {
                let wire = json!({
                    "id": req["id"],
                    "method": req["method"],
                    "result": result,
                });
                let _ = tx.send(serde_json::to_vec(&wire).unwrap());
            }
        }
    }

    impl HidLink for GatedSaRgbLink {
        fn is_exclusive_vendor(&self) -> bool {
            true
        }

        fn backend(&self) -> LinkBackend {
            LinkBackend::MacHidapi267Numbered
        }

        fn write<'a, 'c, 'b>(
            &'a self,
            report: &'c [u8],
        ) -> Pin<Box<dyn Future<Output = Result<(), TransportError>> + Send + 'b>>
        where
            'a: 'b,
            'c: 'b,
            Self: 'b,
        {
            Box::pin(async move {
                let req: Value = serde_json::from_slice(report).unwrap();
                if req.get("method").and_then(Value::as_str) == Some("kb.sa.rgb") {
                    *self.sa_rgb_count.lock().unwrap() += 1;
                }
                let _ = self.out_tx.send(req);
                Ok(())
            })
        }

        fn read<'a, 'b>(
            &'a self,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, TransportError>> + Send + 'b>>
        where
            'a: 'b,
            Self: 'b,
        {
            Box::pin(async move {
                let mut rx = self.in_rx.lock().await;
                rx.recv().await.ok_or(TransportError::Disconnected)
            })
        }

        fn close(&self) {
            self.in_tx.lock().unwrap().take();
        }
    }

    #[test]
    fn sync_without_session_bumps_epoch_and_aborts_in_flight_multi_chunk_batch() {
        let rt = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap(),
        );
        let (link, mut out_rx) = GatedSaRgbLink::new();
        let session = rt.block_on(async {
            let session = DeviceSession::new(
                link.clone(),
                Box::new(IdentityFramer),
                Arc::new(ReadOnlySchema),
                SessionConfig {
                    timeout: Duration::from_secs(2),
                    interval: Duration::ZERO,
                },
            );
            let reply = async {
                let req = out_rx.recv().await.expect("device.status");
                link.reply(
                    &req,
                    json!({
                        "version": "0.7.0-rc.16",
                        "profile_index": 0,
                        "layer_index": 1,
                    }),
                );
            };
            let (res, _) = tokio::join!(session.read_identity_info(), reply);
            res.unwrap();
            session
        });

        let style = match map_session_kind_to_sa_lighting(FourAppSessionKind::Working) {
            DesiredSaLighting::Override(s) => s,
            DesiredSaLighting::DefaultOnboard => unreachable!(),
        };
        let target: BTreeMap<_, _> = (1..=20)
            .map(|raw| (SaId::new(raw).unwrap(), style))
            .collect();

        let coord = SaRgbRuntimeCoordinator::new();
        coord.sync(
            DesiredTarget::Active(target.clone()),
            Some(Arc::clone(&session)),
            &rt,
        );

        let chunk1_req = rt
            .block_on(async { tokio::time::timeout(Duration::from_secs(1), out_rx.recv()).await })
            .expect("chunk 1 dispatched")
            .expect("chunk 1 request");
        assert_eq!(chunk1_req["method"], "kb.sa.rgb");
        assert!(coord.is_in_flight());

        let epoch_before = coord.inner.lock().unwrap().epoch;
        coord.sync(DesiredTarget::Inactive, None, &rt);
        let epoch_after = coord.inner.lock().unwrap().epoch;
        assert_eq!(epoch_after, epoch_before.wrapping_add(1));
        assert!(!coord.is_in_flight());

        link.reply(&chunk1_req, json!({}));
        let no_second_chunk = rt.block_on(async {
            tokio::time::timeout(Duration::from_millis(80), out_rx.recv()).await
        });
        assert!(
            no_second_chunk.is_err(),
            "in-flight multi-chunk batch must abort after chunk 1 when epoch advances"
        );
        assert_eq!(*link.sa_rgb_count.lock().unwrap(), 1);

        std::thread::sleep(SESSION_SA_RGB_MIN_INTERVAL + Duration::from_millis(15));
        let responder = rt.spawn({
            let link = Arc::clone(&link);
            async move {
                while let Some(req) = out_rx.recv().await {
                    link.reply(&req, json!({}));
                }
            }
        });
        coord.sync(
            DesiredTarget::Active(target),
            Some(Arc::clone(&session)),
            &rt,
        );
        let deadline = Instant::now() + Duration::from_secs(1);
        while *link.sa_rgb_count.lock().unwrap() < 3 {
            assert!(
                Instant::now() < deadline,
                "resuming on same session after park must not stall on stale in_flight"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        responder.abort();
        rt.block_on(session.shutdown());
    }

    #[test]
    fn coordinator_defers_background_dispatch_until_write_budget_ready_and_teardown_uses_urgent_clear(
    ) {
        let rt = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap(),
        );
        let (link, mut out_rx) = GatedSaRgbLink::new();
        let budget = Arc::new(mi_transport::budget::DeviceWriteBudget::new(
            mi_transport::budget::WriteBudgetPolicy {
                window: Duration::from_secs(10),
                max_writes: 40,
                min_spacing: Duration::from_millis(160),
                enabled: true,
            },
        ));
        let session = rt.block_on(async {
            let session = DeviceSession::new_with_budget(
                link.clone(),
                Box::new(IdentityFramer),
                Arc::new(ReadOnlySchema),
                SessionConfig {
                    timeout: Duration::from_secs(2),
                    interval: Duration::ZERO,
                },
                Arc::clone(&budget),
            );
            let reply = async {
                let req = out_rx.recv().await.expect("device.status");
                link.reply(
                    &req,
                    json!({
                        "version": "0.7.0-rc.16",
                        "profile_index": 0,
                        "layer_index": 1,
                    }),
                );
            };
            let (res, _) = tokio::join!(session.read_identity_info(), reply);
            res.unwrap();
            session
        });

        let responder = rt.spawn({
            let link = Arc::clone(&link);
            async move {
                while let Some(req) = out_rx.recv().await {
                    link.reply(&req, json!({}));
                }
            }
        });

        let t0 = Instant::now();
        budget.try_acquire(t0, WritePriority::Normal).unwrap();

        let style = match map_session_kind_to_sa_lighting(FourAppSessionKind::Working) {
            DesiredSaLighting::Override(s) => s,
            DesiredSaLighting::DefaultOnboard => unreachable!(),
        };
        let target: BTreeMap<_, _> = [(SaId::new(101).unwrap(), style)].into_iter().collect();

        let coord = SaRgbRuntimeCoordinator::new();
        coord.sync(
            DesiredTarget::Active(target),
            Some(Arc::clone(&session)),
            &rt,
        );

        assert!(
            !coord.is_in_flight(),
            "coordinator must defer dispatch while DeviceWriteBudget requires spacing wait"
        );
        assert_eq!(
            *link.sa_rgb_count.lock().unwrap(),
            0,
            "no kb.sa.rgb frame may be sent before write_budget.next_ready_at"
        );

        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            *link.sa_rgb_count.lock().unwrap(),
            0,
            "dispatch must remain deferred before the 160ms spacing window elapses"
        );

        let deadline = Instant::now() + Duration::from_secs(1);
        while *link.sa_rgb_count.lock().unwrap() < 1 {
            assert!(
                Instant::now() < deadline,
                "coordinator must wake and dispatch once DeviceWriteBudget becomes ready"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            t0.elapsed() >= Duration::from_millis(140),
            "dispatch occurred earlier than DeviceWriteBudget spacing allowed"
        );

        let wait_confirmed = Instant::now() + Duration::from_secs(1);
        while coord.is_in_flight() {
            assert!(Instant::now() < wait_confirmed);
            std::thread::sleep(Duration::from_millis(5));
        }

        let teardown_start = Instant::now();
        coord.teardown_blocking(&rt, Some(Arc::clone(&session)));
        assert!(
            teardown_start.elapsed() < Duration::from_millis(120),
            "teardown_blocking must use WritePriority::UrgentClear to bypass soft BLE spacing"
        );
        assert_eq!(*link.sa_rgb_count.lock().unwrap(), 2);

        responder.abort();
        rt.block_on(session.shutdown());
    }
}
