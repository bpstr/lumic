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
use crate::git_forge::{GITEA_SPEC, GOGS_SPEC, GitForgeSpec};

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
    fn package_install_environment(&self) -> &'static [(&'static str, &'static str)] {
        &[]
    }
    fn secret_names(&self) -> &'static [&'static str] {
        &[]
    }
    fn git_forge_spec(&self) -> Option<&'static GitForgeSpec> {
        None
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
        registry.register(Box::new(GitForgeServiceDriver {
            specification: &GITEA_SPEC,
        }))?;
        registry.register(Box::new(GitForgeServiceDriver {
            specification: &GOGS_SPEC,
        }))?;
        for specification in NATIVE_SERVICE_SPECS {
            registry.register(Box::new(NativeServiceDriver { specification }))?;
        }
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
struct GitForgeServiceDriver {
    specification: &'static GitForgeSpec,
}

impl ServiceDriver for GitForgeServiceDriver {
    fn id(&self) -> &'static str {
        self.specification.id
    }

    fn secret_names(&self) -> &'static [&'static str] {
        if self.id() == "gitea" {
            &["secret_key", "internal_token"]
        } else {
            &["secret_key"]
        }
    }

    fn git_forge_spec(&self) -> Option<&'static GitForgeSpec> {
        Some(self.specification)
    }

    fn default_configuration(&self) -> ServiceConfiguration {
        ServiceConfiguration {
            bind_address: "127.0.0.1".into(),
            port: if self.id() == "gitea" { 3000 } else { 3001 },
            settings: BTreeMap::from([(
                "repository_root".into(),
                "/var/lib/lumic/repositories".into(),
            )]),
        }
    }

    fn validate_configuration(&self, configuration: &ServiceConfiguration) -> Result<()> {
        validate_settings(self.id(), configuration, &["repository_root"])?;
        let root_value = setting_raw(configuration, "repository_root")
            .ok_or_else(|| invalid("settings", "missing required setting 'repository_root'"))?;
        if root_value.chars().any(char::is_whitespace) {
            return Err(invalid(
                "settings.repository_root",
                "must not contain whitespace",
            ));
        }
        let root = Path::new(root_value);
        lumic_core::repository::validate_absolute_path("repository_root", root)
    }

    fn paths(&self, service: &ManagedService, _discovered_config: Option<PathBuf>) -> ServicePaths {
        ServicePaths {
            systemd_unit: service.systemd_unit.clone(),
            configuration_paths: vec![
                format!("/etc/{}/app.ini", self.id()),
                format!("/etc/systemd/system/{}.service", self.id()),
            ],
            data_path: self.specification.data_path.into(),
            log_source: format!("journalctl --unit {}", service.systemd_unit),
        }
    }

    fn health_probe(&self, service: &ManagedService) -> ProcessSpec {
        let path = if self.id() == "gitea" {
            "/api/healthz"
        } else {
            "/"
        };
        ProcessSpec::new("curl").args([
            "--fail",
            "--silent",
            "--show-error",
            &format!(
                "http://{}:{}{path}",
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
        let repository_root = setting(service, "repository_root")?;
        let secret_key = required_secret(secrets, "secret_key")?;
        let content = if self.id() == "gitea" {
            let internal_token = required_secret(secrets, "internal_token")?;
            format!(
                "# Managed by Lumic\nAPP_NAME = Gitea on Lumic\nRUN_MODE = prod\nRUN_USER = gitea\nWORK_PATH = /var/lib/gitea\n\n[repository]\nROOT = {repository_root}\n\n[server]\nPROTOCOL = http\nHTTP_ADDR = {}\nHTTP_PORT = {}\nROOT_URL = http://{}:{}/\nDISABLE_SSH = true\nSTART_SSH_SERVER = false\nOFFLINE_MODE = true\n\n[database]\nDB_TYPE = sqlite3\nPATH = /var/lib/gitea/data/gitea.db\n\n[security]\nINSTALL_LOCK = true\nSECRET_KEY = {secret_key}\nINTERNAL_TOKEN = {internal_token}\n",
                service.configuration.bind_address,
                service.configuration.port,
                service.configuration.bind_address,
                service.configuration.port,
            )
        } else {
            format!(
                "# Managed by Lumic\nRUN_MODE = prod\nRUN_USER = gogs\n\n[repository]\nROOT = {repository_root}\n\n[server]\nPROTOCOL = http\nHTTP_ADDR = {}\nHTTP_PORT = {}\nEXTERNAL_URL = http://{}:{}/\nDISABLE_SSH = true\nSTART_SSH_SERVER = false\nOFFLINE_MODE = true\n\n[database]\nTYPE = sqlite3\nPATH = /var/lib/gogs/gogs.db\n\n[security]\nINSTALL_LOCK = true\nSECRET_KEY = {secret_key}\n",
                service.configuration.bind_address,
                service.configuration.port,
                service.configuration.bind_address,
                service.configuration.port,
            )
        };
        let unit = format!(
            "# Managed by Lumic\n[Unit]\nDescription={} Git forge\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nType=simple\nUser={}\nGroup=lumic-git\nWorkingDirectory={}\nUMask=0007\nExecStart={} web --config /etc/{}/app.ini\nRestart=on-failure\nRestartSec=5s\nNoNewPrivileges=yes\nPrivateTmp=yes\nProtectSystem=strict\nProtectHome=yes\nReadWritePaths={} {}\n\n[Install]\nWantedBy=multi-user.target\n",
            if self.id() == "gitea" {
                "Gitea"
            } else {
                "Gogs"
            },
            self.specification.user,
            self.specification.data_path,
            self.specification.binary_path,
            self.id(),
            self.specification.data_path,
            repository_root,
        );
        Ok(vec![
            DriverConfigurationFile {
                path: PathBuf::from(format!("/etc/{}/app.ini", self.id())),
                content,
                mode: 0o640,
                owner: "root:lumic-git",
            },
            DriverConfigurationFile {
                path: PathBuf::from(format!("/etc/systemd/system/{}.service", self.id())),
                content: unit,
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
        Err(unsupported_operation(self.id(), "backup"))
    }

    fn restore_plan(
        &self,
        _backup_path: &Path,
        _database: Option<&str>,
    ) -> Result<DriverRestorePlan> {
        Err(unsupported_operation(self.id(), "restore"))
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
            content.push_str(&format!("{} {value}\n", redis_directive_name(key)));
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

type ConfigurationRenderer =
    fn(&ManagedService, &BTreeMap<String, String>) -> Result<Vec<DriverConfigurationFile>>;
type HealthProbe = fn(&ManagedService) -> ProcessSpec;
type DefaultConfiguration = fn() -> ServiceConfiguration;
type ConfigurationValidator = fn(&ServiceConfiguration) -> Result<()>;

#[derive(Debug)]
struct NativeServiceSpec {
    id: &'static str,
    install_environment: &'static [(&'static str, &'static str)],
    secrets: &'static [&'static str],
    allowed_settings: &'static [&'static str],
    configuration_paths: &'static [&'static str],
    data_path: &'static str,
    defaults: DefaultConfiguration,
    validate: ConfigurationValidator,
    health: HealthProbe,
    render: ConfigurationRenderer,
}

#[derive(Debug)]
struct NativeServiceDriver {
    specification: &'static NativeServiceSpec,
}

impl ServiceDriver for NativeServiceDriver {
    fn id(&self) -> &'static str {
        self.specification.id
    }

    fn package_install_environment(&self) -> &'static [(&'static str, &'static str)] {
        self.specification.install_environment
    }

    fn secret_names(&self) -> &'static [&'static str] {
        self.specification.secrets
    }

    fn default_configuration(&self) -> ServiceConfiguration {
        (self.specification.defaults)()
    }

    fn validate_configuration(&self, configuration: &ServiceConfiguration) -> Result<()> {
        validate_settings(
            self.id(),
            configuration,
            self.specification.allowed_settings,
        )?;
        (self.specification.validate)(configuration)
    }

    fn paths(&self, service: &ManagedService, _discovered_config: Option<PathBuf>) -> ServicePaths {
        ServicePaths {
            systemd_unit: service.systemd_unit.clone(),
            configuration_paths: self
                .specification
                .configuration_paths
                .iter()
                .map(|path| (*path).to_owned())
                .collect(),
            data_path: self.specification.data_path.into(),
            log_source: format!("journalctl --unit {}", service.systemd_unit),
        }
    }

    fn health_probe(&self, service: &ManagedService) -> ProcessSpec {
        (self.specification.health)(service)
    }

    fn configuration_files(
        &self,
        service: &ManagedService,
        _discovered_config: Option<PathBuf>,
        secrets: &BTreeMap<String, String>,
    ) -> Result<Vec<DriverConfigurationFile>> {
        (self.specification.render)(service, secrets)
    }

    fn backup_plan(
        &self,
        _service: &ManagedService,
        _database: Option<&str>,
        _directory: &Path,
        _backup_id: &str,
    ) -> Result<DriverBackupPlan> {
        Err(unsupported_operation(self.id(), "backup"))
    }

    fn restore_plan(
        &self,
        _backup_path: &Path,
        _database: Option<&str>,
    ) -> Result<DriverRestorePlan> {
        Err(unsupported_operation(self.id(), "restore"))
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

static NATIVE_SERVICE_SPECS: &[NativeServiceSpec] = &[
    NativeServiceSpec {
        id: "valkey",
        install_environment: &[],
        secrets: &[],
        allowed_settings: &["maxmemory", "maxmemory_policy"],
        configuration_paths: &["/etc/valkey/valkey.conf"],
        data_path: "/var/lib/valkey",
        defaults: valkey_defaults,
        validate: validate_valkey,
        health: valkey_health,
        render: render_valkey,
    },
    NativeServiceSpec {
        id: "rabbitmq",
        install_environment: &[],
        secrets: &[],
        allowed_settings: &["memory_high_watermark"],
        configuration_paths: &["/etc/rabbitmq/rabbitmq.conf"],
        data_path: "/var/lib/rabbitmq",
        defaults: rabbitmq_defaults,
        validate: validate_rabbitmq,
        health: rabbitmq_health,
        render: render_rabbitmq,
    },
    NativeServiceSpec {
        id: "minio",
        install_environment: &[],
        secrets: &["root_user", "root_password"],
        allowed_settings: &["console_port"],
        configuration_paths: &["/etc/default/minio", "/etc/systemd/system/minio.service"],
        data_path: "/var/lib/minio",
        defaults: minio_defaults,
        validate: validate_minio,
        health: minio_health,
        render: render_minio,
    },
    NativeServiceSpec {
        id: "opensearch",
        install_environment: OPENSEARCH_INSTALL_ENVIRONMENT,
        secrets: &[],
        allowed_settings: &["cluster_name"],
        configuration_paths: &["/etc/opensearch/opensearch.yml"],
        data_path: "/var/lib/opensearch",
        defaults: opensearch_defaults,
        validate: validate_opensearch,
        health: opensearch_health,
        render: render_opensearch,
    },
    NativeServiceSpec {
        id: "memcached",
        install_environment: &[],
        secrets: &[],
        allowed_settings: &["memory_mb"],
        configuration_paths: &["/etc/memcached.conf"],
        data_path: "/var/lib/memcached",
        defaults: memcached_defaults,
        validate: validate_memcached,
        health: systemd_health,
        render: render_memcached,
    },
    NativeServiceSpec {
        id: "mongodb",
        install_environment: &[],
        secrets: &[],
        allowed_settings: &[],
        configuration_paths: &["/etc/mongod.conf"],
        data_path: "/var/lib/mongodb",
        defaults: mongodb_defaults,
        validate: validate_no_settings,
        health: mongodb_health,
        render: render_mongodb,
    },
    NativeServiceSpec {
        id: "clickhouse",
        install_environment: &[],
        secrets: &[],
        allowed_settings: &[],
        configuration_paths: &["/etc/clickhouse-server/config.d/lumic.xml"],
        data_path: "/var/lib/clickhouse",
        defaults: clickhouse_defaults,
        validate: validate_no_settings,
        health: clickhouse_health,
        render: render_clickhouse,
    },
    NativeServiceSpec {
        id: "prometheus",
        install_environment: &[],
        secrets: &[],
        allowed_settings: &["scrape_interval"],
        configuration_paths: &[
            "/etc/prometheus/prometheus.yml",
            "/etc/systemd/system/prometheus.service.d/lumic.conf",
        ],
        data_path: "/var/lib/prometheus",
        defaults: prometheus_defaults,
        validate: validate_prometheus,
        health: prometheus_health,
        render: render_prometheus,
    },
    NativeServiceSpec {
        id: "grafana",
        install_environment: &[],
        secrets: &["admin_password"],
        allowed_settings: &[],
        configuration_paths: &["/etc/grafana/grafana.ini"],
        data_path: "/var/lib/grafana",
        defaults: grafana_defaults,
        validate: validate_no_settings,
        health: grafana_health,
        render: render_grafana,
    },
    NativeServiceSpec {
        id: "loki",
        install_environment: &[],
        secrets: &[],
        allowed_settings: &["retention_period"],
        configuration_paths: &["/etc/loki/config.yml"],
        data_path: "/var/lib/loki",
        defaults: loki_defaults,
        validate: validate_loki,
        health: loki_health,
        render: render_loki,
    },
];

pub(crate) const OPENSEARCH_INSTALL_ENVIRONMENT: &[(&str, &str)] = &[
    ("DISABLE_INSTALL_DEMO_CONFIG", "true"),
    ("DISABLE_SECURITY_PLUGIN", "true"),
];

fn service_defaults(port: u16, settings: &[(&str, &str)]) -> ServiceConfiguration {
    ServiceConfiguration {
        bind_address: "127.0.0.1".into(),
        port,
        settings: settings
            .iter()
            .map(|(key, value)| ((*key).into(), (*value).into()))
            .collect(),
    }
}

fn valkey_defaults() -> ServiceConfiguration {
    service_defaults(
        6379,
        &[("maxmemory", "0"), ("maxmemory_policy", "noeviction")],
    )
}
fn rabbitmq_defaults() -> ServiceConfiguration {
    service_defaults(5672, &[("memory_high_watermark", "0.4")])
}
fn minio_defaults() -> ServiceConfiguration {
    service_defaults(9000, &[("console_port", "9001")])
}
fn opensearch_defaults() -> ServiceConfiguration {
    service_defaults(9200, &[("cluster_name", "lumic")])
}
fn memcached_defaults() -> ServiceConfiguration {
    service_defaults(11211, &[("memory_mb", "64")])
}
fn mongodb_defaults() -> ServiceConfiguration {
    service_defaults(27017, &[])
}
fn clickhouse_defaults() -> ServiceConfiguration {
    service_defaults(8123, &[])
}
fn prometheus_defaults() -> ServiceConfiguration {
    service_defaults(9090, &[("scrape_interval", "15s")])
}
fn grafana_defaults() -> ServiceConfiguration {
    service_defaults(3000, &[])
}
fn loki_defaults() -> ServiceConfiguration {
    service_defaults(3100, &[("retention_period", "168h")])
}

fn validate_no_settings(_configuration: &ServiceConfiguration) -> Result<()> {
    Ok(())
}

fn validate_valkey(configuration: &ServiceConfiguration) -> Result<()> {
    parse_u64_setting(configuration, "maxmemory", 0, u64::MAX)?;
    match setting_raw(configuration, "maxmemory_policy") {
        Some("noeviction" | "allkeys-lru" | "allkeys-lfu" | "volatile-lru") => Ok(()),
        _ => Err(invalid(
            "settings.maxmemory_policy",
            "must be noeviction, allkeys-lru, allkeys-lfu, or volatile-lru",
        )),
    }
}

fn validate_rabbitmq(configuration: &ServiceConfiguration) -> Result<()> {
    let value = required_setting(configuration, "memory_high_watermark")?
        .parse::<f64>()
        .map_err(|_| invalid("settings.memory_high_watermark", "must be a decimal ratio"))?;
    if !(0.0..1.0).contains(&value) || value == 0.0 {
        return Err(invalid(
            "settings.memory_high_watermark",
            "must be greater than 0 and less than 1",
        ));
    }
    Ok(())
}

fn validate_minio(configuration: &ServiceConfiguration) -> Result<()> {
    let console_port = parse_u64_setting(configuration, "console_port", 1, u16::MAX.into())?;
    if console_port == u64::from(configuration.port) {
        return Err(invalid(
            "settings.console_port",
            "must differ from the API port",
        ));
    }
    Ok(())
}

fn validate_opensearch(configuration: &ServiceConfiguration) -> Result<()> {
    let name = required_setting(configuration, "cluster_name")?;
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(invalid(
            "settings.cluster_name",
            "must contain only ASCII letters, numbers, dots, underscores, or hyphens",
        ));
    }
    Ok(())
}

fn validate_memcached(configuration: &ServiceConfiguration) -> Result<()> {
    parse_u64_setting(configuration, "memory_mb", 16, 1_048_576)?;
    Ok(())
}

fn validate_prometheus(configuration: &ServiceConfiguration) -> Result<()> {
    validate_duration_setting(configuration, "scrape_interval")
}

fn validate_loki(configuration: &ServiceConfiguration) -> Result<()> {
    validate_duration_setting(configuration, "retention_period")
}

fn parse_u64_setting(
    configuration: &ServiceConfiguration,
    name: &str,
    minimum: u64,
    maximum: u64,
) -> Result<u64> {
    let value = required_setting(configuration, name)?
        .parse::<u64>()
        .map_err(|_| invalid(&format!("settings.{name}"), "must be an integer"))?;
    if !(minimum..=maximum).contains(&value) {
        return Err(invalid(
            &format!("settings.{name}"),
            &format!("must be between {minimum} and {maximum}"),
        ));
    }
    Ok(value)
}

fn validate_duration_setting(configuration: &ServiceConfiguration, name: &str) -> Result<()> {
    let value = required_setting(configuration, name)?;
    let digit_count = value.bytes().take_while(u8::is_ascii_digit).count();
    let (number, unit) = value.split_at(digit_count);
    let valid_number = !number.is_empty() && number.parse::<u64>().is_ok_and(|value| value > 0);
    if !valid_number || !matches!(unit, "ms" | "s" | "m" | "h" | "d" | "w" | "y") {
        return Err(invalid(
            &format!("settings.{name}"),
            "must be a positive duration such as 15s, 5m, or 24h",
        ));
    }
    Ok(())
}

fn required_setting<'a>(configuration: &'a ServiceConfiguration, name: &str) -> Result<&'a str> {
    setting_raw(configuration, name)
        .ok_or_else(|| invalid("settings", &format!("missing required setting '{name}'")))
}

