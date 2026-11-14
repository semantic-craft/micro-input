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
    let mut expected = BTreeMap::new();
    for binding in graph.layers.get_mut(&layer_id).unwrap().controls.values_mut() {
        let slot: usize = binding.logical_control.strip_prefix("logical-").unwrap().parse().unwrap();
        if [0, 12].contains(&slot) {
            binding.action_id = None;
        }
        let label = if let Some(id) = binding.action_id.as_ref() {
            let action = graph.actions.get_mut(id).unwrap();
            action.name = format!("custom-native-control-{slot}");
            action.name.clone()
        } else {
            "未绑定".to_string()
        };
        expected.insert(slot, (label, binding.action_id.is_some()));
    }
    store.select_and_activate_profile_at(&native, now, false).unwrap();
    store.dispatch_sidebar_action(SidebarAction::SetProfileLocked(true)).unwrap();
    let before = serde_json::to_value(&store.document_store.draft.document).unwrap();
    let menu = &store.projection().status_menu;
    assert_eq!(menu.active_profile_id.as_deref(), Some(native.as_str()));
    let sheet = menu.cheat_sheet_preview.clone().unwrap();
    assert_eq!(sheet.profile_name, "Codex");
    assert_eq!(sheet.key_cells.len(), 20);
    for (slot, binding) in mi_model::direct_controls::CONTROL_BINDINGS.iter().enumerate() {
        let cell = sheet.key_cells.iter().find(|cell| cell.binding_id == *binding).unwrap();
        assert_eq!((&cell.label, cell.available), (&expected[&slot].0, expected[&slot].1), "{binding}");
    }
    assert_eq!(store.projection_inputs().build_cheat_sheet_projection().key_cells, sheet.key_cells);
    let other = store.find_canonical_profile_id(CanonicalAgent::Herdr).unwrap();
    store.browse_profile(&other).unwrap();
    assert_eq!(store.projection().status_menu.cheat_sheet_preview.as_ref(), Some(&sheet));
    store.bottom_row_layout = BottomRowLayout::Merged;
    store.commit_runtime_update();
    let merged = store.projection().status_menu.cheat_sheet_preview.as_ref().unwrap();
    assert_eq!(merged.key_cells.len(), 19);
    assert!(!merged.key_cells.iter().any(|cell| cell.binding_id == "AG11"));
    assert_eq!(merged.key_cells.iter().find(|cell| cell.binding_id == "AG10").unwrap().label, "custom-native-control-10");
    assert_eq!(serde_json::to_value(&store.document_store.draft.document).unwrap(), before);

    store.document_store.draft.document.editable.graph.profiles.get_mut(&native).unwrap().name = "Other protected owner".into();
    store.commit_runtime_update();
    assert!(store.projection().status_menu.cheat_sheet_preview.as_ref().unwrap().key_cells.iter().all(|cell| !cell.available));
    let graph = &mut store.document_store.draft.document.editable.graph;
    graph.profiles.get_mut(&native).unwrap().name = "Codex".into();
    graph.profiles.remove(&local);
    graph.profile_order.retain(|id| id != &local);
    store.commit_runtime_update();
    assert!(store.projection().status_menu.cheat_sheet_preview.as_ref().unwrap().key_cells.iter().all(|cell| !cell.available));
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
        store.auto_follow_enabled = false;
        store.manual_override = true;
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
        for slot in 0..6 {
            assert_eq!(
                plan.actions[slot],
                Some(ResolvedHostAction::AgentCommand {
                    adapter: "four-app".into(),
                    command: format!("slot-open:{slot}"),
                })
            );
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
        store.auto_follow_enabled = false;
        store.manual_override = true;
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
            let receipt = store
                .route_board_input(event, now)
                .expect("local custom action");
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
