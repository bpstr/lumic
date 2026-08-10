//! Typed, journalable reconciliation pipelines.

use crate::{
    LumicError, Result,
    catalog::Configuration,
    resource::{ResourceRef, validate_output_name, validate_resource_id},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryClassification {
    Retryable,
    Reversible,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PipelineAction {
    EnsurePackage {
        package: String,
    },
    EnsureRepository {
        repository_id: String,
    },
    EnsureArtifact {
        artifact_id: String,
    },
    EnsureUser {
        user: String,
        system: bool,
    },
    EnsureGroup {
        group: String,
        system: bool,
    },
    EnsureDirectory {
        path: String,
        mode: u32,
    },
    WriteManagedFile {
        path: String,
        content_sha256: String,
        mode: u32,
    },
    EnsureSymlink {
        path: String,
        target: String,
    },
    ServiceAction {
        unit: String,
        action: ServiceAction,
    },
    ProviderAction {
        provider: String,
        operation: String,
        #[serde(default)]
        parameters: Configuration,
    },
    HealthCheck {
        check: HealthCheck,
    },
    RecordOutput {
        resource: ResourceRef,
        name: String,
        value: Value,
        sensitive: bool,
    },
    CommitState,
}

impl PipelineAction {
    pub fn recovery_classification(&self) -> RecoveryClassification {
        match self {
            Self::EnsurePackage { .. }
            | Self::EnsureRepository { .. }
            | Self::EnsureArtifact { .. }
            | Self::HealthCheck { .. } => RecoveryClassification::Retryable,
            Self::EnsureUser { .. }
            | Self::EnsureGroup { .. }
            | Self::EnsureDirectory { .. }
            | Self::WriteManagedFile { .. }
            | Self::EnsureSymlink { .. }
            | Self::ServiceAction { .. }
            | Self::RecordOutput { .. }
            | Self::CommitState => RecoveryClassification::Reversible,
            Self::ProviderAction { .. } => RecoveryClassification::Manual,
        }
    }

    fn validate(&self) -> Result<()> {
        match self {
            Self::EnsurePackage { package } => validate_token("step.package", package, 128),
            Self::EnsureRepository { repository_id } => {
                validate_resource_id("step.repository_id", repository_id)
            }
            Self::EnsureArtifact { artifact_id } => {
                validate_resource_id("step.artifact_id", artifact_id)
            }
            Self::EnsureUser { user, .. } => validate_account("step.user", user),
            Self::EnsureGroup { group, .. } => validate_account("step.group", group),
            Self::EnsureDirectory { path, mode } => {
                validate_absolute_path("step.path", path)?;
                validate_mode(*mode)
            }
            Self::WriteManagedFile {
                path,
                content_sha256,
                mode,
            } => {
                validate_absolute_path("step.path", path)?;
                if content_sha256.len() != 64
                    || !content_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
                {
                    return Err(invalid(
                        "step.content_sha256",
                        "must be a SHA-256 hex digest",
                    ));
                }
                validate_mode(*mode)
            }
            Self::EnsureSymlink { path, target } => {
                validate_absolute_path("step.path", path)?;
                validate_absolute_path("step.target", target)
            }
            Self::ServiceAction { unit, .. } => validate_unit(unit),
            Self::ProviderAction {
                provider,
                operation,
                ..
            } => {
                validate_resource_id("step.provider", provider)?;
                validate_resource_id("step.operation", operation)
            }
            Self::HealthCheck { check } => check.validate(),
            Self::RecordOutput {
                resource,
                name,
                value,
                sensitive,
            } => {
                resource.validate()?;
                validate_output_name("step.output", name)?;
                if *sensitive
                    && !value
                        .as_str()
                        .is_some_and(|text| text.starts_with("secret://"))
                {
                    return Err(invalid(
                        "step.value",
                        "sensitive outputs must contain a secret reference",
                    ));
                }
                Ok(())
            }
            Self::CommitState => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceAction {
    Enable,
    Disable,
    Start,
    Stop,
    Restart,
    Reload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum HealthCheck {
    Tcp { address: String, port: u16 },
    Http { url: String, expected_status: u16 },
    Systemd { unit: String },
    Provider { provider: String, operation: String },
}

impl HealthCheck {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Tcp { address, port } => {
                if address.parse::<std::net::IpAddr>().is_err() || *port == 0 {
                    return Err(invalid("step.health_check", "invalid TCP endpoint"));
                }
                Ok(())
            }
            Self::Http {
                url,
                expected_status,
            } => {
                if !(url.starts_with("http://") || url.starts_with("https://"))
                    || !(100..=599).contains(expected_status)
                {
                    return Err(invalid("step.health_check", "invalid HTTP check"));
                }
                Ok(())
            }
            Self::Systemd { unit } => validate_unit(unit),
            Self::Provider {
                provider,
                operation,
            } => {
                validate_resource_id("step.health_check.provider", provider)?;
                validate_resource_id("step.health_check.operation", operation)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineStep {
    pub id: String,
    pub summary: String,
    pub action: PipelineAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pipeline {
    pub id: String,
    pub target: ResourceRef,
    pub summary: String,
    pub steps: Vec<PipelineStep>,
}

impl Pipeline {
    pub fn validate(&self) -> Result<()> {
        validate_resource_id("pipeline.id", &self.id)?;
        self.target.validate()?;
        if self.summary.trim().is_empty() {
            return Err(invalid("pipeline.summary", "must not be empty"));
        }
        if self.steps.is_empty() {
            return Err(invalid("pipeline.steps", "must not be empty"));
        }
        let mut ids = BTreeSet::new();
        for (index, step) in self.steps.iter().enumerate() {
            validate_resource_id("pipeline.step.id", &step.id)?;
            if !ids.insert(step.id.as_str()) {
                return Err(invalid("pipeline.step.id", "duplicate step id"));
            }
            if step.summary.trim().is_empty() {
                return Err(invalid("pipeline.step.summary", "must not be empty"));
            }
            step.action.validate()?;
            if matches!(step.action, PipelineAction::CommitState) && index + 1 != self.steps.len() {
                return Err(invalid(
                    "pipeline.steps",
                    "CommitState must be the final step",
                ));
            }
        }
        if !matches!(
            self.steps.last().map(|step| &step.action),
            Some(PipelineAction::CommitState)
        ) {
            return Err(invalid(
                "pipeline.steps",
                "the final step must commit state",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineStatus {
    Planned,
    Running,
    Succeeded,
    Failed,
    Recovering,
    Recovered,
    RecoveryFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Recovered,
    RecoveryFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepJournalEntry {
    pub step_id: String,
    pub status: StepStatus,
    pub started_at_unix_ms: Option<u64>,
    pub finished_at_unix_ms: Option<u64>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineExecution {
    pub id: String,
    pub pipeline_id: String,
    pub target: ResourceRef,
    pub status: PipelineStatus,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
    pub steps: Vec<StepJournalEntry>,
}

impl PipelineExecution {
    pub fn planned(id: impl Into<String>, pipeline: &Pipeline, now: u64) -> Result<Self> {
        pipeline.validate()?;
        let execution = Self {
            id: id.into(),
            pipeline_id: pipeline.id.clone(),
            target: pipeline.target.clone(),
            status: PipelineStatus::Planned,
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
            steps: pipeline
                .steps
                .iter()
                .map(|step| StepJournalEntry {
                    step_id: step.id.clone(),
                    status: StepStatus::Pending,
                    started_at_unix_ms: None,
                    finished_at_unix_ms: None,
                    message: None,
                })
                .collect(),
        };
        validate_resource_id("execution.id", &execution.id)?;
        Ok(execution)
    }

    pub fn transition(&mut self, next: PipelineStatus, now: u64) -> Result<()> {
        let valid = matches!(
            (self.status, next),
            (PipelineStatus::Planned, PipelineStatus::Running)
                | (PipelineStatus::Running, PipelineStatus::Succeeded)
                | (PipelineStatus::Running, PipelineStatus::Failed)
                | (PipelineStatus::Failed, PipelineStatus::Recovering)
                | (PipelineStatus::Recovering, PipelineStatus::Recovered)
                | (PipelineStatus::Recovering, PipelineStatus::RecoveryFailed)
        );
        if !valid {
            return Err(invalid("execution.status", "invalid pipeline transition"));
        }
        if now < self.updated_at_unix_ms {
            return Err(invalid(
                "execution.updated_at_unix_ms",
                "time moved backwards",
            ));
        }
        self.status = next;
        self.updated_at_unix_ms = now;
        Ok(())
    }

    /// Marks the next pending step running, enforcing declared pipeline order.
    pub fn start_step(&mut self, step_id: &str, now: u64) -> Result<()> {
        if self.status != PipelineStatus::Running || now < self.updated_at_unix_ms {
            return Err(invalid(
                "execution.status",
                "steps may start only while the pipeline is running and time is monotonic",
            ));
        }
        let index = self
            .steps
            .iter()
            .position(|step| step.step_id == step_id)
            .ok_or_else(|| invalid("execution.step_id", "unknown pipeline step"))?;
        if self.steps[index].status != StepStatus::Pending
            || self.steps[..index]
                .iter()
                .any(|step| step.status != StepStatus::Succeeded)
        {
            return Err(invalid(
                "execution.step_id",
                "step is not the next pending step",
            ));
        }
        self.steps[index].status = StepStatus::Running;
        self.steps[index].started_at_unix_ms = Some(now);
        self.updated_at_unix_ms = now;
        Ok(())
    }

    /// Completes the currently running step with a durable outcome.
    pub fn finish_step(
        &mut self,
        step_id: &str,
        succeeded: bool,
        message: Option<String>,
        now: u64,
    ) -> Result<()> {
        let step = self
            .steps
            .iter_mut()
            .find(|step| step.step_id == step_id)
            .ok_or_else(|| invalid("execution.step_id", "unknown pipeline step"))?;
        if step.status != StepStatus::Running
            || step.started_at_unix_ms.is_some_and(|started| started > now)
            || now < self.updated_at_unix_ms
        {
            return Err(invalid(
                "execution.step_id",
                "step is not running or completion time is invalid",
            ));
        }
        step.status = if succeeded {
            StepStatus::Succeeded
        } else {
            StepStatus::Failed
        };
        step.finished_at_unix_ms = Some(now);
        step.message = message;
        self.updated_at_unix_ms = now;
        Ok(())
    }

    /// Completed steps in reverse order, suitable for deterministic recovery.
    pub fn recovery_order(&self) -> Vec<&str> {
        self.steps
            .iter()
            .rev()
            .filter(|step| step.status == StepStatus::Succeeded)
            .map(|step| step.step_id.as_str())
            .collect()
    }
}

fn validate_absolute_path(field: &str, value: &str) -> Result<()> {
    if Path::new(value).is_absolute()
        && value.len() <= 4096
        && !value.bytes().any(|byte| byte.is_ascii_control())
    {
        Ok(())
    } else {
        Err(invalid(field, "must be a safe absolute path"))
    }
}

fn validate_mode(mode: u32) -> Result<()> {
    if mode <= 0o7777 {
        Ok(())
    } else {
        Err(invalid("step.mode", "must be a valid Unix mode"))
    }
}

fn validate_token(field: &str, value: &str, max: usize) -> Result<()> {
    if !value.is_empty()
        && value.len() <= max
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.' | b'_' | b':')
        })
    {
        Ok(())
    } else {
        Err(invalid(field, "contains unsupported characters"))
    }
}

fn validate_account(field: &str, value: &str) -> Result<()> {
    validate_token(field, value, 32)
}

fn validate_unit(value: &str) -> Result<()> {
    validate_token("step.unit", value, 255)?;
    if value.ends_with(".service") {
        Ok(())
    } else {
        Err(invalid("step.unit", "must name a systemd service unit"))
    }
}

fn invalid(field: &str, message: &str) -> LumicError {
    LumicError::InvalidInput {
        field: field.into(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::ResourceKind;

    fn pipeline() -> Pipeline {
        Pipeline {
            id: "install-redis".into(),
            target: ResourceRef::new(ResourceKind::ManagedService, "redis.main").unwrap(),
            summary: "Install Redis".into(),
            steps: vec![
                PipelineStep {
                    id: "package".into(),
                    summary: "Install package".into(),
                    action: PipelineAction::EnsurePackage {
                        package: "redis-server".into(),
                    },
                },
                PipelineStep {
                    id: "commit".into(),
                    summary: "Commit state".into(),
                    action: PipelineAction::CommitState,
                },
            ],
        }
    }

    #[test]
    fn requires_commit_as_final_step() {
        let mut value = pipeline();
        assert!(value.validate().is_ok());
        value.steps.swap(0, 1);
        assert!(value.validate().is_err());
    }

    #[test]
    fn journals_valid_transitions_and_reverse_recovery_order() {
        let mut execution = PipelineExecution::planned("run-1", &pipeline(), 1).unwrap();
        execution.transition(PipelineStatus::Running, 2).unwrap();
        assert!(execution.start_step("commit", 3).is_err());
        execution.start_step("package", 3).unwrap();
        execution.finish_step("package", true, None, 4).unwrap();
        execution.start_step("commit", 5).unwrap();
        execution.finish_step("commit", true, None, 6).unwrap();
        execution.transition(PipelineStatus::Failed, 7).unwrap();
        execution.transition(PipelineStatus::Recovering, 8).unwrap();
        assert_eq!(execution.recovery_order(), vec!["commit", "package"]);
        assert!(execution.transition(PipelineStatus::Succeeded, 9).is_err());
    }
}
