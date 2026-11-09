use crate::ownership::UnixNativeOwnershipTransport;
use crate::scenario::HostActionReceipt;
use mi_model::attention_round::{AttentionCandidate, AttentionRound};
use mi_model::host_router::ResolvedHostAction;
use mi_model::navigation_history::{NavigationHistory, NavigationPlace};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CindyScope {
    pub workspace_path: String,
}

impl CindyScope {
    pub fn new(workspace_path: impl Into<String>) -> Self {
        Self {
            workspace_path: workspace_path.into(),
        }
    }

    pub fn is_valid(&self) -> bool {
        !self.workspace_path.trim().is_empty() && self.workspace_path.starts_with('/')
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CindyTaskAddress {
    pub workspace_path: String,
    pub session_id: String,
}

impl CindyTaskAddress {
    pub fn new(workspace_path: impl Into<String>, session_id: impl Into<String>) -> Self {
        Self {
            workspace_path: workspace_path.into(),
            session_id: session_id.into(),
        }
    }

    pub fn id(&self) -> String {
        format!("{}#{}", self.workspace_path, self.session_id)
    }

    pub fn is_valid(&self) -> bool {
        !self.workspace_path.trim().is_empty()
            && self.workspace_path.starts_with('/')
            && !self.session_id.trim().is_empty()
            && self.session_id.len() <= 128
            && !self.session_id.contains('/')
            && !self.session_id.contains('?')
            && !self.session_id.contains('#')
    }

    pub fn deep_link(&self) -> String {
        format!("cindy://session/{}", self.session_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CindyInteractionType {
    Permission,
    Question,
    Plan,
}

impl CindyInteractionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Permission => "permission",
            Self::Question => "question",
            Self::Plan => "plan",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CindyInteractionDecision {
    Approve,
    AllowForSession,
    Reject,
    Accept,
    Cancel,
}

impl CindyInteractionDecision {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::AllowForSession => "allowForSession",
            Self::Reject => "reject",
            Self::Accept => "accept",
            Self::Cancel => "cancel",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CindyDeliveryMode {
    Auto,
    Steer,
    Queue,
}

impl CindyDeliveryMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Steer => "steer",
            Self::Queue => "queue",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CindyPendingInteraction {
    pub interaction_id: String,
    pub interaction_type: CindyInteractionType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
}

impl CindyPendingInteraction {
    pub fn is_valid(&self) -> bool {
        !self.interaction_id.trim().is_empty()
            && self.interaction_id.len() <= 128
            && !self.interaction_id.contains('\0')
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CindyTaskRow {
    pub address: CindyTaskAddress,
    pub title: String,
    pub phase: String,
    pub attention: bool,
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unread: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_watermark: Option<u64>,
    #[serde(default)]
    pub queued_count: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_interaction: Option<CindyPendingInteraction>,
}

impl CindyTaskRow {
    pub const VALID_PHASES: &[&str] =
        &["idle", "running", "needs-interaction", "completed", "error"];

    pub fn id(&self) -> String {
        self.address.id()
    }

    pub fn is_valid(&self) -> bool {
        self.address.is_valid()
            && !self.title.trim().is_empty()
            && Self::VALID_PHASES.contains(&self.phase.as_str())
            && self.queued_count >= 0
            && self
                .pending_interaction
                .as_ref()
                .is_none_or(CindyPendingInteraction::is_valid)
    }

    pub fn workspace_label(&self) -> String {
        let trimmed = self.address.workspace_path.trim_matches('/');
        trimmed
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.address.workspace_path)
            .to_owned()
    }

    pub fn is_eligible_for_attention_round(&self) -> bool {
        if self.phase == "needs-interaction" || self.pending_interaction.is_some() {
            return true;
        }
        if self.unread == Some(true) {
            return true;
        }
        if self.phase == "error" && self.attention {
            return true;
        }
        false
    }

    pub fn attention_priority_rank(&self) -> usize {
        if self.phase == "needs-interaction" || self.pending_interaction.is_some() {
            return 0;
        }
        if self.phase == "error" && (self.unread == Some(true) || self.attention) {
            return 1;
        }
        if self.unread == Some(true) {
            return 2;
        }
        3
    }

    pub fn round_watermark_signature(&self) -> String {
        let wm = self.event_watermark.unwrap_or(0);
        let iid = self
            .pending_interaction
            .as_ref()
            .map_or("", |p| p.interaction_id.as_str());
        let ur = if self.unread == Some(true) { "1" } else { "0" };
        format!("{wm}:{iid}:{ur}:{}", self.phase)
    }

    pub fn to_attention_candidate(&self) -> AttentionCandidate {
        AttentionCandidate::new(
            NavigationPlace::cindy(&self.address.workspace_path, &self.address.session_id),
            &self.title,
            self.attention_priority_rank(),
            self.round_watermark_signature(),
            self.is_eligible_for_attention_round(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CindySessionSnapshot {
    pub instance_id: String,
    pub generation: u64,
    pub owner_epoch: String,
    pub scopes: Vec<CindyScope>,
    pub rows: Vec<CindyTaskRow>,
    pub selected: Option<CindyTaskAddress>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CindySelection {
    pub address: CindyTaskAddress,
    pub instance_id: String,
    pub generation: u64,
    pub owner_epoch: String,
    pub local_epoch: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CindyOpenResult {
    Delivered,
    Confirmed,
    Unconfirmed,
    Stale,
    Denied,
    Unavailable,
}

impl CindyOpenResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Delivered => "delivered",
            Self::Confirmed => "confirmed",
            Self::Unconfirmed => "unconfirmed",
            Self::Stale => "stale",
            Self::Denied => "denied",
            Self::Unavailable => "unavailable",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Delivered => "深链已投递",
            Self::Confirmed => "导航已确认",
            Self::Unconfirmed => "未确认",
            Self::Stale => "任务已失效",
            Self::Denied => "越权拒绝",
            Self::Unavailable => "桥接不可用",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CindyOpenReply {
    pub instance_id: String,
    pub generation: u64,
    pub owner_epoch: String,
    pub status: CindyOpenResult,
    pub delivered: Option<CindyTaskAddress>,
    pub selected: Option<CindyTaskAddress>,
    pub deep_link: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CindyCreateIntent {
    pub command_id: String,
    pub scope: CindyScope,
    pub text: Option<String>,
    pub instance_id: String,
    pub generation: u64,
    pub owner_epoch: String,
    pub local_epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CindySendIntent {
    pub command_id: String,
    pub selection: CindySelection,
    pub text: String,
    pub delivery_mode: CindyDeliveryMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CindyStopIntent {
    pub command_id: String,
    pub selection: CindySelection,
    pub expected_run_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CindyResolveIntent {
    pub command_id: String,
    pub selection: CindySelection,
    pub interaction_id: String,
    pub interaction_type: CindyInteractionType,
    pub decision: CindyInteractionDecision,
    pub option_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CindyQueryIntent {
    pub command_id: String,
    pub scope: CindyScope,
    pub target: Option<CindyTaskAddress>,
    pub instance_id: String,
    pub generation: u64,
    pub owner_epoch: String,
    pub local_epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CindyQueuedCommand {
    Create {
        scope: CindyScope,
        text: Option<String>,
        command_id: Option<String>,
    },
    Send {
        address: CindyTaskAddress,
        text: String,
        delivery_mode: CindyDeliveryMode,
        command_id: Option<String>,
    },
    Stop {
        address: CindyTaskAddress,
        expected_run_id: String,
        command_id: Option<String>,
    },
    Resolve {
        address: CindyTaskAddress,
        interaction_id: String,
        interaction_type: CindyInteractionType,
        decision: CindyInteractionDecision,
        option_id: Option<String>,
        command_id: Option<String>,
    },
    Query {
        scope: CindyScope,
        target: Option<CindyTaskAddress>,
        command_id: String,
    },
    NextAttention,
}

impl CindyQueuedCommand {
    pub fn action_name(&self) -> &'static str {
        match self {
            Self::Create { .. } => "cindy_create",
            Self::Send { .. } => "cindy_send",
            Self::Stop { .. } => "cindy_stop",
            Self::Resolve { .. } => "cindy_resolve",
            Self::Query { .. } => "cindy_query",
            Self::NextAttention => "cindy_next_attention",
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Create { scope, .. } => format!("Cindy 新建任务 {}", scope.workspace_path),
            Self::Send {
                address,
                delivery_mode,
                ..
            } => format!("Cindy {} {}", delivery_mode.as_str(), address.session_id),
            Self::Stop {
                address,
                expected_run_id,
                ..
            } => format!("Cindy 停止 {}#{}", address.session_id, expected_run_id),
            Self::Resolve {
                address,
                interaction_id,
                interaction_type,
                decision,
                ..
            } => format!(
                "Cindy 响应 {}#{}:{}->{}",
                address.session_id,
                interaction_type.as_str(),
                interaction_id,
                decision.as_str()
            ),
            Self::Query { command_id, .. } => format!("Cindy 查询 {command_id}"),
            Self::NextAttention => "Cindy 下一待办任务".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CindyCommandResult {
    Confirmed,
    Delivered,
    Started,
    Steered,
    Queued,
    Stopped,
    Resolved,
    Noop,
    InteractionTypeMismatch,
    Unconfirmed,
    Stale,
    Denied,
    Unavailable,
    Unknown,
}

impl CindyCommandResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Delivered => "delivered",
            Self::Started => "started",
            Self::Steered => "steered",
            Self::Queued => "queued",
            Self::Stopped => "stopped",
            Self::Resolved => "resolved",
            Self::Noop => "noop",
            Self::InteractionTypeMismatch => "interactionTypeMismatch",
            Self::Unconfirmed => "unconfirmed",
            Self::Stale => "stale",
            Self::Denied => "denied",
            Self::Unavailable => "unavailable",
            Self::Unknown => "unknown",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Confirmed => "已确认",
            Self::Delivered => "深链已投递",
            Self::Started => "已启动轮次",
            Self::Steered => "已中途引导",
            Self::Queued => "已排队",
            Self::Stopped => "已停止",
            Self::Resolved => "已响应请求",
            Self::Noop => "幂等无操作",
            Self::InteractionTypeMismatch => "请求类型不匹配",
            Self::Unconfirmed => "未确认",
            Self::Stale => "任务已失效",
            Self::Denied => "越权拒绝",
            Self::Unavailable => "桥接不可用",
            Self::Unknown => "结果待核对",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CindyCommandReply {
    pub command_id: String,
    pub instance_id: String,
    pub generation: u64,
    pub owner_epoch: String,
    pub status: CindyCommandResult,
    pub duplicate: bool,
    pub delivered: Option<CindyTaskAddress>,
    pub selected: Option<CindyTaskAddress>,
    pub deep_link: Option<String>,
    pub delivery: Option<CindyDeliveryMode>,
    pub run_id: Option<String>,
    pub queued_count: i64,
    pub interaction_id: Option<String>,
    pub interaction_type: Option<CindyInteractionType>,
    pub reason_code: Option<String>,
}

impl CindyCommandReply {
    pub fn simple(
        command_id: impl Into<String>,
        instance_id: impl Into<String>,
        generation: u64,
        owner_epoch: impl Into<String>,
        status: CindyCommandResult,
        reason_code: Option<&str>,
    ) -> Self {
        Self {
            command_id: command_id.into(),
            instance_id: instance_id.into(),
            generation,
            owner_epoch: owner_epoch.into(),
            status,
            duplicate: false,
            delivered: None,
            selected: None,
            deep_link: None,
            delivery: None,
            run_id: None,
            queued_count: 0,
            interaction_id: None,
            interaction_type: None,
            reason_code: reason_code.map(String::from),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CindyFailure {
    #[error("缺少或无效授权")]
    Grant,
    #[error("越权拒绝")]
    Denied,
    #[error("实例或账号已换代")]
    Stale,
    #[error("Cindy 未连接")]
    Unavailable,
    #[error("应答无效")]
    InvalidReply,
    #[error("请求超时待核对")]
    TransportTimeout,
}

impl CindyFailure {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Grant => "缺少或无效授权",
            Self::Denied => "越权拒绝",
            Self::Stale => "实例或账号已换代",
            Self::Unavailable => "Cindy 未连接",
            Self::InvalidReply => "应答无效",
            Self::TransportTimeout => "请求超时待核对",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CindyConnectionState {
    Disabled,
    Connecting,
    Ready,
    Unavailable(CindyFailure),
}

impl CindyConnectionState {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Disabled => "未启用",
            Self::Connecting => "连接中",
            Self::Ready => "已连接",
            Self::Unavailable(f) => f.label(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CindyGrant {
    pub version: u32,
    pub socket_path: String,
    pub token: String,
    pub instance_id: String,
    pub generation: u64,
    pub owner_epoch: String,
    pub scopes: Vec<CindyScope>,
}

impl CindyGrant {
    pub fn is_valid(&self) -> bool {
        if self.version != 1
            || self.socket_path.trim().is_empty()
            || !self.socket_path.starts_with('/')
            || self.socket_path.len() >= 104
            || !(32..=256).contains(&self.token.len())
            || self.instance_id.trim().is_empty()
            || self.instance_id.len() > 256
            || self.generation == 0
            || self.owner_epoch.trim().is_empty()
            || self.owner_epoch.len() > 256
            || self.scopes.is_empty()
            || self.scopes.len() > 64
            || !self.scopes.iter().all(CindyScope::is_valid)
        {
            return false;
        }
        let unique: HashSet<_> = self.scopes.iter().collect();
        unique.len() == self.scopes.len()
    }

    pub fn load(path: &Path) -> Result<Self, CindyFailure> {
        let parent = path.parent().ok_or(CindyFailure::Grant)?;
        let c_parent =
            CString::new(parent.as_os_str().as_bytes()).map_err(|_| CindyFailure::Grant)?;
        let c_path = CString::new(path.as_os_str().as_bytes()).map_err(|_| CindyFailure::Grant)?;
        let uid = unsafe { libc::geteuid() };
        let mut pst: libc::stat = unsafe { std::mem::zeroed() };
        if unsafe { libc::lstat(c_parent.as_ptr(), &mut pst) } != 0
            || (pst.st_mode & libc::S_IFMT) != libc::S_IFDIR
            || pst.st_uid != uid
            || (pst.st_mode & 0o077) != 0
        {
            return Err(CindyFailure::Grant);
        }
        let fd = unsafe {
            libc::open(
                c_path.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK,
            )
        };
        if fd < 0 {
            return Err(CindyFailure::Grant);
        }
        let res = (|| {
            let mut fst: libc::stat = unsafe { std::mem::zeroed() };
            if unsafe { libc::fstat(fd, &mut fst) } != 0
                || (fst.st_mode & libc::S_IFMT) != libc::S_IFREG
                || fst.st_uid != uid
                || (fst.st_mode & 0o077) != 0
                || !(1..=65536).contains(&fst.st_size)
            {
                return Err(CindyFailure::Grant);
            }
            let mut buf = vec![0u8; fst.st_size as usize];
            let mut off = 0;
            while off < buf.len() {
                let n = unsafe { libc::read(fd, buf[off..].as_mut_ptr().cast(), buf.len() - off) };
                if n <= 0 {
                    return Err(CindyFailure::Grant);
                }
                off += n as usize;
            }
            let grant: Self = serde_json::from_slice(&buf).map_err(|_| CindyFailure::Grant)?;
            if !grant.is_valid() {
                return Err(CindyFailure::Grant);
            }
            Ok(grant)
        })();
        unsafe {
            libc::close(fd);
        }
        res
    }
}

pub trait CindySessionServing: Send + Sync {
    fn connect(&self) -> Result<CindySessionSnapshot, CindyFailure>;
    fn read(&self) -> Result<CindySessionSnapshot, CindyFailure>;
    fn open(&self, selection: &CindySelection) -> CindyOpenReply;
    fn create(&self, intent: &CindyCreateIntent) -> CindyCommandReply {
        CindyCommandReply::simple(
            &intent.command_id,
            &intent.instance_id,
            intent.generation,
            &intent.owner_epoch,
            CindyCommandResult::Unavailable,
            Some("bridge.createUnsupported"),
        )
    }
    fn send(&self, intent: &CindySendIntent) -> CindyCommandReply {
        CindyCommandReply::simple(
            &intent.command_id,
            &intent.selection.instance_id,
            intent.selection.generation,
            &intent.selection.owner_epoch,
            CindyCommandResult::Unavailable,
            Some("bridge.sendUnsupported"),
        )
    }
    fn stop(&self, intent: &CindyStopIntent) -> CindyCommandReply {
        CindyCommandReply::simple(
            &intent.command_id,
            &intent.selection.instance_id,
            intent.selection.generation,
            &intent.selection.owner_epoch,
            CindyCommandResult::Unavailable,
            Some("bridge.stopUnsupported"),
        )
    }
    fn resolve(&self, intent: &CindyResolveIntent) -> CindyCommandReply {
        CindyCommandReply::simple(
            &intent.command_id,
            &intent.selection.instance_id,
            intent.selection.generation,
            &intent.selection.owner_epoch,
            CindyCommandResult::Unavailable,
            Some("bridge.resolveUnsupported"),
        )
    }
    fn query(&self, intent: &CindyQueryIntent) -> CindyCommandReply {
        CindyCommandReply::simple(
            &intent.command_id,
            &intent.instance_id,
            intent.generation,
            &intent.owner_epoch,
            CindyCommandResult::Unavailable,
            Some("bridge.queryUnsupported"),
        )
    }
    fn close(&self);
}

impl<T: CindySessionServing + ?Sized> CindySessionServing for Arc<T> {
    fn connect(&self) -> Result<CindySessionSnapshot, CindyFailure> {
        (**self).connect()
    }
    fn read(&self) -> Result<CindySessionSnapshot, CindyFailure> {
        (**self).read()
    }
    fn open(&self, selection: &CindySelection) -> CindyOpenReply {
        (**self).open(selection)
    }
    fn create(&self, intent: &CindyCreateIntent) -> CindyCommandReply {
        (**self).create(intent)
    }
    fn send(&self, intent: &CindySendIntent) -> CindyCommandReply {
        (**self).send(intent)
    }
    fn stop(&self, intent: &CindyStopIntent) -> CindyCommandReply {
        (**self).stop(intent)
    }
    fn resolve(&self, intent: &CindyResolveIntent) -> CindyCommandReply {
        (**self).resolve(intent)
    }
    fn query(&self, intent: &CindyQueryIntent) -> CindyCommandReply {
        (**self).query(intent)
    }
    fn close(&self) {
        (**self).close()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CindyWireRequest<'a> {
    version: u32,
    id: &'a str,
    token: &'a str,
    instance_id: &'a str,
    generation: u64,
    owner_epoch: &'a str,
    op: &'a str,
    scope: &'a CindyScope,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<&'a CindyTaskAddress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    delivery_mode: Option<CindyDeliveryMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expected_run_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    interaction_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    interaction_type: Option<CindyInteractionType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    decision: Option<CindyInteractionDecision>,
    #[serde(skip_serializing_if = "Option::is_none")]
    option_id: Option<&'a str>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CindyWireReply {
    version: u32,
    id: String,
    instance_id: String,
    generation: u64,
    owner_epoch: String,
    status: String,
    command_id: Option<String>,
    duplicate: Option<bool>,
    rows: Option<Vec<CindyTaskRow>>,
    delivered: Option<CindyTaskAddress>,
    selected: Option<CindyTaskAddress>,
    deep_link: Option<String>,
    delivery: Option<CindyDeliveryMode>,
    run_id: Option<String>,
    queued_count: Option<i64>,
    interaction_id: Option<String>,
    interaction_type: Option<CindyInteractionType>,
    reason_code: Option<String>,
}

const CINDY_TOP_KEYS: &[&str] = &[
    "version",
    "id",
    "instanceId",
    "generation",
    "ownerEpoch",
    "status",
    "commandId",
    "duplicate",
    "rows",
    "delivered",
    "selected",
    "deepLink",
    "delivery",
    "runId",
    "queuedCount",
    "interactionId",
    "interactionType",
    "reasonCode",
];

const CINDY_REQUIRED_ROW_KEYS: &[&str] = &["address", "title", "phase", "attention", "pinned"];
const CINDY_ALLOWED_ROW_KEYS: &[&str] = &[
    "address",
    "title",
    "phase",
    "attention",
    "pinned",
    "runId",
    "unread",
    "eventWatermark",
    "queuedCount",
    "pendingInteraction",
];
const CINDY_ALLOWED_PENDING_KEYS: &[&str] =
    &["interactionId", "interactionType", "toolName", "options"];

fn validate_address_obj(v: &Value) -> bool {
    let Some(obj) = v.as_object() else {
        return false;
    };
    obj.len() == 2 && obj.contains_key("workspacePath") && obj.contains_key("sessionId")
}

fn validate_wire_json(data: &[u8]) -> Option<CindyWireReply> {
    let val: Value = serde_json::from_slice(data).ok()?;
    let obj = val.as_object()?;
    if !obj.keys().all(|k| CINDY_TOP_KEYS.contains(&k.as_str())) {
        return None;
    }
    if let Some(rows_val) = obj.get("rows") {
        if !rows_val.is_null() {
            let rows = rows_val.as_array()?;
            for row in rows {
                let row_obj = row.as_object()?;
                if !CINDY_REQUIRED_ROW_KEYS
                    .iter()
                    .all(|&req| row_obj.contains_key(req))
                {
                    return None;
                }
                if !row_obj
                    .keys()
                    .all(|k| CINDY_ALLOWED_ROW_KEYS.contains(&k.as_str()))
                {
                    return None;
                }
                if !row_obj.get("address").is_some_and(validate_address_obj) {
                    return None;
                }
                if let Some(pending) = row_obj.get("pendingInteraction") {
                    if !pending.is_null() {
                        let p_obj = pending.as_object()?;
                        if !p_obj.contains_key("interactionId")
                            || !p_obj.contains_key("interactionType")
                            || !p_obj
                                .keys()
                                .all(|k| CINDY_ALLOWED_PENDING_KEYS.contains(&k.as_str()))
                        {
                            return None;
                        }
                    }
                }
            }
        }
    }
    if let Some(sel) = obj.get("selected") {
        if !sel.is_null() && !validate_address_obj(sel) {
            return None;
        }
    }
    if let Some(deliv) = obj.get("delivered") {
        if !deliv.is_null() && !validate_address_obj(deliv) {
            return None;
        }
    }
    serde_json::from_value(val).ok()
}

pub struct CindySessionClient {
    grant_path: PathBuf,
    timeout: Duration,
    active_grant: Mutex<Option<CindyGrant>>,
}

impl Default for CindySessionClient {
    fn default() -> Self {
        let base = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        Self::new(base.join("Library/Application Support/MicroInput/cindy-grant.json"))
    }
}

struct CindyCommandDispatch<'a> {
    command_id: &'a str,
    scope: &'a CindyScope,
    target: Option<&'a CindyTaskAddress>,
    instance_id: &'a str,
    generation: u64,
    owner_epoch: &'a str,
    op: &'a str,
    text: Option<&'a str>,
    delivery_mode: Option<CindyDeliveryMode>,
    expected_run_id: Option<&'a str>,
    interaction_id: Option<&'a str>,
    interaction_type: Option<CindyInteractionType>,
    decision: Option<CindyInteractionDecision>,
    option_id: Option<&'a str>,
}

impl CindySessionClient {
    pub fn new(grant_path: impl Into<PathBuf>) -> Self {
        Self {
            grant_path: grant_path.into(),
            timeout: Duration::from_secs(2),
            active_grant: Mutex::new(None),
        }
    }

    pub fn with_timeout(grant_path: impl Into<PathBuf>, timeout: Duration) -> Self {
        Self {
            grant_path: grant_path.into(),
            timeout,
            active_grant: Mutex::new(None),
        }
    }

    fn send_wire(&self, req: &CindyWireRequest<'_>, socket_path: &str) -> Option<CindyWireReply> {
        let wire = serde_json::to_vec(req).ok()?;
        let transport = UnixNativeOwnershipTransport {
            path: PathBuf::from(socket_path),
            request_limit: 65536,
            response_limit: 1_048_576,
            timeout: self.timeout,
        };
        let raw = transport.exchange_blocking(&wire).ok()?;
        validate_wire_json(&raw)
    }

    fn list_using(&self, grant: &CindyGrant) -> Result<CindySessionSnapshot, CindyFailure> {
        let mut collected = Vec::new();
        let mut seen_addresses = HashSet::new();
        let mut selected: Option<CindyTaskAddress> = None;

        for scope in &grant.scopes {
            let req_id = Uuid::new_v4().to_string();
            let req = CindyWireRequest {
                version: 1,
                id: &req_id,
                token: &grant.token,
                instance_id: &grant.instance_id,
                generation: grant.generation,
                owner_epoch: &grant.owner_epoch,
                op: "list",
                scope,
                target: None,
                command_id: None,
                text: None,
                delivery_mode: None,
                expected_run_id: None,
                interaction_id: None,
                interaction_type: None,
                decision: None,
                option_id: None,
            };
            let Some(reply) = self.send_wire(&req, &grant.socket_path) else {
                *self.active_grant.lock().unwrap() = None;
                return Err(CindyFailure::Unavailable);
            };
            if reply.version != 1 || reply.id != req_id {
                *self.active_grant.lock().unwrap() = None;
                return Err(CindyFailure::InvalidReply);
            }
            if reply.status == "denied" {
                *self.active_grant.lock().unwrap() = None;
                return Err(CindyFailure::Denied);
            }
            if reply.status == "stale"
                || reply.instance_id != grant.instance_id
                || reply.generation != grant.generation
                || reply.owner_epoch != grant.owner_epoch
            {
                *self.active_grant.lock().unwrap() = None;
                return Err(CindyFailure::Stale);
            }
            if reply.status == "unavailable" {
                *self.active_grant.lock().unwrap() = None;
                return Err(CindyFailure::Unavailable);
            }
            let Some(rows) = reply.rows.filter(|_| reply.status == "ok") else {
                *self.active_grant.lock().unwrap() = None;
                return Err(CindyFailure::InvalidReply);
            };
            for row in rows {
                if !row.is_valid()
                    || row.address.workspace_path != scope.workspace_path
                    || !seen_addresses.insert(row.address.clone())
                {
                    *self.active_grant.lock().unwrap() = None;
                    return Err(CindyFailure::InvalidReply);
                }
                collected.push(row);
            }
            if let Some(reply_sel) = reply.selected {
                if !reply_sel.is_valid() || reply_sel.workspace_path != scope.workspace_path {
                    *self.active_grant.lock().unwrap() = None;
                    return Err(CindyFailure::InvalidReply);
                }
                selected = Some(reply_sel);
            }
        }

        if let Some(ref sel) = selected {
            if !seen_addresses.contains(sel) {
                *self.active_grant.lock().unwrap() = None;
                return Err(CindyFailure::InvalidReply);
            }
        }

        Ok(CindySessionSnapshot {
            instance_id: grant.instance_id.clone(),
            generation: grant.generation,
            owner_epoch: grant.owner_epoch.clone(),
            scopes: grant.scopes.clone(),
            rows: collected,
            selected,
        })
    }

    fn unavailable_open(selection: &CindySelection, status: CindyOpenResult) -> CindyOpenReply {
        CindyOpenReply {
            instance_id: selection.instance_id.clone(),
            generation: selection.generation,
            owner_epoch: selection.owner_epoch.clone(),
            status,
            delivered: None,
            selected: None,
            deep_link: None,
        }
    }

    fn perform_command(&self, cmd: CindyCommandDispatch<'_>) -> CindyCommandReply {
        let CindyCommandDispatch {
            command_id,
            scope,
            target,
            instance_id,
            generation,
            owner_epoch,
            op,
            text,
            delivery_mode,
            expected_run_id,
            interaction_id,
            interaction_type,
            decision,
            option_id,
        } = cmd;
        let fallback = |status: CindyCommandResult, reason: Option<&str>| {
            CindyCommandReply::simple(
                command_id,
                instance_id,
                generation,
                owner_epoch,
                status,
                reason,
            )
        };
        let Some(grant) = self.active_grant.lock().unwrap().clone() else {
            return fallback(CindyCommandResult::Unavailable, Some("bridge.notConnected"));
        };
        let Ok(current_grant) = CindyGrant::load(&self.grant_path) else {
            *self.active_grant.lock().unwrap() = None;
            return fallback(CindyCommandResult::Denied, Some("guard.grantMismatch"));
        };
        if current_grant.token != grant.token
            || current_grant.socket_path != grant.socket_path
            || current_grant.scopes != grant.scopes
        {
            *self.active_grant.lock().unwrap() = None;
            return fallback(CindyCommandResult::Denied, Some("guard.grantMismatch"));
        }
        if instance_id != grant.instance_id
            || generation != grant.generation
            || owner_epoch != grant.owner_epoch
            || current_grant.instance_id != grant.instance_id
            || current_grant.generation != grant.generation
            || current_grant.owner_epoch != grant.owner_epoch
        {
            *self.active_grant.lock().unwrap() = None;
            return CindyCommandReply::simple(
                command_id,
                current_grant.instance_id,
                current_grant.generation,
                current_grant.owner_epoch,
                CindyCommandResult::Stale,
                Some("guard.epochRotated"),
            );
        }
        if !grant.scopes.contains(scope) || !scope.is_valid() {
            *self.active_grant.lock().unwrap() = None;
            return fallback(CindyCommandResult::Denied, Some("guard.scopeOutOfBounds"));
        }
        if let Some(t) = target {
            if !t.is_valid() || t.workspace_path != scope.workspace_path {
                *self.active_grant.lock().unwrap() = None;
                return fallback(
                    CindyCommandResult::Denied,
                    Some("guard.targetScopeMismatch"),
                );
            }
        }
        let req_id = Uuid::new_v4().to_string();
        let req = CindyWireRequest {
            version: 1,
            id: &req_id,
            token: &grant.token,
            instance_id: &grant.instance_id,
            generation: grant.generation,
            owner_epoch: &grant.owner_epoch,
            op,
            scope,
            target,
            command_id: Some(command_id),
            text,
            delivery_mode,
            expected_run_id,
            interaction_id,
            interaction_type,
            decision,
            option_id,
        };
        let Some(reply) = self.send_wire(&req, &grant.socket_path) else {
            if op == "query" {
                return fallback(
                    CindyCommandResult::Unavailable,
                    Some("bridge.transportFailed"),
                );
            }
            return fallback(CindyCommandResult::Unknown, Some("bridge.transportTimeout"));
        };
        if reply.version != 1 || reply.id != req_id {
            *self.active_grant.lock().unwrap() = None;
            return fallback(CindyCommandResult::Unavailable, Some("bridge.invalidReply"));
        }
        if reply.status == "denied" {
            return fallback(CindyCommandResult::Denied, reply.reason_code.as_deref());
        }
        if reply.status == "stale"
            || reply.instance_id != grant.instance_id
            || reply.generation != grant.generation
            || reply.owner_epoch != grant.owner_epoch
        {
            *self.active_grant.lock().unwrap() = None;
            return CindyCommandReply::simple(
                reply.command_id.as_deref().unwrap_or(command_id),
                reply.instance_id,
                reply.generation,
                reply.owner_epoch,
                CindyCommandResult::Stale,
                reply.reason_code.as_deref(),
            );
        }
        let Ok(status) = serde_json::from_value::<CindyCommandResult>(Value::String(reply.status))
        else {
            *self.active_grant.lock().unwrap() = None;
            return fallback(
                CindyCommandResult::Unavailable,
                Some("bridge.unknownStatus"),
            );
        };
        if let Some(ref sel) = reply.selected {
            if !grant.scopes.contains(&CindyScope::new(&sel.workspace_path)) {
                *self.active_grant.lock().unwrap() = None;
                return fallback(CindyCommandResult::Denied, Some("guard.selectedOutOfScope"));
            }
        }
        if let Some(ref deliv) = reply.delivered {
            if !grant
                .scopes
                .contains(&CindyScope::new(&deliv.workspace_path))
            {
                *self.active_grant.lock().unwrap() = None;
                return fallback(
                    CindyCommandResult::Denied,
                    Some("guard.deliveredOutOfScope"),
                );
            }
        }
        CindyCommandReply {
            command_id: reply.command_id.unwrap_or_else(|| command_id.to_owned()),
            instance_id: reply.instance_id,
            generation: reply.generation,
            owner_epoch: reply.owner_epoch,
            status,
            duplicate: reply.duplicate.unwrap_or(false),
            delivered: reply.delivered,
            selected: reply.selected,
            deep_link: reply.deep_link,
            delivery: reply.delivery,
            run_id: reply.run_id,
            queued_count: reply.queued_count.unwrap_or(0),
            interaction_id: reply.interaction_id,
            interaction_type: reply.interaction_type,
            reason_code: reply.reason_code,
        }
    }
}

impl CindySessionServing for CindySessionClient {
    fn connect(&self) -> Result<CindySessionSnapshot, CindyFailure> {
        let grant = match CindyGrant::load(&self.grant_path) {
            Ok(g) => g,
            Err(e) => {
                *self.active_grant.lock().unwrap() = None;
                return Err(e);
            }
        };
        *self.active_grant.lock().unwrap() = Some(grant.clone());
        self.list_using(&grant)
    }

    fn read(&self) -> Result<CindySessionSnapshot, CindyFailure> {
        let Some(grant) = self.active_grant.lock().unwrap().clone() else {
            return self.connect();
        };
        let Ok(current_grant) = CindyGrant::load(&self.grant_path) else {
            *self.active_grant.lock().unwrap() = None;
            return Err(CindyFailure::Grant);
        };
        if current_grant.token != grant.token
            || current_grant.socket_path != grant.socket_path
            || current_grant.scopes != grant.scopes
        {
            *self.active_grant.lock().unwrap() = None;
            return Err(CindyFailure::Grant);
        }
        if current_grant.instance_id != grant.instance_id
            || current_grant.generation != grant.generation
            || current_grant.owner_epoch != grant.owner_epoch
        {
            *self.active_grant.lock().unwrap() = None;
            return Err(CindyFailure::Stale);
        }
        self.list_using(&grant)
    }

    fn open(&self, selection: &CindySelection) -> CindyOpenReply {
        let Some(grant) = self.active_grant.lock().unwrap().clone() else {
            return Self::unavailable_open(selection, CindyOpenResult::Unavailable);
        };
        let Ok(current_grant) = CindyGrant::load(&self.grant_path) else {
            *self.active_grant.lock().unwrap() = None;
            return Self::unavailable_open(selection, CindyOpenResult::Denied);
        };
        if current_grant.token != grant.token
            || current_grant.socket_path != grant.socket_path
            || current_grant.scopes != grant.scopes
        {
            *self.active_grant.lock().unwrap() = None;
            return Self::unavailable_open(selection, CindyOpenResult::Denied);
        }
        if selection.instance_id != grant.instance_id
            || selection.generation != grant.generation
            || selection.owner_epoch != grant.owner_epoch
            || current_grant.instance_id != grant.instance_id
            || current_grant.generation != grant.generation
            || current_grant.owner_epoch != grant.owner_epoch
        {
            *self.active_grant.lock().unwrap() = None;
            return CindyOpenReply {
                instance_id: current_grant.instance_id,
                generation: current_grant.generation,
                owner_epoch: current_grant.owner_epoch,
                status: CindyOpenResult::Stale,
                delivered: None,
                selected: None,
                deep_link: None,
            };
        }
        let target_scope = CindyScope::new(&selection.address.workspace_path);
        if !grant.scopes.contains(&target_scope) || !selection.address.is_valid() {
            *self.active_grant.lock().unwrap() = None;
            return Self::unavailable_open(selection, CindyOpenResult::Denied);
        }
        let req_id = Uuid::new_v4().to_string();
        let req = CindyWireRequest {
            version: 1,
            id: &req_id,
            token: &grant.token,
            instance_id: &grant.instance_id,
            generation: grant.generation,
            owner_epoch: &grant.owner_epoch,
            op: "open",
            scope: &target_scope,
            target: Some(&selection.address),
            command_id: None,
            text: None,
            delivery_mode: None,
            expected_run_id: None,
            interaction_id: None,
            interaction_type: None,
            decision: None,
            option_id: None,
        };
        let Some(reply) = self.send_wire(&req, &grant.socket_path) else {
            *self.active_grant.lock().unwrap() = None;
            return Self::unavailable_open(selection, CindyOpenResult::Unavailable);
        };
        if reply.version != 1 || reply.id != req_id {
            *self.active_grant.lock().unwrap() = None;
            return Self::unavailable_open(selection, CindyOpenResult::Unavailable);
        }
        if reply.status == "denied" {
            *self.active_grant.lock().unwrap() = None;
            return Self::unavailable_open(selection, CindyOpenResult::Denied);
        }
        if reply.status == "stale"
            || reply.instance_id != grant.instance_id
            || reply.generation != grant.generation
            || reply.owner_epoch != grant.owner_epoch
        {
            *self.active_grant.lock().unwrap() = None;
            return CindyOpenReply {
                instance_id: reply.instance_id,
                generation: reply.generation,
                owner_epoch: reply.owner_epoch,
                status: CindyOpenResult::Stale,
                delivered: None,
                selected: None,
                deep_link: None,
            };
        }
        let Ok(status) = serde_json::from_value::<CindyOpenResult>(Value::String(reply.status))
        else {
            *self.active_grant.lock().unwrap() = None;
            return Self::unavailable_open(selection, CindyOpenResult::Unavailable);
        };
        if let Some(ref sel) = reply.selected {
            if !grant.scopes.contains(&CindyScope::new(&sel.workspace_path)) {
                *self.active_grant.lock().unwrap() = None;
                return Self::unavailable_open(selection, CindyOpenResult::Denied);
            }
        }
        if let Some(ref deliv) = reply.delivered {
            if !grant
                .scopes
                .contains(&CindyScope::new(&deliv.workspace_path))
            {
                *self.active_grant.lock().unwrap() = None;
                return Self::unavailable_open(selection, CindyOpenResult::Denied);
            }
        }
        let expected_link = selection.address.deep_link();
        let resolved_status = match status {
            CindyOpenResult::Confirmed => {
                if reply.delivered.as_ref() == Some(&selection.address)
                    && reply.selected.as_ref() == Some(&selection.address)
                    && reply.deep_link.as_deref() == Some(expected_link.as_str())
                {
                    CindyOpenResult::Confirmed
                } else {
                    CindyOpenResult::Unconfirmed
                }
            }
            CindyOpenResult::Delivered => {
                if reply.delivered.as_ref() == Some(&selection.address)
                    && reply.selected.is_none()
                    && reply.deep_link.as_deref() == Some(expected_link.as_str())
                {
                    CindyOpenResult::Delivered
                } else {
                    CindyOpenResult::Unconfirmed
                }
            }
            other => other,
        };
        CindyOpenReply {
            instance_id: reply.instance_id,
            generation: reply.generation,
            owner_epoch: reply.owner_epoch,
            status: resolved_status,
            delivered: reply.delivered,
            selected: reply.selected,
            deep_link: reply.deep_link,
        }
    }

    fn create(&self, intent: &CindyCreateIntent) -> CindyCommandReply {
        self.perform_command(CindyCommandDispatch {
            command_id: &intent.command_id,
            scope: &intent.scope,
            target: None,
            instance_id: &intent.instance_id,
            generation: intent.generation,
            owner_epoch: &intent.owner_epoch,
            op: "create",
            text: intent.text.as_deref(),
            delivery_mode: None,
            expected_run_id: None,
            interaction_id: None,
            interaction_type: None,
            decision: None,
            option_id: None,
        })
    }

    fn send(&self, intent: &CindySendIntent) -> CindyCommandReply {
        let scope = CindyScope::new(&intent.selection.address.workspace_path);
        self.perform_command(CindyCommandDispatch {
            command_id: &intent.command_id,
            scope: &scope,
            target: Some(&intent.selection.address),
            instance_id: &intent.selection.instance_id,
            generation: intent.selection.generation,
            owner_epoch: &intent.selection.owner_epoch,
            op: "send",
            text: Some(&intent.text),
            delivery_mode: Some(intent.delivery_mode),
            expected_run_id: None,
            interaction_id: None,
            interaction_type: None,
            decision: None,
            option_id: None,
        })
    }

    fn stop(&self, intent: &CindyStopIntent) -> CindyCommandReply {
        let scope = CindyScope::new(&intent.selection.address.workspace_path);
        self.perform_command(CindyCommandDispatch {
            command_id: &intent.command_id,
            scope: &scope,
            target: Some(&intent.selection.address),
            instance_id: &intent.selection.instance_id,
            generation: intent.selection.generation,
            owner_epoch: &intent.selection.owner_epoch,
            op: "stop",
            text: None,
            delivery_mode: None,
            expected_run_id: Some(&intent.expected_run_id),
            interaction_id: None,
            interaction_type: None,
            decision: None,
            option_id: None,
        })
    }

    fn resolve(&self, intent: &CindyResolveIntent) -> CindyCommandReply {
        let scope = CindyScope::new(&intent.selection.address.workspace_path);
        self.perform_command(CindyCommandDispatch {
            command_id: &intent.command_id,
            scope: &scope,
            target: Some(&intent.selection.address),
            instance_id: &intent.selection.instance_id,
            generation: intent.selection.generation,
            owner_epoch: &intent.selection.owner_epoch,
            op: "resolve",
            text: None,
            delivery_mode: None,
            expected_run_id: None,
            interaction_id: Some(&intent.interaction_id),
            interaction_type: Some(intent.interaction_type),
            decision: Some(intent.decision),
            option_id: intent.option_id.as_deref(),
        })
    }

    fn query(&self, intent: &CindyQueryIntent) -> CindyCommandReply {
        self.perform_command(CindyCommandDispatch {
            command_id: &intent.command_id,
            scope: &intent.scope,
            target: intent.target.as_ref(),
            instance_id: &intent.instance_id,
            generation: intent.generation,
            owner_epoch: &intent.owner_epoch,
            op: "query",
            text: None,
            delivery_mode: None,
            expected_run_id: None,
            interaction_id: None,
            interaction_type: None,
            decision: None,
            option_id: None,
        })
    }

    fn close(&self) {
        *self.active_grant.lock().unwrap() = None;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CindyAuditReceipt {
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

pub struct CindySessionController<S: CindySessionServing> {
    pub client: S,
    pub enabled: bool,
    pub suspended: bool,
    pub state: CindyConnectionState,
    pub instance_id: Option<String>,
    pub generation: Option<u64>,
    pub owner_epoch: Option<String>,
    pub scopes: Vec<CindyScope>,
    pub rows: Vec<CindyTaskRow>,
    pub delivered: Option<CindyTaskAddress>,
    pub selected: Option<CindyTaskAddress>,
    pub last_deep_link: Option<String>,
    pub last_result: Option<CindyOpenResult>,
    pub last_command_reply: Option<CindyCommandReply>,
    pub pending_unknown_command: Option<CindyQueryIntent>,
    pub attention_round: AttentionRound,
    pub attention_round_visited_count: usize,
    pub voice_reply_enabled: bool,
    pub navigation_history: NavigationHistory,
    pub audit_receipts: Vec<HostActionReceipt>,
    local_epoch: u64,
    latched_commands_by_scope: HashMap<CindyScope, CindyQueryIntent>,
    completed_command_replies: HashMap<String, CindyCommandReply>,
}

impl<S: CindySessionServing> CindySessionController<S> {
    pub fn new(client: S) -> Self {
        Self {
            client,
            enabled: false,
            suspended: false,
            state: CindyConnectionState::Disabled,
            instance_id: None,
            generation: None,
            owner_epoch: None,
            scopes: Vec::new(),
            rows: Vec::new(),
            delivered: None,
            selected: None,
            last_deep_link: None,
            last_result: None,
            last_command_reply: None,
            pending_unknown_command: None,
            attention_round: AttentionRound::new(),
            attention_round_visited_count: 0,
            voice_reply_enabled: false,
            navigation_history: NavigationHistory::new(),
            audit_receipts: Vec::new(),
            local_epoch: 0,
            latched_commands_by_scope: HashMap::new(),
            completed_command_replies: HashMap::new(),
        }
    }

    pub fn set_voice_reply(&mut self, enabled: bool) -> (bool, &'static str) {
        self.voice_reply_enabled = enabled;
        (
            true,
            if enabled {
                "已开启 Cindy 原生语音对话回复"
            } else {
                "已关闭 Cindy 原生语音对话回复"
            },
        )
    }

    pub fn set_snapshot_for_testing(
        &mut self,
        rows: Vec<CindyTaskRow>,
        selected: Option<CindyTaskAddress>,
    ) {
        self.rows = rows;
        self.selected = selected;
    }

    pub fn set_enabled(&mut self, value: bool) {
        if self.enabled == value {
            if value
                && matches!(self.state, CindyConnectionState::Unavailable(_))
                && !self.suspended
            {
                self.local_epoch = self.local_epoch.wrapping_add(1);
                self.state = CindyConnectionState::Connecting;
            }
            return;
        }
        self.enabled = value;
        self.local_epoch = self.local_epoch.wrapping_add(1);
        if value {
            if !self.suspended {
                self.state = CindyConnectionState::Connecting;
            }
        } else {
            self.stop_and_clear(CindyConnectionState::Disabled);
        }
    }

    pub fn suspend(&mut self) {
        if self.suspended {
            return;
        }
        self.suspended = true;
        self.local_epoch = self.local_epoch.wrapping_add(1);
        self.client.close();
    }

    pub fn resume(&mut self) {
        if !self.suspended {
            return;
        }
        self.suspended = false;
        if self.enabled {
            self.local_epoch = self.local_epoch.wrapping_add(1);
            self.state = CindyConnectionState::Connecting;
        }
    }

    pub fn shutdown(&mut self) {
        self.suspended = true;
        self.enabled = false;
        self.stop_and_clear(CindyConnectionState::Disabled);
    }

    pub fn reset_attention_round(&mut self) {
        self.attention_round.reset();
        self.attention_round_visited_count = 0;
    }

    fn scoped_attention_candidates(&self) -> Vec<(CindyTaskRow, AttentionCandidate)> {
        if !self.enabled || self.suspended || self.state != CindyConnectionState::Ready {
            return Vec::new();
        }
        self.rows
            .iter()
            .filter(|row| {
                self.scopes
                    .contains(&CindyScope::new(&row.address.workspace_path))
                    && row.is_eligible_for_attention_round()
            })
            .cloned()
            .map(|r| {
                let c = r.to_attention_candidate();
                (r, c)
            })
            .collect()
    }

    pub fn next_attention_candidate(&self, allow_wrap: bool) -> Option<CindyTaskRow> {
        let pairs = self.scoped_attention_candidates();
        if pairs.is_empty() {
            return None;
        }
        let candidates: Vec<AttentionCandidate> = pairs.iter().map(|(_, c)| c.clone()).collect();
        let mut preview = self.attention_round.clone();
        let picked = preview.next_candidate(&candidates, allow_wrap)?;
        pairs
            .into_iter()
            .find(|(_, c)| c.place == picked.place)
            .map(|(r, _)| r)
    }

    pub fn advance_attention_round(
        &mut self,
        allow_wrap: bool,
        admitted: bool,
    ) -> CindyCommandReply {
        let cmd_id = format!("round-{}", Uuid::new_v4());
        let (Some(inst), Some(gen), Some(owner)) = (
            self.instance_id.clone(),
            self.generation,
            self.owner_epoch.clone(),
        ) else {
            let reply = CindyCommandReply::simple(
                cmd_id,
                self.instance_id.clone().unwrap_or_default(),
                self.generation.unwrap_or(0),
                self.owner_epoch.clone().unwrap_or_default(),
                CindyCommandResult::Stale,
                Some("round.notReady"),
            );
            self.last_command_reply = Some(reply.clone());
            return reply;
        };
        if !self.enabled || self.suspended || self.state != CindyConnectionState::Ready {
            let reply = CindyCommandReply::simple(
                cmd_id,
                inst,
                gen,
                owner,
                CindyCommandResult::Stale,
                Some("round.notReady"),
            );
            self.last_command_reply = Some(reply.clone());
            return reply;
        }
        if !admitted {
            let reply = CindyCommandReply::simple(
                cmd_id,
                inst,
                gen,
                owner,
                CindyCommandResult::Stale,
                Some("guard.notAdmitted"),
            );
            self.last_command_reply = Some(reply.clone());
            return reply;
        }
        let pairs = self.scoped_attention_candidates();
        let candidates: Vec<AttentionCandidate> = pairs.iter().map(|(_, c)| c.clone()).collect();
        let mut preview = self.attention_round.clone();
        let Some(picked_cand) = preview.next_candidate(&candidates, allow_wrap) else {
            let mut reply = CindyCommandReply::simple(
                cmd_id,
                inst,
                gen,
                owner,
                CindyCommandResult::Noop,
                Some("round.exhausted"),
            );
            reply.selected = self.selected.clone();
            self.last_command_reply = Some(reply.clone());
            return reply;
        };
        let candidate_row = pairs
            .into_iter()
            .find(|(_, c)| c.place == picked_cand.place)
            .map(|(r, _)| r)
            .unwrap();
        let open_res = self.open_selection(
            &CindySelection {
                address: candidate_row.address.clone(),
                instance_id: inst.clone(),
                generation: gen,
                owner_epoch: owner.clone(),
                local_epoch: self.local_epoch,
            },
            admitted,
        );
        let mapped_status = match open_res {
            CindyOpenResult::Confirmed => {
                self.attention_round = preview;
                self.attention_round_visited_count = self.attention_round.visited_count();
                CindyCommandResult::Confirmed
            }
            CindyOpenResult::Delivered => {
                self.attention_round = preview;
                self.attention_round_visited_count = self.attention_round.visited_count();
                CindyCommandResult::Delivered
            }
            CindyOpenResult::Unconfirmed => CindyCommandResult::Unconfirmed,
            CindyOpenResult::Stale => CindyCommandResult::Stale,
            CindyOpenResult::Denied => CindyCommandResult::Denied,
            CindyOpenResult::Unavailable => CindyCommandResult::Unavailable,
        };
        let reply = CindyCommandReply {
            command_id: cmd_id,
            instance_id: self.instance_id.clone().unwrap_or(inst),
            generation: self.generation.unwrap_or(gen),
            owner_epoch: self.owner_epoch.clone().unwrap_or(owner),
            status: mapped_status,
            duplicate: false,
            delivered: self.delivered.clone(),
            selected: self.selected.clone(),
            deep_link: self.last_deep_link.clone(),
            delivery: None,
            run_id: None,
            queued_count: 0,
            interaction_id: None,
            interaction_type: None,
            reason_code: None,
        };
        self.last_command_reply = Some(reply.clone());
        reply
    }

    pub fn execute(&mut self, command: &CindyQueuedCommand, admitted: bool) -> CindyCommandReply {
        let reply = match command {
            CindyQueuedCommand::Create {
                scope,
                text,
                command_id,
            } => {
                let cid = command_id
                    .clone()
                    .unwrap_or_else(|| Uuid::new_v4().to_string());
                self.create(scope, text.as_deref(), &cid, admitted)
            }
            CindyQueuedCommand::Send {
                address,
                text,
                delivery_mode,
                command_id,
            } => {
                let cid = command_id
                    .clone()
                    .unwrap_or_else(|| Uuid::new_v4().to_string());
                self.send(address, text, *delivery_mode, &cid, admitted)
            }
            CindyQueuedCommand::Stop {
                address,
                expected_run_id,
                command_id,
            } => {
                let cid = command_id
                    .clone()
                    .unwrap_or_else(|| Uuid::new_v4().to_string());
                self.stop(address, expected_run_id, &cid, admitted)
            }
            CindyQueuedCommand::Resolve {
                address,
                interaction_id,
                interaction_type,
                decision,
                option_id,
                command_id,
            } => {
                let cid = command_id
                    .clone()
                    .unwrap_or_else(|| Uuid::new_v4().to_string());
                self.resolve(
                    address,
                    (interaction_id, *interaction_type),
                    *decision,
                    option_id.as_deref(),
                    &cid,
                    admitted,
                )
            }
            CindyQueuedCommand::Query {
                scope,
                target,
                command_id,
            } => self.query(scope, target.as_ref(), command_id, admitted),
            CindyQueuedCommand::NextAttention => self.advance_attention_round(false, admitted),
        };
        self.record_host_receipt(
            command.action_name(),
            reply.status.as_str(),
            command.label(),
        );
        reply
    }

    pub fn create(
        &mut self,
        scope: &CindyScope,
        text: Option<&str>,
        command_id: &str,
        admitted: bool,
    ) -> CindyCommandReply {
        let (Some(inst), Some(gen), Some(owner)) = (
            self.instance_id.clone(),
            self.generation,
            self.owner_epoch.clone(),
        ) else {
            return self.record_reply(CindyCommandReply::simple(
                command_id,
                self.instance_id.clone().unwrap_or_default(),
                self.generation.unwrap_or(0),
                self.owner_epoch.clone().unwrap_or_default(),
                CindyCommandResult::Stale,
                Some("guard.notReady"),
            ));
        };
        if !self.enabled || self.suspended || self.state != CindyConnectionState::Ready || !admitted
        {
            return self.record_reply(CindyCommandReply::simple(
                command_id,
                inst,
                gen,
                owner,
                CindyCommandResult::Stale,
                Some("guard.notReady"),
            ));
        }
        if !self.scopes.contains(scope) || !scope.is_valid() {
            return self.record_reply(CindyCommandReply::simple(
                command_id,
                inst,
                gen,
                owner,
                CindyCommandResult::Denied,
                Some("guard.scopeOutOfBounds"),
            ));
        }
        let cmd_key = format!("{}::{command_id}", scope.workspace_path);
        if let Some(cached) = self.completed_command_replies.get(&cmd_key).cloned() {
            let mut dup = cached;
            dup.duplicate = true;
            self.last_command_reply = Some(dup.clone());
            return dup;
        }
        if let Some(latched) = self.latched_commands_by_scope.get(scope) {
            if latched.command_id != command_id {
                return self.record_reply(CindyCommandReply::simple(
                    latched.command_id.clone(),
                    inst,
                    gen,
                    owner,
                    CindyCommandResult::Unknown,
                    Some("guard.unknownOutcomeRequiresReconcile"),
                ));
            }
        }
        let epoch = self.local_epoch;
        let intent = CindyCreateIntent {
            command_id: command_id.to_owned(),
            scope: scope.clone(),
            text: text.map(String::from),
            instance_id: inst.clone(),
            generation: gen,
            owner_epoch: owner.clone(),
            local_epoch: epoch,
        };
        let response = self.client.create(&intent);
        if !self.enabled || self.suspended || epoch != self.local_epoch || !admitted {
            return self.record_reply(CindyCommandReply::simple(
                command_id,
                inst,
                gen,
                owner,
                CindyCommandResult::Stale,
                Some("guard.epochChanged"),
            ));
        }
        let target = response
            .selected
            .clone()
            .or_else(|| response.delivered.clone());
        self.handle_command_response(response, scope, target.as_ref(), &cmd_key)
    }

    pub fn send(
        &mut self,
        address: &CindyTaskAddress,
        text: &str,
        delivery_mode: CindyDeliveryMode,
        command_id: &str,
        admitted: bool,
    ) -> CindyCommandReply {
        let scope = CindyScope::new(&address.workspace_path);
        let cmd_key = format!("{}::{command_id}", scope.workspace_path);
        if let Some(cached) = self.completed_command_replies.get(&cmd_key).cloned() {
            let mut dup = cached;
            dup.duplicate = true;
            self.last_command_reply = Some(dup.clone());
            return dup;
        }
        let Some((selection, row)) =
            self.validate_target_command(address, &scope, command_id, admitted)
        else {
            return self.last_command_reply.clone().unwrap();
        };
        if delivery_mode == CindyDeliveryMode::Steer && row.phase != "running" {
            let mut reply = CindyCommandReply::simple(
                command_id,
                selection.instance_id,
                selection.generation,
                selection.owner_epoch,
                CindyCommandResult::Denied,
                Some("guard.steerRequiresRunningTurn"),
            );
            reply.selected = self.selected.clone();
            return self.record_reply(reply);
        }
        let epoch = self.local_epoch;
        let intent = CindySendIntent {
            command_id: command_id.to_owned(),
            selection: selection.clone(),
            text: text.to_owned(),
            delivery_mode,
        };
        let response = self.client.send(&intent);
        if !self.enabled || self.suspended || epoch != self.local_epoch || !admitted {
            return self.record_reply(CindyCommandReply::simple(
                command_id,
                selection.instance_id,
                selection.generation,
                selection.owner_epoch,
                CindyCommandResult::Stale,
                Some("guard.epochChanged"),
            ));
        }
        self.handle_command_response(response, &scope, Some(address), &cmd_key)
    }

    pub fn stop(
        &mut self,
        address: &CindyTaskAddress,
        expected_run_id: &str,
        command_id: &str,
        admitted: bool,
    ) -> CindyCommandReply {
        let scope = CindyScope::new(&address.workspace_path);
        let cmd_key = format!("{}::{command_id}", scope.workspace_path);
        if let Some(cached) = self.completed_command_replies.get(&cmd_key).cloned() {
            let mut dup = cached;
            dup.duplicate = true;
            self.last_command_reply = Some(dup.clone());
            return dup;
        }
        let Some((selection, row)) =
            self.validate_target_command(address, &scope, command_id, admitted)
        else {
            return self.last_command_reply.clone().unwrap();
        };
        if row.phase != "running" || row.run_id.as_deref().is_some_and(|r| r != expected_run_id) {
            let rc = if row.phase != "running" {
                "guard.notRunning"
            } else {
                "guard.stopTargetChanged"
            };
            let mut noop = CindyCommandReply::simple(
                command_id,
                selection.instance_id,
                selection.generation,
                selection.owner_epoch,
                CindyCommandResult::Noop,
                Some(rc),
            );
            noop.selected = Some(address.clone());
            noop.run_id = row.run_id;
            self.completed_command_replies.insert(cmd_key, noop.clone());
            return self.record_reply(noop);
        }
        let epoch = self.local_epoch;
        let intent = CindyStopIntent {
            command_id: command_id.to_owned(),
            selection: selection.clone(),
            expected_run_id: expected_run_id.to_owned(),
        };
        let response = self.client.stop(&intent);
        if !self.enabled || self.suspended || epoch != self.local_epoch || !admitted {
            return self.record_reply(CindyCommandReply::simple(
                command_id,
                selection.instance_id,
                selection.generation,
                selection.owner_epoch,
                CindyCommandResult::Stale,
                Some("guard.epochChanged"),
            ));
        }
        self.handle_command_response(response, &scope, Some(address), &cmd_key)
    }

    pub fn resolve(
        &mut self,
        address: &CindyTaskAddress,
        interaction: (&str, CindyInteractionType),
        decision: CindyInteractionDecision,
        option_id: Option<&str>,
        command_id: &str,
        admitted: bool,
    ) -> CindyCommandReply {
        let (interaction_id, interaction_type) = interaction;
        let scope = CindyScope::new(&address.workspace_path);
        let cmd_key = format!("{}::{command_id}", scope.workspace_path);
        if let Some(cached) = self.completed_command_replies.get(&cmd_key).cloned() {
            let mut dup = cached;
            dup.duplicate = true;
            self.last_command_reply = Some(dup.clone());
            return dup;
        }
        if interaction_type == CindyInteractionType::Question
            && matches!(
                decision,
                CindyInteractionDecision::Approve | CindyInteractionDecision::AllowForSession
            )
            && option_id.is_none_or(str::is_empty)
        {
            let mut mismatch = CindyCommandReply::simple(
                command_id,
                self.instance_id.clone().unwrap_or_default(),
                self.generation.unwrap_or(0),
                self.owner_epoch.clone().unwrap_or_default(),
                CindyCommandResult::InteractionTypeMismatch,
                Some("guard.questionCannotApproveAsPermission"),
            );
            mismatch.selected = self.selected.clone();
            mismatch.interaction_id = Some(interaction_id.to_owned());
            mismatch.interaction_type = Some(interaction_type);
            return self.record_reply(mismatch);
        }
        let Some((selection, row)) =
            self.validate_target_command(address, &scope, command_id, admitted)
        else {
            return self.last_command_reply.clone().unwrap();
        };
        if let Some(ref pending) = row.pending_interaction {
            if pending.interaction_id != interaction_id {
                let mut noop = CindyCommandReply::simple(
                    command_id,
                    selection.instance_id,
                    selection.generation,
                    selection.owner_epoch,
                    CindyCommandResult::Noop,
                    Some("guard.interactionAlreadyResolved"),
                );
                noop.selected = Some(address.clone());
                noop.interaction_id = Some(interaction_id.to_owned());
                noop.interaction_type = Some(interaction_type);
                self.completed_command_replies.insert(cmd_key, noop.clone());
                return self.record_reply(noop);
            }
            if pending.interaction_type != interaction_type {
                let mut mismatch = CindyCommandReply::simple(
                    command_id,
                    selection.instance_id,
                    selection.generation,
                    selection.owner_epoch,
                    CindyCommandResult::InteractionTypeMismatch,
                    Some("guard.interactionTypeMismatch"),
                );
                mismatch.selected = Some(address.clone());
                mismatch.interaction_id = Some(pending.interaction_id.clone());
                mismatch.interaction_type = Some(pending.interaction_type);
                return self.record_reply(mismatch);
            }
        }
        let epoch = self.local_epoch;
        let intent = CindyResolveIntent {
            command_id: command_id.to_owned(),
            selection: selection.clone(),
            interaction_id: interaction_id.to_owned(),
            interaction_type,
            decision,
            option_id: option_id.map(String::from),
        };
        let response = self.client.resolve(&intent);
        if !self.enabled || self.suspended || epoch != self.local_epoch || !admitted {
            return self.record_reply(CindyCommandReply::simple(
                command_id,
                selection.instance_id,
                selection.generation,
                selection.owner_epoch,
                CindyCommandResult::Stale,
                Some("guard.epochChanged"),
            ));
        }
        self.handle_command_response(response, &scope, Some(address), &cmd_key)
    }

    pub fn query(
        &mut self,
        scope: &CindyScope,
        target: Option<&CindyTaskAddress>,
        command_id: &str,
        admitted: bool,
    ) -> CindyCommandReply {
        let (Some(inst), Some(gen), Some(owner)) = (
            self.instance_id.clone(),
            self.generation,
            self.owner_epoch.clone(),
        ) else {
            return self.record_reply(CindyCommandReply::simple(
                command_id,
                self.instance_id.clone().unwrap_or_default(),
                self.generation.unwrap_or(0),
                self.owner_epoch.clone().unwrap_or_default(),
                CindyCommandResult::Stale,
                Some("guard.notReady"),
            ));
        };
        if !self.enabled || self.suspended || self.state != CindyConnectionState::Ready || !admitted
        {
            return self.record_reply(CindyCommandReply::simple(
                command_id,
                inst,
                gen,
                owner,
                CindyCommandResult::Stale,
                Some("guard.notReady"),
            ));
        }
        let epoch = self.local_epoch;
        let intent = CindyQueryIntent {
            command_id: command_id.to_owned(),
            scope: scope.clone(),
            target: target.cloned(),
            instance_id: inst.clone(),
            generation: gen,
            owner_epoch: owner.clone(),
            local_epoch: epoch,
        };
        let response = self.client.query(&intent);
        if !self.enabled || self.suspended || epoch != self.local_epoch || !admitted {
            return self.record_reply(CindyCommandReply::simple(
                command_id,
                inst,
                gen,
                owner,
                CindyCommandResult::Stale,
                Some("guard.epochChanged"),
            ));
        }
        let cmd_key = format!("{}::{command_id}", scope.workspace_path);
        self.handle_command_response(response, scope, target, &cmd_key)
    }

    fn validate_target_command(
        &mut self,
        address: &CindyTaskAddress,
        scope: &CindyScope,
        command_id: &str,
        admitted: bool,
    ) -> Option<(CindySelection, CindyTaskRow)> {
        let (Some(inst), Some(gen), Some(owner)) = (
            self.instance_id.clone(),
            self.generation,
            self.owner_epoch.clone(),
        ) else {
            self.record_reply(CindyCommandReply::simple(
                command_id,
                self.instance_id.clone().unwrap_or_default(),
                self.generation.unwrap_or(0),
                self.owner_epoch.clone().unwrap_or_default(),
                CindyCommandResult::Stale,
                Some("guard.notReady"),
            ));
            return None;
        };
        if !self.enabled || self.suspended || self.state != CindyConnectionState::Ready || !admitted
        {
            self.record_reply(CindyCommandReply::simple(
                command_id,
                inst,
                gen,
                owner,
                CindyCommandResult::Stale,
                Some("guard.notReady"),
            ));
            return None;
        }
        if !self.scopes.contains(scope) || !address.is_valid() {
            self.record_reply(CindyCommandReply::simple(
                command_id,
                inst,
                gen,
                owner,
                CindyCommandResult::Denied,
                Some("guard.scopeOutOfBounds"),
            ));
            return None;
        }
        if let Some(latched) = self.latched_commands_by_scope.get(scope) {
            if latched.command_id != command_id {
                self.record_reply(CindyCommandReply::simple(
                    latched.command_id.clone(),
                    inst,
                    gen,
                    owner,
                    CindyCommandResult::Unknown,
                    Some("guard.unknownOutcomeRequiresReconcile"),
                ));
                return None;
            }
        }
        let Some(row) = self.rows.iter().find(|r| &r.address == address).cloned() else {
            self.record_reply(CindyCommandReply::simple(
                command_id,
                inst,
                gen,
                owner,
                CindyCommandResult::Stale,
                Some("guard.targetMissing"),
            ));
            return None;
        };
        if self.selected.as_ref() != Some(address) {
            let mut unconf = CindyCommandReply::simple(
                command_id,
                inst,
                gen,
                owner,
                CindyCommandResult::Unconfirmed,
                Some("guard.selectionNotCommitted"),
            );
            unconf.selected = self.selected.clone();
            self.record_reply(unconf);
            return None;
        }
        let sel = CindySelection {
            address: address.clone(),
            instance_id: inst,
            generation: gen,
            owner_epoch: owner,
            local_epoch: self.local_epoch,
        };
        Some((sel, row))
    }

    fn handle_command_response(
        &mut self,
        response: CindyCommandReply,
        scope: &CindyScope,
        target: Option<&CindyTaskAddress>,
        command_key: &str,
    ) -> CindyCommandReply {
        if response.status == CindyCommandResult::Unknown {
            if let (Some(inst), Some(gen), Some(owner)) = (
                self.instance_id.clone(),
                self.generation,
                self.owner_epoch.clone(),
            ) {
                let query_intent = CindyQueryIntent {
                    command_id: response.command_id.clone(),
                    scope: scope.clone(),
                    target: target.cloned(),
                    instance_id: inst,
                    generation: gen,
                    owner_epoch: owner,
                    local_epoch: self.local_epoch,
                };
                self.latched_commands_by_scope
                    .insert(scope.clone(), query_intent.clone());
                self.pending_unknown_command = Some(query_intent);
            }
            return self.record_reply(response);
        }
        if self.instance_id.as_deref() != Some(response.instance_id.as_str())
            || self.generation != Some(response.generation)
            || self.owner_epoch.as_deref() != Some(response.owner_epoch.as_str())
        {
            self.invalidate(CindyFailure::Stale);
            let reason = response
                .reason_code
                .clone()
                .unwrap_or_else(|| "guard.epochRotated".to_owned());
            return self.record_reply(CindyCommandReply::simple(
                response.command_id,
                response.instance_id,
                response.generation,
                response.owner_epoch,
                CindyCommandResult::Stale,
                Some(&reason),
            ));
        }
        if self
            .latched_commands_by_scope
            .get(scope)
            .is_some_and(|l| l.command_id == response.command_id)
        {
            self.latched_commands_by_scope.remove(scope);
            self.pending_unknown_command = self.latched_commands_by_scope.values().next().cloned();
        }
        match response.status {
            CindyCommandResult::Confirmed => {
                let created_or_target = response.selected.clone().or_else(|| target.cloned());
                let Some(ct) = created_or_target else {
                    let mut unconf = response;
                    unconf.status = CindyCommandResult::Unconfirmed;
                    return self.record_reply(unconf);
                };
                if response.delivered.as_ref() != Some(&ct)
                    || response.selected.as_ref() != Some(&ct)
                {
                    let mut unconf = response;
                    unconf.status = CindyCommandResult::Unconfirmed;
                    return self.record_reply(unconf);
                }
                self.delivered = Some(ct.clone());
                self.selected = Some(ct.clone());
                self.last_deep_link =
                    Some(response.deep_link.clone().unwrap_or_else(|| ct.deep_link()));
                self.last_result = Some(CindyOpenResult::Confirmed);
                if !self.rows.iter().any(|r| r.address == ct) {
                    self.rows.push(CindyTaskRow {
                        address: ct.clone(),
                        title: ct.session_id.clone(),
                        phase: if response.run_id.is_some() {
                            "running".into()
                        } else {
                            "idle".into()
                        },
                        attention: false,
                        pinned: false,
                        run_id: response.run_id.clone(),
                        unread: Some(false),
                        event_watermark: None,
                        queued_count: 0,
                        pending_interaction: None,
                    });
                }
                self.navigation_history
                    .visit(NavigationPlace::cindy(&ct.workspace_path, &ct.session_id));
                self.completed_command_replies
                    .insert(command_key.to_owned(), response.clone());
            }
            CindyCommandResult::Delivered => {
                self.delivered = response.delivered.clone();
                self.selected = None;
                self.last_deep_link = response.deep_link.clone();
                self.last_result = Some(CindyOpenResult::Delivered);
                self.completed_command_replies
                    .insert(command_key.to_owned(), response.clone());
            }
            CindyCommandResult::Started
            | CindyCommandResult::Steered
            | CindyCommandResult::Queued => {
                if let Some(t) = target {
                    if let Some(row) = self.rows.iter_mut().find(|r| &r.address == t) {
                        row.phase = "running".into();
                        if let Some(ref rid) = response.run_id {
                            row.run_id = Some(rid.clone());
                        }
                        row.queued_count = response.queued_count;
                    }
                }
                self.completed_command_replies
                    .insert(command_key.to_owned(), response.clone());
            }
            CindyCommandResult::Stopped => {
                if let Some(t) = target {
                    if let Some(row) = self.rows.iter_mut().find(|r| &r.address == t) {
                        row.phase = "idle".into();
                        row.run_id = None;
                    }
                }
                self.completed_command_replies
                    .insert(command_key.to_owned(), response.clone());
            }
            CindyCommandResult::Resolved => {
                if let Some(t) = target {
                    if let Some(row) = self.rows.iter_mut().find(|r| &r.address == t) {
                        row.pending_interaction = None;
                        row.attention = false;
                        if row.phase == "needs-interaction" {
                            row.phase = "running".into();
                        }
                    }
                }
                self.completed_command_replies
                    .insert(command_key.to_owned(), response.clone());
            }
            CindyCommandResult::Noop => {
                self.completed_command_replies
                    .insert(command_key.to_owned(), response.clone());
            }
            CindyCommandResult::Stale => {
                self.invalidate(CindyFailure::Stale);
            }
            CindyCommandResult::Denied
            | CindyCommandResult::InteractionTypeMismatch
            | CindyCommandResult::Unconfirmed
            | CindyCommandResult::Unknown => {}
            CindyCommandResult::Unavailable => {
                self.invalidate(CindyFailure::Unavailable);
            }
        }
        self.record_reply(response)
    }

    fn record_reply(&mut self, reply: CindyCommandReply) -> CindyCommandReply {
        self.last_command_reply = Some(reply.clone());
        reply
    }

    pub fn refresh_now(&mut self) {
        if !self.enabled || self.suspended {
            return;
        }
        let epoch = self.local_epoch;
        let result = if self.state == CindyConnectionState::Ready {
            self.client.read()
        } else {
            self.state = CindyConnectionState::Connecting;
            self.client.connect()
        };
        if !self.enabled || self.suspended || epoch != self.local_epoch {
            return;
        }
        self.apply(result);
    }

    pub fn selection(&self, address: &CindyTaskAddress) -> Option<CindySelection> {
        if !self.enabled
            || self.suspended
            || self.state != CindyConnectionState::Ready
            || !self.rows.iter().any(|r| &r.address == address)
        {
            return None;
        }
        Some(CindySelection {
            address: address.clone(),
            instance_id: self.instance_id.clone()?,
            generation: self.generation?,
            owner_epoch: self.owner_epoch.clone()?,
            local_epoch: self.local_epoch,
        })
    }

    pub fn open_address(&mut self, address: &CindyTaskAddress) -> CindyOpenResult {
        let Some(sel) = self.selection(address) else {
            self.last_result = Some(CindyOpenResult::Stale);
            return CindyOpenResult::Stale;
        };
        self.open_selection(&sel, true)
    }

    pub fn open(&mut self, selection: Option<&CindySelection>, admitted: bool) -> CindyOpenResult {
        let res = match selection {
            Some(sel) if admitted => self.open_selection(sel, admitted),
            _ => {
                self.last_result = Some(CindyOpenResult::Stale);
                CindyOpenResult::Stale
            }
        };
        let detail = selection.map_or_else(|| "stale".to_owned(), |s| s.address.id());
        self.record_host_receipt("cindy_open", res.as_str(), detail);
        res
    }

    pub fn open_selection(
        &mut self,
        selection: &CindySelection,
        admitted: bool,
    ) -> CindyOpenResult {
        if !self.enabled
            || self.suspended
            || self.state != CindyConnectionState::Ready
            || selection.local_epoch != self.local_epoch
            || self.instance_id.as_deref() != Some(selection.instance_id.as_str())
            || self.generation != Some(selection.generation)
            || self.owner_epoch.as_deref() != Some(selection.owner_epoch.as_str())
            || !self
                .scopes
                .contains(&CindyScope::new(&selection.address.workspace_path))
            || !self.rows.iter().any(|r| r.address == selection.address)
            || !admitted
        {
            if self.enabled && !self.suspended && self.state == CindyConnectionState::Ready {
                self.invalidate(CindyFailure::Stale);
            }
            self.last_result = Some(CindyOpenResult::Stale);
            return CindyOpenResult::Stale;
        }

        let epoch = self.local_epoch;
        let response = self.client.open(selection);
        if !self.enabled || self.suspended || epoch != self.local_epoch || !admitted {
            self.last_result = Some(CindyOpenResult::Stale);
            return CindyOpenResult::Stale;
        }

        if self.instance_id.as_deref() != Some(response.instance_id.as_str())
            || self.generation != Some(response.generation)
            || self.owner_epoch.as_deref() != Some(response.owner_epoch.as_str())
        {
            self.invalidate(CindyFailure::Stale);
            self.last_result = Some(CindyOpenResult::Stale);
            return CindyOpenResult::Stale;
        }

        match response.status {
            CindyOpenResult::Confirmed => {
                let expected_link = selection.address.deep_link();
                if response.delivered.as_ref() != Some(&selection.address)
                    || response.selected.as_ref() != Some(&selection.address)
                    || response.deep_link.as_deref() != Some(expected_link.as_str())
                {
                    self.delivered = response.delivered;
                    self.selected = None;
                    self.last_deep_link = response.deep_link;
                    self.last_result = Some(CindyOpenResult::Unconfirmed);
                    return CindyOpenResult::Unconfirmed;
                }
                self.delivered = Some(selection.address.clone());
                self.selected = Some(selection.address.clone());
                self.last_deep_link = response.deep_link;
                self.last_result = Some(CindyOpenResult::Confirmed);
                self.navigation_history.visit(NavigationPlace::cindy(
                    &selection.address.workspace_path,
                    &selection.address.session_id,
                ));
                CindyOpenResult::Confirmed
            }
            CindyOpenResult::Delivered => {
                self.delivered = Some(selection.address.clone());
                self.selected = None;
                self.last_deep_link = response.deep_link;
                self.last_result = Some(CindyOpenResult::Delivered);
                CindyOpenResult::Delivered
            }
            CindyOpenResult::Unconfirmed => {
                self.delivered = response.delivered;
                self.selected = None;
                self.last_deep_link = response.deep_link;
                self.last_result = Some(CindyOpenResult::Unconfirmed);
                CindyOpenResult::Unconfirmed
            }
            CindyOpenResult::Stale => {
                self.invalidate(CindyFailure::Stale);
                self.last_result = Some(CindyOpenResult::Stale);
                CindyOpenResult::Stale
            }
            CindyOpenResult::Denied => {
                self.invalidate(CindyFailure::Denied);
                self.last_result = Some(CindyOpenResult::Denied);
                CindyOpenResult::Denied
            }
            CindyOpenResult::Unavailable => {
                self.invalidate(CindyFailure::Unavailable);
                self.last_result = Some(CindyOpenResult::Unavailable);
                CindyOpenResult::Unavailable
            }
        }
    }

    fn apply(&mut self, result: Result<CindySessionSnapshot, CindyFailure>) {
        match result {
            Ok(snapshot) => {
                let identity_changed = self.instance_id.is_some()
                    && (self.instance_id.as_deref() != Some(snapshot.instance_id.as_str())
                        || self.generation != Some(snapshot.generation)
                        || self.owner_epoch.as_deref() != Some(snapshot.owner_epoch.as_str()));
                if identity_changed {
                    self.local_epoch = self.local_epoch.wrapping_add(1);
                    self.latched_commands_by_scope.clear();
                    self.completed_command_replies.clear();
                    self.attention_round.reset();
                    self.pending_unknown_command = None;
                    self.attention_round_visited_count = 0;
                }
                self.instance_id = Some(snapshot.instance_id);
                self.generation = Some(snapshot.generation);
                self.owner_epoch = Some(snapshot.owner_epoch);
                self.scopes = snapshot.scopes;
                self.rows = snapshot.rows;
                if let Some(ref snap_sel) = snapshot.selected {
                    if self.rows.iter().any(|r| &r.address == snap_sel) {
                        self.selected = Some(snap_sel.clone());
                    } else {
                        self.selected = None;
                    }
                } else {
                    self.selected = None;
                }
                if let Some(ref cur_deliv) = self.delivered {
                    if !self.rows.iter().any(|r| &r.address == cur_deliv) {
                        self.delivered = None;
                    }
                }
                self.state = CindyConnectionState::Ready;
            }
            Err(failure) => {
                self.invalidate(failure);
            }
        }
    }

    fn invalidate(&mut self, failure: CindyFailure) {
        self.local_epoch = self.local_epoch.wrapping_add(1);
        self.instance_id = None;
        self.generation = None;
        self.owner_epoch = None;
        self.scopes.clear();
        self.rows.clear();
        self.delivered = None;
        self.selected = None;
        self.last_deep_link = None;
        self.pending_unknown_command = None;
        self.latched_commands_by_scope.clear();
        self.completed_command_replies.clear();
        self.attention_round.reset();
        self.attention_round_visited_count = 0;
        self.state = CindyConnectionState::Unavailable(failure);
    }

    fn stop_and_clear(&mut self, next_state: CindyConnectionState) {
        self.local_epoch = self.local_epoch.wrapping_add(1);
        self.instance_id = None;
        self.generation = None;
        self.owner_epoch = None;
        self.scopes.clear();
        self.rows.clear();
        self.delivered = None;
        self.selected = None;
        self.last_deep_link = None;
        self.last_result = None;
        self.last_command_reply = None;
        self.pending_unknown_command = None;
        self.latched_commands_by_scope.clear();
        self.completed_command_replies.clear();
        self.attention_round.reset();
        self.attention_round_visited_count = 0;
        self.state = next_state;
        self.client.close();
    }

    fn record_host_receipt(&mut self, action_type: &str, outcome: &str, detail: impl Into<String>) {
        let binding_id = if action_type == "cindy_open" {
            "menu-cindy-session"
        } else {
            "menu-cindy-command"
        };
        let detail_str = detail.into();
        self.audit_receipts.push(HostActionReceipt {
            binding_id: binding_id.into(),
            profile_id: "cindy".into(),
            action: ResolvedHostAction::AgentCommand {
                adapter: "cindy".into(),
                command: format!("{action_type}:{outcome}:{detail_str}"),
            },
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CindyOpenOverride {
    Normal,
    DeliveredOnly,
    WrongSelected(CindyTaskAddress),
    MissingSelected,
    RotateGeneration(u64),
    RotateOwner(String, u64),
}

struct FixtureState {
    instance_id: String,
    generation: u64,
    owner_epoch: String,
    scopes: Vec<CindyScope>,
    all_rows: Vec<CindyTaskRow>,
    delivered: Option<CindyTaskAddress>,
    selected: Option<CindyTaskAddress>,
    open_override: CindyOpenOverride,
    authorized_metadata_reads: usize,
    unauthorized_metadata_reads: usize,
    open_history: Vec<CindyTaskAddress>,
    create_count: usize,
    send_count: usize,
    stop_count: usize,
    resolve_count: usize,
    timeout_next_command: bool,
    recorded_commands: HashMap<String, CindyCommandReply>,
}

impl FixtureState {
    fn new() -> Self {
        let alpha1 = CindyTaskAddress::new("/Users/example/Projects/alpha", "sess-alpha-1");
        let alpha2 = CindyTaskAddress::new("/Users/example/Projects/alpha", "sess-alpha-2");
        let beta1 = CindyTaskAddress::new("/Users/example/Projects/beta", "sess-beta-1");
        let secret = CindyTaskAddress::new("/Users/example/Projects/secret", "sess-secret-9");
        Self {
            instance_id: "cindy-instance-1".into(),
            generation: 1,
            owner_epoch: "owner-account-1:1".into(),
            scopes: vec![
                CindyScope::new("/Users/example/Projects/alpha"),
                CindyScope::new("/Users/example/Projects/beta"),
            ],
            all_rows: vec![
                CindyTaskRow {
                    address: alpha1.clone(),
                    title: "Daily Standup".into(),
                    phase: "running".into(),
                    attention: false,
                    pinned: true,
                    run_id: Some("run-alpha-1".into()),
                    unread: Some(false),
                    event_watermark: Some(10),
                    queued_count: 0,
                    pending_interaction: None,
                },
                CindyTaskRow {
                    address: alpha2,
                    title: "Daily Standup".into(),
                    phase: "needs-interaction".into(),
                    attention: true,
                    pinned: false,
                    run_id: Some("run-alpha-2".into()),
                    unread: Some(false),
                    event_watermark: Some(20),
                    queued_count: 0,
                    pending_interaction: Some(CindyPendingInteraction {
                        interaction_id: "perm-alpha-2".into(),
                        interaction_type: CindyInteractionType::Permission,
                        tool_name: Some("bash".into()),
                        options: None,
                    }),
                },
                CindyTaskRow {
                    address: beta1,
                    title: "Daily Standup".into(),
                    phase: "idle".into(),
                    attention: false,
                    pinned: false,
                    run_id: None,
                    unread: None,
                    event_watermark: Some(5),
                    queued_count: 0,
                    pending_interaction: None,
                },
                CindyTaskRow {
                    address: secret,
                    title: "Secret Workspace Task".into(),
                    phase: "running".into(),
                    attention: true,
                    pinned: false,
                    run_id: Some("run-secret-9".into()),
                    unread: Some(true),
                    event_watermark: Some(90),
                    queued_count: 0,
                    pending_interaction: None,
                },
            ],
            delivered: None,
            selected: Some(alpha1),
            open_override: CindyOpenOverride::Normal,
            authorized_metadata_reads: 0,
            unauthorized_metadata_reads: 0,
            open_history: Vec::new(),
            create_count: 0,
            send_count: 0,
            stop_count: 0,
            resolve_count: 0,
            timeout_next_command: false,
            recorded_commands: HashMap::new(),
        }
    }
}

pub struct CindyNavigationFixture {
    state: Mutex<FixtureState>,
}

impl Default for CindyNavigationFixture {
    fn default() -> Self {
        Self::new()
    }
}

impl CindyNavigationFixture {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(FixtureState::new()),
        }
    }

    pub fn reset(&self) {
        *self.state.lock().unwrap() = FixtureState::new();
    }

    pub fn set_open_override(&self, mode: CindyOpenOverride) {
        self.state.lock().unwrap().open_override = mode;
    }

    pub fn set_timeout_next_command(&self, enabled: bool) {
        self.state.lock().unwrap().timeout_next_command = enabled;
    }

    pub fn upsert_task(&self, row: CindyTaskRow) {
        let mut s = self.state.lock().unwrap();
        if let Some(existing) = s.all_rows.iter_mut().find(|r| r.address == row.address) {
            *existing = row;
        } else {
            s.all_rows.push(row);
        }
    }

    pub fn remove_task(&self, address: &CindyTaskAddress) {
        let mut s = self.state.lock().unwrap();
        s.all_rows.retain(|r| &r.address != address);
        if s.delivered.as_ref() == Some(address) {
            s.delivered = None;
        }
        if s.selected.as_ref() == Some(address) {
            s.selected = None;
        }
    }

    pub fn stats(&self) -> (usize, usize, Vec<CindyTaskAddress>) {
        let s = self.state.lock().unwrap();
        (
            s.authorized_metadata_reads,
            s.unauthorized_metadata_reads,
            s.open_history.clone(),
        )
    }

    pub fn command_counts(&self) -> (usize, usize, usize, usize) {
        let s = self.state.lock().unwrap();
        (s.create_count, s.send_count, s.stop_count, s.resolve_count)
    }
}

impl CindySessionServing for CindyNavigationFixture {
    fn connect(&self) -> Result<CindySessionSnapshot, CindyFailure> {
        self.read()
    }

    fn read(&self) -> Result<CindySessionSnapshot, CindyFailure> {
        let mut s = self.state.lock().unwrap();
        s.authorized_metadata_reads += 1;
        let allowed_paths: HashSet<&str> = s
            .scopes
            .iter()
            .map(|sc| sc.workspace_path.as_str())
            .collect();
        let scoped_rows: Vec<CindyTaskRow> = s
            .all_rows
            .iter()
            .filter(|r| allowed_paths.contains(r.address.workspace_path.as_str()))
            .cloned()
            .collect();
        let scoped_selected = s.selected.as_ref().and_then(|cur| {
            scoped_rows
                .iter()
                .any(|r| &r.address == cur)
                .then(|| cur.clone())
        });
        Ok(CindySessionSnapshot {
            instance_id: s.instance_id.clone(),
            generation: s.generation,
            owner_epoch: s.owner_epoch.clone(),
            scopes: s.scopes.clone(),
            rows: scoped_rows,
            selected: scoped_selected,
        })
    }

    fn open(&self, selection: &CindySelection) -> CindyOpenReply {
        let mut s = self.state.lock().unwrap();
        let allowed = s
            .scopes
            .iter()
            .any(|sc| sc.workspace_path == selection.address.workspace_path);
        if !allowed {
            return CindyOpenReply {
                instance_id: s.instance_id.clone(),
                generation: s.generation,
                owner_epoch: s.owner_epoch.clone(),
                status: CindyOpenResult::Denied,
                delivered: None,
                selected: None,
                deep_link: None,
            };
        }

        let mode = std::mem::replace(&mut s.open_override, CindyOpenOverride::Normal);
        match mode {
            CindyOpenOverride::RotateGeneration(next_gen) => {
                s.generation = next_gen;
                return CindyOpenReply {
                    instance_id: s.instance_id.clone(),
                    generation: s.generation,
                    owner_epoch: s.owner_epoch.clone(),
                    status: CindyOpenResult::Stale,
                    delivered: None,
                    selected: None,
                    deep_link: None,
                };
            }
            CindyOpenOverride::RotateOwner(next_owner, next_gen) => {
                s.owner_epoch = next_owner;
                s.generation = next_gen;
                return CindyOpenReply {
                    instance_id: s.instance_id.clone(),
                    generation: s.generation,
                    owner_epoch: s.owner_epoch.clone(),
                    status: CindyOpenResult::Stale,
                    delivered: None,
                    selected: None,
                    deep_link: None,
                };
            }
            CindyOpenOverride::WrongSelected(wrong_addr) => {
                s.delivered = Some(selection.address.clone());
                s.selected = Some(wrong_addr.clone());
                s.open_history.push(selection.address.clone());
                return CindyOpenReply {
                    instance_id: s.instance_id.clone(),
                    generation: s.generation,
                    owner_epoch: s.owner_epoch.clone(),
                    status: CindyOpenResult::Confirmed,
                    delivered: Some(selection.address.clone()),
                    selected: Some(wrong_addr),
                    deep_link: Some(selection.address.deep_link()),
                };
            }
            CindyOpenOverride::MissingSelected => {
                s.delivered = Some(selection.address.clone());
                s.selected = None;
                s.open_history.push(selection.address.clone());
                return CindyOpenReply {
                    instance_id: s.instance_id.clone(),
                    generation: s.generation,
                    owner_epoch: s.owner_epoch.clone(),
                    status: CindyOpenResult::Unconfirmed,
                    delivered: Some(selection.address.clone()),
                    selected: None,
                    deep_link: Some(selection.address.deep_link()),
                };
            }
            CindyOpenOverride::DeliveredOnly => {
                if selection.instance_id != s.instance_id
                    || selection.generation != s.generation
                    || selection.owner_epoch != s.owner_epoch
                {
                    return CindyOpenReply {
                        instance_id: s.instance_id.clone(),
                        generation: s.generation,
                        owner_epoch: s.owner_epoch.clone(),
                        status: CindyOpenResult::Stale,
                        delivered: None,
                        selected: None,
                        deep_link: None,
                    };
                }
                s.authorized_metadata_reads += 1;
                if !s.all_rows.iter().any(|r| r.address == selection.address) {
                    return CindyOpenReply {
                        instance_id: s.instance_id.clone(),
                        generation: s.generation,
                        owner_epoch: s.owner_epoch.clone(),
                        status: CindyOpenResult::Stale,
                        delivered: None,
                        selected: None,
                        deep_link: None,
                    };
                }
                s.delivered = Some(selection.address.clone());
                s.selected = None;
                s.open_history.push(selection.address.clone());
                return CindyOpenReply {
                    instance_id: s.instance_id.clone(),
                    generation: s.generation,
                    owner_epoch: s.owner_epoch.clone(),
                    status: CindyOpenResult::Delivered,
                    delivered: Some(selection.address.clone()),
                    selected: None,
                    deep_link: Some(selection.address.deep_link()),
                };
            }
            CindyOpenOverride::Normal => {}
        }

        if selection.instance_id != s.instance_id
            || selection.generation != s.generation
            || selection.owner_epoch != s.owner_epoch
        {
            return CindyOpenReply {
                instance_id: s.instance_id.clone(),
                generation: s.generation,
                owner_epoch: s.owner_epoch.clone(),
                status: CindyOpenResult::Stale,
                delivered: None,
                selected: None,
                deep_link: None,
            };
        }

        s.authorized_metadata_reads += 1;
        if !s.all_rows.iter().any(|r| r.address == selection.address) {
            return CindyOpenReply {
                instance_id: s.instance_id.clone(),
                generation: s.generation,
                owner_epoch: s.owner_epoch.clone(),
                status: CindyOpenResult::Stale,
                delivered: None,
                selected: None,
                deep_link: None,
            };
        }

        s.delivered = Some(selection.address.clone());
        s.selected = Some(selection.address.clone());
        s.open_history.push(selection.address.clone());
        CindyOpenReply {
            instance_id: s.instance_id.clone(),
            generation: s.generation,
            owner_epoch: s.owner_epoch.clone(),
            status: CindyOpenResult::Confirmed,
            delivered: Some(selection.address.clone()),
            selected: Some(selection.address.clone()),
            deep_link: Some(selection.address.deep_link()),
        }
    }

    fn create(&self, intent: &CindyCreateIntent) -> CindyCommandReply {
        let mut s = self.state.lock().unwrap();
        let key = format!("{}::{}", intent.scope.workspace_path, intent.command_id);
        if let Some(rec) = s.recorded_commands.get(&key).cloned() {
            let mut dup = rec;
            dup.duplicate = true;
            return dup;
        }
        if !s.scopes.contains(&intent.scope) {
            return CindyCommandReply::simple(
                &intent.command_id,
                &s.instance_id,
                s.generation,
                &s.owner_epoch,
                CindyCommandResult::Denied,
                Some("guard.scopeOutOfBounds"),
            );
        }
        s.create_count += 1;
        let created = CindyTaskAddress::new(
            &intent.scope.workspace_path,
            format!("sess-created-{}", s.create_count),
        );
        let run_id = intent
            .text
            .as_ref()
            .map(|_| format!("run-created-{}", s.create_count));
        let new_row = CindyTaskRow {
            address: created.clone(),
            title: intent
                .text
                .clone()
                .unwrap_or_else(|| format!("New Task {}", s.create_count)),
            phase: if intent.text.is_some() {
                "running".into()
            } else {
                "idle".into()
            },
            attention: false,
            pinned: false,
            run_id: run_id.clone(),
            unread: Some(false),
            event_watermark: Some((100 + s.create_count) as u64),
            queued_count: 0,
            pending_interaction: None,
        };
        s.all_rows.push(new_row);
        s.delivered = Some(created.clone());
        s.selected = Some(created.clone());
        let mut reply = CindyCommandReply::simple(
            &intent.command_id,
            &s.instance_id,
            s.generation,
            &s.owner_epoch,
            CindyCommandResult::Confirmed,
            None,
        );
        reply.delivered = Some(created.clone());
        reply.selected = Some(created.clone());
        reply.deep_link = Some(created.deep_link());
        reply.run_id = run_id;
        s.recorded_commands.insert(key, reply.clone());
        reply
    }

    fn send(&self, intent: &CindySendIntent) -> CindyCommandReply {
        let mut s = self.state.lock().unwrap();
        let addr = &intent.selection.address;
        let key = format!("{}::{}", addr.workspace_path, intent.command_id);
        if let Some(rec) = s.recorded_commands.get(&key).cloned() {
            let mut dup = rec;
            dup.duplicate = true;
            return dup;
        }
        if s.selected.as_ref() != Some(addr) {
            let mut r = CindyCommandReply::simple(
                &intent.command_id,
                &s.instance_id,
                s.generation,
                &s.owner_epoch,
                CindyCommandResult::Unconfirmed,
                Some("guard.selectionNotCommitted"),
            );
            r.selected = s.selected.clone();
            return r;
        }
        let Some(idx) = s.all_rows.iter().position(|r| &r.address == addr) else {
            return CindyCommandReply::simple(
                &intent.command_id,
                &s.instance_id,
                s.generation,
                &s.owner_epoch,
                CindyCommandResult::Stale,
                Some("guard.targetMissing"),
            );
        };
        if intent.delivery_mode == CindyDeliveryMode::Steer && s.all_rows[idx].phase != "running" {
            let mut r = CindyCommandReply::simple(
                &intent.command_id,
                &s.instance_id,
                s.generation,
                &s.owner_epoch,
                CindyCommandResult::Denied,
                Some("guard.steerRequiresRunningTurn"),
            );
            r.selected = Some(addr.clone());
            return r;
        }
        s.send_count += 1;
        let send_num = s.send_count;
        let inst = s.instance_id.clone();
        let gen = s.generation;
        let owner = s.owner_epoch.clone();
        let reply = if s.all_rows[idx].phase != "running" {
            let rid = format!("run-send-{send_num}");
            s.all_rows[idx].phase = "running".into();
            s.all_rows[idx].run_id = Some(rid.clone());
            s.all_rows[idx].event_watermark =
                Some(s.all_rows[idx].event_watermark.unwrap_or(0) + 1);
            let mut r = CindyCommandReply::simple(
                &intent.command_id,
                inst,
                gen,
                owner,
                CindyCommandResult::Started,
                None,
            );
            r.selected = Some(addr.clone());
            r.delivery = Some(CindyDeliveryMode::Auto);
            r.run_id = Some(rid);
            r.queued_count = s.all_rows[idx].queued_count;
            r
        } else if intent.delivery_mode == CindyDeliveryMode::Steer {
            s.all_rows[idx].event_watermark =
                Some(s.all_rows[idx].event_watermark.unwrap_or(0) + 1);
            let mut r = CindyCommandReply::simple(
                &intent.command_id,
                inst,
                gen,
                owner,
                CindyCommandResult::Steered,
                None,
            );
            r.selected = Some(addr.clone());
            r.delivery = Some(CindyDeliveryMode::Steer);
            r.run_id = s.all_rows[idx].run_id.clone();
            r.queued_count = s.all_rows[idx].queued_count;
            r
        } else {
            s.all_rows[idx].queued_count += 1;
            s.all_rows[idx].event_watermark =
                Some(s.all_rows[idx].event_watermark.unwrap_or(0) + 1);
            let mut r = CindyCommandReply::simple(
                &intent.command_id,
                inst,
                gen,
                owner,
                CindyCommandResult::Queued,
                None,
            );
            r.selected = Some(addr.clone());
            r.delivery = Some(CindyDeliveryMode::Queue);
            r.run_id = s.all_rows[idx].run_id.clone();
            r.queued_count = s.all_rows[idx].queued_count;
            r
        };
        s.recorded_commands.insert(key, reply.clone());
        if s.timeout_next_command {
            s.timeout_next_command = false;
            return CindyCommandReply::simple(
                &intent.command_id,
                &s.instance_id,
                s.generation,
                &s.owner_epoch,
                CindyCommandResult::Unknown,
                Some("bridge.transportTimeout"),
            );
        }
        reply
    }

    fn stop(&self, intent: &CindyStopIntent) -> CindyCommandReply {
        let mut s = self.state.lock().unwrap();
        let addr = &intent.selection.address;
        let key = format!("{}::{}", addr.workspace_path, intent.command_id);
        if let Some(rec) = s.recorded_commands.get(&key).cloned() {
            let mut dup = rec;
            dup.duplicate = true;
            return dup;
        }
        if s.selected.as_ref() != Some(addr) {
            let mut r = CindyCommandReply::simple(
                &intent.command_id,
                &s.instance_id,
                s.generation,
                &s.owner_epoch,
                CindyCommandResult::Unconfirmed,
                Some("guard.selectionNotCommitted"),
            );
            r.selected = s.selected.clone();
            return r;
        }
        let Some(idx) = s.all_rows.iter().position(|r| &r.address == addr) else {
            return CindyCommandReply::simple(
                &intent.command_id,
                &s.instance_id,
                s.generation,
                &s.owner_epoch,
                CindyCommandResult::Stale,
                Some("guard.targetMissing"),
            );
        };
        let active_run = s.all_rows[idx].run_id.clone();
        if s.all_rows[idx].phase != "running"
            || active_run.as_deref() != Some(intent.expected_run_id.as_str())
        {
            let rc = if s.all_rows[idx].phase != "running" {
                "guard.notRunning"
            } else {
                "guard.stopTargetChanged"
            };
            let mut noop = CindyCommandReply::simple(
                &intent.command_id,
                &s.instance_id,
                s.generation,
                &s.owner_epoch,
                CindyCommandResult::Noop,
                Some(rc),
            );
            noop.selected = Some(addr.clone());
            noop.run_id = active_run;
            s.recorded_commands.insert(key, noop.clone());
            return noop;
        }
        s.stop_count += 1;
        s.all_rows[idx].phase = "idle".into();
        s.all_rows[idx].run_id = None;
        s.all_rows[idx].event_watermark = Some(s.all_rows[idx].event_watermark.unwrap_or(0) + 1);
        let mut reply = CindyCommandReply::simple(
            &intent.command_id,
            &s.instance_id,
            s.generation,
            &s.owner_epoch,
            CindyCommandResult::Stopped,
            None,
        );
        reply.selected = Some(addr.clone());
        reply.run_id = active_run;
        s.recorded_commands.insert(key, reply.clone());
        reply
    }

    fn resolve(&self, intent: &CindyResolveIntent) -> CindyCommandReply {
        let mut s = self.state.lock().unwrap();
        let addr = &intent.selection.address;
        let key = format!("{}::{}", addr.workspace_path, intent.command_id);
        if let Some(rec) = s.recorded_commands.get(&key).cloned() {
            let mut dup = rec;
            dup.duplicate = true;
            return dup;
        }
        if intent.interaction_type == CindyInteractionType::Question
            && matches!(
                intent.decision,
                CindyInteractionDecision::Approve | CindyInteractionDecision::AllowForSession
            )
            && intent.option_id.as_deref().is_none_or(str::is_empty)
        {
            let mut r = CindyCommandReply::simple(
                &intent.command_id,
                &s.instance_id,
                s.generation,
                &s.owner_epoch,
                CindyCommandResult::InteractionTypeMismatch,
                Some("guard.questionCannotApproveAsPermission"),
            );
            r.selected = s.selected.clone();
            r.interaction_id = Some(intent.interaction_id.clone());
            r.interaction_type = Some(intent.interaction_type);
            return r;
        }
        if s.selected.as_ref() != Some(addr) {
            let mut r = CindyCommandReply::simple(
                &intent.command_id,
                &s.instance_id,
                s.generation,
                &s.owner_epoch,
                CindyCommandResult::Unconfirmed,
                Some("guard.selectionNotCommitted"),
            );
            r.selected = s.selected.clone();
            return r;
        }
        let Some(idx) = s.all_rows.iter().position(|r| &r.address == addr) else {
            return CindyCommandReply::simple(
                &intent.command_id,
                &s.instance_id,
                s.generation,
                &s.owner_epoch,
                CindyCommandResult::Stale,
                Some("guard.targetMissing"),
            );
        };
        let pending = s.all_rows[idx].pending_interaction.clone();
        let Some(p) = pending.filter(|p| p.interaction_id == intent.interaction_id) else {
            let mut noop = CindyCommandReply::simple(
                &intent.command_id,
                &s.instance_id,
                s.generation,
                &s.owner_epoch,
                CindyCommandResult::Noop,
                Some("guard.interactionAlreadyResolved"),
            );
            noop.selected = Some(addr.clone());
            noop.interaction_id = Some(intent.interaction_id.clone());
            noop.interaction_type = Some(intent.interaction_type);
            s.recorded_commands.insert(key, noop.clone());
            return noop;
        };
        if p.interaction_type != intent.interaction_type {
            let mut r = CindyCommandReply::simple(
                &intent.command_id,
                &s.instance_id,
                s.generation,
                &s.owner_epoch,
                CindyCommandResult::InteractionTypeMismatch,
                Some("guard.interactionTypeMismatch"),
            );
            r.selected = Some(addr.clone());
            r.interaction_id = Some(p.interaction_id);
            r.interaction_type = Some(p.interaction_type);
            return r;
        }
        s.resolve_count += 1;
        s.all_rows[idx].pending_interaction = None;
        s.all_rows[idx].attention = false;
        s.all_rows[idx].phase = if matches!(
            intent.decision,
            CindyInteractionDecision::Reject | CindyInteractionDecision::Cancel
        ) {
            "idle".into()
        } else {
            "running".into()
        };
        s.all_rows[idx].event_watermark = Some(s.all_rows[idx].event_watermark.unwrap_or(0) + 1);
        let mut reply = CindyCommandReply::simple(
            &intent.command_id,
            &s.instance_id,
            s.generation,
            &s.owner_epoch,
            CindyCommandResult::Resolved,
            None,
        );
        reply.selected = Some(addr.clone());
        reply.interaction_id = Some(p.interaction_id);
        reply.interaction_type = Some(p.interaction_type);
        s.recorded_commands.insert(key, reply.clone());
        reply
    }

    fn query(&self, intent: &CindyQueryIntent) -> CindyCommandReply {
        let s = self.state.lock().unwrap();
        let key = format!("{}::{}", intent.scope.workspace_path, intent.command_id);
        if let Some(rec) = s.recorded_commands.get(&key).cloned() {
            let mut dup = rec;
            dup.duplicate = true;
            return dup;
        }
        CindyCommandReply::simple(
            &intent.command_id,
            &s.instance_id,
            s.generation,
            &s.owner_epoch,
            CindyCommandResult::Noop,
            Some("query.notFound"),
        )
    }

    fn close(&self) {}
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CindyScenarioCheckReceipt {
    pub name: String,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CindyNavigationScenarioReport {
    pub scenario: String,
    pub status: String,
    pub unauthorized_metadata_reads: usize,
    pub authorized_metadata_reads: usize,
    pub local_agent_api_relaxed: bool,
    pub checks: Vec<CindyScenarioCheckReceipt>,
    pub native_app: String,
    pub physical_key: String,
    #[serde(skip)]
    pub receipts: Vec<CindyAuditReceipt>,
}

fn push_cindy_audit(
    receipts: &mut Vec<CindyAuditReceipt>,
    binding_id: &str,
    action_type: &str,
    outcome: &str,
    detail: impl Into<String>,
) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    receipts.push(CindyAuditReceipt {
        timestamp_ms: ts,
        profile_id: "cindy".into(),
        binding_id: binding_id.into(),
        action_type: action_type.into(),
        target: "cindy".into(),
        outcome: outcome.into(),
        detail: detail.into(),
        text_byte_len: None,
    });
}

pub fn run_cindy_navigation_scenario(
    audit_log_path: Option<&Path>,
) -> Result<CindyNavigationScenarioReport, String> {
    let fixture = Arc::new(CindyNavigationFixture::new());
    fixture.reset();
    let mut cindy_sessions = CindySessionController::new(fixture.clone());
    let mut checks = Vec::new();
    let mut receipts = Vec::new();

    let queued_open = |ctrl: &mut CindySessionController<Arc<CindyNavigationFixture>>,
                       recs: &mut Vec<CindyAuditReceipt>,
                       sel: &CindySelection| {
        let res = ctrl.open(Some(sel), true);
        push_cindy_audit(
            recs,
            "menu-cindy-session",
            "cindy_open",
            res.as_str(),
            format!("{} ({})", sel.address.id(), res.label()),
        );
    };
    let queued_command = |ctrl: &mut CindySessionController<Arc<CindyNavigationFixture>>,
                          recs: &mut Vec<CindyAuditReceipt>,
                          cmd: &CindyQueuedCommand| {
        let reply = ctrl.execute(cmd, true);
        push_cindy_audit(
            recs,
            "menu-cindy-command",
            cmd.action_name(),
            reply.status.as_str(),
            format!(
                "{} -> {}",
                cmd.label(),
                reply.reason_code.as_deref().unwrap_or(reply.status.label())
            ),
        );
    };

    let initial_stats = fixture.stats();
    let disabled_zero_reads_pass = !cindy_sessions.enabled
        && cindy_sessions.state == CindyConnectionState::Disabled
        && initial_stats.0 == 0
        && initial_stats.1 == 0;
    checks.push(CindyScenarioCheckReceipt {
        name: "disabled_zero_reads".into(),
        status: if disabled_zero_reads_pass {
            "PASS"
        } else {
            "FAIL"
        }
        .into(),
        detail: format!(
            "enabled={} authorized_reads={}",
            cindy_sessions.enabled, initial_stats.0
        ),
    });

    cindy_sessions.set_enabled(true);
    cindy_sessions.refresh_now();

    let alpha1 = CindyTaskAddress::new("/Users/example/Projects/alpha", "sess-alpha-1");
    let alpha2 = CindyTaskAddress::new("/Users/example/Projects/alpha", "sess-alpha-2");
    let beta1 = CindyTaskAddress::new("/Users/example/Projects/beta", "sess-beta-1");
    let secret = CindyTaskAddress::new("/Users/example/Projects/secret", "sess-secret-9");

    let row_addrs: Vec<CindyTaskAddress> = cindy_sessions
        .rows
        .iter()
        .map(|r| r.address.clone())
        .collect();
    let scoped_catalog_pass = cindy_sessions.state == CindyConnectionState::Ready
        && row_addrs == vec![alpha1.clone(), alpha2.clone(), beta1.clone()]
        && !cindy_sessions.rows.iter().any(|r| r.address == secret);
    checks.push(CindyScenarioCheckReceipt {
        name: "scoped_catalog_only".into(),
        status: if scoped_catalog_pass { "PASS" } else { "FAIL" }.into(),
        detail: format!(
            "rows={}",
            cindy_sessions
                .rows
                .iter()
                .map(CindyTaskRow::id)
                .collect::<Vec<_>>()
                .join(",")
        ),
    });

    fixture.set_open_override(CindyOpenOverride::DeliveredOnly);
    if let Some(sel) = cindy_sessions.selection(&alpha2) {
        queued_open(&mut cindy_sessions, &mut receipts, &sel);
    }
    let delivered_only_pass = cindy_sessions.last_result == Some(CindyOpenResult::Delivered)
        && receipts.last().map(|r| r.outcome.as_str()) == Some("delivered")
        && cindy_sessions.delivered.as_ref() == Some(&alpha2)
        && cindy_sessions.selected.is_none()
        && cindy_sessions.last_deep_link.as_deref() == Some("cindy://session/sess-alpha-2")
        && cindy_sessions.navigation_history.current()
            != Some(&NavigationPlace::cindy(
                &alpha2.workspace_path,
                &alpha2.session_id,
            ))
        && cindy_sessions.navigation_history.current()
            != Some(&NavigationPlace::session("cindy", "sess-alpha-2"));

    if let Some(sel) = cindy_sessions.selection(&alpha2) {
        queued_open(&mut cindy_sessions, &mut receipts, &sel);
    }
    let confirmed_after_delivered_pass = delivered_only_pass
        && cindy_sessions.last_result == Some(CindyOpenResult::Confirmed)
        && receipts.last().map(|r| r.outcome.as_str()) == Some("confirmed")
        && cindy_sessions.delivered.as_ref() == Some(&alpha2)
        && cindy_sessions.selected.as_ref() == Some(&alpha2)
        && cindy_sessions.last_deep_link.as_deref() == Some("cindy://session/sess-alpha-2")
        && (cindy_sessions.navigation_history.current()
            == Some(&NavigationPlace::cindy(
                &alpha2.workspace_path,
                &alpha2.session_id,
            ))
            || cindy_sessions.navigation_history.current()
                == Some(&NavigationPlace::session("cindy", "sess-alpha-2")));
    checks.push(CindyScenarioCheckReceipt {
        name: "delivered_vs_confirmed_separated".into(),
        status: if confirmed_after_delivered_pass {
            "PASS"
        } else {
            "FAIL"
        }
        .into(),
        detail: format!(
            "delivered={} selected={} deepLink={}",
            cindy_sessions
                .delivered
                .as_ref()
                .map_or_else(|| "nil".into(), CindyTaskAddress::id),
            cindy_sessions
                .selected
                .as_ref()
                .map_or_else(|| "nil".into(), CindyTaskAddress::id),
            cindy_sessions.last_deep_link.as_deref().unwrap_or("nil")
        ),
    });

    let same_name_pass = cindy_sessions.last_result == Some(CindyOpenResult::Confirmed)
        && cindy_sessions.selected.as_ref() == Some(&alpha2)
        && cindy_sessions.selected.as_ref() != Some(&alpha1)
        && cindy_sessions.selected.as_ref() != Some(&beta1);
    checks.push(CindyScenarioCheckReceipt {
        name: "same_name_exact_identity".into(),
        status: if same_name_pass { "PASS" } else { "FAIL" }.into(),
        detail: format!(
            "selected={}",
            cindy_sessions
                .selected
                .as_ref()
                .map_or_else(|| "nil".into(), CindyTaskAddress::id)
        ),
    });

    if let Some(sel) = cindy_sessions.selection(&beta1) {
        queued_open(&mut cindy_sessions, &mut receipts, &sel);
    }
    let readback_pass = cindy_sessions.last_result == Some(CindyOpenResult::Confirmed)
        && cindy_sessions.selected.as_ref() == Some(&beta1)
        && (cindy_sessions.navigation_history.current()
            == Some(&NavigationPlace::cindy(
                &beta1.workspace_path,
                &beta1.session_id,
            ))
            || cindy_sessions.navigation_history.current()
                == Some(&NavigationPlace::session("cindy", "sess-beta-1")));
    checks.push(CindyScenarioCheckReceipt {
        name: "selected_readback_matches".into(),
        status: if readback_pass { "PASS" } else { "FAIL" }.into(),
        detail: format!(
            "selected={}",
            cindy_sessions
                .selected
                .as_ref()
                .map_or_else(|| "nil".into(), CindyTaskAddress::id)
        ),
    });

    fixture.set_open_override(CindyOpenOverride::WrongSelected(alpha1.clone()));
    if let Some(sel) = cindy_sessions.selection(&beta1) {
        queued_open(&mut cindy_sessions, &mut receipts, &sel);
    }
    let unconfirmed_pass = cindy_sessions.last_result == Some(CindyOpenResult::Unconfirmed)
        && receipts.last().map(|r| r.outcome.as_str()) == Some("unconfirmed")
        && cindy_sessions.delivered.as_ref() == Some(&beta1)
        && cindy_sessions.selected.is_none();
    checks.push(CindyScenarioCheckReceipt {
        name: "wrong_selected_unconfirmed".into(),
        status: if unconfirmed_pass { "PASS" } else { "FAIL" }.into(),
        detail: format!(
            "result={} selected={}",
            cindy_sessions.last_result.map_or("nil", |r| r.as_str()),
            cindy_sessions
                .selected
                .as_ref()
                .map_or_else(|| "nil".into(), CindyTaskAddress::id)
        ),
    });

    let reads_before_out_of_scope = fixture.stats().0;
    let forged_reply = fixture.open(&CindySelection {
        address: secret,
        instance_id: "cindy-instance-1".into(),
        generation: 1,
        owner_epoch: "owner-account-1:1".into(),
        local_epoch: 1,
    });
    let reads_after_out_of_scope = fixture.stats();
    let out_of_scope_zero_reads_pass = forged_reply.status == CindyOpenResult::Denied
        && reads_after_out_of_scope.0 == reads_before_out_of_scope
        && reads_after_out_of_scope.1 == 0;
    checks.push(CindyScenarioCheckReceipt {
        name: "out_of_scope_zero_reads".into(),
        status: if out_of_scope_zero_reads_pass {
            "PASS"
        } else {
            "FAIL"
        }
        .into(),
        detail: format!(
            "status={} delta_reads={}",
            forged_reply.status.as_str(),
            reads_after_out_of_scope.0 - reads_before_out_of_scope
        ),
    });

    cindy_sessions.refresh_now();
    if let Some(stale_deleted_sel) = cindy_sessions.selection(&alpha2) {
        fixture.remove_task(&alpha2);
        let history_before = fixture.stats().2.len();
        queued_open(&mut cindy_sessions, &mut receipts, &stale_deleted_sel);
        let history_after = fixture.stats().2.len();
        let deleted_no_redirect_pass = cindy_sessions.last_result == Some(CindyOpenResult::Stale)
            && receipts.last().map(|r| r.outcome.as_str()) == Some("stale")
            && cindy_sessions.state == CindyConnectionState::Unavailable(CindyFailure::Stale)
            && cindy_sessions.selected.is_none()
            && cindy_sessions.rows.is_empty()
            && history_after == history_before;
        checks.push(CindyScenarioCheckReceipt {
            name: "deleted_target_no_redirect".into(),
            status: if deleted_no_redirect_pass {
                "PASS"
            } else {
                "FAIL"
            }
            .into(),
            detail: format!(
                "result={} extra_opens={}",
                cindy_sessions.last_result.map_or("nil", |r| r.as_str()),
                history_after - history_before
            ),
        });
    }

    cindy_sessions.refresh_now();
    if let Some(pre_rotation_sel) = cindy_sessions.selection(&alpha1) {
        fixture.set_open_override(CindyOpenOverride::RotateOwner(
            "owner-account-2:2".into(),
            2,
        ));
        queued_open(&mut cindy_sessions, &mut receipts, &pre_rotation_sel);
        let rotation_pass = cindy_sessions.last_result == Some(CindyOpenResult::Stale)
            && receipts.last().map(|r| r.outcome.as_str()) == Some("stale")
            && cindy_sessions.state == CindyConnectionState::Unavailable(CindyFailure::Stale)
            && cindy_sessions.rows.is_empty()
            && cindy_sessions.selected.is_none()
            && cindy_sessions.delivered.is_none();
        checks.push(CindyScenarioCheckReceipt {
            name: "account_or_generation_change_blocks_old_selection".into(),
            status: if rotation_pass { "PASS" } else { "FAIL" }.into(),
            detail: format!(
                "state={} rows={}",
                cindy_sessions.state.label(),
                cindy_sessions.rows.len()
            ),
        });
    }

    fixture.upsert_task(CindyTaskRow {
        address: alpha2.clone(),
        title: "Daily Standup".into(),
        phase: "needs-interaction".into(),
        attention: true,
        pinned: false,
        run_id: Some("run-alpha-2".into()),
        unread: Some(false),
        event_watermark: Some(20),
        queued_count: 0,
        pending_interaction: Some(CindyPendingInteraction {
            interaction_id: "perm-alpha-2".into(),
            interaction_type: CindyInteractionType::Permission,
            tool_name: Some("bash".into()),
            options: None,
        }),
    });
    cindy_sessions.refresh_now();
    if let Some(cur_alpha1) = cindy_sessions.selection(&alpha1) {
        queued_open(&mut cindy_sessions, &mut receipts, &cur_alpha1);
    }

    let stops_before = fixture.command_counts().2;
    queued_command(
        &mut cindy_sessions,
        &mut receipts,
        &CindyQueuedCommand::Stop {
            address: alpha1.clone(),
            expected_run_id: "run-stale-0".into(),
            command_id: Some("cmd-stop-stale".into()),
        },
    );
    let stale_stop_reply = cindy_sessions.last_command_reply.clone();
    let stops_after_stale = fixture.command_counts().2;
    queued_command(
        &mut cindy_sessions,
        &mut receipts,
        &CindyQueuedCommand::Stop {
            address: alpha1.clone(),
            expected_run_id: "run-alpha-1".into(),
            command_id: Some("cmd-stop-exact".into()),
        },
    );
    let exact_stop_reply = cindy_sessions.last_command_reply.clone();
    let stops_after_exact = fixture.command_counts().2;
    queued_command(
        &mut cindy_sessions,
        &mut receipts,
        &CindyQueuedCommand::Stop {
            address: alpha1.clone(),
            expected_run_id: "run-alpha-1".into(),
            command_id: Some("cmd-stop-after-idle".into()),
        },
    );
    let after_idle_stop_reply = cindy_sessions.last_command_reply.clone();
    let stops_after_idle = fixture.command_counts().2;
    let stop_bound_pass = stale_stop_reply.as_ref().map(|r| r.status)
        == Some(CindyCommandResult::Noop)
        && stale_stop_reply
            .as_ref()
            .and_then(|r| r.reason_code.as_deref())
            == Some("guard.stopTargetChanged")
        && stops_after_stale == stops_before
        && exact_stop_reply.as_ref().map(|r| r.status) == Some(CindyCommandResult::Stopped)
        && stops_after_exact == stops_before + 1
        && after_idle_stop_reply.as_ref().map(|r| r.status) == Some(CindyCommandResult::Noop)
        && after_idle_stop_reply
            .as_ref()
            .and_then(|r| r.reason_code.as_deref())
            == Some("guard.notRunning")
        && stops_after_idle == stops_before + 1;
    checks.push(CindyScenarioCheckReceipt {
        name: "stop_bound_to_run_and_stale_noop".into(),
        status: if stop_bound_pass { "PASS" } else { "FAIL" }.into(),
        detail: format!(
            "stale={} exact={} stops={}",
            stale_stop_reply
                .as_ref()
                .map_or("nil", |r| r.status.as_str()),
            exact_stop_reply
                .as_ref()
                .map_or("nil", |r| r.status.as_str()),
            stops_after_idle - stops_before
        ),
    });

    if let Some(sel_alpha2) = cindy_sessions.selection(&alpha2) {
        queued_open(&mut cindy_sessions, &mut receipts, &sel_alpha2);
    }
    let resolves_before = fixture.command_counts().3;
    queued_command(
        &mut cindy_sessions,
        &mut receipts,
        &CindyQueuedCommand::Resolve {
            address: alpha2.clone(),
            interaction_id: "perm-alpha-2".into(),
            interaction_type: CindyInteractionType::Question,
            decision: CindyInteractionDecision::Approve,
            option_id: None,
            command_id: Some("cmd-resolve-q-guard".into()),
        },
    );
    let question_guard_reply = cindy_sessions.last_command_reply.clone();
    queued_command(
        &mut cindy_sessions,
        &mut receipts,
        &CindyQueuedCommand::Resolve {
            address: alpha2.clone(),
            interaction_id: "perm-expired-0".into(),
            interaction_type: CindyInteractionType::Permission,
            decision: CindyInteractionDecision::Approve,
            option_id: None,
            command_id: Some("cmd-resolve-expired".into()),
        },
    );
    let expired_resolve_reply = cindy_sessions.last_command_reply.clone();
    let resolves_after_guards = fixture.command_counts().3;
    queued_command(
        &mut cindy_sessions,
        &mut receipts,
        &CindyQueuedCommand::Resolve {
            address: alpha2.clone(),
            interaction_id: "perm-alpha-2".into(),
            interaction_type: CindyInteractionType::Permission,
            decision: CindyInteractionDecision::Approve,
            option_id: None,
            command_id: Some("cmd-resolve-valid".into()),
        },
    );
    let valid_resolve_reply = cindy_sessions.last_command_reply.clone();
    queued_command(
        &mut cindy_sessions,
        &mut receipts,
        &CindyQueuedCommand::Resolve {
            address: alpha2.clone(),
            interaction_id: "perm-alpha-2".into(),
            interaction_type: CindyInteractionType::Permission,
            decision: CindyInteractionDecision::Approve,
            option_id: None,
            command_id: Some("cmd-resolve-after".into()),
        },
    );
    let after_resolve_reply = cindy_sessions.last_command_reply.clone();
    let resolves_after_all = fixture.command_counts().3;
    let resolve_bound_pass = question_guard_reply.as_ref().map(|r| r.status)
        == Some(CindyCommandResult::InteractionTypeMismatch)
        && question_guard_reply
            .as_ref()
            .and_then(|r| r.reason_code.as_deref())
            == Some("guard.questionCannotApproveAsPermission")
        && expired_resolve_reply.as_ref().map(|r| r.status) == Some(CindyCommandResult::Noop)
        && expired_resolve_reply
            .as_ref()
            .and_then(|r| r.reason_code.as_deref())
            == Some("guard.interactionAlreadyResolved")
        && resolves_after_guards == resolves_before
        && valid_resolve_reply.as_ref().map(|r| r.status) == Some(CindyCommandResult::Resolved)
        && after_resolve_reply.as_ref().map(|r| r.status) == Some(CindyCommandResult::Noop)
        && resolves_after_all == resolves_before + 1;
    checks.push(CindyScenarioCheckReceipt {
        name: "resolve_bound_and_question_guard".into(),
        status: if resolve_bound_pass { "PASS" } else { "FAIL" }.into(),
        detail: format!(
            "q_guard={} valid={} resolves={}",
            question_guard_reply
                .as_ref()
                .map_or("nil", |r| r.status.as_str()),
            valid_resolve_reply
                .as_ref()
                .map_or("nil", |r| r.status.as_str()),
            resolves_after_all - resolves_before
        ),
    });

    let beta_scope = CindyScope::new("/Users/example/Projects/beta");
    queued_command(
        &mut cindy_sessions,
        &mut receipts,
        &CindyQueuedCommand::Create {
            scope: beta_scope.clone(),
            text: None,
            command_id: Some("cmd-create-beta".into()),
        },
    );
    let create_reply = cindy_sessions.last_command_reply.clone();
    let created_addr = create_reply.as_ref().and_then(|r| r.selected.clone());
    if let Some(sel_beta1) = cindy_sessions.selection(&beta1) {
        queued_open(&mut cindy_sessions, &mut receipts, &sel_beta1);
    }
    let sends_before = fixture.command_counts().1;
    queued_command(
        &mut cindy_sessions,
        &mut receipts,
        &CindyQueuedCommand::Send {
            address: beta1.clone(),
            text: "Steer while idle".into(),
            delivery_mode: CindyDeliveryMode::Steer,
            command_id: Some("cmd-steer-idle".into()),
        },
    );
    let steer_idle_reply = cindy_sessions.last_command_reply.clone();
    queued_command(
        &mut cindy_sessions,
        &mut receipts,
        &CindyQueuedCommand::Send {
            address: beta1.clone(),
            text: "Start turn".into(),
            delivery_mode: CindyDeliveryMode::Auto,
            command_id: Some("cmd-send-start".into()),
        },
    );
    let start_send_reply = cindy_sessions.last_command_reply.clone();
    queued_command(
        &mut cindy_sessions,
        &mut receipts,
        &CindyQueuedCommand::Send {
            address: beta1.clone(),
            text: "Steer running turn".into(),
            delivery_mode: CindyDeliveryMode::Steer,
            command_id: Some("cmd-send-steer".into()),
        },
    );
    let steer_running_reply = cindy_sessions.last_command_reply.clone();
    fixture.set_timeout_next_command(true);
    queued_command(
        &mut cindy_sessions,
        &mut receipts,
        &CindyQueuedCommand::Send {
            address: beta1.clone(),
            text: "Queue follow-up".into(),
            delivery_mode: CindyDeliveryMode::Queue,
            command_id: Some("cmd-send-timeout".into()),
        },
    );
    let timeout_send_reply = cindy_sessions.last_command_reply.clone();
    queued_command(
        &mut cindy_sessions,
        &mut receipts,
        &CindyQueuedCommand::Send {
            address: beta1.clone(),
            text: "Blocked while unknown".into(),
            delivery_mode: CindyDeliveryMode::Queue,
            command_id: Some("cmd-send-blocked".into()),
        },
    );
    let blocked_by_unknown_reply = cindy_sessions.last_command_reply.clone();
    queued_command(
        &mut cindy_sessions,
        &mut receipts,
        &CindyQueuedCommand::Query {
            scope: beta_scope.clone(),
            target: Some(beta1.clone()),
            command_id: "cmd-send-timeout".into(),
        },
    );
    let reconciled_reply = cindy_sessions.last_command_reply.clone();
    let sends_after = fixture.command_counts().1;
    let create_and_send_pass = create_reply.as_ref().map(|r| r.status)
        == Some(CindyCommandResult::Confirmed)
        && created_addr.as_ref().map(|a| a.workspace_path.as_str())
            == Some(beta_scope.workspace_path.as_str())
        && steer_idle_reply.as_ref().map(|r| r.status) == Some(CindyCommandResult::Denied)
        && start_send_reply.as_ref().map(|r| r.status) == Some(CindyCommandResult::Started)
        && steer_running_reply.as_ref().map(|r| r.status) == Some(CindyCommandResult::Steered)
        && timeout_send_reply.as_ref().map(|r| r.status) == Some(CindyCommandResult::Unknown)
        && blocked_by_unknown_reply.as_ref().map(|r| r.status) == Some(CindyCommandResult::Unknown)
        && blocked_by_unknown_reply
            .as_ref()
            .and_then(|r| r.reason_code.as_deref())
            == Some("guard.unknownOutcomeRequiresReconcile")
        && reconciled_reply.as_ref().map(|r| r.status) == Some(CindyCommandResult::Queued)
        && reconciled_reply.as_ref().map(|r| r.duplicate) == Some(true)
        && cindy_sessions.pending_unknown_command.is_none()
        && sends_after == sends_before + 3;
    checks.push(CindyScenarioCheckReceipt {
        name: "workspace_create_and_target_prompt_steering".into(),
        status: if create_and_send_pass { "PASS" } else { "FAIL" }.into(),
        detail: format!(
            "create={} start={} steer={} reconcile={}",
            create_reply.as_ref().map_or("nil", |r| r.status.as_str()),
            start_send_reply
                .as_ref()
                .map_or("nil", |r| r.status.as_str()),
            steer_running_reply
                .as_ref()
                .map_or("nil", |r| r.status.as_str()),
            reconciled_reply
                .as_ref()
                .map_or("nil", |r| r.status.as_str())
        ),
    });

    let completed_read_addr =
        CindyTaskAddress::new("/Users/example/Projects/alpha", "sess-completed-read");
    let completed_unknown_addr =
        CindyTaskAddress::new("/Users/example/Projects/alpha", "sess-completed-unknown");
    let completed_unread_addr =
        CindyTaskAddress::new("/Users/example/Projects/alpha", "sess-completed-unread");
    let error_unread_addr =
        CindyTaskAddress::new("/Users/example/Projects/beta", "sess-error-unread");
    fixture.upsert_task(CindyTaskRow {
        address: completed_read_addr.clone(),
        title: "Completed Read".into(),
        phase: "completed".into(),
        attention: false,
        pinned: false,
        run_id: None,
        unread: Some(false),
        event_watermark: Some(30),
        queued_count: 0,
        pending_interaction: None,
    });
    fixture.upsert_task(CindyTaskRow {
        address: completed_unknown_addr.clone(),
        title: "Completed Unknown Unread".into(),
        phase: "completed".into(),
        attention: false,
        pinned: false,
        run_id: None,
        unread: None,
        event_watermark: Some(31),
        queued_count: 0,
        pending_interaction: None,
    });
    fixture.upsert_task(CindyTaskRow {
        address: completed_unread_addr.clone(),
        title: "Completed Unread".into(),
        phase: "completed".into(),
        attention: true,
        pinned: false,
        run_id: None,
        unread: Some(true),
        event_watermark: Some(32),
        queued_count: 0,
        pending_interaction: None,
    });
    fixture.upsert_task(CindyTaskRow {
        address: error_unread_addr.clone(),
        title: "Error Unread".into(),
        phase: "error".into(),
        attention: true,
        pinned: false,
        run_id: None,
        unread: Some(true),
        event_watermark: Some(33),
        queued_count: 0,
        pending_interaction: None,
    });
    cindy_sessions.refresh_now();
    cindy_sessions.reset_attention_round();
    queued_command(
        &mut cindy_sessions,
        &mut receipts,
        &CindyQueuedCommand::NextAttention,
    );
    let first_round_pick = cindy_sessions
        .last_command_reply
        .as_ref()
        .and_then(|r| r.selected.clone());
    queued_command(
        &mut cindy_sessions,
        &mut receipts,
        &CindyQueuedCommand::NextAttention,
    );
    let second_round_pick = cindy_sessions
        .last_command_reply
        .as_ref()
        .and_then(|r| r.selected.clone());
    queued_command(
        &mut cindy_sessions,
        &mut receipts,
        &CindyQueuedCommand::NextAttention,
    );
    let third_round_exhausted = cindy_sessions.last_command_reply.clone();
    fixture.upsert_task(CindyTaskRow {
        address: completed_unread_addr.clone(),
        title: "Completed Unread".into(),
        phase: "completed".into(),
        attention: true,
        pinned: false,
        run_id: None,
        unread: Some(true),
        event_watermark: Some(34),
        queued_count: 0,
        pending_interaction: None,
    });
    cindy_sessions.refresh_now();
    queued_command(
        &mut cindy_sessions,
        &mut receipts,
        &CindyQueuedCommand::NextAttention,
    );
    let reentered_pick = cindy_sessions
        .last_command_reply
        .as_ref()
        .and_then(|r| r.selected.clone());
    let unread_and_round_pass = first_round_pick.as_ref() == Some(&error_unread_addr)
        && second_round_pick.as_ref() == Some(&completed_unread_addr)
        && first_round_pick.as_ref() != Some(&completed_read_addr)
        && first_round_pick.as_ref() != Some(&completed_unknown_addr)
        && second_round_pick.as_ref() != Some(&completed_read_addr)
        && second_round_pick.as_ref() != Some(&completed_unknown_addr)
        && third_round_exhausted.as_ref().map(|r| r.status) == Some(CindyCommandResult::Noop)
        && third_round_exhausted
            .as_ref()
            .and_then(|r| r.reason_code.as_deref())
            == Some("round.exhausted")
        && reentered_pick.as_ref() == Some(&completed_unread_addr);
    checks.push(CindyScenarioCheckReceipt {
        name: "unread_tristate_and_pending_round".into(),
        status: if unread_and_round_pass {
            "PASS"
        } else {
            "FAIL"
        }
        .into(),
        detail: format!(
            "first={} second={} exhausted={} reentered={}",
            first_round_pick
                .as_ref()
                .map_or("nil", |a| a.session_id.as_str()),
            second_round_pick
                .as_ref()
                .map_or("nil", |a| a.session_id.as_str()),
            third_round_exhausted
                .as_ref()
                .map_or("nil", |r| r.status.as_str()),
            reentered_pick
                .as_ref()
                .map_or("nil", |a| a.session_id.as_str())
        ),
    });

    let final_stats = fixture.stats();
    let all_pass =
        checks.len() == 13 && checks.iter().all(|c| c.status == "PASS") && final_stats.1 == 0;
    let report = CindyNavigationScenarioReport {
        scenario: "cindy-navigation".into(),
        status: if all_pass { "PASS" } else { "FAIL" }.into(),
        unauthorized_metadata_reads: final_stats.1,
        authorized_metadata_reads: final_stats.0,
        local_agent_api_relaxed: false,
        checks,
        native_app: "NOT_RUN".into(),
        physical_key: "NOT_RUN".into(),
        receipts,
    };

    if let Some(path) = audit_log_path {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            std::fs::write(
                parent.join("cindy-navigation.json"),
                serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        }
        let lines: String = report
            .receipts
            .iter()
            .map(|r| format!("{}\n", serde_json::to_string(r).unwrap()))
            .collect();
        std::fs::write(path, lines).map_err(|e| e.to_string())?;
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unread_tristate_never_fabricates_unread_for_completed_sessions() {
        let addr = CindyTaskAddress::new("/ws/alpha", "sess-1");
        let completed_false = CindyTaskRow {
            address: addr.clone(),
            title: "Done False".into(),
            phase: "completed".into(),
            attention: false,
            pinned: false,
            run_id: None,
            unread: Some(false),
            event_watermark: Some(1),
            queued_count: 0,
            pending_interaction: None,
        };
        let completed_none = CindyTaskRow {
            address: addr.clone(),
            title: "Done None".into(),
            phase: "completed".into(),
            attention: false,
            pinned: false,
            run_id: None,
            unread: None,
            event_watermark: Some(2),
            queued_count: 0,
            pending_interaction: None,
        };
        let completed_true = CindyTaskRow {
            address: addr,
            title: "Done True".into(),
            phase: "completed".into(),
            attention: true,
            pinned: false,
            run_id: None,
            unread: Some(true),
            event_watermark: Some(3),
            queued_count: 0,
            pending_interaction: None,
        };
        assert!(!completed_false.is_eligible_for_attention_round());
        assert!(!completed_none.is_eligible_for_attention_round());
        assert!(completed_true.is_eligible_for_attention_round());
        assert_eq!(completed_true.attention_priority_rank(), 2);
    }

    #[test]
    fn run_bound_stop_and_question_as_permission_guard_and_epoch_rotation() {
        let fixture = Arc::new(CindyNavigationFixture::new());
        let mut ctrl = CindySessionController::new(fixture.clone());
        ctrl.set_enabled(true);
        ctrl.refresh_now();

        let alpha1 = CindyTaskAddress::new("/Users/example/Projects/alpha", "sess-alpha-1");
        let alpha2 = CindyTaskAddress::new("/Users/example/Projects/alpha", "sess-alpha-2");
        let sel1 = ctrl.selection(&alpha1).unwrap();
        assert_eq!(ctrl.open(Some(&sel1), true), CindyOpenResult::Confirmed);

        let stale_stop = ctrl.stop(&alpha1, "wrong-run-id", "cmd-stop-1", true);
        assert_eq!(stale_stop.status, CindyCommandResult::Noop);
        assert_eq!(
            stale_stop.reason_code.as_deref(),
            Some("guard.stopTargetChanged")
        );

        let sel2 = ctrl.selection(&alpha2).unwrap();
        assert_eq!(ctrl.open(Some(&sel2), true), CindyOpenResult::Confirmed);
        let q_guard = ctrl.resolve(
            &alpha2,
            ("perm-alpha-2", CindyInteractionType::Question),
            CindyInteractionDecision::Approve,
            None,
            "cmd-q-guard",
            true,
        );
        assert_eq!(q_guard.status, CindyCommandResult::InteractionTypeMismatch);
        assert_eq!(
            q_guard.reason_code.as_deref(),
            Some("guard.questionCannotApproveAsPermission")
        );

        fixture.set_open_override(CindyOpenOverride::RotateGeneration(99));
        assert_eq!(ctrl.open(Some(&sel2), true), CindyOpenResult::Stale);
        assert_eq!(
            ctrl.state,
            CindyConnectionState::Unavailable(CindyFailure::Stale)
        );
        assert!(ctrl.rows.is_empty());
        assert!(ctrl.selected.is_none());
    }
}
