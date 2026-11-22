use mi_macos::executor::{
    ClipboardSnapshot, DialScrollRequest, HostOsDriver, LiveHostActionExecutor, MouseButtonKind,
    SimulatedHostActionExecutor,
};
use mi_macos::managed_controls::{
    blocked, ManagedAxControls, ManagedAxProvider, ManagedControl, ManagedWindow,
    ManagedWindowIdentity, ModelPickerConfirmResult, ObservedDictationState, SubagentOpenResult,
};
use mi_model::host_router::ResolvedHostAction;
use std::sync::{Arc, Barrier, Mutex};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq)]
enum PressEffect {
    Toggle,
    NoChange,
    Finish,
    Hide,
    Replace,
    LoseTarget,
    LoseScope,
}
struct Fixture {
    window: ManagedWindow,
    effect: PressEffect,
    events: Vec<String>,
    read_pid: Option<i32>,
    capture_error: Option<String>,
    focus_confirmed: bool,
    exited_pids: Vec<i32>,
    enforce_target_cache: bool,
    latest_target: Option<ManagedWindowIdentity>,
    retained_targets: Vec<ManagedWindowIdentity>,
    after_capture: Option<fn(&mut Fixture)>,
    after_focus: Option<fn(&mut Fixture)>,
    after_press: Option<fn(&mut Fixture, &str, u64)>,
    side_panel_delay_reads: Option<usize>,
    finish_reads_remaining: Option<usize>,
    include_fast_mode: bool,
    window_key_error: Option<String>,
}
#[derive(Clone)]
struct Provider(Arc<Mutex<Fixture>>);
impl Provider {
    fn with_target(bundle_id: &str, start_label: &str) -> Self {
        Self(Arc::new(Mutex::new(Fixture {
            window: ManagedWindow {
                identity: ManagedWindowIdentity {
                    bundle_id: bundle_id.into(),
                    pid: 100,
                    window_id: 1,
                    scope_id: 0,
                    title: "Test".into(),
                },
                controls: vec![
                    ManagedControl {
                        id: 2,
                        role: "AXCheckBox".into(),
                        label: start_label.into(),
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
                ],
            },
            effect: PressEffect::Toggle,
            events: Vec::new(),
            read_pid: None,
            capture_error: None,
            focus_confirmed: true,
            exited_pids: Vec::new(),
            enforce_target_cache: false,
            latest_target: None,
            retained_targets: Vec::new(),
            after_capture: None,
            after_focus: None,
            after_press: None,
            side_panel_delay_reads: None,
            finish_reads_remaining: None,
            include_fast_mode: true,
            window_key_error: None,
        })))
    }
    fn dsh() -> Self {
        Self::with_target("com.deepseek.dsh", "开始听写")
    }
    fn codex() -> Self {
        Self::with_target("com.openai.codex", "Dictate")
    }
    fn antigravity() -> Self {
        Self::with_target("com.google.antigravity", "Record voice memo")
    }
    fn controls(&self) -> ManagedAxControls<Self> {
        ManagedAxControls::with_readback_timeout(self.clone(), Duration::ZERO)
    }
    fn controls_with_delay(&self, delay: Duration) -> ManagedAxControls<Self> {
        ManagedAxControls::with_timeouts(self.clone(), Duration::ZERO, delay)
    }
    fn events(&self) -> Vec<String> {
        self.0.lock().unwrap().events.clone()
    }
}
impl ManagedAxProvider for Provider {
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
    fn activate_window(&mut self, target: &ManagedWindowIdentity) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        assert_eq!(*target, state.window.identity);
        state.events.push("activate_window".into());
        Ok(())
    }
    fn process_has_exited(&self, pid: i32) -> bool {
        self.0.lock().unwrap().exited_pids.contains(&pid)
    }
    fn capture(&mut self, bundle: &str) -> Result<ManagedWindow, String> {
        let mut state = self.0.lock().unwrap();
        state.events.push(format!("capture:{bundle}"));
        if let Some(error) = &state.capture_error {
            return Err(error.clone());
        }
        let mut window = state.window.clone();
        if state.enforce_target_cache && bundle == "com.google.antigravity" {
            window.identity.bundle_id = bundle.into();
            window.identity.pid = 200;
            window.identity.window_id = 2;
            window.controls[0].label = "开始听写".into();
        }
        state.latest_target = Some(window.identity.clone());
        if let Some(change) = state.after_capture.take() {
            change(&mut state);
        }
        Ok(window)
    }
    fn read(&mut self, target: &ManagedWindowIdentity) -> Result<ManagedWindow, String> {
        let mut state = self.0.lock().unwrap();
        if state
            .window
            .controls
            .first()
            .is_some_and(|control| control.label == "Finishing dictation")
        {
            if let Some(remaining) = state.finish_reads_remaining.as_mut() {
                if *remaining == 0 {
                    state.window.controls[0].label = "Dictate".into();
                    state.window.controls[0].enabled = true;
                    state.window.controls[0].pressable = true;
                } else {
                    *remaining -= 1;
                }
            }
        }
        if let Some(remaining) = state.side_panel_delay_reads.as_mut() {
            if *remaining > 0 {
                *remaining -= 1;
            } else if state
                .window
                .controls
                .iter()
                .any(|c| c.id == 10 && c.expanded == Some(false))
            {
                if let Some(btn) = state.window.controls.iter_mut().find(|c| c.id == 10) {
                    btn.expanded = Some(true);
                }
                state.window.controls.push(ManagedControl {
                    id: 20,
                    role: "AXButton".into(),
                    label: "Side Question".into(),
                    enabled: true,
                    pressable: true,
                    editable: false,
                    focused: false,
                    expanded: None,
                });
            }
        }
        if state.enforce_target_cache
            && state.latest_target.as_ref() != Some(target)
            && !state.retained_targets.contains(target)
        {
            return Err("blocked:ax_target_missing".into());
        }
        assert_eq!(
            target.pid, state.window.identity.pid,
            "must not reselect another PID"
        );
        let mut window = state.window.clone();
        if let Some(pid) = state.read_pid {
            window.identity.pid = pid;
        }
        Ok(window)
    }
    fn press(&mut self, target: &ManagedWindowIdentity, control: u64) -> Result<(), String> {
        std::thread::sleep(Duration::from_millis(2));
        let mut state = self.0.lock().unwrap();
        assert_eq!(*target, state.window.identity);
        let effect = state.effect;
        let (old_label, control_id, control_role) = {
            let control = state
                .window
                .controls
                .iter_mut()
                .find(|c| c.id == control)
                .unwrap();
            let old_label = control.label.clone();
            let control_id = control.id;
            let control_role = control.role.clone();
            match effect {
                PressEffect::Toggle | PressEffect::Replace
                    if control_role != "AXMenuItem" && control_role != "AXPopUpButton" =>
                {
                    if let Some(next_label) = match control.label.as_str() {
                        "开始听写" => Some("停止听写"),
                        "停止听写" | "停止并识别" | "Stop and transcribe" => {
                            Some("开始听写")
                        }
                        "Dictate" => Some("Stop dictation"),
                        "Stop dictation" => Some("Dictate"),
                        "Record voice memo" => Some("Stop Recording"),
                        "Stop Recording" => Some("Record voice memo"),
                        "录制语音输入" => Some("停止录制"),
                        "停止录制" => Some("录制语音输入"),
                        "Start Voice mode" => Some("End Voice mode"),
                        "End Voice mode" => Some("Start Voice mode"),
                        "开启语音模式" => Some("结束语音模式"),
                        "结束语音模式" => Some("开启语音模式"),
                        _ => None,
                    } {
                        control.label = next_label.into();
                    }
                    if matches!(effect, PressEffect::Replace) {
                        control.id += 10;
                    }
                }
                PressEffect::Finish => {
                    control.label = "Finishing dictation".into();
                    control.enabled = false;
                    control.pressable = false;
                }
                _ => {}
            }
            if control_role == "AXPopUpButton"
                && effect != PressEffect::NoChange
                && control.expanded == Some(false)
            {
                control.expanded = Some(true);
            }
            if control_role == "AXMenuItem" && effect != PressEffect::NoChange {
                if control.label.contains("fast") {
                    control.label = "Enable standard mode".into();
                } else if control.label.contains("standard") {
                    control.label = "Enable fast mode".into();
                }
            }
            (old_label, control_id, control_role)
        };
        match effect {
            PressEffect::Hide => state.window.controls.retain(|item| item.id != control_id),
            PressEffect::LoseTarget => state.read_pid = Some(999),
            PressEffect::LoseScope => state.window.identity.scope_id += 1,
            _ => {}
        }
        if control_role == "AXPopUpButton" {
            state.window.controls.push(ManagedControl {
                id: 1001,
                role: "AXMenuItem".into(),
                label: "Select model".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: None,
            });
            if state.include_fast_mode {
                state.window.controls.push(ManagedControl {
                    id: 1002,
                    role: "AXMenuItem".into(),
                    label: "Enable fast mode".into(),
                    enabled: true,
                    pressable: true,
                    editable: false,
                    focused: false,
                    expanded: None,
                });
            }
        }
        state.events.push(format!("press:{old_label}"));
        if let Some(hook) = state.after_press {
            hook(&mut state, &old_label, control_id);
        }
        Ok(())
    }
    fn focus(&mut self, target: &ManagedWindowIdentity, id: u64) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        assert_eq!(*target, state.window.identity);
        let confirmed = state.focus_confirmed;
        state
            .window
            .controls
            .iter_mut()
            .find(|c| c.id == id)
            .unwrap()
            .focused = confirmed;
        state.events.push(format!("focus:{id}"));
        if let Some(change) = state.after_focus.take() {
            change(&mut state);
        }
        Ok(())
    }
    fn scroll(
        &mut self,
        target: &ManagedWindowIdentity,
        request: &DialScrollRequest,
    ) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        assert_eq!(*target, state.window.identity);
        assert_eq!(request.target_bundle_id, target.bundle_id);
        if request.sidebar
            && !state
                .window
                .controls
                .iter()
                .any(|c| c.role == "AXScrollArea")
        {
            return Err(blocked("target_scrollbar_or_bottom_control_unavailable"));
        }
        state.events.push("scroll".into());
        Ok(())
    }
    fn send_window_key(
        &mut self,
        target: &ManagedWindowIdentity,
        key_code: u16,
        modifiers: &[String],
    ) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        assert_eq!(*target, state.window.identity);
        if let Some(error) = &state.window_key_error {
            return Err(error.clone());
        }
        if key_code == 0x35 || key_code == 0x24 {
            for control in &mut state.window.controls {
                if control.role == "AXPopUpButton" && control.expanded == Some(true) {
                    control.expanded = Some(false);
                }
            }
        }
        if key_code == 0x35 {
            state.window.controls.retain(|c| c.role != "AXMenuItem");
            state.events.push("window_key:escape".into());
        } else if key_code == 0x7B {
            state.events.push("window_key:arrow_left".into());
        } else if key_code == 0x7C {
            state.events.push("window_key:arrow_right".into());
        } else if key_code == 0x7E {
            state.events.push("window_key:arrow_up".into());
        } else if key_code == 0x7D {
            state.events.push("window_key:arrow_down".into());
        } else if key_code == 0x24 {
            state.events.push("window_key:return".into());
        } else if modifiers.is_empty() {
            state.events.push(format!("window_key:{key_code:x}"));
        } else {
            state
                .events
                .push(format!("window_key:{}:{key_code:x}", modifiers.join("+")));
        }
        Ok(())
    }
    fn insert_text(
        &mut self,
        target: &ManagedWindowIdentity,
        _: u64,
        text: &str,
    ) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        assert_eq!(*target, state.window.identity);
        state.events.push(format!("insert:{text}"));
        Ok(())
    }
    fn shortcut(
        &mut self,
        target: &ManagedWindowIdentity,
        _: u64,
        _: u16,
        _: &[String],
    ) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        assert_eq!(*target, state.window.identity);
        state.events.push("targeted_shortcut".into());
        Ok(())
    }
    fn submit(&mut self, target: &ManagedWindowIdentity, _: u64) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        assert_eq!(*target, state.window.identity);
        assert!(!state.window.controls.iter().any(|control| [
            "停止听写",
            "停止并识别",
            "Stop and transcribe",
            "Stop dictation",
            "Stop Recording",
            "Finishing dictation",
            "正在完成听写"
        ]
        .contains(&control.label.as_str())));
        state.events.push("submit".into());
        Ok(())
    }
}

#[test]
fn toggle_observes_mouse_changes_and_replacement_controls() {
    let provider = Provider::dsh();
    let controls = provider.controls();
    assert_eq!(
        controls.toggle_dictation("dsh-desktop").unwrap(),
        ObservedDictationState::Recording
    );
    provider.0.lock().unwrap().window.controls[0].label = "开始听写".into();
    provider.0.lock().unwrap().effect = PressEffect::Replace;
    assert_eq!(
        controls.toggle_dictation("dsh-desktop").unwrap(),
        ObservedDictationState::Recording
    );
    assert_eq!(
        controls.toggle_dictation("dsh-desktop").unwrap(),
        ObservedDictationState::Idle
    );
    assert_eq!(
        provider
            .events()
            .iter()
            .filter(|e| e.starts_with("press:"))
            .count(),
        3
    );
}

#[test]
fn missing_disabled_or_ambiguous_dictation_controls_never_press() {
    for case in 0..5 {
        let provider = Provider::dsh();
        {
            let mut state = provider.0.lock().unwrap();
            match case {
                0 => {
                    state.window.controls.remove(0);
                }
                1 => state.window.controls[0].enabled = false,
                2 => state.window.controls[0].pressable = false,
                3 => {
                    let mut extra = state.window.controls[0].clone();
                    extra.id = 8;
                    state.window.controls.push(extra);
                }
                _ => {
                    let mut extra = state.window.controls[0].clone();
                    extra.id = 8;
                    extra.label = "停止听写".into();
                    state.window.controls.push(extra);
                }
            }
        }
        assert!(provider
            .controls()
            .toggle_dictation("dsh-desktop")
            .unwrap_err()
            .starts_with("blocked:"));
        assert!(!provider.events().iter().any(|e| e.starts_with("press:")));
    }
}

#[test]
fn target_pid_or_bundle_mismatch_never_press() {
    for at_capture in [true, false] {
        let provider = Provider::dsh();
        if at_capture {
            provider.0.lock().unwrap().window.identity.bundle_id = "com.openai.codex".into();
        } else {
            provider.0.lock().unwrap().read_pid = Some(999);
        }
        assert!(provider
            .controls()
            .toggle_dictation("dsh-desktop")
            .unwrap_err()
            .starts_with("blocked:"));
        assert!(!provider.events().iter().any(|e| e.starts_with("press:")));
    }
}

#[test]
fn missing_or_ambiguous_windows_propagate_without_actions() {
    for error in [
        "blocked:target_application_missing",
        "blocked:ambiguous_or_missing_target_window",
        "blocked:accessibility_untrusted",
    ] {
        let provider = Provider::dsh();
        provider.0.lock().unwrap().capture_error = Some(error.into());
        assert_eq!(
            provider
                .controls()
                .toggle_dictation("dsh-desktop")
                .unwrap_err(),
            error
        );
        assert_eq!(provider.events().len(), 1);
    }
}

#[test]
fn unconfirmed_press_is_not_retried_and_never_submits() {
    for effect in [PressEffect::NoChange, PressEffect::LoseTarget] {
        let provider = Provider::dsh();
        {
            let mut state = provider.0.lock().unwrap();
            state.effect = effect;
            state.window.controls[0].label = "停止听写".into();
        }
        assert!(provider
            .controls()
            .submit("dsh-desktop", None)
            .unwrap_err()
            .starts_with("unconfirmed:"));
        let events = provider.events();
        assert_eq!(events.iter().filter(|e| e.starts_with("press:")).count(), 1);
        assert!(!events
            .iter()
            .any(|e| e == "submit" || e.starts_with("focus:")));
    }
}

#[test]
fn submit_stops_then_focuses_and_sends_in_one_captured_window() {
    let provider = Provider::dsh();
    provider.0.lock().unwrap().window.controls[0].label = "停止听写".into();
    provider
        .controls()
        .submit("dsh-desktop", Some("test prompt"))
        .unwrap();
    assert_eq!(
        provider.events(),
        [
            "capture:com.deepseek.dsh",
            "press:停止听写",
            "focus:3",
            "insert:test prompt",
            "submit"
        ]
    );
}

#[test]
fn missing_or_ambiguous_or_unconfirmed_input_cannot_submit_or_send_shortcuts() {
    for case in 0..3 {
        let provider = Provider::dsh();
        {
            let mut state = provider.0.lock().unwrap();
            match case {
                0 => {
                    state.window.controls.remove(1);
                }
                1 => {
                    let mut extra = state.window.controls[1].clone();
                    extra.id = 8;
                    state.window.controls.push(extra);
                }
                _ => state.focus_confirmed = false,
            }
        }
        assert!(provider.controls().submit("dsh-desktop", None).is_err());
        assert!(provider
            .controls()
            .shortcut("dsh-desktop", 0x2d, &["command".into()])
            .is_err());
        assert!(!provider
            .events()
            .iter()
            .any(|e| e == "submit" || e == "targeted_shortcut"));
    }
}

#[test]
fn focused_input_is_selected_when_another_pane_has_an_input() {
    let provider = Provider::dsh();
    {
        let mut state = provider.0.lock().unwrap();
        let mut second = state.window.controls[1].clone();
        second.id = 8;
        second.focused = true;
        state.window.controls.push(second);
    }
    provider.controls().focus_input("dsh-desktop").unwrap();
    assert_eq!(provider.events(), ["capture:com.deepseek.dsh", "focus:8"]);
}

#[test]
fn input_added_after_capture_is_rejected_before_focus() {
    let provider = Provider::dsh();
    provider.0.lock().unwrap().after_capture = Some(|state| {
        let mut extra = state.window.controls[1].clone();
        extra.id = 8;
        state.window.controls.push(extra);
    });
    assert_eq!(
        provider.controls().focus_input("dsh-desktop").unwrap_err(),
        "blocked:ambiguous_or_missing_input"
    );
    assert_eq!(provider.events(), ["capture:com.deepseek.dsh"]);
}

#[test]
fn focus_confirmation_rejects_removed_or_replaced_input() {
    for replace in [false, true] {
        let provider = Provider::dsh();
        provider.0.lock().unwrap().after_focus = Some(if replace {
            |state| state.window.controls[1].id = 8
        } else {
            |state| {
                state.window.controls.remove(1);
            }
        });
        assert_eq!(
            provider.controls().focus_input("dsh-desktop").unwrap_err(),
            "unconfirmed:blocked:ax_control_removed"
        );
        assert_eq!(provider.events(), ["capture:com.deepseek.dsh", "focus:3"]);
    }
}

#[test]
fn focus_confirmation_cannot_follow_changed_window_or_scope() {
    let changes: [fn(&mut Fixture); 4] = [
        |state| state.read_pid = Some(999),
        |state| state.window.identity.window_id += 1,
        |state| state.window.identity.scope_id += 1,
        |state| state.window.identity.title = "replacement".into(),
    ];
    for change in changes {
        let provider = Provider::dsh();
        provider.0.lock().unwrap().after_focus = Some(change);
        assert_eq!(
            provider.controls().focus_input("dsh-desktop").unwrap_err(),
            "unconfirmed:blocked:target_identity_changed"
        );
        assert_eq!(provider.events(), ["capture:com.deepseek.dsh", "focus:3"]);
    }
}

#[test]
fn unconfirmed_focus_never_inserts_text() {
    let provider = Provider::dsh();
    provider.0.lock().unwrap().focus_confirmed = false;
    assert_eq!(
        provider
            .controls()
            .submit("dsh-desktop", Some("must not be inserted"))
            .unwrap_err(),
        "unconfirmed:input_focus"
    );
    assert_eq!(provider.events(), ["capture:com.deepseek.dsh", "focus:3"]);
}

