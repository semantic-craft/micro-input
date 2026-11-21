use super::device_sync::parse_iso8601_utc_ms;
use super::*;

impl WorkbenchStore {
    pub(super) fn effective_resolved_active_profile_id(&self) -> Option<String> {
        let profiles = &self
            .document_store
            .host_store()
            .draft
            .document
            .editable
            .graph
            .profiles;
        if let Some(locked) = self
            .document_store
            .locked_profile_id()
            .filter(|pid| profiles.contains_key(pid))
        {
            return Some(locked);
        }
        if let Some(profile) = self
            .six_layer_running
            .as_ref()
            .and_then(|running| running.effective_profile_id())
            .or_else(|| {
                self.six_layer_view
                    .as_ref()
                    .and_then(|running| running.effective_profile_id.clone())
            })
        {
            return Some(profile);
        }
        self.document_store
            .host_active_profile_id()
            .filter(|pid| profiles.contains_key(pid))
    }

    pub fn slot_table_for_source(&self, source: LiveSessionSource) -> &FourAppSlotTable {
        match source {
            LiveSessionSource::Antigravity => &self.slot_table,
            LiveSessionSource::Codex => &self.codex_slot_table,
        }
    }

    pub fn slot_table_for_source_mut(
        &mut self,
        source: LiveSessionSource,
    ) -> &mut FourAppSlotTable {
        match source {
            LiveSessionSource::Antigravity => &mut self.slot_table,
            LiveSessionSource::Codex => &mut self.codex_slot_table,
        }
    }

    pub fn active_slot_table(&self) -> &FourAppSlotTable {
        self.slot_table_for_source(self.desired_live_session_source())
    }

    pub fn active_slot_table_mut(&mut self) -> &mut FourAppSlotTable {
        let source = self.desired_live_session_source();
        self.slot_table_for_source_mut(source)
    }

    pub fn source_for_profile_id(&self, profile_id: &str) -> LiveSessionSource {
        if profile_id == "codex" {
            return LiveSessionSource::Codex;
        }
        if profile_id == "workbench" {
            return self.desired_live_session_source();
        }
        let host_doc = &self.document_store.host_store().draft.document;
        let draft_doc = &self.document_store.draft.document;
        if let Some(resolved) = self.runtime_action_profile_id(profile_id) {
            if host_doc
                .editable
                .graph
                .profiles
                .get(resolved)
                .is_some_and(|p| {
                    p.name == mi_model::preset::LOCAL_CODEX_PROFILE_NAME
                        || CanonicalAgent::from_profile_name(&p.name) == Some(CanonicalAgent::Codex)
                })
            {
                return LiveSessionSource::Codex;
            }
        }
        if mi_model::board_layers::native_owner(host_doc, profile_id)
            == Some(NativeLayerOwner::Codex)
            || mi_model::board_layers::native_owner(draft_doc, profile_id)
                == Some(NativeLayerOwner::Codex)
            || draft_doc
                .editable
                .graph
                .profiles
                .get(profile_id)
                .is_some_and(|p| {
                    p.name == mi_model::preset::LOCAL_CODEX_PROFILE_NAME
                        || CanonicalAgent::from_profile_name(&p.name) == Some(CanonicalAgent::Codex)
                })
        {
            LiveSessionSource::Codex
        } else {
            LiveSessionSource::Antigravity
        }
    }

    pub(crate) fn desired_live_session_source(&self) -> LiveSessionSource {
        if let Some(active_id) = self
            .effective_resolved_active_profile_id()
            .or_else(|| self.selected_profile_id.clone())
        {
            return self.source_for_profile_id(&active_id);
        }
        if self.document_store.locked_profile_id().is_none()
            && !self.six_layer_user_pinned
            && self
                .six_layer_running
                .as_ref()
                .is_some_and(|r| r.observed_layer.load(Ordering::Acquire) == 1)
        {
            return LiveSessionSource::Codex;
        }
        LiveSessionSource::Antigravity
    }

