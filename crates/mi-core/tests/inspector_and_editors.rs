use mi_core::{
    CanonicalAgent, CodexKeycapId, EditorAction, EditorModalProjection, InspectorAction,
    InspectorBodyProjection, InspectorDrawer, KeycapRole, MultiBranchKind, MultiBranchTarget,
    ShortcutModifier, SidebarAction, WorkbenchError, WorkbenchStore, INSPECTOR_WIDTH,
};
use mi_model::{
    comparison::Choice as ComparisonChoice,
    lighting::{Effect as LightingEffect, Zone as LightingZone},
    macro_sequence::MacroEvent,
};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

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

    store
        .dispatch_editor_action(EditorAction::LightingBeginPreview)
        .expect("begin 30s hardware lighting preview");
    if let Some(EditorModalProjection::LightingEditor(modal)) = store.active_modal() {
        assert!(modal.has_lighting);
        assert!(modal.preview_active);
        assert_eq!(modal.preview_remaining_secs, 30);
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
        .dispatch_editor_action(EditorAction::LightingEndPreview)
        .expect("end lighting preview");
    if let Some(EditorModalProjection::LightingEditor(modal)) = store.active_modal() {
        assert!(!modal.preview_active);
        assert_eq!(modal.preview_remaining_secs, 0);
    }

    store
        .dispatch_editor_action(EditorAction::LightingBeginPreview)
        .expect("restart lighting preview before switching to protected profile");
    store
        .dispatch_inspector_action(InspectorAction::StartShortcutRecording)
        .expect("start shortcut recording before switching to protected profile");

    let cindy_id = store
        .find_canonical_profile_id(CanonicalAgent::Cindy)
        .expect("Cindy canonical profile must exist");
    store
        .dispatch_sidebar_action(SidebarAction::SelectProfile(cindy_id))
        .expect("switch to protected Cindy profile");

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
        .dispatch_inspector_action(InspectorAction::UnlockProtectedLayer)
        .expect("unlock by creating and switching to custom layer");
    assert!(!store.is_selected_layer_protected());
    assert!(store.projection().inspector.editable().is_some());
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
