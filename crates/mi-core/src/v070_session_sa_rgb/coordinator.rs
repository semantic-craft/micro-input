use std::sync::{Arc, Mutex};
use std::time::Instant;

use mi_session::DeviceSession;

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
        let action = guard
            .reconciler
            .reconcile(Instant::now(), target, session.request_count());
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
                match tokio::time::timeout_at(deadline, session_ref.set_sa_rgb(reset)).await {
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
            let next_action = g
                .reconciler
                .flush_due(Instant::now(), session.request_count());
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
                if session.is_disconnected() || session.set_sa_rgb(&chunk.update).await.is_err() {
                    break;
                }
                completed_chunks += 1;
            }
            let mut guard = inner.lock().unwrap();
            if guard.epoch != epoch {
                return;
            }
            let follow_up = guard.reconciler.finish_batch(
                Instant::now(),
                batch.batch_id,
                completed_chunks,
                session.request_count(),
                preempted,
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
