use crate::{
    atomic_file::write_atomic,
    systemd::{ServiceAction, SystemdServiceManager},
};
use lumic_core::{
    LumicError, OperationContext, Result,
    application::{
        Application, ApplicationProcess, ApplicationProcessKind, MissedRunPolicy, NodeHandoff,
        ScheduleTiming,
    },
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessConfigurationResult {
    pub process: String,
    pub units: Vec<String>,
    pub changed: bool,
}

pub struct NodeReleaseStart<'a> {
    pub application: &'a Application,
    pub handoff: &'a NodeHandoff,
    pub release: &'a Path,
    pub deployment_id: &'a str,
    pub port: u16,
    pub environment_file: Option<&'a Path>,
    pub context: &'a OperationContext,
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
        environment_file: Option<&Path>,
        context: &OperationContext,
    ) -> Result<ProcessConfigurationResult> {
        process.validate()?;
        let prefix = format!("lumic-app-{}-{}", application.id, process.name);
        let service_name = format!("{prefix}.service");
        let service = render_service(application, process, environment_file)?;
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
        if process.health_check.is_some() {
            let health_service_name = format!("{prefix}-health.service");
            let health_timer_name = format!("{prefix}-health.timer");
            changed |= write_atomic(
                &self.unit_dir.join(&health_service_name),
                render_health_service(application, process, &service_name)?.as_bytes(),
                0o644,
            )?
            .changed;
            changed |= write_atomic(
                &self.unit_dir.join(&health_timer_name),
                render_health_timer(process, &health_service_name)?.as_bytes(),
                0o644,
            )?
            .changed;
            units.extend([health_service_name, health_timer_name.clone()]);
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
        if let Some(health_timer) = units.iter().find(|unit| unit.ends_with("-health.timer")) {
            if process.enabled {
                systemd
                    .apply(health_timer, ServiceAction::Enable, context)
                    .await?;
                systemd
                    .apply(health_timer, ServiceAction::Start, context)
                    .await?;
            } else {
                systemd
                    .apply(health_timer, ServiceAction::Stop, context)
                    .await?;
                systemd
                    .apply(health_timer, ServiceAction::Disable, context)
                    .await?;
            }
        }
        Ok(ProcessConfigurationResult {
            process: process.name.clone(),
            units,
            changed,
        })
    }

    pub async fn start_node_release(&self, request: NodeReleaseStart<'_>) -> Result<String> {
        let NodeReleaseStart {
            application,
            handoff,
            release,
            deployment_id,
            port,
            environment_file,
            context,
        } = request;
        handoff.validate()?;
        if application.runtime != lumic_core::application::ApplicationRuntime::Node
            || ![handoff.primary_port, handoff.secondary_port].contains(&port)
            || !release.is_dir()
            || deployment_id.is_empty()
            || !deployment_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(LumicError::InvalidInput {
                field: "node_handoff".into(),
                message: "release, deployment identifier, or selected handoff port is invalid"
                    .into(),
            });
        }
        let unit = format!("lumic-app-{}-web-{deployment_id}.service", application.id);
        let content =
            render_node_release_service(application, handoff, release, port, environment_file);
        write_atomic(&self.unit_dir.join(&unit), content.as_bytes(), 0o644)?;
        let systemd = SystemdServiceManager::at_state_dir(&self.state_dir);
        systemd.daemon_reload().await?;
        systemd.apply(&unit, ServiceAction::Start, context).await?;
        Ok(unit)
    }

    pub async fn stop_node_release(&self, unit: &str, context: &OperationContext) -> Result<()> {
        if !unit.starts_with("lumic-app-") || !unit.ends_with(".service") {
            return Err(LumicError::InvalidInput {
                field: "unit".into(),
                message: "is not a Lumic application release unit".into(),
            });
        }
        let systemd = SystemdServiceManager::at_state_dir(&self.state_dir);
        systemd.apply(unit, ServiceAction::Stop, context).await?;
        Ok(())
    }

    pub async fn start_existing_node_release(
        &self,
        unit: &str,
        context: &OperationContext,
    ) -> Result<()> {
        if !unit.starts_with("lumic-app-")
            || !unit.ends_with(".service")
            || !self.unit_dir.join(unit).is_file()
        {
            return Err(LumicError::InvalidInput {
                field: "unit".into(),
                message: "retained Node release unit is unavailable".into(),
            });
        }
        SystemdServiceManager::at_state_dir(&self.state_dir)
            .apply(unit, ServiceAction::Start, context)
            .await?;
        Ok(())
    }
}

