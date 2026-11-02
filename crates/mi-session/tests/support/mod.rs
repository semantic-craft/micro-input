// Shared fixed A/B harness: each integration test uses only its own entry.
#![allow(dead_code)]
use std::io::Read;
pub const OPT_IN_B: &str = "I_AUTHORIZE_EXACT_TEMPFILE_B";
pub const OPT_IN: &str = "I_AUTHORIZE_EXACT_TEMPFILE_A";
pub type Result<T> = std::result::Result<T, &'static str>;
pub fn authorize(value: Option<&str>) -> Result<()> {
    if value == Some(OPT_IN) {
        Ok(())
    } else {
        Err("operator_opt_in_absent")
    }
}
pub fn temp_name(name: &str) -> Result<()> {
    if name.len() == 19
        && name.starts_with("mi_")
        && name.ends_with(".txt")
        && name.as_bytes()[3..15]
            .iter()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
    {
        Ok(())
    } else {
        Err("invalid_temp_target")
    }
}
fn random<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0; N];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|_| "random_failed")?;
    Ok(bytes)
}

use async_trait::async_trait;
use mi_proto::{Framer, Incoming, JsonCodec, Method, NativeFramer, RpcCodec};
use mi_session::{DeviceFiles, DeviceSession, ReadOnlySchema, SessionConfig};
use mi_transport::{HidCandidate, HidLink, HidTransport, LinkBackend, TransportError};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::sync::{mpsc, oneshot, Mutex as AsyncMutex, Notify};

const PAYLOAD: &[u8] = b"{\"ok\":true}";
#[derive(Clone, Copy, PartialEq)]
enum Probe {
    A,
    B,
}
impl Probe {
    fn label(self) -> &'static str {
        match self {
            Self::A => "a",
            Self::B => "b",
        }
    }
    fn opt_in(self) -> &'static str {
        match self {
            Self::A => OPT_IN,
            Self::B => OPT_IN_B,
        }
    }
    fn payload(self) -> &'static [u8] {
        match self {
            Self::A => PAYLOAD,
            Self::B => b"micro-input\n",
        }
    }
    fn sha1(self) -> &'static str {
        match self {
            Self::A => "022d91f218046ab2e61cac1eb13d6a718f75df2b",
            Self::B => "8c6bf41b8e5eccf2860f597878b8878b560442fc",
        }
    }
}
const RPC_BUDGET: Duration = Duration::from_secs(4);
type Listing = BTreeMap<String, Value>;
struct Pending {
    id: u16,
    method: &'static str,
    response: oneshot::Sender<Result<Value>>,
}

