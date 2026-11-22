use mi_core::{
    BackupCommand, CanonicalAgent, HeaderAction, ProfileCommand, SidebarAction, WorkbenchStore,
};
use mi_model::board_layers::{BoardLayer, BoardLayerRoute, BoardPreviewSelection};
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn legacy_store(temp: &TempDir) -> WorkbenchStore {
    let store = WorkbenchStore::open_isolated(temp.path()).unwrap();
    let doc = &store.document_store.draft.document;
    let find = |name: &str| {
        doc.editable
            .graph
            .profiles
            .iter()
            .find(|(_, p)| p.name == name)
            .unwrap()
            .0
            .clone()
    };
    let ag = find("Antigravity");
    let preferences = serde_json::json!({
        "schema_version":1, "favorites":[], "host_active_profile_id":ag,
        "host_execution_enabled":true, "command_execution_enabled":true,
        "hardware_profile_slots":[ag, ag, find(mi_model::preset::LOCAL_CODEX_PROFILE_NAME), find("DeepSeek Coder (DSH)")],
        "active_hardware_slot":1
    });
    std::fs::write(
        temp.path().join("workbench.json"),
        serde_json::to_vec(doc).unwrap(),
    )
    .unwrap();
    std::fs::write(
        temp.path().join("workbench.preferences.json"),
        serde_json::to_vec(&preferences).unwrap(),
    )
    .unwrap();
    drop(store);
    WorkbenchStore::open_isolated(temp.path()).unwrap()
}

#[test]
fn canonical_sidebar_profiles_preserve_strict_order_and_idempotent_ready_status() {
    let temp = TempDir::new().unwrap();
    let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();

    assert!(
        store.has_all_desktop_agent_presets(),
        "initial isolated store must install all 5 canonical desktop agent presets"
    );

    let expected_display_names = [
        "Antigravity",
        "Codex",
        "Deepseek Harness",
        "Grok Bot",
        "其他软件",
    ];
    let actual_display_names: Vec<&str> = store
        .sidebar_rows()
        .iter()
        .map(|row| row.display_name.as_str())
        .collect();
    assert_eq!(actual_display_names, expected_display_names);

    let expected_raw_names = [
        "Antigravity",
        mi_model::preset::LOCAL_CODEX_PROFILE_NAME,
        "DeepSeek Coder (DSH)",
        "Grok Bot",
        "其他软件",
    ];
    let actual_raw_names: Vec<&str> = store
        .sidebar_rows()
        .iter()
        .map(|row| row.raw_name.as_str())
        .collect();
    assert_eq!(actual_raw_names, expected_raw_names);

    for (idx, agent) in CanonicalAgent::ALL.iter().enumerate() {
        assert_eq!(agent.rank(), idx);
        assert_eq!(agent.display_name(), expected_display_names[idx]);
    }

    let rev_before = store.projection().document_revision;
    store.set_status_message("已就绪");
    store.prepare_desktop_profiles().unwrap();
    assert_eq!(
        store.status_message(),
        "已就绪",
        "prepare_desktop_profiles must be idempotent and never clobber ready status when all 5 presets exist"
    );
    assert_eq!(store.projection().document_revision, rev_before);
    assert_eq!(
        store.projection().header.engine_version,
        env!("CARGO_PKG_VERSION")
    );
}

fn restore_previous_local_codex_starter(store: &mut WorkbenchStore) -> (String, String) {
    use mi_model::draft::Action;
    use serde_json::json;

    let (pid, profile) = store
        .document_store
        .draft
        .document
        .editable
        .graph
        .profiles
        .iter()
        .find(|(_, profile)| profile.name == mi_model::preset::LOCAL_CODEX_PROFILE_NAME)
        .unwrap();
    let pid = pid.clone();
    let lid = profile.layer_order[0].clone();
    store.document_store.update(|draft| {
        for slot in 0..=12 {
            draft.unassign(&json!(pid), &json!(lid), slot)?;
        }
        for (slot, id, name, category, detail, payload) in [
            (0, "desktop-open", "打开 / 聚焦应用", "应用", "com.openai.codex",
                json!({"type":"open_app", "bundle_id":"com.openai.codex"})),
            (1, "desktop-input", "聚焦当前输入框", "Agent", "com.openai.codex",
                json!({"type":"agent_command", "adapter":"codex", "command":"focus-input"})),
            (2, "desktop-search", "搜索任务", "Agent", "com.openai.codex",
                json!({"type":"agent_command", "adapter":"codex", "command":"search"})),
            (4, "desktop-model", "选择模型", "Agent", "目标应用的原生控件；未提供时明确提示",
                json!({"type":"shortcut", "key":"M", "modifiers":["control", "shift"], "target":"com.openai.codex"})),
            (5, "desktop-effort", "调整思考力度", "Agent", "目标应用的原生控件；未提供时明确提示",
                json!({"type":"shortcut", "key":"M", "modifiers":["control", "shift"], "target":"com.openai.codex"})),
            (6, "desktop-dictation", "按住说话 · 松开停止", "Agent", "目标应用的原生控件；未提供时明确提示",
                json!({"type":"agent_command", "adapter":"codex", "command":"push-to-talk"})),
            (7, "desktop-browser", "打开内置浏览器", "Agent", "目标应用的原生控件；未提供时明确提示",
                json!({"type":"shortcut", "key":"T", "modifiers":["command"], "target":"com.openai.codex"})),
            (8, "desktop-terminal", "打开终端面板", "Agent", "目标应用的原生控件；未提供时明确提示",
                json!({"type":"shortcut", "key":"`", "modifiers":["control"], "target":"com.openai.codex"})),
        ] {
            draft.assign(&json!(pid), &json!(lid), slot, Action {
                id: id.into(), name: name.into(), category: category.into(),
                detail: detail.into(), payload,
            })?;
        }
        Ok(())
    }).unwrap();
    (pid, lid)
}

#[test]
fn opening_saved_desktop_presets_migrates_unchanged_local_codex_once() {
    use mi_model::host_router::{resolve_host_action, ResolvedHostAction};

    let temp = TempDir::new().unwrap();
    let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();
    let (pid, lid) = restore_previous_local_codex_starter(&mut store);
    let doc = &store.document_store.draft.document;
    let native = doc
        .editable
        .graph
        .profiles
        .values()
        .find(|p| p.name == "Codex")
        .unwrap();
    let native_lid = native.layer_order[0].clone();
    let native_before = serde_json::to_value(&doc.editable.graph.layers[&native_lid]).unwrap();
    let revision_before = store.document_store.draft.revision;
    std::fs::write(
        temp.path().join("workbench.json"),
        serde_json::to_vec(&store.document_store.draft.document).unwrap(),
    )
    .unwrap();
    drop(store);

    let mut reopened = WorkbenchStore::open_isolated(temp.path()).unwrap();
    assert!(reopened.has_all_desktop_agent_presets());
    assert!(
        reopened
            .document_store
            .draft
            .document
            .editable
            .graph
            .profiles[&pid]
            .layer_order
            == [lid]
    );
    assert_eq!(reopened.document_store.draft.revision, revision_before + 1);
    let doc = &reopened.document_store.draft.document;
    assert_eq!(
        serde_json::to_value(&doc.editable.graph.layers[&native_lid]).unwrap(),
        native_before
    );
    for slot in 0..6u32 {
        assert_eq!(
            resolve_host_action(doc, Some(&pid), &format!("AG{slot:02}"), true),
            Some(ResolvedHostAction::AgentCommand {
                adapter: "four-app".into(),
                command: format!("slot-open:{slot}"),
            })
        );
    }
    for (slot, command) in [
        (6, "live-voice"),
        (7, "focus-input"),
        (8, "new-tab"),
        (9, "cycle-workspace-layout"),
        (10, "new-chat"),
        (11, "toggle-dictation"),
        (12, "submit"),
    ] {
        assert_eq!(
            resolve_host_action(doc, Some(&pid), &format!("AG{slot:02}"), true),
            Some(ResolvedHostAction::AgentCommand {
                adapter: "codex".into(),
                command: command.into()
            })
        );
    }
    let migrated_bytes = std::fs::read(temp.path().join("workbench.json")).unwrap();
    let migrated_revision = reopened.document_store.draft.revision;
    reopened.set_status_message("已就绪");
    reopened.prepare_desktop_profiles().unwrap();
    assert_eq!(reopened.status_message(), "已就绪");
    assert_eq!(reopened.document_store.draft.revision, migrated_revision);
    drop(reopened);

    let reopened = WorkbenchStore::open_isolated(temp.path()).unwrap();
    assert_eq!(reopened.document_store.draft.revision, migrated_revision);
    assert_eq!(
        std::fs::read(temp.path().join("workbench.json")).unwrap(),
        migrated_bytes
    );
}

fn restore_r26_local_codex_starter(store: &mut WorkbenchStore) -> (String, String) {
    use mi_model::draft::Action;
    use serde_json::json;

    let (pid, profile) = store
        .document_store
        .draft
        .document
        .editable
        .graph
        .profiles
        .iter()
        .find(|(_, profile)| profile.name == mi_model::preset::LOCAL_CODEX_PROFILE_NAME)
        .unwrap();
    let pid = pid.clone();
    let lid = profile.layer_order[0].clone();
    store
        .document_store
        .update(|draft| {
            for slot in 0..=5 {
                draft.unassign(&json!(pid), &json!(lid), slot)?;
                draft.assign(
                    &json!(pid),
                    &json!(lid),
                    slot,
                    Action {
                        id: format!("slot-open-{slot}"),
                        name: format!("会话 {}", slot + 1),
                        category: "Codex 会话".into(),
                        detail: "切换并回读对应状态的 Codex 会话".into(),
                        payload: json!({
                            "type": "agent_command",
                            "adapter": "four-app",
                            "command": format!("slot-open:{slot}")
                        }),
                    },
                )?;
            }
            for slot in [8, 9, 10] {
                draft.unassign(&json!(pid), &json!(lid), slot)?;
            }
            for (slot, id, name, detail, command) in [
                (
                    8,
                    "desktop-terminal",
                    "显示 / 隐藏终端",
                    "显示或隐藏终端（Ctrl+`）",
                    "terminal",
                ),
                (
                    9,
                    "desktop-workspace-layout",
                    "轮换工作区布局",
                    "轮换完整视图、分屏与隐藏标签页（Cmd+Shift+B）",
                    "cycle-workspace-layout",
                ),
                (
                    10,
                    "desktop-new",
                    "新建聊天",
                    "新建聊天（Cmd+N）",
                    "new-chat",
                ),
            ] {
                draft.assign(
                    &json!(pid),
                    &json!(lid),
                    slot,
                    Action {
                        id: id.into(),
                        name: name.into(),
                        category: "Agent".into(),
                        detail: detail.into(),
                        payload: json!({"type": "agent_command", "adapter": "codex", "command": command}),
                    },
                )?;
            }
            Ok(())
        })
        .unwrap();
    (pid, lid)
}

#[test]
fn opening_saved_r26_desktop_presets_migrates_unchanged_codex_and_preserves_custom_edits() {
    use mi_model::draft::Action;
    use mi_model::host_router::{resolve_host_action, ResolvedHostAction};
    use serde_json::json;

    let temp = TempDir::new().unwrap();
    let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();
    let (pid, _lid) = restore_r26_local_codex_starter(&mut store);
    let revision_before = store.document_store.draft.revision;
    std::fs::write(
        temp.path().join("workbench.json"),
        serde_json::to_vec(&store.document_store.draft.document).unwrap(),
    )
    .unwrap();
    drop(store);

    let mut reopened = WorkbenchStore::open_isolated(temp.path()).unwrap();
    assert!(reopened.has_all_desktop_agent_presets());
    assert_eq!(reopened.document_store.draft.revision, revision_before + 1);
    let doc = &reopened.document_store.draft.document;
    for (slot, command) in [
        (6, "live-voice"),
        (7, "focus-input"),
        (8, "new-tab"),
        (9, "cycle-workspace-layout"),
        (10, "new-chat"),
        (11, "toggle-dictation"),
        (12, "submit"),
    ] {
        assert_eq!(
            resolve_host_action(doc, Some(&pid), &format!("AG{slot:02}"), true),
            Some(ResolvedHostAction::AgentCommand {
                adapter: "codex".into(),
                command: command.into()
            })
        );
    }
    let migrated_bytes = std::fs::read(temp.path().join("workbench.json")).unwrap();
    let migrated_revision = reopened.document_store.draft.revision;
    reopened.set_status_message("已就绪");
    reopened.prepare_desktop_profiles().unwrap();
    assert_eq!(reopened.status_message(), "已就绪");
    assert_eq!(reopened.document_store.draft.revision, migrated_revision);
    drop(reopened);

    let reopened = WorkbenchStore::open_isolated(temp.path()).unwrap();
    assert_eq!(reopened.document_store.draft.revision, migrated_revision);
    assert_eq!(
        std::fs::read(temp.path().join("workbench.json")).unwrap(),
        migrated_bytes
    );

    let temp_custom = TempDir::new().unwrap();
    let mut store_custom = WorkbenchStore::open_isolated(temp_custom.path()).unwrap();
    let (cpid, clid) = restore_r26_local_codex_starter(&mut store_custom);
    store_custom
        .document_store
        .update(|draft| {
            draft.assign(
                &json!(cpid),
                &json!(clid),
                8,
                Action {
                    id: "custom-slot-8".into(),
                    name: "自定义 Slot 8".into(),
                    category: "快捷键".into(),
                    detail: "custom slot 8".into(),
                    payload: json!({
                        "type": "shortcut",
                        "key": "K",
                        "modifiers": ["command"],
                        "target": "com.openai.codex"
                    }),
                },
            )
        })
        .unwrap();
    let custom_before = serde_json::to_vec(&store_custom.document_store.draft.document).unwrap();
    std::fs::write(temp_custom.path().join("workbench.json"), &custom_before).unwrap();
    let custom_rev = store_custom.document_store.draft.revision;
    drop(store_custom);

    let mut reopened_custom = WorkbenchStore::open_isolated(temp_custom.path()).unwrap();
    assert!(reopened_custom.has_all_desktop_agent_presets());
    reopened_custom.set_status_message("已就绪");
    reopened_custom.prepare_desktop_profiles().unwrap();
    assert_eq!(reopened_custom.status_message(), "已就绪");
    assert_eq!(reopened_custom.document_store.draft.revision, custom_rev);
    assert_eq!(
        std::fs::read(temp_custom.path().join("workbench.json")).unwrap(),
        custom_before
    );
}

