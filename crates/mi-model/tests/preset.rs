use mi_model::{
    document::new_id,
    draft::{Action, Draft},
    preset,
    storage::DocumentStore,
    Source,
};
use serde_json::{json, Value};
use std::{collections::BTreeSet, fs};

fn local_codex_ids(draft: &Draft) -> (String, String) {
    let (pid, profile) = draft
        .document
        .editable
        .graph
        .profiles
        .iter()
        .find(|(_, profile)| profile.name == preset::LOCAL_CODEX_PROFILE_NAME)
        .unwrap();
    (pid.clone(), profile.layer_order[0].clone())
}

fn r25_local_codex_starter() -> Draft {
    let mut draft = Draft::default();
    preset::install_agent_starter_profiles(&mut draft).unwrap();
    let (pid, lid) = local_codex_ids(&draft);
    for slot in 0..=5 {
        draft.unassign(&json!(pid), &json!(lid), slot).unwrap();
    }
    for (slot, id, name, detail, payload) in [
        (
            0,
            "desktop-open",
            "打开 / 聚焦应用",
            "com.openai.codex",
            json!({"type": "open_app", "bundle_id": "com.openai.codex"}),
        ),
        (
            1,
            "desktop-input",
            "聚焦当前输入框",
            "com.openai.codex",
            json!({"type": "agent_command", "adapter": "codex", "command": "focus-input"}),
        ),
        (
            2,
            "desktop-search",
            "搜索任务",
            "com.openai.codex",
            json!({"type": "agent_command", "adapter": "codex", "command": "search"}),
        ),
        (
            4,
            "desktop-model",
            "选择模型",
            "目标应用的原生控件；未提供时明确提示",
            json!({"type": "shortcut", "key": "M", "modifiers": ["control", "shift"], "target": "com.openai.codex"}),
        ),
        (
            5,
            "desktop-effort",
            "调整思考力度",
            "目标应用的原生控件；未提供时明确提示",
            json!({"type": "shortcut", "key": "M", "modifiers": ["control", "shift"], "target": "com.openai.codex"}),
        ),
    ] {
        let category = if slot == 0 { "应用" } else { "Agent" };
        draft
            .assign(
                &json!(pid),
                &json!(lid),
                slot,
                Action {
                    id: id.into(),
                    name: name.into(),
                    category: category.into(),
                    detail: detail.into(),
                    payload,
                },
            )
            .unwrap();
    }
    draft
}

fn previous_local_codex_starter() -> Draft {
    let mut draft = r25_local_codex_starter();
    let (pid, lid) = local_codex_ids(&draft);
    for slot in 6..=12 {
        draft.unassign(&json!(pid), &json!(lid), slot).unwrap();
    }
    for (slot, id, name, payload) in [
        (
            6,
            "desktop-dictation",
            "按住说话 · 松开停止",
            json!({
                "type": "agent_command", "adapter": "codex", "command": "push-to-talk"
            }),
        ),
        (
            7,
            "desktop-browser",
            "打开内置浏览器",
            json!({
                "type": "shortcut", "key": "T", "modifiers": ["command"], "target": "com.openai.codex"
            }),
        ),
        (
            8,
            "desktop-terminal",
            "打开终端面板",
            json!({
                "type": "shortcut", "key": "`", "modifiers": ["control"], "target": "com.openai.codex"
            }),
        ),
    ] {
        draft
            .assign(
                &json!(pid),
                &json!(lid),
                slot,
                Action {
                    id: id.into(),
                    name: name.into(),
                    category: "Agent".into(),
                    detail: "目标应用的原生控件；未提供时明确提示".into(),
                    payload,
                },
            )
            .unwrap();
    }
    draft
}

