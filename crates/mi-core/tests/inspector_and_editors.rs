use mi_core::{
    CanonicalAgent, CanvasAction, CodexKeycapId, EditorAction, EditorModalProjection, HeaderAction,
    InspectorAction, InspectorBodyProjection, InspectorDrawer, KeycapRole, LightingPreviewHandle,
    LightingPreviewPhase, LightingPreviewStarter, LightingUpdateResult, MultiBranchKind,
    MultiBranchTarget, ShortcutModifier, SidebarAction, UiLanguagePreference, WorkbenchCommand,
    WorkbenchError, WorkbenchStore, WorkbenchStoreConfig, INSPECTOR_WIDTH,
};
use mi_model::{
    comparison::Choice as ComparisonChoice,
    lighting::{Edit as LightingEdit, Effect as LightingEffect, Lighting, Zone as LightingZone},
    macro_sequence::MacroEvent,
};
use serde_json::{json, Value};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(1);

struct TempWorkspace {
    dir: PathBuf,
}

impl TempWorkspace {
    fn new(prefix: &str) -> Self {
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("mi-core-r9-{prefix}-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp test workspace");
        Self { dir }
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn build_verified_device_snapshot(config: &mi_model::DeviceConfig, session_id: u64) -> Value {
    let keymap_bytes = serde_json::to_vec(&config.raw).expect("serialize device raw keymap");
    let keymap_file = mi_model::operation_record::file_value(&keymap_bytes);
    let sha1 = keymap_file["sha1"].as_str().unwrap_or_default().to_string();
    json!({
        "firmware": "0.6.3",
        "device_identity": {
            "confidence": "verified-serial",
            "serial": "synthetic-one",
            "session_id": session_id
        },
        "keymap": keymap_file,
        "smart_actions": {
            "state": "missing"
        },
        "files": [
            {
                "name": "keymap",
                "state": "present",
                "size": keymap_bytes.len(),
                "sha1": sha1
            }
        ]
    })
}

fn build_observed_interface_snapshot(
    config: &mi_model::DeviceConfig,
    session_id: u64,
    interface: &str,
) -> Value {
    let keymap_bytes = serde_json::to_vec(&config.raw).expect("serialize device raw keymap");
    let keymap_file = mi_model::operation_record::file_value(&keymap_bytes);
    let sha1 = keymap_file["sha1"].as_str().unwrap_or_default().to_string();
    json!({
        "firmware": "0.6.3",
        "device_identity": {
            "confidence": "observed-interface",
            "interface": interface,
            "session_id": session_id
        },
        "keymap": keymap_file,
        "smart_actions": {
            "state": "missing"
        },
        "files": [
            {
                "name": "keymap",
                "state": "present",
                "size": keymap_bytes.len(),
                "sha1": sha1
            }
        ]
    })
}

#[test]
fn role_switching_keycap_catalog_and_apple_fn_shortcut_recording() {
    let ws = TempWorkspace::new("inspector-roles-keycaps");
    let mut store = WorkbenchStore::open_isolated(&ws.dir).expect("open isolated workbench");

    assert_eq!(store.projection().inspector.width_px, INSPECTOR_WIDTH);
    assert_eq!(CodexKeycapId::ALL_34.len(), 34);
    for keycap in CodexKeycapId::ALL_34 {
        assert!(!keycap.silk_code().is_empty());
        assert!(!keycap.short_label().is_empty());
        assert!(!keycap.chinese_hint().is_empty());
        assert!(!keycap.glyph_symbol().is_empty());
    }

    store
        .dispatch_inspector_action(InspectorAction::SelectSlot(0))
        .expect("select top slot 0");
    let editable = store
        .projection()
        .inspector
        .editable()
        .expect("Antigravity starter layer should be editable");
    assert!(editable.supports_task_role);
    assert_eq!(editable.filtered_keycaps.len(), 34);

    store
        .dispatch_inspector_action(InspectorAction::SetSlotRole(KeycapRole::TaskKey))
        .expect("set top slot 0 to TaskKey");
    let editable = store.projection().inspector.editable().unwrap();
    assert_eq!(editable.slot_role, KeycapRole::TaskKey);
    assert_eq!(editable.bound_action_id.as_deref(), Some("slot-open-0"));

    store
        .dispatch_inspector_action(InspectorAction::SetKeycapSearchQuery("语音".into()))
        .expect("search keycap catalog for voice");
    let editable = store.projection().inspector.editable().unwrap();
    assert!(
        editable
            .filtered_keycaps
            .iter()
            .any(|item| item.id == CodexKeycapId::Mic),
        "expected Mic keycap in filtered results for '语音'"
    );
    assert!(editable.filtered_keycaps.len() < 34);

    store
        .dispatch_inspector_action(InspectorAction::SetKeycapSearchQuery(String::new()))
        .expect("clear keycap search query");
    store
        .dispatch_inspector_action(InspectorAction::ApplyKeycapIcon {
            keycap: CodexKeycapId::Yolo,
            bind_default_action: true,
        })
        .expect("apply Yolo keycap with default action");
    let editable = store.projection().inspector.editable().unwrap();
    assert_eq!(editable.selected_keycap, CodexKeycapId::Yolo);
    assert_eq!(editable.slot_role, KeycapRole::ActionKey);
    assert_eq!(editable.bound_action_id.as_deref(), Some("agent-approve"));
    let cell0 = store
        .projection()
        .canvas
        .keycap_cells
        .iter()
        .find(|c| c.primary_slot == 0)
        .expect("cell 0 on canvas");
    assert_eq!(
        cell0.silkscreen_icon.as_deref(),
        Some(CodexKeycapId::Yolo.glyph_symbol())
    );

    store
        .dispatch_inspector_action(InspectorAction::SetSlotRole(KeycapRole::TaskKey))
        .expect("switch slot 0 back to TaskKey");
    let editable = store.projection().inspector.editable().unwrap();
    assert_eq!(editable.slot_role, KeycapRole::TaskKey);
    assert_eq!(editable.bound_action_id.as_deref(), Some("slot-open-0"));

    store
        .dispatch_inspector_action(InspectorAction::SetSlotRole(KeycapRole::ActionKey))
        .expect("switch slot 0 to ActionKey");
    let editable = store.projection().inspector.editable().unwrap();
    assert_eq!(editable.slot_role, KeycapRole::ActionKey);
    assert_ne!(editable.bound_action_id.as_deref(), Some("slot-open-0"));

    store
        .dispatch_inspector_action(InspectorAction::SelectSlot(6))
        .expect("select middle row slot 6");
    let editable = store.projection().inspector.editable().unwrap();
    assert!(!editable.supports_task_role);
    assert_eq!(
        editable.bound_action_id.as_deref(),
        Some("desktop-live-voice")
    );
    assert_eq!(
        editable.selected_keycap,
        CodexKeycapId::Party,
        "Antigravity slot 6 (Gemini Live 实时语音) must infer Party (5-bar voice waveform) keycap instead of Term"
    );
    let cell6 = store
        .projection()
        .canvas
        .keycap_cells
        .iter()
        .find(|c| c.primary_slot == 6)
        .expect("cell 6 on canvas");
    assert_eq!(cell6.keycap_id, "Party");
    assert_eq!(cell6.concise_caption, "实时语音");
    let role_err = store
        .dispatch_inspector_action(InspectorAction::SetSlotRole(KeycapRole::TaskKey))
        .expect_err("slot 6 must reject TaskKey role");
    assert!(matches!(role_err, WorkbenchError::Storage(_)));

    store
        .dispatch_inspector_action(InspectorAction::BindAppleFnKey)
        .expect("bind one-click Apple Fn key on slot 6");
    let editable = store.projection().inspector.editable().unwrap();
    assert!(editable.is_apple_fn_bound);
    assert_eq!(editable.bound_action_id.as_deref(), Some("apple-fn-key"));
    assert_eq!(editable.selected_keycap, CodexKeycapId::Mic);

    store
        .dispatch_inspector_action(InspectorAction::StartShortcutRecording)
        .expect("start shortcut recording");
    assert!(
        store
            .projection()
            .inspector
            .editable()
            .unwrap()
            .shortcut_recorder
            .is_recording
    );
    store
        .dispatch_inspector_action(InspectorAction::CaptureShortcutChord {
            key: "k".into(),
            modifiers: vec![ShortcutModifier::Command, ShortcutModifier::Shift],
        })
        .expect("capture Cmd+Shift+K");
    store
        .dispatch_inspector_action(InspectorAction::CommitRecordedShortcut)
        .expect("commit recorded shortcut");
    let editable = store.projection().inspector.editable().unwrap();
    assert!(!editable.shortcut_recorder.is_recording);
    assert!(!editable.is_apple_fn_bound);
    assert!(editable.bound_action_name.contains("K") || editable.bound_action_name.contains("k"));

    store
        .dispatch_inspector_action(InspectorAction::StartShortcutRecording)
        .expect("start shortcut recording for globe normalization");
    store
        .dispatch_inspector_action(InspectorAction::CaptureShortcutChord {
            key: "0x3F".into(),
            modifiers: vec![],
        })
        .expect("capture 0x3F Apple Fn chord");
    store
        .dispatch_inspector_action(InspectorAction::CommitRecordedShortcut)
        .expect("commit 0x3F chord as normalized Apple Fn");
    let editable = store.projection().inspector.editable().unwrap();
    assert!(editable.is_apple_fn_bound);
    assert_eq!(editable.bound_action_id.as_deref(), Some("apple-fn-key"));
    assert_eq!(editable.selected_keycap, CodexKeycapId::Mic);

    store
        .dispatch_inspector_action(InspectorAction::ToggleDrawer(InspectorDrawer::MoreActions))
        .expect("open MoreActions drawer");
    assert!(store
        .projection()
        .inspector
        .editable()
        .unwrap()
        .open_drawers
        .contains(&InspectorDrawer::MoreActions));
}

#[test]
fn macro_editor_creates_validates_reorders_binds_and_manages_library() {
    let ws = TempWorkspace::new("macro-editor");
    let mut store = WorkbenchStore::open_isolated(&ws.dir).expect("open isolated workbench");

    store
        .dispatch_inspector_action(InspectorAction::SelectSlot(7))
        .expect("select slot 7");
    store
        .dispatch_editor_action(EditorAction::OpenMacroEditor { macro_id: None })
        .expect("open new macro editor");
    assert!(matches!(
        store.active_modal(),
        Some(EditorModalProjection::MacroEditor(_))
    ));

    store
        .dispatch_editor_action(EditorAction::MacroAppendEvent(MacroEvent::Press {
            key: "a".into(),
        }))
        .expect("append unpaired press");
    assert!(
        store
            .dispatch_editor_action(EditorAction::MacroSave {
                bind_to_selected_slot: true,
            })
            .is_err(),
        "unpaired Press event must fail MacroSave"
    );

    store
        .dispatch_editor_action(EditorAction::MacroClearSteps)
        .expect("clear steps");
    store
        .dispatch_editor_action(EditorAction::MacroAppendEvent(MacroEvent::Delay { ms: 0 }))
        .expect("append zero delay");
    assert!(
        store
            .dispatch_editor_action(EditorAction::MacroSave {
                bind_to_selected_slot: true,
            })
            .is_err(),
        "zero ms Delay must fail MacroSave"
    );

    store
        .dispatch_editor_action(EditorAction::MacroClearSteps)
        .expect("clear steps again");
    store
        .dispatch_editor_action(EditorAction::MacroSetName("一键保存并提交".into()))
        .expect("set macro name");
    store
        .dispatch_editor_action(EditorAction::MacroAddTapKey {
            key: "s".into(),
            delay_ms: 40,
        })
        .expect("add tap s with 40ms delay");
    store
        .dispatch_editor_action(EditorAction::MacroAddTapKey {
            key: "return".into(),
            delay_ms: 20,
        })
        .expect("add tap return with 20ms delay");

    if let Some(EditorModalProjection::MacroEditor(modal)) = store.active_modal() {
        assert_eq!(modal.steps.len(), 6);
    } else {
        panic!("expected MacroEditor modal");
    }

    store
        .dispatch_editor_action(EditorAction::MacroMoveStep { from: 1, to: 5 })
        .expect("move delay step to end");
    store
        .dispatch_editor_action(EditorAction::MacroRemoveStep(5))
        .expect("remove trailing delay step");

    store
        .dispatch_editor_action(EditorAction::MacroSave {
            bind_to_selected_slot: true,
        })
        .expect("save valid macro and bind to slot 7");
    assert!(store.active_modal().is_none());

    let editable = store.projection().inspector.editable().unwrap();
    assert_eq!(editable.bound_action_name, "一键保存并提交");
    assert_eq!(editable.macros.len(), 1);
    let saved_macro = &editable.macros[0];
    assert_eq!(saved_macro.name, "一键保存并提交");
    assert_eq!(saved_macro.reference_count, 1);
    assert!(saved_macro.is_currently_bound);
    let macro_id = saved_macro.id.clone();

    assert!(
        store
            .dispatch_editor_action(EditorAction::MacroDelete {
                macro_id: macro_id.clone(),
            })
            .is_err(),
        "deleting a macro while bound to a slot must be rejected"
    );

    store
        .dispatch_editor_action(EditorAction::MacroBindExisting { macro_id: None })
        .expect("unbind macro from slot 7");
    assert_eq!(
        store.projection().inspector.editable().unwrap().macros[0].reference_count,
        0
    );

    store
        .dispatch_editor_action(EditorAction::MacroBindExisting {
            macro_id: Some(macro_id.clone()),
        })
        .expect("rebind existing macro to slot 7");
    assert_eq!(
        store.projection().inspector.editable().unwrap().macros[0].reference_count,
        1
    );

    store
        .dispatch_editor_action(EditorAction::MacroBindExisting { macro_id: None })
        .expect("unbind macro before deletion");
    store
        .dispatch_editor_action(EditorAction::MacroDelete { macro_id })
        .expect("delete unbound macro from library");
    assert!(store
        .projection()
        .inspector
        .editable()
        .unwrap()
        .macros
        .is_empty());
}

#[test]
fn lighting_editor_dual_zone_all_layers_preview_and_protected_layer_isolation() {
    let ws = TempWorkspace::new("lighting-and-protected");
    let mut store = WorkbenchStore::open_isolated(&ws.dir).expect("open isolated workbench");

    store
        .dispatch_inspector_action(InspectorAction::CreateCustomLayer {
            name: "扩展层 2".into(),
        })
        .expect("create second editable layer");

    store
        .dispatch_editor_action(EditorAction::OpenLightingEditor)
        .expect("open lighting editor");
    store
        .dispatch_editor_action(EditorAction::LightingAddDefault)
        .expect("initialize default dual-zone lighting");
    store
        .dispatch_editor_action(EditorAction::LightingEditEffect {
            zone: LightingZone::Backlight,
            effect: LightingEffect::Breath,
            all_layers: true,
        })
        .expect("set backlight effect to Breath across all layers");
    store
        .dispatch_editor_action(EditorAction::LightingEditColorHex {
            zone: LightingZone::Backlight,
            color_hex: "#33AAFF".into(),
            all_layers: true,
        })
        .expect("set backlight color to #33AAFF");
    store
        .dispatch_editor_action(EditorAction::LightingEditBrightness {
            zone: LightingZone::Backlight,
            brightness_percent: 75,
            all_layers: false,
        })
        .expect("set backlight brightness to 75%");
    store
        .dispatch_editor_action(EditorAction::LightingEditSpeed {
            zone: LightingZone::Backlight,
            speed_percent: 60,
            all_layers: false,
        })
        .expect("set backlight speed to 60%");
    store
        .dispatch_editor_action(EditorAction::LightingEditMagic {
            zone: LightingZone::Backlight,
            magic_percent: 40,
            all_layers: false,
        })
        .expect("set backlight magic to 40%");
    store
        .dispatch_editor_action(EditorAction::LightingEditEffect {
            zone: LightingZone::Underglow,
            effect: LightingEffect::Solid,
            all_layers: false,
        })
        .expect("set underglow effect to Solid");
    store
        .dispatch_editor_action(EditorAction::LightingEditColorHex {
            zone: LightingZone::Underglow,
            color_hex: "#FF8800".into(),
            all_layers: false,
        })
        .expect("set underglow color to #FF8800");

    let before_gen = store.projection_generation();
    let offline_err = store
        .dispatch_editor_action(EditorAction::LightingBeginPreview)
        .expect_err("offline default starter must reject hardware lighting preview");
    assert!(matches!(offline_err, WorkbenchError::Routing(_)));
    assert!(
        store.projection_generation() > before_gen,
        "offline preview rejection must bump projection_generation so FFI publishes validation_error"
    );
    if let Some(EditorModalProjection::LightingEditor(modal)) = store.active_modal() {
        assert!(modal.has_lighting);
        assert!(!modal.preview_active);
        assert_eq!(modal.preview_remaining_secs, 0);
        let err = modal
            .validation_error
            .as_deref()
            .expect("offline rejection must populate modal validation_error");
        assert!(err.contains("USB"));
        let bl = modal.backlight.as_ref().expect("backlight projection");
        assert_eq!(bl.effect, Some(LightingEffect::Breath));
        assert_eq!(bl.color_hex, "#33AAFF");
        assert_eq!(bl.brightness_percent, 75);
        assert_eq!(bl.speed_percent, 60);
        assert_eq!(bl.magic_percent, 40);
        let ug = modal.underglow.as_ref().expect("underglow projection");
        assert_eq!(ug.effect, Some(LightingEffect::Solid));
        assert_eq!(ug.color_hex, "#FF8800");
    } else {
        panic!("expected LightingEditor modal");
    }

    store
        .dispatch_header_action(HeaderAction::SetUiLanguage {
            language: UiLanguagePreference::En,
        })
        .expect("switch UI locale to English");
    if let Some(EditorModalProjection::LightingEditor(modal)) = store.active_modal() {
        let err_en = modal
            .validation_error
            .as_deref()
            .expect("English localized validation_error");
        assert!(
            err_en.contains("No verified USB exclusive keypad connected"),
            "unexpected English validation_error: {err_en}"
        );
    } else {
        panic!("expected LightingEditor modal after locale switch");
    }

    let before_bad_hex = store.projection_generation();
    assert!(store
        .dispatch_editor_action(EditorAction::LightingEditColorHex {
            zone: LightingZone::Backlight,
            color_hex: "#ZZZZZZ".into(),
            all_layers: false,
        })
        .is_err());
    assert!(store.projection_generation() > before_bad_hex);
    if let Some(EditorModalProjection::LightingEditor(modal)) = store.active_modal() {
        let hex_err = modal
            .validation_error
            .as_deref()
            .expect("invalid hex must populate validation_error");
        assert!(hex_err.contains("Invalid hex color"), "got: {hex_err}");
    } else {
        panic!("expected LightingEditor modal after invalid hex");
    }

    store
        .dispatch_editor_action(EditorAction::LightingEndPreview)
        .expect("end lighting preview is idempotent when idle");
    if let Some(EditorModalProjection::LightingEditor(modal)) = store.active_modal() {
        assert!(!modal.preview_active);
        assert_eq!(modal.preview_remaining_secs, 0);
        assert!(modal.validation_error.is_none());
    }

    store
        .dispatch_inspector_action(InspectorAction::StartShortcutRecording)
        .expect("start shortcut recording before switching to protected profile");

    let codex_id = store
        .find_canonical_profile_id(CanonicalAgent::Codex)
        .expect("Codex canonical profile must exist");
    store
        .dispatch_sidebar_action(SidebarAction::SelectProfile(codex_id))
        .expect("switch to protected Codex profile");

    assert!(store.is_selected_layer_protected());
    assert!(
        store.active_modal().is_none(),
        "switching to protected Mg1 layer must auto-close mutating modal"
    );
    assert!(matches!(
        store.projection().inspector.body,
        InspectorBodyProjection::ProtectedLocked(_)
    ));
    assert!(store.projection().inspector.editable().is_none());
    assert!(store.projection().inspector.protected_lock().is_some());

    for forbidden_inspector in [
        InspectorAction::BindAppleFnKey,
        InspectorAction::SetSlotRole(KeycapRole::TaskKey),
        InspectorAction::ApplyKeycapIcon {
            keycap: CodexKeycapId::Mic,
            bind_default_action: true,
        },
        InspectorAction::StartShortcutRecording,
        InspectorAction::AssignCatalogAction("save".into()),
    ] {
        let err = store
            .dispatch_inspector_action(forbidden_inspector)
            .expect_err("protected layer must reject mutating InspectorAction");
        assert_eq!(err, WorkbenchError::ProtectedLayerLocked);
    }

    for forbidden_editor in [
        EditorAction::OpenMacroEditor { macro_id: None },
        EditorAction::OpenLightingEditor,
        EditorAction::OpenMultiEditor { multi_id: None },
        EditorAction::LightingBeginPreview,
    ] {
        let err = store
            .dispatch_editor_action(forbidden_editor)
            .expect_err("protected layer must reject mutating EditorAction");
        assert_eq!(err, WorkbenchError::ProtectedLayerLocked);
    }

    store
        .dispatch_editor_action(EditorAction::LightingEndPreview)
        .expect("stopping preview must remain allowed on protected layers");

    store
        .dispatch_inspector_action(InspectorAction::UnlockProtectedLayer)
        .expect("unlock by creating and switching to custom layer");
    assert!(!store.is_selected_layer_protected());
    assert!(store.projection().inspector.editable().is_some());
}

#[derive(Debug)]
struct FakeHandleRecord {
    updates: Mutex<Vec<(String, String)>>,
    replaces: Mutex<Vec<Lighting>>,
    phase: Mutex<LightingPreviewPhase>,
    end_calls: AtomicUsize,
    detach_calls: AtomicUsize,
    end_gate: Mutex<Option<Arc<tokio::sync::Notify>>>,
}

impl FakeHandleRecord {
    fn active() -> Arc<Self> {
        Arc::new(Self {
            updates: Mutex::new(Vec::new()),
            replaces: Mutex::new(Vec::new()),
            phase: Mutex::new(LightingPreviewPhase::Active),
            end_calls: AtomicUsize::new(0),
            detach_calls: AtomicUsize::new(0),
            end_gate: Mutex::new(None),
        })
    }

    fn set_phase(&self, phase: LightingPreviewPhase) {
        *self.phase.lock().unwrap() = phase;
    }

    fn set_end_gate(&self, gate: Option<Arc<tokio::sync::Notify>>) {
        *self.end_gate.lock().unwrap() = gate;
    }
}

#[derive(Debug)]
struct FakePreviewHandle {
    record: Arc<FakeHandleRecord>,
}

impl LightingPreviewHandle for FakePreviewHandle {
    fn update(&self, zone: LightingZone, edit: LightingEdit) -> Result<(), String> {
        let phase = self.record.phase.lock().unwrap().clone();
        if phase != LightingPreviewPhase::Active {
            return Err("preview has ended".into());
        }
        self.record
            .updates
            .lock()
            .unwrap()
            .push((zone.name().to_string(), format!("{edit:?}")));
        Ok(())
    }

    fn replace(&self, lights: Lighting) -> Result<(), String> {
        let phase = self.record.phase.lock().unwrap().clone();
        if phase != LightingPreviewPhase::Active {
            return Err("preview has ended".into());
        }
        self.record.replaces.lock().unwrap().push(lights);
        Ok(())
    }

    fn phase(&self) -> LightingPreviewPhase {
        self.record.phase.lock().unwrap().clone()
    }

    fn end_future(&self) -> Pin<Box<dyn Future<Output = LightingPreviewPhase> + Send + 'static>> {
        let record = Arc::clone(&self.record);
        Box::pin(async move {
            let gate = record.end_gate.lock().unwrap().clone();
            if let Some(gate) = gate {
                gate.notified().await;
            }
            record.end_calls.fetch_add(1, Ordering::AcqRel);
            *record.phase.lock().unwrap() = LightingPreviewPhase::Stopped;
            LightingPreviewPhase::Stopped
        })
    }

    fn detach_on_drop(&self) {
        self.record.detach_calls.fetch_add(1, Ordering::AcqRel);
    }
}

struct FakePreviewStarter {
    duration: Duration,
    start_count: AtomicUsize,
    started_lights: Mutex<Vec<Lighting>>,
    handles: Mutex<Vec<Arc<FakeHandleRecord>>>,
    next_gate: Mutex<Option<Arc<tokio::sync::Notify>>>,
    next_error: Mutex<Option<String>>,
}

impl FakePreviewStarter {
    fn new(duration: Duration) -> Arc<Self> {
        Arc::new(Self {
            duration,
            start_count: AtomicUsize::new(0),
            started_lights: Mutex::new(Vec::new()),
            handles: Mutex::new(Vec::new()),
            next_gate: Mutex::new(None),
            next_error: Mutex::new(None),
        })
    }

