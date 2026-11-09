use mi_core::{
    AutoSwitchState, BackupCommand, CanonicalAgent, HeaderAction, OwnershipBannerState,
    ProfileCommand, SidebarAction, WorkbenchStore,
};
use std::time::{Duration, Instant};
use tempfile::TempDir;

#[test]
fn canonical_sidebar_profiles_preserve_strict_order_and_idempotent_ready_status() {
    let temp = TempDir::new().unwrap();
    let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();

    assert!(
        store.has_all_desktop_agent_presets(),
        "initial isolated store must install all 6 canonical desktop agent presets"
    );

    let expected_display_names = [
        "Antigravity",
        "Codex",
        "Deepseek Harness",
        "Herdr",
        "Cindy",
        "其他软件",
    ];
    let actual_display_names: Vec<&str> = store
        .sidebar_rows()
        .iter()
        .map(|row| row.display_name.as_str())
        .collect();
    assert_eq!(actual_display_names, expected_display_names);

    let expected_raw_names = [
        "Antigravity",
        "Codex",
        "DeepSeek Coder (DSH)",
        "Herdr",
        "Cindy",
        "其他软件",
    ];
    let actual_raw_names: Vec<&str> = store
        .sidebar_rows()
        .iter()
        .map(|row| row.raw_name.as_str())
        .collect();
    assert_eq!(actual_raw_names, expected_raw_names);

    for (idx, agent) in CanonicalAgent::ALL.iter().enumerate() {
        assert_eq!(agent.rank(), idx);
        assert_eq!(agent.display_name(), expected_display_names[idx]);
        assert_eq!(agent.profile_name(), expected_raw_names[idx]);
    }

    let rev_before = store.projection().document_revision;
    store.set_status_message("已就绪");
    store.prepare_desktop_profiles().unwrap();
    assert_eq!(
        store.status_message(),
        "已就绪",
        "prepare_desktop_profiles must be idempotent and never clobber ready status when all 6 presets exist"
    );
    assert_eq!(store.projection().document_revision, rev_before);
}

#[test]
fn switching_across_all_canonical_profiles_stays_under_two_milliseconds_with_zero_banner_flashes() {
    let temp = TempDir::new().unwrap();
    let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();

    let profile_ids: Vec<String> = store
        .sidebar_rows()
        .iter()
        .map(|row| row.profile_id.clone())
        .collect();
    assert_eq!(profile_ids.len(), 6);

    let mut samples_us: Vec<u128> = Vec::with_capacity(42);
    let mut completed_switches = 0usize;
    let mut error_count = 0usize;

    for _round in 0..7 {
        for pid in &profile_ids {
            let started = Instant::now();
            match store.select_and_activate_profile(pid) {
                Ok(()) => completed_switches += 1,
                Err(_) => error_count += 1,
            }
            let elapsed = started.elapsed();
            samples_us.push(elapsed.as_micros());
            assert!(
                elapsed < Duration::from_millis(2),
                "profile switch took {elapsed:?}, exceeding 2 ms budget"
            );
            assert!(
                store.last_switch_duration() < Duration::from_millis(2),
                "recorded switch duration {:?} exceeded 2 ms budget",
                store.last_switch_duration()
            );
            assert_eq!(store.selected_profile_id(), Some(pid.as_str()));
            assert_eq!(
                store.active_host_profile_id().as_deref(),
                Some(pid.as_str())
            );
            assert!(
                !matches!(
                    store.projection().header.banner,
                    OwnershipBannerState::Caution { .. }
                ),
                "switching profiles must never flash a caution banner"
            );
        }
    }

    samples_us.sort_unstable();
    let median_us = samples_us[samples_us.len() / 2];
    let min_us = samples_us[0];
    let max_us = *samples_us.last().unwrap();
    println!(
        "BENCHMARK_SWITCH_6_PROFILES rounds=7 switches={completed_switches} errors={error_count} median_us={median_us} range_us={min_us}..={max_us} banner_flashes={}",
        store.banner_flash_count()
    );

    assert_eq!(error_count, 0);
    assert_eq!(completed_switches, 42);
    assert_eq!(store.banner_flash_count(), 0);
}

