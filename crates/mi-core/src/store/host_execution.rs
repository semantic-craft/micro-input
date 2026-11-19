use super::inspector_editor_actions::fallback_unbridged_oai_action;
use super::*;

impl WorkbenchStore {
    pub(super) fn route_board_input(
        &mut self,
        event: InputEvent,
        now: Instant,
    ) -> Option<HostActionReceipt> {
        if event.kind == InputEventKind::InputStreamGap {
            self.input_lost_session = Some(event.session_id);
            self.release_board_inputs(now);
            self.status_message = "输入流有缺口，已停止原目标动作，请重新连接".into();
            self.commit_runtime_update();
            return None;
        }
        if event.kind == InputEventKind::Released {
            return self.consume_physical_input_event(event, now);
        }
        if self.input_lost_session == Some(event.session_id)
            || self.hardware_event_tx.source_closed(event.session_id)
        {
            return None;
        }
        if self.six_layer_job.is_some() || self.six_layer_running.is_some() {
            return None;
        }
        if self.six_layer_state.installation.is_some() {
            if self.document_store.locked_profile_id().is_some() && event.action_eligible {
                return self.consume_physical_input_event(event, now);
            }
            return None;
        }
        if self.board_routing == BoardRouting::WaitingForFirstLayer {
            if event.ingress_sequence <= self.board_admit_after
                || !event.action_eligible
                || event.board_layer != Some(1)
                || event.kind != InputEventKind::Pressed
            {
                return None;
            }
            self.enter_board_first_layer();
        }
        match self.board_routing {
            BoardRouting::Legacy if event.board_layer.is_none() => {
                return self.consume_physical_input_event(event, now);
            }
            BoardRouting::Managed { layer } => {
                if event.ingress_sequence <= self.board_admit_after || !event.action_eligible {
                    return None;
                }
                let next = event.board_layer.filter(|value| (1..=3).contains(value))?;
                if event.kind == InputEventKind::Pressed {
                    if layer != next {
                        self.release_board_inputs(now);
                    }
                    self.board_routing = BoardRouting::Managed { layer: next };
                    let Some(target) = self
                        .legacy_host_profile_id(next)
                        .filter(|id| !self.is_protected_oai_profile(id))
                    else {
                        self.set_status_message("旧三层绑定缺失或属于原生应用，按键已暂停");
                        return None;
                    };
                    if layer != next
                        || self.resolved_active_profile_id().as_deref() != Some(target.as_str())
                    {
                        self.status_message = format!("板载三层已就绪 · 第 {next} 层");
                        if let Err(error) =
                            self.activate_legacy_board_layer(next, now, layer != next)
                        {
                            self.set_status_message(error.to_string());
                            return None;
                        }
                    }
                } else {
                    return None;
                }
                return self.consume_physical_input_event(event, now);
            }
            _ => {}
        }
        None
    }

    pub(super) fn release_board_inputs(&mut self, now: Instant) {
        self.update_now = now;
        self.cancel_physical_session_navigation();
        self.cancel_encoder_model_picker();
        self.dial_pressed = false;
        self.dial_turned_while_pressed = false;
        self.radial_state.cancel();
        let stopped = self.live_executor.stop_owned_actions();
        let _ = self.finish_host_cleanup(stopped);
    }

    pub(crate) fn can_retry_host_cleanup(&self) -> bool {
        !self.ptt_controller.cleanup_confirmed
            || self.live_executor.has_unconfirmed_apple_fn_release()
            || matches!(self.system_lifecycle, SystemLifecycle::CleanupFailed(_))
    }

    pub(super) fn finalize_recovered_host_cleanup(&mut self) {
        if self.can_retry_host_cleanup() {
            return;
        }
        if self
            .six_layer_running
            .as_ref()
            .is_some_and(|running| running.is_retired())
        {
            self.six_layer_running = None;
            self.six_layer_view = None;
        }
        self.six_layer_auto_resume_attempts = 0;
        self.six_layer_next_auto_resume_at = None;
        self.try_auto_resume_six_layer(self.update_now);
    }

    pub fn retry_host_cleanup(&mut self) {
        let had_held_dictation = self.held_dictation.is_some();
        let stopped = self.live_executor.stop_owned_actions();
        let ok = self.finish_host_cleanup(stopped).is_ok();
        if ok && !had_held_dictation {
            self.status_message = "已完成宿主动作清理".to_string();
        }
        self.finalize_recovered_host_cleanup();
        self.commit_runtime_update();
    }

    pub(super) fn update_host_caution_from_receipt(
        &mut self,
        receipt: &HostActionReceipt,
        now: Instant,
    ) {
        if receipt.succeeded() {
            if self
                .host_caution
                .as_ref()
                .is_some_and(|c| !c.is_sticky_cleanup)
            {
                self.host_caution = None;
            }
        } else if receipt.outcome != "simulated" {
            let sticky_active = self
                .host_caution
                .as_ref()
                .is_some_and(|c| c.is_sticky_cleanup);
            if !sticky_active {
                self.host_caution = Some(HostActionCaution {
                    detail_code: receipt.detail.clone(),
                    is_sticky_cleanup: false,
                    expires_at: Some(now + HOST_CAUTION_TTL),
                });
            }
        }
    }

