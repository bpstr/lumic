//! Stable resource identity and typed outputs shared by orchestration layers.

use crate::{LumicError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Resource categories managed or observed by Lumic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Package,
    Component,
    Runtime,
    ManagedService,
    ServiceResource,
    Endpoint,
    Application,
    Process,
    Schedule,
    Artifact,
    Certificate,
    Pipeline,
}

/// A stable reference used by bindings, locks, and audit records.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceRef {
    pub kind: ResourceKind,
    pub id: String,
}

impl ResourceRef {
    pub fn new(kind: ResourceKind, id: impl Into<String>) -> Result<Self> {
        let resource = Self {
            kind,
            id: id.into(),
        };
        resource.validate()?;
        Ok(resource)
    }

    pub fn validate(&self) -> Result<()> {
        validate_resource_id("resource.id", &self.id)
    }

    /// A deterministic key suitable for state indexes and lock file names.
    pub fn key(&self) -> String {
        format!("{:?}:{}", self.kind, self.id).to_ascii_lowercase()
    }
}

/// A value exported by a resource for explicit downstream binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceOutput {
    pub value: Value,
    #[serde(default)]
    pub sensitive: bool,
    pub updated_at_unix_ms: u64,
}

impl ResourceOutput {
    pub fn validate(&self, name: &str) -> Result<()> {
        validate_output_name("output.name", name)?;
        if self.sensitive
            && !self
                .value
                .as_str()
                .is_some_and(|value| value.starts_with("secret://"))
        {
            return Err(invalid(
                "output.value",
                "sensitive outputs must contain a secret reference",
            ));
        }
        Ok(())
    }
}

pub type ResourceOutputs = BTreeMap<String, ResourceOutput>;

/// Generic persisted state for resources that are not service instances.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceRecord {
    pub resource: ResourceRef,
    #[serde(default)]
    pub attributes: BTreeMap<String, Value>,
    #[serde(default)]
    pub outputs: ResourceOutputs,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
}

impl ResourceRecord {
    pub fn validate(&self) -> Result<()> {
        self.resource.validate()?;
        if self.created_at_unix_ms > self.updated_at_unix_ms {
            return Err(invalid(
                "resource.updated_at_unix_ms",
                "must not precede creation",
            ));
        }
        for (name, output) in &self.outputs {
            output.validate(name)?;
        }
        Ok(())
    }
}

pub(crate) fn validate_resource_id(field: &str, value: &str) -> Result<()> {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_lowercase()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        });
    if valid {
        Ok(())
    } else {
        Err(invalid(
            field,
            "must start with a lowercase letter and contain lowercase letters, digits, '-', '_', or '.'",
        ))
    }
}

pub(crate) fn validate_output_name(field: &str, value: &str) -> Result<()> {
    validate_resource_id(field, value)
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
    fn validates_stable_resource_references() {
        assert!(ResourceRef::new(ResourceKind::ManagedService, "postgres.main").is_ok());
        assert!(ResourceRef::new(ResourceKind::ManagedService, "../bad").is_err());
    }

    #[test]
    fn sensitive_outputs_never_embed_plaintext() {
        let output = ResourceOutput {
            value: Value::String("plaintext".into()),
            sensitive: true,
            updated_at_unix_ms: 1,
        };
        assert!(output.validate("password").is_err());
    }
}
