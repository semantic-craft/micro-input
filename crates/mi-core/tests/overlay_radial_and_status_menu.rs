use mi_core::{
    format_popover_cheat_label, ActiveHudContent, BackupCommand, BottomRowLayout, CanonicalAgent,
    CanvasAction, FavoriteSlotIndex, FourAppLiveCandidate, FourAppSessionKind, HostOverlayAction,
    HudVisibilityState, PopoverPreviewMode, PresetExportScope, RadialConfirmMode, RadialInputEvent,
    RadialIntent, RadialPhase, RadialPositionSample, RadialPresetKind, RadialSlotConfig,
    SectorIndex, SidebarAction, StatusMenuAction, WorkbenchCommand, WorkbenchStore,
    WorkbenchStoreConfig, ESCAPE_KEY_CODE, PASSIVE_NOTICE_DURATION,
};
use mi_macos::{
    PassivePanelFrameSpec, PassivePanelHost, PassivePanelRole, PresetFileEnvelope,
    RecordingPassivePanelHost, RecordingStatusItemHost, ScreenGeometry, ScreenPoint, ScreenRect,
    StatusIconPrimitive, StatusItemHost, StatusItemIconSpec, MAX_PRESET_FILE_BYTES,
};
use mi_model::navigation_history::NavigationPlace;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::tempdir;

fn open_test_store() -> (
    tempfile::TempDir,
    WorkbenchStore,
    Arc<RecordingPassivePanelHost>,
    Arc<RecordingStatusItemHost>,
) {
    let dir = tempdir().expect("tempdir");
    let mut store = WorkbenchStore::open_isolated(dir.path()).expect("open isolated store");
    store
        .set_host_execution_enabled(true)
        .expect("enable host execution");
    let panel_host = Arc::new(RecordingPassivePanelHost::new());
    let status_item_host = Arc::new(RecordingStatusItemHost::new());
    store.set_hosts(panel_host.clone(), status_item_host.clone());
    (dir, store, panel_host, status_item_host)
}

