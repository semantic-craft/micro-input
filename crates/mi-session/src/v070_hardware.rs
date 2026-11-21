//! Verified `0.7.0-rc.16+` hardware capability RPC helpers gated behind exclusive vendor ownership.

use std::fs;
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use mi_model::private_fs::{
    ensure_private_dir, open_private_file, open_verified_readonly_file,
    open_verified_readonly_file_optional, sync_private_dir, verify_existing_private_dir,
    PrivateOpenMode,
};

use mi_proto::v070_capabilities::{
    decode_device_name_response, decode_joystick_deadzone_response, decode_mouse_config_response,
    decode_pm_config_response, decode_pm_state_response, decode_set_profile_layer_response,
    decode_set_sa_rgb_response, encode_read_device_name, encode_read_joystick_deadzone,
    encode_read_mouse_config, encode_read_pm_config, encode_read_pm_state, encode_set_device_name,
    encode_set_profile_layer, encode_set_sa_rgb, encode_write_joystick_deadzone,
    encode_write_mouse_config, encode_write_pm_config, DeviceNameInfo, JoystickDeadzone,
    MouseConfig, PmConfig, PmState, ProfileLayerTarget, SaRgbUpdate, DEVICE_NAME_METHOD,
    JOYSTICK_DEADZONE_METHOD, MAX_DEVICE_NAME_LEN, MOUSE_CONFIG_METHOD, PM_CONFIG_METHOD,
    PM_STATE_METHOD, PROFILE_LAYER_SET_METHOD, SA_RGB_METHOD,
};
use mi_proto::{Method, ProtoError};
use serde_json::Value;
use sha1::{Digest, Sha1};

use crate::{DeviceSession, SessionError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoystickHardwareModeReceipt {
    pub mode: String,
    pub active_profile: Option<u8>,
    pub active_layer: Option<u8>,
    pub backup_path: PathBuf,
    pub pre_write_keymap: Vec<u8>,
    pub post_write_keymap: Vec<u8>,
    pub changed: bool,
}

type SessionKeymapCache = OnceLock<Mutex<Vec<(u64, Vec<u8>)>>>;
static SESSION_KEYMAP_CACHE: SessionKeymapCache = OnceLock::new();

fn cache_session_keymap(session_id: u64, keymap_bytes: Vec<u8>) {
    let cache = SESSION_KEYMAP_CACHE.get_or_init(|| Mutex::new(Vec::new()));
    let mut guard = cache.lock().unwrap();
    if let Some(pos) = guard.iter().position(|(id, _)| *id == session_id) {
        guard.remove(pos);
    }
    if guard.len() >= 8 {
        guard.remove(0);
    }
    guard.push((session_id, keymap_bytes));
}

fn load_cached_session_keymap(session_id: u64) -> Option<Vec<u8>> {
    let cache = SESSION_KEYMAP_CACHE.get()?;
    let guard = cache.lock().unwrap();
    guard
        .iter()
        .find(|(id, _)| *id == session_id)
        .map(|(_, bytes)| bytes.clone())
}

struct V070KeymapMutationGuard<'a> {
    session: &'a DeviceSession,
    sent: bool,
    verified: bool,
}

impl Drop for V070KeymapMutationGuard<'_> {
    fn drop(&mut self) {
        if self.sent && !self.verified {
            self.session.disconnect();
        }
    }
}

fn read_verified_file_bounded(path: &Path, max_bytes: u64) -> Result<Vec<u8>, String> {
    let file = open_verified_readonly_file(path)?;
    read_open_file_bounded(file, max_bytes)
}

fn read_open_file_bounded(file: fs::File, max_bytes: u64) -> Result<Vec<u8>, String> {
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if meta.len() > max_bytes {
        return Err(format!(
            "备份文件超过大小上限（{} > {} 字节）",
            meta.len(),
            max_bytes
        ));
    }
    let mut buf = Vec::with_capacity(meta.len() as usize);
    file.take(max_bytes + 1)
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    if buf.len() as u64 > max_bytes {
        return Err(format!("备份文件超过大小上限（>{max_bytes} 字节）"));
    }
    Ok(buf)
}

fn write_verified_immutable_file(path: &Path, bytes: &[u8]) -> Result<bool, String> {
    let max_bytes = (bytes.len() as u64).max(mi_proto::mutation::MAX_FILE_BYTES as u64);
    match open_private_file(path, PrivateOpenMode::CreateNew) {
        Ok(mut file) => {
            file.write_all(bytes)
                .and_then(|()| file.sync_all())
                .map_err(|e| e.to_string())?;
            let readback = read_verified_file_bounded(path, max_bytes)?;
            if readback != bytes {
                return Err(format!("备份文件写后读回不一致：{}", path.display()));
            }
            Ok(true)
        }
        Err(create_err) => {
            let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) else {
                return Err(create_err);
            };
            let Some(file_name) = path.file_name() else {
                return Err(create_err);
            };
            let verified_parent = verify_existing_private_dir(parent).map_err(|_| create_err)?;
            let verified_path = verified_parent.join(file_name);
            let file = open_verified_readonly_file(&verified_path)?;
            #[cfg(unix)]
            {
                let meta = file.metadata().map_err(|e| e.to_string())?;
                if (meta.permissions().mode() & 0o777) != 0o600 {
                    file.set_permissions(fs::Permissions::from_mode(0o600))
                        .map_err(|e| e.to_string())?;
                }
            }
            let existing = read_open_file_bounded(file, max_bytes)?;
            if existing != bytes {
                return Err(format!("备份文件摘要内容不符：{}", path.display()));
            }
            Ok(false)
        }
    }
}

