use crate::{
    ProcessOutput, ProcessRunner, ProcessSpec, audit_store::AuditStore, event_store::EventStore,
};
use lumic_core::{
    LumicError, OperationContext, Result,
    events::{AuditRecord, Event},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceAction {
    Start,
    Stop,
    Restart,
    Reload,
    Enable,
    Disable,
    DaemonReload,
}

impl ServiceAction {
    const fn argument(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
            Self::Reload => "reload",
            Self::Enable => "enable",
            Self::Disable => "disable",
            Self::DaemonReload => "daemon-reload",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub unit: String,
    pub load_state: String,
    pub active_state: String,
    pub sub_state: String,
    pub enabled: bool,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceMutation {
    pub unit: String,
    pub action: ServiceAction,
    pub before: ServiceStatus,
    pub after: ServiceStatus,
    pub changed: bool,
}

#[derive(Debug, Clone)]
pub struct SystemdServiceManager {
    runner: ProcessRunner,
    events: EventStore,
    audit: AuditStore,
}

impl SystemdServiceManager {
    pub fn at_state_dir(state_dir: impl AsRef<std::path::Path>) -> Self {
        Self {
            runner: ProcessRunner,
            events: EventStore::at_state_dir(&state_dir),
            audit: AuditStore::at_state_dir(state_dir),
        }
    }

    pub async fn inspect(&self, unit: &str) -> Result<ServiceStatus> {
        validate_unit(unit)?;
        let output = self
            .run(ProcessSpec::new("systemctl").args([
                "show",
                "--no-pager",
                "--property=Id,LoadState,ActiveState,SubState,UnitFileState,Description",
                "--",
                unit,
            ]))
            .await?;
        parse_status(unit, &String::from_utf8_lossy(&output.stdout))
    }

    pub async fn apply(
        &self,
        unit: &str,
        action: ServiceAction,
        context: &OperationContext,
    ) -> Result<ServiceMutation> {
        validate_unit(unit)?;
        if action == ServiceAction::DaemonReload {
            return Err(LumicError::InvalidInput {
                field: "action".into(),
                message: "daemon-reload is internal and has no unit target".into(),
            });
        }
        let before = self.inspect(unit).await?;
        if context.dry_run {
            return Ok(ServiceMutation {
                unit: unit.into(),
                action,
                before: before.clone(),
                after: before,
                changed: false,
            });
        }
        if let Err(error) = self
            .run(ProcessSpec::new("systemctl").args([action.argument(), "--", unit]))
            .await
        {
            self.record_failure(unit, action, &before, context, &error)?;
            return Err(error);
        }
        let after = match self.inspect(unit).await {
            Ok(after) => after,
            Err(error) => {
                self.record_failure(unit, action, &before, context, &error)?;
                return Err(error);
            }
        };
        let mutation = ServiceMutation {
            unit: unit.into(),
            action,
            changed: before != after
                || matches!(action, ServiceAction::Restart | ServiceAction::Reload),
            before,
            after,
        };
        self.record(&mutation, context)?;
        Ok(mutation)
    }

    pub async fn daemon_reload(&self) -> Result<()> {
        self.run(ProcessSpec::new("systemctl").args(["daemon-reload"]))
            .await?;
        Ok(())
    }

    async fn run(&self, spec: ProcessSpec) -> Result<ProcessOutput> {
        let executable = spec.executable.clone();
        let output = self.runner.run(&spec).await?;
        if output.success() {
            Ok(output)
        } else {
            Err(LumicError::Process {
                executable,
                message: String::from_utf8_lossy(&output.stderr).trim().into(),
            })
        }
    }

    fn record(&self, mutation: &ServiceMutation, context: &OperationContext) -> Result<()> {
        let event_type = format!("service.{}", mutation.action.argument());
        self.events.append(&Event::now(
            &event_type,
            &context.actor,
            context.interface,
            "service",
            &mutation.unit,
            &context.correlation_id,
            json!({"changed": mutation.changed, "active_state": mutation.after.active_state}),
        ))?;
        self.audit.append(&AuditRecord::now(
            context,
            format!("service.{}", mutation.action.argument()),
            mutation.action.argument(),
            "service",
            &mutation.unit,
            json!({"unit": mutation.unit}),
            Some(serde_json::to_value(&mutation.before).unwrap_or_default()),
            Some(serde_json::to_value(&mutation.after).unwrap_or_default()),
            true,
            "systemd operation completed",
        ))
    }

    fn record_failure(
        &self,
        unit: &str,
        action: ServiceAction,
        before: &ServiceStatus,
        context: &OperationContext,
        error: &LumicError,
    ) -> Result<()> {
        self.audit.append(&AuditRecord::now(
            context,
            format!("service.{}", action.argument()),
            action.argument(),
            "service",
            unit,
            json!({"unit": unit}),
            Some(serde_json::to_value(before).unwrap_or_default()),
            None,
            false,
            error.to_string(),
        ))
    }
}

pub fn validate_unit(unit: &str) -> Result<()> {
    let valid = !unit.is_empty()
        && unit.len() <= 255
        && unit.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'@' | b':')
        })
        && !unit.starts_with('.')
        && !unit.contains("..");
    if valid {
        Ok(())
    } else {
        Err(LumicError::InvalidInput {
            field: "unit".into(),
            message: "must be a safe systemd unit name".into(),
        })
    }
}

fn parse_status(unit: &str, output: &str) -> Result<ServiceStatus> {
    let values: std::collections::BTreeMap<_, _> = output
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect();
    let get = |name: &str| values.get(name).copied().unwrap_or("").to_owned();
    if get("LoadState").is_empty() {
        return Err(LumicError::Inspection {
            fact: "service".into(),
            message: format!("systemctl returned no state for {unit}"),
        });
    }
    Ok(ServiceStatus {
        unit: get("Id"),
        load_state: get("LoadState"),
        active_state: get("ActiveState"),
        sub_state: get("SubState"),
        enabled: matches!(get("UnitFileState").as_str(), "enabled" | "enabled-runtime"),
        description: get("Description"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_typed_systemd_status_and_rejects_options() {
        let status = parse_status(
            "nginx.service",
            "Id=nginx.service\nLoadState=loaded\nActiveState=active\nSubState=running\nUnitFileState=enabled\nDescription=web\n",
        )
        .unwrap();
        assert!(status.enabled);
        assert_eq!(status.active_state, "active");
        assert!(validate_unit("nginx.service").is_ok());
        assert!(validate_unit("--root=/tmp").is_err());
    }
}
