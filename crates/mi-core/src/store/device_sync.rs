use super::*;

pub(super) fn sorted_verified_usb_candidates(
    candidates: &[mi_transport::HidCandidate],
) -> Vec<mi_transport::HidCandidate> {
    let mut verified_usb = candidates
        .iter()
        .filter(|candidate| {
            mi_transport::is_candidate(candidate)
                && candidate.transport_hint == mi_transport::TransportHint::Usb
                && candidate.pid == 0x8298
        })
        .cloned()
        .collect::<Vec<_>>();
    verified_usb.sort_by(|left, right| {
        (
            left.serial_number.is_none(),
            left.serial_number.as_deref().unwrap_or(""),
            left.path.as_str(),
        )
            .cmp(&(
                right.serial_number.is_none(),
                right.serial_number.as_deref().unwrap_or(""),
                right.path.as_str(),
            ))
    });
    verified_usb
}

pub(super) fn choose_usb_candidate<'a>(
    verified_usb: &'a [mi_transport::HidCandidate],
    explicit_selector: Option<&str>,
    preferred_serial: Option<&str>,
) -> Option<&'a mi_transport::HidCandidate> {
    if let Some(raw_selector) = explicit_selector {
        let selector = raw_selector.trim();
        if selector.is_empty() {
            return None;
        }
        return verified_usb
            .iter()
            .find(|candidate| candidate.serial_number.as_deref() == Some(selector))
            .or_else(|| {
                verified_usb
                    .iter()
                    .find(|candidate| candidate.path == selector)
            });
    }
    if let Some(preferred) = preferred_serial.map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(found) = verified_usb
            .iter()
            .find(|candidate| candidate.serial_number.as_deref() == Some(preferred))
        {
            return Some(found);
        }
    }
    verified_usb.first()
}

impl WorkbenchStore {
    pub fn current_device_snapshot(&self) -> Result<Value, WorkbenchError> {
        if let Some(snap) = &self.document_store.draft.document.baseline {
            return Ok(snap.clone());
        }
        Err(WorkbenchError::Routing(
            self.effective_locale()
                .select(
                    "尚未读取到真实设备基线快照；请先连接设备读取快照",
                    "No real device baseline snapshot has been read yet; connect a device first.",
                )
                .to_string(),
        ))
    }

    fn device_snapshot_for_workflow(&mut self) -> Result<Value, WorkbenchError> {
        if !self.config.isolated {
            return self.current_device_snapshot();
        }
        let wb_path = self.config.workbench_json_path();
        if !wb_path.exists() {
            let prev_lang = self.document_store.ui_language();
            let prev_silent = self.document_store.silent_at_login();
            let _ = self.document_store.open(wb_path);
            let _ = self.document_store.set_ui_language(prev_lang);
            let _ = self.document_store.set_silent_at_login(prev_silent);
        }
        if let Some(snap) = &self.document_store.draft.document.baseline {
            return Ok(snap.clone());
        }
        Ok(self.synthesize_isolated_device_snapshot())
    }

    fn synthesize_isolated_device_snapshot(&mut self) -> Value {
        debug_assert!(self.config.isolated);
        let mut raw_demo = mi_model::demo()
            .map(|d| d.raw)
            .unwrap_or_else(|_| self.document_store.draft.config.raw.clone());
        raw_demo["version"] = json!(1);
        let keymap_bytes = serde_json::to_vec(&raw_demo).unwrap_or_default();
        let keymap_file = mi_model::operation_record::file_value(&keymap_bytes);
        let sha1 = keymap_file["sha1"].as_str().unwrap_or_default().to_string();
        let snap = json!({
            "firmware": "0.6.3",
            "device_identity": {
                "confidence": "verified-serial",
                "serial": "creator-micro-2-local",
                "session_id": 1
            },
            "keymap": keymap_file,
            "smart_actions": {
                "state": "missing"
            },
            "files": [
                {
                    "name": "keymap",
                    "state": "present",
                    "size": keymap_bytes.len(),
                    "sha1": sha1
                }
            ]
        });
        if let Ok(base_cfg) = mi_model::parse_device_files(&keymap_bytes, None) {
            let _ = self
                .document_store
                .create_independent(base_cfg, snap.clone());
            let pid = self.document_store.draft.config.profiles[0].id.clone();
            let lid = self.document_store.draft.config.profiles[0].layers[0]
                .id
                .clone();
            if let Some(device_key_action) = mi_model::draft::actions()
                .into_iter()
                .find(|a| a.id == "device-key")
            {
                let _ = self
                    .document_store
                    .update(|draft| draft.assign(&pid, &lid, 0, device_key_action));
            }
        }
        snap
    }

