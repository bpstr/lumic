use crate::{
    ProcessOutput, ProcessRunner, ProcessSpec,
    app_process::{ApplicationProcessManager, NodeReleaseStart, ProcessConfigurationResult},
    atomic_file::write_atomic,
    audit_store::AuditStore,
    certificate::{CertbotProvider, CertificateManager, NginxCertificateAttacher},
    event_store::EventStore,
    framework_state::FrameworkStateStore,
    resource_lock::ResourceLock,
    runtime::{RuntimeInstallResult, RuntimeManager},
    secret_store::SecretStore,
    web::{NginxManager, WebConfigurationResult},
};
use lumic_core::{
    Capability, Change, LumicError, OperationContext, Plan, Result, Risk, RiskLevel,
    application::{
        Application, ApplicationProcess, ApplicationRuntime, ApplicationRuntimeIntent,
        ApplicationServiceReference, CommitMetadata, Deployment, DeploymentLogEntry,
        DeploymentLogStream, DeploymentPhase, DeploymentPhaseStatus, DeploymentStatus,
        DeploymentWorkflow, NodePackageManager, RepositoryConfig, TlsState, unix_time_ms,
        validate_branch, validate_command, validate_domain, validate_repository_url, validate_slug,
    },
    application_lifecycle::{
        ApplicationLifecycleOperation, ApplicationLifecyclePlan, GenericPhpApplicationSpec,
    },
    application_manifest::{
        APPLICATION_MANIFEST_FILE, ApplicationManifest, ResolvedApplicationManifest,
    },
    binding::Binding,
    certificate::CertificateRequest,
    events::{AuditRecord, Event},
    infrastructure::PortableApplication,
    pipeline::{PipelineExecution, PipelineStatus},
    resource::{ResourceKind, ResourceOutput, ResourceOutputs, ResourceRecord, ResourceRef},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, symlink};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::{sleep, timeout},
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

