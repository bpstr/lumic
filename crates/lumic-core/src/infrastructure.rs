use crate::{
    LumicError, Result,
    application::{
        ApplicationProcess, ApplicationRuntime, ApplicationServiceReference, HealthCheck,
        RepositoryConfig,
    },
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentTier {
    Production,
    Staging,
    #[default]
    Development,
}

impl EnvironmentTier {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Production => "production",
            Self::Staging => "staging",
            Self::Development => "development",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "production" => Ok(Self::Production),
            "staging" => Ok(Self::Staging),
            "development" => Ok(Self::Development),
            _ => Err(invalid(
                "tier",
                "must be production, staging, or development",
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeRole {
    App,
    Worker,
    Database,
    Cache,
    Git,
    Media,
    Backup,
    Edge,
}

impl NodeRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Worker => "worker",
            Self::Database => "database",
            Self::Cache => "cache",
            Self::Git => "git",
            Self::Media => "media",
            Self::Backup => "backup",
            Self::Edge => "edge",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "app" => Ok(Self::App),
            "worker" => Ok(Self::Worker),
            "database" => Ok(Self::Database),
            "cache" => Ok(Self::Cache),
            "git" => Ok(Self::Git),
            "media" => Ok(Self::Media),
            "backup" => Ok(Self::Backup),
            "edge" => Ok(Self::Edge),
            _ => Err(invalid(
                "role",
                "must be app, worker, database, cache, git, media, backup, or edge",
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortableApplication {
    pub id: String,
    pub name: String,
    pub domain: String,
    pub www_alias: bool,
    pub runtime: ApplicationRuntime,
    pub repository: Option<RepositoryConfig>,
    pub environment_references: BTreeMap<String, String>,
    pub service_references: Vec<ApplicationServiceReference>,
    pub health_check: HealthCheck,
    pub processes: Vec<ApplicationProcess>,
    pub release_retention: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentBundle {
    pub schema_version: u32,
    pub id: String,
    pub tier: EnvironmentTier,
    pub source_node_id: String,
    pub application: PortableApplication,
    pub exported_at_unix_ms: u128,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentTransform {
    pub target_id: String,
    pub target_tier: EnvironmentTier,
    pub target_domain: String,
    #[serde(default)]
    pub environment_reference_overrides: BTreeMap<String, String>,
    #[serde(default)]
    pub service_id_overrides: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigurationDiff {
    pub field: String,
    pub source: Option<String>,
    pub target: Option<String>,
    pub sensitive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedRepository {
    pub id: String,
    pub path: String,
    pub default_branch: String,
    pub created_at_unix_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryMirror {
    pub id: String,
    pub source_url: String,
    pub branch: String,
    pub credential_reference: Option<String>,
    pub path: String,
    pub last_updated_at_unix_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushDeployTrigger {
    pub repository_id: String,
    pub application_id: String,
    pub branch: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeIdentity {
    pub id: String,
    pub name: String,
    pub fingerprint: String,
    pub roles: Vec<NodeRole>,
    pub created_at_unix_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeEnrollment {
    pub identity: NodeIdentity,
    pub endpoint: String,
    pub verification_key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustStatus {
    Pending,
    Trusted,
    Revoked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisteredNode {
    pub identity: NodeIdentity,
    pub endpoint: String,
    pub trust_status: TrustStatus,
    pub verification_key: String,
    pub registered_at_unix_ms: u128,
    pub last_health: Option<NodeHealth>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeHealth {
    pub healthy: bool,
    pub message: String,
    pub checked_at_unix_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceEndpoint {
    pub id: String,
    pub provider_node_id: String,
    pub provider_kind: String,
    pub provider_id: String,
    pub consumer_node_id: String,
    pub consumer_kind: String,
    pub consumer_id: String,
    pub protocol: String,
    pub host: String,
    pub port: u16,
    pub health_path: Option<String>,
    pub secret_reference: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MembershipKind {
    Worker,
    ReverseProxy,
}

impl MembershipKind {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "worker" => Ok(Self::Worker),
            "reverse_proxy" => Ok(Self::ReverseProxy),
            _ => Err(invalid("membership", "must be worker or reverse_proxy")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeMembership {
    pub id: String,
    pub kind: MembershipKind,
    pub environment_id: String,
    pub application_id: String,
    pub node_id: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentMemberStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    RolledBack,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentMember {
    pub node_id: String,
    pub application_id: String,
    pub status: DeploymentMemberStatus,
    pub healthy: Option<bool>,
    pub deployment_id: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoordinationStatus {
    Planned,
    Running,
    Succeeded,
    Failed,
    RolledBack,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoordinatedDeployment {
    pub id: String,
    pub environment_id: String,
    pub members: Vec<DeploymentMember>,
    pub status: CoordinationStatus,
    pub failure_boundary: String,
    pub created_at_unix_ms: u128,
    pub finished_at_unix_ms: Option<u128>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteOperation {
    pub kind: String,
    pub resource_id: String,
    pub arguments: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedRemoteRequest {
    pub origin_node_id: String,
    pub target_node_id: String,
    pub nonce: String,
    pub expires_at_unix_ms: u128,
    pub operation: RemoteOperation,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InfrastructureReadModel {
    pub local_node: Option<NodeIdentity>,
    pub repositories: Vec<HostedRepository>,
    pub mirrors: Vec<RepositoryMirror>,
    pub triggers: Vec<PushDeployTrigger>,
    pub environments: Vec<EnvironmentBundle>,
    pub nodes: Vec<RegisteredNode>,
    pub endpoints: Vec<ResourceEndpoint>,
    pub memberships: Vec<NodeMembership>,
    pub deployments: Vec<CoordinatedDeployment>,
}

pub fn validate_endpoint(value: &str) -> Result<()> {
    let supported = value.starts_with("https://")
        || value.starts_with("http://127.0.0.1:")
        || value.starts_with("http://[::1]:");
    let authority = value
        .split_once("://")
        .map(|(_, rest)| rest.split('/').next().unwrap_or(rest));
    if !supported
        || value.contains(['\n', '\r'])
        || authority.is_none_or(|part| part.is_empty() || part.contains('@'))
    {
        return Err(invalid(
            "endpoint",
            "must be HTTPS without credentials, or an explicit loopback HTTP endpoint",
        ));
    }
    Ok(())
}

pub fn validate_secret_reference(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.')
        })
    {
        Err(invalid(
            "secret_reference",
            "must be a lowercase safe identifier",
        ))
    } else {
        Ok(())
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
    fn roles_and_environment_tiers_are_closed_sets() {
        assert_eq!(NodeRole::parse("edge").unwrap(), NodeRole::Edge);
        assert!(NodeRole::parse("scheduler").is_err());
        assert_eq!(
            EnvironmentTier::parse("staging").unwrap(),
            EnvironmentTier::Staging
        );
        assert!(EnvironmentTier::parse("preview").is_err());
    }

    #[test]
    fn remote_endpoints_reject_credentials_and_plaintext_network_access() {
        assert!(validate_endpoint("https://node.example.test/mcp").is_ok());
        assert!(validate_endpoint("http://127.0.0.1:8080").is_ok());
        assert!(validate_endpoint("http://node.example.test").is_err());
        assert!(validate_endpoint("https://token@node.example.test").is_err());
    }
}
