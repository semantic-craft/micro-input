use mi_macos::executor::{
    ClipboardSnapshot, DialScrollRequest, HostOsDriver, LiveHostActionExecutor, APPLE_FN_KEY_CODE,
};
use mi_model::host_router::ResolvedHostAction;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

struct BlockingDriver {
    entered: mpsc::SyncSender<()>,
    proceed: Mutex<mpsc::Receiver<()>>,
    fn_attempt: mpsc::SyncSender<()>,
    events: Arc<Mutex<Vec<&'static str>>>,
}

impl HostOsDriver for BlockingDriver {
    fn is_accessibility_trusted(&self) -> bool {
        true
    }
    fn open_application(&self, _: &str) -> Result<(), String> {
        self.events.lock().unwrap().push("open");
        self.entered.send(()).unwrap();
        self.proceed.lock().unwrap().recv().unwrap();
        Ok(())
    }
    fn post_key_shortcut(&self, _: u16, _: &[String], down: bool) -> Result<(), String> {
        self.events
            .lock()
            .unwrap()
            .push(if down { "key-down" } else { "key-up" });
        Ok(())
    }
    fn post_apple_fn_flags(&self, down: bool) -> Result<(), String> {
        self.events
            .lock()
            .unwrap()
            .push(if down { "fn-down" } else { "fn-up" });
        if down {
            self.fn_attempt.send(()).unwrap();
            Ok(())
        } else {
            Err("fn-up-unconfirmed".into())
        }
    }
    fn snapshot_clipboard(&self, _: usize) -> ClipboardSnapshot {
        unreachable!()
    }
    fn write_clipboard_plain_text(&self, _: &str) -> u64 {
        unreachable!()
    }
    fn restore_clipboard(&self, _: &ClipboardSnapshot, _: u64) -> bool {
        unreachable!()
    }
    fn open_url(&self, _: &str) -> Result<(), String> {
        unreachable!()
    }
    fn focus_agent_input(&self, _: &str) -> Result<(), String> {
        unreachable!()
    }
    fn stop_active_dictation(&self, _: &str) -> Result<bool, String> {
        unreachable!()
    }
    fn scroll_window(&self, _: &DialScrollRequest) -> Result<(), String> {
        unreachable!()
    }
    fn new_conversation_in_project(&self, _: &str) -> Result<(), String> {
        unreachable!()
    }
    fn toggle_live_voice(&self, _: &str) -> Result<(), String> {
        unreachable!()
    }
}

#[test]
fn shared_executor_serializes_host_effects_and_blocks_after_failed_fn_release() {
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (proceed_tx, proceed_rx) = mpsc::sync_channel(1);
    let (fn_tx, fn_rx) = mpsc::sync_channel(1);
    let events = Arc::new(Mutex::new(Vec::new()));
    let executor = Arc::new(LiveHostActionExecutor::new(BlockingDriver {
        entered: entered_tx,
        proceed: Mutex::new(proceed_rx),
        fn_attempt: fn_tx,
        events: events.clone(),
    }));
    let regular_executor = executor.clone();
    let regular = std::thread::spawn(move || {
        regular_executor.execute(
            "profile",
            "key",
            &ResolvedHostAction::Shortcut {
                key: "a".into(),
                key_code: 0,
                modifiers: vec![],
                target_bundle_id: Some("com.example.editor".into()),
            },
        )
    });
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let pulse_executor = executor.clone();
    let (attempt_tx, attempt_rx) = mpsc::sync_channel(1);
    let pulse = std::thread::spawn(move || {
        attempt_tx.send(()).unwrap();
        pulse_executor.execute(
            "other-profile",
            "fn",
            &ResolvedHostAction::Shortcut {
                key: "fn".into(),
                key_code: APPLE_FN_KEY_CODE,
                modifiers: vec![],
                target_bundle_id: None,
            },
        )
    });
    attempt_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let early_fn = fn_rx.recv_timeout(Duration::from_millis(100));
    proceed_tx.send(()).unwrap();
    assert_eq!(regular.join().unwrap().outcome, "executed");
    assert_eq!(pulse.join().unwrap().outcome, "failed");
    assert!(
        early_fn.is_err(),
        "Fn cannot interleave with an admitted host action"
    );
    assert_eq!(
        *events.lock().unwrap(),
        vec!["open", "key-down", "key-up", "fn-down", "fn-up"]
    );
    let before = events.lock().unwrap().clone();
    let blocked = executor.execute(
        "radial-profile",
        "radial-menu",
        &ResolvedHostAction::OpenApp {
            bundle_id: "com.example.other".into(),
        },
    );
    assert_eq!(blocked.outcome, "blocked");
    assert_eq!(blocked.detail, "apple_fn_cleanup_unconfirmed");
    assert!(executor.start_owned_dictation("antigravity").is_err());
    executor.stop_owned_dictations().unwrap();
    assert_eq!(*events.lock().unwrap(), before);
}