    pub(super) fn apply_host_action_receipt(
        &mut self,
        receipt: HostActionReceipt,
        binding_prefix: Option<&str>,
        now: Instant,
    ) {
        let localized_detail = crate::i18n::localize_managed_error_detail(
            &receipt.detail,
            mi_model::ResolvedLocale::ZhHans,
        );
        self.status_message = if let Some(binding_id) = binding_prefix {
            if receipt.succeeded() {
                format!("已触发 {binding_id} · {localized_detail}")
            } else {
                format!("未执行 {binding_id} · {localized_detail}")
            }
        } else {
            format!(
                "{} · {}",
                match receipt.outcome.as_str() {
                    "unconfirmed" => "动作读回未确认",
                    _ if receipt.succeeded() => "已执行",
                    _ => "未执行",
                },
                localized_detail
            )
        };
        self.update_host_caution_from_receipt(&receipt, now);
        self.record_host_action_receipt(receipt);
    }

    pub(super) fn finish_host_cleanup(
        &mut self,
        stopped: Result<(), String>,
    ) -> Result<(), String> {
        if self.held_dictation.is_some() {
            self.finish_dictation(stopped.clone());
        } else {
            self.ptt_controller.cancel();
            self.ptt_controller.cleanup_confirmed = stopped.is_ok();
        }
        match &stopped {
            Ok(()) => {
                if self.lifecycle_cleanup.is_none()
                    && !self.live_executor.has_unconfirmed_apple_fn_release()
                    && matches!(self.system_lifecycle, SystemLifecycle::CleanupFailed(_))
                {
                    self.lifecycle_epoch.fetch_add(1, Ordering::AcqRel);
                    self.system_lifecycle = SystemLifecycle::Awake;
                    self.v070_hardware.set_host_sleep_status("awake");
                }
                if !self.can_retry_host_cleanup()
                    && self
                        .host_caution
                        .as_ref()
                        .is_some_and(|c| c.is_sticky_cleanup)
                {
                    self.host_caution = None;
                }
            }
            Err(error) => {
                self.host_caution = Some(HostActionCaution {
                    detail_code: error.clone(),
                    is_sticky_cleanup: true,
                    expires_at: None,
                });
                let localized = crate::i18n::localize_managed_error_detail(
                    error,
                    mi_model::ResolvedLocale::ZhHans,
                );
                self.status_message = format!("原目标释放未确认 · {localized}");
            }
        }
        stopped
    }

    pub(super) fn finish_dictation(
        &mut self,
        stopped: Result<(), String>,
    ) -> Option<HostActionReceipt> {
        let held = self.held_dictation.as_ref()?.clone();
        self.ptt_controller.cancel();
        self.ptt_controller.cleanup_confirmed = stopped.is_ok();
        let (outcome, detail, status_message) = match stopped {
            Ok(()) => {
                self.held_dictation = None;
                if !self.can_retry_host_cleanup()
                    && self
                        .host_caution
                        .as_ref()
                        .is_some_and(|c| c.is_sticky_cleanup)
                {
                    self.host_caution = None;
                }
                let msg = format!(
                    "已结束原目标听写 ({}, {})",
                    held.input.binding_id, held.adapter
                );
                ("executed", msg.clone(), msg)
            }
            Err(error) => {
                self.host_caution = Some(HostActionCaution {
                    detail_code: error.clone(),
                    is_sticky_cleanup: true,
                    expires_at: None,
                });
                let localized = crate::i18n::localize_managed_error_detail(
                    &error,
                    mi_model::ResolvedLocale::ZhHans,
                );
                (
                    "unconfirmed",
                    format!("原目标听写停止未确认 · {error}"),
                    format!("原目标听写停止未确认 · {localized}"),
                )
            }
        };
        let receipt = HostActionReceipt {
            action_type: "agent_command".into(),
            binding_id: held.input.binding_id,
            profile_id: held.profile_id,
            target: held.adapter,
            outcome: outcome.into(),
            detail,
            text_byte_len: None,
            timestamp_ms: 0,
        };
        self.status_message = status_message;
        self.record_host_action_receipt(receipt.clone());
        self.commit_runtime_update();
        Some(receipt)
    }

    pub(super) fn record_host_action_receipt(&mut self, receipt: HostActionReceipt) {
        if let Some(audit_path) = env::var_os("MI_HOST_AUDIT_LOG") {
            let path = PathBuf::from(audit_path);
            let _ = receipt.append_to_audit_log(&path);
        }
        self.last_host_action_receipt = Some(receipt.clone());
        self.dispatched_receipts.push(receipt);
    }

