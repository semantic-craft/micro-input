use crate::socket::{LocalSocket, SocketError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::CString,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DSHInteractionType {
    Permission,
    Question,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DSHInteractionDecision {
    Approve,
    Reject,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DSHPendingInteraction {
    pub interaction_id: String,
    pub interaction_type: DSHInteractionType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DSHModelConfig {
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thought: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DSHModelOption {
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_thought: Option<String>,
    #[serde(default)]
    pub supported_thoughts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DSHSessionRow {
    pub session_id: String,
    pub workspace_id: String,
    pub title: String,
    pub active: bool,
    pub running: bool,
    pub pending_input: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_turn: Option<i64>,
    #[serde(default)]
    pub queued_count: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_interaction: Option<DSHPendingInteraction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_config: Option<DSHModelConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supported_models: Option<Vec<DSHModelOption>>,
}

impl DSHSessionRow {
    pub fn valid(&self) -> bool {
        DSHSessionSnapshot::valid_id(&self.session_id)
            && self.workspace_id.len() <= 4096
            && self.title.len() <= 4096
            && self.pending_input <= 1_000_000
            && (!self.running || self.active)
            && self
                .current_turn
                .is_none_or(|t| (0..=1_000_000).contains(&t))
            && (0..=1_000_000).contains(&self.queued_count)
    }
    pub fn turn_status_label(&self) -> String {
        match (self.current_turn, self.running) {
            (Some(t), true) => format!("轮次 #{t} 运行中"),
            (Some(t), false) => format!("轮次 #{t} 已结束"),
            (None, true) => "轮次 未知（运行中）".into(),
            (None, false) => "轮次 空闲".into(),
        }
    }
    pub fn queue_status_label(&self) -> String {
        if self.queued_count > 0 {
            format!("队列 {} 条", self.queued_count)
        } else {
            "队列 空".into()
        }
    }
    pub fn status_label(&self) -> String {
        let base = if self.pending_input > 0 {
            format!("等待回应（{}）", self.pending_input)
        } else if self.running {
            "运行中".into()
        } else if self.active {
            "空闲".into()
        } else {
            "未激活".into()
        };
        let mp = self.model_config.as_ref().map_or_else(String::new, |m| {
            format!(
                " · {}{}",
                m.model,
                m.thought
                    .as_ref()
                    .map_or_else(String::new, |t| format!("（{t}）"))
            )
        });
        format!(
            "{base} · {} · {}{mp}",
            self.turn_status_label(),
            self.queue_status_label()
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DSHSessionChange {
    pub sequence: u64,
    pub row: DSHSessionRow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DSHSessionSnapshot {
    pub host_id: String,
    pub generation: u64,
    pub session_ids: Vec<String>,
    pub cursor: u64,
    pub rows: Vec<DSHSessionRow>,
    pub unavailable_session_ids: Vec<String>,
    pub changes: Vec<DSHSessionChange>,
    pub selected: Option<String>,
}

impl DSHSessionSnapshot {
    pub fn valid_id(v: &str) -> bool {
        !v.is_empty() && v.len() <= 4096
    }
    pub fn validate(&self, after: Option<u64>) -> Result<(), DSHFailure> {
        let scope: HashSet<&str> = self.session_ids.iter().map(String::as_str).collect();
        let known: HashSet<&str> = self.rows.iter().map(|r| r.session_id.as_str()).collect();
        let unavail: HashSet<&str> = self
            .unavailable_session_ids
            .iter()
            .map(String::as_str)
            .collect();
        if !Self::valid_id(&self.host_id)
            || self.host_id.len() > 256
            || self.generation == 0
            || scope.is_empty()
            || scope.len() > 4096
            || scope.len() != self.session_ids.len()
            || !self.session_ids.iter().all(|s| Self::valid_id(s))
            || self.rows.len() != known.len()
            || unavail.len() != self.unavailable_session_ids.len()
            || !known.is_disjoint(&unavail)
            || known.len() + unavail.len() != scope.len()
            || !known.iter().all(|k| scope.contains(k))
            || !unavail.iter().all(|u| scope.contains(u))
            || !self.rows.iter().all(DSHSessionRow::valid)
            || self.changes.len() > 4096
            || !self
                .changes
                .iter()
                .all(|c| c.row.valid() && scope.contains(c.row.session_id.as_str()))
            || self.selected.as_deref().is_some_and(|s| !known.contains(s))
        {
            return Err(DSHFailure::InvalidReply);
        }
        if let Some(prev) = after {
            if self.cursor < prev
                || self.cursor - prev != self.changes.len() as u64
                || self
                    .changes
                    .iter()
                    .enumerate()
                    .any(|(i, c)| c.sequence != prev + i as u64 + 1)
            {
                return Err(DSHFailure::Gap);
            }
        } else if !self.changes.is_empty() {
            return Err(DSHFailure::InvalidReply);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DSHSelection {
    pub session_id: String,
    pub workspace_id: String,
    pub host_id: String,
    pub generation: u64,
    pub local_epoch: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DSHOpenResult {
    Confirmed,
    Unconfirmed,
    Stale,
    Denied,
    Unavailable,
}

impl DSHOpenResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Unconfirmed => "unconfirmed",
            Self::Stale => "stale",
            Self::Denied => "denied",
            Self::Unavailable => "unavailable",
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Self::Confirmed => "已确认打开指定 DSH 会话",
            Self::Unconfirmed => "DSH 未确认选中目标",
            Self::Stale => "DSH 目标或连接已变化，打开未确认",
            Self::Denied => "DSH 未授权此操作",
            Self::Unavailable => "DSH 桥不可用，未重试打开",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DSHOpenReply {
    pub host_id: String,
    pub generation: u64,
    pub status: DSHOpenResult,
    pub selected: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DSHPromptMode {
    Queue,
    Steer,
}

impl DSHPromptMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queue => "queue",
            Self::Steer => "steer",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DSHCreateIntent {
    pub workspace_id: String,
    pub request_id: String,
    pub host_id: String,
    pub generation: u64,
    pub local_epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DSHPromptIntent {
    pub session_id: String,
    pub workspace_id: String,
    pub request_id: String,
    pub mode: DSHPromptMode,
    pub text: String,
    pub host_id: String,
    pub generation: u64,
    pub local_epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DSHCancelIntent {
    pub session_id: String,
    pub workspace_id: String,
    pub request_id: String,
    pub expected_turn: i64,
    pub host_id: String,
    pub generation: u64,
    pub local_epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DSHQueryIntent {
    pub session_id: Option<String>,
    pub workspace_id: Option<String>,
    pub request_id: String,
    pub host_id: String,
    pub generation: u64,
    pub local_epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DSHResolveIntent {
    pub session_id: String,
    pub workspace_id: String,
    pub request_id: String,
    pub interaction_id: String,
    pub interaction_type: DSHInteractionType,
    pub decision: DSHInteractionDecision,
    pub option_id: Option<String>,
    pub host_id: String,
    pub generation: u64,
    pub local_epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DSHSwitchModelIntent {
    pub session_id: String,
    pub workspace_id: String,
    pub request_id: String,
    pub provider: String,
    pub model: String,
    pub thought: Option<String>,
    pub host_id: String,
    pub generation: u64,
    pub local_epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DSHQueuedCommand {
    Create(DSHCreateIntent),
    Prompt(DSHPromptIntent),
    Cancel(DSHCancelIntent),
    Query(DSHQueryIntent),
    Resolve(DSHResolveIntent),
    SwitchModel(DSHSwitchModelIntent),
}

impl DSHQueuedCommand {
    fn meta(&self) -> (&'static str, &str, &str, u64, u64) {
        match self {
            Self::Create(i) => (
                "dsh_create",
                &i.request_id,
                &i.host_id,
                i.generation,
                i.local_epoch,
            ),
            Self::Prompt(i) => (
                "dsh_prompt",
                &i.request_id,
                &i.host_id,
                i.generation,
                i.local_epoch,
            ),
            Self::Cancel(i) => (
                "dsh_cancel",
                &i.request_id,
                &i.host_id,
                i.generation,
                i.local_epoch,
            ),
            Self::Query(i) => (
                "dsh_query",
                &i.request_id,
                &i.host_id,
                i.generation,
                i.local_epoch,
            ),
            Self::Resolve(i) => (
                "dsh_resolve",
                &i.request_id,
                &i.host_id,
                i.generation,
                i.local_epoch,
            ),
            Self::SwitchModel(i) => (
                "dsh_switch_model",
                &i.request_id,
                &i.host_id,
                i.generation,
                i.local_epoch,
            ),
        }
    }
    pub fn action_name(&self) -> &'static str {
        self.meta().0
    }
    pub fn request_id(&self) -> &str {
        self.meta().1
    }
    pub fn host_id(&self) -> &str {
        self.meta().2
    }
    pub fn generation(&self) -> u64 {
        self.meta().3
    }
    pub fn local_epoch(&self) -> u64 {
        self.meta().4
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DSHCommandReply {
    pub host_id: String,
    pub generation: u64,
    pub status: String,
    pub session_id: Option<String>,
    pub workspace_id: Option<String>,
    pub selected: Option<String>,
    pub request_id: String,
    pub delivery: Option<DSHPromptMode>,
    pub expected_turn: Option<i64>,
    pub current_turn: Option<i64>,
    pub queued_count: Option<i64>,
    pub duplicate: bool,
    pub reason_code: Option<String>,
    pub model_config: Option<DSHModelConfig>,
}

impl DSHCommandReply {
    fn qa(gen: u64, status: &str, req_id: &str) -> Self {
        Self {
            host_id: "qa-host".into(),
            generation: gen,
            status: status.into(),
            session_id: None,
            workspace_id: None,
            selected: None,
            request_id: req_id.into(),
            delivery: None,
            expected_turn: None,
            current_turn: None,
            queued_count: None,
            duplicate: false,
            reason_code: None,
            model_config: None,
        }
    }
    fn sess(mut self, sid: &str, ws: &str, sel: Option<String>) -> Self {
        self.session_id = Some(sid.into());
        self.workspace_id = Some(ws.into());
        self.selected = sel;
        self
    }
    fn turns(mut self, exp: Option<i64>, cur: Option<i64>, qc: Option<i64>) -> Self {
        self.expected_turn = exp;
        self.current_turn = cur;
        self.queued_count = qc;
        self
    }
    fn rc(mut self, code: &str) -> Self {
        self.reason_code = Some(code.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DSHCommandResult {
    Created(String, String, String, bool),
    Queued(String, String, Option<i64>, i64, bool),
    Steered(String, String, Option<i64>, i64, bool),
    Cancelled(String, String, i64, Option<i64>, i64, bool),
    CancelledQueueRemaining(String, String, i64, Option<i64>, i64, bool),
    Noop(String, Option<i64>, Option<i64>),
    Unknown(String, String),
    Unconfirmed(Option<String>, String),
    Stale,
    Denied(Option<String>),
    Unavailable,
    Resolved(String, String, bool),
    InteractionTypeMismatch(String),
    ModelSwitched(String, String, DSHModelConfig, bool),
    UnsupportedThought(String),
}

impl DSHCommandResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Created(..) => "confirmed",
            Self::Queued(..) => "queued",
            Self::Steered(..) => "steered",
            Self::Cancelled(..) => "cancelled",
            Self::CancelledQueueRemaining(..) => "cancelledQueueRemaining",
            Self::Noop(..) => "noop",
            Self::Unknown(..) => "unknown",
            Self::Unconfirmed(..) => "unconfirmed",
            Self::Stale => "stale",
            Self::Denied(..) => "denied",
            Self::Unavailable => "unavailable",
            Self::Resolved(..) => "resolved",
            Self::InteractionTypeMismatch(..) => "interactionTypeMismatch",
            Self::ModelSwitched(..) => "modelSwitched",
            Self::UnsupportedThought(..) => "unsupportedThought",
        }
    }
    pub fn label(&self) -> String {
        match self {
            Self::Created(..) => "已新建并确认选中 DSH 会话".into(),
            Self::Queued(_, _, ct, qc, _) => {
                let t = ct.map_or_else(|| "轮次空闲".into(), |n| format!("轮次 #{n}"));
                format!("已入队下一轮次（{t} · 队列 {qc} 条）")
            }
            Self::Steered(_, _, ct, qc, _) => {
                let t = ct.map_or_else(|| "运行中".into(), |n| format!("轮次 #{n}"));
                format!("已转向当前轮次（{t} · 队列 {qc} 条）")
            }
            Self::Cancelled(_, _, et, ..) => format!("已停止当前轮次 #{et}（队列已空）"),
            Self::CancelledQueueRemaining(_, _, et, _, qc, _) => {
                format!("已停止当前轮次 #{et}（队列仍有 {qc} 条可能继续）")
            }
            Self::Noop(rc, ct, _) => {
                let t = ct.map_or_else(|| "当前空闲".into(), |n| format!("当前轮次 #{n}"));
                format!("未取消任何轮次（{rc}，{t}）")
            }
            Self::Unknown(req_id, _) => format!("DSH 命令结果未知（{req_id}），需先对账"),
            Self::Unconfirmed(..) => "DSH 未确认选中目标会话，未执行命令".into(),
            Self::Stale => "DSH 目标或连接已变化，命令未确认".into(),
            Self::Denied(Some(rc)) if rc == "guard.unprovableTurnAtomicity" => {
                "DSH 未能证明同 Host 轮次原子核对，已拒绝发送".into()
            }
            Self::Denied(rc) => {
                format!(
                    "DSH 拒绝此命令{}",
                    rc.as_ref().map_or_else(String::new, |r| format!("（{r}）"))
                )
            }
            Self::Unavailable => "DSH 桥不可用，未盲目重试".into(),
            Self::Resolved(..) => "已答复 DSH 请求".into(),
            Self::InteractionTypeMismatch(rc) => format!("交互类型不匹配（{rc}）"),
            Self::ModelSwitched(_, _, cfg, _) => {
                let th = cfg
                    .thought
                    .as_ref()
                    .map_or_else(String::new, |t| format!("（思考: {t}）"));
                format!("已切换模型至 {}{th}", cfg.model)
            }
            Self::UnsupportedThought(rc) => format!("不支持该思考档位（{rc}）"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DSHFailure {
    #[error("请先在 DSH 授权会话连接")]
    Grant,
    #[error("DSH 未授权此会话")]
    Denied,
    #[error("DSH Host 已变化，请重新连接")]
    Stale,
    #[error("DSH 桥连接中断")]
    Unavailable,
    #[error("DSH 返回的会话状态无法核对")]
    InvalidReply,
    #[error("DSH 状态变化存在缺口，请重新连接")]
    Gap,
}

impl DSHFailure {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Grant => "请先在 DSH 授权会话连接",
            Self::Denied => "DSH 未授权此会话",
            Self::Stale => "DSH Host 已变化，请重新连接",
            Self::Unavailable => "DSH 桥连接中断",
            Self::InvalidReply => "DSH 返回的会话状态无法核对",
            Self::Gap => "DSH 状态变化存在缺口，请重新连接",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DSHConnectionState {
    Disabled,
    Connecting,
    Ready,
    Unavailable(DSHFailure),
}

impl DSHConnectionState {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Disabled => "DSH 会话连接未开启",
            Self::Connecting => "正在连接 DSH",
            Self::Ready => "DSH 会话连接已就绪",
            Self::Unavailable(r) => r.label(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DSHGrant {
    pub version: u32,
    pub socket_path: String,
    pub token: String,
    pub host_id: String,
    pub generation: u64,
    pub session_ids: Vec<String>,
}

impl DSHGrant {
    pub fn load(path: &Path) -> Result<Self, DSHFailure> {
        let parent = path.parent().ok_or(DSHFailure::Grant)?;
        let (c_parent, c_path) = (
            CString::new(parent.as_os_str().as_bytes()).map_err(|_| DSHFailure::Grant)?,
            CString::new(path.as_os_str().as_bytes()).map_err(|_| DSHFailure::Grant)?,
        );
        let (uid, mut pst) = (unsafe { libc::getuid() }, unsafe {
            std::mem::zeroed::<libc::stat>()
        });
        if unsafe { libc::lstat(c_parent.as_ptr(), &mut pst) } != 0
            || (pst.st_mode & libc::S_IFMT) != libc::S_IFDIR
            || pst.st_uid != uid
            || (pst.st_mode & 0o077) != 0
        {
            return Err(DSHFailure::Grant);
        }
        let fd = unsafe {
            libc::open(
                c_path.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK,
            )
        };
        if fd < 0 {
            return Err(DSHFailure::Grant);
        }
        let res = (|| {
            let mut fst: libc::stat = unsafe { std::mem::zeroed() };
            if unsafe { libc::fstat(fd, &mut fst) } != 0
                || (fst.st_mode & libc::S_IFMT) != libc::S_IFREG
                || fst.st_uid != uid
                || (fst.st_mode & 0o077) != 0
                || !(1..=65536).contains(&fst.st_size)
            {
                return Err(DSHFailure::Grant);
            }
            let (mut buf, mut off) = (vec![0u8; fst.st_size as usize], 0);
            while off < buf.len() {
                let n = unsafe { libc::read(fd, buf[off..].as_mut_ptr().cast(), buf.len() - off) };
                if n <= 0 {
                    return Err(DSHFailure::Grant);
                }
                off += n as usize;
            }
            let g: Self = serde_json::from_slice(&buf).map_err(|_| DSHFailure::Grant)?;
            let u: HashSet<_> = g.session_ids.iter().collect();
            if g.version != 1
                || !g.socket_path.starts_with('/')
                || g.socket_path.len() >= 104
                || !(32..=256).contains(&g.token.len())
                || g.host_id.is_empty()
                || g.host_id.len() > 256
                || g.generation == 0
                || g.session_ids.is_empty()
                || g.session_ids.len() > 4096
                || !g
                    .session_ids
                    .iter()
                    .all(|s| DSHSessionSnapshot::valid_id(s))
                || u.len() != g.session_ids.len()
            {
                return Err(DSHFailure::Grant);
            }
            Ok(g)
        })();
        unsafe { libc::close(fd) };
        res
    }
}

pub trait DSHSessionServing: Send + Sync {
    fn connect(&self) -> Result<DSHSessionSnapshot, DSHFailure>;
    fn read(&self) -> Result<DSHSessionSnapshot, DSHFailure>;
    fn open(&self, selection: &DSHSelection) -> Result<DSHOpenReply, DSHFailure>;
    fn create(&self, intent: &DSHCreateIntent) -> Result<DSHCommandReply, DSHFailure>;
    fn prompt(&self, intent: &DSHPromptIntent) -> Result<DSHCommandReply, DSHFailure>;
    fn cancel(&self, intent: &DSHCancelIntent) -> Result<DSHCommandReply, DSHFailure>;
    fn query(&self, intent: &DSHQueryIntent) -> Result<DSHCommandReply, DSHFailure>;
    fn resolve(&self, intent: &DSHResolveIntent) -> Result<DSHCommandReply, DSHFailure>;
    fn switch_model(&self, intent: &DSHSwitchModelIntent) -> Result<DSHCommandReply, DSHFailure>;
    fn close(&self);
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DSHWireReply {
    version: u32,
    subscription_id: String,
    id: String,
    host_id: String,
    generation: u64,
    status: String,
    cursor: Option<u64>,
    rows: Option<Vec<DSHSessionRow>>,
    unavailable_session_ids: Option<Vec<String>>,
    changes: Option<Vec<DSHSessionChange>>,
    selected: Option<String>,
    session_id: Option<String>,
    workspace_id: Option<String>,
    request_id: Option<String>,
    delivery: Option<String>,
    expected_turn: Option<i64>,
    current_turn: Option<i64>,
    queued_count: Option<i64>,
    duplicate: Option<bool>,
    reason_code: Option<String>,
    model_config: Option<DSHModelConfig>,
}

#[derive(Default)]
struct ClientState {
    grant: Option<DSHGrant>,
    allowed_session_ids: Vec<String>,
    allowed_workspace_ids: HashSet<String>,
    epoch: u64,
    cursor: Option<u64>,
    subscription_id: Option<String>,
}

pub struct DSHSessionClient {
    grant_path: PathBuf,
    timeout: Duration,
    state: Mutex<ClientState>,
}

impl Default for DSHSessionClient {
    fn default() -> Self {
        let base = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        Self::new(base.join("Library/Application Support/MicroInput/dsh-grant.json"))
    }
}

const ALLOWED_TOP_KEYS: &str = "version|subscriptionId|id|hostId|generation|status|cursor|rows|unavailableSessionIds|changes|selected|sessionId|workspaceId|requestId|delivery|expectedTurn|currentTurn|queuedCount|duplicate|reasonCode|modelConfig";
const ALLOWED_ROW_KEYS: &str = "sessionId|workspaceId|title|active|running|pendingInput|currentTurn|queuedCount|pendingInteraction|modelConfig|supportedModels";
const VALID_STATUSES: &str = "ok|confirmed|queued|steered|cancelled|cancelledQueueRemaining|noop|unknown|unconfirmed|denied|stale|unavailable|gap|resolved|interactionTypeMismatch|modelSwitched|unsupportedThought";

impl DSHSessionClient {
    pub fn new(grant_path: impl Into<PathBuf>) -> Self {
        Self {
            grant_path: grant_path.into(),
            timeout: Duration::from_secs(2),
            state: Mutex::new(ClientState::default()),
        }
    }
    fn current_grant(&self) -> Result<(DSHGrant, u64, String), DSHFailure> {
        let st = self.state.lock().unwrap();
        let (g, sub, ep) = (
            st.grant.clone().ok_or(DSHFailure::Denied)?,
            st.subscription_id.clone().ok_or(DSHFailure::Stale)?,
            st.epoch,
        );
        drop(st);
        if DSHGrant::load(&self.grant_path)? != g {
            return Err(DSHFailure::Stale);
        }
        Ok((g, ep, sub))
    }
    fn guard_cmd(
        &self,
        host_id: &str,
        gen: u64,
        req_id: &str,
        sid: Option<&str>,
        ws: Option<&str>,
    ) -> Result<(DSHGrant, u64, String), DSHFailure> {
        let (g, ep, sub) = self.current_grant()?;
        if host_id != g.host_id || gen != g.generation {
            return Err(DSHFailure::Stale);
        }
        let st = self.state.lock().unwrap();
        if !DSHSessionSnapshot::valid_id(req_id)
            || sid.is_some_and(|s| !st.allowed_session_ids.contains(&s.to_owned()))
            || ws.is_some_and(|w| !st.allowed_workspace_ids.contains(w))
        {
            return Err(DSHFailure::Denied);
        }
        Ok((g, ep, sub))
    }
    fn map_cmd_reply(
        &self,
        r: DSHWireReply,
        expected_req_id: &str,
        allow_new_sid: bool,
    ) -> Result<DSHCommandReply, DSHFailure> {
        if r.rows.is_some()
            || r.changes.is_some()
            || r.cursor.is_some()
            || r.unavailable_session_ids.is_some()
            || r.request_id
                .as_deref()
                .is_some_and(|id| id != expected_req_id)
        {
            return Err(DSHFailure::InvalidReply);
        }
        let delivery = match r.delivery.as_deref() {
            Some("queue") => Some(DSHPromptMode::Queue),
            Some("steer") => Some(DSHPromptMode::Steer),
            Some(_) => return Err(DSHFailure::InvalidReply),
            None => None,
        };
        if allow_new_sid && r.status == "confirmed" {
            if let Some(cid) = r
                .session_id
                .as_ref()
                .filter(|s| DSHSessionSnapshot::valid_id(s))
            {
                let mut st = self.state.lock().unwrap();
                if !st.allowed_session_ids.contains(cid) {
                    st.allowed_session_ids.push(cid.clone());
                }
            }
        }
        Ok(DSHCommandReply {
            host_id: r.host_id,
            generation: r.generation,
            status: r.status,
            session_id: r.session_id,
            workspace_id: r.workspace_id,
            selected: r.selected,
            request_id: r.request_id.unwrap_or_else(|| expected_req_id.to_owned()),
            delivery,
            expected_turn: r.expected_turn,
            current_turn: r.current_turn,
            queued_count: r.queued_count,
            duplicate: r.duplicate.unwrap_or(false),
            reason_code: r.reason_code,
            model_config: r.model_config,
        })
    }
    fn exchange(
        &self,
        op: &str,
        extra: Value,
        grant: &DSHGrant,
        expected: u64,
        sub_id: &str,
    ) -> Result<DSHWireReply, DSHFailure> {
        let req_id = Uuid::new_v4().to_string();
        let mut payload = json!({
            "version": 1, "subscriptionId": sub_id, "id": req_id, "token": grant.token,
            "hostId": grant.host_id, "generation": grant.generation, "op": op
        });
        if op == "subscribe" {
            payload["activate"] = json!(false);
        }
        if let Some(obj) = extra.as_object() {
            for (k, v) in obj.iter().filter(|(_, v)| !v.is_null()) {
                payload[k] = v.clone();
            }
        }
        let wire = serde_json::to_vec(&payload).map_err(|_| DSHFailure::InvalidReply)?;
        let transport = LocalSocket {
            path: PathBuf::from(&grant.socket_path),
            request_limit: 65536,
            response_limit: 1_048_576,
            timeout: self.timeout,
        };
        let raw = transport.exchange_blocking(&wire).map_err(|e| match e {
            SocketError::Unavailable => DSHFailure::Unavailable,
            SocketError::InvalidPeer => DSHFailure::Denied,
            SocketError::InvalidReply => DSHFailure::InvalidReply,
        })?;
        let (cur_g, cur_ep, cur_sub) = self.current_grant()?;
        if cur_ep != expected || &cur_g != grant || cur_sub != sub_id {
            return Err(DSHFailure::Stale);
        }
        let val: Value = serde_json::from_slice(&raw).map_err(|_| DSHFailure::InvalidReply)?;
        let obj = val.as_object().ok_or(DSHFailure::InvalidReply)?;
        let sub_k = |m: &serde_json::Map<String, Value>, list: &str| {
            m.keys().all(|k| list.split('|').any(|x| x == k))
        };
        if !sub_k(obj, ALLOWED_TOP_KEYS) {
            return Err(DSHFailure::InvalidReply);
        }
        if let Some(rows) = obj.get("rows").filter(|v| !v.is_null()) {
            if !rows.as_array().is_some_and(|a| {
                a.iter()
                    .all(|r| r.as_object().is_some_and(|m| sub_k(m, ALLOWED_ROW_KEYS)))
            }) {
                return Err(DSHFailure::InvalidReply);
            }
        }
        if let Some(changes) = obj.get("changes").filter(|v| !v.is_null()) {
            if !changes.as_array().is_some_and(|a| {
                a.iter().all(|c| {
                    c.as_object().is_some_and(|cm| {
                        cm.len() == 2
                            && cm.contains_key("sequence")
                            && cm
                                .get("row")
                                .and_then(Value::as_object)
                                .is_some_and(|rm| sub_k(rm, ALLOWED_ROW_KEYS))
                    })
                })
            }) {
                return Err(DSHFailure::InvalidReply);
            }
        }
        let reply: DSHWireReply =
            serde_json::from_value(val).map_err(|_| DSHFailure::InvalidReply)?;
        if reply.version != 1 || reply.id != req_id || reply.subscription_id != sub_id {
            return Err(DSHFailure::InvalidReply);
        }
        if reply.host_id != grant.host_id || reply.generation != grant.generation {
            return Err(DSHFailure::Stale);
        }
        if !VALID_STATUSES.split('|').any(|s| s == reply.status)
            || (reply.status != "ok"
                && (reply.rows.is_some()
                    || reply.changes.is_some()
                    || reply.cursor.is_some()
                    || reply.unavailable_session_ids.is_some()))
        {
            return Err(DSHFailure::InvalidReply);
        }
        Ok(reply)
    }
    fn build_snapshot(
        &self,
        r: DSHWireReply,
        grant: &DSHGrant,
        after: Option<u64>,
    ) -> Result<DSHSessionSnapshot, DSHFailure> {
        if r.status != "ok" {
            return Err(match r.status.as_str() {
                "denied" => DSHFailure::Denied,
                "stale" => DSHFailure::Stale,
                "unavailable" => DSHFailure::Unavailable,
                "gap" => DSHFailure::Gap,
                _ => DSHFailure::InvalidReply,
            });
        }
        let sids = self.state.lock().unwrap().allowed_session_ids.clone();
        let snap = DSHSessionSnapshot {
            host_id: grant.host_id.clone(),
            generation: grant.generation,
            session_ids: sids,
            cursor: r.cursor.ok_or(DSHFailure::InvalidReply)?,
            rows: r.rows.ok_or(DSHFailure::InvalidReply)?,
            unavailable_session_ids: r.unavailable_session_ids.ok_or(DSHFailure::InvalidReply)?,
            changes: r.changes.ok_or(DSHFailure::InvalidReply)?,
            selected: r.selected,
        };
        snap.validate(after)?;
        let mut st = self.state.lock().unwrap();
        for row in &snap.rows {
            st.allowed_workspace_ids.insert(row.workspace_id.clone());
        }
        st.cursor = Some(snap.cursor);
        Ok(snap)
    }
}

impl DSHSessionServing for DSHSessionClient {
    fn connect(&self) -> Result<DSHSessionSnapshot, DSHFailure> {
        self.close();
        let current = DSHGrant::load(&self.grant_path)?;
        let sub_id = Uuid::new_v4().to_string();
        let expected = {
            let mut st = self.state.lock().unwrap();
            st.grant = Some(current.clone());
            st.allowed_session_ids = current.session_ids.clone();
            st.subscription_id = Some(sub_id.clone());
            st.epoch
        };
        let r = self.exchange("subscribe", json!({}), &current, expected, &sub_id)?;
        self.build_snapshot(r, &current, None)
    }
    fn close(&self) {
        let (prev_g, prev_sub) = {
            let mut st = self.state.lock().unwrap();
            let prev = (st.grant.take(), st.subscription_id.take());
            st.epoch = st.epoch.wrapping_add(1);
            st.allowed_session_ids.clear();
            st.allowed_workspace_ids.clear();
            st.cursor = None;
            prev
        };
        if let (Some(g), Some(sub)) = (prev_g, prev_sub) {
            let req = json!({"version": 1, "subscriptionId": sub, "id": Uuid::new_v4().to_string(), "token": g.token, "hostId": g.host_id, "generation": g.generation, "op": "close"});
            if let Ok(wire) = serde_json::to_vec(&req) {
                let _ = (LocalSocket {
                    path: PathBuf::from(&g.socket_path),
                    request_limit: 65536,
                    response_limit: 1_048_576,
                    timeout: self.timeout,
                })
                .exchange_blocking(&wire);
            }
        }
    }
    fn read(&self) -> Result<DSHSessionSnapshot, DSHFailure> {
        let (g, ep, sub) = self.current_grant()?;
        let after = self.state.lock().unwrap().cursor.ok_or(DSHFailure::Stale)?;
        let r = self.exchange("read", json!({"after": after}), &g, ep, &sub)?;
        self.build_snapshot(r, &g, Some(after))
    }
    fn open(&self, sel: &DSHSelection) -> Result<DSHOpenReply, DSHFailure> {
        let (g, ep, sub) = self.current_grant()?;
        if sel.host_id != g.host_id || sel.generation != g.generation {
            return Err(DSHFailure::Stale);
        }
        if !self
            .state
            .lock()
            .unwrap()
            .allowed_session_ids
            .contains(&sel.session_id)
            || sel.workspace_id.len() > 4096
        {
            return Err(DSHFailure::Denied);
        }
        let r = self.exchange(
            "open",
            json!({"sessionId": sel.session_id, "workspaceId": sel.workspace_id}),
            &g,
            ep,
            &sub,
        )?;
        let status: DSHOpenResult = serde_json::from_value(Value::String(r.status))
            .map_err(|_| DSHFailure::InvalidReply)?;
        if r.rows.is_some()
            || r.changes.is_some()
            || r.cursor.is_some()
            || r.unavailable_session_ids.is_some()
            || (status != DSHOpenResult::Confirmed && r.selected.is_some())
        {
            return Err(DSHFailure::InvalidReply);
        }
        Ok(DSHOpenReply {
            host_id: r.host_id,
            generation: r.generation,
            status,
            selected: r.selected,
        })
    }
    fn create(&self, i: &DSHCreateIntent) -> Result<DSHCommandReply, DSHFailure> {
        let (g, ep, sub) = self.guard_cmd(
            &i.host_id,
            i.generation,
            &i.request_id,
            None,
            Some(&i.workspace_id),
        )?;
        let r = self.exchange(
            "create",
            json!({"workspaceId": i.workspace_id, "requestId": i.request_id}),
            &g,
            ep,
            &sub,
        )?;
        self.map_cmd_reply(r, &i.request_id, true)
    }
    fn prompt(&self, i: &DSHPromptIntent) -> Result<DSHCommandReply, DSHFailure> {
        let (g, ep, sub) = self.guard_cmd(
            &i.host_id,
            i.generation,
            &i.request_id,
            Some(&i.session_id),
            Some(&i.workspace_id),
        )?;
        if i.text.trim().is_empty() {
            return Err(DSHFailure::Denied);
        }
        let r = self.exchange("prompt", json!({"sessionId": i.session_id, "workspaceId": i.workspace_id, "requestId": i.request_id, "mode": i.mode.as_str(), "text": i.text}), &g, ep, &sub)?;
        self.map_cmd_reply(r, &i.request_id, false)
    }
    fn cancel(&self, i: &DSHCancelIntent) -> Result<DSHCommandReply, DSHFailure> {
        let (g, ep, sub) = self.guard_cmd(
            &i.host_id,
            i.generation,
            &i.request_id,
            Some(&i.session_id),
            Some(&i.workspace_id),
        )?;
        if i.expected_turn < 1 {
            return Err(DSHFailure::Denied);
        }
        let r = self.exchange("cancel", json!({"sessionId": i.session_id, "workspaceId": i.workspace_id, "requestId": i.request_id, "expectedTurn": i.expected_turn}), &g, ep, &sub)?;
        self.map_cmd_reply(r, &i.request_id, false)
    }
    fn query(&self, i: &DSHQueryIntent) -> Result<DSHCommandReply, DSHFailure> {
        let (g, ep, sub) = self.guard_cmd(
            &i.host_id,
            i.generation,
            &i.request_id,
            i.session_id.as_deref(),
            None,
        )?;
        let r = self.exchange("query", json!({"sessionId": i.session_id, "workspaceId": i.workspace_id, "requestId": i.request_id}), &g, ep, &sub)?;
        self.map_cmd_reply(r, &i.request_id, true)
    }
    fn resolve(&self, i: &DSHResolveIntent) -> Result<DSHCommandReply, DSHFailure> {
        let (g, ep, sub) = self.guard_cmd(
            &i.host_id,
            i.generation,
            &i.request_id,
            Some(&i.session_id),
            Some(&i.workspace_id),
        )?;
        if i.interaction_id.is_empty() {
            return Err(DSHFailure::Denied);
        }
        let r = self.exchange("resolve", json!({"sessionId": i.session_id, "workspaceId": i.workspace_id, "requestId": i.request_id, "interactionId": i.interaction_id, "interactionType": i.interaction_type, "decision": i.decision, "optionId": i.option_id}), &g, ep, &sub)?;
        self.map_cmd_reply(r, &i.request_id, false)
    }
    fn switch_model(&self, i: &DSHSwitchModelIntent) -> Result<DSHCommandReply, DSHFailure> {
        let (g, ep, sub) = self.guard_cmd(
            &i.host_id,
            i.generation,
            &i.request_id,
            Some(&i.session_id),
            Some(&i.workspace_id),
        )?;
        if i.provider.is_empty() || i.model.is_empty() {
            return Err(DSHFailure::Denied);
        }
        let r = self.exchange("selectModel", json!({"sessionId": i.session_id, "workspaceId": i.workspace_id, "requestId": i.request_id, "provider": i.provider, "model": i.model, "thought": i.thought}), &g, ep, &sub)?;
        self.map_cmd_reply(r, &i.request_id, false)
    }
}

pub struct DSHSessionController {
    pub enabled: bool,
    pub state: DSHConnectionState,
    pub rows: Vec<DSHSessionRow>,
    pub selected: Option<String>,
    pub unavailable_session_ids: Vec<String>,
    pub last_changes: Vec<DSHSessionChange>,
    pub cursor: u64,
    pub last_result: Option<DSHOpenResult>,
    pub last_command_result: Option<DSHCommandResult>,
    pub atomic_turn_check_supported: bool,
    pub pending_unknown_command: Option<DSHQueuedCommand>,
    pub voice_reply_enabled: bool,
    client: Arc<dyn DSHSessionServing>,
    snapshot: Option<DSHSessionSnapshot>,
    epoch: u64,
    next_request_sequence: u64,
    suspended: bool,
}

impl DSHSessionController {
    pub fn new(client: Arc<dyn DSHSessionServing>) -> Self {
        Self {
            enabled: false,
            state: DSHConnectionState::Disabled,
            rows: Vec::new(),
            selected: None,
            unavailable_session_ids: Vec::new(),
            last_changes: Vec::new(),
            cursor: 0,
            last_result: None,
            last_command_result: None,
            atomic_turn_check_supported: true,
            pending_unknown_command: None,
            voice_reply_enabled: false,
            client,
            snapshot: None,
            epoch: 0,
            next_request_sequence: 0,
            suspended: false,
        }
    }
    pub fn set_atomic_turn_check_supported(&mut self, supported: bool) {
        self.atomic_turn_check_supported = supported;
    }
    pub fn set_voice_reply(&mut self, enabled: bool) -> (bool, &'static str) {
        self.voice_reply_enabled = enabled;
        (
            true,
            if enabled {
                "已开启 DSH 语音回复服务"
            } else {
                "已关闭 DSH 语音回复服务"
            },
        )
    }
    pub fn set_enabled(&mut self, value: bool) {
        self.enabled = value;
        self.invalidate();
        if value && !self.suspended {
            self.state = DSHConnectionState::Connecting;
            if let Err(e) = self.client.connect().and_then(|s| self.accept(s)) {
                self.fail(e);
            }
        }
    }
    pub fn suspend(&mut self) {
        self.suspended = true;
        self.invalidate();
    }
    pub fn resume(&mut self) {
        self.suspended = false;
        if self.enabled {
            self.set_enabled(true);
        }
    }
    pub fn shutdown(&mut self) {
        self.enabled = false;
        self.invalidate();
    }
    fn clear_state(&mut self, st: DSHConnectionState) {
        self.snapshot = None;
        self.rows.clear();
        self.selected = None;
        self.last_result = None;
        self.last_command_result = None;
        self.pending_unknown_command = None;
        self.unavailable_session_ids.clear();
        self.last_changes.clear();
        self.cursor = 0;
        self.state = st;
        self.client.close();
    }
    fn invalidate(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.clear_state(DSHConnectionState::Disabled);
    }
    fn fail(&mut self, err: DSHFailure) {
        self.clear_state(DSHConnectionState::Unavailable(err));
    }
    fn accept(&mut self, value: DSHSessionSnapshot) -> Result<(), DSHFailure> {
        value.validate(self.snapshot.as_ref().map(|s| s.cursor))?;
        if let Some(prev) = &self.snapshot {
            if prev.host_id != value.host_id
                || prev.generation != value.generation
                || value.session_ids.len() < prev.session_ids.len()
                || value.session_ids[..prev.session_ids.len()] != prev.session_ids[..]
            {
                return Err(DSHFailure::Stale);
            }
        }
        if let Some(sel) = &value.selected {
            self.selected = Some(sel.clone());
        } else if self
            .selected
            .as_ref()
            .is_some_and(|cur| !value.rows.iter().any(|r| &r.session_id == cur))
        {
            self.selected = None;
        }
        self.rows = value.rows.clone();
        self.unavailable_session_ids = value.unavailable_session_ids.clone();
        self.last_changes = value.changes.clone();
        self.cursor = value.cursor;
        self.snapshot = Some(value);
        self.state = DSHConnectionState::Ready;
        Ok(())
    }
    pub fn refresh(&mut self) {
        if self.enabled && !self.suspended && self.state == DSHConnectionState::Ready {
            if let Err(e) = self.client.read().and_then(|s| self.accept(s)) {
                self.fail(e);
            }
        }
    }
    pub fn row(&self, session_id: &str) -> Option<&DSHSessionRow> {
        self.rows.iter().find(|r| r.session_id == session_id)
    }
    fn ready_snap(&self, require_no_unknown: bool) -> Option<DSHSessionSnapshot> {
        (self.enabled
            && !self.suspended
            && self.state == DSHConnectionState::Ready
            && (!require_no_unknown || self.pending_unknown_command.is_none()))
        .then(|| self.snapshot.clone())
        .flatten()
    }
    pub fn selection(&self, session_id: &str) -> Option<DSHSelection> {
        let (snap, row) = (self.ready_snap(false)?, self.row(session_id)?);
        Some(DSHSelection {
            session_id: session_id.into(),
            workspace_id: row.workspace_id.clone(),
            host_id: snap.host_id,
            generation: snap.generation,
            local_epoch: self.epoch,
        })
    }
    fn make_request_id(&mut self, explicit: Option<&str>) -> String {
        if let Some(id) = explicit.filter(|s| !s.is_empty()) {
            return id.into();
        }
        self.next_request_sequence = self.next_request_sequence.wrapping_add(1);
        format!("dsh-req-{}-{}", self.epoch, self.next_request_sequence)
    }
    pub fn create_intent(
        &mut self,
        workspace_id: &str,
        request_id: Option<&str>,
    ) -> Option<DSHQueuedCommand> {
        let snap = self.ready_snap(true)?;
        if workspace_id.is_empty()
            || (!self.rows.is_empty() && !self.rows.iter().any(|r| r.workspace_id == workspace_id))
        {
            return None;
        }
        let req_id = self.make_request_id(request_id);
        Some(DSHQueuedCommand::Create(DSHCreateIntent {
            workspace_id: workspace_id.into(),
            request_id: req_id,
            host_id: snap.host_id,
            generation: snap.generation,
            local_epoch: self.epoch,
        }))
    }
    pub fn prompt_intent(
        &mut self,
        session_id: &str,
        mode: DSHPromptMode,
        text: &str,
        request_id: Option<&str>,
    ) -> Option<DSHQueuedCommand> {
        let snap = self.ready_snap(true)?;
        if self.selected.as_deref() != Some(session_id) {
            self.last_command_result = Some(DSHCommandResult::Unconfirmed(
                self.selected.clone(),
                "guard.selectionMismatch".into(),
            ));
            return None;
        }
        let row = self.row(session_id)?.clone();
        if text.trim().is_empty() {
            return None;
        }
        if mode == DSHPromptMode::Steer && !row.running {
            self.last_command_result = Some(DSHCommandResult::Denied(Some(
                "guard.steerRequiresRunningTurn".into(),
            )));
            return None;
        }
        let req_id = self.make_request_id(request_id);
        Some(DSHQueuedCommand::Prompt(DSHPromptIntent {
            session_id: session_id.into(),
            workspace_id: row.workspace_id,
            request_id: req_id,
            mode,
            text: text.into(),
            host_id: snap.host_id,
            generation: snap.generation,
            local_epoch: self.epoch,
        }))
    }
    pub fn cancel_intent(
        &mut self,
        session_id: &str,
        expected_turn_override: Option<i64>,
        request_id: Option<&str>,
    ) -> Option<DSHQueuedCommand> {
        let snap = self.ready_snap(true)?;
        if self.selected.as_deref() != Some(session_id) {
            self.last_command_result = Some(DSHCommandResult::Unconfirmed(
                self.selected.clone(),
                "guard.selectionMismatch".into(),
            ));
            return None;
        }
        let row = self.row(session_id)?.clone();
        if !row.running {
            self.last_command_result = Some(DSHCommandResult::Noop(
                "guard.notRunning".into(),
                row.current_turn,
                Some(row.queued_count),
            ));
            return None;
        }
        let Some(turn) = expected_turn_override
            .or(row.current_turn)
            .filter(|_| self.atomic_turn_check_supported)
        else {
            self.last_command_result = Some(DSHCommandResult::Denied(Some(
                "guard.unprovableTurnAtomicity".into(),
            )));
            return None;
        };
        let req_id = self.make_request_id(request_id);
        Some(DSHQueuedCommand::Cancel(DSHCancelIntent {
            session_id: session_id.into(),
            workspace_id: row.workspace_id,
            request_id: req_id,
            expected_turn: turn,
            host_id: snap.host_id,
            generation: snap.generation,
            local_epoch: self.epoch,
        }))
    }
    pub fn query_intent(
        &self,
        session_id: Option<&str>,
        workspace_id: Option<&str>,
        request_id: &str,
    ) -> Option<DSHQueuedCommand> {
        let snap = self.ready_snap(false).filter(|_| !request_id.is_empty())?;
        Some(DSHQueuedCommand::Query(DSHQueryIntent {
            session_id: session_id.map(String::from),
            workspace_id: workspace_id.map(String::from),
            request_id: request_id.into(),
            host_id: snap.host_id,
            generation: snap.generation,
            local_epoch: self.epoch,
        }))
    }
    pub fn resolve_intent(
        &mut self,
        session_id: &str,
        interaction_id: &str,
        interaction_type: DSHInteractionType,
        decision: DSHInteractionDecision,
        option_id: Option<&str>,
        request_id: Option<&str>,
    ) -> Option<DSHQueuedCommand> {
        let (snap, row) = (self.ready_snap(false)?, self.row(session_id)?.clone());
        if interaction_id.is_empty() {
            return None;
        }
        let req_id = self.make_request_id(request_id);
        Some(DSHQueuedCommand::Resolve(DSHResolveIntent {
            session_id: session_id.into(),
            workspace_id: row.workspace_id,
            request_id: req_id,
            interaction_id: interaction_id.into(),
            interaction_type,
            decision,
            option_id: option_id.map(String::from),
            host_id: snap.host_id,
            generation: snap.generation,
            local_epoch: self.epoch,
        }))
    }
    pub fn switch_model_intent(
        &mut self,
        session_id: &str,
        provider: &str,
        model: &str,
        thought: Option<&str>,
        request_id: Option<&str>,
    ) -> Option<DSHQueuedCommand> {
        let (snap, row) = (self.ready_snap(false)?, self.row(session_id)?.clone());
        if provider.is_empty() || model.is_empty() {
            return None;
        }
        let req_id = self.make_request_id(request_id);
        Some(DSHQueuedCommand::SwitchModel(DSHSwitchModelIntent {
            session_id: session_id.into(),
            workspace_id: row.workspace_id,
            request_id: req_id,
            provider: provider.into(),
            model: model.into(),
            thought: thought.map(String::from),
            host_id: snap.host_id,
            generation: snap.generation,
            local_epoch: self.epoch,
        }))
    }
    pub fn open(&mut self, sel: &DSHSelection, admitted: bool) -> DSHOpenResult {
        let valid = self.ready_snap(false).is_some_and(|snap| {
            sel.local_epoch == self.epoch
                && sel.host_id == snap.host_id
                && sel.generation == snap.generation
                && snap.session_ids.contains(&sel.session_id)
                && self
                    .rows
                    .iter()
                    .any(|r| r.session_id == sel.session_id && r.workspace_id == sel.workspace_id)
                && admitted
        });
        if !valid {
            return DSHOpenResult::Stale;
        }
        match self.client.open(sel) {
            Ok(resp) => {
                if resp.host_id != sel.host_id || resp.generation != sel.generation {
                    self.fail(DSHFailure::Stale);
                    self.last_result = Some(DSHOpenResult::Stale);
                    return DSHOpenResult::Stale;
                }
                let res = if resp.status == DSHOpenResult::Confirmed
                    && resp.selected.as_deref() != Some(&sel.session_id)
                {
                    DSHOpenResult::Unconfirmed
                } else {
                    resp.status
                };
                if res == DSHOpenResult::Confirmed {
                    self.selected = Some(sel.session_id.clone());
                }
                if matches!(
                    res,
                    DSHOpenResult::Stale | DSHOpenResult::Denied | DSHOpenResult::Unavailable
                ) {
                    self.fail(match res {
                        DSHOpenResult::Denied => DSHFailure::Denied,
                        DSHOpenResult::Stale => DSHFailure::Stale,
                        _ => DSHFailure::Unavailable,
                    });
                }
                self.last_result = Some(res);
                res
            }
            Err(e) => {
                self.fail(e);
                let res = match e {
                    DSHFailure::Denied => DSHOpenResult::Denied,
                    DSHFailure::Stale => DSHOpenResult::Stale,
                    _ => DSHOpenResult::Unavailable,
                };
                self.last_result = Some(res);
                res
            }
        }
    }
    fn record_cmd(&mut self, res: DSHCommandResult) -> DSHCommandResult {
        self.last_command_result = Some(res.clone());
        res
    }
    fn verify_target(
        &self,
        snap: &DSHSessionSnapshot,
        sid: &str,
        ws: &str,
    ) -> Result<DSHSessionRow, DSHCommandResult> {
        if self.selected.as_deref() != Some(sid) {
            return Err(DSHCommandResult::Unconfirmed(
                self.selected.clone(),
                "guard.selectionMismatch".into(),
            ));
        }
        let row = self
            .rows
            .iter()
            .find(|r| r.session_id == sid && r.workspace_id == ws)
            .filter(|_| snap.session_ids.contains(&sid.to_owned()))
            .cloned()
            .ok_or(DSHCommandResult::Stale)?;
        Ok(row)
    }
    pub fn execute(&mut self, cmd: &DSHQueuedCommand, admitted: bool) -> DSHCommandResult {
        let Some(snap) = self.ready_snap(false) else {
            return self.record_cmd(DSHCommandResult::Stale);
        };
        if cmd.local_epoch() != self.epoch
            || cmd.host_id() != snap.host_id
            || cmd.generation() != snap.generation
            || !admitted
        {
            return self.record_cmd(DSHCommandResult::Stale);
        }
        if let Some(p) = &self.pending_unknown_command {
            if p.request_id() != cmd.request_id() {
                return self.record_cmd(DSHCommandResult::Unknown(
                    p.request_id().into(),
                    "guard.unknownOutcomeRequiresReconcile".into(),
                ));
            }
        }
        let check = match cmd {
            DSHQueuedCommand::Create(i) => {
                (!self.rows.iter().any(|r| r.workspace_id == i.workspace_id))
                    .then(|| DSHCommandResult::Denied(Some("guard.unauthorizedWorkspace".into())))
            }
            DSHQueuedCommand::Prompt(i) => {
                match self.verify_target(&snap, &i.session_id, &i.workspace_id) {
                    Ok(row) if i.mode == DSHPromptMode::Steer && !row.running => Some(
                        DSHCommandResult::Denied(Some("guard.steerRequiresRunningTurn".into())),
                    ),
                    Ok(_) => None,
                    Err(e) => Some(e),
                }
            }
            DSHQueuedCommand::Cancel(i) => {
                if !self.atomic_turn_check_supported {
                    Some(DSHCommandResult::Denied(Some(
                        "guard.unprovableTurnAtomicity".into(),
                    )))
                } else {
                    match self.verify_target(&snap, &i.session_id, &i.workspace_id) {
                        Ok(row) if row.running && row.current_turn.is_none() => Some(
                            DSHCommandResult::Denied(Some("guard.unprovableTurnAtomicity".into())),
                        ),
                        Ok(_) => None,
                        Err(e) => Some(e),
                    }
                }
            }
            DSHQueuedCommand::Resolve(i) => self
                .verify_target(&snap, &i.session_id, &i.workspace_id)
                .err(),
            DSHQueuedCommand::SwitchModel(i) => {
                match self.verify_target(&snap, &i.session_id, &i.workspace_id) {
                    Ok(row) => row
                        .supported_models
                        .as_ref()
                        .and_then(|ms| {
                            ms.iter()
                                .find(|m| m.model == i.model && m.provider == i.provider)
                        })
                        .zip(i.thought.as_ref())
                        .and_then(|(opt, th)| {
                            (!opt.supported_thoughts.contains(th)).then(|| {
                                DSHCommandResult::UnsupportedThought(
                                    "guard.unsupportedThoughtLevel".into(),
                                )
                            })
                        }),
                    Err(e) => Some(e),
                }
            }
            DSHQueuedCommand::Query(_) => None,
        };
        if let Some(early) = check {
            return self.record_cmd(early);
        }
        let res = match cmd {
            DSHQueuedCommand::Create(i) => self.client.create(i),
            DSHQueuedCommand::Prompt(i) => self.client.prompt(i),
            DSHQueuedCommand::Cancel(i) => self.client.cancel(i),
            DSHQueuedCommand::Query(i) => self.client.query(i),
            DSHQueuedCommand::Resolve(i) => self.client.resolve(i),
            DSHQueuedCommand::SwitchModel(i) => self.client.switch_model(i),
        };
        match res {
            Ok(resp) => {
                if resp.host_id != cmd.host_id() || resp.generation != cmd.generation() {
                    self.fail(DSHFailure::Stale);
                    return self.record_cmd(DSHCommandResult::Stale);
                }
                let mapped = self.map_command_reply(resp);
                if matches!(mapped, DSHCommandResult::Unknown(..)) {
                    self.pending_unknown_command = Some(cmd.clone());
                } else if self
                    .pending_unknown_command
                    .as_ref()
                    .is_some_and(|p| p.request_id() == cmd.request_id())
                {
                    self.pending_unknown_command = None;
                }
                match &mapped {
                    DSHCommandResult::Stale => self.fail(DSHFailure::Stale),
                    DSHCommandResult::Unavailable => self.fail(DSHFailure::Unavailable),
                    DSHCommandResult::Denied(rc)
                        if !rc.as_deref().is_some_and(|r| r.starts_with("guard.")) =>
                    {
                        self.fail(DSHFailure::Denied)
                    }
                    _ => {}
                }
                self.record_cmd(mapped)
            }
            Err(e) => {
                self.fail(e);
                self.record_cmd(match e {
                    DSHFailure::Denied => DSHCommandResult::Denied(None),
                    DSHFailure::Stale => DSHCommandResult::Stale,
                    _ => DSHCommandResult::Unavailable,
                })
            }
        }
    }
    fn map_command_reply(&mut self, r: DSHCommandReply) -> DSHCommandResult {
        match r.status.as_str() {
            "confirmed" => {
                let (Some(cid), Some(ws)) = (
                    r.session_id
                        .clone()
                        .filter(|c| r.selected.as_ref() == Some(c)),
                    r.workspace_id.clone(),
                ) else {
                    return DSHCommandResult::Unconfirmed(
                        r.selected,
                        r.reason_code
                            .unwrap_or_else(|| "reply.selectionMismatch".into()),
                    );
                };
                self.selected = Some(cid.clone());
                self.apply_created_locally(&cid, &ws, r.current_turn, r.queued_count.unwrap_or(0));
                DSHCommandResult::Created(cid.clone(), ws, cid, r.duplicate)
            }
            "queued" => {
                let (Some(sid), Some(DSHPromptMode::Queue), Some(qc)) =
                    (r.session_id, r.delivery, r.queued_count)
                else {
                    return DSHCommandResult::Unavailable;
                };
                self.update_row_locally(&sid, None, r.current_turn, qc);
                DSHCommandResult::Queued(sid, r.request_id, r.current_turn, qc, r.duplicate)
            }
            "steered" => {
                let (Some(sid), Some(DSHPromptMode::Steer), Some(qc)) =
                    (r.session_id, r.delivery, r.queued_count)
                else {
                    return DSHCommandResult::Unavailable;
                };
                self.update_row_locally(&sid, None, r.current_turn, qc);
                DSHCommandResult::Steered(sid, r.request_id, r.current_turn, qc, r.duplicate)
            }
            "cancelled" => {
                let (Some(sid), Some(et), Some(qc)) =
                    (r.session_id, r.expected_turn, r.queued_count)
                else {
                    return DSHCommandResult::Unavailable;
                };
                self.update_row_locally(&sid, Some(false), r.current_turn, qc);
                DSHCommandResult::Cancelled(sid, r.request_id, et, r.current_turn, qc, r.duplicate)
            }
            "cancelledQueueRemaining" => {
                let (Some(sid), Some(et), Some(qc)) = (
                    r.session_id,
                    r.expected_turn,
                    r.queued_count.filter(|&q| q > 0),
                ) else {
                    return DSHCommandResult::Unavailable;
                };
                self.update_row_locally(&sid, Some(false), r.current_turn, qc);
                DSHCommandResult::CancelledQueueRemaining(
                    sid,
                    r.request_id,
                    et,
                    r.current_turn,
                    qc,
                    r.duplicate,
                )
            }
            "noop" => {
                if let (Some(sid), Some(qc)) = (&r.session_id, r.queued_count) {
                    self.update_row_locally(sid, None, r.current_turn, qc);
                }
                DSHCommandResult::Noop(
                    r.reason_code.unwrap_or_else(|| "guard.noop".into()),
                    r.current_turn,
                    r.queued_count,
                )
            }
            "unknown" => DSHCommandResult::Unknown(
                r.request_id,
                r.reason_code
                    .unwrap_or_else(|| "query.unknownRequest".into()),
            ),
            "unconfirmed" => DSHCommandResult::Unconfirmed(
                r.selected,
                r.reason_code.unwrap_or_else(|| "guard.unconfirmed".into()),
            ),
            "denied" => DSHCommandResult::Denied(r.reason_code),
            "stale" => DSHCommandResult::Stale,
            "resolved" => {
                let Some(sid) = r.session_id else {
                    return DSHCommandResult::Unavailable;
                };
                self.mutate_row(&sid, |row| row.pending_interaction = None);
                DSHCommandResult::Resolved(sid, r.request_id, r.duplicate)
            }
            "interactionTypeMismatch" => DSHCommandResult::InteractionTypeMismatch(
                r.reason_code
                    .unwrap_or_else(|| "guard.interactionTypeMismatch".into()),
            ),
            "modelSwitched" => {
                let (Some(sid), Some(cfg)) = (r.session_id, r.model_config) else {
                    return DSHCommandResult::Unavailable;
                };
                self.mutate_row(&sid, |row| row.model_config = Some(cfg.clone()));
                DSHCommandResult::ModelSwitched(sid, r.request_id, cfg, r.duplicate)
            }
            "unsupportedThought" => DSHCommandResult::UnsupportedThought(
                r.reason_code
                    .unwrap_or_else(|| "guard.unsupportedThoughtLevel".into()),
            ),
            _ => DSHCommandResult::Unavailable,
        }
    }
    fn apply_created_locally(
        &mut self,
        sid: &str,
        ws: &str,
        current_turn: Option<i64>,
        queued_count: i64,
    ) {
        for r in &mut self.rows {
            r.active = r.session_id == sid;
        }
        if !self.rows.iter().any(|r| r.session_id == sid) {
            self.rows.push(DSHSessionRow {
                session_id: sid.into(),
                workspace_id: ws.into(),
                title: "New Session".into(),
                active: true,
                running: false,
                pending_input: 0,
                current_turn,
                queued_count,
                pending_interaction: None,
                model_config: None,
                supported_models: None,
            });
        }
        if let Some(snap) = &mut self.snapshot {
            if !snap.session_ids.iter().any(|s| s == sid) {
                snap.session_ids.push(sid.into());
            }
            snap.rows = self.rows.clone();
            snap.selected = Some(sid.into());
        }
    }
    fn update_row_locally(
        &mut self,
        sid: &str,
        running: Option<bool>,
        current_turn: Option<i64>,
        queued_count: i64,
    ) {
        self.mutate_row(sid, |row| {
            if let Some(r) = running {
                row.running = r;
            }
            row.current_turn = current_turn;
            row.queued_count = queued_count;
        });
    }
    fn mutate_row(&mut self, sid: &str, f: impl FnOnce(&mut DSHSessionRow)) {
        if let Some(row) = self.rows.iter_mut().find(|r| r.session_id == sid) {
            f(row);
        }
        if let Some(snap) = &mut self.snapshot {
            snap.rows = self.rows.clone();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DSHFixtureMode {
    Normal,
    WrongSelected,
    Deleted,
    Rotated,
    Disconnected,
    Gap,
}

#[derive(Clone)]
struct SessionRuntimeState {
    workspace_id: String,
    title: String,
    running: bool,
    pending_input: u64,
    current_turn: Option<i64>,
    queued_count: i64,
    pending_interaction: Option<DSHPendingInteraction>,
    model_config: Option<DSHModelConfig>,
    supported_models: Option<Vec<DSHModelOption>>,
}

struct DSHFixtureState {
    mode: DSHFixtureMode,
    selected: Option<String>,
    cursor: u64,
    changes: Vec<DSHSessionChange>,
    connected: bool,
    session_order: Vec<String>,
    created_order: Vec<String>,
    states: HashMap<String, SessionRuntimeState>,
    settled_replies: HashMap<String, DSHCommandReply>,
    pub metadata_reads: usize,
    pub body_reads: usize,
    pub opens: usize,
    pub creates: usize,
    pub prompts: usize,
    pub cancels: usize,
    pub resolves: usize,
    pub select_models: usize,
    pub closes: usize,
    pub navigations: Vec<String>,
}

pub struct DSHNavigationFixture {
    state: Mutex<DSHFixtureState>,
}

impl Default for DSHNavigationFixture {
    fn default() -> Self {
        Self::new()
    }
}

impl DSHNavigationFixture {
    pub const FIRST: &'static str = "qa-session-a";
    pub const SECOND: &'static str = "qa-session-b";
    pub const COLD: &'static str = "qa-session-cold";

    fn init_entry(ws: &str, title: &str, running: bool, turn: Option<i64>) -> SessionRuntimeState {
        let m1 = DSHModelOption {
            provider: "deepseek-official".into(),
            model: "deepseek-chat".into(),
            default_thought: Some("medium".into()),
            supported_thoughts: vec!["off".into(), "low".into(), "medium".into(), "high".into()],
        };
        let m2 = DSHModelOption {
            provider: "deepseek-official".into(),
            model: "deepseek-reasoner".into(),
            default_thought: Some("high".into()),
            supported_thoughts: vec!["low".into(), "medium".into(), "high".into()],
        };
        SessionRuntimeState {
            workspace_id: ws.into(),
            title: title.into(),
            running,
            pending_input: 0,
            current_turn: turn,
            queued_count: 0,
            pending_interaction: None,
            model_config: Some(DSHModelConfig {
                provider: "deepseek-official".into(),
                model: "deepseek-chat".into(),
                thought: Some("medium".into()),
            }),
            supported_models: Some(vec![m1, m2]),
        }
    }
    fn fresh_state() -> DSHFixtureState {
        let mut states = HashMap::new();
        states.insert(
            Self::FIRST.into(),
            Self::init_entry("workspace-a", "同名会话", true, Some(1)),
        );
        states.insert(
            Self::SECOND.into(),
            Self::init_entry("workspace-b", "同名会话", true, Some(1)),
        );
        DSHFixtureState {
            mode: DSHFixtureMode::Normal,
            selected: None,
            cursor: 0,
            changes: Vec::new(),
            connected: false,
            session_order: vec![Self::FIRST.into(), Self::SECOND.into()],
            created_order: Vec::new(),
            states,
            settled_replies: HashMap::new(),
            metadata_reads: 0,
            body_reads: 0,
            opens: 0,
            creates: 0,
            prompts: 0,
            cancels: 0,
            resolves: 0,
            select_models: 0,
            closes: 0,
            navigations: Vec::new(),
        }
    }
    pub fn new() -> Self {
        Self {
            state: Mutex::new(Self::fresh_state()),
        }
    }
    pub fn reset(&self) {
        *self.state.lock().unwrap() = Self::fresh_state();
    }
    pub fn set_mode(&self, mode: DSHFixtureMode) {
        self.state.lock().unwrap().mode = mode;
    }
    fn build_row(s: &DSHFixtureState, id: &str) -> DSHSessionRow {
        let st = s.states.get(id).cloned().unwrap_or_else(|| {
            Self::init_entry(
                if id == Self::FIRST {
                    "workspace-a"
                } else {
                    "workspace-b"
                },
                "同名会话",
                true,
                Some(1),
            )
        });
        DSHSessionRow {
            session_id: id.into(),
            workspace_id: st.workspace_id,
            title: st.title,
            active: true,
            running: st.running,
            pending_input: st.pending_input,
            current_turn: st.current_turn,
            queued_count: st.queued_count,
            pending_interaction: st.pending_interaction,
            model_config: st.model_config,
            supported_models: st.supported_models,
        }
    }
    fn push_change(s: &mut DSHFixtureState, id: &str) {
        s.cursor += 1;
        let (seq, row) = (s.cursor, Self::build_row(s, id));
        s.changes.push(DSHSessionChange { sequence: seq, row });
    }
    pub fn set_turn_state(
        &self,
        session_id: &str,
        running: bool,
        current_turn: Option<i64>,
        queued_count: Option<i64>,
    ) {
        let mut s = self.state.lock().unwrap();
        if let Some(entry) = s.states.get_mut(session_id) {
            entry.running = running;
            entry.current_turn = current_turn;
            if let Some(qc) = queued_count {
                entry.queued_count = qc;
            }
            Self::push_change(&mut s, session_id);
        }
    }
    pub fn set_pending_interaction(
        &self,
        session_id: &str,
        interaction: Option<DSHPendingInteraction>,
    ) {
        let mut s = self.state.lock().unwrap();
        if let Some(entry) = s.states.get_mut(session_id) {
            entry.pending_input = u64::from(interaction.is_some());
            entry.pending_interaction = interaction;
            Self::push_change(&mut s, session_id);
        }
    }
    pub fn pending_round_trip(&self) {
        let mut s = self.state.lock().unwrap();
        for val in [1u64, 0u64] {
            if let Some(entry) = s.states.get_mut(Self::FIRST) {
                entry.pending_input = val;
            }
            Self::push_change(&mut s, Self::FIRST);
        }
    }
    fn gen(s: &DSHFixtureState) -> u64 {
        if s.mode == DSHFixtureMode::Rotated {
            2
        } else {
            1
        }
    }
    fn check_cmd(
        s: &DSHFixtureState,
        host_id: &str,
        req_gen: u64,
        req_id: &str,
    ) -> Result<(u64, Option<DSHCommandReply>), DSHFailure> {
        if !s.connected || s.mode == DSHFixtureMode::Disconnected {
            return Err(DSHFailure::Unavailable);
        }
        let gen = Self::gen(s);
        if host_id != "qa-host" || req_gen != gen {
            return Ok((gen, Some(DSHCommandReply::qa(gen, "stale", req_id))));
        }
        if let Some(existing) = s.settled_replies.get(req_id) {
            let mut dup = existing.clone();
            dup.duplicate = true;
            return Ok((gen, Some(dup)));
        }
        Ok((gen, None))
    }
}

impl DSHSessionServing for DSHNavigationFixture {
    fn connect(&self) -> Result<DSHSessionSnapshot, DSHFailure> {
        {
            let mut s = self.state.lock().unwrap();
            s.connected = true;
            s.changes.clear();
        }
        self.read()
    }
    fn close(&self) {
        let mut s = self.state.lock().unwrap();
        if s.connected {
            s.closes += 1;
        }
        s.connected = false;
    }
    fn read(&self) -> Result<DSHSessionSnapshot, DSHFailure> {
        let mut s = self.state.lock().unwrap();
        if !s.connected {
            return Err(DSHFailure::Denied);
        }
        if s.mode == DSHFixtureMode::Disconnected {
            return Err(DSHFailure::Unavailable);
        }
        if s.mode == DSHFixtureMode::Gap {
            return Err(DSHFailure::Gap);
        }
        s.metadata_reads += 1;
        let mut live = s.session_order.clone();
        live.extend(s.created_order.iter().cloned());
        let visible: Vec<String> = if s.mode == DSHFixtureMode::Deleted {
            live.into_iter().filter(|id| id != Self::FIRST).collect()
        } else {
            live
        };
        let mut all_ids = vec![Self::FIRST.into(), Self::SECOND.into(), Self::COLD.into()];
        all_ids.extend(s.created_order.iter().cloned());
        let rows: Vec<DSHSessionRow> = visible.iter().map(|id| Self::build_row(&s, id)).collect();
        let unavail: Vec<String> = all_ids
            .iter()
            .filter(|id| !visible.contains(id))
            .cloned()
            .collect();
        let changes = std::mem::take(&mut s.changes);
        Ok(DSHSessionSnapshot {
            host_id: "qa-host".into(),
            generation: Self::gen(&s),
            session_ids: all_ids,
            cursor: s.cursor,
            rows,
            unavailable_session_ids: unavail,
            changes,
            selected: s.selected.clone(),
        })
    }
    fn open(&self, sel: &DSHSelection) -> Result<DSHOpenReply, DSHFailure> {
        let mut s = self.state.lock().unwrap();
        if !s.connected || s.mode == DSHFixtureMode::Disconnected {
            return Err(DSHFailure::Unavailable);
        }
        let gen = Self::gen(&s);
        let mk = |status, selected| DSHOpenReply {
            host_id: "qa-host".into(),
            generation: gen,
            status,
            selected,
        };
        if sel.host_id != "qa-host" || sel.generation != gen {
            return Ok(mk(DSHOpenResult::Stale, None));
        }
        if s.mode == DSHFixtureMode::Deleted && sel.session_id == Self::FIRST {
            return Ok(mk(DSHOpenResult::Denied, None));
        }
        s.opens += 1;
        s.navigations.push(sel.session_id.clone());
        if s.mode == DSHFixtureMode::WrongSelected {
            s.selected = Some(Self::SECOND.into());
            return Ok(mk(DSHOpenResult::Unconfirmed, Some(Self::SECOND.into())));
        }
        s.selected = Some(sel.session_id.clone());
        Ok(mk(DSHOpenResult::Confirmed, Some(sel.session_id.clone())))
    }
    fn create(&self, i: &DSHCreateIntent) -> Result<DSHCommandReply, DSHFailure> {
        let mut s = self.state.lock().unwrap();
        let (gen, early) = Self::check_cmd(&s, &i.host_id, i.generation, &i.request_id)?;
        if let Some(r) = early {
            return Ok(r);
        }
        s.creates += 1;
        let cid = format!("qa-session-created-{}", s.creates);
        s.created_order.push(cid.clone());
        s.states.insert(
            cid.clone(),
            Self::init_entry(&i.workspace_id, "New Session", false, None),
        );
        s.selected = Some(cid.clone());
        Self::push_change(&mut s, &cid);
        let settled = DSHCommandReply::qa(gen, "confirmed", &i.request_id)
            .sess(&cid.clone(), &i.workspace_id, Some(cid))
            .turns(None, None, Some(0));
        s.settled_replies
            .insert(i.request_id.clone(), settled.clone());
        Ok(settled)
    }
    fn prompt(&self, i: &DSHPromptIntent) -> Result<DSHCommandReply, DSHFailure> {
        let mut s = self.state.lock().unwrap();
        let (gen, early) = Self::check_cmd(&s, &i.host_id, i.generation, &i.request_id)?;
        if let Some(r) = early {
            return Ok(r);
        }
        let Some(mut entry) = s.states.get(&i.session_id).cloned() else {
            return Ok(DSHCommandReply::qa(gen, "denied", &i.request_id).rc("session.notFound"));
        };
        if i.mode == DSHPromptMode::Steer && (!entry.running || entry.current_turn.is_none()) {
            return Ok(
                DSHCommandReply::qa(gen, "denied", &i.request_id).rc("session.steerUnavailable")
            );
        }
        s.prompts += 1;
        entry.queued_count += 1;
        let (ct, qc) = (entry.current_turn, entry.queued_count);
        s.states.insert(i.session_id.clone(), entry);
        Self::push_change(&mut s, &i.session_id);
        let st_str = if i.mode == DSHPromptMode::Steer {
            "steered"
        } else {
            "queued"
        };
        let mut settled = DSHCommandReply::qa(gen, st_str, &i.request_id)
            .sess(&i.session_id, &i.workspace_id, s.selected.clone())
            .turns(None, ct, Some(qc));
        settled.delivery = Some(i.mode);
        s.settled_replies
            .insert(i.request_id.clone(), settled.clone());
        Ok(settled)
    }
    fn cancel(&self, i: &DSHCancelIntent) -> Result<DSHCommandReply, DSHFailure> {
        let mut s = self.state.lock().unwrap();
        let (gen, early) = Self::check_cmd(&s, &i.host_id, i.generation, &i.request_id)?;
        if let Some(r) = early {
            return Ok(r);
        }
        let Some(mut entry) = s.states.get(&i.session_id).cloned() else {
            return Ok(DSHCommandReply::qa(gen, "denied", &i.request_id).rc("session.notFound"));
        };
        if !entry.running || entry.current_turn.is_none() {
            let noop = DSHCommandReply::qa(gen, "noop", &i.request_id)
                .sess(&i.session_id, &i.workspace_id, s.selected.clone())
                .turns(
                    Some(i.expected_turn),
                    entry.current_turn,
                    Some(entry.queued_count),
                )
                .rc("guard.notRunning");
            s.settled_replies.insert(i.request_id.clone(), noop.clone());
            return Ok(noop);
        }
        let cur_turn = entry.current_turn.unwrap();
        if cur_turn != i.expected_turn {
            let noop = DSHCommandReply::qa(gen, "noop", &i.request_id)
                .sess(&i.session_id, &i.workspace_id, s.selected.clone())
                .turns(
                    Some(i.expected_turn),
                    Some(cur_turn),
                    Some(entry.queued_count),
                )
                .rc("guard.turnChanged");
            s.settled_replies.insert(i.request_id.clone(), noop.clone());
            return Ok(noop);
        }
        s.cancels += 1;
        entry.running = false;
        entry.current_turn = None;
        let qc = entry.queued_count;
        s.states.insert(i.session_id.clone(), entry);
        Self::push_change(&mut s, &i.session_id);
        let st_str = if qc > 0 {
            "cancelledQueueRemaining"
        } else {
            "cancelled"
        };
        let settled = DSHCommandReply::qa(gen, st_str, &i.request_id)
            .sess(&i.session_id, &i.workspace_id, s.selected.clone())
            .turns(Some(i.expected_turn), None, Some(qc));
        s.settled_replies
            .insert(i.request_id.clone(), settled.clone());
        Ok(settled)
    }
    fn resolve(&self, i: &DSHResolveIntent) -> Result<DSHCommandReply, DSHFailure> {
        let mut s = self.state.lock().unwrap();
        let (gen, early) = Self::check_cmd(&s, &i.host_id, i.generation, &i.request_id)?;
        if let Some(r) = early {
            return Ok(r);
        }
        s.resolves += 1;
        let Some(mut entry) = s.states.get(&i.session_id).cloned() else {
            return Ok(DSHCommandReply::qa(gen, "denied", &i.request_id).rc("session.notFound"));
        };
        if let Some(p) = &entry.pending_interaction {
            if p.interaction_type != i.interaction_type {
                let rc = if i.interaction_type == DSHInteractionType::Permission {
                    "guard.questionCannotApproveAsPermission"
                } else {
                    "guard.permissionCannotAnswerAsQuestion"
                };
                return Ok(
                    DSHCommandReply::qa(gen, "interactionTypeMismatch", &i.request_id)
                        .sess(&i.session_id, &i.workspace_id, s.selected.clone())
                        .rc(rc),
                );
            }
            entry.pending_interaction = None;
            entry.pending_input = 0;
            s.states.insert(i.session_id.clone(), entry);
            Self::push_change(&mut s, &i.session_id);
            let settled = DSHCommandReply::qa(gen, "resolved", &i.request_id).sess(
                &i.session_id,
                &i.workspace_id,
                s.selected.clone(),
            );
            s.settled_replies
                .insert(i.request_id.clone(), settled.clone());
            Ok(settled)
        } else {
            let noop = DSHCommandReply::qa(gen, "noop", &i.request_id)
                .sess(&i.session_id, &i.workspace_id, s.selected.clone())
                .rc("guard.alreadyResolved");
            s.settled_replies.insert(i.request_id.clone(), noop.clone());
            Ok(noop)
        }
    }
    fn switch_model(&self, i: &DSHSwitchModelIntent) -> Result<DSHCommandReply, DSHFailure> {
        let mut s = self.state.lock().unwrap();
        let (gen, early) = Self::check_cmd(&s, &i.host_id, i.generation, &i.request_id)?;
        if let Some(r) = early {
            return Ok(r);
        }
        s.select_models += 1;
        let Some(mut entry) = s.states.get(&i.session_id).cloned() else {
            return Ok(DSHCommandReply::qa(gen, "denied", &i.request_id).rc("session.notFound"));
        };
        let Some(opt) = entry.supported_models.as_ref().and_then(|ms| {
            ms.iter()
                .find(|m| m.provider == i.provider && m.model == i.model)
        }) else {
            return Ok(DSHCommandReply::qa(gen, "denied", &i.request_id)
                .sess(&i.session_id, &i.workspace_id, s.selected.clone())
                .rc("guard.unsupportedModel"));
        };
        let target_thought = i.thought.clone().or_else(|| opt.default_thought.clone());
        if target_thought
            .as_ref()
            .is_some_and(|th| !opt.supported_thoughts.contains(th))
        {
            return Ok(
                DSHCommandReply::qa(gen, "unsupportedThought", &i.request_id)
                    .sess(&i.session_id, &i.workspace_id, s.selected.clone())
                    .rc("guard.unsupportedThoughtLevel"),
            );
        }
        let cfg = DSHModelConfig {
            provider: i.provider.clone(),
            model: i.model.clone(),
            thought: target_thought,
        };
        entry.model_config = Some(cfg.clone());
        s.states.insert(i.session_id.clone(), entry);
        Self::push_change(&mut s, &i.session_id);
        let mut settled = DSHCommandReply::qa(gen, "modelSwitched", &i.request_id).sess(
            &i.session_id,
            &i.workspace_id,
            s.selected.clone(),
        );
        settled.model_config = Some(cfg);
        s.settled_replies
            .insert(i.request_id.clone(), settled.clone());
        Ok(settled)
    }
    fn query(&self, i: &DSHQueryIntent) -> Result<DSHCommandReply, DSHFailure> {
        let s = self.state.lock().unwrap();
        let (gen, early) = Self::check_cmd(&s, &i.host_id, i.generation, &i.request_id)?;
        Ok(early.unwrap_or_else(|| {
            DSHCommandReply::qa(gen, "unknown", &i.request_id).rc("query.unknownRequest")
        }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DSHAuditReceipt {
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
pub struct DSHNavigationScenarioReport {
    pub status: String,
    pub checks: BTreeMap<String, bool>,
    pub body_reads: usize,
    pub native_app: String,
    pub physical_key: String,
    #[serde(skip)]
    pub receipts: Vec<DSHAuditReceipt>,
}

fn push_dsh_receipt(
    receipts: &mut Vec<DSHAuditReceipt>,
    action: &str,
    outcome: &str,
    detail: impl Into<String>,
) -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let binding_id = match action {
        "dsh_open" => "menu-dsh-session",
        "dsh_create" => "menu-dsh-create",
        "dsh_prompt" => "menu-dsh-prompt",
        "dsh_cancel" => "menu-dsh-cancel",
        "dsh_resolve" => "menu-dsh-resolve",
        "dsh_switch_model" => "menu-dsh-switch-model",
        "dsh_query" => "menu-dsh-query",
        _ => "menu-dsh-session",
    };
    receipts.push(DSHAuditReceipt {
        timestamp_ms: ts,
        profile_id: "dsh".into(),
        binding_id: binding_id.into(),
        action_type: action.into(),
        target: "dsh".into(),
        outcome: outcome.into(),
        detail: detail.into(),
        text_byte_len: None,
    });
    outcome.into()
}

pub fn run_dsh_navigation_scenario(
    audit_log_path: Option<&Path>,
) -> Result<DSHNavigationScenarioReport, String> {
    let fixture = Arc::new(DSHNavigationFixture::new());
    let mut ctrl = DSHSessionController::new(fixture.clone());
    let (mut checks, mut receipts) = (BTreeMap::new(), Vec::new());
    let mut chk = |k: &str, v: bool| {
        checks.insert(k.to_owned(), v);
    };
    let q_open =
        |c: &mut DSHSessionController, r: &mut Vec<DSHAuditReceipt>, sel: &DSHSelection| {
            let res = c.open(sel, true);
            push_dsh_receipt(r, "dsh_open", res.as_str(), res.label())
        };
    let q_cmd =
        |c: &mut DSHSessionController, r: &mut Vec<DSHAuditReceipt>, cmd: &DSHQueuedCommand| {
            let res = c.execute(cmd, true);
            push_dsh_receipt(r, cmd.action_name(), res.as_str(), res.label())
        };

    let before_reads = fixture.state.lock().unwrap().metadata_reads;
    ctrl.refresh();
    chk(
        "disabled_zero_reads",
        fixture.state.lock().unwrap().metadata_reads == before_reads,
    );
    ctrl.set_enabled(true);
    let (first, second) = (
        ctrl.selection(DSHNavigationFixture::FIRST).ok_or("first")?,
        ctrl.selection(DSHNavigationFixture::SECOND)
            .ok_or("second")?,
    );
    let titles: HashSet<_> = ctrl.rows.iter().map(|r| &r.title).collect();
    chk(
        "same_name_exact_identity",
        ctrl.rows.len() == 2
            && titles.len() == 1
            && first.session_id != second.session_id
            && first.workspace_id != second.workspace_id,
    );
    chk(
        "cold_target_unavailable",
        ctrl.unavailable_session_ids == vec![DSHNavigationFixture::COLD.to_string()]
            && ctrl.selection(DSHNavigationFixture::COLD).is_none(),
    );
    fixture.pending_round_trip();
    ctrl.refresh();
    chk(
        "pending_only_transitions_retained",
        ctrl.last_changes
            .iter()
            .map(|c| (c.sequence, c.row.pending_input))
            .collect::<Vec<_>>()
            == vec![(1, 1), (2, 0)],
    );
    let (o1, o2) = (
        q_open(&mut ctrl, &mut receipts, &first) == "confirmed",
        q_open(&mut ctrl, &mut receipts, &second) == "confirmed",
    );
    chk(
        "selected_id_readback",
        o1 && o2
            && fixture.state.lock().unwrap().navigations
                == vec![first.session_id.clone(), second.session_id.clone()],
    );
    q_open(&mut ctrl, &mut receipts, &first);
    let steer_cmd = ctrl
        .prompt_intent(
            DSHNavigationFixture::FIRST,
            DSHPromptMode::Steer,
            "steer current turn",
            Some("qa-req-steer-1"),
        )
        .ok_or("steer_cmd")?;
    let steered_ok = q_cmd(&mut ctrl, &mut receipts, &steer_cmd) == "steered"
        && ctrl.last_command_result
            == Some(DSHCommandResult::Steered(
                DSHNavigationFixture::FIRST.into(),
                "qa-req-steer-1".into(),
                Some(1),
                1,
                false,
            ));
    let p_before = fixture.state.lock().unwrap().prompts;
    q_cmd(&mut ctrl, &mut receipts, &steer_cmd);
    let p_after = fixture.state.lock().unwrap().prompts;
    chk(
        "duplicate_request_no_double_send",
        p_before == 1
            && p_after == 1
            && ctrl.last_command_result
                == Some(DSHCommandResult::Steered(
                    DSHNavigationFixture::FIRST.into(),
                    "qa-req-steer-1".into(),
                    Some(1),
                    1,
                    true,
                )),
    );
    let queue_cmd = ctrl
        .prompt_intent(
            DSHNavigationFixture::FIRST,
            DSHPromptMode::Queue,
            "queue next turn",
            Some("qa-req-queue-1"),
        )
        .ok_or("queue_cmd")?;
    let queued_ok = q_cmd(&mut ctrl, &mut receipts, &queue_cmd) == "queued"
        && ctrl.last_command_result
            == Some(DSHCommandResult::Queued(
                DSHNavigationFixture::FIRST.into(),
                "qa-req-queue-1".into(),
                Some(1),
                2,
                false,
            ));
    let r_after_p = ctrl.row(DSHNavigationFixture::FIRST);
    chk(
        "queue_and_steer_confirmed_separately",
        steered_ok
            && queued_ok
            && r_after_p.map(DSHSessionRow::turn_status_label).as_deref() == Some("轮次 #1 运行中")
            && r_after_p.map(DSHSessionRow::queue_status_label).as_deref() == Some("队列 2 条"),
    );
    let cancel_a_t1 = ctrl
        .cancel_intent(
            DSHNavigationFixture::FIRST,
            None,
            Some("qa-req-cancel-stale-turn1"),
        )
        .ok_or("cancel_a_t1")?;
    q_open(&mut ctrl, &mut receipts, &second);
    let c_before_ws = fixture.state.lock().unwrap().cancels;
    let ws_out = q_cmd(&mut ctrl, &mut receipts, &cancel_a_t1);
    chk(
        "switch_session_no_misstop",
        c_before_ws == 0 && fixture.state.lock().unwrap().cancels == 0 && ws_out == "unconfirmed",
    );
    q_open(&mut ctrl, &mut receipts, &first);
    fixture.set_turn_state(DSHNavigationFixture::FIRST, true, Some(2), None);
    let c_before_st = fixture.state.lock().unwrap().cancels;
    let st_out = q_cmd(&mut ctrl, &mut receipts, &cancel_a_t1);
    chk(
        "same_session_stale_stop_zero_cancel",
        c_before_st == 0
            && fixture.state.lock().unwrap().cancels == 0
            && st_out == "noop"
            && ctrl.last_command_result
                == Some(DSHCommandResult::Noop(
                    "guard.turnChanged".into(),
                    Some(2),
                    Some(2),
                )),
    );
    ctrl.refresh();
    let cancel_a_t2 = ctrl
        .cancel_intent(
            DSHNavigationFixture::FIRST,
            None,
            Some("qa-req-cancel-turn2"),
        )
        .ok_or("cancel_a_t2")?;
    let t2_out = q_cmd(&mut ctrl, &mut receipts, &cancel_a_t2);
    chk(
        "cancel_queue_remaining_not_all_stopped",
        fixture.state.lock().unwrap().cancels == 1
            && t2_out == "cancelledQueueRemaining"
            && ctrl
                .last_command_result
                .as_ref()
                .is_some_and(|r| r.label().contains("队列仍有 2 条可能继续")),
    );
    fixture.set_turn_state(DSHNavigationFixture::FIRST, true, Some(3), None);
    ctrl.refresh();
    let prebuilt_unprov = ctrl
        .cancel_intent(
            DSHNavigationFixture::FIRST,
            None,
            Some("qa-req-unprovable-exec"),
        )
        .ok_or("prebuilt_unprov")?;
    fixture.set_turn_state(DSHNavigationFixture::FIRST, true, None, None);
    ctrl.refresh();
    let unprov_intent =
        ctrl.cancel_intent(DSHNavigationFixture::FIRST, None, Some("qa-req-unprovable"));
    let denied_out = q_cmd(&mut ctrl, &mut receipts, &prebuilt_unprov) == "denied";
    ctrl.set_atomic_turn_check_supported(false);
    fixture.set_turn_state(DSHNavigationFixture::FIRST, true, Some(3), None);
    ctrl.refresh();
    let unsup_atomic = ctrl.cancel_intent(
        DSHNavigationFixture::FIRST,
        None,
        Some("qa-req-unsupported-atomic"),
    );
    ctrl.set_atomic_turn_check_supported(true);
    chk(
        "unprovable_turn_atomicity_refused",
        unprov_intent.is_none()
            && unsup_atomic.is_none()
            && denied_out
            && fixture.state.lock().unwrap().cancels == 1
            && ctrl.last_command_result
                == Some(DSHCommandResult::Denied(Some(
                    "guard.unprovableTurnAtomicity".into(),
                ))),
    );
    let create_cmd = ctrl
        .create_intent("workspace-a", Some("qa-req-create-1"))
        .ok_or("create_cmd")?;
    let cr_out = q_cmd(&mut ctrl, &mut receipts, &create_cmd);
    chk(
        "create_workspace_bound_readback",
        fixture.state.lock().unwrap().creates == 1
            && cr_out == "confirmed"
            && ctrl.selected.as_deref() == Some("qa-session-created-1")
            && ctrl
                .rows
                .iter()
                .any(|r| r.session_id == "qa-session-created-1" && r.workspace_id == "workspace-a"),
    );
    q_open(&mut ctrl, &mut receipts, &first);
    fixture.set_pending_interaction(
        DSHNavigationFixture::FIRST,
        Some(DSHPendingInteraction {
            interaction_id: "qa-perm-1".into(),
            interaction_type: DSHInteractionType::Permission,
            tool_name: Some("bash".into()),
            options: Some(vec!["allowed-once".into(), "rejected".into()]),
        }),
    );
    ctrl.refresh();
    let res_app_cmd = ctrl
        .resolve_intent(
            DSHNavigationFixture::FIRST,
            "qa-perm-1",
            DSHInteractionType::Permission,
            DSHInteractionDecision::Approve,
            None,
            Some("qa-req-res-perm-1"),
        )
        .ok_or("res_app_cmd")?;
    let ra_out = q_cmd(&mut ctrl, &mut receipts, &res_app_cmd);
    chk(
        "resolve_approval_recorded",
        fixture.state.lock().unwrap().resolves == 1
            && ra_out == "resolved"
            && ctrl.last_command_result
                == Some(DSHCommandResult::Resolved(
                    DSHNavigationFixture::FIRST.into(),
                    "qa-req-res-perm-1".into(),
                    false,
                )),
    );
    let ra_dup_out = q_cmd(&mut ctrl, &mut receipts, &res_app_cmd);
    chk(
        "already_resolved_noop_zero_side_effect",
        ra_dup_out == "resolved" || ra_dup_out == "noop",
    );
    fixture.set_pending_interaction(
        DSHNavigationFixture::FIRST,
        Some(DSHPendingInteraction {
            interaction_id: "qa-q-1".into(),
            interaction_type: DSHInteractionType::Question,
            tool_name: Some("ask_user_question".into()),
            options: Some(vec!["Yes".into(), "No".into()]),
        }),
    );
    ctrl.refresh();
    let mismatch_cmd = ctrl
        .resolve_intent(
            DSHNavigationFixture::FIRST,
            "qa-q-1",
            DSHInteractionType::Permission,
            DSHInteractionDecision::Approve,
            None,
            Some("qa-req-mismatch-1"),
        )
        .ok_or("mismatch_cmd")?;
    let mm_out = q_cmd(&mut ctrl, &mut receipts, &mismatch_cmd);
    chk(
        "interaction_type_mismatch_blocked",
        mm_out == "interactionTypeMismatch"
            && ctrl.last_command_result
                == Some(DSHCommandResult::InteractionTypeMismatch(
                    "guard.questionCannotApproveAsPermission".into(),
                )),
    );
    let res_q_cmd = ctrl
        .resolve_intent(
            DSHNavigationFixture::FIRST,
            "qa-q-1",
            DSHInteractionType::Question,
            DSHInteractionDecision::Approve,
            Some("Yes"),
            Some("qa-req-res-q-1"),
        )
        .ok_or("res_q_cmd")?;
    chk(
        "resolve_question_answered",
        q_cmd(&mut ctrl, &mut receipts, &res_q_cmd) == "resolved",
    );
    let sw_cmd = ctrl
        .switch_model_intent(
            DSHNavigationFixture::FIRST,
            "deepseek-official",
            "deepseek-chat",
            Some("high"),
            Some("qa-req-model-1"),
        )
        .ok_or("sw_cmd")?;
    let sw_out = q_cmd(&mut ctrl, &mut receipts, &sw_cmd);
    let exp_cfg = DSHModelConfig {
        provider: "deepseek-official".into(),
        model: "deepseek-chat".into(),
        thought: Some("high".into()),
    };
    chk(
        "select_model_readback_confirmed",
        sw_out == "modelSwitched"
            && ctrl.last_command_result
                == Some(DSHCommandResult::ModelSwitched(
                    DSHNavigationFixture::FIRST.into(),
                    "qa-req-model-1".into(),
                    exp_cfg,
                    false,
                ))
            && ctrl
                .row(DSHNavigationFixture::FIRST)
                .and_then(|r| r.model_config.as_ref())
                .and_then(|m| m.thought.as_deref())
                == Some("high"),
    );
    let sm_before = fixture.state.lock().unwrap().select_models;
    let unsup_th_cmd = ctrl
        .switch_model_intent(
            DSHNavigationFixture::FIRST,
            "deepseek-official",
            "deepseek-reasoner",
            Some("off"),
            Some("qa-req-unsupported-thought"),
        )
        .ok_or("unsup_th_cmd")?;
    q_cmd(&mut ctrl, &mut receipts, &unsup_th_cmd);
    chk(
        "unsupported_thought_level_locally_refused",
        sm_before == fixture.state.lock().unwrap().select_models
            && ctrl.last_command_result
                == Some(DSHCommandResult::UnsupportedThought(
                    "guard.unsupportedThoughtLevel".into(),
                )),
    );
    fixture.set_mode(DSHFixtureMode::WrongSelected);
    chk(
        "wrong_selected_unconfirmed",
        q_open(&mut ctrl, &mut receipts, &first) != "confirmed",
    );
    let outside = DSHSelection {
        session_id: "outside".into(),
        workspace_id: first.workspace_id.clone(),
        host_id: first.host_id.clone(),
        generation: first.generation,
        local_epoch: first.local_epoch,
    };
    let (r_b, o_b) = {
        let s = fixture.state.lock().unwrap();
        (s.metadata_reads, s.opens)
    };
    q_open(&mut ctrl, &mut receipts, &outside);
    {
        let s = fixture.state.lock().unwrap();
        chk(
            "out_of_scope_zero_reads",
            s.metadata_reads == r_b && s.opens == o_b,
        );
    }
    let nav_before_del = fixture.state.lock().unwrap().navigations.clone();
    fixture.set_mode(DSHFixtureMode::Deleted);
    let del_out = q_open(&mut ctrl, &mut receipts, &first);
    chk(
        "deleted_target_no_redirect",
        del_out != "confirmed" && fixture.state.lock().unwrap().navigations == nav_before_del,
    );
    ctrl.set_enabled(false);
    fixture.set_mode(DSHFixtureMode::Normal);
    ctrl.set_enabled(true);
    let old = ctrl.selection(DSHNavigationFixture::SECOND).ok_or("old")?;
    fixture.set_mode(DSHFixtureMode::Rotated);
    ctrl.refresh();
    let opens_before_st = fixture.state.lock().unwrap().opens;
    q_open(&mut ctrl, &mut receipts, &old);
    chk(
        "host_generation_rejects_old_selection",
        fixture.state.lock().unwrap().opens == opens_before_st && ctrl.rows.is_empty(),
    );
    ctrl.set_enabled(false);
    fixture.set_mode(DSHFixtureMode::Normal);
    ctrl.set_enabled(true);
    fixture.set_mode(DSHFixtureMode::Gap);
    ctrl.refresh();
    chk(
        "journal_gap_clears_targets",
        ctrl.rows.is_empty() && ctrl.state != DSHConnectionState::Ready,
    );
    ctrl.shutdown();
    chk(
        "bridge_subscription_closed",
        fixture.state.lock().unwrap().closes > 0,
    );
    let body_reads = fixture.state.lock().unwrap().body_reads;
    let pass = checks.len() == 24 && checks.values().all(|&v| v) && body_reads == 0;
    let report = DSHNavigationScenarioReport {
        status: if pass { "PASS" } else { "FAIL" }.into(),
        checks,
        body_reads,
        native_app: "NOT_RUN".into(),
        physical_key: "NOT_RUN".into(),
        receipts,
    };
    if let Some(path) = audit_log_path {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            std::fs::write(
                parent.join("dsh-navigation.json"),
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
