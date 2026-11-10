use crate::ax_dictation::DictationAdapter;
use crate::executor::DialScrollRequest;
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedWindowIdentity {
    pub bundle_id: String,
    pub pid: i32,
    pub window_id: u64,
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
    fn capture(&mut self, bundle_id: &str) -> Result<ManagedWindow, String>;
    fn capture_for_scroll(&mut self, bundle_id: &str) -> Result<ManagedWindow, String> {
        self.capture(bundle_id)
    }
    fn read(&mut self, target: &ManagedWindowIdentity) -> Result<ManagedWindow, String>;
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
}

#[derive(Debug)]
pub struct ManagedAxControls<P> {
    provider: Mutex<P>,
    readback_timeout: Duration,
    uncleared_targets: Mutex<BTreeMap<String, ManagedWindowIdentity>>,
}

impl<P: ManagedAxProvider> ManagedAxControls<P> {
    pub fn new(provider: P) -> Self {
        Self::with_readback_timeout(provider, Duration::from_millis(900))
    }

    pub fn with_readback_timeout(provider: P, readback_timeout: Duration) -> Self {
        Self {
            provider: Mutex::new(provider),
            readback_timeout,
            uncleared_targets: Mutex::new(BTreeMap::new()),
        }
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
        validate_uncleared_target(&targets, &window.identity)?;
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
        } else if result.is_ok()
            || result
                .as_ref()
                .is_err_and(|error| error.starts_with("unconfirmed:"))
        {
            targets.insert(window.identity.bundle_id.clone(), window.identity.clone());
        }
        result?;
        Ok(desired)
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
        validate_uncleared_target(&targets, &window.identity)?;
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
        } else if result
            .as_ref()
            .is_err_and(|error| error.starts_with("unconfirmed:"))
        {
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
            "com.xd.cindy" | "com.google.antigravity" | "com.deepseek.dsh" | "com.openai.codex"
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
}

impl<P: ManagedAxProvider + Default> Default for ManagedAxControls<P> {
    fn default() -> Self {
        Self::new(P::default())
    }
}

pub fn managed_adapter(adapter: &str) -> Result<DictationAdapter, String> {
    DictationAdapter::parse(adapter)
        .filter(|adapter| *adapter != DictationAdapter::Codex)
        .ok_or_else(|| blocked("unsupported_adapter"))
}

pub fn adapter_bundle(adapter: DictationAdapter) -> &'static str {
    match adapter {
        DictationAdapter::Cindy => "com.xd.cindy",
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

fn read<P: ManagedAxProvider>(
    provider: &mut P,
    target: &ManagedWindowIdentity,
) -> Result<ManagedWindow, String> {
    let window = provider.read(target)?;
    if window.identity != *target {
        return Err(blocked("target_identity_changed"));
    }
    Ok(window)
}

fn dictation_control(
    window: &ManagedWindow,
    adapter: DictationAdapter,
) -> Result<(ObservedDictationState, u64), String> {
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
    if allow_absent
        && !current.controls.iter().any(|control| {
            adapter.matches_start_label(&control.label)
                || adapter.matches_stop_label(&control.label)
        })
    {
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
    let [control] = candidates.as_slice() else {
        return Err(blocked("ambiguous_or_missing_input"));
    };
    let id = control.id;
    provider.focus(target, id)?;
    let deadline = Instant::now() + timeout;
    loop {
        let window = read(provider, target).map_err(|error| format!("unconfirmed:{error}"))?;
        if window
            .controls
            .iter()
            .any(|control| control.id == id && control.editable && control.focused)
        {
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

fn validate_uncleared_target(
    targets: &BTreeMap<String, ManagedWindowIdentity>,
    target: &ManagedWindowIdentity,
) -> Result<(), String> {
    if targets
        .get(&target.bundle_id)
        .is_some_and(|owned| owned != target)
    {
        return Err(blocked("uncleared_dictation_target_changed"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::insert_at_utf16_selection;

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
