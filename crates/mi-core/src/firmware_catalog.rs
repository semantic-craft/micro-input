use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    cmp::Ordering,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
};

pub const RELEASES_URL: &str = "https://github.com/worklouder/cm-v2-fw-releases/releases";
const API: &str = "https://api.github.com/repos/worklouder/cm-v2-fw-releases/releases";
const API_ALLOWED_PREFIX: &str = "https://api.github.com/repos/worklouder/cm-v2-fw-releases/";
const MAX_API_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_FIRMWARE_ASSET_BYTES: u64 = 32 * 1024 * 1024;
const MAX_CURL_STDERR_BYTES: u64 = 16 * 1024;
const MAX_PRERELEASE_PAGES: u64 = 10;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FirmwareChannel {
    #[default]
    Stable,
    Prerelease,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirmwareAsset {
    pub name: String,
    pub url: String,
    pub size: u64,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirmwareRelease {
    pub tag: String,
    pub version: String,
    pub prerelease: bool,
    pub notes: String,
    pub release_url: String,
    pub published_at: Option<String>,
    pub asset: FirmwareAsset,
}

pub use mi_session::firmware_flash::DownloadedFirmware;

#[derive(Deserialize)]
struct ReleaseResponse {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    html_url: String,
    body: Option<String>,
    published_at: Option<String>,
    assets: Vec<AssetResponse>,
}

#[derive(Deserialize)]
struct AssetResponse {
    name: String,
    browser_download_url: String,
    size: u64,
    digest: Option<String>,
    state: String,
}

#[derive(Debug, PartialEq, Eq)]
struct Version {
    core: [u64; 3],
    pre: Vec<String>,
}

impl Version {
    fn parse(input: &str) -> Result<Self, String> {
        let invalid = || format!("无法识别固件版本：{input}");
        let value = input.strip_prefix('v').unwrap_or(input);
        let (value, build) = value
            .split_once('+')
            .map_or((value, None), |(v, b)| (v, Some(b)));
        if build.is_some_and(|b| !valid_identifiers(b, false)) {
            return Err(invalid());
        }
        let (core, pre) = value.split_once('-').map_or((value, ""), |(v, p)| (v, p));
        let parts: Vec<_> = core.split('.').collect();
        if parts.len() != 3 || parts.iter().any(|p| !valid_number(p)) {
            return Err(invalid());
        }
        let core = [
            parts[0].parse().map_err(|_| invalid())?,
            parts[1].parse().map_err(|_| invalid())?,
            parts[2].parse().map_err(|_| invalid())?,
        ];
        if value.contains('-') && !valid_identifiers(pre, false) {
            return Err(invalid());
        }
        Ok(Self {
            core,
            pre: if pre.is_empty() {
                vec![]
            } else {
                pre.split('.').map(str::to_owned).collect()
            },
        })
    }
    fn compare(&self, other: &Self) -> Ordering {
        let core = self.core.cmp(&other.core);
        if core != Ordering::Equal {
            return core;
        }
        match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, false) => return Ordering::Greater,
            (false, true) => return Ordering::Less,
            _ => {}
        }
        for (a, b) in self.pre.iter().zip(&other.pre) {
            let numeric = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
            let ordering = match (numeric(a), numeric(b)) {
                (true, true) => {
                    let a = a.trim_start_matches('0');
                    let b = b.trim_start_matches('0');
                    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
                }
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                _ => a.cmp(b),
            };
            if ordering != Ordering::Equal {
                return ordering;
            }
        }
        self.pre.len().cmp(&other.pre.len())
    }
}
fn valid_number(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|b| b.is_ascii_digit())
        && (value.len() == 1 || !value.starts_with('0'))
}
fn valid_identifiers(value: &str, numeric_rule: bool) -> bool {
    value.split('.').all(|p| {
        !p.is_empty()
            && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && (!numeric_rule || !p.bytes().all(|b| b.is_ascii_digit()) || valid_number(p))
    })
}

pub fn compare_versions(current: &str, candidate: &str) -> Result<Ordering, String> {
    Ok(Version::parse(current)?.compare(&Version::parse(candidate)?))
}