/// Test-only broker: one native reader/handle for both the existing core and exact tempfile RPCs.
/// It observes foreign traffic but cannot establish exclusive ownership.
struct Broker {
    raw: Arc<dyn HidLink>,
    incoming: AsyncMutex<mpsc::UnboundedReceiver<Vec<u8>>>,
    outgoing: Mutex<NativeFramer>,
    expected: Mutex<BTreeSet<u16>>,
    used: Mutex<BTreeSet<u16>>,
    diagnostic: Mutex<Option<Pending>>,
    foreign: AtomicUsize,
    closed: AtomicBool,
    changed: Notify,
    worker: Mutex<Option<tokio::task::JoinHandle<()>>>,
    target: String,
    probe: Probe,
}
impl Broker {
    fn start(raw: Arc<dyn HidLink>, target: String, probe: Probe) -> Arc<Self> {
        let (tx, rx) = mpsc::unbounded_channel();
        let broker = Arc::new(Self {
            raw,
            incoming: AsyncMutex::new(rx),
            outgoing: Mutex::new(NativeFramer::default()),
            expected: Mutex::new(BTreeSet::new()),
            used: Mutex::new(BTreeSet::new()),
            diagnostic: Mutex::new(None),
            foreign: AtomicUsize::new(0),
            closed: AtomicBool::new(false),
            changed: Notify::new(),
            worker: Mutex::new(None),
            target,
            probe,
        });
        let held = broker.clone();
        let task = tokio::spawn(async move {
            let mut framer = NativeFramer::default();
            while !held.closed.load(Ordering::SeqCst) {
                let report = match held.raw.read().await {
                    Ok(r) => r,
                    Err(_) => break,
                };
                if report.get(1) != Some(&2) {
                    held.foreign.fetch_add(1, Ordering::SeqCst);
                }
                let lines = match framer.feed(&report) {
                    Ok(v) => v,
                    Err(_) => {
                        held.foreign.fetch_add(1, Ordering::SeqCst);
                        break;
                    }
                };
                for line in lines {
                    let id = match mi_proto::response_id(&line) {
                        Ok(id) => id,
                        Err(_) => {
                            held.foreign.fetch_add(1, Ordering::SeqCst);
                            held.close();
                            return;
                        }
                    };
                    let is_diagnostic = id.is_some_and(|id| {
                        held.diagnostic
                            .lock()
                            .unwrap()
                            .as_ref()
                            .is_some_and(|p| u64::from(p.id) == id)
                    });
                    if is_diagnostic {
                        let pending = held.diagnostic.lock().unwrap().take().unwrap();
                        let response = diagnostic_response(&line, pending.method);
                        let _ = pending.response.send(response);
                    } else {
                        let owned = id
                            .and_then(|id| u16::try_from(id).ok())
                            .is_some_and(|id| held.expected.lock().unwrap().remove(&id));
                        if !owned {
                            held.foreign.fetch_add(1, Ordering::SeqCst);
                        }
                        match NativeFramer::default().encode(&line) {
                            Ok(reports) => {
                                for r in reports {
                                    if tx.send(r).is_err() {
                                        held.close();
                                        return;
                                    }
                                }
                            }
                            Err(_) => {
                                held.close();
                                return;
                            }
                        }
                    }
                }
            }
            held.close();
        });
        *broker.worker.lock().unwrap() = Some(task);
        broker
    }
    fn uncontested(&self) -> Result<()> {
        if self.closed.load(Ordering::SeqCst) {
            Err("link_closed")
        } else if self.foreign.load(Ordering::SeqCst) != 0 {
            Err("foreign_traffic_observed")
        } else if !self.expected.lock().unwrap().is_empty() {
            Err("core_response_unconfirmed")
        } else {
            Ok(())
        }
    }
    async fn quiet(&self, duration: Duration) -> Result<()> {
        self.uncontested()?;
        tokio::time::sleep(duration).await;
        self.uncontested()
    }
    async fn exact(&self, operation: Exact) -> Result<Value> {
        self.uncontested()?;
        temp_name(&self.target)?;
        let (method, params) = match operation {
            Exact::Create if self.probe == Probe::A => (
                "fs.write",
                json!({"file":self.target,"data":"{\"ok\":true}"}),
            ),
            Exact::Chunk1 if self.probe == Probe::B => (
                "fs.writebin",
                json!({"file":self.target,"data":"bWljcm8t","append":true,"completed":false,"offset":0}),
            ),
            Exact::Chunk2 if self.probe == Probe::B => (
                "fs.writebin",
                json!({"file":self.target,"data":"aW5wdXQK","append":true,"completed":true,"offset":6}),
            ),
            Exact::Create | Exact::Chunk1 | Exact::Chunk2 => return Err("wrong_probe_operation"),
            Exact::Read => (
                "fs.readbin",
                json!({"file":self.target,"offset":0,"len":self.probe.payload().len()}),
            ),
            Exact::Delete => ("fs.delete", json!({"file":self.target})),
        };
        let seed = u16::from_le_bytes(random::<2>()?);
        let id = (0..99u16)
            .map(|n| 900 + ((u32::from(seed) + u32::from(n)) % 99) as u16)
            .find(|id| !self.used.lock().unwrap().contains(id))
            .ok_or("request_ids_exhausted")?;
        self.used.lock().unwrap().insert(id);
        let (tx, rx) = oneshot::channel();
        *self.diagnostic.lock().unwrap() = Some(Pending {
            id,
            method,
            response: tx,
        });
        let wire = json!({"method":method,"params":params,"id":id}).to_string();
        // No arbitrary method/path entry point: only the fixed operations above.
        let outcome = tokio::time::timeout(RPC_BUDGET, async {
            for report in NativeFramer::default()
                .encode(wire.as_bytes())
                .map_err(|_| "framing_failed")?
            {
                self.uncontested()?;
                self.raw.write(&report).await.map_err(|_| "send_unknown")?;
            }
            rx.await.map_err(|_| "response_unknown")?
        })
        .await
        .map_err(|_| "timeout_unknown")
        .and_then(|v| v);
        self.diagnostic.lock().unwrap().take();
        outcome
    }
    async fn finish(&self) {
        self.close();
        let task = self.worker.lock().unwrap().take();
        if let Some(task) = task {
            let _ = task.await;
        }
    }
}
#[derive(Clone, Copy)]
enum Exact {
    Create,
    Chunk1,
    Chunk2,
    Read,
    Delete,
}
fn diagnostic_response(line: &[u8], expected: &str) -> Result<Value> {
    let mut value: Value = serde_json::from_slice(line).map_err(|_| "invalid_response")?;
    // The production decoder deliberately rejects mutation echoes. Validate the exact
    // private echo first, then reuse all its envelope/result/error checks.
    for key in ["method", "m"] {
        if value.get(key).is_some_and(|v| v.as_str() != Some(expected)) {
            return Err("mismatched_method_echo");
        }
    }
    if value.get("params").is_some() || value.get("p").is_some() {
        return Err("unexpected_response_params");
    }
    value
        .as_object_mut()
        .ok_or("invalid_response")?
        .remove("method");
    value.as_object_mut().unwrap().remove("m");
    match JsonCodec
        .decode(&serde_json::to_vec(&value).map_err(|_| "invalid_response")?)
        .map_err(|_| "invalid_response")?
    {
        Incoming::Response(r) if r.raw.get("error").is_none() => Ok(r.raw),
        _ => Err("rpc_error_unknown"),
    }
}
#[async_trait]
impl HidLink for Broker {
    fn backend(&self) -> LinkBackend {
        self.raw.backend()
    }
    async fn write(&self, report: &[u8]) -> std::result::Result<(), TransportError> {
        // Observe core requests; never replace/relax the production read-only validator.
        for line in self
            .outgoing
            .lock()
            .unwrap()
            .feed(report)
            .map_err(|_| TransportError::Unsupported)?
        {
            let value: Value =
                serde_json::from_slice(&line).map_err(|_| TransportError::Unsupported)?;
            let id = value["id"]
                .as_u64()
                .and_then(|v| u16::try_from(v).ok())
                .ok_or(TransportError::Unsupported)?;
            self.expected.lock().unwrap().insert(id);
            self.used.lock().unwrap().insert(id);
        }
        self.raw.write(report).await
    }
    async fn read(&self) -> std::result::Result<Vec<u8>, TransportError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(TransportError::Disconnected);
        }
        let wake = self.changed.notified();
        tokio::pin!(wake);
        wake.as_mut().enable();
        let mut incoming = self.incoming.lock().await;
        if self.closed.load(Ordering::SeqCst) {
            return Err(TransportError::Disconnected);
        }
        tokio::select! { _ = wake => Err(TransportError::Disconnected), r = incoming.recv() => r.ok_or(TransportError::Disconnected) }
    }
    fn close(&self) {
        if !self.closed.swap(true, Ordering::SeqCst) {
            self.raw.close();
            self.changed.notify_waiters();
        }
    }
}

