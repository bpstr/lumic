//! Versioned repository-owned application and deployment intent.

use crate::{
    LumicError, Result,
    application::{
        ApplicationProcess, ApplicationProcessKind, ApplicationRuntime, ApplicationSchedule,
        DeploymentWorkflow, HealthCheck, NodeHandoff, NodePackageManager, ProcessHealthCheck,
        ProcessRestartPolicy, validate_branch, validate_command, validate_slug,
    },
    catalog::{Catalog, Configuration, ServiceCategory},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
};

pub const APPLICATION_MANIFEST_FILE: &str = "lumic.toml";
pub const LEGACY_APPLICATION_MANIFEST_FILE: &str = "lumic.yaml";
pub const APPLICATION_MANIFEST_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationManifest {
    pub schema_version: u32,
    pub name: String,
    #[serde(default)]
    pub source: ManifestSource,
    pub runtime: ManifestRuntime,
    #[serde(default)]
    pub build: Vec<Vec<String>>,
    pub output: Option<PathBuf>,
    pub public: Option<PathBuf>,
    pub web: Option<ManifestWeb>,
    #[serde(default)]
    pub workers: BTreeMap<String, ManifestWorker>,
    #[serde(default)]
    pub cron: BTreeMap<String, ManifestCron>,
    #[serde(default)]
    pub services: BTreeMap<String, ManifestService>,
    #[serde(default)]
    pub requirements: Vec<ManifestRequirement>,
    #[serde(default)]
    pub migrations: Vec<Vec<String>>,
    #[serde(default)]
    pub deployment: ManifestDeployment,
    #[serde(default)]
    pub shared: ManifestSharedPaths,
    pub health: Option<ManifestHealth>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ManifestSource {
    pub branch: Option<String>,
    pub subdirectory: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestRuntime {
    #[serde(rename = "static")]
    pub static_site: Option<bool>,
    pub node: Option<ManifestVersion>,
    pub php: Option<ManifestVersion>,
    #[serde(default)]
    pub extensions: Vec<String>,
    pub package_manager: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ManifestVersion {
    Number(u64),
    Text(String),
}

impl ManifestVersion {
    pub fn as_text(&self) -> String {
        match self {
            Self::Number(value) => value.to_string(),
            Self::Text(value) => value.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestWeb {
    pub command: Option<Vec<String>>,
    pub port: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestWorker {
    pub command: Vec<String>,
    #[serde(default = "default_instances")]
    pub instances: u16,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    pub working_directory: Option<PathBuf>,
    #[serde(default)]
    pub restart: ProcessRestartPolicy,
    pub health: Option<ProcessHealthCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestCron {
    pub command: Vec<String>,
    pub schedule: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ManifestService {
    Simple(String),
    Detailed(ManifestServiceDetail),
}

impl ManifestService {
    pub fn service_type(&self) -> &str {
        match self {
            Self::Simple(value) => value,
            Self::Detailed(value) => &value.service_type,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestServiceDetail {
    #[serde(rename = "type")]
    pub service_type: String,
    pub instance: Option<String>,
    pub database: Option<String>,
    pub user: Option<String>,
}

/// A capability request shared by built-in and repository-owned applications.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestRequirement {
    pub role: String,
    pub capability: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub instance: Option<String>,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub configuration: Configuration,
    #[serde(default)]
    pub resources: BTreeMap<String, Configuration>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ManifestDeployment {
    pub before: Vec<Vec<String>>,
    pub after: Vec<Vec<String>>,
    pub deploy_on_push: bool,
    pub retain_releases: usize,
    pub drain_seconds: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ManifestSharedPaths {
    pub directories: Vec<PathBuf>,
    pub files: Vec<PathBuf>,
}

impl Default for ManifestDeployment {
    fn default() -> Self {
        Self {
            before: Vec::new(),
            after: Vec::new(),
            deploy_on_push: true,
            retain_releases: 5,
            drain_seconds: 10,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestHealth {
    #[serde(default = "default_health_path")]
    pub path: String,
    pub port: Option<u16>,
    #[serde(default = "default_expected_status")]
    pub expect: u16,
    #[serde(default = "default_health_timeout")]
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplicationManifestV2 {
    schema_version: u32,
    application: ManifestApplicationV2,
    #[serde(default)]
    source: ManifestSource,
    runtime: ManifestRuntimeV2,
    #[serde(default)]
    requirements: Vec<ManifestRequirement>,
    #[serde(default)]
    processes: BTreeMap<String, ManifestProcessV2>,
    #[serde(default)]
    schedules: Vec<ManifestScheduleV2>,
    #[serde(default)]
    deployment: ManifestDeploymentV2,
    output: Option<PathBuf>,
    public: Option<PathBuf>,
    #[serde(default)]
    shared: ManifestSharedPaths,
    health: Option<ManifestHealth>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestApplicationV2 {
    name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestRuntimeV2 {
    #[serde(rename = "type")]
    runtime_type: String,
    #[serde(default)]
    version: Option<ManifestVersion>,
    #[serde(default, alias = "components")]
    extensions: Vec<String>,
    #[serde(default)]
    package_manager: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ManifestProcessTypeV2 {
    Web,
    Worker,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestProcessV2 {
    #[serde(rename = "type")]
    process_type: ManifestProcessTypeV2,
    command: Vec<String>,
    #[serde(default = "default_instances")]
    instances: u16,
    #[serde(default)]
    environment: BTreeMap<String, String>,
    working_directory: Option<PathBuf>,
    #[serde(default)]
    restart: ProcessRestartPolicy,
    health: Option<ProcessHealthCheck>,
    port: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestScheduleV2 {
    name: String,
    schedule: String,
    command: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ManifestDeploymentV2 {
    build: Option<Vec<String>>,
    migrate: Option<Vec<String>>,
    before: Vec<Vec<String>>,
    after: Vec<Vec<String>>,
    deploy_on_push: bool,
    retain_releases: usize,
    drain_seconds: u64,
}

impl Default for ManifestDeploymentV2 {
    fn default() -> Self {
        let deployment = ManifestDeployment::default();
        Self {
            build: None,
            migrate: None,
            before: deployment.before,
            after: deployment.after,
            deploy_on_push: deployment.deploy_on_push,
            retain_releases: deployment.retain_releases,
            drain_seconds: deployment.drain_seconds,
        }
    }
}

impl ApplicationManifestV2 {
    fn normalize(self) -> Result<ApplicationManifest> {
        if self.schema_version != APPLICATION_MANIFEST_SCHEMA_VERSION {
            return Err(invalid(
                "schema_version",
                "lumic.toml requires schema_version 2",
            ));
        }
        let runtime = match self.runtime.runtime_type.as_str() {
            "static" => ManifestRuntime {
                static_site: Some(true),
                node: None,
                php: None,
                extensions: self.runtime.extensions,
                package_manager: self.runtime.package_manager,
            },
            "node" => ManifestRuntime {
                static_site: None,
                node: Some(self.runtime.version.ok_or_else(|| {
                    invalid("runtime.version", "Node requires an explicit version")
                })?),
                php: None,
                extensions: self.runtime.extensions,
                package_manager: self.runtime.package_manager,
            },
            "php" => ManifestRuntime {
                static_site: None,
                node: None,
                php: Some(self.runtime.version.ok_or_else(|| {
                    invalid("runtime.version", "PHP requires an explicit version")
                })?),
                extensions: self.runtime.extensions,
                package_manager: self.runtime.package_manager,
            },
            _ => {
                return Err(invalid("runtime.type", "must be static, node, or php"));
            }
        };
        let mut web = None;
        let mut workers = BTreeMap::new();
        for (name, process) in self.processes {
            match process.process_type {
                ManifestProcessTypeV2::Web => {
                    if web.is_some() || process.instances != 1 {
                        return Err(invalid(
                            "processes",
                            "exactly one single-instance web process is supported",
                        ));
                    }
                    web = Some(ManifestWeb {
                        command: Some(process.command),
                        port: process.port,
                    });
                }
                ManifestProcessTypeV2::Worker => {
                    if process.port.is_some() {
                        return Err(invalid(
                            "processes.port",
                            "ports are valid only for web processes",
                        ));
                    }
                    workers.insert(
                        name,
                        ManifestWorker {
                            command: process.command,
                            instances: process.instances,
                            environment: process.environment,
                            working_directory: process.working_directory,
                            restart: process.restart,
                            health: process.health,
                        },
                    );
                }
            }
        }
        let mut cron = BTreeMap::new();
        for schedule in self.schedules {
            if cron
                .insert(
                    schedule.name.clone(),
                    ManifestCron {
                        command: schedule.command,
                        schedule: schedule.schedule,
                    },
                )
                .is_some()
            {
                return Err(invalid("schedules.name", "names must be unique"));
            }
        }
        Ok(ApplicationManifest {
            schema_version: self.schema_version,
            name: self.application.name,
            source: self.source,
            runtime,
            build: self.deployment.build.into_iter().collect(),
            output: self.output,
            public: self.public,
            web,
            workers,
            cron,
            services: BTreeMap::new(),
            requirements: self.requirements,
            migrations: self.deployment.migrate.into_iter().collect(),
            deployment: ManifestDeployment {
                before: self.deployment.before,
                after: self.deployment.after,
                deploy_on_push: self.deployment.deploy_on_push,
                retain_releases: self.deployment.retain_releases,
                drain_seconds: self.deployment.drain_seconds,
            },
            shared: self.shared,
            health: self.health,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedApplicationManifest {
    pub manifest: ApplicationManifest,
    pub runtime: ApplicationRuntime,
    pub runtime_version: Option<String>,
    pub runtime_components: Vec<String>,
    pub package_manager: Option<NodePackageManager>,
    pub branch: String,
    pub source_subdirectory: Option<PathBuf>,
    pub public_directory: Option<PathBuf>,
    pub workflow: DeploymentWorkflow,
    pub health: HealthCheck,
    pub processes: Vec<ApplicationProcess>,
    pub service_requirements: Vec<ManifestServiceRequirement>,
    pub shared_directories: Vec<PathBuf>,
    pub shared_files: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestServiceRequirement {
    pub role: String,
    #[serde(default)]
    pub capability: String,
    pub service_type: String,
    pub instance: Option<String>,
    pub database: Option<String>,
    pub user: Option<String>,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub configuration: Configuration,
    #[serde(default)]
    pub resources: BTreeMap<String, Configuration>,
}

impl ApplicationManifest {
    /// Parses the current repository-owned TOML contract.
    pub fn parse(source: &str) -> Result<Self> {
        let document: ApplicationManifestV2 = toml::from_str(source).map_err(|error| {
            invalid(
                APPLICATION_MANIFEST_FILE,
                &format!("could not parse schema: {error}"),
            )
        })?;
        let manifest = document.normalize()?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Parses the deprecated schema-v1 YAML contract for migration compatibility.
    pub fn parse_legacy_yaml(source: &str) -> Result<Self> {
        let manifest: Self = serde_yaml::from_str(source).map_err(|error| {
            invalid(
                LEGACY_APPLICATION_MANIFEST_FILE,
                &format!("could not parse schema: {error}"),
            )
        })?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<()> {
        if !matches!(self.schema_version, 1 | APPLICATION_MANIFEST_SCHEMA_VERSION) {
            return Err(invalid(
                "schema_version",
                "only lumic.toml schema_version 2 and legacy lumic.yaml schema_version 1 are supported",
            ));
        }
        validate_slug("name", &self.name)?;
        if let Some(branch) = &self.source.branch {
            validate_branch(branch)?;
        }
        validate_optional_relative("source.subdirectory", self.source.subdirectory.as_deref())?;
        validate_optional_relative("output", self.output.as_deref())?;
        validate_optional_relative("public", self.public.as_deref())?;
        if self.output.is_some() && self.public.is_some() {
            return Err(invalid(
                "output",
                "output and public are mutually exclusive",
            ));
        }
        self.runtime.validate()?;
        validate_commands("build", &self.build)?;
        validate_commands("migrations", &self.migrations)?;
        validate_commands("deployment.before", &self.deployment.before)?;
        validate_commands("deployment.after", &self.deployment.after)?;
        if !(1..=100).contains(&self.deployment.retain_releases) {
            return Err(invalid(
                "deployment.retain_releases",
                "must be between 1 and 100",
            ));
        }
        if self.deployment.drain_seconds > 300 {
            return Err(invalid(
                "deployment.drain_seconds",
                "must not exceed 300 seconds",
            ));
        }
        if self.build.len() > 1 {
            return Err(invalid("build", "only one argv build command is supported"));
        }
        if self.migrations.len() > 1 {
            return Err(invalid(
                "migrations",
                "only one argv migration command is supported",
            ));
        }
        for (name, worker) in &self.workers {
            validate_slug("workers.name", name)?;
            validate_command(&worker.command)?;
            if !(1..=64).contains(&worker.instances) {
                return Err(invalid("workers.instances", "must be between 1 and 64"));
            }
        }
        for (name, job) in &self.cron {
            validate_slug("cron.name", name)?;
            validate_command(&job.command)?;
            cron_to_calendar(&job.schedule)?;
        }
        for (role, service) in &self.services {
            validate_slug("services.role", role)?;
            validate_slug("services.type", service.service_type())?;
            if let ManifestService::Detailed(detail) = service
                && let Some(instance) = &detail.instance
            {
                validate_slug("services.instance", instance)?;
            }
        }
        let mut requirement_roles = BTreeSet::new();
        for requirement in &self.requirements {
            validate_slug("requirements.role", &requirement.role)?;
            validate_capability("requirements.capability", &requirement.capability)?;
            if let Some(provider) = &requirement.provider {
                validate_slug("requirements.provider", provider)?;
            }
            if let Some(instance) = &requirement.instance {
                validate_slug("requirements.instance", instance)?;
            }
            if !requirement_roles.insert(requirement.role.as_str())
                || self.services.contains_key(&requirement.role)
            {
                return Err(invalid("requirements.role", "roles must be unique"));
            }
            for resource in requirement.resources.keys() {
                validate_slug("requirements.resources", resource)?;
            }
        }
        if let Some(web) = &self.web {
            if let Some(command) = &web.command {
                validate_command(command)?;
            }
            if web.port == Some(0) {
                return Err(invalid("web.port", "must be non-zero"));
            }
        }
        if let Some(health) = &self.health {
            health.validate()?;
        }
        for path in &self.shared.directories {
            validate_optional_relative("shared.directories", Some(path))?;
        }
        for path in &self.shared.files {
            validate_optional_relative("shared.files", Some(path))?;
        }
        if self.shared.directories.iter().any(|directory| {
            self.shared.files.iter().any(|file| {
                file == directory || file.starts_with(directory) || directory.starts_with(file)
            })
        }) {
            return Err(invalid(
                "shared",
                "file and directory declarations must be distinct and non-overlapping",
            ));
        }
        Ok(())
    }

    pub fn resolve(&self, default_branch: &str) -> Result<ResolvedApplicationManifest> {
        self.validate()?;
        let (runtime, runtime_version, runtime_components) = self.runtime.resolve()?;
        let package_manager = self.runtime.package_manager()?;
        let branch = self
            .source
            .branch
            .as_deref()
            .unwrap_or(default_branch)
            .to_owned();
        validate_branch(&branch)?;
        let node_handoff = match (&self.web, runtime) {
            (Some(web), ApplicationRuntime::Node) => match (&web.command, web.port) {
                (Some(command), Some(port)) => {
                    let secondary_port = port.checked_add(1).ok_or_else(|| {
                        invalid(
                            "web.port",
                            "must leave room for the secondary blue/green port",
                        )
                    })?;
                    Some(NodeHandoff {
                        command: command.clone(),
                        primary_port: port,
                        secondary_port,
                        drain_seconds: self.deployment.drain_seconds,
                    })
                }
                (None, None) => None,
                _ => return Err(invalid("web", "Node web requires both command and port")),
            },
            (Some(web), _) if web.command.is_some() || web.port.is_some() => {
                return Err(invalid(
                    "web",
                    "command and port are only valid for Node runtime",
                ));
            }
            _ => None,
        };
        let workflow = DeploymentWorkflow {
            pre_deploy: self.deployment.before.clone(),
            build: self.build.first().cloned(),
            migrate: self.migrations.first().cloned(),
            post_deploy: self.deployment.after.clone(),
            node_handoff,
        };
        workflow.validate()?;
        let mut processes = Vec::new();
        for (name, worker) in &self.workers {
            for instance in 1..=worker.instances {
                let process_name = if worker.instances == 1 {
                    name.clone()
                } else {
                    format!("{name}-{instance}")
                };
                processes.push(ApplicationProcess {
                    name: process_name,
                    kind: ApplicationProcessKind::Worker,
                    command: worker.command.clone(),
                    schedule: None,
                    enabled: true,
                    environment: worker.environment.clone(),
                    working_directory: worker
                        .working_directory
                        .as_ref()
                        .map(|path| path.to_string_lossy().into_owned()),
                    restart_policy: worker.restart,
                    health_check: worker.health.clone(),
                });
            }
        }
        for (name, job) in &self.cron {
            processes.push(ApplicationProcess {
                name: name.clone(),
                kind: ApplicationProcessKind::Schedule,
                command: job.command.clone(),
                schedule: Some(ApplicationSchedule::calendar(cron_to_calendar(
                    &job.schedule,
                )?)),
                enabled: true,
                environment: BTreeMap::new(),
                working_directory: None,
                restart_policy: ProcessRestartPolicy::OnFailure,
                health_check: None,
            });
        }
        for process in &processes {
            process.validate()?;
        }
        let health = self
            .health
            .as_ref()
            .map_or_else(HealthCheck::default, |health| HealthCheck {
                enabled: true,
                path: health.path.clone(),
                port: health
                    .port
                    .or_else(|| self.web.as_ref().and_then(|web| web.port))
                    .unwrap_or(80),
                expected_status_min: health.expect,
                expected_status_max: health.expect,
                timeout_seconds: health.timeout_seconds,
            });
        let mut service_requirements: Vec<_> = self
            .services
            .iter()
            .map(|(role, service)| {
                let (instance, database, user) = match service {
                    ManifestService::Simple(_) => (None, None, None),
                    ManifestService::Detailed(detail) => (
                        detail.instance.clone(),
                        detail.database.clone(),
                        detail.user.clone(),
                    ),
                };
                ManifestServiceRequirement {
                    role: role.clone(),
                    capability: format!("service.{}", service.service_type()),
                    service_type: service.service_type().to_owned(),
                    instance,
                    database,
                    user,
                    optional: false,
                    configuration: Configuration::new(),
                    resources: BTreeMap::new(),
                }
            })
            .collect();
        let catalog = Catalog::built_in()?;
        for requirement in &self.requirements {
            let provider = if let Some(provider) = &requirement.provider {
                let definition = catalog.service(provider).ok_or_else(|| {
                    invalid(
                        "requirements.provider",
                        &format!("unknown service provider '{provider}'"),
                    )
                })?;
                if !definition
                    .outputs
                    .iter()
                    .any(|output| output.capability == requirement.capability)
                {
                    return Err(invalid(
                        "requirements.provider",
                        &format!(
                            "provider '{provider}' does not expose '{}'",
                            requirement.capability
                        ),
                    ));
                }
                Some(definition)
            } else {
                catalog.service_for_capability(&requirement.capability)?
            };
            let Some(provider) = provider else {
                if requirement.optional || requirement.capability.starts_with("runtime.") {
                    continue;
                }
                return Err(invalid(
                    "requirements.capability",
                    &format!("no trusted service provides '{}'", requirement.capability),
                ));
            };
            let configuration = provider
                .configuration
                .resolve(&requirement.configuration)
                .map_err(|error| requirement_error(&requirement.role, error))?;
            let mut resources = BTreeMap::new();
            for (kind, supplied) in &requirement.resources {
                let definition = provider
                    .resources
                    .iter()
                    .find(|resource| resource.kind == *kind)
                    .ok_or_else(|| {
                        invalid(
                            "requirements.resources",
                            &format!(
                                "provider '{}' does not expose resource type '{kind}'",
                                provider.id
                            ),
                        )
                    })?;
                resources.insert(
                    kind.clone(),
                    definition
                        .configuration
                        .resolve(supplied)
                        .map_err(|error| requirement_error(&requirement.role, error))?,
                );
            }
            if provider.category == ServiceCategory::Web {
                continue;
            }
            let database = resource_name(&resources, "database")?;
            let user = resource_name(&resources, "user")?;
            service_requirements.push(ManifestServiceRequirement {
                role: requirement.role.clone(),
                capability: requirement.capability.clone(),
                service_type: provider.id.clone(),
                instance: requirement.instance.clone(),
                database,
                user,
                optional: requirement.optional,
                configuration,
                resources,
            });
        }
        Ok(ResolvedApplicationManifest {
            manifest: self.clone(),
            runtime,
            runtime_version,
            runtime_components,
            package_manager,
            branch,
            source_subdirectory: self.source.subdirectory.clone(),
            public_directory: self.public.clone().or_else(|| self.output.clone()),
            workflow,
            health,
            processes,
            service_requirements,
            shared_directories: self.shared.directories.clone(),
            shared_files: self.shared.files.clone(),
        })
    }
}

impl ManifestRuntime {
    fn package_manager(&self) -> Result<Option<NodePackageManager>> {
        self.package_manager
            .as_deref()
            .map(|manager| match manager {
                "npm" => Ok(NodePackageManager::Npm),
                "pnpm" => Ok(NodePackageManager::Pnpm),
                "yarn" => Ok(NodePackageManager::Yarn),
                _ => Err(invalid(
                    "runtime.package_manager",
                    "must be npm, pnpm, or yarn for Node",
                )),
            })
            .transpose()
    }
    fn validate(&self) -> Result<()> {
        let selected = usize::from(self.static_site == Some(true))
            + usize::from(self.node.is_some())
            + usize::from(self.php.is_some());
        if selected != 1 {
            return Err(invalid(
                "runtime",
                "select exactly one of static_site, node, or php",
            ));
        }
        if self.static_site == Some(false) {
            return Err(invalid("runtime.static_site", "must be true when selected"));
        }
        if self.php.is_none() && !self.extensions.is_empty() {
            return Err(invalid(
                "runtime.extensions",
                "extensions are only valid for PHP",
            ));
        }
        for extension in &self.extensions {
            validate_slug("runtime.extensions", extension)?;
        }
        if let Some(manager) = &self.package_manager
            && (!matches!(manager.as_str(), "npm" | "pnpm" | "yarn") || self.node.is_none())
        {
            return Err(invalid(
                "runtime.package_manager",
                "must be npm, pnpm, or yarn for Node",
            ));
        }
        Ok(())
    }

    fn resolve(&self) -> Result<(ApplicationRuntime, Option<String>, Vec<String>)> {
        self.validate()?;
        if self.static_site == Some(true) {
            Ok((ApplicationRuntime::Static, None, Vec::new()))
        } else if let Some(version) = &self.node {
            Ok((
                ApplicationRuntime::Node,
                Some(version.as_text()),
                Vec::new(),
            ))
        } else if let Some(version) = &self.php {
            Ok((
                ApplicationRuntime::Php,
                Some(version.as_text()),
                self.extensions.clone(),
            ))
        } else {
            Err(invalid("runtime", "a supported runtime is required"))
        }
    }
}

impl ManifestHealth {
    fn validate(&self) -> Result<()> {
        if !self.path.starts_with('/')
            || self.path.contains(['\n', '\r', '\0'])
            || self.port == Some(0)
            || !(100..=599).contains(&self.expect)
            || self.timeout_seconds == 0
            || self.timeout_seconds > 300
        {
            return Err(invalid(
                "health",
                "requires a safe path, valid port/status, and 1-300 second timeout",
            ));
        }
        Ok(())
    }
}

fn validate_commands(field: &str, commands: &[Vec<String>]) -> Result<()> {
    for command in commands {
        validate_command(command)
            .map_err(|_| invalid(field, "must contain argv command arrays"))?;
    }
    Ok(())
}

fn validate_capability(field: &str, value: &str) -> Result<()> {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value.split('.').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        });
    valid
        .then_some(())
        .ok_or_else(|| invalid(field, "must be a dot-separated capability"))
}

fn resource_name(
    resources: &BTreeMap<String, Configuration>,
    kind: &str,
) -> Result<Option<String>> {
    resources
        .get(kind)
        .and_then(|configuration| configuration.get("name"))
        .map(|value| {
            let name = value.as_str().ok_or_else(|| {
                invalid(
                    &format!("requirements.resources.{kind}.name"),
                    "must be a string",
                )
            })?;
            validate_slug(&format!("requirements.resources.{kind}.name"), name)?;
            Ok(name.to_owned())
        })
        .transpose()
}

fn requirement_error(role: &str, error: LumicError) -> LumicError {
    match error {
        LumicError::InvalidInput { field, message } => {
            invalid(&format!("requirements.{role}.{field}"), &message)
        }
        error => error,
    }
}

fn validate_optional_relative(field: &str, path: Option<&Path>) -> Result<()> {
    if let Some(path) = path
        && (path.as_os_str().is_empty()
            || path.is_absolute()
            || path.components().any(|component| {
                matches!(
                    component,
                    Component::CurDir
                        | Component::ParentDir
                        | Component::RootDir
                        | Component::Prefix(_)
                )
            }))
    {
        return Err(invalid(field, "must be a normalized relative path"));
    }
    Ok(())
}

fn cron_to_calendar(expression: &str) -> Result<String> {
    let fields = expression.split_whitespace().collect::<Vec<_>>();
    if fields.len() != 5 {
        return Err(invalid(
            "cron.schedule",
            "must be a five-field cron expression",
        ));
    }
    let minute = cron_field(fields[0], 0, 59)?;
    let hour = cron_field(fields[1], 0, 23)?;
    let day = cron_field(fields[2], 1, 31)?;
    let month = cron_field(fields[3], 1, 12)?;
    let weekday = cron_field(fields[4], 0, 7)?;
    if day != "*" && weekday != "*" {
        return Err(invalid(
            "cron.schedule",
            "schema version 1 cannot combine day-of-month and day-of-week constraints",
        ));
    }
    let date = format!("*-{month}-{day} {hour}:{minute}:00");
    if weekday == "*" {
        Ok(date)
    } else {
        let name = match weekday.as_str() {
            "0" | "7" => "Sun",
            "1" => "Mon",
            "2" => "Tue",
            "3" => "Wed",
            "4" => "Thu",
            "5" => "Fri",
            "6" => "Sat",
            _ => unreachable!("validated weekday"),
        };
        Ok(format!("{name} {date}"))
    }
}

fn cron_field(value: &str, minimum: u8, maximum: u8) -> Result<String> {
    if value == "*" {
        return Ok(value.into());
    }
    let number = value.parse::<u8>().map_err(|_| {
        invalid(
            "cron.schedule",
            "schema version 1 supports only wildcard or single numeric cron fields",
        )
    })?;
    if !(minimum..=maximum).contains(&number) {
        return Err(invalid("cron.schedule", "contains an out-of-range field"));
    }
    Ok(number.to_string())
}

const fn default_instances() -> u16 {
    1
}
fn default_health_path() -> String {
    "/".into()
}
const fn default_expected_status() -> u16 {
    200
}
const fn default_health_timeout() -> u64 {
    10
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

    const VALID: &str = r#"
schema_version: 1
name: billing-api
source:
  branch: main
runtime:
  node: 24
  package_manager: pnpm
build:
  - ["pnpm", "run", "build"]
output: dist
web:
  command: ["node", "dist/server.js"]
  port: 3100
workers:
  queue:
    command: ["node", "dist/worker.js"]
    instances: 2
    environment:
      QUEUE: default
    working_directory: worker
    restart: always
    health:
      command: ["node", "dist/worker-health.js"]
cron:
  cleanup:
    command: ["node", "dist/cleanup.js"]
    schedule: "0 2 * * *"
services:
  database:
    type: postgresql
    database: app
    user: app
  cache: redis
migrations:
  - ["pnpm", "prisma", "migrate", "deploy"]
deployment:
  after:
    - ["node", "dist/warm-cache.js"]
  deploy_on_push: true
health:
  path: /health
  expect: 200
"#;

    #[test]
    fn parses_and_resolves_the_repository_contract() {
        let manifest = ApplicationManifest::parse_legacy_yaml(VALID).unwrap();
        let resolved = manifest.resolve("trunk").unwrap();
        assert_eq!(resolved.runtime, ApplicationRuntime::Node);
        assert_eq!(resolved.branch, "main");
        assert_eq!(resolved.processes.len(), 3);
        assert_eq!(resolved.package_manager, Some(NodePackageManager::Pnpm));
        assert_eq!(
            resolved.processes[0].restart_policy,
            ProcessRestartPolicy::Always
        );
        assert_eq!(resolved.processes[0].environment["QUEUE"], "default");
        assert!(resolved.processes[0].health_check.is_some());
        assert_eq!(resolved.service_requirements.len(), 2);
        assert_eq!(resolved.public_directory, Some(PathBuf::from("dist")));
        assert!(resolved.workflow.node_handoff.is_some());
    }

    #[test]
    fn rejects_unknown_fields_shell_strings_and_plaintext_shortcuts() {
        assert!(
            ApplicationManifest::parse_legacy_yaml(&VALID.replace("output: dist", "unknown: true"))
                .is_err()
        );
        assert!(
            ApplicationManifest::parse_legacy_yaml(
                &VALID.replace("[\"pnpm\", \"run\", \"build\"]", "pnpm run build",)
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_traversal_and_ambiguous_runtime() {
        assert!(
            ApplicationManifest::parse_legacy_yaml(
                &VALID.replace("output: dist", "output: ../dist")
            )
            .is_err()
        );
        assert!(
            ApplicationManifest::parse_legacy_yaml(
                &VALID.replace("  node: 24", "  node: 24\n  php: \"8.4\"")
            )
            .is_err()
        );
    }

    #[test]
    fn resolves_shared_release_paths_and_rejects_overlaps() {
        let source = VALID.replace(
            "health:\n  path: /health",
            "shared:\n  directories: [storage]\n  files: [.env]\nhealth:\n  path: /health",
        );
        let resolved = ApplicationManifest::parse_legacy_yaml(&source)
            .unwrap()
            .resolve("main")
            .unwrap();
        assert_eq!(resolved.shared_directories, vec![PathBuf::from("storage")]);
        assert_eq!(resolved.shared_files, vec![PathBuf::from(".env")]);

        let overlap = source.replace("files: [.env]", "files: [storage/cache]");
        assert!(ApplicationManifest::parse_legacy_yaml(&overlap).is_err());
    }

    #[test]
    fn parses_toml_capabilities_processes_and_resources() {
        let manifest = ApplicationManifest::parse(
            r#"schema_version = 2

[application]
name = "billing-api"

[runtime]
type = "php"
version = "8.3"
extensions = ["mysqli", "dom"]

[[requirements]]
role = "database"
capability = "database.mysql_compatible"

[requirements.resources.database]
name = "billing"

[requirements.resources.user]
name = "billing"

[processes.queue]
type = "worker"
command = ["php", "artisan", "queue:work"]

[[schedules]]
name = "scheduler"
schedule = "* * * * *"
command = ["php", "artisan", "schedule:run"]

[deployment]
build = ["composer", "install", "--no-dev"]
migrate = ["php", "artisan", "migrate", "--force"]

[health]
path = "/health"
"#,
        )
        .unwrap();
        let resolved = manifest.resolve("main").unwrap();
        assert_eq!(resolved.runtime, ApplicationRuntime::Php);
        assert_eq!(resolved.runtime_components, ["mysqli", "dom"]);
        assert_eq!(resolved.processes.len(), 2);
        assert_eq!(resolved.service_requirements[0].service_type, "mysql");
        assert_eq!(
            resolved.service_requirements[0].database.as_deref(),
            Some("billing")
        );
    }
}
