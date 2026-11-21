use super::device_sync::{parse_hardware_lighting_payload, sync_hardware_lighting_to_session};
use super::inspector_editor_actions::bundle_id_for_place;
use super::*;

impl WorkbenchStore {
    fn recommended_board_plan(&self) -> Result<BoardLayerPlan, WorkbenchError> {
        let required = |id: Option<String>| {
            id.ok_or_else(|| WorkbenchError::Preset("默认六层所需的软件方案未就绪".into()))
        };
        Ok(BoardLayerPlan::recommended(
            required(self.find_canonical_profile_id(CanonicalAgent::Codex))?,
            required(self.find_canonical_profile_id(CanonicalAgent::Antigravity))?,
            required(self.find_canonical_profile_id(CanonicalAgent::DeepseekHarness))?,
        ))
    }

    pub(super) fn initialize_board_plan(&mut self) -> Result<(), WorkbenchError> {
        if self.document_store.board_layer_plan().is_none() {
            let plan = self.recommended_board_plan()?;
            self.document_store.initialize_board_plan_if_absent(plan);
        }
        Ok(())
    }

    fn route_for_profile(&self, profile_id: &str) -> Result<BoardLayerRoute, WorkbenchError> {
        self.projection_inputs().route_for_profile(profile_id)
    }

    fn save_board_plan(
        &mut self,
        plan: BoardLayerPlan,
        preview: BoardPreviewSelection,
        usb: Option<String>,
    ) -> Result<(), WorkbenchError> {
        self.document_store
            .save_board_configuration(plan, preview, usb)
            .map_err(WorkbenchError::Storage)?;
        self.commit_local_update();
        Ok(())
    }

    pub fn preview_board_layer(&mut self, layer: BoardLayer) -> Result<(), WorkbenchError> {
        if self.document_store.editing_device() {
            return Err(WorkbenchError::Storage(
                "请先返回软件方案，再查看键盘层".into(),
            ));
        }
        let profile_id = self
            .document_store
            .board_layer_plan()
            .and_then(|plan| plan.route(layer).profile_id())
            .filter(|id| {
                self.document_store
                    .draft
                    .document
                    .editable
                    .graph
                    .profiles
                    .contains_key(*id)
            })
            .map(str::to_owned);
        self.preview_board_selection(BoardPreviewSelection::Layer(layer))?;
        self.set_browsed_profile(profile_id);
        self.commit_local_update();
        Ok(())
    }

    fn preview_board_selection(
        &mut self,
        preview: BoardPreviewSelection,
    ) -> Result<(), WorkbenchError> {
        self.save_board_plan(
            self.document_store
                .board_layer_plan()
                .cloned()
                .unwrap_or_default(),
            preview,
            self.document_store
                .usb_startup_profile_id()
                .map(str::to_owned),
        )
    }

    pub fn assign_board_layer(
        &mut self,
        layer: BoardLayer,
        profile_id: Option<String>,
    ) -> Result<(), WorkbenchError> {
        let route = profile_id
            .as_deref()
            .map(|id| self.route_for_profile(id))
            .transpose()?
            .unwrap_or_default();
        let mut plan = self
            .document_store
            .board_layer_plan()
            .cloned()
            .unwrap_or_default();
        plan.replace(layer, route);
        self.save_board_plan(
            plan,
            self.document_store.board_preview_selection(),
            self.document_store
                .usb_startup_profile_id()
                .map(str::to_owned),
        )
    }

    pub fn set_usb_startup_profile(
        &mut self,
        profile_id: Option<String>,
    ) -> Result<(), WorkbenchError> {
        if let Some(id) = &profile_id {
            self.route_for_profile(id)?;
        }
        self.save_board_plan(
            self.document_store
                .board_layer_plan()
                .cloned()
                .unwrap_or_default(),
            self.document_store.board_preview_selection(),
            profile_id,
        )
    }

    pub fn reset_board_layer_plan(&mut self) -> Result<(), WorkbenchError> {
        let plan = self.recommended_board_plan()?;
        self.save_board_plan(
            plan,
            self.document_store.board_preview_selection(),
            self.document_store
                .usb_startup_profile_id()
                .map(str::to_owned),
        )
    }

    pub fn browse_profile(&mut self, profile_id: &str) -> Result<(), WorkbenchError> {
        if !self
            .document_store
            .draft
            .document
            .editable
            .graph
            .profiles
            .contains_key(profile_id)
        {
            return Err(WorkbenchError::Routing("方案不存在".into()));
        }
        self.set_browsed_profile(Some(profile_id.to_owned()));
        self.commit_local_update();
        Ok(())
    }

    fn set_browsed_profile(&mut self, profile_id: Option<String>) {
        if self.selected_profile_id == profile_id {
            return;
        }
        self.selected_layer_id = profile_id.as_ref().and_then(|id| {
            self.document_store
                .draft
                .document
                .editable
                .graph
                .profiles
                .get(id)
                .and_then(|profile| profile.layer_order.first().cloned())
        });
        self.selected_profile_id = profile_id;
        self.normalize_surface_for_current_layer();
        self.sync_lighting_preview_after_document_change();
    }

    pub fn select_and_activate_profile(&mut self, profile_id: &str) -> Result<(), WorkbenchError> {
        let started = Instant::now();
        let result = self.select_and_activate_profile_at(profile_id, started, true);
        let elapsed = started.elapsed();
        self.last_switch_duration = elapsed;
        self.projection.last_switch_duration = elapsed;
        result
    }

    pub fn select_and_activate_profile_at(
        &mut self,
        profile_id: &str,
        now: Instant,
        emit_hud_notice: bool,
    ) -> Result<(), WorkbenchError> {
        self.update_now = now;
        let first_layer_id = self
            .document_store
            .draft
            .document
            .editable
            .graph
            .profiles
            .get(profile_id)
            .ok_or_else(|| WorkbenchError::Routing("Profile 不存在".to_string()))?
            .layer_order
            .first()
            .cloned();

        self.apply_routing_operation(RoutingOperation::Select {
            profile_id: profile_id.to_string(),
        })?;
        self.cancel_encoder_model_picker();
        let context_changed = self.selected_profile_id.as_deref() != Some(profile_id)
            || self.selected_layer_id != first_layer_id;
        self.selected_profile_id = Some(profile_id.to_string());
        self.selected_layer_id = first_layer_id;
        self.v070_hardware.device_layer_override = None;
        self.v070_hardware.pending_auto_layer_sync = None;
        self.six_layer_user_pinned = true;
        self.normalize_surface_for_current_layer();
        if context_changed {
            self.sync_lighting_preview_after_document_change();
        }
        self.sync_six_layer_routing_context();
        let _ = self.maybe_sync_v070_active_profile_layer();
        self.sync_live_session_source();
        self.refresh_four_app_lighting();
        if emit_hud_notice && !self.overlay_controller.is_cheat_sheet_visible() {
            let profile_name = self.active_profile_display_name();
            let mode_summary = self.routing_mode_summary_label();
            let empty_sheet = CheatSheetProjection::default();
            self.overlay_controller.request_profile_switch_notice(
                profile_name,
                mode_summary,
                &empty_sheet,
                &self.screen_geometry,
                self.last_cursor_position,
                now,
            );
        }
        self.commit_runtime_update();
        if self.projection.header.banner.is_caution() {
            self.banner_flash_count += 1;
            self.projection.banner_flash_count = self.banner_flash_count;
            self.projection_generation = self.projection_generation.wrapping_add(1);
            self.last_committed_stamp = Some(self.current_runtime_stamp());
        }
        Ok(())
    }

    pub fn foreground_changed(&mut self, bundle_id: Option<&str>) -> bool {
        let changed = self.apply_foreground_changed_routing(bundle_id);
        self.sync_live_session_source();
        changed
    }

    fn apply_foreground_changed_routing(&mut self, bundle_id: Option<&str>) -> bool {
        let bundle_id = bundle_id
            .map(str::trim)
            .filter(|bundle| !bundle.is_empty() && *bundle != "*");
        let before_gen = self.projection_generation;
        let mut env_dirty = self.refresh_accessibility_trusted();
        if bundle_id == Some("com.worklouder.input") {
            env_dirty |= self
                .applications
                .insert("com.worklouder.input".to_string(), true)
                != Some(true);
        } else {
            env_dirty |= self.refresh_competing_input_app_running(false);
        }
        self.navigation_foreground_changed(bundle_id);
        let bundle_changed = self.foreground_bundle_id.as_deref() != bundle_id;
        if bundle_changed {
            self.cancel_encoder_model_picker();
        }
        self.foreground_bundle_id = bundle_id.map(str::to_owned);
        let unlocked = self.document_store.locked_profile_id().is_none();
        let updated = if let Some(running) = &self.six_layer_running {
            let running_updated = running.foreground_changed(bundle_id);
            if bundle_changed && unlocked {
                self.sync_v070_session_sa_rgb();
                self.commit_runtime_update();
                running_updated
            } else {
                if bundle_id == Some("com.worklouder.input") {
                    self.commit_runtime_update();
                }
                false
            }
        } else {
            if unlocked && bundle_changed {
                self.sync_v070_session_sa_rgb();
            }
            if bundle_id == Some("com.worklouder.input") {
                self.commit_runtime_update();
            }
            false
        };
        if env_dirty && self.projection_generation == before_gen {
            self.commit_runtime_update();
        }
        updated
    }