#[test]
fn radial_8_way_selection_and_four_app_prompt_preview() {
    let (_dir, mut store, panel_host, _status_item_host) = open_test_store();
    let now = Instant::now();
    let screen = ScreenGeometry::single(ScreenRect::new(0.0, 0.0, 1440.0, 900.0));
    store.update_screen_context(screen, ScreenPoint::new(100.0, 120.0), false, now);

    let expected_sectors = [
        (0.0, SectorIndex::EAST),
        (0.125, SectorIndex::SOUTH_EAST),
        (0.25, SectorIndex::SOUTH),
        (0.375, SectorIndex::SOUTH_WEST),
        (0.50, SectorIndex::WEST),
        (0.625, SectorIndex::NORTH_WEST),
        (0.75, SectorIndex::NORTH),
        (0.875, SectorIndex::NORTH_EAST),
    ];
    for (angle, expected) in expected_sectors {
        assert_eq!(SectorIndex::from_polar(angle, 0.80), Some(expected));
    }
    assert_eq!(SectorIndex::from_polar(0.0, 0.49), None);

    let antigravity_id = store
        .find_canonical_profile_id(CanonicalAgent::Antigravity)
        .expect("Antigravity profile exists");
    store
        .select_and_activate_profile(&antigravity_id)
        .expect("activate Antigravity");

    store.slot_table.update_candidates(
        vec![FourAppLiveCandidate {
            place: NavigationPlace::antigravity("agy-session-r10", "r10-workspace"),
            title: "R10 Overlay Session".to_string(),
            phase: FourAppSessionKind::Working,
            watermark: "wm-1".to_string(),
            last_modified_ms: 1000,
            last_user_view_ms: 500,
            updated_at_ms: 1000,
        }],
        &std::collections::BTreeMap::new(),
    );

    let launcher_slots = store.resolve_current_radial_slots();
    assert_eq!(launcher_slots[1].intent, Some(RadialIntent::CheatSheet));
    assert_eq!(launcher_slots[2].intent, None);
    assert!(launcher_slots[2].unavailable_reason.is_some());

    store.apply_radial_preset(RadialPresetKind::PaseoEngineering);
    let paseo_slots = store.resolve_current_radial_slots();
    assert_eq!(
        paseo_slots[6].intent,
        Some(RadialIntent::Submit {
            adapter: Some("antigravity".to_string()),
        })
    );

    store.apply_radial_preset(RadialPresetKind::FourAppEngineering);
    store.set_radial_slot_config(
        SectorIndex::SOUTH_EAST,
        RadialSlotConfig::prompt(
            "Test",
            "Run the tests and fix any failures\n\r\n",
            None,
            false,
            Some("checkmark.shield"),
            Some("#00FF66"),
        ),
    );
    let four_app_slots = store.resolve_current_radial_slots();
    assert_eq!(
        four_app_slots[1].intent,
        Some(RadialIntent::Prompt {
            text: "Run the tests and fix any failures".to_string(),
            adapter: Some("antigravity".to_string()),
            submit: false,
        })
    );
    assert!(four_app_slots[1]
        .target_preview
        .as_deref()
        .is_some_and(
            |preview| preview.contains("Antigravity") && preview.contains("R10 Overlay Session")
        ));
    assert_eq!(
        four_app_slots[6].intent,
        Some(RadialIntent::Stop {
            adapter: Some("antigravity".to_string()),
        })
    );

    store.set_radial_menu_enabled(true);
    assert!(matches!(
        store.radial_state().phase(),
        RadialPhase::WaitingForCenter
    ));

    let blocked_while_waiting = store.consume_radial_input(
        RadialInputEvent::Position {
            session_id: 1,
            ingress_seq: 1,
            sample: RadialPositionSample::new(0.125, 0.80),
            admitted: true,
        },
        now,
    );
    assert!(blocked_while_waiting.consumed);
    assert!(store.radial_state().session().is_none());

    let center_return = store.consume_radial_input(
        RadialInputEvent::Position {
            session_id: 1,
            ingress_seq: 2,
            sample: RadialPositionSample::new(0.0, 0.15),
            admitted: true,
        },
        now + Duration::from_millis(20),
    );
    assert!(center_return.consumed);
    assert!(matches!(store.radial_state().phase(), RadialPhase::Idle));

    let opened = store.consume_radial_input(
        RadialInputEvent::Position {
            session_id: 1,
            ingress_seq: 3,
            sample: RadialPositionSample::new(0.125, 0.80),
            admitted: true,
        },
        now + Duration::from_millis(40),
    );
    assert!(opened.consumed);
    let active_session = store
        .radial_state()
        .session()
        .expect("radial menu is showing");
    assert_eq!(active_session.selected, Some(SectorIndex::SOUTH_EAST));

    let radial_panel = panel_host
        .visible_panel(PassivePanelRole::RadialMenu)
        .expect("radial panel presented on host");
    assert_eq!(radial_panel.title, "Micro Input 轮盘菜单");
    assert_eq!(
        store
            .projection()
            .radial
            .panel_spec
            .as_ref()
            .map(|p| p.title.as_str()),
        Some("Micro Input 轮盘菜单")
    );
    assert_eq!(radial_panel.frame, ScreenRect::new(0.0, 0.0, 500.0, 500.0));
    assert!(radial_panel.style.nonactivating_panel);
    assert!(radial_panel.style.can_join_all_spaces);
    assert!(radial_panel.style.full_screen_auxiliary);
    assert!(!radial_panel.style.can_become_key);
    assert!(radial_panel.style.ignores_mouse_events);

    let confirmed = store.consume_radial_input(
        RadialInputEvent::KeyPress {
            session_id: 1,
            ingress_seq: 4,
            binding_id: "ENC_CLK".to_string(),
            admitted: true,
        },
        now + Duration::from_millis(90),
    );
    assert!(confirmed.consumed);
    assert_eq!(
        confirmed.confirmed_intent,
        Some(RadialIntent::Prompt {
            text: "Run the tests and fix any failures".to_string(),
            adapter: Some("antigravity".to_string()),
            submit: false,
        })
    );
    let receipt = confirmed.dispatched_receipt.expect("receipt emitted");
    assert_eq!(receipt.action_type, "paste_text");
    assert_eq!(receipt.binding_id, "radial-menu");
    assert!(panel_host
        .visible_panel(PassivePanelRole::RadialMenu)
        .is_none());

    store.set_radial_confirm_mode(RadialConfirmMode::SpringBack);
    let t0 = now + Duration::from_millis(200);
    store.consume_radial_input(
        RadialInputEvent::Position {
            session_id: 1,
            ingress_seq: 5,
            sample: RadialPositionSample::new(0.0, 0.10),
            admitted: true,
        },
        t0,
    );
    assert!(matches!(store.radial_state().phase(), RadialPhase::Idle));

    store.consume_radial_input(
        RadialInputEvent::Position {
            session_id: 1,
            ingress_seq: 6,
            sample: RadialPositionSample::new(0.75, 0.85),
            admitted: true,
        },
        t0 + Duration::from_millis(30),
    );
    assert_eq!(
        store.radial_state().session().and_then(|s| s.selected),
        Some(SectorIndex::NORTH)
    );

    let spring_confirmed = store.consume_radial_input(
        RadialInputEvent::Position {
            session_id: 1,
            ingress_seq: 7,
            sample: RadialPositionSample::new(0.75, 0.15),
            admitted: true,
        },
        t0 + Duration::from_millis(180),
    );
    assert!(spring_confirmed.consumed);
    assert_eq!(
        spring_confirmed.confirmed_intent,
        Some(RadialIntent::Stop {
            adapter: Some("antigravity".to_string()),
        })
    );
    assert_eq!(
        spring_confirmed
            .dispatched_receipt
            .as_ref()
            .map(|r| r.action_type.as_str()),
        Some("agent_command")
    );
}

