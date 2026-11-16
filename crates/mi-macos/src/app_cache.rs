use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

pub const OTHER_APPS_WILDCARD_BUNDLE_ID: &str = "*";
const MAX_INFO_PLIST_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppBundleInfo {
    pub bundle_id: String,
    pub installed: bool,
    pub app_path: Option<PathBuf>,
    pub display_name: Option<String>,
    pub icon_cached: bool,
}

pub type AppBundleResolver = Arc<dyn Fn(&str) -> AppBundleInfo + Send + Sync>;

pub struct AppBundleCache {
    entries: RwLock<HashMap<String, AppBundleInfo>>,
    resolver: AppBundleResolver,
    lookup_count: AtomicUsize,
    cache_hits: AtomicUsize,
    generation: AtomicU64,
}

impl Default for AppBundleCache {
    fn default() -> Self {
        Self::new()
    }
}

impl AppBundleCache {
    pub fn new() -> Self {
        Self::with_resolver(Arc::new(default_resolve_app_bundle))
    }

    pub fn with_resolver(resolver: AppBundleResolver) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            resolver,
            lookup_count: AtomicUsize::new(0),
            cache_hits: AtomicUsize::new(0),
            generation: AtomicU64::new(1),
        }
    }

    pub fn resolve(&self, bundle_id: &str) -> AppBundleInfo {
        let trimmed = bundle_id.trim();
        if trimmed == OTHER_APPS_WILDCARD_BUNDLE_ID {
            return AppBundleInfo {
                bundle_id: OTHER_APPS_WILDCARD_BUNDLE_ID.to_string(),
                installed: true,
                app_path: None,
                display_name: Some("其他软件".to_string()),
                icon_cached: false,
            };
        }

        if let Some(found) = self
            .entries
            .read()
            .expect("app cache read lock")
            .get(trimmed)
            .cloned()
        {
            self.cache_hits.fetch_add(1, Ordering::SeqCst);
            return found;
        }

        let resolved = (self.resolver)(trimmed);
        self.lookup_count.fetch_add(1, Ordering::SeqCst);

        let mut writer = self.entries.write().expect("app cache write lock");
        writer.insert(trimmed.to_string(), resolved.clone());
        resolved
    }

    pub fn is_installed(&self, bundle_id: &str) -> bool {
        let trimmed = bundle_id.trim();
        if trimmed == OTHER_APPS_WILDCARD_BUNDLE_ID {
            return true;
        }
        if let Some(found) = self
            .entries
            .read()
            .expect("app cache read lock")
            .get(trimmed)
        {
            self.cache_hits.fetch_add(1, Ordering::SeqCst);
            return found.installed;
        }
        self.resolve(trimmed).installed
    }

    pub fn invalidate(&self, bundle_id: &str) {
        self.entries
            .write()
            .expect("app cache write lock")
            .remove(bundle_id.trim());
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    pub fn clear(&self) {
        self.entries.write().expect("app cache write lock").clear();
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub fn lookup_count(&self) -> usize {
        self.lookup_count.load(Ordering::SeqCst)
    }

    pub fn cache_hits(&self) -> usize {
        self.cache_hits.load(Ordering::SeqCst)
    }
}

fn default_resolve_app_bundle(bundle_id: &str) -> AppBundleInfo {
    if !mi_model::host_router::is_valid_bundle_id(bundle_id) {
        return AppBundleInfo {
            bundle_id: bundle_id.to_string(),
            installed: false,
            app_path: None,
            display_name: None,
            icon_cached: false,
        };
    }

    let mut search_dirs = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
    ];
    if let Some(home) = env::var_os("HOME") {
        search_dirs.push(PathBuf::from(home).join("Applications"));
    }

    for dir in &search_dirs {
        if let Some(info) = scan_applications_dir(dir, bundle_id) {
            return info;
        }
    }

    if let Some(info) = resolve_via_mdfind(bundle_id) {
        return info;
    }

    AppBundleInfo {
        bundle_id: bundle_id.to_string(),
        installed: false,
        app_path: None,
        display_name: None,
        icon_cached: false,
    }
}

fn scan_applications_dir(dir: &Path, target_bundle_id: &str) -> Option<AppBundleInfo> {
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("app") {
            continue;
        }
        if let Some(info) = inspect_app_bundle(&path, target_bundle_id) {
            return Some(info);
        }
    }
    None
}