    pub fn set_host_execution_enabled(&mut self, enabled: bool) -> Result<(), WorkbenchError> {
        self.apply_routing_operation(RoutingOperation::Execution { enabled })?;
        if let Some(running) = &self.six_layer_running {
            running.set_execution_policy(enabled, self.command_execution_enabled, self.update_now);
        }
        if enabled {
            let retired_cleanup_fence = self.six_layer_running.as_ref().is_some_and(|running| {
                running.is_retired()
                    && running.receiver.failure().as_deref()
                        == Some("宿主动作清理未确认，输入已暂停")
            });
            if self.can_retry_host_cleanup() {
                let stopped = self.live_executor.stop_owned_actions();
                let _ = self.finish_host_cleanup(stopped);
                self.finalize_recovered_host_cleanup();
            } else if retired_cleanup_fence {
                self.finalize_recovered_host_cleanup();
            }
            if self.status_message == "已暂停软件按键映射"
                || (!self.can_retry_host_cleanup()
                    && (self.status_message.starts_with("原目标释放未确认 · ")
                        || self.status_message.starts_with("宿主动作清理未确认")
                        || self.status_message.starts_with("原目标听写停止未确认 · ")
                        || self.status_message.starts_with("旧会话清理未确认")
                        || self.status_message.starts_with("清理任务已中断")))
            {
                self.status_message = "已启用软件按键映射".to_string();
            }
            let _ = self.maybe_sync_v070_active_profile_layer();
        } else {
            self.status_message = "已暂停软件按键映射".to_string();
            self.v070_hardware.pending_auto_layer_sync = None;
            self.release_board_inputs(self.update_now);
        }
        self.commit_runtime_update();
        Ok(())
    }

    pub fn set_show_other_profiles(&mut self, show: bool) {
        self.show_other_profiles = show;
        self.ensure_selected_profile_visible();
        self.commit_runtime_update();
    }

    pub fn undo_draft(&mut self, redo: bool) -> Result<(), WorkbenchError> {
        self.document_store
            .update(|draft| draft.undo(redo))
            .map_err(WorkbenchError::Storage)?;
        self.ensure_selected_profile_visible();
        self.normalize_surface_for_current_layer();
        self.sync_lighting_preview_after_document_change();
        self.sync_six_layer_routing_context();
        self.sync_v070_session_sa_rgb();
        self.commit_runtime_update();
        Ok(())
    }

    pub fn apply_profile_command(&mut self, cmd: ProfileCommand) -> Result<(), WorkbenchError> {
        match cmd {
            ProfileCommand::Create { name } => {
                let mut created_id = String::new();
                self.document_store
                    .update(|draft| {
                        created_id = draft.create_profile(&name)?;
                        Ok(())
                    })
                    .map_err(WorkbenchError::Storage)?;
                if CanonicalAgent::from_profile_name(&name).is_none() {
                    self.show_other_profiles = true;
                }
                self.selected_profile_id = Some(created_id.clone());
                self.selected_layer_id = self
                    .document_store
                    .draft
                    .document
                    .editable
                    .graph
                    .profiles
                    .get(&created_id)
                    .and_then(|p| p.layer_order.first().cloned());
                self.normalize_surface_for_current_layer();
                self.sync_lighting_preview_after_document_change();
                self.status_message = format!("已新建方案「{name}」");
            }
            ProfileCommand::Rename { profile_id, name } => {
                self.document_store
                    .update(|draft| draft.rename_profile(&profile_id, &name))
                    .map_err(WorkbenchError::Storage)?;
                self.status_message = format!("已重命名为「{name}」");
            }
            ProfileCommand::Duplicate { profile_id, name } => {
                let resolved_name = if name.trim().is_empty() {
                    let src_name = self
                        .document_store
                        .draft
                        .document
                        .editable
                        .graph
                        .profiles
                        .get(&profile_id)
                        .map(|p| p.name.as_str())
                        .unwrap_or("新建方案");
                    format!("{src_name} 副本")
                } else {
                    name
                };
                let mut duplicated_id = String::new();
                self.document_store
                    .update(|draft| {
                        duplicated_id = draft.duplicate_profile(&profile_id, &resolved_name)?;
                        Ok(())
                    })
                    .map_err(WorkbenchError::Storage)?;
                if CanonicalAgent::from_profile_name(&resolved_name).is_none() {
                    self.show_other_profiles = true;
                }
                self.selected_profile_id = Some(duplicated_id.clone());
                self.selected_layer_id = self
                    .document_store
                    .draft
                    .document
                    .editable
                    .graph
                    .profiles
                    .get(&duplicated_id)
                    .and_then(|p| p.layer_order.first().cloned());
                self.normalize_surface_for_current_layer();
                self.sync_lighting_preview_after_document_change();
                self.status_message = format!("已复制方案「{resolved_name}」");
            }
            ProfileCommand::MoveUp { profile_id } => {
                let order = &self
                    .document_store
                    .draft
                    .document
                    .editable
                    .graph
                    .profile_order;
                if let Some(pos) = order.iter().position(|id| id == &profile_id) {
                    if pos > 0 {
                        self.document_store
                            .update(|draft| draft.move_profile(&profile_id, pos - 1))
                            .map_err(WorkbenchError::Storage)?;
                    }
                }
            }
            ProfileCommand::MoveDown { profile_id } => {
                let order = &self
                    .document_store
                    .draft
                    .document
                    .editable
                    .graph
                    .profile_order;
                let len = order.len();
                if let Some(pos) = order.iter().position(|id| id == &profile_id) {
                    if pos + 1 < len {
                        self.document_store
                            .update(|draft| draft.move_profile(&profile_id, pos + 1))
                            .map_err(WorkbenchError::Storage)?;
                    }
                }
            }
            ProfileCommand::Delete { profile_id } => {
                let deleted_selected =
                    self.selected_profile_id.as_deref() == Some(profile_id.as_str());
                self.document_store
                    .update(|draft| draft.delete_profile(&profile_id))
                    .map_err(WorkbenchError::Storage)?;
                if deleted_selected {
                    self.selected_profile_id = None;
                    self.selected_layer_id = None;
                }
                self.sync_routing_snapshot();
                self.ensure_selected_profile_visible();
                if deleted_selected {
                    self.normalize_surface_for_current_layer();
                    self.sync_lighting_preview_after_document_change();
                }
                self.status_message = "已删除方案".to_string();
            }
            ProfileCommand::InstallCodingAgentPresets => {
                self.document_store
                    .update(|draft| {
                        mi_model::preset::install_agent_starter_profiles(draft).map(|_| ())
                    })
                    .map_err(WorkbenchError::Preset)?;
                self.status_message = "已恢复默认软件方案".to_string();
            }
        }
        self.sync_six_layer_routing_context();
        self.sync_live_session_source();
        self.sync_v070_session_sa_rgb();
        self.commit_runtime_update();
        Ok(())
    }

    pub fn apply_backup_command(
        &mut self,
        cmd: BackupCommand,
    ) -> Result<Option<String>, WorkbenchError> {
        match cmd {
            BackupCommand::ExportPreset {
                profile_id,
                layer_id,
            } => {
                let exported = mi_model::preset::export(
                    &self.document_store.draft,
                    profile_id.as_deref(),
                    layer_id.as_deref(),
                )
                .map_err(WorkbenchError::Preset)?;
                self.last_exported_preset = Some(exported.clone());
                self.status_message = "已导出方案预设".to_string();
                self.commit_runtime_update();
                Ok(Some(exported))
            }
            BackupCommand::ImportPreset { text } => {
                let mut imported =
                    mi_model::preset::import(&text).map_err(WorkbenchError::Preset)?;
                imported.revision = self.document_store.draft.revision + 1;
                self.document_store
                    .update(|draft| {
                        *draft = imported;
                        Ok(())
                    })
                    .map_err(WorkbenchError::Storage)?;
                self.end_lighting_preview(LightingPreviewStopReason::ContextChanged);
                self.sync_routing_snapshot();
                self.prepare_desktop_profiles()?;
                self.show_other_profiles = true;
                self.v070_hardware.device_layer_override = None;
                self.ensure_selected_profile_visible();
                self.normalize_surface_for_current_layer();
                self.sync_lighting_preview_after_document_change();
                self.sync_v070_session_sa_rgb();
                self.status_message = "已导入方案预设".to_string();
                self.commit_runtime_update();
                Ok(None)
            }
            BackupCommand::ReturnToHostDocument => {
                self.document_store.return_to_host();
                self.sync_routing_snapshot();
                self.v070_hardware.device_layer_override = None;
                self.ensure_selected_profile_visible();
                self.normalize_surface_for_current_layer();
                self.sync_modal_for_layer_protection();
                self.end_lighting_preview(LightingPreviewStopReason::ContextChanged);
                self.sync_lighting_preview_after_document_change();
                self.sync_v070_session_sa_rgb();
                self.status_message = "已返回常用软件方案".to_string();
                self.commit_runtime_update();
                Ok(None)
            }
        }
    }

