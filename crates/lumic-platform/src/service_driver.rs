//! Trusted provider drivers selected by catalog driver ID.

use lumic_core::{
    LumicError, Result,
    catalog::{Catalog, ServiceDefinition},
    managed_service::{ManagedService, ManagedServiceKind, ServiceConfiguration, ServicePaths},
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use crate::ProcessSpec;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverConfigurationFile {
    pub path: PathBuf,
    pub content: String,
    pub mode: u32,
    pub owner: &'static str,
}

#[derive(Debug, Clone)]
pub struct DriverBackupPlan {
    pub path: PathBuf,
    pub commands: Vec<ProcessSpec>,
    pub copy_source: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct DriverRestoreReplacement {
    pub target: PathBuf,
    pub safety_copy: PathBuf,
    pub owner: &'static str,
}

#[derive(Debug, Clone)]
pub struct DriverRestorePlan {
    pub stop_service: bool,
    pub commands: Vec<ProcessSpec>,
    pub replacement: Option<DriverRestoreReplacement>,
}

/// Provider-specific behavior which cannot safely live in catalog data.
pub trait ServiceDriver: Send + Sync {
    fn id(&self) -> &'static str;
    fn secret_names(&self) -> &'static [&'static str] {
        &[]
    }
    fn default_configuration(&self) -> ServiceConfiguration;
    fn validate_configuration(&self, configuration: &ServiceConfiguration) -> Result<()>;
    fn paths(&self, service: &ManagedService, discovered_config: Option<PathBuf>) -> ServicePaths;
    fn health_probe(&self, service: &ManagedService) -> ProcessSpec;
    fn configuration_files(
        &self,
        service: &ManagedService,
        discovered_config: Option<PathBuf>,
        secrets: &BTreeMap<String, String>,
    ) -> Result<Vec<DriverConfigurationFile>>;
    fn backup_plan(
        &self,
        service: &ManagedService,
        database: Option<&str>,
        directory: &Path,
        backup_id: &str,
    ) -> Result<DriverBackupPlan>;
    fn restore_plan(&self, backup_path: &Path, database: Option<&str>)
    -> Result<DriverRestorePlan>;
    fn create_database_command(&self, name: &str, owner: Option<&str>) -> Result<ProcessSpec>;
    fn create_user_command(&self, name: &str, password: &str) -> Result<ProcessSpec>;
    fn grant_database_command(&self, database: &str, user: &str) -> Result<ProcessSpec>;
}

/// Compile-time registry: catalog documents can select only reviewed Rust drivers.
pub struct ServiceDriverRegistry {
    catalog: Catalog,
    drivers: BTreeMap<&'static str, Box<dyn ServiceDriver>>,
}

impl std::fmt::Debug for ServiceDriverRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ServiceDriverRegistry")
            .field("drivers", &self.drivers.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl ServiceDriverRegistry {
    pub fn built_in() -> Result<Self> {
        let mut registry = Self {
            catalog: Catalog::built_in()?,
            drivers: BTreeMap::new(),
        };
        registry.register(Box::new(MysqlDriver))?;
        registry.register(Box::new(PostgresqlDriver))?;
        registry.register(Box::new(RedisDriver))?;
        registry.register(Box::new(TypesenseDriver))?;
        registry.register(Box::new(MeilisearchDriver))?;
        registry.validate_catalog_references()?;
        Ok(registry)
    }

    pub fn definition(&self, id: &str) -> Result<&ServiceDefinition> {
        self.catalog
            .service(id)
            .ok_or_else(|| invalid("service", "unknown service definition"))
    }

    pub fn driver(&self, id: &str) -> Result<&dyn ServiceDriver> {
        self.drivers.get(id).map(Box::as_ref).ok_or_else(|| {
            invalid(
                "service.driver",
                &format!("unavailable built-in driver '{id}'"),
            )
        })
    }

    /// Compatibility lookup for the old two-provider command surface.
    pub fn legacy_driver(&self, kind: ManagedServiceKind) -> Result<&dyn ServiceDriver> {
        self.driver(kind.id())
    }

    fn register(&mut self, driver: Box<dyn ServiceDriver>) -> Result<()> {
        let id = driver.id();
        if self.drivers.insert(id, driver).is_some() {
            return Err(invalid(
                "service.driver",
                &format!("duplicate driver '{id}'"),
            ));
        }
        Ok(())
    }

    fn validate_catalog_references(&self) -> Result<()> {
        for id in self.drivers.keys() {
            let definition = self.catalog.service(id).ok_or_else(|| {
                invalid(
                    "service.driver",
                    &format!("driver '{id}' has no catalog definition"),
                )
            })?;
            if definition.driver != *id {
                return Err(invalid(
                    "service.driver",
                    &format!(
                        "definition '{}' references driver '{}' instead of '{id}'",
                        definition.id, definition.driver,
                    ),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct MysqlDriver;

impl ServiceDriver for MysqlDriver {
    fn id(&self) -> &'static str {
        "mysql"
    }

    fn default_configuration(&self) -> ServiceConfiguration {
        ServiceConfiguration {
            bind_address: "127.0.0.1".into(),
            port: 3306,
            settings: BTreeMap::new(),
        }
    }

    fn validate_configuration(&self, configuration: &ServiceConfiguration) -> Result<()> {
        validate_settings(
            self.id(),
            configuration,
            &["innodb_buffer_pool_size", "max_connections", "sql_mode"],
        )
    }

    fn paths(&self, service: &ManagedService, _discovered_config: Option<PathBuf>) -> ServicePaths {
        ServicePaths {
            systemd_unit: service.systemd_unit.clone(),
            configuration_paths: vec!["/etc/mysql/mysql.conf.d/99-lumic.cnf".into()],
            data_path: "/var/lib/mysql".into(),
            log_source: format!("journalctl --unit {}", service.systemd_unit),
        }
    }

    fn health_probe(&self, _service: &ManagedService) -> ProcessSpec {
        ProcessSpec::new("mysqladmin").args(["--protocol=socket", "ping", "--silent"])
    }

    fn configuration_files(
        &self,
        service: &ManagedService,
        _discovered_config: Option<PathBuf>,
        _secrets: &BTreeMap<String, String>,
    ) -> Result<Vec<DriverConfigurationFile>> {
        let mut content = format!(
            "# Managed by Lumic\n[mysqld]\nbind-address = {}\nport = {}\n",
            service.configuration.bind_address, service.configuration.port
        );
        for (key, value) in &service.configuration.settings {
            content.push_str(&format!("{key} = {value}\n"));
        }
        Ok(vec![DriverConfigurationFile {
            path: PathBuf::from("/etc/mysql/mysql.conf.d/99-lumic.cnf"),
            content,
            mode: 0o640,
            owner: "root:root",
        }])
    }

    fn backup_plan(
        &self,
        _service: &ManagedService,
        database: Option<&str>,
        directory: &Path,
        backup_id: &str,
    ) -> Result<DriverBackupPlan> {
        let database =
            database.ok_or_else(|| invalid("database", "MySQL backup requires a database"))?;
        let path = directory.join(format!("{backup_id}.sql"));
        Ok(DriverBackupPlan {
            commands: vec![ProcessSpec::new("mysqldump").args([
                "--protocol=socket",
                "--single-transaction",
                "--routines",
                "--events",
                "--databases",
                database,
                "--result-file",
                path.to_string_lossy().as_ref(),
            ])],
            path,
            copy_source: None,
        })
    }

    fn restore_plan(
        &self,
        backup_path: &Path,
        database: Option<&str>,
    ) -> Result<DriverRestorePlan> {
        let database = database.ok_or_else(|| invalid("backup", "MySQL backup has no database"))?;
        Ok(DriverRestorePlan {
            stop_service: false,
            commands: vec![ProcessSpec::new("mysql").args([
                "--protocol=socket",
                "--database",
                database,
                "--execute",
                &format!("source {}", backup_path.to_string_lossy()),
            ])],
            replacement: None,
        })
    }

    fn create_database_command(&self, name: &str, owner: Option<&str>) -> Result<ProcessSpec> {
        if owner.is_some() {
            return Err(invalid(
                "owner",
                "MySQL database ownership is expressed through an explicit user grant",
            ));
        }
        Ok(mysql_spec(&format!(
            "CREATE DATABASE IF NOT EXISTS `{name}`;\n"
        )))
    }

    fn create_user_command(&self, name: &str, password: &str) -> Result<ProcessSpec> {
        let escaped = password.replace('\\', "\\\\").replace('\'', "''");
        Ok(mysql_spec(&format!(
            "CREATE USER IF NOT EXISTS '{name}'@'localhost' IDENTIFIED BY '{escaped}';\nALTER USER '{name}'@'localhost' IDENTIFIED BY '{escaped}';\n"
        )))
    }

    fn grant_database_command(&self, database: &str, user: &str) -> Result<ProcessSpec> {
        Ok(mysql_spec(&format!(
            "GRANT ALL PRIVILEGES ON `{database}`.* TO '{user}'@'localhost';\n"
        )))
    }
}

#[derive(Debug)]
pub struct PostgresqlDriver;

impl ServiceDriver for PostgresqlDriver {
    fn id(&self) -> &'static str {
        "postgresql"
    }

    fn default_configuration(&self) -> ServiceConfiguration {
        ServiceConfiguration {
            bind_address: "127.0.0.1".into(),
            port: 5432,
            settings: BTreeMap::new(),
        }
    }

    fn validate_configuration(&self, configuration: &ServiceConfiguration) -> Result<()> {
        validate_settings(
            self.id(),
            configuration,
            &["max_connections", "shared_buffers", "work_mem"],
        )
    }

    fn paths(&self, service: &ManagedService, discovered_config: Option<PathBuf>) -> ServicePaths {
        ServicePaths {
            systemd_unit: service.systemd_unit.clone(),
            configuration_paths: vec![
                discovered_config
                    .unwrap_or_else(|| PathBuf::from("/etc/postgresql/*/*/conf.d/99-lumic.conf"))
                    .to_string_lossy()
                    .into_owned(),
            ],
            data_path: "/var/lib/postgresql".into(),
            log_source: format!("journalctl --unit {}", service.systemd_unit),
        }
    }

    fn health_probe(&self, service: &ManagedService) -> ProcessSpec {
        ProcessSpec::new("pg_isready").args([
            "--host",
            &service.configuration.bind_address,
            "--port",
            &service.configuration.port.to_string(),
        ])
    }

    fn configuration_files(
        &self,
        service: &ManagedService,
        discovered_config: Option<PathBuf>,
        _secrets: &BTreeMap<String, String>,
    ) -> Result<Vec<DriverConfigurationFile>> {
        let path = discovered_config.ok_or_else(|| {
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
        Ok(vec![DriverConfigurationFile {
            path,
            content,
            mode: 0o640,
            owner: "root:postgres",
        }])
    }

    fn backup_plan(
        &self,
        _service: &ManagedService,
        database: Option<&str>,
        directory: &Path,
        backup_id: &str,
    ) -> Result<DriverBackupPlan> {
        let database =
            database.ok_or_else(|| invalid("database", "PostgreSQL backup requires a database"))?;
        let path = directory.join(format!("{backup_id}.dump"));
        Ok(DriverBackupPlan {
            commands: vec![
                ProcessSpec::new("chown").args([
                    "postgres:postgres",
                    "--",
                    directory.to_string_lossy().as_ref(),
                ]),
                ProcessSpec::new("runuser").args([
                    "-u",
                    "postgres",
                    "--",
                    "pg_dump",
                    "--format=custom",
                    "--file",
                    path.to_string_lossy().as_ref(),
                    "--",
                    database,
                ]),
            ],
            path,
            copy_source: None,
        })
    }

    fn restore_plan(
        &self,
        backup_path: &Path,
        database: Option<&str>,
    ) -> Result<DriverRestorePlan> {
        let database =
            database.ok_or_else(|| invalid("backup", "PostgreSQL backup has no database"))?;
        Ok(DriverRestorePlan {
            stop_service: false,
            commands: vec![ProcessSpec::new("runuser").args([
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
                backup_path.to_string_lossy().as_ref(),
            ])],
            replacement: None,
        })
    }

    fn create_database_command(&self, name: &str, owner: Option<&str>) -> Result<ProcessSpec> {
        let owner_clause = owner
            .map(|value| format!(" OWNER \"{value}\""))
            .unwrap_or_default();
        Ok(psql_spec(&format!(
            "SELECT 'CREATE DATABASE \"{name}\"{owner_clause}' WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = '{name}') \\gexec\n"
        )))
    }

    fn create_user_command(&self, name: &str, password: &str) -> Result<ProcessSpec> {
        let escaped = password.replace('\'', "''");
        Ok(psql_spec(&format!(
            "DO $lumic$ BEGIN IF EXISTS (SELECT FROM pg_roles WHERE rolname = '{name}') THEN ALTER ROLE \"{name}\" PASSWORD '{escaped}'; ELSE CREATE ROLE \"{name}\" LOGIN PASSWORD '{escaped}'; END IF; END $lumic$;\n"
        )))
    }

    fn grant_database_command(&self, database: &str, user: &str) -> Result<ProcessSpec> {
        Ok(psql_spec(&format!(
            "GRANT ALL PRIVILEGES ON DATABASE \"{database}\" TO \"{user}\";\n"
        )))
    }
}

#[derive(Debug)]
pub struct RedisDriver;

impl ServiceDriver for RedisDriver {
    fn id(&self) -> &'static str {
        "redis"
    }

    fn default_configuration(&self) -> ServiceConfiguration {
        ServiceConfiguration {
            bind_address: "127.0.0.1".into(),
            port: 6379,
            settings: BTreeMap::new(),
        }
    }

    fn validate_configuration(&self, configuration: &ServiceConfiguration) -> Result<()> {
        validate_settings(
            self.id(),
            configuration,
            &["maxmemory", "maxmemory_policy", "timeout"],
        )
    }

    fn paths(&self, service: &ManagedService, _discovered_config: Option<PathBuf>) -> ServicePaths {
        ServicePaths {
            systemd_unit: service.systemd_unit.clone(),
            configuration_paths: vec![
                "/etc/redis/redis.conf".into(),
                "/etc/redis/lumic.conf".into(),
            ],
            data_path: "/var/lib/redis".into(),
            log_source: format!("journalctl --unit {}", service.systemd_unit),
        }
    }

    fn health_probe(&self, service: &ManagedService) -> ProcessSpec {
        ProcessSpec::new("redis-cli").args([
            "-h",
            &service.configuration.bind_address,
            "-p",
            &service.configuration.port.to_string(),
            "PING",
        ])
    }

    fn configuration_files(
        &self,
        service: &ManagedService,
        _discovered_config: Option<PathBuf>,
        _secrets: &BTreeMap<String, String>,
    ) -> Result<Vec<DriverConfigurationFile>> {
        let main = PathBuf::from("/etc/redis/redis.conf");
        let include = "include /etc/redis/lumic.conf";
        let existing = fs::read_to_string(&main).map_err(driver_io)?;
        let mut files = Vec::new();
        if !existing.lines().any(|line| line.trim() == include) {
            files.push(DriverConfigurationFile {
                path: main,
                content: format!("{}\n{include}\n", existing.trim_end()),
                mode: 0o640,
                owner: "root:redis",
            });
        }
        let mut content = format!(
            "# Managed by Lumic\nbind {}\nport {}\n",
            service.configuration.bind_address, service.configuration.port
        );
        for (key, value) in &service.configuration.settings {
            content.push_str(&format!("{key} {value}\n"));
        }
        files.push(DriverConfigurationFile {
            path: PathBuf::from("/etc/redis/lumic.conf"),
            content,
            mode: 0o640,
            owner: "root:redis",
        });
        Ok(files)
    }

    fn backup_plan(
        &self,
        service: &ManagedService,
        database: Option<&str>,
        directory: &Path,
        backup_id: &str,
    ) -> Result<DriverBackupPlan> {
        if database.is_some() {
            return Err(invalid(
                "database",
                "Redis backups do not select a database",
            ));
        }
        Ok(DriverBackupPlan {
            path: directory.join(format!("{backup_id}.rdb")),
            commands: vec![redis_cli_spec(service, &["SAVE"])],
            copy_source: Some(PathBuf::from("/var/lib/redis/dump.rdb")),
        })
    }

    fn restore_plan(
        &self,
        _backup_path: &Path,
        database: Option<&str>,
    ) -> Result<DriverRestorePlan> {
        if database.is_some() {
            return Err(invalid(
                "backup",
                "Redis backup unexpectedly names a database",
            ));
        }
        Ok(DriverRestorePlan {
            stop_service: true,
            commands: Vec::new(),
            replacement: Some(DriverRestoreReplacement {
                target: PathBuf::from("/var/lib/redis/dump.rdb"),
                safety_copy: PathBuf::from("/var/lib/redis/dump.rdb.lumic-before-restore"),
                owner: "redis:redis",
            }),
        })
    }

    fn create_database_command(&self, _name: &str, _owner: Option<&str>) -> Result<ProcessSpec> {
        Err(unsupported_resource(self.id()))
    }

    fn create_user_command(&self, _name: &str, _password: &str) -> Result<ProcessSpec> {
        Err(unsupported_resource(self.id()))
    }

    fn grant_database_command(&self, _database: &str, _user: &str) -> Result<ProcessSpec> {
        Err(unsupported_resource(self.id()))
    }
}

#[derive(Debug)]
pub struct TypesenseDriver;

impl ServiceDriver for TypesenseDriver {
    fn id(&self) -> &'static str {
        "typesense"
    }

    fn secret_names(&self) -> &'static [&'static str] {
        &["api_key"]
    }

    fn default_configuration(&self) -> ServiceConfiguration {
        ServiceConfiguration {
            bind_address: "127.0.0.1".into(),
            port: 8108,
            settings: BTreeMap::from([
                ("cors".into(), "false".into()),
                ("data_directory".into(), "/var/lib/typesense".into()),
            ]),
        }
    }

    fn validate_configuration(&self, configuration: &ServiceConfiguration) -> Result<()> {
        validate_search_settings(self.id(), configuration, "/var/lib/typesense", true)
    }

    fn paths(&self, service: &ManagedService, _discovered_config: Option<PathBuf>) -> ServicePaths {
        ServicePaths {
            systemd_unit: service.systemd_unit.clone(),
            configuration_paths: vec!["/etc/typesense/typesense-server.ini".into()],
            data_path: "/var/lib/typesense".into(),
            log_source: format!("journalctl --unit {}", service.systemd_unit),
        }
    }

    fn health_probe(&self, service: &ManagedService) -> ProcessSpec {
        ProcessSpec::new("curl").args([
            "--fail",
            "--silent",
            "--show-error",
            &format!(
                "http://{}:{}/health",
                service.configuration.bind_address, service.configuration.port
            ),
        ])
    }

    fn configuration_files(
        &self,
        service: &ManagedService,
        _discovered_config: Option<PathBuf>,
        secrets: &BTreeMap<String, String>,
    ) -> Result<Vec<DriverConfigurationFile>> {
        let api_key = required_secret(secrets, "api_key")?;
        let data_directory = setting(service, "data_directory")?;
        let cors = setting(service, "cors")?;
        Ok(vec![DriverConfigurationFile {
            path: PathBuf::from("/etc/typesense/typesense-server.ini"),
            content: format!(
                "# Managed by Lumic\napi-key = {api_key}\ndata-dir = {data_directory}\nlisten-address = {}\napi-port = {}\nenable-cors = {cors}\n",
                service.configuration.bind_address, service.configuration.port
            ),
            mode: 0o640,
            owner: "root:typesense",
        }])
    }

    fn backup_plan(
        &self,
        _service: &ManagedService,
        _database: Option<&str>,
        _directory: &Path,
        _backup_id: &str,
    ) -> Result<DriverBackupPlan> {
        Err(unsupported_resource(self.id()))
    }

    fn restore_plan(
        &self,
        _backup_path: &Path,
        _database: Option<&str>,
    ) -> Result<DriverRestorePlan> {
        Err(unsupported_resource(self.id()))
    }

    fn create_database_command(&self, _name: &str, _owner: Option<&str>) -> Result<ProcessSpec> {
        Err(unsupported_resource(self.id()))
    }

    fn create_user_command(&self, _name: &str, _password: &str) -> Result<ProcessSpec> {
        Err(unsupported_resource(self.id()))
    }

    fn grant_database_command(&self, _database: &str, _user: &str) -> Result<ProcessSpec> {
        Err(unsupported_resource(self.id()))
    }
}

#[derive(Debug)]
pub struct MeilisearchDriver;

impl ServiceDriver for MeilisearchDriver {
    fn id(&self) -> &'static str {
        "meilisearch"
    }

    fn secret_names(&self) -> &'static [&'static str] {
        &["master_key"]
    }

    fn default_configuration(&self) -> ServiceConfiguration {
        ServiceConfiguration {
            bind_address: "127.0.0.1".into(),
            port: 7700,
            settings: BTreeMap::from([("data_directory".into(), "/var/lib/meilisearch".into())]),
        }
    }

    fn validate_configuration(&self, configuration: &ServiceConfiguration) -> Result<()> {
        validate_search_settings(self.id(), configuration, "/var/lib/meilisearch", false)
    }

    fn paths(&self, service: &ManagedService, _discovered_config: Option<PathBuf>) -> ServicePaths {
        ServicePaths {
            systemd_unit: service.systemd_unit.clone(),
            configuration_paths: vec![
                "/etc/meilisearch.env".into(),
                "/etc/systemd/system/meilisearch.service".into(),
            ],
            data_path: "/var/lib/meilisearch".into(),
            log_source: format!("journalctl --unit {}", service.systemd_unit),
        }
    }

    fn health_probe(&self, service: &ManagedService) -> ProcessSpec {
        ProcessSpec::new("curl").args([
            "--fail",
            "--silent",
            "--show-error",
            &format!(
                "http://{}:{}/health",
                service.configuration.bind_address, service.configuration.port
            ),
        ])
    }

    fn configuration_files(
        &self,
        service: &ManagedService,
        _discovered_config: Option<PathBuf>,
        secrets: &BTreeMap<String, String>,
    ) -> Result<Vec<DriverConfigurationFile>> {
        let master_key = required_secret(secrets, "master_key")?;
        let data_directory = setting(service, "data_directory")?;
        Ok(vec![
            DriverConfigurationFile {
                path: PathBuf::from("/etc/meilisearch.env"),
                content: format!(
                    "# Managed by Lumic\nMEILI_ENV=production\nMEILI_HTTP_ADDR={}:{}\nMEILI_DB_PATH={data_directory}\nMEILI_MASTER_KEY={master_key}\nMEILI_NO_ANALYTICS=true\n",
                    service.configuration.bind_address, service.configuration.port
                ),
                mode: 0o600,
                owner: "root:root",
            },
            DriverConfigurationFile {
                path: PathBuf::from("/etc/systemd/system/meilisearch.service"),
                content: "# Managed by Lumic\n[Unit]\nDescription=Meilisearch search service\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nType=simple\nDynamicUser=yes\nStateDirectory=meilisearch\nEnvironmentFile=/etc/meilisearch.env\nExecStart=/usr/bin/meilisearch\nRestart=on-failure\nRestartSec=5s\nNoNewPrivileges=yes\nPrivateTmp=yes\nProtectSystem=strict\nProtectHome=yes\nReadWritePaths=/var/lib/meilisearch\n\n[Install]\nWantedBy=multi-user.target\n".into(),
                mode: 0o644,
                owner: "root:root",
            },
        ])
    }

    fn backup_plan(
        &self,
        _service: &ManagedService,
        _database: Option<&str>,
        _directory: &Path,
        _backup_id: &str,
    ) -> Result<DriverBackupPlan> {
        Err(unsupported_resource(self.id()))
    }

    fn restore_plan(
        &self,
        _backup_path: &Path,
        _database: Option<&str>,
    ) -> Result<DriverRestorePlan> {
        Err(unsupported_resource(self.id()))
    }

    fn create_database_command(&self, _name: &str, _owner: Option<&str>) -> Result<ProcessSpec> {
        Err(unsupported_resource(self.id()))
    }

    fn create_user_command(&self, _name: &str, _password: &str) -> Result<ProcessSpec> {
        Err(unsupported_resource(self.id()))
    }

    fn grant_database_command(&self, _database: &str, _user: &str) -> Result<ProcessSpec> {
        Err(unsupported_resource(self.id()))
    }
}

fn validate_search_settings(
    driver: &str,
    configuration: &ServiceConfiguration,
    data_directory: &str,
    supports_cors: bool,
) -> Result<()> {
    let allowed = if supports_cors {
        &["cors", "data_directory"][..]
    } else {
        &["data_directory"][..]
    };
    validate_settings(driver, configuration, allowed)?;
    if setting_raw(configuration, "data_directory") != Some(data_directory) {
        return Err(invalid(
            "settings.data_directory",
            "the managed search data directory cannot be changed",
        ));
    }
    if supports_cors && !matches!(setting_raw(configuration, "cors"), Some("true" | "false")) {
        return Err(invalid("settings.cors", "must be true or false"));
    }
    Ok(())
}

fn setting<'a>(service: &'a ManagedService, name: &str) -> Result<&'a str> {
    setting_raw(&service.configuration, name)
        .ok_or_else(|| invalid("settings", &format!("missing required setting '{name}'")))
}

fn setting_raw<'a>(configuration: &'a ServiceConfiguration, name: &str) -> Option<&'a str> {
    configuration.settings.get(name).map(String::as_str)
}

