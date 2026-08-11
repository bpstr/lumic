use crate::{Capability, Change, LumicError, Plan, Risk, RiskLevel};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    HostSnapshot,
    ProcessSnapshot,
    ServiceHealth,
    ApplicationHealth,
    DeploymentMarker,
    SystemEvent,
    ProviderSignal,
    Remediation,
    Notification,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperationalSignal {
    pub id: String,
    pub timestamp_unix_ms: u128,
    pub kind: SignalKind,
    pub severity: SignalSeverity,
    pub event_type: String,
    pub entity: String,
    pub entity_id: String,
    pub correlation_id: String,
    pub summary: String,
    pub evidence: Vec<String>,
    pub payload: Value,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineQuery {
    pub entity: Option<String>,
    pub entity_id: Option<String>,
    pub event_type: Option<String>,
    pub since_unix_ms: Option<u128>,
    pub until_unix_ms: Option<u128>,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IncidentReport {
    pub generated_at_unix_ms: u128,
    pub window_start_unix_ms: u128,
    pub window_end_unix_ms: u128,
    pub summary: String,
    pub affected_resources: Vec<String>,
    pub evidence: Vec<OperationalSignal>,
    pub findings: Vec<String>,
    pub recommended_actions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebhookDestination {
    pub id: String,
    pub url: String,
    pub secret_reference: String,
    pub timeout_ms: u64,
    pub max_attempts: u8,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventSubscription {
    pub id: String,
    pub destination_id: String,
    pub event_types: Vec<String>,
    pub entity: Option<String>,
    pub entity_id: Option<String>,
    pub enabled: bool,
}

impl EventSubscription {
    pub fn matches(&self, signal: &OperationalSignal) -> bool {
        self.enabled
            && (self.event_types.is_empty()
                || self.event_types.iter().any(|value| value == "*")
                || self.event_types.contains(&signal.event_type))
            && self
                .entity
                .as_ref()
                .is_none_or(|value| value == &signal.entity)
            && self
                .entity_id
                .as_ref()
                .is_none_or(|value| value == &signal.entity_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    Pending,
    Delivered,
    RetryScheduled,
    Exhausted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebhookDelivery {
    pub id: String,
    pub destination_id: String,
    pub signal_id: String,
    pub status: DeliveryStatus,
    pub attempts: u8,
    pub next_attempt_unix_ms: u128,
    pub created_at_unix_ms: u128,
    pub completed_at_unix_ms: Option<u128>,
    pub last_error: Option<String>,
    pub response_status: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AutomationAction {
    RestartService { unit: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationRule {
    pub id: String,
    pub event_type: String,
    pub entity_id: Option<String>,
    pub action: AutomationAction,
    pub cooldown_seconds: u64,
    pub max_attempts: u8,
    pub enabled: bool,
    pub last_applied_unix_ms: Option<u128>,
    pub attempt_count: u8,
}

impl AutomationRule {
    pub fn matches(&self, signal: &OperationalSignal) -> bool {
        self.enabled
            && self.event_type == signal.event_type
            && self
                .entity_id
                .as_ref()
                .is_none_or(|value| value == &signal.entity_id)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AutomationState {
    pub destinations: Vec<WebhookDestination>,
    pub subscriptions: Vec<EventSubscription>,
    pub rules: Vec<AutomationRule>,
    pub deliveries: Vec<WebhookDelivery>,
    pub signal_payloads: BTreeMap<String, OperationalSignal>,
    pub last_event_timestamp_unix_ms: u128,
    pub last_kernel_timestamp_unix_ms: u128,
    pub last_snapshot_timestamp_unix_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationRun {
    pub signal_id: String,
    pub rule_id: String,
    pub action_applied: bool,
    pub verification_succeeded: bool,
    pub message: String,
    pub impacted_resources: Vec<String>,
}

pub fn validate_id(field: &str, value: &str) -> Result<(), LumicError> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(invalid(
            field,
            "must be 1-64 ASCII letters, digits, '-' or '_'",
        ));
    }
    Ok(())
}

pub fn validate_webhook(destination: &WebhookDestination) -> Result<(), LumicError> {
    validate_id("destination_id", &destination.id)?;
    let url = url::Url::parse(&destination.url)
        .map_err(|_| invalid("url", "must be an absolute HTTPS URL"))?;
    if url.scheme() != "https" {
        return Err(invalid("url", "must use HTTPS"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(invalid("url", "embedded credentials are forbidden"));
    }
    let host = url
        .host_str()
        .ok_or_else(|| invalid("url", "must include a host"))?;
    if host.eq_ignore_ascii_case("localhost")
        || host.to_ascii_lowercase().ends_with(".localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| !public_webhook_address(address))
    {
        return Err(invalid(
            "url",
            "private and local destinations are forbidden",
        ));
    }
    if !(100..=30_000).contains(&destination.timeout_ms) {
        return Err(invalid("timeout_ms", "must be between 100 and 30000"));
    }
    if !(1..=8).contains(&destination.max_attempts) {
        return Err(invalid("max_attempts", "must be between 1 and 8"));
    }
    validate_id("secret_reference", &destination.secret_reference)
}

pub fn public_webhook_address(address: std::net::IpAddr) -> bool {
    match address {
        std::net::IpAddr::V4(address) => {
            let [a, b, c, _] = address.octets();
            !(a == 0
                || a == 10
                || a == 127
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && b == 0 && c == 0)
                || (a == 192 && b == 0 && c == 2)
                || (a == 192 && b == 168)
                || (a == 198 && (b == 18 || b == 19))
                || (a == 198 && b == 51 && c == 100)
                || (a == 203 && b == 0 && c == 113)
                || a >= 224)
        }
        std::net::IpAddr::V6(address) => {
            let segments = address.segments();
            if let Some(mapped) = address.to_ipv4_mapped() {
                return public_webhook_address(mapped.into());
            }
            (segments[0] & 0xe000) == 0x2000 && !(segments[0] == 0x2001 && segments[1] == 0x0db8)
        }
    }
}

pub fn validate_rule(rule: &AutomationRule) -> Result<(), LumicError> {
    validate_id("rule_id", &rule.id)?;
    if rule.event_type.is_empty() || rule.event_type.len() > 128 {
        return Err(invalid("event_type", "must be 1-128 characters"));
    }
    if !(5..=86_400).contains(&rule.cooldown_seconds) {
        return Err(invalid("cooldown_seconds", "must be between 5 and 86400"));
    }
    if !(1..=3).contains(&rule.max_attempts) {
        return Err(invalid("max_attempts", "must be between 1 and 3"));
    }
    match &rule.action {
        AutomationAction::RestartService { unit } => validate_systemd_unit(unit),
    }
}

pub fn automation_plan(rule: &AutomationRule, impacted_resources: Vec<String>) -> Plan {
    let action = match &rule.action {
        AutomationAction::RestartService { unit } => format!("restart {unit}"),
    };
    Plan {
        id: format!("automation-rule-{}", rule.id),
        summary: format!(
            "When {}, perform typed action {action}, then verify recovery",
            rule.event_type
        ),
        changes: vec![Change {
            capability: Capability::new("automation.rule.apply"),
            summary: format!("enable deterministic rule {}", rule.id),
            before: None,
            after: Some(action),
            reversible: true,
        }],
        risks: vec![Risk {
            level: RiskLevel::Medium,
            summary: "the target service may briefly interrupt dependent resources".into(),
            mitigation: Some(format!(
                "cooldown {} seconds, at most {} attempts, and verify systemd state after action; impacted: {}",
                rule.cooldown_seconds,
                rule.max_attempts,
                impacted_resources.join(", ")
            )),
        }],
        preconditions: vec!["the target is a validated systemd unit".into()],
        validation: vec!["systemd must report active after remediation".into()],
        recovery: vec!["disable the rule and inspect the correlated timeline".into()],
    }
}

pub fn validate_systemd_unit(unit: &str) -> Result<(), LumicError> {
    if unit.is_empty()
        || unit.len() > 128
        || unit.starts_with('-')
        || !unit.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'@' | b'-' | b':')
        })
        || !unit.ends_with(".service")
    {
        return Err(invalid("unit", "must be a validated .service unit"));
    }
    Ok(())
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
    fn webhook_and_rules_reject_unsafe_inputs() {
        let mut destination = WebhookDestination {
            id: "incident-hook".into(),
            url: "https://ops.example.test/lumic".into(),
            secret_reference: "webhook-key".into(),
            timeout_ms: 2_000,
            max_attempts: 3,
            enabled: true,
        };
        assert!(validate_webhook(&destination).is_ok());
        destination.url = "http://remote.example.test/hook".into();
        assert!(validate_webhook(&destination).is_err());
        destination.url = "https://127.0.0.1:8443/hook".into();
        assert!(validate_webhook(&destination).is_err());
        destination.url = "https://metadata.localhost.example/hook".into();
        assert!(validate_webhook(&destination).is_ok());
        destination.url = "https://metadata.localhost/hook".into();
        assert!(validate_webhook(&destination).is_err());
        destination.url = "https://user@example.test/hook".into();
        assert!(validate_webhook(&destination).is_err());

        assert!(public_webhook_address("8.8.8.8".parse().unwrap()));
        assert!(!public_webhook_address("169.254.169.254".parse().unwrap()));
        assert!(!public_webhook_address("fc00::1".parse().unwrap()));

        let mut rule = AutomationRule {
            id: "restart-demo".into(),
            event_type: "service.failed".into(),
            entity_id: Some("demo.service".into()),
            action: AutomationAction::RestartService {
                unit: "demo.service".into(),
            },
            cooldown_seconds: 60,
            max_attempts: 2,
            enabled: true,
            last_applied_unix_ms: None,
            attempt_count: 0,
        };
        assert!(validate_rule(&rule).is_ok());
        rule.action = AutomationAction::RestartService {
            unit: "--now.service".into(),
        };
        assert!(validate_rule(&rule).is_err());
    }
}
