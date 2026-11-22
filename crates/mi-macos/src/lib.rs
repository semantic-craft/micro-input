pub mod antigravity_ax_labels;
pub mod app_cache;
pub mod ax_dictation;
pub mod codex_sessions;
pub mod diagnostics;
pub mod executor;
pub mod instance_lock;
pub mod managed_controls;
pub mod native_ax;
pub mod passive_panel;
pub mod private_fs;
#[cfg(feature = "qa-runner")]
pub mod scenarios;
pub mod session_navigation;
pub mod status_item;

pub use app_cache::{AppBundleCache, AppBundleInfo, OTHER_APPS_WILDCARD_BUNDLE_ID};
pub use ax_dictation::DictationAdapter;
pub use codex_sessions::{
    apply_codex_window_signals, build_codex_thread_deeplink, observe_codex_threads,
    read_codex_thread_fingerprint, verify_codex_window_readback, CodexLiveWindowSignals,
    CodexObservedStatus, CodexThreadFingerprint, CodexThreadObservation, CodexWindowSnapshot,
};
pub use diagnostics::{write_private_json, DiagnosticRedactor};
pub use executor::{
    is_antigravity_new_conversation_control, is_bundle_running, parse_pid_from_lsappinfo_output,
    redact_home_path, select_antigravity_new_conversation_target, submit_modifiers_for_adapter,
    AntigravityNewConversationTarget, AntigravitySidebarAxNode, ClipboardItemSnapshot,
    ClipboardSnapshot, DialScrollRequest, HeldInputId, HostActionReceipt, HostOsDriver,
    LiveHostActionExecutor, MacOsSystemDriver, MouseButtonKind, RecordedOsEvent, RecordingOsDriver,
    SimulatedHostActionExecutor,
};
pub use instance_lock::DocumentInstanceLock;
pub use managed_controls::{CodexReasoningAction, ModelPickerConfirmResult, SubagentOpenResult};
pub use native_ax::{observe_codex_live_window_signals, observe_codex_window_state};
pub use passive_panel::{
    NoopPassivePanelHost, PassivePanelEvent, PassivePanelFrameSpec, PassivePanelHost,
    PassivePanelRole, PassivePanelStyleSpec, ScreenGeometry, ScreenPoint, ScreenRect, ScreenSize,
};
#[cfg(any(test, feature = "test-util"))]
pub use passive_panel::{RecordingPassivePanelHost, RecordingPassivePanelState};
pub use private_fs::{
    ensure_private_dir, open_private_file, open_verified_readonly_file,
    verify_existing_private_dir, PrivateDirError, PrivateOpenMode,
};
#[cfg(feature = "qa-runner")]
pub use scenarios::{
    run_config_write_scenario, run_diagnostic_export_scenario, run_host_execution_scenario,
    run_lighting_control_scenario, ScenarioResultSummary,
};
pub use session_navigation::{
    codex_process_id, default_codex_home, navigate_codex_session,
    observe_antigravity_sidebar_conversations, observe_codex_threads_live,
    SidebarConversationObservation, SidebarRowVisualState,
};
#[cfg(any(test, feature = "test-util"))]
pub use status_item::RecordingStatusItemHost;
pub use status_item::{
    NoopStatusItemHost, PresetFileEnvelope, StatusIconPrimitive, StatusItemHost,
    StatusItemIconSpec, StatusItemSnapshot, MAX_PRESET_FILE_BYTES,
};

#[cfg(unix)]
pub fn regular_file_nofollow_open_flags() -> i32 {
    libc::O_NOFOLLOW | libc::O_NONBLOCK
}

#[cfg(not(unix))]
pub fn regular_file_nofollow_open_flags() -> i32 {
    0
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

#[cfg(unix)]
pub fn local_year_for_epoch_secs(epoch_secs: i64) -> Option<i32> {
    let t = epoch_secs as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let ptr = unsafe { libc::localtime_r(&t, &mut tm) };
    if ptr.is_null() {
        return None;
    }
    Some(tm.tm_year + 1900)
}

#[cfg(not(unix))]
pub fn local_year_for_epoch_secs(epoch_secs: i64) -> Option<i32> {
    let z = epoch_secs.div_euclid(86_400) + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    Some((y + i64::from(m <= 2)) as i32)
}

#[cfg(unix)]
pub fn local_civil_to_epoch_ms(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    min: u32,
    sec: u32,
    millis: u32,
) -> Option<u64> {
    if !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || min > 59
        || sec > 59
        || millis > 999
    {
        return None;
    }
    static MKTIME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = MKTIME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_year = year.checked_sub(1900)?;
    tm.tm_mon = (month as i32) - 1;
    tm.tm_mday = day as i32;
    tm.tm_hour = hour as i32;
    tm.tm_min = min as i32;
    tm.tm_sec = sec as i32;
    tm.tm_isdst = -1;
    let epoch_secs = unsafe { libc::mktime(&mut tm) };
    if epoch_secs < 0 {
        return None;
    }
    (epoch_secs as u64)
        .checked_mul(1000)?
        .checked_add(u64::from(millis))
}

#[cfg(not(unix))]
pub fn local_civil_to_epoch_ms(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    min: u32,
    sec: u32,
    millis: u32,
) -> Option<u64> {
    if !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || min > 59
        || sec > 59
        || millis > 999
    {
        return None;
    }
    let mut y = i64::from(year);
    let m = i64::from(month);
    let d = i64::from(day);
    if m <= 2 {
        y -= 1;
    }
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = (if m > 2 { m - 3 } else { m + 9 }) as u64;
    let doy = (153 * mp + 2) / 5 + (d as u64) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + (doe as i64) - 719_468;
    let secs = days * 86_400 + i64::from(hour) * 3600 + i64::from(min) * 60 + i64::from(sec);
    if secs < 0 {
        return None;
    }
    (secs as u64)
        .checked_mul(1000)?
        .checked_add(u64::from(millis))
}

#[cfg(test)]
mod local_time_tests {
    use super::*;

    #[test]
    fn local_civil_to_epoch_ms_rejects_invalid_dates_and_roundtrips_year() {
        assert!(local_civil_to_epoch_ms(2025, 2, 29, 12, 0, 0, 0).is_none());
        assert!(local_civil_to_epoch_ms(2026, 13, 1, 0, 0, 0, 0).is_none());
        let ms = local_civil_to_epoch_ms(2026, 6, 15, 12, 30, 45, 123).expect("valid date");
        assert_eq!(ms % 1000, 123);
        assert_eq!(local_year_for_epoch_secs((ms / 1000) as i64), Some(2026));
    }
}
