use std::{
    env, fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntheticScenario9 {
    HostExecution,
    DshNavigation,
    AntigravityStatus,
    ConfigWrite,
    LightingControl,
    DiagnosticExport,
}

impl SyntheticScenario9 {
    pub const ALL: [Self; 6] = [
        Self::HostExecution,
        Self::DshNavigation,
        Self::AntigravityStatus,
        Self::ConfigWrite,
        Self::LightingControl,
        Self::DiagnosticExport,
    ];

    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim() {
            "host-execution" => Ok(Self::HostExecution),
            "dsh-navigation" => Ok(Self::DshNavigation),
            "antigravity-status" => Ok(Self::AntigravityStatus),
            "config-write" => Ok(Self::ConfigWrite),
            "lighting-control" => Ok(Self::LightingControl),
            "diagnostic-export" => Ok(Self::DiagnosticExport),
            other => Err(format!("unknown synthetic scenario: {other}")),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::HostExecution => "host-execution",
            Self::DshNavigation => "dsh-navigation",
            Self::AntigravityStatus => "antigravity-status",
            Self::ConfigWrite => "config-write",
            Self::LightingControl => "lighting-control",
            Self::DiagnosticExport => "diagnostic-export",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QaLaunchEnv {
    pub document_dir: PathBuf,
    pub audit_log_path: Option<PathBuf>,
    pub run_dir: Option<PathBuf>,
    pub diagnostic_evidence_path: Option<PathBuf>,
    pub scenario: Option<SyntheticScenario9>,
}

fn non_empty_path(raw: Option<&str>) -> Option<PathBuf> {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn anchor_path(path: &Path) -> Option<PathBuf> {
    if path.is_relative() {
        Some(env::current_dir().ok()?.join(path))
    } else {
        Some(path.to_path_buf())
    }
}

fn canonicalize_with_tail(path: &Path) -> PathBuf {
    resolve_with_tail(path, 0)
}

fn resolve_with_tail(path: &Path, depth: u8) -> PathBuf {
    let components: Vec<_> = path.components().collect();
    for split in (1..=components.len()).rev() {
        let prefix: PathBuf = components[..split].iter().collect();
        if let Ok(canon) = fs::canonicalize(&prefix) {
            let mut resolved = canon;
            for component in &components[split..] {
                resolved.push(component.as_os_str());
            }
            return normalize_lexical(&resolved);
        }
        if depth < 16
            && fs::symlink_metadata(&prefix)
                .map(|meta| meta.file_type().is_symlink())
                .unwrap_or(false)
        {
            if let Ok(target) = fs::read_link(&prefix) {
                let mut expanded = if target.is_absolute() {
                    target
                } else {
                    prefix.parent().unwrap_or(Path::new("/")).join(target)
                };
                for component in &components[split..] {
                    expanded.push(component.as_os_str());
                }
                return resolve_with_tail(&expanded, depth + 1);
            }
        }
    }
    normalize_lexical(path)
}

fn components_equal(a: std::path::Component<'_>, b: std::path::Component<'_>) -> bool {
    match (a, b) {
        (std::path::Component::Normal(sa), std::path::Component::Normal(sb)) => {
            #[cfg(target_os = "macos")]
            {
                sa.to_string_lossy().to_lowercase() == sb.to_string_lossy().to_lowercase()
            }
            #[cfg(not(target_os = "macos"))]
            {
                sa == sb
            }
        }
        _ => a == b,
    }
}

fn path_starts_with_components(candidate: &Path, base: &Path) -> bool {
    let mut cand_iter = candidate.components();
    for base_comp in base.components() {
        let Some(cand_comp) = cand_iter.next() else {
            return false;
        };
        if !components_equal(cand_comp, base_comp) {
            return false;
        }
    }
    true
}

fn is_inside_production_document_dir(path: &Path) -> bool {
    let Some(home) = env::var_os("HOME").filter(|h| !h.is_empty()) else {
        return false;
    };
    let prod_raw = PathBuf::from(home).join("Library/Application Support/MicroInput");
    let Some(prod_anchored) = anchor_path(&prod_raw) else {
        return false;
    };
    let Some(candidate_anchored) = anchor_path(path) else {
        return false;
    };
    let prod_lex = normalize_lexical(&prod_anchored);
    let candidate_lex = normalize_lexical(&candidate_anchored);
    let prod_canon = canonicalize_with_tail(&prod_anchored);
    let candidate_canon = canonicalize_with_tail(&candidate_anchored);
    path_starts_with_components(&candidate_lex, &prod_lex)
        || path_starts_with_components(&candidate_canon, &prod_canon)
        || path_starts_with_components(&candidate_canon, &prod_lex)
        || path_starts_with_components(&candidate_lex, &prod_canon)
}

impl QaLaunchEnv {
    pub fn from_env() -> Result<Self, String> {
        let doc_dir = env::var("MI_DOCUMENT_DIR").ok();
        let audit_log = env::var("MI_HOST_AUDIT_LOG").ok();
        let qa_run_dir = env::var("MI_QA_RUN_DIR").ok();
        let diagnostic_path = env::var("MI_DIAGNOSTIC_EVIDENCE_PATH").ok();
        let scenario = env::var("MI_SYNTHETIC_SCENARIO").ok();
        Self::from_pairs(
            doc_dir.as_deref(),
            audit_log.as_deref(),
            qa_run_dir.as_deref(),
            diagnostic_path.as_deref(),
            scenario.as_deref(),
        )
    }

    pub fn from_pairs(
        doc_dir: Option<&str>,
        audit_log: Option<&str>,
        qa_run_dir: Option<&str>,
        diagnostic_path: Option<&str>,
        scenario_raw: Option<&str>,
    ) -> Result<Self, String> {
        let scenario = match scenario_raw.map(str::trim).filter(|s| !s.is_empty()) {
            Some(raw) => Some(SyntheticScenario9::parse(raw)?),
            None => None,
        };
        let document_dir = match scenario {
            Some(_) => {
                let explicit = non_empty_path(doc_dir).ok_or_else(|| {
                    "synthetic QA scenario requires non-empty MI_DOCUMENT_DIR".to_string()
                })?;
                if is_inside_production_document_dir(&explicit) {
                    return Err(
                        "synthetic QA scenario cannot target production document directory"
                            .to_string(),
                    );
                }
                explicit
            }
            None => non_empty_path(doc_dir)
                .or_else(|| {
                    env::var_os("HOME").map(|home| {
                        PathBuf::from(home).join("Library/Application Support/MicroInput")
                    })
                })
                .unwrap_or_else(|| PathBuf::from(".micro-input")),
        };
        let audit_log_path = non_empty_path(audit_log);
        let run_dir = non_empty_path(qa_run_dir)
            .or_else(|| {
                audit_log_path
                    .as_deref()
                    .and_then(Path::parent)
                    .and_then(Path::parent)
                    .map(Path::to_path_buf)
            })
            .or_else(|| document_dir.parent().map(Path::to_path_buf));
        let diagnostic_evidence_path = non_empty_path(diagnostic_path).or_else(|| {
            run_dir
                .as_ref()
                .map(|d| d.join("evidence/diagnostic-report.json"))
        });
        Ok(Self {
            document_dir,
            audit_log_path,
            run_dir,
            diagnostic_evidence_path,
            scenario,
        })
    }

    pub fn resolved_run_dir(&self) -> Result<PathBuf, String> {
        self.run_dir
            .clone()
            .ok_or_else(|| "missing QA run directory".to_string())
    }

    pub fn resolved_audit_log_path(&self) -> Result<PathBuf, String> {
        if let Some(path) = &self.audit_log_path {
            return Ok(path.clone());
        }
        Ok(self.resolved_run_dir()?.join("evidence/audit.jsonl"))
    }

    pub fn resolved_diagnostic_report_path(&self) -> Result<PathBuf, String> {
        if let Some(path) = &self.diagnostic_evidence_path {
            return Ok(path.clone());
        }
        Ok(self
            .resolved_run_dir()?
            .join("evidence/diagnostic-report.json"))
    }
}

pub fn run_synthetic_scenario(
    env: &QaLaunchEnv,
    scenario: SyntheticScenario9,
) -> Result<(), String> {
    match scenario {
        SyntheticScenario9::HostExecution => {
            let run_dir = env.resolved_run_dir()?;
            let summary = mi_macos::run_host_execution_scenario(&run_dir)?;
            if summary.status != "PASS" {
                return Err(format!(
                    "host-execution scenario returned status {}",
                    summary.status
                ));
            }
            Ok(())
        }
        SyntheticScenario9::DshNavigation => {
            let audit_path = env.resolved_audit_log_path()?;
            let report = mi_bridge::run_dsh_navigation_scenario(Some(&audit_path))?;
            if report.status != "PASS" {
                return Err(format!(
                    "dsh-navigation scenario returned status {}",
                    report.status
                ));
            }
            Ok(())
        }
        SyntheticScenario9::AntigravityStatus => {
            let audit_path = env.resolved_audit_log_path()?;
            let report = mi_bridge::run_antigravity_status_scenario(Some(&audit_path))?;
            if report.status != "PASS" {
                return Err(format!(
                    "antigravity-status scenario returned status {}",
                    report.status
                ));
            }
            Ok(())
        }
        SyntheticScenario9::ConfigWrite => {
            let run_dir = env.resolved_run_dir()?;
            let summary = mi_macos::run_config_write_scenario(&run_dir)?;
            let _ = fs::remove_dir_all(run_dir.join("workspace"));
            if summary.status != "PASS" {
                return Err(format!(
                    "config-write scenario returned status {}",
                    summary.status
                ));
            }
            Ok(())
        }
        SyntheticScenario9::LightingControl => {
            let run_dir = env.resolved_run_dir()?;
            let summary = mi_macos::run_lighting_control_scenario(&run_dir)?;
            let _ = fs::remove_dir_all(run_dir.join("workspace"));
            if summary.status != "PASS" {
                return Err(format!(
                    "lighting-control scenario returned status {}",
                    summary.status
                ));
            }
            Ok(())
        }
        SyntheticScenario9::DiagnosticExport => {
            let run_dir = env.resolved_run_dir()?;
            let report_path = env.resolved_diagnostic_report_path()?;
            let summary = mi_macos::run_diagnostic_export_scenario(&run_dir, &report_path)?;
            if summary.status != "PASS" {
                return Err(format!(
                    "diagnostic-export scenario returned status {}",
                    summary.status
                ));
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qa_launch_env_requires_non_empty_isolated_document_dir_when_scenario_set() {
        assert!(
            QaLaunchEnv::from_pairs(None, None, None, None, Some("host-execution")).is_err(),
            "synthetic scenario without MI_DOCUMENT_DIR must fail instead of falling back to production"
        );
        assert!(
            QaLaunchEnv::from_pairs(Some("   "), None, None, None, Some("host-execution")).is_err(),
            "synthetic scenario with whitespace MI_DOCUMENT_DIR must fail"
        );
        if let Some(home) = env::var_os("HOME") {
            let prod_doc = PathBuf::from(home).join("Library/Application Support/MicroInput");
            assert!(
                QaLaunchEnv::from_pairs(
                    prod_doc.to_str(),
                    None,
                    None,
                    None,
                    Some("host-execution")
                )
                .is_err(),
                "synthetic scenario targeting production document dir must fail"
            );
            let prod_sub = prod_doc.join("nested");
            assert!(
                QaLaunchEnv::from_pairs(
                    prod_sub.to_str(),
                    None,
                    None,
                    None,
                    Some("host-execution")
                )
                .is_err(),
                "synthetic scenario inside production document dir must fail"
            );
        }
        assert!(
            QaLaunchEnv::from_pairs(None, None, None, None, None).is_ok(),
            "non-QA launch without scenario must remain permitted"
        );
    }

    #[test]
    fn qa_launch_env_blocks_case_relative_and_symlink_production_bypasses() {
        if let Some(raw_home) = env::var_os("HOME").filter(|h| !h.is_empty()) {
            #[cfg(target_os = "macos")]
            {
                let mixed_case =
                    PathBuf::from(&raw_home).join("library/application support/microinput/nested");
                assert!(
                    QaLaunchEnv::from_pairs(
                        mixed_case.to_str(),
                        None,
                        None,
                        None,
                        Some("host-execution")
                    )
                    .is_err(),
                    "case-folded production path on macOS must be rejected"
                );
            }
        }

        if env::var_os("MI_QA_GUARD_CHILD").is_some() {
            let res = QaLaunchEnv::from_env();
            assert!(
                res.is_err(),
                "child probe expected production guard rejection, got {res:?}"
            );
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        let fake_home = temp.path().join("home");
        let app_support = fake_home.join("Library/Application Support");
        let prod_dir = app_support.join("MicroInput");
        fs::create_dir_all(&app_support).unwrap();

        let exe = env::current_exe().unwrap();
        let run_probe = |cwd: &Path, doc_dir: &Path| {
            let out = std::process::Command::new(&exe)
                .args([
                    "--exact",
                    "qa_runner::tests::qa_launch_env_blocks_case_relative_and_symlink_production_bypasses",
                ])
                .current_dir(cwd)
                .env("MI_QA_GUARD_CHILD", "1")
                .env("HOME", &fake_home)
                .env("MI_DOCUMENT_DIR", doc_dir)
                .env("MI_SYNTHETIC_SCENARIO", "host-execution")
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "child probe failed for cwd={} doc_dir={}: stderr={}",
                cwd.display(),
                doc_dir.display(),
                String::from_utf8_lossy(&out.stderr)
            );
        };

        run_probe(
            &fake_home,
            Path::new("Library/Application Support/MicroInput/sub"),
        );

        let link_to_prod = temp.path().join("alias_prod");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&prod_dir, &link_to_prod).unwrap();
        #[cfg(unix)]
        run_probe(temp.path(), &link_to_prod.join("documents"));

        fs::create_dir_all(&prod_dir).unwrap();
        #[cfg(unix)]
        run_probe(temp.path(), &link_to_prod.join("nonexistent-child"));
    }
}
