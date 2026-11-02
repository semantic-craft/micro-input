//! Named macros and shared bindings in the existing document graph; no separate persistence.
use crate::{
    document::{new_id, BindingNode, LocalDocumentV2},
    draft::{Action, Draft},
};
use serde::Serialize;
use serde_json::Value;

pub fn is_local_macro(action: &Action) -> bool {
    action.payload["type"] == "local_macro"
}
#[derive(Serialize)]
pub struct MacroSummary {
    pub id: String,
    pub action: Action,
    pub references: Vec<MacroReference>,
}
#[derive(Serialize)]
pub struct MacroReference {
    pub profile_id: String,
    pub profile_name: String,
    pub layer_id: String,
    pub layer_name: String,
    pub logical_control: String,
    pub parent_action_name: Option<String>,
    pub branch: Option<String>,
}
pub(crate) fn references(doc: &LocalDocumentV2, action_id: &str) -> Vec<MacroReference> {
    let graph = &doc.editable.graph;
    let mut result: Vec<_> = graph
        .profile_order
        .iter()
        .flat_map(|pid| {
            let profile = &graph.profiles[pid];
            profile.layer_order.iter().flat_map(move |lid| {
                let layer = &graph.layers[lid];
                layer.control_order.iter().filter_map(move |bid| {
                    let binding = &layer.controls[bid];
                    (binding.action_id.as_deref() == Some(action_id)).then(|| MacroReference {
                        parent_action_name: None,
                        branch: None,
                        profile_id: pid.clone(),
                        profile_name: profile.name.clone(),
                        layer_id: lid.clone(),
                        layer_name: layer.name.clone(),
                        logical_control: binding.logical_control.clone(),
                    })
                })
            })
        })
        .collect();
    result.extend(crate::multi::parent_references(doc, action_id));
    result
}
pub(crate) fn binding<'a>(
    doc: &'a mut LocalDocumentV2,
    profile: &Value,
    layer: &Value,
    slot: u32,
) -> Result<&'a mut BindingNode, String> {
    let pid = profile.as_str().ok_or("配置必须是本地 UUID")?;
    let lid = layer.as_str().ok_or("层必须是本地 UUID")?;
    let p = doc.editable.graph.profiles.get(pid).ok_or("配置不存在")?;
    if !p.layer_order.iter().any(|id| id == lid) {
        return Err("层不属于所选配置".into());
    }
    doc.editable
        .graph
        .layers
        .get_mut(lid)
        .ok_or("层不存在")?
        .controls
        .values_mut()
        .find(|b| b.logical_control == format!("logical-{slot}"))
        .ok_or("控件不存在".into())
}
impl Draft {
    pub fn macros(&self) -> Vec<MacroSummary> {
        self.document
            .editable
            .graph
            .actions
            .iter()
            .filter(|(_, a)| is_local_macro(a))
            .map(|(id, a)| MacroSummary {
                id: id.clone(),
                action: a.clone(),
                references: references(&self.document, id),
            })
            .collect()
    }
    pub fn save_macro(&mut self, id: Option<&str>, action: Action) -> Result<String, String> {
        if !is_local_macro(&action) {
            return Err("只能保存自有本地宏".into());
        }
        action.validate()?;
        self.mutate(|d| {
            if let Some(id) = id {
                if !d.editable.graph.actions.get(id).is_some_and(is_local_macro) {
                    return Err("本地宏不存在".into());
                }
            }
            let id = id.map(str::to_owned).unwrap_or_else(new_id);
            d.editable.graph.actions.insert(id.clone(), action);
            Ok(id)
        })
    }
    pub fn save_macro_for_binding(
        &mut self,
        id: Option<&str>,
        action: Action,
        profile: &Value,
        layer: &Value,
        slot: u32,
    ) -> Result<String, String> {
        if !is_local_macro(&action) {
            return Err("只能保存自有本地宏".into());
        }
        action.validate()?;
        self.mutate(|d| {
            if let Some(id) = id {
                if !d.editable.graph.actions.get(id).is_some_and(is_local_macro) {
                    return Err("本地宏不存在".into());
                }
            }
            let id = id.map(str::to_owned).unwrap_or_else(new_id);
            binding(d, profile, layer, slot)?.action_id = Some(id.clone());
            d.editable.graph.actions.insert(id.clone(), action);
            Ok(id)
        })
    }
    pub fn bind_macro(
        &mut self,
        profile: &Value,
        layer: &Value,
        slot: u32,
        id: Option<&str>,
    ) -> Result<(), String> {
        self.mutate(|d| {
            if let Some(id) = id {
                if !d.editable.graph.actions.get(id).is_some_and(is_local_macro) {
                    return Err("本地宏不存在".into());
                }
            }
            let old = binding(d, profile, layer, slot)?.action_id.clone();
            if id.is_none()
                && !old
                    .as_ref()
                    .and_then(|a| d.editable.graph.actions.get(a))
                    .is_some_and(is_local_macro)
            {
                return Err("控件没有本地宏引用".into());
            }
            binding(d, profile, layer, slot)?.action_id = id.map(str::to_owned);
            Ok(())
        })
    }
    pub fn delete_macro(&mut self, id: &str) -> Result<(), String> {
        self.mutate(|d| {
            if !d.editable.graph.actions.get(id).is_some_and(is_local_macro) {
                return Err("本地宏不存在".into());
            }
            let refs = references(d, id);
            if !refs.is_empty() {
                return Err(format!(
                    "宏仍被引用，请先移除绑定：{}",
                    refs.iter()
                        .map(|r| r.display_name())
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

impl MacroReference {
    pub fn display_name(&self) -> String {
        if let (Some(parent), Some(branch)) = (&self.parent_action_name, &self.branch) {
            format!("{parent} / {branch}")
        } else {
            format!(
                "{} / {} / {}",
                self.profile_name, self.layer_name, self.logical_control
            )
        }
    }
}
