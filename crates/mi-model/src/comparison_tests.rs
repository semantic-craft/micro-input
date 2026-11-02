use crate::{comparison::*, document::LocalDocumentV2};
use serde_json::json;
fn fixture() -> (LocalDocumentV2, crate::DeviceConfig, serde_json::Value) {
    let config = crate::demo().unwrap();
    let baseline = json!({"device_identity":{"confidence":"verified-serial","serial":"synthetic-one"},"keymap":{"sha1":"base"}});
    let doc =
        LocalDocumentV2::from_config(&config, Some(baseline.clone()), "device-snapshot").unwrap();
    (doc, config, baseline)
}
#[test]
fn local_and_remote_disjoint_names_merge_against_new_base() {
    let (mut local, mut remote, baseline) = fixture();
    let pid = local.editable.graph.profile_order[0].clone();
    let lid = local.editable.graph.profiles[&pid].layer_order[0].clone();
    local.rename_profile(&pid, "Local profile").unwrap();
    remote.profiles[0].layers[0].name = "Device layer".into();
    let comparison = Comparison::new(&local, &remote, baseline).unwrap();
    assert!(comparison.can_merge(), "{:?}", comparison.conflicts);
    let accepted = comparison.resolve(&local, Choice::Merge).unwrap().unwrap();
    assert_eq!(accepted.editable.graph.profiles[&pid].name, "Local profile");
    assert_eq!(accepted.editable.graph.layers[&lid].name, "Device layer");
    assert_eq!(accepted.revision, 1);
    assert_eq!(
        accepted.structure_changes(),
        vec!["配置改名：通用 Coding · 演示 → Local profile"]
    );
}
