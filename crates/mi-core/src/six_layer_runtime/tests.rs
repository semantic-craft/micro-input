use super::encoder_dispatch::{execute_clicks, execute_clicks_with_encoder_picker};
use super::lifecycle::{
    connect, merge_previous_original_into_backup, prepare_apply_karabiner_patch,
    with_verification_session,
};
use super::vendor_bridge::{handle_vendor_slot_notification, record_v070_bridge_observation};
use super::*;
use crate::karabiner::{self, ConfigPatch};
use mi_model::board_layers::BoardLayerPlan;
use mi_model::direct_controls::CONTROL_BINDINGS;
use mi_model::host_router::BottomRowLayout;
use mi_transport::ActivationPayload;
use std::os::unix::net::UnixDatagram;

#[test]
fn verification_session_is_closed_on_success_and_failure() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    for outcome in [Ok(()), Err("readback mismatch".to_string())] {
        let session = {
            let _entered = runtime.enter();
            let (link, _) = mi_transport::MockLink::new();
            DeviceSession::new(
                link,
                Box::new(mi_proto::NativeFramer::default()),
                Arc::new(mi_session::ReadOnlySchema),
                mi_session::SessionConfig::default(),
            )
        };
        let expected = outcome.clone();
        let result = runtime.block_on(with_verification_session(&session, async { outcome }));
        assert_eq!(result, expected);
        assert!(session.is_disconnected());
    }
}

#[test]
fn executor_runs_after_verification_rpc_closes_and_stops_on_input_loss() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let (link, _) = mi_transport::MockLink::new();
    let session = {
        let _entered = runtime.enter();
        DeviceSession::new(
            link.clone(),
            Box::new(mi_proto::NativeFramer::default()),
            Arc::new(mi_session::ReadOnlySchema),
            mi_session::SessionConfig::default(),
        )
    };
    let mut running = RunningSixLayer::isolated(session.clone(), directory.path());
    running.readiness.configuration_observed = true;
    running.readiness.keyboard_grabbed = true;
    runtime
        .block_on(with_verification_session(&session, async { Ok(()) }))
        .unwrap();
    assert!(session.is_disconnected());
    running.receiver.arm().unwrap();
    running.set_execution_policy(true, true, Instant::now() - Duration::from_secs(1));
    running.foreground_changed(Some("com.google.antigravity"));
    let driver = mi_macos::RecordingOsDriver::default();
    running.start_executor(Arc::new(mi_macos::LiveHostActionExecutor::new(
        driver.clone(),
    )));
    send(&running.receiver, 40);
    send(&running.receiver, 7);
    let receipt = running
        .receipts
        .as_ref()
        .unwrap()
        .recv_timeout(Duration::from_secs(3))
        .unwrap();
    assert_eq!(receipt.outcome, "executed");
    assert_eq!(receipt.detail, "antigravity/focus-input");
    assert_eq!(
        receipt.profile_id,
        running.installation.plan.hosts[0].profile_id
    );
    assert_eq!(
        driver.events(),
        vec![
            mi_macos::executor::RecordedOsEvent::CheckAccessibilityTrusted,
            mi_macos::executor::RecordedOsEvent::FocusAgentInput("antigravity".into()),
        ]
    );
    assert!(link.writes.lock().unwrap().is_empty());
    assert!(running.receiver.failure().is_none());
    assert!(!check_input_connection(&running.receiver, false, || Err(
        "USB target changed".into()
    )));
    send(&running.receiver, 7);
    running.executor.take().unwrap().join().unwrap().unwrap();
    assert!(running.receipts.as_ref().unwrap().try_recv().is_err());
    assert_eq!(driver.events().len(), 2);
    running.shutdown(&runtime).unwrap();
}

#[test]
fn input_monitor_ticks_do_not_need_vendor_rpc_and_fail_closed_on_readiness_loss() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (link, _) = mi_transport::MockLink::new();
    let session = {
        let _entered = runtime.enter();
        DeviceSession::new(
            link.clone(),
            Box::new(mi_proto::NativeFramer::default()),
            Arc::new(mi_session::ReadOnlySchema),
            mi_session::SessionConfig::default(),
        )
    };
    runtime.block_on(session.shutdown());
    let receiver = UserCommandReceiver::bind().unwrap();
    receiver.arm().unwrap();
    for _ in 0..1000 {
        assert!(check_input_connection(&receiver, false, || Ok(())));
    }
    assert!(link.writes.lock().unwrap().is_empty());
    assert!(receiver.failure().is_none());
    assert!(!check_input_connection(&receiver, false, || Err(
        "USB target changed".into()
    )));
    assert_eq!(receiver.failure().as_deref(), Some("USB target changed"));
    assert!(receiver.next().is_err());
    let receiver = UserCommandReceiver::bind().unwrap();
    assert!(!check_input_connection(&receiver, true, || panic!(
        "sleep stops before recheck"
    )));
    assert!(receiver.failure().unwrap().contains("睡眠"));
}

fn send(receiver: &UserCommandReceiver, control: u8) {
    let payload =
        serde_json::to_vec(&ActivationPayload::new(receiver.generation(), control)).unwrap();
    UnixDatagram::unbound()
        .unwrap()
        .send_to(&payload, receiver.path())
        .unwrap();
}

fn test_plan() -> AppliedSixLayerPlan {
    let mut draft = mi_model::draft::Draft::default();
    mi_model::preset::install_agent_starter_profiles(&mut draft).unwrap();
    let id = |name: &str| {
        draft
            .document
            .editable
            .graph
            .profiles
            .iter()
            .find(|(_, profile)| profile.name == name)
            .unwrap()
            .0
            .clone()
    };
    let desired =
        BoardLayerPlan::recommended(id("Codex"), id("Antigravity"), id("DeepSeek Coder (DSH)"));
    AppliedSixLayerPlan::compile(&desired, &draft.document, BottomRowLayout::Split, true).unwrap()
}

#[test]
fn native_codex_all_controls_dispatch_compiled_actions_and_pause_without_events() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = crate::WorkbenchStore::open_isolated(directory.path()).unwrap();
    let local = store
        .document_store
        .draft
        .document
        .editable
        .graph
        .profiles
        .iter()
        .find(|(_, profile)| profile.name == mi_model::preset::LOCAL_CODEX_PROFILE_NAME)
        .unwrap()
        .0
        .clone();
    let graph = &mut store.document_store.draft.document.editable.graph;
    let layer_id = graph.profiles[&local].layer_order[0].clone();
    graph.layers.get_mut(&layer_id).unwrap().control_settings =
        Some(mi_model::control_settings::ControlSettings {
            joystick_mode: "logical_sectors".into(),
            active_sectors: 4,
            encoder_hold_action_id: None,
        });
    let template = graph.actions.values().next().unwrap().clone();
    for binding in graph
        .layers
        .get_mut(&layer_id)
        .unwrap()
        .controls
        .values_mut()
    {
        let slot = binding.logical_control.strip_prefix("logical-").unwrap();
        let id = format!("receiver-control-{slot}");
        let mut action = template.clone();
        action.payload = serde_json::json!({
            "type": "open_url", "url": format!("https://example.com/control/{slot}")
        });
        graph.actions.insert(id.clone(), action);
        binding.action_id = Some(id);
    }
    let native = store
        .find_canonical_profile_id(crate::CanonicalAgent::Codex)
        .unwrap();
    let pinned = store.compile_profile_twenty_slots(&native).unwrap();
    let plan = test_plan();
    for mode in [
        SixLayerRoutingMode::Locked(Box::new(pinned.clone())),
        SixLayerRoutingMode::ManualPinned(Box::new(pinned.clone())),
    ] {
        for enabled in [true, false] {
            let receiver = UserCommandReceiver::bind().unwrap();
            receiver.arm().unwrap();
            let routing = Mutex::new(RoutingContext::default());
            routing
                .lock()
                .unwrap()
                .set_routing(mode.clone(), Some("com.openai.codex"));
            for control in 0..20 {
                send(&receiver, control);
            }
            let stop = AtomicBool::new(false);
            let policy = ExecutionPolicy::default();
            policy.set(enabled, true, Instant::now() - Duration::from_secs(1));
            let (tx, rx) = std::sync::mpsc::sync_channel(20);
            let mut dispatched = Vec::new();
            let mut received = 0;
            let deadline = Instant::now() + Duration::from_secs(1);
            execute_clicks(
                &receiver,
                &plan,
                &stop,
                &policy,
                &AtomicU8::new(1),
                &AtomicBool::new(false),
                &routing,
                &tx,
                None,
                || {
                    let next = receiver.next();
                    match &next {
                        Ok(Some(_)) => received += 1,
                        Ok(None) if received == 20 => stop.store(true, Ordering::Release),
                        Ok(None) => {
                            assert!(Instant::now() < deadline, "input delivery timed out")
                        }
                        Err(error) => panic!("input delivery failed: {error}"),
                    }
                    next
                },
                || Ok(()),
                |profile, binding, action, _| {
                    dispatched.push((profile.to_owned(), binding.to_owned(), action.clone()));
                    mi_macos::SimulatedHostActionExecutor.execute(profile, binding, action)
                },
            );
            let expected = if enabled {
                (0..20)
                    .map(|slot| {
                        (
                            native.clone(),
                            mi_model::direct_controls::CONTROL_BINDINGS[slot].into(),
                            ResolvedHostAction::OpenUrl {
                                url: format!("https://example.com/control/{slot}"),
                            },
                        )
                    })
                    .collect()
            } else {
                Vec::new()
            };
            assert_eq!(dispatched, expected);
            let receipts: Vec<_> = rx.try_iter().collect();
            assert_eq!(receipts.len(), if enabled { 20 } else { 0 });
            assert!(receipts.iter().all(|r| r.outcome == "simulated"));
            assert!(receiver.failure().is_none());
        }
    }
}

#[test]
fn custom_codex_routes_layer_one_vendor_in_manual_and_board_layer_modes() {
    let plan = test_plan();
    let custom = PinnedProfilePlan {
        profile_id: "local-codex".into(),
        bundle_id: Some("com.openai.codex".into()),
        actions: plan.hosts[0].actions.clone(),
        encoder_hold_action: plan.hosts[0].encoder_hold_action.clone(),
    };
    for mode in [
        SixLayerRoutingMode::ManualPinned(Box::new(custom.clone())),
        SixLayerRoutingMode::BoardLayers {
            layer_one: Some(Box::new(custom.clone())),
            layer_two: None,
            layer_three: None,
        },
    ] {
        let mut context = RoutingContext::default();
        context.set_routing(mode, Some("com.apple.Safari"));
        assert!(context.accepts_layer1_vendor_fallback());
        assert_eq!(
            context.effective_profile_id(&plan, 1).as_deref(),
            Some("local-codex")
        );
        assert_eq!(
            context.resolve_control(&plan, 1, 0).map(|(id, _)| id),
            Some("local-codex")
        );
    }
}

#[test]
fn custom_codex_vendor_dispatches_in_each_mode_and_stops_when_exclusive_session_is_lost() {
    use mi_proto::Framer;
    use mi_transport::HidLink;

    let plan = test_plan();
    let custom = PinnedProfilePlan {
        profile_id: "local-codex".into(),
        bundle_id: Some("com.openai.codex".into()),
        actions: std::array::from_fn(|slot| {
            (slot == 0).then(|| ResolvedHostAction::AgentCommand {
                adapter: "codex".into(),
                command: "focus-input".into(),
            })
        }),
        encoder_hold_action: None,
    };
    let runtime = tokio::runtime::Runtime::new().unwrap();
    for mode in [
        SixLayerRoutingMode::Locked(Box::new(custom.clone())),
        SixLayerRoutingMode::ManualPinned(Box::new(custom.clone())),
        SixLayerRoutingMode::BoardLayers {
            layer_one: Some(Box::new(custom.clone())),
            layer_two: None,
            layer_three: None,
        },
    ] {
        let (link, _writes) = mi_transport::MockLink::new();
        let session = {
            let _entered = runtime.enter();
            DeviceSession::new(
                Arc::new(MockVendorLink {
                    inner: link.clone(),
                    exclusive: true,
                }),
                Box::new(mi_proto::NativeFramer::default()),
                Arc::new(mi_session::ReadOnlySchema),
                mi_session::SessionConfig::default(),
            )
        };
        assert!(session.has_exclusive_vendor_link());
        let receiver = Arc::new(UserCommandReceiver::bind().unwrap());
        receiver.arm().unwrap();
        let routing = Arc::new(Mutex::new(RoutingContext::default()));
        routing
            .lock()
            .unwrap()
            .set_routing(mode, Some("com.openai.codex"));
        let mut bridge = VendorBridge::new(Some(session.clone()));
        bridge.spawn_with_reopen(
            &runtime,
            Some(session.clone()),
            receiver.clone(),
            routing.clone(),
            true,
            || async { None },
        );
        let wire = b"{\"m\":\"v.oai.hid\",\"p\":{\"k\":\"AG00\",\"act\":1}}\n";
        for frame in mi_proto::NativeFramer::default().encode(wire).unwrap() {
            link.inject.send(frame).unwrap();
        }
        let until = Instant::now() + Duration::from_secs(1);
        let mut event = loop {
            if let Some(event) = receiver.next().unwrap() {
                break Some(event);
            }
            assert!(
                Instant::now() < until,
                "Codex vendor press was not delivered"
            );
            thread::yield_now();
        };
        let stop = AtomicBool::new(false);
        let policy = ExecutionPolicy::default();
        policy.set(true, true, Instant::now() - Duration::from_secs(1));
        let (tx, rx) = std::sync::mpsc::sync_channel(2);
        execute_clicks(
            &receiver,
            &plan,
            &stop,
            &policy,
            &AtomicU8::new(1),
            &AtomicBool::new(false),
            &routing,
            &tx,
            None,
            || {
                let next = event.take();
                if next.is_none() {
                    stop.store(true, Ordering::Release);
                }
                Ok(next)
            },
            || {
                session
                    .has_exclusive_vendor_link()
                    .then_some(())
                    .ok_or_else(|| "exclusive ownership missing".into())
            },
            |profile, binding, action, _| {
                mi_macos::SimulatedHostActionExecutor.execute(profile, binding, action)
            },
        );
        let receipt = rx.try_recv().unwrap();
        assert_eq!(receipt.profile_id, "local-codex");
        assert_eq!(receipt.detail, "codex/focus-input");
        assert_eq!(receipt.outcome, "simulated");
        link.close();
        let until = Instant::now() + Duration::from_secs(1);
        while receiver.failure().is_none() {
            assert!(Instant::now() < until, "lost owner must retire input");
            thread::yield_now();
        }
        assert!(receiver.next().is_err());
        assert!(rx.try_recv().is_err());
        bridge.shutdown(&runtime);
    }
    let mut context = RoutingContext::default();
    let protected = PinnedProfilePlan {
        actions: std::array::from_fn(|_| None),
        encoder_hold_action: None,
        ..custom
    };
    for mode in [
        SixLayerRoutingMode::Locked(Box::new(protected.clone())),
        SixLayerRoutingMode::ManualPinned(Box::new(protected)),
    ] {
        context.set_routing(mode, Some("com.openai.codex"));
        assert!(!context.accepts_layer1_vendor_fallback());
        assert!(context.resolve_control(&plan, 1, 0).is_none());
    }
}

