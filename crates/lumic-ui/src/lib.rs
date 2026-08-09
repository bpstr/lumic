use axum::{
    Form, Json, Router,
    extract::{Path as RoutePath, Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use lumic_core::{
    LumicError, OperationContext, OperationInterface, Result, application::Deployment,
    server::UpdateScope,
};
use lumic_platform::{
    application::ApplicationService, atomic_file::write_atomic, attention::AttentionService,
    event_store::EventStore, infrastructure::InfrastructureService,
    intelligence::ApplicationIntelligence, managed_service::ManagedServiceManager,
    recipe::RecipeManager, server::HostOperator, systemd::ServiceAction,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, VecDeque},
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const SESSION_SECONDS: u64 = 8 * 60 * 60;
const MAX_SESSIONS: usize = 1_024;
const LOGIN_WINDOW_SECONDS: u64 = 60;
const MAX_LOGIN_FAILURES: usize = 5;
const STYLE: &str = r#"
:root{color-scheme:light;--background:#f7f7f6;--surface:#fff;--foreground:#151515;--muted:#6b6b68;--border:#deded9;--accent:#ecece8;--sidenav:#f1f1ee;--sidenav-width:17rem;--radius:.7rem;font-family:Inter,ui-sans-serif,system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif}
*{box-sizing:border-box}body{margin:0;background:var(--background);color:var(--foreground);font-size:15px;line-height:1.55}a{color:inherit}.skip-link{position:fixed;z-index:100;left:1rem;top:-4rem;background:#111;color:#fff;padding:.65rem 1rem;border-radius:.4rem}.skip-link:focus{top:1rem}.shell{min-height:100vh}.sidenav{position:fixed;inset:0 auto 0 0;z-index:20;width:var(--sidenav-width);display:flex;flex-direction:column;background:var(--sidenav);border-right:1px solid var(--border)}.sidenav-header,.sidenav-footer{padding:1rem}.brand{display:flex;align-items:center;gap:.75rem;text-decoration:none;font-weight:700;letter-spacing:-.02em}.brand-mark{display:grid;place-items:center;width:2rem;height:2rem;border-radius:.55rem;background:#151515;color:#fff;font-size:.82rem}.brand-copy{display:flex;flex-direction:column;line-height:1.2}.brand-copy small{color:var(--muted);font-size:.7rem;font-weight:500;letter-spacing:.04em;text-transform:uppercase}.node-chip{margin-top:1rem;padding:.55rem .65rem;border:1px solid var(--border);border-radius:.5rem;background:rgba(255,255,255,.55);font-size:.78rem;color:var(--muted)}.sidenav-content{flex:1;overflow:auto;padding:.25rem .75rem}.sidenav-group{padding:.55rem 0}.sidenav-label{padding:.25rem .65rem;color:#777772;font-size:.68rem;font-weight:700;letter-spacing:.09em;text-transform:uppercase}.sidenav-menu{display:grid;gap:.15rem;margin-top:.25rem}.sidenav-link{display:flex;align-items:center;gap:.7rem;min-height:2.45rem;padding:.5rem .65rem;border-radius:.48rem;text-decoration:none;color:#4c4c49;font-weight:520}.sidenav-link:hover{background:rgba(255,255,255,.7);color:#111}.sidenav-link[aria-current=page]{background:#fff;color:#111;box-shadow:0 1px 2px rgba(0,0,0,.05)}.nav-icon{display:grid;place-items:center;width:1.25rem;color:#777;font-family:ui-monospace,monospace;font-size:.72rem}.sidenav-link[aria-current=page] .nav-icon{color:#111}.sidenav-footer{border-top:1px solid var(--border)}.operator{display:flex;align-items:center;gap:.65rem;margin-bottom:.75rem}.avatar{display:grid;place-items:center;width:2rem;height:2rem;border-radius:50%;background:#d9d9d4;font-size:.72rem;font-weight:700}.operator-copy{display:flex;flex-direction:column;line-height:1.25}.operator-copy small{color:var(--muted)}.signout{width:100%;background:transparent;color:#444;border:1px solid var(--border);border-radius:.48rem;padding:.55rem .7rem;text-align:left}.signout:hover{background:#fff;color:#111}.content{min-width:0;margin-left:var(--sidenav-width)}main{max-width:1260px;margin:0 auto;padding:2.75rem clamp(1.25rem,4vw,3.5rem) 5rem}.mobile-bar{display:none}h1{margin:.1rem 0 .35rem;font-size:clamp(1.75rem,3vw,2.3rem);line-height:1.15;letter-spacing:-.035em}h2{margin:2rem 0 .75rem;font-size:1.05rem;letter-spacing:-.015em}h3{font-size:.95rem}.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(235px,1fr));gap:1rem}.card,table,form.panel{background:var(--surface);border:1px solid var(--border);border-radius:var(--radius);box-shadow:0 1px 2px rgba(0,0,0,.025)}.card,form.panel{padding:1.25rem}.card h2,.card h3{margin-top:0}table{width:100%;border-collapse:separate;border-spacing:0;overflow:hidden}th,td{text-align:left;padding:.8rem .9rem;border-bottom:1px solid #ecece8;vertical-align:top}tr:last-child td{border-bottom:0}th{color:var(--muted);font-size:.69rem;letter-spacing:.065em;text-transform:uppercase}tbody tr:hover{background:#fafaf8}.muted{color:var(--muted)}.ok{color:#176b2c}.bad{color:#9b1c1c}.mono,pre{font-family:"SFMono-Regular",Consolas,"Liberation Mono",monospace}pre{white-space:pre-wrap;background:#171717;color:#eee;padding:1rem;border-radius:var(--radius);overflow:auto}button{background:#171717;color:#fff;border:0;border-radius:.48rem;padding:.62rem .9rem;font:inherit;font-weight:600;cursor:pointer}input{padding:.65rem .75rem;border:1px solid #aaa;border-radius:.45rem;width:min(420px,100%);font:inherit}.actions{display:flex;gap:.6rem;flex-wrap:wrap;margin:1rem 0}.actions a{background:#fff;border:1px solid #aaa;padding:.52rem .78rem;text-decoration:none;border-radius:.48rem;font-weight:600}.actions a:first-child{background:#171717;color:#fff;border-color:#171717}dt{font-weight:650;margin-top:.75rem}dd{margin:.15rem 0;color:#555}.flash{border:1px solid var(--border);border-left:3px solid #111;padding:.85rem 1rem;background:#fff;border-radius:.3rem}a:focus-visible,button:focus-visible,input:focus-visible,summary:focus-visible{outline:3px solid rgba(35,95,190,.35);outline-offset:2px}.login-shell{min-height:100vh;display:grid;place-items:center;padding:2rem}.login-panel{width:min(28rem,100%)}.login-brand{margin-bottom:1.5rem}.mobile-nav{display:none}
@media(max-width:800px){.sidenav{display:none}.content{margin-left:0}.mobile-bar{display:flex;position:sticky;top:0;z-index:30;align-items:center;justify-content:space-between;padding:.7rem 1rem;background:rgba(247,247,246,.94);border-bottom:1px solid var(--border);backdrop-filter:blur(12px)}.mobile-nav{display:block}.mobile-nav summary{list-style:none;cursor:pointer;border:1px solid var(--border);border-radius:.45rem;padding:.4rem .65rem;background:#fff;font-weight:650}.mobile-nav summary::-webkit-details-marker{display:none}.mobile-nav[open] .mobile-sheet{position:fixed;z-index:40;inset:0 auto 0 0;width:min(19rem,88vw);display:flex;flex-direction:column;background:var(--sidenav);border-right:1px solid var(--border);box-shadow:10px 0 35px rgba(0,0,0,.16)}.mobile-nav[open]::after{content:"";position:fixed;z-index:35;inset:0;background:rgba(0,0,0,.32)}.mobile-sheet .sidenav-content{padding:0 .75rem}.mobile-sheet .sidenav-footer{margin-top:auto}main{padding-top:1.75rem}.card,form.panel{padding:1rem}table{display:block;overflow-x:auto;white-space:nowrap}}
@media(prefers-reduced-motion:no-preference){.sidenav-link,.actions a,button{transition:background-color .15s ease,color .15s ease,border-color .15s ease}}
"#;

#[derive(Clone, Copy, PartialEq, Eq)]
enum NavSection {
    Overview,
    Applications,
    Services,
    Recipes,
    Infrastructure,
    Host,
    Events,
}

#[derive(Debug, Clone)]
pub struct UiCredentialStore {
    path: PathBuf,
}

impl UiCredentialStore {
    pub fn at_state_dir(state_dir: impl AsRef<Path>) -> Self {
        Self {
            path: state_dir.as_ref().join("ui-admin-token.sha256"),
        }
    }

    pub fn rotate(&self) -> Result<String> {
        let token = random_token()?;
        let digest = digest(token.as_bytes());
        write_atomic(&self.path, digest.as_bytes(), 0o600)?;
        Ok(token)
    }

    fn verified_revision(&self, token: &str) -> Result<Option<String>> {
        let Some(expected) = self.revision()? else {
            return Ok(None);
        };
        if constant_time_eq(expected.as_bytes(), digest(token.as_bytes()).as_bytes()) {
            Ok(Some(expected))
        } else {
            Ok(None)
        }
    }

    fn revision(&self) -> Result<Option<String>> {
        if !self.path.is_file() || self.path.is_symlink() {
            return Ok(None);
        }
        let expected = fs::read_to_string(&self.path).map_err(ui_io)?;
        Ok(Some(expected.trim().to_owned()))
    }

    pub fn configured(&self) -> bool {
        self.path.is_file() && !self.path.is_symlink()
    }
}

#[derive(Debug, Clone)]
struct SessionRecord {
    csrf: String,
    expires_unix: u64,
    credential_revision: String,
}

#[derive(Debug, Default)]
struct LoginThrottle {
    failures: VecDeque<u64>,
}

impl LoginThrottle {
    fn prune(&mut self, now: u64) {
        while self
            .failures
            .front()
            .is_some_and(|failure| now.saturating_sub(*failure) >= LOGIN_WINDOW_SECONDS)
        {
            self.failures.pop_front();
        }
    }

    fn is_limited(&mut self, now: u64) -> bool {
        self.prune(now);
        self.failures.len() >= MAX_LOGIN_FAILURES
    }

    fn record_failure(&mut self, now: u64) {
        self.prune(now);
        self.failures.push_back(now);
    }
}

#[derive(Debug, Clone)]
pub struct UiState {
    state_dir: PathBuf,
    apps_root: PathBuf,
    sessions: Arc<Mutex<HashMap<String, SessionRecord>>>,
    login_throttle: Arc<Mutex<LoginThrottle>>,
}

impl UiState {
    pub fn new(state_dir: impl AsRef<Path>, apps_root: impl Into<PathBuf>) -> Self {
        Self {
            state_dir: state_dir.as_ref().to_path_buf(),
            apps_root: apps_root.into(),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            login_throttle: Arc::new(Mutex::new(LoginThrottle::default())),
        }
    }

    fn applications(&self) -> ApplicationService {
        ApplicationService::new(&self.state_dir, self.apps_root.clone())
    }

    fn attention(&self) -> AttentionService {
        AttentionService::new(&self.state_dir, self.apps_root.clone())
    }

    fn services(&self) -> ManagedServiceManager {
        ManagedServiceManager::at_state_dir(&self.state_dir)
    }
    fn recipes(&self) -> RecipeManager {
        RecipeManager::at_state_dir(&self.state_dir, self.apps_root.clone())
    }
    fn host_operator(&self) -> HostOperator {
        HostOperator::at_state_dir(&self.state_dir)
    }

    fn infrastructure(&self) -> InfrastructureService {
        InfrastructureService::new(&self.state_dir, self.apps_root.clone())
    }

    fn intelligence(&self) -> ApplicationIntelligence {
        ApplicationIntelligence::new(&self.state_dir, self.apps_root.clone())
    }
}

pub fn router(state: UiState) -> Router {
    Router::new()
        .route("/login", get(login_page).post(login))
        .route("/logout", post(logout))
        .route("/", get(overview))
        .route("/apps", get(applications))
        .route("/apps/{id}", get(application_detail))
        .route("/services", get(services))
        .route("/services/{id}", get(service_detail))
        .route("/services/{id}/logs", get(service_logs))
        .route("/recipes", get(recipes))
        .route("/host", get(host_operator))
        .route("/infrastructure", get(infrastructure))
        .route("/api/infrastructure", get(infrastructure_api))
        .route("/deployments/{app}/{id}", get(deployment_detail))
        .route("/events", get(events))
        .route(
            "/actions/service/{id}/restart",
            get(confirm_service_restart).post(service_restart),
        )
        .route(
            "/actions/app/{id}/deploy",
            get(confirm_app_deploy).post(app_deploy),
        )
        .route(
            "/actions/app/{id}/rollback",
            get(confirm_app_rollback).post(app_rollback),
        )
        .route(
            "/actions/host/security-updates",
            get(confirm_security_updates).post(apply_security_updates),
        )
        .layer(middleware::from_fn(security_headers))
        .with_state(state)
}

async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static(
            "default-src 'self'; style-src 'self' 'unsafe-inline'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'",
        ),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        HeaderName::from_static("referrer-policy"),
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

pub async fn serve(state: UiState, bind: SocketAddr) -> std::io::Result<()> {
    if !bind.ip().is_loopback() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "Lumic UI must bind to loopback; use an authenticated TLS reverse proxy for remote access",
        ));
    }
    let listener = tokio::net::TcpListener::bind(bind).await?;
    axum::serve(listener, router(state)).await
}

async fn login_page(State(state): State<UiState>) -> Response {
    let setup = if UiCredentialStore::at_state_dir(&state.state_dir).configured() {
        String::new()
    } else {
        "<p class=flash>No admin token is configured. Run <span class=mono>lumic ui token rotate</span> on the node.</p>".into()
    };
    page("Sign in", &format!("<h1>Operator sign in</h1>{setup}<form class=panel method=post><p><label>Admin token<br><input name=token type=password autocomplete=current-password required></label></p><button>Sign in</button></form>"), false).into_response()
}

#[derive(Deserialize)]
struct LoginForm {
    token: String,
}

async fn login(State(state): State<UiState>, Form(form): Form<LoginForm>) -> Response {
    let now = unix_seconds();
    let mut throttle = match state.login_throttle.lock() {
        Ok(value) => value,
        Err(_) => return error_response(session_error()),
    };
    if throttle.is_limited(now) {
        return (
            [(header::RETRY_AFTER, LOGIN_WINDOW_SECONDS.to_string())],
            (
                StatusCode::TOO_MANY_REQUESTS,
                page(
                    "Sign in paused",
                    "<h1>Sign in paused</h1><p>Too many failed attempts. Wait one minute and try again.</p>",
                    false,
                ),
            ),
        )
            .into_response();
    }
    let credential_revision = match UiCredentialStore::at_state_dir(&state.state_dir)
        .verified_revision(&form.token)
    {
        Ok(Some(value)) => {
            throttle.failures.clear();
            value
        }
        Ok(None) => {
            throttle.record_failure(now);
            return (StatusCode::UNAUTHORIZED, page("Sign in failed", "<h1>Sign in failed</h1><p>The token was not accepted.</p><p><a href=/login>Try again</a></p>", false)).into_response();
        }
        Err(error) => return error_response(error),
    };
    drop(throttle);
    let session = match random_token() {
        Ok(value) => value,
        Err(error) => return error_response(error),
    };
    let csrf = match random_token() {
        Ok(value) => value,
        Err(error) => return error_response(error),
    };
    let mut sessions = match state.sessions.lock() {
        Ok(value) => value,
        Err(_) => return error_response(session_error()),
    };
    sessions.retain(|_, record| record.expires_unix >= now);
    if sessions.len() >= MAX_SESSIONS
        && let Some(oldest) = sessions
            .iter()
            .min_by_key(|(_, record)| record.expires_unix)
            .map(|(id, _)| id.clone())
    {
        sessions.remove(&oldest);
    }
    sessions.insert(
        session.clone(),
        SessionRecord {
            csrf,
            expires_unix: now + SESSION_SECONDS,
            credential_revision,
        },
    );
    drop(sessions);
    let cookie = format!(
        "lumic_session={session}; HttpOnly; SameSite=Strict; Path=/; Max-Age={SESSION_SECONDS}"
    );
    ([(header::SET_COOKIE, cookie)], Redirect::to("/")).into_response()
}

async fn logout(State(state): State<UiState>, headers: HeaderMap) -> Response {
    if let Some(id) = session_cookie(&headers)
        && let Ok(mut sessions) = state.sessions.lock()
    {
        sessions.remove(&id);
    }
    (
        [(
            header::SET_COOKIE,
            "lumic_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
        )],
        Redirect::to("/login"),
    )
        .into_response()
}

async fn overview(State(state): State<UiState>, headers: HeaderMap) -> Response {
    if auth(&state, &headers).is_none() {
        return Redirect::to("/login").into_response();
    }
    let attention = match state.attention().report(24).await {
        Ok(report) => report,
        Err(error) => return error_response(error),
    };
    let host = &attention.summary.facts;
    let fact = |key: &str| {
        host.iter()
            .find(|item| item.key == key)
            .map(|item| item.value.as_str())
            .unwrap_or("unknown")
    };
    let apps = state.applications().list().unwrap_or_default();
    let services = state.services().list().unwrap_or_default();
    let events = EventStore::at_state_dir(&state.state_dir)
        .list(8)
        .unwrap_or_default();
    let event_rows = events
        .iter()
        .map(|event| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}:{}</td></tr>",
                event.timestamp_unix_ms,
                escape(&event.event_type),
                escape(&event.entity),
                escape(&event.entity_id)
            )
        })
        .collect::<String>();
    let infrastructure = state.infrastructure().read_model().ok();
    let peer_count = infrastructure.as_ref().map_or(0, |model| model.nodes.len());
    page("Overview", &format!("<h1>{}</h1><p class=muted>{}</p><section class=card><h2>How this node is doing</h2><p class=muted>Personality: {}</p><pre>{}</pre></section><div class=grid><div class=card><h2>Applications</h2><p>{}</p><a href=/apps>Inspect applications</a></div><div class=card><h2>Services</h2><p>{}</p><a href=/services>Inspect services</a></div><div class=card><h2>Infrastructure</h2><p>{peer_count} trusted or revoked peers</p><a href=/infrastructure>Inspect topology</a></div><div class=card><h2>Resources</h2><p>{} · {}</p></div></div><h2>Recent events</h2><table><tr><th>Time</th><th>Event</th><th>Entity</th></tr>{event_rows}</table>", escape(fact("hostname")), escape(fact("operating_system")), attention.personality, escape(&attention.rendered), apps.len(), services.len(), escape(fact("load_1m")), escape(fact("memory"))), true).into_response()
}

