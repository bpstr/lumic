//! Catalog-backed service instances with ownership and drift semantics.

use crate::{
    LumicError, Result,
    catalog::Configuration,
    resource::{ResourceOutputs, ResourceRef, validate_resource_id},
};
use serde::{Deserialize, Serialize};

/// Whether Lumic may mutate a discovered service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceOwnership {
    External,
    Lumic,
}

/// Current relationship between observed host state and Lumic state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagementStatus {
    Discovered,
    Managed,
    Drifted,
    Conflicted,
}

/// Desired service lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesiredServiceState {
    Running,
    Stopped,
}

/// A persisted instance of a catalog service definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceInstance {
    pub id: String,
    pub definition_id: String,
    pub definition_version: u32,
    pub display_name: String,
    pub ownership: ResourceOwnership,
    pub management_status: ManagementStatus,
    pub desired_state: DesiredServiceState,
    #[serde(default)]
    pub configuration: Configuration,
    #[serde(default)]
    pub outputs: ResourceOutputs,
    #[serde(default)]
    pub platform_metadata: Configuration,
    pub installed_version: Option<String>,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
}

impl ServiceInstance {
    pub fn validate(&self) -> Result<()> {
        validate_resource_id("service.id", &self.id)?;
        validate_resource_id("service.definition_id", &self.definition_id)?;
        if self.definition_version == 0 {
            return Err(invalid("service.definition_version", "must be positive"));
        }
        if self.display_name.trim().is_empty() || self.display_name.len() > 128 {
            return Err(invalid(
                "service.display_name",
                "must contain between 1 and 128 characters",
            ));
        }
        if self.ownership == ResourceOwnership::External
            && self.management_status == ManagementStatus::Managed
        {
            return Err(invalid(
                "service.management_status",
                "an external service cannot be marked managed",
            ));
        }
        if self.created_at_unix_ms > self.updated_at_unix_ms {
            return Err(invalid(
                "service.updated_at_unix_ms",
                "must not precede creation",
            ));
        }
        for (name, output) in &self.outputs {
            output.validate(name)?;
        }
        Ok(())
    }

    pub fn resource_ref(&self) -> ResourceRef {
        ResourceRef {
            kind: crate::resource::ResourceKind::ManagedService,
            id: self.id.clone(),
        }
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
    fn external_instances_cannot_claim_managed_status() {
        let instance = ServiceInstance {
            id: "redis.main".into(),
            definition_id: "redis".into(),
            definition_version: 1,
            display_name: "Redis main".into(),
            ownership: ResourceOwnership::External,
            management_status: ManagementStatus::Managed,
            desired_state: DesiredServiceState::Running,
            configuration: Configuration::new(),
            outputs: ResourceOutputs::new(),
            platform_metadata: Configuration::new(),
            installed_version: None,
            created_at_unix_ms: 1,
            updated_at_unix_ms: 1,
        };
        assert!(instance.validate().is_err());
    }
}