#[test]
fn six_layer_connection_rejects_a_shared_vendor_link() {
    let (link, _writes) = mi_transport::MockLink::new();
    let transport = mi_transport::MockTransport::new(link);
    let target = TargetKeyboard {
        vendor_id: 0x303a,
        product_id: 0x8298,
        location_id: 1,
        usb_parent: 1,
        vendor_path: "synthetic".into(),
    };
    transport.set_devices(vec![HidCandidate {
        path: target.vendor_path.clone(),
        vid: target.vendor_id,
        pid: target.product_id,
        product: "Synthetic".into(),
        usage_page: 0xff00,
        usage: 1,
        usage_pairs: vec![],
        transport_hint: mi_transport::TransportHint::Usb,
        serial_number: None,
    }]);
    assert!(
        matches!(connect(&transport, &target), Err(error) if error.contains("未取得 USB 专用接口独占"))
    );
}

#[test]
fn board_layers_route_by_physical_layer_regardless_of_foreground_app() {
    let plan = test_plan();
    let receiver = UserCommandReceiver::bind().unwrap();
    receiver.arm().unwrap();
    let layer_one = PinnedProfilePlan {
        profile_id: "local-codex".into(),
        bundle_id: Some("com.openai.codex".into()),
        actions: std::array::from_fn(|_| None),
        encoder_hold_action: None,
    };
    let routing = Mutex::new(RoutingContext::default());
    routing.lock().unwrap().set_routing(
        SixLayerRoutingMode::BoardLayers {
            layer_one: Some(Box::new(layer_one)),
            layer_two: None,
            layer_three: None,
        },
        Some("com.google.antigravity"),
    );
    send(&receiver, 27);
    thread::sleep(Duration::from_millis(30));
    routing.lock().unwrap().update(Some("com.deepseek.dsh"));
    send(&receiver, 7);
    let stop = AtomicBool::new(false);
    let policy = ExecutionPolicy::default();
    policy.set(true, true, Instant::now() - Duration::from_secs(1));
    let observed = AtomicU8::new(1);
    let (tx, rx) = std::sync::mpsc::sync_channel(8);
    let mut dispatched = 0;
    thread::scope(|scope| {
        let (done, wait) = std::sync::mpsc::channel::<()>();
        let stop_ref = &stop;
        scope.spawn(move || {
            if wait.recv_timeout(Duration::from_secs(5)).is_err() {
                stop_ref.store(true, Ordering::Release);
            }
        });
        execute_clicks(
            &receiver,
            &plan,
            &stop,
            &policy,
            &observed,
            &AtomicBool::new(false),
            &routing,
            &tx,
            None,
            || receiver.next(),
            || Ok(()),
            |profile, binding, action, _| {
                dispatched += 1;
                if dispatched == 2 {
                    stop.store(true, Ordering::Release);
                }
                mi_macos::SimulatedHostActionExecutor.execute(profile, binding, action)
            },
        );
        let _ = done.send(());
    });
    let receipts: Vec<_> = rx.try_iter().collect();
    assert_eq!(receipts.len(), 2);
    assert_eq!(receipts[0].profile_id, plan.hosts[1].profile_id);
    assert_eq!(receipts[0].detail, "dsh-desktop/browser");
    assert_eq!(receipts[1].profile_id, plan.hosts[0].profile_id);
    assert_eq!(receipts[1].detail, "antigravity/focus-input");
    assert_eq!(observed.load(Ordering::Acquire), 2);
    let mut ctx = routing.lock().unwrap();
    ctx.update(Some("com.apple.finder"));
    assert_eq!(
        ctx.effective_profile_id(&plan, 1).as_deref(),
        Some("local-codex")
    );
    assert_eq!(
        ctx.effective_profile_id(&plan, 2).as_deref(),
        Some(plan.hosts[0].profile_id.as_str())
    );
    assert_eq!(
        ctx.effective_profile_id(&plan, 3).as_deref(),
        Some(plan.hosts[1].profile_id.as_str())
    );
    assert!(ctx.effective_profile_id(&plan, 4).is_none());
}

#[test]
fn execute_clicks_ignores_retired_layer_and_stops_on_connection_loss() {
    let plan = test_plan();
    let ag_id = plan.hosts[0].profile_id.clone();
    let dsh_id = plan.hosts[1].profile_id.clone();

    let receiver = Arc::new(UserCommandReceiver::bind().unwrap());
    receiver.arm().unwrap();
    send(&receiver, 0);
    send(&receiver, 20);
    send(&receiver, 40);
    send(&receiver, 21);
    thread::sleep(Duration::from_millis(30));

    let stop = AtomicBool::new(false);
    let policy = ExecutionPolicy::default();
    policy.set(true, true, Instant::now() - Duration::from_secs(1));
    let observed = AtomicU8::new(1);
    let first = AtomicBool::new(false);
    let (tx, rx) = std::sync::mpsc::sync_channel(8);
    let mut calls = Vec::new();
    let mut permit_calls = 0usize;

    execute_clicks(
        &receiver,
        &plan,
        &stop,
        &policy,
        &observed,
        &first,
        &Mutex::new(RoutingContext::default()),
        &tx,
        None,
        || receiver.next(),
        || {
            permit_calls += 1;
            if permit_calls <= 2 {
                Ok(())
            } else {
                Err("设备连接已结束".into())
            }
        },
        |profile, binding, _action, _epoch| {
            calls.push((profile.to_owned(), binding.to_owned()));
            mi_macos::HostActionReceipt {
                action_type: "open_app".into(),
                binding_id: binding.into(),
                detail: "ok".into(),
                outcome: "executed".into(),
                profile_id: profile.into(),
                target: "test".into(),
                text_byte_len: None,
                timestamp_ms: 1,
            }
        },
    );

    assert!(first.load(Ordering::Acquire));
    assert_eq!(observed.load(Ordering::Acquire), 3);
    assert_eq!(
        calls,
        vec![(ag_id, "AG00".into()), (dsh_id, "AG00".into()),]
    );
    assert_eq!(rx.try_iter().count(), 2);
    assert_eq!(receiver.failure().as_deref(), Some("设备连接已结束"));
}

#[test]
fn a_click_expiring_during_connection_check_does_not_stop_fresh_clicks() {
    let plan = test_plan();
    let dsh_id = plan.hosts[1].profile_id.clone();

    let receiver = Arc::new(UserCommandReceiver::bind().unwrap());
    receiver.arm().unwrap();
    send(&receiver, 0);
    thread::sleep(Duration::from_millis(30));
    let stop = AtomicBool::new(false);
    let policy = ExecutionPolicy::default();
    policy.set(true, true, Instant::now() - Duration::from_secs(3));
    let observed = AtomicU8::new(1);
    let first = AtomicBool::new(false);
    let (tx, rx) = std::sync::mpsc::sync_channel(8);
    let mut calls = Vec::new();
    let mut permit_calls = 0;
    thread::scope(|scope| {
        let (cancel_tx, cancel_rx) = std::sync::mpsc::channel::<()>();
        let watchdog_stop = &stop;
        scope.spawn(move || {
            if cancel_rx.recv_timeout(Duration::from_secs(5)).is_err() {
                watchdog_stop.store(true, Ordering::Release);
            }
        });
        execute_clicks(
            &receiver,
            &plan,
            &stop,
            &policy,
            &observed,
            &first,
            &Mutex::new(RoutingContext::default()),
            &tx,
            None,
            || receiver.next(),
            || {
                permit_calls += 1;
                if permit_calls == 1 {
                    thread::sleep(Duration::from_millis(2100));
                    send(&receiver, 20);
                    thread::sleep(Duration::from_millis(30));
                }
                Ok(())
            },
            |profile, binding, _, _| {
                calls.push((profile.to_owned(), binding.to_owned()));
                stop.store(true, Ordering::Release);
                mi_macos::HostActionReceipt {
                    action_type: "open_app".into(),
                    binding_id: binding.into(),
                    detail: "ok".into(),
                    outcome: "executed".into(),
                    profile_id: profile.into(),
                    target: "test".into(),
                    text_byte_len: None,
                    timestamp_ms: 1,
                }
            },
        );
        let _ = cancel_tx.send(());
    });
    assert_eq!(calls, vec![(dsh_id, "AG00".into())]);
    assert_eq!(permit_calls, 2);
    assert_eq!(rx.try_iter().count(), 1);
    assert!(receiver.failure().is_none());
}

#[test]
fn disable_then_enable_rejects_in_flight_request_and_accepts_new_input() {
    use mi_macos::{LiveHostActionExecutor, RecordingOsDriver};
    use std::sync::mpsc;

    let receiver = Arc::new(UserCommandReceiver::bind().unwrap());
    let plan = test_plan();
    let now = Instant::now();
    let policy = Arc::new(ExecutionPolicy::default());
    policy.set(true, true, now);
    let old_admission = policy.capture(now);
    policy.set(true, true, now + Duration::from_millis(1));
    assert_eq!(policy.capture(now).epoch, old_admission.epoch);
    let driver = RecordingOsDriver::new(true);
    let executor = Arc::new(LiveHostActionExecutor::new(driver.clone()));
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = mpsc::sync_channel(1);
    let (receipt_tx, receipt_rx) = mpsc::sync_channel(1);
    let worker_policy = Arc::clone(&policy);
    let worker_executor = Arc::clone(&executor);
    let worker = thread::spawn(move || {
        let stop = AtomicBool::new(false);
        let event = mi_transport::Activation {
            control: 0,
            layer_override: None,
            released: false,
            generation: receiver.generation().into(),
            received_sequence: 1,
            received_at: now,
        };
        let mut pending = Some(event);
        execute_clicks(
            &receiver,
            &plan,
            &stop,
            &worker_policy,
            &AtomicU8::new(1),
            &AtomicBool::new(false),
            &Mutex::new(RoutingContext::default()),
            &receipt_tx,
            None,
            || {
                pending
                    .take()
                    .map(Some)
                    .ok_or_else(|| "test input exhausted".into())
            },
            || {
                entered_tx.send(()).unwrap();
                resume_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                Ok(())
            },
            |profile, binding, action, admission| {
                let receipt = worker_executor.execute_when(profile, binding, action, || {
                    !stop.load(Ordering::Acquire)
                        && receiver.failure().is_none()
                        && worker_policy.admits(admission, action)
                });
                stop.store(true, Ordering::Release);
                receipt
            },
        );
    });
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    policy.set(false, true, now + Duration::from_millis(2));
    executor.stop_owned_actions().unwrap();
    policy.set(true, true, now + Duration::from_millis(3));
    resume_tx.send(()).unwrap();
    worker.join().unwrap();
    let receipt = receipt_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(receipt.outcome, "blocked");
    assert_eq!(receipt.detail, "input_retired");
    assert!(driver.events().is_empty());
    let action = ResolvedHostAction::OpenApp {
        bundle_id: "com.example.editor".into(),
    };
    let new_admission = policy.capture(now + Duration::from_millis(4));
    let receipt = executor.execute_when("new", "AG00", &action, || {
        policy.admits(new_admission, &action)
    });
    assert_eq!(receipt.outcome, "executed");
    assert!(!driver.events().is_empty());
}

#[test]
fn disable_then_enable_discards_queued_old_input_before_execution() {
    use mi_macos::{LiveHostActionExecutor, RecordingOsDriver};

    let receiver = UserCommandReceiver::bind().unwrap();
    let plan = test_plan();
    let now = Instant::now();
    let policy = ExecutionPolicy::default();
    policy.set(true, true, now);
    let mut pending = std::collections::VecDeque::from([
        mi_transport::Activation {
            control: 7,
            layer_override: None,
            released: false,
            generation: receiver.generation().into(),
            received_sequence: 1,
            received_at: now,
        },
        mi_transport::Activation {
            control: 7,
            layer_override: None,
            released: false,
            generation: receiver.generation().into(),
            received_sequence: 2,
            received_at: now + Duration::from_millis(3),
        },
    ]);
    let driver = RecordingOsDriver::new(true);
    let executor = LiveHostActionExecutor::new(driver.clone());
    policy.set(false, true, now + Duration::from_millis(1));
    executor.stop_owned_actions().unwrap();
    policy.set(true, true, now + Duration::from_millis(2));
    let stop = AtomicBool::new(false);
    let (tx, rx) = std::sync::mpsc::sync_channel(2);
    let mut executed = 0;
    execute_clicks(
        &receiver,
        &plan,
        &stop,
        &policy,
        &AtomicU8::new(1),
        &AtomicBool::new(false),
        &Mutex::new(RoutingContext::default()),
        &tx,
        None,
        || {
            pending
                .pop_front()
                .map(Some)
                .ok_or_else(|| "test input exhausted".into())
        },
        || Ok(()),
        |profile, binding, action, admission| {
            assert_eq!(admission.received_at, now + Duration::from_millis(3));
            assert!(driver.events().is_empty());
            executed += 1;
            let receipt = executor.execute_when(profile, binding, action, || {
                !stop.load(Ordering::Acquire)
                    && receiver.failure().is_none()
                    && policy.admits(admission, action)
            });
            stop.store(true, Ordering::Release);
            receipt
        },
    );
    assert_eq!(executed, 1);
    assert_eq!(rx.try_iter().count(), 1);
    assert!(!driver.events().is_empty());
    assert!(receiver.failure().is_none());
}

#[test]
fn saved_state_roundtrips_and_rejects_unsupported_versions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("six-layer-runtime.json");
    assert!(!load(&path)
        .unwrap()
        .legacy_native_paused_confirmation
        .is_some());
    let state = SavedSixLayerState {
        legacy_native_paused_confirmation: Some(true),
        ..Default::default()
    };
    save(&path, &state).unwrap();
    assert!(load(&path)
        .unwrap()
        .legacy_native_paused_confirmation
        .is_some());
    fs::write(
        &path,
        br#"{"version":2,"cindy_native_paused_confirmed":true,"installation":null}"#,
    )
    .unwrap();
    assert!(load(&path).is_err());
}