async fn infrastructure(State(state): State<UiState>, headers: HeaderMap) -> Response {
    if auth(&state, &headers).is_none() {
        return Redirect::to("/login").into_response();
    }
    let model = match state.infrastructure().read_model() {
        Ok(value) => value,
        Err(error) => return error_response(error),
    };
    let nodes = model
        .nodes
        .iter()
        .map(|node| {
            let health = node
                .last_health
                .as_ref()
                .map(|health| {
                    if health.healthy {
                        "healthy"
                    } else {
                        "unhealthy"
                    }
                })
                .unwrap_or("unchecked");
            format!(
                "<tr><td>{}</td><td>{:?}</td><td>{}</td><td>{}</td></tr>",
                escape(&node.identity.name),
                node.trust_status,
                escape(&node.endpoint),
                health
            )
        })
        .collect::<String>();
    let environments = model
        .environments
        .iter()
        .map(|environment| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape(&environment.id),
                escape(environment.tier.as_str()),
                escape(&environment.application.id),
                escape(&environment.application.domain)
            )
        })
        .collect::<String>();
    page(
        "Infrastructure",
        &format!(
            "<h1>Infrastructure</h1><p class=muted>Node <span class=mono>{}</span> · roles {:?}</p><div class=grid><div class=card><h2>Git</h2><p>{} hosted · {} mirrors · {} push triggers</p></div><div class=card><h2>Topology</h2><p>{} endpoints · {} memberships</p></div><div class=card><h2>Deployments</h2><p>{} coordinated waves</p></div></div><h2>Nodes</h2><table><tr><th>Node</th><th>Trust</th><th>Endpoint</th><th>Health</th></tr>{nodes}</table><h2>Environments</h2><table><tr><th>Environment</th><th>Tier</th><th>Application</th><th>Domain</th></tr>{environments}</table>",
            escape(&model.local_node.id),
            model.local_node.roles,
            model.repositories.len(),
            model.mirrors.len(),
            model.triggers.len(),
            model.endpoints.len(),
            model.memberships.len(),
            model.deployments.len(),
        ),
        true,
    )
    .into_response()
}

