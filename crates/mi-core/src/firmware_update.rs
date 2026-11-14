use crate::firmware_catalog::{self, DownloadedFirmware, FirmwareChannel, FirmwareRelease};
use crate::firmware_flash::{self, BootloaderPort};
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::mpsc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FirmwareAction {
    Check { channel: FirmwareChannel },
    Download,
    PrepareDevice,
    ScanPorts,
    SelectPort { path: String },
    Install,
    Finish,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FirmwareProjection {
    pub channel: FirmwareChannel,
    pub current_version: Option<String>,
    pub version_status: String,
    pub release: Option<FirmwareRelease>,
    pub phase: String,
    pub message: String,
    pub busy: bool,
    pub downloaded: bool,
    pub digest_verified: bool,
    pub device_reserved: bool,
    pub ports: Vec<BootloaderPort>,
    pub selected_port: Option<String>,
    pub progress: Option<f64>,
    pub backup_path: Option<String>,
}
impl Default for FirmwareProjection {
    fn default() -> Self {
        Self {
            channel: FirmwareChannel::Stable,
            current_version: None,
            version_status: "当前运行版本尚未读取".into(),
            release: None,
            phase: "idle".into(),
            message: "检查 Creator Micro 2 官方稳定版固件".into(),
            busy: false,
            downloaded: false,
            digest_verified: false,
            device_reserved: false,
            ports: vec![],
            selected_port: None,
            progress: None,
            backup_path: None,
        }
    }
}

pub(crate) enum UpdateEvent {
    Release(Result<FirmwareRelease, String>),
    Download(Result<DownloadedFirmware, String>),
    Ports(Result<Vec<BootloaderPort>, String>),
    Bootloader(Result<(), String>),
    Progress(firmware_flash::FlashProgress),
    Flashed(Result<firmware_flash::FlashReceipt, String>),
}

#[derive(Default)]
pub(crate) struct FirmwareUpdater {
    pub projection: FirmwareProjection,
    pub resume_auto_connect: Option<bool>,
    downloaded: Option<DownloadedFirmware>,
    events: Option<mpsc::Receiver<UpdateEvent>>,
}
impl FirmwareUpdater {
    pub fn reserved(&self) -> bool {
        self.projection.device_reserved
    }
    pub fn start(&mut self, phase: &str, message: &str) -> mpsc::Sender<UpdateEvent> {
        let (tx, rx) = mpsc::channel();
        self.events = Some(rx);
        self.projection.busy = true;
        self.projection.phase = phase.into();
        self.projection.message = message.into();
        self.projection.progress = None;
        tx
    }
    pub fn check(&mut self, channel: FirmwareChannel) {
        self.projection.channel = channel;
        self.projection.release = None;
        self.projection.downloaded = false;
        self.projection.digest_verified = false;
        self.downloaded = None;
        let tx = self.start("checking", "正在读取官方固件发布…");
        std::thread::spawn(move || {
            let _ = tx.send(UpdateEvent::Release(firmware_catalog::fetch_release(
                channel,
            )));
        });
    }
    pub fn download(&mut self, root: &Path) -> Result<(), String> {
        let release = self.projection.release.clone().ok_or("请先检查固件更新")?;
        let directory = root.join("firmware-downloads");
        std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        let destination = directory.join(format!(
            "{}-{}.bin",
            release.version,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos()
        ));
        let tx = self.start("downloading", "正在下载并校验官方固件…");
        std::thread::spawn(move || {
            let _ = tx.send(UpdateEvent::Download(firmware_catalog::download_release(
                &release,
                &destination,
            )));
        });
        Ok(())
    }
    pub fn scan(&mut self) {
        self.projection.selected_port = None;
        self.projection.ports.clear();
        let tx = self.start("scanning", "正在查找引导模式设备…");
        std::thread::spawn(move || {
            let _ = tx.send(UpdateEvent::Ports(firmware_flash::enumerate_bootloaders()));
        });
    }
    pub fn install(&mut self, root: &Path) -> Result<(), String> {
        if !self.reserved() {
            return Err("请先暂停设备控制并准备更新".into());
        }
        let firmware = self.downloaded.clone().ok_or("尚未下载固件")?;
        if !firmware.digest_verified {
            return Err("官方未提供可核验摘要，不能自动安装".into());
        }
        let port = self
            .projection
            .ports
            .iter()
            .find(|p| Some(&p.path) == self.projection.selected_port.as_ref())
            .cloned()
            .ok_or("请选择这把 Creator Micro 2 对应的引导模式串口")?;
        let backup_root = root.join("firmware-backups");
        self.projection.backup_path = None;
        let tx = self.start("connecting", "正在连接所选设备；完整备份后才会写入…");
        std::thread::spawn(move || {
            let result = firmware_flash::flash(&port, &firmware, &backup_root, |progress| {
                let _ = tx.send(UpdateEvent::Progress(progress));
            });
            let _ = tx.send(UpdateEvent::Flashed(result));
        });
        Ok(())
    }
    pub fn poll(&mut self) {
        loop {
            let event = match self.events.as_ref().map(|rx| rx.try_recv()) {
                Some(Ok(event)) => event,
                Some(Err(mpsc::TryRecvError::Disconnected)) => {
                    self.events = None;
                    if self.projection.busy {
                        self.fail("后台操作中断；设备状态尚未确认".into());
                    }
                    break;
                }
                _ => break,
            };
            self.accept(event);
        }
    }
    fn fail(&mut self, error: String) {
        self.projection.busy = false;
        self.projection.phase = "failed".into();
        self.projection.message = error;
        self.projection.progress = None;
    }
    fn accept(&mut self, event: UpdateEvent) {
        match event {
            UpdateEvent::Release(Ok(release)) => {
                self.projection.message = format!(
                    "官方{}：{}",
                    if release.prerelease {
                        "预发布版"
                    } else {
                        "稳定版"
                    },
                    release.tag
                );
                self.projection.release = Some(release);
                self.projection.phase = "available".into();
                self.projection.busy = false;
            }
            UpdateEvent::Download(Ok(download)) => {
                self.projection.downloaded = true;
                self.projection.digest_verified = download.digest_verified;
                self.projection.message = if download.digest_verified {
                    "下载完成，SHA-256 与官方附件摘要一致"
                } else {
                    "下载完成；官方未提供摘要，仅记录本地 SHA-256"
                }
                .into();
                self.downloaded = Some(download);
                self.projection.phase = "downloaded".into();
                self.projection.busy = false;
            }
            UpdateEvent::Ports(Ok(ports)) => {
                self.projection.message = if ports.is_empty() {
                    "尚未发现引导模式串口，可稍后重新查找"
                } else {
                    "请明确选择这把 Creator Micro 2 的串口；Espressif 名称本身不能确认型号"
                }
                .into();
                self.projection.ports = ports;
                self.projection.phase = "select_port".into();
                self.projection.busy = false;
            }
            UpdateEvent::Bootloader(result) => {
                self.projection.busy = false;
                self.projection.phase = "bootloader_requested".into();
                self.projection.message = match result {
                    Ok(()) => "已请求进入引导模式，请查找并选择设备串口".into(),
                    Err(error) => format!(
                        "引导模式请求结果未确认：{error}。请查找串口确认设备状态；不会自动重发。"
                    ),
                };
            }
            UpdateEvent::Progress(progress) => {
                if let Some(path) = progress.backup_dir {
                    self.projection.backup_path = Some(path.to_string_lossy().into_owned());
                }
                self.projection.phase = progress.phase.clone();
                self.projection.message = match progress.phase.as_str() {
                    "connecting" => "正在连接设备…",
                    "backing_up" => "正在保存完整 Flash 备份，请保持连接…",
                    "writing" => "正在写入固件，请勿断开设备或退出应用…",
                    "verifying" => "正在校验写入结果…",
                    "restarting" => "正在重启设备…",
                    _ => "正在更新固件…",
                }
                .into();
                self.projection.progress =
                    (progress.total > 0).then(|| progress.completed as f64 / progress.total as f64);
            }
            UpdateEvent::Flashed(Ok(receipt)) => {
                self.projection.backup_path =
                    Some(receipt.backup_dir.to_string_lossy().into_owned());
                self.projection.busy = false;
                self.projection.phase = "flash_verified".into();
                self.projection.message =
                    "固件写入校验通过；运行版本与原键位尚待重连核对。完整 Flash 备份已保留。"
                        .into();
                self.projection.selected_port = None;
                self.projection.ports.clear();
            }
            UpdateEvent::Release(Err(e))
            | UpdateEvent::Download(Err(e))
            | UpdateEvent::Ports(Err(e))
            | UpdateEvent::Flashed(Err(e)) => self.fail(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disconnect_during_worker_never_reports_success_or_releases_device() {
        let mut updater = FirmwareUpdater::default();
        updater.projection.device_reserved = true;
        drop(updater.start("writing", "写入中"));
        updater.poll();
        assert_eq!(updater.projection.phase, "failed");
        assert!(updater.reserved());
        assert!(!updater.projection.busy);
    }
    #[test]
    fn discovery_never_selects_a_port_and_install_requires_reservation() {
        let mut updater = FirmwareUpdater::default();
        updater.accept(UpdateEvent::Ports(Ok(vec![])));
        assert!(updater.projection.selected_port.is_none());
        assert!(updater.install(Path::new("unused")).is_err());
        assert_eq!(updater.projection.channel, FirmwareChannel::Stable);
    }
}
