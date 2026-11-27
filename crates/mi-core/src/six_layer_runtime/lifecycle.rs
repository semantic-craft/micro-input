use super::state::{save, ExecutionPolicy, Installation, InstallationStatus, SavedSixLayerState};
use super::vendor_bridge::VendorBridge;
use super::{RoutingContext, RunningSixLayer};
use crate::karabiner::{self, ConfigPatch, Readiness, ReloadBaseline};
use mi_model::apply::AppliedSixLayerPlan;
use mi_session::DeviceSession;
use mi_transport::{
    budget::DeviceBudgetRegistry, HidCandidate, HidTransport, MacTransport, TargetKeyboard,
    UserCommandReceiver,
};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU8},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

pub(super) fn connect(
    transport: &dyn HidTransport,
    target: &TargetKeyboard,
    budget_registry: &DeviceBudgetRegistry,
) -> Result<(Arc<DeviceSession>, HidCandidate), String> {
    let candidates = transport.enumerate().map_err(|e| e.to_string())?;
    let mut candidate = candidates
        .into_iter()
        .find(|candidate| {
            candidate.path == target.vendor_path
                && mi_transport::is_exclusive_vendor_candidate(candidate)
        })
        .ok_or("已确认的专用接口已消失")?;
    if candidate.location_id.is_none() && target.location_id != 0 {
        candidate.location_id = Some(target.location_id);
    }
    let link = transport.open(&candidate.path).map_err(|e| e.to_string())?;
    if !link.is_exclusive_vendor() {
        link.close();
        return Err("未取得 USB 专用接口独占".into());
    }
    let budget = budget_registry.budget_for_candidate(&candidate);
    let session = DeviceSession::new_with_budget(
        link,
        Box::new(mi_proto::NativeFramer::default()),
        Arc::new(mi_session::ReadOnlySchema),
        mi_session::SessionConfig::default(),
        budget,
    );
    Ok((session, candidate))
}

pub(super) fn prepare_apply_karabiner_patch(
    saved: &SavedSixLayerState,
    target: &TargetKeyboard,
    receiver: &UserCommandReceiver,
    layout: mi_model::host_router::BottomRowLayout,
) -> Result<ConfigPatch, String> {
    let rule = karabiner::rules(target, receiver, layout)?;
    let path = match &saved.installation {
        Some(prev) => prev.config.path.clone(),
        None => karabiner::config_path()?,
    };
    let patch = karabiner::prepare_config(
        path.clone(),
        fs::read(&path).map_err(|e| e.to_string())?,
        target,
        rule,
    )?;
    if let Some(prev) = &saved.installation {
        if patch.profile_name != prev.config.profile_name {
            return Err("原 Karabiner 方案身份已变化；请先恢复原键盘配置后再应用".into());
        }
    }
    Ok(patch)
}

fn read_preferred_serial(document_dir: &Path) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct PreferredSerialHeader {
        #[serde(default)]
        preferred_device_serial: Option<String>,
    }
    let bytes = fs::read(document_dir.join("workbench.preferences.json")).ok()?;
    let header: PreferredSerialHeader = serde_json::from_slice(&bytes).ok()?;
    header
        .preferred_device_serial
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

