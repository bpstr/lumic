use crate::{
    atomic_file::write_atomic,
    systemd::{ServiceAction, SystemdServiceManager},
};
use lumic_core::{
    LumicError, OperationContext, Result,
    application::{
        Application, ApplicationProcess, ApplicationProcessKind, MissedRunPolicy, ScheduleTiming,
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
        process.validate()?;
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
        let activation_unit = match process.kind {
            ApplicationProcessKind::Worker => service_name.as_str(),
            ApplicationProcessKind::Schedule => {
                units
                    .get(1)
                    .map(String::as_str)
                    .ok_or_else(|| LumicError::Internal {
                        message: "scheduled process did not produce a timer unit".into(),
                    })?
            }
        };
        if process.enabled {
            systemd
                .apply(activation_unit, ServiceAction::Enable, context)
                .await?;
            systemd
                .apply(activation_unit, ServiceAction::Start, context)
                .await?;
        } else {
            systemd
                .apply(activation_unit, ServiceAction::Stop, context)
                .await?;
            systemd
                .apply(activation_unit, ServiceAction::Disable, context)
                .await?;
        }
        Ok(ProcessConfigurationResult {
            process: process.name.clone(),
            units,
            changed,
        })
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
        .as_ref()
        .ok_or_else(|| LumicError::InvalidInput {
            field: "schedule".into(),
            message: "timer schedule is required".into(),
        })?;
    schedule.validate()?;
    let timing = match &schedule.timing {
        ScheduleTiming::Calendar { expression } => format!("OnCalendar={expression}"),
        ScheduleTiming::Interval { seconds } => format!("OnUnitActiveSec={seconds}s"),
    };
    let persistent = schedule.missed_run_policy == MissedRunPolicy::RunImmediately;
    let jitter = if schedule.jitter_seconds > 0 {
        format!("RandomizedDelaySec={}s\n", schedule.jitter_seconds)
    } else {
        String::new()
    };
    Ok(format!(
        "# Managed by Lumic\n[Unit]\nDescription=Lumic schedule {}\n\n[Timer]\n{}\nPersistent={}\n{}Unit={}\n\n[Install]\nWantedBy=timers.target\n",
        process.name, timing, persistent, jitter, service_name
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
    use lumic_core::application::{ApplicationRuntime, ApplicationSchedule, HealthCheck, TlsState};
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
            service_references: Vec::new(),
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
            schedule: Some(ApplicationSchedule::calendar("daily")),
            enabled: true,
        };
        assert!(process.validate().is_err());
    }

    #[test]
    fn renders_backend_neutral_interval_schedule_as_a_systemd_timer() {
        let mut schedule = ApplicationSchedule::interval(300);
        schedule.missed_run_policy = MissedRunPolicy::Skip;
        schedule.jitter_seconds = 15;
        let process = ApplicationProcess {
            name: "cleanup".into(),
            kind: ApplicationProcessKind::Schedule,
            command: vec!["php".into(), "cleanup.php".into()],
            schedule: Some(schedule),
            enabled: true,
        };
        let unit = render_timer(&process, "lumic-app-demo-cleanup.service").unwrap();
        assert!(unit.contains("OnUnitActiveSec=300s"));
        assert!(unit.contains("Persistent=false"));
        assert!(unit.contains("RandomizedDelaySec=15s"));
    }
}