fn convert_release(r: ReleaseResponse) -> Result<FirmwareRelease, String> {
    let version = r
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&r.tag_name)
        .to_owned();
    let parsed = Version::parse(&version)?;
    if r.draft || (!parsed.pre.is_empty() && !r.prerelease) {
        return Err("发布状态与固件版本不一致".into());
    }
    let name = format!("firmware_{version}_merged.bin");
    let expected_url = format!("{RELEASES_URL}/download/{}/{name}", r.tag_name);
    let mut matches = r.assets.into_iter().filter(|a| a.name == name);
    let asset = matches
        .next()
        .ok_or("发布中没有匹配的 Creator Micro 2 固件文件")?;
    if matches.next().is_some()
        || asset.browser_download_url != expected_url
        || asset.state != "uploaded"
        || asset.size == 0
    {
        return Err("固件附件身份或上传状态无效".into());
    }
    let sha256 = match asset.digest {
        Some(d) => {
            let digest = d.strip_prefix("sha256:").ok_or("固件附件摘要格式不支持")?;
            if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("固件附件摘要无效".into());
            }
            Some(digest.to_ascii_lowercase())
        }
        None => None,
    };
    if r.html_url != format!("{RELEASES_URL}/tag/{}", r.tag_name) {
        return Err("固件发布链接不属于官方仓库".into());
    }
    Ok(FirmwareRelease {
        tag: r.tag_name,
        version,
        prerelease: r.prerelease,
        notes: r.body.unwrap_or_default(),
        release_url: r.html_url,
        published_at: r.published_at,
        asset: FirmwareAsset {
            name,
            url: expected_url,
            size: asset.size,
            sha256,
        },
    })
}

pub fn parse_stable_release(bytes: &[u8]) -> Result<FirmwareRelease, String> {
    let r = convert_release(
        serde_json::from_slice(bytes).map_err(|e| format!("固件发布信息解析失败：{e}"))?,
    )?;
    if r.prerelease {
        return Err("稳定通道返回了预发布固件".into());
    }
    Ok(r)
}

fn validate_firmware_url(url: &str) -> Result<(), String> {
    if url.is_empty()
        || url
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '#' | '\\' | '@'))
        || url.contains("..")
    {
        return Err("不允许的固件请求地址".into());
    }
    let download_prefix = format!("{RELEASES_URL}/download/");
    if !(url.starts_with(API_ALLOWED_PREFIX) || url.starts_with(&download_prefix)) {
        return Err("不允许的固件请求地址".into());
    }
    Ok(())
}

fn response_byte_limit(size: Option<u64>) -> Result<u64, String> {
    match size {
        Some(n) if n > MAX_FIRMWARE_ASSET_BYTES => Err(format!(
            "固件大小超出安全上限（{MAX_FIRMWARE_ASSET_BYTES} 字节）"
        )),
        Some(n) => Ok(n),
        None => Ok(MAX_API_RESPONSE_BYTES),
    }
}

fn build_curl_command(url: &str, size: Option<u64>) -> Result<(Command, u64), String> {
    validate_firmware_url(url)?;
    let limit = response_byte_limit(size)?;
    let mut command = Command::new("/usr/bin/curl");
    command.args([
        "--disable",
        "--fail",
        "--silent",
        "--show-error",
        "--location",
        "--max-redirs",
        "3",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--connect-timeout",
        "15",
        "--max-time",
        "180",
        "--user-agent",
        "MicroInput-Firmware",
        "--header",
        "Accept: application/vnd.github+json",
    ]);
    if let Some(size) = size {
        command.args(["--max-filesize", &size.to_string()]);
    }
    command.args(["--", url]);
    Ok((command, limit))
}

fn execute_bounded_command(command: &mut Command, max_bytes: u64) -> Result<Vec<u8>, String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| format!("无法检查固件发布：{e}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "无法读取固件响应输出".to_string())?;
    let stderr = child.stderr.take();
    let stderr_thread = std::thread::spawn(move || {
        let mut err_buf = Vec::new();
        if let Some(mut pipe) = stderr {
            let _ = (&mut pipe)
                .take(MAX_CURL_STDERR_BYTES)
                .read_to_end(&mut err_buf);
            let _ = std::io::copy(&mut pipe, &mut std::io::sink());
        }
        err_buf
    });

    let mut out_buf = Vec::new();
    let read_result = stdout
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut out_buf);
    if read_result.as_ref().is_ok() && (out_buf.len() as u64) > max_bytes {
        let _ = child.kill();
        let _ = child.wait();
        let _ = stderr_thread.join();
        return Err(format!("固件响应大小超出安全上限（{max_bytes} 字节）"));
    }

    let status = child.wait().map_err(|e| format!("无法检查固件发布：{e}"))?;
    let stderr_bytes = stderr_thread.join().unwrap_or_default();
    if let Err(err) = read_result {
        return Err(format!("读取固件响应失败：{err}"));
    }
    if !status.success() {
        return Err(format!(
            "固件请求失败：{}",
            String::from_utf8_lossy(&stderr_bytes).trim()
        ));
    }
    Ok(out_buf)
}