#[test]
fn opening_saved_desktop_presets_migrates_unchanged_antigravity_slot8_to_browser() {
    use mi_model::draft::Action;
    use mi_model::host_router::{resolve_host_action, ResolvedHostAction};
    use serde_json::json;

    let temp = TempDir::new().unwrap();
    let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();
    let (ag_pid, ag_lid) = {
        let doc = &store.document_store.draft.document;
        let (pid, profile) = doc
            .editable
            .graph
            .profiles
            .iter()
            .find(|(_, profile)| profile.name == "Antigravity")
            .unwrap();
        (pid.clone(), profile.layer_order[0].clone())
    };
    store
        .document_store
        .update(|draft| {
            draft.assign(
                &json!(ag_pid),
                &json!(ag_lid),
                8,
                Action {
                    id: "desktop-terminal".into(),
                    name: "打开终端面板".into(),
                    category: "Agent".into(),
                    detail: "切换底部终端面板（Cmd+J 或原生控件）".into(),
                    payload: json!({
                        "type": "agent_command",
                        "adapter": "antigravity",
                        "command": "terminal"
                    }),
                },
            )
        })
        .unwrap();
    let revision_before = store.document_store.draft.revision;
    std::fs::write(
        temp.path().join("workbench.json"),
        serde_json::to_vec(&store.document_store.draft.document).unwrap(),
    )
    .unwrap();
    drop(store);

    let mut reopened = WorkbenchStore::open_isolated(temp.path()).unwrap();
    assert!(reopened.has_all_desktop_agent_presets());
    assert_eq!(reopened.document_store.draft.revision, revision_before + 1);
    assert_eq!(
        resolve_host_action(
            &reopened.document_store.draft.document,
            Some(&ag_pid),
            "AG08",
            true
        ),
        Some(ResolvedHostAction::AgentCommand {
            adapter: "antigravity".into(),
            command: "browser".into(),
        })
    );
    let migrated_bytes = std::fs::read(temp.path().join("workbench.json")).unwrap();
    let migrated_revision = reopened.document_store.draft.revision;
    reopened.set_status_message("已就绪");
    reopened.prepare_desktop_profiles().unwrap();
    assert_eq!(reopened.status_message(), "已就绪");
    assert_eq!(reopened.document_store.draft.revision, migrated_revision);
    drop(reopened);

    let reopened = WorkbenchStore::open_isolated(temp.path()).unwrap();
    assert_eq!(reopened.document_store.draft.revision, migrated_revision);
    assert_eq!(
        std::fs::read(temp.path().join("workbench.json")).unwrap(),
        migrated_bytes
    );
}

#[test]
fn opening_saved_desktop_presets_migrates_antigravity_slot9_isolated_and_preserves_customized() {
    use mi_model::draft::Action;
    use mi_model::host_router::{resolve_host_action, ResolvedHostAction};
    use serde_json::json;

    let temp = TempDir::new().unwrap();
    let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();
    let (ag_pid, ag_lid) = {
        let doc = &store.document_store.draft.document;
        let (pid, profile) = doc
            .editable
            .graph
            .profiles
            .iter()
            .find(|(_, profile)| profile.name == "Antigravity")
            .unwrap();
        (pid.clone(), profile.layer_order[0].clone())
    };
    let mut shared_aid = String::new();
    store
        .document_store
        .update(|draft| {
            draft.assign(
                &json!(ag_pid),
                &json!(ag_lid),
                9,
                Action {
                    id: "desktop-split".into(),
                    name: "快速向右分栏".into(),
                    category: "Agent".into(),
                    detail: "将当前会话向右分栏（Cmd+D）".into(),
                    payload: json!({
                        "type": "agent_command",
                        "adapter": "antigravity",
                        "command": "split-right"
                    }),
                },
            )?;
            let layer = &draft.document.editable.graph.layers[&ag_lid];
            shared_aid = layer
                .controls
                .values()
                .find(|b| b.logical_control == "logical-9")
                .and_then(|b| b.action_id.as_ref())
                .unwrap()
                .clone();
            let bid12 = layer
                .controls
                .iter()
                .find(|(_, b)| b.logical_control == "logical-12")
                .map(|(bid, _)| bid.clone())
                .unwrap();
            draft.bind_existing_action(&ag_pid, &ag_lid, &bid12, Some(&shared_aid))
        })
        .unwrap();
    let revision_before = store.document_store.draft.revision;
    std::fs::write(
        temp.path().join("workbench.json"),
        serde_json::to_vec(&store.document_store.draft.document).unwrap(),
    )
    .unwrap();
    drop(store);

    let mut reopened = WorkbenchStore::open_isolated(temp.path()).unwrap();
    assert!(reopened.has_all_desktop_agent_presets());
    assert_eq!(reopened.document_store.draft.revision, revision_before + 1);
    assert_eq!(
        resolve_host_action(
            &reopened.document_store.draft.document,
            Some(&ag_pid),
            "AG09",
            true
        ),
        Some(ResolvedHostAction::AgentCommand {
            adapter: "antigravity".into(),
            command: "side-question".into(),
        })
    );
    assert_eq!(
        resolve_host_action(
            &reopened.document_store.draft.document,
            Some(&ag_pid),
            "AG12",
            true
        ),
        Some(ResolvedHostAction::AgentCommand {
            adapter: "antigravity".into(),
            command: "split-right".into(),
        })
    );
    {
        let doc = &reopened.document_store.draft.document;
        let layer = &doc.editable.graph.layers[&ag_lid];
        let new_aid9 = layer
            .controls
            .values()
            .find(|b| b.logical_control == "logical-9")
            .and_then(|b| b.action_id.as_ref())
            .unwrap();
        let aid12 = layer
            .controls
            .values()
            .find(|b| b.logical_control == "logical-12")
            .and_then(|b| b.action_id.as_ref())
            .unwrap();
        assert_ne!(new_aid9, &shared_aid);
        assert_eq!(aid12, &shared_aid);
        assert_eq!(
            doc.editable.graph.actions[new_aid9].id,
            "desktop-side-question"
        );
        assert_eq!(doc.editable.graph.actions[aid12].id, "desktop-split");
        assert_eq!(doc.editable.graph.actions[aid12].name, "快速向右分栏");
    }
    let migrated_bytes = std::fs::read(temp.path().join("workbench.json")).unwrap();
    let migrated_revision = reopened.document_store.draft.revision;
    reopened.set_status_message("已就绪");
    reopened.prepare_desktop_profiles().unwrap();
    assert_eq!(reopened.status_message(), "已就绪");
    assert_eq!(reopened.document_store.draft.revision, migrated_revision);
    drop(reopened);

    let reopened = WorkbenchStore::open_isolated(temp.path()).unwrap();
    assert_eq!(reopened.document_store.draft.revision, migrated_revision);
    assert_eq!(
        std::fs::read(temp.path().join("workbench.json")).unwrap(),
        migrated_bytes
    );
    drop(reopened);

    for (action_name, adapter) in [("我的向右分栏", "antigravity"), ("快速向右分栏", "codex")]
    {
        let custom_temp = TempDir::new().unwrap();
        let mut custom_store = WorkbenchStore::open_isolated(custom_temp.path()).unwrap();
        let (custom_pid, custom_lid) = {
            let doc = &custom_store.document_store.draft.document;
            let (pid, profile) = doc
                .editable
                .graph
                .profiles
                .iter()
                .find(|(_, profile)| profile.name == "Antigravity")
                .unwrap();
            (pid.clone(), profile.layer_order[0].clone())
        };
        custom_store
            .document_store
            .update(|draft| {
                draft.assign(
                    &json!(custom_pid),
                    &json!(custom_lid),
                    9,
                    Action {
                        id: "desktop-split".into(),
                        name: action_name.into(),
                        category: "Agent".into(),
                        detail: "custom split".into(),
                        payload: json!({
                            "type": "agent_command",
                            "adapter": adapter,
                            "command": "split-right"
                        }),
                    },
                )
            })
            .unwrap();
        let before = serde_json::to_vec(&custom_store.document_store.draft.document).unwrap();
        let custom_rev = custom_store.document_store.draft.revision;
        std::fs::write(custom_temp.path().join("workbench.json"), &before).unwrap();
        drop(custom_store);

        let mut custom_reopened = WorkbenchStore::open_isolated(custom_temp.path()).unwrap();
        assert!(custom_reopened.has_all_desktop_agent_presets());
        custom_reopened.set_status_message("已就绪");
        custom_reopened.prepare_desktop_profiles().unwrap();
        assert_eq!(custom_reopened.status_message(), "已就绪");
        assert_eq!(custom_reopened.document_store.draft.revision, custom_rev);
        assert_eq!(
            std::fs::read(custom_temp.path().join("workbench.json")).unwrap(),
            before
        );
        assert_eq!(
            resolve_host_action(
                &custom_reopened.document_store.draft.document,
                Some(&custom_pid),
                "AG09",
                true
            ),
            Some(ResolvedHostAction::AgentCommand {
                adapter: adapter.into(),
                command: "split-right".into(),
            })
        );
    }
}

#[test]
fn opening_saved_customized_local_codex_preserves_document_and_ready_status() {
    use serde_json::json;

    let temp = TempDir::new().unwrap();
    let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();
    let (pid, lid) = restore_previous_local_codex_starter(&mut store);
    store.document_store.update(|draft| {
        draft.assign(&json!(pid), &json!(lid), 7, mi_model::draft::Action {
            id: "custom-codex-shortcut".into(), name: "我的快捷键".into(),
            category: "快捷键".into(), detail: "custom".into(),
            payload: json!({"type":"shortcut", "key":"X", "modifiers":["command"], "target":"com.openai.codex"}),
        })
    }).unwrap();
    let before = serde_json::to_vec(&store.document_store.draft.document).unwrap();
    std::fs::write(temp.path().join("workbench.json"), &before).unwrap();
    let revision_before = store.document_store.draft.revision;
    drop(store);

    let mut reopened = WorkbenchStore::open_isolated(temp.path()).unwrap();
    assert!(reopened.has_all_desktop_agent_presets());
    reopened.set_status_message("已就绪");
    reopened.prepare_desktop_profiles().unwrap();
    assert_eq!(reopened.status_message(), "已就绪");
    assert_eq!(reopened.document_store.draft.revision, revision_before);
    assert_eq!(
        std::fs::read(temp.path().join("workbench.json")).unwrap(),
        before
    );
}

#[test]
fn retiring_cindy_keeps_a_saved_custom_profile() {
    use mi_model::storage::DocumentStore;
    use serde_json::json;

    let temp = TempDir::new().unwrap();
    let mut saved = DocumentStore::default();
    saved.open(temp.path().join("workbench.json")).unwrap();
    let mut custom_id = String::new();
    saved
        .update(|draft| {
            custom_id = draft.create_profile("Cindy")?;
            let layer_id =
                draft.document.editable.graph.profiles[&custom_id].layer_order[0].clone();
            draft.assign(
                &json!(custom_id),
                &json!(layer_id),
                0,
                mi_model::draft::Action {
                    id: "my-cindy-shortcut".into(),
                    name: "My shortcut".into(),
                    category: "快捷键".into(),
                    detail: String::new(),
                    payload: json!({
                        "type": "shortcut", "key": "k", "modifiers": ["command"],
                        "target": "com.xd.cindy"
                    }),
                },
            )
        })
        .unwrap();
    let layer_id = saved.draft.document.editable.graph.profiles[&custom_id].layer_order[0].clone();
    let original_layer = saved.draft.document.editable.graph.layers[&layer_id].clone();
    saved
        .update_routing(
            serde_json::from_value(json!({
                "document_id": saved.draft.document_id,
                "document_revision": saved.draft.revision,
                "host_state_revision": saved.host_revision(),
                "operation": { "kind": "select", "profile_id": custom_id }
            }))
            .unwrap(),
        )
        .unwrap();

    let mut upgraded = WorkbenchStore::open_isolated(temp.path()).unwrap();
    assert!(
        upgraded.document_store.draft.document.editable.graph.layers[&layer_id] == original_layer
    );
    assert_eq!(
        upgraded.document_store.host_active_profile_id().as_deref(),
        Some(custom_id.as_str())
    );
    let revision = upgraded.document_store.draft.revision;
    upgraded.prepare_desktop_profiles().unwrap();
    assert_eq!(upgraded.document_store.draft.revision, revision);
}

#[test]
fn foreground_app_changes_never_switch_active_host_profile_or_write_disk() {
    let temp = TempDir::new().unwrap();
    let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();

    let grokbot_id = store
        .sidebar_rows()
        .iter()
        .find(|row| row.canonical_agent == Some(CanonicalAgent::GrokBot))
        .unwrap()
        .profile_id
        .clone();
    store
        .dispatch_sidebar_action(SidebarAction::SelectProfile(grokbot_id.clone()))
        .unwrap();
    assert_eq!(
        store.projection().sidebar.selected_display_name.as_deref(),
        Some("Grok Bot")
    );
    assert_eq!(store.active_host_profile_id(), Some(grokbot_id.clone()));

    let host_rev_before = store.projection().host_revision;
    let doc_rev_before = store.projection().document_revision;
    for bundle in [
        "com.deepseek.dsh",
        "com.apple.Safari",
        "com.google.antigravity",
    ] {
        assert!(
            !store.foreground_changed(Some(bundle)),
            "foreground switch to {bundle} must never auto-switch host profile"
        );
        assert_eq!(store.projection().host_revision, host_rev_before);
        assert_eq!(store.projection().document_revision, doc_rev_before);
        assert_eq!(
            store.projection().sidebar.selected_display_name.as_deref(),
            Some("Grok Bot")
        );
        assert_eq!(store.active_host_profile_id(), Some(grokbot_id.clone()));
    }
}

#[test]
fn custom_profiles_filter_crud_undo_redo_and_preset_roundtrip_work_without_ffi() {
    let temp = TempDir::new().unwrap();
    let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();

    store
        .apply_profile_command(ProfileCommand::Create {
            name: "我的实验配置".to_string(),
        })
        .unwrap();
    assert!(
        store.projection().sidebar.show_other_profiles,
        "creating a custom profile must automatically reveal non-canonical profiles"
    );
    let custom_id = store
        .sidebar_rows()
        .iter()
        .find(|row| row.raw_name == "我的实验配置")
        .unwrap()
        .profile_id
        .clone();

    store.set_show_other_profiles(false);
    assert!(!store
        .sidebar_rows()
        .iter()
        .any(|row| row.profile_id == custom_id));
    store.set_show_other_profiles(true);
    assert!(store
        .sidebar_rows()
        .iter()
        .any(|row| row.profile_id == custom_id));

    store
        .apply_profile_command(ProfileCommand::Rename {
            profile_id: custom_id.clone(),
            name: "已重命名配置".to_string(),
        })
        .unwrap();
    assert!(store
        .sidebar_rows()
        .iter()
        .any(|row| row.profile_id == custom_id && row.display_name == "已重命名配置"));

    store.dispatch_header_action(HeaderAction::Undo).unwrap();
    assert!(store
        .sidebar_rows()
        .iter()
        .any(|row| row.profile_id == custom_id && row.display_name == "我的实验配置"));

    store.dispatch_header_action(HeaderAction::Redo).unwrap();
    assert!(store
        .sidebar_rows()
        .iter()
        .any(|row| row.profile_id == custom_id && row.display_name == "已重命名配置"));

    let exported = store
        .apply_backup_command(BackupCommand::ExportPreset {
            profile_id: Some(custom_id.clone()),
            layer_id: None,
        })
        .unwrap()
        .unwrap();
    assert!(exported.contains("已重命名配置"));

    store
        .apply_profile_command(ProfileCommand::Delete {
            profile_id: custom_id.clone(),
        })
        .unwrap();
    assert!(!store
        .sidebar_rows()
        .iter()
        .any(|row| row.profile_id == custom_id));

    store
        .apply_backup_command(BackupCommand::ImportPreset { text: exported })
        .unwrap();
    assert!(store.has_all_desktop_agent_presets());
}