    pub fn dispatch_device_workflow_action(
        &mut self,
        action: DeviceWorkflowAction,
    ) -> Result<(), WorkbenchError> {
        self.require_awake()?;
        match action {
            DeviceWorkflowAction::ReadIdentity => {
                let msg = if let Some(cand) = &self.connected_candidate {
                    format!(
                        "设备身份已读取：{} ({:04X}:{:04X}) · 只读门禁生效",
                        cand.product, cand.vid, cand.pid
                    )
                } else {
                    "当前处于离线草稿模式 · 设备身份：Creator Micro 2 (303A:8298)".to_string()
                };
                self.status_message = msg;
                self.commit_runtime_update();
                Ok(())
            }
            DeviceWorkflowAction::ReadSnapshot => {
                let snap = self.device_snapshot_for_workflow()?;
                let sha1 = snap["keymap"]["sha1"].as_str().unwrap_or("-");
                self.status_message = format!("设备快照已核验 · keymap sha1={sha1}");
                self.commit_runtime_update();
                Ok(())
            }
            DeviceWorkflowAction::ExportDiagnostics => {
                let diag_path = self.config.document_dir.join("diagnostic-report.json");
                let report = json!({
                    "schema_version": 1,
                    "product": "Creator Micro 2",
                    "vid_pid": "303A:8298",
                    "connected": self.device_session_connected(),
                    "document_revision": self.document_store.draft.revision,
                    "host_revision": self.document_store.host_revision(),
                    "execution_enabled": self.document_store.host_execution_enabled(),
                });
                mi_macos::write_private_json(&diag_path, &report).map_err(|error| {
                    WorkbenchError::Storage(format!(
                        "无法保存诊断报告：{}",
                        mi_macos::DiagnosticRedactor::sanitize(&error)
                    ))
                })?;
                let exported_path = diag_path.display().to_string();
                let safe_display_path = mi_macos::DiagnosticRedactor::sanitize(
                    &mi_macos::redact_home_path(&exported_path),
                );
                self.last_exported_diagnostics_path = Some(exported_path);
                self.status_message = format!("已导出诊断报告：{safe_display_path}");
                self.commit_runtime_update();
                Ok(())
            }
            DeviceWorkflowAction::OpenReconnectComparison => {
                let snap = self.device_snapshot_for_workflow()?;
                let device_cfg = self.document_store.draft.config.clone();
                self.dispatch_editor_action(EditorAction::OpenReconnectComparison {
                    device_config: Box::new(device_cfg),
                    baseline: snap,
                })
            }
            DeviceWorkflowAction::ConfirmComparisonIdentity => {
                let snap = self.device_snapshot_for_workflow()?;
                self.dispatch_editor_action(EditorAction::ComparisonConfirmIdentity {
                    current_snapshot: snap,
                })
            }
            DeviceWorkflowAction::ResolveComparison(choice) => {
                let snap = self.device_snapshot_for_workflow()?;
                self.dispatch_editor_action(EditorAction::ComparisonResolve {
                    choice,
                    current_snapshot: snap,
                })
            }
            DeviceWorkflowAction::OpenApplyPreparation => {
                let snap = self.device_snapshot_for_workflow()?;
                self.dispatch_editor_action(EditorAction::OpenApplyPreparation {
                    current_snapshot: snap,
                })
            }
            DeviceWorkflowAction::PrepareApplyBackup => {
                let snap = self.device_snapshot_for_workflow()?;
                self.dispatch_editor_action(EditorAction::ApplyPrepareBackup {
                    current_snapshot: snap,
                })
            }
            DeviceWorkflowAction::TakePreparedCandidate => {
                let snap = self.device_snapshot_for_workflow()?;
                self.dispatch_editor_action(EditorAction::ApplyTakePreparedCandidate {
                    current_snapshot: snap,
                })
            }
            DeviceWorkflowAction::CancelApplyPreview => {
                self.dispatch_editor_action(EditorAction::ApplyCancelPreview)
            }
            DeviceWorkflowAction::SimulateKeyPress { binding_id } => {
                if !self.config.isolated {
                    return Err(WorkbenchError::Routing(
                        self.effective_locale()
                            .select(
                                "生产模式下不允许合成按键注入",
                                "Synthetic key press injection is unavailable in production mode.",
                            )
                            .to_string(),
                    ));
                }
                let ev = InputEvent {
                    session_id: 1,
                    board_layer: None,
                    sequence: 1,
                    ingress_sequence: 1,
                    radial_center: None,
                    radial_position: None,
                    timestamp_ms: 0,
                    kind: InputEventKind::Pressed,
                    logical_switch: None,
                    binding_id: Some(binding_id),
                    physical_control: None,
                    keycap_group: None,
                    action_role: None,
                    lost_count: 0,
                    action_eligible: true,
                };
                let _ = self.consume_physical_input_event(ev, self.update_now);
                Ok(())
            }
        }
    }

    pub fn start_mac_usb_discovery_watcher(&mut self) {
        self.discovery_enabled = true;
        if self.system_lifecycle != SystemLifecycle::Awake {
            return;
        }
        if self.config.isolated || self.mac_watcher_transport.is_some() {
            return;
        }
        let transport = Arc::new(if self.discovery_is_descriptor_only() {
            mi_transport::MacTransport::new()
        } else {
            mi_transport::MacTransport::exclusive_vendor()
        });
        let mut rx = mi_transport::HidTransport::subscribe(transport.as_ref());
        let tx = self.hardware_event_tx.clone();
        let epoch = self.lifecycle_epoch.load(Ordering::Acquire);
        self.tokio_runtime.spawn(async move {
            while rx.recv().await.is_ok() {
                if tx
                    .send(HardwareLoopEvent::DiscoveryChanged { epoch })
                    .is_err()
                {
                    break;
                }
            }
        });
        self.mac_watcher_transport = Some(transport);
    }

