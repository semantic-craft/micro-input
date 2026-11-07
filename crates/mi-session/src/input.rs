//! Community input candidates are diagnostic only (UNKNOWN-07/10). Raw payloads never leave this module.
use crate::DeviceSession;
use mi_proto::HostNotification;
use std::{
    collections::BTreeSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::broadcast;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEventKind {
    UnknownNotification,
    CandidatePressed,
    CandidateReleased,
    Pressed,
    Released,
    InputStreamGap,
}

pub use mi_model::host_router::{
    verified_physical_mapping, verified_physical_mapping_for_layout, BottomRowLayout,
    PhysicalControlSpec,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RadialPosition {
    pub angle_turns: f64,
    pub distance: f64,
}

#[derive(Debug, Clone)]
pub struct InputEvent {
    pub session_id: u64,
    /// Local subscription order, including skipped items; not a firmware sequence.
    pub sequence: u64,
    pub ingress_sequence: u64,
    /// Physical radial state, including notifications that do not create an action edge.
    pub radial_center: Option<bool>,
    pub radial_position: Option<RadialPosition>,
    /// Local receipt time, not a device timestamp.
    pub timestamp_ms: u64,
    pub kind: InputEventKind,
    pub logical_switch: Option<String>,
    /// Community vendor binding name; never an immutable physical scan position.
    pub binding_id: Option<String>,
    pub physical_control: Option<String>,
    pub keycap_group: Option<String>,
    pub action_role: Option<String>,
    pub lost_count: u64,
    pub action_eligible: bool,
}
/// An evidence-backed decoder may supply logical switches. Physical control, keycap coverage
/// and action role must be mapped separately after physical verification (UNKNOWN-10).
/// This seam is not an outbound request or a firmware schema.
pub trait InputDecoder: Send + Sync {
    fn decode(&self, notification: &HostNotification) -> Option<(InputEventKind, String)>;
}

fn parse_ag_notification(notification: &HostNotification) -> Option<(bool, String)> {
    let HostNotification::Unknown(raw) = notification else {
        return None;
    };
    if ["id", "i", "result", "error"]
        .iter()
        .any(|key| raw.get(key).is_some())
    {
        return None;
    }
    let method = candidate_alias(raw, "method", "m")?.as_str()?;
    if method != "v.oai.hid" {
        return None;
    }
    let params = candidate_alias(raw, "params", "p")?.as_object()?;
    let binding = params.get("k")?.as_str()?;
    let digits = binding.strip_prefix("AG")?;
    if digits.len() != 2
        || !digits.bytes().all(|b| b.is_ascii_digit())
        || digits.parse::<u8>().ok()? > 18
    {
        return None;
    }
    let pressed = match params.get("act")?.as_i64()? {
        1 => true,
        0 => false,
        _ => return None,
    };
    Some((pressed, binding.to_owned()))
}

fn parse_verified_notification(
    notification: &HostNotification,
) -> Option<(InputEventKind, String)> {
    if let Some((pressed, binding)) = parse_ag_notification(notification) {
        let kind = if pressed {
            InputEventKind::Pressed
        } else {
            InputEventKind::Released
        };
        return Some((kind, binding));
    }
    let HostNotification::Unknown(raw) = notification else {
        return None;
    };
    if ["id", "i", "result", "error"]
        .iter()
        .any(|key| raw.get(key).is_some())
    {
        return None;
    }
    let method = candidate_alias(raw, "method", "m")?.as_str()?;
    let params = candidate_alias(raw, "params", "p")?.as_object()?;
    match method {
        "v.oai.hid" => {
            let binding = params.get("k")?.as_str()?;
            let act = params.get("act")?.as_i64()?;
            if let Some(digits) = binding.strip_prefix("ACT") {
                if digits.len() == 2
                    && digits.bytes().all(|b| b.is_ascii_digit())
                    && matches!(digits.parse::<u8>().ok(), Some(6..=12))
                {
                    let kind = match act {
                        1 => InputEventKind::Pressed,
                        0 => InputEventKind::Released,
                        _ => return None,
                    };
                    return Some((kind, format!("AG{digits}")));
                }
            }
            match (binding, act) {
                ("ENC_CC" | "AG13", 2) => Some((InputEventKind::Pressed, "AG13".into())),
                ("ENC_CW" | "AG14", 2) => Some((InputEventKind::Pressed, "AG14".into())),
                ("ENC_CC", 1) => Some((InputEventKind::Pressed, "AG13".into())),
                ("ENC_CC", 0) => Some((InputEventKind::Released, "AG13".into())),
                ("ENC_CW", 1) => Some((InputEventKind::Pressed, "AG14".into())),
                ("ENC_CW", 0) => Some((InputEventKind::Released, "AG14".into())),
                ("ENC_CLK", 1) => Some((InputEventKind::Pressed, "ENC_CLK".into())),
                ("ENC_CLK", 0) => Some((InputEventKind::Released, "ENC_CLK".into())),
                _ => None,
            }
        }
        "v.oai.rad" => {
            let RadialPosition {
                angle_turns: angle,
                distance,
            } = parse_radial_position(notification)?;
            if distance >= 0.5 {
                let norm = angle.rem_euclid(1.0);
                // Firmware 0.6.3 radial turns are normalized [0.0, 1.0) in screen coordinates
                // (FACTORY-DEFAULTS.md §6): 0.0 = East (Right), 0.25 = South (Down),
                // 0.5 = West (Left), 0.75 = North (Up).
                let sector = if (0.625..0.875).contains(&norm) {
                    "AG15"
                } else if (0.375..0.625).contains(&norm) {
                    "AG16"
                } else if (0.125..0.375).contains(&norm) {
                    "AG17"
                } else {
                    "AG18"
                };
                Some((InputEventKind::Pressed, sector.into()))
            } else if distance < 0.35 {
                Some((InputEventKind::Released, "__RADIAL_CENTER__".into()))
            } else {
                Some((InputEventKind::Released, "__RADIAL_MID__".into()))
            }
        }
        _ => None,
    }
}

fn parse_radial_position(notification: &HostNotification) -> Option<RadialPosition> {
    let HostNotification::Unknown(raw) = notification else {
        return None;
    };
    if ["id", "i", "result", "error"]
        .iter()
        .any(|key| raw.get(key).is_some())
    {
        return None;
    }
    if candidate_alias(raw, "method", "m")?.as_str()? != "v.oai.rad" {
        return None;
    }
    let params = candidate_alias(raw, "params", "p")?.as_object()?;
    let angle = params.get("a")?.as_f64()?;
    let distance = params.get("d")?.as_f64()?;
    if !angle.is_finite()
        || !distance.is_finite()
        || !(0.0..=1.0).contains(&angle)
        || !(0.0..=1.0).contains(&distance)
    {
        return None;
    }
    Some(RadialPosition {
        angle_turns: angle.rem_euclid(1.0),
        distance,
    })
}

fn is_radial_notification(notification: &HostNotification) -> bool {
    let HostNotification::Unknown(raw) = notification else {
        return false;
    };
    candidate_alias(raw, "method", "m").and_then(|v| v.as_str()) == Some("v.oai.rad")
}

/// Dispatch's public 0.6.2 observation, not current-device or physical-mapping evidence.
/// This accepts only the AG00–AG18 variant; historical ACT/ENC/act=2 remain unknown.
pub struct CandidateInputDecoder;
impl InputDecoder for CandidateInputDecoder {
    fn decode(&self, notification: &HostNotification) -> Option<(InputEventKind, String)> {
        let (pressed, binding) = parse_ag_notification(notification)?;
        let kind = if pressed {
            InputEventKind::CandidatePressed
        } else {
            InputEventKind::CandidateReleased
        };
        Some((kind, binding))
    }
}

/// Verified host routing decoder for AG00–AG18 and stock 0.6.3 Codex layer (ACT06–12, ENC_*, v.oai.rad)
/// with 2U wide-key coalescing and radial edge-triggering in InputMonitor.
pub struct VerifiedInputDecoder;
impl InputDecoder for VerifiedInputDecoder {
    fn decode(&self, notification: &HostNotification) -> Option<(InputEventKind, String)> {
        parse_verified_notification(notification)
    }
}

fn candidate_alias<'a>(
    raw: &'a serde_json::Value,
    long: &str,
    short: &str,
) -> Option<&'a serde_json::Value> {
    match (raw.get(long), raw.get(short)) {
        (Some(a), Some(b)) if a != b => None,
        (Some(value), _) | (_, Some(value)) => Some(value),
        _ => None,
    }
}