#[test]
fn usb_connect_and_verified_input_pump_dispatches_host_actions_and_handles_disconnect() {
    use mi_core::{ControlSurfaceId, HeaderReadyState};
    use mi_proto::{Framer, NativeFramer};
    use mi_transport::{
        HidCandidate, HidLink, HidTransport, LinkBackend, TransportError, TransportHint, UsagePair,
    };
    use std::{
        future::Future,
        pin::Pin,
        sync::{Arc, Mutex},
    };
    use tokio::sync::{broadcast, mpsc};

    struct ChannelHidLink {
        rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<Result<Vec<u8>, TransportError>>>,
        closed_tx: mpsc::UnboundedSender<Result<Vec<u8>, TransportError>>,
    }

    impl HidLink for ChannelHidLink {
        fn backend(&self) -> LinkBackend {
            LinkBackend::MacHidapi267Numbered
        }

        fn write<'life0, 'life1, 'async_trait>(
            &'life0 self,
            _report: &'life1 [u8],
        ) -> Pin<Box<dyn Future<Output = Result<(), TransportError>> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async { Ok(()) })
        }

        fn read<'life0, 'async_trait>(
            &'life0 self,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, TransportError>> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move {
                let mut guard = self.rx.lock().await;
                guard
                    .recv()
                    .await
                    .unwrap_or(Err(TransportError::Disconnected))
            })
        }

        fn close(&self) {
            let _ = self.closed_tx.send(Err(TransportError::Disconnected));
        }
    }

    struct FakeUsbTransport {
        candidate: HidCandidate,
        link: Mutex<Option<Arc<dyn HidLink>>>,
        events: broadcast::Sender<()>,
    }

    impl HidTransport for FakeUsbTransport {
        fn enumerate(&self) -> Result<Vec<HidCandidate>, TransportError> {
            Ok(vec![self.candidate.clone()])
        }

        fn subscribe(&self) -> broadcast::Receiver<()> {
            self.events.subscribe()
        }

        fn open(&self, id: &str) -> Result<Arc<dyn HidLink>, TransportError> {
            assert_eq!(id, self.candidate.path);
            self.link
                .lock()
                .unwrap()
                .take()
                .ok_or_else(|| TransportError::Unavailable("already opened".to_string()))
        }
    }

    let temp = TempDir::new().unwrap();
    let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();
    assert!(store.device_session.is_none());
    assert!(store.projection().header.show_connect_button);

    let (report_tx, report_rx) = mpsc::unbounded_channel();
    let link: Arc<dyn HidLink> = Arc::new(ChannelHidLink {
        rx: tokio::sync::Mutex::new(report_rx),
        closed_tx: report_tx.clone(),
    });
    let (events, _) = broadcast::channel(8);
    let transport = FakeUsbTransport {
        candidate: HidCandidate {
            vid: 0x303A,
            pid: 0x8298,
            usage_page: 0xFF00,
            usage: 1,
            usage_pairs: vec![UsagePair {
                page: 0xFF00,
                usage: 1,
            }],
            transport_hint: TransportHint::Usb,
            product: "Creator Micro 2".to_string(),
            path: "IOService:/Synthetic/CreatorMicro2".to_string(),
            serial_number: None,
            location_id: None,
        },
        link: Mutex::new(Some(link)),
        events,
    };

    let connected = store.connect_with_transport(&transport, true).unwrap();
    assert!(
        connected,
        "single verified USB 303A:8298 candidate must connect"
    );
    assert!(store.device_session.is_some());
    assert!(!store.projection().header.show_connect_button);
    assert!(store.projection().header.host_execution_enabled);
    assert_eq!(
        store.projection().header.ready_state,
        HeaderReadyState::Ready {
            text: "小键盘已连接".to_string()
        }
    );

    let framer = NativeFramer::default();
    let press_ag12 = framer
        .encode(br#"{"m":"v.oai.hid","p":{"k":"AG12","act":1}}"#)
        .unwrap();
    for report in press_ag12 {
        report_tx.send(Ok(report)).unwrap();
    }
    let release_ag12 = framer
        .encode(br#"{"m":"v.oai.hid","p":{"k":"AG12","act":0}}"#)
        .unwrap();
    for report in release_ag12 {
        report_tx.send(Ok(report)).unwrap();
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let press_receipt = rt
        .block_on(async {
            tokio::time::timeout(
                Duration::from_secs(2),
                store.recv_next_hardware_event(Instant::now()),
            )
            .await
            .expect("timed out waiting for AG12 press")
        })
        .expect("channel open")
        .expect("AG12 press must produce a host action receipt");

    assert_eq!(press_receipt.binding_id, "AG12");
    assert_eq!(press_receipt.outcome, "simulated");
    assert_eq!(
        store.projection().canvas.selected_surface,
        ControlSurfaceId::Key12
    );

    let release_receipt = rt
        .block_on(async {
            tokio::time::timeout(
                Duration::from_secs(2),
                store.recv_next_hardware_event(Instant::now()),
            )
            .await
            .expect("timed out waiting for AG12 release")
        })
        .expect("channel open");
    assert!(release_receipt.is_none());

    report_tx.send(Err(TransportError::Disconnected)).unwrap();
    let disconnect_outcome = rt
        .block_on(async {
            tokio::time::timeout(
                Duration::from_secs(2),
                store.recv_next_hardware_event(Instant::now()),
            )
            .await
            .expect("timed out waiting for disconnect event")
        })
        .expect("channel open");
    assert!(disconnect_outcome.is_none());
    assert!(store.device_session.is_none());
    assert!(store.projection().header.show_connect_button);
}

#[test]
fn usb_connect_automatically_verifies_identity_snapshot_lighting_and_writes_acceptance_receipt() {
    check_usb_host_routing(false, 1, UsbRoutingCase::Normal);
}

#[test]
fn verified_board_layers_dispatch_only_to_their_own_host_profile() {
    check_usb_host_routing(true, 1, UsbRoutingCase::Normal);
}

#[test]
fn managed_usb_reconnect_blocks_other_layers_until_a_first_layer_report() {
    check_usb_host_routing(true, 2, UsbRoutingCase::Normal);
}

#[test]
fn recognizing_old_firmware_without_old_bindings_does_not_use_desired_layers() {
    check_usb_host_routing(true, 1, UsbRoutingCase::MissingBindings);
}

#[test]
fn unavailable_legacy_layer_updates_observation_and_recovers_on_a_valid_layer() {
    for profile in [None, Some("missing-profile"), Some("Codex"), Some("Cindy")] {
        check_usb_host_routing(true, 1, UsbRoutingCase::UnavailableSecondLayer(profile));
    }
}

#[test]
fn automatic_usb_acceptance_preserves_saved_host_state_without_migrating() {
    for managed in [false, true] {
        for profile in [None, Some("DeepSeek Coder (DSH)")] {
            check_usb_host_routing(managed, 1, UsbRoutingCase::SavedHost(profile));
        }
    }
}

#[derive(Clone, Copy)]
enum UsbRoutingCase {
    Normal,
    MissingBindings,
    UnavailableSecondLayer(Option<&'static str>),
    SavedHost(Option<&'static str>),
}

fn check_usb_host_routing(managed: bool, initial_layer: u32, case: UsbRoutingCase) {
    use mi_core::HardwareAutoAcceptanceReceipt;
    use mi_proto::{Framer, NativeFramer};
    use mi_transport::{
        HidCandidate, HidLink, HidTransport, LinkBackend, TransportError, TransportHint, UsagePair,
    };
    use serde_json::{json, Value};
    use std::{
        future::Future,
        pin::Pin,
        sync::{Arc, Mutex},
    };
    use tokio::sync::{broadcast, mpsc};

    fn b64_encode(input: &[u8]) -> String {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
        for chunk in input.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
            let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
            let triple = (b0 << 16) | (b1 << 8) | b2;
            out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
            out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
            if chunk.len() > 1 {
                out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
            } else {
                out.push('=');
            }
            if chunk.len() > 2 {
                out.push(TABLE[(triple & 0x3F) as usize] as char);
            } else {
                out.push('=');
            }
        }
        out
    }

    struct AutoAcceptHidLink {
        rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<Result<Vec<u8>, TransportError>>>,
        tx: mpsc::UnboundedSender<Result<Vec<u8>, TransportError>>,
        in_framer: Mutex<NativeFramer>,
        keymap_bytes: Vec<u8>,
        keymap_sha1: String,
        profile_index: u32,
        initial_layer: u32,
        methods_seen: Arc<Mutex<Vec<String>>>,
    }

    impl HidLink for AutoAcceptHidLink {
        fn backend(&self) -> LinkBackend {
            LinkBackend::MacHidapi267Numbered
        }

        fn is_exclusive_vendor(&self) -> bool {
            true
        }

        fn write<'life0, 'life1, 'async_trait>(
            &'life0 self,
            report: &'life1 [u8],
        ) -> Pin<Box<dyn Future<Output = Result<(), TransportError>> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            Self: 'async_trait,
        {
            let lines = self
                .in_framer
                .lock()
                .unwrap()
                .feed(report)
                .unwrap_or_default();
            for line in lines {
                let req: Value = serde_json::from_slice(&line).unwrap();
                let id = req["id"].clone();
                let method = req["method"].as_str().unwrap_or("").to_string();
                self.methods_seen.lock().unwrap().push(method.clone());
                let reply = match method.as_str() {
                    "device.status" => json!({
                        "id": id,
                        "result": {
                            "version": "0.6.3",
                            "profile_index": self.profile_index,
                            "layer_index": self.initial_layer
                        }
                    }),
                    "fs.list" => json!({
                        "id": id,
                        "result": [
                            {
                                "name": "keymap.json",
                                "size": self.keymap_bytes.len(),
                                "checksum": self.keymap_sha1
                            }
                        ]
                    }),
                    "fs.readbin" => {
                        let offset = req["params"]["offset"].as_u64().unwrap_or(0) as usize;
                        let len = req["params"]["len"].as_u64().unwrap_or(3072) as usize;
                        let end = (offset + len).min(self.keymap_bytes.len());
                        let slice = &self.keymap_bytes[offset..end];
                        json!({
                            "id": id,
                            "result": {
                                "total_size": self.keymap_bytes.len(),
                                "data": b64_encode(slice)
                            }
                        })
                    }
                    "v.oai.rgbcfg" | "lights.preview" | "v.oai.thstatus" => json!({
                        "id": id,
                        "result": {}
                    }),
                    other => panic!("unexpected RPC method: {other}"),
                };
                for out_report in NativeFramer::default()
                    .encode(reply.to_string().as_bytes())
                    .unwrap()
                {
                    let _ = self.tx.send(Ok(out_report));
                }
            }
            Box::pin(async { Ok(()) })
        }

        fn read<'life0, 'async_trait>(
            &'life0 self,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, TransportError>> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move {
                let mut guard = self.rx.lock().await;
                guard
                    .recv()
                    .await
                    .unwrap_or(Err(TransportError::Disconnected))
            })
        }

        fn close(&self) {
            let _ = self.tx.send(Err(TransportError::Disconnected));
        }
    }

    struct AutoAcceptTransport {
        candidate: HidCandidate,
        link: Mutex<Option<Arc<dyn HidLink>>>,
        events: broadcast::Sender<()>,
    }

    impl HidTransport for AutoAcceptTransport {
        fn enumerate(&self) -> Result<Vec<HidCandidate>, TransportError> {
            Ok(vec![self.candidate.clone()])
        }

        fn subscribe(&self) -> broadcast::Receiver<()> {
            self.events.subscribe()
        }

        fn open(&self, id: &str) -> Result<Arc<dyn HidLink>, TransportError> {
            assert_eq!(id, self.candidate.path);
            self.link
                .lock()
                .unwrap()
                .take()
                .ok_or_else(|| TransportError::Unavailable("already opened".to_string()))
        }
    }

    let temp = TempDir::new().unwrap();
    let mut store = legacy_store(&temp);
    let missing_bindings = matches!(case, UsbRoutingCase::MissingBindings);
    let preserve_saved_host = matches!(case, UsbRoutingCase::SavedHost(_));
    let path = temp.path().join("workbench.preferences.json");
    if !matches!(case, UsbRoutingCase::Normal) {
        let mut preferences: Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let profile_id = |name: &str| {
            store
                .document_store
                .draft
                .document
                .editable
                .graph
                .profiles
                .iter()
                .find(|(_, profile)| profile.name == name)
                .map(|(id, _)| id.clone())
                .unwrap_or_else(|| name.to_string())
        };
        match case {
            UsbRoutingCase::MissingBindings => {
                preferences
                    .as_object_mut()
                    .unwrap()
                    .remove("hardware_profile_slots");
            }
            UsbRoutingCase::UnavailableSecondLayer(profile) => {
                preferences["hardware_profile_slots"][2] = json!(profile.map(profile_id));
            }
            UsbRoutingCase::SavedHost(profile) => {
                preferences["host_active_profile_id"] = json!(profile.map(profile_id));
                preferences["host_execution_enabled"] = json!(false);
            }
            UsbRoutingCase::Normal => unreachable!(),
        }
        drop(store);
        std::fs::write(&path, serde_json::to_vec_pretty(&preferences).unwrap()).unwrap();
        store = WorkbenchStore::open_isolated(temp.path()).unwrap();
    }
    let original_bindings = store.document_store.legacy_three_bindings().cloned();
    if !preserve_saved_host {
        store.reset_board_layer_plan().unwrap();
        store.assign_board_layer(BoardLayer::ALL[1], None).unwrap();
    }
    drop(store);
    let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();
    let original_preferences = std::fs::read(&path).unwrap();
    let original_routing = store.document_store.routing_state();
    assert_eq!(
        store.document_store.legacy_three_bindings(),
        original_bindings.as_ref()
    );

    let mut raw_demo = mi_model::demo().unwrap().raw;
    raw_demo["version"] = json!(1);
    raw_demo["activeProfileId"] = json!(0);
    let raw_keymap = serde_json::to_vec(&raw_demo).unwrap();
    let (keymap_bytes, profile_index) = if managed {
        match mi_model::apply::compile_three_layer_profile(&raw_keymap, None).unwrap() {
            mi_model::apply::ThreeLayerProvision::Write {
                keymap,
                profile_index,
            } => (keymap, profile_index),
            _ => panic!("fixture did not add a board profile"),
        }
    } else {
        (raw_keymap, 0)
    };
    let keymap_file = mi_model::operation_record::file_value(&keymap_bytes);
    let keymap_sha1 = keymap_file["sha1"].as_str().unwrap().to_string();

    let (report_tx, report_rx) = mpsc::unbounded_channel();
    let methods_seen = Arc::new(Mutex::new(Vec::new()));
    let link: Arc<dyn HidLink> = Arc::new(AutoAcceptHidLink {
        rx: tokio::sync::Mutex::new(report_rx),
        tx: report_tx.clone(),
        in_framer: Mutex::new(NativeFramer::default()),
        keymap_bytes: keymap_bytes.clone(),
        keymap_sha1: keymap_sha1.clone(),
        profile_index,
        initial_layer,
        methods_seen: Arc::clone(&methods_seen),
    });
    let (events, _) = broadcast::channel(8);
    let transport = AutoAcceptTransport {
        candidate: HidCandidate {
            vid: 0x303A,
            pid: 0x8298,
            usage_page: 0xFF00,
            usage: 1,
            usage_pairs: vec![UsagePair {
                page: 0xFF00,
                usage: 1,
            }],
            transport_hint: TransportHint::Usb,
            product: "Creator Micro 2".to_string(),
            path: "IOService:/Synthetic/AutoAcceptCreatorMicro2".to_string(),
            serial_number: None,
            location_id: None,
        },
        link: Mutex::new(Some(link)),
        events,
    };

    assert!(store
        .connect_with_transport(&transport, !preserve_saved_host)
        .unwrap());

    let wait_start = Instant::now();
    while !store.hardware_rpc_verified && wait_start.elapsed() < Duration::from_secs(5) {
        let _ = store.poll_hardware_events();
        std::thread::sleep(Duration::from_millis(10));
    }

    assert!(
        store.hardware_rpc_verified,
        "connecting a responsive USB 303A:8298 device must automatically complete hardware acceptance"
    );
    assert!(
        store.status_message().contains(if missing_bindings {
            "未确认"
        } else if initial_layer != 1 {
            "等待键盘返回第 1 层"
        } else if managed {
            "板载三层已就绪"
        } else {
            "固件 0.6.3 自动验收通过"
        }),
        "unexpected status_message: {}",
        store.status_message()
    );

    let receipt_path = temp.path().join("hardware-auto-acceptance.json");
    assert!(
        receipt_path.is_file(),
        "hardware-auto-acceptance.json must be written automatically on connect"
    );
    let receipt: HardwareAutoAcceptanceReceipt =
        serde_json::from_slice(&std::fs::read(&receipt_path).unwrap()).unwrap();
    assert_eq!(receipt.status, "PASS");
    assert!(receipt.exclusive_vendor);
    assert_eq!(receipt.firmware_version, "0.6.3");
    assert_eq!(receipt.profile_index, Some(profile_index));
    assert_eq!(receipt.layer_index, Some(initial_layer));
    assert_eq!(receipt.keymap_size, keymap_bytes.len() as u64);
    assert_eq!(receipt.keymap_sha1, keymap_sha1);
    assert_eq!(receipt.keymap_sha256, mi_core::sha256_hex(&keymap_bytes));
    assert!(receipt.lighting_rgbcfg_synced);
    assert!(receipt.lighting_preview_synced);
    assert!(receipt.lighting_thstatus_synced);

    let seen = methods_seen.lock().unwrap().clone();
    assert!(seen.contains(&"device.status".to_string()));
    assert!(seen.contains(&"fs.list".to_string()));
    assert!(seen.contains(&"fs.readbin".to_string()));
    assert!(seen.contains(&"v.oai.rgbcfg".to_string()));
    assert!(seen.contains(&"lights.preview".to_string()));
    assert!(seen.contains(&"v.oai.thstatus".to_string()));

    assert_eq!(
        store.projection().sidebar.board_layers.observed_layer,
        if managed && initial_layer == 1 && !missing_bindings {
            Some(1)
        } else {
            None
        }
    );
    if preserve_saved_host {
        assert_eq!(store.document_store.routing_state(), original_routing);
        assert!(!store.document_store.host_execution_enabled());
        assert_eq!(std::fs::read(&path).unwrap(), original_preferences);
        assert!(!temp
            .path()
            .join("workbench.preferences.v1.backup.json")
            .exists());
        return;
    }
    let directory_entries = || {
        let mut names: Vec<_> = std::fs::read_dir(temp.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        names.sort();
        names
    };
    let before_files = directory_entries();
    let before = methods_seen.lock().unwrap().len();
    let generation = store.projection_generation();
    assert!(store
        .provision_board_layers()
        .unwrap_err()
        .to_string()
        .contains("六层"));
    assert_eq!(methods_seen.lock().unwrap().len(), before);
    assert_eq!(store.projection_generation(), generation);
    assert_eq!(directory_entries(), before_files);
    if missing_bindings {
        let mut event = board_test_event("AG00", mi_session::InputEventKind::Pressed);
        event.session_id = store.device_session.as_ref().unwrap().id;
        event.board_layer = Some(1);
        let active = store.active_host_profile_id();
        assert!(store
            .handle_hardware_loop_event(mi_core::HardwareLoopEvent::Input(event), Instant::now())
            .is_none());
        assert_eq!(store.active_host_profile_id(), active);
        assert!(store.dispatched_receipts().is_empty());
        return;
    }

    if matches!(case, UsbRoutingCase::UnavailableSecondLayer(_)) {
        let session = store.device_session.as_ref().unwrap();
        let session_id = session.id;
        let ingress_sequence = session.input_watermark() + 1;
        let active = store.active_host_profile_id();
        let mut event = board_test_event("AG00", mi_session::InputEventKind::Pressed);
        event.session_id = session_id;
        event.ingress_sequence = ingress_sequence;
        event.board_layer = Some(2);
        assert!(store
            .handle_hardware_loop_event(mi_core::HardwareLoopEvent::Input(event), Instant::now())
            .is_none());
        assert_eq!(store.active_host_profile_id(), active);
        assert!(store.dispatched_receipts().is_empty());
        assert_eq!(
            store.projection().sidebar.board_layers.observed_layer,
            Some(2)
        );
        assert!(store
            .projection()
            .sidebar
            .board_layers
            .observed_status
            .contains("按键已暂停"));
        store.preview_board_layer(BoardLayer::ALL[5]).unwrap();
        assert_eq!(
            store.projection().sidebar.board_layers.observed_layer,
            Some(2)
        );

        let mut event = board_test_event("AG07", mi_session::InputEventKind::Pressed);
        event.session_id = session_id;
        event.ingress_sequence = ingress_sequence + 1;
        event.board_layer = Some(1);
        let receipt = store
            .handle_hardware_loop_event(mi_core::HardwareLoopEvent::Input(event), Instant::now())
            .unwrap();
        assert!(receipt.succeeded());
        assert_eq!(receipt.target, "antigravity");
        assert_eq!(store.dispatched_receipts().len(), 1);
        assert_eq!(
            store.projection().sidebar.board_layers.observed_layer,
            Some(1)
        );
        assert!(!store
            .projection()
            .sidebar
            .board_layers
            .observed_status
            .contains("按键已暂停"));
        assert!(!store.status_message().contains("按键已暂停"));
        return;
    }

    let send = |token| {
        for act in [1, 0] {
            let event = json!({"m":"v.oai.hid","p":{"k":token,"act":act}});
            for report in NativeFramer::default()
                .encode(event.to_string().as_bytes())
                .unwrap()
            {
                report_tx.send(Ok(report)).unwrap();
            }
        }
    };
    if initial_layer != 1 {
        send("ACT00");
    }
    for (step, (slot, managed_token, unmanaged_token, target)) in [
        (1, "AG07", "AG07", "antigravity"),
        (2, "ACT07", "AG07", "codex"),
        (3, "AG13", "AG00", "dsh-desktop"),
        (1, "AG07", "AG07", "antigravity"),
    ]
    .into_iter()
    .enumerate()
    {
        if step != 0 && !managed {
            let pid = original_bindings.as_ref().unwrap().0[slot - 1]
                .as_ref()
                .unwrap();
            store.select_and_activate_profile(pid).unwrap();
        }
        assert!(!store.is_selected_layer_protected());
        let count = store.dispatched_receipts().len();
        send(if managed {
            managed_token
        } else {
            unmanaged_token
        });
        let start = Instant::now();
        while store.dispatched_receipts().len() == count && start.elapsed() < Duration::from_secs(2)
        {
            store.poll_hardware_events();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(store.dispatched_receipts().len(), count + 1);
        let routed = store.last_host_action_receipt().unwrap().clone();
        assert_eq!(routed.target, target);
        assert!(routed.succeeded());
        assert_eq!(
            store.active_host_profile_id().as_deref(),
            Some(routed.profile_id.as_str())
        );
        assert!(store.connect_with_transport(&transport, true).unwrap());
        assert_eq!(
            store.projection().sidebar.board_layers.observed_layer,
            if managed { Some(slot as u8) } else { None }
        );
        store.preview_board_layer(BoardLayer::ALL[5]).unwrap();
        store.reset_board_layer_plan().unwrap();
        assert_eq!(
            store.active_host_profile_id().as_deref(),
            Some(routed.profile_id.as_str())
        );
    }
    let session_id = store.device_session.as_ref().unwrap().id;
    store.handle_hardware_loop_event(
        mi_core::HardwareLoopEvent::Disconnected { session_id },
        Instant::now(),
    );
    assert_eq!(store.projection().sidebar.board_layers.observed_layer, None);
    assert_eq!(
        store.document_store.legacy_three_bindings(),
        original_bindings.as_ref()
    );
}

#[test]
fn six_layer_defaults_and_edits_preserve_running_target_and_empty_slots() {
    let temp = TempDir::new().unwrap();
    let mut store = legacy_store(&temp);
    let legacy = store.document_store.legacy_three_bindings().cloned();
    let usb = store
        .document_store
        .usb_startup_profile_id()
        .map(str::to_owned);
    let active = store.active_host_profile_id();
    let host_revision = store.document_store.host_revision();
    store.reset_board_layer_plan().unwrap();
    let board = &store.projection().sidebar.board_layers;
    assert_eq!(
        board
            .layers
            .iter()
            .map(|r| r.display_name.as_str())
            .collect::<Vec<_>>(),
        [
            "Codex",
            "Antigravity",
            "DeepSeek Coder (DSH)",
            "未分配",
            "未分配",
            "未分配"
        ]
    );
    assert_eq!(board.layers[0].owner, "Codex");
    for layer in BoardLayer::ALL {
        store.preview_board_layer(layer).unwrap();
        assert_eq!(store.projection().canvas.touch_led_states, layer.leds());
        assert_eq!(store.active_host_profile_id(), active);
        assert_eq!(store.document_store.host_revision(), host_revision);
        assert_eq!(
            store.document_store.legacy_three_bindings(),
            legacy.as_ref()
        );
        assert_eq!(
            store.document_store.usb_startup_profile_id(),
            usb.as_deref()
        );
        assert_eq!(store.projection().sidebar.board_layers.observed_layer, None);
    }
    let native = store
        .find_canonical_profile_id(CanonicalAgent::Codex)
        .unwrap();
    store
        .assign_board_layer(BoardLayer::ALL[4], Some(native))
        .unwrap();
    assert_eq!(
        store.projection().sidebar.board_layers.layers[4].owner,
        "Codex"
    );
    store.assign_board_layer(BoardLayer::ALL[4], None).unwrap();
    let before = std::fs::read(temp.path().join("workbench.preferences.json")).unwrap();
    drop(store);
    let store = WorkbenchStore::open_isolated(temp.path()).unwrap();
    assert_eq!(
        std::fs::read(temp.path().join("workbench.preferences.json")).unwrap(),
        before
    );
    assert_eq!(
        store.document_store.board_preview_selection(),
        BoardPreviewSelection::Layer(BoardLayer::ALL[5])
    );
    assert_eq!(
        store
            .document_store
            .board_layer_plan()
            .unwrap()
            .route(BoardLayer::ALL[4]),
        &BoardLayerRoute::Unassigned
    );
    assert_eq!(
        store.document_store.legacy_three_bindings(),
        legacy.as_ref()
    );
}

#[test]
fn failed_board_edits_leave_projection_preferences_and_host_state_unchanged() {
    let temp = TempDir::new().unwrap();
    let mut store = legacy_store(&temp);
    let before = store.projection().clone();
    let plan = store.document_store.board_layer_plan().cloned();
    let path = temp.path().join("workbench.preferences.json");
    std::fs::write(path.with_extension("pending"), b"occupied").unwrap();
    assert!(store.preview_board_layer(BoardLayer::ALL[5]).is_err());
    assert!(store.reset_board_layer_plan().is_err());
    assert_eq!(store.projection(), &before);
    assert_eq!(store.document_store.board_layer_plan(), plan.as_ref());
}

#[test]
fn six_layer_preview_cannot_activate_an_unassigned_or_missing_host() {
    let temp = TempDir::new().unwrap();
    let mut store = legacy_store(&temp);
    store.document_store.set_host_active_profile(None).unwrap();
    store.config.isolated = false;
    store.radial_state.set_enabled(true);
    for layer in [BoardLayer::ALL[0], BoardLayer::ALL[3], BoardLayer::ALL[5]] {
        store.preview_board_layer(layer).unwrap();
        let event = mi_session::InputEvent {
            session_id: 1,
            board_layer: None,
            sequence: 1,
            ingress_sequence: 1,
            radial_center: None,
            radial_position: None,
            timestamp_ms: 0,
            kind: mi_session::InputEventKind::Pressed,
            logical_switch: None,
            binding_id: Some("AG00".into()),
            physical_control: None,
            keycap_group: None,
            action_role: None,
            lost_count: 0,
            action_eligible: true,
        };
        assert!(store
            .consume_physical_input_event(event, Instant::now())
            .is_none());
        let mut radial = board_test_event("AG00", mi_session::InputEventKind::UnknownNotification);
        radial.binding_id = None;
        radial.radial_position = Some(mi_session::RadialPosition {
            angle_turns: 0.2,
            distance: 0.8,
        });
        assert!(store
            .consume_physical_input_event(radial, Instant::now())
            .is_none());
        assert!(!store.ptt_controller.is_active);
        assert!(store.dispatched_receipts().is_empty());
    }
    let profile_id = store
        .find_canonical_profile_id(CanonicalAgent::Antigravity)
        .unwrap();
    store
        .document_store
        .set_host_active_profile(Some(profile_id))
        .unwrap();
    assert_eq!(store.selected_profile_id(), None);
    store.set_status_message("normal reconciliation positive control with an empty editor");
}

fn board_test_event(binding: &str, kind: mi_session::InputEventKind) -> mi_session::InputEvent {
    mi_session::InputEvent {
        session_id: 1,
        board_layer: None,
        sequence: 1,
        ingress_sequence: 1,
        radial_center: None,
        radial_position: None,
        timestamp_ms: 0,
        kind,
        logical_switch: None,
        binding_id: Some(binding.into()),
        physical_control: None,
        keycap_group: None,
        action_role: None,
        lost_count: 0,
        action_eligible: true,
    }
}

#[test]
fn desired_board_edits_leave_held_ptt_for_the_original_release() {
    let temp = TempDir::new().unwrap();
    let mut store = legacy_store(&temp);
    let active = store.active_host_profile_id();
    store.consume_physical_input_event(
        board_test_event("AG11", mi_session::InputEventKind::Pressed),
        Instant::now(),
    );
    assert!(store.ptt_controller.is_active);
    let receipts = store.dispatched_receipts().len();
    store.reset_board_layer_plan().unwrap();
    let native = store
        .find_canonical_profile_id(CanonicalAgent::Codex)
        .unwrap();
    store
        .assign_board_layer(BoardLayer::ALL[0], Some(native))
        .unwrap();
    store.preview_board_layer(BoardLayer::ALL[0]).unwrap();
    assert!(store.ptt_controller.is_active);
    assert_eq!(store.active_host_profile_id(), active);
    assert_eq!(store.dispatched_receipts().len(), receipts);
    store.consume_physical_input_event(
        board_test_event("AG11", mi_session::InputEventKind::Released),
        Instant::now(),
    );
    assert!(!store.ptt_controller.is_active);
}

#[test]
fn a_forged_host_route_does_not_grant_execution_to_unknown_protected_owners() {
    let temp = TempDir::new().unwrap();
    let mut store = legacy_store(&temp);
    let id = store
        .find_canonical_profile_id(CanonicalAgent::Codex)
        .unwrap();
    store
        .document_store
        .draft
        .document
        .editable
        .graph
        .profiles
        .get_mut(&id)
        .unwrap()
        .name = "Unknown protected owner".into();
    let mut plan = store.document_store.board_layer_plan().unwrap().clone();
    plan.replace(
        BoardLayer::ALL[0],
        BoardLayerRoute::HostProfile {
            profile_id: id.clone(),
        },
    );
    store
        .document_store
        .save_board_configuration(plan, BoardPreviewSelection::Layer(BoardLayer::ALL[0]), None)
        .unwrap();
    store
        .document_store
        .set_host_active_profile(Some(id))
        .unwrap();
    store.preview_board_layer(BoardLayer::ALL[0]).unwrap();
    assert!(store.projection().sidebar.board_layers.layers[0]
        .assignment_issue
        .is_some());
    assert!(store
        .consume_physical_input_event(
            board_test_event("AG00", mi_session::InputEventKind::Pressed),
            Instant::now()
        )
        .is_none());
    assert!(!store.ptt_controller.is_active);
    assert!(store.dispatched_receipts().is_empty());
}

#[test]
fn opening_missing_board_references_preserves_them_and_all_unrelated_preferences() {
    let temp = TempDir::new().unwrap();
    let store = legacy_store(&temp);
    drop(store);
    let path = temp.path().join("workbench.preferences.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["hardware_profile_slots"] =
        serde_json::json!(["missing-usb", "missing-first", null, "missing-third"]);
    value["host_active_profile_id"] = serde_json::Value::Null;
    value["app_profile_links"] = serde_json::json!({"preserved.app":"missing-first"});
    value["auto_follow_enabled"] = serde_json::json!(true);
    value["manual_override"] = serde_json::json!(true);
    let original = serde_json::to_vec_pretty(&value).unwrap();
    std::fs::write(&path, &original).unwrap();
    let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert!(store.active_host_profile_id().is_none());
    let board = &store.projection().sidebar.board_layers;
    assert_eq!(board.layers[0].profile_id.as_deref(), Some("missing-first"));
    assert_eq!(board.layers[0].display_name, "缺失的方案");
    assert!(board.layers[1].profile_id.is_none());
    assert_eq!(board.usb_startup_profile_id.as_deref(), Some("missing-usb"));
    store.preview_board_layer(BoardLayer::ALL[5]).unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert!(saved.get("app_profile_links").is_none());
    assert!(saved.get("auto_follow_enabled").is_none());
    assert!(saved.get("manual_override").is_none());
    assert_eq!(
        saved["usb_startup_profile_id"],
        serde_json::json!("missing-usb")
    );
    assert_eq!(
        saved["legacy_three_bindings"],
        serde_json::json!(["missing-first", null, "missing-third"])
    );
    assert_eq!(
        saved["host_active_profile_id"],
        value["host_active_profile_id"]
    );
}

#[test]
#[ignore = "requires live Creator Micro 2 (303A:8298), Karabiner 16.3.0"]
fn live_six_layer_hardware_and_karabiner_acceptance() {
    use mi_core::WorkbenchStoreConfig;
    use mi_transport::HidTransport;
    use std::path::PathBuf;

    let home = PathBuf::from(std::env::var("HOME").expect("HOME env required"));
    let doc_dir = home.join("Library/Application Support/MicroInput");
    let three_layer_backups = doc_dir.join("three-layer-device-backups");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    if three_layer_backups.is_dir() {
        for entry in std::fs::read_dir(&three_layer_backups).unwrap().flatten() {
            if entry.path().join("manifest.json").is_file() {
                let target = mi_transport::discover_target_keyboard().unwrap();
                let transport = mi_transport::MacTransport::exclusive_vendor();
                let link = transport.open(&target.vendor_path).unwrap();
                rt.block_on(async {
                    let session = mi_session::DeviceSession::new(
                        link,
                        Box::new(mi_proto::NativeFramer::default()),
                        std::sync::Arc::new(mi_session::ReadOnlySchema),
                        mi_session::SessionConfig::default(),
                    );
                    session
                        .restore_three_layer_backup(&entry.path())
                        .await
                        .unwrap();
                    session.disconnect();
                });
                drop(transport);
                std::thread::sleep(Duration::from_millis(300));
            }
        }
    }

    let make_config = || {
        let mut cfg = WorkbenchStoreConfig::production();
        cfg.auto_connect_usb = false;
        cfg
    };

    let wait_for =
        |store: &mut WorkbenchStore, pred: &dyn Fn(&WorkbenchStore) -> bool, label: &str| {
            let deadline = Instant::now() + Duration::from_secs(25);
            while Instant::now() < deadline {
                store.poll_hardware_events();
                if pred(store) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            panic!(
                "timed out waiting for {label}; status_message={}",
                store.status_message()
            );
        };

    let mut store = WorkbenchStore::open(make_config()).unwrap();
    store.poll_hardware_events();
    std::thread::sleep(Duration::from_millis(50));
    store.poll_hardware_events();
    store
        .dispatch_sidebar_action(SidebarAction::ResetBoardLayerPlan)
        .unwrap();
    store
        .dispatch_sidebar_action(SidebarAction::ProvisionBoardLayers)
        .unwrap();
    wait_for(
        &mut store,
        &|s| {
            let b = &s.projection().sidebar.board_layers;
            b.configuration_observed && b.keyboard_grabbed && b.runtime_ready
        },
        "initial six-layer apply",
    );

    let first_state_bytes = std::fs::read(doc_dir.join("six-layer-runtime.json")).unwrap();
    let first_state: serde_json::Value = serde_json::from_slice(&first_state_bytes).unwrap();
    assert_eq!(first_state["installation"]["status"], "Applied");
    let first_backup = PathBuf::from(first_state["installation"]["backup"].as_str().unwrap());
    let orig_keymap = std::fs::read(first_backup.join("keymap.json")).unwrap();
    let planned_keymap = std::fs::read(first_backup.join("planned-keymap.json")).unwrap();
    assert_eq!(orig_keymap.len(), 7421);
    assert!(planned_keymap.len() > orig_keymap.len());
    let first_installed_karabiner = first_state["installation"]["config"]["installed"].clone();

    drop(store);
    std::thread::sleep(Duration::from_millis(300));

    let mut reconnect_store = WorkbenchStore::open(make_config()).unwrap();
    reconnect_store.connect_default_usb().unwrap();
    wait_for(
        &mut reconnect_store,
        &|s| {
            let b = &s.projection().sidebar.board_layers;
            b.configuration_observed && b.keyboard_grabbed && b.runtime_ready
        },
        "read-only reconnect resume",
    );
    let resumed_state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(doc_dir.join("six-layer-runtime.json")).unwrap())
            .unwrap();
    assert_eq!(
        resumed_state["installation"]["backup"],
        first_state["installation"]["backup"]
    );
    assert_ne!(
        resumed_state["installation"]["config"]["installed"],
        first_installed_karabiner
    );

    reconnect_store
        .dispatch_sidebar_action(SidebarAction::RestoreBoardLayers)
        .unwrap();
    wait_for(
        &mut reconnect_store,
        &|s| {
            !s.projection().sidebar.board_layers.restore_available
                && s.status_message() == "原设备配置及 Karabiner 设置已恢复"
        },
        "restore original board and Karabiner config",
    );
    let restored_state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(doc_dir.join("six-layer-runtime.json")).unwrap())
            .unwrap();
    assert!(restored_state["installation"].is_null());

    reconnect_store
        .dispatch_sidebar_action(SidebarAction::ProvisionBoardLayers)
        .unwrap();
    wait_for(
        &mut reconnect_store,
        &|s| {
            let b = &s.projection().sidebar.board_layers;
            b.configuration_observed && b.keyboard_grabbed && b.runtime_ready
        },
        "final six-layer re-apply for daily use",
    );
    let final_state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(doc_dir.join("six-layer-runtime.json")).unwrap())
            .unwrap();
    assert_eq!(final_state["installation"]["status"], "Applied");
    println!(
        "LIVE_SIX_LAYER_ACCEPTANCE_PASS profile_index={} observed_layer={:?} backup={}",
        final_state["installation"]["profile_index"],
        reconnect_store
            .projection()
            .sidebar
            .board_layers
            .observed_layer,
        final_state["installation"]["backup"]
    );
}

#[test]
fn workspace_sleep_command_retires_a_held_dictation_without_editing_the_draft() {
    let temp = TempDir::new().unwrap();
    let mut store = legacy_store(&temp);
    store.consume_physical_input_event(
        board_test_event("AG11", mi_session::InputEventKind::Pressed),
        Instant::now(),
    );
    assert!(store.ptt_controller.is_active);
    let original = store.document_store.draft.document.editable.clone();
    store
        .document_store
        .update(|draft| draft.create_profile("Sleep draft").map(|_| ()))
        .unwrap();
    let before = serde_json::to_value(&store.document_store.draft.document).unwrap();
    let command = serde_json::from_value::<mi_core::WorkbenchCommand>(
        serde_json::json!({"domain": "system_will_sleep"}),
    );
    assert!(
        command.is_ok(),
        "system_will_sleep must reach the Rust lifecycle"
    );
    store.dispatch_command(command.unwrap()).unwrap();
    assert!(
        !store.ptt_controller.is_active,
        "sleep must stop held dictation"
    );
    assert_eq!(
        before,
        serde_json::to_value(&store.document_store.draft.document).unwrap()
    );
    store.system_did_wake();
    let deadline = Instant::now() + Duration::from_secs(3);
    while store.status_message().contains("等待旧会话") {
        assert!(Instant::now() < deadline);
        store.poll_hardware_events();
        std::thread::sleep(Duration::from_millis(2));
    }
    store
        .document_store
        .update(|draft| draft.undo(false))
        .unwrap();
    assert!(original == store.document_store.draft.document.editable);
    store
        .document_store
        .update(|draft| draft.undo(true))
        .unwrap();
    assert!(store
        .document_store
        .draft
        .document
        .editable
        .graph
        .profiles
        .values()
        .any(|p| p.name == "Sleep draft"));
}

#[test]
fn browsing_profiles_changes_editor_without_activating_host() {
    let temp = TempDir::new().unwrap();
    let mut store = legacy_store(&temp);
    let codex = store
        .find_canonical_profile_id(CanonicalAgent::Codex)
        .unwrap();
    let active = store.active_host_profile_id();
    let revision = store.document_store.host_revision();
    let routing = store.projection().status_menu.routing_mode_summary.clone();
    let banner = store.projection().header.banner.clone();
    store
        .dispatch_sidebar_action(SidebarAction::BrowseProfile(codex.clone()))
        .unwrap();
    assert_eq!(store.selected_profile_id(), Some(codex.as_str()));
    assert_eq!(
        store.projection().canvas.profile_id.as_deref(),
        Some(codex.as_str())
    );
    assert_eq!(
        store.projection().inspector.profile_id.as_deref(),
        Some(codex.as_str())
    );
    assert_eq!(store.active_host_profile_id(), active);
    assert_eq!(store.document_store.host_revision(), revision);
    assert_eq!(store.projection().status_menu.routing_mode_summary, routing);
    assert_eq!(store.projection().header.banner, banner);
    assert!(store.dispatched_receipts().is_empty());

    let before = store.projection().clone();
    assert!(store
        .dispatch_sidebar_action(SidebarAction::BrowseProfile("missing-profile".into()))
        .is_err());
    assert_eq!(store.projection(), &before);

    store
        .dispatch_sidebar_action(SidebarAction::SelectProfile(codex.clone()))
        .unwrap();
    assert_eq!(
        store.active_host_profile_id().as_deref(),
        Some(codex.as_str())
    );
}

#[test]
fn board_layer_preview_browses_assigned_profile_and_clears_empty_editor() {
    let temp = TempDir::new().unwrap();
    let mut store = legacy_store(&temp);
    store.reset_board_layer_plan().unwrap();
    let active = store.active_host_profile_id();
    let revision = store.document_store.host_revision();
    let codex = store.projection().sidebar.board_layers.layers[0]
        .profile_id
        .clone()
        .unwrap();
    store
        .dispatch_sidebar_action(SidebarAction::PreviewBoardLayer(BoardLayer::ALL[0]))
        .unwrap();
    assert_eq!(store.selected_profile_id(), Some(codex.as_str()));
    assert_eq!(store.projection().canvas.board_preview_layer, Some(1));
    assert!(store.projection().canvas.protected_lock.is_some());

    store
        .dispatch_sidebar_action(SidebarAction::PreviewBoardLayer(BoardLayer::ALL[3]))
        .unwrap();
    assert_eq!(
        store.projection().sidebar.selected_display_name.as_deref(),
        None
    );
    assert_eq!(store.projection().canvas.profile_display_name, "");
    assert_eq!(store.active_host_profile_id(), active);

    store
        .dispatch_sidebar_action(SidebarAction::PreviewBoardLayer(BoardLayer::ALL[5]))
        .unwrap();
    assert_eq!(store.selected_profile_id(), None);
    assert_eq!(store.projection().canvas.profile_id, None);
    assert_eq!(store.projection().canvas.selected_layer_id, None);
    assert_eq!(store.projection().canvas.board_preview_layer, Some(6));
    assert!(!store.projection().canvas.can_edit_bindings);
    assert!(!store.projection().canvas.can_add_lighting);
    assert!(!store.projection().canvas.can_export_or_copy_layer);
    assert_eq!(store.active_host_profile_id(), active);
    assert_eq!(store.document_store.host_revision(), revision);
    assert_eq!(store.projection().sidebar.board_layers.observed_layer, None);
    assert!(store.dispatched_receipts().is_empty());
}

#[test]
fn diagnostics_export_reports_failure_when_destination_cannot_be_written() {
    let temp = TempDir::new().unwrap();
    let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();
    let destination = temp.path().join("diagnostic-report.json");
    std::fs::create_dir(&destination).unwrap();
    let previous_status = store.status_message().to_owned();
    let result =
        store.dispatch_device_workflow_action(mi_core::DeviceWorkflowAction::ExportDiagnostics);
    assert!(matches!(result, Err(mi_core::WorkbenchError::Storage(_))));
    assert_eq!(store.status_message(), previous_status);
    assert!(destination.is_dir());

    std::fs::remove_dir(&destination).unwrap();
    store
        .dispatch_device_workflow_action(mi_core::DeviceWorkflowAction::ExportDiagnostics)
        .unwrap();
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&destination).unwrap()).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["connected"], false);
    assert!(store.status_message().contains("已导出诊断报告"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let mode = std::fs::metadata(&destination)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            mode, 0o600,
            "exported diagnostic-report.json must use 0600 permissions"
        );
        let dir_str = temp.path().display().to_string();
        assert!(
            !store.status_message().contains(&dir_str),
            "status_message must redact host directory path: {}",
            store.status_message()
        );

        let victim = temp.path().join("victim.txt");
        std::fs::write(&victim, b"ORIGINAL").unwrap();
        std::fs::remove_file(&destination).unwrap();
        symlink(&victim, &destination).unwrap();
        let sym_err = store
            .dispatch_device_workflow_action(mi_core::DeviceWorkflowAction::ExportDiagnostics)
            .unwrap_err();
        assert!(matches!(sym_err, mi_core::WorkbenchError::Storage(_)));
        assert_eq!(std::fs::read(&victim).unwrap(), b"ORIGINAL");
        assert!(
            !sym_err.to_string().contains(&dir_str),
            "ExportDiagnostics error must redact host paths: {sym_err}"
        );
    }
}

