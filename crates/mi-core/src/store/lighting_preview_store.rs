use super::*;
use crate::six_layer_runtime::VendorBridgePreviewLease;
use std::{future::Future, pin::Pin};

const LIGHTING_PREVIEW_BLOCKING_STOP_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LightingPreviewPhase {
    Starting,
    Active,
    Stopped,
    Unknown(String),
}

pub type LightingPreviewEndFuture =
    Pin<Box<dyn Future<Output = LightingPreviewPhase> + Send + 'static>>;
pub type LightingPreviewBeginFuture =
    Pin<Box<dyn Future<Output = Result<Box<dyn LightingPreviewHandle>, String>> + Send + 'static>>;

pub trait LightingPreviewHandle: Send + Sync + std::fmt::Debug {
    fn update(
        &self,
        zone: mi_model::lighting::Zone,
        edit: mi_model::lighting::Edit,
    ) -> Result<(), String>;
    fn replace(&self, lights: mi_model::lighting::Lighting) -> Result<(), String>;
    fn phase(&self) -> LightingPreviewPhase;
    fn end_future(&self) -> LightingPreviewEndFuture;
    fn detach_on_drop(&self);
}

pub trait LightingPreviewStarter: Send + Sync {
    fn requires_verified_session(&self) -> bool {
        true
    }

    fn preview_duration(&self) -> Duration {
        Duration::from_secs(30)
    }

    fn begin_future(
        &self,
        session: Option<Arc<DeviceSession>>,
        lights: mi_model::lighting::Lighting,
    ) -> LightingPreviewBeginFuture;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NativeLightingPreviewStarter;

struct NativeLightingPreviewHandle {
    preview: Mutex<Option<Arc<mi_session::lighting_preview::Preview>>>,
}

impl std::fmt::Debug for NativeLightingPreviewHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeLightingPreviewHandle")
            .field("attached", &self.preview.lock().unwrap().is_some())
            .finish()
    }
}

fn preview_status_to_phase(status: &Value) -> LightingPreviewPhase {
    match status.get("phase").and_then(Value::as_str) {
        Some("Starting") => LightingPreviewPhase::Starting,
        Some("Active") => LightingPreviewPhase::Active,
        Some("Stopped") => LightingPreviewPhase::Stopped,
        Some("Unknown") => LightingPreviewPhase::Unknown(
            "设备预览连接异常中断，已停止预览（未自动恢复原灯效）".to_string(),
        ),
        Some(other) => LightingPreviewPhase::Unknown(other.to_string()),
        None => LightingPreviewPhase::Unknown("灯光预览状态不可用".to_string()),
    }
}

impl LightingPreviewHandle for NativeLightingPreviewHandle {
    fn update(
        &self,
        zone: mi_model::lighting::Zone,
        edit: mi_model::lighting::Edit,
    ) -> Result<(), String> {
        let preview = self
            .preview
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| "preview has ended".to_string())?;
        preview.update(zone, edit)
    }

    fn replace(&self, lights: mi_model::lighting::Lighting) -> Result<(), String> {
        let preview = self
            .preview
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| "preview has ended".to_string())?;
        preview.replace(lights)
    }

    fn phase(&self) -> LightingPreviewPhase {
        match self.preview.lock().unwrap().as_ref() {
            Some(preview) => preview_status_to_phase(&preview.status()),
            None => LightingPreviewPhase::Stopped,
        }
    }

    fn end_future(&self) -> LightingPreviewEndFuture {
        let preview = self.preview.lock().unwrap().clone();
        Box::pin(async move {
            match preview {
                Some(preview) => preview_status_to_phase(&preview.end().await),
                None => LightingPreviewPhase::Stopped,
            }
        })
    }

    fn detach_on_drop(&self) {
        let _ = self.preview.lock().unwrap().take();
    }
}

impl LightingPreviewStarter for NativeLightingPreviewStarter {
    fn begin_future(
        &self,
        session: Option<Arc<DeviceSession>>,
        lights: mi_model::lighting::Lighting,
    ) -> LightingPreviewBeginFuture {
        Box::pin(async move {
            let session = session.ok_or_else(|| {
                "尚未连接已核验的 USB 专用独占键盘，无法启动硬件灯光预览".to_string()
            })?;
            if session.is_disconnected() || !session.has_exclusive_vendor_link() {
                return Err("需要已连接的 USB 专用独占会话".to_string());
            }
            let preview =
                mi_session::lighting_preview::Preview::begin_native(session, lights).await?;
            Ok(Box::new(NativeLightingPreviewHandle {
                preview: Mutex::new(Some(preview)),
            }) as Box<dyn LightingPreviewHandle>)
        })
    }
}

