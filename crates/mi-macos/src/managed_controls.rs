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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedControl {
    pub id: u64,
    pub role: String,
    pub label: String,
    pub enabled: bool,
    pub pressable: bool,
    pub editable: bool,
    pub focused: bool,
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

    fn process_has_exited(&self, _pid: i32) -> bool {
        false
    }
    fn capture(&mut self, bundle_id: &str) -> Result<ManagedWindow, String>;
    fn capture_for_scroll(&mut self, bundle_id: &str) -> Result<ManagedWindow, String> {
        self.capture(bundle_id)
    }
    fn read(&mut self, target: &ManagedWindowIdentity) -> Result<ManagedWindow, String>;
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
    uncleared_targets: Mutex<BTreeMap<String, ManagedWindowIdentity>>,
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
            uncleared_targets: Mutex::new(BTreeMap::new()),
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
        let mut targets = self
            .uncleared_targets
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        validate_uncleared_target(&mut *provider, &mut targets, &window.identity)?;
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
            targets.remove(&window.identity.bundle_id);
            provider.release_target(&window.identity);
        } else if result.is_ok()
            || result
                .as_ref()
                .is_err_and(|error| error.starts_with("unconfirmed:"))
        {
            provider.retain_target(&window.identity)?;
            targets.insert(window.identity.bundle_id.clone(), window.identity.clone());
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
        let mut targets = self
            .uncleared_targets
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        validate_uncleared_target(&mut *provider, &mut targets, &window.identity)?;
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
            provider.retain_target(&window.identity)?;
            targets.insert(window.identity.bundle_id.clone(), window.identity.clone());
        }
        result
    }

    pub fn stop_owned_dictations(&self) -> Result<(), String> {
        let mut provider = self
            .provider
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        let mut targets = self
            .uncleared_targets
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        let mut failures = Vec::new();
        for target in targets.values().cloned().collect::<Vec<_>>() {
            if provider.process_has_exited(target.pid) {
                targets.remove(&target.bundle_id);
                provider.release_target(&target);
                continue;
            }
            let Some(adapter) = [
                DictationAdapter::Antigravity,
                DictationAdapter::Dsh,
                DictationAdapter::Codex,
            ]
            .into_iter()
            .find(|adapter| adapter_bundle(*adapter) == target.bundle_id) else {
                failures.push(blocked("unsupported_dictation_target"));
                continue;
            };
            let result = provider.read(&target).and_then(|window| {
                if window.identity != target {
                    return Err("unconfirmed:dictation_target_changed".into());
                }
                stop_in_window(
                    &mut *provider,
                    &window,
                    adapter,
                    self.readback_timeout,
                    false,
                )
            });
            match result {
                Ok(_) => {
                    targets.remove(&target.bundle_id);
                    provider.release_target(&target);
                }
                Err(error) => failures.push(error),
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    }

    fn stop_in_window(
        &self,
        provider: &mut P,
        window: &ManagedWindow,
        adapter: DictationAdapter,
    ) -> Result<bool, String> {
        let mut targets = self
            .uncleared_targets
            .lock()
            .map_err(|_| blocked("ax_lock_poisoned"))?;
        validate_uncleared_target(provider, &mut targets, &window.identity)?;
        let allow_absent = !targets.contains_key(&window.identity.bundle_id);
        let result = stop_in_window(
            provider,
            window,
            adapter,
            self.readback_timeout,
            allow_absent,
        );
        if result.is_ok() {
            targets.remove(&window.identity.bundle_id);
            provider.release_target(&window.identity);
        } else if result
            .as_ref()
            .is_err_and(|error| error.starts_with("unconfirmed:"))
        {
            provider.retain_target(&window.identity)?;
            targets.insert(window.identity.bundle_id.clone(), window.identity.clone());
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

fn validate_uncleared_target<P: ManagedAxProvider>(
    provider: &mut P,
    targets: &mut BTreeMap<String, ManagedWindowIdentity>,
    target: &ManagedWindowIdentity,
) -> Result<(), String> {
    if let Some(owned) = targets
        .get(&target.bundle_id)
        .filter(|owned| *owned != target)
    {
        if !provider.process_has_exited(owned.pid) {
            return Err(blocked("uncleared_dictation_target_changed"));
        }
        provider.release_target(owned);
        targets.remove(&target.bundle_id);
    }
    Ok(())
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
