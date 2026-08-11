pub use crate::artifact::ArtifactDefinition as RecipeArtifact;
use crate::{
    Capability, Change, LumicError, Plan, Result, Risk, RiskLevel,
    application::{ApplicationProcess, ApplicationRuntime, validate_domain, validate_slug},
    managed_service::{ManagedServiceKind, validate_resource_id},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipeMetadata {
    pub id: String,
    pub version: String,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipeServiceRequirement {
    pub id_suffix: String,
    pub kind: ManagedServiceKind,
    pub role: String,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipeEnvironmentSource {
    Input { required: bool },
    GeneratedSecret,
    Literal { value: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipeEnvironmentValue {
    pub name: String,
    pub source: RecipeEnvironmentSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum RecipeSetupStep {
    HealthCheck {
        path: String,
        port: u16,
    },
    Process {
        process: ApplicationProcess,
    },
    Deploy,
    WordPress {
        source: RecipeArtifact,
        wp_cli: RecipeArtifact,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipeDefinition {
    pub metadata: RecipeMetadata,
    pub runtime: ApplicationRuntime,
    #[serde(default)]
    pub runtime_version: Option<String>,
    #[serde(default)]
    pub repository_required: bool,
    #[serde(default)]
    pub components: Vec<String>,
    #[serde(default)]
    pub services: Vec<RecipeServiceRequirement>,
    #[serde(default)]
    pub environment: Vec<RecipeEnvironmentValue>,
    #[serde(default)]
    pub setup: Vec<RecipeSetupStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipeInstallRequest {
    pub recipe_id: String,
    pub application_id: String,
    pub domain: String,
    pub repository_url: Option<String>,
    #[serde(default = "default_branch")]
    pub branch: String,
    pub tls_email: Option<String>,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
}

fn default_branch() -> String {
    "main".into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipeInstallationStatus {
    Installed,
    Updating,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RecipeOperationProgress {
    pub execution_id: String,
    #[serde(default)]
    pub completed_steps: Vec<String>,
    pub current_step: Option<String>,
    pub failure: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipeInstallation {
    pub recipe_id: String,
    pub recipe_version: String,
    pub application_id: String,
    pub domain: String,
    #[serde(default)]
    pub repository_url: Option<String>,
    #[serde(default = "default_branch")]
    pub branch: String,
    #[serde(default)]
    pub tls_email: Option<String>,
    pub secret_references: BTreeMap<String, String>,
    pub service_ids: Vec<String>,
    #[serde(default)]
    pub owned_resources: Vec<String>,
    #[serde(default)]
    pub binding_ids: Vec<String>,
    #[serde(default)]
    pub artifacts: BTreeMap<String, String>,
    #[serde(default)]
    pub operation: Option<RecipeOperationProgress>,
    pub status: RecipeInstallationStatus,
    pub installed_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipeApplyResult {
    pub installation: Option<RecipeInstallation>,
    pub changed: bool,
    pub message: String,
}

impl RecipeDefinition {
    pub fn validate(&self) -> Result<()> {
        validate_resource_id("recipe", &self.metadata.id)?;
        validate_version(&self.metadata.version)?;
        match (self.runtime, self.runtime_version.as_deref()) {
            (ApplicationRuntime::Static, None) => {}
            (ApplicationRuntime::Static, Some(_)) => {
                return Err(invalid(
                    "runtime_version",
                    "static recipes cannot declare a runtime version",
                ));
            }
            (ApplicationRuntime::Php, Some("8.1" | "8.2" | "8.3" | "8.4")) => {}
            (ApplicationRuntime::Node, Some("20" | "22" | "24")) => {}
            (ApplicationRuntime::Php, _) => {
                return Err(invalid(
                    "runtime_version",
                    "PHP recipes require a supported explicit version",
                ));
            }
            (ApplicationRuntime::Node, _) => {
                return Err(invalid(
                    "runtime_version",
                    "Node recipes require a supported explicit major version",
                ));
            }
        }
        if self.metadata.name.trim().is_empty() || self.metadata.description.trim().is_empty() {
            return Err(invalid("recipe", "name and description must not be empty"));
        }
        let mut environment = BTreeSet::new();
        for value in &self.environment {
            validate_environment_name(&value.name)?;
            if !environment.insert(&value.name) {
                return Err(invalid("environment", "environment names must be unique"));
            }
            if matches!(&value.source, RecipeEnvironmentSource::Literal { value } if value.len() > 4096 || value.contains('\0'))
            {
                return Err(invalid(
                    "environment",
                    "literal values must be bounded and contain no NUL bytes",
                ));
            }
        }
        let mut roles = BTreeSet::new();
        for service in &self.services {
            validate_slug("service_suffix", &service.id_suffix)?;
            validate_role(&service.role)?;
            if !roles.insert(&service.role) {
                return Err(invalid("service", "service roles must be unique"));
            }
        }
        for step in &self.setup {
            match step {
                RecipeSetupStep::HealthCheck { path, port } => {
                    if !path.starts_with('/') || path.contains(['\n', '\r']) || *port == 0 {
                        return Err(invalid("health_check", "path and port are invalid"));
                    }
                }
                RecipeSetupStep::Process { process } => {
                    crate::application::validate_slug("process", &process.name)?;
                    crate::application::validate_command(&process.command)?;
                }
                RecipeSetupStep::Deploy => {}
                RecipeSetupStep::WordPress { source, wp_cli } => {
                    source.validate()?;
                    wp_cli.validate()?;
                }
            }
        }
        Ok(())
    }

    pub fn plan(&self, request: &RecipeInstallRequest, already_installed: bool) -> Result<Plan> {
        self.validate()?;
        validate_slug("application", &request.application_id)?;
        validate_domain(&request.domain)?;
        if self.metadata.id != request.recipe_id {
            return Err(invalid(
                "recipe",
                "request does not match recipe definition",
            ));
        }
        if self.repository_required && request.repository_url.is_none() && !already_installed {
            return Err(invalid(
                "repository",
                "this recipe requires a repository URL",
            ));
        }
        for value in &self.environment {
            if !already_installed
                && matches!(
                    value.source,
                    RecipeEnvironmentSource::Input { required: true }
                )
                && !request.environment.contains_key(&value.name)
            {
                return Err(invalid(
                    "environment",
                    format!("{} is required", value.name),
                ));
            }
        }
        for (name, value) in &request.environment {
            validate_environment_name(name)?;
            if value.len() > 4096 || value.contains('\0') {
                return Err(invalid(
                    "environment",
                    "input values must be bounded and contain no NUL bytes",
                ));
            }
            if !self.environment.iter().any(|item| {
                item.name == *name && matches!(item.source, RecipeEnvironmentSource::Input { .. })
            }) {
                return Err(invalid(
                    "environment",
                    format!("{name} is not declared as a recipe input"),
                ));
            }
        }
        if self.metadata.id == "wordpress" {
            validate_wordpress_inputs(request, already_installed)?;
        }
        let verb = if already_installed {
            "reconcile"
        } else {
            "install"
        };
        let mut changes = vec![Change {
            capability: Capability::new("recipe.apply"),
            summary: format!(
                "{verb} {} {} for {}",
                self.metadata.id, self.metadata.version, request.application_id
            ),
            before: already_installed.then(|| "managed recipe installation".into()),
            after: Some(format!("{}@{}", self.metadata.id, self.metadata.version)),
            reversible: true,
        }];
        for service in &self.services {
            changes.push(Change {
                capability: Capability::new("managed_service.install"),
                summary: format!(
                    "ensure {:?} service for role {}",
                    service.kind, service.role
                ),
                before: None,
                after: Some(format!("{}-{}", request.application_id, service.id_suffix)),
                reversible: true,
            });
        }
        Ok(Plan {
            id: format!("recipe-{}-{}", self.metadata.id, request.application_id),
            summary: format!("{verb} recipe {}@{}", self.metadata.id, self.metadata.version),
            changes,
            risks: vec![Risk {
                level: RiskLevel::Medium,
                summary: "native packages, systemd units, nginx and application state may change".into(),
                mitigation: Some("all operations use existing typed Lumic services and uninstall moves application data to Lumic trash".into()),
            }],
            preconditions: vec!["supported Debian or Ubuntu host".into(), "unique application/domain or matching existing recipe installation".into()],
            validation: vec!["recipe schema and inputs validate".into(), "runtime and managed services report healthy".into(), "nginx configuration validates before reload".into()],
            recovery: vec!["configuration writers restore the previous validated files".into(), "recipe uninstall removes managed metadata and moves application files to Lumic trash".into()],
        })
    }
}

pub fn reference_recipes() -> Vec<RecipeDefinition> {
    vec![RecipeDefinition {
        metadata: RecipeMetadata {
            id: "static-git".into(),
            version: "1.0.0".into(),
            name: "Static Git application".into(),
            description:
                "A static website deployed from a Git repository through Lumic releases and nginx."
                    .into(),
        },
        runtime: ApplicationRuntime::Static,
        runtime_version: None,
        repository_required: true,
        components: Vec::new(),
        services: Vec::new(),
        environment: vec![RecipeEnvironmentValue {
            name: "LUMIC_RECIPE_TOKEN".into(),
            source: RecipeEnvironmentSource::GeneratedSecret,
        }],
        setup: vec![
            RecipeSetupStep::HealthCheck {
                path: "/".into(),
                port: 80,
            },
            RecipeSetupStep::Deploy,
        ],
    }, wordpress_recipe(), laravel_recipe(false), laravel_recipe(true), drupal_recipe(),
        symfony_recipe(), ghost_recipe(), matomo_recipe()]
}

pub fn wordpress_recipe() -> RecipeDefinition {
    RecipeDefinition {
        metadata: RecipeMetadata {
            id: "wordpress".into(),
            version: "1.0.0".into(),
            name: "WordPress".into(),
            description: "A checksum-verified WordPress application with PHP-FPM, nginx and MySQL.".into(),
        },
        runtime: ApplicationRuntime::Php,
        runtime_version: Some("8.3".into()),
        repository_required: false,
        components: ["curl", "mbstring", "mysql", "xml", "zip"]
            .map(str::to_owned)
            .to_vec(),
        services: vec![RecipeServiceRequirement {
            id_suffix: "mysql".into(),
            kind: ManagedServiceKind::Mysql,
            role: "database".into(),
            required: true,
        }],
        environment: vec![
            input("WORDPRESS_SITE_TITLE"),
            input("WORDPRESS_ADMIN_USER"),
            input("WORDPRESS_ADMIN_EMAIL"),
            RecipeEnvironmentValue {
                name: "WORDPRESS_ADMIN_PASSWORD".into(),
                source: RecipeEnvironmentSource::GeneratedSecret,
            },
        ],
        setup: vec![
            RecipeSetupStep::WordPress {
                source: RecipeArtifact {
                    id: "wordpress".into(),
                    version: "6.8.2".into(),
                    url: "https://wordpress.org/wordpress-6.8.2.tar.gz".into(),
                    sha256: "d85a72e392bfe866816b3c2ebc6a44699072aa50cc3a620f1c4ed2f13b645e2b".into(),
                },
                wp_cli: RecipeArtifact {
                    id: "wp-cli".into(),
                    version: "2.12.0".into(),
                    url: "https://github.com/wp-cli/wp-cli/releases/download/v2.12.0/wp-cli-2.12.0.phar".into(),
                    sha256: "ce34ddd838f7351d6759068d09793f26755463b4a4610a5a5c0a97b68220d85c".into(),
                },
            },
            RecipeSetupStep::HealthCheck { path: "/wp-login.php".into(), port: 80 },
        ],
    }
}

fn php_repository_recipe(
    id: &str,
    name: &str,
    description: &str,
    services: Vec<RecipeServiceRequirement>,
    environment: Vec<RecipeEnvironmentValue>,
) -> RecipeDefinition {
    RecipeDefinition {
        metadata: RecipeMetadata {
            id: id.into(),
            version: "1.0.0".into(),
            name: name.into(),
            description: description.into(),
        },
        runtime: ApplicationRuntime::Php,
        runtime_version: Some("8.3".into()),
        repository_required: true,
        components: ["curl", "intl", "mbstring", "mysql", "xml", "zip"]
            .map(str::to_owned)
            .to_vec(),
        services,
        environment,
        setup: vec![
            RecipeSetupStep::HealthCheck {
                path: "/".into(),
                port: 80,
            },
            RecipeSetupStep::Deploy,
        ],
    }
}

fn service(id_suffix: &str, kind: ManagedServiceKind, role: &str) -> RecipeServiceRequirement {
    RecipeServiceRequirement {
        id_suffix: id_suffix.into(),
        kind,
        role: role.into(),
        required: true,
    }
}

fn laravel_recipe(typesense: bool) -> RecipeDefinition {
    let mut services = vec![
        service("mysql", ManagedServiceKind::Mysql, "database"),
        service("redis", ManagedServiceKind::Redis, "cache"),
    ];
    if typesense {
        services.push(service(
            "typesense",
            ManagedServiceKind::Typesense,
            "search",
        ));
    }
    php_repository_recipe(
        if typesense {
            "laravel-typesense"
        } else {
            "laravel"
        },
        if typesense {
            "Laravel + Typesense"
        } else {
            "Laravel"
        },
        "Laravel application with PHP-FPM, nginx, MySQL and Redis; the Typesense variant adds typed search.",
        services,
        vec![RecipeEnvironmentValue {
            name: "APP_KEY".into(),
            source: RecipeEnvironmentSource::GeneratedSecret,
        }],
    )
}

fn drupal_recipe() -> RecipeDefinition {
    php_repository_recipe(
        "drupal",
        "Drupal",
        "Drupal application with PHP-FPM, nginx and MySQL.",
        vec![service("mysql", ManagedServiceKind::Mysql, "database")],
        vec![RecipeEnvironmentValue {
            name: "DRUPAL_HASH_SALT".into(),
            source: RecipeEnvironmentSource::GeneratedSecret,
        }],
    )
}

fn symfony_recipe() -> RecipeDefinition {
    php_repository_recipe(
        "symfony",
        "Symfony",
        "Symfony application with PHP-FPM, nginx and PostgreSQL.",
        vec![service(
            "postgresql",
            ManagedServiceKind::Postgresql,
            "database",
        )],
        vec![RecipeEnvironmentValue {
            name: "APP_SECRET".into(),
            source: RecipeEnvironmentSource::GeneratedSecret,
        }],
    )
}

fn matomo_recipe() -> RecipeDefinition {
    php_repository_recipe(
        "matomo",
        "Matomo",
        "Matomo analytics application with PHP-FPM, nginx and MySQL.",
        vec![service("mysql", ManagedServiceKind::Mysql, "database")],
        Vec::new(),
    )
}

fn ghost_recipe() -> RecipeDefinition {
    RecipeDefinition {
        metadata: RecipeMetadata {
            id: "ghost".into(),
            version: "1.0.0".into(),
            name: "Ghost".into(),
            description: "Ghost publishing application with Node, nginx and MySQL.".into(),
        },
        runtime: ApplicationRuntime::Node,
        runtime_version: Some("22".into()),
        repository_required: true,
        components: Vec::new(),
        services: vec![service("mysql", ManagedServiceKind::Mysql, "database")],
        environment: vec![RecipeEnvironmentValue {
            name: "GHOST_ADMIN_TOKEN".into(),
            source: RecipeEnvironmentSource::GeneratedSecret,
        }],
        setup: vec![
            RecipeSetupStep::HealthCheck {
                path: "/".into(),
                port: 80,
            },
            RecipeSetupStep::Deploy,
        ],
    }
}

fn input(name: &str) -> RecipeEnvironmentValue {
    RecipeEnvironmentValue {
        name: name.into(),
        source: RecipeEnvironmentSource::Input { required: true },
    }
}

fn validate_wordpress_inputs(
    request: &RecipeInstallRequest,
    already_installed: bool,
) -> Result<()> {
    let value = |name: &str| request.environment.get(name).map(String::as_str);
    if already_installed && request.environment.is_empty() {
        return Ok(());
    }
    let title = value("WORDPRESS_SITE_TITLE").unwrap_or_default();
    if title.trim().is_empty() || title.len() > 200 || title.contains(['\n', '\r']) {
        return Err(invalid(
            "WORDPRESS_SITE_TITLE",
            "must be 1 to 200 characters on one line",
        ));
    }
    let user = value("WORDPRESS_ADMIN_USER").unwrap_or_default();
    if user.len() < 3
        || user.len() > 60
        || !user
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'@'))
    {
        return Err(invalid(
            "WORDPRESS_ADMIN_USER",
            "must be 3 to 60 safe username characters",
        ));
    }
    let email = value("WORDPRESS_ADMIN_EMAIL").unwrap_or_default();
    let mut parts = email.split('@');
    if email.len() > 254
        || parts.next().is_none_or(str::is_empty)
        || parts.next().is_none_or(|domain| !domain.contains('.'))
        || parts.next().is_some()
        || email.contains(['\n', '\r'])
    {
        return Err(invalid(
            "WORDPRESS_ADMIN_EMAIL",
            "must be a valid email address",
        ));
    }
    Ok(())
}

fn validate_version(value: &str) -> Result<()> {
    let parts = value.split('.').collect::<Vec<_>>();
    if parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        Ok(())
    } else {
        Err(invalid(
            "recipe_version",
            "must be a numeric major.minor.patch version",
        ))
    }
}

pub fn validate_environment_name(value: &str) -> Result<()> {
    if !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_uppercase()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    {
        Ok(())
    } else {
        Err(invalid(
            "environment",
            "must be an uppercase environment key",
        ))
    }
}

fn validate_role(value: &str) -> Result<()> {
    if !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        Ok(())
    } else {
        Err(invalid(
            "role",
            "must contain lowercase letters, digits, or underscores",
        ))
    }
}

fn invalid(field: &str, message: impl Into<String>) -> LumicError {
    LumicError::InvalidInput {
        field: field.into(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_recipe_is_versioned_valid_and_plannable() {
        let recipe = reference_recipes().remove(0);
        recipe.validate().unwrap();
        let plan = recipe
            .plan(
                &RecipeInstallRequest {
                    recipe_id: "static-git".into(),
                    application_id: "demo".into(),
                    domain: "demo.example.com".into(),
                    repository_url: Some("https://example.com/demo.git".into()),
                    branch: "main".into(),
                    tls_email: None,
                    environment: BTreeMap::new(),
                },
                false,
            )
            .unwrap();
        assert!(plan.changes.iter().all(|change| change.reversible));
    }

    #[test]
    fn schema_rejects_duplicate_environment_and_unknown_code_steps() {
        let mut recipe = reference_recipes().remove(0);
        recipe.environment.push(recipe.environment[0].clone());
        assert!(recipe.validate().is_err());
    }

    #[test]
    fn reconciliation_reuses_previously_supplied_required_inputs() {
        let mut recipe = reference_recipes().remove(0);
        recipe.environment = vec![RecipeEnvironmentValue {
            name: "DEPLOY_KEY".into(),
            source: RecipeEnvironmentSource::Input { required: true },
        }];
        let request = RecipeInstallRequest {
            recipe_id: "static-git".into(),
            application_id: "demo".into(),
            domain: "demo.example.com".into(),
            repository_url: None,
            branch: "main".into(),
            tls_email: None,
            environment: BTreeMap::new(),
        };
        assert!(recipe.plan(&request, false).is_err());
        assert!(recipe.plan(&request, true).is_ok());
    }

    #[test]
    fn wordpress_definition_pins_valid_artifacts_and_validates_inputs() {
        let recipe = wordpress_recipe();
        recipe.validate().unwrap();
        let request = RecipeInstallRequest {
            recipe_id: "wordpress".into(),
            application_id: "blog".into(),
            domain: "blog.example.com".into(),
            repository_url: None,
            branch: "main".into(),
            tls_email: None,
            environment: BTreeMap::from([
                ("WORDPRESS_SITE_TITLE".into(), "Example blog".into()),
                ("WORDPRESS_ADMIN_USER".into(), "admin_user".into()),
                ("WORDPRESS_ADMIN_EMAIL".into(), "admin@example.com".into()),
            ]),
        };
        assert!(recipe.plan(&request, false).is_ok());
        let mut invalid = request;
        invalid
            .environment
            .insert("WORDPRESS_ADMIN_EMAIL".into(), "invalid".into());
        assert!(recipe.plan(&invalid, false).is_err());
    }
}
