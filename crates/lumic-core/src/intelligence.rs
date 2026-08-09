use crate::{Capability, Change, Plan, Risk, RiskLevel, operations::IncidentReport};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub source: String,
    pub observation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplicationFingerprint {
    pub application_id: String,
    pub source_root: String,
    pub framework: Option<String>,
    pub cms: Option<String>,
    pub runtime: String,
    pub confidence: Confidence,
    pub environment_files: Vec<String>,
    pub dependency_manifests: Vec<String>,
    pub worker_hints: Vec<String>,
    pub scheduler_hints: Vec<String>,
    pub configured_keys: Vec<String>,
    pub health_endpoints: Vec<String>,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigurationKey {
    pub name: String,
    pub configured: bool,
    pub sensitive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigurationInspection {
    pub application_id: String,
    pub path: String,
    pub keys: Vec<ConfigurationKey>,
    pub duplicate_keys: Vec<String>,
    pub secret_values_exposed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigurationDiff {
    pub key: String,
    pub before: String,
    pub after: String,
    pub sensitive: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyNodeKind {
    Application,
    Runtime,
    ManagedService,
    Process,
    WebService,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyNode {
    pub id: String,
    pub kind: DependencyNodeKind,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyEdge {
    pub from: String,
    pub to: String,
    pub relationship: String,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplicationDependencyGraph {
    pub application_id: String,
    pub nodes: Vec<DependencyNode>,
    pub edges: Vec<DependencyEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrationDefinition {
    pub id: String,
    pub application_framework: String,
    pub service_kind: String,
    pub description: String,
    pub configuration_keys: Vec<String>,
    pub verification_steps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrationPlan {
    pub integration_id: String,
    pub application_id: String,
    pub service_id: String,
    pub install_required: bool,
    pub configuration_path: String,
    pub configuration_diff: Vec<ConfigurationDiff>,
    pub affected_processes: Vec<String>,
    pub dependency_graph: ApplicationDependencyGraph,
    pub plan: Plan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrationApplyResult {
    pub integration_id: String,
    pub application_id: String,
    pub service_id: String,
    pub changed: bool,
    pub snapshot_id: Option<String>,
    pub configuration_diff: Vec<ConfigurationDiff>,
    pub restarted_processes: Vec<String>,
    pub verification: Vec<String>,
    pub recovery: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IncidentContext {
    pub schema: String,
    pub report: IncidentReport,
    pub affected_graph_nodes: Vec<String>,
    pub evidence_references: Vec<String>,
    pub redacted_fields: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProposedRemediation {
    RestartService {
        unit: String,
    },
    RollbackApplicationConfiguration {
        application_id: String,
        snapshot_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IncidentAnalysis {
    pub diagnosis: String,
    pub evidence_references: Vec<String>,
    pub proposed_remediations: Vec<ProposedRemediation>,
    #[serde(default = "advisory_default")]
    pub advisory: bool,
}

const fn advisory_default() -> bool {
    true
}

pub fn integration_plan(
    application_id: &str,
    service_id: &str,
    install_required: bool,
    changes: &[ConfigurationDiff],
    affected_processes: &[String],
) -> Plan {
    let mut plan_changes = vec![Change {
        capability: Capability::new("application.integrate"),
        summary: format!("connect application {application_id} to Redis service {service_id}"),
        before: Some("integration absent or incomplete".into()),
        after: Some("typed application service reference and verified dotenv configuration".into()),
        reversible: true,
    }];
    if install_required {
        plan_changes.push(Change {
            capability: Capability::new("managed-service.install"),
            summary: format!("install managed Redis service {service_id}"),
            before: Some("not installed".into()),
            after: Some("managed and health checked".into()),
            reversible: true,
        });
    }
    if !changes.is_empty() {
        plan_changes.push(Change {
            capability: Capability::new("application.environment.update"),
            summary: format!("update {} dotenv keys", changes.len()),
            before: Some("redacted configuration state captured in preview".into()),
            after: Some("redacted configuration state captured in preview".into()),
            reversible: true,
        });
    }
    if !affected_processes.is_empty() {
        plan_changes.push(Change {
            capability: Capability::new("service.restart"),
            summary: format!(
                "restart {} affected application processes",
                affected_processes.len()
            ),
            before: Some("running with previous environment".into()),
            after: Some("running with verified environment".into()),
            reversible: false,
        });
    }
    Plan {
        id: format!("integrate-laravel-redis-{application_id}-{service_id}"),
        summary: format!("Integrate Laravel application {application_id} with Redis {service_id}"),
        changes: plan_changes,
        risks: vec![Risk {
            level: RiskLevel::Medium,
            summary: "environment changes can affect cache, sessions, and queues".into(),
            mitigation: Some("snapshot the dotenv file, restart only affected workers, and verify both resources".into()),
        }],
        preconditions: vec![
            "Laravel fingerprint has high confidence".into(),
            "dotenv file has no duplicate target keys".into(),
            "managed service policy permits Redis installation when required".into(),
        ],
        validation: vec![
            "Redis reports healthy".into(),
            "the application health check succeeds".into(),
        ],
        recovery: vec![
            "restore the owned dotenv snapshot".into(),
            "restart only the affected application processes".into(),
        ],
    }
}

pub fn remediation_plan(remediation: &ProposedRemediation) -> Plan {
    match remediation {
        ProposedRemediation::RestartService { unit } => Plan {
            id: format!("incident-restart-{unit}"),
            summary: format!("Restart systemd service {unit}"),
            changes: vec![Change {
                capability: Capability::new("service.restart"),
                summary: format!("restart {unit} through the normal service capability"),
                before: None,
                after: None,
                reversible: false,
            }],
            risks: vec![Risk {
                level: RiskLevel::Medium,
                summary: "the service is briefly unavailable during restart".into(),
                mitigation: Some("inspect health and logs before approval".into()),
            }],
            preconditions: vec!["operator approval and policy authorization are required".into()],
            validation: vec!["inspect the service health after restart".into()],
            recovery: vec!["inspect logs and use the service-specific recovery workflow".into()],
        },
        ProposedRemediation::RollbackApplicationConfiguration {
            application_id,
            snapshot_id,
        } => Plan {
            id: format!("incident-config-rollback-{application_id}-{snapshot_id}"),
            summary: format!("Restore configuration snapshot {snapshot_id} for {application_id}"),
            changes: vec![Change {
                capability: Capability::new("application.environment.rollback"),
                summary: "restore an owned Lumic configuration snapshot".into(),
                before: Some("current dotenv content".into()),
                after: Some("snapshot content (values remain redacted)".into()),
                reversible: false,
            }],
            risks: vec![Risk {
                level: RiskLevel::Medium,
                summary: "restored settings may no longer match dependent services".into(),
                mitigation: Some(
                    "preview the snapshot metadata and verify the application afterward".into(),
                ),
            }],
            preconditions: vec![
                "snapshot belongs to the application and passes integrity checks".into(),
            ],
            validation: vec!["verify application health after restoration".into()],
            recovery: vec!["restore a newer owned snapshot if required".into()],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integration_plan_never_contains_configuration_values() {
        let diff = vec![ConfigurationDiff {
            key: "REDIS_PASSWORD".into(),
            before: "configured".into(),
            after: "configured".into(),
            sensitive: true,
        }];
        let plan = integration_plan("app", "redis", false, &diff, &[]);
        let json = serde_json::to_string(&plan).unwrap();
        assert!(!json.contains("password-value"));
        assert!(json.contains("redacted"));
    }

    #[test]
    fn analysis_rejects_untyped_remediation_fields() {
        let input = r#"{"diagnosis":"x","evidence_references":[],"proposed_remediations":[{"kind":"restart_service","unit":"redis.service","command":"rm -rf /"}],"advisory":true}"#;
        assert!(serde_json::from_str::<IncidentAnalysis>(input).is_err());
    }
}
