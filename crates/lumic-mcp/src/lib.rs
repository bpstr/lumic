//! Model Context Protocol adapter over Lumic's typed host capabilities.

use lumic_core::{
    HostFacts, OperationContext, OperationInterface,
    application::{ApplicationProcess, ApplicationProcessKind, ApplicationRuntime, Deployment},
    package::PackageName,
};
use lumic_platform::{
    application::ApplicationService,
    apt::AptPackageManager,
    audit_store::AuditStore,
    diagnostics::diagnose_host,
    event_store::EventStore,
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

pub const HOST_STATUS_URI: &str = "lumic://server/status";

pub fn host_status() -> lumic_core::Result<HostFacts> {
    lumic_platform::inspect_host()
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
struct ApplicationId {
    /// Stable Lumic application identifier.
    app: String,
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
        description = "Create application metadata and managed release directories. Mutating: requires LUMIC_MCP_ALLOW_MUTATIONS=1 and approved=true. Does not install runtime packages or deploy code."
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
                .provision(
                    &request.app,
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
        description = "Install Certbot, issue a Let's Encrypt certificate through nginx, and enable HTTPS redirect. Mutating and externally observable: requires node policy enablement and approved=true."
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
            schedule: request.schedule,
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

fn to_json(value: &impl serde::Serialize) -> Result<String, String> {
    serde_json::to_string_pretty(value).map_err(|error| error.to_string())
}

fn string_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn require_mutation(approved: bool) -> Result<(), String> {
    if std::env::var("LUMIC_MCP_ALLOW_MUTATIONS").as_deref() != Ok("1") {
        return Err(
            "MCP mutations are disabled by node policy; set LUMIC_MCP_ALLOW_MUTATIONS=1 when starting the local MCP server"
                .into(),
        );
    }
    if !approved {
        return Err("this mutation requires approved=true after reviewing its plan/status".into());
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
            "Use STATUS/diagnosis and application_plan_deployment before APPLY tools. Mutations require both node policy LUMIC_MCP_ALLOW_MUTATIONS=1 and approved=true. Lumic exposes typed capabilities only and never unrestricted shell execution.",
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
        ]))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        if request.uri != HOST_STATUS_URI {
            return Err(McpError::resource_not_found(
                format!("unknown Lumic resource: {}", request.uri),
                None,
            ));
        }
        let facts =
            host_status().map_err(|error| McpError::internal_error(error.to_string(), None))?;
        let json = serde_json::to_string_pretty(&facts)
            .map_err(|error| McpError::internal_error(error.to_string(), None))?;
        Ok(ReadResourceResult::new(vec![
            ResourceContents::text(json, HOST_STATUS_URI).with_mime_type("application/json"),
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
        assert_eq!(tools.len(), 20);
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
    }

    #[test]
    fn mutations_require_node_policy_and_explicit_approval() {
        unsafe { std::env::remove_var("LUMIC_MCP_ALLOW_MUTATIONS") };
        assert!(require_mutation(true).is_err());
        unsafe { std::env::set_var("LUMIC_MCP_ALLOW_MUTATIONS", "1") };
        assert!(require_mutation(false).is_err());
        assert!(require_mutation(true).is_ok());
        unsafe { std::env::remove_var("LUMIC_MCP_ALLOW_MUTATIONS") };
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
        assert_eq!(resources.resources.len(), 1);
        assert_eq!(resources.resources[0].uri, HOST_STATUS_URI);

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

        client.cancel().await?;
        server.await??;
        Ok(())
    }
}
