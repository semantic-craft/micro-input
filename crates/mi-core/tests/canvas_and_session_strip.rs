use mi_bridge::FourAppAgentPhase;
use mi_core::{
    format_concise_keycap_caption, AmbientColorPreset, BottomRowLayout, CanonicalAgent,
    CanvasAction, ControlSurfaceId, FourAppLiveCandidate, FourAppSessionKind, JoystickDirection8,
    KnobJoystickPreset, LightingBrightnessPreset, LightingTimeoutPreset, SessionStripAction,
    WorkbenchError, WorkbenchStore, CANVAS_HEIGHT, CANVAS_PADDING, CANVAS_WIDTH, KEY_1U_SIZE,
    KEY_2U_WIDTH, KEY_GAP,
};
use mi_model::{navigation_history::NavigationPlace, slot_table::SlotMode};
use tempfile::TempDir;

#[test]
fn hit_testing_covers_all_17_surfaces_20_slots_radial_encoder_and_8_way_joystick() {
    let temp = TempDir::new().unwrap();
    let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();

    assert_eq!(KEY_1U_SIZE, 68.0);
    assert_eq!(KEY_GAP, 8.0);
    assert_eq!(KEY_2U_WIDTH, 144.0);
    assert_eq!(CANVAS_PADDING, 16.0);
    assert_eq!(CANVAS_WIDTH, 328.0);
    assert_eq!(CANVAS_HEIGHT, 328.0);
    assert_eq!(ControlSurfaceId::ALL_17.len(), 17);

    let expected_bindings = [
        (ControlSurfaceId::EncoderCcw, 13, "AG13"),
        (ControlSurfaceId::EncoderCw, 14, "AG14"),
        (ControlSurfaceId::EncoderPress, 15, "ENC_CLK"),
        (ControlSurfaceId::Joystick, 16, "AG15"),
        (ControlSurfaceId::Key0, 0, "AG00"),
        (ControlSurfaceId::Key1, 1, "AG01"),
        (ControlSurfaceId::Key2, 2, "AG02"),
        (ControlSurfaceId::Key3, 3, "AG03"),
        (ControlSurfaceId::Key4, 4, "AG04"),
        (ControlSurfaceId::Key5, 5, "AG05"),
        (ControlSurfaceId::Key6, 6, "AG06"),
        (ControlSurfaceId::Key7, 7, "AG07"),
        (ControlSurfaceId::Key8, 8, "AG08"),
        (ControlSurfaceId::Key9, 9, "AG09"),
        (ControlSurfaceId::Key10, 10, "AG10"),
        (ControlSurfaceId::Key11, 11, "AG11"),
        (ControlSurfaceId::Key12, 12, "AG12"),
    ];

    for (surface, slot, binding_id) in expected_bindings {
        assert_eq!(surface.primary_logical_slot(), slot);
        assert_eq!(surface.binding_id(), binding_id);
        assert_eq!(
            ControlSurfaceId::from_logical_slot(slot, BottomRowLayout::Split),
            Some(surface)
        );

        let bounds = store
            .projection()
            .canvas
            .surface_bounds(surface)
            .expect("every surface in ALL_17 must have bounds in Split layout");
        let (cx, cy) = bounds.center();
        assert_eq!(
            store.projection().canvas.hit_test_point(cx, cy),
            Some(surface),
            "center of {surface:?} must hit-test back to itself"
        );

        store
            .dispatch_canvas_action(CanvasAction::SelectSurface(surface))
            .unwrap();
        assert_eq!(store.projection().canvas.selected_surface, surface);
        assert_eq!(store.selected_binding_slot(), Some(slot));

        store
            .dispatch_canvas_action(CanvasAction::ClickCanvasPoint {
                x_milli_px: (cx * 1000.0) as i32,
                y_milli_px: (cy * 1000.0) as i32,
            })
            .unwrap();
        assert_eq!(store.projection().canvas.selected_surface, surface);
        assert_eq!(store.selected_binding_slot(), Some(slot));
    }

    assert_eq!(
        store.projection().canvas.hit_test_point(50.0, 50.0),
        Some(ControlSurfaceId::EncoderPress)
    );
    assert_eq!(
        store.projection().canvas.hit_test_point(22.0, 50.0),
        Some(ControlSurfaceId::EncoderCcw)
    );
    assert_eq!(
        store.projection().canvas.hit_test_point(78.0, 50.0),
        Some(ControlSurfaceId::EncoderCw)
    );

    for slot in 0..=19 {
        store
            .dispatch_canvas_action(CanvasAction::SelectLogicalSlot(slot))
            .unwrap();
        assert_eq!(store.selected_binding_slot(), Some(slot));
        assert_eq!(store.projection().canvas.selected_logical_slot, slot);
        let mapped = ControlSurfaceId::from_logical_slot(slot, BottomRowLayout::Split).unwrap();
        assert_eq!(store.projection().canvas.selected_surface, mapped);
    }
    assert_eq!(
        ControlSurfaceId::from_logical_slot(20, BottomRowLayout::Split),
        None
    );

    let (joy_cx, joy_cy) = store.projection().canvas.joystick.bounds.center();
    assert_eq!(
        store.projection().canvas.hit_test_point(joy_cx, joy_cy),
        Some(ControlSurfaceId::Joystick)
    );
    assert_eq!(
        store
            .projection()
            .canvas
            .hit_test_joystick_direction(joy_cx, joy_cy),
        None,
        "center deadzone of joystick pad must not resolve a radial direction"
    );

    assert_eq!(JoystickDirection8::ALL.len(), 8);
    assert_eq!(store.projection().canvas.joystick.sectors.len(), 8);
    for dir in JoystickDirection8::ALL {
        assert!(!dir.label().is_empty());
        let badge = store
            .projection()
            .canvas
            .joystick
            .sectors
            .iter()
            .find(|b| b.direction == dir)
            .expect("each of the 8 joystick directions must have a sector badge");
        let (bx, by) = badge.bounds.center();
        assert_eq!(
            store
                .projection()
                .canvas
                .hit_test_joystick_direction(bx, by),
            Some(dir)
        );

        store
            .dispatch_canvas_action(CanvasAction::SelectJoystickDirection(dir))
            .unwrap();
        assert_eq!(store.projection().canvas.selected_joystick_direction, dir);
        assert_eq!(
            store.selected_binding_slot(),
            Some(dir.logical_sector_slot())
        );
        assert!((16..=19).contains(&dir.logical_sector_slot()));
    }
}

