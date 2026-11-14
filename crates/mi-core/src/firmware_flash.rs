use crate::firmware_catalog::DownloadedFirmware;
use espflash::{
    connection::{Connection, ResetAfterOperation, ResetBeforeOperation},
    flasher::Flasher,
    target::{Chip, ProgressCallbacks},
};
use serde::{Deserialize, Serialize};
use serialport::{SerialPort, SerialPortInfo, SerialPortType, UsbPortInfo};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BootloaderPort {
    pub path: String,
    pub vid: u16,
    pub pid: u16,
    pub serial_number: Option<String>,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
}

impl BootloaderPort {
    fn from_info(info: SerialPortInfo) -> Option<Self> {
        let SerialPortType::UsbPort(usb) = info.port_type else {
            return None;
        };
        // Input 1.1.0 identifies an Espressif loader, not a Work Louder model.
        if usb.vid != 0x303a || usb.manufacturer.as_deref() != Some("Espressif") {
            return None;
        }
        #[cfg(target_os = "macos")]
        if !info.port_name.starts_with("/dev/cu.") {
            return None;
        }
        Some(Self {
            path: info.port_name,
            vid: usb.vid,
            pid: usb.pid,
            serial_number: usb.serial_number,
            manufacturer: usb.manufacturer,
            product: usb.product,
        })
    }

    fn usb_info(&self) -> UsbPortInfo {
        UsbPortInfo {
            vid: self.vid,
            pid: self.pid,
            serial_number: self.serial_number.clone(),
            manufacturer: self.manufacturer.clone(),
            product: self.product.clone(),
        }
    }
}