    pub fn effective_locale(&self) -> mi_model::ResolvedLocale {
        mi_model::ResolvedLocale::resolve(
            self.document_store.ui_language(),
            self.config.system_locale,
        )
    }

    pub(super) fn apply_locale_transition(&mut self, next_locale: mi_model::ResolvedLocale) {
        if let InspectorBodyProjection::Editable(prev) = &mut self.projection.inspector.body {
            prev.filtered_keycaps.clear();
            prev.available_categories.clear();
            prev.filtered_actions.clear();
        }
        self.action_category_filter =
            if mi_model::i18n::is_all_category_filter(&self.action_category_filter) {
                mi_model::i18n::all_category_label(next_locale).to_string()
            } else {
                mi_model::i18n::localize_category(&self.action_category_filter, next_locale)
            };
        self.radial_state.cancel();
    }

    pub fn dispatch_header_action(&mut self, action: HeaderAction) -> Result<(), WorkbenchError> {
        match action {
            HeaderAction::ConnectKeyboard => {
                if self.config.isolated {
                    self.status_message = "正在检测小键盘连接…".to_string();
                    self.commit_runtime_update();
                } else {
                    self.start_mac_usb_discovery_watcher();
                    if let Err(err) = self.connect_default_usb() {
                        self.status_message = err.to_string();
                        self.commit_runtime_update();
                    }
                }
                Ok(())
            }
            HeaderAction::ReturnToHostDocument => self
                .apply_backup_command(BackupCommand::ReturnToHostDocument)
                .map(|_| ()),
            HeaderAction::SetExecutionEnabled(enabled) => self.set_host_execution_enabled(enabled),
            HeaderAction::Undo => self.undo_draft(false),
            HeaderAction::Redo => self.undo_draft(true),
            HeaderAction::OpenAccessibilitySettings => {
                self.open_accessibility_settings();
                Ok(())
            }
            HeaderAction::RetryHostCleanup => {
                self.retry_host_cleanup();
                Ok(())
            }
            HeaderAction::OpenSettings => {
                self.refresh_accessibility_trusted();
                self.refresh_competing_input_app_running(true);
                self.status_menu_open = !self.status_menu_open;
                self.status_message = if self.status_menu_open {
                    "已打开高级设置".to_string()
                } else {
                    "已关闭高级设置".to_string()
                };
                self.commit_runtime_update();
                Ok(())
            }
            HeaderAction::SelectCandidateDevice { selector } => {
                self.select_candidate_device_from_ui(&selector)
            }
            HeaderAction::SetUiLanguage { language } => {
                let prev_locale = self.effective_locale();
                self.document_store
                    .set_ui_language(language)
                    .map_err(WorkbenchError::Storage)?;
                let next_locale = self.effective_locale();
                if next_locale != prev_locale {
                    self.apply_locale_transition(next_locale);
                }
                self.commit_runtime_update();
                Ok(())
            }
            HeaderAction::SyncSystemLocale {
                preferred_languages,
            } => {
                let prev_locale = self.effective_locale();
                self.config.system_locale =
                    mi_model::ResolvedLocale::from_bcp47_tags(&preferred_languages);
                let next_locale = self.effective_locale();
                if next_locale != prev_locale {
                    self.apply_locale_transition(next_locale);
                }
                self.commit_runtime_update();
                Ok(())
            }
            HeaderAction::SetSilentAtLogin { silent } => {
                self.document_store
                    .set_silent_at_login(silent)
                    .map_err(WorkbenchError::Storage)?;
                self.commit_runtime_update();
                Ok(())
            }
        }
    }

    pub fn dispatch_sidebar_action(&mut self, action: SidebarAction) -> Result<(), WorkbenchError> {
        match action {
            SidebarAction::SelectProfile(profile_id) => {
                self.select_and_activate_profile_at(&profile_id, self.update_now, true)
            }
            SidebarAction::BrowseProfile(profile_id) => self.browse_profile(&profile_id),
            SidebarAction::PreviewBoardLayer(layer) => self.preview_board_layer(layer),
            SidebarAction::PreviewUsbStartup => {
                self.preview_board_selection(BoardPreviewSelection::UsbStartup)
            }
            SidebarAction::ProvisionBoardLayers => self.provision_board_layers(),
            SidebarAction::RestoreBoardLayers => self.start_six_layer_restore(),
            SidebarAction::SetBoardLayerProfile { layer, profile_id } => {
                self.assign_board_layer(layer, profile_id)
            }
            SidebarAction::SetUsbStartupProfile(profile_id) => {
                self.set_usb_startup_profile(profile_id)
            }
            SidebarAction::ResetBoardLayerPlan => self.reset_board_layer_plan(),
            SidebarAction::SetProfileLocked(locked) => self.set_profile_locked(locked, true),
            SidebarAction::SetShowOtherProfiles(show) => {
                self.set_show_other_profiles(show);
                Ok(())
            }
            SidebarAction::Profile(cmd) => self.apply_profile_command(cmd),
            SidebarAction::Backup(cmd) => self.apply_backup_command(cmd).map(|_| ()),
        }
    }

    pub fn navigation_history(&self) -> &NavigationHistory {
        &self.navigation_history
    }

    pub fn find_canonical_profile_id(&self, agent: CanonicalAgent) -> Option<String> {
        self.document_store
            .draft
            .document
            .editable
            .graph
            .profiles
            .iter()
            .find(|(_, p)| {
                p.name != mi_model::preset::LOCAL_CODEX_PROFILE_NAME
                    && CanonicalAgent::from_profile_name(&p.name) == Some(agent)
            })
            .map(|(pid, _)| pid.clone())
    }

    pub(super) fn local_codex_profile_id(&self) -> Option<String> {
        self.document_store
            .draft
            .document
            .editable
            .graph
            .profiles
            .iter()
            .find(|(_, p)| p.name == mi_model::preset::LOCAL_CODEX_PROFILE_NAME)
            .map(|(pid, _)| pid.clone())
    }

    pub(super) fn default_host_profile_id(&self, agent: CanonicalAgent) -> Option<String> {
        let profiles = &self
            .document_store
            .host_store()
            .draft
            .document
            .editable
            .graph
            .profiles;
        let canonical_match = || {
            profiles
                .iter()
                .find(|(_, p)| {
                    p.name != mi_model::preset::LOCAL_CODEX_PROFILE_NAME
                        && CanonicalAgent::from_profile_name(&p.name) == Some(agent)
                })
                .map(|(pid, _)| pid.clone())
        };
        if agent == CanonicalAgent::Codex {
            profiles
                .iter()
                .find(|(_, p)| p.name == mi_model::preset::LOCAL_CODEX_PROFILE_NAME)
                .map(|(pid, _)| pid.clone())
                .or_else(canonical_match)
        } else {
            canonical_match()
        }
    }

    pub fn is_selected_layer_protected(&self) -> bool {
        self.projection_inputs().is_selected_layer_protected()
    }

    pub fn is_active_lighting_protected(&self) -> bool {
        self.projection_inputs().is_active_lighting_protected()
    }

    pub fn ensure_layer_editable(&self) -> Result<(), WorkbenchError> {
        if self.is_selected_layer_protected() {
            return Err(WorkbenchError::ProtectedLayerLocked);
        }
        Ok(())
    }

