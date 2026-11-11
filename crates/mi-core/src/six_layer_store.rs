use super::*;
use crate::six_layer_runtime::{self, InstallationStatus};
use mi_model::apply::AppliedSixLayerPlan;

impl WorkbenchStore {
    fn compiled_six_layer_plan(&self) -> Result<AppliedSixLayerPlan, String> {
        self.projection_inputs().compiled_six_layer_plan()
    }

    pub(super) fn confirm_cindy_native_paused(
        &mut self,
        confirmed: bool,
    ) -> Result<(), WorkbenchError> {
        self.require_awake()?;
        if self.six_layer_job.is_some() {
            return Err(WorkbenchError::Routing("六层操作正在进行".into()));
        }
        let mut next = self.six_layer_state.clone();
        next.cindy_native_paused_confirmed = confirmed;
        six_layer_runtime::save(
            &self.config.document_dir.join("six-layer-runtime.json"),
            &next,
        )
        .map_err(WorkbenchError::Storage)?;
        self.six_layer_state = next;
        self.commit_local_update();
        Ok(())
    }

    fn suspend_for_six_layer_operation(&mut self) -> Result<(), WorkbenchError> {
        if let Some(running) = self.six_layer_running.take() {
            if let Err(error) = running.shutdown(&self.tokio_runtime) {
                self.system_lifecycle = SystemLifecycle::CleanupFailed(error.clone());
                return Err(WorkbenchError::Routing(error));
            }
        }
        let stopped = self.live_executor.stop_owned_actions();
        self.finish_host_cleanup(stopped)
            .map_err(WorkbenchError::Routing)?;
        self.dial_pressed = false;
        self.dial_turned_while_pressed = false;
        self.radial_state.cancel();
        if let Some(session) = self.device_session.take() {
            self.tokio_runtime.block_on(session.shutdown());
        }
        if let Some(transport) = self.mac_watcher_transport.take() {
            mi_transport::HidTransport::stop_discovery(transport.as_ref());
        }
        self.connected_candidate = None;
        self.hardware_rpc_verified = false;
        self.board_decoder.store(false, Ordering::Release);
        self.board_routing = BoardRouting::Unverified;
        Ok(())
    }

