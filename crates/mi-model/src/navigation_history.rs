use serde::{Deserialize, Serialize};

/// Maximum number of entries retained in the keypad cross-agent navigation stack,
/// matching `paseo-creator-micro` (`MAX_HISTORY = 50`).
pub const MAX_HISTORY: usize = 50;

/// A target place opened or focused via the keypad across desktop agents, apps, and sessions.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NavigationPlace {
    App {
        bundle_id: String,
    },
    Session {
        adapter: String,
        session: String,
    },
    Antigravity {
        conversation_id: String,
        workspace_path: String,
    },
    Cindy {
        workspace_path: String,
        session_id: String,
    },
    ZCode {
        source_server: String,
        remote_session: String,
        workspace: String,
        task_id: String,
    },
    Dsh {
        session_id: String,
        workspace_id: String,
        host_id: String,
    },
}

impl NavigationPlace {
    pub fn app(bundle_id: impl Into<String>) -> Self {
        Self::App {
            bundle_id: bundle_id.into(),
        }
    }

    pub fn session(adapter: impl Into<String>, session: impl Into<String>) -> Self {
        Self::Session {
            adapter: adapter.into(),
            session: session.into(),
        }
    }

    pub fn antigravity(
        conversation_id: impl Into<String>,
        workspace_path: impl Into<String>,
    ) -> Self {
        Self::Antigravity {
            conversation_id: conversation_id.into(),
            workspace_path: workspace_path.into(),
        }
    }

    pub fn cindy(workspace_path: impl Into<String>, session_id: impl Into<String>) -> Self {
        Self::Cindy {
            workspace_path: workspace_path.into(),
            session_id: session_id.into(),
        }
    }

    pub fn zcode(
        source_server: impl Into<String>,
        remote_session: impl Into<String>,
        workspace: impl Into<String>,
        task_id: impl Into<String>,
    ) -> Self {
        Self::ZCode {
            source_server: source_server.into(),
            remote_session: remote_session.into(),
            workspace: workspace.into(),
            task_id: task_id.into(),
        }
    }

    pub fn dsh(
        session_id: impl Into<String>,
        workspace_id: impl Into<String>,
        host_id: impl Into<String>,
    ) -> Self {
        Self::Dsh {
            session_id: session_id.into(),
            workspace_id: workspace_id.into(),
            host_id: host_id.into(),
        }
    }

    pub fn target_id(&self) -> String {
        match self {
            Self::App { bundle_id } => bundle_id.clone(),
            Self::Session { adapter, session } => format!("{adapter}:{session}"),
            Self::Antigravity {
                conversation_id,
                workspace_path,
            } => format!("antigravity:{conversation_id}@{workspace_path}"),
            Self::Cindy {
                workspace_path,
                session_id,
            } => format!("cindy:{session_id}@{workspace_path}"),
            Self::ZCode {
                source_server,
                remote_session,
                workspace,
                task_id,
            } => format!("zcode:{source_server}:{remote_session}:{workspace}:{task_id}"),
            Self::Dsh {
                session_id,
                workspace_id,
                host_id,
            } => format!("dsh:{host_id}:{workspace_id}:{session_id}"),
        }
    }
}

/// Browser-like back/forward history stack for targets opened from the keypad.
///
/// Semantics match `paseo-creator-micro/Plugin/server/history.ts`:
/// - `visit` appends a newly opened target, truncating any forward branch when stepping
///   off a historical position.
/// - Re-visiting the current target is a no-op and preserves the forward stack.
/// - `back` and `forward` move the cursor without recording a new visit, skipping
///   targets that are currently unavailable.
/// - Retains at most [`MAX_HISTORY`] (50) entries, evicting the oldest entries first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavigationHistory {
    entries: Vec<NavigationPlace>,
    index: Option<usize>,
}

