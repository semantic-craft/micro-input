use std::{
    env, fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_LOG_TAIL_BYTES: usize = 131_072;
const FUTURE_SKEW_TOLERANCE_MS: u64 = 300_000;
const MAX_RETRY_REASON_CHARS: usize = 100;
const MAX_API_ERROR_REASON_CHARS: usize = 160;

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
}

#[derive(Debug, Clone)]
struct CachedLogSnapshot {
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
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
        #[cfg(unix)]
        let (link_dev, link_ino) = {
            use std::os::unix::fs::MetadataExt;
            (link_meta.dev(), link_meta.ino())
        };
        if let (Some(mtime), Some(cached)) = (link_mtime, self.cached.as_ref()) {
            #[cfg(unix)]
            let same_file = cached.dev == link_dev && cached.ino == link_ino;
            #[cfg(not(unix))]
            let same_file = true;
            if same_file && cached.modified == mtime && cached.len == link_len {
                return cached.snapshot.clone();
            }
        }

        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(mi_macos::regular_file_nofollow_open_flags());
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
        #[cfg(unix)]
        let (file_dev, file_ino) = {
            use std::os::unix::fs::MetadataExt;
            (meta.dev(), meta.ino())
        };

        let same_grown_file = self.cached.as_ref().is_some_and(|cached| {
            #[cfg(unix)]
            let same_identity = cached.dev == file_dev && cached.ino == file_ino;
            #[cfg(not(unix))]
            let same_identity = true;
            same_identity && file_len > cached.len
        });

        let (read_offset, drop_first_partial_line, base_snapshot) = if same_grown_file {
            let cached = self.cached.as_ref().unwrap();
            let delta = file_len.saturating_sub(cached.len);
            if delta <= MAX_LOG_TAIL_BYTES as u64 {
                (cached.len, false, cached.snapshot.clone())
            } else {
                (
                    file_len.saturating_sub(MAX_LOG_TAIL_BYTES as u64),
                    true,
                    cached.snapshot.clone(),
                )
            }
        } else {
            let start = file_len.saturating_sub(MAX_LOG_TAIL_BYTES as u64);
            (start, start > 0, AntigravityLogSnapshot::default())
        };

        if read_offset > 0 && file.seek(SeekFrom::Start(read_offset)).is_err() {
            self.cached = None;
            return AntigravityLogSnapshot::default();
        }
        let to_read = file_len
            .saturating_sub(read_offset)
            .min(MAX_LOG_TAIL_BYTES as u64);
        let mut buf = Vec::with_capacity(to_read as usize);
        if (&file).take(to_read).read_to_end(&mut buf).is_err() {
            self.cached = None;
            return AntigravityLogSnapshot::default();
        }
        let decoded = String::from_utf8_lossy(&buf);
        let text = if drop_first_partial_line {
            match decoded.find('\n') {
                Some(idx) => &decoded[idx + 1..],
                None => decoded.as_ref(),
            }
        } else {
            decoded.as_ref()
        };
        let now_year = mi_macos::local_year_for_epoch_secs((now_ms / 1000) as i64)
            .unwrap_or_else(|| utc_year_from_epoch_secs(now_ms / 1000));
        let incoming = parse_antigravity_log_tail(text, now_ms, now_year);
        let mut merged = base_snapshot;
        merge_antigravity_log_snapshots(&mut merged, incoming);
        if let Some(mtime) = meta.modified().ok().or(link_mtime) {
            self.cached = Some(CachedLogSnapshot {
                #[cfg(unix)]
                dev: file_dev,
                #[cfg(unix)]
                ino: file_ino,
                modified: mtime,
                len: file_len,
                snapshot: merged.clone(),
            });
        }
        merged
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

#[cfg(test)]
fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

#[cfg(test)]
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

#[cfg(test)]
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

fn parse_glog_timestamp_ms<'a, F>(
    line: &'a str,
    now_ms: u64,
    now_year: i32,
    resolve_civil_ms: &F,
) -> Option<(u64, &'a str)>
where
    F: Fn(i32, u32, u32, u32, u32, u32, u32) -> Option<u64>,
{
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
    let mut frac_ms = 0u32;
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
                frac_ms += u32::from(b - b'0') * weight;
            }
            digit_idx += 1;
            cursor += 1;
        }
    }

    let max_allowed = now_ms.saturating_add(FUTURE_SKEW_TOLERANCE_MS);
    let ts_ms = [now_year + 1, now_year, now_year - 1]
        .into_iter()
        .filter_map(|year| resolve_civil_ms(year, month, day, hour, min, sec, frac_ms))
        .find(|&ts| ts <= max_allowed)?;
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

