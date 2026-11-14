//! WorkbenchStore orchestration and async session dispatch for `0.7.0-rc.16+` hardware capabilities.

use mi_proto::v070_capabilities::{
    validate_device_name, JoystickDeadzone, MouseConfig, MouseSpeedPreset, PmConfig, SaRgbEffect,
    SaRgbUpdate,
};
use mi_session::DeviceSession;

use crate::firmware_update::{
    format_hex_color, parse_hex_color, V070BridgeOutcome, V070DeviceNameProjection,
    V070HardwareAction, V070HardwareProjection, V070MouseConfigProjection, V070PmConfigProjection,
    V070PmStateProjection, V070SaRgbProjection, V070SnapshotPayload,
};
use crate::store::{HardwareLoopEvent, SystemLifecycle, WorkbenchStore};
use crate::v070_session_sa_rgb::{
    resolve_desired_for_device_layer, resolve_desired_for_document_layer, DesiredTarget,
    LayerLightingResolution, ManualSaRgbWrite, SaRgbLitStyle, UrgentClearReason,
};

#[derive(Debug, Clone)]
pub(crate) struct V070BridgeCommand {
    pub seq: u64,
    pub action: V070HardwareAction,
    pub base_mouse: MouseConfig,
    pub base_deadzone: f64,
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
                if snap.battery_percent.is_some() {
                    self.projection.battery_percent = snap.battery_percent;
                }
                if snap.is_charging.is_some() {
                    self.projection.is_charging = snap.is_charging;
                }
                self.projection.mouse =
                    V070MouseConfigProjection::from_mouse_config(&snap.mouse, snap.deadzone);
                self.projection.pm_config = V070PmConfigProjection::from_pm_config(&snap.pm_config);
                self.projection.pm_state = V070PmStateProjection::from_pm_state(&snap.pm_state);
                self.projection.device_name =
                    V070DeviceNameProjection::from_device_name_info(&snap.device_name);
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
            Ok(V070BridgeOutcome::MouseUpdated { mouse, deadzone }) => {
                self.projection.mouse =
                    V070MouseConfigProjection::from_mouse_config(&mouse, deadzone);
                self.projection.last_error = None;
                self.projection.status_message =
                    format!("已更新摇杆鼠标参数（{}）", self.projection.mouse.preset);
            }
            Ok(V070BridgeOutcome::PmConfigUpdated { pm_config }) => {
                self.projection.pm_config = V070PmConfigProjection::from_pm_config(&pm_config);
                self.projection.last_error = None;
                self.projection.status_message = "已更新电源管理设置".into();
            }
            Ok(V070BridgeOutcome::DeviceRenamed { device_name }) => {
                self.projection.device_name =
                    V070DeviceNameProjection::from_device_name_info(&device_name);
                self.projection.last_error = None;
                self.projection.status_message = format!("设备名称已更新为 {}", device_name.name);
            }
            Ok(V070BridgeOutcome::SaRgbApplied {
                sa,
                color_hex,
                brightness,
                speed,
                effect,
            }) => {
                self.projection.sa_rgb = V070SaRgbProjection {
                    sa: Some(sa),
                    color_hex,
                    brightness,
                    speed,
                    effect,
                };
                self.projection.last_error = None;
                self.projection.status_message = format!("已更新 SA #{sa} 独立灯效");
            }
            Ok(V070BridgeOutcome::SaRgbReset { sa }) => {
                self.projection.sa_rgb.sa = Some(sa);
                self.projection.sa_rgb.effect = SaRgbEffect::Off.as_u8();
                self.projection.last_error = None;
                self.projection.status_message = format!("已恢复 SA #{sa} 默认背景灯效");
            }
            Err(error) => {
                self.projection.last_error = Some(error.clone());
                self.projection.status_message = error;
            }
        }
    }
}