fn persist_immutable_v070_keymap_backup(
    document_dir: &Path,
    keymap_bytes: &[u8],
) -> Result<PathBuf, String> {
    ensure_private_dir(document_dir)?;
    let backup_dir = document_dir.join("v070-keymap-backups");
    ensure_private_dir(&backup_dir)?;

    let original_anchor = backup_dir.join("original-keymap.json");
    let original_created = match open_verified_readonly_file_optional(&original_anchor)? {
        Some(file) => {
            #[cfg(unix)]
            {
                let meta = file.metadata().map_err(|e| e.to_string())?;
                if (meta.permissions().mode() & 0o777) != 0o600 {
                    file.set_permissions(fs::Permissions::from_mode(0o600))
                        .map_err(|e| e.to_string())?;
                }
            }
            false
        }
        None => write_verified_immutable_file(&original_anchor, keymap_bytes)?,
    };

    let sha1_hex = format!("{:x}", Sha1::digest(keymap_bytes));
    let snapshot_path = backup_dir.join(format!("keymap-{sha1_hex}.json"));
    let snapshot_created = write_verified_immutable_file(&snapshot_path, keymap_bytes)?;
    if original_created || snapshot_created {
        sync_private_dir(&backup_dir)?;
        sync_private_dir(document_dir)?;
    }
    Ok(snapshot_path)
}

impl DeviceSession {
    async fn exchange_v070<E>(
        &self,
        method: &'static str,
        encode: E,
    ) -> Result<(u16, Value), SessionError>
    where
        E: FnOnce(u16) -> Result<Vec<u8>, ProtoError>,
    {
        self.guarded(async {
            let mut q = self.queue.lock().await;
            let id = q.next_id;
            let encoded = if id >= 999 { Vec::new() } else { encode(id)? };
            let reply = self
                .exchange_authorized(&mut q, method, encoded, Some(method))
                .await?;
            Ok((id, reply))
        })
        .await
    }

    pub fn observed_battery_status(&self) -> (Option<u8>, Option<bool>) {
        *self.battery_status.lock().unwrap()
    }

    pub fn observed_profile_layer(&self) -> Option<(u8, u8)> {
        *self.profile_layer.lock().unwrap()
    }

    pub async fn read_mouse_config(&self) -> Result<MouseConfig, SessionError> {
        let (id, reply) = self
            .exchange_v070(MOUSE_CONFIG_METHOD, encode_read_mouse_config)
            .await?;
        Ok(decode_mouse_config_response(&reply, id)?)
    }

    pub async fn write_mouse_config(&self, cfg: &MouseConfig) -> Result<MouseConfig, SessionError> {
        cfg.validate()?;
        let (id, reply) = self
            .exchange_v070(MOUSE_CONFIG_METHOD, |id| encode_write_mouse_config(cfg, id))
            .await?;
        Ok(decode_mouse_config_response(&reply, id)?)
    }

    pub async fn read_joystick_deadzone(&self) -> Result<f64, SessionError> {
        let (id, reply) = self
            .exchange_v070(JOYSTICK_DEADZONE_METHOD, encode_read_joystick_deadzone)
            .await?;
        Ok(decode_joystick_deadzone_response(&reply, id)?.v)
    }

    pub async fn write_joystick_deadzone(&self, v: f64) -> Result<f64, SessionError> {
        let dz = JoystickDeadzone::new(v)?;
        let (id, reply) = self
            .exchange_v070(JOYSTICK_DEADZONE_METHOD, |id| {
                encode_write_joystick_deadzone(dz, id)
            })
            .await?;
        Ok(decode_joystick_deadzone_response(&reply, id)?.v)
    }

    pub async fn set_profile_layer_flashless(
        &self,
        p: Option<u8>,
        l: Option<u8>,
    ) -> Result<(), SessionError> {
        let target = ProfileLayerTarget { p, l };
        target.validate()?;
        let (id, reply) = self
            .exchange_v070(PROFILE_LAYER_SET_METHOD, |id| {
                encode_set_profile_layer(target, id)
            })
            .await?;
        decode_set_profile_layer_response(&reply, id)?;
        let mut guard = self.profile_layer.lock().unwrap();
        let current = *guard;
        let next_p = p.or_else(|| current.map(|(cp, _)| cp));
        let next_l = l.or_else(|| current.map(|(_, cl)| cl));
        if let Some((np, nl)) = next_p.zip(next_l) {
            *guard = Some((np, nl));
        }
        Ok(())
    }

    pub async fn read_pm_config(&self) -> Result<PmConfig, SessionError> {
        let (id, reply) = self
            .exchange_v070(PM_CONFIG_METHOD, encode_read_pm_config)
            .await?;
        Ok(decode_pm_config_response(&reply, id)?)
    }

    pub async fn write_pm_config(&self, cfg: &PmConfig) -> Result<PmConfig, SessionError> {
        cfg.validate()?;
        let (id, reply) = self
            .exchange_v070(PM_CONFIG_METHOD, |id| encode_write_pm_config(cfg, id))
            .await?;
        Ok(decode_pm_config_response(&reply, id)?)
    }

    pub async fn read_pm_state(&self) -> Result<PmState, SessionError> {
        let (id, reply) = self
            .exchange_v070(PM_STATE_METHOD, encode_read_pm_state)
            .await?;
        Ok(decode_pm_state_response(&reply, id)?)
    }

    pub async fn read_device_name(&self) -> Result<DeviceNameInfo, SessionError> {
        let (id, reply) = self
            .exchange_v070(DEVICE_NAME_METHOD, encode_read_device_name)
            .await?;
        let info = decode_device_name_response(&reply, id)?;
        *self.device_name_max_length.lock().unwrap() = Some(info.max_length);
        Ok(info)
    }

    pub async fn write_device_name(&self, name: &str) -> Result<DeviceNameInfo, SessionError> {
        let max_length = self
            .device_name_max_length
            .lock()
            .unwrap()
            .unwrap_or(MAX_DEVICE_NAME_LEN);
        let (id, reply) = self
            .exchange_v070(DEVICE_NAME_METHOD, |id| {
                encode_set_device_name(name, max_length, id)
            })
            .await?;
        let info = decode_device_name_response(&reply, id)?;
        *self.device_name_max_length.lock().unwrap() = Some(info.max_length);
        Ok(info)
    }

