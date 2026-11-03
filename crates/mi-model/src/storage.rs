//! R-E08: bounded local storage, conflict detection, atomic replacement. No device access.
use crate::draft::Draft;
use std::os::unix::fs::OpenOptionsExt;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
};
const LIMIT: u64 = 4 * 1024 * 1024;
#[derive(Default)]
pub struct DocumentStore {
    pub draft: Draft,
    comparison: Option<crate::comparison::Comparison>,
    path: Option<PathBuf>,
    expected: Option<Vec<u8>>,
    blocked: bool,
    selection: Option<(PathBuf, Option<Vec<u8>>)>,
    preferences: HostPreferencesStore,
    recovery_preview: Option<recovery::InspectedRecovery>,
    agent_preview: Option<agent::Pending>,
    apply_preview: Option<crate::apply::Pending>,
}
fn read(path: &std::path::Path) -> Result<Option<Vec<u8>>, String> {
    let f = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let mut b = Vec::new();
    f.take(LIMIT + 1)
        .read_to_end(&mut b)
        .map_err(|e| e.to_string())?;
    if b.len() as u64 > LIMIT {
        return Err("本地文档超过 4 MiB；原文件保留".into());
    }
    Ok(Some(b))
}
impl DocumentStore {
    fn ensure_apply_document(&self) -> Result<Vec<u8>, String> {
        if self.blocked || self.draft.read_only || self.draft.loaded_format != 2 {
            return Err("文档只读、未来格式或未载入；不能准备应用".into());
        }
        let path = self.path.as_ref().ok_or("请先打开已保存的本地草稿")?;
        let bytes = read(path)?.ok_or("本地草稿未保存")?;
        if Some(&bytes) != self.expected.as_ref() {
            return Err("本地文档原字节已变化；请重新打开并预览".into());
        }
        if serde_json::from_slice::<serde_json::Value>(&bytes).map_err(|_| "文档原字节无效")?
            != serde_json::to_value(&self.draft.document).map_err(|e| e.to_string())?
        {
            return Err("内存与磁盘文档不一致；请重新打开并预览".into());
        }
        if let Some((path, expected)) = &self.selection {
            if &read(path)? != expected {
                return Err("所选文档已变化；请重新预览".into());
            }
        }
        Ok(bytes)
    }
    pub fn preview_apply(
        &mut self,
        current: &serde_json::Value,
    ) -> Result<crate::apply::Preview, String> {
        let bytes = self.ensure_apply_document()?;
        let pending = crate::apply::Pending::new(&self.draft.document, &bytes, current)?;
        let preview = pending.preview.clone();
        self.apply_preview = Some(pending);
        Ok(preview)
    }
    pub fn cancel_apply_preview(&mut self) {
        self.apply_preview = None;
    }
    pub fn prepare_apply_backup(
        &mut self,
        token: &str,
        current: &serde_json::Value,
    ) -> Result<crate::apply::Prepared, String> {
        let bytes = self.ensure_apply_document()?;
        let pending = self
            .apply_preview
            .as_ref()
            .filter(|p| p.preview.token == token)
            .ok_or("预览已过期；请重新预览")?;
        pending.ensure_current(&self.draft.document, &bytes, current)?;
        if pending.prepared {
            return Err("原副本已准备，不重复保存".into());
        }
        let path = self.path.as_ref().ok_or("没有文档路径")?;
        let _guard = Lock::acquire(path.with_extension("lock"))?;
        self.ensure_apply_document()?;
        let result = pending.backup(path.parent().ok_or("无效文档目录")?)?;
        self.ensure_apply_document()?;
        self.apply_preview.as_mut().unwrap().prepared = true;
        Ok(result)
    }
    /// Consumes the frozen software plan once, after checking the saved document and session.
    pub fn take_prepared_candidate(
        &mut self,
        token: &str,
        current: &serde_json::Value,
    ) -> Result<crate::apply::PreparedCandidate, String> {
        let bytes = self.ensure_apply_document()?;
        let pending = self
            .apply_preview
            .as_ref()
            .filter(|p| p.preview.token == token)
            .ok_or("计划不存在或已消费")?;
        pending.ensure_current(&self.draft.document, &bytes, current)?;
        let candidate = pending.candidate()?;
        self.apply_preview = None;
        Ok(candidate)
    }
    pub fn compare(
        &mut self,
        config: &crate::DeviceConfig,
        baseline: serde_json::Value,
    ) -> Result<crate::comparison::Comparison, String> {
        if self.blocked || self.draft.read_only {
            return Err("本地文档只读或未载入；不能比较".into());
        }
        let comparison =
            crate::comparison::Comparison::new(&self.draft.document, config, baseline)?;
        self.comparison = Some(comparison.clone());
        Ok(comparison)
    }
    pub fn confirm_comparison_identity(
        &mut self,
        token: &str,
        current: &serde_json::Value,
    ) -> Result<crate::comparison::Comparison, String> {
        let comparison = self
            .comparison
            .as_mut()
            .filter(|c| c.token == token)
            .ok_or("比较结果已过期；请重新比较")?;
        comparison.confirm_identity(&self.draft.document, current)?;
        Ok(comparison.clone())
    }
    pub fn resolve_comparison(
        &mut self,
        token: &str,
        choice: crate::comparison::Choice,
        current: &serde_json::Value,
    ) -> Result<(), String> {
        let comparison = self
            .comparison
            .as_ref()
            .filter(|c| c.token == token)
            .ok_or("比较结果已过期；请重新比较")?;
        if choice == crate::comparison::Choice::Cancel {
            self.comparison = None;
            return Ok(());
        }
        comparison.ensure_snapshot(current)?;
        let next = comparison
            .resolve(&self.draft.document, choice)?
            .ok_or("没有比较结果")?;
        let draft = Draft::from_document(next, 2, false)?;
        self.update_with_recovery(
            |d| {
                *d = draft;
                Ok(())
            },
            true,
        )?;
        self.comparison = None;
        Ok(())
    }

