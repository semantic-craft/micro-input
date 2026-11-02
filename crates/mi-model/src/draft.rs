//! R-E07/R-E08/R-K05: versioned local intents; never firmware payloads.
use crate::DeviceConfig;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActionKind {
    LocalMulti {
        multi: crate::multi::LocalMulti,
    },
    LocalMacro {
        macro_sequence: crate::macro_sequence::LocalMacro,
    },
    Shortcut {
        key: String,
        modifiers: Vec<String>,
        target: String,
    },
    Text {
        text: String,
    },
    OpenUrl {
        url: String,
    },
    OpenApp {
        bundle_id: String,
    },
    AgentCommand {
        adapter: String,
        command: String,
    },
    FocusSession {
        adapter: String,
        session: String,
    },
    SkillReference {
        adapter: String,
        skill: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Action {
    pub id: String,
    pub name: String,
    pub category: String,
    pub detail: String,
    // Raw payload preserves future action types. Unsupported payloads lock editing.
    pub payload: Value,
}
impl Action {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() || self.name.len() > 256 {
            return Err("动作名称应为 1–256 字节".into());
        }
        let kind: ActionKind =
            serde_json::from_value(self.payload.clone()).map_err(|_| "动作参数未知或格式不正确")?;
        let bounded = |s: &str| !s.trim().is_empty() && s.len() <= 16384;
        let valid = match kind {
            ActionKind::LocalMacro { macro_sequence } => return macro_sequence.validate(),
            ActionKind::LocalMulti { multi } => return multi.validate(),
            ActionKind::Shortcut {
                key,
                modifiers,
                target,
            } => {
                bounded(&key)
                    && bounded(&target)
                    && modifiers
                        .iter()
                        .all(|m| ["command", "shift", "option", "control"].contains(&m.as_str()))
            }
            ActionKind::Text { text } => bounded(&text),
            ActionKind::OpenUrl { url } => {
                url.len() <= 16384
                    && !url.chars().any(char::is_whitespace)
                    && url::Url::parse(&url).is_ok_and(|u| {
                        matches!(u.scheme(), "http" | "https") && u.host_str().is_some()
                    })
            }
            ActionKind::OpenApp { bundle_id } => {
                bundle_id.len() <= 256
                    && bundle_id.split('.').count() >= 2
                    && bundle_id.split('.').all(|part| {
                        !part.is_empty()
                            && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                    })
            }
            ActionKind::AgentCommand { adapter, command } => bounded(&adapter) && bounded(&command),
            ActionKind::FocusSession { adapter, session } => bounded(&adapter) && bounded(&session),
            ActionKind::SkillReference { adapter, skill } => bounded(&adapter) && bounded(&skill),
        };
        if valid {
            Ok(())
        } else {
            Err("动作参数为空、过长或不合法".into())
        }
    }
}
pub fn actions() -> Vec<Action> {
    use serde_json::json;
    [
        ("search", "搜索文件", "快捷键", "需要前台编辑器 · 仅草稿", json!({"type":"shortcut","key":"p","modifiers":["command"],"target":"frontmost-editor"})),
        ("save", "保存文件", "快捷键", "需要前台编辑器 · 仅草稿", json!({"type":"shortcut","key":"s","modifiers":["command"],"target":"frontmost-editor"})),
        ("review", "插入审查提示", "文本", "仅保存文本，不执行", json!({"type":"text","text":"请检查当前变更，优先报告可复现的问题。"})),
        ("url", "打开网址", "URL", "仅保存网址，不打开", json!({"type":"open_url","url":"https://example.com"})),
        ("pane", "下一个 pane", "Herdr", "适配器未验证 · 不执行", json!({"type":"agent_command","adapter":"herdr","command":"next-pane"})),
        ("session", "切换会话", "DSH", "适配器未验证 · 不执行", json!({"type":"focus_session","adapter":"dsh","session":"unselected"})),
        ("skill", "Skill 引用", "Skill", "适配器未验证 · 不执行", json!({"type":"skill_reference","adapter":"unverified","skill":"unselected"})),
        ("app", "打开应用", "应用", "本地应用意图 · 宿主执行未接入", json!({"type":"open_app","bundle_id":"com.apple.TextEdit"})),
        ("local-multi", "Multi 动作", "Multi", "本地四分支 · 不执行 · 固件范围未知", json!({"type":"local_multi","multi":{"format_version":1,"term_ms":250,"tap":null,"double_tap":null,"hold":null,"tap_hold":null}})),
        ("local-macro", "普通宏", "普通宏", "自有本地格式 · 仅草稿 · 不执行", json!({"type":"local_macro","macro_sequence":{"format_version":1,"events":[{"type":"press","key":"a"},{"type":"release","key":"a"}]}})),
    ].into_iter().map(|(id,name,category,detail,payload)| Action {id:id.into(),name:name.into(),category:category.into(),detail:detail.into(),payload}).collect()
}

/// Document-scoped identity: source IDs, not array offsets or display names.
/// Logical positions remain schematic until UNKNOWN-10 is resolved.
pub fn binding_key(profile: &Value, layer: &Value, slot: u32) -> String {
    serde_json::to_string(&(profile, layer, format!("logical-{slot}"))).unwrap()
}
#[derive(Clone, Serialize, Deserialize)]
pub struct LegacyDocumentV1 {
    pub schema_version: u32,
    pub config: DeviceConfig,
    pub edits: BTreeMap<String, Action>,
    #[serde(skip)]
    undo: Vec<BTreeMap<String, Action>>,
    #[serde(skip)]
    redo: Vec<BTreeMap<String, Action>>,
    pub revision: u64,
    #[serde(skip_deserializing)]
    pub can_undo: bool,
    #[serde(skip_deserializing)]
    pub can_redo: bool,
    #[serde(skip_deserializing)]
    pub read_only: bool,
}
impl Default for LegacyDocumentV1 {
    fn default() -> Self {
        Self::new(crate::demo().expect("valid bundled fixture")).unwrap()
    }
}
impl LegacyDocumentV1 {
    pub fn new(config: DeviceConfig) -> Result<Self, String> {
        let d = Self {
            schema_version: 1,
            config,
            edits: BTreeMap::new(),
            undo: vec![],
            redo: vec![],
            revision: 0,
            can_undo: false,
            can_redo: false,
            read_only: false,
        };
        d.validate()?;
        Ok(d)
    }
    fn keys(&self) -> Result<BTreeSet<String>, String> {
        let mut profiles = BTreeSet::new();
        let mut keys = BTreeSet::new();
        for p in &self.config.profiles {
            if !(p.id.is_string() || p.id.is_i64() || p.id.is_u64())
                || !profiles.insert(p.id.to_string())
            {
                return Err("配置 ID 缺失或重复".into());
            }
            let mut layers = BTreeSet::new();
            for l in &p.layers {
                if !(l.id.is_string() || l.id.is_i64() || l.id.is_u64())
                    || !layers.insert(l.id.to_string())
                {
                    return Err("层 ID 缺失或重复".into());
                }
                for slot in 0..16 + l.joystick.sectors.len() {
                    keys.insert(binding_key(&p.id, &l.id, slot as u32));
                }
            }
        }
        Ok(keys)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("本地文档版本不支持；原文件保留".into());
        }
        let keys = self.keys()?;
        if self.edits.keys().any(|k| !keys.contains(k)) {
            return Err("草稿包含不存在的控件引用；原文件保留".into());
        }
        Ok(())
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let raw: Value = serde_json::from_slice(bytes).map_err(|_| "本地文档损坏；原文件保留")?;
        if raw["schema_version"] != 1 {
            return Err("本地文档版本不支持；原文件保留".into());
        }
        let mut d: Self =
            serde_json::from_value(raw.clone()).map_err(|_| "本地文档结构不合法；原文件保留")?;
        d.validate()?;
        let mut normalized = serde_json::to_value(&d).map_err(|e| e.to_string())?;
        let mut original = raw.clone();
        for key in ["can_undo", "can_redo", "read_only"] {
            normalized.as_object_mut().unwrap().remove(key);
            original.as_object_mut().unwrap().remove(key);
        }
        let future_fields = normalized != original;
        // Future fields/types must not be silently discarded by a subsequent save.
        d.read_only = future_fields
            || d.edits.values().any(|a| a.validate().is_err())
            || raw.as_object().unwrap().keys().any(|k| {
                ![
                    "schema_version",
                    "config",
                    "edits",
                    "revision",
                    "can_undo",
                    "can_redo",
                    "read_only",
                ]
                .contains(&k.as_str())
            });
        Ok(d)
    }
    pub fn assign(
        &mut self,
        profile: &Value,
        layer: &Value,
        slot: u32,
        action: Action,
    ) -> Result<(), String> {
        if self.read_only {
            return Err("文档含未知动作或字段，仅可浏览；原文件保留".into());
        }
        action.validate()?;
        let key = binding_key(profile, layer, slot);
        if !self.keys()?.contains(&key) {
            return Err("无效配置、层或控件".into());
        }
        if self.edits.get(&key) == Some(&action) {
            return Ok(());
        }
        self.undo.push(self.edits.clone());
        self.redo.clear();
        self.edits.insert(key, action);
        self.changed();
        Ok(())
    }
    pub fn undo(&mut self, redo: bool) -> Result<(), String> {
        if self.read_only {
            return Err("文档仅可浏览".into());
        }
        if redo {
            if let Some(next) = self.redo.pop() {
                self.undo.push(std::mem::replace(&mut self.edits, next));
                self.changed();
            }
        } else if let Some(old) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.edits, old));
            self.changed();
        }
        Ok(())
    }
    fn changed(&mut self) {
        self.revision += 1;
        self.can_undo = !self.undo.is_empty();
        self.can_redo = !self.redo.is_empty();
    }
}

