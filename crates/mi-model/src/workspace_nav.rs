use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkspaceEntry {
    pub app: String,
    pub workspace_id: String,
    pub display_name: String,
    pub session_count: usize,
}

impl WorkspaceEntry {
    pub fn new(
        app: impl Into<String>,
        workspace_id: impl Into<String>,
        display_name: impl Into<String>,
        session_count: usize,
    ) -> Self {
        Self {
            app: app.into(),
            workspace_id: workspace_id.into(),
            display_name: display_name.into(),
            session_count,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceNavigator {
    workspaces: Vec<WorkspaceEntry>,
    selected_index: Option<usize>,
}

impl WorkspaceNavigator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_workspaces(&mut self, workspaces: Vec<WorkspaceEntry>) {
        let previous_selected = self.current().cloned();
        self.workspaces = workspaces;
        if self.workspaces.is_empty() {
            self.selected_index = None;
            return;
        }
        if let Some(prev) = previous_selected {
            if let Some(idx) = self
                .workspaces
                .iter()
                .position(|w| w.app == prev.app && w.workspace_id == prev.workspace_id)
            {
                self.selected_index = Some(idx);
                return;
            }
        }
        self.selected_index = Some(0);
    }

    pub fn select_next(&mut self) -> Option<&WorkspaceEntry> {
        guard_empty!(self);
        let current = self.selected_index.unwrap_or(0);
        let next = (current + 1) % self.workspaces.len();
        self.selected_index = Some(next);
        self.current()
    }

    pub fn select_previous(&mut self) -> Option<&WorkspaceEntry> {
        guard_empty!(self);
        let current = self.selected_index.unwrap_or(0);
        let prev = if current == 0 {
            self.workspaces.len() - 1
        } else {
            current - 1
        };
        self.selected_index = Some(prev);
        self.current()
    }

    pub fn select_index(&mut self, index: usize) -> Option<&WorkspaceEntry> {
        if index < self.workspaces.len() {
            self.selected_index = Some(index);
            self.current()
        } else {
            None
        }
    }

    pub fn current(&self) -> Option<&WorkspaceEntry> {
        self.selected_index.and_then(|i| self.workspaces.get(i))
    }

    pub fn current_index(&self) -> Option<usize> {
        self.selected_index
    }

    pub fn workspaces(&self) -> &[WorkspaceEntry] {
        &self.workspaces
    }
}

macro_rules! guard_empty {
    ($self:expr) => {
        if $self.workspaces.is_empty() {
            $self.selected_index = None;
            return None;
        }
    };
}
use guard_empty;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigates_workspaces_cyclically_and_preserves_selection() {
        let mut nav = WorkspaceNavigator::new();
        assert_eq!(nav.current(), None);

        let w1 = WorkspaceEntry::new("antigravity", "/qa/ag-1", "AG Alpha", 3);
        let w2 = WorkspaceEntry::new("cindy", "/qa/cindy-1", "Cindy Repo", 5);
        let w3 = WorkspaceEntry::new("dsh", "/qa/dsh-1", "DSH Core", 2);

        nav.set_workspaces(vec![w1.clone(), w2.clone(), w3.clone()]);
        assert_eq!(nav.current(), Some(&w1));
        assert_eq!(nav.current_index(), Some(0));

        assert_eq!(nav.select_next(), Some(&w2));
        assert_eq!(nav.current_index(), Some(1));

        assert_eq!(nav.select_next(), Some(&w3));
        assert_eq!(nav.current_index(), Some(2));

        // Wraps around to w1
        assert_eq!(nav.select_next(), Some(&w1));
        assert_eq!(nav.current_index(), Some(0));

        // Previous wraps to w3
        assert_eq!(nav.select_previous(), Some(&w3));
        assert_eq!(nav.current_index(), Some(2));
    }
}
