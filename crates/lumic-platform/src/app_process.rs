use crate::{
    atomic_file::write_atomic,
    systemd::{ServiceAction, SystemdServiceManager},
};
use lumic_core::{
    LumicError, OperationContext, Result,
    application::{
        Application, ApplicationProcess, ApplicationProcessKind, validate_command, validate_slug,
    },
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessConfigurationResult {
    pub process: String,
    pub units: Vec<String>,
    pub changed: bool,
}

#[derive(Debug, Clone)]
pub struct ApplicationProcessManager {
    state_dir: PathBuf,
    unit_dir: PathBuf,
}

impl ApplicationProcessManager {
    pub fn system(state_dir: impl Into<PathBuf>) -> Self {
        Self::new(state_dir, "/etc/systemd/system")
    }

    pub fn new(state_dir: impl Into<PathBuf>, unit_dir: impl Into<PathBuf>) -> Self {
        Self {
            state_dir: state_dir.into(),
            unit_dir: unit_dir.into(),
        }
    }

    pub async fn configure(
        &self,
        application: &Application,
        process: &ApplicationProcess,
        context: &OperationContext,
    ) -> Result<ProcessConfigurationResult> {
        validate_process(process)?;
        let prefix = format!("lumic-app-{}-{}", application.id, process.name);
        let service_name = format!("{prefix}.service");
        let service = render_service(application, process)?;
        let service_write = write_atomic(
            &self.unit_dir.join(&service_name),
            service.as_bytes(),
            0o644,
        )?;
        let mut changed = service_write.changed;
        let mut units = vec![service_name.clone()];
        if process.kind == ApplicationProcessKind::Schedule {
            let timer_name = format!("{prefix}.timer");
            let timer = render_timer(process, &service_name)?;
            changed |=
                write_atomic(&self.unit_dir.join(&timer_name), timer.as_bytes(), 0o644)?.changed;
            units.push(timer_name);
        }
        let systemd = SystemdServiceManager::at_state_dir(&self.state_dir);
        systemd.daemon_reload().await?;
        if process.enabled {
            let activation_unit = units.last().expect("at least one unit");
            systemd
                .apply(activation_unit, ServiceAction::Enable, context)
                .await?;
            systemd
                .apply(activation_unit, ServiceAction::Start, context)
                .await?;
        }
        Ok(ProcessConfigurationResult {
            process: process.name.clone(),
            units,
            changed,
        })
    }
}

fn validate_process(process: &ApplicationProcess) -> Result<()> {
    validate_slug("process", &process.name)?;
    validate_command(&process.command)?;
    match process.kind {
        ApplicationProcessKind::Worker if process.schedule.is_some() => {
            Err(LumicError::InvalidInput {
                field: "schedule".into(),
                message: "worker processes cannot have a timer schedule".into(),
            })
        }
        ApplicationProcessKind::Schedule => {
            let valid = process.schedule.as_deref().is_some_and(|schedule| {
                !schedule.is_empty()
                    && schedule.len() <= 128
                    && !schedule.contains(['\n', '\r', '\0'])
            });
            if valid {
                Ok(())
            } else {
                Err(LumicError::InvalidInput {
                    field: "schedule".into(),
                    message: "scheduled processes require a safe systemd OnCalendar expression"
                        .into(),
                })
            }
        }
        ApplicationProcessKind::Worker => Ok(()),
    }
}

fn render_service(application: &Application, process: &ApplicationProcess) -> Result<String> {
    let command = process
        .command
        .iter()
        .map(|part| systemd_quote(part))
        .collect::<Vec<_>>()
        .join(" ");
    let service_type = if process.kind == ApplicationProcessKind::Schedule {
        "oneshot"
    } else {
        "simple"
    };
    let restart = if process.kind == ApplicationProcessKind::Worker {
        "Restart=on-failure\nRestartSec=5s\n"
    } else {
        ""
    };
    Ok(format!(
        "# Managed by Lumic\n[Unit]\nDescription=Lumic {} process {}\nAfter=network-online.target\n\n[Service]\nType={}\nUser=www-data\nWorkingDirectory={}\nExecStart={}\n{}\n[Install]\nWantedBy=multi-user.target\n",
        application.id,
        process.name,
        service_type,
        systemd_quote(&format!("{}/current", application.root)),
        command,
        restart
    ))
}

fn render_timer(process: &ApplicationProcess, service_name: &str) -> Result<String> {
    let schedule = process
        .schedule
        .as_deref()
        .ok_or_else(|| LumicError::InvalidInput {
            field: "schedule".into(),
            message: "timer schedule is required".into(),
        })?;
    Ok(format!(
        "# Managed by Lumic\n[Unit]\nDescription=Lumic schedule {}\n\n[Timer]\nOnCalendar={}\nPersistent=true\nUnit={}\n\n[Install]\nWantedBy=timers.target\n",
        process.name, schedule, service_name
    ))
}

fn systemd_quote(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('%', "%%")
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumic_core::application::{ApplicationRuntime, HealthCheck, TlsState};
    use std::collections::BTreeMap;

    fn app() -> Application {
        Application {
            id: "demo".into(),
            name: "demo".into(),
            domain: "demo.example.com".into(),
            www_alias: false,
            root: "/var/lib/lumic/apps/demo".into(),
            runtime: ApplicationRuntime::Php,
            repository: None,
            environment_references: BTreeMap::new(),
            health_check: HealthCheck::default(),
            processes: Vec::new(),
            web_configured: true,
            tls: TlsState::default(),
            release_retention: 5,
            health_status: "healthy".into(),
            created_at_unix_ms: 1,
            updated_at_unix_ms: 1,
        }
    }

    #[test]
    fn renders_worker_as_argv_without_a_shell() {
        let process = ApplicationProcess {
            name: "queue".into(),
            kind: ApplicationProcessKind::Worker,
            command: vec!["php".into(), "artisan".into(), "queue:work".into()],
            schedule: None,
            enabled: true,
        };
        let unit = render_service(&app(), &process).unwrap();
        assert!(unit.contains("ExecStart=\"php\" \"artisan\" \"queue:work\""));
        assert!(!unit.contains("sh -c"));
    }

    #[test]
    fn rejects_worker_schedule_and_control_characters() {
        let process = ApplicationProcess {
            name: "queue".into(),
            kind: ApplicationProcessKind::Worker,
            command: vec!["php\n".into()],
            schedule: Some("daily".into()),
            enabled: true,
        };
        assert!(validate_process(&process).is_err());
    }
}
