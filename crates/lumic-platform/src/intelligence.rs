use crate::{
    ProcessRunner, ProcessSpec,
    application::ApplicationService,
    atomic_file::write_atomic,
    audit_store::AuditStore,
    event_store::EventStore,
    managed_service::ManagedServiceManager,
    operations::OperationsService,
    secret_store::SecretStore,
    systemd::{ServiceAction, SystemdServiceManager},
};
use lumic_core::{
    LumicError, OperationContext, Result,
    application::{
        Application, ApplicationProcessKind, ApplicationRuntime, ApplicationServiceReference,
    },
    events::{AuditRecord, Event},
    intelligence::{
        self, ApplicationDependencyGraph, ApplicationFingerprint, Confidence, ConfigurationDiff,
        ConfigurationInspection, ConfigurationKey, DependencyEdge, DependencyNode,
        DependencyNodeKind, Evidence, IncidentAnalysis, IncidentContext, IntegrationApplyResult,
        IntegrationDefinition, IntegrationPlan,
    },
    managed_service::{ManagedService, ManagedServiceKind},
    operations::{TimelineQuery, validate_systemd_unit, validate_webhook},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

const MAX_DISCOVERY_FILE: u64 = 1024 * 1024;
pub const LARAVEL_REDIS_ID: &str = "laravel-redis@1";
const TARGET_KEYS: [&str; 6] = [
    "REDIS_HOST",
    "REDIS_PORT",
    "CACHE_STORE",
    "CACHE_DRIVER",
    "SESSION_DRIVER",
    "QUEUE_CONNECTION",
];

#[derive(Debug, Clone)]
pub struct ApplicationIntelligence {
    state_dir: PathBuf,
    applications: ApplicationService,
    services: ManagedServiceManager,
    operations: OperationsService,
    events: EventStore,
    audit: AuditStore,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConfigurationSnapshot {
    id: String,
    application_id: String,
    path: String,
    content: Vec<u8>,
    sha256: String,
    created_at_unix_ms: u128,
}

#[derive(Debug, Clone)]
struct Dotenv {
    lines: Vec<String>,
    values: BTreeMap<String, String>,
    duplicates: BTreeSet<String>,
    trailing_newline: bool,
}

impl ApplicationIntelligence {
    pub fn new(state_dir: impl AsRef<Path>, apps_root: impl Into<PathBuf>) -> Self {
        let state_dir = state_dir.as_ref().to_path_buf();
        let apps_root = apps_root.into();
        Self {
            applications: ApplicationService::new(&state_dir, apps_root),
            services: ManagedServiceManager::at_state_dir(&state_dir),
            operations: OperationsService::at_state_dir(&state_dir),
            events: EventStore::at_state_dir(&state_dir),
            audit: AuditStore::at_state_dir(&state_dir),
            state_dir,
        }
    }

    pub fn system() -> Self {
        Self::new("/var/lib/lumic", "/srv/lumic/apps")
    }

    pub fn catalog(&self) -> Vec<IntegrationDefinition> {
        vec![IntegrationDefinition {
            id: LARAVEL_REDIS_ID.into(),
            application_framework: "laravel".into(),
            service_kind: "redis".into(),
            description: "Connect a detected Laravel application to a managed Redis service".into(),
            configuration_keys: TARGET_KEYS.iter().map(ToString::to_string).collect(),
            verification_steps: vec![
                "managed Redis health check".into(),
                "application health check".into(),
            ],
        }]
    }

    pub fn fingerprint(&self, application_id: &str) -> Result<ApplicationFingerprint> {
        let application = self.applications.inspect(application_id)?;
        let root = deployed_root(&application)?;
        let mut evidence = Vec::new();
        let mut manifests = Vec::new();
        let mut framework = None;
        let composer = root.join("composer.json");
        let artisan = root.join("artisan");
        let composer_value = read_json_bounded(&composer).ok();
        if composer.exists() {
            manifests.push(path_string(&composer));
        }
        for name in ["composer.lock", "package.json", "package-lock.json"] {
            let path = root.join(name);
            if path.is_file() {
                manifests.push(path_string(&path));
            }
        }
        let composer_has_laravel = composer_value
            .as_ref()
            .and_then(|value| value.get("require"))
            .and_then(Value::as_object)
            .is_some_and(|requires| requires.contains_key("laravel/framework"));
        if composer_has_laravel {
            evidence.push(Evidence {
                source: path_string(&composer),
                observation: "composer require contains laravel/framework".into(),
            });
        }
        if artisan.is_file() {
            evidence.push(Evidence {
                source: path_string(&artisan),
                observation: "Laravel artisan entry point exists".into(),
            });
        }
        let confidence = if composer_has_laravel && artisan.is_file() {
            framework = Some("laravel".into());
            Confidence::High
        } else if composer_has_laravel || artisan.is_file() {
            framework = Some("laravel".into());
            Confidence::Medium
        } else {
            Confidence::Low
        };
        let environment_files = environment_candidates(&root)
            .into_iter()
            .filter(|path| path.is_file())
            .map(|path| path_string(&path))
            .collect::<Vec<_>>();
        let dotenv = active_environment_path(&root, Path::new(&application.root))
            .ok()
            .and_then(|path| read_bounded(&path).ok())
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .map(|contents| Dotenv::parse(&contents));
        let configured_keys = dotenv
            .as_ref()
            .map(|dotenv| dotenv.values.keys().cloned().collect())
            .unwrap_or_default();
        let worker_hints = application
            .processes
            .iter()
            .filter(|process| process.kind == ApplicationProcessKind::Worker)
            .map(|process| process.name.clone())
            .collect();
        let scheduler_hints = application
            .processes
            .iter()
            .filter(|process| process.kind == ApplicationProcessKind::Schedule)
            .map(|process| process.name.clone())
            .collect();
        let health_endpoints = if framework.as_deref() == Some("laravel") {
            vec!["/up".into()]
        } else {
            Vec::new()
        };
        Ok(ApplicationFingerprint {
            application_id: application.id,
            source_root: path_string(&root),
            framework,
            cms: None,
            runtime: runtime_name(application.runtime).into(),
            confidence,
            environment_files,
            dependency_manifests: manifests,
            worker_hints,
            scheduler_hints,
            configured_keys,
            health_endpoints,
            evidence,
        })
    }

    pub fn inspect_configuration(&self, application_id: &str) -> Result<ConfigurationInspection> {
        let application = self.applications.inspect(application_id)?;
        let path =
            active_environment_path(&deployed_root(&application)?, Path::new(&application.root))?;
        let dotenv = Dotenv::parse(
            &String::from_utf8(read_bounded(&path)?)
                .map_err(|_| invalid("environment", "dotenv file must be UTF-8"))?,
        );
        Ok(ConfigurationInspection {
            application_id: application.id,
            path: path_string(&path),
            keys: dotenv
                .values
                .keys()
                .map(|name| ConfigurationKey {
                    name: name.clone(),
                    configured: true,
                    sensitive: is_sensitive(name),
                })
                .collect(),
            duplicate_keys: dotenv.duplicates.into_iter().collect(),
            secret_values_exposed: false,
        })
    }

    pub fn dependency_graph(&self, application_id: &str) -> Result<ApplicationDependencyGraph> {
        let application = self.applications.inspect(application_id)?;
        Ok(graph_for(&application))
    }

    pub fn plan_integration(
        &self,
        integration_id: &str,
        application_id: &str,
        service_id: Option<&str>,
    ) -> Result<IntegrationPlan> {
        match integration_id {
            LARAVEL_REDIS_ID => self.plan_laravel_redis(application_id, service_id),
            _ => Err(invalid("integration", "unknown integration definition")),
        }
    }

    pub async fn apply_integration(
        &self,
        integration_id: &str,
        application_id: &str,
        service_id: Option<&str>,
        context: &OperationContext,
    ) -> Result<IntegrationApplyResult> {
        match integration_id {
            LARAVEL_REDIS_ID => {
                self.apply_laravel_redis(application_id, service_id, context)
                    .await
            }
            _ => Err(invalid("integration", "unknown integration definition")),
        }
    }

    pub fn plan_laravel_redis(
        &self,
        application_id: &str,
        service_id: Option<&str>,
    ) -> Result<IntegrationPlan> {
        let fingerprint = self.fingerprint(application_id)?;
        if fingerprint.framework.as_deref() != Some("laravel")
            || fingerprint.confidence != Confidence::High
        {
            return Err(invalid(
                "application",
                "Laravel integration requires high-confidence composer and artisan evidence",
            ));
        }
        let application = self.applications.inspect(application_id)?;
        let path =
            active_environment_path(&deployed_root(&application)?, Path::new(&application.root))?;
        let dotenv = Dotenv::parse(
            &String::from_utf8(read_bounded(&path)?)
                .map_err(|_| invalid("environment", "dotenv file must be UTF-8"))?,
        );
        if TARGET_KEYS
            .iter()
            .any(|key| dotenv.duplicates.contains(*key))
        {
            return Err(invalid(
                "environment",
                "duplicate integration keys must be resolved before planning",
            ));
        }
        let selected = select_redis(&self.services.list()?, service_id)?;
        let id = service_id
            .map(ToOwned::to_owned)
            .or_else(|| selected.as_ref().map(|service| service.id.clone()))
            .unwrap_or_else(|| "redis".into());
        let host = selected
            .as_ref()
            .map(|service| service.configuration.bind_address.clone())
            .unwrap_or_else(|| "127.0.0.1".into());
        let port = selected
            .as_ref()
            .map(|service| service.configuration.port)
            .unwrap_or(6379);
        let desired = desired_configuration(&dotenv, &host, port);
        let diff = configuration_diff(&dotenv, &desired);
        let affected_processes = affected_processes(&application);
        let mut graph = graph_for(&application);
        add_redis_graph(&mut graph, &id, &affected_processes);
        Ok(IntegrationPlan {
            integration_id: LARAVEL_REDIS_ID.into(),
            application_id: application.id.clone(),
            service_id: id.clone(),
            install_required: selected.is_none(),
            configuration_path: path_string(&path),
            configuration_diff: diff.clone(),
            affected_processes: affected_processes.clone(),
            dependency_graph: graph,
            plan: intelligence::integration_plan(
                &application.id,
                &id,
                selected.is_none(),
                &diff,
                &affected_processes,
            ),
        })
    }

    pub async fn apply_laravel_redis(
        &self,
        application_id: &str,
        service_id: Option<&str>,
        context: &OperationContext,
    ) -> Result<IntegrationApplyResult> {
        if !context.approved || context.dry_run {
            return Err(invalid(
                "approval",
                "apply requires an approved non-dry-run context; use plan first",
            ));
        }
        let plan = self.plan_laravel_redis(application_id, service_id)?;
        if plan.install_required {
            self.services
                .install(&plan.service_id, ManagedServiceKind::Redis, context)
                .await?;
        }
        let path = PathBuf::from(&plan.configuration_path);
        let original = read_bounded(&path)?;
        let dotenv = Dotenv::parse(
            &String::from_utf8(original.clone())
                .map_err(|_| invalid("environment", "dotenv file must be UTF-8"))?,
        );
        if TARGET_KEYS
            .iter()
            .any(|key| dotenv.duplicates.contains(*key))
        {
            return Err(invalid(
                "environment",
                "duplicate integration keys must be resolved before apply",
            ));
        }
        let snapshot = self.save_snapshot(application_id, &path, &original)?;
        let service = self
            .services
            .list()?
            .into_iter()
            .find(|service| service.id == plan.service_id)
            .ok_or_else(|| {
                invalid(
                    "service",
                    "selected Redis service was not found after installation",
                )
            })?;
        let desired = desired_configuration(
            &dotenv,
            &service.configuration.bind_address,
            service.configuration.port,
        );
        let updated = dotenv.render(&desired);
        let write = write_atomic(&path, updated.as_bytes(), 0o600)?;
        let mut restarted = Vec::new();
        let systemd = SystemdServiceManager::at_state_dir(&self.state_dir);
        let mutation = async {
            for process in &plan.affected_processes {
                let unit = format!("lumic-app-{application_id}-{process}.service");
                systemd
                    .apply(&unit, ServiceAction::Restart, context)
                    .await?;
                restarted.push(process.clone());
            }
            let status = self.services.inspect(&plan.service_id).await?;
            if status.health != lumic_core::managed_service::ServiceHealth::Healthy {
                return Err(LumicError::Inspection {
                    fact: "redis_health".into(),
                    message: status.health_message,
                });
            }
            let application_health = self
                .applications
                .verify_application_health(application_id)
                .await?;
            self.services.attach_to_application(
                &self.applications,
                application_id,
                ApplicationServiceReference {
                    service_id: plan.service_id.clone(),
                    role: "cache".into(),
                    database: None,
                    user: None,
                    secret_reference: None,
                },
                context,
            )?;
            Ok::<_, LumicError>(vec![status.health_message, application_health])
        }
        .await;
        let verification = match mutation {
            Ok(value) => value,
            Err(error) => {
                let _ = self.restore_snapshot(application_id, &snapshot.id, context);
                for process in &restarted {
                    let unit = format!("lumic-app-{application_id}-{process}.service");
                    let _ = systemd.apply(&unit, ServiceAction::Restart, context).await;
                }
                return Err(error);
            }
        };
        self.record("application.integration_applied", application_id, context, json!({"integration_id": LARAVEL_REDIS_ID, "service_id": plan.service_id, "changed_keys": plan.configuration_diff.iter().map(|item| &item.key).collect::<Vec<_>>(), "snapshot_id": snapshot.id}))?;
        Ok(IntegrationApplyResult {
            integration_id: LARAVEL_REDIS_ID.into(),
            application_id: application_id.into(),
            service_id: plan.service_id,
            changed: write.changed,
            snapshot_id: Some(snapshot.id.clone()),
            configuration_diff: plan.configuration_diff,
            restarted_processes: restarted,
            verification,
            recovery: vec![format!(
                "lumic intelligence rollback {application_id} {}",
                snapshot.id
            )],
        })
    }

    pub fn restore_snapshot(
        &self,
        application_id: &str,
        snapshot_id: &str,
        context: &OperationContext,
    ) -> Result<()> {
        if !context.approved || context.dry_run {
            return Err(invalid(
                "approval",
                "rollback requires an approved non-dry-run context",
            ));
        }
        validate_snapshot_id(snapshot_id)?;
        let snapshot: ConfigurationSnapshot =
            serde_json::from_slice(&fs::read(self.snapshot_path(snapshot_id)).map_err(state_io)?)
                .map_err(state_json)?;
        if snapshot.application_id != application_id
            || hex_sha256(&snapshot.content) != snapshot.sha256
        {
            return Err(invalid(
                "snapshot",
                "snapshot ownership or integrity check failed",
            ));
        }
        let application = self.applications.inspect(application_id)?;
        let allowed =
            active_environment_path(&deployed_root(&application)?, Path::new(&application.root))?;
        if Path::new(&snapshot.path) != allowed {
            return Err(invalid(
                "snapshot",
                "snapshot target no longer matches the active application environment",
            ));
        }
        write_atomic(&allowed, &snapshot.content, 0o600)?;
        self.record(
            "application.configuration_rolled_back",
            application_id,
            context,
            json!({"snapshot_id": snapshot_id}),
        )
    }

    pub fn incident_context(
        &self,
        query: TimelineQuery,
        application_id: Option<&str>,
    ) -> Result<IncidentContext> {
        let mut report = self.operations.incident(&query)?;
        let truncated = report.evidence.len() > 128;
        report.evidence.truncate(128);
        let mut redacted = 0;
        for signal in &mut report.evidence {
            redact_value(&mut signal.payload, &mut redacted);
        }
        let evidence_references = report
            .evidence
            .iter()
            .map(|signal| signal.id.clone())
            .collect();
        let affected_graph_nodes = if let Some(id) = application_id {
            self.dependency_graph(id)?
                .nodes
                .into_iter()
                .filter(|node| {
                    report
                        .affected_resources
                        .iter()
                        .any(|resource| resource.ends_with(&node.id))
                        || node.id == format!("application:{id}")
                })
                .map(|node| node.id)
                .collect()
        } else {
            Vec::new()
        };
        Ok(IncidentContext {
            schema: "lumic.incident-context.v1".into(),
            report,
            affected_graph_nodes,
            evidence_references,
            redacted_fields: redacted,
            truncated,
        })
    }

    pub async fn analyze_incident(
        &self,
        context: &IncidentContext,
        destination_id: &str,
    ) -> Result<IncidentAnalysis> {
        let destination = self
            .operations
            .destinations()?
            .into_iter()
            .find(|destination| destination.id == destination_id && destination.enabled)
            .ok_or_else(|| invalid("destination", "analysis destination is missing or disabled"))?;
        validate_webhook(&destination)?;
        let body = serde_json::to_vec(&json!({
            "schema": "lumic.incident-analysis-request.v1",
            "context": context,
        }))
        .map_err(state_json)?;
        if body.len() > 256 * 1024 {
            return Err(invalid("incident", "analysis context exceeds 256 KiB"));
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
                        "--fail-with-body",
                        "--max-time",
                        &timeout_seconds,
                        "--request",
                        "POST",
                        "--header",
                        "Content-Type: application/json",
                        "--header",
                        &format!("X-Lumic-Signature: sha256={signature}"),
                        "--data-binary",
                        "@-",
                        "--",
                        &destination.url,
                    ])
                    .stdin(body),
            )
            .await?;
        if !output.success() {
            return Err(LumicError::Process {
                executable: "curl".into(),
                message: String::from_utf8_lossy(&output.stderr).trim().into(),
            });
        }
        let mut analysis: IncidentAnalysis =
            serde_json::from_slice(&output.stdout).map_err(state_json)?;
        if analysis.diagnosis.len() > 8_192
            || analysis.evidence_references.len() > 128
            || analysis.proposed_remediations.len() > 16
        {
            return Err(invalid(
                "analysis",
                "analysis response exceeds structured bounds",
            ));
        }
        if analysis
            .evidence_references
            .iter()
            .any(|reference| !context.evidence_references.contains(reference))
        {
            return Err(invalid(
                "analysis",
                "analysis cites evidence outside the supplied context",
            ));
        }
        for remediation in &analysis.proposed_remediations {
            match remediation {
                lumic_core::intelligence::ProposedRemediation::RestartService { unit } => {
                    validate_systemd_unit(unit)?;
                }
                lumic_core::intelligence::ProposedRemediation::RollbackApplicationConfiguration {
                    application_id,
                    snapshot_id,
                } => {
                    self.applications.inspect(application_id)?;
                    validate_snapshot_id(snapshot_id)?;
                }
            }
        }
        analysis.advisory = true;
        Ok(analysis)
    }

    fn save_snapshot(
        &self,
        application_id: &str,
        path: &Path,
        content: &[u8],
    ) -> Result<ConfigurationSnapshot> {
        let digest = hex_sha256(content);
        let now = lumic_core::application::unix_time_ms();
        let id = format!("cfg-{now}-{}", &digest[..12]);
        let snapshot = ConfigurationSnapshot {
            id: id.clone(),
            application_id: application_id.into(),
            path: path_string(path),
            content: content.to_vec(),
            sha256: digest,
            created_at_unix_ms: now,
        };
        let serialized = serde_json::to_vec_pretty(&snapshot).map_err(state_json)?;
        write_atomic(&self.snapshot_path(&id), &serialized, 0o600)?;
        Ok(snapshot)
    }

    fn snapshot_path(&self, id: &str) -> PathBuf {
        self.state_dir
            .join("intelligence/snapshots")
            .join(format!("{id}.json"))
    }

    fn record(
        &self,
        event_type: &str,
        application_id: &str,
        context: &OperationContext,
        payload: Value,
    ) -> Result<()> {
        self.events.append(&Event::now(
            event_type,
            &context.actor,
            context.interface,
            "application",
            application_id,
            &context.correlation_id,
            payload.clone(),
        ))?;
        self.audit.append(&AuditRecord::now(
            context,
            "application.integrate",
            "apply",
            "application",
            application_id,
            payload,
            None,
            Some(json!({"recorded": true})),
            true,
            event_type,
        ))
    }
}

