//! Private H0/H1 executor. Never linked into the application or FFI.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use mi_session::DeviceSchema;
use sha1::{Digest, Sha1};
use sha2::Sha256;

pub const H0: &str = "I_AUTHORIZE_H0_READONLY_BACKUP";
pub const H1: &str = "I_AUTHORIZE_H1_EXACT_TEMPFILE";
pub fn long() -> Vec<u8> {
    format!("{}中😀", "a".repeat(3071)).into_bytes()
}
pub fn short() -> Vec<u8> {
    b"0123456789abcdef".repeat(4)
}
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub struct Options {
    pub h1: bool,
    pub authorization: String,
    pub path: String,
    pub target: String,
    pub expected_keymap: Option<String>,
    pub unresolved_reviewed: bool,
}
impl Options {
    fn validate(&self) -> Result<()> {
        if self.authorization != if self.h1 { H1 } else { H0 } || self.path.is_empty() {
            return Err("explicit_scope_and_path_required");
        }
        temp_name(&self.target)?;
        if self.h1
            && (!self.unresolved_reviewed
                || !self
                    .expected_keymap
                    .as_ref()
                    .is_some_and(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit())))
        {
            return Err("review_and_expected_baseline_required");
        }
        Ok(())
    }
}

// A single synchronous request owner is the only reader of this dedicated handle.
// Typed operations cannot name configuration writes or lighting methods.
enum Request {
    Status,
    List,
    Config(mi_session::ConfigFile, usize, usize),
    TempRead(usize, usize),
    Chunk(usize),
    Delete,
}
struct Wire {
    link: Arc<dyn HidLink>,
    framer: NativeFramer,
    next: u16,
    target: String,
    chunk: usize,
    deleted: bool,
}
impl Drop for Wire {
    fn drop(&mut self) {
        self.link.close();
    }
}
impl Wire {
    async fn rpc(&mut self, request: Request) -> Result<Value> {
        if self.next >= 999 {
            return Err("request_budget_exhausted");
        }
        let (method, params) = match request {
            Request::Status => ("device.status", Value::Null),
            Request::List => ("fs.list", json!({"checksum":true})),
            Request::Config(file, offset, len) => (
                "fs.readbin",
                json!({"file":file.name(),"offset":offset,"len":len}),
            ),
            Request::TempRead(offset, len) => (
                "fs.readbin",
                json!({"file":self.target,"offset":offset,"len":len}),
            ),
            Request::Chunk(index) => {
                if index != self.chunk || index >= 5 || self.deleted {
                    return Err("invalid_write_sequence");
                }
                let (data, offset, completed) = match index {
                    0 | 3 => (long()[..3072].to_vec(), 0, false),
                    1 | 4 => (long()[3072..].to_vec(), 3072, true),
                    2 => (short(), 0, true),
                    _ => unreachable!(),
                };
                self.chunk += 1; // Never replay an attempted chunk.
                (
                    "fs.writebin",
                    json!({"file":self.target,"data":STANDARD.encode(data),"offset":offset,"append":true,"completed":completed}),
                )
            }
            Request::Delete => {
                if self.chunk != 5 || self.deleted {
                    return Err("invalid_delete_sequence");
                }
                self.deleted = true;
                ("fs.delete", json!({"file":self.target}))
            }
        };
        let id = self.next;
        self.next += 1;
        let line = json!({"id":id,"method":method,"params":params}).to_string();
        tokio::time::timeout(RPC_BUDGET, async {
            for report in NativeFramer::default()
                .encode(line.as_bytes())
                .map_err(|_| "encode_failed")?
            {
                self.link.write(&report).await.map_err(|_| "send_unknown")?;
            }
            loop {
                let report = self.link.read().await.map_err(|_| "read_unknown")?;
                if report.get(1) != Some(&2) {
                    return Err("foreign_channel");
                }
                let lines = self.framer.feed(&report).map_err(|_| "framing_unknown")?;
                if lines.len() > 1 {
                    return Err("foreign_response");
                }
                if let Some(line) = lines.first() {
                    if mi_proto::response_id(line).map_err(|_| "invalid_id")? != Some(u64::from(id))
                    {
                        return Err("foreign_response");
                    }
                    return diagnostic_response(line, method);
                }
            }
        })
        .await
        .map_err(|_| "rpc_timeout_unknown")?
    }
    async fn quiet(&self, duration: Duration) -> Result<()> {
        match tokio::time::timeout(duration, self.link.read()).await {
            Err(_) => Ok(()),
            Ok(_) => Err("unsolicited_traffic_or_disconnect"),
        }
    }
    async fn status(&mut self) -> Result<Value> {
        let reply = self.rpc(Request::Status).await?;
        let value = &reply["result"];
        if value["version"] != "0.6.3"
            || value["profile_index"].as_u64().is_none()
            || value["layer_index"].as_u64().is_none()
        {
            return Err("unsupported_or_incomplete_identity");
        }
        Ok(
            json!({"version":value["version"],"profile_index":value["profile_index"],"layer_index":value["layer_index"]}),
        )
    }
    async fn list(&mut self) -> Result<Listing> {
        let reply = self.rpc(Request::List).await?;
        let mut entries = Listing::new();
        for item in reply["result"].as_array().ok_or("invalid_list")? {
            let name = item["name"].as_str().ok_or("invalid_list_name")?;
            if entries.insert(name.into(), item.clone()).is_some() {
                return Err("duplicate_list_name");
            }
        }
        Ok(entries)
    }
    async fn configs(&mut self, list: &Listing) -> Result<BTreeMap<String, Vec<u8>>> {
        let metadata = ReadOnlySchema
            .files(&json!({"result":list.values().collect::<Vec<_>>()}))
            .map_err(|_| "invalid_metadata")?;
        if !metadata
            .iter()
            .any(|v| v.file == mi_session::ConfigFile::Keymap)
        {
            return Err("keymap_missing");
        }
        let chunks: u64 = metadata
            .iter()
            .fold(0u64, |n, v| n.saturating_add(v.size.div_ceil(3072)));
        // H0 twice, H1 before/after every mutation; reserve all reads in advance.
        // 14 snapshots + status/list gates + temporary readbacks stay below 999.
        if metadata.iter().any(|v| v.size > 4 * 1024 * 1024)
            || chunks.saturating_mul(14).saturating_add(100) > 999
        {
            return Err("complete_verification_exceeds_budget");
        }
        let mut out = BTreeMap::new();
        for meta in metadata {
            let mut data = Vec::new();
            while data.len() < meta.size as usize {
                let len = (meta.size as usize - data.len()).min(3072);
                let reply = self
                    .rpc(Request::Config(meta.file, data.len(), len))
                    .await?;
                let chunk = ReadOnlySchema.chunk(&reply).map_err(|_| "invalid_chunk")?;
                if chunk.total_size != meta.size || chunk.data.len() != len {
                    return Err("config_read_changed");
                }
                data.extend(chunk.data);
            }
            if format!("{:x}", Sha1::digest(&data)) != meta.checksum_hex() {
                return Err("config_checksum_mismatch");
            }
            out.insert(meta.file.name().into(), data);
        }
        Ok(out)
    }
    async fn temp(&mut self, expected: &[u8]) -> Result<()> {
        let mut offset = 0;
        while offset < expected.len() {
            let len = (expected.len() - offset).min(3072);
            let reply = self.rpc(Request::TempRead(offset, len)).await?;
            let chunk = ReadOnlySchema
                .chunk(&reply)
                .map_err(|_| "invalid_temp_chunk")?;
            if chunk.total_size != expected.len() as u64
                || chunk.data != expected[offset..offset + len]
            {
                return Err("temp_readback_mismatch");
            }
            offset += len;
        }
        Ok(())
    }
}