#[test]
fn radial_center_cancel_zero_side_effects_and_enc_clk_confirm_capture() {
    let (_dir, mut store, panel_host, _status_item_host) = open_test_store();
    let now = Instant::now();
    let antigravity_id = store
        .find_canonical_profile_id(CanonicalAgent::Antigravity)
        .expect("Antigravity profile exists");
    let deepseek_id = store
        .find_canonical_profile_id(CanonicalAgent::DeepseekHarness)
        .expect("Deepseek profile exists");
    store
        .select_and_activate_profile(&antigravity_id)
        .expect("activate Antigravity");

    store.set_radial_menu_enabled(true);
    store.set_radial_confirm_mode(RadialConfirmMode::EncoderPress);
    store.apply_radial_preset(RadialPresetKind::FourAppEngineering);

    store.consume_radial_input(
        RadialInputEvent::Position {
            session_id: 10,
            ingress_seq: 1,
            sample: RadialPositionSample::new(0.0, 0.10),
            admitted: true,
        },
        now,
    );
    store.consume_radial_input(
        RadialInputEvent::Position {
            session_id: 10,
            ingress_seq: 2,
            sample: RadialPositionSample::new(0.125, 0.85),
            admitted: true,
        },
        now + Duration::from_millis(20),
    );
    assert!(store.radial_state().session().is_some());

    let center_cancel = store.consume_radial_input(
        RadialInputEvent::Position {
            session_id: 10,
            ingress_seq: 3,
            sample: RadialPositionSample::new(0.125, 0.20),
            admitted: true,
        },
        now + Duration::from_millis(120),
    );
    assert!(center_cancel.consumed);
    assert!(center_cancel.confirmed_intent.is_none());
    assert!(center_cancel.dispatched_receipt.is_none());
    assert!(store.dispatched_receipts().is_empty());
    assert!(matches!(store.radial_state().phase(), RadialPhase::Idle));
    assert!(panel_host
        .visible_panel(PassivePanelRole::RadialMenu)
        .is_none());

    store.set_radial_confirm_mode(RadialConfirmMode::SpringBack);
    let t_slow = now + Duration::from_millis(300);
    store.consume_radial_input(
        RadialInputEvent::Position {
            session_id: 10,
            ingress_seq: 4,
            sample: RadialPositionSample::new(0.125, 0.85),
            admitted: true,
        },
        t_slow,
    );
    let slow_cancel = store.consume_radial_input(
        RadialInputEvent::Position {
            session_id: 10,
            ingress_seq: 5,
            sample: RadialPositionSample::new(0.125, 0.15),
            admitted: true,
        },
        t_slow + Duration::from_millis(620),
    );
    assert!(slow_cancel.consumed);
    assert!(slow_cancel.confirmed_intent.is_none());
    assert!(slow_cancel.dispatched_receipt.is_none());
    assert!(store.dispatched_receipts().is_empty());

    let t_shallow = t_slow + Duration::from_millis(800);
    store.consume_radial_input(
        RadialInputEvent::Position {
            session_id: 10,
            ingress_seq: 6,
            sample: RadialPositionSample::new(0.125, 0.62),
            admitted: true,
        },
        t_shallow,
    );
    let shallow_cancel = store.consume_radial_input(
        RadialInputEvent::Position {
            session_id: 10,
            ingress_seq: 7,
            sample: RadialPositionSample::new(0.125, 0.15),
            admitted: true,
        },
        t_shallow + Duration::from_millis(120),
    );
    assert!(shallow_cancel.consumed);
    assert!(shallow_cancel.confirmed_intent.is_none());
    assert!(shallow_cancel.dispatched_receipt.is_none());
    assert!(store.dispatched_receipts().is_empty());

    let t_esc = t_shallow + Duration::from_millis(300);
    store.consume_radial_input(
        RadialInputEvent::Position {
            session_id: 10,
            ingress_seq: 8,
            sample: RadialPositionSample::new(0.25, 0.80),
            admitted: true,
        },
        t_esc,
    );
    assert!(store.radial_state().session().is_some());
    let esc_outcome = store.consume_radial_input(
        RadialInputEvent::EscapeKeyDown {
            key_code: ESCAPE_KEY_CODE,
        },
        t_esc + Duration::from_millis(30),
    );
    assert!(esc_outcome.consumed);
    assert!(esc_outcome.dispatched_receipt.is_none());
    assert!(matches!(
        store.radial_state().phase(),
        RadialPhase::WaitingForCenter
    ));

    let t_capture = t_esc + Duration::from_millis(200);
    store.consume_radial_input(
        RadialInputEvent::Position {
            session_id: 10,
            ingress_seq: 9,
            sample: RadialPositionSample::new(0.0, 0.10),
            admitted: true,
        },
        t_capture,
    );
    store.consume_radial_input(
        RadialInputEvent::Position {
            session_id: 10,
            ingress_seq: 10,
            sample: RadialPositionSample::new(0.125, 0.82),
            admitted: true,
        },
        t_capture + Duration::from_millis(30),
    );
    assert!(store.radial_state().captures_confirm());

    store
        .select_and_activate_profile(&deepseek_id)
        .expect("switch profile while radial menu is showing");

    let enc_clk_after_route_change = store.consume_radial_input(
        RadialInputEvent::KeyPress {
            session_id: 10,
            ingress_seq: 11,
            binding_id: "ENC_CLK".to_string(),
            admitted: true,
        },
        t_capture + Duration::from_millis(60),
    );
    assert!(enc_clk_after_route_change.consumed);
    assert!(enc_clk_after_route_change.confirmed_intent.is_none());
    assert!(enc_clk_after_route_change.dispatched_receipt.is_none());
    assert!(store.dispatched_receipts().is_empty());
    assert!(matches!(
        store.radial_state().phase(),
        RadialPhase::WaitingForCenter
    ));
}