#[test]
fn recording_cannot_migrate_to_another_pane_in_the_same_window() {
    let provider = Provider::dsh();
    provider.0.lock().unwrap().window.identity.scope_id = 10;
    let controls = provider.controls();
    controls.start_dictation("dsh-desktop").unwrap();
    provider.0.lock().unwrap().window.identity.scope_id = 11;
    assert_eq!(
        controls.stop_dictation("dsh-desktop").unwrap_err(),
        "blocked:uncleared_dictation_target_changed"
    );
    assert!(controls
        .stop_owned_dictations()
        .unwrap_err()
        .contains("dictation_target_changed"));
    assert_eq!(
        provider
            .events()
            .iter()
            .filter(|event| event.starts_with("press:"))
            .count(),
        1
    );
}

#[test]
fn changed_window_title_leaves_owned_recording_unconfirmed_without_another_press() {
    let provider = Provider::dsh();
    let controls = provider.controls();
    controls.start_dictation("dsh-desktop").unwrap();
    provider.0.lock().unwrap().window.identity.title = "Different conversation".into();
    assert!(controls
        .stop_owned_dictations()
        .unwrap_err()
        .contains("dictation_target_changed"));
    assert_eq!(
        provider
            .events()
            .iter()
            .filter(|event| event.starts_with("press:"))
            .count(),
        1
    );
}

#[test]
fn live_voice_uses_the_captured_pane_and_never_retries_after_scope_loss() {
    for lost in [false, true] {
        let provider = Provider::dsh();
        {
            let mut state = provider.0.lock().unwrap();
            state.window.identity.bundle_id = "com.google.antigravity".into();
            state.window.identity.scope_id = 10;
            state.window.controls[0].label = "Start Voice mode".into();
            if lost {
                state.effect = PressEffect::LoseScope;
            }
        }
        let result = provider.controls().toggle_live_voice("antigravity");
        if lost {
            assert_eq!(
                result.unwrap_err(),
                "unconfirmed:blocked:target_identity_changed"
            );
        } else {
            result.unwrap();
        }
        assert_eq!(
            provider.events(),
            ["capture:com.google.antigravity", "press:Start Voice mode"]
        );
    }
}

#[test]
fn ambiguous_live_voice_buttons_never_press_either_pane() {
    let provider = Provider::dsh();
    {
        let mut state = provider.0.lock().unwrap();
        state.window.identity.bundle_id = "com.google.antigravity".into();
        state.window.controls[0].label = "Start Voice mode".into();
        let mut other = state.window.controls[0].clone();
        other.id = 8;
        state.window.controls.push(other);
    }
    assert_eq!(
        provider
            .controls()
            .toggle_live_voice("antigravity")
            .unwrap_err(),
        "blocked:ambiguous_or_missing_live_voice_control"
    );
    assert_eq!(provider.events(), ["capture:com.google.antigravity"]);
}

#[test]
fn bottom_scroll_and_shortcut_remain_on_captured_target() {
    let provider = Provider::dsh();
    provider
        .controls()
        .scroll(&DialScrollRequest {
            target_bundle_id: "com.deepseek.dsh".into(),
            delta_lines: 0,
            jump_to_bottom: true,
            sidebar: false,
        })
        .unwrap();
    provider
        .controls()
        .shortcut("dsh-desktop", 0x2d, &["command".into()])
        .unwrap();
    assert_eq!(
        provider.events(),
        [
            "capture:com.deepseek.dsh",
            "scroll",
            "focus:3",
            "capture:com.deepseek.dsh",
            "focus:3",
            "targeted_shortcut"
        ]
    );
}

#[test]
fn rapid_clicks_serialize_observation_press_and_readback() {
    let provider = Provider::dsh();
    let controls = Arc::new(provider.controls());
    let barrier = Arc::new(Barrier::new(3));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let controls = controls.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                controls.toggle_dictation("dsh-desktop").unwrap()
            })
        })
        .collect();
    barrier.wait();
    let states: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert!(states.contains(&ObservedDictationState::Idle));
    assert!(states.contains(&ObservedDictationState::Recording));
    assert_eq!(
        provider.events(),
        [
            "capture:com.deepseek.dsh",
            "press:开始听写",
            "capture:com.deepseek.dsh",
            "press:停止听写"
        ]
    );
}

struct Driver {
    controls: ManagedAxControls<Provider>,
    trusted: bool,
}
impl HostOsDriver for Driver {
    fn is_accessibility_trusted(&self) -> bool {
        self.trusted
    }
    fn open_application(&self, _: &str) -> Result<(), String> {
        panic!("must not launch another app")
    }
    fn post_key_shortcut(&self, _: u16, _: &[String], _: bool) -> Result<(), String> {
        panic!("must not send a global shortcut")
    }
    fn post_apple_fn_flags(&self, _: bool) -> Result<(), String> {
        panic!("must not send global Fn")
    }
    fn snapshot_clipboard(&self, _: usize) -> ClipboardSnapshot {
        panic!("must not read clipboard")
    }
    fn write_clipboard_plain_text(&self, _: &str) -> u64 {
        panic!("must not write clipboard")
    }
    fn restore_clipboard(&self, _: &ClipboardSnapshot, _: u64) -> bool {
        panic!("must not change clipboard")
    }
    fn open_url(&self, _: &str) -> Result<(), String> {
        panic!("must not open URL")
    }
    fn focus_agent_input(&self, adapter: &str) -> Result<(), String> {
        self.controls.focus_input(adapter)
    }
    fn stop_active_dictation(&self, adapter: &str) -> Result<bool, String> {
        self.controls.stop_dictation(adapter)
    }
    fn toggle_dictation(&self, adapter: &str) -> Result<ObservedDictationState, String> {
        self.controls.toggle_dictation(adapter)
    }
    fn submit_agent_input(&self, adapter: &str, prompt: Option<&str>) -> Result<(), String> {
        self.controls.submit(adapter, prompt)
    }
    fn insert_agent_prompt(&self, adapter: &str, prompt: &str) -> Result<(), String> {
        self.controls.insert_prompt(adapter, prompt)
    }
    fn agent_shortcut(&self, adapter: &str, key: u16, modifiers: &[String]) -> Result<(), String> {
        self.controls.shortcut(adapter, key, modifiers)
    }
    fn scroll_window(&self, request: &DialScrollRequest) -> Result<(), String> {
        self.controls.scroll(request)
    }
    fn move_cursor_by(&self, _: f64, _: f64) -> Result<(), String> {
        unreachable!()
    }
    fn click_mouse_button(&self, _: MouseButtonKind) -> Result<(), String> {
        unreachable!()
    }
    fn scroll_cursor_wheel(&self, _: i32, _: i32) -> Result<(), String> {
        unreachable!()
    }
    fn new_conversation_in_project(&self, _: &str) -> Result<(), String> {
        unreachable!()
    }
    fn toggle_live_voice(&self, _: &str) -> Result<(), String> {
        unreachable!()
    }
    fn open_model_picker(&self, adapter: &str) -> Result<(), String> {
        self.controls.open_model_picker(adapter)
    }
    fn step_model_picker(&self, adapter: &str, key_code: u16) -> Result<(), String> {
        self.controls.step_model_picker(adapter, key_code)
    }
    fn confirm_model_picker(&self, adapter: &str) -> Result<ModelPickerConfirmResult, String> {
        self.controls.confirm_model_picker(adapter)
    }
    fn open_subagent_picker(&self, adapter: &str) -> Result<SubagentOpenResult, String> {
        self.controls.open_subagent_picker(adapter)
    }
    fn step_subagent_picker(&self, adapter: &str, key_code: u16) -> Result<(), String> {
        self.controls.step_subagent_picker(adapter, key_code)
    }
    fn confirm_subagent_picker(&self, adapter: &str) -> Result<ModelPickerConfirmResult, String> {
        self.controls.confirm_subagent_picker(adapter)
    }
    fn send_model_picker_key(&self, adapter: &str, key_code: u16) -> Result<(), String> {
        self.controls.send_model_picker_key(adapter, key_code)
    }
    fn open_side_panel(&self, adapter: &str) -> Result<(), String> {
        self.controls.set_side_panel_open(adapter, true)
    }
    fn close_side_panel(&self, adapter: &str) -> Result<(), String> {
        self.controls.set_side_panel_open(adapter, false)
    }
    fn open_browser(&self, adapter: &str) -> Result<(), String> {
        self.controls.open_browser(adapter)
    }
}

#[test]
fn live_executor_reports_confirmed_toggle_and_explicit_failure() {
    let provider = Provider::dsh();
    let executor = LiveHostActionExecutor::new(Driver {
        controls: provider.controls(),
        trusted: true,
    });
    let action = ResolvedHostAction::AgentCommand {
        adapter: "dsh-desktop".into(),
        command: "toggle-dictation".into(),
    };
    let receipt = executor.execute("test", "button", &action);
    assert_eq!(receipt.outcome, "executed");
    assert_eq!(receipt.detail, "dictation_started");
    provider.0.lock().unwrap().effect = PressEffect::NoChange;
    let receipt = executor.execute("test", "button", &action);
    assert_eq!(receipt.outcome, "failed");
    assert_eq!(receipt.detail, "unconfirmed:dictation_readback");
    provider.0.lock().unwrap().window.controls.clear();
    assert_eq!(
        executor.execute("test", "button", &action).outcome,
        "blocked"
    );
}

#[test]
fn live_executor_blocks_unknown_native_and_untrusted_adapters_before_actions() {
    for (adapter, trusted) in [("unknown", true), ("herdr", true), ("dsh-desktop", false)] {
        let provider = Provider::dsh();
        let executor = LiveHostActionExecutor::new(Driver {
            controls: provider.controls(),
            trusted,
        });
        let receipt = executor.execute(
            "test",
            "button",
            &ResolvedHostAction::AgentCommand {
                adapter: adapter.into(),
                command: "toggle-dictation".into(),
            },
        );
        assert_eq!(receipt.outcome, "blocked");
        assert!(provider.events().is_empty());
    }
}

#[test]
fn live_executor_uses_targeted_paths_for_submit_and_ordinary_shortcuts() {
    let provider = Provider::dsh();
    let executor = LiveHostActionExecutor::new(Driver {
        controls: provider.controls(),
        trusted: true,
    });
    for command in ["submit", "new-chat", "search", "terminal"] {
        let receipt = executor.execute(
            "test",
            "button",
            &ResolvedHostAction::AgentCommand {
                adapter: "dsh-desktop".into(),
                command: command.into(),
            },
        );
        assert_eq!(receipt.outcome, "executed", "{command}: {}", receipt.detail);
    }
    assert_eq!(
        provider
            .events()
            .iter()
            .filter(|e| e.as_str() == "targeted_shortcut")
            .count(),
        3
    );
}

#[test]
fn shortcut_and_paste_action_types_never_use_global_input_for_managed_targets() {
    for (bundle, adapter) in [
        ("com.google.antigravity", "antigravity"),
        ("com.deepseek.dsh", "dsh-desktop"),
    ] {
        let provider = Provider::dsh();
        provider.0.lock().unwrap().window.identity.bundle_id = bundle.into();
        let executor = LiveHostActionExecutor::new(Driver {
            controls: provider.controls(),
            trusted: true,
        });
        let action = ResolvedHostAction::Shortcut {
            key: "k".into(),
            key_code: 0x28,
            modifiers: vec!["command".into()],
            target_bundle_id: Some(bundle.into()),
        };
        assert_eq!(executor.execute("test", "key", &action).outcome, "executed");
        for (target_bundle_id, adapter, submit) in [
            (Some(bundle.into()), None, false),
            (Some(bundle.into()), Some(adapter.into()), true),
            (None, Some(adapter.into()), false),
        ] {
            let action = ResolvedHostAction::PasteText {
                text: "test\r\n".into(),
                target_bundle_id,
                adapter,
                no_newline: true,
                submit,
            };
            let receipt = executor.execute("test", "key", &action);
            assert_eq!(receipt.outcome, "executed", "{}", receipt.detail);
            assert_eq!(receipt.text_byte_len, Some(4));
        }
        let events = provider.events();
        assert_eq!(
            events
                .iter()
                .filter(|e| e.as_str() == "targeted_shortcut")
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|e| e.as_str() == "insert:test")
                .count(),
            3
        );
        assert_eq!(events.iter().filter(|e| e.as_str() == "submit").count(), 1);
        assert!(events
            .iter()
            .filter(|e| e.starts_with("capture:"))
            .all(|e| e == &format!("capture:{bundle}")));
    }
}

#[test]
fn conflicting_paste_target_and_adapter_do_not_touch_either_app() {
    let provider = Provider::dsh();
    let executor = LiveHostActionExecutor::new(Driver {
        controls: provider.controls(),
        trusted: true,
    });
    for (target, adapter) in [
        ("com.google.antigravity", "dsh-desktop"),
        ("com.apple.TextEdit", "dsh-desktop"),
    ] {
        let receipt = executor.execute(
            "test",
            "key",
            &ResolvedHostAction::PasteText {
                text: "test".into(),
                target_bundle_id: Some(target.into()),
                adapter: Some(adapter.into()),
                no_newline: true,
                submit: false,
            },
        );
        assert_eq!(receipt.outcome, "blocked");
        assert_eq!(receipt.detail, "blocked:managed_target_mismatch");
    }
    assert!(provider.events().is_empty());
}

#[test]
fn absent_voice_capability_does_not_block_submit_but_uncleared_recording_does() {
    let provider = Provider::dsh();
    let controls = provider.controls();
    let voice = provider.0.lock().unwrap().window.controls.remove(0);
    controls.submit("dsh-desktop", None).unwrap();
    provider.0.lock().unwrap().window.controls.insert(0, voice);
    controls.toggle_dictation("dsh-desktop").unwrap();
    provider.0.lock().unwrap().window.controls.remove(0);
    assert!(controls
        .submit("dsh-desktop", None)
        .unwrap_err()
        .starts_with("blocked:"));
    assert_eq!(
        provider
            .events()
            .iter()
            .filter(|e| e.as_str() == "submit")
            .count(),
        1
    );
}

#[test]
fn an_uncleared_recording_cannot_be_stopped_or_submitted_in_a_replacement_window() {
    let provider = Provider::dsh();
    let controls = provider.controls();
    controls.toggle_dictation("dsh-desktop").unwrap();
    provider.0.lock().unwrap().window.identity.window_id = 20;
    assert_eq!(
        controls.submit("dsh-desktop", None).unwrap_err(),
        "blocked:uncleared_dictation_target_changed"
    );
    assert_eq!(
        controls.toggle_dictation("dsh-desktop").unwrap_err(),
        "blocked:uncleared_dictation_target_changed"
    );
    assert_eq!(
        provider
            .events()
            .iter()
            .filter(|e| e.starts_with("press:"))
            .count(),
        1
    );
}

#[test]
fn restarted_app_recovers_only_after_the_previous_process_is_confirmed_exited() {
    for effect in [PressEffect::Toggle, PressEffect::NoChange] {
        let provider = Provider::dsh();
        let controls = provider.controls();
        provider.0.lock().unwrap().effect = effect;
        let _ = controls.toggle_dictation("dsh-desktop");
        {
            let mut state = provider.0.lock().unwrap();
            state.window.identity.pid = 200;
            state.window.identity.window_id = 2;
            state.window.controls[0].label = "开始听写".into();
            state.effect = PressEffect::Toggle;
        }
        assert_eq!(
            controls.toggle_dictation("dsh-desktop").unwrap_err(),
            "blocked:uncleared_dictation_target_changed"
        );
        assert_eq!(
            provider
                .events()
                .iter()
                .filter(|e| e.starts_with("press:"))
                .count(),
            1
        );
        provider.0.lock().unwrap().exited_pids.push(100);
        assert_eq!(
            controls.toggle_dictation("dsh-desktop").unwrap(),
            ObservedDictationState::Recording
        );
        assert_eq!(
            controls.toggle_dictation("dsh-desktop").unwrap(),
            ObservedDictationState::Idle
        );
        assert_eq!(
            provider
                .events()
                .iter()
                .filter(|e| e.starts_with("press:"))
                .count(),
            3
        );
    }
}

#[test]
fn stopping_after_a_confirmed_process_exit_does_not_press_the_new_idle_window() {
    let provider = Provider::dsh();
    let controls = provider.controls();
    controls.toggle_dictation("dsh-desktop").unwrap();
    {
        let mut state = provider.0.lock().unwrap();
        state.window.identity.pid = 200;
        state.window.identity.window_id = 2;
        state.window.controls[0].label = "开始听写".into();
        state.exited_pids.push(100);
    }
    assert!(!controls.stop_dictation("dsh-desktop").unwrap());
    controls.submit("dsh-desktop", None).unwrap();
    assert_eq!(
        provider
            .events()
            .iter()
            .filter(|e| e.starts_with("press:"))
            .count(),
        1
    );
    assert_eq!(
        provider
            .events()
            .iter()
            .filter(|e| e.as_str() == "submit")
            .count(),
        1
    );
}

#[test]
fn current_dsh_labels_are_distinct_from_setup_and_transcription_states() {
    use mi_macos::DictationAdapter;
    for label in ["开始录音", "Start recording"] {
        assert!(DictationAdapter::Dsh.matches_start_label(label));
    }
    for label in ["停止并识别", "Stop and transcribe"] {
        assert!(DictationAdapter::Dsh.matches_stop_label(label));
    }
    for label in [
        "打开语音输入引导",
        "Open voice input setup",
        "下载并准备",
        "正在识别",
    ] {
        assert!(!DictationAdapter::Dsh.matches_start_label(label));
        assert!(!DictationAdapter::Dsh.matches_stop_label(label));
    }
}

#[test]
#[ignore = "explicit opt-in only: operates the real macOS AX target; never sends text"]
fn manual_managed_ax_control() {
    use mi_macos::MacOsSystemDriver;
    assert_eq!(std::env::var("MI_LIVE_AX_VERIFY").as_deref(), Ok("1"));
    let adapter = std::env::var("MI_LIVE_AX_ADAPTER").expect("MI_LIVE_AX_ADAPTER is required");
    assert!(matches!(adapter.as_str(), "antigravity" | "dsh-desktop"));
    let action = std::env::var("MI_LIVE_AX_ACTION").unwrap_or_else(|_| "inspect".into());
    let driver = MacOsSystemDriver::default();
    match action.as_str() {
        "inspect" => println!(
            "{:?}",
            driver
                .inspect_managed_controls(&adapter)
                .expect("inspect target")
        ),
        "focus" => {
            let started = std::time::Instant::now();
            let result = driver.focus_agent_input(&adapter);
            let elapsed = started.elapsed();
            if std::env::var("MI_LIVE_AX_TIMING").as_deref() == Ok("1") {
                let outcome = match &result {
                    Ok(()) => "success",
                    Err(error) if error.starts_with("unconfirmed:") => "unconfirmed",
                    Err(error) if error.starts_with("blocked:") => "blocked",
                    Err(_) => "error",
                };
                println!(
                    "managed_ax_timing action=focus elapsed_us={} result={outcome} confirmed={}",
                    elapsed.as_micros(),
                    result.is_ok()
                );
            }
            result.expect("focus target input");
        }
        "browser" => {
            let started = std::time::Instant::now();
            let result = driver.open_browser(&adapter);
            let elapsed = started.elapsed();
            let outcome = match &result {
                Ok(()) => "success",
                Err(error) if error.starts_with("unconfirmed:") => "unconfirmed",
                Err(error) if error.starts_with("blocked:") => "blocked",
                Err(_) => "error",
            };
            println!(
                "managed_ax_timing action=browser elapsed_us={} result={outcome} confirmed={}",
                elapsed.as_micros(),
                result.is_ok()
            );
            result.expect("open built-in browser");
        }
        "toggle" => println!(
            "{:?}",
            driver
                .toggle_dictation(&adapter)
                .expect("toggle with AX readback")
        ),
        "scroll-up" | "scroll-down" | "bottom" => driver
            .scroll_window(&DialScrollRequest {
                target_bundle_id: mi_macos::executor::bundle_id_for_adapter(&adapter)
                    .unwrap()
                    .into(),
                delta_lines: if action == "scroll-up" { -3 } else { 3 },
                jump_to_bottom: action == "bottom",
                sidebar: false,
            })
            .expect("scroll target"),
        _ => panic!(
            "allowed actions: inspect, focus, browser, toggle, scroll-up, scroll-down, bottom"
        ),
    }
}