    pub async fn set_sa_rgb(&self, update: &SaRgbUpdate) -> Result<(), SessionError> {
        update.validate()?;
        let (id, reply) = self
            .exchange_v070(SA_RGB_METHOD, |id| encode_set_sa_rgb(update, id))
            .await?;
        Ok(decode_set_sa_rgb_response(&reply, id)?)
    }

    pub async fn reset_sa_rgb(&self, sa: Vec<u32>) -> Result<(), SessionError> {
        let reset = SaRgbUpdate::reset(sa);
        self.set_sa_rgb(&reset).await
    }

    fn require_v070_exclusive_vendor(&self) -> Result<String, SessionError> {
        if self.link.backend() != mi_transport::LinkBackend::MacHidapi267Numbered
            || !self.link.is_exclusive_vendor()
        {
            return Err(ProtoError::Unverified(
                "0.7.0 hardware capability requires exclusive vendor MacHidapi267Numbered link",
            )
            .into());
        }
        let version = self.observed_firmware_version().ok_or_else(|| {
            SessionError::from(ProtoError::Unverified(
                "0.7.0 hardware capability requires verified firmware >= 0.7.0-rc.16",
            ))
        })?;
        if !mi_model::supports_v070_hardware_capabilities(&version) {
            return Err(ProtoError::Unverified(
                "0.7.0 hardware capability requires firmware >= 0.7.0-rc.16",
            )
            .into());
        }
        Ok(version)
    }

    pub fn cached_joystick_hardware_mode(
        &self,
        active_profile: Option<u8>,
        active_layer: Option<u8>,
    ) -> Option<String> {
        let bytes = load_cached_session_keymap(self.id)?;
        let observed = self.observed_profile_layer();
        let p = active_profile.or_else(|| observed.map(|(cp, _)| cp));
        let l = active_layer.or_else(|| observed.map(|(_, cl)| cl));
        mi_model::apply::extract_keymap_joystick_hardware_mode(&bytes, p, l)
    }

    pub async fn read_active_joystick_hardware_mode(
        &self,
        active_profile: Option<u8>,
        active_layer: Option<u8>,
    ) -> Result<Option<String>, SessionError> {
        self.require_v070_exclusive_vendor()?;
        self.guarded(async {
            let mut q = self.queue.lock().await;
            if usize::from(q.next_id) + 4 > 999 {
                return Ok(self.cached_joystick_hardware_mode(active_profile, active_layer));
            }
            match self.read_files_locked(&mut q).await {
                Ok(files) => {
                    let bytes = files.keymap.bytes.to_vec();
                    let observed = self.observed_profile_layer();
                    let p = active_profile.or_else(|| observed.map(|(cp, _)| cp));
                    let l = active_layer.or_else(|| observed.map(|(_, cl)| cl));
                    let mode = mi_model::apply::extract_keymap_joystick_hardware_mode(&bytes, p, l);
                    cache_session_keymap(self.id, bytes);
                    Ok(mode)
                }
                Err(SessionError::Config(_) | SessionError::Protocol(_)) => Ok(None),
                Err(err) => Err(err),
            }
        })
        .await
    }