    pub fn dispatch_canvas_action(&mut self, action: CanvasAction) -> Result<(), WorkbenchError> {
        match action {
            CanvasAction::SelectSurface(mut surface) => {
                let layout = self.current_bottom_row_layout();
                if surface == ControlSurfaceId::Key11 && layout == BottomRowLayout::Merged {
                    surface = ControlSurfaceId::Key10;
                }
                self.selected_surface = surface;
                let slot = self.effective_slot_for_surface(surface);
                self.selected_binding_slot = Some(slot);
                self.commit_runtime_update();
                Ok(())
            }
            CanvasAction::SelectLogicalSlot(slot) => {
                let layout = self.current_bottom_row_layout();
                if let Some(surface) = ControlSurfaceId::from_logical_slot(slot, layout) {
                    self.selected_surface = surface;
                    self.selected_binding_slot = Some(slot);
                    if (16..=19).contains(&slot)
                        && self.selected_joystick_direction.logical_sector_slot() != slot
                    {
                        self.selected_joystick_direction = match slot {
                            16 => JoystickDirection8::North,
                            17 => JoystickDirection8::West,
                            18 => JoystickDirection8::South,
                            _ => JoystickDirection8::East,
                        };
                    }
                    self.commit_runtime_update();
                }
                Ok(())
            }
            CanvasAction::SelectJoystickDirection(dir) => {
                self.selected_surface = ControlSurfaceId::Joystick;
                self.selected_joystick_direction = dir;
                self.selected_binding_slot = Some(dir.logical_sector_slot());
                self.commit_runtime_update();
                Ok(())
            }
            CanvasAction::ClickCanvasPoint {
                x_milli_px,
                y_milli_px,
            } => {
                let px_x = x_milli_px as f32 / 1000.0;
                let px_y = y_milli_px as f32 / 1000.0;
                if let Some(dir) = self
                    .projection
                    .canvas
                    .hit_test_joystick_direction(px_x, px_y)
                {
                    self.selected_surface = ControlSurfaceId::Joystick;
                    self.selected_joystick_direction = dir;
                    self.selected_binding_slot = Some(dir.logical_sector_slot());
                    self.commit_runtime_update();
                    return Ok(());
                }
                if let Some(surface) = self.projection.canvas.hit_test_point(px_x, px_y) {
                    self.selected_surface = surface;
                    let slot = self.effective_slot_for_surface(surface);
                    self.selected_binding_slot = Some(slot);
                    self.commit_runtime_update();
                }
                Ok(())
            }
            CanvasAction::SelectLayer(layer_id) => {
                if self
                    .document_store
                    .draft
                    .document
                    .editable
                    .graph
                    .layers
                    .contains_key(&layer_id)
                {
                    let layer_changed =
                        self.selected_layer_id.as_deref() != Some(layer_id.as_str());
                    if layer_changed {
                        self.cancel_encoder_model_picker();
                    }
                    self.selected_layer_id = Some(layer_id);
                    self.v070_hardware.device_layer_override = None;
                    self.normalize_surface_for_current_layer();
                    if layer_changed {
                        self.sync_lighting_preview_after_document_change();
                    }
                    self.refresh_four_app_lighting();
                    self.commit_runtime_update();
                }
                Ok(())
            }
            CanvasAction::SetBottomRowLayout(layout) => {
                self.ensure_layer_editable()?;
                let _ = self.apply_routing_operation(RoutingOperation::BottomRowLayout { layout });
                self.bottom_row_layout = layout;
                self.merge_wide_key
                    .store(layout.is_merged(), Ordering::SeqCst);
                self.normalize_surface_for_current_layer();
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
                Ok(())
            }
            CanvasAction::MergeBottomRowAcrossLayers => {
                self.ensure_layer_editable()?;
                let pid = self
                    .selected_profile_id
                    .clone()
                    .ok_or_else(|| WorkbenchError::Storage("未选择方案".to_string()))?;
                self.document_store
                    .update(|draft| {
                        draft.mutate(|doc| {
                            doc.merge_keys_across_layers(&pid, 10, 11, None)?;
                            Ok(())
                        })
                    })
                    .map_err(WorkbenchError::Storage)?;
                let _ = self.apply_routing_operation(RoutingOperation::BottomRowLayout {
                    layout: BottomRowLayout::Merged,
                });
                self.bottom_row_layout = BottomRowLayout::Merged;
                self.merge_wide_key.store(true, Ordering::SeqCst);
                self.normalize_surface_for_current_layer();
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
                Ok(())
            }
            CanvasAction::SetShowKeycapIcons(show) => {
                self.show_keycap_icons = show;
                self.commit_runtime_update();
                Ok(())
            }
            CanvasAction::SetShowKeycapLabels(show) => {
                self.show_keycap_labels = show;
                self.commit_runtime_update();
                Ok(())
            }
            CanvasAction::ApplyKnobJoystickPreset(preset) => {
                self.ensure_layer_editable()?;
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                if !preset.is_universal() {
                    if let Some(profile) = self
                        .document_store
                        .draft
                        .document
                        .editable
                        .graph
                        .profiles
                        .get(&pid)
                    {
                        let agent = CanonicalAgent::from_profile_name(&profile.name);
                        if agent == Some(CanonicalAgent::Codex) && !preset.is_codex() {
                            return Err(WorkbenchError::Preset(
                                "Codex 方案不支持 Antigravity 旋钮与摇杆预设".into(),
                            ));
                        }
                        if agent == Some(CanonicalAgent::Antigravity) && preset.is_codex() {
                            return Err(WorkbenchError::Preset(
                                "Antigravity 方案不支持 Codex 旋钮预设".into(),
                            ));
                        }
                    }
                }
                let pairs: &[(u32, &str)] = match preset {
                    KnobJoystickPreset::ModelSelectionAndMouse => &[
                        (13, "agent-model-picker-prev"),
                        (14, "agent-model-picker-next"),
                        (15, "agent-search"),
                        (16, "agent-model-picker-prev"),
                        (17, "agent-model-picker-effort-left"),
                        (18, "agent-model-picker-next"),
                        (19, "agent-model-picker-effort-right"),
                    ],
                    KnobJoystickPreset::CycleWorkingAndBlueDot => &[
                        (13, "ag-cycle-attention-prev"),
                        (14, "ag-cycle-attention-next"),
                        (15, "four-app-attention"),
                        (16, "ag-cycle-working-prev"),
                        (17, "ag-cycle-completed-prev"),
                        (18, "ag-cycle-working-next"),
                        (19, "ag-cycle-completed-next"),
                    ],
                    KnobJoystickPreset::DualScrollProjectsAndChat => &[
                        (13, "ag-sidebar-scroll-up"),
                        (14, "ag-sidebar-scroll-down"),
                        (15, "agent-submit"),
                        (16, "ag-prev-conversation"),
                        (17, "ag-prev-project"),
                        (18, "ag-next-conversation"),
                        (19, "ag-next-project"),
                    ],
                    KnobJoystickPreset::CodexScrollChatAndBottom => &[
                        (13, "codex-scroll-up"),
                        (14, "codex-scroll-down"),
                        (15, "codex-scroll-to-bottom"),
                    ],
                    KnobJoystickPreset::CodexReasoningEffortAndSpeed => &[
                        (13, "codex-effort-down"),
                        (14, "codex-effort-up"),
                        (15, "codex-toggle-fast-mode"),
                    ],
                    KnobJoystickPreset::MousePointerAndScroll => &[
                        (13, "host-mouse-scroll-up"),
                        (14, "host-mouse-scroll-down"),
                        (15, "host-mouse-speed-cycle"),
                        (16, "host-mouse-move-up"),
                        (17, "host-mouse-move-left"),
                        (18, "host-mouse-move-down"),
                        (19, "host-mouse-move-right"),
                    ],
                };
                let v070_supported = self.v070_hardware.projection.supported;
                let catalog = mi_model::draft::actions();
                self.document_store
                    .update(|draft| {
                        match preset {
                            KnobJoystickPreset::ModelSelectionAndMouse => {
                                draft.set_control_settings(
                                    &pid,
                                    &lid,
                                    mi_model::control_settings::ControlSettings {
                                        joystick_mode: "mouse".into(),
                                        active_sectors: 0,
                                        encoder_hold_action_id: None,
                                    },
                                )?;
                                let search_action = catalog
                                    .iter()
                                    .find(|a| a.id == "agent-search")
                                    .cloned()
                                    .ok_or_else(|| "未找到内置动作 agent-search".to_string())?;
                                draft.assign_encoder_hold(
                                    &json!(pid),
                                    &json!(lid),
                                    search_action,
                                )?;
                            }
                            KnobJoystickPreset::MousePointerAndScroll => {
                                draft.unassign_encoder_hold(&json!(pid), &json!(lid))?;
                                let (joystick_mode, active_sectors) = if v070_supported {
                                    ("mouse", 0)
                                } else {
                                    ("logical_sectors", 4)
                                };
                                draft.set_control_settings(
                                    &pid,
                                    &lid,
                                    mi_model::control_settings::ControlSettings {
                                        joystick_mode: joystick_mode.into(),
                                        active_sectors,
                                        encoder_hold_action_id: None,
                                    },
                                )?;
                            }
                            _ => {
                                draft.unassign_encoder_hold(&json!(pid), &json!(lid))?;
                            }
                        }
                        for &(slot, action_id) in pairs {
                            let action = catalog
                                .iter()
                                .find(|a| a.id == action_id)
                                .cloned()
                                .ok_or_else(|| format!("未找到内置动作 {action_id}"))?;
                            draft.assign(&json!(pid), &json!(lid), slot, action)?;
                        }
                        Ok(())
                    })
                    .map_err(WorkbenchError::Storage)?;
                self.sync_v070_session_sa_rgb();
                self.commit_runtime_update();
                Ok(())
            }
            CanvasAction::AssignSlotAction { slot, action_id } => {
                self.ensure_layer_editable()?;
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                let action = self.resolve_action_by_id(&action_id)?;
                self.document_store
                    .update(|draft| draft.assign(&json!(pid), &json!(lid), slot, action))
                    .map_err(WorkbenchError::Storage)?;
                self.sync_v070_session_sa_rgb();
                self.commit_runtime_update();
                Ok(())
            }
            CanvasAction::UnlockBySwitchingOrCreatingCustomLayer => {
                let pid = self
                    .selected_profile_id
                    .clone()
                    .ok_or_else(|| WorkbenchError::Storage("未选择方案".to_string()))?;
                let existing_editable = self
                    .document_store
                    .draft
                    .document
                    .editable
                    .graph
                    .profiles
                    .get(&pid)
                    .and_then(|p| {
                        p.layer_order
                            .iter()
                            .find(|lid| {
                                !self
                                    .document_store
                                    .draft
                                    .document
                                    .is_protected_oai_layer(lid)
                            })
                            .cloned()
                    });
                let target_lid = if let Some(lid) = existing_editable {
                    lid
                } else {
                    let mut created_lid = String::new();
                    self.document_store
                        .update(|draft| {
                            created_lid = draft.create_layer(&pid, "自定义层")?;
                            Ok(())
                        })
                        .map_err(WorkbenchError::Storage)?;
                    created_lid
                };
                self.selected_layer_id = Some(target_lid);
                self.v070_hardware.device_layer_override = None;
                self.normalize_surface_for_current_layer();
                self.sync_lighting_preview_after_document_change();
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
                Ok(())
            }
        }
    }

