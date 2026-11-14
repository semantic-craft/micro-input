use crate::firmware_catalog::{self, DownloadedFirmware, FirmwareChannel, FirmwareRelease};
use crate::firmware_flash::{self, BootloaderPort};
use mi_proto::v070_capabilities::{
    DeviceNameInfo, JoystickDeadzone, MouseConfig, MouseSpeedPreset, PmConfig, PmState,
    SaRgbEffect, MAX_DEVICE_NAME_LEN,
};
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::mpsc};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct V070MouseConfigProjection {
    pub preset: String,
    pub delay_ms: u16,
    pub interval_ms: u16,
    pub move_delta: u16,
    pub max_speed: u16,
    pub time_to_max: u16,
    pub wheel_delay_ms: u16,
    pub wheel_interval_ms: u16,
    pub wheel_delta: u16,
    pub wheel_max_speed: u16,
    pub wheel_time_to_max: u16,
    pub stick_speed: u16,
    pub stick_regulator: u16,
    pub stick_scroll: u16,
    pub deadzone: f64,
}

impl Default for V070MouseConfigProjection {
    fn default() -> Self {
        Self::from_mouse_config(&MouseConfig::default(), JoystickDeadzone::DEFAULT)
    }
}

impl V070MouseConfigProjection {
    pub fn from_mouse_config(cfg: &MouseConfig, deadzone: f64) -> Self {
        let preset = cfg
            .matching_preset()
            .map(MouseSpeedPreset::as_str)
            .unwrap_or("custom")
            .to_string();
        let safe_deadzone = if deadzone.is_finite() {
            deadzone
        } else {
            JoystickDeadzone::DEFAULT
        };
        Self {
            preset,
            delay_ms: cfg.delay_ms,
            interval_ms: cfg.interval_ms,
            move_delta: cfg.move_delta,
            max_speed: cfg.max_speed,
            time_to_max: cfg.time_to_max,
            wheel_delay_ms: cfg.wheel_delay_ms,
            wheel_interval_ms: cfg.wheel_interval_ms,
            wheel_delta: cfg.wheel_delta,
            wheel_max_speed: cfg.wheel_max_speed,
            wheel_time_to_max: cfg.wheel_time_to_max,
            stick_speed: cfg.stick_speed,
            stick_regulator: cfg.stick_regulator,
            stick_scroll: cfg.stick_scroll,
            deadzone: safe_deadzone,
        }
    }