    pub async fn apply_joystick_hardware_mode(
        &self,
        document_dir: &Path,
        mode: &str,
    ) -> Result<JoystickHardwareModeReceipt, SessionError> {
        let initial_version = self.require_v070_exclusive_vendor()?;
        let normalized_requested = match mode.trim().to_ascii_lowercase().as_str() {
            "mouse" => "mouse",
            "scroll" => "scroll",
            "logical_sectors" | "sectors" => "logical_sectors",
            other => {
                return Err(SessionError::Config(format!(
                    "不支持的摇杆硬件模式：{other}"
                )));
            }
        };

        self.guarded(async {
            let mut q = self.queue.lock().await;
            let status = self
                .exchange(&mut q, Method::DeviceStatus, Value::Null)
                .await?;
            let status_result = crate::identity_result(&status)?;
            let version = crate::firmware(status_result).unwrap_or(initial_version);
            if !mi_model::supports_v070_hardware_capabilities(&version) {
                return Err(ProtoError::Unverified(
                    "0.7.0 hardware capability requires firmware >= 0.7.0-rc.16",
                )
                .into());
            }
            *self.firmware.lock().unwrap() = Some(version.clone());

            let status_p = status_result
                .get("profile_index")
                .and_then(Value::as_u64)
                .and_then(|v| u8::try_from(v).ok());
            let status_l = status_result
                .get("layer_index")
                .and_then(Value::as_u64)
                .and_then(|v| u8::try_from(v).ok());
            let observed = self.observed_profile_layer();
            let active_profile = status_p.or_else(|| observed.map(|(cp, _)| cp));
            let active_layer = status_l.or_else(|| observed.map(|(_, cl)| cl));
            if let Some((np, nl)) = active_profile.zip(active_layer) {
                *self.profile_layer.lock().unwrap() = Some((np, nl));
            }

            let current = self.read_files_locked(&mut q).await?;
            let pre_write_keymap = current.keymap.bytes.to_vec();
            let smart_ref = current.smart_actions.as_ref().map(|f| f.bytes.as_ref());

            let backup_path = persist_immutable_v070_keymap_backup(document_dir, &pre_write_keymap)
                .map_err(SessionError::Config)?;

            let parsed_cfg = mi_model::parse_device_files(&pre_write_keymap, smart_ref)
                .map_err(|e| SessionError::Config(e.to_string()))?;
            let effective_profile_idx: Option<usize> = if let Some(p) = active_profile {
                Some(usize::from(p))
            } else {
                parsed_cfg
                    .raw
                    .get("profiles")
                    .and_then(Value::as_array)
                    .and_then(|profiles| {
                        let active_id = parsed_cfg.raw.get("activeProfileId")?;
                        let matching: Vec<usize> = profiles
                            .iter()
                            .enumerate()
                            .filter_map(|(i, p)| (&p["id"] == active_id).then_some(i))
                            .collect();
                        if matching.len() == 1 {
                            Some(matching[0])
                        } else {
                            active_id.as_u64().and_then(|n| usize::try_from(n).ok())
                        }
                    })
            };

            let recognized_six_layer = mi_model::apply::recognize_six_layer_profile_for_firmware(
                &pre_write_keymap,
                smart_ref,
                Some(&version),
            )
            .ok()
            .flatten();
            let is_six_layer_profile = recognized_six_layer.is_some()
                && recognized_six_layer.map(|idx| idx as usize) == effective_profile_idx;

            let layer_arg = if is_six_layer_profile {
                None
            } else {
                Some(
                    active_layer
                        .and_then(|l| l.checked_sub(1))
                        .map(usize::from)
                        .unwrap_or(0),
                )
            };

            let (patched_bytes, changed) = mi_model::apply::patch_keymap_joystick_hardware_mode(
                &pre_write_keymap,
                &version,
                effective_profile_idx,
                layer_arg,
                normalized_requested,
            )
            .map_err(SessionError::Config)?;

            if changed {
                const KEYMAP_CHUNK_BYTES: usize = 3072;
                let chunks = patched_bytes.len().div_ceil(KEYMAP_CHUNK_BYTES);
                let smart_chunks = smart_ref.map_or(0, |b| b.len().div_ceil(KEYMAP_CHUNK_BYTES));
                let needed_requests = 2 + chunks + 2 + chunks + smart_chunks + 1;
                if chunks == 0 || usize::from(q.next_id) + needed_requests > 999 {
                    return Err(SessionError::Config(
                        "会话请求预算不足，请重新连接后再修改摇杆硬件模式".into(),
                    ));
                }
                if let Some(snapshot) = self.snapshot.lock().unwrap().as_mut() {
                    snapshot.stale = true;
                }
                let mut guard = V070KeymapMutationGuard {
                    session: self,
                    sent: true,
                    verified: false,
                };
                let tx = self.exchange_tx_begin(&mut q).await?;
                for offset in (0..patched_bytes.len()).step_by(KEYMAP_CHUNK_BYTES) {
                    let expected_id = q.next_id;
                    let reply = self
                        .exchange_keymap_with_tx(&mut q, &patched_bytes, offset, Some(tx))
                        .await?;
                    mi_proto::mutation::validate_mutation_response_candidate(&reply, expected_id)?;
                }
                self.exchange_tx_commit(&mut q, tx).await?;
                let actual = self.read_files_locked(&mut q).await?;
                let actual_smart = actual.smart_actions.as_ref().map(|f| f.bytes.as_ref());
                if actual.keymap.bytes.as_ref() != patched_bytes.as_slice()
                    || actual_smart != smart_ref
                {
                    return Err(SessionError::Config(
                        "摇杆硬件模式写入后 SHA-1 / 原字节读回不一致".into(),
                    ));
                }
                guard.verified = true;
            }

            cache_session_keymap(self.id, patched_bytes.clone());

            if let Some(layer) = active_layer.filter(|&l| l >= 1) {
                let target = ProfileLayerTarget {
                    p: active_profile,
                    l: Some(layer),
                };
                target.validate()?;
                let id = q.next_id;
                let encoded = if id >= 999 {
                    Vec::new()
                } else {
                    encode_set_profile_layer(target, id)?
                };
                let reply = self
                    .exchange_authorized(
                        &mut q,
                        PROFILE_LAYER_SET_METHOD,
                        encoded,
                        Some(PROFILE_LAYER_SET_METHOD),
                    )
                    .await?;
                decode_set_profile_layer_response(&reply, id)?;
            }

            let resolved_mode = mi_model::apply::extract_keymap_joystick_hardware_mode(
                &patched_bytes,
                active_profile,
                active_layer,
            )
            .unwrap_or_else(|| match normalized_requested {
                "mouse" => "mouse".into(),
                "scroll" => "scroll".into(),
                _ => "radial".into(),
            });

            Ok(JoystickHardwareModeReceipt {
                mode: resolved_mode,
                active_profile,
                active_layer,
                backup_path,
                pre_write_keymap,
                post_write_keymap: patched_bytes,
                changed,
            })
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use mi_proto::v070_capabilities::{MouseConfig, PmConfig, SaRgbEffect, SaRgbUpdate};
    use mi_proto::{Framer, NativeFramer, ProtoError};
    use mi_transport::{HidLink, LinkBackend, TransportError};
    use serde_json::{json, Value};

    use crate::{DeviceSession, ReadOnlySchema, SessionConfig, SessionError};

    struct TestFramer;
    impl Framer for TestFramer {
        fn encode(&self, payload: &[u8]) -> Result<Vec<Vec<u8>>, ProtoError> {
            Ok(vec![payload.to_vec()])
        }
        fn feed(&mut self, report: &[u8]) -> Result<Vec<Vec<u8>>, ProtoError> {
            Ok(vec![report.to_vec()])
        }
        fn clear(&mut self) {}
    }

    struct NativeVendorTestLink {
        tx: std::sync::Mutex<Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>>,
        rx: tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>>,
        out_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
        closed: std::sync::atomic::AtomicBool,
        exclusive: bool,
        backend: LinkBackend,
    }

    impl NativeVendorTestLink {
        fn new(
            exclusive: bool,
            backend: LinkBackend,
        ) -> (Arc<Self>, tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>) {
            let (in_tx, in_rx) = tokio::sync::mpsc::unbounded_channel();
            let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel();
            (
                Arc::new(Self {
                    tx: std::sync::Mutex::new(Some(in_tx)),
                    rx: tokio::sync::Mutex::new(in_rx),
                    out_tx,
                    closed: std::sync::atomic::AtomicBool::new(false),
                    exclusive,
                    backend,
                }),
                out_rx,
            )
        }

        fn push_reply(&self, value: Value) {
            if let Some(tx) = self.tx.lock().unwrap().as_ref() {
                let _ = tx.send(serde_json::to_vec(&value).unwrap());
            }
        }
    }

    #[async_trait::async_trait]
    impl HidLink for NativeVendorTestLink {
        fn is_exclusive_vendor(&self) -> bool {
            self.exclusive
        }
        fn backend(&self) -> LinkBackend {
            self.backend
        }
        async fn write(&self, report: &[u8]) -> Result<(), TransportError> {
            if self.closed.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(TransportError::Disconnected);
            }
            let _ = self.out_tx.send(report.to_vec());
            Ok(())
        }
        async fn read(&self) -> Result<Vec<u8>, TransportError> {
            let mut rx = self.rx.lock().await;
            rx.recv().await.ok_or(TransportError::Disconnected)
        }
        fn close(&self) {
            self.closed.store(true, std::sync::atomic::Ordering::SeqCst);
            self.tx.lock().unwrap().take();
        }
    }

    fn setup_session(
        exclusive: bool,
        backend: LinkBackend,
    ) -> (
        Arc<DeviceSession>,
        Arc<NativeVendorTestLink>,
        tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    ) {
        let (link, out_rx) = NativeVendorTestLink::new(exclusive, backend);
        let session = DeviceSession::new(
            link.clone(),
            Box::new(TestFramer),
            Arc::new(ReadOnlySchema),
            SessionConfig {
                timeout: Duration::from_millis(200),
                interval: Duration::ZERO,
            },
        );
        (session, link, out_rx)
    }

    async fn read_identity(
        session: &DeviceSession,
        link: &NativeVendorTestLink,
        out_rx: &mut tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
        version: &str,
    ) {
        let fut = session.read_identity_info();
        tokio::pin!(fut);
        let req_bytes = tokio::select! {
            _ = &mut fut => panic!("read_identity_info returned before receiving reply"),
            b = out_rx.recv() => b.unwrap(),
        };
        let req: Value = serde_json::from_slice(&req_bytes).unwrap();
        link.push_reply(json!({
            "id": req["id"],
            "result": {
                "version": version,
                "profile_index": 3,
                "layer_index": 1,
                "battery": 84,
                "is_charging": true
            }
        }));
        let info = fut.await.unwrap();
        assert_eq!(info.version.as_deref(), Some(version));
        assert_eq!(info.battery, Some(84));
        assert_eq!(info.is_charging, Some(true));
        assert_eq!(session.observed_battery_status(), (Some(84), Some(true)));
        assert_eq!(session.observed_profile_layer(), Some((3, 1)));
    }

    #[tokio::test]
    async fn all_v070_capability_methods_succeed_on_exclusive_v070_link() {
        let (session, link, mut out_rx) = setup_session(true, LinkBackend::MacHidapi267Numbered);
        read_identity(&session, &link, &mut out_rx, "0.7.0-rc.16").await;

        let link_responder = link.clone();
        let responder = tokio::spawn(async move {
            while let Some(raw) = out_rx.recv().await {
                let req: Value = serde_json::from_slice(&raw).unwrap();
                let id = req["id"].clone();
                let method = req["method"].as_str().unwrap();
                let result = match method {
                    "mouse.config" if req["params"].is_null() => {
                        serde_json::to_value(MouseConfig::default()).unwrap()
                    }
                    "mouse.config" => req["params"].clone(),
                    "joystick.deadzone" if req["params"].is_null() => json!({"v": 0.15}),
                    "joystick.deadzone" => req["params"].clone(),
                    "kb.pl.set" => json!({}),
                    "pm.config" if req["params"].is_null() => {
                        serde_json::to_value(PmConfig::default()).unwrap()
                    }
                    "pm.config" => req["params"].clone(),
                    "pm.state" => json!({
                        "state": "active",
                        "floor": "usb",
                        "base_floor": "host_sleep",
                        "line_powered": true,
                        "auto_sleep": true,
                        "sleep_after_s": 900,
                        "idle_s": 3,
                        "standby_s": 3600,
                        "credit_s": 0,
                        "holds": [{"owner": "usb.host", "floor": "usb", "age_ms": 1200}]
                    }),
                    "device.name" if req["params"].is_null() => json!({
                        "name": "Creator Micro 2",
                        "factory_name": "Creator Micro 2",
                        "max_length": 15
                    }),
                    "device.name" => json!({
                        "name": req["params"]["name"],
                        "factory_name": "Creator Micro 2",
                        "max_length": 15
                    }),
                    "kb.sa.rgb" => json!({}),
                    other => panic!("unexpected method {other}"),
                };
                link_responder.push_reply(json!({
                    "id": id,
                    "method": method,
                    "result": result
                }));
            }
        });

        let mc = session.read_mouse_config().await.unwrap();
        assert_eq!(mc, MouseConfig::default());
        let mut custom_mc = mc;
        custom_mc.move_delta = 16;
        assert_eq!(
            session.write_mouse_config(&custom_mc).await.unwrap(),
            custom_mc
        );

        assert_eq!(session.read_joystick_deadzone().await.unwrap(), 0.15);
        assert_eq!(session.write_joystick_deadzone(0.22).await.unwrap(), 0.22);

        session
            .set_profile_layer_flashless(Some(3), Some(2))
            .await
            .unwrap();
        assert_eq!(session.observed_profile_layer(), Some((3, 2)));

        let pm = session.read_pm_config().await.unwrap();
        assert_eq!(pm, PmConfig::default());
        let updated_pm = PmConfig {
            auto_sleep: false,
            host_sleep: true,
            sleep_after_min: 30,
        };
        assert_eq!(
            session.write_pm_config(&updated_pm).await.unwrap(),
            updated_pm
        );

        let pm_state = session.read_pm_state().await.unwrap();
        assert_eq!(pm_state.usb_host_hold().unwrap().floor, "usb");

        let name_info = session.read_device_name().await.unwrap();
        assert_eq!(name_info.max_length, 15);
        let renamed = session.write_device_name("Micro-Studio").await.unwrap();
        assert_eq!(renamed.name, "Micro-Studio");

        let rgb = SaRgbUpdate {
            sa: vec![7057],
            c: Some(0x33AAFF),
            b: Some(0.8),
            s: Some(0.4),
            e: Some(SaRgbEffect::Breath.as_u8()),
        };
        session.set_sa_rgb(&rgb).await.unwrap();
        session.reset_sa_rgb(vec![7057]).await.unwrap();

        session.shutdown().await;
        responder.abort();
    }

    #[tokio::test]
    async fn v070_methods_fail_closed_on_v063_missing_version_or_non_exclusive_links_with_zero_frames(
    ) {
        let (session_063, link_063, mut out_063) =
            setup_session(true, LinkBackend::MacHidapi267Numbered);
        read_identity(&session_063, &link_063, &mut out_063, "0.6.3").await;
        assert!(matches!(
            session_063.read_mouse_config().await,
            Err(SessionError::Protocol(ProtoError::Unverified(_)))
        ));
        assert!(matches!(
            session_063
                .set_profile_layer_flashless(Some(3), Some(1))
                .await,
            Err(SessionError::Protocol(ProtoError::Unverified(_)))
        ));
        assert!(matches!(
            session_063.read_pm_state().await,
            Err(SessionError::Protocol(ProtoError::Unverified(_)))
        ));
        assert!(out_063.try_recv().is_err(), "no frame may be sent on 0.6.3");
        session_063.shutdown().await;

        let (session_missing, _link_missing, mut out_missing) =
            setup_session(true, LinkBackend::MacHidapi267Numbered);
        assert!(matches!(
            session_missing.read_device_name().await,
            Err(SessionError::Protocol(ProtoError::Unverified(_)))
        ));
        assert!(
            out_missing.try_recv().is_err(),
            "no frame may be sent without version"
        );
        session_missing.shutdown().await;

        let (session_shared, link_shared, mut out_shared) =
            setup_session(false, LinkBackend::MacHidapi267Numbered);
        read_identity(
            &session_shared,
            &link_shared,
            &mut out_shared,
            "0.7.0-rc.16",
        )
        .await;
        assert!(matches!(
            session_shared.read_pm_config().await,
            Err(SessionError::Protocol(ProtoError::Unverified(_)))
        ));
        assert!(matches!(
            session_shared.write_joystick_deadzone(0.2).await,
            Err(SessionError::Protocol(ProtoError::Unverified(_)))
        ));
        assert!(
            out_shared.try_recv().is_err(),
            "no frame may be sent without exclusive vendor link"
        );
        session_shared.shutdown().await;

        #[cfg(feature = "synthetic-backend")]
        {
            let (session_syn, link_syn, mut out_syn) =
                setup_session(true, LinkBackend::SyntheticNumbered);
            read_identity(&session_syn, &link_syn, &mut out_syn, "0.7.0-rc.16").await;
            assert!(matches!(
                session_syn.read_mouse_config().await,
                Err(SessionError::Protocol(ProtoError::Unverified(_)))
            ));
            assert!(
                out_syn.try_recv().is_err(),
                "no frame may be sent on non-267 backend"
            );
            session_syn.shutdown().await;
        }
        let _ = NativeFramer::default();
    }

    #[tokio::test]
    async fn kb_pl_notification_updates_observed_profile_layer() {
        let (session, link, _out_rx) = setup_session(true, LinkBackend::MacHidapi267Numbered);
        let mut rx = session.notifications();
        assert_eq!(session.observed_profile_layer(), None);

        link.push_reply(json!({"m": "kb.pl", "p": {"p": 3, "l": 4}}));
        let received = tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            received.notification,
            mi_proto::HostNotification::ProfileLayer(_)
        ));
        assert_eq!(session.observed_profile_layer(), Some((3, 4)));
        session.shutdown().await;
    }

