use super::state::{ExecutionAdmission, ExecutionPolicy};
use super::{activation_to_input_event, RoutingContext};
use crate::store::session_navigation::{
    NavigationCommand, NavigationDispatch, PhysicalNavigationIntent,
};
use crate::store::LiveSessionSource;
use mi_model::apply::AppliedSixLayerPlan;
use mi_model::{
    direct_controls::{CONTROL_BINDINGS, DIRECT_CONTROLS},
    host_router::ResolvedHostAction,
};
use mi_transport::UserCommandReceiver;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Mutex,
    },
    thread,
    time::{Duration, Instant},
};

fn navigation_source_for_dispatch(
    context: &RoutingContext,
    control_layer: u8,
    profile_id: &str,
    action: &ResolvedHostAction,
) -> Option<LiveSessionSource> {
    if matches!(
        action,
        ResolvedHostAction::AgentCommand { adapter, command }
            if adapter == "navigation" && matches!(command.as_str(), "back" | "forward")
    ) {
        return None;
    }
    if matches!(action, ResolvedHostAction::AgentCommand { adapter, .. } if adapter == "codex")
        || profile_id == "codex"
    {
        return Some(LiveSessionSource::Codex);
    }
    match &context.mode {
        crate::six_layer_runtime::SixLayerRoutingMode::BoardLayers { .. } => {
            if control_layer == 1 {
                Some(LiveSessionSource::Codex)
            } else {
                Some(LiveSessionSource::Antigravity)
            }
        }
        crate::six_layer_runtime::SixLayerRoutingMode::Locked(pinned)
        | crate::six_layer_runtime::SixLayerRoutingMode::ManualPinned(pinned) => {
            if pinned.bundle_id.as_deref() == Some("com.openai.codex") {
                Some(LiveSessionSource::Codex)
            } else {
                Some(LiveSessionSource::Antigravity)
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(super) fn execute_clicks(
    receiver: &UserCommandReceiver,
    plan: &AppliedSixLayerPlan,
    stop: &AtomicBool,
    policy: &ExecutionPolicy,
    observed: &AtomicU8,
    first: &AtomicBool,
    routing: &Mutex<RoutingContext>,
    receipts: &std::sync::mpsc::SyncSender<mi_macos::HostActionReceipt>,
    navigation: Option<&NavigationDispatch>,
    mut next: impl FnMut() -> Result<Option<mi_transport::Activation>, String>,
    mut check_connection: impl FnMut() -> Result<(), String>,
    mut execute: impl FnMut(
        &str,
        &str,
        &ResolvedHostAction,
        ExecutionAdmission,
    ) -> mi_macos::HostActionReceipt,
) {
    while !stop.load(Ordering::Acquire) {
        let event = match next() {
            Ok(Some(event)) => event,
            Ok(None) => {
                thread::sleep(Duration::from_millis(5));
                continue;
            }
            Err(_) => return,
        };
        let admission = policy.capture(event.received_at);
        if event.generation != receiver.generation() {
            continue;
        }
        if event.released {
            continue;
        }
        let control = &DIRECT_CONTROLS[event.control as usize];
        let context = routing.lock().unwrap().clone();
        let control_layer = event.layer_override.unwrap_or(control.layer);
        observed.store(control_layer, Ordering::Release);
        first.store(true, Ordering::Release);
        let Some((profile_id, action)) =
            context.resolve_control(plan, control_layer, control.slot as usize)
        else {
            continue;
        };
        if !policy.admits(admission, action) {
            continue;
        }
        if let Err(error) = check_connection() {
            receiver.invalidate(&error);
            return;
        }
        if stop.load(Ordering::Acquire) || receiver.failure().is_some() {
            return;
        }
        if event.is_expired_at(Instant::now()) {
            continue;
        }
        if let (Some(command), Some(dispatch)) =
            (NavigationCommand::from_action(action), navigation)
        {
            if !policy.admits(admission, action) {
                continue;
            }
            let snapshot = dispatch.slots.lock().unwrap().clone();
            let source =
                navigation_source_for_dispatch(&context, control_layer, profile_id, action);
            let revision = match source {
                Some(src) => snapshot.revision_for_source(src),
                None => snapshot.revision,
            };
            if dispatch
                .tx
                .try_send(PhysicalNavigationIntent {
                    command,
                    source,
                    revision,
                    input_generation: event.generation.clone(),
                    admission,
                    received_at: event.received_at,
                    profile: profile_id.to_owned(),
                    binding: CONTROL_BINDINGS[control.slot as usize].into(),
                })
                .is_err()
            {
                receiver.invalidate("会话导航积压，输入已暂停");
                return;
            }
            continue;
        }
        let receipt = execute(
            profile_id,
            CONTROL_BINDINGS[control.slot as usize],
            action,
            admission,
        );
        if receipts.try_send(receipt).is_err() {
            receiver.invalidate("执行回执积压，输入已暂停");
            return;
        }
    }
}

struct PendingSixLayerEncoderHold {
    hold_generation: u64,
    control_layer: u8,
    profile_id: String,
    hold_action: ResolvedHostAction,
    adapter: Option<String>,
    target_overlay_kind: Option<crate::encoder_model_picker::EncoderOverlayKind>,
    admission: ExecutionAdmission,
    input_generation: String,
}

#[allow(clippy::too_many_arguments)]
fn dispatch_hold_action(
    picker: &mut crate::encoder_model_picker::EncoderModelPicker,
    overlay_kind: Option<crate::encoder_model_picker::EncoderOverlayKind>,
    profile_id: &str,
    hold_action: &ResolvedHostAction,
    action_adapter: Option<String>,
    admission: ExecutionAdmission,
    control_layer: u8,
    at: Instant,
    routing: &Mutex<RoutingContext>,
    receipts: &std::sync::mpsc::SyncSender<mi_macos::HostActionReceipt>,
    receiver: &UserCommandReceiver,
    active_picker_context: &mut Option<(u8, String, Option<String>)>,
    mouse_capture_enabled: &mut bool,
    set_mouse_capture: &mut dyn FnMut(bool),
    execute: &mut dyn FnMut(
        &str,
        &str,
        &ResolvedHostAction,
        ExecutionAdmission,
    ) -> mi_macos::HostActionReceipt,
) -> bool {
    let must_esc = picker.prepare_for_hold_action(overlay_kind);
    if must_esc {
        if let Some(ref adapter) = action_adapter {
            let cancel_action = crate::encoder_model_picker::ModelPickerCommand::ModelPickerCancel
                .into_host_action(adapter.clone());
            let _ = execute(profile_id, "ENC_CLK", &cancel_action, admission);
        }
    }
    if overlay_kind != Some(crate::encoder_model_picker::EncoderOverlayKind::ModelPicker)
        && *mouse_capture_enabled
    {
        *mouse_capture_enabled = false;
        set_mouse_capture(false);
    }
    let receipt = execute(profile_id, "ENC_CLK", hold_action, admission);
    let executed_ok = receipt.outcome == "executed";
    if receipts.try_send(receipt).is_err() {
        if *mouse_capture_enabled {
            *mouse_capture_enabled = false;
            set_mouse_capture(false);
        }
        receiver.invalidate("执行回执积压，输入已暂停");
        return false;
    }
    let adapter = action_adapter.unwrap_or_else(|| "antigravity".to_string());
    let transition = picker.apply_hold_execution_outcome(adapter, overlay_kind, executed_ok, at);
    match transition {
        crate::encoder_model_picker::OverlayTransition::Entered(kind) => {
            let current_bundle = routing.lock().unwrap().bundle_id.clone();
            *active_picker_context = Some((control_layer, profile_id.to_string(), current_bundle));
            let captures = kind.captures_mouse();
            if *mouse_capture_enabled != captures {
                *mouse_capture_enabled = captures;
                set_mouse_capture(captures);
            }
        }
        crate::encoder_model_picker::OverlayTransition::Cancelled
        | crate::encoder_model_picker::OverlayTransition::None => {
            *active_picker_context = None;
            if *mouse_capture_enabled {
                *mouse_capture_enabled = false;
                set_mouse_capture(false);
            }
        }
    }
    true
}

#[allow(clippy::too_many_arguments)]
pub(super) fn execute_clicks_with_encoder_picker(
    receiver: &UserCommandReceiver,
    plan: &AppliedSixLayerPlan,
    stop: &AtomicBool,
    policy: &ExecutionPolicy,
    observed: &AtomicU8,
    first: &AtomicBool,
    routing: &Mutex<RoutingContext>,
    receipts: &std::sync::mpsc::SyncSender<mi_macos::HostActionReceipt>,
    navigation: Option<&NavigationDispatch>,
    mut next: impl FnMut() -> Result<Option<mi_transport::Activation>, String>,
    mut check_connection: impl FnMut() -> Result<(), String>,
    mut set_mouse_capture: impl FnMut(bool),
    mut drain_mouse_deltas: impl FnMut() -> Vec<(f64, f64)>,
    mut execute: impl FnMut(
        &str,
        &str,
        &ResolvedHostAction,
        ExecutionAdmission,
    ) -> mi_macos::HostActionReceipt,
) {
    use crate::encoder_model_picker::{
        ConfirmExecutionDisposition, EncoderHoldTimerOutcome, EncoderModelPicker,
        EncoderOverlayKind, EncoderReleaseOutcome, EncoderTurnOutcome, OverlayTransition,
        ENCODER_HOLD_THRESHOLD,
    };

    let mut picker = EncoderModelPicker::new();
    let mut mouse_capture_enabled = false;
    let mut pending_hold: Option<PendingSixLayerEncoderHold> = None;
    let mut active_picker_context: Option<(u8, String, Option<String>)> = None;

    let set_capture_state =
        |enabled: bool, flag: &mut bool, set_mouse_capture: &mut dyn FnMut(bool)| {
            if *flag != enabled {
                *flag = enabled;
                set_mouse_capture(enabled);
            }
        };

    while !stop.load(Ordering::Acquire) {
        let loop_now = Instant::now();
        if !policy.state.lock().unwrap().enabled {
            if picker.hold_state().is_some()
                || picker.active_session().is_some()
                || pending_hold.is_some()
                || mouse_capture_enabled
            {
                picker.cancel();
                pending_hold = None;
                active_picker_context = None;
                set_capture_state(false, &mut mouse_capture_enabled, &mut set_mouse_capture);
            }
        } else {
            if let Some((active_layer, ref active_profile, ref active_bundle)) =
                active_picker_context
            {
                let (current_eff_profile, current_bundle) = {
                    let ctx = routing.lock().unwrap();
                    (
                        ctx.effective_profile_id(plan, active_layer),
                        ctx.bundle_id.clone(),
                    )
                };
                if current_eff_profile.as_deref() != Some(active_profile.as_str())
                    || &current_bundle != active_bundle
                {
                    picker.cancel();
                    pending_hold = None;
                    active_picker_context = None;
                    set_capture_state(false, &mut mouse_capture_enabled, &mut set_mouse_capture);
                }
            }
            if picker.expire_if_idle(loop_now).is_some() {
                active_picker_context = None;
                set_capture_state(false, &mut mouse_capture_enabled, &mut set_mouse_capture);
            }
            let due_hold = pending_hold.as_ref().is_some_and(|ctx| {
                picker.hold_state().is_some_and(|hold| {
                    hold.hold_generation == ctx.hold_generation
                        && !hold.hold_fired
                        && !hold.turned_while_pressed
                        && loop_now.saturating_duration_since(hold.pressed_at)
                            >= ENCODER_HOLD_THRESHOLD
                })
            });
            if due_hold {
                let hold_ctx = pending_hold.take().unwrap();
                if hold_ctx.input_generation == receiver.generation() {
                    if let EncoderHoldTimerOutcome::FiredHold { overlay_kind, .. } = picker
                        .on_hold_timer_with_picker_mode(
                            hold_ctx.hold_generation,
                            hold_ctx.adapter.as_deref(),
                            true,
                            hold_ctx.target_overlay_kind,
                            loop_now,
                        )
                    {
                        let hold_action = hold_ctx.hold_action;
                        if policy.admits(hold_ctx.admission, &hold_action) {
                            if let Err(error) = check_connection() {
                                set_capture_state(
                                    false,
                                    &mut mouse_capture_enabled,
                                    &mut set_mouse_capture,
                                );
                                receiver.invalidate(&error);
                                return;
                            }
                            if stop.load(Ordering::Acquire) || receiver.failure().is_some() {
                                set_capture_state(
                                    false,
                                    &mut mouse_capture_enabled,
                                    &mut set_mouse_capture,
                                );
                                return;
                            }
                            if !dispatch_hold_action(
                                &mut picker,
                                overlay_kind,
                                &hold_ctx.profile_id,
                                &hold_action,
                                hold_ctx.adapter,
                                hold_ctx.admission,
                                hold_ctx.control_layer,
                                loop_now,
                                routing,
                                receipts,
                                receiver,
                                &mut active_picker_context,
                                &mut mouse_capture_enabled,
                                &mut set_mouse_capture,
                                &mut execute,
                            ) {
                                return;
                            }
                        } else {
                            picker.cancel();
                            active_picker_context = None;
                            set_capture_state(
                                false,
                                &mut mouse_capture_enabled,
                                &mut set_mouse_capture,
                            );
                        }
                    }
                } else {
                    picker.cancel();
                    active_picker_context = None;
                    set_capture_state(false, &mut mouse_capture_enabled, &mut set_mouse_capture);
                }
            }

            if picker.is_active(loop_now) {
                for (dx, dy) in drain_mouse_deltas() {
                    if let Some(cmd) = picker.on_mouse_delta(dx, dy, loop_now) {
                        let Some(adapter) = picker.active_adapter(loop_now).map(str::to_owned)
                        else {
                            continue;
                        };
                        let Some((_, ref profile_id, _)) = active_picker_context else {
                            continue;
                        };
                        let flick_admission = policy.capture(loop_now);
                        let action = cmd.into_host_action(adapter);
                        if !policy.admits(flick_admission, &action) {
                            continue;
                        }
                        if let Err(error) = check_connection() {
                            set_capture_state(
                                false,
                                &mut mouse_capture_enabled,
                                &mut set_mouse_capture,
                            );
                            receiver.invalidate(&error);
                            return;
                        }
                        if stop.load(Ordering::Acquire) || receiver.failure().is_some() {
                            set_capture_state(
                                false,
                                &mut mouse_capture_enabled,
                                &mut set_mouse_capture,
                            );
                            return;
                        }
                        let receipt = execute(profile_id, "ENC_CLK", &action, flick_admission);
                        if receipts.try_send(receipt).is_err() {
                            set_capture_state(
                                false,
                                &mut mouse_capture_enabled,
                                &mut set_mouse_capture,
                            );
                            receiver.invalidate("执行回执积压，输入已暂停");
                            return;
                        }
                    }
                }
            }
        }

        let event = match next() {
            Ok(Some(event)) => event,
            Ok(None) => {
                thread::sleep(Duration::from_millis(5));
                continue;
            }
            Err(_) => {
                set_capture_state(false, &mut mouse_capture_enabled, &mut set_mouse_capture);
                return;
            }
        };
        let admission = policy.capture(event.received_at);
        if event.generation != receiver.generation() {
            continue;
        }
        let Some(input_event) = activation_to_input_event(&event, 0) else {
            continue;
        };
        let control = &DIRECT_CONTROLS[event.control as usize];
        let binding = CONTROL_BINDINGS[control.slot as usize];
        let context = routing.lock().unwrap().clone();
        let control_layer = event.layer_override.unwrap_or(control.layer);
        observed.store(control_layer, Ordering::Release);
        first.store(true, Ordering::Release);

        let current_eff_profile = context.effective_profile_id(plan, control_layer);
        if let Some((active_layer, ref active_profile, _)) = active_picker_context {
            if active_layer != control_layer
                || current_eff_profile.as_deref() != Some(active_profile.as_str())
            {
                picker.cancel();
                pending_hold = None;
                active_picker_context = None;
                set_capture_state(false, &mut mouse_capture_enabled, &mut set_mouse_capture);
            }
        }

        let layer_adapter = context.resolve_layer_adapter(plan, control_layer);
        let resolved_hold = context.resolve_encoder_hold(plan, control_layer);

        if binding == "ENC_CLK" {
            match input_event.kind {
                mi_session::InputEventKind::Pressed => {
                    if event.is_expired_at(Instant::now()) {
                        continue;
                    }
                    if resolved_hold.is_some() || picker.is_active(event.received_at) {
                        let hold_gen = picker.on_encoder_press(0, event.received_at);
                        pending_hold = resolved_hold.map(|(prof_id, hold_action)| {
                            let target_overlay_kind = EncoderOverlayKind::from_action(&hold_action);
                            let action_adapter = match &hold_action {
                                ResolvedHostAction::AgentCommand { adapter, .. }
                                    if matches!(
                                        adapter.as_str(),
                                        "antigravity" | "codex" | "dsh-desktop"
                                    ) =>
                                {
                                    Some(adapter.clone())
                                }
                                _ => layer_adapter.map(|(_, a)| a.to_owned()),
                            };
                            PendingSixLayerEncoderHold {
                                hold_generation: hold_gen,
                                control_layer,
                                profile_id: prof_id,
                                hold_action,
                                adapter: action_adapter,
                                target_overlay_kind,
                                admission,
                                input_generation: event.generation.clone(),
                            }
                        });
                        continue;
                    }
                }
                mi_session::InputEventKind::Released => {
                    if picker.hold_state().is_none() {
                        pending_hold = None;
                        continue;
                    }
                    let saved_hold = pending_hold.take();
                    let hold_entry = saved_hold
                        .map(|h| {
                            (
                                h.profile_id,
                                h.hold_action,
                                h.adapter,
                                h.target_overlay_kind,
                            )
                        })
                        .or_else(|| {
                            resolved_hold.map(|(prof_id, act)| {
                                let target_kind = EncoderOverlayKind::from_action(&act);
                                let ad = match &act {
                                    ResolvedHostAction::AgentCommand { adapter, .. }
                                        if matches!(
                                            adapter.as_str(),
                                            "antigravity" | "codex" | "dsh-desktop"
                                        ) =>
                                    {
                                        Some(adapter.clone())
                                    }
                                    _ => layer_adapter.map(|(_, a)| a.to_owned()),
                                };
                                (prof_id, act, ad, target_kind)
                            })
                        });
                    let release_adapter = hold_entry
                        .as_ref()
                        .and_then(|(_, _, ad, _)| ad.as_deref())
                        .or_else(|| layer_adapter.map(|(_, a)| a));
                    let has_hold_action = hold_entry.is_some();
                    let target_overlay_kind = hold_entry.as_ref().and_then(|(_, _, _, k)| *k);
                    match picker.on_encoder_release_with_picker_mode(
                        0,
                        release_adapter,
                        has_hold_action,
                        target_overlay_kind,
                        event.received_at,
                    ) {
                        EncoderReleaseOutcome::Consumed => continue,
                        EncoderReleaseOutcome::FiredHold { overlay_kind, .. } => {
                            let Some((profile_id, hold_action, action_adapter, _)) = hold_entry
                            else {
                                continue;
                            };
                            if !policy.admits(admission, &hold_action) {
                                picker.cancel();
                                active_picker_context = None;
                                set_capture_state(
                                    false,
                                    &mut mouse_capture_enabled,
                                    &mut set_mouse_capture,
                                );
                                continue;
                            }
                            if let Err(error) = check_connection() {
                                set_capture_state(
                                    false,
                                    &mut mouse_capture_enabled,
                                    &mut set_mouse_capture,
                                );
                                receiver.invalidate(&error);
                                return;
                            }
                            if stop.load(Ordering::Acquire) || receiver.failure().is_some() {
                                set_capture_state(
                                    false,
                                    &mut mouse_capture_enabled,
                                    &mut set_mouse_capture,
                                );
                                return;
                            }
                            if !dispatch_hold_action(
                                &mut picker,
                                overlay_kind,
                                &profile_id,
                                &hold_action,
                                action_adapter,
                                admission,
                                control_layer,
                                event.received_at,
                                routing,
                                receipts,
                                receiver,
                                &mut active_picker_context,
                                &mut mouse_capture_enabled,
                                &mut set_mouse_capture,
                                &mut execute,
                            ) {
                                return;
                            }
                            continue;
                        }
                        EncoderReleaseOutcome::ModelPickerConfirm { adapter, command } => {
                            let profile_id = active_picker_context
                                .take()
                                .map(|(_, p, _)| p)
                                .or_else(|| layer_adapter.map(|(p, _)| p.to_owned()))
                                .or_else(|| current_eff_profile.clone());
                            let Some(profile_id) = profile_id else {
                                set_capture_state(
                                    false,
                                    &mut mouse_capture_enabled,
                                    &mut set_mouse_capture,
                                );
                                continue;
                            };
                            let confirm_action = command.into_host_action(adapter);
                            if !policy.admits(admission, &confirm_action) {
                                set_capture_state(
                                    false,
                                    &mut mouse_capture_enabled,
                                    &mut set_mouse_capture,
                                );
                                continue;
                            }
                            if let Err(error) = check_connection() {
                                set_capture_state(
                                    false,
                                    &mut mouse_capture_enabled,
                                    &mut set_mouse_capture,
                                );
                                receiver.invalidate(&error);
                                return;
                            }
                            if stop.load(Ordering::Acquire) || receiver.failure().is_some() {
                                set_capture_state(
                                    false,
                                    &mut mouse_capture_enabled,
                                    &mut set_mouse_capture,
                                );
                                return;
                            }
                            let receipt =
                                execute(&profile_id, "ENC_CLK", &confirm_action, admission);
                            set_capture_state(
                                false,
                                &mut mouse_capture_enabled,
                                &mut set_mouse_capture,
                            );
                            let disposition = picker.apply_confirm_execution_outcome(
                                receipt.succeeded(),
                                &receipt.detail,
                            );
                            if disposition != ConfirmExecutionDisposition::FallbackToShortPress {
                                if receipts.try_send(receipt).is_err() {
                                    receiver.invalidate("执行回执积压，输入已暂停");
                                    return;
                                }
                                continue;
                            }
                        }
                        EncoderReleaseOutcome::ShortPress => {}
                    }
                }
                _ => continue,
            }
        } else if event.released {
            continue;
        }

        if matches!(binding, "AG13" | "AG14") {
            let clockwise = binding == "AG14";
            match picker.on_encoder_turn(clockwise, event.received_at) {
                EncoderTurnOutcome::NormalSlot => {}
                EncoderTurnOutcome::FastScroll { clockwise } => {
                    pending_hold = None;
                    let (profile_id, adapter) = match layer_adapter {
                        Some((p, a)) => (p.to_owned(), a.to_owned()),
                        None => match current_eff_profile.clone() {
                            Some(p) => (p, "host".to_owned()),
                            None => continue,
                        },
                    };
                    let scroll_action = ResolvedHostAction::AgentCommand {
                        adapter,
                        command: if clockwise {
                            "scroll-down-fast".into()
                        } else {
                            "scroll-up-fast".into()
                        },
                    };
                    if !policy.admits(admission, &scroll_action) {
                        continue;
                    }
                    if let Err(error) = check_connection() {
                        set_capture_state(
                            false,
                            &mut mouse_capture_enabled,
                            &mut set_mouse_capture,
                        );
                        receiver.invalidate(&error);
                        return;
                    }
                    if stop.load(Ordering::Acquire) || receiver.failure().is_some() {
                        set_capture_state(
                            false,
                            &mut mouse_capture_enabled,
                            &mut set_mouse_capture,
                        );
                        return;
                    }
                    let receipt = execute(&profile_id, binding, &scroll_action, admission);
                    if receipts.try_send(receipt).is_err() {
                        set_capture_state(
                            false,
                            &mut mouse_capture_enabled,
                            &mut set_mouse_capture,
                        );
                        receiver.invalidate("执行回执积压，输入已暂停");
                        return;
                    }
                    continue;
                }
                EncoderTurnOutcome::ModelPicker(cmd) => {
                    let Some(adapter) = picker.active_adapter(event.received_at).map(str::to_owned)
                    else {
                        continue;
                    };
                    let Some(profile_id) = active_picker_context
                        .as_ref()
                        .map(|(_, p, _)| p.clone())
                        .or_else(|| layer_adapter.map(|(p, _)| p.to_owned()))
                    else {
                        continue;
                    };
                    let nav_action = cmd.into_host_action(adapter);
                    if !policy.admits(admission, &nav_action) {
                        continue;
                    }
                    if let Err(error) = check_connection() {
                        set_capture_state(
                            false,
                            &mut mouse_capture_enabled,
                            &mut set_mouse_capture,
                        );
                        receiver.invalidate(&error);
                        return;
                    }
                    if stop.load(Ordering::Acquire) || receiver.failure().is_some() {
                        set_capture_state(
                            false,
                            &mut mouse_capture_enabled,
                            &mut set_mouse_capture,
                        );
                        return;
                    }
                    let receipt = execute(&profile_id, binding, &nav_action, admission);
                    if receipts.try_send(receipt).is_err() {
                        set_capture_state(
                            false,
                            &mut mouse_capture_enabled,
                            &mut set_mouse_capture,
                        );
                        receiver.invalidate("执行回执积压，输入已暂停");
                        return;
                    }
                    continue;
                }
            }
        } else if matches!(binding, "AG15" | "AG16" | "AG17" | "AG18") {
            if let Some(cmd) = picker.on_sector_direction_while_active(binding, event.received_at) {
                let Some(adapter) = picker.active_adapter(event.received_at).map(str::to_owned)
                else {
                    continue;
                };
                let Some(profile_id) = active_picker_context
                    .as_ref()
                    .map(|(_, p, _)| p.clone())
                    .or_else(|| layer_adapter.map(|(p, _)| p.to_owned()))
                else {
                    continue;
                };
                let sector_action = cmd.into_host_action(adapter);
                if !policy.admits(admission, &sector_action) {
                    continue;
                }
                if let Err(error) = check_connection() {
                    set_capture_state(false, &mut mouse_capture_enabled, &mut set_mouse_capture);
                    receiver.invalidate(&error);
                    return;
                }
                if stop.load(Ordering::Acquire) || receiver.failure().is_some() {
                    set_capture_state(false, &mut mouse_capture_enabled, &mut set_mouse_capture);
                    return;
                }
                let receipt = execute(&profile_id, binding, &sector_action, admission);
                if receipts.try_send(receipt).is_err() {
                    set_capture_state(false, &mut mouse_capture_enabled, &mut set_mouse_capture);
                    receiver.invalidate("执行回执积压，输入已暂停");
                    return;
                }
                continue;
            }
        }

        let Some((profile_id, action)) =
            context.resolve_control(plan, control_layer, control.slot as usize)
        else {
            continue;
        };
        if picker.is_active(event.received_at) {
            picker.cancel();
            pending_hold = None;
            active_picker_context = None;
            set_capture_state(false, &mut mouse_capture_enabled, &mut set_mouse_capture);
        }
        if !policy.admits(admission, action) {
            continue;
        }
        if let Err(error) = check_connection() {
            set_capture_state(false, &mut mouse_capture_enabled, &mut set_mouse_capture);
            receiver.invalidate(&error);
            return;
        }
        if stop.load(Ordering::Acquire) || receiver.failure().is_some() {
            set_capture_state(false, &mut mouse_capture_enabled, &mut set_mouse_capture);
            return;
        }
        if event.is_expired_at(Instant::now()) && binding != "ENC_CLK" {
            continue;
        }
        if let (Some(command), Some(dispatch)) =
            (NavigationCommand::from_action(action), navigation)
        {
            if !policy.admits(admission, action) {
                continue;
            }
            let snapshot = dispatch.slots.lock().unwrap().clone();
            let source =
                navigation_source_for_dispatch(&context, control_layer, profile_id, action);
            let revision = match source {
                Some(src) => snapshot.revision_for_source(src),
                None => snapshot.revision,
            };
            if dispatch
                .tx
                .try_send(PhysicalNavigationIntent {
                    command,
                    source,
                    revision,
                    input_generation: event.generation.clone(),
                    admission,
                    received_at: event.received_at,
                    profile: profile_id.to_owned(),
                    binding: binding.into(),
                })
                .is_err()
            {
                set_capture_state(false, &mut mouse_capture_enabled, &mut set_mouse_capture);
                receiver.invalidate("会话导航积压，输入已暂停");
                return;
            }
            continue;
        }
        let receipt = execute(profile_id, binding, action, admission);
        if receipt.succeeded() {
            let resolved_adapter = match action {
                ResolvedHostAction::AgentCommand { adapter, .. }
                    if matches!(adapter.as_str(), "host" | "active-agent") =>
                {
                    layer_adapter.map(|(_, a)| a).unwrap_or("antigravity")
                }
                ResolvedHostAction::AgentCommand { adapter, .. } => adapter.as_str(),
                _ => layer_adapter.map(|(_, a)| a).unwrap_or("antigravity"),
            };
            let transition = picker.activate_overlay_after_action_with_detail(
                resolved_adapter,
                action,
                &receipt.detail,
                event.received_at,
            );
            match transition {
                OverlayTransition::Entered(kind) => {
                    let current_bundle = routing.lock().unwrap().bundle_id.clone();
                    active_picker_context =
                        Some((control_layer, profile_id.to_owned(), current_bundle));
                    set_capture_state(
                        kind.captures_mouse(),
                        &mut mouse_capture_enabled,
                        &mut set_mouse_capture,
                    );
                }
                OverlayTransition::Cancelled => {
                    active_picker_context = None;
                    set_capture_state(false, &mut mouse_capture_enabled, &mut set_mouse_capture);
                }
                OverlayTransition::None => {}
            }
        }
        if receipts.try_send(receipt).is_err() {
            set_capture_state(false, &mut mouse_capture_enabled, &mut set_mouse_capture);
            receiver.invalidate("执行回执积压，输入已暂停");
            return;
        }
    }
    set_capture_state(false, &mut mouse_capture_enabled, &mut set_mouse_capture);
}
