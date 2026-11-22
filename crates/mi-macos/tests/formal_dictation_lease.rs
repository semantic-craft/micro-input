use mi_macos::executor::DialScrollRequest;
use mi_macos::managed_controls::{
    ManagedAxControls, ManagedAxProvider, ManagedControl, ManagedWindow, ManagedWindowIdentity,
    ObservedDictationState,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const TITLE_DOMAIN: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum LeaseState {
    None,
    Claimed { container: u8, captured_title: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum UiState {
    Idle,
    Recording,
    Finishing,
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct WindowState {
    container: u8,
    title: u8,
    pid_exited: bool,
    ui: UiState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct HostState {
    execution_enabled: bool,
    cleanup_pending: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ModelState {
    lease: LeaseState,
    window: WindowState,
    host: HostState,
}

impl ModelState {
    fn initial() -> Self {
        Self {
            lease: LeaseState::None,
            window: WindowState {
                container: 0,
                title: 0,
                pid_exited: false,
                ui: UiState::Idle,
            },
            host: HostState {
                execution_enabled: true,
                cleanup_pending: false,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Action {
    StartDictation,
    TitleDrift,
    UserStopsManually,
    TransitionToFinishing,
    TransitionToIdle,
    ProcessExits,
    WindowDisappears,
    NonDictationControl,
    DisableHostExecution,
    RetryCleanup,
    SubmitDictation,
}

const ALL_ACTIONS: [Action; 11] = [
    Action::StartDictation,
    Action::TitleDrift,
    Action::UserStopsManually,
    Action::TransitionToFinishing,
    Action::TransitionToIdle,
    Action::ProcessExits,
    Action::WindowDisappears,
    Action::NonDictationControl,
    Action::DisableHostExecution,
    Action::RetryCleanup,
    Action::SubmitDictation,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransitionPolicy {
    FixedContainerIdentity,
    HistoricalBuggyTitleEquality,
    HistoricalBuggySubmitDuringFinishing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActionResult {
    Ok,
    BlockedLease,
    BlockedOther,
    Unconfirmed,
    NoOp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StepOutcome {
    ax_press_issued: bool,
    ax_submit_issued: bool,
    result: ActionResult,
}

impl StepOutcome {
    fn no_side_effects(result: ActionResult) -> Self {
        Self {
            ax_press_issued: false,
            ax_submit_issued: false,
            result,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InvariantFilter {
    All,
    NoTitleDriftDeadlock,
}

fn reconcile_for_captured_window(
    state: &mut ModelState,
    policy: TransitionPolicy,
) -> Result<(), ActionResult> {
    let LeaseState::Claimed {
        container,
        captured_title,
    } = state.lease
    else {
        return Ok(());
    };
    if container == state.window.container && captured_title == state.window.title {
        return Ok(());
    }
    if state.window.pid_exited {
        state.lease = LeaseState::None;
        return Ok(());
    }
    if !matches!(policy, TransitionPolicy::HistoricalBuggyTitleEquality)
        && container == state.window.container
        && state.window.ui == UiState::Idle
    {
        state.lease = LeaseState::None;
        return Ok(());
    }
    Err(ActionResult::BlockedLease)
}

fn stop_all_owned(state: &mut ModelState, policy: TransitionPolicy) -> StepOutcome {
    let LeaseState::Claimed {
        container,
        captured_title,
    } = state.lease
    else {
        state.host.cleanup_pending = false;
        return StepOutcome::no_side_effects(ActionResult::Ok);
    };
    if state.window.pid_exited {
        state.lease = LeaseState::None;
        state.host.cleanup_pending = false;
        return StepOutcome::no_side_effects(ActionResult::Ok);
    }
    if state.window.ui == UiState::Missing || state.window.container != container {
        state.host.cleanup_pending = true;
        return StepOutcome::no_side_effects(ActionResult::Unconfirmed);
    }
    if state.window.title != captured_title {
        if !matches!(policy, TransitionPolicy::HistoricalBuggyTitleEquality)
            && state.window.ui == UiState::Idle
        {
            state.lease = LeaseState::None;
            state.host.cleanup_pending = false;
            return StepOutcome::no_side_effects(ActionResult::Ok);
        }
        state.host.cleanup_pending = true;
        return StepOutcome::no_side_effects(ActionResult::Unconfirmed);
    }
    match state.window.ui {
        UiState::Idle => {
            state.lease = LeaseState::None;
            state.host.cleanup_pending = false;
            StepOutcome::no_side_effects(ActionResult::Ok)
        }
        UiState::Recording => {
            state.window.ui = UiState::Idle;
            state.lease = LeaseState::None;
            state.host.cleanup_pending = false;
            StepOutcome {
                ax_press_issued: true,
                ax_submit_issued: false,
                result: ActionResult::Ok,
            }
        }
        UiState::Finishing | UiState::Missing => {
            state.host.cleanup_pending = true;
            StepOutcome::no_side_effects(ActionResult::Unconfirmed)
        }
    }
}

fn step(state: ModelState, action: Action, policy: TransitionPolicy) -> (ModelState, StepOutcome) {
    let mut next = state;
    match action {
        Action::StartDictation => {
            if !next.host.execution_enabled {
                return (next, StepOutcome::no_side_effects(ActionResult::NoOp));
            }
            if next.window.pid_exited || next.window.ui == UiState::Missing {
                return (
                    next,
                    StepOutcome::no_side_effects(ActionResult::BlockedOther),
                );
            }
            if let Err(err) = reconcile_for_captured_window(&mut next, policy) {
                return (next, StepOutcome::no_side_effects(err));
            }
            if next.window.ui != UiState::Idle {
                return (
                    next,
                    StepOutcome::no_side_effects(ActionResult::BlockedOther),
                );
            }
            next.window.ui = UiState::Recording;
            next.lease = LeaseState::Claimed {
                container: next.window.container,
                captured_title: next.window.title,
            };
            (
                next,
                StepOutcome {
                    ax_press_issued: true,
                    ax_submit_issued: false,
                    result: ActionResult::Ok,
                },
            )
        }
        Action::TitleDrift => {
            if !next.window.pid_exited && next.window.ui != UiState::Missing {
                next.window.title = (next.window.title + 1) % TITLE_DOMAIN;
            }
            (next, StepOutcome::no_side_effects(ActionResult::Ok))
        }
        Action::UserStopsManually => {
            if !next.window.pid_exited && next.window.ui == UiState::Recording {
                next.window.ui = UiState::Idle;
            }
            (next, StepOutcome::no_side_effects(ActionResult::Ok))
        }
        Action::TransitionToFinishing => {
            if !next.window.pid_exited && next.window.ui == UiState::Recording {
                next.window.ui = UiState::Finishing;
            }
            (next, StepOutcome::no_side_effects(ActionResult::Ok))
        }
        Action::TransitionToIdle => {
            if !next.window.pid_exited && next.window.ui == UiState::Finishing {
                next.window.ui = UiState::Idle;
            }
            (next, StepOutcome::no_side_effects(ActionResult::Ok))
        }
        Action::ProcessExits => {
            if !next.window.pid_exited {
                next.window.pid_exited = true;
                next.window.ui = UiState::Missing;
            }
            (next, StepOutcome::no_side_effects(ActionResult::Ok))
        }
        Action::WindowDisappears => {
            if !next.window.pid_exited && next.window.ui != UiState::Missing {
                next.window.ui = UiState::Missing;
            }
            (next, StepOutcome::no_side_effects(ActionResult::Ok))
        }
        Action::NonDictationControl => {
            if !next.host.execution_enabled {
                return (next, StepOutcome::no_side_effects(ActionResult::NoOp));
            }
            if next.window.pid_exited || next.window.ui == UiState::Missing {
                return (
                    next,
                    StepOutcome::no_side_effects(ActionResult::BlockedOther),
                );
            }
            if matches!(policy, TransitionPolicy::HistoricalBuggyTitleEquality) {
                let lease_drifted = match next.lease {
                    LeaseState::Claimed {
                        container,
                        captured_title,
                    } => container != next.window.container || captured_title != next.window.title,
                    LeaseState::None => false,
                };
                if next.host.cleanup_pending || lease_drifted {
                    return (
                        next,
                        StepOutcome::no_side_effects(ActionResult::BlockedLease),
                    );
                }
            }
            (next, StepOutcome::no_side_effects(ActionResult::Ok))
        }
        Action::DisableHostExecution => {
            next.host.execution_enabled = false;
            let outcome = stop_all_owned(&mut next, policy);
            (next, outcome)
        }
        Action::RetryCleanup => {
            let outcome = stop_all_owned(&mut next, policy);
            (next, outcome)
        }
        Action::SubmitDictation => {
            if !next.host.execution_enabled {
                return (next, StepOutcome::no_side_effects(ActionResult::NoOp));
            }
            if next.window.pid_exited || next.window.ui == UiState::Missing {
                return (
                    next,
                    StepOutcome::no_side_effects(ActionResult::BlockedOther),
                );
            }
            if let Err(err) = reconcile_for_captured_window(&mut next, policy) {
                return (next, StepOutcome::no_side_effects(err));
            }
            match next.window.ui {
                UiState::Idle => {
                    next.lease = LeaseState::None;
                    (
                        next,
                        StepOutcome {
                            ax_press_issued: false,
                            ax_submit_issued: true,
                            result: ActionResult::Ok,
                        },
                    )
                }
                UiState::Recording => {
                    next.window.ui = UiState::Idle;
                    next.lease = LeaseState::None;
                    (
                        next,
                        StepOutcome {
                            ax_press_issued: true,
                            ax_submit_issued: true,
                            result: ActionResult::Ok,
                        },
                    )
                }
                UiState::Finishing => {
                    if matches!(
                        policy,
                        TransitionPolicy::HistoricalBuggySubmitDuringFinishing
                    ) {
                        next.lease = LeaseState::None;
                        (
                            next,
                            StepOutcome {
                                ax_press_issued: false,
                                ax_submit_issued: true,
                                result: ActionResult::Ok,
                            },
                        )
                    } else {
                        (
                            next,
                            StepOutcome::no_side_effects(ActionResult::Unconfirmed),
                        )
                    }
                }
                UiState::Missing => (
                    next,
                    StepOutcome::no_side_effects(ActionResult::BlockedOther),
                ),
            }
        }
    }
}

fn check_invariants(
    before: ModelState,
    action: Action,
    after: ModelState,
    outcome: StepOutcome,
    filter: InvariantFilter,
) -> Result<(), (&'static str, String)> {
    let check_deadlock = matches!(
        filter,
        InvariantFilter::All | InvariantFilter::NoTitleDriftDeadlock
    );
    let check_fence = matches!(filter, InvariantFilter::All);
    let check_isolation = matches!(filter, InvariantFilter::All);

    if check_deadlock {
        if let LeaseState::Claimed { container, .. } = before.lease {
            let idle_same_container = !before.window.pid_exited
                && before.window.container == container
                && before.window.ui == UiState::Idle;
            let exited = before.window.pid_exited;
            if idle_same_container || exited {
                match action {
                    Action::RetryCleanup | Action::DisableHostExecution => {
                        if after.lease != LeaseState::None
                            || outcome.ax_press_issued
                            || outcome.result != ActionResult::Ok
                            || after.host.cleanup_pending
                        {
                            return Err((
                                "no_title_drift_deadlock",
                                format!(
                                    "{action:?} failed to clear idle/exited lease with zero AX presses: before={before:?}, after={after:?}, outcome={outcome:?}"
                                ),
                            ));
                        }
                    }
                    Action::StartDictation
                        if before.host.execution_enabled && idle_same_container =>
                    {
                        if outcome.result == ActionResult::BlockedLease {
                            return Err((
                                "no_title_drift_deadlock",
                                format!(
                                    "StartDictation deadlocked with BlockedLease on idle container: before={before:?}, after={after:?}, outcome={outcome:?}"
                                ),
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    if check_fence {
        if let LeaseState::Claimed {
            container,
            captured_title,
        } = before.lease
        {
            let drifted =
                before.window.container != container || before.window.title != captured_title;
            let active_ui = matches!(before.window.ui, UiState::Recording | UiState::Finishing);
            if !before.window.pid_exited && drifted && active_ui {
                if outcome.ax_press_issued || outcome.ax_submit_issued {
                    return Err((
                        "safe_conservative_fence",
                        format!(
                            "{action:?} issued AX press/submit while drifted lease was active: before={before:?}, outcome={outcome:?}"
                        ),
                    ));
                }
                if action == Action::StartDictation && before.host.execution_enabled {
                    if outcome.result != ActionResult::BlockedLease || after.lease != before.lease {
                        return Err((
                            "safe_conservative_fence",
                            format!(
                                "StartDictation bypassed drifted active lease fence: before={before:?}, after={after:?}, outcome={outcome:?}"
                            ),
                        ));
                    }
                }
                if action == Action::SubmitDictation && before.host.execution_enabled {
                    if outcome.result != ActionResult::BlockedLease || after.lease != before.lease {
                        return Err((
                            "safe_conservative_fence",
                            format!(
                                "SubmitDictation bypassed drifted active lease fence: before={before:?}, after={after:?}, outcome={outcome:?}"
                            ),
                        ));
                    }
                }
                if matches!(action, Action::RetryCleanup | Action::DisableHostExecution)
                    && (outcome.result != ActionResult::Unconfirmed
                        || after.lease != before.lease
                        || !after.host.cleanup_pending)
                {
                    return Err((
                        "safe_conservative_fence",
                        format!(
                            "{action:?} failed to stay conservatively blocked on drifted active lease: before={before:?}, after={after:?}, outcome={outcome:?}"
                        ),
                    ));
                }
            }
        }
        if action == Action::SubmitDictation
            && before.window.ui == UiState::Finishing
            && outcome.ax_submit_issued
        {
            return Err((
                "safe_conservative_fence",
                format!(
                    "SubmitDictation issued AX submit while dictation UI was Finishing: before={before:?}, after={after:?}, outcome={outcome:?}"
                ),
            ));
        }
    }

    if check_isolation
        && action == Action::NonDictationControl
        && outcome.result == ActionResult::BlockedLease
    {
        return Err((
            "non_dictation_control_isolation",
            format!(
                "NonDictationControl failed with dictation-lease error: before={before:?}, after={after:?}, outcome={outcome:?}"
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

fn explore_bfs<S, A, FStep, FInv>(
    initial: S,
    actions: &[A],
    step_fn: FStep,
    invariant_fn: FInv,
) -> Result<ExploreStats, CounterexampleTrace>
where
    S: Copy + Ord,
    A: Copy,
    FStep: Fn(S, A) -> (S, StepOutcome),
    FInv: Fn(S, A, S, StepOutcome) -> Result<(), (&'static str, String)>,
    S: Into<ModelState>,
    A: Into<Action>,
{
    let mut visited: BTreeSet<S> = BTreeSet::new();
    let mut parents: BTreeMap<ModelState, TraceStep> = BTreeMap::new();
    let mut queue: VecDeque<S> = VecDeque::new();
    let mut transitions_checked: usize = 0;

    visited.insert(initial);
    queue.push_back(initial);

    while let Some(current) = queue.pop_front() {
        for &action in actions {
            transitions_checked += 1;
            let (next, outcome) = step_fn(current, action);
            let trace_step = TraceStep {
                action: action.into(),
                before: current.into(),
                after: next.into(),
                outcome,
            };
            if let Err((invariant, detail)) = invariant_fn(current, action, next, outcome) {
                return Err(CounterexampleTrace {
                    invariant,
                    detail,
                    steps: reconstruct_trace(&parents, current.into(), trace_step),
                });
            }
            if visited.insert(next) {
                parents.insert(next.into(), trace_step);
                queue.push_back(next);
            }
        }
    }

    Ok(ExploreStats {
        states_explored: visited.len(),
        transitions_checked,
    })
}

fn run_model_checker(
    policy: TransitionPolicy,
    filter: InvariantFilter,
) -> Result<ExploreStats, CounterexampleTrace> {
    explore_bfs(
        ModelState::initial(),
        &ALL_ACTIONS,
        |state, action| step(state, action, policy),
        |before, action, after, outcome| check_invariants(before, action, after, outcome, filter),
    )
}

struct ReplayFixture {
    window: ManagedWindow,
    events: Vec<String>,
    retained_targets: Vec<ManagedWindowIdentity>,
    exited_pids: Vec<i32>,
}

#[derive(Clone)]
struct ReplayProvider(Arc<Mutex<ReplayFixture>>);

impl ReplayProvider {
    fn antigravity() -> Self {
        Self(Arc::new(Mutex::new(ReplayFixture {
            window: ManagedWindow {
                identity: ManagedWindowIdentity {
                    bundle_id: "com.google.antigravity".into(),
                    pid: 100,
                    window_id: 1,
                    scope_id: 0,
                    title: "Conversation A".into(),
                },
                controls: vec![
                    ManagedControl {
                        id: 2,
                        role: "AXCheckBox".into(),
                        label: "Record voice memo".into(),
                        enabled: true,
                        pressable: true,
                        editable: false,
                        focused: false,
                        expanded: None,
                    },
                    ManagedControl {
                        id: 3,
                        role: "AXTextArea".into(),
                        label: String::new(),
                        enabled: true,
                        pressable: false,
                        editable: true,
                        focused: false,
                        expanded: None,
                    },
                    ManagedControl {
                        id: 41,
                        role: "AXPopUpButton".into(),
                        label: "Claude Sonnet 4.6".into(),
                        enabled: true,
                        pressable: true,
                        editable: false,
                        focused: false,
                        expanded: Some(true),
                    },
                    ManagedControl {
                        id: 42,
                        role: "AXButton".into(),
                        label: "Toggle Side Panel".into(),
                        enabled: true,
                        pressable: true,
                        editable: false,
                        focused: false,
                        expanded: Some(false),
                    },
                    ManagedControl {
                        id: 43,
                        role: "AXButton".into(),
                        label: "New tab".into(),
                        enabled: true,
                        pressable: true,
                        editable: false,
                        focused: false,
                        expanded: None,
                    },
                ],
            },
            events: Vec::new(),
            retained_targets: Vec::new(),
            exited_pids: Vec::new(),
        })))
    }

    fn controls(&self) -> ManagedAxControls<Self> {
        ManagedAxControls::with_readback_timeout(self.clone(), Duration::ZERO)
    }

    fn events(&self) -> Vec<String> {
        self.0.lock().unwrap().events.clone()
    }
}

impl ManagedAxProvider for ReplayProvider {
    fn retain_target(&mut self, target: &ManagedWindowIdentity) -> Result<(), String> {
        self.0.lock().unwrap().retained_targets.push(target.clone());
        Ok(())
    }

    fn release_target(&mut self, target: &ManagedWindowIdentity) {
        self.0
            .lock()
            .unwrap()
            .retained_targets
            .retain(|old| old != target);
    }

    fn process_has_exited(&self, pid: i32) -> bool {
        self.0.lock().unwrap().exited_pids.contains(&pid)
    }

    fn capture(&mut self, bundle_id: &str) -> Result<ManagedWindow, String> {
        let mut state = self.0.lock().unwrap();
        state.events.push(format!("capture:{bundle_id}"));
        Ok(state.window.clone())
    }

    fn read(&mut self, target: &ManagedWindowIdentity) -> Result<ManagedWindow, String> {
        let state = self.0.lock().unwrap();
        assert_eq!(target.pid, state.window.identity.pid);
        Ok(state.window.clone())
    }

    fn press(&mut self, target: &ManagedWindowIdentity, control_id: u64) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        assert_eq!(*target, state.window.identity);
        let old_label = {
            let control = state
                .window
                .controls
                .iter_mut()
                .find(|c| c.id == control_id)
                .unwrap();
            let old = control.label.clone();
            match control.label.as_str() {
                "Record voice memo" => control.label = "Stop Recording".into(),
                "Stop Recording" => control.label = "Record voice memo".into(),
                "Toggle Side Panel" => {
                    control.expanded = Some(!control.expanded.unwrap_or(false));
                }
                _ => {}
            }
            old
        };
        if old_label == "New tab" {
            state.window.controls.push(ManagedControl {
                id: 44,
                role: "AXButton".into(),
                label: "Browser".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: None,
            });
        } else if old_label == "Browser" {
            state.window.controls.retain(|c| c.id != 44);
            state.window.controls.push(ManagedControl {
                id: 45,
                role: "AXComboBox".into(),
                label: "Address bar".into(),
                enabled: true,
                pressable: false,
                editable: true,
                focused: false,
                expanded: None,
            });
        }
        state.events.push(format!("press:{old_label}"));
        Ok(())
    }

    fn focus(&mut self, target: &ManagedWindowIdentity, control_id: u64) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        assert_eq!(*target, state.window.identity);
        state
            .window
            .controls
            .iter_mut()
            .find(|c| c.id == control_id)
            .unwrap()
            .focused = true;
        state.events.push(format!("focus:{control_id}"));
        Ok(())
    }

    fn scroll(
        &mut self,
        target: &ManagedWindowIdentity,
        _request: &DialScrollRequest,
    ) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        assert_eq!(*target, state.window.identity);
        state.events.push("scroll".into());
        Ok(())
    }

    fn shortcut(
        &mut self,
        target: &ManagedWindowIdentity,
        _control: u64,
        _key: u16,
        _modifiers: &[String],
    ) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        assert_eq!(*target, state.window.identity);
        state.events.push("shortcut".into());
        Ok(())
    }

    fn insert_text(
        &mut self,
        target: &ManagedWindowIdentity,
        _control: u64,
        text: &str,
    ) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        assert_eq!(*target, state.window.identity);
        state.events.push(format!("insert:{text}"));
        Ok(())
    }

    fn submit(&mut self, target: &ManagedWindowIdentity, _control: u64) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        assert_eq!(*target, state.window.identity);
        state.events.push("submit".into());
        Ok(())
    }

    fn send_window_key(
        &mut self,
        target: &ManagedWindowIdentity,
        key_code: u16,
        _modifiers: &[String],
    ) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        assert_eq!(*target, state.window.identity);
        let label = match key_code {
            0x7D => "window_key:arrow_down".to_string(),
            _ => format!("window_key:{key_code:x}"),
        };
        state.events.push(label);
        Ok(())
    }
}

#[test]
fn formal_model_fixed_dictation_lease_satisfies_all_invariants_across_reachable_state_space() {
    let stats = run_model_checker(
        TransitionPolicy::FixedContainerIdentity,
        InvariantFilter::All,
    )
    .expect("fixed ContainerIdentity dictation lease model must satisfy all invariants");

    assert!(
        stats.states_explored > 100,
        "expected > 100 reachable states, explored {}",
        stats.states_explored
    );
    assert_eq!(
        stats.transitions_checked,
        stats.states_explored * ALL_ACTIONS.len()
    );
    println!(
        "formal_dictation_lease: states_explored={}, transitions_checked={}",
        stats.states_explored, stats.transitions_checked
    );
}

#[test]
fn formal_model_catches_historical_af941e7_title_drift_deadlock_and_control_lock() {
    let control_lock = run_model_checker(
        TransitionPolicy::HistoricalBuggyTitleEquality,
        InvariantFilter::All,
    )
    .expect_err("historical pre-af941e7 policy must violate invariants");
    assert_eq!(control_lock.invariant, "non_dictation_control_isolation");
    assert_eq!(
        control_lock.actions(),
        vec![
            Action::StartDictation,
            Action::TitleDrift,
            Action::NonDictationControl,
        ]
    );
    assert_eq!(control_lock.steps.len(), 3);

    let control_lock_repeat = run_model_checker(
        TransitionPolicy::HistoricalBuggyTitleEquality,
        InvariantFilter::All,
    )
    .expect_err("BFS counterexample must be deterministic");
    assert_eq!(control_lock, control_lock_repeat);

    let deadlock = run_model_checker(
        TransitionPolicy::HistoricalBuggyTitleEquality,
        InvariantFilter::NoTitleDriftDeadlock,
    )
    .expect_err("historical pre-af941e7 policy must violate no_title_drift_deadlock");
    assert_eq!(deadlock.invariant, "no_title_drift_deadlock");
    assert_eq!(
        deadlock.actions(),
        vec![
            Action::StartDictation,
            Action::TitleDrift,
            Action::UserStopsManually,
            Action::StartDictation,
        ]
    );
    assert_eq!(deadlock.steps.len(), 4);
}

#[test]
fn formal_model_catches_historical_submit_during_finishing_bug() {
    let counterexample = run_model_checker(
        TransitionPolicy::HistoricalBuggySubmitDuringFinishing,
        InvariantFilter::All,
    )
    .expect_err("historical submit-during-finishing policy must violate safe_conservative_fence");
    assert_eq!(counterexample.invariant, "safe_conservative_fence");
    assert_eq!(
        counterexample.actions(),
        vec![
            Action::StartDictation,
            Action::TransitionToFinishing,
            Action::SubmitDictation,
        ]
    );
    assert_eq!(counterexample.steps.len(), 3);
    assert!(
        counterexample
            .steps
            .last()
            .unwrap()
            .outcome
            .ax_submit_issued
    );
}

#[test]
fn real_managed_controls_replays_historical_counterexample_trace_without_deadlock() {
    let deadlock_trace = run_model_checker(
        TransitionPolicy::HistoricalBuggyTitleEquality,
        InvariantFilter::NoTitleDriftDeadlock,
    )
    .expect_err("historical model must produce title-drift deadlock counterexample");
    assert_eq!(
        deadlock_trace.actions(),
        vec![
            Action::StartDictation,
            Action::TitleDrift,
            Action::UserStopsManually,
            Action::StartDictation,
        ]
    );

    let provider = ReplayProvider::antigravity();
    let controls = provider.controls();

    assert_eq!(controls.start_dictation("antigravity"), Ok(()));
    assert_eq!(
        provider.0.lock().unwrap().window.controls[0].label,
        "Stop Recording"
    );
    assert_eq!(provider.0.lock().unwrap().retained_targets.len(), 1);

    provider.0.lock().unwrap().window.identity.title = "Conversation B".into();

    provider.0.lock().unwrap().window.controls[0].label = "Record voice memo".into();

    assert_eq!(controls.step_model_picker("antigravity", 0x7D), Ok(()));
    assert_eq!(controls.set_side_panel_open("antigravity", true), Ok(()));
    assert_eq!(controls.open_browser("antigravity"), Ok(()));

    let dictation_presses_before_cleanup = provider
        .events()
        .iter()
        .filter(|event| *event == "press:Record voice memo" || *event == "press:Stop Recording")
        .count();
    assert_eq!(dictation_presses_before_cleanup, 1);

    assert_eq!(controls.stop_owned_dictations(), Ok(()));
    assert!(provider.0.lock().unwrap().retained_targets.is_empty());

    let dictation_presses_after_cleanup = provider
        .events()
        .iter()
        .filter(|event| *event == "press:Record voice memo" || *event == "press:Stop Recording")
        .count();
    assert_eq!(
        dictation_presses_after_cleanup, 1,
        "idle reconcile on drifted title must emit zero additional dictation presses"
    );

    assert_eq!(
        controls.toggle_dictation("antigravity"),
        Ok(ObservedDictationState::Recording)
    );
    assert_eq!(
        controls.stop_owned_dictations(),
        Ok(()),
        "subsequent owned dictation on new title must clean up normally"
    );
    assert!(provider.0.lock().unwrap().retained_targets.is_empty());
}
