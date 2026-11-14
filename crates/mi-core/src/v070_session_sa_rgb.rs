use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use mi_model::document::{BindingNode, LocalDocumentV2};
use mi_model::draft::ActionKind;
use mi_model::slot_table::SLOT_COUNT;
use mi_model::{
    binding_has_oai_prefix, extract_sa_id, is_protected_oai_layer, Binding, Layer, SaId, Token,
};
use mi_proto::v070_capabilities::{SaRgbEffect, SaRgbUpdate};

use crate::session_strip::FourAppSessionKind;

pub const SESSION_SA_RGB_MIN_INTERVAL: Duration = Duration::from_millis(100);
pub const SESSION_SA_RGB_TEARDOWN_TIMEOUT: Duration = Duration::from_millis(200);
pub const MAX_SA_IDS_PER_UPDATE: usize = 16;
pub const MAX_AUTO_SA_RGB_REQUEST_ID: u64 = 980;
pub const MAX_AUTO_RESET_REQUEST_ID: u64 = 995;
pub const MAX_TEARDOWN_REQUEST_ID: u64 = 998;
pub const MAX_CONSECUTIVE_FAILURES: u8 = 3;

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SaRgbLitStyle {
    rgb: u32,
    effect: SaRgbEffect,
    brightness_milli: u16,
    speed_milli: u16,
}

impl SaRgbLitStyle {
    pub const fn new(
        rgb: u32,
        effect: SaRgbEffect,
        brightness_milli: u16,
        speed_milli: u16,
    ) -> Option<Self> {
        if matches!(effect, SaRgbEffect::Off)
            || rgb > SaId::MAX
            || brightness_milli > 1000
            || speed_milli > 1000
        {
            return None;
        }
        Some(Self {
            rgb,
            effect,
            brightness_milli,
            speed_milli,
        })
    }

    pub fn from_manual_apply(rgb: u32, brightness: f64, speed: f64, effect: u8) -> Option<Self> {
        let parsed = SaRgbEffect::from_u8(effect).ok()?;
        if parsed == SaRgbEffect::Off || !brightness.is_finite() || !speed.is_finite() {
            return None;
        }
        let b = (brightness.clamp(0.0, 1.0) * 1000.0).round() as u16;
        let s = (speed.clamp(0.0, 1.0) * 1000.0).round() as u16;
        Self::new(rgb, parsed, b, s)
    }

    pub const fn rgb(self) -> u32 {
        self.rgb
    }

    pub const fn effect(self) -> SaRgbEffect {
        self.effect
    }

