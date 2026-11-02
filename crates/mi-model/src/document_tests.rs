//! R-E07/R-E08/R-K05 regression checks against data loss and wrong-target edits.
use crate::{draft::*, storage::DocumentStore};
use serde_json::json;
use std::{fs, path::PathBuf};
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "mi-document-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn file(&self) -> PathBuf {
        self.0.join("workbench.json")
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn assign(d: &mut Draft) -> Result<(), String> {
    let p = d.config.profiles[0].id.clone();
    let l = d.config.profiles[0].layers[0].id.clone();
    d.assign(&p, &l, 1, actions()[1].clone())
}
#[test]
fn stable_ids_survive_reorder_and_rename() {
    let mut d = Draft::default();
    assign(&mut d).unwrap();
    let original = d.edits.clone();
    d.config.profiles.reverse();
    for p in &mut d.config.profiles {
        p.layers.reverse();
        p.name = "Renamed".into();
    }
    d.validate().unwrap();
    assert_eq!(d.edits, original);
    let mut config = d.config.clone();
    config.profiles[1].id = config.profiles[0].id.clone();
    assert!(Draft::new(config).is_err());
    let p = d.config.profiles[0].id.clone();
    assert!(d
        .assign(&p, &json!(99999), 0, actions()[0].clone())
        .is_err());
}
#[test]
fn restart_preserves_parameters_baseline_and_undo_save() {
    let t = Temp::new();
    let mut s = DocumentStore::default();
    s.open(t.file()).unwrap();
    let baseline = s.draft.config.raw.clone();
    s.update(|d| {
        let p = d.config.profiles[0].id.clone();
        let l = d.config.profiles[0].layers[0].id.clone();
        let mut a = actions()[2].clone();
        a.payload = json!({"type":"text","text":"自有提示\n第二行"});
        d.assign(&p, &l, 2, a)
    })
    .unwrap();
    let mut re = DocumentStore::default();
    re.open(t.file()).unwrap();
    assert_eq!(re.draft.edits, s.draft.edits);
    assert_eq!(re.draft.config.raw, baseline);
    assert!(!re.draft.can_undo);
    s.update(|d| d.undo(false)).unwrap();
    re.open(t.file()).unwrap();
    assert!(re.draft.edits.is_empty());
    s.update(|d| d.undo(true)).unwrap();
    re.open(t.file()).unwrap();
    assert_eq!(re.draft.edits.len(), 1);
}
#[test]
fn corrupt_future_and_unknown_files_are_never_overwritten() {
    let t = Temp::new();
    for bytes in [
        b"{broken".to_vec(),
        serde_json::to_vec(&json!({"schema_version":99})).unwrap(),
    ] {
        fs::write(t.file(), &bytes).unwrap();
        let mut s = DocumentStore::default();
        assert!(s.open(t.file()).is_err());
        assert!(s.update(assign).is_err());
        assert_eq!(fs::read(t.file()).unwrap(), bytes);
    }
    let mut d = Draft::default();
    assign(&mut d).unwrap();
    let mut raw = serde_json::to_value(&d).unwrap();
    raw["config"]["future"] = json!({"keep":true});
    let bytes = serde_json::to_vec(&raw).unwrap();
    fs::write(t.file(), &bytes).unwrap();
    let mut s = DocumentStore::default();
    s.open(t.file()).unwrap();
    assert!(s.draft.read_only);
    assert!(s.update(assign).is_err());
    assert_eq!(fs::read(t.file()).unwrap(), bytes);
    raw["config"].as_object_mut().unwrap().remove("future");
    for a in raw["edits"].as_object_mut().unwrap().values_mut() {
        a["payload"] = json!({"type":"future_action","private":"preserve"});
    }
    let bytes = serde_json::to_vec(&raw).unwrap();
    fs::write(t.file(), &bytes).unwrap();
    s.open(t.file()).unwrap();
    assert!(s.draft.read_only);
    assert!(s.update(assign).is_err());
    assert_eq!(fs::read(t.file()).unwrap(), bytes);
}
#[test]
fn conflicting_instances_and_failed_save_preserve_memory() {
    let t = Temp::new();
    let mut a = DocumentStore::default();
    let mut b = DocumentStore::default();
    a.open(t.file()).unwrap();
    b.open(t.file()).unwrap();
    a.update(assign).unwrap();
    let bytes = fs::read(t.file()).unwrap();
    assert!(b.update(assign).is_err());
    assert!(b.draft.edits.is_empty());
    assert_eq!(fs::read(t.file()).unwrap(), bytes);
    fs::write(t.file().with_extension("pending"), b"interrupted").unwrap();
    let before = a.draft.revision;
    assert!(a.update(|d| d.undo(false)).is_err());
    assert_eq!(a.draft.revision, before);
    assert_eq!(fs::read(t.file()).unwrap(), bytes);
}
#[test]
fn typed_actions_reject_invalid_parameters() {
    for a in actions() {
        a.validate().unwrap();
    }
    let mut a = actions()[3].clone();
    for url in ["javascript:alert(1)", "https://", "https://a b"] {
        a.payload = json!({"type":"open_url","url":url});
        assert!(a.validate().is_err());
    }
    a.payload = json!({"type":"text","text":""});
    assert!(a.validate().is_err());
    a.payload = json!({"type":"shortcut","key":"p","modifiers":["guess"],"target":"editor"});
    assert!(a.validate().is_err());
}
