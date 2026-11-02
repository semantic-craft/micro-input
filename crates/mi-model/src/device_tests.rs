use super::*;
#[test]
fn device_projection_is_independent_and_preserves_raw_values() {
    let c = parse_device_files(
        include_bytes!("../fixtures/device/synthetic-keymap-numeric.json"),
        Some(include_bytes!(
            "../fixtures/device/synthetic-smart-actions-object.json"
        )),
    )
    .unwrap();
    assert_eq!(c.profiles[1].id, 42);
    assert_eq!(c.profiles[1].index, 1);
    assert_eq!(c.profiles[0].layers[0].color, 0x123456);
    assert_eq!(
        c.smart_actions["smartActions"]["SA_9007199254740993"]["id"],
        "existing-id-must-not-be-overwritten"
    );
    assert!(matches!(
        c.profiles[0].layers[0].keys[1],
        Binding::Unknown(_)
    ));
    assert_eq!(c.source, Source::Imported);
}

#[test]
fn ambiguous_duplicate_json_keys_never_become_a_supported_projection() {
    assert!(parse_device_files(br#"{"profiles":[],"profiles":[]}"#, None).is_err());
}

#[test]
fn device_optionals_ambiguities_and_invalid_files_remain_honest() {
    let c = parse_device_files(
        include_bytes!("../fixtures/device/synthetic-keymap-optionals-missing.json"),
        None,
    )
    .unwrap();
    assert!(matches!(
        c.profiles[0].layers[0].encoders[0],
        Binding::Unknown(_)
    ));
    let c = parse_device_files(
        include_bytes!("../fixtures/device/synthetic-keymap-ambiguous-ids.json"),
        None,
    )
    .unwrap();
    assert!(c.warnings.iter().any(|w| w.contains("ID 缺失或重复")));
    assert_eq!(c.raw["future_root"]["u64_max"], u64::MAX);
    assert!(parse_device_files(
        include_bytes!("../fixtures/device/synthetic-keymap-invalid-row.json"),
        None
    )
    .is_err());
    let keymap = include_bytes!("../fixtures/device/synthetic-keymap-numeric.json");
    assert!(parse_device_files(keymap, Some(&[])).is_err());
    assert!(parse_device_files(keymap, Some(b"[]")).is_err());
    assert!(parse_device_files(keymap, Some(br#"{"smartActions":{}}"#)).is_ok());
    let c = parse_device_files(
        include_bytes!("../fixtures/device/synthetic-keymap-invalid-colors.json"),
        None,
    )
    .unwrap();
    assert_eq!(
        c.warnings.iter().filter(|w| w.contains("颜色未知")).count(),
        3
    );
}