    pub(crate) fn sync_live_session_source(&mut self) -> bool {
        let desired = self.desired_live_session_source();
        let changed = {
            let (lock, cvar) = &*self.live_session_source;
            let mut guard = lock.lock().unwrap();
            if guard.source == desired {
                false
            } else {
                guard.source = desired;
                guard.generation = guard.generation.wrapping_add(1);
                cvar.notify_all();
                true
            }
        };
        if !changed {
            return false;
        }
        self.sync_active_navigation_catalog_mirror();
        self.refresh_navigation_revision();
        self.refresh_four_app_lighting();
        self.commit_runtime_update();
        true
    }

    pub fn start_antigravity_live_poller(&self) {
        if self.config.isolated {
            return;
        }
        let tx = self.hardware_event_tx.clone();
        let epoch_source = Arc::clone(&self.lifecycle_epoch);
        let live_source = Arc::clone(&self.live_session_source);
        std::thread::spawn(move || {
            let mut seen_running_ids = BTreeSet::new();
            let mut completed_transition_watermarks = BTreeMap::new();
            loop {
                let source_gen = {
                    let (lock, _) = &*live_source;
                    lock.lock().unwrap().generation
                };
                let epoch = epoch_source.load(Ordering::Acquire);
                let ag_catalog = poll_antigravity_daemon_candidates(
                    &mut seen_running_ids,
                    &mut completed_transition_watermarks,
                );
                if tx
                    .send(HardwareLoopEvent::LiveCandidatesUpdated {
                        epoch,
                        source: LiveSessionSource::Antigravity,
                        catalog: ag_catalog,
                    })
                    .is_err()
                {
                    break;
                }
                let codex_catalog = poll_codex_session_candidates();
                if tx
                    .send(HardwareLoopEvent::LiveCandidatesUpdated {
                        epoch,
                        source: LiveSessionSource::Codex,
                        catalog: codex_catalog,
                    })
                    .is_err()
                {
                    break;
                }
                let (lock, cvar) = &*live_source;
                let guard = lock.lock().unwrap();
                if guard.generation == source_gen {
                    let _ = cvar.wait_timeout(guard, Duration::from_millis(1500));
                }
            }
        });
    }
}

#[cfg(test)]
pub(super) const ANTIGRAVITY_HUB_MAX_BYTES: usize = 65_536;
#[cfg(not(test))]
const ANTIGRAVITY_HUB_MAX_BYTES: usize = 65_536;
const MAX_ANTIGRAVITY_HTTP_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

#[cfg(unix)]
fn current_uid() -> u32 {
    extern "C" {
        fn getuid() -> u32;
    }
    unsafe { getuid() }
}

#[cfg(unix)]
fn antigravity_hub_open_flags() -> i32 {
    #[cfg(target_os = "macos")]
    {
        0x0100 | 0x0004
    }
    #[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "x86")))]
    {
        0o400000 | 0o4000
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        0o100000 | 0o4000
    }
}

pub(super) fn ensure_private_document_dir(dir: &Path) -> Result<(), WorkbenchError> {
    mi_macos::ensure_private_dir(dir).map_err(WorkbenchError::Storage)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CsrfToken(String);

impl CsrfToken {
    pub(super) fn parse(raw: &str) -> Option<Self> {
        if raw.is_empty()
            || raw.trim() != raw
            || raw
                .chars()
                .any(|ch| ch.is_ascii_control() || matches!(ch, '\r' | '\n' | '\0'))
        {
            return None;
        }
        Some(Self(raw.to_owned()))
    }

    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

pub(super) struct ParsedAntigravityHub {
    pub(super) port: u16,
    pub(super) csrf: CsrfToken,
    pub(super) ui_port: u16,
    hub: String,
}

fn read_antigravity_hub_file(path: &Path) -> Option<ParsedAntigravityHub> {
    use std::hash::{Hash, Hasher};
    use std::io::Read;

    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(antigravity_hub_open_flags());
    }
    let file = options.open(path).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.file_type().is_file() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.uid() != current_uid() || (meta.mode() & 0o077) != 0 {
            return None;
        }
    }
    if meta.len() == 0 || meta.len() > ANTIGRAVITY_HUB_MAX_BYTES as u64 {
        return None;
    }
    let mut raw_bytes = Vec::with_capacity(meta.len() as usize);
    file.take((ANTIGRAVITY_HUB_MAX_BYTES + 1) as u64)
        .read_to_end(&mut raw_bytes)
        .ok()?;
    if raw_bytes.is_empty() || raw_bytes.len() > ANTIGRAVITY_HUB_MAX_BYTES {
        return None;
    }
    let raw = String::from_utf8(raw_bytes).ok()?;
    let val = serde_json::from_str::<Value>(&raw).ok()?;
    let port_u64 = val.get("httpPort").and_then(Value::as_u64)?;
    let port = u16::try_from(port_u64).ok().filter(|&p| p != 0)?;
    let csrf = val
        .get("csrfToken")
        .and_then(Value::as_str)
        .and_then(CsrfToken::parse)?;
    let ui_port = u16::try_from(val.get("httpsPort")?.as_u64()?).ok()?;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hash);
    raw.hash(&mut hash);
    Some(ParsedAntigravityHub {
        port,
        csrf,
        ui_port,
        hub: format!("{:x}", hash.finish()),
    })
}

