//! Workbench-owned host preferences. One atomic save per routing transition.
use super::DocumentStore;
use serde::Deserialize;
use serde_json::{json, Value};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingRequest {
    pub document_id: String,
    pub document_revision: u64,
    pub host_state_revision: u64,
    pub operation: RoutingOperation,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoutingOperation {
    Select {
        profile_id: String,
    },
    Enable {
        enabled: bool,
    },
    Execution {
        enabled: bool,
    },
    CommandExecution {
        enabled: bool,
    },
    Resume,
    Upsert {
        bundle_id: String,
        profile_id: String,
    },
    Remove {
        bundle_id: String,
    },
    Follow {
        bundle_id: String,
    },
}
impl DocumentStore {
    pub fn host_revision(&self) -> u64 {
        if let Some(host) = &self.host {
            return host.host_revision();
        }
        self.host_state_revision
    }
    pub fn routing_state(&self) -> Value {
        if let Some(host) = &self.host {
            return host.routing_state();
        }
        let p = &self.preferences.value;
        let graph = &self.draft.document.editable.graph;
        let profile = p
            .host_active_profile_id
            .as_ref()
            .and_then(|id| graph.profiles.get(id));
        let bindings = [
            "AG00", "AG01", "AG02", "AG03", "AG04", "AG05", "AG06", "AG07", "AG08", "AG09", "AG10",
            "AG12", "AG13", "AG14", "ENC_CLK", "AG15", "AG16", "AG17", "AG18",
        ];
        let keys: Vec<Value> = bindings.iter().map(|binding| {
            use crate::host_router::ResolvedHostAction::*;
            let action = crate::host_router::resolve_host_action_with_command_consent(&self.draft.document, p.host_active_profile_id.as_deref(), binding, true, p.command_execution_enabled);
            let bound_action = profile
                .and_then(|prof| prof.layer_order.first())
                .and_then(|lid| graph.layers.get(lid))
                .and_then(|layer| {
                    let slot = crate::host_router::binding_to_logical_slot(binding)?;
                    let find_id = |s: &str| {
                        layer
                            .controls
                            .values()
                            .find(|b| b.logical_control == s)
                            .and_then(|b| b.action_id.as_ref())
                    };
                    let aid = find_id(slot).or_else(|| match slot {
                        "logical-10" => find_id("logical-11"),
                        "logical-11" => find_id("logical-10"),
                        _ => None,
                    })?;
                    graph.actions.get(aid)
                });
            let label = match &action {
                Some(Shortcut{key,modifiers,..}) => {
                    let combo = if modifiers.is_empty() {
                        key.clone()
                    } else {
                        format!("{} {}", modifiers.join("+"), key)
                    };
                    if let Some(a) = bound_action.filter(|a| {
                        matches!(
                            a.id.as_str(),
                            "herdr-desktop"
                                | "desktop-search"
                                | "desktop-new"
                                | "desktop-model"
                                | "desktop-effort"
                                | "desktop-browser"
                                | "desktop-terminal"
                        )
                    }) {
                        format!("{} ({combo})", a.name)
                    } else {
                        combo
                    }
                }
                Some(PasteText{..}) => match bound_action.map(|a| a.id.as_str()) {
                    Some("prompt-continue") => "粘贴文本 · Continue".into(),
                    Some("prompt-test") => "粘贴文本 · Test".into(),
                    Some("prompt-review") => "粘贴文本 · Review".into(),
                    Some("prompt-commit") => "粘贴文本 · Commit".into(),
                    Some("prompt-summary") => "粘贴文本 · Summary".into(),
                    _ => "粘贴文本".into(),
                },
                Some(OpenApp{bundle_id}) => format!("打开 {}", bundle_id),
                Some(ShellCommand{working_directory,..}) => format!("命令 · {}", working_directory),
                Some(OpenUrl{..}) => "打开网址".into(),
                Some(AgentCommand{command,..}) => match command.as_str() {
                    "focus-input" => "聚焦输入框", "search" => "搜索会话", "new-chat" => "新建会话",
                    "model" => "选择模型", "effort" => "思考力度", "dictation" => "听写 / 语音输入",
                    "push-to-talk" => "按住说话",
                    "voice-reply" | "toggle-voice-reply" => "切换语音回复",
                    "browser" => "打开内置浏览器", "terminal" => "打开终端面板",
                    "scroll-up" => "向上滚动对话", "scroll-down" => "向下滚动对话",
                    "scroll-up-fast" => "快速向上滚动", "scroll-down-fast" => "快速向下滚动",
                    "scroll-to-bottom" => "跳到底部并聚焦输入",
                    "back" | "navigate-back" => "后退到上一个打开目标",
                    "forward" | "navigate-forward" => "前进到下一个打开目标",
                    "next-attention" => "下一个需要我",
                    "submit" => "停止听写并提交",
                    c if c.starts_with("submit-prompt:") => "粘贴指令并提交",
                    c if c.starts_with("prompt:") => "粘贴工程指令",
                    _ => "Agent 动作",
                }.into(),
                Some(FocusSession{..}) => "聚焦会话".into(),
                Some(MacroSequence{events}) => format!("本地宏 · {} 步", events.len()),
                None => "不可用 / 未配置".into(),
            };
            let spec = crate::host_router::verified_physical_mapping(binding).unwrap();
            json!({"binding_id":binding,"control":spec.physical_control,"label":label,"available":action.is_some()})
        }).collect();
        json!({"document_id":self.draft.document_id,"document_revision":self.draft.revision,
            "host_state_revision":self.host_state_revision,"profile_id":p.host_active_profile_id,
            "profile_name":profile.map(|p| &p.name),"execution_enabled":p.host_execution_enabled,
            "command_execution_enabled":p.command_execution_enabled,
            "auto_follow_enabled":p.auto_follow_enabled,"manual_override":p.manual_override,
            "app_profile_links":p.app_profile_links,"keys":keys})
    }
    pub fn update_routing(&mut self, request: RoutingRequest) -> Result<(), String> {
        if let Some(host) = &mut self.host {
            return host.update_routing(request);
        }
        if self.blocked
            || request.document_id != self.draft.document_id
            || request.document_revision != self.draft.revision
            || request.host_state_revision != self.host_state_revision
        {
            return Err("路由状态已变化；请刷新后重试".into());
        }
        let mut next = self.preferences.value.clone();
        let valid = |id: &String| self.draft.document.editable.graph.profiles.contains_key(id);
        match request.operation {
            RoutingOperation::Select { profile_id } => {
                if !valid(&profile_id) {
                    return Err("Profile 不存在".into());
                }
                next.host_active_profile_id = Some(profile_id);
                next.manual_override = true;
            }
            RoutingOperation::Enable { enabled } => next.auto_follow_enabled = enabled,
            RoutingOperation::Execution { enabled } => next.host_execution_enabled = enabled,
            RoutingOperation::CommandExecution { enabled } => {
                next.command_execution_enabled = enabled
            }
            RoutingOperation::Resume => next.manual_override = false,
            RoutingOperation::Upsert {
                bundle_id,
                profile_id,
            } => {
                if bundle_id.trim().is_empty() || !valid(&profile_id) {
                    return Err("应用或 Profile 无效".into());
                }
                next.app_profile_links.insert(bundle_id, profile_id);
            }
            RoutingOperation::Remove { bundle_id } => {
                next.app_profile_links.remove(&bundle_id);
            }
            RoutingOperation::Follow { bundle_id } => {
                if next.auto_follow_enabled && !next.manual_override {
                    if let Some(id) = next
                        .app_profile_links
                        .get(&bundle_id)
                        .filter(|id| valid(id))
                    {
                        next.host_active_profile_id = Some(id.clone());
                    }
                }
            }
        }
        if next != self.preferences.value {
            self.preferences.save(next)?;
            self.host_state_revision += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(store: &DocumentStore, operation: Value) -> RoutingRequest {
        serde_json::from_value(json!({"document_id":store.draft.document_id,"document_revision":store.draft.revision,"host_state_revision":store.host_revision(),"operation":operation})).unwrap()
    }
    #[test]
    fn defaults_atomic_selection_stale_guard_and_missing_link() {
        let mut store = DocumentStore::default();
        let ids = crate::preset::install_agent_starter_profiles(&mut store.draft).unwrap();
        assert_eq!(store.routing_state()["auto_follow_enabled"], false);
        assert_eq!(store.routing_state()["manual_override"], false);
        let old = request(&store, json!({"kind":"resume"}));
        let select = request(&store, json!({"kind":"select","profile_id":ids[0]}));
        store.update_routing(select).unwrap();
        assert_eq!(store.host_revision(), 1);
        assert_eq!(store.routing_state()["manual_override"], true);
        // The HUD must keep physical left-to-right order, not the former inverted IDs.
        let keys = store.routing_state()["keys"].as_array().unwrap().clone();
        assert_eq!(keys[0]["binding_id"], "AG00");
        assert_eq!(keys[0]["control"], "row0-col0-left");
        assert_eq!(keys[1]["binding_id"], "AG01");
        assert_eq!(keys[1]["control"], "row0-col1-right");
        assert!(store.update_routing(old).is_err());
        let same = request(&store, json!({"kind":"select","profile_id":ids[0]}));
        store.update_routing(same).unwrap();
        assert_eq!(store.host_revision(), 1);
        for op in [
            json!({"kind":"enable","enabled":true}),
            json!({"kind":"resume"}),
            json!({"kind":"follow","bundle_id":"unmapped"}),
        ] {
            let request = request(&store, op);
            store.update_routing(request).unwrap();
        }
        assert_eq!(store.host_active_profile_id(), Some(ids[0].clone()));
        assert_eq!(store.routing_state()["manual_override"], false);
        assert!(!store.host_execution_enabled());
        let command = request(&store, json!({"kind":"command_execution","enabled":true}));
        store.update_routing(command).unwrap();
        assert_eq!(store.routing_state()["command_execution_enabled"], true);
        assert_eq!(store.command_execution_enabled(), true);
    }
    #[test]
    fn legacy_defaults_and_failed_save_do_not_change_profile_or_override() {
        let dir = std::env::temp_dir().join(format!("mi-routing-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("workbench.json");
        std::fs::write(
            path.with_extension("preferences.json"),
            r#"{"schema_version":1,"favorites":[],"host_execution_enabled":false}"#,
        )
        .unwrap();
        let mut store = DocumentStore::default();
        store.open(path.clone()).unwrap();
        let id = store.draft.document.editable.graph.profile_order[0].clone();
        assert_eq!(store.routing_state()["manual_override"], false);
        let before = store.routing_state();
        std::fs::write(path.with_extension("preferences.json"), "external change").unwrap();
        let req = request(&store, json!({"kind":"select","profile_id":id}));
        assert!(store.update_routing(req).is_err());
        assert_eq!(store.routing_state(), before);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cheat_sheet_hud_displays_all_8_agent_presets_dial_joystick_and_redacts_custom_text() {
        let mut store = DocumentStore::default();
        let ids = crate::preset::install_agent_starter_profiles(&mut store.draft).unwrap();
        assert_eq!(ids.len(), 8);

        for pid in &ids {
            let sel = request(&store, json!({"kind":"select","profile_id":pid}));
            store.update_routing(sel).unwrap();
            let state = store.routing_state();
            let profile_name = state["profile_name"].as_str().unwrap().to_owned();
            let keys = state["keys"].as_array().unwrap();
            assert_eq!(
                keys.len(),
                19,
                "{profile_name} cheat sheet must expose 19 verified controls"
            );

            let by_id = |b: &str| -> &Value {
                keys.iter()
                    .find(|k| k["binding_id"] == b)
                    .unwrap_or_else(|| panic!("missing {b} in {profile_name}"))
            };

            assert_eq!(by_id("AG00")["available"], true);
            assert!(by_id("AG00")["label"]
                .as_str()
                .unwrap()
                .starts_with("打开 "));

            if profile_name == "Herdr" {
                assert_eq!(by_id("AG01")["label"], "选择 Space / Tab (command p)");
                assert_eq!(by_id("ENC_CLK")["label"], "显示 / 隐藏侧栏 (command b)");
                assert_eq!(by_id("AG15")["label"], "聚焦上方面板 (command+option Up)");
                assert_eq!(by_id("AG16")["label"], "聚焦左方面板 (command+option Left)");
                assert_eq!(by_id("AG17")["label"], "聚焦下方面板 (command+option Down)");
                assert_eq!(
                    by_id("AG18")["label"],
                    "聚焦右方面板 (command+option Right)"
                );
            } else {
                assert_eq!(by_id("AG01")["label"], "聚焦输入框");
                assert_eq!(by_id("AG13")["label"], "向上滚动对话");
                assert_eq!(by_id("AG14")["label"], "向下滚动对话");
                assert_eq!(by_id("ENC_CLK")["label"], "跳到底部并聚焦输入");
                if profile_name == "Codex" {
                    assert_eq!(by_id("AG04")["label"], "选择模型 (control+shift M)");
                    assert_eq!(by_id("AG05")["label"], "调整思考力度 (control+shift M)");
                    assert_eq!(by_id("AG07")["label"], "打开内置浏览器 (command T)");
                    assert_eq!(by_id("AG08")["label"], "打开终端面板 (control `)");
                }
            }
        }

        // Verify built-in prompt templates, navigation, T3, submit, and custom text payload redaction
        let ag_pid = &ids[0];
        let lid = store.draft.document.editable.graph.profiles[ag_pid].layer_order[0].clone();
        let catalog = crate::draft::actions();
        let find_cat = |id: &str| catalog.iter().find(|a| a.id == id).unwrap().clone();
        store
            .draft
            .assign(&json!(ag_pid), &json!(lid), 16, find_cat("prompt-continue"))
            .unwrap();
        store
            .draft
            .assign(&json!(ag_pid), &json!(lid), 17, find_cat("navigate-back"))
            .unwrap();
        store
            .draft
            .assign(
                &json!(ag_pid),
                &json!(lid),
                18,
                find_cat("t3-next-attention"),
            )
            .unwrap();
        store
            .draft
            .assign(&json!(ag_pid), &json!(lid), 12, find_cat("agent-submit"))
            .unwrap();
        store
            .draft
            .assign(
                &json!(ag_pid),
                &json!(lid),
                9,
                crate::draft::Action {
                    id: "custom-secret".into(),
                    name: "自定义文本".into(),
                    category: "文本".into(),
                    detail: "保密".into(),
                    payload: json!({"type":"text","text":"super-secret-prompt-payload"}),
                },
            )
            .unwrap();

        let sel = request(&store, json!({"kind":"select","profile_id":ag_pid}));
        store.update_routing(sel).unwrap();
        let state = store.routing_state();
        let serialized = state.to_string();
        assert!(
            !serialized.contains("super-secret-prompt-payload"),
            "Cheat sheet snapshot must never leak raw custom text payload"
        );
        let keys = state["keys"].as_array().unwrap();
        let label_of = |b: &str| {
            keys.iter().find(|k| k["binding_id"] == b).unwrap()["label"]
                .as_str()
                .unwrap()
        };
        assert_eq!(label_of("AG15"), "粘贴文本 · Continue");
        assert_eq!(label_of("AG16"), "后退到上一个打开目标");
        assert_eq!(label_of("AG17"), "下一个需要我");
        assert_eq!(label_of("AG12"), "停止听写并提交");
        assert_eq!(label_of("AG09"), "粘贴文本");
    }
}