pub(crate) fn parse_go_duration_ms(raw: &str) -> Option<u64> {
    let mut rest = raw.trim();
    if rest.is_empty() {
        return None;
    }
    let mut total_ms = 0f64;
    let mut parsed_any = false;
    while !rest.is_empty() {
        let num_len = rest
            .bytes()
            .take_while(|b| b.is_ascii_digit() || *b == b'.')
            .count();
        if num_len == 0 {
            return None;
        }
        let val: f64 = rest[..num_len].parse().ok()?;
        rest = &rest[num_len..];
        let (unit_len, scale_ms) = if rest.starts_with("ms") {
            (2, 1.0)
        } else if rest.starts_with("us") || rest.starts_with("ns") {
            (2, 0.001)
        } else if rest.starts_with("µs") {
            ("µs".len(), 0.001)
        } else if rest.starts_with('h') {
            (1, 3_600_000.0)
        } else if rest.starts_with('m') {
            (1, 60_000.0)
        } else if rest.starts_with('s') {
            (1, 1_000.0)
        } else {
            return None;
        };
        total_ms += val * scale_ms;
        rest = &rest[unit_len..];
        parsed_any = true;
    }
    if !parsed_any || !total_ms.is_finite() || total_ms < 0.0 {
        return None;
    }
    Some(total_ms.round() as u64)
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
    let raw_reason = &after_failed[..end];
    let after_close = &after_failed[end + 1..];
    let retry_delay = after_close
        .strip_prefix(", retrying in ")
        .and_then(|delay_part| delay_part.split_whitespace().next())
        .map(|tok| tok.trim_end_matches([',', '.', ';', ')']))
        .filter(|tok| !tok.is_empty())
        .map(str::to_string);
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
    let stripped = if let Some(idx) = body.find("agent executor error:") {
        let after = body[idx + "agent executor error:".len()..].trim();
        after
            .strip_prefix("generating and executing:")
            .map(str::trim)
            .unwrap_or(after)
    } else if let Some(idx) = body.find("generating and executing:") {
        body[idx + "generating and executing:".len()..].trim()
    } else {
        return None;
    };
    let reason = sanitize_reason(stripped, MAX_API_ERROR_REASON_CHARS);
    if reason.is_empty() {
        return None;
    }
    Some(AntigravityLogApiError { reason, ts_ms })
}

pub(crate) fn merge_antigravity_log_snapshots(
    base: &mut AntigravityLogSnapshot,
    incoming: AntigravityLogSnapshot,
) {
    if let Some(retry) = incoming.latest_retry {
        if base
            .latest_retry
            .as_ref()
            .is_none_or(|prev| retry.ts_ms >= prev.ts_ms)
        {
            base.latest_retry = Some(retry);
        }
    }
    if let Some(api_err) = incoming.latest_api_error {
        if base
            .latest_api_error
            .as_ref()
            .is_none_or(|prev| api_err.ts_ms >= prev.ts_ms)
        {
            base.latest_api_error = Some(api_err);
        }
    }
    if let (Some(err), Some(retry)) = (&base.latest_api_error, &base.latest_retry) {
        if err.ts_ms > retry.ts_ms {
            base.latest_retry = None;
        }
    }
}

