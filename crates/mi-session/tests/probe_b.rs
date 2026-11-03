//! Private acceptance B. Production APIs remain read-only.
mod support;

#[tokio::test]
async fn exact_b_round_trip_uses_only_two_chunks_and_delete() {
    let transport = support::fixture::Fixture::new(support::fixture::Fault::None);
    let outcome = support::run_b(
        &transport,
        Some(support::OPT_IN_B),
        std::time::Duration::ZERO,
    )
    .await
    .unwrap();
    assert!(outcome.passed, "{}", outcome.reason);
    assert!(outcome.cleanup_confirmed);
    assert_eq!(
        *transport.mutations.lock().unwrap(),
        ["fs.writebin", "fs.writebin", "fs.delete"]
    );
    assert_eq!(transport.opens.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(transport.closed.load(std::sync::atomic::Ordering::SeqCst));
    std::fs::remove_dir_all(outcome.directory).unwrap();
}

#[tokio::test]
async fn first_chunk_uncertainty_stops_second_chunk_and_cleanup() {
    use support::fixture::{Fault, Fixture};
    for fault in [
        Fault::WriteTimeout,
        Fault::WriteError,
        Fault::WrongEcho,
        Fault::Disconnect,
        Fault::ForeignAfterCreate,
        Fault::ConfigChanged,
    ] {
        let transport = Fixture::new(fault);
        let outcome = support::run_b(
            &transport,
            Some(support::OPT_IN_B),
            std::time::Duration::ZERO,
        )
        .await
        .unwrap();
        assert!(!outcome.passed, "fault must stop B");
        assert!(!outcome.cleanup_confirmed);
        assert_eq!(*transport.mutations.lock().unwrap(), ["fs.writebin"]);
        assert!(transport.closed.load(std::sync::atomic::Ordering::SeqCst));
        std::fs::remove_dir_all(outcome.directory).unwrap();
    }
}

#[tokio::test]
async fn second_chunk_uncertainty_never_retries_or_cleans_up() {
    use support::fixture::{Fault, Fixture};
    for fault in [
        Fault::SecondChunkTimeout,
        Fault::SecondChunkError,
        Fault::SecondChunkWrongEcho,
        Fault::ReadMismatch,
    ] {
        let transport = Fixture::new(fault);
        let outcome = support::run_b(
            &transport,
            Some(support::OPT_IN_B),
            std::time::Duration::ZERO,
        )
        .await
        .unwrap();
        assert!(!outcome.passed, "fault must stop B");
        assert!(!outcome.cleanup_confirmed);
        assert_eq!(
            *transport.mutations.lock().unwrap(),
            ["fs.writebin", "fs.writebin"]
        );
        assert!(transport.closed.load(std::sync::atomic::Ordering::SeqCst));
        std::fs::remove_dir_all(outcome.directory).unwrap();
    }
}

#[tokio::test]
async fn absent_or_a_authorization_never_enumerates_or_opens_b() {
    use std::sync::atomic::Ordering;
    for value in [None, Some(support::OPT_IN), Some("yes")] {
        let transport = support::fixture::Fixture::new(support::fixture::Fault::None);
        assert!(support::run_b(&transport, value, std::time::Duration::ZERO)
            .await
            .is_err());
        assert_eq!(transport.enumerations.load(Ordering::SeqCst), 0);
        assert_eq!(transport.opens.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn b_refuses_unverified_identity_and_prewrite_metadata_changes() {
    use support::fixture::{Fault, Fixture};
    for fault in [
        Fault::Firmware,
        Fault::Multiple,
        Fault::Foreign,
        Fault::MetadataChanged,
    ] {
        let transport = Fixture::new(fault);
        if let Ok(outcome) = support::run_b(
            &transport,
            Some(support::OPT_IN_B),
            std::time::Duration::ZERO,
        )
        .await
        {
            assert!(!outcome.passed);
            std::fs::remove_dir_all(outcome.directory).unwrap();
        }
        assert!(transport.mutations.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn b_delete_timeout_records_unknown_and_only_checks_readonly() {
    let transport = support::fixture::Fixture::new(support::fixture::Fault::DeleteTimeout);
    let outcome = support::run_b(
        &transport,
        Some(support::OPT_IN_B),
        std::time::Duration::ZERO,
    )
    .await
    .unwrap();
    assert!(!outcome.passed);
    assert!(outcome.cleanup_confirmed);
    assert_eq!(
        *transport.mutations.lock().unwrap(),
        ["fs.writebin", "fs.writebin", "fs.delete"]
    );
    std::fs::remove_dir_all(outcome.directory).unwrap();
}

#[tokio::test]
#[ignore = "physical device mutation; requires scripts/probe-b --execute-exact-tempfile-b"]
async fn native_tempfile_b() {
    support::native_b()
        .await
        .expect("B failed; inspect private stage reports, never retry automatically");
}
