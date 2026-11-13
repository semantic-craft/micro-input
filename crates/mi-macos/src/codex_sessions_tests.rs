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
        vec![UUID_A, UUID_B, UUID_C, UUID_D, UUID_E, UUID_F, UUID_G]
    );

    assert_eq!(rows[0].status, CodexObservedStatus::Working);
    assert_eq!(rows[0].updated_at_ms, 1_700_000_100_000);
    assert_eq!(rows[0].watermark, "turn-a1::1700000100000");

    assert_eq!(rows[1].status, CodexObservedStatus::Working);
    assert_eq!(rows[2].host_id, "remote-ssh:devbox");
    assert_eq!(rows[2].status, CodexObservedStatus::Error);
    assert_eq!(rows[3].status, CodexObservedStatus::Unread);
    assert_eq!(rows[3].watermark, "turn-d1:turn-d1:1700000070000");
    assert_eq!(rows[4].status, CodexObservedStatus::Unread);
    assert_eq!(rows[5].status, CodexObservedStatus::Idle);
    assert_eq!(
        rows[6].status,
        CodexObservedStatus::Unknown("missing_turn".into())
    );

    let limited = observe_codex_threads(dir.path(), 2).unwrap();
    assert_eq!(limited.len(), 2);
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
    assert!(verify_codex_window_readback(&fp, &wrong_title)
        .unwrap_err()
        .contains("codex_title_mismatch"));

    let mut wrong_project = good_snap.clone();
    wrong_project.toolbar_project = Some("other-repo".into());
    assert!(verify_codex_window_readback(&fp, &wrong_project)
        .unwrap_err()
        .contains("codex_project_mismatch"));

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
