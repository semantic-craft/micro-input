use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub fn valid_antigravity_id(value: &str, limit: usize) -> bool {
    !value.is_empty() && value.len() <= limit
}

fn default_true() -> bool {
    true
}

fn default_thinking_level() -> String {
    "high".into()
}

fn default_model_catalog() -> Vec<AntigravityModelOption> {
    AntigravityModelOption::default_catalog()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravityCapabilities {
    pub bundle_id: String,
    pub installed_version: String,
    pub language_server_cl: String,
    pub agentapi_commands: Vec<String>,
    pub sidecar_sdk_version: String,
    pub hook_events: Vec<String>,
    pub supports_session_catalog: bool,
    pub supports_target_focus: bool,
    pub supports_stop_turn: bool,
    pub supports_approval_gate: bool,
    #[serde(default = "default_true")]
    pub supports_model_config: bool,
    #[serde(default = "default_true")]
    pub schema_verified: bool,
    #[serde(default = "default_true")]
    pub stop_schema_verified: bool,
    #[serde(default = "default_true")]
    pub approval_schema_verified: bool,
    #[serde(default = "default_true")]
    pub model_config_schema_verified: bool,
    #[serde(default = "default_true")]
    pub csrf_auth_verified: bool,
    #[serde(default)]
    pub arbitrary_rpc_exposed: bool,
}

impl AntigravityCapabilities {
    pub fn installed_2_29_0() -> Self {
        Self {
            bundle_id: "com.google.antigravity".into(),
            installed_version: "2.29.0".into(),
            language_server_cl: "1013060670".into(),
            agentapi_commands: vec![
                "get-conversation-metadata".into(),
                "new-conversation".into(),
                "send-message".into(),
            ],
            sidecar_sdk_version: "1.0.0".into(),
            hook_events: vec![
                "SessionStart".into(),
                "PreInvocation".into(),
                "PostInvocation".into(),
                "PreToolUse".into(),
                "PostToolUse".into(),
                "Stop".into(),
            ],
            supports_session_catalog: true,
            supports_target_focus: true,
            supports_stop_turn: true,
            supports_approval_gate: true,
            supports_model_config: true,
            schema_verified: true,
            stop_schema_verified: true,
            approval_schema_verified: true,
            model_config_schema_verified: true,
            csrf_auth_verified: true,
            arbitrary_rpc_exposed: false,
        }
    }

    pub fn is_valid(&self) -> bool {
        let expected_cmds = [
            "get-conversation-metadata",
            "new-conversation",
            "send-message",
        ];
        let expected_hooks = [
            "SessionStart",
            "PreInvocation",
            "PostInvocation",
            "PreToolUse",
            "PostToolUse",
            "Stop",
        ];
        self.bundle_id == "com.google.antigravity"
            && valid_antigravity_id(&self.installed_version, 64)
            && valid_antigravity_id(&self.language_server_cl, 64)
            && self
                .agentapi_commands
                .iter()
                .map(String::as_str)
                .eq(expected_cmds)
            && valid_antigravity_id(&self.sidecar_sdk_version, 64)
            && self
                .hook_events
                .iter()
                .map(String::as_str)
                .eq(expected_hooks)
            && !self.arbitrary_rpc_exposed
            && ((!self.supports_session_catalog && !self.supports_target_focus)
                || self.schema_verified)
            && (!self.supports_stop_turn
                || (self.schema_verified && self.stop_schema_verified && self.csrf_auth_verified))
            && (!self.supports_approval_gate
                || (self.schema_verified
                    && self.approval_schema_verified
                    && self.csrf_auth_verified))
            && (!self.supports_model_config
                || (self.schema_verified
                    && self.model_config_schema_verified
                    && self.csrf_auth_verified))
    }

    pub fn supports_catalog_navigation(&self) -> bool {
        self.is_valid()
            && self.schema_verified
            && self.supports_session_catalog
            && self.supports_target_focus
    }

    pub fn supports_create_conversation(&self) -> bool {
        self.is_valid()
            && self.schema_verified
            && self
                .agentapi_commands
                .iter()
                .any(|c| c == "new-conversation")
    }

    pub fn supports_send_message(&self) -> bool {
        self.is_valid()
            && self.schema_verified
            && self.agentapi_commands.iter().any(|c| c == "send-message")
    }

    pub fn supports_stop_conversation(&self) -> bool {
        self.is_valid()
            && self.schema_verified
            && self.stop_schema_verified
            && self.csrf_auth_verified
            && self.supports_stop_turn
    }

    pub fn supports_resolve_interaction(&self) -> bool {
        self.is_valid()
            && self.schema_verified
            && self.approval_schema_verified
            && self.csrf_auth_verified
            && self.supports_approval_gate
    }

    pub fn supports_switch_model_config(&self) -> bool {
        self.is_valid()
            && self.schema_verified
            && self.model_config_schema_verified
            && self.csrf_auth_verified
            && self.supports_model_config
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravitySessionKey {
    pub conversation_id: String,
    pub workspace_path: String,
}

impl AntigravitySessionKey {
    pub fn new(conversation_id: impl Into<String>, workspace_path: impl Into<String>) -> Self {
        Self {
            conversation_id: conversation_id.into(),
            workspace_path: workspace_path.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AntigravityHookEventType {
    SessionStart,
    PreInvocation,
    PostInvocation,
    PreToolUse,
    PostToolUse,
    Stop,
}

impl AntigravityHookEventType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SessionStart => "SessionStart",
            Self::PreInvocation => "PreInvocation",
            Self::PostInvocation => "PostInvocation",
            Self::PreToolUse => "PreToolUse",
            Self::PostToolUse => "PostToolUse",
            Self::Stop => "Stop",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AntigravitySessionPhase {
    Idle,
    Running,
    RunningTool,
    BackgroundActive,
    Completed,
    Failed,
    Canceled,
}

impl AntigravitySessionPhase {
    pub fn is_completed(self) -> bool {
        self == Self::Completed
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "已就绪",
            Self::Running => "运行中",
            Self::RunningTool => "执行工具中",
            Self::BackgroundActive => "后台仍在运行",
            Self::Completed => "已完成",
            Self::Failed => "执行异常",
            Self::Canceled => "已取消",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravityHookEvent {
    pub conversation_id: String,
    pub workspace_path: String,
    pub event_type: AntigravityHookEventType,
    pub model_name: String,
    pub invocation_num: Option<u64>,
    pub step_idx: Option<u64>,
    pub execution_num: Option<u64>,
    pub tool_name: Option<String>,
    pub termination_reason: Option<String>,
    pub has_error: bool,
    pub fully_idle: Option<bool>,
    pub event_id: Option<String>,
    pub title: Option<String>,
    pub project_id: Option<String>,
}

impl AntigravityHookEvent {
    pub fn session_key(&self) -> AntigravitySessionKey {
        AntigravitySessionKey::new(&self.conversation_id, &self.workspace_path)
    }

    pub fn event_key(&self, prior: Option<&AntigravitySessionRow>) -> String {
        if let Some(eid) = &self.event_id {
            if !eid.is_empty() {
                return format!("{}:{}:{}", self.conversation_id, self.workspace_path, eid);
            }
        }
        let inv = self
            .invocation_num
            .or_else(|| prior.map(|p| p.invocation_num))
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-1".into());
        let step = self
            .step_idx
            .or_else(|| prior.map(|p| p.step_idx))
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-1".into());
        let exec = self
            .execution_num
            .or_else(|| prior.map(|p| p.execution_num))
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-1".into());
        let tool = self.tool_name.as_deref().unwrap_or("");
        let term = self.termination_reason.as_deref().unwrap_or("");
        let err = if self.has_error { "1" } else { "0" };
        let idle = match self.fully_idle {
            Some(true) => "1",
            Some(false) => "0",
            None => "-",
        };
        format!(
            "{}:{}:{}:{inv}:{step}:{exec}:{tool}:{term}:{err}:{idle}",
            self.conversation_id,
            self.workspace_path,
            self.event_type.as_str()
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AntigravityInteractionType {
    Permission,
    RunCommand,
    FilePermission,
    Question,
    Approval,
}

impl AntigravityInteractionType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Permission => "工具权限审批",
            Self::RunCommand => "终端命令确认",
            Self::FilePermission => "文件访问审批",
            Self::Question => "问题选择确认",
            Self::Approval => "阶段审批确认",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AntigravityInteractionDecision {
    Allow,
    Deny,
    Answer,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravityPendingInteraction {
    pub interaction_id: String,
    pub interaction_type: AntigravityInteractionType,
    #[serde(default)]
    pub step_index: u64,
    #[serde(default)]
    pub option_ids: Vec<String>,
}

impl AntigravityPendingInteraction {
    pub fn is_valid(&self) -> bool {
        valid_antigravity_id(&self.interaction_id, 256)
            && self.step_index <= 1_000_000_000
            && self.option_ids.len() <= 64
            && self
                .option_ids
                .iter()
                .all(|id| valid_antigravity_id(id, 128))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravityModelOption {
    pub model_name: String,
    pub supported_thinking_levels: Vec<String>,
}

impl AntigravityModelOption {
    pub fn default_catalog() -> Vec<Self> {
        vec![
            Self {
                model_name: "gemini-2.5-pro".into(),
                supported_thinking_levels: vec!["low".into(), "medium".into(), "high".into()],
            },
            Self {
                model_name: "gemini-2.5-flash".into(),
                supported_thinking_levels: vec![
                    "minimal".into(),
                    "low".into(),
                    "medium".into(),
                    "high".into(),
                ],
            },
            Self {
                model_name: "gemini-2.5-flash-lite".into(),
                supported_thinking_levels: vec!["minimal".into(), "low".into()],
            },
            Self {
                model_name: "claude-sonnet-4-5".into(),
                supported_thinking_levels: vec!["low".into(), "medium".into(), "high".into()],
            },
            Self {
                model_name: "flash".into(),
                supported_thinking_levels: vec![
                    "minimal".into(),
                    "low".into(),
                    "medium".into(),
                    "high".into(),
                ],
            },
        ]
    }

    pub fn is_valid(&self) -> bool {
        let allowed = ["minimal", "low", "medium", "high", "modelDefault"];
        valid_antigravity_id(&self.model_name, 256)
            && !self.supported_thinking_levels.is_empty()
            && self.supported_thinking_levels.len() <= 16
            && self
                .supported_thinking_levels
                .iter()
                .all(|lvl| allowed.contains(&lvl.as_str()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravitySessionRow {
    pub conversation_id: String,
    pub workspace_path: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub project_id: String,
    pub model_name: String,
    #[serde(default = "default_thinking_level")]
    pub thinking_level: String,
    pub phase: AntigravitySessionPhase,
    pub last_event_type: AntigravityHookEventType,
    pub invocation_num: u64,
    pub step_idx: u64,
    pub execution_num: u64,
    #[serde(default)]
    pub last_tool_name: Option<String>,
    #[serde(default)]
    pub termination_reason: Option<String>,
    pub fully_idle: bool,
    pub transition_count: u64,
    #[serde(default)]
    pub pending_interaction: Option<AntigravityPendingInteraction>,
    #[serde(default = "default_model_catalog")]
    pub available_models: Vec<AntigravityModelOption>,
}

impl AntigravitySessionRow {
    pub fn normalized(mut self) -> Self {
        if self.title.is_empty() {
            self.title = self.conversation_id.clone();
        }
        if self.thinking_level.is_empty() {
            self.thinking_level = "high".into();
        }
        if self.available_models.is_empty() {
            self.available_models = AntigravityModelOption::default_catalog();
        }
        self
    }

    pub fn key(&self) -> AntigravitySessionKey {
        AntigravitySessionKey::new(&self.conversation_id, &self.workspace_path)
    }

    pub fn id(&self) -> String {
        format!("{}@{}", self.conversation_id, self.workspace_path)
    }

    pub fn is_valid(&self) -> bool {
        let allowed_thinking = ["minimal", "low", "medium", "high", "modelDefault"];
        valid_antigravity_id(&self.conversation_id, 256)
            && valid_antigravity_id(&self.workspace_path, 4096)
            && valid_antigravity_id(&self.title, 512)
            && self.project_id.len() <= 256
            && self.model_name.len() <= 256
            && allowed_thinking.contains(&self.thinking_level.as_str())
            && self.invocation_num <= 1_000_000_000
            && self.step_idx <= 1_000_000_000
            && self.execution_num <= 1_000_000_000
            && self
                .last_tool_name
                .as_deref()
                .is_none_or(|t| valid_antigravity_id(t, 256))
            && self
                .termination_reason
                .as_deref()
                .is_none_or(|r| valid_antigravity_id(r, 128))
            && self.transition_count > 0
            && self.transition_count <= 1_000_000_000
            && self
                .pending_interaction
                .as_ref()
                .is_none_or(AntigravityPendingInteraction::is_valid)
            && !self.available_models.is_empty()
            && self
                .available_models
                .iter()
                .all(AntigravityModelOption::is_valid)
            && (self.phase != AntigravitySessionPhase::Completed
                || (self.fully_idle && self.termination_reason.as_deref() == Some("NO_TOOL_CALL")))
            && (self.phase != AntigravitySessionPhase::BackgroundActive || !self.fully_idle)
    }

    pub fn supports_thinking_level(&self, level: &str, target_model: &str) -> bool {
        self.available_models
            .iter()
            .find(|m| m.model_name == target_model)
            .is_some_and(|opt| opt.supported_thinking_levels.iter().any(|l| l == level))
    }

    pub fn status_label(&self) -> String {
        if let Some(pending) = &self.pending_interaction {
            return format!(
                "{} · 待处理{}",
                self.phase.label(),
                pending.interaction_type.label()
            );
        }
        if self.phase == AntigravitySessionPhase::RunningTool {
            if let Some(tool) = &self.last_tool_name {
                if !tool.is_empty() {
                    return format!("{}（{tool}）", self.phase.label());
                }
            }
        }
        self.phase.label().into()
    }

    pub fn reduce(prior: Option<&Self>, event: &AntigravityHookEvent) -> Self {
        let title = event
            .title
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(String::from)
            .or_else(|| prior.map(|p| p.title.clone()))
            .unwrap_or_else(|| event.conversation_id.clone());
        let project_id = event
            .project_id
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(String::from)
            .or_else(|| prior.map(|p| p.project_id.clone()))
            .unwrap_or_default();
        let model_name = if event.model_name.is_empty() {
            prior.map(|p| p.model_name.clone()).unwrap_or_default()
        } else {
            event.model_name.clone()
        };
        let thinking_level = prior
            .map(|p| p.thinking_level.clone())
            .unwrap_or_else(|| "high".into());
        let available_models = prior
            .map(|p| p.available_models.clone())
            .unwrap_or_else(AntigravityModelOption::default_catalog);
        let invocation_num = event
            .invocation_num
            .or_else(|| prior.map(|p| p.invocation_num))
            .unwrap_or(0);
        let step_idx = event
            .step_idx
            .or_else(|| prior.map(|p| p.step_idx))
            .unwrap_or(0);
        let execution_num = event
            .execution_num
            .or_else(|| prior.map(|p| p.execution_num))
            .unwrap_or(0);

        let mut pending_interaction = prior.and_then(|p| p.pending_interaction.clone());
        let prior_tool = prior.and_then(|p| p.last_tool_name.clone());
        let (phase, last_tool_name, termination_reason, fully_idle) = match event.event_type {
            AntigravityHookEventType::SessionStart => {
                pending_interaction = None;
                (AntigravitySessionPhase::Idle, None, None, true)
            }
            AntigravityHookEventType::PreInvocation => {
                (AntigravitySessionPhase::Running, prior_tool, None, false)
            }
            AntigravityHookEventType::PreToolUse => {
                let tool = event
                    .tool_name
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .or(prior_tool);
                (AntigravitySessionPhase::RunningTool, tool, None, false)
            }
            AntigravityHookEventType::PostToolUse => {
                pending_interaction = None;
                let tool = event
                    .tool_name
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .or(prior_tool);
                (AntigravitySessionPhase::Running, tool, None, false)
            }
            AntigravityHookEventType::PostInvocation => {
                if event.has_error {
                    (
                        AntigravitySessionPhase::Failed,
                        prior_tool,
                        Some("ERROR".into()),
                        false,
                    )
                } else {
                    (AntigravitySessionPhase::Running, prior_tool, None, false)
                }
            }
            AntigravityHookEventType::Stop => {
                pending_interaction = None;
                let reason = event
                    .termination_reason
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .map(String::from);
                if reason.as_deref() == Some("ERROR") || event.has_error {
                    (
                        AntigravitySessionPhase::Failed,
                        prior_tool,
                        reason,
                        event.fully_idle.unwrap_or(false),
                    )
                } else if reason.as_deref() == Some("USER_CANCELED") {
                    (
                        AntigravitySessionPhase::Canceled,
                        prior_tool,
                        reason,
                        event.fully_idle.unwrap_or(false),
                    )
                } else if event.fully_idle == Some(true)
                    && reason.as_deref() == Some("NO_TOOL_CALL")
                {
                    (AntigravitySessionPhase::Completed, prior_tool, reason, true)
                } else {
                    (
                        AntigravitySessionPhase::BackgroundActive,
                        prior_tool,
                        reason,
                        false,
                    )
                }
            }
        };

        Self {
            conversation_id: event.conversation_id.clone(),
            workspace_path: event.workspace_path.clone(),
            title,
            project_id,
            model_name,
            thinking_level,
            phase,
            last_event_type: event.event_type,
            invocation_num,
            step_idx,
            execution_num,
            last_tool_name,
            termination_reason,
            fully_idle,
            transition_count: prior.map_or(1, |p| p.transition_count + 1),
            pending_interaction,
            available_models,
        }
        .normalized()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravityEventChange {
    pub sequence: u64,
    pub event_key: String,
    pub row: AntigravitySessionRow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AntigravityStatusSnapshot {
    pub instance_id: String,
    pub generation: u64,
    pub allowed_workspaces: Vec<String>,
    pub capabilities: AntigravityCapabilities,
    pub cursor: u64,
    pub rows: Vec<AntigravitySessionRow>,
    pub changes: Vec<AntigravityEventChange>,
    pub selected: Option<AntigravitySessionKey>,
    pub transcript_reads: u64,
    pub token_reads: u64,
}

impl AntigravityStatusSnapshot {
    pub fn validate(&self, after: Option<u64>) -> Result<(), AntigravityFailure> {
        let workspace_scope: HashSet<&str> =
            self.allowed_workspaces.iter().map(String::as_str).collect();
        let row_keys: HashSet<AntigravitySessionKey> =
            self.rows.iter().map(AntigravitySessionRow::key).collect();
        if !valid_antigravity_id(&self.instance_id, 256)
            || self.generation == 0
            || workspace_scope.is_empty()
            || workspace_scope.len() > 4096
            || workspace_scope.len() != self.allowed_workspaces.len()
            || !self
                .allowed_workspaces
                .iter()
                .all(|w| valid_antigravity_id(w, 4096))
            || !self.capabilities.is_valid()
            || self.transcript_reads != 0
            || self.token_reads != 0
            || self.rows.len() != row_keys.len()
            || !self
                .rows
                .iter()
                .all(|r| r.is_valid() && workspace_scope.contains(r.workspace_path.as_str()))
            || self
                .selected
                .as_ref()
                .is_some_and(|sel| !row_keys.contains(sel))
            || self.changes.len() > 1024
            || !self.changes.iter().all(|c| {
                valid_antigravity_id(&c.event_key, 1024)
                    && c.row.is_valid()
                    && workspace_scope.contains(c.row.workspace_path.as_str())
            })
        {
            return Err(AntigravityFailure::InvalidReply);
        }
        if let Some(prev) = after {
            if self.cursor < prev || self.cursor - prev != self.changes.len() as u64 {
                return Err(AntigravityFailure::Gap);
            }
            for (offset, change) in self.changes.iter().enumerate() {
                if change.sequence != prev + offset as u64 + 1 {
                    return Err(AntigravityFailure::Gap);
                }
            }
        } else if !self.changes.is_empty() {
            return Err(AntigravityFailure::InvalidReply);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AntigravitySelection {
    pub key: AntigravitySessionKey,
    pub instance_id: String,
    pub generation: u64,
    pub local_epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AntigravityOpenReply {
    pub instance_id: String,
    pub generation: u64,
    pub status: String,
    pub selected: Option<AntigravitySessionKey>,
    pub transcript_reads: u64,
    pub token_reads: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AntigravityOpenResult {
    Confirmed,
    Unconfirmed,
    Stale,
    Denied,
    Unavailable,
}

impl AntigravityOpenResult {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Unconfirmed => "unconfirmed",
            Self::Stale => "stale",
            Self::Denied => "denied",
            Self::Unavailable => "unavailable",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Confirmed => "已聚焦并读回确认 Antigravity 会话",
            Self::Unconfirmed => "已发送聚焦请求，但 Antigravity 选中读回未确认",
            Self::Stale => "目标 Antigravity 会话已过期，未发送跳转",
            Self::Denied => "Antigravity 拒绝会话聚焦或缺少合法 Schema/鉴权",
            Self::Unavailable => "Antigravity 状态桥不可达，无法跳转",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AntigravityDeliveryMode {
    Auto,
    Steer,
    Queue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AntigravityDeliveryOutcome {
    Started,
    NextInvocation,
    Steered,
    Canceled,
    Allow,
    Deny,
    Answer,
    Cancel,
    ModelSwitched,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravityCreateIntent {
    pub request_id: String,
    pub workspace_path: String,
    pub project_id: String,
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
}

impl AntigravityCreateIntent {
    pub fn is_valid(&self) -> bool {
        valid_antigravity_id(&self.request_id, 256)
            && valid_antigravity_id(&self.workspace_path, 4096)
            && valid_antigravity_id(&self.project_id, 256)
            && valid_antigravity_id(&self.prompt, 32768)
            && self
                .title
                .as_deref()
                .is_none_or(|t| valid_antigravity_id(t, 512))
            && self
                .model_name
                .as_deref()
                .is_none_or(|m| valid_antigravity_id(m, 256))
            && self
                .conversation_id
                .as_deref()
                .is_none_or(|c| valid_antigravity_id(c, 256))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravitySendIntent {
    pub request_id: String,
    pub target: AntigravitySessionKey,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub message: String,
    pub delivery_mode: AntigravityDeliveryMode,
}

impl AntigravitySendIntent {
    pub fn is_valid(&self) -> bool {
        valid_antigravity_id(&self.request_id, 256)
            && valid_antigravity_id(&self.target.conversation_id, 256)
            && valid_antigravity_id(&self.target.workspace_path, 4096)
            && self
                .project_id
                .as_deref()
                .is_none_or(|p| valid_antigravity_id(p, 256))
            && valid_antigravity_id(&self.message, 32768)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravityStopIntent {
    pub request_id: String,
    pub target: AntigravitySessionKey,
    pub expected_invocation_num: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_execution_num: Option<u64>,
    pub kill_background_tasks: bool,
}

impl AntigravityStopIntent {
    pub fn is_valid(&self) -> bool {
        valid_antigravity_id(&self.request_id, 256)
            && valid_antigravity_id(&self.target.conversation_id, 256)
            && valid_antigravity_id(&self.target.workspace_path, 4096)
            && self.expected_invocation_num <= 1_000_000_000
            && self
                .expected_execution_num
                .is_none_or(|n| n <= 1_000_000_000)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravityResolveIntent {
    pub request_id: String,
    pub target: AntigravitySessionKey,
    pub interaction_id: String,
    pub interaction_type: AntigravityInteractionType,
    pub decision: AntigravityInteractionDecision,
    pub selected_option_ids: Vec<String>,
}

impl AntigravityResolveIntent {
    pub fn is_valid(&self) -> bool {
        valid_antigravity_id(&self.request_id, 256)
            && valid_antigravity_id(&self.target.conversation_id, 256)
            && valid_antigravity_id(&self.target.workspace_path, 4096)
            && valid_antigravity_id(&self.interaction_id, 256)
            && self.selected_option_ids.len() <= 64
            && self
                .selected_option_ids
                .iter()
                .all(|id| valid_antigravity_id(id, 128))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravitySwitchModelIntent {
    pub request_id: String,
    pub target: AntigravitySessionKey,
    pub model_name: String,
    pub thinking_level: String,
}

impl AntigravitySwitchModelIntent {
    pub fn is_valid(&self) -> bool {
        let allowed = ["minimal", "low", "medium", "high", "modelDefault"];
        valid_antigravity_id(&self.request_id, 256)
            && valid_antigravity_id(&self.target.conversation_id, 256)
            && valid_antigravity_id(&self.target.workspace_path, 4096)
            && valid_antigravity_id(&self.model_name, 256)
            && allowed.contains(&self.thinking_level.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravityQueryIntent {
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_op: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}

impl AntigravityQueryIntent {
    pub fn is_valid(&self) -> bool {
        valid_antigravity_id(&self.request_id, 256)
            && self
                .expected_op
                .as_deref()
                .is_none_or(|op| valid_antigravity_id(op, 32))
            && self
                .conversation_id
                .as_deref()
                .is_none_or(|c| valid_antigravity_id(c, 256))
            && self
                .workspace_path
                .as_deref()
                .is_none_or(|w| valid_antigravity_id(w, 4096))
            && self.project_id.as_deref().is_none_or(|p| p.len() <= 256)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AntigravityCommandOperation {
    Create(AntigravityCreateIntent),
    Send(AntigravitySendIntent),
    Stop(AntigravityStopIntent),
    Resolve(AntigravityResolveIntent),
    SwitchModel(AntigravitySwitchModelIntent),
    Query(AntigravityQueryIntent),
}

impl AntigravityCommandOperation {
    pub fn request_id(&self) -> &str {
        match self {
            Self::Create(i) => &i.request_id,
            Self::Send(i) => &i.request_id,
            Self::Stop(i) => &i.request_id,
            Self::Resolve(i) => &i.request_id,
            Self::SwitchModel(i) => &i.request_id,
            Self::Query(i) => &i.request_id,
        }
    }

    pub fn op_name(&self) -> &'static str {
        match self {
            Self::Create(_) => "create",
            Self::Send(_) => "send",
            Self::Stop(_) => "stop",
            Self::Resolve(_) => "resolve",
            Self::SwitchModel(_) => "switch-model",
            Self::Query(_) => "query",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AntigravityQueuedCommand {
    pub operation: AntigravityCommandOperation,
    pub instance_id: String,
    pub generation: u64,
    pub local_epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AntigravityCommandReply {
    pub instance_id: String,
    pub generation: u64,
    pub status: String,
    pub command_op: String,
    pub request_id: String,
    pub conversation_id: String,
    pub workspace_path: String,
    pub project_id: String,
    pub delivery: AntigravityDeliveryOutcome,
    pub in_flight_steering: bool,
    pub reason_code: String,
    pub duplicate: bool,
    pub reconciled: bool,
    pub cursor: u64,
    pub rows: Vec<AntigravitySessionRow>,
    pub selected: Option<AntigravitySessionKey>,
    pub transcript_reads: u64,
    pub token_reads: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AntigravityCommandResult {
    Created,
    Started,
    Queued,
    Steered,
    Stopped,
    Resolved,
    ModelSwitched,
    Noop,
    Unconfirmed,
    Unknown,
    Stale,
    Denied,
    Unavailable,
}

impl AntigravityCommandResult {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Started => "started",
            Self::Queued => "queued",
            Self::Steered => "steered",
            Self::Stopped => "stopped",
            Self::Resolved => "resolved",
            Self::ModelSwitched => "modelSwitched",
            Self::Noop => "noop",
            Self::Unconfirmed => "unconfirmed",
            Self::Unknown => "unknown",
            Self::Stale => "stale",
            Self::Denied => "denied",
            Self::Unavailable => "unavailable",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Created => "已通过 agentapi 创建并确认 Antigravity 会话",
            Self::Started => "已向 Antigravity 会话发送消息并开始执行",
            Self::Queued => "Antigravity 会话运行中，消息已排入下一轮调用（nextInvocation）",
            Self::Steered => "已向运行中的 Antigravity 会话注入 steering 消息",
            Self::Stopped => "已通过 CancelCascadeInvocation 停止 Antigravity 当前轮次",
            Self::Resolved => "已通过 HandleCascadeUserInteraction 确认 Antigravity 待处理请求",
            Self::ModelSwitched => "已切换 Antigravity 模型与思考档位并完成读回确认",
            Self::Noop => "目标状态已变更或已结束，安全跳过无副作用",
            Self::Unconfirmed => "Antigravity 命令未确认或被安全守卫拦截",
            Self::Unknown => "Antigravity 命令响应超时，已锁定 requestId 待查询对账",
            Self::Stale => "Antigravity 会话快照已过期，未发送命令",
            Self::Denied => "Antigravity 拒绝此命令或缺少合法 Schema/鉴权",
            Self::Unavailable => "Antigravity 状态桥不可达，无法执行命令",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AntigravityFailure {
    #[error("请先配置 Antigravity 状态桥授权")]
    Grant,
    #[error("Antigravity 状态桥拒绝此请求")]
    Denied,
    #[error("Antigravity 事件流已断开或实例已换代")]
    Stale,
    #[error("Antigravity 状态桥连接中断")]
    Unavailable,
    #[error("Antigravity 状态回执无法核对")]
    InvalidReply,
    #[error("Antigravity 事件日志存在缺口，请重连")]
    Gap,
    #[error("Antigravity 命令传输超时")]
    TransportTimeout,
}

impl AntigravityFailure {
    pub fn label(self) -> &'static str {
        match self {
            Self::Grant => "请先配置 Antigravity 状态桥授权",
            Self::Denied => "Antigravity 状态桥拒绝此请求",
            Self::Stale => "Antigravity 事件流已断开或实例已换代",
            Self::Unavailable => "Antigravity 状态桥连接中断",
            Self::InvalidReply => "Antigravity 状态回执无法核对",
            Self::Gap => "Antigravity 事件日志存在缺口，请重连",
            Self::TransportTimeout => "Antigravity 命令传输超时",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AntigravityConnectionState {
    Disabled,
    Connecting,
    Ready,
    Unavailable(AntigravityFailure),
}

impl AntigravityConnectionState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Disabled => "Antigravity 状态连接未开启",
            Self::Connecting => "正在连接 Antigravity 状态桥",
            Self::Ready => "Antigravity 状态连接已就绪",
            Self::Unavailable(r) => r.label(),
        }
    }
}

pub trait AntigravityStatusServing: Send + Sync {
    fn connect(&self) -> Result<AntigravityStatusSnapshot, AntigravityFailure>;
    fn read(&self) -> Result<AntigravityStatusSnapshot, AntigravityFailure>;
    fn open(
        &self,
        _target: &AntigravitySessionKey,
    ) -> Result<AntigravityOpenReply, AntigravityFailure> {
        Err(AntigravityFailure::Denied)
    }
    fn create(
        &self,
        _intent: &AntigravityCreateIntent,
    ) -> Result<AntigravityCommandReply, AntigravityFailure> {
        Err(AntigravityFailure::Denied)
    }
    fn send(
        &self,
        _intent: &AntigravitySendIntent,
    ) -> Result<AntigravityCommandReply, AntigravityFailure> {
        Err(AntigravityFailure::Denied)
    }
    fn stop(
        &self,
        _intent: &AntigravityStopIntent,
    ) -> Result<AntigravityCommandReply, AntigravityFailure> {
        Err(AntigravityFailure::Denied)
    }
    fn resolve(
        &self,
        _intent: &AntigravityResolveIntent,
    ) -> Result<AntigravityCommandReply, AntigravityFailure> {
        Err(AntigravityFailure::Denied)
    }
    fn switch_model(
        &self,
        _intent: &AntigravitySwitchModelIntent,
    ) -> Result<AntigravityCommandReply, AntigravityFailure> {
        Err(AntigravityFailure::Denied)
    }
    fn query(
        &self,
        _intent: &AntigravityQueryIntent,
    ) -> Result<AntigravityCommandReply, AntigravityFailure> {
        Err(AntigravityFailure::Denied)
    }
    fn close(&self);
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravityGrant {
    pub version: u32,
    pub socket_path: String,
    pub token: String,
    pub instance_id: String,
    pub generation: u64,
    pub allowed_workspaces: Vec<String>,
    pub capabilities: AntigravityCapabilities,
}

impl AntigravityGrant {
    pub fn load(path: &Path) -> Result<Self, AntigravityFailure> {
        let uid = unsafe { libc::getuid() };
        let parent = path.parent().ok_or(AntigravityFailure::Grant)?;
        let parent_meta =
            std::fs::symlink_metadata(parent).map_err(|_| AntigravityFailure::Grant)?;
        if !parent_meta.file_type().is_dir()
            || parent_meta.uid() != uid
            || (parent_meta.mode() & 0o077) != 0
        {
            return Err(AntigravityFailure::Grant);
        }
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .map_err(|_| AntigravityFailure::Grant)?;
        let meta = file.metadata().map_err(|_| AntigravityFailure::Grant)?;
        if !meta.file_type().is_file()
            || meta.uid() != uid
            || (meta.mode() & 0o077) != 0
            || meta.len() == 0
            || meta.len() > 65536
        {
            return Err(AntigravityFailure::Grant);
        }
        let mut bytes = Vec::with_capacity(meta.len() as usize);
        file.read_to_end(&mut bytes)
            .map_err(|_| AntigravityFailure::Grant)?;
        let grant: Self = serde_json::from_slice(&bytes).map_err(|_| AntigravityFailure::Grant)?;
        let unique_ws: HashSet<&str> = grant
            .allowed_workspaces
            .iter()
            .map(String::as_str)
            .collect();
        if grant.version != 1
            || !grant.socket_path.starts_with('/')
            || grant.socket_path.len() >= 104
            || grant.token.len() < 32
            || grant.token.len() > 256
            || !valid_antigravity_id(&grant.instance_id, 256)
            || grant.generation == 0
            || grant.allowed_workspaces.is_empty()
            || grant.allowed_workspaces.len() > 4096
            || !grant
                .allowed_workspaces
                .iter()
                .all(|w| valid_antigravity_id(w, 4096))
            || unique_ws.len() != grant.allowed_workspaces.len()
            || !grant.capabilities.is_valid()
        {
            return Err(AntigravityFailure::Grant);
        }
        Ok(grant)
    }
}

#[derive(Debug, Default)]
struct AntigravityClientState {
    grant: Option<AntigravityGrant>,
    epoch: u64,
    cursor: Option<u64>,
    subscription_id: Option<String>,
    next_id: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AntigravityWireReply {
    version: u32,
    subscription_id: String,
    id: String,
    instance_id: String,
    generation: u64,
    status: String,
    capabilities: Option<AntigravityCapabilities>,
    cursor: Option<u64>,
    rows: Option<Vec<AntigravitySessionRow>>,
    changes: Option<Vec<AntigravityEventChange>>,
    selected: Option<AntigravitySessionKey>,
    transcript_reads: Option<u64>,
    token_reads: Option<u64>,
    command_op: Option<String>,
    request_id: Option<String>,
    conversation_id: Option<String>,
    workspace_path: Option<String>,
    project_id: Option<String>,
    delivery: Option<AntigravityDeliveryOutcome>,
    in_flight_steering: Option<bool>,
    reason_code: Option<String>,
    duplicate: Option<bool>,
    reconciled: Option<bool>,
}

pub struct AntigravityStatusClient {
    pub grant_path: PathBuf,
    pub command_timeout: Duration,
    state: Mutex<AntigravityClientState>,
}

impl AntigravityStatusClient {
    const CATALOG_NAV_SCHEMA: &'static str = "antigravity-catalog-nav-v1";
    const AGENTAPI_SCHEMA: &'static str = "antigravity-agentapi-v1";
    const STOP_SCHEMA: &'static str = "antigravity-stop-v1";
    const INTERACTION_SCHEMA: &'static str = "antigravity-interaction-v1";
    const MODEL_CONFIG_SCHEMA: &'static str = "antigravity-model-config-v1";

    pub fn new(grant_path: impl Into<PathBuf>) -> Self {
        Self {
            grant_path: grant_path.into(),
            command_timeout: Duration::from_secs(2),
            state: Mutex::new(AntigravityClientState::default()),
        }
    }

    fn current_grant(&self) -> Result<(AntigravityGrant, u64, String), AntigravityFailure> {
        let guard = self.state.lock().unwrap();
        let grant = guard.grant.clone().ok_or(AntigravityFailure::Denied)?;
        let sub_id = guard
            .subscription_id
            .clone()
            .ok_or(AntigravityFailure::Stale)?;
        let epoch = guard.epoch;
        drop(guard);
        let disk = AntigravityGrant::load(&self.grant_path)?;
        if disk != grant {
            return Err(AntigravityFailure::Stale);
        }
        Ok((grant, epoch, sub_id))
    }

    fn valid_row_object(row: &serde_json::Map<String, Value>) -> bool {
        let required = [
            "conversationId",
            "workspacePath",
            "modelName",
            "phase",
            "lastEventType",
            "invocationNum",
            "stepIdx",
            "executionNum",
            "fullyIdle",
            "transitionCount",
        ];
        let allowed = [
            "conversationId",
            "workspacePath",
            "modelName",
            "phase",
            "lastEventType",
            "invocationNum",
            "stepIdx",
            "executionNum",
            "fullyIdle",
            "transitionCount",
            "title",
            "projectId",
            "thinkingLevel",
            "lastToolName",
            "terminationReason",
            "pendingInteraction",
            "availableModels",
        ];
        if !required.iter().all(|k| row.contains_key(*k))
            || !row.keys().all(|k| allowed.contains(&k.as_str()))
        {
            return false;
        }
        if let Some(pending) = row.get("pendingInteraction") {
            if !pending.is_null() {
                let allowed_pending =
                    ["interactionId", "interactionType", "stepIndex", "optionIds"];
                let Some(obj) = pending.as_object() else {
                    return false;
                };
                if !obj.keys().all(|k| allowed_pending.contains(&k.as_str())) {
                    return false;
                }
            }
        }
        if let Some(models) = row.get("availableModels") {
            if !models.is_null() {
                let Some(arr) = models.as_array() else {
                    return false;
                };
                for item in arr {
                    let Some(obj) = item.as_object() else {
                        return false;
                    };
                    if obj.len() != 2
                        || !obj.contains_key("modelName")
                        || !obj.contains_key("supportedThinkingLevels")
                    {
                        return false;
                    }
                }
            }
        }
        true
    }

    fn valid_selected_object(val: &Value) -> bool {
        if val.is_null() {
            return true;
        }
        let Some(obj) = val.as_object() else {
            return false;
        };
        obj.len() == 2 && obj.contains_key("conversationId") && obj.contains_key("workspacePath")
    }

    fn exchange(
        &self,
        op: &str,
        after: Option<u64>,
        schema_version: Option<&str>,
        target: Option<&AntigravitySessionKey>,
        intent: Option<Value>,
        session: &(AntigravityGrant, u64, String),
    ) -> Result<AntigravityWireReply, AntigravityFailure> {
        let (grant, expected_epoch, subscription_id) = (&session.0, session.1, session.2.as_str());
        let req_id = {
            let mut guard = self.state.lock().unwrap();
            if guard.epoch != expected_epoch
                || guard.grant.as_ref() != Some(grant)
                || guard.subscription_id.as_deref() != Some(subscription_id)
            {
                return Err(AntigravityFailure::Stale);
            }
            guard.next_id += 1;
            format!("ag-wire-{expected_epoch}-{}", guard.next_id)
        };

        let mut payload = serde_json::Map::new();
        payload.insert("version".into(), json!(1));
        payload.insert("subscriptionId".into(), json!(subscription_id));
        payload.insert("id".into(), json!(req_id));
        payload.insert("token".into(), json!(grant.token));
        payload.insert("instanceId".into(), json!(grant.instance_id));
        payload.insert("generation".into(), json!(grant.generation));
        payload.insert("op".into(), json!(op));
        if op == "subscribe" {
            payload.insert("activate".into(), json!(false));
        }
        if let Some(after_val) = after {
            payload.insert("after".into(), json!(after_val));
        }
        if let Some(sv) = schema_version {
            payload.insert("schemaVersion".into(), json!(sv));
        }
        if let Some(t) = target {
            payload.insert("target".into(), serde_json::to_value(t).unwrap());
        }
        if let Some(i) = intent {
            payload.insert("intent".into(), i);
        }

        let is_cmd = ["create", "send", "stop", "resolve", "switch-model", "query"].contains(&op);
        let timeout = if is_cmd {
            self.command_timeout
        } else {
            Duration::from_secs(2)
        };
        let started = Instant::now();
        let line_bytes = self
            .exchange_raw(&grant.socket_path, &Value::Object(payload), timeout)
            .map_err(|e| {
                if is_cmd && started.elapsed() >= timeout {
                    AntigravityFailure::TransportTimeout
                } else {
                    e
                }
            })?;

        {
            let guard = self.state.lock().unwrap();
            if guard.epoch != expected_epoch
                || guard.grant.as_ref() != Some(grant)
                || guard.subscription_id.as_deref() != Some(subscription_id)
            {
                return Err(AntigravityFailure::Stale);
            }
        }
        if AntigravityGrant::load(&self.grant_path)? != *grant {
            return Err(AntigravityFailure::Stale);
        }

        let raw_val: Value =
            serde_json::from_slice(&line_bytes).map_err(|_| AntigravityFailure::InvalidReply)?;
        let Some(top_obj) = raw_val.as_object() else {
            return Err(AntigravityFailure::InvalidReply);
        };
        let allowed_top = [
            "version",
            "subscriptionId",
            "id",
            "instanceId",
            "generation",
            "status",
            "capabilities",
            "cursor",
            "rows",
            "changes",
            "selected",
            "transcriptReads",
            "tokenReads",
            "commandOp",
            "requestId",
            "conversationId",
            "workspacePath",
            "projectId",
            "delivery",
            "inFlightSteering",
            "reasonCode",
            "duplicate",
            "reconciled",
        ];
        if !top_obj.keys().all(|k| allowed_top.contains(&k.as_str())) {
            return Err(AntigravityFailure::InvalidReply);
        }
        if let Some(sel) = top_obj.get("selected") {
            if !Self::valid_selected_object(sel) {
                return Err(AntigravityFailure::InvalidReply);
            }
        }
        if let Some(rows_val) = top_obj.get("rows") {
            if !rows_val.is_null() {
                let Some(arr) = rows_val.as_array() else {
                    return Err(AntigravityFailure::InvalidReply);
                };
                if !arr
                    .iter()
                    .all(|r| r.as_object().is_some_and(Self::valid_row_object))
                {
                    return Err(AntigravityFailure::InvalidReply);
                }
            }
        }
        if let Some(changes_val) = top_obj.get("changes") {
            if !changes_val.is_null() {
                let Some(arr) = changes_val.as_array() else {
                    return Err(AntigravityFailure::InvalidReply);
                };
                for change in arr {
                    let Some(c_obj) = change.as_object() else {
                        return Err(AntigravityFailure::InvalidReply);
                    };
                    if c_obj.len() != 3
                        || !c_obj.contains_key("sequence")
                        || !c_obj.contains_key("eventKey")
                        || !c_obj
                            .get("row")
                            .and_then(Value::as_object)
                            .is_some_and(Self::valid_row_object)
                    {
                        return Err(AntigravityFailure::InvalidReply);
                    }
                }
            }
        }

        let mut reply: AntigravityWireReply =
            serde_json::from_value(raw_val).map_err(|_| AntigravityFailure::InvalidReply)?;
        if let Some(rows) = &mut reply.rows {
            for r in rows.iter_mut() {
                *r = r.clone().normalized();
            }
        }
        if let Some(changes) = &mut reply.changes {
            for c in changes.iter_mut() {
                c.row = c.row.clone().normalized();
            }
        }

        if reply.version != 1 || reply.id != req_id || reply.subscription_id != subscription_id {
            return Err(AntigravityFailure::InvalidReply);
        }
        if reply.instance_id != grant.instance_id || reply.generation != grant.generation {
            return Err(AntigravityFailure::Stale);
        }
        if is_cmd {
            let allowed_cmd_statuses = [
                "created",
                "started",
                "queued",
                "steered",
                "stopped",
                "resolved",
                "modelSwitched",
                "noop",
                "unconfirmed",
                "unknown",
                "denied",
                "stale",
                "unavailable",
            ];
            if !allowed_cmd_statuses.contains(&reply.status.as_str()) {
                return Err(AntigravityFailure::InvalidReply);
            }
            return Ok(reply);
        }
        let allowed_statuses: &[&str] = if op == "open" {
            &["ok", "unconfirmed", "denied", "stale", "unavailable"]
        } else {
            &["ok", "denied", "stale", "unavailable", "gap"]
        };
        if !allowed_statuses.contains(&reply.status.as_str())
            || (reply.status != "ok"
                && (reply.rows.is_some() || reply.changes.is_some() || reply.cursor.is_some()))
        {
            return Err(AntigravityFailure::InvalidReply);
        }
        Ok(reply)
    }

    fn exchange_raw(
        &self,
        socket_path: &str,
        request: &Value,
        timeout: Duration,
    ) -> Result<Vec<u8>, AntigravityFailure> {
        let mut encoded =
            serde_json::to_vec(request).map_err(|_| AntigravityFailure::InvalidReply)?;
        if encoded.len() > 65536 {
            return Err(AntigravityFailure::InvalidReply);
        }
        encoded.push(b'\n');
        let mut stream =
            UnixStream::connect(socket_path).map_err(|_| AntigravityFailure::Unavailable)?;
        let _ = stream.set_read_timeout(Some(timeout));
        let _ = stream.set_write_timeout(Some(timeout));
        stream
            .write_all(&encoded)
            .map_err(|_| AntigravityFailure::Unavailable)?;
        let mut reader = BufReader::new(stream.take(1_048_576));
        let mut line = Vec::new();
        let n = reader
            .read_until(b'\n', &mut line)
            .map_err(|_| AntigravityFailure::Unavailable)?;
        if n == 0 {
            return Err(AntigravityFailure::Unavailable);
        }
        Ok(line)
    }

    fn map_failure(status: &str) -> AntigravityFailure {
        match status {
            "denied" => AntigravityFailure::Denied,
            "stale" => AntigravityFailure::Stale,
            "unavailable" => AntigravityFailure::Unavailable,
            "gap" => AntigravityFailure::Gap,
            _ => AntigravityFailure::InvalidReply,
        }
    }

    fn parse_snapshot(
        &self,
        reply: AntigravityWireReply,
        grant: &AntigravityGrant,
        after: Option<u64>,
    ) -> Result<AntigravityStatusSnapshot, AntigravityFailure> {
        if reply.status != "ok" {
            return Err(Self::map_failure(&reply.status));
        }
        let capabilities = reply.capabilities.ok_or(AntigravityFailure::InvalidReply)?;
        let cursor = reply.cursor.ok_or(AntigravityFailure::InvalidReply)?;
        let rows = reply.rows.ok_or(AntigravityFailure::InvalidReply)?;
        let changes = reply.changes.ok_or(AntigravityFailure::InvalidReply)?;
        let transcript_reads = reply
            .transcript_reads
            .ok_or(AntigravityFailure::InvalidReply)?;
        if capabilities != grant.capabilities {
            return Err(AntigravityFailure::InvalidReply);
        }
        let snapshot = AntigravityStatusSnapshot {
            instance_id: grant.instance_id.clone(),
            generation: grant.generation,
            allowed_workspaces: grant.allowed_workspaces.clone(),
            capabilities,
            cursor,
            rows,
            changes,
            selected: reply.selected,
            transcript_reads,
            token_reads: reply.token_reads.unwrap_or(0),
        };
        snapshot.validate(after)?;
        Ok(snapshot)
    }

    fn parse_command_reply(
        &self,
        reply: AntigravityWireReply,
        grant: &AntigravityGrant,
        expected_request_id: &str,
    ) -> Result<AntigravityCommandReply, AntigravityFailure> {
        let allowed_ok = [
            "created",
            "started",
            "queued",
            "steered",
            "stopped",
            "resolved",
            "modelSwitched",
            "noop",
            "unconfirmed",
            "unknown",
        ];
        if !allowed_ok.contains(&reply.status.as_str()) {
            return Err(Self::map_failure(&reply.status));
        }
        let transcript_reads = reply.transcript_reads.unwrap_or(0);
        let token_reads = reply.token_reads.unwrap_or(0);
        let command_op = reply.command_op.ok_or(AntigravityFailure::InvalidReply)?;
        let request_id = reply.request_id.ok_or(AntigravityFailure::InvalidReply)?;
        let conversation_id = reply
            .conversation_id
            .ok_or(AntigravityFailure::InvalidReply)?;
        let workspace_path = reply
            .workspace_path
            .ok_or(AntigravityFailure::InvalidReply)?;
        let project_id = reply.project_id.ok_or(AntigravityFailure::InvalidReply)?;
        let delivery = reply.delivery.ok_or(AntigravityFailure::InvalidReply)?;
        let in_flight_steering = reply
            .in_flight_steering
            .ok_or(AntigravityFailure::InvalidReply)?;
        let reason_code = reply.reason_code.ok_or(AntigravityFailure::InvalidReply)?;
        let duplicate = reply.duplicate.ok_or(AntigravityFailure::InvalidReply)?;
        let reconciled = reply.reconciled.ok_or(AntigravityFailure::InvalidReply)?;
        let cursor = reply.cursor.ok_or(AntigravityFailure::InvalidReply)?;
        let rows = reply.rows.ok_or(AntigravityFailure::InvalidReply)?;

        let allowed_ops = ["create", "send", "stop", "resolve", "switch-model", "query"];
        if transcript_reads != 0
            || token_reads != 0
            || !allowed_ops.contains(&command_op.as_str())
            || request_id != expected_request_id
            || conversation_id.len() > 256
            || workspace_path.len() > 4096
            || (!workspace_path.is_empty() && !grant.allowed_workspaces.contains(&workspace_path))
            || project_id.len() > 256
            || !valid_antigravity_id(&reason_code, 256)
        {
            return Err(AntigravityFailure::InvalidReply);
        }

        let ws_scope: HashSet<&str> = grant
            .allowed_workspaces
            .iter()
            .map(String::as_str)
            .collect();
        let row_keys: HashSet<AntigravitySessionKey> =
            rows.iter().map(AntigravitySessionRow::key).collect();
        if rows.len() != row_keys.len()
            || !rows
                .iter()
                .all(|r| r.is_valid() && ws_scope.contains(r.workspace_path.as_str()))
            || reply
                .selected
                .as_ref()
                .is_some_and(|s| !row_keys.contains(s))
        {
            return Err(AntigravityFailure::InvalidReply);
        }
        if reply.status == "created" {
            let created_key = AntigravitySessionKey::new(&conversation_id, &workspace_path);
            if conversation_id.is_empty()
                || workspace_path.is_empty()
                || project_id.is_empty()
                || !row_keys.contains(&created_key)
                || reply.selected.as_ref() != Some(&created_key)
            {
                return Err(AntigravityFailure::InvalidReply);
            }
        }
        Ok(AntigravityCommandReply {
            instance_id: grant.instance_id.clone(),
            generation: grant.generation,
            status: reply.status,
            command_op,
            request_id,
            conversation_id,
            workspace_path,
            project_id,
            delivery,
            in_flight_steering,
            reason_code,
            duplicate,
            reconciled,
            cursor,
            rows,
            selected: reply.selected,
            transcript_reads,
            token_reads,
        })
    }
}

impl AntigravityStatusServing for AntigravityStatusClient {
    fn connect(&self) -> Result<AntigravityStatusSnapshot, AntigravityFailure> {
        self.close();
        let current = AntigravityGrant::load(&self.grant_path)?;
        let session = {
            let mut guard = self.state.lock().unwrap();
            guard.grant = Some(current.clone());
            let sub = format!("ag-sub-{}", guard.epoch + 1);
            guard.subscription_id = Some(sub.clone());
            (current, guard.epoch, sub)
        };
        let reply = self.exchange("subscribe", None, None, None, None, &session)?;
        let snap = self.parse_snapshot(reply, &session.0, None)?;
        self.state.lock().unwrap().cursor = Some(snap.cursor);
        Ok(snap)
    }

    fn read(&self) -> Result<AntigravityStatusSnapshot, AntigravityFailure> {
        let session = self.current_grant()?;
        let after = self
            .state
            .lock()
            .unwrap()
            .cursor
            .ok_or(AntigravityFailure::Stale)?;
        let reply = self.exchange("read", Some(after), None, None, None, &session)?;
        let snap = self.parse_snapshot(reply, &session.0, Some(after))?;
        self.state.lock().unwrap().cursor = Some(snap.cursor);
        Ok(snap)
    }

    fn open(
        &self,
        target: &AntigravitySessionKey,
    ) -> Result<AntigravityOpenReply, AntigravityFailure> {
        if !valid_antigravity_id(&target.conversation_id, 256)
            || !valid_antigravity_id(&target.workspace_path, 4096)
        {
            return Err(AntigravityFailure::Denied);
        }
        let session = self.current_grant()?;
        let current = &session.0;
        if !current.capabilities.supports_catalog_navigation()
            || !current.allowed_workspaces.contains(&target.workspace_path)
        {
            return Err(AntigravityFailure::Denied);
        }
        let reply = self.exchange(
            "open",
            None,
            Some(Self::CATALOG_NAV_SCHEMA),
            Some(target),
            None,
            &session,
        )?;
        let transcript_reads = reply.transcript_reads.unwrap_or(0);
        let token_reads = reply.token_reads.unwrap_or(0);
        if transcript_reads != 0 || token_reads != 0 {
            return Err(AntigravityFailure::InvalidReply);
        }
        if reply.status == "ok" || reply.status == "unconfirmed" {
            if let Some(sel) = &reply.selected {
                if !valid_antigravity_id(&sel.conversation_id, 256)
                    || !valid_antigravity_id(&sel.workspace_path, 4096)
                    || !current.allowed_workspaces.contains(&sel.workspace_path)
                {
                    return Err(AntigravityFailure::InvalidReply);
                }
            } else if reply.status == "ok" {
                return Err(AntigravityFailure::InvalidReply);
            }
            return Ok(AntigravityOpenReply {
                instance_id: current.instance_id.clone(),
                generation: current.generation,
                status: reply.status,
                selected: reply.selected,
                transcript_reads,
                token_reads,
            });
        }
        Err(Self::map_failure(&reply.status))
    }

    fn create(
        &self,
        intent: &AntigravityCreateIntent,
    ) -> Result<AntigravityCommandReply, AntigravityFailure> {
        if !intent.is_valid() {
            return Err(AntigravityFailure::Denied);
        }
        let session = self.current_grant()?;
        if !session.0.capabilities.supports_create_conversation()
            || !session
                .0
                .allowed_workspaces
                .contains(&intent.workspace_path)
        {
            return Err(AntigravityFailure::Denied);
        }
        let reply = self.exchange(
            "create",
            None,
            Some(Self::AGENTAPI_SCHEMA),
            None,
            Some(serde_json::to_value(intent).unwrap()),
            &session,
        )?;
        let parsed = self.parse_command_reply(reply, &session.0, &intent.request_id)?;
        self.state.lock().unwrap().cursor = Some(parsed.cursor);
        Ok(parsed)
    }

    fn send(
        &self,
        intent: &AntigravitySendIntent,
    ) -> Result<AntigravityCommandReply, AntigravityFailure> {
        if !intent.is_valid() {
            return Err(AntigravityFailure::Denied);
        }
        let session = self.current_grant()?;
        if !session.0.capabilities.supports_send_message()
            || !session
                .0
                .allowed_workspaces
                .contains(&intent.target.workspace_path)
        {
            return Err(AntigravityFailure::Denied);
        }
        let reply = self.exchange(
            "send",
            None,
            Some(Self::AGENTAPI_SCHEMA),
            None,
            Some(serde_json::to_value(intent).unwrap()),
            &session,
        )?;
        let parsed = self.parse_command_reply(reply, &session.0, &intent.request_id)?;
        self.state.lock().unwrap().cursor = Some(parsed.cursor);
        Ok(parsed)
    }

    fn stop(
        &self,
        intent: &AntigravityStopIntent,
    ) -> Result<AntigravityCommandReply, AntigravityFailure> {
        if !intent.is_valid() {
            return Err(AntigravityFailure::Denied);
        }
        let session = self.current_grant()?;
        if !session.0.capabilities.supports_stop_conversation()
            || !session
                .0
                .allowed_workspaces
                .contains(&intent.target.workspace_path)
        {
            return Err(AntigravityFailure::Denied);
        }
        let reply = self.exchange(
            "stop",
            None,
            Some(Self::STOP_SCHEMA),
            None,
            Some(serde_json::to_value(intent).unwrap()),
            &session,
        )?;
        let parsed = self.parse_command_reply(reply, &session.0, &intent.request_id)?;
        self.state.lock().unwrap().cursor = Some(parsed.cursor);
        Ok(parsed)
    }

    fn resolve(
        &self,
        intent: &AntigravityResolveIntent,
    ) -> Result<AntigravityCommandReply, AntigravityFailure> {
        if !intent.is_valid() {
            return Err(AntigravityFailure::Denied);
        }
        let session = self.current_grant()?;
        if !session.0.capabilities.supports_resolve_interaction()
            || !session
                .0
                .allowed_workspaces
                .contains(&intent.target.workspace_path)
        {
            return Err(AntigravityFailure::Denied);
        }
        let reply = self.exchange(
            "resolve",
            None,
            Some(Self::INTERACTION_SCHEMA),
            None,
            Some(serde_json::to_value(intent).unwrap()),
            &session,
        )?;
        let parsed = self.parse_command_reply(reply, &session.0, &intent.request_id)?;
        self.state.lock().unwrap().cursor = Some(parsed.cursor);
        Ok(parsed)
    }

    fn switch_model(
        &self,
        intent: &AntigravitySwitchModelIntent,
    ) -> Result<AntigravityCommandReply, AntigravityFailure> {
        if !intent.is_valid() {
            return Err(AntigravityFailure::Denied);
        }
        let session = self.current_grant()?;
        if !session.0.capabilities.supports_switch_model_config()
            || !session
                .0
                .allowed_workspaces
                .contains(&intent.target.workspace_path)
        {
            return Err(AntigravityFailure::Denied);
        }
        let reply = self.exchange(
            "switch-model",
            None,
            Some(Self::MODEL_CONFIG_SCHEMA),
            None,
            Some(serde_json::to_value(intent).unwrap()),
            &session,
        )?;
        let parsed = self.parse_command_reply(reply, &session.0, &intent.request_id)?;
        self.state.lock().unwrap().cursor = Some(parsed.cursor);
        Ok(parsed)
    }

    fn query(
        &self,
        intent: &AntigravityQueryIntent,
    ) -> Result<AntigravityCommandReply, AntigravityFailure> {
        if !intent.is_valid() {
            return Err(AntigravityFailure::Denied);
        }
        let session = self.current_grant()?;
        if !session.0.capabilities.schema_verified {
            return Err(AntigravityFailure::Denied);
        }
        if let Some(ws) = &intent.workspace_path {
            if !ws.is_empty() && !session.0.allowed_workspaces.contains(ws) {
                return Err(AntigravityFailure::Denied);
            }
        }
        let reply = self.exchange(
            "query",
            None,
            Some(Self::AGENTAPI_SCHEMA),
            None,
            Some(serde_json::to_value(intent).unwrap()),
            &session,
        )?;
        let parsed = self.parse_command_reply(reply, &session.0, &intent.request_id)?;
        self.state.lock().unwrap().cursor = Some(parsed.cursor);
        Ok(parsed)
    }

    fn close(&self) {
        let (prev_grant, prev_sub) = {
            let mut guard = self.state.lock().unwrap();
            let g = guard.grant.take();
            let s = guard.subscription_id.take();
            guard.epoch = guard.epoch.wrapping_add(1);
            guard.cursor = None;
            (g, s)
        };
        if let (Some(grant), Some(sub)) = (prev_grant, prev_sub) {
            let req = json!({
                "version": 1,
                "subscriptionId": sub,
                "id": "ag-close",
                "token": grant.token,
                "instanceId": grant.instance_id,
                "generation": grant.generation,
                "op": "close"
            });
            let _ = self.exchange_raw(&grant.socket_path, &req, Duration::from_secs(2));
        }
    }
}

pub struct AntigravityStatusController {
    pub enabled: bool,
    pub state: AntigravityConnectionState,
    pub capabilities: Option<AntigravityCapabilities>,
    pub rows: Vec<AntigravitySessionRow>,
    pub selected: Option<AntigravitySessionKey>,
    pub last_changes: Vec<AntigravityEventChange>,
    pub cursor: u64,
    pub transcript_reads: u64,
    pub token_reads: u64,
    pub last_result: Option<AntigravityOpenResult>,
    pub last_command_result: Option<AntigravityCommandResult>,
    pub last_command_reply: Option<AntigravityCommandReply>,
    pub pending_unknown_command: Option<AntigravityQueuedCommand>,
    client: Arc<dyn AntigravityStatusServing>,
    snapshot: Option<AntigravityStatusSnapshot>,
    epoch: u64,
    sequence: u64,
    next_request_sequence: u64,
    suspended: bool,
}

impl AntigravityStatusController {
    pub fn new(client: Arc<dyn AntigravityStatusServing>) -> Self {
        Self {
            enabled: false,
            state: AntigravityConnectionState::Disabled,
            capabilities: None,
            rows: Vec::new(),
            selected: None,
            last_changes: Vec::new(),
            cursor: 0,
            transcript_reads: 0,
            token_reads: 0,
            last_result: None,
            last_command_result: None,
            last_command_reply: None,
            pending_unknown_command: None,
            client,
            snapshot: None,
            epoch: 0,
            sequence: 0,
            next_request_sequence: 0,
            suspended: false,
        }
    }

    pub fn set_voice_reply(&self, _enabled: bool) -> (bool, String) {
        (
            false,
            "Antigravity 2.29.0 language_server does not expose speech generation or voice reply mode"
                .into(),
        )
    }

    pub fn set_enabled(&mut self, value: bool) {
        self.enabled = value;
        self.invalidate();
        if value && !self.suspended {
            self.start();
        }
    }

    pub fn suspend(&mut self) {
        self.suspended = true;
        self.invalidate();
    }

    pub fn resume(&mut self) {
        if !self.suspended {
            return;
        }
        self.suspended = false;
        if self.enabled {
            self.start();
        }
    }

    pub fn shutdown(&mut self) {
        self.enabled = false;
        self.invalidate();
    }

    pub fn row(&self, key: &AntigravitySessionKey) -> Option<&AntigravitySessionRow> {
        if !self.enabled || self.suspended || self.state != AntigravityConnectionState::Ready {
            return None;
        }
        self.rows.iter().find(|r| &r.key() == key)
    }

    pub fn selection(&self, key: &AntigravitySessionKey) -> Option<AntigravitySelection> {
        if !self.enabled || self.suspended || self.state != AntigravityConnectionState::Ready {
            return None;
        }
        let snap = self.snapshot.as_ref()?;
        if !self.rows.iter().any(|r| &r.key() == key) {
            return None;
        }
        Some(AntigravitySelection {
            key: key.clone(),
            instance_id: snap.instance_id.clone(),
            generation: snap.generation,
            local_epoch: self.epoch,
        })
    }

    pub fn open(
        &mut self,
        selection: Option<&AntigravitySelection>,
        admitted: impl Fn() -> bool,
    ) -> AntigravityOpenResult {
        let Some(selection) = selection else {
            return AntigravityOpenResult::Stale;
        };
        if !self.enabled || self.suspended || self.state != AntigravityConnectionState::Ready {
            return AntigravityOpenResult::Stale;
        }
        let Some(snap) = self.snapshot.clone() else {
            return AntigravityOpenResult::Stale;
        };
        let expected = self.epoch;
        if !snap.capabilities.supports_catalog_navigation() {
            self.fail(AntigravityFailure::Denied, expected);
            self.last_result = Some(AntigravityOpenResult::Denied);
            return AntigravityOpenResult::Denied;
        }
        if selection.local_epoch != self.epoch
            || selection.instance_id != snap.instance_id
            || selection.generation != snap.generation
            || !snap
                .allowed_workspaces
                .contains(&selection.key.workspace_path)
            || !self.rows.iter().any(|r| r.key() == selection.key)
            || !admitted()
        {
            return AntigravityOpenResult::Stale;
        }
        self.sequence = self.sequence.wrapping_add(1);
        match self.client.open(&selection.key) {
            Ok(response) => {
                if !self.current(expected) || !admitted() {
                    return AntigravityOpenResult::Stale;
                }
                if response.instance_id != selection.instance_id
                    || response.generation != selection.generation
                    || response.transcript_reads != 0
                    || response.token_reads != 0
                {
                    self.fail(AntigravityFailure::Stale, expected);
                    self.last_result = Some(AntigravityOpenResult::Stale);
                    return AntigravityOpenResult::Stale;
                }
                let is_confirmed = response.status == "ok"
                    && response.selected.as_ref() == Some(&selection.key)
                    && self.rows.iter().any(|r| r.key() == selection.key);
                let result = if is_confirmed {
                    AntigravityOpenResult::Confirmed
                } else {
                    AntigravityOpenResult::Unconfirmed
                };
                if is_confirmed {
                    self.selected = Some(selection.key.clone());
                } else if let Some(resp_sel) = response.selected {
                    if self.rows.iter().any(|r| r.key() == resp_sel) {
                        self.selected = Some(resp_sel);
                    }
                }
                self.last_result = Some(result);
                result
            }
            Err(err) => {
                self.fail(err, expected);
                let result = match err {
                    AntigravityFailure::Denied => AntigravityOpenResult::Denied,
                    AntigravityFailure::Stale => AntigravityOpenResult::Stale,
                    _ => AntigravityOpenResult::Unavailable,
                };
                self.last_result = Some(result);
                result
            }
        }
    }

    fn make_request_id(&mut self, explicit: Option<&str>) -> String {
        if let Some(id) = explicit {
            if !id.is_empty() {
                return id.to_owned();
            }
        }
        self.next_request_sequence = self.next_request_sequence.wrapping_add(1);
        format!("ag-req-{}-{}", self.epoch, self.next_request_sequence)
    }

    pub fn create_intent(
        &mut self,
        workspace_path: &str,
        project_id: Option<&str>,
        prompt: &str,
        title: Option<&str>,
        model_name: Option<&str>,
        ids: (Option<&str>, Option<&str>),
    ) -> Option<AntigravityQueuedCommand> {
        let (conversation_id, request_id) = ids;
        if !self.enabled || self.suspended || self.state != AntigravityConnectionState::Ready {
            return None;
        }
        let snap = self.snapshot.clone()?;
        if let Some(pending) = &self.pending_unknown_command {
            if request_id != Some(pending.operation.request_id()) {
                self.last_command_result = Some(AntigravityCommandResult::Unknown);
                return None;
            }
        }
        let resolved_project = project_id
            .filter(|p| !p.is_empty())
            .map(String::from)
            .or_else(|| {
                self.rows
                    .iter()
                    .find(|r| r.workspace_path == workspace_path && !r.project_id.is_empty())
                    .map(|r| r.project_id.clone())
            })
            .unwrap_or_default();
        let req_id = self.make_request_id(request_id);
        let intent = AntigravityCreateIntent {
            request_id: req_id,
            workspace_path: workspace_path.into(),
            project_id: resolved_project,
            prompt: prompt.into(),
            title: title.map(String::from),
            model_name: model_name.map(String::from),
            conversation_id: conversation_id.map(String::from),
        };
        if !intent.is_valid() {
            return None;
        }
        Some(AntigravityQueuedCommand {
            operation: AntigravityCommandOperation::Create(intent),
            instance_id: snap.instance_id,
            generation: snap.generation,
            local_epoch: self.epoch,
        })
    }

    pub fn send_intent(
        &mut self,
        target: &AntigravitySessionKey,
        project_id: Option<&str>,
        message: &str,
        delivery_mode: AntigravityDeliveryMode,
        request_id: Option<&str>,
    ) -> Option<AntigravityQueuedCommand> {
        if !self.enabled || self.suspended || self.state != AntigravityConnectionState::Ready {
            return None;
        }
        let snap = self.snapshot.clone()?;
        if let Some(pending) = &self.pending_unknown_command {
            if request_id != Some(pending.operation.request_id()) {
                self.last_command_result = Some(AntigravityCommandResult::Unknown);
                return None;
            }
        }
        let resolved_project = project_id
            .filter(|p| !p.is_empty())
            .map(String::from)
            .or_else(|| {
                self.rows
                    .iter()
                    .find(|r| &r.key() == target && !r.project_id.is_empty())
                    .map(|r| r.project_id.clone())
            });
        let req_id = self.make_request_id(request_id);
        let intent = AntigravitySendIntent {
            request_id: req_id,
            target: target.clone(),
            project_id: resolved_project,
            message: message.into(),
            delivery_mode,
        };
        if !intent.is_valid() {
            return None;
        }
        Some(AntigravityQueuedCommand {
            operation: AntigravityCommandOperation::Send(intent),
            instance_id: snap.instance_id,
            generation: snap.generation,
            local_epoch: self.epoch,
        })
    }

    pub fn stop_intent(
        &mut self,
        target: &AntigravitySessionKey,
        expected_invocation_num: Option<u64>,
        expected_execution_num: Option<u64>,
        kill_background_tasks: bool,
        request_id: Option<&str>,
    ) -> Option<AntigravityQueuedCommand> {
        if !self.enabled || self.suspended || self.state != AntigravityConnectionState::Ready {
            return None;
        }
        let snap = self.snapshot.clone()?;
        if let Some(pending) = &self.pending_unknown_command {
            if request_id != Some(pending.operation.request_id()) {
                self.last_command_result = Some(AntigravityCommandResult::Unknown);
                return None;
            }
        }
        let target_row = self.rows.iter().find(|r| &r.key() == target);
        let resolved_inv = expected_invocation_num
            .or_else(|| target_row.map(|r| r.invocation_num))
            .unwrap_or(0);
        let resolved_exec = expected_execution_num.or_else(|| target_row.map(|r| r.execution_num));
        let req_id = self.make_request_id(request_id);
        let intent = AntigravityStopIntent {
            request_id: req_id,
            target: target.clone(),
            expected_invocation_num: resolved_inv,
            expected_execution_num: resolved_exec,
            kill_background_tasks,
        };
        if !intent.is_valid() {
            return None;
        }
        Some(AntigravityQueuedCommand {
            operation: AntigravityCommandOperation::Stop(intent),
            instance_id: snap.instance_id,
            generation: snap.generation,
            local_epoch: self.epoch,
        })
    }

    pub fn resolve_intent(
        &mut self,
        target: &AntigravitySessionKey,
        interaction_id: &str,
        interaction_type: AntigravityInteractionType,
        decision: AntigravityInteractionDecision,
        selected_option_ids: Vec<String>,
        request_id: Option<&str>,
    ) -> Option<AntigravityQueuedCommand> {
        if !self.enabled || self.suspended || self.state != AntigravityConnectionState::Ready {
            return None;
        }
        let snap = self.snapshot.clone()?;
        if let Some(pending) = &self.pending_unknown_command {
            if request_id != Some(pending.operation.request_id()) {
                self.last_command_result = Some(AntigravityCommandResult::Unknown);
                return None;
            }
        }
        let req_id = self.make_request_id(request_id);
        let intent = AntigravityResolveIntent {
            request_id: req_id,
            target: target.clone(),
            interaction_id: interaction_id.into(),
            interaction_type,
            decision,
            selected_option_ids,
        };
        if !intent.is_valid() {
            return None;
        }
        Some(AntigravityQueuedCommand {
            operation: AntigravityCommandOperation::Resolve(intent),
            instance_id: snap.instance_id,
            generation: snap.generation,
            local_epoch: self.epoch,
        })
    }

    pub fn switch_model_intent(
        &mut self,
        target: &AntigravitySessionKey,
        model_name: &str,
        thinking_level: &str,
        request_id: Option<&str>,
    ) -> Option<AntigravityQueuedCommand> {
        if !self.enabled || self.suspended || self.state != AntigravityConnectionState::Ready {
            return None;
        }
        let snap = self.snapshot.clone()?;
        if let Some(pending) = &self.pending_unknown_command {
            if request_id != Some(pending.operation.request_id()) {
                self.last_command_result = Some(AntigravityCommandResult::Unknown);
                return None;
            }
        }
        let req_id = self.make_request_id(request_id);
        let intent = AntigravitySwitchModelIntent {
            request_id: req_id,
            target: target.clone(),
            model_name: model_name.into(),
            thinking_level: thinking_level.into(),
        };
        if !intent.is_valid() {
            return None;
        }
        Some(AntigravityQueuedCommand {
            operation: AntigravityCommandOperation::SwitchModel(intent),
            instance_id: snap.instance_id,
            generation: snap.generation,
            local_epoch: self.epoch,
        })
    }

    pub fn query_intent(
        &self,
        request_id: Option<&str>,
        expected_op: Option<&str>,
        target: Option<&AntigravitySessionKey>,
        project_id: Option<&str>,
    ) -> Option<AntigravityQueuedCommand> {
        if !self.enabled || self.suspended || self.state != AntigravityConnectionState::Ready {
            return None;
        }
        let snap = self.snapshot.as_ref()?;
        let resolved_req_id = request_id
            .filter(|id| !id.is_empty())
            .map(String::from)
            .or_else(|| {
                self.pending_unknown_command
                    .as_ref()
                    .map(|p| p.operation.request_id().to_owned())
            })?;
        let intent = AntigravityQueryIntent {
            request_id: resolved_req_id,
            expected_op: expected_op.map(String::from).or_else(|| {
                self.pending_unknown_command
                    .as_ref()
                    .map(|p| p.operation.op_name().to_owned())
            }),
            conversation_id: target.map(|t| t.conversation_id.clone()),
            workspace_path: target.map(|t| t.workspace_path.clone()),
            project_id: project_id.map(String::from),
        };
        if !intent.is_valid() {
            return None;
        }
        Some(AntigravityQueuedCommand {
            operation: AntigravityCommandOperation::Query(intent),
            instance_id: snap.instance_id.clone(),
            generation: snap.generation,
            local_epoch: self.epoch,
        })
    }

    fn record_unconfirmed_guard(
        &mut self,
        command_op: &str,
        request_id: &str,
        conversation_id: &str,
        workspace_path: &str,
        project_id: &str,
        reason_code: &str,
    ) -> AntigravityCommandResult {
        let (instance_id, generation) = self
            .snapshot
            .as_ref()
            .map(|s| (s.instance_id.clone(), s.generation))
            .unwrap_or_default();
        self.last_command_reply = Some(AntigravityCommandReply {
            instance_id,
            generation,
            status: "unconfirmed".into(),
            command_op: command_op.into(),
            request_id: request_id.into(),
            conversation_id: conversation_id.into(),
            workspace_path: workspace_path.into(),
            project_id: project_id.into(),
            delivery: AntigravityDeliveryOutcome::None,
            in_flight_steering: false,
            reason_code: reason_code.into(),
            duplicate: false,
            reconciled: false,
            cursor: self.cursor,
            rows: self.rows.clone(),
            selected: self.selected.clone(),
            transcript_reads: 0,
            token_reads: 0,
        });
        self.last_command_result = Some(AntigravityCommandResult::Unconfirmed);
        AntigravityCommandResult::Unconfirmed
    }

    pub fn execute(
        &mut self,
        command: Option<&AntigravityQueuedCommand>,
        admitted: impl Fn() -> bool,
    ) -> AntigravityCommandResult {
        let Some(command) = command else {
            let fallback = self
                .last_command_result
                .unwrap_or(AntigravityCommandResult::Stale);
            self.last_command_result = Some(fallback);
            return fallback;
        };
        let Some(current_snap) = self.snapshot.clone() else {
            self.last_command_result = Some(AntigravityCommandResult::Stale);
            return AntigravityCommandResult::Stale;
        };
        if !self.enabled
            || self.suspended
            || self.state != AntigravityConnectionState::Ready
            || command.local_epoch != self.epoch
            || command.instance_id != current_snap.instance_id
            || command.generation != current_snap.generation
            || !admitted()
        {
            self.last_command_result = Some(AntigravityCommandResult::Stale);
            return AntigravityCommandResult::Stale;
        }
        let expected = self.epoch;
        if let Some(pending) = &self.pending_unknown_command {
            if pending.operation.request_id() != command.operation.request_id() {
                self.last_command_result = Some(AntigravityCommandResult::Unknown);
                return AntigravityCommandResult::Unknown;
            }
        }
        let effective_op = match &self.pending_unknown_command {
            Some(pending)
                if pending.operation.request_id() == command.operation.request_id()
                    && command.operation.op_name() != "query" =>
            {
                AntigravityCommandOperation::Query(AntigravityQueryIntent {
                    request_id: pending.operation.request_id().to_owned(),
                    expected_op: Some(pending.operation.op_name().to_owned()),
                    conversation_id: None,
                    workspace_path: None,
                    project_id: None,
                })
            }
            _ => command.operation.clone(),
        };

        match &effective_op {
            AntigravityCommandOperation::Create(intent) => {
                if !current_snap.capabilities.supports_create_conversation()
                    || !current_snap
                        .allowed_workspaces
                        .contains(&intent.workspace_path)
                    || !intent.is_valid()
                {
                    self.fail(AntigravityFailure::Denied, expected);
                    self.last_command_result = Some(AntigravityCommandResult::Denied);
                    return AntigravityCommandResult::Denied;
                }
                if let Some(bound_proj) = self
                    .rows
                    .iter()
                    .find(|r| r.workspace_path == intent.workspace_path && !r.project_id.is_empty())
                    .map(|r| r.project_id.clone())
                {
                    if bound_proj != intent.project_id {
                        return self.record_unconfirmed_guard(
                            "create",
                            &intent.request_id,
                            "",
                            &intent.workspace_path,
                            &intent.project_id,
                            "guard.projectIdMismatch",
                        );
                    }
                }
            }
            AntigravityCommandOperation::Send(intent) => {
                if !current_snap.capabilities.supports_send_message()
                    || !current_snap
                        .allowed_workspaces
                        .contains(&intent.target.workspace_path)
                    || !intent.is_valid()
                {
                    self.fail(AntigravityFailure::Denied, expected);
                    self.last_command_result = Some(AntigravityCommandResult::Denied);
                    return AntigravityCommandResult::Denied;
                }
                let Some(target_row) = self.rows.iter().find(|r| r.key() == intent.target).cloned()
                else {
                    return self.record_unconfirmed_guard(
                        "send",
                        &intent.request_id,
                        &intent.target.conversation_id,
                        &intent.target.workspace_path,
                        intent.project_id.as_deref().unwrap_or(""),
                        "guard.targetNotFound",
                    );
                };
                if self.selected.as_ref() != Some(&intent.target) {
                    return self.record_unconfirmed_guard(
                        "send",
                        &intent.request_id,
                        &intent.target.conversation_id,
                        &intent.target.workspace_path,
                        &target_row.project_id,
                        "guard.selectionMismatch",
                    );
                }
            }
            AntigravityCommandOperation::Stop(intent) => {
                if !current_snap.capabilities.supports_stop_conversation()
                    || !current_snap
                        .allowed_workspaces
                        .contains(&intent.target.workspace_path)
                    || !intent.is_valid()
                {
                    self.fail(AntigravityFailure::Denied, expected);
                    self.last_command_result = Some(AntigravityCommandResult::Denied);
                    return AntigravityCommandResult::Denied;
                }
                let Some(target_row) = self.rows.iter().find(|r| r.key() == intent.target).cloned()
                else {
                    return self.record_unconfirmed_guard(
                        "stop",
                        &intent.request_id,
                        &intent.target.conversation_id,
                        &intent.target.workspace_path,
                        "",
                        "guard.targetNotFound",
                    );
                };
                if self.selected.as_ref() != Some(&intent.target) {
                    return self.record_unconfirmed_guard(
                        "stop",
                        &intent.request_id,
                        &intent.target.conversation_id,
                        &intent.target.workspace_path,
                        &target_row.project_id,
                        "guard.selectionMismatch",
                    );
                }
            }
            AntigravityCommandOperation::Resolve(intent) => {
                if !current_snap.capabilities.supports_resolve_interaction()
                    || !current_snap
                        .allowed_workspaces
                        .contains(&intent.target.workspace_path)
                    || !intent.is_valid()
                {
                    self.fail(AntigravityFailure::Denied, expected);
                    self.last_command_result = Some(AntigravityCommandResult::Denied);
                    return AntigravityCommandResult::Denied;
                }
                let Some(target_row) = self.rows.iter().find(|r| r.key() == intent.target).cloned()
                else {
                    return self.record_unconfirmed_guard(
                        "resolve",
                        &intent.request_id,
                        &intent.target.conversation_id,
                        &intent.target.workspace_path,
                        "",
                        "guard.targetNotFound",
                    );
                };
                if self.selected.as_ref() != Some(&intent.target) {
                    return self.record_unconfirmed_guard(
                        "resolve",
                        &intent.request_id,
                        &intent.target.conversation_id,
                        &intent.target.workspace_path,
                        &target_row.project_id,
                        "guard.selectionMismatch",
                    );
                }
                let is_question_as_permission = (intent.interaction_type
                    == AntigravityInteractionType::Question
                    && matches!(
                        intent.decision,
                        AntigravityInteractionDecision::Allow
                            | AntigravityInteractionDecision::Deny
                    ))
                    || (target_row.pending_interaction.as_ref().is_some_and(|p| {
                        p.interaction_type == AntigravityInteractionType::Question
                    }) && (intent.interaction_type != AntigravityInteractionType::Question
                        || matches!(
                            intent.decision,
                            AntigravityInteractionDecision::Allow
                                | AntigravityInteractionDecision::Deny
                        )));
                if is_question_as_permission {
                    return self.record_unconfirmed_guard(
                        "resolve",
                        &intent.request_id,
                        &intent.target.conversation_id,
                        &intent.target.workspace_path,
                        &target_row.project_id,
                        "guard.questionCannotApproveAsPermission",
                    );
                }
            }
            AntigravityCommandOperation::SwitchModel(intent) => {
                if !current_snap.capabilities.supports_switch_model_config()
                    || !current_snap
                        .allowed_workspaces
                        .contains(&intent.target.workspace_path)
                    || !intent.is_valid()
                {
                    self.fail(AntigravityFailure::Denied, expected);
                    self.last_command_result = Some(AntigravityCommandResult::Denied);
                    return AntigravityCommandResult::Denied;
                }
                let Some(target_row) = self.rows.iter().find(|r| r.key() == intent.target).cloned()
                else {
                    return self.record_unconfirmed_guard(
                        "switch-model",
                        &intent.request_id,
                        &intent.target.conversation_id,
                        &intent.target.workspace_path,
                        "",
                        "guard.targetNotFound",
                    );
                };
                if self.selected.as_ref() != Some(&intent.target) {
                    return self.record_unconfirmed_guard(
                        "switch-model",
                        &intent.request_id,
                        &intent.target.conversation_id,
                        &intent.target.workspace_path,
                        &target_row.project_id,
                        "guard.selectionMismatch",
                    );
                }
                if !target_row
                    .available_models
                    .iter()
                    .any(|m| m.model_name == intent.model_name)
                {
                    return self.record_unconfirmed_guard(
                        "switch-model",
                        &intent.request_id,
                        &intent.target.conversation_id,
                        &intent.target.workspace_path,
                        &target_row.project_id,
                        "guard.unsupportedModel",
                    );
                }
                if !target_row.supports_thinking_level(&intent.thinking_level, &intent.model_name) {
                    return self.record_unconfirmed_guard(
                        "switch-model",
                        &intent.request_id,
                        &intent.target.conversation_id,
                        &intent.target.workspace_path,
                        &target_row.project_id,
                        "guard.unsupportedThinkingLevel",
                    );
                }
            }
            AntigravityCommandOperation::Query(intent) => {
                if !current_snap.capabilities.schema_verified || !intent.is_valid() {
                    self.fail(AntigravityFailure::Denied, expected);
                    self.last_command_result = Some(AntigravityCommandResult::Denied);
                    return AntigravityCommandResult::Denied;
                }
            }
        }

        self.sequence = self.sequence.wrapping_add(1);
        let outcome = match &effective_op {
            AntigravityCommandOperation::Create(i) => self.client.create(i),
            AntigravityCommandOperation::Send(i) => self.client.send(i),
            AntigravityCommandOperation::Stop(i) => self.client.stop(i),
            AntigravityCommandOperation::Resolve(i) => self.client.resolve(i),
            AntigravityCommandOperation::SwitchModel(i) => self.client.switch_model(i),
            AntigravityCommandOperation::Query(i) => self.client.query(i),
        };

        match outcome {
            Ok(response) => {
                if !self.current(expected) || !admitted() {
                    self.last_command_result = Some(AntigravityCommandResult::Stale);
                    return AntigravityCommandResult::Stale;
                }
                if response.instance_id != command.instance_id
                    || response.generation != command.generation
                    || response.transcript_reads != 0
                    || response.token_reads != 0
                {
                    self.fail(AntigravityFailure::Stale, expected);
                    self.last_command_result = Some(AntigravityCommandResult::Stale);
                    return AntigravityCommandResult::Stale;
                }
                let updated_snap = AntigravityStatusSnapshot {
                    instance_id: current_snap.instance_id,
                    generation: current_snap.generation,
                    allowed_workspaces: current_snap.allowed_workspaces,
                    capabilities: current_snap.capabilities,
                    cursor: response.cursor,
                    rows: response.rows.clone(),
                    changes: Vec::new(),
                    selected: response.selected.clone(),
                    transcript_reads: response.transcript_reads,
                    token_reads: response.token_reads,
                };
                self.snapshot = Some(updated_snap);
                self.rows = response.rows.clone();
                self.selected = response.selected.clone();
                self.cursor = response.cursor;
                self.transcript_reads = response.transcript_reads;
                self.token_reads = response.token_reads;
                let mapped = Self::map_command_reply(&response, &command.operation);
                if mapped == AntigravityCommandResult::Unknown {
                    if self.pending_unknown_command.is_none() {
                        self.pending_unknown_command = Some(command.clone());
                    }
                } else if self
                    .pending_unknown_command
                    .as_ref()
                    .is_some_and(|p| p.operation.request_id() == response.request_id)
                {
                    self.pending_unknown_command = None;
                }
                self.last_command_reply = Some(response);
                self.last_command_result = Some(mapped);
                mapped
            }
            Err(AntigravityFailure::TransportTimeout) => {
                if !self.current(expected) {
                    return AntigravityCommandResult::Stale;
                }
                if self.pending_unknown_command.is_none() {
                    self.pending_unknown_command = Some(command.clone());
                }
                self.last_command_reply = Some(AntigravityCommandReply {
                    instance_id: current_snap.instance_id,
                    generation: current_snap.generation,
                    status: "unknown".into(),
                    command_op: command.operation.op_name().into(),
                    request_id: command.operation.request_id().into(),
                    conversation_id: String::new(),
                    workspace_path: String::new(),
                    project_id: String::new(),
                    delivery: AntigravityDeliveryOutcome::None,
                    in_flight_steering: false,
                    reason_code: "transport.timeout".into(),
                    duplicate: false,
                    reconciled: false,
                    cursor: self.cursor,
                    rows: self.rows.clone(),
                    selected: self.selected.clone(),
                    transcript_reads: 0,
                    token_reads: 0,
                });
                self.last_command_result = Some(AntigravityCommandResult::Unknown);
                AntigravityCommandResult::Unknown
            }
            Err(err) => {
                self.fail(err, expected);
                let result = match err {
                    AntigravityFailure::Denied => AntigravityCommandResult::Denied,
                    AntigravityFailure::Stale => AntigravityCommandResult::Stale,
                    _ => AntigravityCommandResult::Unavailable,
                };
                self.last_command_result = Some(result);
                result
            }
        }
    }

    fn map_command_reply(
        response: &AntigravityCommandReply,
        operation: &AntigravityCommandOperation,
    ) -> AntigravityCommandResult {
        let target_key =
            AntigravitySessionKey::new(&response.conversation_id, &response.workspace_path);
        match response.status.as_str() {
            "created" => {
                if response.conversation_id.is_empty()
                    || response.workspace_path.is_empty()
                    || response.project_id.is_empty()
                    || response.selected.as_ref() != Some(&target_key)
                    || !response
                        .rows
                        .iter()
                        .any(|r| r.key() == target_key && r.project_id == response.project_id)
                {
                    AntigravityCommandResult::Unconfirmed
                } else {
                    AntigravityCommandResult::Created
                }
            }
            "started" => {
                if response.delivery == AntigravityDeliveryOutcome::Started
                    && !response.in_flight_steering
                    && response.selected.as_ref() == Some(&target_key)
                    && response.rows.iter().any(|r| r.key() == target_key)
                {
                    AntigravityCommandResult::Started
                } else {
                    AntigravityCommandResult::Unconfirmed
                }
            }
            "queued" => {
                if response.delivery == AntigravityDeliveryOutcome::NextInvocation
                    && !response.in_flight_steering
                    && response.selected.as_ref() == Some(&target_key)
                    && response.rows.iter().any(|r| r.key() == target_key)
                {
                    AntigravityCommandResult::Queued
                } else {
                    AntigravityCommandResult::Unconfirmed
                }
            }
            "steered" => {
                if response.delivery == AntigravityDeliveryOutcome::Steered
                    && response.in_flight_steering
                    && response.selected.as_ref() == Some(&target_key)
                    && response.rows.iter().any(|r| r.key() == target_key)
                {
                    AntigravityCommandResult::Steered
                } else {
                    AntigravityCommandResult::Unconfirmed
                }
            }
            "stopped" => {
                if response.delivery == AntigravityDeliveryOutcome::Canceled
                    && !response.in_flight_steering
                    && response.selected.as_ref() == Some(&target_key)
                    && response.rows.iter().any(|r| {
                        r.key() == target_key
                            && r.phase == AntigravitySessionPhase::Canceled
                            && r.termination_reason.as_deref() == Some("USER_CANCELED")
                    })
                {
                    AntigravityCommandResult::Stopped
                } else {
                    AntigravityCommandResult::Unconfirmed
                }
            }
            "resolved" => {
                let valid_deliv = matches!(
                    response.delivery,
                    AntigravityDeliveryOutcome::Allow
                        | AntigravityDeliveryOutcome::Deny
                        | AntigravityDeliveryOutcome::Answer
                        | AntigravityDeliveryOutcome::Cancel
                );
                if valid_deliv
                    && !response.in_flight_steering
                    && response.selected.as_ref() == Some(&target_key)
                    && response
                        .rows
                        .iter()
                        .any(|r| r.key() == target_key && r.pending_interaction.is_none())
                {
                    AntigravityCommandResult::Resolved
                } else {
                    AntigravityCommandResult::Unconfirmed
                }
            }
            "modelSwitched" => {
                if response.delivery != AntigravityDeliveryOutcome::ModelSwitched
                    || response.in_flight_steering
                    || response.selected.as_ref() != Some(&target_key)
                {
                    return AntigravityCommandResult::Unconfirmed;
                }
                let Some(target_row) = response.rows.iter().find(|r| r.key() == target_key) else {
                    return AntigravityCommandResult::Unconfirmed;
                };
                if let AntigravityCommandOperation::SwitchModel(intent) = operation {
                    if target_row.model_name != intent.model_name
                        || target_row.thinking_level != intent.thinking_level
                    {
                        return AntigravityCommandResult::Unconfirmed;
                    }
                }
                AntigravityCommandResult::ModelSwitched
            }
            "noop" => AntigravityCommandResult::Noop,
            "unknown" => AntigravityCommandResult::Unknown,
            _ => AntigravityCommandResult::Unconfirmed,
        }
    }

    fn invalidate(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.sequence = self.sequence.wrapping_add(1);
        self.snapshot = None;
        self.capabilities = None;
        self.rows.clear();
        self.selected = None;
        self.last_changes.clear();
        self.cursor = 0;
        self.transcript_reads = 0;
        self.token_reads = 0;
        self.last_result = None;
        self.last_command_result = None;
        self.last_command_reply = None;
        self.pending_unknown_command = None;
        self.state = AntigravityConnectionState::Disabled;
        self.client.close();
    }

    fn start(&mut self) {
        let expected = self.epoch;
        self.state = AntigravityConnectionState::Connecting;
        match self.client.connect() {
            Ok(result) => {
                if let Err(err) = self.accept(result) {
                    self.fail(err, expected);
                }
            }
            Err(err) => {
                self.fail(err, expected);
            }
        }
    }

    fn current(&self, expected: u64) -> bool {
        self.epoch == expected && self.enabled && !self.suspended
    }

    fn accept(&mut self, value: AntigravityStatusSnapshot) -> Result<(), AntigravityFailure> {
        value.validate(self.snapshot.as_ref().map(|s| s.cursor))?;
        if let Some(prev) = &self.snapshot {
            if prev.instance_id != value.instance_id
                || prev.generation != value.generation
                || prev.allowed_workspaces != value.allowed_workspaces
                || prev.capabilities != value.capabilities
            {
                return Err(AntigravityFailure::Stale);
            }
        }
        self.capabilities = Some(value.capabilities.clone());
        self.rows = value.rows.clone();
        self.selected = value.selected.clone();
        self.last_changes = value.changes.clone();
        self.cursor = value.cursor;
        self.transcript_reads = value.transcript_reads;
        self.token_reads = value.token_reads;
        self.snapshot = Some(value);
        self.state = AntigravityConnectionState::Ready;
        Ok(())
    }

    fn fail(&mut self, err: AntigravityFailure, expected: u64) {
        if !self.current(expected) {
            return;
        }
        self.snapshot = None;
        self.capabilities = None;
        self.rows.clear();
        self.selected = None;
        self.last_changes.clear();
        self.cursor = 0;
        self.transcript_reads = 0;
        self.token_reads = 0;
        self.last_result = None;
        self.last_command_result = None;
        self.last_command_reply = None;
        self.pending_unknown_command = None;
        self.state = AntigravityConnectionState::Unavailable(err);
        self.client.close();
    }

    pub fn refresh(&mut self) {
        let expected = self.epoch;
        if !self.current(expected) || self.state != AntigravityConnectionState::Ready {
            return;
        }
        self.sequence = self.sequence.wrapping_add(1);
        match self.client.read() {
            Ok(value) => {
                if let Err(err) = self.accept(value) {
                    self.fail(err, expected);
                }
            }
            Err(err) => {
                self.fail(err, expected);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AntigravityFixtureMode {
    Normal,
    Stale,
    Rotated,
    Gap,
    WrongSelected,
    MissingSchema,
}

#[derive(Debug)]
struct AntigravityFixtureState {
    mode: AntigravityFixtureMode,
    connected: bool,
    cursor: u64,
    rows: BTreeMap<AntigravitySessionKey, AntigravitySessionRow>,
    seen_keys: HashMap<AntigravitySessionKey, HashSet<String>>,
    pending_changes: Vec<AntigravityEventChange>,
    selected: Option<AntigravitySessionKey>,
    metadata_reads: usize,
    duplicate_events_dropped: usize,
    transcript_reads: usize,
    token_reads: usize,
    open_requests: usize,
    open_confirmed: usize,
    create_requests: usize,
    create_confirmed: usize,
    send_requests: usize,
    send_confirmed: usize,
    query_requests: usize,
    stop_requests: usize,
    stop_confirmed: usize,
    resolve_requests: usize,
    resolve_confirmed: usize,
    switch_model_requests: usize,
    switch_model_confirmed: usize,
    duplicate_commands_dropped: usize,
    created_seq: usize,
    workspace_projects: HashMap<String, String>,
    command_ledger: HashMap<String, AntigravityCommandReply>,
    expired_interactions: HashMap<AntigravitySessionKey, HashSet<String>>,
    stop_schema_verified: bool,
    approval_schema_verified: bool,
    model_config_schema_verified: bool,
    csrf_auth_verified: bool,
    model_readback_override: Option<(Option<String>, Option<String>)>,
    unknown_next_command: bool,
    in_flight_steer_enabled: bool,
    closes: usize,
}

impl Default for AntigravityFixtureState {
    fn default() -> Self {
        Self {
            mode: AntigravityFixtureMode::Normal,
            connected: false,
            cursor: 0,
            rows: BTreeMap::new(),
            seen_keys: HashMap::new(),
            pending_changes: Vec::new(),
            selected: None,
            metadata_reads: 0,
            duplicate_events_dropped: 0,
            transcript_reads: 0,
            token_reads: 0,
            open_requests: 0,
            open_confirmed: 0,
            create_requests: 0,
            create_confirmed: 0,
            send_requests: 0,
            send_confirmed: 0,
            query_requests: 0,
            stop_requests: 0,
            stop_confirmed: 0,
            resolve_requests: 0,
            resolve_confirmed: 0,
            switch_model_requests: 0,
            switch_model_confirmed: 0,
            duplicate_commands_dropped: 0,
            created_seq: 0,
            workspace_projects: HashMap::new(),
            command_ledger: HashMap::new(),
            expired_interactions: HashMap::new(),
            stop_schema_verified: true,
            approval_schema_verified: true,
            model_config_schema_verified: true,
            csrf_auth_verified: true,
            model_readback_override: None,
            unknown_next_command: false,
            in_flight_steer_enabled: false,
            closes: 0,
        }
    }
}

#[derive(Debug, Default)]
pub struct AntigravityStatusFixture {
    state: Mutex<AntigravityFixtureState>,
}

impl AntigravityStatusFixture {
    pub const WORKSPACE_A: &'static str = "/workspace/ag-alpha";
    pub const WORKSPACE_B: &'static str = "/workspace/ag-beta";

    pub fn new() -> Self {
        Self::default()
    }

    pub fn session_a() -> AntigravitySessionKey {
        AntigravitySessionKey::new("conv-alpha", Self::WORKSPACE_A)
    }

    pub fn session_b() -> AntigravitySessionKey {
        AntigravitySessionKey::new("conv-beta", Self::WORKSPACE_B)
    }

    pub fn reset(&self) {
        *self.state.lock().unwrap() = AntigravityFixtureState::default();
    }

    pub fn set_mode(&self, mode: AntigravityFixtureMode) {
        self.state.lock().unwrap().mode = mode;
    }

    pub fn set_unknown_next_command(&self, enabled: bool) {
        self.state.lock().unwrap().unknown_next_command = enabled;
    }

    pub fn set_in_flight_steer_enabled(&self, enabled: bool) {
        self.state.lock().unwrap().in_flight_steer_enabled = enabled;
    }

    pub fn set_l3_schema(&self, stop: bool, approval: bool, model_config: bool) {
        let mut s = self.state.lock().unwrap();
        s.stop_schema_verified = stop;
        s.approval_schema_verified = approval;
        s.model_config_schema_verified = model_config;
    }

    pub fn set_csrf_auth(&self, valid: bool) {
        self.state.lock().unwrap().csrf_auth_verified = valid;
    }

    pub fn set_model_readback_override(
        &self,
        model_name: Option<String>,
        thinking_level: Option<String>,
    ) {
        let mut s = self.state.lock().unwrap();
        if model_name.is_none() && thinking_level.is_none() {
            s.model_readback_override = None;
        } else {
            s.model_readback_override = Some((model_name, thinking_level));
        }
    }

    pub fn set_pending_interaction(
        &self,
        key: &AntigravitySessionKey,
        pending: Option<AntigravityPendingInteraction>,
    ) {
        let mut s = self.state.lock().unwrap();
        let Some(prior) = s.rows.get(key).cloned() else {
            return;
        };
        let step_idx = pending.as_ref().map_or(prior.step_idx, |p| p.step_index);
        let phase = if pending.is_some() {
            AntigravitySessionPhase::RunningTool
        } else {
            prior.phase
        };
        let pending_id = pending
            .as_ref()
            .map_or_else(|| "clear".into(), |p| p.interaction_id.clone());
        let updated = AntigravitySessionRow {
            conversation_id: prior.conversation_id,
            workspace_path: prior.workspace_path,
            title: prior.title,
            project_id: prior.project_id,
            model_name: prior.model_name,
            thinking_level: prior.thinking_level,
            phase,
            last_event_type: prior.last_event_type,
            invocation_num: prior.invocation_num.max(1),
            step_idx,
            execution_num: prior.execution_num.max(1),
            last_tool_name: prior.last_tool_name,
            termination_reason: None,
            fully_idle: false,
            transition_count: prior.transition_count + 1,
            pending_interaction: pending,
            available_models: prior.available_models,
        }
        .normalized();
        s.rows.insert(key.clone(), updated.clone());
        s.cursor += 1;
        let cursor = s.cursor;
        let ev_key = format!(
            "{}:{}:pending:{pending_id}:{cursor}",
            key.conversation_id, key.workspace_path
        );
        s.seen_keys
            .entry(key.clone())
            .or_default()
            .insert(ev_key.clone());
        s.pending_changes.push(AntigravityEventChange {
            sequence: cursor,
            event_key: ev_key,
            row: updated,
        });
    }

    pub fn expire_pending_interaction(&self, key: &AntigravitySessionKey, interaction_id: &str) {
        let mut s = self.state.lock().unwrap();
        s.expired_interactions
            .entry(key.clone())
            .or_default()
            .insert(interaction_id.to_owned());
        let Some(prior) = s.rows.get(key).cloned() else {
            return;
        };
        if prior
            .pending_interaction
            .as_ref()
            .map(|p| p.interaction_id.as_str())
            != Some(interaction_id)
        {
            return;
        }
        let updated = AntigravitySessionRow {
            pending_interaction: None,
            transition_count: prior.transition_count + 1,
            ..prior
        }
        .normalized();
        s.rows.insert(key.clone(), updated.clone());
        s.cursor += 1;
        let cursor = s.cursor;
        let ev_key = format!(
            "{}:{}:expire:{interaction_id}:{cursor}",
            key.conversation_id, key.workspace_path
        );
        s.seen_keys
            .entry(key.clone())
            .or_default()
            .insert(ev_key.clone());
        s.pending_changes.push(AntigravityEventChange {
            sequence: cursor,
            event_key: ev_key,
            row: updated,
        });
    }

    pub fn delete_session(&self, key: &AntigravitySessionKey) {
        let mut s = self.state.lock().unwrap();
        s.rows.remove(key);
        if s.selected.as_ref() == Some(key) {
            s.selected = None;
        }
    }

    pub fn counts(&self) -> (usize, usize, usize, usize, usize, usize, usize, u64) {
        let s = self.state.lock().unwrap();
        (
            s.metadata_reads,
            s.duplicate_events_dropped,
            s.transcript_reads,
            s.token_reads,
            s.open_requests,
            s.open_confirmed,
            s.closes,
            s.cursor,
        )
    }

    pub fn command_counts(
        &self,
    ) -> (
        usize,
        usize,
        usize,
        usize,
        usize,
        usize,
        usize,
        usize,
        usize,
        usize,
        usize,
        usize,
    ) {
        let s = self.state.lock().unwrap();
        (
            s.create_requests,
            s.create_confirmed,
            s.send_requests,
            s.send_confirmed,
            s.query_requests,
            s.stop_requests,
            s.stop_confirmed,
            s.resolve_requests,
            s.resolve_confirmed,
            s.switch_model_requests,
            s.switch_model_confirmed,
            s.duplicate_commands_dropped,
        )
    }

    fn generation(s: &AntigravityFixtureState) -> u64 {
        if s.mode == AntigravityFixtureMode::Rotated {
            2
        } else {
            1
        }
    }

    fn active_capabilities(s: &AntigravityFixtureState) -> AntigravityCapabilities {
        if s.mode == AntigravityFixtureMode::MissingSchema {
            return AntigravityCapabilities {
                bundle_id: "com.google.antigravity".into(),
                installed_version: "2.29.0".into(),
                language_server_cl: "1013060670".into(),
                agentapi_commands: vec![
                    "get-conversation-metadata".into(),
                    "new-conversation".into(),
                    "send-message".into(),
                ],
                sidecar_sdk_version: "1.0.0".into(),
                hook_events: vec![
                    "SessionStart".into(),
                    "PreInvocation".into(),
                    "PostInvocation".into(),
                    "PreToolUse".into(),
                    "PostToolUse".into(),
                    "Stop".into(),
                ],
                supports_session_catalog: false,
                supports_target_focus: false,
                supports_stop_turn: false,
                supports_approval_gate: false,
                supports_model_config: false,
                schema_verified: false,
                stop_schema_verified: false,
                approval_schema_verified: false,
                model_config_schema_verified: false,
                csrf_auth_verified: false,
                arbitrary_rpc_exposed: false,
            };
        }
        AntigravityCapabilities {
            bundle_id: "com.google.antigravity".into(),
            installed_version: "2.29.0".into(),
            language_server_cl: "1013060670".into(),
            agentapi_commands: vec![
                "get-conversation-metadata".into(),
                "new-conversation".into(),
                "send-message".into(),
            ],
            sidecar_sdk_version: "1.0.0".into(),
            hook_events: vec![
                "SessionStart".into(),
                "PreInvocation".into(),
                "PostInvocation".into(),
                "PreToolUse".into(),
                "PostToolUse".into(),
                "Stop".into(),
            ],
            supports_session_catalog: true,
            supports_target_focus: true,
            supports_stop_turn: s.stop_schema_verified && s.csrf_auth_verified,
            supports_approval_gate: s.approval_schema_verified && s.csrf_auth_verified,
            supports_model_config: s.model_config_schema_verified && s.csrf_auth_verified,
            schema_verified: true,
            stop_schema_verified: s.stop_schema_verified,
            approval_schema_verified: s.approval_schema_verified,
            model_config_schema_verified: s.model_config_schema_verified,
            csrf_auth_verified: s.csrf_auth_verified,
            arbitrary_rpc_exposed: false,
        }
    }

    pub fn ingest(&self, event: &AntigravityHookEvent) -> Result<bool, AntigravityFailure> {
        if event.workspace_path != Self::WORKSPACE_A && event.workspace_path != Self::WORKSPACE_B {
            return Err(AntigravityFailure::Denied);
        }
        let mut s = self.state.lock().unwrap();
        let session_key = event.session_key();
        let prior = s.rows.get(&session_key).cloned();
        let event_key = event.event_key(prior.as_ref());
        if s.seen_keys
            .get(&session_key)
            .is_some_and(|set| set.contains(&event_key))
        {
            s.duplicate_events_dropped += 1;
            return Ok(false);
        }
        s.seen_keys
            .entry(session_key.clone())
            .or_default()
            .insert(event_key.clone());
        let updated = AntigravitySessionRow::reduce(prior.as_ref(), event);
        if !updated.project_id.is_empty()
            && !s.workspace_projects.contains_key(&event.workspace_path)
        {
            s.workspace_projects
                .insert(event.workspace_path.clone(), updated.project_id.clone());
        }
        s.rows.insert(session_key, updated.clone());
        s.cursor += 1;
        let cursor = s.cursor;
        s.pending_changes.push(AntigravityEventChange {
            sequence: cursor,
            event_key,
            row: updated,
        });
        Ok(true)
    }

    fn sorted_rows(s: &AntigravityFixtureState) -> Vec<AntigravitySessionRow> {
        s.rows.values().cloned().collect()
    }

    fn valid_selected(s: &AntigravityFixtureState) -> Option<AntigravitySessionKey> {
        s.selected
            .as_ref()
            .filter(|k| s.rows.contains_key(*k))
            .cloned()
    }

    fn make_command_reply(
        s: &mut AntigravityFixtureState,
        entry: &AntigravityCommandReply,
        status_override: Option<&str>,
        reason_override: Option<&str>,
        duplicate: bool,
        reconciled: bool,
    ) -> AntigravityCommandReply {
        s.pending_changes.clear();
        AntigravityCommandReply {
            instance_id: "qa-antigravity-instance".into(),
            generation: Self::generation(s),
            status: status_override.unwrap_or(&entry.status).to_owned(),
            command_op: entry.command_op.clone(),
            request_id: entry.request_id.clone(),
            conversation_id: entry.conversation_id.clone(),
            workspace_path: entry.workspace_path.clone(),
            project_id: entry.project_id.clone(),
            delivery: entry.delivery,
            in_flight_steering: entry.in_flight_steering,
            reason_code: reason_override.unwrap_or(&entry.reason_code).to_owned(),
            duplicate,
            reconciled,
            cursor: s.cursor,
            rows: Self::sorted_rows(s),
            selected: Self::valid_selected(s),
            transcript_reads: s.transcript_reads as u64,
            token_reads: s.token_reads as u64,
        }
    }

    fn make_unconfirmed_reply(
        s: &mut AntigravityFixtureState,
        command_op: &str,
        request_id: &str,
        conversation_id: &str,
        workspace_path: &str,
        project_id: &str,
        reason_code: &str,
    ) -> AntigravityCommandReply {
        s.pending_changes.clear();
        AntigravityCommandReply {
            instance_id: "qa-antigravity-instance".into(),
            generation: Self::generation(s),
            status: "unconfirmed".into(),
            command_op: command_op.into(),
            request_id: request_id.into(),
            conversation_id: conversation_id.into(),
            workspace_path: workspace_path.into(),
            project_id: project_id.into(),
            delivery: AntigravityDeliveryOutcome::None,
            in_flight_steering: false,
            reason_code: reason_code.into(),
            duplicate: false,
            reconciled: false,
            cursor: s.cursor,
            rows: Self::sorted_rows(s),
            selected: Self::valid_selected(s),
            transcript_reads: s.transcript_reads as u64,
            token_reads: s.token_reads as u64,
        }
    }
}

impl AntigravityStatusServing for AntigravityStatusFixture {
    fn connect(&self) -> Result<AntigravityStatusSnapshot, AntigravityFailure> {
        {
            let mut s = self.state.lock().unwrap();
            s.connected = true;
            s.pending_changes.clear();
        }
        self.read()
    }

    fn read(&self) -> Result<AntigravityStatusSnapshot, AntigravityFailure> {
        let mut s = self.state.lock().unwrap();
        if !s.connected || s.mode == AntigravityFixtureMode::MissingSchema {
            return Err(AntigravityFailure::Denied);
        }
        if s.mode == AntigravityFixtureMode::Stale {
            return Err(AntigravityFailure::Stale);
        }
        if s.mode == AntigravityFixtureMode::Gap {
            return Err(AntigravityFailure::Gap);
        }
        s.metadata_reads += 1;
        let rows = Self::sorted_rows(&s);
        let selected = Self::valid_selected(&s);
        let changes = std::mem::take(&mut s.pending_changes);
        Ok(AntigravityStatusSnapshot {
            instance_id: "qa-antigravity-instance".into(),
            generation: Self::generation(&s),
            allowed_workspaces: vec![Self::WORKSPACE_A.into(), Self::WORKSPACE_B.into()],
            capabilities: Self::active_capabilities(&s),
            cursor: s.cursor,
            rows,
            changes,
            selected,
            transcript_reads: s.transcript_reads as u64,
            token_reads: s.token_reads as u64,
        })
    }

    fn open(
        &self,
        target: &AntigravitySessionKey,
    ) -> Result<AntigravityOpenReply, AntigravityFailure> {
        let mut s = self.state.lock().unwrap();
        if !s.connected
            || s.mode == AntigravityFixtureMode::MissingSchema
            || !Self::active_capabilities(&s).supports_catalog_navigation()
        {
            return Err(AntigravityFailure::Denied);
        }
        if s.mode == AntigravityFixtureMode::Stale {
            return Err(AntigravityFailure::Stale);
        }
        if target.workspace_path != Self::WORKSPACE_A && target.workspace_path != Self::WORKSPACE_B
        {
            return Err(AntigravityFailure::Denied);
        }
        s.open_requests += 1;
        s.metadata_reads += 1;
        if !s.rows.contains_key(target) {
            return Ok(AntigravityOpenReply {
                instance_id: "qa-antigravity-instance".into(),
                generation: Self::generation(&s),
                status: "unconfirmed".into(),
                selected: Self::valid_selected(&s),
                transcript_reads: s.transcript_reads as u64,
                token_reads: s.token_reads as u64,
            });
        }
        if s.mode == AntigravityFixtureMode::WrongSelected {
            let wrong = if *target == Self::session_a() {
                Self::session_b()
            } else {
                Self::session_a()
            };
            if s.rows.contains_key(&wrong) {
                s.selected = Some(wrong);
            }
            return Ok(AntigravityOpenReply {
                instance_id: "qa-antigravity-instance".into(),
                generation: Self::generation(&s),
                status: "unconfirmed".into(),
                selected: s.selected.clone(),
                transcript_reads: s.transcript_reads as u64,
                token_reads: s.token_reads as u64,
            });
        }
        s.selected = Some(target.clone());
        s.open_confirmed += 1;
        Ok(AntigravityOpenReply {
            instance_id: "qa-antigravity-instance".into(),
            generation: Self::generation(&s),
            status: "ok".into(),
            selected: Some(target.clone()),
            transcript_reads: s.transcript_reads as u64,
            token_reads: s.token_reads as u64,
        })
    }

    fn create(
        &self,
        intent: &AntigravityCreateIntent,
    ) -> Result<AntigravityCommandReply, AntigravityFailure> {
        let mut s = self.state.lock().unwrap();
        if !s.connected
            || s.mode == AntigravityFixtureMode::MissingSchema
            || !Self::active_capabilities(&s).supports_create_conversation()
        {
            return Err(AntigravityFailure::Denied);
        }
        if s.mode == AntigravityFixtureMode::Stale {
            return Err(AntigravityFailure::Stale);
        }
        if !intent.is_valid()
            || (intent.workspace_path != Self::WORKSPACE_A
                && intent.workspace_path != Self::WORKSPACE_B)
        {
            return Err(AntigravityFailure::Denied);
        }
        s.create_requests += 1;
        s.metadata_reads += 1;
        if let Some(existing) = s.command_ledger.get(&intent.request_id).cloned() {
            if existing.command_op != "create"
                || existing.workspace_path != intent.workspace_path
                || existing.project_id != intent.project_id
            {
                return Ok(Self::make_unconfirmed_reply(
                    &mut s,
                    "create",
                    &intent.request_id,
                    "",
                    &intent.workspace_path,
                    &intent.project_id,
                    "guard.requestIdConflict",
                ));
            }
            s.duplicate_commands_dropped += 1;
            return Ok(Self::make_command_reply(
                &mut s, &existing, None, None, true, false,
            ));
        }
        if let Some(bound) = s.workspace_projects.get(&intent.workspace_path) {
            if bound != &intent.project_id {
                return Ok(Self::make_unconfirmed_reply(
                    &mut s,
                    "create",
                    &intent.request_id,
                    "",
                    &intent.workspace_path,
                    &intent.project_id,
                    "guard.projectIdMismatch",
                ));
            }
        }
        s.workspace_projects
            .insert(intent.workspace_path.clone(), intent.project_id.clone());
        s.created_seq += 1;
        let seq = s.created_seq;
        let conv_id = intent
            .conversation_id
            .as_deref()
            .filter(|id| {
                !id.is_empty()
                    && !s
                        .rows
                        .contains_key(&AntigravitySessionKey::new(*id, &intent.workspace_path))
            })
            .map(String::from)
            .unwrap_or_else(|| format!("conv-created-{seq}"));
        let target_key = AntigravitySessionKey::new(&conv_id, &intent.workspace_path);
        let new_row = AntigravitySessionRow {
            conversation_id: conv_id.clone(),
            workspace_path: intent.workspace_path.clone(),
            title: intent
                .title
                .clone()
                .unwrap_or_else(|| format!("Conversation {seq}")),
            project_id: intent.project_id.clone(),
            model_name: intent.model_name.clone().unwrap_or_else(|| "flash".into()),
            thinking_level: "high".into(),
            phase: AntigravitySessionPhase::Running,
            last_event_type: AntigravityHookEventType::PreInvocation,
            invocation_num: 1,
            step_idx: 0,
            execution_num: 1,
            last_tool_name: None,
            termination_reason: None,
            fully_idle: false,
            transition_count: 1,
            pending_interaction: None,
            available_models: AntigravityModelOption::default_catalog(),
        }
        .normalized();
        s.rows.insert(target_key.clone(), new_row);
        s.cursor += 1;
        let ev_key = format!(
            "{conv_id}:{}:create:{}",
            intent.workspace_path, intent.request_id
        );
        s.seen_keys
            .entry(target_key.clone())
            .or_default()
            .insert(ev_key);
        s.pending_changes.clear();

        let (status, reason_code) = if s.mode == AntigravityFixtureMode::WrongSelected {
            let wrong = if target_key == Self::session_a() {
                Self::session_b()
            } else {
                Self::session_a()
            };
            if s.rows.contains_key(&wrong) {
                s.selected = Some(wrong);
            }
            ("unconfirmed", "guard.selectionReadbackMismatch")
        } else {
            s.selected = Some(target_key);
            s.create_confirmed += 1;
            ("created", "agentapi.newConversationConfirmed")
        };

        let settled = AntigravityCommandReply {
            instance_id: "qa-antigravity-instance".into(),
            generation: Self::generation(&s),
            status: status.into(),
            command_op: "create".into(),
            request_id: intent.request_id.clone(),
            conversation_id: conv_id,
            workspace_path: intent.workspace_path.clone(),
            project_id: intent.project_id.clone(),
            delivery: if status == "created" {
                AntigravityDeliveryOutcome::Started
            } else {
                AntigravityDeliveryOutcome::None
            },
            in_flight_steering: false,
            reason_code: reason_code.into(),
            duplicate: false,
            reconciled: false,
            cursor: s.cursor,
            rows: Self::sorted_rows(&s),
            selected: Self::valid_selected(&s),
            transcript_reads: s.transcript_reads as u64,
            token_reads: s.token_reads as u64,
        };
        s.command_ledger
            .insert(intent.request_id.clone(), settled.clone());
        if s.unknown_next_command {
            s.unknown_next_command = false;
            return Ok(Self::make_command_reply(
                &mut s,
                &settled,
                Some("unknown"),
                Some("bridge.deliveryUnconfirmed"),
                false,
                false,
            ));
        }
        Ok(settled)
    }

    fn send(
        &self,
        intent: &AntigravitySendIntent,
    ) -> Result<AntigravityCommandReply, AntigravityFailure> {
        let mut s = self.state.lock().unwrap();
        if !s.connected
            || s.mode == AntigravityFixtureMode::MissingSchema
            || !Self::active_capabilities(&s).supports_send_message()
        {
            return Err(AntigravityFailure::Denied);
        }
        if s.mode == AntigravityFixtureMode::Stale {
            return Err(AntigravityFailure::Stale);
        }
        if !intent.is_valid()
            || (intent.target.workspace_path != Self::WORKSPACE_A
                && intent.target.workspace_path != Self::WORKSPACE_B)
        {
            return Err(AntigravityFailure::Denied);
        }
        s.send_requests += 1;
        s.metadata_reads += 1;
        if let Some(existing) = s.command_ledger.get(&intent.request_id).cloned() {
            if existing.command_op != "send"
                || existing.conversation_id != intent.target.conversation_id
                || existing.workspace_path != intent.target.workspace_path
            {
                return Ok(Self::make_unconfirmed_reply(
                    &mut s,
                    "send",
                    &intent.request_id,
                    &intent.target.conversation_id,
                    &intent.target.workspace_path,
                    intent.project_id.as_deref().unwrap_or(""),
                    "guard.requestIdConflict",
                ));
            }
            s.duplicate_commands_dropped += 1;
            return Ok(Self::make_command_reply(
                &mut s, &existing, None, None, true, false,
            ));
        }
        let Some(prior) = s.rows.get(&intent.target).cloned() else {
            return Ok(Self::make_unconfirmed_reply(
                &mut s,
                "send",
                &intent.request_id,
                &intent.target.conversation_id,
                &intent.target.workspace_path,
                intent.project_id.as_deref().unwrap_or(""),
                "guard.targetNotFound",
            ));
        };
        if s.selected.as_ref() != Some(&intent.target) {
            return Ok(Self::make_unconfirmed_reply(
                &mut s,
                "send",
                &intent.request_id,
                &intent.target.conversation_id,
                &intent.target.workspace_path,
                &prior.project_id,
                "guard.selectionMismatch",
            ));
        }
        if let Some(req_proj) = &intent.project_id {
            if !req_proj.is_empty() && !prior.project_id.is_empty() && req_proj != &prior.project_id
            {
                return Ok(Self::make_unconfirmed_reply(
                    &mut s,
                    "send",
                    &intent.request_id,
                    &intent.target.conversation_id,
                    &intent.target.workspace_path,
                    &prior.project_id,
                    "guard.projectIdMismatch",
                ));
            }
        }
        let resolved_project = if !prior.project_id.is_empty() {
            prior.project_id.clone()
        } else {
            intent.project_id.clone().unwrap_or_default()
        };
        let in_flight = matches!(
            prior.phase,
            AntigravitySessionPhase::Running
                | AntigravitySessionPhase::RunningTool
                | AntigravitySessionPhase::BackgroundActive
        );
        let (updated_row, status, delivery, in_flight_steering, reason_code) = if !in_flight {
            (
                AntigravitySessionRow {
                    project_id: resolved_project.clone(),
                    phase: AntigravitySessionPhase::Running,
                    last_event_type: AntigravityHookEventType::PreInvocation,
                    invocation_num: prior.invocation_num + 1,
                    execution_num: prior.execution_num + 1,
                    termination_reason: None,
                    fully_idle: false,
                    transition_count: prior.transition_count + 1,
                    ..prior
                }
                .normalized(),
                "started",
                AntigravityDeliveryOutcome::Started,
                false,
                "agentapi.sendMessageStarted",
            )
        } else if s.in_flight_steer_enabled
            && intent.delivery_mode == AntigravityDeliveryMode::Steer
        {
            (
                AntigravitySessionRow {
                    project_id: resolved_project.clone(),
                    phase: if prior.phase == AntigravitySessionPhase::BackgroundActive {
                        AntigravitySessionPhase::Running
                    } else {
                        prior.phase
                    },
                    step_idx: prior.step_idx + 1,
                    fully_idle: false,
                    transition_count: prior.transition_count + 1,
                    ..prior
                }
                .normalized(),
                "steered",
                AntigravityDeliveryOutcome::Steered,
                true,
                "agentapi.sendMessageSteered",
            )
        } else {
            (
                AntigravitySessionRow {
                    project_id: resolved_project.clone(),
                    fully_idle: false,
                    transition_count: prior.transition_count + 1,
                    ..prior
                }
                .normalized(),
                "queued",
                AntigravityDeliveryOutcome::NextInvocation,
                false,
                if intent.delivery_mode == AntigravityDeliveryMode::Steer {
                    "agentapi.steerUnsupportedQueuedNextInvocation"
                } else {
                    "agentapi.sendMessageQueuedNextInvocation"
                },
            )
        };
        s.rows.insert(intent.target.clone(), updated_row);
        s.cursor += 1;
        let ev_key = format!(
            "{}:{}:send:{}",
            intent.target.conversation_id, intent.target.workspace_path, intent.request_id
        );
        s.seen_keys
            .entry(intent.target.clone())
            .or_default()
            .insert(ev_key);
        s.pending_changes.clear();
        s.send_confirmed += 1;

        let settled = AntigravityCommandReply {
            instance_id: "qa-antigravity-instance".into(),
            generation: Self::generation(&s),
            status: status.into(),
            command_op: "send".into(),
            request_id: intent.request_id.clone(),
            conversation_id: intent.target.conversation_id.clone(),
            workspace_path: intent.target.workspace_path.clone(),
            project_id: resolved_project,
            delivery,
            in_flight_steering,
            reason_code: reason_code.into(),
            duplicate: false,
            reconciled: false,
            cursor: s.cursor,
            rows: Self::sorted_rows(&s),
            selected: Self::valid_selected(&s),
            transcript_reads: s.transcript_reads as u64,
            token_reads: s.token_reads as u64,
        };
        s.command_ledger
            .insert(intent.request_id.clone(), settled.clone());
        if s.unknown_next_command {
            s.unknown_next_command = false;
            return Ok(Self::make_command_reply(
                &mut s,
                &settled,
                Some("unknown"),
                Some("bridge.deliveryUnconfirmed"),
                false,
                false,
            ));
        }
        Ok(settled)
    }

    fn query(
        &self,
        intent: &AntigravityQueryIntent,
    ) -> Result<AntigravityCommandReply, AntigravityFailure> {
        let mut s = self.state.lock().unwrap();
        if !s.connected
            || s.mode == AntigravityFixtureMode::MissingSchema
            || !Self::active_capabilities(&s).schema_verified
        {
            return Err(AntigravityFailure::Denied);
        }
        if s.mode == AntigravityFixtureMode::Stale {
            return Err(AntigravityFailure::Stale);
        }
        if !intent.is_valid() {
            return Err(AntigravityFailure::Denied);
        }
        s.query_requests += 1;
        s.metadata_reads += 1;
        let Some(existing) = s.command_ledger.get(&intent.request_id).cloned() else {
            return Ok(Self::make_unconfirmed_reply(
                &mut s,
                intent.expected_op.as_deref().unwrap_or("query"),
                &intent.request_id,
                intent.conversation_id.as_deref().unwrap_or(""),
                intent.workspace_path.as_deref().unwrap_or(""),
                intent.project_id.as_deref().unwrap_or(""),
                "query.requestNotFound",
            ));
        };
        Ok(Self::make_command_reply(
            &mut s, &existing, None, None, true, true,
        ))
    }

    fn stop(
        &self,
        intent: &AntigravityStopIntent,
    ) -> Result<AntigravityCommandReply, AntigravityFailure> {
        let mut s = self.state.lock().unwrap();
        if !s.connected
            || s.mode == AntigravityFixtureMode::MissingSchema
            || !Self::active_capabilities(&s).supports_stop_conversation()
        {
            return Err(AntigravityFailure::Denied);
        }
        if s.mode == AntigravityFixtureMode::Stale {
            return Err(AntigravityFailure::Stale);
        }
        if !intent.is_valid()
            || (intent.target.workspace_path != Self::WORKSPACE_A
                && intent.target.workspace_path != Self::WORKSPACE_B)
        {
            return Err(AntigravityFailure::Denied);
        }
        s.stop_requests += 1;
        s.metadata_reads += 1;
        if let Some(existing) = s.command_ledger.get(&intent.request_id).cloned() {
            if existing.command_op != "stop"
                || existing.conversation_id != intent.target.conversation_id
                || existing.workspace_path != intent.target.workspace_path
            {
                return Ok(Self::make_unconfirmed_reply(
                    &mut s,
                    "stop",
                    &intent.request_id,
                    &intent.target.conversation_id,
                    &intent.target.workspace_path,
                    "",
                    "guard.requestIdConflict",
                ));
            }
            s.duplicate_commands_dropped += 1;
            return Ok(Self::make_command_reply(
                &mut s, &existing, None, None, true, false,
            ));
        }
        let Some(prior) = s.rows.get(&intent.target).cloned() else {
            return Ok(Self::make_unconfirmed_reply(
                &mut s,
                "stop",
                &intent.request_id,
                &intent.target.conversation_id,
                &intent.target.workspace_path,
                "",
                "guard.targetNotFound",
            ));
        };
        if s.selected.as_ref() != Some(&intent.target) {
            return Ok(Self::make_unconfirmed_reply(
                &mut s,
                "stop",
                &intent.request_id,
                &intent.target.conversation_id,
                &intent.target.workspace_path,
                &prior.project_id,
                "guard.selectionMismatch",
            ));
        }
        let in_flight = matches!(
            prior.phase,
            AntigravitySessionPhase::Running
                | AntigravitySessionPhase::RunningTool
                | AntigravitySessionPhase::BackgroundActive
        );
        if !in_flight {
            let noop = AntigravityCommandReply {
                instance_id: "qa-antigravity-instance".into(),
                generation: Self::generation(&s),
                status: "noop".into(),
                command_op: "stop".into(),
                request_id: intent.request_id.clone(),
                conversation_id: intent.target.conversation_id.clone(),
                workspace_path: intent.target.workspace_path.clone(),
                project_id: prior.project_id,
                delivery: AntigravityDeliveryOutcome::None,
                in_flight_steering: false,
                reason_code: "guard.alreadyStopped".into(),
                duplicate: false,
                reconciled: false,
                cursor: s.cursor,
                rows: Self::sorted_rows(&s),
                selected: Self::valid_selected(&s),
                transcript_reads: s.transcript_reads as u64,
                token_reads: s.token_reads as u64,
            };
            s.command_ledger
                .insert(intent.request_id.clone(), noop.clone());
            s.pending_changes.clear();
            return Ok(noop);
        }
        if (intent.expected_invocation_num > 0
            && intent.expected_invocation_num != prior.invocation_num)
            || intent
                .expected_execution_num
                .is_some_and(|exec| exec != prior.execution_num)
        {
            let noop = AntigravityCommandReply {
                instance_id: "qa-antigravity-instance".into(),
                generation: Self::generation(&s),
                status: "noop".into(),
                command_op: "stop".into(),
                request_id: intent.request_id.clone(),
                conversation_id: intent.target.conversation_id.clone(),
                workspace_path: intent.target.workspace_path.clone(),
                project_id: prior.project_id,
                delivery: AntigravityDeliveryOutcome::None,
                in_flight_steering: false,
                reason_code: "guard.stopTargetChanged".into(),
                duplicate: false,
                reconciled: false,
                cursor: s.cursor,
                rows: Self::sorted_rows(&s),
                selected: Self::valid_selected(&s),
                transcript_reads: s.transcript_reads as u64,
                token_reads: s.token_reads as u64,
            };
            s.command_ledger
                .insert(intent.request_id.clone(), noop.clone());
            s.pending_changes.clear();
            return Ok(noop);
        }
        let resolved_project = prior.project_id.clone();
        let updated_row = AntigravitySessionRow {
            phase: AntigravitySessionPhase::Canceled,
            last_event_type: AntigravityHookEventType::Stop,
            termination_reason: Some("USER_CANCELED".into()),
            fully_idle: intent.kill_background_tasks,
            transition_count: prior.transition_count + 1,
            pending_interaction: None,
            ..prior
        }
        .normalized();
        s.rows.insert(intent.target.clone(), updated_row);
        s.cursor += 1;
        let ev_key = format!(
            "{}:{}:stop:{}",
            intent.target.conversation_id, intent.target.workspace_path, intent.request_id
        );
        s.seen_keys
            .entry(intent.target.clone())
            .or_default()
            .insert(ev_key);
        s.pending_changes.clear();
        s.stop_confirmed += 1;

        let settled = AntigravityCommandReply {
            instance_id: "qa-antigravity-instance".into(),
            generation: Self::generation(&s),
            status: "stopped".into(),
            command_op: "stop".into(),
            request_id: intent.request_id.clone(),
            conversation_id: intent.target.conversation_id.clone(),
            workspace_path: intent.target.workspace_path.clone(),
            project_id: resolved_project,
            delivery: AntigravityDeliveryOutcome::Canceled,
            in_flight_steering: false,
            reason_code: "languageServer.cancelCascadeInvocationConfirmed".into(),
            duplicate: false,
            reconciled: false,
            cursor: s.cursor,
            rows: Self::sorted_rows(&s),
            selected: Self::valid_selected(&s),
            transcript_reads: s.transcript_reads as u64,
            token_reads: s.token_reads as u64,
        };
        s.command_ledger
            .insert(intent.request_id.clone(), settled.clone());
        if s.unknown_next_command {
            s.unknown_next_command = false;
            return Ok(Self::make_command_reply(
                &mut s,
                &settled,
                Some("unknown"),
                Some("bridge.deliveryUnconfirmed"),
                false,
                false,
            ));
        }
        Ok(settled)
    }

    fn resolve(
        &self,
        intent: &AntigravityResolveIntent,
    ) -> Result<AntigravityCommandReply, AntigravityFailure> {
        let mut s = self.state.lock().unwrap();
        if !s.connected
            || s.mode == AntigravityFixtureMode::MissingSchema
            || !Self::active_capabilities(&s).supports_resolve_interaction()
        {
            return Err(AntigravityFailure::Denied);
        }
        if s.mode == AntigravityFixtureMode::Stale {
            return Err(AntigravityFailure::Stale);
        }
        if !intent.is_valid()
            || (intent.target.workspace_path != Self::WORKSPACE_A
                && intent.target.workspace_path != Self::WORKSPACE_B)
        {
            return Err(AntigravityFailure::Denied);
        }
        s.resolve_requests += 1;
        s.metadata_reads += 1;
        if let Some(existing) = s.command_ledger.get(&intent.request_id).cloned() {
            if existing.command_op != "resolve"
                || existing.conversation_id != intent.target.conversation_id
                || existing.workspace_path != intent.target.workspace_path
            {
                return Ok(Self::make_unconfirmed_reply(
                    &mut s,
                    "resolve",
                    &intent.request_id,
                    &intent.target.conversation_id,
                    &intent.target.workspace_path,
                    "",
                    "guard.requestIdConflict",
                ));
            }
            s.duplicate_commands_dropped += 1;
            return Ok(Self::make_command_reply(
                &mut s, &existing, None, None, true, false,
            ));
        }
        let Some(prior) = s.rows.get(&intent.target).cloned() else {
            return Ok(Self::make_unconfirmed_reply(
                &mut s,
                "resolve",
                &intent.request_id,
                &intent.target.conversation_id,
                &intent.target.workspace_path,
                "",
                "guard.targetNotFound",
            ));
        };
        if s.selected.as_ref() != Some(&intent.target) {
            return Ok(Self::make_unconfirmed_reply(
                &mut s,
                "resolve",
                &intent.request_id,
                &intent.target.conversation_id,
                &intent.target.workspace_path,
                &prior.project_id,
                "guard.selectionMismatch",
            ));
        }
        let resolved_project = prior.project_id.clone();
        if s.expired_interactions
            .get(&intent.target)
            .is_some_and(|set| set.contains(&intent.interaction_id))
        {
            let noop = AntigravityCommandReply {
                instance_id: "qa-antigravity-instance".into(),
                generation: Self::generation(&s),
                status: "noop".into(),
                command_op: "resolve".into(),
                request_id: intent.request_id.clone(),
                conversation_id: intent.target.conversation_id.clone(),
                workspace_path: intent.target.workspace_path.clone(),
                project_id: resolved_project,
                delivery: AntigravityDeliveryOutcome::None,
                in_flight_steering: false,
                reason_code: "guard.interactionAlreadyResolved".into(),
                duplicate: false,
                reconciled: false,
                cursor: s.cursor,
                rows: Self::sorted_rows(&s),
                selected: Self::valid_selected(&s),
                transcript_reads: s.transcript_reads as u64,
                token_reads: s.token_reads as u64,
            };
            s.command_ledger
                .insert(intent.request_id.clone(), noop.clone());
            s.pending_changes.clear();
            return Ok(noop);
        }
        let Some(pending) = prior.pending_interaction.clone() else {
            return Ok(Self::make_unconfirmed_reply(
                &mut s,
                "resolve",
                &intent.request_id,
                &intent.target.conversation_id,
                &intent.target.workspace_path,
                &resolved_project,
                "guard.noPendingInteraction",
            ));
        };
        if pending.interaction_id != intent.interaction_id {
            return Ok(Self::make_unconfirmed_reply(
                &mut s,
                "resolve",
                &intent.request_id,
                &intent.target.conversation_id,
                &intent.target.workspace_path,
                &resolved_project,
                "guard.interactionIdMismatch",
            ));
        }
        if pending.interaction_type != intent.interaction_type {
            return Ok(Self::make_unconfirmed_reply(
                &mut s,
                "resolve",
                &intent.request_id,
                &intent.target.conversation_id,
                &intent.target.workspace_path,
                &resolved_project,
                "guard.interactionTypeMismatch",
            ));
        }
        if intent.interaction_type == AntigravityInteractionType::Question {
            if matches!(
                intent.decision,
                AntigravityInteractionDecision::Allow | AntigravityInteractionDecision::Deny
            ) {
                return Ok(Self::make_unconfirmed_reply(
                    &mut s,
                    "resolve",
                    &intent.request_id,
                    &intent.target.conversation_id,
                    &intent.target.workspace_path,
                    &resolved_project,
                    "guard.questionCannotApproveAsPermission",
                ));
            }
            if intent.decision == AntigravityInteractionDecision::Answer
                && intent.selected_option_ids.is_empty()
            {
                return Ok(Self::make_unconfirmed_reply(
                    &mut s,
                    "resolve",
                    &intent.request_id,
                    &intent.target.conversation_id,
                    &intent.target.workspace_path,
                    &resolved_project,
                    "guard.invalidQuestionOption",
                ));
            }
        } else if intent.decision == AntigravityInteractionDecision::Answer {
            return Ok(Self::make_unconfirmed_reply(
                &mut s,
                "resolve",
                &intent.request_id,
                &intent.target.conversation_id,
                &intent.target.workspace_path,
                &resolved_project,
                "guard.invalidInteractionDecision",
            ));
        }

        s.expired_interactions
            .entry(intent.target.clone())
            .or_default()
            .insert(intent.interaction_id.clone());
        let denied_or_canceled = matches!(
            intent.decision,
            AntigravityInteractionDecision::Deny | AntigravityInteractionDecision::Cancel
        );
        let updated_row = AntigravitySessionRow {
            phase: AntigravitySessionPhase::Running,
            last_event_type: if denied_or_canceled {
                AntigravityHookEventType::PreInvocation
            } else {
                AntigravityHookEventType::PostToolUse
            },
            invocation_num: prior.invocation_num.max(1),
            step_idx: prior.step_idx.max(pending.step_index),
            execution_num: prior.execution_num.max(1),
            termination_reason: None,
            fully_idle: false,
            transition_count: prior.transition_count + 1,
            pending_interaction: None,
            ..prior
        }
        .normalized();
        s.rows.insert(intent.target.clone(), updated_row);
        s.cursor += 1;
        let ev_key = format!(
            "{}:{}:resolve:{}",
            intent.target.conversation_id, intent.target.workspace_path, intent.request_id
        );
        s.seen_keys
            .entry(intent.target.clone())
            .or_default()
            .insert(ev_key);
        s.pending_changes.clear();
        s.resolve_confirmed += 1;

        let delivery = match intent.decision {
            AntigravityInteractionDecision::Allow => AntigravityDeliveryOutcome::Allow,
            AntigravityInteractionDecision::Deny => AntigravityDeliveryOutcome::Deny,
            AntigravityInteractionDecision::Answer => AntigravityDeliveryOutcome::Answer,
            AntigravityInteractionDecision::Cancel => AntigravityDeliveryOutcome::Cancel,
        };
        let settled = AntigravityCommandReply {
            instance_id: "qa-antigravity-instance".into(),
            generation: Self::generation(&s),
            status: "resolved".into(),
            command_op: "resolve".into(),
            request_id: intent.request_id.clone(),
            conversation_id: intent.target.conversation_id.clone(),
            workspace_path: intent.target.workspace_path.clone(),
            project_id: resolved_project,
            delivery,
            in_flight_steering: false,
            reason_code: "languageServer.handleCascadeUserInteractionConfirmed".into(),
            duplicate: false,
            reconciled: false,
            cursor: s.cursor,
            rows: Self::sorted_rows(&s),
            selected: Self::valid_selected(&s),
            transcript_reads: s.transcript_reads as u64,
            token_reads: s.token_reads as u64,
        };
        s.command_ledger
            .insert(intent.request_id.clone(), settled.clone());
        if s.unknown_next_command {
            s.unknown_next_command = false;
            return Ok(Self::make_command_reply(
                &mut s,
                &settled,
                Some("unknown"),
                Some("bridge.deliveryUnconfirmed"),
                false,
                false,
            ));
        }
        Ok(settled)
    }

    fn switch_model(
        &self,
        intent: &AntigravitySwitchModelIntent,
    ) -> Result<AntigravityCommandReply, AntigravityFailure> {
        let mut s = self.state.lock().unwrap();
        if !s.connected
            || s.mode == AntigravityFixtureMode::MissingSchema
            || !Self::active_capabilities(&s).supports_switch_model_config()
        {
            return Err(AntigravityFailure::Denied);
        }
        if s.mode == AntigravityFixtureMode::Stale {
            return Err(AntigravityFailure::Stale);
        }
        if !intent.is_valid()
            || (intent.target.workspace_path != Self::WORKSPACE_A
                && intent.target.workspace_path != Self::WORKSPACE_B)
        {
            return Err(AntigravityFailure::Denied);
        }
        s.switch_model_requests += 1;
        s.metadata_reads += 1;
        if let Some(existing) = s.command_ledger.get(&intent.request_id).cloned() {
            if existing.command_op != "switch-model"
                || existing.conversation_id != intent.target.conversation_id
                || existing.workspace_path != intent.target.workspace_path
            {
                return Ok(Self::make_unconfirmed_reply(
                    &mut s,
                    "switch-model",
                    &intent.request_id,
                    &intent.target.conversation_id,
                    &intent.target.workspace_path,
                    "",
                    "guard.requestIdConflict",
                ));
            }
            s.duplicate_commands_dropped += 1;
            return Ok(Self::make_command_reply(
                &mut s, &existing, None, None, true, false,
            ));
        }
        let Some(prior) = s.rows.get(&intent.target).cloned() else {
            return Ok(Self::make_unconfirmed_reply(
                &mut s,
                "switch-model",
                &intent.request_id,
                &intent.target.conversation_id,
                &intent.target.workspace_path,
                "",
                "guard.targetNotFound",
            ));
        };
        if s.selected.as_ref() != Some(&intent.target) {
            return Ok(Self::make_unconfirmed_reply(
                &mut s,
                "switch-model",
                &intent.request_id,
                &intent.target.conversation_id,
                &intent.target.workspace_path,
                &prior.project_id,
                "guard.selectionMismatch",
            ));
        }
        let resolved_project = prior.project_id.clone();
        let Some(matched_model) = prior
            .available_models
            .iter()
            .find(|m| m.model_name == intent.model_name)
        else {
            return Ok(Self::make_unconfirmed_reply(
                &mut s,
                "switch-model",
                &intent.request_id,
                &intent.target.conversation_id,
                &intent.target.workspace_path,
                &resolved_project,
                "guard.unsupportedModel",
            ));
        };
        if !matched_model
            .supported_thinking_levels
            .contains(&intent.thinking_level)
        {
            return Ok(Self::make_unconfirmed_reply(
                &mut s,
                "switch-model",
                &intent.request_id,
                &intent.target.conversation_id,
                &intent.target.workspace_path,
                &resolved_project,
                "guard.unsupportedThinkingLevel",
            ));
        }
        let actual_model = s
            .model_readback_override
            .as_ref()
            .and_then(|(m, _)| m.clone())
            .unwrap_or_else(|| intent.model_name.clone());
        let actual_thinking = s
            .model_readback_override
            .as_ref()
            .and_then(|(_, t)| t.clone())
            .unwrap_or_else(|| intent.thinking_level.clone());
        let updated_row = AntigravitySessionRow {
            model_name: actual_model.clone(),
            thinking_level: actual_thinking.clone(),
            transition_count: prior.transition_count + 1,
            ..prior
        }
        .normalized();
        s.rows.insert(intent.target.clone(), updated_row);
        s.cursor += 1;
        let ev_key = format!(
            "{}:{}:switch-model:{}",
            intent.target.conversation_id, intent.target.workspace_path, intent.request_id
        );
        s.seen_keys
            .entry(intent.target.clone())
            .or_default()
            .insert(ev_key);
        s.pending_changes.clear();
        if actual_model != intent.model_name || actual_thinking != intent.thinking_level {
            return Ok(Self::make_unconfirmed_reply(
                &mut s,
                "switch-model",
                &intent.request_id,
                &intent.target.conversation_id,
                &intent.target.workspace_path,
                &resolved_project,
                "guard.modelReadbackMismatch",
            ));
        }
        s.switch_model_confirmed += 1;
        let settled = AntigravityCommandReply {
            instance_id: "qa-antigravity-instance".into(),
            generation: Self::generation(&s),
            status: "modelSwitched".into(),
            command_op: "switch-model".into(),
            request_id: intent.request_id.clone(),
            conversation_id: intent.target.conversation_id.clone(),
            workspace_path: intent.target.workspace_path.clone(),
            project_id: resolved_project,
            delivery: AntigravityDeliveryOutcome::ModelSwitched,
            in_flight_steering: false,
            reason_code: "languageServer.cascadeModelConfigConfirmed".into(),
            duplicate: false,
            reconciled: false,
            cursor: s.cursor,
            rows: Self::sorted_rows(&s),
            selected: Self::valid_selected(&s),
            transcript_reads: s.transcript_reads as u64,
            token_reads: s.token_reads as u64,
        };
        s.command_ledger
            .insert(intent.request_id.clone(), settled.clone());
        if s.unknown_next_command {
            s.unknown_next_command = false;
            return Ok(Self::make_command_reply(
                &mut s,
                &settled,
                Some("unknown"),
                Some("bridge.deliveryUnconfirmed"),
                false,
                false,
            ));
        }
        Ok(settled)
    }

    fn close(&self) {
        let mut s = self.state.lock().unwrap();
        if s.connected {
            s.closes += 1;
        }
        s.connected = false;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AntigravityAuditReceipt {
    pub timestamp_ms: u64,
    pub profile_id: String,
    pub binding_id: String,
    pub action_type: String,
    pub target: String,
    pub outcome: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_byte_len: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AntigravityStatusScenarioReport {
    pub status: String,
    pub installed_version: String,
    pub language_server_cl: String,
    pub transcript_reads: usize,
    pub token_reads: usize,
    pub open_requests: usize,
    pub open_confirmed: usize,
    pub create_requests: usize,
    pub create_confirmed: usize,
    pub send_requests: usize,
    pub send_confirmed: usize,
    pub query_requests: usize,
    pub stop_requests: usize,
    pub stop_confirmed: usize,
    pub resolve_requests: usize,
    pub resolve_confirmed: usize,
    pub switch_model_requests: usize,
    pub switch_model_confirmed: usize,
    pub duplicate_commands_dropped: usize,
    pub checks: BTreeMap<String, bool>,
    pub native_app: String,
    pub physical_key: String,
    #[serde(skip)]
    pub receipts: Vec<AntigravityAuditReceipt>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn run_antigravity_status_scenario(
    audit_log_path: Option<&Path>,
) -> Result<AntigravityStatusScenarioReport, String> {
    let fixture = Arc::new(AntigravityStatusFixture::new());
    fixture.reset();
    let mut ctrl = AntigravityStatusController::new(fixture.clone());
    let mut checks = BTreeMap::new();
    let mut receipts = Vec::new();
    let profile_id = "profile-antigravity";

    let push_open = |ctrl: &mut AntigravityStatusController,
                     sel: Option<&AntigravitySelection>,
                     receipts: &mut Vec<AntigravityAuditReceipt>|
     -> AntigravityOpenResult {
        let res = ctrl.open(sel, || true);
        receipts.push(AntigravityAuditReceipt {
            timestamp_ms: now_ms(),
            profile_id: profile_id.into(),
            binding_id: "menu-antigravity-session".into(),
            action_type: "antigravity_open".into(),
            target: "antigravity".into(),
            outcome: res.as_str().into(),
            detail: res.label().into(),
            text_byte_len: None,
        });
        res
    };

    let push_cmd = |ctrl: &mut AntigravityStatusController,
                    action_type: &str,
                    binding_id: &str,
                    cmd: Option<&AntigravityQueuedCommand>,
                    receipts: &mut Vec<AntigravityAuditReceipt>|
     -> AntigravityCommandResult {
        let res = ctrl.execute(cmd, || true);
        receipts.push(AntigravityAuditReceipt {
            timestamp_ms: now_ms(),
            profile_id: profile_id.into(),
            binding_id: binding_id.into(),
            action_type: action_type.into(),
            target: "antigravity".into(),
            outcome: res.as_str().into(),
            detail: res.label().into(),
            text_byte_len: None,
        });
        res
    };

    let before_counts = fixture.counts();
    ctrl.refresh();
    let disabled_counts = fixture.counts();
    checks.insert(
        "disabled_zero_reads".into(),
        before_counts.0 == disabled_counts.0,
    );

    let session_a = AntigravityStatusFixture::session_a();
    let session_b = AntigravityStatusFixture::session_b();
    fixture
        .ingest(&AntigravityHookEvent {
            conversation_id: session_a.conversation_id.clone(),
            workspace_path: session_a.workspace_path.clone(),
            event_type: AntigravityHookEventType::SessionStart,
            model_name: "gemini-2.5-pro".into(),
            invocation_num: Some(0),
            step_idx: Some(0),
            execution_num: Some(0),
            tool_name: None,
            termination_reason: None,
            has_error: false,
            fully_idle: Some(true),
            event_id: None,
            title: Some("Shared Investigation".into()),
            project_id: Some("proj-alpha".into()),
        })
        .map_err(|e| e.to_string())?;
    fixture
        .ingest(&AntigravityHookEvent {
            conversation_id: session_b.conversation_id.clone(),
            workspace_path: session_b.workspace_path.clone(),
            event_type: AntigravityHookEventType::SessionStart,
            model_name: "gemini-2.5-flash".into(),
            invocation_num: Some(0),
            step_idx: Some(0),
            execution_num: Some(0),
            tool_name: None,
            termination_reason: None,
            has_error: false,
            fully_idle: Some(true),
            event_id: None,
            title: Some("Shared Investigation".into()),
            project_id: Some("proj-beta".into()),
        })
        .map_err(|e| e.to_string())?;

    ctrl.set_enabled(true);
    let expected_caps = AntigravityCapabilities::installed_2_29_0();
    checks.insert(
        "version_and_capabilities_recorded".into(),
        ctrl.capabilities.as_ref() == Some(&expected_caps)
            && ctrl.capabilities.as_ref().is_some_and(|c| {
                c.installed_version == "2.29.0" && c.language_server_cl == "1013060670"
            }),
    );

    let sel_a1 = ctrl.selection(&session_a);
    let res_a1 = push_open(&mut ctrl, sel_a1.as_ref(), &mut receipts);
    let confirmed_a =
        res_a1 == AntigravityOpenResult::Confirmed && ctrl.selected.as_ref() == Some(&session_a);

    let sel_b1 = ctrl.selection(&session_b);
    let res_b1 = push_open(&mut ctrl, sel_b1.as_ref(), &mut receipts);
    let confirmed_b =
        res_b1 == AntigravityOpenResult::Confirmed && ctrl.selected.as_ref() == Some(&session_b);

    checks.insert(
        "existing_two_sessions_catalog_and_selected_readback_confirmed".into(),
        confirmed_a && confirmed_b,
    );
    checks.insert(
        "same_name_cross_workspace_no_collision".into(),
        ctrl.rows.len() == 2
            && ctrl.row(&session_a).map(|r| r.title.as_str()) == Some("Shared Investigation")
            && ctrl.row(&session_b).map(|r| r.title.as_str()) == Some("Shared Investigation")
            && ctrl.row(&session_a).map(|r| r.project_id.as_str()) == Some("proj-alpha")
            && ctrl.row(&session_b).map(|r| r.project_id.as_str()) == Some("proj-beta")
            && ctrl.selected.as_ref() == Some(&session_b),
    );

    fixture.set_mode(AntigravityFixtureMode::WrongSelected);
    let sel_wrong = ctrl.selection(&session_a);
    let res_wrong = push_open(&mut ctrl, sel_wrong.as_ref(), &mut receipts);
    checks.insert(
        "wrong_selected_readback_unconfirmed".into(),
        res_wrong == AntigravityOpenResult::Unconfirmed,
    );
    fixture.set_mode(AntigravityFixtureMode::Normal);

    let tool_event_a = AntigravityHookEvent {
        conversation_id: session_a.conversation_id.clone(),
        workspace_path: session_a.workspace_path.clone(),
        event_type: AntigravityHookEventType::PreToolUse,
        model_name: "gemini-2.5-pro".into(),
        invocation_num: Some(1),
        step_idx: Some(2),
        execution_num: Some(1),
        tool_name: Some("run_command".into()),
        termination_reason: None,
        has_error: false,
        fully_idle: Some(false),
        event_id: None,
        title: None,
        project_id: None,
    };
    let accepted_first = fixture.ingest(&tool_event_a).map_err(|e| e.to_string())?;
    let accepted_duplicate = fixture.ingest(&tool_event_a).map_err(|e| e.to_string())?;
    ctrl.refresh();

    let row_a1 = ctrl.row(&session_a).cloned();
    let row_b1 = ctrl.row(&session_b).cloned();
    checks.insert(
        "two_sessions_isolated".into(),
        ctrl.rows.len() == 2
            && row_a1.as_ref().map(|r| r.phase) == Some(AntigravitySessionPhase::RunningTool)
            && row_a1.as_ref().and_then(|r| r.last_tool_name.as_deref()) == Some("run_command")
            && row_a1.as_ref().map(|r| r.transition_count) == Some(2)
            && row_a1.as_ref().map(|r| r.title.as_str()) == Some("Shared Investigation")
            && row_b1.as_ref().map(|r| r.phase) == Some(AntigravitySessionPhase::Idle)
            && row_b1
                .as_ref()
                .and_then(|r| r.last_tool_name.as_deref())
                .is_none()
            && row_b1.as_ref().map(|r| r.transition_count) == Some(1)
            && row_b1.as_ref().map(|r| r.title.as_str()) == Some("Shared Investigation"),
    );

    let dup_counts = fixture.counts();
    checks.insert(
        "duplicate_events_deduplicated".into(),
        accepted_first
            && !accepted_duplicate
            && dup_counts.1 == 1
            && ctrl.last_changes.len() == 1
            && row_a1.as_ref().map(|r| r.transition_count) == Some(2),
    );

    let stop_not_idle = AntigravityHookEvent {
        conversation_id: session_a.conversation_id.clone(),
        workspace_path: session_a.workspace_path.clone(),
        event_type: AntigravityHookEventType::Stop,
        model_name: "gemini-2.5-pro".into(),
        invocation_num: None,
        step_idx: None,
        execution_num: None,
        tool_name: None,
        termination_reason: Some("NO_TOOL_CALL".into()),
        has_error: false,
        fully_idle: Some(false),
        event_id: None,
        title: None,
        project_id: None,
    };
    let stop_not_idle_first = fixture.ingest(&stop_not_idle).map_err(|e| e.to_string())?;
    let stop_not_idle_dup = fixture.ingest(&stop_not_idle).map_err(|e| e.to_string())?;
    ctrl.refresh();
    let row_after_not_idle = ctrl.row(&session_a).cloned();
    checks.insert(
        "stop_fully_idle_false_not_completed".into(),
        stop_not_idle_first
            && !stop_not_idle_dup
            && row_after_not_idle.as_ref().map(|r| r.phase)
                == Some(AntigravitySessionPhase::BackgroundActive)
            && row_after_not_idle.as_ref().is_some_and(|r| {
                !r.phase.is_completed() && !r.fully_idle && r.transition_count == 3
            }),
    );

    let stop_fully_idle = AntigravityHookEvent {
        conversation_id: session_a.conversation_id.clone(),
        workspace_path: session_a.workspace_path.clone(),
        event_type: AntigravityHookEventType::Stop,
        model_name: "gemini-2.5-pro".into(),
        invocation_num: None,
        step_idx: None,
        execution_num: None,
        tool_name: None,
        termination_reason: Some("NO_TOOL_CALL".into()),
        has_error: false,
        fully_idle: Some(true),
        event_id: None,
        title: None,
        project_id: None,
    };
    let stop_idle_accepted = fixture
        .ingest(&stop_fully_idle)
        .map_err(|e| e.to_string())?;
    ctrl.refresh();
    let row_after_idle = ctrl.row(&session_a).cloned();
    checks.insert(
        "stop_fully_idle_true_completed".into(),
        stop_idle_accepted
            && row_after_idle.as_ref().map(|r| r.phase) == Some(AntigravitySessionPhase::Completed)
            && row_after_idle
                .as_ref()
                .is_some_and(|r| r.phase.is_completed() && r.fully_idle && r.transition_count == 4),
    );

    let sel_confirm_a = ctrl.selection(&session_a);
    let _ = push_open(&mut ctrl, sel_confirm_a.as_ref(), &mut receipts);

    let created_key =
        AntigravitySessionKey::new("conv-alpha-created", AntigravityStatusFixture::WORKSPACE_A);
    let mismatch_create_cmd = ctrl.create_intent(
        AntigravityStatusFixture::WORKSPACE_A,
        Some("proj-wrong"),
        "Should be blocked by projectId binding",
        Some("Wrong Project"),
        None,
        (Some("conv-alpha-wrong"), Some("req-create-wrong-proj")),
    );
    let mismatch_res = push_cmd(
        &mut ctrl,
        "antigravity_create",
        "menu-antigravity-create",
        mismatch_create_cmd.as_ref(),
        &mut receipts,
    );
    let project_mismatch_blocked = mismatch_res == AntigravityCommandResult::Unconfirmed
        && ctrl
            .last_command_reply
            .as_ref()
            .map(|r| r.reason_code.as_str())
            == Some("guard.projectIdMismatch")
        && ctrl
            .row(&AntigravitySessionKey::new(
                "conv-alpha-wrong",
                AntigravityStatusFixture::WORKSPACE_A,
            ))
            .is_none();

    let create_cmd = ctrl.create_intent(
        AntigravityStatusFixture::WORKSPACE_A,
        Some("proj-alpha"),
        "Create new conversation bound to proj-alpha",
        Some("Alpha Created Conversation"),
        Some("gemini-2.5-flash"),
        (
            Some(&created_key.conversation_id),
            Some("req-create-alpha-1"),
        ),
    );
    let create_res = push_cmd(
        &mut ctrl,
        "antigravity_create",
        "menu-antigravity-create",
        create_cmd.as_ref(),
        &mut receipts,
    );
    let created_row = ctrl.row(&created_key).cloned();
    checks.insert(
        "create_binds_project_id_and_confirms_selected".into(),
        project_mismatch_blocked
            && create_res == AntigravityCommandResult::Created
            && ctrl.selected.as_ref() == Some(&created_key)
            && created_row.as_ref().map(|r| r.project_id.as_str()) == Some("proj-alpha")
            && created_row.as_ref().map(|r| r.title.as_str()) == Some("Alpha Created Conversation")
            && created_row.as_ref().map(|r| r.phase) == Some(AntigravitySessionPhase::Running),
    );

    let sel_back_a = ctrl.selection(&session_a);
    let _ = push_open(&mut ctrl, sel_back_a.as_ref(), &mut receipts);

    let b_before_send = ctrl.row(&session_b).cloned();
    let created_before_send = ctrl.row(&created_key).cloned();
    let unselected_send_cmd = ctrl.send_intent(
        &session_b,
        Some("proj-beta"),
        "Must not touch unselected session B",
        AntigravityDeliveryMode::Auto,
        Some("req-send-unselected-b"),
    );
    let unselected_send_res = push_cmd(
        &mut ctrl,
        "antigravity_send",
        "menu-antigravity-send",
        unselected_send_cmd.as_ref(),
        &mut receipts,
    );
    let unselected_send_blocked = unselected_send_res == AntigravityCommandResult::Unconfirmed
        && ctrl
            .last_command_reply
            .as_ref()
            .map(|r| r.reason_code.as_str())
            == Some("guard.selectionMismatch")
        && ctrl.row(&session_b).cloned() == b_before_send
        && ctrl.row(&session_a).map(|r| r.transition_count) == Some(4);

    let send_idle_a_cmd = ctrl.send_intent(
        &session_a,
        Some("proj-alpha"),
        "Resume session A from completed state",
        AntigravityDeliveryMode::Auto,
        Some("req-send-alpha-idle-1"),
    );
    let send_idle_res = push_cmd(
        &mut ctrl,
        "antigravity_send",
        "menu-antigravity-send",
        send_idle_a_cmd.as_ref(),
        &mut receipts,
    );
    let a_after_send = ctrl.row(&session_a).cloned();
    let b_after_send = ctrl.row(&session_b).cloned();
    let created_after_send = ctrl.row(&created_key).cloned();
    checks.insert(
        "two_sessions_send_only_modifies_target".into(),
        unselected_send_blocked
            && send_idle_res == AntigravityCommandResult::Started
            && ctrl.last_command_reply.as_ref().map(|r| r.delivery)
                == Some(AntigravityDeliveryOutcome::Started)
            && ctrl
                .last_command_reply
                .as_ref()
                .is_some_and(|r| !r.in_flight_steering)
            && a_after_send.as_ref().map(|r| r.phase) == Some(AntigravitySessionPhase::Running)
            && a_after_send.as_ref().map(|r| r.invocation_num) == Some(2)
            && a_after_send.as_ref().map(|r| r.transition_count) == Some(5)
            && b_after_send == b_before_send
            && created_after_send == created_before_send,
    );

    let row_count_before_running = ctrl.rows.len();
    let running_default_steer_cmd = ctrl.send_intent(
        &session_a,
        Some("proj-alpha"),
        "In-flight message under official agentapi contract",
        AntigravityDeliveryMode::Steer,
        Some("req-send-alpha-running-default"),
    );
    let default_queued_res = push_cmd(
        &mut ctrl,
        "antigravity_send",
        "menu-antigravity-send",
        running_default_steer_cmd.as_ref(),
        &mut receipts,
    );
    let default_queued_confirmed = default_queued_res == AntigravityCommandResult::Queued
        && ctrl.last_command_reply.as_ref().map(|r| r.delivery)
            == Some(AntigravityDeliveryOutcome::NextInvocation)
        && ctrl
            .last_command_reply
            .as_ref()
            .is_some_and(|r| !r.in_flight_steering)
        && ctrl
            .last_command_reply
            .as_ref()
            .map(|r| r.reason_code.as_str())
            == Some("agentapi.steerUnsupportedQueuedNextInvocation")
        && ctrl.rows.len() == row_count_before_running
        && ctrl.row(&session_a).map(|r| r.transition_count) == Some(6);

    fixture.set_in_flight_steer_enabled(true);
    let running_steer_enabled_cmd = ctrl.send_intent(
        &session_a,
        Some("proj-alpha"),
        "In-flight message with steering enabled",
        AntigravityDeliveryMode::Steer,
        Some("req-send-alpha-running-steered"),
    );
    let explicit_steer_res = push_cmd(
        &mut ctrl,
        "antigravity_send",
        "menu-antigravity-send",
        running_steer_enabled_cmd.as_ref(),
        &mut receipts,
    );
    fixture.set_in_flight_steer_enabled(false);
    let explicit_steer_confirmed = explicit_steer_res == AntigravityCommandResult::Steered
        && ctrl.last_command_reply.as_ref().map(|r| r.delivery)
            == Some(AntigravityDeliveryOutcome::Steered)
        && ctrl
            .last_command_reply
            .as_ref()
            .is_some_and(|r| r.in_flight_steering)
        && ctrl
            .last_command_reply
            .as_ref()
            .map(|r| r.reason_code.as_str())
            == Some("agentapi.sendMessageSteered")
        && ctrl.rows.len() == row_count_before_running
        && ctrl.row(&session_a).map(|r| r.step_idx) == Some(3)
        && ctrl.row(&session_a).map(|r| r.transition_count) == Some(7);
    checks.insert(
        "running_session_send_steering_verified_separately".into(),
        default_queued_confirmed && explicit_steer_confirmed,
    );

    fixture.set_unknown_next_command(true);
    let timeout_send_cmd = ctrl.send_intent(
        &session_a,
        Some("proj-alpha"),
        "Message whose response is initially unknown",
        AntigravityDeliveryMode::Queue,
        Some("req-send-timeout-1"),
    );
    let timeout_res = push_cmd(
        &mut ctrl,
        "antigravity_send",
        "menu-antigravity-send",
        timeout_send_cmd.as_ref(),
        &mut receipts,
    );
    let cmd_counts_after_timeout = fixture.command_counts();
    let unknown_latched = timeout_res == AntigravityCommandResult::Unknown
        && ctrl
            .pending_unknown_command
            .as_ref()
            .map(|p| p.operation.request_id())
            == Some("req-send-timeout-1");
    let blocked_new_send = ctrl.send_intent(
        &session_a,
        Some("proj-alpha"),
        "Must not issue new request while unknown is latched",
        AntigravityDeliveryMode::Queue,
        Some("req-send-blocked-2"),
    );
    let query_reconcile_cmd = ctrl.query_intent(Some("req-send-timeout-1"), None, None, None);
    let query_res = push_cmd(
        &mut ctrl,
        "antigravity_query",
        "menu-antigravity-query",
        query_reconcile_cmd.as_ref(),
        &mut receipts,
    );
    let cmd_counts_after_reconcile = fixture.command_counts();
    checks.insert(
        "timeout_unknown_reconciles_without_resend".into(),
        unknown_latched
            && blocked_new_send.is_none()
            && query_res == AntigravityCommandResult::Queued
            && ctrl
                .last_command_reply
                .as_ref()
                .is_some_and(|r| r.reconciled && r.duplicate)
            && ctrl.pending_unknown_command.is_none()
            && cmd_counts_after_reconcile.2 == cmd_counts_after_timeout.2
            && cmd_counts_after_reconcile.4 == 1,
    );

    let b_before_stop = ctrl.row(&session_b).cloned();
    let unselected_stop_cmd =
        ctrl.stop_intent(&session_b, None, None, true, Some("req-stop-unselected-b"));
    let unselected_stop_res = push_cmd(
        &mut ctrl,
        "antigravity_stop",
        "menu-antigravity-stop",
        unselected_stop_cmd.as_ref(),
        &mut receipts,
    );
    let unselected_stop_blocked = unselected_stop_res == AntigravityCommandResult::Unconfirmed
        && ctrl
            .last_command_reply
            .as_ref()
            .map(|r| r.reason_code.as_str())
            == Some("guard.selectionMismatch")
        && ctrl.row(&session_b).cloned() == b_before_stop;

    let stale_stop_cmd = ctrl.stop_intent(
        &session_a,
        Some(1),
        Some(1),
        true,
        Some("req-stop-stale-inv"),
    );
    let stale_stop_res = push_cmd(
        &mut ctrl,
        "antigravity_stop",
        "menu-antigravity-stop",
        stale_stop_cmd.as_ref(),
        &mut receipts,
    );
    let stale_stop_noop = stale_stop_res == AntigravityCommandResult::Noop
        && ctrl
            .last_command_reply
            .as_ref()
            .map(|r| r.reason_code.as_str())
            == Some("guard.stopTargetChanged")
        && ctrl.row(&session_a).map(|r| r.phase) == Some(AntigravitySessionPhase::Running);

    let valid_stop_cmd =
        ctrl.stop_intent(&session_a, Some(2), Some(2), true, Some("req-stop-alpha-1"));
    let valid_stop_res = push_cmd(
        &mut ctrl,
        "antigravity_stop",
        "menu-antigravity-stop",
        valid_stop_cmd.as_ref(),
        &mut receipts,
    );
    let a_after_stop = ctrl.row(&session_a).cloned();
    let b_after_stop = ctrl.row(&session_b).cloned();
    checks.insert(
        "stop_selected_session_only_and_stale_noop".into(),
        unselected_stop_blocked
            && stale_stop_noop
            && valid_stop_res == AntigravityCommandResult::Stopped
            && ctrl.last_command_reply.as_ref().map(|r| r.delivery)
                == Some(AntigravityDeliveryOutcome::Canceled)
            && ctrl
                .last_command_reply
                .as_ref()
                .map(|r| r.reason_code.as_str())
                == Some("languageServer.cancelCascadeInvocationConfirmed")
            && a_after_stop.as_ref().map(|r| r.phase) == Some(AntigravitySessionPhase::Canceled)
            && a_after_stop.as_ref().is_some_and(|r| {
                r.fully_idle && r.termination_reason.as_deref() == Some("USER_CANCELED")
            })
            && b_after_stop == b_before_stop,
    );

    fixture.set_pending_interaction(
        &session_a,
        Some(AntigravityPendingInteraction {
            interaction_id: "int-question-1".into(),
            interaction_type: AntigravityInteractionType::Question,
            step_index: 4,
            option_ids: vec!["opt-fast".into(), "opt-safe".into()],
        }),
    );
    fixture.set_pending_interaction(
        &session_b,
        Some(AntigravityPendingInteraction {
            interaction_id: "int-perm-b".into(),
            interaction_type: AntigravityInteractionType::FilePermission,
            step_index: 1,
            option_ids: Vec::new(),
        }),
    );
    ctrl.refresh();

    let question_as_allow_cmd = ctrl.resolve_intent(
        &session_a,
        "int-question-1",
        AntigravityInteractionType::Question,
        AntigravityInteractionDecision::Allow,
        Vec::new(),
        Some("req-resolve-question-as-allow"),
    );
    let question_as_allow_res = push_cmd(
        &mut ctrl,
        "antigravity_resolve",
        "menu-antigravity-resolve",
        question_as_allow_cmd.as_ref(),
        &mut receipts,
    );
    let question_as_allow_rejected = question_as_allow_res == AntigravityCommandResult::Unconfirmed
        && ctrl
            .last_command_reply
            .as_ref()
            .map(|r| r.reason_code.as_str())
            == Some("guard.questionCannotApproveAsPermission")
        && ctrl
            .row(&session_a)
            .and_then(|r| r.pending_interaction.as_ref())
            .map(|p| p.interaction_id.as_str())
            == Some("int-question-1")
        && ctrl
            .row(&session_b)
            .and_then(|r| r.pending_interaction.as_ref())
            .map(|p| p.interaction_id.as_str())
            == Some("int-perm-b");

    let question_answer_cmd = ctrl.resolve_intent(
        &session_a,
        "int-question-1",
        AntigravityInteractionType::Question,
        AntigravityInteractionDecision::Answer,
        vec!["opt-fast".into()],
        Some("req-resolve-question-ok"),
    );
    let question_answer_res = push_cmd(
        &mut ctrl,
        "antigravity_resolve",
        "menu-antigravity-resolve",
        question_answer_cmd.as_ref(),
        &mut receipts,
    );
    let question_answered = question_answer_res == AntigravityCommandResult::Resolved
        && ctrl.last_command_reply.as_ref().map(|r| r.delivery)
            == Some(AntigravityDeliveryOutcome::Answer)
        && ctrl
            .row(&session_a)
            .is_some_and(|r| r.pending_interaction.is_none())
        && ctrl
            .row(&session_b)
            .and_then(|r| r.pending_interaction.as_ref())
            .map(|p| p.interaction_id.as_str())
            == Some("int-perm-b");

    fixture.set_pending_interaction(
        &session_a,
        Some(AntigravityPendingInteraction {
            interaction_id: "int-perm-a".into(),
            interaction_type: AntigravityInteractionType::RunCommand,
            step_index: 5,
            option_ids: Vec::new(),
        }),
    );
    ctrl.refresh();

    let perm_allow_cmd = ctrl.resolve_intent(
        &session_a,
        "int-perm-a",
        AntigravityInteractionType::RunCommand,
        AntigravityInteractionDecision::Allow,
        Vec::new(),
        Some("req-resolve-perm-a"),
    );
    let perm_allow_res = push_cmd(
        &mut ctrl,
        "antigravity_resolve",
        "menu-antigravity-resolve",
        perm_allow_cmd.as_ref(),
        &mut receipts,
    );
    let perm_allowed = perm_allow_res == AntigravityCommandResult::Resolved
        && ctrl.last_command_reply.as_ref().map(|r| r.delivery)
            == Some(AntigravityDeliveryOutcome::Allow)
        && ctrl
            .row(&session_a)
            .is_some_and(|r| r.pending_interaction.is_none())
        && ctrl
            .row(&session_b)
            .and_then(|r| r.pending_interaction.as_ref())
            .map(|p| p.interaction_id.as_str())
            == Some("int-perm-b");

    let perm_dup_res = push_cmd(
        &mut ctrl,
        "antigravity_resolve",
        "menu-antigravity-resolve",
        perm_allow_cmd.as_ref(),
        &mut receipts,
    );
    let perm_duplicate_idempotent = perm_dup_res == AntigravityCommandResult::Resolved
        && ctrl
            .last_command_reply
            .as_ref()
            .is_some_and(|r| r.duplicate);

    fixture.set_pending_interaction(
        &session_a,
        Some(AntigravityPendingInteraction {
            interaction_id: "int-perm-expired".into(),
            interaction_type: AntigravityInteractionType::Permission,
            step_index: 6,
            option_ids: Vec::new(),
        }),
    );
    ctrl.refresh();
    let expired_resolve_cmd = ctrl.resolve_intent(
        &session_a,
        "int-perm-expired",
        AntigravityInteractionType::Permission,
        AntigravityInteractionDecision::Allow,
        Vec::new(),
        Some("req-resolve-expired"),
    );
    fixture.expire_pending_interaction(&session_a, "int-perm-expired");
    let expired_resolve_res = push_cmd(
        &mut ctrl,
        "antigravity_resolve",
        "menu-antigravity-resolve",
        expired_resolve_cmd.as_ref(),
        &mut receipts,
    );
    let expired_resolve_noop = expired_resolve_res == AntigravityCommandResult::Noop
        && ctrl
            .last_command_reply
            .as_ref()
            .map(|r| r.reason_code.as_str())
            == Some("guard.interactionAlreadyResolved")
        && ctrl
            .row(&session_b)
            .and_then(|r| r.pending_interaction.as_ref())
            .map(|p| p.interaction_id.as_str())
            == Some("int-perm-b");

    fixture.set_pending_interaction(&session_b, None);
    ctrl.refresh();
    checks.insert(
        "resolve_interaction_and_question_guard".into(),
        question_as_allow_rejected
            && question_answered
            && perm_allowed
            && perm_duplicate_idempotent
            && expired_resolve_noop,
    );

    let unsupported_thinking_cmd = ctrl.switch_model_intent(
        &session_a,
        "gemini-2.5-flash-lite",
        "high",
        Some("req-model-unsupported-thinking"),
    );
    let unsupported_thinking_res = push_cmd(
        &mut ctrl,
        "antigravity_switch_model",
        "menu-antigravity-model",
        unsupported_thinking_cmd.as_ref(),
        &mut receipts,
    );
    let cmd_counts_after_unsupported_model = fixture.command_counts();
    let unsupported_thinking_rejected = unsupported_thinking_res
        == AntigravityCommandResult::Unconfirmed
        && ctrl
            .last_command_reply
            .as_ref()
            .map(|r| r.reason_code.as_str())
            == Some("guard.unsupportedThinkingLevel")
        && ctrl.row(&session_a).map(|r| r.model_name.as_str()) == Some("gemini-2.5-pro")
        && cmd_counts_after_unsupported_model.9 == 0;

    let valid_switch_cmd = ctrl.switch_model_intent(
        &session_a,
        "gemini-2.5-flash",
        "medium",
        Some("req-model-switch-a"),
    );
    let valid_switch_res = push_cmd(
        &mut ctrl,
        "antigravity_switch_model",
        "menu-antigravity-model",
        valid_switch_cmd.as_ref(),
        &mut receipts,
    );
    let a_after_switch = ctrl.row(&session_a).cloned();
    let b_after_switch = ctrl.row(&session_b).cloned();
    let valid_switch_confirmed = valid_switch_res == AntigravityCommandResult::ModelSwitched
        && ctrl.last_command_reply.as_ref().map(|r| r.delivery)
            == Some(AntigravityDeliveryOutcome::ModelSwitched)
        && ctrl
            .last_command_reply
            .as_ref()
            .map(|r| r.reason_code.as_str())
            == Some("languageServer.cascadeModelConfigConfirmed")
        && a_after_switch.as_ref().map(|r| r.model_name.as_str()) == Some("gemini-2.5-flash")
        && a_after_switch.as_ref().map(|r| r.thinking_level.as_str()) == Some("medium")
        && b_after_switch.as_ref().map(|r| r.model_name.as_str()) == Some("gemini-2.5-flash")
        && b_after_switch.as_ref().map(|r| r.thinking_level.as_str()) == Some("high");

    fixture.set_model_readback_override(Some("gemini-2.5-pro".into()), Some("low".into()));
    let mismatch_switch_cmd = ctrl.switch_model_intent(
        &session_a,
        "gemini-2.5-pro",
        "high",
        Some("req-model-mismatch-a"),
    );
    let mismatch_switch_res = push_cmd(
        &mut ctrl,
        "antigravity_switch_model",
        "menu-antigravity-model",
        mismatch_switch_cmd.as_ref(),
        &mut receipts,
    );
    let mismatch_switch_rejected = mismatch_switch_res == AntigravityCommandResult::Unconfirmed
        && ctrl
            .last_command_reply
            .as_ref()
            .map(|r| r.reason_code.as_str())
            == Some("guard.modelReadbackMismatch");
    fixture.set_model_readback_override(None, None);

    let restore_switch_cmd = ctrl.switch_model_intent(
        &session_a,
        "gemini-2.5-pro",
        "high",
        Some("req-model-restore-a"),
    );
    let restore_switch_res = push_cmd(
        &mut ctrl,
        "antigravity_switch_model",
        "menu-antigravity-model",
        restore_switch_cmd.as_ref(),
        &mut receipts,
    );
    checks.insert(
        "switch_model_and_thinking_guard".into(),
        unsupported_thinking_rejected
            && valid_switch_confirmed
            && mismatch_switch_rejected
            && restore_switch_res == AntigravityCommandResult::ModelSwitched
            && ctrl.row(&session_a).map(|r| r.model_name.as_str()) == Some("gemini-2.5-pro")
            && ctrl.row(&session_a).map(|r| r.thinking_level.as_str()) == Some("high"),
    );

    let missing_target_key =
        AntigravitySessionKey::new("conv-does-not-exist", AntigravityStatusFixture::WORKSPACE_A);
    let missing_send_cmd = ctrl.send_intent(
        &missing_target_key,
        Some("proj-alpha"),
        "Should not redirect to session A or create a session",
        AntigravityDeliveryMode::Auto,
        Some("req-send-missing-target"),
    );
    let a_before_missing = ctrl.row(&session_a).cloned();
    let row_count_before_missing = ctrl.rows.len();
    let missing_send_res = push_cmd(
        &mut ctrl,
        "antigravity_send",
        "menu-antigravity-send",
        missing_send_cmd.as_ref(),
        &mut receipts,
    );
    let missing_send_zero_redirect = missing_send_res == AntigravityCommandResult::Unconfirmed
        && ctrl
            .last_command_reply
            .as_ref()
            .map(|r| r.reason_code.as_str())
            == Some("guard.targetNotFound")
        && ctrl.selected.as_ref() == Some(&session_a)
        && ctrl.row(&session_a).cloned() == a_before_missing
        && ctrl.rows.len() == row_count_before_missing;

    let stale_deleted_selection = ctrl.selection(&session_b);
    fixture.delete_session(&session_b);
    let deleted_open_res = push_open(&mut ctrl, stale_deleted_selection.as_ref(), &mut receipts);
    let deleted_no_redirect = deleted_open_res == AntigravityOpenResult::Unconfirmed
        && ctrl.selected.as_ref() == Some(&session_a);
    checks.insert("deleted_target_no_redirect".into(), deleted_no_redirect);
    checks.insert(
        "missing_or_deleted_target_send_zero_redirect".into(),
        missing_send_zero_redirect && deleted_no_redirect,
    );

    let sel_before_missing_schema = ctrl.selection(&session_a);
    fixture.set_mode(AntigravityFixtureMode::MissingSchema);
    let missing_schema_open_res =
        push_open(&mut ctrl, sel_before_missing_schema.as_ref(), &mut receipts);
    checks.insert(
        "missing_schema_or_auth_fails_closed".into(),
        missing_schema_open_res == AntigravityOpenResult::Denied
            && ctrl.state == AntigravityConnectionState::Unavailable(AntigravityFailure::Denied),
    );

    ctrl.set_enabled(false);
    fixture.set_mode(AntigravityFixtureMode::Normal);
    fixture.set_csrf_auth(true);
    ctrl.set_enabled(true);
    let sel_for_csrf = ctrl.selection(&session_a);
    let _ = push_open(&mut ctrl, sel_for_csrf.as_ref(), &mut receipts);
    let stop_cmd_before_csrf_drop =
        ctrl.stop_intent(&session_a, None, None, true, Some("req-stop-csrf-drop"));
    fixture.set_csrf_auth(false);
    let csrf_drop_res = push_cmd(
        &mut ctrl,
        "antigravity_stop",
        "menu-antigravity-stop",
        stop_cmd_before_csrf_drop.as_ref(),
        &mut receipts,
    );
    checks.insert(
        "l3_schema_or_csrf_invalidation_fails_closed".into(),
        csrf_drop_res == AntigravityCommandResult::Denied
            && ctrl.state == AntigravityConnectionState::Unavailable(AntigravityFailure::Denied),
    );
    fixture.set_csrf_auth(true);

    ctrl.set_enabled(false);
    fixture.set_mode(AntigravityFixtureMode::Normal);
    ctrl.set_enabled(true);
    fixture.set_mode(AntigravityFixtureMode::Stale);
    ctrl.refresh();
    checks.insert(
        "stream_disconnect_marks_stale".into(),
        ctrl.state == AntigravityConnectionState::Unavailable(AntigravityFailure::Stale)
            && ctrl.rows.is_empty(),
    );

    ctrl.set_enabled(false);
    fixture.set_mode(AntigravityFixtureMode::Normal);
    ctrl.set_enabled(true);
    fixture.set_mode(AntigravityFixtureMode::Rotated);
    ctrl.refresh();
    checks.insert(
        "generation_change_marks_stale".into(),
        ctrl.state == AntigravityConnectionState::Unavailable(AntigravityFailure::Stale)
            && ctrl.rows.is_empty(),
    );

    ctrl.set_enabled(false);
    fixture.set_mode(AntigravityFixtureMode::Normal);
    ctrl.set_enabled(true);
    fixture.set_mode(AntigravityFixtureMode::Gap);
    ctrl.refresh();
    checks.insert(
        "journal_gap_marks_unavailable".into(),
        ctrl.state == AntigravityConnectionState::Unavailable(AntigravityFailure::Gap)
            && ctrl.rows.is_empty(),
    );

    let final_counts = fixture.counts();
    checks.insert(
        "zero_transcript_reads".into(),
        final_counts.2 == 0 && ctrl.transcript_reads == 0,
    );
    checks.insert(
        "zero_token_reads".into(),
        final_counts.3 == 0 && ctrl.token_reads == 0,
    );

    ctrl.shutdown();
    let after_shutdown_counts = fixture.counts();
    checks.insert(
        "bridge_subscription_closed".into(),
        after_shutdown_counts.6 > 0,
    );

    let final_cmd_counts = fixture.command_counts();
    let status = if checks.len() == 26 && checks.values().all(|v| *v) {
        "PASS"
    } else {
        "FAIL"
    };
    let report = AntigravityStatusScenarioReport {
        status: status.into(),
        installed_version: expected_caps.installed_version,
        language_server_cl: expected_caps.language_server_cl,
        transcript_reads: after_shutdown_counts.2,
        token_reads: after_shutdown_counts.3,
        open_requests: after_shutdown_counts.4,
        open_confirmed: after_shutdown_counts.5,
        create_requests: final_cmd_counts.0,
        create_confirmed: final_cmd_counts.1,
        send_requests: final_cmd_counts.2,
        send_confirmed: final_cmd_counts.3,
        query_requests: final_cmd_counts.4,
        stop_requests: final_cmd_counts.5,
        stop_confirmed: final_cmd_counts.6,
        resolve_requests: final_cmd_counts.7,
        resolve_confirmed: final_cmd_counts.8,
        switch_model_requests: final_cmd_counts.9,
        switch_model_confirmed: final_cmd_counts.10,
        duplicate_commands_dropped: final_cmd_counts.11,
        checks,
        native_app: "NOT_RUN".into(),
        physical_key: "NOT_RUN".into(),
        receipts,
    };

    if let Some(audit_path) = audit_log_path {
        if let Some(parent) = audit_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            let json_bytes = serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?;
            for name in [
                "antigravity-status.json",
                "antigravity-navigation.json",
                "antigravity-agentapi.json",
                "antigravity-l3-control.json",
            ] {
                std::fs::write(parent.join(name), &json_bytes).map_err(|e| e.to_string())?;
            }
        }
        let mut lines = String::new();
        for r in &report.receipts {
            let line = serde_json::to_string(r).map_err(|e| e.to_string())?;
            lines.push_str(&line);
            lines.push('\n');
        }
        std::fs::write(audit_path, lines).map_err(|e| e.to_string())?;
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_fully_idle_false_reduces_to_background_active_and_distinguishes_event_keys() {
        let key = AntigravitySessionKey::new("conv-1", "/workspace/ag-alpha");
        let start = AntigravityHookEvent {
            conversation_id: key.conversation_id.clone(),
            workspace_path: key.workspace_path.clone(),
            event_type: AntigravityHookEventType::PreInvocation,
            model_name: "gemini-2.5-pro".into(),
            invocation_num: Some(1),
            step_idx: Some(1),
            execution_num: Some(1),
            tool_name: None,
            termination_reason: None,
            has_error: false,
            fully_idle: Some(false),
            event_id: None,
            title: Some("Test".into()),
            project_id: Some("proj-1".into()),
        };
        let row1 = AntigravitySessionRow::reduce(None, &start);
        assert_eq!(row1.phase, AntigravitySessionPhase::Running);

        let stop_bg = AntigravityHookEvent {
            event_type: AntigravityHookEventType::Stop,
            termination_reason: Some("NO_TOOL_CALL".into()),
            fully_idle: Some(false),
            ..start.clone()
        };
        let row2 = AntigravitySessionRow::reduce(Some(&row1), &stop_bg);
        assert_eq!(row2.phase, AntigravitySessionPhase::BackgroundActive);
        assert!(!row2.fully_idle);

        let stop_done = AntigravityHookEvent {
            fully_idle: Some(true),
            ..stop_bg.clone()
        };
        assert_ne!(
            stop_bg.event_key(Some(&row1)),
            stop_done.event_key(Some(&row2))
        );
        let row3 = AntigravitySessionRow::reduce(Some(&row2), &stop_done);
        assert_eq!(row3.phase, AntigravitySessionPhase::Completed);
        assert!(row3.fully_idle);
    }

    #[test]
    fn voice_reply_reports_unsupported_on_antigravity_2_29_0() {
        let fixture = Arc::new(AntigravityStatusFixture::new());
        let ctrl = AntigravityStatusController::new(fixture);
        let (ok, msg) = ctrl.set_voice_reply(true);
        assert!(!ok);
        assert!(msg.contains("2.29.0"));
    }
}