impl NavigationHistory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn visit(&mut self, place: NavigationPlace) {
        if self.current() == Some(&place) {
            return;
        }
        if let Some(current) = self.index {
            self.entries.truncate(current + 1);
        } else {
            self.entries.clear();
        }
        self.entries.push(place);
        if self.entries.len() > MAX_HISTORY {
            let overflow = self.entries.len() - MAX_HISTORY;
            self.entries.drain(0..overflow);
        }
        self.index = Some(self.entries.len() - 1);
    }

    pub fn back<F>(&mut self, mut available: F) -> Option<NavigationPlace>
    where
        F: FnMut(&NavigationPlace) -> bool,
    {
        let current = self.index?;
        for candidate in (0..current).rev() {
            if available(&self.entries[candidate]) {
                self.index = Some(candidate);
                return Some(self.entries[candidate].clone());
            }
        }
        None
    }

    pub fn forward<F>(&mut self, mut available: F) -> Option<NavigationPlace>
    where
        F: FnMut(&NavigationPlace) -> bool,
    {
        let current = self.index?;
        for candidate in (current + 1)..self.entries.len() {
            if available(&self.entries[candidate]) {
                self.index = Some(candidate);
                return Some(self.entries[candidate].clone());
            }
        }
        None
    }

    pub fn can_go_back<F>(&self, mut available: F) -> bool
    where
        F: FnMut(&NavigationPlace) -> bool,
    {
        let Some(current) = self.index else {
            return false;
        };
        (0..current).rev().any(|i| available(&self.entries[i]))
    }

    pub fn can_go_forward<F>(&self, mut available: F) -> bool
    where
        F: FnMut(&NavigationPlace) -> bool,
    {
        let Some(current) = self.index else {
            return false;
        };
        ((current + 1)..self.entries.len()).any(|i| available(&self.entries[i]))
    }

    pub fn current(&self) -> Option<&NavigationPlace> {
        self.index.and_then(|i| self.entries.get(i))
    }

    pub fn index(&self) -> Option<usize> {
        self.index
    }

    pub fn entries(&self) -> &[NavigationPlace] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{NavigationHistory, NavigationPlace, MAX_HISTORY};

    #[test]
    fn moves_back_and_forward_across_cross_agent_places_without_losing_forward_stack() {
        let mut history = NavigationHistory::new();
        let herdr = NavigationPlace::app("dev.herdr.app");
        let antigravity = NavigationPlace::app("com.google.antigravity");
        let qwen = NavigationPlace::app("cn.qwenwork.desktop.mac");
        let cindy = NavigationPlace::app("com.xd.cindy");
        let codex = NavigationPlace::app("com.openai.codex");

        assert!(!history.can_go_back(|_| true));
        assert!(!history.can_go_forward(|_| true));
        assert_eq!(history.back(|_| true), None);
        assert_eq!(history.forward(|_| true), None);

        for place in [&herdr, &antigravity, &qwen, &cindy, &codex] {
            history.visit(place.clone());
        }
        assert_eq!(history.current(), Some(&codex));
        assert!(history.can_go_back(|_| true));
        assert!(!history.can_go_forward(|_| true));

        assert_eq!(history.back(|_| true), Some(cindy.clone()));
        assert_eq!(history.back(|_| true), Some(qwen.clone()));
        assert_eq!(history.back(|_| true), Some(antigravity.clone()));
        assert_eq!(history.back(|_| true), Some(herdr.clone()));
        assert_eq!(history.back(|_| true), None);
        assert!(!history.can_go_back(|_| true));
        assert!(history.can_go_forward(|_| true));

        assert_eq!(history.forward(|_| true), Some(antigravity));
        assert_eq!(history.forward(|_| true), Some(qwen));
        assert_eq!(history.forward(|_| true), Some(cindy));
        assert_eq!(history.forward(|_| true), Some(codex));
        assert_eq!(history.forward(|_| true), None);
    }

    #[test]
    fn deduplicates_consecutive_visits_and_preserves_forward_stack_on_current_revisit() {
        let mut history = NavigationHistory::new();
        let herdr = NavigationPlace::app("dev.herdr.app");
        let cindy = NavigationPlace::app("com.xd.cindy");
        let codex = NavigationPlace::app("com.openai.codex");

        history.visit(herdr.clone());
        history.visit(herdr.clone());
        assert_eq!(history.entries(), &[herdr.clone()]);

        history.visit(cindy.clone());
        history.visit(codex.clone());
        assert_eq!(history.back(|_| true), Some(cindy.clone()));

        // Re-visiting the current place after stepping back must NOT drop the forward stack.
        history.visit(cindy);
        assert!(history.can_go_forward(|_| true));
        assert_eq!(history.forward(|_| true), Some(codex));
    }

    #[test]
    fn truncates_forward_branch_when_visiting_a_new_place_after_stepping_back() {
        let mut history = NavigationHistory::new();
        let herdr = NavigationPlace::app("dev.herdr.app");
        let antigravity = NavigationPlace::app("com.google.antigravity");
        let cindy = NavigationPlace::app("com.xd.cindy");
        let qwen = NavigationPlace::app("cn.qwenwork.desktop.mac");

        history.visit(herdr.clone());
        history.visit(antigravity.clone());
        history.visit(cindy);

        assert_eq!(history.back(|_| true), Some(antigravity.clone()));
        history.visit(qwen.clone());

        assert!(!history.can_go_forward(|_| true));
        assert_eq!(history.forward(|_| true), None);
        assert_eq!(history.entries(), &[herdr, antigravity.clone(), qwen]);
        assert_eq!(history.back(|_| true), Some(antigravity));
    }

    #[test]
    fn skips_unavailable_places_in_both_directions_without_corrupting_cursor() {
        let mut history = NavigationHistory::new();
        let herdr = NavigationPlace::app("dev.herdr.app");
        let stale_thread = NavigationPlace::session("herdr", "session-stale");
        let codex = NavigationPlace::app("com.openai.codex");

        history.visit(herdr.clone());
        history.visit(stale_thread.clone());
        history.visit(codex.clone());

        let is_available = |place: &NavigationPlace| place != &stale_thread;
        assert_eq!(history.back(is_available), Some(herdr.clone()));
        assert_eq!(history.back(is_available), None);
        assert_eq!(history.current(), Some(&herdr));

        assert_eq!(history.forward(is_available), Some(codex.clone()));
        assert_eq!(history.forward(is_available), None);
        assert_eq!(history.current(), Some(&codex));

        // When no earlier place is available, cursor stays put.
        assert!(!history.can_go_back(|_| false));
        assert_eq!(history.back(|_| false), None);
        assert_eq!(history.current(), Some(&codex));
    }

    #[test]
    fn caps_history_length_at_max_history_50_entries() {
        let mut history = NavigationHistory::new();
        for i in 0..60 {
            history.visit(NavigationPlace::session("herdr", format!("session-{i}")));
        }
        assert_eq!(history.len(), MAX_HISTORY);
        assert_eq!(
            history.entries().first(),
            Some(&NavigationPlace::session("herdr", "session-10"))
        );
        assert_eq!(
            history.current(),
            Some(&NavigationPlace::session("herdr", "session-59"))
        );
        assert_eq!(history.index(), Some(MAX_HISTORY - 1));
    }

    #[test]
    fn four_app_places_disambiguate_same_name_across_different_workspaces_and_instances() {
        let mut history = NavigationHistory::new();
        // Antigravity: same conversation ID in two different workspaces
        let ag_ws1 = NavigationPlace::antigravity("conv-1", "/path/workspace-alpha");
        let ag_ws2 = NavigationPlace::antigravity("conv-1", "/path/workspace-beta");
        assert_ne!(ag_ws1, ag_ws2);
        assert_ne!(ag_ws1.target_id(), ag_ws2.target_id());

        // Cindy: same session ID in two different workspaces
        let cindy_ws1 = NavigationPlace::cindy("/path/workspace-alpha", "sess-1");
        let cindy_ws2 = NavigationPlace::cindy("/path/workspace-beta", "sess-1");
        assert_ne!(cindy_ws1, cindy_ws2);
        assert_ne!(cindy_ws1.target_id(), cindy_ws2.target_id());

        // ZCode: same task ID in two different workspaces/remote sessions
        let zcode_ws1 = NavigationPlace::zcode("srv1", "rem1", "/path/workspace-alpha", "task-1");
        let zcode_ws2 = NavigationPlace::zcode("srv1", "rem1", "/path/workspace-beta", "task-1");
        assert_ne!(zcode_ws1, zcode_ws2);
        assert_ne!(zcode_ws1.target_id(), zcode_ws2.target_id());

        // DSH: same session ID across two different workspaces or hosts
        let dsh_ws1 = NavigationPlace::dsh("sess-1", "workspace-alpha", "host-1");
        let dsh_ws2 = NavigationPlace::dsh("sess-1", "workspace-beta", "host-1");
        let dsh_host2 = NavigationPlace::dsh("sess-1", "workspace-alpha", "host-2");
        assert_ne!(dsh_ws1, dsh_ws2);
        assert_ne!(dsh_ws1, dsh_host2);
        assert_ne!(dsh_ws1.target_id(), dsh_ws2.target_id());
        assert_ne!(dsh_ws1.target_id(), dsh_host2.target_id());

        // Visiting each place records distinct history entries without collision
        for p in [
            &ag_ws1, &ag_ws2, &cindy_ws1, &cindy_ws2, &zcode_ws1, &zcode_ws2, &dsh_ws1, &dsh_ws2,
        ] {
            history.visit((*p).clone());
        }
        assert_eq!(history.len(), 8);
        assert_eq!(history.current(), Some(&dsh_ws2));
        assert_eq!(history.back(|_| true), Some(dsh_ws1));
        assert_eq!(history.back(|_| true), Some(zcode_ws2));
        assert_eq!(history.back(|_| true), Some(zcode_ws1));
        assert_eq!(history.back(|_| true), Some(cindy_ws2));
        assert_eq!(history.back(|_| true), Some(cindy_ws1));
        assert_eq!(history.back(|_| true), Some(ag_ws2));
        assert_eq!(history.back(|_| true), Some(ag_ws1));
    }
}
