mod readback;
mod sqlite_ro;

pub use readback::{
    build_codex_thread_deeplink, read_codex_thread_fingerprint, verify_codex_window_readback,
};
#[cfg(test)]
use readback::{MAX_ITEM_JSON_BYTES, MAX_THREAD_CORPUS_BYTES, MAX_THREAD_SNIPPETS};
#[cfg(test)]
use sqlite_ro::{
    checked_sqlite_text_len, sqlite3, sqlite3_exec, sqlite3_free, sqlite3_open_v2, SQLITE_OK,
};
use sqlite_ro::{validate_codex_dir, SqliteDb};
use std::collections::{BTreeMap, BTreeSet};
#[cfg(test)]
use std::ffi::{c_char, c_int, CStr, CString};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodexObservedStatus {
    NeedsAttention,
    Working,
    Error,
    Unread,
    Idle,
    Unknown(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodexLiveWindowSignals {
    pub is_frontmost: bool,
    pub toolbar_project: Option<String>,
    pub active_sidebar_texts: Vec<String>,
    pub attention_sidebar_titles: Vec<String>,
    pub attention_windows: Vec<(Option<String>, Vec<String>)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexThreadObservation {
    pub thread_id: String,
    pub host_id: String,
    pub display_title: String,
    pub workspace_path: String,
    pub status: CodexObservedStatus,
    pub updated_at_ms: u64,
    pub watermark: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexThreadFingerprint {
    pub thread_id: String,
    pub host_id: String,
    pub display_title: String,
    pub project_name: Option<String>,
    pub target_snippets: Vec<String>,
    pub conflicting_sibling_snippets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexWindowSnapshot {
    pub window_title: String,
    pub toolbar_project: Option<String>,
    pub active_sidebar_texts: Vec<String>,
    pub main_pane_texts: Vec<String>,
    pub has_resume_error: bool,
}

fn is_valid_uuid(id: &str) -> bool {
    let bytes = id.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    bytes.iter().enumerate().all(|(idx, byte)| {
        if matches!(idx, 8 | 13 | 18 | 23) {
            *byte == b'-'
        } else {
            byte.is_ascii_hexdigit()
        }
    })
}

fn timestamp_to_ms(raw: f64) -> u64 {
    if !raw.is_finite() || raw <= 0.0 {
        return 0;
    }
    if raw >= 1_000_000_000_000.0 {
        raw.round() as u64
    } else {
        (raw * 1000.0).round() as u64
    }
}

fn resolve_root_thread_id<'a>(
    start_id: &'a str,
    parent_map: &'a BTreeMap<String, String>,
) -> &'a str {
    let mut visited = BTreeSet::new();
    visited.insert(start_id);
    let mut current = start_id;
    for _ in 0..8 {
        let Some(parent) = parent_map.get(current).map(String::as_str) else {
            break;
        };
        if !visited.insert(parent) {
            break;
        }
        current = parent;
    }
    current
}

struct CodexDbs {
    codex: SqliteDb,
    state: SqliteDb,
    history: SqliteDb,
}

impl CodexDbs {
    fn open(codex_home: &Path) -> Result<Self, String> {
        validate_codex_dir(codex_home)?;
        let sqlite_dir = codex_home.join("sqlite");
        validate_codex_dir(&sqlite_dir)?;
        Ok(Self {
            codex: SqliteDb::open_readonly(&sqlite_dir.join("codex.db"))?,
            state: SqliteDb::open_readonly(&codex_home.join("state_5.sqlite"))?,
            history: SqliteDb::open_readonly(&codex_home.join("thread_history_1.sqlite"))?,
        })
    }

    fn is_thread_archived(&self, thread_id: &str) -> Result<bool, String> {
        let mut stmt = self
            .state
            .prepare("SELECT archived FROM threads WHERE id = ?1 LIMIT 1")?;
        stmt.bind_text(1, thread_id)?;
        Ok(stmt.step()? && stmt.col_int64(0) != 0)
    }

    fn load_parent_hierarchy(
        &self,
    ) -> Result<(BTreeSet<String>, BTreeMap<String, String>), String> {
        let mut child_ids = BTreeSet::new();
        let mut parent_map = BTreeMap::new();
        for (child, parent) in self.state.query_optional_text_pairs(
            "SELECT id, attached_thread_parent_id FROM threads WHERE attached_thread_parent_id IS NOT NULL AND attached_thread_parent_id != '' LIMIT 4096",
        )? {
            child_ids.insert(child.clone());
            if child != parent {
                parent_map.insert(child, parent);
            }
        }
        for (child, parent) in self.state.query_optional_text_pairs(
            "SELECT child_thread_id, parent_thread_id FROM thread_spawn_edges LIMIT 4096",
        )? {
            child_ids.insert(child.clone());
            if child != parent {
                parent_map.entry(child).or_insert(parent);
            }
        }
        Ok((child_ids, parent_map))
    }

    fn load_unread_receipts(&self) -> Result<BTreeMap<String, String>, String> {
        let mut stmt = self.state.prepare(
            "SELECT thread_id, first_unread_turn FROM thread_read_receipts WHERE first_unread_turn IS NOT NULL AND first_unread_turn != '' ORDER BY rowid DESC LIMIT 4096",
        )?;
        let mut map = BTreeMap::new();
        while stmt.step()? {
            let thread_id = stmt.col_text(0)?;
            if let Some(turn_id) = stmt.col_opt_text(1)?.filter(|s| !s.is_empty()) {
                map.insert(thread_id, turn_id);
            }
        }
        Ok(map)
    }

    fn load_raw_priority_turn_ids(&self) -> Result<BTreeSet<String>, String> {
        let mut ids = self.history.query_text_set(
            "SELECT thread_id FROM thread_turns WHERE status = 'inProgress' GROUP BY thread_id ORDER BY MAX(rowid) DESC LIMIT 256",
        )?;
        ids.extend(self.history.query_text_set(
            "SELECT thread_id FROM thread_turns WHERE status = 'failed' GROUP BY thread_id ORDER BY MAX(rowid) DESC LIMIT 64",
        )?);
        Ok(ids)
    }

    #[cfg(test)]
    fn load_priority_turn_ids(&self) -> Result<BTreeSet<String>, String> {
        let (_, parent_map) = self.load_parent_hierarchy()?;
        let raw = self.load_raw_priority_turn_ids()?;
        Ok(expand_priority_ids_with_roots(&raw, &parent_map))
    }

    fn latest_turn(
        &self,
        thread_id: &str,
    ) -> Result<Option<(String, String, Option<String>)>, String> {
        let mut stmt = self.history.prepare(
            "SELECT turn_id, status, error_json FROM thread_turns WHERE thread_id = ?1 ORDER BY rollout_ordinal DESC LIMIT 1",
        )?;
        stmt.bind_text(1, thread_id)?;
        if stmt.step()? {
            Ok(Some((
                stmt.col_text(0)?,
                stmt.col_text(1)?,
                stmt.col_opt_text(2)?,
            )))
        } else {
            Ok(None)
        }
    }
}

fn expand_priority_ids_with_roots(
    raw_ids: &BTreeSet<String>,
    parent_map: &BTreeMap<String, String>,
) -> BTreeSet<String> {
    let mut expanded = raw_ids.clone();
    for id in raw_ids {
        let root = resolve_root_thread_id(id, parent_map);
        if root != id {
            expanded.insert(root.to_owned());
        }
    }
    expanded
}

fn map_codex_status(
    in_active_chat_turns: bool,
    latest_turn: Option<&(String, String, Option<String>)>,
    first_unread_turn: Option<&str>,
    automation_pending: bool,
) -> CodexObservedStatus {
    let turn_status = latest_turn.map(|(_, status, _)| status.as_str());
    if in_active_chat_turns || turn_status == Some("inProgress") {
        return CodexObservedStatus::Working;
    }
    let has_unread_signal = first_unread_turn.is_some() || automation_pending;
    match turn_status {
        Some("failed") => {
            if has_unread_signal {
                CodexObservedStatus::Error
            } else {
                CodexObservedStatus::Idle
            }
        }
        Some("completed" | "interrupted") => {
            if has_unread_signal {
                CodexObservedStatus::Unread
            } else {
                CodexObservedStatus::Idle
            }
        }
        Some(other) => CodexObservedStatus::Unknown(other.to_owned()),
        None => CodexObservedStatus::Unknown("missing_turn".to_owned()),
    }
}

fn status_priority_rank(status: &CodexObservedStatus) -> u8 {
    match status {
        CodexObservedStatus::NeedsAttention => 0,
        CodexObservedStatus::Error => 1,
        CodexObservedStatus::Unread => 2,
        CodexObservedStatus::Working => 3,
        CodexObservedStatus::Idle => 4,
        CodexObservedStatus::Unknown(_) => 5,
    }
}

fn sort_codex_observations(observations: &mut [CodexThreadObservation]) {
    observations.sort_by(|a, b| {
        status_priority_rank(&a.status)
            .cmp(&status_priority_rank(&b.status))
            .then_with(|| b.updated_at_ms.cmp(&a.updated_at_ms))
    });
}

fn workspace_basename(workspace_path: &str) -> Option<&str> {
    let trimmed = workspace_path.trim();
    if trimmed.is_empty() {
        return None;
    }
    Path::new(trimmed)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
}

fn workspace_matches_toolbar_project(workspace_path: &str, toolbar_project: Option<&str>) -> bool {
    let Some(proj) = toolbar_project.map(str::trim).filter(|s| !s.is_empty()) else {
        return true;
    };
    match workspace_basename(workspace_path) {
        Some(base) => base.eq_ignore_ascii_case(proj),
        None => true,
    }
}

pub fn apply_codex_window_signals(
    observations: &mut [CodexThreadObservation],
    signals: &CodexLiveWindowSignals,
) -> Option<(String, String, String, String)> {
    let fallback_window = [(
        signals.toolbar_project.as_deref(),
        signals.attention_sidebar_titles.as_slice(),
    )];
    let explicit_windows: Vec<(Option<&str>, &[String])> = signals
        .attention_windows
        .iter()
        .map(|(proj, titles)| (proj.as_deref(), titles.as_slice()))
        .collect();
    let windows: &[(Option<&str>, &[String])] = if explicit_windows.is_empty() {
        &fallback_window
    } else {
        &explicit_windows
    };

    let mut promote_indices = Vec::new();
    for (idx, obs) in observations.iter().enumerate() {
        if obs.status != CodexObservedStatus::Working {
            continue;
        }
        let title = obs.display_title.trim();
        if title.is_empty() {
            continue;
        }
        let matched = windows.iter().any(|(win_proj, titles)| {
            titles.iter().any(|candidate| candidate.trim() == title)
                && workspace_matches_toolbar_project(&obs.workspace_path, *win_proj)
                && observations
                    .iter()
                    .filter(|peer| {
                        peer.status == CodexObservedStatus::Working
                            && peer.display_title.trim() == title
                            && workspace_matches_toolbar_project(&peer.workspace_path, *win_proj)
                    })
                    .count()
                    == 1
        });
        if matched {
            promote_indices.push(idx);
        }
    }

    let mut status_mutated = false;
    for idx in promote_indices {
        let obs = &mut observations[idx];
        obs.status = CodexObservedStatus::NeedsAttention;
        if obs.watermark.is_empty() {
            obs.watermark = format!("{}:attention", obs.updated_at_ms);
        } else if !obs.watermark.ends_with(":attention") {
            obs.watermark.push_str(":attention");
        }
        status_mutated = true;
    }

    let mut frontmost_ack = None;
    if signals.is_frontmost {
        let toolbar_proj = signals
            .toolbar_project
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let ack_matches: Vec<usize> = observations
            .iter()
            .enumerate()
            .filter_map(|(idx, obs)| {
                if !matches!(
                    obs.status,
                    CodexObservedStatus::Unread | CodexObservedStatus::Error
                ) {
                    return None;
                }
                let title = obs.display_title.trim();
                if title.is_empty() {
                    return None;
                }
                let in_active_sidebar = signals
                    .active_sidebar_texts
                    .iter()
                    .any(|text| text.trim() == title);
                (in_active_sidebar
                    && workspace_matches_toolbar_project(&obs.workspace_path, toolbar_proj))
                .then_some(idx)
            })
            .collect();
        if let [idx] = ack_matches.as_slice() {
            let target_title = observations[*idx].display_title.trim();
            let total_matching = observations
                .iter()
                .filter(|obs| {
                    obs.display_title.trim() == target_title
                        && workspace_matches_toolbar_project(&obs.workspace_path, toolbar_proj)
                })
                .count();
            if total_matching == 1 {
                let obs = &mut observations[*idx];
                let watermark = if obs.watermark.is_empty() {
                    obs.updated_at_ms.to_string()
                } else {
                    obs.watermark.clone()
                };
                frontmost_ack = Some((
                    obs.thread_id.clone(),
                    obs.host_id.clone(),
                    obs.workspace_path.trim().to_owned(),
                    watermark,
                ));
                obs.status = CodexObservedStatus::Idle;
                status_mutated = true;
            }
        }
    }

    if status_mutated {
        sort_codex_observations(observations);
    }
    frontmost_ack
}

pub fn observe_codex_threads(
    codex_home: &Path,
    limit: usize,
) -> Result<Vec<CodexThreadObservation>, String> {
    let dbs = CodexDbs::open(codex_home)?;
    if limit == 0 {
        return Ok(Vec::new());
    }
    let archived_ids = dbs.state.query_text_set(
        "SELECT id FROM threads WHERE archived != 0 ORDER BY rowid DESC LIMIT 4096",
    )?;
    let (child_thread_ids, parent_map) = dbs.load_parent_hierarchy()?;
    let active_turn_ids = dbs.codex.query_text_set(
        "SELECT thread_id FROM active_chat_turns GROUP BY thread_id ORDER BY MAX(rowid) DESC LIMIT 256",
    )?;
    let pending_automation_ids = dbs.codex.query_text_set(
        "SELECT thread_id FROM automation_runs WHERE status = 'PENDING_REVIEW' AND read_at IS NULL GROUP BY thread_id ORDER BY MAX(rowid) DESC LIMIT 256",
    )?;
    let unread_receipts = dbs.load_unread_receipts()?;
    let raw_priority_turn_ids = dbs.load_raw_priority_turn_ids()?;
    let priority_turn_ids = expand_priority_ids_with_roots(&raw_priority_turn_ids, &parent_map);

    let mut child_rollups: BTreeMap<String, (CodexObservedStatus, String)> = BTreeMap::new();
    for cid in &child_thread_ids {
        if !is_valid_uuid(cid)
            || archived_ids.contains(cid)
            || (!active_turn_ids.contains(cid) && !raw_priority_turn_ids.contains(cid))
        {
            continue;
        }
        let root_id = resolve_root_thread_id(cid, &parent_map);
        if root_id == cid || child_thread_ids.contains(root_id) || archived_ids.contains(root_id) {
            continue;
        }
        let latest_turn = dbs.latest_turn(cid)?;
        let child_status = map_codex_status(
            active_turn_ids.contains(cid),
            latest_turn.as_ref(),
            unread_receipts.get(cid).map(String::as_str),
            pending_automation_ids.contains(cid),
        );
        let rolled = match child_status {
            CodexObservedStatus::Working => Some((CodexObservedStatus::Working, ":subwork".into())),
            CodexObservedStatus::Error => {
                let child_turn = latest_turn
                    .as_ref()
                    .map(|(id, _, _)| id.as_str())
                    .unwrap_or(cid.as_str());
                Some((
                    CodexObservedStatus::Error,
                    format!(":suberror:{child_turn}"),
                ))
            }
            _ => None,
        };
        if let Some((status, suffix)) = rolled {
            let replace = child_rollups.get(root_id).is_none_or(|(existing, _)| {
                status_priority_rank(&status) < status_priority_rank(existing)
            });
            if replace {
                child_rollups.insert(root_id.to_owned(), (status, suffix));
            }
        }
    }

    let mut catalog = dbs.codex.prepare(
        "SELECT c.thread_id, c.host_id, c.display_title, COALESCE(c.cwd, ''), \
         COALESCE(c.source_updated_at, c.source_recency_at, 0) \
         FROM local_thread_catalog c \
         LEFT JOIN local_thread_catalog_hosts h ON c.host_id = h.host_id \
         WHERE c.missing_candidate = 0 \
           AND COALESCE(h.host_kind, '') != 'chatgpt' \
           AND COALESCE(c.source_kind, '') != 'chatgpt' \
         ORDER BY COALESCE(c.source_updated_at, c.source_recency_at, 0) DESC LIMIT 512",
    )?;

    let mut observations = Vec::with_capacity(limit.min(64));
    while catalog.step()? {
        let thread_id = catalog.col_text(0)?;
        if !is_valid_uuid(&thread_id)
            || archived_ids.contains(&thread_id)
            || child_thread_ids.contains(&thread_id)
        {
            continue;
        }
        let in_active_turns = active_turn_ids.contains(&thread_id);
        let automation_pending = pending_automation_ids.contains(&thread_id);
        let first_unread_turn = unread_receipts.get(&thread_id).map(String::as_str);
        let has_child_rollup = child_rollups.contains_key(&thread_id);
        let is_priority = in_active_turns
            || automation_pending
            || first_unread_turn.is_some()
            || priority_turn_ids.contains(&thread_id)
            || has_child_rollup;
        if observations.len() >= limit && !is_priority {
            continue;
        }
        let host_id = catalog.col_text(1)?;
        let display_title = catalog.col_text(2)?;
        let workspace_path = catalog.col_text(3)?;
        let updated_at_ms = timestamp_to_ms(catalog.col_double(4));

        let latest_turn = dbs.latest_turn(&thread_id)?;
        let mut status = map_codex_status(
            in_active_turns,
            latest_turn.as_ref(),
            first_unread_turn,
            automation_pending,
        );
        let turn_id = latest_turn
            .as_ref()
            .map(|(id, _, _)| id.as_str())
            .unwrap_or("");
        let unread_str = first_unread_turn.unwrap_or("");
        let mut watermark = format!("{turn_id}:{unread_str}:{updated_at_ms}");
        if let Some((rolled_status, rolled_suffix)) = child_rollups.get(&thread_id) {
            if status_priority_rank(rolled_status) < status_priority_rank(&status) {
                status = rolled_status.clone();
                watermark.push_str(rolled_suffix);
            }
        }

        observations.push(CodexThreadObservation {
            thread_id,
            host_id,
            display_title,
            workspace_path,
            status,
            updated_at_ms,
            watermark,
        });
    }
    sort_codex_observations(&mut observations);
    observations.truncate(limit);
    Ok(observations)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use tests::test_support;
