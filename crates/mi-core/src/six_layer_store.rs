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
            mi_bridge::OwnershipState::Released {
                instance,
                generation,
            } => Some(format!("{instance}:{generation}")),
            _ => None,
        }
    }

    pub(super) fn confirm_cindy_native_paused(
        &mut self,
        confirmed: bool,
    ) -> Result<(), WorkbenchError> {
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

    fn suspend_for_six_layer_operation(&mut self) {
        self.six_layer_running = None;
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
    }

    pub(super) fn start_six_layer_apply(
        &mut self,
        resume: Option<AppliedSixLayerPlan>,
    ) -> Result<(), WorkbenchError> {
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
        self.suspend_for_six_layer_operation();
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
        self.rebuild_projection();
        Ok(())
    }

    pub(super) fn start_six_layer_restore(&mut self) -> Result<(), WorkbenchError> {
        if self.six_layer_job.is_some()
            || self.six_layer_state.installation.is_none()
            || self.config.isolated
        {
            return Err(WorkbenchError::Routing(
                "当前没有可执行的六层恢复操作".into(),
            ));
        }
        self.suspend_for_six_layer_operation();
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
                    self.device_session = Some(Arc::clone(&running.session));
                    self.connected_candidate = Some(running.candidate.clone());
                    self.hardware_rpc_verified = true;
                    self.native_ownership.poll_once(true);
                    self.six_layer_ownership_identity = self.released_cindy_identity();
                    if self.six_layer_ownership_identity.is_some() {
                        running.armed = running.receiver.arm().is_ok();
                        if let Some(transport) = self.native_ownership.transport().cloned() {
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
                            });
                        }
                    } else {
                        running
                            .receiver
                            .invalidate("Cindy 当前代释放未确认，请重新连接");
                    }
                    running.start_monitor(Arc::clone(&self.tokio_runtime));
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
        if self.six_layer_ownership_checked_at.elapsed() > Duration::from_millis(500)
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
            self.status_message = format!(
                "{} · {}",
                match receipt.outcome.as_str() {
                    "unconfirmed" => "结果未确认",
                    _ if receipt.succeeded() => "已执行",
                    _ => "未执行",
                },
                receipt.detail
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
                format!(
                    "六层配置已核验 · 第 {} 层 · {}",
                    running.observed_layer.load(Ordering::Acquire),
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
