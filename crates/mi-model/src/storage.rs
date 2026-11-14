//! R-E08: bounded local storage, conflict detection, atomic replacement. No device access.
pub mod routing;
use crate::board_layers::{
    BoardLayer, BoardLayerPlan, BoardLayerRoute, BoardPreviewSelection, LegacyThreeBindings,
};
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
    host: Option<Box<DocumentStore>>,
    comparison: Option<crate::comparison::Comparison>,
    path: Option<PathBuf>,
    expected: Option<Vec<u8>>,
    blocked: bool,
    selection: Option<(PathBuf, Option<Vec<u8>>)>,
    preferences: HostPreferencesStore,
    host_state_revision: u64,
    recovery_preview: Option<recovery::InspectedRecovery>,
    agent_preview: Option<agent::Pending>,
    apply_preview: Option<crate::apply::Pending>,
}
pub(crate) fn read(path: &std::path::Path) -> Result<Option<Vec<u8>>, String> {
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
    pub fn editing_device(&self) -> bool {
        self.host.is_some()
    }
    pub fn host_store(&self) -> &Self {
        self.host.as_deref().unwrap_or(self)
    }
    fn device_editor_path(&self) -> Result<PathBuf, String> {
        let host = self.host_store();
        if host.blocked {
            return Err("宿主文档未载入".into());
        }
        Ok(host
            .selection
            .as_ref()
            .and_then(|(p, _)| p.parent())
            .ok_or("请先打开工作台")?
            .join("device-editor.json"))
    }
    fn select_device_editor(&mut self, device: Self) {
        let host = if let Some(host) = self.host.take() {
            host
        } else {
            Box::new(std::mem::take(self))
        };
        *self = device;
        self.host = Some(host);
    }
    pub fn begin_device_editing(
        &mut self,
        config: crate::DeviceConfig,
        baseline: serde_json::Value,
    ) -> Result<(), String> {
        let mut device = Self::default();
        device.open(self.device_editor_path()?)?;
        device.create_independent(config, baseline)?;
        self.select_device_editor(device);
        Ok(())
    }
    pub fn resume_device_editing(&mut self) -> Result<(), String> {
        if self.editing_device() {
            return Ok(());
        }
        let path = self.device_editor_path()?;
        if !path.with_extension("active.json").exists() {
            return Err("没有已保存的设备草稿；请从设备读取".into());
        }
        let mut device = Self::default();
        device.open(path)?;
        self.select_device_editor(device);
        Ok(())
    }
    pub fn return_to_host(&mut self) {
        if let Some(host) = self.host.take() {
            *self = *host;
        }
    }
    fn ensure_apply_document(&self) -> Result<Vec<u8>, String> {
        if self.blocked || self.draft.read_only || self.draft.loaded_format != 2 {
            return Err("文档只读、未来格式或未载入；不能准备应用".into());
        }
        let path = self.path.as_ref().ok_or("请先打开已保存的本地草稿")?;
        crate::operation_record::ensure_document_editable(path)?;
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
        if pending.prepared.is_some() {
            return Err("原副本已准备，不重复保存".into());
        }
        let path = self.path.as_ref().ok_or("没有文档路径")?;
        let _guard = Lock::acquire(path.with_extension("lock"))?;
        self.ensure_apply_document()?;
        let result = pending.backup(path.parent().ok_or("无效文档目录")?)?;
        self.ensure_apply_document()?;
        self.apply_preview.as_mut().unwrap().prepared = Some(result.clone());
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
        let candidate = pending.candidate(self.path.as_ref().unwrap().clone(), bytes)?;
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
        if crate::operation_record::ensure_document_editable(&path).is_err() {
            self.draft.read_only = true;
        }
        self.path = Some(path);
        self.expected = bytes;
        self.selection = Some((selection_path, selection_bytes));
        self.preferences.open(preferences_path);
        if self.preferences.migration_backup.is_some() {
            if let Some(plan) = &mut self.preferences.value.board_layer_plan {
                for layer in BoardLayer::ALL {
                    if let Some(id) = plan.route(layer).profile_id().map(str::to_owned) {
                        if let Some(owner) =
                            crate::board_layers::native_owner(&self.draft.document, &id)
                        {
                            plan.replace(
                                layer,
                                BoardLayerRoute::Native {
                                    owner,
                                    profile_id: id,
                                },
                            );
                        }
                    }
                }
            }
        }
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
        if let Some(host) = &self.host {
            return host.host_active_profile_id();
        }
        self.preferences.value.host_active_profile_id.clone()
    }
    pub fn locked_profile_id(&self) -> Option<String> {
        if let Some(host) = &self.host {
            return host.locked_profile_id();
        }
        self.preferences
            .value
            .locked_profile_id
            .as_ref()
            .filter(|id| {
                self.draft
                    .document
                    .editable
                    .graph
                    .profiles
                    .contains_key(*id)
            })
            .cloned()
    }
    pub fn host_execution_enabled(&self) -> bool {
        if let Some(host) = &self.host {
            return host.host_execution_enabled();
        }
        self.preferences.value.host_execution_enabled
    }
    pub fn command_execution_enabled(&self) -> bool {
        if let Some(host) = &self.host {
            return host.command_execution_enabled();
        }
        self.preferences.value.command_execution_enabled
    }
    pub fn bottom_row_layout(&self) -> crate::host_router::BottomRowLayout {
        if let Some(host) = &self.host {
            return host.bottom_row_layout();
        }
        self.preferences.value.bottom_row_layout
    }
    pub fn has_saved_host_preferences(&self) -> bool {
        self.host_store().preferences.expected.is_some()
    }
    pub fn board_layer_plan(&self) -> Option<&BoardLayerPlan> {
        self.host_store()
            .preferences
            .value
            .board_layer_plan
            .as_ref()
    }
    pub fn board_preview_selection(&self) -> BoardPreviewSelection {
        self.host_store().preferences.value.preview_selection
    }
    pub fn usb_startup_profile_id(&self) -> Option<&str> {
        self.host_store()
            .preferences
            .value
            .usb_startup_profile_id
            .as_deref()
    }
    pub fn legacy_three_bindings(&self) -> Option<&LegacyThreeBindings> {
        self.host_store()
            .preferences
            .value
            .legacy_three_bindings
            .as_ref()
    }
    pub fn preferred_device_serial(&self) -> Option<String> {
        self.host_store()
            .preferences
            .value
            .preferred_device_serial
            .clone()
    }
    pub fn set_preferred_device_serial(&mut self, serial: Option<String>) -> Result<bool, String> {
        if let Some(host) = &mut self.host {
            return host.set_preferred_device_serial(serial);
        }
        if self.blocked {
            return Err("本地文档未载入，首选设备序列号未保存".into());
        }
        self.preferences.set_preferred_device_serial(serial)
    }
    pub fn v070_auto_layer_sync_enabled(&self) -> bool {
        self.host_store()
            .preferences
            .value
            .v070_auto_layer_sync_enabled
    }
    pub fn set_v070_auto_layer_sync_enabled(&mut self, enabled: bool) -> Result<bool, String> {
        if let Some(host) = &mut self.host {
            return host.set_v070_auto_layer_sync_enabled(enabled);
        }
        if self.blocked {
            return Err("本地文档未载入，0.7.0 前台自动切层开关未保存".into());
        }
        self.preferences.set_v070_auto_layer_sync_enabled(enabled)
    }
    pub fn v070_session_sa_rgb_sync_enabled(&self) -> bool {
        self.host_store()
            .preferences
            .value
            .v070_session_sa_rgb_sync_enabled
    }
    pub fn set_v070_session_sa_rgb_sync_enabled(&mut self, enabled: bool) -> Result<bool, String> {
        if let Some(host) = &mut self.host {
            return host.set_v070_session_sa_rgb_sync_enabled(enabled);
        }
        if self.blocked {
            return Err("本地文档未载入，0.7.0 会话状态映灯开关未保存".into());
        }
        self.preferences
            .set_v070_session_sa_rgb_sync_enabled(enabled)
    }
    pub fn initialize_board_plan_if_absent(&mut self, plan: BoardLayerPlan) {
        if let Some(host) = &mut self.host {
            host.initialize_board_plan_if_absent(plan);
        } else if self.preferences.value.board_layer_plan.is_none() {
            self.preferences.value.board_layer_plan = Some(plan);
            self.preferences.pending_defaults = true;
        }
    }
    pub fn save_board_configuration(
        &mut self,
        plan: BoardLayerPlan,
        preview: BoardPreviewSelection,
        usb_startup: Option<String>,
    ) -> Result<(), String> {
        if let Some(host) = &mut self.host {
            return host.save_board_configuration(plan, preview, usb_startup);
        }
        if self.blocked {
            return Err("本地文档未载入，六层方案没有保存".into());
        }
        let mut next = self.preferences.value.clone();
        next.board_layer_plan = Some(plan);
        next.preview_selection = preview;
        next.usb_startup_profile_id = usb_startup;
        self.preferences.save_with_sync(next, true)
    }
    pub fn set_host_active_profile(&mut self, profile_id: Option<String>) -> Result<(), String> {
        if let Some(host) = &mut self.host {
            return host.set_host_active_profile(profile_id);
        }
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
        let changed = self.preferences.value.host_active_profile_id != profile_id;
        self.preferences.set_host_active_profile(profile_id)?;
        if changed {
            self.host_state_revision += 1;
        }
        Ok(())
    }
    pub fn set_host_execution_enabled(&mut self, enabled: bool) -> Result<(), String> {
        if let Some(host) = &mut self.host {
            return host.set_host_execution_enabled(enabled);
        }
        if self.blocked {
            return Err("本地文档未载入，宿主执行开关未保存".into());
        }
        let changed = self.preferences.value.host_execution_enabled != enabled;
        self.preferences.set_host_execution_enabled(enabled)?;
        if changed {
            self.host_state_revision += 1;
        }
        Ok(())
    }
    pub fn set_command_execution_enabled(&mut self, enabled: bool) -> Result<(), String> {
        if let Some(host) = &mut self.host {
            return host.set_command_execution_enabled(enabled);
        }
        if self.blocked {
            return Err("本地文档未载入，命令执行开关未保存".into());
        }
        let changed = self.preferences.value.command_execution_enabled != enabled;
        self.preferences.set_command_execution_enabled(enabled)?;
        if changed {
            self.host_state_revision += 1;
        }
        Ok(())
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
        if let Some(path) = &self.path {
            crate::operation_record::ensure_document_editable(path)?;
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
            crate::operation_record::ensure_document_editable(path)?;
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
        if self.host.is_none()
            && self.preferences.error.is_none()
            && self
                .preferences
                .value
                .locked_profile_id
                .as_ref()
                .is_some_and(|id| !self.draft.document.editable.graph.profiles.contains_key(id))
        {
            let mut next_prefs = self.preferences.value.clone();
            next_prefs.locked_profile_id = None;
            if self.preferences.save(next_prefs).is_ok() {
                self.host_state_revision += 1;
            }
        }
        Ok(())
    }
}

fn discard_retired_popover_preview_mode<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<(), D::Error> {
    <serde::de::IgnoredAny as serde::Deserialize>::deserialize(deserializer).map(|_| ())
}

#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct HostPreferencesV1 {
    schema_version: u32,
    favorites: std::collections::BTreeSet<String>,
    #[serde(default)]
    host_active_profile_id: Option<String>,
    #[serde(default)]
    host_execution_enabled: bool,
    #[serde(default)]
    command_execution_enabled: bool,
    #[serde(default)]
    auto_follow_enabled: bool,
    #[serde(default)]
    manual_override: bool,
    #[serde(default)]
    bottom_row_layout: crate::host_router::BottomRowLayout,
    #[serde(
        default,
        skip_serializing,
        rename = "popover_preview_mode",
        deserialize_with = "discard_retired_popover_preview_mode"
    )]
    retired_popover_preview_mode: (),
    #[serde(default)]
    app_profile_links: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    hardware_profile_slots: Option<[Option<String>; 4]>,
    #[serde(default)]
    active_hardware_slot: u8,
}
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct HostPreferences {
    schema_version: u32,
    favorites: std::collections::BTreeSet<String>,
    #[serde(default)]
    host_active_profile_id: Option<String>,
    #[serde(default)]
    pub locked_profile_id: Option<String>,
    #[serde(default)]
    host_execution_enabled: bool,
    #[serde(default)]
    command_execution_enabled: bool,
    #[serde(default)]
    auto_follow_enabled: bool,
    #[serde(default)]
    manual_override: bool,
    #[serde(default)]
    bottom_row_layout: crate::host_router::BottomRowLayout,
    #[serde(
        default,
        skip_serializing,
        rename = "popover_preview_mode",
        deserialize_with = "discard_retired_popover_preview_mode"
    )]
    retired_popover_preview_mode: (),
    #[serde(default)]
    app_profile_links: std::collections::BTreeMap<String, String>,
    usb_startup_profile_id: Option<String>,
    board_layer_plan: Option<BoardLayerPlan>,
    preview_selection: BoardPreviewSelection,
    legacy_three_bindings: Option<LegacyThreeBindings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    preferred_device_serial: Option<String>,
    #[serde(default)]
    v070_auto_layer_sync_enabled: bool,
    #[serde(default)]
    v070_session_sa_rgb_sync_enabled: bool,
}
impl Default for HostPreferences {
    fn default() -> Self {
        Self {
            schema_version: 2,
            favorites: Default::default(),
            host_active_profile_id: None,
            locked_profile_id: None,
            host_execution_enabled: false,
            command_execution_enabled: false,
            auto_follow_enabled: false,
            manual_override: false,
            bottom_row_layout: crate::host_router::BottomRowLayout::Split,
            retired_popover_preview_mode: (),
            app_profile_links: Default::default(),
            usb_startup_profile_id: None,
            board_layer_plan: None,
            preview_selection: BoardPreviewSelection::UsbStartup,
            legacy_three_bindings: None,
            preferred_device_serial: None,
            v070_auto_layer_sync_enabled: false,
            v070_session_sa_rgb_sync_enabled: false,
        }
    }
}
#[derive(Default)]
struct HostPreferencesStore {
    value: HostPreferences,
    path: Option<PathBuf>,
    expected: Option<Vec<u8>>,
    error: Option<String>,
    migration_backup: Option<Vec<u8>>,
    pending_defaults: bool,
}
impl HostPreferencesStore {
    fn open(&mut self, path: PathBuf) {
        *self = Self::default();
        let result = (|| -> Result<(), String> {
            let bytes = read(&path)?;
            let value = match &bytes {
                Some(bytes) => {
                    let header: serde_json::Value =
                        serde_json::from_slice(bytes).map_err(|_| "偏好文件损坏；原件保留")?;
                    match header.get("schema_version").and_then(|v| v.as_u64()) {
                        Some(1) => {
                            // An explicit null is not an absent array.
                            if header
                                .get("hardware_profile_slots")
                                .is_some_and(|v| v.is_null())
                            {
                                return Err("硬件槽位格式无效；原件保留".into());
                            }
                            let old: HostPreferencesV1 = serde_json::from_slice(bytes)
                                .map_err(|_| "偏好文件损坏或包含未知字段；原件保留")?;
                            if old.active_hardware_slot > 3 {
                                return Err("旧硬件槽位编号无效；原件保留".into());
                            }
                            let plan = old.hardware_profile_slots.as_ref().map(|slots| {
                                let mut plan = BoardLayerPlan::default();
                                for (layer, id) in
                                    BoardLayer::ALL.into_iter().take(3).zip(&slots[1..])
                                {
                                    if let Some(profile_id) = id {
                                        plan.replace(
                                            layer,
                                            BoardLayerRoute::HostProfile {
                                                profile_id: profile_id.clone(),
                                            },
                                        );
                                    }
                                }
                                plan
                            });
                            let legacy = old.hardware_profile_slots.as_ref().map(|s| {
                                LegacyThreeBindings([s[1].clone(), s[2].clone(), s[3].clone()])
                            });
                            self.migration_backup = Some(bytes.clone());
                            HostPreferences {
                                schema_version: 2,
                                favorites: old.favorites,
                                host_active_profile_id: old.host_active_profile_id,
                                locked_profile_id: None,
                                host_execution_enabled: old.host_execution_enabled,
                                command_execution_enabled: old.command_execution_enabled,
                                auto_follow_enabled: old.auto_follow_enabled,
                                manual_override: old.manual_override,
                                bottom_row_layout: old.bottom_row_layout,
                                retired_popover_preview_mode: (),
                                app_profile_links: old.app_profile_links,
                                usb_startup_profile_id: old
                                    .hardware_profile_slots
                                    .and_then(|s| s[0].clone()),
                                board_layer_plan: plan,
                                preview_selection: if old.active_hardware_slot == 0 {
                                    BoardPreviewSelection::UsbStartup
                                } else {
                                    BoardPreviewSelection::Layer(BoardLayer::try_from(
                                        old.active_hardware_slot,
                                    )?)
                                },
                                legacy_three_bindings: legacy,
                                preferred_device_serial: None,
                                v070_auto_layer_sync_enabled: false,
                                v070_session_sa_rgb_sync_enabled: false,
                            }
                        }
                        Some(2) => serde_json::from_slice(bytes)
                            .map_err(|_| "偏好文件损坏或包含未知字段；原件保留")?,
                        _ => return Err("偏好版本不支持；原件保留".into()),
                    }
                }
                None => HostPreferences::default(),
            };
            self.value = value;
            self.path = Some(path);
            self.expected = bytes;
            Ok(())
        })();
        self.error = result.err();
    }
    fn save(&mut self, value: HostPreferences) -> Result<(), String> {
        self.save_with_sync(value, false)
    }
    fn save_with_sync(&mut self, value: HostPreferences, durable: bool) -> Result<(), String> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        if value == self.value && self.migration_backup.is_none() && !self.pending_defaults {
            return Ok(());
        }
        let Some(path) = self.path.as_ref() else {
            self.value = value;
            self.pending_defaults = false;
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
            file.write_all(&bytes).map_err(|e| e.to_string())?;
            // Board edits and migration are durable recovery boundaries; runtime routing keeps its atomic rename latency.
            if durable || self.pending_defaults || self.migration_backup.is_some() {
                file.sync_all().map_err(|e| e.to_string())?;
            }
            if let Some(original) = &self.migration_backup {
                let backup = path.with_extension("v1.backup.json");
                match read(&backup)? {
                    Some(existing) if existing == *original => {}
                    Some(_) => return Err("旧偏好备份已存在且内容不同；原件保留".into()),
                    None => {
                        let mut recovery = OpenOptions::new()
                            .write(true)
                            .mode(0o600)
                            .create_new(true)
                            .open(&backup)
                            .map_err(|e| e.to_string())?;
                        recovery.write_all(original).map_err(|e| e.to_string())?;
                        recovery.sync_all().map_err(|e| e.to_string())?;
                    }
                }
                if read(&backup)?.as_ref() != Some(original) {
                    return Err("旧偏好备份读回不符；原件保留".into());
                }
                fs::File::open(path.parent().ok_or("无效偏好目录")?)
                    .and_then(|directory| directory.sync_all())
                    .map_err(|e| e.to_string())?;
            }
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
        self.migration_backup = None;
        self.pending_defaults = false;
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
    fn set_command_execution_enabled(&mut self, enabled: bool) -> Result<(), String> {
        let mut next = self.value.clone();
        next.command_execution_enabled = enabled;
        self.save(next)
    }
    fn set_preferred_device_serial(&mut self, serial: Option<String>) -> Result<bool, String> {
        let normalized = serial
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());
        if self.value.preferred_device_serial == normalized {
            return Ok(false);
        }
        let mut next = self.value.clone();
        next.preferred_device_serial = normalized;
        self.save_with_sync(next, true)?;
        Ok(true)
    }
    fn set_v070_auto_layer_sync_enabled(&mut self, enabled: bool) -> Result<bool, String> {
        if self.value.v070_auto_layer_sync_enabled == enabled {
            return Ok(false);
        }
        let mut next = self.value.clone();
        next.v070_auto_layer_sync_enabled = enabled;
        self.save_with_sync(next, true)?;
        Ok(true)
    }
    fn set_v070_session_sa_rgb_sync_enabled(&mut self, enabled: bool) -> Result<bool, String> {
        if self.value.v070_session_sa_rgb_sync_enabled == enabled {
            return Ok(false);
        }
        let mut next = self.value.clone();
        next.v070_session_sa_rgb_sync_enabled = enabled;
        self.save_with_sync(next, true)?;
        Ok(true)
    }
}
pub(crate) struct Lock(PathBuf);
impl Lock {
    pub(crate) fn acquire(path: PathBuf) -> Result<Self, String> {
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

impl DocumentStore {
    pub fn device_operation_paths(&self) -> Result<Vec<std::path::PathBuf>, String> {
        let path = self.path.as_ref().ok_or("未打开文档")?;
        let root = path.parent().ok_or("无文档目录")?.join("device-backups");
        if !root.exists() {
            return Ok(vec![]);
        }
        let current = fs::canonicalize(path).map_err(|e| e.to_string())?;
        let mut paths = vec![];
        for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
            let p = entry.map_err(|e| e.to_string())?.path().join("operation");
            if p.exists() {
                let p = fs::canonicalize(p).map_err(|e| e.to_string())?;
                let r = crate::operation_record::OperationRecord::open(&p)?;
                if r.document_path()? == current {
                    paths.push(p);
                }
            }
        }
        Ok(paths)
    }
    pub fn reload_operation_document(
        &mut self,
        record: &crate::operation_record::OperationRecord,
    ) -> Result<(), String> {
        let path = self.path.as_ref().ok_or("未打开文档")?;
        if fs::canonicalize(path).map_err(|e| e.to_string())? != record.document_path()? {
            return Err("操作属于其他文档".into());
        }
        let bytes = read(path)?.ok_or("文档缺失")?;
        self.draft = Draft::from_bytes(&bytes)?;
        self.draft.read_only |= crate::operation_record::ensure_document_editable(path).is_err();
        self.expected = Some(bytes);
        self.apply_preview = None;
        self.comparison = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_preferences_without_fw5_fields_deserializes_with_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let doc_path = dir.path().join("workbench.json");
        let prefs_path = dir.path().join("workbench.preferences.json");
        fs::write(
            &prefs_path,
            r#"{"schema_version":2,"favorites":[],"usb_startup_profile_id":null,"board_layer_plan":null,"preview_selection":"UsbStartup","legacy_three_bindings":null}"#,
        )
        .unwrap();

        let mut store = DocumentStore::default();
        store.open(doc_path).unwrap();
        assert_eq!(store.preferred_device_serial(), None);
        assert!(!store.v070_auto_layer_sync_enabled());
        assert!(!store.v070_session_sa_rgb_sync_enabled());
    }

    #[test]
    fn v070_session_sa_rgb_sync_persists_and_defaults_false_on_v1_migration() {
        let dir = tempfile::tempdir().unwrap();
        let doc_path = dir.path().join("workbench.json");
        let prefs_path = dir.path().join("workbench.preferences.json");
        fs::write(
            &prefs_path,
            r#"{"schema_version":1,"favorites":[],"hardware_profile_slots":[null,null,null,null],"active_hardware_slot":0}"#,
        )
        .unwrap();

        let mut store = DocumentStore::default();
        store.open(doc_path.clone()).unwrap();
        assert!(!store.v070_session_sa_rgb_sync_enabled());
        assert!(store.set_v070_session_sa_rgb_sync_enabled(true).unwrap());
        assert!(store.v070_session_sa_rgb_sync_enabled());
        assert!(!store.set_v070_session_sa_rgb_sync_enabled(true).unwrap());

        let mut reopened = DocumentStore::default();
        reopened.open(doc_path).unwrap();
        assert!(reopened.v070_session_sa_rgb_sync_enabled());
        assert!(reopened
            .set_v070_session_sa_rgb_sync_enabled(false)
            .unwrap());
        assert!(!reopened.v070_session_sa_rgb_sync_enabled());
    }

    #[test]
    fn preferred_device_serial_and_auto_layer_sync_persist_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let doc_path = dir.path().join("workbench.json");
        let prefs_path = dir.path().join("workbench.preferences.json");

        let mut store = DocumentStore::default();
        store.open(doc_path.clone()).unwrap();
        assert_eq!(store.preferred_device_serial(), None);
        assert!(!store.v070_auto_layer_sync_enabled());

        assert!(!store
            .set_preferred_device_serial(Some("   ".into()))
            .unwrap());
        assert!(store
            .set_preferred_device_serial(Some("  CM2-SERIAL-01  ".into()))
            .unwrap());
        assert_eq!(
            store.preferred_device_serial().as_deref(),
            Some("CM2-SERIAL-01")
        );
        assert!(!store
            .set_preferred_device_serial(Some("CM2-SERIAL-01".into()))
            .unwrap());

        assert!(store.set_v070_auto_layer_sync_enabled(true).unwrap());
        assert!(store.v070_auto_layer_sync_enabled());
        assert!(!store.set_v070_auto_layer_sync_enabled(true).unwrap());

        let mut reopened = DocumentStore::default();
        reopened.open(doc_path.clone()).unwrap();
        assert_eq!(
            reopened.preferred_device_serial().as_deref(),
            Some("CM2-SERIAL-01")
        );
        assert!(reopened.v070_auto_layer_sync_enabled());

        assert!(reopened
            .set_preferred_device_serial(Some(" \t ".into()))
            .unwrap());
        assert_eq!(reopened.preferred_device_serial(), None);
        assert!(reopened.set_v070_auto_layer_sync_enabled(false).unwrap());
        assert!(!reopened.v070_auto_layer_sync_enabled());

        let saved_json = fs::read_to_string(&prefs_path).unwrap();
        assert!(!saved_json.contains("preferred_device_serial"));
    }
}
