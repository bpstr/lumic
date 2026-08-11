use crate::{
    application::ApplicationService, atomic_file::write_atomic, audit_store::AuditStore,
    diagnostics::diagnose_host, event_store::EventStore, framework_state::FrameworkStateStore,
    managed_service::ManagedServiceManager,
};
use lumic_core::{
    DiagnosticReport, LumicError, OperationContext, OperationResult, OperationStatus, Result,
    application::Application,
    attention::{
        AttentionChange, AttentionFact, AttentionItem, AttentionReport, AttentionSeverity,
        AttentionSummary, NodePersonality, render_attention,
    },
    events::{AuditRecord, Event},
    managed_service::{BackupStatus, ServiceBackup},
    resource::ResourceKind,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_PERIOD_HOURS: u64 = 24 * 30;
const MAX_CHANGES: usize = 20;
const MAX_BACKUP_AGE_HOURS: u128 = 24;
const CERTIFICATE_WARNING_DAYS: u64 = 30;
const CERTIFICATE_CRITICAL_DAYS: u64 = 7;

#[derive(Debug, Clone)]
struct CertificateEvidence {
    name: String,
    expires_at_unix_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AttentionSettings {
    personality: NodePersonality,
}

#[derive(Debug, Clone)]
pub struct AttentionService {
    state_dir: PathBuf,
    apps_root: PathBuf,
}

impl AttentionService {
    pub fn new(state_dir: impl Into<PathBuf>, apps_root: impl Into<PathBuf>) -> Self {
        Self {
            state_dir: state_dir.into(),
            apps_root: apps_root.into(),
        }
    }

    pub fn personality(&self) -> Result<NodePersonality> {
        let path = self.settings_path();
        if !path.exists() {
            return Ok(NodePersonality::Professional);
        }
        if path.is_symlink() {
            return Err(LumicError::InvalidInput {
                field: "personality".into(),
                message: "refusing to read personality settings through a symbolic link".into(),
            });
        }
        let bytes = fs::read(&path).map_err(state_io)?;
        if bytes.len() > 16 * 1024 {
            return Err(LumicError::InvalidInput {
                field: "personality".into(),
                message: "personality settings exceed 16 KiB".into(),
            });
        }
        serde_json::from_slice::<AttentionSettings>(&bytes)
            .map(|settings| settings.personality)
            .map_err(|error| LumicError::Internal {
                message: format!("personality settings are invalid: {error}"),
            })
    }

    pub fn set_personality(
        &self,
        personality: NodePersonality,
        context: &OperationContext,
    ) -> Result<OperationResult<NodePersonality>> {
        let before = self.personality()?;
        let bytes =
            serde_json::to_vec_pretty(&AttentionSettings { personality }).map_err(|error| {
                LumicError::Internal {
                    message: format!("could not serialize personality settings: {error}"),
                }
            })?;
        let mut contents = bytes;
        contents.push(b'\n');
        let write = write_atomic(&self.settings_path(), &contents, 0o600)?;
        let result = OperationResult {
            status: OperationStatus::Succeeded,
            value: Some(personality),
            changed: write.changed,
            message: if write.changed {
                format!("node personality changed from {before} to {personality}")
            } else {
                format!("node personality is already {personality}")
            },
        };
        let audit = AuditRecord::now(
            context,
            "attention.personality",
            "set",
            "node",
            "local",
            json!({"personality": personality}),
            Some(json!({"personality": before})),
            Some(json!({"personality": personality})),
            true,
            &result.message,
        );
        AuditStore::at_state_dir(&self.state_dir).append(&audit)?;
        if write.changed {
            EventStore::at_state_dir(&self.state_dir).append(&Event::now(
                "node.personality.changed",
                &context.actor,
                context.interface,
                "node",
                "local",
                &context.correlation_id,
                json!({"before": before, "after": personality}),
            ))?;
        }
        Ok(result)
    }

    pub async fn report(&self, period_hours: u64) -> Result<AttentionReport> {
        if !(1..=MAX_PERIOD_HOURS).contains(&period_hours) {
            return Err(LumicError::InvalidInput {
                field: "period_hours".into(),
                message: format!("must be between 1 and {MAX_PERIOD_HOURS}"),
            });
        }
        let diagnostics = diagnose_host().await?;
        let applications = ApplicationService::new(&self.state_dir, &self.apps_root).list()?;
        let application_service = ApplicationService::new(&self.state_dir, &self.apps_root);
        let deployment_count = applications
            .iter()
            .try_fold(0_usize, |count, application| {
                application_service
                    .deployments(&application.id)
                    .map(|deployments| count + deployments.len())
            })?;
        let manager = ManagedServiceManager::at_state_dir(&self.state_dir);
        let services = manager.list()?;
        let mut backups = Vec::new();
        for service in &services {
            backups.extend(manager.backups(&service.id)?);
        }
        let events = EventStore::at_state_dir(&self.state_dir).list(200)?;
        let certificates = certificate_evidence(&self.state_dir)?;
        let personality = self.personality()?;
        let now = now_ms();
        let summary = build_summary(
            diagnostics,
            &applications,
            &backups,
            &events,
            &certificates,
            services.len(),
            deployment_count,
            now,
            period_hours,
        );
        Ok(AttentionReport {
            personality,
            rendered: render_attention(&summary, personality),
            summary,
        })
    }

    fn settings_path(&self) -> PathBuf {
        self.state_dir.join("attention.json")
    }
}

#[allow(clippy::too_many_arguments)]
fn build_summary(
    diagnostics: DiagnosticReport,
    applications: &[Application],
    backups: &[ServiceBackup],
    events: &[Event],
    certificates: &[CertificateEvidence],
    service_count: usize,
    deployment_count: usize,
    now: u128,
    period_hours: u64,
) -> AttentionSummary {
    let period_start = now.saturating_sub(u128::from(period_hours) * 60 * 60 * 1000);
    let security_updates = diagnostics
        .updates
        .iter()
        .filter(|update| update.security)
        .count();
    let root = diagnostics
        .mounts
        .iter()
        .find(|mount| mount.mount_point == "/")
        .map(|mount| (mount.available_bytes, mount.total_bytes))
        .or_else(|| {
            diagnostics
                .host
                .disks
                .iter()
                .find(|disk| disk.mount_point == "/")
                .map(|disk| (disk.available_bytes, disk.total_bytes))
        });
    let mut facts = vec![
        AttentionFact {
            key: "hostname".into(),
            label: "Hostname".into(),
            value: diagnostics.host.hostname.clone(),
            evidence: "/proc/sys/kernel/hostname".into(),
        },
        AttentionFact {
            key: "operating_system".into(),
            label: "Operating system".into(),
            value: format!(
                "{} {}",
                diagnostics.host.distribution.distribution.id(),
                diagnostics.host.distribution.version_id
            ),
            evidence: "/etc/os-release".into(),
        },
        AttentionFact {
            key: "uptime_seconds".into(),
            label: "Uptime".into(),
            value: format!("{} seconds", diagnostics.load.uptime_seconds),
            evidence: "/proc/uptime".into(),
        },
        AttentionFact {
            key: "load_1m".into(),
            label: "1 minute load".into(),
            value: format!("{:.2}", diagnostics.load.one_minute),
            evidence: "/proc/loadavg".into(),
        },
        AttentionFact {
            key: "memory".into(),
            label: "Available memory".into(),
            value: format!(
                "{} of {} bytes",
                diagnostics.host.memory.available_bytes, diagnostics.host.memory.total_bytes
            ),
            evidence: "/proc/meminfo".into(),
        },
        AttentionFact {
            key: "applications".into(),
            label: "Applications".into(),
            value: applications.len().to_string(),
            evidence: "Lumic application state".into(),
        },
        AttentionFact {
            key: "managed_services".into(),
            label: "Managed services".into(),
            value: service_count.to_string(),
            evidence: "Lumic managed-service state".into(),
        },
        AttentionFact {
            key: "deployments".into(),
            label: "Deployments".into(),
            value: deployment_count.to_string(),
            evidence: "Lumic deployment history".into(),
        },
        AttentionFact {
            key: "failed_services".into(),
            label: "Failed services".into(),
            value: diagnostics.failed_services.len().to_string(),
            evidence: "systemd failed-unit inspection".into(),
        },
        AttentionFact {
            key: "certificates".into(),
            label: "Certificates".into(),
            value: certificates.len().to_string(),
            evidence: "Lumic certificate resource state".into(),
        },
        AttentionFact {
            key: "backups".into(),
            label: "Recorded backups".into(),
            value: backups.len().to_string(),
            evidence: "Lumic managed-service backup state".into(),
        },
        AttentionFact {
            key: "updates".into(),
            label: "Pending updates".into(),
            value: format!(
                "{} total, {security_updates} security",
                diagnostics.updates.len()
            ),
            evidence: "apt candidate versions".into(),
        },
    ];
    if let Some((available, total)) = root {
        facts.push(AttentionFact {
            key: "root_disk".into(),
            label: "Root disk available".into(),
            value: format!("{available} of {total} bytes"),
            evidence: "statvfs or mount inspection".into(),
        });
    }

    let changes = events
        .iter()
        .filter(|event| event.timestamp_unix_ms >= period_start && event.timestamp_unix_ms <= now)
        .take(MAX_CHANGES)
        .map(|event| AttentionChange {
            timestamp_unix_ms: event.timestamp_unix_ms,
            event_type: event.event_type.clone(),
            resource: format!("{}:{}", event.entity, event.entity_id),
            summary: format!(
                "{} for {}:{}",
                event.event_type, event.entity, event.entity_id
            ),
            evidence: format!(
                "recorded by {} via {:?}; correlation {}",
                event.actor, event.interface, event.correlation_id
            ),
        })
        .collect();

    let mut active_incidents = Vec::new();
    let mut upcoming_attention = Vec::new();
    let mut recommendations = Vec::new();
    for finding in &diagnostics.findings {
        let severity = severity_from_finding(&finding.severity);
        let item = AttentionItem {
            id: slug(&finding.summary),
            severity,
            summary: finding.summary.clone(),
            evidence: finding.evidence.clone(),
            recommendation: finding.recommendation.clone(),
        };
        if severity == AttentionSeverity::Critical {
            active_incidents.push(item.clone());
        } else {
            upcoming_attention.push(item.clone());
        }
        recommendations.push(item);
    }
    for application in applications {
        let status = application.health_status.to_ascii_lowercase();
        if matches!(status.as_str(), "failed" | "unhealthy" | "degraded") {
            let severity = if status == "degraded" {
                AttentionSeverity::Warning
            } else {
                AttentionSeverity::Critical
            };
            active_incidents.push(AttentionItem {
                id: format!("application-health-{}", application.id),
                severity,
                summary: format!("application {} reports {status}", application.id),
                evidence: "Lumic application health state".into(),
                recommendation: format!(
                    "inspect application {} health and recent deployments",
                    application.id
                ),
            });
        }
        if application.tls.enabled && application.tls.certificate_name.is_none() {
            upcoming_attention.push(AttentionItem {
                id: format!("application-tls-{}", application.id),
                severity: AttentionSeverity::Warning,
                summary: format!(
                    "application {} has TLS enabled without a recorded certificate",
                    application.id
                ),
                evidence: "Lumic application TLS state".into(),
                recommendation: "inspect the nginx TLS configuration and certificate state".into(),
            });
        }
    }
    for backup in latest_backups(backups) {
        if backup.status == BackupStatus::Failed {
            upcoming_attention.push(AttentionItem {
                id: format!("backup-{}", backup.service_id),
                severity: AttentionSeverity::Warning,
                summary: format!("latest backup for {} failed", backup.service_id),
                evidence: format!("backup {}: {}", backup.id, backup.message),
                recommendation: format!("inspect and retry backup {}", backup.id),
            });
        }
        let age_hours = now
            .saturating_sub(backup.created_at_unix_ms)
            .checked_div(60 * 60 * 1000)
            .unwrap_or(0);
        if age_hours > MAX_BACKUP_AGE_HOURS {
            upcoming_attention.push(AttentionItem {
                id: format!("backup-age-{}", backup.service_id),
                severity: AttentionSeverity::Warning,
                summary: format!(
                    "latest backup for {} is {age_hours} hours old",
                    backup.service_id
                ),
                evidence: format!(
                    "backup {} was recorded at {}; policy maximum is {} hours",
                    backup.id, backup.created_at_unix_ms, MAX_BACKUP_AGE_HOURS
                ),
                recommendation: format!("run and verify a fresh backup for {}", backup.service_id),
            });
        }
    }
    let now_seconds = u64::try_from(now / 1000).unwrap_or(u64::MAX);
    for certificate in certificates {
        let Some(expires_at) = certificate.expires_at_unix_seconds else {
            upcoming_attention.push(AttentionItem {
                id: format!("certificate-expiry-{}", certificate.name),
                severity: AttentionSeverity::Warning,
                summary: format!(
                    "certificate {} has no recorded expiry evidence",
                    certificate.name
                ),
                evidence: "certificate resource has no expires_at_unix_seconds observation".into(),
                recommendation: format!("inspect and renew certificate {}", certificate.name),
            });
            continue;
        };
        let remaining_days = expires_at.saturating_sub(now_seconds) / (24 * 60 * 60);
        if expires_at <= now_seconds || remaining_days <= CERTIFICATE_CRITICAL_DAYS {
            active_incidents.push(AttentionItem {
                id: format!("certificate-expiry-{}", certificate.name),
                severity: AttentionSeverity::Critical,
                summary: if expires_at <= now_seconds {
                    format!("certificate {} is expired", certificate.name)
                } else {
                    format!(
                        "certificate {} expires in {remaining_days} days",
                        certificate.name
                    )
                },
                evidence: format!("recorded expiry is Unix second {expires_at}"),
                recommendation: format!("renew certificate {} immediately", certificate.name),
            });
        } else if remaining_days <= CERTIFICATE_WARNING_DAYS {
            upcoming_attention.push(AttentionItem {
                id: format!("certificate-expiry-{}", certificate.name),
                severity: AttentionSeverity::Warning,
                summary: format!(
                    "certificate {} expires in {remaining_days} days",
                    certificate.name
                ),
                evidence: format!("recorded expiry is Unix second {expires_at}"),
                recommendation: format!("schedule renewal for certificate {}", certificate.name),
            });
        }
    }
    recommendations.extend(active_incidents.iter().cloned());
    recommendations.extend(upcoming_attention.iter().cloned());
    deduplicate(&mut active_incidents);
    deduplicate(&mut upcoming_attention);
    deduplicate(&mut recommendations);
    let severity = active_incidents
        .iter()
        .chain(upcoming_attention.iter())
        .map(|item| item.severity)
        .max()
        .unwrap_or(AttentionSeverity::Healthy);
    AttentionSummary {
        generated_at_unix_ms: now,
        period_start_unix_ms: period_start,
        period_end_unix_ms: now,
        severity,
        facts,
        changes,
        active_incidents,
        upcoming_attention,
        recommendations,
    }
}

fn certificate_evidence(state_dir: &Path) -> Result<Vec<CertificateEvidence>> {
    Ok(FrameworkStateStore::at_state_dir(state_dir)
        .load()?
        .resources
        .into_iter()
        .filter(|record| record.resource.kind == ResourceKind::Certificate)
        .map(|record| CertificateEvidence {
            name: record
                .attributes
                .get("certificate_name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(&record.resource.id)
                .to_owned(),
            expires_at_unix_seconds: record
                .attributes
                .get("expires_at_unix_seconds")
                .and_then(serde_json::Value::as_u64),
        })
        .collect())
}

fn severity_from_finding(value: &str) -> AttentionSeverity {
    match value {
        "critical" | "error" => AttentionSeverity::Critical,
        "warning" => AttentionSeverity::Warning,
        _ => AttentionSeverity::Notice,
    }
}

fn latest_backups(backups: &[ServiceBackup]) -> Vec<&ServiceBackup> {
    let mut service_ids = BTreeSet::new();
    let mut sorted: Vec<_> = backups.iter().collect();
    sorted.sort_by_key(|backup| std::cmp::Reverse(backup.created_at_unix_ms));
    sorted
        .into_iter()
        .filter(|backup| service_ids.insert(backup.service_id.as_str()))
        .collect()
}

fn deduplicate(items: &mut Vec<AttentionItem>) {
    let mut ids = BTreeSet::new();
    items.retain(|item| ids.insert(item.id.clone()));
}

fn slug(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned()
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn state_io(error: std::io::Error) -> LumicError {
    LumicError::Internal {
        message: format!("attention state I/O failed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumic_core::{
        Architecture, DiskFacts, Distribution, DistributionFacts, HostFacts, LoadFacts,
        MemoryFacts, OperationInterface,
    };

    fn diagnostic() -> DiagnosticReport {
        DiagnosticReport {
            host: HostFacts {
                operating_system: lumic_core::OperatingSystem::Linux,
                distribution: DistributionFacts {
                    distribution: Distribution::Ubuntu,
                    version_id: "24.04".into(),
                    version_name: "Ubuntu 24.04".into(),
                },
                architecture: Architecture::X86_64,
                hostname: "demo".into(),
                kernel_release: "test".into(),
                cpu_count: 2,
                memory: MemoryFacts {
                    total_bytes: 100,
                    available_bytes: 50,
                    swap_total_bytes: 0,
                    swap_free_bytes: 0,
                },
                disks: vec![DiskFacts {
                    mount_point: "/".into(),
                    filesystem: "ext4".into(),
                    total_bytes: 100,
                    available_bytes: 50,
                }],
            },
            load: LoadFacts {
                one_minute: 0.5,
                five_minutes: 0.4,
                fifteen_minutes: 0.3,
                running_processes: 1,
                total_processes: 10,
                uptime_seconds: 3600,
            },
            top_processes: Vec::new(),
            failed_services: Vec::new(),
            listeners: Vec::new(),
            mounts: Vec::new(),
            timers: Vec::new(),
            updates: Vec::new(),
            findings: Vec::new(),
        }
    }

    #[test]
    fn recent_events_do_not_invent_an_active_incident() {
        let event = Event {
            timestamp_unix_ms: 9_000,
            event_type: "deployment.failed".into(),
            actor: "tester".into(),
            interface: OperationInterface::Cli,
            entity: "application".into(),
            entity_id: "demo".into(),
            correlation_id: "test".into(),
            payload: json!({}),
        };
        let summary = build_summary(diagnostic(), &[], &[], &[event], &[], 0, 0, 10_000, 1);
        assert_eq!(summary.severity, AttentionSeverity::Healthy);
        assert_eq!(summary.changes.len(), 1);
        assert!(summary.active_incidents.is_empty());
    }

    #[test]
    fn latest_failed_backup_becomes_upcoming_attention_and_recommendation() {
        let backup = ServiceBackup {
            id: "backup-1".into(),
            service_id: "database".into(),
            database: None,
            path: "/var/backups/lumic/backup-1".into(),
            size_bytes: 0,
            checksum_sha256: None,
            status: BackupStatus::Failed,
            created_at_unix_ms: 9_000,
            message: "provider returned failure".into(),
        };
        let summary = build_summary(diagnostic(), &[], &[backup], &[], &[], 1, 0, 10_000, 1);
        assert_eq!(summary.severity, AttentionSeverity::Warning);
        assert!(
            summary.upcoming_attention[0]
                .summary
                .contains("latest backup")
        );
        assert!(summary.recommendations[0].recommendation.contains("retry"));
    }

    #[test]
    fn stale_backup_and_near_expiry_certificate_are_visible_attention() {
        let now = 40 * 60 * 60 * 1000_u128;
        let backup = ServiceBackup {
            id: "backup-old".into(),
            service_id: "database".into(),
            database: None,
            path: "/var/backups/lumic/backup-old".into(),
            size_bytes: 10,
            checksum_sha256: Some("checksum".into()),
            status: BackupStatus::Completed,
            created_at_unix_ms: now - (25 * 60 * 60 * 1000),
            message: "completed".into(),
        };
        let certificate = CertificateEvidence {
            name: "demo.example.com".into(),
            expires_at_unix_seconds: Some(u64::try_from(now / 1000).unwrap() + 6 * 86_400),
        };
        let summary = build_summary(
            diagnostic(),
            &[],
            &[backup],
            &[],
            &[certificate],
            1,
            0,
            now,
            24,
        );
        assert_eq!(summary.severity, AttentionSeverity::Critical);
        assert!(
            summary
                .upcoming_attention
                .iter()
                .any(|item| item.id == "backup-age-database")
        );
        assert!(
            summary
                .active_incidents
                .iter()
                .any(|item| item.id == "certificate-expiry-demo.example.com")
        );
    }

    #[test]
    fn personality_is_durable_audited_and_idempotent() {
        let root = std::env::temp_dir().join(format!("lumic-attention-{}", now_ms()));
        let service = AttentionService::new(&root, root.join("apps"));
        let context = OperationContext {
            actor: "tester".into(),
            interface: OperationInterface::Cli,
            correlation_id: "personality-test".into(),
            dry_run: false,
            approved: true,
        };
        assert_eq!(
            service.personality().unwrap(),
            NodePersonality::Professional
        );
        assert!(
            service
                .set_personality(NodePersonality::Dry, &context)
                .unwrap()
                .changed
        );
        assert_eq!(service.personality().unwrap(), NodePersonality::Dry);
        assert!(
            !service
                .set_personality(NodePersonality::Dry, &context)
                .unwrap()
                .changed
        );
        assert_eq!(EventStore::at_state_dir(&root).list(10).unwrap().len(), 1);
        assert_eq!(AuditStore::at_state_dir(&root).list(10).unwrap().len(), 2);
        let _ = fs::remove_dir_all(root);
    }
}