pub struct Outcome {
    pub passed: bool,
    pub cleanup_confirmed: bool,
    pub reason: &'static str,
    pub directory: PathBuf,
    pub target: String,
}
struct Record {
    directory: PathBuf,
    target: String,
    sequence: usize,
    stage: &'static str,
    cleanup_confirmed: bool,
    mutation_attempted: bool,
}
impl Record {
    fn new(probe: Probe) -> Result<Self> {
        let token: String = random::<16>()?.iter().map(|b| format!("{b:02x}")).collect();
        let target = format!("mi_{}.txt", &token[..12]);
        temp_name(&target)?;
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.scratch/input-study");
        fs::create_dir_all(&root).map_err(|_| "private_parent_failed")?;
        let directory = root.join(format!(
            "probe-{}-{}-{}-{}-{}-{}",
            probe.label(),
            &token[..8],
            &token[8..12],
            &token[12..16],
            &token[16..20],
            &token[20..]
        ));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .map_err(|_| "private_directory_failed")?;
        Ok(Self {
            directory,
            target,
            sequence: 0,
            stage: "created",
            cleanup_confirmed: false,
            mutation_attempted: false,
        })
    }
    fn private(&self, name: &str, bytes: &[u8]) -> Result<()> {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(self.directory.join(name))
            .map_err(|_| "private_file_failed")?;
        f.write_all(bytes)
            .and_then(|_| f.sync_all())
            .map_err(|_| "private_file_failed")
    }
    fn stage(&mut self, stage: &'static str, detail: Value) -> Result<()> {
        self.stage = stage;
        self.sequence += 1;
        let value = json!({"stage":stage,"target":self.target,"mutation_attempted":self.mutation_attempted,
            "cleanup_confirmed":self.cleanup_confirmed,"exclusive_ownership":false,"detail":detail});
        self.private(
            &format!("stage-{:02}.json", self.sequence),
            value.to_string().as_bytes(),
        )
    }
    fn baseline(&self, files: &DeviceFiles) -> Result<()> {
        for file in std::iter::once(&files.keymap).chain(files.smart_actions.iter()) {
            self.private(file.meta.file.name(), &file.bytes)?;
            let saved = fs::read(self.directory.join(file.meta.file.name()))
                .map_err(|_| "baseline_reload_failed")?;
            if saved.as_slice() != file.bytes.as_ref() {
                return Err("baseline_reload_mismatch");
            }
        }
        Ok(())
    }
}
fn candidate(transport: &dyn HidTransport) -> Result<HidCandidate> {
    let candidates: Vec<_> = transport
        .enumerate()
        .map_err(|_| "enumerate_failed")?
        .into_iter()
        .filter(mi_transport::is_candidate)
        .collect();
    if candidates.len() != 1 {
        return Err("ambiguous_or_absent_candidate");
    }
    let selected = candidates.into_iter().next().unwrap();
    if selected.vid != 0x303a
        || selected.pid != 0x8298
        || selected.transport_hint != mi_transport::TransportHint::Usb
    {
        return Err("unsupported_target");
    }
    Ok(selected)
}
async fn listing(session: &DeviceSession) -> Result<Listing> {
    let response = session
        .request(Method::FsList, json!({"checksum":true}))
        .await
        .map_err(|_| "list_failed")?;
    if response.get("error").is_some() {
        return Err("list_rpc_error");
    }
    let entries = response["result"].as_array().ok_or("invalid_list")?;
    let mut listing = Listing::new();
    for entry in entries {
        let name = entry["name"].as_str().ok_or("invalid_list_name")?;
        if listing.insert(name.to_owned(), entry.clone()).is_some() {
            return Err("duplicate_list_name");
        }
    }
    Ok(listing)
}
fn same_files(a: &DeviceFiles, b: &DeviceFiles) -> bool {
    a.keymap.meta == b.keymap.meta
        && a.keymap.bytes == b.keymap.bytes
        && match (&a.smart_actions, &b.smart_actions) {
            (None, None) => true,
            (Some(a), Some(b)) => a.meta == b.meta && a.bytes == b.bytes,
            _ => false,
        }
}
fn exact_added(before: &Listing, after: &Listing, target: &str, probe: Probe) -> Result<()> {
    let mut rest = after.clone();
    let new = rest.remove(target).ok_or("temp_absent_after_write")?;
    let size = new["size"]
        .as_u64()
        .or_else(|| new["size"].as_str().and_then(|s| s.parse().ok()));
    if size != Some(probe.payload().len() as u64)
        || new["checksum"].as_str() != Some(probe.sha1())
        || &rest != before
    {
        return Err("list_changed_or_wrong_temp");
    }
    Ok(())
}
fn kind(response: &Value) -> &'static str {
    match &response["result"] {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}