#[test]
fn owned_dictation_cleanup_reads_the_original_target_without_capturing_frontmost() {
    let provider = Provider::dsh();
    let controls = provider.controls();
    controls.start_dictation("dsh-desktop").unwrap();
    provider.0.lock().unwrap().capture_error = Some("frontmost changed".into());
    controls.stop_owned_dictations().unwrap();
    controls.stop_owned_dictations().unwrap();
    assert_eq!(
        provider
            .events()
            .iter()
            .filter(|e| e.starts_with("capture:"))
            .count(),
        1
    );
    assert_eq!(
        provider
            .events()
            .iter()
            .filter(|e| e.starts_with("press:"))
            .count(),
        2
    );
    assert_eq!(
        provider.0.lock().unwrap().window.controls[0].label,
        "开始听写"
    );
}

#[test]
fn owned_dictation_cleanup_keeps_failed_target_and_refuses_replacement() {
    let provider = Provider::dsh();
    let controls = provider.controls();
    controls.start_dictation("dsh-desktop").unwrap();
    provider.0.lock().unwrap().read_pid = Some(999);
    assert!(controls.stop_owned_dictations().is_err());
    assert_eq!(
        provider
            .events()
            .iter()
            .filter(|e| e.starts_with("press:"))
            .count(),
        1
    );
    provider.0.lock().unwrap().read_pid = None;
    provider.0.lock().unwrap().effect = PressEffect::NoChange;
    assert!(controls
        .stop_owned_dictations()
        .unwrap_err()
        .contains("unconfirmed"));
    provider.0.lock().unwrap().effect = PressEffect::Toggle;
    controls.stop_owned_dictations().unwrap();
    assert_eq!(
        provider.0.lock().unwrap().window.controls[0].label,
        "开始听写"
    );
}

#[test]
fn rejected_owned_start_never_claims_user_recording_or_unsupported_adapter() {
    let provider = Provider::dsh();
    let controls = provider.controls();
    provider.0.lock().unwrap().window.controls[0].label = "停止听写".into();
    assert!(controls
        .start_dictation("dsh-desktop")
        .unwrap_err()
        .starts_with("blocked:"));
    assert!(controls
        .start_dictation("herdr")
        .unwrap_err()
        .starts_with("blocked:"));
    controls.stop_owned_dictations().unwrap();
    assert!(!provider
        .events()
        .iter()
        .any(|event| event.starts_with("press:")));
}

#[test]
fn uncertain_start_keeps_original_target_until_release_and_zero_press_failure_does_not() {
    let provider = Provider::dsh();
    let controls = provider.controls();
    provider.0.lock().unwrap().read_pid = Some(999);
    assert!(controls
        .start_dictation("dsh-desktop")
        .unwrap_err()
        .starts_with("blocked:"));
    provider.0.lock().unwrap().read_pid = None;
    provider.0.lock().unwrap().window.controls[0].label = "停止听写".into();
    controls.stop_owned_dictations().unwrap();
    assert!(!provider
        .events()
        .iter()
        .any(|event| event.starts_with("press:")));
    provider.0.lock().unwrap().window.controls[0].label = "开始听写".into();
    provider.0.lock().unwrap().effect = PressEffect::NoChange;
    assert!(controls
        .start_dictation("dsh-desktop")
        .unwrap_err()
        .starts_with("unconfirmed:"));
    provider.0.lock().unwrap().window.controls[0].label = "停止听写".into();
    provider.0.lock().unwrap().effect = PressEffect::Toggle;
    controls.stop_owned_dictations().unwrap();
    assert_eq!(
        provider.0.lock().unwrap().window.controls[0].label,
        "开始听写"
    );
}

#[test]
fn owned_dictation_survives_another_apps_capture_and_releases_its_native_target() {
    let provider = Provider::dsh();
    provider.0.lock().unwrap().enforce_target_cache = true;
    let controls = provider.controls();
    controls.start_dictation("dsh-desktop").unwrap();
    controls.inspect("antigravity").unwrap();
    assert_eq!(
        provider
            .0
            .lock()
            .unwrap()
            .latest_target
            .as_ref()
            .unwrap()
            .pid,
        200
    );
    controls.stop_owned_dictations().unwrap();
    assert_eq!(
        provider.0.lock().unwrap().window.controls[0].label,
        "开始听写"
    );
    assert!(provider.0.lock().unwrap().retained_targets.is_empty());
    assert_eq!(
        provider
            .events()
            .iter()
            .filter(|event| event.starts_with("press:"))
            .count(),
        2
    );
}

#[test]
fn codex_owned_start_release_and_submit_wait_for_transcription_completion() {
    let provider = Provider::codex();
    let controls =
        ManagedAxControls::with_readback_timeout(provider.clone(), Duration::from_millis(100));
    controls.focus_input("codex").unwrap();
    controls.start_dictation("codex").unwrap();
    {
        let mut state = provider.0.lock().unwrap();
        state.effect = PressEffect::Finish;
        state.finish_reads_remaining = Some(1);
        state.capture_error = Some("frontmost changed".into());
    }
    controls.stop_owned_dictations().unwrap();
    assert!(provider.0.lock().unwrap().retained_targets.is_empty());
    assert!(!provider.events().iter().any(|event| event == "submit"));
    {
        let mut state = provider.0.lock().unwrap();
        state.capture_error = None;
        state.effect = PressEffect::Toggle;
    }
    controls.start_dictation("codex").unwrap();
    {
        let mut state = provider.0.lock().unwrap();
        state.effect = PressEffect::Finish;
        state.finish_reads_remaining = Some(1);
    }
    controls.submit("codex", Some("message")).unwrap();
    assert_eq!(
        provider
            .events()
            .iter()
            .filter(|event| *event == "submit")
            .count(),
        1
    );
    assert_eq!(
        provider
            .events()
            .iter()
            .filter(|event| *event == "press:Stop dictation")
            .count(),
        2
    );
    assert_eq!(
        provider.0.lock().unwrap().window.controls[0].label,
        "Dictate"
    );
}

#[test]
fn codex_finishing_or_missing_owned_control_never_sends_or_represses() {
    for pending in ["Finishing dictation", "正在完成听写", ""] {
        let provider = Provider::codex();
        let controls = provider.controls();
        controls.start_dictation("codex").unwrap();
        {
            let mut state = provider.0.lock().unwrap();
            if pending.is_empty() {
                state.window.controls.remove(0);
            } else {
                state.window.controls[0].label = pending.into();
                state.window.controls[0].enabled = false;
                state.window.controls[0].pressable = false;
            }
        }
        assert_eq!(
            controls.submit("codex", None).unwrap_err(),
            "unconfirmed:dictation_readback"
        );
        assert!(controls.stop_owned_dictations().is_err());
        assert!(!provider.0.lock().unwrap().retained_targets.is_empty());
        assert_eq!(
            provider
                .events()
                .iter()
                .filter(|event| event.starts_with("press:"))
                .count(),
            1
        );
        assert!(!provider.events().iter().any(|event| event == "submit"));
    }
}

#[test]
fn codex_ambiguous_input_or_dictation_and_changed_target_never_send() {
    let provider = Provider::codex();
    let controls = provider.controls();
    let input = provider.0.lock().unwrap().window.controls[1].clone();
    provider
        .0
        .lock()
        .unwrap()
        .window
        .controls
        .push(ManagedControl { id: 4, ..input });
    assert_eq!(
        controls.submit("codex", None).unwrap_err(),
        "blocked:ambiguous_or_missing_input"
    );
    provider.0.lock().unwrap().window.controls.pop();
    let voice = provider.0.lock().unwrap().window.controls[0].clone();
    provider
        .0
        .lock()
        .unwrap()
        .window
        .controls
        .push(ManagedControl { id: 5, ..voice });
    assert!(controls.start_dictation("codex").is_err());
    provider.0.lock().unwrap().window.controls.pop();
    controls.start_dictation("codex").unwrap();
    provider.0.lock().unwrap().window.identity.scope_id = 2;
    assert_eq!(
        controls.submit("codex", None).unwrap_err(),
        "blocked:uncleared_dictation_target_changed"
    );
    assert!(!provider.events().iter().any(|event| event == "submit"));
}

#[test]
fn codex_toggle_and_prompt_actions_use_confirmed_managed_controls() {
    let provider = Provider::codex();
    let executor = LiveHostActionExecutor::new(Driver {
        controls: provider.controls(),
        trusted: true,
    });
    for command in ["focus-input", "toggle-dictation", "submit-prompt:message"] {
        let receipt = executor.execute(
            "test",
            "button",
            &ResolvedHostAction::AgentCommand {
                adapter: "codex".into(),
                command: command.into(),
            },
        );
        assert_eq!(receipt.outcome, "executed", "{command}: {}", receipt.detail);
    }
    assert_eq!(
        provider
            .events()
            .iter()
            .filter(|event| *event == "submit")
            .count(),
        1
    );
}

#[test]
fn codex_missing_unowned_dictation_control_does_not_block_prompt_submission() {
    let provider = Provider::codex();
    provider.0.lock().unwrap().window.controls.remove(0);
    let executor = LiveHostActionExecutor::new(Driver {
        controls: provider.controls(),
        trusted: true,
    });
    let receipt = executor.execute(
        "test",
        "button",
        &ResolvedHostAction::PasteText {
            text: "message".into(),
            target_bundle_id: Some("com.openai.codex".into()),
            adapter: None,
            no_newline: true,
            submit: true,
        },
    );
    assert_eq!(receipt.outcome, "executed", "{}", receipt.detail);
    assert!(provider
        .events()
        .iter()
        .any(|event| event == "insert:message"));
    assert!(provider.events().iter().any(|event| event == "submit"));
}

#[test]
fn codex_stop_does_not_treat_a_disappeared_control_as_completion() {
    let provider = Provider::codex();
    let controls = provider.controls();
    controls.start_dictation("codex").unwrap();
    provider.0.lock().unwrap().effect = PressEffect::Hide;
    assert_eq!(
        controls.submit("codex", None).unwrap_err(),
        "unconfirmed:dictation_readback"
    );
    assert!(controls.stop_owned_dictations().is_err());
    assert!(!provider.0.lock().unwrap().retained_targets.is_empty());
    assert_eq!(
        provider
            .events()
            .iter()
            .filter(|event| event.starts_with("press:"))
            .count(),
        2
    );
    assert!(!provider.events().iter().any(|event| event == "submit"));
}

#[test]
fn codex_reasoning_effort_opens_popover_sends_arrow_and_auto_dismisses() {
    let provider = Provider::codex();
    provider
        .0
        .lock()
        .unwrap()
        .window
        .controls
        .push(ManagedControl {
            id: 10,
            role: "AXPopUpButton".into(),
            label: "o3-mini Medium".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: None,
        });
    let controls = provider.controls_with_delay(Duration::from_millis(60));
    controls
        .codex_reasoning(mi_macos::managed_controls::CodexReasoningAction::DecreaseEffort)
        .unwrap();

    let events = provider.events();
    assert!(events.contains(&"press:o3-mini Medium".to_string()));
    assert!(events.contains(&"window_key:arrow_left".to_string()));
    assert!(!events.contains(&"window_key:escape".to_string()));

    std::thread::sleep(Duration::from_millis(100));
    let events = provider.events();
    assert!(events.contains(&"window_key:escape".to_string()));
}

#[test]
fn codex_reasoning_effort_repeat_turn_cancels_previous_auto_dismiss() {
    let provider = Provider::codex();
    provider
        .0
        .lock()
        .unwrap()
        .window
        .controls
        .push(ManagedControl {
            id: 10,
            role: "AXPopUpButton".into(),
            label: "Select effort".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: None,
        });
    let controls = provider.controls_with_delay(Duration::from_millis(70));
    controls
        .codex_reasoning(mi_macos::managed_controls::CodexReasoningAction::IncreaseEffort)
        .unwrap();

    std::thread::sleep(Duration::from_millis(30));
    controls
        .codex_reasoning(mi_macos::managed_controls::CodexReasoningAction::IncreaseEffort)
        .unwrap();

    std::thread::sleep(Duration::from_millis(50));
    let escape_count = provider
        .events()
        .iter()
        .filter(|e| *e == "window_key:escape")
        .count();
    assert_eq!(
        escape_count, 0,
        "turn 1 auto-dismiss must be cancelled by turn 2"
    );

    std::thread::sleep(Duration::from_millis(50));
    let escape_count = provider
        .events()
        .iter()
        .filter(|e| *e == "window_key:escape")
        .count();
    assert_eq!(escape_count, 1, "turn 2 auto-dismiss must fire once");
}

#[test]
fn codex_reasoning_effort_no_op_when_popover_already_closed() {
    let provider = Provider::codex();
    provider
        .0
        .lock()
        .unwrap()
        .window
        .controls
        .push(ManagedControl {
            id: 10,
            role: "AXPopUpButton".into(),
            label: "Select model".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: None,
        });
    let controls = provider.controls_with_delay(Duration::from_millis(60));
    controls
        .codex_reasoning(mi_macos::managed_controls::CodexReasoningAction::DecreaseEffort)
        .unwrap();

    provider
        .0
        .lock()
        .unwrap()
        .window
        .controls
        .retain(|c| c.role != "AXMenuItem");

    std::thread::sleep(Duration::from_millis(100));
    assert!(
        !provider.events().contains(&"window_key:escape".to_string()),
        "auto-dismiss must not send Escape if popover was already closed"
    );
}

#[test]
fn codex_reasoning_fails_closed_when_trigger_missing_or_ambiguous() {
    let provider = Provider::codex();
    let controls = provider.controls();
    let err = controls
        .codex_reasoning(mi_macos::managed_controls::CodexReasoningAction::DecreaseEffort)
        .unwrap_err();
    assert_eq!(err, "blocked:reasoning_control_unavailable");

    provider
        .0
        .lock()
        .unwrap()
        .window
        .controls
        .push(ManagedControl {
            id: 10,
            role: "AXPopUpButton".into(),
            label: "o1 High".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: None,
        });
    provider
        .0
        .lock()
        .unwrap()
        .window
        .controls
        .push(ManagedControl {
            id: 11,
            role: "AXPopUpButton".into(),
            label: "o3-mini Medium".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: None,
        });
    let err = controls
        .codex_reasoning(mi_macos::managed_controls::CodexReasoningAction::IncreaseEffort)
        .unwrap_err();
    assert_eq!(err, "blocked:reasoning_control_unavailable");
}

#[test]
fn codex_toggle_fast_mode_flips_label_and_schedules_auto_dismiss() {
    let provider = Provider::codex();
    provider
        .0
        .lock()
        .unwrap()
        .window
        .controls
        .push(ManagedControl {
            id: 10,
            role: "AXPopUpButton".into(),
            label: "GPT-4o Standard".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: None,
        });
    let controls = provider.controls_with_delay(Duration::from_millis(60));
    controls
        .codex_reasoning(mi_macos::managed_controls::CodexReasoningAction::ToggleFastMode)
        .unwrap();

    let events = provider.events();
    assert!(events.contains(&"press:GPT-4o Standard".to_string()));
    assert!(events.contains(&"press:Enable fast mode".to_string()));

    std::thread::sleep(Duration::from_millis(100));
    assert!(provider.events().contains(&"window_key:escape".to_string()));
}

#[test]
fn codex_toggle_fast_mode_missing_item_closes_popover_and_fails() {
    let provider = Provider::codex();
    provider.0.lock().unwrap().include_fast_mode = false;
    provider
        .0
        .lock()
        .unwrap()
        .window
        .controls
        .push(ManagedControl {
            id: 10,
            role: "AXPopUpButton".into(),
            label: "Auto Standard".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: None,
        });
    let controls = provider.controls();
    let err = controls
        .codex_reasoning(mi_macos::managed_controls::CodexReasoningAction::ToggleFastMode)
        .unwrap_err();
    assert_eq!(err, "blocked:fast_mode_unavailable");
    assert!(provider.events().contains(&"window_key:escape".to_string()));
}

#[test]
fn codex_targeted_scroll_without_ax_scroll_area_succeeds_and_rejects_sidebar() {
    let provider = Provider::codex();
    let controls = provider.controls();

    controls
        .scroll(&DialScrollRequest {
            target_bundle_id: "com.openai.codex".into(),
            delta_lines: -4,
            jump_to_bottom: false,
            sidebar: false,
        })
        .unwrap();

    controls
        .scroll(&DialScrollRequest {
            target_bundle_id: "com.openai.codex".into(),
            delta_lines: 4,
            jump_to_bottom: false,
            sidebar: false,
        })
        .unwrap();

    controls
        .scroll(&DialScrollRequest {
            target_bundle_id: "com.openai.codex".into(),
            delta_lines: 120,
            jump_to_bottom: true,
            sidebar: false,
        })
        .unwrap();

    let err = controls
        .scroll(&DialScrollRequest {
            target_bundle_id: "com.openai.codex".into(),
            delta_lines: 4,
            jump_to_bottom: false,
            sidebar: true,
        })
        .unwrap_err();
    assert_eq!(
        err,
        "blocked:target_scrollbar_or_bottom_control_unavailable"
    );
}

#[test]
fn codex_reasoning_prefers_explicit_effort_over_fallback_and_ignores_non_effort_controls() {
    let provider = Provider::codex();
    provider.0.lock().unwrap().window.controls.extend([
        ManagedControl {
            id: 10,
            role: "AXPopUpButton".into(),
            label: "Select effort".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: None,
        },
        ManagedControl {
            id: 11,
            role: "AXPopUpButton".into(),
            label: "GPT-6 Astra".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: None,
        },
        ManagedControl {
            id: 12,
            role: "AXPopUpButton".into(),
            label: "Codex Settings".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: None,
        },
        ManagedControl {
            id: 13,
            role: "AXPopUpButton".into(),
            label: "Auto approve".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: None,
        },
    ]);
    let controls = provider.controls_with_delay(Duration::from_millis(60));
    controls
        .codex_reasoning(mi_macos::managed_controls::CodexReasoningAction::IncreaseEffort)
        .unwrap();

    let events = provider.events();
    assert!(events.contains(&"press:Select effort".to_string()));
    assert!(!events.contains(&"press:GPT-6 Astra".to_string()));
    assert!(!events.contains(&"press:Codex Settings".to_string()));
    assert!(!events.contains(&"press:Auto approve".to_string()));
}

#[test]
fn codex_reasoning_manually_opened_popover_does_not_schedule_auto_dismiss() {
    let provider = Provider::codex();
    provider.0.lock().unwrap().window.controls.extend([
        ManagedControl {
            id: 1001,
            role: "AXMenuItem".into(),
            label: "Select model".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: None,
        },
        ManagedControl {
            id: 1002,
            role: "AXMenuItem".into(),
            label: "Enable fast mode".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: None,
        },
    ]);
    let controls = provider.controls_with_delay(Duration::from_millis(60));
    controls
        .codex_reasoning(mi_macos::managed_controls::CodexReasoningAction::DecreaseEffort)
        .unwrap();

    let events = provider.events();
    assert!(events.contains(&"window_key:arrow_left".to_string()));

    std::thread::sleep(Duration::from_millis(100));
    let events = provider.events();
    assert!(
        !events.contains(&"window_key:escape".to_string()),
        "manually opened popover must not send escape after auto-dismiss delay"
    );
}

