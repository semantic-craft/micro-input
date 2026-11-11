use crate::karabiner::{self, ConfigPatch, Readiness, ReloadBaseline};
use mi_model::apply::AppliedSixLayerPlan;
use mi_model::{
    direct_controls::{CONTROL_BINDINGS, DIRECT_CONTROLS},
    host_router::ResolvedHostAction,
};
use mi_session::DeviceSession;
use mi_transport::{HidCandidate, HidTransport, MacTransport, TargetKeyboard, UserCommandReceiver};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum InstallationStatus {
    Applying,
    Applied,
    RecoveryRequired,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Installation {
    pub status: InstallationStatus,
    pub plan: AppliedSixLayerPlan,
    pub target: TargetKeyboard,
    pub profile_index: u32,
    pub backup: PathBuf,
    pub config: ConfigPatch,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedSixLayerState {
    pub version: u8,
    #[serde(
        default,
        rename = "cindy_native_paused_confirmed",
        skip_serializing_if = "Option::is_none"
    )]
    legacy_native_paused_confirmation: Option<bool>,
    pub installation: Option<Installation>,
}

impl Default for SavedSixLayerState {
    fn default() -> Self {
        Self {
            version: 1,
            legacy_native_paused_confirmation: None,
            installation: None,
        }
    }
}

pub fn load(path: &Path) -> Result<SavedSixLayerState, String> {
    match fs::read(path) {
        Ok(bytes) => {
            let saved: SavedSixLayerState =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if saved.version != 1 {
                return Err("不支持此六层运行记录版本".into());
            }
            Ok(saved)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(SavedSixLayerState::default())
        }
        Err(error) => Err(error.to_string()),
    }
}

