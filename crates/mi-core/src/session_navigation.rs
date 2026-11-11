use super::*;
use mi_macos::HostOsDriver;
use std::sync::{mpsc, Mutex};

const CATALOG_TTL: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AntigravityInstance {
    pub hub: String,
    pub pid: i32,
    pub ui_port: u16,
}

#[derive(Clone, Debug)]
pub struct NavigationCatalog {
    pub instance: AntigravityInstance,
    pub observed_at: Instant,
    pub candidates: Vec<FourAppLiveCandidate>,
    pub project_ids: BTreeMap<String, String>,
    pub frontmost_acknowledged: Option<(NavigationPlace, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NavigationCommand {
    Slot(usize),
    Back,
    Forward,
}
impl NavigationCommand {
    pub(crate) fn from_action(action: &ResolvedHostAction) -> Option<Self> {
        let ResolvedHostAction::AgentCommand { adapter, command } = action else {
            return None;
        };
        if let Some(index) = command
            .strip_prefix("slot-open:")
            .and_then(|s| s.parse().ok())
        {
            Some(Self::Slot(index))
        } else if adapter == "navigation" && command == "back" {
            Some(Self::Back)
        } else if adapter == "navigation" && command == "forward" {
            Some(Self::Forward)
        } else {
            None
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SlotRevision {
    pub revision: u64,
    pub changed_at: Instant,
    targets: Vec<Option<NavigationPlace>>,
    instance: Option<AntigravityInstance>,
}
impl Default for SlotRevision {
    fn default() -> Self {
        Self {
            revision: 0,
            changed_at: Instant::now(),
            targets: Vec::new(),
            instance: None,
        }
    }
}
#[derive(Clone)]
pub(crate) struct NavigationDispatch {
    pub slots: Arc<Mutex<SlotRevision>>,
    pub tx: mpsc::SyncSender<PhysicalNavigationIntent>,
}
pub(crate) struct PhysicalNavigationIntent {
    pub command: NavigationCommand,
    pub revision: u64,
    pub input_generation: String,
    pub admission: crate::six_layer_runtime::ExecutionAdmission,
    pub received_at: Instant,
    pub profile: String,
    pub binding: String,
}

#[derive(Clone)]
pub(crate) enum NavigationInputLease {
    SixLayer(String, crate::six_layer_runtime::ExecutionAdmission),
    Vendor(u64),
}

struct PendingNavigation {
    place: NavigationPlace,
    watermark: String,
    acknowledged_at_request: Option<String>,
    instance: Option<AntigravityInstance>,
    history: NavigationHistory,
    history_base: NavigationHistory,
    input_generation: Option<NavigationInputLease>,
    cancelled: Arc<AtomicBool>,
    result: mpsc::Receiver<Result<NavigationPlace, String>>,
    profile: String,
    binding: String,
}

pub(super) struct SessionNavigation {
    pub catalog: Option<NavigationCatalog>,
    pub dispatch: NavigationDispatch,
    intents: mpsc::Receiver<PhysicalNavigationIntent>,
    pending: Option<PendingNavigation>,
    #[cfg(test)]
    test_worker:
        Option<Box<dyn FnOnce(NavigationPlace, mpsc::SyncSender<Result<NavigationPlace, String>>)>>,
}
impl Default for SessionNavigation {
    fn default() -> Self {
        let (tx, intents) = mpsc::sync_channel(16);
        Self {
            catalog: None,
            dispatch: NavigationDispatch {
                slots: Arc::new(Mutex::new(SlotRevision::default())),
                tx,
            },
            intents,
            pending: None,
            #[cfg(test)]
            test_worker: None,
        }
    }
}
impl Drop for SessionNavigation {
    fn drop(&mut self) {
        if let Some(pending) = &self.pending {
            pending.cancelled.store(true, Ordering::Release);
        }
    }
}

impl WorkbenchStore {
    pub(super) fn refresh_navigation_revision(&mut self) {
        let instance = self
            .session_navigation
            .catalog
            .as_ref()
            .map(|c| &c.instance);
        let slots = self.slot_table.slot_table().slots();
        let mut current = self.session_navigation.dispatch.slots.lock().unwrap();
        let unchanged = current.instance.as_ref() == instance
            && current.targets.len() == slots.len()
            && current
                .targets
                .iter()
                .zip(slots)
                .all(|(target, slot)| target == &slot.place);
        if unchanged {
            return;
        }
        current.targets = slots.iter().map(|slot| slot.place.clone()).collect();
        current.instance = instance.cloned();
        current.revision += 1;
        current.changed_at = Instant::now();
    }

    pub(super) fn cancel_session_navigation(&mut self) {
        if let Some(pending) = &self.session_navigation.pending {
            pending.cancelled.store(true, Ordering::Release);
        }
        while self.session_navigation.intents.try_recv().is_ok() {}
        let mut slots = self.session_navigation.dispatch.slots.lock().unwrap();
        slots.revision += 1;
        slots.changed_at = Instant::now();
    }

    fn navigation_input_valid(&self, lease: &NavigationInputLease) -> bool {
        if !self.document_store.host_execution_enabled() {
            return false;
        }
        match lease {
            NavigationInputLease::SixLayer(generation, admission) => {
                self.six_layer_running.as_ref().is_some_and(|r| {
                    r.receiver.generation() == generation
                        && r.receiver.failure().is_none()
                        && r.admits_navigation(*admission)
                })
            }
            NavigationInputLease::Vendor(id) => self
                .device_session
                .as_ref()
                .is_some_and(|s| s.id == *id && !s.is_disconnected()),
        }
    }

    pub(super) fn cancel_physical_session_navigation(&mut self) {
        if let Some(pending) = &self.session_navigation.pending {
            if pending.input_generation.is_some() {
                pending.cancelled.store(true, Ordering::Release);
            }
        }
        while self.session_navigation.intents.try_recv().is_ok() {}
        let mut slots = self.session_navigation.dispatch.slots.lock().unwrap();
        slots.revision += 1;
        slots.changed_at = Instant::now();
    }

    pub(super) fn navigation_foreground_changed(&mut self, bundle: Option<&str>) {
        if self.foreground_bundle_id.as_deref() == bundle {
            return;
        }
        let expected = self
            .session_navigation
            .pending
            .as_ref()
            .is_some_and(|p| Some(bundle_id_for_place(&p.place)) == bundle);
        if !expected {
            self.cancel_session_navigation();
        }
    }

    pub(super) fn update_navigation_catalog(&mut self, catalog: Option<NavigationCatalog>) {
        let changed_instance = self
            .session_navigation
            .catalog
            .as_ref()
            .map(|c| &c.instance)
            != catalog.as_ref().map(|c| &c.instance);
        if changed_instance || catalog.is_none() {
            self.cancel_session_navigation();
        }
        let candidates = catalog.as_ref().map(|c| {
            c.candidates
                .iter()
                .enumerate()
                .filter(|(index, candidate)| {
                    *index < 24
                        || self.slot_table.slot_table().slots().iter().any(|slot| {
                            slot.mode == mi_model::slot_table::SlotMode::Pinned
                                && slot.place.as_ref() == Some(&candidate.place)
                        })
                })
                .map(|(_, candidate)| candidate.clone())
                .collect::<Vec<_>>()
        });
        let mut ack_changed = false;
        if let Some((place, watermark)) = catalog
            .as_ref()
            .and_then(|c| c.frontmost_acknowledged.as_ref())
        {
            let target_in_flight = self
                .session_navigation
                .pending
                .as_ref()
                .is_some_and(|p| &p.place == place);
            if !target_in_flight
                && !watermark.is_empty()
                && self
                    .lighting_arbiter
                    .acknowledged_watermarks
                    .get(&place.target_id())
                    != Some(watermark)
            {
                self.lighting_arbiter
                    .acknowledge_slot(place, watermark.clone());
                ack_changed = true;
            }
        }
        self.session_navigation.catalog = catalog;
        if let Some(candidates) = candidates {
            if self.slot_table.candidates() != candidates.as_slice() {
                self.ingest_four_app_candidates(candidates);
            } else if ack_changed {
                let acks = self.lighting_arbiter.acknowledged_watermarks.clone();
                self.slot_table.refresh_with_watermarks(&acks);
                self.refresh_navigation_revision();
                self.refresh_four_app_lighting();
                self.commit_runtime_update();
            } else {
                self.refresh_navigation_revision();
            }
        } else {
            self.refresh_navigation_revision();
        }
    }

    fn navigation_available(&self, place: &NavigationPlace) -> bool {
        if matches!(place, NavigationPlace::App { .. }) {
            return self.app_cache.resolve(bundle_id_for_place(place)).installed;
        }
        self.session_navigation.catalog.as_ref().is_some_and(|c| {
            c.observed_at.elapsed() <= CATALOG_TTL
                && matches!(place, NavigationPlace::Antigravity { .. })
                && c.candidates.iter().any(|row| &row.place == place)
        })
    }

    pub(super) fn request_navigation_command(
        &mut self,
        command: NavigationCommand,
        revision: Option<u64>,
        input_generation: Option<NavigationInputLease>,
        profile: String,
        binding: String,
    ) {
        if revision.is_some_and(|rev| {
            rev != self
                .session_navigation
                .dispatch
                .slots
                .lock()
                .unwrap()
                .revision
        }) {
            self.navigation_rejected(&profile, &binding, "blocked:session_slots_changed");
            return;
        }
        let mut history = self.navigation_history.clone();
        let place = match command {
            NavigationCommand::Slot(index) => self
                .slot_table
                .slot_table()
                .slots()
                .get(index)
                .and_then(|s| s.place.clone()),
            NavigationCommand::Back => history.back(|p| self.navigation_available(p)),
            NavigationCommand::Forward => history.forward(|p| self.navigation_available(p)),
        };
        let Some(place) = place else {
            self.navigation_rejected(&profile, &binding, "blocked:session_target_unavailable");
            return;
        };
        if matches!(command, NavigationCommand::Slot(_)) {
            history.visit(place.clone());
        }
        self.begin_session_navigation(place, history, input_generation, profile, binding);
    }

    pub(super) fn request_session_navigation(&mut self, place: NavigationPlace) {
        let mut history = self.navigation_history.clone();
        history.visit(place.clone());
        self.begin_session_navigation(
            place,
            history,
            None,
            "workbench".into(),
            "session-strip".into(),
        );
    }

    fn navigation_rejected(&mut self, profile: &str, binding: &str, detail: &str) {
        self.status_message = format!("会话未切换 · {detail}");
        self.record_host_action_receipt(navigation_receipt(
            profile, binding, "", "blocked", detail,
        ));
        self.commit_runtime_update();
    }

    fn begin_session_navigation(
        &mut self,
        place: NavigationPlace,
        history: NavigationHistory,
        input_generation: Option<NavigationInputLease>,
        profile: String,
        binding: String,
    ) {
        let fn_unconfirmed = self.live_executor.has_unconfirmed_apple_fn_release();
        self.begin_session_navigation_checked(
            place,
            history,
            input_generation,
            profile,
            binding,
            fn_unconfirmed,
        );
    }

    fn begin_session_navigation_checked(
        &mut self,
        place: NavigationPlace,
        history: NavigationHistory,
        input_generation: Option<NavigationInputLease>,
        profile: String,
        binding: String,
        fn_unconfirmed: bool,
    ) {
        if self.config.isolated && !self.has_navigation_test_worker() {
            self.navigation_rejected(
                &profile,
                &binding,
                "blocked:isolated_navigation_unavailable",
            );
            return;
        }
        if self.session_navigation.pending.is_some() {
            self.navigation_rejected(&profile, &binding, "blocked:session_navigation_busy");
            return;
        }
        if !self.navigation_available(&place) {
            self.navigation_rejected(
                &profile,
                &binding,
                "blocked:session_target_stale_or_unsupported",
            );
            return;
        }
        if input_generation
            .as_ref()
            .is_some_and(|lease| !self.navigation_input_valid(lease))
        {
            self.navigation_rejected(&profile, &binding, "blocked:session_input_retired");
            return;
        }
        if fn_unconfirmed {
            self.navigation_rejected(&profile, &binding, "blocked:unconfirmed_apple_fn_release");
            return;
        }
        let watermark = self
            .session_navigation
            .catalog
            .as_ref()
            .and_then(|catalog| catalog.candidates.iter().find(|row| row.place == place))
            .map(|c| c.watermark.clone())
            .unwrap_or_default();
        let instance = self
            .session_navigation
            .catalog
            .as_ref()
            .map(|c| c.instance.clone());
        let project_id = match &place {
            NavigationPlace::Antigravity {
                conversation_id, ..
            } => self
                .session_navigation
                .catalog
                .as_ref()
                .and_then(|c| c.project_ids.get(conversation_id))
                .cloned(),
            _ => None,
        };
        let cancelled = Arc::new(AtomicBool::new(false));
        let (tx, result) = mpsc::sync_channel(1);
        self.session_navigation.pending = Some(PendingNavigation {
            place: place.clone(),
            acknowledged_at_request: self
                .lighting_arbiter
                .acknowledged_watermarks
                .get(&place.target_id())
                .cloned(),
            watermark,
            instance: instance.clone(),
            history,
            history_base: self.navigation_history.clone(),
            input_generation,
            cancelled: cancelled.clone(),
            result,
            profile,
            binding,
        });
        #[cfg(test)]
        if let Some(worker) = self.session_navigation.test_worker.take() {
            worker(place, tx);
            return;
        }
        let executor = Arc::clone(&self.live_executor);
        std::thread::spawn(move || {
            let result = executor.with_serialized_host_effect(
                || !cancelled.load(Ordering::Acquire),
                |driver| {
                    execute_navigation(
                        driver,
                        &place,
                        instance.as_ref(),
                        project_id.as_deref(),
                        &cancelled,
                    )
                    .map(|()| place.clone())
                },
            );
            let _ = tx.send(result);
        });
        self.status_message = "正在切换会话，等待目标回读…".into();
        self.commit_runtime_update();
    }

    fn has_navigation_test_worker(&self) -> bool {
        #[cfg(test)]
        {
            self.session_navigation.test_worker.is_some()
        }
        #[cfg(not(test))]
        {
            false
        }
    }

    pub(super) fn poll_session_navigation(&mut self) {
        if self
            .session_navigation
            .catalog
            .as_ref()
            .is_some_and(|c| c.observed_at.elapsed() > CATALOG_TTL)
        {
            self.update_navigation_catalog(None);
        }
        let intents: Vec<_> = self.session_navigation.intents.try_iter().collect();
        for intent in intents {
            let current_input = self.six_layer_running.as_ref().is_some_and(|r| {
                r.receiver.generation() == intent.input_generation
                    && r.receiver.failure().is_none()
                    && r.admits_navigation(intent.admission)
            });
            if current_input
                && intent.received_at.elapsed() < Duration::from_secs(2)
                && self.system_lifecycle == SystemLifecycle::Awake
            {
                self.request_navigation_command(
                    intent.command,
                    Some(intent.revision),
                    Some(NavigationInputLease::SixLayer(
                        intent.input_generation,
                        intent.admission,
                    )),
                    intent.profile,
                    intent.binding,
                );
            }
        }
        let invalid_input = self
            .session_navigation
            .pending
            .as_ref()
            .and_then(|p| p.input_generation.as_ref())
            .is_some_and(|lease| !self.navigation_input_valid(lease));
        if invalid_input {
            self.cancel_session_navigation();
        }
        let result =
            self.session_navigation
                .pending
                .as_ref()
                .and_then(|p| match p.result.try_recv() {
                    Ok(result) => Some(result),
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Some(Err("unconfirmed:session_worker_ended".into()))
                    }
                    Err(mpsc::TryRecvError::Empty) => None,
                });
        if let Some(result) = result {
            let pending = self.session_navigation.pending.take().unwrap();
            let current_instance = self
                .session_navigation
                .catalog
                .as_ref()
                .map(|c| &c.instance);
            let confirmed = self.navigation_history == pending.history_base
                && !pending.cancelled.load(Ordering::Acquire)
                && self.system_lifecycle == SystemLifecycle::Awake
                && result
                    .as_ref()
                    .is_ok_and(|selected| selected == &pending.place)
                && (matches!(pending.place, NavigationPlace::App { .. })
                    || (current_instance == pending.instance.as_ref()
                        && self.navigation_available(&pending.place)));
            let (outcome, detail) = if confirmed {
                if !pending.watermark.is_empty()
                    && self
                        .lighting_arbiter
                        .acknowledged_watermarks
                        .get(&pending.place.target_id())
                        == pending.acknowledged_at_request.as_ref()
                {
                    self.lighting_arbiter
                        .acknowledge_slot(&pending.place, pending.watermark);
                }
                self.navigation_history = pending.history;
                let acks = self.lighting_arbiter.acknowledged_watermarks.clone();
                self.slot_table.refresh_with_watermarks(&acks);
                self.refresh_four_app_lighting();
                (
                    if self.config.isolated {
                        "simulated"
                    } else {
                        "executed"
                    },
                    "session_selection_confirmed".to_string(),
                )
            } else {
                (
                    "unconfirmed",
                    result
                        .err()
                        .unwrap_or_else(|| "session_navigation_retired".into()),
                )
            };
            self.status_message = format!(
                "{} · {detail}",
                if confirmed {
                    "已切换会话"
                } else {
                    "会话未确认"
                }
            );
            self.record_host_action_receipt(navigation_receipt(
                &pending.profile,
                &pending.binding,
                &pending.place.target_id(),
                outcome,
                &detail,
            ));
            self.commit_runtime_update();
        }
    }
}

fn navigation_receipt(
    profile: &str,
    binding: &str,
    target: &str,
    outcome: &str,
    detail: &str,
) -> HostActionReceipt {
    HostActionReceipt {
        action_type: "session_navigation".into(),
        profile_id: profile.into(),
        binding_id: binding.into(),
        target: target.into(),
        outcome: outcome.into(),
        detail: detail.into(),
        text_byte_len: None,
        timestamp_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    }
}

fn execute_navigation(
    driver: &impl HostOsDriver,
    place: &NavigationPlace,
    instance: Option<&AntigravityInstance>,
    project_id: Option<&str>,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    if cancelled.load(Ordering::Acquire) {
        return Err("blocked:session_navigation_retired".into());
    }
    if let NavigationPlace::App { bundle_id } = place {
        return driver.open_application(bundle_id);
    }
    let NavigationPlace::Antigravity {
        conversation_id,
        workspace_path,
    } = place
    else {
        return Err("blocked:session_navigation_unsupported".into());
    };
    let instance = instance.ok_or("blocked:session_catalog_unavailable")?;
    let validate = || {
        if cancelled.load(Ordering::Acquire) {
            return Err("blocked:session_navigation_retired".into());
        }
        let catalog = poll_antigravity_daemon_candidates_with_frontmost(
            &mut BTreeSet::new(),
            &mut BTreeMap::new(),
            false,
        )
        .ok_or("blocked:session_catalog_unavailable")?;
        if &catalog.instance != instance
            || !catalog.candidates.iter().any(|row| &row.place == place)
        {
            return Err("blocked:session_instance_or_workspace_changed".into());
        }
        if cancelled.load(Ordering::Acquire) {
            return Err("blocked:session_navigation_retired".into());
        }
        Ok(())
    };
    validate()?;
    driver.open_application("com.google.antigravity")?;
    mi_macos::session_navigation::navigate_antigravity_session(
        instance.pid,
        instance.ui_port,
        conversation_id,
        workspace_path,
        project_id,
        validate,
        || !cancelled.load(Ordering::Acquire),
    )
}

pub(super) fn workspace_from_summary(summary: &Value) -> Option<String> {
    let metadata = summary.get("trajectoryMetadata");
    let mut workspaces = BTreeSet::new();
    let normalize = |raw: &str| -> Option<String> {
        let Some(raw) = raw.strip_prefix("file://") else {
            return (raw.starts_with('/') && !raw.contains('\0')).then(|| raw.to_string());
        };
        let mut decoded = Vec::new();
        let bytes = raw.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'%' {
                let hex = std::str::from_utf8(bytes.get(index + 1..index + 3)?).ok()?;
                decoded.push(u8::from_str_radix(hex, 16).ok()?);
                index += 3;
            } else {
                decoded.push(bytes[index]);
                index += 1;
            }
        }
        let path = String::from_utf8(decoded).ok()?;
        (path.starts_with('/') && !path.contains('\0')).then_some(path)
    };
    for source in [
        summary.get("workspaces"),
        metadata.and_then(|m| m.get("workspaces")),
    ]
    .into_iter()
    .flatten()
    {
        for workspace in source.as_array()? {
            let fields: Vec<_> = ["workspaceFolderAbsolutePath", "workspaceFolderAbsoluteUri"]
                .into_iter()
                .filter_map(|key| workspace.get(key).and_then(Value::as_str))
                .collect();
            if fields.is_empty() {
                return None;
            }
            for raw in fields {
                workspaces.insert(normalize(raw)?);
            }
        }
    }
    if let Some(uris) = metadata.and_then(|m| m.get("workspaceUris")) {
        for uri in uris.as_array()? {
            workspaces.insert(normalize(uri.as_str()?)?);
        }
    }
    if workspaces.len() == 1 {
        workspaces.into_iter().next()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: &str, watermark: &str) -> FourAppLiveCandidate {
        FourAppLiveCandidate {
            place: NavigationPlace::antigravity(id, "/workspace"),
            title: id.into(),
            phase: FourAppSessionKind::BlueDotCompleted,
            watermark: watermark.into(),
            last_modified_ms: 5000,
            last_user_view_ms: 0,
            updated_at_ms: 5000,
        }
    }
    fn catalog(candidates: Vec<FourAppLiveCandidate>, hub: &str) -> NavigationCatalog {
        NavigationCatalog {
            instance: AntigravityInstance {
                hub: hub.into(),
                pid: 1,
                ui_port: 12,
            },
            observed_at: Instant::now(),
            candidates,
            project_ids: BTreeMap::new(),
            frontmost_acknowledged: None,
        }
    }
    fn store() -> (tempfile::TempDir, WorkbenchStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = WorkbenchStore::open_isolated(dir.path()).unwrap();
        (dir, store)
    }
    fn worker(
        store: &mut WorkbenchStore,
    ) -> mpsc::Receiver<(
        NavigationPlace,
        mpsc::SyncSender<Result<NavigationPlace, String>>,
    )> {
        let (tx, rx) = mpsc::channel();
        store.session_navigation.test_worker = Some(Box::new(move |place, reply| {
            tx.send((place, reply)).unwrap();
        }));
        rx
    }

    #[test]
    fn empty_stale_pinned_and_expired_catalog_never_acknowledge_or_visit() {
        let (_dir, mut store) = store();
        worker(&mut store);
        store
            .dispatch_session_strip_action(SessionStripAction::ActivateSlot(0))
            .unwrap();
        let row = candidate("old", "wm");
        store.update_navigation_catalog(Some(catalog(vec![row.clone()], "hub")));
        store
            .dispatch_session_strip_action(SessionStripAction::TogglePinSlot(0))
            .unwrap();
        store.update_navigation_catalog(Some(catalog(vec![], "hub")));
        store
            .dispatch_session_strip_action(SessionStripAction::ActivateSlot(0))
            .unwrap();
        assert_eq!(
            store.slot_table.slot_table().slots()[0].place,
            Some(row.place.clone())
        );
        store.update_navigation_catalog(Some(catalog(vec![row], "hub")));
        store
            .session_navigation
            .catalog
            .as_mut()
            .unwrap()
            .observed_at = Instant::now() - CATALOG_TTL - Duration::from_secs(1);
        store
            .dispatch_session_strip_action(SessionStripAction::ActivateSlot(0))
            .unwrap();
        assert!(store.navigation_history.is_empty());
        assert!(store.lighting_arbiter.acknowledged_watermarks.is_empty());
        assert!(store.session_navigation.pending.is_none());
    }

    #[test]
    fn confirmation_commits_original_target_and_watermark_once_even_if_slot_changes() {
        let (_dir, mut store) = store();
        let original = candidate("original", "wm1");
        store.update_navigation_catalog(Some(catalog(vec![original.clone()], "hub")));
        let rx = worker(&mut store);
        store
            .dispatch_session_strip_action(SessionStripAction::ActivateSlot(0))
            .unwrap();
        assert!(store.navigation_history.is_empty());
        assert!(store.lighting_arbiter.acknowledged_watermarks.is_empty());
        let (requested, reply) = rx.recv().unwrap();
        assert_eq!(requested, original.place);
        let mut newer = candidate("original", "wm2");
        newer.updated_at_ms = 6000;
        let mut replacement = candidate("replacement", "other");
        replacement.updated_at_ms = 7000;
        store.update_navigation_catalog(Some(catalog(vec![replacement.clone(), newer], "hub")));
        assert_eq!(
            store.slot_table.slot_table().slots()[0].place,
            Some(replacement.place)
        );
        reply.send(Ok(requested.clone())).unwrap();
        store.poll_session_navigation();
        store.poll_session_navigation();
        assert_eq!(store.navigation_history.entries(), &[requested.clone()]);
        assert_eq!(
            store.lighting_arbiter.acknowledged_watermarks[&requested.target_id()],
            "wm1"
        );
        assert!(store
            .slot_table
            .find_candidate(&requested)
            .unwrap()
            .is_unread_blue_dot(&store.lighting_arbiter.acknowledged_watermarks));
        assert_eq!(
            store
                .dispatched_receipts
                .iter()
                .filter(|r| r.detail == "session_selection_confirmed")
                .count(),
            1
        );
    }

    #[test]
    fn rejected_mismatched_disconnected_and_replaced_results_preserve_history() {
        for failure in 0..5 {
            let (_dir, mut store) = store();
            let row = candidate("wanted", "wm");
            store.update_navigation_catalog(Some(catalog(vec![row.clone()], "hub")));
            let prior = NavigationPlace::app("prior.app");
            store.navigation_history.visit(prior.clone());
            let rx = worker(&mut store);
            store
                .dispatch_session_strip_action(SessionStripAction::ActivateSlot(0))
                .unwrap();
            let (requested, reply) = rx.recv().unwrap();
            match failure {
                0 => reply.send(Err("rejected".into())).unwrap(),
                1 => reply
                    .send(Ok(NavigationPlace::antigravity(
                        "wanted",
                        "/other-workspace",
                    )))
                    .unwrap(),
                2 => drop(reply),
                3 => {
                    store.update_navigation_catalog(Some(catalog(vec![row], "replacement")));
                    reply.send(Ok(requested)).unwrap();
                }
                _ => {
                    store.foreground_changed(Some("other.app"));
                    reply.send(Ok(requested)).unwrap();
                }
            }
            store.poll_session_navigation();
            assert_eq!(store.navigation_history.current(), Some(&prior));
            assert!(store.lighting_arbiter.acknowledged_watermarks.is_empty());
            assert!(store.session_navigation.pending.is_none());
        }
    }

    #[test]
    fn history_cursor_moves_only_after_confirmation_and_keeps_forward_branch() {
        let (_dir, mut store) = store();
        let a = candidate("a", "a1");
        let b = candidate("b", "b1");
        store.update_navigation_catalog(Some(catalog(vec![a.clone(), b.clone()], "hub")));
        store.navigation_history.visit(a.place.clone());
        store.navigation_history.visit(b.place.clone());
        for succeeds in [false, true] {
            let rx = worker(&mut store);
            store.request_navigation_command(
                NavigationCommand::Back,
                None,
                None,
                "test".into(),
                "back".into(),
            );
            assert_eq!(store.navigation_history.current(), Some(&b.place));
            let (requested, tx) = rx.recv().unwrap();
            assert_eq!(requested, a.place);
            tx.send(if succeeds {
                Ok(requested)
            } else {
                Err("rejected".into())
            })
            .unwrap();
            store.poll_session_navigation();
        }
        assert_eq!(store.navigation_history.current(), Some(&a.place));
        assert!(store.navigation_history.can_go_forward(|_| true));
        let rx = worker(&mut store);
        store.request_navigation_command(
            NavigationCommand::Forward,
            None,
            None,
            "test".into(),
            "forward".into(),
        );
        let (requested, tx) = rx.recv().unwrap();
        tx.send(Ok(requested)).unwrap();
        store.poll_session_navigation();
        assert_eq!(store.navigation_history.current(), Some(&b.place));
        assert_eq!(store.navigation_history.len(), 2);
    }

    #[test]
    fn queued_revision_cannot_retarget_and_sleep_discards_late_success() {
        let (_dir, mut store) = store();
        let row = candidate("a", "a1");
        store.update_navigation_catalog(Some(catalog(vec![row.clone()], "hub")));
        let revision = store
            .session_navigation
            .dispatch
            .slots
            .lock()
            .unwrap()
            .revision;
        store.update_navigation_catalog(Some(catalog(
            vec![candidate("b", "b1"), row.clone()],
            "hub",
        )));
        let rx = worker(&mut store);
        store.request_navigation_command(
            NavigationCommand::Slot(0),
            Some(revision),
            None,
            "test".into(),
            "AG00".into(),
        );
        assert!(rx.try_recv().is_err());
        store.request_session_navigation(row.place.clone());
        let (_, tx) = rx.recv().unwrap();
        store.system_will_sleep();
        tx.send(Ok(row.place)).unwrap();
        store.poll_session_navigation();
        assert!(store.navigation_history.is_empty());
        assert!(store.lighting_arbiter.acknowledged_watermarks.is_empty());
    }

    #[test]
    fn full_catalog_keeps_selection_identity_beyond_the_recent_display_limit() {
        let (_dir, mut store) = store();
        let candidates: Vec<_> = (0..30)
            .map(|index| candidate(&format!("c{index:02}"), "wm"))
            .collect();
        let older = candidates[25].clone();
        store.update_navigation_catalog(Some(catalog(candidates.clone(), "hub")));
        assert_eq!(
            store
                .session_navigation
                .catalog
                .as_ref()
                .unwrap()
                .candidates
                .len(),
            30
        );
        assert_eq!(store.slot_table.candidates().len(), 24);
        assert!(store.navigation_available(&older.place));
        store
            .slot_table
            .pin_slot(0, older.place.clone(), older.title, 5000, &BTreeMap::new())
            .unwrap();
        store.update_navigation_catalog(Some(catalog(candidates, "hub")));
        assert!(store.slot_table.find_candidate(&older.place).is_some());
    }

    #[test]
    fn workspace_identity_requires_one_consistent_path_across_catalog_fields() {
        let row = json!({"workspaces": [{"workspaceFolderAbsolutePath": "/work space"}], "trajectoryMetadata": {"workspaceUris": ["file:///work%20space"]}});
        assert_eq!(workspace_from_summary(&row).as_deref(), Some("/work space"));
        assert_eq!(
            workspace_from_summary(
                &json!({"trajectoryMetadata": {"workspaceUris": ["file:///workspace"]}})
            )
            .as_deref(),
            Some("/workspace")
        );
        for row in [
            json!({}),
            json!({"workspaces": [{"workspaceFolderAbsolutePath": "/a"}, {"workspaceFolderAbsolutePath": "/b"}]}),
            json!({"workspaces": [{"workspaceFolderAbsolutePath": "/a"}], "trajectoryMetadata": {"workspaceUris": ["file:///b"]}}),
        ] {
            assert!(workspace_from_summary(&row).is_none());
        }
    }

    #[test]
    fn literal_percent_paths_are_not_uri_decoded() {
        let row = json!({"workspaces": [{"workspaceFolderAbsolutePath": "/work%20space", "workspaceFolderAbsoluteUri": "file:///work%2520space"}]});
        assert_eq!(
            workspace_from_summary(&row).as_deref(),
            Some("/work%20space")
        );
        let row = json!({"workspaces": [{"workspaceFolderAbsolutePath": "/100%"}]});
        assert_eq!(workspace_from_summary(&row).as_deref(), Some("/100%"));
    }

    #[test]
    fn newer_explicit_ack_and_concurrent_history_are_never_overwritten() {
        let (_dir, mut store) = store();
        let row = candidate("a", "wm1");
        store.update_navigation_catalog(Some(catalog(vec![row.clone()], "hub")));
        let rx = worker(&mut store);
        store.request_session_navigation(row.place.clone());
        let (_, tx) = rx.recv().unwrap();
        store.update_navigation_catalog(Some(catalog(vec![candidate("a", "wm2")], "hub")));
        store
            .dispatch_session_strip_action(SessionStripAction::AcknowledgeAllCompleted)
            .unwrap();
        tx.send(Ok(row.place.clone())).unwrap();
        store.poll_session_navigation();
        assert_eq!(
            store.lighting_arbiter.acknowledged_watermarks[&row.place.target_id()],
            "wm2"
        );
        assert_eq!(store.projection.session_strip.unread_completed_count, 0);
        let rx = worker(&mut store);
        store.request_session_navigation(row.place.clone());
        let (_, tx) = rx.recv().unwrap();
        let app = NavigationPlace::app("another.app");
        store.navigation_history.visit(app.clone());
        tx.send(Ok(row.place)).unwrap();
        store.poll_session_navigation();
        assert_eq!(store.navigation_history.current(), Some(&app));
        assert_eq!(store.navigation_history.len(), 2);
    }

    #[test]
    fn vendor_navigation_retires_on_pause_disconnect_and_input_release() {
        for change in 0..3 {
            let (_dir, mut store) = store();
            store.set_host_execution_enabled(true).unwrap();
            let row = candidate("a", "wm");
            store.update_navigation_catalog(Some(catalog(vec![row.clone()], "hub")));
            let session = {
                let _entered = store.tokio_runtime.enter();
                let (link, _) = mi_transport::MockLink::new();
                DeviceSession::new(
                    link,
                    Box::new(mi_proto::NativeFramer::default()),
                    Arc::new(mi_session::ReadOnlySchema),
                    mi_session::SessionConfig::default(),
                )
            };
            store.device_session = Some(session.clone());
            let rx = worker(&mut store);
            store.request_navigation_command(
                NavigationCommand::Slot(0),
                None,
                Some(NavigationInputLease::Vendor(session.id)),
                "legacy".into(),
                "AG00".into(),
            );
            let (_, tx) = rx.recv().unwrap();
            match change {
                0 => store.set_host_execution_enabled(false).unwrap(),
                1 => session.disconnect(),
                _ => store.release_board_inputs(Instant::now()),
            }
            tx.send(Ok(row.place)).unwrap();
            store.poll_session_navigation();
            assert!(store.navigation_history.is_empty());
            assert!(store.lighting_arbiter.acknowledged_watermarks.is_empty());
            store.tokio_runtime.block_on(session.shutdown());
        }
        let (_dir, mut store) = store();
        let row = candidate("a", "wm");
        store.update_navigation_catalog(Some(catalog(vec![row.clone()], "hub")));
        let rx = worker(&mut store);
        store.request_session_navigation(row.place.clone());
        let (_, tx) = rx.recv().unwrap();
        store.set_host_execution_enabled(false).unwrap();
        tx.send(Ok(row.place.clone())).unwrap();
        store.poll_session_navigation();
        assert_eq!(store.navigation_history.current(), Some(&row.place));
    }

    #[test]
    #[ignore = "requires a running Antigravity and an explicitly chosen session"]
    fn native_session_navigation_verifies_catalog_and_selected_url() {
        let conversation =
            std::env::var("MI_VERIFY_SESSION_ID").expect("MI_VERIFY_SESSION_ID is required");
        let catalog =
            poll_antigravity_daemon_candidates(&mut BTreeSet::new(), &mut BTreeMap::new())
                .expect("live catalog");
        let place = catalog.candidates.iter().find(|c| matches!(&c.place, NavigationPlace::Antigravity { conversation_id, .. } if conversation_id == &conversation)).expect("exact catalog target").place.clone();
        let project_id = catalog.project_ids.get(&conversation).map(String::as_str);
        execute_navigation(
            &MacOsSystemDriver::default(),
            &place,
            Some(&catalog.instance),
            project_id,
            &AtomicBool::new(false),
        )
        .unwrap();
    }

    #[test]
    fn six_layer_receiver_and_workbench_resolve_the_same_dynamic_target() {
        use mi_transport::ActivationPayload;
        use std::os::unix::net::UnixDatagram;
        let (dir, mut store) = store();
        let row = candidate("dynamic", "wm");
        store.update_navigation_catalog(Some(catalog(vec![row.clone()], "hub")));
        store.set_host_execution_enabled(true).unwrap();
        let session = {
            let _entered = store.tokio_runtime.enter();
            let (link, _) = mi_transport::MockLink::new();
            DeviceSession::new(
                link,
                Box::new(mi_proto::NativeFramer::default()),
                Arc::new(mi_session::ReadOnlySchema),
                mi_session::SessionConfig::default(),
            )
        };
        let mut running = crate::six_layer_runtime::RunningSixLayer::isolated(session, dir.path());
        running.installation.plan.hosts[0].actions[0] = Some(ResolvedHostAction::AgentCommand {
            adapter: "four-app".into(),
            command: "slot-open:0".into(),
        });
        running.navigation = Some(store.session_navigation.dispatch.clone());
        running.receiver.arm().unwrap();
        running.set_execution_policy(true, true, Instant::now() - Duration::from_secs(1));
        let receiver = running.receiver.clone();
        running.start_executor(Arc::clone(&store.live_executor));
        store.six_layer_running = Some(running);
        let rx = worker(&mut store);
        let payload =
            serde_json::to_vec(&ActivationPayload::new(receiver.generation(), 0)).unwrap();
        UnixDatagram::unbound()
            .unwrap()
            .send_to(&payload, receiver.path())
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while store.session_navigation.pending.is_none() {
            assert!(Instant::now() < deadline);
            store.poll_session_navigation();
            std::thread::sleep(Duration::from_millis(5));
        }
        let (physical, reply) = rx.recv().unwrap();
        assert_eq!(physical, row.place);
        reply.send(Ok(physical.clone())).unwrap();
        store.poll_session_navigation();
        let rx = worker(&mut store);
        store
            .dispatch_session_strip_action(SessionStripAction::ActivateSlot(0))
            .unwrap();
        let (workbench, reply) = rx.recv().unwrap();
        assert_eq!(physical, workbench);
        reply.send(Ok(workbench)).unwrap();
        store.poll_session_navigation();
        assert_eq!(store.navigation_history.entries(), &[row.place]);
        store
            .six_layer_running
            .take()
            .unwrap()
            .shutdown(&store.tokio_runtime)
            .unwrap();
    }

    #[test]
    fn watermark_ticks_with_unchanged_slot_places_keep_physical_revision_valid() {
        let (_dir, mut store) = store();
        let initial = candidate("session-a", "wm1");
        store.update_navigation_catalog(Some(catalog(vec![initial.clone()], "hub")));
        let initial_places: Vec<_> = store
            .slot_table
            .slot_table()
            .slots()
            .iter()
            .map(|slot| slot.place.clone())
            .collect();
        assert_eq!(initial_places.len(), 6);
        let (initial_revision, initial_changed_at) = {
            let slots = store.session_navigation.dispatch.slots.lock().unwrap();
            (slots.revision, slots.changed_at)
        };

        let mut updated = candidate("session-a", "wm2");
        updated.updated_at_ms = 6000;
        store.update_navigation_catalog(Some(catalog(vec![updated.clone()], "hub")));
        let after_tick_places: Vec<_> = store
            .slot_table
            .slot_table()
            .slots()
            .iter()
            .map(|slot| slot.place.clone())
            .collect();
        assert_eq!(after_tick_places, initial_places);
        let (after_tick_revision, after_tick_changed_at) = {
            let slots = store.session_navigation.dispatch.slots.lock().unwrap();
            (slots.revision, slots.changed_at)
        };
        assert_eq!(after_tick_revision, initial_revision);
        assert_eq!(after_tick_changed_at, initial_changed_at);

        let rx = worker(&mut store);
        store.request_navigation_command(
            NavigationCommand::Slot(0),
            Some(initial_revision),
            None,
            "test".into(),
            "AG00".into(),
        );
        let (requested, reply) = rx.recv().unwrap();
        assert_eq!(requested, initial.place);
        reply.send(Ok(requested)).unwrap();
        store.poll_session_navigation();
        assert_eq!(
            store.lighting_arbiter.acknowledged_watermarks[&initial.place.target_id()],
            "wm2"
        );

        let mut other = candidate("session-b", "wm3");
        other.updated_at_ms = 7000;
        store.update_navigation_catalog(Some(catalog(vec![other.clone(), updated.clone()], "hub")));
        let slot_changed_revision = store
            .session_navigation
            .dispatch
            .slots
            .lock()
            .unwrap()
            .revision;
        assert!(slot_changed_revision > initial_revision);
        let rx = worker(&mut store);
        store.request_navigation_command(
            NavigationCommand::Slot(0),
            Some(initial_revision),
            None,
            "test".into(),
            "AG00".into(),
        );
        assert!(rx.try_recv().is_err());
        assert_eq!(
            store.dispatched_receipts.last().unwrap().detail,
            "blocked:session_slots_changed"
        );

        store.update_navigation_catalog(Some(catalog(vec![other, updated], "replacement-hub")));
        let instance_changed_revision = store
            .session_navigation
            .dispatch
            .slots
            .lock()
            .unwrap()
            .revision;
        assert!(instance_changed_revision > slot_changed_revision);
        let rx = worker(&mut store);
        store.request_navigation_command(
            NavigationCommand::Slot(0),
            Some(slot_changed_revision),
            None,
            "test".into(),
            "AG00".into(),
        );
        assert!(rx.try_recv().is_err());
        assert_eq!(
            store.dispatched_receipts.last().unwrap().detail,
            "blocked:session_slots_changed"
        );
    }

    #[test]
    fn classify_antigravity_trajectories_matches_jetski_web_pk_rk_and_qcb_rules() {
        let payload = json!({
            "busy-status": {
                "summary": "Busy Run",
                "status": "CASCADE_RUN_STATUS_BUSY",
                "lastModifiedTime": "2026-04-19T12:00:00Z",
                "workspaces": [{"workspaceFolderAbsolutePath": "/work/repo"}],
                "trajectoryMetadata": {"projectId": "  proj-1  "}
            },
            "running-subagents": {
                "summary": "Delegating",
                "status": "CASCADE_RUN_STATUS_IDLE",
                "hasRunningSubagents": true,
                "lastModifiedTime": "2026-04-19T12:01:00Z",
                "workspaces": [{"workspaceFolderAbsolutePath": "/work/repo"}]
            },
            "not-fully-idle": {
                "summary": "Draining",
                "status": "CASCADE_RUN_STATUS_IDLE",
                "notFullyIdle": true,
                "lastModifiedTime": "2026-04-19T12:02:00Z",
                "workspaces": [{"workspaceFolderAbsolutePath": "/work/repo"}]
            },
            "waiting-steps": {
                "summary": "Needs Approval",
                "status": "CASCADE_RUN_STATUS_RUNNING",
                "waitingSteps": [{"stepIndex": 4}],
                "lastModifiedTime": "2026-04-19T12:03:00Z",
                "workspaces": [{"workspaceFolderAbsolutePath": "/work/repo"}]
            },
            "stale-unviewed-blue-dot": {
                "summary": "Finished Two Hours Ago",
                "status": "CASCADE_RUN_STATUS_IDLE",
                "latestExecutorTerminationReason": "EXECUTOR_TERMINATION_REASON_USER_STOPPED",
                "lastModifiedTime": "2026-04-19T08:00:00Z",
                "annotations": {"lastUserViewTime": "2026-04-19T07:30:00Z"},
                "workspaces": [{"workspaceFolderAbsolutePath": "/work/repo"}]
            },
            "marked-unread": {
                "summary": "Explicitly Unread",
                "status": "CASCADE_RUN_STATUS_IDLE",
                "lastModifiedTime": "2026-04-19T09:00:00Z",
                "annotations": {
                    "lastUserViewTime": "2026-04-19T09:30:00Z",
                    "markedAsUnread": true
                },
                "workspaces": [{"workspaceFolderAbsolutePath": "/work/repo"}]
            },
            "frontmost-open": {
                "summary": "Currently Viewed In Antigravity",
                "status": "CASCADE_RUN_STATUS_IDLE",
                "lastModifiedTime": "2026-04-19T11:59:00Z",
                "annotations": {"lastUserViewTime": "2026-04-19T10:00:00Z"},
                "workspaces": [{"workspaceFolderAbsolutePath": "/work/repo"}]
            },
            "errored-term": {
                "summary": "Executor Error",
                "status": "CASCADE_RUN_STATUS_IDLE",
                "latestExecutorTerminationReason": "EXECUTOR_TERMINATION_REASON_INTERNAL_ERROR",
                "lastModifiedTime": "2026-04-19T11:00:00Z",
                "workspaces": [{"workspaceFolderAbsolutePath": "/work/repo"}]
            },
            "killed-running": {
                "summary": "Killed Run Viewed",
                "status": "CASCADE_RUN_STATUS_RUNNING",
                "killed": true,
                "lastModifiedTime": "2026-04-19T10:00:00Z",
                "annotations": {"lastUserViewTime": "2026-04-19T10:01:00Z"},
                "workspaces": [{"workspaceFolderAbsolutePath": "/work/repo"}]
            },
            "archived-conv": {
                "summary": "Archived",
                "status": "CASCADE_RUN_STATUS_IDLE",
                "lastModifiedTime": "2026-04-19T12:00:00Z",
                "annotations": {"archived": true},
                "workspaces": [{"workspaceFolderAbsolutePath": "/work/repo"}]
            },
            "battle-fork": {
                "summary": "Arena Fork",
                "status": "CASCADE_RUN_STATUS_RUNNING",
                "lastModifiedTime": "2026-04-19T12:00:00Z",
                "trajectoryMetadata": {"isBattleModeFork": true},
                "workspaces": [{"workspaceFolderAbsolutePath": "/work/repo"}]
            },
            "side-question": {
                "summary": "Quick Side Question",
                "status": "CASCADE_RUN_STATUS_IDLE",
                "trajectoryType": "TRAJECTORY_TYPE_SIDE_QUESTION",
                "lastModifiedTime": "2026-04-19T12:00:00Z",
                "workspaces": [{"workspaceFolderAbsolutePath": "/work/repo"}]
            },
            "commit-gen": {
                "summary": "Commit Message",
                "status": "CASCADE_RUN_STATUS_IDLE",
                "trajectoryMetadata": {"trajectoryType": "CASCADE_COMMIT_MESSAGE_GENERATION"},
                "lastModifiedTime": "2026-04-19T12:00:00Z",
                "workspaces": [{"workspaceFolderAbsolutePath": "/work/repo"}]
            },
            "hidden-source": {
                "summary": "Hidden Automation",
                "status": "CASCADE_RUN_STATUS_IDLE",
                "trajectoryMetadata": {"sourceMetadata": {"hideFromConversationList": true}},
                "lastModifiedTime": "2026-04-19T12:00:00Z",
                "workspaces": [{"workspaceFolderAbsolutePath": "/work/repo"}]
            },
            "subagent-spec": {
                "summary": "Worker",
                "status": "CASCADE_RUN_STATUS_RUNNING",
                "trajectoryMetadata": {"subagentSpec": {"role": "builder"}},
                "lastModifiedTime": "2026-04-19T12:00:00Z",
                "workspaces": [{"workspaceFolderAbsolutePath": "/work/repo"}]
            }
        });

        let mut seen_running = BTreeSet::new();
        let mut completed_watermarks = BTreeMap::new();
        let (candidates, project_ids, frontmost_ack) = classify_antigravity_trajectory_summaries(
            payload.as_object().unwrap(),
            Some("frontmost-open"),
            &mut seen_running,
            &mut completed_watermarks,
        );
        assert_eq!(
            frontmost_ack,
            Some((
                NavigationPlace::antigravity("frontmost-open", "/work/repo"),
                "CASCADE_RUN_STATUS_IDLE:2026-04-19T11:59:00Z:".to_string(),
            ))
        );

        assert_eq!(
            project_ids.get("busy-status").map(String::as_str),
            Some("proj-1")
        );
        let by_id: BTreeMap<String, &FourAppLiveCandidate> = candidates
            .iter()
            .filter_map(|c| match &c.place {
                NavigationPlace::Antigravity {
                    conversation_id, ..
                } => Some((conversation_id.clone(), c)),
                _ => None,
            })
            .collect();

        for excluded in [
            "archived-conv",
            "battle-fork",
            "side-question",
            "commit-gen",
            "hidden-source",
            "subagent-spec",
        ] {
            assert!(!by_id.contains_key(excluded), "{excluded} must be excluded");
        }

        assert_eq!(by_id["busy-status"].phase, FourAppSessionKind::Working);
        assert_eq!(
            by_id["running-subagents"].phase,
            FourAppSessionKind::Working
        );
        assert_eq!(by_id["not-fully-idle"].phase, FourAppSessionKind::Working);
        assert_eq!(
            by_id["waiting-steps"].phase,
            FourAppSessionKind::NeedsAttention
        );
        assert_eq!(by_id["errored-term"].phase, FourAppSessionKind::Error);
        assert_eq!(by_id["killed-running"].phase, FourAppSessionKind::Idle);

        let stale = by_id["stale-unviewed-blue-dot"];
        assert_eq!(stale.phase, FourAppSessionKind::BlueDotCompleted);
        assert!(stale.is_unread_blue_dot(&BTreeMap::new()));

        let unread = by_id["marked-unread"];
        assert_eq!(unread.phase, FourAppSessionKind::BlueDotCompleted);
        assert!(unread.is_unread_blue_dot(&BTreeMap::new()));

        let frontmost = by_id["frontmost-open"];
        assert_eq!(frontmost.phase, FourAppSessionKind::Idle);
        assert!(!frontmost.is_unread_blue_dot(&BTreeMap::new()));

        let (_, _, unread_frontmost_ack) = classify_antigravity_trajectory_summaries(
            payload.as_object().unwrap(),
            Some("marked-unread"),
            &mut BTreeSet::new(),
            &mut BTreeMap::new(),
        );
        assert!(unread_frontmost_ack.is_none());

        let (_dir, mut store) = store();
        let mut viewed_cat = catalog(candidates.clone(), "/tmp/hub");
        viewed_cat.frontmost_acknowledged = frontmost_ack;
        store.update_navigation_catalog(Some(viewed_cat));

        let (away_candidates, _, away_ack) = classify_antigravity_trajectory_summaries(
            payload.as_object().unwrap(),
            None,
            &mut BTreeSet::new(),
            &mut BTreeMap::new(),
        );
        assert!(away_ack.is_none());
        let away_frontmost = away_candidates
            .iter()
            .find(|c| c.place == NavigationPlace::antigravity("frontmost-open", "/work/repo"))
            .unwrap()
            .clone();
        assert_eq!(away_frontmost.phase, FourAppSessionKind::BlueDotCompleted);
        store.update_navigation_catalog(Some(catalog(away_candidates, "/tmp/hub")));
        assert!(!away_frontmost.is_unread_blue_dot(&store.lighting_arbiter.acknowledged_watermarks));
        assert!(!store
            .projection()
            .session_strip
            .completed_blue_dot_pills
            .iter()
            .any(|p| p.title == "Currently Viewed"));
    }

    #[test]
    fn begin_session_navigation_rejects_when_unconfirmed_apple_fn_release_is_pending() {
        let (_dir, mut store) = store();
        let row = candidate("conv-fn", "wm-fn");
        store.update_navigation_catalog(Some(catalog(vec![row.clone()], "/tmp/hub")));
        worker(&mut store);

        let mut history = store.navigation_history.clone();
        history.visit(row.place.clone());
        store.begin_session_navigation_checked(
            row.place.clone(),
            history,
            None,
            "workbench".into(),
            "session-strip".into(),
            true,
        );
        assert!(store.session_navigation.pending.is_none());
        assert!(store
            .status_message
            .contains("blocked:unconfirmed_apple_fn_release"));
    }
}
