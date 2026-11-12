use super::*;
use crate::six_layer_runtime::{self, InstallationStatus};
use mi_model::apply::AppliedSixLayerPlan;

impl WorkbenchStore {
    fn compiled_six_layer_plan(&self) -> Result<AppliedSixLayerPlan, String> {
        self.projection_inputs().compiled_six_layer_plan()
    }

    pub(crate) fn compile_profile_twenty_slots(
        &self,
        profile_id: &str,
    ) -> Option<six_layer_runtime::PinnedProfilePlan> {
        let doc = &self.document_store.host_store().draft.document;
        let profile = doc.editable.graph.profiles.get(profile_id)?;
        let canonical = CanonicalAgent::from_profile_name(&profile.name);
        let bundle_id = canonical
            .map(CanonicalAgent::bundle_id)
            .filter(|bundle| *bundle != "*")
            .map(str::to_owned)
            .or_else(|| {
                self.app_profile_links
                    .iter()
                    .find(|(bundle, pid)| bundle.as_str() != "*" && pid.as_str() == profile_id)
                    .map(|(bundle, _)| bundle.clone())
            });
        let adapter_name = match canonical {
            Some(CanonicalAgent::Antigravity) => Some("antigravity"),
            Some(CanonicalAgent::Codex) => Some("codex"),
            Some(CanonicalAgent::DeepseekHarness) => Some("dsh-desktop"),
            Some(CanonicalAgent::Herdr) => Some("herdr"),
            _ => None,
        };
        let is_protected_oai = profile
            .layer_order
            .iter()
            .any(|lid| doc.is_protected_oai_layer(lid));
        if is_protected_oai {
            return Some(six_layer_runtime::PinnedProfilePlan {
                profile_id: profile_id.to_string(),
                bundle_id,
                actions: std::array::from_fn(|_| None),
            });
        }
        let layout = self.current_bottom_row_layout();
        let command_execution = self.command_execution_enabled;
        let actions = std::array::from_fn(|slot| {
            let mut action = resolve_host_action_with_layout(
                doc,
                Some(profile_id),
                mi_model::direct_controls::CONTROL_BINDINGS[slot],
                true,
                command_execution,
                layout,
            )?;
            if matches!(
                action,
                ResolvedHostAction::MacroSequence { .. }
                    | ResolvedHostAction::Shortcut { key_code: 0x3f, .. }
            ) {
                return None;
            }
            if let ResolvedHostAction::AgentCommand { adapter, command } = &mut action {
                if adapter == "host" {
                    if let Some(target_adapter) = adapter_name {
                        *adapter = target_adapter.into();
                    }
                }
                if matches!(command.as_str(), "push-to-talk" | "dictation") {
                    *command = "toggle-dictation".into();
                }
            }
            match &mut action {
                ResolvedHostAction::Shortcut {
                    target_bundle_id, ..
                } => {
                    if target_bundle_id.is_none() {
                        *target_bundle_id = bundle_id.clone();
                    }
                }
                ResolvedHostAction::PasteText {
                    target_bundle_id,
                    adapter,
                    ..
                } => {
                    if target_bundle_id.is_none() {
                        *target_bundle_id = bundle_id.clone();
                    }
                    if adapter.is_none() {
                        *adapter = adapter_name.map(str::to_owned);
                    }
                }
                _ => {}
            }
            Some(action)
        });
        Some(six_layer_runtime::PinnedProfilePlan {
            profile_id: profile_id.to_string(),
            bundle_id,
            actions,
        })
    }

    pub(crate) fn build_six_layer_routing_mode(&self) -> six_layer_runtime::SixLayerRoutingMode {
        if let Some(locked_id) = self.document_store.locked_profile_id() {
            if let Some(plan) = self.compile_profile_twenty_slots(&locked_id) {
                return six_layer_runtime::SixLayerRoutingMode::Locked(plan);
            }
        }
        if !self.auto_follow_enabled || self.manual_override {
            if let Some(active_id) = self.document_store.host_active_profile_id() {
                if let Some(plan) = self.compile_profile_twenty_slots(&active_id) {
                    return six_layer_runtime::SixLayerRoutingMode::ManualPinned(plan);
                }
            }
        }
        let mut dynamic_hosts = Vec::new();
        let mut seen_bundles = BTreeSet::new();
        for (bundle, pid) in &self.app_profile_links {
            if bundle == "*" || bundle == "com.openai.codex" || !seen_bundles.insert(bundle.clone())
            {
                continue;
            }
            if let Some(mut plan) = self.compile_profile_twenty_slots(pid) {
                plan.bundle_id = Some(bundle.clone());
                if plan.actions.iter().any(Option::is_some) {
                    dynamic_hosts.push(plan);
                }
            }
        }
        for agent in [
            CanonicalAgent::Antigravity,
            CanonicalAgent::DeepseekHarness,
            CanonicalAgent::Herdr,
        ] {
            let bundle = agent.bundle_id();
            if !seen_bundles.insert(bundle.to_string()) {
                continue;
            }
            if let Some(pid) = self.find_canonical_profile_id(agent) {
                if let Some(mut plan) = self.compile_profile_twenty_slots(&pid) {
                    plan.bundle_id = Some(bundle.to_string());
                    if plan.actions.iter().any(Option::is_some) {
                        dynamic_hosts.push(plan);
                    }
                }
            }
        }
        six_layer_runtime::SixLayerRoutingMode::AutoFollow { dynamic_hosts }
    }

    pub(crate) fn sync_six_layer_routing_context(&mut self) {
        self.last_synced_six_layer_revs = (
            self.document_store.draft.revision,
            self.document_store.host_revision(),
        );
        if self.six_layer_running.is_none() {
            return;
        }
        let mode = self.build_six_layer_routing_mode();
        let bundle = self.foreground_bundle_id.clone();
        if let Some(running) = &self.six_layer_running {
            running.set_routing_context(mode, bundle.as_deref());
        }
    }

