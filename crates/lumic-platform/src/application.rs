use crate::{
    ProcessOutput, ProcessRunner, ProcessSpec,
    app_process::{ApplicationProcessManager, ProcessConfigurationResult},
    atomic_file::write_atomic,
    audit_store::AuditStore,
    event_store::EventStore,
    runtime::{RuntimeInstallResult, RuntimeManager},
    secret_store::SecretStore,
    web::{NginxManager, TlsManager, WebConfigurationResult},
};
use lumic_core::{
    Capability, Change, LumicError, OperationContext, Plan, Result, Risk, RiskLevel,
    application::{
        Application, ApplicationProcess, ApplicationRuntime, ApplicationServiceReference,
        Deployment, DeploymentPhase, DeploymentPhaseStatus, DeploymentStatus, RepositoryConfig,
        TlsState, unix_time_ms, validate_branch, validate_command, validate_domain,
        validate_repository_url, validate_slug,
    },
    events::{AuditRecord, Event},
    infrastructure::PortableApplication,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, symlink};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvisionResult {
    pub runtime: RuntimeInstallResult,
    pub web: WebConfigurationResult,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ApplicationState {
    version: u32,
    applications: Vec<Application>,
    deployments: Vec<Deployment>,
}

#[derive(Debug, Clone)]
pub struct ApplicationStore {
    path: PathBuf,
}

impl ApplicationStore {
    pub fn at_state_dir(directory: impl AsRef<Path>) -> Self {
        Self {
            path: directory.as_ref().join("applications.json"),
        }
    }

    fn load(&self) -> Result<ApplicationState> {
        if !self.path.exists() {
            return Ok(ApplicationState {
                version: 1,
                ..ApplicationState::default()
            });
        }
        let bytes = fs::read(&self.path).map_err(state_io_error)?;
        serde_json::from_slice(&bytes).map_err(|error| LumicError::Internal {
            message: format!("application state is invalid: {error}"),
        })
    }

    fn save(&self, state: &ApplicationState) -> Result<()> {
        let parent = self.path.parent().ok_or_else(|| LumicError::Internal {
            message: "application state path has no parent".into(),
        })?;
        fs::create_dir_all(parent).map_err(state_io_error)?;
        let temporary = parent.join(format!(".applications-{}.tmp", std::process::id()));
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&temporary).map_err(state_io_error)?;
        serde_json::to_writer_pretty(&mut file, state).map_err(|error| LumicError::Internal {
            message: format!("could not serialize application state: {error}"),
        })?;
        file.write_all(b"\n").map_err(state_io_error)?;
        file.sync_all().map_err(state_io_error)?;
        fs::rename(&temporary, &self.path).map_err(state_io_error)
    }
}

#[derive(Debug, Clone)]
pub struct ApplicationService {
    store: ApplicationStore,
    events: EventStore,
    audit: AuditStore,
    state_dir: PathBuf,
    apps_root: PathBuf,
    runner: ProcessRunner,
}

impl ApplicationService {
    pub fn new(state_dir: impl AsRef<Path>, apps_root: impl Into<PathBuf>) -> Self {
        Self {
            store: ApplicationStore::at_state_dir(&state_dir),
            events: EventStore::at_state_dir(&state_dir),
            audit: AuditStore::at_state_dir(&state_dir),
            state_dir: state_dir.as_ref().to_path_buf(),
            apps_root: apps_root.into(),
            runner: ProcessRunner,
        }
    }

    pub fn list(&self) -> Result<Vec<Application>> {
        Ok(self.store.load()?.applications)
    }

    pub fn inspect(&self, id: &str) -> Result<Application> {
        validate_slug("application", id)?;
        self.store
            .load()?
            .applications
            .into_iter()
            .find(|application| application.id == id)
            .ok_or_else(|| not_found(id))
    }

