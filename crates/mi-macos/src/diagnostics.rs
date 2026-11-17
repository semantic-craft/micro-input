use crate::private_fs::{open_private_file, PrivateOpenMode};
use serde_json::Value;
use std::io::Write;
use std::path::Path;

pub fn write_private_json(path: &Path, value: &Value) -> Result<(), String> {
    let mut text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    text.push('\n');
    let mut file = open_private_file(path, PrivateOpenMode::Truncate)?;
    file.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())
}

#[cfg(any(test, feature = "qa-runner"))]
pub(crate) fn append_private_jsonl(path: &Path, value: &Value) -> Result<(), String> {
    let mut line = serde_json::to_string(value).map_err(|e| e.to_string())?;
    line.push('\n');
    let mut file = open_private_file(path, PrivateOpenMode::Append)?;
    file.write_all(line.as_bytes()).map_err(|e| e.to_string())
}

pub struct DiagnosticRedactor;

impl DiagnosticRedactor {
    pub fn sanitize(message: &str) -> String {
        let payloads_redacted = redact_bracketed_payloads(message);
        let paths_redacted = redact_system_paths(&payloads_redacted);
        let serials_redacted = redact_serial_tokens(&paths_redacted);
        redact_mac_addresses(&serials_redacted)
    }
}

fn redact_bracketed_payloads(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '{' || ch == '[' {
            let close = if ch == '{' { '}' } else { ']' };
            let mut depth = 1usize;
            let mut j = i + 1;
            while j < chars.len() && depth > 0 {
                if chars[j] == ch {
                    depth += 1;
                } else if chars[j] == close {
                    depth -= 1;
                }
                j += 1;
            }
            if depth == 0 {
                out.push_str("[redacted-payload]");
                i = j;
                continue;
            }
        }
        out.push(ch);
        i += 1;
    }
    out
}

fn redact_system_paths(input: &str) -> String {
    const ROOTS: [&str; 7] = [
        "/applications/",
        "/private/",
        "/system/",
        "/users/",
        "/volumes/",
        "/var/",
        "/tmp/",
    ];
    const EXACT_ROOTS: [&str; 2] = ["/private", "/tmp"];

    let lower = input.to_ascii_lowercase();
    let bytes = input.as_bytes();
    let lower_bytes = lower.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] == b'/' {
            let rem_lower = &lower[i..];
            let matched_root = ROOTS.iter().any(|r| rem_lower.starts_with(r))
                || EXACT_ROOTS.iter().any(|r| {
                    rem_lower.starts_with(r)
                        && (i + r.len() == lower_bytes.len()
                            || is_path_terminator(bytes[i + r.len()]))
                });
            if matched_root {
                out.push_str("[path]");
                i += 1;
                while i < bytes.len() && !is_path_terminator(bytes[i]) {
                    i += 1;
                }
                continue;
            }
        }
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn is_path_terminator(b: u8) -> bool {
    b.is_ascii_whitespace() || matches!(b, b'"' | b'\'' | b',' | b';' | b')' | b'>')
}

fn redact_serial_tokens(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    while i < input.len() {
        let rem_lower = &lower[i..];
        let key_len = if rem_lower.starts_with("serial_number") {
            Some("serial_number".len())
        } else if rem_lower.starts_with("serial") {
            Some("serial".len())
        } else {
            None
        };
        if let Some(klen) = key_len {
            let mut j = i + klen;
            let bytes = input.as_bytes();
            while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                j += 1;
            }
            if j < bytes.len() && (bytes[j] == b':' || bytes[j] == b'=') {
                let sep = bytes[j] as char;
                j += 1;
                while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                    j += 1;
                }
                let val_start = j;
                while j < bytes.len()
                    && (bytes[j].is_ascii_alphanumeric()
                        || matches!(bytes[j], b'-' | b'_' | b'.' | b':'))
                {
                    j += 1;
                }
                if j > val_start {
                    out.push_str(&input[i..i + klen]);
                    out.push(sep);
                    out.push_str("[redacted]");
                    i = j;
                    continue;
                }
            }
        }
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn is_six_octet_mac(bytes: &[u8], start: usize) -> bool {
    const MAC_LEN: usize = 17;
    if start + MAC_LEN > bytes.len() {
        return false;
    }
    if start > 0 && bytes[start - 1].is_ascii_hexdigit() {
        return false;
    }
    if start + MAC_LEN < bytes.len() && bytes[start + MAC_LEN].is_ascii_hexdigit() {
        return false;
    }
    let delim = bytes[start + 2];
    if !matches!(delim, b':' | b'-') {
        return false;
    }
    (0..6).all(|group| {
        let off = start + group * 3;
        bytes[off].is_ascii_hexdigit()
            && bytes[off + 1].is_ascii_hexdigit()
            && (group == 5 || bytes[off + 2] == delim)
    })
}