    pub fn connect_default_usb(&mut self) -> Result<bool, WorkbenchError> {
        self.require_awake()?;
        if self.firmware_update.reserved() {
            return Err(WorkbenchError::Routing("固件更新期间设备控制已暂停".into()));
        }
        if let Some(previous) = self.firmware_update.resume_auto_connect.take() {
            self.config.auto_connect_usb = previous;
        }
        self.exclusive_usb_transport();
        self.connect_default_usb_with_execution(true)
    }

    pub(super) fn exclusive_usb_transport(&mut self) -> Arc<mi_transport::MacTransport> {
        self.resume_requires_explicit_connect = false;
        if self
            .mac_watcher_transport
            .as_ref()
            .is_some_and(|transport| !transport.is_exclusive_vendor())
        {
            if let Some(transport) = self.mac_watcher_transport.take() {
                mi_transport::HidTransport::stop_discovery(transport.as_ref());
            }
        }
        self.start_mac_usb_discovery_watcher();
        self.mac_watcher_transport
            .get_or_insert_with(|| Arc::new(mi_transport::MacTransport::exclusive_vendor()))
            .clone()
    }

    pub(super) fn connect_default_usb_with_execution(
        &mut self,
        enable_execution_on_connect: bool,
    ) -> Result<bool, WorkbenchError> {
        self.require_awake()?;
        if let Some(installation) = &self.six_layer_state.installation {
            let plan = installation.plan.clone();
            if self
                .six_layer_running
                .as_ref()
                .is_none_or(|running| running.receiver.failure().is_some())
                && self.six_layer_job.is_none()
            {
                self.start_six_layer_apply(Some(plan))?;
            }
            return Ok(self.six_layer_running.is_some());
        }
        let transport = self.exclusive_usb_transport();
        let result = self.connect_with_transport(transport.as_ref(), enable_execution_on_connect);
        if let Err(error) = &result {
            self.set_status_message(error.to_string());
        }
        result
    }

    pub fn connect_with_transport(
        &mut self,
        transport: &dyn mi_transport::HidTransport,
        enable_execution_on_connect: bool,
    ) -> Result<bool, WorkbenchError> {
        self.connect_with_transport_preferring(transport, enable_execution_on_connect, None)
    }