fn read_plist_xml_bounded(plist_path: &Path) -> Option<String> {
    let file = fs::File::open(plist_path).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.file_type().is_file() || meta.len() > MAX_INFO_PLIST_BYTES {
        return None;
    }
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.take(MAX_INFO_PLIST_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_INFO_PLIST_BYTES {
        return None;
    }
    if bytes.starts_with(b"bplist") {
        let output = Command::new("/usr/bin/plutil")
            .args(["-convert", "xml1", "-o", "-", "--"])
            .arg(plist_path)
            .stdin(Stdio::null())
            .output()
            .ok()?;
        if !output.status.success() || output.stdout.len() as u64 > MAX_INFO_PLIST_BYTES {
            return None;
        }
        return String::from_utf8(output.stdout).ok();
    }
    String::from_utf8(bytes).ok()
}

fn inspect_app_bundle(app_path: &Path, target_bundle_id: &str) -> Option<AppBundleInfo> {
    if app_path.extension().and_then(|s| s.to_str()) != Some("app") {
        return None;
    }
    let plist_path = app_path.join("Contents/Info.plist");
    let raw = read_plist_xml_bounded(&plist_path)?;
    let id = extract_plist_string(&raw, "CFBundleIdentifier")?;
    if id != target_bundle_id {
        return None;
    }
    let display_name = extract_plist_string(&raw, "CFBundleDisplayName")
        .or_else(|| extract_plist_string(&raw, "CFBundleName"))
        .or_else(|| {
            app_path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_string)
        });
    let icon_cached = app_path.join("Contents/Resources").is_dir();
    Some(AppBundleInfo {
        bundle_id: target_bundle_id.to_string(),
        installed: true,
        app_path: Some(app_path.to_path_buf()),
        display_name,
        icon_cached,
    })
}

fn extract_plist_string(plist_xml: &str, key: &str) -> Option<String> {
    let needle = format!("<key>{key}</key>");
    let start = plist_xml.find(&needle)? + needle.len();
    let rest = &plist_xml[start..];
    let str_open = rest.find("<string>")? + "<string>".len();
    let after_open = &rest[str_open..];
    let str_close = after_open.find("</string>")?;
    let val = after_open[..str_close].trim();
    (!val.is_empty()).then(|| val.to_string())
}

fn verify_mdfind_candidates(stdout: &str, bundle_id: &str) -> Option<AppBundleInfo> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take(16)
        .find_map(|line| inspect_app_bundle(Path::new(line), bundle_id))
}