async fn infrastructure_api(State(state): State<UiState>, headers: HeaderMap) -> Response {
    if auth(&state, &headers).is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match state.infrastructure().read_model() {
        Ok(model) => Json(model).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": error.to_string()})),
        )
            .into_response(),
    }
}

async fn applications(State(state): State<UiState>, headers: HeaderMap) -> Response {
    if auth(&state, &headers).is_none() {
        return Redirect::to("/login").into_response();
    }
    match state.applications().list() {
        Ok(apps) => {
            let rows = apps.iter().map(|app| format!("<tr><td><a href=/apps/{}>{}</a></td><td>{}</td><td>{:?}</td><td>{}</td></tr>", url_segment(&app.id), escape(&app.name), escape(&app.domain), app.runtime, escape(&app.health_status))).collect::<String>();
            page("Applications", &format!("<h1>Applications</h1><table><tr><th>Name</th><th>Domain</th><th>Runtime</th><th>Health</th></tr>{rows}</table>"), true).into_response()
        }
        Err(error) => error_response(error),
    }
}

async fn recipes(State(state): State<UiState>, headers: HeaderMap) -> Response {
    if auth(&state, &headers).is_none() {
        return Redirect::to("/login").into_response();
    }
    let installations = match state.recipes().list() {
        Ok(value) => value,
        Err(error) => return error_response(error),
    };
    let installed = installations
        .iter()
        .map(|item| {
            format!(
                "<tr><td>{}</td><td>{}@{}</td><td>{:?}</td></tr>",
                escape(&item.application_id),
                escape(&item.recipe_id),
                escape(&item.recipe_version),
                item.status
            )
        })
        .collect::<String>();
    let catalog = state
        .recipes()
        .catalog()
        .iter()
        .map(|item| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape(&item.metadata.id),
                escape(&item.metadata.version),
                escape(&item.metadata.description)
            )
        })
        .collect::<String>();
    page("Recipes", &format!("<h1>Application recipes</h1><p class=muted>Versioned, inspectable compositions over Lumic applications and managed services.</p><h2>Installed</h2><table><tr><th>Application</th><th>Recipe</th><th>Status</th></tr>{installed}</table><h2>Catalog</h2><table><tr><th>Recipe</th><th>Version</th><th>Description</th></tr>{catalog}</table>"), true).into_response()
}

