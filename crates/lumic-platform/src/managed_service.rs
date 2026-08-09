use crate::{
    ProcessOutput, ProcessRunner, ProcessSpec,
    apt::AptPackageManager,
    atomic_file::{restore_backup, write_atomic},
    audit_store::AuditStore,
    event_store::EventStore,
    secret_store::SecretStore,
    systemd::{ServiceAction, SystemdServiceManager},
};
use lumic_core::{
    LumicError, OperationContext, Plan, Result,
    application::{ApplicationServiceReference, unix_time_ms},
    events::{AuditRecord, Event},
    managed_service::{
        BackupStatus, BackupVerification, Database, DatabaseUser, DesiredServiceState,
        ManagedService, ManagedServiceKind, ManagedServiceMutation, ManagedServiceState,
        ManagedServiceStatus, ServiceBackup, ServiceConfiguration, ServiceHealth, ServicePaths,
        install_plan, validate_database_identifier, validate_resource_id,
    },
    package::PackageName,
};
use serde_json::json;
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Clone)]
struct ConfigurationChange {
    path: PathBuf,
    backup: Option<PathBuf>,
    existed_before: bool,
    changed: bool,
}

#[derive(Debug, Clone, Copy)]
struct ServiceDefinition {
    package: &'static str,
    unit: &'static str,
    data_path: &'static str,
}

impl ServiceDefinition {
    const fn for_kind(kind: ManagedServiceKind) -> Self {
        match kind {
            ManagedServiceKind::Postgresql => Self {
                package: "postgresql",
                unit: "postgresql.service",
                data_path: "/var/lib/postgresql",
            },
            ManagedServiceKind::Redis => Self {
                package: "redis-server",
                unit: "redis-server.service",
                data_path: "/var/lib/redis",
            },
        }
    }
}

#[derive(Debug, Clone)]
struct ManagedServiceStore {
    path: PathBuf,
}

impl ManagedServiceStore {
    fn at_state_dir(state_dir: impl AsRef<Path>) -> Self {
        Self {
            path: state_dir.as_ref().join("managed-services.json"),
        }
    }

    fn load(&self) -> Result<ManagedServiceState> {
        if !self.path.exists() {
            return Ok(ManagedServiceState::default());
        }
        serde_json::from_slice(&fs::read(&self.path).map_err(state_io)?).map_err(|error| {
            LumicError::Internal {
                message: format!("managed-service state is invalid: {error}"),
            }
        })
    }

