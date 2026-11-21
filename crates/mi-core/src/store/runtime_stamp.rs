use super::*;
use crate::encoder_model_picker::EncoderOverlayKind;
use mi_macos::ScreenRect;
use mi_model::slot_table::{SlotMode, SlotStatus};

#[derive(Clone, Copy)]
pub(crate) struct Fnv1a(u64);

impl Fnv1a {
    pub(crate) const fn new() -> Self {
        Self(0xcbf29ce484222325)
    }

    pub(crate) fn write_bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
    }

    pub(crate) fn write_u8(&mut self, v: u8) {
        self.write_bytes(&[v]);
    }

    pub(crate) fn write_bool(&mut self, v: bool) {
        self.write_u8(u8::from(v));
    }

    pub(crate) fn write_u16(&mut self, v: u16) {
        self.write_bytes(&v.to_le_bytes());
    }

    pub(crate) fn write_u64(&mut self, v: u64) {
        self.write_bytes(&v.to_le_bytes());
    }

    pub(crate) fn write_f64(&mut self, v: f64) {
        let bits = if v == 0.0 { 0 } else { v.to_bits() };
        self.write_u64(bits);
    }

    pub(crate) fn write_str(&mut self, s: &str) {
        self.write_u64(s.len() as u64);
        self.write_bytes(s.as_bytes());
    }

    pub(crate) fn write_opt_str(&mut self, s: Option<&str>) {
        match s {
            Some(val) => {
                self.write_u8(1);
                self.write_str(val);
            }
            None => self.write_u8(0),
        }
    }

    pub(crate) fn write_rect(&mut self, rect: ScreenRect) {
        self.write_f64(rect.x);
        self.write_f64(rect.y);
        self.write_f64(rect.width);
        self.write_f64(rect.height);
    }

    pub(crate) fn write_candidate(&mut self, c: &mi_transport::HidCandidate) {
        self.write_str(&c.path);
        self.write_opt_str(c.serial_number.as_deref());
        self.write_str(&c.product);
        self.write_u16(c.vid);
        self.write_u16(c.pid);
    }

    pub(crate) fn write_place(&mut self, place: &NavigationPlace) {
        match place {
            NavigationPlace::App { bundle_id } => {
                self.write_u8(0);
                self.write_str(bundle_id);
            }
            NavigationPlace::Session { adapter, session } => {
                self.write_u8(1);
                self.write_str(adapter);
                self.write_str(session);
            }
            NavigationPlace::Antigravity {
                conversation_id,
                workspace_path,
            } => {
                self.write_u8(2);
                self.write_str(conversation_id);
                self.write_str(workspace_path);
            }
            NavigationPlace::Dsh {
                session_id,
                workspace_id,
                host_id,
            } => {
                self.write_u8(3);
                self.write_str(session_id);
                self.write_str(workspace_id);
                self.write_str(host_id);
            }
            NavigationPlace::Codex {
                thread_id,
                host_id,
                workspace_path,
            } => {
                self.write_u8(4);
                self.write_str(thread_id);
                self.write_str(host_id);
                self.write_str(workspace_path);
            }
        }
    }

    pub(crate) const fn finish(self) -> u64 {
        self.0
    }
}