pub(crate) fn validate_v070_action(
    action: &V070HardwareAction,
    base_mouse: &MouseConfig,
    max_name_length: usize,
) -> Result<(), String> {
    match action {
        V070HardwareAction::Refresh
        | V070HardwareAction::SetAutoLayerSync { .. }
        | V070HardwareAction::SetSessionSaRgbSync { .. } => Ok(()),
        V070HardwareAction::SelectLayer { layer } => {
            if *layer == 0 || *layer > 6 {
                return Err(format!(
                    "板载层索引 {layer} 超出有效范围（仅支持第 1–6 层）"
                ));
            }
            Ok(())
        }
        V070HardwareAction::ApplyMousePreset { preset } => {
            MouseSpeedPreset::parse(preset).map_err(|e| e.to_string())?;
            Ok(())
        }
        V070HardwareAction::ApplyMouseConfig {
            move_delta,
            max_speed,
            wheel_delta,
            wheel_max_speed,
            stick_speed,
            stick_scroll,
            deadzone,
        } => {
            let mut cfg = *base_mouse;
            cfg.move_delta = *move_delta;
            cfg.max_speed = *max_speed;
            cfg.wheel_delta = *wheel_delta;
            cfg.wheel_max_speed = *wheel_max_speed;
            cfg.stick_speed = *stick_speed;
            cfg.stick_scroll = *stick_scroll;
            cfg.validate().map_err(|e| e.to_string())?;
            JoystickDeadzone::new(*deadzone).map_err(|e| e.to_string())?;
            Ok(())
        }
        V070HardwareAction::ApplyPmConfig {
            auto_sleep,
            host_sleep,
            sleep_after_min,
        } => PmConfig {
            auto_sleep: *auto_sleep,
            host_sleep: *host_sleep,
            sleep_after_min: *sleep_after_min,
        }
        .validate()
        .map_err(|e| e.to_string()),
        V070HardwareAction::RenameDevice { name } => {
            validate_device_name(name, max_name_length).map_err(|e| e.to_string())?;
            Ok(())
        }
        V070HardwareAction::ApplySaRgb {
            sa,
            color_hex,
            brightness,
            speed,
            effect,
        } => {
            let c = parse_hex_color(color_hex)?;
            if !brightness.is_finite() || !speed.is_finite() {
                return Err("亮度和速度必须为有限数值".into());
            }
            let update = SaRgbUpdate {
                sa: vec![*sa],
                c: Some(c),
                b: Some(*brightness as f32),
                s: Some(*speed as f32),
                e: Some(*effect),
            };
            update.validate().map_err(|e| e.to_string())
        }
        V070HardwareAction::ResetSaRgb { sa } => SaRgbUpdate::reset(vec![*sa])
            .validate()
            .map_err(|e| e.to_string()),
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
    Ok(V070SnapshotPayload {
        session_id: session.id,
        version,
        active_profile,
        active_layer,
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
    match &cmd.action {
        V070HardwareAction::Refresh => fetch_v070_snapshot_on_session(session, true)
            .await
            .map(V070BridgeOutcome::Snapshot),
        V070HardwareAction::SetAutoLayerSync { .. } => {
            Err("自动切层联动仅在宿主侧配置，不直接发送到固件".into())
        }
        V070HardwareAction::SetSessionSaRgbSync { .. } => {
            Err("会话状态自动映灯开关仅在宿主侧配置，不直接经桥接队列发送".into())
        }
        V070HardwareAction::SelectLayer { layer } => {
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
        V070HardwareAction::ApplyMousePreset { preset } => {
            let parsed = MouseSpeedPreset::parse(preset).map_err(|e| e.to_string())?;
            let saved = session
                .write_mouse_config(&parsed.to_mouse_config())
                .await
                .map_err(|e| e.to_string())?;
            Ok(V070BridgeOutcome::MouseUpdated {
                mouse: saved,
                deadzone: cmd.base_deadzone,
            })
        }
        V070HardwareAction::ApplyMouseConfig {
            move_delta,
            max_speed,
            wheel_delta,
            wheel_max_speed,
            stick_speed,
            stick_scroll,
            deadzone,
        } => {
            let mut cfg = cmd.base_mouse;
            cfg.move_delta = *move_delta;
            cfg.max_speed = *max_speed;
            cfg.wheel_delta = *wheel_delta;
            cfg.wheel_max_speed = *wheel_max_speed;
            cfg.stick_speed = *stick_speed;
            cfg.stick_scroll = *stick_scroll;
            let saved_mouse = session
                .write_mouse_config(&cfg)
                .await
                .map_err(|e| e.to_string())?;
            let saved_deadzone = session
                .write_joystick_deadzone(*deadzone)
                .await
                .map_err(|e| e.to_string())?;
            Ok(V070BridgeOutcome::MouseUpdated {
                mouse: saved_mouse,
                deadzone: saved_deadzone,
            })
        }
        V070HardwareAction::ApplyPmConfig {
            auto_sleep,
            host_sleep,
            sleep_after_min,
        } => {
            let cfg = PmConfig {
                auto_sleep: *auto_sleep,
                host_sleep: *host_sleep,
                sleep_after_min: *sleep_after_min,
            };
            let saved = session
                .write_pm_config(&cfg)
                .await
                .map_err(|e| e.to_string())?;
            Ok(V070BridgeOutcome::PmConfigUpdated { pm_config: saved })
        }
        V070HardwareAction::RenameDevice { name } => {
            let info = session
                .write_device_name(name)
                .await
                .map_err(|e| e.to_string())?;
            Ok(V070BridgeOutcome::DeviceRenamed { device_name: info })
        }
        V070HardwareAction::ApplySaRgb {
            sa,
            color_hex,
            brightness,
            speed,
            effect,
        } => {
            let c = parse_hex_color(color_hex)?;
            let update = SaRgbUpdate {
                sa: vec![*sa],
                c: Some(c),
                b: Some(*brightness as f32),
                s: Some(*speed as f32),
                e: Some(*effect),
            };
            session
                .set_sa_rgb(&update)
                .await
                .map_err(|e| e.to_string())?;
            Ok(V070BridgeOutcome::SaRgbApplied {
                sa: *sa,
                color_hex: format_hex_color(c),
                brightness: *brightness,
                speed: *speed,
                effect: *effect,
            })
        }
        V070HardwareAction::ResetSaRgb { sa } => {
            session
                .reset_sa_rgb(vec![*sa])
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

impl WorkbenchStore {
    pub fn live_v070_session(&self) -> Option<std::sync::Arc<DeviceSession>> {
        if let Some(running) = &self.six_layer_running {
            let version_ok = running
                .observed_firmware_version()
                .as_deref()
                .is_some_and(mi_model::supports_v070_hardware_capabilities);
            if !version_ok {
                return None;
            }
            return running.live_vendor_session();
        }
        self.device_session
            .as_ref()
            .filter(|session| {
                !session.is_disconnected()
                    && session.has_exclusive_vendor_link()
                    && session
                        .observed_firmware_version()
                        .as_deref()
                        .is_some_and(mi_model::supports_v070_hardware_capabilities)
            })
            .cloned()
    }

    pub(crate) fn teardown_v070_session_sa_rgb(&mut self) {
        let session = self.live_v070_session();
        self.sa_rgb_coordinator
            .teardown_blocking(&self.tokio_runtime, session);
    }

    pub(crate) fn sync_v070_session_sa_rgb(&mut self) {
        if self.system_lifecycle != SystemLifecycle::Awake || self.firmware_update.reserved() {
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
        let slot_kinds = self
            .slot_table
            .slot_session_kinds(&self.lighting_arbiter.acknowledged_watermarks);
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
            if matches!(
                running.installation.plan.desired.route(board_layer),
                mi_model::board_layers::BoardLayerRoute::Native { .. }
            ) {
                return DesiredTarget::UrgentClear(UrgentClearReason::ProtectedLayer);
            }
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
        if let Some(running) = &self.six_layer_running {
            return running.vendor_bridge_connected()
                && running
                    .observed_firmware_version()
                    .as_deref()
                    .is_some_and(mi_model::supports_v070_hardware_capabilities);
        }
        self.device_session.as_ref().is_some_and(|session| {
            !session.is_disconnected()
                && session.has_exclusive_vendor_link()
                && session
                    .observed_firmware_version()
                    .as_deref()
                    .is_some_and(mi_model::supports_v070_hardware_capabilities)
        })
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
        if !self.document_store.host_execution_enabled()
            || !self.auto_follow_enabled
            || self.manual_override
            || self.document_store.locked_profile_id().is_some()
        {
            return None;
        }
        let active_pid = self.effective_resolved_active_profile_id()?;
        if self.is_protected_oai_profile(&active_pid) {
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
                    if profile_id == &active_pid
            );
            if !plan_matches {
                continue;
            }
            if let Some(desired) = installed_plan {
                let desired_matches = matches!(
                    desired.route(layer),
                    mi_model::board_layers::BoardLayerRoute::HostProfile { profile_id }
                        if profile_id == &active_pid
                );
                if !desired_matches {
                    continue;
                }
            }
            return Some(layer.number());
        }
        None
    }

    pub(crate) fn maybe_sync_v070_foreground_layer(&mut self) -> bool {
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

    pub(super) fn dispatch_v070_hardware_action(
        &mut self,
        action: V070HardwareAction,
    ) -> Result<(), String> {
        if self.firmware_update.reserved() {
            return Err("固件更新流程正在占用设备，请结束后再试".into());
        }
        if let V070HardwareAction::SetAutoLayerSync { enabled } = action {
            self.document_store
                .set_v070_auto_layer_sync_enabled(enabled)?;
            self.v070_hardware.projection.auto_layer_sync_enabled = enabled;
            self.v070_hardware.projection.last_error = None;
            if enabled {
                self.v070_hardware.projection.status_message =
                    "已开启前台切层联动（仅 RAM 零擦写）".into();
                let _ = self.maybe_sync_v070_foreground_layer();
            } else {
                self.v070_hardware.pending_auto_layer_sync = None;
                self.v070_hardware.expected_layer_echo = None;
                self.v070_hardware.projection.status_message = "已关闭前台切层联动".into();
            }
            return Ok(());
        }
        if let V070HardwareAction::SetSessionSaRgbSync { enabled } = action {
            self.document_store
                .set_v070_session_sa_rgb_sync_enabled(enabled)?;
            self.v070_hardware.projection.session_sa_rgb_sync_enabled = enabled;
            self.v070_hardware.projection.last_error = None;
            if enabled {
                self.v070_hardware.projection.status_message = "已开启 AI 会话状态自动映灯".into();
                self.sync_v070_session_sa_rgb();
            } else {
                let session = self.live_v070_session();
                self.sa_rgb_coordinator.sync(
                    DesiredTarget::UrgentClear(UrgentClearReason::OptInDisabled),
                    session,
                    &self.tokio_runtime,
                );
                self.v070_hardware.projection.status_message = "已关闭 AI 会话状态自动映灯".into();
            }
            return Ok(());
        }
        if self.v070_hardware.projection.busy {
            return Err("上一条 0.7.0 硬件指令尚未完成".into());
        }
        let base_mouse = self.v070_hardware.projection.mouse.to_mouse_config();
        let base_deadzone = self.v070_hardware.projection.mouse.deadzone;
        let max_name_length = self.v070_hardware.projection.device_name.max_length;
        validate_v070_action(&action, &base_mouse, max_name_length)?;

        if let Some(running) = &self.six_layer_running {
            let Some(version) = running.observed_firmware_version() else {
                return Err("当前设备固件版本尚未确认，不支持 0.7.0 硬件能力".into());
            };
            if !mi_model::supports_v070_hardware_capabilities(&version) {
                return Err(format!(
                    "当前固件 {version} 低于 0.7.0-rc.16，不支持硬件能力接口"
                ));
            }
            if !running.vendor_bridge_connected() {
                return Err("USB 专用桥接当前未连接".into());
            }
            let seq = self.v070_hardware.allocate_seq();
            let cmd = V070BridgeCommand {
                seq,
                action,
                base_mouse,
                base_deadzone,
            };
            running.send_v070_command(cmd)?;
            self.v070_hardware.projection.busy = true;
            self.v070_hardware.in_flight_seq = Some(seq);
            self.v070_hardware.projection.last_error = None;
            return Ok(());
        }

        let Some(session) = self.device_session.as_ref().map(std::sync::Arc::clone) else {
            return Err("未连接支持 0.7.0 硬件能力的 USB 专用设备".into());
        };
        if session.is_disconnected() || !session.has_exclusive_vendor_link() {
            return Err("需要已连接的 USB 专用独占会话".into());
        }
        let Some(version) = session.observed_firmware_version() else {
            return Err("固件版本尚未确认，请等待连接验证完成".into());
        };
        if !mi_model::supports_v070_hardware_capabilities(&version) {
            return Err(format!(
                "当前固件 {version} 低于 0.7.0-rc.16，不支持硬件能力接口"
            ));
        }

        let seq = self.v070_hardware.allocate_seq();
        let cmd = V070BridgeCommand {
            seq,
            action,
            base_mouse,
            base_deadzone,
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

    fn is_active_v070_session(&self, session_id: u64) -> bool {
        let valid_six_layer = self
            .six_layer_running
            .as_ref()
            .is_some_and(|running| running.vendor_session_id() == Some(session_id));
        let valid_direct = self
            .device_session
            .as_ref()
            .is_some_and(|session| session.id == session_id);
        valid_six_layer || valid_direct
    }

    pub(super) fn handle_v070_capability_outcome(
        &mut self,
        session_id: u64,
        seq: u64,
        result: Result<V070BridgeOutcome, String>,
    ) {
        if !self.is_active_v070_session(session_id) {
            return;
        }
        let mut is_manual_sa_rgb = false;
        if let Ok(ref outcome) = result {
            let sync_enabled = self.document_store.v070_session_sa_rgb_sync_enabled();
            match outcome {
                V070BridgeOutcome::SaRgbApplied {
                    sa,
                    color_hex,
                    brightness,
                    speed,
                    effect,
                } => {
                    is_manual_sa_rgb = true;
                    if let Some(sa_id) = mi_model::SaId::new(*sa) {
                        let style = parse_hex_color(color_hex).ok().and_then(|rgb| {
                            SaRgbLitStyle::from_manual_apply(rgb, *brightness, *speed, *effect)
                        });
                        self.sa_rgb_coordinator.note_manual_sa_rgb_write(
                            session_id,
                            ManualSaRgbWrite::Applied { sa: sa_id, style },
                            sync_enabled,
                        );
                    }
                }
                V070BridgeOutcome::SaRgbReset { sa } => {
                    is_manual_sa_rgb = true;
                    if let Some(sa_id) = mi_model::SaId::new(*sa) {
                        self.sa_rgb_coordinator.note_manual_sa_rgb_write(
                            session_id,
                            ManualSaRgbWrite::Reset { sa: sa_id },
                            sync_enabled,
                        );
                    }
                }
                V070BridgeOutcome::ProfileLayer { layer_index, .. } => {
                    self.v070_hardware.device_layer_override = Some(*layer_index);
                }
                _ => {}
            }
        }
        let succeeded = result.is_ok();
        self.v070_hardware.apply_outcome(seq, result);
        if succeeded {
            if !self.v070_hardware.projection.busy {
                let pending = self.v070_hardware.pending_auto_layer_sync.take();
                if pending.is_some() || seq == 0 {
                    let _ = self.maybe_sync_v070_foreground_layer();
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
        if !self.is_active_v070_session(session_id) {
            return;
        }
        self.v070_hardware.device_layer_override = Some(layer_index);
        let is_expected_echo = self
            .v070_hardware
            .consume_expected_layer_echo(layer_index, self.update_now);
        self.v070_hardware
            .apply_profile_layer_notification(profile_index, layer_index);
        if is_expected_echo {
            self.v070_hardware.projection.status_message =
                format!("前台联动已切至 P{profile_index} · 第 {layer_index} 层（RAM 零擦写）");
        }
        self.sync_v070_session_sa_rgb();
        self.commit_runtime_update();
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
    use crate::firmware_update::FirmwareAction;
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
                store.projection().firmware.v070
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

        poll_until(&mut store, |s| s.projection().firmware.v070.synced);
        let p = store.projection();
        assert!(p.firmware.v070.supported);
        assert_eq!(p.firmware.v070.battery_percent, Some(91));
        assert_eq!(p.firmware.v070.is_charging, Some(false));
        assert_eq!(p.firmware.v070.active_profile, Some(3));
        assert_eq!(p.firmware.v070.active_layer, Some(1));
        assert_eq!(p.firmware.v070.device_name.max_length, 15);
        assert!(p.firmware.v070.pm_state.usb_host_hold);
        assert_eq!(p.firmware.v070.mouse.move_delta, 8);
        assert_eq!(p.firmware.v070.mouse.deadzone, 0.15);

        let before_invalid = store.projection().firmware.v070.clone();
        assert!(store
            .dispatch_command(WorkbenchCommand::Firmware(FirmwareAction::V070Hardware(
                V070HardwareAction::ApplyPmConfig {
                    auto_sleep: true,
                    host_sleep: true,
                    sleep_after_min: 0,
                }
            )))
            .is_err());
        assert!(!store.projection().firmware.v070.busy);
        assert_eq!(
            store.projection().firmware.v070.pm_config,
            before_invalid.pm_config
        );

        store
            .dispatch_command(WorkbenchCommand::Firmware(FirmwareAction::V070Hardware(
                V070HardwareAction::SelectLayer { layer: 2 },
            )))
            .unwrap();
        poll_until(&mut store, |s| {
            !s.projection().firmware.v070.busy
                && s.projection().firmware.v070.active_layer == Some(2)
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
            .dispatch_command(WorkbenchCommand::Firmware(FirmwareAction::V070Hardware(
                V070HardwareAction::ApplyMousePreset {
                    preset: "fast".into(),
                },
            )))
            .unwrap();
        poll_until(&mut store, |s| {
            !s.projection().firmware.v070.busy
                && s.projection().firmware.v070.mouse.preset == "fast"
        });
        assert_eq!(store.projection().firmware.v070.mouse.move_delta, 14);

        store
            .dispatch_command(WorkbenchCommand::Firmware(FirmwareAction::V070Hardware(
                V070HardwareAction::ApplyMouseConfig {
                    move_delta: 12,
                    max_speed: 20,
                    wheel_delta: 2,
                    wheel_max_speed: 12,
                    stick_speed: 3,
                    stick_scroll: 30,
                    deadzone: 0.25,
                },
            )))
            .unwrap();
        poll_until(&mut store, |s| {
            !s.projection().firmware.v070.busy
                && s.projection().firmware.v070.mouse.move_delta == 12
                && s.projection().firmware.v070.mouse.deadzone == 0.25
        });

        store
            .dispatch_command(WorkbenchCommand::Firmware(FirmwareAction::V070Hardware(
                V070HardwareAction::RenameDevice {
                    name: "MicroStudio".into(),
                },
            )))
            .unwrap();
        poll_until(&mut store, |s| {
            !s.projection().firmware.v070.busy
                && s.projection().firmware.v070.device_name.name == "MicroStudio"
        });

        store
            .dispatch_command(WorkbenchCommand::Firmware(FirmwareAction::V070Hardware(
                V070HardwareAction::ApplySaRgb {
                    sa: 7057,
                    color_hex: "#33AAFF".into(),
                    brightness: 0.75,
                    speed: 0.5,
                    effect: 4,
                },
            )))
            .unwrap();
        poll_until(&mut store, |s| {
            !s.projection().firmware.v070.busy
                && s.projection().firmware.v070.sa_rgb.sa == Some(7057)
                && s.projection().firmware.v070.sa_rgb.effect == 4
        });

        store
            .dispatch_command(WorkbenchCommand::Firmware(FirmwareAction::V070Hardware(
                V070HardwareAction::ResetSaRgb { sa: 7057 },
            )))
            .unwrap();
        poll_until(&mut store, |s| {
            !s.projection().firmware.v070.busy && s.projection().firmware.v070.sa_rgb.effect == 0
        });

        link.push_wire(json!({"m": "kb.pl", "p": {"p": 3, "l": 5}}));
        poll_until(&mut store, |s| {
            s.projection().firmware.v070.active_layer == Some(5)
                && s.projection().sidebar.board_layers.observed_layer == Some(5)
        });

        store.system_will_sleep();
        assert_eq!(
            store.projection().firmware.v070.host_sleep_status,
            "sleeping"
        );
        assert!(!store.projection().firmware.v070.supported);
        store.system_did_wake();
        assert!(matches!(
            store.projection().firmware.v070.host_sleep_status.as_str(),
            "waking" | "awake"
        ));
        poll_until(&mut store, |s| {
            s.projection().firmware.v070.host_sleep_status == "awake"
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
        assert!(!store.projection().firmware.v070.supported);
        assert!(!store.projection().firmware.v070.synced);
        assert!(store
            .dispatch_command(WorkbenchCommand::Firmware(FirmwareAction::V070Hardware(
                V070HardwareAction::Refresh,
            )))
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
            store.projection().firmware.v070.host_sleep_status.as_str(),
            "waking" | "awake"
        ));
        poll_until(&mut store, |s| {
            s.projection().firmware.v070.host_sleep_status == "awake"
        });

        poll_until(&mut store, |s| s.projection().firmware.v070.synced);
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
            let err =
                validate_v070_action(&V070HardwareAction::SelectLayer { layer }, &base_mouse, 15)
                    .unwrap_err();
            assert_eq!(
                err,
                format!("板载层索引 {layer} 超出有效范围（仅支持第 1–6 层）")
            );
        }
        for layer in 1..=6 {
            assert!(validate_v070_action(
                &V070HardwareAction::SelectLayer { layer },
                &base_mouse,
                15
            )
            .is_ok());
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
                .dispatch_command(WorkbenchCommand::Firmware(FirmwareAction::V070Hardware(
                    V070HardwareAction::SelectLayer { layer },
                )))
                .is_err());
            assert!(!store.projection().firmware.v070.busy);
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
            V070BridgeOutcome::Snapshot(V070SnapshotPayload {
                session_id: 1,
                version: "0.7.0-rc.16".into(),
                active_profile: Some(3),
                active_layer: Some(layer),
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
            })
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
            running.set_v070_inbox(store.hardware_event_tx.clone());
            running.attach_test_vendor_session(&store.tokio_runtime, session.clone());
            store.six_layer_running = Some(running);

            store.overlay_controller.request_profile_switch_notice(
                "Antigravity".to_string(),
                "自动跟随".to_string(),
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
        assert!(!store.projection().firmware.v070.auto_layer_sync_enabled);
        assert!(!store.document_store.v070_auto_layer_sync_enabled());

        store
            .dispatch_command(WorkbenchCommand::Firmware(FirmwareAction::V070Hardware(
                V070HardwareAction::SetAutoLayerSync { enabled: true },
            )))
            .unwrap();
        assert!(store.projection().firmware.v070.auto_layer_sync_enabled);
        assert!(store.document_store.v070_auto_layer_sync_enabled());
        drop(store);

        let mut reopened = WorkbenchStore::open(cfg).unwrap();
        assert!(reopened.projection().firmware.v070.auto_layer_sync_enabled);
        reopened
            .dispatch_command(WorkbenchCommand::Firmware(FirmwareAction::V070Hardware(
                V070HardwareAction::SetAutoLayerSync { enabled: false },
            )))
            .unwrap();
        assert!(!reopened.projection().firmware.v070.auto_layer_sync_enabled);

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
        let desired = mi_model::board_layers::BoardLayerPlan::recommended(
            profile_id("Codex"),
            profile_id("Antigravity"),
            profile_id("DeepSeek Coder (DSH)"),
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

        poll_until(&mut reopened, |s| s.projection().firmware.v070.synced);
        let badge = reopened
            .projection()
            .header
            .power_status
            .clone()
            .expect("0.7.0 power status badge must be shown in header");
        assert_eq!(badge.battery_percent, Some(91));
        assert_eq!(reopened.projection().status_menu.power_status, Some(badge));

        reopened.set_host_execution_enabled(true).unwrap();
        reopened.set_auto_follow(true).unwrap();
        assert!(reopened.foreground_changed(Some("com.deepseek.dsh")));
        assert!(
            !sent_methods
                .lock()
                .unwrap()
                .iter()
                .any(|m| m == "kb.pl.set"),
            "auto_layer_sync disabled by default must never emit kb.pl.set"
        );

        reopened
            .dispatch_command(WorkbenchCommand::Firmware(FirmwareAction::V070Hardware(
                V070HardwareAction::SetAutoLayerSync { enabled: true },
            )))
            .unwrap();
        poll_until(&mut reopened, |s| {
            !s.projection().firmware.v070.busy
                && s.projection().firmware.v070.active_layer == Some(3)
        });
        assert!(
            reopened.v070_hardware.expected_layer_echo.is_some(),
            "kb.pl.set completion should arm expected_layer_echo"
        );

        link.push_wire(json!({"m": "kb.pl", "p": {"p": 3, "l": 3}}));
        poll_until(&mut reopened, |s| {
            s.v070_hardware.expected_layer_echo.is_none()
        });
        assert!(!reopened.projection().status_menu.manual_override);

        let kb_pl_set_before_codex = sent_methods
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m.as_str() == "kb.pl.set")
            .count();
        assert!(reopened.foreground_changed(Some("com.openai.codex")));
        assert!(!reopened.projection().firmware.v070.busy);
        assert_eq!(
            sent_methods
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.as_str() == "kb.pl.set")
                .count(),
            kb_pl_set_before_codex,
            "OAI native board layer (Codex on L1) must never trigger kb.pl.set"
        );

        reopened
            .dispatch_command(WorkbenchCommand::Firmware(FirmwareAction::V070Hardware(
                V070HardwareAction::ApplyMousePreset {
                    preset: "fast".into(),
                },
            )))
            .unwrap();
        assert!(reopened.projection().firmware.v070.busy);
        assert!(reopened.foreground_changed(Some("com.google.antigravity")));
        assert_eq!(
            reopened.v070_hardware.pending_auto_layer_sync,
            Some(2),
            "layer sync while busy must coalesce to pending_auto_layer_sync"
        );
        poll_until(&mut reopened, |s| {
            !s.projection().firmware.v070.busy
                && s.projection().firmware.v070.active_layer == Some(2)
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
        assert!(reopened.foreground_changed(Some("com.deepseek.dsh")));
        assert!(!reopened.projection().firmware.v070.busy);
        assert_eq!(
            sent_methods
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.as_str() == "kb.pl.set")
                .count(),
            kb_pl_set_while_disabled,
            "host_execution_enabled == false must block kb.pl.set on foreground switches"
        );

        reopened.set_host_execution_enabled(true).unwrap();
        poll_until(&mut reopened, |s| {
            !s.projection().firmware.v070.busy
                && s.projection().firmware.v070.active_layer == Some(3)
        });
        assert_eq!(
            sent_methods
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.as_str() == "kb.pl.set")
                .count(),
            kb_pl_set_while_disabled + 1,
            "re-enabling host_execution_enabled must immediately sync to mapped foreground layer"
        );
        link.push_wire(json!({"m": "kb.pl", "p": {"p": 3, "l": 3}}));
        poll_until(&mut reopened, |s| {
            s.v070_hardware.expected_layer_echo.is_none()
        });

        responder.abort();
        let _ = reopened.tokio_runtime.block_on(&mut responder);
        assert!(reopened.foreground_changed(Some("com.google.antigravity")));
        assert!(reopened.projection().firmware.v070.busy);
        assert!(reopened.v070_hardware.expected_layer_echo.is_some());
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
        poll_until(&mut reopened, |s| !s.projection().firmware.v070.busy);
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
}

#[cfg(test)]
#[path = "v070_session_sa_rgb_store_tests.rs"]
mod fw6_store_tests;