#[test]
fn bottom_row_2u_wide_key_merge_split_and_silkscreen_icon_label_toggles() {
    let temp = TempDir::new().unwrap();
    let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();

    assert_eq!(
        store.projection().canvas.bottom_row_layout,
        BottomRowLayout::Split
    );
    assert_eq!(store.projection().canvas.visible_keycap_cells().len(), 13);
    let key10_split = store
        .projection()
        .canvas
        .cell_for_surface(ControlSurfaceId::Key10)
        .unwrap();
    let key11_split = store
        .projection()
        .canvas
        .cell_for_surface(ControlSurfaceId::Key11)
        .unwrap();
    assert_eq!(key10_split.bounds.width, 68.0);
    assert_eq!(key10_split.bounds.height, 68.0);
    assert_eq!(key11_split.bounds.width, 68.0);
    assert_eq!(key11_split.bounds.height, 68.0);

    store
        .dispatch_canvas_action(CanvasAction::SetBottomRowLayout(BottomRowLayout::Merged))
        .unwrap();
    assert_eq!(
        store.projection().canvas.bottom_row_layout,
        BottomRowLayout::Merged
    );
    assert_eq!(store.projection().canvas.visible_keycap_cells().len(), 12);
    let key10_merged = store
        .projection()
        .canvas
        .cell_for_surface(ControlSurfaceId::Key10)
        .unwrap();
    assert_eq!(key10_merged.bounds.width, 144.0);
    assert_eq!(key10_merged.bounds.height, 68.0);
    assert_eq!(key10_merged.span_cols, 2);
    assert!(store
        .projection()
        .canvas
        .cell_for_surface(ControlSurfaceId::Key11)
        .is_none());
    assert_eq!(
        ControlSurfaceId::from_logical_slot(11, BottomRowLayout::Merged),
        Some(ControlSurfaceId::Key10)
    );
    assert_eq!(
        store.projection().canvas.hit_test_point(180.0, 278.0),
        Some(ControlSurfaceId::Key10),
        "right half of merged 2U key must hit-test to Key10"
    );

    let other_pid = store
        .find_canonical_profile_id(CanonicalAgent::OtherApps)
        .unwrap();
    store.select_and_activate_profile(&other_pid).unwrap();
    let other_wide_cell = store
        .projection()
        .canvas
        .cell_for_surface(ControlSurfaceId::Key10)
        .unwrap();
    assert_eq!(
        other_wide_cell.effective_slot, 11,
        "when slot 10 is unassigned and slot 11 has Apple Fn (0x3F), merged 2U key must fall back to effective_slot 11"
    );
    assert!(other_wide_cell.is_available);
    assert!(other_wide_cell.full_action_title.contains("Fn"));

    store
        .dispatch_canvas_action(CanvasAction::SelectSurface(ControlSurfaceId::Key10))
        .unwrap();
    assert_eq!(store.selected_binding_slot(), Some(11));

    store
        .dispatch_canvas_action(CanvasAction::SetBottomRowLayout(BottomRowLayout::Split))
        .unwrap();
    assert_eq!(store.projection().canvas.visible_keycap_cells().len(), 13);

    store
        .dispatch_canvas_action(CanvasAction::SetShowKeycapIcons(false))
        .unwrap();
    assert!(!store.projection().canvas.show_keycap_icons);
    store
        .dispatch_canvas_action(CanvasAction::SetShowKeycapLabels(false))
        .unwrap();
    assert!(!store.projection().canvas.show_keycap_labels);
    store
        .dispatch_canvas_action(CanvasAction::SetShowKeycapIcons(true))
        .unwrap();
    store
        .dispatch_canvas_action(CanvasAction::SetShowKeycapLabels(true))
        .unwrap();
    assert!(store.projection().canvas.show_keycap_icons);
    assert!(store.projection().canvas.show_keycap_labels);

    let truncated_cjk = format_concise_keycap_caption("四应用槽位 1 · 聚焦当前输入框与超长标签");
    assert!(
        truncated_cjk.chars().count() <= 4,
        "concise caption '{truncated_cjk}' must be <= 4 chars"
    );
    let truncated_plain = format_concise_keycap_caption("按住说话松开停止");
    assert_eq!(truncated_plain, "按住说话");
    assert_eq!(
        format_concise_keycap_caption("Gemini Live 实时语音"),
        "实时语音"
    );
    assert_eq!(
        format_concise_keycap_caption("Antigravity · Gemini Live 实时语音"),
        "实时语音"
    );
}

