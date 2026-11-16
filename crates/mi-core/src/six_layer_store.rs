use super::*;
use crate::six_layer_runtime::{self, InstallationStatus};
use mi_model::apply::AppliedSixLayerPlan;

pub(super) fn runtime_action_profile_id<'a>(
    doc: &'a LocalDocumentV2,
    profile_id: &'a str,
) -> Option<&'a str> {
    let profile = doc.editable.graph.profiles.get(profile_id)?;
    let protected = |profile: &mi_model::document::ProfileNode| {
        profile
            .layer_order
            .iter()
            .any(|id| doc.is_protected_oai_layer(id))
    };
    if !protected(profile) {
        Some(profile_id)
    } else if mi_model::board_layers::native_owner(doc, profile_id)
        == Some(mi_model::board_layers::NativeLayerOwner::Codex)
    {
        doc.editable
            .graph
            .profiles
            .iter()
            .find(|(_, profile)| {
                profile.name == mi_model::preset::LOCAL_CODEX_PROFILE_NAME && !protected(profile)
            })
            .map(|(id, _)| id.as_str())
    } else {
        None
    }
}

impl WorkbenchStore {
    pub(super) fn runtime_action_profile_id<'a>(&'a self, profile_id: &'a str) -> Option<&'a str> {
        runtime_action_profile_id(&self.document_store.host_store().draft.document, profile_id)
    }

    pub(crate) fn observed_connected_firmware(&self) -> Option<String> {
        self.six_layer_running
            .as_ref()
            .and_then(|running| running.observed_firmware_version())
            .or_else(|| {
                self.device_session
                    .as_ref()
                    .filter(|session| !session.is_disconnected())
                    .and_then(|session| session.observed_firmware_version())
            })
    }

    pub(crate) fn validate_six_layer_installation_health(
        inst: &six_layer_runtime::Installation,
        firmware: Option<&str>,
    ) -> Result<(), String> {
        if inst.status != InstallationStatus::Applied {
            return Err("六层恢复记录尚未处理，请先恢复原设备配置".into());
        }
        inst.plan.validate_for_firmware(firmware)?;
        let planned_path = inst.backup.join("planned-keymap.json");
        if planned_path.is_file() {
            let planned = std::fs::read(&planned_path).map_err(|e| e.to_string())?;
            let smart_path = inst.backup.join("smart_actions.json");
            let smart = if smart_path.is_file() {
                Some(std::fs::read(&smart_path).map_err(|e| e.to_string())?)
            } else {
                None
            };
            mi_model::apply::verify_active_six_layer_profile_for_firmware(
                &planned,
                smart.as_deref(),
                inst.profile_index,
                firmware,
            )?;
        }
        Ok(())
    }