    fn arm_gate(&self) -> Arc<tokio::sync::Notify> {
        let notify = Arc::new(tokio::sync::Notify::new());
        *self.next_gate.lock().unwrap() = Some(Arc::clone(&notify));
        notify
    }

    fn last_handle(&self) -> Arc<FakeHandleRecord> {
        self.handles
            .lock()
            .unwrap()
            .last()
            .cloned()
            .expect("fake preview handle")
    }
}

impl LightingPreviewStarter for FakePreviewStarter {
    fn requires_verified_session(&self) -> bool {
        false
    }

    fn preview_duration(&self) -> Duration {
        self.duration
    }

    fn begin_future(
        &self,
        _session: Option<Arc<mi_session::DeviceSession>>,
        lights: Lighting,
    ) -> Pin<
        Box<dyn Future<Output = Result<Box<dyn LightingPreviewHandle>, String>> + Send + 'static>,
    > {
        self.start_count.fetch_add(1, Ordering::AcqRel);
        self.started_lights.lock().unwrap().push(lights);
        let gate = self.next_gate.lock().unwrap().take();
        let next_err = self.next_error.lock().unwrap().take();
        let handle_record = FakeHandleRecord::active();
        if next_err.is_none() {
            self.handles
                .lock()
                .unwrap()
                .push(Arc::clone(&handle_record));
        }
        Box::pin(async move {
            if let Some(gate) = gate {
                gate.notified().await;
            }
            if let Some(err) = next_err {
                return Err(err);
            }
            Ok(Box::new(FakePreviewHandle {
                record: handle_record,
            }) as Box<dyn LightingPreviewHandle>)
        })
    }
}

fn wait_until(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !predicate() {
        assert!(Instant::now() < deadline, "condition timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn lighting_modal(store: &WorkbenchStore) -> mi_core::LightingEditorModalProjection {
    match store.active_modal() {
        Some(EditorModalProjection::LightingEditor(modal)) => modal.clone(),
        other => panic!("expected LightingEditor modal, got {other:?}"),
    }
}

#[test]
fn lighting_preview_fake_starter_lifecycle_edits_countdown_errors_and_teardowns() {
    let ws = TempWorkspace::new("lighting-fake-starter");
    let starter = FakePreviewStarter::new(Duration::from_secs(30));
    let mut cfg = WorkbenchStoreConfig::isolated(&ws.dir);
    cfg.lighting_preview_starter = Arc::clone(&starter) as Arc<dyn LightingPreviewStarter>;
    let mut store = WorkbenchStore::open(cfg).expect("open store with fake preview starter");

    store
        .dispatch_inspector_action(InspectorAction::CreateCustomLayer {
            name: "测试层 2".into(),
        })
        .expect("create second layer");
    let second_layer_id = store.selected_layer_id().unwrap().to_string();
    let first_layer_id = store.projection().canvas.layer_tabs[0].layer_id.clone();

    store
        .dispatch_editor_action(EditorAction::OpenLightingEditor)
        .expect("open lighting editor");
    store
        .dispatch_editor_action(EditorAction::LightingAddDefault)
        .expect("add default lighting");

    let gate = starter.arm_gate();
    let t0 = Instant::now();
    store
        .dispatch_at(
            WorkbenchCommand::Editor(EditorAction::LightingBeginPreview),
            t0,
        )
        .expect("async start returns immediately while gated");
    assert!(
        !lighting_modal(&store).preview_active,
        "Starting state must not falsely report preview_active before device ACK"
    );
    store.refresh_four_app_lighting();
    assert_eq!(
        store.lighting_arbiter.last_update_result,
        LightingUpdateResult::DeviceBusy
    );

    store
        .dispatch_editor_action(EditorAction::LightingEditColorHex {
            zone: LightingZone::Backlight,
            color_hex: "#123456".into(),
            all_layers: false,
        })
        .expect("edit arriving during Starting must be retained");
    gate.notify_one();
    wait_until(|| {
        store.poll_at(t0);
        lighting_modal(&store).preview_active
    });
    let h1 = starter.last_handle();
    assert_eq!(lighting_modal(&store).preview_remaining_secs, 30);
    assert_eq!(
        h1.replaces.lock().unwrap().len(),
        1,
        "dirty edit made during Starting must flush via replace upon Active"
    );
    assert_eq!(
        h1.replaces.lock().unwrap()[0].backlight["color"],
        json!(0x123456)
    );

    store
        .dispatch_editor_action(EditorAction::LightingEditBrightness {
            zone: LightingZone::Underglow,
            brightness_percent: 45,
            all_layers: false,
        })
        .expect("live edit while Active pushes update");
    assert_eq!(h1.updates.lock().unwrap().len(), 1);
    assert_eq!(h1.updates.lock().unwrap()[0].0, "underglow");

    let gen_before_tick = store.projection_generation();
    assert!(store.poll_at(t0 + Duration::from_secs(1)));
    assert!(store.projection_generation() > gen_before_tick);
    assert_eq!(lighting_modal(&store).preview_remaining_secs, 29);

    assert!(store.poll_at(t0 + Duration::from_millis(29_500)));
    assert_eq!(lighting_modal(&store).preview_remaining_secs, 1);

    assert!(store.poll_at(t0 + Duration::from_secs(30)));
    assert!(!lighting_modal(&store).preview_active);
    assert_eq!(lighting_modal(&store).preview_remaining_secs, 0);
    assert!(
        store.status_message().contains("未自动恢复"),
        "expiry status must remain honest about no automatic restoration"
    );
    wait_until(|| {
        store.poll_at(t0 + Duration::from_secs(30));
        h1.end_calls.load(Ordering::Acquire) == 1
    });
    wait_until(|| {
        store.poll_at(t0 + Duration::from_secs(30));
        store.refresh_four_app_lighting();
        store.lighting_arbiter.last_update_result == LightingUpdateResult::Applied
    });

    let cancel_gate = starter.arm_gate();
    let t1 = t0 + Duration::from_secs(31);
    store
        .dispatch_at(
            WorkbenchCommand::Editor(EditorAction::LightingBeginPreview),
            t1,
        )
        .expect("start second preview to test early cancel during Starting");
    cancel_gate.notify_one();
    std::thread::sleep(Duration::from_millis(40));
    let h_stale = starter.last_handle();
    store
        .dispatch_editor_action(EditorAction::LightingEndPreview)
        .expect("cancel while Starting before inbox event is drained");
    store.poll_at(t1 + Duration::from_millis(50));
    assert!(!lighting_modal(&store).preview_active);
    assert_eq!(
        h_stale.detach_calls.load(Ordering::Acquire),
        1,
        "stale handle arriving after early stop during Starting must be detached"
    );

    let t2 = t1 + Duration::from_secs(1);
    store
        .dispatch_at(
            WorkbenchCommand::Editor(EditorAction::LightingBeginPreview),
            t2,
        )
        .expect("start preview for Unknown error test");
    wait_until(|| {
        store.poll_at(t2);
        lighting_modal(&store).preview_active
    });
    let h_err = starter.last_handle();
    let starts_before_err = starter.start_count.load(Ordering::Acquire);
    h_err.set_phase(LightingPreviewPhase::Unknown("transport timeout".into()));
    assert!(store.poll_at(t2 + Duration::from_millis(50)));
    let err_modal = lighting_modal(&store);
    assert!(!err_modal.preview_active);
    assert_eq!(err_modal.preview_remaining_secs, 0);
    assert!(err_modal
        .validation_error
        .as_deref()
        .is_some_and(|e| e.contains("未自动恢复") && e.contains("transport timeout")));
    assert_eq!(h_err.detach_calls.load(Ordering::Acquire), 1);
    assert_eq!(
        starter.start_count.load(Ordering::Acquire),
        starts_before_err,
        "Unknown preview failure must never auto-retry"
    );

    let t3 = t2 + Duration::from_secs(1);
    store
        .dispatch_at(
            WorkbenchCommand::Editor(EditorAction::LightingBeginPreview),
            t3,
        )
        .expect("start preview before CloseModal");
    wait_until(|| {
        store.poll_at(t3);
        lighting_modal(&store).preview_active
    });
    let h_close = starter.last_handle();
    store
        .dispatch_editor_action(EditorAction::CloseModal)
        .expect("close modal stops preview");
    wait_until(|| {
        store.poll_at(t3);
        h_close.end_calls.load(Ordering::Acquire) == 1
    });
    wait_until(|| {
        store.poll_at(t3);
        store.refresh_four_app_lighting();
        store.lighting_arbiter.last_update_result == LightingUpdateResult::Applied
    });

    store
        .dispatch_editor_action(EditorAction::OpenLightingEditor)
        .expect("reopen lighting editor");
    store
        .dispatch_at(
            WorkbenchCommand::Editor(EditorAction::LightingBeginPreview),
            t3,
        )
        .expect("start preview before SelectLayer");
    wait_until(|| {
        store.poll_at(t3);
        lighting_modal(&store).preview_active
    });
    let h_layer = starter.last_handle();
    store
        .dispatch_canvas_action(CanvasAction::SelectLayer(first_layer_id))
        .expect("switching layers must stop active preview");
    wait_until(|| {
        store.poll_at(t3);
        h_layer.end_calls.load(Ordering::Acquire) == 1
    });
    wait_until(|| {
        store.poll_at(t3);
        store.refresh_four_app_lighting();
        store.lighting_arbiter.last_update_result == LightingUpdateResult::Applied
    });

    store
        .dispatch_canvas_action(CanvasAction::SelectLayer(second_layer_id))
        .expect("return to layer with lighting");
    store
        .dispatch_at(
            WorkbenchCommand::Editor(EditorAction::LightingBeginPreview),
            t3,
        )
        .expect("start preview before SelectProfile");
    wait_until(|| {
        store.poll_at(t3);
        lighting_modal(&store).preview_active
    });
    let h_profile = starter.last_handle();
    let dsh_id = store
        .find_canonical_profile_id(CanonicalAgent::DeepseekHarness)
        .expect("DSH profile");
    store
        .dispatch_sidebar_action(SidebarAction::SelectProfile(dsh_id))
        .expect("switching profile must stop active preview");
    wait_until(|| {
        store.poll_at(t3);
        h_profile.end_calls.load(Ordering::Acquire) == 1
    });
    wait_until(|| {
        store.poll_at(t3);
        store.refresh_four_app_lighting();
        store.lighting_arbiter.last_update_result == LightingUpdateResult::Applied
    });

    store
        .dispatch_editor_action(EditorAction::OpenLightingEditor)
        .expect("open lighting editor on DSH");
    store
        .dispatch_editor_action(EditorAction::LightingAddDefault)
        .expect("add lighting on DSH");
    store
        .dispatch_at(
            WorkbenchCommand::Editor(EditorAction::LightingBeginPreview),
            t3,
        )
        .expect("start preview before LightingResetLayer removes lighting");
    wait_until(|| {
        store.poll_at(t3);
        lighting_modal(&store).preview_active
    });
    let h_reset = starter.last_handle();
    store
        .dispatch_editor_action(EditorAction::LightingResetLayer)
        .expect("resetting unsourced layer removes lighting and stops preview");
    assert!(!lighting_modal(&store).has_lighting);
    assert!(!lighting_modal(&store).preview_active);
    wait_until(|| {
        store.poll_at(t3);
        h_reset.end_calls.load(Ordering::Acquire) == 1
    });
    wait_until(|| {
        store.poll_at(t3);
        store.refresh_four_app_lighting();
        store.lighting_arbiter.last_update_result == LightingUpdateResult::Applied
    });

    store
        .dispatch_editor_action(EditorAction::LightingAddDefault)
        .expect("re-add lighting before SystemWillSleep");
    store
        .dispatch_at(
            WorkbenchCommand::Editor(EditorAction::LightingBeginPreview),
            t3,
        )
        .expect("start preview before sleep");
    wait_until(|| {
        store.poll_at(t3);
        lighting_modal(&store).preview_active
    });
    let h_sleep = starter.last_handle();
    let end_gate = Arc::new(tokio::sync::Notify::new());
    h_sleep.set_end_gate(Some(Arc::clone(&end_gate)));
    let releaser = Arc::clone(&end_gate);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        releaser.notify_one();
    });
    store.system_will_sleep();
    assert_eq!(
        h_sleep.end_calls.load(Ordering::Acquire),
        1,
        "system_will_sleep must drain preview end_future synchronously before returning"
    );
    assert!(!lighting_modal(&store).preview_active);
}

#[test]
fn lighting_preview_context_switches_refresh_open_modal_or_close_on_protected_layer() {
    let ws = TempWorkspace::new("lighting-context-switch-modal");
    let starter = FakePreviewStarter::new(Duration::from_secs(30));
    let mut cfg = WorkbenchStoreConfig::isolated(&ws.dir);
    cfg.lighting_preview_starter = Arc::clone(&starter) as Arc<dyn LightingPreviewStarter>;
    let mut store = WorkbenchStore::open(cfg).expect("open store with fake preview starter");
    let t0 = Instant::now();

    let ag_id = store
        .find_canonical_profile_id(CanonicalAgent::Antigravity)
        .expect("Antigravity profile");
    let dsh_id = store
        .find_canonical_profile_id(CanonicalAgent::DeepseekHarness)
        .expect("DSH profile");
    let codex_id = store
        .find_canonical_profile_id(CanonicalAgent::Codex)
        .expect("Codex protected profile");

    store
        .dispatch_sidebar_action(SidebarAction::SelectProfile(ag_id.clone()))
        .unwrap();
    let layer1_id = store.selected_layer_id().unwrap().to_string();
    store
        .dispatch_editor_action(EditorAction::OpenLightingEditor)
        .unwrap();
    store
        .dispatch_editor_action(EditorAction::LightingAddDefault)
        .unwrap();
    store
        .dispatch_editor_action(EditorAction::LightingEditColorHex {
            zone: LightingZone::Backlight,
            color_hex: "#112233".into(),
            all_layers: false,
        })
        .unwrap();

    store
        .dispatch_inspector_action(InspectorAction::CreateCustomLayer {
            name: "第二层".into(),
        })
        .unwrap();
    let layer2_id = store.selected_layer_id().unwrap().to_string();
    assert_eq!(lighting_modal(&store).layer_id, layer2_id);
    assert_eq!(lighting_modal(&store).layer_name, "第二层");
    assert!(!lighting_modal(&store).has_lighting);

    store
        .dispatch_editor_action(EditorAction::LightingAddDefault)
        .unwrap();
    store
        .dispatch_editor_action(EditorAction::LightingEditColorHex {
            zone: LightingZone::Backlight,
            color_hex: "#AABBCC".into(),
            all_layers: false,
        })
        .unwrap();
    store
        .dispatch_at(
            WorkbenchCommand::Editor(EditorAction::LightingBeginPreview),
            t0,
        )
        .unwrap();
    wait_until(|| {
        store.poll_at(t0);
        lighting_modal(&store).preview_active
    });
    let h_select_layer = starter.last_handle();

    store
        .dispatch_canvas_action(CanvasAction::SelectLayer(layer1_id.clone()))
        .unwrap();
    let modal_after_layer = lighting_modal(&store);
    assert_eq!(modal_after_layer.profile_id, ag_id);
    assert_eq!(modal_after_layer.layer_id, layer1_id);
    assert!(!modal_after_layer.preview_active);
    assert_eq!(
        modal_after_layer
            .backlight
            .as_ref()
            .map(|z| z.color_hex.as_str()),
        Some("#112233")
    );
    wait_until(|| {
        store.poll_at(t0);
        h_select_layer.end_calls.load(Ordering::Acquire) == 1
    });
    wait_until(|| {
        store.poll_at(t0);
        store.refresh_four_app_lighting();
        store.lighting_arbiter.last_update_result == LightingUpdateResult::Applied
    });

    store
        .dispatch_canvas_action(CanvasAction::SelectLayer(layer2_id.clone()))
        .unwrap();
    assert_eq!(lighting_modal(&store).layer_id, layer2_id);
    store
        .dispatch_at(
            WorkbenchCommand::Editor(EditorAction::LightingBeginPreview),
            t0,
        )
        .unwrap();
    wait_until(|| {
        store.poll_at(t0);
        lighting_modal(&store).preview_active
    });
    let h_delete_layer = starter.last_handle();

    store
        .dispatch_inspector_action(InspectorAction::DeleteSelectedLayer)
        .unwrap();
    let modal_after_delete = lighting_modal(&store);
    assert_eq!(modal_after_delete.layer_id, layer1_id);
    assert!(!modal_after_delete.preview_active);
    wait_until(|| {
        store.poll_at(t0);
        h_delete_layer.end_calls.load(Ordering::Acquire) == 1
    });
    wait_until(|| {
        store.poll_at(t0);
        store.refresh_four_app_lighting();
        store.lighting_arbiter.last_update_result == LightingUpdateResult::Applied
    });

    store
        .dispatch_at(
            WorkbenchCommand::Editor(EditorAction::LightingBeginPreview),
            t0,
        )
        .unwrap();
    wait_until(|| {
        store.poll_at(t0);
        lighting_modal(&store).preview_active
    });
    let h_switch_editable = starter.last_handle();

    store
        .dispatch_sidebar_action(SidebarAction::SelectProfile(dsh_id.clone()))
        .unwrap();
    let modal_after_dsh = lighting_modal(&store);
    assert_eq!(modal_after_dsh.profile_id, dsh_id);
    assert_eq!(
        Some(modal_after_dsh.layer_id.as_str()),
        store.selected_layer_id()
    );
    assert!(!modal_after_dsh.preview_active);
    wait_until(|| {
        store.poll_at(t0);
        h_switch_editable.end_calls.load(Ordering::Acquire) == 1
    });
    wait_until(|| {
        store.poll_at(t0);
        store.refresh_four_app_lighting();
        store.lighting_arbiter.last_update_result == LightingUpdateResult::Applied
    });

    store
        .dispatch_editor_action(EditorAction::LightingAddDefault)
        .unwrap();
    store
        .dispatch_at(
            WorkbenchCommand::Editor(EditorAction::LightingBeginPreview),
            t0,
        )
        .unwrap();
    wait_until(|| {
        store.poll_at(t0);
        lighting_modal(&store).preview_active
    });
    let h_switch_protected = starter.last_handle();

    store
        .dispatch_sidebar_action(SidebarAction::SelectProfile(codex_id))
        .unwrap();
    assert!(store.is_selected_layer_protected());
    assert!(
        store.active_modal().is_none(),
        "switching to protected profile must close open LightingEditor"
    );
    wait_until(|| {
        store.poll_at(t0);
        h_switch_protected.end_calls.load(Ordering::Acquire) == 1
    });

    store
        .dispatch_sidebar_action(SidebarAction::SelectProfile(dsh_id))
        .unwrap();
    store
        .dispatch_editor_action(EditorAction::OpenLightingEditor)
        .unwrap();
    store
        .dispatch_at(
            WorkbenchCommand::Editor(EditorAction::LightingBeginPreview),
            t0,
        )
        .unwrap();
    wait_until(|| {
        store.poll_at(t0);
        lighting_modal(&store).preview_active
    });
    let h_snapshot = starter.last_handle();
    store
        .dispatch_device_workflow_action(mi_core::DeviceWorkflowAction::ReadSnapshot)
        .unwrap();
    wait_until(|| {
        store.poll_at(t0);
        h_snapshot.end_calls.load(Ordering::Acquire) == 1
    });
    if let Some(EditorModalProjection::LightingEditor(modal)) = store.active_modal() {
        assert_eq!(Some(modal.profile_id.as_str()), store.selected_profile_id());
        assert_eq!(Some(modal.layer_id.as_str()), store.selected_layer_id());
        assert!(!modal.preview_active);
    }
}

#[test]
fn lighting_preview_hiding_other_profiles_ends_preview_and_refreshes_open_modal() {
    let ws = TempWorkspace::new("lighting-hide-other-profiles");
    let starter = FakePreviewStarter::new(Duration::from_secs(30));
    let mut cfg = WorkbenchStoreConfig::isolated(&ws.dir);
    cfg.lighting_preview_starter = Arc::clone(&starter) as Arc<dyn LightingPreviewStarter>;
    let mut store = WorkbenchStore::open(cfg).expect("open store with fake preview starter");
    let t0 = Instant::now();

    store
        .dispatch_sidebar_action(SidebarAction::Profile(mi_core::ProfileCommand::Create {
            name: "Custom Studio".into(),
        }))
        .unwrap();
    let custom_pid = store.selected_profile_id().unwrap().to_string();

    store
        .dispatch_editor_action(EditorAction::OpenLightingEditor)
        .unwrap();
    store
        .dispatch_editor_action(EditorAction::LightingAddDefault)
        .unwrap();
    store
        .dispatch_at(
            WorkbenchCommand::Editor(EditorAction::LightingBeginPreview),
            t0,
        )
        .unwrap();
    wait_until(|| {
        store.poll_at(t0);
        lighting_modal(&store).preview_active
    });
    assert_eq!(lighting_modal(&store).profile_id, custom_pid);
    let handle = starter.last_handle();

    store
        .dispatch_sidebar_action(SidebarAction::SetShowOtherProfiles(false))
        .unwrap();
    assert_ne!(store.selected_profile_id(), Some(custom_pid.as_str()));
    wait_until(|| {
        store.poll_at(t0);
        handle.end_calls.load(Ordering::Acquire) == 1
    });

    if store.is_selected_layer_protected() {
        assert!(store.active_modal().is_none());
    } else {
        let refreshed = lighting_modal(&store);
        assert_eq!(
            Some(refreshed.profile_id.as_str()),
            store.selected_profile_id()
        );
        assert_eq!(Some(refreshed.layer_id.as_str()), store.selected_layer_id());
        assert!(!refreshed.preview_active);
    }
}

#[test]
fn multi_editor_four_branch_actions_and_macros_lifecycle() {
    let ws = TempWorkspace::new("multi-editor");
    let mut store = WorkbenchStore::open_isolated(&ws.dir).expect("open isolated workbench");

    store
        .dispatch_editor_action(EditorAction::OpenMacroEditor { macro_id: None })
        .expect("open macro editor for multi branch target");
    store
        .dispatch_editor_action(EditorAction::MacroSetName("双击宏动作".into()))
        .expect("set macro name");
    store
        .dispatch_editor_action(EditorAction::MacroAddTapKey {
            key: "a".into(),
            delay_ms: 25,
        })
        .expect("add tap key a");
    store
        .dispatch_editor_action(EditorAction::MacroSave {
            bind_to_selected_slot: false,
        })
        .expect("save macro without binding");
    let macro_id = store.projection().inspector.editable().unwrap().macros[0]
        .id
        .clone();

    store
        .dispatch_inspector_action(InspectorAction::SelectSlot(8))
        .expect("select slot 8");
    store
        .dispatch_editor_action(EditorAction::OpenMultiEditor { multi_id: None })
        .expect("open new multi editor");
    store
        .dispatch_editor_action(EditorAction::MultiSetTermMs(0))
        .expect("set invalid 0ms term");
    assert!(
        store
            .dispatch_editor_action(EditorAction::MultiSave {
                bind_to_selected_slot: true,
            })
            .is_err(),
        "term_ms == 0 must fail MultiSave"
    );

    store
        .dispatch_editor_action(EditorAction::MultiSetName("四分支复合键".into()))
        .expect("set multi name");
    store
        .dispatch_editor_action(EditorAction::MultiSetTermMs(220))
        .expect("set valid 220ms term");
    store
        .dispatch_editor_action(EditorAction::MultiSetBranch {
            branch: MultiBranchKind::Tap,
            target: Some(MultiBranchTarget::Action {
                action_id: "save".into(),
            }),
        })
        .expect("set Tap branch");
    store
        .dispatch_editor_action(EditorAction::MultiSetBranch {
            branch: MultiBranchKind::DoubleTap,
            target: Some(MultiBranchTarget::Action {
                action_id: "search".into(),
            }),
        })
        .expect("set DoubleTap branch");
    store
        .dispatch_editor_action(EditorAction::MultiSetBranch {
            branch: MultiBranchKind::Hold,
            target: Some(MultiBranchTarget::Action {
                action_id: "apple-fn-key".into(),
            }),
        })
        .expect("set Hold branch");
    store
        .dispatch_editor_action(EditorAction::MultiSetBranch {
            branch: MultiBranchKind::TapHold,
            target: Some(MultiBranchTarget::Macro {
                action_id: macro_id.clone(),
            }),
        })
        .expect("set TapHold branch to saved macro");

    store
        .dispatch_editor_action(EditorAction::MultiSave {
            bind_to_selected_slot: true,
        })
        .expect("save multi and bind to slot 8");
    assert!(store.active_modal().is_none());

    let editable = store.projection().inspector.editable().unwrap();
    assert_eq!(editable.bound_action_name, "四分支复合键");
    assert_eq!(editable.multis.len(), 1);
    let saved_multi = &editable.multis[0];
    assert!(saved_multi.is_multi);
    assert_eq!(saved_multi.reference_count, 1);
    let multi_id = saved_multi.id.clone();

    store
        .dispatch_editor_action(EditorAction::OpenMultiEditor {
            multi_id: Some(multi_id.clone()),
        })
        .expect("reopen saved multi editor");
    if let Some(EditorModalProjection::MultiEditor(modal)) = store.active_modal() {
        assert_eq!(modal.name, "四分支复合键");
        assert_eq!(modal.term_ms, 220);
        assert!(modal.branches.iter().all(|b| b.target.is_some()));
    } else {
        panic!("expected MultiEditor modal");
    }
    store
        .dispatch_editor_action(EditorAction::CloseModal)
        .expect("close modal");

    assert!(
        store
            .dispatch_editor_action(EditorAction::MultiDelete {
                multi_id: multi_id.clone(),
            })
            .is_err(),
        "deleting a multi while bound to a slot must be rejected"
    );
    store
        .dispatch_editor_action(EditorAction::MultiBindExisting { multi_id: None })
        .expect("unbind multi from slot 8");
    store
        .dispatch_editor_action(EditorAction::MultiDelete { multi_id })
        .expect("delete unbound multi");
    assert!(store
        .projection()
        .inspector
        .editable()
        .unwrap()
        .multis
        .is_empty());
}

#[test]
fn reconnect_comparison_and_apply_preparation_backup_gate() {
    let ws = TempWorkspace::new("comparison-and-apply");
    let mut store = WorkbenchStore::open_isolated(&ws.dir).expect("open isolated workbench");
    store
        .document_store
        .open(ws.dir.join("workbench.json"))
        .expect("open disk-backed workbench document store");

    let base_config = mi_model::demo().expect("load demo device config");
    let base_snapshot = build_verified_device_snapshot(&base_config, 1);
    store
        .document_store
        .create_independent(base_config.clone(), base_snapshot.clone())
        .expect("create independent device-backed draft");

    let pid = store
        .document_store
        .draft
        .document
        .editable
        .graph
        .profile_order[0]
        .clone();
    store
        .document_store
        .update(|draft| draft.rename_profile(&pid, "本地改名方案"))
        .expect("rename profile locally");

    let mut clean_remote = base_config.clone();
    clean_remote.profiles[0].layers[0].name = "设备改名层".into();
    let clean_snapshot = build_verified_device_snapshot(&clean_remote, 1);

    store
        .dispatch_editor_action(EditorAction::OpenReconnectComparison {
            device_config: Box::new(clean_remote.clone()),
            baseline: clean_snapshot.clone(),
        })
        .expect("open clean three-way comparison");
    if let Some(EditorModalProjection::ReconnectComparison(modal)) = store.active_modal() {
        assert!(
            modal.can_merge,
            "disjoint profile vs layer rename should merge cleanly"
        );
        assert!(modal.conflicts.is_empty());
    } else {
        panic!("expected ReconnectComparison modal");
    }
    store
        .dispatch_editor_action(EditorAction::ComparisonResolve {
            choice: ComparisonChoice::Merge,
            current_snapshot: clean_snapshot,
        })
        .expect("resolve clean comparison with Merge");
    assert!(store.active_modal().is_none());

    let mut conflict_remote = clean_remote.clone();
    conflict_remote.profiles[0].name = "设备侧冲突方案名".into();
    let conflict_snapshot = build_verified_device_snapshot(&conflict_remote, 1);
    store
        .dispatch_editor_action(EditorAction::OpenReconnectComparison {
            device_config: Box::new(conflict_remote.clone()),
            baseline: conflict_snapshot.clone(),
        })
        .expect("open conflicting three-way comparison");
    if let Some(EditorModalProjection::ReconnectComparison(modal)) = store.active_modal() {
        assert!(!modal.can_merge);
        assert!(!modal.conflicts.is_empty());
    } else {
        panic!("expected ReconnectComparison modal");
    }
    assert!(
        store
            .dispatch_editor_action(EditorAction::ComparisonResolve {
                choice: ComparisonChoice::Merge,
                current_snapshot: conflict_snapshot.clone(),
            })
            .is_err(),
        "Choice::Merge must be rejected when conflicts exist"
    );

    let usb_base_snapshot = build_observed_interface_snapshot(&clean_remote, 1, "usb-if-0");
    store
        .document_store
        .create_independent(clean_remote.clone(), usb_base_snapshot)
        .expect("create draft with observed-interface baseline");
    let pid_usb = store
        .document_store
        .draft
        .document
        .editable
        .graph
        .profile_order[0]
        .clone();
    store
        .document_store
        .update(|draft| draft.rename_profile(&pid_usb, "本地接口方案"))
        .expect("rename profile locally before USB interface change");

    let mut usb_changed_remote = clean_remote.clone();
    usb_changed_remote.profiles[0].name = "重插 USB 接口方案".into();
    let usb_changed_snapshot =
        build_observed_interface_snapshot(&usb_changed_remote, 2, "usb-if-1");
    store
        .dispatch_editor_action(EditorAction::OpenReconnectComparison {
            device_config: Box::new(usb_changed_remote),
            baseline: usb_changed_snapshot.clone(),
        })
        .expect("open comparison with changed USB interface");
    if let Some(EditorModalProjection::ReconnectComparison(modal)) = store.active_modal() {
        assert!(modal.identity_confirmation_required);
        assert!(!modal.identity_user_confirmed);
        assert!(!modal.can_merge);
    } else {
        panic!("expected ReconnectComparison modal");
    }
    assert!(
        store
            .dispatch_editor_action(EditorAction::ComparisonResolve {
                choice: ComparisonChoice::KeepLocal,
                current_snapshot: usb_changed_snapshot.clone(),
            })
            .is_err(),
        "KeepLocal must be blocked before ComparisonConfirmIdentity"
    );
    store
        .dispatch_editor_action(EditorAction::ComparisonConfirmIdentity {
            current_snapshot: usb_changed_snapshot.clone(),
        })
        .expect("confirm USB interface identity change");
    if let Some(EditorModalProjection::ReconnectComparison(modal)) = store.active_modal() {
        assert!(modal.identity_user_confirmed);
        assert!(
            !modal.can_merge,
            "confirming identity must still keep automatic Merge disabled"
        );
    }
    store
        .dispatch_editor_action(EditorAction::ComparisonResolve {
            choice: ComparisonChoice::KeepLocal,
            current_snapshot: usb_changed_snapshot,
        })
        .expect("resolve with KeepLocal after identity confirmation");
    assert!(store.active_modal().is_none());

    let mut raw_demo = mi_model::demo().expect("load demo config for apply").raw;
    raw_demo["version"] = json!(1);
    let raw_demo_bytes = serde_json::to_vec(&raw_demo).expect("serialize raw demo for apply");
    let apply_base_config =
        mi_model::parse_device_files(&raw_demo_bytes, None).expect("parse device config for apply");
    let apply_snapshot = build_verified_device_snapshot(&apply_base_config, 10);
    store
        .document_store
        .create_independent(apply_base_config, apply_snapshot.clone())
        .expect("create clean device-backed draft for apply");
    let apply_pid = store.document_store.draft.config.profiles[0].id.clone();
    let apply_lid = store.document_store.draft.config.profiles[0].layers[0]
        .id
        .clone();
    let device_key_action = mi_model::draft::actions()
        .into_iter()
        .find(|a| a.id == "device-key")
        .expect("device-key action in catalog");
    store
        .document_store
        .update(|draft| draft.assign(&apply_pid, &apply_lid, 0, device_key_action))
        .expect("assign KC_B device key to slot 0");

    store
        .dispatch_editor_action(EditorAction::OpenApplyPreparation {
            current_snapshot: apply_snapshot.clone(),
        })
        .expect("open apply preparation modal");
    if let Some(EditorModalProjection::ApplyPreparation(modal)) = store.active_modal() {
        assert!(!modal.backup_prepared);
        assert!(!modal.candidate_taken);
        assert!(!modal.changes.is_empty());
    } else {
        panic!("expected ApplyPreparation modal");
    }

    assert!(
        store
            .dispatch_editor_action(EditorAction::ApplyTakePreparedCandidate {
                current_snapshot: apply_snapshot.clone(),
            })
            .is_err(),
        "ApplyTakePreparedCandidate must fail before ApplyPrepareBackup"
    );

    store
        .dispatch_editor_action(EditorAction::ApplyPrepareBackup {
            current_snapshot: apply_snapshot.clone(),
        })
        .expect("prepare on-disk backup before apply");
    if let Some(EditorModalProjection::ApplyPreparation(modal)) = store.active_modal() {
        assert!(modal.backup_prepared);
        assert!(modal.backup_path.is_some());
        assert!(modal.manifest_sha256.is_some());
    }

    store
        .dispatch_editor_action(EditorAction::ApplyTakePreparedCandidate {
            current_snapshot: apply_snapshot,
        })
        .expect("take prepared candidate after backup is verified");
    if let Some(EditorModalProjection::ApplyPreparation(modal)) = store.active_modal() {
        assert!(modal.candidate_taken);
        assert!(modal.candidate_operation_id.is_some());
    }
}

#[test]
fn joystick_dual_mode_inspector_projects_summary_and_preserves_dormant_slots() {
    let ws = TempWorkspace::new("joystick-dual-mode");
    let mut store = WorkbenchStore::open_isolated(&ws.dir).expect("open isolated store");
    store
        .dispatch_inspector_action(InspectorAction::CreateCustomLayer {
            name: "摇杆模式测试层".into(),
        })
        .expect("create custom editable layer");
    store
        .dispatch_inspector_action(InspectorAction::SelectSlot(16))
        .expect("select joystick north slot");

    store
        .dispatch_inspector_action(InspectorAction::SetJoystickMode(
            mi_core::JoystickControlMode::LogicalSectors,
        ))
        .expect("switch to logical sectors");
    store
        .dispatch_inspector_action(InspectorAction::AssignCatalogAction("search".into()))
        .expect("assign search action to joystick sector 16");
    let before = store
        .projection()
        .inspector
        .editable()
        .expect("editable inspector")
        .clone();
    assert_eq!(
        before.joystick_mode,
        mi_core::JoystickControlMode::LogicalSectors
    );
    assert!(before.joystick_mode_summary.contains("宿主 CGEvent"));
    assert!(before.v070_hardware_mouse_supported);
    assert_eq!(store.projection().canvas.joystick.mode_label, "8向摇杆");
    assert_eq!(before.bound_action_id.as_deref(), Some("search"));

    store
        .dispatch_inspector_action(InspectorAction::SetJoystickMode(
            mi_core::JoystickControlMode::Mouse,
        ))
        .expect("switch to hardware mouse");
    let mouse = store
        .projection()
        .inspector
        .editable()
        .expect("editable inspector");
    assert_eq!(mouse.joystick_mode, mi_core::JoystickControlMode::Mouse);
    assert!(mouse.joystick_mode_summary.contains("/fs/keymap.json"));
    assert_eq!(store.projection().canvas.joystick.mode_label, "硬件指针");

    store
        .dispatch_inspector_action(InspectorAction::SetJoystickMode(
            mi_core::JoystickControlMode::Scroll,
        ))
        .expect("switch to hardware scroll");
    let scroll = store
        .projection()
        .inspector
        .editable()
        .expect("editable inspector");
    assert_eq!(scroll.joystick_mode, mi_core::JoystickControlMode::Scroll);
    assert_eq!(store.projection().canvas.joystick.mode_label, "硬件滚轮");

    store
        .dispatch_inspector_action(InspectorAction::SetJoystickMode(
            mi_core::JoystickControlMode::Source,
        ))
        .expect("switch to source mode");
    let source = store
        .projection()
        .inspector
        .editable()
        .expect("editable inspector");
    assert_eq!(source.joystick_mode, mi_core::JoystickControlMode::Source);
    assert_eq!(store.projection().canvas.joystick.mode_label, "8向摇杆");

    store
        .dispatch_inspector_action(InspectorAction::SetJoystickMode(
            mi_core::JoystickControlMode::LogicalSectors,
        ))
        .expect("switch back to logical sectors");
    let restored = store
        .projection()
        .inspector
        .editable()
        .expect("editable inspector");
    assert_eq!(
        restored.joystick_mode,
        mi_core::JoystickControlMode::LogicalSectors
    );
    assert_eq!(
        restored.bound_action_id.as_deref(),
        Some("search"),
        "dormant joystick sector assignment must survive mode roundtrip"
    );
}

#[test]
fn desktop_browser_infers_nav_keycap_for_slot_8() {
    let browser = mi_model::draft::Action {
        id: "desktop-browser".into(),
        name: "打开内置浏览器".into(),
        category: "Agent".into(),
        detail: "切换右侧内置浏览器与预览面板（Cmd+Shift+B 或原生控件）".into(),
        payload: json!({
            "type": "agent_command",
            "adapter": "antigravity",
            "command": "browser"
        }),
    };
    assert_eq!(
        CodexKeycapId::infer_for_slot(8, Some(&browser)),
        CodexKeycapId::Nav
    );
}