#[test]
fn saved_state_migrates_antigravity_v160_top_six_and_mouse_joystick_to_session_and_side_panel() {
    let mut draft = mi_model::draft::Draft::default();
    mi_model::preset::install_agent_starter_profiles(&mut draft).unwrap();
    let ag_pid = draft
        .document
        .editable
        .graph
        .profiles
        .iter()
        .find(|(_, p)| p.name == "Antigravity")
        .unwrap()
        .0
        .clone();
    let desired = BoardLayerPlan::recommended(
        draft
            .document
            .editable
            .graph
            .profiles
            .iter()
            .find(|(_, p)| p.name == "Codex")
            .unwrap()
            .0
            .clone(),
        ag_pid.clone(),
        draft
            .document
            .editable
            .graph
            .profiles
            .iter()
            .find(|(_, p)| p.name == "DeepSeek Coder (DSH)")
            .unwrap()
            .0
            .clone(),
    );
    let mut plan = mi_model::apply::AppliedSixLayerPlan::compile(
        &desired,
        &draft.document,
        mi_model::host_router::BottomRowLayout::Split,
        true,
    )
    .unwrap();
    // Simulate saved #160 Antigravity plan (subagent + mouse-click:left + blanks + MOUSE joystick)
    // while the live document has already migrated to Antigravity session top-6 + logical_sectors.
    let ag_host = plan
        .hosts
        .iter_mut()
        .find(|h| h.profile_id == ag_pid)
        .unwrap();
    ag_host.actions[0] = Some(ResolvedHostAction::AgentCommand {
        adapter: "antigravity".into(),
        command: "subagent".into(),
    });
    ag_host.actions[1] = Some(ResolvedHostAction::AgentCommand {
        adapter: "host".into(),
        command: "mouse-click:left".into(),
    });
    ag_host.actions[2..6].fill(None);
    ag_host.joystick_mode = Some("MOUSE".into());
    ag_host.actions[16..20].fill(None);

    let dir = tempfile::tempdir().unwrap();
    let mut saved = SavedSixLayerState {
        version: 3,
        legacy_native_paused_confirmation: None,
        installation: Some(Installation {
            status: InstallationStatus::Applied,
            target: TargetKeyboard {
                vendor_id: 0x303a,
                product_id: 0x8298,
                location_id: 1,
                usb_parent: 1,
                vendor_path: "synthetic".into(),
            },
            profile_index: 0,
            backup: dir.path().join("backup"),
            config: ConfigPatch {
                path: dir.path().join("karabiner.json"),
                original: vec![],
                installed: vec![],
                profile_name: "synthetic".into(),
                original_device: None,
                installed_device: serde_json::Value::Null,
            },
            plan,
        }),
    };

    assert!(saved.migrate_saved_encoder_defaults(&draft.document));
    // Second call must be idempotent.
    assert!(!saved.migrate_saved_encoder_defaults(&draft.document));

    let migrated_ag = saved
        .installation
        .as_ref()
        .unwrap()
        .plan
        .hosts
        .iter()
        .find(|h| h.profile_id == ag_pid)
        .unwrap();
    for (slot, action) in migrated_ag.actions.iter().enumerate().take(6) {
        assert_eq!(
            *action,
            Some(ResolvedHostAction::AgentCommand {
                adapter: "four-app".into(),
                command: format!("slot-open:{slot}"),
            })
        );
    }
    assert_eq!(migrated_ag.joystick_mode, None);
    assert_eq!(
        migrated_ag.actions[16],
        Some(ResolvedHostAction::AgentCommand {
            adapter: "antigravity".into(),
            command: "scroll-up".into(),
        })
    );
    assert_eq!(
        migrated_ag.actions[17],
        Some(ResolvedHostAction::AgentCommand {
            adapter: "antigravity".into(),
            command: "open-side-panel".into(),
        })
    );
    assert_eq!(
        migrated_ag.actions[18],
        Some(ResolvedHostAction::AgentCommand {
            adapter: "antigravity".into(),
            command: "scroll-down".into(),
        })
    );
    assert_eq!(
        migrated_ag.actions[19],
        Some(ResolvedHostAction::AgentCommand {
            adapter: "antigravity".into(),
            command: "close-side-panel".into(),
        })
    );

    const SIX_SOURCE: &[u8] =
        include_bytes!("../../../mi-model/fixtures/device/synthetic-six-layer-source.json");
    const SMART: &[u8] =
        include_bytes!("../../../mi-model/fixtures/device/synthetic-smart-actions-object.json");
    let migrated_plan = &saved.installation.as_ref().unwrap().plan;
    let old_all_mouse = mi_model::apply::compile_six_layer_profile_for_firmware(
        SIX_SOURCE,
        Some(SMART),
        Some("0.7.0-rc.16"),
        Some("mouse"),
    )
    .unwrap();
    assert!(
        !mi_model::apply::planned_keymap_matches_plan_joystick_modes(
            &old_all_mouse.keymap,
            old_all_mouse.profile_index,
            migrated_plan,
            Some("0.7.0-rc.16"),
        ),
        "old all-MOUSE keymap must fail plan mode match so resume requires explicit apply"
    );
    let recompiled = mi_model::apply::compile_six_layer_profile_for_firmware_layers(
        &old_all_mouse.keymap,
        Some(SMART),
        Some("0.7.0-rc.16"),
        migrated_plan.board_layer_joystick_modes(),
    )
    .unwrap();
    assert!(mi_model::apply::planned_keymap_matches_plan_joystick_modes(
        &recompiled.keymap,
        recompiled.profile_index,
        migrated_plan,
        Some("0.7.0-rc.16"),
    ));
}

#[test]
fn disjoint_pressed_mask_ignores_other_layer_releases_and_codex_layer_never_emits_host_activation()
{
    use mi_proto::Framer;
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (link, _) = mi_transport::MockLink::new();
    let session = {
        let _entered = runtime.enter();
        DeviceSession::new(
            link.clone(),
            Box::new(mi_proto::NativeFramer::default()),
            Arc::new(mi_session::ReadOnlySchema),
            mi_session::SessionConfig::default(),
        )
    };
    let mut running = RunningSixLayer::isolated(session.clone(), dir.path());
    running.receiver.arm().unwrap();
    running.attach_test_vendor_session(&runtime, session);

    let framer = mi_proto::NativeFramer::default();
    for packet in [
        r#"{"m":"v.oai.hid","p":{"k":"AG06","act":1}}"#,
        r#"{"m":"v.oai.hid","p":{"k":"AG12","act":0}}"#,
        r#"{"m":"v.oai.hid","p":{"k":"AG06","act":1}}"#,
        r#"{"m":"v.oai.hid","p":{"k":"AG00","act":0}}"#,
        r#"{"m":"v.oai.hid","p":{"k":"AG00","act":1}}"#,
        r#"{"m":"v.oai.hid","p":{"k":"AG00","act":0}}"#,
        r#"{"m":"v.oai.hid","p":{"k":"AG06","act":0}}"#,
        r#"{"m":"v.oai.hid","p":{"k":"AG06","act":1}}"#,
        r#"{"m":"v.oai.hid","p":{"k":"AG06","act":0}}"#,
    ] {
        let mut line = packet.to_string();
        line.push('\n');
        for frame in framer.encode(line.as_bytes()).unwrap() {
            link.inject.send(frame).unwrap();
        }
    }

    let mut controls = Vec::new();
    let start = Instant::now();
    while controls.len() < 2 && start.elapsed() < Duration::from_secs(2) {
        if let Some(ev) = running.receiver.next().unwrap() {
            controls.push(ev.control);
        } else {
            thread::sleep(Duration::from_millis(5));
        }
    }
    thread::sleep(Duration::from_millis(25));
    while let Some(ev) = running.receiver.next().unwrap() {
        controls.push(ev.control);
    }
    assert_eq!(controls, vec![0, 0]);
    running.shutdown(&runtime).unwrap();
}

#[test]
fn shared_vendor_bridge_rotates_after_shutting_down_seized_session_without_sleep_and_retire_takes_session(
) {
    use mi_proto::Framer;
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let make_rpc_session = |rt: &tokio::runtime::Runtime| {
        let (link, mut writes) = mi_transport::MockLink::new();
        let inject = link.inject.clone();
        let session = {
            let _entered = rt.enter();
            DeviceSession::new(
                link.clone(),
                Box::new(mi_proto::NativeFramer::default()),
                Arc::new(mi_session::ReadOnlySchema),
                mi_session::SessionConfig {
                    timeout: Duration::from_secs(2),
                    interval: Duration::ZERO,
                },
            )
        };
        rt.spawn(async move {
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
                        let result_val = if req["method"].as_str() == Some("v.oai.thstatus") {
                            serde_json::Value::Null
                        } else {
                            serde_json::json!({"version": "0.6.3", "profile_index": 1, "layer_index": 1})
                        };
                        let reply = serde_json::json!({
                            "id": req["id"],
                            "result": result_val
                        });
                        let mut wire = reply.to_string();
                        wire.push('\n');
                        for frame in out_framer.encode(wire.as_bytes()).unwrap() {
                            let _ = inject.send(frame);
                        }
                    }
                }
            });
        (link, session)
    };

    let (link1, session1) = make_rpc_session(&runtime);
    runtime.block_on(async {
        for _ in 0..900 {
            session1
                .request(mi_proto::Method::DeviceStatus, serde_json::Value::Null)
                .await
                .unwrap();
        }
    });
    assert_eq!(session1.request_count(), 900);

    let (link2, session2) = make_rpc_session(&runtime);
    let receiver = Arc::new(UserCommandReceiver::bind().unwrap());
    receiver.arm().unwrap();

    let framer = mi_proto::NativeFramer::default();
    let mut press_line = r#"{"m":"v.oai.hid","p":{"k":"AG06","act":1}}"#.to_string();
    press_line.push('\n');

    let mut bridge = VendorBridge::new(Some(session1.clone()));

    let old_for_check = session1.clone();
    let reopen_session = session2.clone();
    let reopened_flag = Arc::new(AtomicBool::new(false));
    let reopened_flag_inner = Arc::clone(&reopened_flag);
    let rotation_started = Instant::now();

    let routing = Arc::new(Mutex::new(RoutingContext::default()));
    bridge.spawn_with_reopen(
        &runtime,
        Some(session1.clone()),
        Arc::clone(&receiver),
        routing,
        true,
        move || {
            assert!(old_for_check.is_disconnected());
            reopened_flag_inner.store(true, Ordering::Release);
            let next = reopen_session.clone();
            async move { Some(next) }
        },
    );

    while !reopened_flag.load(Ordering::Acquire)
        && rotation_started.elapsed() < Duration::from_secs(2)
    {
        thread::sleep(Duration::from_millis(5));
    }
    assert!(reopened_flag.load(Ordering::Acquire));
    assert!(rotation_started.elapsed() < Duration::from_millis(500));
    assert!(session1.is_disconnected());
    let _ = link1;

    for frame in framer.encode(press_line.as_bytes()).unwrap() {
        link2.inject.send(frame).unwrap();
    }
    let wait_press = Instant::now();
    let mut observed = Vec::new();
    while observed.is_empty() && wait_press.elapsed() < Duration::from_secs(1) {
        while let Some(ev) = receiver.next().unwrap() {
            observed.push(ev.control);
        }
        if observed.is_empty() {
            thread::sleep(Duration::from_millis(5));
        }
    }
    assert!(!observed.is_empty());

    let entries = vec![OaiKeyStatusEntry::new("AG06", 0x22c55e)];
    let _ = bridge.led_tx.send(Some(("digest-backoff".into(), entries)));
    let wait_backoff = Instant::now();
    while bridge.retry_not_before.lock().unwrap().is_none()
        && wait_backoff.elapsed() < Duration::from_secs(1)
    {
        thread::sleep(Duration::from_millis(5));
    }
    assert!(bridge.retry_not_before.lock().unwrap().is_some());
    assert!(!bridge.needs_lighting_sync());

    bridge.retire();
    assert!(bridge.active_session.lock().unwrap().is_none());
    bridge.shutdown(&runtime);
}

#[test]
fn locked_and_manual_pinned_modes_route_layers_two_to_four_and_layer_one_vendor_fallback() {
    use mi_proto::Framer;

    let plan = test_plan();
    let ag_id = plan.hosts[0].profile_id.clone();
    let pinned_actions = plan.hosts[0].actions.clone();
    assert!(pinned_actions[0].is_some());
    assert!(pinned_actions[7].is_some());
    assert!(pinned_actions[15].is_some());
    let expected_slot15_binding = CONTROL_BINDINGS[15].to_string();
    let pinned = PinnedProfilePlan {
        profile_id: ag_id.clone(),
        bundle_id: Some("com.google.antigravity".into()),
        actions: pinned_actions,
        encoder_hold_action: plan.hosts[0].encoder_hold_action.clone(),
    };

    let routing = Arc::new(Mutex::new(RoutingContext::default()));
    {
        let mut ctx = routing.lock().unwrap();
        ctx.set_routing(
            SixLayerRoutingMode::Locked(Box::new(pinned.clone())),
            Some("com.google.antigravity"),
        );
        assert!(!ctx.update(Some("com.openai.codex")));
        assert!(!ctx.update(Some("com.deepseek.dsh")));
        assert_eq!(
            ctx.effective_profile_id(&plan, 1).as_deref(),
            Some(ag_id.as_str())
        );
        assert_eq!(
            ctx.effective_profile_id(&plan, 3).as_deref(),
            Some(ag_id.as_str())
        );
    }

    let receiver = Arc::new(UserCommandReceiver::bind().unwrap());
    receiver.arm().unwrap();
    send(&receiver, 0);
    send(&receiver, 27);
    send(&receiver, 55);
    thread::sleep(Duration::from_millis(30));

    let stop = AtomicBool::new(false);
    let policy = ExecutionPolicy::default();
    policy.set(true, true, Instant::now() - Duration::from_secs(1));
    let observed = AtomicU8::new(1);
    let first = AtomicBool::new(false);
    let (tx, rx) = std::sync::mpsc::sync_channel(8);
    let mut dispatched = Vec::new();

    execute_clicks(
        &receiver,
        &plan,
        &stop,
        &policy,
        &observed,
        &first,
        &routing,
        &tx,
        None,
        || {
            let next = receiver.next();
            if matches!(&next, Ok(None)) {
                stop.store(true, Ordering::Release);
            }
            next
        },
        || Ok(()),
        |profile, binding, _action, _epoch| {
            dispatched.push((profile.to_owned(), binding.to_owned()));
            mi_macos::HostActionReceipt {
                action_type: "open_app".into(),
                binding_id: binding.into(),
                detail: "ok".into(),
                outcome: "executed".into(),
                profile_id: profile.into(),
                target: "com.google.antigravity".into(),
                text_byte_len: None,
                timestamp_ms: 1,
            }
        },
    );

    assert_eq!(
        dispatched,
        vec![
            (ag_id.clone(), "AG00".into()),
            (ag_id.clone(), "AG07".into()),
            (ag_id.clone(), expected_slot15_binding),
        ]
    );
    assert_eq!(rx.try_iter().count(), 3);

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (link, _writes) = mi_transport::MockLink::new();
    let session = {
        let _entered = runtime.enter();
        DeviceSession::new(
            link.clone(),
            Box::new(mi_proto::NativeFramer::default()),
            Arc::new(mi_session::ReadOnlySchema),
            mi_session::SessionConfig {
                timeout: Duration::from_secs(1),
                interval: Duration::ZERO,
            },
        )
    };
    let mut bridge = VendorBridge::new(Some(session.clone()));
    bridge.spawn_with_reopen(
        &runtime,
        Some(session),
        Arc::clone(&receiver),
        Arc::clone(&routing),
        true,
        || async { None },
    );
    let framer = mi_proto::NativeFramer::default();
    let mut oai_press = r#"{"m":"v.oai.hid","p":{"k":"AG00","act":1}}"#.to_string();
    oai_press.push('\n');
    for frame in framer.encode(oai_press.as_bytes()).unwrap() {
        link.inject.send(frame).unwrap();
    }
    let wait = Instant::now();
    let mut vendor_controls = Vec::new();
    while vendor_controls.is_empty() && wait.elapsed() < Duration::from_secs(1) {
        while let Some(ev) = receiver.next().unwrap() {
            vendor_controls.push(ev.control);
        }
        if vendor_controls.is_empty() {
            thread::sleep(Duration::from_millis(5));
        }
    }
    assert_eq!(vendor_controls, vec![0]);
    bridge.retire();
    bridge.shutdown(&runtime);

    routing.lock().unwrap().set_routing(
        SixLayerRoutingMode::ManualPinned(Box::new(pinned)),
        Some("com.deepseek.dsh"),
    );
    send(&receiver, 20);
    thread::sleep(Duration::from_millis(30));
    stop.store(false, Ordering::Release);
    let mut manual_dispatched = Vec::new();
    execute_clicks(
        &receiver,
        &plan,
        &stop,
        &policy,
        &observed,
        &first,
        &routing,
        &tx,
        None,
        || {
            let next = receiver.next();
            if matches!(&next, Ok(None)) {
                stop.store(true, Ordering::Release);
            }
            next
        },
        || Ok(()),
        |profile, binding, _action, _epoch| {
            manual_dispatched.push((profile.to_owned(), binding.to_owned()));
            mi_macos::HostActionReceipt {
                action_type: "open_app".into(),
                binding_id: binding.into(),
                detail: "ok".into(),
                outcome: "executed".into(),
                profile_id: profile.into(),
                target: "com.google.antigravity".into(),
                text_byte_len: None,
                timestamp_ms: 1,
            }
        },
    );
    assert_eq!(manual_dispatched, vec![(ag_id, "AG00".into())]);
}

