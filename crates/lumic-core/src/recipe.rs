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
    HealthCheck { path: String, port: u16 },
    Process { process: ApplicationProcess },
    Deploy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipeDefinition {
    pub metadata: RecipeMetadata,
    pub runtime: ApplicationRuntime,
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
    }]
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
}