pub fn save(path: &Path, state: &SavedSixLayerState) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?;
    if let Ok(expected) = fs::read(path) {
        return karabiner::replace_if_unchanged(path, &expected, &bytes);
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        fs::File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[derive(Default)]
struct ExecutionPolicy {
    state: Mutex<ExecutionPolicyState>,
}

#[derive(Default)]
struct ExecutionPolicyState {
    enabled: bool,
    commands: bool,
    epoch: u64,
    admit_after: Option<Instant>,
}

#[derive(Clone, Copy)]
struct ExecutionAdmission {
    epoch: u64,
    received_at: Instant,
}

impl ExecutionPolicy {
    fn set(&self, enabled: bool, commands: bool, now: Instant) {
        let mut state = self.state.lock().unwrap();
        if state.enabled != enabled || state.commands != commands {
            state.enabled = enabled;
            state.commands = commands;
            state.epoch += 1;
            state.admit_after = Some(now);
        }
    }

    fn capture(&self, received_at: Instant) -> ExecutionAdmission {
        ExecutionAdmission {
            epoch: self.state.lock().unwrap().epoch,
            received_at,
        }
    }

    fn admits(&self, admission: ExecutionAdmission, action: &ResolvedHostAction) -> bool {
        let state = self.state.lock().unwrap();
        state.epoch == admission.epoch
            && state.enabled
            && state
                .admit_after
                .is_none_or(|after| admission.received_at >= after)
            && (!matches!(action, ResolvedHostAction::ShellCommand { .. }) || state.commands)
    }
}

pub struct RunningSixLayer {
    pub receiver: Arc<UserCommandReceiver>,
    pub readiness: Readiness,
    session: Arc<DeviceSession>,
    pub candidate: HidCandidate,
    pub installation: Installation,
    pub observed_layer: Arc<AtomicU8>,
    pub first_event_verified: Arc<AtomicBool>,
    pub armed: bool,
    execution_policy: Arc<ExecutionPolicy>,
    pub receipts: Option<std::sync::mpsc::Receiver<mi_macos::HostActionReceipt>>,
    foreground: Arc<Mutex<ForegroundContext>>,
    stop: Arc<AtomicBool>,
    monitor: Option<JoinHandle<()>>,
    executor: Option<JoinHandle<Result<(), String>>>,
}

#[derive(Clone, Debug)]
struct ForegroundContext {
    bundle_id: Option<String>,
    generation: u64,
    changed_at: Instant,
}

impl Default for ForegroundContext {
    fn default() -> Self {
        Self {
            bundle_id: None,
            generation: 0,
            changed_at: Instant::now(),
        }
    }
}

impl ForegroundContext {
    fn update(&mut self, bundle_id: Option<&str>) -> bool {
        if self.bundle_id.as_deref() == bundle_id {
            return false;
        }
        self.bundle_id = bundle_id.map(str::to_owned);
        self.generation += 1;
        self.changed_at = Instant::now();
        true
    }

    fn host_index(&self, plan: &AppliedSixLayerPlan, physical_layer: u8) -> usize {
        self.bundle_id
            .as_deref()
            .and_then(|bundle| {
                plan.hosts.iter().position(|host| {
                    mi_macos::executor::bundle_id_for_adapter(&host.adapter) == Some(bundle)
                })
            })
            .unwrap_or((physical_layer - 2) as usize)
    }
}

impl RunningSixLayer {
    pub fn foreground_changed(&self, bundle_id: Option<&str>) -> bool {
        self.foreground.lock().unwrap().update(bundle_id)
    }

    pub fn effective_profile_id(&self) -> Option<String> {
        let layer = self.observed_layer.load(Ordering::Acquire);
        if !(2..=3).contains(&layer) {
            return None;
        }
        let foreground = self.foreground.lock().unwrap();
        Some(
            self.installation.plan.hosts[foreground.host_index(&self.installation.plan, layer)]
                .profile_id
                .clone(),
        )
    }

    #[cfg(test)]
    pub(crate) fn isolated(session: Arc<DeviceSession>, directory: &Path) -> Self {
        let mut draft = mi_model::draft::Draft::default();
        mi_model::preset::install_agent_starter_profiles(&mut draft).unwrap();
        let id = |name: &str| {
            draft
                .document
                .editable
                .graph
                .profiles
                .iter()
                .find(|(_, p)| p.name == name)
                .unwrap()
                .0
                .clone()
        };
        let desired = mi_model::board_layers::BoardLayerPlan::recommended(
            id("Codex"),
            id("Antigravity"),
            id("DeepSeek Coder (DSH)"),
        );
        let plan = AppliedSixLayerPlan::compile(
            &desired,
            &draft.document,
            mi_model::host_router::BottomRowLayout::Split,
            true,
        )
        .unwrap();
        let candidate = HidCandidate {
            path: "isolated".into(),
            vid: 0x303a,
            pid: 0x8298,
            product: "Synthetic".into(),
            usage_page: 0xff60,
            usage: 0x61,
            usage_pairs: vec![],
            transport_hint: mi_transport::TransportHint::Usb,
        };
        new_running(
            Arc::new(UserCommandReceiver::bind().unwrap()),
            Readiness::isolated(),
            session,
            candidate,
            Installation {
                status: InstallationStatus::Applied,
                plan,
                target: TargetKeyboard {
                    vendor_id: 0x303a,
                    product_id: 0x8298,
                    location_id: 0,
                    usb_parent: 0,
                    vendor_path: "isolated".into(),
                },
                profile_index: 0,
                backup: directory.join("backup"),
                config: ConfigPatch {
                    path: directory.join("karabiner.json"),
                    original: vec![],
                    installed: vec![],
                    profile_name: "isolated".into(),
                    original_device: None,
                    installed_device: serde_json::Value::Null,
                },
            },
            1,
        )
    }

    pub fn set_execution_policy(&self, enabled: bool, commands: bool, now: Instant) {
        self.execution_policy.set(enabled, commands, now);
    }

    #[cfg(test)]
    pub(crate) fn is_execution_enabled(&self) -> bool {
        self.execution_policy.state.lock().unwrap().enabled
    }

    pub fn start_executor<D: mi_macos::HostOsDriver + 'static>(
        &mut self,
        executor: Arc<mi_macos::LiveHostActionExecutor<D>>,
    ) {
        let receiver = Arc::clone(&self.receiver);
        let plan = self.installation.plan.clone();
        let stop = Arc::clone(&self.stop);
        let policy = Arc::clone(&self.execution_policy);
        let observed = Arc::clone(&self.observed_layer);
        let first = Arc::clone(&self.first_event_verified);
        let foreground = Arc::clone(&self.foreground);
        let (tx, rx) = std::sync::mpsc::sync_channel(32);
        self.receipts = Some(rx);
        self.executor = Some(thread::spawn(move || {
            execute_clicks(
                &receiver,
                &plan,
                &stop,
                &policy,
                &observed,
                &first,
                &foreground,
                &tx,
                || receiver.next(),
                || receiver.failure().map_or(Ok(()), Err),
                |profile, binding, action, admission| {
                    executor.execute_when(profile, binding, action, || {
                        !stop.load(Ordering::Acquire)
                            && receiver.failure().is_none()
                            && policy.admits(admission, action)
                    })
                },
            );
            Ok(())
        }));
    }

    pub fn retire(&self) {
        self.receiver.invalidate("运行代次已结束");
        self.stop.store(true, Ordering::Release);
        self.session.disconnect();
    }

    pub fn is_retired(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }

    pub fn shutdown(mut self, runtime: &tokio::runtime::Runtime) -> Result<(), String> {
        self.quiesce(runtime)
    }

    pub fn quiesce(&mut self, runtime: &tokio::runtime::Runtime) -> Result<(), String> {
        self.retire();
        runtime.block_on(self.session.shutdown());
        let monitor_result = self.monitor.take().map_or(Ok(()), |monitor| {
            monitor.join().map_err(|_| "设备监视清理失败".to_string())
        });
        let executor_result = self.executor.take().map_or(Ok(()), |executor| {
            executor
                .join()
                .map_err(|_| "宿主动作清理失败".to_string())
                .and_then(|result| result)
        });
        monitor_result.and(executor_result)
    }

    pub fn start_monitor(&mut self) {
        let receiver = Arc::clone(&self.receiver);
        let stop = Arc::clone(&self.stop);
        let readiness = self.readiness.clone();
        let patch = self.installation.config.clone();
        let target = self.installation.target.clone();
        self.monitor = Some(thread::spawn(move || {
            let mut wall = SystemTime::now();
            let mut monotonic = Instant::now();
            while !stop.load(Ordering::Acquire) {
                for _ in 0..10 {
                    if stop.load(Ordering::Acquire) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(50));
                }
                let now_wall = SystemTime::now();
                let now_monotonic = Instant::now();
                let slept = now_wall
                    .duration_since(wall)
                    .unwrap_or_default()
                    .saturating_sub(now_monotonic.duration_since(monotonic))
                    > Duration::from_secs(1);
                wall = now_wall;
                monotonic = now_monotonic;
                if !check_input_connection(&receiver, slept, || readiness.recheck(&patch, &target))
                {
                    return;
                }
            }
        }));
    }
}