pub(crate) fn fnv1a_str(s: &str) -> u64 {
    let mut h = Fnv1a::new();
    h.write_str(s);
    h.finish()
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct RuntimeCommitStamp {
    draft_revision: u64,
    host_revision: u64,
    device_connected: bool,
    usb_candidates_hash: u64,
    hardware_rpc_verified: bool,
    six_layer_busy: bool,
    six_layer_present: bool,
    six_layer_user_pinned: bool,
    ptt_active: bool,
    board_routing: BoardRouting,
    radial_state_hash: u64,
    radial_readiness: RadialInputReadiness,
    overlay_state_hash: u64,
    screen_geometry_hash: u64,
    last_cursor_position: ScreenPoint,
    accessibility_trusted: bool,
    competing_input_app_running: bool,
    can_retry_host_cleanup: bool,
    host_caution_hash: u64,
    busy: bool,
    status_menu_open: bool,
    banner_flash_count: u64,
    selected_binding_slot: Option<u32>,
    selected_surface: ControlSurfaceId,
    selected_joystick_direction: JoystickDirection8,
    bottom_row_layout: BottomRowLayout,
    command_execution_enabled: bool,
    firmware_busy: bool,
    firmware_reserved: bool,
    status_message_hash: u64,
    firmware_phase_hash: u64,
    firmware_message_hash: u64,
    v070_text_hash: u64,
    encoder_picker_hash: u64,
    navigation_pending: bool,
    navigation_catalog_mask: u8,
    navigation_history_hash: u64,
    slot_table_hash: u64,
    lighting_arbiter_hash: u64,
    lighting_preview_active: bool,
    lighting_preview_remaining_secs: u8,
    lighting_modal_validation_error_hash: u64,
    board_config_hash: u64,
    slot_role_overrides_hash: u64,
}

impl WorkbenchStore {
    fn usb_candidates_hash(&self) -> u64 {
        let mut h = Fnv1a::new();
        match &self.connected_candidate {
            Some(c) => {
                h.write_u8(1);
                h.write_candidate(c);
            }
            None => h.write_u8(0),
        }
        h.write_u64(self.available_usb_candidates.len() as u64);
        for c in &self.available_usb_candidates {
            h.write_candidate(c);
        }
        h.finish()
    }

    fn radial_state_hash(&self) -> u64 {
        let mut h = Fnv1a::new();
        h.write_bool(self.radial_state.enabled());
        h.write_u8(match self.radial_state.confirm_mode() {
            RadialConfirmMode::EncoderPress => 0,
            RadialConfirmMode::SpringBack => 1,
        });
        match self.radial_state.session() {
            None => h.write_u8(0),
            Some(session) => {
                h.write_u8(1);
                h.write_u64(session.input_session);
                match session.selected {
                    Some(sector) => {
                        h.write_u8(1);
                        h.write_u8(sector.as_u8());
                    }
                    None => h.write_u8(0),
                }
                h.write_rect(session.anchor_frame);
                h.write_u64(session.route.document_revision);
                h.write_u64(session.route.host_revision);
                h.write_opt_str(session.route.profile_id.as_deref());
                h.write_str(&session.route.profile_name);
            }
        }
        h.finish()
    }

    fn screen_geometry_hash(&self) -> u64 {
        let mut h = Fnv1a::new();
        h.write_rect(self.screen_geometry.main_visible_frame);
        h.write_u64(self.screen_geometry.screens.len() as u64);
        for rect in &self.screen_geometry.screens {
            h.write_rect(*rect);
        }
        h.finish()
    }

    fn v070_text_hash(&self) -> u64 {
        let mut h = Fnv1a::new();
        h.write_str(&self.v070_hardware.projection.status_message);
        h.write_opt_str(self.v070_hardware.projection.last_error.as_deref());
        h.finish()
    }

    fn encoder_picker_hash(&self) -> u64 {
        let mut h = Fnv1a::new();
        h.write_bool(self.model_picker_mouse_capture_enabled);
        match self.encoder_model_picker.hold_state() {
            None => h.write_u8(0),
            Some(hold) => {
                h.write_u8(1);
                h.write_u64(hold.session_id);
                h.write_u64(hold.hold_generation);
                h.write_bool(hold.turned_while_pressed);
                h.write_bool(hold.hold_fired);
            }
        }
        match self.encoder_model_picker.active_session() {
            None => h.write_u8(0),
            Some(active) => {
                h.write_u8(1);
                h.write_str(&active.adapter);
                h.write_u8(match active.kind {
                    EncoderOverlayKind::ModelPicker => 0,
                    EncoderOverlayKind::SearchConversations => 1,
                    EncoderOverlayKind::SubagentPicker => 2,
                });
            }
        }
        h.finish()
    }

    fn navigation_history_hash(&self) -> u64 {
        let mut h = Fnv1a::new();
        match self.navigation_history.index() {
            Some(idx) => {
                h.write_u8(1);
                h.write_u64(idx as u64);
            }
            None => h.write_u8(0),
        }
        h.write_u64(self.navigation_history.entries().len() as u64);
        for place in self.navigation_history.entries() {
            h.write_place(place);
        }
        h.finish()
    }

    fn slot_table_hash(&self) -> u64 {
        fn write_table(h: &mut Fnv1a, table: &FourAppSlotTable) {
            let slots = table.slot_table().slots();
            h.write_u64(slots.len() as u64);
            for slot in slots {
                h.write_u64(slot.slot_index as u64);
                h.write_u8(match slot.mode {
                    SlotMode::Pinned => 0,
                    SlotMode::Recent => 1,
                });
                match &slot.place {
                    Some(place) => {
                        h.write_u8(1);
                        h.write_place(place);
                    }
                    None => h.write_u8(0),
                }
                h.write_str(&slot.title);
                h.write_u8(match slot.status {
                    SlotStatus::Idle => 0,
                    SlotStatus::Running => 1,
                    SlotStatus::NeedsInteraction => 2,
                    SlotStatus::Unread => 3,
                    SlotStatus::Error => 4,
                });
                h.write_u64(slot.pinned_at.unwrap_or(u64::MAX));
                h.write_u64(slot.updated_at.unwrap_or(u64::MAX));
            }
            let candidates = table.candidates();
            h.write_u64(candidates.len() as u64);
            for cand in candidates {
                h.write_place(&cand.place);
                h.write_str(&cand.title);
                h.write_u8(match cand.phase {
                    FourAppSessionKind::NeedsAttention => 0,
                    FourAppSessionKind::Error => 1,
                    FourAppSessionKind::BlueDotCompleted => 2,
                    FourAppSessionKind::Working => 3,
                    FourAppSessionKind::Idle => 4,
                });
                h.write_str(&cand.watermark);
                h.write_u64(cand.last_modified_ms);
                h.write_u64(cand.last_user_view_ms);
                h.write_u64(cand.updated_at_ms);
            }
        }
        let mut h = Fnv1a::new();
        h.write_u8(match self.desired_live_session_source() {
            LiveSessionSource::Antigravity => 0,
            LiveSessionSource::Codex => 1,
        });
        write_table(&mut h, &self.slot_table);
        write_table(&mut h, &self.codex_slot_table);
        h.finish()
    }

    fn lighting_arbiter_hash(&self) -> u64 {
        let mut h = Fnv1a::new();
        h.write_u64(self.lighting_arbiter.acknowledged_watermarks.len() as u64);
        for (target, watermark) in &self.lighting_arbiter.acknowledged_watermarks {
            h.write_str(target);
            h.write_str(watermark);
        }
        h.write_u64(self.lighting_arbiter.full_keypad_feedback.len() as u64);
        for key in &self.lighting_arbiter.full_keypad_feedback {
            h.write_str(&key.binding_id);
            h.write_str(&key.color);
            h.write_str(key.phase.label());
            h.write_str(&key.label);
        }
        match &self.lighting_arbiter.dual_zone_feedback {
            None => h.write_u8(0),
            Some(dz) => {
                h.write_u8(1);
                h.write_str(&dz.backlight_color);
                h.write_str(&dz.backlight_effect);
                h.write_f64(dz.backlight_brightness);
                h.write_f64(dz.backlight_speed);
                h.write_str(&dz.underglow_color);
                h.write_str(&dz.underglow_effect);
                h.write_f64(dz.underglow_brightness);
                h.write_f64(dz.underglow_speed);
            }
        }
        h.write_bool(self.lighting_arbiter.hardware_synced);
        h.write_opt_str(self.lighting_arbiter.last_synced_payload_digest.as_deref());
        h.finish()
    }

    fn board_config_hash(&self) -> u64 {
        let mut h = Fnv1a::new();
        match self.document_store.board_layer_plan() {
            None => h.write_u8(0),
            Some(plan) => {
                h.write_u8(1);
                for layer in BoardLayer::ALL {
                    match plan.route(layer) {
                        BoardLayerRoute::Unassigned => h.write_u8(0),
                        BoardLayerRoute::HostProfile { profile_id } => {
                            h.write_u8(1);
                            h.write_str(profile_id);
                        }
                        BoardLayerRoute::Native { owner, profile_id } => {
                            h.write_u8(2);
                            h.write_u8(match owner {
                                NativeLayerOwner::Codex => 0,
                                NativeLayerOwner::Retired => 1,
                            });
                            h.write_str(profile_id);
                        }
                    }
                }
            }
        }
        match self.document_store.board_preview_selection() {
            BoardPreviewSelection::UsbStartup => h.write_u8(0),
            BoardPreviewSelection::Layer(layer) => {
                h.write_u8(1);
                h.write_u8(layer.number());
            }
        }
        h.write_opt_str(self.document_store.usb_startup_profile_id());
        h.finish()
    }

    fn slot_role_overrides_hash(&self) -> u64 {
        let mut h = Fnv1a::new();
        h.write_u64(self.slot_role_overrides.len() as u64);
        for ((layer_id, slot), role) in &self.slot_role_overrides {
            h.write_str(layer_id);
            h.write_u64(u64::from(*slot));
            h.write_u8(match role {
                KeycapRole::TaskKey => 0,
                KeycapRole::ActionKey => 1,
            });
        }
        h.finish()
    }

    fn host_caution_hash(&self) -> u64 {
        let mut h = Fnv1a::new();
        match &self.host_caution {
            None => h.write_u8(0),
            Some(caution) => {
                h.write_u8(1);
                h.write_str(&caution.detail_code);
                h.write_bool(caution.is_sticky_cleanup);
            }
        }
        h.finish()
    }

    fn lighting_modal_validation_error_hash(&self) -> u64 {
        let mut h = Fnv1a::new();
        match &self.active_modal {
            Some(EditorModalProjection::LightingEditor(modal)) => {
                h.write_u8(1);
                h.write_opt_str(modal.validation_error.as_deref());
            }
            _ => h.write_u8(0),
        }
        h.finish()
    }

    pub(crate) fn current_runtime_stamp(&self) -> RuntimeCommitStamp {
        RuntimeCommitStamp {
            draft_revision: self.document_store.draft.revision,
            host_revision: self.document_store.host_revision(),
            device_connected: self.device_session_connected(),
            usb_candidates_hash: self.usb_candidates_hash(),
            hardware_rpc_verified: self.hardware_rpc_verified,
            six_layer_busy: self.six_layer_job.is_some(),
            six_layer_present: self.six_layer_running.is_some(),
            six_layer_user_pinned: self.six_layer_user_pinned,
            ptt_active: self.ptt_controller.is_active,
            board_routing: self.board_routing,
            radial_state_hash: self.radial_state_hash(),
            radial_readiness: self.radial_readiness,
            overlay_state_hash: self.overlay_controller.state_stamp_hash(),
            screen_geometry_hash: self.screen_geometry_hash(),
            last_cursor_position: self.last_cursor_position,
            accessibility_trusted: self.accessibility_trusted,
            competing_input_app_running: self
                .applications
                .get("com.worklouder.input")
                .copied()
                .unwrap_or(false),
            can_retry_host_cleanup: self.can_retry_host_cleanup(),
            host_caution_hash: self.host_caution_hash(),
            busy: self.busy,
            status_menu_open: self.status_menu_open,
            banner_flash_count: self.banner_flash_count as u64,
            selected_binding_slot: self.selected_binding_slot,
            selected_surface: self.selected_surface,
            selected_joystick_direction: self.selected_joystick_direction,
            bottom_row_layout: self.bottom_row_layout,
            command_execution_enabled: self.command_execution_enabled,
            firmware_busy: self.firmware_update.projection.busy,
            firmware_reserved: self.firmware_update.projection.device_reserved,
            status_message_hash: fnv1a_str(&self.status_message),
            firmware_phase_hash: fnv1a_str(&self.firmware_update.projection.phase),
            firmware_message_hash: fnv1a_str(&self.firmware_update.projection.message),
            v070_text_hash: self.v070_text_hash(),
            encoder_picker_hash: self.encoder_picker_hash(),
            navigation_pending: self.session_navigation.is_pending(),
            navigation_catalog_mask: u8::from(
                self.session_navigation.antigravity_catalog.is_some(),
            ) | (u8::from(
                self.session_navigation.codex_catalog.is_some(),
            ) << 1)
                | (u8::from(self.session_navigation.catalog.is_some()) << 2),
            navigation_history_hash: self.navigation_history_hash(),
            slot_table_hash: self.slot_table_hash(),
            lighting_arbiter_hash: self.lighting_arbiter_hash(),
            lighting_preview_active: self.lighting_preview_active,
            lighting_preview_remaining_secs: self.lighting_preview_remaining_secs,
            lighting_modal_validation_error_hash: self.lighting_modal_validation_error_hash(),
            board_config_hash: self.board_config_hash(),
            slot_role_overrides_hash: self.slot_role_overrides_hash(),
        }
    }

    fn observed_firmware_changed(&self) -> bool {
        let prev = self.projection.firmware.current_version.as_deref();
        if self.hardware_rpc_verified && self.device_session_connected() {
            if let Some(receipt) = &self.last_hardware_auto_acceptance {
                return Some(receipt.firmware_version.as_str()) != prev;
            }
        }
        if let Some(running) = &self.six_layer_running {
            if !running.observed_firmware_version_eq(None) {
                return !running.observed_firmware_version_eq(prev);
            }
        }
        if let Some(session) = &self.device_session {
            if let Some(ver) = session.observed_firmware_version() {
                return Some(ver.as_str()) != prev;
            }
        }
        prev.is_some()
    }

    fn v070_hardware_fields_dirty(&self) -> bool {
        let cur = &self.v070_hardware.projection;
        let prev = &self.projection.v070_hardware;
        cur.supported != prev.supported
            || cur.busy != prev.busy
            || cur.synced != prev.synced
            || cur.active_profile != prev.active_profile
            || cur.active_layer != prev.active_layer
            || cur.active_joystick_mode != prev.active_joystick_mode
            || cur.battery_percent != prev.battery_percent
            || cur.is_charging != prev.is_charging
            || cur.host_sleep_status != prev.host_sleep_status
            || self.document_store.v070_auto_layer_sync_enabled() != prev.auto_layer_sync_enabled
            || self.document_store.v070_session_sa_rgb_sync_enabled()
                != prev.session_sa_rgb_sync_enabled
            || self.document_store.preferred_device_serial() != prev.preferred_device_serial
            || cur.mouse != prev.mouse
            || cur.pm_config != prev.pm_config
            || cur.pm_state != prev.pm_state
            || cur.device_name != prev.device_name
            || cur.sa_rgb != prev.sa_rgb
    }

    pub(crate) fn external_runtime_dirty(&self) -> bool {
        if self.projection_generation == 0 {
            return true;
        }
        let current_cache_stamp = (
            Arc::as_ptr(&self.app_cache) as usize,
            self.app_cache.generation(),
        );
        if self.captured_app_cache_stamp != current_cache_stamp {
            return true;
        }
        if self.six_layer_running.is_some() {
            let current_revs = (
                self.document_store.draft.revision,
                self.document_store.host_revision(),
            );
            if self.last_synced_six_layer_revs != current_revs {
                return true;
            }
        }
        let Some(last_stamp) = self.last_committed_stamp else {
            return true;
        };
        if last_stamp != self.current_runtime_stamp() {
            return true;
        }
        if self.selected_profile_id.as_deref() != self.projection.selected_profile_id.as_deref()
            || self.selected_layer_id.as_deref() != self.projection.selected_layer_id.as_deref()
        {
            return true;
        }
        if self.v070_hardware_fields_dirty() || self.observed_firmware_changed() {
            return true;
        }
        match (&self.six_layer_running, &self.six_layer_view) {
            (None, None) => false,
            (Some(_), None) | (None, Some(_)) => true,
            (Some(running), Some(prev)) => {
                prev.configuration_observed != running.readiness.configuration_observed
                    || prev.keyboard_grabbed != running.readiness.keyboard_grabbed
                    || prev.first_event_verified
                        != running.first_event_verified.load(Ordering::Acquire)
                    || prev.observed_layer != running.observed_layer.load(Ordering::Acquire)
                    || prev.armed != running.armed
                    || !running.effective_profile_id_eq(prev.effective_profile_id.as_deref())
                    || running.receiver.failure().as_deref() != prev.failure.as_deref()
            }
        }
    }
}