fn find_freshest_antigravity_hub() -> Option<(u16, CsrfToken, AntigravityInstance)> {
    let home = env::var_os("HOME").map(PathBuf::from)?;
    let daemon_dir = home.join(".gemini/antigravity/daemon");
    let parsed = find_freshest_antigravity_hub_in(&daemon_dir)?;
    let pid = mi_macos::session_navigation::antigravity_process_id()?;
    Some((
        parsed.port,
        parsed.csrf,
        AntigravityInstance {
            hub: parsed.hub,
            pid,
            ui_port: parsed.ui_port,
        },
    ))
}

pub(super) fn find_freshest_antigravity_hub_in(daemon_dir: &Path) -> Option<ParsedAntigravityHub> {
    let verified_dir = mi_macos::verify_existing_private_dir(daemon_dir).ok()?;
    let entries = fs::read_dir(&verified_dir).ok()?;
    let mut hubs: Vec<(std::time::SystemTime, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            let name = path.file_name()?.to_str()?;
            if !name.starts_with("hub_") || !name.ends_with(".json") {
                return None;
            }
            let entry_meta = fs::symlink_metadata(&path).ok()?;
            if entry_meta.file_type().is_symlink() || !entry_meta.file_type().is_file() {
                return None;
            }
            let modified = entry_meta.modified().ok()?;
            Some((modified, path))
        })
        .collect();
    hubs.sort_by_key(|a| std::cmp::Reverse(a.0));
    for (_, path) in hubs {
        if let Some(parsed) = read_antigravity_hub_file(&path) {
            return Some(parsed);
        }
    }
    None
}

pub(super) fn build_antigravity_trajectories_request(
    port: u16,
    csrf: &CsrfToken,
    body_len: usize,
) -> String {
    format!(
        "POST /exa.language_server_pb.LanguageServerService/GetAllCascadeTrajectories HTTP/1.0\r\n\
         Host: 127.0.0.1:{port}\r\n\
         Content-Type: application/json\r\n\
         Connect-Protocol-Version: 1\r\n\
         x-codeium-csrf-token: {}\r\n\
         Connection: close\r\n\
         Content-Length: {body_len}\r\n\r\n",
        csrf.as_str()
    )
}

pub(super) fn parse_antigravity_http_200_body(raw_resp: &[u8]) -> Option<&[u8]> {
    let sep = b"\r\n\r\n";
    let header_end = raw_resp.windows(sep.len()).position(|w| w == sep)?;
    let headers = &raw_resp[..header_end];
    let status_line = headers
        .windows(2)
        .position(|w| w == b"\r\n")
        .map(|pos| &headers[..pos])
        .unwrap_or(headers);
    let is_ok = status_line == b"HTTP/1.1 200"
        || status_line.starts_with(b"HTTP/1.1 200 ")
        || status_line == b"HTTP/1.0 200"
        || status_line.starts_with(b"HTTP/1.0 200 ");
    if !is_ok {
        return None;
    }
    Some(&raw_resp[header_end + sep.len()..])
}