    pub fn dispatch_session_strip_action(
        &mut self,
        action: SessionStripAction,
    ) -> Result<(), WorkbenchError> {
        self.require_awake()?;
        match action {
            SessionStripAction::ActivateSlot(slot_index) => {
                self.request_navigation_command(
                    NavigationCommand::Slot(slot_index),
                    None,
                    None,
                    "workbench".into(),
                    "session-strip".into(),
                );
                Ok(())
            }
            SessionStripAction::TogglePinSlot(slot_index) => {
                if slot_index >= SLOT_COUNT {
                    return Ok(());
                }
                let slot = self.active_slot_table().slot_table().slots()[slot_index].clone();
                let acks = self.lighting_arbiter.acknowledged_watermarks.clone();
                if slot.mode == mi_model::slot_table::SlotMode::Pinned {
                    self.active_slot_table_mut()
                        .unpin_slot(slot_index, &acks)
                        .map_err(WorkbenchError::Storage)?;
                } else if let Some(place) = slot.place {
                    self.active_slot_table_mut()
                        .pin_slot(
                            slot_index,
                            place,
                            slot.title,
                            slot.updated_at.unwrap_or(0),
                            &acks,
                        )
                        .map_err(WorkbenchError::Storage)?;
                }
                self.refresh_navigation_revision();
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
                Ok(())
            }
            SessionStripAction::JumpToSession(place) => {
                self.request_session_navigation(place);
                Ok(())
            }
            SessionStripAction::AcknowledgeAllCompleted => {
                let to_ack: Vec<_> = self
                    .slot_table
                    .candidates()
                    .iter()
                    .chain(self.codex_slot_table.candidates().iter())
                    .filter(|c| c.has_unviewed_completion_timestamp())
                    .map(|c| (c.place.clone(), c.watermark.clone()))
                    .collect();
                for (place, wm) in to_ack {
                    self.lighting_arbiter.acknowledge_slot(&place, wm);
                }
                let acks = self.lighting_arbiter.acknowledged_watermarks.clone();
                self.slot_table.refresh_with_watermarks(&acks);
                self.codex_slot_table.refresh_with_watermarks(&acks);
                self.refresh_navigation_revision();
                self.lighting_popover.manual_preview_phase = None;
                self.lighting_arbiter.manual_preview_phase = None;
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
                Ok(())
            }
            SessionStripAction::BindTopSixAsSessionKeys => {
                self.ensure_layer_editable()?;
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                let catalog = mi_model::draft::actions();
                self.document_store
                    .update(|draft| {
                        for slot in 0..6u32 {
                            let target_id = format!("slot-open-{slot}");
                            let action = catalog
                                .iter()
                                .find(|a| a.id == target_id)
                                .cloned()
                                .ok_or_else(|| format!("未找到动作 {target_id}"))?;
                            draft.assign(&json!(pid), &json!(lid), slot, action)?;
                        }
                        Ok(())
                    })
                    .map_err(WorkbenchError::Storage)?;
                for slot in 0..6u32 {
                    self.slot_role_overrides.remove(&(pid.clone(), slot));
                    self.keycap_overrides.remove(&(pid.clone(), slot));
                }
                self.sync_v070_session_sa_rgb();
                self.commit_runtime_update();
                Ok(())
            }
            SessionStripAction::RestoreDefaultLayout => {
                self.ensure_layer_editable()?;
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                let profile_name = self
                    .document_store
                    .draft
                    .document
                    .editable
                    .graph
                    .profiles
                    .get(&pid)
                    .map(|p| p.name.clone())
                    .unwrap_or_default();

                let mut template = Draft::default();
                mi_model::preset::install_agent_starter_profiles(&mut template)
                    .map_err(WorkbenchError::Preset)?;

                let template_layer_and_actions = template
                    .document
                    .editable
                    .graph
                    .profiles
                    .values()
                    .find(|p| p.name == profile_name)
                    .and_then(|p| p.layer_order.first())
                    .and_then(|tlid| {
                        template
                            .document
                            .editable
                            .graph
                            .layers
                            .get(tlid)
                            .cloned()
                            .map(|layer| (layer, template.document.editable.graph.actions.clone()))
                    });

                self.document_store
                    .update(|draft| {
                        if let Some((tlayer, tactions)) = template_layer_and_actions {
                            for slot in 0..20u32 {
                                let logical = format!("logical-{slot}");
                                let maybe_action = tlayer
                                    .controls
                                    .values()
                                    .find(|b| b.logical_control == logical)
                                    .and_then(|b| b.action_id.as_ref())
                                    .and_then(|aid| tactions.get(aid))
                                    .cloned();
                                if let Some(action) = maybe_action {
                                    draft.assign(&json!(pid), &json!(lid), slot, action)?;
                                } else {
                                    draft.unassign(&json!(pid), &json!(lid), slot)?;
                                }
                            }
                            if let Some(cs) = &tlayer.control_settings {
                                draft.set_control_settings(
                                    &pid,
                                    &lid,
                                    mi_model::control_settings::ControlSettings {
                                        joystick_mode: cs.joystick_mode.clone(),
                                        active_sectors: cs.active_sectors,
                                        encoder_hold_action_id: None,
                                    },
                                )?;
                                if let Some(hold_aid) = cs.encoder_hold_action_id.as_ref() {
                                    if let Some(hold_action) = tactions.get(hold_aid).cloned() {
                                        draft.assign_encoder_hold(
                                            &json!(pid),
                                            &json!(lid),
                                            hold_action,
                                        )?;
                                    } else {
                                        draft.unassign_encoder_hold(&json!(pid), &json!(lid))?;
                                    }
                                } else {
                                    draft.unassign_encoder_hold(&json!(pid), &json!(lid))?;
                                }
                            } else {
                                draft.unassign_encoder_hold(&json!(pid), &json!(lid))?;
                            }
                        } else {
                            mi_model::preset::install_agent_starter_profiles(draft)?;
                        }
                        Ok(())
                    })
                    .map_err(WorkbenchError::Storage)?;
                let _ = self.apply_routing_operation(RoutingOperation::BottomRowLayout {
                    layout: BottomRowLayout::Split,
                });
                self.slot_role_overrides.retain(|(p, _), _| p != &pid);
                self.keycap_overrides.retain(|(p, _), _| p != &pid);
                self.bottom_row_layout = BottomRowLayout::Split;
                self.merge_wide_key.store(false, Ordering::SeqCst);
                self.normalize_surface_for_current_layer();
                self.sync_v070_session_sa_rgb();
                self.commit_runtime_update();
                Ok(())
            }
            SessionStripAction::ToggleLightingPopover => {
                self.lighting_popover.is_open = !self.lighting_popover.is_open;
                self.commit_runtime_update();
                Ok(())
            }
            SessionStripAction::SetLightingEnabled(enabled) => {
                self.lighting_popover.lighting_enabled = enabled;
                self.lighting_arbiter.lighting_enabled = enabled;
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
                Ok(())
            }
            SessionStripAction::SetAutoSessionColor(auto_col) => {
                self.lighting_popover.auto_session_color = auto_col;
                self.lighting_arbiter.session_lighting_linked = auto_col;
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
                Ok(())
            }
            SessionStripAction::SetOnlyGlowOnAttentionOrCompleted(only_done) => {
                self.lighting_popover.only_glow_on_attention_or_completed = only_done;
                self.lighting_arbiter.only_glow_on_attention_or_completed = only_done;
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
                Ok(())
            }
            SessionStripAction::SetGlowOnWorking(glow_working) => {
                self.lighting_popover.glow_on_working = glow_working;
                self.lighting_arbiter.glow_on_working_sessions = glow_working;
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
                Ok(())
            }
            SessionStripAction::SetAmbientColor(preset) => {
                self.lighting_popover.ambient_color = preset;
                self.lighting_arbiter.idle_color_hex = preset.hex().to_string();
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
                Ok(())
            }
            SessionStripAction::SetWorkingColor(preset) => {
                self.lighting_popover.working_color = preset;
                self.lighting_arbiter.working_color_hex = preset.hex().to_string();
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
                Ok(())
            }
            SessionStripAction::SetBrightnessPreset(preset) => {
                self.lighting_popover.brightness = preset;
                self.lighting_arbiter.brightness_scale = preset.scale();
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
                Ok(())
            }
            SessionStripAction::SetAutoOffTimeout(preset) => {
                self.lighting_popover.auto_off_timeout = preset;
                self.commit_runtime_update();
                Ok(())
            }
            SessionStripAction::TriggerTestLighting(phase) => {
                if self.is_active_lighting_protected() {
                    return Err(WorkbenchError::ProtectedLayerLocked);
                }
                self.lighting_popover.manual_preview_phase = phase;
                self.lighting_arbiter.manual_preview_phase = phase;
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
                Ok(())
            }
        }
    }

