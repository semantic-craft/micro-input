//! Versioned local proposal boundary. No transport, execution or networking.
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Stamp {
    pub document_id: String,
    pub revision: u64,
    pub sha256: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    RenameProfile {
        profile_id: String,
        name: String,
    },
    RenameLayer {
        layer_id: String,
        name: String,
    },
    BindAction {
        profile_id: String,
        layer_id: String,
        binding_id: String,
        #[serde(deserialize_with = "required_optional_string")]
        action_id: Option<String>,
    },
}
fn required_optional_string<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error> {
    Option::<String>::deserialize(deserializer)
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub format: String,
    pub format_version: u32,
    pub base: Stamp,
    pub operations: Vec<Operation>,
    pub document: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    api_version: u32,
    workbench: PathBuf,
    expected: Option<Stamp>,
    operations: Option<Vec<Operation>>,
    output: Option<PathBuf>,
}
#[derive(Debug, Serialize)]
pub struct Error {
    pub code: &'static str,
    pub message: String,
}
impl Error {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
type Result<T> = std::result::Result<T, Error>;
fn invalid(message: impl Into<String>) -> Error {
    Error::new("invalid_request", message)
}
fn io(message: impl Into<String>) -> Error {
    Error::new("io_error", message)
}
fn stale() -> Error {
    Error::new(
        "stale_revision",
        "文档身份、修订、原字节或选中记录已变化；请重新读取",
    )
}
fn capabilities() -> Value {
    json!({"local_proposal":true,"device_apply":false,"host_execute":false,"operations":["rename_profile","rename_layer","bind_action"]})
}
fn stamp(draft: &Draft, bytes: &[u8]) -> Stamp {
    Stamp {
        document_id: draft.document_id.clone(),
        revision: draft.revision,
        sha256: format!("{:x}", Sha256::digest(bytes)),
    }
}
fn apply(draft: &Draft, operations: &[Operation]) -> Result<(Draft, Vec<String>)> {
    if draft.read_only || draft.loaded_format != 2 {
        return Err(Error::new(
            "read_only",
            "仅支持已知 v2 本地文档；原文件保留",
        ));
    }
    if operations.is_empty() || operations.len() > 100 {
        return Err(invalid("operations 必须包含 1–100 项"));
    }
    let mut next = draft.clone();
    let mut changes = vec![];
    for op in operations {
        let changed = match op {
            Operation::RenameProfile { profile_id, name } => {
                let old = next
                    .document
                    .editable
                    .graph
                    .profiles
                    .get(profile_id)
                    .ok_or_else(|| Error::new("invalid_operation", "配置引用不存在"))?
                    .name
                    .clone();
                next.rename_profile(profile_id, name)
                    .map(|_| format!("配置：{old} → {name}"))
            }
            Operation::RenameLayer { layer_id, name } => {
                let old = next
                    .document
                    .editable
                    .graph
                    .layers
                    .get(layer_id)
                    .ok_or_else(|| Error::new("invalid_operation", "层引用不存在"))?
                    .name
                    .clone();
                next.rename_layer(layer_id, name)
                    .map(|_| format!("层：{old} → {name}"))
            }
            Operation::BindAction {
                profile_id,
                layer_id,
                binding_id,
                action_id,
            } => next
                .bind_existing_action(profile_id, layer_id, binding_id, action_id.as_deref())
                .map(|_| {
                    let name = action_id
                        .as_ref()
                        .map(|id| next.document.editable.graph.actions[id].name.as_str())
                        .unwrap_or("保留原始绑定");
                    format!("控件 {binding_id}：{name}")
                }),
        }
        .map_err(|m| Error::new("invalid_operation", m))?;
        changes.push(changed);
    }
    Ok((next, changes))
}
#[derive(Clone, Serialize)]
pub struct Preview {
    pub token: String,
    pub changes: Vec<String>,
    pub base: Stamp,
    pub document: Value,
}
pub(super) struct Pending {
    preview: Preview,
    proposal: Proposal,
}
impl DocumentStore {
    fn agent_locks(&self) -> Result<(Lock, Lock)> {
        if self.blocked {
            return Err(Error::new("read_only", "本地文档未载入"));
        }
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| invalid("请先打开本地工作台"))?;
        let (selection, _) = self
            .selection
            .as_ref()
            .ok_or_else(|| invalid("缺少选择记录"))?;
        let selected = Lock::acquire(selection.with_extension("lock"))
            .map_err(|m| Error::new("conflict", m))?;
        let document =
            Lock::acquire(path.with_extension("lock")).map_err(|m| Error::new("conflict", m))?;
        Ok((selected, document))
    }
    fn agent_bytes(&self) -> Result<&[u8]> {
        let path = self.path.as_ref().ok_or_else(|| invalid("没有文档"))?;
        if path.with_extension("pending").exists() {
            return Err(Error::new(
                "conflict",
                "来源文档存在未完成保存记录；请先检查 .pending 文件",
            ));
        }
        let expected = self
            .expected
            .as_ref()
            .ok_or_else(|| Error::new("not_found", "请先在工作台保存本地文档"))?;
        let (selection, bytes) = self
            .selection
            .as_ref()
            .ok_or_else(|| invalid("没有选择记录"))?;
        if read(path).map_err(io)?.as_ref() != Some(expected)
            || &read(selection).map_err(io)? != bytes
        {
            return Err(stale());
        }
        Ok(expected)
    }
    fn check_agent_base(&self, base: &Stamp) -> Result<()> {
        if &stamp(&self.draft, self.agent_bytes()?) != base {
            return Err(stale());
        }
        Ok(())
    }
    fn verify_proposal(&self, proposal: &Proposal) -> Result<(Draft, Vec<String>)> {
        if proposal.format != "micro-input-proposal" || proposal.format_version != 1 {
            return Err(Error::new("unsupported_version", "提案格式或版本不支持"));
        }
        self.check_agent_base(&proposal.base)?;
        let (mut next, changes) = apply(&self.draft, &proposal.operations)?;
        let candidate = Draft::from_bytes(
            &serde_json::to_vec(&proposal.document).map_err(|e| invalid(e.to_string()))?,
        )
        .map_err(|m| Error::new("invalid_operation", m))?;
        if candidate.read_only
            || candidate.loaded_format != 2
            || candidate.document_id == self.draft.document_id
        {
            return Err(Error::new(
                "invalid_operation",
                "提案必须是独立、已知 v2 文档",
            ));
        }
        next.document.document_id = candidate.document_id;
        next.document.origin["agent_proposal"] = json!({"source_document_id":proposal.base.document_id,"source_revision":proposal.base.revision,"source_sha256":proposal.base.sha256});
        if serde_json::to_value(&next.document).map_err(|e| io(e.to_string()))? != proposal.document
        {
            return Err(Error::new(
                "invalid_operation",
                "提案文档与受限操作结果不一致",
            ));
        }
        Ok((
            Draft::from_document(next.document, 2, false).map_err(io)?,
            changes,
        ))
    }
    pub fn inspect_agent_proposal(&mut self, text: &str) -> Result<Preview> {
        self.agent_preview = None;
        if text.len() as u64 > LIMIT {
            return Err(invalid("提案超过 4 MiB"));
        }
        let proposal: Proposal = serde_json::from_str(text).map_err(|e| invalid(e.to_string()))?;
        let _locks = self.agent_locks()?;
        let (_, changes) = self.verify_proposal(&proposal)?;
        let preview = Preview {
            token: crate::document::new_id(),
            base: proposal.base.clone(),
            changes,
            document: proposal.document.clone(),
        };
        self.agent_preview = Some(Pending {
            preview: preview.clone(),
            proposal,
        });
        Ok(preview)
    }
    pub fn reject_agent_proposal(&mut self) {
        self.agent_preview = None;
    }
    pub fn accept_agent_proposal(&mut self, token: &str) -> Result<()> {
        let item = self
            .agent_preview
            .as_ref()
            .filter(|p| p.preview.token == token)
            .ok_or_else(|| invalid("提案预览已失效"))?;
        // select_independent owns the selection lock itself; hold the source lock across
        // verification and independent save, so another app save cannot race acceptance.
        let path = self.path.as_ref().ok_or_else(|| invalid("没有文档"))?;
        let _source =
            Lock::acquire(path.with_extension("lock")).map_err(|m| Error::new("conflict", m))?;
        let (next, _) = self.verify_proposal(&item.proposal)?;
        self.select_independent(next)
            .map_err(|m| Error::new("conflict", m))?;
        self.agent_preview = None;
        Ok(())
    }
}
/// One request per process. No implicit workbench discovery or writes on read/validate.
pub fn execute(command: &str, text: &str) -> Result<Value> {
    if text.len() as u64 > LIMIT {
        return Err(invalid("请求超过 4 MiB"));
    }
    let request: Request = serde_json::from_str(text).map_err(|e| invalid(e.to_string()))?;
    if request.api_version != 1 {
        return Err(Error::new("unsupported_version", "api_version 仅支持 1"));
    }
    if !["read", "validate", "patch"].contains(&command) {
        return Err(Error::new(
            "capability_unavailable",
            "仅提供本地草稿 read/validate/patch；设备应用、宿主执行和其他操作不可用",
        ));
    }
    if !request.workbench.is_absolute() {
        return Err(invalid(
            "使用 mi draft read/validate/patch，workbench 必须是绝对路径",
        ));
    }
    if command == "read"
        && (request.expected.is_some() || request.operations.is_some() || request.output.is_some())
    {
        return Err(invalid("read 不接受 expected/operations/output"));
    }
    if command == "validate" && request.output.is_some() {
        return Err(invalid("validate 不接受 output"));
    }
    if command == "patch" && !request.output.as_ref().is_some_and(|p| p.is_absolute()) {
        return Err(invalid("patch 需要绝对 output 路径"));
    }
    let mut store = DocumentStore::default();
    store.open(request.workbench).map_err(io)?;
    // Missing file must be reported before lock creation (which needs its parent).
    if store.expected.is_none() {
        return Err(Error::new("not_found", "本地文档不存在；请先在工作台保存"));
    }
    let _locks = store.agent_locks()?;
    let bytes = store.agent_bytes()?;
    let base = stamp(&store.draft, bytes);
    if command == "read" {
        return Ok(
            json!({"stamp":base,"document":serde_json::from_slice::<Value>(bytes).map_err(|e|io(e.to_string()))?,"source_kind":store.draft.document.origin["kind"],"read_only":store.draft.read_only || store.draft.loaded_format!=2,"capabilities":capabilities()}),
        );
    }
    store.check_agent_base(
        request
            .expected
            .as_ref()
            .ok_or_else(|| invalid("缺少 expected"))?,
    )?;
    let operations = request
        .operations
        .ok_or_else(|| invalid("缺少 operations"))?;
    let (mut next, changes) = apply(&store.draft, &operations)?;
    if command == "validate" {
        return Ok(json!({"base":base,"document":next.document,"changes":changes}));
    }
    next.document.document_id = crate::document::new_id();
    next.document.origin["agent_proposal"] = json!({"source_document_id":base.document_id,"source_revision":base.revision,"source_sha256":base.sha256});
    let document = serde_json::to_value(&next.document).map_err(|e| io(e.to_string()))?;
    let proposal = Proposal {
        format: "micro-input-proposal".into(),
        format_version: 1,
        base: base.clone(),
        operations,
        document: document.clone(),
    };
    let output = request.output.unwrap();
    let bytes = serde_json::to_vec_pretty(&proposal).map_err(|e| io(e.to_string()))?;
    if bytes.len() as u64 > LIMIT {
        return Err(invalid("提案超过 4 MiB"));
    }
    let mut file = OpenOptions::new()
        .write(true)
        .mode(0o600)
        .create_new(true)
        .open(&output)
        .map_err(|e| {
            Error::new(
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    "conflict"
                } else {
                    "io_error"
                },
                e.to_string(),
            )
        })?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| io(e.to_string()))?;
    Ok(json!({"base":base,"document":document,"changes":changes,"proposal_path":output}))
}