#[test]
fn local_codex_top_six_route_to_session_slot_commands() {
    use mi_model::host_router::{resolve_host_action, ResolvedHostAction};
    let mut draft = Draft::default();
    preset::install_agent_starter_profiles(&mut draft).unwrap();
    let (pid, lid) = local_codex_ids(&draft);
    let layer = &draft.document.editable.graph.layers[&lid];
    for slot in 0..6u32 {
        assert_eq!(
            resolve_host_action(&draft.document, Some(&pid), &format!("AG{slot:02}"), true),
            Some(ResolvedHostAction::AgentCommand {
                adapter: "four-app".into(),
                command: format!("slot-open:{slot}"),
            })
        );
        let binding = layer
            .controls
            .values()
            .find(|binding| binding.logical_control == format!("logical-{slot}"))
            .unwrap();
        let action = &draft.document.editable.graph.actions[binding.action_id.as_ref().unwrap()];
        assert_eq!(action.id, format!("slot-open-{slot}"));
        assert_eq!(action.name, format!("会话 {}", slot + 1));
        assert_eq!(action.category, "Codex 会话");
        assert_eq!(action.detail, "切换并回读对应状态的 Codex 会话");
    }
}

#[test]
fn local_codex_bottom_seven_route_to_distinct_commands() {
    use mi_model::host_router::{resolve_host_action, ResolvedHostAction};
    let mut draft = Draft::default();
    preset::install_agent_starter_profiles(&mut draft).unwrap();
    let (pid, lid) = local_codex_ids(&draft);
    for (slot, command) in [
        (6, "live-voice"),
        (7, "focus-input"),
        (8, "terminal"),
        (9, "cycle-workspace-layout"),
        (10, "new-chat"),
        (11, "toggle-dictation"),
        (12, "submit"),
    ] {
        assert_eq!(
            resolve_host_action(&draft.document, Some(&pid), &format!("AG{slot:02}"), true),
            Some(ResolvedHostAction::AgentCommand {
                adapter: "codex".into(),
                command: command.into(),
            })
        );
        let layer = &draft.document.editable.graph.layers[&lid];
        let binding = layer
            .controls
            .values()
            .find(|binding| binding.logical_control == format!("logical-{slot}"))
            .unwrap();
        let action = &draft.document.editable.graph.actions[binding.action_id.as_ref().unwrap()];
        assert!(
            mi_model::action_library::capability(action, false).execution_available,
            "{command}"
        );
        if slot == 11 {
            assert_eq!(action.name, "点击听写");
            assert_eq!(action.detail, "按一次开始听写，再按一次停止");
        }
    }
}

#[test]
fn unchanged_local_codex_starters_migrate_top_six_and_bottom_seven_preserving_native_profile() {
    use mi_model::host_router::{resolve_host_action, ResolvedHostAction};
    for mut draft in [r25_local_codex_starter(), previous_local_codex_starter()] {
        let (pid, _) = local_codex_ids(&draft);
        assert!(preset::local_codex_profile_needs_starter_slot_migration(
            &draft.document,
            &pid
        ));
        let native_pid = draft
            .document
            .editable
            .graph
            .profiles
            .iter()
            .find(|(_, profile)| profile.name == "Codex")
            .unwrap()
            .0
            .clone();
        let native_lid = draft.document.editable.graph.profiles[&native_pid].layer_order[0].clone();
        let native_before =
            serde_json::to_value(&draft.document.editable.graph.layers[&native_lid]).unwrap();
        let before = draft.document.editable.clone();
        preset::install_agent_starter_profiles(&mut draft).unwrap();
        assert!(draft.document.editable != before);
        for slot in 0..6u32 {
            assert_eq!(
                resolve_host_action(&draft.document, Some(&pid), &format!("AG{slot:02}"), true),
                Some(ResolvedHostAction::AgentCommand {
                    adapter: "four-app".into(),
                    command: format!("slot-open:{slot}"),
                })
            );
        }
        assert_eq!(
            serde_json::to_value(&draft.document.editable.graph.layers[&native_lid]).unwrap(),
            native_before
        );
        for (slot, command) in [
            (6, "live-voice"),
            (7, "focus-input"),
            (8, "terminal"),
            (9, "cycle-workspace-layout"),
            (10, "new-chat"),
            (11, "toggle-dictation"),
            (12, "submit"),
        ] {
            assert_eq!(
                resolve_host_action(&draft.document, Some(&pid), &format!("AG{slot:02}"), true),
                Some(ResolvedHostAction::AgentCommand {
                    adapter: "codex".into(),
                    command: command.into()
                })
            );
        }
        assert!(!preset::local_codex_profile_needs_starter_slot_migration(
            &draft.document,
            &pid
        ));
        let migrated = draft.document.editable.clone();
        preset::install_agent_starter_profiles(&mut draft).unwrap();
        assert!(draft.document.editable == migrated);
        draft.undo(false).unwrap();
        assert!(draft.document.editable == before);
    }
}

