use super::*;
use crate::firmware_update::{FirmwareAction, UpdateEvent};

impl WorkbenchStore {
    pub(super) fn dispatch_firmware(
        &mut self,
        action: FirmwareAction,
    ) -> Result<(), WorkbenchError> {
        let result = self.apply_firmware_action(action);
        if let Err(error) = &result {
            self.firmware_update.projection.message = error.clone();
        }
        self.commit_runtime_update();
        result.map_err(WorkbenchError::Routing)
    }

    fn prepare_device_with_discovery(
        &mut self,
        enumerate_usb: impl FnOnce() -> Result<Vec<mi_transport::HidCandidate>, String>,
        enumerate_bootloaders: impl FnOnce()
            -> Result<Vec<crate::firmware_flash::BootloaderPort>, String>,
    ) -> Result<(), String> {
        if !self.firmware_update.projection.downloaded {
            return Err("请先下载并校验固件".into());
        }
        if !self.firmware_update.projection.digest_verified {
            return Err("官方未提供可核验摘要，不能启动自动安装".into());
        }
        if self.six_layer_job.is_some() {
            return Err("请等待当前设备操作完成".into());
        }
        if self.firmware_update.reserved() {
            return Err("已暂停设备控制，请查找引导模式串口".into());
        }

        enum TargetSource {
            Usb(mi_transport::HidCandidate),
            ExistingBootloader(Vec<crate::firmware_flash::BootloaderPort>),
        }

        let source = if let Some(candidate) = self
            .connected_candidate
            .clone()
            .filter(mi_transport::is_exclusive_vendor_candidate)
        {
            TargetSource::Usb(candidate)
        } else {
            let candidates = enumerate_usb()?;
            if candidates.len() > 1 {
                return Err(
                    "检测到多台 Creator Micro 2 有线 USB 设备，请仅保留目标键盘后再准备更新".into(),
                );
            }
            if let Some(candidate) = candidates.into_iter().next() {
                TargetSource::Usb(candidate)
            } else {
                match enumerate_bootloaders() {
                    Ok(ports) if !ports.is_empty() => TargetSource::ExistingBootloader(ports),
                    _ => {
                        let message = "未检测到 Creator Micro 2 的有线 USB 连接或引导串口（蓝牙连接不支持固件备份与刷写，请接入 USB 数据线后重试）".to_string();
                        self.firmware_update.projection.phase = "failed".into();
                        self.firmware_update.projection.message = message.clone();
                        return Err(message);
                    }
                }
            }
        };

        self.release_board_inputs(self.update_now);
        self.suspend_for_six_layer_operation()
            .map_err(|e| e.to_string())?;
        self.firmware_update.projection.device_reserved = true;
        self.firmware_update
            .resume_auto_connect
            .get_or_insert(self.config.auto_connect_usb);
        self.resume_requires_explicit_connect = true;
        self.six_layer_next_auto_resume_at = None;
        self.lifecycle_epoch.fetch_add(1, Ordering::AcqRel);
        self.last_hardware_auto_acceptance = None;
        self.lighting_arbiter.invalidate_hardware_cache();

        match source {
            TargetSource::ExistingBootloader(ports) => {
                if let Some(transport) = self.mac_watcher_transport.take() {
                    mi_transport::HidTransport::stop_discovery(transport.as_ref());
                }
                self.firmware_update.projection.ports = ports;
                self.firmware_update.projection.selected_port = None;
                self.firmware_update.projection.phase = "select_port".into();
                self.firmware_update.projection.message =
                    "检测到设备已处于引导模式，请核对并选择对应串口".into();
            }
            TargetSource::Usb(target) => {
                let transport = match self.mac_watcher_transport.take() {
                    Some(existing) if existing.is_exclusive_vendor() => existing,
                    Some(descriptor_only) => {
                        mi_transport::HidTransport::stop_discovery(descriptor_only.as_ref());
                        Arc::new(mi_transport::MacTransport::exclusive_vendor())
                    }
                    None => Arc::new(mi_transport::MacTransport::exclusive_vendor()),
                };
                let tx = self
                    .firmware_update
                    .start("bootloading", "设备控制已暂停，正在请求进入引导模式…");
                let runtime = self.tokio_runtime.clone();
                std::thread::spawn(move || {
                    let boot_result = runtime.block_on(async {
                        use mi_transport::HidTransport;
                        let current = transport.enumerate().map_err(|e| e.to_string())?;
                        if !current.iter().any(|c| c == &target) {
                            return Err("原设备连接已变化，未发送引导命令".to_string());
                        }
                        let link = transport.open(&target.path).map_err(|e| e.to_string())?;
                        let session = DeviceSession::new(
                            link,
                            Box::new(mi_proto::NativeFramer::default()),
                            Arc::new(mi_session::ReadOnlySchema),
                            mi_session::SessionConfig::default(),
                        );
                        let result = async {
                            session
                                .read_identity_info()
                                .await
                                .map_err(|e| e.to_string())?;
                            session
                                .enter_firmware_bootloader()
                                .await
                                .map_err(|e| e.to_string())
                        }
                        .await;
                        session.shutdown().await;
                        result
                    });
                    mi_transport::HidTransport::stop_discovery(transport.as_ref());
                    drop(transport);
                    let event = match boot_result {
                        Ok(()) => {
                            let mut detected = Ok(Vec::new());
                            for _ in 0..10 {
                                std::thread::sleep(std::time::Duration::from_millis(350));
                                match crate::firmware_flash::enumerate_bootloaders() {
                                    Ok(ports) if !ports.is_empty() => {
                                        detected = Ok(ports);
                                        break;
                                    }
                                    Ok(_) => {
                                        detected = Ok(Vec::new());
                                    }
                                    Err(err) => detected = Err(err.to_string()),
                                }
                            }
                            if detected.as_ref().is_ok_and(|ports| ports.is_empty()) {
                                detected = crate::firmware_flash::enumerate_bootloaders()
                                    .map_err(|e| e.to_string());
                            }
                            UpdateEvent::Bootloader(detected)
                        }
                        Err(err) => UpdateEvent::Bootloader(Err(err)),
                    };
                    let _ = tx.send(event);
                });
            }
        }
        Ok(())
    }

