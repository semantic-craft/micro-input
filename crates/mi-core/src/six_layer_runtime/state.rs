use crate::karabiner::{self, ConfigPatch};
use mi_model::apply::AppliedSixLayerPlan;
use mi_model::host_router::ResolvedHostAction;
use mi_transport::TargetKeyboard;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Instant,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum InstallationStatus {
    Applying,
    Applied,
    RecoveryRequired,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Installation {
    pub status: InstallationStatus,
    pub plan: AppliedSixLayerPlan,
    pub target: TargetKeyboard,
    pub profile_index: u32,
    pub backup: PathBuf,
    pub config: ConfigPatch,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedSixLayerState {
    pub version: u8,
    #[serde(
        default,
        rename = "cindy_native_paused_confirmed",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) legacy_native_paused_confirmation: Option<bool>,
    pub installation: Option<Installation>,
}

impl SavedSixLayerState {
    pub fn migrate_saved_encoder_defaults(
        &mut self,
        doc: &mi_model::document::LocalDocumentV2,
    ) -> bool {
        let Some(ref mut inst) = self.installation else {
            return false;
        };
        let mut changed = false;
        for host in &mut inst.plan.hosts {
            if !matches!(
                host.adapter.as_str(),
                "antigravity" | "codex" | "dsh-desktop"
            ) {
                continue;
            }
            let Some(profile) = doc.editable.graph.profiles.get(&host.profile_id) else {
                continue;
            };
            let Some(first_lid) = profile.layer_order.first() else {
                continue;
            };
            let Some(layer) = doc.editable.graph.layers.get(first_lid) else {
                continue;
            };
            if doc.is_protected_oai_layer(first_lid) {
                continue;
            }

            let hold_action_id = layer
                .control_settings
                .as_ref()
                .and_then(|cs| cs.encoder_hold_action_id.as_ref());
            let hold_action = hold_action_id.and_then(|aid| doc.editable.graph.actions.get(aid));

            let slot_action = |slot: &str| {
                layer
                    .controls
                    .values()
                    .find(|b| b.logical_control == slot)
                    .and_then(|b| b.action_id.as_ref())
                    .and_then(|id| doc.editable.graph.actions.get(id))
            };
            let is_action_cmd =
                |action: Option<&mi_model::draft::Action>, expected_id: &str, cmd: &str| -> bool {
                    action.is_some_and(|a| {
                        a.id == expected_id
                            && a.payload.get("command").and_then(serde_json::Value::as_str)
                                == Some(cmd)
                    })
                };
            let d0 = slot_action("logical-0");
            let d1 = slot_action("logical-1");
            let d2 = slot_action("logical-2");
            let d3 = slot_action("logical-3");
            let d4 = slot_action("logical-4");
            let d5 = slot_action("logical-5");
            let doc_has_v2_top_six = is_action_cmd(d0, "agent-subagent", "subagent")
                && is_action_cmd(d1, "host-mouse-click-left", "mouse-click:left")
                && d2.is_none()
                && d3.is_none()
                && d4.is_none()
                && d5.is_none();
            if doc_has_v2_top_six && host.actions.len() >= 6 {
                let b0 = host.actions[0].as_ref();
                let b1 = host.actions[1].as_ref();
                let already_v2 = matches!(
                    b0,
                    Some(ResolvedHostAction::AgentCommand { command, .. }) if command == "subagent"
                ) && matches!(
                    b1,
                    Some(ResolvedHostAction::AgentCommand { adapter, command })
                        if adapter == "host" && command == "mouse-click:left"
                ) && host.actions[2..6].iter().all(Option::is_none);
                let baked_session_top_six = (0..6).all(|slot| {
                    matches!(
                        host.actions[slot].as_ref(),
                        Some(ResolvedHostAction::AgentCommand { command, .. })
                            if command == &format!("slot-open:{slot}")
                    )
                });
                let baked_legacy_starter_top_six =
                    matches!(b0, Some(ResolvedHostAction::OpenApp { .. }))
                        && matches!(
                            b1,
                            Some(ResolvedHostAction::AgentCommand { command, .. })
                                if command == "focus-input"
                        );
                if !already_v2 && (baked_session_top_six || baked_legacy_starter_top_six) {
                    host.actions[0] = Some(ResolvedHostAction::AgentCommand {
                        adapter: host.adapter.clone(),
                        command: "subagent".into(),
                    });
                    host.actions[1] = Some(ResolvedHostAction::AgentCommand {
                        adapter: "host".into(),
                        command: "mouse-click:left".into(),
                    });
                    for item in host.actions.iter_mut().take(6).skip(2) {
                        *item = None;
                    }
                    changed = true;
                }
            }
            let doc_has_session_top_six = (0..6usize).all(|slot| {
                let logical = format!("logical-{slot}");
                is_action_cmd(
                    slot_action(&logical),
                    &format!("slot-open-{slot}"),
                    &format!("slot-open:{slot}"),
                )
            });
            if matches!(host.adapter.as_str(), "antigravity" | "codex")
                && doc_has_session_top_six
                && host.actions.len() >= 6
            {
                let b0 = host.actions[0].as_ref();
                let b1 = host.actions[1].as_ref();
                let baked_v2_top_six = matches!(
                    b0,
                    Some(ResolvedHostAction::AgentCommand { command, .. }) if command == "subagent"
                ) && matches!(
                    b1,
                    Some(ResolvedHostAction::AgentCommand { adapter, command })
                        if adapter == "host" && command == "mouse-click:left"
                ) && host.actions[2..6].iter().all(Option::is_none);
                let baked_legacy_starter_top_six =
                    matches!(b0, Some(ResolvedHostAction::OpenApp { .. }))
                        && matches!(
                            b1,
                            Some(ResolvedHostAction::AgentCommand { command, .. })
                                if command == "focus-input"
                        );
                if baked_v2_top_six || baked_legacy_starter_top_six {
                    for (slot, item) in host.actions.iter_mut().enumerate().take(6) {
                        *item = Some(ResolvedHostAction::AgentCommand {
                            adapter: "four-app".into(),
                            command: format!("slot-open:{slot}"),
                        });
                    }
                    changed = true;
                }
            }
            if host.adapter == "antigravity" && host.actions.len() > 7 {
                let d7 = slot_action("logical-7");
                if is_action_cmd(d7, "ag-archive-conversation", "archive-conversation") {
                    let baked_focus_input = matches!(
                        host.actions[7].as_ref(),
                        Some(ResolvedHostAction::AgentCommand { adapter, command })
                            if adapter == "antigravity" && command == "focus-input"
                    );
                    if baked_focus_input {
                        host.actions[7] = Some(ResolvedHostAction::AgentCommand {
                            adapter: "antigravity".into(),
                            command: "archive-conversation".into(),
                        });
                        changed = true;
                    }
                }
            }
            if layer
                .control_settings
                .as_ref()
                .is_some_and(|cs| cs.joystick_mode == "mouse" && cs.active_sectors == 0)
            {
                if host.joystick_mode.as_deref() != Some("MOUSE") {
                    host.joystick_mode = Some("MOUSE".into());
                    changed = true;
                }
                if host.actions.len() >= 20 && host.actions[16..20].iter().any(Option::is_some) {
                    for item in host.actions.iter_mut().skip(16).take(4) {
                        *item = None;
                    }
                    changed = true;
                }
            } else if host.adapter == "antigravity"
                && layer.control_settings.as_ref().is_some_and(|cs| {
                    cs.joystick_mode == "logical_sectors" && cs.active_sectors == 4
                })
            {
                let d16 = slot_action("logical-16");
                let d17 = slot_action("logical-17");
                let d18 = slot_action("logical-18");
                let d19 = slot_action("logical-19");
                let doc_has_canonical_joystick =
                    is_action_cmd(d16, "ag-sidebar-scroll-up", "sidebar-scroll-up")
                        && is_action_cmd(d17, "ag-open-side-panel", "open-side-panel")
                        && is_action_cmd(d18, "ag-sidebar-scroll-down", "sidebar-scroll-down")
                        && is_action_cmd(d19, "ag-close-side-panel", "close-side-panel");
                if host.actions.len() >= 20 {
                    if doc_has_canonical_joystick {
                        let baked_mouse_joystick = host.joystick_mode.as_deref() == Some("MOUSE")
                            && host.actions[16..20].iter().all(|action| {
                                action.as_ref().is_none_or(|a| {
                                    matches!(
                                        a,
                                        ResolvedHostAction::AgentCommand { command, .. }
                                            if command.starts_with("model-picker-")
                                    )
                                })
                            });
                        let baked_legacy_nav_joystick = matches!(
                            host.actions[16].as_ref(),
                            Some(ResolvedHostAction::AgentCommand { command, .. })
                                if command == "scroll-up"
                        ) && matches!(
                            host.actions[17].as_ref(),
                            Some(ResolvedHostAction::Shortcut { key, .. }) if key == "["
                        ) && matches!(
                            host.actions[18].as_ref(),
                            Some(ResolvedHostAction::AgentCommand { command, .. })
                                if command == "scroll-down"
                        ) && matches!(
                            host.actions[19].as_ref(),
                            Some(ResolvedHostAction::Shortcut { key, .. }) if key == "]"
                        );
                        if baked_mouse_joystick || baked_legacy_nav_joystick {
                            host.joystick_mode = None;
                            host.actions[16] = Some(ResolvedHostAction::AgentCommand {
                                adapter: "antigravity".into(),
                                command: "sidebar-scroll-up".into(),
                            });
                            host.actions[17] = Some(ResolvedHostAction::AgentCommand {
                                adapter: "antigravity".into(),
                                command: "open-side-panel".into(),
                            });
                            host.actions[18] = Some(ResolvedHostAction::AgentCommand {
                                adapter: "antigravity".into(),
                                command: "sidebar-scroll-down".into(),
                            });
                            host.actions[19] = Some(ResolvedHostAction::AgentCommand {
                                adapter: "antigravity".into(),
                                command: "close-side-panel".into(),
                            });
                            changed = true;
                        }
                    }
                    if is_action_cmd(d16, "ag-sidebar-scroll-up", "sidebar-scroll-up")
                        && matches!(
                            host.actions[16].as_ref(),
                            Some(ResolvedHostAction::AgentCommand { adapter, command })
                                if adapter == "antigravity" && command == "scroll-up"
                        )
                    {
                        host.actions[16] = Some(ResolvedHostAction::AgentCommand {
                            adapter: "antigravity".into(),
                            command: "sidebar-scroll-up".into(),
                        });
                        changed = true;
                    }
                    if is_action_cmd(d18, "ag-sidebar-scroll-down", "sidebar-scroll-down")
                        && matches!(
                            host.actions[18].as_ref(),
                            Some(ResolvedHostAction::AgentCommand { adapter, command })
                                if adapter == "antigravity" && command == "scroll-down"
                        )
                    {
                        host.actions[18] = Some(ResolvedHostAction::AgentCommand {
                            adapter: "antigravity".into(),
                            command: "sidebar-scroll-down".into(),
                        });
                        changed = true;
                    }
                }
            }

            let d13 = slot_action("logical-13");
            let d14 = slot_action("logical-14");
            let d15 = slot_action("logical-15");

            let is_current_doc = is_action_cmd(d13, "agent-model-picker-prev", "model-picker-prev")
                && is_action_cmd(d14, "agent-model-picker-next", "model-picker-next")
                && is_action_cmd(d15, "agent-search", "search")
                && is_action_cmd(hold_action, "agent-search", "search");

            let is_legacy_doc = mi_model::preset::coding_agent_layer_needs_encoder_migration(
                doc,
                &profile.name,
                first_lid,
            );

            if !is_current_doc && !is_legacy_doc {
                continue;
            }

            let is_host_cmd = |action: Option<&ResolvedHostAction>, expected_cmd: &str| -> bool {
                match action {
                    Some(ResolvedHostAction::AgentCommand { command, .. }) => {
                        command == expected_cmd
                    }
                    _ => false,
                }
            };
            let b13 = host.actions.get(13).and_then(|a| a.as_ref());
            let b14 = host.actions.get(14).and_then(|a| a.as_ref());
            let b15 = host.actions.get(15).and_then(|a| a.as_ref());
            let b_hold = host.encoder_hold_action.as_ref();

            let baked_pre_v080 = (b_hold.is_none() || is_host_cmd(b_hold, "search"))
                && ((is_host_cmd(b13, "scroll-up")
                    && is_host_cmd(b14, "scroll-down")
                    && is_host_cmd(b15, "scroll-to-bottom"))
                    || (is_host_cmd(b13, "prev-conversation")
                        && is_host_cmd(b14, "next-conversation")
                        && is_host_cmd(b15, "search"))
                    || (is_host_cmd(b13, "cycle-attention-prev")
                        && is_host_cmd(b14, "cycle-attention-next")
                        && is_host_cmd(b15, "four-app-attention")));

            let baked_v080 = is_host_cmd(b13, "model-picker-prev")
                && is_host_cmd(b14, "model-picker-next")
                && is_host_cmd(b15, "model-picker-confirm")
                && is_host_cmd(b_hold, "model-picker-open");

            let baked_v090 = is_host_cmd(b13, "model-picker-prev")
                && is_host_cmd(b14, "model-picker-next")
                && is_host_cmd(b15, "model-picker-open")
                && is_host_cmd(b_hold, "search");

            if baked_pre_v080 || baked_v080 || baked_v090 {
                let adapter = host.adapter.clone();
                host.actions[13] = Some(ResolvedHostAction::AgentCommand {
                    adapter: adapter.clone(),
                    command: "model-picker-prev".into(),
                });
                host.actions[14] = Some(ResolvedHostAction::AgentCommand {
                    adapter: adapter.clone(),
                    command: "model-picker-next".into(),
                });
                host.actions[15] = Some(ResolvedHostAction::AgentCommand {
                    adapter: adapter.clone(),
                    command: "search".into(),
                });
                host.encoder_hold_action = Some(ResolvedHostAction::AgentCommand {
                    adapter,
                    command: "search".into(),
                });
                changed = true;
            }
        }
        changed
    }
}

impl Default for SavedSixLayerState {
    fn default() -> Self {
        Self {
            version: 1,
            legacy_native_paused_confirmation: None,
            installation: None,
        }
    }
}

const MAX_RUNTIME_STATE_BYTES: u64 = 1024 * 1024;

pub fn load(path: &Path) -> Result<SavedSixLayerState, String> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        match mi_model::private_fs::verify_existing_private_dir(parent) {
            Ok(_) => {}
            Err(mi_model::private_fs::PrivateDirError::Missing) => {
                return Ok(SavedSixLayerState::default());
            }
            Err(err) => {
                return Err(format!("六层运行记录目录不安全：{err:?}"));
            }
        }
    }
    let Some(file) = mi_model::private_fs::open_verified_readonly_file_optional(path)? else {
        return Ok(SavedSixLayerState::default());
    };
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if meta.len() > MAX_RUNTIME_STATE_BYTES {
        return Err("六层运行记录文件超过大小上限".into());
    }
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    use std::io::Read;
    file.take(MAX_RUNTIME_STATE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_RUNTIME_STATE_BYTES {
        return Err("六层运行记录文件超过大小上限".into());
    }
    let saved: SavedSixLayerState = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if saved.version != 1 {
        return Err("不支持此六层运行记录版本".into());
    }
    Ok(saved)
}

