use serde::{Deserialize, Serialize};

/// Mode of voice reply for an agent target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoiceReplyMode {
    /// Explicitly supported by the agent, with current enabled/disabled state.
    Supported(bool),
    /// Unsupported by the agent model or protocol interface.
    /// Crucially, unsupported targets remain incomplete and are never masked
    /// as "disabled" or "success".
    Unsupported { app: String, reason: String },
}

impl VoiceReplyMode {
    pub fn is_supported(&self) -> bool {
        matches!(self, Self::Supported(_))
    }

    pub fn is_enabled(&self) -> bool {
        match self {
            Self::Supported(v) => *v,
            Self::Unsupported { .. } => false,
        }
    }
}

/// State of voice dictation session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoiceDictationPhase {
    Idle,
    Recording {
        adapter: String,
        target: String,
        started_at_ms: u64,
    },
    Stopping {
        adapter: String,
        target: String,
    },
    Stopped {
        adapter: String,
        target: String,
    },
}

/// Managed session for hold-to-dictate and submit separation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceDictationSession {
    pub phase: VoiceDictationPhase,
    pub transcribed_text: String,
    pub submitted: bool,
}

impl Default for VoiceDictationSession {
    fn default() -> Self {
        Self {
            phase: VoiceDictationPhase::Idle,
            transcribed_text: String::new(),
            submitted: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DictationError {
    AlreadyRecording,
    NotRecording,
    TargetMismatch { expected: String, actual: String },
    MustStopBeforeSubmit,
    EmptyTarget,
}

impl std::fmt::Display for DictationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyRecording => write!(f, "dictation already active"),
            Self::NotRecording => write!(f, "no active dictation to stop"),
            Self::TargetMismatch { expected, actual } => {
                write!(f, "target mismatch: expected {expected}, got {actual}")
            }
            Self::MustStopBeforeSubmit => {
                write!(f, "active dictation must be stopped before submitting")
            }
            Self::EmptyTarget => write!(f, "target address cannot be empty"),
        }
    }
}

impl std::error::Error for DictationError {}

impl VoiceDictationSession {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start dictation bound to a specific adapter and target.
    pub fn start(
        &mut self,
        adapter: &str,
        target: &str,
        now_ms: u64,
    ) -> Result<(), DictationError> {
        if target.trim().is_empty() {
            return Err(DictationError::EmptyTarget);
        }
        if matches!(self.phase, VoiceDictationPhase::Recording { .. }) {
            return Err(DictationError::AlreadyRecording);
        }
        self.phase = VoiceDictationPhase::Recording {
            adapter: adapter.to_string(),
            target: target.to_string(),
            started_at_ms: now_ms,
        };
        self.submitted = false;
        Ok(())
    }

    /// Update transcription buffer as audio is recognized.
    /// Strictly separated from sending.
    pub fn append_transcription(&mut self, text: &str) {
        if matches!(
            self.phase,
            VoiceDictationPhase::Recording { .. } | VoiceDictationPhase::Stopping { .. }
        ) {
            self.transcribed_text.push_str(text);
        }
    }

    /// Stop dictation on key release.
    pub fn stop(&mut self, adapter: &str, target: &str) -> Result<(), DictationError> {
        match &self.phase {
            VoiceDictationPhase::Recording {
                adapter: a,
                target: t,
                ..
            } => {
                if a != adapter || t != target {
                    return Err(DictationError::TargetMismatch {
                        expected: format!("{a}:{t}"),
                        actual: format!("{adapter}:{target}"),
                    });
                }
                self.phase = VoiceDictationPhase::Stopped {
                    adapter: adapter.to_string(),
                    target: target.to_string(),
                };
                Ok(())
            }
            _ => Err(DictationError::NotRecording),
        }
    }

    /// Submit action gate: prevents sending text while dictation is still recording.
    pub fn prepare_submit(&mut self) -> Result<String, DictationError> {
        if matches!(self.phase, VoiceDictationPhase::Recording { .. }) {
            return Err(DictationError::MustStopBeforeSubmit);
        }
        self.submitted = true;
        let payload = std::mem::take(&mut self.transcribed_text);
        self.phase = VoiceDictationPhase::Idle;
        Ok(payload)
    }
}

/// Static capabilities and known gaps for the four supported desktop applications.
pub struct FourAppVoice;

impl FourAppVoice {
    pub const APPS: [&'static str; 2] = ["antigravity", "dsh-desktop"];

    /// Whether the application supports voice dictation (hold-to-dictate).
    pub fn supports_dictation(app: &str) -> bool {
        Self::APPS.contains(&app)
    }

    /// Initial voice reply capability for the application.
    /// Antigravity 2.29.0 does NOT support voice reply; kept as an explicit incomplete gap.
    pub fn initial_voice_reply_mode(app: &str) -> VoiceReplyMode {
        match app {
            "dsh-desktop" => VoiceReplyMode::Supported(false),
            "antigravity" => VoiceReplyMode::Unsupported {
                app: "antigravity".into(),
                reason: "Antigravity 2.29.0 language_server does not expose speech generation or voice reply mode".into(),
            },
            other => VoiceReplyMode::Unsupported {
                app: other.into(),
                reason: format!("Unknown agent application: {other}"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dictation_lifecycle_and_target_binding() {
        let mut session = VoiceDictationSession::new();
        assert_eq!(session.phase, VoiceDictationPhase::Idle);

        // Target binding required
        assert!(session.start("dsh-desktop", "", 1000).is_err());
        assert!(session
            .start("dsh-desktop", "workspace#session1", 1000)
            .is_ok());

        // Cannot double start
        assert!(session
            .start("dsh-desktop", "workspace#session1", 1005)
            .is_err());

        // Transcription separated from send
        session.append_transcription("Hello world");
        assert_eq!(session.transcribed_text, "Hello world");
        assert!(!session.submitted);

        // Cannot submit while recording
        assert_eq!(
            session.prepare_submit(),
            Err(DictationError::MustStopBeforeSubmit)
        );

        // Target mismatch on stop
        assert!(session.stop("dsh-desktop", "other#session").is_err());

        // Successful stop
        assert!(session.stop("dsh-desktop", "workspace#session1").is_ok());
        assert_eq!(
            session.phase,
            VoiceDictationPhase::Stopped {
                adapter: "dsh-desktop".into(),
                target: "workspace#session1".into(),
            }
        );

        // Submit succeeds after stop
        let text = session.prepare_submit().unwrap();
        assert_eq!(text, "Hello world");
        assert_eq!(session.phase, VoiceDictationPhase::Idle);
        assert!(session.submitted);
    }

    #[test]
    fn test_four_app_capabilities_and_antigravity_gap_preserved() {
        for app in FourAppVoice::APPS {
            assert!(FourAppVoice::supports_dictation(app));
        }

        assert!(FourAppVoice::initial_voice_reply_mode("dsh-desktop").is_supported());

        // Antigravity voice reply is unsupported and kept incomplete
        let ag_mode = FourAppVoice::initial_voice_reply_mode("antigravity");
        assert!(!ag_mode.is_supported());
        match ag_mode {
            VoiceReplyMode::Unsupported { app, reason } => {
                assert_eq!(app, "antigravity");
                assert!(reason.contains("Antigravity 2.29.0"));
            }
            _ => panic!("Expected unsupported voice reply mode for Antigravity"),
        }
    }
}