impl Dotenv {
    fn parse(input: &str) -> Self {
        let mut values = BTreeMap::new();
        let mut duplicates = BTreeSet::new();
        for line in input.lines() {
            if let Some((key, value)) = parse_dotenv_line(line)
                && values.insert(key.clone(), value).is_some()
            {
                duplicates.insert(key);
            }
        }
        Self {
            lines: input.lines().map(ToOwned::to_owned).collect(),
            values,
            duplicates,
            trailing_newline: input.ends_with('\n'),
        }
    }

    fn render(&self, desired: &BTreeMap<String, String>) -> String {
        let mut seen = BTreeSet::new();
        let mut lines = self
            .lines
            .iter()
            .map(|line| {
                if let Some((key, _)) = parse_dotenv_line(line)
                    && let Some(value) = desired.get(&key)
                {
                    seen.insert(key.clone());
                    return format!("{key}={}", quote_dotenv(value));
                }
                line.clone()
            })
            .collect::<Vec<_>>();
        for (key, value) in desired {
            if !seen.contains(key) {
                lines.push(format!("{key}={}", quote_dotenv(value)));
            }
        }
        let mut output = lines.join("\n");
        if self.trailing_newline || !output.is_empty() {
            output.push('\n');
        }
        output
    }
}

fn parse_dotenv_line(line: &str) -> Option<(String, String)> {
    let line = line
        .trim_start()
        .strip_prefix("export ")
        .unwrap_or(line.trim_start());
    if line.starts_with('#') {
        return None;
    }
    let (key, value) = line.split_once('=')?;
    let key = key.trim();
    if key.is_empty()
        || !key.bytes().enumerate().all(|(index, byte)| {
            if index == 0 {
                byte.is_ascii_alphabetic() || byte == b'_'
            } else {
                byte.is_ascii_alphanumeric() || byte == b'_'
            }
        })
    {
        return None;
    }
    Some((key.into(), value.trim().trim_matches(['\'', '"']).into()))
}