pub fn enumerate_bootloaders() -> Result<Vec<BootloaderPort>, String> {
    let mut ports: Vec<_> = serialport::available_ports()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter_map(BootloaderPort::from_info)
        .collect();
    ports.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(ports)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlashProgress {
    pub phase: String,
    pub completed: usize,
    pub total: usize,
    pub backup_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlashReceipt {
    pub backup_dir: PathBuf,
    pub backup_sha256: String,
    pub firmware_sha256: String,
    pub flash_size: u32,
    pub status: String,
}

#[derive(Serialize)]
struct Operation<'a> {
    schema_version: u32,
    port: &'a BootloaderPort,
    receipt: &'a FlashReceipt,
    error: Option<&'a str>,
}

fn persist(
    port: &BootloaderPort,
    receipt: &FlashReceipt,
    error: Option<&str>,
) -> Result<(), String> {
    let temp = receipt.backup_dir.join("operation.json.next");
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(&temp).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec_pretty(&Operation {
        schema_version: 1,
        port,
        receipt,
        error,
    })
    .map_err(|e| e.to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    fs::rename(&temp, receipt.backup_dir.join("operation.json")).map_err(|e| e.to_string())?;
    File::open(&receipt.backup_dir)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())
}

fn image_bytes(firmware: &DownloadedFirmware) -> Result<Vec<u8>, String> {
    if !firmware.digest_verified {
        return Err("此资产没有通过官方 SHA-256 校验，不能自动安装".into());
    }
    let bytes = fs::read(&firmware.path).map_err(|e| e.to_string())?;
    if bytes.len() as u64 != firmware.size
        || format!("{:x}", Sha256::digest(&bytes)) != firmware.sha256
    {
        return Err("下载后的固件文件已变化，请重新下载".into());
    }
    if bytes.len() < 24 || bytes[0] != 0xe9 || u16::from_le_bytes([bytes[12], bytes[13]]) != 9 {
        return Err("固件不是 Creator Micro 2 使用的 ESP32-S3 镜像".into());
    }
    Ok(bytes)
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct DataPartition {
    subtype: u8,
    offset: u32,
    size: u32,
    name: Vec<u8>,
    flags: u32,
}

fn data_partitions(bytes: &[u8]) -> Result<Vec<DataPartition>, String> {
    let table = bytes.get(0x8000..0x9000).ok_or("固件缺少完整分区表")?;
    let mut partitions = Vec::new();
    for entry in table.chunks_exact(32) {
        if entry[..2] == [0xff, 0xff] || entry[..2] == [0xeb, 0xeb] {
            break;
        }
        if entry[..2] != [0xaa, 0x50] {
            return Err("分区表无法识别，不能确认配置保留范围".into());
        }
        // ESP-IDF data partitions except the PHY calibration slot are persistent state.
        if entry[2] == 1 && entry[3] != 1 {
            let offset = u32::from_le_bytes(entry[4..8].try_into().unwrap());
            let size = u32::from_le_bytes(entry[8..12].try_into().unwrap());
            if size == 0 || offset.checked_add(size).is_none() {
                return Err("数据分区范围无效".into());
            }
            let name = entry[12..28]
                .iter()
                .copied()
                .take_while(|b| *b != 0)
                .collect();
            partitions.push(DataPartition {
                subtype: entry[3],
                offset,
                size,
                name,
                flags: u32::from_le_bytes(entry[28..32].try_into().unwrap()),
            });
        }
    }
    if partitions.is_empty() {
        return Err("未找到配置数据分区，不能确认配置保留范围".into());
    }
    partitions.sort();
    Ok(partitions)
}

fn verify_preserved_partitions(original: &[u8], image: &[u8]) -> Result<(), String> {
    let old = data_partitions(original)?;
    let new = data_partitions(image)?;
    if old != new {
        return Err("新旧固件的数据分区布局不同；需要专门迁移流程，未写入".into());
    }
    for partition in old {
        if partition.offset as usize >= original.len()
            || partition.offset as u64 + partition.size as u64 > original.len() as u64
        {
            return Err("当前数据分区超出 Flash 备份范围，未写入".into());
        }
        // Erase happens in 4 KiB sectors, including padding past the final image byte.
        let erased_end = image.len().checked_add(0xfff).ok_or("固件范围溢出")? & !0xfff;
        if erased_end > partition.offset as usize {
            return Err("固件写入范围覆盖现有数据分区，未写入".into());
        }
    }
    Ok(())
}

fn verify_flash_capacity(image: &[u8], detected: u32) -> Result<(), String> {
    let code = image.get(3).ok_or("镜像缺少 Flash 容量声明")? >> 4;
    let declared = (code <= 8)
        .then(|| 0x100000u32 << code)
        .ok_or("镜像 Flash 容量声明无法识别")?;
    if detected < declared || image.len() > detected as usize {
        return Err("探测到的 Flash 容量不足以匹配官方镜像，未写入".into());
    }
    Ok(())
}

struct Awake(Child);
impl Awake {
    fn acquire() -> Result<Self, String> {
        Command::new("/usr/bin/caffeinate")
            .args(["-i", "-s", "-w", &std::process::id().to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(Self)
            .map_err(|e| format!("不能阻止更新期间休眠：{e}"))
    }
}
impl Drop for Awake {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Progress<'a, F> {
    report: &'a mut F,
    total: usize,
}
impl<F: FnMut(FlashProgress)> ProgressCallbacks for Progress<'_, F> {
    fn init(&mut self, _addr: u32, total: usize) {
        self.total = total;
        (self.report)(FlashProgress {
            phase: "writing".into(),
            completed: 0,
            total,
            backup_dir: None,
        });
    }
    fn update(&mut self, current: usize) {
        (self.report)(FlashProgress {
            phase: "writing".into(),
            completed: current,
            total: self.total,
            backup_dir: None,
        });
    }
    fn verifying(&mut self) {
        (self.report)(FlashProgress {
            phase: "verifying".into(),
            completed: self.total,
            total: self.total,
            backup_dir: None,
        });
    }
    fn finish(&mut self, _skipped: bool) {
        (self.report)(FlashProgress {
            phase: "restarting".into(),
            completed: self.total,
            total: self.total,
            backup_dir: None,
        });
    }
}

/// The caller must obtain an explicit model/port confirmation and suspend its HID sessions.
/// Success verifies flash bytes, not the restarted firmware or preserved device configuration.
pub fn flash(
    port: &BootloaderPort,
    firmware: &DownloadedFirmware,
    backup_root: &Path,
    mut report: impl FnMut(FlashProgress),
) -> Result<FlashReceipt, String> {
    let bytes = image_bytes(firmware)?;
    if !enumerate_bootloaders()?.contains(port) {
        return Err("所选引导端口已经断开或身份变化，请重新选择".into());
    }
    let _awake = Awake::acquire()?;
    fs::create_dir_all(backup_root).map_err(|e| e.to_string())?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let backup_dir = backup_root.join(format!("firmware-{stamp}-{}", std::process::id()));
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    builder.mode(0o700);
    builder.create(&backup_dir).map_err(|e| e.to_string())?;
    let mut receipt = FlashReceipt {
        backup_dir,
        backup_sha256: String::new(),
        firmware_sha256: firmware.sha256.clone(),
        flash_size: 0,
        status: "not_written".into(),
    };
    persist(port, &receipt, None)?;
    let result: Result<(), String> = (|| {
        report(FlashProgress {
            phase: "connecting".into(),
            completed: 0,
            total: 0,
            backup_dir: Some(receipt.backup_dir.clone()),
        });
        let serial = serialport::new(&port.path, 115_200)
            .timeout(Duration::from_secs(3))
            .open_native()
            .map_err(|e| e.to_string())?;
        let connection = Connection::new(
            serial,
            port.usb_info(),
            ResetAfterOperation::NoResetNoStub,
            ResetBeforeOperation::NoReset,
            115_200,
        );
        let mut flasher = Flasher::connect(
            connection,
            true,
            true,
            false,
            Some(Chip::Esp32s3),
            Some(921_600),
        )
        .map_err(|e| e.to_string())?;
        if flasher.secure_download_mode() {
            return Err("安全下载模式不支持此更新流程，未写入固件".into());
        }
        receipt.flash_size = flasher
            .device_info()
            .map_err(|e| e.to_string())?
            .flash_size
            .size();
        verify_flash_capacity(&bytes, receipt.flash_size)?;
        report(FlashProgress {
            phase: "backing_up".into(),
            completed: 0,
            total: receipt.flash_size as usize,
            backup_dir: None,
        });
        let backup = receipt.backup_dir.join("flash-before.bin");
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        options.open(&backup).map_err(|e| e.to_string())?;
        flasher
            .read_flash(0, receipt.flash_size, 0x1000, 64, backup.clone())
            .map_err(|e| e.to_string())?;
        File::open(&backup)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        let original = fs::read(&backup).map_err(|e| e.to_string())?;
        if original.len() != receipt.flash_size as usize {
            return Err("完整 Flash 备份长度不符，未写入".into());
        }
        receipt.backup_sha256 = format!("{:x}", Sha256::digest(&original));
        receipt.status = "backup_verified_not_written".into();
        persist(port, &receipt, None)?;
        verify_preserved_partitions(&original, &bytes)?;
        receipt.status = "writing_device_state_unknown".into();
        persist(port, &receipt, None)?;
        flasher
            .write_bin_to_flash(
                0,
                &bytes,
                &mut Progress {
                    report: &mut report,
                    total: 0,
                },
            )
            .map_err(|e| e.to_string())?;
        receipt.status = "flash_verified_restart_pending".into();
        persist(port, &receipt, None)?;
        // Input 1.1.0 clears this ESP32-S3 register before toggling RTS (K:13730–13743).
        let mut connection = flasher.into_connection();
        connection
            .write_reg(0x6000_812c, 0, None)
            .map_err(|e| e.to_string())?;
        std::thread::sleep(Duration::from_millis(100));
        let mut serial: espflash::connection::Port = connection.into();
        serial
            .write_data_terminal_ready(false)
            .map_err(|e| e.to_string())?;
        serial
            .write_request_to_send(false)
            .map_err(|e| e.to_string())?;
        serial
            .write_request_to_send(true)
            .map_err(|e| e.to_string())?;
        std::thread::sleep(Duration::from_millis(100));
        serial
            .write_request_to_send(false)
            .map_err(|e| e.to_string())?;
        receipt.status = "flash_verified_runtime_unverified".into();
        persist(port, &receipt, None)?;
        Ok(())
    })();
    if let Err(error) = result {
        let save_error = persist(port, &receipt, Some(&error)).err();
        return Err(format!(
            "{error}；备份与操作记录：{}{}",
            receipt.backup_dir.display(),
            save_error
                .map(|e| format!("；记录保存失败：{e}"))
                .unwrap_or_default()
        ));
    }
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image_with_data_partition(length: usize, offset: u32, size: u32) -> Vec<u8> {
        let mut bytes = vec![0xff; length];
        let entry = &mut bytes[0x8000..0x8020];
        entry.fill(0);
        entry[..4].copy_from_slice(&[0xaa, 0x50, 1, 2]);
        entry[4..8].copy_from_slice(&offset.to_le_bytes());
        entry[8..12].copy_from_slice(&size.to_le_bytes());
        entry[12..15].copy_from_slice(b"nvs");
        bytes
    }

    #[test]
    fn changed_layout_and_sector_overlap_never_allow_automatic_update() {
        let original = image_with_data_partition(0x20000, 0x18000, 0x8000);
        let image = image_with_data_partition(0x10000, 0x18000, 0x8000);
        assert!(verify_preserved_partitions(&original, &image).is_ok());
        let moved = image_with_data_partition(0x10000, 0x17000, 0x8000);
        assert!(verify_preserved_partitions(&original, &moved)
            .unwrap_err()
            .contains("布局不同"));
        let overlapping = image_with_data_partition(0x18001, 0x18000, 0x8000);
        assert!(verify_preserved_partitions(&original, &overlapping)
            .unwrap_err()
            .contains("覆盖"));
        let malformed = vec![0u8; 0x10000];
        assert!(verify_preserved_partitions(&original, &malformed).is_err());
    }

    #[test]
    fn unknown_flash_id_fallback_cannot_backup_a_declared_larger_flash() {
        let mut image = vec![0; 24];
        image[3] = 0x4f;
        assert!(verify_flash_capacity(&image, 0x1000000).is_ok());
        assert!(verify_flash_capacity(&image, 0x400000).is_err());
        image[3] = 0xff;
        assert!(verify_flash_capacity(&image, 0x1000000).is_err());
    }

    #[test]
    fn loader_enumeration_never_promotes_generic_serial_ports() {
        assert!(BootloaderPort::from_info(SerialPortInfo {
            port_name: "/dev/cu.example".into(),
            port_type: SerialPortType::Unknown
        })
        .is_none());
        let mut usb = UsbPortInfo {
            vid: 0x303a,
            pid: 0x1001,
            serial_number: Some("synthetic".into()),
            manufacturer: Some("Espressif".into()),
            product: None,
        };
        assert!(BootloaderPort::from_info(SerialPortInfo {
            port_name: "/dev/cu.example".into(),
            port_type: SerialPortType::UsbPort(usb.clone())
        })
        .is_some());
        usb.manufacturer = Some("Other".into());
        assert!(BootloaderPort::from_info(SerialPortInfo {
            port_name: "/dev/cu.example".into(),
            port_type: SerialPortType::UsbPort(usb)
        })
        .is_none());
    }

    #[test]
    fn modified_download_and_wrong_chip_are_rejected_before_opening_port() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("firmware.bin");
        let mut bytes = vec![0u8; 24];
        bytes[0] = 0xe9;
        bytes[12] = 9;
        fs::write(&path, &bytes).unwrap();
        let firmware = DownloadedFirmware {
            path,
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            digest_verified: true,
            size: bytes.len() as u64,
        };
        assert!(image_bytes(&firmware).is_ok());
        let unverified = DownloadedFirmware {
            digest_verified: false,
            ..firmware.clone()
        };
        assert!(image_bytes(&unverified)
            .unwrap_err()
            .contains("官方 SHA-256"));
        bytes[12] = 5;
        fs::write(&firmware.path, &bytes).unwrap();
        assert!(image_bytes(&firmware).unwrap_err().contains("已变化"));
        let wrong_chip = DownloadedFirmware {
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            ..firmware
        };
        assert!(image_bytes(&wrong_chip).unwrap_err().contains("ESP32-S3"));
    }

    #[test]
    fn receipt_preserves_unknown_write_state_and_private_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let port = BootloaderPort {
            path: "synthetic".into(),
            vid: 0x303a,
            pid: 0x1001,
            serial_number: None,
            manufacturer: Some("Espressif".into()),
            product: None,
        };
        let receipt = FlashReceipt {
            backup_dir: dir.path().into(),
            backup_sha256: "before".into(),
            firmware_sha256: "after".into(),
            flash_size: 0x1000000,
            status: "writing_device_state_unknown".into(),
        };
        persist(&port, &receipt, Some("disconnected")).unwrap();
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.path().join("operation.json")).unwrap()).unwrap();
        assert_eq!(value["receipt"]["status"], "writing_device_state_unknown");
        assert_eq!(value["error"], "disconnected");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(dir.path().join("operation.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn persist_overwrites_stale_temp_file_and_writes_valid_operation_json() {
        let dir = tempfile::tempdir().unwrap();
        let stale = dir.path().join("operation.json.next");
        fs::write(&stale, vec![b'x'; 4096]).unwrap();
        let port = BootloaderPort {
            path: "synthetic".into(),
            vid: 0x303a,
            pid: 0x1001,
            serial_number: None,
            manufacturer: Some("Espressif".into()),
            product: None,
        };
        let receipt = FlashReceipt {
            backup_dir: dir.path().into(),
            backup_sha256: "before".into(),
            firmware_sha256: "after".into(),
            flash_size: 0x1000000,
            status: "verified".into(),
        };
        persist(&port, &receipt, None).unwrap();
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.path().join("operation.json")).unwrap()).unwrap();
        assert_eq!(value["receipt"]["status"], "verified");
        assert!(value["error"].is_null());
        assert!(!stale.exists());
    }
}