#[test]
fn customized_local_codex_assignments_are_never_migrated() {
    for mut draft in [r25_local_codex_starter(), previous_local_codex_starter()] {
        for slot in [0, 3, 6, 7, 9] {
            let mut copy = draft.clone();
            let (pid, lid) = local_codex_ids(&copy);
            copy.assign(&json!(pid), &json!(lid), slot, Action {
                id: "my-shortcut".into(), name: "我的动作".into(), category: "快捷键".into(), detail: "custom".into(),
                payload: json!({"type": "shortcut", "key": "X", "modifiers": ["command"], "target": "com.openai.codex"}),
            }).unwrap();
            let before = copy.document.editable.clone();
            assert!(!preset::local_codex_profile_needs_starter_slot_migration(
                &copy.document,
                &pid
            ));
            preset::install_agent_starter_profiles(&mut copy).unwrap();
            assert!(copy.document.editable == before, "customized slot {slot}");
        }
        let _ = &mut draft;
    }
}

#[test]
fn local_codex_migration_preserves_customized_profile_structure_and_action_metadata() {
    for customization in [
        "layer-name",
        "extra-layer",
        "joystick-mode",
        "sector-count",
        "lighting",
        "action-id",
        "action-name",
        "action-category",
        "action-detail",
    ] {
        let mut draft = previous_local_codex_starter();
        let (pid, lid) = local_codex_ids(&draft);
        match customization {
            "layer-name" => draft.rename_layer(&lid, "我的层").unwrap(),
            "extra-layer" => {
                draft.create_layer(&pid, "第二层").unwrap();
            }
            "joystick-mode" | "sector-count" => draft
                .set_control_settings(
                    &pid,
                    &lid,
                    mi_model::control_settings::ControlSettings {
                        joystick_mode: if customization == "joystick-mode" {
                            "source"
                        } else {
                            "logical_sectors"
                        }
                        .into(),
                        active_sectors: if customization == "sector-count" {
                            8
                        } else {
                            4
                        },
                    },
                )
                .unwrap(),
            _ => draft
                .mutate(|doc| {
                    let layer = doc.editable.graph.layers.get_mut(&lid).unwrap();
                    if customization == "lighting" {
                        layer.lighting = Some(mi_model::lighting::Lighting {
                            backlight: json!({"mode":"off"}),
                            underglow: json!({"mode":"off"}),
                        });
                    } else {
                        let aid = layer
                            .controls
                            .values()
                            .find(|b| b.logical_control == "logical-6")
                            .unwrap()
                            .action_id
                            .clone()
                            .unwrap();
                        let action = doc.editable.graph.actions.get_mut(&aid).unwrap();
                        match customization {
                            "action-id" => action.id = "my-dictation".into(),
                            "action-name" => action.name = "我的听写".into(),
                            "action-category" => action.category = "我的分类".into(),
                            "action-detail" => action.detail = "我的说明".into(),
                            _ => unreachable!(),
                        }
                    }
                    Ok(())
                })
                .unwrap(),
        }
        assert!(
            !preset::local_codex_profile_needs_starter_slot_migration(&draft.document, &pid),
            "{customization}"
        );
        let before = draft.document.editable.clone();
        preset::install_agent_starter_profiles(&mut draft).unwrap();
        assert!(draft.document.editable == before, "{customization}");
    }
}