fn sync_record(record: &Record) -> Result<()> {
    fs::File::open(&record.directory)
        .and_then(|f| f.sync_all())
        .map_err(|_| "directory_sync_failed")
}
fn journal(record: &mut Record, stage: &'static str, mut detail: Value) -> Result<()> {
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "invalid_clock")?;
    detail
        .as_object_mut()
        .ok_or("invalid_record")?
        .insert("unix_ms".into(), json!(time.as_millis()));
    record.stage(stage, detail)?;
    sync_record(record)
}
fn matching_list(
    base: &Listing,
    actual: &Listing,
    target: &str,
    expected: Option<&[u8]>,
) -> Result<()> {
    let mut rest = actual.clone();
    let item = rest.remove(target);
    if &rest != base {
        return Err("non_target_directory_changed");
    }
    match (item, expected) {
        (None, None) => Ok(()),
        (Some(item), Some(bytes))
            if item["size"]
                .as_u64()
                .or_else(|| item["size"].as_str().and_then(|s| s.parse().ok()))
                == Some(bytes.len() as u64)
                && item["checksum"] == format!("{:x}", Sha1::digest(bytes)) =>
        {
            Ok(())
        }
        _ => Err("unexpected_target_state"),
    }
}
async fn unchanged(
    wire: &mut Wire,
    transport: &dyn HidTransport,
    selected: &HidCandidate,
    status: &Value,
    list: &Listing,
    files: &BTreeMap<String, Vec<u8>>,
    expected: Option<&[u8]>,
) -> Result<()> {
    if candidate(transport)? != *selected || wire.status().await? != *status {
        return Err("identity_or_activity_changed");
    }
    let current = wire.list().await?;
    matching_list(list, &current, &wire.target, expected)?;
    if wire.configs(&current).await? != *files {
        return Err("config_bytes_changed");
    }
    matching_list(list, &wire.list().await?, &wire.target, expected)?;
    if wire.status().await? != *status {
        return Err("activity_changed_during_read");
    }
    if let Some(bytes) = expected {
        wire.temp(bytes).await?;
    }
    Ok(())
}