    pub fn connect_with_transport_preferring(
        &mut self,
        transport: &dyn mi_transport::HidTransport,
        enable_execution_on_connect: bool,
        explicit_selector: Option<&str>,
    ) -> Result<bool, WorkbenchError> {
        self.require_awake()?;
        if self.six_layer_state.installation.is_some()
            || self.six_layer_running.is_some()
            || self.six_layer_job.is_some()
        {
            return Err(WorkbenchError::Routing(
                "六层运行独占 USB，普通连接不能启动第二个读取器".into(),
            ));
        }
        if self
            .device_session
            .as_ref()
            .is_some_and(|s| s.is_disconnected() || self.input_lost_session == Some(s.id))
        {
            if self
                .device_session
                .as_ref()
                .is_some_and(|s| !s.is_disconnected())
            {
                self.teardown_v070_session_sa_rgb();
            } else {
                self.sa_rgb_coordinator.clear_on_disconnect();
            }
            self.release_board_inputs(self.update_now);
            if let Some(session) = self.device_session.take() {
                self.tokio_runtime.block_on(session.shutdown());
            }
            self.connected_candidate = None;
        }
        if self.live_executor.has_unconfirmed_apple_fn_release()
            || !self.ptt_controller.cleanup_confirmed
        {
            let stopped = self.live_executor.stop_owned_actions();
            self.finish_host_cleanup(stopped)
                .map_err(WorkbenchError::Routing)?;
        }
        if let (Some(session), Some(cand)) = (&self.device_session, &self.connected_candidate) {
            if explicit_selector.is_none() {
                self.status_message = format!(
                    "已连接 {} ({:04X}:{:04X}) · {}",
                    cand.product,
                    cand.vid,
                    cand.pid,
                    if session.has_exclusive_vendor_link() {
                        "USB 专用接口已独占"
                    } else {
                        "已建立 HID 会话"
                    }
                );
                self.commit_runtime_update();
                return Ok(true);
            }
        }

        let candidates = transport
            .enumerate()
            .map_err(|e| WorkbenchError::Storage(format!("检测小键盘连接失败：{e}")))?;
        let verified_usb = sorted_verified_usb_candidates(&candidates);
        self.available_usb_candidates = verified_usb.clone();

        if verified_usb.is_empty() {
            if let Some(first) = candidates.first() {
                self.status_message = format!(
                    "已检测到 {} ({:04X}:{:04X})",
                    first.product, first.vid, first.pid
                );
            } else {
                self.status_message =
                    "未检测到 Creator Micro 2 USB 连接（离线工作台就绪）".to_string();
            }
            self.commit_runtime_update();
            return Ok(false);
        }

        let preferred_serial = self.document_store.preferred_device_serial();
        let Some(target) = choose_usb_candidate(
            &verified_usb,
            explicit_selector,
            preferred_serial.as_deref(),
        )
        .cloned() else {
            let selector = explicit_selector.unwrap_or_default();
            self.commit_runtime_update();
            return Err(WorkbenchError::Storage(format!(
                "未找到目标 Creator Micro 2 设备（{selector}）"
            )));
        };

        let link = transport
            .open(&target.path)
            .map_err(|e| WorkbenchError::Storage(format!("打开 USB 小键盘失败：{e}")))?;
        if !self.config.isolated && !link.is_exclusive_vendor() {
            link.close();
            return Err(WorkbenchError::Storage("未取得 USB 专用接口独占".into()));
        }
        let session = {
            let _guard = self.tokio_runtime.enter();
            DeviceSession::new(
                link,
                Box::new(mi_proto::NativeFramer::default()),
                Arc::new(mi_session::ReadOnlySchema),
                mi_session::SessionConfig::default(),
            )
        };
        self.merge_wide_key
            .store(self.bottom_row_layout.is_merged(), Ordering::SeqCst);
        self.board_decoder = Arc::new(AtomicBool::new(false));
        self.board_routing = if session.has_exclusive_vendor_link() {
            BoardRouting::Checking
        } else {
            BoardRouting::Legacy
        };
        let mut monitor = session.subscribe_verified_input_with_board_mode(
            Arc::clone(&self.merge_wide_key),
            Arc::clone(&self.board_decoder),
        );
        let session_id = session.id;
        self.hardware_event_tx.open_source(session_id);
        self.input_lost_session = None;
        let tx = self.hardware_event_tx.clone();
        self.tokio_runtime.spawn(async move {
            while let Some(event) = monitor.next().await {
                if tx.send(HardwareLoopEvent::Input(event)).is_err() {
                    return;
                }
            }
            let _ = tx.send(HardwareLoopEvent::Disconnected { session_id });
        });
        let mut cs_notif_rx = session.notifications();
        let cs_session = Arc::clone(&session);
        let cs_tx = self.hardware_event_tx.clone();
        self.tokio_runtime.spawn(async move {
            let map_kind = |notification: &mi_proto::HostNotification| match notification {
                mi_proto::HostNotification::CheatSheetShow(_) => {
                    Some(CheatSheetNotificationKind::Show)
                }
                mi_proto::HostNotification::CheatSheetHide(_) => {
                    Some(CheatSheetNotificationKind::Hide)
                }
                mi_proto::HostNotification::CheatSheetToggle(_) => {
                    Some(CheatSheetNotificationKind::Toggle)
                }
                _ => None,
            };
            loop {
                tokio::select! {
                    _ = cs_session.wait_disconnected() => {
                        loop {
                            match cs_notif_rx.try_recv() {
                                Ok(received) => {
                                    if let Some(kind) = map_kind(&received.notification) {
                                        if cs_tx
                                            .send(HardwareLoopEvent::CheatSheetNotification {
                                                session_id,
                                                ingress_sequence: received.ingress_sequence,
                                                kind,
                                            })
                                            .is_err()
                                        {
                                            return;
                                        }
                                    }
                                }
                                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => continue,
                                Err(
                                    tokio::sync::broadcast::error::TryRecvError::Empty
                                    | tokio::sync::broadcast::error::TryRecvError::Closed,
                                ) => break,
                            }
                        }
                        return;
                    }
                    notif = cs_notif_rx.recv() => {
                        match notif {
                            Ok(received) => {
                                if let Some(kind) = map_kind(&received.notification) {
                                    if cs_tx
                                        .send(HardwareLoopEvent::CheatSheetNotification {
                                            session_id,
                                            ingress_sequence: received.ingress_sequence,
                                            kind,
                                        })
                                        .is_err()
                                    {
                                        return;
                                    }
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                        }
                    }
                }
            }
        });
        self.device_session = Some(Arc::clone(&session));
        self.connected_candidate = Some(target.clone());
        if let Some(sn) = target
            .serial_number
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let _ = self
                .document_store
                .set_preferred_device_serial(Some(sn.to_string()));
        }
        self.hardware_rpc_verified = false;
        if enable_execution_on_connect && !self.document_store.host_execution_enabled() {
            let _ = self.apply_routing_operation(RoutingOperation::Execution { enabled: true });
        }
        self.lighting_arbiter.invalidate_hardware_cache();
        self.refresh_four_app_lighting();
        let accept_session = Arc::clone(&session);
        let accept_target = target.clone();
        let accept_tx = self.hardware_event_tx.clone();
        let initial_lighting_json = self
            .lighting_arbiter
            .last_hardware_sync_json
            .clone()
            .unwrap_or_default();
        self.tokio_runtime.spawn(async move {
            if let Some(receipt) = run_automatic_hardware_acceptance(
                &accept_session,
                &accept_target,
                &initial_lighting_json,
            )
            .await
            {
                let _ = accept_tx.send(HardwareLoopEvent::HardwareAutoAccepted {
                    session_id: accept_session.id,
                    receipt,
                });
            }
        });
        let iface_label = if session.has_exclusive_vendor_link() {
            "USB 专用接口已独占"
        } else {
            "已建立 HID 会话"
        };
        self.status_message = if let Some(sn) = target
            .serial_number
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            format!(
                "已连接 {} ({:04X}:{:04X} · #{sn}) · {iface_label}",
                target.product, target.vid, target.pid
            )
        } else {
            format!(
                "已连接 {} ({:04X}:{:04X}) · {iface_label}",
                target.product, target.vid, target.pid
            )
        };
        self.commit_runtime_update();
        Ok(true)
    }