    pub(super) fn try_auto_resume_six_layer(&mut self, now: Instant) {
        if self.config.isolated
            || !self.config.auto_connect_usb
            || self.system_lifecycle != SystemLifecycle::Awake
            || self.six_layer_job.is_some()
            || self
                .six_layer_running
                .as_ref()
                .is_some_and(|running| running.receiver.failure().is_none())
        {
            return;
        }
        let Some(plan) = self
            .six_layer_state
            .installation
            .as_ref()
            .filter(|inst| {
                inst.status == InstallationStatus::Applied && inst.plan.validate_current().is_ok()
            })
            .map(|inst| inst.plan.clone())
        else {
            return;
        };
        if self.six_layer_auto_resume_attempts >= 3
            || self
                .six_layer_next_auto_resume_at
                .is_some_and(|ready_at| now < ready_at)
        {
            return;
        }
        if mi_transport::discover_target_keyboard().is_err() {
            return;
        }
        self.six_layer_auto_resume_attempts += 1;
        self.six_layer_next_auto_resume_at = Some(now + Duration::from_secs(3));
        self.resume_requires_explicit_connect = false;
        let _ = self.start_six_layer_apply(Some(plan));
    }

    fn suspend_for_six_layer_operation(&mut self) -> Result<(), WorkbenchError> {
        self.cancel_session_navigation();
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
            .is_some_and(|installation| {
                installation.status != InstallationStatus::Applied
                    || installation.plan.validate_current().is_err()
            })
        {
            return Err(WorkbenchError::Routing(
                "六层恢复记录尚未处理，请先恢复原设备配置".into(),
            ));
        }
        plan.validate_current().map_err(WorkbenchError::Routing)?;
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
                    self.six_layer_auto_resume_attempts = 0;
                    self.six_layer_next_auto_resume_at = None;
                    self.device_session = None;
                    self.connected_candidate = Some(running.candidate.clone());
                    self.hardware_rpc_verified = true;
                    running.armed = running.receiver.arm().is_ok();
                    running.set_execution_policy(
                        self.document_store.host_execution_enabled(),
                        self.command_execution_enabled,
                        self.update_now,
                    );
                    running.navigation = Some(self.session_navigation.dispatch.clone());
                    running.start_executor(Arc::clone(&self.live_executor));
                    running.start_monitor();
                    running.start_vendor_bridge(&self.tokio_runtime);
                    self.six_layer_running = Some(running);
                    self.sync_six_layer_routing_context();
                    self.status_message = "六层配置已读回，等待首次实体输入验证".into();
                    self.lighting_arbiter.invalidate_hardware_cache();
                    self.refresh_four_app_lighting();
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
            self.try_auto_resume_six_layer(self.update_now);
            return;
        }
        let (mut lighting_changed, need_lighting_resync) = {
            let running = self.six_layer_running.as_ref().unwrap();
            let confirmed = running.confirmed_lighting_digest();
            let changed = self
                .lighting_arbiter
                .reconcile_six_layer_confirmed(confirmed.as_deref());
            (changed, running.vendor_bridge_needs_lighting_sync())
        };
        if need_lighting_resync {
            let prev_synced = self.lighting_arbiter.hardware_synced;
            let prev_writes = self.lighting_arbiter.hardware_write_count;
            let prev_feedback = self.lighting_arbiter.full_keypad_feedback.clone();
            self.refresh_four_app_lighting();
            if self.lighting_arbiter.hardware_synced != prev_synced
                || self.lighting_arbiter.hardware_write_count != prev_writes
                || self.lighting_arbiter.full_keypad_feedback != prev_feedback
            {
                lighting_changed = true;
            }
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
            || !self.ptt_controller.cleanup_confirmed
        {
            running
                .receiver
                .invalidate("宿主动作清理未确认，输入已暂停");
        }
        let observed = running.observed_layer.load(Ordering::Acquire);
        let first = running.first_event_verified.load(Ordering::Acquire);
        let failure = running.receiver.failure();
        let needs_cleanup = failure.is_some() && !running.is_retired();
        let changed = lighting_changed
            || self.projection.sidebar.board_layers.observed_layer != Some(observed)
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
                    "unconfirmed" => "动作读回未确认",
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
        self.try_auto_resume_six_layer(self.update_now);
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
    fn six_layer_banner_tracks_input_connection_and_user_pause_preserves_receiver() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(directory.path()).unwrap();
        let session = mock_session(&store.tokio_runtime);
        let mut running =
            six_layer_runtime::RunningSixLayer::isolated(session.clone(), directory.path());
        running.readiness.configuration_observed = true;
        running.readiness.keyboard_grabbed = true;
        running.receiver.arm().unwrap();
        running.armed = true;
        store.tokio_runtime.block_on(session.shutdown());
        assert!(session.is_disconnected());
        store.connected_candidate = Some(running.candidate.clone());
        store.six_layer_state.installation = Some(running.installation.clone());
        store.six_layer_running = Some(running);
        store.set_host_execution_enabled(true).unwrap();
        store.poll_six_layer_runtime();
        assert!(store.projection.sidebar.board_layers.runtime_ready);
        assert_eq!(
            store.projection.header.banner,
            OwnershipBannerState::Ready {
                summary: "六层标准键输入已连接".into(),
            }
        );
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
        assert!(matches!(
            store.projection.header.banner,
            OwnershipBannerState::Ready { .. }
        ));
        store
            .six_layer_running
            .as_ref()
            .unwrap()
            .receiver
            .invalidate("USB removed");
        store.commit_runtime_update();
        assert_eq!(
            store.projection.header.banner,
            OwnershipBannerState::Caution {
                reason: "USB removed".into(),
            }
        );
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
    #[test]
    fn retired_installation_remains_loadable_recoverable_and_never_starts() {
        let directory = tempfile::tempdir().unwrap();
        let store = WorkbenchStore::open_isolated(directory.path()).unwrap();
        let session = mock_session(&store.tokio_runtime);
        let running =
            six_layer_runtime::RunningSixLayer::isolated(session.clone(), directory.path());
        let mut record = serde_json::json!({
            "version": 1,
            "cindy_native_paused_confirmed": true,
            "installation": running.installation
        });
        let mut old_host = record["installation"]["plan"]["hosts"][1].clone();
        old_host["adapter"] = serde_json::json!("cindy");
        old_host["profile_id"] = serde_json::json!("saved-cindy");
        record["installation"]["plan"]["hosts"]
            .as_array_mut()
            .unwrap()
            .push(old_host);
        record["installation"]["plan"]["desired"]["layers"][3] =
            serde_json::json!({"kind":"HostProfile","profile_id":"saved-cindy"});
        let original = serde_json::to_vec_pretty(&record).unwrap();
        let path = directory.path().join("six-layer-runtime.json");
        fs::write(&path, &original).unwrap();
        drop(running);
        store.tokio_runtime.block_on(session.shutdown());
        drop(store);
        let mut reopened = WorkbenchStore::open_isolated(directory.path()).unwrap();
        assert!(reopened.projection.sidebar.board_layers.restore_available);
        assert!(!reopened.projection.sidebar.board_layers.runtime_ready);
        assert!(reopened.six_layer_running.is_none());
        let plan = reopened
            .six_layer_state
            .installation
            .as_ref()
            .unwrap()
            .plan
            .clone();
        assert_eq!(plan.hosts[2].adapter, "cindy");
        assert!(reopened.start_six_layer_apply(Some(plan)).is_err());
        let result = reopened.tokio_runtime.block_on(six_layer_runtime::resume(
            directory.path(),
            reopened.six_layer_state.clone(),
        ));
        assert!(matches!(result, Err(error) if error.contains("旧六层安装已暂停")));
        assert_eq!(fs::read(&path).unwrap(), original);
        let mut invalid = record;
        invalid["unrecognized"] = serde_json::json!(true);
        fs::write(&path, serde_json::to_vec(&invalid).unwrap()).unwrap();
        assert!(six_layer_runtime::load(&path).is_err());
    }

    struct MacMockLink {
        inner: Arc<mi_transport::MockLink>,
        exclusive: bool,
    }

    impl mi_transport::HidLink for MacMockLink {
        fn backend(&self) -> mi_transport::LinkBackend {
            mi_transport::LinkBackend::MacHidapi267Numbered
        }
        fn is_exclusive_vendor(&self) -> bool {
            self.exclusive
        }
        fn write<'life0, 'life1, 'async_trait>(
            &'life0 self,
            report: &'life1 [u8],
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<(), mi_transport::TransportError>>
                    + Send
                    + 'async_trait,
            >,
        >
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            Self: 'async_trait,
        {
            self.inner.write(report)
        }
        fn read<'life0, 'async_trait>(
            &'life0 self,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<Vec<u8>, mi_transport::TransportError>>
                    + Send
                    + 'async_trait,
            >,
        >
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            self.inner.read()
        }
        fn close(&self) {
            self.inner.close()
        }
    }

    fn spawn_thstatus_mock_session_with_fault(
        runtime: &tokio::runtime::Runtime,
        fail_next_rpc: Arc<AtomicBool>,
    ) -> (
        Arc<mi_transport::MockLink>,
        Arc<DeviceSession>,
        std::sync::mpsc::Receiver<serde_json::Value>,
    ) {
        let (link, mut writes) = mi_transport::MockLink::new();
        let inject = link.inject.clone();
        let (rpc_tx, rpc_rx) = std::sync::mpsc::channel();
        let session = {
            let _entered = runtime.enter();
            DeviceSession::new(
                Arc::new(MacMockLink {
                    inner: link.clone(),
                    exclusive: false,
                }),
                Box::new(mi_proto::NativeFramer::default()),
                Arc::new(mi_session::ReadOnlySchema),
                mi_session::SessionConfig {
                    timeout: Duration::from_secs(2),
                    interval: Duration::ZERO,
                },
            )
        };
        runtime.spawn(async move {
            use mi_proto::Framer;
            let mut in_framer = mi_proto::NativeFramer::default();
            let out_framer = mi_proto::NativeFramer::default();
            while let Some(report) = writes.recv().await {
                let Ok(messages) = in_framer.feed(&report) else {
                    break;
                };
                for msg in messages {
                    let Ok(req) = serde_json::from_slice::<serde_json::Value>(&msg) else {
                        continue;
                    };
                    let _ = rpc_tx.send(req.clone());
                    let reply = if fail_next_rpc.swap(false, Ordering::AcqRel) {
                        serde_json::json!({
                            "id": req["id"],
                            "error": {"code": -32000, "message": "busy"}
                        })
                    } else {
                        serde_json::json!({
                            "id": req["id"],
                            "result": {}
                        })
                    };
                    let mut wire = reply.to_string();
                    wire.push('\n');
                    for frame in out_framer.encode(wire.as_bytes()).unwrap() {
                        let _ = inject.send(frame);
                    }
                }
            }
        });
        (link, session, rpc_rx)
    }

    fn spawn_thstatus_mock_session(
        runtime: &tokio::runtime::Runtime,
    ) -> (
        Arc<mi_transport::MockLink>,
        Arc<DeviceSession>,
        std::sync::mpsc::Receiver<serde_json::Value>,
    ) {
        spawn_thstatus_mock_session_with_fault(runtime, Arc::new(AtomicBool::new(false)))
    }

    fn wait_lighting_synced(store: &mut WorkbenchStore, expected: bool) {
        let wait = Instant::now();
        while store.lighting_arbiter.hardware_synced != expected
            && wait.elapsed() < Duration::from_secs(4)
        {
            store.poll_six_layer_runtime();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(store.lighting_arbiter.hardware_synced, expected);
    }

    #[test]
    fn six_layer_v2_green_and_blue_per_key_sync_without_preview_or_rgbcfg_and_extinguishes_on_disable(
    ) {
        let directory = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(directory.path()).unwrap();
        let (_link, session, rpc_rx) = spawn_thstatus_mock_session(&store.tokio_runtime);
        let mut running =
            six_layer_runtime::RunningSixLayer::isolated(session.clone(), directory.path());
        running.readiness.configuration_observed = true;
        running.readiness.keyboard_grabbed = true;
        running.attach_test_vendor_session(&store.tokio_runtime, session);
        store.connected_candidate = Some(running.candidate.clone());
        store.six_layer_state.installation = Some(running.installation.clone());
        store.six_layer_running = Some(running);

        store.ingest_four_app_candidates(vec![
            FourAppLiveCandidate {
                place: mi_model::navigation_history::NavigationPlace::antigravity(
                    "conv-running",
                    "/ws/run",
                ),
                title: "Running Task".into(),
                phase: FourAppSessionKind::Working,
                watermark: "wm-run".into(),
                last_modified_ms: 9_000,
                last_user_view_ms: 9_000,
                updated_at_ms: 9_000,
            },
            FourAppLiveCandidate {
                place: mi_model::navigation_history::NavigationPlace::antigravity(
                    "conv-completed",
                    "/ws/done",
                ),
                title: "Completed Task".into(),
                phase: FourAppSessionKind::BlueDotCompleted,
                watermark: "wm-done".into(),
                last_modified_ms: 8_000,
                last_user_view_ms: 1_000,
                updated_at_ms: 8_000,
            },
        ]);

        let first_rpc = rpc_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(first_rpc["method"], "v.oai.thstatus");
        wait_lighting_synced(&mut store, true);
        let keys = first_rpc["params"].as_array().unwrap();
        let key_names: Vec<&str> = keys.iter().map(|k| k["k"].as_str().unwrap()).collect();
        assert_eq!(
            key_names,
            vec![
                "AG06", "AG07", "AG08", "AG09", "AG10", "AG11", "AG12", "AG13", "AG14", "AG15",
                "AG16", "AG17"
            ]
        );
        assert!(key_names
            .iter()
            .all(|k| !matches!(*k, "AG00" | "AG01" | "AG02" | "AG03" | "AG04" | "AG05")));
        assert_eq!(keys[0]["c"], 0x00FF66);
        assert_eq!(keys[6]["c"], 0x00FF66);
        assert_eq!(keys[1]["c"], 0x0088FF);
        assert_eq!(keys[7]["c"], 0x0088FF);
        for empty_idx in [2, 3, 4, 5, 8, 9, 10, 11] {
            assert_eq!(keys[empty_idx]["c"], 0);
            assert_eq!(keys[empty_idx]["b"], 0.0);
        }

        store
            .dispatch_session_strip_action(SessionStripAction::TriggerTestLighting(Some(
                FourAppAgentPhase::Error,
            )))
            .unwrap();
        let preview_rpc = rpc_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(preview_rpc["method"], "v.oai.thstatus");
        wait_lighting_synced(&mut store, true);
        assert!(preview_rpc["params"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["c"] == 0xFF2200));

        store
            .dispatch_session_strip_action(SessionStripAction::SetLightingEnabled(false))
            .unwrap();
        let extinguish_rpc = rpc_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(extinguish_rpc["method"], "v.oai.thstatus");
        assert!(!store.lighting_arbiter.hardware_synced);
        let off_keys = extinguish_rpc["params"].as_array().unwrap();
        assert_eq!(off_keys.len(), 12);
        assert!(off_keys
            .iter()
            .all(|entry| entry["c"] == 0 && entry["b"] == 0.0 && entry["e"] == 0));
        assert!(rpc_rx.recv_timeout(Duration::from_millis(50)).is_err());

        store
            .six_layer_running
            .take()
            .unwrap()
            .shutdown(&store.tokio_runtime)
            .unwrap();
    }

    #[test]
    fn six_layer_v2_vendor_hid_slot_press_routes_through_executor_and_preserves_r21_on_vendor_disconnect(
    ) {
        use mi_proto::Framer;
        let directory = tempfile::tempdir().unwrap();
        let store = WorkbenchStore::open_isolated(directory.path()).unwrap();
        let (link, session, _rpc_rx) = spawn_thstatus_mock_session(&store.tokio_runtime);
        let mut running =
            six_layer_runtime::RunningSixLayer::isolated(session.clone(), directory.path());
        running.installation.plan = store.compiled_six_layer_plan().unwrap();
        running.readiness.configuration_observed = true;
        running.readiness.keyboard_grabbed = true;
        running.receiver.arm().unwrap();
        running.armed = true;
        running.set_execution_policy(true, true, Instant::now() - Duration::from_secs(1));
        running.navigation = Some(store.session_navigation.dispatch.clone());
        running.start_executor(Arc::clone(&store.live_executor));
        running.attach_test_vendor_session(&store.tokio_runtime, session.clone());

        let framer = mi_proto::NativeFramer::default();
        for packet in [
            r#"{"m":"v.oai.hid","p":{"k":"AG06","act":1}}"#,
            r#"{"m":"v.oai.hid","p":{"k":"AG06","act":1}}"#,
            r#"{"m":"v.oai.hid","p":{"k":"AG06","act":0}}"#,
        ] {
            let mut line = packet.to_string();
            line.push('\n');
            for frame in framer.encode(line.as_bytes()).unwrap() {
                link.inject.send(frame).unwrap();
            }
        }

        let start = Instant::now();
        while !running.first_event_verified.load(Ordering::Acquire)
            && start.elapsed() < Duration::from_secs(2)
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(running.first_event_verified.load(Ordering::Acquire));
        assert_eq!(running.observed_layer.load(Ordering::Acquire), 2);
        assert!(running.receiver.failure().is_none());

        store.tokio_runtime.block_on(session.shutdown());
        let disconnect_wait = Instant::now();
        while running.vendor_bridge_connected()
            && disconnect_wait.elapsed() < Duration::from_secs(2)
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!running.vendor_bridge_connected());
        assert!(running.receiver.failure().is_none());

        running.shutdown(&store.tokio_runtime).unwrap();
    }

    #[test]
    fn six_layer_disconnected_and_faulty_bridge_report_truthful_unsynced_lighting_until_confirmed()
    {
        let directory = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(directory.path()).unwrap();
        let closed_session = mock_session(&store.tokio_runtime);
        store.tokio_runtime.block_on(closed_session.shutdown());
        let mut running =
            six_layer_runtime::RunningSixLayer::isolated(closed_session, directory.path());
        running.readiness.configuration_observed = true;
        running.readiness.keyboard_grabbed = true;
        store.connected_candidate = Some(running.candidate.clone());
        store.six_layer_state.installation = Some(running.installation.clone());
        store.six_layer_running = Some(running);

        store.ingest_four_app_candidates(vec![FourAppLiveCandidate {
            place: mi_model::navigation_history::NavigationPlace::antigravity(
                "conv-run", "/ws/run",
            ),
            title: "Running Task".into(),
            phase: FourAppSessionKind::Working,
            watermark: "wm-1".into(),
            last_modified_ms: 5_000,
            last_user_view_ms: 5_000,
            updated_at_ms: 5_000,
        }]);
        assert!(!store.lighting_arbiter.hardware_synced);
        assert_eq!(
            store.projection.sidebar.board_layers.observed_status,
            "六层配置已核验 · 最近确认第 1 层 · 等待首次实体输入"
        );

        let fail_first = Arc::new(AtomicBool::new(true));
        let (_link, vendor_session, rpc_rx) =
            spawn_thstatus_mock_session_with_fault(&store.tokio_runtime, Arc::clone(&fail_first));
        store
            .six_layer_running
            .as_mut()
            .unwrap()
            .attach_test_vendor_session(&store.tokio_runtime, vendor_session.clone());
        let failed_rpc = rpc_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(failed_rpc["method"], "v.oai.thstatus");
        assert!(!store.lighting_arbiter.hardware_synced);
        store.poll_six_layer_runtime();
        let gen_after_failure = store.projection_generation();
        for _ in 0..5 {
            store.poll_six_layer_runtime();
            assert!(rpc_rx.recv_timeout(Duration::from_millis(30)).is_err());
        }
        assert_eq!(store.projection_generation(), gen_after_failure);

        wait_lighting_synced(&mut store, true);
        let synced_rpc = rpc_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(synced_rpc["method"], "v.oai.thstatus");
        assert_eq!(synced_rpc["params"][0]["c"], 0x00FF66);
        assert_eq!(synced_rpc["params"][6]["c"], 0x00FF66);

        store.tokio_runtime.block_on(vendor_session.shutdown());
        wait_lighting_synced(&mut store, false);
        store
            .dispatch_session_strip_action(SessionStripAction::SetLightingEnabled(false))
            .unwrap();
        let (_link2, vendor_reconnected, rpc_rx2) =
            spawn_thstatus_mock_session(&store.tokio_runtime);
        store
            .six_layer_running
            .as_mut()
            .unwrap()
            .attach_test_vendor_session(&store.tokio_runtime, vendor_reconnected);
        let reconnect_rpc = rpc_rx2.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(reconnect_rpc["method"], "v.oai.thstatus");
        assert!(reconnect_rpc["params"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["c"] == 0 && entry["b"] == 0.0));

        store
            .six_layer_running
            .take()
            .unwrap()
            .shutdown(&store.tokio_runtime)
            .unwrap();
    }

    #[test]
    fn selecting_codex_in_draft_sidebar_keeps_editor_locked_without_suspending_live_six_layer_lighting(
    ) {
        let directory = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(directory.path()).unwrap();
        let (_link, session, rpc_rx) = spawn_thstatus_mock_session(&store.tokio_runtime);
        let mut running =
            six_layer_runtime::RunningSixLayer::isolated(session.clone(), directory.path());
        running.installation.plan = store.compiled_six_layer_plan().unwrap();
        running.readiness.configuration_observed = true;
        running.readiness.keyboard_grabbed = true;
        running.attach_test_vendor_session(&store.tokio_runtime, session);
        store.connected_candidate = Some(running.candidate.clone());
        store.six_layer_state.installation = Some(running.installation.clone());
        store.six_layer_running = Some(running);

        store.ingest_four_app_candidates(vec![
            FourAppLiveCandidate {
                place: mi_model::navigation_history::NavigationPlace::antigravity(
                    "conv-green",
                    "/ws/green",
                ),
                title: "Running Green".into(),
                phase: FourAppSessionKind::Working,
                watermark: "wm-green".into(),
                last_modified_ms: 9_000,
                last_user_view_ms: 9_000,
                updated_at_ms: 9_000,
            },
            FourAppLiveCandidate {
                place: mi_model::navigation_history::NavigationPlace::antigravity(
                    "conv-blue",
                    "/ws/blue",
                ),
                title: "Completed Blue".into(),
                phase: FourAppSessionKind::BlueDotCompleted,
                watermark: "wm-blue".into(),
                last_modified_ms: 8_000,
                last_user_view_ms: 1_000,
                updated_at_ms: 8_000,
            },
        ]);
        let initial_rpc = rpc_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(initial_rpc["method"], "v.oai.thstatus");
        wait_lighting_synced(&mut store, true);

        let codex_pid = store
            .find_canonical_profile_id(CanonicalAgent::Codex)
            .unwrap();
        store
            .dispatch_sidebar_action(SidebarAction::SelectProfile(codex_pid))
            .unwrap();

        assert!(store.is_selected_layer_protected());
        assert!(!store.is_active_lighting_protected());
        assert!(store.projection().canvas.protected_lock.is_some());
        assert!(!store.projection().session_strip.can_mutate_layout);
        assert!(
            !store
                .projection()
                .session_strip
                .lighting_popover
                .suspended_by_protected_layer
        );
        assert!(store.lighting_arbiter.hardware_synced);
        assert_eq!(
            store.projection().session_strip.slots[0].led_color_hex,
            "#00FF66"
        );
        assert_eq!(
            store.projection().session_strip.slots[1].led_color_hex,
            "#0088FF"
        );
        assert_eq!(
            store.projection().session_strip.working_pills[0].color_hex,
            "#00FF66"
        );
        assert_eq!(
            store.projection().session_strip.completed_blue_dot_pills[0].color_hex,
            "#0088FF"
        );

        store
            .six_layer_running
            .take()
            .unwrap()
            .shutdown(&store.tokio_runtime)
            .unwrap();
    }

    use crate::six_layer_runtime::{test_b64_decode, test_b64_encode};

    #[test]
    fn six_layer_breaking_upgrade_preserves_original_backup_chain_for_exact_restore() {
        const SIX_SOURCE: &[u8] =
            include_bytes!("../../mi-model/fixtures/device/synthetic-six-layer-source.json");
        const SMART: &[u8] =
            include_bytes!("../../mi-model/fixtures/device/synthetic-smart-actions-object.json");
        const STALE_V1_SMART: &[u8] = br#"{"smart_actions":[]}"#;

        let v2_template =
            mi_model::apply::compile_six_layer_profile(SIX_SOURCE, Some(SMART)).unwrap();
        let mut v1_value: serde_json::Value = serde_json::from_slice(&v2_template.keymap).unwrap();
        for layer_idx in 1..6 {
            let tokens: Vec<&str> = if (1..=2).contains(&layer_idx) {
                mi_model::direct_controls::DIRECT_CONTROLS[(layer_idx - 1) * 20..layer_idx * 20]
                    .iter()
                    .map(|c| c.token)
                    .collect()
            } else {
                vec!["KC_NONE"; 20]
            };
            let rows = serde_json::json!([
                &tokens[0..2],
                &tokens[2..6],
                &tokens[6..10],
                &tokens[10..13]
            ]);
            let layer_obj = v1_value["profiles"][1]["layers"][layer_idx]
                .as_object_mut()
                .unwrap();
            layer_obj.remove("lights");
            layer_obj["layout"]["base"] = rows.clone();
            layer_obj["layout"]["keymap"] = rows;
            layer_obj["layout"]["encoders"] = serde_json::json!([&tokens[13..16]]);
            for slot in 0..4 {
                layer_obj["layout"]["joystick"]["sectors"][slot]["k"] =
                    serde_json::json!(tokens[16 + slot]);
            }
        }
        let raw_doc: std::collections::BTreeMap<&str, &serde_json::value::RawValue> =
            serde_json::from_slice(SIX_SOURCE).unwrap();
        let raw_profiles: Vec<std::collections::BTreeMap<&str, &serde_json::value::RawValue>> =
            serde_json::from_str(raw_doc["profiles"].get()).unwrap();
        let raw_layers: Vec<&serde_json::value::RawValue> =
            serde_json::from_str(raw_profiles[0]["layers"].get()).unwrap();
        let raw_layer0 = raw_layers[0].get();
        let v1_managed_strings: Vec<String> = (1..6)
            .map(|i| serde_json::to_string(&v1_value["profiles"][1]["layers"][i]).unwrap())
            .collect();
        let v1_profile = format!(
            "{{\"id\":1,\"name\":\"{}\",\"layers\":[{},{}]}}",
            mi_model::apply::SIX_LAYER_PROFILE_NAME,
            raw_layer0,
            v1_managed_strings.join(",")
        );
        let v1_bytes = std::str::from_utf8(SIX_SOURCE)
            .unwrap()
            .replacen("\"activeProfileId\": 0", "\"activeProfileId\": 1", 1)
            .replacen(
                "  }] }],\n  \"macros\"",
                &format!("  }}] }},{v1_profile}],\n  \"macros\""),
                1,
            )
            .into_bytes();

        let dir = tempfile::tempdir().unwrap();
        let v1_backup = dir.path().join("six-layer-backup-v1");
        fs::create_dir_all(&v1_backup).unwrap();
        fs::write(v1_backup.join("keymap.json"), SIX_SOURCE).unwrap();
        fs::write(v1_backup.join("planned-keymap.json"), &v1_bytes).unwrap();
        fs::write(v1_backup.join("smart_actions.json"), STALE_V1_SMART).unwrap();
        fs::write(
            v1_backup.join("manifest.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "format": "micro-input-three-layer-recovery-v1",
                "previous_profile_index": 0,
                "previous_layer_index": 1,
                "planned_profile_index": 1,
                "keymap_sha256": mi_bridge::lighting_arbiter::sha256_hex(SIX_SOURCE),
                "planned_keymap_sha256": mi_bridge::lighting_arbiter::sha256_hex(&v1_bytes),
            }))
            .unwrap(),
        )
        .unwrap();

        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            use mi_proto::Framer;
            let (link, mut requests) = mi_transport::MockLink::new();
            let inject = link.inject.clone();
            let session = DeviceSession::new(
                Arc::new(MacMockLink {
                    inner: link,
                    exclusive: true,
                }),
                Box::new(mi_proto::NativeFramer::default()),
                Arc::new(mi_session::ReadOnlySchema),
                mi_session::SessionConfig {
                    timeout: Duration::from_secs(2),
                    interval: Duration::ZERO,
                },
            );
            let initial_v1 = v1_bytes.clone();
            let worker = tokio::spawn(async move {
                let mut incoming = mi_proto::NativeFramer::default();
                let outbound = mi_proto::NativeFramer::default();
                let mut active = 1u64;
                let mut keymap = initial_v1;
                let mut staged = Vec::new();
                while let Some(frame) = requests.recv().await {
                    for bytes in incoming.feed(&frame).unwrap() {
                        let req: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                        let params = &req["params"];
                        let meta = |name: &str, data: &[u8]| {
                            let sha1 = mi_model::operation_record::file_value(data)["sha1"]
                                .as_str()
                                .unwrap()
                                .to_owned();
                            serde_json::json!({"name": name, "size": data.len(), "checksum": sha1})
                        };
                        let result = match req["method"].as_str().unwrap() {
                            "device.status" => serde_json::json!({
                                "version": "0.6.3",
                                "profile_index": active,
                                "layer_index": 2
                            }),
                            "fs.list" => serde_json::json!([
                                meta("keymap.json", &keymap),
                                meta("smart_actions.json", SMART)
                            ]),
                            "fs.readbin" => {
                                let file_bytes = match params["file"].as_str().unwrap() {
                                    "keymap.json" => keymap.as_slice(),
                                    "smart_actions.json" => SMART,
                                    other => panic!("unexpected file {other}"),
                                };
                                let offset = params["offset"].as_u64().unwrap() as usize;
                                let len = params["len"].as_u64().unwrap() as usize;
                                let end = (offset + len).min(file_bytes.len());
                                serde_json::json!({
                                    "total_size": file_bytes.len(),
                                    "data": test_b64_encode(&file_bytes[offset..end])
                                })
                            }
                            "fs.txbegin" => serde_json::json!({"tx": 77}),
                            "fs.writebin" => {
                                staged.extend(test_b64_decode(params["data"].as_str().unwrap()));
                                serde_json::Value::Null
                            }
                            "fs.txcommit" => {
                                keymap = std::mem::take(&mut staged);
                                let parsed: serde_json::Value =
                                    serde_json::from_slice(&keymap).unwrap();
                                active = parsed["activeProfileId"].as_u64().unwrap();
                                serde_json::Value::Null
                            }
                            other => panic!("unexpected method {other}"),
                        };
                        let reply = serde_json::json!({"id": req["id"], "result": result});
                        let mut wire = reply.to_string();
                        wire.push('\n');
                        for out in outbound.encode(wire.as_bytes()).unwrap() {
                            let _ = inject.send(out);
                        }
                    }
                }
                keymap
            });