    pub fn to_mouse_config(&self) -> MouseConfig {
        MouseConfig {
            delay_ms: self.delay_ms,
            interval_ms: self.interval_ms,
            move_delta: self.move_delta,
            max_speed: self.max_speed,
            time_to_max: self.time_to_max,
            wheel_delay_ms: self.wheel_delay_ms,
            wheel_interval_ms: self.wheel_interval_ms,
            wheel_delta: self.wheel_delta,
            wheel_max_speed: self.wheel_max_speed,
            wheel_time_to_max: self.wheel_time_to_max,
            stick_speed: self.stick_speed,
            stick_regulator: self.stick_regulator,
            stick_scroll: self.stick_scroll,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V070PmConfigProjection {
    pub auto_sleep: bool,
    pub host_sleep: bool,
    pub sleep_after_min: u16,
}

impl Default for V070PmConfigProjection {
    fn default() -> Self {
        let defaults = PmConfig::default();
        Self::from_pm_config(&defaults)
    }
}

impl V070PmConfigProjection {
    pub fn from_pm_config(cfg: &PmConfig) -> Self {
        Self {
            auto_sleep: cfg.auto_sleep,
            host_sleep: cfg.host_sleep,
            sleep_after_min: cfg.sleep_after_min,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V070PmStateHoldProjection {
    pub owner: String,
    pub floor: String,
    pub age_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct V070PmStateProjection {
    pub state: String,
    pub floor: String,
    pub base_floor: String,
    pub line_powered: bool,
    pub auto_sleep: bool,
    pub sleep_after_s: u32,
    pub idle_s: u32,
    pub standby_s: u32,
    pub credit_s: u32,
    pub usb_host_hold: bool,
    pub holds: Vec<V070PmStateHoldProjection>,
}

impl V070PmStateProjection {
    pub fn from_pm_state(state: &PmState) -> Self {
        Self {
            state: state.state.clone(),
            floor: state.floor.clone(),
            base_floor: state.base_floor.clone(),
            line_powered: state.line_powered,
            auto_sleep: state.auto_sleep,
            sleep_after_s: state.sleep_after_s,
            idle_s: state.idle_s,
            standby_s: state.standby_s,
            credit_s: state.credit_s,
            usb_host_hold: state.usb_host_hold().is_some(),
            holds: state
                .holds
                .iter()
                .map(|h| V070PmStateHoldProjection {
                    owner: h.owner.clone(),
                    floor: h.floor.clone(),
                    age_ms: h.age_ms,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V070DeviceNameProjection {
    pub name: String,
    pub factory_name: String,
    pub max_length: usize,
}

impl Default for V070DeviceNameProjection {
    fn default() -> Self {
        Self {
            name: String::new(),
            factory_name: String::new(),
            max_length: MAX_DEVICE_NAME_LEN,
        }
    }
}

impl V070DeviceNameProjection {
    pub fn from_device_name_info(info: &DeviceNameInfo) -> Self {
        Self {
            name: info.name.clone(),
            factory_name: info.factory_name.clone(),
            max_length: info.max_length,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct V070SaRgbProjection {
    pub sa: Option<u32>,
    pub color_hex: String,
    pub brightness: f64,
    pub speed: f64,
    pub effect: u8,
}

impl Default for V070SaRgbProjection {
    fn default() -> Self {
        Self {
            sa: None,
            color_hex: "#FFFFFF".into(),
            brightness: 1.0,
            speed: 0.5,
            effect: SaRgbEffect::Solid.as_u8(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct V070HardwareProjection {
    pub supported: bool,
    pub busy: bool,
    pub synced: bool,
    pub active_profile: Option<u8>,
    pub active_layer: Option<u8>,
    pub battery_percent: Option<u8>,
    pub is_charging: Option<bool>,
    pub host_sleep_status: String,
    pub status_message: String,
    pub last_error: Option<String>,
    #[serde(default)]
    pub auto_layer_sync_enabled: bool,
    #[serde(default)]
    pub preferred_device_serial: Option<String>,
    pub mouse: V070MouseConfigProjection,
    pub pm_config: V070PmConfigProjection,
    pub pm_state: V070PmStateProjection,
    pub device_name: V070DeviceNameProjection,
    pub sa_rgb: V070SaRgbProjection,
}

impl Default for V070HardwareProjection {
    fn default() -> Self {
        Self {
            supported: false,
            busy: false,
            synced: false,
            active_profile: None,
            active_layer: None,
            battery_percent: None,
            is_charging: None,
            host_sleep_status: "awake".into(),
            status_message: "需连接 0.7.0-rc.16+ 独占固件设备".into(),
            last_error: None,
            auto_layer_sync_enabled: false,
            preferred_device_serial: None,
            mouse: V070MouseConfigProjection::default(),
            pm_config: V070PmConfigProjection::default(),
            pm_state: V070PmStateProjection::default(),
            device_name: V070DeviceNameProjection::default(),
            sa_rgb: V070SaRgbProjection::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct V070SnapshotPayload {
    pub session_id: u64,
    pub version: String,
    pub active_profile: Option<u8>,
    pub active_layer: Option<u8>,
    pub battery_percent: Option<u8>,
    pub is_charging: Option<bool>,
    pub mouse: MouseConfig,
    pub deadzone: f64,
    pub pm_config: PmConfig,
    pub pm_state: PmState,
    pub device_name: DeviceNameInfo,
}

#[derive(Debug, Clone)]
pub enum V070BridgeOutcome {
    Snapshot(V070SnapshotPayload),
    ProfileLayer {
        profile_index: Option<u8>,
        layer_index: u8,
    },
    MouseUpdated {
        mouse: MouseConfig,
        deadzone: f64,
    },
    PmConfigUpdated {
        pm_config: PmConfig,
    },
    DeviceRenamed {
        device_name: DeviceNameInfo,
    },
    SaRgbApplied {
        sa: u32,
        color_hex: String,
        brightness: f64,
        speed: f64,
        effect: u8,
    },
    SaRgbReset {
        sa: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum V070HardwareAction {
    Refresh,
    SetAutoLayerSync {
        enabled: bool,
    },
    SelectLayer {
        layer: u8,
    },
    ApplyMousePreset {
        preset: String,
    },
    ApplyMouseConfig {
        move_delta: u16,
        max_speed: u16,
        wheel_delta: u16,
        wheel_max_speed: u16,
        stick_speed: u16,
        stick_scroll: u16,
        deadzone: f64,
    },
    ApplyPmConfig {
        auto_sleep: bool,
        host_sleep: bool,
        sleep_after_min: u16,
    },
    RenameDevice {
        name: String,
    },
    ApplySaRgb {
        sa: u32,
        color_hex: String,
        brightness: f64,
        speed: f64,
        effect: u8,
    },
    ResetSaRgb {
        sa: u32,
    },
}

pub(crate) fn parse_hex_color(raw: &str) -> Result<u32, String> {
    let trimmed = raw
        .trim()
        .strip_prefix('#')
        .or_else(|| raw.trim().strip_prefix("0x"))
        .or_else(|| raw.trim().strip_prefix("0X"))
        .unwrap_or(raw.trim());
    if trimmed.len() != 6 || !trimmed.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("RGB 颜色必须为 6 位十六进制（例如 #33AAFF）".into());
    }
    u32::from_str_radix(trimmed, 16).map_err(|_| "RGB 颜色格式无效".into())
}

pub(crate) fn format_hex_color(rgb: u32) -> String {
    format!("#{:06X}", rgb & 0xFF_FFFF)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FirmwareAction {
    Check { channel: FirmwareChannel },
    Download,
    PrepareDevice,
    ScanPorts,
    SelectPort { path: String },
    Install,
    Finish,
    V070Hardware(V070HardwareAction),
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
    #[serde(default)]
    pub v070: V070HardwareProjection,
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
            v070: V070HardwareProjection::default(),
        }
    }
}

pub(crate) enum UpdateEvent {
    Release(Result<FirmwareRelease, String>),
    Download(Result<DownloadedFirmware, String>),
    Ports(Result<Vec<BootloaderPort>, String>),
    Bootloader(Result<Vec<BootloaderPort>, String>),
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
                match result {
                    Ok(ports) => {
                        self.projection.message = if ports.is_empty() {
                            "已请求进入引导模式，暂未自动发现串口，请点击「查找串口」确认".into()
                        } else {
                            "已进入引导模式并发现以下串口，请核对并手动选择设备串口".into()
                        };
                        self.projection.ports = ports;
                        self.projection.selected_port = None;
                        self.projection.phase = "select_port".into();
                    }
                    Err(error) => {
                        self.projection.phase = "bootloader_requested".into();
                        self.projection.message = format!(
                            "引导模式请求结果未确认：{error}。请查找串口确认设备状态；不会自动重发。"
                        );
                    }
                }
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

    #[test]
    fn bootloader_discovery_populates_ports_without_auto_selecting() {
        let mut updater = FirmwareUpdater::default();
        updater.projection.device_reserved = true;
        updater.projection.busy = true;
        updater.projection.selected_port = Some("/dev/cu.stale".into());
        let port = BootloaderPort {
            path: "/dev/cu.usbmodem101".into(),
            vid: 0x303a,
            pid: 0x1001,
            serial_number: Some("ABC123".into()),
            manufacturer: Some("Espressif".into()),
            product: Some("USB JTAG".into()),
        };
        updater.accept(UpdateEvent::Bootloader(Ok(vec![port.clone()])));
        assert!(!updater.projection.busy);
        assert_eq!(updater.projection.phase, "select_port");
        assert!(updater.projection.selected_port.is_none());
        assert_eq!(updater.projection.ports, vec![port]);

        updater.accept(UpdateEvent::Bootloader(Err("timeout".into())));
        assert_eq!(updater.projection.phase, "bootloader_requested");
        assert!(updater.projection.message.contains("timeout"));
    }
}