fn desired_configuration(dotenv: &Dotenv, host: &str, port: u16) -> BTreeMap<String, String> {
    let cache_key = if dotenv.values.contains_key("CACHE_DRIVER") {
        "CACHE_DRIVER"
    } else {
        "CACHE_STORE"
    };
    [
        ("REDIS_HOST", host.to_owned()),
        ("REDIS_PORT", port.to_string()),
        (cache_key, "redis".into()),
        ("SESSION_DRIVER", "redis".into()),
        ("QUEUE_CONNECTION", "redis".into()),
    ]
    .into_iter()
    .map(|(key, value)| (key.into(), value))
    .collect()
}

fn configuration_diff(
    dotenv: &Dotenv,
    desired: &BTreeMap<String, String>,
) -> Vec<ConfigurationDiff> {
    desired
        .iter()
        .filter(|(key, value)| dotenv.values.get(*key) != Some(*value))
        .map(|(key, _)| ConfigurationDiff {
            key: key.clone(),
            before: if dotenv.values.contains_key(key) {
                "configured".into()
            } else {
                "unset".into()
            },
            after: "configured".into(),
            sensitive: is_sensitive(key),
        })
        .collect()
}

fn graph_for(application: &Application) -> ApplicationDependencyGraph {
    let app = format!("application:{}", application.id);
    let runtime = format!("runtime:{}", runtime_name(application.runtime));
    let web = "web:nginx".to_string();
    let mut graph = ApplicationDependencyGraph {
        application_id: application.id.clone(),
        nodes: vec![
            DependencyNode {
                id: app.clone(),
                kind: DependencyNodeKind::Application,
                label: application.name.clone(),
            },
            DependencyNode {
                id: runtime.clone(),
                kind: DependencyNodeKind::Runtime,
                label: runtime_name(application.runtime).into(),
            },
            DependencyNode {
                id: web.clone(),
                kind: DependencyNodeKind::WebService,
                label: "nginx".into(),
            },
        ],
        edges: vec![
            DependencyEdge {
                from: app.clone(),
                to: runtime,
                relationship: "runs_on".into(),
                evidence: vec!["application runtime declaration".into()],
            },
            DependencyEdge {
                from: app.clone(),
                to: web,
                relationship: "served_by".into(),
                evidence: vec![
                    if application.web_configured {
                        "nginx configuration is active"
                    } else {
                        "nginx is the application web adapter"
                    }
                    .into(),
                ],
            },
        ],
    };
    for reference in &application.service_references {
        add_service_reference_graph(&mut graph, &reference.service_id, &reference.role);
    }
    graph
}

