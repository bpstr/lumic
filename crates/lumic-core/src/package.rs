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

impl PackagePolicy {
    pub fn default_catalog() -> Self {
        let names = [
            "ca-certificates",
            "certbot",
            "composer",
            "curl",
            "ffmpeg",
            "git",
            "nginx",
            "nodejs",
            "php",
            "php-cli",
            "php-common",
            "php-curl",
            "php-fpm",
            "php-intl",
            "php-mbstring",
            "php-xml",
            "php-zip",
            "python3-certbot-nginx",
            "postgresql",
            "redis-server",
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
}