pub(crate) type ClassifiedAntigravityTrajectories = (
    Vec<FourAppLiveCandidate>,
    BTreeMap<String, String>,
    Option<(NavigationPlace, String)>,
);

pub(crate) fn classify_antigravity_trajectory_summaries(
    summaries: &serde_json::Map<String, Value>,
    active_frontmost_cid: Option<&str>,
    seen_running_ids: &mut BTreeSet<String>,
    completed_transition_watermarks: &mut BTreeMap<String, String>,
) -> ClassifiedAntigravityTrajectories {
    let mut parent_map: BTreeMap<&str, &str> = BTreeMap::new();
    let mut waiting_sources: Vec<&str> = Vec::new();
    for (cid, item) in summaries {
        let Some(obj) = item.as_object() else {
            continue;
        };
        let ann = obj.get("annotations").and_then(Value::as_object);
        if ann.and_then(|a| a.get("archived")).and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let killed = obj.get("killed").and_then(Value::as_bool).unwrap_or(false);
        if killed {
            continue;
        }
        if let Some(parent_id) = obj
            .get("trajectoryMetadata")
            .and_then(Value::as_object)
            .and_then(|m| m.get("parentConversationId"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            parent_map.insert(cid.as_str(), parent_id);
        }
        let waiting_steps = obj
            .get("waitingSteps")
            .and_then(Value::as_array)
            .is_some_and(|a| !a.is_empty());
        let status_str = obj
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("CASCADE_RUN_STATUS_IDLE");
        if waiting_steps
            || matches!(
                status_str,
                "CASCADE_RUN_STATUS_WAITING_FOR_USER_INPUT"
                    | "CASCADE_RUN_STATUS_WAITING_FOR_PERMISSION"
            )
        {
            waiting_sources.push(cid.as_str());
        }
    }

    let mut descendant_waiting_cids: BTreeSet<String> = BTreeSet::new();
    for start_cid in waiting_sources {
        let mut visited = BTreeSet::new();
        visited.insert(start_cid);
        let mut current = start_cid;
        for _ in 0..8 {
            let Some(&parent) = parent_map.get(current) else {
                break;
            };
            if !visited.insert(parent) {
                break;
            }
            descendant_waiting_cids.insert(parent.to_string());
            current = parent;
        }
    }

    let mut candidates = Vec::new();
    let mut project_ids = BTreeMap::new();
    let mut frontmost_acknowledged = None;
    for (cid, item) in summaries {
        let Some(obj) = item.as_object() else {
            continue;
        };
        let ann = obj.get("annotations").and_then(Value::as_object);
        let meta = obj.get("trajectoryMetadata").and_then(Value::as_object);
        if ann.and_then(|a| a.get("archived")).and_then(Value::as_bool) == Some(true)
            || ann
                .and_then(|a| a.get("antigravity.isSubagent"))
                .is_some_and(|v| {
                    v.as_bool() == Some(true)
                        || v.as_str().is_some_and(|s| s.eq_ignore_ascii_case("true"))
                })
        {
            seen_running_ids.remove(cid);
            completed_transition_watermarks.remove(cid);
            continue;
        }
        if let Some(m) = meta {
            if m.get("parentConversationId")
                .and_then(Value::as_str)
                .is_some_and(|s| !s.trim().is_empty())
                || m.get("subagentSpec").is_some_and(|v| !v.is_null())
                || m.get("isBattleModeFork").and_then(Value::as_bool) == Some(true)
                || m.get("nestingDepth")
                    .and_then(Value::as_i64)
                    .is_some_and(|d| d > 0)
                || m.get("sourceMetadata")
                    .and_then(Value::as_object)
                    .and_then(|sm| sm.get("hideFromConversationList"))
                    .and_then(Value::as_bool)
                    == Some(true)
            {
                seen_running_ids.remove(cid);
                completed_transition_watermarks.remove(cid);
                continue;
            }
        }
        let trajectory_type = obj
            .get("trajectoryType")
            .and_then(Value::as_str)
            .or_else(|| {
                meta.and_then(|m| m.get("trajectoryType"))
                    .and_then(Value::as_str)
            })
            .unwrap_or_default();
        if trajectory_type.ends_with("SIDE_QUESTION")
            || trajectory_type.ends_with("COMMIT_MESSAGE_GENERATION")
        {
            seen_running_ids.remove(cid);
            completed_transition_watermarks.remove(cid);
            continue;
        }
        let summary_str = obj
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        let title_str = ann
            .and_then(|a| a.get("title"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        let has_user_input = obj
            .get("lastUserInputTime")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty());
        if summary_str.is_empty() && title_str.is_empty() && !has_user_input {
            continue;
        }
        let Some(workspace_path) = session_navigation::workspace_from_summary(item) else {
            continue;
        };
        if let Some(pid_str) = meta
            .and_then(|m| m.get("projectId"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            project_ids.insert(cid.clone(), pid_str.to_string());
        }
        let title = if !summary_str.is_empty() {
            summary_str.to_string()
        } else if !title_str.is_empty() {
            title_str.to_string()
        } else {
            cid.chars().take(8).collect()
        };

        let status_str = obj
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("CASCADE_RUN_STATUS_IDLE");
        let last_mod_str = obj
            .get("lastModifiedTime")
            .and_then(Value::as_str)
            .or_else(|| obj.get("createdTime").and_then(Value::as_str))
            .unwrap_or_default();
        let view_str = ann
            .and_then(|a| a.get("lastUserViewTime"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let term_str = obj
            .get("latestExecutorTerminationReason")
            .and_then(Value::as_str)
            .or_else(|| obj.get("terminationReason").and_then(Value::as_str))
            .unwrap_or_default();
        let killed = obj.get("killed").and_then(Value::as_bool).unwrap_or(false);
        let waiting_steps = obj
            .get("waitingSteps")
            .and_then(Value::as_array)
            .is_some_and(|a| !a.is_empty());
        let has_running_subagents = obj
            .get("hasRunningSubagents")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let not_fully_idle = obj
            .get("notFullyIdle")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let errored = obj.get("errored").and_then(Value::as_bool).unwrap_or(false);
        let marked_unread = ann
            .and_then(|a| a.get("markedAsUnread"))
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let last_modified_ms = parse_iso8601_utc_ms(last_mod_str);
        let raw_view_ms = parse_iso8601_utc_ms(view_str);
        let descendant_waiting = !killed && descendant_waiting_cids.contains(cid.as_str());
        let has_waiting = waiting_steps || descendant_waiting;
        let mut watermark = if marked_unread {
            format!("{status_str}:{last_mod_str}:{term_str}:unread")
        } else {
            format!("{status_str}:{last_mod_str}:{term_str}")
        };
        if !waiting_steps && descendant_waiting {
            watermark.push_str(":subwait");
        }

        let (phase, effective_view_ms) = if !killed
            && (has_waiting
                || matches!(
                    status_str,
                    "CASCADE_RUN_STATUS_WAITING_FOR_USER_INPUT"
                        | "CASCADE_RUN_STATUS_WAITING_FOR_PERMISSION"
                )) {
            seen_running_ids.insert(cid.clone());
            (FourAppSessionKind::NeedsAttention, raw_view_ms)
        } else if !killed
            && (matches!(
                status_str,
                "CASCADE_RUN_STATUS_RUNNING"
                    | "CASCADE_RUN_STATUS_BUSY"
                    | "CASCADE_RUN_STATUS_CANCELING"
                    | "CASCADE_RUN_STATUS_WAITING_FOR_MODEL"
            ) || has_running_subagents
                || not_fully_idle)
        {
            seen_running_ids.insert(cid.clone());
            (FourAppSessionKind::Working, raw_view_ms)
        } else {
            let is_errored = errored
                || matches!(
                    status_str,
                    "CASCADE_RUN_STATUS_ERROR" | "CASCADE_RUN_STATUS_FAILED"
                )
                || term_str == "ERROR"
                || term_str.ends_with("_ERROR");
            if seen_running_ids.remove(cid) {
                completed_transition_watermarks.insert(cid.clone(), watermark.clone());
            }
            let just_transitioned = completed_transition_watermarks.get(cid) == Some(&watermark);
            let has_unviewed = marked_unread || last_modified_ms > raw_view_ms.saturating_add(500);
            if active_frontmost_cid == Some(cid.as_str()) && !marked_unread {
                completed_transition_watermarks.remove(cid);
                if !watermark.is_empty() {
                    frontmost_acknowledged = Some((
                        NavigationPlace::antigravity(cid.clone(), workspace_path.clone()),
                        watermark.clone(),
                    ));
                }
                (FourAppSessionKind::Idle, last_modified_ms.max(raw_view_ms))
            } else if has_unviewed || just_transitioned {
                if is_errored {
                    (FourAppSessionKind::Error, raw_view_ms)
                } else {
                    let adj_view_ms = if last_modified_ms > raw_view_ms.saturating_add(500) {
                        raw_view_ms
                    } else {
                        last_modified_ms.saturating_sub(1000)
                    };
                    (FourAppSessionKind::BlueDotCompleted, adj_view_ms)
                }
            } else {
                (FourAppSessionKind::Idle, raw_view_ms)
            }
        };

        candidates.push(FourAppLiveCandidate {
            place: NavigationPlace::antigravity(cid.clone(), workspace_path),
            title,
            phase,
            watermark,
            last_modified_ms,
            last_user_view_ms: effective_view_ms,
            updated_at_ms: last_modified_ms.max(effective_view_ms),
        });
    }

    candidates.sort_by(|a, b| {
        a.phase
            .cmp(&b.phase)
            .then_with(|| b.last_modified_ms.cmp(&a.last_modified_ms))
            .then_with(|| a.place.target_id().cmp(&b.place.target_id()))
    });
    (candidates, project_ids, frontmost_acknowledged)
}

pub(super) fn poll_antigravity_daemon_candidates(
    seen_running_ids: &mut BTreeSet<String>,
    completed_transition_watermarks: &mut BTreeMap<String, String>,
) -> Option<NavigationCatalog> {
    poll_antigravity_daemon_candidates_with_frontmost(
        seen_running_ids,
        completed_transition_watermarks,
        true,
    )
}

pub(crate) fn poll_antigravity_daemon_candidates_with_frontmost(
    seen_running_ids: &mut BTreeSet<String>,
    completed_transition_watermarks: &mut BTreeMap<String, String>,
    suppress_frontmost: bool,
) -> Option<NavigationCatalog> {
    use std::io::{Read, Write};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};

    let (port, csrf, instance) = find_freshest_antigravity_hub()?;
    let body = b"{\"metadata\":{}}";
    let request = build_antigravity_trajectories_request(port, &csrf, body.len());
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(2000)).ok()?;
    let _ = stream.set_write_timeout(Some(Duration::from_millis(2000)));
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    stream.write_all(request.as_bytes()).ok()?;
    stream.write_all(body).ok()?;
    let mut raw_resp = Vec::with_capacity(65536);
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if Instant::now() >= deadline {
            return None;
        }
        let mut chunk = [0u8; 16384];
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => {
                if raw_resp.len().saturating_add(count) > MAX_ANTIGRAVITY_HTTP_RESPONSE_BYTES {
                    return None;
                }
                raw_resp.extend_from_slice(&chunk[..count]);
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue
            }
            Err(_) => return None,
        }
    }
    let body_bytes = parse_antigravity_http_200_body(&raw_resp)?;
    if find_freshest_antigravity_hub()?.2 != instance {
        return None;
    }
    let parsed: Value = serde_json::from_slice(body_bytes).ok()?;
    let summaries = parsed.get("trajectorySummaries")?.as_object()?;
    let active_frontmost_cid = suppress_frontmost
        .then(|| {
            mi_macos::session_navigation::frontmost_selected_antigravity_conversation(
                instance.pid,
                instance.ui_port,
            )
        })
        .flatten();
    let (candidates, project_ids, frontmost_acknowledged) =
        classify_antigravity_trajectory_summaries(
            summaries,
            active_frontmost_cid.as_deref(),
            seen_running_ids,
            completed_transition_watermarks,
        );
    Some(NavigationCatalog {
        instance: CatalogInstance::Antigravity(instance),
        observed_at: Instant::now(),
        candidates,
        project_ids,
        frontmost_acknowledged,
    })
}

pub fn codex_observations_to_catalog(
    codex_home: std::path::PathBuf,
    observations: &[mi_macos::CodexThreadObservation],
) -> NavigationCatalog {
    codex_observations_to_catalog_with_ack(codex_home, observations, None)
}

pub fn codex_observations_to_catalog_with_ack(
    codex_home: std::path::PathBuf,
    observations: &[mi_macos::CodexThreadObservation],
    frontmost_acknowledged: Option<(NavigationPlace, String)>,
) -> NavigationCatalog {
    let mut candidates = Vec::new();

    for obs in observations {
        let (phase, last_modified_ms, last_user_view_ms) = match &obs.status {
            mi_macos::CodexObservedStatus::NeedsAttention => (
                FourAppSessionKind::NeedsAttention,
                obs.updated_at_ms,
                obs.updated_at_ms,
            ),
            mi_macos::CodexObservedStatus::Working => (
                FourAppSessionKind::Working,
                obs.updated_at_ms,
                obs.updated_at_ms,
            ),
            mi_macos::CodexObservedStatus::Error => (
                FourAppSessionKind::Error,
                obs.updated_at_ms,
                obs.updated_at_ms,
            ),
            mi_macos::CodexObservedStatus::Unread => (
                FourAppSessionKind::BlueDotCompleted,
                obs.updated_at_ms.max(1000),
                0,
            ),
            mi_macos::CodexObservedStatus::Idle => (
                FourAppSessionKind::Idle,
                obs.updated_at_ms,
                obs.updated_at_ms,
            ),
            mi_macos::CodexObservedStatus::Unknown(_) => continue,
        };

        let workspace_path = obs.workspace_path.trim().to_string();
        let title = if obs.display_title.trim().is_empty() {
            obs.thread_id.clone()
        } else {
            obs.display_title.trim().to_string()
        };
        let watermark = if obs.watermark.is_empty() {
            obs.updated_at_ms.to_string()
        } else {
            obs.watermark.clone()
        };

        candidates.push(FourAppLiveCandidate {
            place: NavigationPlace::codex(
                obs.thread_id.clone(),
                obs.host_id.clone(),
                workspace_path,
            ),
            title,
            phase,
            watermark,
            last_modified_ms,
            last_user_view_ms,
            updated_at_ms: obs.updated_at_ms,
        });
    }

    candidates.sort_by(|a, b| {
        a.phase
            .cmp(&b.phase)
            .then_with(|| b.updated_at_ms.cmp(&a.updated_at_ms))
            .then_with(|| a.place.target_id().cmp(&b.place.target_id()))
    });

    NavigationCatalog {
        instance: CatalogInstance::Codex { codex_home },
        observed_at: Instant::now(),
        candidates,
        project_ids: BTreeMap::new(),
        frontmost_acknowledged,
    }
}

pub(crate) fn poll_codex_session_candidates() -> Option<NavigationCatalog> {
    let codex_home = mi_macos::default_codex_home()?;
    let mut observations = mi_macos::observe_codex_threads(&codex_home, 64).ok()?;
    let frontmost_acknowledged = if observations.iter().any(|obs| {
        matches!(
            obs.status,
            mi_macos::CodexObservedStatus::Working
                | mi_macos::CodexObservedStatus::Unread
                | mi_macos::CodexObservedStatus::Error
        )
    }) {
        mi_macos::observe_codex_live_window_signals()
            .and_then(|signals| mi_macos::apply_codex_window_signals(&mut observations, &signals))
            .map(|(thread_id, host_id, workspace_path, watermark)| {
                (
                    NavigationPlace::codex(thread_id, host_id, workspace_path),
                    watermark,
                )
            })
    } else {
        None
    };
    Some(codex_observations_to_catalog_with_ack(
        codex_home,
        &observations,
        frontmost_acknowledged,
    ))
}