struct ReleasePreparation<'a> {
    pinned_commit: Option<&'a str>,
    previous_node_port: Option<u16>,
    release: &'a Path,
    context: &'a OperationContext,
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

    /// Builds an explicit desired-state plan without changing host or framework state.
    pub fn plan_generic_php(
        &self,
        spec: &GenericPhpApplicationSpec,
        operation: ApplicationLifecycleOperation,
    ) -> Result<ApplicationLifecyclePlan> {
        spec.validate()?;
        let expected_root = self.apps_root.join(&spec.id);
        if Path::new(&spec.root) != expected_root {
            return Err(LumicError::InvalidInput {
                field: "root".into(),
                message: format!(
                    "generic applications must use the managed root {}",
                    expected_root.display()
                ),
            });
        }
        let existing = self
            .store
            .load()?
            .applications
            .into_iter()
            .find(|application| application.id == spec.id);
        match (operation, existing.as_ref()) {
            (ApplicationLifecycleOperation::Install, Some(_)) => {
                return Err(LumicError::InvalidInput {
                    field: "application".into(),
                    message: "install requires the application to be absent; use reconcile".into(),
                });
            }
            (ApplicationLifecycleOperation::Reconcile, None)
            | (ApplicationLifecycleOperation::Update, None)
            | (ApplicationLifecycleOperation::Remove, None) => return Err(not_found(&spec.id)),
            _ => {}
        }
        if let Some(application) = existing
            && (application.runtime != ApplicationRuntime::Php
                || application.domain != spec.domain
                || application.www_alias != spec.www_alias
                || application.root != spec.root)
        {
            return Err(LumicError::InvalidInput {
                field: "application".into(),
                message: "desired identity, domain, root, and PHP runtime must match the managed application"
                    .into(),
            });
        }
        spec.lifecycle_plan(operation)
    }

    /// Persists the planned/running execution journal at the application apply boundary.
    pub fn begin_generic_php_lifecycle(
        &self,
        lifecycle: &ApplicationLifecyclePlan,
        execution_id: &str,
    ) -> Result<PipelineExecution> {
        let _lock = ResourceLock::try_acquire(&self.state_dir, &lifecycle.pipeline.target)?;
        let now = framework_time();
        let mut execution = PipelineExecution::planned(execution_id, &lifecycle.pipeline, now)?;
        execution.transition(PipelineStatus::Running, now)?;
        let store = FrameworkStateStore::at_state_dir(&self.state_dir);
        let mut state = store.load_or_migrate(now)?;
        if state
            .pipeline_executions
            .iter()
            .any(|existing| existing.id == execution.id)
        {
            return Err(LumicError::InvalidInput {
                field: "execution.id".into(),
                message: "lifecycle execution id already exists".into(),
            });
        }
        state.pipeline_executions.push(execution.clone());
        store.save(&state)?;
        Ok(execution)
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
        let root_preexisted = root.exists();
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
            runtime_intent: ApplicationRuntimeIntent::default(),
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
        let previous_state = state.clone();
        state.applications.push(application.clone());
        self.store.save(&state)?;
        if let Err(error) = persist_application_resource(&self.state_dir, &application) {
            let _ = self.store.save(&previous_state);
            if !root_preexisted {
                let _ = fs::remove_dir_all(&root);
            }
            return Err(error);
        }
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
            deployment: Default::default(),
            contract: None,
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
        self.provision_versioned(id, None, components, context)
            .await
    }

    pub async fn provision_versioned(
        &self,
        id: &str,
        runtime_version: Option<&str>,
        components: &[String],
        context: &OperationContext,
    ) -> Result<ProvisionResult> {
        let application = self.inspect(id)?;
        let runtime_manager = RuntimeManager::at_state_dir(&self.state_dir);
        runtime_manager.validate_request(application.runtime, runtime_version, components)?;
        let nginx = NginxManager::system(&self.state_dir);
        nginx.ensure_service(context).await.inspect_err(|error| {
            let _ = self.audit_failure(
                context,
                "application.provision",
                "provision",
                id,
                json!({"service": "nginx"}),
                error,
            );
        })?;
        let runtime = runtime_manager
            .install_versioned(application.runtime, runtime_version, components, context)
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
        let web = nginx
            .configure(
                &application,
                runtime.fpm_socket.as_deref().map(Path::new),
                runtime.runtime_resource_id.as_deref(),
                context,
            )
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
            json!({
                "runtime": application.runtime,
                "runtime_version": runtime.runtime_version.as_deref(),
                "components": components,
            }),
            Some(json!({"web_configured": application.web_configured})),
            Some(json!({"web_configured": true})),
            true,
            "runtime and owned nginx web host configured",
        ))?;
        self.emit(
            "application.provisioned",
            id,
            context,
            json!({
                "runtime": application.runtime,
                "runtime_version": runtime.runtime_version.as_deref(),
                "runtime_resource_id": runtime.runtime_resource_id.as_deref(),
            }),
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
        let domains = if application.www_alias {
            vec![
                application.domain.clone(),
                format!("www.{}", application.domain),
            ]
        } else {
            vec![application.domain.clone()]
        };
        let request = CertificateRequest {
            resource: ResourceRef::new(
                ResourceKind::Certificate,
                format!("certificate.{}", application.id),
            )?,
            consumer: ResourceRef::new(
                ResourceKind::ServiceResource,
                format!("nginx.web-host.{}", application.id),
            )?,
            provider: "certbot-letsencrypt".into(),
            certificate_name: application.domain.clone(),
            domains,
            contact_email: email.into(),
        };
        CertificateManager::new(
            &self.state_dir,
            CertbotProvider::default(),
            NginxCertificateAttacher::new(&self.state_dir),
        )
        .issue(&request, context)
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
        let environment = self.resolve_environment(&application)?;
        let environment_file = self.materialize_environment(&application, &environment)?;
        let result = ApplicationProcessManager::system(&self.state_dir)
            .configure(&application, &process, environment_file.as_deref(), context)
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
        persist_application_process(&self.state_dir, &application, &process, &result)?;
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

    pub fn configure_deployment(
        &self,
        id: &str,
        workflow: DeploymentWorkflow,
        context: &OperationContext,
    ) -> Result<Application> {
        workflow.validate()?;
        if self.inspect(id)?.repository.is_none() {
            return Err(LumicError::InvalidInput {
                field: "repository".into(),
                message: "configure a repository before its deployment workflow".into(),
            });
        }
        let application = self.update_application(id, |application| {
            if let Some(repository) = &mut application.repository {
                repository.deployment = workflow.clone();
            }
        })?;
        self.emit(
            "application.deployment_workflow_configured",
            id,
            context,
            json!({
                "pre_deploy_commands": workflow.pre_deploy.len(),
                "custom_build": workflow.build.is_some(),
                "migration": workflow.migrate.is_some(),
                "post_deploy_commands": workflow.post_deploy.len(),
                "node_handoff": workflow.node_handoff.is_some(),
            }),
        )?;
        Ok(application)
    }

    /// Read and validate the repository-owned `lumic.yaml` contract without changing state.
    pub fn inspect_manifest(&self, repository_root: &Path) -> Result<ApplicationManifest> {
        read_application_manifest(repository_root)
    }

    /// Resolve a repository contract against an application's current repository configuration.
    pub fn plan_manifest(&self, id: &str, repository_root: &Path) -> Result<Plan> {
        let application = self.inspect(id)?;
        let repository =
            application
                .repository
                .as_ref()
                .ok_or_else(|| LumicError::InvalidInput {
                    field: "repository".into(),
                    message: "configure a repository before planning lumic.yaml".into(),
                })?;
        let contract = read_application_manifest(repository_root)?.resolve(&repository.branch)?;
        validate_manifest_application(&application, &contract)?;

        let mut risks = Vec::new();
        if contract.workflow.migrate.is_some() {
            risks.push(Risk {
                level: RiskLevel::High,
                summary: "the repository requests a database migration that release rollback may not reverse".into(),
                mitigation: Some("use backward-compatible expand/contract migrations and keep an operator recovery procedure".into()),
            });
        }
        if !contract.service_requirements.is_empty() {
            risks.push(Risk {
                level: RiskLevel::Medium,
                summary: "repository service requirements must resolve to managed service bindings before deployment".into(),
                mitigation: Some("review and bind each typed service requirement before applying a deployment".into()),
            });
        }
        Ok(Plan {
            id: format!("application-manifest-{id}"),
            summary: format!("Apply {} as the deployment contract for {id}", APPLICATION_MANIFEST_FILE),
            changes: vec![Change {
                capability: Capability::new("application.manifest.apply"),
                summary: "persist the validated runtime, build, public path, processes, schedules, services, health check, migration, and deployment intent".into(),
                before: repository.contract.as_ref().map(|value| format!("schema {} contract", value.manifest.schema_version)),
                after: Some(format!("schema {} contract from {}", contract.manifest.schema_version, repository_root.display())),
                reversible: true,
            }],
            risks,
            preconditions: vec![
                format!("{} is a regular file no larger than 256 KiB", repository_root.join(APPLICATION_MANIFEST_FILE).display()),
                format!("manifest application name and runtime match {id}"),
                "referenced runtime and managed services are available before deployment".into(),
            ],
            validation: vec![
                "all executable fields are non-empty argv arrays".into(),
                "all source and public paths remain relative to the repository".into(),
                "workers and schedules compile to typed systemd process definitions".into(),
            ],
            recovery: vec![
                "restore the previous lumic.yaml and apply its plan".into(),
                format!("redeploy the last healthy release for {id}"),
            ],
        })
    }

    /// Apply a previously reviewable repository contract to application state.
    pub async fn apply_manifest(
        &self,
        id: &str,
        repository_root: &Path,
        context: &OperationContext,
    ) -> Result<Application> {
        if !context.approved {
            return Err(LumicError::InvalidInput {
                field: "approval".into(),
                message: "applying lumic.yaml requires explicit approval".into(),
            });
        }
        let before = self.inspect(id)?;
        let repository = before
            .repository
            .as_ref()
            .ok_or_else(|| LumicError::InvalidInput {
                field: "repository".into(),
                message: "configure a repository before applying lumic.yaml".into(),
            })?;
        let contract = read_application_manifest(repository_root)?.resolve(&repository.branch)?;
        validate_manifest_application(&before, &contract)?;
        if context.dry_run {
            self.plan_manifest(id, repository_root)?;
            return Ok(before);
        }

        let runtime_intent = ApplicationRuntimeIntent {
            version: contract.runtime_version.clone(),
            components: contract.runtime_components.clone(),
            package_manager: contract.package_manager,
        };
        RuntimeManager::at_state_dir(&self.state_dir)
            .reconcile_intent(contract.runtime, &runtime_intent, context)
            .await?;

        let application = self.update_application(id, |application| {
            application.runtime_intent = runtime_intent.clone();
            application.health_check = contract.health.clone();
            application.processes = contract.processes.clone();
            application.release_retention = contract.manifest.deployment.retain_releases;
            if let Some(repository) = &mut application.repository {
                repository.branch = contract.branch.clone();
                repository.deployment = contract.workflow.clone();
                repository.contract = Some(contract.clone());
            }
        })?;
        self.audit.append(&AuditRecord::now(
            context,
            "application.manifest.apply",
            "apply_manifest",
            "application",
            id,
            json!({"file": APPLICATION_MANIFEST_FILE, "schema_version": contract.manifest.schema_version}),
            serde_json::to_value(&before).ok(),
            serde_json::to_value(&application).ok(),
            true,
            "repository application contract applied",
        ))?;
        self.emit(
            "application.manifest_applied",
            id,
            context,
            json!({
                "schema_version": contract.manifest.schema_version,
                "deploy_on_push": contract.manifest.deployment.deploy_on_push,
                "services": contract.service_requirements.len(),
                "processes": contract.processes.len(),
            }),
        )?;
        Ok(application)
    }

    pub fn deployment_logs(
        &self,
        id: &str,
        deployment_id: &str,
        after_sequence: u64,
    ) -> Result<Vec<DeploymentLogEntry>> {
        self.deployment(id, deployment_id)?;
        let path = self.deployment_log_path(deployment_id)?;
        if !path.exists() {
            return Ok(Vec::new());
        }
        let bytes = fs::read(path).map_err(state_io_error)?;
        let entries: Vec<DeploymentLogEntry> =
            serde_json::from_slice(&bytes).map_err(|error| LumicError::Internal {
                message: format!("deployment log is invalid: {error}"),
            })?;
        Ok(entries
            .into_iter()
            .filter(|entry| entry.sequence > after_sequence)
            .collect())
    }

    pub fn cancel_deployment(
        &self,
        id: &str,
        deployment_id: &str,
        context: &OperationContext,
    ) -> Result<Deployment> {
        let mut deployment = self.deployment(id, deployment_id)?;
        if deployment.status != DeploymentStatus::Started {
            return Err(invalid(
                "deployment",
                "only an active deployment can be cancelled",
            ));
        }
        let marker = self.cancellation_path(deployment_id)?;
        fs::create_dir_all(marker.parent().unwrap_or(&self.state_dir)).map_err(state_io_error)?;
        write_atomic(&marker, b"cancel\n", 0o600)?;
        deployment.status = DeploymentStatus::Cancelling;
        deployment.message =
            "cancellation requested; waiting for the current command to finish".into();
        self.upsert_deployment(&deployment)?;
        self.emit(
            "deployment.cancellation_requested",
            id,
            context,
            json!({"deployment_id": deployment_id}),
        )?;
        Ok(deployment)
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
        if repository.deployment.migrate.is_some() {
            risks.push(Risk {
                level: RiskLevel::High,
                summary: "the configured database migration may not be reversible by a release rollback"
                    .into(),
                mitigation: Some(
                    "use backward-compatible expand/contract migrations and keep an operator recovery procedure"
                        .into(),
                ),
            });
        }
        Ok(Plan {
            id: format!("deploy-{id}"),
            summary: format!("Deploy the latest {} revision for {id}", repository.branch),
            changes: vec![Change {
                capability: Capability::new("application.deploy"),
                summary: "lock deployment, create an isolated release, run pre-deploy/build/migrate, atomically activate, health-check, then run post-deploy"
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
                "no other deployment holds the application deployment lock".into(),
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
        self.deploy_revision(id, None, None, context).await
    }

    pub async fn redeploy(
        &self,
        id: &str,
        deployment_id: &str,
        context: &OperationContext,
    ) -> Result<Deployment> {
        let prior = self.deployment(id, deployment_id)?;
        if prior.commit.is_empty() || prior.status == DeploymentStatus::Started {
            return Err(invalid(
                "deployment",
                "redeploy requires a prior deployment with a resolved commit",
            ));
        }
        self.deploy_revision(id, Some(prior.id), Some(prior.commit), context)
            .await
    }

    async fn deploy_revision(
        &self,
        id: &str,
        retry_of: Option<String>,
        pinned_commit: Option<String>,
        context: &OperationContext,
    ) -> Result<Deployment> {
        let mut application = self.inspect(id)?;
        let resource = ResourceRef::new(ResourceKind::Application, id)?;
        let _deployment_lock = ResourceLock::try_acquire(&self.state_dir, &resource)?;
        let mut repository =
            application
                .repository
                .clone()
                .ok_or_else(|| LumicError::InvalidInput {
                    field: "repository".into(),
                    message: "configure a repository before deployment".into(),
                })?;
        repository.deployment.validate()?;
        let deployment_id = format!("{}-{}", unix_time_ms(), std::process::id());
        let release = PathBuf::from(&application.root)
            .join("releases")
            .join(&deployment_id);
        let previous_release = current_release(&application)?;
        let previous_node = previous_release.as_deref().and_then(|release_path| {
            self.store
                .load()
                .ok()?
                .deployments
                .into_iter()
                .rev()
                .find(|item| {
                    item.application_id == id
                        && item.status == DeploymentStatus::Completed
                        && item.release_path == release_path
                        && item.process_unit.is_some()
                })
        });
        let mut deployment = Deployment {
            id: deployment_id,
            application_id: id.into(),
            release_path: release.to_string_lossy().into_owned(),
            commit: String::new(),
            commit_metadata: None,
            status: DeploymentStatus::Started,
            healthy: false,
            message: "preparing release".into(),
            previous_release,
            phases: Vec::new(),
            automatic_rollback: false,
            retry_of,
            node_port: None,
            process_unit: None,
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
        self.append_log(
            &deployment.id,
            "deployment",
            DeploymentLogStream::System,
            "deployment lock acquired",
        )?;

        match self
            .prepare_release(
                &mut application,
                &mut repository,
                &mut deployment,
                ReleasePreparation {
                    pinned_commit: pinned_commit.as_deref(),
                    previous_node_port: previous_node.as_ref().and_then(|item| item.node_port),
                    release: &release,
                    context,
                },
            )
            .await
        {
            Ok(commit) => {
                deployment.commit = commit;
                let validation = match self.verify_health(&application).await {
                    Ok(message) => {
                        deployment.phases.push(phase(
                            "health",
                            DeploymentPhaseStatus::Completed,
                            message,
                        ));
                        self.upsert_deployment(&deployment)?;
                        match self.ensure_not_cancelled(&mut deployment) {
                            Ok(()) => {
                                let working_directory =
                                    manifest_working_directory(&repository, &release);
                                if let Err(error) = self
                                    .run_workflow_commands(
                                        "post_deploy",
                                        &repository.deployment.post_deploy,
                                        &working_directory,
                                        &application,
                                        &mut deployment,
                                    )
                                    .await
                                {
                                    Err(("post_deploy", error))
                                } else {
                                    let mut configured = Ok(());
                                    for process in application.processes.clone() {
                                        if let Err(error) =
                                            self.add_process(id, process, context).await
                                        {
                                            configured = Err(("processes", error));
                                            break;
                                        }
                                    }
                                    configured
                                }
                            }
                            Err(error) => Err(("cancellation", error)),
                        }
                    }
                    Err(error) => Err(("health", error)),
                };
                match validation {
                    Ok(()) => {
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
                        if let (Some(handoff), Some(previous)) =
                            (&repository.deployment.node_handoff, previous_node.as_ref())
                        {
                            if handoff.drain_seconds > 0 {
                                sleep(Duration::from_secs(handoff.drain_seconds)).await;
                            }
                            if let Some(unit) = &previous.process_unit {
                                let drain_result =
                                    ApplicationProcessManager::system(&self.state_dir)
                                        .stop_node_release(unit, context)
                                        .await;
                                match drain_result {
                                    Ok(()) => deployment.phases.push(phase(
                                        "drain",
                                        DeploymentPhaseStatus::Completed,
                                        format!("old process {unit} drained and stopped"),
                                    )),
                                    Err(error) => {
                                        deployment.phases.push(phase(
                                            "drain",
                                            DeploymentPhaseStatus::Failed,
                                            format!("release is healthy, but {unit} could not be stopped: {error}"),
                                        ));
                                        deployment.message = format!(
                                            "release activated and healthy; old process requires manual drain: {error}"
                                        );
                                    }
                                }
                                self.upsert_deployment(&deployment)?;
                            }
                        } else {
                            deployment.phases.push(phase(
                                "drain",
                                DeploymentPhaseStatus::Skipped,
                                "no previous blue-green Node process",
                            ));
                            self.upsert_deployment(&deployment)?;
                        }
                        self.prune_releases(&application)?;
                        Ok(deployment)
                    }
                    Err((failed_phase, error)) => {
                        deployment.phases.push(phase(
                            failed_phase,
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
                        if deployment.process_unit.is_some() {
                            if let Some(port) =
                                previous_node.as_ref().and_then(|item| item.node_port)
                            {
                                NginxManager::system(&self.state_dir)
                                    .configure_node_upstream(&application, port, context)
                                    .await?;
                            }
                            if let Some(unit) = &deployment.process_unit {
                                ApplicationProcessManager::system(&self.state_dir)
                                    .stop_node_release(unit, context)
                                    .await?;
                            }
                        }
                        deployment.automatic_rollback = rolled_back;
                        deployment.status = if failed_phase == "cancellation" {
                            DeploymentStatus::Cancelled
                        } else if rolled_back {
                            DeploymentStatus::FailedRolledBack
                        } else {
                            DeploymentStatus::Failed
                        };
                        deployment.message = if rolled_back {
                            format!("{failed_phase} failed; previous release restored: {error}")
                        } else {
                            format!(
                                "{failed_phase} failed and no previous release was available: {error}"
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
                let activated = deployment.phases.iter().any(|item| {
                    item.name == "activation" && item.status == DeploymentPhaseStatus::Completed
                });
                let restored = if activated {
                    if let Some(previous) = &deployment.previous_release {
                        if Path::new(previous).is_dir() {
                            activate(&application, Path::new(previous), "automatic-rollback")?;
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                };
                if deployment.process_unit.is_some()
                    && let Some(port) = previous_node.as_ref().and_then(|item| item.node_port)
                {
                    let _ = NginxManager::system(&self.state_dir)
                        .configure_node_upstream(&application, port, context)
                        .await;
                }
                if let Some(unit) = &deployment.process_unit {
                    let _ = ApplicationProcessManager::system(&self.state_dir)
                        .stop_node_release(unit, context)
                        .await;
                }
                if release.exists() {
                    fs::remove_dir_all(&release).map_err(state_io_error)?;
                }
                let cancelled = self.cancellation_path(&deployment.id)?.exists();
                deployment.automatic_rollback = restored;
                deployment.status = if cancelled {
                    DeploymentStatus::Cancelled
                } else if restored {
                    DeploymentStatus::FailedRolledBack
                } else {
                    DeploymentStatus::Failed
                };
                deployment.phases.push(phase(
                    "deployment",
                    DeploymentPhaseStatus::Failed,
                    error.to_string(),
                ));
                deployment.message = error.to_string();
                deployment.finished_at_unix_ms = Some(unix_time_ms());
                self.upsert_deployment(&deployment)?;
                self.set_health(
                    id,
                    if cancelled {
                        "deployment_cancelled"
                    } else {
                        "deployment_failed"
                    },
                )?;
                self.audit_deployment(&deployment, context, false)?;
                self.emit(
                    if cancelled {
                        "deployment.cancelled"
                    } else {
                        "deployment.failed"
                    },
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

    pub async fn rollback(&self, id: &str, context: &OperationContext) -> Result<Deployment> {
        let application = self.inspect(id)?;
        let resource = ResourceRef::new(ResourceKind::Application, id)?;
        let _deployment_lock = ResourceLock::try_acquire(&self.state_dir, &resource)?;
        let current = current_release(&application);
        let current = current?;
        let state = self.store.load()?;
        let active_node = current.as_ref().and_then(|release| {
            state.deployments.iter().rev().find(|deployment| {
                deployment.application_id == id
                    && deployment.status == DeploymentStatus::Completed
                    && Path::new(&deployment.release_path) == release
                    && deployment.process_unit.is_some()
                    && deployment.node_port.is_some()
            })
        });
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
        if let (Some(unit), Some(port)) = (&target.process_unit, target.node_port) {
            let processes = ApplicationProcessManager::system(&self.state_dir);
            processes.start_existing_node_release(unit, context).await?;
            if let Err(error) = self.verify_health_on_port(&application, port).await {
                let _ = processes.stop_node_release(unit, context).await;
                return Err(error);
            }
            activate(&application, Path::new(&target.release_path), &target.id)?;
            if let Err(error) = NginxManager::system(&self.state_dir)
                .configure_node_upstream(&application, port, context)
                .await
            {
                if let Some(previous) = &current {
                    activate(&application, Path::new(previous), "rollback-recovery")?;
                }
                let _ = processes.stop_node_release(unit, context).await;
                return Err(error);
            }
            if let Err(error) = self.verify_health(&application).await {
                if let Some(previous) = &current {
                    activate(&application, Path::new(previous), "rollback-recovery")?;
                }
                if let Some(previous) = active_node
                    && let Some(previous_port) = previous.node_port
                {
                    let _ = NginxManager::system(&self.state_dir)
                        .configure_node_upstream(&application, previous_port, context)
                        .await;
                }
                let _ = processes.stop_node_release(unit, context).await;
                return Err(error);
            }
            if let Some(previous) = active_node
                && let Some(previous_unit) = &previous.process_unit
                && previous_unit != unit
            {
                if let Some(handoff) = application
                    .repository
                    .as_ref()
                    .and_then(|repository| repository.deployment.node_handoff.as_ref())
                {
                    tokio::time::sleep(Duration::from_secs(handoff.drain_seconds)).await;
                }
                processes.stop_node_release(previous_unit, context).await?;
            }
        } else {
            activate(&application, Path::new(&target.release_path), &target.id)?;
        }
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
        application: &mut Application,
        repository: &mut RepositoryConfig,
        deployment: &mut Deployment,
        preparation: ReleasePreparation<'_>,
    ) -> Result<String> {
        let ReleasePreparation {
            pinned_commit,
            previous_node_port,
            release,
            context,
        } = preparation;
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
        self.ensure_not_cancelled(deployment)?;
        let reference = pinned_commit
            .map(str::to_owned)
            .unwrap_or_else(|| format!("refs/heads/{}", repository.branch));
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
        let metadata_output = self
            .run_git(
                [
                    "--git-dir",
                    path_text(&repository_path)?,
                    "show",
                    "-s",
                    "--format=%H%x00%an%x00%ae%x00%s%x00%aI",
                    &commit,
                ],
                credential.as_deref(),
            )
            .await?;
        deployment.commit_metadata = Some(parse_commit_metadata(&metadata_output.stdout)?);
        deployment.commit = commit.clone();
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

        let manifest_path = release.join(APPLICATION_MANIFEST_FILE);
        if manifest_path.exists() {
            let before_manifest_sync = application.clone();
            let contract = read_application_manifest(release)?.resolve(&repository.branch)?;
            validate_manifest_application(application, &contract)?;
            if contract.branch != repository.branch {
                return Err(invalid(
                    "source.branch",
                    "a branch change must be reviewed with application manifest plan/apply before deployment",
                ));
            }
            validate_manifest_service_bindings(application, &contract)?;
            if repository.contract.as_ref() != Some(&contract) {
                return Err(invalid(
                    APPLICATION_MANIFEST_FILE,
                    "the deployed revision changed runtime or deployment intent; review and apply its manifest before deployment",
                ));
            }
            let runtime_intent = validate_reconciled_runtime_intent(application, &contract)?;
            RuntimeManager::at_state_dir(&self.state_dir)
                .verify_intent(contract.runtime, &runtime_intent)
                .await?;
            application.health_check = contract.health.clone();
            application.processes = contract.processes.clone();
            application.release_retention = contract.manifest.deployment.retain_releases;
            repository.deployment = contract.workflow.clone();
            repository.contract = Some(contract);
            application.repository = Some(repository.clone());
            *application = self.update_application(&application.id, |stored| {
                stored.health_check = application.health_check.clone();
                stored.processes = application.processes.clone();
                stored.release_retention = application.release_retention;
                stored.repository = application.repository.clone();
            })?;
            self.audit.append(&AuditRecord::now(
                context,
                "application.manifest.sync",
                "prepare_release",
                "application",
                &application.id,
                json!({
                    "deployment_id": deployment.id,
                    "commit": commit,
                    "schema_version": repository
                        .contract
                        .as_ref()
                        .map(|contract| contract.manifest.schema_version),
                }),
                serde_json::to_value(&before_manifest_sync).ok(),
                serde_json::to_value(&application).ok(),
                true,
                "repository contract resolved from checked-out deployment revision",
            ))?;
            self.emit(
                "application.manifest_synced",
                &application.id,
                context,
                json!({
                    "deployment_id": deployment.id,
                    "commit": commit,
                }),
            )?;
            deployment.phases.push(phase(
                "manifest",
                DeploymentPhaseStatus::Completed,
                "validated and resolved repository lumic.yaml",
            ));
            self.upsert_deployment(deployment)?;
        } else if repository.contract.is_some() {
            return Err(invalid(
                APPLICATION_MANIFEST_FILE,
                "the deployed revision removed its applied repository contract",
            ));
        }

        materialize_shared_paths(application, repository, release)?;

        let working_directory = manifest_working_directory(repository, release);
        if !working_directory.is_dir() {
            return Err(invalid(
                "source.subdirectory",
                "the configured source subdirectory is missing from the release",
            ));
        }
        let environment = self.resolve_environment(application)?;
        let environment_file = self.materialize_environment(application, &environment)?;

        self.run_workflow_commands(
            "pre_deploy",
            &repository.deployment.pre_deploy,
            &working_directory,
            application,
            deployment,
        )
        .await?;
        self.ensure_not_cancelled(deployment)?;

        if let Some(command) = &repository.deployment.build {
            self.run_workflow_commands(
                "build",
                std::slice::from_ref(command),
                &working_directory,
                application,
                deployment,
            )
            .await?;
        } else if let Some((spec, message)) = dependency_install_spec(
            application.runtime,
            application.runtime_intent.package_manager,
            &working_directory,
        ) {
            deployment.phases.push(phase(
                "build",
                DeploymentPhaseStatus::Running,
                "running bounded runtime dependency build",
            ));
            self.upsert_deployment(deployment)?;
            if let Err(error) = self
                .run_deployment_process("build", spec, &environment, deployment)
                .await
            {
                finish_running_phase(
                    deployment,
                    "build",
                    DeploymentPhaseStatus::Failed,
                    error.to_string(),
                );
                self.upsert_deployment(deployment)?;
                return Err(error);
            }
            finish_running_phase(
                deployment,
                "build",
                DeploymentPhaseStatus::Completed,
                message,
            );
        } else {
            deployment.phases.push(phase(
                "build",
                DeploymentPhaseStatus::Skipped,
                "runtime has no dependency build step",
            ));
        }
        self.upsert_deployment(deployment)?;
        self.ensure_not_cancelled(deployment)?;
        if let Some(command) = &repository.deployment.migrate {
            self.run_workflow_commands(
                "migrate",
                std::slice::from_ref(command),
                &working_directory,
                application,
                deployment,
            )
            .await?;
        } else {
            deployment.phases.push(phase(
                "migrate",
                DeploymentPhaseStatus::Skipped,
                "no database migration command configured",
            ));
            self.upsert_deployment(deployment)?;
        }
        self.ensure_not_cancelled(deployment)?;
        let public_directory = manifest_public_directory(repository, &working_directory);
        let entry_point = match application.runtime {
            ApplicationRuntime::Static => public_directory.join("index.html"),
            ApplicationRuntime::Php => public_directory.join("index.php"),
            ApplicationRuntime::Node => working_directory.join("package.json"),
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
        if let Some(handoff) = &repository.deployment.node_handoff {
            if application.runtime != ApplicationRuntime::Node || !application.web_configured {
                return Err(invalid(
                    "node_handoff",
                    "blue-green handoff requires a provisioned Node application web host",
                ));
            }
            let port = if previous_node_port == Some(handoff.primary_port) {
                handoff.secondary_port
            } else {
                handoff.primary_port
            };
            let unit = ApplicationProcessManager::system(&self.state_dir)
                .start_node_release(NodeReleaseStart {
                    application,
                    handoff,
                    release: &working_directory,
                    deployment_id: &deployment.id,
                    port,
                    environment_file: environment_file.as_deref(),
                    context,
                })
                .await?;
            deployment.node_port = Some(port);
            deployment.process_unit = Some(unit.clone());
            deployment.phases.push(phase(
                "node_start",
                DeploymentPhaseStatus::Completed,
                format!("started {unit} on loopback port {port}"),
            ));
            self.upsert_deployment(deployment)?;
            self.verify_health_on_port(application, port).await?;
            deployment.phases.push(phase(
                "node_readiness",
                DeploymentPhaseStatus::Completed,
                format!("new Node process is ready on port {port}"),
            ));
        }
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
        if let Some(port) = deployment.node_port {
            NginxManager::system(&self.state_dir)
                .configure_node_upstream(application, port, context)
                .await?;
            deployment.phases.push(phase(
                "node_handoff",
                DeploymentPhaseStatus::Completed,
                format!("nginx atomically switched to Node port {port}"),
            ));
        }
        self.upsert_deployment(deployment)?;
        Ok(commit)
    }

    fn ensure_not_cancelled(&self, deployment: &mut Deployment) -> Result<()> {
        if self.cancellation_path(&deployment.id)?.exists() {
            deployment.status = DeploymentStatus::Cancelled;
            deployment.message = "deployment cancelled at a safe phase boundary".into();
            deployment.finished_at_unix_ms = Some(unix_time_ms());
            self.upsert_deployment(deployment)?;
            self.append_log(
                &deployment.id,
                "deployment",
                DeploymentLogStream::System,
                &deployment.message,
            )?;
            return Err(invalid("deployment", &deployment.message));
        }
        Ok(())
    }

    async fn run_workflow_commands(
        &self,
        phase_name: &str,
        commands: &[Vec<String>],
        release: &Path,
        application: &Application,
        deployment: &mut Deployment,
    ) -> Result<()> {
        if commands.is_empty() {
            deployment.phases.push(phase(
                phase_name,
                DeploymentPhaseStatus::Skipped,
                "no commands configured",
            ));
            self.upsert_deployment(deployment)?;
            return Ok(());
        }
        deployment.phases.push(phase(
            phase_name,
            DeploymentPhaseStatus::Running,
            format!("running {} command(s)", commands.len()),
        ));
        self.upsert_deployment(deployment)?;
        let environment = self.resolve_environment(application)?;
        for command in commands {
            self.ensure_not_cancelled(deployment)?;
            validate_command(command)?;
            let mut spec = ProcessSpec::new(&command[0])
                .args(command.iter().skip(1))
                .current_dir(release);
            spec.environment.extend(environment.clone());
            if let Err(error) = self
                .run_deployment_process(phase_name, spec, &environment, deployment)
                .await
            {
                finish_running_phase(
                    deployment,
                    phase_name,
                    DeploymentPhaseStatus::Failed,
                    error.to_string(),
                );
                self.upsert_deployment(deployment)?;
                return Err(error);
            }
        }
        finish_running_phase(
            deployment,
            phase_name,
            DeploymentPhaseStatus::Completed,
            format!("{} command(s) completed", commands.len()),
        );
        self.upsert_deployment(deployment)
    }

    async fn run_deployment_process(
        &self,
        phase_name: &str,
        mut spec: ProcessSpec,
        environment: &BTreeMap<String, String>,
        deployment: &mut Deployment,
    ) -> Result<ProcessOutput> {
        spec.timeout = Duration::from_secs(1_800);
        spec.environment.extend(environment.clone());
        let executable = spec.executable.clone();
        self.append_log(
            &deployment.id,
            phase_name,
            DeploymentLogStream::System,
            format!("starting {executable}"),
        )?;
        let output = self.runner.run(&spec).await?;
        self.append_output(
            &deployment.id,
            phase_name,
            DeploymentLogStream::Stdout,
            &output.stdout,
            environment,
        )?;
        self.append_output(
            &deployment.id,
            phase_name,
            DeploymentLogStream::Stderr,
            &output.stderr,
            environment,
        )?;
        if output.success() {
            Ok(output)
        } else {
            Err(LumicError::Process {
                executable,
                message: redact_environment(
                    String::from_utf8_lossy(&output.stderr).trim(),
                    environment,
                ),
            })
        }
    }

    fn append_output(
        &self,
        deployment_id: &str,
        phase_name: &str,
        stream: DeploymentLogStream,
        bytes: &[u8],
        environment: &BTreeMap<String, String>,
    ) -> Result<()> {
        for line in String::from_utf8_lossy(bytes).lines() {
            self.append_log(
                deployment_id,
                phase_name,
                stream,
                redact_environment(line, environment),
            )?;
        }
        Ok(())
    }

    fn resolve_environment(&self, application: &Application) -> Result<BTreeMap<String, String>> {
        let store = SecretStore::at_state_dir(&self.state_dir);
        application
            .environment_references
            .iter()
            .map(|(name, reference)| {
                let value = store.read(reference)?;
                validate_application_environment_value(&value)?;
                let value = String::from_utf8(value).map_err(|_| {
                    invalid(
                        "secret",
                        "application environment value must be valid UTF-8",
                    )
                })?;
                Ok((name.clone(), value))
            })
            .collect()
    }

    fn materialize_environment(
        &self,
        application: &Application,
        environment: &BTreeMap<String, String>,
    ) -> Result<Option<PathBuf>> {
        if environment.is_empty() {
            return Ok(None);
        }
        let path = PathBuf::from("/run/lumic/application-environments")
            .join(format!("{}.env", application.id));
        write_atomic(
            &path,
            render_environment_file(environment).as_bytes(),
            0o600,
        )?;
        Ok(Some(path))
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

    /// Stores an application-owned environment value and attaches only its stable reference.
    /// The value is accepted only at this explicit mutation boundary and is never audited.
    pub fn set_environment_secret(
        &self,
        id: &str,
        name: &str,
        value: &[u8],
        context: &OperationContext,
    ) -> Result<Application> {
        validate_application_environment_value(value)?;
        self.inspect(id)?;
        let reference = application_environment_reference(id, name)?;
        SecretStore::at_state_dir(&self.state_dir).put(&reference, value)?;
        self.set_environment_reference(id, name, &reference, context)
    }

    /// Replaces an application-owned value with fresh random material without returning it.
    pub fn rotate_environment_secret(
        &self,
        id: &str,
        name: &str,
        context: &OperationContext,
    ) -> Result<Application> {
        let application = self.inspect(id)?;
        lumic_core::recipe::validate_environment_name(name)?;
        let reference = application
            .environment_references
            .get(name)
            .ok_or_else(|| invalid("environment", "environment key is not configured"))?;
        let expected = application_environment_reference(id, name)?;
        if reference != &expected {
            return Err(invalid(
                "environment",
                "rotation is limited to application-owned secret references",
            ));
        }
        SecretStore::at_state_dir(&self.state_dir).rotate(reference)?;
        self.emit(
            "application.environment_secret_rotated",
            id,
            context,
            json!({"name": name}),
        )?;
        self.audit.append(&AuditRecord::now(
            context,
            "application.environment.rotate",
            "rotate",
            "application",
            id,
            json!({"name": name}),
            None,
            Some(json!({"configured": true})),
            true,
            "application environment secret rotated",
        ))?;
        Ok(application)
    }

    /// Detaches an environment key and removes its value only when Lumic owns that value.
    pub fn delete_environment_secret(
        &self,
        id: &str,
        name: &str,
        context: &OperationContext,
    ) -> Result<Application> {
        lumic_core::recipe::validate_environment_name(name)?;
        let before = self.inspect(id)?;
        let reference = before
            .environment_references
            .get(name)
            .cloned()
            .ok_or_else(|| invalid("environment", "environment key is not configured"))?;
        let application = self.update_application(id, |application| {
            application.environment_references.remove(name);
        })?;
        if reference == application_environment_reference(id, name)? {
            SecretStore::at_state_dir(&self.state_dir).delete(&reference)?;
        }
        self.emit(
            "application.environment_secret_deleted",
            id,
            context,
            json!({"name": name}),
        )?;
        self.audit.append(&AuditRecord::now(
            context,
            "application.environment.delete",
            "delete",
            "application",
            id,
            json!({"name": name}),
            Some(json!({"configured": true})),
            Some(json!({"configured": false})),
            true,
            "application environment secret deleted",
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
            process.validate()?;
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
        let _state_lock = ResourceLock::acquire_application_state(&self.state_dir)?;
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

    fn deployment(&self, application_id: &str, deployment_id: &str) -> Result<Deployment> {
        validate_slug("application", application_id)?;
        validate_deployment_id(deployment_id)?;
        self.store
            .load()?
            .deployments
            .into_iter()
            .find(|item| item.application_id == application_id && item.id == deployment_id)
            .ok_or_else(|| {
                invalid(
                    "deployment",
                    "deployment was not found for this application",
                )
            })
    }

    fn deployment_log_path(&self, deployment_id: &str) -> Result<PathBuf> {
        validate_deployment_id(deployment_id)?;
        Ok(self
            .state_dir
            .join("deployment-logs")
            .join(format!("{deployment_id}.json")))
    }

    fn cancellation_path(&self, deployment_id: &str) -> Result<PathBuf> {
        validate_deployment_id(deployment_id)?;
        Ok(self
            .state_dir
            .join("deployment-cancellations")
            .join(deployment_id))
    }

    fn append_log(
        &self,
        deployment_id: &str,
        phase: &str,
        stream: DeploymentLogStream,
        message: impl Into<String>,
    ) -> Result<()> {
        let path = self.deployment_log_path(deployment_id)?;
        let mut entries = if path.exists() {
            serde_json::from_slice::<Vec<DeploymentLogEntry>>(
                &fs::read(&path).map_err(state_io_error)?,
            )
            .map_err(|error| LumicError::Internal {
                message: format!("deployment log is invalid: {error}"),
            })?
        } else {
            Vec::new()
        };
        entries.push(DeploymentLogEntry {
            sequence: entries.last().map_or(1, |entry| entry.sequence + 1),
            timestamp_unix_ms: unix_time_ms(),
            phase: phase.into(),
            stream,
            message: message.into(),
        });
        let bytes = serde_json::to_vec_pretty(&entries).map_err(|error| LumicError::Internal {
            message: format!("could not serialize deployment log: {error}"),
        })?;
        write_atomic(&path, &bytes, 0o600)?;
        Ok(())
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

    async fn verify_health_on_port(&self, application: &Application, port: u16) -> Result<String> {
        let mut candidate = application.clone();
        candidate.health_check.enabled = true;
        candidate.health_check.port = port;
        self.verify_health(&candidate).await
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

fn application_environment_reference(id: &str, name: &str) -> Result<String> {
    validate_slug("application", id)?;
    lumic_core::recipe::validate_environment_name(name)?;
    let key = name
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' => char::from(byte.to_ascii_lowercase()),
            b'a'..=b'z' | b'0'..=b'9' => char::from(byte),
            _ => '-',
        })
        .collect::<String>();
    let reference = format!("application-{id}-environment-{key}");
    lumic_core::managed_service::validate_resource_id("secret_reference", &reference)?;
    Ok(reference)
}

fn validate_application_environment_value(value: &[u8]) -> Result<()> {
    if value.is_empty()
        || value.len() > 16 * 1024
        || value.contains(&0)
        || value.contains(&b'\n')
        || value.contains(&b'\r')
        || std::str::from_utf8(value).is_err()
    {
        return Err(invalid(
            "secret",
            "application environment values must be non-empty single-line UTF-8, at most 16 KiB, with no NUL bytes",
        ));
    }
    Ok(())
}

fn render_environment_file(environment: &BTreeMap<String, String>) -> String {
    let mut output = String::new();
    for (name, value) in environment {
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        output.push_str(name);
        output.push_str("=\"");
        output.push_str(&escaped);
        output.push_str("\"\n");
    }
    output
}

fn redact_environment(value: &str, environment: &BTreeMap<String, String>) -> String {
    environment
        .values()
        .fold(value.to_owned(), |redacted, secret| {
            if secret.is_empty() {
                redacted
            } else {
                redacted.replace(secret, "[REDACTED]")
            }
        })
}

fn dependency_install_spec(
    runtime: ApplicationRuntime,
    package_manager: Option<NodePackageManager>,
    release: &Path,
) -> Option<(ProcessSpec, &'static str)> {
    let (mut spec, message) = match runtime {
        ApplicationRuntime::Php if release.join("composer.json").is_file() => (
            ProcessSpec::new("composer")
                .args([
                    "install",
                    "--no-dev",
                    "--no-interaction",
                    "--no-plugins",
                    "--no-scripts",
                    "--prefer-dist",
                    "--optimize-autoloader",
                ])
                .current_dir(release),
            "Composer dependencies installed with plugins and scripts disabled",
        ),
        ApplicationRuntime::Node
            if package_manager.unwrap_or(NodePackageManager::Npm) == NodePackageManager::Npm
                && release.join("package-lock.json").is_file() =>
        {
            (
                ProcessSpec::new("npm")
                    .args(["ci", "--omit=dev", "--ignore-scripts"])
                    .current_dir(release),
                "npm dependencies installed with lifecycle scripts disabled",
            )
        }
        ApplicationRuntime::Node
            if package_manager == Some(NodePackageManager::Pnpm)
                && release.join("pnpm-lock.yaml").is_file() =>
        {
            (
                ProcessSpec::new("pnpm")
                    .args(["install", "--prod", "--frozen-lockfile", "--ignore-scripts"])
                    .current_dir(release),
                "pnpm dependencies installed with lifecycle scripts disabled",
            )
        }
        ApplicationRuntime::Node
            if package_manager == Some(NodePackageManager::Yarn)
                && release.join("yarn.lock").is_file() =>
        {
            (
                ProcessSpec::new("yarn")
                    .args([
                        "install",
                        "--production",
                        "--frozen-lockfile",
                        "--ignore-scripts",
                    ])
                    .current_dir(release),
                "Yarn dependencies installed with lifecycle scripts disabled",
            )
        }
        _ => return None,
    };
    spec.timeout = Duration::from_secs(600);
    Some((spec, message))
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
        started_at_unix_ms: unix_time_ms(),
        finished_at_unix_ms: (status != DeploymentPhaseStatus::Running).then(unix_time_ms),
    }
}

fn finish_running_phase(
    deployment: &mut Deployment,
    name: &str,
    status: DeploymentPhaseStatus,
    message: impl Into<String>,
) {
    if let Some(phase) = deployment
        .phases
        .iter_mut()
        .rev()
        .find(|phase| phase.name == name && phase.status == DeploymentPhaseStatus::Running)
    {
        phase.status = status;
        phase.message = message.into();
        phase.finished_at_unix_ms = Some(unix_time_ms());
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

fn read_application_manifest(repository_root: &Path) -> Result<ApplicationManifest> {
    if !repository_root.is_dir() {
        return Err(LumicError::InvalidInput {
            field: "repository_root".into(),
            message: "must be an existing repository directory".into(),
        });
    }
    let path = repository_root.join(APPLICATION_MANIFEST_FILE);
    let metadata = fs::symlink_metadata(&path).map_err(state_io_error)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > 256 * 1024 {
        return Err(LumicError::InvalidInput {
            field: APPLICATION_MANIFEST_FILE.into(),
            message: "must be a non-symlink regular file no larger than 256 KiB".into(),
        });
    }
    let source = fs::read_to_string(path).map_err(state_io_error)?;
    ApplicationManifest::parse(&source)
}

fn manifest_working_directory(repository: &RepositoryConfig, release: &Path) -> PathBuf {
    repository
        .contract
        .as_ref()
        .and_then(|contract| contract.source_subdirectory.as_ref())
        .map_or_else(|| release.to_path_buf(), |path| release.join(path))
}

fn manifest_public_directory(repository: &RepositoryConfig, working_directory: &Path) -> PathBuf {
    repository
        .contract
        .as_ref()
        .and_then(|contract| contract.public_directory.as_ref())
        .map_or_else(
            || working_directory.to_path_buf(),
            |path| working_directory.join(path),
        )
}

fn materialize_shared_paths(
    application: &Application,
    repository: &RepositoryConfig,
    release: &Path,
) -> Result<()> {
    let Some(contract) = &repository.contract else {
        return Ok(());
    };
    let shared_root = PathBuf::from(&application.root).join("shared");
    for relative in &contract.shared_directories {
        let shared = shared_root.join(relative);
        let destination = release.join(relative);
        if destination.exists() || destination.is_symlink() {
            return Err(invalid(
                "shared.directories",
                &format!(
                    "{} already exists in the release; shared paths must not be committed",
                    relative.display()
                ),
            ));
        }
        fs::create_dir_all(&shared).map_err(state_io_error)?;
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(state_io_error)?;
        }
        #[cfg(unix)]
        symlink(&shared, &destination).map_err(state_io_error)?;
    }
    for relative in &contract.shared_files {
        let shared = shared_root.join(relative);
        let destination = release.join(relative);
        if destination.is_symlink() || shared.is_symlink() {
            return Err(invalid(
                "shared.files",
                "shared file paths cannot be symlinks",
            ));
        }
        if let Some(parent) = shared.parent() {
            fs::create_dir_all(parent).map_err(state_io_error)?;
        }
        if !shared.exists() {
            if destination.is_file() {
                fs::copy(&destination, &shared).map_err(state_io_error)?;
            } else {
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&shared)
                    .map_err(state_io_error)?;
            }
        }
        if destination.exists() {
            if !destination.is_file() {
                return Err(invalid(
                    "shared.files",
                    "shared file collides with a directory",
                ));
            }
            fs::remove_file(&destination).map_err(state_io_error)?;
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(state_io_error)?;
        }
        #[cfg(unix)]
        symlink(&shared, &destination).map_err(state_io_error)?;
    }
    Ok(())
}

fn validate_manifest_service_bindings(
    application: &Application,
    contract: &ResolvedApplicationManifest,
) -> Result<()> {
    for requirement in &contract.service_requirements {
        let binding = application
            .service_references
            .iter()
            .find(|binding| binding.role == requirement.role)
            .ok_or_else(|| LumicError::InvalidInput {
                field: format!("services.{}", requirement.role),
                message: format!(
                    "requires a bound {} managed service before deployment",
                    requirement.service_type
                ),
            })?;
        if binding.service_type.as_deref() != Some(requirement.service_type.as_str())
            || requirement
                .instance
                .as_ref()
                .is_some_and(|instance| instance != &binding.service_id)
            || requirement
                .database
                .as_ref()
                .is_some_and(|database| binding.database.as_ref() != Some(database))
            || requirement
                .user
                .as_ref()
                .is_some_and(|user| binding.user.as_ref() != Some(user))
        {
            return Err(LumicError::InvalidInput {
                field: format!("services.{}", requirement.role),
                message: "does not match the application's managed service binding".into(),
            });
        }
    }
    Ok(())
}

fn validate_manifest_application(
    application: &Application,
    contract: &ResolvedApplicationManifest,
) -> Result<()> {
    if contract.manifest.name != application.id {
        return Err(LumicError::InvalidInput {
            field: "name".into(),
            message: format!("must match application id {}", application.id),
        });
    }
    if contract.runtime != application.runtime {
        return Err(LumicError::InvalidInput {
            field: "runtime".into(),
            message: format!(
                "must match the application's {:?} runtime",
                application.runtime
            ),
        });
    }
    Ok(())
}

fn validate_reconciled_runtime_intent(
    application: &Application,
    contract: &ResolvedApplicationManifest,
) -> Result<ApplicationRuntimeIntent> {
    let expected = ApplicationRuntimeIntent {
        version: contract.runtime_version.clone(),
        components: contract.runtime_components.clone(),
        package_manager: contract.package_manager,
    };
    if application.runtime_intent != expected {
        return Err(LumicError::InvalidInput {
            field: "runtime".into(),
            message: "lumic.yaml runtime intent has not been applied; review and apply the manifest before deployment".into(),
        });
    }
    Ok(expected)
}

fn persist_application_resource(state_dir: &Path, application: &Application) -> Result<()> {
    let now = framework_time();
    let store = FrameworkStateStore::at_state_dir(state_dir);
    let mut state = store.load_or_migrate(now)?;
    upsert_application_record(&mut state.resources, application, now)?;
    store.save(&state)
}

fn persist_application_process(
    state_dir: &Path,
    application: &Application,
    process: &ApplicationProcess,
    result: &ProcessConfigurationResult,
) -> Result<()> {
    let now = framework_time();
    let store = FrameworkStateStore::at_state_dir(state_dir);
    let mut state = store.load_or_migrate(now)?;
    let application_ref = upsert_application_record(&mut state.resources, application, now)?;
    let process_id = format!("application.{}.{}", application.id, process.name);
    state.resources.retain(|record| {
        !(record.resource.id == process_id
            && matches!(
                record.resource.kind,
                ResourceKind::Process | ResourceKind::Schedule
            ))
    });
    state.bindings.0.retain(|binding| {
        !(binding.producer.id == process_id
            && matches!(
                binding.producer.kind,
                ResourceKind::Process | ResourceKind::Schedule
            ))
    });
    let kind = match process.kind {
        lumic_core::application::ApplicationProcessKind::Worker => ResourceKind::Process,
        lumic_core::application::ApplicationProcessKind::Schedule => ResourceKind::Schedule,
    };
    let process_ref = ResourceRef::new(kind, process_id)?;
    state.resources.push(ResourceRecord {
        resource: process_ref.clone(),
        attributes: BTreeMap::from([
            ("application_id".into(), json!(application.id)),
            ("name".into(), json!(process.name)),
            ("kind".into(), json!(process.kind)),
            ("command".into(), json!(process.command)),
            ("schedule".into(), json!(process.schedule)),
            ("enabled".into(), json!(process.enabled)),
            ("ownership".into(), json!("lumic")),
        ]),
        outputs: ResourceOutputs::from([(
            "units".into(),
            ResourceOutput {
                value: json!(result.units),
                sensitive: false,
                updated_at_unix_ms: now,
            },
        )]),
        created_at_unix_ms: now,
        updated_at_unix_ms: now,
    });
    state.bindings.0.push(Binding {
        id: format!("{}-{}-to-application", application.id, process.name),
        producer: process_ref,
        output: "units".into(),
        consumer: application_ref,
        input: format!("process_{}", process.name),
        created_at_unix_ms: now,
    });
    store.save(&state)
}

fn upsert_application_record(
    resources: &mut Vec<ResourceRecord>,
    application: &Application,
    now: u64,
) -> Result<ResourceRef> {
    let application_ref = ResourceRef::new(ResourceKind::Application, &application.id)?;
    let created_at = resources
        .iter()
        .find(|record| record.resource == application_ref)
        .map_or(now, |record| record.created_at_unix_ms);
    resources.retain(|record| record.resource != application_ref);
    resources.push(ResourceRecord {
        resource: application_ref.clone(),
        attributes: BTreeMap::from([
            ("domain".into(), json!(application.domain)),
            ("www_alias".into(), json!(application.www_alias)),
            ("root".into(), json!(application.root)),
            ("runtime".into(), json!(application.runtime)),
            ("resource_type".into(), json!("application")),
        ]),
        outputs: ResourceOutputs::from([
            (
                "domain".into(),
                ResourceOutput {
                    value: json!(application.domain),
                    sensitive: false,
                    updated_at_unix_ms: now,
                },
            ),
            (
                "root".into(),
                ResourceOutput {
                    value: json!(application.root),
                    sensitive: false,
                    updated_at_unix_ms: now,
                },
            ),
        ]),
        created_at_unix_ms: created_at,
        updated_at_unix_ms: now,
    });
    Ok(application_ref)
}

fn framework_time() -> u64 {
    u64::try_from(unix_time_ms()).unwrap_or(u64::MAX)
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

fn parse_commit_metadata(bytes: &[u8]) -> Result<CommitMetadata> {
    let value = String::from_utf8_lossy(bytes);
    let fields = value.trim_end().split('\0').collect::<Vec<_>>();
    if fields.len() != 5 || fields[0].is_empty() {
        return Err(LumicError::Internal {
            message: "Git returned invalid commit metadata".into(),
        });
    }
    Ok(CommitMetadata {
        id: fields[0].into(),
        author_name: fields[1].into(),
        author_email: fields[2].into(),
        subject: fields[3].into(),
        authored_at: fields[4].into(),
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

fn invalid(field: &str, message: &str) -> LumicError {
    LumicError::InvalidInput {
        field: field.into(),
        message: message.into(),
    }
}

fn validate_deployment_id(value: &str) -> Result<()> {
    if !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        Ok(())
    } else {
        Err(invalid("deployment", "deployment identifier is invalid"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumic_core::{
        OperationContext, OperationInterface,
        application::{ApplicationProcessKind, ApplicationSchedule, HealthCheck},
        pipeline::PipelineStatus,
    };
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

    fn generic_php_spec(root: &Path) -> GenericPhpApplicationSpec {
        GenericPhpApplicationSpec {
            id: "php-app".into(),
            domain: "php-app.example.com".into(),
            www_alias: false,
            root: root.join("php-app").to_string_lossy().into_owned(),
            php_version: "8.3".into(),
            repository: Some(RepositoryConfig {
                url: "https://example.com/php-app.git".into(),
                branch: "main".into(),
                credential_reference: None,
                deployment: Default::default(),
                contract: None,
            }),
            components: vec!["curl".into(), "mbstring".into()],
            databases: Vec::new(),
            packages: vec!["git".into(), "composer".into()],
            tls: true,
            processes: vec![ApplicationProcess {
                name: "queue".into(),
                kind: ApplicationProcessKind::Worker,
                command: vec!["php".into(), "worker.php".into()],
                schedule: None,
                enabled: true,
                environment: Default::default(),
                working_directory: None,
                restart_policy: Default::default(),
                health_check: None,
            }],
            health: HealthCheck {
                enabled: true,
                path: "/health".into(),
                ..HealthCheck::default()
            },
        }
    }

    #[tokio::test]
    async fn plans_and_applies_a_repository_manifest_without_host_mutation() {
        let base = std::env::temp_dir().join(format!(
            "lumic-manifest-test-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        let repository = base.join("repository");
        fs::create_dir_all(&repository).unwrap();
        fs::write(
            repository.join(APPLICATION_MANIFEST_FILE),
            r#"schema_version: 1
name: manifest-app
runtime:
  static: true
build:
  - ["make", "site"]
output: public
workers:
  indexer:
    command: ["bin/indexer", "--watch"]
cron:
  cleanup:
    command: ["bin/cleanup"]
    schedule: "0 2 * * *"
deployment:
  deploy_on_push: true
  retain_releases: 7
health:
  path: /health
  expect: 204
"#,
        )
        .unwrap();
        let service = ApplicationService::new(base.join("state"), base.join("apps"));
        service
            .create(
                "manifest-app",
                "manifest.example.com",
                ApplicationRuntime::Static,
                false,
                &context(),
            )
            .unwrap();
        service
            .set_repository(
                "manifest-app",
                "https://example.com/manifest.git",
                "main",
                None,
                &context(),
            )
            .unwrap();

        let plan = service.plan_manifest("manifest-app", &repository).unwrap();
        assert_eq!(
            plan.changes[0].capability.0.as_str(),
            "application.manifest.apply"
        );
        let applied = service
            .apply_manifest("manifest-app", &repository, &context())
            .await
            .unwrap();
        assert_eq!(applied.release_retention, 7);
        assert_eq!(applied.processes.len(), 2);
        assert_eq!(
            applied
                .repository
                .as_ref()
                .unwrap()
                .contract
                .as_ref()
                .unwrap()
                .public_directory,
            Some(PathBuf::from("public"))
        );
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn dependency_installers_disable_untrusted_lifecycle_code() {
        let root = std::env::temp_dir().join(format!(
            "lumic-dependency-spec-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("composer.json"), b"{}").unwrap();
        let (composer, _) = dependency_install_spec(ApplicationRuntime::Php, None, &root).unwrap();
        assert!(composer.args.iter().any(|arg| arg == "--no-plugins"));
        assert!(composer.args.iter().any(|arg| arg == "--no-scripts"));

        fs::write(root.join("package-lock.json"), b"{}").unwrap();
        let (npm, _) = dependency_install_spec(
            ApplicationRuntime::Node,
            Some(NodePackageManager::Npm),
            &root,
        )
        .unwrap();
        assert!(npm.args.iter().any(|arg| arg == "--ignore-scripts"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn manifest_service_requirements_verify_the_bound_service_type() {
        let base = std::env::temp_dir().join(format!(
            "lumic-service-type-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        let service = ApplicationService::new(base.join("state"), base.join("apps"));
        let mut application = service
            .create(
                "typed-service",
                "typed.example.com",
                ApplicationRuntime::Static,
                false,
                &context(),
            )
            .unwrap();
        application
            .service_references
            .push(ApplicationServiceReference {
                role: "cache".into(),
                service_id: "cache-primary".into(),
                service_type: Some("memcached".into()),
                database: None,
                user: None,
                secret_reference: None,
            });
        let contract = ApplicationManifest::parse(
            "schema_version: 1\nname: typed-service\nruntime:\n  static: true\nservices:\n  cache: redis\n",
        )
        .unwrap()
        .resolve("main")
        .unwrap();
        assert!(validate_manifest_service_bindings(&application, &contract).is_err());
        application.service_references[0].service_type = Some("redis".into());
        assert!(validate_manifest_service_bindings(&application, &contract).is_ok());
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn generic_php_lifecycle_is_planned_journaled_and_resource_backed() {
        let base = std::env::temp_dir().join(format!(
            "lumic-generic-php-lifecycle-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        let state_dir = base.join("state");
        let apps_root = base.join("apps");
        let service = ApplicationService::new(&state_dir, &apps_root);
        let spec = generic_php_spec(&apps_root);

        let install = service
            .plan_generic_php(&spec, ApplicationLifecycleOperation::Install)
            .unwrap();
        assert!(
            install
                .pipeline
                .steps
                .iter()
                .any(|step| step.id == "component-mbstring")
        );
        service
            .create(
                &spec.id,
                &spec.domain,
                ApplicationRuntime::Php,
                spec.www_alias,
                &context(),
            )
            .unwrap();
        assert!(
            service
                .plan_generic_php(&spec, ApplicationLifecycleOperation::Install)
                .is_err()
        );
        let reconcile = service
            .plan_generic_php(&spec, ApplicationLifecycleOperation::Reconcile)
            .unwrap();
        let execution = service
            .begin_generic_php_lifecycle(&reconcile, "php-app-reconcile-1")
            .unwrap();
        assert_eq!(execution.status, PipelineStatus::Running);

        let framework = FrameworkStateStore::at_state_dir(&state_dir)
            .load()
            .unwrap();
        assert!(framework.resources.iter().any(|record| {
            record.resource.kind == ResourceKind::Application
                && record.resource.id == "php-app"
                && record.outputs.contains_key("root")
        }));
        assert_eq!(framework.pipeline_executions, vec![execution]);
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn process_and_schedule_definitions_are_owned_resources_bound_to_the_application() {
        let base = std::env::temp_dir().join(format!(
            "lumic-app-process-resource-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        let state_dir = base.join("state");
        let apps_root = base.join("apps");
        let service = ApplicationService::new(&state_dir, &apps_root);
        let application = service
            .create(
                "scheduled-app",
                "scheduled.example.com",
                ApplicationRuntime::Php,
                false,
                &context(),
            )
            .unwrap();
        let process = ApplicationProcess {
            name: "cron".into(),
            kind: ApplicationProcessKind::Schedule,
            command: vec!["php".into(), "cron.php".into()],
            schedule: Some(ApplicationSchedule::calendar("hourly")),
            enabled: true,
            environment: Default::default(),
            working_directory: None,
            restart_policy: Default::default(),
            health_check: None,
        };
        persist_application_process(
            &state_dir,
            &application,
            &process,
            &ProcessConfigurationResult {
                process: "cron".into(),
                units: vec![
                    "lumic-app-scheduled-app-cron.service".into(),
                    "lumic-app-scheduled-app-cron.timer".into(),
                ],
                changed: true,
            },
        )
        .unwrap();

        let framework = FrameworkStateStore::at_state_dir(&state_dir)
            .load()
            .unwrap();
        let schedule = framework
            .resources
            .iter()
            .find(|record| record.resource.kind == ResourceKind::Schedule)
            .unwrap();
        assert_eq!(schedule.resource.id, "application.scheduled-app.cron");
        assert!(framework.bindings.0.iter().any(|binding| {
            binding.producer == schedule.resource
                && binding.consumer.kind == ResourceKind::Application
        }));
        fs::remove_dir_all(base).unwrap();
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

        service.rollback("example", &context()).await.unwrap();
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

    #[tokio::test]
    async fn runs_explicit_phases_records_provenance_logs_and_redeploys_exact_commit() {
        let base = std::env::temp_dir().join(format!(
            "lumic-workflow-test-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        let source = base.join("source");
        fs::create_dir_all(&source).unwrap();
        git(&source, &["init", "--initial-branch=main"]);
        git(&source, &["config", "user.email", "deploy@lumic.invalid"]);
        git(&source, &["config", "user.name", "Deployment Test"]);
        fs::write(source.join("index.html"), "first").unwrap();
        git(&source, &["add", "index.html"]);
        git(&source, &["commit", "-m", "first release"]);

        let service = ApplicationService::new(base.join("state"), base.join("apps"));
        service
            .create(
                "workflow",
                "workflow.example.com",
                ApplicationRuntime::Static,
                false,
                &context(),
            )
            .unwrap();
        service
            .set_repository(
                "workflow",
                &format!("file://{}", source.display()),
                "main",
                None,
                &context(),
            )
            .unwrap();
        service
            .configure_deployment(
                "workflow",
                DeploymentWorkflow {
                    pre_deploy: vec![vec!["touch".into(), "pre-ran".into()]],
                    build: Some(vec!["touch".into(), "build-ran".into()]),
                    migrate: Some(vec!["touch".into(), "migration-ran".into()]),
                    post_deploy: vec![vec!["touch".into(), "post-ran".into()]],
                    node_handoff: None,
                },
                &context(),
            )
            .unwrap();
        let first = service.deploy("workflow", &context()).await.unwrap();
        assert_eq!(
            first.commit_metadata.as_ref().unwrap().subject,
            "first release"
        );
        assert!(
            Path::new(&first.release_path)
                .join("migration-ran")
                .is_file()
        );
        assert!(Path::new(&first.release_path).join("post-ran").is_file());
        assert!(first.phases.iter().any(|phase| phase.name == "migrate"));
        assert!(
            !service
                .deployment_logs("workflow", &first.id, 0)
                .unwrap()
                .is_empty()
        );

        fs::write(source.join("index.html"), "second").unwrap();
        git(&source, &["add", "index.html"]);
        git(&source, &["commit", "-m", "second release"]);
        let repeated = service
            .redeploy("workflow", &first.id, &context())
            .await
            .unwrap();
        assert_eq!(repeated.commit, first.commit);
        assert_eq!(repeated.retry_of.as_deref(), Some(first.id.as_str()));
        assert_eq!(
            fs::read_to_string(base.join("apps/workflow/current/index.html")).unwrap(),
            "first"
        );
        fs::remove_dir_all(base).unwrap();
    }

    #[tokio::test]
    async fn deployment_uses_the_committed_manifest_public_directory() {
        let base = std::env::temp_dir().join(format!(
            "lumic-manifest-deploy-test-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        let source = base.join("source");
        fs::create_dir_all(source.join("public")).unwrap();
        git(&source, &["init", "--initial-branch=main"]);
        git(&source, &["config", "user.email", "manifest@lumic.invalid"]);
        git(&source, &["config", "user.name", "Manifest Test"]);
        fs::write(source.join("public/index.html"), "manifest release").unwrap();
        fs::write(
            source.join(APPLICATION_MANIFEST_FILE),
            r#"schema_version: 1
name: manifest-deploy
runtime:
  static: true
public: public
deployment:
  deploy_on_push: true
"#,
        )
        .unwrap();
        git(&source, &["add", "."]);
        git(&source, &["commit", "-m", "manifest release"]);

        let service = ApplicationService::new(base.join("state"), base.join("apps"));
        service
            .create(
                "manifest-deploy",
                "manifest-deploy.example.com",
                ApplicationRuntime::Static,
                false,
                &context(),
            )
            .unwrap();
        service
            .set_repository(
                "manifest-deploy",
                &format!("file://{}", source.display()),
                "main",
                None,
                &context(),
            )
            .unwrap();
        service
            .apply_manifest("manifest-deploy", &source, &context())
            .await
            .unwrap();

        let deployment = service.deploy("manifest-deploy", &context()).await.unwrap();
        assert_eq!(deployment.status, DeploymentStatus::Completed);
        assert!(
            deployment
                .phases
                .iter()
                .any(|phase| phase.name == "manifest")
        );
        assert_eq!(
            fs::read_to_string(base.join("apps/manifest-deploy/current/public/index.html"))
                .unwrap(),
            "manifest release"
        );
        assert!(
            service
                .inspect("manifest-deploy")
                .unwrap()
                .repository
                .unwrap()
                .contract
                .is_some()
        );
        fs::remove_dir_all(base).unwrap();
    }

    #[tokio::test]
    async fn serializes_deployments_and_cancels_at_a_phase_boundary() {
        let base = std::env::temp_dir().join(format!(
            "lumic-cancel-test-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        let source = base.join("source");
        fs::create_dir_all(&source).unwrap();
        git(&source, &["init", "--initial-branch=main"]);
        git(&source, &["config", "user.email", "cancel@lumic.invalid"]);
        git(&source, &["config", "user.name", "Cancellation Test"]);
        fs::write(source.join("index.html"), "release").unwrap();
        git(&source, &["add", "index.html"]);
        git(&source, &["commit", "-m", "release"]);

        let service = ApplicationService::new(base.join("state"), base.join("apps"));
        service
            .create(
                "cancel-demo",
                "cancel.example.com",
                ApplicationRuntime::Static,
                false,
                &context(),
            )
            .unwrap();
        service
            .set_repository(
                "cancel-demo",
                &format!("file://{}", source.display()),
                "main",
                None,
                &context(),
            )
            .unwrap();
        service
            .configure_deployment(
                "cancel-demo",
                DeploymentWorkflow {
                    pre_deploy: vec![vec!["sleep".into(), "1".into()]],
                    ..DeploymentWorkflow::default()
                },
                &context(),
            )
            .unwrap();

        let deploying = service.clone();
        let task = tokio::spawn(async move { deploying.deploy("cancel-demo", &context()).await });
        let deployment_id = loop {
            if let Some(item) = service.deployments("cancel-demo").unwrap().first() {
                break item.id.clone();
            }
            sleep(Duration::from_millis(10)).await;
        };
        assert!(service.deploy("cancel-demo", &context()).await.is_err());
        let requested = service
            .cancel_deployment("cancel-demo", &deployment_id, &context())
            .unwrap();
        assert_eq!(requested.status, DeploymentStatus::Cancelling);
        assert!(task.await.unwrap().is_err());
        let cancelled = service.deployments("cancel-demo").unwrap().remove(0);
        assert_eq!(cancelled.status, DeploymentStatus::Cancelled);
        assert!(!base.join("apps/cancel-demo/current").exists());
        fs::remove_dir_all(base).unwrap();
    }
}