#[derive(Default)]
pub(crate) enum LightingPreviewState {
    #[default]
    Idle,
    Starting {
        generation: u64,
        profile_id: String,
        layer_id: String,
        abort_handle: tokio::task::AbortHandle,
        lease: Option<VendorBridgePreviewLease>,
        latest_lights: Lighting,
        lights_dirty: bool,
    },
    Active {
        generation: u64,
        profile_id: String,
        layer_id: String,
        handle: Box<dyn LightingPreviewHandle>,
        lease: Option<VendorBridgePreviewLease>,
        deadline: Instant,
    },
    Stopping {
        generation: u64,
        join_handle: tokio::task::JoinHandle<()>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LightingPreviewStopReason {
    UserRequested,
    Expired,
    ModalClosed,
    ContextChanged,
    LayerProtected,
    LightingRemoved,
    DeviceDisconnected,
    ExclusiveDeviceTransition,
    SystemSleep,
    SystemTerminate,
}

fn validate_lighting_for_preview(lights: &Lighting) -> Result<(), String> {
    let params: mi_proto::lighting_candidate::LightingPreviewCandidate =
        serde_json::from_value(serde_json::to_value(lights).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    mi_proto::lighting_candidate::encode_lighting_preview_candidate(&params, 0)
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn remaining_preview_secs(deadline: Instant, now: Instant) -> u8 {
    let rem_ms = deadline.saturating_duration_since(now).as_millis();
    if rem_ms == 0 {
        0
    } else {
        rem_ms.saturating_add(999).saturating_div(1000).min(30) as u8
    }
}

fn normalize_preview_start_error(raw: &str) -> String {
    match raw.trim() {
        "unsupported preview connection" => {
            "当前设备连接不支持原生灯光预览（需 macOS USB 专用独占通道）".to_string()
        }
        "unsupported preview firmware" => "当前设备固件版本不支持原生灯光预览".to_string(),
        "channel closed" => "设备预览通道已关闭，无法启动灯光预览".to_string(),
        msg if msg.starts_with("当前")
            || msg.starts_with("需要")
            || msg.starts_with("尚未")
            || msg.starts_with("固件")
            || msg.starts_with("六层")
            || msg.starts_with("USB ")
            || msg.starts_with("设备")
            || msg.starts_with("灯光")
            || msg.starts_with("上一轮")
            || msg.starts_with("正在核验") =>
        {
            msg.to_string()
        }
        other => format!("硬件灯光预览启动失败：{other}"),
    }
}

fn normalize_preview_unknown_error(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed == "Unknown"
        || trimmed == "设备预览连接异常中断，已停止预览（未自动恢复原灯效）"
    {
        "设备预览连接异常中断，已停止预览（未自动恢复原灯效）".to_string()
    } else if trimmed.starts_with("硬件灯光预览异常停止（未自动恢复原灯效）：")
    {
        trimmed.to_string()
    } else {
        format!("硬件灯光预览异常停止（未自动恢复原灯效）：{trimmed}")
    }
}

impl WorkbenchStore {
    pub(crate) fn is_lighting_preview_busy(&self) -> bool {
        !matches!(self.lighting_preview_state, LightingPreviewState::Idle)
    }

    fn refresh_open_lighting_modal(&mut self, validation_error: Option<String>) {
        if matches!(
            self.active_modal,
            Some(EditorModalProjection::LightingEditor(_))
        ) {
            match self.build_lighting_modal_projection(validation_error) {
                Ok(proj) => {
                    self.active_modal = Some(EditorModalProjection::LightingEditor(proj));
                }
                Err(_) => {
                    self.active_modal = None;
                }
            }
        }
    }

    fn resume_background_device_sync(&mut self) {
        self.refresh_four_app_lighting();
        self.sync_v070_session_sa_rgb();
        let _ = self.maybe_sync_v070_active_profile_layer();
    }

    fn present_lighting_preview_start_error(&mut self, msg: String) {
        self.status_message = msg.clone();
        if let Ok(proj) = self.build_lighting_modal_projection(Some(msg)) {
            self.active_modal = Some(EditorModalProjection::LightingEditor(proj));
        }
        self.commit_runtime_update();
    }

    fn acquire_lighting_preview_session(
        &self,
    ) -> Result<(Arc<DeviceSession>, Option<VendorBridgePreviewLease>), String> {
        if self.firmware_update.reserved() {
            return Err("固件更新流程正在占用设备，请结束后再试".to_string());
        }
        if self.six_layer_job.is_some() {
            return Err("六层操作正在进行，请稍后重试灯光预览".to_string());
        }
        if let Some(running) = &self.six_layer_running {
            let (session, lease) = running.acquire_preview_session()?;
            return Ok((session, Some(lease)));
        }
        let session = self
            .device_session
            .as_ref()
            .ok_or_else(|| "尚未连接已核验的 USB 专用独占键盘，无法启动硬件灯光预览".to_string())?;
        if session.is_disconnected() || !session.has_exclusive_vendor_link() {
            return Err("需要已连接的 USB 专用独占会话".to_string());
        }
        if !self.hardware_rpc_verified {
            return Err("正在核验 USB 键盘会话，请稍后再试".to_string());
        }
        if session.request_count() >= 900 {
            return Err("当前 USB 会话请求配额即将耗尽，请稍后重试预览".to_string());
        }
        Ok((Arc::clone(session), None))
    }

    pub(crate) fn begin_lighting_preview(&mut self) -> Result<(), WorkbenchError> {
        self.require_awake()?;
        self.ensure_layer_editable()?;
        if self.is_lighting_preview_busy() {
            let msg = match self.lighting_preview_state {
                LightingPreviewState::Starting { .. } => "灯光预览正在启动，请稍候",
                LightingPreviewState::Active { .. } => "灯光预览已在进行中",
                LightingPreviewState::Stopping { .. } => "上一轮灯光预览正在停止，请稍候重试",
                LightingPreviewState::Idle => unreachable!(),
            }
            .to_string();
            self.present_lighting_preview_start_error(msg.clone());
            return Err(WorkbenchError::Routing(msg));
        }
        let (pid, lid) = self.require_selected_profile_and_layer()?;
        let lights = match self.document_store.draft.document.lighting(&lid) {
            Ok(lights) => lights,
            Err(_) => {
                let msg = "当前图层尚未配置双区灯效，请先初始化灯效后再预览".to_string();
                self.present_lighting_preview_start_error(msg.clone());
                return Err(WorkbenchError::Routing(msg));
            }
        };
        if let Err(err) = validate_lighting_for_preview(&lights) {
            let msg = format!("当前灯光配置无效，无法启动硬件预览：{err}");
            self.present_lighting_preview_start_error(msg.clone());
            return Err(WorkbenchError::Routing(msg));
        }
        let (session, lease) = if self
            .config
            .lighting_preview_starter
            .requires_verified_session()
        {
            match self.acquire_lighting_preview_session() {
                Ok((session, lease)) => (Some(session), lease),
                Err(msg) => {
                    self.present_lighting_preview_start_error(msg.clone());
                    return Err(WorkbenchError::Routing(msg));
                }
            }
        } else {
            (None, None)
        };

        self.sa_rgb_coordinator.sync(
            crate::v070_session_sa_rgb::DesiredTarget::Inactive,
            None,
            &self.tokio_runtime,
        );
        self.lighting_arbiter.invalidate_hardware_cache();

        self.lighting_preview_generation = self.lighting_preview_generation.wrapping_add(1).max(1);
        let generation = self.lighting_preview_generation;
        let starter = Arc::clone(&self.config.lighting_preview_starter);
        let inbox = self.hardware_event_tx.clone();
        let initial_lights = lights.clone();
        let join_handle = self.tokio_runtime.spawn(async move {
            let result = starter.begin_future(session, initial_lights).await;
            let _ = inbox.send(HardwareLoopEvent::LightingPreviewStarted { generation, result });
        });
        self.lighting_preview_state = LightingPreviewState::Starting {
            generation,
            profile_id: pid,
            layer_id: lid,
            abort_handle: join_handle.abort_handle(),
            lease,
            latest_lights: lights,
            lights_dirty: false,
        };
        self.lighting_preview_active = false;
        self.lighting_preview_remaining_secs = 0;
        self.status_message = "正在启动 30 秒硬件灯光预览…".to_string();
        let proj = self.build_lighting_modal_projection(None)?;
        self.active_modal = Some(EditorModalProjection::LightingEditor(proj));
        self.commit_runtime_update();
        Ok(())
    }

    pub(crate) fn end_lighting_preview(&mut self, reason: LightingPreviewStopReason) {
        let state = std::mem::replace(&mut self.lighting_preview_state, LightingPreviewState::Idle);
        match state {
            LightingPreviewState::Idle => {
                self.lighting_preview_active = false;
                self.lighting_preview_remaining_secs = 0;
                if reason == LightingPreviewStopReason::UserRequested {
                    self.refresh_open_lighting_modal(None);
                }
            }
            LightingPreviewState::Starting {
                abort_handle,
                lease,
                ..
            } => {
                abort_handle.abort();
                drop(lease);
                self.lighting_preview_active = false;
                self.lighting_preview_remaining_secs = 0;
                self.apply_preview_stop_copy(reason);
                if self.should_resume_background_leds_after_stop(reason) {
                    self.resume_background_device_sync();
                }
            }
            LightingPreviewState::Active {
                generation,
                handle,
                lease,
                ..
            } => {
                let inbox = self.hardware_event_tx.clone();
                let join_handle = self.tokio_runtime.spawn(async move {
                    let _phase = handle.end_future().await;
                    drop(handle);
                    drop(lease);
                    let _ = inbox.send(HardwareLoopEvent::LightingPreviewEnded { generation });
                });
                self.lighting_preview_state = LightingPreviewState::Stopping {
                    generation,
                    join_handle,
                };
                self.lighting_preview_active = false;
                self.lighting_preview_remaining_secs = 0;
                self.apply_preview_stop_copy(reason);
            }
            LightingPreviewState::Stopping {
                generation,
                join_handle,
            } => {
                self.lighting_preview_state = LightingPreviewState::Stopping {
                    generation,
                    join_handle,
                };
                self.lighting_preview_active = false;
                self.lighting_preview_remaining_secs = 0;
                if reason == LightingPreviewStopReason::UserRequested {
                    self.refresh_open_lighting_modal(None);
                }
            }
        }
    }

    fn apply_preview_stop_copy(&mut self, reason: LightingPreviewStopReason) {
        match reason {
            LightingPreviewStopReason::UserRequested
            | LightingPreviewStopReason::LightingRemoved => {
                self.status_message = "硬件灯光预览已停止（未自动恢复设备运行态灯效）".to_string();
                self.refresh_open_lighting_modal(None);
            }
            LightingPreviewStopReason::Expired => {
                self.status_message =
                    "30 秒硬件灯光预览已自动停止（未自动恢复设备运行态灯效）".to_string();
                self.refresh_open_lighting_modal(None);
            }
            LightingPreviewStopReason::DeviceDisconnected => {
                self.refresh_open_lighting_modal(Some(
                    "设备连接已断开，灯光预览已停止（未自动恢复原灯效）".to_string(),
                ));
            }
            LightingPreviewStopReason::ModalClosed
            | LightingPreviewStopReason::ContextChanged
            | LightingPreviewStopReason::LayerProtected
            | LightingPreviewStopReason::ExclusiveDeviceTransition
            | LightingPreviewStopReason::SystemSleep
            | LightingPreviewStopReason::SystemTerminate => {
                self.refresh_open_lighting_modal(None);
            }
        }
    }

    fn should_resume_background_leds_after_stop(&self, reason: LightingPreviewStopReason) -> bool {
        self.system_lifecycle == SystemLifecycle::Awake
            && matches!(
                reason,
                LightingPreviewStopReason::UserRequested
                    | LightingPreviewStopReason::Expired
                    | LightingPreviewStopReason::ModalClosed
                    | LightingPreviewStopReason::ContextChanged
                    | LightingPreviewStopReason::LightingRemoved
            )
    }

    fn block_on_preview_teardown(&self, fut: impl Future<Output = ()> + Send + 'static) {
        if tokio::runtime::Handle::try_current().is_ok() {
            let rt = Arc::clone(&self.tokio_runtime);
            let _ = std::thread::spawn(move || rt.block_on(fut)).join();
        } else {
            self.tokio_runtime.block_on(fut);
        }
    }

    pub(crate) fn end_lighting_preview_blocking(&mut self, reason: LightingPreviewStopReason) {
        let state = std::mem::replace(&mut self.lighting_preview_state, LightingPreviewState::Idle);
        let was_running = !matches!(state, LightingPreviewState::Idle);
        match state {
            LightingPreviewState::Idle => {}
            LightingPreviewState::Starting {
                abort_handle,
                lease,
                ..
            } => {
                abort_handle.abort();
                drop(lease);
            }
            LightingPreviewState::Active { handle, lease, .. } => {
                let fut = async move {
                    let _ = tokio::time::timeout(
                        LIGHTING_PREVIEW_BLOCKING_STOP_TIMEOUT,
                        handle.end_future(),
                    )
                    .await;
                    handle.detach_on_drop();
                    drop(handle);
                    drop(lease);
                };
                self.block_on_preview_teardown(fut);
            }
            LightingPreviewState::Stopping {
                mut join_handle, ..
            } => {
                let fut = async move {
                    let abort = join_handle.abort_handle();
                    if tokio::time::timeout(
                        LIGHTING_PREVIEW_BLOCKING_STOP_TIMEOUT,
                        &mut join_handle,
                    )
                    .await
                    .is_err()
                    {
                        abort.abort();
                        let _ = tokio::time::timeout(Duration::from_millis(50), join_handle).await;
                    }
                };
                self.block_on_preview_teardown(fut);
            }
        }
        self.lighting_preview_active = false;
        self.lighting_preview_remaining_secs = 0;
        if was_running {
            self.apply_preview_stop_copy(reason);
        }
    }

    pub(crate) fn detach_lighting_preview_on_drop(&mut self) {
        let state = std::mem::replace(&mut self.lighting_preview_state, LightingPreviewState::Idle);
        match state {
            LightingPreviewState::Idle => {}
            LightingPreviewState::Starting {
                abort_handle,
                lease,
                ..
            } => {
                abort_handle.abort();
                drop(lease);
            }
            LightingPreviewState::Active { handle, lease, .. } => {
                handle.detach_on_drop();
                drop(handle);
                drop(lease);
            }
            LightingPreviewState::Stopping { join_handle, .. } => {
                join_handle.abort();
            }
        }
        self.lighting_preview_active = false;
        self.lighting_preview_remaining_secs = 0;
    }

    fn teardown_active_preview_on_unknown(&mut self, detail: String) -> String {
        let state = std::mem::replace(&mut self.lighting_preview_state, LightingPreviewState::Idle);
        if let LightingPreviewState::Active { handle, lease, .. } = state {
            handle.detach_on_drop();
            drop(handle);
            drop(lease);
        }
        self.lighting_preview_active = false;
        self.lighting_preview_remaining_secs = 0;
        let msg = normalize_preview_unknown_error(&detail);
        self.status_message = msg.clone();
        self.refresh_open_lighting_modal(Some(msg.clone()));
        if self.system_lifecycle == SystemLifecycle::Awake {
            self.resume_background_device_sync();
        }
        msg
    }

    fn handle_active_preview_write_error(&mut self, err: String) -> Option<String> {
        let phase = match &self.lighting_preview_state {
            LightingPreviewState::Active { handle, .. } => handle.phase(),
            _ => return None,
        };
        match phase {
            LightingPreviewPhase::Unknown(detail) => {
                Some(self.teardown_active_preview_on_unknown(detail))
            }
            LightingPreviewPhase::Stopped => {
                self.end_lighting_preview(LightingPreviewStopReason::Expired);
                None
            }
            LightingPreviewPhase::Starting | LightingPreviewPhase::Active => {
                Some(self.teardown_active_preview_on_unknown(err))
            }
        }
    }

    pub(crate) fn push_lighting_preview_edit(
        &mut self,
        layer_id: &str,
        zone: LightingZone,
        edit: LightingEdit,
    ) -> Option<String> {
        match &mut self.lighting_preview_state {
            LightingPreviewState::Starting {
                layer_id: preview_lid,
                latest_lights,
                lights_dirty,
                ..
            } if preview_lid == layer_id => {
                if let Ok(doc_lights) = self.document_store.draft.document.lighting(layer_id) {
                    *latest_lights = doc_lights;
                    *lights_dirty = true;
                }
                None
            }
            LightingPreviewState::Active {
                layer_id: preview_lid,
                handle,
                ..
            } if preview_lid == layer_id => match handle.update(zone, edit) {
                Ok(()) => None,
                Err(err) => self.handle_active_preview_write_error(err),
            },
            _ => None,
        }
    }

    pub(crate) fn sync_lighting_preview_for_layer(&mut self, layer_id: &str) -> Option<String> {
        let matches_layer = match &self.lighting_preview_state {
            LightingPreviewState::Starting {
                layer_id: preview_lid,
                ..
            }
            | LightingPreviewState::Active {
                layer_id: preview_lid,
                ..
            } => preview_lid == layer_id,
            LightingPreviewState::Idle | LightingPreviewState::Stopping { .. } => false,
        };
        if !matches_layer {
            return None;
        }
        if self
            .document_store
            .draft
            .document
            .is_protected_oai_layer(layer_id)
        {
            self.end_lighting_preview(LightingPreviewStopReason::LayerProtected);
            return None;
        }
        let Ok(lights) = self.document_store.draft.document.lighting(layer_id) else {
            self.end_lighting_preview(LightingPreviewStopReason::LightingRemoved);
            return None;
        };
        if validate_lighting_for_preview(&lights).is_err() {
            self.end_lighting_preview(LightingPreviewStopReason::LightingRemoved);
            return None;
        }
        match &mut self.lighting_preview_state {
            LightingPreviewState::Starting {
                latest_lights,
                lights_dirty,
                ..
            } => {
                if *latest_lights != lights {
                    *latest_lights = lights;
                    *lights_dirty = true;
                }
                None
            }
            LightingPreviewState::Active { handle, .. } => match handle.replace(lights) {
                Ok(()) => None,
                Err(err) => self.handle_active_preview_write_error(err),
            },
            LightingPreviewState::Idle | LightingPreviewState::Stopping { .. } => None,
        }
    }

    pub(crate) fn sync_lighting_preview_after_document_change(&mut self) {
        self.sync_modal_for_layer_protection();
        let active_target = match &self.lighting_preview_state {
            LightingPreviewState::Starting {
                profile_id,
                layer_id,
                ..
            }
            | LightingPreviewState::Active {
                profile_id,
                layer_id,
                ..
            } => Some((profile_id.clone(), layer_id.clone())),
            LightingPreviewState::Idle | LightingPreviewState::Stopping { .. } => None,
        };
        let mut refreshed = false;
        let preview_err = if let Some((pid, lid)) = active_target {
            if self.selected_profile_id.as_deref() != Some(pid.as_str())
                || self.selected_layer_id.as_deref() != Some(lid.as_str())
            {
                self.end_lighting_preview(LightingPreviewStopReason::ContextChanged);
                refreshed = true;
                None
            } else {
                self.sync_lighting_preview_for_layer(&lid)
            }
        } else {
            None
        };
        if !refreshed {
            self.refresh_open_lighting_modal(preview_err);
        }
    }

    pub(crate) fn handle_lighting_preview_started(
        &mut self,
        now: Instant,
        generation: u64,
        result: Result<Box<dyn LightingPreviewHandle>, String>,
    ) -> bool {
        let is_matching = matches!(
            &self.lighting_preview_state,
            LightingPreviewState::Starting {
                generation: cur, ..
            } if *cur == generation
        );
        if !is_matching || self.system_lifecycle != SystemLifecycle::Awake {
            if let Ok(handle) = result {
                handle.detach_on_drop();
            }
            return false;
        }
        let LightingPreviewState::Starting {
            generation,
            profile_id,
            layer_id,
            lease,
            latest_lights,
            lights_dirty,
            ..
        } = std::mem::replace(&mut self.lighting_preview_state, LightingPreviewState::Idle)
        else {
            unreachable!();
        };

        match result {
            Ok(handle) => {
                match handle.phase() {
                    LightingPreviewPhase::Unknown(detail) => {
                        handle.detach_on_drop();
                        drop(handle);
                        drop(lease);
                        let msg = normalize_preview_unknown_error(&detail);
                        self.lighting_preview_active = false;
                        self.lighting_preview_remaining_secs = 0;
                        self.status_message = msg.clone();
                        self.refresh_open_lighting_modal(Some(msg));
                        self.resume_background_device_sync();
                        return true;
                    }
                    LightingPreviewPhase::Stopped => {
                        handle.detach_on_drop();
                        drop(handle);
                        drop(lease);
                        self.lighting_preview_active = false;
                        self.lighting_preview_remaining_secs = 0;
                        self.status_message =
                            "硬件灯光预览已停止（未自动恢复设备运行态灯效）".to_string();
                        self.refresh_open_lighting_modal(None);
                        self.resume_background_device_sync();
                        return true;
                    }
                    LightingPreviewPhase::Starting | LightingPreviewPhase::Active => {}
                }
                if lights_dirty {
                    if let Err(err) = handle.replace(latest_lights) {
                        let phase = handle.phase();
                        handle.detach_on_drop();
                        drop(handle);
                        drop(lease);
                        self.lighting_preview_active = false;
                        self.lighting_preview_remaining_secs = 0;
                        match phase {
                            LightingPreviewPhase::Stopped => {
                                self.status_message =
                                    "硬件灯光预览已停止（未自动恢复设备运行态灯效）".to_string();
                                self.refresh_open_lighting_modal(None);
                            }
                            LightingPreviewPhase::Unknown(detail) => {
                                let msg = normalize_preview_unknown_error(&detail);
                                self.status_message = msg.clone();
                                self.refresh_open_lighting_modal(Some(msg));
                            }
                            LightingPreviewPhase::Starting | LightingPreviewPhase::Active => {
                                let msg = normalize_preview_unknown_error(&err);
                                self.status_message = msg.clone();
                                self.refresh_open_lighting_modal(Some(msg));
                            }
                        }
                        self.resume_background_device_sync();
                        return true;
                    }
                }
                let duration = self.config.lighting_preview_starter.preview_duration();
                let deadline = now + duration;
                let remaining = remaining_preview_secs(deadline, now);
                self.lighting_preview_state = LightingPreviewState::Active {
                    generation,
                    profile_id,
                    layer_id,
                    handle,
                    lease,
                    deadline,
                };
                self.lighting_preview_active = true;
                self.lighting_preview_remaining_secs = remaining;
                self.status_message =
                    "硬件灯光预览已开启（仅临时 lights.preview，不写入闪存）".to_string();
                self.refresh_open_lighting_modal(None);
                true
            }
            Err(err) => {
                drop(lease);
                let msg = normalize_preview_start_error(&err);
                self.lighting_preview_active = false;
                self.lighting_preview_remaining_secs = 0;
                self.status_message = msg.clone();
                self.refresh_open_lighting_modal(Some(msg));
                self.resume_background_device_sync();
                true
            }
        }
    }

    pub(crate) fn handle_lighting_preview_ended(&mut self, generation: u64) -> bool {
        let is_matching = matches!(
            &self.lighting_preview_state,
            LightingPreviewState::Stopping {
                generation: cur, ..
            } if *cur == generation
        );
        if !is_matching {
            return false;
        }
        self.lighting_preview_state = LightingPreviewState::Idle;
        self.lighting_preview_active = false;
        self.lighting_preview_remaining_secs = 0;
        self.refresh_open_lighting_modal(None);
        if self.system_lifecycle == SystemLifecycle::Awake {
            self.resume_background_device_sync();
        }
        true
    }

    pub(crate) fn poll_lighting_preview(&mut self, now: Instant) -> bool {
        let (phase, deadline) = match &self.lighting_preview_state {
            LightingPreviewState::Active {
                handle, deadline, ..
            } => (handle.phase(), *deadline),
            LightingPreviewState::Idle
            | LightingPreviewState::Starting { .. }
            | LightingPreviewState::Stopping { .. } => return false,
        };
        match phase {
            LightingPreviewPhase::Unknown(detail) => {
                self.teardown_active_preview_on_unknown(detail);
                true
            }
            LightingPreviewPhase::Stopped => {
                self.end_lighting_preview(LightingPreviewStopReason::Expired);
                true
            }
            LightingPreviewPhase::Starting | LightingPreviewPhase::Active => {
                if now >= deadline {
                    self.end_lighting_preview(LightingPreviewStopReason::Expired);
                    return true;
                }
                let next_remaining = remaining_preview_secs(deadline, now);
                if self.lighting_preview_remaining_secs != next_remaining
                    || !self.lighting_preview_active
                {
                    self.lighting_preview_active = true;
                    self.lighting_preview_remaining_secs = next_remaining;
                    self.refresh_open_lighting_modal(None);
                    return true;
                }
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mi_proto::Framer;
    use mi_transport::{HidLink, LinkBackend, MockLink, TransportError};

    struct ExclusiveNativeLink(Arc<MockLink>);

    impl HidLink for ExclusiveNativeLink {
        fn backend(&self) -> LinkBackend {
            LinkBackend::MacHidapi267Numbered
        }

        fn is_exclusive_vendor(&self) -> bool {
            true
        }

        fn write<'a, 'c, 'b>(
            &'a self,
            report: &'c [u8],
        ) -> Pin<Box<dyn Future<Output = Result<(), TransportError>> + Send + 'b>>
        where
            'a: 'b,
            'c: 'b,
            Self: 'b,
        {
            let link = Arc::clone(&self.0);
            let bytes = report.to_vec();
            Box::pin(async move { link.write(&bytes).await })
        }

        fn read<'a, 'b>(
            &'a self,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, TransportError>> + Send + 'b>>
        where
            'a: 'b,
            Self: 'b,
        {
            let link = Arc::clone(&self.0);
            Box::pin(async move { link.read().await })
        }

        fn close(&self) {
            self.0.close();
        }
    }

    struct NativeMockSession {
        session: Arc<DeviceSession>,
        requests: Arc<Mutex<Vec<Value>>>,
        fail_preview: Arc<AtomicBool>,
        responder: tokio::task::JoinHandle<()>,
    }

    fn spawn_native_mock_session(
        runtime: &tokio::runtime::Runtime,
        firmware_version: &'static str,
    ) -> NativeMockSession {
        let (link, mut outgoing) = MockLink::new();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&requests);
        let fail_preview = Arc::new(AtomicBool::new(false));
        let fault = Arc::clone(&fail_preview);
        let incoming = Arc::clone(&link);
        let responder = runtime.spawn(async move {
            let mut framer = mi_proto::NativeFramer::default();
            while let Some(report) = outgoing.recv().await {
                for line in framer.feed(&report).unwrap() {
                    let request: Value = serde_json::from_slice(&line).unwrap();
                    recorded.lock().unwrap().push(request.clone());
                    let method = request["method"].as_str().unwrap_or("");
                    let reply = if method == "sys.version" || method == "device.status" {
                        json!({"id": request["id"], "result": {"version": firmware_version}})
                    } else if method == "lights.preview" && fault.load(Ordering::Acquire) {
                        json!({"id": request["id"], "error": {"code": 400, "message": "fixture rejected"}})
                    } else if method == "kb.pl.set" || method == "kb.sa.rgb" {
                        json!({"id": request["id"], "method": method, "result": {}})
                    } else {
                        json!({"id": request["id"], "result": null})
                    };
                    for frame in framer.encode(reply.to_string().as_bytes()).unwrap() {
                        let _ = incoming.inject.send(frame);
                    }
                }
            }
        });
        let session = {
            let _entered = runtime.enter();
            DeviceSession::new(
                Arc::new(ExclusiveNativeLink(link)),
                Box::<mi_proto::NativeFramer>::default(),
                Arc::new(mi_session::ReadOnlySchema),
                mi_session::SessionConfig {
                    timeout: Duration::from_millis(300),
                    interval: Duration::ZERO,
                },
            )
        };
        NativeMockSession {
            session,
            requests,
            fail_preview,
            responder,
        }
    }

    fn wait_for_condition(mut pred: impl FnMut() -> bool) {
        let limit = Instant::now() + Duration::from_secs(3);
        while !pred() {
            assert!(Instant::now() < limit, "timed out waiting for condition");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn wait_for_preview_frames(requests: &Mutex<Vec<Value>>, min_preview_count: usize) {
        wait_for_condition(|| {
            requests
                .lock()
                .unwrap()
                .iter()
                .filter(|r| r["method"] == "lights.preview")
                .count()
                >= min_preview_count
        });
    }

    #[test]
    fn native_preview_starter_emits_lights_preview_frames_on_begin_edit_reset_and_clean_end_without_restore(
    ) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(dir.path()).unwrap();
        store
            .document_store
            .open(dir.path().join("workbench.json"))
            .unwrap();
        let mut base_raw = mi_model::demo().expect("load demo device config").raw;
        base_raw["profiles"][0]["layers"][0]["lights"] = json!({
            "backlight": {
                "effect": "solid",
                "brightness": 0.75,
                "speed": 0.4,
                "magic": 0.2,
                "color": 0x112233u32
            },
            "underglow": {
                "effect": "breath",
                "brightness": 0.6,
                "speed": 0.7,
                "magic": 0.3,
                "color": 0x654321u32
            }
        });
        let keymap_bytes = serde_json::to_vec(&base_raw).unwrap();
        let base_cfg = mi_model::parse_device_files(&keymap_bytes, None).unwrap();
        let keymap_file = mi_model::operation_record::file_value(&keymap_bytes);
        let snapshot = json!({
            "firmware": "0.6.3",
            "device_identity": {
                "confidence": "verified-serial",
                "serial": "native-preview-test",
                "session_id": 1
            },
            "keymap": keymap_file,
            "smart_actions": { "state": "missing" },
            "files": []
        });
        store
            .document_store
            .create_independent(base_cfg, snapshot)
            .unwrap();
        let pid = store
            .document_store
            .draft
            .document
            .editable
            .graph
            .profile_order[0]
            .clone();
        let lid = store
            .document_store
            .draft
            .document
            .editable
            .graph
            .profiles
            .get(&pid)
            .unwrap()
            .layer_order[0]
            .clone();
        store.selected_profile_id = Some(pid);
        store.selected_layer_id = Some(lid.clone());
        store.commit_runtime_update();

        let source_lighting = store
            .document_store
            .draft
            .document
            .lighting(&lid)
            .expect("demo layer has baseline lighting");

        let NativeMockSession {
            session,
            requests,
            responder,
            ..
        } = spawn_native_mock_session(&store.tokio_runtime, "0.6.3");
        store.device_session = Some(Arc::clone(&session));
        store.hardware_rpc_verified = true;

        store
            .dispatch_editor_action(EditorAction::OpenLightingEditor)
            .unwrap();
        store
            .dispatch_editor_action(EditorAction::LightingBeginPreview)
            .unwrap();
        wait_for_condition(|| {
            store.poll_at(Instant::now());
            matches!(
                store.active_modal(),
                Some(EditorModalProjection::LightingEditor(m)) if m.preview_active
            )
        });
        wait_for_preview_frames(&requests, 1);

        store
            .dispatch_editor_action(EditorAction::LightingEditColorHex {
                zone: LightingZone::Backlight,
                color_hex: "#445566".into(),
                all_layers: false,
            })
            .unwrap();
        wait_for_preview_frames(&requests, 2);
        {
            let recorded = requests.lock().unwrap();
            let preview_frames: Vec<&Value> = recorded
                .iter()
                .filter(|r| r["method"] == "lights.preview")
                .collect();
            assert_eq!(preview_frames[1]["params"]["backlight"]["color"], 0x445566);
        }

        store
            .dispatch_editor_action(EditorAction::LightingResetLayer)
            .unwrap();
        wait_for_preview_frames(&requests, 3);
        {
            let recorded = requests.lock().unwrap();
            let preview_frames: Vec<&Value> = recorded
                .iter()
                .filter(|r| r["method"] == "lights.preview")
                .collect();
            assert_eq!(
                preview_frames[2]["params"]["backlight"]["color"],
                source_lighting.backlight["color"]
            );
        }

        let count_before_end = requests.lock().unwrap().len();
        store
            .dispatch_editor_action(EditorAction::LightingEndPreview)
            .unwrap();
        wait_for_condition(|| {
            store.poll_at(Instant::now());
            !store.is_lighting_preview_busy()
        });
        std::thread::sleep(Duration::from_millis(100));
        {
            let recorded = requests.lock().unwrap();
            assert_eq!(
                recorded.len(),
                count_before_end,
                "native preview end must emit zero restore or file-write wire frames"
            );
            assert!(recorded.iter().all(|r| {
                matches!(
                    r["method"].as_str(),
                    Some("sys.version" | "device.status" | "lights.preview")
                )
            }));
        }
        assert!(store.status_message().contains("未自动恢复"));

        if let Some(session) = store.device_session.take() {
            store.tokio_runtime.block_on(session.shutdown());
        }
        responder.abort();
    }

    #[test]
    fn native_preview_rpc_failure_disconnects_without_retry_and_suspend_drains_synchronously() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(dir.path()).unwrap();
        store
            .dispatch_editor_action(EditorAction::OpenLightingEditor)
            .unwrap();
        store
            .dispatch_editor_action(EditorAction::LightingAddDefault)
            .unwrap();

        let NativeMockSession {
            session: session1,
            requests: requests1,
            fail_preview: fail1,
            responder: responder1,
        } = spawn_native_mock_session(&store.tokio_runtime, "0.6.3");
        store.device_session = Some(Arc::clone(&session1));
        store.hardware_rpc_verified = true;
        store
            .dispatch_editor_action(EditorAction::LightingBeginPreview)
            .unwrap();
        wait_for_condition(|| {
            store.poll_at(Instant::now());
            matches!(
                store.active_modal(),
                Some(EditorModalProjection::LightingEditor(m)) if m.preview_active
            )
        });
        fail1.store(true, Ordering::Release);
        store
            .dispatch_editor_action(EditorAction::LightingEditColorHex {
                zone: LightingZone::Backlight,
                color_hex: "#ABCDEF".into(),
                all_layers: false,
            })
            .unwrap();
        wait_for_preview_frames(&requests1, 2);
        wait_for_condition(|| {
            store.poll_at(Instant::now());
            !store.is_lighting_preview_busy()
        });
        assert!(session1.is_disconnected());
        if let Some(EditorModalProjection::LightingEditor(modal)) = store.active_modal() {
            assert!(!modal.preview_active);
            assert!(modal
                .validation_error
                .as_deref()
                .is_some_and(|e| e.contains("未自动恢复")));
        } else {
            panic!("expected LightingEditor modal after RPC error");
        }
        let preview_count_after_fail = requests1
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["method"] == "lights.preview")
            .count();
        std::thread::sleep(Duration::from_millis(120));
        assert_eq!(
            requests1
                .lock()
                .unwrap()
                .iter()
                .filter(|r| r["method"] == "lights.preview")
                .count(),
            preview_count_after_fail
        );
        if let Some(s) = store.device_session.take() {
            store.tokio_runtime.block_on(s.shutdown());
        }
        responder1.abort();

        let NativeMockSession {
            session: session2,
            requests: requests2,
            responder: responder2,
            ..
        } = spawn_native_mock_session(&store.tokio_runtime, "0.7.0");
        let mut running =
            crate::six_layer_runtime::RunningSixLayer::isolated(Arc::clone(&session2), dir.path());
        running.attach_test_vendor_session(&store.tokio_runtime, Arc::clone(&session2));
        store.six_layer_running = Some(running);

        store
            .dispatch_editor_action(EditorAction::LightingBeginPreview)
            .unwrap();
        wait_for_condition(|| {
            store.poll_at(Instant::now());
            matches!(
                store.active_modal(),
                Some(EditorModalProjection::LightingEditor(m)) if m.preview_active
            )
        });
        wait_for_preview_frames(&requests2, 1);

        store.suspend_for_six_layer_operation().unwrap();
        assert!(!store.is_lighting_preview_busy());
        assert!(!store.lighting_preview_active);
        assert!(store.six_layer_running.is_none());
        assert!(session2.is_disconnected());
        responder2.abort();
    }

    #[test]
    fn direct_mode_unverified_session_rejects_preview_and_normalizes_errors() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(dir.path()).unwrap();
        store
            .dispatch_editor_action(EditorAction::OpenLightingEditor)
            .unwrap();
        store
            .dispatch_editor_action(EditorAction::LightingAddDefault)
            .unwrap();

        let NativeMockSession {
            session,
            requests,
            responder,
            ..
        } = spawn_native_mock_session(&store.tokio_runtime, "0.6.3");
        store.device_session = Some(Arc::clone(&session));
        store.hardware_rpc_verified = false;

        let err = store
            .dispatch_editor_action(EditorAction::LightingBeginPreview)
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("正在核验 USB 键盘会话，请稍后再试"));
        assert!(!store.is_lighting_preview_busy());
        assert_eq!(store.status_message(), "正在核验 USB 键盘会话，请稍后再试");
        assert!(requests.lock().unwrap().is_empty());
        if let Some(EditorModalProjection::LightingEditor(modal)) = store.active_modal() {
            assert_eq!(
                modal.validation_error.as_deref(),
                Some("正在核验 USB 键盘会话，请稍后再试")
            );
        } else {
            panic!("expected LightingEditor modal");
        }

        assert_eq!(
            normalize_preview_start_error("正在核验 USB 键盘会话，请稍后再试"),
            "正在核验 USB 键盘会话，请稍后再试"
        );
        assert_eq!(
            normalize_preview_start_error("channel closed"),
            "设备预览通道已关闭，无法启动灯光预览"
        );
        assert_ne!(
            crate::i18n::tr(
                "正在核验 USB 键盘会话，请稍后再试",
                mi_model::ResolvedLocale::En
            ),
            "正在核验 USB 键盘会话，请稍后再试"
        );
        assert_ne!(
            crate::i18n::tr(
                "设备预览通道已关闭，无法启动灯光预览",
                mi_model::ResolvedLocale::En
            ),
            "设备预览通道已关闭，无法启动灯光预览"
        );

        if let Some(s) = store.device_session.take() {
            store.tokio_runtime.block_on(s.shutdown());
        }
        responder.abort();
    }

    #[test]
    fn v070_auto_layer_sync_defers_during_preview_and_explicit_command_tears_down_preview() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(dir.path()).unwrap();
        let ag_id = store
            .find_canonical_profile_id(CanonicalAgent::Antigravity)
            .expect("Antigravity profile");
        store.select_and_activate_profile(&ag_id).unwrap();
        store.set_host_execution_enabled(true).unwrap();
        store
            .document_store
            .set_v070_auto_layer_sync_enabled(true)
            .unwrap();

        store
            .dispatch_editor_action(EditorAction::OpenLightingEditor)
            .unwrap();
        store
            .dispatch_editor_action(EditorAction::LightingAddDefault)
            .unwrap();

        let NativeMockSession {
            session,
            requests,
            responder,
            ..
        } = spawn_native_mock_session(&store.tokio_runtime, "0.7.0-rc.16");
        store
            .tokio_runtime
            .block_on(session.read_identity_info())
            .unwrap();
        store.device_session = Some(Arc::clone(&session));
        store.hardware_rpc_verified = true;
        store
            .v070_hardware
            .mark_supported("0.7.0-rc.16", Some(0), Some(1), None, None);

        store
            .dispatch_editor_action(EditorAction::LightingBeginPreview)
            .unwrap();
        wait_for_condition(|| {
            store.poll_at(Instant::now());
            matches!(
                store.active_modal(),
                Some(EditorModalProjection::LightingEditor(m)) if m.preview_active
            )
        });
        wait_for_preview_frames(&requests, 1);

        assert!(
            !store.maybe_sync_v070_active_profile_layer(),
            "auto layer sync must defer while preview is busy"
        );
        assert!(
            !requests
                .lock()
                .unwrap()
                .iter()
                .any(|r| r["method"] == "kb.pl.set"),
            "kb.pl.set must not be sent while preview is active"
        );

        store
            .dispatch_editor_action(EditorAction::LightingEndPreview)
            .unwrap();
        wait_for_condition(|| {
            store.poll_at(Instant::now());
            !store.is_lighting_preview_busy()
                && requests
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|r| r["method"] == "kb.pl.set")
        });
        wait_for_condition(|| {
            store.poll_at(Instant::now());
            !store.v070_hardware.projection.busy
        });

        store
            .dispatch_editor_action(EditorAction::LightingBeginPreview)
            .unwrap();
        wait_for_condition(|| {
            store.poll_at(Instant::now());
            matches!(
                store.active_modal(),
                Some(EditorModalProjection::LightingEditor(m)) if m.preview_active
            )
        });

        let pl_before = requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["method"] == "kb.pl.set")
            .count();
        store
            .dispatch_v070_hardware(crate::V070HardwareAction::SelectLayer { layer: 3 })
            .expect("explicit v070 device command must tear down preview and dispatch");
        assert!(
            !store.is_lighting_preview_busy(),
            "explicit v070 device command must end preview synchronously before wire dispatch"
        );
        wait_for_condition(|| {
            store.poll_at(Instant::now());
            requests
                .lock()
                .unwrap()
                .iter()
                .filter(|r| r["method"] == "kb.pl.set")
                .count()
                > pl_before
        });

        if let Some(s) = store.device_session.take() {
            store.tokio_runtime.block_on(s.shutdown());
        }
        responder.abort();
    }