#[test]
fn codex_toggle_fast_mode_unconfirmed_flip_fails_closed_and_closes_popover() {
    let provider = Provider::codex();
    provider.0.lock().unwrap().effect = PressEffect::NoChange;
    provider
        .0
        .lock()
        .unwrap()
        .window
        .controls
        .push(ManagedControl {
            id: 10,
            role: "AXPopUpButton".into(),
            label: "o3-mini Medium".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: None,
        });
    let controls = provider.controls();
    let err = controls
        .codex_reasoning(mi_macos::managed_controls::CodexReasoningAction::ToggleFastMode)
        .unwrap_err();
    assert_eq!(err, "unconfirmed:fast_mode_toggle_readback");
    assert!(provider.events().contains(&"window_key:escape".to_string()));
}

#[test]
fn codex_fast_mode_requires_positive_readback_after_press() {
    for effect in [
        PressEffect::LoseTarget,
        PressEffect::LoseScope,
        PressEffect::Hide,
    ] {
        let provider = Provider::codex();
        {
            let mut state = provider.0.lock().unwrap();
            state.effect = effect;
            state.window.controls = vec![ManagedControl {
                id: 1002,
                role: "AXMenuItem".into(),
                label: "Enable fast mode".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: None,
            }];
        }
        let error = provider
            .controls()
            .codex_reasoning(mi_macos::managed_controls::CodexReasoningAction::ToggleFastMode)
            .unwrap_err();
        assert_eq!(error, "unconfirmed:fast_mode_toggle_readback");
        assert_eq!(
            provider.events(),
            vec!["capture:com.openai.codex", "press:Enable fast mode"]
        );
    }
}

#[test]
fn model_picker_navigation_keys_deliver_direct_window_keys_without_refocusing_composer() {
    for (bundle, adapter, trigger_label) in [
        (
            "com.google.antigravity",
            "antigravity",
            "Select model, current: Gemini 4 Carbon High",
        ),
        ("com.openai.codex", "codex", "GPT-5.4 Standard"),
        ("com.deepseek.dsh", "dsh-desktop", "DeepSeek-R1"),
    ] {
        let provider = Provider::with_target(bundle, "Dictate");
        provider
            .0
            .lock()
            .unwrap()
            .window
            .controls
            .push(ManagedControl {
                id: 50,
                role: "AXPopUpButton".into(),
                label: trigger_label.into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: Some(false),
            });
        let executor = LiveHostActionExecutor::new(Driver {
            controls: provider.controls(),
            trusted: true,
        });
        for (command, expected_event) in [
            ("model-picker-prev", "window_key:arrow_up"),
            ("model-picker-next", "window_key:arrow_down"),
            ("model-picker-effort-right", "window_key:arrow_right"),
            ("model-picker-effort-left", "window_key:arrow_left"),
            ("model-picker-confirm", "window_key:return"),
            ("model-picker-cancel", "window_key:escape"),
        ] {
            let receipt = executor.execute(
                "Local Antigravity",
                "ENC_CW",
                &ResolvedHostAction::AgentCommand {
                    adapter: adapter.into(),
                    command: command.into(),
                },
            );
            assert_eq!(
                receipt.outcome, "executed",
                "{adapter}/{command}: {}",
                receipt.detail
            );
            assert!(
                provider.events().iter().any(|ev| ev == expected_event),
                "missing {expected_event} in {:?}",
                provider.events()
            );
        }
        assert!(
            !provider.events().iter().any(|ev| ev.starts_with("focus:")),
            "popup navigation must never refocus composer for {adapter}: {:?}",
            provider.events()
        );
    }
}

#[test]
fn model_picker_open_supports_antigravity_codex_and_dsh_with_ax_fallbacks() {
    let ag_provider = Provider::with_target("com.google.antigravity", "Start dictation");
    let ag_executor = LiveHostActionExecutor::new(Driver {
        controls: ag_provider.controls(),
        trusted: true,
    });
    for cmd in ["model", "model-picker-open"] {
        let receipt = ag_executor.execute(
            "Local Antigravity",
            "ENC_CLK",
            &ResolvedHostAction::AgentCommand {
                adapter: "antigravity".into(),
                command: cmd.into(),
            },
        );
        assert_eq!(receipt.outcome, "executed", "{cmd}: {}", receipt.detail);
    }
    assert_eq!(
        ag_provider
            .events()
            .iter()
            .filter(|ev| ev.as_str() == "targeted_shortcut")
            .count(),
        2
    );

    let codex_provider = Provider::codex();
    let codex_executor = LiveHostActionExecutor::new(Driver {
        controls: codex_provider.controls(),
        trusted: true,
    });
    let codex_receipt = codex_executor.execute(
        "Local Codex",
        "ENC_CLK",
        &ResolvedHostAction::AgentCommand {
            adapter: "codex".into(),
            command: "model-picker-open".into(),
        },
    );
    assert_eq!(codex_receipt.outcome, "executed");
    assert!(codex_provider
        .events()
        .contains(&"window_key:control+shift:2e".to_string()));
    assert!(!codex_provider
        .events()
        .iter()
        .any(|ev| ev.starts_with("focus:")));

    let codex_ax_provider = Provider::codex();
    {
        let mut st = codex_ax_provider.0.lock().unwrap();
        st.window_key_error = Some("blocked:window_key_unavailable".into());
        st.window.controls.push(ManagedControl {
            id: 20,
            role: "AXPopUpButton".into(),
            label: "GPT-5.4 Standard".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: None,
        });
    }
    let codex_ax_executor = LiveHostActionExecutor::new(Driver {
        controls: codex_ax_provider.controls(),
        trusted: true,
    });
    let codex_ax_receipt = codex_ax_executor.execute(
        "Local Codex",
        "ENC_CLK",
        &ResolvedHostAction::AgentCommand {
            adapter: "host".into(),
            command: "model-picker-open".into(),
        },
    );
    assert_eq!(codex_ax_receipt.outcome, "executed");
    assert!(codex_ax_provider
        .events()
        .contains(&"press:GPT-5.4 Standard".to_string()));
    assert!(codex_ax_provider
        .events()
        .contains(&"press:Select model".to_string()));

    let dsh_provider = Provider::dsh();
    dsh_provider
        .0
        .lock()
        .unwrap()
        .window
        .controls
        .push(ManagedControl {
            id: 30,
            role: "AXPopUpButton".into(),
            label: "DeepSeek-R1".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: None,
        });
    let dsh_executor = LiveHostActionExecutor::new(Driver {
        controls: dsh_provider.controls(),
        trusted: true,
    });
    let dsh_receipt = dsh_executor.execute(
        "Local DeepSeek",
        "ENC_CLK",
        &ResolvedHostAction::AgentCommand {
            adapter: "dsh-desktop".into(),
            command: "model".into(),
        },
    );
    assert_eq!(dsh_receipt.outcome, "executed");
    assert!(dsh_provider
        .events()
        .contains(&"press:DeepSeek-R1".to_string()));
    assert!(!dsh_provider
        .events()
        .iter()
        .any(|ev| ev.starts_with("focus:")));

    let dsh_fallback_provider = Provider::dsh();
    let dsh_fallback_executor = LiveHostActionExecutor::new(Driver {
        controls: dsh_fallback_provider.controls(),
        trusted: true,
    });
    let dsh_fallback_receipt = dsh_fallback_executor.execute(
        "Local DeepSeek",
        "ENC_CLK",
        &ResolvedHostAction::AgentCommand {
            adapter: "dsh-desktop".into(),
            command: "model-picker-open".into(),
        },
    );
    assert_eq!(dsh_fallback_receipt.outcome, "executed");
    assert!(dsh_fallback_provider
        .events()
        .contains(&"targeted_shortcut".to_string()));
}

#[test]
fn model_picker_mouse_capture_bridge_is_noop_in_isolated_mode_and_drains_cleanly() {
    use mi_macos::executor::{MacHostExecutor, MacOsSystemDriver, RecordingOsDriver};

    let isolated_exec = MacHostExecutor::new(MacOsSystemDriver::new_isolated());
    isolated_exec.set_model_picker_mouse_capture(true);
    assert!(isolated_exec.drain_model_picker_mouse_deltas().is_empty());
    isolated_exec.set_model_picker_mouse_capture(false);
    assert!(isolated_exec.drain_model_picker_mouse_deltas().is_empty());

    let rec_driver = RecordingOsDriver::new(true);
    let rec_exec = LiveHostActionExecutor::new(rec_driver.clone());
    rec_driver.push_model_picker_mouse_delta(16.0, 2.0);
    assert!(rec_exec.drain_model_picker_mouse_deltas().is_empty());

    rec_exec.set_model_picker_mouse_capture(true);
    assert!(rec_driver.is_model_picker_mouse_capture_enabled());
    rec_driver.push_model_picker_mouse_delta(18.0, 1.0);
    rec_driver.push_model_picker_mouse_delta(14.0, -2.0);
    assert_eq!(
        rec_exec.drain_model_picker_mouse_deltas(),
        vec![(18.0, 1.0), (14.0, -2.0)]
    );
    assert!(rec_exec.drain_model_picker_mouse_deltas().is_empty());

    rec_driver.push_model_picker_mouse_delta(-30.0, 0.0);
    rec_exec.stop_owned_actions().unwrap();
    assert!(!rec_driver.is_model_picker_mouse_capture_enabled());
    assert!(rec_exec.drain_model_picker_mouse_deltas().is_empty());
}

#[test]
fn host_search_command_resolves_to_active_agent_and_posts_cmd_k() {
    use mi_macos::executor::{RecordedOsEvent, RecordingOsDriver};

    let rec_driver = RecordingOsDriver::new(true);
    let executor = LiveHostActionExecutor::new(rec_driver.clone());

    let receipt_ag = executor.execute(
        "Local Antigravity",
        "ENC_CLK",
        &ResolvedHostAction::AgentCommand {
            adapter: "host".into(),
            command: "search".into(),
        },
    );
    assert_eq!(receipt_ag.outcome, "executed");
    let events = rec_driver.events();
    assert!(events.iter().any(|ev| matches!(
        ev,
        RecordedOsEvent::OpenApplication(bundle) if bundle == "com.google.antigravity"
    )));
    assert!(events.iter().any(|ev| matches!(
        ev,
        RecordedOsEvent::PostKeyShortcut {
            key_code: 0x28,
            modifiers,
            key_down: true,
        } if modifiers == &["command".to_string()]
    )));

    let receipt_codex = executor.execute(
        "Local Codex",
        "ENC_CLK",
        &ResolvedHostAction::AgentCommand {
            adapter: "host".into(),
            command: "search".into(),
        },
    );
    assert_eq!(receipt_codex.outcome, "executed");
    let events = rec_driver.events();
    assert!(events.iter().any(|ev| matches!(
        ev,
        RecordedOsEvent::OpenApplication(bundle) if bundle == "com.openai.codex"
    )));
    assert!(events.iter().any(|ev| matches!(
        ev,
        RecordedOsEvent::PostKeyShortcut {
            key_code: 0x28,
            modifiers,
            key_down: true,
        } if modifiers == &["command".to_string()]
    )));
}

#[test]
fn antigravity_direct_step_activates_window_presses_select_model_and_steps_without_focusing_composer(
) {
    let provider = Provider::with_target("com.google.antigravity", "Start dictation");
    let trigger_label = "Select model, current: Gemini 4 Carbon High";
    provider
        .0
        .lock()
        .unwrap()
        .window
        .controls
        .push(ManagedControl {
            id: 40,
            role: "AXPopUpButton".into(),
            label: trigger_label.into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: Some(false),
        });
    let executor = LiveHostActionExecutor::new(Driver {
        controls: provider.controls(),
        trusted: true,
    });

    let first_turn = executor.execute(
        "Local Antigravity",
        "ENC_CW",
        &ResolvedHostAction::AgentCommand {
            adapter: "antigravity".into(),
            command: "model-picker-next".into(),
        },
    );
    assert_eq!(first_turn.outcome, "executed");
    assert_eq!(first_turn.detail, "antigravity/model-picker-next");
    assert_eq!(
        provider.events(),
        vec![
            "capture:com.google.antigravity",
            "activate_window",
            &format!("press:{trigger_label}"),
            "window_key:arrow_down",
        ]
    );
    assert_eq!(
        provider.0.lock().unwrap().window.controls[2].expanded,
        Some(true)
    );

    let second_turn = executor.execute(
        "Local Antigravity",
        "ENC_CW",
        &ResolvedHostAction::AgentCommand {
            adapter: "antigravity".into(),
            command: "model-picker-next".into(),
        },
    );
    assert_eq!(second_turn.outcome, "executed");
    assert_eq!(
        provider.events(),
        vec![
            "capture:com.google.antigravity",
            "activate_window",
            &format!("press:{trigger_label}"),
            "window_key:arrow_down",
            "capture:com.google.antigravity",
            "window_key:arrow_down",
        ]
    );

    let idempotent_open = executor.execute(
        "Local Antigravity",
        "ENC_CLK",
        &ResolvedHostAction::AgentCommand {
            adapter: "antigravity".into(),
            command: "model-picker-open".into(),
        },
    );
    assert_eq!(idempotent_open.outcome, "executed");
    assert_eq!(
        provider
            .events()
            .iter()
            .filter(|ev| ev.starts_with("press:"))
            .count(),
        1
    );
    assert!(!provider.events().iter().any(|ev| ev.starts_with("focus:")));

    {
        let mut st = provider.0.lock().unwrap();
        st.window.controls[2].expanded = Some(false);
        st.effect = PressEffect::NoChange;
    }
    let unconfirmed_step = executor.execute(
        "Local Antigravity",
        "ENC_CW",
        &ResolvedHostAction::AgentCommand {
            adapter: "antigravity".into(),
            command: "model-picker-next".into(),
        },
    );
    assert_eq!(unconfirmed_step.outcome, "failed");
    assert_eq!(
        unconfirmed_step.detail,
        "unconfirmed:model_picker_open_readback"
    );
}

#[test]
fn external_popup_dismissal_reopens_on_next_step_and_confirm_reopens_without_sending_return() {
    let provider = Provider::with_target("com.google.antigravity", "Start dictation");
    let trigger_label = "Select model, current: Gemini 4 Carbon High";
    provider
        .0
        .lock()
        .unwrap()
        .window
        .controls
        .push(ManagedControl {
            id: 41,
            role: "AXPopUpButton".into(),
            label: trigger_label.into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: Some(false),
        });
    let executor = LiveHostActionExecutor::new(Driver {
        controls: provider.controls(),
        trusted: true,
    });

    let open = executor.execute(
        "Local Antigravity",
        "ENC_CLK",
        &ResolvedHostAction::AgentCommand {
            adapter: "antigravity".into(),
            command: "model-picker-open".into(),
        },
    );
    assert_eq!(open.outcome, "executed");
    assert_eq!(
        provider.0.lock().unwrap().window.controls[2].expanded,
        Some(true)
    );

    provider.0.lock().unwrap().window.controls[2].expanded = Some(false);
    provider.0.lock().unwrap().events.clear();

    let step_after_dismiss = executor.execute(
        "Local Antigravity",
        "ENC_CC",
        &ResolvedHostAction::AgentCommand {
            adapter: "antigravity".into(),
            command: "model-picker-prev".into(),
        },
    );
    assert_eq!(step_after_dismiss.outcome, "executed");
    assert_eq!(
        provider.events(),
        vec![
            "capture:com.google.antigravity",
            "activate_window",
            &format!("press:{trigger_label}"),
            "window_key:arrow_up",
        ]
    );

    provider.0.lock().unwrap().window.controls[2].expanded = Some(false);
    provider.0.lock().unwrap().events.clear();

    let confirm_when_closed = executor.execute(
        "Local Antigravity",
        "ENC_CLK",
        &ResolvedHostAction::AgentCommand {
            adapter: "host".into(),
            command: "model-picker-confirm".into(),
        },
    );
    assert_eq!(confirm_when_closed.outcome, "executed");
    assert_eq!(
        confirm_when_closed.detail,
        "antigravity/model-picker-already-closed"
    );
    assert_eq!(provider.events(), vec!["capture:com.google.antigravity"]);
    assert!(!provider.events().contains(&"window_key:return".to_string()));
    assert_eq!(
        provider.0.lock().unwrap().window.controls[2].expanded,
        Some(false)
    );

    provider.0.lock().unwrap().window.controls[2].expanded = Some(true);
    provider.0.lock().unwrap().events.clear();
    let confirm_when_open = executor.execute(
        "Local Antigravity",
        "ENC_CLK",
        &ResolvedHostAction::AgentCommand {
            adapter: "antigravity".into(),
            command: "model-picker-confirm".into(),
        },
    );
    assert_eq!(confirm_when_open.outcome, "executed");
    assert_eq!(confirm_when_open.detail, "antigravity/model-picker-confirm");
    assert_eq!(
        provider.events(),
        vec!["capture:com.google.antigravity", "window_key:return"]
    );

    let codex_provider = Provider::codex();
    codex_provider
        .0
        .lock()
        .unwrap()
        .window
        .controls
        .push(ManagedControl {
            id: 42,
            role: "AXPopUpButton".into(),
            label: "GPT-5.4 Standard".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: Some(false),
        });
    let codex_executor = LiveHostActionExecutor::new(Driver {
        controls: codex_provider.controls(),
        trusted: true,
    });
    let codex_closed = codex_executor.execute(
        "Local Codex",
        "ENC_CLK",
        &ResolvedHostAction::AgentCommand {
            adapter: "codex".into(),
            command: "model-picker-confirm".into(),
        },
    );
    assert_eq!(codex_closed.outcome, "executed");
    assert_eq!(codex_closed.detail, "codex/model-picker-already-closed");
    assert_eq!(codex_provider.events(), vec!["capture:com.openai.codex"]);
    assert!(!codex_provider
        .events()
        .contains(&"window_key:return".to_string()));
}

#[test]
fn search_navigation_commands_send_direct_window_keys_without_pressing_model_picker() {
    let provider = Provider::with_target("com.google.antigravity", "Start dictation");
    provider
        .0
        .lock()
        .unwrap()
        .window
        .controls
        .push(ManagedControl {
            id: 45,
            role: "AXPopUpButton".into(),
            label: "Select model, current: Gemini 4 Carbon High".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: Some(false),
        });
    let executor = LiveHostActionExecutor::new(Driver {
        controls: provider.controls(),
        trusted: true,
    });
    let simulated = SimulatedHostActionExecutor::new();

    for (command, expected_key) in [
        ("search-prev", "window_key:arrow_up"),
        ("search-next", "window_key:arrow_down"),
        ("search-confirm", "window_key:return"),
    ] {
        let action = ResolvedHostAction::AgentCommand {
            adapter: "antigravity".into(),
            command: command.into(),
        };
        let sim_receipt = simulated.execute("Local Antigravity", "ENC_CW", &action);
        assert_eq!(sim_receipt.outcome, "simulated");
        assert_eq!(sim_receipt.detail, format!("antigravity/{command}"));

        provider.0.lock().unwrap().events.clear();
        let live_receipt = executor.execute("Local Antigravity", "ENC_CW", &action);
        assert_eq!(live_receipt.outcome, "executed");
        assert_eq!(live_receipt.detail, format!("antigravity/{command}"));
        assert_eq!(
            provider.events(),
            vec!["capture:com.google.antigravity", expected_key]
        );
    }
    assert_eq!(
        provider.0.lock().unwrap().window.controls[2].expanded,
        Some(false)
    );
}

