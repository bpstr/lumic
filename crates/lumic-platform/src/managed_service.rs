use crate::{
    ProcessOutput, ProcessRunner, ProcessSpec,
    apt::AptPackageManager,
    atomic_file::{restore_backup, write_atomic},
    audit_store::AuditStore,
    event_store::EventStore,
    framework_state::FrameworkStateStore,
    secret_store::SecretStore,
    service_driver::{DriverRestoreReplacement, ServiceDriverRegistry},
    systemd::{ServiceAction, SystemdServiceManager},
};
use lumic_core::{
    LumicError, OperationContext, Plan, Result,
    application::{ApplicationServiceReference, unix_time_ms},
    binding::Binding,
    catalog::{Catalog, ServiceDefinition},
    events::{AuditRecord, Event},
    managed_service::{
        BackupStatus, BackupVerification, Database, DatabaseUser, DesiredServiceState,
        ManagedService, ManagedServiceKind, ManagedServiceMutation, ManagedServiceState,
        ManagedServiceStatus, ServiceBackup, ServiceConfiguration, ServiceHealth, ServicePaths,
        install_plan, validate_database_identifier, validate_resource_id,
    },
    package::PackageName,
    resource::{ResourceKind, ResourceOutputs, ResourceRecord, ResourceRef},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
struct ConfigurationChange {
    path: PathBuf,
    backup: Option<PathBuf>,
    existed_before: bool,
    changed: bool,
}

#[derive(Debug, Clone)]
struct ManagedServiceStore {
    framework: FrameworkStateStore,
}

impl ManagedServiceStore {
    fn at_state_dir(state_dir: impl AsRef<Path>) -> Self {
        Self {
            framework: FrameworkStateStore::at_state_dir(state_dir),
        }
    }

    fn load(&self) -> Result<ManagedServiceState> {
        self.framework
            .load_managed_service_state(current_state_time())
    }

    fn save(&self, state: &ManagedServiceState) -> Result<()> {
        self.framework
            .save_managed_service_state(state, current_state_time())
    }
}

#[derive(Debug, Clone)]
pub struct ManagedServiceManager {
    state_dir: PathBuf,
    store: ManagedServiceStore,
    secrets: SecretStore,
    packages: AptPackageManager,
    systemd: SystemdServiceManager,
    events: EventStore,
    audit: AuditStore,
    runner: ProcessRunner,
}

impl ManagedServiceManager {
    pub fn at_state_dir(state_dir: impl AsRef<Path>) -> Self {
        let state_dir = state_dir.as_ref().to_path_buf();
        let events = EventStore::at_state_dir(&state_dir);
        Self {
            store: ManagedServiceStore::at_state_dir(&state_dir),
            secrets: SecretStore::at_state_dir(&state_dir),
            packages: AptPackageManager::system(events.clone()),
            systemd: SystemdServiceManager::at_state_dir(&state_dir),
            audit: AuditStore::at_state_dir(&state_dir),
            events,
            runner: ProcessRunner,
            state_dir,
        }
    }

    pub fn list(&self) -> Result<Vec<ManagedService>> {
        Ok(self.store.load()?.services)
    }

    /// Returns the trusted service catalog used by every public adapter.
    pub fn catalog(&self) -> Result<Vec<ServiceDefinition>> {
        Ok(Catalog::built_in()?.services().cloned().collect())
    }

    pub fn schema(&self, definition_id: &str) -> Result<ServiceDefinition> {
        Catalog::built_in()?
            .service(definition_id)
            .cloned()
            .ok_or_else(|| LumicError::InvalidInput {
                field: "definition".into(),
                message: "unknown service catalog definition".into(),
            })
    }

    pub fn plan_catalog_install(&self, id: &str, definition_id: &str) -> Result<Plan> {
        self.plan_install(id, managed_kind_for_definition(definition_id)?)
    }

    pub async fn install_catalog(
        &self,
        id: &str,
        definition_id: &str,
        context: &OperationContext,
    ) -> Result<ManagedServiceMutation> {
        self.install(id, managed_kind_for_definition(definition_id)?, context)
            .await
    }

    pub async fn detect_catalog(&self, definition_id: &str) -> Result<ManagedServiceStatus> {
        self.detect(managed_kind_for_definition(definition_id)?)
            .await
    }

    pub fn databases(&self, id: &str) -> Result<Vec<Database>> {
        self.find_service(id)?;
        Ok(self
            .store
            .load()?
            .databases
            .into_iter()
            .filter(|item| item.service_id == id)
            .collect())
    }

    pub fn users(&self, id: &str) -> Result<Vec<DatabaseUser>> {
        self.find_service(id)?;
        Ok(self
            .store
            .load()?
            .users
            .into_iter()
            .filter(|item| item.service_id == id)
            .collect())
    }

    pub fn backups(&self, id: &str) -> Result<Vec<ServiceBackup>> {
        self.find_service(id)?;
        Ok(self
            .store
            .load()?
            .backups
            .into_iter()
            .filter(|item| item.service_id == id)
            .collect())
    }

    pub fn plan_install(&self, id: &str, kind: ManagedServiceKind) -> Result<Plan> {
        validate_resource_id("service", id)?;
        let managed = self.store.load()?.services.iter().any(|item| item.id == id);
        Ok(install_plan(id, kind, managed))
    }

    pub async fn detect(&self, kind: ManagedServiceKind) -> Result<ManagedServiceStatus> {
        let (package, systemd_unit) = service_definition(kind)?;
        let configuration = default_service_configuration(kind)?;
        let now = unix_time_ms();
        self.inspect_service(ManagedService {
            id: kind.id().into(),
            name: kind.id().into(),
            kind,
            package,
            systemd_unit,
            desired_state: DesiredServiceState::Running,
            configuration,
            secret_references: Vec::new(),
            dependencies: Vec::new(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        })
        .await
    }

    pub async fn inspect(&self, id: &str) -> Result<ManagedServiceStatus> {
        self.inspect_service(self.find_service(id)?).await
    }

    async fn inspect_service(&self, service: ManagedService) -> Result<ManagedServiceStatus> {
        let package = self
            .packages
            .inspect(&PackageName::parse(&service.package)?)
            .await?;
        let systemd = self.systemd.inspect(&service.systemd_unit).await?;
        let (health, health_message) = if systemd.active_state == "active" {
            self.health(&service).await
        } else {
            (
                ServiceHealth::Unhealthy,
                format!("systemd state is {}", systemd.active_state),
            )
        };
        Ok(ManagedServiceStatus {
            service: service.clone(),
            detected: package.installed_version.is_some(),
            version: package.installed_version,
            active_state: systemd.active_state,
            sub_state: systemd.sub_state,
            enabled: systemd.enabled,
            health,
            health_message,
            paths: self.paths(&service)?,
        })
    }

    pub async fn install(
        &self,
        id: &str,
        kind: ManagedServiceKind,
        context: &OperationContext,
    ) -> Result<ManagedServiceMutation> {
        validate_resource_id("service", id)?;
        let (package, systemd_unit) = service_definition(kind)?;
        let existing = self
            .store
            .load()?
            .services
            .into_iter()
            .find(|item| item.id == id);
        if let Some(item) = &existing
            && item.kind != kind
        {
            return Err(LumicError::InvalidInput {
                field: "kind".into(),
                message: "service id is already managed with a different kind".into(),
            });
        }
        let now = unix_time_ms();
        let configuration = default_service_configuration(kind)?;
        let mut service = existing.clone().unwrap_or_else(|| ManagedService {
            id: id.into(),
            name: id.into(),
            kind,
            package: package.clone(),
            systemd_unit: systemd_unit.clone(),
            desired_state: DesiredServiceState::Running,
            configuration,
            secret_references: Vec::new(),
            dependencies: Vec::new(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        });
        let registry = ServiceDriverRegistry::built_in()?;
        let driver = registry.legacy_driver(kind)?;
        for secret_name in driver.secret_names() {
            let reference = service_secret_reference(id, secret_name);
            if !service.secret_references.contains(&reference) {
                service.secret_references.push(reference);
            }
        }
        service.configuration.validate()?;
        let package = PackageName::parse(&package)?;
        let package_mutation = self
            .packages
            .install_with_environment(&package, context, driver.package_install_environment())
            .await?;
        if context.dry_run {
            return Ok(ManagedServiceMutation {
                service,
                action: "install".into(),
                changed: false,
                message: "dry run: package, configuration, enable, start, and health validation"
                    .into(),
            });
        }
        if self
            .packages
            .inspect(&package)
            .await?
            .installed_version
            .is_none()
        {
            return Err(LumicError::Process {
                executable: "apt-get".into(),
                message: format!(
                    "package '{}' was not installed after apt completed",
                    package
                ),
            });
        }
        let created_secrets = self.create_missing_service_secrets(&service, driver)?;
        let configured = match self.write_configuration(&service).await {
            Ok(configured) => configured,
            Err(error) => {
                self.delete_secrets(&created_secrets)?;
                return Err(error);
            }
        };
        if configuration_requires_daemon_reload(&configured)
            && let Err(error) = self.systemd.daemon_reload().await
        {
            self.restore_configuration(&configured)?;
            let _ = self.systemd.daemon_reload().await;
            self.delete_secrets(&created_secrets)?;
            return Err(error);
        }
        if let Err(error) = self
            .systemd
            .apply(&systemd_unit, ServiceAction::Enable, context)
            .await
        {
            self.restore_configuration(&configured)?;
            if configuration_requires_daemon_reload(&configured) {
                let _ = self.systemd.daemon_reload().await;
            }
            self.delete_secrets(&created_secrets)?;
            return Err(error);
        }
        if let Err(error) = self
            .systemd
            .apply(&systemd_unit, ServiceAction::Restart, context)
            .await
        {
            self.restore_configuration(&configured)?;
            if configuration_requires_daemon_reload(&configured) {
                let _ = self.systemd.daemon_reload().await;
            }
            self.delete_secrets(&created_secrets)?;
            return Err(error);
        }
        let (health, message) = self.health(&service).await;
        if health != ServiceHealth::Healthy {
            self.restore_configuration(&configured)?;
            if configuration_requires_daemon_reload(&configured) {
                let _ = self.systemd.daemon_reload().await;
            }
            self.delete_secrets(&created_secrets)?;
            return Err(LumicError::Process {
                executable: systemd_unit,
                message: format!("service failed post-install health validation: {message}"),
            });
        }
        self.upsert_service(service.clone())?;
        let changed = existing.is_none()
            || package_mutation.changed
            || configured.iter().any(|item| item.changed);
        self.record(
            "managed_service.installed",
            "install",
            &service,
            context,
            changed,
            json!({"kind": kind, "health": health}),
        )?;
        Ok(ManagedServiceMutation {
            service,
            action: "install".into(),
            changed,
            message: format!("healthy: {message}"),
        })
    }

    pub async fn lifecycle(
        &self,
        id: &str,
        action: ServiceAction,
        context: &OperationContext,
    ) -> Result<ManagedServiceMutation> {
        if !matches!(
            action,
            ServiceAction::Start | ServiceAction::Stop | ServiceAction::Restart
        ) {
            return Err(invalid(
                "action",
                "managed services support start, stop, and restart",
            ));
        }
        let mut service = self.find_service(id)?;
        let mutation = self
            .systemd
            .apply(&service.systemd_unit, action, context)
            .await?;
        let message = if action == ServiceAction::Stop || context.dry_run {
            if action == ServiceAction::Stop {
                "service stopped".into()
            } else {
                "dry run: lifecycle action would be applied and health-checked".into()
            }
        } else {
            let (health, message) = self.health(&service).await;
            if health != ServiceHealth::Healthy {
                return Err(LumicError::Process {
                    executable: service.systemd_unit.clone(),
                    message: format!("service failed post-{action:?} health validation: {message}"),
                });
            }
            format!("healthy: {message}")
        };
        if !context.dry_run {
            service.desired_state = if action == ServiceAction::Stop {
                DesiredServiceState::Stopped
            } else {
                DesiredServiceState::Running
            };
            service.updated_at_unix_ms = unix_time_ms();
            self.upsert_service(service.clone())?;
        }
        self.record(
            &format!("managed_service.{action:?}").to_ascii_lowercase(),
            &format!("{action:?}").to_ascii_lowercase(),
            &service,
            context,
            mutation.changed,
            json!({"active_state": mutation.after.active_state}),
        )?;
        Ok(ManagedServiceMutation {
            service,
            action: format!("{action:?}").to_ascii_lowercase(),
            changed: mutation.changed,
            message,
        })
    }

    pub fn declare_dependency(
        &self,
        id: &str,
        dependency_id: &str,
        purpose: &str,
        required: bool,
        context: &OperationContext,
    ) -> Result<ManagedServiceMutation> {
        if id == dependency_id {
            return Err(invalid("dependency", "a service cannot depend on itself"));
        }
        if purpose.is_empty()
            || purpose.len() > 120
            || purpose.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(invalid("purpose", "must be bounded single-line text"));
        }
        let mut service = self.find_service(id)?;
        self.find_service(dependency_id)?;
        let dependency = lumic_core::managed_service::ServiceDependency {
            service_id: dependency_id.into(),
            required,
            purpose: purpose.into(),
        };
        let changed = !service.dependencies.iter().any(|item| item == &dependency);
        if changed && !context.dry_run {
            service
                .dependencies
                .retain(|item| item.service_id != dependency_id);
            service.dependencies.push(dependency.clone());
            service.updated_at_unix_ms = unix_time_ms();
            self.upsert_service(service.clone())?;
        }
        self.record(
            "managed_service.dependency_declared",
            "declare_dependency",
            &service,
            context,
            changed,
            json!({"dependency": dependency}),
        )?;
        Ok(ManagedServiceMutation {
            service,
            action: "declare_dependency".into(),
            changed,
            message: if context.dry_run {
                "dry run: dependency would be declared".into()
            } else if changed {
                "dependency declared".into()
            } else {
                "dependency already declared".into()
            },
        })
    }

    pub async fn update(
        &self,
        id: &str,
        context: &OperationContext,
    ) -> Result<ManagedServiceMutation> {
        let service = self.find_service(id)?;
        let mutation = self
            .packages
            .upgrade(&PackageName::parse(&service.package)?, context)
            .await?;
        if mutation.changed && !context.dry_run {
            self.systemd
                .apply(&service.systemd_unit, ServiceAction::Restart, context)
                .await?;
        }
        self.record(
            "managed_service.updated",
            "update",
            &service,
            context,
            mutation.changed,
            json!({}),
        )?;
        Ok(ManagedServiceMutation {
            service,
            action: "update".into(),
            changed: mutation.changed,
            message: mutation.output,
        })
    }

    pub async fn remove(
        &self,
        id: &str,
        purge_data: bool,
        context: &OperationContext,
    ) -> Result<ManagedServiceMutation> {
        if purge_data {
            return Err(invalid(
                "purge_data",
                "data purge is intentionally separate; take a backup and remove native data explicitly",
            ));
        }
        let service = self.find_service(id)?;
        let framework = FrameworkStateStore::at_state_dir(&self.state_dir)
            .load_or_migrate(current_state_time())?;
        framework
            .bindings
            .assert_removable(&ResourceRef::new(ResourceKind::ManagedService, id)?)?;
        for resource in framework.resources.iter().filter(|resource| {
            resource
                .attributes
                .get("provider_service_id")
                .and_then(Value::as_str)
                == Some(id)
        }) {
            framework.bindings.assert_removable(&resource.resource)?;
        }
        if !context.dry_run {
            self.systemd
                .apply(&service.systemd_unit, ServiceAction::Stop, context)
                .await?;
            self.systemd
                .apply(&service.systemd_unit, ServiceAction::Disable, context)
                .await?;
        }
        let mutation = self
            .packages
            .remove(&PackageName::parse(&service.package)?, context)
            .await?;
        if !context.dry_run {
            let mut state = self.store.load()?;
            state.services.retain(|item| item.id != id);
            state.databases.retain(|item| item.service_id != id);
            state.users.retain(|item| item.service_id != id);
            self.store.save(&state)?;
        }
        self.record(
            "managed_service.removed",
            "remove",
            &service,
            context,
            mutation.changed,
            json!({"data_retained": true}),
        )?;
        Ok(ManagedServiceMutation {
            service,
            action: "remove".into(),
            changed: mutation.changed,
            message: "native data retained for explicit recovery or removal".into(),
        })
    }

    pub async fn configure(
        &self,
        id: &str,
        configuration: ServiceConfiguration,
        context: &OperationContext,
    ) -> Result<ManagedServiceMutation> {
        configuration.validate()?;
        self.validate_settings(self.find_service(id)?.kind, &configuration)?;
        let mut service = self.find_service(id)?;
        if service.configuration == configuration {
            return Ok(ManagedServiceMutation {
                service,
                action: "configure".into(),
                changed: false,
                message: "configuration already matches".into(),
            });
        }
        if context.dry_run {
            service.configuration = configuration;
            return Ok(ManagedServiceMutation {
                service,
                action: "configure".into(),
                changed: false,
                message: "dry run: configuration would be written and health-checked".into(),
            });
        }
        let before = service.configuration.clone();
        service.configuration = configuration;
        let backup = self.write_configuration(&service).await?;
        if configuration_requires_daemon_reload(&backup)
            && let Err(error) = self.systemd.daemon_reload().await
        {
            self.restore_configuration(&backup)?;
            let _ = self.systemd.daemon_reload().await;
            return Err(error);
        }
        if let Err(error) = self
            .systemd
            .apply(&service.systemd_unit, ServiceAction::Restart, context)
            .await
        {
            self.restore_configuration(&backup)?;
            if configuration_requires_daemon_reload(&backup) {
                let _ = self.systemd.daemon_reload().await;
            }
            let _ = self
                .systemd
                .apply(&service.systemd_unit, ServiceAction::Restart, context)
                .await;
            return Err(error);
        }
        let (health, message) = self.health(&service).await;
        if health != ServiceHealth::Healthy {
            self.restore_configuration(&backup)?;
            if configuration_requires_daemon_reload(&backup) {
                self.systemd.daemon_reload().await?;
            }
            self.systemd
                .apply(&service.systemd_unit, ServiceAction::Restart, context)
                .await?;
            return Err(LumicError::Process {
                executable: service.systemd_unit.clone(),
                message: format!("configuration rolled back after failed health check: {message}"),
            });
        }
        service.updated_at_unix_ms = unix_time_ms();
        self.upsert_service(service.clone())?;
        self.record(
            "managed_service.configured",
            "configure",
            &service,
            context,
            true,
            json!({"before": before, "after": service.configuration}),
        )?;
        Ok(ManagedServiceMutation {
            service,
            action: "configure".into(),
            changed: true,
            message: format!("healthy: {message}"),
        })
    }

    pub async fn logs(&self, id: &str, lines: usize) -> Result<String> {
        if !(1..=1_000).contains(&lines) {
            return Err(invalid("lines", "must be between 1 and 1000"));
        }
        let service = self.find_service(id)?;
        let output = self
            .run(ProcessSpec::new("journalctl").args([
                "--unit",
                &service.systemd_unit,
                "--no-pager",
                "--lines",
                &lines.to_string(),
            ]))
            .await?;
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    pub async fn create_database(
        &self,
        service_id: &str,
        name: &str,
        owner: Option<&str>,
        context: &OperationContext,
    ) -> Result<Database> {
        validate_database_identifier("database", name)?;
        if let Some(owner) = owner {
            validate_database_identifier("owner", owner)?;
        }
        let service = self.find_service(service_id)?;
        let registry = ServiceDriverRegistry::built_in()?;
        let driver = registry.legacy_driver(service.kind)?;
        let mut state = self.store.load()?;
        if let Some(existing) = state
            .databases
            .iter()
            .find(|item| item.service_id == service_id && item.name == name)
        {
            return Ok(existing.clone());
        }
        let database = Database {
            id: format!("{service_id}-{name}"),
            service_id: service_id.into(),
            name: name.into(),
            owner: owner.map(str::to_owned),
            created_at_unix_ms: unix_time_ms(),
        };
        if context.dry_run {
            return Ok(database);
        }
        self.run(driver.create_database_command(name, owner)?)
            .await?;
        state.databases.push(database.clone());
        self.store.save(&state)?;
        self.record(
            "database.created",
            "create_database",
            &service,
            context,
            true,
            json!({"database": name, "owner": owner}),
        )?;
        Ok(database)
    }

    pub async fn create_database_user(
        &self,
        service_id: &str,
        name: &str,
        context: &OperationContext,
    ) -> Result<DatabaseUser> {
        validate_database_identifier("user", name)?;
        let service = self.find_service(service_id)?;
        let registry = ServiceDriverRegistry::built_in()?;
        let driver = registry.legacy_driver(service.kind)?;
        let mut state = self.store.load()?;
        if let Some(existing) = state
            .users
            .iter()
            .find(|item| item.service_id == service_id && item.name == name)
        {
            return Ok(existing.clone());
        }
        let secret_reference = database_user_secret_reference(service_id, name);
        let now = unix_time_ms();
        let user = DatabaseUser {
            id: format!("{service_id}-{name}"),
            service_id: service_id.into(),
            name: name.into(),
            secret_reference: secret_reference.clone(),
            databases: Vec::new(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };
        if context.dry_run {
            return Ok(user);
        }
        self.secrets.create(&secret_reference)?;
        let password = self.secrets.read(&secret_reference)?;
        let password = String::from_utf8(password).map_err(|_| LumicError::Internal {
            message: "generated secret is not UTF-8".into(),
        })?;
        if let Err(error) = self.run(driver.create_user_command(name, &password)?).await {
            self.secrets.delete(&secret_reference)?;
            return Err(error);
        }
        state.users.push(user.clone());
        if let Some(item) = state.services.iter_mut().find(|item| item.id == service_id) {
            item.secret_references.push(secret_reference.clone());
        }
        self.store.save(&state)?;
        self.record(
            "database.user_created",
            "create_user",
            &service,
            context,
            true,
            json!({"user": name, "secret_reference": secret_reference}),
        )?;
        Ok(user)
    }

    pub async fn grant_database(
        &self,
        service_id: &str,
        database: &str,
        user: &str,
        context: &OperationContext,
    ) -> Result<DatabaseUser> {
        validate_database_identifier("database", database)?;
        validate_database_identifier("user", user)?;
        let service = self.find_service(service_id)?;
        let registry = ServiceDriverRegistry::built_in()?;
        let driver = registry.legacy_driver(service.kind)?;
        let mut state = self.store.load()?;
        if !state
            .databases
            .iter()
            .any(|item| item.service_id == service_id && item.name == database)
        {
            return Err(invalid(
                "database",
                "database is not managed by this service",
            ));
        }
        let item = state
            .users
            .iter_mut()
            .find(|item| item.service_id == service_id && item.name == user)
            .ok_or_else(|| invalid("user", "database user is not managed by this service"))?;
        if item.databases.iter().any(|item| item == database) {
            return Ok(item.clone());
        }
        if context.dry_run {
            return Ok(item.clone());
        }
        self.run(driver.grant_database_command(database, user)?)
            .await?;
        item.databases.push(database.into());
        item.updated_at_unix_ms = unix_time_ms();
        let item = item.clone();
        self.store.save(&state)?;
        self.record(
            "database.granted",
            "grant",
            &service,
            context,
            true,
            json!({"database": database, "user": user}),
        )?;
        Ok(item)
    }

    pub async fn backup(
        &self,
        service_id: &str,
        database: Option<&str>,
        context: &OperationContext,
    ) -> Result<ServiceBackup> {
        let service = self.find_service(service_id)?;
        let now = unix_time_ms();
        let backup_id = match database {
            Some(name) => {
                validate_database_identifier("database", name)?;
                format!("{service_id}-{name}-{now}")
            }
            None => format!("{service_id}-{now}"),
        };
        let directory = PathBuf::from("/var/backups/lumic").join(service_id);
        let registry = ServiceDriverRegistry::built_in()?;
        let driver = registry.legacy_driver(service.kind)?;
        let plan = driver.backup_plan(&service, database, &directory, &backup_id)?;
        let path = plan.path.clone();
        if context.dry_run {
            return Ok(ServiceBackup {
                id: backup_id,
                service_id: service_id.into(),
                database: database.map(str::to_owned),
                path: path.to_string_lossy().into_owned(),
                size_bytes: 0,
                checksum_sha256: None,
                status: BackupStatus::Completed,
                created_at_unix_ms: now,
                message: "dry run: local backup would be created".into(),
            });
        }
        fs::create_dir_all(&directory).map_err(state_io)?;
        for command in plan.commands {
            self.run(command).await?;
        }
        if let Some(source) = plan.copy_source {
            fs::copy(source, &path).map_err(state_io)?;
        }
        let backup = ServiceBackup {
            id: backup_id,
            service_id: service_id.into(),
            database: database.map(str::to_owned),
            path: path.to_string_lossy().into_owned(),
            size_bytes: fs::metadata(&path).map_err(state_io)?.len(),
            checksum_sha256: Some(file_sha256(&path)?),
            status: BackupStatus::Completed,
            created_at_unix_ms: now,
            message: "local backup completed".into(),
        };
        let mut state = self.store.load()?;
        state.backups.push(backup.clone());
        self.store.save(&state)?;
        self.record(
            "managed_service.backup_completed",
            "backup",
            &service,
            context,
            true,
            json!({"backup_id": backup.id, "database": database}),
        )?;
        Ok(backup)
    }

    pub fn verify_backup(&self, backup_id: &str) -> Result<BackupVerification> {
        validate_backup_id(backup_id)?;
        let state = self.store.load()?;
        let backup = state
            .backups
            .iter()
            .find(|item| item.id == backup_id)
            .ok_or_else(|| invalid("backup", "backup is not managed by Lumic"))?;
        let path = Path::new(&backup.path);
        if !path.is_file() {
            return Ok(BackupVerification {
                backup_id: backup.id.clone(),
                verified_at_unix_ms: unix_time_ms(),
                exists: false,
                size_matches: false,
                checksum_matches: backup.checksum_sha256.as_ref().map(|_| false),
                format_valid: false,
                checksum_sha256: None,
                message: "backup file is missing".into(),
            });
        }
        let size_matches = fs::metadata(path).map_err(state_io)?.len() == backup.size_bytes;
        let checksum = file_sha256(path)?;
        let checksum_matches = backup
            .checksum_sha256
            .as_ref()
            .map(|expected| expected == &checksum);
        let mut header = [0_u8; 16];
        let read = fs::File::open(path)
            .and_then(|mut file| file.read(&mut header))
            .map_err(state_io)?;
        let extension = path.extension().and_then(|value| value.to_str());
        let format_valid = match extension {
            Some("dump") => read >= 5 && &header[..5] == b"PGDMP",
            Some("rdb") => read >= 5 && &header[..5] == b"REDIS",
            Some("sql") => {
                read >= 12
                    && (header.starts_with(b"-- MySQL dump") || header.starts_with(b"/*M!999999"))
            }
            _ => false,
        };
        let verified = size_matches && checksum_matches.unwrap_or(true) && format_valid;
        Ok(BackupVerification {
            backup_id: backup.id.clone(),
            verified_at_unix_ms: unix_time_ms(),
            exists: true,
            size_matches,
            checksum_matches,
            format_valid,
            checksum_sha256: Some(checksum),
            message: if verified {
                "backup size, checksum, and native format header verified".into()
            } else {
                "backup verification failed; do not restore this artifact".into()
            },
        })
    }

    pub async fn restore(
        &self,
        service_id: &str,
        backup_id: &str,
        context: &OperationContext,
    ) -> Result<ServiceBackup> {
        validate_backup_id(backup_id)?;
        let service = self.find_service(service_id)?;
        let mut state = self.store.load()?;
        let source = state
            .backups
            .iter()
            .find(|item| item.id == backup_id && item.service_id == service_id)
            .cloned()
            .ok_or_else(|| invalid("backup", "backup is not managed by this service"))?;
        if !Path::new(&source.path).is_file() {
            return Err(invalid("backup", "backup file is missing"));
        }
        if context.dry_run {
            return Ok(source);
        }
        let registry = ServiceDriverRegistry::built_in()?;
        let driver = registry.legacy_driver(service.kind)?;
        let plan = driver.restore_plan(Path::new(&source.path), source.database.as_deref())?;
        if plan.stop_service {
            self.systemd
                .apply(&service.systemd_unit, ServiceAction::Stop, context)
                .await?;
        }
        let had_target = plan
            .replacement
            .as_ref()
            .map(|replacement| replacement.target.is_file());
        if let Some(replacement) = &plan.replacement
            && had_target == Some(true)
            && let Err(error) =
                fs::copy(&replacement.target, &replacement.safety_copy).map_err(state_io)
        {
            if plan.stop_service {
                let _ = self
                    .systemd
                    .apply(&service.systemd_unit, ServiceAction::Start, context)
                    .await;
            }
            return Err(error);
        }
        let apply_result = async {
            for command in plan.commands {
                self.run(command).await?;
            }
            if let Some(replacement) = &plan.replacement {
                fs::copy(&source.path, &replacement.target).map_err(state_io)?;
                self.set_configuration_owner(&replacement.target, replacement.owner)
                    .await?;
            }
            if plan.stop_service {
                self.systemd
                    .apply(&service.systemd_unit, ServiceAction::Start, context)
                    .await?;
            }
            let (health, message) = self.health(&service).await;
            if health != ServiceHealth::Healthy {
                return Err(LumicError::Process {
                    executable: service.systemd_unit.clone(),
                    message: format!("restore completed but health validation failed: {message}"),
                });
            }
            Ok(())
        }
        .await;
        if let Err(error) = apply_result {
            if let (Some(replacement), Some(had_target)) = (&plan.replacement, had_target) {
                self.recover_restore(replacement, had_target, &service, context)
                    .await
                    .map_err(|recovery| LumicError::Internal {
                        message: format!(
                            "restore failed ({error}); recovery also failed ({recovery})"
                        ),
                    })?;
            }
            return Err(error);
        }
        let restored = ServiceBackup {
            id: format!("restore-{backup_id}-{}", unix_time_ms()),
            status: BackupStatus::Restored,
            created_at_unix_ms: unix_time_ms(),
            message: "local backup restored".into(),
            ..source
        };
        state.backups.push(restored.clone());
        self.store.save(&state)?;
        self.record(
            "managed_service.backup_restored",
            "restore",
            &service,
            context,
            true,
            json!({"backup_id": backup_id}),
        )?;
        Ok(restored)
    }

    pub fn attach_to_application(
        &self,
        application_service: &crate::application::ApplicationService,
        application: &str,
        mut reference: ApplicationServiceReference,
        context: &OperationContext,
    ) -> Result<lumic_core::application::Application> {
        let state = self.store.load()?;
        let managed_service = state
            .services
            .iter()
            .find(|item| item.id == reference.service_id)
            .ok_or_else(|| not_found(&reference.service_id))?;
        let is_search = matches!(
            managed_service.kind,
            ManagedServiceKind::Typesense | ManagedServiceKind::Meilisearch
        );
        if is_search && (reference.database.is_some() || reference.user.is_some()) {
            return Err(invalid(
                "service_reference",
                "search services expose an endpoint and credential, not a database or user",
            ));
        }
        if let Some(database) = &reference.database
            && !state
                .databases
                .iter()
                .any(|item| item.service_id == reference.service_id && item.name == *database)
        {
            return Err(invalid(
                "database",
                "database is not managed by this service",
            ));
        }
        let user = if let Some(name) = &reference.user {
            let user = state
                .users
                .iter()
                .find(|item| item.service_id == reference.service_id && item.name == *name)
                .ok_or_else(|| invalid("user", "user is not managed by this service"))?;
            if let Some(database) = &reference.database
                && !user.databases.iter().any(|granted| granted == database)
            {
                return Err(invalid(
                    "user",
                    "database user has not been granted access to the selected database",
                ));
            }
            Some(user)
        } else {
            None
        };
        reference.secret_reference = if is_search {
            let secret_name = search_secret_name(managed_service.kind).expect("search kind");
            let secret_reference = service_secret_reference(&managed_service.id, secret_name);
            if !managed_service
                .secret_references
                .contains(&secret_reference)
            {
                return Err(invalid(
                    "secret_reference",
                    "search service is missing its managed credential",
                ));
            }
            Some(secret_reference)
        } else {
            user.map(|user| user.secret_reference.clone())
        };
        let current_application = application_service.inspect(application)?;
        if context.dry_run {
            return Ok(current_application);
        }

        // Upgrade older schema-v2 child resources with the typed outputs used below.
        self.store.save(&state)?;
        let framework_store = FrameworkStateStore::at_state_dir(&self.state_dir);
        let previous_framework = framework_store.load_or_migrate(current_state_time())?;
        let mut framework = previous_framework.clone();
        persist_application_service_bindings(
            &mut framework,
            &current_application,
            managed_service,
            &reference,
            current_state_time(),
        )?;
        framework_store.save(&framework)?;

        match application_service.attach_service(application, reference, context) {
            Ok(application) => Ok(application),
            Err(error) => {
                framework_store.save(&previous_framework)?;
                Err(error)
            }
        }
    }

    fn find_service(&self, id: &str) -> Result<ManagedService> {
        validate_resource_id("service", id)?;
        self.store
            .load()?
            .services
            .into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| not_found(id))
    }

    fn upsert_service(&self, service: ManagedService) -> Result<()> {
        let mut state = self.store.load()?;
        if let Some(existing) = state.services.iter_mut().find(|item| item.id == service.id) {
            *existing = service;
        } else {
            state.services.push(service);
        }
        self.store.save(&state)
    }

    fn paths(&self, service: &ManagedService) -> Result<ServicePaths> {
        let registry = ServiceDriverRegistry::built_in()?;
        let driver = registry.legacy_driver(service.kind)?;
        let discovered = self.postgresql_config_path();
        Ok(driver.paths(service, discovered))
    }

    async fn write_configuration(
        &self,
        service: &ManagedService,
    ) -> Result<Vec<ConfigurationChange>> {
        self.validate_settings(service.kind, &service.configuration)?;
        let registry = ServiceDriverRegistry::built_in()?;
        let driver = registry.legacy_driver(service.kind)?;
        let mut secrets = BTreeMap::new();
        for name in driver.secret_names() {
            let reference = service_secret_reference(&service.id, name);
            if !service.secret_references.contains(&reference) {
                return Err(invalid(
                    "secret_reference",
                    &format!("managed service is missing required secret '{name}'"),
                ));
            }
            let value = String::from_utf8(self.secrets.read(&reference)?)
                .map_err(|_| invalid("secret_reference", "managed service secret is not UTF-8"))?;
            if value.is_empty() || value.len() > 4_096 || value.chars().any(char::is_control) {
                return Err(invalid(
                    "secret_reference",
                    "managed service secret must be bounded single-line text",
                ));
            }
            secrets.insert((*name).to_owned(), value);
        }
        let files = driver.configuration_files(service, self.postgresql_config_path(), &secrets)?;
        let mut changes = Vec::new();
        for file in files {
            let path = file.path;
            let existed_before = path.is_file();
            let result = match write_atomic(&path, file.content.as_bytes(), file.mode) {
                Ok(result) => result,
                Err(error) => {
                    self.restore_configuration(&changes)?;
                    return Err(error);
                }
            };
            changes.push(ConfigurationChange {
                path: path.clone(),
                backup: result.backup,
                existed_before,
                changed: result.changed,
            });
            if let Err(error) = self.set_configuration_owner(&path, file.owner).await {
                self.restore_configuration(&changes)?;
                return Err(error);
            }
        }
        Ok(changes)
    }

    fn create_missing_service_secrets(
        &self,
        service: &ManagedService,
        driver: &dyn crate::service_driver::ServiceDriver,
    ) -> Result<Vec<String>> {
        let mut created = Vec::new();
        for name in driver.secret_names() {
            let reference = service_secret_reference(&service.id, name);
            if !self.secrets.exists(&reference)? {
                if let Err(error) = self.secrets.create(&reference) {
                    self.delete_secrets(&created)?;
                    return Err(error);
                }
                created.push(reference);
            }
        }
        Ok(created)
    }

    fn delete_secrets(&self, references: &[String]) -> Result<()> {
        for reference in references {
            self.secrets.delete(reference)?;
        }
        Ok(())
    }

    fn restore_configuration(&self, changes: &[ConfigurationChange]) -> Result<()> {
        for change in changes.iter().rev().filter(|item| item.changed) {
            if let Some(backup) = &change.backup {
                restore_backup(&change.path, backup)?;
            } else if !change.existed_before && change.path.exists() {
                fs::remove_file(&change.path).map_err(state_io)?;
            }
        }
        Ok(())
    }

    async fn set_configuration_owner(&self, path: &Path, owner: &str) -> Result<()> {
        let path = path.to_string_lossy().into_owned();
        self.run(ProcessSpec::new("chown").args([owner, path.as_str()]))
            .await?;
        Ok(())
    }

    async fn recover_restore(
        &self,
        replacement: &DriverRestoreReplacement,
        had_target: bool,
        service: &ManagedService,
        context: &OperationContext,
    ) -> Result<()> {
        let _ = self
            .systemd
            .apply(&service.systemd_unit, ServiceAction::Stop, context)
            .await;
        if had_target {
            if !replacement.safety_copy.is_file() {
                return Err(invalid("backup", "restore recovery snapshot is missing"));
            }
            fs::copy(&replacement.safety_copy, &replacement.target).map_err(state_io)?;
            self.set_configuration_owner(&replacement.target, replacement.owner)
                .await?;
        } else if replacement.target.exists() {
            fs::remove_file(&replacement.target).map_err(state_io)?;
        }
        self.systemd
            .apply(&service.systemd_unit, ServiceAction::Start, context)
            .await?;
        let (health, message) = self.health(service).await;
        if health != ServiceHealth::Healthy {
            return Err(LumicError::Process {
                executable: service.systemd_unit.clone(),
                message: format!("restore recovery failed health validation: {message}"),
            });
        }
        Ok(())
    }

    fn postgresql_config_path(&self) -> Option<PathBuf> {
        let root = Path::new("/etc/postgresql");
        let mut versions: Vec<_> = fs::read_dir(root)
            .ok()?
            .filter_map(|item| item.ok())
            .collect();
        versions.sort_by_key(|item| item.file_name());
        for version in versions.into_iter().rev() {
            let mut clusters: Vec<_> = fs::read_dir(version.path())
                .ok()?
                .filter_map(|item| item.ok())
                .collect();
            clusters.sort_by_key(|item| item.file_name());
            for cluster in clusters {
                let directory = cluster.path().join("conf.d");
                if directory.is_dir() {
                    return Some(directory.join("99-lumic.conf"));
                }
            }
        }
        None
    }

    fn validate_settings(
        &self,
        kind: ManagedServiceKind,
        configuration: &ServiceConfiguration,
    ) -> Result<()> {
        ServiceDriverRegistry::built_in()?
            .legacy_driver(kind)?
            .validate_configuration(configuration)
    }

    async fn health(&self, service: &ManagedService) -> (ServiceHealth, String) {
        let probe = match ServiceDriverRegistry::built_in()
            .and_then(|registry| Ok(registry.legacy_driver(service.kind)?.health_probe(service)))
        {
            Ok(probe) => probe,
            Err(error) => return (ServiceHealth::Unhealthy, error.to_string()),
        };
        let result = self.run(probe).await;
        match result {
            Ok(output) => (
                ServiceHealth::Healthy,
                String::from_utf8_lossy(&output.stdout).trim().into(),
            ),
            Err(error) => (ServiceHealth::Unhealthy, error.to_string()),
        }
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

    fn record(
        &self,
        event_type: &str,
        action: &str,
        service: &ManagedService,
        context: &OperationContext,
        changed: bool,
        payload: serde_json::Value,
    ) -> Result<()> {
        self.events.append(&Event::now(
            event_type,
            &context.actor,
            context.interface,
            "managed_service",
            &service.id,
            &context.correlation_id,
            payload,
        ))?;
        self.audit.append(&AuditRecord::now(
            context,
            format!("managed_service.{action}"),
            action,
            "managed_service",
            &service.id,
            json!({"service_id": service.id, "kind": service.kind}),
            None,
            Some(json!({"changed": changed})),
            true,
            "managed-service operation completed",
        ))
    }

    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }
}

fn file_sha256(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).map_err(state_io)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(state_io)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn invalid(field: &str, message: &str) -> LumicError {
    LumicError::InvalidInput {
        field: field.into(),
        message: message.into(),
    }
}

fn validate_backup_id(value: &str) -> Result<()> {
    let valid = !value.is_empty()
        && value.len() <= 192
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        });
    if valid {
        Ok(())
    } else {
        Err(invalid(
            "backup",
            "backup id contains unsupported characters",
        ))
    }
}

fn database_user_secret_reference(service_id: &str, user: &str) -> String {
    format!("{service_id}-{}-password", user.replace('_', "-"))
}

fn service_secret_reference(service_id: &str, secret_name: &str) -> String {
    format!("{service_id}-{}", secret_name.replace('_', "-"))
}

fn configuration_requires_daemon_reload(changes: &[ConfigurationChange]) -> bool {
    changes
        .iter()
        .any(|change| change.changed && change.path.starts_with("/etc/systemd/system"))
}

fn not_found(id: &str) -> LumicError {
    invalid("service", &format!("managed service '{id}' was not found"))
}

fn managed_kind_for_definition(definition_id: &str) -> Result<ManagedServiceKind> {
    let definition = Catalog::built_in()?
        .service(definition_id)
        .cloned()
        .ok_or_else(|| LumicError::InvalidInput {
            field: "definition".into(),
            message: "unknown service catalog definition".into(),
        })?;
    match definition.driver.as_str() {
        "mysql" => Ok(ManagedServiceKind::Mysql),
        "postgresql" => Ok(ManagedServiceKind::Postgresql),
        "redis" => Ok(ManagedServiceKind::Redis),
        "typesense" => Ok(ManagedServiceKind::Typesense),
        "meilisearch" => Ok(ManagedServiceKind::Meilisearch),
        "valkey" => Ok(ManagedServiceKind::Valkey),
        "rabbitmq" => Ok(ManagedServiceKind::Rabbitmq),
        "minio" => Ok(ManagedServiceKind::Minio),
        "opensearch" => Ok(ManagedServiceKind::Opensearch),
        "memcached" => Ok(ManagedServiceKind::Memcached),
        "mongodb" => Ok(ManagedServiceKind::Mongodb),
        "clickhouse" => Ok(ManagedServiceKind::Clickhouse),
        "prometheus" => Ok(ManagedServiceKind::Prometheus),
        "grafana" => Ok(ManagedServiceKind::Grafana),
        "loki" => Ok(ManagedServiceKind::Loki),
        _ => Err(LumicError::InvalidInput {
            field: "definition".into(),
            message: format!(
                "catalog definition '{}' has no managed-service driver",
                definition.id
            ),
        }),
    }
}

fn service_definition(kind: ManagedServiceKind) -> Result<(String, String)> {
    let registry = ServiceDriverRegistry::built_in()?;
    let definition = registry.definition(kind.id())?;
    let platform = definition
        .platforms
        .iter()
        .find(|platform| matches!(platform.distribution.as_str(), "debian" | "ubuntu"))
        .ok_or_else(|| invalid("service.platform", "no supported native platform mapping"))?;
    Ok((platform.package.clone(), platform.unit.clone()))
}

fn default_service_configuration(kind: ManagedServiceKind) -> Result<ServiceConfiguration> {
    let registry = ServiceDriverRegistry::built_in()?;
    Ok(registry.legacy_driver(kind)?.default_configuration())
}

fn state_io(error: std::io::Error) -> LumicError {
    LumicError::Internal {
        message: format!("managed-service state I/O failed: {error}"),
    }
}

fn current_state_time() -> u64 {
    u64::try_from(unix_time_ms()).unwrap_or(u64::MAX)
}

fn persist_application_service_bindings(
    state: &mut crate::framework_state::FrameworkState,
    application: &lumic_core::application::Application,
    service: &ManagedService,
    reference: &ApplicationServiceReference,
    now: u64,
) -> Result<()> {
    let application_ref = ResourceRef::new(ResourceKind::Application, &application.id)?;
    if let Some(existing) = state
        .resources
        .iter_mut()
        .find(|resource| resource.resource == application_ref)
    {
        existing.updated_at_unix_ms = now;
    } else {
        state.resources.push(ResourceRecord {
            resource: application_ref.clone(),
            attributes: BTreeMap::from([
                ("domain".into(), Value::String(application.domain.clone())),
                ("resource_type".into(), Value::String("application".into())),
            ]),
            outputs: ResourceOutputs::new(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        });
    }

    let database_input = format!("{}_database", reference.role);
    let endpoint_input = format!("{}_endpoint", reference.role);
    let credential_input = format!("{}_credential", reference.role);
    state.bindings.0.retain(|binding| {
        binding.consumer != application_ref
            || (binding.input != database_input
                && binding.input != endpoint_input
                && binding.input != credential_input)
    });
    if let Some(secret_name) = search_secret_name(service.kind) {
        replace_binding(
            &mut state.bindings.0,
            Binding {
                id: binding_id("endpoint", &application.id, &reference.role),
                producer: ResourceRef::new(ResourceKind::ManagedService, &reference.service_id)?,
                output: "http".into(),
                consumer: application_ref.clone(),
                input: endpoint_input,
                created_at_unix_ms: now,
            },
        );
        replace_binding(
            &mut state.bindings.0,
            Binding {
                id: binding_id("credential", &application.id, &reference.role),
                producer: ResourceRef::new(ResourceKind::ManagedService, &reference.service_id)?,
                output: secret_name.into(),
                consumer: application_ref,
                input: credential_input,
                created_at_unix_ms: now,
            },
        );
        return state.validate();
    }
    if let Some(database) = &reference.database {
        replace_binding(
            &mut state.bindings.0,
            Binding {
                id: binding_id("database", &application.id, &reference.role),
                producer: ResourceRef::new(
                    ResourceKind::ServiceResource,
                    format!("database.{}-{database}", reference.service_id),
                )?,
                output: "database".into(),
                consumer: application_ref.clone(),
                input: database_input,
                created_at_unix_ms: now,
            },
        );
    }
    if let Some(user) = &reference.user {
        replace_binding(
            &mut state.bindings.0,
            Binding {
                id: binding_id("credential", &application.id, &reference.role),
                producer: ResourceRef::new(
                    ResourceKind::ServiceResource,
                    format!("database-user.{}-{user}", reference.service_id),
                )?,
                output: "credential".into(),
                consumer: application_ref,
                input: credential_input,
                created_at_unix_ms: now,
            },
        );
    }
    state.validate()
}

fn search_secret_name(kind: ManagedServiceKind) -> Option<&'static str> {
    match kind {
        ManagedServiceKind::Typesense => Some("api_key"),
        ManagedServiceKind::Meilisearch => Some("master_key"),
        _ => None,
    }
}

fn replace_binding(bindings: &mut Vec<Binding>, binding: Binding) {
    bindings.retain(|existing| {
        existing.id != binding.id
            && !(existing.consumer == binding.consumer && existing.input == binding.input)
    });
    bindings.push(binding);
}

fn binding_id(kind: &str, application: &str, role: &str) -> String {
    let digest = Sha256::digest(format!("{application}\0{role}\0{kind}").as_bytes());
    format!("application-{kind}-{digest:x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumic_core::application::{Application, ApplicationRuntime, HealthCheck, TlsState};

    fn temp_dir(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("lumic-managed-{name}-{}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn application_fixture(now: u128) -> Application {
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
            web_configured: false,
            tls: TlsState::default(),
            release_retention: 5,
            health_status: "not_deployed".into(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        }
    }

    #[test]
    fn store_round_trips_generic_service_state() {
        let directory = temp_dir("store");
        let store = ManagedServiceStore::at_state_dir(&directory);
        let now = unix_time_ms();
        let service = ManagedService {
            id: "primary-db".into(),
            name: "primary-db".into(),
            kind: ManagedServiceKind::Postgresql,
            package: "postgresql".into(),
            systemd_unit: "postgresql.service".into(),
            desired_state: DesiredServiceState::Running,
            configuration: default_service_configuration(ManagedServiceKind::Postgresql).unwrap(),
            secret_references: Vec::new(),
            dependencies: Vec::new(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };
        store
            .save(&ManagedServiceState {
                services: vec![service.clone()],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(store.load().unwrap().services, vec![service]);
        assert!(directory.join("resources.json").exists());
        assert!(!directory.join("managed-services.json").exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn provider_settings_are_explicitly_allowlisted() {
        let directory = temp_dir("settings");
        let manager = ManagedServiceManager::at_state_dir(&directory);
        let mut config = default_service_configuration(ManagedServiceKind::Redis).unwrap();
        config.settings.insert("requirepass".into(), "leak".into());
        assert!(
            manager
                .validate_settings(ManagedServiceKind::Redis, &config)
                .is_err()
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn database_user_secret_references_are_valid_resource_ids() {
        let reference = database_user_secret_reference("primary-db", "demo_user");
        assert_eq!(reference, "primary-db-demo-user-password");
        validate_resource_id("secret_reference", &reference).unwrap();
    }

    #[test]
    fn multiple_application_databases_use_distinct_secret_reference_bindings() {
        let directory = temp_dir("application-database-bindings");
        let store = ManagedServiceStore::at_state_dir(&directory);
        let now = unix_time_ms();
        let service = ManagedService {
            id: "mysql".into(),
            name: "mysql".into(),
            kind: ManagedServiceKind::Mysql,
            package: "default-mysql-server".into(),
            systemd_unit: "mysql.service".into(),
            desired_state: DesiredServiceState::Running,
            configuration: default_service_configuration(ManagedServiceKind::Mysql).unwrap(),
            secret_references: vec![
                "mysql-primary-password".into(),
                "mysql-audit-password".into(),
            ],
            dependencies: Vec::new(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };
        store
            .save(&ManagedServiceState {
                services: vec![service.clone()],
                databases: vec![
                    Database {
                        id: "mysql-primary".into(),
                        service_id: "mysql".into(),
                        name: "primary".into(),
                        owner: None,
                        created_at_unix_ms: now,
                    },
                    Database {
                        id: "mysql-audit".into(),
                        service_id: "mysql".into(),
                        name: "audit".into(),
                        owner: None,
                        created_at_unix_ms: now,
                    },
                ],
                users: vec![
                    DatabaseUser {
                        id: "mysql-primary_user".into(),
                        service_id: "mysql".into(),
                        name: "primary_user".into(),
                        secret_reference: "mysql-primary-password".into(),
                        databases: vec!["primary".into()],
                        created_at_unix_ms: now,
                        updated_at_unix_ms: now,
                    },
                    DatabaseUser {
                        id: "mysql-audit_user".into(),
                        service_id: "mysql".into(),
                        name: "audit_user".into(),
                        secret_reference: "mysql-audit-password".into(),
                        databases: vec!["audit".into()],
                        created_at_unix_ms: now,
                        updated_at_unix_ms: now,
                    },
                ],
                backups: Vec::new(),
            })
            .unwrap();
        let application = application_fixture(now);
        let framework_store = FrameworkStateStore::at_state_dir(&directory);
        let mut framework = framework_store.load().unwrap();
        for (role, database, user, secret) in [
            (
                "primary",
                "primary",
                "primary_user",
                "mysql-primary-password",
            ),
            ("audit", "audit", "audit_user", "mysql-audit-password"),
        ] {
            persist_application_service_bindings(
                &mut framework,
                &application,
                &service,
                &ApplicationServiceReference {
                    service_id: "mysql".into(),
                    role: role.into(),
                    database: Some(database.into()),
                    user: Some(user.into()),
                    secret_reference: Some(secret.into()),
                },
                current_state_time(),
            )
            .unwrap();
        }
        assert_eq!(framework.bindings.0.len(), 4);
        let credential_outputs = framework
            .resources
            .iter()
            .filter_map(|resource| resource.outputs.get("credential"))
            .collect::<Vec<_>>();
        assert_eq!(credential_outputs.len(), 2);
        assert!(credential_outputs.iter().all(|output| {
            output.sensitive
                && output
                    .value
                    .as_str()
                    .is_some_and(|value| value.starts_with("secret://"))
        }));
        assert!(framework.validate().is_ok());
        assert!(
            framework
                .bindings
                .assert_removable(
                    &ResourceRef::new(ResourceKind::ServiceResource, "database.mysql-primary")
                        .unwrap()
                )
                .is_err()
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn search_attachment_publishes_endpoint_and_credential_bindings() {
        let directory = temp_dir("application-search-bindings");
        let now = unix_time_ms();
        let service = ManagedService {
            id: "search".into(),
            name: "search".into(),
            kind: ManagedServiceKind::Typesense,
            package: "typesense-server".into(),
            systemd_unit: "typesense-server.service".into(),
            desired_state: DesiredServiceState::Running,
            configuration: default_service_configuration(ManagedServiceKind::Typesense).unwrap(),
            secret_references: vec!["search-api-key".into()],
            dependencies: Vec::new(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        };
        ManagedServiceStore::at_state_dir(&directory)
            .save(&ManagedServiceState {
                services: vec![service.clone()],
                ..Default::default()
            })
            .unwrap();
        let mut framework = FrameworkStateStore::at_state_dir(&directory)
            .load()
            .unwrap();
        persist_application_service_bindings(
            &mut framework,
            &application_fixture(now),
            &service,
            &ApplicationServiceReference {
                service_id: "search".into(),
                role: "search".into(),
                database: None,
                user: None,
                secret_reference: Some("search-api-key".into()),
            },
            current_state_time(),
        )
        .unwrap();

        assert_eq!(framework.bindings.0.len(), 2);
        assert!(
            framework
                .bindings
                .0
                .iter()
                .any(|binding| { binding.output == "http" && binding.input == "search_endpoint" })
        );
        assert!(framework.bindings.0.iter().any(|binding| {
            binding.output == "api_key" && binding.input == "search_credential"
        }));
        assert!(
            framework
                .bindings
                .assert_removable(
                    &ResourceRef::new(ResourceKind::ManagedService, "search").unwrap()
                )
                .is_err()
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn backup_verification_detects_valid_and_tampered_native_artifacts() {
        let directory = temp_dir("backup-verification");
        let backup_path = directory.join("redis.rdb");
        fs::write(&backup_path, b"REDIS0011payload").unwrap();
        let checksum = file_sha256(&backup_path).unwrap();
        ManagedServiceStore::at_state_dir(&directory)
            .save(&ManagedServiceState {
                backups: vec![ServiceBackup {
                    id: "backup-reference".into(),
                    service_id: "cache".into(),
                    database: None,
                    path: backup_path.display().to_string(),
                    size_bytes: fs::metadata(&backup_path).unwrap().len(),
                    checksum_sha256: Some(checksum),
                    status: BackupStatus::Completed,
                    created_at_unix_ms: unix_time_ms(),
                    message: "test fixture".into(),
                }],
                ..Default::default()
            })
            .unwrap();

        let manager = ManagedServiceManager::at_state_dir(&directory);
        let valid = manager.verify_backup("backup-reference").unwrap();
        assert!(valid.exists && valid.size_matches && valid.format_valid);
        assert_eq!(valid.checksum_matches, Some(true));

        fs::write(&backup_path, b"REDIS0011tampered").unwrap();
        let tampered = manager.verify_backup("backup-reference").unwrap();
        assert_eq!(tampered.checksum_matches, Some(false));
        assert!(!tampered.size_matches);
        assert!(tampered.message.contains("do not restore"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn dependencies_are_validated_idempotent_and_persisted() {
        let directory = temp_dir("dependencies");
        let store = ManagedServiceStore::at_state_dir(&directory);
        let now = unix_time_ms();
        let service = |id: &str, kind| {
            let (package, systemd_unit) = service_definition(kind).unwrap();
            ManagedService {
                id: id.into(),
                name: id.into(),
                kind,
                package,
                systemd_unit,
                desired_state: DesiredServiceState::Running,
                configuration: default_service_configuration(kind).unwrap(),
                secret_references: Vec::new(),
                dependencies: Vec::new(),
                created_at_unix_ms: now,
                updated_at_unix_ms: now,
            }
        };
        store
            .save(&ManagedServiceState {
                services: vec![
                    service("database", ManagedServiceKind::Postgresql),
                    service("cache", ManagedServiceKind::Redis),
                ],
                ..Default::default()
            })
            .unwrap();
        let manager = ManagedServiceManager::at_state_dir(&directory);
        let context = OperationContext {
            actor: "test".into(),
            interface: lumic_core::OperationInterface::Cli,
            correlation_id: "test-dependency".into(),
            dry_run: false,
            approved: true,
        };
        let first = manager
            .declare_dependency("database", "cache", "cache coordination", true, &context)
            .unwrap();
        assert!(first.changed);
        let second = manager
            .declare_dependency("database", "cache", "cache coordination", true, &context)
            .unwrap();
        assert!(!second.changed);
        assert_eq!(manager.list().unwrap()[0].dependencies.len(), 1);
        assert!(
            manager
                .declare_dependency("database", "database", "invalid", true, &context)
                .is_err()
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
