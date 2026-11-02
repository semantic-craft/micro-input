//! Private acceptance A. Production APIs remain read-only.
mod support;

#[test]
fn absent_opt_in_never_opens_a_transport() {
    assert!(support::authorize(None).is_err());
}

#[test]
fn arbitrary_target_and_operator_values_are_rejected() {
    assert!(support::authorize(Some("yes")).is_err());
    for target in [
        "keymap.json",
        "smart_actions.json",
        "mi_123456789abc.txt/",
        "../mi_123456789abc.txt",
        "mi_123456789abC.txt",
    ] {
        assert!(support::temp_name(target).is_err());
    }
    assert!(support::temp_name("mi_123456789abc.txt").is_ok());
}

#[tokio::test]
async fn run_without_opt_in_does_not_even_enumerate_or_open() {
    let transport = support::fixture::Fixture::new(support::fixture::Fault::None);
    assert!(support::run(&transport, None, std::time::Duration::ZERO)
        .await
        .is_err());
    assert_eq!(transport.opens.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(
        transport
            .enumerations
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}

#[tokio::test]
#[ignore = "physical device mutation; requires scripts/probe-a --execute-exact-tempfile-a"]
async fn native_tempfile_a() {
    support::native()
        .await
        .expect("A failed; inspect private stage reports, never retry automatically");
}

#[tokio::test]
async fn exact_a_round_trip_preserves_both_config_bytes_and_private_copies() {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::Ordering;
    let transport = support::fixture::Fixture::new(support::fixture::Fault::None);
    let outcome = support::run(&transport, Some(support::OPT_IN), std::time::Duration::ZERO)
        .await
        .unwrap();
    assert!(outcome.passed, "{}", outcome.reason);
    assert!(outcome.cleanup_confirmed);
    assert_eq!(
        *transport.mutations.lock().unwrap(),
        ["fs.write", "fs.delete"]
    );
    assert_eq!(transport.opens.load(Ordering::SeqCst), 1);
    assert!(transport.closed.load(Ordering::SeqCst));
    assert_eq!(
        std::fs::metadata(&outcome.directory)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    for item in std::fs::read_dir(&outcome.directory).unwrap() {
        assert_eq!(
            item.unwrap().metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert_eq!(
        std::fs::read(outcome.directory.join("smart_actions.json")).unwrap(),
        br#"{"smartActions":{}}"#
    );
    std::fs::remove_dir_all(outcome.directory).unwrap();
}

#[tokio::test]
async fn foreign_firmware_metadata_and_ambiguous_candidates_abort_before_mutation() {
    use support::fixture::{Fault, Fixture};
    for fault in [
        Fault::Firmware,
        Fault::Multiple,
        Fault::Foreign,
        Fault::MetadataChanged,
    ] {
        let transport = Fixture::new(fault);
        let outcome =
            support::run(&transport, Some(support::OPT_IN), std::time::Duration::ZERO).await;
        if let Ok(outcome) = outcome {
            assert!(!outcome.passed);
            std::fs::remove_dir_all(outcome.directory).unwrap();
        }
        assert!(transport.mutations.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn uncertain_or_changed_results_never_retry_or_delete_and_always_close() {
    use std::sync::atomic::Ordering;
    use support::fixture::{Fault, Fixture};
    for fault in [
        Fault::WriteTimeout,
        Fault::WriteError,
        Fault::WrongEcho,
        Fault::ForeignAfterCreate,
        Fault::ReadMismatch,
        Fault::ConfigChanged,
        Fault::Disconnect,
    ] {
        let transport = Fixture::new(fault);
        let outcome = support::run(&transport, Some(support::OPT_IN), std::time::Duration::ZERO)
            .await
            .unwrap();
        assert!(!outcome.passed);
        assert!(!outcome.cleanup_confirmed);
        assert_eq!(*transport.mutations.lock().unwrap(), ["fs.write"]);
        assert!(transport.closed.load(Ordering::SeqCst));
        std::fs::remove_dir_all(outcome.directory).unwrap();
    }
}

#[tokio::test]
async fn delete_timeout_only_uses_readonly_postchecks_even_when_absence_is_confirmed() {
    let transport = support::fixture::Fixture::new(support::fixture::Fault::DeleteTimeout);
    let outcome = support::run(&transport, Some(support::OPT_IN), std::time::Duration::ZERO)
        .await
        .unwrap();
    assert!(!outcome.passed);
    assert!(outcome.cleanup_confirmed);
    assert_eq!(
        *transport.mutations.lock().unwrap(),
        ["fs.write", "fs.delete"]
    );
    std::fs::remove_dir_all(outcome.directory).unwrap();
}
