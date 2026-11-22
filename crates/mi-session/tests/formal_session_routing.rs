use std::collections::{BTreeMap, BTreeSet, VecDeque};

const ID_DOMAIN: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Method {
    ReadStatus,
    WriteChunk,
    VerifyRead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum OpPhase {
    NotSent,
    Writing,
    Verifying,
    Verified,
    DeviceStateUnknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ModelState {
    session_invalid: bool,
    pending: Option<(u8, Method)>,
    next_id: u8,
    send_started: bool,
    send_complete: bool,
    op_phase: OpPhase,
}

impl ModelState {
    fn initial() -> Self {
        Self {
            session_invalid: false,
            pending: None,
            next_id: 0,
            send_started: false,
            send_complete: false,
            op_phase: OpPhase::NotSent,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Action {
    StartRequest,
    SendChunkToWire,
    CompleteSend,
    ReceiveOwnedValidResponse,
    ReceiveOwnedMalformedResponse,
    ReceiveForeignIdResponse,
    ReceiveSameIdForeignMethodResponse,
    CancelInFlight,
    VerifySuccess,
}

const ALL_ACTIONS: [Action; 9] = [
    Action::StartRequest,
    Action::SendChunkToWire,
    Action::CompleteSend,
    Action::ReceiveOwnedValidResponse,
    Action::ReceiveOwnedMalformedResponse,
    Action::ReceiveForeignIdResponse,
    Action::ReceiveSameIdForeignMethodResponse,
    Action::CancelInFlight,
    Action::VerifySuccess,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Policy {
    Fixed,
    HistoricalBuggyPreOwnershipValidation,
    HistoricalBuggyUninvalidatedCancelAfterSend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StepOutcome {
    response_discarded: bool,
    session_disconnected: bool,
}

impl StepOutcome {
    fn idle() -> Self {
        Self {
            response_discarded: false,
            session_disconnected: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TraceStep {
    action: Action,
    before: ModelState,
    after: ModelState,
    outcome: StepOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CounterexampleTrace {
    invariant: &'static str,
    detail: String,
    steps: Vec<TraceStep>,
}

impl CounterexampleTrace {
    fn actions(&self) -> Vec<Action> {
        self.steps.iter().map(|step| step.action).collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ExploreStats {
    states_explored: usize,
    transitions_checked: usize,
}

fn step(state: ModelState, action: Action, policy: Policy) -> (ModelState, StepOutcome) {
    let mut next = state;
    let mut outcome = StepOutcome::idle();

    match action {
        Action::StartRequest => {
            if !next.session_invalid && next.pending.is_none() {
                match next.op_phase {
                    OpPhase::NotSent => {
                        let id = next.next_id;
                        next.next_id = (next.next_id + 1) % ID_DOMAIN;
                        next.pending = Some((id, Method::WriteChunk));
                        next.send_started = false;
                        next.send_complete = false;
                    }
                    OpPhase::Verifying => {
                        let id = next.next_id;
                        next.next_id = (next.next_id + 1) % ID_DOMAIN;
                        next.pending = Some((id, Method::VerifyRead));
                        next.send_complete = true;
                    }
                    OpPhase::Verified => {
                        let id = next.next_id;
                        next.next_id = (next.next_id + 1) % ID_DOMAIN;
                        next.pending = Some((id, Method::ReadStatus));
                        next.send_complete = true;
                    }
                    OpPhase::Writing | OpPhase::DeviceStateUnknown => {}
                }
            }
        }
        Action::SendChunkToWire => {
            if !next.session_invalid
                && matches!(next.pending, Some((_, Method::WriteChunk)))
                && !next.send_started
            {
                next.send_started = true;
                next.op_phase = OpPhase::Writing;
            }
        }
        Action::CompleteSend => {
            if !next.session_invalid
                && next.pending.is_some()
                && next.send_started
                && !next.send_complete
            {
                next.send_complete = true;
            }
        }
        Action::ReceiveOwnedValidResponse => {
            if !next.session_invalid && next.send_complete {
                if let Some((_, method)) = next.pending.take() {
                    match method {
                        Method::WriteChunk => {
                            next.op_phase = OpPhase::Verifying;
                        }
                        Method::VerifyRead => {
                            next.op_phase = OpPhase::Verified;
                        }
                        Method::ReadStatus => {}
                    }
                }
            }
        }
        Action::ReceiveOwnedMalformedResponse => {
            if !next.session_invalid && next.pending.is_some() {
                next.session_invalid = true;
                next.pending = None;
                outcome.session_disconnected = true;
                if next.send_started && next.op_phase != OpPhase::Verified {
                    next.op_phase = OpPhase::DeviceStateUnknown;
                }
            }
        }
        Action::ReceiveForeignIdResponse => {
            if !next.session_invalid {
                if matches!(policy, Policy::HistoricalBuggyPreOwnershipValidation) {
                    next.session_invalid = true;
                    next.pending = None;
                    outcome.session_disconnected = true;
                    if next.send_started && next.op_phase != OpPhase::Verified {
                        next.op_phase = OpPhase::DeviceStateUnknown;
                    }
                } else {
                    outcome.response_discarded = true;
                }
            }
        }
        Action::ReceiveSameIdForeignMethodResponse => {
            if !next.session_invalid && next.pending.is_some() {
                if matches!(policy, Policy::HistoricalBuggyPreOwnershipValidation) {
                    next.session_invalid = true;
                    next.pending = None;
                    outcome.session_disconnected = true;
                    if next.send_started && next.op_phase != OpPhase::Verified {
                        next.op_phase = OpPhase::DeviceStateUnknown;
                    }
                } else {
                    outcome.response_discarded = true;
                }
            }
        }
        Action::CancelInFlight => {
            if next.pending.is_some()
                || matches!(next.op_phase, OpPhase::Writing | OpPhase::Verifying)
            {
                next.pending = None;
                if next.send_started && next.op_phase != OpPhase::Verified {
                    if matches!(policy, Policy::HistoricalBuggyUninvalidatedCancelAfterSend) {
                        next.op_phase = OpPhase::NotSent;
                        next.send_started = false;
                        next.send_complete = false;
                    } else {
                        next.session_invalid = true;
                        next.op_phase = OpPhase::DeviceStateUnknown;
                        outcome.session_disconnected = true;
                    }
                }
            }
        }
        Action::VerifySuccess => {
            if !next.session_invalid
                && next.op_phase == OpPhase::Verifying
                && next.pending.is_none()
            {
                next.op_phase = OpPhase::Verified;
            }
        }
    }

    (next, outcome)
}

fn check_invariants(
    before: ModelState,
    action: Action,
    after: ModelState,
    _outcome: StepOutcome,
) -> Result<(), (&'static str, String)> {
    if matches!(
        action,
        Action::ReceiveForeignIdResponse | Action::ReceiveSameIdForeignMethodResponse
    ) {
        if (!before.session_invalid && after.session_invalid) || after.pending != before.pending {
            return Err((
                "foreign_response_never_kills_session_or_fulfills_pending",
                format!(
                    "{action:?} mutated session_invalid or pending: before={before:?}, after={after:?}"
                ),
            ));
        }
    }

    if action == Action::CancelInFlight
        && before.send_started
        && before.op_phase != OpPhase::Verified
        && (!after.session_invalid || after.op_phase != OpPhase::DeviceStateUnknown)
    {
        return Err((
            "cancel_after_wire_write_forces_invalid_and_unknown",
            format!(
                "CancelInFlight after wire write failed to invalidate session and mark DeviceStateUnknown: before={before:?}, after={after:?}"
            ),
        ));
    }

    Ok(())
}

fn reconstruct_trace(
    parents: &BTreeMap<ModelState, TraceStep>,
    mut cursor: ModelState,
    final_step: TraceStep,
) -> Vec<TraceStep> {
    let mut reversed = vec![final_step];
    while let Some(step) = parents.get(&cursor) {
        reversed.push(step.clone());
        cursor = step.before;
    }
    reversed.reverse();
    reversed
}

fn run_model_checker(policy: Policy) -> Result<ExploreStats, CounterexampleTrace> {
    let initial = ModelState::initial();
    let mut visited: BTreeSet<ModelState> = BTreeSet::new();
    let mut parents: BTreeMap<ModelState, TraceStep> = BTreeMap::new();
    let mut queue: VecDeque<ModelState> = VecDeque::new();
    let mut transitions_checked: usize = 0;

    visited.insert(initial);
    queue.push_back(initial);

    while let Some(current) = queue.pop_front() {
        for &action in &ALL_ACTIONS {
            transitions_checked += 1;
            let (next, outcome) = step(current, action, policy);
            let trace_step = TraceStep {
                action,
                before: current,
                after: next,
                outcome,
            };
            if let Err((invariant, detail)) = check_invariants(current, action, next, outcome) {
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

#[test]
fn formal_model_c_fixed_session_routing_satisfies_all_invariants() {
    let stats = run_model_checker(Policy::Fixed)
        .expect("fixed DeviceSession routing + MutationGuard model must satisfy all invariants");

    assert!(
        stats.states_explored > 15,
        "expected > 15 reachable states, explored {}",
        stats.states_explored
    );
    assert_eq!(
        stats.transitions_checked,
        stats.states_explored * ALL_ACTIONS.len()
    );
    println!(
        "formal_session_routing_model_c: states_explored={}, transitions_checked={}",
        stats.states_explored, stats.transitions_checked
    );
}

#[test]
fn formal_model_c_catches_historical_5880dcf_pre_ownership_validation() {
    let counterexample = run_model_checker(Policy::HistoricalBuggyPreOwnershipValidation)
        .expect_err("historical pre-5880dcf policy must violate foreign response invariant");
    assert_eq!(
        counterexample.invariant,
        "foreign_response_never_kills_session_or_fulfills_pending"
    );
    assert_eq!(
        counterexample.actions(),
        vec![Action::ReceiveForeignIdResponse]
    );
    assert_eq!(counterexample.steps.len(), 1);

    let repeat = run_model_checker(Policy::HistoricalBuggyPreOwnershipValidation)
        .expect_err("BFS counterexample must be deterministic");
    assert_eq!(counterexample, repeat);
}

#[test]
fn formal_model_c_catches_historical_2860df3_uninvalidated_cancel_after_send() {
    let counterexample = run_model_checker(Policy::HistoricalBuggyUninvalidatedCancelAfterSend)
        .expect_err("historical pre-2860df3 policy must violate cancel-after-send invariant");
    assert_eq!(
        counterexample.invariant,
        "cancel_after_wire_write_forces_invalid_and_unknown"
    );
    assert_eq!(
        counterexample.actions(),
        vec![
            Action::StartRequest,
            Action::SendChunkToWire,
            Action::CancelInFlight,
        ]
    );
    assert_eq!(counterexample.steps.len(), 3);

    let repeat = run_model_checker(Policy::HistoricalBuggyUninvalidatedCancelAfterSend)
        .expect_err("BFS counterexample must be deterministic");
    assert_eq!(counterexample, repeat);
}

#[test]
fn real_proto_is_unmatched_response_discards_foreign_id_and_foreign_method() {
    let foreign_id = br#"{"id":99,"method":"host.unknown","result":{}}"#;
    assert_eq!(
        mi_proto::is_unmatched_response(foreign_id, Some((7, "fs.writebin"))).unwrap(),
        true
    );
    assert_eq!(
        mi_proto::is_unmatched_response(foreign_id, None).unwrap(),
        true
    );

    let same_id_foreign_method = br#"{"id":7,"method":"device.status","result":{}}"#;
    assert_eq!(
        mi_proto::is_unmatched_response(same_id_foreign_method, Some((7, "fs.writebin"))).unwrap(),
        true
    );

    let owned_matching = br#"{"id":7,"method":"fs.writebin","result":{"ok":true}}"#;
    assert_eq!(
        mi_proto::is_unmatched_response(owned_matching, Some((7, "fs.writebin"))).unwrap(),
        false
    );
}
