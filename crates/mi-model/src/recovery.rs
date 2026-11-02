//! Local document recovery only; no device/session/host execution dependencies.
use super::*;
use serde::Serialize;
use sha2::{Digest, Sha256};
#[derive(Clone, Serialize)]
pub struct RecoveryPreview {
    pub token: String,
    pub path: String,
    pub sha256: String,
    pub schema_version: Option<u64>,
    pub source_kind: String,
    pub profiles: usize,
    pub revision: Option<u64>,
    pub can_restore: bool,
    pub message: String,
}
pub(super) struct InspectedRecovery {
    preview: RecoveryPreview,
    bytes: Vec<u8>,
    draft: Option<Draft>,
}
impl DocumentStore {
    /// Discover only owned document and recovery file names. Other app data is not a backup.
    pub fn recovery_files(&self) -> Result<Vec<String>, String> {
        let (selection, _) = self.selection.as_ref().ok_or("请先打开工作台")?;
        let parent = selection.parent().ok_or("无效文档目录")?;
        let mut paths = vec![];
        if !parent.exists() {
            return Ok(paths);
        }
        for entry in fs::read_dir(parent).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.file_type().map_err(|e| e.to_string())?.is_file()
                && (name == "workbench.json"
                    || name == "workbench.active.json"
                    || name == "workbench.active.pending"
                    || name == "workbench.pending"
                    || name.ends_with(".backup")
                    || (name.starts_with("draft-")
                        && (name.ends_with(".json") || name.ends_with(".pending"))))
            {
                paths.push(entry.path().to_string_lossy().into_owned());
            }
        }
        paths.sort();
        Ok(paths)
    }
    /// Pin exact bytes to this preview; confirmation must not consume a changed source.
    pub fn inspect_recovery(&mut self, path: PathBuf) -> Result<RecoveryPreview, String> {
        self.recovery_preview = None;
        let bytes = read(&path)?.ok_or("恢复来源不存在；原件保留")?;
        let raw = serde_json::from_slice::<serde_json::Value>(&bytes).ok();
        let draft = Draft::from_bytes(&bytes).ok();
        let can_restore = draft.as_ref().is_some_and(|d| !d.read_only);
        let preview = RecoveryPreview {
            token: crate::document::new_id(),
            path: path.to_string_lossy().into_owned(),
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            schema_version: raw.as_ref().and_then(|r| r["schema_version"].as_u64()),
            source_kind: draft
                .as_ref()
                .and_then(|d| d.document.origin["kind"].as_str())
                .unwrap_or("unknown")
                .into(),
            profiles: draft
                .as_ref()
                .map_or(0, |d| d.document.editable.graph.profiles.len()),
            revision: raw.as_ref().and_then(|r| r["revision"].as_u64()),
            can_restore,
            message: if can_restore {
                "校验通过；恢复为独立 v2 本地草稿，来源原件保留"
            } else {
                "文件损坏、版本不支持或含未知数据；仅可保留原始字节副本，不能转换或编辑"
            }
            .into(),
        };
        self.recovery_preview = Some(InspectedRecovery {
            preview: preview.clone(),
            bytes,
            draft,
        });
        Ok(preview)
    }
    fn checked_recovery(&self, token: &str) -> Result<&InspectedRecovery, String> {
        let item = self
            .recovery_preview
            .as_ref()
            .filter(|p| p.preview.token == token)
            .ok_or("恢复预览已失效；请重新检查")?;
        if read(std::path::Path::new(&item.preview.path))?.as_ref() != Some(&item.bytes) {
            return Err("恢复来源已变化；请重新检查，未切换草稿".into());
        }
        Ok(item)
    }
    pub fn recover_document(&mut self, token: &str) -> Result<(), String> {
        let item = self.checked_recovery(token)?;
        if !item.preview.can_restore {
            return Err(item.preview.message.clone());
        }
        let mut document = item
            .draft
            .as_ref()
            .ok_or("没有可恢复草稿")?
            .document
            .clone();
        document.document_id = crate::document::new_id();
        document.origin["local_recovery"] = serde_json::json!({"sha256":item.preview.sha256,"schema_version":item.preview.schema_version,"kind":"local-document-copy"});
        let next = Draft::from_document(document, 2, false)?;
        self.select_copy(next, true)?;
        self.recovery_preview = None;
        Ok(())
    }
    pub fn save_document_as(&mut self) -> Result<(), String> {
        if self.blocked || self.draft.read_only {
            return Err("当前文档未载入或含未知数据；请检查原件并保留原始字节副本".into());
        }
        let mut document = self.draft.document.clone();
        document.origin["local_copy"] =
            serde_json::json!({"kind":"save-as","source_document_id":document.document_id});
        document.document_id = crate::document::new_id();
        self.select_copy(Draft::from_document(document, 2, false)?, false)
    }
    /// Never replace an existing destination, including the original or an interrupted file.
    pub fn copy_recovery_original(&self, token: &str, destination: PathBuf) -> Result<(), String> {
        let item = self.checked_recovery(token)?;
        let mut file = OpenOptions::new()
            .write(true)
            .mode(0o600)
            .create_new(true)
            .open(destination)
            .map_err(|e| e.to_string())?;
        file.write_all(&item.bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())
    }
}