struct MockVendorLink {
    inner: Arc<mi_transport::MockLink>,
    exclusive: bool,
}

impl mi_transport::HidLink for MockVendorLink {
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

use self::{test_b64_decode as b64_decode, test_b64_encode as b64_encode};

#[test]
fn outdated_snapshot_requires_explicit_apply_and_refuses_external_modifications() {
    const SIX_SOURCE: &[u8] =
        include_bytes!("../../../mi-model/fixtures/device/synthetic-six-layer-source.json");
    const SMART: &[u8] =
        include_bytes!("../../../mi-model/fixtures/device/synthetic-smart-actions-object.json");

    let canonical_template =
        mi_model::apply::compile_six_layer_profile(SIX_SOURCE, Some(SMART)).unwrap();
    let mut outdated_val: serde_json::Value =
        serde_json::from_slice(&canonical_template.keymap).unwrap();
    for layer_idx in 1..6 {
        let layer_obj = outdated_val["profiles"][1]["layers"][layer_idx]
            .as_object_mut()
            .unwrap();
        layer_obj.remove("lights");
        if layer_idx == 1 {
            layer_obj["layout"]["keymap"][0] = serde_json::json!(["KC_F13", "KC_F14"]);
        }
    }
    let raw_doc: std::collections::BTreeMap<&str, &serde_json::value::RawValue> =
        serde_json::from_slice(SIX_SOURCE).unwrap();
    let raw_profiles: Vec<std::collections::BTreeMap<&str, &serde_json::value::RawValue>> =
        serde_json::from_str(raw_doc["profiles"].get()).unwrap();
    let raw_layers: Vec<&serde_json::value::RawValue> =
        serde_json::from_str(raw_profiles[0]["layers"].get()).unwrap();
    let raw_layer0 = raw_layers[0].get();
    let outdated_layers: Vec<String> = (1..6)
        .map(|i| serde_json::to_string(&outdated_val["profiles"][1]["layers"][i]).unwrap())
        .collect();
    let outdated_profile = format!(
        "{{\"id\":1,\"name\":\"{}\",\"layers\":[{},{}]}}",
        mi_model::apply::SIX_LAYER_PROFILE_NAME,
        raw_layer0,
        outdated_layers.join(",")
    );
    let outdated_bytes = std::str::from_utf8(SIX_SOURCE)
        .unwrap()
        .replacen("\"activeProfileId\": 0", "\"activeProfileId\": 1", 1)
        .replacen(
            "  }] }],\n  \"macros\"",
            &format!("  }}] }},{outdated_profile}],\n  \"macros\""),
            1,
        )
        .into_bytes();
    assert!(mi_model::apply::recognize_six_layer_profile(&outdated_bytes, Some(SMART)).is_err());

    let dir = tempfile::tempdir().unwrap();
    let old_backup = dir.path().join("six-layer-backup-old");
    fs::create_dir_all(&old_backup).unwrap();
    fs::write(old_backup.join("keymap.json"), SIX_SOURCE).unwrap();
    fs::write(old_backup.join("planned-keymap.json"), &outdated_bytes).unwrap();
    fs::write(old_backup.join("smart_actions.json"), SMART).unwrap();
    fs::write(
        old_backup.join("manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "format": "micro-input-three-layer-recovery-v1",
            "previous_profile_index": 0,
            "previous_layer_index": 1,
            "planned_profile_index": 1,
            "keymap_sha256": crate::lighting_arbiter::sha256_hex(SIX_SOURCE),
            "planned_keymap_sha256": crate::lighting_arbiter::sha256_hex(&outdated_bytes),
        }))
        .unwrap(),
    )
    .unwrap();

    let device_state = Arc::new(Mutex::new((0u64, outdated_bytes.clone(), 0usize)));
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let open_mock_session = |exclusive: bool| {
        let state = Arc::clone(&device_state);
        let (link, mut requests) = mi_transport::MockLink::new();
        let inject = link.inject.clone();
        let session = {
            let _entered = runtime.enter();
            DeviceSession::new(
                Arc::new(MockVendorLink {
                    inner: link,
                    exclusive,
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
            let mut incoming = mi_proto::NativeFramer::default();
            let outbound = mi_proto::NativeFramer::default();
            let mut staged = Vec::new();
            while let Some(frame) = requests.recv().await {
                let Ok(messages) = incoming.feed(&frame) else {
                    break;
                };
                for bytes in messages {
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
                        "device.status" => {
                            let guard = state.lock().unwrap();
                            serde_json::json!({
                                "version": "0.6.3",
                                "profile_index": guard.0,
                                "layer_index": 2
                            })
                        }
                        "fs.list" => {
                            let guard = state.lock().unwrap();
                            serde_json::json!([
                                meta("keymap.json", &guard.1),
                                meta("smart_actions.json", SMART)
                            ])
                        }
                        "fs.readbin" => {
                            let guard = state.lock().unwrap();
                            let file_bytes = match params["file"].as_str().unwrap() {
                                "keymap.json" => guard.1.as_slice(),
                                "smart_actions.json" => SMART,
                                other => panic!("unexpected file {other}"),
                            };
                            let offset = params["offset"].as_u64().unwrap() as usize;
                            let len = params["len"].as_u64().unwrap() as usize;
                            let end = (offset + len).min(file_bytes.len());
                            serde_json::json!({
                                "total_size": file_bytes.len(),
                                "data": b64_encode(&file_bytes[offset..end])
                            })
                        }
                        "fs.txbegin" => serde_json::json!({"tx": 91}),
                        "fs.writebin" => {
                            staged.extend(b64_decode(params["data"].as_str().unwrap()));
                            serde_json::Value::Null
                        }
                        "fs.txcommit" => {
                            let next_keymap = std::mem::take(&mut staged);
                            let parsed: serde_json::Value =
                                serde_json::from_slice(&next_keymap).unwrap();
                            let mut guard = state.lock().unwrap();
                            guard.0 = parsed["activeProfileId"].as_u64().unwrap();
                            guard.1 = next_keymap;
                            guard.2 += 1;
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
        });
        session
    };

    runtime.block_on(async {
        let mut cindy_val: serde_json::Value =
            serde_json::from_slice(&canonical_template.keymap).unwrap();
        cindy_val["profiles"][1]["layers"][3]["name"] = serde_json::json!("Cindy");
        let cindy_bytes = serde_json::to_vec(&cindy_val).unwrap();
        let cindy_session = open_mock_session(false);
        let cindy_err = inspect_resume_snapshot(&cindy_session, &cindy_bytes, Some(SMART), 1)
            .await
            .unwrap_err();
        assert_eq!(cindy_session.request_count(), 0);
        cindy_session.shutdown().await;
        assert!(
            cindy_err.contains("未写设备，请显式应用或恢复"),
            "{cindy_err}"
        );

        let mismatch_profile_session = open_mock_session(false);
        let mismatch_err =
            inspect_resume_snapshot(&mismatch_profile_session, &outdated_bytes, Some(SMART), 1)
                .await
                .unwrap_err();
        mismatch_profile_session.shutdown().await;
        assert!(
            mismatch_err.contains("六层固件、方案身份或层位未就绪；未写设备，请显式应用或恢复"),
            "{mismatch_err}"
        );
        assert_eq!(device_state.lock().unwrap().2, 0);

        {
            let mut guard = device_state.lock().unwrap();
            guard.0 = 1;
            guard.1 = SIX_SOURCE.to_vec();
        }
        let mismatch_bytes_session = open_mock_session(false);
        let mismatch_bytes_err =
            inspect_resume_snapshot(&mismatch_bytes_session, &outdated_bytes, Some(SMART), 1)
                .await
                .unwrap_err();
        mismatch_bytes_session.shutdown().await;
        assert!(mismatch_bytes_err.contains("旧版六层快照与当前设备不符，未自动覆盖"));
        assert_eq!(device_state.lock().unwrap().2, 0);

        {
            let mut guard = device_state.lock().unwrap();
            guard.0 = 1;
            guard.1 = outdated_bytes.clone();
        }
        let probe_session = open_mock_session(false);
        assert_eq!(
            inspect_resume_snapshot(&probe_session, &outdated_bytes, Some(SMART), 1)
                .await
                .unwrap(),
            ResumeSnapshotAction::RequiresExplicitApply
        );
        probe_session.shutdown().await;
        assert!(probe_session.is_disconnected());
        assert_eq!(device_state.lock().unwrap().2, 0);

        let exclusive_session = open_mock_session(true);
        let prepared = exclusive_session
            .prepare_six_layer_write(dir.path())
            .await
            .unwrap();
        merge_previous_original_into_backup(&old_backup, &prepared.backup).unwrap();
        let upgraded_candidate = prepared.candidate.clone();
        let receipt = exclusive_session
            .apply_six_layer_write(prepared)
            .await
            .unwrap();
        exclusive_session.shutdown().await;
        fs::remove_dir_all(&old_backup).unwrap();

        assert_eq!(device_state.lock().unwrap().2, 1);
        assert_eq!(
            mi_model::apply::recognize_six_layer_profile(&upgraded_candidate, Some(SMART)).unwrap(),
            Some(1)
        );
        assert_eq!(
            fs::read(receipt.backup.join("keymap.json")).unwrap(),
            SIX_SOURCE
        );
        assert_eq!(
            fs::read(receipt.backup.join("planned-keymap.json")).unwrap(),
            upgraded_candidate
        );

        let fast_resume_session = open_mock_session(false);
        assert_eq!(
            inspect_resume_snapshot(&fast_resume_session, &upgraded_candidate, Some(SMART), 1)
                .await
                .unwrap(),
            ResumeSnapshotAction::VerifyReady { layer: 2 }
        );
        fast_resume_session.shutdown().await;
        assert_eq!(device_state.lock().unwrap().2, 1);

        let restore_session = open_mock_session(true);
        restore_session
            .restore_six_layer_backup(&receipt.backup)
            .await
            .unwrap();
        restore_session.shutdown().await;
        assert_eq!(device_state.lock().unwrap().2, 2);
        assert_eq!(device_state.lock().unwrap().1, SIX_SOURCE);
    });
}

#[test]
fn external_edit_mid_write_abort_recovers_factory_original_via_pre_write_backup() {
    const SIX_SOURCE: &[u8] =
        include_bytes!("../../../mi-model/fixtures/device/synthetic-six-layer-source.json");
    const SMART: &[u8] =
        include_bytes!("../../../mi-model/fixtures/device/synthetic-smart-actions-object.json");

    let canonical = mi_model::apply::compile_six_layer_profile(SIX_SOURCE, Some(SMART)).unwrap();
    let mut external_edit_val: serde_json::Value =
        serde_json::from_slice(&canonical.keymap).unwrap();
    external_edit_val["profiles"][1]["layers"][1]["layout"]["keymap"][0][0] =
        serde_json::json!("KC_F13");
    let external_edit_bytes = serde_json::to_vec(&external_edit_val).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let prev_backup = dir.path().join("six-layer-backup-prev");
    fs::create_dir_all(&prev_backup).unwrap();
    fs::write(prev_backup.join("keymap.json"), SIX_SOURCE).unwrap();
    fs::write(prev_backup.join("planned-keymap.json"), &canonical.keymap).unwrap();
    fs::write(prev_backup.join("smart_actions.json"), SMART).unwrap();
    fs::write(
        prev_backup.join("manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "format": "micro-input-three-layer-recovery-v1",
            "previous_profile_index": 0,
            "previous_layer_index": 1,
            "planned_profile_index": 1,
            "keymap_sha256": crate::lighting_arbiter::sha256_hex(SIX_SOURCE),
            "planned_keymap_sha256": crate::lighting_arbiter::sha256_hex(&canonical.keymap),
        }))
        .unwrap(),
    )
    .unwrap();

    let device_state = Arc::new(Mutex::new((1u64, external_edit_bytes.clone(), 0usize)));
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let open_exclusive = || {
        let state = Arc::clone(&device_state);
        let (link, mut requests) = mi_transport::MockLink::new();
        let inject = link.inject.clone();
        let session = {
            let _entered = runtime.enter();
            DeviceSession::new(
                Arc::new(MockVendorLink {
                    inner: link,
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
            let mut incoming = mi_proto::NativeFramer::default();
            let outbound = mi_proto::NativeFramer::default();
            let mut staged = Vec::new();
            while let Some(frame) = requests.recv().await {
                let Ok(messages) = incoming.feed(&frame) else {
                    break;
                };
                for bytes in messages {
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
                        "device.status" => {
                            let guard = state.lock().unwrap();
                            serde_json::json!({
                                "version": "0.6.3",
                                "profile_index": guard.0,
                                "layer_index": 2
                            })
                        }
                        "fs.list" => {
                            let guard = state.lock().unwrap();
                            serde_json::json!([
                                meta("keymap.json", &guard.1),
                                meta("smart_actions.json", SMART)
                            ])
                        }
                        "fs.readbin" => {
                            let guard = state.lock().unwrap();
                            let file_bytes = match params["file"].as_str().unwrap() {
                                "keymap.json" => guard.1.as_slice(),
                                "smart_actions.json" => SMART,
                                other => panic!("unexpected file {other}"),
                            };
                            let offset = params["offset"].as_u64().unwrap() as usize;
                            let len = params["len"].as_u64().unwrap() as usize;
                            let end = (offset + len).min(file_bytes.len());
                            serde_json::json!({
                                "total_size": file_bytes.len(),
                                "data": b64_encode(&file_bytes[offset..end])
                            })
                        }
                        "fs.txbegin" => serde_json::json!({"tx": 99}),
                        "fs.writebin" => {
                            staged.extend(b64_decode(params["data"].as_str().unwrap()));
                            serde_json::Value::Null
                        }
                        "fs.txcommit" => {
                            let next_keymap = std::mem::take(&mut staged);
                            let parsed: serde_json::Value =
                                serde_json::from_slice(&next_keymap).unwrap();
                            let mut guard = state.lock().unwrap();
                            guard.0 = parsed["activeProfileId"].as_u64().unwrap();
                            guard.1 = next_keymap;
                            guard.2 += 1;
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
        });
        session
    };

    runtime.block_on(async {
        let prep_session = open_exclusive();
        let prepared = prep_session
            .prepare_six_layer_write(dir.path())
            .await
            .unwrap();
        merge_previous_original_into_backup(&prev_backup, &prepared.backup).unwrap();
        let aborted_backup = prepared.backup.clone();
        prep_session.shutdown().await;
        drop(prepared);

        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(aborted_backup.join("manifest.json")).unwrap())
                .unwrap();
        assert_eq!(
            manifest["pre_write_keymap_sha256"].as_str(),
            Some(crate::lighting_arbiter::sha256_hex(&external_edit_bytes).as_str())
        );
        assert_eq!(device_state.lock().unwrap().1, external_edit_bytes);

        let restore_session = open_exclusive();
        restore_session
            .restore_six_layer_backup(&aborted_backup)
            .await
            .unwrap();
        restore_session.shutdown().await;
        assert_eq!(device_state.lock().unwrap().1, SIX_SOURCE);
    });
}

#[test]
fn reapply_uses_previous_karabiner_path_and_rejects_profile_name_drift() {
    let dir = tempfile::tempdir().unwrap();
    let karabiner_path = dir.path().join("isolated-karabiner.json");
    let target = TargetKeyboard {
        vendor_id: 0x303a,
        product_id: 0x8298,
        location_id: 0x0110_0000,
        usb_parent: 0x1000_0001,
        vendor_path: "isolated-vendor".into(),
    };
    let receiver = UserCommandReceiver::bind().unwrap();
    let initial_karabiner = serde_json::to_vec_pretty(&serde_json::json!({
            "profiles": [
                {"name": "Default", "selected": true, "complex_modifications": {"rules": []}, "devices": []},
                {"name": "Work", "selected": false, "complex_modifications": {"rules": []}, "devices": []}
            ]
        }))
        .unwrap();
    fs::write(&karabiner_path, &initial_karabiner).unwrap();
    let rule = karabiner::rules(&target, &receiver, BottomRowLayout::Split).unwrap();
    let prev_patch =
        karabiner::prepare_config(karabiner_path.clone(), initial_karabiner, &target, rule)
            .unwrap();
    fs::write(&karabiner_path, &prev_patch.installed).unwrap();

    let saved = SavedSixLayerState {
        installation: Some(Installation {
            status: InstallationStatus::Applied,
            plan: test_plan(),
            target: target.clone(),
            profile_index: 1,
            backup: dir.path().join("backup"),
            config: prev_patch,
        }),
        ..SavedSixLayerState::default()
    };

    let same_profile =
        prepare_apply_karabiner_patch(&saved, &target, &receiver, BottomRowLayout::Split).unwrap();
    assert_eq!(same_profile.path, karabiner_path);
    assert_eq!(same_profile.profile_name, "Default");

    let drifted_karabiner = serde_json::to_vec_pretty(&serde_json::json!({
            "profiles": [
                {"name": "Default", "selected": false, "complex_modifications": {"rules": []}, "devices": []},
                {"name": "Work", "selected": true, "complex_modifications": {"rules": []}, "devices": []}
            ]
        }))
        .unwrap();
    fs::write(&karabiner_path, &drifted_karabiner).unwrap();
    let err = prepare_apply_karabiner_patch(&saved, &target, &receiver, BottomRowLayout::Split)
        .unwrap_err();
    assert_eq!(
        err,
        "原 Karabiner 方案身份已变化；请先恢复原键盘配置后再应用"
    );
}

fn make_firmware_rpc_session(
    rt: &tokio::runtime::Runtime,
    firmware: &'static str,
) -> Arc<DeviceSession> {
    use mi_proto::Framer;
    let (link, mut writes) = mi_transport::MockLink::new();
    let inject = link.inject.clone();
    let session = {
        let _entered = rt.enter();
        DeviceSession::new(
            link,
            Box::new(mi_proto::NativeFramer::default()),
            Arc::new(mi_session::ReadOnlySchema),
            mi_session::SessionConfig {
                timeout: Duration::from_secs(2),
                interval: Duration::ZERO,
            },
        )
    };
    rt.spawn(async move {
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
                let result_val = if req["method"].as_str() == Some("v.oai.thstatus") {
                    serde_json::Value::Null
                } else {
                    serde_json::json!({
                        "version": firmware,
                        "profile_index": 1,
                        "layer_index": 1
                    })
                };
                let reply = serde_json::json!({
                    "id": req["id"],
                    "result": result_val
                });
                let mut wire = reply.to_string();
                wire.push('\n');
                for frame in out_framer.encode(wire.as_bytes()).unwrap() {
                    let _ = inject.send(frame);
                }
            }
        }
    });
    session
}

#[test]
fn observed_firmware_version_survives_vendor_bridge_clear_and_rotation() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();

    let session1 = make_firmware_rpc_session(&runtime, "0.7.0-rc.16");
    runtime.block_on(async {
        let info = session1.read_identity_info().await.unwrap();
        assert_eq!(info.version.as_deref(), Some("0.7.0-rc.16"));
        for _ in 0..899 {
            session1
                .request(mi_proto::Method::DeviceStatus, serde_json::Value::Null)
                .await
                .unwrap();
        }
    });
    assert_eq!(session1.request_count(), 900);
    assert_eq!(
        session1.observed_firmware_version().as_deref(),
        Some("0.7.0-rc.16")
    );

    let mut running = RunningSixLayer::isolated(session1.clone(), dir.path());
    assert_eq!(
        running.observed_firmware_version().as_deref(),
        Some("0.7.0-rc.16")
    );

    let _cleared = running.vendor_bridge.active_session.lock().unwrap().take();
    assert_eq!(
        running.observed_firmware_version().as_deref(),
        Some("0.7.0-rc.16")
    );

    let session2 = make_firmware_rpc_session(&runtime, "0.7.0-rc.16");
    assert_eq!(session2.observed_firmware_version(), None);
    let reopened = Arc::new(AtomicBool::new(false));
    let reopened_flag = Arc::clone(&reopened);
    let next_session = session2.clone();
    running.vendor_bridge.spawn_with_reopen(
        &runtime,
        Some(session1.clone()),
        Arc::clone(&running.receiver),
        Arc::clone(&running.routing),
        true,
        move || {
            reopened_flag.store(true, Ordering::Release);
            let rotated = next_session.clone();
            async move { Some(rotated) }
        },
    );

    let deadline = Instant::now() + Duration::from_secs(2);
    while (!reopened.load(Ordering::Acquire)
        || !running
            .vendor_bridge
            .active_session
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|s| Arc::ptr_eq(s, &session2)))
        && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(5));
    }
    assert!(reopened.load(Ordering::Acquire));
    assert!(session1.is_disconnected());
    assert_eq!(session2.observed_firmware_version(), None);
    assert_eq!(
        running.observed_firmware_version().as_deref(),
        Some("0.7.0-rc.16")
    );
    running.shutdown(&runtime).unwrap();

    let blank_session = make_firmware_rpc_session(&runtime, "0.6.3");
    let observed_session = make_firmware_rpc_session(&runtime, "0.6.3");
    runtime.block_on(async {
        observed_session.read_identity_info().await.unwrap();
    });
    let mut attached = RunningSixLayer::isolated(blank_session, dir.path());
    assert_eq!(attached.observed_firmware_version(), None);
    attached.attach_test_vendor_session(&runtime, observed_session);
    assert_eq!(
        attached.observed_firmware_version().as_deref(),
        Some("0.6.3")
    );
    let _ = attached.vendor_bridge.active_session.lock().unwrap().take();
    assert_eq!(
        attached.observed_firmware_version().as_deref(),
        Some("0.6.3")
    );
    attached.shutdown(&runtime).unwrap();
}

