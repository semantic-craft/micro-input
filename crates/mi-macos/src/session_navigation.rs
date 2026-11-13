use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarRowVisualState {
    InProgress,
    UnreadIdle,
    ReadIdle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SidebarConversationObservation {
    pub conversation_id: String,
    pub title: String,
    pub state: SidebarRowVisualState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionNavigationWindow {
    pub pid: i32,
    pub window: u64,
    pub selected_urls: Vec<String>,
}

pub trait SessionNavigationProvider {
    fn capture(&mut self, pid: i32) -> Result<SessionNavigationWindow, String>;
    fn read(&mut self, target: &SessionNavigationWindow)
        -> Result<SessionNavigationWindow, String>;
    fn select_conversation(
        &mut self,
        target: &SessionNavigationWindow,
        conversation: &str,
        validate_lease: &mut dyn FnMut() -> Result<(), String>,
        is_current: &dyn Fn() -> bool,
    ) -> Result<(), String>;
}

pub(crate) fn app_document_path(url: &str, expected_port: u16) -> Option<&str> {
    let rest = url.strip_prefix("https://")?;
    let (host, path) = rest.split_once('/')?;
    let port = host.strip_prefix("127.0.0.1:")?.parse::<u16>().ok()?;
    (port != 0 && port == expected_port).then_some(path)
}

pub(crate) fn conversation_id_from_url(url: &str, expected_port: u16) -> Option<&str> {
    let path = app_document_path(url, expected_port)?;
    path.split(['?', '#'])
        .next()
        .and_then(|path| path.strip_prefix("c/"))
        .filter(|id| !id.is_empty() && !id.contains('/'))
}

fn percent_decode(raw: &str) -> Option<String> {
    let mut out = Vec::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = std::str::from_utf8(bytes.get(index + 1..index + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else if bytes[index] == b'+' {
            out.push(b' ');
            index += 1;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

pub(crate) fn url_matches_project(
    url: &str,
    expected_port: u16,
    expected_project_id: &str,
) -> bool {
    if expected_project_id.is_empty() {
        return false;
    }
    let Some(path_and_query) = app_document_path(url, expected_port) else {
        return false;
    };
    let before_fragment = path_and_query.split('#').next().unwrap_or_default();
    let Some((_, query)) = before_fragment.split_once('?') else {
        return false;
    };
    query.split('&').any(|pair| {
        let Some((key, value)) = pair.split_once('=') else {
            return false;
        };
        key == "project"
            && (value == expected_project_id
                || percent_decode(value).as_deref() == Some(expected_project_id))
    })
}

pub(crate) fn selected_conversation(
    window: &SessionNavigationWindow,
    port: u16,
) -> Result<String, String> {
    let documents: Vec<_> = window
        .selected_urls
        .iter()
        .filter_map(|url| app_document_path(url, port))
        .collect();
    if documents.len() != 1 {
        return Err("unconfirmed:session_document_ambiguous".into());
    }
    let id = documents[0]
        .split(['?', '#'])
        .next()
        .and_then(|path| path.strip_prefix("c/"))
        .filter(|id| !id.is_empty() && !id.contains('/'))
        .ok_or("unconfirmed:session_document_not_selected")?;
    Ok(id.to_string())
}

pub fn navigate_session(
    provider: &mut impl SessionNavigationProvider,
    pid: i32,
    port: u16,
    conversation: &str,
    mut validate_lease: impl FnMut() -> Result<(), String>,
    is_current: impl Fn() -> bool,
) -> Result<(), String> {
    validate_lease()?;
    let window = provider.capture(pid)?;
    if window.pid != pid {
        return Err("blocked:session_process_changed".into());
    }
    provider.select_conversation(&window, conversation, &mut validate_lease, &is_current)?;
    let deadline = Instant::now() + Duration::from_millis(1500);
    loop {
        let readback = provider
            .read(&window)
            .map_err(|error| format!("unconfirmed:session_readback:{error}"))?;
        if readback.pid != window.pid || readback.window != window.window {
            return Err("unconfirmed:session_window_changed".into());
        }
        if selected_conversation(&readback, port).as_deref() == Ok(conversation) {
            validate_lease()?;
            let final_readback = provider
                .read(&window)
                .map_err(|error| format!("unconfirmed:session_final_readback:{error}"))?;
            if final_readback.pid != pid
                || final_readback.window != window.window
                || selected_conversation(&final_readback, port).as_deref() != Ok(conversation)
            {
                return Err("unconfirmed:session_selection_changed_during_validation".into());
            }
            if !is_current() {
                return Err("blocked:session_navigation_retired".into());
            }
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("unconfirmed:session_readback_mismatch".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub fn antigravity_process_id() -> Option<i32> {
    #[cfg(target_os = "macos")]
    {
        crate::executor::find_pid_for_bundle_id("com.google.antigravity")
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

pub fn frontmost_selected_antigravity_conversation(pid: i32, port: u16) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        crate::native_ax::frontmost_selected_antigravity_conversation(pid, port)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (pid, port);
        None
    }
}

pub fn observe_antigravity_sidebar_conversations(
    pid: i32,
    port: u16,
) -> Result<Vec<SidebarConversationObservation>, String> {
    #[cfg(target_os = "macos")]
    {
        crate::native_ax::observe_antigravity_sidebar_conversations(pid, port)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (pid, port);
        Err("blocked:macos_required".into())
    }
}

pub fn navigate_antigravity_session(
    pid: i32,
    port: u16,
    conversation: &str,
    workspace_path: &str,
    project_id: Option<&str>,
    validate_lease: impl FnMut() -> Result<(), String>,
    is_current: impl Fn() -> bool,
) -> Result<(), String> {
    navigate_session(
        &mut crate::native_ax::NativeSessionNavigation::new(port, workspace_path, project_id),
        pid,
        port,
        conversation,
        validate_lease,
        is_current,
    )
}

pub fn default_codex_home() -> Option<PathBuf> {
    if let Some(codex_home) = std::env::var_os("CODEX_HOME") {
        if !codex_home.is_empty() {
            return Some(PathBuf::from(codex_home));
        }
    }
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(|home| PathBuf::from(home).join(".codex"))
}

pub fn codex_process_id() -> Option<i32> {
    #[cfg(target_os = "macos")]
    {
        crate::executor::find_pid_for_bundle_id("com.openai.codex")
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

pub fn observe_codex_threads_live(
    limit: usize,
) -> Result<Vec<crate::codex_sessions::CodexThreadObservation>, String> {
    let codex_home =
        default_codex_home().ok_or_else(|| "blocked:codex_home_unavailable".to_string())?;
    crate::codex_sessions::observe_codex_threads(&codex_home, limit)
}

#[cfg(target_os = "macos")]
fn open_codex_deeplink(deeplink: &str) -> Result<(), String> {
    let status = std::process::Command::new("/usr/bin/open")
        .args(["-b", "com.openai.codex", deeplink])
        .status()
        .map_err(|error| format!("blocked:codex_open_failed:{error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("blocked:codex_open_exited:{status}"))
    }
}

fn is_fatal_codex_ax_error(error: &str) -> bool {
    matches!(
        error,
        "blocked:accessibility_untrusted" | "blocked:macos_required"
    )
}

fn navigate_codex_session_with(
    codex_home: &Path,
    thread_id: &str,
    host_id: &str,
    mut validate_lease: impl FnMut() -> Result<(), String>,
    is_current: impl Fn() -> bool,
    mut open_deeplink: impl FnMut(&str) -> Result<(), String>,
    mut observe_window: impl FnMut() -> Result<crate::codex_sessions::CodexWindowSnapshot, String>,
    timeout: Duration,
    poll_interval: Duration,
) -> Result<(), String> {
    validate_lease()?;
    if !is_current() {
        return Err("blocked:session_navigation_retired".into());
    }
    let fingerprint =
        crate::codex_sessions::read_codex_thread_fingerprint(codex_home, thread_id, host_id)?;
    let deeplink = crate::codex_sessions::build_codex_thread_deeplink(thread_id, host_id)?;
    validate_lease()?;
    if !is_current() {
        return Err("blocked:session_navigation_retired".into());
    }
    open_deeplink(&deeplink)?;

    let deadline = Instant::now() + timeout;
    loop {
        if !is_current() {
            return Err("blocked:session_navigation_retired".into());
        }
        let last_error = match observe_window() {
            Ok(snapshot) => {
                match crate::codex_sessions::verify_codex_window_readback(&fingerprint, &snapshot) {
                    Ok(()) => {
                        validate_lease()?;
                        if !is_current() {
                            return Err("blocked:session_navigation_retired".into());
                        }
                        match observe_window() {
                            Ok(final_snapshot) => {
                                match crate::codex_sessions::verify_codex_window_readback(
                                    &fingerprint,
                                    &final_snapshot,
                                ) {
                                    Ok(()) => {
                                        if !is_current() {
                                            return Err("blocked:session_navigation_retired".into());
                                        }
                                        return Ok(());
                                    }
                                    Err(error) => error,
                                }
                            }
                            Err(error) => {
                                if is_fatal_codex_ax_error(&error) {
                                    return Err(error);
                                }
                                format!("unconfirmed:codex_final_readback:{error}")
                            }
                        }
                    }
                    Err(error) => error,
                }
            }
            Err(error) => {
                if is_fatal_codex_ax_error(&error) {
                    return Err(error);
                }
                error
            }
        };
        if Instant::now() >= deadline {
            return Err(last_error);
        }
        std::thread::sleep(poll_interval);
    }
}

pub fn navigate_codex_session(
    codex_home: &Path,
    thread_id: &str,
    host_id: &str,
    validate_lease: impl FnMut() -> Result<(), String>,
    is_current: impl Fn() -> bool,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        navigate_codex_session_with(
            codex_home,
            thread_id,
            host_id,
            validate_lease,
            is_current,
            open_codex_deeplink,
            crate::native_ax::observe_codex_window_state,
            Duration::from_secs(4),
            Duration::from_millis(100),
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (codex_home, thread_id, host_id, validate_lease, is_current);
        Err("blocked:macos_required".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fake {
        readback: SessionNavigationWindow,
        presses: usize,
        reject: bool,
        selection_changed: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    }
    impl SessionNavigationProvider for Fake {
        fn capture(&mut self, _: i32) -> Result<SessionNavigationWindow, String> {
            Ok(self.readback.clone())
        }
        fn read(&mut self, _: &SessionNavigationWindow) -> Result<SessionNavigationWindow, String> {
            let mut readback = self.readback.clone();
            if self
                .selection_changed
                .as_ref()
                .is_some_and(|changed| changed.load(std::sync::atomic::Ordering::Acquire))
            {
                readback.selected_urls = vec!["https://127.0.0.1:4321/c/other".into()];
            }
            Ok(readback)
        }
        fn select_conversation(
            &mut self,
            _: &SessionNavigationWindow,
            _: &str,
            validate_lease: &mut dyn FnMut() -> Result<(), String>,
            is_current: &dyn Fn() -> bool,
        ) -> Result<(), String> {
            validate_lease()?;
            if !is_current() {
                return Err("retired".into());
            }
            self.presses += 1;
            if self.reject {
                Err("blocked:rejected".into())
            } else {
                Ok(())
            }
        }
    }
    fn fake() -> Fake {
        let url = "https://127.0.0.1:4321/c/target".to_string();
        Fake {
            readback: SessionNavigationWindow {
                pid: 12,
                window: 1,
                selected_urls: vec![url],
            },
            presses: 0,
            reject: false,
            selection_changed: None,
        }
    }
    #[test]
    fn selection_and_same_window_url_readback_are_required() {
        let mut p = fake();
        navigate_session(&mut p, 12, 4321, "target", || Ok(()), || true).unwrap();
        assert_eq!(p.presses, 1);
        let mut p = fake();
        p.reject = true;
        assert!(navigate_session(&mut p, 12, 4321, "target", || Ok(()), || true).is_err());
        let mut p = fake();
        p.readback.selected_urls.clear();
        assert!(
            navigate_session(&mut p, 12, 4321, "target", || Ok(()), || true)
                .unwrap_err()
                .contains("readback")
        );
        let mut p = fake();
        let mut checks = 0;
        assert!(navigate_session(
            &mut p,
            12,
            4321,
            "target",
            || {
                checks += 1;
                if checks == 3 {
                    Err("replaced".into())
                } else {
                    Ok(())
                }
            },
            || true
        )
        .is_err());
    }
    #[test]
    fn selection_ignores_background_documents_but_rejects_multiple_app_documents() {
        let mut p = fake();
        p.readback
            .selected_urls
            .insert(0, "data:text/html,background".into());
        navigate_session(&mut p, 12, 4321, "target", || Ok(()), || true).unwrap();
        let mut p = fake();
        p.readback
            .selected_urls
            .push(p.readback.selected_urls[0].clone());
        assert!(navigate_session(&mut p, 12, 4321, "target", || Ok(()), || true).is_err());
        let mut p = fake();
        assert!(navigate_session(&mut p, 12, 4321, "target", || Ok(()), || false).is_err());
        assert_eq!(p.presses, 0);
    }

    #[test]
    fn selection_change_during_catalog_validation_cannot_confirm_the_prior_session() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        let changed = Arc::new(AtomicBool::new(false));
        let mut p = fake();
        p.selection_changed = Some(changed.clone());
        let mut checks = 0;
        let result = navigate_session(
            &mut p,
            12,
            4321,
            "target",
            || {
                checks += 1;
                if checks == 3 {
                    changed.store(true, Ordering::Release);
                }
                Ok(())
            },
            || true,
        );
        assert_eq!(
            result.unwrap_err(),
            "unconfirmed:session_selection_changed_during_validation"
        );
        assert_eq!(p.presses, 1);
    }

    #[test]
    fn urls_cannot_match_another_origin_or_conversation_prefix() {
        for url in [
            "https://evil.test/c/a",
            "https://127.0.0.1:45/c/ab",
            "https://127.0.0.1:0/c/a",
        ] {
            let mut p = fake();
            p.readback.selected_urls = vec![url.into()];
            assert_ne!(selected_conversation(&p.readback, 45).as_deref(), Ok("a"));
        }
        let mut p = fake();
        p.readback.selected_urls = vec!["https://127.0.0.1:45/c/a?project=workspace".into()];
        assert_eq!(selected_conversation(&p.readback, 45).as_deref(), Ok("a"));
    }

    #[test]
    fn codex_navigation_polls_until_verified_and_respects_lease_and_retirement() {
        use crate::codex_sessions::{test_support, CodexWindowSnapshot};
        let dir = tempfile::TempDir::new().unwrap();
        test_support::create_codex_fixture(dir.path());

        let mut opened = Vec::new();
        let mut polls = 0usize;
        navigate_codex_session_with(
            dir.path(),
            test_support::UUID_A,
            "local",
            || Ok(()),
            || true,
            |url| {
                opened.push(url.to_owned());
                Ok(())
            },
            || {
                polls += 1;
                if polls == 1 {
                    Ok(CodexWindowSnapshot {
                        window_title: "ChatGPT".into(),
                        toolbar_project: None,
                        active_sidebar_texts: vec![],
                        main_pane_texts: vec![],
                        has_resume_error: false,
                    })
                } else {
                    Ok(CodexWindowSnapshot {
                        window_title: "Same Title".into(),
                        toolbar_project: Some("micro-input".into()),
                        active_sidebar_texts: vec!["Same Title".into()],
                        main_pane_texts: vec!["Alpha unique reply from thread A".into()],
                        has_resume_error: false,
                    })
                }
            },
            Duration::from_millis(200),
            Duration::from_millis(1),
        )
        .unwrap();
        assert_eq!(
            opened,
            vec![format!("codex://threads/{}", test_support::UUID_A)]
        );
        assert_eq!(polls, 3);

        let mut opened_on_stale = false;
        let err = navigate_codex_session_with(
            dir.path(),
            test_support::UUID_A,
            "local",
            || Err("blocked:session_input_retired".into()),
            || true,
            |_| {
                opened_on_stale = true;
                Ok(())
            },
            || unreachable!("should not poll when lease fails"),
            Duration::from_millis(50),
            Duration::from_millis(1),
        )
        .unwrap_err();
        assert_eq!(err, "blocked:session_input_retired");
        assert!(!opened_on_stale);

        let err_sibling = navigate_codex_session_with(
            dir.path(),
            test_support::UUID_A,
            "local",
            || Ok(()),
            || true,
            |_| Ok(()),
            || {
                Ok(CodexWindowSnapshot {
                    window_title: "Same Title".into(),
                    toolbar_project: Some("micro-input".into()),
                    active_sidebar_texts: vec![],
                    main_pane_texts: vec!["Beta conflicting sibling reply".into()],
                    has_resume_error: false,
                })
            },
            Duration::from_millis(20),
            Duration::from_millis(1),
        )
        .unwrap_err();
        assert!(err_sibling.contains("codex_conflicting_sibling_detected"));
    }

    #[test]
    fn codex_navigation_retries_transient_second_readback_failure_before_deadline() {
        use crate::codex_sessions::{test_support, CodexWindowSnapshot};
        let dir = tempfile::TempDir::new().unwrap();
        test_support::create_codex_fixture(dir.path());

        let good = || CodexWindowSnapshot {
            window_title: "Same Title".into(),
            toolbar_project: Some("micro-input".into()),
            active_sidebar_texts: vec!["Same Title".into()],
            main_pane_texts: vec!["Alpha unique reply from thread A".into()],
            has_resume_error: false,
        };
        let mut polls = 0usize;
        navigate_codex_session_with(
            dir.path(),
            test_support::UUID_A,
            "local",
            || Ok(()),
            || true,
            |_| Ok(()),
            || {
                polls += 1;
                if polls == 2 {
                    Err("blocked:ax_snapshot_timeout".into())
                } else {
                    Ok(good())
                }
            },
            Duration::from_millis(200),
            Duration::from_millis(1),
        )
        .unwrap();
        assert_eq!(polls, 4);
    }
}
