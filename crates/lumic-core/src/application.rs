use crate::{LumicError, Result};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplicationServiceReference {
    pub service_id: String,
    pub role: String,
    pub database: Option<String>,
    pub user: Option<String>,
    pub secret_reference: Option<String>,
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
}

impl ApplicationProcess {
    pub fn validate(&self) -> Result<()> {
        validate_slug("process", &self.name)?;
        validate_command(&self.command)?;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentStatus {
    Started,
    Completed,
    Failed,
    RolledBack,
    FailedRolledBack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentPhaseStatus {
    Completed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentPhase {
    pub name: String,
    pub status: DeploymentPhaseStatus,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deployment {
    pub id: String,
    pub application_id: String,
    pub release_path: String,
    pub commit: String,
    pub status: DeploymentStatus,
    pub healthy: bool,
    pub message: String,
    pub previous_release: Option<String>,
    #[serde(default)]
    pub phases: Vec<DeploymentPhase>,
    #[serde(default)]
    pub automatic_rollback: bool,
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
}