fn required_secret<'a>(secrets: &'a BTreeMap<String, String>, name: &str) -> Result<&'a str> {
    secrets
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| invalid("secret", &format!("missing required secret '{name}'")))
}

fn psql_spec(sql: &str) -> ProcessSpec {
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
    spec
}

fn mysql_spec(sql: &str) -> ProcessSpec {
    let mut spec = ProcessSpec::new("mysql").args(["--protocol=socket", "--batch"]);
    spec.timeout = Duration::from_secs(60);
    spec.stdin = Some(sql.as_bytes().to_vec());
    spec
}

fn redis_cli_spec(service: &ManagedService, arguments: &[&str]) -> ProcessSpec {
    let mut spec = ProcessSpec::new("redis-cli").args([
        "-h",
        &service.configuration.bind_address,
        "-p",
        &service.configuration.port.to_string(),
    ]);
    spec.args
        .extend(arguments.iter().map(|value| (*value).to_owned()));
    spec
}

fn unsupported_resource(driver: &str) -> LumicError {
    invalid(
        "service",
        &format!("driver '{driver}' does not support database resources"),
    )
}

fn driver_io(error: std::io::Error) -> LumicError {
    LumicError::Internal {
        message: format!("service driver I/O failed: {error}"),
    }
}

fn validate_settings(
    driver: &str,
    configuration: &ServiceConfiguration,
    allowed: &[&str],
) -> Result<()> {
    configuration.validate()?;
    if let Some(key) = configuration
        .settings
        .keys()
        .find(|key| !allowed.contains(&key.as_str()))
    {
        return Err(invalid(
            "settings",
            &format!("unsupported {driver} setting: {key}"),
        ));
    }
    Ok(())
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
    use lumic_core::managed_service::DesiredServiceState;

    fn service(kind: ManagedServiceKind) -> ManagedService {
        let registry = ServiceDriverRegistry::built_in().unwrap();
        ManagedService {
            id: kind.id().into(),
            name: kind.id().into(),
            kind,
            package: kind.id().into(),
            systemd_unit: format!("{}.service", kind.id()),
            desired_state: DesiredServiceState::Running,
            configuration: registry
                .legacy_driver(kind)
                .unwrap()
                .default_configuration(),
            secret_references: Vec::new(),
            dependencies: Vec::new(),
            created_at_unix_ms: 1,
            updated_at_unix_ms: 1,
        }
    }

    #[test]
    fn registry_resolves_catalog_driver_ids() {
        let registry = ServiceDriverRegistry::built_in().unwrap();
        let definition = registry.definition("redis").unwrap();
        assert_eq!(registry.driver(&definition.driver).unwrap().id(), "redis");
        assert_eq!(registry.driver("typesense").unwrap().id(), "typesense");
        assert_eq!(registry.driver("meilisearch").unwrap().id(), "meilisearch");
        assert!(registry.driver("community-shell-plugin").is_err());
    }

    #[test]
    fn drivers_own_provider_configuration_validation() {
        let registry = ServiceDriverRegistry::built_in().unwrap();
        let driver = registry.driver("redis").unwrap();
        let mut configuration = driver.default_configuration();
        configuration
            .settings
            .insert("requirepass".into(), "unsafe".into());
        assert!(driver.validate_configuration(&configuration).is_err());
    }

    #[test]
    fn redis_backup_plan_uses_supported_connection_flags() {
        let service = service(ManagedServiceKind::Redis);
        let plan = RedisDriver
            .backup_plan(&service, None, Path::new("/tmp"), "cache-1")
            .unwrap();
        assert_eq!(plan.commands[0].executable, "redis-cli");
        assert_eq!(
            plan.commands[0].args,
            ["-h", "127.0.0.1", "-p", "6379", "SAVE"]
        );
    }

    #[test]
    fn mysql_driver_uses_socket_auth_and_stdin_for_credentials() {
        let service = service(ManagedServiceKind::Mysql);
        let health = MysqlDriver.health_probe(&service);
        assert_eq!(health.executable, "mysqladmin");
        assert!(health.args.contains(&"--protocol=socket".into()));

        let user = MysqlDriver.create_user_command("demo", "private").unwrap();
        assert_eq!(user.executable, "mysql");
        assert!(
            !user
                .args
                .iter()
                .any(|argument| argument.contains("private"))
        );
        assert!(user.stdin.as_deref().is_some_and(|sql| {
            String::from_utf8_lossy(sql).contains("IDENTIFIED BY 'private'")
        }));
    }

    #[test]
    fn drivers_reject_unsupported_child_resources() {
        assert!(RedisDriver.create_database_command("app", None).is_err());
        assert!(
            PostgresqlDriver
                .create_database_command("app", Some("owner"))
                .is_ok()
        );
    }

    #[test]
    fn typesense_configuration_uses_managed_api_key() {
        let service = service(ManagedServiceKind::Typesense);
        let secrets = BTreeMap::from([("api_key".into(), "private-api-key".into())]);
        let files = TypesenseDriver
            .configuration_files(&service, None, &secrets)
            .unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].mode, 0o640);
        assert!(files[0].content.contains("api-key = private-api-key"));
        assert!(files[0].content.contains("listen-address = 127.0.0.1"));
        assert!(
            !TypesenseDriver
                .health_probe(&service)
                .args
                .iter()
                .any(|arg| { arg.contains("private-api-key") })
        );
    }

    #[test]
    fn meilisearch_configuration_uses_private_environment_and_hardened_unit() {
        let service = service(ManagedServiceKind::Meilisearch);
        let secrets = BTreeMap::from([("master_key".into(), "private-master-key".into())]);
        let files = MeilisearchDriver
            .configuration_files(&service, None, &secrets)
            .unwrap();

        assert_eq!(files.len(), 2);
        let environment = files
            .iter()
            .find(|file| file.path == Path::new("/etc/meilisearch.env"))
            .unwrap();
        assert_eq!(environment.mode, 0o600);
        assert!(
            environment
                .content
                .contains("MEILI_MASTER_KEY=private-master-key")
        );
        let unit = files
            .iter()
            .find(|file| file.path == Path::new("/etc/systemd/system/meilisearch.service"))
            .unwrap();
        assert!(unit.content.contains("DynamicUser=yes"));
        assert!(unit.content.contains("ProtectSystem=strict"));
    }
}