    pub fn create(
        &self,
        name: &str,
        domain: &str,
        runtime: ApplicationRuntime,
        www_alias: bool,
        context: &OperationContext,
    ) -> Result<Application> {
        validate_slug("application", name)?;
        validate_domain(domain)?;
        let mut state = self.store.load()?;
        if state
            .applications
            .iter()
            .any(|app| app.id == name || app.domain == domain)
        {
            return Err(LumicError::InvalidInput {
                field: "application".into(),
                message: "application name and domain must be unique".into(),
            });
        }
        let root = self.apps_root.join(name);
        for directory in ["releases", "shared", "repository"] {
            fs::create_dir_all(root.join(directory)).map_err(state_io_error)?;
        }
        let now = unix_time_ms();
        let application = Application {
            id: name.into(),
            name: name.into(),
            domain: domain.into(),
            www_alias,
            root: root.to_string_lossy().into_owned(),
            runtime,
            repository: None,
            environment_references: Default::default(),
            service_references: Vec::new(),
            health_check: Default::default(),
            processes: Vec::new(),
            web_configured: false,
            tls: TlsState::default(),
            release_retention: 5,
            health_status: "not_deployed".into(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };
        state.applications.push(application.clone());
        self.store.save(&state)?;
        self.emit(
            "application.created",
            &application.id,
            context,
            json!({
                "domain": application.domain,
                "runtime": application.runtime,
            }),
        )?;
        self.audit.append(&AuditRecord::now(
            context,
            "application.create",
            "create",
            "application",
            &application.id,
            json!({"domain": application.domain, "runtime": application.runtime}),
            None,
            serde_json::to_value(&application).ok(),
            true,
            "application created",
        ))?;
        Ok(application)
    }

    pub fn set_repository(
        &self,
        id: &str,
        url: &str,
        branch: &str,
        credential_reference: Option<String>,
        context: &OperationContext,
    ) -> Result<Application> {
        validate_repository_url(url)?;
        validate_branch(branch)?;
        let mut state = self.store.load()?;
        let application = state
            .applications
            .iter_mut()
            .find(|application| application.id == id)
            .ok_or_else(|| not_found(id))?;
        application.repository = Some(RepositoryConfig {
            url: url.into(),
            branch: branch.into(),
            credential_reference: credential_reference.clone(),
        });
        application.updated_at_unix_ms = unix_time_ms();
        let application = application.clone();
        self.store.save(&state)?;
        self.emit(
            "application.repository_configured",
            id,
            context,
            json!({
                "branch": branch,
            }),
        )?;
        self.audit.append(&AuditRecord::now(
            context, "application.configure", "set_repository", "application", id,
            json!({"url": url, "branch": branch, "credential_reference": credential_reference.as_ref().map(|_| "redacted")}),
            None, Some(json!({"repository_configured": true, "branch": branch})), true,
            "repository configured",
        ))?;
        Ok(application)
    }

    pub fn import_ssh_credential(
        &self,
        name: &str,
        source: &Path,
        context: &OperationContext,
    ) -> Result<String> {
        validate_slug("credential", name)?;
        if !source.is_file() || source.is_symlink() {
            return Err(LumicError::InvalidInput {
                field: "credential".into(),
                message: "source must be a regular private-key file".into(),
            });
        }
        let bytes = fs::read(source).map_err(state_io_error)?;
        if bytes.is_empty() || bytes.len() > 64 * 1024 {
            return Err(LumicError::InvalidInput {
                field: "credential".into(),
                message: "private key must be between 1 byte and 64 KiB".into(),
            });
        }
        let path = self.state_dir.join("credentials").join(name);
        let before = path.exists().then(|| json!({"configured": true}));
        let result = write_atomic(&path, &bytes, 0o600)?;
        self.audit.append(&AuditRecord::now(
            context,
            "application.credential.import",
            "import_ssh_credential",
            "credential",
            name,
            json!({"source": "redacted", "kind": "ssh_private_key"}),
            before,
            Some(json!({"configured": true})),
            true,
            if result.changed {
                "credential imported"
            } else {
                "credential unchanged"
            },
        ))?;
        self.emit(
            "credential.imported",
            name,
            context,
            json!({"kind": "ssh_private_key"}),
        )?;
        Ok(name.into())
    }

    pub fn set_health_check(
        &self,
        id: &str,
        path: &str,
        port: u16,
        context: &OperationContext,
    ) -> Result<Application> {
        if !path.starts_with('/') || path.contains(['\n', '\r']) || port == 0 {
            return Err(LumicError::InvalidInput {
                field: "health".into(),
                message: "path must start with '/' and port must be non-zero".into(),
            });
        }
        let mut state = self.store.load()?;
        let application = state
            .applications
            .iter_mut()
            .find(|app| app.id == id)
            .ok_or_else(|| not_found(id))?;
        let before = serde_json::to_value(&application.health_check).ok();
        application.health_check.enabled = true;
        application.health_check.path = path.into();
        application.health_check.port = port;
        application.updated_at_unix_ms = unix_time_ms();
        let application = application.clone();
        self.store.save(&state)?;
        self.audit.append(&AuditRecord::now(
            context,
            "application.configure",
            "set_health_check",
            "application",
            id,
            json!({"path": path, "port": port}),
            before,
            serde_json::to_value(&application.health_check).ok(),
            true,
            "health check configured",
        ))?;
        Ok(application)
    }

    pub async fn provision(
        &self,
        id: &str,
        components: &[String],
        context: &OperationContext,
    ) -> Result<ProvisionResult> {
        let application = self.inspect(id)?;
        let runtime = RuntimeManager::at_state_dir(&self.state_dir)
            .install(application.runtime, components, context)
            .await
            .inspect_err(|error| {
                let _ = self.audit_failure(
                    context,
                    "application.provision",
                    "provision",
                    id,
                    json!({"runtime": application.runtime, "components": components}),
                    error,
                );
            })?;
        let web = NginxManager::system(&self.state_dir)
            .configure(&application, context)
            .await
            .inspect_err(|error| {
                let _ = self.audit_failure(
                    context,
                    "application.provision",
                    "provision",
                    id,
                    json!({"runtime": application.runtime, "components": components}),
                    error,
                );
            })?;
        self.update_application(id, |application| application.web_configured = true)?;
        self.audit.append(&AuditRecord::now(
            context,
            "application.provision",
            "provision",
            "application",
            id,
            json!({"runtime": application.runtime, "components": components}),
            Some(json!({"web_configured": application.web_configured})),
            Some(json!({"web_configured": true})),
            true,
            "runtime and nginx configured",
        ))?;
        self.emit(
            "application.provisioned",
            id,
            context,
            json!({"runtime": application.runtime}),
        )?;
        Ok(ProvisionResult { runtime, web })
    }

    pub async fn enable_tls(
        &self,
        id: &str,
        email: &str,
        context: &OperationContext,
    ) -> Result<Application> {
        let application = self.inspect(id)?;
        let packages =
            crate::apt::AptPackageManager::system(EventStore::at_state_dir(&self.state_dir));
        for name in ["certbot", "python3-certbot-nginx"] {
            packages
                .install(&lumic_core::package::PackageName::parse(name)?, context)
                .await?;
        }
        TlsManager::enable(&application, email)
            .await
            .inspect_err(|error| {
                let _ = self.audit_failure(
                    context,
                    "application.tls.enable",
                    "enable_tls",
                    id,
                    json!({"email": "redacted"}),
                    error,
                );
            })?;
        let application = self.update_application(id, |app| {
            app.tls.enabled = true;
            app.tls.certificate_name = Some(app.domain.clone());
        })?;
        self.audit.append(&AuditRecord::now(
            context,
            "application.tls.enable",
            "enable_tls",
            "application",
            id,
            json!({"email": "redacted", "domains": if application.www_alias { 2 } else { 1 }}),
            Some(json!({"enabled": false})),
            Some(json!({"enabled": true})),
            true,
            "certificate issued and nginx redirect enabled",
        ))?;
        self.emit(
            "certificate.issued",
            id,
            context,
            json!({"domain": application.domain}),
        )?;
        Ok(application)
    }

    pub async fn add_process(
        &self,
        id: &str,
        process: ApplicationProcess,
        context: &OperationContext,
    ) -> Result<ProcessConfigurationResult> {
        validate_slug("process", &process.name)?;
        validate_command(&process.command)?;
        let application = self.inspect(id)?;
        let result = ApplicationProcessManager::system(&self.state_dir)
            .configure(&application, &process, context)
            .await
            .inspect_err(|error| {
                let _ = self.audit_failure(
                    context,
                    "application.process.configure",
                    "configure_process",
                    id,
                    json!({"name": process.name, "kind": process.kind, "command": process.command}),
                    error,
                );
            })?;
        self.update_application(id, |application| {
            if let Some(existing) = application
                .processes
                .iter_mut()
                .find(|item| item.name == process.name)
            {
                *existing = process.clone();
            } else {
                application.processes.push(process.clone());
            }
        })?;
        self.audit.append(&AuditRecord::now(
            context,
            "application.process.configure",
            "configure_process",
            "application",
            id,
            json!({"name": process.name, "kind": process.kind, "command": process.command}),
            None,
            Some(serde_json::to_value(&process).unwrap_or_default()),
            true,
            "systemd process units configured",
        ))?;
        self.emit(
            "application.process_configured",
            id,
            context,
            json!({"process": process.name}),
        )?;
        Ok(result)
    }

    pub fn delete(&self, id: &str, context: &OperationContext) -> Result<()> {
        validate_slug("application", id)?;
        let mut state = self.store.load()?;
        let index = state
            .applications
            .iter()
            .position(|application| application.id == id)
            .ok_or_else(|| not_found(id))?;
        let application = state.applications.remove(index);
        state
            .deployments
            .retain(|deployment| deployment.application_id != id);
        let root = PathBuf::from(&application.root);
        if root.exists() {
            let trash = self.apps_root.join(".trash");
            fs::create_dir_all(&trash).map_err(state_io_error)?;
            fs::rename(&root, trash.join(format!("{id}-{}", unix_time_ms())))
                .map_err(state_io_error)?;
        }
        self.store.save(&state)?;
        self.emit(
            "application.deleted",
            id,
            context,
            json!({"recoverable_from_trash": true}),
        )?;
        self.audit.append(&AuditRecord::now(
            context,
            "application.delete",
            "delete",
            "application",
            id,
            json!({}),
            serde_json::to_value(&application).ok(),
            Some(json!({"recoverable_from_trash": true})),
            true,
            "application moved to trash",
        ))
    }

    pub fn deployments(&self, id: &str) -> Result<Vec<Deployment>> {
        self.inspect(id)?;
        let mut deployments: Vec<_> = self
            .store
            .load()?
            .deployments
            .into_iter()
            .filter(|deployment| deployment.application_id == id)
            .collect();
        deployments.reverse();
        Ok(deployments)
    }

    pub fn plan_deployment(&self, id: &str) -> Result<Plan> {
        let application = self.inspect(id)?;
        let repository =
            application
                .repository
                .as_ref()
                .ok_or_else(|| LumicError::InvalidInput {
                    field: "repository".into(),
                    message: "configure a repository before planning a deployment".into(),
                })?;
        let current = current_release(&application)?;
        let mut risks = vec![Risk {
            level: RiskLevel::Low,
            summary: "the current release symlink will be replaced atomically".into(),
            mitigation: Some("Lumic retains previous releases for immediate rollback".into()),
        }];
        if !application.health_check.enabled {
            risks.push(Risk {
                level: RiskLevel::Medium,
                summary: "automatic rollback is unavailable because health checks are disabled"
                    .into(),
                mitigation: Some("configure an HTTP health check before applying this plan".into()),
            });
        }
        Ok(Plan {
            id: format!("deploy-{id}"),
            summary: format!("Deploy the latest {} revision for {id}", repository.branch),
            changes: vec![Change {
                capability: Capability::new("application.deploy"),
                summary:
                    "create an isolated release, run the runtime build, and atomically activate it"
                        .into(),
                before: current,
                after: Some(format!("latest {} revision", repository.branch)),
                reversible: true,
            }],
            risks,
            preconditions: vec![
                format!("repository {} is reachable", repository.url),
                "runtime build tools are installed".into(),
                "the application root has sufficient free space".into(),
            ],
            validation: vec![
                "runtime entry point exists".into(),
                if application.health_check.enabled {
                    format!(
                        "HTTP {}:{}{} returns a successful status",
                        application.domain,
                        application.health_check.port,
                        application.health_check.path
                    )
                } else {
                    "release activation completes (health check disabled)".into()
                },
            ],
            recovery: vec![
                "restore the previous current-release symlink automatically on failed health"
                    .into(),
                format!("run `lumic app rollback {id}` for an explicit rollback"),
            ],
        })
    }

    pub async fn deploy(&self, id: &str, context: &OperationContext) -> Result<Deployment> {
        let application = self.inspect(id)?;
        let repository =
            application
                .repository
                .clone()
                .ok_or_else(|| LumicError::InvalidInput {
                    field: "repository".into(),
                    message: "configure a repository before deployment".into(),
                })?;
        let deployment_id = format!("{}-{}", unix_time_ms(), std::process::id());
        let release = PathBuf::from(&application.root)
            .join("releases")
            .join(&deployment_id);
        let previous_release = current_release(&application)?;
        let mut deployment = Deployment {
            id: deployment_id,
            application_id: id.into(),
            release_path: release.to_string_lossy().into_owned(),
            commit: String::new(),
            status: DeploymentStatus::Started,
            healthy: false,
            message: "preparing release".into(),
            previous_release,
            phases: Vec::new(),
            automatic_rollback: false,
            started_at_unix_ms: unix_time_ms(),
            finished_at_unix_ms: None,
        };
        self.upsert_deployment(&deployment)?;
        self.emit(
            "deployment.started",
            id,
            context,
            json!({"deployment_id": deployment.id}),
        )?;

        match self
            .prepare_release(&application, &repository, &release, &mut deployment)
            .await
        {
            Ok(commit) => {
                deployment.commit = commit;
                match self.verify_health(&application).await {
                    Ok(message) => {
                        deployment.phases.push(phase(
                            "health",
                            DeploymentPhaseStatus::Completed,
                            message,
                        ));
                        deployment.healthy = true;
                        deployment.status = DeploymentStatus::Completed;
                        deployment.message =
                            "release activated and health validation succeeded".into();
                        deployment.finished_at_unix_ms = Some(unix_time_ms());
                        self.upsert_deployment(&deployment)?;
                        self.set_health(id, "healthy")?;
                        self.audit_deployment(&deployment, context, true)?;
                        self.emit(
                            "deployment.succeeded",
                            id,
                            context,
                            json!({"deployment_id": deployment.id, "commit": deployment.commit}),
                        )?;
                        self.prune_releases(&application)?;
                        Ok(deployment)
                    }
                    Err(error) => {
                        deployment.phases.push(phase(
                            "health",
                            DeploymentPhaseStatus::Failed,
                            error.to_string(),
                        ));
                        let rolled_back = if let Some(previous) = &deployment.previous_release {
                            if Path::new(previous).is_dir() {
                                activate(&application, Path::new(previous), "automatic-rollback")?;
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        };
                        deployment.automatic_rollback = rolled_back;
                        deployment.status = if rolled_back {
                            DeploymentStatus::FailedRolledBack
                        } else {
                            DeploymentStatus::Failed
                        };
                        deployment.message = if rolled_back {
                            format!("health check failed; previous release restored: {error}")
                        } else {
                            format!(
                                "health check failed and no previous release was available: {error}"
                            )
                        };
                        deployment.finished_at_unix_ms = Some(unix_time_ms());
                        self.upsert_deployment(&deployment)?;
                        self.set_health(
                            id,
                            if rolled_back {
                                "healthy_after_rollback"
                            } else {
                                "unhealthy"
                            },
                        )?;
                        self.audit_deployment(&deployment, context, false)?;
                        self.emit(
                            if rolled_back { "deployment.rolled_back" } else { "deployment.failed" },
                            id,
                            context,
                            json!({"deployment_id": deployment.id, "automatic": rolled_back, "reason": deployment.message}),
                        )?;
                        if !rolled_back {
                            let current = PathBuf::from(&application.root).join("current");
                            if current.symlink_metadata().is_ok() {
                                fs::remove_file(&current).map_err(state_io_error)?;
                            }
                        }
                        if release.exists() {
                            fs::remove_dir_all(&release).map_err(state_io_error)?;
                        }
                        Err(error)
                    }
                }
            }
            Err(error) => {
                if release.exists() {
                    fs::remove_dir_all(&release).map_err(state_io_error)?;
                }
                deployment.status = DeploymentStatus::Failed;
                deployment.phases.push(phase(
                    "deployment",
                    DeploymentPhaseStatus::Failed,
                    error.to_string(),
                ));
                deployment.message = error.to_string();
                deployment.finished_at_unix_ms = Some(unix_time_ms());
                self.upsert_deployment(&deployment)?;
                self.set_health(id, "deployment_failed")?;
                self.audit_deployment(&deployment, context, false)?;
                self.emit(
                    "deployment.failed",
                    id,
                    context,
                    json!({
                        "deployment_id": deployment.id,
                        "reason": deployment.message,
                    }),
                )?;
                Err(error)
            }
        }
    }

    pub fn rollback(&self, id: &str, context: &OperationContext) -> Result<Deployment> {
        let application = self.inspect(id)?;
        let current = current_release(&application);
        let current = current?;
        let state = self.store.load()?;
        let target = state
            .deployments
            .iter()
            .rev()
            .find(|deployment| {
                deployment.application_id == id
                    && deployment.status == DeploymentStatus::Completed
                    && Some(&deployment.release_path) != current.as_ref()
                    && Path::new(&deployment.release_path).exists()
            })
            .cloned()
            .ok_or_else(|| LumicError::InvalidInput {
                field: "deployment".into(),
                message: "no previous known-good release is available".into(),
            })?;
        activate(&application, Path::new(&target.release_path), &target.id)?;
        let mut rollback = target;
        rollback.id = format!("rollback-{}-{}", unix_time_ms(), std::process::id());
        rollback.status = DeploymentStatus::RolledBack;
        rollback.previous_release = current;
        rollback.started_at_unix_ms = unix_time_ms();
        rollback.finished_at_unix_ms = Some(unix_time_ms());
        rollback.message = "previous known-good release activated".into();
        rollback.automatic_rollback = false;
        rollback.phases = vec![phase(
            "rollback",
            DeploymentPhaseStatus::Completed,
            "previous known-good release activated",
        )];
        self.upsert_deployment(&rollback)?;
        self.set_health(id, "healthy")?;
        self.audit_deployment(&rollback, context, true)?;
        self.emit(
            "deployment.rolled_back",
            id,
            context,
            json!({
                "deployment_id": rollback.id,
                "release": rollback.release_path,
            }),
        )?;
        Ok(rollback)
    }

    async fn prepare_release(
        &self,
        application: &Application,
        repository: &RepositoryConfig,
        release: &Path,
        deployment: &mut Deployment,
    ) -> Result<String> {
        let repository_path = PathBuf::from(&application.root).join("repository/source.git");
        let credential = self.credential_path(repository.credential_reference.as_deref())?;
        if repository_path.exists() {
            self.run_git(
                [
                    "--git-dir",
                    path_text(&repository_path)?,
                    "remote",
                    "set-url",
                    "origin",
                    &repository.url,
                ],
                credential.as_deref(),
            )
            .await?;
            self.run_git(
                [
                    "--git-dir",
                    path_text(&repository_path)?,
                    "fetch",
                    "--prune",
                    "origin",
                ],
                credential.as_deref(),
            )
            .await?;
        } else {
            self.run_git(
                [
                    "clone",
                    "--mirror",
                    "--",
                    &repository.url,
                    path_text(&repository_path)?,
                ],
                credential.as_deref(),
            )
            .await?;
        }
        deployment.phases.push(phase(
            "source",
            DeploymentPhaseStatus::Completed,
            "Git mirror fetched",
        ));
        self.upsert_deployment(deployment)?;
        let reference = format!("refs/heads/{}", repository.branch);
        let commit_output = self
            .run_git(
                [
                    "--git-dir",
                    path_text(&repository_path)?,
                    "rev-parse",
                    "--verify",
                    &reference,
                ],
                credential.as_deref(),
            )
            .await?;
        let commit = String::from_utf8_lossy(&commit_output.stdout)
            .trim()
            .to_owned();
        self.run_git(
            [
                "clone",
                "--quiet",
                "--no-checkout",
                "--",
                path_text(&repository_path)?,
                path_text(release)?,
            ],
            credential.as_deref(),
        )
        .await?;
        self.run_git(
            [
                "-C",
                path_text(release)?,
                "checkout",
                "--quiet",
                "--detach",
                &commit,
            ],
            credential.as_deref(),
        )
        .await?;
        deployment.phases.push(phase(
            "checkout",
            DeploymentPhaseStatus::Completed,
            format!("checked out {commit}"),
        ));
        self.upsert_deployment(deployment)?;

        match application.runtime {
            ApplicationRuntime::Php if release.join("composer.json").exists() => {
                let mut spec = ProcessSpec::new("composer")
                    .args([
                        "install",
                        "--no-dev",
                        "--no-interaction",
                        "--prefer-dist",
                        "--optimize-autoloader",
                    ])
                    .current_dir(release);
                spec.timeout = Duration::from_secs(600);
                self.run(spec).await?;
                deployment.phases.push(phase(
                    "build",
                    DeploymentPhaseStatus::Completed,
                    "Composer dependencies installed",
                ));
            }
            ApplicationRuntime::Node if release.join("package-lock.json").exists() => {
                let mut spec = ProcessSpec::new("npm")
                    .args(["ci", "--omit=dev"])
                    .current_dir(release);
                spec.timeout = Duration::from_secs(600);
                self.run(spec).await?;
                deployment.phases.push(phase(
                    "build",
                    DeploymentPhaseStatus::Completed,
                    "npm dependencies installed",
                ));
            }
            _ => deployment.phases.push(phase(
                "build",
                DeploymentPhaseStatus::Skipped,
                "runtime has no dependency build step",
            )),
        }
        let entry_point = match application.runtime {
            ApplicationRuntime::Static => release.join("index.html"),
            ApplicationRuntime::Php => release.join("index.php"),
            ApplicationRuntime::Node => release.join("package.json"),
        };
        if !entry_point.is_file() {
            return Err(LumicError::InvalidInput {
                field: "health".into(),
                message: format!("required entry point {} is missing", entry_point.display()),
            });
        }
        deployment.phases.push(phase(
            "pre_activation",
            DeploymentPhaseStatus::Completed,
            format!("validated {}", entry_point.display()),
        ));
        activate(
            application,
            release,
            &release.file_name().unwrap_or_default().to_string_lossy(),
        )?;
        deployment.phases.push(phase(
            "activation",
            DeploymentPhaseStatus::Completed,
            "current symlink switched atomically",
        ));
        self.upsert_deployment(deployment)?;
        Ok(commit)
    }

    async fn run_git<const N: usize>(
        &self,
        args: [&str; N],
        credential: Option<&Path>,
    ) -> Result<ProcessOutput> {
        let mut spec = ProcessSpec::new("git").args(args);
        if let Some(credential) = credential {
            spec = spec.environment(
                "GIT_SSH_COMMAND",
                format!(
                    "ssh -i {} -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new",
                    credential.display()
                ),
            );
        }
        spec.timeout = Duration::from_secs(300);
        self.run(spec).await
    }

    fn credential_path(&self, reference: Option<&str>) -> Result<Option<PathBuf>> {
        let Some(reference) = reference else {
            return Ok(None);
        };
        validate_slug("credential", reference)?;
        let path = self.state_dir.join("credentials").join(reference);
        if !path.is_file() {
            return Err(LumicError::InvalidInput {
                field: "credential".into(),
                message: format!("credential reference {reference} does not exist"),
            });
        }
        Ok(Some(path))
    }

    async fn run(&self, spec: ProcessSpec) -> Result<ProcessOutput> {
        let executable = spec.executable.clone();
        let output = self.runner.run(&spec).await?;
        if output.success() {
            Ok(output)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(LumicError::Process {
                executable,
                message: stderr.trim().to_owned(),
            })
        }
    }

    pub fn attach_service(
        &self,
        id: &str,
        reference: ApplicationServiceReference,
        context: &OperationContext,
    ) -> Result<Application> {
        validate_slug("application", id)?;
        lumic_core::managed_service::validate_resource_id("service", &reference.service_id)?;
        if reference.role.is_empty()
            || reference.role.len() > 64
            || !reference
                .role
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(LumicError::InvalidInput {
                field: "role".into(),
                message: "must be a lowercase role containing letters, digits, or underscores"
                    .into(),
            });
        }
        let service_id = reference.service_id.clone();
        let role = reference.role.clone();
        let application = self.update_application(id, |application| {
            if let Some(existing) = application
                .service_references
                .iter_mut()
                .find(|item| item.role == role)
            {
                *existing = reference.clone();
            } else {
                application.service_references.push(reference.clone());
            }
        })?;
        self.emit(
            "application.service_attached",
            id,
            context,
            json!({"service_id": service_id, "role": role}),
        )?;
        self.audit.append(&AuditRecord::now(
            context,
            "application.service.attach",
            "attach",
            "application",
            id,
            json!({"service_id": service_id, "role": role}),
            None,
            Some(json!({"attached": true})),
            true,
            "managed service attached to application",
        ))?;
        Ok(application)
    }

    pub fn set_environment_reference(
        &self,
        id: &str,
        name: &str,
        reference: &str,
        context: &OperationContext,
    ) -> Result<Application> {
        lumic_core::recipe::validate_environment_name(name)?;
        lumic_core::managed_service::validate_resource_id("secret_reference", reference)?;
        if !SecretStore::at_state_dir(&self.state_dir).exists(reference)? {
            return Err(LumicError::InvalidInput {
                field: "secret_reference".into(),
                message: "must identify an existing target-local secret".into(),
            });
        }
        let application = self.update_application(id, |application| {
            application
                .environment_references
                .insert(name.to_owned(), reference.to_owned());
        })?;
        self.emit(
            "application.environment_reference_set",
            id,
            context,
            json!({"name": name, "secret_reference": reference}),
        )?;
        self.audit.append(&AuditRecord::now(
            context,
            "application.environment.configure",
            "set_reference",
            "application",
            id,
            json!({"name": name, "secret_reference": reference}),
            None,
            Some(json!({"configured": true})),
            true,
            "application environment secret reference configured",
        ))?;
        Ok(application)
    }

    pub fn apply_portable_configuration(
        &self,
        id: &str,
        configuration: &PortableApplication,
        context: &OperationContext,
    ) -> Result<Application> {
        validate_slug("application", id)?;
        validate_domain(&configuration.domain)?;
        if configuration.release_retention == 0 || configuration.release_retention > 100 {
            return Err(LumicError::InvalidInput {
                field: "release_retention".into(),
                message: "must be between 1 and 100".into(),
            });
        }
        if let Some(repository) = &configuration.repository {
            validate_repository_url(&repository.url)?;
            validate_branch(&repository.branch)?;
            if let Some(reference) = &repository.credential_reference {
                lumic_core::managed_service::validate_resource_id(
                    "credential_reference",
                    reference,
                )?;
            }
        }
        for (name, reference) in &configuration.environment_references {
            lumic_core::recipe::validate_environment_name(name)?;
            lumic_core::managed_service::validate_resource_id("secret_reference", reference)?;
        }
        for reference in &configuration.service_references {
            lumic_core::managed_service::validate_resource_id("service", &reference.service_id)?;
            if let Some(database) = &reference.database {
                lumic_core::managed_service::validate_resource_id("database", database)?;
            }
            if let Some(user) = &reference.user {
                lumic_core::managed_service::validate_resource_id("user", user)?;
            }
            if let Some(secret_reference) = &reference.secret_reference {
                lumic_core::managed_service::validate_resource_id(
                    "secret_reference",
                    secret_reference,
                )?;
            }
            if reference.role.is_empty()
                || reference.role.len() > 64
                || !reference
                    .role
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
            {
                return Err(LumicError::InvalidInput {
                    field: "role".into(),
                    message: "must be a lowercase role containing letters, digits, or underscores"
                        .into(),
                });
            }
        }
        for process in &configuration.processes {
            validate_slug("process", &process.name)?;
            validate_command(&process.command)?;
            if process.kind == lumic_core::application::ApplicationProcessKind::Schedule
                && !process.schedule.as_deref().is_some_and(|schedule| {
                    !schedule.is_empty()
                        && schedule.len() <= 128
                        && !schedule.contains(['\n', '\r', '\0'])
                })
            {
                return Err(LumicError::InvalidInput {
                    field: "schedule".into(),
                    message: "scheduled processes require a safe systemd OnCalendar expression"
                        .into(),
                });
            }
            if process.kind == lumic_core::application::ApplicationProcessKind::Worker
                && process.schedule.is_some()
            {
                return Err(LumicError::InvalidInput {
                    field: "schedule".into(),
                    message: "worker processes cannot have a schedule".into(),
                });
            }
        }
        if configuration.health_check.enabled
            && (!configuration.health_check.path.starts_with('/')
                || configuration.health_check.path.contains(['\n', '\r'])
                || configuration.health_check.port == 0)
        {
            return Err(LumicError::InvalidInput {
                field: "health".into(),
                message: "path must start with '/' and port must be non-zero".into(),
            });
        }

        let before = self.inspect(id)?;
        if self
            .list()?
            .iter()
            .any(|application| application.id != id && application.domain == configuration.domain)
        {
            return Err(LumicError::InvalidInput {
                field: "domain".into(),
                message: format!(
                    "{} is already assigned to another application",
                    configuration.domain
                ),
            });
        }
        let application = self.update_application(id, |application| {
            application.name = configuration.name.clone();
            application.domain = configuration.domain.clone();
            application.www_alias = configuration.www_alias;
            application.runtime = configuration.runtime;
            application.repository = configuration.repository.clone();
            application.environment_references = configuration.environment_references.clone();
            application.service_references = configuration.service_references.clone();
            application.health_check = configuration.health_check.clone();
            application.processes = configuration.processes.clone();
            application.release_retention = configuration.release_retention;
        })?;
        self.audit.append(&AuditRecord::now(
            context,
            "application.environment.import",
            "apply_portable_configuration",
            "application",
            id,
            json!({"source_application": configuration.id, "secret_values": "not_exported"}),
            serde_json::to_value(before).ok(),
            serde_json::to_value(&application).ok(),
            true,
            "portable application configuration applied",
        ))?;
        self.emit(
            "application.environment_imported",
            id,
            context,
            json!({"source_application": configuration.id}),
        )?;
        Ok(application)
    }

    fn update_application(
        &self,
        id: &str,
        update: impl FnOnce(&mut Application),
    ) -> Result<Application> {
        let mut state = self.store.load()?;
        let application = state
            .applications
            .iter_mut()
            .find(|application| application.id == id)
            .ok_or_else(|| not_found(id))?;
        update(application);
        application.updated_at_unix_ms = unix_time_ms();
        let application = application.clone();
        self.store.save(&state)?;
        Ok(application)
    }

    fn upsert_deployment(&self, deployment: &Deployment) -> Result<()> {
        let mut state = self.store.load()?;
        if let Some(existing) = state
            .deployments
            .iter_mut()
            .find(|item| item.id == deployment.id)
        {
            *existing = deployment.clone();
        } else {
            state.deployments.push(deployment.clone());
        }
        self.store.save(&state)
    }

    fn set_health(&self, id: &str, health: &str) -> Result<()> {
        let mut state = self.store.load()?;
        let application = state
            .applications
            .iter_mut()
            .find(|application| application.id == id)
            .ok_or_else(|| not_found(id))?;
        application.health_status = health.into();
        application.updated_at_unix_ms = unix_time_ms();
        self.store.save(&state)
    }

    fn prune_releases(&self, application: &Application) -> Result<()> {
        let current = current_release(application)?;
        let mut successful: Vec<_> = self
            .store
            .load()?
            .deployments
            .into_iter()
            .filter(|item| {
                item.application_id == application.id && item.status == DeploymentStatus::Completed
            })
            .collect();
        successful.reverse();
        let releases_root = PathBuf::from(&application.root).join("releases");
        for deployment in successful.into_iter().skip(application.release_retention) {
            let path = PathBuf::from(deployment.release_path);
            if Some(path.to_string_lossy().as_ref()) == current.as_deref() {
                continue;
            }
            if path.parent() == Some(releases_root.as_path()) && path.is_dir() {
                fs::remove_dir_all(path).map_err(state_io_error)?;
            }
        }
        Ok(())
    }

    async fn verify_health(&self, application: &Application) -> Result<String> {
        let health = &application.health_check;
        if !health.enabled {
            return Ok("HTTP health check disabled; entry point validation passed".into());
        }
        let duration = Duration::from_secs(health.timeout_seconds.max(1));
        let check = async {
            let mut stream = TcpStream::connect(("127.0.0.1", health.port))
                .await
                .map_err(|error| LumicError::Inspection {
                    fact: "application_health".into(),
                    message: error.to_string(),
                })?;
            let request = format!(
                "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nUser-Agent: lumic-health\r\n\r\n",
                health.path, application.domain
            );
            stream
                .write_all(request.as_bytes())
                .await
                .map_err(|error| LumicError::Inspection {
                    fact: "application_health".into(),
                    message: error.to_string(),
                })?;
            let mut response = vec![0_u8; 4096];
            let read =
                stream
                    .read(&mut response)
                    .await
                    .map_err(|error| LumicError::Inspection {
                        fact: "application_health".into(),
                        message: error.to_string(),
                    })?;
            let status = String::from_utf8_lossy(&response[..read])
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .and_then(|code| code.parse::<u16>().ok())
                .ok_or_else(|| LumicError::Inspection {
                    fact: "application_health".into(),
                    message: "invalid HTTP response".into(),
                })?;
            if (health.expected_status_min..=health.expected_status_max).contains(&status) {
                Ok(format!(
                    "HTTP {status} from localhost:{}{} with Host {}",
                    health.port, health.path, application.domain
                ))
            } else {
                Err(LumicError::Inspection {
                    fact: "application_health".into(),
                    message: format!(
                        "HTTP {status}, expected {}-{}",
                        health.expected_status_min, health.expected_status_max
                    ),
                })
            }
        };
        timeout(duration, check)
            .await
            .map_err(|_| LumicError::Timeout {
                executable: "application-health".into(),
                timeout_ms: duration.as_millis() as u64,
            })?
    }

    pub async fn verify_application_health(&self, id: &str) -> Result<String> {
        let application = self.inspect(id)?;
        self.verify_health(&application).await
    }

    fn audit_deployment(
        &self,
        deployment: &Deployment,
        context: &OperationContext,
        succeeded: bool,
    ) -> Result<()> {
        self.audit.append(&AuditRecord::now(
            context, "application.deploy", "deploy", "application", &deployment.application_id,
            json!({"deployment_id": deployment.id, "commit": deployment.commit}),
            deployment.previous_release.as_ref().map(|release| json!({"release": release})),
            Some(json!({"release": deployment.release_path, "status": deployment.status, "healthy": deployment.healthy})),
            succeeded, &deployment.message,
        ))
    }

    fn audit_failure(
        &self,
        context: &OperationContext,
        capability: &str,
        operation: &str,
        id: &str,
        arguments: serde_json::Value,
        error: &LumicError,
    ) -> Result<()> {
        self.audit.append(&AuditRecord::now(
            context,
            capability,
            operation,
            "application",
            id,
            arguments,
            None,
            None,
            false,
            error.to_string(),
        ))
    }

    fn emit(
        &self,
        event_type: &str,
        id: &str,
        context: &OperationContext,
        payload: serde_json::Value,
    ) -> Result<()> {
        self.events.append(&Event::now(
            event_type,
            &context.actor,
            context.interface,
            "application",
            id,
            &context.correlation_id,
            payload,
        ))
    }
}

fn phase(
    name: impl Into<String>,
    status: DeploymentPhaseStatus,
    message: impl Into<String>,
) -> DeploymentPhase {
    DeploymentPhase {
        name: name.into(),
        status,
        message: message.into(),
    }
}

fn current_release(application: &Application) -> Result<Option<String>> {
    let current = PathBuf::from(&application.root).join("current");
    match fs::read_link(current) {
        Ok(path) => Ok(Some(path.to_string_lossy().into_owned())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(state_io_error(error)),
    }
}

#[cfg(unix)]
fn activate(application: &Application, release: &Path, suffix: &str) -> Result<()> {
    let root = PathBuf::from(&application.root);
    let temporary = root.join(format!(".current-{suffix}"));
    if temporary.exists() || temporary.is_symlink() {
        fs::remove_file(&temporary).map_err(state_io_error)?;
    }
    symlink(release, &temporary).map_err(state_io_error)?;
    fs::rename(temporary, root.join("current")).map_err(state_io_error)
}

#[cfg(not(unix))]
fn activate(_application: &Application, _release: &Path, _suffix: &str) -> Result<()> {
    Err(LumicError::UnsupportedPlatform {
        platform: "atomic release symlinks require Unix".into(),
    })
}

fn path_text(path: &Path) -> Result<&str> {
    path.to_str().ok_or_else(|| LumicError::InvalidInput {
        field: "path".into(),
        message: "must be valid UTF-8".into(),
    })
}

fn state_io_error(error: std::io::Error) -> LumicError {
    LumicError::Internal {
        message: format!("application state I/O failed: {error}"),
    }
}

fn not_found(id: &str) -> LumicError {
    LumicError::InvalidInput {
        field: "application".into(),
        message: format!("application '{id}' does not exist"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumic_core::{OperationContext, OperationInterface};
    use std::process::Command;
    use tokio::net::TcpListener;

    fn context() -> OperationContext {
        OperationContext {
            actor: "test".into(),
            interface: OperationInterface::Internal,
            correlation_id: "application-test".into(),
            dry_run: false,
            approved: true,
        }
    }

    fn git(repository: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git command failed: {args:?}");
    }

    #[tokio::test]
    async fn static_releases_are_persistent_and_rollback_is_atomic() {
        let base = std::env::temp_dir().join(format!(
            "lumic-app-test-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        let state = base.join("state");
        let apps = base.join("apps");
        let source = base.join("source");
        fs::create_dir_all(&source).unwrap();
        git(&source, &["init", "--initial-branch=main"]);
        git(&source, &["config", "user.email", "test@lumic.invalid"]);
        git(&source, &["config", "user.name", "Lumic Test"]);
        fs::write(source.join("index.html"), "first").unwrap();
        git(&source, &["add", "index.html"]);
        git(&source, &["commit", "-m", "first"]);

        let service = ApplicationService::new(&state, &apps);
        service
            .create(
                "example",
                "example.com",
                ApplicationRuntime::Static,
                false,
                &context(),
            )
            .unwrap();
        service
            .set_repository(
                "example",
                &format!("file://{}", source.display()),
                "main",
                None,
                &context(),
            )
            .unwrap();
        let first = service.deploy("example", &context()).await.unwrap();

        fs::write(source.join("index.html"), "second").unwrap();
        git(&source, &["add", "index.html"]);
        git(&source, &["commit", "-m", "second"]);
        let second = service.deploy("example", &context()).await.unwrap();
        assert_ne!(first.commit, second.commit);
        assert_eq!(
            fs::read_to_string(apps.join("example/current/index.html")).unwrap(),
            "second"
        );

        service.rollback("example", &context()).unwrap();
        assert_eq!(
            fs::read_to_string(apps.join("example/current/index.html")).unwrap(),
            "first"
        );
        let reloaded = ApplicationService::new(&state, &apps);
        assert_eq!(reloaded.list().unwrap().len(), 1);
        assert_eq!(reloaded.deployments("example").unwrap().len(), 3);
        fs::remove_dir_all(base).unwrap();
    }

    #[tokio::test]
    async fn failed_health_check_automatically_restores_previous_release() {
        let base = std::env::temp_dir().join(format!(
            "lumic-health-test-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        let source = base.join("source");
        fs::create_dir_all(&source).unwrap();
        git(&source, &["init", "--initial-branch=main"]);
        git(&source, &["config", "user.email", "test@lumic.invalid"]);
        git(&source, &["config", "user.name", "Lumic Test"]);
        fs::write(source.join("index.html"), "known-good").unwrap();
        git(&source, &["add", "index.html"]);
        git(&source, &["commit", "-m", "known-good"]);

        let service = ApplicationService::new(base.join("state"), base.join("apps"));
        service
            .create(
                "health-demo",
                "health.example.com",
                ApplicationRuntime::Static,
                false,
                &context(),
            )
            .unwrap();
        service
            .set_repository(
                "health-demo",
                &format!("file://{}", source.display()),
                "main",
                None,
                &context(),
            )
            .unwrap();
        service.deploy("health-demo", &context()).await.unwrap();

        fs::write(source.join("index.html"), "unhealthy").unwrap();
        git(&source, &["add", "index.html"]);
        git(&source, &["commit", "-m", "unhealthy"]);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        service
            .set_health_check("health-demo", "/health", port, &context())
            .unwrap();
        let responder = tokio::spawn(async move {
            let (mut connection, _) = listener.accept().await.unwrap();
            connection
                .write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n")
                .await
                .unwrap();
        });

        assert!(service.deploy("health-demo", &context()).await.is_err());
        responder.await.unwrap();
        assert_eq!(
            fs::read_to_string(base.join("apps/health-demo/current/index.html")).unwrap(),
            "known-good"
        );
        let deployments = service.deployments("health-demo").unwrap();
        assert_eq!(deployments[0].status, DeploymentStatus::FailedRolledBack);
        assert!(deployments[0].automatic_rollback);
        fs::remove_dir_all(base).unwrap();
    }

    #[tokio::test]
    async fn generic_php_repository_uses_the_same_release_mechanism() {
        let base = std::env::temp_dir().join(format!(
            "lumic-php-test-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        let source = base.join("source");
        fs::create_dir_all(&source).unwrap();
        git(&source, &["init", "--initial-branch=main"]);
        git(&source, &["config", "user.email", "test@lumic.invalid"]);
        git(&source, &["config", "user.name", "Lumic Test"]);
        fs::write(source.join("index.php"), "<?php echo 'Lumic';").unwrap();
        git(&source, &["add", "index.php"]);
        git(&source, &["commit", "-m", "php"]);

        let service = ApplicationService::new(base.join("state"), base.join("apps"));
        service
            .create(
                "php-demo",
                "php.example.com",
                ApplicationRuntime::Php,
                false,
                &context(),
            )
            .unwrap();
        service
            .set_repository(
                "php-demo",
                &format!("file://{}", source.display()),
                "main",
                None,
                &context(),
            )
            .unwrap();
        let deployment = service.deploy("php-demo", &context()).await.unwrap();
        assert_eq!(deployment.status, DeploymentStatus::Completed);
        assert!(base.join("apps/php-demo/current/index.php").is_file());
        fs::remove_dir_all(base).unwrap();
    }
}