fn add_service_reference_graph(
    graph: &mut ApplicationDependencyGraph,
    service_id: &str,
    role: &str,
) {
    let service = format!("service:{service_id}");
    if graph.nodes.iter().any(|node| node.id == service) {
        return;
    }
    graph.nodes.push(DependencyNode {
        id: service.clone(),
        kind: DependencyNodeKind::ManagedService,
        label: service_id.into(),
    });
    graph.edges.push(DependencyEdge {
        from: format!("application:{}", graph.application_id),
        to: service,
        relationship: "uses".into(),
        evidence: vec![format!(
            "typed application service reference with role {role}"
        )],
    });
}

fn add_redis_graph(graph: &mut ApplicationDependencyGraph, service_id: &str, processes: &[String]) {
    let service = format!("service:{service_id}");
    if !graph.nodes.iter().any(|node| node.id == service) {
        graph.nodes.push(DependencyNode {
            id: service.clone(),
            kind: DependencyNodeKind::ManagedService,
            label: service_id.into(),
        });
        graph.edges.push(DependencyEdge {
            from: format!("application:{}", graph.application_id),
            to: service.clone(),
            relationship: "uses".into(),
            evidence: vec!["Laravel Redis integration".into()],
        });
    }
    for process in processes {
        let id = format!("process:{process}");
        if !graph.nodes.iter().any(|node| node.id == id) {
            graph.nodes.push(DependencyNode {
                id: id.clone(),
                kind: DependencyNodeKind::Process,
                label: process.clone(),
            });
            graph.edges.push(DependencyEdge {
                from: service.clone(),
                to: id,
                relationship: "required_by".into(),
                evidence: vec!["queue worker consumes Redis-backed configuration".into()],
            });
        }
    }
}

