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
    #[serde(alias = "select_profile")]
    Select {
        profile_id: String,
    },
    #[serde(alias = "lock")]
    LockProfile {
        #[serde(default)]
        profile_id: Option<String>,
    },
    #[serde(alias = "set_auto_follow_enabled")]
    Enable {
        enabled: bool,
    },
    Execution {
        enabled: bool,
    },
    CommandExecution {
        enabled: bool,
    },
    #[serde(alias = "resume_auto_follow")]
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
    BottomRowLayout {
        layout: crate::host_router::BottomRowLayout,
    },
    PopoverPreviewMode {
        mode: crate::host_router::PopoverPreviewMode,
    },
}
impl DocumentStore {
    pub fn host_revision(&self) -> u64 {
        if let Some(host) = &self.host {
            return host.host_revision();
        }
        self.host_state_revision
    }
    pub fn resolve_host_input(
        &self,
        binding_id: &str,
        is_pressed: bool,
    ) -> Option<crate::host_router::ResolvedHostAction> {
        let host = self.host_store();
        let active_id = host
            .locked_profile_id()
            .or_else(|| host.preferences.value.host_active_profile_id.clone());
        crate::host_router::resolve_host_action_with_layout(
            &host.draft.document,
            active_id.as_deref(),
            binding_id,
            is_pressed,
            host.preferences.value.command_execution_enabled,
            host.preferences.value.bottom_row_layout,
        )
    }
    pub fn routing_state(&self) -> Value {
        if let Some(host) = &self.host {
            return host.routing_state();
        }
        let p = &self.preferences.value;
        let layout = p.bottom_row_layout;
        let graph = &self.draft.document.editable.graph;
        let locked = self.locked_profile_id();
        let active_profile_id = locked.as_ref().or(p.host_active_profile_id.as_ref());
        let profile = active_profile_id.and_then(|id| graph.profiles.get(id));
        let split_bindings = [
            "AG00", "AG01", "AG02", "AG03", "AG04", "AG05", "AG06", "AG07", "AG08", "AG09", "AG10",
            "AG11", "AG12", "AG13", "AG14", "ENC_CLK", "AG15", "AG16", "AG17", "AG18",
        ];
        let merged_bindings = [
            "AG00", "AG01", "AG02", "AG03", "AG04", "AG05", "AG06", "AG07", "AG08", "AG09", "AG10",
            "AG12", "AG13", "AG14", "ENC_CLK", "AG15", "AG16", "AG17", "AG18",
        ];
        let bindings: &[&str] = if layout.is_split() {
            &split_bindings
        } else {
            &merged_bindings
        };
        let keys: Vec<Value> = bindings.iter().map(|binding| {
            use crate::host_router::ResolvedHostAction::*;
            let action = crate::host_router::resolve_host_action_with_layout(
                &self.draft.document,
                active_profile_id.map(String::as_str),
                binding,
                true,
                p.command_execution_enabled,
                layout,
            );
            let bound_action = profile
                .and_then(|prof| prof.layer_order.first())
                .and_then(|lid| graph.layers.get(lid))
                .and_then(|layer| {
                    let slot = crate::host_router::binding_to_logical_slot(binding)?;
                    let aid = crate::host_router::resolve_layer_slot_action_id(
                        &self.draft.document,
                        layer,
                        slot,
                        layout,
                    )?;
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
                                | "apple-fn-key"
                                | "desktop-search"
                                | "desktop-new"
                                | "desktop-model"
                                | "desktop-effort"
                                | "desktop-browser"
                                | "desktop-terminal"
                                | "desktop-workspace"
                                | "desktop-nav-back"
                                | "desktop-nav-forward"
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
                    "new-conversation-in-project" => "在项目中新建会话",
                    "model" => "选择模型", "effort" => "思考力度", "dictation" => "听写 / 语音输入",
                    "push-to-talk" => "按住说话",
                    "live-voice" => "Gemini Live 实时语音",
                    "project" => "选项目 / 新建项目",
                    "split-right" => "快速向右分栏",
                    "prev-conversation" => "上一会话（侧栏轮转）",
                    "next-conversation" => "下一会话（侧栏轮转）",
                    "voice-reply" | "toggle-voice-reply" => "切换语音回复",
                    "browser" => "打开内置浏览器", "terminal" => "打开终端面板",
                    "scroll-up" => "向上滚动对话", "scroll-down" => "向下滚动对话",
                    "scroll-up-fast" => "快速向上滚动", "scroll-down-fast" => "快速向下滚动",
                    "scroll-to-bottom" => "跳到底部并聚焦输入",
                    "sidebar-scroll-up" => "在项目中向上滚动",
                    "sidebar-scroll-down" => "在项目中向下滚动",
                    "prev-project" => "上一项目",
                    "next-project" => "下一项目",
                    "cycle-working-prev" => "上一个 Working 会话",
                    "cycle-working-next" => "下一个 Working 会话",
                    "cycle-completed-prev" => "上一个蓝点已完成会话",
                    "cycle-completed-next" => "下一个蓝点已完成会话",
                    "cycle-attention-prev" => "上一个 Working 或蓝点会话",
                    "cycle-attention-next" => "下一个 Working 或蓝点会话",
                    "back" | "navigate-back" => "后退到上一个打开目标",
                    "forward" | "navigate-forward" => "前进到下一个打开目标",
                    "next-attention" => "下一个需要我",
                    "submit" => "停止听写并提交",
                    "slot-open:0" => "会话 1",
                    "slot-open:1" => "会话 2",
                    "slot-open:2" => "会话 3",
                    "slot-open:3" => "会话 4",
                    "slot-open:4" => "会话 5",
                    "slot-open:5" => "会话 6",
                    "new-tab" => "新建标签页",
                    "cycle-workspace-layout" => "内置浏览器 · 布局",
                    "toggle-dictation" => "点击听写",
                    "effort-down" => "降低推理档位",
                    "effort-up" => "提高推理档位",
                    "toggle-fast-mode" => "切换 Fast 模式",
                    c if c.starts_with("submit-prompt:") => "粘贴指令并提交",
                    c if c.starts_with("prompt:") => "粘贴工程指令",
                    _ => "Agent 动作",
                }.into(),
                Some(FocusSession{..}) => "聚焦会话".into(),
                Some(MacroSequence{events}) => format!("本地宏 · {} 步", events.len()),
                None => "不可用 / 未配置".into(),
            };
            let spec = crate::host_router::verified_physical_mapping_for_layout(binding, layout).unwrap();
            json!({"binding_id":binding,"control":spec.physical_control,"label":label,"available":action.is_some()})
        }).collect();
        json!({"document_id":self.draft.document_id,"document_revision":self.draft.revision,
            "host_state_revision":self.host_state_revision,"profile_id":p.host_active_profile_id,
            "locked_profile_id":locked,
            "profile_name":profile.map(|p| &p.name),"execution_enabled":p.host_execution_enabled,
            "command_execution_enabled":p.command_execution_enabled,
            "auto_follow_enabled":p.auto_follow_enabled,"manual_override":p.manual_override,
            "bottom_row_layout":layout,
            "popover_preview_mode":p.popover_preview_mode,
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
                if next.locked_profile_id.is_some() {
                    next.locked_profile_id = Some(profile_id.clone());
                }
                next.host_active_profile_id = Some(profile_id);
                next.manual_override = true;
            }
            RoutingOperation::LockProfile { profile_id } => match profile_id {
                Some(pid) => {
                    if !valid(&pid) {
                        return Err(format!("Profile 不存在: {pid}"));
                    }
                    next.locked_profile_id = Some(pid.clone());
                    next.host_active_profile_id = Some(pid);
                    next.manual_override = true;
                }
                None => {
                    next.locked_profile_id = None;
                    next.manual_override = !next.auto_follow_enabled;
                }
            },
            RoutingOperation::Enable { enabled } => next.auto_follow_enabled = enabled,
            RoutingOperation::Execution { enabled } => next.host_execution_enabled = enabled,
            RoutingOperation::CommandExecution { enabled } => {
                next.command_execution_enabled = enabled
            }
            RoutingOperation::Resume => {
                next.locked_profile_id = None;
                next.auto_follow_enabled = true;
                next.manual_override = false;
            }
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
                if next.locked_profile_id.is_none()
                    && next.auto_follow_enabled
                    && !next.manual_override
                {
                    if let Some(id) = next
                        .app_profile_links
                        .get(&bundle_id)
                        .or_else(|| next.app_profile_links.get("*"))
                        .filter(|id| valid(id))
                    {
                        next.host_active_profile_id = Some(id.clone());
                    }
                }
            }
            RoutingOperation::BottomRowLayout { layout } => {
                next.bottom_row_layout = layout;
            }
            RoutingOperation::PopoverPreviewMode { mode } => {
                next.popover_preview_mode = mode;
            }
        }
        if next != self.preferences.value {
            self.preferences.save(next)?;
            self.host_state_revision += 1;
        }
        Ok(())
    }

    pub fn apply_routing_operation(&mut self, operation: RoutingOperation) -> Result<(), String> {
        if let Some(host) = &mut self.host {
            return host.apply_routing_operation(operation);
        }
        let request = RoutingRequest {
            document_id: self.draft.document_id.clone(),
            document_revision: self.draft.revision,
            host_state_revision: self.host_state_revision,
            operation,
        };
        self.update_routing(request)
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
    fn cheat_sheet_hud_displays_all_6_agent_presets_dial_joystick_and_redacts_custom_text() {
        let mut store = DocumentStore::default();
        let ids = crate::preset::install_agent_starter_profiles(&mut store.draft).unwrap();
        assert_eq!(ids.len(), 6);

        for pid in &ids {
            let sel = request(&store, json!({"kind":"select","profile_id":pid}));
            store.update_routing(sel).unwrap();
            let state = store.routing_state();
            let profile_name = state["profile_name"].as_str().unwrap().to_owned();
            let keys = state["keys"].as_array().unwrap();
            assert_eq!(
                keys.len(),
                20,
                "{profile_name} cheat sheet must expose 20 verified controls in default split mode"
            );

            let by_id = |b: &str| -> &Value {
                keys.iter()
                    .find(|k| k["binding_id"] == b)
                    .unwrap_or_else(|| panic!("missing {b} in {profile_name}"))
            };

            if matches!(profile_name.as_str(), "Codex") {
                assert_eq!(by_id("AG00")["available"], false);
                assert_eq!(by_id("AG00")["label"], "不可用 / 未配置");
                continue;
            }
            if profile_name == "其他软件" {
                assert_eq!(by_id("AG00")["available"], false);
                assert_eq!(by_id("AG00")["label"], "不可用 / 未配置");
                assert_eq!(by_id("AG11")["available"], true);
                assert_eq!(by_id("AG11")["label"], "苹果 Fn 键（语音输入） (fn)");
                continue;
            }
            if profile_name == crate::preset::LOCAL_CODEX_PROFILE_NAME {
                for slot in 0..6 {
                    let binding = format!("AG{slot:02}");
                    assert_eq!(by_id(&binding)["available"], true);
                    assert_eq!(by_id(&binding)["label"], format!("会话 {}", slot + 1));
                }
                assert_eq!(by_id("AG06")["label"], "Gemini Live 实时语音");
                assert_eq!(by_id("AG07")["label"], "聚焦输入框");
                assert_eq!(by_id("AG08")["label"], "新建标签页");
                assert_eq!(by_id("AG09")["label"], "内置浏览器 · 布局");
                assert_eq!(by_id("AG10")["label"], "新建会话");
                assert_eq!(by_id("AG11")["label"], "点击听写");
                assert_eq!(by_id("AG12")["label"], "停止听写并提交");
                assert_eq!(by_id("AG13")["label"], "向上滚动对话");
                assert_eq!(by_id("AG14")["label"], "向下滚动对话");
                assert_eq!(by_id("ENC_CLK")["label"], "跳到底部并聚焦输入");
                continue;
            }

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
            } else if profile_name == "Antigravity" {
                assert_eq!(by_id("AG01")["label"], "聚焦输入框");
                assert_eq!(by_id("AG02")["label"], "新建会话");
                assert_eq!(by_id("AG03")["label"], "选项目 / 新建项目");
                assert_eq!(by_id("AG04")["label"], "选择模型");
                assert_eq!(by_id("AG05")["label"], "项目工作区快切 (command+shift K)");
                assert_eq!(by_id("AG06")["label"], "Gemini Live 实时语音");
                assert_eq!(by_id("AG07")["label"], "聚焦输入框");
                assert_eq!(by_id("AG08")["label"], "打开终端面板");
                assert_eq!(by_id("AG09")["label"], "快速向右分栏");
                assert_eq!(by_id("AG10")["label"], "在项目中新建会话");
                assert_eq!(by_id("AG11")["label"], "按住说话");
                assert_eq!(by_id("AG12")["label"], "停止听写并提交");
                assert_eq!(by_id("AG13")["label"], "上一会话（侧栏轮转）");
                assert_eq!(by_id("AG14")["label"], "下一会话（侧栏轮转）");
                assert_eq!(by_id("ENC_CLK")["label"], "搜索会话");
                assert_eq!(by_id("AG15")["label"], "向上滚动对话");
                assert_eq!(by_id("AG16")["label"], "历史后退 (command [)");
                assert_eq!(by_id("AG17")["label"], "向下滚动对话");
                assert_eq!(by_id("AG18")["label"], "历史前进 (command ])");
            } else {
                assert_eq!(by_id("AG01")["label"], "聚焦输入框");
                assert_eq!(by_id("AG13")["label"], "向上滚动对话");
                assert_eq!(by_id("AG14")["label"], "向下滚动对话");
                assert_eq!(by_id("ENC_CLK")["label"], "跳到底部并聚焦输入");
            }
        }

        // Verify built-in prompt templates, navigation, submit, and custom text payload redaction
        let ag_pid = ids
            .iter()
            .find(|pid| store.draft.document.editable.graph.profiles[*pid].name == "Antigravity")
            .unwrap();
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
                find_cat("navigate-forward"),
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
        assert_eq!(label_of("AG17"), "前进到下一个打开目标");
        assert_eq!(label_of("AG12"), "停止听写并提交");
        assert_eq!(label_of("AG09"), "粘贴文本");

        store
            .draft
            .assign(
                &json!(ag_pid),
                &json!(lid),
                1,
                find_cat("codex-effort-down"),
            )
            .unwrap();
        store
            .draft
            .assign(&json!(ag_pid), &json!(lid), 2, find_cat("codex-effort-up"))
            .unwrap();
        store
            .draft
            .assign(
                &json!(ag_pid),
                &json!(lid),
                3,
                find_cat("codex-toggle-fast-mode"),
            )
            .unwrap();
        let state = store.routing_state();
        let keys = state["keys"].as_array().unwrap();
        let label_of = |b: &str| {
            keys.iter().find(|k| k["binding_id"] == b).unwrap()["label"]
                .as_str()
                .unwrap()
        };
        assert_eq!(label_of("AG01"), "降低推理档位");
        assert_eq!(label_of("AG02"), "提高推理档位");
        assert_eq!(label_of("AG03"), "切换 Fast 模式");
    }

    #[test]
    fn popover_preview_mode_backcompat_migration_and_atomic_persistence() {
        use crate::host_router::PopoverPreviewMode;
        let dir = std::env::temp_dir().join(format!("mi-popover-pref-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let v1_missing_path = dir.join("v1-missing.json");
        std::fs::write(
            v1_missing_path.with_extension("preferences.json"),
            r#"{"schema_version":1,"favorites":[],"host_execution_enabled":false}"#,
        )
        .unwrap();
        let mut store_v1_missing = DocumentStore::default();
        store_v1_missing.open(v1_missing_path).unwrap();
        assert!(store_v1_missing.favorite_actions().is_ok());
        assert_eq!(
            store_v1_missing.popover_preview_mode(),
            PopoverPreviewMode::SessionPreview
        );
        assert_eq!(
            store_v1_missing.routing_state()["popover_preview_mode"],
            "session_preview"
        );

        let v2_missing_path = dir.join("v2-missing.json");
        std::fs::write(
            v2_missing_path.with_extension("preferences.json"),
            r#"{"schema_version":2,"favorites":[],"usb_startup_profile_id":null,"board_layer_plan":null,"preview_selection":"UsbStartup","legacy_three_bindings":null}"#,
        )
        .unwrap();
        let mut store_v2_missing = DocumentStore::default();
        store_v2_missing.open(v2_missing_path.clone()).unwrap();
        assert!(store_v2_missing.favorite_actions().is_ok());
        assert_eq!(
            store_v2_missing.popover_preview_mode(),
            PopoverPreviewMode::SessionPreview
        );

        let v1_cheat_path = dir.join("v1-cheat.json");
        std::fs::write(
            v1_cheat_path.with_extension("preferences.json"),
            r#"{"schema_version":1,"favorites":[],"popover_preview_mode":"cheat_sheet"}"#,
        )
        .unwrap();
        let mut store_v1_cheat = DocumentStore::default();
        store_v1_cheat.open(v1_cheat_path.clone()).unwrap();
        assert_eq!(
            store_v1_cheat.popover_preview_mode(),
            PopoverPreviewMode::CheatSheet
        );
        store_v1_cheat
            .save_board_configuration(Default::default(), Default::default(), None)
            .unwrap();
        let mut reopened_migrated = DocumentStore::default();
        reopened_migrated.open(v1_cheat_path).unwrap();
        assert_eq!(
            reopened_migrated.popover_preview_mode(),
            PopoverPreviewMode::CheatSheet
        );

        let prefs_path = v2_missing_path.with_extension("preferences.json");
        let lock_path = prefs_path.with_extension("lock");
        let pending_path = prefs_path.with_extension("pending");

        std::fs::write(&pending_path, b"stale").unwrap();
        let blocked_req = request(
            &store_v2_missing,
            json!({"kind":"popover_preview_mode","mode":"cheat_sheet"}),
        );
        assert!(store_v2_missing.update_routing(blocked_req).is_err());
        assert_eq!(
            store_v2_missing.popover_preview_mode(),
            PopoverPreviewMode::SessionPreview
        );
        std::fs::remove_file(&pending_path).unwrap();

        std::fs::write(&lock_path, b"locked").unwrap();
        let locked_req = request(
            &store_v2_missing,
            json!({"kind":"popover_preview_mode","mode":"cheat_sheet"}),
        );
        assert!(store_v2_missing.update_routing(locked_req).is_err());
        std::fs::remove_file(&lock_path).unwrap();

        let rev_before = store_v2_missing.host_revision();
        let ok_req = request(
            &store_v2_missing,
            json!({"kind":"popover_preview_mode","mode":"cheat_sheet"}),
        );
        store_v2_missing.update_routing(ok_req).unwrap();
        assert_eq!(store_v2_missing.host_revision(), rev_before + 1);
        assert_eq!(
            store_v2_missing.popover_preview_mode(),
            PopoverPreviewMode::CheatSheet
        );
        assert_eq!(
            store_v2_missing.routing_state()["popover_preview_mode"],
            "cheat_sheet"
        );
        assert!(!lock_path.exists());
        assert!(!pending_path.exists());

        let mut reopened = DocumentStore::default();
        reopened.open(v2_missing_path).unwrap();
        assert_eq!(
            reopened.popover_preview_mode(),
            PopoverPreviewMode::CheatSheet
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn profile_lock_blocks_follow_switches_on_select_unlocks_on_resume_and_clears_on_delete() {
        let dir = std::env::temp_dir().join(format!("mi-lock-pref-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let doc_path = dir.join("workbench.json");
        std::fs::write(
            doc_path.with_extension("preferences.json"),
            r#"{"schema_version":2,"favorites":[],"usb_startup_profile_id":null,"board_layer_plan":null,"preview_selection":"UsbStartup","legacy_three_bindings":null}"#,
        )
        .unwrap();

        let mut store = DocumentStore::default();
        store.open(doc_path.clone()).unwrap();
        assert_eq!(store.locked_profile_id(), None);
        assert_eq!(store.routing_state()["locked_profile_id"], Value::Null);

        let mut ids = Vec::new();
        store
            .update(|draft| {
                ids = crate::preset::install_agent_starter_profiles(draft)?;
                Ok(())
            })
            .unwrap();
        let ag_id = ids
            .iter()
            .find(|id| store.draft.document.editable.graph.profiles[*id].name == "Antigravity")
            .unwrap()
            .clone();
        let herdr_id = ids
            .iter()
            .find(|id| store.draft.document.editable.graph.profiles[*id].name == "Herdr")
            .unwrap()
            .clone();
        let claude_id = ids
            .iter()
            .find(|id| *id != &ag_id && *id != &herdr_id)
            .unwrap()
            .clone();

        for (bundle, pid) in [
            ("com.google.antigravity", ag_id.as_str()),
            ("com.anthropic.claudefordesktop", claude_id.as_str()),
        ] {
            let upsert = request(
                &store,
                json!({"kind":"upsert","bundle_id":bundle,"profile_id":pid}),
            );
            store.update_routing(upsert).unwrap();
        }
        store
            .update_routing(request(&store, json!({"kind":"enable","enabled":true})))
            .unwrap();

        assert!(store
            .update_routing(request(
                &store,
                json!({"kind":"lock_profile","profile_id":"missing-profile-id"})
            ))
            .is_err());

        store
            .update_routing(request(
                &store,
                json!({"kind":"lock_profile","profile_id":ag_id}),
            ))
            .unwrap();
        assert_eq!(store.locked_profile_id(), Some(ag_id.clone()));
        assert_eq!(store.host_active_profile_id(), Some(ag_id.clone()));
        assert_eq!(store.routing_state()["locked_profile_id"], ag_id);
        assert_eq!(store.routing_state()["manual_override"], true);

        let rev_before_follow = store.host_revision();
        store
            .update_routing(request(
                &store,
                json!({"kind":"follow","bundle_id":"com.anthropic.claudefordesktop"}),
            ))
            .unwrap();
        assert_eq!(store.host_revision(), rev_before_follow);
        assert_eq!(store.locked_profile_id(), Some(ag_id.clone()));
        assert_eq!(store.host_active_profile_id(), Some(ag_id.clone()));
        assert_eq!(store.routing_state()["manual_override"], true);

        store
            .update_routing(request(
                &store,
                json!({"kind":"select","profile_id":herdr_id}),
            ))
            .unwrap();
        assert_eq!(store.locked_profile_id(), Some(herdr_id.clone()));
        assert_eq!(store.host_active_profile_id(), Some(herdr_id.clone()));

        store
            .update_routing(request(&store, json!({"kind":"enable","enabled":false})))
            .unwrap();
        assert_eq!(store.locked_profile_id(), Some(herdr_id.clone()));

        store
            .update_routing(request(
                &store,
                json!({"kind":"lock_profile","profile_id":null}),
            ))
            .unwrap();
        assert_eq!(store.locked_profile_id(), None);
        assert_eq!(store.host_active_profile_id(), Some(herdr_id.clone()));
        assert_eq!(store.routing_state()["manual_override"], true);

        store
            .update_routing(request(
                &store,
                json!({"kind":"lock_profile","profile_id":claude_id}),
            ))
            .unwrap();
        store
            .update_routing(request(&store, json!({"kind":"resume"})))
            .unwrap();
        assert_eq!(store.locked_profile_id(), None);
        assert_eq!(store.routing_state()["auto_follow_enabled"], true);
        assert_eq!(store.routing_state()["manual_override"], false);

        store
            .update_routing(request(
                &store,
                json!({"kind":"lock_profile","profile_id":herdr_id}),
            ))
            .unwrap();
        assert_eq!(store.locked_profile_id(), Some(herdr_id.clone()));
        store
            .update(|draft| draft.delete_profile(&herdr_id))
            .unwrap();
        assert_eq!(store.locked_profile_id(), None);
        assert_eq!(store.routing_state()["locked_profile_id"], Value::Null);

        let mut reopened = DocumentStore::default();
        reopened.open(doc_path).unwrap();
        assert_eq!(reopened.locked_profile_id(), None);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