fn parse_antigravity_log_tail_with_resolver<F>(
    text: &str,
    now_ms: u64,
    now_year: i32,
    resolve_civil_ms: F,
) -> AntigravityLogSnapshot
where
    F: Fn(i32, u32, u32, u32, u32, u32, u32) -> Option<u64>,
{
    let mut snapshot = AntigravityLogSnapshot::default();
    for raw_line in text.lines() {
        if raw_line.is_empty() || is_noise_line(raw_line) {
            continue;
        }
        let stripped = raw_line
            .strip_prefix("ERROR: logging before google.Init: ")
            .unwrap_or(raw_line);
        let Some((ts_ms, after_ts)) =
            parse_glog_timestamp_ms(stripped, now_ms, now_year, &resolve_civil_ms)
        else {
            continue;
        };
        let Some((header, body)) = after_ts.split_once("] ") else {
            continue;
        };
        let body = body.trim();
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
    if let (Some(err), Some(retry)) = (&snapshot.latest_api_error, &snapshot.latest_retry) {
        if err.ts_ms > retry.ts_ms {
            snapshot.latest_retry = None;
        }
    }
    snapshot
}

pub(crate) fn parse_antigravity_log_tail(
    text: &str,
    now_ms: u64,
    now_year: i32,
) -> AntigravityLogSnapshot {
    parse_antigravity_log_tail_with_resolver(
        text,
        now_ms,
        now_year,
        mi_macos::local_civil_to_epoch_ms,
    )
}

#[cfg(test)]
pub(crate) fn parse_antigravity_log_tail_with_offset(
    text: &str,
    now_ms: u64,
    now_year: i32,
    local_offset_secs: i64,
) -> AntigravityLogSnapshot {
    parse_antigravity_log_tail_with_resolver(
        text,
        now_ms,
        now_year,
        |year, month, day, hour, min, sec, millis| {
            let local_secs = civil_to_utc_secs(year, month, day, hour, min, sec)?;
            let utc_secs = local_secs.checked_sub(local_offset_secs)?;
            if utc_secs < 0 {
                return None;
            }
            Some(
                (utc_secs as u64)
                    .saturating_mul(1000)
                    .saturating_add(u64::from(millis)),
            )
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parses_real_retry_line_with_google_init_prefix_and_nested_parens() {
        let now_ms = (civil_to_utc_secs(2026, 11, 21, 14, 20, 0).unwrap() as u64) * 1000;
        let log = concat!(
            "ERROR: logging before google.Init: I1121 22:16:20.828895   95269 run.go:317] Run: attempt 1 failed (stream reading error: unexpected EOF), retrying in 4s\n",
            "ERROR: logging before google.Init: I1121 22:16:25.100000   95269 run.go:317] CallWithRetry: attempt 2 failed (HTTP 503 (Service Unavailable)), retrying in 1m30s\n",
        );
        let snap = parse_antigravity_log_tail_with_offset(log, now_ms, 2026, 8 * 3600);
        let retry = snap.latest_retry.expect("retry parsed");
        assert_eq!(retry.attempt, 2);
        assert_eq!(retry.reason, "HTTP 503 (Service Unavailable)");
        assert_eq!(retry.retry_delay.as_deref(), Some("1m30s"));
        let expected_ms =
            (civil_to_utc_secs(2026, 11, 21, 14, 16, 25).unwrap() as u64) * 1000 + 100;
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
        let snap = parse_antigravity_log_tail_with_offset(log, now_ms, 2026, 8 * 3600);
        assert!(snap.latest_retry.is_none());
        assert!(snap.latest_api_error.is_none());
    }

    #[test]
    fn extracts_api_error_variants_and_clears_older_retry() {
        let now_ms = (civil_to_utc_secs(2026, 11, 21, 14, 20, 0).unwrap() as u64) * 1000;
        let log = concat!(
            "ERROR: logging before google.Init: I1121 22:16:20.000000 100 run.go:317] Run: attempt 2 failed (HTTP 429), retrying in 4s\n",
            "ERROR: logging before google.Init: E1121 22:18:18.807524 106587 errorreport.go:285] generating and executing: RESOURCE_EXHAUSTED (code 429): Individual quota reached. Resets in 13m8s\n",
        );
        let snap = parse_antigravity_log_tail_with_offset(log, now_ms, 2026, 8 * 3600);
        assert!(snap.latest_retry.is_none());
        let err = snap.latest_api_error.expect("api error parsed");
        assert_eq!(
            err.reason,
            "RESOURCE_EXHAUSTED (code 429): Individual quota reached. Resets in 13m8s"
        );

        let long_cn = "中文配额提示".repeat(40);
        let long_log = format!(
            "E1121 22:18:18.807524 106587 errorreport.go:285] agent executor error: generating and executing: RESOURCE_EXHAUSTED (code 429): {long_cn} Resets in 13m8s\n"
        );
        let long_snap = parse_antigravity_log_tail_with_offset(&long_log, now_ms, 2026, 8 * 3600);
        let long_err = long_snap.latest_api_error.expect("truncated error");
        assert!(long_err.reason.chars().count() <= MAX_API_ERROR_REASON_CHARS);
        assert!(long_err.reason.ends_with('…'));
    }

    #[test]
    fn resolves_year_rollover_clock_skew_and_unrotated_old_entries_with_5m_tolerance() {
        let new_year_now = (civil_to_utc_secs(2027, 1, 1, 0, 5, 0).unwrap() as u64) * 1000;
        let dec_line =
            "I1231 23:59:30.500000 100 run.go:317] Run: attempt 1 failed (EOF), retrying in 2s\n";
        let dec_snap = parse_antigravity_log_tail_with_offset(dec_line, new_year_now, 2027, 0);
        assert_eq!(
            dec_snap.latest_retry.unwrap().ts_ms,
            (civil_to_utc_secs(2026, 12, 31, 23, 59, 30).unwrap() as u64) * 1000 + 500
        );

        let eve_now = (civil_to_utc_secs(2026, 12, 31, 23, 59, 55).unwrap() as u64) * 1000;
        let skew_line =
            "I0101 00:00:05.000000 100 run.go:317] Run: attempt 1 failed (EOF), retrying in 2s\n";
        let skew_snap = parse_antigravity_log_tail_with_offset(skew_line, eve_now, 2026, 0);
        assert_eq!(
            skew_snap.latest_retry.unwrap().ts_ms,
            (civil_to_utc_secs(2027, 1, 1, 0, 0, 5).unwrap() as u64) * 1000
        );

        let nov_now = (civil_to_utc_secs(2026, 11, 21, 12, 0, 0).unwrap() as u64) * 1000;
        let stale_line =
            "I1205 10:00:00.000000 100 run.go:317] Run: attempt 1 failed (EOF), retrying in 2s\n";
        let stale_snap = parse_antigravity_log_tail_with_offset(stale_line, nov_now, 2026, 0);
        assert_eq!(
            stale_snap.latest_retry.unwrap().ts_ms,
            (civil_to_utc_secs(2025, 12, 5, 10, 0, 0).unwrap() as u64) * 1000
        );
    }

    #[test]
    fn incremental_append_preserves_earlier_429_after_40kb_noise_and_skips_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("language_server.log");
        let now_ms = system_now_ms();
        let now_year = mi_macos::local_year_for_epoch_secs((now_ms / 1000) as i64).unwrap_or(2026);
        let sample_ts_ms =
            mi_macos::local_civil_to_epoch_ms(now_year, 1, 15, 12, 0, 0, 0).unwrap_or(now_ms);
        let effective_now = now_ms.max(sample_ts_ms + 60_000);
        let err_line = "ERROR: logging before google.Init: E0115 12:00:00.000000 100 errorreport.go:285] agent executor error: generating and executing: RESOURCE_EXHAUSTED (code 429): Quota reached. Resets in 10m\n";
        {
            let mut f = fs::File::create(&log_path).unwrap();
            f.write_all(err_line.as_bytes()).unwrap();
        }
        let mut reader = AntigravityLogTailReader::new();
        let first = reader.read_snapshot_from(&log_path, effective_now);
        assert!(first
            .latest_api_error
            .as_ref()
            .is_some_and(|e| e.reason.contains("RESOURCE_EXHAUSTED")));

        {
            let mut f = fs::OpenOptions::new().append(true).open(&log_path).unwrap();
            let noise_line = "ERROR: logging before google.Init: I0115 12:00:10.000000 100 llm_monitor.go:706] LLM Monitor sir_flash_lite: Calling LLM padding_0123456789abcdef0123456789abcdef\n";
            for _ in 0..320 {
                f.write_all(noise_line.as_bytes()).unwrap();
            }
        }
        assert!(fs::metadata(&log_path).unwrap().len() > 45_000);
        let after_noise = reader.read_snapshot_from(&log_path, effective_now + 5_000);
        assert_eq!(after_noise.latest_api_error, first.latest_api_error);

        #[cfg(unix)]
        {
            let link_path = dir.path().join("symlink.log");
            std::os::unix::fs::symlink(&log_path, &link_path).unwrap();
            let linked = reader.read_snapshot_from(&link_path, effective_now + 6_000);
            assert_eq!(linked, AntigravityLogSnapshot::default());
        }
    }

    #[test]
    fn parses_go_durations_accurately() {
        assert_eq!(parse_go_duration_ms("4s"), Some(4_000));
        assert_eq!(parse_go_duration_ms("500ms"), Some(500));
        assert_eq!(parse_go_duration_ms("1m30s"), Some(90_000));
        assert_eq!(parse_go_duration_ms("2.5s"), Some(2_500));
        assert_eq!(parse_go_duration_ms("bad"), None);
    }
}