            let prepared = session.prepare_six_layer_write(dir.path()).await.unwrap();
            six_layer_runtime::merge_previous_original_into_backup(&v1_backup, &prepared.backup)
                .unwrap();
            let v2_candidate = prepared.candidate.clone();
            let receipt = session.apply_six_layer_write(prepared).await.unwrap();
            fs::remove_dir_all(&v1_backup).unwrap();

            assert_eq!(
                fs::read(receipt.backup.join("keymap.json")).unwrap(),
                SIX_SOURCE
            );
            assert_eq!(
                fs::read(receipt.backup.join("previous-planned-keymap.json")).unwrap(),
                v1_bytes
            );
            assert_eq!(
                fs::read(receipt.backup.join("smart_actions.json")).unwrap(),
                SMART
            );
            let manifest: serde_json::Value =
                serde_json::from_slice(&fs::read(receipt.backup.join("manifest.json")).unwrap())
                    .unwrap();
            assert_eq!(
                manifest["keymap_sha256"].as_str(),
                Some(mi_bridge::lighting_arbiter::sha256_hex(SIX_SOURCE).as_str())
            );
            assert_eq!(manifest["previous_profile_index"].as_u64(), Some(0));
            assert_eq!(manifest["previous_layer_index"].as_u64(), Some(1));
            assert_eq!(
                fs::read(receipt.backup.join("planned-keymap.json")).unwrap(),
                v2_candidate
            );
            assert_eq!(
                session
                    .verify_six_layer_files(&v2_candidate, Some(SMART), 1)
                    .await
                    .unwrap(),
                2
            );