#[test]
fn foreground_auto_follow_switches_linked_agents_and_ignores_unbound_apps_without_disk_writes() {
    let temp = TempDir::new().unwrap();
    let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();

    store.set_auto_follow(true).unwrap();
    assert_eq!(
        store.projection().sidebar.auto_switch,
        AutoSwitchState::Active
    );

    let switched_dsh = store.foreground_changed(Some("com.deepseek.dsh"));
    assert!(switched_dsh);
    assert_eq!(
        store.projection().sidebar.selected_display_name.as_deref(),
        Some("Deepseek Harness")
    );

    let host_rev_before_safari = store.projection().host_revision;
    let doc_rev_before_safari = store.projection().document_revision;
    let switched_safari = store.foreground_changed(Some("com.apple.Safari"));
    assert!(
        !switched_safari,
        "unbound apps must not switch away from the active coding agent profile"
    );
    assert_eq!(store.projection().host_revision, host_rev_before_safari);
    assert_eq!(store.projection().document_revision, doc_rev_before_safari);
    assert_eq!(
        store.projection().sidebar.selected_display_name.as_deref(),
        Some("Deepseek Harness")
    );

    let herdr_id = store
        .sidebar_rows()
        .iter()
        .find(|row| row.canonical_agent == Some(CanonicalAgent::Herdr))
        .unwrap()
        .profile_id
        .clone();
    store
        .dispatch_sidebar_action(SidebarAction::SelectProfile(herdr_id))
        .unwrap();
    assert_eq!(
        store.projection().sidebar.auto_switch,
        AutoSwitchState::Paused
    );

    let switched_while_paused = store.foreground_changed(Some("com.google.antigravity"));
    assert!(!switched_while_paused);
    assert_eq!(
        store.projection().sidebar.selected_display_name.as_deref(),
        Some("Herdr")
    );

    store
        .dispatch_sidebar_action(SidebarAction::ToggleAutoFollowPause)
        .unwrap();
    assert_eq!(
        store.projection().sidebar.auto_switch,
        AutoSwitchState::Active
    );
    assert!(store.foreground_changed(Some("com.google.antigravity")));
    assert_eq!(
        store.projection().sidebar.selected_display_name.as_deref(),
        Some("Antigravity")
    );
}

#[test]
fn custom_profiles_filter_crud_undo_redo_and_preset_roundtrip_work_without_ffi() {
    let temp = TempDir::new().unwrap();
    let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();

    store
        .apply_profile_command(ProfileCommand::Create {
            name: "我的实验配置".to_string(),
        })
        .unwrap();
    assert!(
        store.projection().sidebar.show_other_profiles,
        "creating a custom profile must automatically reveal non-canonical profiles"
    );
    assert_eq!(store.sidebar_rows().len(), 7);
    let custom_id = store
        .sidebar_rows()
        .iter()
        .find(|row| row.raw_name == "我的实验配置")
        .unwrap()
        .profile_id
        .clone();

    store.set_show_other_profiles(false);
    assert_eq!(
        store.sidebar_rows().len(),
        6,
        "hiding other profiles must leave only the 6 canonical profiles visible"
    );
    store.set_show_other_profiles(true);
    assert_eq!(store.sidebar_rows().len(), 7);

    store
        .apply_profile_command(ProfileCommand::Rename {
            profile_id: custom_id.clone(),
            name: "已重命名配置".to_string(),
        })
        .unwrap();
    assert!(store
        .sidebar_rows()
        .iter()
        .any(|row| row.profile_id == custom_id && row.display_name == "已重命名配置"));

    store.dispatch_header_action(HeaderAction::Undo).unwrap();
    assert!(store
        .sidebar_rows()
        .iter()
        .any(|row| row.profile_id == custom_id && row.display_name == "我的实验配置"));

    store.dispatch_header_action(HeaderAction::Redo).unwrap();
    assert!(store
        .sidebar_rows()
        .iter()
        .any(|row| row.profile_id == custom_id && row.display_name == "已重命名配置"));

    let exported = store
        .apply_backup_command(BackupCommand::ExportPreset {
            profile_id: Some(custom_id.clone()),
            layer_id: None,
        })
        .unwrap()
        .unwrap();
    assert!(exported.contains("已重命名配置"));

    store
        .apply_profile_command(ProfileCommand::Delete {
            profile_id: custom_id,
        })
        .unwrap();
    assert_eq!(store.sidebar_rows().len(), 6);

    store
        .apply_backup_command(BackupCommand::ImportPreset { text: exported })
        .unwrap();
    assert!(store.has_all_desktop_agent_presets());
}

