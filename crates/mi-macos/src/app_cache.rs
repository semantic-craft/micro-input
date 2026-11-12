use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

pub const OTHER_APPS_WILDCARD_BUNDLE_ID: &str = "*";

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
    }

    pub fn clear(&self) {
        self.entries.write().expect("app cache write lock").clear();
    }

    pub fn lookup_count(&self) -> usize {
        self.lookup_count.load(Ordering::SeqCst)
    }

    pub fn cache_hits(&self) -> usize {
        self.cache_hits.load(Ordering::SeqCst)
    }
}

fn default_resolve_app_bundle(bundle_id: &str) -> AppBundleInfo {
    if bundle_id.is_empty() {
        return AppBundleInfo {
            bundle_id: String::new(),
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

fn inspect_app_bundle(app_path: &Path, target_bundle_id: &str) -> Option<AppBundleInfo> {
    let plist_path = app_path.join("Contents/Info.plist");
    let raw = fs::read_to_string(&plist_path).ok()?;
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

fn resolve_via_mdfind(bundle_id: &str) -> Option<AppBundleInfo> {
    let query = format!("kMDItemCFBundleIdentifier == '{bundle_id}'");
    let output = Command::new("/usr/bin/mdfind").arg(&query).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().map(str::trim).find(|l| !l.is_empty())?;
    let app_path = PathBuf::from(first_line);
    if !app_path.exists() {
        return None;
    }
    let display_name = app_path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_string);
    let icon_cached = app_path.join("Contents/Resources").is_dir();
    Some(AppBundleInfo {
        bundle_id: bundle_id.to_string(),
        installed: true,
        app_path: Some(app_path),
        display_name,
        icon_cached,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn inspect_app_bundle_parses_plist_and_caches_invalidation() {
        let temp = TempDir::new().unwrap();
        let app_dir = temp.path().join("Sample.app");
        let contents = app_dir.join("Contents");
        fs::create_dir_all(contents.join("Resources")).unwrap();
        fs::write(
            contents.join("Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
    <key>CFBundleIdentifier</key>
    <string>com.example.sample</string>
    <key>CFBundleDisplayName</key>
    <string>Sample App</string>
</dict></plist>"#,
        )
        .unwrap();

        let inspected = inspect_app_bundle(&app_dir, "com.example.sample").unwrap();
        assert!(inspected.installed);
        assert_eq!(inspected.display_name.as_deref(), Some("Sample App"));
        assert!(inspected.icon_cached);
    }
}