async fn host_operator(State(state): State<UiState>, headers: HeaderMap) -> Response {
    if auth(&state, &headers).is_none() {
        return Redirect::to("/login").into_response();
    }
    let snapshot = match state.host_operator().snapshot().await {
        Ok(value) => value,
        Err(error) => return error_response(error),
    };
    let listeners = snapshot
        .listeners
        .iter()
        .map(|item| {
            format!(
                "<tr><td>{}</td><td>{}:{}</td><td>{}</td></tr>",
                escape(&item.protocol),
                escape(&item.local_address),
                item.port,
                escape(item.process.as_deref().unwrap_or("unknown"))
            )
        })
        .collect::<String>();
    let mounts = snapshot
        .mounts
        .iter()
        .map(|item| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}% free</td></tr>",
                escape(&item.mount_point),
                escape(&item.filesystem),
                item.available_bytes
                    .saturating_mul(100)
                    .checked_div(item.total_bytes)
                    .unwrap_or(0)
            )
        })
        .collect::<String>();
    let updates = snapshot
        .updates
        .iter()
        .map(|item| {
            format!(
                "<tr><td>{}</td><td>{} → {}</td><td>{}</td></tr>",
                escape(&item.package),
                escape(&item.current_version),
                escape(&item.candidate_version),
                if item.security { "security" } else { "regular" }
            )
        })
        .collect::<String>();
    let body = format!(
        "<h1>Host operator</h1><div class=grid><div class=card><h2>Accounts</h2><p>{} users · {} groups</p></div><div class=card><h2>Processes</h2><p>{} inspected</p></div><div class=card><h2>Timers</h2><p>{} active or known</p></div><div class=card><h2>Updates</h2><p>{} pending</p><a href=/actions/host/security-updates>Apply security updates</a></div></div><h2>Listening ports</h2><table><tr><th>Protocol</th><th>Address</th><th>Process</th></tr>{listeners}</table><h2>Mounts</h2><table><tr><th>Mount</th><th>Filesystem</th><th>Capacity</th></tr>{mounts}</table><h2>Pending updates</h2><table><tr><th>Package</th><th>Version</th><th>Class</th></tr>{updates}</table>",
        snapshot.users.len(),
        snapshot.groups.len(),
        snapshot.processes.len(),
        snapshot.timers.len(),
        snapshot.updates.len()
    );
    page("Host operator", &body, true).into_response()
}