#[test]
fn live_ownership_socket_handshake_reconciles_ready_and_native_held_banners_on_profile_switch() {
    use mi_core::WorkbenchStoreConfig;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;

    let temp = TempDir::new().unwrap();
    let sock_path = std::path::PathBuf::from("/tmp").join(format!(
        "mi-own-{}-{}.sock",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_file(&sock_path);
    let listener = UnixListener::bind(&sock_path).unwrap();
    std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o600)).unwrap();

    let server_thread = std::thread::spawn(move || {
        let native_inst = "11111111-2222-3333-4444-555555555555";
        for stream in listener.incoming().take(3) {
            let mut stream = stream.unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let req: serde_json::Value = serde_json::from_str(&line).unwrap();
            let desired = req["desiredOwner"].as_str().unwrap_or("native");
            let (state, reason, input_drained, ptt_drained) = if desired == "micro-input" {
                ("unknown", "native_drain_unconfirmed", false, false)
            } else {
                ("native", "desired_native", false, false)
            };
            let reply = serde_json::json!({
                "version": 1,
                "requestId": req["requestId"],
                "clientInstance": req["clientInstance"],
                "generation": req["generation"],
                "nativeInstance": native_inst,
                "nativeGeneration": 1,
                "state": state,
                "reason": reason,
                "inputDrained": input_drained,
                "pttDrained": ptt_drained,
                "lightingDrained": true,
                "capabilities": ["fenced-release-v1"]
            });
            let mut bytes = serde_json::to_vec(&reply).unwrap();
            bytes.push(b'\n');
            stream.write_all(&bytes).unwrap();
        }
    });

    let mut cfg = WorkbenchStoreConfig::isolated(temp.path());
    cfg.isolated = false;
    cfg.allow_unbridged_desktop = true;
    cfg.ownership_socket_path = Some(sock_path.clone());

    let mut store = WorkbenchStore::open(cfg).unwrap();
    assert_eq!(
        store.projection().header.banner,
        OwnershipBannerState::Ready {
            summary: "Cindy 已确认释放".to_string()
        }
    );

    let codex_id = store
        .find_canonical_profile_id(CanonicalAgent::Codex)
        .unwrap();
    store.select_and_activate_profile(&codex_id).unwrap();
    assert_eq!(
        store.projection().header.banner,
        OwnershipBannerState::NativeHeld {
            summary: "Cindy 原生持有，Micro Input 按键、听写与灯光暂停".to_string()
        }
    );

    let ag_id = store
        .find_canonical_profile_id(CanonicalAgent::Antigravity)
        .unwrap();
    store.select_and_activate_profile(&ag_id).unwrap();
    assert_eq!(
        store.projection().header.banner,
        OwnershipBannerState::Ready {
            summary: "Cindy 已确认释放".to_string()
        }
    );

    server_thread.join().unwrap();
    let _ = std::fs::remove_file(&sock_path);
    let mut fence_os = sock_path.into_os_string();
    fence_os.push(".fence");
    let _ = std::fs::remove_file(std::path::PathBuf::from(fence_os));
}