    fn compiled_six_layer_plan(&self) -> Result<AppliedSixLayerPlan, String> {
        let plan = self.projection_inputs().compiled_six_layer_plan()?;
        let fw = self.observed_connected_firmware();
        plan.validate_for_firmware(fw.as_deref())?;
        Ok(plan)
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
            .map(str::to_owned);
        let adapter_name = match canonical {
            Some(CanonicalAgent::Antigravity) => Some("antigravity"),
            Some(CanonicalAgent::Codex) => Some("codex"),
            Some(CanonicalAgent::DeepseekHarness) => Some("dsh-desktop"),
            Some(CanonicalAgent::Herdr) => Some("herdr"),
            _ => None,
        };
        let action_profile_id = self.runtime_action_profile_id(profile_id);
        let layout = self.current_bottom_row_layout();
        let command_execution = self.command_execution_enabled;
        let normalize_action = |mut action: ResolvedHostAction| -> Option<ResolvedHostAction> {
            if matches!(
                action,
                ResolvedHostAction::MacroSequence { .. }
                    | ResolvedHostAction::Shortcut { key_code: 0x3f, .. }
            ) {
                return None;
            }
            if let ResolvedHostAction::AgentCommand { adapter, command } = &mut action {
                if matches!(adapter.as_str(), "host" | "active-agent")
                    && !mi_model::is_host_mouse_command(command)
                {
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
        };
        let actions = std::array::from_fn(|slot| {
            let binding_id = mi_model::direct_controls::CONTROL_BINDINGS[slot];
            let source_id = action_profile_id?;
            let action = resolve_host_action_with_layout(
                doc,
                Some(source_id),
                binding_id,
                true,
                command_execution,
                layout,
            )?;
            normalize_action(action)
        });
        let encoder_hold_action = action_profile_id
            .and_then(|source_id| {
                mi_model::resolve_encoder_hold_action(doc, Some(source_id), command_execution)
            })
            .and_then(&normalize_action);
        Some(six_layer_runtime::PinnedProfilePlan {
            profile_id: profile_id.to_string(),
            bundle_id,
            actions,
            encoder_hold_action,
        })
    }

    pub(crate) fn build_six_layer_routing_mode(&self) -> six_layer_runtime::SixLayerRoutingMode {
        if let Some(locked_id) = self.document_store.locked_profile_id() {
            if let Some(plan) = self.compile_profile_twenty_slots(&locked_id) {
                return six_layer_runtime::SixLayerRoutingMode::Locked(plan);
            }
        }
        if self.six_layer_user_pinned {
            if let Some(active_id) = self.document_store.host_active_profile_id() {
                if let Some(plan) = self.compile_profile_twenty_slots(&active_id) {
                    return six_layer_runtime::SixLayerRoutingMode::ManualPinned(plan);
                }
            }
        }
        let doc = &self.document_store.host_store().draft.document;
        let layer_one = doc
            .editable
            .graph
            .profiles
            .iter()
            .find(|(_, profile)| profile.name == mi_model::preset::LOCAL_CODEX_PROFILE_NAME)
            .and_then(|(pid, _)| self.compile_profile_twenty_slots(pid))
            .filter(six_layer_runtime::PinnedProfilePlan::has_any_action);
        let resolve_board_host_layer = |board_layer: mi_model::board_layers::BoardLayer,
                                        host_idx: usize,
                                        canonical: CanonicalAgent|
         -> Option<six_layer_runtime::PinnedProfilePlan> {
            let pid = self
                .document_store
                .board_layer_plan()
                .and_then(|plan| match plan.route(board_layer) {
                    mi_model::board_layers::BoardLayerRoute::HostProfile { profile_id } => {
                        Some(profile_id.clone())
                    }
                    _ => None,
                })
                .or_else(|| {
                    self.six_layer_state
                        .installation
                        .as_ref()
                        .and_then(|inst| inst.plan.hosts.get(host_idx))
                        .map(|h| h.profile_id.clone())
                })
                .or_else(|| self.find_canonical_profile_id(canonical))?;
            self.compile_profile_twenty_slots(&pid)
        };
        let layer_two = resolve_board_host_layer(
            mi_model::board_layers::BoardLayer::ALL[1],
            0,
            CanonicalAgent::Antigravity,
        );
        let layer_three = resolve_board_host_layer(
            mi_model::board_layers::BoardLayer::ALL[2],
            1,
            CanonicalAgent::DeepseekHarness,
        );
        six_layer_runtime::SixLayerRoutingMode::BoardLayers {
            layer_one,
            layer_two,
            layer_three,
        }
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
        if self.firmware_update.reserved()
            || self.config.isolated
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
        let connected_fw = self.observed_connected_firmware();
        let Some(plan) = self
            .six_layer_state
            .installation
            .as_ref()
            .filter(|inst| {
                Self::validate_six_layer_installation_health(inst, connected_fw.as_deref()).is_ok()
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
        let preferred = self.document_store.preferred_device_serial();
        if mi_transport::discover_target_keyboard_preferred(preferred.as_deref()).is_err() {
            return;
        }
        self.six_layer_auto_resume_attempts += 1;
        self.six_layer_next_auto_resume_at = Some(now + Duration::from_secs(3));
        self.resume_requires_explicit_connect = false;
        let _ = self.start_six_layer_apply(Some(plan));
    }

    pub(super) fn suspend_for_six_layer_operation(&mut self) -> Result<(), WorkbenchError> {
        self.cancel_session_navigation();
        self.cancel_encoder_model_picker();
        self.teardown_v070_session_sa_rgb();
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
        self.connected_candidate = None;
        self.available_usb_candidates.clear();
        self.v070_hardware.on_disconnected();
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
        if self.firmware_update.reserved() {
            return Err(WorkbenchError::Routing(
                "固件更新期间不能恢复六层控制".into(),
            ));
        }
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
        let connected_fw = self.observed_connected_firmware();
        if let Some(installation) = self.six_layer_state.installation.as_ref() {
            if installation.status != InstallationStatus::Applied
                || installation.plan.validate_current().is_err()
            {
                return Err(WorkbenchError::Routing(
                    "六层恢复记录尚未处理，请先恢复原设备配置".into(),
                ));
            }
            Self::validate_six_layer_installation_health(installation, connected_fw.as_deref())
                .map_err(WorkbenchError::Routing)?;
        }
        plan.validate_for_firmware(connected_fw.as_deref())
            .map_err(WorkbenchError::Routing)?;
        self.suspend_for_six_layer_operation()?;
        let transport = self.exclusive_usb_transport();
        let dir = self.config.document_dir.clone();
        let saved = self.six_layer_state.clone();
        let runtime = Arc::clone(&self.tokio_runtime);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = if reconnecting {
                runtime.block_on(six_layer_runtime::resume(&dir, saved, transport))
            } else {
                runtime.block_on(six_layer_runtime::apply(&dir, plan, saved, transport))
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
        if self.firmware_update.reserved() {
            return Err(WorkbenchError::Routing(
                "固件更新期间不能写入六层配置".into(),
            ));
        }
        if self.six_layer_job.is_some()
            || self.six_layer_state.installation.is_none()
            || self.config.isolated
        {
            return Err(WorkbenchError::Routing(
                "当前没有可执行的六层恢复操作".into(),
            ));
        }
        self.suspend_for_six_layer_operation()?;
        let transport = self.exclusive_usb_transport();
        let dir = self.config.document_dir.clone();
        let saved = self.six_layer_state.clone();
        let runtime = Arc::clone(&self.tokio_runtime);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = runtime
                .block_on(six_layer_runtime::restore(&dir, saved, transport))
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
                    let firmware_health = running
                        .observed_firmware_version()
                        .map(|version| {
                            Self::validate_six_layer_installation_health(
                                &running.installation,
                                Some(&version),
                            )
                        })
                        .transpose();
                    if let Err(err) = firmware_health {
                        running.receiver.invalidate(&err);
                        let _ = running.shutdown(&self.tokio_runtime);
                        self.v070_hardware.on_disconnected();
                        self.sa_rgb_coordinator.clear_on_disconnect();
                        self.status_message = err;
                    } else {
                        self.six_layer_auto_resume_attempts = 0;
                        self.six_layer_next_auto_resume_at = None;
                        self.device_session = None;
                        self.connected_candidate = Some(running.candidate.clone());
                        if let Some(serial) = running
                            .candidate
                            .serial_number
                            .as_deref()
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                        {
                            let _ = self
                                .document_store
                                .set_preferred_device_serial(Some(serial.to_string()));
                        }
                        if self.available_usb_candidates.is_empty() {
                            self.available_usb_candidates = vec![running.candidate.clone()];
                        }
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
                        running.set_v070_inbox(self.hardware_event_tx.clone());
                        if let Some(version) = running.observed_firmware_version() {
                            if mi_model::supports_v070_hardware_capabilities(&version) {
                                let pl = running.observed_v070_profile_layer();
                                self.v070_hardware.mark_supported(
                                    &version,
                                    pl.map(|(p, _)| p),
                                    pl.map(|(_, l)| l),
                                    None,
                                    None,
                                );
                                let planned_path =
                                    running.installation.backup.join("planned-keymap.json");
                                if self.v070_hardware.projection.active_joystick_mode.is_none()
                                    && planned_path.is_file()
                                {
                                    if let Ok(bytes) = std::fs::read(&planned_path) {
                                        self.v070_hardware.projection.active_joystick_mode =
                                            mi_model::apply::extract_keymap_joystick_hardware_mode(
                                                &bytes,
                                                pl.map(|(p, _)| p),
                                                pl.map(|(_, l)| l),
                                            );
                                    }
                                }
                            }
                        }
                        running.start_vendor_bridge(&self.tokio_runtime);
                        self.six_layer_running = Some(running);
                        self.six_layer_user_pinned = false;
                        self.sync_six_layer_routing_context();
                        self.status_message = "六层配置已读回，等待首次实体输入验证".into();
                        self.lighting_arbiter.invalidate_hardware_cache();
                        self.refresh_four_app_lighting();
                        self.sync_v070_session_sa_rgb();
                        self.start_mac_usb_discovery_watcher();
                    }
                }
                Ok(None) => {
                    self.status_message = "原设备配置及 Karabiner 设置已恢复".into();
                    self.board_routing = BoardRouting::Legacy;
                    self.v070_hardware.on_disconnected();
                    self.sa_rgb_coordinator.clear_on_disconnect();
                }
                Err(error) => self.status_message = error,
            }
            self.commit_runtime_update();
        }
        if self.six_layer_running.is_none() {
            self.try_auto_resume_six_layer(self.update_now);
            return;
        }
        let (mut lighting_changed, need_lighting_resync, v070_pl, v070_ver) = {
            let running = self.six_layer_running.as_ref().unwrap();
            let confirmed = running.confirmed_lighting_digest();
            let changed = self
                .lighting_arbiter
                .reconcile_six_layer_confirmed(confirmed.as_deref());
            (
                changed,
                running.vendor_bridge_needs_lighting_sync(),
                running.observed_v070_profile_layer(),
                running.observed_firmware_version(),
            )
        };
        let mut v070_changed = false;
        if let Some(version) = v070_ver {
            if let Some(running) = self.six_layer_running.as_ref() {
                if let Err(err) = Self::validate_six_layer_installation_health(
                    &running.installation,
                    Some(&version),
                ) {
                    running.receiver.invalidate(&err);
                }
            }
            if mi_model::supports_v070_hardware_capabilities(&version)
                && !self.v070_hardware.projection.supported
            {
                self.v070_hardware.mark_supported(
                    &version,
                    v070_pl.map(|(p, _)| p),
                    v070_pl.map(|(_, l)| l),
                    None,
                    None,
                );
                v070_changed = true;
            }
        }
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
        let observed_changed =
            self.projection.sidebar.board_layers.observed_layer != Some(observed);
        let first = running.first_event_verified.load(Ordering::Acquire);
        let failure = running.receiver.failure();
        let needs_cleanup = failure.is_some() && !running.is_retired();
        let changed = lighting_changed
            || v070_changed
            || observed_changed
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
            self.sa_rgb_coordinator.clear_on_disconnect();
            let stopped = self
                .six_layer_running
                .as_mut()
                .unwrap()
                .quiesce(&self.tokio_runtime)
                .and(self.live_executor.stop_owned_actions());
            if let Err(error) = self.finish_host_cleanup(stopped) {
                self.status_message = format!("宿主动作清理未确认 · {error}");
            }
        } else if observed_changed || v070_changed {
            self.sync_v070_session_sa_rgb();
        }
        if changed {
            self.commit_runtime_update();
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
    fn custom_codex_is_available_to_six_layer_board_layers() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(directory.path()).unwrap();
        let selected_id = store
            .document_store
            .host_active_profile_id()
            .expect("default active profile");
        let six_layer_runtime::SixLayerRoutingMode::BoardLayers {
            layer_one,
            layer_two,
            layer_three,
        } = store.build_six_layer_routing_mode()
        else {
            panic!("expected fresh store to start in board-layers mode");
        };
        let custom = layer_one.expect("local Codex populates layer_one");
        assert_eq!(custom.bundle_id.as_deref(), Some("com.openai.codex"));
        assert!(custom.actions.iter().any(Option::is_some));
        assert_eq!(
            custom.encoder_hold_action,
            Some(ResolvedHostAction::AgentCommand {
                adapter: "codex".into(),
                command: "search".into(),
            })
        );
        let live_l2 = layer_two.expect("Antigravity populates live layer_two");
        assert_eq!(
            live_l2.encoder_hold_action,
            Some(ResolvedHostAction::AgentCommand {
                adapter: "antigravity".into(),
                command: "search".into(),
            })
        );
        let live_l3 = layer_three.expect("DSH populates live layer_three");
        assert_eq!(
            live_l3.encoder_hold_action,
            Some(ResolvedHostAction::AgentCommand {
                adapter: "dsh-desktop".into(),
                command: "search".into(),
            })
        );

        store.select_and_activate_profile(&selected_id).unwrap();
        let six_layer_runtime::SixLayerRoutingMode::ManualPinned(manual) =
            store.build_six_layer_routing_mode()
        else {
            panic!("expected manual pinned after explicit profile selection");
        };
        assert_eq!(manual.profile_id, selected_id);
        assert_eq!(
            manual.encoder_hold_action,
            Some(ResolvedHostAction::AgentCommand {
                adapter: "antigravity".into(),
                command: "search".into(),
            })
        );

        store
            .dispatch_sidebar_action(crate::sidebar::SidebarAction::SetProfileLocked(true))
            .unwrap();
        let six_layer_runtime::SixLayerRoutingMode::Locked(locked) =
            store.build_six_layer_routing_mode()
        else {
            panic!("expected locked mode");
        };
        assert_eq!(locked.profile_id, selected_id);

        store
            .dispatch_sidebar_action(crate::sidebar::SidebarAction::SetProfileLocked(false))
            .unwrap();
        assert!(matches!(
            store.build_six_layer_routing_mode(),
            six_layer_runtime::SixLayerRoutingMode::BoardLayers {
                layer_one: Some(_),
                layer_two: Some(_),
                layer_three: Some(_),
            }
        ));

        // Unassigning EncoderHold on Antigravity updates live BoardLayers layer_two immediately
        store
            .dispatch_canvas_action(crate::canvas::CanvasAction::SelectSurface(
                crate::canvas::ControlSurfaceId::EncoderHold,
            ))
            .unwrap();
        store
            .dispatch_inspector_action(crate::inspector::InspectorAction::UnassignSelectedSlot)
            .unwrap();
        if let six_layer_runtime::SixLayerRoutingMode::BoardLayers {
            layer_two: Some(updated_l2),
            ..
        } = store.build_six_layer_routing_mode()
        {
            assert_eq!(updated_l2.encoder_hold_action, None);
        } else {
            panic!("expected live layer_two after unassigning EncoderHold");
        }
    }

    #[test]
    fn resuming_promotes_descriptor_discovery_and_reuses_the_exclusive_owner() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(directory.path()).unwrap();
        let descriptor = Arc::new(mi_transport::MacTransport::new());
        store.mac_watcher_transport = Some(descriptor.clone());
        store.resume_requires_explicit_connect = true;
        let exclusive = store.exclusive_usb_transport();
        assert!(exclusive.is_exclusive_vendor());
        assert!(!Arc::ptr_eq(&descriptor, &exclusive));
        assert!(!store.resume_requires_explicit_connect);
        let repeated = store.exclusive_usb_transport();
        assert!(Arc::ptr_eq(&exclusive, &repeated));
    }

    #[test]
    fn installed_six_layer_keeps_exclusive_discovery_until_sleep() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(directory.path()).unwrap();
        let session = mock_session(&store.tokio_runtime);
        let running =
            six_layer_runtime::RunningSixLayer::isolated(session.clone(), directory.path());
        store.six_layer_state.installation = Some(running.installation.clone());
        let (link, _writes) = mi_transport::MockLink::new();
        let transport = mi_transport::MockTransport::new(link.clone());
        assert!(store.connect_with_transport(&transport, true).is_err());
        assert!(link.writes.lock().unwrap().is_empty());
        assert!(!store.discovery_is_descriptor_only());
        store.resume_requires_explicit_connect = true;
        assert!(store.discovery_is_descriptor_only());
        drop(running);
        store.tokio_runtime.block_on(session.shutdown());
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
    fn six_layer_board_layer_keeps_physical_layer_across_foreground_changes() {
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
        let dsh = running.installation.plan.hosts[1].profile_id.clone();
        running.observed_layer.store(3, Ordering::Release);
        store.six_layer_running = Some(running);
        store.sync_six_layer_routing_context();
        let saved_profile = store.document_store.host_active_profile_id();
        assert!(store.foreground_changed(Some("com.google.antigravity")));
        assert_eq!(store.resolved_active_profile_id(), Some(dsh.clone()));
        assert_eq!(
            store.projection.sidebar.board_layers.observed_layer,
            Some(3)
        );
        assert!(!store
            .projection
            .sidebar
            .board_layers
            .observed_status
            .contains("前台优先"));
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
            Arc::new(mi_transport::MacTransport::exclusive_vendor()),
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
                    exclusive: true,
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
    fn browsing_native_codex_keeps_editor_locked_without_suspending_active_antigravity_lighting() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(directory.path()).unwrap();
        let (_link, session, rpc_rx) = spawn_thstatus_mock_session(&store.tokio_runtime);
        let mut running =
            six_layer_runtime::RunningSixLayer::isolated(session.clone(), directory.path());
        running.installation.plan = store.compiled_six_layer_plan().unwrap();
        let active = store
            .find_canonical_profile_id(CanonicalAgent::Antigravity)
            .unwrap();
        running.set_routing_context(
            six_layer_runtime::SixLayerRoutingMode::Locked(
                store.compile_profile_twenty_slots(&active).unwrap(),
            ),
            Some("com.google.antigravity"),
        );
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
            .dispatch_sidebar_action(SidebarAction::BrowseProfile(codex_pid))
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
            .dispatch_status_menu_action(crate::status_menu::StatusMenuAction::SetProfileLocked(
                false,
            ))
            .unwrap();
        assert!(!store.projection.status_menu.is_profile_locked);
        assert!(store.projection.status_menu.locked_profile_id.is_none());
    }

    #[test]
    fn compile_profile_twenty_slots_excludes_joystick_slots_when_mode_is_mouse_scroll_or_source() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(dir.path()).unwrap();
        let mut custom_pid = String::new();
        store
            .document_store
            .update(|draft| {
                custom_pid = draft.create_profile("Joystick Routing Test")?;
                Ok(())
            })
            .unwrap();
        store.commit_runtime_update();
        store.browse_profile(&custom_pid).unwrap();
        let lid = store.selected_layer_id().unwrap().to_string();

        let catalog = mi_model::draft::actions();
        let search_action = catalog.iter().find(|a| a.id == "search").unwrap().clone();
        store
            .document_store
            .update(|draft| {
                draft.set_control_settings(
                    &custom_pid,
                    &lid,
                    mi_model::control_settings::ControlSettings {
                        joystick_mode: "logical_sectors".into(),
                        active_sectors: 4,
                        encoder_hold_action_id: None,
                    },
                )?;
                draft.assign(&json!(custom_pid), &json!(lid), 0, search_action.clone())?;
                for slot in 16..=19 {
                    draft.assign(&json!(custom_pid), &json!(lid), slot, search_action.clone())?;
                }
                Ok(())
            })
            .unwrap();

        let logical_plan = store.compile_profile_twenty_slots(&custom_pid).unwrap();
        assert!(logical_plan.actions[0].is_some());
        assert!(logical_plan.actions[16..20].iter().all(|a| a.is_some()));

        for mode in [
            crate::JoystickControlMode::Mouse,
            crate::JoystickControlMode::Scroll,
            crate::JoystickControlMode::Source,
        ] {
            store
                .dispatch_inspector_action(crate::InspectorAction::SetJoystickMode(mode))
                .unwrap();
            let plan = store.compile_profile_twenty_slots(&custom_pid).unwrap();
            assert!(
                plan.actions[0].is_some(),
                "key slot 0 stays active in {mode:?}"
            );
            assert!(
                plan.actions[16..20].iter().all(|a| a.is_none()),
                "joystick slots 16..=19 must be excluded in {mode:?}"
            );
        }

        store
            .dispatch_inspector_action(crate::InspectorAction::SetJoystickMode(
                crate::JoystickControlMode::LogicalSectors,
            ))
            .unwrap();
        let restored = store.compile_profile_twenty_slots(&custom_pid).unwrap();
        assert!(restored.actions[16..20].iter().all(|a| a.is_some()));
    }