async fn application_detail(
    State(state): State<UiState>,
    headers: HeaderMap,
    RoutePath(id): RoutePath<String>,
) -> Response {
    if auth(&state, &headers).is_none() {
        return Redirect::to("/login").into_response();
    }
    let app = match state.applications().inspect(&id) {
        Ok(value) => value,
        Err(error) => return error_response(error),
    };
    let deployments = state.applications().deployments(&id).unwrap_or_default();
    let deployment_rows = deployments
        .iter()
        .map(|deployment| {
            format!(
                "<tr><td><a href=/deployments/{}/{}>{}</a></td><td>{:?}</td><td>{}</td></tr>",
                url_segment(&id),
                url_segment(&deployment.id),
                escape(&deployment.id),
                deployment.status,
                escape(&deployment.commit)
            )
        })
        .collect::<String>();
    let references = app
        .service_references
        .iter()
        .map(|item| {
            format!(
                "<li>{} → {}{}</li>",
                escape(&item.role),
                escape(&item.service_id),
                item.database
                    .as_ref()
                    .map(|value| format!(" / {}", escape(value)))
                    .unwrap_or_default()
            )
        })
        .collect::<String>();
    let intelligence = state.intelligence();
    let fingerprint = intelligence.fingerprint(&id).ok();
    let graph = intelligence.dependency_graph(&id).ok();
    let intelligence_panel = fingerprint.map(|fingerprint| {
        let evidence = fingerprint.evidence.iter().map(|item| format!("<li><span class=mono>{}</span>: {}</li>", escape(&item.source), escape(&item.observation))).collect::<String>();
        let dependencies = graph.as_ref().map(|graph| graph.edges.iter().map(|edge| format!("<li><span class=mono>{}</span> → <span class=mono>{}</span> ({})</li>", escape(&edge.from), escape(&edge.to), escape(&edge.relationship))).collect::<String>()).unwrap_or_default();
        format!("<h2>Application intelligence</h2><div class=grid><div class=card><h3>Fingerprint</h3><p>{} · {:?} confidence · {} runtime</p><ul>{evidence}</ul></div><div class=card><h3>Dependencies</h3><ul>{dependencies}</ul><p class=muted>Use CLI or MCP to review and apply integration plans.</p></div></div>", escape(fingerprint.framework.as_deref().unwrap_or("unrecognized")), fingerprint.confidence, escape(&fingerprint.runtime))
    }).unwrap_or_else(|| "<h2>Application intelligence</h2><p class=muted>No deployed-source evidence is available yet.</p>".into());
    page(&app.name, &format!("<h1>{}</h1><p>{} · {:?} · <span class=mono>{}</span></p><div class=actions><a href=/actions/app/{}/deploy>Deploy</a><a href=/actions/app/{}/rollback>Rollback</a></div><div class=grid><dl class=card><dt>Health</dt><dd>{}</dd><dt>Root</dt><dd class=mono>{}</dd><dt>Repository</dt><dd>{}</dd></dl><div class=card><h2>Service references</h2><ul>{}</ul></div></div>{intelligence_panel}<h2>Deployments</h2><table><tr><th>ID</th><th>Status</th><th>Commit</th></tr>{deployment_rows}</table>", escape(&app.name), escape(&app.domain), app.runtime, escape(&app.id), url_segment(&id), url_segment(&id), escape(&app.health_status), escape(&app.root), app.repository.as_ref().map(|repo| format!("{} · {}", escape(&repo.url), escape(&repo.branch))).unwrap_or_else(|| "Not configured".into()), references), true).into_response()
}

async fn services(State(state): State<UiState>, headers: HeaderMap) -> Response {
    if auth(&state, &headers).is_none() {
        return Redirect::to("/login").into_response();
    }
    match state.services().list() {
        Ok(services) => {
            let rows = services.iter().map(|service| format!("<tr><td><a href=/services/{}>{}</a></td><td>{}</td><td>{}</td><td>{}</td></tr>", url_segment(&service.id), escape(&service.name), service.kind, escape(&service.systemd_unit), service.configuration.port)).collect::<String>();
            page("Services", &format!("<h1>Managed services</h1><table><tr><th>Name</th><th>Kind</th><th>Unit</th><th>Port</th></tr>{rows}</table>"), true).into_response()
        }
        Err(error) => error_response(error),
    }
}