#[test]
fn live_ownership_slow_drain_defers_to_background_and_completes_on_poll() {
    use mi_core::WorkbenchStoreConfig;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;
    use std::time::{Duration, Instant};

    let temp = TempDir::new().unwrap();
    let sock_path = std::path::PathBuf::from("/tmp").join(format!(
        "mi-slow-{}-{}.sock",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_file(&sock_path);
    let listener = UnixListener::bind(&sock_path).unwrap();
    std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o600)).unwrap();

    let server_thread = std::thread::spawn(move || {
        let native_inst = "11111111-2222-3333-4444-555555555555";
        for (idx, stream) in listener.incoming().take(3).enumerate() {
            let mut stream = stream.unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let req: serde_json::Value = serde_json::from_str(&line).unwrap();
            let desired = req["desiredOwner"].as_str().unwrap_or("native");
            if idx == 2 && desired == "micro-input" {
                std::thread::sleep(Duration::from_millis(120));
            }
            let (state, reason) = if desired == "micro-input" {
                ("unknown", "native_drain_unconfirmed")
            } else {
                ("unknown", "native_state_unconfirmed")
            };
            let reply = serde_json::json!({
                "version": 1,
                "requestId": req["requestId"],
                "clientInstance": req["clientInstance"],
                "generation": req["generation"],
                "nativeInstance": native_inst,
                "nativeGeneration": 1,
                "state": state,
                "reason": reason,
                "inputDrained": false,
                "pttDrained": false,
                "lightingDrained": desired == "micro-input",
                "capabilities": ["fenced-release-v1"]
            });
            let mut bytes = serde_json::to_vec(&reply).unwrap();
            bytes.push(b'\n');
            stream.write_all(&bytes).unwrap();
        }
    });

    let mut cfg = WorkbenchStoreConfig::isolated(temp.path());
    cfg.isolated = false;
    cfg.allow_unbridged_desktop = true;
    cfg.ownership_socket_path = Some(sock_path.clone());

    let mut store = WorkbenchStore::open(cfg).unwrap();
    let codex_id = store
        .find_canonical_profile_id(CanonicalAgent::Codex)
        .unwrap();
    store.select_and_activate_profile(&codex_id).unwrap();
    assert_eq!(
        store.projection().header.banner,
        OwnershipBannerState::NativeHeld {
            summary: "Cindy 原生持有，Micro Input 按键、听写与灯光暂停".to_string()
        }
    );

    let ag_id = store
        .find_canonical_profile_id(CanonicalAgent::Antigravity)
        .unwrap();
    let switch_start = Instant::now();
    store.select_and_activate_profile(&ag_id).unwrap();
    let switch_elapsed = switch_start.elapsed();
    assert!(
        switch_elapsed < Duration::from_millis(45),
        "expected fast non-blocking switch (<45ms), got {switch_elapsed:?}"
    );
    assert_eq!(
        store.projection().header.banner,
        OwnershipBannerState::Ready {
            summary: "Cindy 已确认释放".to_string()
        }
    );

    // Wait for background stream reply and drain hardware events
    let wait_start = Instant::now();
    while !store.poll_hardware_events() && wait_start.elapsed() < Duration::from_millis(500) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        store.projection().header.banner,
        OwnershipBannerState::Ready {
            summary: "Cindy 已确认释放".to_string()
        }
    );

    // Subsequent switches between micro-input profiles should not re-poll socket
    let herdr_id = store
        .find_canonical_profile_id(CanonicalAgent::Herdr)
        .unwrap();
    let switch2_start = Instant::now();
    store.select_and_activate_profile(&herdr_id).unwrap();
    assert!(switch2_start.elapsed() < Duration::from_millis(5));

    server_thread.join().unwrap();
    let _ = std::fs::remove_file(&sock_path);
    let mut fence_os = sock_path.into_os_string();
    fence_os.push(".fence");
    let _ = std::fs::remove_file(std::path::PathBuf::from(fence_os));
}