#[test]
fn mg1_protected_oai_layer_enforces_all_four_hard_isolation_invariants_and_one_click_unlock() {
    let temp = TempDir::new().unwrap();
    let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();

    for agent in [CanonicalAgent::Codex] {
        let pid = store.find_canonical_profile_id(agent).unwrap();
        store.select_and_activate_profile(&pid).unwrap();
        assert!(store.is_selected_layer_protected());
        assert_eq!(
            store.ensure_layer_editable(),
            Err(WorkbenchError::ProtectedLayerLocked)
        );

        let lock = store
            .projection()
            .canvas
            .protected_lock
            .as_ref()
            .expect("protected layer must project ProtectedLayerLockProjection");
        assert!(lock.is_locked);
        assert!(!lock.lock_badge_text.is_empty());
        assert!(!lock.explanation_text.is_empty());

        assert!(!store.projection().canvas.can_edit_bindings);
        assert!(!store.projection().canvas.inspector_expanded);
        assert!(!store.projection().session_strip.can_mutate_layout);
        assert_eq!(
            store.dispatch_session_strip_action(SessionStripAction::BindTopSixAsSessionKeys),
            Err(WorkbenchError::ProtectedLayerLocked)
        );
        assert_eq!(
            store.dispatch_session_strip_action(SessionStripAction::RestoreDefaultLayout),
            Err(WorkbenchError::ProtectedLayerLocked)
        );
        assert_eq!(
            store.dispatch_canvas_action(CanvasAction::ApplyKnobJoystickPreset(
                KnobJoystickPreset::CycleWorkingAndBlueDot
            )),
            Err(WorkbenchError::ProtectedLayerLocked)
        );
        assert_eq!(
            store.dispatch_canvas_action(CanvasAction::AssignSlotAction {
                slot: 0,
                action_id: "search".to_string(),
            }),
            Err(WorkbenchError::ProtectedLayerLocked)
        );

        assert!(!store.projection().canvas.can_add_lighting);
        assert!(!store.projection().canvas.can_export_or_copy_layer);
        assert!(
            store
                .projection()
                .session_strip
                .lighting_popover
                .suspended_by_protected_layer
        );
        assert_eq!(
            store.dispatch_session_strip_action(SessionStripAction::TriggerTestLighting(Some(
                FourAppAgentPhase::Completed
            ))),
            Err(WorkbenchError::ProtectedLayerLocked)
        );
    }

    let codex_pid = store
        .find_canonical_profile_id(CanonicalAgent::Codex)
        .unwrap();
    store.select_and_activate_profile(&codex_pid).unwrap();
    let protected_lid = store.selected_layer_id().unwrap().to_string();

    store
        .dispatch_canvas_action(CanvasAction::UnlockBySwitchingOrCreatingCustomLayer)
        .unwrap();
    let custom_lid = store.selected_layer_id().unwrap().to_string();
    assert_ne!(custom_lid, protected_lid);
    assert!(!store.is_selected_layer_protected());
    assert!(store.projection().canvas.protected_lock.is_none());
    assert!(store.projection().canvas.can_edit_bindings);
    assert!(store.projection().canvas.can_add_lighting);
    assert!(store.projection().canvas.can_export_or_copy_layer);
    assert!(store.projection().canvas.inspector_expanded);

    store
        .dispatch_canvas_action(CanvasAction::AssignSlotAction {
            slot: 11,
            action_id: "apple-fn-key".to_string(),
        })
        .unwrap();
    store
        .dispatch_canvas_action(CanvasAction::MergeBottomRowAcrossLayers)
        .unwrap();

    let doc = &store.document_store.draft.document;
    assert!(
        doc.is_protected_oai_layer(&protected_lid),
        "protected Mg1 layer must remain intact after cross-layer 2U merge"
    );
    let custom_slot_11_assigned = doc.editable.graph.layers[&custom_lid]
        .controls
        .values()
        .find(|b| b.logical_control == "logical-11")
        .and_then(|b| b.action_id.as_ref());
    assert!(
        custom_slot_11_assigned.is_none(),
        "editable layer slot 11 must be cleared by cross-layer 2U merge"
    );
}

