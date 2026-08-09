use crate::{
    ProcessRunner, ProcessSpec,
    application::ApplicationService,
    atomic_file::write_atomic,
    audit_store::AuditStore,
    event_store::EventStore,
    hex_encode, jsonl_store,
    managed_service::ManagedServiceManager,
    secret_store::SecretStore,
    systemd::{ServiceAction, SystemdServiceManager},
};
use lumic_core::{
    LumicError, OperationContext, OperationInterface, Plan, Result,
    events::{AuditRecord, Event},
    operations::{
        AutomationAction, AutomationRule, AutomationRun, AutomationState, DeliveryStatus,
        EventSubscription, IncidentReport, OperationalSignal, SignalKind, SignalSeverity,
        TimelineQuery, WebhookDelivery, WebhookDestination, automation_plan, validate_id,
        validate_rule, validate_webhook,
    },
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MAX_TIMELINE_READ: usize = 10_000;
const MAX_DELIVERY_HISTORY: usize = 1_000;
static ID_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct OperationsService {
    state_dir: PathBuf,
    apps_root: PathBuf,
    state_path: PathBuf,
    timeline_path: PathBuf,
}

impl OperationsService {
    pub fn at_state_dir(state_dir: impl AsRef<Path>) -> Self {
        let state_dir = state_dir.as_ref().to_path_buf();
        let apps_root = std::env::var_os("LUMIC_APPS_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| state_dir.join("apps"));
        Self::new(state_dir, apps_root)
    }

    pub fn new(state_dir: impl AsRef<Path>, apps_root: impl Into<PathBuf>) -> Self {
        let state_dir = state_dir.as_ref().to_path_buf();
        let operations_dir = state_dir.join("operations");
        Self {
            state_dir,
            apps_root: apps_root.into(),
            state_path: operations_dir.join("state.json"),
            timeline_path: operations_dir.join("timeline.jsonl"),
        }
    }

    pub fn plan_destination(&self, destination: &WebhookDestination) -> Result<Plan> {
        validate_webhook(destination)?;
        if !SecretStore::at_state_dir(&self.state_dir).exists(&destination.secret_reference)? {
            return Err(invalid(
                "secret_reference",
                "secret does not exist; create it with the secret store first",
            ));
        }
        Ok(configuration_plan(
            "webhook",
            &destination.id,
            format!("deliver signed events to {}", destination.url),
        ))
    }

    pub fn apply_destination(
        &self,
        destination: WebhookDestination,
        context: &OperationContext,
    ) -> Result<WebhookDestination> {
        self.plan_destination(&destination)?;
        let mut state = self.load_state()?;
        let before = state
            .destinations
            .iter()
            .find(|value| value.id == destination.id)
            .cloned();
        upsert(&mut state.destinations, destination.clone(), |value| {
            &value.id
        });
        self.save_configuration_state(&state)?;
        self.record_configuration(
            context,
            ("operations.webhook.configure", "configure"),
            "webhook_destination",
            &destination.id,
            serde_json::to_value(before).map_err(json_error)?,
            serde_json::to_value(&destination).map_err(json_error)?,
        )?;
        Ok(destination)
    }

    pub fn apply_subscription(
        &self,
        subscription: EventSubscription,
        context: &OperationContext,
    ) -> Result<EventSubscription> {
        validate_id("subscription_id", &subscription.id)?;
        validate_id("destination_id", &subscription.destination_id)?;
        if subscription
            .event_types
            .iter()
            .any(|value| value.is_empty() || value.len() > 128 || value.contains(['\n', '\r']))
        {
            return Err(invalid(
                "event_types",
                "each event type must be 1-128 characters without control characters",
            ));
        }
        let mut state = self.load_state()?;
        if !state
            .destinations
            .iter()
            .any(|value| value.id == subscription.destination_id)
        {
            return Err(invalid("destination_id", "destination does not exist"));
        }
        let before = state
            .subscriptions
            .iter()
            .find(|value| value.id == subscription.id)
            .cloned();
        upsert(&mut state.subscriptions, subscription.clone(), |value| {
            &value.id
        });
        self.save_configuration_state(&state)?;
        self.record_configuration(
            context,
            ("operations.subscription.configure", "configure"),
            "event_subscription",
            &subscription.id,
            serde_json::to_value(before).map_err(json_error)?,
            serde_json::to_value(&subscription).map_err(json_error)?,
        )?;
        Ok(subscription)
    }

    pub fn plan_rule(&self, rule: &AutomationRule) -> Result<Plan> {
        validate_rule(rule)?;
        Ok(automation_plan(rule, impacted_resources(rule)))
    }

    pub fn apply_rule(
        &self,
        rule: AutomationRule,
        context: &OperationContext,
    ) -> Result<AutomationRule> {
        self.plan_rule(&rule)?;
        let mut state = self.load_state()?;
        let before = state
            .rules
            .iter()
            .find(|value| value.id == rule.id)
            .cloned();
        upsert(&mut state.rules, rule.clone(), |value| &value.id);
        self.save_configuration_state(&state)?;
        self.record_configuration(
            context,
            ("operations.rule.configure", "configure"),
            "automation_rule",
            &rule.id,
            serde_json::to_value(before).map_err(json_error)?,
            serde_json::to_value(&rule).map_err(json_error)?,
        )?;
        Ok(rule)
    }

    pub fn rollback_configuration(&self, context: &OperationContext) -> Result<()> {
        let backup = self.state_path.with_file_name(".state.json.lumic-backup");
        if !backup.is_file() || backup.parent() != self.state_path.parent() {
            return Err(invalid(
                "backup",
                "operations configuration snapshot is unavailable",
            ));
        }
        let previous: AutomationState =
            serde_json::from_slice(&fs::read(&backup).map_err(io_error)?).map_err(json_error)?;
        let mut current = self.load_state()?;
        let before = configuration_value(&current);
        current.destinations = previous.destinations;
        current.subscriptions = previous.subscriptions;
        current.rules = previous.rules;
        self.save_configuration_state(&current)?;
        self.record_configuration(
            context,
            ("operations.configuration.rollback", "rollback"),
            "operations_configuration",
            "local",
            before,
            configuration_value(&current),
        )
    }

    pub fn destinations(&self) -> Result<Vec<WebhookDestination>> {
        Ok(self.load_state()?.destinations)
    }

    pub fn subscriptions(&self) -> Result<Vec<EventSubscription>> {
        Ok(self.load_state()?.subscriptions)
    }

    pub fn rules(&self) -> Result<Vec<AutomationRule>> {
        Ok(self.load_state()?.rules)
    }

    pub fn deliveries(&self, limit: usize) -> Result<Vec<WebhookDelivery>> {
        let mut deliveries = self.load_state()?.deliveries;
        deliveries.reverse();
        deliveries.truncate(limit.clamp(1, MAX_DELIVERY_HISTORY));
        Ok(deliveries)
    }

    pub async fn record_provider_signal(
        &self,
        event_type: &str,
        entity: &str,
        entity_id: &str,
        severity: SignalSeverity,
        summary: &str,
        payload: Value,
    ) -> Result<(OperationalSignal, Vec<AutomationRun>)> {
        validate_signal_fields(event_type, entity, entity_id, summary)?;
        if serde_json::to_vec(&payload).map_err(json_error)?.len() > 64 * 1024 {
            return Err(invalid("payload", "must serialize to at most 64 KiB"));
        }
        let signal = OperationalSignal {
            id: unique_id("signal"),
            timestamp_unix_ms: now_ms(),
            kind: SignalKind::ProviderSignal,
            severity,
            event_type: event_type.into(),
            entity: entity.into(),
            entity_id: entity_id.into(),
            correlation_id: unique_id("provider"),
            summary: summary.into(),
            evidence: vec!["reported through the typed provider signal hook".into()],
            payload,
        };
        let runs = self.record_and_automate(signal.clone()).await?;
        Ok((signal, runs))
    }

    pub async fn capture_events(&self) -> Result<Vec<OperationalSignal>> {
        let events = EventStore::at_state_dir(&self.state_dir).list(MAX_TIMELINE_READ)?;
        let mut state = self.load_state()?;
        let mut captured = Vec::new();
        for event in events.into_iter().rev() {
            if event.timestamp_unix_ms <= state.last_event_timestamp_unix_ms {
                continue;
            }
            state.last_event_timestamp_unix_ms = state
                .last_event_timestamp_unix_ms
                .max(event.timestamp_unix_ms);
            let signal = signal_from_event(event);
            self.append_timeline(&signal)?;
            enqueue_subscriptions(&mut state, &signal);
            captured.push(signal);
        }
        self.save_state(&state)?;
        for signal in &captured {
            let _ = self.apply_matching_rules(signal).await?;
        }
        Ok(captured)
    }

    pub fn timeline(&self, query: &TimelineQuery) -> Result<Vec<OperationalSignal>> {
        let mut signals = self.read_timeline()?;
        signals.retain(|signal| {
            query
                .entity
                .as_ref()
                .is_none_or(|value| value == &signal.entity)
                && query
                    .entity_id
                    .as_ref()
                    .is_none_or(|value| value == &signal.entity_id)
                && query
                    .event_type
                    .as_ref()
                    .is_none_or(|value| value == &signal.event_type)
                && query
                    .since_unix_ms
                    .is_none_or(|value| signal.timestamp_unix_ms >= value)
                && query
                    .until_unix_ms
                    .is_none_or(|value| signal.timestamp_unix_ms <= value)
        });
        signals.truncate(query.limit.clamp(1, MAX_TIMELINE_READ));
        Ok(signals)
    }

    pub fn incident(&self, query: &TimelineQuery) -> Result<IncidentReport> {
        let evidence = self.timeline(query)?;
        let end = query.until_unix_ms.unwrap_or_else(now_ms);
        let start = query.since_unix_ms.unwrap_or_else(|| {
            evidence
                .last()
                .map_or(end.saturating_sub(3_600_000), |value| {
                    value.timestamp_unix_ms
                })
        });
        let mut resources = BTreeSet::new();
        let mut findings = Vec::new();
        for signal in &evidence {
            resources.insert(format!("{}:{}", signal.entity, signal.entity_id));
            if matches!(
                signal.severity,
                SignalSeverity::Error | SignalSeverity::Critical
            ) {
                findings.push(format!(
                    "{}: {} ({})",
                    signal.event_type, signal.summary, signal.timestamp_unix_ms
                ));
            }
        }
        findings.dedup();
        let recommended_actions = if findings.is_empty() {
            vec!["No failure evidence was found in this window; widen the query if needed.".into()]
        } else {
            vec![
                "Inspect the correlated evidence before applying another mutation.".into(),
                "Use a typed Lumic plan/apply operation for recovery.".into(),
            ]
        };
        Ok(IncidentReport {
            generated_at_unix_ms: now_ms(),
            window_start_unix_ms: start,
            window_end_unix_ms: end,
            summary: format!(
                "{} signals across {} affected resources; {} failure findings",
                evidence.len(),
                resources.len(),
                findings.len()
            ),
            affected_resources: resources.into_iter().collect(),
            evidence,
            findings,
            recommended_actions,
        })
    }

    pub async fn run_once(&self) -> Result<Value> {
        let captured = self.capture_events().await?;
        let snapshots = self.capture_snapshot().await?;
        let delivered = self.deliver_due().await?;
        Ok(json!({
            "captured": captured.len(),
            "snapshots": snapshots.len(),
            "deliveries_processed": delivered
        }))
    }

    pub async fn capture_snapshot(&self) -> Result<Vec<OperationalSignal>> {
        self.capture_snapshot_inner(false).await
    }

    /// Capture current host, process, service, application and system evidence immediately.
    ///
    /// Unlike the daemon-oriented snapshot path, this bypasses the five-minute sampling gate so
    /// an operator can deliberately observe and correlate a just-induced failure.
    pub async fn observe_now(&self) -> Result<Vec<OperationalSignal>> {
        self.capture_snapshot_inner(true).await
    }

    async fn capture_snapshot_inner(&self, force: bool) -> Result<Vec<OperationalSignal>> {
        let mut state = self.load_state()?;
        let now = now_ms();
        if !force && now.saturating_sub(state.last_snapshot_timestamp_unix_ms) < 300_000 {
            return Ok(Vec::new());
        }
        state.last_snapshot_timestamp_unix_ms = now;
        self.save_state(&state)?;

        let mut signals = Vec::new();
        let report = crate::diagnostics::diagnose_host().await?;
        signals.push(OperationalSignal {
            id: unique_id("host"),
            timestamp_unix_ms: now,
            kind: SignalKind::HostSnapshot,
            severity: if report.findings.is_empty() {
                SignalSeverity::Info
            } else {
                SignalSeverity::Warning
            },
            event_type: "host.snapshot".into(),
            entity: "host".into(),
            entity_id: report.host.hostname.clone(),
            correlation_id: unique_id("snapshot"),
            summary: format!(
                "load {:.2}; {} bytes memory available; {} findings",
                report.load.one_minute,
                report.host.memory.available_bytes,
                report.findings.len()
            ),
            evidence: report
                .findings
                .iter()
                .map(|value| value.evidence.clone())
                .collect(),
            payload: serde_json::to_value(&report).map_err(json_error)?,
        });
        signals.push(OperationalSignal {
            id: unique_id("processes"),
            timestamp_unix_ms: now,
            kind: SignalKind::ProcessSnapshot,
            severity: SignalSeverity::Info,
            event_type: "process.snapshot".into(),
            entity: "host".into(),
            entity_id: report.host.hostname.clone(),
            correlation_id: signals[0].correlation_id.clone(),
            summary: format!(
                "{} highest-memory processes captured",
                report.top_processes.len()
            ),
            evidence: vec!["read from procfs without mutating processes".into()],
            payload: serde_json::to_value(&report.top_processes).map_err(json_error)?,
        });
        for unit in &report.failed_services {
            signals.push(OperationalSignal {
                id: unique_id("service"),
                timestamp_unix_ms: now,
                kind: SignalKind::ServiceHealth,
                severity: SignalSeverity::Error,
                event_type: "service.failed".into(),
                entity: "systemd_service".into(),
                entity_id: unit.clone(),
                correlation_id: signals[0].correlation_id.clone(),
                summary: format!("systemd reports {unit} failed"),
                evidence: vec!["systemctl --failed reports the unit".into()],
                payload: json!({"unit": unit, "active_state": "failed"}),
            });
        }
        for application in ApplicationService::new(&self.state_dir, &self.apps_root).list()? {
            let healthy = application.health_status == "healthy";
            signals.push(OperationalSignal {
                id: unique_id("application"),
                timestamp_unix_ms: now,
                kind: SignalKind::ApplicationHealth,
                severity: if healthy {
                    SignalSeverity::Info
                } else {
                    SignalSeverity::Warning
                },
                event_type: if healthy {
                    "application.healthy"
                } else {
                    "application.unhealthy"
                }
                .into(),
                entity: "application".into(),
                entity_id: application.id.clone(),
                correlation_id: signals[0].correlation_id.clone(),
                summary: format!("application health is {}", application.health_status),
                evidence: vec!["read from the Lumic application health state".into()],
                payload: serde_json::to_value(application).map_err(json_error)?,
            });
        }
        let managed = ManagedServiceManager::at_state_dir(&self.state_dir);
        for service in managed.list()? {
            match managed.inspect(&service.id).await {
                Ok(status) => {
                    let healthy = status.active_state == "active"
                        && status.health == lumic_core::managed_service::ServiceHealth::Healthy;
                    signals.push(OperationalSignal {
                        id: unique_id("managed-service"),
                        timestamp_unix_ms: now,
                        kind: SignalKind::ServiceHealth,
                        severity: if healthy {
                            SignalSeverity::Info
                        } else {
                            SignalSeverity::Error
                        },
                        event_type: if healthy {
                            "managed_service.healthy"
                        } else {
                            "managed_service.failed"
                        }
                        .into(),
                        entity: "managed_service".into(),
                        entity_id: service.id,
                        correlation_id: signals[0].correlation_id.clone(),
                        summary: status.health_message.clone(),
                        evidence: vec![format!("systemd active state is {}", status.active_state)],
                        payload: serde_json::to_value(status).map_err(json_error)?,
                    });
                }
                Err(error) => signals.push(OperationalSignal {
                    id: unique_id("managed-service"),
                    timestamp_unix_ms: now,
                    kind: SignalKind::ServiceHealth,
                    severity: SignalSeverity::Error,
                    event_type: "managed_service.inspection_failed".into(),
                    entity: "managed_service".into(),
                    entity_id: service.id,
                    correlation_id: signals[0].correlation_id.clone(),
                    summary: "managed service inspection failed".into(),
                    evidence: vec![error.to_string()],
                    payload: json!({}),
                }),
            }
        }
        signals.extend(self.capture_kernel_events().await?);
        for signal in &signals {
            self.append_timeline(signal)?;
            let mut state = self.load_state()?;
            enqueue_subscriptions(&mut state, signal);
            self.save_state(&state)?;
        }
        for signal in &signals {
            let _ = self.apply_matching_rules(signal).await?;
        }
        Ok(signals)
    }

    async fn capture_kernel_events(&self) -> Result<Vec<OperationalSignal>> {
        let mut state = self.load_state()?;
        let since_seconds = state.last_kernel_timestamp_unix_ms / 1_000;
        let since = format!("@{}", since_seconds.max(now_ms() / 1_000 - 300));
        let output = ProcessRunner
            .run(&ProcessSpec::new("journalctl").args([
                "--dmesg",
                "--output",
                "short-unix",
                "--since",
                &since,
                "--no-pager",
            ]))
            .await;
        let Ok(output) = output else {
            return Ok(Vec::new());
        };
        if !output.success() {
            return Ok(Vec::new());
        }
        let mut signals = Vec::new();
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let lower = line.to_ascii_lowercase();
            if ![
                "out of memory",
                "oom-kill",
                "killed process",
                "kernel panic",
                "i/o error",
            ]
            .iter()
            .any(|needle| lower.contains(needle))
            {
                continue;
            }
            let timestamp = line
                .split_whitespace()
                .next()
                .and_then(|value| value.parse::<f64>().ok())
                .map_or_else(now_ms, |value| (value * 1_000.0) as u128);
            if timestamp <= state.last_kernel_timestamp_unix_ms {
                continue;
            }
            state.last_kernel_timestamp_unix_ms = timestamp;
            signals.push(OperationalSignal {
                id: unique_id("kernel"),
                timestamp_unix_ms: timestamp,
                kind: SignalKind::SystemEvent,
                severity: if lower.contains("kernel panic") {
                    SignalSeverity::Critical
                } else {
                    SignalSeverity::Error
                },
                event_type: if lower.contains("oom")
                    || lower.contains("out of memory")
                    || lower.contains("killed process")
                {
                    "system.oom"
                } else {
                    "system.kernel_error"
                }
                .into(),
                entity: "kernel".into(),
                entity_id: "local".into(),
                correlation_id: unique_id("kernel"),
                summary: truncate(line, 512),
                evidence: vec!["read from the kernel journal".into()],
                payload: json!({"journal_line": truncate(line, 2_048)}),
            });
        }
        self.save_state(&state)?;
        Ok(signals)
    }

    async fn record_and_automate(&self, signal: OperationalSignal) -> Result<Vec<AutomationRun>> {
        self.append_timeline(&signal)?;
        let mut state = self.load_state()?;
        enqueue_subscriptions(&mut state, &signal);
        self.save_state(&state)?;
        self.apply_matching_rules(&signal).await
    }

    async fn apply_matching_rules(&self, signal: &OperationalSignal) -> Result<Vec<AutomationRun>> {
        let state = self.load_state()?;
        let matching = state
            .rules
            .iter()
            .filter(|rule| rule.matches(signal))
            .cloned()
            .collect::<Vec<_>>();
        let mut runs = Vec::new();
        for rule in matching {
            if let Some(run) = self.apply_rule_action(signal, &rule).await? {
                runs.push(run);
            }
        }
        Ok(runs)
    }

    async fn apply_rule_action(
        &self,
        signal: &OperationalSignal,
        configured_rule: &AutomationRule,
    ) -> Result<Option<AutomationRun>> {
        let mut state = self.load_state()?;
        let Some(index) = state
            .rules
            .iter()
            .position(|value| value.id == configured_rule.id)
        else {
            return Ok(None);
        };
        let rule = &state.rules[index];
        let cooldown_ms = u128::from(rule.cooldown_seconds) * 1_000;
        if rule.attempt_count >= rule.max_attempts
            || rule
                .last_applied_unix_ms
                .is_some_and(|last| now_ms().saturating_sub(last) < cooldown_ms)
        {
            return Ok(None);
        }
        state.rules[index].last_applied_unix_ms = Some(now_ms());
        state.rules[index].attempt_count = state.rules[index].attempt_count.saturating_add(1);
        self.save_state(&state)?;

        let (action_applied, verification_succeeded, message) = match &configured_rule.action {
            AutomationAction::RestartService { unit } => {
                let context = OperationContext {
                    actor: format!("automation:{}", configured_rule.id),
                    interface: OperationInterface::Internal,
                    correlation_id: signal.correlation_id.clone(),
                    dry_run: false,
                    approved: true,
                };
                match SystemdServiceManager::at_state_dir(&self.state_dir)
                    .apply(unit, ServiceAction::Restart, &context)
                    .await
                {
                    Ok(mutation) => {
                        let verified = mutation.after.active_state == "active";
                        (
                            true,
                            verified,
                            format!("restart completed; active={verified}"),
                        )
                    }
                    Err(error) => (false, false, error.to_string()),
                }
            }
        };
        if verification_succeeded {
            let mut state = self.load_state()?;
            if let Some(rule) = state
                .rules
                .iter_mut()
                .find(|value| value.id == configured_rule.id)
            {
                rule.attempt_count = 0;
            }
            self.save_state(&state)?;
        }
        let run = AutomationRun {
            signal_id: signal.id.clone(),
            rule_id: configured_rule.id.clone(),
            action_applied,
            verification_succeeded,
            message: message.clone(),
            impacted_resources: impacted_resources(configured_rule),
        };
        let remediation = OperationalSignal {
            id: unique_id("remediation"),
            timestamp_unix_ms: now_ms(),
            kind: SignalKind::Remediation,
            severity: if verification_succeeded {
                SignalSeverity::Info
            } else {
                SignalSeverity::Error
            },
            event_type: if verification_succeeded {
                "automation.recovered".into()
            } else {
                "automation.failed".into()
            },
            entity: "automation_rule".into(),
            entity_id: configured_rule.id.clone(),
            correlation_id: signal.correlation_id.clone(),
            summary: message,
            evidence: vec![format!("trigger signal {}", signal.id)],
            payload: serde_json::to_value(&run).map_err(json_error)?,
        };
        self.append_timeline(&remediation)?;
        let mut state = self.load_state()?;
        enqueue_subscriptions(&mut state, &remediation);
        self.save_state(&state)?;
        Ok(Some(run))
    }

    async fn deliver_due(&self) -> Result<usize> {
        let snapshot = self.load_state()?;
        let due = snapshot
            .deliveries
            .iter()
            .filter(|delivery| {
                matches!(
                    delivery.status,
                    DeliveryStatus::Pending | DeliveryStatus::RetryScheduled
                ) && delivery.next_attempt_unix_ms <= now_ms()
            })
            .map(|delivery| delivery.id.clone())
            .collect::<Vec<_>>();
        let mut processed = 0;
        for delivery_id in due {
            self.deliver(&delivery_id).await?;
            processed += 1;
        }
        Ok(processed)
    }

    async fn deliver(&self, delivery_id: &str) -> Result<()> {
        let state = self.load_state()?;
        let delivery = state
            .deliveries
            .iter()
            .find(|value| value.id == delivery_id)
            .cloned()
            .ok_or_else(|| invalid("delivery_id", "delivery does not exist"))?;
        let destination = state
            .destinations
            .iter()
            .find(|value| value.id == delivery.destination_id && value.enabled)
            .cloned()
            .ok_or_else(|| invalid("destination_id", "destination is missing or disabled"))?;
        let signal = state
            .signal_payloads
            .get(&delivery.signal_id)
            .cloned()
            .ok_or_else(|| invalid("signal_id", "signal payload is unavailable"))?;
        let body = serde_json::to_vec(&json!({
            "schema": "lumic.webhook.v1",
            "delivery_id": delivery.id,
            "signal": signal,
        }))
        .map_err(json_error)?;
        if body.len() > 256 * 1024 {
            return self.record_delivery_result(
                delivery_id,
                false,
                None,
                Some("structured webhook payload exceeds 256 KiB".into()),
            );
        }
        let secret =
            SecretStore::at_state_dir(&self.state_dir).read(&destination.secret_reference)?;
        let signature = hmac_sha256_hex(&secret, &body);
        let timeout_seconds = destination.timeout_ms.div_ceil(1_000).to_string();
        let output = ProcessRunner
            .run(
                &ProcessSpec::new("curl")
                    .args([
                        "--silent",
                        "--show-error",
                        "--output",
                        "/dev/null",
                        "--write-out",
                        "%{http_code}",
                        "--max-time",
                        &timeout_seconds,
                        "--request",
                        "POST",
                        "--header",
                        "Content-Type: application/json",
                        "--header",
                        &format!("X-Lumic-Signature: sha256={signature}"),
                        "--header",
                        &format!("X-Lumic-Delivery: {delivery_id}"),
                        "--data-binary",
                        "@-",
                        "--",
                        &destination.url,
                    ])
                    .stdin(body),
            )
            .await;
        let (success, status, error) = match output {
            Ok(output) => {
                let status = String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .parse::<u16>()
                    .ok();
                let success =
                    output.success() && status.is_some_and(|value| (200..300).contains(&value));
                let error = (!success).then(|| {
                    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
                    if stderr.is_empty() {
                        format!("webhook returned HTTP {}", status.unwrap_or(0))
                    } else {
                        stderr
                    }
                });
                (success, status, error)
            }
            Err(error) => (false, None, Some(error.to_string())),
        };
        self.record_delivery_result(delivery_id, success, status, error)
    }

    fn record_delivery_result(
        &self,
        delivery_id: &str,
        success: bool,
        status: Option<u16>,
        error: Option<String>,
    ) -> Result<()> {
        let mut state = self.load_state()?;
        let destination_attempts = state
            .deliveries
            .iter()
            .find(|value| value.id == delivery_id)
            .and_then(|delivery| {
                state
                    .destinations
                    .iter()
                    .find(|value| value.id == delivery.destination_id)
            })
            .map_or(1, |value| value.max_attempts);
        let delivery = state
            .deliveries
            .iter_mut()
            .find(|value| value.id == delivery_id)
            .ok_or_else(|| invalid("delivery_id", "delivery does not exist"))?;
        delivery.attempts = delivery.attempts.saturating_add(1);
        delivery.response_status = status;
        delivery.last_error = error.map(|value| truncate(&value, 1_024));
        if success {
            delivery.status = DeliveryStatus::Delivered;
            delivery.completed_at_unix_ms = Some(now_ms());
        } else if delivery.attempts >= destination_attempts {
            delivery.status = DeliveryStatus::Exhausted;
            delivery.completed_at_unix_ms = Some(now_ms());
        } else {
            delivery.status = DeliveryStatus::RetryScheduled;
            let delay_seconds = 2_u128.pow(u32::from(delivery.attempts.min(8)));
            delivery.next_attempt_unix_ms = now_ms() + delay_seconds * 1_000;
        }
        self.save_state(&state)
    }

    fn load_state(&self) -> Result<AutomationState> {
        if !self.state_path.exists() {
            return Ok(AutomationState::default());
        }
        let bytes = fs::read(&self.state_path).map_err(io_error)?;
        serde_json::from_slice(&bytes).map_err(json_error)
    }

    fn save_state(&self, state: &AutomationState) -> Result<()> {
        let backup = self.state_path.with_file_name(".state.json.lumic-backup");
        let preserved_snapshot = fs::read(&backup).ok();
        self.write_state(state)?;
        if let Some(bytes) = preserved_snapshot {
            let mut options = OpenOptions::new();
            options.write(true).truncate(true);
            let mut file = options.open(&backup).map_err(io_error)?;
            file.write_all(&bytes).map_err(io_error)?;
            file.sync_all().map_err(io_error)?;
        }
        Ok(())
    }

    fn save_configuration_state(&self, state: &AutomationState) -> Result<()> {
        self.write_state(state)
    }

    fn write_state(&self, state: &AutomationState) -> Result<()> {
        let mut state = state.clone();
        if state.deliveries.len() > MAX_DELIVERY_HISTORY {
            let remove = state.deliveries.len() - MAX_DELIVERY_HISTORY;
            for delivery in state.deliveries.drain(..remove) {
                state.signal_payloads.remove(&delivery.signal_id);
            }
        }
        let bytes = serde_json::to_vec_pretty(&state).map_err(json_error)?;
        write_atomic(&self.state_path, &bytes, 0o600)?;
        Ok(())
    }

    fn append_timeline(&self, signal: &OperationalSignal) -> Result<()> {
        jsonl_store::append(&self.timeline_path, signal, "operations timeline")
    }

    fn record_configuration(
        &self,
        context: &OperationContext,
        action: (&str, &str),
        entity: &str,
        entity_id: &str,
        before: Value,
        after: Value,
    ) -> Result<()> {
        AuditStore::at_state_dir(&self.state_dir).append(&AuditRecord::now(
            context,
            action.0,
            action.1,
            entity,
            entity_id,
            json!({}),
            Some(before),
            Some(after),
            true,
            "operations configuration applied",
        ))
    }

    fn read_timeline(&self) -> Result<Vec<OperationalSignal>> {
        jsonl_store::latest(
            &self.timeline_path,
            MAX_TIMELINE_READ,
            "operations timeline",
        )
    }
}

fn configuration_value(state: &AutomationState) -> Value {
    json!({
        "destinations": &state.destinations,
        "subscriptions": &state.subscriptions,
        "rules": &state.rules,
    })
}

fn signal_from_event(event: Event) -> OperationalSignal {
    let severity = if event.event_type.contains("failed") || event.event_type.contains("failure") {
        SignalSeverity::Error
    } else {
        SignalSeverity::Info
    };
    let kind = if event.event_type.contains("deploy") {
        SignalKind::DeploymentMarker
    } else if event.entity.contains("service") {
        SignalKind::ServiceHealth
    } else if event.entity.contains("application") {
        SignalKind::ApplicationHealth
    } else {
        SignalKind::SystemEvent
    };
    OperationalSignal {
        id: unique_id("event"),
        timestamp_unix_ms: event.timestamp_unix_ms,
        kind,
        severity,
        event_type: event.event_type,
        entity: event.entity,
        entity_id: event.entity_id,
        correlation_id: event.correlation_id,
        summary: format!(
            "event recorded by {} through {:?}",
            event.actor, event.interface
        ),
        evidence: vec!["imported from Lumic's durable event store".into()],
        payload: event.payload,
    }
}

fn enqueue_subscriptions(state: &mut AutomationState, signal: &OperationalSignal) {
    for subscription in state
        .subscriptions
        .iter()
        .filter(|value| value.matches(signal))
    {
        if !state
            .destinations
            .iter()
            .any(|destination| destination.id == subscription.destination_id && destination.enabled)
        {
            continue;
        }
        state.deliveries.push(WebhookDelivery {
            id: unique_id("delivery"),
            destination_id: subscription.destination_id.clone(),
            signal_id: signal.id.clone(),
            status: DeliveryStatus::Pending,
            attempts: 0,
            next_attempt_unix_ms: now_ms(),
            created_at_unix_ms: now_ms(),
            completed_at_unix_ms: None,
            last_error: None,
            response_status: None,
        });
        state
            .signal_payloads
            .insert(signal.id.clone(), signal.clone());
    }
}

fn configuration_plan(kind: &str, id: &str, after: String) -> Plan {
    use lumic_core::{Capability, Change, Risk, RiskLevel};
    Plan {
        id: format!("operations-{kind}-{id}"),
        summary: format!("Configure operations {kind} {id}"),
        changes: vec![Change {
            capability: Capability::new("operations.configuration.apply"),
            summary: format!("configure {kind} {id}"),
            before: None,
            after: Some(after),
            reversible: true,
        }],
        risks: vec![Risk {
            level: RiskLevel::Low,
            summary: "notification configuration changes operational routing".into(),
            mitigation: Some(
                "Lumic keeps a recoverable sibling snapshot of the prior configuration".into(),
            ),
        }],
        preconditions: vec!["the referenced secret exists and is never returned".into()],
        validation: vec!["destination and retry bounds pass validation".into()],
        recovery: vec!["restore the previous Lumic operations configuration snapshot".into()],
    }
}

fn impacted_resources(rule: &AutomationRule) -> Vec<String> {
    match &rule.action {
        AutomationAction::RestartService { unit } => vec![format!("systemd:{unit}")],
    }
}

fn validate_signal_fields(
    event_type: &str,
    entity: &str,
    entity_id: &str,
    summary: &str,
) -> Result<()> {
    for (field, value, limit) in [
        ("event_type", event_type, 128),
        ("entity", entity, 64),
        ("entity_id", entity_id, 128),
        ("summary", summary, 512),
    ] {
        if value.is_empty() || value.len() > limit || value.contains(['\n', '\r']) {
            return Err(invalid(
                field,
                "is empty, too long, or contains control characters",
            ));
        }
    }
    Ok(())
}

fn upsert<T>(values: &mut Vec<T>, new: T, id: impl Fn(&T) -> &String) {
    if let Some(index) = values.iter().position(|value| id(value) == id(&new)) {
        values[index] = new;
    } else {
        values.push(new);
    }
}

fn hmac_sha256_hex(secret: &[u8], data: &[u8]) -> String {
    const BLOCK_SIZE: usize = 64;
    let mut key = if secret.len() > BLOCK_SIZE {
        Sha256::digest(secret).to_vec()
    } else {
        secret.to_vec()
    };
    key.resize(BLOCK_SIZE, 0);
    let mut inner_pad = [0x36_u8; BLOCK_SIZE];
    let mut outer_pad = [0x5c_u8; BLOCK_SIZE];
    for (index, byte) in key.iter().enumerate() {
        inner_pad[index] ^= byte;
        outer_pad[index] ^= byte;
    }
    let mut inner = Sha256::new();
    inner.update(inner_pad);
    inner.update(data);
    let inner = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(inner);
    hex_encode(&outer.finalize())
}

fn unique_id(prefix: &str) -> String {
    format!(
        "{prefix}-{}-{}-{}",
        now_ms(),
        std::process::id(),
        ID_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis()
}

fn truncate(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

fn invalid(field: &str, message: &str) -> LumicError {
    LumicError::InvalidInput {
        field: field.into(),
        message: message.into(),
    }
}

fn io_error(error: std::io::Error) -> LumicError {
    LumicError::Internal {
        message: format!("operations store I/O failed: {error}"),
    }
}

fn json_error(error: serde_json::Error) -> LumicError {
    LumicError::Internal {
        message: format!("operations data is invalid: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("lumic-operations-{name}-{}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn context() -> OperationContext {
        OperationContext {
            actor: "test".into(),
            interface: OperationInterface::Internal,
            correlation_id: "epic-e-test".into(),
            dry_run: false,
            approved: true,
        }
    }

    #[test]
    fn hmac_matches_rfc_4231_vector() {
        assert_eq!(
            hmac_sha256_hex(&[0x0b; 20], b"Hi There"),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[tokio::test]
    async fn persists_timeline_configuration_and_bounded_delivery_history() {
        let state_dir = directory("flow");
        SecretStore::at_state_dir(&state_dir)
            .put("webhook-key", b"super-secret")
            .unwrap();
        let service = OperationsService::at_state_dir(&state_dir);
        service
            .apply_destination(
                WebhookDestination {
                    id: "local".into(),
                    url: "http://127.0.0.1:9321/hook".into(),
                    secret_reference: "webhook-key".into(),
                    timeout_ms: 1_000,
                    max_attempts: 3,
                    enabled: true,
                },
                &context(),
            )
            .unwrap();
        service
            .apply_subscription(
                EventSubscription {
                    id: "failures".into(),
                    destination_id: "local".into(),
                    event_types: vec!["provider.failed".into()],
                    entity: None,
                    entity_id: None,
                    enabled: true,
                },
                &context(),
            )
            .unwrap();
        service
            .record_provider_signal(
                "provider.failed",
                "provider",
                "demo",
                SignalSeverity::Error,
                "reference failure",
                json!({"source": "test"}),
            )
            .await
            .unwrap();
        assert_eq!(
            service
                .timeline(&TimelineQuery {
                    limit: 10,
                    ..Default::default()
                })
                .unwrap()
                .len(),
            1
        );
        assert_eq!(service.deliveries(10).unwrap().len(), 1);
        assert_eq!(
            service
                .incident(&TimelineQuery {
                    limit: 10,
                    ..Default::default()
                })
                .unwrap()
                .findings
                .len(),
            1
        );
        service.rollback_configuration(&context()).unwrap();
        assert!(service.subscriptions().unwrap().is_empty());
        assert_eq!(service.deliveries(10).unwrap().len(), 1);
        assert_eq!(
            AuditStore::at_state_dir(&state_dir).list(10).unwrap().len(),
            3
        );
        fs::remove_dir_all(state_dir).unwrap();
    }

    #[test]
    fn failed_delivery_retries_then_exhausts() {
        let state_dir = directory("retry");
        let service = OperationsService::at_state_dir(&state_dir);
        let mut state = AutomationState {
            destinations: vec![WebhookDestination {
                id: "local".into(),
                url: "http://127.0.0.1:1/hook".into(),
                secret_reference: "missing".into(),
                timeout_ms: 100,
                max_attempts: 2,
                enabled: true,
            }],
            deliveries: vec![WebhookDelivery {
                id: "delivery-1".into(),
                destination_id: "local".into(),
                signal_id: "signal-1".into(),
                status: DeliveryStatus::Pending,
                attempts: 0,
                next_attempt_unix_ms: 0,
                created_at_unix_ms: 0,
                completed_at_unix_ms: None,
                last_error: None,
                response_status: None,
            }],
            ..Default::default()
        };
        service.save_state(&state).unwrap();
        service
            .record_delivery_result("delivery-1", false, None, Some("offline".into()))
            .unwrap();
        state = service.load_state().unwrap();
        assert_eq!(state.deliveries[0].status, DeliveryStatus::RetryScheduled);
        service
            .record_delivery_result("delivery-1", false, None, Some("offline".into()))
            .unwrap();
        state = service.load_state().unwrap();
        assert_eq!(state.deliveries[0].status, DeliveryStatus::Exhausted);
        fs::remove_dir_all(state_dir).unwrap();
    }
}
