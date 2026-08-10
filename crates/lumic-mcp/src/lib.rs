//! Model Context Protocol adapter over Lumic's typed host capabilities.

mod transport;

pub use transport::{McpHttpCredentialStore, serve_stdio, streamable_http_router};

use lumic_core::{
    HostFacts, OperationContext, OperationInterface,
    application::{
        ApplicationProcess, ApplicationProcessKind, ApplicationRuntime, ApplicationSchedule,
        ApplicationServiceReference, Deployment,
    },
    binding::Binding,
    infrastructure::{
        DeploymentMemberStatus, EnvironmentBundle, EnvironmentTier, EnvironmentTransform,
        MembershipKind, NodeEnrollment, NodeRole, RemoteOperation, ResourceEndpoint,
        SignedRemoteRequest,
    },
    managed_service::{ManagedServiceKind, ServiceConfiguration},
    operations::{
        AutomationAction, AutomationRule, EventSubscription, SignalSeverity, TimelineQuery,
        WebhookDestination,
    },
    package::PackageName,
    recipe::RecipeInstallRequest,
    resource::{ResourceKind, ResourceRef},
    server::{
        FirewallDecision, FirewallRule, NetworkProtocol, ProcessSignal, RemediationAction,
        UpdateScope,
    },
};
use lumic_platform::{
    application::ApplicationService,
    apt::AptPackageManager,
    attention::AttentionService,
    audit_store::AuditStore,
    diagnostics::diagnose_host,
    event_store::EventStore,
    infrastructure::InfrastructureService,
    intelligence::ApplicationIntelligence,
    managed_service::ManagedServiceManager,
    operations::OperationsService,
    recipe::RecipeManager,
    resource_framework::ResourceFramework,
    server::HostOperator,
    software::SoftwareManager,
    systemd::{ServiceAction, SystemdServiceManager},
};
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        ListResourcesResult, PaginatedRequestParams, ReadResourceRequestParams,
        ReadResourceResponse, ReadResourceResult, Resource, ResourceContents, ServerCapabilities,
        ServerInfo,
    },
    service::RequestContext,
    tool, tool_handler, tool_router,
};
use serde::Deserialize;
use std::collections::BTreeMap;

pub const HOST_STATUS_URI: &str = "lumic://server/status";
pub const SERVER_ATTENTION_URI: &str = "lumic://server/attention";

