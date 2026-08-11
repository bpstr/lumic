//! Versioned state for the catalog-backed resource framework.

use crate::atomic_file::write_atomic;
use lumic_core::{
    LumicError, Result,
    binding::BindingGraph,
    catalog::Configuration,
    managed_service::{
        BackupStatus, Database, DatabaseUser, ManagedService, ManagedServiceKind,
        ManagedServiceState, ServiceBackup, ServiceConfiguration, ServiceDependency,
    },
    pipeline::PipelineExecution,
    resource::{ResourceKind, ResourceOutput, ResourceOutputs, ResourceRecord, ResourceRef},
    service::{DesiredServiceState, ManagementStatus, ResourceOwnership, ServiceInstance},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub const RESOURCE_STATE_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateMigration {
    pub from_schema_version: u32,
    pub to_schema_version: u32,
    pub source: String,
    pub backup: String,
    pub migrated_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameworkState {
    pub schema_version: u32,
    #[serde(default)]
    pub services: Vec<ServiceInstance>,
    #[serde(default)]
    pub resources: Vec<ResourceRecord>,
    #[serde(default)]
    pub bindings: BindingGraph,
    #[serde(default)]
    pub pipeline_executions: Vec<PipelineExecution>,
    #[serde(default)]
    pub migrations: Vec<StateMigration>,
}

impl Default for FrameworkState {
    fn default() -> Self {
        Self {
            schema_version: RESOURCE_STATE_SCHEMA_VERSION,
            services: Vec::new(),
            resources: Vec::new(),
            bindings: BindingGraph::default(),
            pipeline_executions: Vec::new(),
            migrations: Vec::new(),
        }
    }
}

impl FrameworkState {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != RESOURCE_STATE_SCHEMA_VERSION {
            return Err(invalid(
                "state.schema_version",
                &format!(
                    "expected {RESOURCE_STATE_SCHEMA_VERSION}, got {}",
                    self.schema_version
                ),
            ));
        }
        let mut references = BTreeSet::new();
        for service in &self.services {
            service.validate()?;
            if !references.insert(service.resource_ref()) {
                return Err(invalid("state.services", "duplicate service resource"));
            }
        }
        for resource in &self.resources {
            resource.validate()?;
            if !references.insert(resource.resource.clone()) {
                return Err(invalid("state.resources", "duplicate resource"));
            }
        }
        self.bindings.validate()?;
        for binding in &self.bindings.0 {
            if !references.contains(&binding.producer) {
                return Err(invalid(
                    "state.bindings.producer",
                    "binding references an unknown producer",
                ));
            }
            if !references.contains(&binding.consumer) {
                return Err(invalid(
                    "state.bindings.consumer",
                    "binding references an unknown consumer",
                ));
            }
            let has_output = self
                .services
                .iter()
                .find(|service| service.resource_ref() == binding.producer)
                .is_some_and(|service| service.outputs.contains_key(&binding.output))
                || self
                    .resources
                    .iter()
                    .find(|resource| resource.resource == binding.producer)
                    .is_some_and(|resource| resource.outputs.contains_key(&binding.output));
            if !has_output {
                return Err(invalid(
                    "state.bindings.output",
                    "binding references an unknown producer output",
                ));
            }
        }
        let mut execution_ids = BTreeSet::new();
        for execution in &self.pipeline_executions {
            if !execution_ids.insert(execution.id.as_str()) {
                return Err(invalid(
                    "state.pipeline_executions",
                    "duplicate execution id",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct FrameworkStateStore {
    state_dir: PathBuf,
    path: PathBuf,
    legacy_path: PathBuf,
}

impl FrameworkStateStore {
    pub fn at_state_dir(state_dir: impl AsRef<Path>) -> Self {
        let state_dir = state_dir.as_ref().to_path_buf();
        Self {
            path: state_dir.join("resources.json"),
            legacy_path: state_dir.join("managed-services.json"),
            state_dir,
        }
    }

    pub fn load(&self) -> Result<FrameworkState> {
        if !self.path.exists() {
            return Ok(FrameworkState::default());
        }
        if self.path.is_symlink() {
            return Err(invalid(
                "state.path",
                "refusing to load state through a symbolic link",
            ));
        }
        let state: FrameworkState =
            serde_json::from_slice(&fs::read(&self.path).map_err(state_io)?).map_err(|error| {
                LumicError::Internal {
                    message: format!("resource framework state is invalid: {error}"),
                }
            })?;
        state.validate()?;
        Ok(state)
    }

    pub fn save(&self, state: &FrameworkState) -> Result<()> {
        state.validate()?;
        if !self.state_dir.is_absolute() {
            return Err(invalid("state_dir", "must be an absolute path"));
        }
        fs::create_dir_all(&self.state_dir).map_err(state_io)?;
        let mut contents =
            serde_json::to_vec_pretty(state).map_err(|error| LumicError::Internal {
                message: format!("could not serialize resource framework state: {error}"),
            })?;
        contents.push(b'\n');
        write_atomic(&self.path, &contents, 0o600)?;
        Ok(())
    }

    /// Loads current state or performs the one-time v1 managed-service migration.
    pub fn load_or_migrate(&self, now: u64) -> Result<FrameworkState> {
        if self.path.exists() || !self.legacy_path.exists() {
            return self.load();
        }
        if self.legacy_path.is_symlink() {
            return Err(invalid(
                "state.legacy_path",
                "refusing to migrate state through a symbolic link",
            ));
        }
        let legacy_bytes = fs::read(&self.legacy_path).map_err(state_io)?;
        let legacy: ManagedServiceState =
            serde_json::from_slice(&legacy_bytes).map_err(|error| LumicError::Internal {
                message: format!("legacy managed-service state is invalid: {error}"),
            })?;
        let backup = self.state_dir.join("managed-services.v1.json");
        if backup.exists() {
            return Err(LumicError::Internal {
                message: format!(
                    "legacy state backup already exists without migrated state: {}",
                    backup.display()
                ),
            });
        }
        fs::copy(&self.legacy_path, &backup).map_err(state_io)?;
        #[cfg(unix)]
        fs::set_permissions(&backup, fs::Permissions::from_mode(0o600)).map_err(state_io)?;

        let state = migrate_legacy(legacy, &self.legacy_path, &backup, now)?;
        if let Err(error) = self.save(&state) {
            let _ = fs::remove_file(&backup);
            return Err(error);
        }
        Ok(state)
    }

    /// Loads the compatibility command model from the authoritative framework state.
    pub fn load_managed_service_state(&self, now: u64) -> Result<ManagedServiceState> {
        framework_to_managed_state(&self.load_or_migrate(now)?)
    }

    /// Replaces compatibility service records without disturbing newer framework state.
    pub fn save_managed_service_state(&self, legacy: &ManagedServiceState, now: u64) -> Result<()> {
        let mut state = self.load_or_migrate(now)?;
        let compatibility_service_ids = state
            .services
            .iter()
            .filter(|service| {
                matches!(
                    service.definition_id.as_str(),
                    "mysql" | "postgresql" | "redis" | "typesense" | "meilisearch"
                )
            })
            .map(|service| service.id.clone())
            .collect::<BTreeSet<_>>();
        state.services.retain(|service| {
            !matches!(
                service.definition_id.as_str(),
                "mysql" | "postgresql" | "redis" | "typesense" | "meilisearch"
            )
        });
        state.resources.retain(|resource| {
            let is_compatibility_resource = matches!(
                resource
                    .attributes
                    .get("resource_type")
                    .and_then(Value::as_str),
                Some("database" | "database_user" | "backup")
            ) && resource
                .attributes
                .get("provider_service_id")
                .and_then(Value::as_str)
                .is_some_and(|service_id| compatibility_service_ids.contains(service_id));
            !is_compatibility_resource
        });
        for service in legacy.services.iter().cloned() {
            state.services.push(migrate_service(service)?);
        }
        for database in &legacy.databases {
            state.resources.push(database_resource(database)?);
        }
        for user in &legacy.users {
            state.resources.push(database_user_resource(user)?);
        }
        for backup in &legacy.backups {
            state.resources.push(backup_resource(backup)?);
        }
        self.save(&state)
    }
}

fn migrate_legacy(
    legacy: ManagedServiceState,
    source: &Path,
    backup: &Path,
    now: u64,
) -> Result<FrameworkState> {
    let mut state = FrameworkState::default();
    for service in legacy.services {
        state.services.push(migrate_service(service)?);
    }
    for database in legacy.databases {
        state.resources.push(database_resource(&database)?);
    }
    for user in legacy.users {
        state.resources.push(database_user_resource(&user)?);
    }
    for backup_record in legacy.backups {
        state.resources.push(backup_resource(&backup_record)?);
    }
    state.migrations.push(StateMigration {
        from_schema_version: 1,
        to_schema_version: RESOURCE_STATE_SCHEMA_VERSION,
        source: source.display().to_string(),
        backup: backup.display().to_string(),
        migrated_at_unix_ms: now,
    });
    state.validate()?;
    Ok(state)
}

fn framework_to_managed_state(state: &FrameworkState) -> Result<ManagedServiceState> {
    let services = state
        .services
        .iter()
        .filter(|service| {
            matches!(
                service.definition_id.as_str(),
                "mysql" | "postgresql" | "redis" | "typesense" | "meilisearch"
            )
        })
        .map(service_from_instance)
        .collect::<Result<Vec<_>>>()?;
    let mut legacy = ManagedServiceState {
        services,
        ..ManagedServiceState::default()
    };
    for resource in &state.resources {
        match resource
            .attributes
            .get("resource_type")
            .and_then(Value::as_str)
        {
            Some("database") => legacy.databases.push(database_from_resource(resource)?),
            Some("database_user") => legacy.users.push(database_user_from_resource(resource)?),
            Some("backup") => legacy.backups.push(backup_from_resource(resource)?),
            _ => {}
        }
    }
    Ok(legacy)
}

fn service_from_instance(instance: &ServiceInstance) -> Result<ManagedService> {
    let kind = match instance.definition_id.as_str() {
        "mysql" => ManagedServiceKind::Mysql,
        "postgresql" => ManagedServiceKind::Postgresql,
        "redis" => ManagedServiceKind::Redis,
        "typesense" => ManagedServiceKind::Typesense,
        "meilisearch" => ManagedServiceKind::Meilisearch,
        "valkey" => ManagedServiceKind::Valkey,
        "rabbitmq" => ManagedServiceKind::Rabbitmq,
        "minio" => ManagedServiceKind::Minio,
        "opensearch" => ManagedServiceKind::Opensearch,
        "memcached" => ManagedServiceKind::Memcached,
        "mongodb" => ManagedServiceKind::Mongodb,
        "clickhouse" => ManagedServiceKind::Clickhouse,
        "prometheus" => ManagedServiceKind::Prometheus,
        "grafana" => ManagedServiceKind::Grafana,
        "loki" => ManagedServiceKind::Loki,
        "gitea" => ManagedServiceKind::Gitea,
        "gogs" => ManagedServiceKind::Gogs,
        _ => {
            return Err(invalid(
                "service.definition_id",
                "unsupported compatibility service",
            ));
        }
    };
    let bind_address = required_string(&instance.configuration, "bind_address")?;
    let port = instance
        .configuration
        .get("port")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .ok_or_else(|| invalid("service.configuration.port", "must be a valid port"))?;
    let settings = deserialize_attribute(&instance.configuration, "settings")?;
    let package = required_string(&instance.platform_metadata, "package")?;
    let systemd_unit = required_string(&instance.platform_metadata, "systemd_unit")?;
    let secret_references =
        deserialize_attribute(&instance.platform_metadata, "secret_references")?;
    let dependencies: Vec<ServiceDependency> =
        deserialize_attribute(&instance.platform_metadata, "legacy_dependencies")?;
    Ok(ManagedService {
        id: instance.id.clone(),
        name: instance.display_name.clone(),
        kind,
        package,
        systemd_unit,
        desired_state: match instance.desired_state {
            DesiredServiceState::Running => {
                lumic_core::managed_service::DesiredServiceState::Running
            }
            DesiredServiceState::Stopped => {
                lumic_core::managed_service::DesiredServiceState::Stopped
            }
        },
        configuration: ServiceConfiguration {
            bind_address,
            port,
            settings,
        },
        secret_references,
        dependencies,
        created_at_unix_ms: u128::from(instance.created_at_unix_ms),
        updated_at_unix_ms: u128::from(instance.updated_at_unix_ms),
    })
}

fn database_resource(database: &Database) -> Result<ResourceRecord> {
    let created = narrow_timestamp(database.created_at_unix_ms)?;
    Ok(ResourceRecord {
        resource: ResourceRef::new(
            ResourceKind::ServiceResource,
            format!("database.{}", database.id),
        )?,
        attributes: Configuration::from([
            (
                "provider_service_id".into(),
                Value::String(database.service_id.clone()),
            ),
            ("name".into(), Value::String(database.name.clone())),
            (
                "owner".into(),
                database
                    .owner
                    .as_ref()
                    .map(|owner| Value::String(owner.clone()))
                    .unwrap_or(Value::Null),
            ),
            ("resource_type".into(), Value::String("database".into())),
        ]),
        outputs: ResourceOutputs::from([(
            "database".into(),
            ResourceOutput {
                value: Value::String(database.name.clone()),
                sensitive: false,
                updated_at_unix_ms: created,
            },
        )]),
        created_at_unix_ms: created,
        updated_at_unix_ms: created,
    })
}

fn database_user_resource(user: &DatabaseUser) -> Result<ResourceRecord> {
    Ok(ResourceRecord {
        resource: ResourceRef::new(
            ResourceKind::ServiceResource,
            format!("database-user.{}", user.id),
        )?,
        attributes: Configuration::from([
            (
                "provider_service_id".into(),
                Value::String(user.service_id.clone()),
            ),
            ("name".into(), Value::String(user.name.clone())),
            (
                "secret_reference".into(),
                Value::String(user.secret_reference.clone()),
            ),
            ("databases".into(), json!(user.databases)),
            (
                "resource_type".into(),
                Value::String("database_user".into()),
            ),
        ]),
        outputs: ResourceOutputs::from([(
            "credential".into(),
            ResourceOutput {
                value: Value::String(format!("secret://{}", user.secret_reference)),
                sensitive: true,
                updated_at_unix_ms: narrow_timestamp(user.updated_at_unix_ms)?,
            },
        )]),
        created_at_unix_ms: narrow_timestamp(user.created_at_unix_ms)?,
        updated_at_unix_ms: narrow_timestamp(user.updated_at_unix_ms)?,
    })
}

fn backup_resource(backup: &ServiceBackup) -> Result<ResourceRecord> {
    let created = narrow_timestamp(backup.created_at_unix_ms)?;
    Ok(ResourceRecord {
        resource: ResourceRef::new(ResourceKind::Artifact, format!("backup.{}", backup.id))?,
        attributes: Configuration::from([
            (
                "provider_service_id".into(),
                Value::String(backup.service_id.clone()),
            ),
            (
                "database".into(),
                backup
                    .database
                    .as_ref()
                    .map(|database| Value::String(database.clone()))
                    .unwrap_or(Value::Null),
            ),
            ("path".into(), Value::String(backup.path.clone())),
            ("size_bytes".into(), Value::from(backup.size_bytes)),
            ("checksum_sha256".into(), json!(backup.checksum_sha256)),
            ("status".into(), json!(backup.status)),
            ("message".into(), Value::String(backup.message.clone())),
            ("resource_type".into(), Value::String("backup".into())),
        ]),
        outputs: ResourceOutputs::new(),
        created_at_unix_ms: created,
        updated_at_unix_ms: created,
    })
}

fn database_from_resource(resource: &ResourceRecord) -> Result<Database> {
    Ok(Database {
        id: prefixed_id(&resource.resource.id, "database.")?,
        service_id: required_string(&resource.attributes, "provider_service_id")?,
        name: required_string(&resource.attributes, "name")?,
        owner: optional_string(&resource.attributes, "owner")?,
        created_at_unix_ms: u128::from(resource.created_at_unix_ms),
    })
}

fn database_user_from_resource(resource: &ResourceRecord) -> Result<DatabaseUser> {
    Ok(DatabaseUser {
        id: prefixed_id(&resource.resource.id, "database-user.")?,
        service_id: required_string(&resource.attributes, "provider_service_id")?,
        name: required_string(&resource.attributes, "name")?,
        secret_reference: required_string(&resource.attributes, "secret_reference")?,
        databases: deserialize_attribute(&resource.attributes, "databases")?,
        created_at_unix_ms: u128::from(resource.created_at_unix_ms),
        updated_at_unix_ms: u128::from(resource.updated_at_unix_ms),
    })
}

fn backup_from_resource(resource: &ResourceRecord) -> Result<ServiceBackup> {
    Ok(ServiceBackup {
        id: prefixed_id(&resource.resource.id, "backup.")?,
        service_id: required_string(&resource.attributes, "provider_service_id")?,
        database: optional_string(&resource.attributes, "database")?,
        path: required_string(&resource.attributes, "path")?,
        size_bytes: resource
            .attributes
            .get("size_bytes")
            .and_then(Value::as_u64)
            .ok_or_else(|| invalid("resource.size_bytes", "must be an unsigned integer"))?,
        checksum_sha256: optional_string(&resource.attributes, "checksum_sha256")?,
        status: deserialize_attribute::<BackupStatus>(&resource.attributes, "status")?,
        created_at_unix_ms: u128::from(resource.created_at_unix_ms),
        message: required_string(&resource.attributes, "message")?,
    })
}

fn required_string(attributes: &Configuration, key: &str) -> Result<String> {
    attributes
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| invalid(&format!("resource.{key}"), "must be a string"))
}

fn optional_string(attributes: &Configuration, key: &str) -> Result<Option<String>> {
    match attributes.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(invalid(
            &format!("resource.{key}"),
            "must be a string or null",
        )),
    }
}

fn deserialize_attribute<T: serde::de::DeserializeOwned>(
    attributes: &Configuration,
    key: &str,
) -> Result<T> {
    let value = attributes
        .get(key)
        .cloned()
        .ok_or_else(|| invalid(&format!("resource.{key}"), "is missing"))?;
    serde_json::from_value(value).map_err(|error| {
        invalid(
            &format!("resource.{key}"),
            &format!("has an invalid value: {error}"),
        )
    })
}

fn prefixed_id(value: &str, prefix: &str) -> Result<String> {
    value
        .strip_prefix(prefix)
        .map(str::to_owned)
        .ok_or_else(|| invalid("resource.id", &format!("must start with '{prefix}'")))
}

fn migrate_service(service: ManagedService) -> Result<ServiceInstance> {
    let kind = service.kind;
    let definition_id = kind.id();
    let desired_state = match service.desired_state {
        lumic_core::managed_service::DesiredServiceState::Running => DesiredServiceState::Running,
        lumic_core::managed_service::DesiredServiceState::Stopped => DesiredServiceState::Stopped,
    };
    let updated = narrow_timestamp(service.updated_at_unix_ms)?;
    let address = service.configuration.bind_address;
    let port = service.configuration.port;
    let endpoint = if address.contains(':') {
        format!("[{address}]:{port}")
    } else {
        format!("{address}:{port}")
    };
    let mut outputs = ResourceOutputs::from([
        (
            "address".into(),
            ResourceOutput {
                value: Value::String(address.clone()),
                sensitive: false,
                updated_at_unix_ms: updated,
            },
        ),
        (
            "port".into(),
            ResourceOutput {
                value: Value::from(port),
                sensitive: false,
                updated_at_unix_ms: updated,
            },
        ),
        (
            "endpoint".into(),
            ResourceOutput {
                value: Value::String(endpoint.clone()),
                sensitive: false,
                updated_at_unix_ms: updated,
            },
        ),
    ]);
    if matches!(
        kind,
        ManagedServiceKind::Typesense | ManagedServiceKind::Meilisearch
    ) {
        outputs.insert(
            "http".into(),
            ResourceOutput {
                value: Value::String(format!("http://{endpoint}")),
                sensitive: false,
                updated_at_unix_ms: updated,
            },
        );
        let secret_name = match kind {
            ManagedServiceKind::Typesense => "api_key",
            ManagedServiceKind::Meilisearch => "master_key",
            _ => unreachable!(),
        };
        let expected_reference = format!("{}-{}", service.id, secret_name.replace('_', "-"));
        if !service.secret_references.contains(&expected_reference) {
            return Err(invalid(
                "service.secret_references",
                &format!("search service is missing required secret '{secret_name}'"),
            ));
        }
        outputs.insert(
            secret_name.into(),
            ResourceOutput {
                value: Value::String(format!("secret://{expected_reference}")),
                sensitive: true,
                updated_at_unix_ms: updated,
            },
        );
    }
    let configuration = Configuration::from([
        ("bind_address".into(), Value::String(address)),
        ("port".into(), Value::from(port)),
        ("settings".into(), json!(service.configuration.settings)),
    ]);
    let platform_metadata = Configuration::from([
        ("package".into(), Value::String(service.package)),
        ("systemd_unit".into(), Value::String(service.systemd_unit)),
        ("secret_references".into(), json!(service.secret_references)),
        ("legacy_dependencies".into(), json!(service.dependencies)),
    ]);
    Ok(ServiceInstance {
        id: service.id,
        definition_id: definition_id.into(),
        definition_version: 1,
        display_name: service.name,
        ownership: ResourceOwnership::Lumic,
        management_status: ManagementStatus::Managed,
        desired_state,
        configuration,
        outputs,
        platform_metadata,
        installed_version: None,
        created_at_unix_ms: narrow_timestamp(service.created_at_unix_ms)?,
        updated_at_unix_ms: updated,
    })
}

fn narrow_timestamp(timestamp: u128) -> Result<u64> {
    u64::try_from(timestamp).map_err(|_| invalid("timestamp", "exceeds supported range"))
}

fn invalid(field: &str, message: &str) -> LumicError {
    LumicError::InvalidInput {
        field: field.into(),
        message: message.into(),
    }
}

fn state_io(error: impl std::fmt::Display) -> LumicError {
    LumicError::Internal {
        message: format!("resource framework state operation failed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumic_core::managed_service::{
        DesiredServiceState as LegacyDesiredState, ServiceConfiguration,
    };
    use std::collections::BTreeMap;

    fn temp_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "lumic-framework-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    #[test]
    fn state_round_trips_with_private_permissions() {
        let directory = temp_dir("round-trip");
        let store = FrameworkStateStore::at_state_dir(&directory);
        store.save(&FrameworkState::default()).unwrap();
        assert_eq!(store.load().unwrap(), FrameworkState::default());
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(directory.join("resources.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn migrates_legacy_state_after_preserving_a_backup() {
        let directory = temp_dir("migration");
        fs::create_dir_all(&directory).unwrap();
        let legacy = ManagedServiceState {
            services: vec![ManagedService {
                id: "primary-db".into(),
                name: "Primary database".into(),
                kind: ManagedServiceKind::Postgresql,
                package: "postgresql".into(),
                systemd_unit: "postgresql.service".into(),
                desired_state: LegacyDesiredState::Running,
                configuration: ServiceConfiguration {
                    bind_address: "127.0.0.1".into(),
                    port: 5432,
                    settings: Default::default(),
                },
                secret_references: Vec::new(),
                dependencies: Vec::new(),
                created_at_unix_ms: 1,
                updated_at_unix_ms: 2,
            }],
            ..Default::default()
        };
        let legacy_bytes = serde_json::to_vec_pretty(&legacy).unwrap();
        fs::write(directory.join("managed-services.json"), &legacy_bytes).unwrap();
        let store = FrameworkStateStore::at_state_dir(&directory);
        let migrated = store.load_or_migrate(3).unwrap();
        assert_eq!(migrated.services[0].definition_id, "postgresql");
        assert_eq!(
            fs::read(directory.join("managed-services.v1.json")).unwrap(),
            legacy_bytes
        );
        assert_eq!(store.load_or_migrate(4).unwrap(), migrated);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn malformed_legacy_state_is_left_untouched() {
        let directory = temp_dir("bad-migration");
        fs::create_dir_all(&directory).unwrap();
        let legacy_path = directory.join("managed-services.json");
        fs::write(&legacy_path, b"not json").unwrap();
        let store = FrameworkStateStore::at_state_dir(&directory);
        assert!(store.load_or_migrate(1).is_err());
        assert_eq!(fs::read(&legacy_path).unwrap(), b"not json");
        assert!(!directory.join("resources.json").exists());
        assert!(!directory.join("managed-services.v1.json").exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn search_service_publishes_http_and_sensitive_credential_outputs() {
        let instance = migrate_service(ManagedService {
            id: "search".into(),
            name: "Search".into(),
            kind: ManagedServiceKind::Typesense,
            package: "typesense-server".into(),
            systemd_unit: "typesense-server.service".into(),
            desired_state: LegacyDesiredState::Running,
            configuration: ServiceConfiguration {
                bind_address: "127.0.0.1".into(),
                port: 8108,
                settings: BTreeMap::from([
                    ("cors".into(), "false".into()),
                    ("data_directory".into(), "/var/lib/typesense".into()),
                ]),
            },
            secret_references: vec!["search-api-key".into()],
            dependencies: Vec::new(),
            created_at_unix_ms: 1,
            updated_at_unix_ms: 2,
        })
        .unwrap();

        assert_eq!(
            instance.outputs.get("http").unwrap().value,
            Value::String("http://127.0.0.1:8108".into())
        );
        let credential = instance.outputs.get("api_key").unwrap();
        assert!(credential.sensitive);
        assert_eq!(
            credential.value,
            Value::String("secret://search-api-key".into())
        );
    }

    #[test]
    fn compatibility_save_preserves_resources_from_other_drivers() {
        let directory = temp_dir("preserve-newer-resources");
        let store = FrameworkStateStore::at_state_dir(&directory);
        let mysql_database = ResourceRecord {
            resource: ResourceRef::new(ResourceKind::ServiceResource, "database.app").unwrap(),
            attributes: Configuration::from([
                (
                    "provider_service_id".into(),
                    Value::String("mysql-main".into()),
                ),
                ("resource_type".into(), Value::String("database".into())),
            ]),
            outputs: ResourceOutputs::new(),
            created_at_unix_ms: 1,
            updated_at_unix_ms: 1,
        };
        store
            .save(&FrameworkState {
                resources: vec![mysql_database.clone()],
                ..FrameworkState::default()
            })
            .unwrap();

        store
            .save_managed_service_state(&ManagedServiceState::default(), 2)
            .unwrap();

        assert_eq!(store.load().unwrap().resources, vec![mysql_database]);
        fs::remove_dir_all(directory).unwrap();
    }
}