pub async fn apply(
    document_dir: &Path,
    plan: AppliedSixLayerPlan,
    mut saved: SavedSixLayerState,
    transport: Arc<MacTransport>,
    budget_registry: Arc<DeviceBudgetRegistry>,
) -> Result<RunningSixLayer, String> {
    let state_path = document_dir.join("six-layer-runtime.json");
    plan.validate_current()?;
    let preferred_serial = read_preferred_serial(document_dir);
    let target = mi_transport::discover_target_keyboard_preferred(preferred_serial.as_deref())
        .map_err(|e| e.to_string())?;
    let receiver = Arc::new(UserCommandReceiver::bind_default().map_err(|e| e.to_string())?);
    let patch = prepare_apply_karabiner_patch(&saved, &target, &receiver, plan.bottom_row_layout)?;
    let (session, candidate) = connect(transport.as_ref(), &target, budget_registry.as_ref())?;
    let layer_modes: [Option<String>; 6] = plan
        .board_layer_joystick_modes()
        .map(|mode| mode.map(str::to_owned));
    let outcome: Result<RunningSixLayer, String> = async {
        let prepared = session
            .prepare_six_layer_write_with_layer_modes(
                document_dir,
                Some(layer_modes.each_ref().map(|mode| mode.as_deref())),
            )
            .await
            .map_err(|e| e.to_string())?;
        let candidate_bytes = prepared.candidate.clone();
        let smart = match fs::read(prepared.backup.join("smart_actions.json")) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.to_string()),
        };
        let baseline = ReloadBaseline::capture()?;
        patch.install(&document_dir.join("karabiner-backups"))?;
        let readiness = match wait_ready(&baseline, &patch, &target) {
            Ok(ready) => ready,
            Err(error) => {
                let rollback = patch.rollback_exact();
                return Err(format!(
                    "{error}；规则恢复：{}",
                    rollback.map(|_| "已确认".into()).unwrap_or_else(|e| e)
                ));
            }
        };
        let previous = saved.clone();
        let mut restoration_config = patch.clone();
        if let Some(prev) = &previous.installation {
            restoration_config.original = prev.config.original.clone();
            restoration_config.original_device = prev.config.original_device.clone();
            if let Err(error) = merge_previous_original_into_backup(&prev.backup, &prepared.backup)
            {
                let _ = patch.rollback_exact();
                let _ = fs::remove_dir_all(&prepared.backup);
                return Err(error);
            }
        }
        let backup = prepared.backup.clone();
        let mut installation = Installation {
            status: InstallationStatus::Applying,
            plan,
            target: target.clone(),
            profile_index: prepared.profile_index,
            backup,
            config: restoration_config,
        };
        saved.installation = Some(installation.clone());
        if let Err(error) = save(&state_path, &saved) {
            let _ = patch.rollback_exact();
            if previous
                .installation
                .as_ref()
                .is_none_or(|prev| prev.backup != installation.backup)
            {
                let _ = fs::remove_dir_all(&installation.backup);
            }
            return Err(error);
        }
        let result = session.apply_six_layer_write(prepared).await;
        let receipt = match result {
            Ok(receipt) => receipt,
            Err(error) => {
                if error.original_verified {
                    let rollback = patch.rollback_exact();
                    if rollback.is_ok()
                        && save(&state_path, &previous).is_ok()
                        && previous
                            .installation
                            .as_ref()
                            .is_none_or(|prev| prev.backup != installation.backup)
                    {
                        let _ = fs::remove_dir_all(&installation.backup);
                    }
                    return Err(format!(
                        "{}；设备原件已确认；规则恢复：{}",
                        error.detail,
                        rollback.map(|_| "已确认".into()).unwrap_or_else(|e| e)
                    ));
                }
                if let Some(err_backup) = error.backup {
                    let merged_ok = previous.installation.as_ref().is_none_or(|prev| {
                        merge_previous_original_into_backup(&prev.backup, &err_backup).is_ok()
                    });
                    if merged_ok {
                        installation.backup = err_backup;
                    }
                }
                installation.status = InstallationStatus::RecoveryRequired;
                saved.installation = Some(installation);
                let _ = save(&state_path, &saved);
                return Err(format!(
                    "{}；设备恢复未确认，保留吞键规则，停止派发",
                    error.detail
                ));
            }
        };
        installation.backup = receipt.backup.clone();
        installation.status = InstallationStatus::RecoveryRequired;
        saved.installation = Some(installation.clone());
        save(&state_path, &saved)?;
        let layer = session
            .verify_six_layer_files(&candidate_bytes, smart.as_deref(), receipt.profile_index)
            .await
            .map_err(|e| format!("{e}；保留吞键规则，请恢复设备"))?;
        readiness.recheck_preferred(&patch, &target, candidate.serial_number.as_deref())?;
        installation.status = InstallationStatus::Applied;
        saved.installation = Some(installation.clone());
        save(&state_path, &saved)?;
        if let Some(prev) = &previous.installation {
            if prev.backup != installation.backup {
                let _ = fs::remove_dir_all(&prev.backup);
            }
        }
        let mut running = new_running(
            receiver,
            readiness,
            Some(session.clone()),
            candidate,
            installation,
            layer as u8,
        );
        running.transport = Some(transport);
        running.budget_registry = budget_registry;
        Ok(running)
    }
    .await;
    if outcome.is_err() {
        session.shutdown().await;
    }
    outcome
}