impl Drop for RunningSixLayer {
    fn drop(&mut self) {
        self.retire();
        // The monitor only owns immutable evidence and its old receiver, so it
        // may finish a bounded CLI check without delaying the workbench thread.
        self.monitor.take();
        self.executor.take();
    }
}

// Input liveness belongs to the grabbed keyboard and its Karabiner generation.
// Vendor RPC is used only while establishing or changing the configuration.
fn check_input_connection(
    receiver: &UserCommandReceiver,
    slept: bool,
    recheck: impl FnOnce() -> Result<(), String>,
) -> bool {
    let result = if slept {
        Err("检测到系统睡眠，六层输入已暂停".into())
    } else {
        recheck()
    };
    if let Err(error) = result {
        receiver.invalidate(&error);
        false
    } else {
        true
    }
}

fn execute_clicks(
    receiver: &UserCommandReceiver,
    plan: &AppliedSixLayerPlan,
    stop: &AtomicBool,
    policy: &ExecutionPolicy,
    observed: &AtomicU8,
    first: &AtomicBool,
    foreground: &Mutex<ForegroundContext>,
    receipts: &std::sync::mpsc::SyncSender<mi_macos::HostActionReceipt>,
    mut next: impl FnMut() -> Result<Option<mi_transport::Activation>, String>,
    mut check_connection: impl FnMut() -> Result<(), String>,
    mut execute: impl FnMut(
        &str,
        &str,
        &ResolvedHostAction,
        ExecutionAdmission,
    ) -> mi_macos::HostActionReceipt,
) {
    while !stop.load(Ordering::Acquire) {
        let event = match next() {
            Ok(Some(event)) => event,
            Ok(None) => {
                thread::sleep(Duration::from_millis(5));
                continue;
            }
            Err(_) => return,
        };
        let admission = policy.capture(event.received_at);
        if event.generation != receiver.generation() {
            continue;
        }
        let control = &DIRECT_CONTROLS[event.control as usize];
        observed.store(control.layer, Ordering::Release);
        first.store(true, Ordering::Release);
        if !(2..=3).contains(&control.layer) {
            continue;
        }
        let context = foreground.lock().unwrap().clone();
        if context.generation != 0 && event.received_at < context.changed_at {
            continue;
        }
        let Some(host) = plan.hosts.get(context.host_index(plan, control.layer)) else {
            continue;
        };
        let Some(action) = &host.actions[control.slot as usize] else {
            continue;
        };
        if !policy.admits(admission, action) {
            continue;
        }
        if let Err(error) = check_connection() {
            receiver.invalidate(&error);
            return;
        }
        if stop.load(Ordering::Acquire) || receiver.failure().is_some() {
            return;
        }
        if event.is_expired_at(Instant::now()) {
            continue;
        }
        if foreground.lock().unwrap().generation != context.generation {
            continue;
        }
        let receipt = execute(
            &host.profile_id,
            CONTROL_BINDINGS[control.slot as usize],
            action,
            admission,
        );
        if receipts.try_send(receipt).is_err() {
            receiver.invalidate("执行回执积压，输入已暂停");
            return;
        }
    }
}

fn connect(
    transport: &dyn HidTransport,
    target: &TargetKeyboard,
) -> Result<(Arc<DeviceSession>, HidCandidate), String> {
    let candidates = transport.enumerate().map_err(|e| e.to_string())?;
    let candidate = candidates
        .into_iter()
        .find(|candidate| {
            candidate.path == target.vendor_path
                && mi_transport::is_exclusive_vendor_candidate(candidate)
        })
        .ok_or("已确认的专用接口已消失")?;
    let link = transport.open(&candidate.path).map_err(|e| e.to_string())?;
    let session = DeviceSession::new(
        link,
        Box::new(mi_proto::NativeFramer::default()),
        Arc::new(mi_session::ReadOnlySchema),
        mi_session::SessionConfig::default(),
    );
    Ok((session, candidate))
}

