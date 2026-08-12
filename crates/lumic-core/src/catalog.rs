//! Trusted, declarative definitions for services, runtimes, and applications.

use crate::{LumicError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;
use std::path::Path;

/// Current version of the built-in catalog document contract.
pub const CATALOG_SCHEMA_VERSION: u32 = 1;

/// A validated configuration map whose keys are defined by a trusted catalog entry.
pub type Configuration = BTreeMap<String, Value>;

/// Operational category used to group catalog definitions without changing behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceCategory {
    Web,
    Database,
    Cache,
    Search,
    Runtime,
    Other,
}

/// Supported service instance cardinality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstancePolicy {
    Singleton,
    Named,
    Versioned,
}

/// Typed configuration field kinds shared by every interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigurationFieldType {
    String,
    Integer,
    Boolean,
    Enum,
    Bytes,
    Address,
    Port,
    Path,
    Secret,
    Version,
}

/// Host action required after a configuration value changes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplyBehavior {
    #[default]
    None,
    Reload,
    Restart,
    Recreate,
}

/// Reviewed transport used by a built-in definition to apply one typed value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConfigurationTarget {
    File { file: String, key: String },
    Command { command: Vec<String> },
}

/// File formats supported by the generic configuration writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigurationFileFormat {
    Ini,
    Directives,
}

/// A trusted file destination. Repository manifests cannot declare these paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigurationFileDefinition {
    pub id: String,
    pub path: String,
    pub format: ConfigurationFileFormat,
    #[serde(default)]
    pub section: Option<String>,
}

/// Fully rendered content for one reviewed destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedConfigurationFile {
    pub path: String,
    pub content: String,
}

/// A reviewed direct argv command with one typed value substituted as one argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedConfigurationCommand {
    pub field: String,
    pub command: Vec<String>,
}

/// One reusable field in a catalog-driven configuration form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigurationField {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub description: String,
    #[serde(rename = "type")]
    pub field_type: ConfigurationFieldType,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<Value>,
    #[serde(default)]
    pub values: Vec<Value>,
    #[serde(default)]
    pub minimum: Option<i64>,
    #[serde(default)]
    pub maximum: Option<i64>,
    #[serde(default)]
    pub advanced: bool,
    #[serde(default)]
    pub sensitive: bool,
    #[serde(default)]
    pub apply: ApplyBehavior,
    #[serde(default)]
    pub target: Option<ConfigurationTarget>,
}

