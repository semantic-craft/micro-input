use super::*;
use crate::six_layer_runtime::{self, InstallationStatus};
use mi_model::apply::AppliedSixLayerPlan;

impl WorkbenchStore {
    fn compiled_six_layer_plan(&self) -> Result<AppliedSixLayerPlan, String> {
        AppliedSixLayerPlan::compile(
            self.document_store
                .board_layer_plan()
                .ok_or("六层方案尚未建立")?,
            &self.document_store.host_store().draft.document,
            self.current_bottom_row_layout(),
            self.command_execution_enabled,
        )
    }

    fn released_cindy_identity(&self) -> Option<String> {
        if !self.six_layer_state.cindy_native_paused_confirmed
            || !self.native_ownership.allows_execution()
        {
            return None;
        }
        if !self.config.isolated
            && !self
                .native_ownership
                .transport()
                .is_some_and(|transport| transport.socket_exists())
        {
            return None;
        }
        match self.native_ownership.state() {
            mi_bridge::OwnershipState::Released { instance, .. } => Some(instance.to_string()),
            _ => None,
        }
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
        if !confirmed {
            if let Some(running) = &self.six_layer_running {
                running.receiver.invalidate("Cindy 原生暂停确认已撤销");
            }
        }
        self.rebuild_local_projection();
        Ok(())
    }