pub(crate) fn merge_previous_original_into_backup(
    previous_backup: &Path,
    new_backup: &Path,
) -> Result<(), String> {
    if previous_backup == new_backup {
        return Ok(());
    }
    let prev_keymap = previous_backup.join("keymap.json");
    if !prev_keymap.exists() {
        return Ok(());
    }
    let pre_write_path = new_backup.join("pre-write-keymap.json");
    let new_keymap_path = new_backup.join("keymap.json");
    let pre_write_bytes = if pre_write_path.exists() {
        Some(fs::read(&pre_write_path).map_err(|e| e.to_string())?)
    } else if new_keymap_path.exists() {
        let bytes = fs::read(&new_keymap_path).map_err(|e| e.to_string())?;
        write_private_backup_file(&pre_write_path, &bytes)?;
        Some(bytes)
    } else {
        None
    };
    let original_keymap = fs::read(&prev_keymap).map_err(|e| e.to_string())?;
    write_private_backup_file(&new_keymap_path, &original_keymap)?;
    let prev_planned_path = previous_backup.join("planned-keymap.json");
    let prev_planned = if prev_planned_path.exists() {
        let bytes = fs::read(&prev_planned_path).map_err(|e| e.to_string())?;
        write_private_backup_file(&new_backup.join("previous-planned-keymap.json"), &bytes)?;
        Some(bytes)
    } else {
        None
    };
    let prev_manifest = fs::read(previous_backup.join("manifest.json"))
        .ok()
        .and_then(|raw| serde_json::from_slice::<serde_json::Value>(&raw).ok());
    let manifest_path = new_backup.join("manifest.json");
    if manifest_path.exists() {
        let raw_manifest = fs::read(&manifest_path).map_err(|e| e.to_string())?;
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&raw_manifest).map_err(|e| e.to_string())?;
        manifest["keymap_sha256"] =
            serde_json::Value::String(crate::lighting_arbiter::sha256_hex(&original_keymap));
        if let Some(pre_write) = &pre_write_bytes {
            manifest["pre_write_keymap_sha256"] =
                serde_json::Value::String(crate::lighting_arbiter::sha256_hex(pre_write));
        }
        if let Some(prev_manifest) = &prev_manifest {
            if let Some(prev_profile) = prev_manifest.get("previous_profile_index") {
                manifest["previous_profile_index"] = prev_profile.clone();
            }
            if let Some(prev_layer) = prev_manifest.get("previous_layer_index") {
                manifest["previous_layer_index"] = prev_layer.clone();
            }
        }
        if let Some(prev_planned) = &prev_planned {
            manifest["previous_planned_keymap_sha256"] =
                serde_json::Value::String(crate::lighting_arbiter::sha256_hex(prev_planned));
        }
        let updated = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
        write_private_backup_file(&manifest_path, &updated)?;
    }
    if let Ok(dir) = fs::File::open(new_backup) {
        let _ = dir.sync_all();
    }
    Ok(())
}

fn write_private_backup_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())
}