    pub fn ingest_candidates_for_source(
        &mut self,
        source: LiveSessionSource,
        candidates: Vec<FourAppLiveCandidate>,
    ) {
        let acks = self.lighting_arbiter.acknowledged_watermarks.clone();
        self.slot_table_for_source_mut(source)
            .update_candidates(candidates, &acks);
        self.refresh_navigation_revision();
        self.refresh_four_app_lighting();
        self.commit_runtime_update();
    }

    pub fn ingest_four_app_candidates(&mut self, candidates: Vec<FourAppLiveCandidate>) {
        let default_source = self.desired_live_session_source();
        if candidates.is_empty() {
            self.ingest_candidates_for_source(default_source, candidates);
            return;
        }
        let source_for = |place: &NavigationPlace| match place {
            NavigationPlace::Codex { .. } => LiveSessionSource::Codex,
            NavigationPlace::Antigravity { .. } => LiveSessionSource::Antigravity,
            _ => default_source,
        };
        let all_codex = candidates
            .iter()
            .all(|c| source_for(&c.place) == LiveSessionSource::Codex);
        let none_codex = candidates
            .iter()
            .all(|c| source_for(&c.place) != LiveSessionSource::Codex);
        if all_codex {
            self.ingest_candidates_for_source(LiveSessionSource::Codex, candidates);
        } else if none_codex {
            self.ingest_candidates_for_source(LiveSessionSource::Antigravity, candidates);
        } else {
            let (codex_cands, ag_cands): (Vec<_>, Vec<_>) = candidates
                .into_iter()
                .partition(|c| source_for(&c.place) == LiveSessionSource::Codex);
            let acks = self.lighting_arbiter.acknowledged_watermarks.clone();
            self.codex_slot_table.update_candidates(codex_cands, &acks);
            self.slot_table.update_candidates(ag_cands, &acks);
            self.refresh_navigation_revision();
            self.refresh_four_app_lighting();
            self.commit_runtime_update();
        }
    }

    pub(crate) fn six_layer_slot_snapshots(
        &self,
        acks: &BTreeMap<String, String>,
    ) -> (
        [crate::lighting_arbiter::FourAppSlotSnapshot; SLOT_COUNT],
        [crate::lighting_arbiter::FourAppSlotSnapshot; SLOT_COUNT],
        [crate::lighting_arbiter::FourAppSlotSnapshot; SLOT_COUNT],
        u8,
    ) {
        let empty_snapshot = || crate::lighting_arbiter::FourAppSlotSnapshot {
            app: String::new(),
            title: String::new(),
            status: "idle".to_string(),
            place: None,
            watermark: String::new(),
        };
        let preferred_layer = self
            .six_layer_running
            .as_ref()
            .map(|r| r.observed_layer.load(Ordering::Acquire))
            .filter(|l| (1..=3).contains(l))
            .unwrap_or(2);
        let Some(plan) = self
            .six_layer_running
            .as_ref()
            .map(|r| &r.installation.plan)
            .or_else(|| self.six_layer_state.installation.as_ref().map(|i| &i.plan))
        else {
            return (
                std::array::from_fn(|_| empty_snapshot()),
                std::array::from_fn(|_| empty_snapshot()),
                std::array::from_fn(|_| empty_snapshot()),
                preferred_layer,
            );
        };
        let ctx = crate::six_layer_runtime::RoutingContext {
            mode: self.build_six_layer_routing_mode(),
            bundle_id: self.foreground_bundle_id.clone(),
        };
        let ag_snapshots = self.slot_table.lighting_snapshots(acks);
        let codex_snapshots = self.codex_slot_table.lighting_snapshots(acks);
        let snapshots_for_layer = |control_layer: u8| {
            std::array::from_fn(|slot_idx| {
                let Some((profile_id, action)) = ctx.resolve_control(plan, control_layer, slot_idx)
                else {
                    return empty_snapshot();
                };
                let ResolvedHostAction::AgentCommand { adapter, command } = action else {
                    return empty_snapshot();
                };
                let Some(target_idx) = command
                    .strip_prefix("slot-open:")
                    .and_then(|s| s.parse::<usize>().ok())
                    .filter(|&idx| idx < SLOT_COUNT)
                else {
                    return empty_snapshot();
                };
                let source = if adapter == "codex" {
                    LiveSessionSource::Codex
                } else {
                    self.source_for_profile_id(profile_id)
                };
                match source {
                    LiveSessionSource::Codex => codex_snapshots[target_idx].clone(),
                    LiveSessionSource::Antigravity => ag_snapshots[target_idx].clone(),
                }
            })
        };
        (
            snapshots_for_layer(1),
            snapshots_for_layer(2),
            snapshots_for_layer(3),
            preferred_layer,
        )
    }

    pub fn refresh_four_app_lighting(&mut self) {
        let acks = self.lighting_arbiter.acknowledged_watermarks.clone();
        let snapshots = self.active_slot_table().lighting_snapshots(&acks);
        let live_phase = self.derive_live_lighting_phase(&acks);
        let is_protected = self.is_active_lighting_protected();
        let preview_busy = self.is_lighting_preview_busy();
        let is_split = self.current_bottom_row_layout() == BottomRowLayout::Split;
        let six_layer_mode =
            self.six_layer_state.installation.is_some() || self.six_layer_running.is_some();
        if six_layer_mode {
            let result = self.lighting_arbiter.recompute_feedback(
                live_phase,
                &snapshots,
                false,
                is_protected,
                preview_busy,
                is_split,
            );
            if result == LightingUpdateResult::DeviceBusy {
                self.lighting_arbiter.invalidate_hardware_cache();
            } else {
                let (l1_slots, l2_slots, l3_slots, preferred_layer) =
                    self.six_layer_slot_snapshots(&acks);
                let running = &self.six_layer_running;
                let _ = self.lighting_arbiter.sync_six_layer_hardware_for_layers(
                    &l1_slots,
                    &l2_slots,
                    &l3_slots,
                    preferred_layer,
                    |digest, entries| {
                        Ok(running
                            .as_ref()
                            .and_then(|r| r.queue_session_lighting(digest, entries)))
                    },
                );
            }
            self.sync_v070_session_sa_rgb();
            return;
        }
        let verified_session = self
            .device_session
            .as_ref()
            .filter(|_| self.hardware_rpc_verified)
            .filter(|s| !s.is_disconnected())
            .cloned();
        let connected_device_id = verified_session
            .as_ref()
            .and_then(|_| self.connected_candidate.as_ref().map(|c| c.path.clone()));
        let runtime_for_lighting = Arc::clone(&self.tokio_runtime);
        let result = self.lighting_arbiter.update_feedback(
            live_phase,
            &snapshots,
            false,
            is_protected,
            preview_busy,
            is_split,
            connected_device_id.as_deref(),
            |payload_str| {
                let Some(session) = verified_session.clone() else {
                    return Err("no_verified_device_session".to_string());
                };
                let (preview, entries) = parse_hardware_lighting_payload(payload_str)?;
                runtime_for_lighting.spawn(async move {
                    let _ = sync_hardware_lighting_to_session(&session, &preview, &entries).await;
                });
                Ok("{\"synced\":true}".to_string())
            },
        );
        if result == LightingUpdateResult::DeviceBusy {
            self.lighting_arbiter.invalidate_hardware_cache();
        }
        self.sync_v070_session_sa_rgb();
    }

    fn derive_live_lighting_phase(&self, acks: &BTreeMap<String, String>) -> FourAppAgentPhase {
        self.projection_inputs().derive_live_lighting_phase(acks)
    }

    pub fn set_hosts(
        &mut self,
        panel_host: Arc<dyn PassivePanelHost>,
        status_item_host: Arc<dyn StatusItemHost>,
    ) {
        self.panel_host = panel_host;
        self.status_item_host = status_item_host;
        self.last_status_item_snapshot = None;
        self.last_hud_panel = None;
        self.last_radial_panel = None;
        self.commit_runtime_update();
    }

    pub fn update_screen_context(
        &mut self,
        screen: ScreenGeometry,
        cursor: ScreenPoint,
        main_window_active: bool,
        now: Instant,
    ) {
        self.update_now = now;
        self.screen_geometry = screen;
        self.last_cursor_position = cursor;
        self.overlay_controller
            .set_main_workbench_window_active(main_window_active);
        self.overlay_controller.expire_notice_if_due(now);
        self.commit_runtime_update();
    }

    pub fn set_main_workbench_window_active(&mut self, active: bool) {
        let previous_visibility = self.overlay_controller.visibility().clone();
        self.overlay_controller
            .set_main_workbench_window_active(active);
        if self.overlay_controller.visibility() != &previous_visibility {
            self.commit_runtime_update();
        } else {
            self.commit_local_update();
        }
    }

    pub fn expire_overlay_if_due(&mut self, now: Instant) -> bool {
        self.update_now = now;
        let expired = self.overlay_controller.expire_notice_if_due(now);
        if expired {
            self.commit_runtime_update();
        }
        expired
    }

    pub fn set_radial_menu_enabled(&mut self, enabled: bool) {
        self.radial_state.set_enabled(enabled);
        self.commit_runtime_update();
    }

    pub fn set_radial_confirm_mode(&mut self, mode: RadialConfirmMode) {
        self.radial_state.set_confirm_mode(mode);
        self.commit_runtime_update();
    }

