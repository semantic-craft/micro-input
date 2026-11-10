use super::*;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum SystemLifecycle {
    Awake,
    Sleeping,
    Waking,
    CleanupFailed(String),
}

impl WorkbenchStore {
    pub(super) fn discovery_is_descriptor_only(&self) -> bool {
        self.resume_requires_explicit_connect || self.six_layer_state.installation.is_some()
    }

    pub(super) fn require_awake(&self) -> Result<(), WorkbenchError> {
        if self.system_lifecycle == SystemLifecycle::Awake {
            Ok(())
        } else {
            Err(WorkbenchError::Routing(
                "系统睡眠或旧会话仍在释放，宿主输入已暂停".into(),
            ))
        }
    }

    pub fn system_will_sleep(&mut self) {
        if self.system_lifecycle != SystemLifecycle::Awake {
            if self.system_lifecycle == SystemLifecycle::Waking {
                self.system_lifecycle = SystemLifecycle::Sleeping;
            }
            return;
        }
        self.system_lifecycle = SystemLifecycle::Sleeping;
        self.lifecycle_epoch.fetch_add(1, Ordering::AcqRel);
        self.resume_requires_explicit_connect = true;
        self.native_ownership.stop();
        self.held_ptt_binding = None;
        self.ptt_controller.cancel();
        self.board_pressed.clear();
        self.dial_pressed = false;
        self.dial_turned_while_pressed = false;
        self.board_decoder.store(false, Ordering::Release);
        self.board_routing = BoardRouting::Unverified;
        self.radial_state.cancel();
        self.radial_readiness = RadialInputReadiness::Disconnected;
        self.overlay_controller.hide();
        self.busy = false;
        self.document_store.cancel_apply_preview();
        if let Some(EditorModalProjection::ReconnectComparison(modal)) = &self.active_modal {
            let _ = self.document_store.resolve_comparison(
                &modal.token,
                mi_model::comparison::Choice::Cancel,
                &Value::Null,
            );
        }
        if matches!(
            self.active_modal,
            Some(
                EditorModalProjection::ReconnectComparison(_)
                    | EditorModalProjection::ApplyPreparation(_)
            )
        ) {
            self.active_modal = None;
        }
        self.lighting_preview_active = false;
        self.lighting_preview_remaining_secs = 0;
        self.connected_candidate = None;
        self.hardware_rpc_verified = false;
        self.last_hardware_auto_acceptance = None;
        self.lighting_arbiter.invalidate_hardware_cache();
        let session = self.device_session.take();
        if let Some(session) = &session {
            session.disconnect();
        }
        let running = self.six_layer_running.take();
        if let Some(running) = &running {
            running.retire();
        }
        let pending = self.six_layer_job.take();
        if let Some(transport) = self.mac_watcher_transport.take() {
            mi_transport::HidTransport::stop_discovery(transport.as_ref());
        }
        let stop_result = if self.config.isolated {
            if self
                .tokio_runtime
                .block_on(self.ptt_controller.verify_stopped())
            {
                Ok(())
            } else {
                Err("原听写停止未确认".into())
            }
        } else {
            self.live_executor.stop_owned_actions()
        };
        self.ptt_controller.cleanup_confirmed = stop_result.is_ok();
        if stop_result.is_ok() {
            self.voice_manager.confirm_dictation_stopped();
        }
        let runtime = Arc::clone(&self.tokio_runtime);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut result = stop_result;
            if let Some(session) = session {
                runtime.block_on(session.shutdown());
            }
            if let Some(running) = running {
                result = result.and(running.shutdown(&runtime));
            }
            if let Some(pending) = pending {
                match pending.recv() {
                    Ok(Ok(Some(running))) => {
                        result = result.and(running.shutdown(&runtime));
                    }
                    Ok(Ok(None)) | Ok(Err(_)) => {}
                    Err(_) => result = result.and(Err("旧设备操作清理未确认".into())),
                }
            }
            let _ = tx.send(result);
        });
        self.lifecycle_cleanup = Some(rx);
        self.status_message = "系统睡眠 · 旧会话已失效，正在释放资源".into();
        self.rebuild_projection();
    }

    pub fn system_did_wake(&mut self) {
        if self.system_lifecycle == SystemLifecycle::Sleeping {
            self.system_lifecycle = SystemLifecycle::Waking;
            self.status_message = "系统已唤醒 · 等待旧会话释放".into();
            self.rebuild_projection();
        }
        self.poll_workspace_lifecycle();
    }

    pub(super) fn poll_workspace_lifecycle(&mut self) {
        if let Some(rx) = &self.lifecycle_cleanup {
            match rx.try_recv() {
                Ok(result) => {
                    self.lifecycle_cleanup = None;
                    if let Err(error) = result {
                        self.ptt_controller.cleanup_confirmed = false;
                        self.status_message =
                            format!("旧会话清理未确认 · {error}；宿主输入保持暂停");
                        self.system_lifecycle = SystemLifecycle::CleanupFailed(error);
                        self.rebuild_projection();
                        return;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.lifecycle_cleanup = None;
                    self.system_lifecycle = SystemLifecycle::CleanupFailed("清理任务已中断".into());
                    self.status_message = "清理任务已中断 · 宿主输入保持暂停".into();
                    self.rebuild_projection();
                    return;
                }
            }
        }
        if self.system_lifecycle != SystemLifecycle::Waking {
            return;
        }
        self.lifecycle_epoch.fetch_add(1, Ordering::AcqRel);
        self.system_lifecycle = SystemLifecycle::Awake;
        self.status_message = if self.discovery_enabled {
            self.start_mac_usb_discovery_watcher();
            self.discovery_refresh_count += 1;
            if let Some(transport) = &self.mac_watcher_transport {
                let _ = mi_transport::HidTransport::enumerate(transport.as_ref());
            }
            "系统已唤醒 · 设备发现已刷新，请明确重新连接"
        } else {
            "系统已唤醒 · USB 发现尚未开始"
        }
        .into();
        self.rebuild_projection();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mi_transport::{HidLink, LinkBackend, TransportError};
    use std::{future::Future, pin::Pin, sync::atomic::AtomicUsize};

    struct DelayedReader {
        entered: AtomicBool,
        closes: AtomicUsize,
        release: tokio::sync::Notify,
    }

    impl HidLink for DelayedReader {
        fn backend(&self) -> LinkBackend {
            LinkBackend::MacHidapi267Numbered
        }
        fn read<'a, 'b>(
            &'a self,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, TransportError>> + Send + 'b>>
        where
            'a: 'b,
            Self: 'b,
        {
            Box::pin(async move {
                self.entered.store(true, Ordering::Release);
                self.release.notified().await;
                Err(TransportError::Disconnected)
            })
        }
        fn write<'a, 'c, 'b>(
            &'a self,
            _: &'c [u8],
        ) -> Pin<Box<dyn Future<Output = Result<(), TransportError>> + Send + 'b>>
        where
            'a: 'b,
            'c: 'b,
            Self: 'b,
        {
            Box::pin(async { Ok(()) })
        }
        fn close(&self) {
            self.closes.fetch_add(1, Ordering::AcqRel);
        }
    }

    fn until(mut condition: impl FnMut() -> bool) {
        let limit = Instant::now() + Duration::from_secs(3);
        while !condition() {
            assert!(Instant::now() < limit, "lifecycle completion timed out");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn wake_waits_for_old_reader_and_rejects_late_callbacks_without_reconnecting() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(dir.path()).unwrap();
        store.start_mac_usb_discovery_watcher();
        let link = Arc::new(DelayedReader {
            entered: AtomicBool::new(false),
            closes: AtomicUsize::new(0),
            release: tokio::sync::Notify::new(),
        });
        let old = {
            let _entered = store.tokio_runtime.enter();
            DeviceSession::new(
                link.clone(),
                Box::new(mi_proto::NativeFramer::default()),
                Arc::new(mi_session::ReadOnlySchema),
                mi_session::SessionConfig::default(),
            )
        };
        until(|| link.entered.load(Ordering::Acquire));
        store.device_session = Some(old.clone());
        store.hardware_rpc_verified = true;
        let old_epoch = store.lifecycle_epoch.load(Ordering::Acquire);
        store.system_will_sleep();
        assert!(old.is_disconnected());
        assert!(link.closes.load(Ordering::Acquire) > 0);
        assert!(store.device_session.is_none());
        store.system_did_wake();
        store.system_did_wake();
        store.poll_hardware_events();
        assert_eq!(store.system_lifecycle, SystemLifecycle::Waking);
        assert_eq!(store.discovery_refresh_count, 0);
        assert!(store.require_awake().is_err());
        link.release.notify_one();
        until(|| {
            store.poll_hardware_events();
            store.system_lifecycle == SystemLifecycle::Awake
        });
        let awake_epoch = store.lifecycle_epoch.load(Ordering::Acquire);
        assert!(awake_epoch > old_epoch + 1);
        assert_eq!(store.discovery_refresh_count, 1);
        assert!(store.discovery_is_descriptor_only());
        store.system_did_wake();
        assert_eq!(store.discovery_refresh_count, 1);
        assert_eq!(store.lifecycle_epoch.load(Ordering::Acquire), awake_epoch);
        let message = store.status_message().to_string();
        for epoch in [old_epoch, old_epoch + 1, awake_epoch] {
            store.handle_hardware_loop_event(
                HardwareLoopEvent::DiscoveryChanged { epoch },
                Instant::now(),
            );
        }
        store.handle_hardware_loop_event(
            HardwareLoopEvent::Disconnected { session_id: old.id },
            Instant::now(),
        );
        assert!(store.device_session.is_none());
        assert!(!store.hardware_rpc_verified);
        assert_eq!(store.status_message(), message);
    }

    #[test]
    fn old_receipts_cannot_overwrite_a_new_connection_or_its_persisted_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(dir.path()).unwrap();
        let old_epoch = store.lifecycle_epoch.load(Ordering::Acquire);
        let request = store.native_ownership.begin_request(true);
        store.system_will_sleep();
        store.system_did_wake();
        until(|| {
            store.poll_hardware_events();
            store.system_lifecycle == SystemLifecycle::Awake
        });
        let link = Arc::new(DelayedReader {
            entered: AtomicBool::new(false),
            closes: AtomicUsize::new(0),
            release: tokio::sync::Notify::new(),
        });
        let current = {
            let _entered = store.tokio_runtime.enter();
            DeviceSession::new(
                link.clone(),
                Box::new(mi_proto::NativeFramer::default()),
                Arc::new(mi_session::ReadOnlySchema),
                mi_session::SessionConfig::default(),
            )
        };
        until(|| link.entered.load(Ordering::Acquire));
        store.device_session = Some(current.clone());
        let receipt = HardwareAutoAcceptanceReceipt {
            schema_version: 1,
            status: "PASS".into(),
            session_id: current.id,
            backend: "synthetic".into(),
            exclusive_vendor: false,
            product: "current".into(),
            vid_pid: "303A:8298".into(),
            device_path: "synthetic".into(),
            firmware_version: "0.6.3".into(),
            profile_index: Some(0),
            layer_index: Some(1),
            keymap_size: 0,
            keymap_sha1: "fixture".into(),
            keymap_sha256: "fixture".into(),
            smart_actions_present: false,
            smart_actions_size: None,
            smart_actions_sha1: None,
            lighting_rgbcfg_synced: false,
            lighting_preview_synced: false,
            lighting_thstatus_synced: false,
            timestamp_ms: 0,
        };
        store.handle_hardware_loop_event(
            HardwareLoopEvent::HardwareAutoAccepted {
                session_id: current.id,
                receipt: receipt.clone(),
            },
            Instant::now(),
        );
        let path = dir.path().join("hardware-auto-acceptance.json");
        let saved = fs::read(&path).unwrap();
        let status = store.status_message().to_string();
        let old_session_id = current.id.wrapping_add(1000);
        let mut stale = receipt.clone();
        stale.product = "old".into();
        stale.session_id = old_session_id;
        let input = InputEvent {
            session_id: old_session_id,
            board_layer: None,
            sequence: 1,
            ingress_sequence: 1,
            radial_center: None,
            radial_position: None,
            timestamp_ms: 0,
            kind: InputEventKind::Pressed,
            logical_switch: None,
            binding_id: Some("AG11".into()),
            physical_control: None,
            keycap_group: None,
            action_role: None,
            lost_count: 0,
            action_eligible: true,
        };
        for event in [
            HardwareLoopEvent::HardwareAutoAccepted {
                session_id: old_session_id,
                receipt: stale,
            },
            HardwareLoopEvent::Input(input),
            HardwareLoopEvent::Disconnected {
                session_id: old_session_id,
            },
            HardwareLoopEvent::OwnershipReply {
                epoch: old_epoch,
                request,
                reply: None,
            },
            HardwareLoopEvent::DiscoveryChanged { epoch: old_epoch },
        ] {
            assert!(store
                .handle_hardware_loop_event(event, Instant::now())
                .is_none());
        }
        assert_eq!(store.device_session.as_ref().unwrap().id, current.id);
        assert!(store.hardware_rpc_verified);
        assert_eq!(store.last_hardware_auto_acceptance, Some(receipt));
        assert_eq!(fs::read(path).unwrap(), saved);
        assert_eq!(store.status_message(), status);
        assert!(!store.ptt_controller.is_active);
        current.disconnect();
        link.release.notify_one();
        store.tokio_runtime.block_on(current.shutdown());
    }

    #[test]
    fn late_six_layer_job_is_drained_before_wake_and_never_installed() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(dir.path()).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        store.six_layer_job = Some(rx);
        store.system_will_sleep();
        store.system_did_wake();
        store.poll_hardware_events();
        assert_eq!(store.system_lifecycle, SystemLifecycle::Waking);
        assert!(store.six_layer_job.is_none());
        let link = Arc::new(DelayedReader {
            entered: AtomicBool::new(false),
            closes: AtomicUsize::new(0),
            release: tokio::sync::Notify::new(),
        });
        let session = {
            let _entered = store.tokio_runtime.enter();
            DeviceSession::new(
                link.clone(),
                Box::new(mi_proto::NativeFramer::default()),
                Arc::new(mi_session::ReadOnlySchema),
                mi_session::SessionConfig::default(),
            )
        };
        until(|| link.entered.load(Ordering::Acquire));
        let running =
            crate::six_layer_runtime::RunningSixLayer::isolated(session.clone(), dir.path());
        let receiver = running.receiver.clone();
        tx.send(Ok(Some(running))).unwrap();
        until(|| session.is_disconnected());
        store.poll_hardware_events();
        assert_eq!(store.system_lifecycle, SystemLifecycle::Waking);
        assert!(receiver.failure().is_some());
        link.release.notify_one();
        until(|| {
            store.poll_hardware_events();
            store.system_lifecycle == SystemLifecycle::Awake
        });
        assert!(store.six_layer_running.is_none());
        assert!(store.device_session.is_none());
        assert_eq!(store.discovery_refresh_count, 0);
        assert!(store.status_message().contains("USB 发现尚未开始"));
    }

    #[test]
    fn sleep_during_pending_wake_cannot_reopen_and_failed_stop_stays_closed() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(dir.path()).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        store.six_layer_job = Some(rx);
        store.system_will_sleep();
        store.system_did_wake();
        store.system_will_sleep();
        tx.send(Ok(None)).unwrap();
        until(|| {
            store.poll_hardware_events();
            store.lifecycle_cleanup.is_none()
        });
        assert_eq!(store.system_lifecycle, SystemLifecycle::Sleeping);
        store.system_did_wake();
        assert_eq!(store.system_lifecycle, SystemLifecycle::Awake);
        store.ptt_controller.set_next_verify_stopped(false);
        store.system_will_sleep();
        store.system_did_wake();
        until(|| {
            store.poll_hardware_events();
            store.lifecycle_cleanup.is_none()
        });
        assert!(matches!(
            store.system_lifecycle,
            SystemLifecycle::CleanupFailed(_)
        ));
        store.system_did_wake();
        assert!(store.require_awake().is_err());
        assert!(!store.ptt_controller.cleanup_confirmed);
        assert!(store.status_message().contains("未确认"));
        assert!(store
            .dispatch_session_strip_action(SessionStripAction::ActivateSlot(0))
            .is_err());
    }
}
