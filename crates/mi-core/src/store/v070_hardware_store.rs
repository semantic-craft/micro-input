//! WorkbenchStore orchestration and async session dispatch for `0.7.0-rc.16+` hardware capabilities.

use mi_proto::v070_capabilities::SaRgbEffect;
use mi_session::DeviceSession;

use crate::store::{HardwareLoopEvent, SystemLifecycle, WorkbenchError, WorkbenchStore};
use crate::v070_hardware::{
    format_hex_color, parse_v070_action, ParsedV070Action, V070BridgeOutcome, V070HardwareAction,
    V070HardwareProjection, V070HostPreference, V070MouseConfigProjection, V070PmStateProjection,
    V070SaRgbProjection, V070SnapshotPayload, ValidatedV070DeviceCommand,
};
use crate::v070_session_sa_rgb::{
    resolve_desired_for_device_layer, resolve_desired_for_document_layer, DesiredTarget,
    LayerLightingResolution, ManualSaRgbWrite, SaRgbLitStyle, UrgentClearReason,
};

#[derive(Debug, Clone)]
pub(crate) struct V070BridgeCommand {
    pub seq: u64,
    pub command: ValidatedV070DeviceCommand,
    pub document_dir: std::path::PathBuf,
    pub active_six_layer_backup: Option<std::path::PathBuf>,
}

