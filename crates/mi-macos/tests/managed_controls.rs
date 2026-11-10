use mi_macos::executor::{
    ClipboardSnapshot, DialScrollRequest, HostOsDriver, LiveHostActionExecutor,
};
use mi_macos::managed_controls::{
    ManagedAxControls, ManagedAxProvider, ManagedControl, ManagedWindow, ManagedWindowIdentity,
    ObservedDictationState,
};
use mi_model::host_router::ResolvedHostAction;
use std::sync::{Arc, Barrier, Mutex};
use std::time::Duration;

#[derive(Clone, Copy)]
enum PressEffect {
    Toggle,
    NoChange,
    Replace,
    LoseTarget,
}
struct Fixture {
    window: ManagedWindow,
    effect: PressEffect,
    events: Vec<String>,
    read_pid: Option<i32>,
    capture_error: Option<String>,
    focus_confirmed: bool,
}
#[derive(Clone)]
struct Provider(Arc<Mutex<Fixture>>);
impl Provider {
    fn cindy() -> Self {
        Self(Arc::new(Mutex::new(Fixture {
            window: ManagedWindow {
                identity: ManagedWindowIdentity {
                    bundle_id: "com.xd.cindy".into(),
                    pid: 100,
                    window_id: 1,
                    title: "Test".into(),
                },
                controls: vec![
                    ManagedControl {
                        id: 2,
                        role: "AXCheckBox".into(),
                        label: "开始语音输入".into(),
                        enabled: true,
                        pressable: true,
                        editable: false,
                        focused: false,
                    },
                    ManagedControl {
                        id: 3,
                        role: "AXTextArea".into(),
                        label: String::new(),
                        enabled: true,
                        pressable: false,
                        editable: true,
                        focused: false,
                    },
                ],
            },
            effect: PressEffect::Toggle,
            events: Vec::new(),
            read_pid: None,
            capture_error: None,
            focus_confirmed: true,
        })))
    }
    fn controls(&self) -> ManagedAxControls<Self> {
        ManagedAxControls::with_readback_timeout(self.clone(), Duration::ZERO)
    }
    fn events(&self) -> Vec<String> {
        self.0.lock().unwrap().events.clone()
    }
}
impl ManagedAxProvider for Provider {
    fn capture(&mut self, bundle: &str) -> Result<ManagedWindow, String> {
        let mut state = self.0.lock().unwrap();
        state.events.push(format!("capture:{bundle}"));
        if let Some(error) = &state.capture_error {
            return Err(error.clone());
        }
        Ok(state.window.clone())
    }
    fn read(&mut self, target: &ManagedWindowIdentity) -> Result<ManagedWindow, String> {
        let state = self.0.lock().unwrap();
        assert_eq!(target.pid, 100, "must not reselect another PID");
        let mut window = state.window.clone();
        if let Some(pid) = state.read_pid {
            window.identity.pid = pid;
        }
        Ok(window)
    }
    fn press(&mut self, target: &ManagedWindowIdentity, control: u64) -> Result<(), String> {
        // Yield to another simultaneous request before changing observed AX state.
        std::thread::sleep(Duration::from_millis(2));
        let mut state = self.0.lock().unwrap();
        assert_eq!(*target, state.window.identity);
        let effect = state.effect;
        let control = state
            .window
            .controls
            .iter_mut()
            .find(|c| c.id == control)
            .unwrap();
        let old_label = control.label.clone();
        match effect {
            PressEffect::Toggle | PressEffect::Replace => {
                control.label = if control.label == "开始语音输入" {
                    "停止语音输入"
                } else {
                    "开始语音输入"
                }
                .into();
                if matches!(effect, PressEffect::Replace) {
                    control.id += 10;
                }
            }
            PressEffect::LoseTarget => state.read_pid = Some(999),
            PressEffect::NoChange => {}
        }
        state.events.push(format!("press:{old_label}"));
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
        state.events.push("scroll".into());
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
            "停止语音输入",
            "停止并识别",
            "Stop and transcribe"
        ]
        .contains(&control.label.as_str())));
        state.events.push("submit".into());
        Ok(())
    }
}

#[test]
fn toggle_observes_mouse_changes_and_replacement_controls() {
    let provider = Provider::cindy();
    let controls = provider.controls();
    assert_eq!(
        controls.toggle_dictation("cindy").unwrap(),
        ObservedDictationState::Recording
    );
    // A user stopped recording externally; the next click must start again.
    provider.0.lock().unwrap().window.controls[0].label = "开始语音输入".into();
    provider.0.lock().unwrap().effect = PressEffect::Replace;
    assert_eq!(
        controls.toggle_dictation("cindy").unwrap(),
        ObservedDictationState::Recording
    );
    assert_eq!(
        controls.toggle_dictation("cindy").unwrap(),
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
        let provider = Provider::cindy();
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
                    extra.label = "停止语音输入".into();
                    state.window.controls.push(extra);
                }
            }
        }
        assert!(provider
            .controls()
            .toggle_dictation("cindy")
            .unwrap_err()
            .starts_with("blocked:"));
        assert!(!provider.events().iter().any(|e| e.starts_with("press:")));
    }
}

