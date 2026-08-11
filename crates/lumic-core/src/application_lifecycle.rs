//! Desired state and explicit lifecycle plans for generic PHP applications.

use crate::{
    Capability, Change, LumicError, Plan, Result, Risk, RiskLevel,
    application::{
        ApplicationProcess, ApplicationProcessKind, ApplicationServiceReference, HealthCheck,
        RepositoryConfig, validate_branch, validate_domain, validate_repository_url, validate_slug,
    },
    catalog::Configuration,
    package::{PackagePolicy, ReviewedPackageRequirement},
    pipeline::{HealthCheck as PipelineHealthCheck, Pipeline, PipelineAction, PipelineStep},
    resource::{ResourceKind, ResourceRef},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeSet, path::Path};

const PHP_VERSIONS: [&str; 4] = ["8.1", "8.2", "8.3", "8.4"];
const PHP_COMPONENTS: [&str; 6] = ["curl", "intl", "mbstring", "mysql", "xml", "zip"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationLifecycleOperation {
    Install,
    Reconcile,
    Update,
    Remove,
}

impl ApplicationLifecycleOperation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Reconcile => "reconcile",
            Self::Update => "update",
            Self::Remove => "remove",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenericPhpApplicationSpec {
    pub id: String,
    pub domain: String,
    #[serde(default)]
    pub www_alias: bool,
    pub root: String,
    pub php_version: String,
    pub repository: Option<RepositoryConfig>,
    #[serde(default)]
    pub components: Vec<String>,
    #[serde(default)]
    pub databases: Vec<ApplicationServiceReference>,
    #[serde(default)]
    pub packages: Vec<String>,
    #[serde(default)]
    pub tls: bool,
    #[serde(default)]
    pub processes: Vec<ApplicationProcess>,
    #[serde(default)]
    pub health: HealthCheck,
}

impl GenericPhpApplicationSpec {
    pub fn validate(&self) -> Result<()> {
        validate_slug("application", &self.id)?;
        validate_domain(&self.domain)?;
        validate_root(&self.root)?;
        if let Some(repository) = &self.repository {
            validate_repository_url(&repository.url)?;
            validate_branch(&repository.branch)?;
            if repository
                .credential_reference
                .as_deref()
                .is_some_and(|reference| !reference.starts_with("secret://"))
            {
                return Err(invalid(
                    "repository.credential_reference",
                    "must use a secret:// reference",
                ));
            }
        }
        if !PHP_VERSIONS.contains(&self.php_version.as_str()) {
            return Err(invalid(
                "php_version",
                "must be one of 8.1, 8.2, 8.3, or 8.4",
            ));
        }
        validate_unique_tokens("component", &self.components, &PHP_COMPONENTS)?;
        validate_packages(&self.packages)?;
        validate_databases(&self.databases)?;
        validate_processes(&self.processes)?;
        validate_health(&self.health)
    }

    pub fn resource_ref(&self) -> Result<ResourceRef> {
        ResourceRef::new(ResourceKind::Application, &self.id)
    }

    pub fn lifecycle_plan(
        &self,
        operation: ApplicationLifecycleOperation,
    ) -> Result<ApplicationLifecyclePlan> {
        self.validate()?;
        if operation == ApplicationLifecycleOperation::Update && self.repository.is_none() {
            return Err(invalid(
                "repository",
                "an update plan requires a configured repository",
            ));
        }
        let plan = human_plan(self, operation);
        let pipeline = pipeline(self, operation)?;
        pipeline.validate()?;
        let package_requirements = PackagePolicy::default_catalog()
            .review_names(&self.packages, "generic PHP application requirement")?;
        Ok(ApplicationLifecyclePlan {
            operation,
            plan,
            pipeline,
            package_requirements,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationLifecyclePlan {
    pub operation: ApplicationLifecycleOperation,
    pub plan: Plan,
    pub pipeline: Pipeline,
    /// Requirements that acquired trust through an explicit package policy review.
    pub package_requirements: Vec<ReviewedPackageRequirement>,
}

fn human_plan(spec: &GenericPhpApplicationSpec, operation: ApplicationLifecycleOperation) -> Plan {
    let operation_name = operation.as_str();
    let removal = operation == ApplicationLifecycleOperation::Remove;
    let mut changes = vec![change(
        "application.lifecycle",
        format!("{operation_name} generic PHP application {}", spec.id),
        removal.then(|| "managed application present".into()),
        (!removal).then(|| "desired application state reconciled".into()),
        true,
    )];
    if !removal {
        changes.extend([
            change(
                "application.web.configure",
                format!("route {} to {}", spec.domain, spec.root),
                None,
                Some("owned nginx web host configured".into()),
                true,
            ),
            change(
                "application.runtime.provision",
                format!(
                    "ensure PHP {} with components {}",
                    spec.php_version,
                    display_list(&spec.components)
                ),
                None,
                Some("versioned PHP runtime bound to web host".into()),
                false,
            ),
        ]);
        if !spec.databases.is_empty() {
            changes.push(change(
                "application.service.attach",
                format!("bind database roles {}", database_roles(spec)),
                None,
                Some("typed database and credential references attached".into()),
                true,
            ));
        }
        if !spec.processes.is_empty() {
            changes.push(change(
                "application.process.configure",
                format!(
                    "reconcile {} process or schedule definition(s)",
                    spec.processes.len()
                ),
                None,
                Some("owned systemd units configured".into()),
                true,
            ));
        }
    }
    Plan {
        id: format!("application-{}-{operation_name}", spec.id),
        summary: format!(
            "{} generic PHP application {}",
            title(operation_name),
            spec.id
        ),
        changes,
        risks: vec![Risk {
            level: if removal {
                RiskLevel::High
            } else {
                RiskLevel::Medium
            },
            summary: if removal {
                "The application will stop serving traffic and its root will move to recoverable trash"
                    .into()
            } else {
                "Package, nginx, PHP-FPM, process, and TLS configuration may change".into()
            },
            mitigation: Some(if removal {
                "Shared runtimes, packages, databases, and managed services are retained".into()
            } else {
                "The pipeline validates native configuration before committing framework state"
                    .into()
            }),
        }],
        preconditions: vec![
            "the application resource lock is available".into(),
            "requested packages are allowed by policy".into(),
            "referenced database resources and grants exist".into(),
            "material changes are explicitly approved".into(),
        ],
        validation: if removal {
            vec![
                "owned web, certificate, process, and schedule bindings are absent".into(),
                "shared providers remain installed".into(),
            ]
        } else {
            vec![
                "nginx configuration validates and reloads".into(),
                "the selected PHP-FPM runtime is healthy".into(),
                "the configured application health check succeeds".into(),
            ]
        },
        recovery: if removal {
            vec!["restore the application root from Lumic trash and reconcile it again".into()]
        } else {
            vec![
                "restore the last known-good nginx configuration".into(),
                "keep shared packages, runtimes, and managed services for a safe retry".into(),
                "inspect the persisted pipeline journal before retrying".into(),
            ]
        },
    }
}

fn pipeline(
    spec: &GenericPhpApplicationSpec,
    operation: ApplicationLifecycleOperation,
) -> Result<Pipeline> {
    let mut steps = if operation == ApplicationLifecycleOperation::Remove {
        removal_steps(spec)
    } else {
        reconciliation_steps(spec, operation)
    };
    steps.push(PipelineStep {
        id: "commit".into(),
        summary: "Commit application resource state".into(),
        action: PipelineAction::CommitState,
    });
    Ok(Pipeline {
        id: format!("application-{}-{}", spec.id, operation.as_str()),
        target: spec.resource_ref()?,
        summary: format!(
            "{} generic PHP application {}",
            title(operation.as_str()),
            spec.id
        ),
        steps,
    })
}

fn reconciliation_steps(
    spec: &GenericPhpApplicationSpec,
    operation: ApplicationLifecycleOperation,
) -> Vec<PipelineStep> {
    let mut steps = vec![PipelineStep {
        id: "root".into(),
        summary: "Ensure the managed application root".into(),
        action: PipelineAction::EnsureDirectory {
            path: spec.root.clone(),
            mode: 0o755,
        },
    }];
    steps.extend(spec.packages.iter().map(|package| PipelineStep {
        id: format!("package-{package}"),
        summary: format!("Ensure trusted package {package}"),
        action: PipelineAction::EnsurePackage {
            package: package.clone(),
        },
    }));
    if let Some(repository) = &spec.repository {
        steps.push(provider_step(
            "repository",
            format!("Configure repository branch {}", repository.branch),
            "repository",
            Configuration::from([
                ("url".into(), Value::String(repository.url.clone())),
                ("branch".into(), Value::String(repository.branch.clone())),
            ]),
        ));
    }
    steps.push(provider_step(
        "runtime",
        format!("Ensure PHP {} runtime", spec.php_version),
        "runtime",
        Configuration::from([("version".into(), Value::String(spec.php_version.clone()))]),
    ));
    steps.extend(spec.components.iter().map(|component| {
        provider_step(
            &format!("component-{component}"),
            format!("Ensure PHP component {component}"),
            "component",
            Configuration::from([
                ("name".into(), Value::String(component.clone())),
                ("version".into(), Value::String(spec.php_version.clone())),
            ]),
        )
    }));
    steps.extend(spec.databases.iter().map(|database| {
        provider_step(
            &format!("database-{}", database.role),
            format!("Bind database role {}", database.role),
            "database_binding",
            database_parameters(database),
        )
    }));
    steps.push(provider_step(
        "web",
        format!("Configure owned web host for {}", spec.domain),
        "web_host",
        Configuration::from([
            ("domain".into(), Value::String(spec.domain.clone())),
            ("root".into(), Value::String(spec.root.clone())),
            ("www_alias".into(), Value::Bool(spec.www_alias)),
        ]),
    ));
    steps.extend(spec.processes.iter().map(|process| {
        let operation = match process.kind {
            ApplicationProcessKind::Worker => "process",
            ApplicationProcessKind::Schedule => "schedule",
        };
        provider_step(
            &format!("{operation}-{}", process.name),
            format!("Configure {operation} {}", process.name),
            operation,
            Configuration::from([("name".into(), Value::String(process.name.clone()))]),
        )
    }));
    if spec.tls {
        steps.push(provider_step(
            "tls",
            format!("Ensure TLS for {}", spec.domain),
            "tls",
            Configuration::from([("domain".into(), Value::String(spec.domain.clone()))]),
        ));
    }
    if operation == ApplicationLifecycleOperation::Update {
        steps.push(provider_step(
            "release",
            "Deploy the latest configured repository release".into(),
            "deploy",
            Configuration::new(),
        ));
    }
    if spec.health.enabled {
        let scheme = if spec.tls { "https" } else { "http" };
        steps.push(PipelineStep {
            id: "health".into(),
            summary: "Verify application health".into(),
            action: PipelineAction::HealthCheck {
                check: PipelineHealthCheck::Http {
                    url: format!("{scheme}://{}{}", spec.domain, spec.health.path),
                    expected_status: spec.health.expected_status_min,
                },
            },
        });
    }
    steps
}

fn removal_steps(spec: &GenericPhpApplicationSpec) -> Vec<PipelineStep> {
    let mut steps = Vec::new();
    if spec.tls {
        steps.push(provider_step(
            "tls-detach",
            "Detach the owned certificate".into(),
            "tls_detach",
            Configuration::new(),
        ));
    }
    steps.extend(spec.processes.iter().rev().map(|process| {
        let kind = match process.kind {
            ApplicationProcessKind::Worker => "process_remove",
            ApplicationProcessKind::Schedule => "schedule_remove",
        };
        provider_step(
            &format!("remove-{}", process.name),
            format!("Remove owned units for {}", process.name),
            kind,
            Configuration::from([("name".into(), Value::String(process.name.clone()))]),
        )
    }));
    steps.extend(spec.databases.iter().rev().map(|database| {
        provider_step(
            &format!("unbind-{}", database.role),
            format!("Detach database role {}", database.role),
            "database_unbind",
            Configuration::from([("role".into(), Value::String(database.role.clone()))]),
        )
    }));
    steps.push(provider_step(
        "web-remove",
        "Remove the owned web host".into(),
        "web_host_remove",
        Configuration::new(),
    ));
    steps.push(provider_step(
        "root-trash",
        "Move the application root to recoverable trash".into(),
        "root_trash",
        Configuration::from([("root".into(), Value::String(spec.root.clone()))]),
    ));
    steps
}

fn provider_step(
    id: &str,
    summary: String,
    operation: &str,
    parameters: Configuration,
) -> PipelineStep {
    PipelineStep {
        id: id.into(),
        summary,
        action: PipelineAction::ProviderAction {
            provider: "generic_php_application".into(),
            operation: operation.into(),
            parameters,
        },
    }
}

fn database_parameters(reference: &ApplicationServiceReference) -> Configuration {
    let mut parameters = Configuration::from([
        (
            "service_id".into(),
            Value::String(reference.service_id.clone()),
        ),
        ("role".into(), Value::String(reference.role.clone())),
    ]);
    for (name, value) in [
        ("database", &reference.database),
        ("user", &reference.user),
        ("secret_reference", &reference.secret_reference),
    ] {
        if let Some(value) = value {
            parameters.insert(name.into(), Value::String(value.clone()));
        }
    }
    parameters
}

fn validate_root(value: &str) -> Result<()> {
    let path = Path::new(value);
    if !path.is_absolute()
        || value.len() > 4096
        || value.bytes().any(|byte| byte.is_ascii_control())
        || path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(invalid(
            "root",
            "must be a safe absolute path without parent traversal",
        ));
    }
    Ok(())
}

fn validate_unique_tokens(field: &str, values: &[String], allowed: &[&str]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for value in values {
        if !allowed.contains(&value.as_str()) {
            return Err(invalid(field, "is not a supported PHP component"));
        }
        if !seen.insert(value) {
            return Err(invalid(field, "must not contain duplicates"));
        }
    }
    Ok(())
}

fn validate_packages(packages: &[String]) -> Result<()> {
    let policy = PackagePolicy::default_catalog();
    let reviewed = policy.review_names(packages, "generic PHP application requirement")?;
    let mut seen = BTreeSet::new();
    for package in reviewed {
        if !seen.insert(package.requirement.name) {
            return Err(invalid("package", "must not contain duplicates"));
        }
    }
    Ok(())
}

fn validate_databases(databases: &[ApplicationServiceReference]) -> Result<()> {
    let mut roles = BTreeSet::new();
    for database in databases {
        ResourceRef::new(ResourceKind::ManagedService, &database.service_id)?;
        validate_slug("database.role", &database.role)?;
        if !roles.insert(&database.role) {
            return Err(invalid("database.role", "must be unique"));
        }
        if database.database.is_none() && database.user.is_none() {
            return Err(invalid(
                "database",
                "must reference a database, a user, or both",
            ));
        }
        if database
            .secret_reference
            .as_deref()
            .is_some_and(|reference| !reference.starts_with("secret://"))
        {
            return Err(invalid(
                "database.secret_reference",
                "must use a secret:// reference",
            ));
        }
    }
    Ok(())
}

fn validate_processes(processes: &[ApplicationProcess]) -> Result<()> {
    let mut names = BTreeSet::new();
    for process in processes {
        process.validate()?;
        if !names.insert(&process.name) {
            return Err(invalid("process", "names must be unique"));
        }
    }
    Ok(())
}

fn validate_health(health: &HealthCheck) -> Result<()> {
    if !health.enabled {
        return Ok(());
    }
    if !health.path.starts_with('/')
        || health.path.contains(['\n', '\r', '\0'])
        || health.port == 0
        || !(100..=599).contains(&health.expected_status_min)
        || !(health.expected_status_min..=599).contains(&health.expected_status_max)
        || health.timeout_seconds == 0
    {
        return Err(invalid("health", "contains an invalid HTTP health check"));
    }
    Ok(())
}

fn change(
    capability: &str,
    summary: String,
    before: Option<String>,
    after: Option<String>,
    reversible: bool,
) -> Change {
    Change {
        capability: Capability::new(capability),
        summary,
        before,
        after,
        reversible,
    }
}

fn database_roles(spec: &GenericPhpApplicationSpec) -> String {
    display_list(
        &spec
            .databases
            .iter()
            .map(|database| database.role.clone())
            .collect::<Vec<_>>(),
    )
}

fn display_list(values: &[String]) -> String {
    if values.is_empty() {
        "none".into()
    } else {
        values.join(", ")
    }
}

fn title(value: &str) -> String {
    let mut characters = value.chars();
    characters
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + characters.as_str())
        .unwrap_or_default()
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

    fn spec() -> GenericPhpApplicationSpec {
        GenericPhpApplicationSpec {
            id: "shop".into(),
            domain: "shop.example.com".into(),
            www_alias: true,
            root: "/var/lib/lumic/apps/shop".into(),
            php_version: "8.3".into(),
            repository: Some(RepositoryConfig {
                url: "https://example.com/shop.git".into(),
                branch: "main".into(),
                credential_reference: None,
                deployment: Default::default(),
                contract: None,
            }),
            components: vec!["curl".into(), "mysql".into()],
            databases: vec![ApplicationServiceReference {
                service_id: "mysql.main".into(),
                role: "primary".into(),
                database: Some("shop".into()),
                user: Some("shop".into()),
                secret_reference: Some("secret://mysql-shop-password".into()),
            }],
            packages: vec!["git".into(), "composer".into()],
            tls: true,
            processes: vec![ApplicationProcess {
                name: "queue".into(),
                kind: ApplicationProcessKind::Worker,
                command: vec!["php".into(), "artisan".into(), "queue:work".into()],
                schedule: None,
                enabled: true,
            }],
            health: HealthCheck {
                enabled: true,
                path: "/health".into(),
                ..HealthCheck::default()
            },
        }
    }

    #[test]
    fn builds_distinct_valid_lifecycle_plans() {
        for operation in [
            ApplicationLifecycleOperation::Install,
            ApplicationLifecycleOperation::Reconcile,
            ApplicationLifecycleOperation::Update,
            ApplicationLifecycleOperation::Remove,
        ] {
            let lifecycle = spec().lifecycle_plan(operation).unwrap();
            assert_eq!(lifecycle.operation, operation);
            assert!(lifecycle.pipeline.validate().is_ok());
            assert_eq!(lifecycle.package_requirements.len(), 2);
            assert!(lifecycle.package_requirements.iter().all(|requirement| {
                requirement.trust_source == crate::package::PackageTrustSource::BuiltInPolicy
            }));
            assert_eq!(
                lifecycle.pipeline.steps.last().unwrap().action,
                PipelineAction::CommitState
            );
        }
    }

    #[test]
    fn update_deploys_and_reconcile_does_not() {
        let update = spec()
            .lifecycle_plan(ApplicationLifecycleOperation::Update)
            .unwrap();
        let reconcile = spec()
            .lifecycle_plan(ApplicationLifecycleOperation::Reconcile)
            .unwrap();
        assert!(
            update
                .pipeline
                .steps
                .iter()
                .any(|step| step.id == "release")
        );
        assert!(
            !reconcile
                .pipeline
                .steps
                .iter()
                .any(|step| step.id == "release")
        );
    }

    #[test]
    fn removal_is_dependency_aware_and_keeps_shared_providers() {
        let removal = spec()
            .lifecycle_plan(ApplicationLifecycleOperation::Remove)
            .unwrap();
        let ids = removal
            .pipeline
            .steps
            .iter()
            .map(|step| step.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids.first(), Some(&"tls-detach"));
        assert!(ids.contains(&"unbind-primary"));
        assert!(!ids.iter().any(|id| id.starts_with("package-")));
        assert!(!ids.contains(&"runtime"));
    }

    #[test]
    fn rejects_untrusted_packages_duplicate_roles_and_unsafe_roots() {
        let mut value = spec();
        value.packages.push("not-in-policy".into());
        assert!(value.validate().is_err());

        let mut value = spec();
        value.databases.push(value.databases[0].clone());
        assert!(value.validate().is_err());

        let mut value = spec();
        value.root = "/var/lib/lumic/../etc".into();
        assert!(value.validate().is_err());
    }

    #[test]
    fn plans_never_persist_tls_contact_or_secret_values() {
        let serialized = serde_json::to_string(
            &spec()
                .lifecycle_plan(ApplicationLifecycleOperation::Install)
                .unwrap(),
        )
        .unwrap();
        assert!(!serialized.contains("password-value"));
        assert!(serialized.contains("secret://mysql-shop-password"));
        assert!(!serialized.contains("contact_email"));
    }
}