fn select_redis(
    services: &[ManagedService],
    requested: Option<&str>,
) -> Result<Option<ManagedService>> {
    if let Some(id) = requested {
        if let Some(service) = services.iter().find(|service| service.id == id) {
            if service.kind != ManagedServiceKind::Redis {
                return Err(invalid("service", "selected service is not Redis"));
            }
            return Ok(Some(service.clone()));
        }
        return Ok(None);
    }
    Ok(services
        .iter()
        .find(|service| service.kind == ManagedServiceKind::Redis)
        .cloned())
}

fn affected_processes(application: &Application) -> Vec<String> {
    application
        .processes
        .iter()
        .filter(|process| {
            process.enabled
                && process.kind == ApplicationProcessKind::Worker
                && process
                    .command
                    .iter()
                    .any(|part| part.contains("horizon") || part.contains("queue"))
        })
        .map(|process| process.name.clone())
        .collect()
}
fn environment_candidates(root: &Path) -> Vec<PathBuf> {
    vec![root.join(".env"), root.join(".env.example")]
}
fn active_environment_path(root: &Path, application_root: &Path) -> Result<PathBuf> {
    let candidate = root.join(".env");
    if !candidate.exists() {
        return Err(invalid("environment", "active .env file was not found"));
    }
    let path = candidate.canonicalize().map_err(state_io)?;
    let allowed = application_root.canonicalize().map_err(state_io)?;
    if !path.starts_with(allowed) {
        return Err(invalid(
            "environment",
            "active .env escapes the managed application root",
        ));
    }
    Ok(path)
}