fn fixture() -> Draft {
    let mut d = Draft::default();
    let p = d.config.profiles[0].id.clone();
    let l = d.config.profiles[0].layers[0].id.clone();
    let macro_id = new_id();
    let multi_id = new_id();
    let tool_id = new_id();
    let g = &mut d.document.editable.graph;
    let macro_action = mi_model::draft::actions()
        .into_iter()
        .find(|a| a.id == "local-macro")
        .unwrap();
    let tool = mi_model::draft::actions()
        .into_iter()
        .find(|a| a.id == "session")
        .unwrap();
    g.actions.insert(macro_id.clone(), macro_action);
    g.actions.insert(tool_id.clone(), tool);
    g.actions.insert(multi_id.clone(),Action {id:"local-multi".into(), name:"共享四分支".into(),category:"Multi".into(),detail:"本地".into(), payload:json!({"type":"local_multi","multi":{"format_version":1,"term_ms":250,"tap":{"type":"macro","action_id":macro_id},"hold":{"type":"action","action_id":tool_id},"double_tap":{"type":"macro","action_id":macro_id},"tap_hold":null}})});
    let layer = g.layers.get_mut(l.as_str().unwrap()).unwrap();
    for slot in [0, 1] {
        layer
            .controls
            .get_mut(&layer.control_order[slot])
            .unwrap()
            .action_id = Some(multi_id.clone());
    }
    d.document.origin["config"]["raw"]["future_metadata"] =
        json!({"counter":u64::MAX,"color":"未知"});
    d.document.baseline = Some(json!({"private_device_fact":"historical-only"}));
    d.document.validate().unwrap();
    let mut d = Draft::from_document(d.document, 2, false).unwrap();
    d.assign(&p, &l, 2, mi_model::draft::actions()[0].clone())
        .unwrap();
    d
}
fn ids(d: &Draft) -> BTreeSet<String> {
    let g = &d.document.editable.graph;
    g.profiles
        .keys()
        .chain(g.layers.keys())
        .chain(g.actions.keys())
        .cloned()
        .chain(g.layers.values().flat_map(|l| l.controls.keys().cloned()))
        .collect()
}
#[test]
fn round_trip_remaps_shared_multi_macro_and_tool_references_preserving_raw_and_metadata() {
    let d = fixture();
    let exported = preset::export(&d, None, None).unwrap();
    let imported = preset::import(&exported).unwrap();
    imported.document.validate().unwrap();
    assert!(ids(&d).is_disjoint(&ids(&imported)));
    assert_ne!(d.document_id, imported.document_id);
    assert!(imported.document.baseline.is_none());
    assert!(matches!(imported.config.source, Source::LocalDraft));
    assert_eq!(
        imported.document.origin["imported_baseline"],
        d.document.baseline.clone().unwrap()
    );
    assert_eq!(
        imported.document.origin["config"],
        d.document.origin["config"]
    );
    let p = &imported.document.editable.graph.profile_order[0];
    let g = &imported.document.editable.graph;
    let l = &g.layers[&g.profiles[p].layer_order[0]];
    let first = l.controls[&l.control_order[0]].action_id.as_ref().unwrap();
    assert_eq!(
        Some(first),
        l.controls[&l.control_order[1]].action_id.as_ref()
    );
    let m = mi_model::multi::local_multi(&g.actions[first]).unwrap();
    assert_eq!(
        m.tap.as_ref().unwrap().id(),
        m.double_tap.as_ref().unwrap().id()
    );
    assert_eq!(
        g.actions[m.tap.unwrap().id()].payload["type"],
        "local_macro"
    );
    assert_eq!(g.actions[m.hold.unwrap().id()].payload["adapter"], "dsh");
    assert_eq!(imported.revision, 0);
    assert!(!imported.can_undo);
    let twice = preset::import(&preset::export(&imported, None, None).unwrap()).unwrap();
    assert_eq!(twice.config.raw, imported.config.raw);
}
#[test]
fn scoped_exports_keep_transitive_dependencies_and_only_selected_editable_nodes() {
    let mut d = fixture();
    let unused = new_id();
    d.document
        .editable
        .graph
        .actions
        .insert(unused.clone(), mi_model::draft::actions()[3].clone());
    let p = d.document.editable.graph.profile_order[0].clone();
    let l = d.document.editable.graph.profiles[&p].layer_order[0].clone();
    for layer in [None, Some(l.as_str())] {
        let text = preset::export(&d, Some(&p), layer).unwrap();
        let parsed = preset::parse(&text).unwrap();
        assert!(parsed.document["graph"]["actions"].get(&unused).is_none());
        let i = preset::import(&text).unwrap();
        assert_eq!(i.document.editable.graph.profiles.len(), 1);
        assert_eq!(
            i.document.editable.graph.layers.len(),
            if layer.is_some() { 1 } else { 2 }
        );
        assert_eq!(i.document.editable.graph.actions.len(), 4);
        assert_eq!(i.document.origin["config"], d.document.origin["config"]);
    }
    assert!(preset::export(&d, None, Some(&l)).is_err());
}
#[test]
fn invalid_versions_unknown_fields_and_dangling_references_are_rejected() {
    let text = preset::export(&fixture(), None, None).unwrap();
    let raw: Value = serde_json::from_str(&text).unwrap();
    let mut bad = raw.clone();
    bad["version"] = json!(2);
    assert!(preset::import(&bad.to_string()).is_err());
    let mut bad = raw.clone();
    bad["document"]["future"] = json!("keep me");
    assert!(preset::import(&bad.to_string()).is_err());
    let mut bad = raw.clone();
    bad["document"]["graph"]["actions"] = json!({});
    assert!(preset::import(&bad.to_string()).is_err());
    assert!(preset::import(&" ".repeat(preset::LIMIT + 1)).is_err());
    assert!(preset::import("{broken").is_err());
}
#[test]
fn import_is_independent_persistent_and_conflicts_preserve_existing_selection() {
    let dir = std::env::temp_dir().join(format!("mi-preset-{}", new_id()));
    fs::create_dir(&dir).unwrap();
    let path = dir.join("workbench.json");
    fs::write(&path, serde_json::to_vec(&fixture().document).unwrap()).unwrap();
    let original = fs::read(&path).unwrap();
    let mut store = DocumentStore::default();
    store.open(path.clone()).unwrap();
    let mut stale = DocumentStore::default();
    stale.open(path.clone()).unwrap();
    let text = preset::starter().unwrap();
    assert_eq!(preset::parse(&text).unwrap().name, "Coding 起步");
    store.import_preset(&text).unwrap();
    assert_eq!(fs::read(&path).unwrap(), original);
    let current = store.draft.document_id.clone();
    let mut reopened = DocumentStore::default();
    reopened.open(path.clone()).unwrap();
    assert_eq!(reopened.draft.document_id, current);
    assert_eq!(reopened.draft.config.profiles[0].name, "Coding 起步");
    let selection = fs::read(path.with_extension("active.json")).unwrap();
    assert!(stale.import_preset(&text).is_err());
    assert!(store.import_preset("{}").is_err());
    assert_eq!(
        fs::read(path.with_extension("active.json")).unwrap(),
        selection
    );
    assert_eq!(store.draft.document_id, current);
    store.import_preset(&text).unwrap();
    assert_ne!(store.draft.document_id, current);
    assert_eq!(store.draft.config.profiles[0].name, "Coding 起步");
    assert_eq!(fs::read(&path).unwrap(), original);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn source_archive_is_not_duplicated_on_import() {
    let mut d = Draft::default();
    d.document.origin["config"]["raw"]["large_unknown"] = json!("x".repeat(2_200_000));
    let text = preset::export(&d, None, None).unwrap();
    let imported = preset::import(&text).unwrap();
    let saved = serde_json::to_vec_pretty(&imported.document).unwrap();
    assert!(saved.len() <= preset::LIMIT);
    assert_eq!(
        imported.document.origin["config"],
        d.document.origin["config"]
    );
    assert!(imported.document.origin["imported_origin"]
        .get("config")
        .is_none());
}

#[test]
fn starter_contains_only_original_local_content() {
    let d = preset::import(&preset::starter().unwrap()).unwrap();
    assert_eq!(d.document.editable.graph.actions.len(), 2);
    let config = &d.document.origin["config"];
    assert_eq!(config["macros"], json!([]));
    assert_eq!(config["multi_actions"], json!([]));
    assert_eq!(config["warnings"], json!([]));
    assert_eq!(config["read_at"], 0);
    assert!(config["firmware"].is_null());
    assert!(config["smart_actions"].is_null());
}

#[test]
fn six_agent_presets_and_paseo_actions_round_trip_across_all_export_scopes() {
    let mut d = Draft::default();
    let ids = preset::install_agent_starter_profiles(&mut d).unwrap();
    assert_eq!(ids.len(), 6);

    // Bind Slice C & D actions (navigation, prompts, submit) onto Antigravity before exporting
    let ag_pid = ids[0].clone();
    let ag_lid = d.document.editable.graph.profiles[&ag_pid].layer_order[0].clone();
    let catalog = mi_model::draft::actions();
    let find_cat = |id: &str| catalog.iter().find(|a| a.id == id).unwrap().clone();
    for (slot, action_id) in [
        (9, "navigate-back"),
        (10, "navigate-forward"),
        (12, "agent-submit"),
        (16, "prompt-continue"),
        (17, "prompt-test"),
        (18, "prompt-review"),
        (19, "prompt-summary"),
    ] {
        d.assign(&json!(ag_pid), &json!(ag_lid), slot, find_cat(action_id))
            .unwrap();
    }

    // 1. Full document export/import round-trip
    let doc_text = preset::export(&d, None, None).unwrap();
    let doc_imported = preset::import(&doc_text).unwrap();
    assert_eq!(doc_imported.config.profiles.len(), 6);
    let imported_ag_pid = doc_imported
        .document
        .editable
        .graph
        .profiles
        .iter()
        .find(|(_, p)| p.name == "Antigravity")
        .unwrap()
        .0
        .clone();
    for (binding, expected) in [
        (
            "AG13",
            mi_model::host_router::ResolvedHostAction::AgentCommand {
                adapter: "antigravity".into(),
                command: "prev-conversation".into(),
            },
        ),
        (
            "ENC_CLK",
            mi_model::host_router::ResolvedHostAction::AgentCommand {
                adapter: "antigravity".into(),
                command: "search".into(),
            },
        ),
        (
            "AG09",
            mi_model::host_router::ResolvedHostAction::AgentCommand {
                adapter: "navigation".into(),
                command: "back".into(),
            },
        ),
        (
            "AG10",
            mi_model::host_router::ResolvedHostAction::AgentCommand {
                adapter: "navigation".into(),
                command: "forward".into(),
            },
        ),
        (
            "AG12",
            mi_model::host_router::ResolvedHostAction::AgentCommand {
                adapter: "antigravity".into(),
                command: "submit".into(),
            },
        ),
        (
            "AG15",
            mi_model::host_router::ResolvedHostAction::PasteText {
                text: "Continue".into(),
                target_bundle_id: Some("com.google.antigravity".into()),
                adapter: Some("antigravity".into()),
                no_newline: true,
                submit: false,
            },
        ),
    ] {
        assert_eq!(
            mi_model::host_router::resolve_host_action(
                &doc_imported.document,
                Some(&imported_ag_pid),
                binding,
                true
            ),
            Some(expected),
            "Imported {binding} must preserve exact Paseo/Agent routing semantics"
        );
    }

    // 2. Profile-scoped and Layer-scoped export/import round-trip
    for scope_layer in [None, Some(ag_lid.as_str())] {
        let scoped_text = preset::export(&d, Some(&ag_pid), scope_layer).unwrap();
        let scoped_imported = preset::import(&scoped_text).unwrap();
        assert_eq!(scoped_imported.config.profiles.len(), 1);
        assert_eq!(scoped_imported.config.profiles[0].name, "Antigravity");
        let pid = &scoped_imported.document.editable.graph.profile_order[0];
        assert!(mi_model::host_router::resolve_host_action(
            &scoped_imported.document,
            Some(pid),
            "AG15",
            true
        )
        .is_some());
    }
}

#[test]
fn antigravity_legacy_bottom_row_dictation_upgrades_to_focus_input_and_push_to_talk() {
    let mut d = Draft::default();
    let ids = preset::install_agent_starter_profiles(&mut d).unwrap();
    let ag_pid = ids
        .iter()
        .find(|pid| d.document.editable.graph.profiles[*pid].name == "Antigravity")
        .unwrap()
        .clone();
    let ag_lid = d.document.editable.graph.profiles[&ag_pid].layer_order[0].clone();

    d.assign(
        &json!(ag_pid),
        &json!(ag_lid),
        7,
        mi_model::draft::Action {
            id: "desktop-browser".into(),
            name: "打开内置浏览器".into(),
            category: "Agent".into(),
            detail: "呼出 Antigravity 内置预览浏览器".into(),
            payload: json!({"type":"agent_command","adapter":"antigravity","command":"browser"}),
        },
    )
    .unwrap();
    d.assign(
        &json!(ag_pid),
        &json!(ag_lid),
        10,
        mi_model::draft::Action {
            id: "desktop-dictation".into(),
            name: "按住说话 · 松开停止".into(),
            category: "Agent".into(),
            detail: "2U 宽键按住启动听写".into(),
            payload: json!({"type":"agent_command","adapter":"antigravity","command":"push-to-talk"}),
        },
    )
    .unwrap();
    d.assign(
        &json!(ag_pid),
        &json!(ag_lid),
        11,
        mi_model::draft::Action {
            id: "desktop-dictation-r".into(),
            name: "按住说话 · 松开停止".into(),
            category: "Agent".into(),
            detail: "2U 宽键按住启动听写".into(),
            payload: json!({"type":"agent_command","adapter":"antigravity","command":"push-to-talk"}),
        },
    )
    .unwrap();
    d.assign(
        &json!(ag_pid),
        &json!(ag_lid),
        9,
        mi_model::draft::Action {
            id: "custom-user-key".into(),
            name: "用户自定义键".into(),
            category: "文本".into(),
            detail: "保留".into(),
            payload: json!({"type":"text","text":"keep-me"}),
        },
    )
    .unwrap();

    assert!(preset::antigravity_layer_needs_starter_slot_migration(
        &d.document,
        &ag_lid
    ));

    preset::install_agent_starter_profiles(&mut d).unwrap();

    assert!(!preset::antigravity_layer_needs_starter_slot_migration(
        &d.document,
        &ag_lid
    ));

    let layer = &d.document.editable.graph.layers[&ag_lid];
    let action_for = |slot: &str| {
        let aid = layer
            .controls
            .values()
            .find(|b| b.logical_control == slot)
            .and_then(|b| b.action_id.as_ref())
            .unwrap();
        &d.document.editable.graph.actions[aid]
    };
    let a7 = action_for("logical-7");
    assert_eq!(a7.id, "desktop-input");
    assert_eq!(a7.name, "聚焦当前输入框");
    assert_eq!(a7.payload["command"], "focus-input");

    let a10 = action_for("logical-10");
    assert_eq!(a10.id, "desktop-new-project-chat");
    assert_eq!(a10.name, "在项目中新建会话");
    assert_eq!(a10.payload["command"], "new-conversation-in-project");

    let a11 = action_for("logical-11");
    assert_eq!(a11.id, "desktop-dictation");
    assert_eq!(a11.name, "按住说话 · 松开停止");
    assert_eq!(a11.payload["command"], "push-to-talk");

    let a12 = action_for("logical-12");
    assert_eq!(a12.id, "desktop-submit");
    assert_eq!(a12.name, "发送消息（先停听写）");
    assert_eq!(a12.payload["command"], "submit");

    let a9 = action_for("logical-9");
    assert_eq!(a9.id, "custom-user-key");
}