pub struct UnverifiedInputDecoder;
impl InputDecoder for UnverifiedInputDecoder {
    fn decode(&self, _: &HostNotification) -> Option<(InputEventKind, String)> {
        None
    }
}
pub struct InputMonitor {
    session: Arc<DeviceSession>,
    receiver: broadcast::Receiver<crate::ReceivedNotification>,
    decoder: Box<dyn InputDecoder>,
    sequence: u64,
    stopped: bool,
    pressed: BTreeSet<String>,
    active_wide_switches: BTreeSet<String>,
    active_radial_sector: Option<String>,
    verified_radial: bool,
    merge_wide_key: Arc<AtomicBool>,
    was_merging_wide_key: bool,
}
impl DeviceSession {
    pub fn subscribe_input(self: &Arc<Self>) -> InputMonitor {
        self.subscribe_input_with_decoder(Box::new(CandidateInputDecoder))
    }
    pub fn subscribe_verified_input(self: &Arc<Self>) -> InputMonitor {
        self.subscribe_verified_input_with_merge_flag(Arc::new(AtomicBool::new(true)))
    }
    pub fn subscribe_verified_input_with_merge_flag(
        self: &Arc<Self>,
        merge_wide_key: Arc<AtomicBool>,
    ) -> InputMonitor {
        let was_merging_wide_key = merge_wide_key.load(Ordering::SeqCst);
        InputMonitor {
            session: self.clone(),
            receiver: self.notifications(),
            decoder: Box::new(VerifiedInputDecoder),
            sequence: 0,
            stopped: false,
            pressed: BTreeSet::new(),
            active_wide_switches: BTreeSet::new(),
            active_radial_sector: None,
            verified_radial: true,
            merge_wide_key,
            was_merging_wide_key,
        }
    }
    pub fn subscribe_input_with_decoder(
        self: &Arc<Self>,
        decoder: Box<dyn InputDecoder>,
    ) -> InputMonitor {
        InputMonitor {
            session: self.clone(),
            receiver: self.notifications(),
            decoder,
            sequence: 0,
            stopped: false,
            pressed: BTreeSet::new(),
            active_wide_switches: BTreeSet::new(),
            active_radial_sector: None,
            verified_radial: false,
            merge_wide_key: Arc::new(AtomicBool::new(false)),
            was_merging_wide_key: false,
        }
    }
}
impl InputMonitor {
    fn radial_state_event(
        &mut self,
        ingress_sequence: u64,
        centered: bool,
        radial_position: Option<RadialPosition>,
    ) -> InputEvent {
        self.sequence += 1;
        InputEvent {
            session_id: self.session.id,
            sequence: self.sequence,
            ingress_sequence,
            radial_center: Some(centered),
            radial_position,
            timestamp_ms: 0,
            kind: InputEventKind::UnknownNotification,
            logical_switch: None,
            binding_id: None,
            physical_control: None,
            keycap_group: None,
            action_role: None,
            lost_count: 0,
            action_eligible: false,
        }
    }
    pub fn pressed_switches(&self) -> Vec<String> {
        self.pressed.iter().cloned().collect()
    }
    pub fn active_wide_switches(&self) -> Vec<String> {
        self.active_wide_switches.iter().cloned().collect()
    }
    pub fn stopped(&self) -> bool {
        self.stopped
    }
    pub fn close(&mut self) {
        self.stopped = true;
        self.pressed.clear();
        self.active_wide_switches.clear();
        self.active_radial_sector = None;
    }
    /// Overflow is terminal for this subscription. Recovery creates a new subscription and
    /// empty state; no old buffered notification can resume delivery after a gap.
    pub async fn next(&mut self) -> Option<InputEvent> {
        loop {
            if self.stopped || self.session.invalid.load(Ordering::SeqCst) {
                self.close();
                return None;
            }
            let session = self.session.clone();
            let changed = session.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.session.invalid.load(Ordering::SeqCst) {
                self.stopped = true;
                self.pressed.clear();
                self.active_wide_switches.clear();
                self.active_radial_sector = None;
                return None;
            }
            let result = tokio::select! { biased; _ = changed => None, value = self.receiver.recv() => Some(value) };
            if self.session.invalid.load(Ordering::SeqCst) || result.is_none() {
                self.stopped = true;
                self.pressed.clear();
                self.active_wide_switches.clear();
                self.active_radial_sector = None;
                return None;
            }
            let mut ingress_sequence = 0;
            let mut radial_center = None;
            let mut radial_position = None;
            let (
                kind,
                logical_switch,
                binding_id,
                physical_control,
                keycap_group,
                action_role,
                lost_count,
                action_eligible,
            ) = match result.unwrap() {
                Ok(received) => {
                    ingress_sequence = received.ingress_sequence;
                    let notification = received.notification;
                    let is_radial = is_radial_notification(&notification);
                    if self.verified_radial {
                        radial_position = parse_radial_position(&notification);
                    }
                    let merge_wide = self.merge_wide_key.load(Ordering::SeqCst);
                    if merge_wide != self.was_merging_wide_key {
                        self.active_wide_switches.clear();
                        self.was_merging_wide_key = merge_wide;
                    }
                    let layout = if merge_wide {
                        BottomRowLayout::Merged
                    } else {
                        BottomRowLayout::Split
                    };
                    let map_physical = |id: &str| -> Option<PhysicalControlSpec> {
                        if self.verified_radial {
                            verified_physical_mapping_for_layout(id, layout)
                        } else {
                            verified_physical_mapping(id)
                        }
                    };
                    let decoded = self.decoder.decode(&notification);
                    if is_radial {
                        radial_center = match &decoded {
                            Some((_, id)) if id == "__RADIAL_CENTER__" => Some(true),
                            Some(_) => Some(false),
                            _ => None,
                        };
                    }
                    let normalized = match decoded {
                        Some((InputEventKind::Pressed, sector)) if is_radial => {
                            if self.active_radial_sector.as_deref() == Some(sector.as_str()) {
                                return Some(self.radial_state_event(
                                    ingress_sequence,
                                    false,
                                    radial_position,
                                ));
                            }
                            if let Some(prev) = self.active_radial_sector.replace(sector.clone()) {
                                if let Some(prev_spec) = map_physical(&prev) {
                                    self.pressed.remove(prev_spec.logical_switch);
                                } else {
                                    self.pressed.remove(&prev);
                                }
                            }
                            Some((InputEventKind::Pressed, sector))
                        }
                        Some((InputEventKind::Released, ref raw_id))
                            if raw_id == "__RADIAL_CENTER__" =>
                        {
                            let Some(prev) = self.active_radial_sector.take() else {
                                return Some(self.radial_state_event(
                                    ingress_sequence,
                                    true,
                                    radial_position,
                                ));
                            };
                            Some((InputEventKind::Released, prev))
                        }
                        Some((_, ref raw_id)) if raw_id == "__RADIAL_MID__" => {
                            return Some(self.radial_state_event(
                                ingress_sequence,
                                false,
                                radial_position,
                            ));
                        }
                        other => other,
                    };
                    self.sequence += 1;
                    match normalized {
                        Some((InputEventKind::Pressed, raw_id)) => {
                            if merge_wide && matches!(raw_id.as_str(), "AG10" | "AG11") {
                                let was_empty = self.active_wide_switches.is_empty();
                                self.active_wide_switches.insert(raw_id);
                                if !was_empty {
                                    continue;
                                }
                                let spec = verified_physical_mapping_for_layout(
                                    "AG10",
                                    BottomRowLayout::Merged,
                                )
                                .unwrap();
                                self.pressed.insert(spec.logical_switch.to_owned());
                                (
                                    InputEventKind::Pressed,
                                    Some(spec.logical_switch.to_owned()),
                                    Some(spec.binding_id.to_owned()),
                                    Some(spec.physical_control.to_owned()),
                                    Some(spec.keycap_group.to_owned()),
                                    Some(spec.action_role.to_owned()),
                                    0,
                                    true,
                                )
                            } else if let Some(spec) = map_physical(&raw_id) {
                                if spec.action_role != "encoder_step" {
                                    if !self.pressed.insert(spec.logical_switch.to_owned()) {
                                        continue;
                                    }
                                }
                                (
                                    InputEventKind::Pressed,
                                    Some(spec.logical_switch.to_owned()),
                                    Some(spec.binding_id.to_owned()),
                                    Some(spec.physical_control.to_owned()),
                                    Some(spec.keycap_group.to_owned()),
                                    Some(spec.action_role.to_owned()),
                                    0,
                                    true,
                                )
                            } else {
                                self.pressed.insert(raw_id.clone());
                                (
                                    InputEventKind::Pressed,
                                    Some(raw_id),
                                    None,
                                    None,
                                    None,
                                    None,
                                    0,
                                    false,
                                )
                            }
                        }
                        Some((InputEventKind::Released, raw_id)) => {
                            if merge_wide && matches!(raw_id.as_str(), "AG10" | "AG11") {
                                let removed = self.active_wide_switches.remove(&raw_id);
                                if !removed || !self.active_wide_switches.is_empty() {
                                    continue;
                                }
                                let spec = verified_physical_mapping_for_layout(
                                    "AG10",
                                    BottomRowLayout::Merged,
                                )
                                .unwrap();
                                self.pressed.remove(spec.logical_switch);
                                (
                                    InputEventKind::Released,
                                    Some(spec.logical_switch.to_owned()),
                                    Some(spec.binding_id.to_owned()),
                                    Some(spec.physical_control.to_owned()),
                                    Some(spec.keycap_group.to_owned()),
                                    Some(spec.action_role.to_owned()),
                                    0,
                                    true,
                                )
                            } else if let Some(spec) = map_physical(&raw_id) {
                                self.pressed.remove(spec.logical_switch);
                                (
                                    InputEventKind::Released,
                                    Some(spec.logical_switch.to_owned()),
                                    Some(spec.binding_id.to_owned()),
                                    Some(spec.physical_control.to_owned()),
                                    Some(spec.keycap_group.to_owned()),
                                    Some(spec.action_role.to_owned()),
                                    0,
                                    true,
                                )
                            } else {
                                self.pressed.remove(&raw_id);
                                (
                                    InputEventKind::Released,
                                    Some(raw_id),
                                    None,
                                    None,
                                    None,
                                    None,
                                    0,
                                    false,
                                )
                            }
                        }
                        Some((
                            kind @ (InputEventKind::CandidatePressed
                            | InputEventKind::CandidateReleased),
                            binding,
                        )) => (kind, None, Some(binding), None, None, None, 0, false),
                        _ => (
                            InputEventKind::UnknownNotification,
                            None,
                            None,
                            None,
                            None,
                            None,
                            0,
                            false,
                        ),
                    }
                }
                Err(broadcast::error::RecvError::Lagged(count)) => {
                    self.sequence += count;
                    self.stopped = true;
                    self.pressed.clear();
                    self.active_wide_switches.clear();
                    self.active_radial_sector = None;
                    (
                        InputEventKind::InputStreamGap,
                        None,
                        None,
                        None,
                        None,
                        None,
                        count,
                        false,
                    )
                }
                Err(broadcast::error::RecvError::Closed) => {
                    self.stopped = true;
                    self.pressed.clear();
                    self.active_wide_switches.clear();
                    self.active_radial_sector = None;
                    return None;
                }
            };
            return Some(InputEvent {
                session_id: self.session.id,
                sequence: self.sequence,
                ingress_sequence,
                radial_center,
                radial_position,
                timestamp_ms: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
                    .min(u64::MAX as u128) as u64,
                kind,
                logical_switch,
                binding_id,
                physical_control,
                keycap_group,
                action_role,
                lost_count,
                action_eligible,
            });
        }
    }
}