    pub fn select_candidate_device_with_transport(
        &mut self,
        transport: &dyn mi_transport::HidTransport,
        selector: &str,
    ) -> Result<bool, WorkbenchError> {
        let selector = selector.trim();
        if selector.is_empty() {
            return Err(WorkbenchError::Routing("未指定目标设备标识".into()));
        }
        self.require_awake()?;
        if self.firmware_update.reserved() {
            return Err(WorkbenchError::Routing(
                "固件更新流程正在占用设备，请结束后再切换设备".into(),
            ));
        }
        if self.six_layer_state.installation.is_some()
            || self.six_layer_running.is_some()
            || self.six_layer_job.is_some()
        {
            return Err(WorkbenchError::Routing(
                "六层运行独占 USB，不能直接切换连接设备".into(),
            ));
        }
        if self.live_executor.has_unconfirmed_apple_fn_release()
            || !self.ptt_controller.cleanup_confirmed
        {
            let stopped = self.live_executor.stop_owned_actions();
            self.finish_host_cleanup(stopped)
                .map_err(WorkbenchError::Routing)?;
        }
        let candidates = transport
            .enumerate()
            .map_err(|e| WorkbenchError::Storage(format!("检测小键盘连接失败：{e}")))?;
        let verified_usb = sorted_verified_usb_candidates(&candidates);
        let Some(target) = choose_usb_candidate(&verified_usb, Some(selector), None).cloned()
        else {
            self.available_usb_candidates = verified_usb;
            self.commit_runtime_update();
            return Err(WorkbenchError::Storage(format!(
                "未找到目标 Creator Micro 2 设备（{selector}）"
            )));
        };
        self.available_usb_candidates = verified_usb;
        if self
            .device_session
            .as_ref()
            .is_some_and(|s| !s.is_disconnected())
            && self
                .connected_candidate
                .as_ref()
                .is_some_and(|cur| cur.path == target.path)
        {
            if let Some(sn) = target
                .serial_number
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                let _ = self
                    .document_store
                    .set_preferred_device_serial(Some(sn.to_string()));
            }
            self.commit_runtime_update();
            return Ok(true);
        }
        self.teardown_v070_session_sa_rgb();
        self.release_board_inputs(self.update_now);
        if let Some(session) = self.device_session.take() {
            self.tokio_runtime.block_on(session.shutdown());
        }
        self.connected_candidate = None;
        self.hardware_rpc_verified = false;
        self.lighting_arbiter.invalidate_hardware_cache();
        self.v070_hardware.on_disconnected();
        self.board_routing = BoardRouting::Checking;
        self.connect_with_transport_preferring(transport, false, Some(&target.path))
    }

    pub(super) fn select_candidate_device_from_ui(
        &mut self,
        selector: &str,
    ) -> Result<(), WorkbenchError> {
        let result = (|| -> Result<(), WorkbenchError> {
            self.require_awake()?;
            if self.config.isolated {
                return Err(WorkbenchError::Routing(
                    "隔离工作台不支持切换实体 USB 设备".into(),
                ));
            }
            let transport = self.exclusive_usb_transport();
            self.select_candidate_device_with_transport(transport.as_ref(), selector)
                .map(|_| ())
        })();
        if let Err(err) = &result {
            self.status_message = err.to_string();
            self.commit_runtime_update();
        }
        result
    }

    pub fn handle_hardware_loop_event(
        &mut self,
        event: HardwareLoopEvent,
        now: Instant,
    ) -> Option<HostActionReceipt> {
        self.update_now = now;
        if self.system_lifecycle != SystemLifecycle::Awake {
            return None;
        }
        match event {
            HardwareLoopEvent::Input(input_event) => {
                let session = self.device_session.as_ref()?;
                if session.id != input_event.session_id
                    || (session.is_disconnected() && input_event.kind != InputEventKind::Released)
                {
                    return None;
                }
                self.route_board_input(input_event, now)
            }
            HardwareLoopEvent::Disconnected { session_id } => {
                if self
                    .device_session
                    .as_ref()
                    .is_some_and(|s| s.id == session_id)
                {
                    if let Some(running) = &self.six_layer_running {
                        running.receiver.invalidate("USB 会话已断开");
                    }
                    self.sa_rgb_coordinator.clear_on_disconnect();
                    self.release_board_inputs(now);
                    self.board_routing = BoardRouting::Checking;
                    self.device_session = None;
                    let previous = self.connected_candidate.take();
                    if let Some(prev) = &previous {
                        self.available_usb_candidates
                            .retain(|c| c.path != prev.path);
                    }
                    self.hardware_rpc_verified = false;
                    self.lighting_arbiter.invalidate_hardware_cache();
                    self.v070_hardware.on_disconnected();
                    self.status_message = "小键盘已断开连接（等待重新插入）".to_string();
                    self.commit_runtime_update();
                    if self.config.auto_connect_usb && !self.config.isolated {
                        let transport = self.exclusive_usb_transport();
                        use mi_transport::HidTransport;
                        if let Ok(candidates) = transport.enumerate() {
                            let next_candidates = sorted_verified_usb_candidates(&candidates);
                            let reenumerated = next_candidates.iter().any(|candidate| {
                                mi_transport::is_exclusive_vendor_candidate(candidate)
                                    && previous
                                        .as_ref()
                                        .is_none_or(|old| old.path != candidate.path)
                            });
                            if self.available_usb_candidates != next_candidates {
                                self.available_usb_candidates = next_candidates;
                                self.commit_runtime_update();
                            }
                            if reenumerated {
                                let _ = self.connect_default_usb_with_execution(false);
                            }
                        }
                    }
                }
                None
            }
            HardwareLoopEvent::DiscoveryChanged { epoch } => {
                if self.firmware_update.reserved() {
                    return None;
                }
                if epoch != self.lifecycle_epoch.load(Ordering::Acquire) {
                    return None;
                }
                if !self.config.isolated
                    && self.config.auto_connect_usb
                    && self
                        .six_layer_state
                        .installation
                        .as_ref()
                        .is_some_and(|inst| {
                            inst.status == crate::six_layer_runtime::InstallationStatus::Applied
                                && inst.plan.validate_current().is_ok()
                        })
                {
                    self.resume_requires_explicit_connect = false;
                    self.six_layer_auto_resume_attempts = 0;
                    self.six_layer_next_auto_resume_at = None;
                }
                if self.resume_requires_explicit_connect {
                    return None;
                }
                let mut candidates_changed = false;
                let mut old_path_missing = false;
                if let Some(transport) = &self.mac_watcher_transport {
                    use mi_transport::HidTransport;
                    if let Ok(candidates) = transport.enumerate() {
                        let next_candidates = sorted_verified_usb_candidates(&candidates);
                        if self.available_usb_candidates != next_candidates {
                            self.available_usb_candidates = next_candidates;
                            candidates_changed = true;
                        }
                        old_path_missing = self.connected_candidate.as_ref().is_some_and(|old| {
                            !candidates
                                .iter()
                                .any(|candidate| candidate.path == old.path)
                        });
                    }
                }
                let mut needs_commit = candidates_changed;
                if self
                    .device_session
                    .as_ref()
                    .is_some_and(|s| s.is_disconnected() || old_path_missing)
                {
                    if let Some(running) = &self.six_layer_running {
                        running.receiver.invalidate("USB 设备身份已变化");
                    }
                    self.release_board_inputs(now);
                    self.board_routing = BoardRouting::Checking;
                    if let Some(session) = self.device_session.take() {
                        session.disconnect();
                    }
                    self.connected_candidate = None;
                    self.hardware_rpc_verified = false;
                    self.lighting_arbiter.invalidate_hardware_cache();
                    self.v070_hardware.on_disconnected();
                    self.sa_rgb_coordinator.clear_on_disconnect();
                    needs_commit = true;
                }
                let before_gen = self.projection_generation;
                if !self.config.isolated && !self.device_session_connected() {
                    let _ = self.connect_default_usb_with_execution(false);
                }
                if needs_commit && self.projection_generation == before_gen {
                    self.commit_runtime_update();
                }
                None
            }
            HardwareLoopEvent::HardwareAutoAccepted {
                session_id,
                receipt,
            } => {
                if self
                    .device_session
                    .as_ref()
                    .is_some_and(|s| s.id == session_id)
                {
                    if self.board_routing == BoardRouting::Checking {
                        self.confirm_board_routing(&receipt);
                    }
                    self.hardware_rpc_verified = true;
                    if self.board_routing == BoardRouting::Legacy {
                        self.status_message = format!(
                            "已连接 {} ({}) · 固件 {} 自动验收通过",
                            receipt.product, receipt.vid_pid, receipt.firmware_version
                        );
                    }
                    if let Ok(bytes) = serde_json::to_vec_pretty(&receipt) {
                        let path = self
                            .config
                            .document_dir
                            .join("hardware-auto-acceptance.json");
                        let tmp = path.with_extension("json.tmp");
                        if fs::write(&tmp, bytes).is_ok() {
                            let _ = fs::rename(tmp, path);
                        }
                    }
                    self.last_hardware_auto_acceptance = Some(receipt);
                    self.lighting_arbiter.invalidate_hardware_cache();
                    self.refresh_four_app_lighting();
                    if let Some(session) = self
                        .device_session
                        .as_ref()
                        .filter(|s| s.id == session_id)
                        .map(Arc::clone)
                    {
                        self.trigger_v070_session_snapshot_if_supported(&session);
                    }
                    self.commit_runtime_update();
                }
                None
            }
            HardwareLoopEvent::LiveCandidatesUpdated { epoch, catalog } => {
                if epoch != self.lifecycle_epoch.load(Ordering::Acquire) {
                    return None;
                }
                let desired = self.desired_live_session_source();
                if catalog
                    .as_ref()
                    .is_some_and(|c| c.instance.source() != desired)
                {
                    return None;
                }
                self.update_navigation_catalog(catalog);
                None
            }
            HardwareLoopEvent::V070CapabilityOutcome {
                session_id,
                seq,
                result,
            } => {
                self.handle_v070_capability_outcome(session_id, seq, result);
                None
            }
            HardwareLoopEvent::V070ProfileLayerChanged {
                session_id,
                profile_index,
                layer_index,
            } => {
                self.handle_v070_profile_layer_changed(session_id, profile_index, layer_index);
                None
            }
            HardwareLoopEvent::CheatSheetNotification {
                session_id,
                ingress_sequence,
                kind,
            } => {
                let active_session = self
                    .device_session
                    .as_ref()
                    .is_some_and(|s| s.id == session_id)
                    || self.six_layer_running.is_some();
                if !active_session
                    || self.last_cheat_sheet_notif == Some((session_id, ingress_sequence))
                {
                    return None;
                }
                self.last_cheat_sheet_notif = Some((session_id, ingress_sequence));
                match kind {
                    CheatSheetNotificationKind::Show => {
                        let sheet = self.build_cheat_sheet_projection();
                        self.overlay_controller.show_cheat_sheet(
                            sheet,
                            &self.screen_geometry,
                            self.last_cursor_position,
                        );
                        self.commit_runtime_update();
                    }
                    CheatSheetNotificationKind::Hide => {
                        if self.overlay_controller.hide_cheat_sheet() {
                            self.commit_runtime_update();
                        }
                    }
                    CheatSheetNotificationKind::Toggle => {
                        let sheet = self.build_cheat_sheet_projection();
                        self.overlay_controller.toggle_cheat_sheet(
                            sheet,
                            &self.screen_geometry,
                            self.last_cursor_position,
                        );
                        self.commit_runtime_update();
                    }
                }
                None
            }
            HardwareLoopEvent::EncoderHoldTimer {
                session_id,
                hold_generation,
            } => self.fire_due_encoder_hold(session_id, hold_generation, now),
        }
    }

    pub fn drain_pending_hardware_events(&mut self, now: Instant) -> Vec<HostActionReceipt> {
        self.update_now = now;
        let events = self.hardware_event_tx.take_batch();
        let mut receipts = Vec::new();
        for ev in events {
            if let Some(receipt) = self.handle_hardware_loop_event(ev, now) {
                receipts.push(receipt);
            }
        }
        receipts
    }

    pub async fn recv_next_hardware_event(
        &mut self,
        now: Instant,
    ) -> Option<Option<HostActionReceipt>> {
        let event = self.hardware_event_tx.recv().await;
        Some(self.handle_hardware_loop_event(event, now))
    }
}