fn deployed_root(application: &Application) -> Result<PathBuf> {
    let root = PathBuf::from(&application.root);
    let current = root.join("current");
    let selected = if current.exists() {
        current.canonicalize().map_err(state_io)?
    } else {
        root.canonicalize().map_err(state_io)?
    };
    let owned = root.canonicalize().map_err(state_io)?;
    if !selected.starts_with(&owned) {
        return Err(invalid(
            "application",
            "deployed root escapes the managed application root",
        ));
    }
    Ok(selected)
}
fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    let metadata = fs::metadata(path).map_err(state_io)?;
    if !metadata.is_file() || metadata.len() > MAX_DISCOVERY_FILE {
        return Err(invalid(
            "file",
            "discovery files must be regular files no larger than 1 MiB",
        ));
    }
    fs::read(path).map_err(state_io)
}
fn read_json_bounded(path: &Path) -> Result<Value> {
    serde_json::from_slice(&read_bounded(path)?).map_err(state_json)
}
fn runtime_name(runtime: ApplicationRuntime) -> &'static str {
    match runtime {
        ApplicationRuntime::Static => "static",
        ApplicationRuntime::Php => "php",
        ApplicationRuntime::Node => "node",
    }
}
fn quote_dotenv(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    )
}
fn is_sensitive(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "password",
        "passwd",
        "secret",
        "token",
        "credential",
        "private_key",
        "api_key",
        "access_key",
        "app_key",
        "signing_key",
        "authorization",
        "cookie",
    ]
    .iter()
    .any(|term| key.contains(term))
        || key == "key"
        || key.ends_with("_key")
}
fn redact_value(value: &mut Value, count: &mut usize) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if is_sensitive(key) {
                    *value = Value::String("[redacted]".into());
                    *count += 1;
                } else {
                    redact_value(value, count);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                redact_value(value, count);
            }
        }
        _ => {}
    }
}
fn hex_sha256(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
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
    format!("{:x}", outer.finalize())
}
fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
fn validate_snapshot_id(id: &str) -> Result<()> {
    if id.len() > 96
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(invalid("snapshot", "invalid snapshot identifier"));
    }
    Ok(())
}
fn invalid(field: &str, message: &str) -> LumicError {
    LumicError::InvalidInput {
        field: field.into(),
        message: message.into(),
    }
}
fn state_io(error: std::io::Error) -> LumicError {
    LumicError::Internal {
        message: format!("application intelligence I/O failed: {error}"),
    }
}
fn state_json(error: serde_json::Error) -> LumicError {
    LumicError::Internal {
        message: format!("application intelligence data is invalid: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumic_core::OperationInterface;
    use std::sync::atomic::{AtomicU64, Ordering};

    static FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

    fn fixture() -> (PathBuf, PathBuf, OperationContext) {
        let root = std::env::temp_dir().join(format!(
            "lumic-intelligence-{}-{}",
            std::process::id(),
            FIXTURE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let state = root.join("state");
        let apps = root.join("apps");
        let context = OperationContext {
            actor: "test".into(),
            interface: OperationInterface::Internal,
            correlation_id: "intelligence-test".into(),
            dry_run: false,
            approved: true,
        };
        (state, apps, context)
    }

    #[test]
    fn dotenv_update_preserves_comments_and_redacts_preview() {
        let dotenv = Dotenv::parse("# keep me\nAPP_NAME=Lumic\nREDIS_HOST=old\n");
        let desired = desired_configuration(&dotenv, "127.0.0.1", 6379);
        let rendered = dotenv.render(&desired);
        assert!(rendered.starts_with("# keep me\nAPP_NAME=Lumic\n"));
        assert!(rendered.contains("REDIS_HOST=\"127.0.0.1\""));
        let preview = configuration_diff(&dotenv, &desired);
        assert!(preview.iter().all(|change| change.after == "configured"));
        assert!(
            !serde_json::to_string(&preview)
                .unwrap()
                .contains("127.0.0.1")
        );
    }

    #[test]
    fn redaction_walks_nested_incident_payloads() {
        let mut value = json!({"safe": 1, "nested": {"api_token": "value"}});
        let mut count = 0;
        redact_value(&mut value, &mut count);
        assert_eq!(count, 1);
        assert_eq!(value["nested"]["api_token"], "[redacted]");
    }

    #[test]
    fn laravel_fingerprint_and_redis_plan_are_evidence_backed() {
        let (state, apps, context) = fixture();
        let application = ApplicationService::new(&state, &apps)
            .create(
                "shop",
                "shop.example",
                ApplicationRuntime::Php,
                false,
                &context,
            )
            .unwrap();
        let root = PathBuf::from(&application.root);
        fs::write(
            root.join("composer.json"),
            r#"{"require":{"laravel/framework":"^12.0"}}"#,
        )
        .unwrap();
        fs::write(root.join("artisan"), "<?php").unwrap();
        fs::write(root.join(".env"), "APP_NAME=Shop\nAPP_KEY=secret-value\n").unwrap();
        let intelligence = ApplicationIntelligence::new(&state, &apps);

        let fingerprint = intelligence.fingerprint("shop").unwrap();
        assert_eq!(fingerprint.framework.as_deref(), Some("laravel"));
        assert_eq!(fingerprint.confidence, Confidence::High);
        assert!(fingerprint.evidence.len() >= 2);
        let inspection = intelligence.inspect_configuration("shop").unwrap();
        assert!(
            !serde_json::to_string(&inspection)
                .unwrap()
                .contains("secret-value")
        );
        assert!(
            inspection
                .keys
                .iter()
                .any(|key| key.name == "APP_KEY" && key.sensitive)
        );
        let plan = intelligence.plan_laravel_redis("shop", None).unwrap();
        assert!(plan.install_required);
        assert!(
            intelligence
                .plan_integration("unknown@1", "shop", None)
                .is_err()
        );
        assert!(
            plan.configuration_diff
                .iter()
                .any(|item| item.key == "REDIS_HOST")
        );
        assert!(
            plan.dependency_graph
                .nodes
                .iter()
                .any(|node| node.id == "service:redis")
        );
        fs::write(
            root.join(".env"),
            "APP_NAME=Shop\nCACHE_DRIVER=file\nCACHE_DRIVER=array\n",
        )
        .unwrap();
        assert!(intelligence.plan_laravel_redis("shop", None).is_err());
        fs::remove_dir_all(state.parent().unwrap()).unwrap();
    }

    #[test]
    fn owned_configuration_snapshot_round_trips_with_integrity_check() {
        let (state, apps, context) = fixture();
        let application = ApplicationService::new(&state, &apps)
            .create(
                "shop",
                "shop.example",
                ApplicationRuntime::Php,
                false,
                &context,
            )
            .unwrap();
        let env = PathBuf::from(&application.root).join(".env");
        fs::write(&env, "APP_NAME=Before\n").unwrap();
        let intelligence = ApplicationIntelligence::new(&state, &apps);
        let snapshot = intelligence
            .save_snapshot("shop", &env.canonicalize().unwrap(), b"APP_NAME=Before\n")
            .unwrap();
        fs::write(&env, "APP_NAME=After\n").unwrap();
        intelligence
            .restore_snapshot("shop", &snapshot.id, &context)
            .unwrap();
        assert_eq!(fs::read_to_string(&env).unwrap(), "APP_NAME=Before\n");
        fs::remove_dir_all(state.parent().unwrap()).unwrap();
    }
}
