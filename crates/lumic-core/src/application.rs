use crate::application_manifest::ResolvedApplicationManifest;
use crate::{LumicError, Result};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplicationServiceReference {
    pub service_id: String,
    pub role: String,
    /// The catalog service type resolved when the binding was created.
    #[serde(default)]
    pub service_type: Option<String>,
    pub database: Option<String>,
    pub user: Option<String>,
    pub secret_reference: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodePackageManager {
    Npm,
    Pnpm,
    Yarn,
}

impl NodePackageManager {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Yarn => "yarn",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplicationRuntimeIntent {
    pub version: Option<String>,
    #[serde(default)]
    pub components: Vec<String>,
    pub package_manager: Option<NodePackageManager>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationRuntime {
    Static,
    Php,
    Node,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryConfig {
    pub url: String,
    pub branch: String,
    pub credential_reference: Option<String>,
    #[serde(default)]
    pub deployment: DeploymentWorkflow,
    /// The validated repository-owned contract last applied to this application.
    #[serde(default)]
    pub contract: Option<ResolvedApplicationManifest>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentWorkflow {
    #[serde(default)]
    pub pre_deploy: Vec<Vec<String>>,
    pub build: Option<Vec<String>>,
    pub migrate: Option<Vec<String>>,
    #[serde(default)]
    pub post_deploy: Vec<Vec<String>>,
    pub node_handoff: Option<NodeHandoff>,
}

impl DeploymentWorkflow {
    pub fn validate(&self) -> Result<()> {
        for command in self
            .pre_deploy
            .iter()
            .chain(self.build.iter())
            .chain(self.migrate.iter())
            .chain(self.post_deploy.iter())
        {
            validate_command(command)?;
        }
        if let Some(handoff) = &self.node_handoff {
            handoff.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeHandoff {
    pub command: Vec<String>,
    pub primary_port: u16,
    pub secondary_port: u16,
    #[serde(default = "default_drain_seconds")]
    pub drain_seconds: u64,
}

impl NodeHandoff {
    pub fn validate(&self) -> Result<()> {
        validate_command(&self.command)?;
        if self.primary_port == 0
            || self.secondary_port == 0
            || self.primary_port == self.secondary_port
            || self.drain_seconds > 300
        {
            return Err(LumicError::InvalidInput {
                field: "node_handoff".into(),
                message:
                    "requires two distinct non-zero ports and a drain time of at most 300 seconds"
                        .into(),
            });
        }
        Ok(())
    }
}

const fn default_drain_seconds() -> u64 {
    10
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthCheck {
    #[serde(default)]
    pub enabled: bool,
    pub path: String,
    #[serde(default = "default_health_port")]
    pub port: u16,
    pub expected_status_min: u16,
    pub expected_status_max: u16,
    pub timeout_seconds: u64,
}

impl Default for HealthCheck {
    fn default() -> Self {
        Self {
            enabled: false,
            path: "/".into(),
            port: default_health_port(),
            expected_status_min: 200,
            expected_status_max: 399,
            timeout_seconds: 10,
        }
    }
}

const fn default_health_port() -> u16 {
    80
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationProcessKind {
    Worker,
    Schedule,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessRestartPolicy {
    No,
    #[default]
    OnFailure,
    Always,
}

impl ProcessRestartPolicy {
    pub const fn systemd_value(self) -> &'static str {
        match self {
            Self::No => "no",
            Self::OnFailure => "on-failure",
            Self::Always => "always",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessHealthCheck {
    pub command: Vec<String>,
    #[serde(default = "default_process_health_interval")]
    pub interval_seconds: u64,
    #[serde(default = "default_process_health_timeout")]
    pub timeout_seconds: u64,
}

impl ProcessHealthCheck {
    pub fn validate(&self) -> Result<()> {
        validate_command(&self.command)?;
        if self.interval_seconds == 0
            || self.interval_seconds > 86_400
            || self.timeout_seconds == 0
            || self.timeout_seconds > self.interval_seconds
        {
            return Err(LumicError::InvalidInput {
                field: "process.health".into(),
                message: "requires a 1-86400 second interval and a non-zero timeout no longer than the interval".into(),
            });
        }
        Ok(())
    }
}

const fn default_process_health_interval() -> u64 {
    30
}

const fn default_process_health_timeout() -> u64 {
    5
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScheduleTiming {
    Calendar { expression: String },
    Interval { seconds: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissedRunPolicy {
    RunImmediately,
    Skip,
}

/// Backend-neutral timing and missed-run intent for an application job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApplicationSchedule {
    pub timing: ScheduleTiming,
    pub missed_run_policy: MissedRunPolicy,
    pub jitter_seconds: u64,
}

impl ApplicationSchedule {
    pub fn calendar(expression: impl Into<String>) -> Self {
        Self {
            timing: ScheduleTiming::Calendar {
                expression: expression.into(),
            },
            missed_run_policy: MissedRunPolicy::RunImmediately,
            jitter_seconds: 0,
        }
    }

    pub fn interval(seconds: u64) -> Self {
        Self {
            timing: ScheduleTiming::Interval { seconds },
            missed_run_policy: MissedRunPolicy::RunImmediately,
            jitter_seconds: 0,
        }
    }

    pub fn validate(&self) -> Result<()> {
        match &self.timing {
            ScheduleTiming::Calendar { expression }
                if expression.is_empty()
                    || expression.len() > 128
                    || expression.contains(['\n', '\r', '\0']) =>
            {
                Err(invalid_schedule("calendar expression is invalid"))
            }
            ScheduleTiming::Interval { seconds } if *seconds == 0 => {
                Err(invalid_schedule("interval must be greater than zero"))
            }
            _ if self.jitter_seconds > 86_400 => {
                Err(invalid_schedule("jitter must not exceed one day"))
            }
            _ => Ok(()),
        }
    }
}

impl<'de> Deserialize<'de> for ApplicationSchedule {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Representation {
            Legacy(String),
            Structured {
                timing: ScheduleTiming,
                missed_run_policy: MissedRunPolicy,
                #[serde(default)]
                jitter_seconds: u64,
            },
        }

        Ok(match Representation::deserialize(deserializer)? {
            Representation::Legacy(expression) => Self::calendar(expression),
            Representation::Structured {
                timing,
                missed_run_policy,
                jitter_seconds,
            } => Self {
                timing,
                missed_run_policy,
                jitter_seconds,
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplicationProcess {
    pub name: String,
    pub kind: ApplicationProcessKind,
    pub command: Vec<String>,
    pub schedule: Option<ApplicationSchedule>,
    pub enabled: bool,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    pub working_directory: Option<String>,
    #[serde(default)]
    pub restart_policy: ProcessRestartPolicy,
    pub health_check: Option<ProcessHealthCheck>,
}

impl ApplicationProcess {
    pub fn validate(&self) -> Result<()> {
        validate_slug("process", &self.name)?;
        validate_command(&self.command)?;
        validate_process_environment(&self.environment)?;
        if self.working_directory.as_deref().is_some_and(|directory| {
            directory.is_empty()
                || directory.len() > 4096
                || directory.contains(['\n', '\r', '\0'])
                || directory.split('/').any(|part| part == "..")
        }) {
            return Err(LumicError::InvalidInput {
                field: "process.working_directory".into(),
                message: "must be a bounded normalized path without parent traversal".into(),
            });
        }
        if let Some(health) = &self.health_check {
            health.validate()?;
        }
        match self.kind {
            ApplicationProcessKind::Worker if self.schedule.is_some() => {
                Err(invalid_schedule("worker processes cannot have a schedule"))
            }
            ApplicationProcessKind::Schedule => self
                .schedule
                .as_ref()
                .ok_or_else(|| invalid_schedule("scheduled processes require timing"))?
                .validate(),
            ApplicationProcessKind::Worker => Ok(()),
        }
    }
}

fn invalid_schedule(message: &str) -> LumicError {
    LumicError::InvalidInput {
        field: "schedule".into(),
        message: message.into(),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TlsState {
    pub enabled: bool,
    pub certificate_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Application {
    pub id: String,
    pub name: String,
    pub domain: String,
    pub www_alias: bool,
    pub root: String,
    pub runtime: ApplicationRuntime,
    #[serde(default)]
    pub runtime_intent: ApplicationRuntimeIntent,
    pub repository: Option<RepositoryConfig>,
    pub environment_references: BTreeMap<String, String>,
    #[serde(default)]
    pub service_references: Vec<ApplicationServiceReference>,
    pub health_check: HealthCheck,
    #[serde(default)]
    pub processes: Vec<ApplicationProcess>,
    #[serde(default)]
    pub web_configured: bool,
    #[serde(default)]
    pub tls: TlsState,
    pub release_retention: usize,
    pub health_status: String,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

fn validate_process_environment(environment: &BTreeMap<String, String>) -> Result<()> {
    let invalid = environment.iter().any(|(key, value)| {
        key.is_empty()
            || key.len() > 128
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
            || key.as_bytes()[0].is_ascii_digit()
            || value.len() > 16 * 1024
            || value.contains(['\0', '\n', '\r'])
    });
    if invalid {
        Err(LumicError::InvalidInput {
            field: "process.environment".into(),
            message: "keys must be uppercase environment names and values must be bounded single-line text".into(),
        })
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentStatus {
    Started,
    Cancelling,
    Cancelled,
    Completed,
    Failed,
    RolledBack,
    FailedRolledBack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentPhaseStatus {
    Running,
    Completed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentPhase {
    pub name: String,
    pub status: DeploymentPhaseStatus,
    pub message: String,
    #[serde(default)]
    pub started_at_unix_ms: u128,
    pub finished_at_unix_ms: Option<u128>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitMetadata {
    pub id: String,
    pub author_name: String,
    pub author_email: String,
    pub subject: String,
    pub authored_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentLogStream {
    System,
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentLogEntry {
    pub sequence: u64,
    pub timestamp_unix_ms: u128,
    pub phase: String,
    pub stream: DeploymentLogStream,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deployment {
    pub id: String,
    pub application_id: String,
    pub release_path: String,
    pub commit: String,
    pub commit_metadata: Option<CommitMetadata>,
    pub status: DeploymentStatus,
    pub healthy: bool,
    pub message: String,
    pub previous_release: Option<String>,
    #[serde(default)]
    pub phases: Vec<DeploymentPhase>,
    #[serde(default)]
    pub automatic_rollback: bool,
    pub retry_of: Option<String>,
    pub node_port: Option<u16>,
    pub process_unit: Option<String>,
    pub started_at_unix_ms: u128,
    pub finished_at_unix_ms: Option<u128>,
}

pub fn validate_slug(field: &str, value: &str) -> Result<()> {
    let valid = !value.is_empty()
        && value.len() <= 63
        && value.as_bytes()[0].is_ascii_lowercase()
        && value.as_bytes()[value.len() - 1].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    if valid {
        Ok(())
    } else {
        Err(LumicError::InvalidInput {
            field: field.into(),
            message: "must be a lowercase DNS-style slug".into(),
        })
    }
}

pub fn validate_domain(value: &str) -> Result<()> {
    if value.len() > 253
        || !value.contains('.')
        || value
            .split('.')
            .any(|label| validate_slug("domain", label).is_err())
    {
        return Err(LumicError::InvalidInput {
            field: "domain".into(),
            message: "must be a valid lowercase fully-qualified domain name".into(),
        });
    }
    Ok(())
}

pub fn validate_branch(value: &str) -> Result<()> {
    let invalid = value.is_empty()
        || value.len() > 255
        || value.starts_with('-')
        || value.starts_with('/')
        || value.ends_with('/')
        || value.contains("..")
        || value.contains("@{")
        || value.bytes().any(|byte| {
            byte.is_ascii_control()
                || byte.is_ascii_whitespace()
                || matches!(byte, b'~' | b'^' | b':' | b'?' | b'*' | b'[' | b'\\')
        });
    if invalid {
        Err(LumicError::InvalidInput {
            field: "branch".into(),
            message: "is not a safe Git branch name".into(),
        })
    } else {
        Ok(())
    }
}

pub fn validate_repository_url(value: &str) -> Result<()> {
    let supported = value.starts_with("https://")
        || value.starts_with("ssh://")
        || value.starts_with("git@")
        || value.starts_with("file://");
    let has_embedded_https_secret = value
        .strip_prefix("https://")
        .and_then(|rest| rest.split('/').next())
        .is_some_and(|authority| authority.contains('@'));
    if !supported || value.contains('\n') || has_embedded_https_secret {
        Err(LumicError::InvalidInput {
            field: "repository".into(),
            message:
                "must use HTTPS, SSH, Git scp syntax, or file URL without embedded credentials"
                    .into(),
        })
    } else {
        Ok(())
    }
}

pub fn validate_command(command: &[String]) -> Result<()> {
    if command.is_empty()
        || command.iter().any(|part| {
            part.is_empty()
                || part.contains('\0')
                || part.contains('\n')
                || part.contains('\r')
                || part.len() > 4096
        })
    {
        Err(LumicError::InvalidInput {
            field: "command".into(),
            message: "must be a non-empty argv vector without control characters".into(),
        })
    } else {
        Ok(())
    }
}

pub fn unix_time_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_path_traversal_and_embedded_repository_credentials() {
        assert!(validate_slug("app", "../app").is_err());
        assert!(validate_domain("Example.com").is_err());
        assert!(validate_branch("--upload-pack=id").is_err());
        assert!(validate_repository_url("https://token@example.com/repo.git").is_err());
        assert!(validate_repository_url("https://example.com/repo.git").is_ok());
        assert!(validate_command(&["php".into(), "artisan".into()]).is_ok());
        assert!(validate_command(&["sh\n-c".into()]).is_err());
    }

    #[test]
    fn schedule_deserialization_migrates_legacy_calendar_strings() {
        let schedule: ApplicationSchedule = serde_json::from_str("\"daily\"").unwrap();
        assert_eq!(schedule, ApplicationSchedule::calendar("daily"));
        assert!(
            serde_json::to_string(&schedule)
                .unwrap()
                .contains("missed_run_policy")
        );
    }

    #[test]
    fn interval_schedules_reject_zero_seconds() {
        assert!(ApplicationSchedule::interval(0).validate().is_err());
        assert!(ApplicationSchedule::interval(60).validate().is_ok());
    }

    #[test]
    fn validates_typed_deployment_workflow_and_node_handoff() {
        let workflow = DeploymentWorkflow {
            migrate: Some(vec!["php".into(), "artisan".into(), "migrate".into()]),
            node_handoff: Some(NodeHandoff {
                command: vec!["node".into(), "server.js".into()],
                primary_port: 3100,
                secondary_port: 3101,
                drain_seconds: 15,
            }),
            ..DeploymentWorkflow::default()
        };
        assert!(workflow.validate().is_ok());
        let mut invalid = workflow;
        invalid.node_handoff.as_mut().unwrap().secondary_port = 3100;
        assert!(invalid.validate().is_err());
    }
}
