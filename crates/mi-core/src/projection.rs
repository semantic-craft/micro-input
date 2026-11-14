use super::*;
use crate::six_layer_runtime::InstallationStatus;
use mi_model::apply::AppliedSixLayerPlan;

pub(super) struct SixLayerView {
    pub configuration_observed: bool,
    pub keyboard_grabbed: bool,
    pub first_event_verified: bool,
    pub armed: bool,
    pub observed_layer: u8,
    pub failure: Option<String>,
    pub effective_profile_id: Option<String>,
    pub foreground_priority_adapter: Option<String>,
}

pub(super) struct ReusableInspectorCatalogs {
    pub keycaps: Option<Vec<KeycapCatalogItemProjection>>,
    pub categories: Option<Vec<String>>,
    pub actions: Option<Vec<ActionLibraryRowProjection>>,
}

pub(super) struct ProjectionInputs<'a> {
    pub accessibility_trusted: bool,
    pub reusable_catalogs: ReusableInspectorCatalogs,
    pub action_category_filter: &'a String,
    pub action_search_query: &'a String,
    pub active_modal: &'a Option<EditorModalProjection>,
    pub applications: &'a BTreeMap<String, bool>,
    pub auto_follow_enabled: bool,
    pub banner_flash_count: usize,
    pub bind_keycap_default_action_on_select: bool,
    pub board_routing: BoardRouting,
    pub bottom_row_layout: BottomRowLayout,
    pub busy: bool,
    pub command_execution_enabled: bool,
    pub competing_input_app_running: bool,
    pub connected_candidate: &'a Option<mi_transport::HidCandidate>,
    pub device_connected: bool,
    pub document_store: &'a DocumentStore,
    pub favorite_action_ids: &'a BTreeSet<String>,
    pub favorites_only: bool,
    pub hardware_rpc_verified: bool,
    pub isolated: bool,
    pub keycap_overrides: &'a BTreeMap<(String, u32), CodexKeycapId>,
    pub keycap_search_query: &'a String,
    pub last_cursor_position: ScreenPoint,
    pub last_exported_preset: &'a Option<String>,
    pub last_switch_duration: Duration,
    pub acknowledged_watermarks: &'a BTreeMap<String, String>,
    pub full_keypad_feedback: &'a [mi_bridge::PerKeyLightingFeedback],
    pub dual_zone_feedback: &'a Option<mi_bridge::DualZoneLightingFeedback>,
    pub lighting_popover: &'a SimpleLightingPopoverProjection,
    pub manual_override: bool,
    pub navigation_history: &'a NavigationHistory,
    pub open_drawers: &'a Vec<InspectorDrawer>,
    pub overlay_controller: &'a OverlayController,
    pub pending_preset_export_scope: Option<PresetExportScope>,
    pub pending_preset_import: &'a Option<PendingPresetImportPreview>,
    pub pinned_favorites: &'a PinnedFavoritesGrid,
    pub preset_transfer_error: &'a Option<String>,
    pub ptt_active: bool,
    pub radial_readiness: &'a RadialInputReadiness,
    pub radial_slot_configs: &'a [RadialSlotConfig; RADIAL_SECTOR_COUNT],
    pub radial_state: &'a RadialMenuState,
    pub screen_geometry: &'a ScreenGeometry,
    pub selected_binding_slot: Option<u32>,
    pub selected_joystick_direction: JoystickDirection8,
    pub selected_layer_id: &'a Option<String>,
    pub selected_profile_id: &'a Option<String>,
    pub selected_surface: ControlSurfaceId,
    pub shortcut_recorder: &'a ShortcutRecorderState,
    pub show_keycap_icons: bool,
    pub show_keycap_labels: bool,
    pub show_other_profiles: bool,
    pub six_layer_busy: bool,
    pub six_layer_running_present: bool,
    pub six_layer_state: &'a crate::six_layer_runtime::SavedSixLayerState,
    pub six_layer_view: &'a Option<SixLayerView>,
    pub slot_role_overrides: &'a BTreeMap<(String, u32), KeycapRole>,
    pub slot_table: &'a FourAppSlotTable,
    pub status_menu_open: bool,
    pub status_message: &'a String,
}