fn request(url: &str, size: Option<u64>) -> Result<Vec<u8>, String> {
    let (mut command, limit) = build_curl_command(url, size)?;
    execute_bounded_command(&mut command, limit)
}

fn fetch_prerelease_with<F>(mut fetch_page: F) -> Result<FirmwareRelease, String>
where
    F: FnMut(u64) -> Result<Vec<u8>, String>,
{
    let mut selected: Option<ReleaseResponse> = None;
    for page in 1..=MAX_PRERELEASE_PAGES {
        let raw = fetch_page(page)?;
        let releases: Vec<ReleaseResponse> =
            serde_json::from_slice(&raw).map_err(|e| format!("固件目录解析失败：{e}"))?;
        let count = releases.len();
        select_newest(&mut selected, releases)?;
        if count < 100 {
            return convert_release(selected.ok_or("官方仓库暂无可用固件")?);
        }
    }
    Err(format!(
        "官方固件发布分页超出安全上限（最多 {MAX_PRERELEASE_PAGES} 页）"
    ))
}

pub fn fetch_release(channel: FirmwareChannel) -> Result<FirmwareRelease, String> {
    if channel == FirmwareChannel::Stable {
        return parse_stable_release(&request(&format!("{API}/latest"), None)?);
    }
    fetch_prerelease_with(|page| request(&format!("{API}?per_page=100&page={page}"), None))
}

fn select_newest(
    selected: &mut Option<ReleaseResponse>,
    releases: Vec<ReleaseResponse>,
) -> Result<(), String> {
    for response in releases.into_iter().filter(|r| !r.draft) {
        Version::parse(&response.tag_name)?;
        if selected
            .as_ref()
            .map(|s| compare_versions(&s.tag_name, &response.tag_name))
            .transpose()?
            .is_none_or(|o| o == Ordering::Less)
        {
            *selected = Some(response);
        }
    }
    Ok(())
}

pub fn verify_release_bytes(release: &FirmwareRelease, bytes: &[u8]) -> Result<String, String> {
    if bytes.len() as u64 != release.asset.size {
        return Err("固件下载大小与发布信息不一致".into());
    }
    let digest = format!("{:x}", Sha256::digest(bytes));
    if release
        .asset
        .sha256
        .as_ref()
        .is_some_and(|expected| *expected != digest)
    {
        return Err("固件 SHA-256 校验失败".into());
    }
    Ok(digest)
}

pub fn download_release(
    release: &FirmwareRelease,
    destination: &Path,
) -> Result<DownloadedFirmware, String> {
    download_release_with(release, destination, |url, size| request(url, Some(size)))
}