#[test]
fn passive_hud_suppressed_when_main_workbench_active_and_cheat_sheet_refreshes_in_place() {
    let (_dir, mut store, panel_host, _status_item_host) = open_test_store();
    let now = Instant::now();
    let screen = ScreenGeometry::single(ScreenRect::new(0.0, 0.0, 1440.0, 900.0));
    let cursor = ScreenPoint::new(720.0, 450.0);

    let antigravity_id = store
        .find_canonical_profile_id(CanonicalAgent::Antigravity)
        .expect("Antigravity profile exists");
    let deepseek_id = store
        .find_canonical_profile_id(CanonicalAgent::DeepseekHarness)
        .expect("Deepseek profile exists");

    store.update_screen_context(screen.clone(), cursor, true, now);
    store
        .select_and_activate_profile_at(&antigravity_id, now, true)
        .expect("switch while main window active");

    assert!(matches!(
        store.hud_visibility(),
        HudVisibilityState::SuppressedByActiveWorkbench
    ));
    assert_eq!(
        store.projection().overlay.content,
        ActiveHudContent::SuppressedByActiveWorkbench
    );
    assert!(panel_host
        .visible_panel(PassivePanelRole::PassiveNotice)
        .is_none());

    let t_bg = now + Duration::from_millis(100);
    store.update_screen_context(screen.clone(), cursor, false, t_bg);
    store
        .select_and_activate_profile_at(&deepseek_id, t_bg, true)
        .expect("switch while main window inactive");

    match store.hud_visibility() {
        HudVisibilityState::PassiveToast {
            profile_name,
            frame,
            ..
        } => {
            assert!(profile_name.contains("DeepSeek") || profile_name.contains("Deepseek"));
            assert_eq!(
                *frame,
                ScreenRect::new(1440.0 - 280.0 - 20.0, 900.0 - 76.0 - 20.0, 280.0, 76.0)
            );
        }
        other => panic!("expected PassiveToast, got {other:?}"),
    }
    let toast_panel = panel_host
        .visible_panel(PassivePanelRole::PassiveNotice)
        .expect("passive toast panel visible");
    assert_eq!(toast_panel.frame.width, 280.0);
    assert_eq!(toast_panel.frame.height, 76.0);
    assert!(toast_panel.style.ignores_mouse_events);
    assert!(toast_panel.style.nonactivating_panel);

    assert!(!store.expire_overlay_if_due(t_bg + Duration::from_millis(1400)));
    assert!(store.expire_overlay_if_due(t_bg + PASSIVE_NOTICE_DURATION));
    assert!(matches!(store.hud_visibility(), HudVisibilityState::Hidden));
    assert!(panel_host
        .visible_panel(PassivePanelRole::PassiveNotice)
        .is_none());

    store
        .dispatch_overlay_action(HostOverlayAction::ToggleCheatSheet)
        .expect("open cheat sheet");
    let cheat_panel = panel_host
        .visible_panel(PassivePanelRole::CheatSheet)
        .expect("cheat sheet panel visible");
    assert_eq!(
        cheat_panel.frame,
        PassivePanelFrameSpec::top_right_hud_frame(
            PassivePanelFrameSpec::CHEAT_SHEET_SIZE,
            ScreenRect::new(0.0, 0.0, 1440.0, 900.0)
        )
    );
    assert_eq!(cheat_panel.frame.width, 400.0);
    assert_eq!(cheat_panel.frame.height, 610.0);
    assert!(!cheat_panel.style.ignores_mouse_events);

    let t_refresh = t_bg + Duration::from_millis(2000);
    store
        .select_and_activate_profile_at(&antigravity_id, t_refresh, true)
        .expect("switch profile while cheat sheet open");

    match store.hud_visibility() {
        HudVisibilityState::InteractiveCheatSheet { projection, .. } => {
            assert_eq!(projection.profile_name, "Antigravity");
            assert!(projection.target_agent_card.is_some());
            assert!(!projection.key_cells.is_empty());
        }
        other => panic!("expected InteractiveCheatSheet refreshed in-place, got {other:?}"),
    }
    assert!(panel_host
        .visible_panel(PassivePanelRole::CheatSheet)
        .is_some());
}