pub(super) fn new_running(
    receiver: Arc<UserCommandReceiver>,
    readiness: Readiness,
    initial_session: Option<Arc<DeviceSession>>,
    candidate: HidCandidate,
    installation: Installation,
    layer: u8,
) -> RunningSixLayer {
    let firmware_version = initial_session
        .as_ref()
        .and_then(|s| s.observed_firmware_version());
    let observed_layer = Arc::new(AtomicU8::new(layer));
    let mut vendor_bridge = VendorBridge::new(initial_session);
    vendor_bridge.observed_layer = Arc::clone(&observed_layer);
    RunningSixLayer {
        receiver,
        readiness,
        candidate,
        installation,
        observed_layer,
        first_event_verified: Arc::new(AtomicBool::new(false)),
        armed: false,
        execution_policy: Arc::new(ExecutionPolicy::default()),
        receipts: None,
        routing: Arc::new(Mutex::new(RoutingContext::default())),
        navigation: None,
        vendor_bridge,
        firmware_version,
        transport: None,
        budget_registry: Arc::new(DeviceBudgetRegistry::new()),
        stop: Arc::new(AtomicBool::new(false)),
        monitor: None,
        executor: None,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ResumeSnapshotAction {
    VerifyReady { layer: u8 },
    RequiresExplicitApply,
}

pub(crate) async fn inspect_resume_snapshot(
    session: &DeviceSession,
    expected: &[u8],
    smart: Option<&[u8]>,
    profile_index: u32,
) -> Result<ResumeSnapshotAction, String> {
    if mi_model::apply::recognize_six_layer_profile(expected, smart) == Ok(Some(profile_index)) {
        let layer = session
            .verify_six_layer_files(expected, smart, profile_index)
            .await
            .map_err(|e| format!("{e}；未写设备，请显式应用或恢复"))?;
        Ok(ResumeSnapshotAction::VerifyReady { layer: layer as u8 })
    } else {
        let compiled = mi_model::apply::compile_six_layer_profile(expected, smart)
            .map_err(|e| format!("{e}；未写设备，请显式应用或恢复"))?;
        if compiled.profile_index != profile_index {
            return Err("六层活动方案索引不符；未写设备，请显式应用或恢复".into());
        }
        match session
            .matches_six_layer_snapshot(expected, smart, profile_index)
            .await
        {
            Ok(mi_session::SixLayerSnapshotCheck::Matched) => {
                Ok(ResumeSnapshotAction::RequiresExplicitApply)
            }
            Ok(mi_session::SixLayerSnapshotCheck::FilesDiverged) => Err(
                "旧版六层快照与当前设备不符，未自动覆盖；请显式点击「应用到键盘」或「恢复原键盘配置」"
                    .into(),
            ),
            Ok(mi_session::SixLayerSnapshotCheck::StatusNotReady) => {
                Err("六层固件、方案身份或层位未就绪；未写设备，请显式应用或恢复".into())
            }
            Err(e) => Err(format!("{e}；未写设备，请显式应用或恢复")),
        }
    }
}

pub async fn resume(
    document_dir: &Path,
    mut saved: SavedSixLayerState,
    transport: Arc<MacTransport>,
    budget_registry: Arc<DeviceBudgetRegistry>,
) -> Result<RunningSixLayer, String> {
    let mut installation = saved
        .installation
        .clone()
        .filter(|installation| installation.status == InstallationStatus::Applied)
        .ok_or("六层记录未完成，需要恢复")?;
    installation.plan.validate_current()?;
    let expected =
        fs::read(installation.backup.join("planned-keymap.json")).map_err(|e| e.to_string())?;
    let smart = match fs::read(installation.backup.join("smart_actions.json")) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.to_string()),
    };
    let preferred_serial = read_preferred_serial(document_dir);
    let target = mi_transport::discover_target_keyboard_preferred(preferred_serial.as_deref())
        .map_err(|e| e.to_string())?;
    let (session, candidate) = connect(transport.as_ref(), &target, budget_registry.as_ref())?;
    let layer = match inspect_resume_snapshot(
        &session,
        &expected,
        smart.as_deref(),
        installation.profile_index,
    )
    .await
    {
        Ok(ResumeSnapshotAction::VerifyReady { layer }) => layer,
        Ok(ResumeSnapshotAction::RequiresExplicitApply) => {
            session.shutdown().await;
            return Err("旧版六层布局需要显式应用；重连未写设备".into());
        }
        Err(error) => {
            session.shutdown().await;
            return Err(error);
        }
    };
    if !mi_model::apply::planned_keymap_matches_plan_joystick_modes(
        &expected,
        installation.profile_index,
        &installation.plan,
        session.observed_firmware_version().as_deref(),
    ) {
        session.shutdown().await;
        return Err("旧版六层布局需要显式应用；重连未写设备".into());
    }
    let outcome: Result<RunningSixLayer, String> = async {
        let receiver = Arc::new(UserCommandReceiver::bind_default().map_err(|e| e.to_string())?);
        let rule = karabiner::rules(&target, &receiver, installation.plan.bottom_row_layout)?;
        let path = installation.config.path.clone();
        let patch = karabiner::prepare_config(
            path.clone(),
            fs::read(&path).map_err(|e| e.to_string())?,
            &target,
            rule,
        )?;
        let baseline = ReloadBaseline::capture()?;
        let readiness = install_resume_patch_with(
            document_dir,
            &mut saved,
            &mut installation,
            &target,
            &patch,
            |patch, target| wait_ready(&baseline, patch, target),
        )?;
        let mut running = new_running(
            receiver,
            readiness,
            Some(session.clone()),
            candidate,
            installation,
            layer,
        );
        running.transport = Some(transport);
        running.budget_registry = budget_registry;
        Ok(running)
    }
    .await;
    if outcome.is_err() {
        session.shutdown().await;
    }
    outcome
}

