use crate::{Capability, Change, LumicError, Plan, Result, Risk, RiskLevel};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoftwareCategory {
    Application,
    Runtime,
    Database,
    Cache,
    Search,
    WebServer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoftwareSetupScope {
    System,
    User,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoftwarePackageSource {
    Distribution,
    ExternalRepository,
}

impl fmt::Display for SoftwareCategory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Application => "application",
            Self::Runtime => "runtime",
            Self::Database => "database",
            Self::Cache => "cache",
            Self::Search => "search",
            Self::WebServer => "web server",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SoftwareDefinition {
    pub id: &'static str,
    pub name: &'static str,
    pub category: SoftwareCategory,
    pub description: &'static str,
    pub setup_scope: SoftwareSetupScope,
    pub package_source: SoftwarePackageSource,
    pub packages: &'static [&'static str],
}

const WORDPRESS_PACKAGES: &[&str] = &["php-fpm", "php-mysql", "default-mysql-server", "nginx"];
const PHP_PACKAGES: &[&str] = &["php-fpm", "php-cli", "composer"];
const MYSQL_PACKAGES: &[&str] = &["default-mysql-server"];
const POSTGRESQL_PACKAGES: &[&str] = &["postgresql"];
const REDIS_PACKAGES: &[&str] = &["redis-server"];
const TYPESENSE_PACKAGES: &[&str] = &["typesense-server"];
const MEILISEARCH_PACKAGES: &[&str] = &["meilisearch"];
const NGINX_PACKAGES: &[&str] = &["nginx"];
const APACHE_PACKAGES: &[&str] = &["apache2"];
const NODEJS_PACKAGES: &[&str] = &["nodejs", "npm"];
const NVM_PACKAGES: &[&str] = &["git", "curl"];

pub const SOFTWARE_CATALOG: &[SoftwareDefinition] = &[
    SoftwareDefinition {
        id: "wordpress",
        name: "WordPress prerequisites",
        category: SoftwareCategory::Application,
        description: "PHP-FPM, MySQL support, and nginx for a future WordPress recipe",
        setup_scope: SoftwareSetupScope::System,
        package_source: SoftwarePackageSource::Distribution,
        packages: WORDPRESS_PACKAGES,
    },
    SoftwareDefinition {
        id: "php",
        name: "PHP",
        category: SoftwareCategory::Runtime,
        description: "PHP-FPM, the PHP CLI, and Composer",
        setup_scope: SoftwareSetupScope::System,
        package_source: SoftwarePackageSource::Distribution,
        packages: PHP_PACKAGES,
    },
    SoftwareDefinition {
        id: "mysql",
        name: "Default MySQL-compatible server",
        category: SoftwareCategory::Database,
        description: "Ubuntu MySQL or Debian MariaDB through the distribution default metapackage",
        setup_scope: SoftwareSetupScope::System,
        package_source: SoftwarePackageSource::Distribution,
        packages: MYSQL_PACKAGES,
    },
    SoftwareDefinition {
        id: "postgresql",
        name: "PostgreSQL",
        category: SoftwareCategory::Database,
        description: "PostgreSQL server",
        setup_scope: SoftwareSetupScope::System,
        package_source: SoftwarePackageSource::Distribution,
        packages: POSTGRESQL_PACKAGES,
    },
    SoftwareDefinition {
        id: "redis",
        name: "Redis",
        category: SoftwareCategory::Cache,
        description: "Redis server",
        setup_scope: SoftwareSetupScope::System,
        package_source: SoftwarePackageSource::Distribution,
        packages: REDIS_PACKAGES,
    },
    SoftwareDefinition {
        id: "typesense",
        name: "Typesense",
        category: SoftwareCategory::Search,
        description: "Typesense search server from a configured apt source",
        setup_scope: SoftwareSetupScope::System,
        package_source: SoftwarePackageSource::ExternalRepository,
        packages: TYPESENSE_PACKAGES,
    },
    SoftwareDefinition {
        id: "meilisearch",
        name: "Meilisearch",
        category: SoftwareCategory::Search,
        description: "Meilisearch server from a configured apt source",
        setup_scope: SoftwareSetupScope::System,
        package_source: SoftwarePackageSource::ExternalRepository,
        packages: MEILISEARCH_PACKAGES,
    },
    SoftwareDefinition {
        id: "nginx",
        name: "nginx",
        category: SoftwareCategory::WebServer,
        description: "nginx web server",
        setup_scope: SoftwareSetupScope::System,
        package_source: SoftwarePackageSource::Distribution,
        packages: NGINX_PACKAGES,
    },
    SoftwareDefinition {
        id: "apache",
        name: "Apache",
        category: SoftwareCategory::WebServer,
        description: "Apache HTTP Server",
        setup_scope: SoftwareSetupScope::System,
        package_source: SoftwarePackageSource::Distribution,
        packages: APACHE_PACKAGES,
    },
    SoftwareDefinition {
        id: "nodejs",
        name: "Node.js",
        category: SoftwareCategory::Runtime,
        description: "System Node.js runtime and npm from distribution apt sources",
        setup_scope: SoftwareSetupScope::System,
        package_source: SoftwarePackageSource::Distribution,
        packages: NODEJS_PACKAGES,
    },
    SoftwareDefinition {
        id: "nvm",
        name: "NVM",
        category: SoftwareCategory::Runtime,
        description: "Per-user Node Version Manager from the pinned official Git repository",
        setup_scope: SoftwareSetupScope::User,
        package_source: SoftwarePackageSource::Distribution,
        packages: NVM_PACKAGES,
    },
];

pub fn software(id: &str) -> Result<&'static SoftwareDefinition> {
    SOFTWARE_CATALOG
        .iter()
        .find(|item| item.id == id)
        .ok_or_else(|| LumicError::InvalidInput {
            field: "software".into(),
            message: format!("unknown supported software: {id}"),
        })
}