    pub(super) fn start_six_layer_apply(
        &mut self,
        resume: Option<AppliedSixLayerPlan>,
    ) -> Result<(), WorkbenchError> {
        self.require_awake()?;
        if self.six_layer_job.is_some() {
            return Err(WorkbenchError::Routing("六层操作正在进行".into()));
        }
        let reconnecting = resume.is_some();
        let plan = resume
            .map(Ok)
            .unwrap_or_else(|| self.compiled_six_layer_plan())
            .map_err(|e| WorkbenchError::Routing(format!("六层方案：{e}")))?;
        if self.config.isolated {
            return Err(WorkbenchError::Routing(
                "隔离工作台不能应用实体六层方案".into(),
            ));
        }
        if self
            .six_layer_state
            .installation
            .as_ref()
            .is_some_and(|installation| installation.status != InstallationStatus::Applied)
        {
            return Err(WorkbenchError::Routing(
                "六层恢复记录尚未处理，请先恢复原设备配置".into(),
            ));
        }
        self.suspend_for_six_layer_operation()?;
        let dir = self.config.document_dir.clone();
        let saved = self.six_layer_state.clone();
        let runtime = Arc::clone(&self.tokio_runtime);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = if reconnecting {
                runtime.block_on(six_layer_runtime::resume(&dir, saved))
            } else {
                runtime.block_on(six_layer_runtime::apply(&dir, plan, saved))
            };
            let _ = tx.send(result.map(Some));
        });
        self.six_layer_job = Some(rx);
        self.status_message = "正在准备六层输入并核验设备配置…".into();
        self.commit_runtime_update();
        Ok(())
    }

    pub(super) fn start_six_layer_restore(&mut self) -> Result<(), WorkbenchError> {
        self.require_awake()?;
        if self.six_layer_job.is_some()
            || self.six_layer_state.installation.is_none()
            || self.config.isolated
        {
            return Err(WorkbenchError::Routing(
                "当前没有可执行的六层恢复操作".into(),
            ));
        }
        self.suspend_for_six_layer_operation()?;
        let dir = self.config.document_dir.clone();
        let saved = self.six_layer_state.clone();
        let runtime = Arc::clone(&self.tokio_runtime);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = runtime
                .block_on(six_layer_runtime::restore(&dir, saved))
                .map(|_| None);
            let _ = tx.send(result);
        });
        self.six_layer_job = Some(rx);
        self.status_message = "正在恢复原设备配置，确认后再撤销吞键规则…".into();
        self.commit_runtime_update();
        Ok(())
    }

    pub(super) fn poll_six_layer_runtime(&mut self) {
        let result = self
            .six_layer_job
            .as_ref()
            .and_then(|rx| rx.try_recv().ok());
        if let Some(result) = result {
            self.six_layer_job = None;
            if let Ok(saved) =
                six_layer_runtime::load(&self.config.document_dir.join("six-layer-runtime.json"))
            {
                self.six_layer_state = saved;
            }
            match result {
                Ok(Some(mut running)) => {
                    running.foreground_changed(self.foreground_bundle_id.as_deref());
                    self.device_session = None;
                    self.connected_candidate = Some(running.candidate.clone());
                    self.hardware_rpc_verified = true;
                    running.armed = running.receiver.arm().is_ok();
                    running.set_execution_policy(
                        self.document_store.host_execution_enabled(),
                        self.command_execution_enabled,
                        self.update_now,
                    );
                    running.start_executor(Arc::clone(&self.live_executor));
                    running.start_monitor();
                    self.six_layer_running = Some(running);
                    self.status_message = "六层配置已读回，等待首次实体输入验证".into();
                    self.start_mac_usb_discovery_watcher();
                }
                Ok(None) => {
                    self.status_message = "原设备配置及 Karabiner 设置已恢复".into();
                    self.board_routing = BoardRouting::Legacy;
                }
                Err(error) => self.status_message = error,
            }
            self.commit_runtime_update();
        }
        if self.six_layer_running.is_none() {
            return;
        }
        let running = self.six_layer_running.as_ref().unwrap();
        running.set_execution_policy(
            self.document_store.host_execution_enabled(),
            self.command_execution_enabled,
            self.update_now,
        );
        let mut receipts = Vec::new();
        if let Some(rx) = &running.receipts {
            for _ in 0..runtime_inbox::INPUT_BATCH {
                let Ok(receipt) = rx.try_recv() else {
                    break;
                };
                receipts.push(receipt);
            }
        }
        if self.live_executor.has_unconfirmed_apple_fn_release()
            || receipts
                .iter()
                .any(|receipt| receipt.outcome == "unconfirmed")
        {
            running
                .receiver
                .invalidate("宿主动作清理未确认，输入已暂停");
        }
        let observed = running.observed_layer.load(Ordering::Acquire);
        let first = running.first_event_verified.load(Ordering::Acquire);
        let failure = running.receiver.failure();
        let needs_cleanup = failure.is_some() && !running.is_retired();
        let changed = self.projection.sidebar.board_layers.observed_layer != Some(observed)
            || self.projection.sidebar.board_layers.first_event_verified != first
            || failure
                .as_ref()
                .is_some_and(|failure| &self.status_message != failure)
            || !receipts.is_empty();
        for receipt in receipts {
            let detail = match receipt.detail.as_str() {
                "blocked:ambiguous_or_missing_focused_pane" => {
                    "无法确定目标分屏，请点击目标会话区域"
                }
                "blocked:ambiguous_or_missing_input" => "未找到唯一可编辑输入框",
                detail => detail,
            };
            self.status_message = format!(
                "{} · {}",
                match receipt.outcome.as_str() {
                    "unconfirmed" => "结果未确认",
                    _ if receipt.succeeded() => "已执行",
                    _ => "未执行",
                },
                detail
            );
            self.record_host_action_receipt(receipt);
        }
        if let Some(failure) = failure {
            self.status_message = failure;
        }
        if needs_cleanup {
            let stopped = self
                .six_layer_running
                .as_mut()
                .unwrap()
                .quiesce(&self.tokio_runtime)
                .and(self.live_executor.stop_owned_actions());
            if let Err(error) = self.finish_host_cleanup(stopped) {
                self.status_message = format!("宿主动作清理未确认 · {error}");
            }
        }
        if changed {
            self.capture_runtime_view();
            self.commit_local_update();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_layer_connection_outlives_the_verification_rpc_session() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(directory.path()).unwrap();
        let session = mock_session(&store.tokio_runtime);
        let mut running =
            six_layer_runtime::RunningSixLayer::isolated(session.clone(), directory.path());
        running.readiness.configuration_observed = true;
        running.readiness.keyboard_grabbed = true;
        store.tokio_runtime.block_on(session.shutdown());
        store.connected_candidate = Some(running.candidate.clone());
        store.six_layer_running = Some(running);
        store.commit_runtime_update();
        assert!(store.projection.device_connected);
        assert!(store.device_session.is_none());
        store.handle_hardware_loop_event(
            HardwareLoopEvent::Disconnected {
                session_id: session.id,
            },
            Instant::now(),
        );
        assert!(store
            .six_layer_running
            .as_ref()
            .unwrap()
            .receiver
            .failure()
            .is_none());
        store
            .six_layer_running
            .as_ref()
            .unwrap()
            .receiver
            .invalidate("USB removed");
        store.commit_runtime_update();
        assert!(!store.projection.device_connected);
        store
            .six_layer_running
            .take()
            .unwrap()
            .shutdown(&store.tokio_runtime)
            .unwrap();
    }

    #[test]
    fn six_layer_ready_and_user_pause_are_independent_of_native_ownership() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(directory.path()).unwrap();
        store.native_ownership = NativeOwnershipController::new(None, false);
        assert!(!store.native_ownership.allows_execution());
        let session = mock_session(&store.tokio_runtime);
        let mut running =
            six_layer_runtime::RunningSixLayer::isolated(session.clone(), directory.path());
        running.readiness.configuration_observed = true;
        running.readiness.keyboard_grabbed = true;
        running.receiver.arm().unwrap();
        running.armed = true;
        store.tokio_runtime.block_on(session.shutdown());
        store.connected_candidate = Some(running.candidate.clone());
        store.six_layer_state.installation = Some(running.installation.clone());
        store.six_layer_running = Some(running);
        store.set_host_execution_enabled(true).unwrap();
        store.poll_six_layer_runtime();
        assert!(store.projection.sidebar.board_layers.runtime_ready);
        assert!(!format!("{:?}", store.projection.header).contains("原生所有权未知"));
        store.confirm_cindy_native_paused(false).unwrap();
        assert!(store.projection.sidebar.board_layers.runtime_ready);
        store.set_host_execution_enabled(false).unwrap();
        store.poll_six_layer_runtime();
        assert!(!store.projection.sidebar.board_layers.runtime_ready);
        assert!(!store
            .six_layer_running
            .as_ref()
            .unwrap()
            .is_execution_enabled());
        assert!(store
            .six_layer_running
            .as_ref()
            .unwrap()
            .receiver
            .failure()
            .is_none());
        store
            .six_layer_running
            .take()
            .unwrap()
            .shutdown(&store.tokio_runtime)
            .unwrap();
    }

    fn mock_session(runtime: &tokio::runtime::Runtime) -> Arc<DeviceSession> {
        let _entered = runtime.enter();
        let (link, _) = mi_transport::MockLink::new();
        DeviceSession::new(
            link,
            Box::new(mi_proto::NativeFramer::default()),
            Arc::new(mi_session::ReadOnlySchema),
            mi_session::SessionConfig::default(),
        )
    }

    #[test]
    fn disconnected_six_layer_session_is_not_projected_as_connected() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(directory.path()).unwrap();
        let (link, _) = mi_transport::MockLink::new();
        let session = {
            let _entered = store.tokio_runtime.enter();
            DeviceSession::new(
                link,
                Box::new(mi_proto::NativeFramer::default()),
                Arc::new(mi_session::ReadOnlySchema),
                mi_session::SessionConfig::default(),
            )
        };
        let running =
            six_layer_runtime::RunningSixLayer::isolated(session.clone(), directory.path());
        store.connected_candidate = Some(running.candidate.clone());
        store.device_session = Some(session.clone());
        drop(running);
        store.commit_runtime_update();
        session.disconnect();
        store.commit_runtime_update();
        assert!(!store.projection.device_connected);
        assert!(!format!("{:?}", store.projection.header).contains("小键盘已连接"));
        store.six_layer_running = None;
        store.tokio_runtime.block_on(session.shutdown());
    }

    #[test]
    fn six_layer_foreground_priority_ignores_legacy_manual_lock_and_keeps_physical_layer() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(directory.path()).unwrap();
        let (link, _writes) = mi_transport::MockLink::new();
        let session = {
            let _entered = store.tokio_runtime.enter();
            DeviceSession::new(
                link.clone(),
                Box::new(mi_proto::NativeFramer::default()),
                Arc::new(mi_session::ReadOnlySchema),
                mi_session::SessionConfig::default(),
            )
        };
        let mut running =
            six_layer_runtime::RunningSixLayer::isolated(session.clone(), directory.path());
        running.installation.plan = store.compiled_six_layer_plan().unwrap();
        let antigravity = running.installation.plan.hosts[0].profile_id.clone();
        let dsh = running.installation.plan.hosts[1].profile_id.clone();
        running.observed_layer.store(3, Ordering::Release);
        store.six_layer_running = Some(running);
        store.auto_follow_enabled = false;
        store.manual_override = true;
        let saved_profile = store.document_store.host_active_profile_id();
        assert!(store.foreground_changed(Some("com.google.antigravity")));
        assert_eq!(store.resolved_active_profile_id(), Some(antigravity));
        assert_eq!(
            store.projection.sidebar.board_layers.observed_layer,
            Some(3)
        );
        assert!(store
            .projection
            .sidebar
            .board_layers
            .observed_status
            .contains("前台优先 antigravity"));
        assert!(!store.foreground_changed(Some("com.google.antigravity")));
        assert!(store.foreground_changed(Some("com.openai.codex")));
        assert_eq!(store.resolved_active_profile_id(), Some(dsh));
        assert_eq!(store.document_store.host_active_profile_id(), saved_profile);
        assert!(link.writes.lock().unwrap().is_empty());
        store.six_layer_running = None;
        store.tokio_runtime.block_on(session.shutdown());
    }
}