    fn apply_firmware_action(&mut self, action: FirmwareAction) -> Result<(), String> {
        self.require_awake().map_err(|e| e.to_string())?;
        if self.firmware_update.projection.busy {
            return Err("固件操作正在进行，请等待完成".into());
        }
        match action {
            FirmwareAction::Check { channel } => {
                if self.firmware_update.reserved() {
                    return Err("请先结束当前设备更新流程".into());
                }
                self.firmware_update.check(channel);
            }
            FirmwareAction::Download => {
                if self.firmware_update.reserved() {
                    return Err("请先结束当前设备更新流程".into());
                }
                self.firmware_update.download(&self.config.document_dir)?;
            }
            FirmwareAction::PrepareDevice => {
                if self.config.isolated {
                    return Err("隔离验收不会切换实体设备的引导模式".into());
                }
                self.prepare_device_with_discovery(
                    || {
                        use mi_transport::HidTransport;
                        mi_transport::MacTransport::exclusive_vendor()
                            .enumerate()
                            .map_err(|e| e.to_string())
                    },
                    || crate::firmware_flash::enumerate_bootloaders().map_err(|e| e.to_string()),
                )?;
            }
            FirmwareAction::ScanPorts => {
                if !self.firmware_update.reserved() {
                    return Err("请先准备设备更新".into());
                }
                self.firmware_update.scan();
            }
            FirmwareAction::SelectPort { path } => {
                if !self.firmware_update.reserved()
                    || !self
                        .firmware_update
                        .projection
                        .ports
                        .iter()
                        .any(|p| p.path == path)
                {
                    return Err("串口不在本次发现结果中，请重新查找".into());
                }
                self.firmware_update.projection.selected_port = Some(path);
            }
            FirmwareAction::Install => {
                if self.config.isolated {
                    return Err("隔离验收不能刷写实体设备".into());
                }
                self.firmware_update.install(&self.config.document_dir)?;
            }
            FirmwareAction::Finish => {
                if !self.firmware_update.reserved() {
                    return Err("当前没有设备更新流程".into());
                }
                self.firmware_update.projection.device_reserved = false;
                self.firmware_update.projection.phase = "idle".into();
                self.firmware_update.projection.ports.clear();
                self.firmware_update.projection.selected_port = None;
                self.firmware_update.projection.message = "固件流程已结束。请重新连接设备核对运行版本与键位；新固件的配置兼容性需另行核验。".into();
                // Keep discovery passive until the user's explicit connect action.
                self.resume_requires_explicit_connect = true;
                self.config.auto_connect_usb = false;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_firmware_job_blocks_host_control_and_cannot_be_finished_while_busy() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();
        let before = store.document_store.host_execution_enabled();
        store.firmware_update.projection.device_reserved = true;
        assert!(store
            .dispatch_command(WorkbenchCommand::Header(HeaderAction::SetExecutionEnabled(
                !before
            )))
            .is_err());
        assert_eq!(store.document_store.host_execution_enabled(), before);
        assert!(store.connect_default_usb().is_err());
        assert!(store
            .start_six_layer_apply(None)
            .unwrap_err()
            .to_string()
            .contains("固件更新"));
        assert!(store
            .start_six_layer_restore()
            .unwrap_err()
            .to_string()
            .contains("固件更新"));
        let _worker = store.firmware_update.start("writing", "写入中");
        assert!(store.dispatch_firmware(FirmwareAction::Finish).is_err());
        assert!(store.firmware_update.reserved());
        assert!(store.firmware_update.projection.busy);
        store.firmware_update.projection.busy = false;
        store.dispatch_firmware(FirmwareAction::Finish).unwrap();
        assert!(!store.firmware_update.reserved());
        assert_eq!(store.firmware_update.projection.phase, "idle");
        assert!(store.resume_requires_explicit_connect);
        assert!(!store.config.auto_connect_usb);
    }

    #[test]
    fn isolated_app_and_missing_download_never_start_bootloader_or_flash() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();
        assert!(store
            .dispatch_firmware(FirmwareAction::PrepareDevice)
            .is_err());
        assert!(!store.firmware_update.reserved());
        assert!(store.dispatch_firmware(FirmwareAction::Install).is_err());
        assert!(!temp.path().join("firmware-backups").exists());
        assert!(!store.projection().firmware.busy);
    }

    #[test]
    fn prepare_device_never_reserves_without_usb_or_bootloader_and_recognizes_existing_port() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();
        store.firmware_update.projection.downloaded = true;
        store.firmware_update.projection.digest_verified = true;

        let err = store
            .prepare_device_with_discovery(|| Ok(vec![]), || Ok(vec![]))
            .unwrap_err();
        assert!(err.contains("有线 USB"));
        assert!(!store.firmware_update.reserved());
        assert_eq!(store.firmware_update.projection.phase, "failed");

        let candidate = mi_transport::HidCandidate {
            vid: 0x303a,
            pid: 0x8298,
            usage_page: 0xff00,
            usage: 1,
            usage_pairs: vec![mi_transport::UsagePair {
                page: 0xff00,
                usage: 1,
            }],
            transport_hint: mi_transport::TransportHint::Usb,
            product: "Creator Micro 2".into(),
            path: "usb-1".into(),
        };
        let multi_err = store
            .prepare_device_with_discovery(
                || Ok(vec![candidate.clone(), candidate.clone()]),
                || Ok(vec![]),
            )
            .unwrap_err();
        assert!(multi_err.contains("多台"));
        assert!(!store.firmware_update.reserved());

        let port = crate::firmware_flash::BootloaderPort {
            path: "/dev/cu.usbmodem201".into(),
            vid: 0x303a,
            pid: 0x1001,
            serial_number: Some("BOOT201".into()),
            manufacturer: Some("Espressif".into()),
            product: Some("USB JTAG".into()),
        };
        store.mac_watcher_transport =
            Some(Arc::new(mi_transport::MacTransport::exclusive_vendor()));
        store
            .prepare_device_with_discovery(|| Ok(vec![]), || Ok(vec![port.clone()]))
            .unwrap();
        assert!(store.mac_watcher_transport.is_none());
        assert!(store.firmware_update.reserved());
        assert_eq!(store.firmware_update.projection.phase, "select_port");
        assert!(store.firmware_update.projection.selected_port.is_none());
        assert_eq!(store.firmware_update.projection.ports, vec![port]);

        store.firmware_update.projection.device_reserved = false;
        store.mac_watcher_transport =
            Some(Arc::new(mi_transport::MacTransport::exclusive_vendor()));
        store
            .prepare_device_with_discovery(|| Ok(vec![candidate]), || Ok(vec![]))
            .unwrap();
        assert!(store.mac_watcher_transport.is_none());
        assert!(store.firmware_update.reserved());
    }
}
