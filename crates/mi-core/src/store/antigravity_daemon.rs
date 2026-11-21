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
            let mut liveness = AntigravityLivenessTracker::new();
            loop {
                let source_gen = {
                    let (lock, _) = &*live_source;
                    lock.lock().unwrap().generation
                };
                let epoch = epoch_source.load(Ordering::Acquire);
                if let AntigravityPollUpdate::Replace(ag_catalog) =
                    poll_antigravity_daemon_candidates_with_liveness(
                        &mut seen_running_ids,
                        &mut completed_transition_watermarks,
                        &mut liveness,
                    )
                {
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
        options.custom_flags(mi_macos::regular_file_nofollow_open_flags());
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

        let direct_waiting = waiting_steps
            || matches!(
                status_str,
                "CASCADE_RUN_STATUS_WAITING_FOR_USER_INPUT"
                    | "CASCADE_RUN_STATUS_WAITING_FOR_PERMISSION"
            );
        let (phase, effective_view_ms, status_detail) = if !killed
            && (has_waiting || direct_waiting)
        {
            seen_running_ids.insert(cid.clone());
            let detail = if direct_waiting {
                "等待审批或用户输入"
            } else {
                "子代理等待审批或用户输入"
            };
            (
                FourAppSessionKind::NeedsAttention,
                raw_view_ms,
                Some(detail.to_string()),
            )
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
            (FourAppSessionKind::Working, raw_view_ms, None)
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
                (
                    FourAppSessionKind::Idle,
                    last_modified_ms.max(raw_view_ms),
                    None,
                )
            } else if has_unviewed || just_transitioned {
                if is_errored {
                    (FourAppSessionKind::Error, raw_view_ms, None)
                } else {
                    let adj_view_ms = if last_modified_ms > raw_view_ms.saturating_add(500) {
                        raw_view_ms
                    } else {
                        last_modified_ms.saturating_sub(1000)
                    };
                    (
                        FourAppSessionKind::BlueDotCompleted,
                        adj_view_ms,
                        Some("已完成未读 · 按键直达".to_string()),
                    )
                }
            } else {
                (FourAppSessionKind::Idle, raw_view_ms, None)
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
            status_detail,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DisconnectEpisode {
    pid: i32,
    since: Instant,
    episode: u64,
}

#[derive(Debug)]
pub(crate) enum AntigravityPollUpdate {
    Replace(Option<NavigationCatalog>),
    KeepExisting,
}

#[derive(Debug, Default)]
pub(crate) struct AntigravityLivenessTracker {
    progress_by_cid: BTreeMap<String, (u64, u64)>,
    disconnect: Option<DisconnectEpisode>,
    next_disconnect_episode: u64,
    log_reader: antigravity_log::AntigravityLogTailReader,
}

impl AntigravityLivenessTracker {
    pub(crate) fn new() -> Self {
        Self {
            progress_by_cid: BTreeMap::new(),
            disconnect: None,
            next_disconnect_episode: 0,
            log_reader: antigravity_log::AntigravityLogTailReader::new(),
        }
    }

    pub(crate) fn reconcile_poll_result(
        &mut self,
        poll_result: Option<NavigationCatalog>,
        antigravity_pid: impl FnOnce() -> Option<i32>,
        now_instant: Instant,
        now_ms: u64,
    ) -> AntigravityPollUpdate {
        if let Some(mut catalog) = poll_result {
            self.disconnect = None;
            let log = self.log_reader.read_snapshot(now_ms);
            apply_antigravity_liveness(
                &mut catalog.candidates,
                &log,
                &mut self.progress_by_cid,
                now_ms,
            );
            return AntigravityPollUpdate::Replace(Some(catalog));
        }
        self.reconcile_rpc_failure(antigravity_pid(), now_instant, now_ms)
    }

    fn reconcile_rpc_failure(
        &mut self,
        antigravity_pid: Option<i32>,
        now_instant: Instant,
        now_ms: u64,
    ) -> AntigravityPollUpdate {
        let Some(pid) = antigravity_pid else {
            self.disconnect = None;
            self.progress_by_cid.clear();
            return AntigravityPollUpdate::Replace(None);
        };
        let active = match self.disconnect {
            Some(ep) if ep.pid == pid => ep,
            _ => {
                self.next_disconnect_episode = self.next_disconnect_episode.wrapping_add(1);
                let ep = DisconnectEpisode {
                    pid,
                    since: now_instant,
                    episode: self.next_disconnect_episode,
                };
                self.disconnect = Some(ep);
                ep
            }
        };
        let elapsed = now_instant
            .saturating_duration_since(active.since)
            .as_secs();
        if elapsed < 3 {
            return AntigravityPollUpdate::KeepExisting;
        }
        let episode = active.episode;
        let (phase, title, status_detail, watermark) = if elapsed < 12 {
            (
                FourAppSessionKind::NeedsAttention,
                "Antigravity 后台重连中".to_string(),
                format!("正在重连 Antigravity 后台服务（{elapsed}s）"),
                format!("disconnect:warn:{pid}:{episode}"),
            )
        } else {
            (
                FourAppSessionKind::Error,
                "Antigravity 后台断连".to_string(),
                format!("Antigravity 后台守护进程无响应已 {elapsed}s"),
                format!("disconnect:error:{pid}:{episode}"),
            )
        };
        AntigravityPollUpdate::Replace(Some(NavigationCatalog {
            instance: CatalogInstance::Antigravity(AntigravityInstance {
                hub: "disconnected".to_string(),
                pid,
                ui_port: 0,
            }),
            observed_at: now_instant,
            candidates: vec![FourAppLiveCandidate {
                place: NavigationPlace::app("com.google.antigravity"),
                title,
                phase,
                watermark,
                last_modified_ms: now_ms,
                last_user_view_ms: 0,
                updated_at_ms: now_ms,
                status_detail: Some(status_detail),
            }],
            project_ids: BTreeMap::new(),
            frontmost_acknowledged: None,
        }))
    }
}

fn format_retry_detail(retry: &antigravity_log::AntigravityLogRetry) -> String {
    let raw = match retry.retry_delay.as_deref().filter(|d| !d.is_empty()) {
        Some(delay) => format!(
            "模型自动重试（第 {} 次）：{} · {delay}",
            retry.attempt, retry.reason
        ),
        None => format!("模型自动重试（第 {} 次）：{}", retry.attempt, retry.reason),
    };
    antigravity_log::truncate_chars(&raw, 140)
}

fn format_stall_detail(stalled_ms: u64) -> String {
    let total_secs = stalled_ms / 1000;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    format!("等待模型响应已 {mins}m{secs:02}s（超 60s 无进展）")
}

fn retry_active_window_ms(retry: &antigravity_log::AntigravityLogRetry) -> u64 {
    retry
        .retry_delay
        .as_deref()
        .and_then(antigravity_log::parse_go_duration_ms)
        .map(|delay_ms| delay_ms.saturating_add(45_000).clamp(90_000, 240_000))
        .unwrap_or(120_000)
}

pub(crate) fn apply_antigravity_liveness(
    candidates: &mut [FourAppLiveCandidate],
    log: &antigravity_log::AntigravityLogSnapshot,
    progress_by_cid: &mut BTreeMap<String, (u64, u64)>,
    now_ms: u64,
) {
    let mut active_working_ids = BTreeSet::new();
    for cand in candidates.iter_mut() {
        let cid = cand.place.target_id();
        if cand.phase == FourAppSessionKind::Working {
            active_working_ids.insert(cid.clone());
            let entry = progress_by_cid.entry(cid).or_insert_with(|| {
                let initial_progress = if cand.last_modified_ms > 0 {
                    cand.last_modified_ms.min(now_ms)
                } else {
                    now_ms
                };
                (cand.last_modified_ms, initial_progress)
            });
            if entry.0 != cand.last_modified_ms {
                entry.0 = cand.last_modified_ms;
                entry.1 = now_ms;
            }
            let last_progress_observed_at_ms = entry.1;
            let stalled_ms = now_ms.saturating_sub(last_progress_observed_at_ms);
            let active_retry = log.latest_retry.as_ref().filter(|retry| {
                now_ms.saturating_sub(retry.ts_ms) <= retry_active_window_ms(retry)
                    && retry.ts_ms.saturating_add(2_000) >= last_progress_observed_at_ms
            });
            if let Some(retry) = active_retry {
                cand.phase = FourAppSessionKind::NeedsAttention;
                cand.status_detail = Some(format_retry_detail(retry));
            } else if stalled_ms >= 60_000 {
                cand.phase = FourAppSessionKind::NeedsAttention;
                cand.status_detail = Some(format_stall_detail(stalled_ms));
            } else {
                cand.status_detail = None;
            }
        } else {
            progress_by_cid.remove(&cid);
            match cand.phase {
                FourAppSessionKind::Error if cand.status_detail.is_none() => {
                    let matched_err = log.latest_api_error.as_ref().filter(|err| {
                        if cand.last_modified_ms == 0 {
                            now_ms.saturating_sub(err.ts_ms) <= 120_000
                        } else {
                            err.ts_ms.abs_diff(cand.last_modified_ms) <= 90_000
                        }
                    });
                    cand.status_detail = Some(match matched_err {
                        Some(err) => antigravity_log::truncate_chars(
                            &format!("API 报错：{}", err.reason),
                            168,
                        ),
                        None => "执行出错，点击查看".to_string(),
                    });
                }
                FourAppSessionKind::BlueDotCompleted if cand.status_detail.is_none() => {
                    cand.status_detail = Some("已完成未读 · 按键直达".to_string());
                }
                _ => {}
            }
        }
    }
    progress_by_cid.retain(|cid, _| active_working_ids.contains(cid));
    candidates.sort_by(|a, b| {
        a.phase
            .cmp(&b.phase)
            .then_with(|| b.last_modified_ms.cmp(&a.last_modified_ms))
            .then_with(|| a.place.target_id().cmp(&b.place.target_id()))
    });
}

pub(super) fn poll_antigravity_daemon_candidates_with_liveness(
    seen_running_ids: &mut BTreeSet<String>,
    completed_transition_watermarks: &mut BTreeMap<String, String>,
    tracker: &mut AntigravityLivenessTracker,
) -> AntigravityPollUpdate {
    let poll_result = poll_antigravity_daemon_candidates_with_frontmost(
        seen_running_ids,
        completed_transition_watermarks,
        true,
    );
    let now_instant = Instant::now();
    let now_ms = antigravity_log::system_now_ms();
    tracker.reconcile_poll_result(
        poll_result,
        mi_macos::session_navigation::antigravity_process_id,
        now_instant,
        now_ms,
    )
}

#[cfg(test)]
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
            status_detail: None,
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

#[cfg(test)]
mod liveness_tests {
    use super::*;
    use crate::store::antigravity_log::{
        AntigravityLogApiError, AntigravityLogRetry, AntigravityLogSnapshot,
    };

    fn make_ag_candidate(
        cid: &str,
        phase: FourAppSessionKind,
        last_modified_ms: u64,
    ) -> FourAppLiveCandidate {
        FourAppLiveCandidate {
            place: NavigationPlace::antigravity(cid, "/ws/ag"),
            title: format!("Conv {cid}"),
            phase,
            watermark: format!("wm:{cid}:{last_modified_ms}"),
            last_modified_ms,
            last_user_view_ms: 0,
            updated_at_ms: last_modified_ms,
            status_detail: None,
        }
    }

    #[test]
    fn retry_promotes_working_and_clears_on_progress_or_adaptive_window_expiry() {
        let mut progress = BTreeMap::new();
        let now_ms = 1_700_000_100_000u64;
        let retry_log = AntigravityLogSnapshot {
            latest_retry: Some(AntigravityLogRetry {
                attempt: 2,
                reason: "stream reading error: unexpected EOF".to_string(),
                retry_delay: Some("4s".to_string()),
                ts_ms: now_ms - 1_000,
            }),
            latest_api_error: None,
        };

        let mut cands = vec![make_ag_candidate(
            "c1",
            FourAppSessionKind::Working,
            now_ms - 5_000,
        )];
        apply_antigravity_liveness(&mut cands, &retry_log, &mut progress, now_ms);
        assert_eq!(cands[0].phase, FourAppSessionKind::NeedsAttention);
        assert_eq!(
            cands[0].status_detail.as_deref(),
            Some("模型自动重试（第 2 次）：stream reading error: unexpected EOF · 4s")
        );

        let mut cands_progress = vec![make_ag_candidate(
            "c1",
            FourAppSessionKind::Working,
            now_ms + 4_000,
        )];
        apply_antigravity_liveness(
            &mut cands_progress,
            &retry_log,
            &mut progress,
            now_ms + 5_000,
        );
        assert_eq!(cands_progress[0].phase, FourAppSessionKind::Working);
        assert_eq!(cands_progress[0].status_detail, None);

        let mut progress_expiry = BTreeMap::new();
        let mut cands_expired = vec![make_ag_candidate(
            "c2",
            FourAppSessionKind::Working,
            now_ms + 50_000,
        )];
        apply_antigravity_liveness(
            &mut cands_expired,
            &retry_log,
            &mut progress_expiry,
            now_ms + 95_000,
        );
        assert_eq!(cands_expired[0].phase, FourAppSessionKind::Working);
        assert_eq!(cands_expired[0].status_detail, None);
    }

    #[test]
    fn stall_seeds_from_last_modified_and_reorders_ahead_of_healthy_working() {
        let mut progress = BTreeMap::new();
        let log = AntigravityLogSnapshot::default();
        let t0 = 1_700_000_100_000u64;

        let mut immediate = vec![make_ag_candidate(
            "pre-stalled",
            FourAppSessionKind::Working,
            t0 - 75_000,
        )];
        apply_antigravity_liveness(&mut immediate, &log, &mut progress, t0);
        assert_eq!(immediate[0].phase, FourAppSessionKind::NeedsAttention);
        assert_eq!(
            immediate[0].status_detail.as_deref(),
            Some("等待模型响应已 1m15s（超 60s 无进展）")
        );

        let mut progress2 = BTreeMap::new();
        let mut tick1 = vec![
            make_ag_candidate("healthy", FourAppSessionKind::Working, t0 - 2_000),
            make_ag_candidate("stalled", FourAppSessionKind::Working, t0 - 5_000),
        ];
        apply_antigravity_liveness(&mut tick1, &log, &mut progress2, t0);
        assert_eq!(tick1[0].phase, FourAppSessionKind::Working);
        assert_eq!(tick1[0].status_detail, None);
        assert_eq!(tick1[1].phase, FourAppSessionKind::Working);
        assert_eq!(tick1[1].status_detail, None);

        let mut tick2 = vec![
            make_ag_candidate("healthy", FourAppSessionKind::Working, t0 + 60_000),
            make_ag_candidate("stalled", FourAppSessionKind::Working, t0 - 5_000),
        ];
        apply_antigravity_liveness(&mut tick2, &log, &mut progress2, t0 + 60_000);
        assert_eq!(
            tick2[0].place,
            NavigationPlace::antigravity("stalled", "/ws/ag")
        );
        assert_eq!(tick2[0].phase, FourAppSessionKind::NeedsAttention);
        assert_eq!(
            tick2[0].status_detail.as_deref(),
            Some("等待模型响应已 1m05s（超 60s 无进展）")
        );
        assert_eq!(
            tick2[1].place,
            NavigationPlace::antigravity("healthy", "/ws/ag")
        );
        assert_eq!(tick2[1].phase, FourAppSessionKind::Working);
        assert_eq!(tick2[1].status_detail, None);
    }

    #[test]
    fn api_error_enriches_matching_error_candidate_and_falls_back_when_stale() {
        let mut progress = BTreeMap::new();
        let err_ts = 1_700_000_100_000u64;
        let log = AntigravityLogSnapshot {
            latest_retry: None,
            latest_api_error: Some(AntigravityLogApiError {
                reason: "RESOURCE_EXHAUSTED (code 429): Individual quota reached. Resets in 13m8s"
                    .to_string(),
                ts_ms: err_ts,
            }),
        };

        let mut cands = vec![
            make_ag_candidate("matched", FourAppSessionKind::Error, err_ts - 15_000),
            make_ag_candidate("stale", FourAppSessionKind::Error, err_ts - 200_000),
            make_ag_candidate("done", FourAppSessionKind::BlueDotCompleted, err_ts - 5_000),
        ];
        apply_antigravity_liveness(&mut cands, &log, &mut progress, err_ts + 2_000);
        let by_id: BTreeMap<String, &FourAppLiveCandidate> =
            cands.iter().map(|c| (c.place.target_id(), c)).collect();
        assert_eq!(
            by_id[&NavigationPlace::antigravity("matched", "/ws/ag").target_id()]
                .status_detail
                .as_deref(),
            Some("API 报错：RESOURCE_EXHAUSTED (code 429): Individual quota reached. Resets in 13m8s")
        );
        assert_eq!(
            by_id[&NavigationPlace::antigravity("stale", "/ws/ag").target_id()]
                .status_detail
                .as_deref(),
            Some("执行出错，点击查看")
        );
        assert_eq!(
            by_id[&NavigationPlace::antigravity("done", "/ws/ag").target_id()]
                .status_detail
                .as_deref(),
            Some("已完成未读 · 按键直达")
        );
    }

    #[test]
    fn liveness_tracker_keeps_existing_during_grace_resets_on_pid_restart_and_bumps_episodes() {
        let mut tracker = AntigravityLivenessTracker::new();
        let t0 = Instant::now();
        let now_ms = 1_700_000_000_000u64;

        assert!(matches!(
            tracker.reconcile_poll_result(None, || Some(4242), t0, now_ms),
            AntigravityPollUpdate::KeepExisting
        ));
        assert!(matches!(
            tracker.reconcile_poll_result(
                None,
                || Some(4242),
                t0 + Duration::from_secs(2),
                now_ms + 2_000
            ),
            AntigravityPollUpdate::KeepExisting
        ));

        let AntigravityPollUpdate::Replace(Some(warn_cat)) = tracker.reconcile_poll_result(
            None,
            || Some(4242),
            t0 + Duration::from_secs(5),
            now_ms + 5_000,
        ) else {
            panic!("expected warn synthetic catalog");
        };
        assert_eq!(warn_cat.candidates.len(), 1);
        assert_eq!(
            warn_cat.candidates[0].place,
            NavigationPlace::app("com.google.antigravity")
        );
        assert_eq!(
            warn_cat.candidates[0].phase,
            FourAppSessionKind::NeedsAttention
        );
        assert_eq!(warn_cat.candidates[0].title, "Antigravity 后台重连中");
        assert_eq!(warn_cat.candidates[0].watermark, "disconnect:warn:4242:1");
        assert_eq!(
            warn_cat.candidates[0].status_detail.as_deref(),
            Some("正在重连 Antigravity 后台服务（5s）")
        );

        let AntigravityPollUpdate::Replace(Some(err_cat_1)) = tracker.reconcile_poll_result(
            None,
            || Some(4242),
            t0 + Duration::from_secs(14),
            now_ms + 14_000,
        ) else {
            panic!("expected error synthetic catalog");
        };
        assert_eq!(err_cat_1.candidates[0].phase, FourAppSessionKind::Error);
        assert_eq!(err_cat_1.candidates[0].title, "Antigravity 后台断连");
        assert_eq!(err_cat_1.candidates[0].watermark, "disconnect:error:4242:1");
        assert_eq!(
            err_cat_1.candidates[0].status_detail.as_deref(),
            Some("Antigravity 后台守护进程无响应已 14s")
        );

        let healthy_cat = NavigationCatalog {
            instance: CatalogInstance::Antigravity(AntigravityInstance {
                hub: "hub-ok".into(),
                pid: 4242,
                ui_port: 8443,
            }),
            observed_at: t0 + Duration::from_secs(20),
            candidates: Vec::new(),
            project_ids: BTreeMap::new(),
            frontmost_acknowledged: None,
        };
        assert!(matches!(
            tracker.reconcile_poll_result(
                Some(healthy_cat),
                || Some(4242),
                t0 + Duration::from_secs(20),
                now_ms + 20_000,
            ),
            AntigravityPollUpdate::Replace(Some(_))
        ));

        assert!(matches!(
            tracker.reconcile_poll_result(
                None,
                || Some(4242),
                t0 + Duration::from_secs(30),
                now_ms + 30_000,
            ),
            AntigravityPollUpdate::KeepExisting
        ));
        let AntigravityPollUpdate::Replace(Some(err_cat_2)) = tracker.reconcile_poll_result(
            None,
            || Some(4242),
            t0 + Duration::from_secs(45),
            now_ms + 45_000,
        ) else {
            panic!("expected second disconnect error catalog");
        };
        assert_eq!(err_cat_2.candidates[0].watermark, "disconnect:error:4242:2");
        assert_ne!(
            err_cat_2.candidates[0].watermark,
            err_cat_1.candidates[0].watermark
        );

        assert!(matches!(
            tracker.reconcile_poll_result(
                None,
                || Some(9999),
                t0 + Duration::from_secs(46),
                now_ms + 46_000,
            ),
            AntigravityPollUpdate::KeepExisting
        ));
        let AntigravityPollUpdate::Replace(Some(restarted_warn)) = tracker.reconcile_poll_result(
            None,
            || Some(9999),
            t0 + Duration::from_secs(50),
            now_ms + 50_000,
        ) else {
            panic!("expected restarted pid warn catalog");
        };
        assert_eq!(
            restarted_warn.candidates[0].phase,
            FourAppSessionKind::NeedsAttention
        );
        assert_eq!(
            restarted_warn.candidates[0].watermark,
            "disconnect:warn:9999:3"
        );

        assert!(matches!(
            tracker.reconcile_poll_result(
                None,
                || None,
                t0 + Duration::from_secs(55),
                now_ms + 55_000,
            ),
            AntigravityPollUpdate::Replace(None)
        ));
    }

    #[test]
    fn activating_disconnect_slot_acknowledges_episode_and_second_episode_relights_red() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(dir.path()).unwrap();
        let place = NavigationPlace::app("com.google.antigravity");

        let make_disconnect_cat = |episode: u64| NavigationCatalog {
            instance: CatalogInstance::Antigravity(AntigravityInstance {
                hub: "disconnected".into(),
                pid: 4242,
                ui_port: 0,
            }),
            observed_at: Instant::now(),
            candidates: vec![FourAppLiveCandidate {
                place: place.clone(),
                title: "Antigravity 后台断连".into(),
                phase: FourAppSessionKind::Error,
                watermark: format!("disconnect:error:4242:{episode}"),
                last_modified_ms: 10_000 + episode,
                last_user_view_ms: 0,
                updated_at_ms: 10_000 + episode,
                status_detail: Some("Antigravity 后台守护进程无响应已 14s".into()),
            }],
            project_ids: BTreeMap::new(),
            frontmost_acknowledged: None,
        };

        store.update_navigation_catalog_for_source(
            LiveSessionSource::Antigravity,
            Some(make_disconnect_cat(1)),
        );
        assert_eq!(
            store.projection().session_strip.slots[0].kind,
            FourAppSessionKind::Error
        );
        assert_eq!(store.projection().canvas.underglow_hex, "#FF2200");

        store.session_navigation.test_worker = Some(Box::new(|selected, reply| {
            let _ = reply.send(Ok(selected));
        }));
        store
            .dispatch_session_strip_action(SessionStripAction::ActivateSlot(0))
            .unwrap();
        store.poll_session_navigation();
        assert_eq!(
            store
                .lighting_arbiter
                .acknowledged_watermarks
                .get(&place.target_id())
                .map(String::as_str),
            Some("disconnect:error:4242:1")
        );
        assert_eq!(
            store.projection().session_strip.slots[0].kind,
            FourAppSessionKind::Idle
        );
        assert_eq!(
            store.projection().session_strip.slots[0].status_detail,
            None
        );
        assert_eq!(store.projection().canvas.underglow_hex, "#000000");

        store.update_navigation_catalog_for_source(
            LiveSessionSource::Antigravity,
            Some(make_disconnect_cat(2)),
        );
        assert_eq!(
            store.projection().session_strip.slots[0].kind,
            FourAppSessionKind::Error
        );
        assert_eq!(store.projection().canvas.underglow_hex, "#FF2200");
    }

    #[test]
    fn status_detail_updates_projection_without_churning_slot_revision_or_hardware_led_and_localizes_en(
    ) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = WorkbenchStore::open_isolated(dir.path()).unwrap();
        let place = NavigationPlace::antigravity("conv-detail", "/ws/ag");

        let make_cat = |title: &str, phase, detail: Option<&str>| NavigationCatalog {
            instance: CatalogInstance::Antigravity(AntigravityInstance {
                hub: "hub-1".into(),
                pid: 100,
                ui_port: 8443,
            }),
            observed_at: Instant::now(),
            candidates: vec![FourAppLiveCandidate {
                place: place.clone(),
                title: title.into(),
                phase,
                watermark: "wm-err-1".into(),
                last_modified_ms: 10_000,
                last_user_view_ms: 0,
                updated_at_ms: 10_000,
                status_detail: detail.map(str::to_string),
            }],
            project_ids: BTreeMap::new(),
            frontmost_acknowledged: None,
        };

        store.update_navigation_catalog_for_source(
            LiveSessionSource::Antigravity,
            Some(make_cat(
                "Retry Conv",
                FourAppSessionKind::NeedsAttention,
                Some("等待模型响应已 1m01s（超 60s 无进展）"),
            )),
        );
        let gen1 = store.projection_generation();
        let rev1 = store
            .session_navigation
            .dispatch
            .slots
            .lock()
            .unwrap()
            .antigravity_revision;
        let digest1 = store.lighting_arbiter.last_hardware_sync_json.clone();
        assert_eq!(
            store.projection().session_strip.slots[0]
                .status_detail
                .as_deref(),
            Some("等待模型响应已 1m01s（超 60s 无进展）")
        );

        store.update_navigation_catalog_for_source(
            LiveSessionSource::Antigravity,
            Some(make_cat(
                "Retry Conv",
                FourAppSessionKind::NeedsAttention,
                Some("等待模型响应已 1m05s（超 60s 无进展）"),
            )),
        );
        let gen2 = store.projection_generation();
        let rev2 = store
            .session_navigation
            .dispatch
            .slots
            .lock()
            .unwrap()
            .antigravity_revision;
        let digest2 = store.lighting_arbiter.last_hardware_sync_json.clone();
        assert!(gen2 > gen1);
        assert_eq!(rev2, rev1);
        assert_eq!(digest2, digest1);

        store
            .dispatch_header_action(HeaderAction::SetUiLanguage {
                language: mi_model::UiLanguagePreference::En,
            })
            .unwrap();
        assert_eq!(
            store.projection().session_strip.slots[0]
                .status_detail
                .as_deref(),
            Some("Waiting for model response for 1m05s (>60s stalled)")
        );

        store.update_navigation_catalog_for_source(
            LiveSessionSource::Antigravity,
            Some(make_cat(
                "Antigravity 后台重连中",
                FourAppSessionKind::NeedsAttention,
                Some("正在重连 Antigravity 后台服务（5s）"),
            )),
        );
        assert_eq!(
            store.projection().session_strip.slots[0].title,
            "Reconnecting Antigravity Daemon"
        );
        assert_eq!(
            store.projection().session_strip.slots[0]
                .status_detail
                .as_deref(),
            Some("Reconnecting Antigravity daemon (5s)")
        );

        store.update_navigation_catalog_for_source(
            LiveSessionSource::Antigravity,
            Some(make_cat(
                "Antigravity 后台断连",
                FourAppSessionKind::Error,
                Some("API 报错：RESOURCE_EXHAUSTED (code 429)"),
            )),
        );
        assert_eq!(
            store.projection().session_strip.slots[0].title,
            "Antigravity Daemon Disconnected"
        );
        assert_eq!(
            store.projection().session_strip.slots[0]
                .status_detail
                .as_deref(),
            Some("API error: RESOURCE_EXHAUSTED (code 429)")
        );

        assert_eq!(
            crate::i18n::localize_session_status_detail(
                "模型自动重试（第 2 次）：unexpected EOF · 4s",
                mi_model::ResolvedLocale::En,
            ),
            "Model retrying (attempt 2): unexpected EOF · 4s"
        );
        assert_eq!(
            crate::i18n::localize_session_status_detail(
                "Antigravity 后台守护进程无响应已 14s",
                mi_model::ResolvedLocale::En,
            ),
            "Antigravity daemon unresponsive for 14s"
        );
        assert_eq!(
            crate::i18n::localize_session_status_detail(
                "子代理等待审批或用户输入",
                mi_model::ResolvedLocale::En,
            ),
            "Subagent waiting for approval or user input"
        );

        store.lighting_arbiter.acknowledge_slot(&place, "wm-err-1");
        let acks_after = store.lighting_arbiter.acknowledged_watermarks.clone();
        store.slot_table.refresh_with_watermarks(&acks_after);
        store.refresh_four_app_lighting();
        store.commit_runtime_update();
        assert_eq!(
            store.projection().session_strip.slots[0].kind,
            FourAppSessionKind::Idle
        );
        assert_eq!(
            store.projection().session_strip.slots[0].status_detail,
            None
        );
    }

    #[test]
    fn classify_sets_direct_waiting_and_descendant_waiting_status_details() {
        let summaries = serde_json::json!({
            "main-direct": {
                "summary": "Direct Wait",
                "status": "CASCADE_RUN_STATUS_WAITING_FOR_PERMISSION",
                "lastModifiedTime": "2026-11-21T14:00:00Z",
                "workspaces": [{ "workspaceFolderAbsoluteUri": "file:///ws/a" }]
            },
            "main-parent": {
                "summary": "Parent With Subagent",
                "status": "CASCADE_RUN_STATUS_RUNNING",
                "lastModifiedTime": "2026-11-21T14:00:00Z",
                "workspaces": [{ "workspaceFolderAbsoluteUri": "file:///ws/a" }]
            },
            "child-sub": {
                "summary": "Subagent",
                "status": "CASCADE_RUN_STATUS_WAITING_FOR_USER_INPUT",
                "trajectoryMetadata": { "parentConversationId": "main-parent" },
                "workspaces": [{ "workspaceFolderAbsoluteUri": "file:///ws/a" }]
            }
        });
        let mut seen = BTreeSet::new();
        let mut wms = BTreeMap::new();
        let (cands, _, _) = classify_antigravity_trajectory_summaries(
            summaries.as_object().unwrap(),
            None,
            &mut seen,
            &mut wms,
        );
        let by_title: BTreeMap<&str, &FourAppLiveCandidate> =
            cands.iter().map(|c| (c.title.as_str(), c)).collect();
        assert_eq!(
            by_title["Direct Wait"].status_detail.as_deref(),
            Some("等待审批或用户输入")
        );
        assert_eq!(
            by_title["Parent With Subagent"].status_detail.as_deref(),
            Some("子代理等待审批或用户输入")
        );
    }
}