fn render_node_release_service(
    application: &Application,
    handoff: &NodeHandoff,
    release: &Path,
    port: u16,
    environment_file: Option<&Path>,
) -> String {
    let command = handoff
        .command
        .iter()
        .enumerate()
        .map(|(index, part)| {
            if index == 0 && part == "node" {
                systemd_quote("/usr/bin/node")
            } else {
                systemd_quote(part)
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    let environment = environment_file.map_or_else(String::new, |path| {
        format!(
            "EnvironmentFile={}\n",
            systemd_quote(&path.to_string_lossy())
        )
    });
    format!(
        "# Managed by Lumic\n[Unit]\nDescription=Lumic {} blue-green web process\nAfter=network-online.target\n\n[Service]\nType=simple\nUser=www-data\nWorkingDirectory={}\nEnvironment=PORT={}\n{}ExecStart={}\nRestart=on-failure\nRestartSec=2s\n\n[Install]\nWantedBy=multi-user.target\n",
        application.id,
        systemd_quote(&release.to_string_lossy()),
        port,
        environment,
        command,
    )
}

fn render_service(
    application: &Application,
    process: &ApplicationProcess,
    environment_file: Option<&Path>,
) -> Result<String> {
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
        format!(
            "Restart={}\nRestartSec=5s\n",
            process.restart_policy.systemd_value()
        )
    } else {
        String::new()
    };
    let environment = environment_file.map_or_else(String::new, |path| {
        format!(
            "EnvironmentFile={}\n",
            systemd_quote(&path.to_string_lossy())
        )
    });
    let explicit_environment = process
        .environment
        .iter()
        .map(|(key, value)| format!("Environment={}\n", systemd_quote(&format!("{key}={value}"))))
        .collect::<String>();
    let working_directory = process.working_directory.as_ref().map_or_else(
        || PathBuf::from(&application.root).join("current"),
        |directory| {
            let path = Path::new(directory);
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                PathBuf::from(&application.root).join("current").join(path)
            }
        },
    );
    Ok(format!(
        "# Managed by Lumic\n[Unit]\nDescription=Lumic {} process {}\nAfter=network-online.target\n\n[Service]\nType={}\nUser=www-data\nWorkingDirectory={}\n{}{}ExecStart={}\n{}\n[Install]\nWantedBy=multi-user.target\n",
        application.id,
        process.name,
        service_type,
        systemd_quote(&working_directory.to_string_lossy()),
        environment,
        explicit_environment,
        command,
        restart
    ))
}

fn render_health_service(
    application: &Application,
    process: &ApplicationProcess,
    process_unit: &str,
) -> Result<String> {
    let health = process
        .health_check
        .as_ref()
        .ok_or_else(|| LumicError::Internal {
            message: "process health service requested without a health check".into(),
        })?;
    health.validate()?;
    let command = health
        .command
        .iter()
        .map(|part| systemd_quote(part))
        .collect::<Vec<_>>()
        .join(" ");
    Ok(format!(
        "# Managed by Lumic\n[Unit]\nDescription=Lumic {} process {} health check\nAfter={}\n\n[Service]\nType=oneshot\nUser=www-data\nWorkingDirectory={}\nTimeoutStartSec={}s\nExecStart={}\n",
        application.id,
        process.name,
        process_unit,
        systemd_quote(&format!("{}/current", application.root)),
        health.timeout_seconds,
        command,
    ))
}

fn render_health_timer(process: &ApplicationProcess, health_service: &str) -> Result<String> {
    let health = process
        .health_check
        .as_ref()
        .ok_or_else(|| LumicError::Internal {
            message: "process health timer requested without a health check".into(),
        })?;
    Ok(format!(
        "# Managed by Lumic\n[Unit]\nDescription=Lumic process {} health schedule\n\n[Timer]\nOnUnitActiveSec={}s\nUnit={}\n\n[Install]\nWantedBy=timers.target\n",
        process.name, health.interval_seconds, health_service,
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
            runtime_intent: Default::default(),
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
            environment: Default::default(),
            working_directory: None,
            restart_policy: Default::default(),
            health_check: None,
        };
        let unit = render_service(&app(), &process, None).unwrap();
        assert!(unit.contains("ExecStart=\"php\" \"artisan\" \"queue:work\""));
        assert!(!unit.contains("sh -c"));
    }

    #[test]
    fn renders_worker_runtime_policy_and_health_units() {
        let process = ApplicationProcess {
            name: "queue".into(),
            kind: ApplicationProcessKind::Worker,
            command: vec!["php".into(), "artisan".into(), "queue:work".into()],
            schedule: None,
            enabled: true,
            environment: BTreeMap::from([("QUEUE".into(), "priority".into())]),
            working_directory: Some("worker".into()),
            restart_policy: lumic_core::application::ProcessRestartPolicy::Always,
            health_check: Some(lumic_core::application::ProcessHealthCheck {
                command: vec!["php".into(), "artisan".into(), "queue:health".into()],
                interval_seconds: 45,
                timeout_seconds: 7,
            }),
        };

        let unit = render_service(&app(), &process, None).unwrap();
        assert!(unit.contains("WorkingDirectory=\"/var/lib/lumic/apps/demo/current/worker\""));
        assert!(unit.contains("Environment=\"QUEUE=priority\""));
        assert!(unit.contains("Restart=always"));

        let health =
            render_health_service(&app(), &process, "lumic-app-demo-queue.service").unwrap();
        assert!(health.contains("TimeoutStartSec=7s"));
        assert!(health.contains("ExecStart=\"php\" \"artisan\" \"queue:health\""));
        let timer = render_health_timer(&process, "lumic-app-demo-queue-health.service").unwrap();
        assert!(timer.contains("OnUnitActiveSec=45s"));
    }

    #[test]
    fn rejects_worker_schedule_and_control_characters() {
        let process = ApplicationProcess {
            name: "queue".into(),
            kind: ApplicationProcessKind::Worker,
            command: vec!["php\n".into()],
            schedule: Some(ApplicationSchedule::calendar("daily")),
            enabled: true,
            environment: Default::default(),
            working_directory: None,
            restart_policy: Default::default(),
            health_check: None,
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
            environment: Default::default(),
            working_directory: None,
            restart_policy: Default::default(),
            health_check: None,
        };
        let unit = render_timer(&process, "lumic-app-demo-cleanup.service").unwrap();
        assert!(unit.contains("OnUnitActiveSec=300s"));
        assert!(unit.contains("Persistent=false"));
        assert!(unit.contains("RandomizedDelaySec=15s"));
    }

    #[test]
    fn renders_release_scoped_node_process_with_an_explicit_port() {
        let handoff = NodeHandoff {
            command: vec!["node".into(), "server.js".into()],
            primary_port: 3100,
            secondary_port: 3101,
            drain_seconds: 5,
        };
        let release = Path::new("/var/lib/lumic/apps/demo/releases/123");
        let unit = render_node_release_service(&app(), &handoff, release, 3101, None);
        assert!(unit.contains("WorkingDirectory=\"/var/lib/lumic/apps/demo/releases/123\""));
        assert!(unit.contains("Environment=PORT=3101"));
        assert!(unit.contains("ExecStart=\"/usr/bin/node\" \"server.js\""));
        assert!(!unit.contains("sh -c"));
    }

    #[test]
    fn preserves_an_explicit_node_release_executable() {
        let handoff = NodeHandoff {
            command: vec!["/opt/node/bin/node".into(), "server.js".into()],
            primary_port: 3100,
            secondary_port: 3101,
            drain_seconds: 5,
        };
        let unit = render_node_release_service(
            &app(),
            &handoff,
            Path::new("/srv/apps/demo/releases/release-1"),
            3100,
            None,
        );
        assert!(unit.contains("ExecStart=\"/opt/node/bin/node\" \"server.js\""));
    }

    #[test]
    fn release_process_uses_a_root_loaded_runtime_environment_file() {
        let handoff = NodeHandoff {
            command: vec!["npm".into(), "start".into()],
            primary_port: 3101,
            secondary_port: 3102,
            drain_seconds: 5,
        };
        let unit = render_node_release_service(
            &app(),
            &handoff,
            Path::new("/srv/apps/demo/releases/release-1"),
            3101,
            Some(Path::new("/run/lumic/application-environments/demo.env")),
        );
        assert!(unit.contains("EnvironmentFile=\"/run/lumic/application-environments/demo.env\""));
        assert!(!unit.contains("SECRET="));
    }
}