    #[test]
    fn six_layer_store_health_startup_and_resume_accept_v070_mouse_and_scroll_and_reject_on_v063() {
        const SIX_SOURCE: &[u8] =
            include_bytes!("../../mi-model/fixtures/device/synthetic-six-layer-source.json");
        const SMART: &[u8] =
            include_bytes!("../../mi-model/fixtures/device/synthetic-smart-actions-object.json");

        let dir = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(dir.path()).unwrap();

        let ag_id = store
            .find_canonical_profile_id(crate::CanonicalAgent::Antigravity)
            .unwrap();
        store.browse_profile(&ag_id).unwrap();
        store
            .dispatch_inspector_action(crate::InspectorAction::SetJoystickMode(
                crate::JoystickControlMode::Mouse,
            ))
            .unwrap();

        let plan = store.compiled_six_layer_plan().unwrap();
        assert!(plan
            .hosts
            .iter()
            .any(|h| h.joystick_mode.as_deref() == Some("MOUSE")));
        assert!(plan.validate_current().is_ok());
        assert!(plan.validate_for_firmware(None).is_ok());
        assert!(plan.validate_for_firmware(Some("0.7.0-rc.16")).is_ok());
        assert!(plan.validate_for_firmware(Some("0.7.0")).is_ok());
        assert!(plan.validate_for_firmware(Some("0.6.3")).is_err());

        for (hw_mode, cli_mode) in [("MOUSE", "mouse"), ("SCROLL", "scroll")] {
            let compiled = mi_model::apply::compile_six_layer_profile_for_firmware(
                SIX_SOURCE,
                Some(SMART),
                Some("0.7.0-rc.16"),
                Some(hw_mode),
            )
            .unwrap();

            let backup = dir.path().join(format!("backup-{cli_mode}"));
            fs::create_dir_all(&backup).unwrap();
            fs::write(backup.join("keymap.json"), SIX_SOURCE).unwrap();
            fs::write(backup.join("planned-keymap.json"), &compiled.keymap).unwrap();
            fs::write(backup.join("smart_actions.json"), SMART).unwrap();

            let base_session = mock_session(&store.tokio_runtime);
            let mut template_running =
                six_layer_runtime::RunningSixLayer::isolated(base_session.clone(), dir.path());
            template_running.installation.profile_index = compiled.profile_index;
            template_running.installation.backup = backup.clone();
            template_running.installation.plan = plan.clone();
            template_running.installation.plan.codex_joystick_mode = Some(hw_mode.to_string());
            for host in &mut template_running.installation.plan.hosts {
                host.joystick_mode = Some(hw_mode.to_string());
            }
            let inst = template_running.installation.clone();
            drop(template_running);
            store.tokio_runtime.block_on(base_session.shutdown());

            assert!(
                WorkbenchStore::validate_six_layer_installation_health(&inst, None).is_ok(),
                "{hw_mode} installation must pass pre-connect startup health check"
            );
            assert!(WorkbenchStore::validate_six_layer_installation_health(
                &inst,
                Some("0.7.0-rc.16")
            )
            .is_ok());
            let err_v063 =
                WorkbenchStore::validate_six_layer_installation_health(&inst, Some("0.6.3"))
                    .unwrap_err();
            assert!(err_v063.contains("0.7.0-rc.16"));

            let mut tampered_inst = inst.clone();
            let tampered_backup = dir.path().join(format!("backup-tampered-{cli_mode}"));
            fs::create_dir_all(&tampered_backup).unwrap();
            let tampered_keymap = String::from_utf8(compiled.keymap.clone())
                .unwrap()
                .replacen("KV_OAI_AG06", "KC_A", 1)
                .into_bytes();
            fs::write(tampered_backup.join("planned-keymap.json"), tampered_keymap).unwrap();
            fs::write(tampered_backup.join("smart_actions.json"), SMART).unwrap();
            tampered_inst.backup = tampered_backup;
            assert!(
                WorkbenchStore::validate_six_layer_installation_health(
                    &tampered_inst,
                    Some("0.7.0-rc.16")
                )
                .is_err(),
                "manual key edits must still be strictly rejected on 0.7.0-rc.16"
            );

            store.tokio_runtime.block_on(async {
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
                let expected_km = compiled.keymap.clone();
                let worker = tokio::spawn(async move {
                    let mut incoming = mi_proto::NativeFramer::default();
                    let outbound = mi_proto::NativeFramer::default();
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
                                    "version": "0.7.0-rc.16",
                                    "profile_index": 1,
                                    "layer_index": 2
                                }),
                                "fs.list" => serde_json::json!([
                                    meta("keymap.json", &expected_km),
                                    meta("smart_actions.json", SMART)
                                ]),
                                "fs.readbin" => {
                                    let file_bytes = match params["file"].as_str().unwrap() {
                                        "keymap.json" => expected_km.as_slice(),
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
                });

                let resume_action = six_layer_runtime::inspect_resume_snapshot(
                    &session,
                    &compiled.keymap,
                    Some(SMART),
                    compiled.profile_index,
                )
                .await
                .unwrap();
                assert_eq!(
                    resume_action,
                    six_layer_runtime::ResumeSnapshotAction::VerifyReady { layer: 2 }
                );
                session.shutdown().await;
                worker.abort();
            });
        }

        let compiled_mouse = mi_model::apply::compile_six_layer_profile_for_firmware(
            SIX_SOURCE,
            Some(SMART),
            Some("0.7.0-rc.16"),
            Some("MOUSE"),
        )
        .unwrap();
        let backup_poll = dir.path().join("backup-poll-mouse");
        fs::create_dir_all(&backup_poll).unwrap();
        fs::write(backup_poll.join("keymap.json"), SIX_SOURCE).unwrap();
        fs::write(
            backup_poll.join("planned-keymap.json"),
            &compiled_mouse.keymap,
        )
        .unwrap();
        fs::write(backup_poll.join("smart_actions.json"), SMART).unwrap();

        let make_mock_firmware_session = |rt: &tokio::runtime::Runtime, version: &str| {
            rt.block_on(async {
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
                        timeout: Duration::from_secs(1),
                        interval: Duration::ZERO,
                    },
                );
                let v = version.to_string();
                let worker = tokio::spawn(async move {
                    let mut incoming = mi_proto::NativeFramer::default();
                    let outbound = mi_proto::NativeFramer::default();
                    while let Some(frame) = requests.recv().await {
                        for bytes in incoming.feed(&frame).unwrap() {
                            let req: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                            let result = match req["method"].as_str().unwrap() {
                                "device.status" => serde_json::json!({
                                    "version": v,
                                    "profile_index": 1,
                                    "layer_index": 2
                                }),
                                _ => serde_json::json!({}),
                            };
                            let reply = serde_json::json!({"id": req["id"], "result": result});
                            let mut wire = reply.to_string();
                            wire.push('\n');
                            for out in outbound.encode(wire.as_bytes()).unwrap() {
                                let _ = inject.send(out);
                            }
                        }
                    }
                });
                session.read_identity_info().await.unwrap();
                worker.abort();
                session
            })
        };

        let s_v063 = make_mock_firmware_session(&store.tokio_runtime, "0.6.3");
        let mut r_v063 = six_layer_runtime::RunningSixLayer::isolated(s_v063.clone(), dir.path());
        r_v063.installation.profile_index = 1;
        r_v063.installation.backup = backup_poll.clone();
        r_v063.installation.plan = plan.clone();
        store.tokio_runtime.block_on(s_v063.shutdown());
        let (tx_bad, rx_bad) = std::sync::mpsc::channel();
        tx_bad.send(Ok(Some(r_v063))).unwrap();
        store.six_layer_job = Some(rx_bad);
        store.poll_six_layer_runtime();
        assert!(
            store.six_layer_running.is_none(),
            "poll_six_layer_runtime must reject MOUSE installation when connected firmware is 0.6.3"
        );
        assert!(store.status_message.contains("0.7.0-rc.16"));

        let s_v070 = make_mock_firmware_session(&store.tokio_runtime, "0.7.0-rc.16");
        let mut r_v070 = six_layer_runtime::RunningSixLayer::isolated(s_v070.clone(), dir.path());
        r_v070.installation.profile_index = 1;
        r_v070.installation.backup = backup_poll;
        r_v070.installation.plan = plan;
        store.tokio_runtime.block_on(s_v070.shutdown());
        let (tx_ok, rx_ok) = std::sync::mpsc::channel();
        tx_ok.send(Ok(Some(r_v070))).unwrap();
        store.six_layer_job = Some(rx_ok);
        store.poll_six_layer_runtime();
        assert!(store.six_layer_running.is_some());
        assert_eq!(
            store
                .projection()
                .v070_hardware
                .active_joystick_mode
                .as_deref(),
            Some("mouse")
        );
        if let Some(running) = store.six_layer_running.take() {
            running.shutdown(&store.tokio_runtime).unwrap();
        }
    }
}