    fn surface_for_binding(&self, binding_id: &str) -> Option<ControlSurfaceId> {
        let layout = self.current_bottom_row_layout();
        match binding_id {
            "AG00" => Some(ControlSurfaceId::Key0),
            "AG01" => Some(ControlSurfaceId::Key1),
            "AG02" => Some(ControlSurfaceId::Key2),
            "AG03" => Some(ControlSurfaceId::Key3),
            "AG04" => Some(ControlSurfaceId::Key4),
            "AG05" => Some(ControlSurfaceId::Key5),
            "AG06" | "ACT06" => Some(ControlSurfaceId::Key6),
            "AG07" | "ACT07" => Some(ControlSurfaceId::Key7),
            "AG08" | "ACT08" => Some(ControlSurfaceId::Key8),
            "AG09" | "ACT09" => Some(ControlSurfaceId::Key9),
            "AG10" | "ACT10" => Some(ControlSurfaceId::Key10),
            "AG11" | "ACT11" => Some(if layout.is_merged() {
                ControlSurfaceId::Key10
            } else {
                ControlSurfaceId::Key11
            }),
            "AG12" | "ACT12" => Some(ControlSurfaceId::Key12),
            "AG13" | "ENC_CC" => Some(ControlSurfaceId::EncoderCcw),
            "AG14" | "ENC_CW" => Some(ControlSurfaceId::EncoderCw),
            "ENC_CLK" => Some(ControlSurfaceId::EncoderPress),
            "AG15" | "AG16" | "AG17" | "AG18" => Some(ControlSurfaceId::Joystick),
            _ => None,
        }
    }

    fn resolve_physical_action(
        &self,
        profile_id: &str,
        binding_id: &str,
        layout: BottomRowLayout,
    ) -> Option<ResolvedHostAction> {
        let source_id = self.runtime_action_profile_id(profile_id)?;
        let doc = &self.document_store.host_store().draft.document;
        if let Some(resolved) = resolve_host_action_with_layout(
            doc,
            Some(source_id),
            binding_id,
            true,
            self.command_execution_enabled,
            layout,
        ) {
            return Some(resolved);
        }
        if source_id != profile_id {
            return None;
        }
        let profile = doc.editable.graph.profiles.get(source_id)?;
        fallback_unbridged_oai_action(&profile.name, binding_id, layout)
    }

    fn resolve_encoder_hold_physical_action(&self, profile_id: &str) -> Option<ResolvedHostAction> {
        let source_id = self.runtime_action_profile_id(profile_id)?;
        let doc = &self.document_store.host_store().draft.document;
        if let Some(resolved) = mi_model::resolve_encoder_hold_action(
            doc,
            Some(source_id),
            self.command_execution_enabled,
        ) {
            return Some(resolved);
        }
        if source_id != profile_id {
            return None;
        }
        let profile = doc.editable.graph.profiles.get(source_id)?;
        if profile.name.eq_ignore_ascii_case("Codex")
            && profile
                .layer_order
                .iter()
                .any(|lid| doc.is_protected_oai_layer(lid))
        {
            return Some(ResolvedHostAction::AgentCommand {
                adapter: "codex".to_string(),
                command: "search".to_string(),
            });
        }
        None
    }

    fn resolve_model_picker_target_adapter(
        &self,
        profile_id: &str,
        hold_action: Option<&ResolvedHostAction>,
    ) -> Option<String> {
        if let Some(ResolvedHostAction::AgentCommand { adapter, .. }) = hold_action {
            if matches!(adapter.as_str(), "antigravity" | "codex" | "dsh-desktop") {
                return Some(adapter.clone());
            }
        }
        let doc = &self.document_store.host_store().draft.document;
        if let Some(source_id) = self.runtime_action_profile_id(profile_id) {
            if let Some(profile) = doc.editable.graph.profiles.get(source_id) {
                if let Some(agent) = CanonicalAgent::from_profile_name(&profile.name) {
                    let mapped = match agent {
                        CanonicalAgent::Antigravity => Some("antigravity"),
                        CanonicalAgent::Codex => Some("codex"),
                        CanonicalAgent::DeepseekHarness => Some("dsh-desktop"),
                        _ => None,
                    };
                    if let Some(a) = mapped {
                        return Some(a.to_string());
                    }
                }
            }
        }
        if let Some(adapter) = self.active_desktop_agent_adapter() {
            if matches!(adapter.as_str(), "antigravity" | "codex" | "dsh-desktop") {
                return Some(adapter);
            }
        }
        if let Some(bundle) = self.foreground_bundle_id.as_deref() {
            if bundle == "com.google.antigravity" {
                return Some("antigravity".to_string());
            }
            if bundle == "com.openai.codex" {
                return Some("codex".to_string());
            }
            if bundle.starts_with("com.deepseek.dsh") {
                return Some("dsh-desktop".to_string());
            }
        }
        if hold_action.is_some_and(
            crate::encoder_model_picker::ModelPickerCommand::is_open_model_picker_action,
        ) {
            return Some("antigravity".to_string());
        }
        None
    }

    pub(super) fn fire_due_encoder_hold(
        &mut self,
        session_id: u64,
        hold_generation: u64,
        now: Instant,
    ) -> Option<HostActionReceipt> {
        self.fire_due_encoder_hold_with_commit(session_id, hold_generation, now, true)
            .0
    }

