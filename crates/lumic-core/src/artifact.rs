use crate::{LumicError, Result, managed_service::validate_resource_id};
use serde::{Deserialize, Serialize};

/// A reviewed, immutable artifact that may enter Lumic's local cache.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactDefinition {
    pub id: String,
    pub version: String,
    pub url: String,
    pub sha256: String,
}

impl ArtifactDefinition {
    pub fn validate(&self) -> Result<()> {
        validate_resource_id("artifact", &self.id)?;
        validate_version(&self.version)?;
        if !self.url.starts_with("https://")
            || self.url.len() > 2048
            || self.url.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(invalid("artifact_url", "must be a bounded HTTPS URL"));
        }
        if self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(invalid(
                "artifact_sha256",
                "must be a lowercase SHA-256 digest",
            ));
        }
        Ok(())
    }

    pub fn cache_file_name(&self, extension: &str) -> Result<String> {
        self.validate()?;
        if extension.is_empty()
            || extension.len() > 16
            || !extension
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.')
        {
            return Err(invalid(
                "artifact_extension",
                "must contain lowercase letters, digits, or dots",
            ));
        }
        Ok(format!("{}-{}.{}", self.id, self.version, extension))
    }
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
            "artifact_version",
            "must be a numeric major.minor.patch version",
        ))
    }
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
    fn artifact_definition_requires_https_version_and_digest() {
        let artifact = ArtifactDefinition {
            id: "tool".into(),
            version: "1.2.3".into(),
            url: "https://example.com/tool".into(),
            sha256: "a".repeat(64),
        };
        assert!(artifact.validate().is_ok());
    }

    #[test]
    fn artifact_cache_name_rejects_path_separators() {
        let artifact = ArtifactDefinition {
            id: "tool".into(),
            version: "1.2.3".into(),
            url: "https://example.com/tool".into(),
            sha256: "a".repeat(64),
        };
        assert!(artifact.cache_file_name("../bin").is_err());
    }
}