impl ProjectionInputs<'_> {
    pub(super) fn active_canonical_agent(&self) -> Option<CanonicalAgent> {
        let pid = self.resolved_active_profile_id()?;
        let profile = self
            .document_store
            .host_store()
            .draft
            .document
            .editable
            .graph
            .profiles
            .get(&pid)?;
        CanonicalAgent::from_profile_name(&profile.name)
    }

    pub(super) fn active_desktop_agent_adapter(&self) -> Option<String> {
        let agent = self.active_canonical_agent()?;
        match agent {
            CanonicalAgent::Antigravity => Some("antigravity".to_string()),
            CanonicalAgent::Codex => Some("codex".to_string()),
            CanonicalAgent::DeepseekHarness => Some("dsh-desktop".to_string()),
            CanonicalAgent::Herdr => Some("herdr".to_string()),
            CanonicalAgent::OtherApps => None,
        }
    }

    pub(super) fn active_profile_display_name(&self) -> String {
        let Some(pid) = self.resolved_active_profile_id() else {
            return "未选择方案".to_string();
        };
        let Some(profile) = self
            .document_store
            .host_store()
            .draft
            .document
            .editable
            .graph
            .profiles
            .get(&pid)
        else {
            return "未选择方案".to_string();
        };
        CanonicalAgent::from_profile_name(&profile.name)
            .map(|a| a.display_name().to_string())
            .unwrap_or_else(|| profile.name.clone())
    }

    pub(super) fn build_board_layers_projection(&self) -> BoardLayersProjection {
        let doc = &self.document_store.host_store().draft.document;
        let plan = self
            .document_store
            .board_layer_plan()
            .cloned()
            .unwrap_or_default();
        let preview = self.document_store.board_preview_selection();
        let mut result = BoardLayersProjection::default();
        result.layers = std::array::from_fn(|i| {
            let layer = BoardLayer::ALL[i];
            let route = plan.route(layer);
            let id = route.profile_id();
            let profile = id.and_then(|id| doc.editable.graph.profiles.get(id));
            let issue = match route {
                BoardLayerRoute::Unassigned => None,
                _ if profile.is_none() => Some("方案不存在，原引用已保留".into()),
                _ if id.and_then(|id| self.route_for_profile(id).ok()).as_ref() != Some(route) => {
                    Some("方案归属与保存的意图不一致，不能执行".into())
                }
                BoardLayerRoute::Native { .. } => {
                    Some("保留 OAI 映射；USB 独占后由 Micro Input 当前方案控制".into())
                }
                _ => None,
            };
            BoardLayerProjection {
                layer: layer.number(),
                leds: layer.leds(),
                profile_id: id.map(str::to_owned),
                display_name: profile.map(|p| p.name.clone()).unwrap_or_else(|| {
                    if id.is_some() {
                        "缺失的方案".into()
                    } else {
                        "未分配".into()
                    }
                }),
                owner: match route {
                    BoardLayerRoute::Unassigned => "Unassigned",
                    BoardLayerRoute::HostProfile { .. } => "MicroInput",
                    BoardLayerRoute::Native {
                        owner: NativeLayerOwner::Codex,
                        ..
                    } => "Codex",
                    BoardLayerRoute::Native {
                        owner: NativeLayerOwner::Retired,
                        ..
                    } => "Unavailable",
                }
                .into(),
                is_preview_selected: preview == BoardPreviewSelection::Layer(layer),
                assignment_issue: issue,
            }
        });
        result.profile_choices = doc
            .editable
            .graph
            .profile_order
            .iter()
            .filter_map(|id| {
                self.route_for_profile(id).ok()?;
                Some(BoardProfileChoice {
                    profile_id: id.clone(),
                    display_name: doc.editable.graph.profiles.get(id)?.name.clone(),
                })
            })
            .collect();
        result.usb_startup_profile_id = self
            .document_store
            .usb_startup_profile_id()
            .map(str::to_owned);
        result.usb_startup_display_name = result
            .usb_startup_profile_id
            .as_ref()
            .map(|id| {
                doc.editable
                    .graph
                    .profiles
                    .get(id)
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| "缺失的方案（原引用保留）".into())
            })
            .unwrap_or_else(|| "未分配".into());
        result.usb_preview_selected = preview == BoardPreviewSelection::UsbStartup;
        if let BoardRouting::Managed { layer } = self.board_routing {
            if self.device_connected && self.hardware_rpc_verified {
                result.observed_layer = Some(layer);
                result.observed_profile_kind = Some("legacy_three".into());
                result.observed_status = format!("已确认旧三层，第 {layer} 层");
                if self.legacy_host_profile_id(layer).is_none() {
                    result
                        .observed_status
                        .push_str(" · 绑定缺失或属于原生应用，按键已暂停");
                }
            }
        }
        self.project_six_layer_runtime(&mut result);
        result
    }

    pub(super) fn build_canvas_projection(&self, can_edit_draft: bool) -> CanvasProjection {
        let doc = &self.document_store.draft.document;
        let graph = &doc.editable.graph;
        let profile = self
            .selected_profile_id
            .as_ref()
            .and_then(|pid| graph.profiles.get(pid));
        let profile_display_name = profile
            .map(|p| {
                CanonicalAgent::from_profile_name(&p.name)
                    .map(|a| a.display_name().to_string())
                    .unwrap_or_else(|| p.name.clone())
            })
            .unwrap_or_default();

        let layer_tabs: Vec<CanvasLayerTab> = profile
            .map(|p| {
                p.layer_order
                    .iter()
                    .filter_map(|lid| {
                        let l = graph.layers.get(lid)?;
                        Some(CanvasLayerTab {
                            layer_id: lid.clone(),
                            name: l.name.clone(),
                            is_selected: self.selected_layer_id.as_deref() == Some(lid.as_str()),
                            is_protected_oai: doc.is_protected_oai_layer(lid),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let layer = self
            .selected_layer_id
            .as_ref()
            .and_then(|lid| graph.layers.get(lid));
        let bottom_row_layout = self.current_bottom_row_layout();

        let is_protected = self.is_selected_layer_protected();
        let can_edit_bindings = can_edit_draft && layer.is_some() && !is_protected;
        let can_add_lighting = can_edit_draft && layer.is_some() && !is_protected;
        let can_export_or_copy_layer = layer.is_some() && !is_protected;
        let inspector_expanded = !is_protected;

        let protected_lock = if is_protected {
            let lid = self.selected_layer_id.clone().unwrap_or_default();
            let layer_name = layer
                .map(|l| l.name.clone())
                .unwrap_or_else(|| "ChatGPT".to_string());
            let owner_client_name = profile
                .map(|p| p.name.clone())
                .unwrap_or_else(|| "ChatGPT / Codex".to_string());
            Some(ProtectedLayerLockProjection {
                is_locked: true,
                layer_id: lid,
                layer_name,
                owner_client_name,
                lock_badge_text: "只读保护层 · 保留原生映射 (KV_OAI_*)".to_string(),
                explanation_text:
                    "保留此层的 OAI 固件映射；USB 独占后由 Micro Input 当前方案处理输入。按键映射、预设覆盖与灯光编辑保持只读保护。"
                        .to_string(),
                unlock_button_title: "新建并切换到自定义层".to_string(),
            })
        } else {
            None
        };

        let step = KEY_1U_SIZE + KEY_GAP;
        let indexed_controls = index_layer_controls(layer);
        let encoder_bounds =
            CanvasRect::new(CANVAS_PADDING, CANVAS_PADDING, KEY_1U_SIZE, KEY_1U_SIZE);
        let encoder = EncoderDialProjection {
            bounds: encoder_bounds,
            ccw_bounds: CanvasRect::new(CANVAS_PADDING, CANVAS_PADDING, 22.0, KEY_1U_SIZE),
            cw_bounds: CanvasRect::new(
                CANVAS_PADDING + KEY_1U_SIZE - 22.0,
                CANVAS_PADDING,
                22.0,
                KEY_1U_SIZE,
            ),
            press_bounds: CanvasRect::new(CANVAS_PADDING + 22.0, CANVAS_PADDING + 22.0, 24.0, 24.0),
            ccw_label: resolve_indexed_slot(doc, &indexed_controls, 13).0,
            cw_label: resolve_indexed_slot(doc, &indexed_controls, 14).0,
            press_label: format_concise_keycap_caption(
                &resolve_indexed_slot(doc, &indexed_controls, 15).0,
            ),
            selected_slot: match self.selected_surface {
                ControlSurfaceId::EncoderCcw => Some(13),
                ControlSurfaceId::EncoderCw => Some(14),
                ControlSurfaceId::EncoderPress => Some(15),
                _ => None,
            },
            ring_glow_hex: if is_protected {
                "#222222".to_string()
            } else {
                "#0088FF".to_string()
            },
        };

        let joy_x = CANVAS_PADDING + step * 3.0;
        let joy_y = CANVAS_PADDING;
        let joy_bounds = CanvasRect::new(joy_x, joy_y, KEY_1U_SIZE, KEY_1U_SIZE);
        let (jcx, jcy) = joy_bounds.center();
        let sectors: Vec<JoystickDirectionBadge> = JoystickDirection8::ALL
            .into_iter()
            .map(|dir| {
                let (ux, uy) = dir.unit_offset();
                let bx = jcx + ux * 24.0;
                let by = jcy + uy * 24.0;
                let logical_slot = dir.logical_sector_slot();
                let full_title = resolve_indexed_slot(doc, &indexed_controls, logical_slot).0;
                JoystickDirectionBadge {
                    direction: dir,
                    logical_slot,
                    bounds: CanvasRect::new(bx - 6.0, by - 6.0, 12.0, 12.0),
                    concise_label: format_concise_keycap_caption(&full_title),
                    icon_token: Some(dir.label().to_string()),
                    is_selected: self.selected_surface == ControlSurfaceId::Joystick
                        && self.selected_joystick_direction == dir,
                }
            })
            .collect();

        let joystick = JoystickPadProjection {
            bounds: joy_bounds,
            center_deadzone_radius: KEY_1U_SIZE * 0.20,
            mode_label: "8向摇杆".to_string(),
            active_direction: self.selected_joystick_direction,
            sectors,
            is_selected: self.selected_surface == ControlSurfaceId::Joystick,
        };

        let key_surfaces = [
            (ControlSurfaceId::Key0, 0u8, 1u8),
            (ControlSurfaceId::Key1, 0, 2),
            (ControlSurfaceId::Key2, 1, 0),
            (ControlSurfaceId::Key3, 1, 1),
            (ControlSurfaceId::Key4, 1, 2),
            (ControlSurfaceId::Key5, 1, 3),
            (ControlSurfaceId::Key6, 2, 0),
            (ControlSurfaceId::Key7, 2, 1),
            (ControlSurfaceId::Key8, 2, 2),
            (ControlSurfaceId::Key9, 2, 3),
            (ControlSurfaceId::Key10, 3, 1),
            (ControlSurfaceId::Key11, 3, 2),
            (ControlSurfaceId::Key12, 3, 3),
        ];

        let mut keycap_cells = Vec::with_capacity(13);
        for (surface, row, col) in key_surfaces {
            if surface == ControlSurfaceId::Key11 && bottom_row_layout == BottomRowLayout::Merged {
                continue;
            }
            let span_cols = if surface == ControlSurfaceId::Key10
                && bottom_row_layout == BottomRowLayout::Merged
            {
                2u8
            } else {
                1u8
            };
            let width = if span_cols == 2 {
                KEY_2U_WIDTH
            } else {
                KEY_1U_SIZE
            };
            let x = CANVAS_PADDING + (col as f32) * step;
            let y = CANVAS_PADDING + (row as f32) * step;
            let primary_slot = surface.primary_logical_slot();
            let effective_slot = if surface == ControlSurfaceId::Key10
                && bottom_row_layout == BottomRowLayout::Merged
            {
                resolve_indexed_merged_bottom_slot(doc, &indexed_controls)
            } else {
                primary_slot
            };
            let (full_action_title, action_obj) =
                resolve_indexed_slot(doc, &indexed_controls, effective_slot);
            let override_keycap = if !self.keycap_overrides.is_empty() {
                self.selected_profile_id
                    .as_ref()
                    .and_then(|pid| self.keycap_overrides.get(&(pid.clone(), effective_slot)))
                    .copied()
            } else {
                None
            };
            let silkscreen_icon = Some(
                override_keycap
                    .map(|k| k.glyph_symbol().to_string())
                    .unwrap_or_else(|| resolve_silkscreen_icon(effective_slot, action_obj)),
            );
            let concise_caption = format_concise_keycap_caption(&full_action_title);
            let is_session_slot = (0..6u32)
                .contains(&primary_slot)
                .then_some(primary_slot as usize)
                .filter(|&slot_idx| {
                    action_obj.is_some_and(|a| is_session_slot_action(a, slot_idx))
                });
            let keycap_enum = override_keycap
                .unwrap_or_else(|| CodexKeycapId::infer_for_slot(effective_slot, action_obj));
            let keycap_id = format!("{keycap_enum:?}");
            let cell_role = if !(0..6u32).contains(&primary_slot) {
                KeycapRole::ActionKey
            } else if is_protected {
                KeycapRole::TaskKey
            } else if let Some(pid) = self
                .selected_profile_id
                .as_ref()
                .filter(|_| !self.slot_role_overrides.is_empty())
            {
                self.slot_role_overrides
                    .get(&(pid.clone(), primary_slot))
                    .copied()
                    .unwrap_or(if is_session_slot.is_some() {
                        KeycapRole::TaskKey
                    } else {
                        KeycapRole::ActionKey
                    })
            } else if is_session_slot.is_some() {
                KeycapRole::TaskKey
            } else {
                KeycapRole::ActionKey
            };
            let slot_role = match cell_role {
                KeycapRole::TaskKey => "TaskKey".to_string(),
                KeycapRole::ActionKey => "ActionKey".to_string(),
            };
            let binding_id = surface.binding_id().to_string();
            let per_key_led_hex = if is_protected || !self.lighting_popover.lighting_enabled {
                "#000000".to_string()
            } else {
                self.full_keypad_feedback
                    .iter()
                    .find(|item| item.binding_id == binding_id)
                    .map(|item| item.color.clone())
                    .unwrap_or_else(|| self.lighting_popover.ambient_color.hex().to_string())
            };

            keycap_cells.push(ControlCellProjection {
                surface,
                primary_slot,
                effective_slot,
                binding_id,
                row,
                col,
                span_cols,
                bounds: CanvasRect::new(x, y, width, KEY_1U_SIZE),
                silkscreen_icon,
                keycap_id,
                slot_role,
                concise_caption,
                full_action_title,
                is_session_slot,
                is_selected: self.selected_surface == surface,
                is_available: action_obj.is_some() || is_protected,
                per_key_led_hex,
            });
        }

        let underglow_hex = if is_protected || !self.lighting_popover.lighting_enabled {
            "#000000".to_string()
        } else {
            self.dual_zone_feedback
                .as_ref()
                .map(|d| d.underglow_color.clone())
                .unwrap_or_else(|| self.lighting_popover.ambient_color.hex().to_string())
        };

        let selected_logical_slot = self
            .selected_binding_slot
            .unwrap_or_else(|| self.selected_surface.primary_logical_slot());

        let board_preview_layer = match self.document_store.board_preview_selection() {
            BoardPreviewSelection::Layer(layer) => Some(layer),
            BoardPreviewSelection::UsbStartup => None,
        };
        let touch_led_states = board_preview_layer
            .map(|layer| layer.leds())
            .unwrap_or([false; 3]);
        let board_preview_label = board_preview_layer
            .map(|layer| format!("第 {} 层预览", layer.number()))
            .unwrap_or_else(|| "USB 启动方案预览".into());

        CanvasProjection {
            profile_id: self.selected_profile_id.clone(),
            profile_display_name,
            selected_layer_id: self.selected_layer_id.clone(),
            layer_tabs,
            selected_surface: self.selected_surface,
            selected_logical_slot,
            selected_joystick_direction: self.selected_joystick_direction,
            bottom_row_layout,
            show_keycap_icons: self.show_keycap_icons,
            show_keycap_labels: self.show_keycap_labels,
            encoder,
            joystick,
            keycap_cells,
            status_cluster_bounds: CanvasRect::new(
                CANVAS_PADDING,
                CANVAS_PADDING + step * 3.0,
                KEY_1U_SIZE,
                KEY_1U_SIZE,
            ),
            status_led_hex: underglow_hex.clone(),
            underglow_hex,
            board_preview_layer: board_preview_layer.map(|layer| layer.number()),
            touch_led_states,
            board_preview_label,
            protected_lock,
            can_edit_bindings,
            can_add_lighting,
            can_export_or_copy_layer,
            inspector_expanded,
        }
    }

    pub(super) fn build_static_cheat_sheet_projection(&self) -> CheatSheetProjection {
        let doc = &self.document_store.host_store().draft.document;
        let graph = &doc.editable.graph;
        let pid_opt = self.resolved_active_profile_id();
        let layer = pid_opt
            .as_ref()
            .and_then(|pid| graph.profiles.get(pid))
            .and_then(|p| p.layer_order.first())
            .and_then(|lid| graph.layers.get(lid));

        let is_split = self.current_bottom_row_layout() == BottomRowLayout::Split;
        let slot_defs: [(u32, &str, &str); 20] = [
            (0, "AG00", "左上键"),
            (1, "AG01", "右上键"),
            (2, "AG02", "按键 03"),
            (3, "AG03", "按键 04"),
            (4, "AG04", "按键 05"),
            (5, "AG05", "按键 06"),
            (6, "AG06", "按键 07"),
            (7, "AG07", "按键 08"),
            (8, "AG08", "按键 09"),
            (9, "AG09", "按键 10"),
            (
                10,
                "AG10",
                if is_split {
                    "底行左键"
                } else {
                    "底部宽键（2U）"
                },
            ),
            (11, "AG11", "底行中键"),
            (12, "AG12", "底行右键"),
            (13, "AG13", "旋钮左旋"),
            (14, "AG14", "旋钮右旋"),
            (15, "ENC_CLK", "旋钮按下"),
            (16, "AG15", "摇杆（上）"),
            (17, "AG16", "摇杆（左）"),
            (18, "AG17", "摇杆（下）"),
            (19, "AG18", "摇杆（右）"),
        ];

        let mut key_cells = Vec::with_capacity(20);
        for (slot, binding_id, control_name) in slot_defs {
            if slot == 11 && !is_split {
                continue;
            }
            let effective_slot = if !is_split && slot == 10 {
                resolve_merged_bottom_slot(doc, layer)
            } else {
                slot
            };
            let (label, act_opt) = resolve_slot_action_info(doc, layer, effective_slot);
            let short_label = format_popover_cheat_label(&label);
            key_cells.push(CheatSheetKeyCellProjection {
                binding_id: binding_id.to_string(),
                control_name: control_name.to_string(),
                label,
                short_label,
                available: act_opt.is_some(),
            });
        }

        CheatSheetProjection {
            profile_name: self.active_profile_display_name(),
            routing_summary: self.routing_mode_summary_label(),
            target_agent_card: None,
            key_cells,
        }
    }

    pub(super) fn build_cheat_sheet_projection(&self) -> CheatSheetProjection {
        let mut sheet = self.build_static_cheat_sheet_projection();
        sheet.target_agent_card = self.active_desktop_agent_adapter().map(|adapter| {
            let agent_name = self
                .active_canonical_agent()
                .map(|a| a.display_name().to_string())
                .unwrap_or_else(|| adapter.clone());
            let acks = &self.acknowledged_watermarks;
            let phase = self.derive_live_lighting_phase(acks);
            let (phase_label, phase_symbol) = match phase {
                FourAppAgentPhase::Idle => ("空闲就绪", "checkmark.circle"),
                FourAppAgentPhase::Thinking => ("正在运行", "sparkles"),
                FourAppAgentPhase::AwaitingInput => ("等待确认", "exclamationmark.Bubble"),
                FourAppAgentPhase::Completed => ("已完成待查看", "circle.fill"),
                FourAppAgentPhase::Error => ("执行异常", "xmark.octagon.fill"),
                FourAppAgentPhase::BackgroundActive => ("后台运行中", "clock.arrow.circlepath"),
            };
            let is_recording = self.ptt_active;
            let voice_summary = if is_recording {
                "语音输入中".to_string()
            } else {
                "按住说话已就绪".to_string()
            };
            TargetAgentStatusCardProjection {
                agent_name,
                adapter,
                phase_label: phase_label.to_string(),
                phase_symbol: phase_symbol.to_string(),
                is_recording,
                voice_summary,
                confirm_mode: self.radial_state.confirm_mode(),
            }
        });
        sheet
    }

    pub(super) fn build_inspector_projection(
        &mut self,
        can_edit_draft: bool,
        canvas: &CanvasProjection,
    ) -> InspectorProjection {
        let doc = &self.document_store.draft.document;
        let graph = &doc.editable.graph;
        let layer = self
            .selected_layer_id
            .as_ref()
            .and_then(|lid| graph.layers.get(lid));
        let layer_name = layer
            .map(|l| l.name.clone())
            .unwrap_or_else(|| "默认层".to_string());
        let is_protected = self.is_selected_layer_protected();

        if let Some(lock) = &canvas.protected_lock {
            return InspectorProjection {
                width_px: INSPECTOR_WIDTH,
                profile_id: self.selected_profile_id.clone(),
                profile_display_name: canvas.profile_display_name.clone(),
                layer_id: self.selected_layer_id.clone(),
                layer_name,
                is_protected_layer: true,
                can_edit_draft,
                body: InspectorBodyProjection::ProtectedLocked(lock.clone()),
            };
        }

        let slot = self.current_selected_slot();
        let supports_task_role = (0..6u32).contains(&slot);
        let (bound_action_name, action_obj) = resolve_slot_action_info(doc, layer, slot);
        let bound_graph_action_id = resolve_slot_graph_action_id(layer, slot);
        let bound_action_id = action_obj
            .map(|a| a.id.clone())
            .or_else(|| bound_graph_action_id.clone());
        let bound_action_category = action_obj.map(|a| a.category.clone()).unwrap_or_default();
        let bound_action_detail = action_obj.map(|a| a.detail.clone()).unwrap_or_default();
        let bound_capability =
            action_obj.map(|a| action_library::capability(a, self.command_execution_enabled));

        let is_session_action = action_obj.is_some_and(|a| {
            a.id.starts_with("slot-open-")
                || a.id.starts_with("cockpit-slot-")
                || a.payload
                    .get("command")
                    .and_then(|v| v.as_str())
                    .is_some_and(|c| c.starts_with("slot-open:"))
        });

        let slot_role = if !supports_task_role {
            KeycapRole::ActionKey
        } else if let Some(pid) = &self.selected_profile_id {
            self.slot_role_overrides
                .get(&(pid.clone(), slot))
                .copied()
                .unwrap_or(if is_session_action {
                    KeycapRole::TaskKey
                } else {
                    KeycapRole::ActionKey
                })
        } else if is_session_action {
            KeycapRole::TaskKey
        } else {
            KeycapRole::ActionKey
        };

        let is_apple_fn_bound = action_obj.is_some_and(|a| {
            a.id == "apple-fn-key"
                || (a.payload.get("type").and_then(|v| v.as_str()) == Some("shortcut")
                    && a.payload
                        .get("key")
                        .and_then(|v| v.as_str())
                        .is_some_and(|k| k.eq_ignore_ascii_case("fn"))
                    && a.payload
                        .get("modifiers")
                        .and_then(|v| v.as_array())
                        .is_some_and(Vec::is_empty))
        });

        let selected_keycap = self.selected_keycap(slot);
        static ALL_KEYCAP_ITEMS: std::sync::OnceLock<Vec<KeycapCatalogItemProjection>> =
            std::sync::OnceLock::new();
        let filtered_keycaps: Vec<KeycapCatalogItemProjection> =
            if let Some(mut rows) = self.reusable_catalogs.keycaps.take() {
                for row in &mut rows {
                    row.is_selected = row.id == selected_keycap;
                }
                rows
            } else if self.keycap_search_query.trim().is_empty() {
                let mut rows = ALL_KEYCAP_ITEMS
                    .get_or_init(|| {
                        CodexKeycapId::ALL_34
                            .into_iter()
                            .map(|k| KeycapCatalogItemProjection {
                                id: k,
                                silk_code: k.silk_code().to_string(),
                                short_label: k.short_label().to_string(),
                                chinese_hint: k.chinese_hint().to_string(),
                                glyph_symbol: k.glyph_symbol().to_string(),
                                default_action_id: k.default_action_id().map(str::to_string),
                                is_selected: false,
                            })
                            .collect()
                    })
                    .clone();
                for row in &mut rows {
                    if row.id == selected_keycap {
                        row.is_selected = true;
                        break;
                    }
                }
                rows
            } else {
                CodexKeycapId::ALL_34
                    .into_iter()
                    .filter(|k| k.matches_query(&self.keycap_search_query))
                    .map(|k| KeycapCatalogItemProjection {
                        id: k,
                        silk_code: k.silk_code().to_string(),
                        short_label: k.short_label().to_string(),
                        chinese_hint: k.chinese_hint().to_string(),
                        glyph_symbol: k.glyph_symbol().to_string(),
                        default_action_id: k.default_action_id().map(str::to_string),
                        is_selected: k == selected_keycap,
                    })
                    .collect()
            };

        static AVAILABLE_CATEGORIES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
        let available_categories = self.reusable_catalogs.categories.take().unwrap_or_else(|| {
            AVAILABLE_CATEGORIES
                .get_or_init(|| {
                    let all_catalog = mi_model::draft::actions();
                    let mut categories = vec!["全部".to_string()];
                    for act in &all_catalog {
                        if !categories.contains(&act.category) {
                            categories.push(act.category.clone());
                        }
                    }
                    categories
                })
                .clone()
        });

        static DEFAULT_ACTION_ROWS: std::sync::OnceLock<(
            Vec<ActionLibraryRowProjection>,
            Vec<ActionLibraryRowProjection>,
        )> = std::sync::OnceLock::new();
        let filtered_actions: Vec<ActionLibraryRowProjection> = if let Some(mut rows) =
            self.reusable_catalogs.actions.take()
        {
            for row in &mut rows {
                row.is_currently_bound = bound_action_id.as_deref() == Some(row.action_id.as_str());
            }
            rows
        } else if self.action_search_query.trim().is_empty()
            && self.action_category_filter == "全部"
            && !self.favorites_only
            && self.favorite_action_ids.is_empty()
        {
            let (enabled_rows, disabled_rows) = DEFAULT_ACTION_ROWS.get_or_init(|| {
                let empty_favs = BTreeSet::new();
                let build_for = |exec_enabled: bool| {
                    action_library::search("", "全部", false, &empty_favs, exec_enabled)
                        .into_iter()
                        .map(|entry| ActionLibraryRowProjection {
                            action_id: entry.action.id,
                            name: entry.action.name,
                            category: entry.action.category,
                            detail: entry.action.detail,
                            target_summary: entry.capability.target,
                            execution_available: entry.capability.execution_available,
                            unavailable_reason: entry.capability.unavailable_reason,
                            is_favorite: false,
                            is_currently_bound: false,
                        })
                        .collect()
                };
                (build_for(true), build_for(false))
            });
            let mut rows = if self.command_execution_enabled {
                enabled_rows.clone()
            } else {
                disabled_rows.clone()
            };
            if let Some(bound_id) = bound_action_id.as_deref() {
                for row in &mut rows {
                    if row.action_id == bound_id {
                        row.is_currently_bound = true;
                        break;
                    }
                }
            }
            rows
        } else {
            let entries = action_library::search(
                &self.action_search_query,
                &self.action_category_filter,
                self.favorites_only,
                &self.favorite_action_ids,
                self.command_execution_enabled,
            );
            entries
                .into_iter()
                .map(|entry| {
                    let is_currently_bound =
                        bound_action_id.as_deref() == Some(entry.action.id.as_str());
                    ActionLibraryRowProjection {
                        action_id: entry.action.id,
                        name: entry.action.name,
                        category: entry.action.category,
                        detail: entry.action.detail,
                        target_summary: entry.capability.target,
                        execution_available: entry.capability.execution_available,
                        unavailable_reason: entry.capability.unavailable_reason,
                        is_favorite: entry.favorite,
                        is_currently_bound,
                    }
                })
                .collect()
        };

        let mut has_local_macro = false;
        let mut has_local_multi = false;
        for act in graph.actions.values() {
            match act.payload.get("type").and_then(|v| v.as_str()) {
                Some("local_macro") => has_local_macro = true,
                Some("local_multi") => has_local_multi = true,
                _ => {}
            }
            if has_local_macro && has_local_multi {
                break;
            }
        }

        let macros: Vec<MacroLibraryRowProjection> = if !has_local_macro {
            Vec::new()
        } else {
            self.document_store
                .draft
                .macros()
                .into_iter()
                .map(|m| {
                    let is_currently_bound = bound_graph_action_id.as_deref()
                        == Some(m.id.as_str())
                        || bound_action_id.as_deref() == Some(m.id.as_str());
                    let event_count = m
                        .action
                        .payload
                        .get("macro_sequence")
                        .and_then(|s| s.get("events"))
                        .and_then(|e| e.as_array())
                        .map(Vec::len)
                        .unwrap_or(0);
                    let reference_count = m.references.len();
                    let reference_summary = format_macro_references(&m.references);
                    MacroLibraryRowProjection {
                        id: m.id,
                        name: m.action.name,
                        is_multi: false,
                        step_or_branch_summary: format!("{event_count} 步"),
                        reference_count,
                        reference_summary,
                        is_currently_bound,
                    }
                })
                .collect()
        };

        let multis: Vec<MacroLibraryRowProjection> = if !has_local_multi {
            Vec::new()
        } else {
            self.document_store
                .draft
                .multis()
                .into_iter()
                .map(|m| {
                    let is_currently_bound = bound_graph_action_id.as_deref()
                        == Some(m.id.as_str())
                        || bound_action_id.as_deref() == Some(m.id.as_str());
                    let reference_count = m.references.len();
                    let reference_summary = format_macro_references(&m.references);
                    MacroLibraryRowProjection {
                        id: m.id,
                        name: m.action.name,
                        is_multi: true,
                        step_or_branch_summary: "四分支 Multi".to_string(),
                        reference_count,
                        reference_summary,
                        is_currently_bound,
                    }
                })
                .collect()
        };

        let can_add_lighting = self
            .selected_layer_id
            .as_ref()
            .is_some_and(|lid| !doc.is_protected_oai_layer(lid));
        let has_layer_lighting = self
            .selected_layer_id
            .as_ref()
            .is_some_and(|lid| doc.lighting(lid).is_ok());

        let surface_display_name = match self.selected_surface {
            ControlSurfaceId::Joystick => {
                format!("摇杆 · {}", self.selected_joystick_direction.label())
            }
            other => format!("按键 {}", other.binding_id()),
        };

        let editable = EditableSlotInspectorProjection {
            selected_surface: self.selected_surface,
            selected_logical_slot: slot,
            selected_binding_id: self.selected_surface.binding_id().to_string(),
            surface_display_name,
            joystick_direction: (self.selected_surface == ControlSurfaceId::Joystick)
                .then_some(self.selected_joystick_direction),
            bottom_row_layout: self.current_bottom_row_layout(),
            supports_task_role,
            slot_role,
            bound_action_id,
            bound_action_name,
            bound_action_category,
            bound_action_detail,
            bound_capability,
            is_apple_fn_bound,
            selected_keycap,
            keycap_search_query: self.keycap_search_query.clone(),
            filtered_keycaps,
            bind_keycap_default_action_on_select: self.bind_keycap_default_action_on_select,
            shortcut_recorder: self.shortcut_recorder.clone(),
            action_search_query: self.action_search_query.clone(),
            action_category_filter: self.action_category_filter.clone(),
            available_categories,
            favorites_only: self.favorites_only,
            filtered_actions,
            macros,
            multis,
            open_drawers: self.open_drawers.clone(),
            can_add_lighting,
            has_layer_lighting,
            command_execution_enabled: self.command_execution_enabled,
        };

        InspectorProjection {
            width_px: INSPECTOR_WIDTH,
            profile_id: self.selected_profile_id.clone(),
            profile_display_name: canvas.profile_display_name.clone(),
            layer_id: self.selected_layer_id.clone(),
            layer_name,
            is_protected_layer: is_protected,
            can_edit_draft,
            body: InspectorBodyProjection::Editable(Box::new(editable)),
        }
    }

    pub(super) fn build_radial_projection(&self) -> RadialOverlayProjection {
        let visible_session = self.radial_state.session().cloned();
        let panel_spec = visible_session
            .as_ref()
            .map(|session| PassivePanelFrameSpec::for_radial_frame(session.anchor_frame));
        RadialOverlayProjection {
            enabled: self.radial_state.enabled(),
            readiness: *self.radial_readiness,
            confirm_mode: self.radial_state.confirm_mode(),
            visible_session,
            panel_spec,
        }
    }

    pub(super) fn build_session_strip_projection(
        &self,
        can_edit_draft: bool,
    ) -> SessionStripProjection {
        let doc = &self.document_store.draft.document;
        let graph = &doc.editable.graph;
        let layer = self
            .selected_layer_id
            .as_ref()
            .and_then(|lid| graph.layers.get(lid));
        let is_selected_protected = self.is_selected_layer_protected();
        let is_lighting_protected = self.is_active_lighting_protected();
        let acks = &self.acknowledged_watermarks;

        let mut completed_blue_dot_pills = Vec::new();
        let mut working_pills = Vec::new();
        let mut attention_pills = Vec::new();

        for cand in self.slot_table.candidates() {
            let eff_kind = cand.effective_kind(acks);
            if eff_kind == FourAppSessionKind::Idle {
                continue;
            }
            let agent = agent_for_place(&cand.place);
            let pill = SessionCapsulePillProjection {
                place: cand.place.clone(),
                agent,
                agent_monogram: agent.badge_monogram().to_string(),
                title: cand.title.clone(),
                kind: eff_kind,
                badge_text: eff_kind.badge_prefix().to_string(),
                color_hex: eff_kind.capsule_hex().to_string(),
                watermark: cand.watermark.clone(),
            };
            match eff_kind {
                FourAppSessionKind::NeedsAttention | FourAppSessionKind::Error => {
                    attention_pills.push(pill);
                }
                FourAppSessionKind::BlueDotCompleted => {
                    completed_blue_dot_pills.push(pill);
                }
                FourAppSessionKind::Working => {
                    working_pills.push(pill);
                }
                FourAppSessionKind::Idle => {}
            }
        }

        let unread_completed_count = completed_blue_dot_pills.len();
        let working_count = working_pills.len();

        let indexed_controls = index_layer_controls(layer);
        let top_six_bound_as_session_keys = (0..6u32).all(|slot| {
            resolve_indexed_slot(doc, &indexed_controls, slot)
                .1
                .is_some_and(|a| is_session_slot_action(a, slot as usize))
        });

        let binding_ids = ["AG00", "AG01", "AG02", "AG03", "AG04", "AG05"];

        let slots: [SessionSlotCardProjection; SLOT_COUNT] = std::array::from_fn(|idx| {
            let slot = &self.slot_table.slot_table().slots()[idx];
            let cand = slot
                .place
                .as_ref()
                .and_then(|p| self.slot_table.find_candidate(p));
            let kind = cand
                .map(|c| c.effective_kind(acks))
                .unwrap_or(FourAppSessionKind::Idle);
            let agent = slot.place.as_ref().map(agent_for_place);
            let agent_monogram = agent
                .map(|a| a.badge_monogram().to_string())
                .unwrap_or_default();
            let is_unread_blue_dot = cand.is_some_and(|c| c.is_unread_blue_dot(acks));
            let is_working_animated = kind == FourAppSessionKind::Working;
            let is_bound_to_top_key = resolve_indexed_slot(doc, &indexed_controls, idx as u32)
                .1
                .is_some_and(|a| is_session_slot_action(a, idx));
            let led_color_hex = if is_lighting_protected || !self.lighting_popover.lighting_enabled
            {
                "#000000".to_string()
            } else {
                self.full_keypad_feedback
                    .iter()
                    .find(|item| item.binding_id == binding_ids[idx])
                    .map(|item| item.color.clone())
                    .unwrap_or_else(|| "#222222".to_string())
            };
            let subtitle = match kind {
                FourAppSessionKind::NeedsAttention => "待处理审批/输入".to_string(),
                FourAppSessionKind::BlueDotCompleted => "● 已完成待查看".to_string(),
                FourAppSessionKind::Working => ":: 正在运行".to_string(),
                FourAppSessionKind::Error => "执行异常".to_string(),
                FourAppSessionKind::Idle => "空闲已读".to_string(),
            };

            SessionSlotCardProjection {
                slot_index: idx,
                key_binding_id: binding_ids[idx].to_string(),
                mode: slot.mode,
                status: slot.status,
                kind,
                agent,
                agent_monogram,
                title: slot.title.clone(),
                subtitle,
                place: slot.place.clone(),
                watermark: cand.map(|c| c.watermark.clone()).unwrap_or_default(),
                is_unread_blue_dot,
                is_working_animated,
                is_bound_to_top_key,
                led_color_hex,
            }
        });

        let mut lighting_popover = self.lighting_popover.clone();
        lighting_popover.suspended_by_protected_layer = is_lighting_protected;

        SessionStripProjection {
            slots,
            completed_blue_dot_pills,
            working_pills,
            attention_pills,
            unread_completed_count,
            working_count,
            top_six_bound_as_session_keys,
            can_mutate_layout: can_edit_draft && !is_selected_protected,
            lighting_popover,
        }
    }

    pub(super) fn build_status_menu_projection(
        &self,
        can_edit_draft: bool,
    ) -> StatusMenuProjection {
        let profiles = &self
            .document_store
            .host_store()
            .draft
            .document
            .editable
            .graph
            .profiles;
        let active_pid = self.resolved_active_profile_id();
        let favorites_2x2: [FavoriteSlotCardProjection; FAVORITE_SLOT_COUNT] =
            std::array::from_fn(|idx| {
                let slot = FavoriteSlotIndex::ALL[idx];
                let pid_opt = self.pinned_favorites.slots[idx].clone();
                let title = pid_opt
                    .as_ref()
                    .and_then(|pid| profiles.get(pid))
                    .map(|p| {
                        CanonicalAgent::from_profile_name(&p.name)
                            .map(|a| a.display_name().to_string())
                            .unwrap_or_else(|| p.name.clone())
                    })
                    .unwrap_or_else(|| "未绑定".to_string());
                let is_active = pid_opt.is_some() && pid_opt == active_pid;
                FavoriteSlotCardProjection {
                    slot,
                    profile_id: pid_opt,
                    title,
                    is_active,
                }
            });

        let is_protected = self.is_selected_layer_protected();
        let popover_preview_mode = self.document_store.popover_preview_mode();
        let cheat_sheet_preview = match (popover_preview_mode, active_pid.is_some()) {
            (PopoverPreviewMode::CheatSheet, true) => {
                Some(self.build_static_cheat_sheet_projection())
            }
            _ => None,
        };
        let locked_profile_id = self.document_store.locked_profile_id();
        let is_profile_locked = locked_profile_id.is_some();
        let competing_host_notice = self.competing_input_app_running.then(|| {
            if is_profile_locked {
                "检测到原版 Input 正在后台运行，已由方案锁定隔离干扰".to_string()
            } else {
                "检测到原版 Input 正在后台抢占层切换，建议开启「锁定当前方案」".to_string()
            }
        });
        StatusMenuProjection {
            icon: StatusItemIconSpec::canonical(),
            active_profile_id: active_pid,
            active_profile_name: self.active_profile_display_name(),
            routing_mode_summary: self.routing_mode_summary_label(),
            favorites_2x2,
            host_execution_enabled: self.document_store.host_execution_enabled(),
            auto_follow_enabled: self.auto_follow_enabled,
            manual_override: self.manual_override,
            is_profile_locked,
            locked_profile_id,
            accessibility_trusted: self.accessibility_trusted,
            competing_host_notice,
            radial_menu_enabled: self.radial_state.enabled(),
            radial_confirm_mode: self.radial_state.confirm_mode(),
            lighting_enabled: self.lighting_popover.lighting_enabled,
            popover_preview_mode,
            cheat_sheet_preview,
            cheat_sheet_visible: self.overlay_controller.is_cheat_sheet_visible(),
            can_export_profile: can_edit_draft && self.selected_profile_id.is_some(),
            can_export_layer: can_edit_draft && !is_protected && self.selected_layer_id.is_some(),
            pending_import: self.pending_preset_import.clone(),
            pending_export_scope: self.pending_preset_export_scope,
            preset_error: self.preset_transfer_error.clone(),
        }
    }

    pub(super) fn compute_sidebar_rows(&self) -> Vec<SidebarProfileRow> {
        let doc = &self.document_store.draft.document;
        let graph = &doc.editable.graph;
        let active_host_id = self.document_store.host_active_profile_id();
        let selected_id = self.selected_profile_id.as_deref();

        let mut canonical_rows: Vec<(CanonicalAgent, SidebarProfileRow)> = Vec::with_capacity(8);
        let mut custom_rows: Vec<SidebarProfileRow> = Vec::new();

        for pid in &graph.profile_order {
            let Some(profile) = graph.profiles.get(pid) else {
                continue;
            };
            let is_protected_oai = profile
                .layer_order
                .first()
                .is_some_and(|lid| doc.is_protected_oai_layer(lid));
            let canonical_agent = CanonicalAgent::from_profile_name(&profile.name);
            if canonical_agent.is_none() && !self.show_other_profiles {
                continue;
            }
            let display_name = canonical_agent
                .map(|a| a.display_name().to_string())
                .unwrap_or_else(|| profile.name.clone());
            let badge_monogram = canonical_agent
                .map(|a| a.badge_monogram().to_string())
                .unwrap_or_else(|| custom_monogram(&profile.name));
            let app_installed = canonical_agent
                .map(|a| {
                    self.applications
                        .get(a.bundle_id())
                        .copied()
                        .unwrap_or(false)
                })
                .unwrap_or(true);
            let is_manual_fallback =
                canonical_agent.is_some_and(CanonicalAgent::is_manual_fallback);

            let row = SidebarProfileRow {
                profile_id: pid.clone(),
                raw_name: profile.name.clone(),
                display_name,
                badge_monogram,
                canonical_agent,
                is_selected: selected_id == Some(pid.as_str()),
                is_active_host: active_host_id.as_deref() == Some(pid.as_str()),
                is_protected_oai,
                app_installed,
                is_manual_fallback,
            };

            if let Some(agent) = canonical_agent {
                if let Some((_, existing)) = canonical_rows
                    .iter_mut()
                    .find(|(existing, _)| *existing == agent)
                {
                    if profile.name == mi_model::preset::LOCAL_CODEX_PROFILE_NAME {
                        let mut original = std::mem::replace(existing, row);
                        original.display_name = "Codex · 原生".into();
                        if self.show_other_profiles {
                            custom_rows.push(original);
                        }
                    } else if self.show_other_profiles {
                        custom_rows.push(row);
                    }
                } else {
                    canonical_rows.push((agent, row));
                }
            } else {
                custom_rows.push(row);
            }
        }

        canonical_rows.sort_by_key(|(agent, _)| agent.rank());
        let mut rows: Vec<SidebarProfileRow> =
            canonical_rows.into_iter().map(|(_, row)| row).collect();
        rows.extend(custom_rows);
        rows
    }

    pub(super) fn current_bottom_row_layout(&self) -> BottomRowLayout {
        self.bottom_row_layout
    }

    pub(super) fn current_selected_slot(&self) -> u32 {
        self.selected_binding_slot
            .unwrap_or_else(|| self.selected_surface.primary_logical_slot())
    }

    pub(super) fn derive_live_lighting_phase(
        &self,
        acks: &BTreeMap<String, String>,
    ) -> FourAppAgentPhase {
        let mut has_completed = false;
        let mut has_working = false;
        let mut has_error = false;
        let mut newest_completed_ms = 0u64;
        let mut newest_working_ms = 0u64;
        for cand in self.slot_table.candidates() {
            match cand.effective_kind(acks) {
                FourAppSessionKind::NeedsAttention => return FourAppAgentPhase::AwaitingInput,
                FourAppSessionKind::Error => has_error = true,
                FourAppSessionKind::BlueDotCompleted => {
                    has_completed = true;
                    newest_completed_ms = newest_completed_ms.max(cand.last_modified_ms);
                }
                FourAppSessionKind::Working => {
                    has_working = true;
                    newest_working_ms = newest_working_ms.max(cand.last_modified_ms);
                }
                FourAppSessionKind::Idle => {}
            }
        }
        if has_error {
            FourAppAgentPhase::Error
        } else if has_completed && (!has_working || newest_completed_ms >= newest_working_ms) {
            FourAppAgentPhase::Completed
        } else if has_working {
            FourAppAgentPhase::Thinking
        } else if has_completed {
            FourAppAgentPhase::Completed
        } else {
            FourAppAgentPhase::Idle
        }
    }

    pub(super) fn is_protected_oai_profile(&self, profile_id: &str) -> bool {
        let doc = &self.document_store.draft.document;
        doc.editable
            .graph
            .profiles
            .get(profile_id)
            .is_some_and(|p| {
                p.layer_order
                    .iter()
                    .any(|lid| doc.is_protected_oai_layer(lid))
            })
    }

    pub(super) fn is_selected_layer_protected(&self) -> bool {
        let doc = &self.document_store.draft.document;
        if let Some(lid) = &self.selected_layer_id {
            return doc.is_protected_oai_layer(lid);
        }
        self.selected_profile_id
            .as_deref()
            .is_some_and(|pid| self.is_protected_oai_profile(pid))
    }

    pub(super) fn is_active_lighting_protected(&self) -> bool {
        if self.six_layer_state.installation.is_some() || self.six_layer_running_present {
            return false;
        }
        let Some(active_pid) = self.resolved_active_profile_id() else {
            return false;
        };
        let doc = &self.document_store.draft.document;
        let Some(profile) = doc.editable.graph.profiles.get(&active_pid) else {
            return false;
        };
        if self.selected_profile_id.as_deref() == Some(active_pid.as_str()) {
            if let Some(lid) = &self.selected_layer_id {
                if profile.layer_order.iter().any(|id| id == lid) {
                    return doc.is_protected_oai_layer(lid);
                }
            }
        }
        profile
            .layer_order
            .first()
            .is_some_and(|lid| doc.is_protected_oai_layer(lid))
    }

    pub(super) fn legacy_host_profile_id(&self, layer: u8) -> Option<String> {
        let index = layer.checked_sub(1)? as usize;
        let id = self
            .document_store
            .legacy_three_bindings()?
            .0
            .get(index)?
            .as_ref()?;
        let doc = &self.document_store.host_store().draft.document;
        let profile = doc.editable.graph.profiles.get(id)?;
        if profile
            .layer_order
            .iter()
            .any(|lid| doc.is_protected_oai_layer(lid))
        {
            return None;
        }
        Some(id.clone())
    }

    pub(super) fn project(mut self) -> WorkbenchProjection {
        let rows = self.compute_sidebar_rows();
        let active_host_id = self.document_store.host_active_profile_id();
        let is_protected_active = active_host_id
            .as_deref()
            .or(self.selected_profile_id.as_deref())
            .is_some_and(|pid| self.is_protected_oai_profile(pid));

        let locked_profile_id = self.document_store.locked_profile_id();
        let is_profile_locked = locked_profile_id.is_some();
        let host_execution_enabled = self.document_store.host_execution_enabled();
        let untrusted_exec = !self.accessibility_trusted && host_execution_enabled;

        let ownership_banner = if untrusted_exec {
            OwnershipBannerState::Caution {
                reason: "缺少 macOS 辅助功能权限，按键动作已被系统拦截，请打开辅助功能设置授权"
                    .to_string(),
            }
        } else if self.six_layer_state.installation.is_some() || self.six_layer_view.is_some() {
            if let Some(reason) = self
                .six_layer_view
                .as_ref()
                .and_then(|running| running.failure.clone())
            {
                OwnershipBannerState::Caution { reason }
            } else if self.device_connected {
                OwnershipBannerState::Ready {
                    summary: "六层标准键输入已连接".into(),
                }
            } else {
                OwnershipBannerState::Caution {
                    reason: "六层输入尚未连接".into(),
                }
            }
        } else if is_protected_active {
            OwnershipBannerState::NativeHeld {
                summary: "原生受保护方案".into(),
            }
        } else {
            OwnershipBannerState::Ready {
                summary: "就绪".into(),
            }
        };
        let editing_device = self.document_store.editing_device();
        let can_edit_draft =
            !self.document_store.draft.read_only && self.document_store.draft.loaded_format == 2;

        let ready_state = if editing_device {
            HeaderReadyState::ModeNotice {
                text: "硬件布局草稿模式 · 返回后继续管理软件方案".to_string(),
            }
        } else if untrusted_exec {
            HeaderReadyState::Caution {
                text: "待授权辅助功能".to_string(),
            }
        } else if ownership_banner.is_caution() {
            HeaderReadyState::Caution {
                text: ownership_banner.text().to_string(),
            }
        } else if self.device_connected {
            HeaderReadyState::Ready {
                text: "小键盘已连接".to_string(),
            }
        } else {
            HeaderReadyState::Ready {
                text: "已就绪".to_string(),
            }
        };

        let order = &self
            .document_store
            .draft
            .document
            .editable
            .graph
            .profile_order;
        let selected_pos = self
            .selected_profile_id
            .as_ref()
            .and_then(|pid| order.iter().position(|id| id == pid));
        let can_move_up = selected_pos.is_some_and(|pos| pos > 0);
        let can_move_down = selected_pos.is_some_and(|pos| pos + 1 < order.len());
        let can_delete = order.len() > 1 && self.selected_profile_id.is_some();

        let selected_display_name = rows
            .iter()
            .find(|row| row.is_selected)
            .map(|row| row.display_name.clone())
            .or_else(|| {
                self.selected_profile_id.as_ref().and_then(|id| {
                    self.document_store
                        .draft
                        .document
                        .editable
                        .graph
                        .profiles
                        .get(id)
                        .map(|profile| profile.name.clone())
                })
            });

        let auto_switch =
            AutoSwitchState::from_flags(true, self.auto_follow_enabled, self.manual_override);

        let header = HeaderProjection {
            engine_version: env!("CARGO_PKG_VERSION").to_string(),
            ready_state,
            banner: ownership_banner.clone(),
            show_connect_button: !self.device_connected,
            show_return_to_host_button: editing_device,
            host_execution_enabled,
            accessibility_trusted: self.accessibility_trusted,
            can_edit_draft,
            can_undo: self.document_store.draft.can_undo,
            can_redo: self.document_store.draft.can_redo,
            busy: self.busy,
        };

        let sidebar = SidebarProjection {
            section_title: "常用软件方案".to_string(),
            rows,
            board_layers: self.build_board_layers_projection(),
            selected_profile_id: self.selected_profile_id.clone(),
            selected_display_name,
            active_host_profile_id: active_host_id,
            is_profile_locked,
            locked_profile_id,
            accessibility_trusted: self.accessibility_trusted,
            auto_switch,
            show_other_profiles: self.show_other_profiles,
            can_edit_draft,
            editing_device,
            can_move_up,
            can_move_down,
            can_delete,
            busy: self.busy,
            ownership_banner: ownership_banner.clone(),
            footer_status_text: ownership_banner.text().to_string(),
            footer_limit_text: "长按说话上限 120 秒，响应超时 8 秒".to_string(),
        };

        let canvas = self.build_canvas_projection(can_edit_draft);
        let session_strip = self.build_session_strip_projection(can_edit_draft);
        let inspector = self.build_inspector_projection(can_edit_draft, &canvas);

        let overlay = self
            .overlay_controller
            .project(&self.screen_geometry, self.last_cursor_position);

        let radial = self.build_radial_projection();

        let status_menu = self.build_status_menu_projection(can_edit_draft);
        let (device_connected, device_summary) = if let Some(cand) = &self.connected_candidate {
            (
                self.device_connected,
                format!(
                    "{} ({:04X}:{:04X}) · USB HID 只读保护已启用",
                    cand.product, cand.vid, cand.pid
                ),
            )
        } else {
            (
                false,
                "Creator Micro 2 (303A:8298) · 离线草稿模式".to_string(),
            )
        };

        WorkbenchProjection {
            header,
            sidebar,
            canvas,
            session_strip,
            inspector,
            radial,
            radial_slot_configs: self.radial_slot_configs.clone(),
            overlay,
            status_menu,
            status_menu_open: self.status_menu_open,
            active_modal: self.active_modal.clone(),
            selected_profile_id: self.selected_profile_id.clone(),
            selected_layer_id: self.selected_layer_id.clone(),
            selected_binding_slot: self.selected_binding_slot,
            status_message: self.status_message.clone(),
            last_exported_preset: self.last_exported_preset.clone(),
            device_connected,
            device_summary,
            document_revision: self.document_store.draft.revision,
            host_revision: self.document_store.host_revision(),
            banner_flash_count: self.banner_flash_count,
            last_switch_duration: self.last_switch_duration,
        }
    }

    pub(super) fn resolve_current_radial_slots(&self) -> [RadialSlot; RADIAL_SECTOR_COUNT] {
        let active_agent = self.active_canonical_agent();
        let active_session_preview = active_agent.and_then(|agent| {
            self.slot_table
                .candidates()
                .iter()
                .find(|c| agent_for_place(&c.place) == agent)
                .map(|c| c.title.clone())
        });
        let ctx = RadialSlotResolutionContext {
            has_active_profile: self.resolved_active_profile_id().is_some(),
            active_adapter: self.active_desktop_agent_adapter(),
            active_agent_display_name: active_agent.map(|a| a.display_name().to_string()),
            active_session_preview,
            can_navigate_back: self.navigation_history.can_go_back(|_| true),
            can_navigate_forward: self.navigation_history.can_go_forward(|_| true),
        };
        resolve_radial_slots(&self.radial_slot_configs, &ctx, |bundle_id| {
            self.applications.get(bundle_id).copied().unwrap_or(false)
        })
    }

    pub(super) fn resolved_active_profile_id(&self) -> Option<String> {
        let profiles = &self
            .document_store
            .host_store()
            .draft
            .document
            .editable
            .graph
            .profiles;
        if let Some(locked) = self
            .document_store
            .locked_profile_id()
            .filter(|pid| profiles.contains_key(pid))
        {
            return Some(locked);
        }
        if let Some(profile) = self
            .six_layer_view
            .as_ref()
            .and_then(|running| running.effective_profile_id.clone())
        {
            return Some(profile);
        }
        self.document_store
            .host_active_profile_id()
            .filter(|pid| profiles.contains_key(pid))
    }

    pub(super) fn route_for_profile(
        &self,
        profile_id: &str,
    ) -> Result<BoardLayerRoute, WorkbenchError> {
        let doc = &self.document_store.host_store().draft.document;
        let profile = doc
            .editable
            .graph
            .profiles
            .get(profile_id)
            .ok_or_else(|| WorkbenchError::Routing("所选方案不存在".into()))?;
        if let Some(owner) = mi_model::board_layers::native_owner(doc, profile_id) {
            return Ok(BoardLayerRoute::Native {
                owner,
                profile_id: profile_id.into(),
            });
        }
        if profile
            .layer_order
            .iter()
            .any(|lid| doc.is_protected_oai_layer(lid))
        {
            return Err(WorkbenchError::Routing(
                "受保护方案的原生所有者未确认".into(),
            ));
        }
        Ok(BoardLayerRoute::HostProfile {
            profile_id: profile_id.into(),
        })
    }

    pub(super) fn routing_mode_summary_label(&self) -> String {
        if self.document_store.locked_profile_id().is_some() {
            return "已锁定 · 忽略前台切换与外部干扰".to_string();
        }
        let mode = if self.six_layer_view.is_some() {
            "托管层前台优先"
        } else if !self.auto_follow_enabled {
            "手动模式"
        } else if self.manual_override {
            "手动锁定（自动跟随已暂停）"
        } else {
            "自动跟随前台软件"
        };
        let exec = if self.document_store.host_execution_enabled() {
            "按键控制已启用"
        } else {
            "按键控制已暂停"
        };
        format!("{mode}，{exec}")
    }

    pub(super) fn selected_keycap(&self, slot: u32) -> CodexKeycapId {
        if let Some(pid) = &self.selected_profile_id {
            if let Some(&override_id) = self.keycap_overrides.get(&(pid.clone(), slot)) {
                return override_id;
            }
        }
        let doc = &self.document_store.draft.document;
        let layer = self
            .selected_layer_id
            .as_ref()
            .and_then(|lid| doc.editable.graph.layers.get(lid));
        let (_, action_obj) = resolve_slot_action_info(doc, layer, slot);
        CodexKeycapId::infer_for_slot(slot, action_obj)
    }

    pub(super) fn project_six_layer_runtime(&self, result: &mut BoardLayersProjection) {
        result.restore_available =
            self.six_layer_state.installation.is_some() && !self.six_layer_busy;
        result.apply_block_reason = if self.six_layer_busy {
            "六层操作正在进行".into()
        } else if self
            .six_layer_state
            .installation
            .as_ref()
            .is_some_and(|installation| installation.status != InstallationStatus::Applied)
        {
            "六层恢复记录尚未处理，请先恢复原设备配置".into()
        } else if self
            .six_layer_state
            .installation
            .as_ref()
            .is_some_and(|installation| installation.plan.validate_current().is_err())
        {
            "旧六层安装已暂停，请先恢复原设备配置".into()
        } else if self.isolated {
            "隔离工作台不能应用实体六层方案".into()
        } else {
            self.compiled_six_layer_plan().err().unwrap_or_default()
        };
        result.can_apply = result.apply_block_reason.is_empty();
        if let Some(running) = &self.six_layer_view {
            result.configuration_observed = running.configuration_observed;
            result.keyboard_grabbed = running.keyboard_grabbed && running.failure.is_none();
            result.first_event_verified = running.first_event_verified;
            result.runtime_ready = running.armed
                && running.failure.is_none()
                && self.document_store.host_execution_enabled();
            result.observed_layer = Some(running.observed_layer);
            result.observed_profile_kind = Some("six_layer_direct".into());
            result.observed_status = running.failure.clone().unwrap_or_else(|| {
                let priority = running
                    .foreground_priority_adapter
                    .as_ref()
                    .map(|adapter| format!(" · 前台优先 {adapter}"))
                    .unwrap_or_default();
                format!(
                    "六层配置已核验 · 最近确认第 {} 层{} · {}",
                    running.observed_layer,
                    priority,
                    if running.first_event_verified {
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

    pub(super) fn compiled_six_layer_plan(&self) -> Result<AppliedSixLayerPlan, String> {
        AppliedSixLayerPlan::compile(
            self.document_store
                .board_layer_plan()
                .ok_or("六层方案尚未建立")?,
            &self.document_store.host_store().draft.document,
            self.current_bottom_row_layout(),
            self.command_execution_enabled,
        )
    }
}

fn strip_popover_parens(mut s: &str) -> &str {
    loop {
        s = s.trim();
        let Some(first) = s.chars().next() else {
            return "";
        };
        if first != '(' && first != '（' {
            let base = s.split(['(', '（']).next().unwrap_or(s).trim();
            return if base.is_empty() { s } else { base };
        }

        let mut depth = 0usize;
        let mut close_end = None;
        for (idx, ch) in s.char_indices() {
            match ch {
                '(' | '（' => depth += 1,
                ')' | '）' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        close_end = Some(idx + ch.len_utf8());
                        break;
                    }
                }
                _ => {}
            }
        }

        match close_end {
            Some(end) if end < s.len() => {
                let after = s[end..].trim();
                if !after.is_empty() {
                    s = after;
                    continue;
                }
                return s;
            }
            Some(end) => {
                let last_len = s.chars().next_back().map_or(0, char::len_utf8);
                s = &s[first.len_utf8()..end.saturating_sub(last_len)];
            }
            None => {
                let rest = s[first.len_utf8()..].trim();
                return if rest.is_empty() { s } else { rest };
            }
        }
    }
}

pub fn format_popover_cheat_label(raw_label: &str) -> String {
    let trimmed = raw_label.trim();
    if trimmed.is_empty() {
        return "未绑定".to_string();
    }

    let dot_selected = if let Some((left, right)) = trimmed.split_once('·') {
        let left_trim = left.trim();
        let right_trim = right.trim();
        if right_trim.starts_with("松开停止")
            || right_trim.starts_with("AG")
            || right_trim == "ENC_CLK"
            || right_trim.is_empty()
        {
            left_trim
        } else if left_trim.is_empty()
            || left_trim == "粘贴文本"
            || left_trim == "命令"
            || left_trim == "Codex"
            || left_trim == "Antigravity"
        {
            right_trim
        } else {
            trimmed
        }
    } else {
        trimmed
    };

    let segment = strip_popover_parens(dot_selected);
    if segment.is_empty() {
        return "未绑定".to_string();
    }

    let stripped = if let Some(rest) = segment
        .strip_prefix("打开 ")
        .map(str::trim)
        .filter(|r| !r.is_empty() && !r.starts_with('/'))
    {
        rest
    } else {
        segment
    };

    let normalized = if let Some(alias) = crate::canvas::concise_dial_caption_alias(stripped) {
        alias
    } else {
        match stripped {
            "打开 / 聚焦应用" => "聚焦应用",
            "不可用 / 未配置" => "未配置",
            "聚焦当前输入框" => "聚焦输入框",
            "在项目中向上滚动" => "项目上滚",
            "在项目中向下滚动" => "项目下滚",
            "显示 / 隐藏侧栏" => "显隐侧栏",
            "选择 Space / Tab" => "Space/Tab",
            "选项目 / 新建项目" => "选/建项目",
            "项目工作区快切" => "工作区快切",
            "停止听写并提交" => "停止提交",
            "快速向右分栏" => "向右分栏",
            "内置浏览器 · 布局" => "浏览器/布局",
            "打开内置浏览器" => "内置浏览器",
            "打开终端面板" => "终端面板",
            "新建 Tab / 终端" => "新建 Tab",
            "放大 / 还原面板" => "缩放面板",
            "聚焦上方面板" => "上方面板",
            "聚焦下方面板" => "下方面板",
            "聚焦左方面板" => "左方面板",
            "聚焦右方面板" => "右方面板",
            "向上切分屏焦点" | "北向分屏焦点" => "上方分屏",
            "向下切分屏焦点" | "南向分屏焦点" => "下方分屏",
            "向左切分屏焦点" | "西向分屏焦点" => "左方分屏",
            "向右切分屏焦点" | "东向分屏焦点" => "右方分屏",
            other => other,
        }
    };

    if normalized.is_empty() {
        return "未绑定".to_string();
    }

    let width_units: usize = normalized
        .chars()
        .map(|c| if c.is_ascii() { 1 } else { 2 })
        .sum();
    if width_units <= 16 {
        return normalized.to_string();
    }

    let mut acc = String::new();
    let mut used = 0usize;
    for ch in normalized.chars() {
        let w = if ch.is_ascii() { 1 } else { 2 };
        if used + w > 14 {
            break;
        }
        acc.push(ch);
        used += w;
    }
    let compact = acc.trim_end();
    if compact.is_empty() {
        "未绑定".to_string()
    } else {
        format!("{compact}…")
    }
}

fn index_layer_controls<'a>(
    layer: Option<&'a LayerNode>,
) -> [Option<&'a mi_model::document::BindingNode>; 20] {
    let mut slots = [None; 20];
    let Some(layer) = layer else {
        return slots;
    };
    for binding in layer.controls.values() {
        if let Some(idx) = binding
            .logical_control
            .strip_prefix("logical-")
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|&idx| idx < 20)
        {
            slots[idx] = Some(binding);
        }
    }
    slots
}

fn resolve_indexed_slot<'a>(
    doc: &'a LocalDocumentV2,
    indexed: &[Option<&'a mi_model::document::BindingNode>; 20],
    slot: u32,
) -> (String, Option<&'a Action>) {
    let Some(binding) = indexed.get(slot as usize).and_then(|b| *b) else {
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

fn resolve_indexed_merged_bottom_slot(
    doc: &LocalDocumentV2,
    indexed: &[Option<&mi_model::document::BindingNode>; 20],
) -> u32 {
    let (_, act10) = resolve_indexed_slot(doc, indexed, 10);
    let (_, act11) = resolve_indexed_slot(doc, indexed, 11);
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

fn is_session_slot_action(action: &Action, slot_idx: usize) -> bool {
    const SLOT_OPEN_IDS: [&str; 6] = [
        "slot-open-0",
        "slot-open-1",
        "slot-open-2",
        "slot-open-3",
        "slot-open-4",
        "slot-open-5",
    ];
    const COCKPIT_SLOT_IDS: [&str; 6] = [
        "cockpit-slot-0",
        "cockpit-slot-1",
        "cockpit-slot-2",
        "cockpit-slot-3",
        "cockpit-slot-4",
        "cockpit-slot-5",
    ];
    const SLOT_OPEN_CMDS: [&str; 6] = [
        "slot-open:0",
        "slot-open:1",
        "slot-open:2",
        "slot-open:3",
        "slot-open:4",
        "slot-open:5",
    ];
    let Some(&open_id) = SLOT_OPEN_IDS.get(slot_idx) else {
        return false;
    };
    action.id == open_id
        || action.id == COCKPIT_SLOT_IDS[slot_idx]
        || action.payload.get("command").and_then(|v| v.as_str()) == Some(SLOT_OPEN_CMDS[slot_idx])
}