    fn save(&self, state: &ManagedServiceState) -> Result<()> {
        let parent = self.path.parent().ok_or_else(|| LumicError::Internal {
            message: "managed-service state path has no parent".into(),
        })?;
        fs::create_dir_all(parent).map_err(state_io)?;
        let temporary = parent.join(format!(".managed-services-{}.tmp", std::process::id()));
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&temporary).map_err(state_io)?;
        serde_json::to_writer_pretty(&mut file, state).map_err(|error| LumicError::Internal {
            message: format!("could not serialize managed-service state: {error}"),
        })?;
        file.write_all(b"\n").map_err(state_io)?;
        file.sync_all().map_err(state_io)?;
        fs::rename(temporary, &self.path).map_err(state_io)
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
        let definition = ServiceDefinition::for_kind(kind);
        let now = unix_time_ms();
        self.inspect_service(ManagedService {
            id: kind.id().into(),
            name: kind.id().into(),
            kind,
            package: definition.package.into(),
            systemd_unit: definition.unit.into(),
            desired_state: DesiredServiceState::Running,
            configuration: ServiceConfiguration::defaults(kind),
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
            paths: self.paths(&service),
        })
    }

    pub async fn install(
        &self,
        id: &str,
        kind: ManagedServiceKind,
        context: &OperationContext,
    ) -> Result<ManagedServiceMutation> {
        validate_resource_id("service", id)?;
        let definition = ServiceDefinition::for_kind(kind);
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
        let service = existing.clone().unwrap_or_else(|| ManagedService {
            id: id.into(),
            name: id.into(),
            kind,
            package: definition.package.into(),
            systemd_unit: definition.unit.into(),
            desired_state: DesiredServiceState::Running,
            configuration: ServiceConfiguration::defaults(kind),
            secret_references: Vec::new(),
            dependencies: Vec::new(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
        });
        service.configuration.validate()?;
        let package = PackageName::parse(definition.package)?;
        let package_mutation = self.packages.install(&package, context).await?;
        if context.dry_run {
            return Ok(ManagedServiceMutation {
                service,
                action: "install".into(),
                changed: false,
                message: "dry run: package, configuration, enable, start, and health validation"
                    .into(),
            });
        }
        let configured = self.write_configuration(&service).await?;
        if let Err(error) = self
            .systemd
            .apply(definition.unit, ServiceAction::Enable, context)
            .await
        {
            self.restore_configuration(&configured)?;
            return Err(error);
        }
        if let Err(error) = self
            .systemd
            .apply(definition.unit, ServiceAction::Restart, context)
            .await
        {
            self.restore_configuration(&configured)?;
            return Err(error);
        }
        let (health, message) = self.health(&service).await;
        if health != ServiceHealth::Healthy {
            self.restore_configuration(&configured)?;
            return Err(LumicError::Process {
                executable: definition.unit.into(),
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
            message,
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
            message
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
        if let Err(error) = self
            .systemd
            .apply(&service.systemd_unit, ServiceAction::Restart, context)
            .await
        {
            self.restore_configuration(&backup)?;
            let _ = self
                .systemd
                .apply(&service.systemd_unit, ServiceAction::Restart, context)
                .await;
            return Err(error);
        }
        let (health, message) = self.health(&service).await;
        if health != ServiceHealth::Healthy {
            self.restore_configuration(&backup)?;
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
            message,
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
        let service = self.postgresql(service_id)?;
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
        let owner_clause = owner
            .map(|value| format!(" OWNER \"{value}\""))
            .unwrap_or_default();
        self.psql(&format!(
            "SELECT 'CREATE DATABASE \"{name}\"{owner_clause}' WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = '{name}') \\gexec\n"
        ))
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
        let service = self.postgresql(service_id)?;
        let mut state = self.store.load()?;
        if let Some(existing) = state
            .users
            .iter()
            .find(|item| item.service_id == service_id && item.name == name)
        {
            return Ok(existing.clone());
        }
        let secret_reference = format!("{service_id}-{name}-password");
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
        let escaped = password.replace('\'', "''");
        if let Err(error) = self
            .psql(&format!(
                "DO $lumic$ BEGIN IF EXISTS (SELECT FROM pg_roles WHERE rolname = '{name}') THEN ALTER ROLE \"{name}\" PASSWORD '{escaped}'; ELSE CREATE ROLE \"{name}\" LOGIN PASSWORD '{escaped}'; END IF; END $lumic$;\n"
            ))
            .await
        {
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
        let service = self.postgresql(service_id)?;
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
        self.psql(&format!(
            "GRANT ALL PRIVILEGES ON DATABASE \"{database}\" TO \"{user}\";\n"
        ))
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
        let path = match service.kind {
            ManagedServiceKind::Postgresql => {
                database
                    .ok_or_else(|| invalid("database", "PostgreSQL backup requires a database"))?;
                directory.join(format!("{backup_id}.dump"))
            }
            ManagedServiceKind::Redis => directory.join(format!("{backup_id}.rdb")),
        };
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
        match service.kind {
            ManagedServiceKind::Postgresql => {
                let database = database.expect("validated above");
                self.run(ProcessSpec::new("chown").args([
                    "postgres:postgres",
                    "--",
                    directory.to_string_lossy().as_ref(),
                ]))
                .await?;
                self.run(ProcessSpec::new("runuser").args([
                    "-u",
                    "postgres",
                    "--",
                    "pg_dump",
                    "--format=custom",
                    "--file",
                    path.to_string_lossy().as_ref(),
                    "--",
                    database,
                ]))
                .await?;
            }
            ManagedServiceKind::Redis => {
                self.redis_cli(&service, &["SAVE"]).await?;
                fs::copy("/var/lib/redis/dump.rdb", &path).map_err(state_io)?;
            }
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
        let mut header = [0_u8; 5];
        let read = fs::File::open(path)
            .and_then(|mut file| file.read(&mut header))
            .map_err(state_io)?;
        let extension = path.extension().and_then(|value| value.to_str());
        let format_valid = match extension {
            Some("dump") => read == 5 && &header == b"PGDMP",
            Some("rdb") => read == 5 && &header == b"REDIS",
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
        match service.kind {
            ManagedServiceKind::Postgresql => {
                let database = source
                    .database
                    .as_deref()
                    .ok_or_else(|| invalid("backup", "PostgreSQL backup has no database"))?;
                self.run(ProcessSpec::new("runuser").args([
                    "-u",
                    "postgres",
                    "--",
                    "pg_restore",
                    "--clean",
                    "--if-exists",
                    "--exit-on-error",
                    "--dbname",
                    database,
                    "--",
                    &source.path,
                ]))
                .await?;
                let (health, message) = self.health(&service).await;
                if health != ServiceHealth::Healthy {
                    return Err(LumicError::Process {
                        executable: service.systemd_unit.clone(),
                        message: format!(
                            "PostgreSQL restore completed but health failed: {message}"
                        ),
                    });
                }
            }
            ManagedServiceKind::Redis => {
                self.systemd
                    .apply(&service.systemd_unit, ServiceAction::Stop, context)
                    .await?;
                let target = Path::new("/var/lib/redis/dump.rdb");
                let safety = Path::new("/var/lib/redis/dump.rdb.lumic-before-restore");
                let had_target = target.is_file();
                if had_target && let Err(error) = fs::copy(target, safety).map_err(state_io) {
                    let _ = self
                        .systemd
                        .apply(&service.systemd_unit, ServiceAction::Start, context)
                        .await;
                    return Err(error);
                }
                let replacement = async {
                    fs::copy(&source.path, target).map_err(state_io)?;
                    self.run(ProcessSpec::new("chown").args([
                        "redis:redis",
                        "--",
                        target.to_string_lossy().as_ref(),
                    ]))
                    .await?;
                    self.systemd
                        .apply(&service.systemd_unit, ServiceAction::Start, context)
                        .await?;
                    let (health, message) = self.health(&service).await;
                    if health != ServiceHealth::Healthy {
                        return Err(LumicError::Process {
                            executable: service.systemd_unit.clone(),
                            message: format!("Redis restore failed health validation: {message}"),
                        });
                    }
                    Ok(())
                }
                .await;
                if let Err(error) = replacement {
                    self.recover_redis_restore(target, safety, had_target, &service, context)
                        .await
                        .map_err(|recovery| LumicError::Internal {
                            message: format!(
                                "Redis restore failed ({error}); recovery also failed ({recovery})"
                            ),
                        })?;
                    return Err(error);
                }
            }
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
        if !state
            .services
            .iter()
            .any(|item| item.id == reference.service_id)
        {
            return Err(not_found(&reference.service_id));
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
        reference.secret_reference = if let Some(name) = &reference.user {
            Some(
                state
                    .users
                    .iter()
                    .find(|item| item.service_id == reference.service_id && item.name == *name)
                    .ok_or_else(|| invalid("user", "user is not managed by this service"))?
                    .secret_reference
                    .clone(),
            )
        } else {
            None
        };
        application_service.attach_service(application, reference, context)
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

    fn postgresql(&self, id: &str) -> Result<ManagedService> {
        let service = self.find_service(id)?;
        if service.kind != ManagedServiceKind::Postgresql {
            return Err(invalid(
                "service",
                "database and user primitives require PostgreSQL",
            ));
        }
        Ok(service)
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

    fn paths(&self, service: &ManagedService) -> ServicePaths {
        let definition = ServiceDefinition::for_kind(service.kind);
        ServicePaths {
            systemd_unit: definition.unit.into(),
            configuration_paths: match service.kind {
                ManagedServiceKind::Postgresql => vec![
                    self.postgresql_config_path()
                        .unwrap_or_else(|| {
                            PathBuf::from("/etc/postgresql/*/*/conf.d/99-lumic.conf")
                        })
                        .to_string_lossy()
                        .into_owned(),
                ],
                ManagedServiceKind::Redis => vec![
                    "/etc/redis/redis.conf".into(),
                    "/etc/redis/lumic.conf".into(),
                ],
            },
            data_path: definition.data_path.into(),
            log_source: format!("journalctl --unit {}", definition.unit),
        }
    }

    async fn write_configuration(
        &self,
        service: &ManagedService,
    ) -> Result<Vec<ConfigurationChange>> {
        self.validate_settings(service.kind, &service.configuration)?;
        match service.kind {
            ManagedServiceKind::Postgresql => {
                let path = self.postgresql_config_path().ok_or_else(|| {
                    invalid(
                        "configuration",
                        "could not discover the Debian PostgreSQL cluster conf.d directory",
                    )
                })?;
                let mut content = format!(
                    "# Managed by Lumic\nlisten_addresses = '{}'\nport = {}\n",
                    service.configuration.bind_address, service.configuration.port
                );
                for (key, value) in &service.configuration.settings {
                    content.push_str(&format!("{key} = '{value}'\n"));
                }
                let existed_before = path.is_file();
                let result = write_atomic(&path, content.as_bytes(), 0o640)?;
                let changes = vec![ConfigurationChange {
                    path,
                    backup: result.backup,
                    existed_before,
                    changed: result.changed,
                }];
                if let Err(error) = self
                    .set_configuration_owner(&changes[0].path, "root:postgres")
                    .await
                {
                    self.restore_configuration(&changes)?;
                    return Err(error);
                }
                Ok(changes)
            }
            ManagedServiceKind::Redis => {
                let main = Path::new("/etc/redis/redis.conf");
                let include = "include /etc/redis/lumic.conf";
                let existing = fs::read_to_string(main).map_err(state_io)?;
                let mut changes = Vec::new();
                if !existing.lines().any(|line| line.trim() == include) {
                    let content = format!("{}\n{include}\n", existing.trim_end());
                    let result = write_atomic(main, content.as_bytes(), 0o640)?;
                    changes.push(ConfigurationChange {
                        path: main.to_path_buf(),
                        backup: result.backup,
                        existed_before: true,
                        changed: result.changed,
                    });
                }
                let mut content = format!(
                    "# Managed by Lumic\nbind {}\nport {}\n",
                    service.configuration.bind_address, service.configuration.port
                );
                for (key, value) in &service.configuration.settings {
                    content.push_str(&format!("{key} {value}\n"));
                }
                let path = Path::new("/etc/redis/lumic.conf");
                let existed_before = path.is_file();
                match write_atomic(path, content.as_bytes(), 0o640) {
                    Ok(result) => {
                        changes.push(ConfigurationChange {
                            path: path.to_path_buf(),
                            backup: result.backup,
                            existed_before,
                            changed: result.changed,
                        });
                        if let Err(error) = self.set_configuration_owner(path, "root:redis").await {
                            self.restore_configuration(&changes)?;
                            return Err(error);
                        }
                        Ok(changes)
                    }
                    Err(error) => {
                        self.restore_configuration(&changes)?;
                        Err(error)
                    }
                }
            }
        }
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

    async fn recover_redis_restore(
        &self,
        target: &Path,
        safety: &Path,
        had_target: bool,
        service: &ManagedService,
        context: &OperationContext,
    ) -> Result<()> {
        let _ = self
            .systemd
            .apply(&service.systemd_unit, ServiceAction::Stop, context)
            .await;
        if had_target {
            if !safety.is_file() {
                return Err(invalid("backup", "Redis recovery snapshot is missing"));
            }
            fs::copy(safety, target).map_err(state_io)?;
            self.run(ProcessSpec::new("chown").args([
                "redis:redis",
                "--",
                target.to_string_lossy().as_ref(),
            ]))
            .await?;
        } else if target.exists() {
            fs::remove_file(target).map_err(state_io)?;
        }
        self.systemd
            .apply(&service.systemd_unit, ServiceAction::Start, context)
            .await?;
        let (health, message) = self.health(service).await;
        if health != ServiceHealth::Healthy {
            return Err(LumicError::Process {
                executable: service.systemd_unit.clone(),
                message: format!("Redis recovery failed health validation: {message}"),
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
        let allowed: &[&str] = match kind {
            ManagedServiceKind::Postgresql => &["max_connections", "shared_buffers", "work_mem"],
            ManagedServiceKind::Redis => &["maxmemory", "maxmemory_policy", "timeout"],
        };
        if let Some(key) = configuration
            .settings
            .keys()
            .find(|key| !allowed.contains(&key.as_str()))
        {
            return Err(invalid(
                "settings",
                &format!("unsupported {kind} setting: {key}"),
            ));
        }
        Ok(())
    }

    async fn health(&self, service: &ManagedService) -> (ServiceHealth, String) {
        let result = match service.kind {
            ManagedServiceKind::Postgresql => {
                self.run(ProcessSpec::new("pg_isready").args([
                    "--host",
                    &service.configuration.bind_address,
                    "--port",
                    &service.configuration.port.to_string(),
                ]))
                .await
            }
            ManagedServiceKind::Redis => self.redis_cli(service, &["PING"]).await,
        };
        match result {
            Ok(output) => (
                ServiceHealth::Healthy,
                String::from_utf8_lossy(&output.stdout).trim().into(),
            ),
            Err(error) => (ServiceHealth::Unhealthy, error.to_string()),
        }
    }

    async fn redis_cli(
        &self,
        service: &ManagedService,
        arguments: &[&str],
    ) -> Result<ProcessOutput> {
        let port = service.configuration.port.to_string();
        let mut spec = ProcessSpec::new("redis-cli").args([
            "--host",
            &service.configuration.bind_address,
            "--port",
            &port,
        ]);
        spec.args
            .extend(arguments.iter().map(|value| (*value).to_owned()));
        self.run(spec).await
    }

    async fn psql(&self, sql: &str) -> Result<()> {
        let mut spec = ProcessSpec::new("runuser").args([
            "-u",
            "postgres",
            "--",
            "psql",
            "--no-psqlrc",
            "--set",
            "ON_ERROR_STOP=1",
            "--quiet",
        ]);
        spec.timeout = Duration::from_secs(60);
        spec.stdin = Some(sql.as_bytes().to_vec());
        self.run(spec).await.map(|_| ())
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

fn not_found(id: &str) -> LumicError {
    invalid("service", &format!("managed service '{id}' was not found"))
}

fn state_io(error: std::io::Error) -> LumicError {
    LumicError::Internal {
        message: format!("managed-service state I/O failed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("lumic-managed-{name}-{}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
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
            configuration: ServiceConfiguration::defaults(ManagedServiceKind::Postgresql),
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
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn provider_settings_are_explicitly_allowlisted() {
        let directory = temp_dir("settings");
        let manager = ManagedServiceManager::at_state_dir(&directory);
        let mut config = ServiceConfiguration::defaults(ManagedServiceKind::Redis);
        config.settings.insert("requirepass".into(), "leak".into());
        assert!(
            manager
                .validate_settings(ManagedServiceKind::Redis, &config)
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
        let service = |id: &str, kind| ManagedService {
            id: id.into(),
            name: id.into(),
            kind,
            package: ServiceDefinition::for_kind(kind).package.into(),
            systemd_unit: ServiceDefinition::for_kind(kind).unit.into(),
            desired_state: DesiredServiceState::Running,
            configuration: ServiceConfiguration::defaults(kind),
            secret_references: Vec::new(),
            dependencies: Vec::new(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
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
