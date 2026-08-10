use crate::LumicMcpServer;
use axum::{
    Router,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use lumic_core::{LumicError, Result};
use lumic_platform::atomic_file::write_atomic;
use rmcp::{
    ServiceExt,
    transport::{
        stdio,
        streamable_http_server::{
            StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
        },
    },
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Debug, Clone)]
pub struct McpHttpCredentialStore {
    path: PathBuf,
}

impl McpHttpCredentialStore {
    pub fn at_state_dir(state_dir: impl AsRef<Path>) -> Self {
        Self {
            path: state_dir.as_ref().join("mcp-http-token.sha256"),
        }
    }

    pub fn rotate(&self) -> Result<String> {
        let mut bytes = [0_u8; 32];
        fs::File::open("/dev/urandom")
            .and_then(|mut file| file.read_exact(&mut bytes))
            .map_err(credential_io)?;
        let token = bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        write_atomic(&self.path, digest(token.as_bytes()).as_bytes(), 0o600)?;
        Ok(token)
    }

    pub fn configured(&self) -> bool {
        self.path.is_file() && !self.path.is_symlink()
    }

    fn verify(&self, token: &str) -> Result<bool> {
        if !self.configured() {
            return Ok(false);
        }
        let expected = fs::read_to_string(&self.path).map_err(credential_io)?;
        Ok(constant_time_eq(
            expected.trim().as_bytes(),
            digest(token.as_bytes()).as_bytes(),
        ))
    }
}

pub async fn serve_stdio() -> anyhow::Result<()> {
    let service = LumicMcpServer::new().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

pub fn streamable_http_router(state_dir: impl AsRef<Path>, allowed_hosts: Vec<String>) -> Router {
    let mut config = StreamableHttpServerConfig::default();
    config.allowed_hosts = allowed_hosts;
    config.json_response = true;
    let service = StreamableHttpService::new(
        || Ok(LumicMcpServer::new()),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    let credentials = Arc::new(McpHttpCredentialStore::at_state_dir(state_dir));
    Router::new()
        .route_service("/mcp", service)
        .layer(middleware::from_fn_with_state(credentials, authorize))
}

async fn authorize(
    State(credentials): State<Arc<McpHttpCredentialStore>>,
    request: Request,
    next: Next,
) -> Response {
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if token.is_some_and(|token| credentials.verify(token).unwrap_or(false)) {
        return next.run(request).await;
    }
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Bearer")],
        "valid Lumic MCP bearer token required",
    )
        .into_response()
}

fn digest(value: &[u8]) -> String {
    let digest = Sha256::digest(value);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn credential_io(error: std::io::Error) -> LumicError {
    LumicError::Internal {
        message: format!("MCP HTTP credential I/O failed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt as _;

    #[test]
    fn token_is_stored_as_a_private_digest() {
        let directory =
            std::env::temp_dir().join(format!("lumic-mcp-credential-test-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let store = McpHttpCredentialStore::at_state_dir(&directory);
        let token = store.rotate().unwrap();
        let persisted = fs::read_to_string(directory.join("mcp-http-token.sha256")).unwrap();
        assert!(!persisted.contains(&token));
        assert!(store.verify(&token).unwrap());
        assert!(!store.verify("wrong").unwrap());
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn http_transport_rejects_missing_token_and_accepts_valid_token() {
        let directory =
            std::env::temp_dir().join(format!("lumic-mcp-http-test-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let token = McpHttpCredentialStore::at_state_dir(&directory)
            .rotate()
            .unwrap();
        let router = streamable_http_router(&directory, vec!["localhost".into()]);
        let unauthorized = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/mcp")
                    .header(header::HOST, "localhost")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let initialize = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#;
        let authorized = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/mcp")
                    .header(header::HOST, "localhost")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::ACCEPT, "application/json, text/event-stream")
                    .body(Body::from(initialize))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(authorized.status(), StatusCode::OK);
        fs::remove_dir_all(directory).unwrap();
    }
}