pub async fn apply(
    document_dir: &Path,
    plan: AppliedSixLayerPlan,
    mut saved: SavedSixLayerState,
) -> Result<RunningSixLayer, String> {
    let state_path = document_dir.join("six-layer-runtime.json");
    plan.validate_current()?;
    let target = mi_transport::discover_target_keyboard().map_err(|e| e.to_string())?;
    let receiver = Arc::new(UserCommandReceiver::bind().map_err(|e| e.to_string())?);
    let rule = karabiner::rules(&target, &receiver, plan.bottom_row_layout)?;
    let path = karabiner::config_path()?;
    let patch = karabiner::prepare_config(
        path.clone(),
        fs::read(&path).map_err(|e| e.to_string())?,
        &target,
        rule,
    )?;
    let exclusive = MacTransport::exclusive_vendor();
    let (session, _) = connect(&exclusive, &target)?;
    with_verification_session(&session, async {
        let prepared = session
            .prepare_six_layer_write(document_dir)
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
        let backup = if let Some(previous) = &saved.installation {
            restoration_config.original = previous.config.original.clone();
            restoration_config.original_device = previous.config.original_device.clone();
            previous.backup.clone()
        } else {
            prepared.backup.clone()
        };
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
            return Err(error);
        }
        let result = session.apply_six_layer_write(prepared).await;
        let receipt = match result {
            Ok(receipt) => receipt,
            Err(error) => {
                if error.original_verified {
                    let rollback = patch.rollback_exact();
                    if rollback.is_ok() {
                        let _ = save(&state_path, &previous);
                    }
                    return Err(format!(
                        "{}；设备原件已确认；规则恢复：{}",
                        error.detail,
                        rollback.map(|_| "已确认".into()).unwrap_or_else(|e| e)
                    ));
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
        session.shutdown().await;
        drop(exclusive);
        let (shared, candidate) = match connect(&MacTransport::new(), &target) {
            Ok(connected) => connected,
            Err(error) => {
                installation.status = InstallationStatus::RecoveryRequired;
                saved.installation = Some(installation);
                let _ = save(&state_path, &saved);
                return Err(format!("{error}；保留吞键规则，请恢复设备"));
            }
        };
        with_verification_session(&shared, async {
            let layer = shared
                .verify_six_layer_files(&candidate_bytes, smart.as_deref(), receipt.profile_index)
                .await
                .map_err(|e| format!("{e}；保留吞键规则，请恢复设备"))?;
            readiness.recheck(&patch, &target)?;
            installation.status = InstallationStatus::Applied;
            saved.installation = Some(installation.clone());
            save(&state_path, &saved)?;
            Ok(new_running(
                receiver,
                readiness,
                shared.clone(),
                candidate,
                installation,
                layer as u8,
            ))
        })
        .await
    })
    .await
}

fn new_running(
    receiver: Arc<UserCommandReceiver>,
    readiness: Readiness,
    session: Arc<DeviceSession>,
    candidate: HidCandidate,
    installation: Installation,
    layer: u8,
) -> RunningSixLayer {
    RunningSixLayer {
        receiver,
        readiness,
        session,
        candidate,
        installation,
        observed_layer: Arc::new(AtomicU8::new(layer)),
        first_event_verified: Arc::new(AtomicBool::new(false)),
        armed: false,
        execution_policy: Arc::new(ExecutionPolicy::default()),
        receipts: None,
        foreground: Arc::new(Mutex::new(ForegroundContext::default())),
        stop: Arc::new(AtomicBool::new(false)),
        monitor: None,
        executor: None,
    }
}

/// Reconnection never selects a profile or writes device configuration.
pub async fn resume(
    document_dir: &Path,
    mut saved: SavedSixLayerState,
) -> Result<RunningSixLayer, String> {
    let mut installation = saved
        .installation
        .clone()
        .filter(|installation| installation.status == InstallationStatus::Applied)
        .ok_or("六层记录未完成，需要恢复")?;
    installation.plan.validate_current()?;
    let target = mi_transport::discover_target_keyboard().map_err(|e| e.to_string())?;
    let (session, candidate) = connect(&MacTransport::new(), &target)?;
    with_verification_session(&session, async {
        let expected =
            fs::read(installation.backup.join("planned-keymap.json")).map_err(|e| e.to_string())?;
        let smart = match fs::read(installation.backup.join("smart_actions.json")) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.to_string()),
        };
        let layer = session
            .verify_six_layer_files(&expected, smart.as_deref(), installation.profile_index)
            .await
            .map_err(|e| format!("{e}；未写设备，请显式应用或恢复"))?;
        let receiver = Arc::new(UserCommandReceiver::bind().map_err(|e| e.to_string())?);
        let rule = karabiner::rules(&target, &receiver, installation.plan.bottom_row_layout)?;
        let path = installation.config.path.clone();
        let patch = karabiner::prepare_config(
            path.clone(),
            fs::read(&path).map_err(|e| e.to_string())?,
            &target,
            rule,
        )?;
        let baseline = ReloadBaseline::capture()?;
        patch.install(&document_dir.join("karabiner-backups"))?;
        installation.config.installed = patch.installed.clone();
        installation.config.installed_device = patch.installed_device.clone();
        installation.target = target.clone();
        saved.installation = Some(installation.clone());
        save(&document_dir.join("six-layer-runtime.json"), &saved)?;
        // The board is already direct-coded. A failed reload must leave swallowing
        // rules in place, including when the previous rule had been removed.
        let readiness = wait_ready(&baseline, &patch, &target)?;
        Ok(new_running(
            receiver,
            readiness,
            session.clone(),
            candidate,
            installation,
            layer as u8,
        ))
    })
    .await
}

