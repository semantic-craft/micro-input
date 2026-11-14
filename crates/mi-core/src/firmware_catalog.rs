use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    cmp::Ordering,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

pub const RELEASES_URL: &str = "https://github.com/worklouder/cm-v2-fw-releases/releases";
const API: &str = "https://api.github.com/repos/worklouder/cm-v2-fw-releases/releases";

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadedFirmware {
    pub path: PathBuf,
    pub sha256: String,
    pub digest_verified: bool,
    pub size: u64,
}

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

/// Compares current against candidate; Less means an upgrade is available.
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

fn request(url: &str, size: Option<u64>) -> Result<Vec<u8>, String> {
    let mut command = Command::new("/usr/bin/curl");
    command.args([
        "--disable",
        "--fail",
        "--silent",
        "--show-error",
        "--location",
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
    let output = command
        .arg(url)
        .output()
        .map_err(|e| format!("无法检查固件发布：{e}"))?;
    if !output.status.success() {
        return Err(format!(
            "固件请求失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}

/// Blocking network operation; invoke on a worker thread, never on the UI thread.
pub fn fetch_release(channel: FirmwareChannel) -> Result<FirmwareRelease, String> {
    if channel == FirmwareChannel::Stable {
        return parse_stable_release(&request(&format!("{API}/latest"), None)?);
    }
    let mut selected: Option<ReleaseResponse> = None;
    let mut page = 1u64;
    loop {
        let releases: Vec<ReleaseResponse> =
            serde_json::from_slice(&request(&format!("{API}?per_page=100&page={page}"), None)?)
                .map_err(|e| format!("固件目录解析失败：{e}"))?;
        let count = releases.len();
        select_newest(&mut selected, releases)?;
        if count < 100 {
            break;
        }
        page += 1;
    }
    convert_release(selected.ok_or("官方仓库暂无可用固件")?)
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

/// Saves a new file only after size and published digest validation. Missing upstream
/// digests remain explicitly unverified; a local hash is not an authenticity proof.
pub fn download_release(
    release: &FirmwareRelease,
    destination: &Path,
) -> Result<DownloadedFirmware, String> {
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
    let bytes = request(&release.asset.url, Some(release.asset.size))?;
    let sha256 = verify_release_bytes(release, &bytes)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
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
}