impl ConfigurationField {
    fn validate_definition(&self) -> Result<()> {
        validate_catalog_key("configuration.key", &self.key)?;
        if self.label.trim().is_empty() {
            return Err(invalid("configuration.label", "must not be empty"));
        }
        if self.field_type == ConfigurationFieldType::Enum && self.values.is_empty() {
            return Err(invalid(
                "configuration.values",
                "enum fields require at least one allowed value",
            ));
        }
        if self.field_type != ConfigurationFieldType::Enum && !self.values.is_empty() {
            return Err(invalid(
                "configuration.values",
                "allowed values are supported only for enum fields",
            ));
        }
        if self
            .minimum
            .zip(self.maximum)
            .is_some_and(|(min, max)| min > max)
        {
            return Err(invalid("configuration.minimum", "must not exceed maximum"));
        }
        if self.sensitive && self.field_type != ConfigurationFieldType::Secret {
            return Err(invalid(
                "configuration.sensitive",
                "only secret fields may be marked sensitive",
            ));
        }
        if let Some(default) = &self.default {
            self.validate_value(default)?;
        }
        if let Some(target) = &self.target {
            match target {
                ConfigurationTarget::File { file, key } => {
                    validate_catalog_key("configuration.target.file", file)?;
                    let valid_key = !key.is_empty()
                        && key.len() <= 128
                        && key.bytes().all(|byte| {
                            byte.is_ascii_lowercase()
                                || byte.is_ascii_digit()
                                || matches!(byte, b'_' | b'-' | b'.')
                        });
                    if !valid_key {
                        return Err(invalid(
                            "configuration.target.key",
                            "must be a bounded directive key",
                        ));
                    }
                }
                ConfigurationTarget::Command { command } => {
                    if command.is_empty()
                        || command.iter().any(|part| {
                            part.is_empty() || part.bytes().any(|byte| byte.is_ascii_control())
                        })
                        || matches!(command.first().map(String::as_str), Some("sh" | "bash"))
                        || command.iter().any(|part| part == "-c")
                        || command.iter().filter(|part| *part == "{{ value }}").count() != 1
                    {
                        return Err(invalid(
                            "configuration.target.command",
                            "must be direct argv without a shell and contain one '{{ value }}' argument",
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_value(&self, value: &Value) -> Result<()> {
        let valid = match self.field_type {
            ConfigurationFieldType::String => bounded_string(value, false),
            ConfigurationFieldType::Integer => integer_in_range(value, self.minimum, self.maximum),
            ConfigurationFieldType::Boolean => value.is_boolean(),
            ConfigurationFieldType::Enum => self.values.contains(value),
            ConfigurationFieldType::Bytes => integer_in_range(value, Some(0), self.maximum),
            ConfigurationFieldType::Address => value
                .as_str()
                .is_some_and(|text| text.parse::<IpAddr>().is_ok()),
            ConfigurationFieldType::Port => value
                .as_u64()
                .is_some_and(|port| (1..=u16::MAX as u64).contains(&port)),
            ConfigurationFieldType::Path => value.as_str().is_some_and(|text| {
                text.len() <= 4096
                    && Path::new(text).is_absolute()
                    && !text.bytes().any(|byte| byte.is_ascii_control())
            }),
            ConfigurationFieldType::Secret => value.as_str().is_some_and(|text| {
                text.starts_with("secret://")
                    && text.len() <= 512
                    && !text.bytes().any(|byte| byte.is_ascii_control())
            }),
            ConfigurationFieldType::Version => value.as_str().is_some_and(valid_version),
        };
        if valid {
            Ok(())
        } else {
            Err(invalid(
                &format!("configuration.{}", self.key),
                "does not match the catalog field schema",
            ))
        }
    }
}

/// Shared validation/defaulting schema used by CLI, UI, MCP, and drivers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConfigurationSchema(pub Vec<ConfigurationField>);

impl ConfigurationSchema {
    /// Validates the schema itself, including duplicate fields and unsafe defaults.
    pub fn validate(&self) -> Result<()> {
        let mut keys = BTreeSet::new();
        for field in &self.0 {
            field.validate_definition()?;
            if !keys.insert(field.key.as_str()) {
                return Err(invalid(
                    "configuration.key",
                    &format!("duplicate field '{}'", field.key),
                ));
            }
        }
        Ok(())
    }

    /// Applies defaults and validates every submitted value, rejecting unknown keys.
    pub fn resolve(&self, supplied: &Configuration) -> Result<Configuration> {
        self.validate()?;
        let known: BTreeMap<_, _> = self
            .0
            .iter()
            .map(|field| (field.key.as_str(), field))
            .collect();
        if let Some(key) = supplied
            .keys()
            .find(|key| !known.contains_key(key.as_str()))
        {
            return Err(invalid("configuration", &format!("unknown field '{key}'")));
        }

        let mut resolved = Configuration::new();
        for field in &self.0 {
            if let Some(value) = supplied.get(&field.key).or(field.default.as_ref()) {
                field.validate_value(value)?;
                resolved.insert(field.key.clone(), value.clone());
            } else if field.required {
                return Err(invalid(
                    &format!("configuration.{}", field.key),
                    "is required",
                ));
            }
        }
        Ok(resolved)
    }

    /// Returns the strongest action required by changes between two validated maps.
    pub fn apply_behavior(&self, before: &Configuration, after: &Configuration) -> ApplyBehavior {
        self.0
            .iter()
            .filter(|field| before.get(&field.key) != after.get(&field.key))
            .map(|field| field.apply)
            .max()
            .unwrap_or_default()
    }

    /// Produces a configuration safe for normal status/audit output.
    pub fn redacted(&self, configuration: &Configuration) -> Configuration {
        let sensitive: BTreeSet<_> = self
            .0
            .iter()
            .filter(|field| field.sensitive)
            .map(|field| field.key.as_str())
            .collect();
        configuration
            .iter()
            .map(|(key, value)| {
                let value = if sensitive.contains(key.as_str()) {
                    Value::String("[redacted]".into())
                } else {
                    value.clone()
                };
                (key.clone(), value)
            })
            .collect()
    }
}

/// Capabilities declared by a service definition and enforced by its driver.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServiceCapabilities {
    pub configure: bool,
    pub start: bool,
    pub stop: bool,
    pub restart: bool,
    pub reload: bool,
    pub enable: bool,
    pub disable: bool,
    pub health: bool,
    pub logs: bool,
    pub upgrade: bool,
    pub backup: bool,
    pub restore: bool,
    pub remove: bool,
    pub resources: bool,
}

/// A typed output published by a service, runtime, or child resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputDefinition {
    pub key: String,
    #[serde(rename = "type")]
    pub output_type: String,
    pub capability: String,
    #[serde(default)]
    pub sensitive: bool,
}

/// Reviewed native package/unit mapping for one Linux distribution family.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformMapping {
    pub distribution: String,
    pub package: String,
    pub unit: String,
    #[serde(default)]
    pub data_path: Option<String>,
}

/// Trusted declarative metadata paired with an optional built-in Rust driver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceDefinition {
    pub schema_version: u32,
    pub definition_version: u32,
    pub id: String,
    pub name: String,
    pub category: ServiceCategory,
    pub description: String,
    pub driver: String,
    pub instance_policy: InstancePolicy,
    #[serde(default)]
    pub capabilities: ServiceCapabilities,
    #[serde(default)]
    pub configuration: ConfigurationSchema,
    #[serde(default)]
    pub config_files: Vec<ConfigurationFileDefinition>,
    #[serde(default)]
    pub resources: Vec<ServiceResourceDefinition>,
    #[serde(default)]
    pub outputs: Vec<OutputDefinition>,
    #[serde(default)]
    pub platforms: Vec<PlatformMapping>,
}

impl ServiceDefinition {
    /// Validates identifiers, schemas, capabilities, outputs, and platform mappings.
    pub fn validate(&self) -> Result<()> {
        validate_definition_header(
            self.schema_version,
            self.definition_version,
            &self.id,
            &self.name,
            &self.driver,
        )?;
        self.configuration.validate()?;
        let mut config_files = BTreeSet::new();
        for file in &self.config_files {
            validate_catalog_key("config_files.id", &file.id)?;
            let path = Path::new(&file.path);
            if !path.is_absolute()
                || file.path.bytes().any(|byte| byte.is_ascii_control())
                || !config_files.insert(file.id.as_str())
            {
                return Err(invalid(
                    "config_files",
                    "IDs must be unique and paths must be safe absolute paths",
                ));
            }
            if file.format != ConfigurationFileFormat::Ini && file.section.is_some() {
                return Err(invalid(
                    "config_files.section",
                    "sections are supported only for ini files",
                ));
            }
        }
        for field in &self.configuration.0 {
            if let Some(ConfigurationTarget::File { file, .. }) = &field.target
                && !config_files.contains(file.as_str())
            {
                return Err(invalid(
                    "configuration.target.file",
                    &format!("references unknown config file '{file}'"),
                ));
            }
        }
        let mut resource_kinds = BTreeSet::new();
        for resource in &self.resources {
            resource.validate()?;
            if !resource_kinds.insert(resource.kind.as_str()) {
                return Err(invalid(
                    "resources.kind",
                    &format!("duplicate resource kind '{}'", resource.kind),
                ));
            }
        }
        let mut outputs = BTreeSet::new();
        for output in &self.outputs {
            validate_catalog_key("outputs.key", &output.key)?;
            validate_capability("outputs.capability", &output.capability)?;
            if output.output_type.trim().is_empty() || !outputs.insert(output.key.as_str()) {
                return Err(invalid(
                    "outputs",
                    "output keys must be unique and output types must not be empty",
                ));
            }
        }
        let mut platforms = BTreeSet::new();
        for platform in &self.platforms {
            if !matches!(platform.distribution.as_str(), "debian" | "ubuntu")
                || platform.package.trim().is_empty()
                || platform.unit.trim().is_empty()
                || !platforms.insert(platform.distribution.as_str())
            {
                return Err(invalid(
                    "platforms",
                    "platform mappings must be unique, supported, and complete",
                ));
            }
        }
        if self.platforms.is_empty() {
            return Err(invalid("platforms", "at least one mapping is required"));
        }
        Ok(())
    }

    /// Renders known file formats from validated typed values.
    ///
    /// Paths and keys come only from the embedded trusted service definition.
    pub fn render_configuration_files(
        &self,
        supplied: &Configuration,
    ) -> Result<Vec<RenderedConfigurationFile>> {
        let configuration = self.configuration.resolve(supplied)?;
        let mut entries: BTreeMap<&str, Vec<(&str, String)>> = BTreeMap::new();
        for field in &self.configuration.0 {
            if let Some(ConfigurationTarget::File { file, key }) = &field.target
                && let Some(value) = configuration.get(&field.key)
            {
                entries
                    .entry(file)
                    .or_default()
                    .push((key, render_configuration_value(value)?));
            }
        }
        self.config_files
            .iter()
            .filter(|file| entries.contains_key(file.id.as_str()))
            .map(|file| {
                let mut content = String::from("# Managed by Lumic\n");
                if let Some(section) = &file.section {
                    content.push_str(&format!("[{section}]\n"));
                }
                for (key, value) in &entries[file.id.as_str()] {
                    match file.format {
                        ConfigurationFileFormat::Ini => {
                            content.push_str(&format!("{key} = {value}\n"));
                        }
                        ConfigurationFileFormat::Directives => {
                            content.push_str(&format!("{key} {value}\n"));
                        }
                    }
                }
                Ok(RenderedConfigurationFile {
                    path: file.path.clone(),
                    content,
                })
            })
            .collect()
    }

    /// Renders reviewed command targets without invoking a shell.
    pub fn render_configuration_commands(
        &self,
        supplied: &Configuration,
    ) -> Result<Vec<RenderedConfigurationCommand>> {
        let configuration = self.configuration.resolve(supplied)?;
        self.configuration
            .0
            .iter()
            .filter_map(
                |field| match (&field.target, configuration.get(&field.key)) {
                    (Some(ConfigurationTarget::Command { command }), Some(value)) => {
                        Some((field, command, value))
                    }
                    _ => None,
                },
            )
            .map(|(field, command, value)| {
                let value = render_configuration_value(value)?;
                Ok(RenderedConfigurationCommand {
                    field: field.key.clone(),
                    command: command
                        .iter()
                        .map(|part| {
                            if part == "{{ value }}" {
                                value.clone()
                            } else {
                                part.clone()
                            }
                        })
                        .collect(),
                })
            })
            .collect()
    }
}

/// Declarative schema for a child resource implemented by a trusted service driver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceResourceDefinition {
    #[serde(rename = "type")]
    pub kind: String,
    pub capability: String,
    #[serde(default)]
    pub configuration: ConfigurationSchema,
    #[serde(default)]
    pub outputs: Vec<OutputDefinition>,
}

impl ServiceResourceDefinition {
    fn validate(&self) -> Result<()> {
        validate_catalog_key("resources.type", &self.kind)?;
        validate_capability("resources.capability", &self.capability)?;
        self.configuration.validate()?;
        validate_outputs(&self.outputs)
    }
}

/// Declarative metadata for an explicitly versioned application runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeDefinition {
    pub schema_version: u32,
    pub definition_version: u32,
    pub id: String,
    pub name: String,
    pub description: String,
    pub driver: String,
    pub instance_policy: InstancePolicy,
    #[serde(default)]
    pub configuration: ConfigurationSchema,
    #[serde(default)]
    pub outputs: Vec<OutputDefinition>,
}

impl RuntimeDefinition {
    /// Validates runtime metadata and its shared schema.
    pub fn validate(&self) -> Result<()> {
        validate_definition_header(
            self.schema_version,
            self.definition_version,
            &self.id,
            &self.name,
            &self.driver,
        )?;
        if self.instance_policy != InstancePolicy::Versioned {
            return Err(invalid(
                "instance_policy",
                "runtime definitions must use versioned instances",
            ));
        }
        self.configuration.validate()?;
        validate_outputs(&self.outputs)
    }
}

fn validate_outputs(outputs: &[OutputDefinition]) -> Result<()> {
    let mut keys = BTreeSet::new();
    for output in outputs {
        validate_catalog_key("outputs.key", &output.key)?;
        validate_capability("outputs.capability", &output.capability)?;
        if output.output_type.trim().is_empty() || !keys.insert(output.key.as_str()) {
            return Err(invalid(
                "outputs",
                "output keys must be unique and output types must not be empty",
            ));
        }
    }
    Ok(())
}

/// A trusted application requirement referencing catalog capabilities, not shell steps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationRequirement {
    pub capability: String,
    pub role: String,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub configuration: Configuration,
    #[serde(default)]
    pub resources: BTreeMap<String, Configuration>,
}