#[test]
fn standard_device_session_connect_forwards_kb_cs_notifications_with_dedup() {
    use mi_proto::{Framer, NativeFramer};
    use mi_transport::{
        HidCandidate, HidLink, HidTransport, LinkBackend, TransportError, TransportHint, UsagePair,
    };
    use std::{
        future::Future,
        pin::Pin,
        sync::{Arc, Mutex},
    };
    use tokio::sync::{broadcast, mpsc};

    struct ChannelHidLink {
        rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<Result<Vec<u8>, TransportError>>>,
        closed_tx: mpsc::UnboundedSender<Result<Vec<u8>, TransportError>>,
    }

    impl HidLink for ChannelHidLink {
        fn backend(&self) -> LinkBackend {
            LinkBackend::MacHidapi267Numbered
        }

        fn write<'life0, 'life1, 'async_trait>(
            &'life0 self,
            _report: &'life1 [u8],
        ) -> Pin<Box<dyn Future<Output = Result<(), TransportError>> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async { Ok(()) })
        }

        fn read<'life0, 'async_trait>(
            &'life0 self,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, TransportError>> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move {
                let mut guard = self.rx.lock().await;
                guard
                    .recv()
                    .await
                    .unwrap_or(Err(TransportError::Disconnected))
            })
        }

        fn close(&self) {
            let _ = self.closed_tx.send(Err(TransportError::Disconnected));
        }
    }

    struct FakeUsbTransport {
        candidate: HidCandidate,
        link: Mutex<Option<Arc<dyn HidLink>>>,
        events: broadcast::Sender<()>,
    }

    impl HidTransport for FakeUsbTransport {
        fn enumerate(&self) -> Result<Vec<HidCandidate>, TransportError> {
            Ok(vec![self.candidate.clone()])
        }

        fn subscribe(&self) -> broadcast::Receiver<()> {
            self.events.subscribe()
        }

        fn open(&self, id: &str) -> Result<Arc<dyn HidLink>, TransportError> {
            assert_eq!(id, self.candidate.path);
            self.link
                .lock()
                .unwrap()
                .take()
                .ok_or_else(|| TransportError::Unavailable("already opened".to_string()))
        }
    }

    let temp = TempDir::new().unwrap();
    let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();
    let (report_tx, report_rx) = mpsc::unbounded_channel();
    let link: Arc<dyn HidLink> = Arc::new(ChannelHidLink {
        rx: tokio::sync::Mutex::new(report_rx),
        closed_tx: report_tx.clone(),
    });
    let (events, _) = broadcast::channel(8);
    let transport = FakeUsbTransport {
        candidate: HidCandidate {
            vid: 0x303A,
            pid: 0x8298,
            usage_page: 0xFF00,
            usage: 1,
            usage_pairs: vec![UsagePair {
                page: 0xFF00,
                usage: 1,
            }],
            transport_hint: TransportHint::Usb,
            product: "Creator Micro 2".to_string(),
            path: "IOService:/Synthetic/CheatSheetSession".to_string(),
            serial_number: None,
            location_id: None,
        },
        link: Mutex::new(Some(link)),
        events,
    };

    assert!(store.connect_with_transport(&transport, true).unwrap());
    let framer = NativeFramer::default();
    let send_wire = |payload: &[u8]| {
        for report in framer.encode(payload).unwrap() {
            report_tx.send(Ok(report)).unwrap();
        }
    };
    let wait_visible = |store: &mut WorkbenchStore, expected: bool| {
        let deadline = Instant::now() + Duration::from_secs(3);
        while matches!(
            store.projection().overlay.content,
            mi_core::ActiveHudContent::InteractiveCheatSheet(_)
        ) != expected
        {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for cheat_sheet_visible={expected}"
            );
            store.poll_at(Instant::now());
            std::thread::sleep(Duration::from_millis(10));
        }
    };

    send_wire(br#"{"method":"kb.cs.show","params":{}}"#);
    wait_visible(&mut store, true);

    send_wire(br#"{"method":"kb.cs.hide","params":{}}"#);
    wait_visible(&mut store, false);

    send_wire(br#"{"method":"kb.cs.toggle","params":{}}"#);
    wait_visible(&mut store, true);

    send_wire(br#"{"method":"kb.cs.toggle","params":{}}"#);
    wait_visible(&mut store, false);
}

#[test]
fn multi_device_usb_selection_persistence_fallback_and_switch_isolation() {
    use mi_proto::{Framer, NativeFramer};
    use mi_transport::{
        HidCandidate, HidLink, HidTransport, LinkBackend, TransportError, TransportHint, UsagePair,
    };
    use serde_json::{json, Value};
    use std::{
        future::Future,
        pin::Pin,
        sync::{Arc, Mutex},
    };
    use tokio::sync::{broadcast, mpsc};

    fn b64_encode(input: &[u8]) -> String {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
        for chunk in input.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
            let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
            let triple = (b0 << 16) | (b1 << 8) | b2;
            out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
            out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
            out.push(if chunk.len() > 1 {
                TABLE[((triple >> 6) & 0x3F) as usize] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                TABLE[(triple & 0x3F) as usize] as char
            } else {
                '='
            });
        }
        out
    }

    struct ResponderLink {
        rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<Result<Vec<u8>, TransportError>>>,
        tx: mpsc::UnboundedSender<Result<Vec<u8>, TransportError>>,
        in_framer: Mutex<NativeFramer>,
        keymap_bytes: Vec<u8>,
        keymap_sha1: String,
    }

    impl HidLink for ResponderLink {
        fn backend(&self) -> LinkBackend {
            LinkBackend::MacHidapi267Numbered
        }

        fn is_exclusive_vendor(&self) -> bool {
            true
        }

        fn write<'life0, 'life1, 'async_trait>(
            &'life0 self,
            report: &'life1 [u8],
        ) -> Pin<Box<dyn Future<Output = Result<(), TransportError>> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            Self: 'async_trait,
        {
            let lines = self
                .in_framer
                .lock()
                .unwrap()
                .feed(report)
                .unwrap_or_default();
            for line in lines {
                let req: Value = serde_json::from_slice(&line).unwrap();
                let id = req["id"].clone();
                let method = req["method"].as_str().unwrap_or("").to_string();
                let reply = match method.as_str() {
                    "device.status" => json!({
                        "id": id,
                        "result": {
                            "version": "0.6.3",
                            "profile_index": 0,
                            "layer_index": 1
                        }
                    }),
                    "fs.list" => json!({
                        "id": id,
                        "result": [{
                            "name": "keymap.json",
                            "size": self.keymap_bytes.len(),
                            "checksum": self.keymap_sha1
                        }]
                    }),
                    "fs.readbin" => {
                        let offset = req["params"]["offset"].as_u64().unwrap_or(0) as usize;
                        let len = req["params"]["len"].as_u64().unwrap_or(3072) as usize;
                        let end = (offset + len).min(self.keymap_bytes.len());
                        json!({
                            "id": id,
                            "result": {
                                "total_size": self.keymap_bytes.len(),
                                "data": b64_encode(&self.keymap_bytes[offset..end])
                            }
                        })
                    }
                    "v.oai.rgbcfg" | "lights.preview" | "v.oai.thstatus" => json!({
                        "id": id,
                        "result": {}
                    }),
                    other => panic!("unexpected RPC method: {other}"),
                };
                for out_report in NativeFramer::default()
                    .encode(reply.to_string().as_bytes())
                    .unwrap()
                {
                    let _ = self.tx.send(Ok(out_report));
                }
            }
            Box::pin(async { Ok(()) })
        }

        fn read<'life0, 'async_trait>(
            &'life0 self,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, TransportError>> + Send + 'async_trait>>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move {
                let mut guard = self.rx.lock().await;
                guard
                    .recv()
                    .await
                    .unwrap_or(Err(TransportError::Disconnected))
            })
        }

        fn close(&self) {
            let _ = self.tx.send(Err(TransportError::Disconnected));
        }
    }

    struct MultiUsbTransport {
        candidates: Mutex<Vec<HidCandidate>>,
        opened_paths: Mutex<Vec<String>>,
        events: broadcast::Sender<()>,
        keymap_bytes: Vec<u8>,
        keymap_sha1: String,
    }

    impl HidTransport for MultiUsbTransport {
        fn enumerate(&self) -> Result<Vec<HidCandidate>, TransportError> {
            Ok(self.candidates.lock().unwrap().clone())
        }

        fn subscribe(&self) -> broadcast::Receiver<()> {
            self.events.subscribe()
        }

        fn open(&self, id: &str) -> Result<Arc<dyn HidLink>, TransportError> {
            self.opened_paths.lock().unwrap().push(id.to_string());
            let (tx, rx) = mpsc::unbounded_channel();
            Ok(Arc::new(ResponderLink {
                rx: tokio::sync::Mutex::new(rx),
                tx,
                in_framer: Mutex::new(NativeFramer::default()),
                keymap_bytes: self.keymap_bytes.clone(),
                keymap_sha1: self.keymap_sha1.clone(),
            }))
        }
    }

    let make_candidate = |path: &str, serial: &str| HidCandidate {
        vid: 0x303A,
        pid: 0x8298,
        usage_page: 0xFF00,
        usage: 1,
        usage_pairs: vec![UsagePair {
            page: 0xFF00,
            usage: 1,
        }],
        transport_hint: TransportHint::Usb,
        product: "Creator Micro 2".to_string(),
        path: path.to_string(),
        serial_number: Some(serial.to_string()),
        location_id: None,
    };

    let beta = make_candidate("IOService:/Synthetic/Beta", "SN-BETA");
    let alpha = make_candidate("IOService:/Synthetic/Alpha", "SN-ALPHA");
    let mut raw_demo = mi_model::demo().unwrap().raw;
    raw_demo["version"] = json!(1);
    raw_demo["activeProfileId"] = json!(0);
    let keymap_bytes = serde_json::to_vec(&raw_demo).unwrap();
    let keymap_sha1 = mi_model::operation_record::file_value(&keymap_bytes)["sha1"]
        .as_str()
        .unwrap()
        .to_string();
    let (events, _) = broadcast::channel(8);
    let transport = MultiUsbTransport {
        candidates: Mutex::new(vec![beta.clone(), alpha.clone()]),
        opened_paths: Mutex::new(Vec::new()),
        events,
        keymap_bytes,
        keymap_sha1,
    };

    let temp = TempDir::new().unwrap();
    let mut cfg = mi_core::WorkbenchStoreConfig::isolated(temp.path());
    cfg.isolated = false;
    let mut store = WorkbenchStore::open(cfg.clone()).unwrap();

    assert!(store.connect_with_transport(&transport, true).unwrap());
    assert_eq!(
        transport.opened_paths.lock().unwrap().as_slice(),
        &["IOService:/Synthetic/Alpha"]
    );
    assert_eq!(
        store.projection().header.preferred_device_serial.as_deref(),
        Some("SN-ALPHA")
    );
    assert!(
        store.projection().header.power_status.is_none(),
        "0.6.3 device must never display 0.7.0 power_status capsule"
    );
    assert_eq!(
        store.projection().status_menu.power_status,
        store.projection().header.power_status
    );
    assert_eq!(store.projection().header.candidate_devices.len(), 2);
    assert_eq!(
        store.projection().header.candidate_devices[0]
            .serial_number
            .as_deref(),
        Some("SN-ALPHA")
    );
    assert!(store.projection().header.candidate_devices[0].is_connected);
    assert!(store.projection().header.candidate_devices[0].is_preferred);
    assert!(!store.projection().header.candidate_devices[1].is_connected);
    assert_eq!(
        store.projection().status_menu.candidate_devices,
        store.projection().header.candidate_devices
    );

    assert!(store
        .select_candidate_device_with_transport(&transport, "SN-ALPHA")
        .unwrap());
    assert_eq!(transport.opened_paths.lock().unwrap().len(), 1);

    assert!(store
        .select_candidate_device_with_transport(&transport, "MISSING-SERIAL")
        .is_err());
    assert!(store.device_session.is_some());
    assert_eq!(transport.opened_paths.lock().unwrap().len(), 1);

    assert!(store
        .select_candidate_device_with_transport(&transport, "SN-BETA")
        .unwrap());
    assert_eq!(
        transport.opened_paths.lock().unwrap().as_slice(),
        &["IOService:/Synthetic/Alpha", "IOService:/Synthetic/Beta"]
    );
    assert_eq!(
        store.projection().header.preferred_device_serial.as_deref(),
        Some("SN-BETA")
    );
    assert_eq!(
        store
            .projection()
            .status_menu
            .preferred_device_serial
            .as_deref(),
        Some("SN-BETA")
    );
    assert!(store.projection().header.candidate_devices[1].is_connected);
    assert!(store.projection().header.candidate_devices[1].is_preferred);
    assert!(!store.projection().header.candidate_devices[0].is_connected);

    store.system_will_sleep();
    assert!(store
        .select_candidate_device_with_transport(&transport, "SN-ALPHA")
        .is_err());
    store.system_did_wake();
    drop(store);

    transport.opened_paths.lock().unwrap().clear();
    let mut reopened = WorkbenchStore::open(cfg.clone()).unwrap();
    assert_eq!(
        reopened
            .projection()
            .header
            .preferred_device_serial
            .as_deref(),
        Some("SN-BETA")
    );
    assert!(reopened.connect_with_transport(&transport, true).unwrap());
    assert_eq!(
        transport.opened_paths.lock().unwrap().as_slice(),
        &["IOService:/Synthetic/Beta"]
    );
    drop(reopened);

    *transport.candidates.lock().unwrap() = vec![alpha];
    transport.opened_paths.lock().unwrap().clear();
    let mut fallback = WorkbenchStore::open(cfg).unwrap();
    assert!(fallback.connect_with_transport(&transport, true).unwrap());
    assert_eq!(
        transport.opened_paths.lock().unwrap().as_slice(),
        &["IOService:/Synthetic/Alpha"]
    );
    assert_eq!(
        fallback
            .projection()
            .header
            .preferred_device_serial
            .as_deref(),
        Some("SN-ALPHA")
    );
}