fn write_private_backup_file(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let parent = path
        .parent()
        .ok_or_else(|| "备份目录路径缺失".to_string())?;
    let tmp = path.with_extension("v070.tmp");
    let _ = std::fs::remove_file(&tmp);
    {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|e| e.to_string())?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|e| e.to_string())?;
    }
    if std::fs::read(&tmp).map_err(|e| e.to_string())? != bytes {
        let _ = std::fs::remove_file(&tmp);
        return Err("备份写后读回不一致".into());
    }
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())?;
    std::fs::File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn sync_six_layer_backup_after_joystick_write(
    backup_dir: &std::path::Path,
    pre_write_keymap: &[u8],
    post_write_keymap: &[u8],
) -> Result<(), String> {
    let planned_path = backup_dir.join("planned-keymap.json");
    if !planned_path.is_file() {
        return Ok(());
    }
    let current_planned = std::fs::read(&planned_path).map_err(|e| e.to_string())?;
    if current_planned != pre_write_keymap {
        return Ok(());
    }
    let prev_planned_path = backup_dir.join("previous-planned-keymap.json");
    if !prev_planned_path.exists() {
        write_private_backup_file(&prev_planned_path, pre_write_keymap)?;
    }
    write_private_backup_file(&planned_path, post_write_keymap)?;
    let manifest_path = backup_dir.join("manifest.json");
    if manifest_path.is_file() {
        if let Ok(raw_manifest) = std::fs::read(&manifest_path) {
            if let Ok(mut manifest) = serde_json::from_slice::<serde_json::Value>(&raw_manifest) {
                if let Some(obj) = manifest.as_object_mut() {
                    obj.insert(
                        "planned_keymap_sha256".into(),
                        serde_json::Value::String(crate::lighting_arbiter::sha256_hex(
                            post_write_keymap,
                        )),
                    );
                    if !obj.contains_key("previous_planned_keymap_sha256")
                        && prev_planned_path.is_file()
                    {
                        if let Ok(prev_bytes) = std::fs::read(&prev_planned_path) {
                            obj.insert(
                                "previous_planned_keymap_sha256".into(),
                                serde_json::Value::String(crate::lighting_arbiter::sha256_hex(
                                    &prev_bytes,
                                )),
                            );
                        }
                    }
                    if let Ok(encoded) = serde_json::to_vec_pretty(&manifest) {
                        let _ = write_private_backup_file(&manifest_path, &encoded);
                    }
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Default)]
pub(crate) struct V070HardwareController {
    pub projection: V070HardwareProjection,
    pub next_seq: u64,
    pub in_flight_seq: Option<u64>,
    pub last_snapshot_session_id: Option<u64>,
    pub expected_layer_echo: Option<(u8, std::time::Instant)>,
    pub pending_auto_layer_sync: Option<u8>,
    pub device_layer_override: Option<u8>,
}

impl V070HardwareController {
    pub fn allocate_seq(&mut self) -> u64 {
        self.next_seq = self.next_seq.saturating_add(1);
        self.next_seq
    }

    pub fn set_host_sleep_status(&mut self, status: &str) {
        self.projection.host_sleep_status = status.to_string();
    }

    pub fn consume_expected_layer_echo(&mut self, layer: u8, now: std::time::Instant) -> bool {
        if let Some((expected, deadline)) = self.expected_layer_echo {
            if now > deadline {
                self.expected_layer_echo = None;
                return false;
            }
            if expected == layer {
                self.expected_layer_echo = None;
                return true;
            }
        }
        false
    }

    pub fn on_disconnected(&mut self) {
        self.projection.supported = false;
        self.projection.busy = false;
        self.projection.synced = false;
        self.projection.active_profile = None;
        self.projection.active_layer = None;
        self.projection.active_joystick_mode = None;
        self.projection.battery_percent = None;
        self.projection.is_charging = None;
        self.projection.last_error = None;
        self.projection.status_message = "需连接 0.7.0-rc.16+ 独占固件设备".into();
        self.in_flight_seq = None;
        self.last_snapshot_session_id = None;
        self.expected_layer_echo = None;
        self.pending_auto_layer_sync = None;
        self.device_layer_override = None;
    }

    pub fn mark_supported(
        &mut self,
        version: &str,
        profile_index: Option<u8>,
        layer_index: Option<u8>,
        battery_percent: Option<u8>,
        is_charging: Option<bool>,
    ) {
        self.projection.supported = true;
        if profile_index.is_some() {
            self.projection.active_profile = profile_index;
        }
        if layer_index.is_some() {
            self.projection.active_layer = layer_index;
        }
        if battery_percent.is_some() {
            self.projection.battery_percent = battery_percent;
        }
        if is_charging.is_some() {
            self.projection.is_charging = is_charging;
        }
        if !self.projection.synced {
            self.projection.status_message = format!("已识别 {version} 硬件能力，等待同步");
        }
    }

    pub fn apply_profile_layer_notification(&mut self, profile_index: u8, layer_index: u8) {
        self.projection.active_profile = Some(profile_index);
        self.projection.active_layer = Some(layer_index);
        self.projection.last_error = None;
        self.projection.status_message =
            format!("板载层已同步至 P{profile_index} · 第 {layer_index} 层");
    }

    pub fn apply_outcome(&mut self, seq: u64, result: Result<V070BridgeOutcome, String>) {
        if seq == 0 && self.in_flight_seq.is_some() && self.projection.synced {
            return;
        }
        if seq != 0 {
            if let Some(in_flight) = self.in_flight_seq {
                if seq < in_flight {
                    return;
                }
            }
        }
        if Some(seq) == self.in_flight_seq || (seq == 0 && self.in_flight_seq.is_none()) {
            self.projection.busy = false;
            self.in_flight_seq = None;
        }
        match result {
            Ok(V070BridgeOutcome::Snapshot(snap)) => {
                self.projection.supported = true;
                self.projection.synced = true;
                self.projection.last_error = None;
                self.last_snapshot_session_id = Some(snap.session_id);
                if snap.active_profile.is_some() {
                    self.projection.active_profile = snap.active_profile;
                }
                if snap.active_layer.is_some() {
                    self.projection.active_layer = snap.active_layer;
                }
                self.projection.active_joystick_mode = snap.active_joystick_mode;
                if snap.battery_percent.is_some() {
                    self.projection.battery_percent = snap.battery_percent;
                }
                if snap.is_charging.is_some() {
                    self.projection.is_charging = snap.is_charging;
                }
                self.projection.mouse =
                    V070MouseConfigProjection::from_mouse_config(&snap.mouse, snap.deadzone);
                self.projection.pm_config = snap.pm_config;
                self.projection.pm_state = V070PmStateProjection::from_pm_state(&snap.pm_state);
                self.projection.device_name = snap.device_name;
                self.projection.status_message =
                    format!("0.7.0 硬件能力已同步（{}）", snap.version);
            }
            Ok(V070BridgeOutcome::ProfileLayer {
                profile_index,
                layer_index,
            }) => {
                if let Some(profile) = profile_index {
                    self.projection.active_profile = Some(profile);
                }
                self.projection.active_layer = Some(layer_index);
                self.projection.last_error = None;
                self.projection.status_message =
                    format!("已切换当前板载层至第 {layer_index} 层（不写入闪存）");
            }
            Ok(V070BridgeOutcome::JoystickHardwareModeUpdated {
                mode,
                active_profile,
                active_layer,
            }) => {
                if let Some(profile) = active_profile {
                    self.projection.active_profile = Some(profile);
                }
                if let Some(layer) = active_layer {
                    self.projection.active_layer = Some(layer);
                }
                let mode_label = match mode.as_str() {
                    "mouse" => "硬件鼠标（MOUSE）",
                    "scroll" => "硬件滚轮（SCROLL）",
                    "vendor" => "原生通道（VENDOR）",
                    "radial" => "四向扇区（RADIAL）",
                    other => other,
                };
                self.projection.active_joystick_mode = Some(mode.clone());
                self.projection.last_error = None;
                self.projection.status_message = format!("已同步板载摇杆硬件模式为{mode_label}");
            }
            Ok(V070BridgeOutcome::MouseConfigUpdated { mouse }) => {
                self.projection.mouse = V070MouseConfigProjection::from_mouse_config(
                    &mouse,
                    self.projection.mouse.deadzone,
                );
                self.projection.last_error = None;
                self.projection.status_message =
                    format!("已更新硬件鼠标参数（{}）", self.projection.mouse.preset);
            }
            Ok(V070BridgeOutcome::DeadzoneUpdated { deadzone }) => {
                self.projection.mouse.deadzone = deadzone;
                self.projection.last_error = None;
                self.projection.status_message = format!("已更新摇杆防漂移死区（{deadzone:.2}）");
            }
            Ok(V070BridgeOutcome::PmConfigUpdated { pm_config }) => {
                self.projection.pm_config = pm_config;
                self.projection.last_error = None;
                self.projection.status_message = "已更新电源管理设置".into();
            }
            Ok(V070BridgeOutcome::DeviceRenamed { device_name }) => {
                let updated_name = device_name.name.clone();
                self.projection.device_name = device_name;
                self.projection.last_error = None;
                self.projection.status_message = format!("设备名称已更新为 {updated_name}");
            }
            Ok(V070BridgeOutcome::SaRgbApplied {
                sa,
                rgb,
                brightness,
                speed,
                effect,
            }) => {
                self.projection.sa_rgb = V070SaRgbProjection {
                    sa: Some(sa.get()),
                    color_hex: format_hex_color(rgb),
                    brightness,
                    speed,
                    effect: effect.as_u8(),
                };
                self.projection.last_error = None;
                self.projection.status_message = format!("已更新 SA #{} 独立灯效", sa.get());
            }
            Ok(V070BridgeOutcome::SaRgbReset { sa }) => {
                self.projection.sa_rgb.sa = Some(sa.get());
                self.projection.sa_rgb.effect = SaRgbEffect::Off.as_u8();
                self.projection.last_error = None;
                self.projection.status_message = format!("已恢复 SA #{} 默认背景灯效", sa.get());
            }
            Err(error) => {
                self.projection.last_error = Some(error.clone());
                self.projection.status_message = error;
            }
        }
    }
}

pub(crate) async fn fetch_v070_snapshot_on_session(
    session: &DeviceSession,
    refresh_identity: bool,
) -> Result<V070SnapshotPayload, String> {
    if session.is_disconnected() || !session.has_exclusive_vendor_link() {
        return Err("需要已连接的 USB 专用独占会话".into());
    }
    let version = session
        .observed_firmware_version()
        .ok_or_else(|| "固件版本尚未确认".to_string())?;
    if !mi_model::supports_v070_hardware_capabilities(&version) {
        return Err(format!(
            "当前固件 {version} 低于 0.7.0-rc.16，不支持硬件能力接口"
        ));
    }
    let (active_profile, active_layer, battery_percent, is_charging) = if refresh_identity {
        let identity = session
            .read_identity_info()
            .await
            .map_err(|e| e.to_string())?;
        (
            identity.profile_index.and_then(|v| u8::try_from(v).ok()),
            identity.layer_index.and_then(|v| u8::try_from(v).ok()),
            identity.battery,
            identity.is_charging,
        )
    } else {
        let (p, l) = session
            .observed_profile_layer()
            .map(|(p, l)| (Some(p), Some(l)))
            .unwrap_or((None, None));
        let (bat, chg) = session.observed_battery_status();
        (p, l, bat, chg)
    };
    let device_name = session
        .read_device_name()
        .await
        .map_err(|e| e.to_string())?;
    let pm_config = session.read_pm_config().await.map_err(|e| e.to_string())?;
    let pm_state = session.read_pm_state().await.map_err(|e| e.to_string())?;
    let mouse = session
        .read_mouse_config()
        .await
        .map_err(|e| e.to_string())?;
    let deadzone = session
        .read_joystick_deadzone()
        .await
        .map_err(|e| e.to_string())?;
    let active_joystick_mode = session
        .read_active_joystick_hardware_mode(active_profile, active_layer)
        .await
        .map_err(|e| e.to_string())?;
    Ok(V070SnapshotPayload {
        session_id: session.id,
        version,
        active_profile,
        active_layer,
        active_joystick_mode,
        battery_percent,
        is_charging,
        mouse,
        deadzone,
        pm_config,
        pm_state,
        device_name,
    })
}

pub(crate) async fn execute_v070_command_on_session(
    session: &DeviceSession,
    cmd: &V070BridgeCommand,
) -> Result<V070BridgeOutcome, String> {
    if session.is_disconnected() || !session.has_exclusive_vendor_link() {
        return Err("需要已连接的 USB 专用独占会话".into());
    }
    let version = session
        .observed_firmware_version()
        .ok_or_else(|| "固件版本尚未确认".to_string())?;
    if !mi_model::supports_v070_hardware_capabilities(&version) {
        return Err(format!(
            "当前固件 {version} 低于 0.7.0-rc.16，不支持硬件能力接口"
        ));
    }
    match &cmd.command {
        ValidatedV070DeviceCommand::Refresh => fetch_v070_snapshot_on_session(session, true)
            .await
            .map(Box::new)
            .map(V070BridgeOutcome::Snapshot),
        ValidatedV070DeviceCommand::SelectLayer { layer } => {
            session
                .set_profile_layer_flashless(None, Some(*layer))
                .await
                .map_err(|e| e.to_string())?;
            let observed = session.observed_profile_layer();
            Ok(V070BridgeOutcome::ProfileLayer {
                profile_index: observed.map(|(p, _)| p),
                layer_index: observed.map(|(_, l)| l).unwrap_or(*layer),
            })
        }
        ValidatedV070DeviceCommand::ApplyJoystickHardwareMode { mode } => {
            let receipt = session
                .apply_joystick_hardware_mode(&cmd.document_dir, mode)
                .await
                .map_err(|e| e.to_string())?;
            if receipt.changed {
                if let Some(ref backup_dir) = cmd.active_six_layer_backup {
                    sync_six_layer_backup_after_joystick_write(
                        backup_dir,
                        &receipt.pre_write_keymap,
                        &receipt.post_write_keymap,
                    )?;
                }
            }
            Ok(V070BridgeOutcome::JoystickHardwareModeUpdated {
                mode: receipt.mode,
                active_profile: receipt.active_profile,
                active_layer: receipt.active_layer,
            })
        }
        ValidatedV070DeviceCommand::ApplyMousePreset { config }
        | ValidatedV070DeviceCommand::ApplyMouseConfig { config } => {
            let saved = session
                .write_mouse_config(config)
                .await
                .map_err(|e| e.to_string())?;
            Ok(V070BridgeOutcome::MouseConfigUpdated { mouse: saved })
        }
        ValidatedV070DeviceCommand::ApplyJoystickDeadzone { deadzone } => {
            let saved = session
                .write_joystick_deadzone(deadzone.v)
                .await
                .map_err(|e| e.to_string())?;
            Ok(V070BridgeOutcome::DeadzoneUpdated { deadzone: saved })
        }
        ValidatedV070DeviceCommand::ApplyPmConfig(cfg) => {
            let saved = session
                .write_pm_config(cfg)
                .await
                .map_err(|e| e.to_string())?;
            Ok(V070BridgeOutcome::PmConfigUpdated { pm_config: saved })
        }
        ValidatedV070DeviceCommand::RenameDevice(name) => {
            let info = session
                .write_device_name(name)
                .await
                .map_err(|e| e.to_string())?;
            Ok(V070BridgeOutcome::DeviceRenamed { device_name: info })
        }
        ValidatedV070DeviceCommand::ApplySaRgb {
            sa,
            rgb,
            brightness,
            speed,
            effect,
            update,
        } => {
            session
                .set_sa_rgb(update)
                .await
                .map_err(|e| e.to_string())?;
            Ok(V070BridgeOutcome::SaRgbApplied {
                sa: *sa,
                rgb: *rgb,
                brightness: *brightness,
                speed: *speed,
                effect: *effect,
            })
        }
        ValidatedV070DeviceCommand::ResetSaRgb { sa } => {
            session
                .reset_sa_rgb(vec![sa.get()])
                .await
                .map_err(|e| e.to_string())?;
            Ok(V070BridgeOutcome::SaRgbReset { sa: *sa })
        }
    }
}

fn map_layer_lighting_resolution(res: LayerLightingResolution) -> DesiredTarget {
    match res {
        LayerLightingResolution::Active(map) => DesiredTarget::Active(map),
        LayerLightingResolution::ProtectedOai => {
            DesiredTarget::UrgentClear(UrgentClearReason::ProtectedLayer)
        }
        LayerLightingResolution::MissingLayer => {
            DesiredTarget::Active(std::collections::BTreeMap::new())
        }
    }
}

enum V070CommandTarget {
    SixLayerBridge(std::sync::Arc<DeviceSession>),
    DirectSession(std::sync::Arc<DeviceSession>),
}

impl V070CommandTarget {
    fn into_session(self) -> std::sync::Arc<DeviceSession> {
        match self {
            Self::SixLayerBridge(session) | Self::DirectSession(session) => session,
        }
    }
}

impl WorkbenchStore {
    fn resolve_v070_command_target(&self) -> Result<V070CommandTarget, String> {
        if let Some(running) = &self.six_layer_running {
            let version = running
                .observed_firmware_version()
                .ok_or_else(|| "当前设备固件版本尚未确认，不支持 0.7.0 硬件能力".to_string())?;
            if !mi_model::supports_v070_hardware_capabilities(&version) {
                return Err(format!(
                    "当前固件 {version} 低于 0.7.0-rc.16，不支持硬件能力接口"
                ));
            }
            let session = running
                .live_vendor_session()
                .ok_or_else(|| "USB 专用桥接当前未连接".to_string())?;
            return Ok(V070CommandTarget::SixLayerBridge(session));
        }
        let session = self
            .device_session
            .as_ref()
            .ok_or_else(|| "未连接支持 0.7.0 硬件能力的 USB 专用设备".to_string())?;
        if session.is_disconnected() || !session.has_exclusive_vendor_link() {
            return Err("需要已连接的 USB 专用独占会话".to_string());
        }
        let version = session
            .observed_firmware_version()
            .ok_or_else(|| "固件版本尚未确认，请等待连接验证完成".to_string())?;
        if !mi_model::supports_v070_hardware_capabilities(&version) {
            return Err(format!(
                "当前固件 {version} 低于 0.7.0-rc.16，不支持硬件能力接口"
            ));
        }
        Ok(V070CommandTarget::DirectSession(std::sync::Arc::clone(
            session,
        )))
    }

    pub fn live_v070_session(&self) -> Option<std::sync::Arc<DeviceSession>> {
        self.resolve_v070_command_target()
            .ok()
            .map(V070CommandTarget::into_session)
    }

    pub(crate) fn teardown_v070_session_sa_rgb(&mut self) {
        let session = self.live_v070_session();
        self.sa_rgb_coordinator
            .teardown_blocking(&self.tokio_runtime, session);
    }

    pub(crate) fn sync_v070_session_sa_rgb(&mut self) {
        if self.system_lifecycle != SystemLifecycle::Awake
            || self.firmware_update.reserved()
            || self.is_lighting_preview_busy()
        {
            self.sa_rgb_coordinator
                .sync(DesiredTarget::Inactive, None, &self.tokio_runtime);
            return;
        }
        let Some(session) = self.live_v070_session() else {
            self.sa_rgb_coordinator
                .sync(DesiredTarget::Inactive, None, &self.tokio_runtime);
            return;
        };
        if !self.document_store.v070_session_sa_rgb_sync_enabled() {
            self.sa_rgb_coordinator.sync(
                DesiredTarget::Inactive,
                Some(session),
                &self.tokio_runtime,
            );
            return;
        }
        let target = self.resolve_v070_session_sa_rgb_target();
        self.sa_rgb_coordinator
            .sync(target, Some(session), &self.tokio_runtime);
    }

    fn resolve_v070_session_sa_rgb_target(&self) -> DesiredTarget {
        if !self.lighting_arbiter.lighting_enabled {
            return DesiredTarget::UrgentClear(UrgentClearReason::OptInDisabled);
        }
        let glow_on_working = self.lighting_arbiter.glow_on_working_sessions;
        let slot_kinds_for_source = |source: crate::store::LiveSessionSource| {
            self.slot_table_for_source(source)
                .slot_session_kinds(&self.lighting_arbiter.acknowledged_watermarks)
                .map(|kind| {
                    crate::v070_session_sa_rgb::filter_session_kind_for_lighting(
                        kind,
                        glow_on_working,
                    )
                })
        };
        let source_for_board_route = |route: &mi_model::board_layers::BoardLayerRoute| match route {
            mi_model::board_layers::BoardLayerRoute::HostProfile { profile_id } => {
                self.source_for_profile_id(profile_id)
            }
            mi_model::board_layers::BoardLayerRoute::Native {
                owner: mi_model::board_layers::NativeLayerOwner::Codex,
                ..
            } => crate::store::LiveSessionSource::Codex,
            _ => self.desired_live_session_source(),
        };
        let fallback_sa = self
            .v070_hardware
            .projection
            .sa_rgb
            .sa
            .and_then(mi_model::SaId::new);

        if let Some(running) = &self.six_layer_running {
            let Some(l) = self
                .v070_hardware
                .projection
                .active_layer
                .or_else(|| {
                    Some(
                        running
                            .observed_layer
                            .load(std::sync::atomic::Ordering::Acquire),
                    )
                })
                .filter(|l| (1..=6).contains(l))
            else {
                return DesiredTarget::UrgentClear(UrgentClearReason::ProtectedLayer);
            };
            let Ok(board_layer) = mi_model::board_layers::BoardLayer::try_from(l) else {
                return DesiredTarget::UrgentClear(UrgentClearReason::ProtectedLayer);
            };
            let route = self
                .document_store
                .board_layer_plan()
                .map(|p| p.route(board_layer))
                .unwrap_or_else(|| running.installation.plan.desired.route(board_layer));
            if matches!(
                route,
                mi_model::board_layers::BoardLayerRoute::Native { .. }
            ) {
                return DesiredTarget::UrgentClear(UrgentClearReason::ProtectedLayer);
            }
            let board_source = if let Some(locked_id) = self.document_store.locked_profile_id() {
                self.source_for_profile_id(&locked_id)
            } else if self.six_layer_user_pinned {
                self.document_store
                    .host_active_profile_id()
                    .map(|pid| self.source_for_profile_id(&pid))
                    .unwrap_or_else(|| source_for_board_route(route))
            } else {
                source_for_board_route(route)
            };
            let slot_kinds = slot_kinds_for_source(board_source);
            let compiled_layer = running.installation.plan.compiled_board_layer(board_layer);
            return map_layer_lighting_resolution(resolve_desired_for_device_layer(
                &compiled_layer,
                &slot_kinds,
                None,
            ));
        }

        let doc = &self.document_store.draft.document;
        let is_device_snapshot = doc.origin["kind"] == "device-snapshot";
        let resolve_snapshot_board_layer = |layer_1_based: u8| -> Option<DesiredTarget> {
            if !is_device_snapshot || layer_1_based < 1 {
                return None;
            }
            let p = self.v070_hardware.projection.active_profile?;
            let dev_layer = self
                .document_store
                .draft
                .config
                .profiles
                .get(usize::from(p))
                .and_then(|prof| prof.layers.get(usize::from(layer_1_based - 1)))?;
            let board_source = mi_model::board_layers::BoardLayer::try_from(layer_1_based)
                .ok()
                .and_then(|bl| {
                    self.document_store
                        .board_layer_plan()
                        .map(|plan| source_for_board_route(plan.route(bl)))
                })
                .unwrap_or_else(|| self.desired_live_session_source());
            let slot_kinds = slot_kinds_for_source(board_source);
            Some(map_layer_lighting_resolution(
                resolve_desired_for_device_layer(dev_layer, &slot_kinds, fallback_sa),
            ))
        };

        if is_device_snapshot && doc.editable.graph.profiles.is_empty() {
            let target_layer = self
                .v070_hardware
                .device_layer_override
                .or(self.v070_hardware.projection.active_layer);
            return target_layer
                .and_then(resolve_snapshot_board_layer)
                .unwrap_or_else(|| DesiredTarget::Active(std::collections::BTreeMap::new()));
        }

        let Some(active_pid) = self
            .effective_resolved_active_profile_id()
            .or_else(|| self.selected_profile_id.clone())
        else {
            return DesiredTarget::Active(std::collections::BTreeMap::new());
        };
        let Some(profile) = doc.editable.graph.profiles.get(&active_pid) else {
            return DesiredTarget::Active(std::collections::BTreeMap::new());
        };
        let slot_kinds = slot_kinds_for_source(self.source_for_profile_id(&active_pid));

        if let Some(l) = self.v070_hardware.device_layer_override.filter(|&l| l >= 1) {
            if let Some(lid) = profile.layer_order.get(usize::from(l - 1)) {
                return map_layer_lighting_resolution(resolve_desired_for_document_layer(
                    doc,
                    lid,
                    &slot_kinds,
                    fallback_sa,
                ));
            }
            return resolve_snapshot_board_layer(l)
                .unwrap_or_else(|| DesiredTarget::Active(std::collections::BTreeMap::new()));
        }

        if self.is_active_lighting_protected() {
            return DesiredTarget::UrgentClear(UrgentClearReason::ProtectedLayer);
        }

        let active_lid = if self.selected_profile_id.as_deref() == Some(active_pid.as_str()) {
            self.selected_layer_id
                .as_ref()
                .filter(|lid| profile.layer_order.iter().any(|id| id == *lid))
                .or_else(|| profile.layer_order.first())
        } else {
            profile.layer_order.first()
        };
        let Some(lid) = active_lid else {
            return DesiredTarget::Active(std::collections::BTreeMap::new());
        };
        map_layer_lighting_resolution(resolve_desired_for_document_layer(
            doc,
            lid,
            &slot_kinds,
            fallback_sa,
        ))
    }

    pub(super) fn has_active_v070_device(&self) -> bool {
        self.resolve_v070_command_target().is_ok()
    }

    pub(super) fn schedule_v070_refresh(&mut self) -> Result<(), String> {
        self.dispatch_v070_hardware_action(V070HardwareAction::Refresh)
    }

    pub(super) fn trigger_v070_session_snapshot_if_supported(
        &mut self,
        session: &std::sync::Arc<DeviceSession>,
    ) {
        if session.is_disconnected()
            || !session.has_exclusive_vendor_link()
            || !session
                .observed_firmware_version()
                .as_deref()
                .is_some_and(mi_model::supports_v070_hardware_capabilities)
        {
            return;
        }
        let version = session.observed_firmware_version().unwrap_or_default();
        let observed_pl = session.observed_profile_layer();
        let (battery, charging) = session.observed_battery_status();
        self.v070_hardware.mark_supported(
            &version,
            observed_pl.map(|(p, _)| p),
            observed_pl.map(|(_, l)| l),
            battery,
            charging,
        );
        if self.v070_hardware.last_snapshot_session_id == Some(session.id) {
            return;
        }
        self.v070_hardware.last_snapshot_session_id = Some(session.id);
        let session_clone = std::sync::Arc::clone(session);
        let inbox = self.hardware_event_tx.clone();
        self.tokio_runtime.spawn(async move {
            let mut notif_rx = session_clone.notifications();
            let snap_result = fetch_v070_snapshot_on_session(&session_clone, false)
                .await
                .map(Box::new)
                .map(V070BridgeOutcome::Snapshot);
            let _ = inbox.send(HardwareLoopEvent::V070CapabilityOutcome {
                session_id: session_clone.id,
                seq: 0,
                result: snap_result,
            });
            loop {
                tokio::select! {
                    _ = session_clone.wait_disconnected() => break,
                    received = notif_rx.recv() => {
                        match received {
                            Ok(msg) => {
                                if let mi_proto::HostNotification::ProfileLayer(ref raw) = msg.notification {
                                    if let Some((profile_index, layer_index)) =
                                        mi_proto::v070_capabilities::parse_profile_layer_notification(raw)
                                    {
                                        let _ = inbox.send(HardwareLoopEvent::V070ProfileLayerChanged {
                                            session_id: session_clone.id,
                                            profile_index,
                                            layer_index,
                                        });
                                    }
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                }
            }
        });
    }

    pub(crate) fn resolve_safe_auto_sync_board_layer(&self) -> Option<u8> {
        if !self.document_store.v070_auto_layer_sync_enabled() {
            return None;
        }
        if self.system_lifecycle != SystemLifecycle::Awake
            || self.firmware_update.reserved()
            || self.six_layer_job.is_some()
        {
            return None;
        }
        if !self.has_active_v070_device()
            || self.v070_hardware.projection.host_sleep_status != "awake"
        {
            return None;
        }
        if !self.document_store.host_execution_enabled() {
            return None;
        }
        let target_pid = if let Some(locked_pid) = self.document_store.locked_profile_id() {
            locked_pid
        } else if self.six_layer_running.is_none() || self.six_layer_user_pinned {
            self.document_store.host_active_profile_id()?
        } else {
            return None;
        };
        let doc = &self.document_store.host_store().draft.document;
        let profile = doc.editable.graph.profiles.get(&target_pid)?;
        if profile
            .layer_order
            .iter()
            .any(|lid| doc.is_protected_oai_layer(lid))
        {
            return None;
        }
        let plan = self.document_store.board_layer_plan()?;
        let installed_plan = self
            .six_layer_running
            .as_ref()
            .map(|running| &running.installation.plan.desired);
        for layer in mi_model::board_layers::BoardLayer::ALL {
            let plan_matches = matches!(
                plan.route(layer),
                mi_model::board_layers::BoardLayerRoute::HostProfile { profile_id }
                    if profile_id == &target_pid
            );
            if !plan_matches {
                continue;
            }
            if let Some(desired) = installed_plan {
                let desired_matches = matches!(
                    desired.route(layer),
                    mi_model::board_layers::BoardLayerRoute::HostProfile { profile_id }
                        if profile_id == &target_pid
                );
                if !desired_matches {
                    continue;
                }
            }
            return Some(layer.number());
        }
        None
    }

    pub(crate) fn maybe_sync_v070_active_profile_layer(&mut self) -> bool {
        if self.is_lighting_preview_busy() {
            return false;
        }
        let Some(layer) = self.resolve_safe_auto_sync_board_layer() else {
            self.v070_hardware.pending_auto_layer_sync = None;
            return false;
        };
        if self.v070_hardware.projection.active_layer == Some(layer)
            && self.v070_hardware.pending_auto_layer_sync.is_none()
        {
            return false;
        }
        if self.v070_hardware.projection.busy {
            self.v070_hardware.pending_auto_layer_sync = Some(layer);
            return false;
        }
        self.v070_hardware.pending_auto_layer_sync = None;
        self.v070_hardware.expected_layer_echo =
            Some((layer, self.update_now + std::time::Duration::from_secs(2)));
        if self
            .dispatch_v070_hardware_action(V070HardwareAction::SelectLayer { layer })
            .is_err()
        {
            self.v070_hardware.expected_layer_echo = None;
            return false;
        }
        true
    }

    pub(super) fn dispatch_v070_hardware(
        &mut self,
        action: V070HardwareAction,
    ) -> Result<(), WorkbenchError> {
        let result = self
            .require_awake()
            .map_err(|e| e.to_string())
            .and_then(|()| {
                if self.firmware_update.projection.busy {
                    return Err("固件操作正在进行，请等待完成".into());
                }
                self.dispatch_v070_hardware_action(action)
            });
        if let Err(error) = &result {
            self.v070_hardware.projection.last_error = Some(error.clone());
            self.v070_hardware.projection.status_message = error.clone();
        }
        self.commit_runtime_update();
        result.map_err(WorkbenchError::Routing)
    }

    pub(super) fn dispatch_v070_hardware_action(
        &mut self,
        action: V070HardwareAction,
    ) -> Result<(), String> {
        if self.firmware_update.reserved() {
            return Err("固件更新流程正在占用设备，请结束后再试".into());
        }
        let parsed = parse_v070_action(
            action,
            &self.v070_hardware.projection.mouse,
            self.v070_hardware.projection.device_name.max_length,
        )?;
        match parsed {
            ParsedV070Action::HostPreference(V070HostPreference::AutoLayerSync { enabled }) => {
                self.document_store
                    .set_v070_auto_layer_sync_enabled(enabled)?;
                self.v070_hardware.projection.last_error = None;
                if enabled {
                    self.v070_hardware.projection.status_message =
                        "已开启方案切层联动（仅 RAM 零擦写）".into();
                    let _ = self.maybe_sync_v070_active_profile_layer();
                } else {
                    self.v070_hardware.pending_auto_layer_sync = None;
                    self.v070_hardware.expected_layer_echo = None;
                    self.v070_hardware.projection.status_message = "已关闭方案切层联动".into();
                }
                Ok(())
            }
            ParsedV070Action::HostPreference(V070HostPreference::SessionSaRgbSync { enabled }) => {
                self.document_store
                    .set_v070_session_sa_rgb_sync_enabled(enabled)?;
                self.v070_hardware.projection.last_error = None;
                if enabled {
                    self.v070_hardware.projection.status_message =
                        "已开启 AI 会话状态自动映灯".into();
                    self.sync_v070_session_sa_rgb();
                } else {
                    if self.is_lighting_preview_busy() {
                        self.sa_rgb_coordinator.sync(
                            DesiredTarget::Inactive,
                            None,
                            &self.tokio_runtime,
                        );
                    } else {
                        let session = self.live_v070_session();
                        self.sa_rgb_coordinator.sync(
                            DesiredTarget::UrgentClear(UrgentClearReason::OptInDisabled),
                            session,
                            &self.tokio_runtime,
                        );
                    }
                    self.v070_hardware.projection.status_message =
                        "已关闭 AI 会话状态自动映灯".into();
                }
                Ok(())
            }
            ParsedV070Action::DeviceCommand(command) => {
                if self.v070_hardware.projection.busy {
                    return Err("上一条 0.7.0 硬件指令尚未完成".into());
                }
                let _ = self.resolve_v070_command_target()?;
                if self.is_lighting_preview_busy() {
                    self.end_lighting_preview_blocking(
                        crate::store::lighting_preview_store::LightingPreviewStopReason::ExclusiveDeviceTransition,
                    );
                }
                let document_dir = self.config.document_dir.clone();
                let active_six_layer_backup = self
                    .six_layer_running
                    .as_ref()
                    .map(|running| running.installation.backup.clone())
                    .or_else(|| {
                        self.six_layer_state
                            .installation
                            .as_ref()
                            .filter(|inst| {
                                inst.status == crate::six_layer_runtime::InstallationStatus::Applied
                            })
                            .map(|inst| inst.backup.clone())
                    });
                match self.resolve_v070_command_target()? {
                    V070CommandTarget::SixLayerBridge(_) => {
                        let seq = self.v070_hardware.allocate_seq();
                        let cmd = V070BridgeCommand {
                            seq,
                            command,
                            document_dir,
                            active_six_layer_backup,
                        };
                        self.six_layer_running
                            .as_ref()
                            .ok_or_else(|| "USB 专用桥接当前未连接".to_string())?
                            .send_v070_command(cmd)?;
                        self.v070_hardware.projection.busy = true;
                        self.v070_hardware.in_flight_seq = Some(seq);
                        self.v070_hardware.projection.last_error = None;
                        Ok(())
                    }
                    V070CommandTarget::DirectSession(session) => {
                        let seq = self.v070_hardware.allocate_seq();
                        let cmd = V070BridgeCommand {
                            seq,
                            command,
                            document_dir,
                            active_six_layer_backup,
                        };
                        self.v070_hardware.projection.busy = true;
                        self.v070_hardware.in_flight_seq = Some(seq);
                        self.v070_hardware.projection.last_error = None;
                        let inbox = self.hardware_event_tx.clone();
                        self.tokio_runtime.spawn(async move {
                            let result = execute_v070_command_on_session(&session, &cmd).await;
                            let _ = inbox.send(HardwareLoopEvent::V070CapabilityOutcome {
                                session_id: session.id,
                                seq,
                                result,
                            });
                        });
                        Ok(())
                    }
                }
            }
        }
    }

    fn owns_v070_session_id(&self, session_id: u64) -> bool {
        let owns_six_layer = self
            .six_layer_running
            .as_ref()
            .is_some_and(|running| running.vendor_session_id() == Some(session_id));
        let owns_direct = self
            .device_session
            .as_ref()
            .is_some_and(|session| session.id == session_id);
        owns_six_layer || owns_direct
    }

    pub(super) fn handle_v070_capability_outcome(
        &mut self,
        session_id: u64,
        seq: u64,
        result: Result<V070BridgeOutcome, String>,
    ) {
        if !self.owns_v070_session_id(session_id) {
            return;
        }
        let mut is_manual_sa_rgb = false;
        if let Ok(ref outcome) = result {
            let sync_enabled = self.document_store.v070_session_sa_rgb_sync_enabled();
            match outcome {
                V070BridgeOutcome::SaRgbApplied {
                    sa,
                    rgb,
                    brightness,
                    speed,
                    effect,
                } => {
                    is_manual_sa_rgb = true;
                    let style =
                        SaRgbLitStyle::from_manual_apply(*rgb, *brightness, *speed, effect.as_u8());
                    self.sa_rgb_coordinator.note_manual_sa_rgb_write(
                        session_id,
                        ManualSaRgbWrite::Applied { sa: *sa, style },
                        sync_enabled,
                    );
                }
                V070BridgeOutcome::SaRgbReset { sa } => {
                    is_manual_sa_rgb = true;
                    self.sa_rgb_coordinator.note_manual_sa_rgb_write(
                        session_id,
                        ManualSaRgbWrite::Reset { sa: *sa },
                        sync_enabled,
                    );
                }
                V070BridgeOutcome::ProfileLayer { layer_index, .. } => {
                    self.v070_hardware.device_layer_override = Some(*layer_index);
                }
                V070BridgeOutcome::JoystickHardwareModeUpdated { mode, .. } => {
                    let plan_hw_mode = match mode.as_str() {
                        "mouse" => Some("MOUSE".to_string()),
                        "scroll" => Some("SCROLL".to_string()),
                        _ => None,
                    };
                    if let Some(running) = self.six_layer_running.as_mut() {
                        running.installation.plan.codex_joystick_mode = plan_hw_mode.clone();
                        for host in &mut running.installation.plan.hosts {
                            host.joystick_mode = plan_hw_mode.clone();
                        }
                    }
                    if let Some(inst) = self.six_layer_state.installation.as_mut() {
                        inst.plan.codex_joystick_mode = plan_hw_mode.clone();
                        for host in &mut inst.plan.hosts {
                            host.joystick_mode = plan_hw_mode.clone();
                        }
                        if !self.config.isolated {
                            let state_path =
                                self.config.document_dir.join("six-layer-runtime.json");
                            if state_path.is_file() {
                                let _ = crate::six_layer_runtime::save(
                                    &state_path,
                                    &self.six_layer_state,
                                );
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        let is_profile_layer = matches!(result, Ok(V070BridgeOutcome::ProfileLayer { .. }));
        let succeeded = result.is_ok();
        self.v070_hardware.apply_outcome(seq, result);
        if is_profile_layer {
            if let Some(session) = self.live_v070_session() {
                if let Some(cached_mode) = session.cached_joystick_hardware_mode(
                    self.v070_hardware.projection.active_profile,
                    self.v070_hardware.projection.active_layer,
                ) {
                    self.v070_hardware.projection.active_joystick_mode = Some(cached_mode);
                }
            }
        }
        if succeeded {
            if !self.v070_hardware.projection.busy {
                let pending = self.v070_hardware.pending_auto_layer_sync.take();
                if pending.is_some() || seq == 0 {
                    let _ = self.maybe_sync_v070_active_profile_layer();
                }
            }
            if !is_manual_sa_rgb {
                self.sync_v070_session_sa_rgb();
            }
        } else {
            self.v070_hardware.pending_auto_layer_sync = None;
            self.v070_hardware.expected_layer_echo = None;
        }
        self.commit_runtime_update();
    }

    pub(super) fn handle_v070_profile_layer_changed(
        &mut self,
        session_id: u64,
        profile_index: u8,
        layer_index: u8,
    ) {
        if !self.owns_v070_session_id(session_id) {
            return;
        }
        self.v070_hardware.device_layer_override = Some(layer_index);
        let is_expected_echo = self
            .v070_hardware
            .consume_expected_layer_echo(layer_index, self.update_now);
        self.v070_hardware
            .apply_profile_layer_notification(profile_index, layer_index);
        if let Some(session) = self.live_v070_session() {
            if let Some(cached_mode) =
                session.cached_joystick_hardware_mode(Some(profile_index), Some(layer_index))
            {
                self.v070_hardware.projection.active_joystick_mode = Some(cached_mode);
            }
        }
        let mut unpinned = false;
        let mut source_synced = false;
        if !is_expected_echo
            && self.document_store.locked_profile_id().is_none()
            && self.six_layer_user_pinned
        {
            self.six_layer_user_pinned = false;
            self.sync_six_layer_routing_context();
            source_synced = self.sync_live_session_source();
            unpinned = true;
        }
        if is_expected_echo {
            self.v070_hardware.projection.status_message =
                format!("方案切层联动已切至 P{profile_index} · 第 {layer_index} 层（RAM 零擦写）");
        }
        if unpinned {
            if !source_synced {
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
            } else if is_expected_echo {
                self.commit_runtime_update();
            }
        } else {
            self.sync_v070_session_sa_rgb();
            self.commit_runtime_update();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use mi_proto::v070_capabilities::{MouseConfig, PmConfig};
    use mi_proto::{Framer, ProtoError};
    use mi_session::{DeviceSession, ReadOnlySchema, SessionConfig};
    use mi_transport::{HidLink, LinkBackend, TransportError};
    use serde_json::{json, Value};

    use super::*;
    use crate::six_layer_runtime::RunningSixLayer;
    use crate::store::WorkbenchCommand;

    pub(super) struct TestFramer;
    impl Framer for TestFramer {
        fn encode(&self, payload: &[u8]) -> Result<Vec<Vec<u8>>, ProtoError> {
            Ok(vec![payload.to_vec()])
        }
        fn feed(&mut self, report: &[u8]) -> Result<Vec<Vec<u8>>, ProtoError> {
            Ok(vec![report.to_vec()])
        }
        fn clear(&mut self) {}
    }

    #[derive(Debug, Clone, PartialEq)]
    pub(super) enum MockWireEvent {
        SaRgb(Value),
        LinkClosed,
    }

    pub(super) struct MockV070Link {
        in_tx: std::sync::Mutex<Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>>,
        in_rx: tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>>,
        out_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
        closed: std::sync::atomic::AtomicBool,
        pub(super) wire_events: Arc<Mutex<Vec<MockWireEvent>>>,
    }

    impl MockV070Link {
        pub(super) fn new() -> (Arc<Self>, tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>) {
            let (in_tx, in_rx) = tokio::sync::mpsc::unbounded_channel();
            let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel();
            (
                Arc::new(Self {
                    in_tx: std::sync::Mutex::new(Some(in_tx)),
                    in_rx: tokio::sync::Mutex::new(in_rx),
                    out_tx,
                    closed: std::sync::atomic::AtomicBool::new(false),
                    wire_events: Arc::new(Mutex::new(Vec::new())),
                }),
                out_rx,
            )
        }

        pub(super) fn push_wire(&self, value: Value) {
            if let Some(tx) = self.in_tx.lock().unwrap().as_ref() {
                let _ = tx.send(serde_json::to_vec(&value).unwrap());
            }
        }

        pub(super) fn sa_rgb_params(&self) -> Vec<Value> {
            self.wire_events
                .lock()
                .unwrap()
                .iter()
                .filter_map(|ev| match ev {
                    MockWireEvent::SaRgb(params) => Some(params.clone()),
                    MockWireEvent::LinkClosed => None,
                })
                .collect()
        }

        pub(super) fn clear_wire_events(&self) {
            self.wire_events.lock().unwrap().clear();
        }
    }

    impl HidLink for MockV070Link {
        fn is_exclusive_vendor(&self) -> bool {
            true
        }
        fn backend(&self) -> LinkBackend {
            LinkBackend::MacHidapi267Numbered
        }
        fn write<'a, 'c, 'b>(
            &'a self,
            report: &'c [u8],
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<(), TransportError>> + Send + 'b>,
        >
        where
            'a: 'b,
            'c: 'b,
            Self: 'b,
        {
            Box::pin(async move {
                if self.closed.load(std::sync::atomic::Ordering::SeqCst) {
                    return Err(TransportError::Disconnected);
                }
                if let Ok(req) = serde_json::from_slice::<Value>(report) {
                    if req.get("method").and_then(Value::as_str) == Some("kb.sa.rgb") {
                        self.wire_events
                            .lock()
                            .unwrap()
                            .push(MockWireEvent::SaRgb(req["params"].clone()));
                    }
                }
                let _ = self.out_tx.send(report.to_vec());
                Ok(())
            })
        }
        fn read<'a, 'b>(
            &'a self,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Vec<u8>, TransportError>> + Send + 'b>,
        >
        where
            'a: 'b,
            Self: 'b,
        {
            Box::pin(async move {
                let mut rx = self.in_rx.lock().await;
                rx.recv().await.ok_or(TransportError::Disconnected)
            })
        }
        fn close(&self) {
            if !self.closed.swap(true, std::sync::atomic::Ordering::SeqCst) {
                self.wire_events
                    .lock()
                    .unwrap()
                    .push(MockWireEvent::LinkClosed);
            }
            self.in_tx.lock().unwrap().take();
        }
    }

    pub(super) fn spawn_mock_v070_session(
        runtime: &tokio::runtime::Runtime,
        version: &str,
        sent_methods: Arc<Mutex<Vec<String>>>,
    ) -> (
        Arc<DeviceSession>,
        Arc<MockV070Link>,
        tokio::task::JoinHandle<()>,
    ) {
        let (link, mut out_rx) = MockV070Link::new();
        let session = runtime.block_on(async {
            DeviceSession::new(
                link.clone(),
                Box::new(TestFramer),
                Arc::new(ReadOnlySchema),
                SessionConfig {
                    timeout: Duration::from_millis(500),
                    interval: Duration::ZERO,
                },
            )
        });
        let version_s = version.to_string();
        let link_responder = link.clone();
        let responder = runtime.spawn(async move {
            let mut mouse = MouseConfig::default();
            let mut deadzone = 0.15_f64;
            let mut pm_cfg = PmConfig::default();
            let mut name = "Creator Micro 2".to_string();
            let mut layer = 1_u8;
            while let Some(raw) = out_rx.recv().await {
                let req: Value = serde_json::from_slice(&raw).unwrap();
                let id = req["id"].clone();
                let method = req["method"].as_str().unwrap().to_string();
                sent_methods.lock().unwrap().push(method.clone());
                let result = match method.as_str() {
                    "device.status" => json!({
                        "version": version_s,
                        "profile_index": 3,
                        "layer_index": layer,
                        "battery": 91,
                        "is_charging": false
                    }),
                    "device.name" if req["params"].is_null() => json!({
                        "name": name,
                        "factory_name": "Creator Micro 2",
                        "max_length": 15
                    }),
                    "device.name" => {
                        name = req["params"]["name"].as_str().unwrap().to_string();
                        json!({
                            "name": name,
                            "factory_name": "Creator Micro 2",
                            "max_length": 15
                        })
                    }
                    "pm.config" if req["params"].is_null() => serde_json::to_value(pm_cfg).unwrap(),
                    "pm.config" => {
                        pm_cfg = serde_json::from_value(req["params"].clone()).unwrap();
                        serde_json::to_value(pm_cfg).unwrap()
                    }
                    "pm.state" => json!({
                        "state": "active",
                        "floor": "usb",
                        "base_floor": "host_sleep",
                        "line_powered": true,
                        "auto_sleep": pm_cfg.auto_sleep,
                        "sleep_after_s": u32::from(pm_cfg.sleep_after_min) * 60,
                        "idle_s": 5,
                        "standby_s": 3600,
                        "credit_s": 0,
                        "holds": [{"owner": "usb.host", "floor": "usb", "age_ms": 1500}]
                    }),
                    "mouse.config" if req["params"].is_null() => {
                        serde_json::to_value(mouse).unwrap()
                    }
                    "mouse.config" => {
                        mouse = serde_json::from_value(req["params"].clone()).unwrap();
                        serde_json::to_value(mouse).unwrap()
                    }
                    "joystick.deadzone" if req["params"].is_null() => json!({"v": deadzone}),
                    "joystick.deadzone" => {
                        deadzone = req["params"]["v"].as_f64().unwrap();
                        json!({"v": deadzone})
                    }
                    "kb.pl.set" => {
                        if let Some(l) = req["params"].get("l").and_then(Value::as_u64) {
                            layer = l as u8;
                        }
                        json!({})
                    }
                    "kb.sa.rgb" => json!({}),
                    _ => json!({}),
                };
                link_responder.push_wire(json!({
                    "id": id,
                    "method": method,
                    "result": result
                }));
            }
        });
        runtime.block_on(async {
            session.read_identity_info().await.unwrap();
        });
        (session, link, responder)
    }

    pub(super) fn poll_until(store: &mut WorkbenchStore, pred: impl Fn(&WorkbenchStore) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !pred(store) {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for store condition; v070={:?}",
                store.projection().v070_hardware
            );
            store.poll_at(Instant::now());
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn v070_initial_snapshot_and_action_dispatch_over_vendor_bridge() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(dir.path()).unwrap();
        let sent_methods = Arc::new(Mutex::new(Vec::new()));
        let (session, link, responder) =
            spawn_mock_v070_session(&store.tokio_runtime, "0.7.0-rc.16", sent_methods.clone());

        let mut running = RunningSixLayer::isolated(session.clone(), dir.path());
        running.readiness.configuration_observed = true;
        running.readiness.keyboard_grabbed = true;
        running.set_v070_inbox(store.hardware_event_tx.clone());
        running.attach_test_vendor_session(&store.tokio_runtime, session.clone());
        store.six_layer_running = Some(running);

        poll_until(&mut store, |s| s.projection().v070_hardware.synced);
        let p = store.projection();
        assert!(p.v070_hardware.supported);
        assert_eq!(p.v070_hardware.battery_percent, Some(91));
        assert_eq!(p.v070_hardware.is_charging, Some(false));
        assert_eq!(p.v070_hardware.active_profile, Some(3));
        assert_eq!(p.v070_hardware.active_layer, Some(1));
        assert_eq!(p.v070_hardware.device_name.max_length, 15);
        assert!(p.v070_hardware.pm_state.usb_host_hold);
        assert_eq!(p.v070_hardware.mouse.move_delta, 14);
        assert_eq!(p.v070_hardware.mouse.stick_speed, 24);
        assert_eq!(p.v070_hardware.mouse.stick_regulator, 10);
        assert_eq!(p.v070_hardware.mouse.deadzone, 0.15);

        let before_invalid = store.projection().v070_hardware.clone();
        assert!(store
            .dispatch_command(WorkbenchCommand::V070Hardware(
                V070HardwareAction::ApplyPmConfig {
                    auto_sleep: true,
                    host_sleep: true,
                    sleep_after_min: 0,
                }
            ))
            .is_err());
        assert!(!store.projection().v070_hardware.busy);
        assert_eq!(
            store.projection().v070_hardware.pm_config,
            before_invalid.pm_config
        );

        store
            .dispatch_command(WorkbenchCommand::V070Hardware(
                V070HardwareAction::SelectLayer { layer: 2 },
            ))
            .unwrap();
        poll_until(&mut store, |s| {
            !s.projection().v070_hardware.busy
                && s.projection().v070_hardware.active_layer == Some(2)
        });
        assert_eq!(
            store.projection().sidebar.board_layers.observed_layer,
            Some(2)
        );
        assert_eq!(
            store
                .six_layer_running
                .as_ref()
                .and_then(|r| r.observed_v070_profile_layer()),
            Some((3, 2))
        );
        assert!(
            !sent_methods
                .lock()
                .unwrap()
                .iter()
                .any(|m| m == "fs.writebin"),
            "kb.pl.set must stay flashless"
        );

        store
            .dispatch_command(WorkbenchCommand::V070Hardware(
                V070HardwareAction::ApplyMousePreset {
                    preset: "fast".into(),
                },
            ))
            .unwrap();
        poll_until(&mut store, |s| {
            !s.projection().v070_hardware.busy
                && s.projection().v070_hardware.mouse.preset == "fast"
        });
        assert_eq!(store.projection().v070_hardware.mouse.move_delta, 18);

        let before_custom_mouse_idx = sent_methods.lock().unwrap().len();
        store
            .dispatch_command(WorkbenchCommand::V070Hardware(
                V070HardwareAction::ApplyMouseConfig {
                    move_delta: 12,
                    max_speed: 20,
                    wheel_delta: 2,
                    wheel_max_speed: 12,
                    stick_speed: 3,
                    stick_scroll: 30,
                },
            ))
            .unwrap();
        poll_until(&mut store, |s| {
            !s.projection().v070_hardware.busy
                && s.projection().v070_hardware.mouse.move_delta == 12
        });
        {
            let guard = sent_methods.lock().unwrap();
            let slice = &guard[before_custom_mouse_idx..];
            assert!(slice.iter().any(|m| m == "mouse.config"));
            assert!(!slice.iter().any(|m| m == "joystick.deadzone"));
        }
        assert_eq!(store.projection().v070_hardware.mouse.preset, "custom");
        assert!((store.projection().v070_hardware.mouse.deadzone - 0.15).abs() < 1e-9);

        let before_deadzone_idx = sent_methods.lock().unwrap().len();
        store
            .dispatch_command(WorkbenchCommand::V070Hardware(
                V070HardwareAction::ApplyJoystickDeadzone { deadzone: 0.25 },
            ))
            .unwrap();
        poll_until(&mut store, |s| {
            !s.projection().v070_hardware.busy
                && (s.projection().v070_hardware.mouse.deadzone - 0.25).abs() < 1e-9
        });
        {
            let guard = sent_methods.lock().unwrap();
            let slice = &guard[before_deadzone_idx..];
            assert!(slice.iter().any(|m| m == "joystick.deadzone"));
            assert!(!slice.iter().any(|m| m == "mouse.config"));
        }
        assert_eq!(store.projection().v070_hardware.mouse.preset, "custom");
        assert_eq!(store.projection().v070_hardware.mouse.move_delta, 12);

        store
            .dispatch_command(WorkbenchCommand::V070Hardware(
                V070HardwareAction::RenameDevice {
                    name: "MicroStudio".into(),
                },
            ))
            .unwrap();
        poll_until(&mut store, |s| {
            !s.projection().v070_hardware.busy
                && s.projection().v070_hardware.device_name.name == "MicroStudio"
        });

        store
            .dispatch_command(WorkbenchCommand::V070Hardware(
                V070HardwareAction::ApplySaRgb {
                    sa: 7057,
                    color_hex: "#33AAFF".into(),
                    brightness: 0.75,
                    speed: 0.5,
                    effect: 4,
                },
            ))
            .unwrap();
        poll_until(&mut store, |s| {
            !s.projection().v070_hardware.busy
                && s.projection().v070_hardware.sa_rgb.sa == Some(7057)
                && s.projection().v070_hardware.sa_rgb.effect == 4
        });

        store
            .dispatch_command(WorkbenchCommand::V070Hardware(
                V070HardwareAction::ResetSaRgb { sa: 7057 },
            ))
            .unwrap();
        poll_until(&mut store, |s| {
            !s.projection().v070_hardware.busy && s.projection().v070_hardware.sa_rgb.effect == 0
        });

        link.push_wire(json!({"m": "kb.pl", "p": {"p": 3, "l": 5}}));
        poll_until(&mut store, |s| {
            s.projection().v070_hardware.active_layer == Some(5)
                && s.projection().sidebar.board_layers.observed_layer == Some(5)
        });

        store.system_will_sleep();
        assert_eq!(
            store.projection().v070_hardware.host_sleep_status,
            "sleeping"
        );
        assert!(!store.projection().v070_hardware.supported);
        store.system_did_wake();
        assert!(matches!(
            store.projection().v070_hardware.host_sleep_status.as_str(),
            "waking" | "awake"
        ));
        poll_until(&mut store, |s| {
            s.projection().v070_hardware.host_sleep_status == "awake"
        });

        if let Some(running) = store.six_layer_running.take() {
            running.shutdown(&store.tokio_runtime).unwrap();
        }
        responder.abort();
    }

    #[test]
    fn v063_device_never_triggers_snapshot_and_rejects_v070_actions() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(dir.path()).unwrap();
        let sent_methods = Arc::new(Mutex::new(Vec::new()));
        let (session, _link, responder) =
            spawn_mock_v070_session(&store.tokio_runtime, "0.6.3", sent_methods.clone());

        let mut running = RunningSixLayer::isolated(session.clone(), dir.path());
        running.set_v070_inbox(store.hardware_event_tx.clone());
        running.attach_test_vendor_session(&store.tokio_runtime, session.clone());
        store.six_layer_running = Some(running);

        std::thread::sleep(Duration::from_millis(80));
        store.poll_at(Instant::now());
        assert!(!store.projection().v070_hardware.supported);
        assert!(!store.projection().v070_hardware.synced);
        assert!(store
            .dispatch_command(WorkbenchCommand::V070Hardware(V070HardwareAction::Refresh,))
            .is_err());
        assert_eq!(
            *sent_methods.lock().unwrap(),
            vec!["device.status".to_string()]
        );

        if let Some(running) = store.six_layer_running.take() {
            running.shutdown(&store.tokio_runtime).unwrap();
        }
        responder.abort();
    }

    #[test]
    fn system_wake_triggers_lightweight_refresh_when_v070_device_active() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(dir.path()).unwrap();
        let sent_methods = Arc::new(Mutex::new(Vec::new()));
        let (session, _link, responder) =
            spawn_mock_v070_session(&store.tokio_runtime, "0.7.0", sent_methods.clone());

        store.device_session = Some(session.clone());
        store.system_lifecycle = crate::store::SystemLifecycle::Sleeping;
        store.v070_hardware.set_host_sleep_status("sleeping");
        store.system_did_wake();
        assert!(matches!(
            store.projection().v070_hardware.host_sleep_status.as_str(),
            "waking" | "awake"
        ));
        poll_until(&mut store, |s| {
            s.projection().v070_hardware.host_sleep_status == "awake"
        });

        poll_until(&mut store, |s| s.projection().v070_hardware.synced);
        assert!(
            sent_methods
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.as_str() == "device.status")
                .count()
                >= 2
        );

        if let Some(session) = store.device_session.take() {
            store.tokio_runtime.block_on(session.shutdown());
        }
        responder.abort();
    }

    #[test]
    fn select_layer_validation_rejects_out_of_range_before_sending_wire_command() {
        let base_mouse = MouseConfig::default();
        for layer in [0, 7] {
            let err = parse_v070_action(V070HardwareAction::SelectLayer { layer }, &base_mouse, 15)
                .unwrap_err();
            assert_eq!(
                err,
                format!("板载层索引 {layer} 超出有效范围（仅支持第 1–6 层）")
            );
        }
        for layer in 1..=6 {
            assert!(
                parse_v070_action(V070HardwareAction::SelectLayer { layer }, &base_mouse, 15)
                    .is_ok()
            );
        }

        let dir = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(dir.path()).unwrap();
        let sent_methods = Arc::new(Mutex::new(Vec::new()));
        let (session, _link, responder) =
            spawn_mock_v070_session(&store.tokio_runtime, "0.7.0-rc.16", sent_methods.clone());
        store.device_session = Some(session);

        let before_wire_count = sent_methods.lock().unwrap().len();
        for layer in [0, 7] {
            assert!(store
                .dispatch_command(WorkbenchCommand::V070Hardware(
                    V070HardwareAction::SelectLayer { layer },
                ))
                .is_err());
            assert!(!store.projection().v070_hardware.busy);
            assert_eq!(sent_methods.lock().unwrap().len(), before_wire_count);
        }

        if let Some(session) = store.device_session.take() {
            store.tokio_runtime.block_on(session.shutdown());
        }
        responder.abort();
    }

    #[test]
    fn apply_outcome_preserves_in_flight_command_against_stale_snapshot() {
        let make_snapshot = |layer: u8, name: &str| {
            V070BridgeOutcome::Snapshot(Box::new(V070SnapshotPayload {
                session_id: 1,
                version: "0.7.0-rc.16".into(),
                active_profile: Some(3),
                active_layer: Some(layer),
                active_joystick_mode: None,
                battery_percent: Some(80),
                is_charging: Some(false),
                mouse: MouseConfig::default(),
                deadzone: 0.15,
                pm_config: PmConfig::default(),
                pm_state: mi_proto::v070_capabilities::PmState {
                    state: "active".into(),
                    floor: "usb".into(),
                    base_floor: "idle".into(),
                    line_powered: true,
                    auto_sleep: true,
                    sleep_after_s: 900,
                    idle_s: 1,
                    standby_s: 0,
                    credit_s: 0,
                    holds: Vec::new(),
                },
                device_name: mi_proto::v070_capabilities::DeviceNameInfo {
                    name: name.into(),
                    factory_name: "Creator Micro 2".into(),
                    max_length: 15,
                },
            }))
        };

        let mut controller = V070HardwareController::default();
        let seq = controller.allocate_seq();
        assert_eq!(seq, 1);
        controller.projection.busy = true;
        controller.in_flight_seq = Some(seq);

        controller.apply_outcome(0, Ok(make_snapshot(1, "Initial")));
        assert!(controller.projection.busy);
        assert_eq!(controller.in_flight_seq, Some(1));
        assert!(controller.projection.synced);
        assert_eq!(controller.projection.active_layer, Some(1));
        assert_eq!(controller.projection.device_name.name, "Initial");

        controller.projection.active_layer = Some(2);
        controller.projection.device_name.name = "Current".into();
        controller.apply_outcome(0, Ok(make_snapshot(4, "Stale")));
        assert!(controller.projection.busy);
        assert_eq!(controller.in_flight_seq, Some(1));
        assert_eq!(controller.projection.active_layer, Some(2));
        assert_eq!(controller.projection.device_name.name, "Current");

        controller.apply_outcome(
            1,
            Ok(V070BridgeOutcome::ProfileLayer {
                profile_index: Some(3),
                layer_index: 5,
            }),
        );
        assert!(!controller.projection.busy);
        assert_eq!(controller.in_flight_seq, None);
        assert_eq!(controller.projection.active_layer, Some(5));
    }

    #[test]
    fn six_layer_vendor_bridge_forwards_kb_cs_notifications_on_v063_and_v070_with_dedup() {
        for version in ["0.6.3", "0.7.0-rc.16"] {
            let dir = tempfile::tempdir().unwrap();
            let mut store = WorkbenchStore::open_isolated(dir.path()).unwrap();
            let sent_methods = Arc::new(Mutex::new(Vec::new()));
            let (session, link, responder) =
                spawn_mock_v070_session(&store.tokio_runtime, version, sent_methods);

            let mut running = RunningSixLayer::isolated(session.clone(), dir.path());
            if version == "0.6.3" {
                running.installation.plan.codex_joystick_mode = None;
                for host in &mut running.installation.plan.hosts {
                    host.joystick_mode = None;
                }
            }
            running.set_v070_inbox(store.hardware_event_tx.clone());
            running.attach_test_vendor_session(&store.tokio_runtime, session.clone());
            store.six_layer_running = Some(running);

            store.overlay_controller.request_profile_switch_notice(
                "Antigravity".to_string(),
                "手动选择方案".to_string(),
                &crate::CheatSheetProjection::default(),
                &store.screen_geometry,
                store.last_cursor_position,
                Instant::now(),
            );
            assert!(matches!(
                store.overlay_controller.visibility(),
                &crate::HudVisibilityState::PassiveToast { .. }
            ));

            link.push_wire(json!({ "method": "kb.cs.hide", "params": {} }));
            std::thread::sleep(Duration::from_millis(60));
            store.poll_at(Instant::now());
            assert!(
                matches!(
                    store.overlay_controller.visibility(),
                    &crate::HudVisibilityState::PassiveToast { .. }
                ),
                "kb.cs.hide must never clobber an active PassiveToast on firmware {version}"
            );

            let is_cs_visible = |s: &WorkbenchStore| {
                matches!(
                    s.projection().overlay.content,
                    crate::ActiveHudContent::InteractiveCheatSheet(_)
                )
            };

            link.push_wire(json!({ "method": "kb.cs.show", "params": {} }));
            poll_until(&mut store, is_cs_visible);

            link.push_wire(json!({ "method": "kb.cs.hide", "params": {} }));
            poll_until(&mut store, |s| !is_cs_visible(s));

            link.push_wire(json!({ "method": "kb.cs.toggle", "params": {} }));
            poll_until(&mut store, is_cs_visible);

            link.push_wire(json!({ "method": "kb.cs.toggle", "params": {} }));
            poll_until(&mut store, |s| !is_cs_visible(s));

            let dup_event = crate::store::HardwareLoopEvent::CheatSheetNotification {
                session_id: session.id,
                ingress_sequence: 9001,
                kind: crate::CheatSheetNotificationKind::Show,
            };
            store.hardware_event_tx.send(dup_event).unwrap();
            poll_until(&mut store, is_cs_visible);

            let duplicate_toggle = crate::store::HardwareLoopEvent::CheatSheetNotification {
                session_id: session.id,
                ingress_sequence: 9001,
                kind: crate::CheatSheetNotificationKind::Toggle,
            };
            store.hardware_event_tx.send(duplicate_toggle).unwrap();
            store.poll_at(Instant::now());
            assert!(
                is_cs_visible(&store),
                "duplicate (session_id, ingress_sequence) must be ignored on firmware {version}"
            );

            if version == "0.6.3" {
                let edit = store
                    .projection()
                    .inspector
                    .editable()
                    .expect("editable inspector");
                assert!(!edit.v070_hardware_mouse_supported);
                store
                    .dispatch_inspector_action(crate::InspectorAction::SetJoystickMode(
                        crate::JoystickControlMode::Mouse,
                    ))
                    .unwrap();
                let updated = store
                    .projection()
                    .inspector
                    .editable()
                    .expect("editable inspector after SetJoystickMode");
                assert!(!updated.v070_hardware_mouse_supported);
                assert!(updated.joystick_mode_summary.contains("0.7.0-rc.16+"));
            }

            if let Some(running) = store.six_layer_running.take() {
                running.shutdown(&store.tokio_runtime).unwrap();
            }
            responder.abort();
        }
    }

    #[test]
    fn v070_power_status_badge_formats_and_gates_by_support_and_connection() {
        let mut proj = V070HardwareProjection::default();
        assert!(crate::PowerStatusBadgeProjection::from_v070(&proj, false).is_none());
        assert!(crate::PowerStatusBadgeProjection::from_v070(&proj, true).is_none());

        proj.supported = true;
        let unsynced = crate::PowerStatusBadgeProjection::from_v070(&proj, true)
            .expect("supported connected device produces badge");
        assert_eq!(unsynced.summary_label, "电量读取中");
        assert_eq!(unsynced.usb_mode, "");

        proj.synced = true;
        proj.battery_percent = Some(88);
        proj.is_charging = Some(true);
        proj.pm_state.usb_host_hold = true;
        proj.pm_state.state = "idle".into();
        let charging_hold = crate::PowerStatusBadgeProjection::from_v070(&proj, true).unwrap();
        assert_eq!(charging_hold.usb_mode, "usb.host 保持");
        assert_eq!(
            charging_hold.summary_label,
            "88% 充电中 · usb.host 保持 · 待机"
        );

        proj.battery_percent = None;
        proj.is_charging = None;
        proj.pm_state.line_powered = true;
        proj.pm_state.usb_host_hold = false;
        proj.pm_config.host_sleep = true;
        proj.pm_state.state = "active".into();
        let line_follow = crate::PowerStatusBadgeProjection::from_v070(&proj, true).unwrap();
        assert_eq!(line_follow.usb_mode, "跟随宿主");
        assert_eq!(line_follow.summary_label, "USB 供电 · 跟随宿主");

        proj.battery_percent = Some(50);
        proj.is_charging = Some(false);
        proj.pm_config.host_sleep = false;
        proj.pm_config.auto_sleep = false;
        proj.pm_state.state = "sleep".into();
        let awake_sleep = crate::PowerStatusBadgeProjection::from_v070(&proj, true).unwrap();
        assert_eq!(awake_sleep.summary_label, "50% · 常醒 · 休眠");
    }

    #[test]
    fn v070_auto_layer_sync_default_off_oai_exclusion_echo_suppression_and_busy_coalescing() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = crate::store::WorkbenchStoreConfig::isolated(dir.path());
        cfg.isolated = false;
        let mut store = WorkbenchStore::open(cfg.clone()).unwrap();
        assert!(!store.projection().v070_hardware.auto_layer_sync_enabled);
        assert!(!store.document_store.v070_auto_layer_sync_enabled());

        store
            .dispatch_command(WorkbenchCommand::V070Hardware(
                V070HardwareAction::SetAutoLayerSync { enabled: true },
            ))
            .unwrap();
        assert!(store.projection().v070_hardware.auto_layer_sync_enabled);
        assert!(store.document_store.v070_auto_layer_sync_enabled());
        drop(store);

        let mut reopened = WorkbenchStore::open(cfg).unwrap();
        assert!(reopened.projection().v070_hardware.auto_layer_sync_enabled);
        reopened
            .dispatch_command(WorkbenchCommand::V070Hardware(
                V070HardwareAction::SetAutoLayerSync { enabled: false },
            ))
            .unwrap();
        assert!(!reopened.projection().v070_hardware.auto_layer_sync_enabled);

        let sent_methods = Arc::new(Mutex::new(Vec::new()));
        let (session, link, mut responder) =
            spawn_mock_v070_session(&reopened.tokio_runtime, "0.7.0-rc.16", sent_methods.clone());
        let profile_id = |name: &str| {
            reopened
                .document_store
                .draft
                .document
                .editable
                .graph
                .profiles
                .iter()
                .find(|(_, p)| p.name == name)
                .unwrap()
                .0
                .clone()
        };
        let codex_id = profile_id("Codex");
        let ag_id = profile_id("Antigravity");
        let dsh_id = profile_id("DeepSeek Coder (DSH)");
        let desired = mi_model::board_layers::BoardLayerPlan::recommended(
            codex_id.clone(),
            ag_id.clone(),
            dsh_id.clone(),
        );
        let compiled = mi_model::apply::AppliedSixLayerPlan::compile(
            &desired,
            &reopened.document_store.draft.document,
            mi_model::host_router::BottomRowLayout::Split,
            true,
        )
        .unwrap();

        let mut running = RunningSixLayer::isolated(session.clone(), dir.path());
        running.installation.plan = compiled;
        running.readiness.configuration_observed = true;
        running.readiness.keyboard_grabbed = true;
        running.set_v070_inbox(reopened.hardware_event_tx.clone());
        running.attach_test_vendor_session(&reopened.tokio_runtime, session.clone());
        reopened.six_layer_running = Some(running);

        poll_until(&mut reopened, |s| s.projection().v070_hardware.synced);
        let badge = reopened
            .projection()
            .header
            .power_status
            .clone()
            .expect("0.7.0 power status badge must be shown in header");
        assert_eq!(badge.battery_percent, Some(91));
        assert_eq!(reopened.projection().status_menu.power_status, Some(badge));

        reopened.set_host_execution_enabled(true).unwrap();
        reopened.select_and_activate_profile(&dsh_id).unwrap();
        let _ = reopened.foreground_changed(Some("com.deepseek.dsh"));
        assert!(
            !sent_methods
                .lock()
                .unwrap()
                .iter()
                .any(|m| m == "kb.pl.set"),
            "auto_layer_sync disabled by default must never emit kb.pl.set"
        );
        reopened
            .dispatch_command(WorkbenchCommand::Sidebar(
                crate::sidebar::SidebarAction::SetProfileLocked(true),
            ))
            .unwrap();
        reopened
            .dispatch_command(WorkbenchCommand::Sidebar(
                crate::sidebar::SidebarAction::SetProfileLocked(false),
            ))
            .unwrap();
        assert!(matches!(
            reopened.build_six_layer_routing_mode(),
            crate::six_layer_runtime::SixLayerRoutingMode::BoardLayers { .. }
        ));

        reopened
            .dispatch_command(WorkbenchCommand::V070Hardware(
                V070HardwareAction::SetAutoLayerSync { enabled: true },
            ))
            .unwrap();
        assert!(!reopened.projection().v070_hardware.busy);
        let _ = reopened.foreground_changed(Some("com.deepseek.dsh"));
        assert!(
            !sent_methods
                .lock()
                .unwrap()
                .iter()
                .any(|m| m == "kb.pl.set"),
            "foreground_changed must never emit kb.pl.set even when auto_layer_sync is enabled"
        );

        reopened.select_and_activate_profile(&dsh_id).unwrap();
        assert!(matches!(
            reopened.build_six_layer_routing_mode(),
            crate::six_layer_runtime::SixLayerRoutingMode::ManualPinned(_)
        ));
        poll_until(&mut reopened, |s| {
            !s.projection().v070_hardware.busy
                && s.projection().v070_hardware.active_layer == Some(3)
        });
        assert!(
            reopened.v070_hardware.expected_layer_echo.is_some(),
            "kb.pl.set completion should arm expected_layer_echo"
        );

        link.push_wire(json!({"m": "kb.pl", "p": {"p": 3, "l": 3}}));
        poll_until(&mut reopened, |s| {
            s.v070_hardware.expected_layer_echo.is_none()
        });
        assert!(matches!(
            reopened.build_six_layer_routing_mode(),
            crate::six_layer_runtime::SixLayerRoutingMode::ManualPinned(_)
        ));
        assert!(reopened
            .projection()
            .v070_hardware
            .status_message
            .contains("方案切层联动已切至 P3 · 第 3 层"));

        let kb_pl_set_before_codex = sent_methods
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m.as_str() == "kb.pl.set")
            .count();
        reopened.select_and_activate_profile(&codex_id).unwrap();
        assert!(!reopened.projection().v070_hardware.busy);
        assert_eq!(
            sent_methods
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.as_str() == "kb.pl.set")
                .count(),
            kb_pl_set_before_codex,
            "OAI-protected profile (Codex on L1) must never emit kb.pl.set"
        );

        reopened
            .dispatch_command(WorkbenchCommand::V070Hardware(
                V070HardwareAction::ApplyMousePreset {
                    preset: "fast".into(),
                },
            ))
            .unwrap();
        assert!(reopened.projection().v070_hardware.busy);
        reopened.select_and_activate_profile(&ag_id).unwrap();
        assert_eq!(
            reopened.v070_hardware.pending_auto_layer_sync,
            Some(2),
            "layer sync while busy must coalesce to pending_auto_layer_sync"
        );
        poll_until(&mut reopened, |s| {
            !s.projection().v070_hardware.busy
                && s.projection().v070_hardware.active_layer == Some(2)
        });
        link.push_wire(json!({"m": "kb.pl", "p": {"p": 3, "l": 2}}));
        poll_until(&mut reopened, |s| {
            s.v070_hardware.expected_layer_echo.is_none()
        });

        reopened.set_host_execution_enabled(false).unwrap();
        let kb_pl_set_while_disabled = sent_methods
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m.as_str() == "kb.pl.set")
            .count();
        reopened.select_and_activate_profile(&dsh_id).unwrap();
        assert!(!reopened.projection().v070_hardware.busy);
        assert_eq!(
            sent_methods
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.as_str() == "kb.pl.set")
                .count(),
            kb_pl_set_while_disabled,
            "host_execution_enabled == false must block kb.pl.set on profile activation"
        );

        reopened.set_host_execution_enabled(true).unwrap();
        poll_until(&mut reopened, |s| {
            !s.projection().v070_hardware.busy
                && s.projection().v070_hardware.active_layer == Some(3)
        });
        assert_eq!(
            sent_methods
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.as_str() == "kb.pl.set")
                .count(),
            kb_pl_set_while_disabled + 1,
            "re-enabling host_execution_enabled must immediately sync to active profile layer"
        );
        link.push_wire(json!({"m": "kb.pl", "p": {"p": 3, "l": 3}}));
        poll_until(&mut reopened, |s| {
            s.v070_hardware.expected_layer_echo.is_none()
        });

        let kb_pl_set_before_physical_unpin = sent_methods
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m.as_str() == "kb.pl.set")
            .count();
        link.push_wire(json!({"m": "kb.pl", "p": {"p": 3, "l": 1}}));
        poll_until(&mut reopened, |s| {
            s.projection().v070_hardware.active_layer == Some(1)
                && matches!(
                    s.build_six_layer_routing_mode(),
                    crate::six_layer_runtime::SixLayerRoutingMode::BoardLayers { .. }
                )
        });
        assert!(!reopened.projection().v070_hardware.busy);
        assert_eq!(
            sent_methods
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.as_str() == "kb.pl.set")
                .count(),
            kb_pl_set_before_physical_unpin,
            "unexpected physical layer change must unpin to BoardLayers without emitting counter-kb.pl.set"
        );

        reopened.browse_profile(&ag_id).unwrap();
        reopened
            .dispatch_command(WorkbenchCommand::Sidebar(
                crate::sidebar::SidebarAction::SetProfileLocked(true),
            ))
            .unwrap();
        assert!(matches!(
            reopened.build_six_layer_routing_mode(),
            crate::six_layer_runtime::SixLayerRoutingMode::Locked(_)
        ));
        poll_until(&mut reopened, |s| {
            !s.projection().v070_hardware.busy
                && s.projection().v070_hardware.active_layer == Some(2)
        });
        link.push_wire(json!({"m": "kb.pl", "p": {"p": 3, "l": 2}}));
        poll_until(&mut reopened, |s| {
            s.v070_hardware.expected_layer_echo.is_none()
        });

        let kb_pl_set_before_unlock = sent_methods
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m.as_str() == "kb.pl.set")
            .count();
        reopened
            .dispatch_command(WorkbenchCommand::Sidebar(
                crate::sidebar::SidebarAction::SetProfileLocked(false),
            ))
            .unwrap();
        assert!(matches!(
            reopened.build_six_layer_routing_mode(),
            crate::six_layer_runtime::SixLayerRoutingMode::BoardLayers { .. }
        ));
        assert!(!reopened.projection().v070_hardware.busy);
        assert_eq!(
            sent_methods
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.as_str() == "kb.pl.set")
                .count(),
            kb_pl_set_before_unlock,
            "SetProfileLocked(false) must restore BoardLayers without emitting kb.pl.set"
        );

        link.push_wire(json!({"m": "kb.pl", "p": {"p": 3, "l": 1}}));
        poll_until(&mut reopened, |s| {
            s.projection().v070_hardware.active_layer == Some(1)
        });
        responder.abort();
        let _ = reopened.tokio_runtime.block_on(&mut responder);
        reopened.select_and_activate_profile(&ag_id).unwrap();
        assert!(reopened.projection().v070_hardware.busy);
        assert!(reopened.v070_hardware.expected_layer_echo.is_some());
        reopened.select_and_activate_profile(&dsh_id).unwrap();
        assert_eq!(reopened.v070_hardware.pending_auto_layer_sync, Some(3));
        let in_flight_seq = reopened
            .v070_hardware
            .in_flight_seq
            .expect("in-flight SelectLayer seq");
        reopened
            .hardware_event_tx
            .send(HardwareLoopEvent::V070CapabilityOutcome {
                session_id: session.id,
                seq: in_flight_seq,
                result: Err("timeout".into()),
            })
            .unwrap();
        poll_until(&mut reopened, |s| !s.projection().v070_hardware.busy);
        assert!(
            reopened.v070_hardware.expected_layer_echo.is_none(),
            "failed V070CapabilityOutcome must clear expected_layer_echo"
        );
        assert!(reopened.v070_hardware.pending_auto_layer_sync.is_none());

        assert!(
            !sent_methods
                .lock()
                .unwrap()
                .iter()
                .any(|m| m == "fs.writebin"),
            "auto layer sync must stay completely flashless"
        );

        if let Some(running) = reopened.six_layer_running.take() {
            running.shutdown(&reopened.tokio_runtime).unwrap();
        }
    }

    #[test]
    fn command_domain_target_predicate_and_disconnect_cleanup_stay_unified() {
        let cmd: WorkbenchCommand = serde_json::from_value(json!({
            "domain": "v070_hardware",
            "action": { "SetAutoLayerSync": { "enabled": true } }
        }))
        .unwrap();
        assert!(matches!(
            cmd,
            WorkbenchCommand::V070Hardware(V070HardwareAction::SetAutoLayerSync { enabled: true })
        ));

        let dir = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(dir.path()).unwrap();
        let sent_methods = Arc::new(Mutex::new(Vec::new()));
        let (session, link, responder) =
            spawn_mock_v070_session(&store.tokio_runtime, "0.7.0-rc.16", sent_methods);
        let mut running = RunningSixLayer::isolated(session.clone(), dir.path());
        running.attach_test_vendor_session(&store.tokio_runtime, session.clone());
        store.six_layer_running = Some(running);

        assert!(store.has_active_v070_device());
        assert_eq!(store.live_v070_session().map(|s| s.id), Some(session.id));
        assert!(store.resolve_v070_command_target().is_ok());

        store.firmware_update.projection.busy = true;
        assert!(store
            .dispatch_command(WorkbenchCommand::V070Hardware(V070HardwareAction::Refresh,))
            .is_err());
        assert!(store
            .projection()
            .v070_hardware
            .last_error
            .as_deref()
            .is_some_and(|e| e.contains("固件操作正在进行")));
        store.firmware_update.projection.busy = false;

        let seq = store.v070_hardware.allocate_seq();
        store.v070_hardware.projection.busy = true;
        store.v070_hardware.in_flight_seq = Some(seq);
        store.commit_runtime_update();
        assert!(store.projection().v070_hardware.busy);
        store.six_layer_running.as_ref().unwrap().retire();

        assert!(!store.has_active_v070_device());
        assert!(store.live_v070_session().is_none());
        assert!(store.resolve_v070_command_target().is_err());

        store.handle_v070_capability_outcome(session.id, seq, Err("USB 会话已断开".into()));
        assert!(!store.projection().v070_hardware.busy);
        assert_eq!(store.v070_hardware.in_flight_seq, None);

        drop(link);
        if let Some(running) = store.six_layer_running.take() {
            running.shutdown(&store.tokio_runtime).unwrap();
        }
        responder.abort();
    }

    #[test]
    fn v070_minimal_mock_degrades_active_joystick_mode_to_none_and_v063_rejects_apply() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(dir.path()).unwrap();
        let sent_methods = Arc::new(Mutex::new(Vec::new()));
        let (session, _link, responder) =
            spawn_mock_v070_session(&store.tokio_runtime, "0.7.0-rc.16", sent_methods.clone());
        let mut running = RunningSixLayer::isolated(session.clone(), dir.path());
        running.set_v070_inbox(store.hardware_event_tx.clone());
        running.attach_test_vendor_session(&store.tokio_runtime, session);
        store.six_layer_running = Some(running);

        poll_until(&mut store, |s| s.projection().v070_hardware.synced);
        assert_eq!(store.projection().v070_hardware.active_joystick_mode, None);

        assert!(store
            .dispatch_command(WorkbenchCommand::V070Hardware(
                V070HardwareAction::ApplyJoystickHardwareMode {
                    mode: "bogus".into(),
                }
            ))
            .is_err());
        assert!(!store.projection().v070_hardware.busy);

        if let Some(running) = store.six_layer_running.take() {
            running.shutdown(&store.tokio_runtime).unwrap();
        }
        responder.abort();

        let dir_v063 = tempfile::tempdir().unwrap();
        let mut store_v063 = WorkbenchStore::open_isolated(dir_v063.path()).unwrap();
        let sent_v063 = Arc::new(Mutex::new(Vec::new()));
        let (session_v063, _link_v063, responder_v063) =
            spawn_mock_v070_session(&store_v063.tokio_runtime, "0.6.3", sent_v063.clone());
        let mut running_v063 = RunningSixLayer::isolated(session_v063.clone(), dir_v063.path());
        running_v063.set_v070_inbox(store_v063.hardware_event_tx.clone());
        running_v063.attach_test_vendor_session(&store_v063.tokio_runtime, session_v063);
        store_v063.six_layer_running = Some(running_v063);

        let err = store_v063
            .dispatch_command(WorkbenchCommand::V070Hardware(
                V070HardwareAction::ApplyJoystickHardwareMode {
                    mode: "mouse".into(),
                },
            ))
            .unwrap_err();
        assert!(err.to_string().contains("0.7.0-rc.16"));
        assert!(!dir_v063.path().join("v070-keymap-backups").exists());
        assert!(!sent_v063.lock().unwrap().iter().any(|m| m == "fs.writebin"));

        if let Some(running) = store_v063.six_layer_running.take() {
            running.shutdown(&store_v063.tokio_runtime).unwrap();
        }
        responder_v063.abort();
    }

    #[test]
    fn v070_joystick_hardware_mode_snapshot_apply_backup_six_layer_sync_and_reload() {
        use crate::six_layer_runtime::{test_b64_decode, test_b64_encode};
        use std::os::unix::fs::PermissionsExt;

        const SIX_SOURCE: &[u8] =
            include_bytes!("../../../mi-model/fixtures/device/synthetic-six-layer-source.json");
        const SMART: &[u8] =
            include_bytes!("../../../mi-model/fixtures/device/synthetic-smart-actions-object.json");

        let compiled_six =
            mi_model::apply::compile_six_layer_profile(SIX_SOURCE, Some(SMART)).unwrap();
        assert_eq!(compiled_six.profile_index, 1);
        let initial_planned_bytes = compiled_six.keymap.clone();

        let dir = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(dir.path()).unwrap();

        let six_backup_dir = dir.path().join("six-layer-backup");
        std::fs::create_dir_all(&six_backup_dir).unwrap();
        std::fs::write(six_backup_dir.join("keymap.json"), SIX_SOURCE).unwrap();
        std::fs::write(
            six_backup_dir.join("planned-keymap.json"),
            &initial_planned_bytes,
        )
        .unwrap();
        std::fs::write(six_backup_dir.join("smart_actions.json"), SMART).unwrap();
        std::fs::write(
            six_backup_dir.join("manifest.json"),
            serde_json::to_vec_pretty(&json!({
                "format": "micro-input-three-layer-recovery-v1",
                "previous_profile_index": 0,
                "previous_layer_index": 1,
                "planned_profile_index": 1,
                "keymap_sha256": crate::lighting_arbiter::sha256_hex(SIX_SOURCE),
                "planned_keymap_sha256": crate::lighting_arbiter::sha256_hex(&initial_planned_bytes),
            }))
            .unwrap(),
        )
        .unwrap();

        let sent_methods = Arc::new(Mutex::new(Vec::<String>::new()));
        let kb_pl_set_calls = Arc::new(Mutex::new(Vec::<Value>::new()));
        let board_keymap = Arc::new(Mutex::new(initial_planned_bytes.clone()));

        let (link, mut out_rx) = MockV070Link::new();
        let session = store.tokio_runtime.block_on(async {
            DeviceSession::new(
                link.clone(),
                Box::new(TestFramer),
                Arc::new(ReadOnlySchema),
                SessionConfig {
                    timeout: Duration::from_millis(800),
                    interval: Duration::ZERO,
                },
            )
        });
        let sent_clone = sent_methods.clone();
        let pl_clone = kb_pl_set_calls.clone();
        let board_keymap_clone = board_keymap.clone();
        let link_responder = link.clone();
        let responder = store.tokio_runtime.spawn(async move {
            let mouse = MouseConfig::default();
            let deadzone = 0.15_f64;
            let pm_cfg = PmConfig::default();
            let mut active_profile = 1_u8;
            let mut active_layer = 2_u8;
            let mut staged = Vec::<u8>::new();
            while let Some(raw) = out_rx.recv().await {
                let req: Value = serde_json::from_slice(&raw).unwrap();
                let id = req["id"].clone();
                let method = req["method"].as_str().unwrap().to_string();
                let params = &req["params"];
                sent_clone.lock().unwrap().push(method.clone());
                let meta = |name: &str, data: &[u8]| {
                    let sha1 = mi_model::operation_record::file_value(data)["sha1"]
                        .as_str()
                        .unwrap()
                        .to_owned();
                    json!({"name": name, "size": data.len(), "checksum": sha1})
                };
                let result = match method.as_str() {
                    "device.status" => json!({
                        "version": "0.7.0-rc.16",
                        "profile_index": active_profile,
                        "layer_index": active_layer,
                        "battery": 88,
                        "is_charging": false
                    }),
                    "device.name" => json!({
                        "name": "Creator Micro 2",
                        "factory_name": "Creator Micro 2",
                        "max_length": 15
                    }),
                    "pm.config" => serde_json::to_value(pm_cfg).unwrap(),
                    "pm.state" => json!({
                        "state": "active",
                        "floor": "usb",
                        "base_floor": "host_sleep",
                        "line_powered": true,
                        "auto_sleep": true,
                        "sleep_after_s": 900,
                        "idle_s": 2,
                        "standby_s": 100,
                        "credit_s": 0,
                        "holds": [{"owner": "usb.host", "floor": "usb", "age_ms": 100}]
                    }),
                    "mouse.config" => serde_json::to_value(mouse).unwrap(),
                    "joystick.deadzone" => json!({"v": deadzone}),
                    "fs.list" => {
                        let km = board_keymap_clone.lock().unwrap();
                        json!([meta("keymap.json", &km), meta("smart_actions.json", SMART)])
                    }
                    "fs.readbin" => {
                        let km = board_keymap_clone.lock().unwrap().clone();
                        let file_bytes: &[u8] = match params["file"].as_str().unwrap() {
                            "keymap.json" => km.as_slice(),
                            "smart_actions.json" => SMART,
                            other => panic!("unexpected file {other}"),
                        };
                        let offset = params["offset"].as_u64().unwrap() as usize;
                        let len = params["len"].as_u64().unwrap() as usize;
                        let end = (offset + len).min(file_bytes.len());
                        json!({
                            "total_size": file_bytes.len(),
                            "data": test_b64_encode(&file_bytes[offset..end])
                        })
                    }
                    "fs.txbegin" => {
                        staged.clear();
                        json!({"tx": 42})
                    }
                    "fs.writebin" => {
                        assert_eq!(params["file"].as_str(), Some("keymap.json"));
                        assert_eq!(params["tx"].as_u64(), Some(42));
                        staged.extend(test_b64_decode(params["data"].as_str().unwrap()));
                        Value::Null
                    }
                    "fs.txcommit" => {
                        assert_eq!(params["tx"].as_u64(), Some(42));
                        *board_keymap_clone.lock().unwrap() = std::mem::take(&mut staged);
                        Value::Null
                    }
                    "kb.pl.set" => {
                        pl_clone.lock().unwrap().push(params.clone());
                        if let Some(p) = params.get("p").and_then(Value::as_u64) {
                            active_profile = p as u8;
                        }
                        if let Some(l) = params.get("l").and_then(Value::as_u64) {
                            active_layer = l as u8;
                        }
                        json!({})
                    }
                    "kb.sa.rgb" | "v.oai.thstatus" => json!({}),
                    other => panic!("unexpected method {other}"),
                };
                link_responder.push_wire(json!({
                    "id": id,
                    "method": method,
                    "result": result
                }));
            }
        });
        store.tokio_runtime.block_on(async {
            session.read_identity_info().await.unwrap();
        });

        let profile_id = |store: &WorkbenchStore, name: &str| {
            store
                .document_store
                .draft
                .document
                .editable
                .graph
                .profiles
                .iter()
                .find(|(_, p)| p.name == name)
                .unwrap()
                .0
                .clone()
        };
        let desired = mi_model::board_layers::BoardLayerPlan::recommended(
            profile_id(&store, "Codex"),
            profile_id(&store, "Antigravity"),
            profile_id(&store, "DeepSeek Coder (DSH)"),
        );
        let compiled_plan = mi_model::apply::AppliedSixLayerPlan::compile(
            &desired,
            &store.document_store.draft.document,
            mi_model::host_router::BottomRowLayout::Split,
            true,
        )
        .unwrap();

        let mut running = RunningSixLayer::isolated(session.clone(), dir.path());
        running.installation.profile_index = 1;
        running.installation.backup = six_backup_dir.clone();
        running.installation.plan = compiled_plan;
        running.readiness.configuration_observed = true;
        running.readiness.keyboard_grabbed = true;
        running.set_v070_inbox(store.hardware_event_tx.clone());
        running.attach_test_vendor_session(&store.tokio_runtime, session.clone());
        store.six_layer_running = Some(running);

        poll_until(&mut store, |s| s.projection().v070_hardware.synced);
        assert_eq!(
            store
                .projection()
                .v070_hardware
                .active_joystick_mode
                .as_deref(),
            Some("radial"),
            "initial snapshot on six-layer L2 must populate active_joystick_mode = radial"
        );

        store
            .dispatch_command(WorkbenchCommand::V070Hardware(
                V070HardwareAction::SelectLayer { layer: 1 },
            ))
            .unwrap();
        poll_until(&mut store, |s| {
            !s.projection().v070_hardware.busy
                && s.projection().v070_hardware.active_layer == Some(1)
        });
        assert_eq!(
            store
                .projection()
                .v070_hardware
                .active_joystick_mode
                .as_deref(),
            Some("vendor"),
            "switching to six-layer L1 must reflect cached vendor joystick mode"
        );

        store
            .dispatch_command(WorkbenchCommand::V070Hardware(
                V070HardwareAction::SelectLayer { layer: 2 },
            ))
            .unwrap();
        poll_until(&mut store, |s| {
            !s.projection().v070_hardware.busy
                && s.projection().v070_hardware.active_layer == Some(2)
        });
        assert_eq!(
            store
                .projection()
                .v070_hardware
                .active_joystick_mode
                .as_deref(),
            Some("radial")
        );
        let pl_calls_before_apply = kb_pl_set_calls.lock().unwrap().len();

        let cmd_json: WorkbenchCommand = serde_json::from_value(json!({
            "domain": "v070_hardware",
            "action": { "ApplyJoystickHardwareMode": { "mode": "mouse" } }
        }))
        .unwrap();
        store.dispatch_command(cmd_json).unwrap();
        poll_until(&mut store, |s| {
            !s.projection().v070_hardware.busy
                && s.projection().v070_hardware.active_joystick_mode.as_deref() == Some("mouse")
        });

        let backup_root = dir.path().join("v070-keymap-backups");
        let orig_backup = backup_root.join("original-keymap.json");
        assert!(orig_backup.is_file());
        assert_eq!(
            std::fs::metadata(&orig_backup)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(&backup_root)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(std::fs::read(&orig_backup).unwrap(), initial_planned_bytes);

        let pl_calls = kb_pl_set_calls.lock().unwrap().clone();
        assert_eq!(pl_calls.len(), pl_calls_before_apply + 1);
        assert_eq!(pl_calls.last(), Some(&json!({"p": 1, "l": 2})));

        let live_mouse_bytes = board_keymap.lock().unwrap().clone();
        assert_eq!(
            mi_model::apply::recognize_six_layer_profile_for_firmware(
                &live_mouse_bytes,
                Some(SMART),
                Some("0.7.0-rc.16"),
            ),
            Ok(Some(1))
        );
        for l in 1..=6 {
            assert_eq!(
                mi_model::apply::extract_keymap_joystick_hardware_mode(
                    &live_mouse_bytes,
                    Some(1),
                    Some(l),
                )
                .as_deref(),
                Some("mouse"),
                "six-layer profile must apply MOUSE across all 6 layers so layer-switching preserves hardware mouse"
            );
        }

        assert_eq!(
            std::fs::read(six_backup_dir.join("keymap.json")).unwrap(),
            SIX_SOURCE,
            "original pre-six-layer keymap.json backup must never be mutated"
        );
        assert_eq!(
            std::fs::read(six_backup_dir.join("previous-planned-keymap.json")).unwrap(),
            initial_planned_bytes
        );
        assert_eq!(
            std::fs::read(six_backup_dir.join("planned-keymap.json")).unwrap(),
            live_mouse_bytes
        );
        assert_eq!(
            store
                .six_layer_running
                .as_ref()
                .unwrap()
                .installation
                .plan
                .codex_joystick_mode
                .as_deref(),
            Some("MOUSE")
        );
        assert!(store
            .six_layer_running
            .as_ref()
            .unwrap()
            .installation
            .plan
            .hosts
            .iter()
            .all(|h| h.joystick_mode.as_deref() == Some("MOUSE")));

        store
            .dispatch_command(WorkbenchCommand::V070Hardware(
                V070HardwareAction::ApplyJoystickHardwareMode {
                    mode: "scroll".into(),
                },
            ))
            .unwrap();
        poll_until(&mut store, |s| {
            !s.projection().v070_hardware.busy
                && s.projection().v070_hardware.active_joystick_mode.as_deref() == Some("scroll")
        });
        assert_eq!(
            std::fs::read(&orig_backup).unwrap(),
            initial_planned_bytes,
            "original-keymap.json in v070-keymap-backups must stay immutable across subsequent writes"
        );
        let mouse_sha1 = mi_model::operation_record::file_value(&live_mouse_bytes)["sha1"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(
            std::fs::read(backup_root.join(format!("keymap-{mouse_sha1}.json"))).unwrap(),
            live_mouse_bytes
        );

        store
            .dispatch_command(WorkbenchCommand::V070Hardware(
                V070HardwareAction::ApplyJoystickHardwareMode {
                    mode: "logical_sectors".into(),
                },
            ))
            .unwrap();
        poll_until(&mut store, |s| {
            !s.projection().v070_hardware.busy
                && s.projection().v070_hardware.active_joystick_mode.as_deref() == Some("radial")
        });
        let restored_bytes = board_keymap.lock().unwrap().clone();
        assert_eq!(
            mi_model::apply::extract_keymap_joystick_hardware_mode(
                &restored_bytes,
                Some(1),
                Some(1)
            )
            .as_deref(),
            Some("vendor")
        );
        assert_eq!(
            mi_model::apply::extract_keymap_joystick_hardware_mode(
                &restored_bytes,
                Some(1),
                Some(2)
            )
            .as_deref(),
            Some("radial")
        );

        link.push_wire(json!({"m": "kb.pl", "p": {"p": 1, "l": 1}}));
        poll_until(&mut store, |s| {
            s.projection().v070_hardware.active_layer == Some(1)
                && s.projection().v070_hardware.active_joystick_mode.as_deref() == Some("vendor")
        });

        drop(link);
        if let Some(running) = store.six_layer_running.take() {
            running.shutdown(&store.tokio_runtime).unwrap();
        }
        responder.abort();
    }
}

#[cfg(test)]
mod sa_rgb_tests;
