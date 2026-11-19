use crate::ax_dictation::DictationAdapter;
use crate::executor::DialScrollRequest;
use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedWindowIdentity {
    pub bundle_id: String,
    pub pid: i32,
    pub window_id: u64,
    pub scope_id: u64,
    pub title: String,
}

impl ManagedWindowIdentity {
    pub fn same_container(&self, other: &Self) -> bool {
        self.bundle_id == other.bundle_id
            && self.pid == other.pid
            && self.window_id == other.window_id
            && self.scope_id == other.scope_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedControl {
    pub id: u64,
    pub role: String,
    pub label: String,
    pub enabled: bool,
    pub pressable: bool,
    pub editable: bool,
    pub focused: bool,
    pub expanded: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedWindow {
    pub identity: ManagedWindowIdentity,
    pub controls: Vec<ManagedControl>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservedDictationState {
    Idle,
    Recording,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObservedContainerDictation {
    Idle,
    Recording,
    Finishing,
    Indeterminate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedControlReadiness {
    pub editable_inputs: usize,
    pub dictation: Result<ObservedDictationState, String>,
}

pub trait ManagedAxProvider: Send {
    fn retain_target(&mut self, _target: &ManagedWindowIdentity) -> Result<(), String> {
        Ok(())
    }
    fn release_target(&mut self, _target: &ManagedWindowIdentity) {}
    fn activate_window(&mut self, _target: &ManagedWindowIdentity) -> Result<(), String> {
        Ok(())
    }

    fn process_has_exited(&self, _pid: i32) -> bool {
        false
    }
    fn capture(&mut self, bundle_id: &str) -> Result<ManagedWindow, String>;
    fn capture_full_window(&mut self, bundle_id: &str) -> Result<ManagedWindow, String> {
        self.capture(bundle_id)
    }
    fn capture_for_scroll(&mut self, bundle_id: &str) -> Result<ManagedWindow, String> {
        self.capture(bundle_id)
    }
    fn capture_for_window_key(&mut self, bundle_id: &str) -> Result<ManagedWindow, String> {
        self.capture(bundle_id)
    }
    fn read(&mut self, target: &ManagedWindowIdentity) -> Result<ManagedWindow, String>;
    fn read_container(&mut self, target: &ManagedWindowIdentity) -> Result<ManagedWindow, String> {
        self.read(target)
    }
    fn read_control(
        &mut self,
        target: &ManagedWindowIdentity,
        control: u64,
    ) -> Result<ManagedControl, String> {
        read(self, target)?
            .controls
            .into_iter()
            .find(|item| item.id == control)
            .ok_or_else(|| blocked("ax_control_removed"))
    }
    fn press(&mut self, target: &ManagedWindowIdentity, control: u64) -> Result<(), String>;
    fn focus(&mut self, target: &ManagedWindowIdentity, control: u64) -> Result<(), String>;
    fn scroll(
        &mut self,
        target: &ManagedWindowIdentity,
        request: &DialScrollRequest,
    ) -> Result<(), String>;
    fn shortcut(
        &mut self,
        target: &ManagedWindowIdentity,
        control: u64,
        key: u16,
        modifiers: &[String],
    ) -> Result<(), String>;
    fn insert_text(
        &mut self,
        target: &ManagedWindowIdentity,
        control: u64,
        text: &str,
    ) -> Result<(), String>;
    fn submit(&mut self, target: &ManagedWindowIdentity, control: u64) -> Result<(), String>;
    fn send_window_key(
        &mut self,
        _target: &ManagedWindowIdentity,
        _key_code: u16,
        _modifiers: &[String],
    ) -> Result<(), String> {
        Err(blocked("window_key_unavailable"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexReasoningAction {
    DecreaseEffort,
    IncreaseEffort,
    ToggleFastMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelPickerConfirmResult {
    Confirmed,
    PopupAlreadyClosed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentOpenResult {
    PopupOpened,
    OpenedDirectly,
}

const MODEL_PICKER_FALLBACK_TTL: Duration = Duration::from_secs(12);
/// Short pre-mutation discovery window for Antigravity pages without side-panel/browser controls
/// (e.g., Home / New Conversation), so `open_browser` fails fast instead of polling the full
/// 3500ms post-mutation readback deadline.
const BROWSER_PRE_MUTATION_DISCOVERY_TIMEOUT: Duration = Duration::from_millis(250);

fn is_codex_trigger_excluded(label: &str) -> bool {
    let lower = label.trim().to_lowercase();
    let excluded = [
        "actions",
        "options",
        "menu",
        "switch mode",
        "rate response",
        "more actions",
        "regenerate response",
        "change permissions",
        "attach files",
    ];
    excluded.iter().any(|ex| lower.contains(ex))
}

pub(crate) fn is_codex_explicit_effort_trigger(label: &str) -> bool {
    if is_codex_trigger_excluded(label) {
        return false;
    }
    let trimmed = label.trim();
    if trimmed.eq_ignore_ascii_case("select effort") {
        return true;
    }
    let standalone = ["minimal", "low", "medium", "high", "extra high", "max"];
    if standalone
        .iter()
        .any(|level| trimmed.eq_ignore_ascii_case(level))
    {
        return true;
    }
    let suffixes = [
        " Minimal",
        " Low",
        " Medium",
        " High",
        " Extra High",
        " Max",
    ];
    suffixes.iter().any(|suffix| trimmed.ends_with(suffix))
}

pub(crate) fn is_codex_fallback_reasoning_trigger(label: &str) -> bool {
    if is_codex_trigger_excluded(label) {
        return false;
    }
    let trimmed = label.trim();
    if trimmed.eq_ignore_ascii_case("select model") {
        return true;
    }
    let prefixes = ["GPT-", "o1", "o3", "o4", "codex-"];
    if prefixes.iter().any(|prefix| trimmed.starts_with(prefix)) {
        return true;
    }
    trimmed.ends_with(" Standard")
}

pub(crate) fn is_codex_reasoning_trigger(label: &str) -> bool {
    is_codex_explicit_effort_trigger(label) || is_codex_fallback_reasoning_trigger(label)
}

pub(crate) fn is_codex_popover_menu_item(label: &str) -> bool {
    let trimmed = label.trim();
    let allowed = [
        "select model",
        "enable fast mode",
        "enable standard mode",
        "reset to default",
        "power",
    ];
    allowed
        .iter()
        .any(|target| trimmed.eq_ignore_ascii_case(target))
}

pub(crate) fn is_codex_model_picker_trigger(label: &str) -> bool {
    if is_codex_trigger_excluded(label) {
        return false;
    }
    let trimmed = label.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto approve") {
        return false;
    }
    if is_codex_fallback_reasoning_trigger(label) {
        return true;
    }
    let exact = [
        "model",
        "select model",
        "choose model",
        "switch model",
        "模型",
        "选择模型",
        "切换模型",
    ];
    if exact.iter().any(|item| trimmed.eq_ignore_ascii_case(item)) {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    let prefixes = [
        "gpt-", "claude", "gemini", "deepseek", "o1", "o3", "o4", "codex-", "auto ",
    ];
    prefixes.iter().any(|prefix| lower.starts_with(prefix))
}

pub(crate) fn is_dsh_model_picker_trigger(label: &str) -> bool {
    if is_codex_trigger_excluded(label) {
        return false;
    }
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_lowercase();
    if lower.contains("subagent") || trimmed.contains("子智能体") {
        return false;
    }
    let exact = [
        "model",
        "select model",
        "choose model",
        "switch model",
        "effort",
        "select effort",
        "模型",
        "选择模型",
        "切换模型",
        "深度思考",
        "思考力度",
        "推理力度",
    ];
    if exact.iter().any(|item| trimmed.eq_ignore_ascii_case(item)) {
        return true;
    }
    if lower.starts_with("select model")
        || lower.starts_with("choose model")
        || lower.starts_with("switch model")
        || trimmed.starts_with("选择模型")
        || trimmed.starts_with("切换模型")
    {
        return true;
    }
    let prefixes = [
        "deepseek",
        "claude",
        "gemini",
        "gpt-",
        "qwen",
        "r1",
        "v3",
        "model:",
        "模型:",
        "模型：",
    ];
    prefixes.iter().any(|prefix| lower.starts_with(prefix))
}

pub(crate) fn is_antigravity_model_picker_trigger(label: &str) -> bool {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_lowercase();
    let excluded = [
        "actions",
        "options",
        "menu",
        "switch mode",
        "agent permissions",
        "add context",
        "customize sections",
        "notifications",
        "extensions",
        "subagent",
    ];
    if excluded.iter().any(|ex| lower.contains(ex)) {
        return false;
    }
    trimmed.eq_ignore_ascii_case("model")
        || lower.starts_with("select model")
        || lower.starts_with("choose model")
        || lower.starts_with("switch model")
        || trimmed == "模型"
        || trimmed.starts_with("选择模型")
        || trimmed.starts_with("切换模型")
}

pub(crate) fn is_antigravity_subagent_popup_trigger(label: &str) -> bool {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_lowercase();
    if lower.contains("stop subagent")
        || lower.contains("cancel subagent")
        || lower.starts_with("open subagent:")
        || lower.starts_with("close ")
        || trimmed.starts_with("打开子智能体")
        || trimmed.contains("停止子智能体")
    {
        return false;
    }
    lower.contains("subagent") || trimmed.contains("子智能体")
}

pub(crate) fn is_antigravity_overview_subagents_button(label: &str) -> bool {
    let trimmed = label.trim();
    let lower = trimmed.to_lowercase();
    lower == "subagents"
        || lower.starts_with("subagents ")
        || trimmed == "子智能体"
        || trimmed.starts_with("子智能体 ")
}

pub(crate) fn is_antigravity_open_subagent_button(label: &str) -> bool {
    let trimmed = label.trim();
    let lower = trimmed.to_lowercase();
    lower.starts_with("open subagent:") || trimmed.starts_with("打开子智能体")
}

pub(crate) fn is_dsh_subagent_picker_trigger(label: &str) -> bool {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_lowercase();
    let excluded = [
        "stop subagent",
        "cancel subagent",
        "open in sidebar",
        "在侧边栏打开",
    ];
    if excluded.iter().any(|ex| lower.contains(ex)) {
        return false;
    }
    lower.contains("subagent") || trimmed.contains("子智能体")
}

pub(crate) fn is_codex_subagent_picker_trigger(label: &str) -> bool {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_lowercase();
    let excluded = ["stop subagent", "cancel subagent"];
    if excluded.iter().any(|ex| lower.contains(ex)) {
        return false;
    }
    lower.contains("subagent")
        || lower.contains("switch agent")
        || lower.contains("select agent")
        || trimmed.contains("子智能体")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObservedModelPickerPopupState {
    Open,
    Closed,
    Unknown,
}

fn select_unique_model_trigger(
    window: &ManagedWindow,
    matches_label: fn(&str) -> bool,
) -> Result<Option<&ManagedControl>, String> {
    let popup_candidates: Vec<&ManagedControl> = window
        .controls
        .iter()
        .filter(|c| {
            c.enabled && c.pressable && c.role == "AXPopUpButton" && matches_label(&c.label)
        })
        .collect();
    if let [trigger] = popup_candidates.as_slice() {
        return Ok(Some(*trigger));
    }
    if popup_candidates.len() > 1 {
        return Err(blocked("ambiguous_model_picker_control"));
    }
    let button_candidates: Vec<&ManagedControl> = window
        .controls
        .iter()
        .filter(|c| c.enabled && c.pressable && c.role == "AXButton" && matches_label(&c.label))
        .collect();
    if let [trigger] = button_candidates.as_slice() {
        return Ok(Some(*trigger));
    }
    if button_candidates.len() > 1 {
        return Err(blocked("ambiguous_model_picker_control"));
    }
    Ok(None)
}

fn has_codex_popover_menu_items(window: &ManagedWindow) -> bool {
    window
        .controls
        .iter()
        .any(|c| c.role == "AXMenuItem" && is_codex_popover_menu_item(&c.label))
}

fn unique_codex_model_trigger(window: &ManagedWindow) -> Option<&ManagedControl> {
    let popup_candidates: Vec<&ManagedControl> = window
        .controls
        .iter()
        .filter(|c| {
            c.enabled
                && c.pressable
                && c.role == "AXPopUpButton"
                && is_codex_model_picker_trigger(&c.label)
        })
        .collect();
    if let [only] = popup_candidates.as_slice() {
        return Some(*only);
    }
    if !popup_candidates.is_empty() {
        return None;
    }
    let button_candidates: Vec<&ManagedControl> = window
        .controls
        .iter()
        .filter(|c| {
            c.enabled
                && c.pressable
                && c.role == "AXButton"
                && is_codex_model_picker_trigger(&c.label)
        })
        .collect();
    if let [only] = button_candidates.as_slice() {
        return Some(*only);
    }
    if !button_candidates.is_empty() {
        return None;
    }
    let fallback_reasoning: Vec<&ManagedControl> = window
        .controls
        .iter()
        .filter(|c| {
            c.enabled
                && c.pressable
                && c.role == "AXPopUpButton"
                && is_codex_reasoning_trigger(&c.label)
        })
        .collect();
    if let [only] = fallback_reasoning.as_slice() {
        Some(*only)
    } else {
        None
    }
}

fn is_codex_model_popover_visible(window: &ManagedWindow) -> bool {
    has_codex_popover_menu_items(window)
        || unique_codex_model_trigger(window).is_some_and(|c| c.expanded == Some(true))
}

fn observe_model_picker_popup_state(
    window: &ManagedWindow,
    adapter: DictationAdapter,
) -> Result<ObservedModelPickerPopupState, String> {
    match adapter {
        DictationAdapter::Antigravity => {
            let trigger = select_unique_model_trigger(window, is_antigravity_model_picker_trigger)?;
            Ok(match trigger.and_then(|c| c.expanded) {
                Some(true) => ObservedModelPickerPopupState::Open,
                Some(false) => ObservedModelPickerPopupState::Closed,
                None => ObservedModelPickerPopupState::Unknown,
            })
        }
        DictationAdapter::Dsh => {
            let trigger = select_unique_model_trigger(window, is_dsh_model_picker_trigger)?;
            Ok(match trigger.and_then(|c| c.expanded) {
                Some(true) => ObservedModelPickerPopupState::Open,
                Some(false) => ObservedModelPickerPopupState::Closed,
                None => ObservedModelPickerPopupState::Unknown,
            })
        }
        DictationAdapter::Codex => {
            if is_codex_model_popover_visible(window) {
                return Ok(ObservedModelPickerPopupState::Open);
            }
            if unique_codex_model_trigger(window).and_then(|c| c.expanded) == Some(false) {
                return Ok(ObservedModelPickerPopupState::Closed);
            }
            Ok(ObservedModelPickerPopupState::Unknown)
        }
    }
}

fn confirm_popup_expansion_with_reason<P: ManagedAxProvider>(
    provider: &mut P,
    target: &ManagedWindowIdentity,
    control_id: u64,
    timeout: Duration,
    unconfirmed_reason: &str,
) -> Result<bool, String> {
    let deadline = Instant::now() + timeout;
    loop {
        let ctrl = provider
            .read_control(target, control_id)
            .map_err(|error| format!("unconfirmed:{error}"))?;
        match ctrl.expanded {
            Some(true) => return Ok(true),
            None => return Ok(false),
            Some(false) => {
                if Instant::now() >= deadline {
                    return Err(format!("unconfirmed:{unconfirmed_reason}"));
                }
                std::thread::sleep(Duration::from_millis(15));
            }
        }
    }
}

fn confirm_popup_expansion<P: ManagedAxProvider>(
    provider: &mut P,
    target: &ManagedWindowIdentity,
    control_id: u64,
    timeout: Duration,
) -> Result<bool, String> {
    confirm_popup_expansion_with_reason(
        provider,
        target,
        control_id,
        timeout,
        "model_picker_open_readback",
    )
}

fn poll_optional_popup_expansion<P: ManagedAxProvider>(
    provider: &mut P,
    target: &ManagedWindowIdentity,
    control_id: u64,
    timeout: Duration,
) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let Ok(ctrl) = provider.read_control(target, control_id) else {
            return false;
        };
        match ctrl.expanded {
            Some(true) => return true,
            None => return false,
            Some(false) => {
                if Instant::now() >= deadline {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(15));
            }
        }
    }
}

fn select_latest_subagent_trigger(
    window: &ManagedWindow,
    matches_popup_label: fn(&str) -> bool,
    matches_button_label: Option<fn(&str) -> bool>,
) -> Option<&ManagedControl> {
    if let Some(popup) = window.controls.iter().rfind(|c| {
        c.enabled && c.pressable && c.role == "AXPopUpButton" && matches_popup_label(&c.label)
    }) {
        return Some(popup);
    }
    let button_matcher = matches_button_label?;
    window
        .controls
        .iter()
        .rfind(|c| c.enabled && c.pressable && c.role == "AXButton" && button_matcher(&c.label))
}

fn observe_subagent_picker_popup_state(
    window: &ManagedWindow,
    adapter: DictationAdapter,
) -> ObservedModelPickerPopupState {
    let trigger = match adapter {
        DictationAdapter::Antigravity => {
            select_latest_subagent_trigger(window, is_antigravity_subagent_popup_trigger, None)
        }
        DictationAdapter::Dsh => select_latest_subagent_trigger(
            window,
            is_dsh_subagent_picker_trigger,
            Some(is_dsh_subagent_picker_trigger),
        ),
        DictationAdapter::Codex => select_latest_subagent_trigger(
            window,
            is_codex_subagent_picker_trigger,
            Some(is_codex_subagent_picker_trigger),
        ),
    };
    match trigger.and_then(|c| c.expanded) {
        Some(true) => ObservedModelPickerPopupState::Open,
        Some(false) => ObservedModelPickerPopupState::Closed,
        None => ObservedModelPickerPopupState::Unknown,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActivePickerKind {
    Model,
    Subagent,
}

struct ActivePickerTarget {
    kind: ActivePickerKind,
    adapter: DictationAdapter,
    identity: ManagedWindowIdentity,
    updated_at: Instant,
    ax_open_confirmed: bool,
}

struct DismissRequest {
    target: Option<ManagedWindowIdentity>,
    generation: u64,
    deadline: Option<Instant>,
    shutdown: bool,
    owned_by_us: bool,
}

struct DismissState {
    lock: Mutex<DismissRequest>,
    cvar: Condvar,
}

pub struct ManagedAxControls<P: 'static> {
    provider: Arc<Mutex<P>>,
    readback_timeout: Duration,
    auto_dismiss_delay: Duration,
    dictation_leases: Mutex<DictationLeaseRegistry>,
    active_picker_target: Mutex<Option<ActivePickerTarget>>,
    dismiss_state: Arc<DismissState>,
    worker_handle: Option<std::thread::JoinHandle<()>>,
}

impl<P> std::fmt::Debug for ManagedAxControls<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ManagedAxControls")
            .field("readback_timeout", &self.readback_timeout)
            .field("auto_dismiss_delay", &self.auto_dismiss_delay)
            .finish()
    }
}

impl<P: 'static> Drop for ManagedAxControls<P> {
    fn drop(&mut self) {
        {
            let mut state = self.dismiss_state.lock.lock().unwrap();
            state.shutdown = true;
            self.dismiss_state.cvar.notify_all();
        }
        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }
    }
}

impl<P: ManagedAxProvider + 'static> ManagedAxControls<P> {
    pub fn new(provider: P) -> Self {
        Self::with_timeouts(
            provider,
            Duration::from_millis(900),
            Duration::from_millis(1500),
        )
    }

    pub fn with_readback_timeout(provider: P, readback_timeout: Duration) -> Self {
        Self::with_timeouts(provider, readback_timeout, Duration::from_millis(1500))
    }

    pub fn with_auto_dismiss_delay(provider: P, auto_dismiss_delay: Duration) -> Self {
        Self::with_timeouts(provider, Duration::from_millis(900), auto_dismiss_delay)
    }

    pub fn with_timeouts(
        provider: P,
        readback_timeout: Duration,
        auto_dismiss_delay: Duration,
    ) -> Self {
        let provider = Arc::new(Mutex::new(provider));
        let dismiss_state = Arc::new(DismissState {
            lock: Mutex::new(DismissRequest {
                target: None,
                generation: 0,
                deadline: None,
                shutdown: false,
                owned_by_us: false,
            }),
            cvar: Condvar::new(),
        });
        let worker_provider = Arc::clone(&provider);
        let worker_state = Arc::clone(&dismiss_state);
        let worker_handle = std::thread::spawn(move || loop {
            let mut state = worker_state.lock.lock().unwrap();
            if state.shutdown {
                break;
            }
            match state.deadline {
                None => {
                    state = worker_state.cvar.wait(state).unwrap();
                }
                Some(deadline) => {
                    let now = Instant::now();
                    if now < deadline {
                        let wait_time = deadline - now;
                        let (next_state, _) =
                            worker_state.cvar.wait_timeout(state, wait_time).unwrap();
                        state = next_state;
                    } else {
                        let gen = state.generation;
                        let target = state.target.clone();
                        state.deadline = None;
                        drop(state);

                        if let Some(target) = target {
                            let mut p = worker_provider.lock().unwrap();
                            let current = worker_state.lock.lock().unwrap();
                            if current.generation == gen && !current.shutdown {
                                drop(current);
                                if p.process_has_exited(target.pid) {
                                    let mut current = worker_state.lock.lock().unwrap();
                                    if current.generation == gen && !current.shutdown {
                                        current.owned_by_us = false;
                                        current.target = None;
                                    }
                                } else if let Ok(window) = p.read(&target) {
                                    let has_popover = window.controls.iter().any(|c| {
                                        c.role == "AXMenuItem"
                                            && is_codex_popover_menu_item(&c.label)
                                    });
                                    if has_popover {
                                        let current = worker_state.lock.lock().unwrap();
                                        if current.generation == gen && !current.shutdown {
                                            drop(current);
                                            let _ = p.send_window_key(&target, 0x35, &[]);
                                            let mut current = worker_state.lock.lock().unwrap();
                                            if current.generation == gen && !current.shutdown {
                                                current.owned_by_us = false;
                                                current.target = None;
                                            }
                                        }
                                    } else {
                                        let mut current = worker_state.lock.lock().unwrap();
                                        if current.generation == gen && !current.shutdown {
                                            current.owned_by_us = false;
                                            current.target = None;
                                        }
                                    }
                                } else {
                                    let mut current = worker_state.lock.lock().unwrap();
                                    if current.generation == gen && !current.shutdown {
                                        current.owned_by_us = false;
                                        current.target = None;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });
        Self {
            provider,
            readback_timeout,
            auto_dismiss_delay,
            dictation_leases: Mutex::new(DictationLeaseRegistry::default()),
            active_picker_target: Mutex::new(None),
            dismiss_state,
            worker_handle: Some(worker_handle),
        }
    }

    pub fn auto_dismiss_delay(&self) -> Duration {
        self.auto_dismiss_delay
    }

    fn begin_codex_reasoning_turn(&self) -> bool {
        let mut state = self.dismiss_state.lock.lock().unwrap();
        state.generation = state.generation.wrapping_add(1);
        state.deadline = None;
        self.dismiss_state.cvar.notify_all();
        state.owned_by_us
    }

    fn set_codex_popover_owned(&self, owned: bool) {
        let mut state = self.dismiss_state.lock.lock().unwrap();
        state.generation = state.generation.wrapping_add(1);
        state.deadline = None;
        state.owned_by_us = owned;
        if !owned {
            state.target = None;
        }
        self.dismiss_state.cvar.notify_all();
    }

    fn schedule_auto_dismiss(&self, target: &ManagedWindowIdentity) {
        let mut state = self.dismiss_state.lock.lock().unwrap();
        state.generation = state.generation.wrapping_add(1);
        state.target = Some(target.clone());
        state.owned_by_us = true;
        state.deadline = Some(Instant::now() + self.auto_dismiss_delay);
        self.dismiss_state.cvar.notify_all();
    }

    pub fn inspect(&self, adapter: &str) -> Result<ManagedControlReadiness, String> {
        let adapter = managed_adapter(adapter)?;
        let mut provider = self
            .provider
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        let window = capture(&mut *provider, adapter)?;
        Ok(ManagedControlReadiness {
            editable_inputs: window
                .controls
                .iter()
                .filter(|control| {
                    control.enabled && control.editable && control.role == "AXTextArea"
                })
                .count(),
            dictation: dictation_control(&window, adapter).map(|(state, _)| state),
        })
    }

    pub fn toggle_dictation(&self, adapter: &str) -> Result<ObservedDictationState, String> {
        let adapter = managed_adapter(adapter)?;
        let mut provider = self
            .provider
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        let window = capture(&mut *provider, adapter)?;
        let mut leases = self
            .dictation_leases
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        leases.reconcile_for_captured_window(&mut *provider, &window, adapter)?;
        let (state, control) = dictation_control(&window, adapter)?;
        let desired = match state {
            ObservedDictationState::Idle => ObservedDictationState::Recording,
            ObservedDictationState::Recording => ObservedDictationState::Idle,
        };
        let result = press_and_confirm(
            &mut *provider,
            &window.identity,
            adapter,
            control,
            desired,
            self.readback_timeout,
        );
        if result.is_ok() && desired == ObservedDictationState::Idle {
            leases.release_claim(&mut *provider, &window.identity.bundle_id);
        } else if result.is_ok()
            || result
                .as_ref()
                .is_err_and(|error| error.starts_with("unconfirmed:"))
        {
            leases.retain_claim(&mut *provider, &window.identity)?;
        }
        result?;
        Ok(desired)
    }

    pub fn toggle_live_voice(&self, adapter: &str) -> Result<(), String> {
        if managed_adapter(adapter)? != DictationAdapter::Antigravity {
            return Err(blocked("unsupported_live_voice_adapter"));
        }
        let mut provider = self
            .provider
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        let window = capture(&mut *provider, DictationAdapter::Antigravity)?;
        let (active, control) = live_voice_control(&window)?;
        provider.press(&window.identity, control)?;
        let deadline = Instant::now() + self.readback_timeout;
        loop {
            let current = read(&mut *provider, &window.identity)
                .map_err(|error| format!("unconfirmed:{error}"))?;
            if live_voice_control(&current).is_ok_and(|(now_active, _)| now_active != active) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err("unconfirmed:live_voice_readback".into());
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    pub fn stop_dictation(&self, adapter: &str) -> Result<bool, String> {
        let adapter = managed_adapter(adapter)?;
        let mut provider = self
            .provider
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        let window = capture(&mut *provider, adapter)?;
        self.stop_in_window(&mut *provider, &window, adapter)
    }

    pub fn start_dictation(&self, adapter: &str) -> Result<(), String> {
        let adapter = managed_adapter(adapter)?;
        let mut provider = self
            .provider
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        let window = capture(&mut *provider, adapter)?;
        let mut leases = self
            .dictation_leases
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        leases.reconcile_for_captured_window(&mut *provider, &window, adapter)?;
        let (state, control) = dictation_control(&window, adapter)?;
        if state != ObservedDictationState::Idle {
            return Err(blocked("dictation_already_recording"));
        }
        let result = press_and_confirm(
            &mut *provider,
            &window.identity,
            adapter,
            control,
            ObservedDictationState::Recording,
            self.readback_timeout,
        );
        if result.is_ok()
            || result
                .as_ref()
                .is_err_and(|error| error.starts_with("unconfirmed:"))
        {
            leases.retain_claim(&mut *provider, &window.identity)?;
        }
        result
    }

    pub fn stop_owned_dictations(&self) -> Result<(), String> {
        let mut provider = self
            .provider
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        let mut leases = self
            .dictation_leases
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        leases.stop_all_owned(&mut *provider, self.readback_timeout)
    }

    fn stop_in_window(
        &self,
        provider: &mut P,
        window: &ManagedWindow,
        adapter: DictationAdapter,
    ) -> Result<bool, String> {
        let mut leases = self
            .dictation_leases
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        leases.reconcile_for_captured_window(provider, window, adapter)?;
        let allow_absent = !leases.has_claim(&window.identity.bundle_id);
        let result = stop_in_window(
            provider,
            window,
            adapter,
            self.readback_timeout,
            allow_absent,
        );
        if result.is_ok() {
            leases.release_claim(provider, &window.identity.bundle_id);
        } else if result
            .as_ref()
            .is_err_and(|error| error.starts_with("unconfirmed:"))
        {
            leases.retain_claim(provider, &window.identity)?;
        }
        result
    }

    pub fn focus_input(&self, adapter: &str) -> Result<(), String> {
        let adapter = managed_adapter(adapter)?;
        let mut provider = self
            .provider
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        let window = capture(&mut *provider, adapter)?;
        focus_in_window(&mut *provider, &window.identity, self.readback_timeout).map(|_| ())
    }

    pub fn set_side_panel_open(&self, adapter: &str, open: bool) -> Result<(), String> {
        if managed_adapter(adapter)? != DictationAdapter::Antigravity {
            return Err(blocked("unsupported_side_panel_adapter"));
        }
        let mut provider = self
            .provider
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        let window =
            self.capture_full_window_for_key(&mut *provider, DictationAdapter::Antigravity)?;
        let btn = window
            .controls
            .iter()
            .find(|c| c.role == "AXButton" && c.label.trim() == "Toggle Side Panel")
            .ok_or_else(|| blocked("side_panel_control_not_found"))?;
        if !btn.enabled || !btn.pressable {
            return Err(blocked("side_panel_unavailable"));
        }
        let btn_id = btn.id;
        let current_open = btn
            .expanded
            .ok_or_else(|| blocked("side_panel_state_unknown"))?;
        if current_open == open {
            return Ok(());
        }
        provider.press(&window.identity, btn_id)?;
        let timeout = if self.readback_timeout.is_zero() {
            Duration::from_millis(900)
        } else {
            self.readback_timeout
        };
        let deadline = Instant::now() + timeout;
        let is_transient_removal = |err: &str| {
            matches!(
                err,
                "blocked:ax_control_removed" | "blocked:ax_target_missing"
            )
        };
        loop {
            let observed_expanded = match provider.read_control(&window.identity, btn_id) {
                Ok(ctrl) => ctrl.expanded,
                Err(err) if is_transient_removal(&err) => {
                    let refreshed = read(&mut *provider, &window.identity)
                        .map_err(|e| format!("unconfirmed:{e}"))?;
                    refreshed
                        .controls
                        .iter()
                        .find(|c| c.role == "AXButton" && c.label.trim() == "Toggle Side Panel")
                        .and_then(|c| c.expanded)
                }
                Err(error) => return Err(format!("unconfirmed:{error}")),
            };
            if observed_expanded == Some(open) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "unconfirmed:{}",
                    if open {
                        "side_panel_open"
                    } else {
                        "side_panel_close"
                    }
                ));
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    pub fn open_side_question(&self, adapter: &str) -> Result<(), String> {
        if managed_adapter(adapter)? != DictationAdapter::Antigravity {
            return Err(blocked("unsupported_side_question_adapter"));
        }
        let mut provider = self
            .provider
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        let window = provider.capture_full_window("com.google.antigravity")?;
        provider.activate_window(&window.identity)?;

        let timeout = if self.readback_timeout.is_zero() {
            Duration::from_millis(1200)
        } else if self.readback_timeout < Duration::from_millis(100) {
            self.readback_timeout * 2
        } else {
            self.readback_timeout.max(Duration::from_millis(1200))
        };
        let deadline = Instant::now() + timeout;
        let mut pressed_toggle_panel = false;
        let mut pressed_new_tab = false;
        let mut pressed_side_question = false;
        let mut pressed_new_side_question = false;

        let is_transient_removal = |err: &str| {
            matches!(
                err,
                "blocked:ax_control_removed" | "blocked:ax_target_missing"
            )
        };

        loop {
            let pressed_any = pressed_toggle_panel
                || pressed_new_tab
                || pressed_side_question
                || pressed_new_side_question;

            if Instant::now() >= deadline {
                if pressed_any {
                    return Err("unconfirmed:side_question_open".into());
                } else {
                    return Err(blocked("side_question_control_not_found"));
                }
            }

            let current = match read(&mut *provider, &window.identity) {
                Ok(window) => window,
                Err(error) if pressed_any => return Err(format!("unconfirmed:{error}")),
                Err(error) => return Err(error),
            };

            let side_question_input = current.controls.iter().find(|c| {
                c.enabled
                    && c.editable
                    && matches!(c.role.as_str(), "AXTextArea" | "AXTextField")
                    && is_side_question_input(c)
            });

            if let Some(input) = side_question_input {
                let id = input.id;
                if let Err(err) = provider.focus(&window.identity, id) {
                    if pressed_any && is_transient_removal(&err) && Instant::now() < deadline {
                        std::thread::sleep(Duration::from_millis(25));
                        continue;
                    }
                    return Err(err);
                }
                let focus_timeout = if self.readback_timeout.is_zero() {
                    Duration::from_millis(1200)
                } else {
                    self.readback_timeout.max(Duration::from_millis(300))
                };
                let focus_deadline = Instant::now() + focus_timeout;
                loop {
                    let control = provider
                        .read_control(&window.identity, id)
                        .map_err(|error| format!("unconfirmed:{error}"))?;
                    if control.id == id && control.editable && control.focused {
                        return Ok(());
                    }
                    if Instant::now() >= focus_deadline {
                        return Err("unconfirmed:input_focus".into());
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
            }

            let toggle_panel_btn = current.controls.iter().find(|c| {
                c.role == "AXButton"
                    && c.label.trim() == "Toggle Side Panel"
                    && c.enabled
                    && c.pressable
            });

            if let Some(btn) = current.controls.iter().find(|c| {
                c.role == "AXButton"
                    && c.label.trim() == "New Side Question"
                    && c.enabled
                    && c.pressable
            }) {
                if !pressed_new_side_question {
                    match provider.press(&window.identity, btn.id) {
                        Ok(()) => pressed_new_side_question = true,
                        Err(err)
                            if pressed_any
                                && is_transient_removal(&err)
                                && Instant::now() < deadline => {}
                        Err(err) => return Err(err),
                    }
                }
                std::thread::sleep(Duration::from_millis(25));
            } else if let Some(btn) = current
                .controls
                .iter()
                .find(|c| c.role == "AXButton" && c.label.trim() == "Side Question")
            {
                if !btn.enabled || !btn.pressable {
                    return Err(blocked("side_question_unavailable"));
                }
                if !pressed_side_question {
                    match provider.press(&window.identity, btn.id) {
                        Ok(()) => pressed_side_question = true,
                        Err(err)
                            if pressed_any
                                && is_transient_removal(&err)
                                && Instant::now() < deadline => {}
                        Err(err) => return Err(err),
                    }
                }
                std::thread::sleep(Duration::from_millis(25));
            } else if let Some(btn) = toggle_panel_btn.filter(|btn| btn.expanded == Some(false)) {
                if !pressed_toggle_panel {
                    match provider.press(&window.identity, btn.id) {
                        Ok(()) => pressed_toggle_panel = true,
                        Err(err)
                            if pressed_any
                                && is_transient_removal(&err)
                                && Instant::now() < deadline => {}
                        Err(err) => return Err(err),
                    }
                }
                std::thread::sleep(Duration::from_millis(25));
            } else if let Some(btn) = current.controls.iter().find(|c| {
                c.role == "AXButton" && c.label.trim() == "New tab" && c.enabled && c.pressable
            }) {
                if !pressed_new_tab {
                    match provider.press(&window.identity, btn.id) {
                        Ok(()) => pressed_new_tab = true,
                        Err(err)
                            if pressed_any
                                && is_transient_removal(&err)
                                && Instant::now() < deadline => {}
                        Err(err) => return Err(err),
                    }
                }
                std::thread::sleep(Duration::from_millis(25));
            } else if let Some(btn) = toggle_panel_btn.filter(|btn| btn.expanded != Some(true)) {
                if !pressed_toggle_panel {
                    match provider.press(&window.identity, btn.id) {
                        Ok(()) => pressed_toggle_panel = true,
                        Err(err)
                            if pressed_any
                                && is_transient_removal(&err)
                                && Instant::now() < deadline => {}
                        Err(err) => return Err(err),
                    }
                }
                std::thread::sleep(Duration::from_millis(25));
            } else {
                std::thread::sleep(Duration::from_millis(25));
            }
        }
    }

    pub fn open_browser(&self, adapter: &str) -> Result<(), String> {
        if managed_adapter(adapter)? != DictationAdapter::Antigravity {
            return Err(blocked("unsupported_browser_adapter"));
        }
        let mut provider = self
            .provider
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        let window =
            self.capture_full_window_for_key(&mut *provider, DictationAdapter::Antigravity)?;
        let identity = window.identity.clone();
        provider.activate_window(&identity)?;

        let timeout = if self.readback_timeout.is_zero() {
            Duration::from_millis(3500)
        } else if self.readback_timeout < Duration::from_millis(100) {
            self.readback_timeout * 2
        } else {
            self.readback_timeout.max(Duration::from_millis(3500))
        };
        let started = Instant::now();
        let deadline = started + timeout;
        let discovery_deadline = started + timeout.min(BROWSER_PRE_MUTATION_DISCOVERY_TIMEOUT);
        let mut initial_window = Some(window);
        let mut pressed_toggle_panel = false;
        let mut pressed_new_tab = false;
        let mut pressed_clear_search = false;
        let mut pressed_browser = false;

        let is_transient_removal = |err: &str| {
            matches!(
                err,
                "blocked:ax_control_removed" | "blocked:ax_target_missing"
            )
        };
        let wrap_post_mutation = |err: String, pressed_any: bool| {
            if pressed_any && !err.starts_with("unconfirmed:") {
                format!("unconfirmed:{err}")
            } else {
                err
            }
        };
        let try_press = |provider: &mut P,
                         flag: &mut bool,
                         control_id: u64,
                         pressed_any: bool|
         -> Result<(), String> {
            if *flag {
                return Ok(());
            }
            match provider.press(&identity, control_id) {
                Ok(()) => {
                    *flag = true;
                    Ok(())
                }
                Err(err)
                    if pressed_any && is_transient_removal(&err) && Instant::now() < deadline =>
                {
                    Ok(())
                }
                Err(err) => Err(wrap_post_mutation(err, pressed_any)),
            }
        };

        loop {
            let pressed_any =
                pressed_toggle_panel || pressed_new_tab || pressed_clear_search || pressed_browser;
            let active_deadline = if pressed_any {
                deadline
            } else {
                discovery_deadline
            };

            if initial_window.is_none() && Instant::now() >= active_deadline {
                if pressed_any {
                    return Err("unconfirmed:browser_open".into());
                } else {
                    return Err(blocked("browser_control_not_found"));
                }
            }

            let current = match initial_window.take() {
                Some(snapshot) => snapshot,
                None => match read(&mut *provider, &identity) {
                    Ok(snapshot) => snapshot,
                    Err(error) => return Err(wrap_post_mutation(error, pressed_any)),
                },
            };

            let toggle_panel_btn = current
                .controls
                .iter()
                .find(|c| c.role == "AXButton" && c.label.trim() == "Toggle Side Panel");
            let panel_collapsed = toggle_panel_btn.is_some_and(|btn| btn.expanded == Some(false));

            if panel_collapsed {
                let btn = toggle_panel_btn.expect("panel_collapsed implies toggle_panel_btn");
                if !pressed_toggle_panel {
                    if !btn.enabled || !btn.pressable {
                        return Err(wrap_post_mutation(
                            blocked("browser_unavailable"),
                            pressed_any,
                        ));
                    }
                    try_press(
                        &mut provider,
                        &mut pressed_toggle_panel,
                        btn.id,
                        pressed_any,
                    )?;
                }
                std::thread::sleep(Duration::from_millis(25));
                continue;
            }

            if is_antigravity_browser_open(&current) {
                return Ok(());
            }

            if pressed_browser {
                std::thread::sleep(Duration::from_millis(25));
                continue;
            }

            let has_new_tab_search = current
                .controls
                .iter()
                .any(|c| c.role == "AXTextField" && c.label.trim() == "Search for content to open");

            let browser_btn = current
                .controls
                .iter()
                .find(|c| c.role == "AXButton" && c.label.trim() == "Browser")
                .or_else(|| {
                    has_new_tab_search.then(|| {
                        current
                            .controls
                            .iter()
                            .find(|c| c.role == "AXButton" && c.label.trim() == "Preview")
                    })?
                });

            if let Some(btn) = browser_btn {
                if !btn.enabled || !btn.pressable {
                    return Err(wrap_post_mutation(
                        blocked("browser_unavailable"),
                        pressed_any,
                    ));
                }
                try_press(&mut provider, &mut pressed_browser, btn.id, pressed_any)?;
                std::thread::sleep(Duration::from_millis(25));
            } else if has_new_tab_search {
                if let Some(btn) = current.controls.iter().find(|c| {
                    c.role == "AXButton"
                        && c.label.trim() == "Clear search"
                        && c.enabled
                        && c.pressable
                }) {
                    try_press(
                        &mut provider,
                        &mut pressed_clear_search,
                        btn.id,
                        pressed_any,
                    )?;
                }
                std::thread::sleep(Duration::from_millis(25));
            } else if let Some(btn) = current
                .controls
                .iter()
                .find(|c| c.role == "AXButton" && c.label.trim() == "New tab")
            {
                if !pressed_new_tab {
                    if !btn.enabled || !btn.pressable {
                        return Err(wrap_post_mutation(
                            blocked("browser_unavailable"),
                            pressed_any,
                        ));
                    }
                    try_press(&mut provider, &mut pressed_new_tab, btn.id, pressed_any)?;
                }
                std::thread::sleep(Duration::from_millis(25));
            } else if let Some(btn) = toggle_panel_btn.filter(|btn| btn.expanded != Some(true)) {
                if !pressed_toggle_panel {
                    if !btn.enabled || !btn.pressable {
                        return Err(wrap_post_mutation(
                            blocked("browser_unavailable"),
                            pressed_any,
                        ));
                    }
                    try_press(
                        &mut provider,
                        &mut pressed_toggle_panel,
                        btn.id,
                        pressed_any,
                    )?;
                }
                std::thread::sleep(Duration::from_millis(25));
            } else {
                std::thread::sleep(Duration::from_millis(25));
            }
        }
    }

    pub fn scroll(&self, request: &DialScrollRequest) -> Result<(), String> {
        if !matches!(
            request.target_bundle_id.as_str(),
            "com.google.antigravity" | "com.deepseek.dsh" | "com.openai.codex"
        ) {
            return Err(blocked("unsupported_scroll_target"));
        }
        let mut provider = self
            .provider
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        let window = provider.capture_for_scroll(&request.target_bundle_id)?;
        validate_target(&window.identity, &request.target_bundle_id)?;
        provider.scroll(&window.identity, request)?;
        if request.jump_to_bottom {
            focus_in_window(&mut *provider, &window.identity, self.readback_timeout)?;
        }
        Ok(())
    }

    pub fn submit(&self, adapter: &str, prompt: Option<&str>) -> Result<(), String> {
        let adapter = managed_adapter(adapter)?;
        let mut provider = self
            .provider
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        let window = capture(&mut *provider, adapter)?;
        self.stop_in_window(&mut *provider, &window, adapter)?;
        let control = focus_in_window(&mut *provider, &window.identity, self.readback_timeout)?;
        if let Some(prompt) = prompt {
            provider.insert_text(&window.identity, control, prompt)?;
        }
        // Re-observe immediately before sending; mouse input can start recording too.
        self.stop_in_window(&mut *provider, &window, adapter)?;
        provider.submit(&window.identity, control)
    }
    pub fn shortcut(&self, adapter: &str, key: u16, modifiers: &[String]) -> Result<(), String> {
        let adapter = managed_adapter(adapter)?;
        let mut provider = self
            .provider
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        let window = capture(&mut *provider, adapter)?;
        let control = focus_in_window(&mut *provider, &window.identity, self.readback_timeout)?;
        provider.shortcut(&window.identity, control, key, modifiers)
    }

    pub fn insert_prompt(&self, adapter: &str, prompt: &str) -> Result<(), String> {
        let adapter = managed_adapter(adapter)?;
        let mut provider = self
            .provider
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        let window = capture(&mut *provider, adapter)?;
        let control = focus_in_window(&mut *provider, &window.identity, self.readback_timeout)?;
        provider.insert_text(&window.identity, control, prompt)
    }

    fn active_picker_state(
        &self,
        kind: ActivePickerKind,
        adapter: DictationAdapter,
        identity: &ManagedWindowIdentity,
    ) -> Option<bool> {
        let Ok(mut guard) = self.active_picker_target.lock() else {
            return None;
        };
        let active = guard.as_ref()?;
        if active.updated_at.elapsed() >= MODEL_PICKER_FALLBACK_TTL {
            *guard = None;
            return None;
        }
        (active.kind == kind
            && active.adapter == adapter
            && active.identity.same_container(identity))
        .then_some(active.ax_open_confirmed)
    }

    fn effective_popup_state(
        &self,
        window: &ManagedWindow,
        adapter: DictationAdapter,
    ) -> Result<ObservedModelPickerPopupState, String> {
        match observe_model_picker_popup_state(window, adapter)? {
            ObservedModelPickerPopupState::Open => Ok(ObservedModelPickerPopupState::Open),
            ObservedModelPickerPopupState::Closed => {
                let has_focused_editable = window.controls.iter().any(|c| c.editable && c.focused);
                if adapter == DictationAdapter::Codex
                    && self.active_picker_state(ActivePickerKind::Model, adapter, &window.identity)
                        == Some(false)
                    && !has_focused_editable
                {
                    Ok(ObservedModelPickerPopupState::Unknown)
                } else {
                    Ok(ObservedModelPickerPopupState::Closed)
                }
            }
            ObservedModelPickerPopupState::Unknown => Ok(ObservedModelPickerPopupState::Unknown),
        }
    }

    fn record_active_picker_target(
        &self,
        kind: ActivePickerKind,
        adapter: DictationAdapter,
        identity: &ManagedWindowIdentity,
        ax_open_confirmed: bool,
    ) {
        if let Ok(mut guard) = self.active_picker_target.lock() {
            *guard = Some(ActivePickerTarget {
                kind,
                adapter,
                identity: identity.clone(),
                updated_at: Instant::now(),
                ax_open_confirmed,
            });
        }
    }

    fn clear_active_picker_target(&self) {
        if let Ok(mut guard) = self.active_picker_target.lock() {
            *guard = None;
        }
    }

    fn capture_window_for_key(
        &self,
        provider: &mut P,
        adapter: DictationAdapter,
    ) -> Result<ManagedWindow, String> {
        let bundle = adapter_bundle(adapter);
        let window = provider.capture_for_window_key(bundle)?;
        validate_target(&window.identity, bundle)?;
        Ok(window)
    }

    fn capture_full_window_for_key(
        &self,
        provider: &mut P,
        adapter: DictationAdapter,
    ) -> Result<ManagedWindow, String> {
        let bundle = adapter_bundle(adapter);
        let window = provider.capture_full_window(bundle)?;
        validate_target(&window.identity, bundle)?;
        Ok(window)
    }

    fn press_subagent_trigger_in_window(
        provider: &mut P,
        window: &ManagedWindow,
        trigger: &ManagedControl,
    ) -> Result<(ManagedWindowIdentity, bool), String> {
        let trigger_id = trigger.id;
        let initial_expanded = trigger.expanded;
        let is_popup = trigger.role == "AXPopUpButton";
        provider.activate_window(&window.identity)?;
        let confirmed = if initial_expanded == Some(true) {
            true
        } else {
            provider.press(&window.identity, trigger_id)?;
            if is_popup && initial_expanded == Some(false) {
                confirm_popup_expansion_with_reason(
                    provider,
                    &window.identity,
                    trigger_id,
                    Duration::from_millis(250),
                    "subagent_picker_open_readback",
                )?
            } else {
                false
            }
        };
        Ok((window.identity.clone(), confirmed))
    }

    fn open_subagent_picker_in_window(
        &self,
        provider: &mut P,
        adapter: DictationAdapter,
        window: ManagedWindow,
    ) -> Result<(ManagedWindowIdentity, bool, SubagentOpenResult), String> {
        match adapter {
            DictationAdapter::Antigravity => {
                if let Some(trigger) = select_latest_subagent_trigger(
                    &window,
                    is_antigravity_subagent_popup_trigger,
                    None,
                ) {
                    let (id, confirmed) =
                        Self::press_subagent_trigger_in_window(provider, &window, trigger)?;
                    return Ok((id, confirmed, SubagentOpenResult::PopupOpened));
                }
                let full_window = self.capture_full_window_for_key(provider, adapter)?;
                if let Some(trigger) = select_latest_subagent_trigger(
                    &full_window,
                    is_antigravity_subagent_popup_trigger,
                    None,
                ) {
                    let (id, confirmed) =
                        Self::press_subagent_trigger_in_window(provider, &full_window, trigger)?;
                    return Ok((id, confirmed, SubagentOpenResult::PopupOpened));
                }
                if let Some(overview_btn) = full_window.controls.iter().rfind(|c| {
                    c.enabled
                        && c.pressable
                        && c.role == "AXButton"
                        && is_antigravity_overview_subagents_button(&c.label)
                }) {
                    provider.activate_window(&full_window.identity)?;
                    provider.press(&full_window.identity, overview_btn.id)?;
                    return Ok((
                        full_window.identity,
                        false,
                        SubagentOpenResult::OpenedDirectly,
                    ));
                }
                if let Some(open_btn) = full_window.controls.iter().rfind(|c| {
                    c.enabled
                        && c.pressable
                        && c.role == "AXButton"
                        && is_antigravity_open_subagent_button(&c.label)
                }) {
                    provider.activate_window(&full_window.identity)?;
                    provider.press(&full_window.identity, open_btn.id)?;
                    return Ok((
                        full_window.identity,
                        false,
                        SubagentOpenResult::OpenedDirectly,
                    ));
                }
                Err(blocked("subagent_picker_unavailable"))
            }
            DictationAdapter::Dsh => {
                if let Some(trigger) = select_latest_subagent_trigger(
                    &window,
                    is_dsh_subagent_picker_trigger,
                    Some(is_dsh_subagent_picker_trigger),
                ) {
                    let (id, confirmed) =
                        Self::press_subagent_trigger_in_window(provider, &window, trigger)?;
                    return Ok((id, confirmed, SubagentOpenResult::PopupOpened));
                }
                let full_window = self.capture_full_window_for_key(provider, adapter)?;
                if let Some(trigger) = select_latest_subagent_trigger(
                    &full_window,
                    is_dsh_subagent_picker_trigger,
                    Some(is_dsh_subagent_picker_trigger),
                ) {
                    let (id, confirmed) =
                        Self::press_subagent_trigger_in_window(provider, &full_window, trigger)?;
                    return Ok((id, confirmed, SubagentOpenResult::PopupOpened));
                }
                Err(blocked("subagent_picker_unavailable"))
            }
            DictationAdapter::Codex => {
                if let Some(trigger) = select_latest_subagent_trigger(
                    &window,
                    is_codex_subagent_picker_trigger,
                    Some(is_codex_subagent_picker_trigger),
                ) {
                    let (id, confirmed) =
                        Self::press_subagent_trigger_in_window(provider, &window, trigger)?;
                    return Ok((id, confirmed, SubagentOpenResult::PopupOpened));
                }
                let full_window = self.capture_full_window_for_key(provider, adapter)?;
                if let Some(trigger) = select_latest_subagent_trigger(
                    &full_window,
                    is_codex_subagent_picker_trigger,
                    Some(is_codex_subagent_picker_trigger),
                ) {
                    let (id, confirmed) =
                        Self::press_subagent_trigger_in_window(provider, &full_window, trigger)?;
                    return Ok((id, confirmed, SubagentOpenResult::PopupOpened));
                }
                Err(blocked("subagent_picker_unavailable"))
            }
        }
    }

    fn open_model_picker_in_window(
        &self,
        provider: &mut P,
        adapter: DictationAdapter,
        window: ManagedWindow,
    ) -> Result<(ManagedWindowIdentity, bool), String> {
        match adapter {
            DictationAdapter::Antigravity => {
                if let Some(trigger) =
                    select_unique_model_trigger(&window, is_antigravity_model_picker_trigger)?
                {
                    let trigger_id = trigger.id;
                    let initial_expanded = trigger.expanded;
                    provider.activate_window(&window.identity)?;
                    let confirmed = if initial_expanded == Some(true) {
                        true
                    } else {
                        provider.press(&window.identity, trigger_id)?;
                        if initial_expanded == Some(false) {
                            confirm_popup_expansion(
                                provider,
                                &window.identity,
                                trigger_id,
                                Duration::from_millis(250),
                            )?
                        } else {
                            false
                        }
                    };
                    return Ok((window.identity, confirmed));
                }
                let fallback_window = capture(provider, adapter)?;
                let control =
                    focus_in_window(provider, &fallback_window.identity, self.readback_timeout)?;
                let cmd = vec!["command".to_string()];
                provider.shortcut(&fallback_window.identity, control, 0x2C, &cmd)?;
                Ok((fallback_window.identity, false))
            }
            DictationAdapter::Codex => {
                provider.activate_window(&window.identity)?;
                if is_codex_model_popover_visible(&window) {
                    return Ok((window.identity, true));
                }
                let observable_popup_id = unique_codex_model_trigger(&window)
                    .filter(|c| c.role == "AXPopUpButton" && c.expanded == Some(false))
                    .map(|c| c.id);
                let ctrl_shift = vec!["control".to_string(), "shift".to_string()];
                if provider
                    .send_window_key(&window.identity, 0x2E, &ctrl_shift)
                    .is_ok()
                {
                    let confirmed = if let Some(trigger_id) = observable_popup_id {
                        poll_optional_popup_expansion(
                            provider,
                            &window.identity,
                            trigger_id,
                            Duration::from_millis(60),
                        ) || read(provider, &window.identity)
                            .is_ok_and(|updated| is_codex_model_popover_visible(&updated))
                    } else {
                        false
                    };
                    return Ok((window.identity, confirmed));
                }
                let trigger = unique_codex_model_trigger(&window)
                    .ok_or_else(|| blocked("model_picker_control_unavailable"))?;
                let trigger_id = trigger.id;
                let was_collapsed_popup =
                    trigger.role == "AXPopUpButton" && trigger.expanded == Some(false);
                provider.press(&window.identity, trigger_id)?;
                let mut confirmed = if was_collapsed_popup {
                    poll_optional_popup_expansion(
                        provider,
                        &window.identity,
                        trigger_id,
                        Duration::from_millis(60),
                    )
                } else {
                    false
                };
                if let Ok(updated) = read(provider, &window.identity) {
                    if is_codex_model_popover_visible(&updated) {
                        confirmed = true;
                    }
                    let select_model_items: Vec<&ManagedControl> = updated
                        .controls
                        .iter()
                        .filter(|c| {
                            c.enabled
                                && c.pressable
                                && c.role == "AXMenuItem"
                                && c.label.trim().eq_ignore_ascii_case("select model")
                        })
                        .collect();
                    if let [item] = select_model_items.as_slice() {
                        let _ = provider.press(&window.identity, item.id);
                    }
                }
                Ok((window.identity, confirmed))
            }
            DictationAdapter::Dsh => {
                provider.activate_window(&window.identity)?;
                if let Some(trigger) =
                    select_unique_model_trigger(&window, is_dsh_model_picker_trigger)?
                {
                    let trigger_id = trigger.id;
                    let initial_expanded = trigger.expanded;
                    let confirmed = if initial_expanded == Some(true) {
                        true
                    } else {
                        provider.press(&window.identity, trigger_id)?;
                        if initial_expanded == Some(false) {
                            confirm_popup_expansion(
                                provider,
                                &window.identity,
                                trigger_id,
                                Duration::from_millis(250),
                            )?
                        } else {
                            false
                        }
                    };
                    return Ok((window.identity, confirmed));
                }
                let cmd = vec!["command".to_string()];
                if let Ok(control) =
                    focus_in_window(provider, &window.identity, self.readback_timeout)
                {
                    provider.shortcut(&window.identity, control, 0x2C, &cmd)?;
                } else {
                    provider.send_window_key(&window.identity, 0x2C, &cmd)?;
                }
                Ok((window.identity, false))
            }
        }
    }

    pub fn open_model_picker(&self, adapter: &str) -> Result<(), String> {
        let adapter = managed_adapter(adapter)?;
        if adapter == DictationAdapter::Codex {
            let _ = self.begin_codex_reasoning_turn();
        }
        let mut provider = self
            .provider
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        let window = self.capture_window_for_key(&mut *provider, adapter)?;
        let (target_identity, ax_open_confirmed) =
            self.open_model_picker_in_window(&mut *provider, adapter, window)?;
        self.record_active_picker_target(
            ActivePickerKind::Model,
            adapter,
            &target_identity,
            ax_open_confirmed,
        );
        Ok(())
    }

    pub fn step_model_picker(&self, adapter: &str, key_code: u16) -> Result<(), String> {
        let adapter = managed_adapter(adapter)?;
        if adapter == DictationAdapter::Codex {
            let _ = self.begin_codex_reasoning_turn();
        }
        let mut provider = self
            .provider
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        let window = self.capture_window_for_key(&mut *provider, adapter)?;
        let prior_ax_confirmed =
            self.active_picker_state(ActivePickerKind::Model, adapter, &window.identity);
        let observed = self.effective_popup_state(&window, adapter)?;
        let is_open = match observed {
            ObservedModelPickerPopupState::Open => true,
            ObservedModelPickerPopupState::Closed => false,
            ObservedModelPickerPopupState::Unknown => prior_ax_confirmed.is_some(),
        };
        let (target_identity, ax_open_confirmed) = if is_open {
            (
                window.identity,
                observed == ObservedModelPickerPopupState::Open
                    || prior_ax_confirmed.unwrap_or(false),
            )
        } else {
            self.open_model_picker_in_window(&mut *provider, adapter, window)?
        };
        provider.send_window_key(&target_identity, key_code, &[])?;
        self.record_active_picker_target(
            ActivePickerKind::Model,
            adapter,
            &target_identity,
            ax_open_confirmed,
        );
        Ok(())
    }

    pub fn confirm_model_picker(&self, adapter: &str) -> Result<ModelPickerConfirmResult, String> {
        let adapter = managed_adapter(adapter)?;
        if adapter == DictationAdapter::Codex {
            let _ = self.begin_codex_reasoning_turn();
        }
        let mut provider = self
            .provider
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        let window = self.capture_window_for_key(&mut *provider, adapter)?;
        match self.effective_popup_state(&window, adapter)? {
            ObservedModelPickerPopupState::Closed => {
                self.clear_active_picker_target();
                Ok(ModelPickerConfirmResult::PopupAlreadyClosed)
            }
            ObservedModelPickerPopupState::Open | ObservedModelPickerPopupState::Unknown => {
                self.clear_active_picker_target();
                provider.send_window_key(&window.identity, 0x24, &[])?;
                Ok(ModelPickerConfirmResult::Confirmed)
            }
        }
    }

    pub fn open_subagent_picker(&self, adapter: &str) -> Result<SubagentOpenResult, String> {
        let adapter = managed_adapter(adapter)?;
        if adapter == DictationAdapter::Codex {
            let _ = self.begin_codex_reasoning_turn();
        }
        let mut provider = self
            .provider
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        let window = self.capture_window_for_key(&mut *provider, adapter)?;
        let (target_identity, ax_open_confirmed, mode) =
            self.open_subagent_picker_in_window(&mut *provider, adapter, window)?;
        match mode {
            SubagentOpenResult::PopupOpened => {
                self.record_active_picker_target(
                    ActivePickerKind::Subagent,
                    adapter,
                    &target_identity,
                    ax_open_confirmed,
                );
            }
            SubagentOpenResult::OpenedDirectly => {
                self.clear_active_picker_target();
            }
        }
        Ok(mode)
    }

    pub fn step_subagent_picker(&self, adapter: &str, key_code: u16) -> Result<(), String> {
        let adapter = managed_adapter(adapter)?;
        if adapter == DictationAdapter::Codex {
            let _ = self.begin_codex_reasoning_turn();
        }
        let mut provider = self
            .provider
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        let window = self.capture_window_for_key(&mut *provider, adapter)?;
        let mut resolved_identity = window.identity.clone();
        let mut prior_ax_confirmed =
            self.active_picker_state(ActivePickerKind::Subagent, adapter, &window.identity);
        let mut observed = observe_subagent_picker_popup_state(&window, adapter);
        if observed == ObservedModelPickerPopupState::Unknown && window.identity.scope_id != 0 {
            if let Ok(full_window) = self.capture_full_window_for_key(&mut *provider, adapter) {
                let full_observed = observe_subagent_picker_popup_state(&full_window, adapter);
                let full_prior = self.active_picker_state(
                    ActivePickerKind::Subagent,
                    adapter,
                    &full_window.identity,
                );
                if full_observed != ObservedModelPickerPopupState::Unknown || full_prior.is_some() {
                    resolved_identity = full_window.identity;
                    observed = full_observed;
                    prior_ax_confirmed = full_prior;
                }
            }
        }
        let is_open = match observed {
            ObservedModelPickerPopupState::Open => true,
            ObservedModelPickerPopupState::Closed => false,
            ObservedModelPickerPopupState::Unknown => prior_ax_confirmed.is_some(),
        };
        let (target_identity, ax_open_confirmed) = if is_open {
            (
                resolved_identity,
                observed == ObservedModelPickerPopupState::Open
                    || prior_ax_confirmed.unwrap_or(false),
            )
        } else {
            let (id, confirmed, mode) =
                self.open_subagent_picker_in_window(&mut *provider, adapter, window)?;
            if mode == SubagentOpenResult::OpenedDirectly {
                self.clear_active_picker_target();
                return Ok(());
            }
            (id, confirmed)
        };
        provider.send_window_key(&target_identity, key_code, &[])?;
        self.record_active_picker_target(
            ActivePickerKind::Subagent,
            adapter,
            &target_identity,
            ax_open_confirmed,
        );
        Ok(())
    }

    pub fn confirm_subagent_picker(
        &self,
        adapter: &str,
    ) -> Result<ModelPickerConfirmResult, String> {
        let adapter = managed_adapter(adapter)?;
        if adapter == DictationAdapter::Codex {
            let _ = self.begin_codex_reasoning_turn();
        }
        let mut provider = self
            .provider
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        let window = self.capture_window_for_key(&mut *provider, adapter)?;
        let mut target_identity = window.identity.clone();
        let mut prior_ax_confirmed =
            self.active_picker_state(ActivePickerKind::Subagent, adapter, &window.identity);
        let mut observed = observe_subagent_picker_popup_state(&window, adapter);
        if observed == ObservedModelPickerPopupState::Unknown && window.identity.scope_id != 0 {
            if let Ok(full_window) = self.capture_full_window_for_key(&mut *provider, adapter) {
                let full_observed = observe_subagent_picker_popup_state(&full_window, adapter);
                let full_prior = self.active_picker_state(
                    ActivePickerKind::Subagent,
                    adapter,
                    &full_window.identity,
                );
                if full_observed != ObservedModelPickerPopupState::Unknown || full_prior.is_some() {
                    target_identity = full_window.identity;
                    observed = full_observed;
                    prior_ax_confirmed = full_prior;
                }
            }
        }
        match observed {
            ObservedModelPickerPopupState::Closed => {
                self.clear_active_picker_target();
                Ok(ModelPickerConfirmResult::PopupAlreadyClosed)
            }
            ObservedModelPickerPopupState::Unknown if prior_ax_confirmed.is_none() => {
                self.clear_active_picker_target();
                Ok(ModelPickerConfirmResult::PopupAlreadyClosed)
            }
            ObservedModelPickerPopupState::Open | ObservedModelPickerPopupState::Unknown => {
                self.clear_active_picker_target();
                provider.send_window_key(&target_identity, 0x24, &[])?;
                Ok(ModelPickerConfirmResult::Confirmed)
            }
        }
    }

    pub fn send_model_picker_key(&self, adapter: &str, key_code: u16) -> Result<(), String> {
        let adapter = managed_adapter(adapter)?;
        if adapter == DictationAdapter::Codex {
            let _ = self.begin_codex_reasoning_turn();
        }
        let mut provider = self
            .provider
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        let window = self.capture_window_for_key(&mut *provider, adapter)?;
        if key_code == 0x35 {
            self.clear_active_picker_target();
        } else if matches!(key_code, 0x7B | 0x7C) {
            if let Some(ax_open_confirmed) =
                self.active_picker_state(ActivePickerKind::Model, adapter, &window.identity)
            {
                self.record_active_picker_target(
                    ActivePickerKind::Model,
                    adapter,
                    &window.identity,
                    ax_open_confirmed,
                );
            }
        }
        provider.send_window_key(&window.identity, key_code, &[])
    }

    pub fn codex_reasoning(&self, action: CodexReasoningAction) -> Result<(), String> {
        let prior_owned_by_us = self.begin_codex_reasoning_turn();
        let adapter = DictationAdapter::Codex;
        let mut provider = self
            .provider
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        let mut window = capture(&mut *provider, adapter)?;
        validate_target(&window.identity, adapter_bundle(adapter))?;

        let is_popover_visible = |w: &ManagedWindow| {
            w.controls
                .iter()
                .any(|c| c.role == "AXMenuItem" && is_codex_popover_menu_item(&c.label))
        };

        let popover_owned = if !is_popover_visible(&window) {
            let mut explicit = Vec::new();
            let mut fallback = Vec::new();
            for c in &window.controls {
                if c.role == "AXPopUpButton" && c.enabled && c.pressable {
                    if is_codex_explicit_effort_trigger(&c.label) {
                        explicit.push(c);
                    } else if is_codex_fallback_reasoning_trigger(&c.label) {
                        fallback.push(c);
                    }
                }
            }
            let candidates = if !explicit.is_empty() {
                explicit
            } else {
                fallback
            };
            let trigger = match candidates.as_slice() {
                [t] => *t,
                _ => return Err(blocked("reasoning_control_unavailable")),
            };
            provider.press(&window.identity, trigger.id)?;

            let deadline = Instant::now() + Duration::from_millis(400);
            let mut popover_appeared = false;
            while Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(25));
                window = read(&mut *provider, &window.identity)?;
                if is_popover_visible(&window) {
                    popover_appeared = true;
                    break;
                }
            }
            if !popover_appeared {
                self.set_codex_popover_owned(false);
                return Err(blocked("reasoning_popover_unavailable"));
            }
            self.set_codex_popover_owned(true);
            true
        } else {
            prior_owned_by_us
        };

        match action {
            CodexReasoningAction::DecreaseEffort | CodexReasoningAction::IncreaseEffort => {
                let key_code = match action {
                    CodexReasoningAction::DecreaseEffort => 0x7B,
                    CodexReasoningAction::IncreaseEffort => 0x7C,
                    _ => unreachable!(),
                };
                if let Err(e) = provider.send_window_key(&window.identity, key_code, &[]) {
                    if popover_owned {
                        let _ = provider.send_window_key(&window.identity, 0x35, &[]);
                        self.set_codex_popover_owned(false);
                    }
                    return Err(e);
                }
                if popover_owned {
                    self.schedule_auto_dismiss(&window.identity);
                }
                Ok(())
            }
            CodexReasoningAction::ToggleFastMode => {
                let fast_items: Vec<&ManagedControl> = window
                    .controls
                    .iter()
                    .filter(|c| {
                        c.role == "AXMenuItem"
                            && (c.label.trim().eq_ignore_ascii_case("enable fast mode")
                                || c.label.trim().eq_ignore_ascii_case("enable standard mode"))
                    })
                    .collect();
                let fast_item = match fast_items.as_slice() {
                    [item] => *item,
                    _ => {
                        if popover_owned {
                            let _ = provider.send_window_key(&window.identity, 0x35, &[]);
                            self.set_codex_popover_owned(false);
                        }
                        return Err(blocked("fast_mode_unavailable"));
                    }
                };
                let initial_label = fast_item.label.trim().to_lowercase();
                let target_label = if initial_label == "enable fast mode" {
                    "enable standard mode"
                } else {
                    "enable fast mode"
                };

                if let Err(e) = provider.press(&window.identity, fast_item.id) {
                    if popover_owned {
                        let _ = provider.send_window_key(&window.identity, 0x35, &[]);
                        self.set_codex_popover_owned(false);
                    }
                    return Err(e);
                }

                let deadline = Instant::now() + Duration::from_millis(350);
                let mut flipped = false;
                while Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(25));
                    let Ok(cur_window) = read(&mut *provider, &window.identity) else {
                        break;
                    };
                    if !is_popover_visible(&cur_window) {
                        self.set_codex_popover_owned(false);
                        break;
                    }
                    if cur_window.controls.iter().any(|c| {
                        c.role == "AXMenuItem" && c.label.trim().eq_ignore_ascii_case(target_label)
                    }) {
                        flipped = true;
                        break;
                    }
                }
                if !flipped {
                    if popover_owned {
                        let _ = provider.send_window_key(&window.identity, 0x35, &[]);
                        self.set_codex_popover_owned(false);
                    }
                    return Err("unconfirmed:fast_mode_toggle_readback".into());
                }
                if popover_owned {
                    self.schedule_auto_dismiss(&window.identity);
                }
                Ok(())
            }
        }
    }
}

impl<P: ManagedAxProvider + Default + 'static> Default for ManagedAxControls<P> {
    fn default() -> Self {
        Self::new(P::default())
    }
}

pub fn managed_adapter(adapter: &str) -> Result<DictationAdapter, String> {
    DictationAdapter::parse(adapter).ok_or_else(|| blocked("unsupported_adapter"))
}

pub fn adapter_bundle(adapter: DictationAdapter) -> &'static str {
    match adapter {
        DictationAdapter::Antigravity => "com.google.antigravity",
        DictationAdapter::Dsh => "com.deepseek.dsh",
        DictationAdapter::Codex => "com.openai.codex",
    }
}

pub fn blocked(reason: &str) -> String {
    format!("blocked:{reason}")
}

fn validate_target(target: &ManagedWindowIdentity, bundle: &str) -> Result<(), String> {
    if target.pid <= 0 || target.window_id == 0 || target.bundle_id != bundle {
        return Err(blocked("target_identity_mismatch"));
    }
    Ok(())
}

fn capture<P: ManagedAxProvider>(
    provider: &mut P,
    adapter: DictationAdapter,
) -> Result<ManagedWindow, String> {
    let bundle = adapter_bundle(adapter);
    let window = provider.capture(bundle)?;
    validate_target(&window.identity, bundle)?;
    Ok(window)
}

fn read<P: ManagedAxProvider + ?Sized>(
    provider: &mut P,
    target: &ManagedWindowIdentity,
) -> Result<ManagedWindow, String> {
    let window = provider.read(target)?;
    if window.identity != *target {
        return Err(blocked("target_identity_changed"));
    }
    Ok(window)
}

pub(crate) fn select_input_scope(
    input_paths: &[Vec<u64>],
    focus_path: &[u64],
) -> Result<Option<u64>, String> {
    if input_paths.len() <= 1 {
        return Ok(None);
    }
    let common =
        |left: &[u64], right: &[u64]| left.iter().zip(right).take_while(|(a, b)| a == b).count();
    let scores: Vec<_> = input_paths
        .iter()
        .map(|path| common(path, focus_path))
        .collect();
    let maximum = scores.iter().copied().max().unwrap_or(0);
    let selected: Vec<_> = scores
        .iter()
        .enumerate()
        .filter(|(_, score)| **score == maximum)
        .collect();
    let [(selected, _)] = selected.as_slice() else {
        return Err(blocked("ambiguous_or_missing_focused_pane"));
    };
    let path = &input_paths[*selected];
    let boundary = input_paths
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != *selected)
        .map(|(_, other)| common(path, other))
        .max()
        .unwrap_or(0);
    if maximum <= boundary || boundary >= path.len().saturating_sub(1) {
        return Err(blocked("ambiguous_or_missing_focused_pane"));
    }
    Ok(Some(path[boundary]))
}

fn live_voice_control(window: &ManagedWindow) -> Result<(bool, u64), String> {
    let controls: Vec<_> = window
        .controls
        .iter()
        .filter_map(|control| {
            let active = if crate::executor::is_antigravity_live_voice_start_label(&control.label) {
                false
            } else if crate::executor::is_antigravity_live_voice_end_label(&control.label) {
                true
            } else {
                return None;
            };
            Some((active, control))
        })
        .collect();
    let [(active, control)] = controls.as_slice() else {
        return Err(blocked("ambiguous_or_missing_live_voice_control"));
    };
    if !control.enabled || !control.pressable {
        return Err(blocked("live_voice_control_unavailable"));
    }
    Ok((*active, control.id))
}

fn dictation_control(
    window: &ManagedWindow,
    adapter: DictationAdapter,
) -> Result<(ObservedDictationState, u64), String> {
    if window
        .controls
        .iter()
        .any(|control| adapter.matches_finishing_label(&control.label))
    {
        return Err(blocked("dictation_finishing"));
    }
    let controls: Vec<_> = window
        .controls
        .iter()
        .filter_map(|control| {
            let state = if adapter.matches_start_label(&control.label) {
                ObservedDictationState::Idle
            } else if adapter.matches_stop_label(&control.label) {
                ObservedDictationState::Recording
            } else {
                return None;
            };
            Some((state, control))
        })
        .collect();
    let [(state, control)] = controls.as_slice() else {
        return Err(blocked("ambiguous_or_missing_dictation_control"));
    };
    if !control.enabled || !control.pressable {
        return Err(blocked("dictation_control_unavailable"));
    }
    Ok((*state, control.id))
}

pub(crate) fn validate_codex_submit(window: &ManagedWindow) -> Result<(), String> {
    if window.identity.bundle_id != adapter_bundle(DictationAdapter::Codex) {
        return Ok(());
    }
    let adapter = DictationAdapter::Codex;
    if !window.controls.iter().any(|control| {
        adapter.matches_start_label(&control.label)
            || adapter.matches_stop_label(&control.label)
            || adapter.matches_finishing_label(&control.label)
    }) {
        return Ok(());
    }
    match dictation_control(window, adapter)? {
        (ObservedDictationState::Idle, _) => Ok(()),
        _ => Err(blocked("dictation_still_recording")),
    }
}

fn press_and_confirm<P: ManagedAxProvider>(
    provider: &mut P,
    target: &ManagedWindowIdentity,
    adapter: DictationAdapter,
    control: u64,
    desired: ObservedDictationState,
    timeout: Duration,
) -> Result<(), String> {
    let before = read(provider, target)?;
    let (state, current_control) = dictation_control(&before, adapter)?;
    if current_control != control || state == desired {
        return Err(blocked("dictation_changed_before_press"));
    }
    provider.press(target, control)?;
    confirm_dictation_state(provider, target, adapter, desired, timeout)
}

fn confirm_dictation_state<P: ManagedAxProvider>(
    provider: &mut P,
    target: &ManagedWindowIdentity,
    adapter: DictationAdapter,
    desired: ObservedDictationState,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        let current = read(provider, target).map_err(|error| format!("unconfirmed:{error}"))?;
        if let Ok((state, _)) = dictation_control(&current, adapter) {
            if state == desired {
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            return Err("unconfirmed:dictation_readback".into());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn stop_in_window<P: ManagedAxProvider>(
    provider: &mut P,
    window: &ManagedWindow,
    adapter: DictationAdapter,
    timeout: Duration,
    allow_absent: bool,
) -> Result<bool, String> {
    let current = read(provider, &window.identity)?;
    let has_dictation = current.controls.iter().any(|control| {
        adapter.matches_start_label(&control.label)
            || adapter.matches_stop_label(&control.label)
            || adapter.matches_finishing_label(&control.label)
    });
    if adapter == DictationAdapter::Codex && !has_dictation && !allow_absent {
        return Err("unconfirmed:dictation_readback".into());
    }
    if current
        .controls
        .iter()
        .any(|control| adapter.matches_finishing_label(&control.label))
    {
        confirm_dictation_state(
            provider,
            &window.identity,
            adapter,
            ObservedDictationState::Idle,
            timeout,
        )?;
        return Ok(true);
    }
    if allow_absent && !has_dictation {
        return Ok(false);
    }
    let (state, control) = dictation_control(&current, adapter)?;
    if state == ObservedDictationState::Idle {
        return Ok(false);
    }
    press_and_confirm(
        provider,
        &window.identity,
        adapter,
        control,
        ObservedDictationState::Idle,
        timeout,
    )?;
    Ok(true)
}

pub(crate) fn is_side_question_input(control: &ManagedControl) -> bool {
    control.label == "Side Question Input"
}

fn is_antigravity_browser_open(window: &ManagedWindow) -> bool {
    window.controls.iter().any(|c| {
        (matches!(c.role.as_str(), "AXComboBox" | "AXTextField") && c.label.trim() == "Address bar")
            || (c.role == "AXButton" && c.label.trim() == "Open in external browser")
    })
}

fn focus_in_window<P: ManagedAxProvider>(
    provider: &mut P,
    target: &ManagedWindowIdentity,
    timeout: Duration,
) -> Result<u64, String> {
    let window = read(provider, target)?;
    let candidates: Vec<_> = window
        .controls
        .iter()
        .filter(|control| control.enabled && control.editable && control.role == "AXTextArea")
        .collect();
    let focused: Vec<_> = candidates
        .iter()
        .copied()
        .filter(|control| control.focused)
        .collect();
    let control = match (focused.as_slice(), candidates.as_slice()) {
        ([control], _) => *control,
        ([], [control]) => *control,
        ([], _) if candidates.len() > 1 => {
            let primary: Vec<_> = candidates
                .iter()
                .copied()
                .filter(|control| !is_side_question_input(control))
                .collect();
            if let [control] = primary.as_slice() {
                *control
            } else {
                return Err(blocked("ambiguous_or_missing_input"));
            }
        }
        _ => return Err(blocked("ambiguous_or_missing_input")),
    };
    let id = control.id;
    provider.focus(target, id)?;
    let deadline = Instant::now() + timeout;
    loop {
        let control = provider
            .read_control(target, id)
            .map_err(|error| format!("unconfirmed:{error}"))?;
        if control.id == id && control.editable && control.focused {
            return Ok(id);
        }
        if Instant::now() >= deadline {
            return Err("unconfirmed:input_focus".into());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

pub(crate) fn insert_at_utf16_selection(
    original: &str,
    location: isize,
    length: isize,
    text: &str,
) -> Result<String, String> {
    let start = usize::try_from(location).map_err(|_| blocked("input_selection_invalid"))?;
    let length = usize::try_from(length).map_err(|_| blocked("input_selection_invalid"))?;
    let end = start
        .checked_add(length)
        .ok_or_else(|| blocked("input_selection_invalid"))?;
    let utf16: Vec<_> = original.encode_utf16().collect();
    if end > utf16.len() {
        return Err(blocked("input_selection_invalid"));
    }
    let before =
        String::from_utf16(&utf16[..start]).map_err(|_| blocked("input_selection_invalid"))?;
    let after =
        String::from_utf16(&utf16[end..]).map_err(|_| blocked("input_selection_invalid"))?;
    Ok(format!("{before}{text}{after}"))
}

pub(crate) fn observe_container_dictation(
    window: &ManagedWindow,
    adapter: DictationAdapter,
) -> ObservedContainerDictation {
    if window
        .controls
        .iter()
        .any(|control| adapter.matches_finishing_label(&control.label))
    {
        return ObservedContainerDictation::Finishing;
    }
    match dictation_control(window, adapter) {
        Ok((ObservedDictationState::Idle, _)) => ObservedContainerDictation::Idle,
        Ok((ObservedDictationState::Recording, _)) => ObservedContainerDictation::Recording,
        Err(_) => ObservedContainerDictation::Indeterminate,
    }
}

#[derive(Debug, Default)]
pub(crate) struct DictationLeaseRegistry {
    claims: BTreeMap<String, ManagedWindowIdentity>,
}

impl DictationLeaseRegistry {
    pub(crate) fn reconcile_for_captured_window<P: ManagedAxProvider>(
        &mut self,
        provider: &mut P,
        captured: &ManagedWindow,
        adapter: DictationAdapter,
    ) -> Result<(), String> {
        let Some(owned) = self.claims.get(&captured.identity.bundle_id).cloned() else {
            return Ok(());
        };
        if owned == captured.identity {
            return Ok(());
        }
        if provider.process_has_exited(owned.pid) {
            self.release_claim(provider, &owned.bundle_id);
            return Ok(());
        }
        if owned.same_container(&captured.identity)
            && observe_container_dictation(captured, adapter) == ObservedContainerDictation::Idle
        {
            self.release_claim(provider, &owned.bundle_id);
            return Ok(());
        }
        Err(blocked("uncleared_dictation_target_changed"))
    }

    pub(crate) fn retain_claim<P: ManagedAxProvider>(
        &mut self,
        provider: &mut P,
        identity: &ManagedWindowIdentity,
    ) -> Result<(), String> {
        provider.retain_target(identity)?;
        if let Some(prev) = self
            .claims
            .insert(identity.bundle_id.clone(), identity.clone())
        {
            if prev != *identity {
                provider.release_target(&prev);
            }
        }
        Ok(())
    }

    pub(crate) fn release_claim<P: ManagedAxProvider>(
        &mut self,
        provider: &mut P,
        bundle_id: &str,
    ) {
        if let Some(prev) = self.claims.remove(bundle_id) {
            provider.release_target(&prev);
        }
    }

    pub(crate) fn has_claim(&self, bundle_id: &str) -> bool {
        self.claims.contains_key(bundle_id)
    }

    pub(crate) fn stop_all_owned<P: ManagedAxProvider>(
        &mut self,
        provider: &mut P,
        readback_timeout: Duration,
    ) -> Result<(), String> {
        let mut failures = Vec::new();
        for owned in self.claims.values().cloned().collect::<Vec<_>>() {
            if provider.process_has_exited(owned.pid) {
                self.release_claim(provider, &owned.bundle_id);
                continue;
            }
            let Some(adapter) = [
                DictationAdapter::Antigravity,
                DictationAdapter::Dsh,
                DictationAdapter::Codex,
            ]
            .into_iter()
            .find(|adapter| adapter_bundle(*adapter) == owned.bundle_id) else {
                failures.push(blocked("unsupported_dictation_target"));
                continue;
            };
            let result = provider.read_container(&owned).and_then(|live| {
                if !live.identity.same_container(&owned) {
                    return Err("unconfirmed:dictation_target_changed".into());
                }
                if live.identity.title != owned.title {
                    if observe_container_dictation(&live, adapter)
                        == ObservedContainerDictation::Idle
                    {
                        Ok(false)
                    } else {
                        Err("unconfirmed:dictation_target_changed".into())
                    }
                } else {
                    stop_in_window(provider, &live, adapter, readback_timeout, false)
                }
            });
            match result {
                Ok(_) => self.release_claim(provider, &owned.bundle_id),
                Err(error) => failures.push(error),
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{insert_at_utf16_selection, select_input_scope};

    #[test]
    fn split_panes_resolve_only_the_focused_branch() {
        let inputs = vec![vec![1, 2, 3, 4], vec![1, 2, 5, 6]];
        assert_eq!(select_input_scope(&inputs, &[1, 2, 5, 6]).unwrap(), Some(5));
        assert_eq!(select_input_scope(&inputs, &[1, 2, 3, 9]).unwrap(), Some(3));
        for focus in [vec![], vec![1], vec![1, 2], vec![10, 11]] {
            assert!(select_input_scope(&inputs, &focus).is_err());
        }
        assert_eq!(select_input_scope(&[vec![1, 2, 3]], &[]).unwrap(), None);
        assert!(select_input_scope(&[vec![1, 2], vec![1, 3]], &[1, 2]).is_err());
    }

    #[test]
    fn prompt_insertion_preserves_utf16_selection_and_existing_text() {
        assert_eq!(
            insert_at_utf16_selection("a😀bc", 1, 2, "new").unwrap(),
            "anewbc"
        );
        assert_eq!(
            insert_at_utf16_selection("a😀bc", 3, 0, "new").unwrap(),
            "a😀newbc"
        );
        for (start, length) in [(-1, 0), (1, -1), (2, 0), (0, 99)] {
            assert!(insert_at_utf16_selection("a😀bc", start, length, "new").is_err());
        }
    }
}