#[test]
fn open_migrates_on_disk_six_layer_runtime_in_memory_and_persisted() {
    use mi_model::host_router::ResolvedHostAction;

    let temp = TempDir::new().unwrap();
    let initial_store = WorkbenchStore::open_isolated(temp.path()).unwrap();
    let doc = initial_store.document_store.draft.document.clone();
    let ag_pid = initial_store
        .find_canonical_profile_id(CanonicalAgent::Antigravity)
        .unwrap();
    let dsh_pid = initial_store
        .find_canonical_profile_id(CanonicalAgent::DeepseekHarness)
        .unwrap();
    let dsh_lid = doc.editable.graph.profiles[&dsh_pid].layer_order[0].clone();
    let mut v090_draft = initial_store.document_store.draft.clone();
    v090_draft
        .assign(
            &serde_json::json!(dsh_pid),
            &serde_json::json!(dsh_lid),
            15,
            mi_model::draft::Action {
                id: "agent-model-picker-open".into(),
                name: "弹出模型选择页".into(),
                category: "Agent".into(),
                detail: "打开当前目标 Agent 的模型选择弹层并进入旋钮选模型与推理力度会话".into(),
                payload: serde_json::json!({
                    "type": "agent_command",
                    "adapter": "host",
                    "command": "model-picker-open"
                }),
            },
        )
        .unwrap();
    std::fs::write(
        temp.path().join("workbench.json"),
        serde_json::to_vec(&v090_draft.document).unwrap(),
    )
    .unwrap();
    drop(initial_store);

    let mk_cmd = |adapter: &str, command: &str| {
        serde_json::to_value(ResolvedHostAction::AgentCommand {
            adapter: adapter.into(),
            command: command.into(),
        })
        .unwrap()
    };
    let desired_json =
        serde_json::to_value(mi_model::board_layers::BoardLayerPlan::default()).unwrap();
    let v080_and_v090_json = serde_json::json!({
        "version": 1,
        "installation": {
            "status": "Applied",
            "plan": {
                "desired": desired_json,
                "bottom_row_layout": "split",
                "hosts": [
                    {
                        "profile_id": ag_pid,
                        "adapter": "antigravity",
                        "actions": [
                            null, null, null, null, null, null, null, null, null, null,
                            null, null, null,
                            mk_cmd("antigravity", "model-picker-prev"),
                            mk_cmd("antigravity", "model-picker-next"),
                            mk_cmd("antigravity", "model-picker-confirm"),
                            null, null, null, null
                        ],
                        "encoder_hold_action": mk_cmd("antigravity", "model-picker-open")
                    },
                    {
                        "profile_id": dsh_pid,
                        "adapter": "dsh-desktop",
                        "actions": [
                            null, null, null, null, null, null, null, null, null, null,
                            null, null, null,
                            mk_cmd("dsh-desktop", "model-picker-prev"),
                            mk_cmd("dsh-desktop", "model-picker-next"),
                            mk_cmd("dsh-desktop", "model-picker-open"),
                            null, null, null, null
                        ],
                        "encoder_hold_action": mk_cmd("dsh-desktop", "search")
                    },
                    {
                        "profile_id": "custom-profile",
                        "adapter": "custom-adapter",
                        "actions": [
                            null, null, null, null, null, null, null, null, null, null,
                            null, null, null, null, null,
                            mk_cmd("custom-adapter", "custom-command"),
                            null, null, null, null
                        ],
                        "encoder_hold_action": mk_cmd("custom-adapter", "custom-hold")
                    }
                ]
            },
            "target": {
                "vendor_id": 0x303A,
                "product_id": 0x8298,
                "location_id": 0,
                "usb_parent": 0,
                "vendor_path": ""
            },
            "profile_index": 0,
            "backup": temp.path().join("backup.json"),
            "config": {
                "path": temp.path().join("karabiner.json"),
                "original": [],
                "installed": [],
                "profile_name": "Default profile",
                "installed_device": {}
            }
        }
    });

    let runtime_path = temp.path().join("six-layer-runtime.json");
    std::fs::write(
        &runtime_path,
        serde_json::to_vec_pretty(&v080_and_v090_json).unwrap(),
    )
    .unwrap();

    let mut cfg = mi_core::WorkbenchStoreConfig::isolated(temp.path());
    cfg.isolated = false;
    cfg.auto_connect_usb = false;
    let mut store = WorkbenchStore::open(cfg).unwrap();
    assert!(store.has_all_desktop_agent_presets());
    let rev_before = store.document_store.draft.revision;
    store.prepare_desktop_profiles().unwrap();
    assert_eq!(store.document_store.draft.revision, rev_before);

    let inst = store.six_layer_state().installation.as_ref().unwrap();
    let migrated_ag = inst
        .plan
        .hosts
        .iter()
        .find(|h| h.profile_id == ag_pid)
        .unwrap();
    assert_eq!(
        migrated_ag.actions[15],
        Some(ResolvedHostAction::AgentCommand {
            adapter: "antigravity".into(),
            command: "search".into(),
        })
    );
    assert_eq!(
        migrated_ag.encoder_hold_action,
        Some(ResolvedHostAction::AgentCommand {
            adapter: "antigravity".into(),
            command: "search".into(),
        })
    );

    let migrated_dsh = inst
        .plan
        .hosts
        .iter()
        .find(|h| h.profile_id == dsh_pid)
        .unwrap();
    assert_eq!(
        migrated_dsh.actions[15],
        Some(ResolvedHostAction::AgentCommand {
            adapter: "dsh-desktop".into(),
            command: "search".into(),
        })
    );
    assert_eq!(
        migrated_dsh.encoder_hold_action,
        Some(ResolvedHostAction::AgentCommand {
            adapter: "dsh-desktop".into(),
            command: "search".into(),
        })
    );

    let preserved_custom = inst
        .plan
        .hosts
        .iter()
        .find(|h| h.profile_id == "custom-profile")
        .unwrap();
    assert_eq!(
        preserved_custom.actions[15],
        Some(ResolvedHostAction::AgentCommand {
            adapter: "custom-adapter".into(),
            command: "custom-command".into(),
        })
    );
    assert_eq!(
        preserved_custom.encoder_hold_action,
        Some(ResolvedHostAction::AgentCommand {
            adapter: "custom-adapter".into(),
            command: "custom-hold".into(),
        })
    );

    let on_disk = mi_core::six_layer_runtime::load(&runtime_path).unwrap();
    let disk_inst = on_disk.installation.as_ref().unwrap();
    let disk_ag = disk_inst
        .plan
        .hosts
        .iter()
        .find(|h| h.profile_id == ag_pid)
        .unwrap();
    assert_eq!(
        disk_ag.actions[15],
        Some(ResolvedHostAction::AgentCommand {
            adapter: "antigravity".into(),
            command: "search".into(),
        })
    );
    assert_eq!(
        disk_ag.encoder_hold_action,
        Some(ResolvedHostAction::AgentCommand {
            adapter: "antigravity".into(),
            command: "search".into(),
        })
    );
    let disk_dsh = disk_inst
        .plan
        .hosts
        .iter()
        .find(|h| h.profile_id == dsh_pid)
        .unwrap();
    assert_eq!(
        disk_dsh.actions[15],
        Some(ResolvedHostAction::AgentCommand {
            adapter: "dsh-desktop".into(),
            command: "search".into(),
        })
    );
}