#[test]
fn four_app_six_slot_session_strip_sorting_pinning_mark_read_lighting_and_quick_actions() {
    let temp = TempDir::new().unwrap();
    let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();

    let ag_pid = store
        .find_canonical_profile_id(CanonicalAgent::Antigravity)
        .unwrap();
    store.select_and_activate_profile(&ag_pid).unwrap();

    let place_idle_recent = NavigationPlace::antigravity("conv-idle", "/ws/idle");
    let place_viewed_done = NavigationPlace::session("dsh-desktop", "sess-viewed");
    let place_unread_done = NavigationPlace::antigravity("conv-done", "/ws/done");
    let place_working = NavigationPlace::session("dsh-desktop", "sess-working");
    let place_attention = NavigationPlace::dsh("sess-attn", "ws-dsh", "host-1");

    store.ingest_four_app_candidates(vec![
        FourAppLiveCandidate {
            place: place_idle_recent.clone(),
            title: "Idle But High Timestamp".into(),
            phase: FourAppSessionKind::Idle,
            watermark: "wm-idle".into(),
            last_modified_ms: 9_000,
            last_user_view_ms: 9_000,
            updated_at_ms: 9_000,
        },
        FourAppLiveCandidate {
            place: place_viewed_done.clone(),
            title: "Already Viewed Completed".into(),
            phase: FourAppSessionKind::BlueDotCompleted,
            watermark: "wm-viewed".into(),
            last_modified_ms: 5_200,
            last_user_view_ms: 5_000,
            updated_at_ms: 8_000,
        },
        FourAppLiveCandidate {
            place: place_unread_done.clone(),
            title: "Unread Blue Dot Task".into(),
            phase: FourAppSessionKind::BlueDotCompleted,
            watermark: "wm-unread-1".into(),
            last_modified_ms: 6_000,
            last_user_view_ms: 2_000,
            updated_at_ms: 6_000,
        },
        FourAppLiveCandidate {
            place: place_working.clone(),
            title: "Codex Running Build".into(),
            phase: FourAppSessionKind::Working,
            watermark: "wm-work-1".into(),
            last_modified_ms: 7_000,
            last_user_view_ms: 7_000,
            updated_at_ms: 7_000,
        },
        FourAppLiveCandidate {
            place: place_attention.clone(),
            title: "DSH Approval Needed".into(),
            phase: FourAppSessionKind::NeedsAttention,
            watermark: "wm-attn-1".into(),
            last_modified_ms: 6_500,
            last_user_view_ms: 1_000,
            updated_at_ms: 6_500,
        },
    ]);

    let strip = &store.projection().session_strip;
    assert_eq!(strip.attention_pills.len(), 1);
    assert_eq!(strip.working_pills.len(), 1);
    assert_eq!(strip.completed_blue_dot_pills.len(), 1);
    assert_eq!(strip.unread_completed_count, 1);
    assert_eq!(strip.working_count, 1);
    assert_eq!(
        strip.completed_blue_dot_pills[0].place,
        place_unread_done,
        "only completed session with last_modified_ms > last_user_view_ms + 500 must appear in unread blue-dot pills"
    );

    let first_three_places: Vec<NavigationPlace> = strip.slots[0..3]
        .iter()
        .map(|s| s.place.clone().unwrap())
        .collect();
    assert!(
        first_three_places.contains(&place_attention)
            && first_three_places.contains(&place_working)
            && first_three_places.contains(&place_unread_done),
        "active sessions (NeedsAttention, Working, unread BlueDotCompleted) must sort ahead of Idle sessions"
    );
    assert_eq!(strip.slots[3].kind, FourAppSessionKind::Idle);
    assert_eq!(strip.slots[4].kind, FourAppSessionKind::Idle);

    let pinned_place = strip.slots[0].place.clone().unwrap();
    store
        .dispatch_session_strip_action(SessionStripAction::TogglePinSlot(0))
        .unwrap();
    assert_eq!(
        store.projection().session_strip.slots[0].mode,
        SlotMode::Pinned
    );

    store.ingest_four_app_candidates(vec![FourAppLiveCandidate {
        place: place_working.clone(),
        title: "Codex Running Build Updated".into(),
        phase: FourAppSessionKind::Working,
        watermark: "wm-work-2".into(),
        last_modified_ms: 20_000,
        last_user_view_ms: 20_000,
        updated_at_ms: 20_000,
    }]);
    assert_eq!(
        store.projection().session_strip.slots[0].mode,
        SlotMode::Pinned
    );
    assert_eq!(
        store.projection().session_strip.slots[0].place.as_ref(),
        Some(&pinned_place)
    );

    store
        .dispatch_session_strip_action(SessionStripAction::JumpToSession(place_working.clone()))
        .unwrap();
    assert_eq!(store.navigation_history().current(), None);

    store
        .dispatch_session_strip_action(SessionStripAction::ActivateSlot(0))
        .unwrap();
    assert_eq!(store.navigation_history().current(), None);

    store.ingest_four_app_candidates(vec![FourAppLiveCandidate {
        place: place_unread_done.clone(),
        title: "Unread Blue Dot Task".into(),
        phase: FourAppSessionKind::BlueDotCompleted,
        watermark: "wm-unread-2".into(),
        last_modified_ms: 30_000,
        last_user_view_ms: 2_000,
        updated_at_ms: 30_000,
    }]);
    assert_eq!(store.projection().session_strip.unread_completed_count, 1);
    assert_eq!(
        store
            .projection()
            .session_strip
            .completed_blue_dot_pills
            .len(),
        1
    );

    store
        .dispatch_session_strip_action(SessionStripAction::AcknowledgeAllCompleted)
        .unwrap();
    assert_eq!(store.projection().session_strip.unread_completed_count, 0);
    assert!(store
        .projection()
        .session_strip
        .completed_blue_dot_pills
        .is_empty());
    assert_eq!(
        store
            .lighting_arbiter
            .acknowledged_watermarks
            .get(&place_unread_done.target_id())
            .map(String::as_str),
        Some("wm-unread-2")
    );
    let dual = store.lighting_arbiter.dual_zone_feedback.as_ref().unwrap();
    assert_eq!(
        dual.underglow_color, "#000000",
        "acknowledging all completed sessions must extinguish completion glow"
    );

    store
        .dispatch_session_strip_action(SessionStripAction::ToggleLightingPopover)
        .unwrap();
    assert!(store.projection().session_strip.lighting_popover.is_open);

    store
        .dispatch_session_strip_action(SessionStripAction::SetBrightnessPreset(
            LightingBrightnessPreset::Soft,
        ))
        .unwrap();
    assert_eq!(
        store.projection().session_strip.lighting_popover.brightness,
        LightingBrightnessPreset::Soft
    );
    assert!((store.lighting_arbiter.brightness_scale - 0.35).abs() < 1e-6);

    store
        .dispatch_session_strip_action(SessionStripAction::SetAmbientColor(
            AmbientColorPreset::EmeraldGreen,
        ))
        .unwrap();
    assert_eq!(store.lighting_arbiter.idle_color_hex, "#00FF66");

    store
        .dispatch_session_strip_action(SessionStripAction::SetWorkingColor(
            AmbientColorPreset::AmberGold,
        ))
        .unwrap();
    assert_eq!(store.lighting_arbiter.working_color_hex, "#FFAA00");

    store
        .dispatch_session_strip_action(SessionStripAction::SetAmbientColor(
            AmbientColorPreset::OceanBlue,
        ))
        .unwrap();
    assert_eq!(store.lighting_arbiter.idle_color_hex, "#0088FF");

    store
        .dispatch_session_strip_action(SessionStripAction::SetAutoOffTimeout(
            LightingTimeoutPreset::FiveMinutes,
        ))
        .unwrap();
    assert_eq!(
        store
            .projection()
            .session_strip
            .lighting_popover
            .auto_off_timeout,
        LightingTimeoutPreset::FiveMinutes
    );

    store
        .dispatch_session_strip_action(SessionStripAction::SetGlowOnWorking(true))
        .unwrap();
    assert!(store.lighting_arbiter.glow_on_working_sessions);

    store
        .dispatch_session_strip_action(SessionStripAction::SetOnlyGlowOnAttentionOrCompleted(false))
        .unwrap();
    assert!(!store.lighting_arbiter.only_glow_on_attention_or_completed);

    store
        .dispatch_session_strip_action(SessionStripAction::SetAutoSessionColor(false))
        .unwrap();
    assert!(!store.lighting_arbiter.session_lighting_linked);

    store
        .dispatch_session_strip_action(SessionStripAction::SetLightingEnabled(false))
        .unwrap();
    assert!(!store.lighting_arbiter.lighting_enabled);
    store
        .dispatch_session_strip_action(SessionStripAction::SetLightingEnabled(true))
        .unwrap();
    store
        .dispatch_session_strip_action(SessionStripAction::SetAutoSessionColor(true))
        .unwrap();

    store
        .dispatch_session_strip_action(SessionStripAction::TriggerTestLighting(Some(
            FourAppAgentPhase::AwaitingInput,
        )))
        .unwrap();
    assert_eq!(
        store.lighting_arbiter.manual_preview_phase,
        Some(FourAppAgentPhase::AwaitingInput)
    );

    assert!(
        !store
            .projection()
            .session_strip
            .top_six_bound_as_session_keys
    );
    store
        .dispatch_session_strip_action(SessionStripAction::BindTopSixAsSessionKeys)
        .unwrap();
    assert!(
        store
            .projection()
            .session_strip
            .top_six_bound_as_session_keys
    );
    for slot_idx in 0..6 {
        let cell = store
            .projection()
            .canvas
            .cell_for_surface(
                ControlSurfaceId::from_logical_slot(slot_idx as u32, BottomRowLayout::Split)
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(cell.is_session_slot, Some(slot_idx));
    }

    store
        .dispatch_canvas_action(CanvasAction::ApplyKnobJoystickPreset(
            KnobJoystickPreset::CycleWorkingAndBlueDot,
        ))
        .unwrap();
    assert!(store
        .projection()
        .canvas
        .encoder
        .ccw_label
        .contains("Working"));

    store
        .dispatch_canvas_action(CanvasAction::ApplyKnobJoystickPreset(
            KnobJoystickPreset::DualScrollProjectsAndChat,
        ))
        .unwrap();
    assert!(store.projection().canvas.encoder.ccw_label.contains("滚动"));

    store
        .dispatch_session_strip_action(SessionStripAction::RestoreDefaultLayout)
        .unwrap();
    assert!(
        !store
            .projection()
            .session_strip
            .top_six_bound_as_session_keys
    );
}
