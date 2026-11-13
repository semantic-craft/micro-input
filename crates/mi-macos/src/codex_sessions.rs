use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
#[cfg(test)]
use std::ffi::c_void;
use std::ffi::{c_char, c_int, c_uchar, CStr, CString};
use std::path::Path;

const SQLITE_OK: c_int = 0;
const SQLITE_ROW: c_int = 100;
const SQLITE_DONE: c_int = 101;
const SQLITE_NULL: c_int = 5;
const SQLITE_OPEN_READONLY: c_int = 0x0000_0001;
const SQLITE_OPEN_URI: c_int = 0x0000_0040;
const SQLITE_TRANSIENT: isize = -1;
const MAX_SNIPPETS: usize = 8;
const MAX_SNIPPET_CHARS: usize = 80;
const MIN_SUBSTRING_SNIPPET_CHARS: usize = 12;

#[repr(C)]
struct sqlite3 {
    _private: [u8; 0],
}

#[repr(C)]
struct sqlite3_stmt {
    _private: [u8; 0],
}

#[link(name = "sqlite3")]
unsafe extern "C" {
    fn sqlite3_open_v2(
        filename: *const c_char,
        pp_db: *mut *mut sqlite3,
        flags: c_int,
        z_vfs: *const c_char,
    ) -> c_int;
    fn sqlite3_close_v2(db: *mut sqlite3) -> c_int;
    fn sqlite3_busy_timeout(db: *mut sqlite3, ms: c_int) -> c_int;
    fn sqlite3_prepare_v2(
        db: *mut sqlite3,
        z_sql: *const c_char,
        n_byte: c_int,
        pp_stmt: *mut *mut sqlite3_stmt,
        pz_tail: *mut *const c_char,
    ) -> c_int;
    fn sqlite3_finalize(stmt: *mut sqlite3_stmt) -> c_int;
    fn sqlite3_step(stmt: *mut sqlite3_stmt) -> c_int;
    fn sqlite3_bind_text(
        stmt: *mut sqlite3_stmt,
        idx: c_int,
        val: *const c_char,
        n_bytes: c_int,
        destructor: isize,
    ) -> c_int;
    fn sqlite3_bind_int64(stmt: *mut sqlite3_stmt, idx: c_int, val: i64) -> c_int;
    fn sqlite3_column_type(stmt: *mut sqlite3_stmt, i_col: c_int) -> c_int;
    fn sqlite3_column_text(stmt: *mut sqlite3_stmt, i_col: c_int) -> *const c_uchar;
    fn sqlite3_column_bytes(stmt: *mut sqlite3_stmt, i_col: c_int) -> c_int;
    fn sqlite3_column_int64(stmt: *mut sqlite3_stmt, i_col: c_int) -> i64;
    fn sqlite3_column_double(stmt: *mut sqlite3_stmt, i_col: c_int) -> f64;
    fn sqlite3_errmsg(db: *mut sqlite3) -> *const c_char;
    #[cfg(test)]
    fn sqlite3_exec(
        db: *mut sqlite3,
        sql: *const c_char,
        callback: Option<
            unsafe extern "C" fn(*mut c_void, c_int, *mut *mut c_char, *mut *mut c_char) -> c_int,
        >,
        arg: *mut c_void,
        errmsg: *mut *mut c_char,
    ) -> c_int;
    #[cfg(test)]
    fn sqlite3_free(ptr: *mut c_void);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodexObservedStatus {
    Working,
    Error,
    Unread,
    Idle,
    Unknown(String),
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

struct SqliteDb {
    ptr: *mut sqlite3,
}

impl Drop for SqliteDb {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe {
                sqlite3_close_v2(self.ptr);
            }
            self.ptr = std::ptr::null_mut();
        }
    }
}

impl SqliteDb {
    fn open_readonly(path: &Path) -> Result<Self, String> {
        if !path.is_file() {
            return Err(format!("blocked:codex_sqlite_missing:{}", path.display()));
        }
        let c_uri = CString::new(file_uri_for_path(path)?)
            .map_err(|e| format!("blocked:codex_sqlite_path:{e}"))?;
        let mut raw_db: *mut sqlite3 = std::ptr::null_mut();
        let rc = unsafe {
            sqlite3_open_v2(
                c_uri.as_ptr(),
                &mut raw_db,
                SQLITE_OPEN_READONLY | SQLITE_OPEN_URI,
                std::ptr::null(),
            )
        };
        let db = Self { ptr: raw_db };
        if rc != SQLITE_OK {
            return Err(format!(
                "blocked:codex_sqlite_open:{}:{rc}:{}",
                path.display(),
                db.last_error()
            ));
        }
        db.check_rc("busy_timeout", unsafe { sqlite3_busy_timeout(db.ptr, 250) })?;
        Ok(db)
    }