#[test]
fn codex_modal_focusing_composer_reopens_on_confirm_while_unfocused_composer_steps_and_confirms() {
    let provider = Provider::codex();
    provider
        .0
        .lock()
        .unwrap()
        .window
        .controls
        .push(ManagedControl {
            id: 46,
            role: "AXPopUpButton".into(),
            label: "GPT-5.4 Standard".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: Some(false),
        });
    let executor = LiveHostActionExecutor::new(Driver {
        controls: provider.controls(),
        trusted: true,
    });

    let open_receipt = executor.execute(
        "Local Codex",
        "ENC_CLK",
        &ResolvedHostAction::AgentCommand {
            adapter: "codex".into(),
            command: "model-picker-open".into(),
        },
    );
    assert_eq!(open_receipt.outcome, "executed");
    assert!(provider
        .events()
        .contains(&"window_key:control+shift:2e".to_string()));
    assert_eq!(
        provider.0.lock().unwrap().window.controls[2].expanded,
        Some(false)
    );

    provider.0.lock().unwrap().events.clear();
    let step_receipt = executor.execute(
        "Local Codex",
        "ENC_CW",
        &ResolvedHostAction::AgentCommand {
            adapter: "codex".into(),
            command: "model-picker-next".into(),
        },
    );
    assert_eq!(step_receipt.outcome, "executed");
    assert_eq!(
        provider.events(),
        vec!["capture:com.openai.codex", "window_key:arrow_down"]
    );

    provider.0.lock().unwrap().events.clear();
    let confirm_receipt = executor.execute(
        "Local Codex",
        "ENC_CLK",
        &ResolvedHostAction::AgentCommand {
            adapter: "codex".into(),
            command: "model-picker-confirm".into(),
        },
    );
    assert_eq!(confirm_receipt.outcome, "executed");
    assert_eq!(confirm_receipt.detail, "codex/model-picker-confirm");
    assert_eq!(
        provider.events(),
        vec!["capture:com.openai.codex", "window_key:return"]
    );

    let reopen_receipt = executor.execute(
        "Local Codex",
        "ENC_CLK",
        &ResolvedHostAction::AgentCommand {
            adapter: "codex".into(),
            command: "model-picker-open".into(),
        },
    );
    assert_eq!(reopen_receipt.outcome, "executed");

    {
        let mut st = provider.0.lock().unwrap();
        st.window.controls[1].focused = true;
        st.events.clear();
    }

    let guarded_confirm = executor.execute(
        "Local Codex",
        "ENC_CLK",
        &ResolvedHostAction::AgentCommand {
            adapter: "codex".into(),
            command: "model-picker-confirm".into(),
        },
    );
    assert_eq!(guarded_confirm.outcome, "executed");
    assert_eq!(guarded_confirm.detail, "codex/model-picker-already-closed");
    assert_eq!(provider.events(), vec!["capture:com.openai.codex"]);
    assert!(!provider.events().contains(&"window_key:return".to_string()));
}

#[test]
fn side_question_flow_opens_closed_side_panel_then_clicks_side_question_and_focuses_input() {
    let provider = Provider::with_target("com.google.antigravity", "开始听写");
    {
        let mut state = provider.0.lock().unwrap();
        state.effect = PressEffect::NoChange;
        state.window.controls = vec![ManagedControl {
            id: 10,
            role: "AXButton".into(),
            label: "Toggle Side Panel".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: Some(false),
        }];
        state.after_press = Some(|fixture, label, _id| {
            if label == "Toggle Side Panel" {
                if let Some(btn) = fixture.window.controls.iter_mut().find(|c| c.id == 10) {
                    btn.expanded = Some(true);
                }
                fixture.window.controls.push(ManagedControl {
                    id: 20,
                    role: "AXButton".into(),
                    label: "Side Question".into(),
                    enabled: true,
                    pressable: true,
                    editable: false,
                    focused: false,
                    expanded: None,
                });
            } else if label == "Side Question" {
                fixture.window.controls.push(ManagedControl {
                    id: 30,
                    role: "AXButton".into(),
                    label: "New Side Question".into(),
                    enabled: true,
                    pressable: true,
                    editable: false,
                    focused: false,
                    expanded: None,
                });
                fixture.window.controls.push(ManagedControl {
                    id: 31,
                    role: "AXTextArea".into(),
                    label: "Side Question Input".into(),
                    enabled: true,
                    pressable: false,
                    editable: true,
                    focused: false,
                    expanded: None,
                });
            }
        });
    }

    let controls = provider.controls();
    controls.open_side_question("antigravity").unwrap();

    let events = provider.events();
    assert!(events.contains(&"activate_window".to_string()));
    assert!(events.contains(&"press:Toggle Side Panel".to_string()));
    assert!(events.contains(&"press:Side Question".to_string()));
    assert!(events.contains(&"focus:31".to_string()));
}

#[test]
fn side_question_flow_clicks_new_tab_in_open_side_panel_then_clicks_side_question_and_focuses() {
    let provider = Provider::with_target("com.google.antigravity", "开始听写");
    {
        let mut state = provider.0.lock().unwrap();
        state.effect = PressEffect::NoChange;
        state.window.controls = vec![
            ManagedControl {
                id: 10,
                role: "AXButton".into(),
                label: "Toggle Side Panel".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: Some(true),
            },
            ManagedControl {
                id: 11,
                role: "AXButton".into(),
                label: "New tab".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: None,
            },
        ];
        state.after_press = Some(|fixture, label, _id| {
            if label == "New tab" {
                fixture.window.controls.push(ManagedControl {
                    id: 20,
                    role: "AXButton".into(),
                    label: "Side Question".into(),
                    enabled: true,
                    pressable: true,
                    editable: false,
                    focused: false,
                    expanded: None,
                });
            } else if label == "Side Question" {
                fixture.window.controls.push(ManagedControl {
                    id: 30,
                    role: "AXButton".into(),
                    label: "New Side Question".into(),
                    enabled: true,
                    pressable: true,
                    editable: false,
                    focused: false,
                    expanded: None,
                });
                fixture.window.controls.push(ManagedControl {
                    id: 31,
                    role: "AXTextArea".into(),
                    label: "Side Question Input".into(),
                    enabled: true,
                    pressable: false,
                    editable: true,
                    focused: false,
                    expanded: None,
                });
            }
        });
    }

    let controls = provider.controls();
    controls.open_side_question("antigravity").unwrap();

    let events = provider.events();
    assert!(events.contains(&"activate_window".to_string()));
    assert!(events.contains(&"press:New tab".to_string()));
    assert!(events.contains(&"press:Side Question".to_string()));
    assert!(events.contains(&"focus:31".to_string()));
}

#[test]
fn side_question_returns_blocked_when_side_question_button_disabled_with_zero_press() {
    let provider = Provider::with_target("com.google.antigravity", "开始听写");
    {
        let mut state = provider.0.lock().unwrap();
        state.window.controls = vec![
            ManagedControl {
                id: 10,
                role: "AXButton".into(),
                label: "Toggle Side Panel".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: Some(true),
            },
            ManagedControl {
                id: 20,
                role: "AXButton".into(),
                label: "Side Question".into(),
                enabled: false,
                pressable: false,
                editable: false,
                focused: false,
                expanded: None,
            },
        ];
    }

    let controls = provider.controls();
    let err = controls.open_side_question("antigravity").unwrap_err();
    assert_eq!(err, "blocked:side_question_unavailable");

    let events = provider.events();
    assert!(!events.iter().any(|ev| ev.starts_with("press:")));
}

#[test]
fn side_question_already_open_focuses_composer_directly() {
    let provider = Provider::with_target("com.google.antigravity", "开始听写");
    {
        let mut state = provider.0.lock().unwrap();
        state.window.controls = vec![
            ManagedControl {
                id: 10,
                role: "AXButton".into(),
                label: "Toggle Side Panel".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: Some(true),
            },
            ManagedControl {
                id: 30,
                role: "AXButton".into(),
                label: "New Side Question".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: None,
            },
            ManagedControl {
                id: 31,
                role: "AXTextArea".into(),
                label: "Side Question Input".into(),
                enabled: true,
                pressable: false,
                editable: true,
                focused: false,
                expanded: None,
            },
        ];
    }

    let controls = provider.controls();
    controls.open_side_question("antigravity").unwrap();

    let events = provider.events();
    assert!(!events.iter().any(|ev| ev.starts_with("press:")));
    assert!(events.contains(&"focus:31".to_string()));
}

#[test]
fn side_question_flow_presses_toggle_side_panel_only_once_during_delayed_expansion() {
    let provider = Provider::with_target("com.google.antigravity", "开始听写");
    {
        let mut state = provider.0.lock().unwrap();
        state.effect = PressEffect::NoChange;
        state.window.controls = vec![ManagedControl {
            id: 10,
            role: "AXButton".into(),
            label: "Toggle Side Panel".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: Some(false),
        }];
        state.side_panel_delay_reads = Some(2);
        state.after_press = Some(|fixture, label, _id| {
            if label == "Side Question" {
                fixture.window.controls.push(ManagedControl {
                    id: 30,
                    role: "AXButton".into(),
                    label: "New Side Question".into(),
                    enabled: true,
                    pressable: true,
                    editable: false,
                    focused: false,
                    expanded: None,
                });
                fixture.window.controls.push(ManagedControl {
                    id: 31,
                    role: "AXTextArea".into(),
                    label: "Side Question Input".into(),
                    enabled: true,
                    pressable: false,
                    editable: true,
                    focused: false,
                    expanded: None,
                });
            }
        });
    }

    let controls = provider.controls();
    controls.open_side_question("antigravity").unwrap();

    let events = provider.events();
    assert_eq!(
        events
            .iter()
            .filter(|ev| *ev == "press:Toggle Side Panel")
            .count(),
        1,
        "Toggle Side Panel must be pressed exactly once during delayed expansion"
    );
    assert!(events.contains(&"press:Side Question".to_string()));
    assert!(events.contains(&"focus:31".to_string()));
}

#[test]
fn side_question_already_open_without_new_side_question_button_focuses_directly_with_zero_press() {
    let provider = Provider::with_target("com.google.antigravity", "开始听写");
    {
        let mut state = provider.0.lock().unwrap();
        state.window.controls = vec![
            ManagedControl {
                id: 10,
                role: "AXButton".into(),
                label: "Toggle Side Panel".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: Some(true),
            },
            ManagedControl {
                id: 31,
                role: "AXTextArea".into(),
                label: "Side Question Input".into(),
                enabled: true,
                pressable: false,
                editable: true,
                focused: false,
                expanded: None,
            },
        ];
    }

    let controls = provider.controls();
    controls.open_side_question("antigravity").unwrap();

    let events = provider.events();
    assert!(!events.iter().any(|ev| ev.starts_with("press:")));
    assert!(events.contains(&"focus:31".to_string()));
}

#[test]
fn side_question_collapsed_toggle_side_panel_precedes_unrelated_new_tab_button() {
    let provider = Provider::with_target("com.google.antigravity", "开始听写");
    {
        let mut state = provider.0.lock().unwrap();
        state.effect = PressEffect::NoChange;
        state.window.controls = vec![
            ManagedControl {
                id: 10,
                role: "AXButton".into(),
                label: "Toggle Side Panel".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: Some(false),
            },
            ManagedControl {
                id: 11,
                role: "AXButton".into(),
                label: "New tab".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: None,
            },
        ];
        state.after_press = Some(|fixture, label, _id| {
            if label == "Toggle Side Panel" {
                if let Some(btn) = fixture.window.controls.iter_mut().find(|c| c.id == 10) {
                    btn.expanded = Some(true);
                }
                fixture.window.controls.push(ManagedControl {
                    id: 20,
                    role: "AXButton".into(),
                    label: "Side Question".into(),
                    enabled: true,
                    pressable: true,
                    editable: false,
                    focused: false,
                    expanded: None,
                });
            } else if label == "Side Question" {
                fixture.window.controls.push(ManagedControl {
                    id: 31,
                    role: "AXTextArea".into(),
                    label: "Side Question Input".into(),
                    enabled: true,
                    pressable: false,
                    editable: true,
                    focused: false,
                    expanded: None,
                });
            }
        });
    }

    let controls = provider.controls();
    controls.open_side_question("antigravity").unwrap();

    let events = provider.events();
    assert!(events.contains(&"press:Toggle Side Panel".to_string()));
    assert!(!events.contains(&"press:New tab".to_string()));
    assert!(events.contains(&"press:Side Question".to_string()));
    assert!(events.contains(&"focus:31".to_string()));
}

#[test]
fn side_question_pre_press_read_error_preserves_blocked_prefix() {
    let provider = Provider::with_target("com.google.antigravity", "开始听写");
    {
        let mut state = provider.0.lock().unwrap();
        state.after_capture = Some(|fixture| {
            fixture.read_pid = Some(999);
        });
    }

    let controls = provider.controls();
    let err = controls.open_side_question("antigravity").unwrap_err();
    assert!(
        err.starts_with("blocked:"),
        "pre-press read failure must stay blocked:*, got {err}"
    );
}

#[test]
fn focus_input_with_primary_and_unfocused_side_question_input_focuses_primary() {
    let provider = Provider::with_target("com.google.antigravity", "开始听写");
    {
        let mut state = provider.0.lock().unwrap();
        state.window.controls = vec![
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
                id: 31,
                role: "AXTextArea".into(),
                label: "Side Question Input".into(),
                enabled: true,
                pressable: false,
                editable: true,
                focused: false,
                expanded: None,
            },
        ];
    }

    let controls = provider.controls();
    controls.focus_input("antigravity").unwrap();

    let events = provider.events();
    assert!(events.contains(&"focus:3".to_string()));
    assert!(!events.contains(&"focus:31".to_string()));
}

#[test]
fn subagent_picker_opens_steps_and_confirms_across_antigravity_dsh_and_codex() {
    let ag = Provider::with_target("com.google.antigravity", "开始听写");
    {
        let mut state = ag.0.lock().unwrap();
        state.window.controls.extend([
            ManagedControl {
                id: 50,
                role: "AXButton".into(),
                label: "Stop subagent".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: None,
            },
            ManagedControl {
                id: 51,
                role: "AXPopUpButton".into(),
                label: "1 subagent".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: Some(false),
            },
            ManagedControl {
                id: 52,
                role: "AXPopUpButton".into(),
                label: "2 subagents running".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: Some(false),
            },
        ]);
        state.after_press = Some(|fixture, _label, id| {
            if let Some(ctrl) = fixture.window.controls.iter_mut().find(|c| c.id == id) {
                if ctrl.role == "AXPopUpButton" {
                    ctrl.expanded = Some(true);
                }
            }
        });
    }
    let ag_exec = LiveHostActionExecutor::new(Driver {
        controls: ag.controls(),
        trusted: true,
    });
    let r_open = ag_exec.execute(
        "antigravity",
        "logical-0",
        &ResolvedHostAction::AgentCommand {
            adapter: "host".into(),
            command: "subagent".into(),
        },
    );
    assert_eq!(r_open.outcome, "executed", "{r_open:?}");
    let r_next = ag_exec.execute(
        "antigravity",
        "logical-14",
        &ResolvedHostAction::AgentCommand {
            adapter: "host".into(),
            command: "subagent-next".into(),
        },
    );
    assert_eq!(r_next.outcome, "executed", "{r_next:?}");
    let r_confirm = ag_exec.execute(
        "antigravity",
        "logical-15",
        &ResolvedHostAction::AgentCommand {
            adapter: "host".into(),
            command: "subagent-confirm".into(),
        },
    );
    assert_eq!(r_confirm.outcome, "executed", "{r_confirm:?}");
    let ag_events = ag.events();
    assert!(ag_events.contains(&"press:2 subagents running".to_string()));
    assert!(!ag_events.contains(&"press:Stop subagent".to_string()));
    assert!(ag_events.contains(&"window_key:arrow_down".to_string()));
    assert!(ag_events.contains(&"window_key:return".to_string()));

    let dsh = Provider::dsh();
    {
        let mut state = dsh.0.lock().unwrap();
        state.window.controls.extend([
            ManagedControl {
                id: 60,
                role: "AXButton".into(),
                label: "在侧边栏打开".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: None,
            },
            ManagedControl {
                id: 61,
                role: "AXButton".into(),
                label: "3 个子智能体".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: None,
            },
        ]);
    }
    let dsh_controls = dsh.controls();
    dsh_controls.open_subagent_picker("dsh-desktop").unwrap();
    dsh_controls
        .step_subagent_picker("dsh-desktop", 0x7D)
        .unwrap();
    assert_eq!(
        dsh_controls.confirm_subagent_picker("dsh-desktop").unwrap(),
        ModelPickerConfirmResult::Confirmed
    );
    let dsh_events = dsh.events();
    assert_eq!(
        dsh_events
            .iter()
            .filter(|e| *e == "press:3 个子智能体")
            .count(),
        1,
        "step_subagent_picker must not re-press AXButton while subagent dialog is active: {dsh_events:?}"
    );
    assert!(!dsh_events.contains(&"press:在侧边栏打开".to_string()));
    assert!(dsh_events.contains(&"window_key:arrow_down".to_string()));
    assert!(dsh_events.contains(&"window_key:return".to_string()));

    let codex = Provider::codex();
    let codex_controls = codex.controls();
    let err = codex_controls.open_subagent_picker("codex").unwrap_err();
    assert_eq!(err, "blocked:subagent_picker_unavailable");
    assert!(
        !codex.events().iter().any(|e| e.starts_with("press:")),
        "missing subagent picker must never press unrelated controls"
    );

    let ag_zero = Provider::with_target("com.google.antigravity", "Record voice memo");
    {
        let mut state = ag_zero.0.lock().unwrap();
        state.window.controls.extend([
            ManagedControl {
                id: 70,
                role: "AXCheckBox".into(),
                label: "Toggle Overview Panel".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: None,
            },
            ManagedControl {
                id: 71,
                role: "AXButton".into(),
                label: "Improvement Coding Worker Stop subagent".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: None,
            },
        ]);
    }
    let ag_zero_controls = ag_zero.controls();
    assert_eq!(
        ag_zero_controls
            .open_subagent_picker("antigravity")
            .unwrap_err(),
        "blocked:subagent_picker_unavailable"
    );
    assert!(
        !ag_zero.events().iter().any(|e| e.starts_with("press:")),
        "0-subagent conversation must never toggle Overview panel or press Stop subagent"
    );

    let ag_direct = Provider::with_target("com.google.antigravity", "Record voice memo");
    {
        let mut state = ag_direct.0.lock().unwrap();
        state.window.controls.push(ManagedControl {
            id: 80,
            role: "AXButton".into(),
            label: "Open Subagent: Code Quality Auditor & Release Engineer (DeepCoder)".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: None,
        });
    }
    let ag_direct_exec = LiveHostActionExecutor::new(Driver {
        controls: ag_direct.controls(),
        trusted: true,
    });
    let r_direct = ag_direct_exec.execute(
        "antigravity",
        "logical-0",
        &ResolvedHostAction::AgentCommand {
            adapter: "host".into(),
            command: "subagent".into(),
        },
    );
    assert_eq!(r_direct.outcome, "executed");
    assert_eq!(r_direct.detail, "antigravity/subagent-opened-direct");
    let r_direct_confirm = ag_direct_exec.execute(
        "antigravity",
        "logical-15",
        &ResolvedHostAction::AgentCommand {
            adapter: "host".into(),
            command: "subagent-confirm".into(),
        },
    );
    assert_eq!(r_direct_confirm.outcome, "executed");
    assert_eq!(
        r_direct_confirm.detail,
        "antigravity/subagent-picker-already-closed"
    );
    assert!(
        !ag_direct
            .events()
            .contains(&"window_key:return".to_string()),
        "direct subagent button must never fire stray Return into composer"
    );

    let dsh_model = Provider::dsh();
    {
        let mut state = dsh_model.0.lock().unwrap();
        state.window.controls.push(ManagedControl {
            id: 90,
            role: "AXPopUpButton".into(),
            label: "选择模型，当前 DeepSeek-V4.1-Flash，推理等级 High".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: Some(false),
        });
        state.after_press = Some(|fixture, _label, id| {
            if let Some(ctrl) = fixture.window.controls.iter_mut().find(|c| c.id == id) {
                ctrl.expanded = Some(true);
            }
        });
    }
    dsh_model
        .controls()
        .open_model_picker("dsh-desktop")
        .unwrap();
    assert!(dsh_model
        .events()
        .contains(&"press:选择模型，当前 DeepSeek-V4.1-Flash，推理等级 High".to_string()));
}

