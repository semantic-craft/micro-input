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
        atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
        Arc,
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
    pub cindy_native_paused_confirmed: bool,
    pub installation: Option<Installation>,
}

impl Default for SavedSixLayerState {
    fn default() -> Self {
        Self {
            version: 1,
            cindy_native_paused_confirmed: false,
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

pub struct RunningSixLayer {
    pub receiver: Arc<UserCommandReceiver>,
    pub readiness: Readiness,
    pub session: Arc<DeviceSession>,
    pub candidate: HidCandidate,
    pub installation: Installation,
    pub observed_layer: Arc<AtomicU8>,
    pub first_event_verified: Arc<AtomicBool>,
    pub armed: bool,
    pub execution_enabled: Arc<AtomicBool>,
    pub command_execution_enabled: Arc<AtomicBool>,
    pub receipts: Option<std::sync::mpsc::Receiver<mi_macos::HostActionReceipt>>,
    stop: Arc<AtomicBool>,
    monitor: Option<JoinHandle<()>>,
    executor: Option<JoinHandle<Result<(), String>>>,
}

pub struct OwnershipPermit {
    pub transport: mi_bridge::UnixNativeOwnershipTransport,
    pub request: mi_bridge::NativeOwnershipRequest,
    pub native_identity: String,
    pub min_native_generation: AtomicU64,
    pub allow_unbridged_drain: bool,
}

impl OwnershipPermit {
    fn check(&self) -> Result<(), String> {
        let _guard = self.transport.lock_path();
        let mut request = self.request.clone();
        if let Some(fence) = self.transport.read_persisted_fence() {
            request.client_instance = fence.client_instance;
            request.generation = request.generation.max(fence.generation);
        }
        request.request_id = mi_model::document::new_id()
            .parse()
            .map_err(|_| "请求身份生成失败")?;
        let reply = self
            .transport
            .exchange_locked(&request)
            .map_err(|e| e.to_string())?;
        let has_fenced_cap = reply
            .capabilities
            .as_ref()
            .is_some_and(|caps| caps.iter().any(|cap| cap == "fenced-release-v1"));
        let released_ok = (reply.state == "released"
            && reply.input_drained
            && reply.ptt_drained
            && reply.lighting_drained)
            || (self.allow_unbridged_drain
                && reply.state == "unknown"
                && reply.reason == "native_drain_unconfirmed"
                && reply.lighting_drained);
        let min_gen = self.min_native_generation.load(Ordering::Acquire);
        if reply.version != 1
            || reply.request_id != request.request_id
            || reply.client_instance != request.client_instance
            || reply.generation != request.generation
            || !released_ok
            || !has_fenced_cap
            || reply.native_instance.to_string() != self.native_identity
            || reply.native_generation < min_gen
        {
            return Err("Cindy 当前代释放未确认".into());
        }
        self.min_native_generation
            .fetch_max(reply.native_generation, Ordering::AcqRel);
        Ok(())
    }
}

impl RunningSixLayer {
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
            id(mi_model::preset::MANAGED_CINDY_PROFILE_NAME),
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

    pub fn start_executor(&mut self, permit: OwnershipPermit) {
        let receiver = Arc::clone(&self.receiver);
        let plan = self.installation.plan.clone();
        let stop = Arc::clone(&self.stop);
        let enabled = Arc::clone(&self.execution_enabled);
        let commands = Arc::clone(&self.command_execution_enabled);
        let observed = Arc::clone(&self.observed_layer);
        let first = Arc::clone(&self.first_event_verified);
        let (tx, rx) = std::sync::mpsc::sync_channel(32);
        self.receipts = Some(rx);
        self.executor = Some(thread::spawn(move || {
            let executor =
                mi_macos::LiveHostActionExecutor::new(mi_macos::MacOsSystemDriver::default());
            execute_clicks(
                &receiver,
                &plan,
                &stop,
                &enabled,
                &commands,
                &observed,
                &first,
                &tx,
                || permit.check(),
                |profile, binding, action| executor.execute(profile, binding, action),
            );
            executor.stop_owned_actions()
        }));
    }

    pub fn retire(&self) {
        self.receiver.invalidate("运行代次已结束");
        self.stop.store(true, Ordering::Release);
        self.session.disconnect();
    }

    pub fn shutdown(mut self, runtime: &tokio::runtime::Runtime) -> Result<(), String> {
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

    pub fn start_monitor(&mut self, runtime: Arc<tokio::runtime::Runtime>) {
        let receiver = Arc::clone(&self.receiver);
        let stop = Arc::clone(&self.stop);
        let readiness = self.readiness.clone();
        let patch = self.installation.config.clone();
        let target = self.installation.target.clone();
        let session = Arc::clone(&self.session);
        let observed = Arc::clone(&self.observed_layer);
        let profile_index = self.installation.profile_index;
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
                if slept || session.is_disconnected() {
                    receiver.invalidate("系统睡眠或设备连接已变化");
                    return;
                }
                if let Err(error) = readiness.recheck(&patch, &target) {
                    receiver.invalidate(&error);
                    return;
                }
                match runtime.block_on(async {
                    tokio::time::timeout(Duration::from_secs(2), session.read_identity_info()).await
                }) {
                    Ok(Ok(identity))
                        if identity.version.as_deref() == Some("0.6.3")
                            && identity.profile_index == Some(profile_index)
                            && identity
                                .layer_index
                                .is_some_and(|layer| (1..=6).contains(&layer)) =>
                    {
                        observed.store(identity.layer_index.unwrap() as u8, Ordering::Release);
                    }
                    _ => {
                        receiver.invalidate("当前设备方案或层位未确认");
                        return;
                    }
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

fn execute_clicks(
    receiver: &UserCommandReceiver,
    plan: &AppliedSixLayerPlan,
    stop: &AtomicBool,
    enabled: &AtomicBool,
    commands: &AtomicBool,
    observed: &AtomicU8,
    first: &AtomicBool,
    receipts: &std::sync::mpsc::SyncSender<mi_macos::HostActionReceipt>,
    mut permit: impl FnMut() -> Result<(), String>,
    mut execute: impl FnMut(&str, &str, &ResolvedHostAction) -> mi_macos::HostActionReceipt,
) {
    while !stop.load(Ordering::Acquire) {
        let event = match receiver.next() {
            Ok(Some(event)) => event,
            Ok(None) => {
                thread::sleep(Duration::from_millis(5));
                continue;
            }
            Err(_) => return,
        };
        if event.generation != receiver.generation() {
            continue;
        }
        let control = &DIRECT_CONTROLS[event.control as usize];
        observed.store(control.layer, Ordering::Release);
        first.store(true, Ordering::Release);
        if !enabled.load(Ordering::Acquire) {
            continue;
        }
        let host = &plan.hosts[(control.layer - 2) as usize];
        let Some(action) = &host.actions[control.slot as usize] else {
            continue;
        };
        if matches!(action, ResolvedHostAction::ShellCommand { .. })
            && !commands.load(Ordering::Acquire)
        {
            continue;
        }
        if let Err(error) = permit() {
            receiver.invalidate(&error);
            return;
        }
        if stop.load(Ordering::Acquire) || receiver.failure().is_some() {
            return;
        }
        if event.received_at.elapsed() > Duration::from_secs(2) {
            receiver.invalidate("点击已过期，停止派发积压输入");
            return;
        }
        let receipt = execute(
            &host.profile_id,
            CONTROL_BINDINGS[control.slot as usize],
            action,
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
    if !saved.cindy_native_paused_confirmed {
        return Err("请先确认 Cindy 原生键盘集成已关闭".into());
    }
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
    session.disconnect();
    drop(session);
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
        shared,
        candidate,
        installation,
        layer as u8,
    ))
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
        execution_enabled: Arc::new(AtomicBool::new(false)),
        command_execution_enabled: Arc::new(AtomicBool::new(false)),
        receipts: None,
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
    if !saved.cindy_native_paused_confirmed {
        return Err("Cindy 原生暂停尚未确认".into());
    }
    let mut installation = saved
        .installation
        .clone()
        .filter(|installation| installation.status == InstallationStatus::Applied)
        .ok_or("六层记录未完成，需要恢复")?;
    let target = mi_transport::discover_target_keyboard().map_err(|e| e.to_string())?;
    let (session, candidate) = connect(&MacTransport::new(), &target)?;
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
        session,
        candidate,
        installation,
        layer as u8,
    ))
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
    let installation = saved.installation.as_ref().ok_or("没有可恢复的六层记录")?;
    let target = mi_transport::discover_target_keyboard().map_err(|e| e.to_string())?;
    if target.location_id != installation.target.location_id {
        return Err("键盘 USB 位置已变化，未自动恢复".into());
    }
    let transport = MacTransport::exclusive_vendor();
    let (session, _) = connect(&transport, &target)?;
    session
        .restore_six_layer_backup(&installation.backup)
        .await
        .map_err(|e| format!("{e}；保留吞键规则"))?;
    installation.config.restore_owned(&target)?;
    session.disconnect();
    saved.installation = None;
    save(&document_dir.join("six-layer-runtime.json"), &saved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mi_model::board_layers::BoardLayerPlan;
    use mi_model::host_router::BottomRowLayout;
    use mi_transport::ActivationPayload;
    use std::os::unix::net::UnixDatagram;

    fn send(receiver: &UserCommandReceiver, control: u8) {
        let payload =
            serde_json::to_vec(&ActivationPayload::new(receiver.generation(), control)).unwrap();
        UnixDatagram::unbound()
            .unwrap()
            .send_to(&payload, receiver.path())
            .unwrap();
    }

    #[test]
    fn execute_clicks_routes_by_layer_and_stops_on_ownership_loss() {
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
        let codex_id = id("Codex");
        let ag_id = id("Antigravity");
        let dsh_id = id("DeepSeek Coder (DSH)");
        let cindy_id = id(mi_model::preset::MANAGED_CINDY_PROFILE_NAME);
        let desired =
            BoardLayerPlan::recommended(codex_id, ag_id.clone(), dsh_id.clone(), cindy_id.clone());
        let plan =
            AppliedSixLayerPlan::compile(&desired, &draft.document, BottomRowLayout::Split, true)
                .unwrap();

        let receiver = Arc::new(UserCommandReceiver::bind().unwrap());
        receiver.arm().unwrap();
        send(&receiver, 0);
        send(&receiver, 20);
        send(&receiver, 40);
        send(&receiver, 41);
        thread::sleep(Duration::from_millis(30));

        let stop = AtomicBool::new(false);
        let enabled = AtomicBool::new(true);
        let commands = AtomicBool::new(true);
        let observed = AtomicU8::new(1);
        let first = AtomicBool::new(false);
        let (tx, rx) = std::sync::mpsc::sync_channel(8);
        let mut calls = Vec::new();
        let mut permit_calls = 0usize;

        execute_clicks(
            &receiver,
            &plan,
            &stop,
            &enabled,
            &commands,
            &observed,
            &first,
            &tx,
            || {
                permit_calls += 1;
                if permit_calls <= 3 {
                    Ok(())
                } else {
                    Err("Cindy 当前代释放未确认".into())
                }
            },
            |profile, binding, _action| {
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
        assert_eq!(observed.load(Ordering::Acquire), 4);
        assert_eq!(
            calls,
            vec![
                (ag_id, "AG00".into()),
                (dsh_id, "AG00".into()),
                (cindy_id, "AG00".into()),
            ]
        );
        assert_eq!(rx.try_iter().count(), 3);
        assert_eq!(
            receiver.failure().as_deref(),
            Some("Cindy 当前代释放未确认")
        );
    }

    #[test]
    fn a_click_expiring_during_ownership_check_does_not_stop_fresh_clicks() {
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
        let codex_id = id("Codex");
        let ag_id = id("Antigravity");
        let dsh_id = id("DeepSeek Coder (DSH)");
        let cindy_id = id(mi_model::preset::MANAGED_CINDY_PROFILE_NAME);
        let desired =
            BoardLayerPlan::recommended(codex_id, ag_id.clone(), dsh_id.clone(), cindy_id.clone());
        let plan =
            AppliedSixLayerPlan::compile(&desired, &draft.document, BottomRowLayout::Split, true)
                .unwrap();

        let receiver = Arc::new(UserCommandReceiver::bind().unwrap());
        receiver.arm().unwrap();
        send(&receiver, 0);
        thread::sleep(Duration::from_millis(30));
        let stop = AtomicBool::new(false);
        let enabled = AtomicBool::new(true);
        let commands = AtomicBool::new(true);
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
                &enabled,
                &commands,
                &observed,
                &first,
                &tx,
                || {
                    permit_calls += 1;
                    if permit_calls == 1 {
                        thread::sleep(Duration::from_millis(2100));
                        send(&receiver, 20);
                        thread::sleep(Duration::from_millis(30));
                    }
                    Ok(())
                },
                |profile, binding, _| {
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
    fn saved_state_roundtrips_and_rejects_unsupported_versions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("six-layer-runtime.json");
        assert!(!load(&path).unwrap().cindy_native_paused_confirmed);
        let mut state = SavedSixLayerState::default();
        state.cindy_native_paused_confirmed = true;
        save(&path, &state).unwrap();
        assert!(load(&path).unwrap().cindy_native_paused_confirmed);
        fs::write(
            &path,
            br#"{"version":2,"cindy_native_paused_confirmed":true,"installation":null}"#,
        )
        .unwrap();
        assert!(load(&path).is_err());
    }

    #[test]
    fn ownership_permit_accepts_monotonic_unbridged_drain_and_rejects_stale_native_generation() {
        use std::io::{BufRead, BufReader};
        use std::os::unix::{fs::PermissionsExt, net::UnixListener};

        let dir = tempfile::Builder::new()
            .prefix("mi-permit-")
            .tempdir_in("/tmp")
            .unwrap();
        let sock_path = dir.path().join("cindy-ownership.sock");
        let transport = mi_bridge::UnixNativeOwnershipTransport::new(&sock_path);
        let client_instance = "c426ac36-354d-4149-a8b7-40b719a9dfff".parse().unwrap();
        let native_instance = "d1706f6a-dda3-4821-a862-e587e63cc25f";
        let native_identity = native_instance.to_string();
        transport
            .write_persisted_fence(&mi_bridge::PersistedNativeOwnershipFence {
                client_instance,
                generation: 40,
            })
            .unwrap();

        let listener = UnixListener::bind(&sock_path).unwrap();
        fs::set_permissions(&sock_path, fs::Permissions::from_mode(0o600)).unwrap();

        let server = thread::spawn(move || {
            for (expected_gen, native_gen) in [(40u64, 202u64), (41, 203), (41, 201)] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                let req: serde_json::Value = serde_json::from_str(&line).unwrap();
                assert_eq!(req["generation"].as_u64(), Some(expected_gen));
                let reply = serde_json::json!({
                    "version": 1,
                    "requestId": req["requestId"],
                    "clientInstance": req["clientInstance"],
                    "generation": req["generation"],
                    "nativeInstance": native_instance,
                    "nativeGeneration": native_gen,
                    "state": "unknown",
                    "reason": "native_drain_unconfirmed",
                    "inputDrained": false,
                    "pttDrained": false,
                    "lightingDrained": true,
                    "capabilities": ["fenced-release-v1"]
                });
                let mut out = serde_json::to_vec(&reply).unwrap();
                out.push(b'\n');
                stream.write_all(&out).unwrap();
            }
        });

        let permit = OwnershipPermit {
            transport: transport.clone(),
            request: mi_bridge::NativeOwnershipRequest {
                version: 1,
                request_id: mi_model::document::new_id().parse().unwrap(),
                client_instance,
                generation: 38,
                desired_owner: "micro-input".into(),
                micro_drained: true,
            },
            native_identity,
            min_native_generation: AtomicU64::new(202),
            allow_unbridged_drain: true,
        };

        assert!(permit.check().is_ok());
        transport
            .write_persisted_fence(&mi_bridge::PersistedNativeOwnershipFence {
                client_instance,
                generation: 41,
            })
            .unwrap();
        assert!(permit.check().is_ok());
        assert!(permit.check().is_err());
        server.join().unwrap();
    }
}
