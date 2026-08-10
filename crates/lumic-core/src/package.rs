use crate::{LumicError, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fmt, str::FromStr};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PackageName(String);

impl PackageName {
    pub fn parse(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 128
            && value.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'+' | b'-' | b'.')
            })
            && value.as_bytes()[0].is_ascii_alphanumeric();
        if !valid {
            return Err(LumicError::InvalidInput {
                field: "package".into(),
                message:
                    "must be a Debian package identifier (lowercase letters, digits, +, - and .)"
                        .into(),
            });
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PackageName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for PackageName {
    type Err = LumicError;

    fn from_str(value: &str) -> Result<Self> {
        Self::parse(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackagePolicy {
    allowed: BTreeSet<PackageName>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageRequirement {
    pub name: PackageName,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageTrustSource {
    BuiltInPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewedPackageRequirement {
    pub requirement: PackageRequirement,
    pub trust_source: PackageTrustSource,
}

impl PackagePolicy {
    pub fn default_catalog() -> Self {
        let names = [
            "ca-certificates",
            "certbot",
            "apache2",
            "composer",
            "curl",
            "default-mysql-server",
            "ffmpeg",
            "git",
            "nginx",
            "meilisearch",
            "nodejs",
            "npm",
            "php",
            "php-cli",
            "php-common",
            "php-curl",
            "php-fpm",
            "php-intl",
            "php-mysql",
            "php-mbstring",
            "php-xml",
            "php-zip",
            "php8.1-cli",
            "php8.1-curl",
            "php8.1-fpm",
            "php8.1-intl",
            "php8.1-mbstring",
            "php8.1-mysql",
            "php8.1-xml",
            "php8.1-zip",
            "php8.2-cli",
            "php8.2-curl",
            "php8.2-fpm",
            "php8.2-intl",
            "php8.2-mbstring",
            "php8.2-mysql",
            "php8.2-xml",
            "php8.2-zip",
            "php8.3-cli",
            "php8.3-curl",
            "php8.3-fpm",
            "php8.3-intl",
            "php8.3-mbstring",
            "php8.3-mysql",
            "php8.3-xml",
            "php8.3-zip",
            "php8.4-cli",
            "php8.4-curl",
            "php8.4-fpm",
            "php8.4-intl",
            "php8.4-mbstring",
            "php8.4-mysql",
            "php8.4-xml",
            "php8.4-zip",
            "python3-certbot-nginx",
            "postgresql",
            "redis-server",
            "typesense-server",
        ];
        Self {
            allowed: names
                .into_iter()
                .map(|name| PackageName::parse(name).expect("built-in package names are valid"))
                .collect(),
        }
    }

    pub fn with_trusted(mut self, package: PackageName) -> Self {
        self.allowed.insert(package);
        self
    }

    pub fn authorize(&self, package: &PackageName) -> Result<()> {
        if self.allowed.contains(package) {
            Ok(())
        } else {
            Err(LumicError::PolicyDenied {
                capability: crate::Capability::new(format!("package.install.{package}")),
            })
        }
    }

    pub fn review(&self, requirement: PackageRequirement) -> Result<ReviewedPackageRequirement> {
        if requirement.reason.trim().is_empty()
            || requirement.reason.len() > 256
            || requirement.reason.contains(['\n', '\r', '\0'])
        {
            return Err(LumicError::InvalidInput {
                field: "package.reason".into(),
                message: "must be a short, non-empty explanation".into(),
            });
        }
        self.authorize(&requirement.name)?;
        Ok(ReviewedPackageRequirement {
            requirement,
            trust_source: PackageTrustSource::BuiltInPolicy,
        })
    }

    pub fn review_names(
        &self,
        packages: &[String],
        reason: &str,
    ) -> Result<Vec<ReviewedPackageRequirement>> {
        packages
            .iter()
            .map(|package| {
                self.review(PackageRequirement {
                    name: PackageName::parse(package)?,
                    reason: reason.into(),
                })
            })
            .collect()
    }

    pub fn allowed(&self) -> impl Iterator<Item = &PackageName> {
        self.allowed.iter()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageRecord {
    pub name: PackageName,
    pub installed_version: Option<String>,
    pub candidate_version: Option<String>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageMutation {
    pub package: PackageName,
    pub action: String,
    pub changed: bool,
    pub output: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_names_reject_shell_and_apt_syntax() {
        for value in ["", "Nginx", "nginx;id", "nginx=1.2", "../nginx", "-nginx"] {
            assert!(PackageName::parse(value).is_err(), "accepted {value}");
        }
        assert_eq!(
            PackageName::parse("php8.3-fpm").unwrap().as_str(),
            "php8.3-fpm"
        );
    }

    #[test]
    fn unknown_package_is_not_implicitly_trusted() {
        let policy = PackagePolicy::default_catalog();
        assert!(
            policy
                .authorize(&PackageName::parse("nginx").unwrap())
                .is_ok()
        );
        assert!(
            policy
                .authorize(&PackageName::parse("unknown-good-name").unwrap())
                .is_err()
        );
    }

    #[test]
    fn package_trust_is_derived_only_after_policy_review() {
        let policy = PackagePolicy::default_catalog();
        let reviewed = policy
            .review(PackageRequirement {
                name: PackageName::parse("nginx").unwrap(),
                reason: "serve the application".into(),
            })
            .unwrap();
        assert_eq!(reviewed.trust_source, PackageTrustSource::BuiltInPolicy);
    }

    #[test]
    fn versioned_php_component_packages_are_trusted() {
        let policy = PackagePolicy::default_catalog();

        for version in ["8.1", "8.2", "8.3", "8.4"] {
            for component in ["curl", "intl", "mbstring", "mysql", "xml", "zip"] {
                let package = PackageName::parse(format!("php{version}-{component}")).unwrap();
                assert!(
                    policy.authorize(&package).is_ok(),
                    "versioned PHP component package {package} is not trusted"
                );
            }
        }
    }
}
