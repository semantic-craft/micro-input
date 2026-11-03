use mi_model::{
    document::LocalDocumentV2,
    draft::{Action, Draft},
    lighting::{self, Edit, Effect, Zone},
    storage::DocumentStore,
};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
const RAW: &[u8] = include_bytes!("../fixtures/device/synthetic-keymap-lighting.json");
fn baseline(bytes: &[u8]) -> Value {
    json!({"device_identity":{"confidence":"verified-serial","serial":"synthetic","session_id":4},"firmware":"synthetic-0.6.3","keymap":{"state":"present","size":bytes.len(),"sha1":format!("{:x}",Sha1::digest(bytes)),"bytes":bytes},"smart_actions":{"state":"missing"}})
}
fn setup() -> (tempfile::TempDir, DocumentStore, Value, String, String) {
    let config = mi_model::parse_device_files(RAW, None).unwrap();
    let base = baseline(RAW);
    let doc = LocalDocumentV2::from_config(&config, Some(base.clone()), "device-snapshot").unwrap();
    let p = doc.editable.graph.profile_order[0].clone();
    let l = doc.editable.graph.profiles[&p].layer_order[0].clone();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("draft.json");
    std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
    let mut store = DocumentStore::default();
    store.open(path).unwrap();
    (dir, store, base, p, l)
}
#[test]
fn lighting_edits_save_undo_copy_and_presets_preserve_unknown_and_other_zone() {
    let (dir, mut store, _, p, l) = setup();
    let original = store.draft.document.lighting(&l).unwrap();
    let bytes = std::fs::read(dir.path().join("draft.json")).unwrap();
    assert!(store.draft.document.editable.graph.layers[&l]
        .lighting
        .is_none());
    assert_eq!(std::fs::read(dir.path().join("draft.json")).unwrap(), bytes);
    store
        .update(|d| d.edit_lighting(&p, &l, Zone::Backlight, Edit::Color(0xabcdef), false))
        .unwrap();
    let changed = store.draft.document.lighting(&l).unwrap();
    assert_eq!(changed.underglow, original.underglow);
    assert_eq!(changed.backlight["future"], original.backlight["future"]);
    store.update(|d| d.undo(false)).unwrap();
    assert!(store.draft.document.editable.graph.layers[&l]
        .lighting
        .is_none());
    store.update(|d| d.undo(true)).unwrap();
    assert_eq!(store.draft.document.lighting(&l).unwrap(), changed);
    let mut reopened = DocumentStore::default();
    reopened.open(dir.path().join("draft.json")).unwrap();
    assert_eq!(reopened.draft.document.lighting(&l).unwrap(), changed);
    let copy = reopened.draft.duplicate_layer(&p, &l, "Copy").unwrap();
    assert_eq!(reopened.draft.document.lighting(&copy).unwrap(), changed);
    let preset = mi_model::preset::export(&reopened.draft, Some(&p), Some(&l)).unwrap();
    let imported = mi_model::preset::import(&preset).unwrap();
    assert!(imported.document.baseline.is_none());
    assert!(imported
        .document
        .editable
        .graph
        .layers
        .values()
        .any(|l| l.lighting.as_ref() == Some(&changed)));
    assert_eq!(lighting::color_from_hex("#AbC123").unwrap(), 0xabc123);
    assert_eq!(lighting::color_to_hex(0x000a01).unwrap(), "#000A01");
    assert!(lighting::color_from_hex("#abc").is_err());
    assert!(lighting::color_to_hex(0x1000000).is_err());
}
#[test]
fn candidate_is_exact_field_patch_requires_backup_and_can_only_be_taken_once() {
    let (dir, mut store, base, p, l) = setup();
    store
        .update(|d| d.edit_lighting(&p, &l, Zone::Backlight, Edit::Color(0xabcdef), false))
        .unwrap();
    let preview = store.preview_apply(&base).unwrap();
    assert_eq!(preview.kind, "lighting");
    assert!(!preview.apply_available);
    assert!(store
        .take_prepared_candidate(&preview.token, &base)
        .is_err());
    store.prepare_apply_backup(&preview.token, &base).unwrap();
    let plan = store
        .take_prepared_candidate(&preview.token, &base)
        .unwrap();
    let expected = String::from_utf8(RAW.to_vec())
        .unwrap()
        .replacen("1193046", "11259375", 1);
    assert_eq!(plan.candidate(), expected.as_bytes());
    assert_eq!(plan.original(), RAW);
    assert_eq!(plan.session_id(), 4);
    assert!(store
        .take_prepared_candidate(&preview.token, &base)
        .is_err());
    assert_eq!(
        std::fs::read(
            dir.path()
                .join("device-backups")
                .join(&preview.token)
                .join("keymap.json")
        )
        .unwrap(),
        RAW
    );
}
#[test]
fn invalid_or_mixed_changes_rejected_and_noop_creates_no_revision() {
    let (_dir, mut store, base, p, l) = setup();
    let rev = store.draft.revision;
    store
        .update(|d| d.edit_lighting(&p, &l, Zone::Backlight, Edit::Color(1193046), false))
        .unwrap();
    assert_eq!(store.draft.revision, rev);
    assert!(store
        .update(|d| d.edit_lighting(&p, &l, Zone::Backlight, Edit::Speed(40), false))
        .is_err());
    assert!(store
        .update(|d| d.edit_lighting(&p, &l, Zone::Backlight, Edit::Brightness(101), false))
        .is_err());
    store
        .update(|d| d.edit_lighting(&p, &l, Zone::Backlight, Edit::Brightness(25), false))
        .unwrap();
    store
        .update(|d| {
            d.assign(
                &json!(p),
                &json!(l),
                0,
                Action {
                    id: "key".into(),
                    name: "Key".into(),
                    category: "Device".into(),
                    detail: "local".into(),
                    payload: json!({"type":"device_key","token":"KC_B"}),
                },
            )
        })
        .unwrap();
    assert!(store.preview_apply(&base).is_err());
    let mut unknown = store.draft.document.lighting(&l).unwrap();
    unknown.backlight["effect"] = json!("future-effect");
    assert!(unknown
        .edit(Zone::Backlight, Edit::Effect(Effect::Solid))
        .is_err());
}
#[test]
fn ag_original_is_allowed_only_at_known_unique_position() {
    let (_dir, mut store, base, p, l) = setup();
    store
        .update(|d| {
            d.assign(
                &json!(p),
                &json!(l),
                0,
                Action {
                    id: "key".into(),
                    name: "Key".into(),
                    category: "Device".into(),
                    detail: "local".into(),
                    payload: json!({"type":"device_key","token":"KC_B"}),
                },
            )
        })
        .unwrap();
    let preview = store.preview_apply(&base).unwrap();
    assert_eq!(preview.before, "KV_OAI_AG01");
    store.prepare_apply_backup(&preview.token, &base).unwrap();
    let plan = store
        .take_prepared_candidate(&preview.token, &base)
        .unwrap();
    assert_eq!(
        plan.candidate(),
        String::from_utf8(RAW.to_vec())
            .unwrap()
            .replacen("KV_OAI_AG01", "KC_B", 1)
            .as_bytes()
    );
}
#[test]
fn three_way_lighting_merge_and_new_baseline_do_not_repeat_applied_differences() {
    use mi_model::comparison::{Choice, Comparison};
    let (_dir, mut store, _, p, l) = setup();
    store
        .draft
        .edit_lighting(&p, &l, Zone::Backlight, Edit::Color(0xabcdef), false)
        .unwrap();
    let raw = String::from_utf8(RAW.to_vec())
        .unwrap()
        .replace("\"brightness\":0.6", "\"brightness\":0.2");
    let config = mi_model::parse_device_files(raw.as_bytes(), None).unwrap();
    let cmp = Comparison::new(&store.draft.document, &config, baseline(raw.as_bytes())).unwrap();
    assert!(cmp.can_merge(), "{:?}", cmp.conflicts);
    let merged = cmp
        .resolve(&store.draft.document, Choice::Merge)
        .unwrap()
        .unwrap();
    assert_eq!(merged.lighting(&l).unwrap().backlight["color"], 0xabcdef);
    assert_eq!(merged.lighting(&l).unwrap().underglow["brightness"], 0.2);
    let applied = raw.replace("1193046", "11259375");
    let config = mi_model::parse_device_files(applied.as_bytes(), None).unwrap();
    let cmp = Comparison::new(&merged, &config, baseline(applied.as_bytes())).unwrap();
    let accepted = cmp.resolve(&merged, Choice::UseDevice).unwrap().unwrap();
    assert!(accepted.structure_changes().is_empty());
    let changed = raw.replace("\"x\":true", "\"x\":false");
    let config = mi_model::parse_device_files(changed.as_bytes(), None).unwrap();
    let cmp = Comparison::new(&merged, &config, baseline(changed.as_bytes())).unwrap();
    assert!(!cmp.can_merge());
}
// Frozen pre-lighting v2 layer decoder and the old round-trip rule. Do not replace
// OldLayer with the current LayerNode: that would stop testing old-reader behavior.
#[derive(serde::Serialize, serde::Deserialize)]
struct OldLayer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    control_settings: Option<mi_model::control_settings::ControlSettings>,
    name: String,
    control_order: Vec<String>,
    controls: std::collections::BTreeMap<String, mi_model::document::BindingNode>,
}
#[test]
fn frozen_old_layer_reader_detects_new_field_and_new_reader_retains_unknowns() {
    let (_dir, mut store, _, p, l) = setup();
    let old = json!(store.draft.document.editable.graph.layers[&l]);
    let decoded: OldLayer = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(json!(decoded), old);
    store
        .draft
        .edit_lighting(&p, &l, Zone::Backlight, Edit::Color(7), false)
        .unwrap();
    let new = json!(store.draft.document.editable.graph.layers[&l]);
    let decoded: OldLayer = serde_json::from_value(new.clone()).unwrap();
    assert_ne!(
        json!(decoded),
        new,
        "old round-trip mismatch activates read-only"
    );
    let mut raw = json!(store.draft.document);
    raw["graph"]["layers"][&l]["future_local_field"] = json!(true);
    let mut future = Draft::from_bytes(&serde_json::to_vec(&raw).unwrap()).unwrap();
    assert!(future.read_only);
    assert!(future
        .edit_lighting(&p, &l, Zone::Backlight, Edit::Color(8), false)
        .is_err());
    assert!(mi_model::preset::export(&future, None, None).is_err());
}