    pub fn set_radial_input_readiness(&mut self, readiness: RadialInputReadiness) {
        self.radial_readiness = readiness;
        if readiness != RadialInputReadiness::Ready {
            self.radial_state.cancel();
        }
        self.commit_runtime_update();
    }

    pub fn apply_radial_preset(&mut self, preset: RadialPresetKind) {
        self.radial_slot_configs = RadialSlotConfig::for_preset(preset);
        self.radial_state.cancel();
        self.commit_runtime_update();
    }

    pub fn set_radial_slot_configs(&mut self, configs: [RadialSlotConfig; RADIAL_SECTOR_COUNT]) {
        self.radial_slot_configs = configs;
        self.radial_state.cancel();
        self.commit_runtime_update();
    }

    pub fn set_radial_slot_config(&mut self, sector: SectorIndex, config: RadialSlotConfig) {
        self.radial_slot_configs[sector.as_usize()] = config;
        self.radial_state.cancel();
        self.commit_runtime_update();
    }

    pub fn resolve_current_radial_slots(&self) -> [RadialSlot; RADIAL_SECTOR_COUNT] {
        self.projection_inputs().resolve_current_radial_slots()
    }

    fn execute_radial_intent(
        &mut self,
        session: &RadialMenuSession,
        intent: &RadialIntent,
    ) -> HostActionReceipt {
        let executor = Arc::clone(&self.live_executor);
        let isolated = self.config.isolated;
        self.execute_radial_intent_using(session, intent, move |profile, action| {
            if isolated {
                mi_macos::SimulatedHostActionExecutor::new().execute(profile, "radial-menu", action)
            } else {
                executor.execute(profile, "radial-menu", action)
            }
        })
    }

    pub(super) fn execute_radial_intent_using(
        &mut self,
        session: &RadialMenuSession,
        intent: &RadialIntent,
        execute: impl FnOnce(&str, &ResolvedHostAction) -> HostActionReceipt,
    ) -> HostActionReceipt {
        let profile_id = session.route.profile_id.clone().unwrap_or_default();
        let local_receipt = |outcome: &str, detail: &str| HostActionReceipt {
            action_type: "cheat_sheet".into(),
            binding_id: "radial-menu".into(),
            detail: detail.into(),
            outcome: outcome.into(),
            profile_id: profile_id.clone(),
            target: "micro-input".into(),
            text_byte_len: None,
            timestamp_ms: 0,
        };
        let previous_history = self.navigation_history.clone();
        let action = match intent {
            RadialIntent::CheatSheet => {
                let sheet = self.build_cheat_sheet_projection();
                self.overlay_controller.show_cheat_sheet(
                    sheet,
                    &self.screen_geometry,
                    self.last_cursor_position,
                );
                let receipt = local_receipt("executed", "已显示当前宿主配置的按键速查");
                self.update_host_caution_from_receipt(&receipt, self.update_now);
                self.record_host_action_receipt(receipt.clone());
                return receipt;
            }
            RadialIntent::OpenApp { bundle_id } => ResolvedHostAction::OpenApp {
                bundle_id: bundle_id.clone(),
            },
            RadialIntent::Prompt {
                text,
                adapter,
                submit,
            } => ResolvedHostAction::AgentCommand {
                adapter: adapter
                    .clone()
                    .or_else(|| self.active_desktop_agent_adapter())
                    .unwrap_or_else(|| "host".into()),
                command: format!(
                    "{}:{text}",
                    if *submit { "submit-prompt" } else { "prompt" }
                ),
            },
            RadialIntent::Submit { adapter } | RadialIntent::Stop { adapter } => {
                ResolvedHostAction::AgentCommand {
                    adapter: adapter
                        .clone()
                        .or_else(|| self.active_desktop_agent_adapter())
                        .unwrap_or_else(|| "host".into()),
                    command: if matches!(intent, RadialIntent::Submit { .. }) {
                        "submit"
                    } else {
                        "deny"
                    }
                    .into(),
                }
            }
            RadialIntent::NavigateBack | RadialIntent::NavigateForward => {
                let applications = &self.applications;
                let available = |place: &NavigationPlace| {
                    applications
                        .get(bundle_id_for_place(place))
                        .copied()
                        .unwrap_or(false)
                };
                let entry = if matches!(intent, RadialIntent::NavigateBack) {
                    self.navigation_history.back(available)
                } else {
                    self.navigation_history.forward(available)
                };
                let Some(entry) = entry else {
                    let mut receipt = local_receipt("blocked", "没有可打开的历史目标");
                    receipt.action_type = "open_app".into();
                    receipt.target = "navigation".into();
                    self.update_host_caution_from_receipt(&receipt, self.update_now);
                    self.record_host_action_receipt(receipt.clone());
                    return receipt;
                };
                ResolvedHostAction::OpenApp {
                    bundle_id: bundle_id_for_place(&entry).into(),
                }
            }
        };
        let mut receipt = execute(&profile_id, &action);
        if matches!(intent, RadialIntent::Prompt { .. }) {
            receipt.action_type = "paste_text".into();
        }
        if !receipt.succeeded() {
            self.navigation_history = previous_history;
        }
        self.update_host_caution_from_receipt(&receipt, self.update_now);
        self.record_host_action_receipt(receipt.clone());
        receipt
    }

    pub fn consume_radial_input(
        &mut self,
        event: RadialInputEvent,
        now: Instant,
    ) -> RadialConsumeOutcome {
        self.update_now = now;
        if self.system_lifecycle != SystemLifecycle::Awake || !self.radial_state.enabled() {
            return RadialConsumeOutcome {
                consumed: false,
                confirmed_intent: None,
                dispatched_receipt: None,
            };
        }

        if let RadialInputEvent::EscapeKeyDown { key_code } = event {
            if key_code == ESCAPE_KEY_CODE && self.radial_state.session().is_some() {
                self.radial_state.cancel();
                self.commit_runtime_update();
                return RadialConsumeOutcome {
                    consumed: true,
                    confirmed_intent: None,
                    dispatched_receipt: None,
                };
            }
            return RadialConsumeOutcome {
                consumed: false,
                confirmed_intent: None,
                dispatched_receipt: None,
            };
        }

        let is_position = matches!(event, RadialInputEvent::Position { .. });
        let is_enc_clk = matches!(
            &event,
            RadialInputEvent::KeyPress { binding_id, .. } if binding_id == "ENC_CLK"
        );
        let captured_confirm = self.radial_state.captures_confirm() && is_enc_clk;

        let gates_ok = self.system_lifecycle == SystemLifecycle::Awake
            && self.document_store.host_execution_enabled()
            && !self.ptt_controller.is_active
            && self.ptt_controller.cleanup_confirmed
            && !self.live_executor.has_unconfirmed_apple_fn_release()
            && self.radial_readiness == RadialInputReadiness::Ready
            && self.resolved_active_profile_id().is_some();

        if !gates_ok {
            self.radial_state.cancel();
            self.commit_runtime_update();
            return RadialConsumeOutcome {
                consumed: is_position || captured_confirm,
                confirmed_intent: None,
                dispatched_receipt: None,
            };
        }

        let current_route = self.current_radial_route_stamp();
        let event_session_id = match &event {
            RadialInputEvent::Position { session_id, .. }
            | RadialInputEvent::KeyPress { session_id, .. } => *session_id,
            RadialInputEvent::EscapeKeyDown { .. } => 0,
        };

        if let Some(active_session) = self.radial_state.session() {
            if active_session.input_session != event_session_id
                || active_session.route != current_route
            {
                self.radial_state.cancel();
                self.commit_runtime_update();
                if captured_confirm {
                    return RadialConsumeOutcome {
                        consumed: true,
                        confirmed_intent: None,
                        dispatched_receipt: None,
                    };
                }
            }
        }

        match event {
            RadialInputEvent::Position {
                session_id,
                sample,
                admitted,
                ..
            } => {
                if !admitted {
                    self.radial_state.cancel();
                    self.commit_runtime_update();
                    return RadialConsumeOutcome {
                        consumed: true,
                        confirmed_intent: None,
                        dispatched_receipt: None,
                    };
                }
                let slots = self.resolve_current_radial_slots();
                let anchor_frame = PassivePanelFrameSpec::for_radial(
                    self.last_cursor_position,
                    &self.screen_geometry,
                )
                .frame;
                let confirmed = self.radial_state.update(sample, now, || {
                    Some(RadialMenuSession {
                        input_session: session_id,
                        route: current_route,
                        slots,
                        selected: None,
                        anchor_frame,
                    })
                });
                if let Some((confirmed_session, intent)) = confirmed {
                    let receipt = self.execute_radial_intent(&confirmed_session, &intent);
                    self.commit_runtime_update();
                    return RadialConsumeOutcome {
                        consumed: true,
                        confirmed_intent: Some(intent),
                        dispatched_receipt: Some(receipt),
                    };
                }
                self.commit_runtime_update();
                RadialConsumeOutcome {
                    consumed: true,
                    confirmed_intent: None,
                    dispatched_receipt: None,
                }
            }
            RadialInputEvent::KeyPress {
                binding_id,
                admitted,
                ..
            } => {
                if binding_id == "ENC_CLK" && self.radial_state.captures_confirm() {
                    if admitted {
                        if let Some((confirmed_session, intent)) = self.radial_state.confirm() {
                            let receipt = self.execute_radial_intent(&confirmed_session, &intent);
                            self.commit_runtime_update();
                            return RadialConsumeOutcome {
                                consumed: true,
                                confirmed_intent: Some(intent),
                                dispatched_receipt: Some(receipt),
                            };
                        }
                    }
                    self.commit_runtime_update();
                    return RadialConsumeOutcome {
                        consumed: true,
                        confirmed_intent: None,
                        dispatched_receipt: None,
                    };
                }
                if self.radial_state.session().is_some() {
                    self.radial_state.cancel();
                    self.commit_runtime_update();
                }
                RadialConsumeOutcome {
                    consumed: false,
                    confirmed_intent: None,
                    dispatched_receipt: None,
                }
            }
            RadialInputEvent::EscapeKeyDown { .. } => RadialConsumeOutcome {
                consumed: false,
                confirmed_intent: None,
                dispatched_receipt: None,
            },
        }
    }

