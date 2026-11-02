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
