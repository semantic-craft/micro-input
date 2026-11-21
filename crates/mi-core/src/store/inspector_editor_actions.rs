use super::*;

impl WorkbenchStore {
    pub(super) fn sync_modal_for_layer_protection(&mut self) {
        if self.is_selected_layer_protected() {
            if self
                .active_modal
                .as_ref()
                .is_some_and(EditorModalProjection::is_mutating_layer_editor)
            {
                self.active_modal = None;
            }
            self.shortcut_recorder.is_recording = false;
            self.end_lighting_preview(LightingPreviewStopReason::LayerProtected);
            self.lighting_popover.manual_preview_phase = None;
            self.lighting_arbiter.manual_preview_phase = None;
        }
    }

    fn current_selected_slot(&self) -> u32 {
        self.projection_inputs().current_selected_slot()
    }

    pub fn dispatch_inspector_action(
        &mut self,
        action: InspectorAction,
    ) -> Result<(), WorkbenchError> {
        match action {
            InspectorAction::UnlockProtectedLayer => {
                self.dispatch_canvas_action(CanvasAction::UnlockBySwitchingOrCreatingCustomLayer)
            }
            InspectorAction::SelectSlot(slot) => {
                self.dispatch_canvas_action(CanvasAction::SelectLogicalSlot(slot))
            }
            InspectorAction::SelectJoystickDirection(dir) => {
                self.dispatch_canvas_action(CanvasAction::SelectJoystickDirection(dir))
            }
            InspectorAction::SetJoystickMode(mode) => {
                self.ensure_layer_editable()?;
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                let active_sectors = self
                    .document_store
                    .draft
                    .document
                    .editable
                    .graph
                    .layers
                    .get(&lid)
                    .and_then(|l| l.control_settings.as_ref())
                    .map(|s| s.active_sectors)
                    .map(|n| {
                        if matches!(mode, JoystickControlMode::LogicalSectors) && n == 0 {
                            4
                        } else {
                            n
                        }
                    })
                    .unwrap_or(match mode {
                        JoystickControlMode::LogicalSectors => 4,
                        JoystickControlMode::Source
                        | JoystickControlMode::Mouse
                        | JoystickControlMode::Scroll => 0,
                    });
                self.document_store
                    .update(|draft| {
                        draft.set_control_settings(
                            &pid,
                            &lid,
                            mi_model::control_settings::ControlSettings {
                                joystick_mode: mode.as_model_str().to_string(),
                                active_sectors,
                                encoder_hold_action_id: None,
                            },
                        )
                    })
                    .map_err(WorkbenchError::Storage)?;
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::SetKeycapSearchQuery(query) => {
                self.keycap_search_query = query;
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::SetBindKeycapDefaultAction(bind) => {
                self.bind_keycap_default_action_on_select = bind;
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::ToggleDrawer(drawer) => {
                if let Some(pos) = self.open_drawers.iter().position(|d| *d == drawer) {
                    self.open_drawers.remove(pos);
                } else {
                    self.open_drawers.push(drawer);
                }
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::SetActionSearchQuery(query) => {
                self.action_search_query = query;
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::SetActionCategoryFilter(category) => {
                self.action_category_filter = category;
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::SetFavoritesOnly(fav_only) => {
                self.favorites_only = fav_only;
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::ToggleActionFavorite {
                action_id,
                favorite,
            } => {
                if favorite {
                    self.favorite_action_ids.insert(action_id);
                } else {
                    self.favorite_action_ids.remove(&action_id);
                }
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::SetCommandExecutionEnabled(enabled) => {
                self.apply_routing_operation(RoutingOperation::CommandExecution { enabled })?;
                self.sync_routing_snapshot();
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::CancelShortcutRecording => {
                self.shortcut_recorder.is_recording = false;
                self.shortcut_recorder.validation_error = None;
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::CreateCustomLayer { name } => {
                let pid = self
                    .selected_profile_id
                    .clone()
                    .ok_or_else(|| WorkbenchError::Storage("未选择方案".to_string()))?;
                let mut created_lid = String::new();
                self.document_store
                    .update(|draft| {
                        created_lid = draft.create_layer(&pid, &name)?;
                        Ok(())
                    })
                    .map_err(WorkbenchError::Storage)?;
                self.selected_layer_id = Some(created_lid);
                self.v070_hardware.device_layer_override = None;
                self.normalize_surface_for_current_layer();
                self.sync_lighting_preview_after_document_change();
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::OpenEditor(editor_action) => {
                self.dispatch_editor_action(editor_action)
            }
            InspectorAction::ApplyKnobJoystickPreset(preset) => {
                self.dispatch_canvas_action(CanvasAction::ApplyKnobJoystickPreset(preset))
            }
            InspectorAction::SetBottomRowLayout(layout) => {
                self.dispatch_canvas_action(CanvasAction::SetBottomRowLayout(layout))
            }
            InspectorAction::MergeBottomRowAcrossLayers => {
                self.dispatch_canvas_action(CanvasAction::MergeBottomRowAcrossLayers)
            }
            InspectorAction::SetSlotRole(role) => {
                self.ensure_layer_editable()?;
                if self.selected_surface == ControlSurfaceId::EncoderHold {
                    return match role {
                        KeycapRole::TaskKey => Err(WorkbenchError::Storage(
                            "仅顶部 6 键 (AG00..AG05) 支持设为会话键".to_string(),
                        )),
                        KeycapRole::ActionKey => Ok(()),
                    };
                }
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                let slot = self.current_selected_slot();
                match role {
                    KeycapRole::TaskKey => {
                        if slot >= 6 {
                            return Err(WorkbenchError::Storage(
                                "仅顶部 6 键 (AG00..AG05) 支持设为会话键".to_string(),
                            ));
                        }
                        let target_id = format!("slot-open-{slot}");
                        let action = self.resolve_action_by_id(&target_id)?;
                        self.document_store
                            .update(|draft| draft.assign(&json!(pid), &json!(lid), slot, action))
                            .map_err(WorkbenchError::Storage)?;
                        self.slot_role_overrides
                            .insert((pid, slot), KeycapRole::TaskKey);
                    }
                    KeycapRole::ActionKey => {
                        self.slot_role_overrides
                            .insert((pid.clone(), slot), KeycapRole::ActionKey);
                        let doc = &self.document_store.draft.document;
                        let layer = doc.editable.graph.layers.get(&lid);
                        let is_session =
                            resolve_slot_action_info(doc, layer, slot)
                                .1
                                .is_some_and(|a| {
                                    a.id.starts_with("slot-open-")
                                        || a.id.starts_with("cockpit-slot-")
                                        || a.payload
                                            .get("command")
                                            .and_then(|v| v.as_str())
                                            .is_some_and(|c| c.starts_with("slot-open:"))
                                });
                        if is_session {
                            let keycap = self.selected_keycap(slot);
                            let fallback = keycap
                                .default_action()
                                .or_else(|| CodexKeycapId::Fast.default_action());
                            self.document_store
                                .update(|draft| {
                                    if let Some(act) = fallback {
                                        draft.assign(&json!(pid), &json!(lid), slot, act)
                                    } else {
                                        draft.unassign(&json!(pid), &json!(lid), slot)
                                    }
                                })
                                .map_err(WorkbenchError::Storage)?;
                        }
                    }
                }
                self.sync_v070_session_sa_rgb();
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::UnassignSelectedSlot => {
                self.ensure_layer_editable()?;
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                if self.selected_surface == ControlSurfaceId::EncoderHold {
                    self.document_store
                        .update(|draft| draft.unassign_encoder_hold(&json!(pid), &json!(lid)))
                        .map_err(WorkbenchError::Storage)?;
                    self.refresh_four_app_lighting();
                    self.commit_runtime_update();
                    return Ok(());
                }
                let slot = self.current_selected_slot();
                self.document_store
                    .update(|draft| draft.unassign(&json!(pid), &json!(lid), slot))
                    .map_err(WorkbenchError::Storage)?;
                self.slot_role_overrides
                    .insert((pid, slot), KeycapRole::ActionKey);
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::ApplyKeycapIcon {
                keycap,
                bind_default_action,
            } => {
                self.ensure_layer_editable()?;
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                if self.selected_surface == ControlSurfaceId::EncoderHold {
                    if bind_default_action {
                        if let Some(action) = keycap.default_action() {
                            self.document_store
                                .update(|draft| {
                                    draft.assign_encoder_hold(&json!(pid), &json!(lid), action)
                                })
                                .map_err(WorkbenchError::Storage)?;
                        } else if keycap == CodexKeycapId::Blank {
                            self.document_store
                                .update(|draft| {
                                    draft.unassign_encoder_hold(&json!(pid), &json!(lid))
                                })
                                .map_err(WorkbenchError::Storage)?;
                        }
                    }
                    self.commit_runtime_update();
                    return Ok(());
                }
                let slot = self.current_selected_slot();
                self.keycap_overrides.insert((pid.clone(), slot), keycap);
                if bind_default_action {
                    self.slot_role_overrides
                        .insert((pid.clone(), slot), KeycapRole::ActionKey);
                    if let Some(action) = keycap.default_action() {
                        self.document_store
                            .update(|draft| draft.assign(&json!(pid), &json!(lid), slot, action))
                            .map_err(WorkbenchError::Storage)?;
                    } else if keycap == CodexKeycapId::Blank {
                        self.document_store
                            .update(|draft| draft.unassign(&json!(pid), &json!(lid), slot))
                            .map_err(WorkbenchError::Storage)?;
                    }
                    self.sync_v070_session_sa_rgb();
                }
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::BindAppleFnKey => {
                self.ensure_layer_editable()?;
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                let action = self.resolve_action_by_id("apple-fn-key")?;
                if self.selected_surface == ControlSurfaceId::EncoderHold {
                    self.document_store
                        .update(|draft| draft.assign_encoder_hold(&json!(pid), &json!(lid), action))
                        .map_err(WorkbenchError::Storage)?;
                    self.shortcut_recorder.is_recording = false;
                    self.shortcut_recorder.validation_error = None;
                    self.commit_runtime_update();
                    return Ok(());
                }
                let slot = self.current_selected_slot();
                self.document_store
                    .update(|draft| draft.assign(&json!(pid), &json!(lid), slot, action))
                    .map_err(WorkbenchError::Storage)?;
                self.keycap_overrides
                    .insert((pid.clone(), slot), CodexKeycapId::Mic);
                self.slot_role_overrides
                    .insert((pid, slot), KeycapRole::ActionKey);
                self.shortcut_recorder.is_recording = false;
                self.shortcut_recorder.validation_error = None;
                self.sync_v070_session_sa_rgb();
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::StartShortcutRecording => {
                self.ensure_layer_editable()?;
                self.shortcut_recorder = ShortcutRecorderState {
                    is_recording: true,
                    key: String::new(),
                    modifiers: Vec::new(),
                    target: "frontmost-editor".to_string(),
                    preview_badge: "请按下快捷键…".to_string(),
                    validation_error: None,
                };
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::CaptureShortcutChord { key, mut modifiers } => {
                self.ensure_layer_editable()?;
                modifiers.sort();
                modifiers.dedup();
                let mut badge = String::new();
                for m in &modifiers {
                    badge.push_str(m.symbol());
                }
                badge.push_str(&key.trim().to_uppercase());
                self.shortcut_recorder.key = key.trim().to_string();
                self.shortcut_recorder.modifiers = modifiers;
                self.shortcut_recorder.preview_badge = badge;
                self.shortcut_recorder.validation_error = None;
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::SetShortcutTarget(target) => {
                self.ensure_layer_editable()?;
                self.shortcut_recorder.target = target;
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::CommitRecordedShortcut => {
                self.ensure_layer_editable()?;
                let raw_key = self.shortcut_recorder.key.trim().to_lowercase();
                if raw_key.is_empty() {
                    let msg = "请先录入快捷键按键".to_string();
                    self.shortcut_recorder.validation_error = Some(msg.clone());
                    self.commit_runtime_update();
                    return Err(WorkbenchError::Storage(msg));
                }
                if self.shortcut_recorder.modifiers.is_empty()
                    && matches!(raw_key.as_str(), "fn" | "globe" | "0x3f" | "apple_fn")
                {
                    return self.dispatch_inspector_action(InspectorAction::BindAppleFnKey);
                }
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                let slot = self.current_selected_slot();
                let mods: Vec<String> = self
                    .shortcut_recorder
                    .modifiers
                    .iter()
                    .map(|m| m.as_draft_str().to_string())
                    .collect();
                let target = if self.shortcut_recorder.target.trim().is_empty() {
                    "frontmost-editor".to_string()
                } else {
                    self.shortcut_recorder.target.trim().to_string()
                };
                let badge = if self.shortcut_recorder.preview_badge.is_empty() {
                    raw_key.to_uppercase()
                } else {
                    self.shortcut_recorder.preview_badge.clone()
                };
                let is_encoder_hold = self.selected_surface == ControlSurfaceId::EncoderHold;
                let action = Action {
                    id: if is_encoder_hold {
                        "custom-shortcut-encoder-hold".to_string()
                    } else {
                        format!("custom-shortcut-{slot}")
                    },
                    name: format!("快捷键 {badge}"),
                    category: "快捷键".to_string(),
                    detail: format!("自定义录制快捷键 · 目标 {target}"),
                    payload: json!({
                        "type": "shortcut",
                        "key": raw_key,
                        "modifiers": mods,
                        "target": target,
                    }),
                };
                action.validate().map_err(WorkbenchError::Storage)?;
                if is_encoder_hold {
                    self.document_store
                        .update(|draft| draft.assign_encoder_hold(&json!(pid), &json!(lid), action))
                        .map_err(WorkbenchError::Storage)?;
                } else {
                    self.document_store
                        .update(|draft| draft.assign(&json!(pid), &json!(lid), slot, action))
                        .map_err(WorkbenchError::Storage)?;
                    self.slot_role_overrides
                        .insert((pid, slot), KeycapRole::ActionKey);
                    self.sync_v070_session_sa_rgb();
                }
                self.shortcut_recorder.is_recording = false;
                self.shortcut_recorder.validation_error = None;
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::AssignCatalogAction(action_id)
            | InspectorAction::BindExistingAction(action_id) => {
                self.ensure_layer_editable()?;
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                if self.selected_surface == ControlSurfaceId::EncoderHold {
                    let exists_in_graph = self
                        .document_store
                        .draft
                        .document
                        .editable
                        .graph
                        .actions
                        .contains_key(&action_id);
                    if exists_in_graph {
                        self.document_store
                            .update(|draft| {
                                draft.bind_existing_encoder_hold_action(
                                    &pid,
                                    &lid,
                                    Some(&action_id),
                                )
                            })
                            .map_err(WorkbenchError::Storage)?;
                    } else {
                        let action = self.resolve_action_by_id(&action_id)?;
                        self.document_store
                            .update(|draft| {
                                draft.assign_encoder_hold(&json!(pid), &json!(lid), action)
                            })
                            .map_err(WorkbenchError::Storage)?;
                    }
                    self.commit_runtime_update();
                    return Ok(());
                }
                let slot = self.current_selected_slot();
                let action = self.resolve_action_by_id(&action_id)?;
                self.document_store
                    .update(|draft| draft.assign(&json!(pid), &json!(lid), slot, action))
                    .map_err(WorkbenchError::Storage)?;
                if action_id.starts_with("slot-open-") && slot < 6 {
                    self.slot_role_overrides
                        .insert((pid, slot), KeycapRole::TaskKey);
                } else {
                    self.slot_role_overrides
                        .insert((pid, slot), KeycapRole::ActionKey);
                }
                self.sync_v070_session_sa_rgb();
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::AssignCustomText {
                name,
                text,
                submit_on_paste,
            } => {
                self.ensure_layer_editable()?;
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                let is_encoder_hold = self.selected_surface == ControlSurfaceId::EncoderHold;
                let slot = self.current_selected_slot();
                let suffix = if is_encoder_hold {
                    "encoder-hold".to_string()
                } else {
                    slot.to_string()
                };
                let action = if submit_on_paste {
                    Action {
                        id: format!("custom-prompt-submit-{suffix}"),
                        name,
                        category: "提交控制".to_string(),
                        detail: "显式配置下先停听写、聚焦目标输入区、暂存剪贴板粘贴后再回车"
                            .to_string(),
                        payload: json!({
                            "type": "agent_command",
                            "adapter": "host",
                            "command": format!("submit-prompt:{text}"),
                        }),
                    }
                } else {
                    Action {
                        id: format!("custom-text-{suffix}"),
                        name,
                        category: "文本".to_string(),
                        detail: "默认仅粘贴不回车（no_newline） · 自动暂存恢复剪贴板".to_string(),
                        payload: json!({
                            "type": "text",
                            "text": text,
                        }),
                    }
                };
                action.validate().map_err(WorkbenchError::Storage)?;
                if is_encoder_hold {
                    self.document_store
                        .update(|draft| draft.assign_encoder_hold(&json!(pid), &json!(lid), action))
                        .map_err(WorkbenchError::Storage)?;
                } else {
                    self.document_store
                        .update(|draft| draft.assign(&json!(pid), &json!(lid), slot, action))
                        .map_err(WorkbenchError::Storage)?;
                    self.slot_role_overrides
                        .insert((pid, slot), KeycapRole::ActionKey);
                    self.sync_v070_session_sa_rgb();
                }
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::RenameSelectedLayer { name } => {
                self.ensure_layer_editable()?;
                let (_, lid) = self.require_selected_profile_and_layer()?;
                self.document_store
                    .update(|draft| draft.rename_layer(&lid, &name))
                    .map_err(WorkbenchError::Storage)?;
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::DuplicateSelectedLayer { name } => {
                self.ensure_layer_editable()?;
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                let resolved_name = if name.trim().is_empty() {
                    let src_layer_name = self
                        .document_store
                        .draft
                        .document
                        .editable
                        .graph
                        .layers
                        .get(&lid)
                        .map(|l| l.name.as_str())
                        .unwrap_or("默认层");
                    format!("{src_layer_name} 副本")
                } else {
                    name
                };
                let mut created = String::new();
                self.document_store
                    .update(|draft| {
                        created = draft.duplicate_layer(&pid, &lid, &resolved_name)?;
                        Ok(())
                    })
                    .map_err(WorkbenchError::Storage)?;
                self.selected_layer_id = Some(created);
                self.v070_hardware.device_layer_override = None;
                self.normalize_surface_for_current_layer();
                self.sync_lighting_preview_after_document_change();
                self.sync_v070_session_sa_rgb();
                self.commit_runtime_update();
                Ok(())
            }
            InspectorAction::DeleteSelectedLayer => {
                self.ensure_layer_editable()?;
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                self.document_store
                    .update(|draft| draft.delete_layer(&pid, &lid))
                    .map_err(WorkbenchError::Storage)?;
                self.selected_layer_id = self
                    .document_store
                    .draft
                    .document
                    .editable
                    .graph
                    .profiles
                    .get(&pid)
                    .and_then(|p| p.layer_order.first().cloned());
                self.v070_hardware.device_layer_override = None;
                self.normalize_surface_for_current_layer();
                self.sync_lighting_preview_after_document_change();
                self.sync_v070_session_sa_rgb();
                self.commit_runtime_update();
                Ok(())
            }
        }
    }

    pub fn dispatch_editor_action(&mut self, action: EditorAction) -> Result<(), WorkbenchError> {
        if matches!(
            action,
            EditorAction::OpenReconnectComparison { .. }
                | EditorAction::ComparisonConfirmIdentity { .. }
                | EditorAction::ComparisonResolve { .. }
                | EditorAction::OpenApplyPreparation { .. }
                | EditorAction::ApplyPrepareBackup { .. }
                | EditorAction::ApplyTakePreparedCandidate { .. }
                | EditorAction::LightingBeginPreview
        ) {
            self.require_awake()?;
        }
        match action {
            EditorAction::CloseModal => {
                self.end_lighting_preview(LightingPreviewStopReason::ModalClosed);
                self.active_modal = None;
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::LightingEndPreview => {
                self.end_lighting_preview(LightingPreviewStopReason::UserRequested);
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::ApplyCancelPreview => {
                self.document_store.cancel_apply_preview();
                self.active_modal = None;
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::OpenReconnectComparison {
                device_config,
                baseline,
            } => {
                let cmp = self
                    .document_store
                    .compare(&device_config, baseline)
                    .map_err(WorkbenchError::Storage)?;
                self.end_lighting_preview(LightingPreviewStopReason::ModalClosed);
                self.active_modal = Some(EditorModalProjection::ReconnectComparison(
                    build_comparison_modal_projection(&cmp),
                ));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::ComparisonConfirmIdentity { current_snapshot } => {
                let token = match &self.active_modal {
                    Some(EditorModalProjection::ReconnectComparison(m)) => m.token.clone(),
                    _ => return Err(WorkbenchError::Storage("未打开重连比较窗口".to_string())),
                };
                let cmp = self
                    .document_store
                    .confirm_comparison_identity(&token, &current_snapshot)
                    .map_err(WorkbenchError::Storage)?;
                self.active_modal = Some(EditorModalProjection::ReconnectComparison(
                    build_comparison_modal_projection(&cmp),
                ));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::ComparisonResolve {
                choice,
                current_snapshot,
            } => {
                let token = match &self.active_modal {
                    Some(EditorModalProjection::ReconnectComparison(m)) => m.token.clone(),
                    _ => return Err(WorkbenchError::Storage("未打开重连比较窗口".to_string())),
                };
                self.document_store
                    .resolve_comparison(&token, choice, &current_snapshot)
                    .map_err(WorkbenchError::Storage)?;
                self.end_lighting_preview(LightingPreviewStopReason::ContextChanged);
                self.active_modal = None;
                self.v070_hardware.device_layer_override = None;
                self.ensure_selected_profile_visible();
                self.sync_v070_session_sa_rgb();
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::OpenApplyPreparation { current_snapshot } => {
                let preview = self
                    .document_store
                    .preview_apply(&current_snapshot)
                    .map_err(WorkbenchError::Storage)?;
                self.end_lighting_preview(LightingPreviewStopReason::ModalClosed);
                self.active_modal = Some(EditorModalProjection::ApplyPreparation(
                    build_apply_modal_projection(&preview),
                ));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::ApplyPrepareBackup { current_snapshot } => {
                let mut modal = match &self.active_modal {
                    Some(EditorModalProjection::ApplyPreparation(m)) => m.clone(),
                    _ => {
                        return Err(WorkbenchError::Storage(
                            "未打开配置应用预检窗口".to_string(),
                        ))
                    }
                };
                let prepared = self
                    .document_store
                    .prepare_apply_backup(&modal.token, &current_snapshot)
                    .map_err(WorkbenchError::Storage)?;
                modal.backup_prepared = true;
                modal.backup_path = Some(prepared.backup_path);
                modal.manifest_sha256 = Some(prepared.manifest_sha256);
                modal.unavailable_reason = prepared.unavailable_reason;
                modal.error_message = None;
                self.active_modal = Some(EditorModalProjection::ApplyPreparation(modal));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::ApplyTakePreparedCandidate { current_snapshot } => {
                let mut modal = match &self.active_modal {
                    Some(EditorModalProjection::ApplyPreparation(m)) => m.clone(),
                    _ => {
                        return Err(WorkbenchError::Storage(
                            "未打开配置应用预检窗口".to_string(),
                        ))
                    }
                };
                let candidate = self
                    .document_store
                    .take_prepared_candidate(&modal.token, &current_snapshot)
                    .map_err(WorkbenchError::Storage)?;
                modal.candidate_taken = true;
                modal.candidate_operation_id = Some(candidate.operation_id().to_string());
                modal.error_message = None;
                self.active_modal = Some(EditorModalProjection::ApplyPreparation(modal));
                self.commit_runtime_update();
                Ok(())
            }
            mutating_action => {
                self.ensure_layer_editable()?;
                self.dispatch_mutating_editor_action(mutating_action)
            }
        }
    }

    fn dispatch_mutating_editor_action(
        &mut self,
        action: EditorAction,
    ) -> Result<(), WorkbenchError> {
        match action {
            EditorAction::OpenMacroEditor { macro_id } => {
                let target_slot = Some(self.current_selected_slot());
                let proj = if let Some(id) = macro_id {
                    let act = self
                        .document_store
                        .draft
                        .document
                        .editable
                        .graph
                        .actions
                        .get(&id)
                        .ok_or_else(|| WorkbenchError::Storage("宏不存在".to_string()))?;
                    let ActionKind::LocalMacro { macro_sequence } =
                        serde_json::from_value(act.payload.clone())
                            .map_err(|_| WorkbenchError::Storage("不是有效的本地宏".to_string()))?
                    else {
                        return Err(WorkbenchError::Storage("不是有效的本地宏".to_string()));
                    };
                    let reference_summaries = self
                        .document_store
                        .draft
                        .macros()
                        .into_iter()
                        .find(|m| m.id == id)
                        .map(|m| {
                            m.references
                                .iter()
                                .map(|r| {
                                    format!(
                                        "{} / {} · {}",
                                        r.profile_name, r.layer_name, r.logical_control
                                    )
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    build_macro_editor_projection(
                        Some(id),
                        act.name.clone(),
                        macro_sequence.events,
                        target_slot,
                        reference_summaries,
                    )
                } else {
                    build_macro_editor_projection(
                        None,
                        "新建宏".to_string(),
                        Vec::new(),
                        target_slot,
                        Vec::new(),
                    )
                };
                self.end_lighting_preview(LightingPreviewStopReason::ModalClosed);
                self.active_modal = Some(EditorModalProjection::MacroEditor(proj));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MacroSetName(name) => {
                let mut modal = self.take_macro_modal()?;
                modal.name = name;
                self.active_modal = Some(EditorModalProjection::MacroEditor(modal));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MacroSetDraftKeyInput(key) => {
                let mut modal = self.take_macro_modal()?;
                modal.draft_key_input = key;
                self.active_modal = Some(EditorModalProjection::MacroEditor(modal));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MacroSetDraftDelayMs(ms) => {
                let mut modal = self.take_macro_modal()?;
                modal.draft_delay_ms = ms;
                self.active_modal = Some(EditorModalProjection::MacroEditor(modal));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MacroAppendEvent(event) => {
                let modal = self.take_macro_modal()?;
                let mut events: Vec<MacroEvent> =
                    modal.steps.iter().map(|s| s.event.clone()).collect();
                events.push(event);
                let next = rebuild_macro_modal_with_events(modal, events);
                self.active_modal = Some(EditorModalProjection::MacroEditor(next));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MacroAddTapKey { key, delay_ms } => {
                let modal = self.take_macro_modal()?;
                let mut events: Vec<MacroEvent> =
                    modal.steps.iter().map(|s| s.event.clone()).collect();
                let normalized_key = key.trim().to_lowercase();
                events.push(MacroEvent::Press {
                    key: normalized_key.clone(),
                });
                if delay_ms > 0 {
                    events.push(MacroEvent::Delay { ms: delay_ms });
                }
                events.push(MacroEvent::Release {
                    key: normalized_key,
                });
                let next = rebuild_macro_modal_with_events(modal, events);
                self.active_modal = Some(EditorModalProjection::MacroEditor(next));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MacroRemoveStep(index) => {
                let modal = self.take_macro_modal()?;
                let mut events: Vec<MacroEvent> =
                    modal.steps.iter().map(|s| s.event.clone()).collect();
                if index < events.len() {
                    events.remove(index);
                }
                let next = rebuild_macro_modal_with_events(modal, events);
                self.active_modal = Some(EditorModalProjection::MacroEditor(next));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MacroMoveStep { from, to } => {
                let modal = self.take_macro_modal()?;
                let mut events: Vec<MacroEvent> =
                    modal.steps.iter().map(|s| s.event.clone()).collect();
                if from < events.len() && to < events.len() && from != to {
                    let ev = events.remove(from);
                    events.insert(to, ev);
                }
                let next = rebuild_macro_modal_with_events(modal, events);
                self.active_modal = Some(EditorModalProjection::MacroEditor(next));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MacroClearSteps => {
                let modal = self.take_macro_modal()?;
                let next = rebuild_macro_modal_with_events(modal, Vec::new());
                self.active_modal = Some(EditorModalProjection::MacroEditor(next));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MacroSave {
                bind_to_selected_slot,
            } => {
                let mut modal = self.take_macro_modal()?;
                if modal.macro_id.is_none() && self.document_store.draft.macros().len() >= 30 {
                    let msg = "宏库已达 30 个上限".to_string();
                    modal.validation_error = Some(msg.clone());
                    self.active_modal = Some(EditorModalProjection::MacroEditor(modal));
                    self.commit_runtime_update();
                    return Err(WorkbenchError::Storage(msg));
                }
                let events: Vec<MacroEvent> = modal.steps.iter().map(|s| s.event.clone()).collect();
                let local_macro = LocalMacro {
                    format_version: 1,
                    events,
                };
                if let Err(err) = local_macro.validate() {
                    modal.validation_error = Some(err.clone());
                    self.active_modal = Some(EditorModalProjection::MacroEditor(modal));
                    self.commit_runtime_update();
                    return Err(WorkbenchError::Storage(err));
                }
                let trimmed_name = modal.name.trim();
                let canonical_name = if modal.macro_id.is_none() && trimmed_name == "New Macro" {
                    "新建宏".to_string()
                } else {
                    trimmed_name.to_string()
                };
                let action = Action {
                    id: modal
                        .macro_id
                        .clone()
                        .unwrap_or_else(|| "local-macro".to_string()),
                    name: canonical_name,
                    category: "普通宏".to_string(),
                    detail: "自有本地序列 · 已验证键名可宿主执行".to_string(),
                    payload: serde_json::to_value(ActionKind::LocalMacro {
                        macro_sequence: local_macro,
                    })
                    .map_err(|e| WorkbenchError::Storage(e.to_string()))?,
                };
                if bind_to_selected_slot {
                    let (pid, lid) = self.require_selected_profile_and_layer()?;
                    if self.selected_surface == ControlSurfaceId::EncoderHold {
                        self.document_store
                            .update(|draft| {
                                let saved_id =
                                    draft.save_macro(modal.macro_id.as_deref(), action)?;
                                draft.bind_existing_encoder_hold_action(&pid, &lid, Some(&saved_id))
                            })
                            .map_err(WorkbenchError::Storage)?;
                    } else {
                        let slot = modal
                            .target_slot
                            .unwrap_or_else(|| self.current_selected_slot());
                        self.document_store
                            .update(|draft| {
                                draft
                                    .save_macro_for_binding(
                                        modal.macro_id.as_deref(),
                                        action,
                                        &json!(pid),
                                        &json!(lid),
                                        slot,
                                    )
                                    .map(|_| ())
                            })
                            .map_err(WorkbenchError::Storage)?;
                        self.slot_role_overrides
                            .insert((pid, slot), KeycapRole::ActionKey);
                    }
                } else {
                    self.document_store
                        .update(|draft| {
                            draft
                                .save_macro(modal.macro_id.as_deref(), action)
                                .map(|_| ())
                        })
                        .map_err(WorkbenchError::Storage)?;
                }
                self.active_modal = None;
                self.sync_v070_session_sa_rgb();
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MacroBindExisting { macro_id } => {
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                if self.selected_surface == ControlSurfaceId::EncoderHold {
                    self.document_store
                        .update(|draft| {
                            draft.bind_existing_encoder_hold_action(&pid, &lid, macro_id.as_deref())
                        })
                        .map_err(WorkbenchError::Storage)?;
                } else {
                    let slot = self.current_selected_slot();
                    self.document_store
                        .update(|draft| {
                            draft.bind_macro(&json!(pid), &json!(lid), slot, macro_id.as_deref())
                        })
                        .map_err(WorkbenchError::Storage)?;
                    self.slot_role_overrides
                        .insert((pid, slot), KeycapRole::ActionKey);
                    self.sync_v070_session_sa_rgb();
                }
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MacroDelete { macro_id } => {
                self.document_store
                    .update(|draft| draft.delete_macro(&macro_id))
                    .map_err(WorkbenchError::Storage)?;
                if matches!(
                    &self.active_modal,
                    Some(EditorModalProjection::MacroEditor(m)) if m.macro_id.as_deref() == Some(macro_id.as_str())
                ) {
                    self.active_modal = None;
                }
                self.sync_v070_session_sa_rgb();
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::OpenLightingEditor => {
                let proj = self.build_lighting_modal_projection(None)?;
                self.active_modal = Some(EditorModalProjection::LightingEditor(proj));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::LightingSetApplyToAllLayers(all_layers) => {
                self.lighting_apply_to_all_layers = all_layers;
                let proj = self.build_lighting_modal_projection(None)?;
                self.active_modal = Some(EditorModalProjection::LightingEditor(proj));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::LightingAddDefault => {
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                let all_layers = self.lighting_apply_to_all_layers;
                self.document_store
                    .update(|draft| ensure_layer_has_base_lighting(draft, &pid, &lid, all_layers))
                    .map_err(WorkbenchError::Storage)?;
                let preview_err = self.sync_lighting_preview_for_layer(&lid);
                let proj = self.build_lighting_modal_projection(preview_err)?;
                self.active_modal = Some(EditorModalProjection::LightingEditor(proj));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::LightingEditEffect {
                zone,
                effect,
                all_layers,
            } => {
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                self.lighting_apply_to_all_layers = all_layers;
                self.document_store
                    .update(|draft| {
                        ensure_layer_has_base_lighting(draft, &pid, &lid, all_layers)?;
                        draft.edit_lighting(
                            &pid,
                            &lid,
                            zone,
                            LightingEdit::Effect(effect),
                            all_layers,
                        )
                    })
                    .map_err(WorkbenchError::Storage)?;
                let preview_err =
                    self.push_lighting_preview_edit(&lid, zone, LightingEdit::Effect(effect));
                let proj = self.build_lighting_modal_projection(preview_err)?;
                self.active_modal = Some(EditorModalProjection::LightingEditor(proj));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::LightingEditBrightness {
                zone,
                brightness_percent,
                all_layers,
            } => {
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                self.lighting_apply_to_all_layers = all_layers;
                let clamped = brightness_percent.min(100);
                self.document_store
                    .update(|draft| {
                        ensure_layer_has_base_lighting(draft, &pid, &lid, all_layers)?;
                        draft.edit_lighting(
                            &pid,
                            &lid,
                            zone,
                            LightingEdit::Brightness(clamped),
                            all_layers,
                        )
                    })
                    .map_err(WorkbenchError::Storage)?;
                let preview_err =
                    self.push_lighting_preview_edit(&lid, zone, LightingEdit::Brightness(clamped));
                let proj = self.build_lighting_modal_projection(preview_err)?;
                self.active_modal = Some(EditorModalProjection::LightingEditor(proj));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::LightingEditSpeed {
                zone,
                speed_percent,
                all_layers,
            } => {
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                self.lighting_apply_to_all_layers = all_layers;
                let clamped = speed_percent.min(100);
                self.document_store
                    .update(|draft| {
                        ensure_layer_has_base_lighting(draft, &pid, &lid, all_layers)?;
                        draft.edit_lighting(
                            &pid,
                            &lid,
                            zone,
                            LightingEdit::Speed(clamped),
                            all_layers,
                        )
                    })
                    .map_err(WorkbenchError::Storage)?;
                let preview_err =
                    self.push_lighting_preview_edit(&lid, zone, LightingEdit::Speed(clamped));
                let proj = self.build_lighting_modal_projection(preview_err)?;
                self.active_modal = Some(EditorModalProjection::LightingEditor(proj));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::LightingEditMagic {
                zone,
                magic_percent,
                all_layers,
            } => {
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                self.lighting_apply_to_all_layers = all_layers;
                let clamped = magic_percent.min(100);
                self.document_store
                    .update(|draft| {
                        ensure_layer_has_base_lighting(draft, &pid, &lid, all_layers)?;
                        draft.edit_lighting(
                            &pid,
                            &lid,
                            zone,
                            LightingEdit::Magic(clamped),
                            all_layers,
                        )
                    })
                    .map_err(WorkbenchError::Storage)?;
                let preview_err =
                    self.push_lighting_preview_edit(&lid, zone, LightingEdit::Magic(clamped));
                let proj = self.build_lighting_modal_projection(preview_err)?;
                self.active_modal = Some(EditorModalProjection::LightingEditor(proj));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::LightingEditColorHex {
                zone,
                color_hex,
                all_layers,
            } => {
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                self.lighting_apply_to_all_layers = all_layers;
                let rgb_u32 = match parse_hex_color(&color_hex) {
                    Ok(rgb) => rgb,
                    Err(err) => {
                        if let Ok(proj) = self.build_lighting_modal_projection(Some(err.clone())) {
                            self.active_modal = Some(EditorModalProjection::LightingEditor(proj));
                        }
                        self.commit_runtime_update();
                        return Err(WorkbenchError::Storage(err));
                    }
                };
                self.document_store
                    .update(|draft| {
                        ensure_layer_has_base_lighting(draft, &pid, &lid, all_layers)?;
                        draft.edit_lighting(
                            &pid,
                            &lid,
                            zone,
                            LightingEdit::Color(rgb_u32),
                            all_layers,
                        )
                    })
                    .map_err(WorkbenchError::Storage)?;
                let preview_err =
                    self.push_lighting_preview_edit(&lid, zone, LightingEdit::Color(rgb_u32));
                let proj = self.build_lighting_modal_projection(preview_err)?;
                self.active_modal = Some(EditorModalProjection::LightingEditor(proj));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::LightingResetLayer => {
                let (_, lid) = self.require_selected_profile_and_layer()?;
                let has_source = self
                    .document_store
                    .draft
                    .document
                    .editable
                    .source_refs
                    .contains_key(&lid);
                self.document_store
                    .update(|draft| {
                        if has_source {
                            draft.reset_lighting(&lid)
                        } else {
                            draft.mutate(|doc| {
                                if let Some(layer) = doc.editable.graph.layers.get_mut(&lid) {
                                    layer.lighting = None;
                                }
                                Ok(())
                            })
                        }
                    })
                    .map_err(WorkbenchError::Storage)?;
                let preview_err = self.sync_lighting_preview_for_layer(&lid);
                let proj = self.build_lighting_modal_projection(preview_err)?;
                self.active_modal = Some(EditorModalProjection::LightingEditor(proj));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::LightingBeginPreview => self.begin_lighting_preview(),
            EditorAction::OpenMultiEditor { multi_id } => {
                let proj = self.build_multi_modal_projection(multi_id)?;
                self.end_lighting_preview(LightingPreviewStopReason::ModalClosed);
                self.active_modal = Some(EditorModalProjection::MultiEditor(proj));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MultiSetName(name) => {
                let mut modal = self.take_multi_modal()?;
                modal.name = name;
                self.active_modal = Some(EditorModalProjection::MultiEditor(modal));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MultiSetTermMs(term_ms) => {
                let mut modal = self.take_multi_modal()?;
                modal.term_ms = term_ms;
                self.active_modal = Some(EditorModalProjection::MultiEditor(modal));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MultiSetBranch { branch, target } => {
                let mut modal = self.take_multi_modal()?;
                let display_name = target
                    .as_ref()
                    .map(|t| self.resolve_multi_target_display_name(t))
                    .unwrap_or_else(|| "未设置".to_string());
                modal.branches[branch as usize] = MultiBranchSlotProjection {
                    branch,
                    title: branch.title().to_string(),
                    target,
                    target_display_name: display_name,
                };
                self.active_modal = Some(EditorModalProjection::MultiEditor(modal));
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MultiSave {
                bind_to_selected_slot,
            } => {
                let mut modal = self.take_multi_modal()?;
                let mut additions: BTreeMap<String, Action> = BTreeMap::new();
                let mut resolved_refs: [Option<ActionReference>; 4] = [None, None, None, None];
                for (idx, slot_proj) in modal.branches.iter().enumerate() {
                    if let Some(target) = &slot_proj.target {
                        match self.resolve_multi_branch_reference(target, &mut additions) {
                            Ok(reference) => resolved_refs[idx] = Some(reference),
                            Err(err) => {
                                modal.validation_error = Some(err.to_string());
                                self.active_modal = Some(EditorModalProjection::MultiEditor(modal));
                                self.commit_runtime_update();
                                return Err(err);
                            }
                        }
                    }
                }
                let local_multi = LocalMulti {
                    format_version: 1,
                    term_ms: modal.term_ms,
                    tap: resolved_refs[0].clone(),
                    double_tap: resolved_refs[1].clone(),
                    hold: resolved_refs[2].clone(),
                    tap_hold: resolved_refs[3].clone(),
                };
                if let Err(err) = local_multi.validate() {
                    modal.validation_error = Some(err.clone());
                    self.active_modal = Some(EditorModalProjection::MultiEditor(modal));
                    self.commit_runtime_update();
                    return Err(WorkbenchError::Storage(err));
                }
                let trimmed_name = modal.name.trim();
                let canonical_name = if modal.multi_id.is_none() && trimmed_name == "New Multi" {
                    "新建 Multi".to_string()
                } else {
                    trimmed_name.to_string()
                };
                let action = Action {
                    id: modal
                        .multi_id
                        .clone()
                        .unwrap_or_else(|| "local-multi".to_string()),
                    name: canonical_name,
                    category: "Multi".to_string(),
                    detail: "本地四分支 · 不执行 · 固件范围未知".to_string(),
                    payload: serde_json::to_value(ActionKind::LocalMulti { multi: local_multi })
                        .map_err(|e| WorkbenchError::Storage(e.to_string()))?,
                };
                if bind_to_selected_slot {
                    let (pid, lid) = self.require_selected_profile_and_layer()?;
                    if self.selected_surface == ControlSurfaceId::EncoderHold {
                        self.document_store
                            .update(|draft| {
                                draft
                                    .save_multi_for_encoder_hold(
                                        modal.multi_id.as_deref(),
                                        action,
                                        additions,
                                        &pid,
                                        &lid,
                                    )
                                    .map(|_| ())
                            })
                            .map_err(WorkbenchError::Storage)?;
                    } else {
                        let slot = modal
                            .target_slot
                            .unwrap_or_else(|| self.current_selected_slot());
                        self.document_store
                            .update(|draft| {
                                draft
                                    .save_multi_for_binding(
                                        modal.multi_id.as_deref(),
                                        action,
                                        additions,
                                        &json!(pid),
                                        &json!(lid),
                                        slot,
                                    )
                                    .map(|_| ())
                            })
                            .map_err(WorkbenchError::Storage)?;
                        self.slot_role_overrides
                            .insert((pid, slot), KeycapRole::ActionKey);
                    }
                } else if let Some(ref existing_id) = modal.multi_id {
                    self.document_store
                        .update(|draft| {
                            draft.save_multi(existing_id, action, additions).map(|_| ())
                        })
                        .map_err(WorkbenchError::Storage)?;
                } else {
                    let (pid, lid) = self.require_selected_profile_and_layer()?;
                    if self.selected_surface == ControlSurfaceId::EncoderHold {
                        self.document_store
                            .update(|draft| {
                                draft
                                    .save_multi_for_encoder_hold(
                                        None, action, additions, &pid, &lid,
                                    )
                                    .map(|_| ())
                            })
                            .map_err(WorkbenchError::Storage)?;
                    } else {
                        let slot = modal
                            .target_slot
                            .unwrap_or_else(|| self.current_selected_slot());
                        self.document_store
                            .update(|draft| {
                                draft
                                    .save_multi_for_binding(
                                        None,
                                        action,
                                        additions,
                                        &json!(pid),
                                        &json!(lid),
                                        slot,
                                    )
                                    .map(|_| ())
                            })
                            .map_err(WorkbenchError::Storage)?;
                    }
                }
                self.active_modal = None;
                self.sync_v070_session_sa_rgb();
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MultiBindExisting { multi_id } => {
                let (pid, lid) = self.require_selected_profile_and_layer()?;
                if self.selected_surface == ControlSurfaceId::EncoderHold {
                    self.document_store
                        .update(|draft| {
                            draft.bind_existing_encoder_hold_action(&pid, &lid, multi_id.as_deref())
                        })
                        .map_err(WorkbenchError::Storage)?;
                } else {
                    let slot = self.current_selected_slot();
                    self.document_store
                        .update(|draft| {
                            draft.bind_multi(&json!(pid), &json!(lid), slot, multi_id.as_deref())
                        })
                        .map_err(WorkbenchError::Storage)?;
                    self.slot_role_overrides
                        .insert((pid, slot), KeycapRole::ActionKey);
                    self.sync_v070_session_sa_rgb();
                }
                self.commit_runtime_update();
                Ok(())
            }
            EditorAction::MultiDelete { multi_id } => {
                self.document_store
                    .update(|draft| draft.delete_multi(&multi_id))
                    .map_err(WorkbenchError::Storage)?;
                if matches!(
                    &self.active_modal,
                    Some(EditorModalProjection::MultiEditor(m)) if m.multi_id.as_deref() == Some(multi_id.as_str())
                ) {
                    self.active_modal = None;
                }
                self.sync_v070_session_sa_rgb();
                self.commit_runtime_update();
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn take_macro_modal(&self) -> Result<MacroEditorModalProjection, WorkbenchError> {
        match &self.active_modal {
            Some(EditorModalProjection::MacroEditor(m)) => Ok(m.clone()),
            _ => Err(WorkbenchError::Storage("未打开宏编辑器".to_string())),
        }
    }

    fn take_multi_modal(&self) -> Result<MultiEditorModalProjection, WorkbenchError> {
        match &self.active_modal {
            Some(EditorModalProjection::MultiEditor(m)) => Ok(m.clone()),
            _ => Err(WorkbenchError::Storage("未打开 Multi 编辑器".to_string())),
        }
    }

    pub(super) fn build_lighting_modal_projection(
        &self,
        validation_error: Option<String>,
    ) -> Result<LightingEditorModalProjection, WorkbenchError> {
        let (pid, lid) = self.require_selected_profile_and_layer()?;
        let doc = &self.document_store.draft.document;
        let layer_name = doc
            .editable
            .graph
            .layers
            .get(&lid)
            .map(|l| l.name.clone())
            .unwrap_or_else(|| "默认层".to_string());
        let lighting_opt = doc.lighting(&lid).ok();
        let has_lighting = lighting_opt.is_some();
        let backlight = lighting_opt.as_ref().map(|l| {
            build_zone_projection(
                LightingZone::Backlight,
                "按键背光 (Backlight)",
                &l.backlight,
            )
        });
        let underglow = lighting_opt.as_ref().map(|l| {
            build_zone_projection(
                LightingZone::Underglow,
                "底灯环绕 (Underglow)",
                &l.underglow,
            )
        });
        Ok(LightingEditorModalProjection {
            profile_id: pid,
            layer_id: lid.clone(),
            layer_name,
            apply_to_all_layers: self.lighting_apply_to_all_layers,
            can_add_lighting: !doc.is_protected_oai_layer(&lid) && !has_lighting,
            has_lighting,
            backlight,
            underglow,
            preview_active: self.lighting_preview_active,
            preview_remaining_secs: self.lighting_preview_remaining_secs,
            validation_error,
        })
    }

    fn build_multi_modal_projection(
        &self,
        multi_id: Option<String>,
    ) -> Result<MultiEditorModalProjection, WorkbenchError> {
        let target_slot = Some(self.current_selected_slot());
        let available_actions: Vec<MultiTargetOptionProjection> = mi_model::draft::actions()
            .into_iter()
            .filter(|a| a.id != "local-macro" && a.id != "local-multi")
            .map(|a| MultiTargetOptionProjection {
                id: a.id,
                label: a.name,
            })
            .collect();
        let available_macros: Vec<MultiTargetOptionProjection> = self
            .document_store
            .draft
            .macros()
            .into_iter()
            .map(|m| MultiTargetOptionProjection {
                id: m.id,
                label: m.action.name,
            })
            .collect();

        if let Some(id) = multi_id {
            let act = self
                .document_store
                .draft
                .document
                .editable
                .graph
                .actions
                .get(&id)
                .ok_or_else(|| WorkbenchError::Storage("Multi 动作不存在".to_string()))?;
            let ActionKind::LocalMulti { multi } = serde_json::from_value(act.payload.clone())
                .map_err(|_| WorkbenchError::Storage("不是有效的 Multi 动作".to_string()))?
            else {
                return Err(WorkbenchError::Storage("不是有效的 Multi 动作".to_string()));
            };
            let refs = [&multi.tap, &multi.double_tap, &multi.hold, &multi.tap_hold];
            let branches: [MultiBranchSlotProjection; 4] = std::array::from_fn(|idx| {
                let kind = MultiBranchKind::ALL[idx];
                let target = refs[idx].as_ref().map(|r| match r {
                    ActionReference::Action { action_id } => MultiBranchTarget::Action {
                        action_id: action_id.clone(),
                    },
                    ActionReference::Macro { action_id } => MultiBranchTarget::Macro {
                        action_id: action_id.clone(),
                    },
                });
                let display = target
                    .as_ref()
                    .map(|t| self.resolve_multi_target_display_name(t))
                    .unwrap_or_else(|| "未设置".to_string());
                MultiBranchSlotProjection {
                    branch: kind,
                    title: kind.title().to_string(),
                    target,
                    target_display_name: display,
                }
            });
            let reference_summaries = self
                .document_store
                .draft
                .multis()
                .into_iter()
                .find(|m| m.id == id)
                .map(|m| {
                    m.references
                        .iter()
                        .map(|r| {
                            format!(
                                "{} / {} · {}",
                                r.profile_name, r.layer_name, r.logical_control
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            Ok(MultiEditorModalProjection {
                multi_id: Some(id),
                name: act.name.clone(),
                term_ms: multi.term_ms,
                branches,
                bind_to_selected_slot: true,
                target_slot,
                available_actions,
                available_macros,
                reference_summaries,
                validation_error: None,
            })
        } else {
            let branches: [MultiBranchSlotProjection; 4] = std::array::from_fn(|idx| {
                let kind = MultiBranchKind::ALL[idx];
                MultiBranchSlotProjection {
                    branch: kind,
                    title: kind.title().to_string(),
                    target: None,
                    target_display_name: "未设置".to_string(),
                }
            });
            Ok(MultiEditorModalProjection {
                multi_id: None,
                name: "新建 Multi".to_string(),
                term_ms: 200,
                branches,
                bind_to_selected_slot: true,
                target_slot,
                available_actions,
                available_macros,
                reference_summaries: Vec::new(),
                validation_error: None,
            })
        }
    }

    fn resolve_multi_target_display_name(&self, target: &MultiBranchTarget) -> String {
        let action_id = match target {
            MultiBranchTarget::Action { action_id } | MultiBranchTarget::Macro { action_id } => {
                action_id
            }
        };
        if let Some(a) = self
            .document_store
            .draft
            .document
            .editable
            .graph
            .actions
            .get(action_id)
        {
            return a.name.clone();
        }
        if let Some(a) = self
            .document_store
            .draft
            .document
            .editable
            .graph
            .actions
            .values()
            .find(|a| a.id == *action_id)
        {
            return a.name.clone();
        }
        mi_model::draft::actions()
            .into_iter()
            .find(|a| a.id == *action_id)
            .map(|a| a.name)
            .unwrap_or_else(|| action_id.clone())
    }

    fn resolve_multi_branch_reference(
        &self,
        target: &MultiBranchTarget,
        additions: &mut BTreeMap<String, Action>,
    ) -> Result<ActionReference, WorkbenchError> {
        let graph_actions = &self.document_store.draft.document.editable.graph.actions;
        match target {
            MultiBranchTarget::Macro { action_id } => {
                if !graph_actions.contains_key(action_id) {
                    return Err(WorkbenchError::Storage(format!(
                        "未找到引用的宏 {action_id}"
                    )));
                }
                Ok(ActionReference::Macro {
                    action_id: action_id.clone(),
                })
            }
            MultiBranchTarget::Action { action_id } => {
                if graph_actions.contains_key(action_id) {
                    return Ok(ActionReference::Action {
                        action_id: action_id.clone(),
                    });
                }
                if let Some((existing_uuid, _)) =
                    graph_actions.iter().find(|(_, a)| a.id == *action_id)
                {
                    return Ok(ActionReference::Action {
                        action_id: existing_uuid.clone(),
                    });
                }
                if let Some((added_uuid, _)) = additions.iter().find(|(_, a)| a.id == *action_id) {
                    return Ok(ActionReference::Action {
                        action_id: added_uuid.clone(),
                    });
                }
                let catalog_action = self.resolve_action_by_id(action_id)?;
                let new_uuid = mi_model::document::new_id();
                additions.insert(new_uuid.clone(), catalog_action);
                Ok(ActionReference::Action {
                    action_id: new_uuid,
                })
            }
        }
    }
}

fn matches_logical_slot(logical_control: &str, slot: u32) -> bool {
    logical_control
        .strip_prefix("logical-")
        .and_then(|s| s.parse::<u32>().ok())
        == Some(slot)
}

pub(super) fn resolve_slot_graph_action_id(layer: Option<&LayerNode>, slot: u32) -> Option<String> {
    let layer = layer?;
    layer
        .controls
        .values()
        .find(|b| matches_logical_slot(&b.logical_control, slot))
        .and_then(|b| b.action_id.clone())
}

pub(crate) fn format_macro_references(
    refs: &[mi_model::macro_library::MacroReference],
    locale: mi_model::ResolvedLocale,
) -> String {
    if refs.is_empty() {
        return locale.select("未绑定", "Unbound").to_string();
    }
    let sep = locale.select("、", ", ");
    refs.iter()
        .map(|r| {
            format!(
                "{} / {} · {}",
                mi_model::i18n::localize_profile_name(&r.profile_name, locale),
                mi_model::i18n::localize_layer_name(&r.layer_name, locale),
                r.logical_control
            )
        })
        .collect::<Vec<_>>()
        .join(sep)
}

fn is_supported_macro_key(key: &str) -> bool {
    let trimmed = key.trim();
    !trimmed.is_empty() && trimmed.len() <= 256
}

fn build_macro_step_drafts(events: &[MacroEvent]) -> Vec<MacroStepDraft> {
    events
        .iter()
        .enumerate()
        .map(|(index, event)| {
            let (summary_label, macos_key_supported) = match event {
                MacroEvent::Press { key } => (
                    format!("按下 {}", key.to_uppercase()),
                    is_supported_macro_key(key),
                ),
                MacroEvent::Release { key } => (
                    format!("释放 {}", key.to_uppercase()),
                    is_supported_macro_key(key),
                ),
                MacroEvent::Delay { ms } => (format!("等待 {ms} ms"), *ms > 0),
            };
            MacroStepDraft {
                index,
                event: event.clone(),
                summary_label,
                macos_key_supported,
            }
        })
        .collect()
}

fn build_macro_editor_projection(
    macro_id: Option<String>,
    name: String,
    events: Vec<MacroEvent>,
    target_slot: Option<u32>,
    reference_summaries: Vec<String>,
) -> MacroEditorModalProjection {
    let steps = build_macro_step_drafts(&events);
    let host_execution_supported = steps.iter().all(|s| s.macos_key_supported);
    MacroEditorModalProjection {
        macro_id,
        name,
        steps,
        draft_key_input: "a".to_string(),
        draft_delay_ms: 30,
        bind_to_selected_slot: true,
        target_slot,
        reference_summaries,
        validation_error: None,
        host_execution_supported,
    }
}

fn rebuild_macro_modal_with_events(
    mut modal: MacroEditorModalProjection,
    events: Vec<MacroEvent>,
) -> MacroEditorModalProjection {
    let steps = build_macro_step_drafts(&events);
    modal.host_execution_supported = steps.iter().all(|s| s.macos_key_supported);
    modal.steps = steps;
    modal.validation_error = None;
    modal
}

fn ensure_layer_has_base_lighting(
    draft: &mut Draft,
    pid: &str,
    lid: &str,
    all_layers: bool,
) -> Result<(), String> {
    let target_lids: Vec<String> = if all_layers {
        draft
            .document
            .editable
            .graph
            .profiles
            .get(pid)
            .map(|p| {
                p.layer_order
                    .iter()
                    .filter(|id| !draft.document.is_protected_oai_layer(id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_else(|| vec![lid.to_string()])
    } else {
        vec![lid.to_string()]
    };

    for target_lid in target_lids {
        if draft.document.lighting(&target_lid).is_ok() {
            continue;
        }
        if draft.document.can_add_lighting(&target_lid) {
            draft.add_lighting(&target_lid)?;
        } else {
            draft.mutate(|doc| {
                if let Some(layer) = doc.editable.graph.layers.get_mut(&target_lid) {
                    layer.lighting = Some(Lighting::creator_micro_2_defaults());
                }
                Ok(())
            })?;
        }
    }
    Ok(())
}

fn build_zone_projection(
    zone: LightingZone,
    zone_title: &str,
    raw: &Value,
) -> LightingZoneProjection {
    let effect: Option<LightingEffect> = raw
        .get("effect")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let brightness_percent = raw
        .get("brightness")
        .and_then(|v| v.as_f64())
        .map(|f| (f.clamp(0.0, 1.0) * 100.0).round() as u8)
        .unwrap_or(100);
    let speed_percent = raw
        .get("speed")
        .and_then(|v| v.as_f64())
        .map(|f| (f.clamp(0.0, 1.0) * 100.0).round() as u8)
        .unwrap_or(50);
    let magic_percent = raw
        .get("magic")
        .and_then(|v| v.as_f64())
        .map(|f| (f.clamp(0.0, 1.0) * 100.0).round() as u8)
        .unwrap_or(50);
    let color_u32 = raw
        .get("color")
        .and_then(|v| v.as_u64())
        .map(|n| (n as u32) & 0x00FF_FFFF)
        .unwrap_or(0x00FF_FFFF);
    let color_hex = format!("#{color_u32:06X}");
    let (can_edit_color, can_edit_brightness, can_edit_speed, can_edit_magic) = match effect {
        Some(LightingEffect::Off) => (false, false, false, false),
        Some(LightingEffect::Solid) => (true, true, false, false),
        Some(LightingEffect::Gradient) => (true, true, false, true),
        Some(LightingEffect::Snake) => (true, true, true, false),
        Some(LightingEffect::Rainbow) => (false, true, true, true),
        Some(LightingEffect::Breath) => (true, true, true, true),
        None => (false, false, false, false),
    };
    LightingZoneProjection {
        zone,
        zone_title: zone_title.to_string(),
        effect,
        brightness_percent,
        speed_percent,
        magic_percent,
        color_hex,
        can_edit_color,
        can_edit_brightness,
        can_edit_speed,
        can_edit_magic,
    }
}

fn parse_hex_color(input: &str) -> Result<u32, String> {
    let trimmed = input.trim();
    let hex = trimmed.strip_prefix('#').unwrap_or(trimmed);
    if hex.len() != 6 {
        return Err(format!("颜色格式无效（需为 #RRGGBB）：{input}"));
    }
    u32::from_str_radix(hex, 16).map_err(|_| format!("颜色十六进制无效：{input}"))
}

fn build_comparison_modal_projection(
    cmp: &mi_model::comparison::Comparison,
) -> ReconnectComparisonModalProjection {
    let rows = cmp
        .rows
        .iter()
        .map(|r| ComparisonRowProjection {
            path: r.path.clone(),
            label: r.label.clone(),
            base_summary: r.base_summary.clone(),
            local_summary: r.local_summary.clone(),
            device_summary: r.device_summary.clone(),
            result_badge: r.result.clone(),
            is_conflict: r.result.contains("冲突"),
        })
        .collect();
    ReconnectComparisonModalProjection {
        token: cmp.token.clone(),
        revision: cmp.revision,
        identity_matches: cmp.identity_matches,
        identity_conflict: cmp.identity_conflict,
        identity_confirmation_required: cmp.identity_confirmation_required,
        identity_user_confirmed: cmp.identity_user_confirmed,
        can_merge: cmp.can_merge(),
        conflicts: cmp.conflicts.clone(),
        rows,
        error_message: None,
    }
}

fn build_apply_modal_projection(
    preview: &mi_model::apply::Preview,
) -> ApplyPreparationModalProjection {
    let changes = preview
        .changes
        .iter()
        .map(|c| {
            let kind = c
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("change")
                .to_string();
            let location_label = c
                .get("control")
                .or_else(|| c.get("zone"))
                .or_else(|| c.get("field"))
                .and_then(|v| v.as_str())
                .unwrap_or("layer")
                .to_string();
            let before = c
                .get("before")
                .map(Value::to_string)
                .unwrap_or_else(|| "null".to_string());
            let after = c
                .get("after")
                .map(Value::to_string)
                .unwrap_or_else(|| "null".to_string());
            ApplyChangeRowProjection {
                kind,
                location_label,
                before,
                after,
            }
        })
        .collect();
    ApplyPreparationModalProjection {
        token: preview.token.clone(),
        kind: preview.kind.clone(),
        document_id: preview.document_id.clone(),
        revision: preview.revision,
        document_sha256: preview.document_sha256.clone(),
        planned_keymap_sha256: preview.planned_keymap_sha256.clone(),
        before_summary: preview.before.clone(),
        after_summary: preview.after.clone(),
        changes,
        backup_prepared: false,
        backup_path: None,
        manifest_sha256: None,
        candidate_taken: false,
        candidate_operation_id: None,
        unavailable_reason: preview.unavailable_reason.clone(),
        error_message: None,
    }
}

pub(super) fn resolve_slot_action_info<'a>(
    doc: &'a LocalDocumentV2,
    layer: Option<&'a LayerNode>,
    slot: u32,
) -> (String, Option<&'a Action>) {
    let Some(layer) = layer else {
        return ("未绑定".to_string(), None);
    };
    let Some(binding) = layer
        .controls
        .values()
        .find(|b| matches_logical_slot(&b.logical_control, slot))
    else {
        return ("未绑定".to_string(), None);
    };
    if let Some(aid) = &binding.action_id {
        if let Some(action) = doc.editable.graph.actions.get(aid) {
            return (action.name.clone(), Some(action));
        }
    }
    if let Some(raw_token) = binding.original.as_str() {
        if raw_token != "Empty" && !raw_token.is_empty() {
            return (raw_token.to_string(), None);
        }
    }
    ("未绑定".to_string(), None)
}

pub(super) fn resolve_merged_bottom_slot(doc: &LocalDocumentV2, layer: Option<&LayerNode>) -> u32 {
    let (_, act10) = resolve_slot_action_info(doc, layer, 10);
    let (_, act11) = resolve_slot_action_info(doc, layer, 11);
    let cmd10 = act10.and_then(|a| a.payload.get("command").and_then(|v| v.as_str()));
    let cmd11 = act11.and_then(|a| a.payload.get("command").and_then(|v| v.as_str()));
    if matches!(cmd10, Some("focus-input" | "new-conversation-in-project"))
        && matches!(cmd11, Some("push-to-talk" | "dictation"))
    {
        return 11;
    }
    if act10.is_some() {
        return 10;
    }
    if act11.is_some() {
        return 11;
    }
    10
}

pub(super) fn resolve_silkscreen_icon(slot: u32, action: Option<&Action>) -> String {
    let Some(a) = action else {
        return "□".to_string();
    };
    let cmd = a.payload.get("command").and_then(|v| v.as_str());
    if a.id == "agent-subagent"
        || a.id.contains("subagent")
        || matches!(cmd, Some("subagent" | "subagent-open"))
    {
        return "⑃".to_string();
    }
    if a.id.starts_with("host-mouse-click") || cmd.is_some_and(|c| c.starts_with("mouse-click:")) {
        return "🖱".to_string();
    }
    if a.id == "ag-live-voice" || a.id == "desktop-live-voice" || cmd == Some("live-voice") {
        return "🎙".to_string();
    }
    if cmd == Some("new-conversation-in-project") {
        return "＋".to_string();
    }
    if a.id.contains("dictation")
        || a.id.contains("ptt")
        || a.id == "apple-fn-key"
        || cmd == Some("push-to-talk")
    {
        return "🎤".to_string();
    }
    if a.id.contains("submit") {
        return "⏎".to_string();
    }
    if a.id.contains("side-panel") || matches!(cmd, Some("open-side-panel" | "close-side-panel")) {
        return "◫".to_string();
    }
    if a.id.contains("slot-open") || a.id.contains("cockpit-slot") {
        return "💬".to_string();
    }
    match slot {
        0..=5 => "💬".to_string(),
        10 | 11 => "🎤".to_string(),
        12 => "⏎".to_string(),
        _ => "⌘".to_string(),
    }
}

pub(super) fn custom_monogram(name: &str) -> String {
    let trimmed = name.trim();
    let mut chars = trimmed.chars();
    match (chars.next(), chars.next()) {
        (Some(a), Some(b)) => format!("{}{}", a.to_uppercase(), b.to_uppercase()),
        (Some(a), None) => a.to_uppercase().to_string(),
        _ => "方案".to_string(),
    }
}

pub(super) fn bundle_id_for_place(place: &NavigationPlace) -> &str {
    match place {
        NavigationPlace::App { bundle_id } => bundle_id.as_str(),
        other => agent_for_place(other).bundle_id(),
    }
}

pub(super) fn fallback_unbridged_oai_action(
    profile_name: &str,
    binding_id: &str,
    layout: BottomRowLayout,
) -> Option<ResolvedHostAction> {
    let normalized = binding_id.trim().to_ascii_uppercase();
    let effective = if layout == BottomRowLayout::Merged && normalized == "AG11" {
        "AG10"
    } else {
        normalized.as_str()
    };
    let (adapter, bundle_id) = if profile_name.eq_ignore_ascii_case("Codex") {
        ("codex", "com.openai.codex")
    } else if profile_name.eq_ignore_ascii_case("ChatGPT") {
        ("chatgpt", "com.openai.atlas")
    } else {
        return None;
    };
    match effective {
        "AG00" => Some(ResolvedHostAction::OpenApp {
            bundle_id: bundle_id.to_string(),
        }),
        "AG01" => Some(ResolvedHostAction::AgentCommand {
            adapter: adapter.to_string(),
            command: "new-chat".to_string(),
        }),
        "AG02" => Some(ResolvedHostAction::AgentCommand {
            adapter: adapter.to_string(),
            command: "search".to_string(),
        }),
        "AG03" => Some(ResolvedHostAction::AgentCommand {
            adapter: adapter.to_string(),
            command: if adapter == "codex" {
                "effort".to_string()
            } else {
                "model".to_string()
            },
        }),
        "AG05" if adapter == "codex" => Some(ResolvedHostAction::AgentCommand {
            adapter: "codex".to_string(),
            command: "terminal".to_string(),
        }),
        "AG10" => Some(ResolvedHostAction::AgentCommand {
            adapter: adapter.to_string(),
            command: if layout == BottomRowLayout::Merged {
                "push-to-talk".to_string()
            } else {
                "focus-input".to_string()
            },
        }),
        "AG11" => Some(ResolvedHostAction::AgentCommand {
            adapter: adapter.to_string(),
            command: "push-to-talk".to_string(),
        }),
        "AG12" | "JS_CLK" => Some(ResolvedHostAction::AgentCommand {
            adapter: adapter.to_string(),
            command: "submit".to_string(),
        }),
        "AG13" | "ENC_CCW" => Some(ResolvedHostAction::AgentCommand {
            adapter: adapter.to_string(),
            command: "scroll-up".to_string(),
        }),
        "AG14" | "ENC_CW" => Some(ResolvedHostAction::AgentCommand {
            adapter: adapter.to_string(),
            command: "scroll-down".to_string(),
        }),
        "ENC_CLK" => Some(ResolvedHostAction::AgentCommand {
            adapter: adapter.to_string(),
            command: if adapter == "codex" {
                "model-picker-open".to_string()
            } else {
                "scroll-to-bottom".to_string()
            },
        }),
        _ => None,
    }
}
