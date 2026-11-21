use super::*;
use tempfile::TempDir;

pub(crate) mod test_support {
    use super::*;

    const SQLITE_OPEN_READWRITE: c_int = 0x0000_0002;
    const SQLITE_OPEN_CREATE: c_int = 0x0000_0004;

    pub(crate) const UUID_A: &str = "01a1c5b2-d0fd-7001-96e4-f8395067a8f0";
    pub(crate) const UUID_B: &str = "01a1c5a6-96fc-78e1-8e0c-690c1de8c3c8";
    pub(crate) const UUID_C: &str = "01a1c5a6-96fc-78e1-8e0c-690c1de8c3c9";
    pub(crate) const UUID_D: &str = "01a1c5a6-96fc-78e1-8e0c-690c1de8c3ca";
    pub(crate) const UUID_E: &str = "01a1c5a6-96fc-78e1-8e0c-690c1de8c3cb";
    pub(crate) const UUID_F: &str = "01a1c5a6-96fc-78e1-8e0c-690c1de8c3cc";
    pub(crate) const UUID_G: &str = "01a1c5a6-96fc-78e1-8e0c-690c1de8c3cd";
    pub(crate) const UUID_CHATGPT: &str = "01a1c5a6-96fc-78e1-8e0c-690c1de8c3ce";
    pub(crate) const UUID_ARCHIVED: &str = "01a1c5a6-96fc-78e1-8e0c-690c1de8c3cf";

    pub(crate) fn exec_sql(path: &Path, sql: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        let mut raw_db: *mut sqlite3 = std::ptr::null_mut();
        let rc = unsafe {
            sqlite3_open_v2(
                c_path.as_ptr(),
                &mut raw_db,
                SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
                std::ptr::null(),
            )
        };
        let db = SqliteDb { ptr: raw_db };
        assert_eq!(rc, SQLITE_OK, "{}", db.last_error());
        let c_sql = CString::new(sql).unwrap();
        let mut errmsg: *mut c_char = std::ptr::null_mut();
        let exec_rc = unsafe {
            sqlite3_exec(
                db.ptr,
                c_sql.as_ptr(),
                None,
                std::ptr::null_mut(),
                &mut errmsg,
            )
        };
        if !errmsg.is_null() {
            let msg = unsafe { CStr::from_ptr(errmsg) }
                .to_string_lossy()
                .into_owned();
            unsafe { sqlite3_free(errmsg.cast()) };
            panic!("sqlite3_exec failed ({exec_rc}): {msg}");
        }
        assert_eq!(exec_rc, SQLITE_OK);
    }