#[test]
fn usb_connect_and_verified_input_pump_dispatches_host_actions_and_handles_disconnect() {
    use mi_core::{ControlSurfaceId, HeaderReadyState};
    use mi_proto::{Framer, NativeFramer};
    use mi_transport::{
        HidCandidate, HidLink, HidTransport, LinkBackend, TransportError, TransportHint, UsagePair,
    };
    use std::{
        future::Future,
        pin::Pin,
        sync::{Arc, Mutex},
    };
    use tokio::sync::{broadcast, mpsc};

    struct ChannelHidLink {
        rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<Result<Vec<u8>, TransportError>>>,
        closed_tx: mpsc::UnboundedSender<Result<Vec<u8>, TransportError>>,
    }

    impl HidLink for ChannelHidLink {
        fn backend(&self) -> LinkBackend {
            LinkBackend::MacHidapi267Numbered
        }

        fn write<'life0, 'life1, 'async_trait>(
            &'life0 self,
            _report: &'life1 [u8],
        ) -> Pin<Box<dyn Future<Output = Result<(), TransportError>> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async { Ok(()) })
        }

        fn read<'life0, 'async_trait>(
            &'life0 self,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, TransportError>> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move {
                let mut guard = self.rx.lock().await;
                guard
                    .recv()
                    .await
                    .unwrap_or(Err(TransportError::Disconnected))
            })
        }

        fn close(&self) {
            let _ = self.closed_tx.send(Err(TransportError::Disconnected));
        }
    }

    struct FakeUsbTransport {
        candidate: HidCandidate,
        link: Mutex<Option<Arc<dyn HidLink>>>,
        events: broadcast::Sender<()>,
    }

    impl HidTransport for FakeUsbTransport {
        fn enumerate(&self) -> Result<Vec<HidCandidate>, TransportError> {
            Ok(vec![self.candidate.clone()])
        }

        fn subscribe(&self) -> broadcast::Receiver<()> {
            self.events.subscribe()
        }

        fn open(&self, id: &str) -> Result<Arc<dyn HidLink>, TransportError> {
            assert_eq!(id, self.candidate.path);
            self.link
                .lock()
                .unwrap()
                .take()
                .ok_or_else(|| TransportError::Unavailable("already opened".to_string()))
        }
    }

    let temp = TempDir::new().unwrap();
    let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();
    assert!(store.device_session.is_none());
    assert!(store.projection().header.show_connect_button);

    let (report_tx, report_rx) = mpsc::unbounded_channel();
    let link: Arc<dyn HidLink> = Arc::new(ChannelHidLink {
        rx: tokio::sync::Mutex::new(report_rx),
        closed_tx: report_tx.clone(),
    });
    let (events, _) = broadcast::channel(8);
    let transport = FakeUsbTransport {
        candidate: HidCandidate {
            vid: 0x303A,
            pid: 0x8298,
            usage_page: 0xFF00,
            usage: 1,
            usage_pairs: vec![UsagePair {
                page: 0xFF00,
                usage: 1,
            }],
            transport_hint: TransportHint::Usb,
            product: "Creator Micro 2".to_string(),
            path: "IOService:/Synthetic/CreatorMicro2".to_string(),
        },
        link: Mutex::new(Some(link)),
        events,
    };

    let connected = store.connect_with_transport(&transport, true).unwrap();
    assert!(
        connected,
        "single verified USB 303A:8298 candidate must connect"
    );
    assert!(store.device_session.is_some());
    assert!(!store.projection().header.show_connect_button);
    assert!(store.projection().header.host_execution_enabled);
    assert_eq!(
        store.projection().header.ready_state,
        HeaderReadyState::Ready {
            text: "小键盘已连接".to_string()
        }
    );

    let framer = NativeFramer::default();
    let press_ag12 = framer
        .encode(br#"{"m":"v.oai.hid","p":{"k":"AG12","act":1}}"#)
        .unwrap();
    for report in press_ag12 {
        report_tx.send(Ok(report)).unwrap();
    }
    let release_ag12 = framer
        .encode(br#"{"m":"v.oai.hid","p":{"k":"AG12","act":0}}"#)
        .unwrap();
    for report in release_ag12 {
        report_tx.send(Ok(report)).unwrap();
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let press_receipt = rt
        .block_on(async {
            tokio::time::timeout(
                Duration::from_secs(2),
                store.recv_next_hardware_event(Instant::now()),
            )
            .await
            .expect("timed out waiting for AG12 press")
        })
        .expect("channel open")
        .expect("AG12 press must produce a host action receipt");

    assert_eq!(press_receipt.binding_id, "AG12");
    assert_eq!(press_receipt.outcome, "simulated");
    assert_eq!(
        store.projection().canvas.selected_surface,
        ControlSurfaceId::Key12
    );

    let release_receipt = rt
        .block_on(async {
            tokio::time::timeout(
                Duration::from_secs(2),
                store.recv_next_hardware_event(Instant::now()),
            )
            .await
            .expect("timed out waiting for AG12 release")
        })
        .expect("channel open");
    assert!(release_receipt.is_none());

    report_tx.send(Err(TransportError::Disconnected)).unwrap();
    let disconnect_outcome = rt
        .block_on(async {
            tokio::time::timeout(
                Duration::from_secs(2),
                store.recv_next_hardware_event(Instant::now()),
            )
            .await
            .expect("timed out waiting for disconnect event")
        })
        .expect("channel open");
    assert!(disconnect_outcome.is_none());
    assert!(store.device_session.is_none());
    assert!(store.projection().header.show_connect_button);
}

#[test]
fn usb_connect_automatically_verifies_identity_snapshot_lighting_and_writes_acceptance_receipt() {
    use mi_core::HardwareAutoAcceptanceReceipt;
    use mi_proto::{Framer, NativeFramer};
    use mi_transport::{
        HidCandidate, HidLink, HidTransport, LinkBackend, TransportError, TransportHint, UsagePair,
    };
    use serde_json::{json, Value};
    use std::{
        future::Future,
        pin::Pin,
        sync::{Arc, Mutex},
    };
    use tokio::sync::{broadcast, mpsc};

    fn b64_encode(input: &[u8]) -> String {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
        for chunk in input.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
            let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
            let triple = (b0 << 16) | (b1 << 8) | b2;
            out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
            out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
            if chunk.len() > 1 {
                out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
            } else {
                out.push('=');
            }
            if chunk.len() > 2 {
                out.push(TABLE[(triple & 0x3F) as usize] as char);
            } else {
                out.push('=');
            }
        }
        out
    }

    struct AutoAcceptHidLink {
        rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<Result<Vec<u8>, TransportError>>>,
        tx: mpsc::UnboundedSender<Result<Vec<u8>, TransportError>>,
        in_framer: Mutex<NativeFramer>,
        keymap_bytes: Vec<u8>,
        keymap_sha1: String,
        methods_seen: Arc<Mutex<Vec<String>>>,
    }

    impl HidLink for AutoAcceptHidLink {
        fn backend(&self) -> LinkBackend {
            LinkBackend::MacHidapi267Numbered
        }

        fn write<'life0, 'life1, 'async_trait>(
            &'life0 self,
            report: &'life1 [u8],
        ) -> Pin<Box<dyn Future<Output = Result<(), TransportError>> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            Self: 'async_trait,
        {
            let lines = self
                .in_framer
                .lock()
                .unwrap()
                .feed(report)
                .unwrap_or_default();
            for line in lines {
                let req: Value = serde_json::from_slice(&line).unwrap();
                let id = req["id"].clone();
                let method = req["method"].as_str().unwrap_or("").to_string();
                self.methods_seen.lock().unwrap().push(method.clone());
                let reply = match method.as_str() {
                    "device.status" => json!({
                        "id": id,
                        "result": {
                            "version": "0.6.3",
                            "profile_index": 0,
                            "layer_index": 1
                        }
                    }),
                    "fs.list" => json!({
                        "id": id,
                        "result": [
                            {
                                "name": "keymap.json",
                                "size": self.keymap_bytes.len(),
                                "checksum": self.keymap_sha1
                            }
                        ]
                    }),
                    "fs.readbin" => {
                        let offset = req["params"]["offset"].as_u64().unwrap_or(0) as usize;
                        let len = req["params"]["len"].as_u64().unwrap_or(3072) as usize;
                        let end = (offset + len).min(self.keymap_bytes.len());
                        let slice = &self.keymap_bytes[offset..end];
                        json!({
                            "id": id,
                            "result": {
                                "total_size": self.keymap_bytes.len(),
                                "data": b64_encode(slice)
                            }
                        })
                    }
                    "v.oai.rgbcfg" | "lights.preview" | "v.oai.thstatus" => json!({
                        "id": id,
                        "result": {}
                    }),
                    other => panic!("unexpected RPC method: {other}"),
                };
                for out_report in NativeFramer::default()
                    .encode(reply.to_string().as_bytes())
                    .unwrap()
                {
                    let _ = self.tx.send(Ok(out_report));
                }
            }
            Box::pin(async { Ok(()) })
        }

        fn read<'life0, 'async_trait>(
            &'life0 self,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, TransportError>> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move {
                let mut guard = self.rx.lock().await;
                guard
                    .recv()
                    .await
                    .unwrap_or(Err(TransportError::Disconnected))
            })
        }

        fn close(&self) {
            let _ = self.tx.send(Err(TransportError::Disconnected));
        }
    }

    struct AutoAcceptTransport {
        candidate: HidCandidate,
        link: Mutex<Option<Arc<dyn HidLink>>>,
        events: broadcast::Sender<()>,
    }

    impl HidTransport for AutoAcceptTransport {
        fn enumerate(&self) -> Result<Vec<HidCandidate>, TransportError> {
            Ok(vec![self.candidate.clone()])
        }

        fn subscribe(&self) -> broadcast::Receiver<()> {
            self.events.subscribe()
        }

        fn open(&self, id: &str) -> Result<Arc<dyn HidLink>, TransportError> {
            assert_eq!(id, self.candidate.path);
            self.link
                .lock()
                .unwrap()
                .take()
                .ok_or_else(|| TransportError::Unavailable("already opened".to_string()))
        }
    }

    let temp = TempDir::new().unwrap();
    let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();

    let mut raw_demo = mi_model::demo().unwrap().raw;
    raw_demo["version"] = json!(1);
    let keymap_bytes = serde_json::to_vec(&raw_demo).unwrap();
    let keymap_file = mi_model::operation_record::file_value(&keymap_bytes);
    let keymap_sha1 = keymap_file["sha1"].as_str().unwrap().to_string();

    let (report_tx, report_rx) = mpsc::unbounded_channel();
    let methods_seen = Arc::new(Mutex::new(Vec::new()));
    let link: Arc<dyn HidLink> = Arc::new(AutoAcceptHidLink {
        rx: tokio::sync::Mutex::new(report_rx),
        tx: report_tx,
        in_framer: Mutex::new(NativeFramer::default()),
        keymap_bytes: keymap_bytes.clone(),
        keymap_sha1: keymap_sha1.clone(),
        methods_seen: Arc::clone(&methods_seen),
    });
    let (events, _) = broadcast::channel(8);
    let transport = AutoAcceptTransport {
        candidate: HidCandidate {
            vid: 0x303A,
            pid: 0x8298,
            usage_page: 0xFF00,
            usage: 1,
            usage_pairs: vec![UsagePair {
                page: 0xFF00,
                usage: 1,
            }],
            transport_hint: TransportHint::Usb,
            product: "Creator Micro 2".to_string(),
            path: "IOService:/Synthetic/AutoAcceptCreatorMicro2".to_string(),
        },
        link: Mutex::new(Some(link)),
        events,
    };

    assert!(store.connect_with_transport(&transport, true).unwrap());

    let wait_start = Instant::now();
    while !store.hardware_rpc_verified && wait_start.elapsed() < Duration::from_secs(5) {
        let _ = store.poll_hardware_events();
        std::thread::sleep(Duration::from_millis(10));
    }

    assert!(
        store.hardware_rpc_verified,
        "connecting a responsive USB 303A:8298 device must automatically complete hardware acceptance"
    );
    assert!(
        store.status_message().contains("固件 0.6.3 自动验收通过"),
        "unexpected status_message: {}",
        store.status_message()
    );

    let receipt_path = temp.path().join("hardware-auto-acceptance.json");
    assert!(
        receipt_path.is_file(),
        "hardware-auto-acceptance.json must be written automatically on connect"
    );
    let receipt: HardwareAutoAcceptanceReceipt =
        serde_json::from_slice(&std::fs::read(&receipt_path).unwrap()).unwrap();
    assert_eq!(receipt.status, "PASS");
    assert_eq!(receipt.firmware_version, "0.6.3");
    assert_eq!(receipt.profile_index, Some(0));
    assert_eq!(receipt.layer_index, Some(1));
    assert_eq!(receipt.keymap_size, keymap_bytes.len() as u64);
    assert_eq!(receipt.keymap_sha1, keymap_sha1);
    assert_eq!(receipt.keymap_sha256, mi_bridge::sha256_hex(&keymap_bytes));
    assert!(receipt.lighting_rgbcfg_synced);
    assert!(receipt.lighting_preview_synced);
    assert!(receipt.lighting_thstatus_synced);

    let seen = methods_seen.lock().unwrap().clone();
    assert!(seen.contains(&"device.status".to_string()));
    assert!(seen.contains(&"fs.list".to_string()));
    assert!(seen.contains(&"fs.readbin".to_string()));
    assert!(seen.contains(&"v.oai.rgbcfg".to_string()));
    assert!(seen.contains(&"lights.preview".to_string()));
    assert!(seen.contains(&"v.oai.thstatus".to_string()));
}

