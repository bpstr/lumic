use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

pub mod application;
pub mod events;
pub mod managed_service;
pub mod package;
pub mod recipe;
pub mod server;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatingSystem {
    Linux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Distribution {
    Debian,
    Ubuntu,
}

impl Distribution {
    pub const fn id(self) -> &'static str {
        match self {
            Self::Debian => "debian",
            Self::Ubuntu => "ubuntu",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Architecture {
    X86_64,
    Aarch64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistributionFacts {
    pub distribution: Distribution,
    pub version_id: String,
    pub version_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryFacts {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_free_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskFacts {
    pub mount_point: String,
    pub filesystem: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostFacts {
    pub operating_system: OperatingSystem,
    pub distribution: DistributionFacts,
    pub architecture: Architecture,
    pub hostname: String,
    pub kernel_release: String,
    pub cpu_count: usize,
    pub memory: MemoryFacts,
    pub disks: Vec<DiskFacts>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoadFacts {
    pub one_minute: f64,
    pub five_minutes: f64,
    pub fifteen_minutes: f64,
    pub running_processes: u64,
    pub total_processes: u64,
    pub uptime_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessFacts {
    pub pid: u32,
    pub name: String,
    pub state: String,
    pub resident_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticFinding {
    pub severity: String,
    pub summary: String,
    pub evidence: String,
    pub recommendation: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticReport {
    pub host: HostFacts,
    pub load: LoadFacts,
    pub top_processes: Vec<ProcessFacts>,
    pub failed_services: Vec<String>,
    #[serde(default)]
    pub listeners: Vec<server::ListeningPort>,
    #[serde(default)]
    pub mounts: Vec<server::MountStatus>,
    #[serde(default)]
    pub timers: Vec<server::TimerStatus>,
    #[serde(default)]
    pub updates: Vec<server::UpdateStatus>,
    pub findings: Vec<DiagnosticFinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Capability(pub String);

impl Capability {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl std::fmt::Display for Capability {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationInterface {
    Cli,
    Daemon,
    Http,
    Ui,
    Mcp,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationContext {
    pub actor: String,
    pub interface: OperationInterface,
    pub correlation_id: String,
    pub dry_run: bool,
    pub approved: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    Succeeded,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationResult<T> {
    pub status: OperationStatus,
    pub value: Option<T>,
    pub changed: bool,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Risk {
    pub level: RiskLevel,
    pub summary: String,
    pub mitigation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub capability: Capability,
    pub summary: String,
    pub before: Option<String>,
    pub after: Option<String>,
    pub reversible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub id: String,
    pub summary: String,
    pub changes: Vec<Change>,
    pub risks: Vec<Risk>,
    pub preconditions: Vec<String>,
    pub validation: Vec<String>,
    pub recovery: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidInput,
    UnsupportedPlatform,
    InspectionFailed,
    ProcessFailed,
    Timeout,
    PolicyDenied,
    Internal,
}

#[derive(Debug, Error)]
pub enum LumicError {
    #[error("invalid input for {field}: {message}")]
    InvalidInput { field: String, message: String },
    #[error("unsupported platform: {platform}")]
    UnsupportedPlatform { platform: String },
    #[error("host inspection failed for {fact}: {message}")]
    Inspection { fact: String, message: String },
    #[error("process '{executable}' failed: {message}")]
    Process { executable: String, message: String },
    #[error("process '{executable}' timed out after {timeout_ms} ms")]
    Timeout { executable: String, timeout_ms: u64 },
    #[error("policy denied capability '{capability}'")]
    PolicyDenied { capability: Capability },
    #[error("internal error: {message}")]
    Internal { message: String },
}

impl LumicError {
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidInput { .. } => ErrorCode::InvalidInput,
            Self::UnsupportedPlatform { .. } => ErrorCode::UnsupportedPlatform,
            Self::Inspection { .. } => ErrorCode::InspectionFailed,
            Self::Process { .. } => ErrorCode::ProcessFailed,
            Self::Timeout { .. } => ErrorCode::Timeout,
            Self::PolicyDenied { .. } => ErrorCode::PolicyDenied,
            Self::Internal { .. } => ErrorCode::Internal,
        }
    }

    pub fn details(&self) -> BTreeMap<String, String> {
        let mut details = BTreeMap::new();
        match self {
            Self::InvalidInput { field, .. } => {
                details.insert("field".into(), field.clone());
            }
            Self::UnsupportedPlatform { platform } => {
                details.insert("platform".into(), platform.clone());
            }
            Self::Inspection { fact, .. } => {
                details.insert("fact".into(), fact.clone());
            }
            Self::Process { executable, .. } | Self::Timeout { executable, .. } => {
                details.insert("executable".into(), executable.clone());
            }
            Self::PolicyDenied { capability } => {
                details.insert("capability".into(), capability.0.clone());
            }
            Self::Internal { .. } => {}
        }
        details
    }
}

pub type Result<T> = std::result::Result<T, LumicError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_errors_have_stable_codes_and_details() {
        let error = LumicError::InvalidInput {
            field: "package".into(),
            message: "must not be empty".into(),
        };
        assert_eq!(error.code(), ErrorCode::InvalidInput);
        assert_eq!(
            error.details().get("field").map(String::as_str),
            Some("package")
        );
    }

    #[test]
    fn capability_serializes_as_a_stable_string() {
        let capability = Capability::new("server.read");
        assert_eq!(
            serde_json::to_string(&capability).unwrap(),
            "\"server.read\""
        );
    }
}
