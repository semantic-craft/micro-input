use std::{
    env, fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_LOG_TAIL_BYTES: usize = 32_768;
const YEAR_ROLLOVER_FUTURE_TOLERANCE_MS: u64 = 30 * 86_400_000;
const MAX_RETRY_REASON_CHARS: usize = 100;
const MAX_API_ERROR_REASON_CHARS: usize = 132;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AntigravityLogRetry {
    pub attempt: u32,
    pub reason: String,
    pub retry_delay: Option<String>,
    pub ts_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AntigravityLogApiError {
    pub reason: String,
    pub ts_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AntigravityLogSnapshot {
    pub latest_retry: Option<AntigravityLogRetry>,
    pub latest_api_error: Option<AntigravityLogApiError>,
    pub latest_stream_activity_ms: Option<u64>,
}

#[derive(Debug, Clone)]
struct CachedLogSnapshot {
    modified: SystemTime,
    len: u64,
    snapshot: AntigravityLogSnapshot,
}

#[derive(Debug, Default)]
pub(crate) struct AntigravityLogTailReader {
    cached: Option<CachedLogSnapshot>,
}

impl AntigravityLogTailReader {
    pub(crate) fn new() -> Self {
        Self { cached: None }
    }

    pub(crate) fn read_snapshot(&mut self, now_ms: u64) -> AntigravityLogSnapshot {
        let Some(path) = default_antigravity_log_path() else {
            self.cached = None;
            return AntigravityLogSnapshot::default();
        };
        self.read_snapshot_from(&path, now_ms)
    }

    pub(crate) fn read_snapshot_from(
        &mut self,
        path: &Path,
        now_ms: u64,
    ) -> AntigravityLogSnapshot {
        let Ok(link_meta) = fs::symlink_metadata(path) else {
            self.cached = None;
            return AntigravityLogSnapshot::default();
        };
        if link_meta.file_type().is_symlink() || !link_meta.file_type().is_file() {
            self.cached = None;
            return AntigravityLogSnapshot::default();
        }
        let link_mtime = link_meta.modified().ok();
        let link_len = link_meta.len();
        if let (Some(mtime), Some(cached)) = (link_mtime, self.cached.as_ref()) {
            if cached.modified == mtime && cached.len == link_len {
                return cached.snapshot.clone();
            }
        }

        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(log_open_flags());
        }
        let Ok(mut file) = options.open(path) else {
            self.cached = None;
            return AntigravityLogSnapshot::default();
        };
        let Ok(meta) = file.metadata() else {
            self.cached = None;
            return AntigravityLogSnapshot::default();
        };
        if !meta.file_type().is_file() {
            self.cached = None;
            return AntigravityLogSnapshot::default();
        }
        let file_len = meta.len();
        let sought_mid_file = if file_len > MAX_LOG_TAIL_BYTES as u64 {
            file.seek(SeekFrom::End(-(MAX_LOG_TAIL_BYTES as i64)))
                .is_ok()
        } else {
            false
        };
        let mut buf = Vec::with_capacity(file_len.min(MAX_LOG_TAIL_BYTES as u64) as usize);
        if (&file)
            .take(MAX_LOG_TAIL_BYTES as u64)
            .read_to_end(&mut buf)
            .is_err()
        {
            self.cached = None;
            return AntigravityLogSnapshot::default();
        }
        let decoded = String::from_utf8_lossy(&buf);
        let text = if sought_mid_file {
            match decoded.find('\n') {
                Some(idx) => &decoded[idx + 1..],
                None => decoded.as_ref(),
            }
        } else {
            decoded.as_ref()
        };
        let (now_year, local_offset_secs) = local_year_and_offset(now_ms / 1000);
        let snapshot = parse_antigravity_log_tail(text, now_ms, now_year, local_offset_secs);
        if let Some(mtime) = meta.modified().ok().or(link_mtime) {
            self.cached = Some(CachedLogSnapshot {
                modified: mtime,
                len: file_len,
                snapshot: snapshot.clone(),
            });
        }
        snapshot
    }
}

pub(crate) fn default_antigravity_log_path() -> Option<PathBuf> {
    env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(|home| PathBuf::from(home).join("Library/Logs/Antigravity/language_server.log"))
}

pub(crate) fn system_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(unix)]
fn log_open_flags() -> i32 {
    #[cfg(target_os = "macos")]
    {
        0x0100 | 0x0004
    }
    #[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "x86")))]
    {
        0o400000 | 0o4000
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        0o100000 | 0o4000
    }
}

#[cfg(unix)]
#[repr(C)]
struct CTm {
    tm_sec: i32,
    tm_min: i32,
    tm_hour: i32,
    tm_mday: i32,
    tm_mon: i32,
    tm_year: i32,
    tm_wday: i32,
    tm_yday: i32,
    tm_isdst: i32,
    tm_gmtoff: isize,
    tm_zone: *const u8,
}

#[cfg(unix)]
fn local_year_and_offset(epoch_secs: u64) -> (i32, i64) {
    extern "C" {
        fn localtime_r(timep: *const i64, result: *mut CTm) -> *mut CTm;
    }
    let t = epoch_secs as i64;
    let mut tm = CTm {
        tm_sec: 0,
        tm_min: 0,
        tm_hour: 0,
        tm_mday: 0,
        tm_mon: 0,
        tm_year: 0,
        tm_wday: 0,
        tm_yday: 0,
        tm_isdst: 0,
        tm_gmtoff: 0,
        tm_zone: std::ptr::null(),
    };
    let ptr = unsafe { localtime_r(&t, &mut tm) };
    if ptr.is_null() {
        return (utc_year_from_epoch_secs(epoch_secs), 0);
    }
    let year = tm.tm_year + 1900;
    let month = (tm.tm_mon + 1) as u32;
    let day = tm.tm_mday as u32;
    let hour = tm.tm_hour as u32;
    let min = tm.tm_min as u32;
    let sec = tm.tm_sec.clamp(0, 59) as u32;
    let offset = civil_to_utc_secs(year, month, day, hour, min, sec)
        .map(|local_as_utc| local_as_utc - t)
        .unwrap_or(0);
    (year, offset)
}

#[cfg(not(unix))]
fn local_year_and_offset(epoch_secs: u64) -> (i32, i64) {
    (utc_year_from_epoch_secs(epoch_secs), 0)
}

fn utc_year_from_epoch_secs(epoch_secs: u64) -> i32 {
    let z = (epoch_secs / 86_400) as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (y + i64::from(m <= 2)) as i32
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

fn civil_to_utc_secs(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    min: u32,
    sec: u32,
) -> Option<i64> {
    if !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || min > 59
        || sec > 59
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
    Some(days * 86_400 + i64::from(hour) * 3600 + i64::from(min) * 60 + i64::from(sec))
}

fn parse_two_digits(bytes: &[u8]) -> Option<u32> {
    let a = bytes.first()?.checked_sub(b'0')?;
    let b = bytes.get(1)?.checked_sub(b'0')?;
    if a > 9 || b > 9 {
        return None;
    }
    Some(u32::from(a) * 10 + u32::from(b))
}

fn parse_glog_timestamp_ms(
    line: &str,
    now_ms: u64,
    now_year: i32,
    local_offset_secs: i64,
) -> Option<(u64, &str)> {
    let bytes = line.as_bytes();
    if bytes.len() < 14 {
        return None;
    }
    if !matches!(bytes[0], b'I' | b'W' | b'E' | b'F') {
        return None;
    }
    if bytes[5] != b' ' || bytes[8] != b':' || bytes[11] != b':' {
        return None;
    }
    let month = parse_two_digits(&bytes[1..3])?;
    let day = parse_two_digits(&bytes[3..5])?;
    let hour = parse_two_digits(&bytes[6..8])?;
    let min = parse_two_digits(&bytes[9..11])?;
    let sec = parse_two_digits(&bytes[12..14])?;
    let mut cursor = 14usize;
    let mut frac_ms = 0u64;
    if bytes.get(cursor) == Some(&b'.') {
        cursor += 1;
        let mut digit_idx = 0usize;
        while let Some(&b) = bytes.get(cursor) {
            if !b.is_ascii_digit() {
                break;
            }
            if digit_idx < 3 {
                let weight = match digit_idx {
                    0 => 100,
                    1 => 10,
                    _ => 1,
                };
                frac_ms += u64::from(b - b'0') * weight;
            }
            digit_idx += 1;
            cursor += 1;
        }
    }

    let to_epoch_ms = |year: i32| -> Option<u64> {
        let local_secs = civil_to_utc_secs(year, month, day, hour, min, sec)?;
        let utc_secs = local_secs.checked_sub(local_offset_secs)?;
        if utc_secs < 0 {
            return None;
        }
        Some(
            (utc_secs as u64)
                .saturating_mul(1000)
                .saturating_add(frac_ms),
        )
    };

    let ts_ms = match to_epoch_ms(now_year) {
        Some(ts) if ts <= now_ms.saturating_add(YEAR_ROLLOVER_FUTURE_TOLERANCE_MS) => ts,
        _ => to_epoch_ms(now_year - 1)?,
    };
    Some((ts_ms, &line[cursor..]))
}

fn is_noise_line(line: &str) -> bool {
    line.contains("play.googleapis.com/log")
        || line.contains("llm_monitor.go")
        || line.contains("sir_flash_lite")
        || line.contains("extract_searchable_content.go")
}

fn is_user_canceled(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("context canceled")
        || lower.contains("canceled by user")
        || lower.contains("cancelled by user")
        || lower.contains("aborted by user")
        || lower.contains("user canceled")
        || lower.contains("user cancelled")
}

pub(crate) fn truncate_chars(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let mut chars = text.chars();
    let mut out = String::new();
    for _ in 0..max_chars {
        match chars.next() {
            Some(ch) => out.push(ch),
            None => return out,
        }
    }
    if chars.next().is_some() {
        out.pop();
        out.push('…');
    }
    out
}

fn sanitize_reason(raw: &str, max_chars: usize) -> String {
    let mut collapsed = String::with_capacity(raw.len().min(max_chars * 4));
    let mut prev_space = true;
    for ch in raw.chars() {
        if ch.is_control() || ch.is_whitespace() {
            if !prev_space {
                collapsed.push(' ');
                prev_space = true;
            }
        } else {
            collapsed.push(ch);
            prev_space = false;
        }
    }
    let trimmed = collapsed.trim();
    truncate_chars(trimmed, max_chars)
}

fn extract_retry(body: &str, ts_ms: u64) -> Option<AntigravityLogRetry> {
    let after_marker = body
        .find("CallWithRetry: attempt ")
        .map(|idx| &body[idx + "CallWithRetry: attempt ".len()..])
        .or_else(|| {
            body.find("Run: attempt ")
                .map(|idx| &body[idx + "Run: attempt ".len()..])
        })?;
    let digits_len = after_marker
        .bytes()
        .take_while(|b| b.is_ascii_digit())
        .count();
    if digits_len == 0 {
        return None;
    }
    let attempt = after_marker[..digits_len].parse::<u32>().ok()?;
    let after_failed = after_marker[digits_len..].strip_prefix(" failed (")?;
    let (raw_reason, retry_delay) =
        if let Some((reason_part, delay_part)) = after_failed.split_once("), retrying in ") {
            let delay = delay_part
                .split_whitespace()
                .next()
                .map(|tok| tok.trim_end_matches([',', '.', ';', ')']))
                .filter(|tok| !tok.is_empty())
                .map(str::to_string);
            (reason_part, delay)
        } else {
            let mut depth = 1usize;
            let mut close_idx = None;
            for (idx, ch) in after_failed.char_indices() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            close_idx = Some(idx);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let end = close_idx.or_else(|| after_failed.rfind(')'))?;
            (&after_failed[..end], None)
        };
    let reason = sanitize_reason(raw_reason, MAX_RETRY_REASON_CHARS);
    if reason.is_empty() {
        return None;
    }
    Some(AntigravityLogRetry {
        attempt,
        reason,
        retry_delay,
        ts_ms,
    })
}

fn extract_api_error(header: &str, body: &str, ts_ms: u64) -> Option<AntigravityLogApiError> {
    if !header.contains("errorreport.go:") {
        return None;
    }
    let idx = body.find("agent executor error:")?;
    let after = body[idx + "agent executor error:".len()..].trim();
    let stripped = after
        .strip_prefix("generating and executing:")
        .map(str::trim)
        .unwrap_or(after);
    let reason = sanitize_reason(stripped, MAX_API_ERROR_REASON_CHARS);
    if reason.is_empty() {
        return None;
    }
    Some(AntigravityLogApiError { reason, ts_ms })
}

pub(crate) fn parse_antigravity_log_tail(
    text: &str,
    now_ms: u64,
    now_year: i32,
    local_offset_secs: i64,
) -> AntigravityLogSnapshot {
    let mut snapshot = AntigravityLogSnapshot::default();
    for raw_line in text.lines() {
        if raw_line.is_empty() || is_noise_line(raw_line) {
            continue;
        }
        let stripped = raw_line
            .strip_prefix("ERROR: logging before google.Init: ")
            .unwrap_or(raw_line);
        let Some((ts_ms, after_ts)) =
            parse_glog_timestamp_ms(stripped, now_ms, now_year, local_offset_secs)
        else {
            continue;
        };
        let Some((header, body)) = after_ts.split_once("] ") else {
            continue;
        };
        let body = body.trim();
        if header.contains("http_helpers.go:") && body.contains("streamGenerateContent") {
            snapshot.latest_stream_activity_ms = Some(
                snapshot
                    .latest_stream_activity_ms
                    .map_or(ts_ms, |prev| prev.max(ts_ms)),
            );
            continue;
        }
        if is_user_canceled(body) {
            continue;
        }
        if let Some(retry) = extract_retry(body, ts_ms) {
            if snapshot
                .latest_retry
                .as_ref()
                .is_none_or(|prev| retry.ts_ms >= prev.ts_ms)
            {
                snapshot.latest_retry = Some(retry);
            }
            continue;
        }
        if let Some(api_err) = extract_api_error(header, body, ts_ms) {
            if snapshot
                .latest_api_error
                .as_ref()
                .is_none_or(|prev| api_err.ts_ms >= prev.ts_ms)
            {
                snapshot.latest_api_error = Some(api_err);
            }
        }
    }
    snapshot
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parses_real_retry_line_with_google_init_prefix() {
        let now_ms = (civil_to_utc_secs(2026, 11, 21, 14, 20, 0).unwrap() as u64) * 1000;
        let log = "ERROR: logging before google.Init: I1121 22:16:20.828895   95269 run.go:317] Run: attempt 1 failed (stream reading error: unexpected EOF), retrying in 4s\n";
        let snap = parse_antigravity_log_tail(log, now_ms, 2026, 8 * 3600);
        let retry = snap.latest_retry.expect("retry parsed");
        assert_eq!(retry.attempt, 1);
        assert_eq!(retry.reason, "stream reading error: unexpected EOF");
        assert_eq!(retry.retry_delay.as_deref(), Some("4s"));
        let expected_ms =
            (civil_to_utc_secs(2026, 11, 21, 14, 16, 20).unwrap() as u64) * 1000 + 828;
        assert_eq!(retry.ts_ms, expected_ms);
    }

    #[test]
    fn ignores_user_canceled_and_noise_lines() {
        let now_ms = (civil_to_utc_secs(2026, 11, 21, 14, 25, 0).unwrap() as u64) * 1000;
        let log = concat!(
            "ERROR: logging before google.Init: I1121 22:16:20.100000 100 run.go:317] Run: attempt 1 failed (context canceled by user), retrying in 2s\n",
            "ERROR: logging before google.Init: E1121 22:16:21.100000 100 errorreport.go:285] agent executor error: request cancelled by user\n",
            "ERROR: logging before google.Init: I1121 22:16:22.100000 100 llm_monitor.go:706] CallWithRetry: attempt 2 failed (transient), retrying in 3s\n",
            "ERROR: logging before google.Init: I1121 22:16:23.100000 100 runner.go:956] Monitor sir_flash_lite CallWithRetry: attempt 3 failed (timeout), retrying in 5s\n",
            "ERROR: logging before google.Init: E1121 22:16:24.100000 100 errorreport.go:285] play.googleapis.com/log agent executor error: ignored telemetry\n",
            "ERROR: logging before google.Init: I1121 22:16:25.100000 100 extract_searchable_content.go:42] Run: attempt 1 failed (pdf parse), retrying in 1s\n",
        );
        let snap = parse_antigravity_log_tail(log, now_ms, 2026, 8 * 3600);
        assert!(snap.latest_retry.is_none());
        assert!(snap.latest_api_error.is_none());
        assert!(snap.latest_stream_activity_ms.is_none());
    }

    #[test]
    fn extracts_api_error_and_truncates_utf8_cleanly() {
        let now_ms = (civil_to_utc_secs(2026, 11, 21, 14, 20, 0).unwrap() as u64) * 1000;
        let long_cn = "中文配额提示".repeat(30);
        let log = format!(
            "ERROR: logging before google.Init: E1121 22:18:18.807524  106587 errorreport.go:285] agent executor error: generating and executing: RESOURCE_EXHAUSTED (code 429): {long_cn} Resets in 13m8s\n"
        );
        let snap = parse_antigravity_log_tail(&log, now_ms, 2026, 8 * 3600);
        let err = snap.latest_api_error.expect("api error parsed");
        assert!(err.reason.starts_with("RESOURCE_EXHAUSTED (code 429):"));
        assert!(err.reason.chars().count() <= MAX_API_ERROR_REASON_CHARS);
        assert!(err.reason.ends_with('…'));
    }

    #[test]
    fn tracks_stream_activity_timestamp_for_retry_recovery() {
        let now_ms = (civil_to_utc_secs(2026, 11, 21, 18, 25, 0).unwrap() as u64) * 1000;
        let log = concat!(
            "ERROR: logging before google.Init: I1122 02:20:10.000000 1146550 run.go:317] CallWithRetry: attempt 2 failed (HTTP 503), retrying in 6s\n",
            "ERROR: logging before google.Init: I1122 02:21:49.694317 1146553 http_helpers.go:709] URL: https://daily-cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse Trace: 0x1234\n",
        );
        let snap = parse_antigravity_log_tail(log, now_ms, 2026, 8 * 3600);
        let retry = snap.latest_retry.expect("retry");
        let stream_ms = snap.latest_stream_activity_ms.expect("stream activity");
        assert!(stream_ms > retry.ts_ms);
    }

    #[test]
    fn handles_year_rollover_around_new_year() {
        let now_ms = (civil_to_utc_secs(2027, 1, 1, 0, 5, 0).unwrap() as u64) * 1000;
        let log =
            "I1231 23:59:30.500000 100 run.go:317] Run: attempt 1 failed (EOF), retrying in 2s\n";
        let snap = parse_antigravity_log_tail(log, now_ms, 2027, 0);
        let retry = snap.latest_retry.expect("rollover retry");
        let expected_ms =
            (civil_to_utc_secs(2026, 12, 31, 23, 59, 30).unwrap() as u64) * 1000 + 500;
        assert_eq!(retry.ts_ms, expected_ms);
    }

    #[test]
    fn tail_reader_caches_unchanged_file_and_skips_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("language_server.log");
        {
            let mut f = fs::File::create(&log_path).unwrap();
            let padding = "x".repeat(40_000);
            writeln!(f, "{padding}").unwrap();
            writeln!(
                f,
                "ERROR: logging before google.Init: I1121 22:16:20.828895 95269 run.go:317] Run: attempt 1 failed (EOF), retrying in 4s"
            )
            .unwrap();
        }
        let now_ms = (civil_to_utc_secs(2026, 11, 21, 14, 20, 0).unwrap() as u64) * 1000;
        let mut reader = AntigravityLogTailReader::new();
        let first = reader.read_snapshot_from(&log_path, now_ms);
        assert_eq!(first.latest_retry.as_ref().map(|r| r.attempt), Some(1));
        let second = reader.read_snapshot_from(&log_path, now_ms + 1500);
        assert_eq!(first, second);

        #[cfg(unix)]
        {
            let link_path = dir.path().join("symlink.log");
            std::os::unix::fs::symlink(&log_path, &link_path).unwrap();
            let linked = reader.read_snapshot_from(&link_path, now_ms + 3000);
            assert_eq!(linked, AntigravityLogSnapshot::default());
        }
    }
}