    pub fn open(&mut self, path: PathBuf) -> Result<(), String> {
        self.blocked = true;
        let preferences_path = path.with_extension("preferences.json");
        let selection_path = path.with_extension("active.json");
        let selection_bytes = read(&selection_path)?;
        self.path = Some(path.clone());
        self.selection = Some((selection_path.clone(), selection_bytes.clone()));
        self.recovery_preview = None;
        self.preferences.open(preferences_path.clone());
        let path = if let Some(bytes) = &selection_bytes {
            let name: String =
                serde_json::from_slice(bytes).map_err(|_| "所选文档记录损坏；原文件保留")?;
            let id = name
                .strip_prefix("draft-")
                .and_then(|s| s.strip_suffix(".json"))
                .ok_or("所选文档路径无效")?;
            if uuid::Uuid::parse_str(id).is_err() || name.contains('/') || name.contains('\\') {
                return Err("所选文档路径无效".into());
            }
            let chosen = path.parent().ok_or("无效文档目录")?.join(name);
            if !chosen.is_file() {
                return Err("所选本地文档不存在；没有切换到其他草稿".into());
            }
            chosen
        } else {
            path
        };
        self.path = Some(path.clone());
        let bytes = read(&path)?;
        let draft = match &bytes {
            Some(b) => Draft::from_bytes(b)?,
            None => Draft::default(),
        };
        self.draft = draft;
        self.path = Some(path);
        self.expected = bytes;
        self.selection = Some((selection_path, selection_bytes));
        self.preferences.open(preferences_path);
        self.blocked = false;
        Ok(())
    }
    pub fn favorite_actions(&self) -> Result<&std::collections::BTreeSet<String>, String> {
        if let Some(error) = &self.preferences.error {
            return Err(error.clone());
        }
        Ok(&self.preferences.value.favorites)
    }
    pub fn set_action_favorite(&mut self, id: &str, favorite: bool) -> Result<(), String> {
        if self.blocked {
            return Err("本地文档未载入，收藏没有保存".into());
        }
        if !crate::draft::actions().iter().any(|action| action.id == id) {
            return Err("动作目录没有此标识；收藏没有保存".into());
        }
        self.preferences.set_favorite(id, favorite)
    }
    pub fn host_active_profile_id(&self) -> Option<String> {
        self.preferences.value.host_active_profile_id.clone()
    }
    pub fn host_execution_enabled(&self) -> bool {
        self.preferences.value.host_execution_enabled
    }
    pub fn set_host_active_profile(&mut self, profile_id: Option<String>) -> Result<(), String> {
        if self.blocked {
            return Err("本地文档未载入，宿主激活 Profile 未保存".into());
        }
        if let Some(pid) = &profile_id {
            if !self
                .draft
                .document
                .editable
                .graph
                .profiles
                .contains_key(pid)
            {
                return Err("所选宿主激活 Profile 不存在".into());
            }
        }
        self.preferences.set_host_active_profile(profile_id)
    }
    pub fn set_host_execution_enabled(&mut self, enabled: bool) -> Result<(), String> {
        if self.blocked {
            return Err("本地文档未载入，宿主执行开关未保存".into());
        }
        self.preferences.set_host_execution_enabled(enabled)
    }
    /// Explicitly save a separate document, then select it. The previously open file is untouched.
    pub fn create_independent(
        &mut self,
        config: crate::DeviceConfig,
        baseline: serde_json::Value,
    ) -> Result<(), String> {
        if self.blocked {
            return Err("当前文档未载入，不能切换草稿".into());
        }
        let doc = crate::document::LocalDocumentV2::from_config(
            &config,
            Some(baseline),
            "device-snapshot",
        )?;
        let next = Draft::from_document(doc, 2, false)?;
        self.select_independent(next)
    }
    pub fn import_preset(&mut self, text: &str) -> Result<(), String> {
        let next = crate::preset::import(text)?;
        self.select_independent(next)
    }
    fn select_independent(&mut self, next: Draft) -> Result<(), String> {
        self.select_copy(next, false)
    }
    fn select_copy(&mut self, next: Draft, recovery: bool) -> Result<(), String> {
        if self.blocked && !recovery {
            return Err("当前文档未载入，不能切换草稿".into());
        }
        let (selection_path, expected) = self.selection.as_ref().ok_or("请先打开工作台")?;
        let bytes = serde_json::to_vec_pretty(&next.document).map_err(|e| e.to_string())?;
        if bytes.len() as u64 > LIMIT {
            return Err("本地文档超过 4 MiB；未创建草稿".into());
        }
        let parent = selection_path.parent().ok_or("无效文档目录")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let _guard = Lock::acquire(selection_path.with_extension("lock"))?;
        if &read(selection_path)? != expected {
            return Err("所选文档已被其他实例修改；请重开应用".into());
        }
        if recovery {
            if let Some(original) = expected {
                use sha2::{Digest, Sha256};
                let hash = format!("{:x}", Sha256::digest(original));
                let backup = selection_path.with_file_name(format!(
                    "{}.recovery.{hash}.backup",
                    selection_path
                        .file_name()
                        .ok_or("无效文件名")?
                        .to_string_lossy()
                ));
                match OpenOptions::new()
                    .write(true)
                    .mode(0o600)
                    .create_new(true)
                    .open(&backup)
                {
                    Ok(mut file) => file
                        .write_all(original)
                        .and_then(|_| file.sync_all())
                        .map_err(|e| e.to_string())?,
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(e.to_string()),
                }
                if read(&backup)?.as_ref() != Some(original) {
                    return Err("所选文档恢复副本不一致；未切换草稿".into());
                }
            }
        }
        let name = format!("draft-{}.json", next.document_id);
        let path = parent.join(&name);
        let mut f = OpenOptions::new()
            .write(true)
            .mode(0o600)
            .create_new(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        let selected = serde_json::to_vec(&name).map_err(|e| e.to_string())?;
        let pending = selection_path.with_extension("pending");
        let result = (|| -> Result<(), String> {
            f.write_all(&bytes)
                .and_then(|_| f.sync_all())
                .map_err(|e| e.to_string())?;
            let mut p = OpenOptions::new()
                .write(true)
                .mode(0o600)
                .create_new(true)
                .open(&pending)
                .map_err(|_| "所选文档存在未完成保存记录")?;
            let result = (|| -> Result<(), String> {
                p.write_all(&selected)
                    .and_then(|_| p.sync_all())
                    .map_err(|e| e.to_string())?;
                if &read(selection_path)? != expected {
                    return Err("所选文档已变化；未切换草稿".into());
                }
                fs::rename(&pending, selection_path).map_err(|e| e.to_string())
            })();
            if result.is_err() {
                let _ = fs::remove_file(&pending);
            }
            result
        })();
        if let Err(e) = result {
            return Err(format!("未切换草稿；原草稿保持，新文档文件保留供检查：{e}"));
        }
        self.blocked = false;
        self.comparison = None;
        self.draft = next;
        self.path = Some(path);
        self.expected = Some(bytes);
        self.selection = Some((selection_path.clone(), Some(selected)));
        Ok(())
    }
    pub fn update(
        &mut self,
        edit: impl FnOnce(&mut Draft) -> Result<(), String>,
    ) -> Result<(), String> {
        self.update_with_recovery(edit, false)
    }
    fn update_with_recovery(
        &mut self,
        edit: impl FnOnce(&mut Draft) -> Result<(), String>,
        recovery: bool,
    ) -> Result<(), String> {
        if self.blocked {
            return Err("本地文档未成功载入，已停止保存；原文件保留".into());
        }
        let mut next = self.draft.clone();
        edit(&mut next)?;
        if next.revision == self.draft.revision {
            return Ok(());
        }
        if let Some(path) = &self.path {
            let parent = path.parent().ok_or("无效保存目录")?;
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            let lock = path.with_extension("lock");
            let _guard = Lock::acquire(lock)?;
            if read(path)? != self.expected {
                return Err("本地文档已被其他实例修改；请重开应用后编辑".into());
            }
            let bytes = serde_json::to_vec_pretty(&next.document).map_err(|e| e.to_string())?;
            if bytes.len() as u64 > LIMIT {
                return Err("本地文档超过 4 MiB".into());
            }
            if self.draft.loaded_format == 1 {
                use sha2::{Digest, Sha256};
                let original = self.expected.as_ref().ok_or("缺少迁移原件")?;
                let hash = format!("{:x}", Sha256::digest(original));
                let backup = path.with_file_name(format!(
                    "{}.v1.{hash}.backup",
                    path.file_name().ok_or("无效文件名")?.to_string_lossy()
                ));
                match OpenOptions::new()
                    .write(true)
                    .mode(0o600)
                    .create_new(true)
                    .open(&backup)
                {
                    Ok(mut f) => {
                        f.write_all(original)
                            .and_then(|_| f.sync_all())
                            .map_err(|e| e.to_string())?;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(e.to_string()),
                }
                if read(&backup)?.as_ref() != Some(original) {
                    return Err("迁移备份不一致；原件保留".into());
                }
            }
            if recovery {
                use sha2::{Digest, Sha256};
                let old = self.expected.clone().unwrap_or(
                    serde_json::to_vec_pretty(&self.draft.document).map_err(|e| e.to_string())?,
                );
                let hash = format!("{:x}", Sha256::digest(&old));
                let backup = path.with_file_name(format!(
                    "{}.reconnect.{hash}.backup",
                    path.file_name().ok_or("无效文件名")?.to_string_lossy()
                ));
                match OpenOptions::new()
                    .write(true)
                    .mode(0o600)
                    .create_new(true)
                    .open(&backup)
                {
                    Ok(mut f) => f
                        .write_all(&old)
                        .and_then(|_| f.sync_all())
                        .map_err(|e| e.to_string())?,
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(e.to_string()),
                }
                if read(&backup)?.as_ref() != Some(&old) {
                    return Err("重连恢复副本不一致；没有保存".into());
                }
            }
            let tmp = path.with_extension("pending");
            let mut f = OpenOptions::new()
                .write(true)
                .mode(0o600)
                .create_new(true)
                .open(&tmp)
                .map_err(|_| "存在未完成保存文件；请保留并检查 .pending 文件")?;
            let result = (|| {
                f.write_all(&bytes)?;
                f.sync_all()?;
                if read(path).map_err(std::io::Error::other)? != self.expected {
                    return Err(std::io::Error::other("保存前原件已变化"));
                }
                fs::rename(&tmp, path)
            })()
            .map_err(|e: std::io::Error| e.to_string());
            if result.is_err() {
                let _ = fs::remove_file(&tmp);
            }
            result?;
            self.expected = Some(bytes);
        }
        next.loaded_format = 2;
        self.draft = next;
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct HostPreferences {
    schema_version: u32,
    favorites: std::collections::BTreeSet<String>,
    #[serde(default)]
    host_active_profile_id: Option<String>,
    #[serde(default)]
    host_execution_enabled: bool,
}
impl Default for HostPreferences {
    fn default() -> Self {
        Self {
            schema_version: 1,
            favorites: Default::default(),
            host_active_profile_id: None,
            host_execution_enabled: false,
        }
    }
}
#[derive(Default)]
struct HostPreferencesStore {
    value: HostPreferences,
    path: Option<PathBuf>,
    expected: Option<Vec<u8>>,
    error: Option<String>,
}
impl HostPreferencesStore {
    fn open(&mut self, path: PathBuf) {
        *self = Self::default();
        let result = (|| -> Result<(), String> {
            let bytes = read(&path)?;
            let value: HostPreferences = match &bytes {
                Some(bytes) => serde_json::from_slice(bytes)
                    .map_err(|_| "收藏文件损坏或包含未知字段；原件保留")?,
                None => HostPreferences::default(),
            };
            if value.schema_version != 1 {
                return Err("收藏版本不支持；原件保留".into());
            }
            self.value = value;
            self.path = Some(path);
            self.expected = bytes;
            Ok(())
        })();
        self.error = result.err();
    }
    fn save(&mut self, value: HostPreferences) -> Result<(), String> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        if value == self.value {
            return Ok(());
        }
        let Some(path) = self.path.as_ref() else {
            self.value = value;
            return Ok(());
        };
        fs::create_dir_all(path.parent().ok_or("无效收藏目录")?).map_err(|e| e.to_string())?;
        let _guard = Lock::acquire(path.with_extension("lock"))?;
        if read(path)? != self.expected {
            return Err("收藏已被其他实例修改；请重开应用".into());
        }
        let bytes = serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?;
        if bytes.len() as u64 > LIMIT {
            return Err("收藏文件过大；没有保存".into());
        }
        let pending = path.with_extension("pending");
        let mut file = OpenOptions::new()
            .write(true)
            .mode(0o600)
            .create_new(true)
            .open(&pending)
            .map_err(|_| "收藏存在未完成保存文件；原件保留")?;
        let result = (|| -> Result<(), String> {
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|e| e.to_string())?;
            if read(path)? != self.expected {
                return Err("保存前收藏已变化；原件保留".into());
            }
            fs::rename(&pending, path).map_err(|e| e.to_string())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&pending);
        }
        result?;
        self.value = value;
        self.expected = Some(bytes);
        Ok(())
    }
    fn set_favorite(&mut self, id: &str, favorite: bool) -> Result<(), String> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        let _ = self.path.as_ref().ok_or("请先打开工作台再收藏")?;
        let mut next = self.value.clone();
        if favorite {
            next.favorites.insert(id.into());
        } else {
            next.favorites.remove(id);
        }
        self.save(next)
    }
    fn set_host_active_profile(&mut self, profile_id: Option<String>) -> Result<(), String> {
        let mut next = self.value.clone();
        next.host_active_profile_id = profile_id;
        self.save(next)
    }
    fn set_host_execution_enabled(&mut self, enabled: bool) -> Result<(), String> {
        let mut next = self.value.clone();
        next.host_execution_enabled = enabled;
        self.save(next)
    }
}
struct Lock(PathBuf);
impl Lock {
    fn acquire(path: PathBuf) -> Result<Self, String> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| "文档正在保存或上次保存中断；请检查 .lock 文件")?;
        Ok(Self(path))
    }
}
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[path = "recovery.rs"]
mod recovery;
pub use recovery::RecoveryPreview;

#[path = "agent.rs"]
pub mod agent;