fn resolve_via_mdfind(bundle_id: &str) -> Option<AppBundleInfo> {
    if !mi_model::host_router::is_valid_bundle_id(bundle_id) {
        return None;
    }
    let query = format!("kMDItemCFBundleIdentifier == '{bundle_id}'");
    let output = Command::new("/usr/bin/mdfind")
        .arg(&query)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    verify_mdfind_candidates(&stdout, bundle_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_test_bundle(root: &Path, name: &str, bundle_id: &str, display_name: &str) -> PathBuf {
        let app_dir = root.join(name);
        let contents = app_dir.join("Contents");
        fs::create_dir_all(contents.join("Resources")).unwrap();
        fs::write(
            contents.join("Info.plist"),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
    <key>CFBundleIdentifier</key>
    <string>{bundle_id}</string>
    <key>CFBundleDisplayName</key>
    <string>{display_name}</string>
</dict></plist>"#
            ),
        )
        .unwrap();
        app_dir
    }

    #[test]
    fn inspect_app_bundle_parses_plist_and_caches_invalidation() {
        let temp = TempDir::new().unwrap();
        let app_dir = write_test_bundle(
            temp.path(),
            "Sample.app",
            "com.example.sample",
            "Sample App",
        );

        let inspected = inspect_app_bundle(&app_dir, "com.example.sample").unwrap();
        assert!(inspected.installed);
        assert_eq!(inspected.display_name.as_deref(), Some("Sample App"));
        assert!(inspected.icon_cached);
    }

    #[test]
    fn mdfind_resolution_blocks_injection_and_verifies_bundle_id_plist_within_16_candidates() {
        assert!(resolve_via_mdfind("com.apple.Safari' || kMDItemFSName == '*'").is_none());
        assert!(resolve_via_mdfind("-a.evil").is_none());

        let rejected = default_resolve_app_bundle("com.apple.Safari' || kMDItemFSName == '*'");
        assert!(!rejected.installed);

        let temp = TempDir::new().unwrap();
        let decoy = write_test_bundle(temp.path(), "Decoy.app", "com.evil.decoy", "Decoy");
        let real = write_test_bundle(temp.path(), "Real.app", "com.example.my_app", "Real My App");

        let stdout = format!("{}\n{}\n", decoy.display(), real.display());
        let verified = verify_mdfind_candidates(&stdout, "com.example.my_app").unwrap();
        assert_eq!(verified.app_path.as_deref(), Some(real.as_path()));
        assert_eq!(verified.display_name.as_deref(), Some("Real My App"));

        let mut beyond_limit = String::new();
        for _ in 0..16 {
            beyond_limit.push_str(&format!("{}\n", decoy.display()));
        }
        beyond_limit.push_str(&format!("{}\n", real.display()));
        assert!(verify_mdfind_candidates(&beyond_limit, "com.example.my_app").is_none());
    }

    #[test]
    fn inspect_app_bundle_handles_binary_plists_and_rejects_oversized_or_non_app_targets() {
        let temp = TempDir::new().unwrap();
        let bin_app = write_test_bundle(
            temp.path(),
            "Binary.app",
            "com.example.binary",
            "Binary App",
        );
        let bin_plist = bin_app.join("Contents/Info.plist");
        let status = Command::new("/usr/bin/plutil")
            .args(["-convert", "binary1", "--"])
            .arg(&bin_plist)
            .stdin(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        assert!(fs::read(&bin_plist).unwrap().starts_with(b"bplist00"));

        let inspected = inspect_app_bundle(&bin_app, "com.example.binary").unwrap();
        assert!(inspected.installed);
        assert_eq!(inspected.bundle_id, "com.example.binary");
        assert_eq!(inspected.display_name.as_deref(), Some("Binary App"));
        assert_eq!(inspected.app_path.as_deref(), Some(bin_app.as_path()));
        assert!(inspected.icon_cached);

        let verified =
            verify_mdfind_candidates(&format!("{}\n", bin_app.display()), "com.example.binary")
                .unwrap();
        assert_eq!(verified.app_path.as_deref(), Some(bin_app.as_path()));

        assert!(inspect_app_bundle(&bin_app, "com.example.wrong").is_none());
        assert!(
            verify_mdfind_candidates(&format!("{}\n", bin_app.display()), "com.example.wrong")
                .is_none()
        );

        let non_app = write_test_bundle(
            temp.path(),
            "NotAnApp.bundle",
            "com.example.binary",
            "Not An App",
        );
        assert!(inspect_app_bundle(&non_app, "com.example.binary").is_none());
        assert!(verify_mdfind_candidates(
            &format!("{}\n", non_app.display()),
            "com.example.binary"
        )
        .is_none());

        let oversized_app = write_test_bundle(
            temp.path(),
            "Oversized.app",
            "com.example.oversized",
            "Oversized App",
        );
        let oversized_plist = oversized_app.join("Contents/Info.plist");
        let padding = " ".repeat((MAX_INFO_PLIST_BYTES as usize) + 64);
        fs::write(
            &oversized_plist,
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
    <key>CFBundleIdentifier</key>
    <string>com.example.oversized</string>
    <key>CFBundleDisplayName</key>
    <string>Oversized App</string>
</dict></plist>
<!-- {padding} -->"#
            ),
        )
        .unwrap();
        assert!(read_plist_xml_bounded(&oversized_plist).is_none());
        assert!(inspect_app_bundle(&oversized_app, "com.example.oversized").is_none());
    }

    #[test]
    fn generation_tracks_invalidate_and_clear_without_bumping_on_reads() {
        let cache = AppBundleCache::with_resolver(Arc::new(|bundle_id| AppBundleInfo {
            bundle_id: bundle_id.to_string(),
            installed: true,
            app_path: None,
            display_name: None,
            icon_cached: false,
        }));
        assert_eq!(cache.generation(), 1);
        assert!(cache.is_installed("com.example.alpha"));
        assert!(cache.is_installed("com.example.alpha"));
        assert_eq!(cache.generation(), 1);

        cache.invalidate("com.example.alpha");
        assert_eq!(cache.generation(), 2);

        cache.clear();
        assert_eq!(cache.generation(), 3);
    }
}