pub fn setup_plan(definition: &SoftwareDefinition, installed: bool) -> Plan {
    let (summary, change, preconditions, recovery) = if definition.setup_scope
        == SoftwareSetupScope::User
    {
        (
                format!("Set up {} for a validated Linux user", definition.name),
                "Install prerequisites, reconcile the pinned NVM Git checkout, and activate it from the user's profile".into(),
                vec![
                    "Debian or Ubuntu with apt".into(),
                    "An existing Linux user with a writable home directory".into(),
                    "Outbound HTTPS access to the official nvm-sh/nvm Git repository".into(),
                ],
                vec!["Remove the user's .nvm directory and the Lumic-managed profile block after checking installed Node versions".into()],
            )
    } else {
        let source_precondition = match definition.package_source {
            SoftwarePackageSource::Distribution => {
                "Configured distribution package sources are reachable"
            }
            SoftwarePackageSource::ExternalRepository => {
                "A trusted external package source is configured and reachable"
            }
        };
        (
                format!("Set up {} from trusted native packages", definition.name),
                format!("Install and reconcile: {}", definition.packages.join(", ")),
                vec![
                    "Debian or Ubuntu with apt".into(),
                    source_precondition.into(),
                ],
                vec!["Use the package manager to remove unwanted packages after reviewing dependent packages and data".into()],
            )
    };
    Plan {
        id: format!("software-setup-{}", definition.id),
        summary,
        changes: vec![Change {
            capability: Capability::new("software.setup"),
            summary: change,
            before: Some(
                if installed {
                    "installed"
                } else {
                    "not installed"
                }
                .into(),
            ),
            after: Some("required packages installed".into()),
            reversible: true,
        }],
        risks: vec![Risk {
            level: RiskLevel::Medium,
            summary: "Setup can install packages, change services, or update user runtime files"
                .into(),
            mitigation: Some(
                "Lumic validates its fixed catalog, uses separated process arguments, and pins NVM to a reviewed upstream tag"
                    .into(),
            ),
        }],
        preconditions,
        validation: vec!["Inspect installed and candidate package versions after setup".into()],
        recovery,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::{PackageName, PackagePolicy};

    #[test]
    fn default_catalog_contains_requested_software_and_safe_packages() {
        let policy = PackagePolicy::default_catalog();

        for id in [
            "wordpress",
            "php",
            "mysql",
            "postgresql",
            "redis",
            "typesense",
            "meilisearch",
            "nginx",
            "apache",
            "nodejs",
            "nvm",
        ] {
            let definition = software(id).unwrap();
            assert!(!definition.packages.is_empty());
            for package in definition.packages {
                let package = PackageName::parse(*package).unwrap();
                assert!(
                    policy.authorize(&package).is_ok(),
                    "catalog package {package} is not trusted"
                );
            }
        }
    }

    #[test]
    fn wordpress_setup_does_not_claim_an_unavailable_native_package() {
        let definition = software("wordpress").unwrap();

        assert!(!definition.packages.contains(&"wordpress"));
    }

    #[test]
    fn mysql_setup_uses_the_cross_distribution_metapackage() {
        let definition = software("mysql").unwrap();

        assert_eq!(definition.packages, &["default-mysql-server"]);
    }

    #[test]
    fn only_third_party_catalog_entries_require_external_repositories() {
        for id in ["typesense", "meilisearch"] {
            assert_eq!(
                software(id).unwrap().package_source,
                SoftwarePackageSource::ExternalRepository
            );
        }

        for id in [
            "wordpress",
            "php",
            "mysql",
            "postgresql",
            "redis",
            "nginx",
            "apache",
            "nodejs",
            "nvm",
        ] {
            assert_eq!(
                software(id).unwrap().package_source,
                SoftwarePackageSource::Distribution
            );
        }
    }
}
