use mi_core::radial::{
    resolve_radial_slots, RadialConfirmMode, RadialMenuSession, RadialMenuState, RadialPhase,
    RadialPositionSample, RadialPresetKind, RadialRouteStamp, RadialSlotConfig,
    RadialSlotResolutionContext, SectorIndex,
};
use mi_macos::ScreenRect;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ExploreStats {
    states_explored: usize,
    transitions_checked: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TraceStep<S, A, O> {
    action: A,
    before: S,
    after: S,
    outcome: O,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CounterexampleTrace<S, A, O> {
    invariant: &'static str,
    detail: String,
    steps: Vec<TraceStep<S, A, O>>,
}

impl<S, A: Copy, O> CounterexampleTrace<S, A, O> {
    fn actions(&self) -> Vec<A> {
        self.steps.iter().map(|step| step.action).collect()
    }
}

fn reconstruct_trace<S, A, O>(
    parents: &BTreeMap<S, TraceStep<S, A, O>>,
    mut cursor: S,
    final_step: TraceStep<S, A, O>,
) -> Vec<TraceStep<S, A, O>>
where
    S: Copy + Ord,
    A: Copy,
    O: Copy,
{
    let mut reversed = vec![final_step];
    while let Some(step) = parents.get(&cursor) {
        reversed.push(step.clone());
        cursor = step.before;
    }
    reversed.reverse();
    reversed
}

fn explore_bfs<S, A, O, FStep, FInv>(
    initial: S,
    actions: &[A],
    step_fn: FStep,
    invariant_fn: FInv,
) -> Result<ExploreStats, CounterexampleTrace<S, A, O>>
where
    S: Copy + Ord,
    A: Copy,
    O: Copy,
    FStep: Fn(S, A) -> (S, O),
    FInv: Fn(S, A, S, O) -> Result<(), (&'static str, String)>,
{
    let mut visited: BTreeSet<S> = BTreeSet::new();
    let mut parents: BTreeMap<S, TraceStep<S, A, O>> = BTreeMap::new();
    let mut queue: VecDeque<S> = VecDeque::new();
    let mut transitions_checked: usize = 0;

    visited.insert(initial);
    queue.push_back(initial);

    while let Some(current) = queue.pop_front() {
        for &action in actions {
            transitions_checked += 1;
            let (next, outcome) = step_fn(current, action);
            let trace_step = TraceStep {
                action,
                before: current,
                after: next,
                outcome,
            };
            if let Err((invariant, detail)) = invariant_fn(current, action, next, outcome) {
                return Err(CounterexampleTrace {
                    invariant,
                    detail,
                    steps: reconstruct_trace(&parents, current, trace_step),
                });
            }
            if visited.insert(next) {
                parents.insert(next, trace_step);
                queue.push_back(next);
            }
        }
    }

    Ok(ExploreStats {
        states_explored: visited.len(),
        transitions_checked,
    })
}

mod vendor_bridge_model {
    use super::{explore_bfs, CounterexampleTrace, ExploreStats};

    const MAX_LEASE_EPOCH: u8 = 2;
    const LED_DOMAIN: u8 = 3;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub enum WireSyncState {
        Unsynced,
        InFlight { digest: u8, lease_epoch_before: u8 },
        Confirmed(u8),
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub enum PreviewPhase {
        Inactive,
        Active,
        Restored,
        Unknown,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub struct State {
        pub connected: bool,
        pub preview_lease_active: bool,
        pub preview_lease_epoch: u8,
        pub sync_state: WireSyncState,
        pub queued_digest: Option<u8>,
        pub deferred_led_flush: bool,
        pub hardware_led: u8,
        pub external_led_dirty: bool,
        pub preview_last_confirmed_led: Option<u8>,
        pub preview_original_led: Option<u8>,
        pub preview_phase: PreviewPhase,
    }

    impl State {
        pub fn initial() -> Self {
            Self {
                connected: false,
                preview_lease_active: false,
                preview_lease_epoch: 0,
                sync_state: WireSyncState::Unsynced,
                queued_digest: None,
                deferred_led_flush: false,
                hardware_led: 0,
                external_led_dirty: false,
                preview_last_confirmed_led: None,
                preview_original_led: None,
                preview_phase: PreviewPhase::Inactive,
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub enum Action {
        Connect,
        Disconnect,
        QueueDigest0,
        QueueDigest1,
        StartThstatusSend,
        ThstatusAckOk,
        ThstatusAckErr,
        AcquirePreviewLease,
        PreviewWriteLed,
        ExternalLedMutate,
        ReleasePreviewLeaseAndRestore,
        FlushDeferredLed,
    }

    pub const ALL_ACTIONS: [Action; 12] = [
        Action::Connect,
        Action::Disconnect,
        Action::QueueDigest0,
        Action::QueueDigest1,
        Action::StartThstatusSend,
        Action::ThstatusAckOk,
        Action::ThstatusAckErr,
        Action::AcquirePreviewLease,
        Action::ExternalLedMutate,
        Action::ReleasePreviewLeaseAndRestore,
        Action::PreviewWriteLed,
        Action::FlushDeferredLed,
    ];

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Policy {
        Fixed,
        HistoricalBuggyMissingLeaseEpochCheck,
        HistoricalBuggyTargetOnlyPreviewRestore,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct StepOutcome {
        pub flush_sent: bool,
        pub restore_wrote_hardware: bool,
    }

    impl StepOutcome {
        fn idle() -> Self {
            Self {
                flush_sent: false,
                restore_wrote_hardware: false,
            }
        }
    }

    pub fn step(state: State, action: Action, policy: Policy) -> (State, StepOutcome) {
        let mut next = state;
        let mut outcome = StepOutcome::idle();
        match action {
            Action::Connect => {
                if !next.connected {
                    next.connected = true;
                    next.sync_state = WireSyncState::Unsynced;
                    next.deferred_led_flush =
                        next.queued_digest.is_some() && next.preview_lease_active;
                }
            }
            Action::Disconnect => {
                if next.connected {
                    next.connected = false;
                    next.sync_state = WireSyncState::Unsynced;
                    next.deferred_led_flush = false;
                }
            }
            Action::QueueDigest0 => {
                next.queued_digest = Some(0);
                if next.connected && next.preview_lease_active {
                    next.deferred_led_flush = true;
                }
            }
            Action::QueueDigest1 => {
                next.queued_digest = Some(1);
                if next.connected && next.preview_lease_active {
                    next.deferred_led_flush = true;
                }
            }
            Action::StartThstatusSend => {
                if next.connected {
                    if let Some(digest) = next.queued_digest {
                        if next.preview_lease_active {
                            if !matches!(next.sync_state, WireSyncState::InFlight { .. }) {
                                next.sync_state = WireSyncState::Unsynced;
                            }
                            next.deferred_led_flush = true;
                        } else {
                            let should_send = match next.sync_state {
                                WireSyncState::Unsynced => true,
                                WireSyncState::Confirmed(confirmed) => confirmed != digest,
                                WireSyncState::InFlight { .. } => false,
                            };
                            if should_send {
                                next.sync_state = WireSyncState::InFlight {
                                    digest,
                                    lease_epoch_before: next.preview_lease_epoch,
                                };
                                next.deferred_led_flush = false;
                                outcome.flush_sent = true;
                            }
                        }
                    }
                }
            }
            Action::ThstatusAckOk => {
                if let WireSyncState::InFlight {
                    digest,
                    lease_epoch_before,
                } = next.sync_state
                {
                    let epoch_matches = !next.preview_lease_active
                        && next.preview_lease_epoch == lease_epoch_before;
                    if matches!(policy, Policy::HistoricalBuggyMissingLeaseEpochCheck)
                        || epoch_matches
                    {
                        next.sync_state = WireSyncState::Confirmed(digest);
                        if !next.preview_lease_active {
                            next.hardware_led = digest;
                            next.external_led_dirty = false;
                        }
                    } else {
                        next.sync_state = WireSyncState::Unsynced;
                        if next.queued_digest.is_some() {
                            next.deferred_led_flush = true;
                        }
                    }
                }
            }
            Action::ThstatusAckErr => {
                if matches!(next.sync_state, WireSyncState::InFlight { .. }) {
                    next.sync_state = WireSyncState::Unsynced;
                }
            }
            Action::AcquirePreviewLease => {
                if next.connected
                    && !next.preview_lease_active
                    && next.preview_lease_epoch < MAX_LEASE_EPOCH
                {
                    next.preview_lease_active = true;
                    next.preview_lease_epoch += 1;
                    next.preview_original_led = Some(next.hardware_led);
                    next.preview_last_confirmed_led = Some(next.hardware_led);
                    next.preview_phase = PreviewPhase::Active;
                    next.external_led_dirty = false;
                    if matches!(next.sync_state, WireSyncState::Confirmed(_)) {
                        next.sync_state = WireSyncState::Unsynced;
                    }
                }
            }
            Action::PreviewWriteLed => {
                if next.connected
                    && next.preview_lease_active
                    && next.preview_phase == PreviewPhase::Active
                {
                    let target_led = if next.hardware_led == 1 { 2 } else { 1 };
                    let cas_ok = !next.external_led_dirty
                        && next.preview_last_confirmed_led == Some(next.hardware_led);
                    if cas_ok || matches!(policy, Policy::HistoricalBuggyTargetOnlyPreviewRestore) {
                        next.hardware_led = target_led;
                        next.preview_last_confirmed_led = Some(target_led);
                    } else {
                        next.preview_phase = PreviewPhase::Unknown;
                    }
                }
            }
            Action::ExternalLedMutate => {
                next.hardware_led = (next.hardware_led + 1) % LED_DOMAIN;
                next.external_led_dirty = true;
            }
            Action::ReleasePreviewLeaseAndRestore => {
                if next.preview_lease_active {
                    if next.preview_phase == PreviewPhase::Active {
                        let cas_ok = next.connected
                            && !next.external_led_dirty
                            && next.preview_last_confirmed_led == Some(next.hardware_led);
                        let target_only_ok = next.connected
                            && matches!(policy, Policy::HistoricalBuggyTargetOnlyPreviewRestore);
                        if cas_ok || target_only_ok {
                            if let Some(original) = next.preview_original_led {
                                next.hardware_led = original;
                                outcome.restore_wrote_hardware = true;
                            }
                            next.preview_phase = PreviewPhase::Restored;
                            next.external_led_dirty = false;
                        } else {
                            next.preview_phase = PreviewPhase::Unknown;
                        }
                    }
                    next.preview_lease_active = false;
                    next.preview_last_confirmed_led = None;
                    next.preview_original_led = None;
                    if next.connected && next.queued_digest.is_some() {
                        next.deferred_led_flush = true;
                    }
                    if matches!(next.sync_state, WireSyncState::Confirmed(_)) {
                        next.sync_state = WireSyncState::Unsynced;
                    }
                }
            }
            Action::FlushDeferredLed => {
                if next.connected && !next.preview_lease_active && next.deferred_led_flush {
                    match (next.queued_digest, next.sync_state) {
                        (Some(digest), WireSyncState::Unsynced) => {
                            next.deferred_led_flush = false;
                            next.sync_state = WireSyncState::InFlight {
                                digest,
                                lease_epoch_before: next.preview_lease_epoch,
                            };
                            outcome.flush_sent = true;
                        }
                        (Some(digest), WireSyncState::Confirmed(confirmed)) => {
                            next.deferred_led_flush = false;
                            if confirmed != digest {
                                next.sync_state = WireSyncState::InFlight {
                                    digest,
                                    lease_epoch_before: next.preview_lease_epoch,
                                };
                                outcome.flush_sent = true;
                            }
                        }
                        (None, _) => {
                            next.deferred_led_flush = false;
                        }
                        (Some(_), WireSyncState::InFlight { .. }) => {}
                    }
                }
            }
        }
        (next, outcome)
    }

    pub fn check_invariants(
        before: State,
        action: Action,
        after: State,
        outcome: StepOutcome,
    ) -> Result<(), (&'static str, String)> {
        if action == Action::ThstatusAckOk {
            if let WireSyncState::InFlight {
                lease_epoch_before, ..
            } = before.sync_state
            {
                let stale =
                    before.preview_lease_active || lease_epoch_before != before.preview_lease_epoch;
                if stale && matches!(after.sync_state, WireSyncState::Confirmed(_)) {
                    return Err((
                        "no_stale_ack_confirmation_across_preview_lease",
                        format!(
                            "stale thstatus ACK transitioned to Confirmed: before={before:?}, after={after:?}"
                        ),
                    ));
                }
            }
        }

        if action == Action::ReleasePreviewLeaseAndRestore
            && before.preview_lease_active
            && before.preview_phase == PreviewPhase::Active
        {
            let externally_mutated = before.external_led_dirty
                || before.preview_last_confirmed_led != Some(before.hardware_led);
            if externally_mutated
                && (after.hardware_led != before.hardware_led
                    || after.preview_phase != PreviewPhase::Unknown
                    || outcome.restore_wrote_hardware)
            {
                return Err((
                    "external_led_mutation_never_overwritten_on_preview_restore",
                    format!(
                        "preview restore overwrote externally mutated LED or failed to report Unknown: before={before:?}, after={after:?}, outcome={outcome:?}"
                    ),
                ));
            }
        }

        if action == Action::PreviewWriteLed
            && before.connected
            && before.preview_lease_active
            && before.preview_phase == PreviewPhase::Active
        {
            let externally_mutated = before.external_led_dirty
                || before.preview_last_confirmed_led != Some(before.hardware_led);
            if externally_mutated
                && (after.hardware_led != before.hardware_led
                    || after.preview_phase != PreviewPhase::Unknown)
            {
                return Err((
                    "external_led_mutation_never_overwritten_on_preview_restore",
                    format!(
                        "preview update overwrote externally mutated LED: before={before:?}, after={after:?}"
                    ),
                ));
            }
        }

        if action == Action::FlushDeferredLed
            && before.connected
            && !before.preview_lease_active
            && before.deferred_led_flush
            && before.sync_state == WireSyncState::Unsynced
        {
            if let Some(digest) = before.queued_digest {
                let expected = WireSyncState::InFlight {
                    digest,
                    lease_epoch_before: before.preview_lease_epoch,
                };
                if !outcome.flush_sent || after.deferred_led_flush || after.sync_state != expected {
                    return Err((
                        "quiescent_flush_converges_after_preview_release",
                        format!(
                            "deferred flush was suppressed while Unsynced: before={before:?}, after={after:?}, outcome={outcome:?}"
                        ),
                    ));
                }
            }
        }

        Ok(())
    }

    pub fn run(
        policy: Policy,
    ) -> Result<ExploreStats, CounterexampleTrace<State, Action, StepOutcome>> {
        explore_bfs(
            State::initial(),
            &ALL_ACTIONS,
            |state, action| step(state, action, policy),
            check_invariants,
        )
    }
}

mod radial_model {
    use super::{explore_bfs, CounterexampleTrace, ExploreStats};

    const ROUTE_STAMP_DOMAIN: u8 = 2;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub enum Phase {
        Disabled,
        WaitingForCenter,
        Idle,
        Showing { route_stamp: u8 },
        Settling,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub struct State {
        pub phase: Phase,
        pub current_route_stamp: u8,
        pub gates_ok: bool,
        pub joystick_centered: bool,
        pub last_normal_enc_clk_fired: bool,
    }

    impl State {
        pub fn initial() -> Self {
            Self {
                phase: Phase::Disabled,
                current_route_stamp: 0,
                gates_ok: true,
                joystick_centered: true,
                last_normal_enc_clk_fired: false,
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub enum Action {
        EnableRadial,
        DisableRadial,
        ToggleGates,
        ChangeRouteStamp,
        JoystickReturnToCenter,
        JoystickMoveOffCenter,
        EncClk,
        EscapeKeyDown,
    }

    pub const ALL_ACTIONS: [Action; 8] = [
        Action::EnableRadial,
        Action::DisableRadial,
        Action::ChangeRouteStamp,
        Action::ToggleGates,
        Action::JoystickReturnToCenter,
        Action::JoystickMoveOffCenter,
        Action::EncClk,
        Action::EscapeKeyDown,
    ];

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Policy {
        Fixed,
        HistoricalBuggyRadialFallthrough,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct StepOutcome {
        pub consumed: bool,
    }

    pub fn step(state: State, action: Action, policy: Policy) -> (State, StepOutcome) {
        let mut next = state;
        next.last_normal_enc_clk_fired = false;
        let mut consumed = false;
        match action {
            Action::EnableRadial => {
                next.phase = Phase::WaitingForCenter;
            }
            Action::DisableRadial => {
                next.phase = Phase::Disabled;
            }
            Action::ToggleGates => {
                next.gates_ok = !next.gates_ok;
            }
            Action::ChangeRouteStamp => {
                next.current_route_stamp = (next.current_route_stamp + 1) % ROUTE_STAMP_DOMAIN;
            }
            Action::JoystickReturnToCenter => {
                next.joystick_centered = true;
                if next.phase != Phase::Disabled {
                    consumed = true;
                    if !next.gates_ok {
                        next.phase = Phase::WaitingForCenter;
                    } else if let Phase::Showing { route_stamp } = next.phase {
                        if route_stamp != next.current_route_stamp {
                            next.phase = Phase::WaitingForCenter;
                        } else {
                            next.phase = Phase::Idle;
                        }
                    } else {
                        next.phase = Phase::Idle;
                    }
                }
            }
            Action::JoystickMoveOffCenter => {
                next.joystick_centered = false;
                if next.phase != Phase::Disabled {
                    consumed = true;
                    if !next.gates_ok {
                        next.phase = Phase::WaitingForCenter;
                    } else if let Phase::Showing { route_stamp } = next.phase {
                        if route_stamp != next.current_route_stamp {
                            next.phase = Phase::WaitingForCenter;
                        }
                    } else if next.phase == Phase::Idle {
                        next.phase = Phase::Showing {
                            route_stamp: next.current_route_stamp,
                        };
                    }
                }
            }
            Action::EncClk => {
                if next.phase == Phase::Disabled {
                    next.last_normal_enc_clk_fired = true;
                } else {
                    let captured_confirm =
                        matches!(next.phase, Phase::Showing { .. } | Phase::Settling);
                    if !next.gates_ok {
                        next.phase = Phase::WaitingForCenter;
                        consumed = match policy {
                            Policy::Fixed => captured_confirm,
                            Policy::HistoricalBuggyRadialFallthrough => false,
                        };
                        next.last_normal_enc_clk_fired = !consumed;
                    } else if let Phase::Showing { route_stamp } = next.phase {
                        if route_stamp != next.current_route_stamp {
                            next.phase = Phase::WaitingForCenter;
                            consumed = match policy {
                                Policy::Fixed => captured_confirm,
                                Policy::HistoricalBuggyRadialFallthrough => false,
                            };
                            next.last_normal_enc_clk_fired = !consumed;
                        } else {
                            next.phase = Phase::Settling;
                            consumed = true;
                            next.last_normal_enc_clk_fired = false;
                        }
                    } else if next.phase == Phase::Settling {
                        consumed = true;
                        next.last_normal_enc_clk_fired = false;
                    } else {
                        consumed = false;
                        next.last_normal_enc_clk_fired = true;
                    }
                }
            }
            Action::EscapeKeyDown => {
                if matches!(next.phase, Phase::Showing { .. }) {
                    next.phase = Phase::WaitingForCenter;
                    consumed = true;
                }
            }
        }
        (next, StepOutcome { consumed })
    }

    pub fn check_invariants(
        before: State,
        action: Action,
        after: State,
        _outcome: StepOutcome,
    ) -> Result<(), (&'static str, String)> {
        if action == Action::EncClk
            && matches!(before.phase, Phase::Showing { .. } | Phase::Settling)
            && after.last_normal_enc_clk_fired
        {
            return Err((
                "captures_confirm_never_leaks_enc_clk_to_normal_binding",
                format!(
                    "EncClk leaked to normal binding while radial captured confirm: before={before:?}, after={after:?}"
                ),
            ));
        }

        if before.phase == Phase::Settling
            && !before.joystick_centered
            && matches!(after.phase, Phase::Showing { .. })
        {
            return Err((
                "settling_requires_center_before_reopen",
                format!(
                    "Settling transitioned directly to Showing without centering: before={before:?}, after={after:?}"
                ),
            ));
        }

        Ok(())
    }

    pub fn run(
        policy: Policy,
    ) -> Result<ExploreStats, CounterexampleTrace<State, Action, StepOutcome>> {
        explore_bfs(
            State::initial(),
            &ALL_ACTIONS,
            |state, action| step(state, action, policy),
            check_invariants,
        )
    }
}

#[test]
fn formal_model_a_fixed_vendor_bridge_and_preview_satisfies_all_invariants() {
    let stats = vendor_bridge_model::run(vendor_bridge_model::Policy::Fixed)
        .expect("fixed VendorBridge + Preview CAS model must satisfy all invariants");

    assert!(
        stats.states_explored > 50,
        "expected > 50 reachable states, explored {}",
        stats.states_explored
    );
    assert_eq!(
        stats.transitions_checked,
        stats.states_explored * vendor_bridge_model::ALL_ACTIONS.len()
    );
    println!(
        "formal_vendor_bridge_model_a: states_explored={}, transitions_checked={}",
        stats.states_explored, stats.transitions_checked
    );
}

#[test]
fn formal_model_a_catches_historical_2ca06be_missing_lease_epoch_check() {
    use vendor_bridge_model::{Action, Policy};

    let counterexample = vendor_bridge_model::run(Policy::HistoricalBuggyMissingLeaseEpochCheck)
        .expect_err("historical pre-2ca06be policy must violate stale ACK invariant");
    assert_eq!(
        counterexample.invariant,
        "no_stale_ack_confirmation_across_preview_lease"
    );
    assert_eq!(
        counterexample.actions(),
        vec![
            Action::Connect,
            Action::QueueDigest0,
            Action::StartThstatusSend,
            Action::AcquirePreviewLease,
            Action::ThstatusAckOk,
        ]
    );
    assert_eq!(counterexample.steps.len(), 5);

    let repeat = vendor_bridge_model::run(Policy::HistoricalBuggyMissingLeaseEpochCheck)
        .expect_err("BFS counterexample must be deterministic");
    assert_eq!(counterexample, repeat);
}

#[test]
fn formal_model_a_catches_historical_5990342_target_only_preview_restore() {
    use vendor_bridge_model::{Action, Policy};

    let counterexample = vendor_bridge_model::run(Policy::HistoricalBuggyTargetOnlyPreviewRestore)
        .expect_err("historical pre-5990342 policy must violate external LED mutation invariant");
    assert_eq!(
        counterexample.invariant,
        "external_led_mutation_never_overwritten_on_preview_restore"
    );
    assert_eq!(
        counterexample.actions(),
        vec![
            Action::Connect,
            Action::AcquirePreviewLease,
            Action::ExternalLedMutate,
            Action::ReleasePreviewLeaseAndRestore,
        ]
    );
    assert_eq!(counterexample.steps.len(), 4);

    let repeat = vendor_bridge_model::run(Policy::HistoricalBuggyTargetOnlyPreviewRestore)
        .expect_err("BFS counterexample must be deterministic");
    assert_eq!(counterexample, repeat);
}

#[test]
fn formal_model_b_fixed_radial_menu_satisfies_all_invariants() {
    let stats = radial_model::run(radial_model::Policy::Fixed)
        .expect("fixed RadialMenuState model must satisfy all invariants");

    assert!(
        stats.states_explored > 20,
        "expected > 20 reachable states, explored {}",
        stats.states_explored
    );
    assert_eq!(
        stats.transitions_checked,
        stats.states_explored * radial_model::ALL_ACTIONS.len()
    );
    println!(
        "formal_radial_model_b: states_explored={}, transitions_checked={}",
        stats.states_explored, stats.transitions_checked
    );
}

#[test]
fn formal_model_b_catches_historical_4be36ce_radial_fallthrough() {
    use radial_model::{Action, Policy};

    let counterexample = radial_model::run(Policy::HistoricalBuggyRadialFallthrough)
        .expect_err("historical pre-4be36ce policy must violate captures_confirm invariant");
    assert_eq!(
        counterexample.invariant,
        "captures_confirm_never_leaks_enc_clk_to_normal_binding"
    );
    assert_eq!(
        counterexample.actions(),
        vec![
            Action::EnableRadial,
            Action::JoystickReturnToCenter,
            Action::JoystickMoveOffCenter,
            Action::ChangeRouteStamp,
            Action::EncClk,
        ]
    );
    assert_eq!(counterexample.steps.len(), 5);

    let repeat = radial_model::run(Policy::HistoricalBuggyRadialFallthrough)
        .expect_err("BFS counterexample must be deterministic");
    assert_eq!(counterexample, repeat);
}

#[test]
fn real_radial_menu_state_confirms_captures_confirm_and_settling_center_return() {
    let mut radial = RadialMenuState::new();
    radial.set_confirm_mode(RadialConfirmMode::EncoderPress);
    radial.set_enabled(true);
    assert!(!radial.captures_confirm());
    assert!(matches!(radial.phase(), RadialPhase::WaitingForCenter));

    let make_session = || {
        let configs = RadialSlotConfig::for_preset(RadialPresetKind::PaseoEngineering);
        let ctx = RadialSlotResolutionContext {
            has_active_profile: true,
            active_adapter: Some("antigravity".into()),
            active_agent_display_name: Some("Antigravity".into()),
            active_session_preview: None,
            can_navigate_back: true,
            can_navigate_forward: true,
        };
        Some(RadialMenuSession {
            input_session: 1,
            route: RadialRouteStamp {
                document_revision: 1,
                host_revision: 1,
                profile_id: Some("profile-1".into()),
                profile_name: "Profile 1".into(),
            },
            slots: resolve_radial_slots(&configs, &ctx, |_| true),
            selected: None,
            anchor_frame: ScreenRect {
                x: 0.0,
                y: 0.0,
                width: 500.0,
                height: 500.0,
            },
        })
    };

    let now = Instant::now();
    assert!(radial
        .update(RadialPositionSample::new(0.0, 0.8), now, make_session)
        .is_none());
    assert!(matches!(radial.phase(), RadialPhase::WaitingForCenter));

    assert!(radial
        .update(RadialPositionSample::new(0.0, 0.1), now, make_session)
        .is_none());
    assert!(matches!(radial.phase(), RadialPhase::Idle));
    assert!(!radial.captures_confirm());

    assert!(radial
        .update(RadialPositionSample::new(0.0, 0.8), now, make_session)
        .is_none());
    assert!(matches!(radial.phase(), RadialPhase::Showing(_)));
    assert!(radial.captures_confirm());
    assert_eq!(
        radial.session().and_then(|s| s.selected),
        Some(SectorIndex::EAST)
    );

    let confirmed = radial.confirm();
    assert!(confirmed.is_some());
    assert!(matches!(radial.phase(), RadialPhase::Settling));
    assert!(radial.captures_confirm());

    assert!(radial
        .update(RadialPositionSample::new(0.0, 0.85), now, make_session)
        .is_none());
    assert!(
        matches!(radial.phase(), RadialPhase::Settling),
        "Settling must not reopen Showing before joystick returns to center"
    );
    assert!(radial.captures_confirm());

    assert!(radial
        .update(RadialPositionSample::new(0.0, 0.1), now, make_session)
        .is_none());
    assert!(matches!(radial.phase(), RadialPhase::Idle));
    assert!(!radial.captures_confirm());

    assert!(radial
        .update(RadialPositionSample::new(0.0, 0.85), now, make_session)
        .is_none());
    assert!(matches!(radial.phase(), RadialPhase::Showing(_)));
    assert!(radial.captures_confirm());
}