fn parse_hex_rgb(hex: &str) -> Option<u32> {
    let trimmed = hex.trim().strip_prefix('#').unwrap_or(hex.trim());
    if trimmed.len() != 6 {
        return None;
    }
    u32::from_str_radix(trimmed, 16).ok()
}

fn parse_zone_candidate(
    zone: Option<&Value>,
) -> mi_proto::lighting_candidate::LightingZoneCandidate {
    let Some(z) = zone else {
        return mi_proto::lighting_candidate::LightingZoneCandidate::default();
    };
    mi_proto::lighting_candidate::LightingZoneCandidate {
        effect: z
            .get("effect")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        brightness: z
            .get("brightness")
            .and_then(Value::as_f64)
            .map(|v| v.clamp(0.0, 1.0)),
        speed: z
            .get("speed")
            .and_then(Value::as_f64)
            .map(|v| v.clamp(0.0, 1.0)),
        magic: z
            .get("magic")
            .and_then(Value::as_f64)
            .map(|v| v.clamp(0.0, 1.0)),
        color: z
            .get("color")
            .and_then(Value::as_str)
            .and_then(parse_hex_rgb),
    }
}

pub(super) fn parse_hardware_lighting_payload(
    payload_str: &str,
) -> Result<
    (
        mi_proto::lighting_candidate::LightingPreviewCandidate,
        Vec<mi_proto::lighting_candidate::OaiKeyStatusEntry>,
    ),
    String,
