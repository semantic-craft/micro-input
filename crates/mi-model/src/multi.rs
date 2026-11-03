//! Local four-branch intent. This format, term and references never become a device payload.
use crate::{
    document::{new_id, Graph, LocalDocumentV2},
    draft::{Action, ActionKind, Draft},
    macro_library::{binding, is_local_macro, references, MacroReference, MacroSummary},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LocalMulti {
    pub format_version: u32,
    pub term_ms: u32,
    pub tap: Option<ActionReference>,
    pub double_tap: Option<ActionReference>,
    pub hold: Option<ActionReference>,
    pub tap_hold: Option<ActionReference>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActionReference {
    Macro { action_id: String },
    Action { action_id: String },
}
impl ActionReference {
    pub fn id(&self) -> &str {
        match self {
            Self::Macro { action_id } | Self::Action { action_id } => action_id,
        }
    }
}
impl LocalMulti {
    pub fn branches(&self) -> [(&str, Option<&ActionReference>); 4] {
        [
            ("单击", self.tap.as_ref()),
            ("双击", self.double_tap.as_ref()),
            ("长按", self.hold.as_ref()),
            ("点击后长按", self.tap_hold.as_ref()),
        ]
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.format_version != 1 || self.term_ms == 0 {
            return Err("本地 Multi 版本不支持，或本地判定时长不是正整数毫秒；固件范围未知".into());
        }
        for (_, r) in self.branches() {
            if let Some(r) = r {
                uuid::Uuid::parse_str(r.id()).map_err(|_| "Multi 分支必须引用文档内动作 UUID")?;
            }
        }
        Ok(())
    }
}
pub fn is_local_multi(a: &Action) -> bool {
    a.payload["type"] == "local_multi"
}
pub fn local_multi(a: &Action) -> Option<LocalMulti> {
    match serde_json::from_value(a.payload.clone()).ok()? {
        ActionKind::LocalMulti { multi } if multi.format_version == 1 => Some(multi),
        _ => None,
    }
}
pub fn parent_references(d: &LocalDocumentV2, target: &str) -> Vec<MacroReference> {
    d.editable
        .graph
        .actions
        .values()
        .flat_map(|a| {
            local_multi(a).into_iter().flat_map(move |m| {
                m.branches()
                    .into_iter()
                    .filter_map(|(branch, r)| {
                        r.filter(|r| r.id() == target).map(|_| MacroReference {
                            profile_id: String::new(),
                            profile_name: String::new(),
                            layer_id: String::new(),
                            layer_name: String::new(),
                            logical_control: String::new(),
                            parent_action_name: Some(a.name.clone()),
                            branch: Some(branch.into()),
                        })
                    })
                    .collect::<Vec<_>>()
            })
        })
        .collect()
}
pub fn validate_references(g: &Graph) -> Result<(), String> {
    fn visit(
        id: &str,
        g: &Graph,
        visiting: &mut BTreeSet<String>,
        done: &mut BTreeSet<String>,
    ) -> Result<(), String> {
        if done.contains(id) {
            return Ok(());
        }
        if !visiting.insert(id.into()) {
            return Err("Multi 分支存在递归引用".into());
        }
        if let Some(m) = local_multi(&g.actions[id]) {
            m.validate()?;
            for (branch, r) in m.branches() {
                if let Some(r) = r {
                    let a = g.actions.get(r.id()).ok_or_else(|| {
                        format!("{} / {branch} 引用的动作不存在", g.actions[id].name)
                    })?;
                    if matches!(r, ActionReference::Macro { .. }) != is_local_macro(a) {
                        return Err(format!(
                            "{} / {branch} 的宏/动作引用类型不匹配",
                            g.actions[id].name
                        ));
                    }
                    visit(r.id(), g, visiting, done)?;
                }
            }
        }
        visiting.remove(id);
        done.insert(id.into());
        Ok(())
    }
    let mut done = BTreeSet::new();
    for id in g.actions.keys() {
        visit(id, g, &mut BTreeSet::new(), &mut done)?;
    }
    Ok(())
}
impl Draft {
    pub fn multis(&self) -> Vec<MacroSummary> {
        self.document
            .editable
            .graph
            .actions
            .iter()
            .filter(|(_, a)| is_local_multi(a))
            .map(|(id, a)| MacroSummary {
                id: id.clone(),
                action: a.clone(),
                references: references(&self.document, id),
            })
            .collect()
    }
    pub fn save_multi_for_binding(
        &mut self,
        id: Option<&str>,
        action: Action,
        additions: BTreeMap<String, Action>,
        profile: &Value,
        layer: &Value,
        slot: u32,
    ) -> Result<String, String> {
        self.save_multi_target(id, action, additions, Some((profile, layer, slot)))
    }
    pub fn save_multi(
        &mut self,
        id: &str,
        action: Action,
        additions: BTreeMap<String, Action>,
    ) -> Result<String, String> {
        self.save_multi_target(Some(id), action, additions, None)
    }
    fn save_multi_target(
        &mut self,
        id: Option<&str>,
        action: Action,
        additions: BTreeMap<String, Action>,
        target: Option<(&Value, &Value, u32)>,
    ) -> Result<String, String> {
        if !is_local_multi(&action) {
            return Err("只能保存本地 Multi".into());
        }
        action.validate()?;
        self.mutate(|d| {
            if let Some(id) = id {
                if !d.editable.graph.actions.get(id).is_some_and(is_local_multi) {
                    return Err("本地 Multi 不存在".into());
                }
            }
            let multi = local_multi(&action).ok_or("Multi 格式无效")?;
            for (aid, a) in additions {
                uuid::Uuid::parse_str(&aid).map_err(|_| "新增动作必须使用 UUID")?;
                if d.editable.graph.actions.contains_key(&aid)
                    || !multi
                        .branches()
                        .iter()
                        .any(|(_, r)| r.is_some_and(|r| r.id() == aid))
                {
                    return Err("新增动作覆盖已有对象或没有分支引用".into());
                }
                // Shared macros are created/edited only through the existing macro library.
                if is_local_macro(&a) || is_local_multi(&a) {
                    return Err("宏与 Multi 必须引用现有共享对象".into());
                }
                a.validate()?;
                d.editable.graph.actions.insert(aid, a);
            }
            let id = id.map(str::to_owned).unwrap_or_else(new_id);
            if let Some((profile, layer, slot)) = target {
                binding(d, profile, layer, slot)?.action_id = Some(id.clone());
            }
            d.editable.graph.actions.insert(id.clone(), action);
            Ok(id)
        })
    }
    pub fn bind_multi(
        &mut self,
        profile: &Value,
        layer: &Value,
        slot: u32,
        id: Option<&str>,
    ) -> Result<(), String> {
        self.mutate(|d| {
            if let Some(id) = id {
                if !d.editable.graph.actions.get(id).is_some_and(is_local_multi) {
                    return Err("本地 Multi 不存在".into());
                }
            } else {
                let old = binding(d, profile, layer, slot)?.action_id.clone();
                if !old
                    .as_ref()
                    .and_then(|id| d.editable.graph.actions.get(id))
                    .is_some_and(is_local_multi)
                {
                    return Err("控件没有 Multi 引用".into());
                }
            }
            binding(d, profile, layer, slot)?.action_id = id.map(str::to_owned);
            Ok(())
        })
    }
    pub fn delete_multi(&mut self, id: &str) -> Result<(), String> {
        self.mutate(|d| {
            if !d.editable.graph.actions.get(id).is_some_and(is_local_multi) {
                return Err("本地 Multi 不存在".into());
            }
            let refs = references(d, id);
            if !refs.is_empty() {
                return Err(format!(
                    "Multi 仍被引用：{}",
                    refs.iter()
                        .map(MacroReference::display_name)
                        .collect::<Vec<_>>()
                        .join("、")
                ));
            }
            d.editable.graph.actions.remove(id);
            d.editable.source_refs.remove(id);
            Ok(())
        })
    }
}
