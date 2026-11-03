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
        self.host_state_revision
    }
    pub fn routing_state(&self) -> Value {
        let p = &self.preferences.value;
        let graph = &self.draft.document.editable.graph;
        let profile = p
            .host_active_profile_id
            .as_ref()
            .and_then(|id| graph.profiles.get(id));
        let bindings = [
            "AG01", "AG00", "AG02", "AG03", "AG04", "AG05", "AG06", "AG07", "AG08", "AG09", "AG10",
            "AG12", "AG13", "AG14", "ENC_CLK", "AG15", "AG16", "AG17", "AG18",
        ];
        let keys: Vec<Value> = bindings.iter().map(|binding| {
            use crate::host_router::ResolvedHostAction::*;
            let action = crate::host_router::resolve_host_action(&self.draft.document, p.host_active_profile_id.as_deref(), binding, true);
            let label = match &action {
                Some(Shortcut{key,modifiers,..}) => format!("{} {}", modifiers.join("+"), key),
                Some(PasteText{..}) => "粘贴文本".into(),
                Some(OpenApp{bundle_id}) => format!("打开 {}", bundle_id),
                Some(OpenUrl{..}) => "打开网址".into(),
                Some(AgentCommand{command,..}) => match command.as_str() {
                    "focus-input" => "聚焦输入框", "search" => "搜索会话", "new-chat" => "新建会话",
                    "model" => "选择模型", "effort" => "思考力度", "dictation" => "听写 / 语音输入",
                    "browser" => "打开内置浏览器", "terminal" => "打开终端面板", _ => "Agent 动作",
                }.into(),
                Some(FocusSession{..}) => "聚焦会话".into(),
                None => "不可用 / 未配置".into(),
            };
            let spec = crate::host_router::verified_physical_mapping(binding).unwrap();
            json!({"binding_id":binding,"control":spec.physical_control,"label":label,"available":action.is_some()})
        }).collect();
        json!({"document_id":self.draft.document_id,"document_revision":self.draft.revision,
            "host_state_revision":self.host_state_revision,"profile_id":p.host_active_profile_id,
            "profile_name":profile.map(|p| &p.name),"execution_enabled":p.host_execution_enabled,
            "auto_follow_enabled":p.auto_follow_enabled,"manual_override":p.manual_override,
            "app_profile_links":p.app_profile_links,"keys":keys})
    }
    pub fn update_routing(&mut self, request: RoutingRequest) -> Result<(), String> {
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
}
