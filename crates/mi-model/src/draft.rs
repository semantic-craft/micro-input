//! R-E01/R-E02/R-K01/R-P03: offline intent drafts, never device payloads.
use serde::Serialize;
use std::collections::BTreeMap;
#[derive(Clone, Serialize)]
pub struct Action {
    pub id: String,
    pub name: String,
    pub category: String,
    pub detail: String,
}
pub fn actions() -> Vec<Action> {
    [
        (
            "search",
            "搜索文件",
            "快捷键",
            "⌘ P · 需要前台编辑器；仅草稿",
        ),
        ("save", "保存文件", "快捷键", "⌘ S · 需要前台编辑器；仅草稿"),
        (
            "review",
            "插入审查提示",
            "文本",
            "请检查当前变更，优先报告可复现的问题。",
        ),
        ("pane", "下一个 pane", "Herdr", "工具接入待验证 · 不执行"),
        ("session", "切换会话", "DSH", "工具接入待验证 · 不执行"),
    ]
    .into_iter()
    .map(|(id, name, category, detail)| Action {
        id: id.into(),
        name: name.into(),
        category: category.into(),
        detail: detail.into(),
    })
    .collect()
}
#[derive(Default, Serialize)]
pub struct Draft {
    pub edits: BTreeMap<String, String>,
    #[serde(skip)]
    undo: Vec<BTreeMap<String, String>>,
    #[serde(skip)]
    redo: Vec<BTreeMap<String, String>>,
    pub revision: u64,
    pub can_undo: bool,
    pub can_redo: bool,
}
impl Draft {
    pub fn assign(
        &mut self,
        profile: u32,
        layer: u32,
        slot: u32,
        action: &str,
    ) -> Result<(), String> {
        let demo = crate::demo().map_err(|e| e.to_string())?;
        let l = demo
            .profiles
            .get(profile as usize)
            .and_then(|p| p.layers.get(layer as usize))
            .ok_or("无效配置或层")?;
        if slot as usize >= 16 + l.joystick.sectors.len()
            || !actions().iter().any(|a| a.id == action)
        {
            return Err("无效控件或动作".into());
        }
        let key = format!("{profile}:{layer}:{slot}");
        if self.edits.get(&key).is_some_and(|a| a == action) {
            return Ok(());
        }
        self.undo.push(self.edits.clone());
        self.redo.clear();
        self.edits.insert(key, action.into());
        self.changed();
        Ok(())
    }
    pub fn undo(&mut self) {
        if let Some(old) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.edits, old));
            self.changed();
        }
    }
    pub fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.edits, next));
            self.changed();
        }
    }
    fn changed(&mut self) {
        self.revision += 1;
        self.can_undo = !self.undo.is_empty();
        self.can_redo = !self.redo.is_empty();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offline_edit_validation_undo_redo_and_raw_preservation() {
        let raw = crate::demo().unwrap().raw;
        let mut d = Draft::default();
        assert!(d.assign(99, 0, 0, "save").is_err());
        assert!(d.assign(0, 0, 99, "save").is_err());
        assert!(d.assign(0, 0, 0, "arbitrary-rpc").is_err());
        assert_eq!(d.revision, 0);
        d.assign(0, 0, 1, "save").unwrap();
        d.assign(1, 0, 2, "review").unwrap();
        assert_eq!(d.edits.len(), 2);
        d.undo();
        assert_eq!(d.edits.len(), 1);
        d.redo();
        assert_eq!(d.edits.len(), 2);
        d.undo();
        d.assign(0, 1, 0, "search").unwrap();
        assert!(!d.can_redo);
        assert_eq!(crate::demo().unwrap().raw, raw); // Unknown original binding survives draft overlay.
    }
}