#[test]
fn observed_firmware_version_clears_immediately_on_retire_or_invalidation() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();

    let session_retire = make_firmware_rpc_session(&runtime, "0.7.0-rc.16");
    runtime.block_on(async {
        session_retire.read_identity_info().await.unwrap();
    });
    let running_retire = RunningSixLayer::isolated(session_retire, dir.path());
    assert_eq!(
        running_retire.observed_firmware_version().as_deref(),
        Some("0.7.0-rc.16")
    );
    running_retire.retire();
    assert!(running_retire.is_retired());
    assert_eq!(running_retire.observed_firmware_version(), None);
    running_retire.shutdown(&runtime).unwrap();

    let session_invalidate = make_firmware_rpc_session(&runtime, "0.6.3");
    runtime.block_on(async {
        session_invalidate.read_identity_info().await.unwrap();
    });
    let running_invalidate = RunningSixLayer::isolated(session_invalidate, dir.path());
    assert_eq!(
        running_invalidate.observed_firmware_version().as_deref(),
        Some("0.6.3")
    );
    running_invalidate.receiver.invalidate("USB removed");
    assert!(!running_invalidate.is_retired());
    assert_eq!(running_invalidate.observed_firmware_version(), None);
    running_invalidate.shutdown(&runtime).unwrap();
}

#[test]
fn v070_profile_layer_outcome_retains_existing_profile_when_omitted() {
    let observed_layer = AtomicU8::new(1);
    let observed_pl = Mutex::new(Some((3_u8, 1_u8)));

    record_v070_bridge_observation(
        &crate::v070_hardware::V070BridgeOutcome::ProfileLayer {
            profile_index: None,
            layer_index: 4,
        },
        &observed_layer,
        &observed_pl,
    );
    assert_eq!(observed_layer.load(Ordering::Acquire), 4);
    assert_eq!(*observed_pl.lock().unwrap(), Some((3, 4)));

    record_v070_bridge_observation(
        &crate::v070_hardware::V070BridgeOutcome::ProfileLayer {
            profile_index: Some(2),
            layer_index: 5,
        },
        &observed_layer,
        &observed_pl,
    );
    assert_eq!(observed_layer.load(Ordering::Acquire), 5);
    assert_eq!(*observed_pl.lock().unwrap(), Some((2, 5)));
}