/// Runtime components required by an application definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeRequirement {
    pub role: String,
    pub runtime: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub extensions: Vec<String>,
}

/// Declarative portion of a known application installation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationDefinition {
    pub schema_version: u32,
    pub definition_version: u32,
    pub id: String,
    pub name: String,
    pub description: String,
    pub driver: String,
    #[serde(default)]
    pub configuration: ConfigurationSchema,
    #[serde(default)]
    pub requirements: Vec<ApplicationRequirement>,
    #[serde(default)]
    pub runtime_requirements: Vec<RuntimeRequirement>,
}

impl ApplicationDefinition {
    /// Validates application metadata and capability requirements.
    pub fn validate(&self) -> Result<()> {
        validate_definition_header(
            self.schema_version,
            self.definition_version,
            &self.id,
            &self.name,
            &self.driver,
        )?;
        self.configuration.validate()?;
        let mut roles = BTreeSet::new();
        for requirement in &self.requirements {
            validate_capability("requirements.capability", &requirement.capability)?;
            validate_catalog_key("requirements.role", &requirement.role)?;
            if !roles.insert(requirement.role.as_str()) {
                return Err(invalid(
                    "requirements.role",
                    &format!("duplicate role '{}'", requirement.role),
                ));
            }
            for resource in requirement.resources.keys() {
                validate_catalog_key("requirements.resources", resource)?;
            }
        }
        for requirement in &self.runtime_requirements {
            validate_catalog_key("runtime_requirements.role", &requirement.role)?;
            validate_catalog_id("runtime_requirements.runtime", &requirement.runtime)?;
            if let Some(version) = &requirement.version
                && !valid_version(version)
            {
                return Err(invalid(
                    "runtime_requirements.version",
                    "must be a valid runtime version",
                ));
            }
            let mut extensions = BTreeSet::new();
            for extension in &requirement.extensions {
                validate_catalog_key("runtime_requirements.extensions", extension)?;
                if !extensions.insert(extension.as_str()) {
                    return Err(invalid(
                        "runtime_requirements.extensions",
                        &format!("duplicate extension '{extension}'"),
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Compile-time trusted catalog visible to every Lumic interface.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    services: BTreeMap<String, ServiceDefinition>,
    runtimes: BTreeMap<String, RuntimeDefinition>,
    applications: BTreeMap<String, ApplicationDefinition>,
}

impl Catalog {
    /// Parses and validates built-in TOML documents, rejecting duplicate IDs.
    pub fn from_documents(
        services: &[&str],
        runtimes: &[&str],
        applications: &[&str],
    ) -> Result<Self> {
        let mut catalog = Self::default();
        for document in services {
            let definition: ServiceDefinition = parse_toml("service", document)?;
            definition.validate()?;
            insert_unique(&mut catalog.services, definition.id.clone(), definition)?;
        }
        for document in runtimes {
            let definition: RuntimeDefinition = parse_toml("runtime", document)?;
            definition.validate()?;
            insert_unique(&mut catalog.runtimes, definition.id.clone(), definition)?;
        }
        for document in applications {
            let definition: ApplicationDefinition = parse_toml("application", document)?;
            definition.validate()?;
            insert_unique(&mut catalog.applications, definition.id.clone(), definition)?;
        }
        Ok(catalog)
    }

    /// Loads the reviewed catalog embedded in the Lumic binary.
    pub fn built_in() -> Result<Self> {
        Self::from_documents(BUILT_IN_SERVICES, BUILT_IN_RUNTIMES, BUILT_IN_APPLICATIONS)
    }

    pub fn services(&self) -> impl Iterator<Item = &ServiceDefinition> {
        self.services.values()
    }

    pub fn runtimes(&self) -> impl Iterator<Item = &RuntimeDefinition> {
        self.runtimes.values()
    }

    pub fn applications(&self) -> impl Iterator<Item = &ApplicationDefinition> {
        self.applications.values()
    }

    pub fn service(&self, id: &str) -> Option<&ServiceDefinition> {
        self.services.get(id)
    }

    pub fn runtime(&self, id: &str) -> Option<&RuntimeDefinition> {
        self.runtimes.get(id)
    }

    pub fn application(&self, id: &str) -> Option<&ApplicationDefinition> {
        self.applications.get(id)
    }

    /// Resolves a service capability to exactly one reviewed provider.
    pub fn service_for_capability(&self, capability: &str) -> Result<Option<&ServiceDefinition>> {
        validate_capability("requirements.capability", capability)?;
        let mut providers = self.services.values().filter(|service| {
            service
                .outputs
                .iter()
                .any(|output| output.capability == capability)
        });
        let provider = providers.next();
        if providers.next().is_some() {
            return Err(invalid(
                "requirements.capability",
                &format!("capability '{capability}' has multiple providers; select one explicitly"),
            ));
        }
        Ok(provider)
    }
}

const BUILT_IN_SERVICES: &[&str] = &[
    include_str!("../catalog/services/nginx.toml"),
    include_str!("../catalog/services/mysql.toml"),
    include_str!("../catalog/services/postgresql.toml"),
    include_str!("../catalog/services/redis.toml"),
    include_str!("../catalog/services/typesense.toml"),
    include_str!("../catalog/services/meilisearch.toml"),
    include_str!("../catalog/services/valkey.toml"),
    include_str!("../catalog/services/rabbitmq.toml"),
    include_str!("../catalog/services/minio.toml"),
    include_str!("../catalog/services/opensearch.toml"),
    include_str!("../catalog/services/memcached.toml"),
    include_str!("../catalog/services/mongodb.toml"),
    include_str!("../catalog/services/clickhouse.toml"),
    include_str!("../catalog/services/prometheus.toml"),
    include_str!("../catalog/services/grafana.toml"),
    include_str!("../catalog/services/loki.toml"),
    include_str!("../catalog/services/gitea.toml"),
    include_str!("../catalog/services/gogs.toml"),
];
const BUILT_IN_RUNTIMES: &[&str] = &[
    include_str!("../catalog/runtimes/php.toml"),
    include_str!("../catalog/runtimes/node.toml"),
];
const BUILT_IN_APPLICATIONS: &[&str] = &[
    include_str!("../catalog/applications/wordpress.toml"),
    include_str!("../catalog/applications/laravel.toml"),
    include_str!("../catalog/applications/laravel-typesense.toml"),
    include_str!("../catalog/applications/drupal.toml"),
    include_str!("../catalog/applications/symfony.toml"),
    include_str!("../catalog/applications/forgejo.toml"),
    include_str!("../catalog/applications/ghost.toml"),
    include_str!("../catalog/applications/matomo.toml"),
];

fn parse_toml<T>(kind: &str, document: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    toml::from_str(document).map_err(|error| invalid(kind, &format!("invalid TOML: {error}")))
}

fn insert_unique<T>(map: &mut BTreeMap<String, T>, id: String, value: T) -> Result<()> {
    if map.insert(id.clone(), value).is_some() {
        Err(invalid("catalog.id", &format!("duplicate ID '{id}'")))
    } else {
        Ok(())
    }
}

fn validate_definition_header(
    schema_version: u32,
    definition_version: u32,
    id: &str,
    name: &str,
    driver: &str,
) -> Result<()> {
    if schema_version != CATALOG_SCHEMA_VERSION {
        return Err(invalid(
            "schema_version",
            &format!("unsupported catalog schema version {schema_version}"),
        ));
    }
    if definition_version == 0 {
        return Err(invalid("definition_version", "must be at least one"));
    }
    validate_catalog_id("id", id)?;
    validate_catalog_id("driver", driver)?;
    if name.trim().is_empty() {
        return Err(invalid("name", "must not be empty"));
    }
    Ok(())
}

pub fn validate_catalog_id(field: &str, value: &str) -> Result<()> {
    let valid = !value.is_empty()
        && value.len() <= 63
        && value.as_bytes()[0].is_ascii_lowercase()
        && value.as_bytes()[value.len() - 1].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    valid.then_some(()).ok_or_else(|| {
        invalid(
            field,
            "must be a lowercase catalog ID containing letters, digits, or hyphens",
        )
    })
}

fn validate_catalog_key(field: &str, value: &str) -> Result<()> {
    let valid = !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
    valid
        .then_some(())
        .ok_or_else(|| invalid(field, &format!("'{value}' must be a lowercase data key")))
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

fn bounded_string(value: &Value, allow_empty: bool) -> bool {
    value.as_str().is_some_and(|text| {
        (allow_empty || !text.is_empty())
            && text.len() <= 4096
            && !text.bytes().any(|byte| byte.is_ascii_control())
    })
}

fn render_configuration_value(value: &Value) -> Result<String> {
    let rendered = match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        _ => {
            return Err(invalid(
                "configuration",
                "file targets support only scalar values",
            ));
        }
    };
    if rendered.is_empty()
        || !rendered.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/' | b':')
        })
    {
        return Err(invalid(
            "configuration",
            "rendered target values must be safe single-token scalars",
        ));
    }
    Ok(rendered)
}

fn integer_in_range(value: &Value, minimum: Option<i64>, maximum: Option<i64>) -> bool {
    value.as_i64().is_some_and(|number| {
        minimum.is_none_or(|minimum| number >= minimum)
            && maximum.is_none_or(|maximum| number <= maximum)
    })
}

fn valid_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value.as_bytes()[0].is_ascii_digit()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'+'))
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

    #[test]
    fn built_in_catalog_is_valid_and_contains_foundation_definitions() {
        let catalog = Catalog::built_in().unwrap();
        assert_eq!(catalog.services().count(), 18);
        for id in [
            "nginx",
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
            assert!(catalog.service(id).is_some(), "missing service {id}");
        }
        assert!(catalog.runtime("php").is_some());
        assert!(catalog.application("wordpress").is_some());
    }

    #[test]
    fn configuration_defaults_unknown_keys_and_secrets_are_safe() {
        let definition = Catalog::built_in()
            .unwrap()
            .service("redis")
            .unwrap()
            .clone();
        let resolved = definition
            .configuration
            .resolve(&Configuration::new())
            .unwrap();
        assert_eq!(resolved["port"], 6379);

        let unknown = BTreeMap::from([("danger".into(), Value::Bool(true))]);
        assert!(definition.configuration.resolve(&unknown).is_err());

        let typesense = Catalog::built_in()
            .unwrap()
            .service("typesense")
            .unwrap()
            .clone();
        let invalid_secret =
            BTreeMap::from([("api_key".into(), Value::String("plaintext-secret".into()))]);
        assert!(typesense.configuration.resolve(&invalid_secret).is_err());
    }

    #[test]
    fn duplicate_definition_ids_and_invalid_schema_are_rejected() {
        let redis = include_str!("../catalog/services/redis.toml");
        assert!(Catalog::from_documents(&[redis, redis], &[], &[]).is_err());

        let invalid = redis.replacen("schema_version = 1", "schema_version = 99", 1);
        assert!(Catalog::from_documents(&[&invalid], &[], &[]).is_err());
    }

    #[test]
    fn configuration_diff_selects_strongest_apply_behavior() {
        let definition = Catalog::built_in()
            .unwrap()
            .service("redis")
            .unwrap()
            .clone();
        let before = definition
            .configuration
            .resolve(&Configuration::new())
            .unwrap();
        let mut after = before.clone();
        after.insert("port".into(), Value::from(6380));
        after.insert("maxmemory".into(), Value::from(1_073_741_824_u64));
        assert_eq!(
            definition.configuration.apply_behavior(&before, &after),
            ApplyBehavior::Restart
        );
    }

    #[test]
    fn command_targets_render_values_as_single_argv_arguments() {
        let definition: ServiceDefinition = toml::from_str(
            r#"schema_version = 1
definition_version = 1
id = "example"
name = "Example"
category = "other"
description = "Example"
driver = "example"
instance_policy = "singleton"

[[configuration]]
key = "limit"
label = "Limit"
type = "integer"
required = true
[configuration.target]
type = "command"
command = ["examplectl", "set", "limit", "{{ value }}"]

[[platforms]]
distribution = "debian"
package = "example"
unit = "example.service"
"#,
        )
        .unwrap();
        definition.validate().unwrap();
        let commands = definition
            .render_configuration_commands(&BTreeMap::from([("limit".into(), Value::from(42))]))
            .unwrap();
        assert_eq!(commands[0].command, ["examplectl", "set", "limit", "42"]);

        let unsafe_definition = toml::to_string(&definition)
            .unwrap()
            .replace("examplectl", "bash")
            .replace(", \"{{ value }}\"", "");
        let unsafe_definition: ServiceDefinition = toml::from_str(&unsafe_definition).unwrap();
        assert!(unsafe_definition.validate().is_err());
    }
}