#[test]
fn status_menu_quick_switch_2x2_favorites_swap_and_icon_and_preset_bounds() {
    let (_dir, mut store, panel_host, status_item_host) = open_test_store();
    let now = Instant::now();
    store.update_screen_context(
        ScreenGeometry::single(ScreenRect::new(0.0, 0.0, 1440.0, 900.0)),
        ScreenPoint::new(720.0, 450.0),
        false,
        now,
    );

    let icon = StatusItemIconSpec::canonical();
    assert_eq!((icon.width_pt, icon.height_pt), (20.0, 18.0));
    assert!(icon.is_template);
    assert_eq!(icon.accessibility_description, "Micro Input 键盘");
    assert_eq!(icon.primitives.len(), 10);
    assert!(matches!(
        icon.primitives[0],
        StatusIconPrimitive::KeyboardBodyRoundedRect { .. }
    ));

    let antigravity_id = store
        .find_canonical_profile_id(CanonicalAgent::Antigravity)
        .expect("Antigravity");
    let deepseek_id = store
        .find_canonical_profile_id(CanonicalAgent::DeepseekHarness)
        .expect("Deepseek");
    let herdr_id = store
        .find_canonical_profile_id(CanonicalAgent::Herdr)
        .expect("Herdr");
    let codex_id = store
        .sidebar_rows()
        .iter()
        .find(|row| row.canonical_agent == Some(CanonicalAgent::Codex) && !row.is_protected_oai)
        .expect("editable Codex")
        .profile_id
        .clone();

    assert_eq!(
        store.favorites_grid().slot(FavoriteSlotIndex::SLOT_0),
        Some(antigravity_id.as_str())
    );
    assert_eq!(
        store.favorites_grid().slot(FavoriteSlotIndex::SLOT_1),
        Some(deepseek_id.as_str())
    );
    assert_eq!(
        store.favorites_grid().slot(FavoriteSlotIndex::SLOT_2),
        Some(herdr_id.as_str())
    );
    assert_eq!(
        store.favorites_grid().slot(FavoriteSlotIndex::SLOT_3),
        Some(codex_id.as_str())
    );

    store
        .dispatch_status_menu_action(StatusMenuAction::PinFavoriteSlot {
            slot: FavoriteSlotIndex::SLOT_0,
            profile_id: codex_id.clone(),
        })
        .expect("swap slot 0 and slot 3");

    assert_eq!(
        store.favorites_grid().slot(FavoriteSlotIndex::SLOT_0),
        Some(codex_id.as_str())
    );
    assert_eq!(
        store.favorites_grid().slot(FavoriteSlotIndex::SLOT_3),
        Some(antigravity_id.as_str())
    );

    store
        .dispatch_status_menu_action(StatusMenuAction::ActivateFavoriteSlot(
            FavoriteSlotIndex::SLOT_1,
        ))
        .expect("activate favorite slot 1");
    assert_eq!(store.selected_profile_id(), Some(deepseek_id.as_str()));
    assert!(panel_host
        .visible_panel(PassivePanelRole::PassiveNotice)
        .is_some());

    store
        .dispatch_status_menu_action(StatusMenuAction::SetHostExecutionEnabled(false))
        .expect("disable host execution");
    assert!(!store.projection().status_menu.host_execution_enabled);

    store
        .dispatch_status_menu_action(StatusMenuAction::SetAutoFollowEnabled(true))
        .expect("enable auto follow");
    assert!(store.projection().status_menu.auto_follow_enabled);

    store
        .dispatch_status_menu_action(StatusMenuAction::SelectProfile(antigravity_id.clone()))
        .expect("manual select profile sets manual override");
    assert!(store.projection().status_menu.manual_override);

    store
        .dispatch_status_menu_action(StatusMenuAction::ResumeAutoFollow)
        .expect("resume auto follow");
    assert!(!store.projection().status_menu.manual_override);

    store
        .dispatch_status_menu_action(StatusMenuAction::SetRadialMenuEnabled(true))
        .expect("enable radial menu from status bar");
    assert!(store.projection().status_menu.radial_menu_enabled);

    let snapshot = status_item_host
        .latest_snapshot()
        .expect("status item host synced");
    assert!(snapshot.radial_menu_enabled);
    assert!(snapshot.auto_follow_enabled);
    assert!(!snapshot.host_execution_enabled);

    let oversized = vec![b' '; MAX_PRESET_FILE_BYTES + 1];
    assert!(PresetFileEnvelope::from_bytes(&oversized).is_err());
    assert!(store
        .dispatch_status_menu_action(StatusMenuAction::StagePresetImportBytes(oversized))
        .is_err());
    assert!(store.projection().status_menu.preset_error.is_some());

    let exported = store
        .apply_backup_command(BackupCommand::ExportPreset {
            profile_id: Some(antigravity_id),
            layer_id: None,
        })
        .expect("export preset")
        .expect("exported json");
    store
        .dispatch_status_menu_action(StatusMenuAction::StagePresetImportBytes(
            exported.into_bytes(),
        ))
        .expect("stage valid preset bytes");
    assert!(store.projection().status_menu.pending_import.is_some());
    store
        .dispatch_status_menu_action(StatusMenuAction::ConfirmStagedPresetImport)
        .expect("confirm staged preset import");
    assert!(store.projection().status_menu.pending_import.is_none());

    store
        .dispatch_status_menu_action(StatusMenuAction::RequestPresetExport(
            PresetExportScope::Document,
        ))
        .expect("stage export");
    assert_eq!(
        store.projection().status_menu.pending_export_scope,
        Some(PresetExportScope::Document)
    );
    store
        .dispatch_status_menu_action(StatusMenuAction::ConfirmPresetExport)
        .expect("confirm export");
    assert!(store.last_exported_preset().is_some());
}