async fn service_detail(
    State(state): State<UiState>,
    headers: HeaderMap,
    RoutePath(id): RoutePath<String>,
) -> Response {
    if auth(&state, &headers).is_none() {
        return Redirect::to("/login").into_response();
    }
    match state.services().inspect(&id).await {
        Ok(status) => {
            let manager = state.services();
            let databases = manager
                .databases(&id)
                .unwrap_or_default()
                .iter()
                .map(|item| format!("<li>{}</li>", escape(&item.name)))
                .collect::<String>();
            let users = manager
                .users(&id)
                .unwrap_or_default()
                .iter()
                .map(|item| format!("<li>{}</li>", escape(&item.name)))
                .collect::<String>();
            let backups = manager
                .backups(&id)
                .unwrap_or_default()
                .iter()
                .map(|item| {
                    format!(
                        "<li>{} · {:?} · {} bytes</li>",
                        escape(&item.id),
                        item.status,
                        item.size_bytes
                    )
                })
                .collect::<String>();
            let dependencies = status
                .service
                .dependencies
                .iter()
                .map(|item| {
                    format!(
                        "<li>{} · {}{}</li>",
                        escape(&item.service_id),
                        escape(&item.purpose),
                        if item.required { " · required" } else { "" }
                    )
                })
                .collect::<String>();
            let paths = status
                .paths
                .configuration_paths
                .iter()
                .map(|path| format!("<li class=mono>{}</li>", escape(path)))
                .collect::<String>();
            page(&status.service.name, &format!("<h1>{}</h1><p>{} · <span class={}>{:?}</span></p><div class=actions><a href=/actions/service/{}/restart>Restart</a><a href=/services/{}/logs>Logs</a></div><div class=grid><dl class=card><dt>Version</dt><dd>{}</dd><dt>Systemd</dt><dd>{} / {}</dd><dt>Enabled</dt><dd>{}</dd><dt>Address</dt><dd class=mono>{}:{}</dd></dl><div class=card><h2>Expert paths</h2><ul>{}</ul><p class=mono>{}</p><p class=mono>{}</p></div><div class=card><h2>Dependencies</h2><ul>{dependencies}</ul></div><div class=card><h2>Databases</h2><ul>{databases}</ul><h2>Users</h2><ul>{users}</ul></div><div class=card><h2>Local backups</h2><ul>{backups}</ul></div></div><p>{}</p>", escape(&status.service.name), status.service.kind, if status.health == lumic_core::managed_service::ServiceHealth::Healthy { "ok" } else { "bad" }, status.health, url_segment(&id), url_segment(&id), status.version.as_deref().unwrap_or("unknown"), escape(&status.active_state), escape(&status.sub_state), status.enabled, escape(&status.service.configuration.bind_address), status.service.configuration.port, paths, escape(&status.paths.data_path), escape(&status.paths.log_source), escape(&status.health_message)), true).into_response()
        }
        Err(error) => error_response(error),
    }
}

async fn service_logs(
    State(state): State<UiState>,
    headers: HeaderMap,
    RoutePath(id): RoutePath<String>,
) -> Response {
    if auth(&state, &headers).is_none() {
        return Redirect::to("/login").into_response();
    }
    match state.services().logs(&id, 200).await {
        Ok(logs) => page(
            "Service logs",
            &format!("<h1>{} logs</h1><pre>{}</pre>", escape(&id), escape(&logs)),
            true,
        )
        .into_response(),
        Err(error) => error_response(error),
    }
}

async fn deployment_detail(
    State(state): State<UiState>,
    headers: HeaderMap,
    RoutePath((app, id)): RoutePath<(String, String)>,
) -> Response {
    if auth(&state, &headers).is_none() {
        return Redirect::to("/login").into_response();
    }
    let deployment = state
        .applications()
        .deployments(&app)
        .ok()
        .and_then(|items| items.into_iter().find(|item| item.id == id));
    match deployment {
        Some(item) => page("Deployment", &deployment_html(&item), true).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            page("Not found", "<h1>Deployment not found</h1>", true),
        )
            .into_response(),
    }
}

async fn events(State(state): State<UiState>, headers: HeaderMap) -> Response {
    if auth(&state, &headers).is_none() {
        return Redirect::to("/login").into_response();
    }
    match EventStore::at_state_dir(&state.state_dir).list(250) {
        Ok(events) => {
            let rows = events
                .iter()
                .map(|event| {
                    format!(
                        "<tr><td>{}</td><td>{}</td><td>{}:{}</td><td>{}</td></tr>",
                        event.timestamp_unix_ms,
                        escape(&event.event_type),
                        escape(&event.entity),
                        escape(&event.entity_id),
                        escape(&event.actor)
                    )
                })
                .collect::<String>();
            page("Events", &format!("<h1>Events</h1><table><tr><th>Time</th><th>Event</th><th>Entity</th><th>Actor</th></tr>{rows}</table>"), true).into_response()
        }
        Err(error) => error_response(error),
    }
}

async fn confirm_service_restart(
    State(state): State<UiState>,
    headers: HeaderMap,
    RoutePath(id): RoutePath<String>,
) -> Response {
    confirm(
        &state,
        &headers,
        "Restart service",
        &format!("Restart {} and run its provider health check?", escape(&id)),
    )
}
async fn confirm_app_deploy(
    State(state): State<UiState>,
    headers: HeaderMap,
    RoutePath(id): RoutePath<String>,
) -> Response {
    confirm(
        &state,
        &headers,
        "Deploy application",
        &format!(
            "Deploy a new release of {} and health-gate activation?",
            escape(&id)
        ),
    )
}
async fn confirm_app_rollback(
    State(state): State<UiState>,
    headers: HeaderMap,
    RoutePath(id): RoutePath<String>,
) -> Response {
    confirm(
        &state,
        &headers,
        "Roll back application",
        &format!(
            "Activate the previous known-good release of {}?",
            escape(&id)
        ),
    )
}

async fn confirm_security_updates(State(state): State<UiState>, headers: HeaderMap) -> Response {
    confirm(
        &state,
        &headers,
        "Apply security updates",
        "Apply pending security updates with unattended-upgrade, then re-inspect pending packages?",
    )
}

#[derive(Deserialize)]
struct ConfirmForm {
    csrf: String,
}