pub fn save(path: &Path, state: &SavedSixLayerState) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?;
    if let Ok(expected) = fs::read(path) {
        return karabiner::replace_if_unchanged(path, &expected, &bytes);
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        fs::File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[derive(Default)]
pub(crate) struct ExecutionPolicy {
    pub(super) state: Mutex<ExecutionPolicyState>,
}

#[derive(Default)]
pub(super) struct ExecutionPolicyState {
    pub(super) enabled: bool,
    pub(super) commands: bool,
    pub(super) epoch: u64,
    pub(super) admit_after: Option<Instant>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ExecutionAdmission {
    pub(super) epoch: u64,
    pub(super) received_at: Instant,
}

impl ExecutionPolicy {
    pub(super) fn set(&self, enabled: bool, commands: bool, now: Instant) {
        let mut state = self.state.lock().unwrap();
        if state.enabled != enabled || state.commands != commands {
            state.enabled = enabled;
            state.commands = commands;
            state.epoch += 1;
            state.admit_after = Some(now);
        }
    }

    pub(super) fn capture(&self, received_at: Instant) -> ExecutionAdmission {
        ExecutionAdmission {
            epoch: self.state.lock().unwrap().epoch,
            received_at,
        }
    }

    pub(super) fn admits(
        &self,
        admission: ExecutionAdmission,
        action: &ResolvedHostAction,
    ) -> bool {
        let state = self.state.lock().unwrap();
        state.epoch == admission.epoch
            && state.enabled
            && state
                .admit_after
                .is_none_or(|after| admission.received_at >= after)
            && (!matches!(action, ResolvedHostAction::ShellCommand { .. }) || state.commands)
    }
}

pub const SLOTS_PER_LAYER: usize = 20;
pub type HostAction = ResolvedHostAction;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedProfilePlan {
    pub profile_id: String,
    pub bundle_id: Option<String>,
    pub actions: [Option<HostAction>; SLOTS_PER_LAYER],
    pub encoder_hold_action: Option<HostAction>,
}

impl PinnedProfilePlan {
    pub fn has_any_action(&self) -> bool {
        self.actions.iter().any(Option::is_some) || self.encoder_hold_action.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SixLayerRoutingMode {
    Locked(Box<PinnedProfilePlan>),
    ManualPinned(Box<PinnedProfilePlan>),
    BoardLayers {
        layer_one: Option<Box<PinnedProfilePlan>>,
        layer_two: Option<Box<PinnedProfilePlan>>,
        layer_three: Option<Box<PinnedProfilePlan>>,
    },
}

impl Default for SixLayerRoutingMode {
    fn default() -> Self {
        Self::BoardLayers {
            layer_one: None,
            layer_two: None,
            layer_three: None,
        }
    }
}