#[test]
fn interleaved_karabiner_l2_and_vendor_l1_and_expired_l1_never_misroute() {
    let plan = test_plan();
    let ag_id = plan.hosts[0].profile_id.clone();
    let custom_codex = PinnedProfilePlan {
        profile_id: "local-codex".into(),
        bundle_id: Some("com.openai.codex".into()),
        actions: std::array::from_fn(|i| {
            (i == 0).then(|| ResolvedHostAction::OpenApp {
                bundle_id: "com.openai.codex".into(),
            })
        }),
        encoder_hold_action: None,
    };
    let routing = Mutex::new(RoutingContext {
        mode: SixLayerRoutingMode::BoardLayers {
            layer_one: Some(Box::new(custom_codex)),
            layer_two: None,
            layer_three: None,
        },
        bundle_id: None,
    });

    let receiver = UserCommandReceiver::bind().unwrap();
    receiver.arm().unwrap();
    assert!(!receiver.push_layer1_vendor_activation(20));

    let mut received_events = Vec::new();
    send(&receiver, 0);
    let deadline = Instant::now() + Duration::from_secs(2);
    while received_events.is_empty() && Instant::now() < deadline {
        if let Some(ev) = receiver.next().unwrap() {
            received_events.push(ev);
        } else {
            thread::sleep(Duration::from_millis(2));
        }
    }
    assert_eq!(received_events.len(), 1);
    assert_eq!(received_events[0].control, 0);
    assert_eq!(received_events[0].layer_override, None);

    let mut mask = 0u64;
    let ag00 = mi_session::ReceivedNotification {
        ingress_sequence: 1,
        notification: mi_proto::HostNotification::Unknown(
            serde_json::from_str(r#"{"m":"v.oai.hid","p":{"k":"AG00","act":1}}"#).unwrap(),
        ),
    };
    handle_vendor_slot_notification(&ag00, &mut mask, &receiver, true);
    let l1_event = receiver
        .next()
        .unwrap()
        .expect("AG00 vendor notification must queue L1 activation");
    assert_eq!(l1_event.control, 0);
    assert_eq!(l1_event.layer_override, Some(1));

    let now = Instant::now();
    let policy = ExecutionPolicy::default();
    policy.set(true, true, now);
    let expired_l1 = mi_transport::Activation {
        control: 0,
        layer_override: Some(1),
        released: false,
        generation: receiver.generation().into(),
        received_sequence: 3,
        received_at: now - Duration::from_secs(1),
    };
    let fresh_l2 = mi_transport::Activation {
        control: 0,
        layer_override: None,
        released: false,
        generation: receiver.generation().into(),
        received_sequence: 4,
        received_at: now + Duration::from_millis(1),
    };
    let mut queue = std::collections::VecDeque::from([
        mi_transport::Activation {
            received_at: now,
            ..received_events.remove(0)
        },
        mi_transport::Activation {
            received_at: now,
            ..l1_event
        },
        expired_l1,
        fresh_l2,
    ]);

    let stop = AtomicBool::new(false);
    let observed = AtomicU8::new(1);
    let first = AtomicBool::new(false);
    let (tx, _rx) = std::sync::mpsc::sync_channel(8);
    let mut calls = Vec::new();
    execute_clicks(
        &receiver,
        &plan,
        &stop,
        &policy,
        &observed,
        &first,
        &routing,
        &tx,
        None,
        || match queue.pop_front() {
            Some(ev) => Ok(Some(ev)),
            None => {
                stop.store(true, Ordering::Release);
                Ok(None)
            }
        },
        || Ok(()),
        |profile, binding, _action, _admission| {
            calls.push((profile.to_owned(), binding.to_owned()));
            mi_macos::HostActionReceipt {
                action_type: "open_app".into(),
                binding_id: binding.into(),
                detail: "ok".into(),
                outcome: "executed".into(),
                profile_id: profile.into(),
                target: "test".into(),
                text_byte_len: None,
                timestamp_ms: 1,
            }
        },
    );
    assert_eq!(
        calls,
        vec![
            (ag_id.clone(), "AG00".into()),
            ("local-codex".into(), "AG00".into()),
            (ag_id, "AG00".into()),
        ]
    );
}

#[test]
fn vendor_pressed_mask_keeps_ag00_ag06_act06_and_radial_disjoint() {
    let receiver = UserCommandReceiver::bind().unwrap();
    receiver.arm().unwrap();
    let mut mask = 0u64;
    let mut send_vendor = |json: &str| {
        let received = mi_session::ReceivedNotification {
            ingress_sequence: 1,
            notification: mi_proto::HostNotification::Unknown(serde_json::from_str(json).unwrap()),
        };
        handle_vendor_slot_notification(&received, &mut mask, &receiver, true);
    };

    send_vendor(r#"{"m":"v.oai.hid","p":{"k":"AG00","act":1}}"#);
    send_vendor(r#"{"m":"v.oai.hid","p":{"k":"AG06","act":1}}"#);
    send_vendor(r#"{"m":"v.oai.hid","p":{"k":"ACT06","act":1}}"#);
    send_vendor(r#"{"m":"v.oai.hid","p":{"k":"AG06","act":0}}"#);
    send_vendor(r#"{"m":"v.oai.hid","p":{"k":"AG00","act":1}}"#);
    send_vendor(r#"{"m":"v.oai.hid","p":{"k":"AG00","act":0}}"#);
    send_vendor(r#"{"m":"v.oai.hid","p":{"k":"AG00","act":1}}"#);
    send_vendor(r#"{"m":"v.oai.rad","p":{"a":0.75,"d":0.8}}"#);
    send_vendor(r#"{"m":"v.oai.rad","p":{"a":0.25,"d":0.8}}"#);
    send_vendor(r#"{"m":"v.oai.rad","p":{"a":0.25,"d":0.1}}"#);
    send_vendor(r#"{"m":"v.oai.rad","p":{"a":0.25,"d":0.8}}"#);

    let mut events = Vec::new();
    while let Some(ev) = receiver.next().unwrap() {
        events.push((ev.control, ev.layer_override));
    }
    assert_eq!(
        events,
        vec![
            (0, Some(1)),
            (0, None),
            (6, Some(1)),
            (0, Some(1)),
            (16, Some(1)),
            (18, Some(1)),
        ]
    );
}

#[test]
fn enc_clk_press_and_release_edges_emit_input_events_across_l1_vendor_and_l2_l3_karabiner() {
    let receiver = UserCommandReceiver::bind().unwrap();
    receiver.arm().unwrap();
    let mut mask = 0u64;
    let send_vendor = |raw: &str, mask: &mut u64| {
        let received = mi_session::ReceivedNotification {
            ingress_sequence: 1,
            notification: mi_proto::HostNotification::Unknown(serde_json::from_str(raw).unwrap()),
        };
        handle_vendor_slot_notification(&received, mask, &receiver, true);
    };

    // Layer 1 Vendor: ENC_CLK press (act: 1) -> release (act: 0), duplicate release ignored.
    send_vendor(
        r#"{"m":"v.oai.hid","p":{"k":"ENC_CLK","act":1}}"#,
        &mut mask,
    );
    send_vendor(
        r#"{"m":"v.oai.hid","p":{"k":"ENC_CLK","act":0}}"#,
        &mut mask,
    );
    send_vendor(
        r#"{"m":"v.oai.hid","p":{"k":"ENC_CLK","act":0}}"#,
        &mut mask,
    );

    let l1_press = receiver.next().unwrap().expect("L1 ENC_CLK press");
    let l1_release = receiver.next().unwrap().expect("L1 ENC_CLK release");
    assert!(receiver.next().unwrap().is_none());

    let l1_press_input = activation_to_input_event(&l1_press, 7).unwrap();
    let l1_release_input = activation_to_input_event(&l1_release, 7).unwrap();
    assert_eq!(l1_press_input.binding_id.as_deref(), Some("ENC_CLK"));
    assert_eq!(l1_press_input.board_layer, Some(1));
    assert_eq!(l1_press_input.kind, mi_session::InputEventKind::Pressed);
    assert_eq!(l1_release_input.binding_id.as_deref(), Some("ENC_CLK"));
    assert_eq!(l1_release_input.board_layer, Some(1));
    assert_eq!(l1_release_input.kind, mi_session::InputEventKind::Released);

    // Layers 2/3/4 Karabiner direct controls (15, 35, 55): press + to_after_key_up release.
    for &(control_id, expected_layer) in &[(15u8, 2u8), (35u8, 3u8), (55u8, 4u8)] {
        let sock = std::os::unix::net::UnixDatagram::unbound().unwrap();
        let press_payload = serde_json::to_vec(&mi_transport::ActivationPayload::new(
            receiver.generation(),
            control_id,
        ))
        .unwrap();
        let release_payload = serde_json::to_vec(&mi_transport::ActivationPayload::new_release(
            receiver.generation(),
            control_id,
        ))
        .unwrap();
        sock.send_to(&press_payload, receiver.path()).unwrap();
        sock.send_to(&release_payload, receiver.path()).unwrap();

        let wait_next = |label: &str| {
            let deadline = Instant::now() + Duration::from_millis(500);
            loop {
                if let Some(ev) = receiver.next().unwrap() {
                    return ev;
                }
                assert!(Instant::now() < deadline, "timed out waiting for {label}");
                thread::sleep(Duration::from_millis(2));
            }
        };

        let press = wait_next("Karabiner ENC_CLK press");
        let release = wait_next("Karabiner ENC_CLK release");
        assert!(!press.released);
        assert!(release.released);
        let press_ev = activation_to_input_event(&press, 9).unwrap();
        let release_ev = activation_to_input_event(&release, 9).unwrap();
        assert_eq!(press_ev.binding_id.as_deref(), Some("ENC_CLK"));
        assert_eq!(press_ev.board_layer, Some(expected_layer));
        assert_eq!(press_ev.kind, mi_session::InputEventKind::Pressed);
        assert_eq!(release_ev.binding_id.as_deref(), Some("ENC_CLK"));
        assert_eq!(release_ev.board_layer, Some(expected_layer));
        assert_eq!(release_ev.kind, mi_session::InputEventKind::Released);
    }
}

#[test]
fn six_layer_encoder_hold_opens_model_picker_navigates_flicks_and_confirms_without_short_press_leak(
) {
    let receiver = UserCommandReceiver::bind().unwrap();
    let mut plan = test_plan();
    // Configure a normal short-press action on L2 ENC_CLK (slot 15) to prove hold never leaks short-press,
    // and leave AG18 (slot 19) unassigned so this test isolates captures_sectors == false vs assigned controls.
    plan.hosts[0].actions[15] = Some(ResolvedHostAction::AgentCommand {
        adapter: "antigravity".into(),
        command: "submit".into(),
    });
    plan.hosts[0].actions[19] = None;
    plan.hosts[0].encoder_hold_action = Some(ResolvedHostAction::AgentCommand {
        adapter: "antigravity".into(),
        command: "model-picker-open".into(),
    });
    let ag_profile_id = plan.hosts[0].profile_id.clone();

    let base = Instant::now() - Duration::from_millis(100);
    let policy = ExecutionPolicy::default();
    policy.set(true, true, base - Duration::from_secs(1));
    let routing = Mutex::new(RoutingContext::default());
    let observed = AtomicU8::new(1);
    let first = AtomicBool::new(false);
    let stop = AtomicBool::new(false);
    let (tx, _rx) = std::sync::mpsc::sync_channel(16);

    let gen: String = receiver.generation().into();
    let mk_ev = |seq: u64, control: u8, released: bool, at: Instant| mi_transport::Activation {
        control,
        layer_override: None,
        released,
        generation: gen.clone(),
        received_sequence: seq,
        received_at: at,
    };

    // Sequence:
    // 1) Hold ENC_CLK (control 15 on L2) >= 350ms -> opens model picker (zero short-press leak)
    // 2) Turn CW (control 14) -> model-picker-next
    // 3) Radial East AG18 (control 19) -> model-picker-effort-right
    // 4) Horizontal mouse flick left (-34, 2) -> model-picker-effort-left
    // 5) Tap ENC_CLK (< 350ms) -> model-picker-confirm (exits active picker)
    // 6) Subsequent Tap ENC_CLK (< 350ms) -> normal short-press ("submit")
    let mut steps = std::collections::VecDeque::from([
        Some(mk_ev(1, 15, false, base)),
        Some(mk_ev(2, 15, true, base + Duration::from_millis(380))),
        Some(mk_ev(3, 14, false, base + Duration::from_millis(400))),
        Some(mk_ev(4, 19, false, base + Duration::from_millis(420))),
        None, // allow loop iteration to drain queued mouse delta while picker is active
        Some(mk_ev(5, 15, false, base + Duration::from_millis(450))),
        Some(mk_ev(6, 15, true, base + Duration::from_millis(520))),
        Some(mk_ev(7, 15, false, base + Duration::from_millis(560))),
        Some(mk_ev(8, 15, true, base + Duration::from_millis(620))),
    ]);

    let mut capture_transitions = Vec::new();
    let ready_for_flick = AtomicBool::new(false);
    let mut injected_flick = false;
    let mut dispatched = Vec::new();

    execute_clicks_with_encoder_picker(
        &receiver,
        &plan,
        &stop,
        &policy,
        &observed,
        &first,
        &routing,
        &tx,
        None,
        || match steps.pop_front() {
            Some(Some(ev)) => Ok(Some(ev)),
            Some(None) => {
                ready_for_flick.store(true, Ordering::Release);
                Ok(None)
            }
            None => {
                stop.store(true, Ordering::Release);
                Ok(None)
            }
        },
        || Ok(()),
        |enabled| capture_transitions.push(enabled),
        || {
            if ready_for_flick.load(Ordering::Acquire) && !injected_flick {
                injected_flick = true;
                vec![(-34.0, 2.0)]
            } else {
                Vec::new()
            }
        },
        |profile, binding, action, _admission| {
            let cmd = match action {
                ResolvedHostAction::AgentCommand { adapter, command } => {
                    format!("{adapter}:{command}")
                }
                other => format!("{other:?}"),
            };
            dispatched.push((profile.to_owned(), binding.to_owned(), cmd.clone()));
            mi_macos::HostActionReceipt {
                action_type: "agent_command".into(),
                binding_id: binding.into(),
                detail: cmd,
                outcome: "executed".into(),
                profile_id: profile.into(),
                target: "antigravity".into(),
                text_byte_len: None,
                timestamp_ms: 1,
            }
        },
    );

    assert_eq!(
        dispatched,
        vec![
            (
                ag_profile_id.clone(),
                "ENC_CLK".into(),
                "antigravity:model-picker-open".into(),
            ),
            (
                ag_profile_id.clone(),
                "AG14".into(),
                "antigravity:model-picker-next".into(),
            ),
            (
                ag_profile_id.clone(),
                "ENC_CLK".into(),
                "antigravity:model-picker-confirm".into(),
            ),
            (ag_profile_id, "ENC_CLK".into(), "antigravity:submit".into(),),
        ]
    );
    assert!(capture_transitions.is_empty());
}

#[test]
fn six_layer_custom_and_unassigned_encoder_hold_and_live_board_layer_override() {
    let receiver = UserCommandReceiver::bind().unwrap();
    receiver.arm().unwrap();
    let mut stale_plan = test_plan();
    // Simulate a stale saved six-layer-runtime.json where L2 had no encoder_hold_action
    // and slot 15 had an old action.
    stale_plan.hosts[0].encoder_hold_action = None;
    stale_plan.hosts[0].actions[15] = Some(ResolvedHostAction::OpenUrl {
        url: "https://stale.example/slot15".into(),
    });

    let base = Instant::now() - Duration::from_millis(100);
    let policy = ExecutionPolicy::default();
    policy.set(true, true, base - Duration::from_secs(1));
    let gen: String = receiver.generation().into();
    let mk_ev = |seq: u64, control: u8, released: bool, at: Instant| mi_transport::Activation {
        control,
        layer_override: None,
        released,
        generation: gen.clone(),
        received_sequence: seq,
        received_at: at,
    };

    // Case 1: Unassigned EncoderHold on live L2 -> ENC_CLK fires slot 15 immediately on Pressed
    // with zero release delay and zero ModelPickerOpen.
    let mut l2_unassigned_actions: [Option<ResolvedHostAction>; SLOTS_PER_LAYER] =
        std::array::from_fn(|_| None);
    l2_unassigned_actions[15] = Some(ResolvedHostAction::OpenUrl {
        url: "https://live.example/immediate-short".into(),
    });
    let l2_unassigned = PinnedProfilePlan {
        profile_id: "live-ag".into(),
        bundle_id: Some("com.google.antigravity".into()),
        actions: l2_unassigned_actions,
        encoder_hold_action: None,
    };
    let routing = Mutex::new(RoutingContext::default());
    routing.lock().unwrap().set_routing(
        SixLayerRoutingMode::BoardLayers {
            layer_one: None,
            layer_two: Some(Box::new(l2_unassigned)),
            layer_three: None,
        },
        None,
    );
    let mut steps = std::collections::VecDeque::from([
        // Only Pressed (no Released yet) -> must execute slot 15 immediately!
        Some(mk_ev(1, 15, false, base)),
        // Subsequent Released at +500ms -> must be ignored without firing ModelPickerOpen or double-firing slot 15!
        Some(mk_ev(2, 15, true, base + Duration::from_millis(500))),
    ]);
    let stop = AtomicBool::new(false);
    let (tx, _rx) = std::sync::mpsc::sync_channel(16);
    let mut dispatched = Vec::new();
    let mut capture_transitions = Vec::new();
    execute_clicks_with_encoder_picker(
        &receiver,
        &stale_plan,
        &stop,
        &policy,
        &AtomicU8::new(1),
        &AtomicBool::new(false),
        &routing,
        &tx,
        None,
        || match steps.pop_front() {
            Some(Some(ev)) => Ok(Some(ev)),
            _ => {
                stop.store(true, Ordering::Release);
                Ok(None)
            }
        },
        || Ok(()),
        |enabled| capture_transitions.push(enabled),
        Vec::new,
        |profile, binding, action, _| {
            dispatched.push((profile.to_owned(), binding.to_owned(), action.clone()));
            mi_macos::HostActionReceipt {
                action_type: "url".into(),
                binding_id: binding.into(),
                detail: format!("{action:?}"),
                outcome: "executed".into(),
                profile_id: profile.into(),
                target: "host".into(),
                text_byte_len: None,
                timestamp_ms: 1,
            }
        },
    );
    assert_eq!(
        dispatched,
        vec![(
            "live-ag".into(),
            "ENC_CLK".into(),
            ResolvedHostAction::OpenUrl {
                url: "https://live.example/immediate-short".into(),
            },
        )]
    );
    assert!(capture_transitions.is_empty());

    // Case 2: Custom non-picker EncoderHold on a Locked non-Agent profile -> fires custom action
    // at >= 350ms without entering model picker mode or capturing mouse, and consumes Released.
    let mut custom_actions: [Option<ResolvedHostAction>; SLOTS_PER_LAYER] =
        std::array::from_fn(|_| None);
    custom_actions[14] = Some(ResolvedHostAction::OpenUrl {
        url: "https://custom.example/cw".into(),
    });
    custom_actions[15] = Some(ResolvedHostAction::OpenUrl {
        url: "https://custom.example/short".into(),
    });
    let locked_custom = PinnedProfilePlan {
        profile_id: "custom-figma".into(),
        bundle_id: None,
        actions: custom_actions,
        encoder_hold_action: Some(ResolvedHostAction::OpenUrl {
            url: "https://custom.example/hold".into(),
        }),
    };
    assert!(locked_custom.has_any_action());
    routing.lock().unwrap().set_routing(
        SixLayerRoutingMode::Locked(Box::new(locked_custom)),
        Some("com.figma.Desktop"),
    );
    let mut steps2 = std::collections::VecDeque::from([
        Some(mk_ev(10, 15, false, base)),
        Some(mk_ev(11, 15, true, base + Duration::from_millis(400))),
        // Subsequent CW turn must use normal slot 14 (NOT ModelPickerNext)!
        Some(mk_ev(12, 14, false, base + Duration::from_millis(430))),
    ]);
    let stop2 = AtomicBool::new(false);
    let mut dispatched2 = Vec::new();
    let mut capture2 = Vec::new();
    execute_clicks_with_encoder_picker(
        &receiver,
        &stale_plan,
        &stop2,
        &policy,
        &AtomicU8::new(1),
        &AtomicBool::new(false),
        &routing,
        &tx,
        None,
        || match steps2.pop_front() {
            Some(Some(ev)) => Ok(Some(ev)),
            _ => {
                stop2.store(true, Ordering::Release);
                Ok(None)
            }
        },
        || Ok(()),
        |enabled| capture2.push(enabled),
        Vec::new,
        |profile, binding, action, _| {
            dispatched2.push((profile.to_owned(), binding.to_owned(), action.clone()));
            mi_macos::HostActionReceipt {
                action_type: "url".into(),
                binding_id: binding.into(),
                detail: format!("{action:?}"),
                outcome: "executed".into(),
                profile_id: profile.into(),
                target: "host".into(),
                text_byte_len: None,
                timestamp_ms: 1,
            }
        },
    );
    assert_eq!(
        dispatched2,
        vec![
            (
                "custom-figma".into(),
                "ENC_CLK".into(),
                ResolvedHostAction::OpenUrl {
                    url: "https://custom.example/hold".into(),
                },
            ),
            (
                "custom-figma".into(),
                "AG14".into(),
                ResolvedHostAction::OpenUrl {
                    url: "https://custom.example/cw".into(),
                },
            ),
        ]
    );
    assert!(capture2.is_empty());
}

#[test]
fn six_layer_v090_short_press_enc_clk_opens_model_picker_and_second_short_press_confirms() {
    let receiver = UserCommandReceiver::bind().unwrap();
    let plan = test_plan();
    let ag_profile_id = plan.hosts[0].profile_id.clone();

    let base = Instant::now() - Duration::from_millis(100);
    let policy = ExecutionPolicy::default();
    policy.set(true, true, base - Duration::from_secs(1));
    let routing = Mutex::new(RoutingContext::default());
    let observed = AtomicU8::new(1);
    let first = AtomicBool::new(false);
    let stop = AtomicBool::new(false);
    let (tx, _rx) = std::sync::mpsc::sync_channel(16);

    let gen: String = receiver.generation().into();
    let mk_ev = |seq: u64, control: u8, released: bool, at: Instant| mi_transport::Activation {
        control,
        layer_override: None,
        released,
        generation: gen.clone(),
        received_sequence: seq,
        received_at: at,
    };

    let mut steps = std::collections::VecDeque::from([
        Some(mk_ev(1, 15, false, base)),
        Some(mk_ev(2, 15, true, base + Duration::from_millis(50))),
        Some(mk_ev(3, 14, false, base + Duration::from_millis(100))),
        Some(mk_ev(4, 15, false, base + Duration::from_millis(150))),
        Some(mk_ev(5, 15, true, base + Duration::from_millis(200))),
    ]);

    let mut capture_transitions = Vec::new();
    let mut dispatched = Vec::new();

    execute_clicks_with_encoder_picker(
        &receiver,
        &plan,
        &stop,
        &policy,
        &observed,
        &first,
        &routing,
        &tx,
        None,
        || match steps.pop_front() {
            Some(Some(ev)) => Ok(Some(ev)),
            Some(None) => Ok(None),
            None => {
                stop.store(true, Ordering::Release);
                Ok(None)
            }
        },
        || Ok(()),
        |enabled| capture_transitions.push(enabled),
        Vec::new,
        |profile, binding, action, _admission| {
            let cmd = match action {
                ResolvedHostAction::AgentCommand { adapter, command } => {
                    format!("{adapter}:{command}")
                }
                other => format!("{other:?}"),
            };
            dispatched.push((profile.to_owned(), binding.to_owned(), cmd.clone()));
            mi_macos::HostActionReceipt {
                action_type: "agent_command".into(),
                binding_id: binding.into(),
                detail: cmd,
                outcome: "executed".into(),
                profile_id: profile.into(),
                target: "antigravity".into(),
                text_byte_len: None,
                timestamp_ms: 1,
            }
        },
    );

    assert_eq!(
        dispatched,
        vec![
            (
                ag_profile_id.clone(),
                "ENC_CLK".into(),
                "antigravity:search".into(),
            ),
            (
                ag_profile_id.clone(),
                "AG14".into(),
                "antigravity:search-next".into(),
            ),
            (
                ag_profile_id,
                "ENC_CLK".into(),
                "antigravity:search-confirm".into(),
            ),
        ]
    );
    assert!(!capture_transitions.contains(&true));
}

#[test]
fn six_layer_v090_long_hold_enc_clk_opens_search_and_second_short_press_confirms() {
    let receiver = UserCommandReceiver::bind().unwrap();
    let plan = test_plan();
    let ag_profile_id = plan.hosts[0].profile_id.clone();

    let base = Instant::now() - Duration::from_millis(100);
    let policy = ExecutionPolicy::default();
    policy.set(true, true, base - Duration::from_secs(1));
    let routing = Mutex::new(RoutingContext::default());
    let observed = AtomicU8::new(1);
    let first = AtomicBool::new(false);
    let stop = AtomicBool::new(false);
    let (tx, _rx) = std::sync::mpsc::sync_channel(16);

    let gen: String = receiver.generation().into();
    let mk_ev = |seq: u64, control: u8, released: bool, at: Instant| mi_transport::Activation {
        control,
        layer_override: None,
        released,
        generation: gen.clone(),
        received_sequence: seq,
        received_at: at,
    };

    let mut steps = std::collections::VecDeque::from([
        Some(mk_ev(1, 15, false, base)),
        Some(mk_ev(2, 15, true, base + Duration::from_millis(380))),
        Some(mk_ev(3, 14, false, base + Duration::from_millis(420))),
        Some(mk_ev(4, 15, false, base + Duration::from_millis(460))),
        Some(mk_ev(5, 15, true, base + Duration::from_millis(500))),
    ]);

    let mut capture_transitions = Vec::new();
    let mut dispatched = Vec::new();

    execute_clicks_with_encoder_picker(
        &receiver,
        &plan,
        &stop,
        &policy,
        &observed,
        &first,
        &routing,
        &tx,
        None,
        || match steps.pop_front() {
            Some(Some(ev)) => Ok(Some(ev)),
            Some(None) => Ok(None),
            None => {
                stop.store(true, Ordering::Release);
                Ok(None)
            }
        },
        || Ok(()),
        |enabled| capture_transitions.push(enabled),
        Vec::new,
        |profile, binding, action, _admission| {
            let cmd = match action {
                ResolvedHostAction::AgentCommand { adapter, command } => {
                    format!("{adapter}:{command}")
                }
                other => format!("{other:?}"),
            };
            dispatched.push((profile.to_owned(), binding.to_owned(), cmd.clone()));
            mi_macos::HostActionReceipt {
                action_type: "agent_command".into(),
                binding_id: binding.into(),
                detail: cmd,
                outcome: "executed".into(),
                profile_id: profile.into(),
                target: "antigravity".into(),
                text_byte_len: None,
                timestamp_ms: 1,
            }
        },
    );

    assert_eq!(
        dispatched,
        vec![
            (
                ag_profile_id.clone(),
                "ENC_CLK".into(),
                "antigravity:search".into(),
            ),
            (
                ag_profile_id.clone(),
                "AG14".into(),
                "antigravity:search-next".into(),
            ),
            (
                ag_profile_id,
                "ENC_CLK".into(),
                "antigravity:search-confirm".into(),
            ),
        ]
    );
    assert!(!capture_transitions.contains(&true));
}

#[test]
fn six_layer_v090_holding_search_while_model_picker_active_sends_esc_then_search() {
    let receiver = UserCommandReceiver::bind().unwrap();
    let plan = test_plan();
    let ag_profile_id = plan.hosts[0].profile_id.clone();

    let base = Instant::now() - Duration::from_millis(100);
    let policy = ExecutionPolicy::default();
    policy.set(true, true, base - Duration::from_secs(1));
    let routing = Mutex::new(RoutingContext::default());
    let observed = AtomicU8::new(1);
    let first = AtomicBool::new(false);
    let stop = AtomicBool::new(false);
    let (tx, _rx) = std::sync::mpsc::sync_channel(16);

    let gen: String = receiver.generation().into();
    let mk_ev = |seq: u64, control: u8, released: bool, at: Instant| mi_transport::Activation {
        control,
        layer_override: None,
        released,
        generation: gen.clone(),
        received_sequence: seq,
        received_at: at,
    };

    let mut steps = std::collections::VecDeque::from([
        Some(mk_ev(1, 14, false, base)),
        Some(mk_ev(2, 15, false, base + Duration::from_millis(100))),
        Some(mk_ev(3, 15, true, base + Duration::from_millis(480))),
    ]);

    let mut capture_transitions = Vec::new();
    let mut dispatched = Vec::new();

    execute_clicks_with_encoder_picker(
        &receiver,
        &plan,
        &stop,
        &policy,
        &observed,
        &first,
        &routing,
        &tx,
        None,
        || match steps.pop_front() {
            Some(Some(ev)) => Ok(Some(ev)),
            Some(None) => Ok(None),
            None => {
                stop.store(true, Ordering::Release);
                Ok(None)
            }
        },
        || Ok(()),
        |enabled| capture_transitions.push(enabled),
        Vec::new,
        |profile, binding, action, _admission| {
            let cmd = match action {
                ResolvedHostAction::AgentCommand { adapter, command } => {
                    format!("{adapter}:{command}")
                }
                other => format!("{other:?}"),
            };
            dispatched.push((profile.to_owned(), binding.to_owned(), cmd.clone()));
            mi_macos::HostActionReceipt {
                action_type: "agent_command".into(),
                binding_id: binding.into(),
                detail: cmd,
                outcome: "executed".into(),
                profile_id: profile.into(),
                target: "antigravity".into(),
                text_byte_len: None,
                timestamp_ms: 1,
            }
        },
    );

    assert_eq!(
        dispatched,
        vec![
            (
                ag_profile_id.clone(),
                "AG14".into(),
                "antigravity:model-picker-next".into(),
            ),
            (
                ag_profile_id.clone(),
                "ENC_CLK".into(),
                "antigravity:model-picker-cancel".into(),
            ),
            (ag_profile_id, "ENC_CLK".into(), "antigravity:search".into(),),
        ]
    );
    assert!(capture_transitions.is_empty());
}

#[test]
fn six_layer_foreground_app_switch_cancels_model_picker_on_idle_tick() {
    let receiver = UserCommandReceiver::bind().unwrap();
    receiver.arm().unwrap();
    let plan = test_plan();

    let base = Instant::now() - Duration::from_millis(100);
    let policy = ExecutionPolicy::default();
    policy.set(true, true, base - Duration::from_secs(1));
    let routing = Mutex::new(RoutingContext::default());
    let observed = AtomicU8::new(1);
    let first = AtomicBool::new(false);
    let stop = AtomicBool::new(false);
    let (tx, _rx) = std::sync::mpsc::sync_channel(16);

    let gen: String = receiver.generation().into();
    let mk_ev = |seq: u64, control: u8, released: bool, at: Instant| mi_transport::Activation {
        control,
        layer_override: None,
        released,
        generation: gen.clone(),
        received_sequence: seq,
        received_at: at,
    };

    let mut step = 0;
    let mut capture_transitions = Vec::new();
    let mut dispatched = Vec::new();

    execute_clicks_with_encoder_picker(
        &receiver,
        &plan,
        &stop,
        &policy,
        &observed,
        &first,
        &routing,
        &tx,
        None,
        || {
            step += 1;
            match step {
                1 => Ok(Some(mk_ev(1, 14, false, base))),
                2 => {
                    routing.lock().unwrap().update(Some("com.apple.Safari"));
                    Ok(None)
                }
                _ => {
                    stop.store(true, Ordering::Release);
                    Ok(None)
                }
            }
        },
        || Ok(()),
        |enabled| capture_transitions.push(enabled),
        Vec::new,
        |profile, binding, action, _admission| {
            let cmd = match action {
                ResolvedHostAction::AgentCommand { adapter, command } => {
                    format!("{adapter}:{command}")
                }
                other => format!("{other:?}"),
            };
            dispatched.push((profile.to_owned(), binding.to_owned(), cmd.clone()));
            mi_macos::HostActionReceipt {
                action_type: "agent_command".into(),
                binding_id: binding.into(),
                detail: cmd,
                outcome: "executed".into(),
                profile_id: profile.into(),
                target: "antigravity".into(),
                text_byte_len: None,
                timestamp_ms: 1,
            }
        },
    );

    assert!(capture_transitions.is_empty());
}

#[test]
fn six_layer_due_hold_timer_fires_search_and_transitions_from_model_picker() {
    let receiver = UserCommandReceiver::bind().unwrap();
    receiver.arm().unwrap();
    let plan = test_plan();
    let ag_profile_id = plan.hosts[0].profile_id.clone();

    let policy = ExecutionPolicy::default();
    policy.set(true, true, Instant::now() - Duration::from_secs(1));
    let routing = Mutex::new(RoutingContext::default());
    let observed = AtomicU8::new(1);
    let first = AtomicBool::new(false);
    let stop = AtomicBool::new(false);
    let (tx, _rx) = std::sync::mpsc::sync_channel(16);

    let gen: String = receiver.generation().into();
    let mk_ev = |seq: u64, control: u8, released: bool, at: Instant| mi_transport::Activation {
        control,
        layer_override: None,
        released,
        generation: gen.clone(),
        received_sequence: seq,
        received_at: at,
    };

    let now = Instant::now();
    let mut steps = std::collections::VecDeque::from([
        Some(mk_ev(1, 14, false, now)),
        Some(mk_ev(2, 15, false, now)),
        None,
        Some(mk_ev(3, 15, true, now)),
    ]);

    let mut capture_transitions = Vec::new();
    let mut dispatched = Vec::new();
    let mut slept_for_hold = false;

    execute_clicks_with_encoder_picker(
        &receiver,
        &plan,
        &stop,
        &policy,
        &observed,
        &first,
        &routing,
        &tx,
        None,
        || match steps.pop_front() {
            Some(Some(ev)) => {
                let mut ev = ev;
                ev.received_at = Instant::now();
                Ok(Some(ev))
            }
            Some(None) => {
                if !slept_for_hold {
                    slept_for_hold = true;
                    thread::sleep(Duration::from_millis(360));
                }
                Ok(None)
            }
            None => {
                stop.store(true, Ordering::Release);
                Ok(None)
            }
        },
        || Ok(()),
        |enabled| capture_transitions.push(enabled),
        Vec::new,
        |profile, binding, action, _admission| {
            let cmd = match action {
                ResolvedHostAction::AgentCommand { adapter, command } => {
                    format!("{adapter}:{command}")
                }
                other => format!("{other:?}"),
            };
            dispatched.push((profile.to_owned(), binding.to_owned(), cmd.clone()));
            mi_macos::HostActionReceipt {
                action_type: "agent_command".into(),
                binding_id: binding.into(),
                detail: cmd,
                outcome: "executed".into(),
                profile_id: profile.into(),
                target: "antigravity".into(),
                text_byte_len: None,
                timestamp_ms: 1,
            }
        },
    );

    assert_eq!(
        dispatched,
        vec![
            (
                ag_profile_id.clone(),
                "AG14".into(),
                "antigravity:model-picker-next".into(),
            ),
            (
                ag_profile_id.clone(),
                "ENC_CLK".into(),
                "antigravity:model-picker-cancel".into(),
            ),
            (ag_profile_id, "ENC_CLK".into(), "antigravity:search".into()),
        ]
    );
    assert!(capture_transitions.is_empty());
}

#[test]
fn six_layer_sector_press_during_search_overlay_cancels_overlay() {
    let receiver = UserCommandReceiver::bind().unwrap();
    receiver.arm().unwrap();
    let mut plan = test_plan();
    plan.hosts[0].actions[19] = Some(ResolvedHostAction::Shortcut {
        key: "]".into(),
        key_code: 30,
        modifiers: vec!["command".into()],
        target_bundle_id: Some("com.google.antigravity".into()),
    });
    let ag_profile_id = plan.hosts[0].profile_id.clone();

    let base = Instant::now() - Duration::from_millis(500);
    let policy = ExecutionPolicy::default();
    policy.set(true, true, base - Duration::from_secs(1));
    let routing = Mutex::new(RoutingContext::default());
    let observed = AtomicU8::new(1);
    let first = AtomicBool::new(false);
    let stop = AtomicBool::new(false);
    let (tx, _rx) = std::sync::mpsc::sync_channel(16);

    let gen: String = receiver.generation().into();
    let mk_ev = |seq: u64, control: u8, released: bool, at: Instant| mi_transport::Activation {
        control,
        layer_override: None,
        released,
        generation: gen.clone(),
        received_sequence: seq,
        received_at: at,
    };

    let mut steps = std::collections::VecDeque::from([
        Some(mk_ev(1, 15, false, base)),
        Some(mk_ev(2, 15, true, base + Duration::from_millis(450))),
        None, // inject mouse delta during active search
        Some(mk_ev(3, 19, false, base + Duration::from_millis(500))),
        Some(mk_ev(4, 14, false, base + Duration::from_millis(550))),
    ]);

    let mut capture_transitions = Vec::new();
    let mut dispatched = Vec::new();
    let mut injected_drag = false;

    execute_clicks_with_encoder_picker(
        &receiver,
        &plan,
        &stop,
        &policy,
        &observed,
        &first,
        &routing,
        &tx,
        None,
        || match steps.pop_front() {
            Some(Some(ev)) => Ok(Some(ev)),
            Some(None) => Ok(None),
            None => {
                stop.store(true, Ordering::Release);
                Ok(None)
            }
        },
        || Ok(()),
        |enabled| capture_transitions.push(enabled),
        || {
            if !injected_drag {
                injected_drag = true;
                vec![(-45.0, 0.0)]
            } else {
                Vec::new()
            }
        },
        |profile, binding, action, _admission| {
            let cmd = match action {
                ResolvedHostAction::AgentCommand { adapter, command } => {
                    format!("{adapter}:{command}")
                }
                other => format!("{other:?}"),
            };
            dispatched.push((profile.to_owned(), binding.to_owned(), cmd.clone()));
            mi_macos::HostActionReceipt {
                action_type: "agent_command".into(),
                binding_id: binding.into(),
                detail: cmd,
                outcome: "executed".into(),
                profile_id: profile.into(),
                target: "antigravity".into(),
                text_byte_len: None,
                timestamp_ms: 1,
            }
        },
    );

    assert_eq!(
            dispatched,
            vec![
                (
                    ag_profile_id.clone(),
                    "ENC_CLK".into(),
                    "antigravity:search".into(),
                ),
                (
                    ag_profile_id.clone(),
                    "AG18".into(),
                    "Shortcut { key: \"]\", key_code: 30, modifiers: [\"command\"], target_bundle_id: Some(\"com.google.antigravity\") }".into(),
                ),
                (
                    ag_profile_id,
                    "AG14".into(),
                    "antigravity:model-picker-next".into(),
                ),
            ]
        );
    assert!(capture_transitions.is_empty());
}

#[test]
fn six_layer_idle_turn_activates_model_picker_and_reopened_confirm_preserves_session() {
    let receiver = UserCommandReceiver::bind().unwrap();
    receiver.arm().unwrap();
    let plan = test_plan();
    let ag_profile_id = plan.hosts[0].profile_id.clone();

    let base = Instant::now() - Duration::from_millis(200);
    let policy = ExecutionPolicy::default();
    policy.set(true, true, base - Duration::from_secs(1));
    let routing = Mutex::new(RoutingContext::default());
    let observed = AtomicU8::new(1);
    let first = AtomicBool::new(false);
    let stop = AtomicBool::new(false);
    let (tx, _rx) = std::sync::mpsc::sync_channel(16);

    let gen: String = receiver.generation().into();
    let mk_ev = |seq: u64, control: u8, released: bool, at: Instant| mi_transport::Activation {
        control,
        layer_override: None,
        released,
        generation: gen.clone(),
        received_sequence: seq,
        received_at: at,
    };

    let mut steps = std::collections::VecDeque::from([
        Some(mk_ev(1, 13, false, base)),
        None,
        Some(mk_ev(2, 15, false, base + Duration::from_millis(60))),
        Some(mk_ev(3, 15, true, base + Duration::from_millis(100))),
        Some(mk_ev(4, 14, false, base + Duration::from_millis(140))),
        Some(mk_ev(5, 15, false, base + Duration::from_millis(180))),
        Some(mk_ev(6, 15, true, base + Duration::from_millis(220))),
        Some(mk_ev(7, 15, false, base + Duration::from_millis(260))),
        Some(mk_ev(8, 14, false, base + Duration::from_millis(300))),
        Some(mk_ev(9, 15, true, base + Duration::from_millis(340))),
    ]);

    let mut capture_transitions = Vec::new();
    let mut dispatched = Vec::new();
    let mut injected_flick = false;
    let mut already_closed_first_confirm = true;

    execute_clicks_with_encoder_picker(
        &receiver,
        &plan,
        &stop,
        &policy,
        &observed,
        &first,
        &routing,
        &tx,
        None,
        || match steps.pop_front() {
            Some(Some(ev)) => Ok(Some(ev)),
            Some(None) => Ok(None),
            None => {
                stop.store(true, Ordering::Release);
                Ok(None)
            }
        },
        || Ok(()),
        |enabled| capture_transitions.push(enabled),
        || {
            if !injected_flick {
                injected_flick = true;
                vec![(34.0, 1.0)]
            } else {
                Vec::new()
            }
        },
        |profile, binding, action, _admission| {
            let (cmd, detail) = match action {
                ResolvedHostAction::AgentCommand { adapter, command } => {
                    let logged = format!("{adapter}:{command}");
                    let receipt_detail =
                        if command == "model-picker-confirm" && already_closed_first_confirm {
                            already_closed_first_confirm = false;
                            format!("{adapter}/model-picker-already-closed")
                        } else {
                            format!("{adapter}/{command}")
                        };
                    (logged, receipt_detail)
                }
                other => {
                    let dbg = format!("{other:?}");
                    (dbg.clone(), dbg)
                }
            };
            dispatched.push((profile.to_owned(), binding.to_owned(), cmd));
            mi_macos::HostActionReceipt {
                action_type: "agent_command".into(),
                binding_id: binding.into(),
                detail,
                outcome: "executed".into(),
                profile_id: profile.into(),
                target: "antigravity".into(),
                text_byte_len: None,
                timestamp_ms: 1,
            }
        },
    );

    assert_eq!(
        dispatched,
        vec![
            (
                ag_profile_id.clone(),
                "AG13".into(),
                "antigravity:model-picker-prev".into(),
            ),
            (
                ag_profile_id.clone(),
                "ENC_CLK".into(),
                "antigravity:model-picker-confirm".into(),
            ),
            (
                ag_profile_id.clone(),
                "ENC_CLK".into(),
                "antigravity:search".into(),
            ),
            (
                ag_profile_id.clone(),
                "AG14".into(),
                "antigravity:search-next".into(),
            ),
            (
                ag_profile_id.clone(),
                "ENC_CLK".into(),
                "antigravity:search-confirm".into(),
            ),
            (
                ag_profile_id,
                "AG14".into(),
                "antigravity:scroll-down-fast".into(),
            ),
        ]
    );
    assert!(capture_transitions.is_empty());
}

#[cfg(test)]
pub(crate) fn test_b64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(b2 & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
pub(crate) fn test_b64_decode(raw: &str) -> Vec<u8> {
    let val = |c: u8| -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };
    let clean: Vec<u8> = raw.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let mut out = Vec::new();
    for chunk in clean.chunks(4) {
        let a = val(chunk[0]).unwrap();
        let b = val(chunk[1]).unwrap();
        out.push((a << 2) | (b >> 4));
        if chunk.get(2).copied() != Some(b'=') {
            if let Some(c) = chunk.get(2).copied().and_then(val) {
                out.push((b << 4) | (c >> 2));
                if chunk.get(3).copied() != Some(b'=') {
                    if let Some(d) = chunk.get(3).copied().and_then(val) {
                        out.push((c << 6) | d);
                    }
                }
            }
        }
    }
    out
}

#[cfg(unix)]
#[test]
fn runtime_state_load_and_backup_path_containment_enforce_private_fs_boundaries() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    let temp = tempfile::tempdir().unwrap();
    let doc_dir = temp.path().join("doc");
    let external_dir = temp.path().join("external");
    fs::create_dir_all(&doc_dir).unwrap();
    fs::create_dir_all(&external_dir).unwrap();
    fs::set_permissions(&doc_dir, fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(&external_dir, fs::Permissions::from_mode(0o700)).unwrap();

    // Missing file returns default state.
    let state_path = doc_dir.join("six-layer-runtime.json");
    assert_eq!(load(&state_path).unwrap().version, 1);

    // Symlinked six-layer-runtime.json is rejected.
    let target = doc_dir.join("real.json");
    fs::write(&target, br#"{"version":1}"#).unwrap();
    symlink(&target, &state_path).unwrap();
    assert!(load(&state_path).is_err());
    fs::remove_file(&state_path).unwrap();

    // Oversized six-layer-runtime.json is rejected before reading into heap.
    let big = fs::File::create(&state_path).unwrap();
    big.set_len(2 * 1024 * 1024).unwrap();
    drop(big);
    assert!(load(&state_path).is_err());
    fs::remove_file(&state_path).unwrap();

    // World-writable parent dir is rejected by load.
    fs::set_permissions(&doc_dir, fs::Permissions::from_mode(0o777)).unwrap();
    fs::write(&state_path, br#"{"version":1}"#).unwrap();
    assert!(load(&state_path).is_err());
    fs::set_permissions(&doc_dir, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(load(&state_path).unwrap().version, 1);

    // Backup containment accepts private backup inside document_dir.
    let valid_backup = doc_dir.join("six-layer-recovery/b1");
    fs::create_dir_all(&valid_backup).unwrap();
    fs::set_permissions(
        doc_dir.join("six-layer-recovery"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    fs::set_permissions(&valid_backup, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(lifecycle::validate_backup_within_document_dir(&doc_dir, &valid_backup).is_ok());

    // Backup outside document_dir is rejected.
    let ext_backup = external_dir.join("b2");
    fs::create_dir_all(&ext_backup).unwrap();
    fs::set_permissions(&ext_backup, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(lifecycle::validate_backup_within_document_dir(&doc_dir, &ext_backup).is_err());

    // Traversal escaping document_dir is rejected.
    let traversal = doc_dir.join("../external/b2");
    assert!(lifecycle::validate_backup_within_document_dir(&doc_dir, &traversal).is_err());

    // Symlinked backup inside document_dir pointing outside is rejected.
    let sym_backup = doc_dir.join("six-layer-recovery/sym");
    symlink(&ext_backup, &sym_backup).unwrap();
    assert!(lifecycle::validate_backup_within_document_dir(&doc_dir, &sym_backup).is_err());
}