> {
    let parsed: Value = serde_json::from_str(payload_str).map_err(|e| e.to_string())?;
    let preview = mi_proto::lighting_candidate::LightingPreviewCandidate {
        backlight: parse_zone_candidate(parsed.get("backlight")),
        underglow: parse_zone_candidate(parsed.get("underglow")),
    };
    mi_proto::lighting_candidate::encode_oai_rgbcfg_candidate(&preview, 0)
        .map_err(|e| e.to_string())?;
    mi_proto::lighting_candidate::encode_lighting_preview_candidate(&preview, 0)
        .map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    if let Some(items) = parsed.get("per_key").and_then(Value::as_array) {
        for item in items {
            let Some(k) = item.get("k").and_then(Value::as_str) else {
                continue;
            };
            let color = item
                .get("color")
                .and_then(Value::as_str)
                .and_then(parse_hex_rgb)
                .unwrap_or(0);
            let brightness = item
                .get("brightness")
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            let effect = item
                .get("e")
                .and_then(Value::as_u64)
                .and_then(|v| u8::try_from(v).ok())
                .unwrap_or(1)
                .min(6);
            let speed = item
                .get("s")
                .and_then(Value::as_f64)
                .unwrap_or(0.65)
                .clamp(0.0, 1.0);
            entries.push(mi_proto::lighting_candidate::OaiKeyStatusEntry::with_style(
                k, color, brightness, effect, speed,
            ));
        }
    }
    if !entries.is_empty() {
        mi_proto::lighting_candidate::encode_oai_thstatus_candidate(&entries, 0)
            .map_err(|e| e.to_string())?;
    }
    Ok((preview, entries))
}

fn default_acceptance_lighting_payload() -> (
    mi_proto::lighting_candidate::LightingPreviewCandidate,
    Vec<mi_proto::lighting_candidate::OaiKeyStatusEntry>,
) {
    let off_zone = mi_proto::lighting_candidate::LightingZoneCandidate {
        effect: Some("off".to_string()),
        brightness: Some(0.0),
        speed: Some(0.0),
        magic: Some(0.5),
        color: Some(0),
    };
    let preview = mi_proto::lighting_candidate::LightingPreviewCandidate {
        backlight: off_zone.clone(),
        underglow: off_zone,
    };
    let keys = [
        "AG00", "AG01", "AG02", "AG03", "AG04", "AG05", "AG06", "AG07", "AG08", "AG09", "AG10",
        "AG11", "AG12",
    ];
    let entries = keys
        .into_iter()
        .map(|k| mi_proto::lighting_candidate::OaiKeyStatusEntry::with_style(k, 0, 0.0, 0, 0.65))
        .collect();
    (preview, entries)
}