pub async fn run(
    transport: &dyn HidTransport,
    options: &Options,
    quiet: Duration,
) -> Result<Outcome> {
    options.validate()?; // Before enumeration and open.
    let selected = candidate(transport)?;
    if selected.path != options.path {
        return Err("selected_path_mismatch");
    }
    let mut record = Record::named("h")?;
    record.target = options.target.clone();
    journal(
        &mut record,
        "h_before_open",
        json!({"scope":if options.h1 {"H1"}else{"H0"},"candidate":selected,"stable_hardware_id":"NOT_AVAILABLE","same_connection_only":true}),
    )?;
    let raw = transport.open(&selected.path).map_err(|_| "open_failed")?;
    let mut wire = Wire {
        link: raw,
        framer: NativeFramer::default(),
        next: 0,
        target: options.target.clone(),
        chunk: 0,
        deleted: false,
    };
    let result=tokio::time::timeout(Duration::from_secs(240), async {
        wire.quiet(quiet).await?;
        let status=wire.status().await?;
        let list=wire.list().await?;
        if list.contains_key(&options.target) { return Err("target_already_exists"); }
        let files=wire.configs(&list).await?;
        unchanged(&mut wire,transport,&selected,&status,&list,&files,None).await?;
        if options.h1 && options.expected_keymap.as_deref()!=Some(hash(&files["keymap.json"]).as_str()) { return Err("expected_keymap_mismatch"); }
        for (name,bytes) in &files { record.private(name,bytes)?; if fs::read(record.directory.join(name)).map_err(|_|"backup_read_failed")? != *bytes { return Err("backup_mismatch"); } }
        let meta: BTreeMap<_,_>=files.iter().map(|(name,bytes)|(name,json!({"size":bytes.len(),"sha1":format!("{:x}",Sha1::digest(bytes)),"sha256":hash(bytes)}))).collect();
        journal(&mut record,"h_backup_verified",json!({"files":meta,"smart_actions_present":files.contains_key("smart_actions.json"),"directory":list,"status":status,"identity_scope":"same_open_handle_and_expected_configuration","physical_keys":"NOT_RUN","lighting_visual":"NOT_RUN"}))?;
        if !options.h1 { return Ok(()); }
        h1_steps(&mut wire, transport, &selected, &status, &list, &files, &mut record, quiet, 0).await?;
        Ok(())
    }).await.map_err(|_|"overall_timeout_unknown").and_then(|r|r);
    wire.link.close(); // No post-error requests, retry, reconnect, restore or cleanup.
    transport.stop_discovery();
    let reason = result.err().unwrap_or("passed");
    let attempted = record.mutation_attempted;
    journal(
        &mut record,
        "h_finished",
        json!({"passed":reason=="passed","reason":reason,"state":if reason=="passed" {"VERIFIED_SCOPE_ONLY"}else if attempted {"UNKNOWN"}else{"NOT_SENT"},"requests":wire.next,"physical_keys":"NOT_RUN","lighting_visual":"NOT_RUN"}),
    )?;
    Ok(Outcome {
        passed: reason == "passed",
        cleanup_confirmed: record.cleanup_confirmed,
        reason,
        directory: record.directory,
        target: record.target,
    })
}