    pub(super) fn fire_due_encoder_hold_with_commit(
        &mut self,
        session_id: u64,
        hold_generation: u64,
        now: Instant,
        commit: bool,
    ) -> (Option<HostActionReceipt>, bool) {
        if self.system_lifecycle != SystemLifecycle::Awake
            || !self.document_store.host_execution_enabled()
        {
            return (None, false);
        }
        let Some(current) = self.encoder_model_picker.hold_state() else {
            return (None, false);
        };
        if current.session_id != session_id || current.hold_generation != hold_generation {
            return (None, false);
        }
        if self
            .device_session
            .as_ref()
            .is_some_and(|s| s.id != session_id || s.is_disconnected())
        {
            self.cancel_encoder_model_picker();
            if commit {
                self.commit_runtime_update();
            }
            return (None, !commit);
        }
        let hold_profile = self
            .encoder_hold_profile_id
            .clone()
            .or_else(|| self.resolved_active_profile_id())
            .unwrap_or_default();
        if let Some(active_pid) = self.resolved_active_profile_id() {
            if !hold_profile.is_empty() && active_pid != hold_profile {
                self.cancel_encoder_model_picker();
                if commit {
                    self.commit_runtime_update();
                }
                return (None, !commit);
            }
        }
        let hold_action = self.resolve_encoder_hold_physical_action(&hold_profile);
        let target_adapter =
            self.resolve_model_picker_target_adapter(&hold_profile, hold_action.as_ref());
        let target_kind = hold_action
            .as_ref()
            .and_then(crate::encoder_model_picker::EncoderOverlayKind::from_action);
        match self.encoder_model_picker.on_hold_timer_with_picker_mode(
            hold_generation,
            target_adapter.as_deref(),
            hold_action.is_some(),
            target_kind,
            now,
        ) {
            crate::encoder_model_picker::EncoderHoldTimerOutcome::Ignored => (None, false),
            crate::encoder_model_picker::EncoderHoldTimerOutcome::FiredHold {
                overlay_kind,
                ..
            } => (
                self.execute_encoder_hold_outcome_with_commit(
                    overlay_kind,
                    &hold_profile,
                    session_id,
                    hold_action,
                    target_adapter,
                    now,
                    commit,
                ),
                !commit,
            ),
        }
    }

    fn format_overlay_status_message(
        kind: crate::encoder_model_picker::EncoderOverlayKind,
        detail: &str,
    ) -> String {
        match kind {
            crate::encoder_model_picker::EncoderOverlayKind::ModelPicker => {
                format!("已弹出模型选择页 · 左右旋转选模型，短按确认 ({detail})")
            }
            crate::encoder_model_picker::EncoderOverlayKind::SearchConversations => {
                format!("已打开搜索会话 · 上下旋转选择会话，短按确认 ({detail})")
            }
            crate::encoder_model_picker::EncoderOverlayKind::SubagentPicker => {
                format!("已打开 Subagent 选择 · 上下旋转选择，短按打开 ({detail})")
            }
        }
    }

    fn apply_overlay_transition(
        &mut self,
        transition: crate::encoder_model_picker::OverlayTransition,
        receipt: Option<&HostActionReceipt>,
    ) {
        self.apply_overlay_transition_with_commit(transition, receipt, true);
    }

    fn apply_overlay_transition_with_commit(
        &mut self,
        transition: crate::encoder_model_picker::OverlayTransition,
        receipt: Option<&HostActionReceipt>,
        commit: bool,
    ) {
        match transition {
            crate::encoder_model_picker::OverlayTransition::Entered(kind) => {
                self.set_model_picker_mouse_capture(kind.captures_mouse());
                if let Some(r) = receipt {
                    self.status_message = Self::format_overlay_status_message(kind, &r.detail);
                }
            }
            crate::encoder_model_picker::OverlayTransition::Cancelled => {
                self.cancel_encoder_model_picker();
            }
            crate::encoder_model_picker::OverlayTransition::None => {
                self.set_model_picker_mouse_capture(false);
            }
        }
        if commit {
            self.commit_runtime_update();
        }
    }

    pub(crate) fn finalize_encoder_confirm_outcome(
        &mut self,
        receipt: Option<&HostActionReceipt>,
    ) -> crate::encoder_model_picker::ConfirmExecutionDisposition {
        let disposition = self.encoder_model_picker.apply_confirm_execution_outcome(
            receipt.is_some_and(|r| r.succeeded()),
            receipt.map_or("", |r| r.detail.as_str()),
        );
        self.set_model_picker_mouse_capture(false);
        self.commit_runtime_update();
        disposition
    }

    pub(super) fn execute_encoder_short_press(
        &mut self,
        hold_profile: &str,
        session_id: u64,
        now: Instant,
    ) -> Option<HostActionReceipt> {
        let layout = self.current_bottom_row_layout();
        let Some(short_action) = self.resolve_physical_action(hold_profile, "ENC_CLK", layout)
        else {
            self.status_message = "已按下 ENC_CLK（当前槽位未绑定动作）".to_string();
            self.commit_runtime_update();
            return None;
        };
        let target_adapter =
            self.resolve_model_picker_target_adapter(hold_profile, Some(&short_action));
        let mut action = short_action;
        if let ResolvedHostAction::AgentCommand { adapter, .. } = &mut action {
            if matches!(adapter.as_str(), "host" | "active-agent") {
                if let Some(ref resolved) = target_adapter {
                    *adapter = resolved.clone();
                }
            }
        }
        let receipt = self.execute_resolved_physical_action_with_session(
            hold_profile,
            "ENC_CLK",
            session_id,
            action.clone(),
        );
        if let Some(ok_receipt) = receipt.as_ref().filter(|r| r.succeeded()) {
            let eff_adapter = target_adapter.unwrap_or_else(|| "antigravity".to_string());
            let transition = self
                .encoder_model_picker
                .activate_overlay_after_action_with_detail(
                    eff_adapter,
                    &action,
                    &ok_receipt.detail,
                    now,
                );
            self.apply_overlay_transition(transition, receipt.as_ref());
        }
        receipt
    }

