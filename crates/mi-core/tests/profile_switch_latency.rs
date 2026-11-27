use mi_core::{OwnershipBannerState, WorkbenchStore};
use std::time::{Duration, Instant};
use tempfile::TempDir;

#[test]
fn switching_across_all_canonical_profiles_stays_under_two_milliseconds_with_zero_banner_flashes() {
    let temp = TempDir::new().unwrap();
    let mut store = WorkbenchStore::open_isolated(temp.path()).unwrap();

    let profile_ids: Vec<String> = store
        .sidebar_rows()
        .iter()
        .map(|row| row.profile_id.clone())
        .collect();
    assert_eq!(profile_ids.len(), 5);

    let per_switch_ceiling = if std::env::var_os("CI").is_some() {
        Duration::from_millis(15)
    } else {
        Duration::from_millis(2)
    };
    store.select_and_activate_profile(&profile_ids[0]).unwrap();

    let mut samples_us: Vec<u128> = Vec::with_capacity(35);
    let mut completed_switches = 0usize;
    let mut error_count = 0usize;

    for _round in 0..7 {
        for pid in &profile_ids {
            let started = Instant::now();
            match store.select_and_activate_profile(pid) {
                Ok(()) => completed_switches += 1,
                Err(_) => error_count += 1,
            }
            let elapsed = started.elapsed();
            samples_us.push(elapsed.as_micros());
            assert!(
                elapsed < per_switch_ceiling,
                "profile switch took {elapsed:?}, exceeding {per_switch_ceiling:?} budget"
            );
            assert!(
                store.last_switch_duration() < per_switch_ceiling,
                "recorded switch duration {:?} exceeded {per_switch_ceiling:?} budget",
                store.last_switch_duration()
            );
            assert_eq!(store.selected_profile_id(), Some(pid.as_str()));
            assert_eq!(
                store.active_host_profile_id().as_deref(),
                Some(pid.as_str())
            );
            assert!(
                !matches!(
                    store.projection().header.banner,
                    OwnershipBannerState::Caution { .. }
                ),
                "switching profiles must never flash a caution banner"
            );
        }
    }

    samples_us.sort_unstable();
    let median_us = samples_us[samples_us.len() / 2];
    let min_us = samples_us[0];
    let max_us = *samples_us.last().unwrap();
    println!(
        "BENCHMARK_SWITCH_5_PROFILES rounds=7 switches={completed_switches} errors={error_count} median_us={median_us} range_us={min_us}..={max_us} banner_flashes={}",
        store.banner_flash_count()
    );

    assert_eq!(error_count, 0);
    assert_eq!(completed_switches, 35);
    assert!(
        median_us < 2_000,
        "median profile switch {median_us}us exceeded 2000us budget"
    );
    assert_eq!(store.banner_flash_count(), 0);
}