    pub fn dispatch_radial_overlay_action(
        &mut self,
        action: RadialOverlayAction,
    ) -> Result<Option<HostActionReceipt>, WorkbenchError> {
        self.require_awake()?;
        match action {
            RadialOverlayAction::CancelEscape => {
                self.radial_state.cancel();
                self.commit_runtime_update();
                Ok(None)
            }
            RadialOverlayAction::SelectSector(sector) => {
                self.radial_state
                    .select_sector_in_place(sector, self.update_now);
                self.commit_runtime_update();
                Ok(None)
            }
            RadialOverlayAction::ConfirmSelected => {
                let outcome = self
                    .radial_state
                    .confirm()
                    .map(|(session, intent)| self.execute_radial_intent(&session, &intent));
                self.commit_runtime_update();
                Ok(outcome)
            }
        }
    }

    pub fn dispatch_overlay_action(
        &mut self,
        action: HostOverlayAction,
    ) -> Result<(), WorkbenchError> {
        match action {
            HostOverlayAction::CloseCheatSheet => {
                self.overlay_controller.hide();
                self.commit_runtime_update();
                Ok(())
            }
            HostOverlayAction::ToggleCheatSheet => {
                let sheet = self.build_cheat_sheet_projection();
                self.overlay_controller.toggle_cheat_sheet(
                    sheet,
                    &self.screen_geometry,
                    self.last_cursor_position,
                );
                self.commit_runtime_update();
                Ok(())
            }
        }
    }

    pub fn dispatch_status_menu_action(
        &mut self,
        action: StatusMenuAction,
    ) -> Result<(), WorkbenchError> {
        match action {
            StatusMenuAction::ActivateFavoriteSlot(slot) => {
                let pid = self
                    .pinned_favorites
                    .slot(slot)
                    .map(str::to_string)
                    .ok_or_else(|| WorkbenchError::Routing("该常用格未绑定方案".to_string()))?;
                self.select_and_activate_profile_at(&pid, self.update_now, true)
            }
            StatusMenuAction::PinFavoriteSlot { slot, profile_id } => {
                if !self
                    .document_store
                    .draft
                    .document
                    .editable
                    .graph
                    .profiles
                    .contains_key(&profile_id)
                {
                    return Err(WorkbenchError::Routing("Profile 不存在".to_string()));
                }
                self.pinned_favorites.assign_or_swap(slot, profile_id);
                self.commit_runtime_update();
                Ok(())
            }
            StatusMenuAction::SelectProfile(profile_id) => {
                self.select_and_activate_profile_at(&profile_id, self.update_now, true)
            }
            StatusMenuAction::SelectCandidateDevice { selector } => {
                self.select_candidate_device_from_ui(&selector)
            }
            StatusMenuAction::SetHostExecutionEnabled(enabled) => {
                self.set_host_execution_enabled(enabled)
            }
            StatusMenuAction::SetProfileLocked(locked) => self.set_profile_locked(locked, false),
            StatusMenuAction::OpenAccessibilitySettings => {
                self.open_accessibility_settings();
                Ok(())
            }
            StatusMenuAction::RetryHostCleanup => {
                self.retry_host_cleanup();
                Ok(())
            }
            StatusMenuAction::SetRadialMenuEnabled(enabled) => {
                self.set_radial_menu_enabled(enabled);
                Ok(())
            }
            StatusMenuAction::SetRadialConfirmMode(mode) => {
                self.set_radial_confirm_mode(mode);
                Ok(())
            }
            StatusMenuAction::ApplyRadialPreset(preset) => {
                self.apply_radial_preset(preset);
                Ok(())
            }
            StatusMenuAction::ToggleCheatSheet => {
                self.dispatch_overlay_action(HostOverlayAction::ToggleCheatSheet)
            }
            StatusMenuAction::StagePresetImportBytes(bytes) => {
                let envelope = match PresetFileEnvelope::from_bytes(&bytes) {
                    Ok(env) => env,
                    Err(msg) => {
                        self.status_message = msg.clone();
                        self.preset_transfer_error = Some(msg.clone());
                        self.pending_preset_import = None;
                        self.commit_runtime_update();
                        return Err(WorkbenchError::Preset(msg));
                    }
                };
                let parsed = match mi_model::preset::import(&envelope.raw_utf8) {
                    Ok(draft) => draft,
                    Err(msg) => {
                        self.status_message = msg.clone();
                        self.preset_transfer_error = Some(msg.clone());
                        self.pending_preset_import = None;
                        self.commit_runtime_update();
                        return Err(WorkbenchError::Preset(msg));
                    }
                };
                let graph = &parsed.document.editable.graph;
                let name = graph
                    .profile_order
                    .first()
                    .and_then(|pid| graph.profiles.get(pid))
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| "预设方案".to_string());
                let scope = parsed
                    .document
                    .origin
                    .get("preset_scope")
                    .and_then(|v| v.as_str())
                    .unwrap_or("document")
                    .to_string();
                let contains_complete_archive = parsed
                    .document
                    .origin
                    .get("preset_metadata")
                    .and_then(|m| m.get("source_archive"))
                    .and_then(|v| v.as_str())
                    == Some("complete");
                self.pending_preset_import = Some(PendingPresetImportPreview {
                    raw_text: envelope.raw_utf8,
                    name,
                    scope,
                    contains_complete_archive,
                    profile_count: graph.profiles.len(),
                    layer_count: graph.layers.len(),
                    action_count: graph.actions.len(),
                });
                self.preset_transfer_error = None;
                self.commit_runtime_update();
                Ok(())
            }
            StatusMenuAction::ConfirmStagedPresetImport => {
                let Some(staged) = self.pending_preset_import.take() else {
                    let msg = "没有待确认的预设导入".to_string();
                    self.preset_transfer_error = Some(msg.clone());
                    self.commit_runtime_update();
                    return Err(WorkbenchError::Preset(msg));
                };
                self.preset_transfer_error = None;
                self.apply_backup_command(BackupCommand::ImportPreset {
                    text: staged.raw_text,
                })?;
                Ok(())
            }
            StatusMenuAction::CancelStagedPresetImport => {
                self.pending_preset_import = None;
                self.preset_transfer_error = None;
                self.commit_runtime_update();
                Ok(())
            }
            StatusMenuAction::RequestPresetExport(scope) => {
                self.pending_preset_export_scope = Some(scope);
                self.preset_transfer_error = None;
                self.commit_runtime_update();
                Ok(())
            }
            StatusMenuAction::ConfirmPresetExport => {
                let Some(scope) = self.pending_preset_export_scope.take() else {
                    let msg = "没有待确认的预设导出范围".to_string();
                    self.preset_transfer_error = Some(msg.clone());
                    self.commit_runtime_update();
                    return Err(WorkbenchError::Preset(msg));
                };
                let (profile_id, layer_id) = match scope {
                    PresetExportScope::Document => (None, None),
                    PresetExportScope::Profile => (self.selected_profile_id.clone(), None),
                    PresetExportScope::Layer => (
                        self.selected_profile_id.clone(),
                        self.selected_layer_id.clone(),
                    ),
                };
                self.preset_transfer_error = None;
                self.apply_backup_command(BackupCommand::ExportPreset {
                    profile_id,
                    layer_id,
                })?;
                Ok(())
            }
            StatusMenuAction::CancelPresetExport => {
                self.pending_preset_export_scope = None;
                self.preset_transfer_error = None;
                self.commit_runtime_update();
                Ok(())
            }
            StatusMenuAction::OpenMainWorkbenchWindow => {
                self.set_main_workbench_window_active(true);
                self.status_message = "已打开主工作台窗口".to_string();
                self.commit_runtime_update();
                Ok(())
            }
        }
    }

    pub fn hud_visibility(&self) -> &HudVisibilityState {
        self.overlay_controller.visibility()
    }

    pub fn favorites_grid(&self) -> &PinnedFavoritesGrid {
        &self.pinned_favorites
    }

    pub fn radial_state(&self) -> &RadialMenuState {
        &self.radial_state
    }

    pub fn last_host_action_receipt(&self) -> Option<&HostActionReceipt> {
        self.last_host_action_receipt.as_ref()
    }

    pub fn dispatched_receipts(&self) -> &[HostActionReceipt] {
        &self.dispatched_receipts
    }
}
