use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionNavigationWindow {
    pub pid: i32,
    pub window: u64,
    pub links: Vec<(u64, String)>,
    pub selected_urls: Vec<String>,
}

pub trait SessionNavigationProvider {
    fn capture(&mut self, pid: i32) -> Result<SessionNavigationWindow, String>;
    fn read(&mut self, target: &SessionNavigationWindow)
        -> Result<SessionNavigationWindow, String>;
    fn press(
        &mut self,
        target: &SessionNavigationWindow,
        link: u64,
        is_current: &dyn Fn() -> bool,
    ) -> Result<(), String>;
}

fn app_document_path(url: &str, expected_port: u16) -> Option<&str> {
    let rest = url.strip_prefix("https://")?;
    let (host, path) = rest.split_once('/')?;
    let port = host.strip_prefix("127.0.0.1:")?.parse::<u16>().ok()?;
    (port != 0 && port == expected_port).then_some(path)
}

fn conversation_url(url: &str, conversation: &str, expected_port: u16) -> bool {
    app_document_path(url, expected_port).is_some_and(|path| {
        path.split(['?', '#']).next() == Some(format!("c/{conversation}").as_str())
    })
}

fn selected_conversation(window: &SessionNavigationWindow, port: u16) -> Result<String, String> {
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
    let links: Vec<_> = window
        .links
        .iter()
        .filter(|(_, url)| conversation_url(url, conversation, port))
        .collect();
    if links.len() != 1 {
        return Err("blocked:session_link_unavailable_or_ambiguous".into());
    }
    let (link, _) = links[0];
    validate_lease()?;
    provider.press(&window, *link, &is_current)?;
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

pub fn navigate_antigravity_session(
    pid: i32,
    port: u16,
    conversation: &str,
    validate_lease: impl FnMut() -> Result<(), String>,
    is_current: impl Fn() -> bool,
) -> Result<(), String> {
    navigate_session(
        &mut crate::native_ax::NativeSessionNavigation::default(),
        pid,
        port,
        conversation,
        validate_lease,
        is_current,
    )
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
        fn press(
            &mut self,
            _: &SessionNavigationWindow,
            _: u64,
            is_current: &dyn Fn() -> bool,
        ) -> Result<(), String> {
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
                links: vec![(8, url.clone())],
                selected_urls: vec![url],
            },
            presses: 0,
            reject: false,
            selection_changed: None,
        }
    }
    #[test]
    fn exact_unique_link_and_same_window_url_readback_are_required() {
        let mut p = fake();
        navigate_session(&mut p, 12, 4321, "target", || Ok(()), || true).unwrap();
        assert_eq!(p.presses, 1);
        let mut p = fake();
        p.reject = true;
        assert!(navigate_session(&mut p, 12, 4321, "target", || Ok(()), || true).is_err());
        let mut p = fake();
        p.readback.links.push(p.readback.links[0].clone());
        assert!(navigate_session(&mut p, 12, 4321, "target", || Ok(()), || true).is_err());
        assert_eq!(p.presses, 0);
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
            assert!(!conversation_url(url, "a", 45));
        }
        assert!(conversation_url("https://127.0.0.1:45/c/a", "a", 45));
    }
}
