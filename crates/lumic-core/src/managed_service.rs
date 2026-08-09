use crate::{Capability, Change, LumicError, Plan, Result, Risk, RiskLevel};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedServiceKind {
    Postgresql,
    Redis,
}

impl ManagedServiceKind {
    pub const fn id(self) -> &'static str {
        match self {
            Self::Postgresql => "postgresql",
            Self::Redis => "redis",
        }
    }
}

impl std::fmt::Display for ManagedServiceKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.id())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesiredServiceState {
    Running,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceHealth {
    Unknown,
    Healthy,
    Degraded,
    Unhealthy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceConfiguration {
    pub bind_address: String,
    pub port: u16,
    #[serde(default)]
    pub settings: BTreeMap<String, String>,
}

impl ServiceConfiguration {
    pub fn defaults(kind: ManagedServiceKind) -> Self {
        Self {
            bind_address: "127.0.0.1".into(),
            port: match kind {
                ManagedServiceKind::Postgresql => 5432,
                ManagedServiceKind::Redis => 6379,
            },
            settings: BTreeMap::new(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.port == 0 {
            return Err(invalid("port", "must be between 1 and 65535"));
        }
        if !matches!(self.bind_address.as_str(), "127.0.0.1" | "::1") {
            return Err(invalid(
                "bind_address",
                "reference services are loopback-only; remote exposure needs a later explicit policy",
            ));
        }
        for (key, value) in &self.settings {
            if !valid_setting_key(key)
                || value.is_empty()
                || value.len() > 256
                || value.bytes().any(|byte| byte.is_ascii_control())
            {
                return Err(invalid(
                    "settings",
                    "keys and values must be bounded single-line configuration data",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceDependency {
    pub service_id: String,
    pub required: bool,
    pub purpose: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedService {
    pub id: String,
    pub name: String,
    pub kind: ManagedServiceKind,
    pub package: String,
    pub systemd_unit: String,
    pub desired_state: DesiredServiceState,
    pub configuration: ServiceConfiguration,
    #[serde(default)]
    pub secret_references: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<ServiceDependency>,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServicePaths {
    pub systemd_unit: String,
    pub configuration_paths: Vec<String>,
    pub data_path: String,
    pub log_source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedServiceStatus {
    pub service: ManagedService,
    pub detected: bool,
    pub version: Option<String>,
    pub active_state: String,
    pub sub_state: String,
    pub enabled: bool,
    pub health: ServiceHealth,
    pub health_message: String,
    pub paths: ServicePaths,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedServiceMutation {
    pub service: ManagedService,
    pub action: String,
    pub changed: bool,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Database {
    pub id: String,
    pub service_id: String,
    pub name: String,
    pub owner: Option<String>,
    pub created_at_unix_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseUser {
    pub id: String,
    pub service_id: String,
    pub name: String,
    pub secret_reference: String,
    #[serde(default)]
    pub databases: Vec<String>,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupStatus {
    Completed,
    Failed,
    Restored,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceBackup {
    pub id: String,
    pub service_id: String,
    pub database: Option<String>,
    pub path: String,
    pub size_bytes: u64,
    #[serde(default)]
    pub checksum_sha256: Option<String>,
    pub status: BackupStatus,
    pub created_at_unix_ms: u128,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupVerification {
    pub backup_id: String,
    pub verified_at_unix_ms: u128,
    pub exists: bool,
    pub size_matches: bool,
    pub checksum_matches: Option<bool>,
    pub format_valid: bool,
    pub checksum_sha256: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedServiceState {
    #[serde(default)]
    pub services: Vec<ManagedService>,
    #[serde(default)]
    pub databases: Vec<Database>,
    #[serde(default)]
    pub users: Vec<DatabaseUser>,
    #[serde(default)]
    pub backups: Vec<ServiceBackup>,
}

pub fn validate_resource_id(field: &str, value: &str) -> Result<()> {
    let valid = !value.is_empty()
        && value.len() <= 63
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    if valid {
        Ok(())
    } else {
        Err(invalid(field, "must be a lowercase resource id"))
    }
}

pub fn validate_database_identifier(field: &str, value: &str) -> Result<()> {
    let valid = !value.is_empty()
        && value.len() <= 63
        && (value.as_bytes()[0].is_ascii_lowercase() || value.as_bytes()[0] == b'_')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
    if valid {
        Ok(())
    } else {
        Err(invalid(
            field,
            "must be a lowercase SQL identifier containing letters, digits, or underscores",
        ))
    }
}

pub fn install_plan(id: &str, kind: ManagedServiceKind, already_managed: bool) -> Plan {
    Plan {
        id: format!("service-install-{id}"),
        summary: format!("Install and manage {kind} service '{id}'"),
        changes: vec![Change {
            capability: Capability::new("managed_service.install"),
            summary: format!(
                "{} native package, systemd lifecycle, and Lumic resource state",
                if already_managed {
                    "Reconcile"
                } else {
                    "Install"
                }
            ),
            before: Some(if already_managed { "managed" } else { "absent" }.into()),
            after: Some("installed, enabled, healthy, and managed".into()),
            reversible: true,
        }],
        risks: vec![Risk {
            level: RiskLevel::Medium,
            summary: "Package installation and service startup change the host".into(),
            mitigation: Some(
                "Native package state and service events are audited; uninstall is explicit".into(),
            ),
        }],
        preconditions: vec!["Debian or Ubuntu host with apt and systemd".into()],
        validation: vec!["Package detection, systemd state, and provider health probe".into()],
        recovery: vec!["Stop/disable the unit or run managed service removal".into()],
    }
}

fn valid_setting_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
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
    fn reference_configurations_are_loopback_only_and_identifiers_are_safe() {
        for kind in [ManagedServiceKind::Postgresql, ManagedServiceKind::Redis] {
            assert!(ServiceConfiguration::defaults(kind).validate().is_ok());
        }
        let mut exposed = ServiceConfiguration::defaults(ManagedServiceKind::Redis);
        exposed.bind_address = "0.0.0.0".into();
        assert!(exposed.validate().is_err());
        assert!(validate_database_identifier("database", "app_prod").is_ok());
        assert!(validate_database_identifier("database", "app;drop").is_err());
    }

    #[test]
    fn old_application_records_accept_missing_service_references() {
        let value = serde_json::json!({
            "id":"app","name":"App","domain":"app.test","www_alias":false,
            "root":"public","runtime":"static","repository":null,
            "environment_references":{},"health_check":HealthCheckFixture::value(),
            "processes":[],"web_configured":false,"release_retention":5,
            "health_status":"unknown","created_at_unix_ms":1,"updated_at_unix_ms":1
        });
        let application: crate::application::Application = serde_json::from_value(value).unwrap();
        assert!(application.service_references.is_empty());
    }

    struct HealthCheckFixture;
    impl HealthCheckFixture {
        fn value() -> serde_json::Value {
            serde_json::json!({
                "enabled":false,"path":"/","port":80,"expected_status_min":200,
                "expected_status_max":399,"timeout_seconds":10
            })
        }
    }
}