async fn identity(session: &DeviceSession) -> Result<(Option<u32>, Option<u32>)> {
    let identity = session
        .read_identity_info()
        .await
        .map_err(|_| "identity_failed")?;
    if identity.version.as_deref() != Some("0.6.3") {
        return Err("unsupported_firmware");
    }
    Ok((identity.profile_index, identity.layer_index))
}
async fn snapshot(session: &DeviceSession) -> Result<DeviceFiles> {
    session
        .read_config_snapshot()
        .await
        .map_err(|_| "verified_snapshot_failed")?;
    let snapshot = session.snapshot().ok_or("snapshot_absent")?;
    if snapshot.stale {
        return Err("snapshot_stale");
    }
    Ok(snapshot.files)
}
async fn experiment(
    transport: &dyn HidTransport,
    selected: &HidCandidate,
    broker: &Broker,
    session: &DeviceSession,
    record: &mut Record,
    quiet: Duration,
) -> Result<()> {
    let probe = broker.probe;
    broker.quiet(quiet).await?;
    let active = identity(session).await?;
    let before = snapshot(session).await?;
    record.baseline(&before)?;
    let before_list = listing(session).await?;
    if before_list.contains_key(&record.target) {
        return Err("temp_already_exists");
    }
    record.stage("baseline_saved", json!({"keymap_size":before.keymap.meta.size,"keymap_sha1":before.keymap.meta.checksum_hex(),
        "smart_present":before.smart_actions.is_some(),"smart_size":before.smart_actions.as_ref().map(|v|v.meta.size),
        "smart_sha1":before.smart_actions.as_ref().map(|v|v.meta.checksum_hex()),"profile_index":active.0,"layer_index":active.1,
        "active_indices_unavailable":active.0.is_none() || active.1.is_none()}))?;
    broker.quiet(quiet).await?;
    if candidate(transport)? != *selected {
        return Err("candidate_changed");
    }
    if identity(session).await? != active {
        return Err("active_state_changed");
    }
    // The last pre-write RPC proves exact T absent and all entries stable.
    if listing(session).await? != before_list {
        return Err("prewrite_metadata_changed");
    }
    broker.uncontested()?;
    // The existing core allocates 0..998 sequentially. Bound this fixed experiment's
    // before/after reads below 900 so reserved diagnostic IDs can never be reused
    // later by the core, including after a diagnostic timeout/late response.
    if broker.used.lock().unwrap().len() > 400 {
        return Err("core_request_budget_exceeded");
    }
    record.mutation_attempted = true;
    let operation = async {
        match probe {
            Probe::A => {
                record.stage("create_attempt", json!({}))?;
                let reply = broker.exact(Exact::Create).await?;
                record.stage("create_ack", json!({"result_kind":kind(&reply)}))?;
            }
            Probe::B => {
                record.stage(
                    "chunk_1_attempt",
                    json!({"offset":0,"size":6,"completed":false}),
                )?;
                let reply = broker.exact(Exact::Chunk1).await?;
                record.stage("chunk_1_ack", json!({"result_kind":kind(&reply)}))?;
                // A successful envelope is required before a second mutation. A
                // quiet/identity gate also catches observed external changes.
                broker.quiet(quiet).await?;
                if candidate(transport)? != *selected || identity(session).await? != active {
                    return Err("identity_or_activity_changed");
                }
                let mut interim = listing(session).await?;
                interim.remove(&record.target);
                if interim != before_list {
                    return Err("original_entries_changed_between_chunks");
                }
                broker.uncontested()?;
                record.stage(
                    "chunk_2_attempt",
                    json!({"offset":6,"size":6,"completed":true}),
                )?;
                let reply = broker.exact(Exact::Chunk2).await?;
                record.stage("chunk_2_ack", json!({"result_kind":kind(&reply)}))?;
            }
        }
        exact_added(
            &before_list,
            &listing(session).await?,
            &record.target,
            probe,
        )?;
        let reply = broker.exact(Exact::Read).await?;
        use mi_session::DeviceSchema;
        let chunk = ReadOnlySchema
            .chunk(&reply)
            .map_err(|_| "invalid_temp_read")?;
        if chunk.total_size != probe.payload().len() as u64 || chunk.data != probe.payload() {
            return Err("temp_byte_mismatch");
        }
        record.stage(
            "temp_bytes_verified",
            json!({"size":probe.payload().len(),"sha1":probe.sha1()}),
        )?;
        // Immediately re-check every original entry and activity before the exact cleanup.
        if candidate(transport)? != *selected || identity(session).await? != active {
            return Err("identity_or_activity_changed");
        }
        exact_added(
            &before_list,
            &listing(session).await?,
            &record.target,
            probe,
        )?;
        broker.uncontested()?;
        record.stage("delete_attempt", json!({}))?;
        let reply = broker.exact(Exact::Delete).await?;
        record.stage("delete_ack", json!({"result_kind":kind(&reply)}))?;
        Ok(())
    }
    .await;
    // Always attempt only read-only postchecks after a mutation attempt. No retries,
    // no automatic deletion on uncertainty, and no configuration restoration.
    let post = async {
        let after_list = listing(session).await?;
        let absent = !after_list.contains_key(&record.target);
        record.cleanup_confirmed = absent;
        let mut originals = after_list.clone();
        originals.remove(&record.target);
        record.stage(
            "post_list",
            json!({"temp_absent":absent,"original_entries_unchanged": originals == before_list}),
        )?;
        let after_active = identity(session).await?;
        let after = snapshot(session).await?;
        let bytes_equal = same_files(&before, &after);
        let final_list = listing(session).await?;
        record.stage(
            "post_bytes",
            json!({"config_bytes_equal":bytes_equal,"activity_equal":after_active == active,
            "final_list_equal":final_list == before_list}),
        )?;
        if !bytes_equal
            || after_active != active
            || after_list != before_list
            || final_list != before_list
            || candidate(transport)? != *selected
        {
            return Err("postcheck_mismatch");
        }
        broker.uncontested()
    }
    .await;
    operation?;
    post
}
/// System boundary injection is test-only; authorization precedes all transport calls.
async fn run_exact(
    transport: &dyn HidTransport,
    opt_in: Option<&str>,
    quiet: Duration,
    probe: Probe,
) -> Result<Outcome> {
    if opt_in != Some(probe.opt_in()) {
        return Err("operator_opt_in_absent");
    }
    let selected = candidate(transport)?;
    let mut record = Record::new(probe)?;
    record.stage(
        "before_open",
        json!({"vid":"303A","pid":"8298","quiet_window_ms":quiet.as_millis()}),
    )?;
    let raw = transport.open(&selected.path).map_err(|_| "open_failed")?;
    let broker = Broker::start(raw, record.target.clone(), probe);
    let session = DeviceSession::new(
        broker.clone(),
        Box::<NativeFramer>::default(),
        Arc::new(ReadOnlySchema),
        SessionConfig {
            timeout: RPC_BUDGET,
            interval: Duration::from_millis(20),
        },
    );
    let result = tokio::time::timeout(
        Duration::from_secs(150),
        experiment(transport, &selected, &broker, &session, &mut record, quiet),
    )
    .await
    .map_err(|_| "overall_timeout_unknown")
    .and_then(|v| v);
    let reason = result.err().unwrap_or("passed");
    let report_result = record.stage("finished", json!({"passed":reason == "passed","reason":reason,"last_stage":record.stage,"foreign_count":broker.foreign.load(Ordering::SeqCst)}));
    session.shutdown().await;
    broker.finish().await;
    transport.stop_discovery();
    report_result?;
    Ok(Outcome {
        passed: reason == "passed",
        cleanup_confirmed: record.cleanup_confirmed,
        reason,
        directory: record.directory,
        target: record.target,
    })
}
pub async fn run(
    transport: &dyn HidTransport,
    opt_in: Option<&str>,
    quiet: Duration,
) -> Result<Outcome> {
    run_exact(transport, opt_in, quiet, Probe::A).await
}
pub async fn run_b(
    transport: &dyn HidTransport,
    opt_in: Option<&str>,
    quiet: Duration,
) -> Result<Outcome> {
    run_exact(transport, opt_in, quiet, Probe::B).await
}
pub async fn native_b() -> Result<()> {
    native_exact(Probe::B).await
}
pub async fn native() -> Result<()> {
    native_exact(Probe::A).await
}
async fn native_exact(probe: Probe) -> Result<()> {
    let value = std::env::var(match probe {
        Probe::A => "MI_PROBE_A_OPERATOR",
        Probe::B => "MI_PROBE_B_OPERATOR",
    })
    .ok();
    if value.as_deref() != Some(probe.opt_in()) {
        return Err("operator_opt_in_absent");
    } // Even constructing native discovery requires opt-in.
    let transport = mi_transport::MacTransport::new();
    let outcome = run_exact(&transport, value.as_deref(), Duration::from_secs(2), probe).await?;
    println!(
        "{} metadata: passed={} cleanup_confirmed={} reason={} target={} private_evidence={}",
        probe.label(),
        outcome.passed,
        outcome.cleanup_confirmed,
        outcome.reason,
        outcome.target,
        outcome.directory.display()
    );
    if outcome.passed {
        Ok(())
    } else {
        Err(outcome.reason)
    }
}

pub mod fixture;
