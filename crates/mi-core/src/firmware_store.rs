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
                let target = self
                    .connected_candidate
                    .clone()
                    .filter(mi_transport::is_exclusive_vendor_candidate);
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
                if let Some(target) = target {
                    let tx = self
                        .firmware_update
                        .start("bootloading", "设备控制已暂停，正在请求进入引导模式…");
                    let runtime = self.tokio_runtime.clone();
                    std::thread::spawn(move || {
                        let result = runtime.block_on(async {
                            use mi_transport::HidTransport;
                            let transport = mi_transport::MacTransport::exclusive_vendor();
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
                        let _ = tx.send(UpdateEvent::Bootloader(result));
                    });
                } else {
                    self.firmware_update.projection.message = "设备控制已暂停。若设备已处于引导模式，请查找并选择其串口；否则请先结束流程并连接 Creator Micro 2。".into();
                    self.firmware_update.projection.phase = "prepared".into();
                }
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
}
