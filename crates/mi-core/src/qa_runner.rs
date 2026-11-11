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
        let document_dir = non_empty_path(doc_dir)
            .or_else(|| {
                env::var_os("HOME")
                    .map(|home| PathBuf::from(home).join("Library/Application Support/MicroInput"))
            })
            .unwrap_or_else(|| PathBuf::from(".micro-input"));
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
        let scenario = match scenario_raw.map(str::trim).filter(|s| !s.is_empty()) {
            Some(raw) => Some(SyntheticScenario9::parse(raw)?),
            None => None,
        };
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