    fn query_text_set(&self, sql: &str) -> Result<BTreeSet<String>, String> {
        let mut stmt = self.prepare(sql)?;
        let mut set = BTreeSet::new();
        while stmt.step()? {
            let value = stmt.col_text(0)?;
            if !value.is_empty() {
                set.insert(value);
            }
        }
        Ok(set)
    }

    fn check_rc(&self, op: &str, rc: c_int) -> Result<(), String> {
        if rc == SQLITE_OK {
            Ok(())
        } else {
            Err(format!(
                "blocked:codex_sqlite_{op}:{rc}:{}",
                self.last_error()
            ))
        }
    }

    fn last_error(&self) -> String {
        if self.ptr.is_null() {
            return "null sqlite handle".to_owned();
        }
        let msg = unsafe { sqlite3_errmsg(self.ptr) };
        if msg.is_null() {
            "unknown sqlite error".to_owned()
        } else {
            unsafe { CStr::from_ptr(msg) }
                .to_string_lossy()
                .into_owned()
        }
    }

    fn prepare(&self, sql: &str) -> Result<SqliteStmt<'_>, String> {
        let c_sql = CString::new(sql).map_err(|e| format!("blocked:codex_sql_invalid:{e}"))?;
        let mut raw_stmt: *mut sqlite3_stmt = std::ptr::null_mut();
        let rc = unsafe {
            sqlite3_prepare_v2(
                self.ptr,
                c_sql.as_ptr(),
                sql.len() as c_int,
                &mut raw_stmt,
                std::ptr::null_mut(),
            )
        };
        self.check_rc("prepare", rc)?;
        if raw_stmt.is_null() {
            return Err(format!(
                "blocked:codex_sqlite_prepare:{rc}:{}",
                self.last_error()
            ));
        }
        Ok(SqliteStmt {
            db: self,
            ptr: raw_stmt,
        })
    }
}

struct SqliteStmt<'a> {
    db: &'a SqliteDb,
    ptr: *mut sqlite3_stmt,
}

impl Drop for SqliteStmt<'_> {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe { sqlite3_finalize(self.ptr) };
            self.ptr = std::ptr::null_mut();
        }
    }
}

impl SqliteStmt<'_> {
    fn bind_text(&mut self, index: c_int, value: &str) -> Result<(), String> {
        let rc = unsafe {
            sqlite3_bind_text(
                self.ptr,
                index,
                value.as_ptr().cast::<c_char>(),
                value.len() as c_int,
                SQLITE_TRANSIENT,
            )
        };
        self.db.check_rc("bind_text", rc)
    }

    fn bind_int64(&mut self, index: c_int, value: i64) -> Result<(), String> {
        self.db.check_rc("bind_int64", unsafe {
            sqlite3_bind_int64(self.ptr, index, value)
        })
    }

    fn step(&mut self) -> Result<bool, String> {
        match unsafe { sqlite3_step(self.ptr) } {
            SQLITE_ROW => Ok(true),
            SQLITE_DONE => Ok(false),
            other => Err(self.db.check_rc("step", other).unwrap_err()),
        }
    }

    fn col_opt_text(&self, col: c_int) -> Result<Option<String>, String> {
        if unsafe { sqlite3_column_type(self.ptr, col) } == SQLITE_NULL {
            return Ok(None);
        }
        let ptr = unsafe { sqlite3_column_text(self.ptr, col) };
        let len = unsafe { sqlite3_column_bytes(self.ptr, col) };
        if ptr.is_null() || len < 0 {
            return Ok(Some(String::new()));
        }
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
        std::str::from_utf8(bytes)
            .map(|s| Some(s.to_owned()))
            .map_err(|e| format!("blocked:codex_sqlite_utf8:{e}"))
    }

    fn col_text(&self, col: c_int) -> Result<String, String> {
        Ok(self.col_opt_text(col)?.unwrap_or_default())
    }

    fn col_int64(&self, col: c_int) -> i64 {
        unsafe { sqlite3_column_int64(self.ptr, col) }
    }

    fn col_double(&self, col: c_int) -> f64 {
        unsafe { sqlite3_column_double(self.ptr, col) }
    }
}