            session
                .restore_six_layer_backup(&receipt.backup)
                .await
                .unwrap();
            session.shutdown().await;
            drop(session);

            let restored = worker.await.unwrap();
            assert_eq!(restored, SIX_SOURCE);
        });

        let proj_dir = tempfile::tempdir().unwrap();
        let mut config = WorkbenchStoreConfig::isolated(proj_dir.path());
        config.isolated = false;
        config.auto_connect_usb = false;
        let mut store = WorkbenchStore::open(config).unwrap();
        let dummy_session = mock_session(&store.tokio_runtime);
        let mut dummy_running =
            six_layer_runtime::RunningSixLayer::isolated(dummy_session.clone(), proj_dir.path());
        store.tokio_runtime.block_on(dummy_session.shutdown());
        dummy_running.installation.plan = store.compiled_six_layer_plan().unwrap();
        dummy_running.installation.status = InstallationStatus::RecoveryRequired;
        store.six_layer_state.installation = Some(dummy_running.installation.clone());
        drop(dummy_running);
        store.commit_runtime_update();
        assert!(!store.projection.sidebar.board_layers.can_apply);
        assert_eq!(
            store
                .projection
                .sidebar
                .board_layers
                .apply_block_reason
                .as_str(),
            "六层恢复记录尚未处理，请先恢复原设备配置"
        );
    }

    #[test]
    fn one_shot_unconfirmed_receipt_preserves_six_layer_while_unconfirmed_cleanup_fences_receiver()
    {
        let dir = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(dir.path()).unwrap();
        let session = mock_session(&store.tokio_runtime);
        let mut running = six_layer_runtime::RunningSixLayer::isolated(session, dir.path());
        running.readiness.configuration_observed = true;
        running.readiness.keyboard_grabbed = true;
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        running.receipts = Some(rx);
        store.six_layer_running = Some(running);

        tx.send(mi_macos::HostActionReceipt {
            action_type: "voice_start".into(),
            binding_id: "AG10".into(),
            profile_id: "ag".into(),
            target: "com.google.antigravity".into(),
            outcome: "unconfirmed".into(),
            detail: "live_voice_readback_timeout".into(),
            text_byte_len: None,
            timestamp_ms: 1,
        })
        .unwrap();

        store.poll_six_layer_runtime();
        assert!(store
            .six_layer_running
            .as_ref()
            .unwrap()
            .receiver
            .failure()
            .is_none());
        assert_eq!(
            store.status_message,
            "动作读回未确认 · live_voice_readback_timeout"
        );

        store.ptt_controller.cleanup_confirmed = false;
        store.poll_six_layer_runtime();
        assert!(store.six_layer_running.as_ref().unwrap().is_retired());
        assert_eq!(store.status_message, "宿主动作清理未确认，输入已暂停");
    }

    #[test]
    fn profile_lock_accessibility_caution_and_competing_host_notice_work_end_to_end() {
        let driver = mi_macos::RecordingOsDriver::new(true);
        let executor = mi_macos::LiveHostActionExecutor::new(driver.clone());
        assert!(executor.is_accessibility_trusted());
        driver.set_accessibility_trusted(false);
        assert!(!executor.is_accessibility_trusted());
        executor.open_accessibility_settings().unwrap();
        assert!(driver.events().iter().any(|ev| matches!(
            ev,
            mi_macos::RecordedOsEvent::OpenApplication(target)
                if target == "com.apple.systempreferences.accessibility"
        )));

        let dir = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(dir.path()).unwrap();
        store.set_host_execution_enabled(true).unwrap();
        store.set_accessibility_trusted_for_test(Some(false));
        assert!(!store.projection.header.accessibility_trusted);
        assert!(!store.projection.status_menu.accessibility_trusted);
        assert_eq!(
            store.projection.header.ready_state,
            crate::header::HeaderReadyState::Caution {
                text: "待授权辅助功能".to_string()
            }
        );
        assert_eq!(
            store.projection.header.banner,
            crate::OwnershipBannerState::Caution {
                reason: "缺少 macOS 辅助功能权限，按键动作已被系统拦截，请打开辅助功能设置授权"
                    .to_string()
            }
        );
        store
            .dispatch_status_menu_action(
                crate::status_menu::StatusMenuAction::OpenAccessibilitySettings,
            )
            .unwrap();
        assert_eq!(
            store
                .last_host_action_receipt
                .as_ref()
                .map(|r| r.action_type.as_str()),
            Some("open_accessibility_settings")
        );

        store.set_accessibility_trusted_for_test(Some(true));
        store.set_competing_input_app_running_for_test(true);
        assert_eq!(
            store
                .projection
                .status_menu
                .competing_host_notice
                .as_deref(),
            Some("检测到原版 Input 正在后台抢占层切换，建议开启「锁定当前方案」")
        );

        let ag_id = store
            .find_canonical_profile_id(crate::CanonicalAgent::Antigravity)
            .unwrap();
        let dsh_id = store
            .find_canonical_profile_id(crate::CanonicalAgent::DeepseekHarness)
            .unwrap();
        store.browse_profile(&ag_id).unwrap();
        store
            .dispatch_sidebar_action(crate::sidebar::SidebarAction::SetProfileLocked(true))
            .unwrap();
        assert!(store.projection.sidebar.is_profile_locked);
        assert_eq!(
            store.projection.sidebar.locked_profile_id.as_deref(),
            Some(ag_id.as_str())
        );
        assert_eq!(
            store.projection.status_menu.routing_mode_summary,
            "已锁定 · 忽略前台切换与外部干扰"
        );
        assert_eq!(
            store
                .projection
                .status_menu
                .competing_host_notice
                .as_deref(),
            Some("检测到原版 Input 正在后台运行，已由方案锁定隔离干扰")
        );

        assert!(!store.foreground_changed(Some("com.deepseek.dsh")));
        assert_eq!(
            store.resolved_active_profile_id().as_deref(),
            Some(ag_id.as_str())
        );

        store
            .dispatch_sidebar_action(crate::sidebar::SidebarAction::SelectProfile(dsh_id.clone()))
            .unwrap();
        assert_eq!(
            store.projection.sidebar.locked_profile_id.as_deref(),
            Some(dsh_id.as_str())
        );

        store
            .dispatch_status_menu_action(crate::status_menu::StatusMenuAction::ResumeAutoFollow)
            .unwrap();
        assert!(!store.projection.status_menu.is_profile_locked);
        assert!(store.projection.status_menu.locked_profile_id.is_none());
        assert!(store.projection.status_menu.auto_follow_enabled);
    }
}