pub fn host_status() -> lumic_core::Result<HostFacts> {
    lumic_platform::inspect_host()
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct ApplicationId {
    /// Stable Lumic application identifier.
    app: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct AttentionRequest {
    /// Relevant event history window, from 1 to 720 hours. Defaults to 24.
    #[serde(default = "default_attention_period")]
    period_hours: u64,
}

const fn default_attention_period() -> u64 {
    24
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct ApplicationIntegration {
    app: String,
    /// Versioned integration definition; defaults to `laravel-redis@1`.
    integration: Option<String>,
    /// Existing Redis service identifier; omit to select one or plan installation of `redis`.
    service: Option<String>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct ApprovedApplicationIntegration {
    app: String,
    /// Versioned integration definition; defaults to `laravel-redis@1`.
    integration: Option<String>,
    service: Option<String>,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct ApprovedConfigurationRollback {
    app: String,
    snapshot: String,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct IncidentContextRequest {
    app: Option<String>,
    since_unix_ms: Option<u128>,
    until_unix_ms: Option<u128>,
    #[serde(default = "default_timeline_limit")]
    limit: usize,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct IncidentAnalysisRequest {
    destination: String,
    app: Option<String>,
    since_unix_ms: Option<u128>,
    until_unix_ms: Option<u128>,
    #[serde(default = "default_timeline_limit")]
    limit: usize,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct InitializeNode {
    id: String,
    name: String,
    /// One or more of: app, worker, database, cache, git, media, backup, edge.
    roles: Vec<String>,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct NodeEndpoint {
    /// HTTPS MCP/API endpoint, or explicit loopback HTTP for local testing.
    endpoint: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct NodeId {
    node: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct ApprovedNode {
    node: String,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct RegisterNode {
    /// Exact JSON returned by node_enrollment on the peer.
    enrollment_json: String,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct CreateHostedRepository {
    repository: String,
    #[serde(default = "default_branch")]
    branch: String,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct SyncRepositoryMirror {
    mirror: String,
    source_url: String,
    #[serde(default = "default_branch")]
    branch: String,
    credential_reference: Option<String>,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct ConfigurePushTrigger {
    repository: String,
    application: String,
    #[serde(default = "default_branch")]
    branch: String,
    #[serde(default = "default_true")]
    enabled: bool,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct ExportEnvironment {
    application: String,
    environment: String,
    /// One of: production, staging, development.
    tier: String,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct GenerateSecret {
    reference: String,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct SetEnvironmentReference {
    application: String,
    name: String,
    reference: String,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct ImportEnvironment {
    /// Exact JSON returned by environment_export.
    bundle_json: String,
    target: String,
    /// One of: production, staging, development.
    tier: String,
    domain: String,
    /// Source environment name to an existing target-local secret reference.
    #[serde(default)]
    environment_reference_overrides: BTreeMap<String, String>,
    /// Source managed-service id to target managed-service id.
    #[serde(default)]
    service_id_overrides: BTreeMap<String, String>,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct DiffEnvironments {
    source_bundle_json: String,
    target_bundle_json: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct RegisterEndpoint {
    id: String,
    provider_node_id: String,
    provider_kind: String,
    provider_id: String,
    consumer_node_id: String,
    consumer_kind: String,
    consumer_id: String,
    /// One of: tcp, http, https.
    protocol: String,
    host: String,
    port: u16,
    health_path: Option<String>,
    secret_reference: Option<String>,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct ConfigureMembership {
    /// One of: worker, reverse_proxy.
    kind: String,
    environment: String,
    application: String,
    node: String,
    #[serde(default = "default_true")]
    enabled: bool,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct BeginCoordination {
    environment: String,
    /// Map of trusted/local node ids to their node-local application ids.
    members: BTreeMap<String, String>,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct ReportCoordination {
    coordination: String,
    node: String,
    /// One of: pending, running, succeeded, failed, rolled_back.
    status: String,
    healthy: Option<bool>,
    deployment: Option<String>,
    message: String,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct SignRemoteOperation {
    target: String,
    /// One of: application.deploy, application.rollback.
    operation: String,
    application: String,
    #[serde(default = "default_remote_ttl")]
    ttl_seconds: u64,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct ApplyRemoteOperation {
    /// Exact JSON returned by remote_operation_sign on a trusted peer.
    signed_request_json: String,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct ServiceUnit {
    /// Validated systemd unit name, for example nginx.service.
    unit: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct CreateApplication {
    name: String,
    domain: String,
    /// One of: static, php, node.
    runtime: String,
    #[serde(default)]
    www: bool,
    /// Must be true after the caller has approved this mutation.
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct SetRepository {
    app: String,
    url: String,
    #[serde(default = "default_branch")]
    branch: String,
    credential_reference: Option<String>,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct ProvisionApplication {
    app: String,
    /// Required for PHP. Supported values: 8.1, 8.2, 8.3, 8.4.
    runtime_version: Option<String>,
    #[serde(default)]
    components: Vec<String>,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct SetHealthCheck {
    app: String,
    #[serde(default = "default_health_path")]
    path: String,
    #[serde(default = "default_health_port")]
    port: u16,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct ApprovedApplication {
    app: String,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct EnableTls {
    app: String,
    email: String,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct ApplyService {
    unit: String,
    /// One of: start, stop, restart, reload, enable, disable.
    action: String,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct PackageInput {
    /// Exact Debian package name from Lumic's trusted package catalog.
    package: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct InstallPackage {
    package: String,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct SoftwareInput {
    /// One of: wordpress, php, mysql, postgresql, redis, typesense, meilisearch, nginx, apache, nodejs, nvm.
    software: String,
    /// Existing Linux account. Required to inspect NVM; ignored by system-scoped installers.
    user: Option<String>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct SetupSoftware {
    /// One of: wordpress, php, mysql, postgresql, redis, typesense, meilisearch, nginx, apache, nodejs, nvm.
    software: String,
    /// Existing Linux account. Required for NVM; ignored by system-scoped installers.
    user: Option<String>,
    #[serde(default)]
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct ManagedServiceId {
    service: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct CatalogSchemaRequest {
    definition: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct ResourceRequest {
    /// Resource kind such as managed_service, application, runtime, or artifact.
    kind: String,
    id: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct OptionalResourceRequest {
    kind: Option<String>,
    id: Option<String>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct ResourcePlanRequest {
    /// install, start, stop, restart, update, or remove.
    action: String,
    service: String,
    definition: Option<String>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct ResourceApplyRequest {
    /// install, start, stop, restart, update, or remove.
    action: String,
    service: String,
    definition: Option<String>,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct CreateResourceBinding {
    id: String,
    producer_kind: String,
    producer_id: String,
    output: String,
    consumer_kind: String,
    consumer_id: String,
    input: String,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct RemoveResourceBinding {
    binding: String,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct OperationRequest {
    operation: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct DetectManagedService {
    /// One of: mysql, postgresql, redis, typesense, meilisearch.
    kind: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct InstallManagedService {
    service: String,
    /// One of: mysql, postgresql, redis, typesense, meilisearch.
    kind: String,
    #[serde(default)]
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct ConfigureManagedService {
    service: String,
    bind_address: String,
    port: u16,
    #[serde(default)]
    settings: BTreeMap<String, String>,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct ManagedServiceAction {
    service: String,
    /// One of: start, stop, restart, update, remove.
    action: String,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct DeclareServiceDependency {
    service: String,
    dependency: String,
    purpose: String,
    #[serde(default = "default_true")]
    required: bool,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct CreateDatabase {
    service: String,
    database: String,
    owner: Option<String>,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct CreateDatabaseUser {
    service: String,
    user: String,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct GrantDatabase {
    service: String,
    database: String,
    user: String,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct BackupService {
    service: String,
    database: Option<String>,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct RestoreService {
    service: String,
    backup: String,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct VerifyBackup {
    backup: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct OperationsTimeline {
    entity: Option<String>,
    entity_id: Option<String>,
    event_type: Option<String>,
    since_unix_ms: Option<u128>,
    until_unix_ms: Option<u128>,
    #[serde(default = "default_timeline_limit")]
    limit: usize,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct ProviderSignalRequest {
    event_type: String,
    entity: String,
    entity_id: String,
    /// One of: info, warning, error, critical.
    severity: String,
    summary: String,
    #[serde(default)]
    payload: serde_json::Value,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct ConfigureWebhook {
    id: String,
    url: String,
    secret_reference: String,
    #[serde(default = "default_webhook_timeout")]
    timeout_ms: u64,
    #[serde(default = "default_webhook_attempts")]
    max_attempts: u8,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct ConfigureSubscription {
    id: String,
    destination_id: String,
    event_types: Vec<String>,
    entity: Option<String>,
    entity_id: Option<String>,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct ConfigureAutomationRule {
    id: String,
    event_type: String,
    entity_id: Option<String>,
    unit: String,
    #[serde(default = "default_rule_cooldown")]
    cooldown_seconds: u64,
    #[serde(default = "default_rule_attempts")]
    max_attempts: u8,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct ApprovedOperation {
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct AttachManagedService {
    app: String,
    service: String,
    role: String,
    database: Option<String>,
    user: Option<String>,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct ConfigureProcess {
    app: String,
    name: String,
    /// One of: worker, schedule.
    kind: String,
    /// Direct executable and argument vector. A shell command string is not accepted.
    command: Vec<String>,
    /// Required for schedule processes; a systemd OnCalendar expression.
    schedule: Option<String>,
    #[serde(default = "default_true")]
    enabled: bool,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct RecipeRequest {
    recipe: String,
    app: String,
    domain: String,
    repository_url: Option<String>,
    #[serde(default = "default_branch")]
    branch: String,
    tls_email: Option<String>,
    #[serde(default)]
    environment: BTreeMap<String, String>,
    #[serde(default)]
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct ApprovedRecipeApplication {
    app: String,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct HostAccountMutation {
    /// One of: user_create, user_delete, group_create, group_add_member.
    action: String,
    name: String,
    member: Option<String>,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct HostFirewallMutation {
    /// One of: allow, deny.
    decision: String,
    port: u16,
    /// One of: tcp, udp.
    protocol: String,
    #[serde(default = "default_any")]
    source: String,
    #[serde(default)]
    remove: bool,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct HostProcessMutation {
    pid: u32,
    /// One of: terminate, kill, hangup.
    signal: String,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct HostPermissionsMutation {
    path: String,
    owner: String,
    group: String,
    /// Octal mode, for example 0750.
    mode: String,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct HostBackupSchedule {
    id: String,
    service: String,
    database: Option<String>,
    on_calendar: String,
    #[serde(default = "default_true")]
    enabled: bool,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct HostUpdateMutation {
    /// One of: security, all.
    scope: String,
    approved: bool,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct HostLogSearch {
    unit: Option<String>,
    priority: Option<String>,
    since: Option<String>,
    query: Option<String>,
    #[serde(default = "default_log_lines")]
    lines: usize,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct HostRemediation {
    /// One of: restart_service, terminate_process, vacuum_journal.
    action: String,
    unit: Option<String>,
    pid: Option<u32>,
    older_than_days: Option<u16>,
    approved: bool,
}

fn default_branch() -> String {
    "main".into()
}

fn default_health_path() -> String {
    "/".into()
}

const fn default_health_port() -> u16 {
    80
}

const fn default_true() -> bool {
    true
}
const fn default_remote_ttl() -> u64 {
    60
}
fn default_any() -> String {
    "any".into()
}
const fn default_log_lines() -> usize {
    100
}

const fn default_timeline_limit() -> usize {
    100
}

const fn default_webhook_timeout() -> u64 {
    5_000
}

const fn default_webhook_attempts() -> u8 {
    3
}

const fn default_rule_cooldown() -> u64 {
    60
}

const fn default_rule_attempts() -> u8 {
    2
}

#[derive(Debug, Clone)]
pub struct LumicMcpServer {
    tool_router: ToolRouter<Self>,
}

impl Default for LumicMcpServer {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router(router = tool_router)]
impl LumicMcpServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        name = "inspect_server",
        description = "Read live Debian/Ubuntu host identity, OS, architecture, CPU, memory, swap and root-disk facts. This operation is read-only and makes no host changes."
    )]
    fn inspect_server(&self) -> Result<String, String> {
        let facts = host_status().map_err(|error| error.to_string())?;
        serde_json::to_string_pretty(&facts).map_err(|error| error.to_string())
    }

    #[tool(
        name = "diagnose_server",
        description = "Read live load, uptime, memory pressure, high-memory processes, failed systemd units, evidence and recovery suggestions. Read-only."
    )]
    async fn diagnose_server(&self) -> Result<String, String> {
        to_json(&diagnose_host().await.map_err(|error| error.to_string())?)
    }

    #[tool(
        name = "server_attention",
        description = "Answer how the node is doing with factual health severity, evidence, recent changes, active incidents, upcoming attention and recommendations. Read-only. The summary object is authoritative; personality only changes the rendered copy and cannot remove warnings."
    )]
    async fn server_attention(
        &self,
        Parameters(request): Parameters<AttentionRequest>,
    ) -> Result<String, String> {
        to_json(
            &attention_service()
                .report(request.period_hours)
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "application_list",
        description = "List Lumic-managed applications from the persistent node state, including domains, runtimes, repository configuration references and health state. Read-only. Repository credentials are never returned."
    )]
    fn application_list(&self) -> Result<String, String> {
        serde_json::to_string_pretty(
            &application_service()
                .list()
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())
    }

    #[tool(
        name = "application_inspect",
        description = "Inspect one Lumic-managed application's runtime, repository reference, health gate, processes, web and TLS state. Read-only; secrets are never returned."
    )]
    fn application_inspect(
        &self,
        Parameters(ApplicationId { app }): Parameters<ApplicationId>,
    ) -> Result<String, String> {
        to_json(&application_service().inspect(&app).map_err(string_error)?)
    }

    #[tool(
        name = "application_fingerprint",
        description = "Detect a managed application's framework/runtime, manifests, dotenv files, workers, scheduler hints and health endpoints with explicit evidence and confidence. Read-only; configuration values are never returned."
    )]
    fn application_fingerprint(
        &self,
        Parameters(ApplicationId { app }): Parameters<ApplicationId>,
    ) -> Result<String, String> {
        to_json(
            &intelligence_service()
                .fingerprint(&app)
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "application_configuration_inspect",
        description = "Inspect an application's active dotenv key names, sensitivity classification and duplicate keys without returning any values. Read-only."
    )]
    fn application_configuration_inspect(
        &self,
        Parameters(ApplicationId { app }): Parameters<ApplicationId>,
    ) -> Result<String, String> {
        to_json(
            &intelligence_service()
                .inspect_configuration(&app)
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "application_dependency_graph",
        description = "Return typed application, runtime, nginx, managed-service and process dependency nodes with evidence-bearing edges. Read-only."
    )]
    fn application_dependency_graph(
        &self,
        Parameters(ApplicationId { app }): Parameters<ApplicationId>,
    ) -> Result<String, String> {
        to_json(
            &intelligence_service()
                .dependency_graph(&app)
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "application_integration_catalog",
        description = "List compiled, versioned application integration definitions and their configuration and verification contracts. Read-only."
    )]
    fn application_integration_catalog(&self) -> Result<String, String> {
        to_json(&intelligence_service().catalog())
    }

    #[tool(
        name = "application_integration_plan",
        description = "Resolve a Laravel-to-Redis integration plan with redacted dotenv diff, affected workers, dependency graph, risks, validation and recovery. Read-only; call before application_integration_apply."
    )]
    fn application_integration_plan(
        &self,
        Parameters(request): Parameters<ApplicationIntegration>,
    ) -> Result<String, String> {
        to_json(
            &intelligence_service()
                .plan_integration(
                    request.integration.as_deref().unwrap_or("laravel-redis@1"),
                    &request.app,
                    request.service.as_deref(),
                )
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "application_integration_apply",
        description = "Apply the reviewed Laravel-to-Redis integration through typed managed-service and application capabilities, snapshot dotenv, restart only affected workers, and verify health. Mutating: requires application.integrate scope and approved=true."
    )]
    async fn application_integration_apply(
        &self,
        Parameters(request): Parameters<ApprovedApplicationIntegration>,
    ) -> Result<String, String> {
        require_scope("application.integrate", request.approved)?;
        to_json(
            &intelligence_service()
                .apply_integration(
                    request.integration.as_deref().unwrap_or("laravel-redis@1"),
                    &request.app,
                    request.service.as_deref(),
                    &operation_context("application_integration_apply"),
                )
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "application_configuration_rollback",
        description = "Restore an integrity-checked Lumic-owned dotenv snapshot for its application. Mutating: requires application.integrate scope and approved=true."
    )]
    fn application_configuration_rollback(
        &self,
        Parameters(request): Parameters<ApprovedConfigurationRollback>,
    ) -> Result<String, String> {
        require_scope("application.integrate", request.approved)?;
        intelligence_service()
            .restore_snapshot(
                &request.app,
                &request.snapshot,
                &operation_context("application_configuration_rollback"),
            )
            .map_err(string_error)?;
        to_json(
            &serde_json::json!({"application_id": request.app, "snapshot_id": request.snapshot, "restored": true}),
        )
    }

    #[tool(
        name = "incident_context",
        description = "Build a bounded, redacted factual incident evidence package and map affected resources onto an optional application's dependency graph. Read-only; it does not assert a root cause."
    )]
    fn incident_context(
        &self,
        Parameters(request): Parameters<IncidentContextRequest>,
    ) -> Result<String, String> {
        to_json(
            &intelligence_service()
                .incident_context(
                    TimelineQuery {
                        entity: None,
                        entity_id: request.app.clone(),
                        event_type: None,
                        since_unix_ms: request.since_unix_ms,
                        until_unix_ms: request.until_unix_ms,
                        limit: request.limit,
                    },
                    request.app.as_deref(),
                )
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "incident_analyze",
        description = "Send a bounded redacted incident context to a configured HTTPS analysis destination and validate its diagnosis, evidence citations, and typed advisory remediations. No remediation is executed. External disclosure requires incident.analyze scope and approved=true."
    )]
    async fn incident_analyze(
        &self,
        Parameters(request): Parameters<IncidentAnalysisRequest>,
    ) -> Result<String, String> {
        require_scope("incident.analyze", request.approved)?;
        let service = intelligence_service();
        let context = service
            .incident_context(
                TimelineQuery {
                    entity: None,
                    entity_id: request.app.clone(),
                    event_type: None,
                    since_unix_ms: request.since_unix_ms,
                    until_unix_ms: request.until_unix_ms,
                    limit: request.limit,
                },
                request.app.as_deref(),
            )
            .map_err(string_error)?;
        to_json(
            &service
                .analyze_incident(&context, &request.destination)
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "application_plan_deployment",
        description = "Resolve the exact release activation change, risks, preconditions, validation and recovery steps for an application. Read-only; call before application_deploy."
    )]
    fn application_plan_deployment(
        &self,
        Parameters(ApplicationId { app }): Parameters<ApplicationId>,
    ) -> Result<String, String> {
        to_json(
            &application_service()
                .plan_deployment(&app)
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "application_deployments",
        description = "Return newest-first deployment history, phases, health result and automatic rollback state for one application. Read-only."
    )]
    fn application_deployments(
        &self,
        Parameters(ApplicationId { app }): Parameters<ApplicationId>,
    ) -> Result<String, String> {
        to_json(
            &application_service()
                .deployments(&app)
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "application_create",
        description = "Create application metadata and managed release directories. Mutating: requires node mutation policy, the mutations scope, and approved=true. Does not install runtime packages or deploy code."
    )]
    fn application_create(
        &self,
        Parameters(request): Parameters<CreateApplication>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        let runtime = parse_runtime(&request.runtime)?;
        to_json(
            &application_service()
                .create(
                    &request.name,
                    &request.domain,
                    runtime,
                    request.www,
                    &operation_context("application_create"),
                )
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "application_set_repository",
        description = "Configure a validated Git source, branch and optional credential reference for an application. Mutating: requires node policy enablement and approved=true. Credential contents are never accepted or returned."
    )]
    fn application_set_repository(
        &self,
        Parameters(request): Parameters<SetRepository>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        to_json(
            &application_service()
                .set_repository(
                    &request.app,
                    &request.url,
                    &request.branch,
                    request.credential_reference,
                    &operation_context("application_set_repository"),
                )
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "application_provision",
        description = "Install the application's explicit runtime/component packages and write a validated, recoverable nginx site configuration. Mutating: requires node policy enablement and approved=true."
    )]
    async fn application_provision(
        &self,
        Parameters(request): Parameters<ProvisionApplication>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        to_json(
            &application_service()
                .provision_versioned(
                    &request.app,
                    request.runtime_version.as_deref(),
                    &request.components,
                    &operation_context("application_provision"),
                )
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "application_set_health_check",
        description = "Configure the local HTTP health gate used after activation and before a deployment is accepted. Mutating: requires node policy enablement and approved=true."
    )]
    fn application_set_health_check(
        &self,
        Parameters(request): Parameters<SetHealthCheck>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        to_json(
            &application_service()
                .set_health_check(
                    &request.app,
                    &request.path,
                    request.port,
                    &operation_context("application_set_health_check"),
                )
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "application_deploy",
        description = "Apply a previously reviewed deployment plan: fetch, build, atomically activate, health-check, and automatically restore the previous release on failure. Mutating: requires node policy enablement and approved=true. Safe to retry after inspection."
    )]
    async fn application_deploy(
        &self,
        Parameters(ApprovedApplication { app, approved }): Parameters<ApprovedApplication>,
    ) -> Result<String, String> {
        require_mutation(approved)?;
        let result: Deployment = application_service()
            .deploy(&app, &operation_context("application_deploy"))
            .await
            .map_err(string_error)?;
        to_json(&result)
    }

    #[tool(
        name = "application_rollback",
        description = "Atomically restore the previous known-good release. Mutating and potentially disruptive: requires node policy enablement and approved=true."
    )]
    fn application_rollback(
        &self,
        Parameters(ApprovedApplication { app, approved }): Parameters<ApprovedApplication>,
    ) -> Result<String, String> {
        require_mutation(approved)?;
        to_json(
            &application_service()
                .rollback(&app, &operation_context("application_rollback"))
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "application_enable_tls",
        description = "Install the trusted Certbot packages, issue a named Let's Encrypt certificate, then atomically attach it to the owned nginx web host with validation, reload, rollback, and a persisted resource binding. Mutating and externally observable: requires node policy enablement and approved=true."
    )]
    async fn application_enable_tls(
        &self,
        Parameters(request): Parameters<EnableTls>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        to_json(
            &application_service()
                .enable_tls(
                    &request.app,
                    &request.email,
                    &operation_context("application_enable_tls"),
                )
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "application_configure_process",
        description = "Write and activate a validated systemd worker or timer using a direct argument vector. Mutating: requires node policy enablement and approved=true. Never invokes a shell."
    )]
    async fn application_configure_process(
        &self,
        Parameters(request): Parameters<ConfigureProcess>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        let kind = match request.kind.as_str() {
            "worker" => ApplicationProcessKind::Worker,
            "schedule" => ApplicationProcessKind::Schedule,
            _ => return Err("kind must be one of: worker, schedule".into()),
        };
        let process = ApplicationProcess {
            name: request.name,
            kind,
            command: request.command,
            schedule: request.schedule.map(ApplicationSchedule::calendar),
            enabled: request.enabled,
        };
        to_json(
            &application_service()
                .add_process(
                    &request.app,
                    process,
                    &operation_context("application_configure_process"),
                )
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "infrastructure_status",
        description = "Read the local node identity and the persistent cross-node read model: trusted peers, Git repositories/mirrors/triggers, environments, explicit endpoints, worker/proxy memberships, and coordinated deployments. Read-only; secrets and signing keys are never returned."
    )]
    fn infrastructure_status(&self) -> Result<String, String> {
        to_json(
            &infrastructure_service()
                .read_model()
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "node_initialize",
        description = "Create this node's stable identity and private Ed25519 signing key with explicit roles. One-time mutation: requires node policy enablement and approved=true."
    )]
    fn node_initialize(
        &self,
        Parameters(request): Parameters<InitializeNode>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        let roles = request
            .roles
            .iter()
            .map(|role| NodeRole::parse(role).map_err(string_error))
            .collect::<Result<Vec<_>, _>>()?;
        to_json(
            &infrastructure_service()
                .initialize_node(
                    &request.id,
                    &request.name,
                    roles,
                    &operation_context("node_initialize"),
                )
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "node_enrollment",
        description = "Export this node's non-secret identity, HTTPS endpoint and Ed25519 verification key for explicit exchange with another Lumic node. Read-only."
    )]
    fn node_enrollment(
        &self,
        Parameters(NodeEndpoint { endpoint }): Parameters<NodeEndpoint>,
    ) -> Result<String, String> {
        to_json(
            &infrastructure_service()
                .enrollment(&endpoint)
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "node_register",
        description = "Validate and trust a peer's public enrollment package. Mutating trust boundary: requires node policy enablement and approved=true. Private signing material is never exchanged."
    )]
    fn node_register(
        &self,
        Parameters(request): Parameters<RegisterNode>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        let enrollment: NodeEnrollment =
            serde_json::from_str(&request.enrollment_json).map_err(string_error)?;
        to_json(
            &infrastructure_service()
                .register_node(enrollment, &operation_context("node_register"))
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "node_revoke",
        description = "Revoke a peer so future signed remote requests from it are rejected. Mutating trust boundary: requires node policy enablement and approved=true."
    )]
    fn node_revoke(
        &self,
        Parameters(ApprovedNode { node, approved }): Parameters<ApprovedNode>,
    ) -> Result<String, String> {
        require_mutation(approved)?;
        to_json(
            &infrastructure_service()
                .revoke_node(&node, &operation_context("node_revoke"))
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "node_health",
        description = "Check whether a trusted peer's declared Lumic endpoint accepts a bounded TCP connection and persist the evidence. Read-only with respect to the remote node."
    )]
    async fn node_health(
        &self,
        Parameters(NodeId { node }): Parameters<NodeId>,
    ) -> Result<String, String> {
        to_json(
            &infrastructure_service()
                .check_node_health(&node)
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "git_repository_host",
        description = "Create an idempotent native bare Git repository under Lumic state using direct git argv. Mutating: requires node policy enablement and approved=true."
    )]
    async fn git_repository_host(
        &self,
        Parameters(request): Parameters<CreateHostedRepository>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        to_json(
            &infrastructure_service()
                .create_hosted_repository(
                    &request.repository,
                    &request.branch,
                    &operation_context("git_repository_host"),
                )
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "git_mirror_sync",
        description = "Create or refresh a native bare Git mirror with an optional imported credential reference. Mutating: requires node policy enablement and approved=true."
    )]
    async fn git_mirror_sync(
        &self,
        Parameters(request): Parameters<SyncRepositoryMirror>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        to_json(
            &infrastructure_service()
                .sync_mirror(
                    &request.mirror,
                    &request.source_url,
                    &request.branch,
                    request.credential_reference,
                    &operation_context("git_mirror_sync"),
                )
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "git_push_deploy_configure",
        description = "Attach a fixed post-receive hook that invokes only Lumic's validated receive capability for one branch/application mapping. Mutating: requires node policy enablement and approved=true."
    )]
    fn git_push_deploy_configure(
        &self,
        Parameters(request): Parameters<ConfigurePushTrigger>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        to_json(
            &infrastructure_service()
                .set_push_trigger(
                    &request.repository,
                    &request.application,
                    &request.branch,
                    request.enabled,
                    &operation_context("git_push_deploy_configure"),
                )
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "environment_secret_generate",
        description = "Generate a private random target-local secret and return only its stable reference. Mutating: requires policy enablement and approved=true. Secret values are never returned or logged."
    )]
    fn environment_secret_generate(
        &self,
        Parameters(request): Parameters<GenerateSecret>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        to_json(
            &infrastructure_service()
                .generate_secret(
                    &request.reference,
                    &operation_context("environment_secret_generate"),
                )
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "application_environment_reference_set",
        description = "Attach an existing target-local secret reference to an application environment name. Mutating: requires policy enablement and approved=true. Fails closed for missing secrets."
    )]
    fn application_environment_reference_set(
        &self,
        Parameters(request): Parameters<SetEnvironmentReference>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        to_json(
            &application_service()
                .set_environment_reference(
                    &request.application,
                    &request.name,
                    &request.reference,
                    &operation_context("application_environment_reference_set"),
                )
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "environment_export",
        description = "Export a versioned portable application/environment bundle containing runtime, repository, health, processes, managed-service references and secret references—but never secret values. Mutating only to persist the named environment snapshot; requires policy enablement and approved=true."
    )]
    fn environment_export(
        &self,
        Parameters(request): Parameters<ExportEnvironment>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        to_json(
            &infrastructure_service()
                .export_environment(
                    &request.application,
                    &request.environment,
                    EnvironmentTier::parse(&request.tier).map_err(string_error)?,
                    &operation_context("environment_export"),
                )
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "environment_import",
        description = "Clone a portable bundle into a target application/environment with explicit tier, domain, target-local secret-reference and service-id transforms. Mutating: requires policy enablement and approved=true. Missing target secrets fail closed."
    )]
    fn environment_import(
        &self,
        Parameters(request): Parameters<ImportEnvironment>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        let bundle: EnvironmentBundle =
            serde_json::from_str(&request.bundle_json).map_err(string_error)?;
        to_json(
            &infrastructure_service()
                .import_environment(
                    &bundle,
                    &EnvironmentTransform {
                        target_id: request.target,
                        target_tier: EnvironmentTier::parse(&request.tier).map_err(string_error)?,
                        target_domain: request.domain,
                        environment_reference_overrides: request.environment_reference_overrides,
                        service_id_overrides: request.service_id_overrides,
                    },
                    &operation_context("environment_import"),
                )
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "environment_diff",
        description = "Compare two portable environment bundles. Returns domain, tier, runtime, service mapping and redacted secret-reference differences. Read-only."
    )]
    fn environment_diff(
        &self,
        Parameters(request): Parameters<DiffEnvironments>,
    ) -> Result<String, String> {
        let source: EnvironmentBundle =
            serde_json::from_str(&request.source_bundle_json).map_err(string_error)?;
        let target: EnvironmentBundle =
            serde_json::from_str(&request.target_bundle_json).map_err(string_error)?;
        to_json(&infrastructure_service().diff_environments(&source, &target))
    }

    #[tool(
        name = "resource_endpoint_register",
        description = "Register an explicit producer-to-consumer service/application endpoint with protocol, host, port, health path and optional secret reference. Mutating: requires policy enablement and approved=true."
    )]
    fn resource_endpoint_register(
        &self,
        Parameters(request): Parameters<RegisterEndpoint>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        to_json(
            &infrastructure_service()
                .register_endpoint(
                    ResourceEndpoint {
                        id: request.id,
                        provider_node_id: request.provider_node_id,
                        provider_kind: request.provider_kind,
                        provider_id: request.provider_id,
                        consumer_node_id: request.consumer_node_id,
                        consumer_kind: request.consumer_kind,
                        consumer_id: request.consumer_id,
                        protocol: request.protocol,
                        host: request.host,
                        port: request.port,
                        health_path: request.health_path,
                        secret_reference: request.secret_reference,
                    },
                    &operation_context("resource_endpoint_register"),
                )
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "node_membership_configure",
        description = "Declare explicit worker or reverse-proxy membership for an application environment. Mutating: requires policy enablement and approved=true. This is topology state, not an implicit scheduler."
    )]
    fn node_membership_configure(
        &self,
        Parameters(request): Parameters<ConfigureMembership>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        to_json(
            &infrastructure_service()
                .register_membership(
                    MembershipKind::parse(&request.kind).map_err(string_error)?,
                    &request.environment,
                    &request.application,
                    &request.node,
                    request.enabled,
                    &operation_context("node_membership_configure"),
                )
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "coordinated_deployment_begin",
        description = "Create an externally orchestrated deployment wave across explicit local/trusted node members. It records the stop-and-rollback failure boundary but performs no hidden remote mutation. Requires policy enablement and approved=true."
    )]
    fn coordinated_deployment_begin(
        &self,
        Parameters(request): Parameters<BeginCoordination>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        to_json(
            &infrastructure_service()
                .begin_coordination(
                    &request.environment,
                    request.members.into_iter().collect(),
                    &operation_context("coordinated_deployment_begin"),
                )
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "coordinated_deployment_report",
        description = "Record one node-local deployment result and health outcome. The coordination succeeds only when all members explicitly report healthy success; first failure closes the wave as failed. Requires policy enablement and approved=true."
    )]
    fn coordinated_deployment_report(
        &self,
        Parameters(request): Parameters<ReportCoordination>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        let status = parse_member_status(&request.status)?;
        to_json(
            &infrastructure_service()
                .report_coordination_member(
                    &request.coordination,
                    &request.node,
                    status,
                    request.healthy,
                    request.deployment,
                    request.message,
                    &operation_context("coordinated_deployment_report"),
                )
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "remote_operation_sign",
        description = "Create a short-lived Ed25519-signed typed request for application.deploy or application.rollback on a target node. Read-only locally; does not contact or mutate the target."
    )]
    fn remote_operation_sign(
        &self,
        Parameters(request): Parameters<SignRemoteOperation>,
    ) -> Result<String, String> {
        to_json(
            &infrastructure_service()
                .sign_remote_request(
                    &request.target,
                    RemoteOperation {
                        kind: request.operation,
                        resource_id: request.application,
                        arguments: BTreeMap::new(),
                    },
                    request.ttl_seconds,
                )
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "remote_operation_apply",
        description = "Verify origin trust, target, expiry, Ed25519 signature, nonce replay protection and the closed operation allowlist, then invoke the normal node-local deploy/rollback contract. Mutating: requires policy enablement and approved=true."
    )]
    async fn remote_operation_apply(
        &self,
        Parameters(request): Parameters<ApplyRemoteOperation>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        let signed: SignedRemoteRequest =
            serde_json::from_str(&request.signed_request_json).map_err(string_error)?;
        to_json(
            &infrastructure_service()
                .execute_remote_request(&signed, &operation_context("remote_operation_apply"))
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "resource_catalog",
        description = "List trusted service, runtime, and application catalog definitions, including capabilities, schemas, outputs, and platform mappings. Read-only."
    )]
    fn resource_catalog(&self) -> Result<String, String> {
        to_json(&resource_framework().catalog().map_err(string_error)?)
    }

    #[tool(
        name = "resource_schema",
        description = "Return one trusted service definition and the exact shared configuration schema used by CLI, UI, MCP, and its driver. Read-only."
    )]
    fn resource_schema(
        &self,
        Parameters(request): Parameters<CatalogSchemaRequest>,
    ) -> Result<String, String> {
        to_json(
            &resource_framework()
                .service_schema(&request.definition)
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "resource_plan",
        description = "Resolve a catalog-driven install or typed lifecycle plan without mutation. Use resource_apply only after reviewing it."
    )]
    async fn resource_plan(
        &self,
        Parameters(request): Parameters<ResourcePlanRequest>,
    ) -> Result<String, String> {
        let manager = managed_service_manager();
        let context = planning_context("resource_plan");
        match request.action.as_str() {
            "install" => to_json(
                &manager
                    .plan_catalog_install(
                        &request.service,
                        request
                            .definition
                            .as_deref()
                            .ok_or("definition is required for install")?,
                    )
                    .map_err(string_error)?,
            ),
            "start" => to_json(
                &manager
                    .lifecycle(&request.service, ServiceAction::Start, &context)
                    .await
                    .map_err(string_error)?,
            ),
            "stop" => to_json(
                &manager
                    .lifecycle(&request.service, ServiceAction::Stop, &context)
                    .await
                    .map_err(string_error)?,
            ),
            "restart" => to_json(
                &manager
                    .lifecycle(&request.service, ServiceAction::Restart, &context)
                    .await
                    .map_err(string_error)?,
            ),
            "update" => to_json(
                &manager
                    .update(&request.service, &context)
                    .await
                    .map_err(string_error)?,
            ),
            "remove" => to_json(
                &manager
                    .remove(&request.service, false, &context)
                    .await
                    .map_err(string_error)?,
            ),
            _ => Err("action must be install, start, stop, restart, update, or remove".into()),
        }
    }

    #[tool(
        name = "resource_apply",
        description = "Apply a catalog install or a typed start, stop, restart, update, or remove action. Mutating: requires node policy enablement and approved=true."
    )]
    async fn resource_apply(
        &self,
        Parameters(request): Parameters<ResourceApplyRequest>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        let manager = managed_service_manager();
        let context = operation_context("resource_apply");
        let mutation = match request.action.as_str() {
            "install" => {
                manager
                    .install_catalog(
                        &request.service,
                        request
                            .definition
                            .as_deref()
                            .ok_or("definition is required for install")?,
                        &context,
                    )
                    .await
            }
            "start" => {
                manager
                    .lifecycle(&request.service, ServiceAction::Start, &context)
                    .await
            }
            "stop" => {
                manager
                    .lifecycle(&request.service, ServiceAction::Stop, &context)
                    .await
            }
            "restart" => {
                manager
                    .lifecycle(&request.service, ServiceAction::Restart, &context)
                    .await
            }
            "update" => manager.update(&request.service, &context).await,
            "remove" => manager.remove(&request.service, false, &context).await,
            _ => {
                return Err(
                    "action must be install, start, stop, restart, update, or remove".into(),
                );
            }
        };
        to_json(&mutation.map_err(string_error)?)
    }

    #[tool(
        name = "resource_inspect",
        description = "Inspect one persisted resource with secret values redacted. Read-only."
    )]
    fn resource_inspect(
        &self,
        Parameters(request): Parameters<ResourceRequest>,
    ) -> Result<String, String> {
        let resource = resource_ref(&request.kind, &request.id)?;
        to_json(
            &resource_framework()
                .inspect(&resource)
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "resource_bindings",
        description = "List explicit producer-output to consumer-input bindings, optionally filtered to one resource. Read-only."
    )]
    fn resource_bindings(
        &self,
        Parameters(request): Parameters<OptionalResourceRequest>,
    ) -> Result<String, String> {
        let resource = optional_resource_ref(request.kind.as_deref(), request.id.as_deref())?;
        to_json(
            &resource_framework()
                .bindings(resource.as_ref())
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "resource_binding_apply",
        description = "Create an explicit validated resource binding. Rejects missing outputs, duplicate consumer inputs, and cycles. Mutating: requires policy enablement and approved=true."
    )]
    fn resource_binding_apply(
        &self,
        Parameters(request): Parameters<CreateResourceBinding>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        let binding = Binding {
            id: request.id,
            producer: resource_ref(&request.producer_kind, &request.producer_id)?,
            output: request.output,
            consumer: resource_ref(&request.consumer_kind, &request.consumer_id)?,
            input: request.input,
            created_at_unix_ms: current_unix_ms(),
        };
        to_json(
            &resource_framework()
                .bind(binding, false)
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "resource_binding_remove",
        description = "Remove one explicit resource binding by stable id. Mutating: requires policy enablement and approved=true."
    )]
    fn resource_binding_remove(
        &self,
        Parameters(request): Parameters<RemoveResourceBinding>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        to_json(
            &resource_framework()
                .unbind(&request.binding, false)
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "resource_operations",
        description = "List durable pipeline operation journals, optionally filtered to one target resource. Read-only and suitable for progress/failure monitoring."
    )]
    fn resource_operations(
        &self,
        Parameters(request): Parameters<OptionalResourceRequest>,
    ) -> Result<String, String> {
        let resource = optional_resource_ref(request.kind.as_deref(), request.id.as_deref())?;
        to_json(
            &resource_framework()
                .operations(resource.as_ref())
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "resource_operation_inspect",
        description = "Inspect one durable pipeline operation and every step outcome/message. Read-only."
    )]
    fn resource_operation_inspect(
        &self,
        Parameters(request): Parameters<OperationRequest>,
    ) -> Result<String, String> {
        to_json(
            &resource_framework()
                .operation(&request.operation)
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "managed_service_list",
        description = "List Lumic-managed native services and their desired configuration. Read-only; secret values are never returned."
    )]
    fn managed_service_list(&self) -> Result<String, String> {
        to_json(&managed_service_manager().list().map_err(string_error)?)
    }

    #[tool(
        name = "managed_service_detect",
        description = "Detect a native MySQL, PostgreSQL, Redis, Typesense, or Meilisearch package, systemd state, version and provider health without adopting or changing it. Read-only."
    )]
    async fn managed_service_detect(
        &self,
        Parameters(request): Parameters<DetectManagedService>,
    ) -> Result<String, String> {
        to_json(
            &managed_service_manager()
                .detect(parse_managed_kind(&request.kind)?)
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "managed_service_inspect",
        description = "Inspect a managed service with live package version, systemd state, provider health, ports and expert paths. Read-only."
    )]
    async fn managed_service_inspect(
        &self,
        Parameters(ManagedServiceId { service }): Parameters<ManagedServiceId>,
    ) -> Result<String, String> {
        to_json(
            &managed_service_manager()
                .inspect(&service)
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "managed_service_plan_install",
        description = "Resolve native package, systemd, health validation, risk and recovery steps for MySQL, PostgreSQL, Redis, Typesense, or Meilisearch. Read-only; call before managed_service_install."
    )]
    fn managed_service_plan_install(
        &self,
        Parameters(request): Parameters<InstallManagedService>,
    ) -> Result<String, String> {
        to_json(
            &managed_service_manager()
                .plan_install(&request.service, parse_managed_kind(&request.kind)?)
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "managed_service_install",
        description = "Install and reconcile one MySQL, PostgreSQL, Redis, Typesense, or Meilisearch service through apt, validated configuration, generated secrets where required, systemd, and a provider health gate. Mutating: requires node policy enablement and approved=true."
    )]
    async fn managed_service_install(
        &self,
        Parameters(request): Parameters<InstallManagedService>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        to_json(
            &managed_service_manager()
                .install(
                    &request.service,
                    parse_managed_kind(&request.kind)?,
                    &operation_context("managed_service_install"),
                )
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "managed_service_configure",
        description = "Apply a loopback-only, provider-allowlisted configuration and restart with health-gated rollback. Mutating: requires node policy enablement and approved=true."
    )]
    async fn managed_service_configure(
        &self,
        Parameters(request): Parameters<ConfigureManagedService>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        let configuration = ServiceConfiguration {
            bind_address: request.bind_address,
            port: request.port,
            settings: request.settings,
        };
        to_json(
            &managed_service_manager()
                .configure(
                    &request.service,
                    configuration,
                    &operation_context("managed_service_configure"),
                )
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "managed_service_apply",
        description = "Start, stop, restart, update or remove a managed service through typed native operations. Remove retains native data. Mutating: requires node policy enablement and approved=true."
    )]
    async fn managed_service_apply(
        &self,
        Parameters(request): Parameters<ManagedServiceAction>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        let manager = managed_service_manager();
        let context = operation_context("managed_service_apply");
        let mutation = match request.action.as_str() {
            "start" => {
                manager
                    .lifecycle(&request.service, ServiceAction::Start, &context)
                    .await
            }
            "stop" => {
                manager
                    .lifecycle(&request.service, ServiceAction::Stop, &context)
                    .await
            }
            "restart" => {
                manager
                    .lifecycle(&request.service, ServiceAction::Restart, &context)
                    .await
            }
            "update" => manager.update(&request.service, &context).await,
            "remove" => manager.remove(&request.service, false, &context).await,
            _ => return Err("action must be one of: start, stop, restart, update, remove".into()),
        };
        to_json(&mutation.map_err(string_error)?)
    }

    #[tool(
        name = "managed_service_declare_dependency",
        description = "Declare a typed relationship between two managed services for impact inspection. Mutating Lumic state: requires node policy enablement and approved=true."
    )]
    fn managed_service_declare_dependency(
        &self,
        Parameters(request): Parameters<DeclareServiceDependency>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        to_json(
            &managed_service_manager()
                .declare_dependency(
                    &request.service,
                    &request.dependency,
                    &request.purpose,
                    request.required,
                    &operation_context("managed_service_declare_dependency"),
                )
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "managed_service_database_create",
        description = "Create an idempotently tracked PostgreSQL database using a validated identifier. Mutating: requires node policy enablement and approved=true."
    )]
    async fn managed_service_database_create(
        &self,
        Parameters(request): Parameters<CreateDatabase>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        to_json(
            &managed_service_manager()
                .create_database(
                    &request.service,
                    &request.database,
                    request.owner.as_deref(),
                    &operation_context("managed_service_database_create"),
                )
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "managed_service_user_create",
        description = "Create a PostgreSQL login and store its generated password in Lumic's private secret store. Returns only the secret reference. Mutating: requires node policy enablement and approved=true."
    )]
    async fn managed_service_user_create(
        &self,
        Parameters(request): Parameters<CreateDatabaseUser>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        to_json(
            &managed_service_manager()
                .create_database_user(
                    &request.service,
                    &request.user,
                    &operation_context("managed_service_user_create"),
                )
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "managed_service_database_grant",
        description = "Grant a managed PostgreSQL user access to a managed database. Mutating: requires node policy enablement and approved=true."
    )]
    async fn managed_service_database_grant(
        &self,
        Parameters(request): Parameters<GrantDatabase>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        to_json(
            &managed_service_manager()
                .grant_database(
                    &request.service,
                    &request.database,
                    &request.user,
                    &operation_context("managed_service_database_grant"),
                )
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "managed_service_backup",
        description = "Create a local MySQL/PostgreSQL database dump or Redis snapshot and record it in service history. Mutating: requires node policy enablement and approved=true."
    )]
    async fn managed_service_backup(
        &self,
        Parameters(request): Parameters<BackupService>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        to_json(
            &managed_service_manager()
                .backup(
                    &request.service,
                    request.database.as_deref(),
                    &operation_context("managed_service_backup"),
                )
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "managed_service_backup_verify",
        description = "Verify that a managed backup exists and matches its recorded size, SHA-256 checksum, and native MySQL/PostgreSQL/Redis format header. Read-only and safe before restore."
    )]
    fn managed_service_backup_verify(
        &self,
        Parameters(VerifyBackup { backup }): Parameters<VerifyBackup>,
    ) -> Result<String, String> {
        to_json(
            &managed_service_manager()
                .verify_backup(&backup)
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "managed_service_restore",
        description = "Restore a recorded local service backup. Disruptive: requires node policy enablement and approved=true. Inspect the backup and service first."
    )]
    async fn managed_service_restore(
        &self,
        Parameters(request): Parameters<RestoreService>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        to_json(
            &managed_service_manager()
                .restore(
                    &request.service,
                    &request.backup,
                    &operation_context("managed_service_restore"),
                )
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "application_attach_managed_service",
        description = "Attach a typed database/user pair or reusable search endpoint/credential reference to an application. Secret values remain in the node store. Mutating: requires node policy enablement and approved=true."
    )]
    fn application_attach_managed_service(
        &self,
        Parameters(request): Parameters<AttachManagedService>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        to_json(
            &managed_service_manager()
                .attach_to_application(
                    &application_service(),
                    &request.app,
                    ApplicationServiceReference {
                        service_id: request.service,
                        role: request.role,
                        database: request.database,
                        user: request.user,
                        secret_reference: None,
                    },
                    &operation_context("application_attach_managed_service"),
                )
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "package_inspect",
        description = "Inspect installed and candidate versions for one validated Debian package name. Read-only."
    )]
    async fn package_inspect(
        &self,
        Parameters(PackageInput { package }): Parameters<PackageInput>,
    ) -> Result<String, String> {
        let package = PackageName::parse(package).map_err(string_error)?;
        to_json(
            &AptPackageManager::system(event_store())
                .inspect(&package)
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "package_install",
        description = "Install one exact package authorized by Lumic's built-in catalog through apt argument vectors. Mutating: requires node policy enablement and approved=true."
    )]
    async fn package_install(
        &self,
        Parameters(request): Parameters<InstallPackage>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        let package = PackageName::parse(request.package).map_err(string_error)?;
        to_json(
            &AptPackageManager::system(event_store())
                .install(&package, &operation_context("package_install"))
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "software_catalog",
        description = "List Lumic's default supported software installers, scopes, descriptions, and prerequisites. Read-only."
    )]
    fn software_catalog(&self) -> Result<String, String> {
        to_json(lumic_core::software::SOFTWARE_CATALOG)
    }

    #[tool(
        name = "software_status",
        description = "Inspect installed and candidate versions for supported software. NVM inspection requires a target Linux user. Read-only."
    )]
    async fn software_status(
        &self,
        Parameters(SoftwareInput { software, user }): Parameters<SoftwareInput>,
    ) -> Result<String, String> {
        to_json(
            &software_manager()
                .status_for_user(&software, user.as_deref())
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "software_plan_setup",
        description = "Resolve setup into exact native packages or pinned per-user NVM actions, risks, preconditions, validation, and recovery. Read-only."
    )]
    async fn software_plan_setup(
        &self,
        Parameters(SoftwareInput { software, user }): Parameters<SoftwareInput>,
    ) -> Result<String, String> {
        to_json(
            &software_manager()
                .plan_setup_for_user(&software, user.as_deref())
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "software_setup",
        description = "Install or idempotently reconcile supported software using its fixed installer contract. Mutating: requires approved=true and node policy enablement."
    )]
    async fn software_setup(
        &self,
        Parameters(request): Parameters<SetupSoftware>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        to_json(
            &software_manager()
                .setup_for_user(
                    &request.software,
                    request.user.as_deref(),
                    &operation_context("software_setup"),
                )
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "service_inspect",
        description = "Inspect a validated systemd unit's load, active, sub and enablement state. Read-only."
    )]
    async fn service_inspect(
        &self,
        Parameters(ServiceUnit { unit }): Parameters<ServiceUnit>,
    ) -> Result<String, String> {
        to_json(
            &SystemdServiceManager::at_state_dir(state_directory())
                .inspect(&unit)
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "service_apply",
        description = "Apply one validated systemd lifecycle action without shell execution. Mutating and potentially disruptive: requires node policy enablement and approved=true. Inspect first."
    )]
    async fn service_apply(
        &self,
        Parameters(request): Parameters<ApplyService>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        let action = parse_service_action(&request.action)?;
        to_json(
            &SystemdServiceManager::at_state_dir(state_directory())
                .apply(&request.unit, action, &operation_context("service_apply"))
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "recipe_catalog",
        description = "List the built-in, validated and versioned application recipe catalog. Read-only; use recipe_plan before recipe_install."
    )]
    fn recipe_catalog(&self) -> Result<String, String> {
        to_json(recipe_manager().catalog())
    }

    #[tool(
        name = "recipe_installations",
        description = "List installed application recipes and their versions, managed services and secret references. Secret values are never returned. Read-only."
    )]
    fn recipe_installations(&self) -> Result<String, String> {
        to_json(&recipe_manager().list().map_err(string_error)?)
    }

    #[tool(
        name = "recipe_plan",
        description = "Resolve a recipe installation or reconciliation into exact changes, risks, preconditions and recovery guidance. Read-only."
    )]
    fn recipe_plan(
        &self,
        Parameters(request): Parameters<RecipeRequest>,
    ) -> Result<String, String> {
        to_json(
            &recipe_manager()
                .plan_install(&recipe_request(request))
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "recipe_install",
        description = "Install or idempotently reconcile a versioned recipe through Lumic applications, managed services, secret references, systemd, nginx and TLS. Mutating: requires approved=true and node policy enablement."
    )]
    async fn recipe_install(
        &self,
        Parameters(request): Parameters<RecipeRequest>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        to_json(
            &recipe_manager()
                .install(
                    &recipe_request(request),
                    &operation_context("recipe_install"),
                )
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "recipe_update",
        description = "Reconcile an installed recipe to the current catalog version. Mutating: requires approved=true and node policy enablement."
    )]
    async fn recipe_update(
        &self,
        Parameters(request): Parameters<ApprovedRecipeApplication>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        to_json(
            &recipe_manager()
                .update(&request.app, &operation_context("recipe_update"))
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "recipe_uninstall",
        description = "Uninstall a recipe, disable its application, retain releases for recovery, and remove generated secret material. Mutating: requires approved=true and node policy enablement."
    )]
    fn recipe_uninstall(
        &self,
        Parameters(request): Parameters<ApprovedRecipeApplication>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        to_json(
            &recipe_manager()
                .uninstall(&request.app, &operation_context("recipe_uninstall"))
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "host_operator_snapshot",
        description = "Read users, groups, firewall, listeners, mounts, processes, systemd timers, pending updates and backup schedules as one operator snapshot. Read-only."
    )]
    async fn host_operator_snapshot(&self) -> Result<String, String> {
        to_json(&host_operator().snapshot().await.map_err(string_error)?)
    }

    #[tool(
        name = "host_search_logs",
        description = "Search the systemd journal with bounded typed filters for unit, priority, time and text. Read-only."
    )]
    async fn host_search_logs(
        &self,
        Parameters(request): Parameters<HostLogSearch>,
    ) -> Result<String, String> {
        host_operator()
            .search_journal(
                request.unit.as_deref(),
                request.priority.as_deref(),
                request.since.as_deref(),
                request.query.as_deref(),
                request.lines,
            )
            .await
            .map_err(string_error)
    }

    #[tool(
        name = "host_account_apply",
        description = "Create/delete a user or group, or add a group member through validated direct argv. Mutating: inspect first; requires approved=true and node policy enablement."
    )]
    async fn host_account_apply(
        &self,
        Parameters(request): Parameters<HostAccountMutation>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        let operator = host_operator();
        let context = operation_context("host_account_apply");
        let result = match request.action.as_str() {
            "user_create" => operator.create_user(&request.name, &context).await,
            "user_delete" => operator.delete_user(&request.name, &context).await,
            "group_create" => operator.create_group(&request.name, &context).await,
            "group_delete" => operator.delete_group(&request.name, &context).await,
            "group_add_member" => {
                operator
                    .add_group_member(
                        &request.name,
                        request
                            .member
                            .as_deref()
                            .ok_or("member is required for group_add_member")?,
                        &context,
                    )
                    .await
            }
            _ => return Err(
                "action must be one of: user_create, user_delete, group_create, group_delete, group_add_member".into(),
            ),
        }
        .map_err(string_error)?;
        to_json(&result)
    }

    #[tool(
        name = "host_permissions_apply",
        description = "Set validated owner, group and octal permissions on an absolute non-root path. Mutating: requires approved=true and node policy enablement."
    )]
    async fn host_permissions_apply(
        &self,
        Parameters(request): Parameters<HostPermissionsMutation>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        let mode = u32::from_str_radix(request.mode.trim_start_matches("0o"), 8)
            .map_err(|_| "mode must be octal, for example 0750")?;
        to_json(
            &host_operator()
                .set_permissions(
                    std::path::Path::new(&request.path),
                    &request.owner,
                    &request.group,
                    mode,
                    &operation_context("host_permissions_apply"),
                )
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "host_firewall_apply",
        description = "Apply or remove a validated UFW allow/deny rule for an IP/CIDR, port and protocol. Mutating: inspect first; requires approved=true and node policy enablement."
    )]
    async fn host_firewall_apply(
        &self,
        Parameters(request): Parameters<HostFirewallMutation>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        let decision = match request.decision.as_str() {
            "allow" => FirewallDecision::Allow,
            "deny" => FirewallDecision::Deny,
            _ => return Err("decision must be allow or deny".into()),
        };
        let protocol = match request.protocol.as_str() {
            "tcp" => NetworkProtocol::Tcp,
            "udp" => NetworkProtocol::Udp,
            _ => return Err("protocol must be tcp or udp".into()),
        };
        to_json(
            &host_operator()
                .apply_firewall_rule(
                    &FirewallRule {
                        decision,
                        port: request.port,
                        protocol,
                        source: request.source,
                    },
                    request.remove,
                    &operation_context("host_firewall_apply"),
                )
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "host_process_signal",
        description = "Send a fixed TERM, KILL or HUP signal to a validated PID. PID 0, 1 and Lumic itself are protected. Mutating: requires approved=true and node policy enablement."
    )]
    fn host_process_signal(
        &self,
        Parameters(request): Parameters<HostProcessMutation>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        let signal = match request.signal.as_str() {
            "terminate" => ProcessSignal::Terminate,
            "kill" => ProcessSignal::Kill,
            "hangup" => ProcessSignal::Hangup,
            _ => return Err("signal must be terminate, kill, or hangup".into()),
        };
        to_json(
            &host_operator()
                .signal_process(
                    request.pid,
                    signal,
                    &operation_context("host_process_signal"),
                )
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "host_updates_apply",
        description = "Apply security-only or all pending apt updates with fixed package-manager operations. Mutating: inspect snapshot first; requires approved=true and node policy enablement."
    )]
    async fn host_updates_apply(
        &self,
        Parameters(request): Parameters<HostUpdateMutation>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        let scope = match request.scope.as_str() {
            "security" => UpdateScope::Security,
            "all" => UpdateScope::All,
            _ => return Err("scope must be security or all".into()),
        };
        to_json(
            &host_operator()
                .apply_updates(scope, &operation_context("host_updates_apply"))
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "host_backup_schedule",
        description = "Create or reconcile a persistent systemd timer for a Lumic-managed service backup. Mutating: requires approved=true and node policy enablement."
    )]
    async fn host_backup_schedule(
        &self,
        Parameters(request): Parameters<HostBackupSchedule>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        to_json(
            &host_operator()
                .schedule_backup(
                    lumic_core::server::BackupSchedule {
                        id: request.id,
                        service_id: request.service,
                        database: request.database,
                        on_calendar: request.on_calendar,
                        enabled: request.enabled,
                    },
                    &operation_context("host_backup_schedule"),
                )
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "host_remediate",
        description = "Apply one deterministic remediation: restart_service, terminate_process, or vacuum_journal. Mutating: requires approved=true and node policy enablement."
    )]
    async fn host_remediate(
        &self,
        Parameters(request): Parameters<HostRemediation>,
    ) -> Result<String, String> {
        require_mutation(request.approved)?;
        let action = match request.action.as_str() {
            "restart_service" => RemediationAction::RestartService {
                unit: request.unit.ok_or("unit is required")?,
            },
            "terminate_process" => RemediationAction::TerminateProcess {
                pid: request.pid.ok_or("pid is required")?,
            },
            "vacuum_journal" => RemediationAction::VacuumJournal {
                older_than_days: request
                    .older_than_days
                    .ok_or("older_than_days is required")?,
            },
            _ => {
                return Err(
                    "action must be restart_service, terminate_process, or vacuum_journal".into(),
                );
            }
        };
        to_json(
            &host_operator()
                .remediate(action, &operation_context("host_remediate"))
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "operations_timeline",
        description = "Query newest correlated application, deployment, service, system, provider, remediation, and notification evidence. Read-only."
    )]
    fn operations_timeline(
        &self,
        Parameters(request): Parameters<OperationsTimeline>,
    ) -> Result<String, String> {
        to_json(
            &operations_service()
                .timeline(&TimelineQuery {
                    entity: request.entity,
                    entity_id: request.entity_id,
                    event_type: request.event_type,
                    since_unix_ms: request.since_unix_ms,
                    until_unix_ms: request.until_unix_ms,
                    limit: request.limit,
                })
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "operations_incident",
        description = "Reconstruct a factual incident report from correlated evidence in a bounded time/resource query. Read-only and does not invent a root cause."
    )]
    fn operations_incident(
        &self,
        Parameters(request): Parameters<OperationsTimeline>,
    ) -> Result<String, String> {
        to_json(
            &operations_service()
                .incident(&TimelineQuery {
                    entity: request.entity,
                    entity_id: request.entity_id,
                    event_type: request.event_type,
                    since_unix_ms: request.since_unix_ms,
                    until_unix_ms: request.until_unix_ms,
                    limit: request.limit,
                })
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "operations_provider_signal",
        description = "Record a typed provider signal and evaluate preconfigured deterministic rules. Mutating: requires approved=true and the operations.signal MCP scope."
    )]
    async fn operations_provider_signal(
        &self,
        Parameters(request): Parameters<ProviderSignalRequest>,
    ) -> Result<String, String> {
        require_scope("operations.signal", request.approved)?;
        let severity = match request.severity.as_str() {
            "info" => SignalSeverity::Info,
            "warning" => SignalSeverity::Warning,
            "error" => SignalSeverity::Error,
            "critical" => SignalSeverity::Critical,
            _ => return Err("severity must be info, warning, error, or critical".into()),
        };
        to_json(
            &operations_service()
                .record_provider_signal(
                    &request.event_type,
                    &request.entity,
                    &request.entity_id,
                    severity,
                    &request.summary,
                    request.payload,
                )
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "operations_webhook_plan",
        description = "Validate and preview a signed webhook destination, secret precondition, risk, and rollback. Read-only; call before operations_webhook_apply."
    )]
    fn operations_webhook_plan(
        &self,
        Parameters(request): Parameters<ConfigureWebhook>,
    ) -> Result<String, String> {
        to_json(
            &operations_service()
                .plan_destination(&webhook_destination(request))
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "operations_webhook_apply",
        description = "Apply a validated signed webhook destination using only a secret reference and recoverable configuration snapshot. Requires approved=true and operations.configure scope."
    )]
    fn operations_webhook_apply(
        &self,
        Parameters(request): Parameters<ConfigureWebhook>,
    ) -> Result<String, String> {
        require_scope("operations.configure", request.approved)?;
        let context = operation_context("operations-webhook-apply");
        to_json(
            &operations_service()
                .apply_destination(webhook_destination(request), &context)
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "operations_subscription_apply",
        description = "Subscribe a signed webhook destination to exact typed event filters. Requires approved=true and operations.configure scope."
    )]
    fn operations_subscription_apply(
        &self,
        Parameters(request): Parameters<ConfigureSubscription>,
    ) -> Result<String, String> {
        require_scope("operations.configure", request.approved)?;
        let context = operation_context("operations-subscription-apply");
        to_json(
            &operations_service()
                .apply_subscription(
                    EventSubscription {
                        id: request.id,
                        destination_id: request.destination_id,
                        event_types: request.event_types,
                        entity: request.entity,
                        entity_id: request.entity_id,
                        enabled: true,
                    },
                    &context,
                )
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "operations_rule_plan",
        description = "Preview the typed systemd restart action, cooldown, attempt bound, verification, impact and recovery for an event rule. Read-only."
    )]
    fn operations_rule_plan(
        &self,
        Parameters(request): Parameters<ConfigureAutomationRule>,
    ) -> Result<String, String> {
        to_json(
            &operations_service()
                .plan_rule(&automation_rule(request))
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "operations_rule_apply",
        description = "Enable a deterministic, cooldown- and attempt-bounded typed systemd restart rule with verification. Requires approved=true and operations.automate scope."
    )]
    fn operations_rule_apply(
        &self,
        Parameters(request): Parameters<ConfigureAutomationRule>,
    ) -> Result<String, String> {
        require_scope("operations.automate", request.approved)?;
        let context = operation_context("operations-rule-apply");
        to_json(
            &operations_service()
                .apply_rule(automation_rule(request), &context)
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "operations_run_once",
        description = "Capture new Lumic events and process due signed webhook deliveries once. Requires approved=true and operations.run scope."
    )]
    async fn operations_run_once(
        &self,
        Parameters(request): Parameters<ApprovedOperation>,
    ) -> Result<String, String> {
        require_scope("operations.run", request.approved)?;
        to_json(
            &operations_service()
                .run_once()
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "operations_observe",
        description = "Immediately observe host, process, service, application and system evidence, bypassing the normal sampling interval. This can activate configured deterministic rules, so it requires approved=true and operations.run scope."
    )]
    async fn operations_observe(
        &self,
        Parameters(request): Parameters<ApprovedOperation>,
    ) -> Result<String, String> {
        require_scope("operations.run", request.approved)?;
        to_json(
            &operations_service()
                .observe_now()
                .await
                .map_err(string_error)?,
        )
    }

    #[tool(
        name = "operations_deliveries",
        description = "Return bounded notification delivery and retry history. Read-only."
    )]
    fn operations_deliveries(&self) -> Result<String, String> {
        to_json(&operations_service().deliveries(100).map_err(string_error)?)
    }

    #[tool(
        name = "operations_configuration_rollback",
        description = "Restore the previous Lumic-managed operations configuration snapshot. Requires approved=true and operations.configure scope."
    )]
    fn operations_configuration_rollback(
        &self,
        Parameters(request): Parameters<ApprovedOperation>,
    ) -> Result<String, String> {
        require_scope("operations.configure", request.approved)?;
        let context = operation_context("operations-configuration-rollback");
        operations_service()
            .rollback_configuration(&context)
            .map_err(string_error)?;
        Ok("{\"restored\":true}".into())
    }

    #[tool(
        name = "events_list",
        description = "Return the newest 100 structured Lumic infrastructure events. Read-only; event payloads never contain repository credentials."
    )]
    fn events_list(&self) -> Result<String, String> {
        serde_json::to_string_pretty(&event_store().list(100).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())
    }

    #[tool(
        name = "audit_list",
        description = "Return the newest 100 structured mutation audit records with actor, interface, arguments and before/after state. Read-only; credential values are redacted."
    )]
    fn audit_list(&self) -> Result<String, String> {
        to_json(
            &AuditStore::at_state_dir(state_directory())
                .list(100)
                .map_err(string_error)?,
        )
    }
}

fn state_directory() -> std::path::PathBuf {
    std::env::var_os("LUMIC_STATE_DIR")
        .map(Into::into)
        .unwrap_or_else(|| "/var/lib/lumic".into())
}

fn event_store() -> EventStore {
    EventStore::at_state_dir(state_directory())
}

fn application_service() -> ApplicationService {
    let state = state_directory();
    let apps = std::env::var_os("LUMIC_APPS_ROOT")
        .map(Into::into)
        .unwrap_or_else(|| state.join("apps"));
    ApplicationService::new(state, apps)
}

fn attention_service() -> AttentionService {
    let state = state_directory();
    let apps = std::env::var_os("LUMIC_APPS_ROOT")
        .map(Into::into)
        .unwrap_or_else(|| state.join("apps"));
    AttentionService::new(state, apps)
}

fn intelligence_service() -> ApplicationIntelligence {
    let state = state_directory();
    let apps = std::env::var_os("LUMIC_APPS_ROOT")
        .map(Into::into)
        .unwrap_or_else(|| state.join("apps"));
    ApplicationIntelligence::new(state, apps)
}

fn infrastructure_service() -> InfrastructureService {
    let state = state_directory();
    let apps = std::env::var_os("LUMIC_APPS_ROOT")
        .map(Into::into)
        .unwrap_or_else(|| state.join("apps"));
    InfrastructureService::new(state, apps)
}

fn parse_member_status(value: &str) -> Result<DeploymentMemberStatus, String> {
    match value {
        "pending" => Ok(DeploymentMemberStatus::Pending),
        "running" => Ok(DeploymentMemberStatus::Running),
        "succeeded" => Ok(DeploymentMemberStatus::Succeeded),
        "failed" => Ok(DeploymentMemberStatus::Failed),
        "rolled_back" => Ok(DeploymentMemberStatus::RolledBack),
        _ => Err("status must be pending, running, succeeded, failed, or rolled_back".into()),
    }
}

fn recipe_manager() -> RecipeManager {
    let state = state_directory();
    let apps = std::env::var_os("LUMIC_APPS_ROOT")
        .map(Into::into)
        .unwrap_or_else(|| state.join("apps"));
    RecipeManager::at_state_dir(state, apps)
}

fn host_operator() -> HostOperator {
    HostOperator::at_state_dir(state_directory())
}

fn recipe_request(request: RecipeRequest) -> RecipeInstallRequest {
    RecipeInstallRequest {
        recipe_id: request.recipe,
        application_id: request.app,
        domain: request.domain,
        repository_url: request.repository_url,
        branch: request.branch,
        tls_email: request.tls_email,
        environment: request.environment,
    }
}

fn managed_service_manager() -> ManagedServiceManager {
    ManagedServiceManager::at_state_dir(state_directory())
}

fn resource_framework() -> ResourceFramework {
    ResourceFramework::at_state_dir(state_directory())
}

fn software_manager() -> SoftwareManager {
    SoftwareManager::at_state_dir(state_directory())
}

fn operations_service() -> OperationsService {
    OperationsService::at_state_dir(state_directory())
}

fn webhook_destination(request: ConfigureWebhook) -> WebhookDestination {
    WebhookDestination {
        id: request.id,
        url: request.url,
        secret_reference: request.secret_reference,
        timeout_ms: request.timeout_ms,
        max_attempts: request.max_attempts,
        enabled: true,
    }
}

fn automation_rule(request: ConfigureAutomationRule) -> AutomationRule {
    AutomationRule {
        id: request.id,
        event_type: request.event_type,
        entity_id: request.entity_id,
        action: AutomationAction::RestartService { unit: request.unit },
        cooldown_seconds: request.cooldown_seconds,
        max_attempts: request.max_attempts,
        enabled: true,
        last_applied_unix_ms: None,
        attempt_count: 0,
    }
}

fn to_json(value: &(impl serde::Serialize + ?Sized)) -> Result<String, String> {
    serde_json::to_string_pretty(value).map_err(|error| error.to_string())
}

fn string_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn require_mutation(approved: bool) -> Result<(), String> {
    require_scope("mutations", approved)
}

fn require_scope(scope: &str, approved: bool) -> Result<(), String> {
    if std::env::var("LUMIC_MCP_ALLOW_MUTATIONS").as_deref() != Ok("1") {
        return Err(
            "MCP mutations are disabled by node policy; set LUMIC_MCP_ALLOW_MUTATIONS=1 when starting the MCP server"
                .into(),
        );
    }
    if !approved {
        return Err("this mutation requires approved=true after reviewing its plan/status".into());
    }
    let scopes = std::env::var("LUMIC_MCP_SCOPES").unwrap_or_default();
    let allowed = scopes.split(',').map(str::trim).any(|value| {
        value == "*"
            || value == scope
            || value
                .strip_suffix(".*")
                .is_some_and(|prefix| scope.starts_with(&format!("{prefix}.")))
    });
    if !allowed {
        return Err(format!(
            "MCP scope '{scope}' is not granted by LUMIC_MCP_SCOPES"
        ));
    }
    Ok(())
}

fn parse_runtime(runtime: &str) -> Result<ApplicationRuntime, String> {
    match runtime {
        "static" => Ok(ApplicationRuntime::Static),
        "php" => Ok(ApplicationRuntime::Php),
        "node" => Ok(ApplicationRuntime::Node),
        _ => Err("runtime must be one of: static, php, node".into()),
    }
}

fn parse_service_action(action: &str) -> Result<ServiceAction, String> {
    match action {
        "start" => Ok(ServiceAction::Start),
        "stop" => Ok(ServiceAction::Stop),
        "restart" => Ok(ServiceAction::Restart),
        "reload" => Ok(ServiceAction::Reload),
        "enable" => Ok(ServiceAction::Enable),
        "disable" => Ok(ServiceAction::Disable),
        _ => Err("action must be one of: start, stop, restart, reload, enable, disable".into()),
    }
}

fn parse_managed_kind(kind: &str) -> Result<ManagedServiceKind, String> {
    match kind {
        "mysql" => Ok(ManagedServiceKind::Mysql),
        "postgresql" => Ok(ManagedServiceKind::Postgresql),
        "redis" => Ok(ManagedServiceKind::Redis),
        "typesense" => Ok(ManagedServiceKind::Typesense),
        "meilisearch" => Ok(ManagedServiceKind::Meilisearch),
        _ => Err("kind must be one of: mysql, postgresql, redis, typesense, meilisearch".into()),
    }
}

fn resource_ref(kind: &str, id: &str) -> Result<ResourceRef, String> {
    let kind = match kind {
        "package" => ResourceKind::Package,
        "component" => ResourceKind::Component,
        "runtime" => ResourceKind::Runtime,
        "managed_service" => ResourceKind::ManagedService,
        "service_resource" => ResourceKind::ServiceResource,
        "endpoint" => ResourceKind::Endpoint,
        "application" => ResourceKind::Application,
        "process" => ResourceKind::Process,
        "schedule" => ResourceKind::Schedule,
        "artifact" => ResourceKind::Artifact,
        "certificate" => ResourceKind::Certificate,
        "pipeline" => ResourceKind::Pipeline,
        _ => return Err("unknown resource kind".into()),
    };
    ResourceRef::new(kind, id).map_err(string_error)
}

fn optional_resource_ref(
    kind: Option<&str>,
    id: Option<&str>,
) -> Result<Option<ResourceRef>, String> {
    match (kind, id) {
        (None, None) => Ok(None),
        (Some(kind), Some(id)) => resource_ref(kind, id).map(Some),
        _ => Err("kind and id must be supplied together".into()),
    }
}

fn current_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn operation_context(operation: &str) -> OperationContext {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    OperationContext {
        actor: std::env::var("LUMIC_MCP_ACTOR").unwrap_or_else(|_| "local-mcp-client".into()),
        interface: OperationInterface::Mcp,
        correlation_id: format!("mcp-{operation}-{timestamp}"),
        dry_run: false,
        approved: true,
    }
}

fn planning_context(operation: &str) -> OperationContext {
    let mut context = operation_context(operation);
    context.dry_run = true;
    context.approved = false;
    context
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for LumicMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_instructions(
            "Use STATUS/diagnosis and application_plan_deployment before APPLY tools. Mutations require node policy LUMIC_MCP_ALLOW_MUTATIONS=1, a matching LUMIC_MCP_SCOPES grant, and approved=true. Lumic exposes typed capabilities only and never unrestricted shell execution.",
        )
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult::with_all_items(vec![
            Resource::new(HOST_STATUS_URI, "server-status")
                .with_title("Lumic server status")
                .with_description("Live host identity and resource facts; read-only")
                .with_mime_type("application/json"),
            Resource::new(SERVER_ATTENTION_URI, "server-attention")
                .with_title("Lumic server attention summary")
                .with_description(
                    "Factual health, changes, incidents, upcoming attention and recommendations; read-only",
                )
                .with_mime_type("application/json"),
        ]))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        let value = match request.uri.as_str() {
            HOST_STATUS_URI => serde_json::to_value(
                host_status().map_err(|error| McpError::internal_error(error.to_string(), None))?,
            ),
            SERVER_ATTENTION_URI => serde_json::to_value(
                attention_service()
                    .report(24)
                    .await
                    .map_err(|error| McpError::internal_error(error.to_string(), None))?,
            ),
            _ => {
                return Err(McpError::resource_not_found(
                    format!("unknown Lumic resource: {}", request.uri),
                    None,
                ));
            }
        }
        .map_err(|error| McpError::internal_error(error.to_string(), None))?;
        let json = serde_json::to_string_pretty(&value)
            .map_err(|error| McpError::internal_error(error.to_string(), None))?;
        Ok(ReadResourceResult::new(vec![
            ResourceContents::text(json, request.uri).with_mime_type("application/json"),
        ])
        .into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::ServiceExt;
    #[cfg(target_os = "linux")]
    use rmcp::model::{CallToolRequestParams, ReadResourceRequestParams};

    #[test]
    fn publishes_status_plan_apply_and_recovery_tools() {
        let tools = LumicMcpServer::new().tool_router.list_all();
        assert!(tools.len() >= 34);
        assert!(tools.iter().any(|tool| tool.name == "inspect_server"));
        assert!(tools.iter().any(|tool| tool.name == "application_list"));
        assert!(tools.iter().any(|tool| tool.name == "events_list"));
        assert!(
            tools
                .iter()
                .any(|tool| tool.name == "application_plan_deployment")
        );
        assert!(tools.iter().any(|tool| tool.name == "application_deploy"));
        assert!(tools.iter().any(|tool| tool.name == "application_rollback"));
        assert!(tools.iter().any(|tool| tool.name == "diagnose_server"));
        assert!(tools.iter().any(|tool| tool.name == "package_install"));
        assert!(
            tools
                .iter()
                .any(|tool| tool.name == "application_configure_process")
        );
        for name in [
            "resource_catalog",
            "resource_schema",
            "resource_plan",
            "resource_apply",
            "resource_inspect",
            "resource_bindings",
            "resource_binding_apply",
            "resource_binding_remove",
            "resource_operations",
            "resource_operation_inspect",
            "software_catalog",
            "software_status",
            "software_plan_setup",
            "software_setup",
            "managed_service_list",
            "managed_service_detect",
            "managed_service_inspect",
            "managed_service_plan_install",
            "managed_service_install",
            "managed_service_declare_dependency",
            "managed_service_database_create",
            "managed_service_backup",
            "managed_service_backup_verify",
            "managed_service_restore",
            "application_attach_managed_service",
            "recipe_catalog",
            "recipe_plan",
            "recipe_install",
            "recipe_update",
            "recipe_uninstall",
            "host_operator_snapshot",
            "host_search_logs",
            "host_account_apply",
            "host_permissions_apply",
            "host_firewall_apply",
            "host_process_signal",
            "host_updates_apply",
            "host_backup_schedule",
            "host_remediate",
            "infrastructure_status",
            "node_initialize",
            "node_enrollment",
            "node_register",
            "node_revoke",
            "node_health",
            "git_repository_host",
            "git_mirror_sync",
            "git_push_deploy_configure",
            "environment_secret_generate",
            "application_environment_reference_set",
            "environment_export",
            "environment_import",
            "environment_diff",
            "resource_endpoint_register",
            "node_membership_configure",
            "coordinated_deployment_begin",
            "coordinated_deployment_report",
            "remote_operation_sign",
            "remote_operation_apply",
            "operations_timeline",
            "operations_incident",
            "operations_provider_signal",
            "operations_webhook_plan",
            "operations_webhook_apply",
            "operations_subscription_apply",
            "operations_rule_plan",
            "operations_rule_apply",
            "operations_run_once",
            "operations_observe",
            "operations_deliveries",
            "operations_configuration_rollback",
        ] {
            assert!(tools.iter().any(|tool| tool.name == name), "missing {name}");
        }
    }

    #[test]
    fn managed_service_kind_parser_accepts_built_in_drivers() {
        for (value, expected) in [
            ("mysql", ManagedServiceKind::Mysql),
            ("postgresql", ManagedServiceKind::Postgresql),
            ("redis", ManagedServiceKind::Redis),
            ("typesense", ManagedServiceKind::Typesense),
            ("meilisearch", ManagedServiceKind::Meilisearch),
        ] {
            assert_eq!(parse_managed_kind(value), Ok(expected));
        }
        assert!(parse_managed_kind("mariadb").is_err());
    }

    #[test]
    fn mutations_require_node_policy_and_explicit_approval() {
        unsafe { std::env::remove_var("LUMIC_MCP_ALLOW_MUTATIONS") };
        assert!(require_mutation(true).is_err());
        unsafe { std::env::set_var("LUMIC_MCP_ALLOW_MUTATIONS", "1") };
        assert!(require_mutation(false).is_err());
        assert!(require_mutation(true).is_err());
        unsafe { std::env::set_var("LUMIC_MCP_SCOPES", "mutations,operations.*") };
        assert!(require_mutation(true).is_ok());
        assert!(require_scope("operations.automate", true).is_ok());
        assert!(require_scope("secrets.read", true).is_err());
        unsafe { std::env::remove_var("LUMIC_MCP_ALLOW_MUTATIONS") };
        unsafe { std::env::remove_var("LUMIC_MCP_SCOPES") };
    }

    #[tokio::test]
    async fn serves_resource_catalog_over_mcp() -> anyhow::Result<()> {
        let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
        let server = tokio::spawn(async move {
            LumicMcpServer::new()
                .serve(server_transport)
                .await?
                .waiting()
                .await?;
            anyhow::Ok(())
        });
        let client = ().serve(client_transport).await?;

        let resources = client.list_resources(None).await?;
        assert_eq!(resources.resources.len(), 2);
        assert_eq!(resources.resources[0].uri, HOST_STATUS_URI);
        assert_eq!(resources.resources[1].uri, SERVER_ATTENTION_URI);

        client.cancel().await?;
        server.await??;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn serves_status_tool_and_resource_over_mcp() -> anyhow::Result<()> {
        let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
        let server = tokio::spawn(async move {
            LumicMcpServer::new()
                .serve(server_transport)
                .await?
                .waiting()
                .await?;
            anyhow::Ok(())
        });
        let client = ().serve(client_transport).await?;

        let resources = client.list_resources(None).await?;
        assert_eq!(resources.resources[0].uri, HOST_STATUS_URI);
        let resource = client
            .read_resource(ReadResourceRequestParams::new(HOST_STATUS_URI))
            .await?;
        assert_eq!(resource.contents.len(), 1);

        let result = client
            .call_tool(CallToolRequestParams::new("inspect_server"))
            .await?;
        let text = result.content[0].as_text().expect("text tool result");
        let json: serde_json::Value = serde_json::from_str(&text.text)?;
        assert_eq!(json["operating_system"], "linux");

        let attention = client
            .call_tool(
                CallToolRequestParams::new("server_attention").with_arguments(
                    serde_json::Map::from_iter([("period_hours".into(), serde_json::json!(24))]),
                ),
            )
            .await?;
        let text = attention.content[0].as_text().expect("text tool result");
        let json: serde_json::Value = serde_json::from_str(&text.text)?;
        assert!(json["summary"]["severity"].is_string());
        assert!(json["summary"]["facts"].is_array());

        client.cancel().await?;
        server.await??;
        Ok(())
    }
}