async fn service_restart(
    State(state): State<UiState>,
    headers: HeaderMap,
    RoutePath(id): RoutePath<String>,
    Form(form): Form<ConfirmForm>,
) -> Response {
    if !authorized_post(&state, &headers, &form.csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match state
        .services()
        .lifecycle(&id, ServiceAction::Restart, &ui_context("service_restart"))
        .await
    {
        Ok(result) => result_page("Service restarted", &result.message),
        Err(error) => error_response(error),
    }
}

async fn app_deploy(
    State(state): State<UiState>,
    headers: HeaderMap,
    RoutePath(id): RoutePath<String>,
    Form(form): Form<ConfirmForm>,
) -> Response {
    if !authorized_post(&state, &headers, &form.csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match state
        .applications()
        .deploy(&id, &ui_context("application_deploy"))
        .await
    {
        Ok(result) => result_page("Deployment complete", &result.message),
        Err(error) => error_response(error),
    }
}

async fn app_rollback(
    State(state): State<UiState>,
    headers: HeaderMap,
    RoutePath(id): RoutePath<String>,
    Form(form): Form<ConfirmForm>,
) -> Response {
    if !authorized_post(&state, &headers, &form.csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match state
        .applications()
        .rollback(&id, &ui_context("application_rollback"))
    {
        Ok(result) => result_page("Rollback complete", &result.message),
        Err(error) => error_response(error),
    }
}

async fn apply_security_updates(
    State(state): State<UiState>,
    headers: HeaderMap,
    Form(form): Form<ConfirmForm>,
) -> Response {
    if !authorized_post(&state, &headers, &form.csrf) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match state
        .host_operator()
        .apply_updates(UpdateScope::Security, &ui_context("host_security_updates"))
        .await
    {
        Ok(result) => result_page("Security updates complete", &result.message),
        Err(error) => error_response(error),
    }
}

fn confirm(state: &UiState, headers: &HeaderMap, title: &str, message: &str) -> Response {
    let session = match auth(state, headers) {
        Some(value) => value,
        None => return Redirect::to("/login").into_response(),
    };
    page(title, &format!("<h1>{}</h1><form class=panel method=post><p>{}</p><input type=hidden name=csrf value=\"{}\"><button>Confirm</button></form>", escape(title), message, escape(&session.csrf)), true).into_response()
}

fn auth(state: &UiState, headers: &HeaderMap) -> Option<SessionRecord> {
    let id = session_cookie(headers)?;
    let mut sessions = state.sessions.lock().ok()?;
    let now = unix_seconds();
    sessions.retain(|_, record| record.expires_unix >= now);
    let session = sessions.get(&id)?.clone();
    let current_revision = UiCredentialStore::at_state_dir(&state.state_dir)
        .revision()
        .ok()
        .flatten();
    if !current_revision.is_some_and(|revision| {
        constant_time_eq(revision.as_bytes(), session.credential_revision.as_bytes())
    }) {
        sessions.remove(&id);
        None
    } else {
        Some(session)
    }
}

fn authorized_post(state: &UiState, headers: &HeaderMap, csrf: &str) -> bool {
    auth(state, headers)
        .is_some_and(|session| constant_time_eq(session.csrf.as_bytes(), csrf.as_bytes()))
}

fn session_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|part| {
            part.trim()
                .strip_prefix("lumic_session=")
                .map(str::to_owned)
        })
}

fn result_page(title: &str, message: &str) -> Response {
    page(
        title,
        &format!(
            "<h1>{}</h1><p class=flash>{}</p><p><a href=/>Return to overview</a></p>",
            escape(title),
            escape(message)
        ),
        true,
    )
    .into_response()
}
fn error_response(error: LumicError) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        page(
            "Operation failed",
            &format!(
                "<h1>Operation failed</h1><p class=bad>{}</p>",
                escape(&error.to_string())
            ),
            true,
        ),
    )
        .into_response()
}

fn deployment_html(item: &Deployment) -> String {
    let phases = item
        .phases
        .iter()
        .map(|phase| {
            format!(
                "<tr><td>{}</td><td>{:?}</td><td>{}</td></tr>",
                escape(&phase.name),
                phase.status,
                escape(&phase.message)
            )
        })
        .collect::<String>();
    format!(
        "<h1>Deployment {}</h1><p>{:?} · commit <span class=mono>{}</span></p><p>{}</p><table><tr><th>Phase</th><th>Status</th><th>Message</th></tr>{phases}</table>",
        escape(&item.id),
        item.status,
        escape(&item.commit),
        escape(&item.message)
    )
}

fn page(title: &str, body: &str, navigation: bool) -> Html<String> {
    let section = navigation.then(|| nav_section_for_page(title, body));
    page_for(title, body, section)
}

fn page_for(title: &str, body: &str, section: Option<NavSection>) -> Html<String> {
    let title = escape(title);
    let content = if let Some(section) = section {
        let navigation = navigation(section);
        format!(
            "<a class=skip-link href=#main-content>Skip to content</a><div class=shell><aside class=sidenav aria-label=Primary>{navigation}</aside><div class=content><div class=mobile-bar><a class=brand href=/><span class=brand-mark>L</span><span>Lumic</span></a><details class=mobile-nav><summary aria-label=\"Open navigation\">Menu</summary><div class=mobile-sheet>{navigation}</div></details></div><main id=main-content tabindex=-1>{body}</main></div></div>"
        )
    } else {
        format!(
            "<div class=login-shell><main class=login-panel id=main-content><a class=\"brand login-brand\" href=/><span class=brand-mark>L</span><span class=brand-copy>Lumic<small>Node operator</small></span></a>{body}</main></div>"
        )
    };
    Html(format!(
        "<!doctype html><html lang=en><head><meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1\"><meta name=color-scheme content=light><title>{title} · Lumic</title><style>{STYLE}</style></head><body>{content}</body></html>"
    ))
}

fn nav_section_for_page(title: &str, body: &str) -> NavSection {
    match title {
        "Applications"
        | "Deployment"
        | "Deployment complete"
        | "Deploy application"
        | "Rollback complete"
        | "Roll back application"
        | "Not found" => NavSection::Applications,
        "Services" | "Service logs" | "Restart service" | "Service restarted" => {
            NavSection::Services
        }
        "Recipes" => NavSection::Recipes,
        "Infrastructure" => NavSection::Infrastructure,
        "Host operator" | "Apply security updates" | "Security updates complete" => {
            NavSection::Host
        }
        "Events" => NavSection::Events,
        _ if body.contains("href=/actions/app/") || body.contains("href=/deployments/") => {
            NavSection::Applications
        }
        _ if body.contains("href=/actions/service/") || body.contains("href=/services/") => {
            NavSection::Services
        }
        _ => NavSection::Overview,
    }
}

