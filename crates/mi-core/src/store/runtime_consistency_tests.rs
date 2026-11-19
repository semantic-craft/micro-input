use super::*;
use std::sync::atomic::AtomicUsize;

fn input(session_id: u64, kind: InputEventKind) -> InputEvent {
    InputEvent {
        session_id,
        board_layer: None,
        sequence: 1,
        ingress_sequence: 1,
        radial_center: None,
        radial_position: None,
        timestamp_ms: 0,
        kind,
        logical_switch: None,
        binding_id: Some("AG11".into()),
        physical_control: None,
        keycap_group: None,
        action_role: None,
        lost_count: 0,
        action_eligible: true,
    }
}

fn active_store() -> (tempfile::TempDir, WorkbenchStore, Instant) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = WorkbenchStore::open_isolated(dir.path()).unwrap();
    let now = Instant::now() + Duration::from_secs(3600);
    let profile = store
        .find_canonical_profile_id(CanonicalAgent::Antigravity)
        .unwrap();
    store
        .select_and_activate_profile_at(&profile, now, false)
        .unwrap();
    store.set_host_execution_enabled(true).unwrap();
    (dir, store, now)
}

#[derive(Default)]
struct PanelSpy {
    calls: AtomicUsize,
}
impl PassivePanelHost for PanelSpy {
    fn present_panel(&self, _: PassivePanelFrameSpec) {
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
    fn hide_panel(&self, _: PassivePanelRole) {
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
    fn visible_panel(&self, _: PassivePanelRole) -> Option<PassivePanelFrameSpec> {
        None
    }
    fn events(&self) -> Vec<mi_macos::PassivePanelEvent> {
        Vec::new()
    }
}
#[derive(Default)]
struct StatusSpy {
    calls: AtomicUsize,
}
impl StatusItemHost for StatusSpy {
    fn sync_status_item(&self, _: StatusItemSnapshot) {
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
    fn latest_snapshot(&self) -> Option<StatusItemSnapshot> {
        None
    }
    fn snapshots(&self) -> Vec<StatusItemSnapshot> {
        Vec::new()
    }
}

#[test]
fn projector_reads_only_captured_facts_and_never_presents_or_normalizes() {
    let (_dir, mut store, now) = active_store();
    let panels = Arc::new(PanelSpy::default());
    let status = Arc::new(StatusSpy::default());
    store.set_hosts(panels.clone(), status.clone());
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    store.app_cache = Arc::new(AppBundleCache::with_resolver(Arc::new(move |id| {
        observed.fetch_add(1, Ordering::SeqCst);
        AppBundleInfo {
            bundle_id: id.into(),
            installed: false,
            app_path: None,
            display_name: None,
            icon_cached: false,
        }
    })));
    let before = store.projection().clone();
    let generation = store.projection_generation();
    let panel_calls = panels.calls.load(Ordering::SeqCst);
    let status_calls = status.calls.load(Ordering::SeqCst);
    store.update_now = now + Duration::from_secs(10000);
    for _ in 0..5 {
        assert_eq!(store.projection_inputs().project(), before);
        assert_eq!(store.projection(), &before);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(panels.calls.load(Ordering::SeqCst), panel_calls);
    assert_eq!(status.calls.load(Ordering::SeqCst), status_calls);
    assert_eq!(store.projection_generation(), generation);
    let herdr = store
        .find_canonical_profile_id(CanonicalAgent::Herdr)
        .unwrap();
    store.browse_profile(&herdr).unwrap();
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "browsing uses captured application facts"
    );
    assert_eq!(panels.calls.load(Ordering::SeqCst), panel_calls);
    assert_eq!(status.calls.load(Ordering::SeqCst), status_calls);
}

#[test]
fn presentation_is_explicit_deduplicated_and_notice_time_is_consumption_time() {
    let (_dir, mut store, now) = active_store();
    let panels = Arc::new(PanelSpy::default());
    let status = Arc::new(StatusSpy::default());
    store.set_hosts(panels.clone(), status.clone());
    let profile = store.active_host_profile_id().unwrap();
    store
        .select_and_activate_profile_at(&profile, now, true)
        .unwrap();
    assert!(store.projection().overlay.panel_spec.is_some());
    let calls = panels.calls.load(Ordering::SeqCst);
    let menus = status.calls.load(Ordering::SeqCst);
    store.poll_at(now + Duration::from_millis(100));
    assert_eq!(panels.calls.load(Ordering::SeqCst), calls);
    assert_eq!(status.calls.load(Ordering::SeqCst), menus);
    let deadline = now + crate::PASSIVE_NOTICE_DURATION;
    store.poll_at(deadline - Duration::from_nanos(1));
    assert!(store.projection().overlay.panel_spec.is_some());
    store.poll_at(deadline);
    assert!(store.projection().overlay.panel_spec.is_none());
    assert_eq!(panels.calls.load(Ordering::SeqCst), calls + 2);
}

#[test]
fn dictation_release_keeps_original_session_profile_and_adapter_after_deletion() {
    let (_dir, mut store, now) = active_store();
    let original = store
        .consume_physical_input_event(input(4, InputEventKind::Pressed), now)
        .unwrap();
    let replacement = store
        .find_canonical_profile_id(CanonicalAgent::DeepseekHarness)
        .unwrap();
    store
        .select_and_activate_profile_at(&replacement, now, false)
        .unwrap();
    store
        .document_store
        .draft
        .document
        .editable
        .graph
        .profiles
        .remove(&original.profile_id);
    assert!(store
        .consume_physical_input_event(input(5, InputEventKind::Released), now)
        .is_none());
    assert!(store.ptt_controller.is_active);
    let released = store
        .consume_physical_input_event(input(4, InputEventKind::Released), now)
        .unwrap();
    assert_eq!(released.profile_id, original.profile_id);
    assert_eq!(released.target, original.target);
    assert!(!store.ptt_controller.is_active);
    assert!(store
        .consume_physical_input_event(input(4, InputEventKind::Released), now)
        .is_none());
}

#[test]
fn uncertain_dictation_cleanup_retains_origin_and_blocks_new_press_until_retry() {
    let (_dir, mut store, now) = active_store();
    let original = store
        .consume_physical_input_event(input(4, InputEventKind::Pressed), now)
        .unwrap();
    let failed = store
        .finish_dictation(Err("original window stop unconfirmed".into()))
        .unwrap();
    assert_eq!(failed.outcome, "unconfirmed");
    assert_eq!(failed.profile_id, original.profile_id);
    assert!(store.held_dictation.is_some());
    assert!(!store.ptt_controller.cleanup_confirmed);
    assert!(store
        .consume_physical_input_event(input(5, InputEventKind::Pressed), now)
        .is_none());
    let released = store
        .consume_physical_input_event(input(4, InputEventKind::Released), now)
        .unwrap();
    assert_eq!(released.profile_id, original.profile_id);
    assert!(store.held_dictation.is_none());
    assert!(store.ptt_controller.cleanup_confirmed);
    assert!(store
        .consume_physical_input_event(input(5, InputEventKind::Pressed), now)
        .is_some());
}

#[test]
fn legacy_and_managed_gaps_cancel_without_reconstructing_releases_from_current_profile() {
    for routing in [BoardRouting::Legacy, BoardRouting::Managed { layer: 1 }] {
        let (_dir, mut store, now) = active_store();
        let original = store
            .consume_physical_input_event(input(4, InputEventKind::Pressed), now)
            .unwrap();
        store.board_routing = routing;
        let replacement = store
            .find_canonical_profile_id(CanonicalAgent::DeepseekHarness)
            .unwrap();
        store
            .select_and_activate_profile_at(&replacement, now, false)
            .unwrap();
        store.route_board_input(input(4, InputEventKind::InputStreamGap), now);
        assert!(!store.ptt_controller.is_active);
        assert!(store.held_dictation.is_none());
        assert_eq!(
            store.last_host_action_receipt.as_ref().unwrap().profile_id,
            original.profile_id
        );
        assert!(store
            .consume_physical_input_event(input(4, InputEventKind::Pressed), now)
            .is_none());
        store.poll_at(now + Duration::from_secs(10));
        assert!(
            store
                .consume_physical_input_event(input(4, InputEventKind::Pressed), now)
                .is_none(),
            "polling must not revive a lost stream"
        );
    }
}

#[test]
fn disabling_execution_cancels_original_dictation() {
    let (_dir, mut store, now) = active_store();
    store
        .consume_physical_input_event(input(4, InputEventKind::Pressed), now)
        .unwrap();
    store.set_host_execution_enabled(false).unwrap();
    assert!(store.held_dictation.is_none());
    assert!(!store.ptt_controller.is_active);
    assert!(store.ptt_controller.cleanup_confirmed);
}

#[test]
fn identical_consumption_times_replay_commands_ownership_and_overlay_expiry() {
    let dir = tempfile::tempdir().unwrap();
    let now = Instant::now() - Duration::from_secs(60);
    let first = WorkbenchStore::open_isolated(dir.path()).unwrap();
    let profile = first
        .find_canonical_profile_id(CanonicalAgent::Antigravity)
        .unwrap();
    let replacement = first
        .find_canonical_profile_id(CanonicalAgent::DeepseekHarness)
        .unwrap();
    fs::write(
        dir.path().join("workbench.json"),
        serde_json::to_vec(&first.document_store.draft.document).unwrap(),
    )
    .unwrap();
    drop(first);
    let second_dir = tempfile::tempdir().unwrap();
    for name in ["workbench.json", "workbench.preferences.json"] {
        if dir.path().join(name).exists() {
            fs::copy(dir.path().join(name), second_dir.path().join(name)).unwrap();
        }
    }
    let mut first = WorkbenchStore::open_isolated(dir.path()).unwrap();
    let mut second = WorkbenchStore::open_isolated(second_dir.path()).unwrap();
    let replay = |store: &mut WorkbenchStore| {
        store
            .dispatch_at(
                WorkbenchCommand::Sidebar(SidebarAction::SelectProfile(profile.clone())),
                now,
            )
            .unwrap();
        store.set_host_execution_enabled(true).unwrap();
        store
            .consume_physical_input_event(
                input(88, InputEventKind::Pressed),
                now + Duration::from_millis(10),
            )
            .unwrap();
        store
            .dispatch_at(
                WorkbenchCommand::Sidebar(SidebarAction::SelectProfile(replacement.clone())),
                now + Duration::from_millis(20),
            )
            .unwrap();
        let release = store
            .consume_physical_input_event(
                input(88, InputEventKind::Released),
                now + Duration::from_millis(30),
            )
            .unwrap();
        store.poll_at(now + Duration::from_secs(5));
        (store.projection().clone(), release)
    };
    let (first_projection, first_release) = replay(&mut first);
    let (second_projection, second_release) = replay(&mut second);
    assert_eq!(first_projection, second_projection);
    assert_eq!(first_release, second_release);
    assert_eq!(first_release.profile_id, profile);
    assert!(first_projection.overlay.panel_spec.is_none());
}

#[test]
fn loss_latched_after_batch_prefetch_blocks_remaining_presses_before_next_poll() {
    let (_dir, mut store, now) = active_store();
    store.hardware_event_tx.open_source(4);
    store
        .hardware_event_tx
        .send(HardwareLoopEvent::Input(input(4, InputEventKind::Pressed)))
        .unwrap();
    let prefetched = store.hardware_event_tx.take_batch();
    for _ in 0..=runtime_inbox::INPUT_CAPACITY {
        store
            .hardware_event_tx
            .send(HardwareLoopEvent::Input(input(4, InputEventKind::Pressed)))
            .unwrap();
    }
    let HardwareLoopEvent::Input(event) = prefetched.into_iter().next().unwrap() else {
        panic!("expected input");
    };
    assert!(store.consume_physical_input_event(event, now).is_none());
    assert!(!store.ptt_controller.is_active);
    assert!(store.held_dictation.is_none());
}

#[test]
fn radial_host_failure_is_preserved_and_does_not_claim_success() {
    let (_dir, mut store, now) = active_store();
    store.set_radial_menu_enabled(true);
    store.consume_radial_input(
        RadialInputEvent::Position {
            session_id: 7,
            ingress_seq: 1,
            sample: RadialPositionSample::new(0.0, 0.1),
            admitted: true,
        },
        now,
    );
    store.consume_radial_input(
        RadialInputEvent::Position {
            session_id: 7,
            ingress_seq: 2,
            sample: RadialPositionSample::new(0.0, 0.9),
            admitted: true,
        },
        now + Duration::from_millis(20),
    );
    let session = store.radial_state.session().unwrap().clone();
    let expected = HostActionReceipt {
        action_type: "open_app".into(),
        binding_id: "radial-menu".into(),
        profile_id: session.route.profile_id.clone().unwrap(),
        target: "test-app".into(),
        detail: "apple_fn_cleanup_unconfirmed".into(),
        outcome: "blocked".into(),
        text_byte_len: None,
        timestamp_ms: 0,
    };
    let receipt = store.execute_radial_intent_using(&session, &RadialIntent::OpenApp { bundle_id: "test-app".into() }, |profile, action| {
        assert_eq!(profile, expected.profile_id);
        assert!(matches!(action, ResolvedHostAction::OpenApp { bundle_id } if bundle_id == "test-app"));
        expected.clone()
    });
    assert_eq!(receipt, expected);
    assert_eq!(store.last_host_action_receipt(), Some(&expected));
    assert!(!receipt.succeeded());
}

#[test]
fn global_cleanup_without_a_core_ptt_owner_recovers_admission_after_confirmed_retry() {
    let (_dir, mut store, now) = active_store();
    assert!(store.held_dictation.is_none());
    assert!(store
        .finish_host_cleanup(Err("six-layer original window stop unconfirmed".into()))
        .is_err());
    assert!(!store.ptt_controller.cleanup_confirmed);
    assert!(store
        .consume_physical_input_event(input(7, InputEventKind::Pressed), now)
        .is_none());
    store.set_host_execution_enabled(false).unwrap();
    assert!(store.ptt_controller.cleanup_confirmed);
    store.set_host_execution_enabled(true).unwrap();
    assert!(store
        .consume_physical_input_event(input(7, InputEventKind::Pressed), now)
        .is_some());
}

#[test]
fn native_codex_cheat_sheets_show_local_labels_and_preserve_empty_controls() {
    let (_dir, mut store, now) = active_store();
    store.bottom_row_layout = BottomRowLayout::Split;
    let native = store
        .find_canonical_profile_id(CanonicalAgent::Codex)
        .unwrap();
    let local = store.local_codex_profile_id().unwrap();
    let graph = &mut store.document_store.draft.document.editable.graph;
    let layer_id = graph.profiles[&local].layer_order[0].clone();
    if let Some(cs) = graph
        .layers
        .get_mut(&layer_id)
        .unwrap()
        .control_settings
        .as_mut()
    {
        cs.joystick_mode = "logical_sectors".into();
        cs.active_sectors = 4;
    }
    let mut expected = BTreeMap::new();
    for binding in graph
        .layers
        .get_mut(&layer_id)
        .unwrap()
        .controls
        .values_mut()
    {
        let slot: usize = binding
            .logical_control
            .strip_prefix("logical-")
            .unwrap()
            .parse()
            .unwrap();
        if [0, 12].contains(&slot) {
            binding.action_id = None;
        }
        let label = if let Some(id) = binding.action_id.as_ref() {
            let action = graph.actions.get_mut(id).unwrap();
            action.name = format!("custom-native-control-{slot}");
            if slot == 15 {
                format!("{} · 长按搜索会话", action.name)
            } else {
                action.name.clone()
            }
        } else {
            "未绑定".to_string()
        };
        expected.insert(slot, (label, binding.action_id.is_some()));
    }
    store
        .select_and_activate_profile_at(&native, now, false)
        .unwrap();
    store
        .dispatch_sidebar_action(SidebarAction::SetProfileLocked(true))
        .unwrap();
    let before = serde_json::to_value(&store.document_store.draft.document).unwrap();
    let menu = &store.projection().status_menu;
    assert_eq!(menu.active_profile_id.as_deref(), Some(native.as_str()));
    let sheet = menu.cheat_sheet_preview.clone().unwrap();
    assert_eq!(sheet.profile_name, "Codex");
    assert_eq!(sheet.key_cells.len(), 20);
    for (slot, binding) in mi_model::direct_controls::CONTROL_BINDINGS
        .iter()
        .enumerate()
    {
        let cell = sheet
            .key_cells
            .iter()
            .find(|cell| cell.binding_id == *binding)
            .unwrap();
        assert_eq!(
            (&cell.label, cell.available),
            (&expected[&slot].0, expected[&slot].1),
            "{binding}"
        );
    }
    assert_eq!(
        store
            .projection_inputs()
            .build_cheat_sheet_projection()
            .key_cells,
        sheet.key_cells
    );
    let other = store
        .find_canonical_profile_id(CanonicalAgent::Herdr)
        .unwrap();
    store.browse_profile(&other).unwrap();
    assert_eq!(
        store.projection().status_menu.cheat_sheet_preview.as_ref(),
        Some(&sheet)
    );
    store.bottom_row_layout = BottomRowLayout::Merged;
    store.commit_runtime_update();
    let merged = store
        .projection()
        .status_menu
        .cheat_sheet_preview
        .as_ref()
        .unwrap();
    assert_eq!(merged.key_cells.len(), 19);
    assert!(!merged
        .key_cells
        .iter()
        .any(|cell| cell.binding_id == "AG11"));
    assert_eq!(
        merged
            .key_cells
            .iter()
            .find(|cell| cell.binding_id == "AG10")
            .unwrap()
            .label,
        "custom-native-control-10"
    );
    assert_eq!(
        serde_json::to_value(&store.document_store.draft.document).unwrap(),
        before
    );

    store
        .document_store
        .draft
        .document
        .editable
        .graph
        .profiles
        .get_mut(&native)
        .unwrap()
        .name = "Other protected owner".into();
    store.commit_runtime_update();
    assert!(store
        .projection()
        .status_menu
        .cheat_sheet_preview
        .as_ref()
        .unwrap()
        .key_cells
        .iter()
        .all(|cell| !cell.available));
    let graph = &mut store.document_store.draft.document.editable.graph;
    graph.profiles.get_mut(&native).unwrap().name = "Codex".into();
    graph.profiles.remove(&local);
    graph.profile_order.retain(|id| id != &local);
    store.commit_runtime_update();
    assert!(store
        .projection()
        .status_menu
        .cheat_sheet_preview
        .as_ref()
        .unwrap()
        .key_cells
        .iter()
        .all(|cell| !cell.available));
}

#[test]
fn native_codex_all_controls_keep_existing_actions_in_locked_and_manual_routes() {
    for locked in [false, true] {
        let (_dir, mut store, now) = active_store();
        store.bottom_row_layout = BottomRowLayout::Split;
        let native = store
            .find_canonical_profile_id(CanonicalAgent::Codex)
            .unwrap();
        store
            .select_and_activate_profile_at(&native, now, false)
            .unwrap();
        if locked {
            store
                .dispatch_sidebar_action(SidebarAction::SetProfileLocked(true))
                .unwrap();
        }
        let before = serde_json::to_value(&store.document_store.draft.document).unwrap();
        let plan = match store.build_six_layer_routing_mode() {
            crate::six_layer_runtime::SixLayerRoutingMode::Locked(plan) if locked => plan,
            crate::six_layer_runtime::SixLayerRoutingMode::ManualPinned(plan) if !locked => plan,
            _ => panic!("expected pinned route"),
        };
        assert_eq!(plan.profile_id, native);
        for (slot, command) in [
            (6, "live-voice"),
            (7, "focus-input"),
            (8, "new-tab"),
            (9, "cycle-workspace-layout"),
            (10, "new-chat"),
            (11, "toggle-dictation"),
            (12, "submit"),
        ] {
            assert_eq!(
                plan.actions[slot],
                Some(ResolvedHostAction::AgentCommand {
                    adapter: "codex".into(),
                    command: command.into(),
                }),
                "slot {slot}"
            );
            for prefix in ["AG", "ACT"] {
                let mut event = input(9, InputEventKind::Pressed);
                event.binding_id = Some(format!("{prefix}{slot:02}"));
                let receipt = store
                    .route_board_input(event, now)
                    .expect("existing lower-row action");
                assert_eq!(receipt.profile_id, native);
                assert_eq!(receipt.detail, format!("codex/{command}"));
            }
        }
        let local = store.local_codex_profile_id().unwrap();
        let local_plan = store.compile_profile_twenty_slots(&local).unwrap();
        assert_eq!(plan.actions, local_plan.actions);
        assert_eq!(
            plan.actions[0],
            Some(ResolvedHostAction::AgentCommand {
                adapter: "codex".into(),
                command: "subagent".into(),
            })
        );
        assert_eq!(
            plan.actions[1],
            Some(ResolvedHostAction::AgentCommand {
                adapter: "host".into(),
                command: "mouse-click:left".into(),
            })
        );
        for action in &plan.actions[2..6] {
            assert_eq!(*action, None);
        }
        assert_eq!(
            store.desired_live_session_source(),
            LiveSessionSource::Codex
        );
        assert_eq!(
            serde_json::to_value(&store.document_store.draft.document).unwrap(),
            before
        );
        store.set_host_execution_enabled(false).unwrap();
        assert!(store
            .route_board_input(input(9, InputEventKind::Pressed), now)
            .is_none());
    }
}

#[test]
fn native_codex_preserves_custom_actions_unbound_slots_and_other_owners() {
    let (_dir, mut store, now) = active_store();
    let native = store
        .find_canonical_profile_id(CanonicalAgent::Codex)
        .unwrap();
    let local = store
        .document_store
        .draft
        .document
        .editable
        .graph
        .profiles
        .iter()
        .find(|(_, p)| p.name == mi_model::preset::LOCAL_CODEX_PROFILE_NAME)
        .map(|(id, p)| (id.clone(), p.layer_order[0].clone()))
        .unwrap();
    let graph = &mut store.document_store.draft.document.editable.graph;
    let layer = graph.layers.get_mut(&local.1).unwrap();
    let custom_id = layer
        .controls
        .values()
        .find(|c| c.logical_control == "logical-8")
        .unwrap()
        .action_id
        .clone()
        .unwrap();
    for slot in [0, 12] {
        layer
            .controls
            .values_mut()
            .find(|c| c.logical_control == format!("logical-{slot}"))
            .unwrap()
            .action_id = None;
    }
    graph.actions.get_mut(&custom_id).unwrap().payload = serde_json::json!({
        "type": "open_url", "url": "https://example.com/custom-action"
    });
    store
        .select_and_activate_profile_at(&native, now, false)
        .unwrap();
    let before = serde_json::to_value(&store.document_store.draft.document).unwrap();
    let plan = store.compile_profile_twenty_slots(&native).unwrap();
    assert_eq!(
        plan.actions[8],
        Some(ResolvedHostAction::OpenUrl {
            url: "https://example.com/custom-action".into(),
        })
    );
    assert!(plan.actions[0].is_none());
    assert!(plan.actions[12].is_none());
    for (binding, expected) in [
        ("ACT08", true),
        ("AG00", false),
        ("AG12", false),
        ("AG13", true),
    ] {
        let mut event = input(9, InputEventKind::Pressed);
        event.binding_id = Some(binding.into());
        assert_eq!(
            store.route_board_input(event, now).is_some(),
            expected,
            "{binding}"
        );
    }
    assert_eq!(
        serde_json::to_value(&store.document_store.draft.document).unwrap(),
        before
    );
    store
        .document_store
        .draft
        .document
        .editable
        .graph
        .profiles
        .get_mut(&native)
        .unwrap()
        .name = "Other protected owner".into();
    assert!(store
        .compile_profile_twenty_slots(&native)
        .unwrap()
        .actions
        .iter()
        .all(Option::is_none));
    let mut event = input(9, InputEventKind::Pressed);
    event.binding_id = Some("ACT08".into());
    assert!(store.route_board_input(event, now).is_none());
}

#[test]
fn native_codex_all_controls_dispatch_custom_local_actions_without_rewriting_mappings() {
    for locked in [false, true] {
        let (_dir, mut store, now) = active_store();
        store.bottom_row_layout = BottomRowLayout::Split;
        let native = store
            .find_canonical_profile_id(CanonicalAgent::Codex)
            .unwrap();
        let local = store.local_codex_profile_id().unwrap();
        let graph = &mut store.document_store.draft.document.editable.graph;
        let layer_id = graph.profiles[&local].layer_order[0].clone();
        if let Some(cs) = graph
            .layers
            .get_mut(&layer_id)
            .unwrap()
            .control_settings
            .as_mut()
        {
            cs.joystick_mode = "logical_sectors".into();
            cs.active_sectors = 4;
        }
        let template = graph.actions.values().next().unwrap().clone();
        for binding in graph
            .layers
            .get_mut(&layer_id)
            .unwrap()
            .controls
            .values_mut()
        {
            let slot = binding.logical_control.strip_prefix("logical-").unwrap();
            let id = format!("custom-control-{slot}");
            let mut action = template.clone();
            action.payload = serde_json::json!({
                "type": "open_url", "url": format!("https://example.com/control/{slot}")
            });
            graph.actions.insert(id.clone(), action);
            binding.action_id = Some(id);
        }
        store
            .select_and_activate_profile_at(&native, now, false)
            .unwrap();
        if locked {
            store
                .dispatch_sidebar_action(SidebarAction::SetProfileLocked(true))
                .unwrap();
        }
        let before = serde_json::to_value(&store.document_store.draft.document).unwrap();
        let plan = store.compile_profile_twenty_slots(&native).unwrap();
        for (slot, binding) in mi_model::direct_controls::CONTROL_BINDINGS
            .iter()
            .enumerate()
        {
            let url = format!("https://example.com/control/{slot}");
            assert_eq!(
                plan.actions[slot],
                Some(ResolvedHostAction::OpenUrl { url: url.clone() })
            );
            let mut event = input(9, InputEventKind::Pressed);
            event.binding_id = Some((*binding).into());
            let receipt = if *binding == "ENC_CLK" {
                assert!(store.route_board_input(event, now).is_none());
                let mut release = input(9, InputEventKind::Released);
                release.binding_id = Some("ENC_CLK".into());
                store
                    .route_board_input(release, now + Duration::from_millis(50))
                    .expect("local custom action on ENC_CLK short tap")
            } else {
                store
                    .route_board_input(event, now)
                    .expect("local custom action")
            };
            assert_eq!(receipt.profile_id, native);
            assert_eq!(receipt.target, "default-browser");
            assert_eq!(receipt.detail, "example.com");
            assert_eq!(receipt.text_byte_len, Some(url.len()));
        }
        assert_eq!(
            serde_json::to_value(&store.document_store.draft.document).unwrap(),
            before
        );
    }
}

#[test]
fn native_codex_accepts_radial_positions_without_a_binding_and_unknown_owners_stay_closed() {
    let (_dir, mut store, now) = active_store();
    let native = store
        .find_canonical_profile_id(CanonicalAgent::Codex)
        .unwrap();
    store
        .select_and_activate_profile_at(&native, now, false)
        .unwrap();
    for recognized in [true, false] {
        if !recognized {
            store
                .document_store
                .draft
                .document
                .editable
                .graph
                .profiles
                .get_mut(&native)
                .unwrap()
                .name = "Other protected owner".into();
        }
        store.set_radial_menu_enabled(false);
        store.set_radial_menu_enabled(true);
        for (sequence, distance) in [(1, 0.0), (2, 0.8)] {
            let mut event = input(9, InputEventKind::UnknownNotification);
            event.binding_id = None;
            event.ingress_sequence = sequence;
            event.radial_position = Some(mi_session::RadialPosition {
                angle_turns: 0.0,
                distance,
            });
            assert!(store.route_board_input(event, now).is_none());
        }
        assert_eq!(store.radial_state().session().is_some(), recognized);
    }
}

#[test]
fn host_mouse_commands_preserve_host_adapter_in_compile_profile_twenty_slots() {
    for target_agent in [CanonicalAgent::Antigravity, CanonicalAgent::Codex] {
        let dir = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(dir.path()).unwrap();
        let target_pid = store.find_canonical_profile_id(target_agent).unwrap();
        let edit_pid = if target_agent == CanonicalAgent::Codex {
            store.local_codex_profile_id().unwrap()
        } else {
            target_pid.clone()
        };
        store.select_and_activate_profile(&edit_pid).unwrap();
        store
            .dispatch_canvas_action(CanvasAction::ApplyKnobJoystickPreset(
                KnobJoystickPreset::MousePointerAndScroll,
            ))
            .unwrap();

        let compiled = store.compile_profile_twenty_slots(&target_pid).unwrap();
        for (slot, action) in compiled.actions.iter().enumerate().skip(13) {
            match action {
                Some(mi_model::host_router::ResolvedHostAction::AgentCommand {
                    adapter,
                    command,
                }) => {
                    assert_eq!(
                        adapter, "host",
                        "{target_agent:?} slot {slot} ({command}) must preserve adapter == host"
                    );
                    assert!(mi_model::is_host_mouse_command(command));
                }
                other => panic!(
                    "{target_agent:?} slot {slot} expected host mouse AgentCommand, got {other:?}"
                ),
            }
        }
    }
}

#[test]
fn idle_poll_at_skips_commit_local_update_and_does_not_bump_projection_generation() {
    let (_dir, mut store, now) = active_store();
    let initial_gen = store.projection_generation();
    let initial_commits = store.commit_local_update_count;
    assert!(initial_gen > 0);
    assert!(initial_commits > 0);

    for offset in [
        Duration::from_millis(16),
        Duration::from_secs(1),
        Duration::from_secs(5),
        Duration::from_secs(30),
    ] {
        assert!(!store.poll_at(now + offset));
        assert_eq!(store.projection_generation(), initial_gen);
        assert_eq!(store.commit_local_update_count, initial_commits);
    }
}

#[test]
fn poll_at_detects_app_cache_invalidate_and_overlay_expiry() {
    let dir = tempfile::tempdir().unwrap();
    let installed_flag = Arc::new(AtomicBool::new(false));
    let flag_reader = Arc::clone(&installed_flag);
    let mut cfg = WorkbenchStoreConfig::isolated(dir.path());
    cfg.app_cache = Arc::new(AppBundleCache::with_resolver(Arc::new(move |id| {
        let installed = id == "com.google.antigravity" && flag_reader.load(Ordering::Acquire);
        AppBundleInfo {
            bundle_id: id.to_string(),
            installed,
            app_path: None,
            display_name: None,
            icon_cached: false,
        }
    })));
    let mut store = WorkbenchStore::open(cfg).unwrap();
    let t0 = Instant::now() + Duration::from_secs(3600);
    let profile = store
        .find_canonical_profile_id(CanonicalAgent::Antigravity)
        .unwrap();

    assert!(
        !store
            .projection()
            .sidebar
            .rows
            .iter()
            .find(|r| r.profile_id == profile)
            .unwrap()
            .app_installed
    );
    assert!(!store.poll_at(t0));

    installed_flag.store(true, Ordering::Release);
    assert!(!store.poll_at(t0 + Duration::from_millis(50)));

    store.app_cache.invalidate("com.google.antigravity");
    let before_gen = store.projection_generation();
    assert!(store.poll_at(t0 + Duration::from_millis(100)));
    assert_eq!(store.projection_generation(), before_gen + 1);
    assert!(
        store
            .projection()
            .sidebar
            .rows
            .iter()
            .find(|r| r.profile_id == profile)
            .unwrap()
            .app_installed
    );

    let settled_commits = store.commit_local_update_count;
    assert!(!store.poll_at(t0 + Duration::from_millis(150)));
    assert_eq!(store.commit_local_update_count, settled_commits);

    let t1 = t0 + Duration::from_secs(1);
    store
        .select_and_activate_profile_at(&profile, t1, true)
        .unwrap();
    assert!(store.projection().overlay.panel_spec.is_some());

    let before_expiry_commits = store.commit_local_update_count;
    assert!(!store.poll_at(t1 + crate::PASSIVE_NOTICE_DURATION - Duration::from_millis(1)));
    assert_eq!(store.commit_local_update_count, before_expiry_commits);

    let gen_before_expiry = store.projection_generation();
    assert!(store.poll_at(t1 + crate::PASSIVE_NOTICE_DURATION));
    assert_eq!(store.projection_generation(), gen_before_expiry + 1);
    assert!(store.projection().overlay.panel_spec.is_none());

    let commits_after_expiry = store.commit_local_update_count;
    assert!(!store.poll_at(t1 + crate::PASSIVE_NOTICE_DURATION + Duration::from_millis(50)));
    assert_eq!(store.commit_local_update_count, commits_after_expiry);
}

#[test]
fn poll_at_triggers_encoder_model_picker_hold_and_idle_expiry() {
    let (_dir, mut store, now) = active_store();
    let press = InputEvent {
        binding_id: Some("ENC_CLK".into()),
        ..input(11, InputEventKind::Pressed)
    };
    assert!(store.consume_physical_input_event(press, now).is_none());

    let pre_hold_commits = store.commit_local_update_count;
    assert!(!store.poll_at(now + Duration::from_millis(100)));
    assert_eq!(store.commit_local_update_count, pre_hold_commits);

    let gen_before_hold = store.projection_generation();
    assert!(store.poll_at(now + crate::ENCODER_HOLD_THRESHOLD));
    assert_eq!(store.commit_local_update_count, pre_hold_commits + 1);
    assert!(store.projection_generation() > gen_before_hold);
    assert!(store.projection().status_message.contains("搜索会话"));

    let release = InputEvent {
        binding_id: Some("ENC_CLK".into()),
        ..input(11, InputEventKind::Released)
    };
    assert!(store
        .consume_physical_input_event(release, now + Duration::from_millis(400))
        .is_none());

    let confirm_press = InputEvent {
        binding_id: Some("ENC_CLK".into()),
        ..input(11, InputEventKind::Pressed)
    };
    assert!(store
        .consume_physical_input_event(confirm_press, now + Duration::from_millis(420))
        .is_none());
    let confirm_release = InputEvent {
        binding_id: Some("ENC_CLK".into()),
        ..input(11, InputEventKind::Released)
    };
    assert!(store
        .consume_physical_input_event(confirm_release, now + Duration::from_millis(440))
        .is_some());
    assert!(store.encoder_model_picker.active_session().is_none());

    let t_turn = now + Duration::from_millis(480);
    let turn = InputEvent {
        binding_id: Some("AG14".into()),
        ..input(11, InputEventKind::Pressed)
    };
    assert!(store.consume_physical_input_event(turn, t_turn).is_some());
    assert!(!store.is_model_picker_mouse_capture_enabled());
    assert!(store.encoder_model_picker.active_session().is_some());

    let mid_commits = store.commit_local_update_count;
    assert!(!store.poll_at(t_turn + Duration::from_secs(5)));
    assert_eq!(store.commit_local_update_count, mid_commits);
    assert!(!store.is_model_picker_mouse_capture_enabled());

    let _ = store.poll_at(t_turn + crate::MODEL_PICKER_IDLE_TIMEOUT + Duration::from_millis(1));
    assert_eq!(store.commit_local_update_count, mid_commits + 1);
    assert!(!store.is_model_picker_mouse_capture_enabled());
    assert!(store.encoder_model_picker.active_session().is_none());

    let post_expiry_commits = store.commit_local_update_count;
    assert!(!store.poll_at(t_turn + crate::MODEL_PICKER_IDLE_TIMEOUT + Duration::from_secs(1)));
    assert_eq!(store.commit_local_update_count, post_expiry_commits);
}

#[test]
fn poll_at_detects_session_navigation_and_slot_table_mutations() {
    let (_dir, mut store, now) = active_store();
    let cand = FourAppLiveCandidate {
        place: NavigationPlace::antigravity("conv-poll-1", "/tmp/ws"),
        title: "Poll Session 1".into(),
        phase: FourAppSessionKind::BlueDotCompleted,
        watermark: "wm-1".into(),
        last_modified_ms: 5000,
        last_user_view_ms: 1000,
        updated_at_ms: 5000,
    };

    let gen_before_slot = store.projection_generation();
    let pre_slot_commits = store.commit_local_update_count;
    store
        .slot_table
        .update_candidates(vec![cand.clone()], &BTreeMap::new());
    assert!(store.poll_at(now + Duration::from_millis(20)));
    assert_eq!(store.commit_local_update_count, pre_slot_commits + 1);
    assert!(store.projection_generation() > gen_before_slot);
    assert_eq!(
        store.projection().session_strip.slots[0].title,
        "Poll Session 1"
    );

    let idle_commits = store.commit_local_update_count;
    assert!(!store.poll_at(now + Duration::from_millis(40)));
    assert_eq!(store.commit_local_update_count, idle_commits);

    store.session_navigation.catalog = Some(NavigationCatalog {
        instance: CatalogInstance::Antigravity(AntigravityInstance {
            hub: "hub-poll".into(),
            pid: 1,
            ui_port: 12,
        }),
        observed_at: Instant::now(),
        candidates: vec![cand.clone()],
        project_ids: BTreeMap::new(),
        frontmost_acknowledged: None,
    });
    let (worker_tx, worker_rx) = std::sync::mpsc::channel();
    store.session_navigation.test_worker = Some(Box::new(move |place, reply| {
        worker_tx.send((place, reply)).unwrap();
    }));

    store.request_session_navigation(cand.place.clone());
    let (_place, reply) = worker_rx.recv().unwrap();
    reply.send(Ok(cand.place.clone())).unwrap();

    let gen_before_nav = store.projection_generation();
    let pre_nav_commits = store.commit_local_update_count;
    assert!(store.poll_at(now + Duration::from_millis(60)));
    assert_eq!(store.commit_local_update_count, pre_nav_commits + 1);
    assert!(store.projection_generation() > gen_before_nav);
    assert!(store.projection().status_message.contains("已切换会话"));
    assert_eq!(
        store
            .lighting_arbiter
            .acknowledged_watermarks
            .get(&cand.place.target_id()),
        Some(&"wm-1".to_string())
    );

    let post_nav_commits = store.commit_local_update_count;
    assert!(!store.poll_at(now + Duration::from_millis(80)));
    assert_eq!(store.commit_local_update_count, post_nav_commits);
}

#[test]
fn idle_poll_after_browse_profile_skips_commit() {
    let (_dir, mut store, now) = active_store();
    let other = store
        .find_canonical_profile_id(CanonicalAgent::Herdr)
        .unwrap();
    store.browse_profile(&other).unwrap();
    let commits_after_browse = store.commit_local_update_count;
    let gen_after_browse = store.projection_generation();

    assert!(!store.poll_at(now + Duration::from_millis(16)));
    assert_eq!(store.commit_local_update_count, commits_after_browse);
    assert_eq!(store.projection_generation(), gen_after_browse);
}

#[test]
fn idle_live_candidates_none_does_not_force_commit_local_update() {
    let (_dir, mut store, now) = active_store();
    let commits_before = store.commit_local_update_count;
    let gen_before = store.projection_generation();

    store.handle_hardware_loop_event(
        HardwareLoopEvent::LiveCandidatesUpdated {
            epoch: 0,
            catalog: None,
        },
        now,
    );

    assert!(!store.poll_at(now + Duration::from_millis(16)));
    assert_eq!(store.commit_local_update_count, commits_before);
    assert_eq!(store.projection_generation(), gen_before);
}

#[test]
fn external_board_configuration_and_slot_role_override_mutations_are_detected_by_poll_at() {
    let (_dir, mut store, now) = active_store();
    let target = store
        .find_canonical_profile_id(CanonicalAgent::Herdr)
        .unwrap();
    store
        .assign_board_layer(BoardLayer::ALL[3], Some(target))
        .unwrap();
    let commits_after_assign = store.commit_local_update_count;
    assert!(!store.poll_at(now + Duration::from_millis(16)));
    assert_eq!(store.commit_local_update_count, commits_after_assign);

    let mut next_plan = store
        .document_store
        .board_layer_plan()
        .cloned()
        .unwrap_or_default();
    next_plan.replace(BoardLayer::ALL[3], BoardLayerRoute::Unassigned);
    store
        .document_store
        .save_board_configuration(
            next_plan,
            BoardPreviewSelection::Layer(BoardLayer::ALL[3]),
            Some("usb-startup-profile".into()),
        )
        .unwrap();
    assert!(store.poll_at(now + Duration::from_millis(32)));
    assert_eq!(store.commit_local_update_count, commits_after_assign + 1);

    let profile_id = store.selected_profile_id().unwrap().to_string();
    let flipped_role = if store.projection().canvas.keycap_cells[0].slot_role == "TaskKey" {
        KeycapRole::ActionKey
    } else {
        KeycapRole::TaskKey
    };
    store
        .slot_role_overrides
        .insert((profile_id, 0), flipped_role);
    assert!(store.poll_at(now + Duration::from_millis(48)));
    assert_eq!(store.commit_local_update_count, commits_after_assign + 2);
}

#[test]
fn localize_managed_error_detail_translates_zh_and_en_without_raw_prefix_leakage() {
    use crate::i18n::{localize_managed_error_detail, relocalize_status_message};
    use mi_model::ResolvedLocale;

    let cases = [
        (
            "blocked:uncleared_dictation_target_changed",
            "上次听写所在窗口或对话已切换，请先确认原录音已停止",
            "Previous dictation window or chat changed; confirm recording stopped",
        ),
        (
            "unconfirmed:dictation_target_changed",
            "上次听写所在窗口或对话已切换，请先确认原录音已停止",
            "Previous dictation window or chat changed; confirm recording stopped",
        ),
        (
            "unconfirmed:blocked:target_window_changed",
            "目标窗口或会话已变化，请回到原窗口后重试",
            "Target window or session changed; return to the original window and retry",
        ),
        (
            "blocked:target_identity_changed",
            "目标窗口或会话已变化，请回到原窗口后重试",
            "Target window or session changed; return to the original window and retry",
        ),
        (
            "unconfirmed:dictation_readback",
            "听写状态回读未确认，请检查原窗口是否仍在录音",
            "Dictation state unconfirmed; check whether recording is still active",
        ),
        (
            "blocked:ambiguous_or_missing_focused_pane",
            "无法确定目标分屏，请点击目标会话区域",
            "Unable to determine target pane; click inside the target session",
        ),
        (
            "blocked:ambiguous_or_missing_input",
            "未找到唯一可编辑输入框",
            "No unique editable input field found",
        ),
        (
            "blocked:dictation_control_unavailable",
            "未找到可用的听写按钮，请先切到对话输入界面",
            "Dictation control unavailable; switch to an active chat input",
        ),
        (
            "blocked:dictation_already_recording",
            "当前已在听写中",
            "Dictation is already recording",
        ),
        (
            "blocked:dictation_finishing",
            "听写正在结束处理中，请稍候",
            "Dictation is still finishing; please wait",
        ),
        (
            "blocked:accessibility_untrusted",
            "缺少 macOS 辅助功能权限，按键动作已被系统拦截，请打开辅助功能设置授权",
            "macOS Accessibility permission is required; key actions are blocked until granted in System Settings",
        ),
        (
            "blocked:unconfirmed_apple_fn_release",
            "苹果 Fn 听写键释放未确认，请点击「重试清理」",
            "Apple Fn key release unconfirmed; click Retry Cleanup",
        ),
    ];

    for (raw, expected_zh, expected_en) in cases {
        let zh = localize_managed_error_detail(raw, ResolvedLocale::ZhHans);
        let en = localize_managed_error_detail(raw, ResolvedLocale::En);
        assert_eq!(zh, expected_zh, "zh mismatch for {raw}");
        assert_eq!(en, expected_en, "en mismatch for {raw}");
        assert!(!zh.contains("blocked:") && !zh.contains("unconfirmed:"));
        assert!(!en.contains("blocked:") && !en.contains("unconfirmed:"));
        assert_eq!(
            relocalize_status_message(&format!("未执行 · {zh}"), ResolvedLocale::En),
            format!("Not executed · {expected_en}")
        );
    }

    let joined_raw = "blocked:uncleared_dictation_target_changed; unconfirmed:dictation_readback";
    let joined_zh = localize_managed_error_detail(joined_raw, ResolvedLocale::ZhHans);
    let joined_en = localize_managed_error_detail(joined_raw, ResolvedLocale::En);
    assert_eq!(
        joined_zh,
        "上次听写所在窗口或对话已切换，请先确认原录音已停止；听写状态回读未确认，请检查原窗口是否仍在录音"
    );
    assert_eq!(
        joined_en,
        "Previous dictation window or chat changed; confirm recording stopped; Dictation state unconfirmed; check whether recording is still active"
    );
    assert_eq!(
        relocalize_status_message(
            &format!("原目标释放未确认 · {joined_zh}"),
            ResolvedLocale::En
        ),
        format!("Original target release unconfirmed · {joined_en}")
    );

    for unknown in [
        "blocked:future_unknown_code",
        "unconfirmed:blocked:mystery_state",
    ] {
        let zh = localize_managed_error_detail(unknown, ResolvedLocale::ZhHans);
        let en = localize_managed_error_detail(unknown, ResolvedLocale::En);
        assert!(!zh.contains("blocked:") && !zh.contains("unconfirmed:"));
        assert!(!en.contains("blocked:") && !en.contains("unconfirmed:"));
        assert_ne!(zh, en);
    }

    let unprefixed_cases = [
        (
            "accessibility_untrusted",
            "缺少 macOS 辅助功能权限，按键动作已被系统拦截，请打开辅助功能设置授权",
            "macOS Accessibility permission is required; key actions are blocked until granted in System Settings",
        ),
        (
            "apple_fn_cleanup_unconfirmed",
            "苹果 Fn 听写键释放未确认，请点击「重试清理」",
            "Apple Fn key release unconfirmed; click Retry Cleanup",
        ),
        (
            "unconfirmed_apple_fn_release",
            "苹果 Fn 听写键释放未确认，请点击「重试清理」",
            "Apple Fn key release unconfirmed; click Retry Cleanup",
        ),
        (
            "unsupported_agent_adapter",
            "当前目标应用暂不支持该动作",
            "Current target app does not support this agent action",
        ),
        (
            "unsupported_dictation_adapter",
            "当前目标应用暂不支持托管听写",
            "Current target app does not support managed dictation",
        ),
        (
            "unsupported_agent_command",
            "当前目标应用不支持此指令",
            "Current target app does not support this command",
        ),
        (
            "session_navigation_unavailable",
            "当前无法执行会话切换",
            "Session navigation is currently unavailable",
        ),
        (
            "subagent_picker_unavailable",
            "当前目标应用不支持 Subagent 选择",
            "Subagent picker is unavailable in the current app",
        ),
        (
            "input_retired",
            "按键输入已失效，未执行动作",
            "Input event expired; action not executed",
        ),
        (
            "empty_paste_text",
            "粘贴内容为空，未执行粘贴",
            "Paste text is empty; nothing pasted",
        ),
        (
            "live_voice_readback_timeout",
            "实时语音状态回读超时，请检查目标窗口",
            "Live voice readback timed out; check target window",
        ),
        (
            "session_navigation_retired",
            "会话切换已取消或失效",
            "Session navigation cancelled or expired",
        ),
    ];
    for (raw, expected_zh, expected_en) in unprefixed_cases {
        assert_eq!(
            localize_managed_error_detail(raw, ResolvedLocale::ZhHans),
            expected_zh,
            "zh mismatch for unprefixed {raw}"
        );
        assert_eq!(
            localize_managed_error_detail(raw, ResolvedLocale::En),
            expected_en,
            "en mismatch for unprefixed {raw}"
        );
    }

    assert_eq!(
        relocalize_status_message(
            "会话未确认 · session_navigation_retired",
            ResolvedLocale::ZhHans
        ),
        "会话未确认 · 会话切换已取消或失效"
    );
    assert_eq!(
        relocalize_status_message("会话未确认 · 会话切换已取消或失效", ResolvedLocale::En),
        "Session switch unconfirmed · Session navigation cancelled or expired"
    );
    assert_eq!(
        relocalize_status_message(
            "已切换会话 · session_selection_confirmed",
            ResolvedLocale::ZhHans
        ),
        "已切换会话 · 已确认目标会话"
    );
    assert_eq!(
        relocalize_status_message("已切换会话 · 已确认目标会话", ResolvedLocale::En),
        "Switched session · Target session confirmed"
    );

    assert_eq!(
        localize_managed_error_detail("antigravity/focus-input", ResolvedLocale::ZhHans),
        "antigravity/focus-input"
    );
    assert_eq!(
        localize_managed_error_detail("antigravity/focus-input", ResolvedLocale::En),
        "antigravity/focus-input"
    );
    assert_eq!(
        localize_managed_error_detail("未找到唯一可编辑输入框", ResolvedLocale::En),
        "No unique editable input field found"
    );

    let (_dir, mut store, now) = active_store();
    store.update_now = now;
    assert!(store
        .finish_host_cleanup(Err("accessibility_untrusted".into()))
        .is_err());
    store.commit_runtime_update();
    assert_eq!(
        store.projection().status_message,
        "原目标释放未确认 · 缺少 macOS 辅助功能权限，按键动作已被系统拦截，请打开辅助功能设置授权"
    );
    assert_eq!(
        store
            .projection()
            .status_menu
            .host_caution_notice
            .as_deref(),
        Some("缺少 macOS 辅助功能权限，按键动作已被系统拦截，请打开辅助功能设置授权")
    );
    store
        .dispatch_header_action(HeaderAction::SetUiLanguage {
            language: mi_model::UiLanguagePreference::En,
        })
        .unwrap();
    assert_eq!(
        store.projection().status_message,
        "Original target release unconfirmed · macOS Accessibility permission is required; key actions are blocked until granted in System Settings"
    );
    assert_eq!(
        store
            .projection()
            .status_menu
            .host_caution_notice
            .as_deref(),
        Some("macOS Accessibility permission is required; key actions are blocked until granted in System Settings")
    );
}

#[test]
fn transient_blocked_receipt_sets_localized_status_and_caution_clears_on_success_and_expires_on_poll(
) {
    let (_dir, mut store, now) = active_store();
    store.update_now = now;

    let blocked_receipt = HostActionReceipt {
        action_type: "agent_command".into(),
        binding_id: "AG11".into(),
        profile_id: store.active_host_profile_id().unwrap(),
        target: "com.google.antigravity".into(),
        outcome: "blocked".into(),
        detail: "blocked:uncleared_dictation_target_changed".into(),
        text_byte_len: None,
        timestamp_ms: 100,
    };
    store.apply_host_action_receipt(blocked_receipt.clone(), None, now);
    store.commit_runtime_update();

    assert_eq!(
        store.last_host_action_receipt().unwrap().detail,
        "blocked:uncleared_dictation_target_changed"
    );
    assert_eq!(
        store.projection().status_message,
        "未执行 · 上次听写所在窗口或对话已切换，请先确认原录音已停止"
    );
    assert_eq!(
        store
            .projection()
            .status_menu
            .host_caution_notice
            .as_deref(),
        Some("上次听写所在窗口或对话已切换，请先确认原录音已停止")
    );
    assert!(!store.projection().status_menu.can_retry_host_cleanup);
    assert!(!store.projection().header.can_retry_host_cleanup);
    assert!(
        !store.projection().header.banner.is_caution(),
        "transient blocked receipt must not shift main window header banner"
    );

    store
        .dispatch_header_action(HeaderAction::SetUiLanguage {
            language: mi_model::UiLanguagePreference::En,
        })
        .unwrap();
    assert_eq!(
        store.projection().status_message,
        "Not executed · Previous dictation window or chat changed; confirm recording stopped"
    );
    assert_eq!(
        store
            .projection()
            .status_menu
            .host_caution_notice
            .as_deref(),
        Some("Previous dictation window or chat changed; confirm recording stopped")
    );
    store
        .dispatch_header_action(HeaderAction::SetUiLanguage {
            language: mi_model::UiLanguagePreference::ZhHans,
        })
        .unwrap();

    let ok_receipt = HostActionReceipt {
        action_type: "agent_command".into(),
        binding_id: "AG08".into(),
        profile_id: store.active_host_profile_id().unwrap(),
        target: "com.google.antigravity".into(),
        outcome: "executed".into(),
        detail: "antigravity/browser".into(),
        text_byte_len: None,
        timestamp_ms: 200,
    };
    store.apply_host_action_receipt(ok_receipt, Some("AG08"), now + Duration::from_secs(1));
    store.commit_runtime_update();
    assert!(store.projection().status_menu.host_caution_notice.is_none());

    let t_blocked = now + Duration::from_secs(2);
    store.apply_host_action_receipt(blocked_receipt, Some("AG11"), t_blocked);
    store.commit_runtime_update();
    assert!(store.projection().status_menu.host_caution_notice.is_some());

    assert!(!store.poll_at(t_blocked + HOST_CAUTION_TTL - Duration::from_millis(1)));
    assert!(store.projection().status_menu.host_caution_notice.is_some());

    assert!(store.poll_at(t_blocked + HOST_CAUTION_TTL));
    assert!(store.projection().status_menu.host_caution_notice.is_none());

    assert!(!store.poll_at(t_blocked + HOST_CAUTION_TTL + Duration::from_secs(1)));
}

#[test]
fn sticky_cleanup_failure_surfaces_caution_banner_and_self_heals_via_retry_and_enable_toggle() {
    let (_dir, mut store, now) = active_store();
    store.update_now = now;

    assert!(store
        .finish_host_cleanup(Err("unconfirmed:dictation_target_changed".into()))
        .is_err());
    store.commit_runtime_update();

    assert!(store.projection().status_menu.can_retry_host_cleanup);
    assert!(store.projection().header.can_retry_host_cleanup);
    assert_eq!(
        store
            .projection()
            .status_menu
            .host_caution_notice
            .as_deref(),
        Some("上次听写所在窗口或对话已切换，请先确认原录音已停止")
    );
    assert!(matches!(
        &store.projection().header.banner,
        OwnershipBannerState::Caution { reason }
            if reason == "上次听写所在窗口或对话已切换，请先确认原录音已停止"
    ));

    assert!(!store.poll_at(now + HOST_CAUTION_TTL + Duration::from_secs(60)));
    assert!(store.projection().status_menu.can_retry_host_cleanup);
    assert!(store.projection().status_menu.host_caution_notice.is_some());

    let transient = HostActionReceipt {
        action_type: "agent_command".into(),
        binding_id: "AG07".into(),
        profile_id: store.active_host_profile_id().unwrap(),
        target: "com.google.antigravity".into(),
        outcome: "blocked".into(),
        detail: "blocked:ambiguous_or_missing_input".into(),
        text_byte_len: None,
        timestamp_ms: 300,
    };
    store.apply_host_action_receipt(transient, Some("AG07"), now + Duration::from_secs(61));
    store.commit_runtime_update();
    assert_eq!(
        store
            .projection()
            .status_menu
            .host_caution_notice
            .as_deref(),
        Some("上次听写所在窗口或对话已切换，请先确认原录音已停止"),
        "transient blocked receipt must not overwrite active sticky cleanup caution"
    );

    store
        .dispatch_status_menu_action(StatusMenuAction::RetryHostCleanup)
        .unwrap();
    assert!(store.ptt_controller.cleanup_confirmed);
    assert!(!store.projection().status_menu.can_retry_host_cleanup);
    assert!(!store.projection().header.can_retry_host_cleanup);
    assert!(store.projection().status_menu.host_caution_notice.is_none());
    assert!(!store.projection().header.banner.is_caution());

    assert!(store
        .finish_host_cleanup(Err("unconfirmed:dictation_readback".into()))
        .is_err());
    store.commit_runtime_update();
    assert!(store.projection().header.can_retry_host_cleanup);
    assert!(store.projection().header.banner.is_caution());

    store
        .dispatch_header_action(HeaderAction::RetryHostCleanup)
        .unwrap();
    assert!(store.ptt_controller.cleanup_confirmed);
    assert!(!store.projection().header.can_retry_host_cleanup);
    assert!(store.projection().status_menu.host_caution_notice.is_none());
    assert!(!store.projection().header.banner.is_caution());

    assert!(store
        .finish_host_cleanup(Err("unconfirmed:dictation_target_changed".into()))
        .is_err());
    store.commit_runtime_update();
    assert!(!store.ptt_controller.cleanup_confirmed);
    assert!(store.projection().header.can_retry_host_cleanup);

    store.set_host_execution_enabled(true).unwrap();
    assert!(store.ptt_controller.cleanup_confirmed);
    assert!(!store.projection().header.can_retry_host_cleanup);
    assert!(!store.projection().status_menu.can_retry_host_cleanup);
    assert!(store.projection().status_menu.host_caution_notice.is_none());
    assert!(!store.projection().header.banner.is_caution());
}

#[test]
fn finish_dictation_preserves_sticky_caution_when_apple_fn_release_unconfirmed() {
    let (_dir, mut store, now) = active_store();
    store.update_now = now;
    let profile_id = store.active_host_profile_id().unwrap();
    store.held_dictation = Some(HeldDictation {
        input: HeldInputId {
            session_id: 9,
            binding_id: "AG11".into(),
        },
        profile_id,
        adapter: "antigravity".into(),
    });
    store
        .live_executor
        .set_unconfirmed_apple_fn_release_for_test(Some(HeldInputId {
            session_id: 9,
            binding_id: "AG10".into(),
        }));
    store.host_caution = Some(HostActionCaution {
        detail_code: "blocked:unconfirmed_apple_fn_release".into(),
        is_sticky_cleanup: true,
        expires_at: None,
    });
    store.commit_runtime_update();

    let receipt = store.finish_dictation(Ok(())).expect("dictation receipt");
    assert!(receipt.succeeded());
    assert!(store.held_dictation.is_none());
    assert!(store.ptt_controller.cleanup_confirmed);
    assert!(store.can_retry_host_cleanup());
    assert!(store.projection().status_menu.can_retry_host_cleanup);
    assert_eq!(
        store
            .projection()
            .status_menu
            .host_caution_notice
            .as_deref(),
        Some("苹果 Fn 听写键释放未确认，请点击「重试清理」")
    );

    store
        .live_executor
        .set_unconfirmed_apple_fn_release_for_test(None);
    store.held_dictation = Some(HeldDictation {
        input: HeldInputId {
            session_id: 9,
            binding_id: "AG11".into(),
        },
        profile_id: store.active_host_profile_id().unwrap(),
        adapter: "antigravity".into(),
    });
    let receipt2 = store
        .finish_dictation(Ok(()))
        .expect("second dictation receipt");
    assert!(receipt2.succeeded());
    assert!(!store.can_retry_host_cleanup());
    assert!(store.projection().status_menu.host_caution_notice.is_none());
}

#[test]
fn radial_blocked_receipt_populates_status_menu_host_caution_notice() {
    let (_dir, mut store, now) = active_store();
    store.update_now = now;
    store.set_radial_menu_enabled(true);
    store.consume_radial_input(
        RadialInputEvent::Position {
            session_id: 7,
            ingress_seq: 1,
            sample: RadialPositionSample::new(0.0, 0.1),
            admitted: true,
        },
        now,
    );
    store.consume_radial_input(
        RadialInputEvent::Position {
            session_id: 7,
            ingress_seq: 2,
            sample: RadialPositionSample::new(0.0, 0.9),
            admitted: true,
        },
        now + Duration::from_millis(20),
    );
    let session = store.radial_state.session().unwrap().clone();
    let blocked = HostActionReceipt {
        action_type: "open_app".into(),
        binding_id: "radial-menu".into(),
        profile_id: session.route.profile_id.clone().unwrap(),
        target: "test-app".into(),
        detail: "apple_fn_cleanup_unconfirmed".into(),
        outcome: "blocked".into(),
        text_byte_len: None,
        timestamp_ms: 0,
    };
    let receipt = store.execute_radial_intent_using(
        &session,
        &RadialIntent::OpenApp {
            bundle_id: "test-app".into(),
        },
        |_, _| blocked.clone(),
    );
    store.commit_runtime_update();
    assert_eq!(receipt.detail, "apple_fn_cleanup_unconfirmed");
    assert_eq!(
        store
            .projection()
            .status_menu
            .host_caution_notice
            .as_deref(),
        Some("苹果 Fn 听写键释放未确认，请点击「重试清理」")
    );
}