#[test]
fn popover_cheat_sheet_preview_tracks_active_profile_merges_wide_key_and_isolates_live_sessions() {
    assert_eq!(format_popover_cheat_label(""), "未绑定");
    assert_eq!(format_popover_cheat_label("   "), "未绑定");
    assert_eq!(format_popover_cheat_label("()"), "未绑定");
    assert_eq!(format_popover_cheat_label("(command k)"), "command k");
    assert_eq!(format_popover_cheat_label("（常用）运行测试"), "运行测试");
    assert_eq!(
        format_popover_cheat_label("命令 · (make test)"),
        "make test"
    );
    assert_eq!(format_popover_cheat_label("粘贴文本 · /review"), "/review");
    assert_eq!(
        format_popover_cheat_label("按住说话 · 松开停止"),
        "按住说话"
    );
    assert_eq!(
        format_popover_cheat_label("打开 Antigravity"),
        "Antigravity"
    );
    assert_eq!(
        format_popover_cheat_label("上一会话（侧栏轮转）"),
        "上一会话"
    );
    assert_eq!(
        format_popover_cheat_label("历史后退 (command [)"),
        "历史后退"
    );
    assert_eq!(
        format_popover_cheat_label("打开 DeepSeek Harness"),
        "DeepSeek Harness"
    );
    assert_eq!(
        format_popover_cheat_label("自定义超长自动化脚本动作名称测试"),
        "自定义超长自动…"
    );
    assert_eq!(format_popover_cheat_label("新建标签页"), "新标签页");
    assert_eq!(
        format_popover_cheat_label("内置浏览器 · 布局"),
        "浏览器/布局"
    );

    let dir = tempdir().expect("tempdir");
    let mut cfg = WorkbenchStoreConfig::isolated(dir.path());
    cfg.isolated = false;
    let mut store = WorkbenchStore::open(cfg.clone()).expect("open persistent temp store");
    store
        .set_host_execution_enabled(true)
        .expect("enable host execution");
    let antigravity_id = store
        .find_canonical_profile_id(CanonicalAgent::Antigravity)
        .expect("Antigravity");
    let herdr_id = store
        .find_canonical_profile_id(CanonicalAgent::Herdr)
        .expect("Herdr");
    store
        .select_and_activate_profile(&antigravity_id)
        .expect("activate Antigravity");

    assert_eq!(
        store.projection().status_menu.popover_preview_mode,
        PopoverPreviewMode::SessionPreview
    );
    assert!(store.projection().status_menu.cheat_sheet_preview.is_none());
    assert!(store.projection().status_menu.lighting_enabled);

    store
        .dispatch_status_menu_action(StatusMenuAction::SetPopoverPreviewMode(
            PopoverPreviewMode::CheatSheet,
        ))
        .expect("set cheat sheet preview mode");
    assert_eq!(
        store.projection().status_menu.popover_preview_mode,
        PopoverPreviewMode::CheatSheet
    );

    let split_sheet = store
        .projection()
        .status_menu
        .cheat_sheet_preview
        .clone()
        .expect("cheat sheet preview populated");
    assert_eq!(split_sheet.profile_name, "Antigravity");
    assert!(split_sheet.target_agent_card.is_none());
    assert_eq!(split_sheet.key_cells.len(), 20);
    let ag10_split = split_sheet
        .key_cells
        .iter()
        .find(|c| c.binding_id == "AG10")
        .expect("AG10 cell");
    let ag11_split = split_sheet
        .key_cells
        .iter()
        .find(|c| c.binding_id == "AG11")
        .expect("AG11 cell");
    assert_eq!(ag10_split.short_label, "项目新建");
    assert_eq!(ag11_split.short_label, "按住说话");

    store
        .dispatch_command(WorkbenchCommand::Sidebar(SidebarAction::BrowseProfile(
            herdr_id.clone(),
        )))
        .expect("browse Herdr in sidebar without activating");
    assert_eq!(store.selected_profile_id(), Some(herdr_id.as_str()));
    assert_eq!(
        store.projection().status_menu.cheat_sheet_preview.as_ref(),
        Some(&split_sheet),
        "browsing an inactive sidebar profile must never alter popover cheat_sheet_preview"
    );

    store
        .dispatch_command(WorkbenchCommand::Canvas(CanvasAction::SetBottomRowLayout(
            BottomRowLayout::Merged,
        )))
        .expect("switch to merged 2U bottom row");
    let merged_sheet = store
        .projection()
        .status_menu
        .cheat_sheet_preview
        .clone()
        .expect("merged cheat sheet preview");
    assert_eq!(merged_sheet.key_cells.len(), 19);
    assert!(merged_sheet
        .key_cells
        .iter()
        .all(|c| c.binding_id != "AG11"));
    let ag10_merged = merged_sheet
        .key_cells
        .iter()
        .find(|c| c.binding_id == "AG10")
        .expect("2U AG10 cell");
    assert_eq!(ag10_merged.short_label, "按住说话");

    let status_before_live = store.projection().status_menu.clone();
    store.ingest_four_app_candidates(vec![
        FourAppLiveCandidate {
            place: NavigationPlace::antigravity("agy-live-1", "workspace-a"),
            title: "Streaming Subagent Build".to_string(),
            phase: FourAppSessionKind::Working,
            watermark: "wm-live-1".to_string(),
            last_modified_ms: 5000,
            last_user_view_ms: 1000,
            updated_at_ms: 5000,
        },
        FourAppLiveCandidate {
            place: NavigationPlace::antigravity("agy-live-2", "workspace-b"),
            title: "Completed PR Review".to_string(),
            phase: FourAppSessionKind::BlueDotCompleted,
            watermark: "wm-live-2".to_string(),
            last_modified_ms: 6000,
            last_user_view_ms: 2000,
            updated_at_ms: 6000,
        },
    ]);
    assert_eq!(
        store.projection().status_menu,
        status_before_live,
        "live session candidate updates must leave StatusMenuProjection strictly unchanged in CheatSheet mode"
    );

    drop(store);
    let reopened = WorkbenchStore::open(cfg).expect("reopen persistent temp store");
    assert_eq!(
        reopened.projection().status_menu.popover_preview_mode,
        PopoverPreviewMode::CheatSheet
    );
    assert!(reopened
        .projection()
        .status_menu
        .cheat_sheet_preview
        .is_some());
}