    #[tokio::test]
    async fn joystick_hardware_mode_readback_degrades_gracefully_without_keymap_and_gates_firmware()
    {
        let dir = tempfile::tempdir().unwrap();
        let (session_063, link_063, mut out_063) =
            setup_session(true, LinkBackend::MacHidapi267Numbered);
        read_identity(&session_063, &link_063, &mut out_063, "0.6.3").await;
        assert!(matches!(
            session_063
                .read_active_joystick_hardware_mode(Some(3), Some(1))
                .await,
            Err(SessionError::Protocol(ProtoError::Unverified(_)))
        ));
        assert!(matches!(
            session_063
                .apply_joystick_hardware_mode(dir.path(), "mouse")
                .await,
            Err(SessionError::Protocol(ProtoError::Unverified(_)))
        ));
        assert!(out_063.try_recv().is_err());
        session_063.shutdown().await;

        let (session_min, link_min, mut out_min) =
            setup_session(true, LinkBackend::MacHidapi267Numbered);
        read_identity(&session_min, &link_min, &mut out_min, "0.7.0-rc.16").await;
        let link_resp = link_min.clone();
        let responder = tokio::spawn(async move {
            while let Some(raw) = out_min.recv().await {
                let req: Value = serde_json::from_slice(&raw).unwrap();
                link_resp.push_reply(json!({
                    "id": req["id"],
                    "method": req["method"],
                    "result": {}
                }));
            }
        });
        assert_eq!(
            session_min
                .read_active_joystick_hardware_mode(Some(3), Some(1))
                .await
                .unwrap(),
            None
        );
        session_min.shutdown().await;
        responder.abort();
    }