#[test]
fn command_execution_enabled_defaults_fail_closed_across_store_and_inspector() {
    let temp = TempDir::new().unwrap();
    let store = WorkbenchStore::open_isolated(temp.path()).unwrap();
    match &store.projection().inspector.body {
        mi_core::InspectorBodyProjection::Editable(edit) => {
            assert!(
                !edit.command_execution_enabled,
                "command_execution_enabled must default to false"
            );
        }
        other => panic!("expected Editable inspector, got {other:?}"),
    }
    assert!(
        !mi_core::EditableSlotInspectorProjection::default().command_execution_enabled,
        "EditableSlotInspectorProjection::default() must fail closed"
    );
}

#[test]
fn set_command_execution_enabled_persists_across_return_to_host_and_reopen() {
    let temp = TempDir::new().unwrap();
    let seed = WorkbenchStore::open_isolated(temp.path()).unwrap();
    std::fs::write(
        temp.path().join("workbench.json"),
        serde_json::to_vec(&seed.document_store.draft.document).unwrap(),
    )
    .unwrap();
    drop(seed);

    let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();
    assert!(!store.document_store.command_execution_enabled());

    store
        .dispatch_inspector_action(mi_core::InspectorAction::SetCommandExecutionEnabled(true))
        .expect("enable command execution");
    assert!(store.document_store.command_execution_enabled());

    store
        .apply_backup_command(BackupCommand::ReturnToHostDocument)
        .expect("return to host document");
    assert!(
        store.document_store.command_execution_enabled(),
        "command_execution_enabled must survive ReturnToHostDocument"
    );
    match &store.projection().inspector.body {
        mi_core::InspectorBodyProjection::Editable(edit) => {
            assert!(edit.command_execution_enabled);
        }
        other => panic!("expected Editable inspector, got {other:?}"),
    }

    drop(store);
    let reopened = WorkbenchStore::open_isolated(temp.path()).unwrap();
    assert!(
        reopened.document_store.command_execution_enabled(),
        "command_execution_enabled must survive store reopen"
    );
    match &reopened.projection().inspector.body {
        mi_core::InspectorBodyProjection::Editable(edit) => {
            assert!(edit.command_execution_enabled);
        }
        other => panic!("expected Editable inspector, got {other:?}"),
    }
}