    #[derive(Debug)]
    struct ControlledPreviewHandle {
        phase: Mutex<LightingPreviewPhase>,
        replace_err: Option<String>,
        end_gate: Option<Arc<tokio::sync::Notify>>,
    }

    impl LightingPreviewHandle for ControlledPreviewHandle {
        fn update(
            &self,
            _zone: mi_model::lighting::Zone,
            _edit: mi_model::lighting::Edit,
        ) -> Result<(), String> {
            Ok(())
        }

        fn replace(&self, _lights: mi_model::lighting::Lighting) -> Result<(), String> {
            match &self.replace_err {
                Some(err) => Err(err.clone()),
                None => Ok(()),
            }
        }

        fn phase(&self) -> LightingPreviewPhase {
            self.phase.lock().unwrap().clone()
        }

        fn end_future(&self) -> LightingPreviewEndFuture {
            let gate = self.end_gate.clone();
            Box::pin(async move {
                if let Some(g) = gate {
                    g.notified().await;
                }
                LightingPreviewPhase::Stopped
            })
        }

        fn detach_on_drop(&self) {}
    }

    struct ControlledPreviewStarter {
        next_gate: Mutex<Option<Arc<tokio::sync::Notify>>>,
        next_handle: Mutex<Option<Box<dyn LightingPreviewHandle>>>,
    }