#[test]
fn all_layer_edit_is_atomic_and_copies_are_kept_by_preset_cleanup() {
    let (_dir, mut store, _, p, l) = setup();
    let second = store.draft.duplicate_layer(&p, &l, "second").unwrap();
    let before = serde_json::to_value(&store.draft.document.editable).unwrap();
    store
        .draft
        .edit_lighting(&p, &l, Zone::Backlight, Edit::Brightness(50), true)
        .unwrap();
    assert_eq!(
        store.draft.document.lighting(&l).unwrap().backlight["brightness"],
        0.5
    );
    assert_eq!(
        store.draft.document.lighting(&second).unwrap().backlight["brightness"],
        0.5
    );
    store.draft.undo(false).unwrap();
    assert_eq!(
        serde_json::to_value(&store.draft.document.editable).unwrap(),
        before
    );
    let mut demo = Draft::default();
    let pid = demo.document.editable.graph.profile_order[0].clone();
    let lid = demo.document.editable.graph.profiles[&pid].layer_order[0].clone();
    let target = store.draft.document.lighting(&l).unwrap();
    demo.mutate(|d| {
        d.editable.graph.layers.get_mut(&lid).unwrap().lighting = Some(target);
        Ok(())
    })
    .unwrap();
    mi_model::preset::install_agent_starter_profiles(&mut demo).unwrap();
    assert!(demo.document.editable.graph.profiles.contains_key(&pid));
    assert!(demo.document.editable.graph.layers[&lid].lighting.is_some());
}
#[test]
fn compiler_rejects_unknown_field_edits_missing_structure_and_invalid_sources() {
    let (_dir, mut store, base, p, l) = setup();
    store
        .update(|d| d.edit_lighting(&p, &l, Zone::Backlight, Edit::Color(7), false))
        .unwrap();
    store
        .update(|d| {
            d.mutate(|doc| {
                doc.editable
                    .graph
                    .layers
                    .get_mut(&l)
                    .unwrap()
                    .lighting
                    .as_mut()
                    .unwrap()
                    .backlight["future"] = json!(false);
                Ok(())
            })
        })
        .unwrap();
    assert!(store.preview_apply(&base).is_err());
    let (_dir, mut store, base, p, l) = setup();
    store
        .update(|d| d.edit_lighting(&p, &l, Zone::Backlight, Edit::Color(7), false))
        .unwrap();
    store
        .update(|d| {
            d.mutate(|doc| {
                doc.editable.source_refs.get_mut(&l).unwrap()["source_id"] = json!(999);
                Ok(())
            })
        })
        .unwrap();
    assert!(store.preview_apply(&base).is_err());
    let mut missing = lighting::Lighting {
        backlight: Value::Null,
        underglow: json!({"effect":"solid"}),
    };
    assert!(missing
        .edit(Zone::Backlight, Edit::Effect(Effect::Solid))
        .is_err());
    assert!(missing.edit(Zone::Underglow, Edit::Color(1)).is_err());
}

#[test]
fn invalid_existing_parameter_is_preserved_instead_of_normalized() {
    let mut lights = lighting::Lighting {
        backlight: json!({"effect":"solid","color":"future-color"}),
        underglow: Value::Null,
    };
    let original = lights.clone();
    assert!(lights.edit(Zone::Backlight, Edit::Color(1)).is_err());
    assert_eq!(lights, original);
}

#[test]
fn unchanged_full_lighting_target_does_not_block_later_key_candidate() {
    let (_dir, mut store, base, p, l) = setup();
    store
        .update(|d| {
            d.mutate(|doc| {
                let target = doc.lighting(&l)?;
                doc.editable.graph.layers.get_mut(&l).unwrap().lighting = Some(target);
                Ok(())
            })
        })
        .unwrap();
    store
        .update(|d| {
            d.assign(
                &json!(p),
                &json!(l),
                1,
                Action {
                    id: "key".into(),
                    name: "Key".into(),
                    category: "Device".into(),
                    detail: "local".into(),
                    payload: json!({"type":"device_key","token":"KC_B"}),
                },
            )
        })
        .unwrap();
    assert_eq!(
        store.preview_apply(&base).unwrap().kind,
        "single_device_key"
    );
}