fn redact_mac_addresses(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if is_six_octet_mac(bytes, i) {
            out.push_str("[redacted]");
            i += 17;
            continue;
        }
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::time::Duration;

    #[test]
    fn private_evidence_writers_enforce_0700_dirs_and_reject_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let evidence = dir.path().join("evidence");
        fs::create_dir_all(&evidence).unwrap();
        fs::set_permissions(&evidence, fs::Permissions::from_mode(0o755)).unwrap();

        let json_path = evidence.join("report.json");
        write_private_json(&json_path, &json!({"ok": true})).unwrap();
        assert_eq!(
            fs::metadata(&evidence).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&json_path).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let target_file = dir.path().join("victim.txt");
        fs::write(&target_file, b"ORIGINAL").unwrap();
        let symlink_json = evidence.join("link.json");
        symlink(&target_file, &symlink_json).unwrap();
        assert!(
            write_private_json(&symlink_json, &json!({"pwned": true})).is_err(),
            "write_private_json must refuse symlinks via O_NOFOLLOW"
        );
        assert_eq!(fs::read(&target_file).unwrap(), b"ORIGINAL");

        let nested_evidence = dir.path().join("nested_evidence");
        let jsonl_path = nested_evidence.join("audit.jsonl");
        append_private_jsonl(&jsonl_path, &json!({"line": 1})).unwrap();
        assert_eq!(
            fs::metadata(&nested_evidence).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&jsonl_path).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let symlink_jsonl = nested_evidence.join("link.jsonl");
        symlink(&target_file, &symlink_jsonl).unwrap();
        assert!(
            append_private_jsonl(&symlink_jsonl, &json!({"pwned": true})).is_err(),
            "append_private_jsonl must refuse symlinks via O_NOFOLLOW"
        );
        assert_eq!(fs::read(&target_file).unwrap(), b"ORIGINAL");
    }

    #[test]
    fn diagnostic_redactor_masks_colon_serials_and_bare_macs_without_touching_vid_pid_or_timestamps(
    ) {
        let kv_sanitized = DiagnosticRedactor::sanitize(
            "usb serial_number: AA:BB:CC:DD:EE:FF serial=11:22:33:44:55:66",
        );
        assert!(kv_sanitized.contains("serial_number:[redacted]"));
        assert!(kv_sanitized.contains("serial=[redacted]"));
        assert!(!kv_sanitized.contains("AA:BB"));
        assert!(!kv_sanitized.contains("CC:DD"));
        assert!(!kv_sanitized.contains("11:22"));
        assert!(!kv_sanitized.contains("33:44"));

        let bare_sanitized = DiagnosticRedactor::sanitize(
            "device 02:00:5e:10:00:01 failed at 12:34:56 on 303A:8298",
        );
        assert!(!bare_sanitized.contains("02:00:5e:10:00:01"));
        assert!(bare_sanitized.contains("12:34:56"));
        assert!(bare_sanitized.contains("303A:8298"));

        let hyphen_sanitized = DiagnosticRedactor::sanitize(
            "device 02-00-5e-10-00-01 failed on 2026-11-16 uuid 12345678-1234-1234-1234-123456789012 vid 303A:8298 mixed 02:00-5e:10-00:01",
        );
        assert!(!hyphen_sanitized.contains("02-00-5e-10-00-01"));
        assert!(hyphen_sanitized.contains("2026-11-16"));
        assert!(hyphen_sanitized.contains("12345678-1234-1234-1234-123456789012"));
        assert!(hyphen_sanitized.contains("303A:8298"));
        assert!(hyphen_sanitized.contains("02:00-5e:10-00:01"));
    }

    #[test]
    fn private_fs_rejects_symlink_dirs_preserves_target_perms_and_refuses_fifo_nonblocking() {
        let dir = tempfile::tempdir().unwrap();
        let victim_dir = dir.path().join("victim_dir");
        fs::create_dir_all(&victim_dir).unwrap();
        fs::set_permissions(&victim_dir, fs::Permissions::from_mode(0o755)).unwrap();

        let link_dir = dir.path().join("link_dir");
        symlink(&victim_dir, &link_dir).unwrap();
        assert!(
            write_private_json(&link_dir.join("out.json"), &json!({"ok": true})).is_err(),
            "target dir symlink must be rejected"
        );
        assert_eq!(
            fs::metadata(&victim_dir).unwrap().permissions().mode() & 0o777,
            0o755,
            "symlink target directory permissions must not be modified"
        );
        assert!(
            write_private_json(&link_dir.join("nested/out.json"), &json!({"ok": true})).is_err(),
            "intermediate user-owned symlink must be rejected"
        );
        assert_eq!(
            fs::metadata(&victim_dir).unwrap().permissions().mode() & 0o777,
            0o755
        );

        let fifo_dir = dir.path().join("fifo_dir");
        fs::create_dir_all(&fifo_dir).unwrap();
        fs::set_permissions(&fifo_dir, fs::Permissions::from_mode(0o700)).unwrap();
        let fifo_path = fifo_dir.join("pipe.json");
        let c_fifo = std::ffi::CString::new(fifo_path.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c_fifo.as_ptr(), 0o600) }, 0);

        let (tx, rx) = std::sync::mpsc::channel();
        let fifo_for_thread = fifo_path.clone();
        let handle = std::thread::spawn(move || {
            let res = write_private_json(&fifo_for_thread, &json!({"secret": 1}));
            let _ = tx.send(res);
        });
        let recv_res = rx.recv_timeout(Duration::from_millis(200));
        if recv_res.is_err() {
            let drain_fd =
                unsafe { libc::open(c_fifo.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK) };
            let _ = handle.join();
            if drain_fd >= 0 {
                unsafe { libc::close(drain_fd) };
            }
            panic!("write_private_json blocked when opening FIFO without O_NONBLOCK");
        }
        let _ = handle.join();
        assert!(
            recv_res.unwrap().is_err(),
            "FIFO path must be rejected immediately"
        );

        let reader_fd = unsafe { libc::open(c_fifo.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK) };
        assert!(reader_fd >= 0);
        let active_res = write_private_json(&fifo_path, &json!({"secret": 2}));
        let mut buf = [0u8; 32];
        let n = unsafe { libc::read(reader_fd, buf.as_mut_ptr().cast(), buf.len()) };
        unsafe { libc::close(reader_fd) };
        assert!(
            active_res.is_err(),
            "FIFO with active reader must still be rejected"
        );
        assert!(n <= 0, "must never write bytes to FIFO");
    }
}