    impl LightingPreviewStarter for ControlledPreviewStarter {
        fn requires_verified_session(&self) -> bool {
            false
        }

        fn begin_future(
            &self,
            _session: Option<Arc<DeviceSession>>,
            _lights: mi_model::lighting::Lighting,
        ) -> LightingPreviewBeginFuture {
            let gate = self.next_gate.lock().unwrap().take();
            let handle = self.next_handle.lock().unwrap().take().unwrap();
            Box::pin(async move {
                if let Some(g) = gate {
                    g.notified().await;
                }
                Ok(handle)
            })
        }
    }

    #[test]
    fn stopping_validation_error_clears_on_end_and_lights_dirty_stopped_uses_clean_copy() {
        let dir = tempfile::tempdir().unwrap();
        let end_gate = Arc::new(tokio::sync::Notify::new());
        let starter = Arc::new(ControlledPreviewStarter {
            next_gate: Mutex::new(None),
            next_handle: Mutex::new(Some(Box::new(ControlledPreviewHandle {
                phase: Mutex::new(LightingPreviewPhase::Active),
                replace_err: None,
                end_gate: Some(Arc::clone(&end_gate)),
            }))),
        });
        let mut cfg = WorkbenchStoreConfig::isolated(dir.path());
        cfg.lighting_preview_starter = Arc::clone(&starter) as Arc<dyn LightingPreviewStarter>;
        let mut store = WorkbenchStore::open(cfg).unwrap();
        store
            .dispatch_editor_action(EditorAction::OpenLightingEditor)
            .unwrap();
        store
            .dispatch_editor_action(EditorAction::LightingAddDefault)
            .unwrap();

        store
            .dispatch_editor_action(EditorAction::LightingBeginPreview)
            .unwrap();
        wait_for_condition(|| {
            store.poll_at(Instant::now());
            matches!(
                store.active_modal(),
                Some(EditorModalProjection::LightingEditor(m)) if m.preview_active
            )
        });

        store
            .dispatch_editor_action(EditorAction::LightingEndPreview)
            .unwrap();
        let retry_err = store
            .dispatch_editor_action(EditorAction::LightingBeginPreview)
            .unwrap_err();
        assert!(retry_err.to_string().contains("上一轮灯光预览正在停止"));
        if let Some(EditorModalProjection::LightingEditor(modal)) = store.active_modal() {
            assert_eq!(
                modal.validation_error.as_deref(),
                Some("上一轮灯光预览正在停止，请稍候重试")
            );
        } else {
            panic!("expected LightingEditor modal");
        }

        end_gate.notify_one();
        wait_for_condition(|| {
            store.poll_at(Instant::now());
            !store.is_lighting_preview_busy()
        });
        if let Some(EditorModalProjection::LightingEditor(modal)) = store.active_modal() {
            assert_eq!(
                modal.validation_error, None,
                "stopping validation error must clear when Stopping -> Idle completes"
            );
        } else {
            panic!("expected LightingEditor modal");
        }

        let start_gate = Arc::new(tokio::sync::Notify::new());
        *starter.next_gate.lock().unwrap() = Some(Arc::clone(&start_gate));
        let dirty_handle_phase = Arc::new(Mutex::new(LightingPreviewPhase::Active));
        #[derive(Debug)]
        struct StoppedOnReplaceHandle {
            phase: Arc<Mutex<LightingPreviewPhase>>,
        }
        impl LightingPreviewHandle for StoppedOnReplaceHandle {
            fn update(
                &self,
                _zone: mi_model::lighting::Zone,
                _edit: mi_model::lighting::Edit,
            ) -> Result<(), String> {
                Ok(())
            }
            fn replace(&self, _lights: mi_model::lighting::Lighting) -> Result<(), String> {
                *self.phase.lock().unwrap() = LightingPreviewPhase::Stopped;
                Err("preview has ended".into())
            }
            fn phase(&self) -> LightingPreviewPhase {
                self.phase.lock().unwrap().clone()
            }
            fn end_future(&self) -> LightingPreviewEndFuture {
                Box::pin(async { LightingPreviewPhase::Stopped })
            }
            fn detach_on_drop(&self) {}
        }
        *starter.next_handle.lock().unwrap() = Some(Box::new(StoppedOnReplaceHandle {
            phase: Arc::clone(&dirty_handle_phase),
        }));

        store
            .dispatch_editor_action(EditorAction::LightingBeginPreview)
            .unwrap();
        store
            .dispatch_editor_action(EditorAction::LightingEditColorHex {
                zone: LightingZone::Backlight,
                color_hex: "#223344".into(),
                all_layers: false,
            })
            .unwrap();
        start_gate.notify_one();
        wait_for_condition(|| {
            store.poll_at(Instant::now());
            !store.is_lighting_preview_busy()
        });
        assert_eq!(
            store.status_message(),
            "硬件灯光预览已停止（未自动恢复设备运行态灯效）"
        );
        if let Some(EditorModalProjection::LightingEditor(modal)) = store.active_modal() {
            assert!(!modal.preview_active);
            assert_eq!(modal.validation_error, None);
        } else {
            panic!("expected LightingEditor modal");
        }
    }