// The vendor reader is an establishment resource, never an input-liveness lease.
async fn with_verification_session<T>(
    session: &DeviceSession,
    operation: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, String> {
    let result = operation.await;
    session.shutdown().await;
    result
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

pub async fn restore(document_dir: &Path, mut saved: SavedSixLayerState) -> Result<(), String> {
    let installation = saved.installation.clone().ok_or("没有可恢复的六层记录")?;
    let target = mi_transport::discover_target_keyboard().map_err(|e| e.to_string())?;
    if target.location_id != installation.target.location_id {
        return Err("键盘 USB 位置已变化，未自动恢复".into());
    }
    let transport = MacTransport::exclusive_vendor();
    let (session, _) = connect(&transport, &target)?;
    with_verification_session(&session, async {
        session
            .restore_six_layer_backup(&installation.backup)
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
    use mi_model::board_layers::BoardLayerPlan;
    use mi_model::host_router::BottomRowLayout;
    use mi_transport::ActivationPayload;
    use std::os::unix::net::UnixDatagram;

    #[test]
    fn verification_session_is_closed_on_success_and_failure() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        for outcome in [Ok(()), Err("readback mismatch".to_string())] {
            let session = {
                let _entered = runtime.enter();
                let (link, _) = mi_transport::MockLink::new();
                DeviceSession::new(
                    link,
                    Box::new(mi_proto::NativeFramer::default()),
                    Arc::new(mi_session::ReadOnlySchema),
                    mi_session::SessionConfig::default(),
                )
            };
            let expected = outcome.clone();
            let result = runtime.block_on(with_verification_session(&session, async { outcome }));
            assert_eq!(result, expected);
            assert!(session.is_disconnected());
        }
    }

    #[test]
    fn executor_runs_after_verification_rpc_closes_and_stops_on_input_loss() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let (link, _) = mi_transport::MockLink::new();
        let session = {
            let _entered = runtime.enter();
            DeviceSession::new(
                link.clone(),
                Box::new(mi_proto::NativeFramer::default()),
                Arc::new(mi_session::ReadOnlySchema),
                mi_session::SessionConfig::default(),
            )
        };
        let mut running = RunningSixLayer::isolated(session.clone(), directory.path());
        running.readiness.configuration_observed = true;
        running.readiness.keyboard_grabbed = true;
        runtime
            .block_on(with_verification_session(&session, async { Ok(()) }))
            .unwrap();
        assert!(session.is_disconnected());
        running.receiver.arm().unwrap();
        running.set_execution_policy(true, true, Instant::now() - Duration::from_secs(1));
        running.foreground_changed(Some("com.google.antigravity"));
        let driver = mi_macos::RecordingOsDriver::default();
        running.start_executor(Arc::new(mi_macos::LiveHostActionExecutor::new(
            driver.clone(),
        )));
        send(&running.receiver, 40);
        send(&running.receiver, 7);
        let receipt = running
            .receipts
            .as_ref()
            .unwrap()
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        assert_eq!(receipt.outcome, "executed");
        assert_eq!(receipt.detail, "antigravity/focus-input");
        assert_eq!(
            receipt.profile_id,
            running.installation.plan.hosts[0].profile_id
        );
        assert_eq!(
            driver.events(),
            vec![
                mi_macos::executor::RecordedOsEvent::CheckAccessibilityTrusted,
                mi_macos::executor::RecordedOsEvent::FocusAgentInput("antigravity".into()),
            ]
        );
        assert!(link.writes.lock().unwrap().is_empty());
        assert!(running.receiver.failure().is_none());
        assert!(!check_input_connection(&running.receiver, false, || Err(
            "USB target changed".into()
        )));
        send(&running.receiver, 7);
        running.executor.take().unwrap().join().unwrap().unwrap();
        assert!(running.receipts.as_ref().unwrap().try_recv().is_err());
        assert_eq!(driver.events().len(), 2);
        running.shutdown(&runtime).unwrap();
    }

    #[test]
    fn input_monitor_ticks_do_not_need_vendor_rpc_and_fail_closed_on_readiness_loss() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (link, _) = mi_transport::MockLink::new();
        let session = {
            let _entered = runtime.enter();
            DeviceSession::new(
                link.clone(),
                Box::new(mi_proto::NativeFramer::default()),
                Arc::new(mi_session::ReadOnlySchema),
                mi_session::SessionConfig::default(),
            )
        };
        runtime.block_on(session.shutdown());
        let receiver = UserCommandReceiver::bind().unwrap();
        receiver.arm().unwrap();
        for _ in 0..1000 {
            assert!(check_input_connection(&receiver, false, || Ok(())));
        }
        assert!(link.writes.lock().unwrap().is_empty());
        assert!(receiver.failure().is_none());
        assert!(!check_input_connection(&receiver, false, || Err(
            "USB target changed".into()
        )));
        assert_eq!(receiver.failure().as_deref(), Some("USB target changed"));
        assert!(receiver.next().is_err());
        let receiver = UserCommandReceiver::bind().unwrap();
        assert!(!check_input_connection(&receiver, true, || panic!(
            "sleep stops before recheck"
        )));
        assert!(receiver.failure().unwrap().contains("睡眠"));
    }

    fn send(receiver: &UserCommandReceiver, control: u8) {
        let payload =
            serde_json::to_vec(&ActivationPayload::new(receiver.generation(), control)).unwrap();
        UnixDatagram::unbound()
            .unwrap()
            .send_to(&payload, receiver.path())
            .unwrap();
    }

    fn test_plan() -> AppliedSixLayerPlan {
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

    #[test]
    fn foreground_host_overrides_physical_host_without_changing_observed_layer() {
        let plan = test_plan();
        let receiver = UserCommandReceiver::bind().unwrap();
        receiver.arm().unwrap();
        let foreground = Mutex::new(ForegroundContext::default());
        foreground
            .lock()
            .unwrap()
            .update(Some("com.google.antigravity"));
        send(&receiver, 27);
        let stop = AtomicBool::new(false);
        let policy = ExecutionPolicy::default();
        policy.set(true, true, Instant::now() - Duration::from_secs(1));
        let observed = AtomicU8::new(1);
        let (tx, rx) = std::sync::mpsc::sync_channel(8);
        thread::scope(|scope| {
            let (done, wait) = std::sync::mpsc::channel::<()>();
            let stop_ref = &stop;
            scope.spawn(move || {
                if wait.recv_timeout(Duration::from_secs(5)).is_err() {
                    stop_ref.store(true, Ordering::Release);
                }
            });
            execute_clicks(
                &receiver,
                &plan,
                &stop,
                &policy,
                &observed,
                &AtomicBool::new(false),
                &foreground,
                &tx,
                || receiver.next(),
                || Ok(()),
                |profile, binding, action, _| {
                    stop.store(true, Ordering::Release);
                    mi_macos::SimulatedHostActionExecutor.execute(profile, binding, action)
                },
            );
            let _ = done.send(());
        });
        let receipt = rx.try_recv().unwrap();
        assert_eq!(receipt.profile_id, plan.hosts[0].profile_id);
        assert_eq!(receipt.detail, "antigravity/focus-input");
        assert_eq!(observed.load(Ordering::Acquire), 3);
        let mut context = foreground.lock().unwrap();
        context.update(Some("com.openai.codex"));
        assert_eq!(context.host_index(&plan, 3), 1);
        context.update(Some("com.apple.finder"));
        assert_eq!(context.host_index(&plan, 4), 2);
    }

    #[test]
    fn foreground_switch_discards_queued_and_waiting_clicks_without_retargeting() {
        let plan = test_plan();
        let receiver = UserCommandReceiver::bind().unwrap();
        receiver.arm().unwrap();
        let foreground = Mutex::new(ForegroundContext::default());
        send(&receiver, 7);
        thread::sleep(Duration::from_millis(30));
        foreground
            .lock()
            .unwrap()
            .update(Some("com.google.antigravity"));
        send(&receiver, 27);
        let stop = AtomicBool::new(false);
        let policy = ExecutionPolicy::default();
        policy.set(true, true, Instant::now() - Duration::from_secs(1));
        let (tx, rx) = std::sync::mpsc::sync_channel(8);
        let mut permits = 0;
        thread::scope(|scope| {
            let (done, wait) = std::sync::mpsc::channel::<()>();
            let stop_ref = &stop;
            scope.spawn(move || {
                if wait.recv_timeout(Duration::from_secs(5)).is_err() {
                    stop_ref.store(true, Ordering::Release);
                }
            });
            execute_clicks(
                &receiver,
                &plan,
                &stop,
                &policy,
                &AtomicU8::new(1),
                &AtomicBool::new(false),
                &foreground,
                &tx,
                || receiver.next(),
                || {
                    permits += 1;
                    if permits == 1 {
                        foreground.lock().unwrap().update(Some("com.deepseek.dsh"));
                        send(&receiver, 27);
                    }
                    Ok(())
                },
                |profile, binding, action, _| {
                    stop.store(true, Ordering::Release);
                    mi_macos::SimulatedHostActionExecutor.execute(profile, binding, action)
                },
            );
            let _ = done.send(());
        });
        let receipts: Vec<_> = rx.try_iter().collect();
        assert_eq!(permits, 2);
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].profile_id, plan.hosts[1].profile_id);
        assert_eq!(receipts[0].detail, "dsh-desktop/browser");
        assert!(receiver.failure().is_none());
    }

    #[test]
    fn execute_clicks_ignores_retired_layer_and_stops_on_connection_loss() {
        let plan = test_plan();
        let ag_id = plan.hosts[0].profile_id.clone();
        let dsh_id = plan.hosts[1].profile_id.clone();

        let receiver = Arc::new(UserCommandReceiver::bind().unwrap());
        receiver.arm().unwrap();
        send(&receiver, 0);
        send(&receiver, 20);
        send(&receiver, 40);
        send(&receiver, 21);
        thread::sleep(Duration::from_millis(30));

        let stop = AtomicBool::new(false);
        let policy = ExecutionPolicy::default();
        policy.set(true, true, Instant::now() - Duration::from_secs(1));
        let observed = AtomicU8::new(1);
        let first = AtomicBool::new(false);
        let (tx, rx) = std::sync::mpsc::sync_channel(8);
        let mut calls = Vec::new();
        let mut permit_calls = 0usize;

        execute_clicks(
            &receiver,
            &plan,
            &stop,
            &policy,
            &observed,
            &first,
            &Mutex::new(ForegroundContext::default()),
            &tx,
            || receiver.next(),
            || {
                permit_calls += 1;
                if permit_calls <= 2 {
                    Ok(())
                } else {
                    Err("设备连接已结束".into())
                }
            },
            |profile, binding, _action, _epoch| {
                calls.push((profile.to_owned(), binding.to_owned()));
                mi_macos::HostActionReceipt {
                    action_type: "open_app".into(),
                    binding_id: binding.into(),
                    detail: "ok".into(),
                    outcome: "executed".into(),
                    profile_id: profile.into(),
                    target: "test".into(),
                    text_byte_len: None,
                    timestamp_ms: 1,
                }
            },
        );

        assert!(first.load(Ordering::Acquire));
        assert_eq!(observed.load(Ordering::Acquire), 3);
        assert_eq!(
            calls,
            vec![(ag_id, "AG00".into()), (dsh_id, "AG00".into()),]
        );
        assert_eq!(rx.try_iter().count(), 2);
        assert_eq!(receiver.failure().as_deref(), Some("设备连接已结束"));
    }

    #[test]
    fn a_click_expiring_during_connection_check_does_not_stop_fresh_clicks() {
        let plan = test_plan();
        let dsh_id = plan.hosts[1].profile_id.clone();

        let receiver = Arc::new(UserCommandReceiver::bind().unwrap());
        receiver.arm().unwrap();
        send(&receiver, 0);
        thread::sleep(Duration::from_millis(30));
        let stop = AtomicBool::new(false);
        let policy = ExecutionPolicy::default();
        policy.set(true, true, Instant::now() - Duration::from_secs(3));
        let observed = AtomicU8::new(1);
        let first = AtomicBool::new(false);
        let (tx, rx) = std::sync::mpsc::sync_channel(8);
        let mut calls = Vec::new();
        let mut permit_calls = 0;
        thread::scope(|scope| {
            let (cancel_tx, cancel_rx) = std::sync::mpsc::channel::<()>();
            let watchdog_stop = &stop;
            scope.spawn(move || {
                if cancel_rx.recv_timeout(Duration::from_secs(5)).is_err() {
                    watchdog_stop.store(true, Ordering::Release);
                }
            });
            execute_clicks(
                &receiver,
                &plan,
                &stop,
                &policy,
                &observed,
                &first,
                &Mutex::new(ForegroundContext::default()),
                &tx,
                || receiver.next(),
                || {
                    permit_calls += 1;
                    if permit_calls == 1 {
                        thread::sleep(Duration::from_millis(2100));
                        send(&receiver, 20);
                        thread::sleep(Duration::from_millis(30));
                    }
                    Ok(())
                },
                |profile, binding, _, _| {
                    calls.push((profile.to_owned(), binding.to_owned()));
                    stop.store(true, Ordering::Release);
                    mi_macos::HostActionReceipt {
                        action_type: "open_app".into(),
                        binding_id: binding.into(),
                        detail: "ok".into(),
                        outcome: "executed".into(),
                        profile_id: profile.into(),
                        target: "test".into(),
                        text_byte_len: None,
                        timestamp_ms: 1,
                    }
                },
            );
            let _ = cancel_tx.send(());
        });
        assert_eq!(calls, vec![(dsh_id, "AG00".into())]);
        assert_eq!(permit_calls, 2);
        assert_eq!(rx.try_iter().count(), 1);
        assert!(receiver.failure().is_none());
    }

    #[test]
    fn disable_then_enable_rejects_in_flight_request_and_accepts_new_input() {
        use mi_macos::{LiveHostActionExecutor, RecordingOsDriver};
        use std::sync::mpsc;

        let receiver = Arc::new(UserCommandReceiver::bind().unwrap());
        let plan = test_plan();
        let now = Instant::now();
        let policy = Arc::new(ExecutionPolicy::default());
        policy.set(true, true, now);
        let old_admission = policy.capture(now);
        policy.set(true, true, now + Duration::from_millis(1));
        assert_eq!(policy.capture(now).epoch, old_admission.epoch);
        let driver = RecordingOsDriver::new(true);
        let executor = Arc::new(LiveHostActionExecutor::new(driver.clone()));
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (resume_tx, resume_rx) = mpsc::sync_channel(1);
        let (receipt_tx, receipt_rx) = mpsc::sync_channel(1);
        let worker_policy = Arc::clone(&policy);
        let worker_executor = Arc::clone(&executor);
        let worker = thread::spawn(move || {
            let stop = AtomicBool::new(false);
            let event = mi_transport::Activation {
                control: 0,
                generation: receiver.generation().into(),
                received_sequence: 1,
                received_at: now,
            };
            let mut pending = Some(event);
            execute_clicks(
                &receiver,
                &plan,
                &stop,
                &worker_policy,
                &AtomicU8::new(1),
                &AtomicBool::new(false),
                &Mutex::new(ForegroundContext::default()),
                &receipt_tx,
                || {
                    pending
                        .take()
                        .map(Some)
                        .ok_or_else(|| "test input exhausted".into())
                },
                || {
                    entered_tx.send(()).unwrap();
                    resume_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                    Ok(())
                },
                |profile, binding, action, admission| {
                    let receipt = worker_executor.execute_when(profile, binding, action, || {
                        !stop.load(Ordering::Acquire)
                            && receiver.failure().is_none()
                            && worker_policy.admits(admission, action)
                    });
                    stop.store(true, Ordering::Release);
                    receipt
                },
            );
        });
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        policy.set(false, true, now + Duration::from_millis(2));
        executor.stop_owned_actions().unwrap();
        policy.set(true, true, now + Duration::from_millis(3));
        resume_tx.send(()).unwrap();
        worker.join().unwrap();
        let receipt = receipt_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(receipt.outcome, "blocked");
        assert_eq!(receipt.detail, "input_retired");
        assert!(driver.events().is_empty());
        let action = ResolvedHostAction::OpenApp {
            bundle_id: "com.example.editor".into(),
        };
        let new_admission = policy.capture(now + Duration::from_millis(4));
        let receipt = executor.execute_when("new", "AG00", &action, || {
            policy.admits(new_admission, &action)
        });
        assert_eq!(receipt.outcome, "executed");
        assert!(!driver.events().is_empty());
    }

    #[test]
    fn disable_then_enable_discards_queued_old_input_before_execution() {
        use mi_macos::{LiveHostActionExecutor, RecordingOsDriver};

        let receiver = UserCommandReceiver::bind().unwrap();
        let plan = test_plan();
        let now = Instant::now();
        let policy = ExecutionPolicy::default();
        policy.set(true, true, now);
        let mut pending = std::collections::VecDeque::from([
            mi_transport::Activation {
                control: 0,
                generation: receiver.generation().into(),
                received_sequence: 1,
                received_at: now,
            },
            mi_transport::Activation {
                control: 0,
                generation: receiver.generation().into(),
                received_sequence: 2,
                received_at: now + Duration::from_millis(3),
            },
        ]);
        let driver = RecordingOsDriver::new(true);
        let executor = LiveHostActionExecutor::new(driver.clone());
        policy.set(false, true, now + Duration::from_millis(1));
        executor.stop_owned_actions().unwrap();
        policy.set(true, true, now + Duration::from_millis(2));
        let stop = AtomicBool::new(false);
        let (tx, rx) = std::sync::mpsc::sync_channel(2);
        let mut executed = 0;
        execute_clicks(
            &receiver,
            &plan,
            &stop,
            &policy,
            &AtomicU8::new(1),
            &AtomicBool::new(false),
            &Mutex::new(ForegroundContext::default()),
            &tx,
            || {
                pending
                    .pop_front()
                    .map(Some)
                    .ok_or_else(|| "test input exhausted".into())
            },
            || Ok(()),
            |profile, binding, action, admission| {
                assert_eq!(admission.received_at, now + Duration::from_millis(3));
                assert!(driver.events().is_empty());
                executed += 1;
                let receipt = executor.execute_when(profile, binding, action, || {
                    !stop.load(Ordering::Acquire)
                        && receiver.failure().is_none()
                        && policy.admits(admission, action)
                });
                stop.store(true, Ordering::Release);
                receipt
            },
        );
        assert_eq!(executed, 1);
        assert_eq!(rx.try_iter().count(), 1);
        assert!(!driver.events().is_empty());
        assert!(receiver.failure().is_none());
    }

    #[test]
    fn saved_state_roundtrips_and_rejects_unsupported_versions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("six-layer-runtime.json");
        assert!(!load(&path)
            .unwrap()
            .legacy_native_paused_confirmation
            .is_some());
        let mut state = SavedSixLayerState::default();
        state.legacy_native_paused_confirmation = Some(true);
        save(&path, &state).unwrap();
        assert!(load(&path)
            .unwrap()
            .legacy_native_paused_confirmation
            .is_some());
        fs::write(
            &path,
            br#"{"version":2,"cindy_native_paused_confirmed":true,"installation":null}"#,
        )
        .unwrap();
        assert!(load(&path).is_err());
    }
}