pub(super) async fn with_verification_session<T>(
    session: &DeviceSession,
    operation: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, String> {
    let result = operation.await;
    session.shutdown().await;
    result
}

pub(super) fn install_resume_patch_with<F>(
    document_dir: &Path,
    saved: &mut SavedSixLayerState,
    installation: &mut Installation,
    target: &TargetKeyboard,
    patch: &ConfigPatch,
    wait_ready_fn: F,
) -> Result<Readiness, String>
where
    F: FnOnce(&ConfigPatch, &TargetKeyboard) -> Result<Readiness, String>,
{
    patch.install(&document_dir.join("karabiner-backups"))?;
    let readiness = match wait_ready_fn(patch, target) {
        Ok(ready) => ready,
        Err(error) => {
            let _ = patch.rollback_exact();
            return Err(error);
        }
    };
    let mut next_installation = installation.clone();
    next_installation.config.installed = patch.installed.clone();
    next_installation.config.installed_device = patch.installed_device.clone();
    next_installation.target = target.clone();
    let mut next_saved = saved.clone();
    next_saved.installation = Some(next_installation.clone());
    if let Err(error) = save(&document_dir.join("six-layer-runtime.json"), &next_saved) {
        let _ = patch.rollback_exact();
        return Err(error);
    }
    *installation = next_installation;
    *saved = next_saved;
    Ok(readiness)
}

fn wait_ready(
    baseline: &ReloadBaseline,
    patch: &ConfigPatch,
    target: &TargetKeyboard,
) -> Result<Readiness, String> {
    let start = Instant::now();
    loop {
        match baseline.check(patch, target) {
            Ok(ready) => return Ok(ready),
            Err(error) if start.elapsed() > Duration::from_secs(8) => return Err(error),
            Err(_) => {}
        }
        thread::sleep(Duration::from_millis(100));
    }
}

pub(crate) fn validate_backup_within_document_dir(
    document_dir: &Path,
    backup: &Path,
) -> Result<std::path::PathBuf, String> {
    let verified_doc = mi_model::private_fs::verify_existing_private_dir(document_dir)
        .map_err(|err| format!("文档目录不安全或不可用：{err:?}"))?;
    let verified_backup = mi_model::private_fs::verify_existing_private_dir(backup)
        .map_err(|err| format!("六层备份目录不安全或不可用：{err:?}"))?;
    if !verified_backup.starts_with(&verified_doc) || verified_backup == verified_doc {
        return Err("六层备份路径超出当前文档目录，拒绝恢复".into());
    }
    Ok(verified_backup)
}

pub async fn restore(
    document_dir: &Path,
    mut saved: SavedSixLayerState,
    transport: Arc<MacTransport>,
    budget_registry: Arc<DeviceBudgetRegistry>,
) -> Result<(), String> {
    let installation = saved.installation.clone().ok_or("没有可恢复的六层记录")?;
    let verified_backup = validate_backup_within_document_dir(document_dir, &installation.backup)?;
    let preferred_serial = read_preferred_serial(document_dir);
    let target = mi_transport::discover_target_keyboard_preferred(preferred_serial.as_deref())
        .map_err(|e| e.to_string())?;
    if target.location_id != installation.target.location_id {
        return Err("键盘 USB 位置已变化，未自动恢复".into());
    }
    let (session, _) = connect(transport.as_ref(), &target, budget_registry.as_ref())?;
    with_verification_session(&session, async {
        session
            .restore_six_layer_backup(&verified_backup)
            .await
            .map_err(|e| format!("{e}；保留吞键规则"))?;
        installation.config.restore_owned(&target)?;
        saved.installation = None;
        save(&document_dir.join("six-layer-runtime.json"), &saved)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::six_layer_runtime::state::load;
    use mi_model::board_layers::BoardLayerPlan;
    use mi_model::host_router::BottomRowLayout;

    fn sample_plan() -> AppliedSixLayerPlan {
        let mut draft = mi_model::draft::Draft::default();
        mi_model::preset::install_agent_starter_profiles(&mut draft).unwrap();
        let id = |name: &str| {
            draft
                .document
                .editable
                .graph
                .profiles
                .iter()
                .find(|(_, profile)| profile.name == name)
                .unwrap()
                .0
                .clone()
        };
        let desired =
            BoardLayerPlan::recommended(id("Codex"), id("Antigravity"), id("DeepSeek Coder (DSH)"));
        AppliedSixLayerPlan::compile(&desired, &draft.document, BottomRowLayout::Split, true)
            .unwrap()
    }

    fn sample_target() -> TargetKeyboard {
        TargetKeyboard {
            vendor_id: 0x303a,
            product_id: 0x8298,
            location_id: 42,
            usb_parent: 8,
            vendor_path: "synthetic-vendor".into(),
        }
    }

    #[test]
    fn resume_rolls_back_karabiner_and_preserves_saved_state_when_wait_ready_fails() {
        let dir = tempfile::tempdir().unwrap();
        let karabiner_path = dir.path().join("karabiner.json");
        let original = br#"{"profiles":[{"name":"Default","selected":true,"devices":[],"complex_modifications":{"rules":[]}}]}"#.to_vec();
        fs::write(&karabiner_path, &original).unwrap();

        let target = sample_target();
        let prev_receiver = UserCommandReceiver::bind().unwrap();
        let prev_rule = karabiner::rules(&target, &prev_receiver, BottomRowLayout::Split).unwrap();
        let prev_patch =
            karabiner::prepare_config(karabiner_path.clone(), original.clone(), &target, prev_rule)
                .unwrap();

        let mut installation = Installation {
            status: InstallationStatus::Applied,
            plan: sample_plan(),
            target: target.clone(),
            profile_index: 0,
            backup: dir.path().join("backup"),
            config: prev_patch.clone(),
        };
        let mut saved = SavedSixLayerState {
            version: 1,
            legacy_native_paused_confirmation: None,
            installation: Some(installation.clone()),
        };
        let state_path = dir.path().join("six-layer-runtime.json");
        save(&state_path, &saved).unwrap();

        let next_receiver = UserCommandReceiver::bind().unwrap();
        let next_rule = karabiner::rules(&target, &next_receiver, BottomRowLayout::Split).unwrap();
        let next_patch = karabiner::prepare_config(
            karabiner_path.clone(),
            fs::read(&karabiner_path).unwrap(),
            &target,
            next_rule,
        )
        .unwrap();

        let err = install_resume_patch_with(
            dir.path(),
            &mut saved,
            &mut installation,
            &target,
            &next_patch,
            |_, _| Err("readiness timeout".into()),
        )
        .unwrap_err();
        assert_eq!(err, "readiness timeout");
        assert_eq!(fs::read(&karabiner_path).unwrap(), original);
        assert_eq!(installation.config.installed, prev_patch.installed);
        let persisted = load(&state_path).unwrap();
        assert_eq!(
            persisted.installation.unwrap().config.installed,
            prev_patch.installed
        );

        install_resume_patch_with(
            dir.path(),
            &mut saved,
            &mut installation,
            &target,
            &next_patch,
            |_, _| Ok(Readiness::isolated()),
        )
        .unwrap();
        assert_eq!(fs::read(&karabiner_path).unwrap(), next_patch.installed);
        assert_eq!(installation.config.installed, next_patch.installed);
        let persisted_after = load(&state_path).unwrap();
        assert_eq!(
            persisted_after.installation.unwrap().config.installed,
            next_patch.installed
        );
    }
}