#[test]
fn coding_agent_top_six_defaults_migrate_cockpit_slots_and_preserve_explicit_session_keys() {
    use mi_model::draft::Action;
    use mi_model::host_router::{resolve_host_action, ResolvedHostAction};
    use serde_json::json;

    let temp = TempDir::new().unwrap();
    let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();
    let find_pid = |s: &WorkbenchStore, name: &str| {
        s.document_store
            .draft
            .document
            .editable
            .graph
            .profiles
            .iter()
            .find(|(_, p)| p.name == name)
            .unwrap()
            .0
            .clone()
    };

    let ag_pid = find_pid(&store, "Antigravity");
    let ag_lid =
        store.document_store.draft.document.editable.graph.profiles[&ag_pid].layer_order[0].clone();
    store.select_and_activate_profile(&ag_pid).unwrap();
    {
        let doc = &store.document_store.draft.document;
        for slot in 0..6u32 {
            assert_eq!(
                resolve_host_action(doc, Some(&ag_pid), &format!("AG{slot:02}"), true),
                Some(ResolvedHostAction::AgentCommand {
                    adapter: "four-app".into(),
                    command: format!("slot-open:{slot}"),
                })
            );
        }
        for (binding, cmd) in [
            ("AG15", "scroll-up"),
            ("AG16", "open-side-panel"),
            ("AG17", "scroll-down"),
            ("AG18", "close-side-panel"),
        ] {
            assert_eq!(
                resolve_host_action(doc, Some(&ag_pid), binding, true),
                Some(ResolvedHostAction::AgentCommand {
                    adapter: "antigravity".into(),
                    command: cmd.into(),
                })
            );
        }
        let ag_cs = doc.editable.graph.layers[&ag_lid]
            .control_settings
            .as_ref()
            .unwrap();
        assert_eq!(ag_cs.joystick_mode, "logical_sectors");
        assert_eq!(ag_cs.active_sectors, 4);
    }
    for (slot, cell) in store.projection().canvas.keycap_cells[..6]
        .iter()
        .enumerate()
    {
        assert_eq!(cell.slot_role, "TaskKey");
        assert_eq!(cell.is_session_slot, Some(slot));
    }

    let codex_pid = find_pid(&store, mi_model::preset::LOCAL_CODEX_PROFILE_NAME);
    let codex_lid = store.document_store.draft.document.editable.graph.profiles[&codex_pid]
        .layer_order[0]
        .clone();
    store.select_and_activate_profile(&codex_pid).unwrap();
    {
        let doc = &store.document_store.draft.document;
        for slot in 0..6u32 {
            assert_eq!(
                resolve_host_action(doc, Some(&codex_pid), &format!("AG{slot:02}"), true),
                Some(ResolvedHostAction::AgentCommand {
                    adapter: "four-app".into(),
                    command: format!("slot-open:{slot}"),
                })
            );
        }
        let codex_cs = doc.editable.graph.layers[&codex_lid]
            .control_settings
            .as_ref()
            .unwrap();
        assert_eq!(codex_cs.joystick_mode, "mouse");
        assert_eq!(codex_cs.active_sectors, 0);
    }
    for (slot, cell) in store.projection().canvas.keycap_cells[..6]
        .iter()
        .enumerate()
    {
        assert_eq!(cell.slot_role, "TaskKey");
        assert_eq!(cell.is_session_slot, Some(slot));
    }

    for (name, expected_adapter) in [("DeepSeek Coder (DSH)", "dsh-desktop")] {
        let pid = find_pid(&store, name);
        store.select_and_activate_profile(&pid).unwrap();
        let doc = &store.document_store.draft.document;
        assert_eq!(
            resolve_host_action(doc, Some(&pid), "AG00", true),
            Some(ResolvedHostAction::AgentCommand {
                adapter: expected_adapter.into(),
                command: "subagent".into(),
            })
        );
        assert_eq!(
            resolve_host_action(doc, Some(&pid), "AG01", true),
            Some(ResolvedHostAction::AgentCommand {
                adapter: "host".into(),
                command: "mouse-click:left".into(),
            })
        );
        for slot in 2..6u32 {
            assert_eq!(
                resolve_host_action(doc, Some(&pid), &format!("AG{slot:02}"), true),
                None,
                "{name} second-row slot {slot} must be unbound"
            );
        }
        let cells = &store.projection().canvas.keycap_cells;
        assert_eq!(cells[0].keycap_id, "Branch");
        assert_eq!(cells[0].slot_role, "ActionKey");
        assert_eq!(cells[1].keycap_id, "Nav");
        assert_eq!(cells[1].slot_role, "ActionKey");
        for cell in &cells[2..6] {
            assert_eq!(cell.keycap_id, "Blank");
            assert_eq!(cell.slot_role, "ActionKey");
            assert!(!cell.is_available);
        }
    }

    let dsh_pid = find_pid(&store, "DeepSeek Coder (DSH)");
    let dsh_lid = store.document_store.draft.document.editable.graph.profiles[&dsh_pid].layer_order
        [0]
    .clone();
    store
        .document_store
        .update(|draft| {
            draft.assign(
                &json!(ag_pid),
                &json!(ag_lid),
                0,
                Action {
                    id: "agent-subagent".into(),
                    name: "选择 Subagent".into(),
                    category: "Agent".into(),
                    detail: "legacy #160".into(),
                    payload: json!({"type":"agent_command","adapter":"antigravity","command":"subagent"}),
                },
            )?;
            draft.assign(
                &json!(ag_pid),
                &json!(ag_lid),
                1,
                Action {
                    id: "host-mouse-click-left".into(),
                    name: "鼠标左键点击".into(),
                    category: "Agent".into(),
                    detail: "legacy #160".into(),
                    payload: json!({"type":"agent_command","adapter":"host","command":"mouse-click:left"}),
                },
            )?;
            for slot in 2..6u32 {
                draft.unassign(&json!(ag_pid), &json!(ag_lid), slot)?;
            }
            for slot in 16..20u32 {
                draft.unassign(&json!(ag_pid), &json!(ag_lid), slot)?;
            }
            draft.set_control_settings(
                &ag_pid,
                &ag_lid,
                mi_model::control_settings::ControlSettings {
                    joystick_mode: "mouse".into(),
                    active_sectors: 0,
                    encoder_hold_action_id: None,
                },
            )?;
            draft.assign_encoder_hold(
                &json!(ag_pid),
                &json!(ag_lid),
                mi_model::preset::canonical_encoder_hold_action(),
            )?;

            for slot in 0..6u32 {
                let id = if slot == 0 {
                    "slot-open-0".to_string()
                } else {
                    format!("cockpit-slot-{slot}")
                };
                draft.assign(
                    &json!(dsh_pid),
                    &json!(dsh_lid),
                    slot,
                    Action {
                        id,
                        name: format!("会话 {}", slot + 1),
                        category: "四应用总控".into(),
                        detail: "legacy cockpit".into(),
                        payload: json!({
                            "type": "agent_command",
                            "adapter": "four-app",
                            "command": format!("slot-open:{slot}")
                        }),
                    },
                )?;
            }
            draft.set_control_settings(
                &dsh_pid,
                &dsh_lid,
                mi_model::control_settings::ControlSettings {
                    joystick_mode: "logical_sectors".into(),
                    active_sectors: 4,
                    encoder_hold_action_id: None,
                },
            )?;
            draft.assign_encoder_hold(
                &json!(dsh_pid),
                &json!(dsh_lid),
                mi_model::preset::canonical_encoder_hold_action(),
            )?;
            Ok(())
        })
        .unwrap();
    std::fs::write(
        temp.path().join("workbench.json"),
        serde_json::to_vec(&store.document_store.draft.document).unwrap(),
    )
    .unwrap();
    drop(store);

    let mut migrated = WorkbenchStore::open_isolated(temp.path()).unwrap();
    assert!(migrated.has_all_desktop_agent_presets());
    {
        let doc = &migrated.document_store.draft.document;
        for slot in 0..6u32 {
            assert_eq!(
                resolve_host_action(doc, Some(&ag_pid), &format!("AG{slot:02}"), true),
                Some(ResolvedHostAction::AgentCommand {
                    adapter: "four-app".into(),
                    command: format!("slot-open:{slot}"),
                })
            );
        }
        assert_eq!(
            resolve_host_action(doc, Some(&ag_pid), "AG16", true),
            Some(ResolvedHostAction::AgentCommand {
                adapter: "antigravity".into(),
                command: "open-side-panel".into(),
            })
        );
        assert_eq!(
            resolve_host_action(doc, Some(&ag_pid), "AG18", true),
            Some(ResolvedHostAction::AgentCommand {
                adapter: "antigravity".into(),
                command: "close-side-panel".into(),
            })
        );
        assert_eq!(
            doc.editable.graph.layers[&ag_lid]
                .control_settings
                .as_ref()
                .unwrap()
                .joystick_mode,
            "logical_sectors"
        );

        assert_eq!(
            resolve_host_action(doc, Some(&dsh_pid), "AG00", true),
            Some(ResolvedHostAction::AgentCommand {
                adapter: "dsh-desktop".into(),
                command: "subagent".into(),
            })
        );
        assert_eq!(
            resolve_host_action(doc, Some(&dsh_pid), "AG01", true),
            Some(ResolvedHostAction::AgentCommand {
                adapter: "host".into(),
                command: "mouse-click:left".into(),
            })
        );
        for slot in 2..6u32 {
            assert_eq!(
                resolve_host_action(doc, Some(&dsh_pid), &format!("AG{slot:02}"), true),
                None
            );
        }
        assert_eq!(
            doc.editable.graph.layers[&dsh_lid]
                .control_settings
                .as_ref()
                .unwrap()
                .joystick_mode,
            "mouse"
        );
    }

    migrated.select_and_activate_profile(&dsh_pid).unwrap();
    migrated
        .dispatch_session_strip_action(mi_core::SessionStripAction::BindTopSixAsSessionKeys)
        .unwrap();
    drop(migrated);

    let mut reopened = WorkbenchStore::open_isolated(temp.path()).unwrap();
    let doc_after = &reopened.document_store.draft.document;
    for slot in 0..6u32 {
        assert_eq!(
            resolve_host_action(doc_after, Some(&dsh_pid), &format!("AG{slot:02}"), true),
            Some(ResolvedHostAction::AgentCommand {
                adapter: "four-app".into(),
                command: format!("slot-open:{slot}"),
            })
        );
    }

    reopened.select_and_activate_profile(&dsh_pid).unwrap();
    reopened
        .dispatch_inspector_action(mi_core::InspectorAction::SelectSlot(0))
        .unwrap();
    reopened
        .dispatch_inspector_action(mi_core::InspectorAction::SetSlotRole(
            mi_core::KeycapRole::TaskKey,
        ))
        .unwrap();
    assert_eq!(
        reopened.projection().canvas.keycap_cells[0].slot_role,
        "TaskKey"
    );
    reopened
        .dispatch_session_strip_action(mi_core::SessionStripAction::RestoreDefaultLayout)
        .unwrap();
    let restored_cells = &reopened.projection().canvas.keycap_cells;
    assert_eq!(restored_cells[0].slot_role, "ActionKey");
    assert_eq!(restored_cells[0].full_action_title, "选择 Subagent");
    assert_eq!(restored_cells[1].slot_role, "ActionKey");
    assert_eq!(restored_cells[1].full_action_title, "鼠标左键点击");
    for cell in &restored_cells[2..6] {
        assert_eq!(cell.slot_role, "ActionKey");
        assert_eq!(cell.keycap_id, "Blank");
    }

    reopened.select_and_activate_profile(&ag_pid).unwrap();
    reopened
        .dispatch_inspector_action(mi_core::InspectorAction::SelectSlot(0))
        .unwrap();
    reopened
        .dispatch_inspector_action(mi_core::InspectorAction::SetSlotRole(
            mi_core::KeycapRole::ActionKey,
        ))
        .unwrap();
    reopened
        .dispatch_session_strip_action(mi_core::SessionStripAction::RestoreDefaultLayout)
        .unwrap();
    let ag_restored = reopened.projection();
    assert_eq!(ag_restored.canvas.joystick.mode, "logical_sectors");
    for (slot, cell) in ag_restored.canvas.keycap_cells[..6].iter().enumerate() {
        assert_eq!(cell.slot_role, "TaskKey");
        assert_eq!(cell.is_session_slot, Some(slot));
    }
}

