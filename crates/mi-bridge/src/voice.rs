use crate::ownership::{NativeOwnershipController, OwnershipState};
use mi_model::navigation_history::NavigationPlace;
use mi_model::voice_control::{FourAppVoice, VoiceReplyMode};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn place_target_app(place: &NavigationPlace) -> &str {
    match place {
        NavigationPlace::Antigravity { .. } => "antigravity",
        NavigationPlace::Cindy { .. } => "cindy",
        NavigationPlace::Dsh { .. } => "dsh-desktop",
        NavigationPlace::App { bundle_id } => bundle_id.as_str(),
        NavigationPlace::Session { adapter, .. } => adapter.as_str(),
    }
}

pub fn place_display_label(place: &NavigationPlace) -> String {
    match place {
        NavigationPlace::Antigravity {
            conversation_id,
            workspace_path,
        } => format!("{conversation_id}@{workspace_path}"),
        NavigationPlace::Cindy {
            workspace_path,
            session_id,
        } => format!("{session_id}@{workspace_path}"),
        NavigationPlace::Dsh {
            session_id,
            workspace_id,
            ..
        } => format!("{session_id}@{workspace_id}"),
        NavigationPlace::App { bundle_id } => bundle_id.clone(),
        NavigationPlace::Session { adapter, session } => format!("{adapter}:{session}"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FourAppDictationState {
    Idle,
    Recording {
        app: String,
        session: NavigationPlace,
        started_at_ms: u64,
    },
    Stopped {
        app: String,
        session: NavigationPlace,
    },
}

impl FourAppDictationState {
    pub fn is_recording(&self) -> bool {
        matches!(self, Self::Recording { .. })
    }

    pub fn active_app(&self) -> Option<&str> {
        match self {
            Self::Idle => None,
            Self::Recording { app, .. } | Self::Stopped { app, .. } => Some(app.as_str()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PushToTalkController {
    pub is_active: bool,
    pub cleanup_confirmed: bool,
    pub simulated: bool,
    pub active_binding: Option<String>,
    pub active_adapter: Option<String>,
    pub active_profile: Option<String>,
    next_start_outcome: (bool, String),
    next_verify_stopped: bool,
}

impl Default for PushToTalkController {
    fn default() -> Self {
        Self {
            is_active: false,
            cleanup_confirmed: true,
            simulated: true,
            active_binding: None,
            active_adapter: None,
            active_profile: None,
            next_start_outcome: (true, "按住听写已启动".into()),
            next_verify_stopped: true,
        }
    }
}

impl PushToTalkController {
    pub fn new_simulated() -> Self {
        Self::default()
    }

    pub fn set_next_start_outcome(&mut self, confirmed: bool, detail: impl Into<String>) {
        self.next_start_outcome = (confirmed, detail.into());
    }

    pub fn set_next_verify_stopped(&mut self, stopped: bool) {
        self.next_verify_stopped = stopped;
    }

    pub fn start_held_dictation(&mut self, adapter: &str, binding: &str, profile: &str) {
        self.active_adapter = Some(adapter.to_owned());
        self.active_binding = Some(binding.to_owned());
        self.active_profile = Some(profile.to_owned());
        self.is_active = self.next_start_outcome.0;
    }

    pub async fn wait_for_start_outcome(&mut self) -> (bool, String) {
        let outcome = self.next_start_outcome.clone();
        if !outcome.0 {
            self.is_active = false;
            self.active_adapter = None;
            self.active_binding = None;
            self.active_profile = None;
        }
        outcome
    }

    pub fn release(&mut self, binding: &str) {
        if self.active_binding.as_deref() == Some(binding) || self.active_binding.is_none() {
            self.is_active = false;
            self.active_binding = None;
            self.active_adapter = None;
            self.active_profile = None;
        }
    }

    pub fn cancel(&mut self) {
        self.is_active = false;
        self.active_binding = None;
        self.active_adapter = None;
        self.active_profile = None;
    }

    pub async fn wait_until_stopped(&mut self) {
        self.is_active = false;
    }

    pub async fn verify_stopped(&mut self) -> bool {
        self.cleanup_confirmed = self.next_verify_stopped;
        self.next_verify_stopped
    }
}

pub trait VoiceReplyBackend {
    fn read_voice_reply_enabled(&self, app: &str) -> bool;
    fn write_voice_reply_enabled(&mut self, app: &str, enabled: bool) -> (bool, String);
}

#[derive(Debug, Default, Clone)]
pub struct SimulatedVoiceReplyBackend {
    pub cindy_enabled: bool,
    pub dsh_enabled: bool,
}

impl VoiceReplyBackend for SimulatedVoiceReplyBackend {
    fn read_voice_reply_enabled(&self, app: &str) -> bool {
        match app {
            "cindy" => self.cindy_enabled,
            "dsh-desktop" => self.dsh_enabled,
            _ => false,
        }
    }

    fn write_voice_reply_enabled(&mut self, app: &str, enabled: bool) -> (bool, String) {
        match app {
            "cindy" => {
                self.cindy_enabled = enabled;
                (true, format!("cindy voice_reply={enabled}"))
            }
            "dsh-desktop" => {
                self.dsh_enabled = enabled;
                (true, format!("dsh-desktop voice_reply={enabled}"))
            }
            _ => (false, "unsupported".into()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FourAppVoiceManager {
    dictation_state: FourAppDictationState,
    voice_reply_modes: BTreeMap<String, VoiceReplyMode>,
}

impl Default for FourAppVoiceManager {
    fn default() -> Self {
        Self::new()
    }
}

impl FourAppVoiceManager {
    pub fn new() -> Self {
        let mut voice_reply_modes = BTreeMap::new();
        for app in FourAppVoice::APPS {
            voice_reply_modes.insert(app.to_owned(), FourAppVoice::initial_voice_reply_mode(app));
        }
        Self {
            dictation_state: FourAppDictationState::Idle,
            voice_reply_modes,
        }
    }

    pub fn dictation_state(&self) -> &FourAppDictationState {
        &self.dictation_state
    }

    pub fn confirm_dictation_stopped(&mut self) {
        if let FourAppDictationState::Recording { app, session, .. } = &self.dictation_state {
            self.dictation_state = FourAppDictationState::Stopped {
                app: app.clone(),
                session: session.clone(),
            };
        }
    }

    pub fn voice_reply_modes(&self) -> &BTreeMap<String, VoiceReplyMode> {
        &self.voice_reply_modes
    }

    pub async fn start_dictation(
        &mut self,
        app: &str,
        session: NavigationPlace,
        binding: &str,
        profile: &str,
        ownership: &NativeOwnershipController,
        host_execution_enabled: bool,
        ptt: &mut PushToTalkController,
    ) -> (bool, String) {
        if !FourAppVoice::supports_dictation(app) {
            return (false, format!("未接入的应用听写目标: {app}"));
        }
        let actual_app = place_target_app(&session);
        if actual_app != app {
            return (
                false,
                format!("听写目标不匹配: 期望 {app}，实际 {actual_app}"),
            );
        }
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        if app == "cindy" && matches!(ownership.state(), OwnershipState::Native { .. }) {
            self.dictation_state = FourAppDictationState::Recording {
                app: app.to_owned(),
                session,
                started_at_ms: now_ms,
            };
            return (true, "Cindy 复用原生语音；按住听写已启动".into());
        }
        if let FourAppDictationState::Recording {
            app: current_app,
            session: current_session,
            ..
        } = &self.dictation_state
        {
            return (
                false,
                format!(
                    "听写已在进行中: {current_app} [{}]",
                    place_display_label(current_session)
                ),
            );
        }
        let dispatch_open = ownership.allows_execution() || ownership.is_transient_reconfirming();
        if !dispatch_open || !host_execution_enabled || !ptt.cleanup_confirmed || ptt.is_active {
            return (false, "宿主所有权或听写清理门禁未就绪，未启动听写".into());
        }
        ptt.start_held_dictation(app, binding, profile);
        let (confirmed, detail) = ptt.wait_for_start_outcome().await;
        if !confirmed {
            return (false, detail);
        }
        self.dictation_state = FourAppDictationState::Recording {
            app: app.to_owned(),
            session,
            started_at_ms: now_ms,
        };
        (true, detail)
    }

    pub async fn stop_dictation(
        &mut self,
        app: &str,
        session: NavigationPlace,
        binding: &str,
        ownership: &NativeOwnershipController,
        ptt: &mut PushToTalkController,
    ) -> (bool, String) {
        let FourAppDictationState::Recording {
            app: rec_app,
            session: rec_session,
            ..
        } = &self.dictation_state
        else {
            return (false, "没有正在录音的听写会话".into());
        };
        if rec_app != app || rec_session != &session {
            return (
                false,
                format!("停止听写目标不匹配: 期望 {rec_app}, 实际 {app}"),
            );
        }
        if app == "cindy" && matches!(ownership.state(), OwnershipState::Native { .. }) {
            self.dictation_state = FourAppDictationState::Stopped {
                app: app.to_owned(),
                session,
            };
            return (true, "已确认 Cindy 原生听写停止；未发送消息".into());
        }
        ptt.release(binding);
        ptt.wait_until_stopped().await;
        let stopped = ptt.verify_stopped().await;
        self.dictation_state = FourAppDictationState::Stopped {
            app: app.to_owned(),
            session,
        };
        (
            stopped,
            if stopped {
                "已确认原听写停止；未发送消息".into()
            } else {
                "原听写停止未确认；未发送消息".into()
            },
        )
    }

    pub async fn stop_before_submit(
        &mut self,
        app: &str,
        ownership: &NativeOwnershipController,
        ptt: &mut PushToTalkController,
    ) -> (bool, String) {
        if let FourAppDictationState::Recording {
            app: rec_app,
            session,
            ..
        } = self.dictation_state.clone()
        {
            if rec_app == app {
                if app == "cindy" && matches!(ownership.state(), OwnershipState::Native { .. }) {
                    self.dictation_state = FourAppDictationState::Stopped {
                        app: app.to_owned(),
                        session,
                    };
                    return (true, "提交前已先停止活跃听写".into());
                }
                ptt.cancel();
                ptt.wait_until_stopped().await;
                let ptt_stopped = ptt.verify_stopped().await;
                self.dictation_state = FourAppDictationState::Stopped {
                    app: app.to_owned(),
                    session,
                };
                return (
                    ptt_stopped,
                    if ptt_stopped {
                        "提交前已先停止活跃听写".into()
                    } else {
                        "原听写停止未确认".into()
                    },
                );
            }
        }
        (true, format!("dictation_inactive:{app}"))
    }

    pub fn get_voice_reply_mode(
        &mut self,
        app: &str,
        backend: &dyn VoiceReplyBackend,
    ) -> VoiceReplyMode {
        let mode = match app {
            "cindy" | "dsh-desktop" => {
                VoiceReplyMode::Supported(backend.read_voice_reply_enabled(app))
            }
            "antigravity" => FourAppVoice::initial_voice_reply_mode("antigravity"),
            other => VoiceReplyMode::Unsupported {
                app: other.to_owned(),
                reason: format!("未接入的应用: {other}"),
            },
        };
        self.voice_reply_modes.insert(app.to_owned(), mode.clone());
        mode
    }

    pub fn set_voice_reply(
        &mut self,
        app: &str,
        enabled: bool,
        backend: &mut dyn VoiceReplyBackend,
    ) -> (bool, VoiceReplyMode, String) {
        match app {
            "cindy" | "dsh-desktop" => {
                let (_, detail) = backend.write_voice_reply_enabled(app, enabled);
                let readback = backend.read_voice_reply_enabled(app);
                let mode = VoiceReplyMode::Supported(readback);
                self.voice_reply_modes.insert(app.to_owned(), mode.clone());
                let verified = readback == enabled;
                let fallback = match app {
                    "cindy" => "Cindy 语音回复回读不一致",
                    _ => "DSH 语音回复回读不一致",
                };
                (
                    verified,
                    mode,
                    if verified { detail } else { fallback.into() },
                )
            }
            "antigravity" => {
                let mode = FourAppVoice::initial_voice_reply_mode("antigravity");
                self.voice_reply_modes.insert(app.to_owned(), mode.clone());
                (
                    false,
                    mode,
                    "Antigravity 2.29.0 模型/接口不支持语音回复，保持未完成缺口".into(),
                )
            }
            other => {
                let mode = VoiceReplyMode::Unsupported {
                    app: other.to_owned(),
                    reason: format!("未知应用: {other}"),
                };
                (false, mode, "未接入的应用操作".into())
            }
        }
    }

    pub fn toggle_voice_reply(
        &mut self,
        app: &str,
        backend: &mut dyn VoiceReplyBackend,
    ) -> (bool, VoiceReplyMode, String) {
        let current = self.get_voice_reply_mode(app, backend);
        match current {
            VoiceReplyMode::Supported(enabled) => self.set_voice_reply(app, !enabled, backend),
            VoiceReplyMode::Unsupported { .. } => (
                false,
                current,
                "目标应用模型/接口不支持语音回复，保持未完成缺口".into(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_start_dictation_fails_closed_when_ownership_blocked_or_driver_rejects() {
        let mut manager = FourAppVoiceManager::new();
        let mut ptt = PushToTalkController::new_simulated();
        let blocked_ownership = NativeOwnershipController::new(None, false);
        let session = NavigationPlace::antigravity("conv-1", "/tmp/ws");

        let (ok, reason) = manager
            .start_dictation(
                "antigravity",
                session.clone(),
                "AG10",
                "profile-ag",
                &blocked_ownership,
                true,
                &mut ptt,
            )
            .await;
        assert!(!ok);
        assert!(reason.contains("未就绪"));
        assert_eq!(manager.dictation_state(), &FourAppDictationState::Idle);
        assert!(!ptt.is_active);

        let isolated_ownership = NativeOwnershipController::new_isolated();
        ptt.set_next_start_outcome(false, "底层录音设备拒绝启动");
        let (ok2, reason2) = manager
            .start_dictation(
                "antigravity",
                session.clone(),
                "AG10",
                "profile-ag",
                &isolated_ownership,
                true,
                &mut ptt,
            )
            .await;
        assert!(!ok2);
        assert_eq!(reason2, "底层录音设备拒绝启动");
        assert_eq!(manager.dictation_state(), &FourAppDictationState::Idle);
        assert!(!ptt.is_active);

        ptt.set_next_start_outcome(true, "按住听写已启动");
        let (ok3, _) = manager
            .start_dictation(
                "antigravity",
                session.clone(),
                "AG10",
                "profile-ag",
                &isolated_ownership,
                true,
                &mut ptt,
            )
            .await;
        assert!(ok3);
        assert!(manager.dictation_state().is_recording());
        assert!(ptt.is_active);
    }
}