/// Runtime projection plus one authoritative v2 document. History is session-local.
#[derive(Clone, Serialize)]
pub struct Draft {
    pub config: DeviceConfig,
    pub edits: BTreeMap<String, Action>,
    pub revision: u64,
    pub can_undo: bool,
    pub can_redo: bool,
    pub read_only: bool,
    pub loaded_format: u32,
    pub document_id: String,
    pub structure_changes: Vec<String>,
    #[serde(skip)]
    pub document: crate::document::LocalDocumentV2,
    #[serde(skip)]
    undo: Vec<crate::document::EditableState>,
    #[serde(skip)]
    redo: Vec<crate::document::EditableState>,
}
impl Default for Draft {
    fn default() -> Self {
        Self::new(crate::demo().expect("valid fixture")).unwrap()
    }
}
impl Draft {
    pub fn new(config: DeviceConfig) -> Result<Self, String> {
        let doc = crate::document::LocalDocumentV2::from_config(&config, None, "synthetic-local")?;
        Self::from_document(doc, 2, false)
    }
    pub fn from_document(
        document: crate::document::LocalDocumentV2,
        loaded_format: u32,
        read_only: bool,
    ) -> Result<Self, String> {
        let (config, edits) = document.view()?;
        Ok(Self {
            structure_changes: document.structure_changes(),
            revision: document.revision,
            document_id: document.document_id.clone(),
            config,
            edits,
            can_undo: false,
            can_redo: false,
            read_only,
            loaded_format,
            document,
            undo: vec![],
            redo: vec![],
        })
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        use sha2::{Digest, Sha256};
        let raw: Value = serde_json::from_slice(bytes).map_err(|_| "本地文档损坏；原文件保留")?;
        match raw["schema_version"].as_u64() {
            Some(1) => {
                let legacy = LegacyDocumentV1::from_bytes(bytes)?;
                let doc = crate::document::LocalDocumentV2::from_legacy(
                    &legacy,
                    Some(format!("{:x}", Sha256::digest(bytes))),
                )?;
                Self::from_document(doc, 1, legacy.read_only)
            }
            Some(2) => {
                let doc: crate::document::LocalDocumentV2 = serde_json::from_value(raw.clone())
                    .map_err(|_| "本地文档结构不合法；原文件保留")?;
                doc.validate()?;
                let read_only = serde_json::to_value(&doc).map_err(|e| e.to_string())? != raw
                    || !doc.actions_known();
                Self::from_document(doc, 2, read_only)
            }
            _ => Err("本地文档版本不支持；原文件保留".into()),
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        self.document.validate()
    }
    pub fn mutate<T>(
        &mut self,
        edit: impl FnOnce(&mut crate::document::LocalDocumentV2) -> Result<T, String>,
    ) -> Result<T, String> {
        if self.read_only {
            return Err("文档含未知动作或字段，仅可浏览；原文件保留".into());
        }
        let mut next = self.document.clone();
        let result = edit(&mut next)?;
        next.validate()?;
        if next.editable == self.document.editable {
            return Ok(result);
        }
        next.revision = self.revision.checked_add(1).ok_or("文档修订号已达上限")?;
        let (config, edits) = next.view()?;
        self.undo.push(self.document.editable.clone());
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.document = next;
        self.config = config;
        self.edits = edits;
        self.refresh();
        Ok(result)
    }
    fn refresh(&mut self) {
        self.structure_changes = self.document.structure_changes();
        self.revision = self.document.revision;
        self.can_undo = !self.undo.is_empty();
        self.can_redo = !self.redo.is_empty();
    }
    pub fn create_profile(&mut self, name: &str) -> Result<String, String> {
        self.mutate(|d| d.create_profile(name))
    }
    pub fn create_layer(&mut self, pid: &str, name: &str) -> Result<String, String> {
        self.mutate(|d| d.create_layer(pid, name))
    }
    pub fn duplicate_profile(&mut self, pid: &str, name: &str) -> Result<String, String> {
        self.mutate(|d| d.duplicate_profile(pid, name))
    }
    pub fn duplicate_layer(&mut self, pid: &str, lid: &str, name: &str) -> Result<String, String> {
        self.mutate(|d| d.duplicate_layer(pid, lid, name))
    }
    pub fn rename_profile(&mut self, id: &str, name: &str) -> Result<(), String> {
        self.mutate(|d| d.rename_profile(id, name))
    }
    pub fn rename_layer(&mut self, id: &str, name: &str) -> Result<(), String> {
        self.mutate(|d| d.rename_layer(id, name))
    }
    pub fn move_profile(&mut self, id: &str, index: usize) -> Result<(), String> {
        self.mutate(|d| d.move_profile(id, index))
    }
    pub fn move_layer(&mut self, pid: &str, id: &str, index: usize) -> Result<(), String> {
        self.mutate(|d| d.move_layer(pid, id, index))
    }
    pub fn delete_profile(&mut self, id: &str) -> Result<(), String> {
        self.mutate(|d| d.delete_profile(id))
    }
    pub fn delete_layer(&mut self, pid: &str, id: &str) -> Result<(), String> {
        self.mutate(|d| d.delete_layer(pid, id))
    }
    pub fn assign(
        &mut self,
        profile: &Value,
        layer: &Value,
        slot: u32,
        action: Action,
    ) -> Result<(), String> {
        action.validate()?;
        let pid = profile.as_str().ok_or("配置必须是本地 UUID")?;
        let lid = layer.as_str().ok_or("层必须是本地 UUID")?;
        self.mutate(|d| {
            let p = d.editable.graph.profiles.get(pid).ok_or("配置不存在")?;
            if !p.layer_order.iter().any(|id| id == lid) {
                return Err("层不属于所选配置".into());
            }
            let l = d.editable.graph.layers.get_mut(lid).ok_or("层不存在")?;
            let binding = l
                .controls
                .values_mut()
                .find(|b| b.logical_control == format!("logical-{slot}"))
                .ok_or("控件不存在")?;
            if binding
                .action_id
                .as_ref()
                .and_then(|id| d.editable.graph.actions.get(id))
                == Some(&action)
            {
                return Ok(());
            }
            let old = binding.action_id.replace(crate::document::new_id());
            d.editable
                .graph
                .actions
                .insert(binding.action_id.clone().unwrap(), action);
            if let Some(old) = old {
                if !d.editable.graph.actions.get(&old).is_some_and(|a| {
                    crate::macro_library::is_local_macro(a) || crate::multi::is_local_multi(a)
                }) && crate::multi::parent_references(d, &old).is_empty()
                    && !d.editable.graph.layers.values().any(|l| {
                        l.controls
                            .values()
                            .any(|b| b.action_id.as_ref() == Some(&old))
                    })
                {
                    d.editable.graph.actions.remove(&old);
                    d.editable.source_refs.remove(&old);
                }
            }
            Ok(())
        })
    }
    pub fn undo(&mut self, redo: bool) -> Result<(), String> {
        if self.read_only {
            return Err("文档仅可浏览".into());
        }
        let stack = if redo { &self.redo } else { &self.undo };
        let Some(state) = stack.last().cloned() else {
            return Ok(());
        };
        let mut next = self.document.clone();
        next.editable = state;
        next.revision = self.revision.checked_add(1).ok_or("文档修订号已达上限")?;
        next.validate()?;
        let (config, edits) = next.view()?;
        if redo {
            self.redo.pop();
            self.undo.push(self.document.editable.clone());
        } else {
            self.undo.pop();
            self.redo.push(self.document.editable.clone());
        }
        self.document = next;
        self.config = config;
        self.edits = edits;
        self.refresh();
        Ok(())
    }
}