pub(super) async fn sync_hardware_lighting_to_session(
    session: &DeviceSession,
    preview: &mi_proto::lighting_candidate::LightingPreviewCandidate,
    entries: &[mi_proto::lighting_candidate::OaiKeyStatusEntry],
) -> (bool, bool, bool) {
    let rgbcfg_ok = session.send_oai_rgbcfg(preview).await.is_ok();
    let preview_ok = session.send_lights_preview(preview).await.is_ok();
    let thstatus_ok = if entries.is_empty() {
        true
    } else {
        session.send_oai_thstatus(entries).await.is_ok()
    };
    (rgbcfg_ok, preview_ok, thstatus_ok)
}

async fn run_automatic_hardware_acceptance(
    session: &DeviceSession,
    target: &mi_transport::HidCandidate,
    initial_lighting_json: &str,
) -> Option<HardwareAutoAcceptanceReceipt> {
    let identity = session.read_identity_info().await.ok()?;
    let _config = session.read_config_snapshot().await.ok()?;
    let snap = session.snapshot()?;
    let keymap_size = snap.files.keymap.meta.size;
    let keymap_sha1 = snap.files.keymap.meta.checksum_hex();
    let keymap_sha256 = crate::lighting_arbiter::sha256_hex(&snap.files.keymap.bytes);
    let smart_actions_present = snap.files.smart_actions.is_some();
    let smart_actions_size = snap.files.smart_actions.as_ref().map(|f| f.meta.size);
    let smart_actions_sha1 = snap
        .files
        .smart_actions
        .as_ref()
        .map(|f| f.meta.checksum_hex());
    let (preview, entries) = if initial_lighting_json.trim().is_empty() {
        default_acceptance_lighting_payload()
    } else {
        parse_hardware_lighting_payload(initial_lighting_json)
            .unwrap_or_else(|_| default_acceptance_lighting_payload())
    };
    let (rgbcfg_ok, preview_ok, thstatus_ok) =
        sync_hardware_lighting_to_session(session, &preview, &entries).await;
    let receipt = HardwareAutoAcceptanceReceipt {
        schema_version: 1,
        status: "PASS".to_string(),
        session_id: session.id,
        backend: session.backend_name().to_string(),
        exclusive_vendor: session.has_exclusive_vendor_link(),
        product: target.product.clone(),
        vid_pid: format!("{:04X}:{:04X}", target.vid, target.pid),
        device_path: target.path.clone(),
        firmware_version: identity.version.unwrap_or_else(|| "unknown".to_string()),
        profile_index: identity.profile_index,
        layer_index: identity.layer_index,
        keymap_size,
        keymap_sha1,
        keymap_sha256,
        smart_actions_present,
        smart_actions_size,
        smart_actions_sha1,
        lighting_rgbcfg_synced: rgbcfg_ok,
        lighting_preview_synced: preview_ok,
        lighting_thstatus_synced: thstatus_ok,
        timestamp_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
    };
    Some(receipt)
}

pub(super) fn parse_iso8601_utc_ms(raw: &str) -> u64 {
    let trimmed = raw.trim().strip_suffix('Z').unwrap_or(raw.trim());
    let Some((date_part, time_part)) = trimmed.split_once('T') else {
        return 0;
    };
    let mut date_it = date_part.split('-');
    let (Some(y_s), Some(m_s), Some(d_s)) = (date_it.next(), date_it.next(), date_it.next()) else {
        return 0;
    };
    let (Ok(mut y), Ok(m), Ok(d)) = (y_s.parse::<i64>(), m_s.parse::<i64>(), d_s.parse::<i64>())
    else {
        return 0;
    };
    let (hms_part, frac_part) = time_part.split_once('.').unwrap_or((time_part, ""));
    let mut hms_it = hms_part.split(':');
    let (Some(hh_s), Some(mm_s), Some(ss_s)) = (hms_it.next(), hms_it.next(), hms_it.next()) else {
        return 0;
    };
    let (Ok(hh), Ok(mm), Ok(ss)) = (
        hh_s.parse::<u64>(),
        mm_s.parse::<u64>(),
        ss_s.parse::<u64>(),
    ) else {
        return 0;
    };
    let mut frac_ms = 0u64;
    for (idx, ch) in frac_part.chars().take(3).enumerate() {
        if let Some(digit) = ch.to_digit(10) {
            let weight = match idx {
                0 => 100,
                1 => 10,
                _ => 1,
            };
            frac_ms += u64::from(digit) * weight;
        }
    }
    if m <= 2 {
        y -= 1;
    }
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = (if m > 2 { m - 3 } else { m + 9 }) as u64;
    let doy = (153 * mp + 2) / 5 + (d as u64) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + (doe as i64) - 719468;
    if days < 0 {
        return 0;
    }
    (days as u64) * 86_400_000 + (hh * 3600 + mm * 60 + ss) * 1000 + frac_ms
}