    pub fn to_sa_rgb_update(self, sa: Vec<u32>) -> SaRgbUpdate {
        SaRgbUpdate {
            sa,
            c: Some(self.rgb),
            b: Some(f32::from(self.brightness_milli) / 1000.0),
            s: Some(f32::from(self.speed_milli) / 1000.0),
            e: Some(self.effect.as_u8()),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DesiredSaLighting {
    DefaultOnboard,
    Override(SaRgbLitStyle),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayerLightingResolution {
    Active(BTreeMap<SaId, SaRgbLitStyle>),
    ProtectedOai,
    MissingLayer,
}

pub const fn session_kind_urgency(kind: FourAppSessionKind) -> u8 {
    match kind {
        FourAppSessionKind::NeedsAttention => 4,
        FourAppSessionKind::Error => 3,
        FourAppSessionKind::BlueDotCompleted => 2,
        FourAppSessionKind::Working => 1,
        FourAppSessionKind::Idle => 0,
    }
}

pub fn aggregate_session_kind(slot_kinds: &[FourAppSessionKind; SLOT_COUNT]) -> FourAppSessionKind {
    slot_kinds
        .iter()
        .copied()
        .max_by_key(|&k| session_kind_urgency(k))
        .unwrap_or(FourAppSessionKind::Idle)
}

const fn lit_override(rgb: u32, effect: SaRgbEffect, b: u16, s: u16) -> DesiredSaLighting {
    DesiredSaLighting::Override(SaRgbLitStyle {
        rgb,
        effect,
        brightness_milli: b,
        speed_milli: s,
    })
}

pub const fn map_session_kind_to_sa_lighting(kind: FourAppSessionKind) -> DesiredSaLighting {
    match kind {
        FourAppSessionKind::Idle => DesiredSaLighting::DefaultOnboard,
        FourAppSessionKind::Working => lit_override(0x00FF66, SaRgbEffect::ShallowBreath, 850, 500),
        FourAppSessionKind::NeedsAttention => {
            lit_override(0xFFAA00, SaRgbEffect::Breath, 1000, 750)
        }
        FourAppSessionKind::BlueDotCompleted => {
            lit_override(0x0088FF, SaRgbEffect::Solid, 900, 500)
        }
        FourAppSessionKind::Error => lit_override(0xFF2200, SaRgbEffect::Breath, 1000, 850),
    }
}

fn merge_sa_kind(
    kinds_by_sa: &mut BTreeMap<SaId, FourAppSessionKind>,
    sa: SaId,
    candidate: FourAppSessionKind,
) {
    kinds_by_sa
        .entry(sa)
        .and_modify(|existing| {
            if session_kind_urgency(candidate) > session_kind_urgency(*existing) {
                *existing = candidate;
            }
        })
        .or_insert(candidate);
}

fn finish_desired_map(
    mut kinds_by_sa: BTreeMap<SaId, FourAppSessionKind>,
    aggregate: FourAppSessionKind,
    fallback_sa: Option<SaId>,
) -> BTreeMap<SaId, SaRgbLitStyle> {
    if kinds_by_sa.is_empty() {
        if let Some(sa) = fallback_sa {
            kinds_by_sa.insert(sa, aggregate);
        }
    }
    kinds_by_sa
        .into_iter()
        .filter_map(|(sa, kind)| match map_session_kind_to_sa_lighting(kind) {
            DesiredSaLighting::Override(style) => Some((sa, style)),
            DesiredSaLighting::DefaultOnboard => None,
        })
        .collect()
}

pub fn resolve_desired_for_device_layer(
    layer: &Layer,
    slot_kinds: &[FourAppSessionKind; SLOT_COUNT],
    fallback_sa: Option<SaId>,
) -> LayerLightingResolution {
    if is_protected_oai_layer(layer) {
        return LayerLightingResolution::ProtectedOai;
    }
    let aggregate = aggregate_session_kind(slot_kinds);
    let mut kinds_by_sa = BTreeMap::new();
    for (idx, binding) in layer.keys.iter().enumerate() {
        if let Some(sa) = extract_sa_id(binding) {
            let kind = slot_kinds.get(idx).copied().unwrap_or(aggregate);
            merge_sa_kind(&mut kinds_by_sa, sa, kind);
        }
    }
    for binding in layer.encoders.iter().chain(layer.joystick.sectors.iter()) {
        if let Some(sa) = extract_sa_id(binding) {
            merge_sa_kind(&mut kinds_by_sa, sa, aggregate);
        }
    }
    LayerLightingResolution::Active(finish_desired_map(kinds_by_sa, aggregate, fallback_sa))
}

enum EffectiveControlBinding {
    Sa(SaId),
    NonSa,
    ProtectedOai,
}

fn classify_binding(binding: &Binding) -> EffectiveControlBinding {
    if binding_has_oai_prefix(binding) {
        EffectiveControlBinding::ProtectedOai
    } else if let Some(sa) = extract_sa_id(binding) {
        EffectiveControlBinding::Sa(sa)
    } else {
        EffectiveControlBinding::NonSa
    }
}

fn classify_doc_control(doc: &LocalDocumentV2, node: &BindingNode) -> EffectiveControlBinding {
    if let Some(aid) = &node.action_id {
        let Some(action) = doc.editable.graph.actions.get(aid) else {
            return EffectiveControlBinding::NonSa;
        };
        let Ok(ActionKind::DeviceKey { token }) =
            serde_json::from_value::<ActionKind>(action.payload.clone())
        else {
            return EffectiveControlBinding::NonSa;
        };
        return classify_binding(&Binding::Basic(Token(token)));
    }
    serde_json::from_value::<Binding>(node.original.clone())
        .map(|b| classify_binding(&b))
        .unwrap_or(EffectiveControlBinding::NonSa)
}

pub fn resolve_desired_for_document_layer(
    doc: &LocalDocumentV2,
    layer_id: &str,
    slot_kinds: &[FourAppSessionKind; SLOT_COUNT],
    fallback_sa: Option<SaId>,
) -> LayerLightingResolution {
    if doc.is_protected_oai_layer(layer_id) {
        return LayerLightingResolution::ProtectedOai;
    }
    let Some(layer) = doc.editable.graph.layers.get(layer_id) else {
        return LayerLightingResolution::MissingLayer;
    };
    let aggregate = aggregate_session_kind(slot_kinds);
    let mut kinds_by_sa = BTreeMap::new();
    for node in layer.controls.values() {
        match classify_doc_control(doc, node) {
            EffectiveControlBinding::ProtectedOai => return LayerLightingResolution::ProtectedOai,
            EffectiveControlBinding::NonSa => {}
            EffectiveControlBinding::Sa(sa) => {
                let Some(slot_idx) = node
                    .logical_control
                    .strip_prefix("logical-")
                    .and_then(|s| s.parse::<usize>().ok())
                else {
                    continue;
                };
                let kind = slot_kinds.get(slot_idx).copied().unwrap_or(aggregate);
                merge_sa_kind(&mut kinds_by_sa, sa, kind);
            }
        }
    }
    LayerLightingResolution::Active(finish_desired_map(kinds_by_sa, aggregate, fallback_sa))
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum UrgentClearReason {
    OptInDisabled,
    ProtectedLayer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DesiredTarget {
    Active(BTreeMap<SaId, SaRgbLitStyle>),
    UrgentClear(UrgentClearReason),
    Inactive,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ManualSaRgbWrite {
    Applied {
        sa: SaId,
        style: Option<SaRgbLitStyle>,
    },
    Reset {
        sa: SaId,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaRgbChunkKind {
    Reset(Vec<SaId>),
    Set {
        style: SaRgbLitStyle,
        ids: Vec<SaId>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct SaRgbBatchChunk {
    pub update: SaRgbUpdate,
    pub kind: SaRgbChunkKind,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DispatchedSaRgbBatch {
    pub batch_id: u64,
    pub chunks: Vec<SaRgbBatchChunk>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ReconcileAction {
    NoOp,
    ScheduleWake(Instant),
    Dispatch(DispatchedSaRgbBatch),
}

#[derive(Default, Debug)]
pub struct SessionSaRgbReconciler {
    confirmed: BTreeMap<SaId, SaRgbLitStyle>,
    desired: BTreeMap<SaId, SaRgbLitStyle>,
    uncertain_dirty: BTreeSet<SaId>,
    manual_dirty: BTreeSet<SaId>,
    pub(super) in_flight: Option<DispatchedSaRgbBatch>,
    next_batch_id: u64,
    next_allowed_at: Option<Instant>,
    consecutive_failures: u8,
    halted_on_error: bool,
    last_target_was_urgent_clear: bool,
    pub(super) urgent_clear_pending: bool,
    parked: bool,
}

impl SessionSaRgbReconciler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn confirmed(&self) -> &BTreeMap<SaId, SaRgbLitStyle> {
        &self.confirmed
    }

    pub fn has_dirty_keys(&self) -> bool {
        !self.confirmed.is_empty()
            || !self.uncertain_dirty.is_empty()
            || !self.manual_dirty.is_empty()
            || self.in_flight.is_some()
    }

    pub fn halted_on_error(&self) -> bool {
        self.halted_on_error
    }

    pub fn consecutive_failures(&self) -> u8 {
        self.consecutive_failures
    }

    pub fn park_without_session(&mut self) {
        self.parked = true;
        self.desired.clear();
        self.urgent_clear_pending = false;
        self.last_target_was_urgent_clear = false;
    }

    pub fn reset_for_new_session(&mut self) {
        self.confirmed.clear();
        self.uncertain_dirty.clear();
        self.manual_dirty.clear();
        self.in_flight = None;
        self.next_allowed_at = None;
        self.consecutive_failures = 0;
        self.halted_on_error = false;
        self.last_target_was_urgent_clear = false;
        self.urgent_clear_pending = false;
        self.parked = false;
    }

    pub fn clear_on_disconnect(&mut self) {
        self.desired.clear();
        self.reset_for_new_session();
    }

    pub fn note_manual_sa_rgb_write(&mut self, write: ManualSaRgbWrite, sync_enabled: bool) {
        match write {
            ManualSaRgbWrite::Applied { sa, style } => {
                self.manual_dirty.insert(sa);
                if sync_enabled {
                    if let Some(lit) = style {
                        self.confirmed.insert(sa, lit);
                        self.uncertain_dirty.remove(&sa);
                    } else {
                        self.confirmed.remove(&sa);
                        self.uncertain_dirty.insert(sa);
                    }
                }
            }
            ManualSaRgbWrite::Reset { sa } => {
                self.confirmed.remove(&sa);
                self.uncertain_dirty.remove(&sa);
                self.manual_dirty.remove(&sa);
            }
        }
    }

    fn drain_manual_dirty(&mut self) -> bool {
        let had_manual = !self.manual_dirty.is_empty();
        for sa in std::mem::take(&mut self.manual_dirty) {
            if !self.confirmed.contains_key(&sa) {
                self.uncertain_dirty.insert(sa);
            }
        }
        had_manual
    }

    pub fn reconcile(
        &mut self,
        now: Instant,
        target: DesiredTarget,
        request_count: u64,
    ) -> ReconcileAction {
        self.parked = false;
        match target {
            DesiredTarget::Active(next_desired) => {
                let had_manual = self.drain_manual_dirty();
                if self.last_target_was_urgent_clear || next_desired != self.desired || had_manual {
                    self.consecutive_failures = 0;
                    self.halted_on_error = false;
                }
                self.last_target_was_urgent_clear = false;
                self.desired = next_desired;
                self.urgent_clear_pending = false;
                self.try_dispatch(now, request_count, false)
            }
            DesiredTarget::UrgentClear(_reason) => {
                let had_manual = self.drain_manual_dirty();
                if !self.last_target_was_urgent_clear || had_manual {
                    self.consecutive_failures = 0;
                    self.halted_on_error = false;
                }
                self.last_target_was_urgent_clear = true;
                self.desired.clear();
                self.urgent_clear_pending = true;
                self.try_dispatch(now, request_count, true)
            }
            DesiredTarget::Inactive => {
                self.desired.clear();
                if self.confirmed.is_empty() && self.uncertain_dirty.is_empty() {
                    self.urgent_clear_pending = false;
                    self.last_target_was_urgent_clear = false;
                    ReconcileAction::NoOp
                } else {
                    self.try_dispatch(now, request_count, self.urgent_clear_pending)
                }
            }
        }
    }

    pub fn flush_due(&mut self, now: Instant, request_count: u64) -> ReconcileAction {
        self.try_dispatch(now, request_count, self.urgent_clear_pending)
    }

    pub fn complete_batch(
        &mut self,
        now: Instant,
        batch_id: u64,
        completed_chunks: usize,
        request_count: u64,
    ) -> ReconcileAction {
        self.finish_batch(now, batch_id, completed_chunks, request_count, false)
    }

    fn finish_batch(
        &mut self,
        now: Instant,
        batch_id: u64,
        completed_chunks: usize,
        request_count: u64,
        preempted: bool,
    ) -> ReconcileAction {
        let Some(batch) = self.in_flight.take() else {
            return ReconcileAction::NoOp;
        };
        if batch.batch_id != batch_id {
            self.in_flight = Some(batch);
            return ReconcileAction::NoOp;
        }
        let done = completed_chunks.min(batch.chunks.len());
        for chunk in &batch.chunks[..done] {
            match &chunk.kind {
                SaRgbChunkKind::Reset(ids) => {
                    for sa in ids {
                        self.confirmed.remove(sa);
                        self.uncertain_dirty.remove(sa);
                        self.manual_dirty.remove(sa);
                    }
                }
                SaRgbChunkKind::Set { style, ids } => {
                    for &sa in ids {
                        self.confirmed.insert(sa, *style);
                        self.uncertain_dirty.remove(&sa);
                    }
                }
            }
        }
        for chunk in &batch.chunks[done..] {
            match (&chunk.kind, preempted) {
                (SaRgbChunkKind::Reset(ids), _) | (SaRgbChunkKind::Set { ids, .. }, false) => {
                    for &sa in ids {
                        self.confirmed.remove(&sa);
                        self.uncertain_dirty.insert(sa);
                    }
                }
                (SaRgbChunkKind::Set { .. }, true) => {}
            }
        }
        if !preempted {
            if done < batch.chunks.len() {
                self.consecutive_failures = self.consecutive_failures.saturating_add(1);
                let shift = self.consecutive_failures.min(MAX_CONSECUTIVE_FAILURES) - 1;
                self.next_allowed_at = Some(now + Duration::from_millis(100 * (1u64 << shift)));
                if self.consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                    self.halted_on_error = true;
                }
            } else {
                self.consecutive_failures = 0;
                self.halted_on_error = false;
            }
        }
        self.try_dispatch(now, request_count, self.urgent_clear_pending)
    }

    pub fn take_dirty_for_teardown(&mut self) -> Vec<SaRgbUpdate> {
        let mut dirty = BTreeSet::new();
        dirty.extend(self.confirmed.keys().copied());
        dirty.extend(self.uncertain_dirty.iter().copied());
        dirty.extend(self.manual_dirty.iter().copied());
        if let Some(batch) = self.in_flight.take() {
            for chunk in batch.chunks {
                let (SaRgbChunkKind::Reset(ids) | SaRgbChunkKind::Set { ids, .. }) = chunk.kind;
                dirty.extend(ids);
            }
        }
        self.confirmed.clear();
        self.desired.clear();
        self.uncertain_dirty.clear();
        self.manual_dirty.clear();
        self.next_allowed_at = None;
        self.consecutive_failures = 0;
        self.halted_on_error = false;
        self.last_target_was_urgent_clear = false;
        self.urgent_clear_pending = false;

        let ids: Vec<u32> = dirty.into_iter().map(SaId::get).collect();
        ids.chunks(MAX_SA_IDS_PER_UPDATE)
            .map(|c| SaRgbUpdate::reset(c.to_vec()))
            .collect()
    }

    pub fn restore_unconfirmed_teardown(&mut self, unconfirmed: &[SaRgbUpdate]) {
        for update in unconfirmed {
            for &raw in &update.sa {
                if let Some(sa) = SaId::new(raw) {
                    self.uncertain_dirty.insert(sa);
                }
            }
        }
    }

    fn try_dispatch(
        &mut self,
        now: Instant,
        request_count: u64,
        bypass_cooldown: bool,
    ) -> ReconcileAction {
        if self.parked || self.in_flight.is_some() {
            return ReconcileAction::NoOp;
        }
        if self.halted_on_error {
            self.urgent_clear_pending = false;
            return ReconcileAction::NoOp;
        }
        let Some(mut batch) = self.build_pending_batch(request_count) else {
            self.urgent_clear_pending = false;
            return ReconcileAction::NoOp;
        };
        if !bypass_cooldown || self.consecutive_failures > 0 {
            if let Some(ready_at) = self.next_allowed_at.filter(|&ready_at| now < ready_at) {
                return ReconcileAction::ScheduleWake(ready_at);
            }
        }
        self.next_batch_id = self.next_batch_id.saturating_add(1);
        batch.batch_id = self.next_batch_id;
        self.next_allowed_at = Some(now + SESSION_SA_RGB_MIN_INTERVAL);
        self.urgent_clear_pending = false;
        self.in_flight = Some(batch.clone());
        ReconcileAction::Dispatch(batch)
    }

    fn build_pending_batch(&self, request_count: u64) -> Option<DispatchedSaRgbBatch> {
        let resets: Vec<SaId> = self
            .confirmed
            .keys()
            .chain(self.uncertain_dirty.iter())
            .copied()
            .filter(|sa| !self.desired.contains_key(sa))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let mut chunks = Vec::new();
        let reset_budget = MAX_AUTO_RESET_REQUEST_ID.saturating_sub(request_count) as usize;
        for chunk in resets.chunks(MAX_SA_IDS_PER_UPDATE).take(reset_budget) {
            chunks.push(SaRgbBatchChunk {
                update: SaRgbUpdate::reset(chunk.iter().copied().map(SaId::get).collect()),
                kind: SaRgbChunkKind::Reset(chunk.to_vec()),
            });
        }
        let set_budget = MAX_AUTO_SA_RGB_REQUEST_ID
            .saturating_sub(request_count.saturating_add(chunks.len() as u64))
            as usize;
        if set_budget > 0 {
            let mut by_style: BTreeMap<SaRgbLitStyle, Vec<SaId>> = BTreeMap::new();
            for (&sa, &style) in &self.desired {
                if self.confirmed.get(&sa) != Some(&style) || self.uncertain_dirty.contains(&sa) {
                    by_style.entry(style).or_default().push(sa);
                }
            }
            let mut used = 0usize;
            'styles: for (style, ids) in by_style {
                for chunk in ids.chunks(MAX_SA_IDS_PER_UPDATE) {
                    if used >= set_budget {
                        break 'styles;
                    }
                    used += 1;
                    chunks.push(SaRgbBatchChunk {
                        update: style
                            .to_sa_rgb_update(chunk.iter().copied().map(SaId::get).collect()),
                        kind: SaRgbChunkKind::Set {
                            style,
                            ids: chunk.to_vec(),
                        },
                    });
                }
            }
        }
        (!chunks.is_empty()).then_some(DispatchedSaRgbBatch {
            batch_id: 0,
            chunks,
        })
    }
}

#[path = "v070_sa_rgb_coordinator.rs"]
mod coordinator;
pub(crate) use coordinator::SaRgbRuntimeCoordinator;

#[cfg(test)]
#[path = "v070_session_sa_rgb_tests.rs"]
mod fw6_tests;