#[test]
fn target_pid_or_bundle_mismatch_never_press() {
    for at_capture in [true, false] {
        let provider = Provider::cindy();
        if at_capture {
            provider.0.lock().unwrap().window.identity.bundle_id = "com.openai.codex".into();
        } else {
            provider.0.lock().unwrap().read_pid = Some(999);
        }
        assert!(provider
            .controls()
            .toggle_dictation("cindy")
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
        let provider = Provider::cindy();
        provider.0.lock().unwrap().capture_error = Some(error.into());
        assert_eq!(
            provider.controls().toggle_dictation("cindy").unwrap_err(),
            error
        );
        assert_eq!(provider.events().len(), 1);
    }
}

#[test]
fn unconfirmed_press_is_not_retried_and_never_submits() {
    for effect in [PressEffect::NoChange, PressEffect::LoseTarget] {
        let provider = Provider::cindy();
        {
            let mut state = provider.0.lock().unwrap();
            state.effect = effect;
            state.window.controls[0].label = "停止语音输入".into();
        }
        assert!(provider
            .controls()
            .submit("cindy", None)
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
    let provider = Provider::cindy();
    provider.0.lock().unwrap().window.controls[0].label = "停止语音输入".into();
    provider
        .controls()
        .submit("cindy", Some("test prompt"))
        .unwrap();
    assert_eq!(
        provider.events(),
        [
            "capture:com.xd.cindy",
            "press:停止语音输入",
            "focus:3",
            "insert:test prompt",
            "submit"
        ]
    );
}

#[test]
fn missing_or_ambiguous_or_unconfirmed_input_cannot_submit_or_send_shortcuts() {
    for case in 0..3 {
        let provider = Provider::cindy();
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
        assert!(provider.controls().submit("cindy", None).is_err());
        assert!(provider
            .controls()
            .shortcut("cindy", 0x2d, &["command".into()])
            .is_err());
        assert!(!provider
            .events()
            .iter()
            .any(|e| e == "submit" || e == "targeted_shortcut"));
    }
}

#[test]
fn bottom_scroll_and_shortcut_remain_on_captured_target() {
    let provider = Provider::cindy();
    provider
        .controls()
        .scroll(&DialScrollRequest {
            target_bundle_id: "com.xd.cindy".into(),
            delta_lines: 0,
            jump_to_bottom: true,
            sidebar: false,
        })
        .unwrap();
    provider
        .controls()
        .shortcut("cindy", 0x2d, &["command".into()])
        .unwrap();
    assert_eq!(
        provider.events(),
        [
            "capture:com.xd.cindy",
            "scroll",
            "focus:3",
            "capture:com.xd.cindy",
            "focus:3",
            "targeted_shortcut"
        ]
    );
}

#[test]
fn rapid_clicks_serialize_observation_press_and_readback() {
    let provider = Provider::cindy();
    let controls = Arc::new(provider.controls());
    let barrier = Arc::new(Barrier::new(3));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let controls = controls.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                controls.toggle_dictation("cindy").unwrap()
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
            "capture:com.xd.cindy",
            "press:开始语音输入",
            "capture:com.xd.cindy",
            "press:停止语音输入"
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
    fn agent_shortcut(&self, adapter: &str, key: u16, modifiers: &[String]) -> Result<(), String> {
        self.controls.shortcut(adapter, key, modifiers)
    }
    fn scroll_window(&self, request: &DialScrollRequest) -> Result<(), String> {
        self.controls.scroll(request)
    }
    fn new_conversation_in_project(&self, _: &str) -> Result<(), String> {
        unreachable!()
    }
    fn toggle_live_voice(&self, _: &str) -> Result<(), String> {
        unreachable!()
    }
}

#[test]
fn live_executor_reports_confirmed_toggle_and_explicit_failure() {
    let provider = Provider::cindy();
    let executor = LiveHostActionExecutor::new(Driver {
        controls: provider.controls(),
        trusted: true,
    });
    let action = ResolvedHostAction::AgentCommand {
        adapter: "cindy".into(),
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
    for (adapter, trusted) in [("unknown", true), ("codex", true), ("cindy", false)] {
        let provider = Provider::cindy();
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
    let provider = Provider::cindy();
    let executor = LiveHostActionExecutor::new(Driver {
        controls: provider.controls(),
        trusted: true,
    });
    for command in ["submit", "new-chat", "search", "terminal"] {
        let receipt = executor.execute(
            "test",
            "button",
            &ResolvedHostAction::AgentCommand {
                adapter: "cindy".into(),
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
fn absent_voice_capability_does_not_block_submit_but_uncleared_recording_does() {
    let provider = Provider::cindy();
    let controls = provider.controls();
    let voice = provider.0.lock().unwrap().window.controls.remove(0);
    controls.submit("cindy", None).unwrap();
    provider.0.lock().unwrap().window.controls.insert(0, voice);
    controls.toggle_dictation("cindy").unwrap();
    provider.0.lock().unwrap().window.controls.remove(0);
    assert!(controls
        .submit("cindy", None)
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
    let provider = Provider::cindy();
    let controls = provider.controls();
    controls.toggle_dictation("cindy").unwrap();
    provider.0.lock().unwrap().window.identity.window_id = 20;
    assert_eq!(
        controls.submit("cindy", None).unwrap_err(),
        "blocked:uncleared_dictation_target_changed"
    );
    assert_eq!(
        controls.toggle_dictation("cindy").unwrap_err(),
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
    assert!(matches!(
        adapter.as_str(),
        "cindy" | "antigravity" | "dsh-desktop"
    ));
    let action = std::env::var("MI_LIVE_AX_ACTION").unwrap_or_else(|_| "inspect".into());
    let driver = MacOsSystemDriver::default();
    match action.as_str() {
        "inspect" => println!(
            "{:?}",
            driver
                .inspect_managed_controls(&adapter)
                .expect("inspect target")
        ),
        "focus" => driver
            .focus_agent_input(&adapter)
            .expect("focus target input"),
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
        _ => panic!("allowed actions: inspect, focus, toggle, scroll-up, scroll-down, bottom"),
    }
}