    #[test]
    fn poll_six_layer_runtime_disconnect_with_busy_preview_commits_projection_update() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(dir.path()).unwrap();
        store
            .dispatch_editor_action(EditorAction::OpenLightingEditor)
            .unwrap();
        store
            .dispatch_editor_action(EditorAction::LightingAddDefault)
            .unwrap();

        let NativeMockSession {
            session, responder, ..
        } = spawn_native_mock_session(&store.tokio_runtime, "0.7.0");
        let mut running =
            crate::six_layer_runtime::RunningSixLayer::isolated(Arc::clone(&session), dir.path());
        running.attach_test_vendor_session(&store.tokio_runtime, Arc::clone(&session));
        store.six_layer_running = Some(running);

        store
            .dispatch_editor_action(EditorAction::LightingBeginPreview)
            .unwrap();
        wait_for_condition(|| {
            store.poll_at(Instant::now());
            matches!(
                store.active_modal(),
                Some(EditorModalProjection::LightingEditor(m)) if m.preview_active
            )
        });

        let failure_msg = "USB 独占会话已失效，输入已暂停";
        store.status_message = failure_msg.to_string();
        store.commit_runtime_update();
        let gen_before = store.projection_generation();
        store
            .six_layer_running
            .as_ref()
            .unwrap()
            .receiver
            .invalidate(failure_msg);

        store.poll_six_layer_runtime();
        assert!(!store.is_lighting_preview_busy());
        assert!(
            store.projection_generation() > gen_before,
            "poll_six_layer_runtime must commit updated modal state when disconnecting an active preview"
        );
        if let Some(EditorModalProjection::LightingEditor(modal)) = store.active_modal() {
            assert!(!modal.preview_active);
            assert!(modal
                .validation_error
                .as_deref()
                .is_some_and(|e| e.contains("设备连接已断开")));
        } else {
            panic!("expected LightingEditor modal");
        }

        store
            .six_layer_running
            .take()
            .unwrap()
            .shutdown(&store.tokio_runtime)
            .unwrap();
        responder.abort();
    }
}
