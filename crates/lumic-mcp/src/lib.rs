//! Read-only Model Context Protocol adapter for Lumic host status.

use lumic_core::HostFacts;
use lumic_platform::{application::ApplicationService, event_store::EventStore};
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::router::tool::ToolRouter,
    model::{
        ListResourcesResult, PaginatedRequestParams, ReadResourceRequestParams,
        ReadResourceResponse, ReadResourceResult, Resource, ResourceContents, ServerCapabilities,
        ServerInfo,
    },
    service::RequestContext,
    tool, tool_handler, tool_router,
};

pub const HOST_STATUS_URI: &str = "lumic://server/status";

pub fn host_status() -> lumic_core::Result<HostFacts> {
    lumic_platform::inspect_host()
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
        name = "events_list",
        description = "Return the newest 100 structured Lumic infrastructure events. Read-only; event payloads never contain repository credentials."
    )]
    fn events_list(&self) -> Result<String, String> {
        serde_json::to_string_pretty(&event_store().list(100).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())
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
            "Lumic exposes read-only live host, application and event state. It does not expose shell execution. Mutating MCP capabilities remain disabled until policy and approval wiring is complete.",
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
    fn publishes_only_read_only_tools() {
        let tools = LumicMcpServer::new().tool_router.list_all();
        assert_eq!(tools.len(), 3);
        assert!(tools.iter().any(|tool| tool.name == "inspect_server"));
        assert!(tools.iter().any(|tool| tool.name == "application_list"));
        assert!(tools.iter().any(|tool| tool.name == "events_list"));
        assert!(tools.iter().all(|tool| {
            tool.description
                .as_deref()
                .unwrap()
                .to_lowercase()
                .contains("read-only")
        }));
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