fn navigation(active: NavSection) -> String {
    fn link(
        active: NavSection,
        section: NavSection,
        href: &str,
        icon: &str,
        label: &str,
    ) -> String {
        let current = if active == section {
            " aria-current=page"
        } else {
            ""
        };
        format!(
            "<a class=sidenav-link href={href}{current}><span class=nav-icon aria-hidden=true>{icon}</span><span>{label}</span></a>"
        )
    }

    let monitor = link(active, NavSection::Overview, "/", "01", "Overview");
    let applications = link(
        active,
        NavSection::Applications,
        "/apps",
        "02",
        "Applications",
    );
    let services = link(active, NavSection::Services, "/services", "03", "Services");
    let recipes = link(active, NavSection::Recipes, "/recipes", "04", "Recipes");
    let infrastructure = link(
        active,
        NavSection::Infrastructure,
        "/infrastructure",
        "05",
        "Infrastructure",
    );
    let host = link(active, NavSection::Host, "/host", "06", "Host");
    let events = link(active, NavSection::Events, "/events", "07", "Events");

    format!(
        "<div class=sidenav-header><a class=brand href=/><span class=brand-mark>L</span><span class=brand-copy>Lumic<small>Node operator</small></span></a><div class=node-chip>Local node</div></div><nav class=sidenav-content><section class=sidenav-group aria-labelledby=nav-monitor><div class=sidenav-label id=nav-monitor>Monitor</div><div class=sidenav-menu>{monitor}</div></section><section class=sidenav-group aria-labelledby=nav-workloads><div class=sidenav-label id=nav-workloads>Workloads</div><div class=sidenav-menu>{applications}{services}{recipes}</div></section><section class=sidenav-group aria-labelledby=nav-system><div class=sidenav-label id=nav-system>System</div><div class=sidenav-menu>{infrastructure}{host}{events}</div></section></nav><div class=sidenav-footer><div class=operator><span class=avatar aria-hidden=true>OP</span><span class=operator-copy><strong>Operator</strong><small>Local administrator</small></span></div><form method=post action=/logout><button class=signout type=submit>Sign out</button></form></div>"
    )
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn url_segment(value: &str) -> String {
    value
        .bytes()
        .filter(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        .map(char::from)
        .collect()
}
fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}
fn digest(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
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

fn random_token() -> Result<String> {
    use std::io::Read;
    let mut bytes = [0_u8; 32];
    fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(ui_io)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn ui_context(operation: &str) -> OperationContext {
    OperationContext {
        actor: "local-ui-admin".into(),
        interface: OperationInterface::Ui,
        correlation_id: format!("ui-{operation}-{}", unix_seconds()),
        dry_run: false,
        approved: true,
    }
}

fn ui_io(error: std::io::Error) -> LumicError {
    LumicError::Internal {
        message: format!("operator UI credential I/O failed: {error}"),
    }
}

fn session_error() -> LumicError {
    LumicError::Internal {
        message: "operator UI session store is unavailable".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("lumic-ui-{name}-{}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn credential_store_only_persists_a_private_hash() {
        let directory = temp_dir("credential");
        let store = UiCredentialStore::at_state_dir(&directory);
        let token = store.rotate().unwrap();
        let persisted = fs::read_to_string(directory.join("ui-admin-token.sha256")).unwrap();
        assert!(!persisted.contains(&token));
        assert!(store.verified_revision(&token).unwrap().is_some());
        assert!(store.verified_revision("wrong").unwrap().is_none());
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn unauthenticated_pages_redirect_and_login_sets_hardened_cookie() {
        let directory = temp_dir("auth");
        let token = UiCredentialStore::at_state_dir(&directory)
            .rotate()
            .unwrap();
        let app = router(UiState::new(&directory, directory.join("apps")));
        for uri in [
            "/apps",
            "/recipes",
            "/host",
            "/infrastructure",
            "/actions/host/security-updates",
        ] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::SEE_OTHER);
        }
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/infrastructure")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let response = app
            .clone()
            .oneshot(Request::builder().uri("/apps").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            response
                .headers()
                .get(header::X_CONTENT_TYPE_OPTIONS)
                .and_then(|value| value.to_str().ok()),
            Some("nosniff")
        );
        assert!(response.headers().contains_key("content-security-policy"));
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/login")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from(format!("token={token}")))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));

        UiCredentialStore::at_state_dir(&directory)
            .rotate()
            .unwrap();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/apps")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn session_store_is_bounded() {
        let directory = temp_dir("session-bound");
        let token = UiCredentialStore::at_state_dir(&directory)
            .rotate()
            .unwrap();
        let state = UiState::new(&directory, directory.join("apps"));

        for _ in 0..=MAX_SESSIONS {
            let response = login(
                State(state.clone()),
                Form(LoginForm {
                    token: token.clone(),
                }),
            )
            .await;
            assert_eq!(response.status(), StatusCode::SEE_OTHER);
        }

        assert_eq!(state.sessions.lock().unwrap().len(), MAX_SESSIONS);
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn repeated_login_failures_are_throttled_and_success_resets_failures() {
        let directory = temp_dir("login-throttle");
        let token = UiCredentialStore::at_state_dir(&directory)
            .rotate()
            .unwrap();
        let state = UiState::new(&directory, directory.join("apps"));

        for _ in 0..MAX_LOGIN_FAILURES {
            let response = login(
                State(state.clone()),
                Form(LoginForm {
                    token: "wrong".into(),
                }),
            )
            .await;
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
        let response = login(
            State(state.clone()),
            Form(LoginForm {
                token: token.clone(),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(response.headers().contains_key(header::RETRY_AFTER));

        state.login_throttle.lock().unwrap().failures.clear();
        let response = login(State(state), Form(LoginForm { token })).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn authenticated_shell_uses_grouped_route_aware_sidenav() {
        let html = page(
            "Applications",
            "<h1>Applications</h1><p>Managed workloads</p>",
            true,
        )
        .0;

        assert!(html.contains("class=sidenav"));
        assert!(html.contains("id=nav-monitor>Monitor"));
        assert!(html.contains("id=nav-workloads>Workloads"));
        assert!(html.contains("id=nav-system>System"));
        assert!(html.contains("href=/apps aria-current=page"));
        assert!(html.contains("class=mobile-nav"));
        assert!(html.contains("href=#main-content>Skip to content"));
    }

    #[test]
    fn detail_pages_keep_their_parent_navigation_active() {
        let application = page(
            "Example application",
            "<a href=/actions/app/example/deploy>Deploy</a>",
            true,
        )
        .0;
        let service = page(
            "PostgreSQL",
            "<a href=/services/postgresql/logs>Logs</a>",
            true,
        )
        .0;

        assert!(application.contains("href=/apps aria-current=page"));
        assert!(service.contains("href=/services aria-current=page"));
    }
}