    fn suspend_for_six_layer_operation(&mut self) -> Option<six_layer_runtime::RunningSixLayer> {
        let running = self.six_layer_running.take();
        if let Some(running) = &running {
            running.retire();
        }
        self.six_layer_ownership_identity = None;
        if let Some(session) = &self.device_session {
            self.release_board_inputs(session.id, Instant::now());
        }
        if let Some(session) = self.device_session.take() {
            session.disconnect();
        }
        if let Some(transport) = self.mac_watcher_transport.take() {
            mi_transport::HidTransport::stop_discovery(transport.as_ref());
        }
        self.connected_candidate = None;
        self.hardware_rpc_verified = false;
        self.board_decoder.store(false, Ordering::Release);
        self.board_routing = BoardRouting::Unverified;
        running
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
        if !self.six_layer_state.cindy_native_paused_confirmed {
            return Err(WorkbenchError::Routing(
                "应用六层前，请先确认 Cindy 原生键盘集成已关闭".into(),
            ));
        }
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
        self.native_ownership.set_desired_owner("micro-input");
        self.native_ownership.poll_once(true);
        if self.released_cindy_identity().is_none() {
            return Err(WorkbenchError::Ownership(
                "六层输入需要 Cindy 当前代已释放的回执；无桥或失联时不执行".into(),
            ));
        }
        let previous = self.suspend_for_six_layer_operation();
        let dir = self.config.document_dir.clone();
        let saved = self.six_layer_state.clone();
        let runtime = Arc::clone(&self.tokio_runtime);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = (|| {
                if let Some(previous) = previous {
                    previous.shutdown(&runtime)?;
                }
                if reconnecting {
                    runtime.block_on(six_layer_runtime::resume(&dir, saved))
                } else {
                    runtime.block_on(six_layer_runtime::apply(&dir, plan, saved))
                }
            })();
            let _ = tx.send(result.map(Some));
        });
        self.six_layer_job = Some(rx);
        self.status_message = "正在准备六层输入并核验设备配置…".into();
        self.rebuild_projection();
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
        let previous = self.suspend_for_six_layer_operation();
        let dir = self.config.document_dir.clone();
        let saved = self.six_layer_state.clone();
        let runtime = Arc::clone(&self.tokio_runtime);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = (|| {
                if let Some(previous) = previous {
                    previous.shutdown(&runtime)?;
                }
                runtime
                    .block_on(six_layer_runtime::restore(&dir, saved))
                    .map(|_| None)
            })();
            let _ = tx.send(result);
        });
        self.six_layer_job = Some(rx);
        self.status_message = "正在恢复原设备配置，确认后再撤销吞键规则…".into();
        self.rebuild_projection();
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
                    self.native_ownership.poll_once(true);
                    self.six_layer_ownership_checked_at = Instant::now();
                    self.six_layer_ownership_identity = self.released_cindy_identity();
                    if self.six_layer_ownership_identity.is_some() {
                        running.armed = running.receiver.arm().is_ok();
                        if let Some(transport) = self.native_ownership.transport().cloned() {
                            let min_native_gen = match self.native_ownership.state() {
                                mi_bridge::OwnershipState::Released { generation, .. } => {
                                    *generation
                                }
                                _ => 0,
                            };
                            let request = mi_bridge::NativeOwnershipRequest {
                                version: 1,
                                request_id: mi_model::document::new_id().parse().unwrap(),
                                client_instance: self.native_ownership.client_instance(),
                                generation: self.native_ownership.generation(),
                                desired_owner: "micro-input".into(),
                                micro_drained: true,
                            };
                            running.execution_enabled.store(
                                self.document_store.host_execution_enabled(),
                                Ordering::Release,
                            );
                            running
                                .command_execution_enabled
                                .store(self.command_execution_enabled, Ordering::Release);
                            running.start_executor(six_layer_runtime::OwnershipPermit {
                                transport,
                                request,
                                native_identity: self.six_layer_ownership_identity.clone().unwrap(),
                                min_native_generation: std::sync::atomic::AtomicU64::new(
                                    min_native_gen,
                                ),
                                allow_unbridged_drain: self.config.allow_unbridged_desktop,
                            });
                        }
                    } else {
                        running
                            .receiver
                            .invalidate("Cindy 当前代释放未确认，请重新连接");
                    }
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
            self.rebuild_projection();
        }
        if self.six_layer_running.is_none() {
            return;
        }
        if self.six_layer_ownership_checked_at.elapsed() > Duration::from_secs(2)
            && !self.native_ownership.has_pending_request()
        {
            self.six_layer_ownership_checked_at = Instant::now();
            self.poll_ownership_fast_or_spawn(true, Instant::now());
        }
        let identity = self.released_cindy_identity();
        let running = self.six_layer_running.as_ref().unwrap();
        running.execution_enabled.store(
            self.document_store.host_execution_enabled(),
            Ordering::Release,
        );
        running
            .command_execution_enabled
            .store(self.command_execution_enabled, Ordering::Release);
        if identity.is_none() || identity != self.six_layer_ownership_identity {
            running
                .receiver
                .invalidate("Cindy 原生所有权已变化，六层输入已暂停");
        }
        let mut receipts = Vec::new();
        if let Some(rx) = &running.receipts {
            while let Ok(receipt) = rx.try_recv() {
                receipts.push(receipt);
            }
        }
        let observed = running.observed_layer.load(Ordering::Acquire);
        let first = running.first_event_verified.load(Ordering::Acquire);
        let failure = running.receiver.failure();
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
        if changed {
            self.rebuild_local_projection();
        }
    }

    pub(super) fn project_six_layer_runtime(&self, result: &mut BoardLayersProjection) {
        result.cindy_native_paused_confirmed = self.six_layer_state.cindy_native_paused_confirmed;
        result.restore_available =
            self.six_layer_state.installation.is_some() && self.six_layer_job.is_none();
        result.apply_block_reason = if self.six_layer_job.is_some() {
            "六层操作正在进行".into()
        } else if !result.cindy_native_paused_confirmed {
            "应用六层前，请确认 Cindy 原生键盘集成已关闭".into()
        } else {
            self.compiled_six_layer_plan().err().unwrap_or_default()
        };
        result.can_apply = result.apply_block_reason.is_empty();
        if let Some(running) = &self.six_layer_running {
            result.configuration_observed = running.readiness.configuration_observed;
            result.keyboard_grabbed =
                running.readiness.keyboard_grabbed && running.receiver.failure().is_none();
            result.first_event_verified = running.first_event_verified.load(Ordering::Acquire);
            result.runtime_ready = running.armed
                && running.receiver.failure().is_none()
                && self.released_cindy_identity().is_some()
                && self.document_store.host_execution_enabled();
            result.observed_layer = Some(running.observed_layer.load(Ordering::Acquire));
            result.observed_profile_kind = Some("six_layer_direct".into());
            result.observed_status = running.receiver.failure().unwrap_or_else(|| {
                let layer = running.observed_layer.load(Ordering::Acquire);
                let priority = self
                    .foreground_bundle_id
                    .as_deref()
                    .filter(|_| (2..=4).contains(&layer))
                    .and_then(|bundle| {
                        running.installation.plan.hosts.iter().find(|host| {
                            mi_macos::executor::bundle_id_for_adapter(&host.adapter) == Some(bundle)
                        })
                    })
                    .map(|host| format!(" · 前台优先 {}", host.adapter))
                    .unwrap_or_default();
                format!(
                    "六层配置已核验 · 最近确认第 {} 层{} · {}",
                    layer,
                    priority,
                    if running.first_event_verified.load(Ordering::Acquire) {
                        "实体输入已验证"
                    } else {
                        "等待首次实体输入"
                    }
                )
            });
        } else if let Some(installation) = &self.six_layer_state.installation {
            result.observed_layer = None;
            result.observed_status = if installation.status == InstallationStatus::Applied {
                "六层已保存，当前输入连接尚未确认"
            } else {
                "六层恢复待处理，吞键规则保留"
            }
            .into();
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
        store.rebuild_local_projection();
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
        store.rebuild_local_projection();
        assert!(!store.projection.device_connected);
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
        store.rebuild_local_projection();
        session.disconnect();
        store.rebuild_local_projection();
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