#[test]
fn four_hardware_profile_slots_default_to_agy_agy_codex_dsh_and_cycle_and_persist() {
    let temp = TempDir::new().unwrap();
    let wb_path = temp.path().join("workbench.json");
    let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();
    store.document_store.open(wb_path).unwrap();
    store.prepare_desktop_profiles().unwrap();
    store.ensure_default_hardware_profile_slots();

    let slots = &store.projection().sidebar.hardware_slots;
    assert_eq!(slots[0].display_name, "Antigravity");
    assert_eq!(slots[1].display_name, "Antigravity");
    assert_eq!(slots[2].display_name, "Codex");
    assert_eq!(slots[3].display_name, "Deepseek Harness");
    assert_eq!(store.projection().sidebar.active_hardware_slot, 0);
    assert_eq!(
        store.projection().canvas.touch_led_states,
        [true, true, true]
    );

    store
        .dispatch_sidebar_action(SidebarAction::CycleTouchProfileSlot)
        .unwrap();
    assert_eq!(store.projection().sidebar.active_hardware_slot, 1);
    assert_eq!(
        store.projection().sidebar.selected_display_name.as_deref(),
        Some("Antigravity")
    );
    assert_eq!(
        store.projection().canvas.touch_led_states,
        [true, false, false]
    );

    store
        .dispatch_sidebar_action(SidebarAction::CycleTouchProfileSlot)
        .unwrap();
    assert_eq!(store.projection().sidebar.active_hardware_slot, 2);
    assert_eq!(
        store.projection().sidebar.selected_display_name.as_deref(),
        Some("Codex")
    );
    assert_eq!(
        store.projection().canvas.touch_led_states,
        [false, true, false]
    );

    store
        .dispatch_sidebar_action(SidebarAction::CycleTouchProfileSlot)
        .unwrap();
    assert_eq!(store.projection().sidebar.active_hardware_slot, 3);
    assert_eq!(
        store.projection().sidebar.selected_display_name.as_deref(),
        Some("Deepseek Harness")
    );
    assert_eq!(
        store.projection().canvas.touch_led_states,
        [false, false, true]
    );

    store
        .dispatch_sidebar_action(SidebarAction::CycleTouchProfileSlot)
        .unwrap();
    assert_eq!(store.projection().sidebar.active_hardware_slot, 1);
    assert_eq!(
        store.projection().sidebar.selected_display_name.as_deref(),
        Some("Antigravity")
    );

    let herdr_id = store
        .find_canonical_profile_id(CanonicalAgent::Herdr)
        .unwrap();
    store
        .dispatch_sidebar_action(SidebarAction::SetHardwareSlotProfile {
            slot_index: 2,
            profile_id: herdr_id.clone(),
        })
        .unwrap();
    assert_eq!(
        store.projection().sidebar.hardware_slots[2].display_name,
        "Herdr"
    );

    drop(store);
    let mut reopened = WorkbenchStore::open_isolated(temp.path()).unwrap();
    assert_eq!(
        reopened.projection().sidebar.hardware_slots[2].display_name,
        "Herdr"
    );

    reopened
        .dispatch_sidebar_action(SidebarAction::ResetHardwareSlotsToDefault)
        .unwrap();
    assert_eq!(
        reopened.projection().sidebar.hardware_slots[2].display_name,
        "Codex"
    );
    assert_eq!(reopened.projection().sidebar.active_hardware_slot, 0);
}