    pub(crate) fn create_codex_fixture(root: &Path) {
        exec_sql(
            &root.join("sqlite").join("codex.db"),
            &format!(
                "
                CREATE TABLE local_thread_catalog_hosts (
                    host_id TEXT PRIMARY KEY,
                    host_kind TEXT NOT NULL
                );
                CREATE TABLE local_thread_catalog (
                    host_id TEXT NOT NULL,
                    thread_id TEXT NOT NULL,
                    display_title TEXT NOT NULL,
                    source_created_at REAL NOT NULL,
                    source_updated_at REAL NOT NULL,
                    cwd TEXT,
                    source_kind TEXT,
                    missing_candidate INTEGER NOT NULL DEFAULT 0,
                    source_recency_at REAL NOT NULL DEFAULT 0,
                    PRIMARY KEY (host_id, thread_id)
                );
                CREATE TABLE active_chat_turns (
                    profile TEXT NOT NULL,
                    account_id TEXT NOT NULL,
                    user_id TEXT NOT NULL,
                    thread_id TEXT NOT NULL,
                    launch_id TEXT NOT NULL,
                    observed_at_ms INTEGER NOT NULL,
                    PRIMARY KEY (profile, account_id, user_id, thread_id)
                );
                CREATE TABLE automation_runs (
                    thread_id TEXT PRIMARY KEY,
                    automation_id TEXT NOT NULL,
                    status TEXT NOT NULL,
                    read_at INTEGER
                );
                INSERT INTO local_thread_catalog_hosts (host_id, host_kind) VALUES
                    ('local', 'local'),
                    ('remote-ssh:devbox', 'ssh'),
                    ('chatgpt:acc', 'chatgpt');
                INSERT INTO local_thread_catalog
                    (host_id, thread_id, display_title, source_created_at, source_updated_at, cwd, source_kind, missing_candidate, source_recency_at)
                VALUES
                    ('local', '{UUID_A}', 'Same Title', 100.0, 1700000100.0, '/tmp/ws/micro-input', 'vscode', 0, 1700000100.0),
                    ('local', '{UUID_B}', 'Same Title', 100.0, 1700000090.0, '/tmp/ws/micro-input', 'vscode', 0, 1700000090.0),
                    ('remote-ssh:devbox', '{UUID_C}', 'Remote Fail', 100.0, 1700000080.0, '/srv/app', 'vscode', 0, 1700000080.0),
                    ('local', '{UUID_D}', 'Unread Receipt', 100.0, 1700000070.0, '/tmp/ws/other', 'vscode', 0, 1700000070.0),
                    ('local', '{UUID_E}', 'Auto Pending', 100.0, 1700000060.0, '/tmp/ws/auto', 'vscode', 0, 1700000060.0),
                    ('local', '{UUID_F}', 'Idle Ready', 100.0, 1700000050.0, '/tmp/ws/idle', 'vscode', 0, 1700000050.0),
                    ('local', '{UUID_G}', 'Missing Turn', 100.0, 1700000040.0, '/tmp/ws/unknown', 'vscode', 0, 1700000040.0),
                    ('local', 'not-a-uuid', 'Bad UUID', 100.0, 1700000200.0, '/tmp/ws/bad', 'vscode', 0, 1700000200.0),
                    ('local', '{UUID_ARCHIVED}', 'Archived', 100.0, 1700000190.0, '/tmp/ws/arch', 'vscode', 0, 1700000190.0),
                    ('chatgpt:acc', '{UUID_CHATGPT}', 'Web Chat', 100.0, 1700000180.0, NULL, 'chatgpt', 0, 1700000180.0),
                    ('local', '01a1c5a6-96fc-78e1-8e0c-690c1de8c3ff', 'Missing Candidate', 100.0, 1700000170.0, '/tmp/ws/miss', 'vscode', 1, 1700000170.0);
                INSERT INTO active_chat_turns VALUES ('default', 'a', 'u', '{UUID_A}', 'launch-1', 1700000100123);
                INSERT INTO automation_runs VALUES ('{UUID_E}', 'auto-1', 'PENDING_REVIEW', NULL);
                "
            ),
        );
        exec_sql(
            &root.join("state_5.sqlite"),
            &format!(
                "
                CREATE TABLE threads (
                    id TEXT PRIMARY KEY,
                    archived INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE thread_read_receipts (
                    thread_id TEXT PRIMARY KEY NOT NULL,
                    first_unread_turn TEXT,
                    revision TEXT NOT NULL,
                    last_published_turn TEXT
                );
                INSERT INTO threads (id, archived) VALUES
                    ('{UUID_A}', 0),
                    ('{UUID_B}', 0),
                    ('{UUID_C}', 0),
                    ('{UUID_D}', 0),
                    ('{UUID_E}', 0),
                    ('{UUID_F}', 0),
                    ('{UUID_G}', 0),
                    ('{UUID_ARCHIVED}', 1);
                INSERT INTO thread_read_receipts (thread_id, first_unread_turn, revision, last_published_turn) VALUES
                    ('{UUID_C}', 'turn-c1', 'rev-0', 'turn-c1'),
                    ('{UUID_D}', 'turn-d1', 'rev-1', 'turn-d1'),
                    ('{UUID_F}', NULL, 'rev-2', 'turn-f1');
                "
            ),
        );
        exec_sql(
            &root.join("thread_history_1.sqlite"),
            &format!(
                "
                CREATE TABLE thread_turns (
                    thread_id TEXT NOT NULL,
                    turn_id TEXT NOT NULL,
                    rollout_ordinal INTEGER NOT NULL,
                    status TEXT NOT NULL,
                    error_json TEXT,
                    PRIMARY KEY (thread_id, turn_id)
                );
                CREATE TABLE thread_items (
                    thread_id TEXT NOT NULL,
                    turn_id TEXT NOT NULL,
                    item_id TEXT NOT NULL,
                    rollout_ordinal INTEGER NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    item_json TEXT NOT NULL,
                    item_type TEXT NOT NULL DEFAULT '',
                    PRIMARY KEY (thread_id, turn_id, item_id)
                );
                INSERT INTO thread_turns (thread_id, turn_id, rollout_ordinal, status, error_json) VALUES
                    ('{UUID_A}', 'turn-a1', 1, 'completed', NULL),
                    ('{UUID_B}', 'turn-b1', 1, 'inProgress', NULL),
                    ('{UUID_C}', 'turn-c1', 1, 'failed', '{{\"message\":\"boom\"}}'),
                    ('{UUID_D}', 'turn-d1', 1, 'completed', NULL),
                    ('{UUID_E}', 'turn-e1', 1, 'interrupted', NULL),
                    ('{UUID_F}', 'turn-f1', 1, 'completed', NULL);
                INSERT INTO thread_items (thread_id, turn_id, item_id, rollout_ordinal, created_at_ms, item_json, item_type) VALUES
                    ('{UUID_A}', 'turn-a1', 'i1', 1, 1000, '{{\"type\":\"userMessage\",\"content\":[{{\"type\":\"text\",\"text\":\"Shared prompt line\"}}]}}', 'userMessage'),
                    ('{UUID_A}', 'turn-a1', 'i2', 2, 2000, '{{\"type\":\"agentMessage\",\"text\":\"Alpha unique reply from thread A\"}}', 'agentMessage'),
                    ('{UUID_B}', 'turn-b1', 'i1', 1, 1000, '{{\"type\":\"userMessage\",\"content\":[{{\"type\":\"text\",\"text\":\"Shared prompt line\"}}]}}', 'userMessage'),
                    ('{UUID_B}', 'turn-b1', 'i2', 2, 2000, '{{\"type\":\"agentMessage\",\"text\":\"Beta conflicting sibling reply\"}}', 'agentMessage');
                "
            ),
        );
    }
}

use test_support::*;

#[test]
fn fails_closed_when_sqlite_databases_are_missing() {
    let dir = TempDir::new().unwrap();
    let missing_dir_err = observe_codex_threads(dir.path(), 6).unwrap_err();
    assert!(
        missing_dir_err.starts_with("blocked:codex_home_missing:"),
        "{missing_dir_err}"
    );
    std::fs::create_dir_all(dir.path().join("sqlite")).unwrap();
    let err = observe_codex_threads(dir.path(), 6).unwrap_err();
    assert!(err.starts_with("blocked:codex_sqlite_missing:"), "{err}");
}

#[test]
fn observes_codex_threads_filtering_mapping_statuses_and_watermarks() {
    let dir = TempDir::new().unwrap();
    create_codex_fixture(dir.path());

    let rows = observe_codex_threads(dir.path(), 10).unwrap();
    let ids: Vec<&str> = rows.iter().map(|r| r.thread_id.as_str()).collect();
    assert_eq!(
        ids,
        vec![UUID_C, UUID_D, UUID_E, UUID_A, UUID_B, UUID_F, UUID_G]
    );

    assert_eq!(rows[0].host_id, "remote-ssh:devbox");
    assert_eq!(rows[0].status, CodexObservedStatus::Error);
    assert_eq!(rows[1].status, CodexObservedStatus::Unread);
    assert_eq!(rows[1].watermark, "turn-d1:turn-d1:1700000070000");
    assert_eq!(rows[2].status, CodexObservedStatus::Unread);
    assert_eq!(rows[3].status, CodexObservedStatus::Working);
    assert_eq!(rows[3].updated_at_ms, 1_700_000_100_000);
    assert_eq!(rows[3].watermark, "turn-a1::1700000100000");
    assert_eq!(rows[4].status, CodexObservedStatus::Working);
    assert_eq!(rows[5].status, CodexObservedStatus::Idle);
    assert_eq!(
        rows[6].status,
        CodexObservedStatus::Unknown("missing_turn".into())
    );

    let limited = observe_codex_threads(dir.path(), 2).unwrap();
    assert_eq!(limited.len(), 2);
    assert_eq!(limited[0].thread_id, UUID_C);
    assert_eq!(limited[1].thread_id, UUID_D);
}

#[test]
fn observes_threads_with_null_source_kind_on_local_host() {
    let dir = TempDir::new().unwrap();
    create_codex_fixture(dir.path());
    let uuid_null = "01a1c5a6-96fc-78e1-8e0c-690c1de8c3d0";

    exec_sql(
        &dir.path().join("sqlite").join("codex.db"),
        &format!(
            "INSERT INTO local_thread_catalog \
             (host_id, thread_id, display_title, source_created_at, source_updated_at, cwd, source_kind, missing_candidate, source_recency_at) \
             VALUES ('local', '{uuid_null}', 'Null Source', 100.0, 1700000105.0, '/tmp/ws/null', NULL, 0, 1700000105.0);"
        ),
    );
    exec_sql(
        &dir.path().join("state_5.sqlite"),
        &format!("INSERT INTO threads (id, archived) VALUES ('{uuid_null}', 0);"),
    );
    exec_sql(
        &dir.path().join("thread_history_1.sqlite"),
        &format!(
            "INSERT INTO thread_turns (thread_id, turn_id, rollout_ordinal, status, error_json) \
             VALUES ('{uuid_null}', 'turn-null-1', 1, 'completed', NULL);"
        ),
    );

    let rows = observe_codex_threads(dir.path(), 20).unwrap();
    assert!(rows.iter().any(|row| row.thread_id == uuid_null));
    let fp = read_codex_thread_fingerprint(dir.path(), uuid_null, "local").unwrap();
    assert_eq!(fp.display_title, "Null Source");
}

#[test]
fn observe_codex_threads_retains_older_working_thread_behind_64_idle_and_archived_rows() {
    let dir = TempDir::new().unwrap();
    create_codex_fixture(dir.path());

    let mut codex_sql = String::from(
        "DELETE FROM local_thread_catalog; DELETE FROM active_chat_turns; DELETE FROM automation_runs;\n",
    );
    let mut state_sql = String::from("DELETE FROM threads; DELETE FROM thread_read_receipts;\n");
    let mut history_sql = String::from("DELETE FROM thread_turns; DELETE FROM thread_items;\n");

    let older_working = "01a1c5b2-d0fd-7001-96e4-000000000001";
    codex_sql.push_str(&format!(
        "INSERT INTO local_thread_catalog VALUES ('local', '{older_working}', 'Older Working', 100.0, 1700000001.0, '/tmp/ws/work', 'vscode', 0, 1700000001.0);\n"
    ));
    codex_sql.push_str(&format!(
        "INSERT INTO active_chat_turns VALUES ('default', 'a', 'u', '{older_working}', 'launch-work', 1700000001123);\n"
    ));
    state_sql.push_str(&format!(
        "INSERT INTO threads (id, archived) VALUES ('{older_working}', 0);\n"
    ));
    history_sql.push_str(&format!(
        "INSERT INTO thread_turns VALUES ('{older_working}', 'turn-work-1', 1, 'completed', NULL);\n"
    ));

    for i in 0..64u32 {
        let id = format!("01a1c5b2-d0fd-7001-96e4-10000000{i:04x}");
        let ts = 1_700_001_000.0 + f64::from(i);
        codex_sql.push_str(&format!(
            "INSERT INTO local_thread_catalog VALUES ('local', '{id}', 'Idle {i}', 100.0, {ts}, '/tmp/ws/idle-{i}', 'vscode', 0, {ts});\n"
        ));
        state_sql.push_str(&format!(
            "INSERT INTO threads (id, archived) VALUES ('{id}', 0);\n"
        ));
        history_sql.push_str(&format!(
            "INSERT INTO thread_turns VALUES ('{id}', 'turn-idle-{i}', 1, 'completed', NULL);\n"
        ));
    }

    for i in 0..256u32 {
        let id = format!("01a1c5b2-d0fd-7001-96e4-20000000{i:04x}");
        let ts = 1_700_002_000.0 + f64::from(i);
        codex_sql.push_str(&format!(
            "INSERT INTO local_thread_catalog VALUES ('local', '{id}', 'Arch {i}', 100.0, {ts}, '/tmp/ws/arch-{i}', 'vscode', 0, {ts});\n"
        ));
        state_sql.push_str(&format!(
            "INSERT INTO threads (id, archived) VALUES ('{id}', 1);\n"
        ));
    }

    exec_sql(&dir.path().join("sqlite").join("codex.db"), &codex_sql);
    exec_sql(&dir.path().join("state_5.sqlite"), &state_sql);
    exec_sql(&dir.path().join("thread_history_1.sqlite"), &history_sql);

    let rows = observe_codex_threads(dir.path(), 64).unwrap();
    assert_eq!(rows.len(), 64);
    assert_eq!(rows[0].thread_id, older_working);
    assert_eq!(rows[0].status, CodexObservedStatus::Working);
    assert!(rows
        .iter()
        .all(|row| !row.display_title.starts_with("Arch ")));
}

#[test]
fn fingerprint_extracts_disjoint_sibling_snippets_and_readback_verifies_strictly() {
    let dir = TempDir::new().unwrap();
    create_codex_fixture(dir.path());

    let fp = read_codex_thread_fingerprint(dir.path(), UUID_A, "local").unwrap();
    assert_eq!(fp.display_title, "Same Title");
    assert_eq!(fp.project_name.as_deref(), Some("micro-input"));
    assert_eq!(
        fp.target_snippets,
        vec!["Alpha unique reply from thread A".to_string()]
    );
    assert_eq!(
        fp.conflicting_sibling_snippets,
        vec!["Beta conflicting sibling reply".to_string()]
    );

    let good_snap = CodexWindowSnapshot {
        window_title: "Same Title".into(),
        toolbar_project: Some("micro-input".into()),
        active_sidebar_texts: vec!["Same Title".into()],
        main_pane_texts: vec!["Alpha unique reply from thread A".into()],
        has_resume_error: false,
    };
    verify_codex_window_readback(&fp, &good_snap).unwrap();

    let mut resume_err_snap = good_snap.clone();
    resume_err_snap.has_resume_error = true;
    assert!(verify_codex_window_readback(&fp, &resume_err_snap)
        .unwrap_err()
        .contains("codex_resume_error"));

    let mut wrong_title = good_snap.clone();
    wrong_title.window_title = "ChatGPT".into();
    let title_err = verify_codex_window_readback(&fp, &wrong_title).unwrap_err();
    assert_eq!(title_err, "unconfirmed:codex_title_mismatch");
    assert!(!title_err.contains("Same Title"));
    assert!(!title_err.contains("ChatGPT"));

    let mut wrong_project = good_snap.clone();
    wrong_project.toolbar_project = Some("other-repo".into());
    let project_err = verify_codex_window_readback(&fp, &wrong_project).unwrap_err();
    assert_eq!(project_err, "unconfirmed:codex_project_mismatch");
    assert!(!project_err.contains("micro-input"));
    assert!(!project_err.contains("other-repo"));

    let mut wrong_sibling = good_snap.clone();
    wrong_sibling.main_pane_texts = vec!["Beta conflicting sibling reply".into()];
    assert!(verify_codex_window_readback(&fp, &wrong_sibling)
        .unwrap_err()
        .contains("codex_conflicting_sibling_detected"));

    let mut missing_target = good_snap;
    missing_target.main_pane_texts = vec!["Shared prompt line".into()];
    assert!(verify_codex_window_readback(&fp, &missing_target)
        .unwrap_err()
        .contains("codex_target_snippet_missing"));
}

#[test]
fn readback_accepts_empty_target_snippets_when_conflicting_sibling_snippets_are_absent() {
    let fp = CodexThreadFingerprint {
        thread_id: UUID_A.into(),
        host_id: "local".into(),
        display_title: "Same Title".into(),
        project_name: Some("micro-input".into()),
        target_snippets: Vec::new(),
        conflicting_sibling_snippets: vec!["Beta conflicting sibling reply".into()],
    };

    let clean_snap = CodexWindowSnapshot {
        window_title: "Same Title".into(),
        toolbar_project: Some("micro-input".into()),
        active_sidebar_texts: vec!["Same Title".into()],
        main_pane_texts: vec!["Shared prompt line".into()],
        has_resume_error: false,
    };
    verify_codex_window_readback(&fp, &clean_snap).unwrap();

    let mut sibling_snap = clean_snap;
    sibling_snap
        .main_pane_texts
        .push("Beta conflicting sibling reply".into());
    assert_eq!(
        verify_codex_window_readback(&fp, &sibling_snap).unwrap_err(),
        "unconfirmed:codex_conflicting_sibling_detected"
    );
}

#[test]
fn short_sibling_reply_requires_exact_node_match_and_avoids_substring_false_positive() {
    let dir = TempDir::new().unwrap();
    create_codex_fixture(dir.path());
    exec_sql(
        &dir.path().join("thread_history_1.sqlite"),
        &format!(
            "DELETE FROM thread_items; \
             INSERT INTO thread_items (thread_id, turn_id, item_id, rollout_ordinal, created_at_ms, item_json, item_type) VALUES \
             ('{UUID_A}', 'turn-a1', 'i1', 1, 1000, '{{\"type\":\"agentMessage\",\"text\":\"已确认原聊天仍在处理并继续运行\"}}', 'agentMessage'), \
             ('{UUID_B}', 'turn-b1', 'i1', 1, 1000, '{{\"type\":\"agentMessage\",\"text\":\"确认\"}}', 'agentMessage');"
        ),
    );

    let fp = read_codex_thread_fingerprint(dir.path(), UUID_A, "local").unwrap();
    assert_eq!(
        fp.target_snippets,
        vec!["已确认原聊天仍在处理并继续运行".to_string()]
    );
    assert_eq!(fp.conflicting_sibling_snippets, vec!["确认".to_string()]);

    let snap = CodexWindowSnapshot {
        window_title: "Same Title".into(),
        toolbar_project: Some("micro-input".into()),
        active_sidebar_texts: vec!["Same Title".into()],
        main_pane_texts: vec!["已确认原聊天仍在处理并继续运行".into()],
        has_resume_error: false,
    };
    verify_codex_window_readback(&fp, &snap).unwrap();

    let mut exact_conflict = snap;
    exact_conflict.main_pane_texts.push("确认".into());
    assert_eq!(
        verify_codex_window_readback(&fp, &exact_conflict).unwrap_err(),
        "unconfirmed:codex_conflicting_sibling_detected"
    );
}

#[test]
fn markdown_multiline_snippets_match_split_ax_nodes_and_case_insensitive_project() {
    let dir = TempDir::new().unwrap();
    create_codex_fixture(dir.path());
    exec_sql(
        &dir.path().join("thread_history_1.sqlite"),
        &format!(
            "DELETE FROM thread_items; \
             INSERT INTO thread_items (thread_id, turn_id, item_id, rollout_ordinal, created_at_ms, item_json, item_type) VALUES \
             ('{UUID_A}', 'turn-a1', 'i1', 1, 1000, '{{\"type\":\"agentMessage\",\"text\":\"## 结论\\n```rust\\nlet fenced_only = 1;\\n```\\n- 查看 [交互稿](file:///tmp/a.md) 和 `/poteto-mode`\\\\\\n> 下一步 *直接* 推进\"}}', 'agentMessage');"
        ),
    );

    let fp = read_codex_thread_fingerprint(dir.path(), UUID_A, "local").unwrap();
    assert_eq!(fp.project_name.as_deref(), Some("micro-input"));
    assert!(fp
        .target_snippets
        .contains(&"查看 交互稿 和 /poteto-mode".to_string()));
    assert!(fp.target_snippets.contains(&"下一步 直接 推进".to_string()));
    assert!(fp.target_snippets.contains(&"结论".to_string()));
    assert!(fp.target_snippets.iter().all(|s| !s.contains("```")));
    assert!(fp
        .target_snippets
        .contains(&"let fenced_only = 1;".to_string()));

    let snap = CodexWindowSnapshot {
        window_title: "Same Title".into(),
        toolbar_project: Some("Micro-input".into()),
        active_sidebar_texts: vec!["Same Title".into()],
        main_pane_texts: vec![
            "查看".into(),
            "交互稿".into(),
            "和".into(),
            "/poteto-mode".into(),
        ],
        has_resume_error: false,
    };
    verify_codex_window_readback(&fp, &snap).unwrap();
}

#[test]
fn substring_in_long_target_line_excludes_conflicting_sibling_heading() {
    let dir = TempDir::new().unwrap();
    create_codex_fixture(dir.path());
    let long_target_line = "  4. When the done check passes and all verification gates are green, end your reply with the `## Next Chat Prompt (Copy & Paste)` code block.";
    assert!(long_target_line.chars().count() > 100);
    exec_sql(
        &dir.path().join("thread_history_1.sqlite"),
        &format!(
            "DELETE FROM thread_items; \
             INSERT INTO thread_items (thread_id, turn_id, item_id, rollout_ordinal, created_at_ms, item_json, item_type) VALUES \
             ('{UUID_A}', 'turn-a1', 'i1', 1, 1000, '{{\"type\":\"userMessage\",\"content\":[{{\"type\":\"text\",\"text\":{long_json}}}]}}', 'userMessage'), \
             ('{UUID_A}', 'turn-a1', 'i2', 2, 2000, '{{\"type\":\"agentMessage\",\"text\":\"Thread A unique confirmation sentence\"}}', 'agentMessage'), \
             ('{UUID_B}', 'turn-b1', 'i1', 1, 1000, '{{\"type\":\"agentMessage\",\"text\":\"## Next Chat Prompt (Copy & Paste)\\nBeta unique conflicting sibling sentence\"}}', 'agentMessage');",
            long_json = serde_json::to_string(long_target_line).unwrap(),
        ),
    );

    let fp_a = read_codex_thread_fingerprint(dir.path(), UUID_A, "local").unwrap();
    assert!(
        !fp_a
            .conflicting_sibling_snippets
            .iter()
            .any(|s| s == "Next Chat Prompt (Copy & Paste)"),
        "unexpected conflicting snippet: {:?}",
        fp_a.conflicting_sibling_snippets
    );
    assert!(fp_a
        .conflicting_sibling_snippets
        .contains(&"Beta unique conflicting sibling sentence".to_string()));

    let snap_with_ncp_heading = CodexWindowSnapshot {
        window_title: "Same Title".into(),
        toolbar_project: Some("micro-input".into()),
        active_sidebar_texts: vec!["Same Title".into()],
        main_pane_texts: vec![
            "Thread A unique confirmation sentence".into(),
            "Next Chat Prompt (Copy & Paste)".into(),
        ],
        has_resume_error: false,
    };
    verify_codex_window_readback(&fp_a, &snap_with_ncp_heading).unwrap();
}

#[test]
fn deeplink_builder_validates_uuid_and_encodes_non_local_host() {
    assert_eq!(
        build_codex_thread_deeplink(UUID_A, "local").unwrap(),
        format!("codex://threads/{UUID_A}")
    );
    assert_eq!(
        build_codex_thread_deeplink(UUID_A, "remote-ssh:devbox").unwrap(),
        format!("codex://threads/{UUID_A}?hostId=remote-ssh%3Adevbox")
    );
    assert!(build_codex_thread_deeplink("not-a-uuid", "local").is_err());
    assert!(build_codex_thread_deeplink(UUID_A, "").is_err());
}

#[test]
fn checked_sqlite_text_len_rejects_lengths_exceeding_c_int_max() {
    assert_eq!(checked_sqlite_text_len(0).unwrap(), 0);
    assert_eq!(checked_sqlite_text_len(128).unwrap(), 128);
    assert_eq!(
        checked_sqlite_text_len(c_int::MAX as usize).unwrap(),
        c_int::MAX
    );
    let err = checked_sqlite_text_len((c_int::MAX as usize).saturating_add(1)).unwrap_err();
    assert!(err.contains("i32 length limit"));
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_codex_home_and_sqlite_and_redacts_error_paths() {
    use std::os::unix::fs::symlink;

    let real_dir = TempDir::new().unwrap();
    create_codex_fixture(real_dir.path());

    let link_parent = TempDir::new().unwrap();
    let linked_home = link_parent.path().join("linked-codex-home");
    symlink(real_dir.path(), &linked_home).unwrap();
    let home_err = observe_codex_threads(&linked_home, 6).unwrap_err();
    assert!(
        home_err.starts_with("blocked:codex_home_unsafe:"),
        "{home_err}"
    );

    let sqlite_symlink_root = TempDir::new().unwrap();
    create_codex_fixture(sqlite_symlink_root.path());
    let sqlite_dir = sqlite_symlink_root.path().join("sqlite");
    let moved_sqlite_dir = sqlite_symlink_root.path().join("moved-sqlite");
    std::fs::rename(&sqlite_dir, &moved_sqlite_dir).unwrap();
    symlink(&moved_sqlite_dir, &sqlite_dir).unwrap();
    let sqlite_dir_err = observe_codex_threads(sqlite_symlink_root.path(), 6).unwrap_err();
    assert!(
        sqlite_dir_err.starts_with("blocked:codex_home_unsafe:"),
        "{sqlite_dir_err}"
    );
    let sqlite_parent_err = SqliteDb::open_readonly(&sqlite_dir.join("codex.db"))
        .err()
        .unwrap();
    assert!(
        sqlite_parent_err.starts_with("blocked:codex_sqlite_unsafe:"),
        "{sqlite_parent_err}"
    );

    let db_symlink_root = TempDir::new().unwrap();
    create_codex_fixture(db_symlink_root.path());
    let codex_db = db_symlink_root.path().join("sqlite").join("codex.db");
    let moved_db = db_symlink_root.path().join("moved-codex.db");
    std::fs::rename(&codex_db, &moved_db).unwrap();
    symlink(&moved_db, &codex_db).unwrap();
    let db_err = observe_codex_threads(db_symlink_root.path(), 6).unwrap_err();
    assert!(
        db_err.starts_with("blocked:codex_sqlite_unsafe:"),
        "{db_err}"
    );

    let missing_home = Path::new("/Users/private/.codex");
    let missing_home_err = observe_codex_threads(missing_home, 6).unwrap_err();
    assert!(
        missing_home_err.starts_with("blocked:codex_home_missing:/Users/[redacted]/.codex"),
        "{missing_home_err}"
    );
    assert!(!missing_home_err.contains("private"));

    let missing_db = Path::new("/Users/private/.codex/sqlite/codex.db");
    let missing_db_err = SqliteDb::open_readonly(missing_db).err().unwrap();
    assert!(
        missing_db_err
            .starts_with("blocked:codex_sqlite_missing:/Users/[redacted]/.codex/sqlite/codex.db"),
        "{missing_db_err}"
    );
    assert!(!missing_db_err.contains("private"));
}

#[test]
fn skips_oversized_item_json_caps_utf8_corpus_and_bounds_sibling_scan() {
    let dir = TempDir::new().unwrap();
    create_codex_fixture(dir.path());

    let giant_text = "x".repeat(MAX_ITEM_JSON_BYTES + 1024);
    let unicode_chunk = "中".repeat(12_000);
    let unicode_chunk_2 = "界".repeat(12_000);
    let mut history_sql = String::from("DELETE FROM thread_items;\n");
    history_sql.push_str(&format!(
        "INSERT INTO thread_items (thread_id, turn_id, item_id, rollout_ordinal, created_at_ms, item_json, item_type) VALUES \
         ('{UUID_A}', 'turn-a1', 'i-giant', 4, 4000, '{{\"type\":\"agentMessage\",\"text\":\"{giant_text}\"}}', 'agentMessage'), \
         ('{UUID_A}', 'turn-a1', 'i-utf8-1', 3, 3000, '{{\"type\":\"agentMessage\",\"text\":\"{unicode_chunk}\"}}', 'agentMessage'), \
         ('{UUID_A}', 'turn-a1', 'i-utf8-2', 2, 2000, '{{\"type\":\"agentMessage\",\"text\":\"{unicode_chunk_2}\"}}', 'agentMessage'), \
         ('{UUID_A}', 'turn-a1', 'i-normal', 1, 1000, '{{\"type\":\"agentMessage\",\"text\":\"Normal unique reply after giant item\"}}', 'agentMessage');\n"
    ));

    let mut codex_sql = String::new();
    let mut state_sql = String::new();
    for i in 0..32u32 {
        let sib_id = format!("01a1c5b2-d0fd-7001-96e4-99000000{i:04x}");
        let ts = 1_700_000_000.0 + f64::from(32 - i);
        codex_sql.push_str(&format!(
            "INSERT INTO local_thread_catalog VALUES ('local', '{sib_id}', 'Same Title', 100.0, {ts}, '/tmp/ws/sib-{i}', 'vscode', 0, {ts});\n"
        ));
        state_sql.push_str(&format!(
            "INSERT INTO threads (id, archived) VALUES ('{sib_id}', 0);\n"
        ));
        history_sql.push_str(&format!(
            "INSERT INTO thread_items (thread_id, turn_id, item_id, rollout_ordinal, created_at_ms, item_json, item_type) VALUES \
             ('{sib_id}', 'turn-sib-{i}', 'i1', 1, 1000, '{{\"type\":\"agentMessage\",\"text\":\"Sibling reply #{i:02}\"}}', 'agentMessage');\n"
        ));
    }
    exec_sql(&dir.path().join("sqlite").join("codex.db"), &codex_sql);
    exec_sql(&dir.path().join("state_5.sqlite"), &state_sql);
    exec_sql(&dir.path().join("thread_history_1.sqlite"), &history_sql);

    let dbs = CodexDbs::open(dir.path()).unwrap();
    let idx = dbs.thread_snippets(UUID_A, 256).unwrap();
    assert!(idx.corpus.len() <= MAX_THREAD_CORPUS_BYTES);
    assert!(idx.corpus.len() > 65_000);
    assert!(idx
        .snippets
        .contains(&"Normal unique reply after giant item".to_string()));
    assert!(!idx.snippets.iter().any(|s| s.starts_with("xxxx")));

    let fp = read_codex_thread_fingerprint(dir.path(), UUID_A, "local").unwrap();
    assert!(fp
        .target_snippets
        .contains(&"Normal unique reply after giant item".to_string()));
    assert!(fp
        .conflicting_sibling_snippets
        .contains(&"Sibling reply #00".to_string()));
    assert!(!fp
        .conflicting_sibling_snippets
        .contains(&"Sibling reply #31".to_string()));
    assert!(fp.conflicting_sibling_snippets.len() <= 24);
}

#[cfg(unix)]
#[test]
fn open_readonly_flags_symlinked_parent_even_when_target_lacks_db() {
    use std::os::unix::fs::symlink;

    let dir = TempDir::new().unwrap();
    let real_empty = dir.path().join("real-empty");
    std::fs::create_dir_all(&real_empty).unwrap();
    let linked_parent = dir.path().join("linked-sqlite");
    symlink(&real_empty, &linked_parent).unwrap();

    let err = SqliteDb::open_readonly(&linked_parent.join("codex.db"))
        .err()
        .unwrap();
    assert!(err.starts_with("blocked:codex_sqlite_unsafe:"), "{err}");
}

#[test]
fn noisy_thread_turns_do_not_evict_older_failed_or_in_progress_threads_and_snippets_are_capped() {
    let dir = TempDir::new().unwrap();
    create_codex_fixture(dir.path());

    let older_failed = "01a1c5b2-d0fd-7001-96e4-aaaa00000001";
    let older_working = "01a1c5b2-d0fd-7001-96e4-aaaa00000002";
    let noisy_failed = "01a1c5b2-d0fd-7001-96e4-bbbb00000001";
    let noisy_working = "01a1c5b2-d0fd-7001-96e4-bbbb00000002";

    let mut codex_sql = String::from(
        "DELETE FROM local_thread_catalog; DELETE FROM active_chat_turns; DELETE FROM automation_runs;\n",
    );
    let mut state_sql = String::from("DELETE FROM threads; DELETE FROM thread_read_receipts;\n");
    let mut history_sql = String::from("DELETE FROM thread_turns; DELETE FROM thread_items;\n");

    for (idx, (id, title)) in [
        (older_failed, "Older Failed"),
        (older_working, "Older InProgress"),
        (noisy_failed, "Noisy Failed"),
        (noisy_working, "Noisy InProgress"),
    ]
    .into_iter()
    .enumerate()
    {
        let ts = 1_700_000_000.0 + (idx as f64);
        codex_sql.push_str(&format!(
            "INSERT INTO local_thread_catalog VALUES ('local', '{id}', '{title}', 100.0, {ts}, '/tmp/ws/{idx}', 'vscode', 0, {ts});\n"
        ));
        state_sql.push_str(&format!(
            "INSERT INTO threads (id, archived) VALUES ('{id}', 0);\n"
        ));
    }

    for i in 0..8u32 {
        let idle_id = format!("01a1c5b2-d0fd-7001-96e4-cccc0000{i:04x}");
        let ts = 1_700_001_000.0 + f64::from(i);
        codex_sql.push_str(&format!(
            "INSERT INTO local_thread_catalog VALUES ('local', '{idle_id}', 'Idle {i}', 100.0, {ts}, '/tmp/ws/idle-{i}', 'vscode', 0, {ts});\n"
        ));
        state_sql.push_str(&format!(
            "INSERT INTO threads (id, archived) VALUES ('{idle_id}', 0);\n"
        ));
        history_sql.push_str(&format!(
            "INSERT INTO thread_turns VALUES ('{idle_id}', 'turn-idle-{i}', 1, 'completed', NULL);\n"
        ));
    }

    history_sql.push_str(&format!(
        "INSERT INTO thread_turns VALUES ('{older_failed}', 'turn-of-1', 1, 'failed', '{{\"message\":\"old\"}}');\n"
    ));
    state_sql.push_str(&format!(
        "INSERT INTO thread_read_receipts VALUES ('{older_failed}', 'turn-of-1', 'rev-of', 'turn-of-1');\n"
    ));
    state_sql.push_str(&format!(
        "INSERT INTO thread_read_receipts VALUES ('{noisy_failed}', 'turn-nf-0', 'rev-nf', 'turn-nf-0');\n"
    ));
    history_sql.push_str(&format!(
        "INSERT INTO thread_turns VALUES ('{older_working}', 'turn-ow-1', 1, 'inProgress', NULL);\n"
    ));
    for i in 0..70u32 {
        history_sql.push_str(&format!(
            "INSERT INTO thread_turns VALUES ('{noisy_failed}', 'turn-nf-{i}', {i}, 'failed', '{{\"message\":\"noisy\"}}');\n"
        ));
    }
    for i in 0..260u32 {
        history_sql.push_str(&format!(
            "INSERT INTO thread_turns VALUES ('{noisy_working}', 'turn-nw-{i}', {i}, 'inProgress', NULL);\n"
        ));
    }

    for i in 0..(MAX_THREAD_SNIPPETS + 48) {
        history_sql.push_str(&format!(
            "INSERT INTO thread_items (thread_id, turn_id, item_id, rollout_ordinal, created_at_ms, item_json, item_type) VALUES \
             ('{older_working}', 'turn-ow-1', 'item-{i}', {i}, 1000, '{{\"type\":\"agentMessage\",\"text\":\"Unique snippet line number {i:04}\"}}', 'agentMessage');\n"
        ));
    }

    exec_sql(&dir.path().join("sqlite").join("codex.db"), &codex_sql);
    exec_sql(&dir.path().join("state_5.sqlite"), &state_sql);
    exec_sql(&dir.path().join("thread_history_1.sqlite"), &history_sql);

    let dbs = CodexDbs::open(dir.path()).unwrap();
    let priority_ids = dbs.load_priority_turn_ids().unwrap();
    assert!(priority_ids.contains(older_failed));
    assert!(priority_ids.contains(older_working));
    assert!(priority_ids.contains(noisy_failed));
    assert!(priority_ids.contains(noisy_working));

    let observed = observe_codex_threads(dir.path(), 4).unwrap();
    let observed_ids: BTreeSet<&str> = observed.iter().map(|o| o.thread_id.as_str()).collect();
    assert!(observed_ids.contains(older_failed));
    assert!(observed_ids.contains(older_working));

    let idx = dbs.thread_snippets(older_working, 1024).unwrap();
    assert_eq!(idx.snippets.len(), MAX_THREAD_SNIPPETS);
    assert!(idx.snippets.len() <= MAX_THREAD_SNIPPETS);
}

#[test]
fn excludes_attached_and_spawned_child_threads_while_retaining_root_threads() {
    let dir = TempDir::new().unwrap();
    create_codex_fixture(dir.path());

    let state_db = dir.path().join("state_5.sqlite");
    exec_sql(
        &state_db,
        &format!(
            "ALTER TABLE threads ADD COLUMN attached_thread_parent_id TEXT;
             UPDATE threads SET attached_thread_parent_id = '{UUID_C}' WHERE id = '{UUID_A}';
             UPDATE threads SET attached_thread_parent_id = '' WHERE id = '{UUID_B}';"
        ),
    );

    let after_parent_col = observe_codex_threads(dir.path(), 10).unwrap();
    let ids_col_only: Vec<&str> = after_parent_col
        .iter()
        .map(|r| r.thread_id.as_str())
        .collect();
    assert!(!ids_col_only.contains(&UUID_A));
    assert!(ids_col_only.contains(&UUID_B));

    exec_sql(
        &state_db,
        &format!(
            "CREATE TABLE thread_spawn_edges (
                 parent_thread_id TEXT NOT NULL,
                 child_thread_id TEXT NOT NULL
             );
             INSERT INTO thread_spawn_edges (parent_thread_id, child_thread_id)
             VALUES ('{UUID_C}', '{UUID_D}');"
        ),
    );

    let after_both = observe_codex_threads(dir.path(), 10).unwrap();
    let ids_both: Vec<&str> = after_both.iter().map(|r| r.thread_id.as_str()).collect();
    assert_eq!(ids_both, vec![UUID_C, UUID_E, UUID_B, UUID_F, UUID_G]);
}

#[test]
fn status_priority_rank_orders_attention_error_unread_working_idle_under_limit() {
    let dir = TempDir::new().unwrap();
    create_codex_fixture(dir.path());
    let unique_working = "01a1c5b2-d0fd-7001-96e4-777700000001";
    exec_sql(
        &dir.path().join("sqlite").join("codex.db"),
        &format!(
            "INSERT INTO local_thread_catalog VALUES \
             ('local', '{unique_working}', 'Deploy Staging', 100.0, 1700000010.0, '/tmp/ws/deploy', 'vscode', 0, 1700000010.0); \
             INSERT INTO active_chat_turns VALUES ('default', 'a', 'u', '{unique_working}', 'launch-2', 1700000010000);"
        ),
    );
    exec_sql(
        &dir.path().join("state_5.sqlite"),
        &format!("INSERT INTO threads (id, archived) VALUES ('{unique_working}', 0);"),
    );
    exec_sql(
        &dir.path().join("thread_history_1.sqlite"),
        &format!(
            "INSERT INTO thread_turns (thread_id, turn_id, rollout_ordinal, status, error_json) \
             VALUES ('{unique_working}', 'turn-u1', 1, 'inProgress', NULL);"
        ),
    );

    let limited = observe_codex_threads(dir.path(), 6).unwrap();
    let limited_statuses: Vec<CodexObservedStatus> =
        limited.iter().map(|r| r.status.clone()).collect();
    assert_eq!(
        limited_statuses,
        vec![
            CodexObservedStatus::Error,
            CodexObservedStatus::Unread,
            CodexObservedStatus::Unread,
            CodexObservedStatus::Working,
            CodexObservedStatus::Working,
            CodexObservedStatus::Working,
        ]
    );

    let mut all = observe_codex_threads(dir.path(), 10).unwrap();
    let signals = CodexLiveWindowSignals {
        is_frontmost: false,
        toolbar_project: None,
        active_sidebar_texts: Vec::new(),
        attention_sidebar_titles: vec!["Deploy Staging".into()],
        ..Default::default()
    };
    assert_eq!(apply_codex_window_signals(&mut all, &signals), None);
    assert_eq!(all[0].thread_id, unique_working);
    assert_eq!(all[0].status, CodexObservedStatus::NeedsAttention);
    assert_eq!(all[0].watermark, "turn-u1::1700000010000:attention");
    assert_eq!(all[1].status, CodexObservedStatus::Error);
    assert_eq!(all[2].status, CodexObservedStatus::Unread);
    assert_eq!(all[4].status, CodexObservedStatus::Working);
    assert_eq!(all[6].status, CodexObservedStatus::Idle);
}

#[test]
fn apply_codex_window_signals_handles_duplicate_working_titles_and_frontmost_unread_acknowledgement(
) {
    let obs = |id: &str, title: &str, ws: &str, status: CodexObservedStatus, ts: u64, wm: &str| {
        CodexThreadObservation {
            thread_id: id.into(),
            host_id: "local".into(),
            display_title: title.into(),
            workspace_path: ws.into(),
            status,
            updated_at_ms: ts,
            watermark: wm.into(),
        }
    };

    let mut duplicate_working = vec![
        obs(
            UUID_A,
            "Same Title",
            "/tmp/ws/micro-input",
            CodexObservedStatus::Working,
            2_000,
            "wm-a",
        ),
        obs(
            UUID_B,
            "Same Title",
            "/tmp/ws/micro-input",
            CodexObservedStatus::Working,
            1_000,
            "wm-b",
        ),
    ];
    let dup_signals = CodexLiveWindowSignals {
        is_frontmost: false,
        toolbar_project: None,
        active_sidebar_texts: Vec::new(),
        attention_sidebar_titles: vec!["Same Title".into()],
        ..Default::default()
    };
    assert_eq!(
        apply_codex_window_signals(&mut duplicate_working, &dup_signals),
        None
    );
    assert_eq!(duplicate_working[0].status, CodexObservedStatus::Working);
    assert_eq!(duplicate_working[1].status, CodexObservedStatus::Working);

    let mut unread_rows = vec![
        obs(
            UUID_D,
            "Unread Receipt",
            "/tmp/ws/micro-input",
            CodexObservedStatus::Unread,
            3_000,
            "wm-d",
        ),
        obs(
            UUID_E,
            "Ambiguous Unread",
            "/tmp/ws/micro-input",
            CodexObservedStatus::Unread,
            2_500,
            "wm-e",
        ),
        obs(
            UUID_F,
            "Ambiguous Unread",
            "/tmp/ws/micro-input",
            CodexObservedStatus::Idle,
            1_500,
            "wm-f",
        ),
    ];

    let background_signals = CodexLiveWindowSignals {
        is_frontmost: false,
        toolbar_project: Some("micro-input".into()),
        active_sidebar_texts: vec!["Unread Receipt".into()],
        attention_sidebar_titles: Vec::new(),
        ..Default::default()
    };
    assert_eq!(
        apply_codex_window_signals(&mut unread_rows, &background_signals),
        None
    );
    assert_eq!(unread_rows[0].status, CodexObservedStatus::Unread);

    let wrong_project_signals = CodexLiveWindowSignals {
        is_frontmost: true,
        toolbar_project: Some("other-project".into()),
        active_sidebar_texts: vec!["Unread Receipt".into()],
        attention_sidebar_titles: Vec::new(),
        ..Default::default()
    };
    assert_eq!(
        apply_codex_window_signals(&mut unread_rows, &wrong_project_signals),
        None
    );
    assert_eq!(unread_rows[0].status, CodexObservedStatus::Unread);

    let ambiguous_title_signals = CodexLiveWindowSignals {
        is_frontmost: true,
        toolbar_project: Some("micro-input".into()),
        active_sidebar_texts: vec!["Ambiguous Unread".into()],
        attention_sidebar_titles: Vec::new(),
        ..Default::default()
    };
    assert_eq!(
        apply_codex_window_signals(&mut unread_rows, &ambiguous_title_signals),
        None
    );
    assert_eq!(unread_rows[1].status, CodexObservedStatus::Unread);

    let valid_frontmost_signals = CodexLiveWindowSignals {
        is_frontmost: true,
        toolbar_project: Some("Micro-Input".into()),
        active_sidebar_texts: vec!["Unread Receipt".into(), "1m".into()],
        attention_sidebar_titles: Vec::new(),
        ..Default::default()
    };
    let ack = apply_codex_window_signals(&mut unread_rows, &valid_frontmost_signals);
    assert_eq!(
        ack,
        Some((
            UUID_D.to_string(),
            "local".to_string(),
            "/tmp/ws/micro-input".to_string(),
            "wm-d".to_string(),
        ))
    );
    assert_eq!(unread_rows[0].thread_id, UUID_E);
    assert_eq!(unread_rows[0].status, CodexObservedStatus::Unread);
    assert_eq!(unread_rows[1].thread_id, UUID_D);
    assert_eq!(unread_rows[1].status, CodexObservedStatus::Idle);
}

#[test]
fn read_failed_turn_is_idle_and_frontmost_window_acknowledges_error_thread() {
    let dir = TempDir::new().unwrap();
    create_codex_fixture(dir.path());
    exec_sql(
        &dir.path().join("state_5.sqlite"),
        &format!("UPDATE thread_read_receipts SET first_unread_turn = NULL WHERE thread_id = '{UUID_C}';"),
    );

    let rows = observe_codex_threads(dir.path(), 10).unwrap();
    let row_c = rows.iter().find(|r| r.thread_id == UUID_C).unwrap();
    assert_eq!(row_c.status, CodexObservedStatus::Idle);

    let mut errored_rows = vec![CodexThreadObservation {
        thread_id: UUID_C.into(),
        host_id: "remote-ssh:devbox".into(),
        display_title: "Remote Fail".into(),
        workspace_path: "/srv/app".into(),
        status: CodexObservedStatus::Error,
        updated_at_ms: 1_700_000_080_000,
        watermark: "turn-c1:turn-c1:1700000080000".into(),
    }];
    let signals = CodexLiveWindowSignals {
        is_frontmost: true,
        toolbar_project: Some("app".into()),
        active_sidebar_texts: vec!["Remote Fail".into()],
        attention_sidebar_titles: Vec::new(),
        attention_windows: vec![(Some("app".into()), Vec::new())],
    };
    let ack = apply_codex_window_signals(&mut errored_rows, &signals);
    assert_eq!(
        ack,
        Some((
            UUID_C.to_string(),
            "remote-ssh:devbox".to_string(),
            "/srv/app".to_string(),
            "turn-c1:turn-c1:1700000080000".to_string(),
        ))
    );
    assert_eq!(errored_rows[0].status, CodexObservedStatus::Idle);
}

#[test]
fn multi_window_attention_scopes_by_project_and_child_threads_roll_up_to_root() {
    let mut multi_ws = vec![
        CodexThreadObservation {
            thread_id: UUID_A.into(),
            host_id: "local".into(),
            display_title: "Shared Title".into(),
            workspace_path: "/tmp/ws/repo-a".into(),
            status: CodexObservedStatus::Working,
            updated_at_ms: 2_000,
            watermark: "wm-a".into(),
        },
        CodexThreadObservation {
            thread_id: UUID_B.into(),
            host_id: "local".into(),
            display_title: "Shared Title".into(),
            workspace_path: "/tmp/ws/repo-b".into(),
            status: CodexObservedStatus::Working,
            updated_at_ms: 1_900,
            watermark: "wm-b".into(),
        },
    ];
    let multi_signals = CodexLiveWindowSignals {
        is_frontmost: false,
        toolbar_project: Some("repo-a".into()),
        active_sidebar_texts: Vec::new(),
        attention_sidebar_titles: vec!["Shared Title".into()],
        attention_windows: vec![
            (Some("repo-a".into()), Vec::new()),
            (Some("repo-b".into()), vec!["Shared Title".into()]),
        ],
    };
    assert_eq!(
        apply_codex_window_signals(&mut multi_ws, &multi_signals),
        None
    );
    assert_eq!(multi_ws[0].thread_id, UUID_B);
    assert_eq!(multi_ws[0].status, CodexObservedStatus::NeedsAttention);
    assert_eq!(multi_ws[0].watermark, "wm-b:attention");
    assert_eq!(multi_ws[1].thread_id, UUID_A);
    assert_eq!(multi_ws[1].status, CodexObservedStatus::Working);

    let dir = TempDir::new().unwrap();
    create_codex_fixture(dir.path());
    let root_work = "01a1c5b2-d0fd-7001-96e4-dddd00000001";
    let mid_child = "01a1c5b2-d0fd-7001-96e4-dddd00000002";
    let leaf_work = "01a1c5b2-d0fd-7001-96e4-dddd00000003";
    let root_err = "01a1c5b2-d0fd-7001-96e4-dddd00000004";
    let child_err = "01a1c5b2-d0fd-7001-96e4-dddd00000005";
    let cyc_a = "01a1c5b2-d0fd-7001-96e4-dddd00000006";
    let cyc_b = "01a1c5b2-d0fd-7001-96e4-dddd00000007";

    let mut codex_sql = String::from(
        "DELETE FROM local_thread_catalog; DELETE FROM active_chat_turns; DELETE FROM automation_runs;\n",
    );
    let mut state_sql = String::from(
        "DELETE FROM threads; DELETE FROM thread_read_receipts;\n\
         ALTER TABLE threads ADD COLUMN attached_thread_parent_id TEXT;\n\
         CREATE TABLE thread_spawn_edges (parent_thread_id TEXT NOT NULL, child_thread_id TEXT NOT NULL);\n",
    );
    let mut history_sql = String::from("DELETE FROM thread_turns; DELETE FROM thread_items;\n");

    for (idx, (id, title)) in [
        (root_work, "Older Root With Working Grandchild"),
        (root_err, "Older Root With Errored Child"),
        (mid_child, "Mid Child"),
        (leaf_work, "Leaf Working Child"),
        (child_err, "Errored Child"),
        (cyc_a, "Cycle A"),
        (cyc_b, "Cycle B"),
    ]
    .into_iter()
    .enumerate()
    {
        let ts = 1_700_000_001.0 + (idx as f64);
        codex_sql.push_str(&format!(
            "INSERT INTO local_thread_catalog VALUES ('local', '{id}', '{title}', 100.0, {ts}, '/tmp/ws/{idx}', 'vscode', 0, {ts});\n"
        ));
    }
    state_sql.push_str(&format!(
        "INSERT INTO threads (id, archived, attached_thread_parent_id) VALUES \
         ('{root_work}', 0, NULL), \
         ('{mid_child}', 0, '{root_work}'), \
         ('{leaf_work}', 0, NULL), \
         ('{root_err}', 0, NULL), \
         ('{child_err}', 0, '{root_err}'), \
         ('{cyc_a}', 0, '{cyc_b}'), \
         ('{cyc_b}', 0, '{cyc_a}');\n\
         INSERT INTO thread_spawn_edges VALUES ('{mid_child}', '{leaf_work}');\n\
         INSERT INTO thread_read_receipts VALUES ('{child_err}', 'turn-ce-1', 'rev-ce', 'turn-ce-1');\n"
    ));
    history_sql.push_str(&format!(
        "INSERT INTO thread_turns VALUES \
         ('{root_work}', 'turn-rw-1', 1, 'completed', NULL), \
         ('{leaf_work}', 'turn-lw-1', 1, 'inProgress', NULL), \
         ('{root_err}', 'turn-re-1', 1, 'completed', NULL), \
         ('{child_err}', 'turn-ce-1', 1, 'failed', '{{\"message\":\"subagent boom\"}}'), \
         ('{cyc_a}', 'turn-ca-1', 1, 'inProgress', NULL);\n"
    ));
    for i in 0..8u32 {
        let idle_id = format!("01a1c5b2-d0fd-7001-96e4-eeee0000{i:04x}");
        let ts = 1_700_005_000.0 + f64::from(i);
        codex_sql.push_str(&format!(
            "INSERT INTO local_thread_catalog VALUES ('local', '{idle_id}', 'Idle {i}', 100.0, {ts}, '/tmp/ws/idle-{i}', 'vscode', 0, {ts});\n"
        ));
        state_sql.push_str(&format!(
            "INSERT INTO threads (id, archived, attached_thread_parent_id) VALUES ('{idle_id}', 0, NULL);\n"
        ));
        history_sql.push_str(&format!(
            "INSERT INTO thread_turns VALUES ('{idle_id}', 'turn-idle-{i}', 1, 'completed', NULL);\n"
        ));
    }

    exec_sql(&dir.path().join("sqlite").join("codex.db"), &codex_sql);
    exec_sql(&dir.path().join("state_5.sqlite"), &state_sql);
    exec_sql(&dir.path().join("thread_history_1.sqlite"), &history_sql);

    let observed = observe_codex_threads(dir.path(), 4).unwrap();
    assert_eq!(observed[0].thread_id, root_err);
    assert_eq!(observed[0].status, CodexObservedStatus::Error);
    assert!(observed[0].watermark.ends_with(":suberror:turn-ce-1"));
    assert_eq!(observed[1].thread_id, root_work);
    assert_eq!(observed[1].status, CodexObservedStatus::Working);
    assert!(observed[1].watermark.ends_with(":subwork"));
    assert!(observed.iter().all(|o| {
        !matches!(
            o.thread_id.as_str(),
            id if id == mid_child || id == leaf_work || id == child_err || id == cyc_a || id == cyc_b
        )
    }));
}