fn socket_address(bind_address: &str, port: u16) -> String {
    if bind_address.contains(':') {
        format!("[{bind_address}]:{port}")
    } else {
        format!("{bind_address}:{port}")
    }
}

fn http_health(service: &ManagedService, path: &str) -> ProcessSpec {
    let address = socket_address(
        &service.configuration.bind_address,
        service.configuration.port,
    );
    ProcessSpec::new("curl").args([
        "--fail",
        "--silent",
        "--show-error",
        &format!("http://{address}{path}"),
    ])
}

fn valkey_health(service: &ManagedService) -> ProcessSpec {
    ProcessSpec::new("valkey-cli").args([
        "-h",
        &service.configuration.bind_address,
        "-p",
        &service.configuration.port.to_string(),
        "PING",
    ])
}
fn rabbitmq_health(_service: &ManagedService) -> ProcessSpec {
    ProcessSpec::new("rabbitmq-diagnostics").args(["-q", "ping"])
}
fn minio_health(service: &ManagedService) -> ProcessSpec {
    http_health(service, "/minio/health/live")
}
fn opensearch_health(service: &ManagedService) -> ProcessSpec {
    http_health(service, "/_cluster/health")
}
fn systemd_health(service: &ManagedService) -> ProcessSpec {
    ProcessSpec::new("systemctl").args(["is-active", "--quiet", &service.systemd_unit])
}
fn mongodb_health(service: &ManagedService) -> ProcessSpec {
    ProcessSpec::new("mongosh").args([
        "--quiet",
        "--host",
        &service.configuration.bind_address,
        "--port",
        &service.configuration.port.to_string(),
        "--eval",
        "db.adminCommand({ ping: 1 })",
    ])
}
fn clickhouse_health(service: &ManagedService) -> ProcessSpec {
    http_health(service, "/ping")
}
fn prometheus_health(service: &ManagedService) -> ProcessSpec {
    http_health(service, "/-/healthy")
}
fn grafana_health(service: &ManagedService) -> ProcessSpec {
    http_health(service, "/api/health")
}
fn loki_health(service: &ManagedService) -> ProcessSpec {
    http_health(service, "/ready")
}

