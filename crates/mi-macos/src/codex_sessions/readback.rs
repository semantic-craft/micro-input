use super::{is_valid_uuid, CodexDbs, CodexThreadFingerprint, CodexWindowSnapshot};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;

const MAX_SNIPPETS: usize = 8;
const MAX_SNIPPET_CHARS: usize = 80;
const MIN_SUBSTRING_SNIPPET_CHARS: usize = 12;
pub(super) const MAX_ITEM_JSON_BYTES: usize = 262_144;
pub(super) const MAX_THREAD_CORPUS_BYTES: usize = 65_536;
pub(super) const MAX_THREAD_SNIPPETS: usize = 512;

pub(super) struct ThreadTextIndex {
    pub(super) snippets: Vec<String>,
    pub(super) corpus: String,
}

impl CodexDbs {
    pub(super) fn thread_snippets(
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
            if corpus.len() >= MAX_THREAD_CORPUS_BYTES && snippets.len() >= MAX_THREAD_SNIPPETS {
                break;
            }
            let Some(raw_json) = stmt.col_bounded_text(0, MAX_ITEM_JSON_BYTES)? else {
                continue;
            };
            let Ok(parsed) = serde_json::from_str::<Value>(&raw_json) else {
                continue;
            };
            let lines = extract_normalized_lines_from_value(&parsed);
            if corpus.len() < MAX_THREAD_CORPUS_BYTES {
                for line in &lines {
                    if !push_bounded_corpus(&mut corpus, line, MAX_THREAD_CORPUS_BYTES) {
                        break;
                    }
                }
            }
            if snippets.len() < MAX_THREAD_SNIPPETS {
                for line in &lines {
                    if let Some(snippet) = line_to_snippet(line) {
                        if seen.insert(snippet.clone()) {
                            snippets.push(snippet);
                            if snippets.len() >= MAX_THREAD_SNIPPETS {
                                break;
                            }
                        }
                    }
                }
            }
        }
        Ok(ThreadTextIndex { snippets, corpus })
    }
}

fn push_bounded_corpus(corpus: &mut String, addition: &str, max_bytes: usize) -> bool {
    if corpus.len() >= max_bytes {
        return false;
    }
    if addition.is_empty() {
        return true;
    }
    if !corpus.is_empty() {
        if corpus.len() + 1 >= max_bytes {
            return false;
        }
        corpus.push('\n');
    }
    let remaining = max_bytes - corpus.len();
    if addition.len() <= remaining {
        corpus.push_str(addition);
        corpus.len() < max_bytes
    } else {
        let mut end = remaining.min(addition.len());
        while end > 0 && !addition.is_char_boundary(end) {
            end -= 1;
        }
        corpus.push_str(&addition[..end]);
        false
    }
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

fn extract_normalized_lines_from_value(parsed: &Value) -> Vec<String> {
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

fn line_to_snippet(normalized: &str) -> Option<String> {
    let snippet = normalized
        .chars()
        .take(MAX_SNIPPET_CHARS)
        .collect::<String>()
        .trim_end()
        .to_owned();
    (snippet.chars().count() >= 2 && snippet.chars().any(char::is_alphanumeric)).then_some(snippet)
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
           AND NOT (c.thread_id = ?2 AND c.host_id = ?3) \
         ORDER BY COALESCE(c.source_updated_at, c.source_recency_at, 0) DESC LIMIT 24",
    )?;
    sibling_stmt.bind_text(1, &display_title)?;
    sibling_stmt.bind_text(2, thread_id)?;
    sibling_stmt.bind_text(3, host_id)?;

    let mut sibling_candidates = Vec::new();
    let mut sibling_corpus = String::new();
    let max_sibling_corpus_bytes = MAX_THREAD_CORPUS_BYTES * 4;
    while sibling_stmt.step()? {
        let sib_id = sibling_stmt.col_text(0)?;
        if !is_valid_uuid(&sib_id) || dbs.is_thread_archived(&sib_id)? {
            continue;
        }
        let sib_index = dbs.thread_snippets(&sib_id, 256)?;
        sibling_candidates.extend(sib_index.snippets);
        if sibling_corpus.len() < max_sibling_corpus_bytes && !sib_index.corpus.is_empty() {
            let _ = push_bounded_corpus(
                &mut sibling_corpus,
                &sib_index.corpus,
                max_sibling_corpus_bytes,
            );
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
        return Err("unconfirmed:codex_title_mismatch".into());
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
            return Err("unconfirmed:codex_project_mismatch".into());
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