    #[tokio::test]
    async fn apply_joystick_hardware_mode_backs_up_verifies_sha1_patches_six_layers_and_reloads_pl()
    {
        use base64::engine::general_purpose::STANDARD as B64;
        use base64::Engine as _;
        use sha1::{Digest, Sha1};
        use std::os::unix::fs::PermissionsExt;

        const SIX_SOURCE: &[u8] =
            include_bytes!("../../mi-model/fixtures/device/synthetic-six-layer-source.json");
        let initial_six = mi_model::apply::compile_six_layer_profile(SIX_SOURCE, None)
            .unwrap()
            .keymap;

        let dir = tempfile::tempdir().unwrap();
        let (session, link, mut out_rx) = setup_session(true, LinkBackend::MacHidapi267Numbered);
        read_identity(&session, &link, &mut out_rx, "0.7.0-rc.16").await;

        let recorded_methods = Arc::new(std::sync::Mutex::new(Vec::<(String, Value)>::new()));
        let methods_clone = recorded_methods.clone();
        let corrupt_readback = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let corrupt_clone = corrupt_readback.clone();
        let link_resp = link.clone();
        let initial_for_task = initial_six.clone();

        let responder = tokio::spawn(async move {
            let mut keymap = initial_for_task;
            let mut staged = Vec::new();
            let mut active_profile = 1_u64;
            let mut active_layer = 2_u64;
            while let Some(raw) = out_rx.recv().await {
                let req: Value = serde_json::from_slice(&raw).unwrap();
                let id = req["id"].clone();
                let method = req["method"].as_str().unwrap().to_string();
                let params = req["params"].clone();
                methods_clone
                    .lock()
                    .unwrap()
                    .push((method.clone(), params.clone()));

                let visible_keymap = if corrupt_clone.load(std::sync::atomic::Ordering::SeqCst) {
                    let mut bad = keymap.clone();
                    bad.push(b' ');
                    bad
                } else {
                    keymap.clone()
                };

                let result = match method.as_str() {
                    "device.status" => json!({
                        "version": "0.7.0-rc.16",
                        "profile_index": active_profile,
                        "layer_index": active_layer
                    }),
                    "fs.list" => {
                        let sha1 = format!("{:x}", Sha1::digest(&visible_keymap));
                        json!([{
                            "name": "keymap.json",
                            "size": visible_keymap.len(),
                            "checksum": sha1
                        }])
                    }
                    "fs.readbin" => {
                        let offset = params["offset"].as_u64().unwrap() as usize;
                        let len = params["len"].as_u64().unwrap() as usize;
                        let end = (offset + len).min(visible_keymap.len());
                        json!({
                            "total_size": visible_keymap.len(),
                            "data": B64.encode(&visible_keymap[offset..end])
                        })
                    }
                    "fs.txbegin" => {
                        staged.clear();
                        json!({"tx": 42})
                    }
                    "fs.writebin" => {
                        assert_eq!(params["tx"], json!(42));
                        let chunk = B64.decode(params["data"].as_str().unwrap()).unwrap();
                        staged.extend_from_slice(&chunk);
                        Value::Null
                    }
                    "fs.txcommit" => {
                        assert_eq!(params["tx"], json!(42));
                        keymap = std::mem::take(&mut staged);
                        Value::Null
                    }
                    "kb.pl.set" => {
                        if let Some(p) = params.get("p").and_then(Value::as_u64) {
                            active_profile = p;
                        }
                        if let Some(l) = params.get("l").and_then(Value::as_u64) {
                            active_layer = l;
                        }
                        json!({})
                    }
                    other => panic!("unexpected method {other}"),
                };

                link_resp.push_reply(json!({
                    "id": id,
                    "method": method,
                    "result": result
                }));
            }
        });

        assert_eq!(
            session
                .read_active_joystick_hardware_mode(Some(1), Some(2))
                .await
                .unwrap()
                .as_deref(),
            Some("radial")
        );

        let receipt_mouse = session
            .apply_joystick_hardware_mode(dir.path(), "mouse")
            .await
            .unwrap();
        assert!(receipt_mouse.changed);
        assert_eq!(receipt_mouse.mode, "mouse");
        assert_eq!(receipt_mouse.active_profile, Some(1));
        assert_eq!(receipt_mouse.active_layer, Some(2));
        assert_eq!(receipt_mouse.pre_write_keymap, initial_six);

        let backup_dir = dir.path().join("v070-keymap-backups");
        assert_eq!(
            std::fs::metadata(&backup_dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&receipt_mouse.backup_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::read(&receipt_mouse.backup_path).unwrap(),
            initial_six
        );
        assert_eq!(
            std::fs::read(backup_dir.join("original-keymap.json")).unwrap(),
            initial_six
        );

        for layer_1_based in 1..=6_u8 {
            assert_eq!(
                session
                    .cached_joystick_hardware_mode(Some(1), Some(layer_1_based))
                    .as_deref(),
                Some("mouse")
            );
        }
        assert!(recorded_methods
            .lock()
            .unwrap()
            .iter()
            .any(|(m, p)| m == "kb.pl.set" && p == &json!({"p": 1, "l": 2})));

        let receipt_scroll = session
            .apply_joystick_hardware_mode(dir.path(), "scroll")
            .await
            .unwrap();
        assert!(receipt_scroll.changed);
        assert_eq!(receipt_scroll.mode, "scroll");
        assert_eq!(
            std::fs::read(backup_dir.join("original-keymap.json")).unwrap(),
            initial_six
        );

        let receipt_sectors = session
            .apply_joystick_hardware_mode(dir.path(), "logical_sectors")
            .await
            .unwrap();
        assert!(receipt_sectors.changed);
        assert_eq!(receipt_sectors.mode, "radial");
        assert_eq!(
            session
                .cached_joystick_hardware_mode(Some(1), Some(1))
                .as_deref(),
            Some("vendor")
        );
        assert_eq!(
            session
                .cached_joystick_hardware_mode(Some(1), Some(2))
                .as_deref(),
            Some("radial")
        );

        corrupt_readback.store(true, std::sync::atomic::Ordering::SeqCst);
        let err = session
            .apply_joystick_hardware_mode(dir.path(), "mouse")
            .await
            .unwrap_err();
        assert!(!session.is_disconnected() || matches!(err, SessionError::Config(_)));

        session.shutdown().await;
        responder.abort();
    }

    #[tokio::test]
    #[ignore = "requires live Creator Micro 2 (303A:8298) on 0.7.0-rc.16+"]
    async fn live_v070_sync_standard_mouse_speed() {
        use mi_proto::v070_capabilities::MouseSpeedPreset;
        use mi_transport::HidTransport;

        let target =
            mi_transport::discover_target_keyboard().expect("Creator Micro 2 not connected");
        let transport = mi_transport::MacTransport::exclusive_vendor();
        let link = transport
            .open(&target.vendor_path)
            .expect("failed to open exclusive vendor link");
        let session = DeviceSession::new(
            link,
            Box::new(NativeFramer::default()),
            Arc::new(ReadOnlySchema),
            SessionConfig::default(),
        );

        let info = session
            .read_identity_info()
            .await
            .expect("read_identity_info failed");
        let before_mc = session
            .read_mouse_config()
            .await
            .expect("read_mouse_config before failed");
        let deadzone = session
            .read_joystick_deadzone()
            .await
            .expect("read_joystick_deadzone failed");

        let standard_mc = MouseSpeedPreset::Standard.to_mouse_config();
        let written_mc = session
            .write_mouse_config(&standard_mc)
            .await
            .expect("write_mouse_config failed");
        let after_mc = session
            .read_mouse_config()
            .await
            .expect("read_mouse_config after failed");

        println!(
            "live_v070_sync_standard_mouse_speed: version={:?} deadzone={:.3} before={:?} after={:?}",
            info.version, deadzone, before_mc, after_mc
        );
        assert_eq!(written_mc, standard_mc);
        assert_eq!(after_mc, standard_mc);

        session.shutdown().await;
    }

    #[cfg(unix)]
    #[test]
    fn v070_keymap_backup_rejects_symlinks_fifos_and_enforces_private_fs() {
        use super::{persist_immutable_v070_keymap_backup, Digest, Sha1};
        use std::fs;
        use std::os::unix::fs::{symlink, PermissionsExt};

        let temp = tempfile::tempdir().unwrap();
        let real_doc = temp.path().join("real-doc");
        fs::create_dir_all(&real_doc).unwrap();
        fs::set_permissions(&real_doc, fs::Permissions::from_mode(0o700)).unwrap();

        let sym_doc = temp.path().join("sym-doc");
        symlink(&real_doc, &sym_doc).unwrap();
        assert!(persist_immutable_v070_keymap_backup(&sym_doc, br#"{"v":1}"#).is_err());

        let doc_b = temp.path().join("doc-b");
        let backup_b = doc_b.join("v070-keymap-backups");
        fs::create_dir_all(&backup_b).unwrap();
        let dummy = temp.path().join("dummy.json");
        fs::write(&dummy, br#"{"dummy":true}"#).unwrap();
        symlink(&dummy, backup_b.join("original-keymap.json")).unwrap();
        assert!(persist_immutable_v070_keymap_backup(&doc_b, br#"{"v":1}"#).is_err());

        let payload = br#"{"keymap":"sample"}"#;
        let sha1_hex = format!("{:x}", Sha1::digest(payload));

        let doc_c_sym = temp.path().join("doc-c-sym");
        let backup_c_sym = doc_c_sym.join("v070-keymap-backups");
        fs::create_dir_all(&backup_c_sym).unwrap();
        symlink(&dummy, backup_c_sym.join(format!("keymap-{sha1_hex}.json"))).unwrap();
        assert!(persist_immutable_v070_keymap_backup(&doc_c_sym, payload).is_err());

        let doc_c_fifo = temp.path().join("doc-c-fifo");
        let backup_c_fifo = doc_c_fifo.join("v070-keymap-backups");
        fs::create_dir_all(&backup_c_fifo).unwrap();
        let fifo_path = backup_c_fifo.join(format!("keymap-{sha1_hex}.json"));
        let status = std::process::Command::new("/usr/bin/mkfifo")
            .arg(&fifo_path)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(persist_immutable_v070_keymap_backup(&doc_c_fifo, payload).is_err());

        let doc_ok = temp.path().join("doc-ok");
        let first = persist_immutable_v070_keymap_backup(&doc_ok, br#"{"first":1}"#).unwrap();
        assert_eq!(
            fs::metadata(&first).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(doc_ok.join("v070-keymap-backups"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        let second = persist_immutable_v070_keymap_backup(&doc_ok, br#"{"second":2}"#).unwrap();
        assert_ne!(first, second);
        assert_eq!(
            fs::read(doc_ok.join("v070-keymap-backups/original-keymap.json")).unwrap(),
            br#"{"first":1}"#
        );

        let unsafe_ancestor = temp.path().join("unsafe-ancestor");
        let child_dir = unsafe_ancestor.join("child");
        fs::create_dir_all(&child_dir).unwrap();
        let existing_file = child_dir.join("existing.json");
        fs::write(&existing_file, br#"{"match":true}"#).unwrap();
        fs::set_permissions(&child_dir, fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(&unsafe_ancestor, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(
            super::write_verified_immutable_file(&existing_file, br#"{"match":true}"#).is_err()
        );
        fs::set_permissions(&unsafe_ancestor, fs::Permissions::from_mode(0o700)).unwrap();
    }
}