fn download_release_with<F>(
    release: &FirmwareRelease,
    destination: &Path,
    mut fetch: F,
) -> Result<DownloadedFirmware, String>
where
    F: FnMut(&str, u64) -> Result<Vec<u8>, String>,
{
    Version::parse(&release.version)?;
    if release.tag.strip_prefix('v').unwrap_or(&release.tag) != release.version
        || release.asset.name != format!("firmware_{}_merged.bin", release.version)
        || release.asset.url
            != format!(
                "{RELEASES_URL}/download/{}/{}",
                release.tag, release.asset.name
            )
    {
        return Err("固件附件与所选发布不匹配".into());
    }
    validate_firmware_url(&release.asset.url)?;
    let _ = response_byte_limit(Some(release.asset.size))?;
    let bytes = fetch(&release.asset.url, release.asset.size)?;
    let sha256 = verify_release_bytes(release, &bytes)?;
    let mut file = mi_macos::open_private_file(destination, mi_macos::PrivateOpenMode::CreateNew)
        .map_err(|e| format!("无法保存固件：{e}"))?;
    if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(destination);
        return Err(format!("无法保存完整固件：{error}"));
    }
    Ok(DownloadedFirmware {
        path: destination.to_owned(),
        sha256,
        digest_verified: release.asset.sha256.is_some(),
        size: bytes.len() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture(version: &str, prerelease: bool) -> serde_json::Value {
        json!({"tag_name":format!("v{version}"),"draft":false,"prerelease":prerelease,"html_url":format!("{RELEASES_URL}/tag/v{version}"),"body":"Release notes", "published_at":"2026-11-01T09:12:13Z", "assets":[{"name":format!("firmware_{version}_merged.bin"),"browser_download_url":format!("{RELEASES_URL}/download/v{version}/firmware_{version}_merged.bin"),"size":3,"digest":format!("sha256:{:x}",Sha256::digest(b"abc")),"state":"uploaded"}]})
    }
    #[test]
    fn semantic_versions_order_rc_numerically_and_stable_after_rc() {
        for (a, b) in [
            ("0.6.3", "0.7.0-rc.1"),
            ("0.7.0-rc.9", "0.7.0-rc.16"),
            ("0.7.0-rc.01", "0.7.0-rc.10"),
            ("0.7.0-rc.16", "0.7.0"),
        ] {
            assert_eq!(compare_versions(a, b).unwrap(), Ordering::Less);
        }
        assert_eq!(
            compare_versions("v0.6.3+build.1", "0.6.3").unwrap(),
            Ordering::Equal
        );
        for bad in ["0.6", "0.06.3", "0.7.0-", "0.7.0+", "0.7.0/path"] {
            assert!(compare_versions(bad, "0.6.3").is_err());
        }
    }
    #[test]
    fn exact_asset_and_channel_validation() {
        let mut value = fixture("0.6.3", false);
        value["assets"].as_array_mut().unwrap().push(json!({"name":"unrelated.bin","browser_download_url":"https://example.com/unrelated.bin","size":42,"state":"uploaded"}));
        let r = parse_stable_release(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(r.asset.name, "firmware_0.6.3_merged.bin");
        assert!(
            parse_stable_release(&serde_json::to_vec(&fixture("0.7.0-rc.16", true)).unwrap())
                .is_err()
        );
        value["assets"][0]["browser_download_url"] =
            json!("https://example.com/firmware_0.6.3_merged.bin");
        assert!(parse_stable_release(&serde_json::to_vec(&value).unwrap()).is_err());
        assert!(parse_stable_release(b"rate limited").is_err());
    }
    #[test]
    fn newest_selection_ignores_order_and_old_asset_conventions() {
        let mut old = fixture("0.6.1", false);
        old["assets"][0]["name"] = json!("firmware_v0.6.1_merged.bin");
        let mut draft = fixture("9.0.0", false);
        draft["draft"] = json!(true);
        let values = vec![
            fixture("0.7.0-rc.9", true),
            old,
            fixture("0.6.3", false),
            fixture("0.7.0-rc.16", true),
            draft,
            fixture("0.7.0-rc.01", true),
        ];
        let mut selected = None;
        select_newest(
            &mut selected,
            serde_json::from_value(json!(values)).unwrap(),
        )
        .unwrap();
        assert_eq!(
            convert_release(selected.unwrap()).unwrap().version,
            "0.7.0-rc.16"
        );
    }
    #[test]
    fn partial_or_changed_download_rejected_and_missing_digest_explicit() {
        let mut r =
            parse_stable_release(&serde_json::to_vec(&fixture("0.6.3", false)).unwrap()).unwrap();
        assert!(verify_release_bytes(&r, b"ab").is_err());
        assert!(verify_release_bytes(&r, b"abd").is_err());
        assert!(verify_release_bytes(&r, b"abc").is_ok());
        r.asset.sha256 = None;
        assert!(verify_release_bytes(&r, b"abc").is_ok());
        assert!(r.asset.sha256.is_none());
    }

    #[test]
    fn request_rejects_disallowed_urls_control_chars_and_oversized_limit() {
        let dir = tempfile::tempdir().unwrap();
        let local_secret = dir.path().join("local-secret.txt");
        std::fs::write(&local_secret, b"LOCAL_FILE_LEAK").unwrap();

        let file_url = format!("file://{}", local_secret.display());
        assert!(
            request(&file_url, None).is_err(),
            "file:// URL must be rejected before invoking curl"
        );

        for bad in [
            "http://api.github.com/repos/worklouder/cm-v2-fw-releases/releases/latest",
            "https://evil.example/repos/worklouder/cm-v2-fw-releases/releases/latest",
            "https://github.com/other/repo/releases/download/v1.0.0/fw.bin",
            "https://api.github.com/repos/worklouder/cm-v2-fw-releases/releases/latest\n--output=/tmp/x",
            "https://github.com/worklouder/cm-v2-fw-releases/releases/download/v1.0.0/fw .bin",
            "-K/etc/passwd",
        ] {
            let err = request(bad, None).unwrap_err();
            assert!(
                err.contains("不允许") || err.contains("URL") || err.contains("地址"),
                "expected URL validation rejection for {bad}, got: {err}"
            );
        }

        let oversized_err = request(
            &format!("{RELEASES_URL}/download/v0.6.3/firmware_0.6.3_merged.bin"),
            Some(64 * 1024 * 1024),
        )
        .unwrap_err();
        assert!(
            oversized_err.contains("上限") || oversized_err.contains("大小"),
            "expected size ceiling error, got: {oversized_err}"
        );
    }

    #[test]
    fn build_curl_command_sets_redirect_and_protocol_bounds() {
        let (cmd, limit) = build_curl_command(
            &format!("{RELEASES_URL}/download/v0.6.3/firmware_0.6.3_merged.bin"),
            Some(128),
        )
        .unwrap();
        assert_eq!(limit, 128);
        assert_eq!(cmd.get_program(), "/usr/bin/curl");
        let args: Vec<&str> = cmd.get_args().map(|a| a.to_str().unwrap()).collect();
        assert!(args.windows(2).any(|w| w == ["--max-redirs", "3"]));
        assert!(args.windows(2).any(|w| w == ["--proto", "=https"]));
        assert!(args.windows(2).any(|w| w == ["--proto-redir", "=https"]));
        assert!(args.contains(&"--"));
    }

    #[test]
    fn execute_bounded_command_enforces_stdout_cap_and_bounds_stderr() {
        let mut ok_cmd = Command::new("/bin/sh");
        ok_cmd.args(["-c", "printf 'hello'"]);
        assert_eq!(execute_bounded_command(&mut ok_cmd, 5).unwrap(), b"hello");

        let mut overflow_cmd = Command::new("/bin/sh");
        overflow_cmd.args(["-c", "printf '123456'"]);
        let err = execute_bounded_command(&mut overflow_cmd, 5).unwrap_err();
        assert!(err.contains("上限"), "unexpected error: {err}");

        let mut stderr_cmd = Command::new("/bin/sh");
        stderr_cmd.args(["-c", "printf 'x%.0s' {1..25000} >&2; exit 22"]);
        let err = execute_bounded_command(&mut stderr_cmd, 1024).unwrap_err();
        assert!(err.len() <= (MAX_CURL_STDERR_BYTES as usize) + 64);
        assert!(err.contains("固件请求失败"));
    }

    #[test]
    fn fetch_prerelease_with_caps_pagination_at_max_pages() {
        let full_page = vec![fixture("0.7.0-rc.1", true); 100];
        let payload = serde_json::to_vec(&full_page).unwrap();
        let mut calls = 0u64;
        let err = fetch_prerelease_with(|page| {
            calls += 1;
            assert_eq!(page, calls);
            Ok(payload.clone())
        })
        .unwrap_err();
        assert_eq!(calls, MAX_PRERELEASE_PAGES);
        assert!(err.contains("分页超出安全上限"));
    }

    #[cfg(unix)]
    #[test]
    fn download_release_enforces_0600_mode_and_refuses_symlinks() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let dir = tempfile::tempdir().unwrap();
        let release =
            parse_stable_release(&serde_json::to_vec(&fixture("0.6.3", false)).unwrap()).unwrap();
        let dest = dir.path().join("fw.bin");
        let downloaded =
            download_release_with(&release, &dest, |_, _| Ok(b"abc".to_vec())).unwrap();
        assert_eq!(downloaded.size, 3);
        assert_eq!(
            std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let victim = dir.path().join("victim.bin");
        std::fs::write(&victim, b"KEEP").unwrap();
        let link_dest = dir.path().join("fw-link.bin");
        symlink(&victim, &link_dest).unwrap();
        assert!(
            download_release_with(&release, &link_dest, |_, _| Ok(b"abc".to_vec())).is_err(),
            "symlink destination must be refused"
        );
        assert_eq!(std::fs::read(&victim).unwrap(), b"KEEP");

        let real_parent = dir.path().join("real_parent");
        std::fs::create_dir_all(&real_parent).unwrap();
        std::fs::set_permissions(&real_parent, std::fs::Permissions::from_mode(0o755)).unwrap();
        let link_parent = dir.path().join("link_parent");
        symlink(&real_parent, &link_parent).unwrap();
        assert!(
            download_release_with(&release, &link_parent.join("fw.bin"), |_, _| Ok(
                b"abc".to_vec()
            ))
            .is_err(),
            "symlink parent dir must be refused"
        );
        assert_eq!(
            std::fs::metadata(&real_parent)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
    }
}