#[test]
fn non_isolated_store_refuses_synthetic_device_snapshot_when_baseline_is_absent() {
    let temp = TempDir::new().unwrap();
    let mut cfg = mi_core::WorkbenchStoreConfig::isolated(temp.path());
    cfg.isolated = false;
    cfg.auto_connect_usb = false;
    let mut store = WorkbenchStore::open(cfg).unwrap();
    assert!(store.document_store.draft.document.baseline.is_none());
    let before_rev = store.document_store.draft.revision;

    assert!(store.current_device_snapshot().is_err());
    for action in [
        mi_core::DeviceWorkflowAction::ReadSnapshot,
        mi_core::DeviceWorkflowAction::OpenReconnectComparison,
        mi_core::DeviceWorkflowAction::ConfirmComparisonIdentity,
        mi_core::DeviceWorkflowAction::OpenApplyPreparation,
        mi_core::DeviceWorkflowAction::PrepareApplyBackup,
        mi_core::DeviceWorkflowAction::TakePreparedCandidate,
        mi_core::DeviceWorkflowAction::SimulateKeyPress {
            binding_id: "AG00".to_string(),
        },
    ] {
        assert!(
            store
                .dispatch_device_workflow_action(action.clone())
                .is_err(),
            "production config must reject synthetic fallback for {action:?}"
        );
    }

    assert!(store.document_store.draft.document.baseline.is_none());
    assert_eq!(store.document_store.draft.revision, before_rev);
    assert!(store.projection().active_modal.is_none());
}

#[test]
fn open_isolated_migrates_persisted_v2_local_codex_and_lights_six_layer_session_slots() {
    use mi_core::{FourAppLiveCandidate, FourAppSessionKind};
    use mi_model::draft::Action;
    use mi_model::host_router::{resolve_host_action, BottomRowLayout, ResolvedHostAction};
    use mi_model::navigation_history::NavigationPlace;
    use serde_json::json;

    let temp = TempDir::new().unwrap();
    let mut initial = WorkbenchStore::open_isolated(temp.path()).unwrap();
    let find_pid = |s: &WorkbenchStore, name: &str| {
        s.document_store
            .draft
            .document
            .editable
            .graph
            .profiles
            .iter()
            .find(|(_, p)| p.name == name)
            .unwrap()
            .0
            .clone()
    };
    let native_codex_pid = find_pid(&initial, "Codex");
    let local_codex_pid = find_pid(&initial, mi_model::preset::LOCAL_CODEX_PROFILE_NAME);
    let local_codex_lid = initial
        .document_store
        .draft
        .document
        .editable
        .graph
        .profiles[&local_codex_pid]
        .layer_order[0]
        .clone();
    let ag_pid = find_pid(&initial, "Antigravity");
    let dsh_pid = find_pid(&initial, "DeepSeek Coder (DSH)");

    initial
        .document_store
        .update(|draft| {
            for slot in 0..6u32 {
                draft.unassign(&json!(local_codex_pid), &json!(local_codex_lid), slot)?;
            }
            draft.assign(
                &json!(local_codex_pid),
                &json!(local_codex_lid),
                0,
                Action {
                    id: "agent-subagent".into(),
                    name: "选择 Subagent".into(),
                    category: "Agent".into(),
                    detail: "打开当前目标 Agent 的子智能体（Subagent）选择弹层，支持旋钮上下选择并短按打开".into(),
                    payload: json!({"type": "agent_command", "adapter": "host", "command": "subagent"}),
                },
            )?;
            draft.assign(
                &json!(local_codex_pid),
                &json!(local_codex_lid),
                1,
                Action {
                    id: "host-mouse-click-left".into(),
                    name: "鼠标左键点击".into(),
                    category: "鼠标控制".into(),
                    detail: "在当前光标位置发送左键按下与抬起（CGEvent）".into(),
                    payload: json!({"type": "agent_command", "adapter": "host", "command": "mouse-click:left"}),
                },
            )?;
            Ok(())
        })
        .unwrap();
    assert!(
        mi_model::preset::local_codex_profile_needs_starter_slot_migration(
            &initial.document_store.draft.document,
            &local_codex_pid
        )
    );
    assert!(!initial.has_all_desktop_agent_presets());

    let desired =
        mi_model::board_layers::BoardLayerPlan::recommended(native_codex_pid, ag_pid, dsh_pid);
    let mut applied_plan = mi_model::apply::AppliedSixLayerPlan::compile(
        &desired,
        &initial.document_store.draft.document,
        BottomRowLayout::Split,
        true,
    )
    .unwrap();
    let mut baked_codex_actions: [Option<ResolvedHostAction>; 20] = std::array::from_fn(|_| None);
    baked_codex_actions[0] = Some(ResolvedHostAction::AgentCommand {
        adapter: "codex".into(),
        command: "subagent".into(),
    });
    baked_codex_actions[1] = Some(ResolvedHostAction::AgentCommand {
        adapter: "host".into(),
        command: "mouse-click:left".into(),
    });
    applied_plan.hosts.push(mi_model::apply::AppliedHostLayer {
        profile_id: local_codex_pid.clone(),
        adapter: "codex".into(),
        actions: baked_codex_actions,
        encoder_hold_action: Some(ResolvedHostAction::AgentCommand {
            adapter: "codex".into(),
            command: "search".into(),
        }),
        joystick_mode: Some("MOUSE".into()),
    });
    let saved_runtime = json!({
        "version": 1,
        "installation": {
            "status": "Applied",
            "plan": applied_plan,
            "target": {
                "vendor_id": 0x303A,
                "product_id": 0x8298,
                "location_id": 0,
                "usb_parent": 0,
                "vendor_path": ""
            },
            "profile_index": 0,
            "backup": temp.path().join("backup"),
            "config": {
                "path": temp.path().join("karabiner.json"),
                "original": [],
                "installed": [],
                "profile_name": "Default profile",
                "installed_device": {}
            }
        }
    });
    std::fs::write(
        temp.path().join("workbench.json"),
        serde_json::to_vec(&initial.document_store.draft.document).unwrap(),
    )
    .unwrap();
    std::fs::write(
        temp.path().join("six-layer-runtime.json"),
        serde_json::to_vec_pretty(&saved_runtime).unwrap(),
    )
    .unwrap();
    drop(initial);

    let mut reopened = WorkbenchStore::open_isolated(temp.path()).unwrap();
    assert!(reopened.has_all_desktop_agent_presets());
    for slot in 0..6u32 {
        assert_eq!(
            resolve_host_action(
                &reopened.document_store.draft.document,
                Some(&local_codex_pid),
                &format!("AG{slot:02}"),
                true
            ),
            Some(ResolvedHostAction::AgentCommand {
                adapter: "four-app".into(),
                command: format!("slot-open:{slot}"),
            })
        );
    }
    let migrated_codex_host = reopened
        .six_layer_state()
        .installation
        .as_ref()
        .unwrap()
        .plan
        .hosts
        .iter()
        .find(|h| h.profile_id == local_codex_pid)
        .unwrap();
    for (slot, action) in migrated_codex_host.actions.iter().enumerate().take(6) {
        assert_eq!(
            *action,
            Some(ResolvedHostAction::AgentCommand {
                adapter: "four-app".into(),
                command: format!("slot-open:{slot}"),
            })
        );
    }

    let codex_candidates: Vec<FourAppLiveCandidate> = (0..6usize)
        .map(|slot| FourAppLiveCandidate {
            place: NavigationPlace::codex(
                format!("thread-{slot}"),
                "local",
                format!("/ws/codex-{slot}"),
            ),
            title: format!("Codex Session {}", slot + 1),
            phase: if slot == 0 {
                FourAppSessionKind::NeedsAttention
            } else {
                FourAppSessionKind::BlueDotCompleted
            },
            watermark: format!("wm-{slot}"),
            last_modified_ms: (10_000 - slot * 100) as u64,
            last_user_view_ms: 1_000,
            updated_at_ms: (10_000 - slot * 100) as u64,
            status_detail: None,
        })
        .collect();
    reopened.ingest_four_app_candidates(codex_candidates);

    let entries = reopened
        .lighting_arbiter
        .last_six_layer_entries()
        .expect("six-layer entries computed in BoardLayers mode");
    assert_eq!(entries[0].k, "AG00");
    assert_eq!(entries[0].c, 0xFFAA00);
    assert!(entries[0].b > 0.0);
    assert!(entries[0].sa);
    for (slot, entry) in entries.iter().enumerate().take(6).skip(1) {
        assert_eq!(entry.k, format!("AG{slot:02}"));
        assert_eq!(entry.c, 0x0088FF);
        assert!(entry.b > 0.0);
    }
}

#[test]
fn grokbot_replaces_unmodified_herdr_and_projects_three_key_glyphs() {
    let temp = TempDir::new().unwrap();
    let doc_path = temp.path().join("workbench.json");
    let mut doc_store = mi_model::storage::DocumentStore::default();
    doc_store.open(doc_path.clone()).unwrap();
    doc_store
        .update(|draft| {
            mi_model::preset::install_agent_starter_profiles(draft)?;
            let grok_id = draft
                .document
                .editable
                .graph
                .profiles
                .iter()
                .find(|(_, p)| p.name == "Grok Bot")
                .map(|(id, _)| id.clone())
                .unwrap();
            draft.delete_profile(&grok_id)?;
            let herdr_id = draft.create_profile("Herdr")?;
            let herdr_lid =
                draft.document.editable.graph.profiles[&herdr_id].layer_order[0].clone();
            draft.set_control_settings(
                &herdr_id,
                &herdr_lid,
                mi_model::control_settings::ControlSettings {
                    joystick_mode: "logical_sectors".into(),
                    active_sectors: 4,
                    encoder_hold_action_id: None,
                },
            )?;
            draft.assign(
                &serde_json::json!(herdr_id),
                &serde_json::json!(herdr_lid),
                0,
                mi_model::draft::Action {
                    id: "desktop-open".into(),
                    name: "打开 Herdr".into(),
                    category: "应用".into(),
                    detail: "dev.herdr.app".into(),
                    payload: serde_json::json!({"type":"open_app","bundle_id":"dev.herdr.app"}),
                },
            )?;
            for (slot, label, key, modifiers) in [
                (1u32, "选择 Space / Tab", "p", vec!["command"]),
                (2, "搜索命令", "p", vec!["command", "shift"]),
                (3, "新建 Space", "n", vec!["command"]),
                (4, "新建 Tab / 终端", "t", vec!["command"]),
                (5, "向右分屏", "d", vec!["command"]),
                (6, "向下分屏", "d", vec!["command", "shift"]),
                (7, "上一个 Space", "[", vec!["command", "option"]),
                (8, "下一个 Space", "]", vec!["command", "option"]),
                (9, "上一个 Tab", "Tab", vec!["control", "shift"]),
                (10, "下一个 Tab", "Tab", vec!["control"]),
                (12, "放大 / 还原面板", "Return", vec!["command", "shift"]),
                (15, "显示 / 隐藏侧栏", "b", vec!["command"]),
                (16, "聚焦上方面板", "Up", vec!["command", "option"]),
                (17, "聚焦左方面板", "Left", vec!["command", "option"]),
                (18, "聚焦下方面板", "Down", vec!["command", "option"]),
                (19, "聚焦右方面板", "Right", vec!["command", "option"]),
            ] {
                draft.assign(
                    &serde_json::json!(herdr_id),
                    &serde_json::json!(herdr_lid),
                    slot,
                    mi_model::draft::Action {
                        id: "herdr-desktop".into(),
                        name: label.into(),
                        category: "快捷键".into(),
                        detail: "dev.herdr.app".into(),
                        payload: serde_json::json!({
                            "type": "shortcut",
                            "key": key,
                            "modifiers": modifiers,
                            "target": "dev.herdr.app"
                        }),
                    },
                )?;
            }
            Ok(())
        })
        .unwrap();

    let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();
    assert!(store.has_all_desktop_agent_presets());
    assert!(
        store
            .sidebar_rows()
            .iter()
            .all(|row| row.raw_name != "Herdr"),
        "unmodified Herdr profile must be retired on open"
    );
    let grokbot_id = {
        let grokbot_row = store
            .sidebar_rows()
            .iter()
            .find(|row| row.canonical_agent == Some(CanonicalAgent::GrokBot))
            .expect("Grok Bot canonical sidebar row");
        assert_eq!(grokbot_row.display_name, "Grok Bot");
        assert_eq!(grokbot_row.badge_monogram, "GB");
        grokbot_row.profile_id.clone()
    };

    store
        .dispatch_sidebar_action(SidebarAction::SelectProfile(grokbot_id))
        .unwrap();
    let canvas = &store.projection().canvas;
    let cell = |binding: &str| {
        canvas
            .keycap_cells
            .iter()
            .find(|c| c.binding_id == binding)
            .unwrap_or_else(|| panic!("missing {binding}"))
    };
    assert_eq!(cell("AG06").keycap_id, "Party");
    assert_eq!(cell("AG11").keycap_id, "Mic");
    assert_eq!(cell("AG12").keycap_id, "Play");
    for bound in ["AG06", "AG11", "AG12"] {
        assert!(cell(bound).is_available, "Grok Bot {bound} must be bound");
    }
    for unbound in [
        "AG00", "AG01", "AG02", "AG05", "AG07", "AG08", "AG09", "AG10",
    ] {
        assert!(
            !cell(unbound).is_available,
            "Grok Bot {unbound} must remain unbound"
        );
    }
}