#[test]
fn antigravity_side_panel_open_and_close_are_directional_idempotent_and_verified() {
    let ag = Provider::with_target("com.google.antigravity", "Record voice memo");
    {
        let mut state = ag.0.lock().unwrap();
        state.effect = PressEffect::NoChange;
        state.window.controls.push(ManagedControl {
            id: 42,
            role: "AXButton".into(),
            label: "Toggle Side Panel".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: Some(false),
        });
        state.after_press = Some(|fixture, _label, id| {
            if let Some(ctrl) = fixture.window.controls.iter_mut().find(|c| c.id == id) {
                ctrl.expanded = Some(!ctrl.expanded.unwrap_or(false));
            }
        });
    }
    let exec = LiveHostActionExecutor::new(Driver {
        controls: ag.controls(),
        trusted: true,
    });
    let open_cmd = ResolvedHostAction::AgentCommand {
        adapter: "antigravity".into(),
        command: "open-side-panel".into(),
    };
    let close_cmd = ResolvedHostAction::AgentCommand {
        adapter: "antigravity".into(),
        command: "close-side-panel".into(),
    };

    let press_events = || -> Vec<String> {
        ag.events()
            .into_iter()
            .filter(|e| e.starts_with("press:"))
            .collect()
    };

    let r_close_noop = exec.execute("antigravity", "AG18", &close_cmd);
    assert_eq!(r_close_noop.outcome, "executed", "{r_close_noop:?}");
    assert!(press_events().is_empty());

    let r_open = exec.execute("antigravity", "AG16", &open_cmd);
    assert_eq!(r_open.outcome, "executed", "{r_open:?}");
    assert_eq!(press_events(), vec!["press:Toggle Side Panel".to_string()]);

    let r_open_noop = exec.execute("antigravity", "AG16", &open_cmd);
    assert_eq!(r_open_noop.outcome, "executed", "{r_open_noop:?}");
    assert_eq!(press_events().len(), 1);

    let r_close = exec.execute("antigravity", "AG18", &close_cmd);
    assert_eq!(r_close.outcome, "executed", "{r_close:?}");
    assert_eq!(press_events().len(), 2);

    let ag_unknown = Provider::with_target("com.google.antigravity", "Record voice memo");
    ag_unknown
        .0
        .lock()
        .unwrap()
        .window
        .controls
        .push(ManagedControl {
            id: 43,
            role: "AXButton".into(),
            label: "Toggle Side Panel".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: None,
        });
    let unknown_exec = LiveHostActionExecutor::new(Driver {
        controls: ag_unknown.controls(),
        trusted: true,
    });
    let r_unknown = unknown_exec.execute("antigravity", "AG16", &open_cmd);
    assert_eq!(r_unknown.outcome, "blocked", "{r_unknown:?}");
    assert_eq!(r_unknown.detail, "blocked:side_panel_state_unknown");
    assert!(!ag_unknown.events().iter().any(|e| e.starts_with("press:")));

    let ag_stuck = Provider::with_target("com.google.antigravity", "Record voice memo");
    {
        let mut state = ag_stuck.0.lock().unwrap();
        state.effect = PressEffect::NoChange;
        state.window.controls.push(ManagedControl {
            id: 44,
            role: "AXButton".into(),
            label: "Toggle Side Panel".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: Some(false),
        });
    }
    let stuck_exec = LiveHostActionExecutor::new(Driver {
        controls: ManagedAxControls::with_readback_timeout(
            ag_stuck.clone(),
            std::time::Duration::from_millis(30),
        ),
        trusted: true,
    });
    let r_stuck = stuck_exec.execute("antigravity", "AG16", &open_cmd);
    assert_eq!(r_stuck.outcome, "failed", "{r_stuck:?}");
    assert_eq!(r_stuck.detail, "unconfirmed:side_panel_open");

    let ag_replaced = Provider::with_target("com.google.antigravity", "Record voice memo");
    {
        let mut state = ag_replaced.0.lock().unwrap();
        state.effect = PressEffect::NoChange;
        state.window.controls.push(ManagedControl {
            id: 45,
            role: "AXButton".into(),
            label: "Toggle Side Panel".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: Some(false),
        });
        state.after_press = Some(|fixture, _label, id| {
            fixture.window.controls.retain(|c| c.id != id);
            fixture.window.controls.push(ManagedControl {
                id: 145,
                role: "AXButton".into(),
                label: "Toggle Side Panel".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: Some(true),
            });
        });
    }
    let replaced_exec = LiveHostActionExecutor::new(Driver {
        controls: ag_replaced.controls(),
        trusted: true,
    });
    let r_replaced = replaced_exec.execute("antigravity", "AG16", &open_cmd);
    assert_eq!(r_replaced.outcome, "executed", "{r_replaced:?}");
}

#[test]
fn antigravity_browser_opens_closed_side_panel_then_clicks_browser_and_confirms_address_bar() {
    let ag = Provider::with_target("com.google.antigravity", "Record voice memo");
    {
        let mut state = ag.0.lock().unwrap();
        state.effect = PressEffect::NoChange;
        state.window.controls = vec![ManagedControl {
            id: 10,
            role: "AXButton".into(),
            label: "Toggle Side Panel".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: Some(false),
        }];
        state.after_press = Some(|fixture, label, _id| {
            if label == "Toggle Side Panel" {
                if let Some(btn) = fixture.window.controls.iter_mut().find(|c| c.id == 10) {
                    btn.expanded = Some(true);
                }
                fixture.window.controls.push(ManagedControl {
                    id: 20,
                    role: "AXTextField".into(),
                    label: "Search for content to open".into(),
                    enabled: true,
                    pressable: false,
                    editable: true,
                    focused: true,
                    expanded: None,
                });
                fixture.window.controls.push(ManagedControl {
                    id: 21,
                    role: "AXButton".into(),
                    label: "Browser".into(),
                    enabled: true,
                    pressable: true,
                    editable: false,
                    focused: false,
                    expanded: None,
                });
            } else if label == "Browser" {
                fixture.window.controls.retain(|c| c.id != 20 && c.id != 21);
                fixture.window.controls.push(ManagedControl {
                    id: 30,
                    role: "AXComboBox".into(),
                    label: "Address bar".into(),
                    enabled: true,
                    pressable: false,
                    editable: true,
                    focused: false,
                    expanded: None,
                });
                fixture.window.controls.push(ManagedControl {
                    id: 31,
                    role: "AXButton".into(),
                    label: "Open in external browser".into(),
                    enabled: true,
                    pressable: true,
                    editable: false,
                    focused: false,
                    expanded: None,
                });
            }
        });
    }

    let exec = LiveHostActionExecutor::new(Driver {
        controls: ag.controls(),
        trusted: true,
    });
    let cmd = ResolvedHostAction::AgentCommand {
        adapter: "antigravity".into(),
        command: "browser".into(),
    };
    let receipt = exec.execute("antigravity", "AG08", &cmd);
    assert_eq!(receipt.outcome, "executed", "{receipt:?}");
    assert_eq!(receipt.detail, "antigravity/browser");

    let events = ag.events();
    assert!(events.contains(&"activate_window".to_string()));
    assert_eq!(
        events
            .iter()
            .filter(|e| e.starts_with("press:"))
            .cloned()
            .collect::<Vec<_>>(),
        vec![
            "press:Toggle Side Panel".to_string(),
            "press:Browser".to_string(),
        ]
    );
}

#[test]
fn antigravity_browser_from_open_new_tab_clicks_browser_directly_without_toggling_panel() {
    let ag = Provider::with_target("com.google.antigravity", "Record voice memo");
    {
        let mut state = ag.0.lock().unwrap();
        state.effect = PressEffect::NoChange;
        state.window.controls = vec![
            ManagedControl {
                id: 10,
                role: "AXButton".into(),
                label: "Toggle Side Panel".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: Some(true),
            },
            ManagedControl {
                id: 20,
                role: "AXTextField".into(),
                label: "Search for content to open".into(),
                enabled: true,
                pressable: false,
                editable: true,
                focused: true,
                expanded: None,
            },
            ManagedControl {
                id: 21,
                role: "AXButton".into(),
                label: "Browser".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: None,
            },
        ];
        state.after_press = Some(|fixture, label, _id| {
            if label == "Browser" {
                fixture.window.controls.retain(|c| c.id != 20 && c.id != 21);
                fixture.window.controls.push(ManagedControl {
                    id: 30,
                    role: "AXComboBox".into(),
                    label: "Address bar".into(),
                    enabled: true,
                    pressable: false,
                    editable: true,
                    focused: false,
                    expanded: None,
                });
            }
        });
    }

    ag.controls().open_browser("antigravity").unwrap();
    let presses: Vec<String> = ag
        .events()
        .into_iter()
        .filter(|e| e.starts_with("press:"))
        .collect();
    assert_eq!(presses, vec!["press:Browser".to_string()]);
}

#[test]
fn antigravity_browser_clicks_new_tab_when_other_tab_active_and_confirms_toolbar_marker() {
    let ag = Provider::with_target("com.google.antigravity", "Record voice memo");
    {
        let mut state = ag.0.lock().unwrap();
        state.effect = PressEffect::NoChange;
        state.window.controls = vec![
            ManagedControl {
                id: 10,
                role: "AXButton".into(),
                label: "Toggle Side Panel".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: Some(true),
            },
            ManagedControl {
                id: 11,
                role: "AXButton".into(),
                label: "New tab".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: None,
            },
        ];
        state.after_press = Some(|fixture, label, _id| {
            if label == "New tab" {
                fixture.window.controls.push(ManagedControl {
                    id: 20,
                    role: "AXTextField".into(),
                    label: "Search for content to open".into(),
                    enabled: true,
                    pressable: false,
                    editable: true,
                    focused: true,
                    expanded: None,
                });
                fixture.window.controls.push(ManagedControl {
                    id: 21,
                    role: "AXButton".into(),
                    label: "Browser".into(),
                    enabled: true,
                    pressable: true,
                    editable: false,
                    focused: false,
                    expanded: None,
                });
            } else if label == "Browser" {
                fixture.window.controls.retain(|c| c.id != 20 && c.id != 21);
                fixture.window.controls.push(ManagedControl {
                    id: 31,
                    role: "AXButton".into(),
                    label: "Open in external browser".into(),
                    enabled: true,
                    pressable: true,
                    editable: false,
                    focused: false,
                    expanded: None,
                });
            }
        });
    }

    ag.controls().open_browser("antigravity").unwrap();
    let presses: Vec<String> = ag
        .events()
        .into_iter()
        .filter(|e| e.starts_with("press:"))
        .collect();
    assert_eq!(
        presses,
        vec!["press:New tab".to_string(), "press:Browser".to_string()]
    );
}

#[test]
fn antigravity_browser_already_open_is_idempotent_and_collapsed_panel_reopens_without_new_tab() {
    let open = Provider::with_target("com.google.antigravity", "Record voice memo");
    open.0.lock().unwrap().window.controls = vec![
        ManagedControl {
            id: 10,
            role: "AXButton".into(),
            label: "Toggle Side Panel".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: Some(true),
        },
        ManagedControl {
            id: 30,
            role: "AXComboBox".into(),
            label: "Address bar".into(),
            enabled: true,
            pressable: false,
            editable: true,
            focused: false,
            expanded: None,
        },
    ];
    open.controls().open_browser("antigravity").unwrap();
    assert!(!open.events().iter().any(|e| e.starts_with("press:")));

    let collapsed = Provider::with_target("com.google.antigravity", "Record voice memo");
    {
        let mut state = collapsed.0.lock().unwrap();
        state.effect = PressEffect::NoChange;
        state.window.controls = vec![
            ManagedControl {
                id: 10,
                role: "AXButton".into(),
                label: "Toggle Side Panel".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: Some(false),
            },
            ManagedControl {
                id: 30,
                role: "AXComboBox".into(),
                label: "Address bar".into(),
                enabled: true,
                pressable: false,
                editable: true,
                focused: false,
                expanded: None,
            },
        ];
        state.after_press = Some(|fixture, label, _id| {
            if label == "Toggle Side Panel" {
                if let Some(btn) = fixture.window.controls.iter_mut().find(|c| c.id == 10) {
                    btn.expanded = Some(true);
                }
            }
        });
    }
    collapsed.controls().open_browser("antigravity").unwrap();
    let presses: Vec<String> = collapsed
        .events()
        .into_iter()
        .filter(|e| e.starts_with("press:"))
        .collect();
    assert_eq!(presses, vec!["press:Toggle Side Panel".to_string()]);
}