fn file_uri_for_path(path: &Path) -> Result<String, String> {
    let raw = path
        .to_str()
        .ok_or_else(|| "blocked:codex_sqlite_non_utf8_path".to_owned())?;
    let mut out = String::with_capacity(raw.len() + 16);
    out.push_str("file:");
    for byte in raw.as_bytes() {
        match byte {
            b'%' => out.push_str("%25"),
            b'?' => out.push_str("%3F"),
            b'#' => out.push_str("%23"),
            b => out.push(*b as char),
        }
    }
    Ok(out)
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

struct CodexDbs {
    codex: SqliteDb,
    state: SqliteDb,
    history: SqliteDb,
}

struct ThreadTextIndex {
    snippets: Vec<String>,
    corpus: String,
}

impl CodexDbs {
    fn open(codex_home: &Path) -> Result<Self, String> {
        Ok(Self {
            codex: SqliteDb::open_readonly(&codex_home.join("sqlite").join("codex.db"))?,
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

    fn load_unread_receipts(&self) -> Result<BTreeMap<String, String>, String> {
        let mut stmt = self.state.prepare(
            "SELECT thread_id, first_unread_turn FROM thread_read_receipts WHERE first_unread_turn IS NOT NULL AND first_unread_turn != ''",
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

    fn load_priority_turn_ids(&self) -> Result<BTreeSet<String>, String> {
        let mut ids = self.history.query_text_set(
            "SELECT DISTINCT thread_id FROM thread_turns WHERE status = 'inProgress'",
        )?;
        ids.extend(self.history.query_text_set(
            "SELECT DISTINCT thread_id FROM thread_turns WHERE status = 'failed' ORDER BY rowid DESC LIMIT 64",
        )?);
        Ok(ids)
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

    fn thread_snippets(
        &self,
        thread_id: &str,
        max_items: usize,
    ) -> Result<ThreadTextIndex, String> {
        let mut stmt = self.history.prepare(
            "SELECT item_json FROM thread_items WHERE thread_id = ?1 AND item_type IN ('agentMessage', 'userMessage') ORDER BY rollout_ordinal DESC LIMIT ?2",
        )?;
        stmt.bind_text(1, thread_id)?;
        stmt.bind_int64(2, max_items as i64)?;
        let mut snippets = Vec::new();
        let mut seen = BTreeSet::new();
        let mut corpus = String::new();
        while stmt.step()? {
            let raw_json = stmt.col_text(0)?;
            for line in extract_normalized_texts_from_item_json(&raw_json) {
                if !corpus.is_empty() {
                    corpus.push('\n');
                }
                corpus.push_str(&line);
            }
            for snippet in extract_snippets_from_item_json(&raw_json) {
                if seen.insert(snippet.clone()) {
                    snippets.push(snippet);
                }
            }
        }
        Ok(ThreadTextIndex { snippets, corpus })
    }
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
    match turn_status {
        Some("failed") => CodexObservedStatus::Error,
        Some("completed" | "interrupted") => {
            if first_unread_turn.is_some() || automation_pending {
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
        CodexObservedStatus::Working | CodexObservedStatus::Error | CodexObservedStatus::Unread => {
            0
        }
        CodexObservedStatus::Idle => 1,
        CodexObservedStatus::Unknown(_) => 2,
    }
}

pub fn observe_codex_threads(
    codex_home: &Path,
    limit: usize,
) -> Result<Vec<CodexThreadObservation>, String> {
    let dbs = CodexDbs::open(codex_home)?;
    if limit == 0 {
        return Ok(Vec::new());
    }
    let archived_ids = dbs
        .state
        .query_text_set("SELECT id FROM threads WHERE archived != 0")?;
    let active_turn_ids = dbs
        .codex
        .query_text_set("SELECT DISTINCT thread_id FROM active_chat_turns")?;
    let pending_automation_ids = dbs.codex.query_text_set(
        "SELECT DISTINCT thread_id FROM automation_runs WHERE status = 'PENDING_REVIEW' AND read_at IS NULL",
    )?;
    let unread_receipts = dbs.load_unread_receipts()?;
    let priority_turn_ids = dbs.load_priority_turn_ids()?;

    let mut catalog = dbs.codex.prepare(
        "SELECT c.thread_id, c.host_id, c.display_title, COALESCE(c.cwd, ''), \
         COALESCE(c.source_updated_at, c.source_recency_at, 0) \
         FROM local_thread_catalog c \
         LEFT JOIN local_thread_catalog_hosts h ON c.host_id = h.host_id \
         WHERE c.missing_candidate = 0 \
           AND COALESCE(h.host_kind, '') != 'chatgpt' \
           AND COALESCE(c.source_kind, '') != 'chatgpt' \
         ORDER BY COALESCE(c.source_updated_at, c.source_recency_at, 0) DESC",
    )?;

    let mut observations = Vec::with_capacity(limit.min(64));
    while catalog.step()? {
        let thread_id = catalog.col_text(0)?;
        if !is_valid_uuid(&thread_id) || archived_ids.contains(&thread_id) {
            continue;
        }
        let in_active_turns = active_turn_ids.contains(&thread_id);
        let automation_pending = pending_automation_ids.contains(&thread_id);
        let first_unread_turn = unread_receipts.get(&thread_id).map(String::as_str);
        let is_priority = in_active_turns
            || automation_pending
            || first_unread_turn.is_some()
            || priority_turn_ids.contains(&thread_id);
        if observations.len() >= limit && !is_priority {
            continue;
        }
        let host_id = catalog.col_text(1)?;
        let display_title = catalog.col_text(2)?;
        let workspace_path = catalog.col_text(3)?;
        let updated_at_ms = timestamp_to_ms(catalog.col_double(4));

        let latest_turn = dbs.latest_turn(&thread_id)?;
        let status = map_codex_status(
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
        let watermark = format!("{turn_id}:{unread_str}:{updated_at_ms}");

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
    observations.sort_by(|a, b| {
        status_priority_rank(&a.status)
            .cmp(&status_priority_rank(&b.status))
            .then_with(|| b.updated_at_ms.cmp(&a.updated_at_ms))
    });
    observations.truncate(limit);
    Ok(observations)
}

fn collapse_whitespace(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_inline_markdown(raw: &str) -> String {
    let mut s = raw.trim_start();
    loop {
        if let Some(rest) = ["- ", "* ", "+ ", "> "]
            .iter()
            .find_map(|p| s.strip_prefix(p))
        {
            s = rest.trim_start();
        } else if let Some(rest) = Some(s.trim_start_matches('#'))
            .filter(|r| r.len() < s.len() && (r.is_empty() || r.starts_with(char::is_whitespace)))
        {
            s = rest.trim_start();
        } else {
            if s == ">" {
                s = "";
            }
            break;
        }
    }

    let mut unlinked = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(open) = rest.find('[') {
        unlinked.push_str(&rest[..open]);
        let after_open = &rest[open + 1..];
        if let Some((label, after_close)) = after_open
            .find(']')
            .map(|close| (&after_open[..close], &after_open[close + 1..]))
            .filter(|(label, after)| !label.contains('[') && after.starts_with('('))
        {
            if let Some(close_paren) = after_close[1..].find(')') {
                unlinked.push_str(label);
                rest = &after_close[1 + close_paren + 1..];
                continue;
            }
        }
        unlinked.push('[');
        rest = after_open;
    }
    unlinked.push_str(rest);

    let cleaned = unlinked.replace(['`', '*'], "");
    collapse_whitespace(cleaned.trim_end().trim_end_matches('\\'))
}

fn extract_normalized_texts_from_item_json(raw_json: &str) -> Vec<String> {
    let Ok(parsed) = serde_json::from_str::<Value>(raw_json) else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    let texts = parsed
        .get("text")
        .into_iter()
        .chain(
            parsed
                .get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|part| part.get("text")),
        )
        .filter_map(Value::as_str);
    for raw in texts {
        for line in raw.lines() {
            let trimmed_raw = line.trim();
            if trimmed_raw.starts_with("```")
                || (trimmed_raw.starts_with('<') && trimmed_raw.ends_with('>'))
                || trimmed_raw.starts_with("[{\"")
                || trimmed_raw.starts_with("{\"")
            {
                continue;
            }
            let normalized = strip_inline_markdown(trimmed_raw);
            if !normalized.is_empty() {
                lines.push(normalized);
            }
        }
    }
    lines
}

fn extract_snippets_from_item_json(raw_json: &str) -> Vec<String> {
    let mut snippets = Vec::new();
    let mut seen = BTreeSet::new();
    for normalized in extract_normalized_texts_from_item_json(raw_json) {
        let snippet = normalized
            .chars()
            .take(MAX_SNIPPET_CHARS)
            .collect::<String>()
            .trim_end()
            .to_owned();
        if snippet.chars().count() >= 2
            && snippet.chars().any(char::is_alphanumeric)
            && seen.insert(snippet.clone())
        {
            snippets.push(snippet);
        }
    }
    snippets
}

fn snippet_overlaps_peer(snippet: &str, peer_set: &BTreeSet<&str>, peer_corpus: &str) -> bool {
    peer_set.contains(snippet)
        || (snippet.chars().count() >= MIN_SUBSTRING_SNIPPET_CHARS && peer_corpus.contains(snippet))
}

pub fn read_codex_thread_fingerprint(
    codex_home: &Path,
    thread_id: &str,
    host_id: &str,
) -> Result<CodexThreadFingerprint, String> {
    if !is_valid_uuid(thread_id) {
        return Err(format!("blocked:invalid_codex_thread_id:{thread_id}"));
    }
    if host_id.trim().is_empty() {
        return Err("blocked:empty_codex_host_id".into());
    }
    let dbs = CodexDbs::open(codex_home)?;

    let mut target_stmt = dbs.codex.prepare(
        "SELECT c.display_title, COALESCE(c.cwd, '') \
         FROM local_thread_catalog c \
         LEFT JOIN local_thread_catalog_hosts h ON c.host_id = h.host_id \
         WHERE c.thread_id = ?1 \
           AND c.host_id = ?2 \
           AND c.missing_candidate = 0 \
           AND COALESCE(h.host_kind, '') != 'chatgpt' \
           AND COALESCE(c.source_kind, '') != 'chatgpt' \
         LIMIT 1",
    )?;
    target_stmt.bind_text(1, thread_id)?;
    target_stmt.bind_text(2, host_id)?;
    if !target_stmt.step()? {
        return Err(format!(
            "blocked:codex_thread_missing:{host_id}:{thread_id}"
        ));
    }
    if dbs.is_thread_archived(thread_id)? {
        return Err(format!(
            "blocked:codex_thread_archived:{host_id}:{thread_id}"
        ));
    }
    let display_title = target_stmt.col_text(0)?;
    let cwd = target_stmt.col_text(1)?;
    let project_name = (!cwd.trim().is_empty())
        .then(|| Path::new(&cwd).file_name().and_then(|s| s.to_str()))
        .flatten()
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    let target_index = dbs.thread_snippets(thread_id, 256)?;
    let mut sibling_stmt = dbs.codex.prepare(
        "SELECT c.thread_id \
         FROM local_thread_catalog c \
         LEFT JOIN local_thread_catalog_hosts h ON c.host_id = h.host_id \
         WHERE c.display_title = ?1 \
           AND c.missing_candidate = 0 \
           AND COALESCE(h.host_kind, '') != 'chatgpt' \
           AND COALESCE(c.source_kind, '') != 'chatgpt' \
           AND NOT (c.thread_id = ?2 AND c.host_id = ?3)",
    )?;
    sibling_stmt.bind_text(1, &display_title)?;
    sibling_stmt.bind_text(2, thread_id)?;
    sibling_stmt.bind_text(3, host_id)?;

    let mut sibling_candidates = Vec::new();
    let mut sibling_corpus = String::new();
    while sibling_stmt.step()? {
        let sib_id = sibling_stmt.col_text(0)?;
        if !is_valid_uuid(&sib_id) || dbs.is_thread_archived(&sib_id)? {
            continue;
        }
        let sib_index = dbs.thread_snippets(&sib_id, 256)?;
        sibling_candidates.extend(sib_index.snippets);
        if !sib_index.corpus.is_empty() {
            if !sibling_corpus.is_empty() {
                sibling_corpus.push('\n');
            }
            sibling_corpus.push_str(&sib_index.corpus);
        }
    }

    let target_set: BTreeSet<&str> = target_index.snippets.iter().map(String::as_str).collect();
    let sibling_set: BTreeSet<&str> = sibling_candidates.iter().map(String::as_str).collect();

    let (long_unique, short_unique): (Vec<String>, Vec<String>) = target_index
        .snippets
        .iter()
        .filter(|s| {
            !snippet_overlaps_peer(s.as_str(), &sibling_set, &sibling_corpus)
                && s.chars().count() >= 2
        })
        .cloned()
        .partition(|s| s.chars().count() >= MIN_SUBSTRING_SNIPPET_CHARS);
    let target_snippets: Vec<String> = long_unique
        .into_iter()
        .chain(short_unique)
        .take(MAX_SNIPPETS)
        .collect();

    let mut seen_siblings = BTreeSet::new();
    let conflicting_sibling_snippets: Vec<String> = sibling_candidates
        .into_iter()
        .filter(|snippet| {
            !snippet_overlaps_peer(snippet.as_str(), &target_set, &target_index.corpus)
                && snippet.chars().count() >= 2
                && seen_siblings.insert(snippet.clone())
        })
        .collect();

    Ok(CodexThreadFingerprint {
        thread_id: thread_id.to_owned(),
        host_id: host_id.to_owned(),
        display_title,
        project_name,
        target_snippets,
        conflicting_sibling_snippets,
    })
}

fn percent_encode_query_value(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(*byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

pub fn build_codex_thread_deeplink(thread_id: &str, host_id: &str) -> Result<String, String> {
    if !is_valid_uuid(thread_id) {
        return Err(format!("blocked:invalid_codex_thread_id:{thread_id}"));
    }
    let host_id = host_id.trim();
    if host_id.is_empty() {
        return Err("blocked:empty_codex_host_id".into());
    }
    if host_id == "local" {
        Ok(format!("codex://threads/{thread_id}"))
    } else {
        Ok(format!(
            "codex://threads/{thread_id}?hostId={}",
            percent_encode_query_value(host_id)
        ))
    }
}

struct SnapshotTextIndex<'a> {
    trimmed_nodes: BTreeSet<&'a str>,
    searchable: Vec<String>,
}

impl<'a> SnapshotTextIndex<'a> {
    fn new(snap: &'a CodexWindowSnapshot) -> Self {
        let all = snap
            .main_pane_texts
            .iter()
            .chain(&snap.active_sidebar_texts);
        let trimmed_nodes = all.clone().map(|t| t.trim()).collect();
        let mut searchable: Vec<String> = all.map(|t| collapse_whitespace(t)).collect();
        searchable.push(collapse_whitespace(&snap.main_pane_texts.join(" ")));
        searchable.push(collapse_whitespace(&snap.active_sidebar_texts.join(" ")));
        Self {
            trimmed_nodes,
            searchable,
        }
    }
}

fn snippet_matches_snapshot(snippet: &str, snap_ctx: &SnapshotTextIndex<'_>) -> bool {
    let trimmed = snippet.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.chars().count() < MIN_SUBSTRING_SNIPPET_CHARS {
        return snap_ctx.trimmed_nodes.contains(trimmed);
    }
    let collapsed = collapse_whitespace(trimmed);
    snap_ctx.searchable.iter().any(|t| t.contains(&collapsed))
}

pub fn verify_codex_window_readback(
    fp: &CodexThreadFingerprint,
    snap: &CodexWindowSnapshot,
) -> Result<(), String> {
    if snap.has_resume_error {
        return Err("unconfirmed:codex_resume_error".into());
    }
    let expected_title = fp.display_title.trim();
    if !expected_title.is_empty() && snap.window_title.trim() != expected_title {
        return Err(format!(
            "unconfirmed:codex_title_mismatch:expected={expected_title}:actual={}",
            snap.window_title.trim()
        ));
    }
    if let Some(expected_project) = fp
        .project_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if !snap
            .toolbar_project
            .as_deref()
            .is_some_and(|actual| actual.trim().eq_ignore_ascii_case(expected_project))
        {
            return Err(format!(
                "unconfirmed:codex_project_mismatch:expected={expected_project}:actual={:?}",
                snap.toolbar_project
            ));
        }
    }
    let snap_ctx = SnapshotTextIndex::new(snap);
    if fp
        .conflicting_sibling_snippets
        .iter()
        .any(|bad| snippet_matches_snapshot(bad, &snap_ctx))
    {
        return Err("unconfirmed:codex_conflicting_sibling_detected".into());
    }
    if !fp.target_snippets.is_empty()
        && !fp
            .target_snippets
            .iter()
            .any(|snippet| snippet_matches_snapshot(snippet, &snap_ctx))
    {
        return Err("unconfirmed:codex_target_snippet_missing".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "codex_sessions_tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) use tests::test_support;