fn driver_file(
    path: &str,
    content: String,
    mode: u32,
    owner: &'static str,
) -> Vec<DriverConfigurationFile> {
    vec![DriverConfigurationFile {
        path: path.into(),
        content,
        mode,
        owner,
    }]
}

fn render_valkey(
    service: &ManagedService,
    _secrets: &BTreeMap<String, String>,
) -> Result<Vec<DriverConfigurationFile>> {
    Ok(driver_file(
        "/etc/valkey/valkey.conf",
        format!(
            "# Managed by Lumic\nbind {}\nport {}\ndir /var/lib/valkey\nmaxmemory {}\nmaxmemory-policy {}\n",
            service.configuration.bind_address,
            service.configuration.port,
            setting(service, "maxmemory")?,
            setting(service, "maxmemory_policy")?
        ),
        0o640,
        "root:valkey",
    ))
}
fn render_rabbitmq(
    service: &ManagedService,
    _secrets: &BTreeMap<String, String>,
) -> Result<Vec<DriverConfigurationFile>> {
    Ok(driver_file(
        "/etc/rabbitmq/rabbitmq.conf",
        format!(
            "# Managed by Lumic\nlisteners.tcp.1 = {}\nvm_memory_high_watermark.relative = {}\n",
            socket_address(
                &service.configuration.bind_address,
                service.configuration.port
            ),
            setting(service, "memory_high_watermark")?
        ),
        0o640,
        "root:rabbitmq",
    ))
}
fn render_minio(
    service: &ManagedService,
    secrets: &BTreeMap<String, String>,
) -> Result<Vec<DriverConfigurationFile>> {
    Ok(vec![
        DriverConfigurationFile {
            path: "/etc/default/minio".into(),
            content: format!(
            "# Managed by Lumic\nMINIO_VOLUMES=/var/lib/minio\nMINIO_OPTS=\"--address {} --console-address {}\"\nMINIO_ROOT_USER={}\nMINIO_ROOT_PASSWORD={}\n",
                socket_address(&service.configuration.bind_address, service.configuration.port),
                socket_address(
                    &service.configuration.bind_address,
                    setting(service, "console_port")?.parse::<u16>().map_err(|_| invalid("settings.console_port", "must be a port"))?
                ),
                required_secret(secrets, "root_user")?,
                required_secret(secrets, "root_password")?
            ),
            mode: 0o600,
            owner: "root:root",
        },
        DriverConfigurationFile {
            path: "/etc/systemd/system/minio.service".into(),
            content: "# Managed by Lumic\n[Unit]\nDescription=MinIO object storage\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nDynamicUser=yes\nStateDirectory=minio\nEnvironmentFile=/etc/default/minio\nExecStart=/usr/local/bin/minio server $MINIO_OPTS $MINIO_VOLUMES\nRestart=on-failure\nNoNewPrivileges=yes\nPrivateTmp=yes\nProtectSystem=strict\nReadWritePaths=/var/lib/minio\n\n[Install]\nWantedBy=multi-user.target\n".into(),
            mode: 0o644,
            owner: "root:root",
        },
    ])
}
fn render_opensearch(
    service: &ManagedService,
    _secrets: &BTreeMap<String, String>,
) -> Result<Vec<DriverConfigurationFile>> {
    Ok(driver_file(
        "/etc/opensearch/opensearch.yml",
        format!(
            "# Managed by Lumic\ncluster.name: {}\ndiscovery.type: single-node\nnetwork.host: \"{}\"\nhttp.port: {}\npath.data: /var/lib/opensearch\npath.logs: /var/log/opensearch\nplugins.security.disabled: true\n",
            setting(service, "cluster_name")?,
            service.configuration.bind_address,
            service.configuration.port
        ),
        0o640,
        "root:opensearch",
    ))
}
fn render_memcached(
    service: &ManagedService,
    _secrets: &BTreeMap<String, String>,
) -> Result<Vec<DriverConfigurationFile>> {
    Ok(driver_file(
        "/etc/memcached.conf",
        format!(
            "# Managed by Lumic\n-m {}\n-p {}\n-l {}\n-u memcache\n",
            setting(service, "memory_mb")?,
            service.configuration.port,
            service.configuration.bind_address
        ),
        0o644,
        "root:root",
    ))
}
fn render_mongodb(
    service: &ManagedService,
    _secrets: &BTreeMap<String, String>,
) -> Result<Vec<DriverConfigurationFile>> {
    Ok(driver_file(
        "/etc/mongod.conf",
        format!(
            "# Managed by Lumic\nstorage:\n  dbPath: /var/lib/mongodb\nsystemLog:\n  destination: file\n  path: /var/log/mongodb/mongod.log\n  logAppend: true\nnet:\n  bindIp: {}\n  port: {}\nprocessManagement:\n  timeZoneInfo: /usr/share/zoneinfo\n",
            service.configuration.bind_address, service.configuration.port
        ),
        0o640,
        "root:mongodb",
    ))
}
fn render_clickhouse(
    service: &ManagedService,
    _secrets: &BTreeMap<String, String>,
) -> Result<Vec<DriverConfigurationFile>> {
    Ok(driver_file(
        "/etc/clickhouse-server/config.d/lumic.xml",
        format!(
            "<!-- Managed by Lumic -->\n<clickhouse>\n  <listen_host>{}</listen_host>\n  <http_port>{}</http_port>\n</clickhouse>\n",
            service.configuration.bind_address, service.configuration.port
        ),
        0o640,
        "root:clickhouse",
    ))
}
fn render_prometheus(
    service: &ManagedService,
    _secrets: &BTreeMap<String, String>,
) -> Result<Vec<DriverConfigurationFile>> {
    Ok(vec![
        DriverConfigurationFile {
            path: "/etc/prometheus/prometheus.yml".into(),
            content: format!(
                "# Managed by Lumic\nglobal:\n  scrape_interval: {}\nscrape_configs:\n  - job_name: prometheus\n    static_configs:\n      - targets: ['{}']\n",
                setting(service, "scrape_interval")?,
                socket_address(
                    &service.configuration.bind_address,
                    service.configuration.port
                )
            ),
            mode: 0o640,
            owner: "root:prometheus",
        },
        DriverConfigurationFile {
            path: "/etc/systemd/system/prometheus.service.d/lumic.conf".into(),
            content: format!(
                "# Managed by Lumic\n[Service]\nExecStart=\nExecStart=/usr/bin/prometheus --config.file=/etc/prometheus/prometheus.yml --storage.tsdb.path=/var/lib/prometheus --web.listen-address={}\n",
                socket_address(
                    &service.configuration.bind_address,
                    service.configuration.port
                )
            ),
            mode: 0o644,
            owner: "root:root",
        },
    ])
}
fn render_grafana(
    service: &ManagedService,
    secrets: &BTreeMap<String, String>,
) -> Result<Vec<DriverConfigurationFile>> {
    Ok(driver_file(
        "/etc/grafana/grafana.ini",
        format!(
            "# Managed by Lumic\n[server]\nhttp_addr = {}\nhttp_port = {}\n[security]\nadmin_password = {}\n",
            service.configuration.bind_address,
            service.configuration.port,
            required_secret(secrets, "admin_password")?
        ),
        0o640,
        "root:grafana",
    ))
}
fn render_loki(
    service: &ManagedService,
    _secrets: &BTreeMap<String, String>,
) -> Result<Vec<DriverConfigurationFile>> {
    Ok(driver_file(
        "/etc/loki/config.yml",
        format!(
            "# Managed by Lumic\nauth_enabled: false\nserver:\n  http_listen_address: {}\n  http_listen_port: {}\ncommon:\n  path_prefix: /var/lib/loki\n  replication_factor: 1\n  ring:\n    kvstore:\n      store: inmemory\nschema_config:\n  configs:\n    - from: 2024-01-01\n      store: tsdb\n      object_store: filesystem\n      schema: v13\n      index:\n        prefix: index_\n        period: 24h\nlimits_config:\n  retention_period: {}\ncompactor:\n  working_directory: /var/lib/loki/compactor\n  retention_enabled: true\n",
            service.configuration.bind_address,
            service.configuration.port,
            setting(service, "retention_period")?
        ),
        0o640,
        "root:loki",
    ))
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

fn unsupported_operation(driver: &str, operation: &str) -> LumicError {
    invalid(
        "service",
        &format!("driver '{driver}' does not support {operation}"),
    )
}

fn driver_io(error: std::io::Error) -> LumicError {
    LumicError::Internal {
        message: format!("service driver I/O failed: {error}"),
    }
}

fn redis_directive_name(setting: &str) -> &str {
    match setting {
        "maxmemory_policy" => "maxmemory-policy",
        setting => setting,
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
        for id in [
            "mysql",
            "postgresql",
            "redis",
            "typesense",
            "meilisearch",
            "valkey",
            "rabbitmq",
            "minio",
            "opensearch",
            "memcached",
            "mongodb",
            "clickhouse",
            "prometheus",
            "grafana",
            "loki",
            "gitea",
            "gogs",
        ] {
            let definition = registry.definition(id).unwrap();
            assert_eq!(registry.driver(&definition.driver).unwrap().id(), id);
        }
        assert!(registry.driver("community-shell-plugin").is_err());
    }

    #[test]
    fn redis_policy_setting_uses_native_directive_name() {
        assert_eq!(redis_directive_name("maxmemory_policy"), "maxmemory-policy");
    }

    #[test]
    fn new_native_drivers_render_loopback_configuration() {
        let registry = ServiceDriverRegistry::built_in().unwrap();
        let secrets = BTreeMap::from([
            ("root_user".into(), "minio-admin".into()),
            ("root_password".into(), "minio-private".into()),
            ("admin_password".into(), "grafana-private".into()),
        ]);

        for kind in [
            ManagedServiceKind::Valkey,
            ManagedServiceKind::Rabbitmq,
            ManagedServiceKind::Minio,
            ManagedServiceKind::Opensearch,
            ManagedServiceKind::Memcached,
            ManagedServiceKind::Mongodb,
            ManagedServiceKind::Clickhouse,
            ManagedServiceKind::Prometheus,
            ManagedServiceKind::Grafana,
            ManagedServiceKind::Loki,
        ] {
            let driver = registry.legacy_driver(kind).unwrap();
            let service = service(kind);
            driver
                .validate_configuration(&service.configuration)
                .unwrap();
            let files = driver
                .configuration_files(&service, None, &secrets)
                .unwrap();
            assert!(!files.is_empty(), "{} rendered no files", kind.id());
            assert!(
                files.iter().any(|file| file.content.contains("127.0.0.1")),
                "{} does not render its loopback binding",
                kind.id()
            );
            assert!(!driver.health_probe(&service).executable.is_empty());
        }

        let rabbitmq = service(ManagedServiceKind::Rabbitmq);
        let rabbitmq_files = registry
            .driver("rabbitmq")
            .unwrap()
            .configuration_files(&rabbitmq, None, &secrets)
            .unwrap();
        assert!(!rabbitmq_files[0].content.contains("listeners.tcp.default"));

        assert_eq!(
            registry
                .driver("opensearch")
                .unwrap()
                .package_install_environment(),
            OPENSEARCH_INSTALL_ENVIRONMENT
        );

        for (id, setting_name, invalid_value) in [
            ("valkey", "maxmemory", "unlimited"),
            ("rabbitmq", "memory_high_watermark", "1.5"),
            ("minio", "console_port", "9000"),
            ("opensearch", "cluster_name", "unsafe: value"),
            ("memcached", "memory_mb", "4"),
            ("prometheus", "scrape_interval", "soon"),
            ("loki", "retention_period", "forever"),
        ] {
            let driver = registry.driver(id).unwrap();
            let mut configuration = driver.default_configuration();
            configuration
                .settings
                .insert(setting_name.into(), invalid_value.into());
            assert!(
                driver.validate_configuration(&configuration).is_err(),
                "{id} accepted an invalid {setting_name}"
            );
        }

        let prometheus = registry.driver("prometheus").unwrap();
        let mut ipv6_service = service(ManagedServiceKind::Prometheus);
        ipv6_service.configuration.bind_address = "::1".into();
        let ipv6_files = prometheus
            .configuration_files(&ipv6_service, None, &secrets)
            .unwrap();
        assert!(
            ipv6_files
                .iter()
                .any(|file| file.content.contains("[::1]:9090"))
        );

        for (id, path) in [
            ("minio", "/etc/systemd/system/minio.service"),
            (
                "prometheus",
                "/etc/systemd/system/prometheus.service.d/lumic.conf",
            ),
        ] {
            let kind = parse_test_kind(id);
            let files = registry
                .driver(id)
                .unwrap()
                .configuration_files(&service(kind), None, &secrets)
                .unwrap();
            assert!(files.iter().any(|file| file.path == Path::new(path)));
        }
    }

    fn parse_test_kind(id: &str) -> ManagedServiceKind {
        match id {
            "minio" => ManagedServiceKind::Minio,
            "prometheus" => ManagedServiceKind::Prometheus,
            _ => unreachable!("test only uses known service IDs"),
        }
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
    fn git_forge_drivers_share_the_lumic_repository_root() {
        let registry = ServiceDriverRegistry::built_in().unwrap();
        let secrets = BTreeMap::from([
            ("secret_key".into(), "forge-secret".into()),
            ("internal_token".into(), "forge-internal-token".into()),
        ]);
        for kind in [ManagedServiceKind::Gitea, ManagedServiceKind::Gogs] {
            let driver = registry.legacy_driver(kind).unwrap();
            assert!(driver.git_forge_spec().is_some());
            let service = service(kind);
            driver
                .validate_configuration(&service.configuration)
                .unwrap();
            let files = driver
                .configuration_files(&service, None, &secrets)
                .unwrap();
            assert!(
                files
                    .iter()
                    .any(|file| { file.content.contains("ROOT = /var/lib/lumic/repositories") })
            );
            let unit = files
                .iter()
                .find(|file| file.path.starts_with("/etc/systemd/system"))
                .unwrap();
            assert!(unit.content.contains("Group=lumic-git"));
            assert!(unit.content.contains("ReadWritePaths=/var/lib/"));

            let mut invalid = service.configuration;
            invalid
                .settings
                .insert("repository_root".into(), "relative".into());
            assert!(driver.validate_configuration(&invalid).is_err());
        }
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