#[test]
fn antigravity_browser_clears_new_tab_search_and_does_not_click_new_tab_during_mount_race() {
    let with_query = Provider::with_target("com.google.antigravity", "Record voice memo");
    {
        let mut state = with_query.0.lock().unwrap();
        state.effect = PressEffect::NoChange;
        state.window.controls = vec![
            ManagedControl {
                id: 10,
                role: "AXButton".into(),
                label: "Toggle Side Panel".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: Some(true),
            },
            ManagedControl {
                id: 11,
                role: "AXButton".into(),
                label: "New tab".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: None,
            },
            ManagedControl {
                id: 20,
                role: "AXTextField".into(),
                label: "Search for content to open".into(),
                enabled: true,
                pressable: false,
                editable: true,
                focused: true,
                expanded: None,
            },
            ManagedControl {
                id: 22,
                role: "AXButton".into(),
                label: "Clear search".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: None,
            },
        ];
        state.after_press = Some(|fixture, label, _id| {
            if label == "Clear search" {
                fixture.window.controls.retain(|c| c.id != 22);
                fixture.window.controls.push(ManagedControl {
                    id: 21,
                    role: "AXButton".into(),
                    label: "Browser".into(),
                    enabled: true,
                    pressable: true,
                    editable: false,
                    focused: false,
                    expanded: None,
                });
            } else if label == "Browser" {
                fixture.window.controls.retain(|c| c.id != 20 && c.id != 21);
                fixture.window.controls.push(ManagedControl {
                    id: 30,
                    role: "AXComboBox".into(),
                    label: "Address bar".into(),
                    enabled: true,
                    pressable: false,
                    editable: true,
                    focused: false,
                    expanded: None,
                });
            }
        });
    }
    with_query.controls().open_browser("antigravity").unwrap();
    let presses: Vec<String> = with_query
        .events()
        .into_iter()
        .filter(|e| e.starts_with("press:"))
        .collect();
    assert_eq!(
        presses,
        vec![
            "press:Clear search".to_string(),
            "press:Browser".to_string()
        ]
    );

    let race = Provider::with_target("com.google.antigravity", "Record voice memo");
    {
        let mut state = race.0.lock().unwrap();
        state.effect = PressEffect::NoChange;
        state.window.controls = vec![
            ManagedControl {
                id: 10,
                role: "AXButton".into(),
                label: "Toggle Side Panel".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: Some(true),
            },
            ManagedControl {
                id: 20,
                role: "AXTextField".into(),
                label: "Search for content to open".into(),
                enabled: true,
                pressable: false,
                editable: true,
                focused: true,
                expanded: None,
            },
            ManagedControl {
                id: 21,
                role: "AXButton".into(),
                label: "Browser".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: None,
            },
        ];
        state.after_press = Some(|fixture, label, _id| {
            if label == "Browser" {
                fixture.window.controls.retain(|c| c.id != 20 && c.id != 21);
                fixture.window.controls.push(ManagedControl {
                    id: 11,
                    role: "AXButton".into(),
                    label: "New tab".into(),
                    enabled: true,
                    pressable: true,
                    editable: false,
                    focused: false,
                    expanded: None,
                });
            }
        });
    }
    let race_bg = race.clone();
    std::thread::spawn(move || loop {
        let pressed = race_bg.events().iter().any(|e| e == "press:Browser");
        if pressed {
            std::thread::sleep(Duration::from_millis(60));
            race_bg
                .0
                .lock()
                .unwrap()
                .window
                .controls
                .push(ManagedControl {
                    id: 30,
                    role: "AXComboBox".into(),
                    label: "Address bar".into(),
                    enabled: true,
                    pressable: false,
                    editable: true,
                    focused: false,
                    expanded: None,
                });
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    });
    race.controls().open_browser("antigravity").unwrap();
    let race_presses: Vec<String> = race
        .events()
        .into_iter()
        .filter(|e| e.starts_with("press:"))
        .collect();
    assert_eq!(
        race_presses,
        vec!["press:Browser".to_string()],
        "must never press New tab while waiting for Browser pane to mount"
    );
}

#[test]
fn antigravity_browser_preview_fallback_and_fail_closed_boundaries() {
    let preview_ok = Provider::with_target("com.google.antigravity", "Record voice memo");
    {
        let mut state = preview_ok.0.lock().unwrap();
        state.effect = PressEffect::NoChange;
        state.window.controls = vec![
            ManagedControl {
                id: 10,
                role: "AXButton".into(),
                label: "Toggle Side Panel".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: Some(true),
            },
            ManagedControl {
                id: 20,
                role: "AXTextField".into(),
                label: "Search for content to open".into(),
                enabled: true,
                pressable: false,
                editable: true,
                focused: true,
                expanded: None,
            },
            ManagedControl {
                id: 25,
                role: "AXButton".into(),
                label: "Preview".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: None,
            },
        ];
        state.after_press = Some(|fixture, label, _id| {
            if label == "Preview" {
                fixture.window.controls.push(ManagedControl {
                    id: 30,
                    role: "AXComboBox".into(),
                    label: "Address bar".into(),
                    enabled: true,
                    pressable: false,
                    editable: true,
                    focused: false,
                    expanded: None,
                });
            }
        });
    }
    preview_ok.controls().open_browser("antigravity").unwrap();
    assert_eq!(
        preview_ok
            .events()
            .into_iter()
            .filter(|e| e.starts_with("press:"))
            .collect::<Vec<_>>(),
        vec!["press:Preview".to_string()]
    );

    let stray_preview = Provider::with_target("com.google.antigravity", "Record voice memo");
    stray_preview.0.lock().unwrap().window.controls = vec![ManagedControl {
        id: 25,
        role: "AXButton".into(),
        label: "Preview".into(),
        enabled: true,
        pressable: true,
        editable: false,
        focused: false,
        expanded: None,
    }];
    let err =
        ManagedAxControls::with_readback_timeout(stray_preview.clone(), Duration::from_millis(20))
            .open_browser("antigravity")
            .unwrap_err();
    assert_eq!(err, "blocked:browser_control_not_found");
    assert!(!stray_preview
        .events()
        .iter()
        .any(|e| e.starts_with("press:")));

    let disabled = Provider::with_target("com.google.antigravity", "Record voice memo");
    disabled.0.lock().unwrap().window.controls = vec![
        ManagedControl {
            id: 10,
            role: "AXButton".into(),
            label: "Toggle Side Panel".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: Some(true),
        },
        ManagedControl {
            id: 21,
            role: "AXButton".into(),
            label: "Browser".into(),
            enabled: false,
            pressable: false,
            editable: false,
            focused: false,
            expanded: None,
        },
    ];
    assert_eq!(
        disabled.controls().open_browser("antigravity").unwrap_err(),
        "blocked:browser_unavailable"
    );
    assert!(!disabled.events().iter().any(|e| e.starts_with("press:")));

    let stuck = Provider::with_target("com.google.antigravity", "Record voice memo");
    {
        let mut state = stuck.0.lock().unwrap();
        state.effect = PressEffect::NoChange;
        state.window.controls = vec![
            ManagedControl {
                id: 10,
                role: "AXButton".into(),
                label: "Toggle Side Panel".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: Some(true),
            },
            ManagedControl {
                id: 21,
                role: "AXButton".into(),
                label: "Browser".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: None,
            },
        ];
    }
    let stuck_exec = LiveHostActionExecutor::new(Driver {
        controls: ManagedAxControls::with_readback_timeout(
            stuck.clone(),
            Duration::from_millis(20),
        ),
        trusted: true,
    });
    let cmd = ResolvedHostAction::AgentCommand {
        adapter: "antigravity".into(),
        command: "browser".into(),
    };
    let r_stuck = stuck_exec.execute("antigravity", "AG08", &cmd);
    assert_eq!(r_stuck.outcome, "failed", "{r_stuck:?}");
    assert_eq!(r_stuck.detail, "unconfirmed:browser_open");

    assert_eq!(
        Provider::codex()
            .controls()
            .open_browser("codex")
            .unwrap_err(),
        "blocked:unsupported_browser_adapter"
    );

    let untrusted = Provider::with_target("com.google.antigravity", "Record voice memo");
    let untrusted_exec = LiveHostActionExecutor::new(Driver {
        controls: untrusted.controls(),
        trusted: false,
    });
    let r_untrusted = untrusted_exec.execute("antigravity", "AG08", &cmd);
    assert_eq!(r_untrusted.outcome, "blocked", "{r_untrusted:?}");
    assert_eq!(r_untrusted.detail, "accessibility_untrusted");
    assert!(untrusted.events().is_empty());
}

#[test]
fn antigravity_browser_three_step_collapsed_other_tab_to_new_tab_to_browser_and_target_validation()
{
    let ag = Provider::with_target("com.google.antigravity", "Record voice memo");
    {
        let mut state = ag.0.lock().unwrap();
        state.effect = PressEffect::NoChange;
        state.window.controls = vec![ManagedControl {
            id: 10,
            role: "AXButton".into(),
            label: "Toggle Side Panel".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: Some(false),
        }];
        state.after_press = Some(|fixture, label, _id| {
            if label == "Toggle Side Panel" {
                if let Some(btn) = fixture.window.controls.iter_mut().find(|c| c.id == 10) {
                    btn.expanded = Some(true);
                }
                fixture.window.controls.push(ManagedControl {
                    id: 11,
                    role: "AXButton".into(),
                    label: "New tab".into(),
                    enabled: true,
                    pressable: true,
                    editable: false,
                    focused: false,
                    expanded: None,
                });
            } else if label == "New tab" {
                fixture.window.controls.push(ManagedControl {
                    id: 20,
                    role: "AXTextField".into(),
                    label: "Search for content to open".into(),
                    enabled: true,
                    pressable: false,
                    editable: true,
                    focused: true,
                    expanded: None,
                });
                fixture.window.controls.push(ManagedControl {
                    id: 21,
                    role: "AXButton".into(),
                    label: "Browser".into(),
                    enabled: true,
                    pressable: true,
                    editable: false,
                    focused: false,
                    expanded: None,
                });
            } else if label == "Browser" {
                fixture.window.controls.retain(|c| c.id != 20 && c.id != 21);
                fixture.window.controls.push(ManagedControl {
                    id: 30,
                    role: "AXComboBox".into(),
                    label: "Address bar".into(),
                    enabled: true,
                    pressable: false,
                    editable: true,
                    focused: false,
                    expanded: None,
                });
                fixture.window.controls.push(ManagedControl {
                    id: 31,
                    role: "AXButton".into(),
                    label: "Open in external browser".into(),
                    enabled: true,
                    pressable: true,
                    editable: false,
                    focused: false,
                    expanded: None,
                });
            }
        });
    }

    ag.controls().open_browser("antigravity").unwrap();
    let presses: Vec<String> = ag
        .events()
        .into_iter()
        .filter(|e| e.starts_with("press:"))
        .collect();
    assert_eq!(
        presses,
        vec![
            "press:Toggle Side Panel".to_string(),
            "press:New tab".to_string(),
            "press:Browser".to_string(),
        ]
    );

    let wrong_bundle = Provider::with_target("com.other.app", "Record voice memo");
    let err = wrong_bundle
        .controls()
        .open_browser("antigravity")
        .unwrap_err();
    assert!(
        err.starts_with("blocked:"),
        "wrong bundle must be blocked, got {err}"
    );
    assert!(
        !wrong_bundle
            .events()
            .iter()
            .any(|e| e == "activate_window" || e.starts_with("press:")),
        "must not activate or press mismatched target window: {:?}",
        wrong_bundle.events()
    );

    let post_fail = Provider::with_target("com.google.antigravity", "Record voice memo");
    {
        let mut state = post_fail.0.lock().unwrap();
        state.effect = PressEffect::NoChange;
        state.window.controls = vec![ManagedControl {
            id: 10,
            role: "AXButton".into(),
            label: "Toggle Side Panel".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: Some(false),
        }];
        state.after_press = Some(|fixture, label, _id| {
            if label == "Toggle Side Panel" {
                if let Some(btn) = fixture.window.controls.iter_mut().find(|c| c.id == 10) {
                    btn.expanded = Some(true);
                }
                fixture.window.controls.push(ManagedControl {
                    id: 21,
                    role: "AXButton".into(),
                    label: "Browser".into(),
                    enabled: true,
                    pressable: true,
                    editable: false,
                    focused: false,
                    expanded: None,
                });
                fixture.effect = PressEffect::LoseTarget;
            }
        });
    }
    let err =
        ManagedAxControls::with_readback_timeout(post_fail.clone(), Duration::from_millis(20))
            .open_browser("antigravity")
            .unwrap_err();
    assert!(
        err.starts_with("unconfirmed:"),
        "failure after mutating Side Panel must report unconfirmed, got {err}"
    );
}

#[test]
fn antigravity_browser_post_mutation_unavailable_controls_report_unconfirmed_not_blocked() {
    let make_btn =
        |id: u64, label: &str, enabled: bool, pressable: bool, expanded: Option<bool>| {
            ManagedControl {
                id,
                role: "AXButton".into(),
                label: label.into(),
                enabled,
                pressable,
                editable: false,
                focused: false,
                expanded,
            }
        };

    let disabled_browser = Provider::with_target("com.google.antigravity", "Record voice memo");
    {
        let mut state = disabled_browser.0.lock().unwrap();
        state.effect = PressEffect::NoChange;
        state.window.controls = vec![make_btn(10, "Toggle Side Panel", true, true, Some(false))];
        state.after_press = Some(|fixture, label, _id| {
            if label == "Toggle Side Panel" {
                fixture.window.controls[0].expanded = Some(true);
                fixture.window.controls.push(ManagedControl {
                    id: 21,
                    role: "AXButton".into(),
                    label: "Browser".into(),
                    enabled: false,
                    pressable: true,
                    editable: false,
                    focused: false,
                    expanded: None,
                });
            }
        });
    }
    let exec = LiveHostActionExecutor::new(Driver {
        controls: disabled_browser.controls(),
        trusted: true,
    });
    let cmd = ResolvedHostAction::AgentCommand {
        adapter: "antigravity".into(),
        command: "browser".into(),
    };
    let receipt = exec.execute("antigravity", "AG08", &cmd);
    assert_eq!(receipt.outcome, "failed", "{receipt:?}");
    assert_eq!(receipt.detail, "unconfirmed:blocked:browser_unavailable");

    let disabled_preview = Provider::with_target("com.google.antigravity", "Record voice memo");
    {
        let mut state = disabled_preview.0.lock().unwrap();
        state.effect = PressEffect::NoChange;
        state.window.controls = vec![make_btn(10, "Toggle Side Panel", true, true, Some(false))];
        state.after_press = Some(|fixture, label, _id| {
            if label == "Toggle Side Panel" {
                fixture.window.controls[0].expanded = Some(true);
                fixture.window.controls.push(ManagedControl {
                    id: 20,
                    role: "AXTextField".into(),
                    label: "Search for content to open".into(),
                    enabled: true,
                    pressable: false,
                    editable: true,
                    focused: true,
                    expanded: None,
                });
                fixture.window.controls.push(ManagedControl {
                    id: 25,
                    role: "AXButton".into(),
                    label: "Preview".into(),
                    enabled: true,
                    pressable: false,
                    editable: false,
                    focused: false,
                    expanded: None,
                });
            }
        });
    }
    assert_eq!(
        disabled_preview
            .controls()
            .open_browser("antigravity")
            .unwrap_err(),
        "unconfirmed:blocked:browser_unavailable"
    );

    let disabled_new_tab = Provider::with_target("com.google.antigravity", "Record voice memo");
    {
        let mut state = disabled_new_tab.0.lock().unwrap();
        state.effect = PressEffect::NoChange;
        state.window.controls = vec![make_btn(10, "Toggle Side Panel", true, true, Some(false))];
        state.after_press = Some(|fixture, label, _id| {
            if label == "Toggle Side Panel" {
                fixture.window.controls[0].expanded = Some(true);
                fixture.window.controls.push(ManagedControl {
                    id: 11,
                    role: "AXButton".into(),
                    label: "New tab".into(),
                    enabled: false,
                    pressable: false,
                    editable: false,
                    focused: false,
                    expanded: None,
                });
            }
        });
    }
    assert_eq!(
        disabled_new_tab
            .controls()
            .open_browser("antigravity")
            .unwrap_err(),
        "unconfirmed:blocked:browser_unavailable"
    );

    let disabled_collapsed_toggle =
        Provider::with_target("com.google.antigravity", "Record voice memo");
    {
        let mut state = disabled_collapsed_toggle.0.lock().unwrap();
        state.effect = PressEffect::NoChange;
        state.window.controls = vec![
            make_btn(10, "Toggle Side Panel", true, true, Some(true)),
            make_btn(11, "New tab", true, true, None),
        ];
        state.after_press = Some(|fixture, label, _id| {
            if label == "New tab" {
                fixture.window.controls.retain(|c| c.id != 11);
                fixture.window.controls[0].expanded = Some(false);
                fixture.window.controls[0].enabled = false;
            }
        });
    }
    assert_eq!(
        disabled_collapsed_toggle
            .controls()
            .open_browser("antigravity")
            .unwrap_err(),
        "unconfirmed:blocked:browser_unavailable"
    );

    let stuck_collapsed_toggle =
        Provider::with_target("com.google.antigravity", "Record voice memo");
    {
        let mut state = stuck_collapsed_toggle.0.lock().unwrap();
        state.effect = PressEffect::NoChange;
        state.window.controls = vec![make_btn(10, "Toggle Side Panel", true, true, Some(false))];
        state.after_press = Some(|fixture, label, _id| {
            if label == "Toggle Side Panel" {
                fixture.window.controls[0].enabled = false;
            }
        });
    }
    let stuck_exec = LiveHostActionExecutor::new(Driver {
        controls: ManagedAxControls::with_readback_timeout(
            stuck_collapsed_toggle.clone(),
            Duration::from_millis(30),
        ),
        trusted: true,
    });
    let stuck_receipt = stuck_exec.execute("antigravity", "AG08", &cmd);
    assert_eq!(stuck_receipt.outcome, "failed", "{stuck_receipt:?}");
    assert_eq!(stuck_receipt.detail, "unconfirmed:browser_open");

    let disabled_fallback_toggle =
        Provider::with_target("com.google.antigravity", "Record voice memo");
    {
        let mut state = disabled_fallback_toggle.0.lock().unwrap();
        state.effect = PressEffect::NoChange;
        state.window.controls = vec![
            make_btn(10, "Toggle Side Panel", true, true, None),
            make_btn(11, "New tab", true, true, None),
        ];
        state.after_press = Some(|fixture, label, _id| {
            if label == "New tab" {
                fixture.window.controls.retain(|c| c.id != 11);
                fixture.window.controls[0].enabled = false;
            }
        });
    }
    assert_eq!(
        disabled_fallback_toggle
            .controls()
            .open_browser("antigravity")
            .unwrap_err(),
        "unconfirmed:blocked:browser_unavailable"
    );

    for pre_controls in [
        vec![make_btn(10, "Toggle Side Panel", false, true, Some(false))],
        vec![make_btn(10, "Toggle Side Panel", true, false, None)],
        vec![
            make_btn(10, "Toggle Side Panel", true, true, Some(true)),
            make_btn(11, "New tab", false, false, None),
        ],
    ] {
        let pre = Provider::with_target("com.google.antigravity", "Record voice memo");
        pre.0.lock().unwrap().window.controls = pre_controls;
        assert_eq!(
            pre.controls().open_browser("antigravity").unwrap_err(),
            "blocked:browser_unavailable"
        );
        assert!(
            !pre.events().iter().any(|e| e.starts_with("press:")),
            "pre-mutation unavailable control must never press"
        );
    }
}

#[test]
fn antigravity_browser_pre_mutation_fails_fast_while_preserving_post_mutation_deadline() {
    let home_page = Provider::with_target("com.google.antigravity", "Record voice memo");
    let started = std::time::Instant::now();
    let err = ManagedAxControls::new(home_page.clone())
        .open_browser("antigravity")
        .unwrap_err();
    let elapsed = started.elapsed();
    assert_eq!(err, "blocked:browser_control_not_found");
    assert!(
        !home_page.events().iter().any(|e| e.starts_with("press:")),
        "Home/New Conversation page must perform zero presses"
    );
    assert!(
        elapsed < Duration::from_millis(1200),
        "pre-mutation discovery should fail fast (~250ms), not wait 3500ms; elapsed={elapsed:?}"
    );

    let delayed_mount = Provider::with_target("com.google.antigravity", "Record voice memo");
    {
        let mut state = delayed_mount.0.lock().unwrap();
        state.effect = PressEffect::NoChange;
        state.window.controls = vec![
            ManagedControl {
                id: 10,
                role: "AXButton".into(),
                label: "Toggle Side Panel".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: Some(true),
            },
            ManagedControl {
                id: 21,
                role: "AXButton".into(),
                label: "Browser".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: None,
            },
        ];
        state.after_press = Some(|fixture, label, _id| {
            if label == "Browser" {
                fixture.window.controls.retain(|c| c.id != 21);
            }
        });
    }
    let bg = delayed_mount.clone();
    let mount_thread = std::thread::spawn(move || loop {
        if bg.events().iter().any(|e| e == "press:Browser") {
            std::thread::sleep(Duration::from_millis(400));
            bg.0.lock().unwrap().window.controls.push(ManagedControl {
                id: 30,
                role: "AXComboBox".into(),
                label: "Address bar".into(),
                enabled: true,
                pressable: false,
                editable: true,
                focused: false,
                expanded: None,
            });
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    });
    ManagedAxControls::new(delayed_mount.clone())
        .open_browser("antigravity")
        .expect("post-mutation readback must exceed the 250ms pre-mutation discovery window");
    mount_thread.join().unwrap();
    let presses: Vec<String> = delayed_mount
        .events()
        .into_iter()
        .filter(|e| e.starts_with("press:"))
        .collect();
    assert_eq!(presses, vec!["press:Browser".to_string()]);
}

#[test]
fn antigravity_browser_waits_through_transient_mid_animation_disable_on_already_pressed_controls() {
    let animating_panel = Provider::with_target("com.google.antigravity", "Record voice memo");
    {
        let mut state = animating_panel.0.lock().unwrap();
        state.effect = PressEffect::NoChange;
        state.window.controls = vec![ManagedControl {
            id: 10,
            role: "AXButton".into(),
            label: "Toggle Side Panel".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: Some(false),
        }];
        state.after_press = Some(|fixture, label, _id| {
            if label == "Toggle Side Panel" {
                fixture.window.controls[0].enabled = false;
            } else if label == "Browser" {
                fixture.window.controls.retain(|c| c.id != 21);
                fixture.window.controls.push(ManagedControl {
                    id: 30,
                    role: "AXComboBox".into(),
                    label: "Address bar".into(),
                    enabled: true,
                    pressable: false,
                    editable: true,
                    focused: false,
                    expanded: None,
                });
            }
        });
    }
    let bg_panel = animating_panel.clone();
    let panel_thread = std::thread::spawn(move || loop {
        if bg_panel
            .events()
            .iter()
            .any(|e| e == "press:Toggle Side Panel")
        {
            std::thread::sleep(Duration::from_millis(60));
            let mut st = bg_panel.0.lock().unwrap();
            st.window.controls[0].enabled = true;
            st.window.controls[0].expanded = Some(true);
            st.window.controls.push(ManagedControl {
                id: 21,
                role: "AXButton".into(),
                label: "Browser".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: None,
            });
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    });
    animating_panel
        .controls()
        .open_browser("antigravity")
        .expect("already-pressed Toggle Side Panel disabling mid-animation must recover");
    panel_thread.join().unwrap();
    let panel_presses: Vec<String> = animating_panel
        .events()
        .into_iter()
        .filter(|e| e.starts_with("press:"))
        .collect();
    assert_eq!(
        panel_presses,
        vec![
            "press:Toggle Side Panel".to_string(),
            "press:Browser".to_string(),
        ]
    );

    let animating_new_tab = Provider::with_target("com.google.antigravity", "Record voice memo");
    {
        let mut state = animating_new_tab.0.lock().unwrap();
        state.effect = PressEffect::NoChange;
        state.window.controls = vec![
            ManagedControl {
                id: 10,
                role: "AXButton".into(),
                label: "Toggle Side Panel".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: Some(true),
            },
            ManagedControl {
                id: 11,
                role: "AXButton".into(),
                label: "New tab".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: None,
            },
        ];
        state.after_press = Some(|fixture, label, _id| {
            if label == "New tab" {
                if let Some(btn) = fixture.window.controls.iter_mut().find(|c| c.id == 11) {
                    btn.enabled = false;
                }
            } else if label == "Browser" {
                fixture.window.controls.retain(|c| c.id != 21);
                fixture.window.controls.push(ManagedControl {
                    id: 31,
                    role: "AXButton".into(),
                    label: "Open in external browser".into(),
                    enabled: true,
                    pressable: true,
                    editable: false,
                    focused: false,
                    expanded: None,
                });
            }
        });
    }
    let bg_new_tab = animating_new_tab.clone();
    let new_tab_thread = std::thread::spawn(move || loop {
        if bg_new_tab.events().iter().any(|e| e == "press:New tab") {
            std::thread::sleep(Duration::from_millis(60));
            let mut st = bg_new_tab.0.lock().unwrap();
            st.window.controls.push(ManagedControl {
                id: 21,
                role: "AXButton".into(),
                label: "Browser".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: None,
            });
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    });
    animating_new_tab
        .controls()
        .open_browser("antigravity")
        .expect("already-pressed New tab disabling while mounting must recover");
    new_tab_thread.join().unwrap();
    let new_tab_presses: Vec<String> = animating_new_tab
        .events()
        .into_iter()
        .filter(|e| e.starts_with("press:"))
        .collect();
    assert_eq!(
        new_tab_presses,
        vec!["press:New tab".to_string(), "press:Browser".to_string()]
    );
}

#[test]
fn mouse_stopped_dictation_with_changed_window_title_recovers_on_toggle_submit_and_cleanup() {
    let toggle_provider = Provider::antigravity();
    toggle_provider.0.lock().unwrap().window.identity.title = "Conversation A".into();
    let toggle_controls = toggle_provider.controls();
    assert_eq!(
        toggle_controls.toggle_dictation("antigravity"),
        Ok(ObservedDictationState::Recording)
    );
    {
        let mut state = toggle_provider.0.lock().unwrap();
        state.window.controls[0].label = "Record voice memo".into();
        state.window.identity.title = "Conversation B".into();
    }
    assert_eq!(
        toggle_controls.toggle_dictation("antigravity"),
        Ok(ObservedDictationState::Recording),
        "foreground toggle must recover after mouse-stopped dictation and conversation title drift"
    );
    assert_eq!(
        toggle_controls.toggle_dictation("antigravity"),
        Ok(ObservedDictationState::Idle),
        "subsequent toggle must stop dictation cleanly in the updated conversation"
    );
    assert!(
        toggle_provider
            .0
            .lock()
            .unwrap()
            .retained_targets
            .is_empty(),
        "stopping after title-drift recovery must release all retained targets"
    );
    assert_eq!(
        toggle_provider
            .events()
            .into_iter()
            .filter(|e| e.starts_with("press:"))
            .collect::<Vec<_>>(),
        vec![
            "press:Record voice memo".to_string(),
            "press:Record voice memo".to_string(),
            "press:Stop Recording".to_string(),
        ]
    );

    let submit_provider = Provider::antigravity();
    submit_provider.0.lock().unwrap().window.identity.title = "Conversation A".into();
    let submit_controls = submit_provider.controls();
    assert_eq!(
        submit_controls.toggle_dictation("antigravity"),
        Ok(ObservedDictationState::Recording)
    );
    {
        let mut state = submit_provider.0.lock().unwrap();
        state.window.controls[0].label = "Record voice memo".into();
        state.window.identity.title = "Conversation B".into();
    }
    assert_eq!(
        submit_controls.submit("antigravity", None),
        Ok(()),
        "foreground submit must recover after mouse-stopped dictation and conversation title drift"
    );
    assert!(
        submit_provider
            .0
            .lock()
            .unwrap()
            .retained_targets
            .is_empty(),
        "submit recovery must release retained dictation targets"
    );
    let submit_events = submit_provider.events();
    assert_eq!(
        submit_events
            .iter()
            .filter(|e| e.starts_with("press:"))
            .count(),
        1,
        "submit recovery after mouse stop must not press dictation again: {submit_events:?}"
    );
    assert_eq!(
        submit_events.iter().filter(|e| *e == "submit").count(),
        1,
        "submit recovery must submit cleanly: {submit_events:?}"
    );

    let cleanup_provider = Provider::antigravity();
    cleanup_provider.0.lock().unwrap().window.identity.title = "Conversation A".into();
    let cleanup_controls = cleanup_provider.controls();
    assert_eq!(
        cleanup_controls.toggle_dictation("antigravity"),
        Ok(ObservedDictationState::Recording)
    );
    {
        let mut state = cleanup_provider.0.lock().unwrap();
        state.window.controls[0].label = "Record voice memo".into();
        state.window.identity.title = "Conversation B".into();
    }
    assert_eq!(
        cleanup_controls.stop_owned_dictations(),
        Ok(()),
        "background cleanup must recover when the same window is idle after conversation title drift"
    );
    assert!(
        cleanup_provider
            .0
            .lock()
            .unwrap()
            .retained_targets
            .is_empty(),
        "background cleanup recovery must release retained dictation targets"
    );
    assert_eq!(
        cleanup_provider
            .events()
            .iter()
            .filter(|e| e.starts_with("press:"))
            .count(),
        1,
        "background cleanup recovery after mouse stop must not emit extra press events"
    );
}

#[test]
fn non_dictation_controls_ignore_uncleared_dictation_target_title_change() {
    let provider = Provider::antigravity();
    {
        let mut state = provider.0.lock().unwrap();
        state.window.identity.title = "Conversation A".into();
        state.window.controls.extend([
            ManagedControl {
                id: 40,
                role: "AXPopUpButton".into(),
                label: "Select model, current: Gemini 4 Carbon High".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: Some(false),
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
        ]);
        state.after_press = Some(|fixture, label, id| match label {
            "Toggle Side Panel" => {
                if let Some(ctrl) = fixture.window.controls.iter_mut().find(|c| c.id == id) {
                    ctrl.expanded = Some(!ctrl.expanded.unwrap_or(false));
                }
            }
            "New tab" => {
                fixture.window.controls.push(ManagedControl {
                    id: 44,
                    role: "AXButton".into(),
                    label: "Browser".into(),
                    enabled: true,
                    pressable: true,
                    editable: false,
                    focused: false,
                    expanded: None,
                });
            }
            "Browser" => {
                fixture.window.controls.retain(|c| c.id != 44);
                fixture.window.controls.push(ManagedControl {
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
            _ => {}
        });
    }
    let controls = provider.controls();
    assert_eq!(
        controls.toggle_dictation("antigravity"),
        Ok(ObservedDictationState::Recording)
    );
    provider.0.lock().unwrap().window.identity.title = "Conversation B".into();

    assert_eq!(
        controls.step_model_picker("antigravity", 0x7D),
        Ok(()),
        "model picker stepping must not be blocked by uncleared dictation title drift"
    );
    assert_eq!(
        controls.set_side_panel_open("antigravity", true),
        Ok(()),
        "side panel open must not be blocked by uncleared dictation title drift"
    );
    assert_eq!(
        controls.open_browser("antigravity"),
        Ok(()),
        "browser open must not be blocked by uncleared dictation title drift"
    );
    let events = provider.events();
    assert!(events.contains(&"window_key:arrow_down".to_string()));
    assert!(events.contains(&"press:Toggle Side Panel".to_string()));
    assert!(events.contains(&"press:Browser".to_string()));
    assert!(
        !events.contains(&"press:Stop Recording".to_string()),
        "non-dictation controls must not press the active dictation control: {events:?}"
    );
}

#[test]
fn active_recording_with_changed_window_title_still_blocks_toggle_and_submit_without_press() {
    let provider = Provider::antigravity();
    provider.0.lock().unwrap().window.identity.title = "Conversation A".into();
    let controls = provider.controls();
    assert_eq!(
        controls.toggle_dictation("antigravity"),
        Ok(ObservedDictationState::Recording)
    );
    assert_eq!(
        provider.0.lock().unwrap().window.controls[0].label,
        "Stop Recording"
    );

    provider.0.lock().unwrap().window.identity.title = "Conversation B".into();

    assert_eq!(
        controls.toggle_dictation("antigravity"),
        Err("blocked:uncleared_dictation_target_changed".into())
    );
    assert_eq!(
        controls.submit("antigravity", None),
        Err("blocked:uncleared_dictation_target_changed".into())
    );
    let cleanup_err = controls
        .stop_owned_dictations()
        .expect_err("cleanup must fail closed while dictation is still actively recording under a changed title");
    assert!(
        cleanup_err.contains("dictation_target_changed"),
        "expected dictation_target_changed error, got {cleanup_err}"
    );
    let blocked_events = provider.events();
    assert_eq!(
        blocked_events
            .iter()
            .filter(|e| e.starts_with("press:"))
            .count(),
        1,
        "active recording with changed title must not emit any new press events: {blocked_events:?}"
    );
    assert!(
        !blocked_events.iter().any(|e| e == "submit"),
        "active recording with changed title must never submit: {blocked_events:?}"
    );
    assert!(
        !provider.0.lock().unwrap().retained_targets.is_empty(),
        "active recording target must stay retained while still recording"
    );

    provider.0.lock().unwrap().window.controls[0].label = "Record voice memo".into();
    assert_eq!(
        controls.stop_owned_dictations(),
        Ok(()),
        "cleanup must succeed and clear the retained target once UI reports Idle"
    );
    assert!(
        provider.0.lock().unwrap().retained_targets.is_empty(),
        "cleanup after manual UI stop must release the retained target"
    );
    assert_eq!(
        provider
            .events()
            .iter()
            .filter(|e| e.starts_with("press:"))
            .count(),
        1,
        "cleanup after manual UI stop must not emit any new press events"
    );
}

#[test]
fn codex_finishing_or_missing_control_with_changed_title_stays_strictly_blocked() {
    for pending in ["Finishing dictation", "正在完成听写", ""] {
        let provider = Provider::codex();
        provider.0.lock().unwrap().window.identity.title = "Thread A".into();
        let controls = provider.controls();
        controls.start_dictation("codex").unwrap();
        {
            let mut state = provider.0.lock().unwrap();
            state.window.identity.title = "Thread B".into();
            if pending.is_empty() {
                state.window.controls.remove(0);
            } else {
                state.window.controls[0].label = pending.into();
                state.window.controls[0].enabled = false;
                state.window.controls[0].pressable = false;
            }
        }
        assert_eq!(
            controls.toggle_dictation("codex"),
            Err("blocked:uncleared_dictation_target_changed".into())
        );
        assert_eq!(
            controls.submit("codex", None),
            Err("blocked:uncleared_dictation_target_changed".into())
        );
        let cleanup_err = controls.stop_owned_dictations().expect_err(
            "cleanup must remain blocked while Codex dictation is finishing or missing",
        );
        assert!(
            cleanup_err.contains("dictation_target_changed"),
            "expected dictation_target_changed error, got {cleanup_err}"
        );
        let blocked_events = provider.events();
        assert_eq!(
            blocked_events
                .iter()
                .filter(|e| e.starts_with("press:"))
                .count(),
            1,
            "finishing or missing Codex control under changed title must not emit extra presses: {blocked_events:?}"
        );
        assert!(
            !blocked_events.iter().any(|e| e == "submit"),
            "finishing or missing Codex control under changed title must never submit: {blocked_events:?}"
        );
        assert_eq!(
            provider.0.lock().unwrap().retained_targets.len(),
            1,
            "retained dictation lease must remain held while state is not confirmed Idle"
        );

        {
            let mut state = provider.0.lock().unwrap();
            let idle_control = ManagedControl {
                id: 2,
                role: "AXCheckBox".into(),
                label: "Dictate".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: None,
            };
            if pending.is_empty() {
                state.window.controls.insert(0, idle_control);
            } else {
                state.window.controls[0] = idle_control;
            }
        }
        assert_eq!(
            controls.stop_owned_dictations(),
            Ok(()),
            "cleanup must release the stale lease once Codex reports Idle"
        );
        assert!(provider.0.lock().unwrap().retained_targets.is_empty());
        assert_eq!(
            provider
                .events()
                .iter()
                .filter(|e| e.starts_with("press:"))
                .count(),
            1
        );
    }
}

#[test]
fn active_model_picker_stays_active_when_window_title_changes_in_place() {
    let ag_provider = Provider::antigravity();
    {
        let mut state = ag_provider.0.lock().unwrap();
        state.window.identity.title = "Conversation A".into();
        state.window.controls.push(ManagedControl {
            id: 40,
            role: "AXButton".into(),
            label: "Select model".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: None,
        });
    }
    let ag_controls = ag_provider.controls();
    ag_controls.open_model_picker("antigravity").unwrap();
    ag_provider.0.lock().unwrap().window.identity.title = "Conversation B".into();
    ag_controls.step_model_picker("antigravity", 0x7D).unwrap();
    assert_eq!(
        ag_controls.confirm_model_picker("antigravity"),
        Ok(ModelPickerConfirmResult::Confirmed)
    );
    let ag_events = ag_provider.events();
    assert_eq!(
        ag_events
            .iter()
            .filter(|e| *e == "press:Select model")
            .count(),
        1,
        "in-place title drift must not re-open an already active Antigravity model picker: {ag_events:?}"
    );
    assert!(ag_events.contains(&"window_key:arrow_down".to_string()));
    assert!(ag_events.contains(&"window_key:return".to_string()));

    ag_controls.open_model_picker("antigravity").unwrap();
    ag_provider.0.lock().unwrap().window.identity.window_id = 2;
    ag_controls.step_model_picker("antigravity", 0x7D).unwrap();
    assert_eq!(
        ag_provider
            .events()
            .iter()
            .filter(|e| *e == "press:Select model")
            .count(),
        3,
        "switching to a different window container must not inherit the prior active picker target"
    );

    let codex_provider = Provider::codex();
    {
        let mut state = codex_provider.0.lock().unwrap();
        state.window.identity.title = "Thread A".into();
        state.window.controls.push(ManagedControl {
            id: 41,
            role: "AXPopUpButton".into(),
            label: "GPT-5".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: Some(false),
        });
    }
    let codex_controls = codex_provider.controls();
    codex_controls.open_model_picker("codex").unwrap();
    codex_provider.0.lock().unwrap().window.identity.title = "Thread B".into();
    codex_controls.step_model_picker("codex", 0x7D).unwrap();
    assert_eq!(
        codex_controls.confirm_model_picker("codex"),
        Ok(ModelPickerConfirmResult::Confirmed),
        "in-place title drift must keep an active Codex model picker confirmable"
    );
    let codex_events = codex_provider.events();
    assert_eq!(
        codex_events
            .iter()
            .filter(|e| *e == "window_key:control+shift:2e")
            .count(),
        1,
        "in-place title drift must not re-send Ctrl+Shift+M on an active Codex picker: {codex_events:?}"
    );
    assert!(codex_events.contains(&"window_key:arrow_down".to_string()));
    assert!(codex_events.contains(&"window_key:return".to_string()));
}

#[test]
fn antigravity_chinese_dictation_toggle_starts_and_stops() {
    let provider = Provider::with_target("com.google.antigravity", "录制语音输入");
    let controls = provider.controls();
    assert_eq!(
        controls.toggle_dictation("antigravity"),
        Ok(ObservedDictationState::Recording)
    );
    assert_eq!(
        controls.toggle_dictation("antigravity"),
        Ok(ObservedDictationState::Idle)
    );
    assert_eq!(
        provider.events(),
        vec![
            "capture:com.google.antigravity",
            "press:录制语音输入",
            "capture:com.google.antigravity",
            "press:停止录制",
        ]
    );
}

#[test]
fn antigravity_chinese_live_voice_toggles() {
    let provider = Provider::with_target("com.google.antigravity", "开启语音模式");
    let controls = provider.controls();
    controls.toggle_live_voice("antigravity").unwrap();
    controls.toggle_live_voice("antigravity").unwrap();
    assert_eq!(
        provider.events(),
        vec![
            "capture:com.google.antigravity",
            "press:开启语音模式",
            "capture:com.google.antigravity",
            "press:结束语音模式",
        ]
    );
}

#[test]
fn antigravity_chinese_model_picker_and_side_panel_and_browser_and_side_question() {
    let model_provider = Provider::with_target("com.google.antigravity", "录制语音输入");
    {
        let mut state = model_provider.0.lock().unwrap();
        state.window.controls.push(ManagedControl {
            id: 40,
            role: "AXButton".into(),
            label: "模型，当前：Gemini 4 Neon High".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: None,
        });
        state.window.controls.push(ManagedControl {
            id: 41,
            role: "AXButton".into(),
            label: "权限模式，当前：全自动放行 (Turbo)".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: None,
        });
    }
    let model_controls = model_provider.controls();
    model_controls.open_model_picker("antigravity").unwrap();
    model_controls
        .step_model_picker("antigravity", 0x7D)
        .unwrap();
    assert_eq!(
        model_controls.confirm_model_picker("antigravity"),
        Ok(ModelPickerConfirmResult::Confirmed)
    );
    assert!(model_provider
        .events()
        .contains(&"press:模型，当前：Gemini 4 Neon High".to_string()));

    let panel_provider = Provider::with_target("com.google.antigravity", "录制语音输入");
    {
        let mut state = panel_provider.0.lock().unwrap();
        state.effect = PressEffect::NoChange;
        state.window.controls = vec![ManagedControl {
            id: 10,
            role: "AXButton".into(),
            label: "切换侧边面板".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: Some(false),
        }];
        state.after_press = Some(|fixture, label, _id| {
            if label == "切换侧边面板" {
                if let Some(btn) = fixture.window.controls.iter_mut().find(|c| c.id == 10) {
                    btn.expanded = Some(!btn.expanded.unwrap_or(false));
                }
            }
        });
    }
    let panel_controls = panel_provider.controls();
    panel_controls
        .set_side_panel_open("antigravity", true)
        .unwrap();
    panel_controls
        .set_side_panel_open("antigravity", false)
        .unwrap();
    assert_eq!(
        panel_provider
            .events()
            .iter()
            .filter(|e| *e == "press:切换侧边面板")
            .count(),
        2
    );

    let browser_provider = Provider::with_target("com.google.antigravity", "录制语音输入");
    {
        let mut state = browser_provider.0.lock().unwrap();
        state.effect = PressEffect::NoChange;
        state.window.controls = vec![ManagedControl {
            id: 10,
            role: "AXButton".into(),
            label: "切换侧边面板".into(),
            enabled: true,
            pressable: true,
            editable: false,
            focused: false,
            expanded: Some(false),
        }];
        state.after_press = Some(|fixture, label, _id| {
            if label == "切换侧边面板" {
                if let Some(btn) = fixture.window.controls.iter_mut().find(|c| c.id == 10) {
                    btn.expanded = Some(true);
                }
                fixture.window.controls.push(ManagedControl {
                    id: 11,
                    role: "AXButton".into(),
                    label: "新标签页".into(),
                    enabled: true,
                    pressable: true,
                    editable: false,
                    focused: false,
                    expanded: None,
                });
            } else if label == "新标签页" {
                fixture.window.controls.push(ManagedControl {
                    id: 20,
                    role: "AXTextField".into(),
                    label: "搜索要打开的内容".into(),
                    enabled: true,
                    pressable: false,
                    editable: true,
                    focused: true,
                    expanded: None,
                });
                fixture.window.controls.push(ManagedControl {
                    id: 21,
                    role: "AXButton".into(),
                    label: "内置浏览器".into(),
                    enabled: true,
                    pressable: true,
                    editable: false,
                    focused: false,
                    expanded: None,
                });
            } else if label == "内置浏览器" {
                fixture.window.controls.retain(|c| c.id != 20 && c.id != 21);
                fixture.window.controls.push(ManagedControl {
                    id: 30,
                    role: "AXComboBox".into(),
                    label: "地址栏".into(),
                    enabled: true,
                    pressable: false,
                    editable: true,
                    focused: false,
                    expanded: None,
                });
            }
        });
    }
    browser_provider
        .controls()
        .open_browser("antigravity")
        .unwrap();
    let browser_events = browser_provider.events();
    assert!(browser_events.contains(&"press:切换侧边面板".to_string()));
    assert!(browser_events.contains(&"press:新标签页".to_string()));
    assert!(browser_events.contains(&"press:内置浏览器".to_string()));

    let side_question_provider = Provider::with_target("com.google.antigravity", "录制语音输入");
    {
        let mut state = side_question_provider.0.lock().unwrap();
        state.effect = PressEffect::NoChange;
        state.window.controls = vec![
            ManagedControl {
                id: 10,
                role: "AXButton".into(),
                label: "切换侧边面板".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: Some(true),
            },
            ManagedControl {
                id: 20,
                role: "AXButton".into(),
                label: "旁路提问".into(),
                enabled: true,
                pressable: true,
                editable: false,
                focused: false,
                expanded: None,
            },
        ];
        state.after_press = Some(|fixture, label, _id| {
            if label == "旁路提问" {
                fixture.window.controls.retain(|c| c.id != 20);
                fixture.window.controls.push(ManagedControl {
                    id: 25,
                    role: "AXButton".into(),
                    label: "新建支线提问".into(),
                    enabled: true,
                    pressable: true,
                    editable: false,
                    focused: false,
                    expanded: None,
                });
            } else if label == "新建支线提问" {
                fixture.window.controls.push(ManagedControl {
                    id: 30,
                    role: "AXTextArea".into(),
                    label: "Side Question Input".into(),
                    enabled: true,
                    pressable: false,
                    editable: true,
                    focused: false,
                    expanded: None,
                });
            }
        });
    }
    side_question_provider
        .controls()
        .open_side_question("antigravity")
        .unwrap();
    let sq_events = side_question_provider.events();
    assert!(sq_events.contains(&"press:旁路提问".to_string()));
    assert!(sq_events.contains(&"press:新建支线提问".to_string()));
    assert!(sq_events.contains(&"focus:30".to_string()));
}
