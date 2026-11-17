//! Pure state machine for direct-turn, short-press, and long-press (`ENC_CLK`)
//! target-app model-picker and search overlay navigation.

use mi_model::host_router::ResolvedHostAction;
use std::time::{Duration, Instant};

pub const ENCODER_HOLD_THRESHOLD: Duration = Duration::from_millis(350);
pub const MODEL_PICKER_IDLE_TIMEOUT: Duration = Duration::from_secs(12);
pub const MOUSE_FLICK_DX_THRESHOLD: f64 = 28.0;
pub const MOUSE_FLICK_COOLDOWN: Duration = Duration::from_millis(220);
pub const MOUSE_FLICK_IDLE_RESET: Duration = Duration::from_millis(180);
pub const MOUSE_FLICK_HORIZONTAL_DOMINANCE: f64 = 1.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncoderOverlayKind {
    ModelPicker,
    SearchConversations,
}

impl EncoderOverlayKind {
    pub const fn captures_mouse(self) -> bool {
        false
    }

    pub const fn captures_sectors(self) -> bool {
        false
    }

    pub fn from_action(action: &ResolvedHostAction) -> Option<Self> {
        match action {
            ResolvedHostAction::AgentCommand { command, .. } => match command.as_str() {
                "model" | "model-picker-open" | "model-picker-prev" | "model-picker-next" => {
                    Some(Self::ModelPicker)
                }
                "search" => Some(Self::SearchConversations),
                _ => None,
            },
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncoderHoldState {
    pub session_id: u64,
    pub pressed_at: Instant,
    pub hold_generation: u64,
    pub turned_while_pressed: bool,
    pub hold_fired: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveModelPickerSession {
    pub adapter: String,
    pub kind: EncoderOverlayKind,
    pub opened_at: Instant,
    pub last_activity_at: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelPickerCommand {
    ModelPickerOpen,
    ModelPickerPrev,
    ModelPickerNext,
    ModelPickerConfirm,
    EffortRight,
    EffortLeft,
    ModelPickerCancel,
    SearchPrev,
    SearchNext,
    SearchConfirm,
}

impl ModelPickerCommand {
    pub const fn as_command_str(self) -> &'static str {
        match self {
            Self::ModelPickerOpen => "model-picker-open",
            Self::ModelPickerPrev => "model-picker-prev",
            Self::ModelPickerNext => "model-picker-next",
            Self::ModelPickerConfirm => "model-picker-confirm",
            Self::EffortRight => "model-picker-effort-right",
            Self::EffortLeft => "model-picker-effort-left",
            Self::ModelPickerCancel => "model-picker-cancel",
            Self::SearchPrev => "search-prev",
            Self::SearchNext => "search-next",
            Self::SearchConfirm => "search-confirm",
        }
    }

    pub fn into_host_action(self, adapter: impl Into<String>) -> ResolvedHostAction {
        ResolvedHostAction::AgentCommand {
            adapter: adapter.into(),
            command: self.as_command_str().to_string(),
        }
    }

    pub fn is_open_model_picker_action(action: &ResolvedHostAction) -> bool {
        matches!(
            action,
            ResolvedHostAction::AgentCommand { command, .. }
                if matches!(command.as_str(), "model" | "model-picker-open")
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EncoderHoldTimerOutcome {
    Ignored,
    FiredHold {
        adapter: Option<String>,
        overlay_kind: Option<EncoderOverlayKind>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncoderTurnOutcome {
    NormalSlot,
    FastScroll { clockwise: bool },
    ModelPicker(ModelPickerCommand),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EncoderReleaseOutcome {
    Consumed,
    ShortPress,
    FiredHold {
        adapter: Option<String>,
        overlay_kind: Option<EncoderOverlayKind>,
    },
    ModelPickerConfirm {
        adapter: String,
        command: ModelPickerCommand,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayTransition {
    None,
    Entered(EncoderOverlayKind),
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmExecutionDisposition {
    Completed,
    FallbackToShortPress,
}

#[derive(Clone, Debug, Default)]
struct MouseFlickState {
    acc_dx: f64,
    acc_dy: f64,
    last_delta_at: Option<Instant>,
    last_flick_at: Option<Instant>,
}

impl MouseFlickState {
    fn reset_accumulators(&mut self) {
        self.acc_dx = 0.0;
        self.acc_dy = 0.0;
    }

    fn clear(&mut self) {
        *self = Self::default();
    }
}

#[derive(Clone, Debug, Default)]
pub struct EncoderModelPicker {
    next_hold_generation: u64,
    hold: Option<EncoderHoldState>,
    active: Option<ActiveModelPickerSession>,
    flick: MouseFlickState,
}

pub type EncoderModelPickerState = EncoderModelPicker;

impl EncoderModelPicker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn hold_state(&self) -> Option<&EncoderHoldState> {
        self.hold.as_ref()
    }

    pub fn is_encoder_pressed(&self) -> bool {
        self.hold.is_some()
    }

    pub fn turned_while_pressed(&self) -> bool {
        self.hold
            .as_ref()
            .is_some_and(|hold| hold.turned_while_pressed)
    }

    pub fn active_session(&self) -> Option<&ActiveModelPickerSession> {
        self.active.as_ref()
    }

    pub fn is_active(&self, now: Instant) -> bool {
        self.active.as_ref().is_some_and(|session| {
            now.saturating_duration_since(session.last_activity_at) <= MODEL_PICKER_IDLE_TIMEOUT
        })
    }

    pub fn active_adapter(&self, now: Instant) -> Option<&str> {
        self.is_active(now)
            .then(|| self.active.as_ref().map(|s| s.adapter.as_str()))
            .flatten()
    }

    pub fn active_kind(&self, now: Instant) -> Option<EncoderOverlayKind> {
        self.is_active(now)
            .then(|| self.active.as_ref().map(|s| s.kind))
            .flatten()
    }

    pub fn expire_if_idle(&mut self, now: Instant) -> Option<ActiveModelPickerSession> {
        if self.active.as_ref().is_some_and(|session| {
            now.saturating_duration_since(session.last_activity_at) > MODEL_PICKER_IDLE_TIMEOUT
        }) {
            self.flick.clear();
            return self.active.take();
        }
        None
    }

    pub fn on_encoder_press(&mut self, session_id: u64, now: Instant) -> u64 {
        self.expire_if_idle(now);
        self.next_hold_generation = self.next_hold_generation.wrapping_add(1).max(1);
        let generation = self.next_hold_generation;
        self.hold = Some(EncoderHoldState {
            session_id,
            pressed_at: now,
            hold_generation: generation,
            turned_while_pressed: false,
            hold_fired: false,
        });
        generation
    }

    pub fn prepare_for_hold_action(&mut self, next_kind: Option<EncoderOverlayKind>) -> bool {
        let was_model_picker = self
            .active
            .as_ref()
            .is_some_and(|s| s.kind == EncoderOverlayKind::ModelPicker);
        let entering_search = next_kind == Some(EncoderOverlayKind::SearchConversations);
        if self.active.is_some() {
            self.active = None;
            self.flick.clear();
        }
        was_model_picker && entering_search
    }

    pub fn apply_hold_execution_outcome(
        &mut self,
        adapter: impl Into<String>,
        kind: Option<EncoderOverlayKind>,
        succeeded: bool,
        now: Instant,
    ) -> OverlayTransition {
        if succeeded {
            if let Some(kind) = kind {
                self.active = Some(ActiveModelPickerSession {
                    adapter: adapter.into(),
                    kind,
                    opened_at: now,
                    last_activity_at: now,
                });
                self.flick.clear();
                return OverlayTransition::Entered(kind);
            }
            OverlayTransition::None
        } else {
            self.cancel();
            OverlayTransition::Cancelled
        }
    }

    pub fn apply_confirm_execution_outcome(
        &mut self,
        succeeded: bool,
        detail: &str,
    ) -> ConfirmExecutionDisposition {
        self.cancel();
        if succeeded && detail.ends_with("/model-picker-already-closed") {
            ConfirmExecutionDisposition::FallbackToShortPress
        } else {
            ConfirmExecutionDisposition::Completed
        }
    }

    pub fn activate_overlay_after_action(
        &mut self,
        adapter: impl Into<String>,
        action: &ResolvedHostAction,
        now: Instant,
    ) -> OverlayTransition {
        if matches!(action, ResolvedHostAction::Shortcut { key_code: 0x35, .. })
            || matches!(
                action,
                ResolvedHostAction::AgentCommand { command, .. }
                    if command == "model-picker-cancel"
            )
        {
            self.cancel();
            return OverlayTransition::Cancelled;
        }
        if let Some(kind) = EncoderOverlayKind::from_action(action) {
            self.active = Some(ActiveModelPickerSession {
                adapter: adapter.into(),
                kind,
                opened_at: now,
                last_activity_at: now,
            });
            self.flick.clear();
            return OverlayTransition::Entered(kind);
        }
        OverlayTransition::None
    }

    pub fn on_hold_timer(
        &mut self,
        hold_generation: u64,
        adapter: Option<&str>,
        has_hold_action: bool,
        now: Instant,
    ) -> EncoderHoldTimerOutcome {
        let target_kind = if has_hold_action && adapter.is_some_and(|a| !a.trim().is_empty()) {
            Some(EncoderOverlayKind::ModelPicker)
        } else {
            None
        };
        self.on_hold_timer_with_picker_mode(
            hold_generation,
            adapter,
            has_hold_action,
            target_kind,
            now,
        )
    }

    pub fn on_hold_timer_with_picker_mode(
        &mut self,
        hold_generation: u64,
        adapter: Option<&str>,
        has_hold_action: bool,
        target_kind: Option<EncoderOverlayKind>,
        now: Instant,
    ) -> EncoderHoldTimerOutcome {
        self.expire_if_idle(now);
        let Some(hold) = self.hold.as_mut() else {
            return EncoderHoldTimerOutcome::Ignored;
        };
        if hold.hold_generation != hold_generation
            || hold.turned_while_pressed
            || hold.hold_fired
            || !has_hold_action
            || now.saturating_duration_since(hold.pressed_at) < ENCODER_HOLD_THRESHOLD
        {
            return EncoderHoldTimerOutcome::Ignored;
        }
        hold.hold_fired = true;
        let normalized_adapter = adapter
            .map(str::trim)
            .filter(|a| !a.is_empty())
            .map(str::to_owned);
        let overlay_kind = if target_kind.is_some() && normalized_adapter.is_some() {
            target_kind
        } else {
            None
        };
        EncoderHoldTimerOutcome::FiredHold {
            adapter: normalized_adapter,
            overlay_kind,
        }
    }

    pub fn on_encoder_turn(&mut self, clockwise: bool, now: Instant) -> EncoderTurnOutcome {
        self.expire_if_idle(now);
        if let Some(active) = self.active.as_mut() {
            if let Some(hold) = self.hold.as_mut() {
                hold.turned_while_pressed = true;
            }
            active.last_activity_at = now;
            let cmd = match (active.kind, clockwise) {
                (EncoderOverlayKind::SearchConversations, true) => ModelPickerCommand::SearchNext,
                (EncoderOverlayKind::SearchConversations, false) => ModelPickerCommand::SearchPrev,
                (EncoderOverlayKind::ModelPicker, true) => ModelPickerCommand::ModelPickerNext,
                (EncoderOverlayKind::ModelPicker, false) => ModelPickerCommand::ModelPickerPrev,
            };
            return EncoderTurnOutcome::ModelPicker(cmd);
        }
        if let Some(hold) = self.hold.as_mut() {
            if !hold.hold_fired {
                hold.turned_while_pressed = true;
                return EncoderTurnOutcome::FastScroll { clockwise };
            }
        }
        EncoderTurnOutcome::NormalSlot
    }

    pub fn on_encoder_release(
        &mut self,
        session_id: u64,
        adapter: Option<&str>,
        has_hold_action: bool,
        now: Instant,
    ) -> EncoderReleaseOutcome {
        let target_kind = if has_hold_action && adapter.is_some_and(|a| !a.trim().is_empty()) {
            Some(EncoderOverlayKind::ModelPicker)
        } else {
            None
        };
        self.on_encoder_release_with_picker_mode(
            session_id,
            adapter,
            has_hold_action,
            target_kind,
            now,
        )
    }

    pub fn on_encoder_release_with_picker_mode(
        &mut self,
        session_id: u64,
        adapter: Option<&str>,
        has_hold_action: bool,
        target_kind: Option<EncoderOverlayKind>,
        now: Instant,
    ) -> EncoderReleaseOutcome {
        self.expire_if_idle(now);
        let Some(current) = self.hold.as_ref() else {
            return EncoderReleaseOutcome::Consumed;
        };
        if current.session_id != session_id {
            return EncoderReleaseOutcome::Consumed;
        }
        let hold = self.hold.take().unwrap();
        if hold.hold_fired || hold.turned_while_pressed {
            return EncoderReleaseOutcome::Consumed;
        }
        let elapsed = now.saturating_duration_since(hold.pressed_at);
        if has_hold_action && elapsed >= ENCODER_HOLD_THRESHOLD {
            let normalized_adapter = adapter
                .map(str::trim)
                .filter(|a| !a.is_empty())
                .map(str::to_owned);
            let overlay_kind = if target_kind.is_some() && normalized_adapter.is_some() {
                target_kind
            } else {
                None
            };
            return EncoderReleaseOutcome::FiredHold {
                adapter: normalized_adapter,
                overlay_kind,
            };
        }
        if let Some(active) = self.active.take() {
            self.flick.clear();
            let command = match active.kind {
                EncoderOverlayKind::ModelPicker => ModelPickerCommand::ModelPickerConfirm,
                EncoderOverlayKind::SearchConversations => ModelPickerCommand::SearchConfirm,
            };
            return EncoderReleaseOutcome::ModelPickerConfirm {
                adapter: active.adapter,
                command,
            };
        }
        EncoderReleaseOutcome::ShortPress
    }

    pub fn on_mouse_delta(&mut self, dx: f64, dy: f64, now: Instant) -> Option<ModelPickerCommand> {
        self.expire_if_idle(now);
        let active = self.active.as_mut()?;
        if !active.kind.captures_mouse() {
            return None;
        }
        if !dx.is_finite() || !dy.is_finite() || (dx == 0.0 && dy == 0.0) {
            return None;
        }
        if self
            .flick
            .last_flick_at
            .is_some_and(|last| now.saturating_duration_since(last) < MOUSE_FLICK_COOLDOWN)
        {
            self.flick.reset_accumulators();
            self.flick.last_delta_at = Some(now);
            return None;
        }
        if self
            .flick
            .last_delta_at
            .is_some_and(|last| now.saturating_duration_since(last) > MOUSE_FLICK_IDLE_RESET)
        {
            self.flick.reset_accumulators();
        }
        if (self.flick.acc_dx > 0.0 && dx < 0.0) || (self.flick.acc_dx < 0.0 && dx > 0.0) {
            self.flick.reset_accumulators();
        }
        self.flick.acc_dx += dx;
        self.flick.acc_dy += dy.abs();
        self.flick.last_delta_at = Some(now);

        if dy.abs() > dx.abs() && self.flick.acc_dy > self.flick.acc_dx.abs() {
            self.flick.reset_accumulators();
            return None;
        }

        let abs_dx = self.flick.acc_dx.abs();
        if abs_dx >= MOUSE_FLICK_DX_THRESHOLD
            && abs_dx >= MOUSE_FLICK_HORIZONTAL_DOMINANCE * self.flick.acc_dy
        {
            let cmd = if self.flick.acc_dx > 0.0 {
                ModelPickerCommand::EffortRight
            } else {
                ModelPickerCommand::EffortLeft
            };
            self.flick.reset_accumulators();
            self.flick.last_flick_at = Some(now);
            active.last_activity_at = now;
            return Some(cmd);
        }
        None
    }

    pub fn on_sector_direction_while_active(
        &mut self,
        binding_id: &str,
        now: Instant,
    ) -> Option<ModelPickerCommand> {
        self.expire_if_idle(now);
        let active = self.active.as_mut()?;
        if !active.kind.captures_sectors() {
            return None;
        }
        let cmd = match binding_id {
            "AG18" => ModelPickerCommand::EffortRight,
            "AG16" => ModelPickerCommand::EffortLeft,
            "AG15" => ModelPickerCommand::ModelPickerPrev,
            "AG17" => ModelPickerCommand::ModelPickerNext,
            _ => return None,
        };
        active.last_activity_at = now;
        Some(cmd)
    }

    pub fn enter_active_session(
        &mut self,
        adapter: impl Into<String>,
        kind: EncoderOverlayKind,
        now: Instant,
    ) {
        self.active = Some(ActiveModelPickerSession {
            adapter: adapter.into(),
            kind,
            opened_at: now,
            last_activity_at: now,
        });
        self.flick.clear();
    }

    pub fn cancel(&mut self) -> Option<ActiveModelPickerSession> {
        self.hold = None;
        self.flick.clear();
        self.active.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hold_timer_fires_while_still_held_and_consumes_subsequent_release_without_short_press() {
        let mut picker = EncoderModelPicker::new();
        let t0 = Instant::now();
        let gen = picker.on_encoder_press(7, t0);
        assert_eq!(gen, 1);

        // Before 350ms threshold: timer is a no-op.
        assert_eq!(
            picker.on_hold_timer(
                gen,
                Some("antigravity"),
                true,
                t0 + Duration::from_millis(349)
            ),
            EncoderHoldTimerOutcome::Ignored
        );
        assert!(!picker.is_active(t0 + Duration::from_millis(349)));

        // Stale generation is ignored.
        assert_eq!(
            picker.on_hold_timer(
                gen + 1,
                Some("antigravity"),
                true,
                t0 + Duration::from_millis(350)
            ),
            EncoderHoldTimerOutcome::Ignored
        );

        // At 350ms while still held: fires hold immediately and enters Active.
        assert_eq!(
            picker.on_hold_timer(
                gen,
                Some("antigravity"),
                true,
                t0 + Duration::from_millis(350)
            ),
            EncoderHoldTimerOutcome::FiredHold {
                adapter: Some("antigravity".into()),
                overlay_kind: Some(EncoderOverlayKind::ModelPicker),
            }
        );
        picker.apply_hold_execution_outcome(
            "antigravity",
            Some(EncoderOverlayKind::ModelPicker),
            true,
            t0 + Duration::from_millis(350),
        );
        assert!(picker.is_active(t0 + Duration::from_millis(350)));
        assert_eq!(
            picker.active_adapter(t0 + Duration::from_millis(350)),
            Some("antigravity")
        );
        assert_eq!(
            picker.active_kind(t0 + Duration::from_millis(350)),
            Some(EncoderOverlayKind::ModelPicker)
        );

        // Duplicate timer wakeup for same hold generation is ignored.
        assert_eq!(
            picker.on_hold_timer(
                gen,
                Some("antigravity"),
                true,
                t0 + Duration::from_millis(400)
            ),
            EncoderHoldTimerOutcome::Ignored
        );

        // Turning while still held AFTER hold fired navigates the newly opened picker!
        assert_eq!(
            picker.on_encoder_turn(true, t0 + Duration::from_millis(420)),
            EncoderTurnOutcome::ModelPicker(ModelPickerCommand::ModelPickerNext)
        );

        // Releasing after hold_fired is consumed with zero short-press fallback, keeping Active open.
        assert_eq!(
            picker.on_encoder_release(
                7,
                Some("antigravity"),
                true,
                t0 + Duration::from_millis(600)
            ),
            EncoderReleaseOutcome::Consumed
        );
        assert!(picker.is_active(t0 + Duration::from_millis(600)));
    }

    #[test]
    fn turn_before_hold_threshold_cancels_hold_into_fast_scroll_and_consumes_release() {
        let mut picker = EncoderModelPicker::new();
        let t0 = Instant::now();
        let gen = picker.on_encoder_press(3, t0);

        assert_eq!(
            picker.on_encoder_turn(false, t0 + Duration::from_millis(120)),
            EncoderTurnOutcome::FastScroll { clockwise: false }
        );
        assert!(picker.turned_while_pressed());

        // Timer arriving after 350ms must NOT fire hold because user turned while pressed.
        assert_eq!(
            picker.on_hold_timer(gen, Some("codex"), true, t0 + Duration::from_millis(380)),
            EncoderHoldTimerOutcome::Ignored
        );
        assert!(!picker.is_active(t0 + Duration::from_millis(380)));

        // Release after push+turn is consumed (no short-press submit, no hold).
        assert_eq!(
            picker.on_encoder_release(3, Some("codex"), true, t0 + Duration::from_millis(500)),
            EncoderReleaseOutcome::Consumed
        );
        assert!(!picker.is_encoder_pressed());
    }

    #[test]
    fn synchronous_release_fallback_and_short_tap_confirm_or_slot_dispatch() {
        let mut picker = EncoderModelPicker::new();
        let t0 = Instant::now();

        // Normal short tap (< 350ms) when inactive -> ShortPress (slot 15).
        picker.on_encoder_press(1, t0);
        assert_eq!(
            picker.on_encoder_release(1, Some("codex"), true, t0 + Duration::from_millis(120)),
            EncoderReleaseOutcome::ShortPress
        );

        // Synchronous >= 350ms release without async timer wakeup -> fires hold & enters Active upon outcome.
        let t1 = t0 + Duration::from_secs(1);
        picker.on_encoder_press(1, t1);
        assert_eq!(
            picker.on_encoder_release(1, Some("codex"), true, t1 + Duration::from_millis(360)),
            EncoderReleaseOutcome::FiredHold {
                adapter: Some("codex".into()),
                overlay_kind: Some(EncoderOverlayKind::ModelPicker),
            }
        );
        picker.apply_hold_execution_outcome(
            "codex",
            Some(EncoderOverlayKind::ModelPicker),
            true,
            t1 + Duration::from_millis(360),
        );
        assert!(picker.is_active(t1 + Duration::from_millis(360)));

        // CCW / CW turns while Active route to ModelPickerPrev / ModelPickerNext and refresh activity.
        let t_turn = t1 + Duration::from_secs(8);
        assert_eq!(
            picker.on_encoder_turn(false, t_turn),
            EncoderTurnOutcome::ModelPicker(ModelPickerCommand::ModelPickerPrev)
        );
        assert_eq!(
            picker.on_encoder_turn(true, t_turn + Duration::from_secs(8)),
            EncoderTurnOutcome::ModelPicker(ModelPickerCommand::ModelPickerNext)
        );

        // Short tap (< 350ms) while Active -> exits Active and emits ModelPickerConfirm.
        let t_tap = t_turn + Duration::from_secs(9);
        picker.on_encoder_press(1, t_tap);
        assert_eq!(
            picker.on_encoder_release(1, Some("codex"), true, t_tap + Duration::from_millis(90)),
            EncoderReleaseOutcome::ModelPickerConfirm {
                adapter: "codex".into(),
                command: ModelPickerCommand::ModelPickerConfirm,
            }
        );
        assert!(!picker.is_active(t_tap + Duration::from_millis(90)));

        // Next tap when no longer Active -> normal ShortPress again.
        let t_after = t_tap + Duration::from_secs(1);
        picker.on_encoder_press(1, t_after);
        assert_eq!(
            picker.on_encoder_release(1, Some("codex"), true, t_after + Duration::from_millis(80)),
            EncoderReleaseOutcome::ShortPress
        );
    }

    #[test]
    fn mouse_deltas_never_intercept_hardware_mouse_pointer_even_while_model_picker_active() {
        let mut picker = EncoderModelPicker::new();
        let t0 = Instant::now();

        assert!(!EncoderOverlayKind::ModelPicker.captures_mouse());
        assert!(!EncoderOverlayKind::SearchConversations.captures_mouse());
        assert_eq!(picker.on_mouse_delta(40.0, 0.0, t0), None);

        let gen = picker.on_encoder_press(9, t0);
        picker.on_hold_timer(
            gen,
            Some("antigravity"),
            true,
            t0 + Duration::from_millis(350),
        );
        picker.apply_hold_execution_outcome(
            "antigravity",
            Some(EncoderOverlayKind::ModelPicker),
            true,
            t0 + Duration::from_millis(350),
        );

        let mut now = t0 + Duration::from_millis(400);
        for (dx, dy) in [(15.0, 2.0), (25.0, -2.0), (-40.0, 0.0), (50.0, 0.0)] {
            now += Duration::from_millis(16);
            assert_eq!(picker.on_mouse_delta(dx, dy, now), None);
        }
    }

    #[test]
    fn sector_directions_idle_timeout_and_cancel_behave_deterministically() {
        let mut picker = EncoderModelPicker::new();
        let t0 = Instant::now();
        assert!(!EncoderOverlayKind::ModelPicker.captures_sectors());
        assert!(!EncoderOverlayKind::SearchConversations.captures_sectors());

        let gen = picker.on_encoder_press(5, t0);
        picker.on_hold_timer(
            gen,
            Some("dsh-desktop"),
            true,
            t0 + Duration::from_millis(350),
        );
        picker.apply_hold_execution_outcome(
            "dsh-desktop",
            Some(EncoderOverlayKind::ModelPicker),
            true,
            t0 + Duration::from_millis(350),
        );
        picker.on_encoder_release(
            5,
            Some("dsh-desktop"),
            true,
            t0 + Duration::from_millis(400),
        );

        let t1 = t0 + Duration::from_secs(1);
        for (idx, binding) in ["AG15", "AG17", "AG18", "AG16", "AG00"]
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                picker.on_sector_direction_while_active(
                    binding,
                    t1 + Duration::from_millis(idx as u64 * 50)
                ),
                None
            );
        }

        // Idle timeout (> 12s after last activity) expires Active automatically.
        let t_expired =
            t0 + Duration::from_millis(350) + Duration::from_secs(12) + Duration::from_millis(1);
        assert_eq!(
            picker.on_encoder_turn(true, t_expired),
            EncoderTurnOutcome::NormalSlot
        );
        assert!(!picker.is_active(t_expired));

        // Explicit cancel clears both hold and active session.
        let gen2 = picker.on_encoder_press(5, t_expired);
        picker.on_hold_timer(
            gen2,
            Some("dsh-desktop"),
            true,
            t_expired + Duration::from_millis(350),
        );
        picker.apply_hold_execution_outcome(
            "dsh-desktop",
            Some(EncoderOverlayKind::ModelPicker),
            true,
            t_expired + Duration::from_millis(350),
        );
        assert!(picker.cancel().is_some());
        assert!(!picker.is_active(t_expired + Duration::from_millis(350)));
        assert!(!picker.is_encoder_pressed());
    }

    #[test]
    fn search_conversations_overlay_disables_mouse_and_sectors_but_routes_turns_and_confirm() {
        let mut picker = EncoderModelPicker::new();
        let t0 = Instant::now();

        picker.enter_active_session("antigravity", EncoderOverlayKind::SearchConversations, t0);
        assert!(picker.is_active(t0));
        assert_eq!(
            picker.active_kind(t0),
            Some(EncoderOverlayKind::SearchConversations)
        );

        // Mouse flicks are strictly disabled in SearchConversations
        assert_eq!(
            picker.on_mouse_delta(50.0, 0.0, t0 + Duration::from_millis(10)),
            None
        );

        // Sectors are strictly disabled in SearchConversations
        assert_eq!(
            picker.on_sector_direction_while_active("AG15", t0 + Duration::from_millis(20)),
            None
        );
        assert_eq!(
            picker.on_sector_direction_while_active("AG18", t0 + Duration::from_millis(30)),
            None
        );

        assert_eq!(
            picker.on_encoder_turn(false, t0 + Duration::from_millis(50)),
            EncoderTurnOutcome::ModelPicker(ModelPickerCommand::SearchPrev)
        );
        assert_eq!(
            picker.on_encoder_turn(true, t0 + Duration::from_millis(60)),
            EncoderTurnOutcome::ModelPicker(ModelPickerCommand::SearchNext)
        );

        picker.on_encoder_press(2, t0 + Duration::from_millis(100));
        assert_eq!(
            picker.on_encoder_turn(true, t0 + Duration::from_millis(120)),
            EncoderTurnOutcome::ModelPicker(ModelPickerCommand::SearchNext)
        );
        assert_eq!(
            picker.on_encoder_release(
                2,
                Some("antigravity"),
                true,
                t0 + Duration::from_millis(130)
            ),
            EncoderReleaseOutcome::Consumed
        );

        picker.on_encoder_press(3, t0 + Duration::from_millis(140));
        assert_eq!(
            picker.on_encoder_release(
                3,
                Some("antigravity"),
                true,
                t0 + Duration::from_millis(150)
            ),
            EncoderReleaseOutcome::ModelPickerConfirm {
                adapter: "antigravity".into(),
                command: ModelPickerCommand::SearchConfirm,
            }
        );
        assert!(!picker.is_active(t0 + Duration::from_millis(150)));
    }

    #[test]
    fn model_picker_to_search_long_hold_transition_signals_escape_and_resets_capture() {
        let mut picker = EncoderModelPicker::new();
        let t0 = Instant::now();

        picker.enter_active_session("antigravity", EncoderOverlayKind::ModelPicker, t0);
        assert!(picker.is_active(t0));

        let search_action = ResolvedHostAction::AgentCommand {
            adapter: "antigravity".into(),
            command: "search".into(),
        };
        let next_kind = EncoderOverlayKind::from_action(&search_action);
        assert_eq!(next_kind, Some(EncoderOverlayKind::SearchConversations));

        // Transition from ModelPicker to Search requires sending Esc first
        let must_esc = picker.prepare_for_hold_action(next_kind);
        assert!(must_esc);
        assert!(!picker.is_active(t0));

        // Successful execution enters SearchConversations
        let transition = picker.apply_hold_execution_outcome(
            "antigravity",
            next_kind,
            true,
            t0 + Duration::from_millis(50),
        );
        assert_eq!(
            transition,
            OverlayTransition::Entered(EncoderOverlayKind::SearchConversations)
        );
        assert!(picker.is_active(t0 + Duration::from_millis(50)));
        assert_eq!(
            picker.active_kind(t0 + Duration::from_millis(50)),
            Some(EncoderOverlayKind::SearchConversations)
        );
    }

    #[test]
    fn idle_turn_actions_activate_model_picker_and_reopened_confirm_restores_session() {
        let mut picker = EncoderModelPicker::new();
        let t0 = Instant::now();

        for cmd in ["model-picker-prev", "model-picker-next"] {
            picker.cancel();
            let action = ResolvedHostAction::AgentCommand {
                adapter: "antigravity".into(),
                command: cmd.into(),
            };
            assert_eq!(
                picker.on_encoder_turn(true, t0),
                EncoderTurnOutcome::NormalSlot
            );
            let transition = picker.activate_overlay_after_action("antigravity", &action, t0);
            assert_eq!(
                transition,
                OverlayTransition::Entered(EncoderOverlayKind::ModelPicker)
            );
            assert!(picker.is_active(t0));
            assert_eq!(
                picker.on_mouse_delta(32.0, 0.0, t0 + Duration::from_millis(20)),
                None
            );
        }

        picker.on_encoder_press(10, t0 + Duration::from_millis(60));
        assert_eq!(
            picker.on_encoder_release(
                10,
                Some("antigravity"),
                true,
                t0 + Duration::from_millis(100)
            ),
            EncoderReleaseOutcome::ModelPickerConfirm {
                adapter: "antigravity".into(),
                command: ModelPickerCommand::ModelPickerConfirm,
            }
        );
        assert!(!picker.is_active(t0 + Duration::from_millis(100)));

        let fallback =
            picker.apply_confirm_execution_outcome(true, "antigravity/model-picker-already-closed");
        assert_eq!(fallback, ConfirmExecutionDisposition::FallbackToShortPress);
        assert!(!picker.is_active(t0 + Duration::from_millis(105)));

        picker.enter_active_session(
            "antigravity",
            EncoderOverlayKind::ModelPicker,
            t0 + Duration::from_millis(120),
        );
        picker.on_encoder_press(11, t0 + Duration::from_millis(150));
        assert_eq!(
            picker.on_encoder_release(
                11,
                Some("antigravity"),
                true,
                t0 + Duration::from_millis(190)
            ),
            EncoderReleaseOutcome::ModelPickerConfirm {
                adapter: "antigravity".into(),
                command: ModelPickerCommand::ModelPickerConfirm,
            }
        );
        let closed =
            picker.apply_confirm_execution_outcome(true, "antigravity/model-picker-confirm");
        assert_eq!(closed, ConfirmExecutionDisposition::Completed);
        assert!(!picker.is_active(t0 + Duration::from_millis(195)));
    }
}