    fn execute_encoder_hold_outcome(
        &mut self,
        overlay_kind: Option<crate::encoder_model_picker::EncoderOverlayKind>,
        hold_profile: &str,
        session_id: u64,
        hold_action: Option<ResolvedHostAction>,
        target_adapter: Option<String>,
        now: Instant,
    ) -> Option<HostActionReceipt> {
        self.execute_encoder_hold_outcome_with_commit(
            overlay_kind,
            hold_profile,
            session_id,
            hold_action,
            target_adapter,
            now,
            true,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_encoder_hold_outcome_with_commit(
        &mut self,
        overlay_kind: Option<crate::encoder_model_picker::EncoderOverlayKind>,
        hold_profile: &str,
        session_id: u64,
        hold_action: Option<ResolvedHostAction>,
        target_adapter: Option<String>,
        now: Instant,
        commit: bool,
    ) -> Option<HostActionReceipt> {
        let must_esc = self
            .encoder_model_picker
            .prepare_for_hold_action(overlay_kind);
        if must_esc {
            if let Some(ref adapter) = target_adapter {
                let cancel_action =
                    crate::encoder_model_picker::ModelPickerCommand::ModelPickerCancel
                        .into_host_action(adapter.clone());
                let _ = self.execute_resolved_physical_action_with_session_commit(
                    hold_profile,
                    "ENC_CLK",
                    session_id,
                    cancel_action,
                    false,
                );
            }
        }
        if overlay_kind != Some(crate::encoder_model_picker::EncoderOverlayKind::ModelPicker) {
            self.set_model_picker_mouse_capture(false);
        }
        let Some(mut action) = hold_action else {
            if commit {
                self.commit_runtime_update();
            }
            return None;
        };
        if let ResolvedHostAction::AgentCommand { adapter, .. } = &mut action {
            if matches!(adapter.as_str(), "host" | "active-agent") {
                if let Some(ref resolved) = target_adapter {
                    *adapter = resolved.clone();
                }
            }
        }
        let receipt = self.execute_resolved_physical_action_with_session_commit(
            hold_profile,
            "ENC_CLK",
            session_id,
            action,
            false,
        );
        let succeeded = receipt.as_ref().is_some_and(|r| r.succeeded());
        let adapter = target_adapter.unwrap_or_else(|| "antigravity".to_string());
        let transition = self.encoder_model_picker.apply_hold_execution_outcome(
            adapter,
            overlay_kind,
            succeeded,
            now,
        );
        self.apply_overlay_transition_with_commit(transition, receipt.as_ref(), commit);
        receipt
    }

    pub fn consume_physical_input_event(
        &mut self,
        event: InputEvent,
        now: Instant,
    ) -> Option<HostActionReceipt> {
        if self.system_lifecycle != SystemLifecycle::Awake {
            return None;
        }
        self.update_now = now;
        if event.kind == InputEventKind::InputStreamGap {
            self.input_lost_session = Some(event.session_id);
            self.release_board_inputs(now);
            self.commit_runtime_update();
            return None;
        }
        if event.kind != InputEventKind::Released
            && (self.input_lost_session == Some(event.session_id)
                || self.hardware_event_tx.source_closed(event.session_id))
        {
            return None;
        }
        let profile_id = self.resolved_active_profile_id();
        if event.kind != InputEventKind::Released
            && profile_id
                .as_ref()
                .is_none_or(|id| self.runtime_action_profile_id(id).is_none())
        {
            return None;
        }
        if let Some(pos) = event.radial_position {
            if self.radial_state.enabled() {
                let outcome = self.consume_radial_input(
                    RadialInputEvent::Position {
                        session_id: event.session_id,
                        ingress_seq: event.ingress_sequence,
                        sample: RadialPositionSample {
                            angle_turns: pos.angle_turns,
                            distance: pos.distance,
                        },
                        admitted: true,
                    },
                    now,
                );
                if outcome.consumed && event.kind == InputEventKind::UnknownNotification {
                    return outcome.dispatched_receipt;
                }
                if let Some(receipt) = outcome.dispatched_receipt {
                    return Some(receipt);
                }
            }
        }

        let binding_id = event.binding_id.clone()?;
        if event.kind != InputEventKind::Released
            && (!event.action_eligible
                || self.input_lost_session == Some(event.session_id)
                || self.live_executor.has_unconfirmed_apple_fn_release()
                || !self.ptt_controller.cleanup_confirmed)
        {
            return None;
        }

        let profile_id = profile_id.unwrap_or_default();

        match event.kind {
            InputEventKind::Released => {
                if binding_id == "ENC_CLK" {
                    self.dial_pressed = false;
                    self.dial_turned_while_pressed = false;
                    if self.encoder_model_picker.hold_state().is_some() {
                        let hold_profile = self
                            .encoder_hold_profile_id
                            .take()
                            .unwrap_or_else(|| profile_id.clone());
                        if !self.document_store.host_execution_enabled()
                            || (!profile_id.is_empty() && hold_profile != profile_id)
                        {
                            self.cancel_encoder_model_picker();
                            self.commit_runtime_update();
                            return None;
                        }
                        let hold_action = self.resolve_encoder_hold_physical_action(&hold_profile);
                        let target_adapter = self.resolve_model_picker_target_adapter(
                            &hold_profile,
                            hold_action.as_ref(),
                        );
                        let target_kind = hold_action
                            .as_ref()
                            .and_then(crate::encoder_model_picker::EncoderOverlayKind::from_action);
                        match self.encoder_model_picker.on_encoder_release_with_picker_mode(
                            event.session_id,
                            target_adapter.as_deref(),
                            hold_action.is_some(),
                            target_kind,
                            now,
                        ) {
                            crate::encoder_model_picker::EncoderReleaseOutcome::Consumed => {
                                return None;
                            }
                            crate::encoder_model_picker::EncoderReleaseOutcome::FiredHold {
                                overlay_kind,
                                ..
                            } => {
                                return self.execute_encoder_hold_outcome(
                                    overlay_kind,
                                    &hold_profile,
                                    event.session_id,
                                    hold_action,
                                    target_adapter,
                                    now,
                                );
                            }
                            crate::encoder_model_picker::EncoderReleaseOutcome::ModelPickerConfirm {
                                adapter,
                                command,
                            } => {
                                let confirm_action = command.into_host_action(adapter);
                                let receipt = self.execute_resolved_physical_action_with_session(
                                    &hold_profile,
                                    "ENC_CLK",
                                    event.session_id,
                                    confirm_action,
                                );
                                if self.finalize_encoder_confirm_outcome(receipt.as_ref())
                                    == crate::encoder_model_picker::ConfirmExecutionDisposition::FallbackToShortPress
                                {
                                    return self.execute_encoder_short_press(
                                        &hold_profile,
                                        event.session_id,
                                        now,
                                    );
                                }
                                return receipt;
                            }
                            crate::encoder_model_picker::EncoderReleaseOutcome::ShortPress => {
                                return self.execute_encoder_short_press(
                                    &hold_profile,
                                    event.session_id,
                                    now,
                                );
                            }
                        }
                    }
                }
                let input = HeldInputId {
                    session_id: event.session_id,
                    binding_id: binding_id.clone(),
                };
                if let Some(receipt) = self.live_executor.handle_apple_fn_release(&input) {
                    let localized_detail = crate::i18n::localize_managed_error_detail(
                        &receipt.detail,
                        mi_model::ResolvedLocale::ZhHans,
                    );
                    self.status_message = format!("已释放 {binding_id} · {localized_detail}");
                    if self.live_executor.has_unconfirmed_apple_fn_release() {
                        self.host_caution = Some(HostActionCaution {
                            detail_code: "blocked:unconfirmed_apple_fn_release".into(),
                            is_sticky_cleanup: true,
                            expires_at: None,
                        });
                    } else {
                        self.update_host_caution_from_receipt(&receipt, now);
                    }
                    self.record_host_action_receipt(receipt.clone());
                    self.commit_runtime_update();
                    return Some(receipt);
                }
                if self
                    .held_dictation
                    .as_ref()
                    .is_some_and(|held| held.input == input)
                {
                    let stopped = if self.config.isolated {
                        Ok(())
                    } else {
                        self.live_executor.stop_owned_dictations()
                    };
                    return self.finish_dictation(stopped);
                }
                None
            }
            InputEventKind::Pressed => {
                if self.radial_state.enabled() {
                    let radial_outcome = self.consume_radial_input(
                        RadialInputEvent::KeyPress {
                            session_id: event.session_id,
                            ingress_seq: event.ingress_sequence,
                            binding_id: binding_id.clone(),
                            admitted: true,
                        },
                        now,
                    );
                    if radial_outcome.consumed {
                        return radial_outcome.dispatched_receipt;
                    }
                }

                let inspector_search_active = !self.action_search_query.trim().is_empty()
                    || !self.keycap_search_query.trim().is_empty()
                    || self.shortcut_recorder.is_recording;
                if !inspector_search_active {
                    if let Some(surface) = self.surface_for_binding(&binding_id) {
                        self.selected_surface = surface;
                        let slot = self.effective_slot_for_surface(surface);
                        self.selected_binding_slot = Some(slot);
                    }
                    match binding_id.as_str() {
                        "AG15" => self.selected_joystick_direction = JoystickDirection8::North,
                        "AG16" => self.selected_joystick_direction = JoystickDirection8::West,
                        "AG17" => self.selected_joystick_direction = JoystickDirection8::South,
                        "AG18" => self.selected_joystick_direction = JoystickDirection8::East,
                        _ => {}
                    }
                }

                if !self.document_store.host_execution_enabled() {
                    self.status_message = format!("已按下 {binding_id}（已暂停软件按键映射）");
                    self.commit_runtime_update();
                    return None;
                }

                if self.encoder_model_picker.expire_if_idle(now).is_some() {
                    self.set_model_picker_mouse_capture(false);
                }

                if binding_id == "ENC_CLK" {
                    self.dial_pressed = true;
                    self.dial_turned_while_pressed = false;
                    let hold_action = self.resolve_encoder_hold_physical_action(&profile_id);
                    let picker_active = self.encoder_model_picker.is_active(now);
                    if hold_action.is_some() || picker_active {
                        let hold_gen = self
                            .encoder_model_picker
                            .on_encoder_press(event.session_id, now);
                        self.encoder_hold_profile_id = Some(profile_id);
                        if hold_action.is_some() {
                            let tx = self.hardware_event_tx.clone();
                            let session_id = event.session_id;
                            self.tokio_runtime.spawn(async move {
                                tokio::time::sleep(
                                    crate::encoder_model_picker::ENCODER_HOLD_THRESHOLD,
                                )
                                .await;
                                let _ = tx.send(HardwareLoopEvent::EncoderHoldTimer {
                                    session_id,
                                    hold_generation: hold_gen,
                                });
                            });
                        }
                        self.commit_runtime_update();
                        return None;
                    }
                } else if matches!(
                    binding_id.as_str(),
                    "AG13" | "ENC_CC" | "ENC_CCW" | "AG14" | "ENC_CW"
                ) {
                    let clockwise = matches!(binding_id.as_str(), "AG14" | "ENC_CW");
                    match self.encoder_model_picker.on_encoder_turn(clockwise, now) {
                        crate::encoder_model_picker::EncoderTurnOutcome::ModelPicker(cmd) => {
                            if let Some(adapter) = self
                                .encoder_model_picker
                                .active_adapter(now)
                                .map(str::to_owned)
                            {
                                let action = cmd.into_host_action(adapter);
                                return self.execute_resolved_physical_action_with_session(
                                    &profile_id,
                                    &binding_id,
                                    event.session_id,
                                    action,
                                );
                            }
                        }
                        crate::encoder_model_picker::EncoderTurnOutcome::FastScroll {
                            clockwise,
                        } => {
                            self.dial_turned_while_pressed = true;
                            self.encoder_hold_profile_id = None;
                            let adapter = self
                                .resolve_model_picker_target_adapter(&profile_id, None)
                                .or_else(|| self.active_desktop_agent_adapter())
                                .unwrap_or_else(|| "antigravity".to_string());
                            let scroll_action = ResolvedHostAction::AgentCommand {
                                adapter,
                                command: if clockwise {
                                    "scroll-down-fast".to_string()
                                } else {
                                    "scroll-up-fast".to_string()
                                },
                            };
                            return self.execute_resolved_physical_action_with_session(
                                &profile_id,
                                &binding_id,
                                event.session_id,
                                scroll_action,
                            );
                        }
                        crate::encoder_model_picker::EncoderTurnOutcome::NormalSlot => {}
                    }
                } else if !matches!(binding_id.as_str(), "AG15" | "AG16" | "AG17" | "AG18")
                    && self.encoder_model_picker.is_active(now)
                {
                    self.cancel_encoder_model_picker();
                }

                let layout = self.current_bottom_row_layout();
                let Some(action) = self.resolve_physical_action(&profile_id, &binding_id, layout)
                else {
                    self.status_message = format!("已按下 {binding_id}（当前槽位未绑定动作）");
                    self.commit_runtime_update();
                    return None;
                };
                if matches!(binding_id.as_str(), "AG15" | "AG16" | "AG17" | "AG18")
                    && self.encoder_model_picker.is_active(now)
                {
                    self.cancel_encoder_model_picker();
                }

                let target_adapter =
                    self.resolve_model_picker_target_adapter(&profile_id, Some(&action));
                let receipt = self.execute_resolved_physical_action_with_session(
                    &profile_id,
                    &binding_id,
                    event.session_id,
                    action.clone(),
                );
                if let Some(ok_receipt) = receipt.as_ref().filter(|r| r.succeeded()) {
                    let eff_adapter = target_adapter
                        .or_else(|| self.active_desktop_agent_adapter())
                        .unwrap_or_else(|| "antigravity".to_string());
                    let transition = self
                        .encoder_model_picker
                        .activate_overlay_after_action_with_detail(
                            eff_adapter,
                            &action,
                            &ok_receipt.detail,
                            now,
                        );
                    if transition != crate::encoder_model_picker::OverlayTransition::None {
                        self.apply_overlay_transition(transition, receipt.as_ref());
                    }
                }
                receipt
            }
            _ => None,
        }
    }

    pub(super) fn execute_resolved_physical_action_with_session(
        &mut self,
        profile_id: &str,
        binding_id: &str,
        session_id: u64,
        action: ResolvedHostAction,
    ) -> Option<HostActionReceipt> {
        self.execute_resolved_physical_action_with_session_commit(
            profile_id, binding_id, session_id, action, true,
        )
    }

    pub(super) fn execute_resolved_physical_action_with_session_commit(
        &mut self,
        profile_id: &str,
        binding_id: &str,
        session_id: u64,
        mut action: ResolvedHostAction,
        commit: bool,
    ) -> Option<HostActionReceipt> {
        if matches!(
            binding_id,
            "AG13" | "AG14" | "ENC_CC" | "ENC_CCW" | "ENC_CW"
        ) && self.dial_pressed
        {
            self.dial_turned_while_pressed = true;
            if let ResolvedHostAction::AgentCommand { command, .. } = &mut action {
                if command == "scroll-up" {
                    *command = "scroll-up-fast".to_string();
                } else if command == "scroll-down" {
                    *command = "scroll-down-fast".to_string();
                }
            }
        }

        if let ResolvedHostAction::Shortcut {
            key_code,
            modifiers,
            target_bundle_id,
            ..
        } = &action
        {
            if *key_code == APPLE_FN_KEY_CODE && modifiers.is_empty() {
                let target = target_bundle_id.as_deref().unwrap_or("frontmost-editor");
                if let Some(receipt) = self.live_executor.handle_apple_fn_press(
                    &HeldInputId {
                        session_id,
                        binding_id: binding_id.to_string(),
                    },
                    profile_id,
                    target,
                ) {
                    let now = self.update_now;
                    self.apply_host_action_receipt(receipt.clone(), Some(binding_id), now);
                    if commit {
                        self.commit_runtime_update();
                    }
                    return Some(receipt);
                }
                return None;
            }
        }

        if let ResolvedHostAction::AgentCommand { adapter, command } = &mut action {
            if matches!(adapter.as_str(), "host" | "active-agent")
                && !mi_model::is_host_mouse_command(command)
            {
                *adapter = if command.starts_with("model-picker")
                    || command.starts_with("search-")
                    || command == "model"
                    || command == "search"
                {
                    self.resolve_model_picker_target_adapter(profile_id, None)
                        .or_else(|| self.active_desktop_agent_adapter())
                        .unwrap_or_else(|| "antigravity".to_string())
                } else {
                    self.active_desktop_agent_adapter()
                        .unwrap_or_else(|| "antigravity".to_string())
                };
            }
            if let Some(command) =
                NavigationCommand::from_action(&ResolvedHostAction::AgentCommand {
                    adapter: adapter.clone(),
                    command: command.clone(),
                })
            {
                self.request_navigation_command_with_commit(
                    command,
                    None,
                    Some(NavigationInputLease::Vendor(session_id)),
                    profile_id.to_string(),
                    binding_id.to_string(),
                    commit,
                );
                return None;
            }
            if command == "push-to-talk" {
                let resolved_adapter = adapter.clone();
                if self.held_dictation.is_some() {
                    return None;
                }
                let started = if self.config.isolated {
                    Ok(())
                } else {
                    self.live_executor.start_owned_dictation(&resolved_adapter)
                };
                let uncertain = started
                    .as_ref()
                    .is_err_and(|error| error.starts_with("unconfirmed:"));
                self.ptt_controller.is_active = started.is_ok();
                self.ptt_controller.cleanup_confirmed = !uncertain;
                if started.is_ok() || uncertain {
                    self.held_dictation = Some(HeldDictation {
                        input: HeldInputId {
                            session_id,
                            binding_id: binding_id.to_string(),
                        },
                        profile_id: profile_id.to_string(),
                        adapter: resolved_adapter.clone(),
                    });
                }
                let (outcome, detail) = match started {
                    Ok(()) => (
                        if self.config.isolated {
                            "simulated"
                        } else {
                            "executed"
                        },
                        format!("{resolved_adapter}/push-to-talk"),
                    ),
                    Err(error) => (if uncertain { "unconfirmed" } else { "blocked" }, error),
                };
                let receipt = HostActionReceipt {
                    action_type: "agent_command".into(),
                    binding_id: binding_id.to_string(),
                    detail: detail.clone(),
                    outcome: outcome.into(),
                    profile_id: profile_id.to_string(),
                    target: resolved_adapter,
                    text_byte_len: None,
                    timestamp_ms: 0,
                };
                let localized_detail = crate::i18n::localize_managed_error_detail(
                    &detail,
                    mi_model::ResolvedLocale::ZhHans,
                );
                self.status_message = if self.ptt_controller.is_active {
                    format!("正在按住语音 ({binding_id})…")
                } else {
                    format!("听写未启动 · {localized_detail}")
                };
                if uncertain {
                    self.host_caution = Some(HostActionCaution {
                        detail_code: receipt.detail.clone(),
                        is_sticky_cleanup: true,
                        expires_at: None,
                    });
                } else {
                    self.update_host_caution_from_receipt(&receipt, self.update_now);
                }
                self.record_host_action_receipt(receipt.clone());
                if commit {
                    self.commit_runtime_update();
                }
                return Some(receipt);
            }
        }

        let receipt = if self.config.isolated {
            mi_macos::SimulatedHostActionExecutor::new().execute(profile_id, binding_id, &action)
        } else {
            self.live_executor.execute(profile_id, binding_id, &action)
        };
        if receipt.succeeded() && receipt.detail.ends_with("/model-picker-already-closed") {
            return Some(receipt);
        }

        if let ResolvedHostAction::OpenApp { bundle_id } = &action {
            if receipt.succeeded() {
                self.navigation_history
                    .visit(NavigationPlace::app(bundle_id.clone()));
            }
        }

        let now = self.update_now;
        self.apply_host_action_receipt(receipt.clone(), Some(binding_id), now);
        if commit {
            self.commit_runtime_update();
        }
        Some(receipt)
    }
}