pub async fn native(h1: bool) -> Result<()> {
    let options = Options {
        h1,
        authorization: std::env::var("MI_HARDWARE_OPERATOR").unwrap_or_default(),
        path: std::env::var("MI_HARDWARE_PATH").unwrap_or_default(),
        target: std::env::var("MI_HARDWARE_TARGET").unwrap_or_default(),
        expected_keymap: std::env::var("MI_HARDWARE_KEYMAP_SHA256").ok(),
        unresolved_reviewed: std::env::var("MI_HARDWARE_REVIEWED").as_deref() == Ok("yes"),
    };
    options.validate()?;
    let transport = mi_transport::MacTransport::new();
    let outcome = run(&transport, &options, Duration::from_millis(200)).await?;
    println!(
        "H{} passed={} reason={} private_evidence={}",
        if h1 { 1 } else { 0 },
        outcome.passed,
        outcome.reason,
        outcome.directory.display()
    );
    if outcome.passed {
        Ok(())
    } else {
        Err(outcome.reason)
    }
}

async fn original_files_unchanged(
    wire: &mut Wire,
    transport: &dyn HidTransport,
    selected: &HidCandidate,
    status: &Value,
    list: &Listing,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<()> {
    if candidate(transport)? != *selected || wire.status().await? != *status {
        return Err("identity_changed");
    }
    let current = wire.list().await?;
    let mut rest = current.clone();
    rest.remove(&wire.target);
    if rest != *list || wire.configs(&current).await? != *files {
        return Err("original_files_changed");
    }
    let mut rest = wire.list().await?;
    rest.remove(&wire.target);
    if rest != *list || wire.status().await? != *status {
        return Err("originals_changed_during_read");
    }
    Ok(())
}
async fn h1_steps(
    wire: &mut Wire,
    transport: &dyn HidTransport,
    selected: &HidCandidate,
    status: &Value,
    list: &Listing,
    files: &BTreeMap<String, Vec<u8>>,
    record: &mut Record,
    quiet: Duration,
    start: usize,
) -> Result<()> {
    let mut expected: Option<Vec<u8>> = None;
    for index in start..5 {
        if index == 1 || index == 4 {
            original_files_unchanged(wire, transport, selected, status, list, files).await?;
        } else {
            unchanged(
                wire,
                transport,
                selected,
                status,
                list,
                files,
                expected.as_deref(),
            )
            .await?;
        }
        wire.quiet(quiet).await?;
        record.mutation_attempted = true;
        journal(record, "h_chunk_attempt", json!({"index":index}))?;
        let reply = wire.rpc(Request::Chunk(index)).await?;
        journal(
            record,
            "h_chunk_ack",
            json!({"index":index,"result":reply["result"],"durability":"NOT_YET_VERIFIED"}),
        )?;
        if index == 0 || index == 3 {
            original_files_unchanged(wire, transport, selected, status, list, files).await?;
        } else {
            expected = Some(if index == 2 { short() } else { long() });
            unchanged(
                wire,
                transport,
                selected,
                status,
                list,
                files,
                expected.as_deref(),
            )
            .await?;
            journal(record, "h_transfer_verified", json!({"index":index}))?;
        }
    }
    unchanged(
        wire,
        transport,
        selected,
        status,
        list,
        files,
        expected.as_deref(),
    )
    .await?;
    wire.quiet(quiet).await?;
    journal(record, "h_delete_attempt", json!({}))?;
    wire.rpc(Request::Delete).await?;
    unchanged(wire, transport, selected, status, list, files, None).await?;
    record.cleanup_confirmed = true;
    journal(record, "h_delete_verified", json!({}))
}
