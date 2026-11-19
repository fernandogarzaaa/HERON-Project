//! Local HTTP server behind the desktop app and web UI.
//! Loopback-only by default; every API call requires the per-launch token.

use crate::agent::Session;
use crate::app::{App, ConfigChange};
use crate::config::Config;
use crate::events::Item;
use crate::permissions::{Approval, ApprovalRequest, Approver, Mode};
use anyhow::Result;
use async_trait::async_trait;
use axum::extract::{Path, Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;

const INDEX_HTML: &str = include_str!("../assets/index.html");

/// A live approval waiting for `POST /api/approve`.
struct PendingApproval {
    session_id: String,
    run_id: String,
    tx: oneshot::Sender<Approval>,
}

pub struct ServerState {
    pub app: Arc<App>,
    pub token: String,
    /// True unless the user deliberately bound the server to a non-loopback address.
    pub loopback_only: bool,
    approvals: Mutex<HashMap<String, PendingApproval>>,
    /// One-time, ten-minute OAuth authorization attempts. Never persisted or exposed to the model.
    oauth_pending: Mutex<HashMap<String, crate::oauth::Pending>>,
    /// Serialize callback token installation with account disconnect.
    oauth_transition: tokio::sync::Mutex<()>,
    /// Runs in progress, by conversation, with their run id: a run that is finishing can never
    /// clear the entry of the one that followed it. Only `POST /api/cancel` aborts a run.
    running: Mutex<HashMap<String, (String, tokio::task::JoinHandle<()>)>>,
}

impl ServerState {
    fn new(app: Arc<App>, token: String, loopback_only: bool) -> Arc<ServerState> {
        Arc::new(ServerState { app, token, loopback_only, approvals: Default::default(), oauth_pending: Default::default(), oauth_transition: Default::default(), running: Default::default() })
    }

    /// The run is over: clear its entry. False when a cancel got there first, in which case the
    /// cancel has published the run's last events.
    fn finish(&self, sid: &str, run_id: &str) -> bool {
        let mut running = self.running.lock().unwrap();
        if running.get(sid).is_some_and(|(id, _)| id == run_id) {
            running.remove(sid);
            true
        } else {
            false
        }
    }
}

type S = State<Arc<ServerState>>;
type ApiResult = std::result::Result<Json<Value>, ApiError>;

pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error": self.1}))).into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(e: E) -> Self {
        ApiError(StatusCode::BAD_REQUEST, format!("{:#}", e.into()))
    }
}

fn bad(msg: &str) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.to_string())
}

pub fn new_token() -> String {
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn host_only(value: &str) -> &str {
    let v = value.trim();
    if v.starts_with('[') {
        return v.split(']').next().map(|h| &v[..h.len() + 1]).unwrap_or(v);
    }
    v.rsplit_once(':').map(|(h, _)| h).unwrap_or(v)
}

fn is_loopback_name(host: &str) -> bool {
    matches!(host.to_lowercase().as_str(), "127.0.0.1" | "localhost" | "[::1]")
}

/// Defence in depth against other websites and DNS rebinding: besides the token, the request must be
/// addressed to this computer by a loopback name, and any Origin must be the app's own page.
fn request_is_local(headers: &HeaderMap) -> bool {
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
    if !is_loopback_name(host_only(host)) {
        return false;
    }
    match headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        None => true,
        Some(origin) => origin.strip_prefix("http://").map(|o| o.eq_ignore_ascii_case(host)).unwrap_or(false),
    }
}

async fn auth(State(st): S, headers: HeaderMap, req: Request, next: Next) -> Response {
    if st.loopback_only && !request_is_local(&headers) {
        return ApiError(StatusCode::FORBIDDEN, "Heron only answers requests made to this computer from its own window.".into()).into_response();
    }
    let supplied = headers
        .get("x-heron-token")
        .and_then(|v| v.to_str().ok())
        .or_else(|| headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")))
        .unwrap_or("");
    if !constant_time_eq(supplied, &st.token) {
        return ApiError(StatusCode::UNAUTHORIZED, "missing or invalid token — open Heron from the link it printed at startup".into()).into_response();
    }
    next.run(req).await
}

/// The router, loopback-only unless `[server] bind` names another address.
pub fn router(app: Arc<App>, token: String) -> Router {
    let bind = app.cfg().server.bind.clone();
    let loopback_only = matches!(bind.as_str(), "127.0.0.1" | "localhost" | "::1");
    router_with(app, token, loopback_only)
}

/// The router, with loopback-only checking as given (from the address actually bound).
pub fn router_with(app: Arc<App>, token: String, loopback_only: bool) -> Router {
    let state = ServerState::new(app, token, loopback_only);
    let api = Router::new()
        .route("/state", get(get_state))
        .route("/sessions", get(list_sessions).post(new_session))
        .route("/sessions/{id}", get(get_session).delete(delete_session))
        .route("/chat", post(chat))
        .route("/approve", post(approve))
        .route("/cancel", post(cancel))
        .route("/undo", post(undo))
        .route("/mode", post(set_mode))
        .route("/memories", get(list_memories).post(add_memory))
        .route("/memories/{id}", delete(delete_memory))
        .route("/memories/{id}/confirm", post(confirm_memory))
        .route("/tasks", get(list_tasks).post(add_task))
        .route("/tasks/{id}", post(update_task).delete(delete_task))
        .route("/schedules", get(list_schedules).post(add_schedule))
        .route("/schedules/{id}", post(update_schedule).delete(delete_schedule))
        .route("/schedules/{id}/run", post(run_schedule))
        .route("/schedules/{id}/runs", get(schedule_runs))
        .route("/inbox", get(get_inbox))
        .route("/inbox/read", post(read_inbox))
        .route("/audit", get(get_audit))
        .route("/awareness", get(get_awareness).post(set_awareness).delete(clear_awareness))
        .route("/mcp/{name}/accept", post(accept_mcp))
        // Connector directory is metadata only. Creating registrations or
        // storing API keys never activates a remote tool or exposes a key.
        .route("/connectors", get(list_connectors))
        .route("/connectors/custom", post(add_custom_connector))
        .route("/connectors/custom/{id}", delete(delete_custom_connector))
        .route("/connectors/custom/{id}/api-key", post(set_connector_api_key).delete(delete_connector_api_key))
        .route("/persona", get(get_persona).post(set_persona).delete(reset_persona))
        .route("/pause", post(set_pause))
        .route("/backup", post(make_backup))
        .route("/approvals", get(list_approvals))
        .route("/approvals/{id}", post(answer_approval))
        .route("/tools", get(list_tools))
        .route("/tools/{name}", post(set_tool))
        .route("/today", get(get_today))
        .route("/telegram", get(get_telegram).post(set_telegram).delete(remove_telegram))
        .route("/usage", get(get_usage).post(set_usage))
        .route("/sources", get(get_sources))
        .route("/oauth", get(get_oauth_status))
        .route("/oauth/{provider}/start", post(start_oauth))
        .route("/oauth/{provider}/sync", post(sync_oauth))
        .route("/oauth/{provider}", delete(disconnect_oauth))
        .route("/sources/{name}", post(connect_source).delete(disconnect_source))
        .route("/sources/{name}/learn", post(learn_source))
        .route("/onboarded", post(set_onboarded))
        .route("/suggestions", get(list_suggestions))
        .route("/suggestions/{id}", post(answer_suggestion))
        .route("/settings", get(get_settings).post(set_settings))
        .route("/events", get(get_events))
        .route("/risk-notice", get(get_risk_notice).post(accept_risk_notice))
        .route("/providers", get(list_providers))
        .route("/providers/{id}/accept-version", post(accept_provider_version))
        .route("/grants", get(list_grants).delete(revoke_grant))
        .layer(middleware::from_fn_with_state(state.clone(), auth));
    Router::new()
        .route("/", get(|| async { ([(header::CACHE_CONTROL, "no-store"), (header::HeaderName::from_static("content-security-policy"), "default-src 'self'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'")], Html(INDEX_HTML)) }))
        // OAuth provider redirects cannot carry Heron\'s API bearer token; the callback is
        // authenticated by an unguessable, single-use state and a loopback Host check.
        .route("/oauth/callback/{provider}", get(oauth_callback))
        .nest("/api", api)
        .with_state(state)
}

pub async fn serve(app: Arc<App>, listener: tokio::net::TcpListener, token: String) -> Result<()> {
    // Before anything can be started from the app: if this Heron is to own background work, it takes
    // ownership (and reports what the previous one left unfinished) now, never in the middle of its own work.
    app.own_background_work();
    run_server(app.clone(), listener, router(app, token)).await
}

async fn run_server(app: Arc<App>, listener: tokio::net::TcpListener, router: Router) -> Result<()> {
    tokio::spawn(app.clone().scheduler_loop());
    tokio::spawn(app.clone().observer_loop());
    crate::telegram::spawn_if_configured(&app);
    axum::serve(listener, router).await?;
    Ok(())
}

/// A server that is running: where it is, its token, and whether this core runs background work.
pub struct Started {
    /// The launch URL, with the token.
    pub url: String,
    pub port: u16,
    pub token: String,
    /// False when another Heron on this home already holds `background.lock`.
    pub background_owner: bool,
    pub task: tokio::task::JoinHandle<Result<()>>,
}

/// Bind and start serving in the background. Returns the launch URL (with token) and the server task.
pub async fn start(app: Arc<App>, bind: &str, port: u16) -> Result<(String, tokio::task::JoinHandle<Result<()>>)> {
    let started = start_endpoint(app, bind, port).await?;
    Ok((started.url, started.task))
}

/// Bind and start serving in the background. Loopback-only checking follows the address bound.
pub async fn start_endpoint(app: Arc<App>, bind: &str, port: u16) -> Result<Started> {
    let listener = match tokio::net::TcpListener::bind((bind, port)).await {
        Ok(l) => l,
        // The preferred port is taken (perhaps another Heron window): fall back to any free port.
        Err(_) => tokio::net::TcpListener::bind((bind, 0)).await?,
    };
    let addr = listener.local_addr()?;
    let token = new_token();
    let host = if addr.ip().is_unspecified() || addr.ip().is_loopback() { "127.0.0.1".to_string() } else { addr.ip().to_string() };
    let url = format!("http://{host}:{}/?token={token}", addr.port());
    // Before anything can be started from the app: if this Heron is to own background work, it takes
    // ownership (and reports what the previous one left unfinished) now, never in the middle of its own work.
    let background_owner = app.own_background_work();
    let router = router_with(app.clone(), token.clone(), addr.ip().is_loopback());
    let task = tokio::spawn(run_server(app, listener, router));
    Ok(Started { url, port: addr.port(), token, background_owner, task })
}

/// The one line `heron serve --print-endpoint` prints for the Mac app.
pub fn endpoint_line(started: &Started) -> String {
    json!({"port": started.port, "token": started.token, "background_owner": started.background_owner, "version": env!("CARGO_PKG_VERSION")}).to_string()
}

// ---------- state & settings ----------

async fn get_state(State(st): S) -> ApiResult {
    let cfg = st.app.cfg();
    let unread = st.app.store.inbox(true, 100)?.len();
    let usage = st.app.usage.lock().unwrap().clone();
    Ok(Json(json!({
        "name": cfg.agent.name, "version": env!("CARGO_PKG_VERSION"), "model": cfg.llm.model, "mode": st.app.policy.mode().name(),
        "workspace": st.app.workspace.display().to_string(), "has_key": cfg.api_key().is_some() || is_local(&cfg.llm.base_url) || crate::llm::is_demo(&cfg), "demo": crate::llm::is_demo(&cfg),
        "memories": st.app.store.memory_count(), "unread": unread, "paused": st.app.paused(), "waiting": st.app.store.pending_approvals()?.len(), "onboarded": st.app.store.kv_get("onboarded").is_some(), "suggestions": st.app.store.list_suggestions(true, 50)?.len(), "mcp": st.app.mcp_status, "usage": usage,
        "tools": st.app.tools.names().len(), "os": std::env::consts::OS, "sandbox": crate::sandbox::active(&cfg).name(),
        "risk_notice_accepted": crate::risk_notice::accepted(&st.app.store), "risk_notice_required": crate::risk_notice::required(),
    })))
}

fn is_local(base_url: &str) -> bool {
    base_url.contains("://localhost") || base_url.contains("://127.0.0.1") || base_url.contains("://[::1]")
}

async fn get_settings(State(st): S) -> ApiResult {
    let cfg = st.app.cfg();
    let key_source = if std::env::var("HERON_API_KEY").is_ok() || std::env::var(&cfg.llm.api_key_env).map(|k| !k.is_empty()).unwrap_or(false) {
        "environment"
    } else if cfg.stored_api_key().is_some() {
        "saved"
    } else {
        "none"
    };
    let providers: Vec<Value> = crate::cli_llm::providers().iter().map(|s| json!({"id": s.id, "name": s.display_name, "auth": s.auth_kind, "cost": s.cost_note, "login": s.login_command, "experimental": s.experimental})).collect();
    Ok(Json(json!({
        "name": cfg.agent.name, "base_url": cfg.llm.base_url, "model": cfg.llm.model, "fast_model": cfg.llm.fast_model,
        "fallback_models": cfg.llm.fallback_models.join(", "), "key_source": key_source,
        "provider": cfg.llm.provider, "cli_command": cfg.llm.cli_command, "cli_model": cfg.llm.cli_model,
        "cli_config_dir": cfg.llm.cli_config_dir, "cli_allow_agent_tools": cfg.llm.cli_allow_agent_tools,
        "cli_allow_api_key": cfg.llm.cli_allow_api_key, "cli_subscription": cfg.llm.cli_subscription,
        "allow_unattended_subscription": cfg.llm.allow_unattended_subscription, "providers": providers,
        "mode": st.app.policy.mode().name(), "daemon_mode": cfg.permissions.daemon_mode, "auto_memory": cfg.agent.auto_memory,
        "neural_memory": cfg.agent.neural_memory, "neural_available": crate::embed::neural_built_in(),
        "browser": crate::browser::find(&cfg).map(|p| p.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()), "browser_visible": cfg.agent.browser_visible,
        "workspace": st.app.workspace.display().to_string(), "home": cfg.home.display().to_string(),
        "allow_rules": st.app.policy.allow_rules(), "secret_storage": cfg.secret_storage(),
        "rule_notices": st.app.policy.rule_notices().into_iter().map(|(rule, notice)| json!({"rule": rule, "notice": notice})).collect::<Vec<_>>(),
        "mcp": st.app.mcp_status, "mcp_changed": st.app.mcp_pending.keys().collect::<Vec<_>>(),
    })))
}

async fn set_settings(State(st): S, Json(body): Json<Value>) -> ApiResult {
    // Every value is checked first, on a copy, so a bad one changes nothing, the API key included.
    apply_settings(&body, &mut (*st.app.cfg()).clone())?;
    let text = |k: &str| body.get(k).and_then(|v| v.as_str()).map(|s| s.trim().to_string());
    if let Some(key) = text("api_key").filter(|k| !k.is_empty()) {
        st.app.cfg().store_api_key(&key)?;
    }
    // Applied to the settings as they are when they are saved, never to a copy held across an await.
    let mut restart_browser = false;
    st.app.update_config(ConfigChange::try_edit(|cfg| {
        restart_browser = body.get("browser_visible").and_then(|v| v.as_bool()).is_some_and(|v| v != cfg.agent.browser_visible);
        apply_settings(&body, cfg).map_err(|e| anyhow::anyhow!(e.1))
    }))?;
    if let Some(v) = text("mode") {
        st.app.policy.set_mode(Mode::parse(&v)?);
    }
    if restart_browser {
        crate::browser::shutdown().await; // the next use starts it the new way
    }
    get_settings(State(st)).await
}

/// Set in `cfg` what a `POST /api/settings` body asks for, or say what in it is wrong.
fn apply_settings(body: &Value, cfg: &mut Config) -> std::result::Result<(), ApiError> {
    let text = |k: &str| body.get(k).and_then(|v| v.as_str()).map(|s| s.trim().to_string());
    if let Some(v) = text("name").filter(|v| !v.is_empty()) {
        cfg.agent.name = v;
    }
    if let Some(v) = text("base_url").filter(|v| v.eq_ignore_ascii_case("demo")) {
        cfg.llm.base_url = v;
    } else if let Some(v) = text("base_url").filter(|v| !v.is_empty()) {
        let u = url::Url::parse(&v).map_err(|_| bad("The endpoint URL is not valid. It should look like https://openrouter.ai/api/v1"))?;
        if !matches!(u.scheme(), "http" | "https") {
            return Err(bad("The endpoint URL must start with http:// or https://"));
        }
        cfg.llm.base_url = v;
    }
    if let Some(v) = text("model").filter(|v| !v.is_empty()) {
        cfg.llm.model = v;
    }
    if let Some(v) = text("fast_model") {
        cfg.llm.fast_model = v;
    }
    if let Some(v) = text("fallback_models") {
        cfg.llm.fallback_models = v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    }
    if let Some(v) = text("provider") {
        if v != "http" && crate::cli_llm::spec_for(&v).is_none() {
            return Err(bad(
                "Unknown provider. Pick \"http\" or one of the ids listed by `heron providers`.",
            ));
        }
        cfg.llm.provider = v;
    }
    if let Some(v) = text("cli_command") {
        cfg.llm.cli_command = v;
    }
    if let Some(v) = text("cli_model") {
        cfg.llm.cli_model = v;
    }
    if let Some(v) = text("cli_config_dir") {
        cfg.llm.cli_config_dir = v;
    }
    if let Some(v) = body.get("cli_allow_agent_tools").and_then(|v| v.as_bool()) {
        cfg.llm.cli_allow_agent_tools = v;
    }
    if let Some(v) = body.get("cli_allow_api_key") {
        cfg.llm.cli_allow_api_key = match v {
            Value::Bool(b) => crate::config::ApiKeyAllow::Bool(*b),
            Value::Array(ids) => crate::config::ApiKeyAllow::List(
                ids.iter().map(|id| id.as_str().map(|s| s.trim().to_string()).ok_or_else(|| bad("cli_allow_api_key must be true, false or a list of provider ids."))).collect::<std::result::Result<Vec<_>, _>>()?,
            ),
            _ => return Err(bad("cli_allow_api_key must be true, false or a list of provider ids.")),
        };
    }
    if let Some(v) = body.get("cli_subscription") {
        let Some(map) = v.as_object() else { return Err(bad("cli_subscription must be an object of {\"<provider id>\": true or false}.")) };
        let mut subs = std::collections::BTreeMap::new();
        for (id, flag) in map {
            let Some(flag) = flag.as_bool() else { return Err(bad("cli_subscription must be an object of {\"<provider id>\": true or false}.")) };
            subs.insert(id.clone(), flag);
        }
        cfg.llm.cli_subscription = subs;
    }
    if let Some(v) = body.get("allow_unattended_subscription").and_then(|v| v.as_bool()) {
        cfg.llm.allow_unattended_subscription = v;
    }
    if let Some(v) = text("daemon_mode") {
        Mode::parse(&v)?;
        cfg.permissions.daemon_mode = v;
    }
    if let Some(v) = text("mode") {
        Mode::parse(&v)?;
        cfg.permissions.mode = v;
    }
    if let Some(v) = body.get("auto_memory").and_then(|v| v.as_bool()) {
        cfg.agent.auto_memory = v;
    }
    if let Some(v) = body.get("browser_visible").and_then(|v| v.as_bool()) {
        cfg.agent.browser_visible = v;
    }
    if let Some(v) = body.get("neural_memory").and_then(|v| v.as_bool()) {
        cfg.agent.neural_memory = v && crate::embed::neural_built_in();
    }
    if let Some(v) = text("workspace").filter(|v| !v.is_empty()) {
        cfg.agent.workspace = v;
    }
    Ok(())
}

#[derive(Deserialize)]
struct ModeBody {
    mode: String,
}

async fn set_mode(State(st): S, Json(b): Json<ModeBody>) -> ApiResult {
    let mode = Mode::parse(&b.mode)?;
    // The config file stays the one source of truth, so the mode survives a restart. It is the
    // user's explicit choice, so it is written even when `--mode` already set the same mode.
    st.app.update_config_choosing(ConfigChange::edit(|c| c.permissions.mode = mode.name().to_string()), &["permissions.mode"])?;
    st.app.policy.set_mode(mode);
    Ok(Json(json!({"mode": st.app.policy.mode().name()})))
}

// ---------- sessions ----------

async fn list_sessions(State(st): S) -> ApiResult {
    Ok(Json(json!(st.app.store.list_sessions(100)?)))
}

async fn new_session(State(st): S) -> ApiResult {
    Ok(Json(json!({"id": st.app.store.create_session("", "user")?})))
}

async fn get_session(State(st): S, Path(id): Path<String>) -> ApiResult {
    let id = st.app.store.resolve_session(&id).map_err(|e| ApiError(StatusCode::NOT_FOUND, e.to_string()))?;
    let busy = st.running.lock().unwrap().contains_key(&id);
    let messages = st.app.store.load_messages(&id)?;
    // Every step is saved as it happens, so a reply cut short (a crash, a closed window) can be picked up again.
    let interrupted = !busy && messages.last().map(|m| m.role == "tool" || m.role == "user" || !m.tool_calls.is_empty()).unwrap_or(false);
    Ok(Json(json!({"id": id, "title": st.app.store.session_title(&id), "messages": messages, "busy": busy, "interrupted": interrupted})))
}

async fn delete_session(State(st): S, Path(id): Path<String>) -> ApiResult {
    if st.running.lock().unwrap().contains_key(&id) {
        return Err(ApiError(StatusCode::CONFLICT, "Stop the running reply before deleting this conversation.".into()));
    }
    st.app.store.delete_session(&id)?;
    Ok(Json(json!({"ok": true})))
}

// ---------- chat ----------

struct WebApprover {
    state: Arc<ServerState>,
    session_id: String,
    run_id: String,
}

#[async_trait]
impl Approver for WebApprover {
    async fn approve(&self, req: &ApprovalRequest) -> Approval {
        let app = &self.state.app;
        let id = uuid::Uuid::new_v4().simple().to_string();
        let (tx, mut rx) = oneshot::channel();
        self.state.approvals.lock().unwrap().insert(id.clone(), PendingApproval { session_id: self.session_id.clone(), run_id: self.run_id.clone(), tx });
        // The approval waits on the bus, not on one connection: any client can answer it.
        app.events.publish(&self.session_id, &self.run_id, json!({"type": "approval", "id": id, "tool": req.tool, "summary": req.summary, "risk": req.risk.label(), "detail": req.detail,
            "always_rule": req.always_rule, "can_always": req.can_always(), "can_session": crate::agent::may_grant_for_conversation(&req.tool, req.risk, req.tainted), "tainted": req.tainted}));
        let wait = Duration::from_secs(app.cfg().server.approval_timeout_secs);
        match tokio::time::timeout(wait, &mut rx).await {
            Ok(Ok(answer)) => return answer,
            // Cancelled: `cancel` cleared it and published `approval_resolved`.
            Ok(Err(_)) => return Approval::Deny(Some("the reply was stopped".into())),
            Err(_) => {}
        }
        // Nobody answered in time. If an answer or a cancel took the entry at this very moment, it wins.
        if self.state.approvals.lock().unwrap().remove(&id).is_none() {
            return rx.await.unwrap_or(Approval::Deny(Some("the reply was stopped".into())));
        }
        // Park it with today's deny-and-carry-on: the user can still approve it later, from the list
        // of waiting approvals. The UUID is never reused.
        let origin = app.store.session_origin(&self.session_id).unwrap_or_else(|| "user".into());
        match app.store.park_approval(&self.session_id, &req.tool, &req.args.to_string(), &req.summary, &req.detail, req.risk.label(), req.tainted, &origin) {
            Ok(parked_id) => {
                app.events.publish(&self.session_id, &self.run_id, json!({"type": "approval_parked", "id": id, "parked_id": parked_id}));
                Approval::Deny(Some(crate::permissions::PARKED_FOR_LATER.into()))
            }
            Err(_) => {
                app.events.publish(&self.session_id, &self.run_id, json!({"type": "approval_resolved", "id": id, "decision": "deny"}));
                Approval::Deny(Some("no answer to the approval request".into()))
            }
        }
    }
}

#[derive(Deserialize)]
struct ChatBody {
    session_id: Option<String>,
    message: String,
}

fn session_busy() -> Response {
    (StatusCode::CONFLICT, Json(json!({"error": "session_busy", "message": "Heron is still replying in this conversation. Stop it first, or wait."}))).into_response()
}

async fn chat(State(st): S, Json(body): Json<ChatBody>) -> std::result::Result<Response, ApiError> {
    let message = body.message.trim().to_string();
    if message.is_empty() {
        return Err(bad("Type a message first."));
    }
    // Checked before anything starts: once the SSE response begins, its status can no longer change.
    if crate::risk_notice::blocks(&st.app.store) {
        return Err(ApiError(StatusCode::PRECONDITION_REQUIRED, "risk_notice_not_accepted".into()));
    }
    if let Some(id) = body.session_id.as_deref().filter(|s| !s.is_empty()) {
        if st.running.lock().unwrap().contains_key(id) {
            return Ok(session_busy());
        }
    }
    let session = match body.session_id.as_deref().filter(|s| !s.is_empty()) {
        Some(id) => Session::load(&st.app.store, id).map_err(|e| ApiError(StatusCode::NOT_FOUND, e.to_string()))?,
        None => Session::create(&st.app.store, "")?,
    };
    let sid = session.id.clone();
    let run_id = crate::events::new_run_id();
    // Subscribed before the run's first event, so this response sees all of them.
    let events = st.app.events.subscribe(None);
    {
        let mut running = st.running.lock().unwrap();
        if running.contains_key(&sid) {
            return Ok(session_busy());
        }
        st.app.publish_run_start(&sid, &run_id);
        let handle = tokio::spawn(run_chat(st.clone(), session, message, run_id.clone()));
        running.insert(sid, (run_id.clone(), handle));
    }
    // Dropping this response, or its falling behind, ends only the response: the run goes on, and a
    // client picks it up again from `/api/events`.
    let stream = run_view(events, run_id).map(|item| Ok::<_, std::convert::Infallible>(sse(&item)));
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))).into_response())
}

/// One run's events from a subscription, ending right after its `run_end` (or a `resync`), without
/// waiting for whatever the bus publishes next.
fn run_view(events: crate::events::Subscription, run_id: String) -> impl futures_util::Stream<Item = Item> + Send {
    futures_util::stream::unfold((events, run_id, false), |(mut sub, run_id, ended)| async move {
        if ended {
            return None;
        }
        loop {
            let item = sub.next().await?;
            let last = match &item {
                Item::Event(e) if e.run_id != run_id => continue,
                Item::Event(e) => e.kind() == "run_end",
                Item::Resync { .. } => true,
            };
            return Some((item, (sub, run_id, last)));
        }
    })
}

fn sse(item: &Item) -> SseEvent {
    match item {
        Item::Event(e) => SseEvent::default().id(e.seq.to_string()).data(e.envelope().to_string()),
        Item::Resync { .. } => SseEvent::default().data(item.json().to_string()),
    }
}

/// One chat run, as its own task. Whoever ends the run publishes its last events: this task when the
/// turn ends, or `cancel` when it stops the run first.
async fn run_chat(st: Arc<ServerState>, mut session: Session, message: String, run_id: String) {
    let sid = session.id.clone();
    // Rules another Heron on this home revoked or added since this one last looked apply from here.
    st.app.reload_rules();
    let agent = st.app.agent(Arc::new(WebApprover { state: st.clone(), session_id: sid.clone(), run_id: run_id.clone() }));
    let outcome = agent.run_turn(&mut session, &message, &st.app.sink_for(&sid, &run_id)).await;
    // Free the conversation before the (slower) background learning step.
    if !st.finish(&sid, &run_id) {
        return;
    }
    if outcome.is_ok() {
        st.app.persist_rules();
    }
    st.app.publish_outcome(&sid, &run_id, outcome.as_deref());
    if let Ok(text) = &outcome {
        let learned = agent.learn(&session, &message, text).await;
        if !learned.is_empty() {
            st.app.events.publish(&sid, &run_id, json!({"type": "learned", "items": learned}));
        }
    }
    st.app.publish_run_end(&sid, &run_id);
}

#[derive(Deserialize)]
struct ApproveBody {
    id: String,
    decision: String,
}

async fn approve(State(st): S, Json(b): Json<ApproveBody>) -> ApiResult {
    let Some(pending) = st.approvals.lock().unwrap().remove(&b.id) else {
        return Err(ApiError(StatusCode::NOT_FOUND, "That request is no longer waiting for an answer.".into()));
    };
    let (answer, decision) = match b.decision.as_str() {
        "once" => (Approval::Once, "once"),
        "always" => (Approval::Always, "always"),
        "session" => (Approval::Session, "session"),
        _ => (Approval::Deny(None), "deny"),
    };
    st.app.events.publish(&pending.session_id, &pending.run_id, json!({"type": "approval_resolved", "id": b.id, "decision": decision}));
    let _ = pending.tx.send(answer);
    Ok(Json(json!({"ok": true})))
}

#[derive(Deserialize)]
struct SessionBody {
    session_id: String,
}

async fn cancel(State(st): S, Json(b): Json<SessionBody>) -> ApiResult {
    let (entry, cleared) = {
        let mut running = st.running.lock().unwrap();
        let entry = running.remove(&b.session_id);
        // Stopped before its approvals are dropped, so a refused approval cannot wake the run for one
        // more step.
        if let Some((_, handle)) = &entry {
            handle.abort();
        }
        // A new chat must not register its approval between removing the
        // previous run and clearing that previous run's approvals.
        let mut approvals = st.approvals.lock().unwrap();
        let ids: Vec<String> = approvals.iter().filter(|(_, p)| p.session_id == b.session_id).map(|(id, _)| id.clone()).collect();
        let cleared: Vec<(String, PendingApproval)> = ids.into_iter().filter_map(|id| approvals.remove(&id).map(|p| (id, p))).collect();
        (entry, cleared)
    };
    for (id, p) in cleared {
        st.app.events.publish(&p.session_id, &p.run_id, json!({"type": "approval_resolved", "id": id, "decision": "cancelled"}));
    }
    let Some((run_id, handle)) = entry else {
        return Ok(Json(json!({"ok": true, "cancelled": false})));
    };
    // The task may be in the middle of a step on another thread. Once it has stopped it publishes
    // nothing more, so waiting for that keeps `run_end` the run's last event. The run has left
    // `running`, so its end is published by a task of its own: a client that drops this request
    // while it waits cannot leave the run without `run_end`.
    let ended = tokio::spawn(async move {
        let _ = handle.await;
        // An approval that step registered after the ones above were cleared goes too.
        let late: Vec<(String, PendingApproval)> = {
            let mut approvals = st.approvals.lock().unwrap();
            let ids: Vec<String> = approvals.iter().filter(|(_, p)| p.run_id == run_id).map(|(id, _)| id.clone()).collect();
            ids.into_iter().filter_map(|id| approvals.remove(&id).map(|p| (id, p))).collect()
        };
        for (id, p) in late {
            st.app.events.publish(&p.session_id, &p.run_id, json!({"type": "approval_resolved", "id": id, "decision": "cancelled"}));
        }
        st.app.events.publish(&b.session_id, &run_id, json!({"type": "cancelled"}));
        st.app.publish_run_end(&b.session_id, &run_id);
    });
    let _ = ended.await;
    Ok(Json(json!({"ok": true, "cancelled": true})))
}

// ---------- the event bus, the risk notice, providers and grants ----------

#[derive(Deserialize)]
struct EventsQuery {
    since: Option<String>,
}

/// Every event on the bus as SSE: after `since` when given, otherwise new events only.
async fn get_events(State(st): S, Query(q): Query<EventsQuery>) -> std::result::Result<Response, ApiError> {
    let since = match q.since.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => Some(s.parse::<u64>().map_err(|_| bad("since must be a sequence number"))?),
        None => None,
    };
    let stream = st.app.events.subscribe(since).into_stream().map(|item| Ok::<_, std::convert::Infallible>(sse(&item)));
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))).into_response())
}

async fn get_risk_notice(State(st): S) -> ApiResult {
    Ok(Json(json!({"version": crate::risk_notice::version(), "text": crate::risk_notice::TEXT, "accepted_version": crate::risk_notice::accepted_version(&st.app.store), "required": crate::risk_notice::required()})))
}

#[derive(Deserialize)]
struct RiskNoticeBody {
    version: u32,
}

async fn accept_risk_notice(State(st): S, Json(b): Json<RiskNoticeBody>) -> std::result::Result<StatusCode, ApiError> {
    if b.version != crate::risk_notice::version() {
        return Err(ApiError(StatusCode::CONFLICT, format!("Version {} is not the current risk notice ({}). Reload it and read it again.", b.version, crate::risk_notice::version())));
    }
    crate::risk_notice::accept(&st.app.store, b.version)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Every CLI provider: installed, its version against the tested range, and the login.
async fn list_providers(State(st): S) -> ApiResult {
    let cfg = st.app.cfg();
    let specs = crate::cli_llm::providers();
    let probed = futures_util::future::join_all(specs.iter().map(|spec| crate::cli_llm::probe_status(&cfg, spec))).await;
    let items: Vec<Value> = specs
        .iter()
        .zip(probed)
        .map(|(spec, status)| {
            let output = status.version.clone().unwrap_or_default();
            let parsed = crate::cli_llm::extract_version(&output);
            let version = status.version.as_ref().map(|_| match parsed {
                Some((a, b, c)) => format!("{a}.{b}.{c}"),
                None => output.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string(),
            });
            let accepted = st.app.store.kv_get(&crate::cli_llm::version_ok_key(spec.id));
            let in_range = match (spec.supported, status.version.as_ref()) {
                (Some(range), Some(_)) => Some(parsed.is_some_and(|v| range.contains(v))),
                _ => None,
            };
            json!({
                "id": spec.id, "display_name": spec.display_name, "installed": status.installed, "version": version,
                "supported": spec.supported.map(|r| format!(">={}.{}.{} <{}.{}.{}", r.min.0, r.min.1, r.min.2, r.below.0, r.below.1, r.below.2)),
                "in_range": in_range, "logged_in": status.logged_in, "experimental": spec.experimental,
                "version_accepted": version.is_some() && accepted.is_some() && accepted == version,
            })
        })
        .collect();
    Ok(Json(json!(items)))
}

#[derive(Deserialize)]
struct AcceptVersionBody {
    version: String,
}

/// The user accepts one untested version of a CLI (CUJ 5).
async fn accept_provider_version(State(st): S, Path(id): Path<String>, Json(b): Json<AcceptVersionBody>) -> ApiResult {
    if crate::cli_llm::spec_for(&id).is_none() {
        return Err(ApiError(StatusCode::NOT_FOUND, "Unknown provider.".into()));
    }
    let version = b.version.trim();
    if version.is_empty() {
        return Err(bad("Say which version to accept."));
    }
    st.app.store.kv_set(&crate::cli_llm::version_ok_key(&id), version)?;
    Ok(Json(json!({"id": id, "version_accepted": version})))
}

/// Every standing allow rule: folder reads (`read:`) and tool rules. While `config.toml` cannot be
/// read, none of its rules are listed (they do not apply), and the `x-heron-notice` header says why;
/// the body stays the plain list, so a client that does not know the header is unaffected.
async fn list_grants(State(st): S) -> std::result::Result<Response, ApiError> {
    // Including what another Heron on this home granted or revoked since this one last looked.
    st.app.reload_rules();
    let grants: Vec<Value> = st.app.policy.allow_rules().into_iter().map(|rule| json!({"kind": if rule.starts_with("read:") { "read" } else { "tool" }, "rule": rule})).collect();
    let mut response = Json(json!(grants)).into_response();
    if st.app.config_unreadable().is_some() {
        response.headers_mut().insert("x-heron-notice", header::HeaderValue::from_static(crate::app::CONFIG_UNREADABLE_NOTICE));
    }
    Ok(response)
}

#[derive(Deserialize)]
struct GrantBody {
    rule: String,
}

/// Revoke a standing rule everywhere it lives: the policy, the saved "always" answers and the
/// settings file. When the settings file cannot be updated, nothing is changed.
async fn revoke_grant(State(st): S, Json(b): Json<GrantBody>) -> ApiResult {
    if !st.app.revoke_grant(&b.rule)? {
        return Err(ApiError(StatusCode::NOT_FOUND, "That permission is not granted.".into()));
    }
    Ok(Json(json!({"ok": true, "rule": b.rule})))
}

#[derive(Deserialize)]
struct UndoBody {
    session_id: String,
    /// Restore even though the file has changed since (the current version is kept beside it).
    #[serde(default)]
    force: bool,
    /// With `force`: the change the person was asked about, as named in the refusal. Restoring
    /// anyway applies to that one only.
    #[serde(default)]
    checkpoint: Option<String>,
}

async fn undo(State(st): S, Json(b): Json<UndoBody>) -> ApiResult {
    use crate::tools::fs::Undo;
    let agent = st.app.agent(Arc::new(crate::permissions::DenyAll));
    let restore_anyway = match (b.force, b.checkpoint.as_deref()) {
        (false, _) => None,
        (true, Some(id)) => Some(id),
        (true, None) => return Err(ApiError(StatusCode::BAD_REQUEST, "restoring anyway needs the checkpoint that was asked about".into())),
    };
    Ok(Json(match crate::tools::fs::undo(&agent.tool_ctx(&b.session_id), restore_anyway)? {
        Undo::Nothing => json!({"undone": null}),
        Undo::Done(said) => json!({"undone": said}),
        Undo::Conflict { id, why } => json!({"undone": null, "conflict": why, "checkpoint": id}),
        Undo::Stale(why) => json!({"undone": null, "stale": why}),
    }))
}

// ---------- memory / tasks / schedules / inbox / audit ----------

#[derive(Deserialize)]
struct SearchQuery {
    q: Option<String>,
}

async fn list_memories(State(st): S, Query(q): Query<SearchQuery>) -> ApiResult {
    let items = match q.q.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
        Some(query) => st.app.memory.search(query, 50).await?,
        None => st.app.store.list_memories(None, 500)?,
    };
    Ok(Json(json!(items)))
}

async fn add_memory(State(st): S, Json(b): Json<Value>) -> ApiResult {
    let content = b.get("content").and_then(|c| c.as_str()).map(str::trim).filter(|c| c.len() >= 3).ok_or_else(|| bad("Write what Heron should remember."))?;
    let kind = b.get("kind").and_then(|c| c.as_str()).unwrap_or("fact");
    let out = st.app.memory.save(content, kind, "", 0.7, "user").await?;
    Ok(Json(json!({"id": out.id()})))
}

async fn confirm_memory(State(st): S, Path(id): Path<i64>) -> ApiResult {
    Ok(Json(json!({"confirmed": st.app.store.confirm_memory(id)?})))
}

async fn delete_memory(State(st): S, Path(id): Path<i64>) -> ApiResult {
    Ok(Json(json!({"deleted": st.app.store.delete_memory(id)?})))
}

async fn list_tasks(State(st): S) -> ApiResult {
    Ok(Json(json!(st.app.store.list_tasks(None)?)))
}

async fn add_task(State(st): S, Json(b): Json<Value>) -> ApiResult {
    let title = b.get("title").and_then(|c| c.as_str()).map(str::trim).filter(|c| !c.is_empty()).ok_or_else(|| bad("Give the task a title."))?;
    let id = st.app.store.add_task(title, "", 2, b.get("due").and_then(|d| d.as_str()).filter(|d| !d.is_empty()))?;
    if b.get("owner").and_then(|o| o.as_str()) == Some("heron") {
        st.app.store.set_task_owner(id, "heron")?;
    }
    Ok(Json(json!({"id": id})))
}

async fn update_task(State(st): S, Path(id): Path<i64>, Json(b): Json<Value>) -> ApiResult {
    let s = |k: &str| b.get(k).and_then(|v| v.as_str());
    if let Some(owner) = s("owner") {
        st.app.store.set_task_owner(id, owner)?;
    }
    Ok(Json(json!({"updated": st.app.store.update_task(id, s("title"), s("notes"), s("status"), b.get("priority").and_then(|p| p.as_i64()), s("due"))?})))
}

async fn delete_task(State(st): S, Path(id): Path<i64>) -> ApiResult {
    Ok(Json(json!({"deleted": st.app.store.delete_task(id)?})))
}

async fn list_schedules(State(st): S) -> ApiResult {
    let items: Vec<Value> = st
        .app
        .store
        .list_schedules()?
        .into_iter()
        .map(|s| {
            let trigger = if s.cron.starts_with('@') { Some(crate::schedule::describe_trigger(&s.cron)) } else { None };
            let workflow = if s.workflow.is_empty() { None } else { serde_json::from_str::<Value>(&s.workflow).ok().and_then(|w| crate::workflow::Workflow::parse(&w).ok()) };
            let mut v = json!(s);
            v["trigger"] = json!(trigger);
            v["outline"] = json!(workflow.as_ref().map(|w| w.describe()));
            v["uses_model"] = json!(workflow.as_ref().map(|w| w.uses_model()));
            v.as_object_mut().map(|o| o.remove("workflow"));
            v
        })
        .collect();
    Ok(Json(json!(items)))
}

async fn add_schedule(State(st): S, Json(b): Json<Value>) -> ApiResult {
    let s = |k: &str| b.get(k).and_then(|v| v.as_str()).map(str::trim).filter(|v| !v.is_empty());
    let prompt = s("prompt").ok_or_else(|| bad("Describe what Heron should do."))?;
    let when_text = s("when").ok_or_else(|| bad("Say when it should run, e.g. \"tomorrow 9:00\", \"daily 08:00\" or \"when an email arrives from Maria\"."))?;
    let name = s("name").map(|n| n.to_string()).unwrap_or_else(|| crate::util::preview(prompt, 40));
    let quiet = b.get("quiet").and_then(|q| q.as_bool()).unwrap_or(false);
    // What this job may do while nobody is watching. Granted here by the user, never by the agent.
    let allow = grants_for(s("may").unwrap_or("read")).ok_or_else(|| bad("Unknown permission level for this reminder."))?;
    let id = match crate::schedule::parse_trigger(when_text) {
        Some(trigger) => {
            if trigger.starts_with("@email") && st.app.cfg().sources.email.is_none() {
                return Err(bad("Connect your email first (under Getting to know you), then Heron can react to new messages."));
            }
            st.app.store.add_trigger(&name, &trigger, prompt, allow)?
        }
        None => {
            let when = crate::schedule::parse_when(when_text, chrono::Local::now())?;
            st.app.store.add_schedule(&name, &when.cron, prompt, when.next_run, allow)?
        }
    };
    st.app.store.set_schedule_quiet(id, quiet)?;
    Ok(Json(json!({"id": id})))
}

fn grants_for(may: &str) -> Option<&'static str> {
    match may {
        "read" => Some(""),
        "write" => Some("write_file:*\nedit_file:*"),
        "commands" => Some("write_file:*\nedit_file:*\nshell:*"),
        _ => None,
    }
}

/// Change what a job may do while unattended. Only the user can do this, here.
async fn update_schedule(State(st): S, Path(id): Path<i64>, Json(b): Json<Value>) -> ApiResult {
    let may = b.get("may").and_then(|m| m.as_str()).and_then(grants_for).ok_or_else(|| bad("Unknown permission level."))?;
    if !st.app.store.set_schedule_allow(id, may)? {
        return Err(ApiError(StatusCode::NOT_FOUND, "That job no longer exists.".into()));
    }
    Ok(Json(json!({"ok": true})))
}

async fn run_schedule(State(st): S, Path(id): Path<i64>) -> ApiResult {
    if st.app.store.get_schedule(id).is_none() {
        return Err(ApiError(StatusCode::NOT_FOUND, "That job no longer exists.".into()));
    }
    let app = st.app.clone();
    tokio::spawn(async move {
        let _ = app.run_now(id).await;
    });
    Ok(Json(json!({"started": true})))
}

async fn schedule_runs(State(st): S, Path(id): Path<i64>) -> ApiResult {
    let runs: Vec<Value> = st.app.store.list_runs(id, 10)?.into_iter().map(|(ts, status, tokens, record)| json!({"ts": ts, "status": status, "tokens": tokens, "record": serde_json::from_str::<Value>(&record).unwrap_or(Value::Null)})).collect();
    Ok(Json(json!(runs)))
}

async fn delete_schedule(State(st): S, Path(id): Path<i64>) -> ApiResult {
    Ok(Json(json!({"deleted": st.app.store.remove_schedule(id)?})))
}

async fn get_inbox(State(st): S) -> ApiResult {
    Ok(Json(json!(st.app.store.inbox(false, 100)?)))
}

async fn read_inbox(State(st): S) -> ApiResult {
    st.app.store.mark_inbox_read()?;
    Ok(Json(json!({"ok": true})))
}

async fn get_awareness(State(st): S) -> ApiResult {
    let cfg = st.app.cfg();
    let (store, ws) = (st.app.store.clone(), st.app.workspace.clone());
    let seen = tokio::task::spawn_blocking(move || crate::ambient::awareness(&store, &ws)).await?;
    let journal: Vec<Value> = st.app.store.observations_since(0, 60)?.into_iter().filter(|(_, k, _)| k == "file").map(|(ts, _, subject)| json!({"ts": ts, "path": subject})).collect();
    Ok(Json(json!({
        "enabled": cfg.agent.ambient_context, "reflect": cfg.agent.reflect, "proactive": cfg.agent.proactive,
        "budget_used": st.app.background_budget().0, "budget_limit": st.app.background_budget().1, "demo": crate::llm::is_demo(&cfg),
        "focus": seen.focus, "active_hours": seen.active_hours, "branch": seen.branch, "journal": journal,
        "insights": st.app.store.memories_by_source("observed", 20)?, "workspace": st.app.workspace.display().to_string(),
        "proactive_status": st.app.store.kv_get("proactive_last_status").unwrap_or_else(|| "not_checked".to_string()),
        "proactive_status_at": st.app.store.kv_get("proactive_last_status_at").and_then(|v| v.parse::<i64>().ok()).unwrap_or(0),
    })))
}

async fn set_awareness(State(st): S, Json(b): Json<Value>) -> ApiResult {
    let proactive = b.get("proactive").and_then(|v| v.as_str());
    if proactive.is_some_and(|v| !["off", "suggest", "prepare", "takeover"].contains(&v)) {
        return Err(bad("Unknown proactivity level."));
    }
    st.app.update_config(ConfigChange::edit(|cfg| {
        if let Some(v) = b.get("enabled").and_then(|v| v.as_bool()) {
            cfg.agent.ambient_context = v;
        }
        if let Some(v) = b.get("reflect").and_then(|v| v.as_bool()) {
            cfg.agent.reflect = v;
        }
        if let Some(v) = proactive {
            cfg.agent.proactive = v.to_string();
        }
    }))?;
    get_awareness(State(st)).await
}

async fn clear_awareness(State(st): S) -> ApiResult {
    let removed = st.app.store.clear_observations()?;
    for m in st.app.store.memories_by_source("observed", 500)? {
        st.app.store.delete_memory(m.id)?;
    }
    Ok(Json(json!({"removed": removed})))
}

/// The user reviewed a connected app whose tools changed and accepts the new definitions.
async fn accept_mcp(State(st): S, Path(name): Path<String>) -> ApiResult {
    let fingerprint = st.app.mcp_pending.get(&name).ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "Nothing to accept for that app.".into()))?;
    st.app.store.kv_set(&format!("mcp_pin:{name}"), fingerprint)?;
    Ok(Json(json!({"accepted": true, "note": "Restart Heron for the change to take effect."})))
}

// ---------- character, pause, waiting approvals, per-tool settings, today ----------

// ---------- Connector directory + isolated credential vault ----------

async fn list_connectors(State(st): S) -> ApiResult {
    let cfg = st.app.cfg();
    Ok(Json(json!({
        "connectors": crate::connectors::list(&cfg)?,
        "vault": {
            "backend": cfg.secret_storage(),
            "os_keychain_supported": cfg!(any(target_os = "macos", windows)),
            "credentials_readable_by_agent": false
        },
        "custom_execution_enabled": false,
    })))
}

fn local_connector_admin(st: &ServerState, headers: &HeaderMap) -> std::result::Result<(), ApiError> {
    if st.loopback_only && request_is_local(headers) {
        Ok(())
    } else {
        Err(ApiError(StatusCode::FORBIDDEN,
            "Connector and credential changes require HERON's authenticated local desktop server.".into()))
    }
}

async fn add_custom_connector(
    State(st): S, headers: HeaderMap,
    Json(spec): Json<crate::connectors::CustomConnector>
) -> ApiResult {
    local_connector_admin(&st, &headers)?;
    let id = spec.id.clone();
    crate::connectors::register(&st.app.cfg(), spec)?;
    st.app.store.audit("connectors", "connector.register", &id, "user-local", "metadata saved; not activated");
    Ok(Json(json!({"id": id, "status": "saved_not_active"})))
}

async fn delete_custom_connector(State(st): S, Path(id): Path<String>, headers: HeaderMap) -> ApiResult {
    local_connector_admin(&st, &headers)?;
    crate::connectors::remove_custom(&st.app.cfg(), &id)?;
    st.app.store.audit("connectors", "connector.delete", &id, "user-local", "credential removed");
    Ok(Json(json!({"id": id, "deleted": true})))
}

async fn set_connector_api_key(
    State(st): S, Path(id): Path<String>, headers: HeaderMap, Json(body): Json<Value>
) -> ApiResult {
    local_connector_admin(&st, &headers)?;
    let key = body.get("api_key").and_then(Value::as_str)
        .ok_or_else(|| bad("Supply the connector API key."))?;
    // There must be no audit entry containing an input key or HTTP body.
    crate::connectors::put_api_key(&st.app.cfg(), &id, key)?;
    st.app.store.audit("connectors", "credential.store", &id, "user-local", "stored in OS vault");
    Ok(Json(json!({"id": id, "has_credential": true})))
}

async fn delete_connector_api_key(State(st): S, Path(id): Path<String>, headers: HeaderMap) -> ApiResult {
    local_connector_admin(&st, &headers)?;
    crate::connectors::remove_api_key(&st.app.cfg(), &id)?;
    st.app.store.audit("connectors", "credential.delete", &id, "user-local", "removed");
    Ok(Json(json!({"id": id, "has_credential": false})))
}

async fn get_persona(State(st): S) -> ApiResult {
    let cfg = st.app.cfg();
    Ok(Json(json!({"text": crate::persona::load(&cfg), "is_default": crate::persona::is_default(&cfg), "name": cfg.agent.name, "max": crate::persona::MAX_CHARS})))
}

async fn set_persona(State(st): S, Json(b): Json<Value>) -> ApiResult {
    if let Some(text) = b.get("text").and_then(|t| t.as_str()) {
        crate::persona::save(&st.app.cfg(), text)?;
    }
    if let Some(name) = b.get("name").and_then(|n| n.as_str()).map(str::trim).filter(|n| !n.is_empty() && n.chars().count() <= 40) {
        if name != st.app.cfg().agent.name {
            st.app.update_config(ConfigChange::edit(|c| c.agent.name = name.to_string()))?;
        }
    }
    get_persona(State(st)).await
}

async fn reset_persona(State(st): S) -> ApiResult {
    crate::persona::reset(&st.app.cfg())?;
    get_persona(State(st)).await
}

async fn set_pause(State(st): S, Json(b): Json<Value>) -> ApiResult {
    st.app.set_paused(b.get("paused").and_then(|p| p.as_bool()).unwrap_or(true));
    Ok(Json(json!({"paused": st.app.paused()})))
}

/// Save a complete copy of the database beside it. The app keeps running while it is made.
async fn make_backup(State(st): S) -> ApiResult {
    let store = st.app.store.clone();
    let saved = tokio::task::spawn_blocking(move || store.backup(None)).await.map_err(|e| anyhow::anyhow!("the copy could not be made: {e}"))??;
    Ok(Json(json!({"path": saved.display().to_string()})))
}

async fn list_approvals(State(st): S) -> ApiResult {
    Ok(Json(json!(st.app.store.pending_approvals()?)))
}

async fn answer_approval(State(st): S, Path(id): Path<i64>, Json(b): Json<Value>) -> ApiResult {
    match b.get("action").and_then(|a| a.as_str()) {
        Some("approve") => match st.app.run_parked(id).await {
            Ok(result) => Ok(Json(json!({"result": result}))),
            // As for `/api/chat` (Q15): the request stays waiting until the notice is accepted.
            Err(e) if e.downcast_ref::<crate::risk_notice::RiskNoticeNotAccepted>().is_some() => Err(ApiError(StatusCode::PRECONDITION_REQUIRED, "risk_notice_not_accepted".into())),
            Err(e) => Err(e.into()),
        },
        Some("dismiss") => {
            st.app.store.resolve_approval(id, "dismissed", None)?;
            Ok(Json(json!({"dismissed": true})))
        }
        _ => Err(bad("Unknown action.")),
    }
}

async fn list_tools(State(st): S) -> ApiResult {
    let agent = st.app.agent(Arc::new(crate::permissions::DenyAll));
    let ctx = agent.tool_ctx("settings");
    let tools: Vec<Value> = st
        .app
        .tools
        .names()
        .into_iter()
        .filter_map(|name| st.app.tools.get(&name))
        .map(|t| {
            // Tools that never change anything need no setting.
            let risk = t.risk(&json!({}), &ctx);
            let first = t.description().split(". ").next().unwrap_or("").trim_end_matches('.').to_string();
            json!({"name": t.name(), "about": first, "setting": st.app.policy.tool_setting(t.name()), "always_safe": risk == crate::permissions::Effect::LocalRead && !t.name().starts_with("mcp__") && !["read_file", "list_dir", "search_files", "find_files", "shell"].contains(&t.name())})
        })
        .collect();
    Ok(Json(json!(tools)))
}

async fn set_tool(State(st): S, Path(name): Path<String>, Json(b): Json<Value>) -> ApiResult {
    if st.app.tools.get(&name).is_none() {
        return Err(ApiError(StatusCode::NOT_FOUND, "Unknown tool.".into()));
    }
    let setting = b.get("setting").and_then(|s| s.as_str()).unwrap_or("ask");
    st.app.policy.set_tool_setting(&name, setting)?;
    // Keep the choice across restarts: per-tool rules live in the settings file.
    let rule = format!("{name}:*");
    st.app.update_config(ConfigChange::edit(|cfg| {
        cfg.permissions.allow.retain(|r| *r != rule);
        cfg.permissions.deny.retain(|r| *r != rule);
        match setting {
            "allow" => cfg.permissions.allow.push(rule.clone()),
            "never" => cfg.permissions.deny.push(rule.clone()),
            _ => {}
        }
    }))?;
    st.app.persist_rules();
    Ok(Json(json!({"name": name, "setting": st.app.policy.tool_setting(&name)})))
}

/// What deserves the user's attention today, assembled locally without a model.
async fn get_today(State(st): S) -> ApiResult {
    let store = &st.app.store;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let tasks = store.list_tasks(Some("open"))?;
    let due: Vec<&crate::store::Task> = tasks.iter().filter(|t| t.due.as_deref().map(|d| d[..d.len().min(10)] <= *today).unwrap_or(false)).collect();
    let now = crate::util::now_ts();
    let new_mail = store.knowledge_between("email", now - 86_400, now + 3600, 200)?.len();
    Ok(Json(json!({
        "events": crate::sources::upcoming(store, 24, 6),
        "due_tasks": due.iter().take(6).map(|t| json!({"id": t.id, "title": t.title, "due": t.due, "overdue": t.due.as_deref().map(|d| d[..d.len().min(10)] < *today).unwrap_or(false)})).collect::<Vec<_>>(),
        "open_tasks": tasks.iter().filter(|t| t.owner != "heron").count(),
        "heron_tasks": tasks.iter().filter(|t| t.owner == "heron").count(),
        "waiting": store.pending_approvals()?.len(),
        "unconfirmed": store.unconfirmed_memory_count(),
        "new_mail": new_mail,
        "unread": store.inbox(true, 100)?.len(),
        "paused": st.app.paused(),
    })))
}

// ---------- phone (Telegram) ----------

/// What has been used, what is left and where the month is heading.
async fn get_usage(State(st): S) -> ApiResult {
    let cfg = st.app.cfg();
    let report = crate::usage::report(&st.app.store, &cfg, st.app.background_budget().0);
    let mut out = serde_json::to_value(&report).map_err(anyhow::Error::from)?;
    out["session"] = serde_json::to_value(st.app.usage.lock().unwrap().clone()).unwrap_or_default();
    out["demo"] = json!(crate::llm::is_demo(&cfg));
    // Where the provider can say how much credit is left, show that too (OpenRouter can).
    if cfg.llm.base_url.contains("openrouter.ai") {
        if let Some(key) = cfg.api_key() {
            let client = reqwest::Client::builder().timeout(Duration::from_secs(6)).build().map_err(anyhow::Error::from)?;
            if let Ok(resp) = client.get(format!("{}/credits", cfg.llm.base_url.trim_end_matches('/'))).bearer_auth(key).send().await {
                if let Ok(v) = resp.json::<Value>().await {
                    if let (Some(total), Some(used)) = (v["data"]["total_credits"].as_f64(), v["data"]["total_usage"].as_f64()) {
                        out["provider"] = json!({"name": "OpenRouter", "credits_bought": total, "credits_used": used, "credits_left": (total - used).max(0.0)});
                    }
                }
            }
        }
    }
    Ok(Json(out))
}

/// Change the limits and whether simple requests go to the cheaper model.
async fn set_usage(State(st): S, Json(b): Json<Value>) -> ApiResult {
    let tokens = |v: &Value| v.as_u64().or_else(|| v.as_str().and_then(|s| s.replace([',', ' ', '_'], "").parse().ok()));
    st.app.update_config(ConfigChange::edit(|cfg| {
        if let Some(v) = b.get("daily_token_limit").and_then(tokens) {
            cfg.agent.daily_token_limit = v;
        }
        if let Some(v) = b.get("monthly_token_limit").and_then(tokens) {
            cfg.agent.monthly_token_limit = v;
        }
        if let Some(v) = b.get("background_tokens_per_day").and_then(tokens) {
            cfg.agent.background_tokens_per_day = v;
        }
        if let Some(v) = b.get("routing").and_then(|v| v.as_bool()) {
            cfg.agent.routing = if v { "auto".into() } else { "off".into() };
        }
        if let Some(v) = b.get("fast_model").and_then(|v| v.as_str()) {
            cfg.llm.fast_model = v.trim().to_string();
        }
    }))?;
    get_usage(State(st)).await
}

async fn get_telegram(State(st): S) -> ApiResult {
    let configured = st.app.cfg().vault_only_secret_present("telegram_token");
    let paired = crate::telegram::paired_chat(&st.app).is_some();
    Ok(Json(json!({"configured": configured, "paired": paired, "code": if configured && !paired { Some(crate::telegram::show_pairing_code(&st.app)) } else { None }})))
}

async fn set_telegram(State(st): S, Json(b): Json<Value>) -> ApiResult {
    let token = b.get("token").and_then(|t| t.as_str()).map(str::trim).unwrap_or("");
    // Telegram bot tokens look like 123456789:AA...; reject anything else before it is stored or used in a URL.
    let valid = token.split_once(':').map(|(id, key)| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) && key.len() >= 20 && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')).unwrap_or(false);
    if !valid {
        return Err(bad("That does not look like a bot token. Create a bot with @BotFather in Telegram and paste the token it gives you."));
    }
    let cfg = st.app.cfg();
    let old_token = cfg.secret("telegram_token");
    // A wrong or revoked token would otherwise unpair the phone and leave a gateway that never connects.
    if old_token.as_deref() != Some(token) {
        crate::telegram::verify_token(&cfg.telegram.api_base, token).await.map_err(|msg| bad(&msg))?;
    }
    if !cfg!(any(target_os = "macos", windows)) {
        return Err(ApiError(StatusCode::NOT_IMPLEMENTED,
            "Telegram bot credentials require a verified native vault; Linux is not yet supported.".into()));
    }
    // Install first or fail without changing the previous pairing/task.
    // No plaintext-file fallback is permitted for bot credentials.
    cfg.store_vault_only_secret("telegram_token", token)?;
    if old_token.as_deref() != Some(token) {
        crate::telegram::stop(&st.app);
        // A gateway in another Heron process sharing this home stops on its next poll.
        crate::telegram::bump_generation(&st.app);
        if let Some(old) = &old_token {
            crate::telegram::purge_token_spool(&st.app, old);
        }
        crate::telegram::unpair(&st.app);
        let _ = st.app.store.kv_set("telegram_offset", "0");
    }
    // Restarts the connection, so a replaced token is the one in use and there is only ever one reader.
    crate::telegram::spawn_if_configured(&st.app);
    get_telegram(State(st)).await
}

async fn remove_telegram(State(st): S) -> ApiResult {
    crate::telegram::stop(&st.app);
    crate::telegram::bump_generation(&st.app);
    // Revocation must purge the account's channel spool before erasing
    // the bot token that identifies its records.
    crate::telegram::unpair(&st.app);
    let cfg = st.app.cfg();
    cfg.delete_vault_only_secret("telegram_token")?;
    // Clear any legacy fallback that may predate the secure connector vault.
    cfg.store_secret("telegram_token", "")?;
    Ok(Json(json!({"configured": false, "paired": false})))
}


// ---------- optional Google / Microsoft OAuth 2.0 ----------

async fn get_oauth_status(State(st): S) -> ApiResult {
    let cfg = st.app.cfg();
    let providers: Vec<Value> = [crate::oauth::Provider::Google, crate::oauth::Provider::Microsoft]
        .iter().map(|p| json!({
            "provider": p.name(), "connected": p.connected(&cfg),
            "client_configured": cfg.secret(&format!("oauth_{}_client_id", p.name())).is_some() || match p {
                crate::oauth::Provider::Google => option_env!("HERON_GOOGLE_OAUTH_CLIENT_ID").is_some(),
                crate::oauth::Provider::Microsoft => option_env!("HERON_MICROSOFT_OAUTH_CLIENT_ID").is_some(),
            },
            "read_only": true
        })).collect();
    Ok(Json(json!({"providers": providers})))
}

/// A local, authenticated request starts a PKCE authorization. No arbitrary redirect URI or
/// provider-controlled OAuth URL is accepted. The returned URL must be opened by the user.
async fn start_oauth(State(st): S, Path(provider): Path<String>, headers: HeaderMap, Json(body): Json<Value>) -> ApiResult {
    if !st.loopback_only || !request_is_local(&headers) {
        return Err(ApiError(StatusCode::FORBIDDEN, "OAuth desktop connections require a loopback-only server.".into()));
    }
    // OAuth account tokens must not be persisted through the legacy plaintext
    // fallback. Until a verified Linux Secret Service backend exists, do not
    // initiate a consent flow that could never be stored securely.
    if !cfg!(any(target_os = "macos", windows)) {
        return Err(ApiError(StatusCode::NOT_IMPLEMENTED,
            "Google/Microsoft OAuth requires a verified native credential vault (macOS Keychain or Windows Credential Manager).".into()));
    }
    let provider = crate::oauth::Provider::parse(&provider)?;
    let cfg = st.app.cfg();
    // Public desktop client IDs may be bundled by the HERON distributor (no secret).
    // Let a user override them with their own app registration for self-hosted use.
    let bundled = match provider {
        crate::oauth::Provider::Google => option_env!("HERON_GOOGLE_OAUTH_CLIENT_ID"),
        crate::oauth::Provider::Microsoft => option_env!("HERON_MICROSOFT_OAUTH_CLIENT_ID"),
    };
    let client_id = body.get("client_id").and_then(Value::as_str).map(str::trim)
        .filter(|s| !s.is_empty()).map(str::to_string)
        .or_else(|| cfg.secret(&format!("oauth_{}_client_id", provider.name())))
        .or_else(|| bundled.map(str::to_string))
        .ok_or_else(|| bad("This build has no public desktop OAuth client ID. Add your own client ID or ask the HERON distributor to register an app."))?;
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or_default();
    let port = host.rsplit_once(':').and_then(|(_, p)| p.parse::<u16>().ok())
        .ok_or_else(|| bad("Cannot find this local server's OAuth callback port."))?;
    let callback_host = if provider == crate::oauth::Provider::Google { "127.0.0.1" } else { "localhost" };
    let redirect_uri = format!("http://{callback_host}:{port}/oauth/callback/{}", provider.name());
    let (state, pending, url) = crate::oauth::begin(provider, &client_id, &redirect_uri)?;
    // Linearize sign-in admission with Disconnect, which removes all
    // pending states for this provider. Without this transition lock,
    // Disconnect can clear the map and a concurrently starting request
    // can insert a stale authorization afterwards.
    let _transition = st.oauth_transition.lock().await;
    // Reconnecting is allowed: finishing the sign-in admits only the account already connected.
    let mut waiting = st.oauth_pending.lock().unwrap_or_else(|e| e.into_inner());
    waiting.retain(|_, p| !p.expired());
    if waiting.len() >= 8 {
        return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "Too many unfinished sign-ins. Try again later.".into()));
    }
    waiting.insert(state, pending);
    Ok(Json(json!({"provider": provider.name(), "authorization_url": url.as_str(), "expires_in_seconds": 600})))
}

/// The provider sends a GET through the system browser. Its code is never rendered or logged.
/// Each state can be redeemed only once. We do not infer success from a browser navigation alone.
async fn oauth_callback(State(st): S, Path(provider): Path<String>, headers: HeaderMap, Query(query): Query<HashMap<String, String>>) -> Response {
    if !st.loopback_only || !request_is_local(&headers) {
        return (StatusCode::FORBIDDEN, Html("OAuth callback is only available on localhost.")).into_response();
    }
    let Ok(provider) = crate::oauth::Provider::parse(&provider) else {
        return (StatusCode::BAD_REQUEST, Html("Unknown OAuth provider.")).into_response();
    };
    let Some(state) = query.get("state") else {
        return (StatusCode::BAD_REQUEST, Html("Missing OAuth state. Start sign-in in HERON again.")).into_response();
    };
    // Disconnect invalidates pending states and waits for any callback already underway.
    let _transition = st.oauth_transition.lock().await;
    let pending = {
        let mut waiting = st.oauth_pending.lock().unwrap_or_else(|e| e.into_inner());
        waiting.remove(state)
    };
    let Some(pending) = pending else {
        return (StatusCode::BAD_REQUEST, Html("OAuth sign-in expired or was already used. Start again in HERON.")).into_response();
    };
    if pending.provider != provider || pending.expired() {
        return (StatusCode::BAD_REQUEST, Html("OAuth sign-in expired or did not match the provider.")).into_response();
    }
    let Some(code) = query.get("code") else {
        return (StatusCode::BAD_REQUEST, Html("Account permission was declined. You can close this tab.")).into_response();
    };
    match crate::oauth::finish(&st.app.cfg(), pending, code).await {
        Ok(()) => {
            // Import independently of the browser redirect; a temporary Graph/Gmail
            // error must not roll back a valid consent. The user may retry via /sync.
            let app = st.app.clone();
            tokio::spawn(async move { let _ = crate::oauth::sync(&app, provider).await; });
            (StatusCode::OK, Html("<!doctype html><meta charset='utf-8'><title>HERON connected</title><h1>Account connected</h1><p>HERON has started a read-only sync. You can close this tab and return to HERON.</p>")).into_response()
        }
        Err(_) => (StatusCode::BAD_REQUEST, Html("Account sign-in could not be completed. Check your OAuth app registration, then retry in HERON.")).into_response(),
    }
}

async fn sync_oauth(State(st): S, Path(provider): Path<String>) -> ApiResult {
    let provider = crate::oauth::Provider::parse(&provider)?;
    let (emails, events) = crate::oauth::sync(&st.app, provider).await?;
    Ok(Json(json!({"provider": provider.name(), "emails": emails, "events": events})))
}

async fn disconnect_oauth(State(st): S, Path(provider): Path<String>) -> ApiResult {
    let provider = crate::oauth::Provider::parse(&provider)?;
    let _transition = st.oauth_transition.lock().await;
    {
        let mut pending = st.oauth_pending.lock().unwrap_or_else(|e| e.into_inner());
        pending.retain(|_, sign_in| sign_in.provider != provider);
    }
    // Wait for any provider-derived memory already being embedded/saved
    // before deleting that account's index and memories. No new provider
    // learning can commit after the connection has been removed.
    let _learning = crate::oauth::OAUTH_LEARNING_COMMIT.lock().await;
    let (items, memories) = crate::oauth::disconnect(&st.app, provider)?;
    Ok(Json(json!({"provider": provider.name(), "removed_items": items, "removed_memories": memories})))
}

// ---------- onboarding & context sources ----------

async fn get_sources(State(st): S) -> ApiResult {
    let cfg = st.app.cfg();
    let store = &st.app.store;
    let learned = |source: &str| store.memories_by_source(source, 500).map(|m| m.len()).unwrap_or(0);
    let hosts: Vec<String> = crate::sources::calendar_links(&cfg).iter().filter_map(|l| url::Url::parse(&crate::sources::normalize_calendar_url(l)).ok()).filter_map(|u| u.host_str().map(String::from)).collect();
    let google = crate::oauth::Provider::Google.connected(&cfg);
    let microsoft = crate::oauth::Provider::Microsoft.connected(&cfg);
    let oauth_email_count = store.knowledge_count("google_email") + store.knowledge_count("microsoft_email");
    let oauth_calendar_count = store.knowledge_count("google_calendar") + store.knowledge_count("microsoft_calendar");
    Ok(Json(json!({
        "onboarded": store.kv_get("onboarded").is_some(),
        "demo": crate::llm::is_demo(&cfg),
        "has_model": cfg.api_key().is_some() || is_local(&cfg.llm.base_url),
        "model": cfg.llm.model,
        "about": {"saved": learned("onboarding"), "indexed": store.knowledge_count("about"), "learned": learned("about")},
        "computer": {"connected": store.knowledge_count("computer") > 0, "indexed": store.knowledge_count("computer"), "learned": learned("computer")},
        "documents": {"connected": !cfg.sources.folders.is_empty(), "folders": cfg.sources.folders, "indexed": store.knowledge_count("documents"), "learned": learned("documents")},
        "calendar": {"connected": !hosts.is_empty() || google || microsoft, "hosts": hosts, "indexed": store.knowledge_count("calendar") + oauth_calendar_count, "learned": learned("calendar")},
        "email": {"connected": cfg.sources.email.is_some() || google || microsoft, "address": cfg.sources.email.as_ref().map(|e| e.username.clone()), "indexed": store.knowledge_count("email") + oauth_email_count, "learned": learned("email")},
        "oauth": {"google": google, "microsoft": microsoft},
    })))
}

async fn connect_source(State(st): S, Path(name): Path<String>, Json(b): Json<Value>) -> ApiResult {
    let text = |k: &str| b.get(k).and_then(|v| v.as_str()).map(str::trim).filter(|v| !v.is_empty());
    let app = &st.app;
    // Secrets are kept by the Heron folder, not in the settings. Every settings change below is
    // applied to the settings as they are when it is saved, never to a copy taken before an await.
    let cfg = app.cfg();
    let result = match name.as_str() {
        "about" => {
            let about: crate::sources::About = serde_json::from_value(b.clone())?;
            json!({"saved": crate::sources::save_about(app, &about).await?})
        }
        "computer" => json!({"learned": crate::sources::scan_computer(app).await?}),
        "documents" => {
            let raw = text("path").ok_or_else(|| bad("Enter the folder that holds your notes or documents."))?;
            let folder = crate::permissions::resolve_path(&app.workspace, raw);
            if !folder.is_dir() {
                return Err(bad("Heron could not find that folder. Check the path and try again."));
            }
            if matches!(crate::permissions::classify_path(&folder, &app.workspace, &app.cfg(), false), crate::permissions::Effect::Refused | crate::permissions::Effect::SensitiveRead) {
                return Err(bad("That folder holds keys, passwords or Heron's own data, so Heron will not index it."));
            }
            let folder = folder.to_string_lossy().to_string();
            app.update_config(ConfigChange::edit(|c| {
                if !c.sources.folders.contains(&folder) {
                    c.sources.folders.push(folder.clone());
                }
            }))?;
            json!({"indexed": crate::sources::sync_documents(app).await?})
        }
        "calendar" => {
            let link = text("url").ok_or_else(|| bad("Paste your calendar's private ICS link."))?;
            let before = cfg.secret("calendar_links").unwrap_or_default();
            let mut links: Vec<&str> = before.lines().filter(|l| !l.trim().is_empty()).collect();
            if !links.contains(&link) {
                links.push(link);
            }
            cfg.store_secret("calendar_links", &links.join("\n"))?;
            match crate::sources::sync_calendar(app).await {
                Ok(n) => json!({"indexed": n}),
                Err(e) => {
                    cfg.store_secret("calendar_links", &before)?; // do not keep a link that does not work
                    return Err(e.into());
                }
            }
        }
        "email" => {
            let host = text("host").ok_or_else(|| bad("Enter your mail server, for example imap.gmail.com."))?;
            let username = text("username").ok_or_else(|| bad("Enter your email address."))?;
            let password = text("password").ok_or_else(|| bad("Enter the app password for this account."))?;
            let port = b.get("port").and_then(|p| p.as_u64()).unwrap_or(993) as u16;
            let tls = b.get("tls").and_then(|t| t.as_bool()).unwrap_or(true);
            // Prove the sign-in works before saving anything.
            crate::mail::fetch_recent(host, port, tls, username, password, 1).await?;
            cfg.store_secret("email_password", password)?;
            let email = crate::config::EmailConfig { host: host.to_string(), port, username: username.to_string(), tls };
            app.update_config(ConfigChange::edit(|c| c.sources.email = Some(email)))?;
            json!({"indexed": crate::sources::sync_email(app).await?})
        }
        _ => return Err(ApiError(StatusCode::NOT_FOUND, "Unknown source.".into())),
    };
    Ok(Json(result))
}

async fn learn_source(State(st): S, Path(name): Path<String>) -> ApiResult {
    Ok(Json(json!({"learned": crate::sources::learn(&st.app, &name).await?})))
}

/// Disconnect a source and forget everything that came from it.
async fn disconnect_source(State(st): S, Path(name): Path<String>) -> ApiResult {
    if !crate::sources::SOURCES.contains(&name.as_str()) {
        return Err(ApiError(StatusCode::NOT_FOUND, "Unknown source.".into()));
    }
    // A provider-specific source cannot be safely removed through the
    // legacy route: its OAuth credentials and background sync remain live.
    // Only the OAuth disconnect endpoint acquires the transition/learning
    // gates, revokes local credentials and erases both associated sources.
    if matches!(name.as_str(), "google_email" | "google_calendar" |
                             "microsoft_email" | "microsoft_calendar") {
        return Err(bad("Disconnect Google or Microsoft under Settings > Google and Microsoft accounts to remove its data."));
    }
    let cfg = st.app.cfg();
    // OAuth consent covers both mail and calendar. Do not silently claim that
    // clicking a legacy source's Disconnect button revoked an OAuth account.
    if (name == "email" || name == "calendar") &&
        (crate::oauth::Provider::Google.connected(&cfg) || crate::oauth::Provider::Microsoft.connected(&cfg)) {
        return Err(bad("An OAuth account is still connected. Disconnect Google or Microsoft under Settings > Google and Microsoft accounts first."));
    }
    match name.as_str() {
        "email" => cfg.store_secret("email_password", "")?,
        "calendar" => cfg.store_secret("calendar_links", "")?,
        _ => {}
    }
    st.app.update_config(ConfigChange::edit(|c| match name.as_str() {
        "email" => c.sources.email = None,
        "calendar" => c.sources.calendars.clear(),
        "documents" => c.sources.folders.clear(),
        _ => {}
    }))?;
    let items = st.app.store.clear_knowledge(&name)?;
    let memories = st.app.store.delete_memories_by_source(&name)?;
    Ok(Json(json!({"removed_items": items, "removed_memories": memories})))
}

async fn set_onboarded(State(st): S) -> ApiResult {
    st.app.store.kv_set("onboarded", "1")?;
    Ok(Json(json!({"ok": true})))
}

async fn list_suggestions(State(st): S) -> ApiResult {
    Ok(Json(json!(st.app.store.list_suggestions(true, 20)?)))
}

/// accept: the user wants it done (the app starts a conversation with the prompt, under normal approvals).
/// dismiss: not wanted — remembered so similar offers are not repeated.
async fn answer_suggestion(State(st): S, Path(id): Path<i64>, Json(b): Json<Value>) -> ApiResult {
    let status = match b.get("action").and_then(|a| a.as_str()) {
        Some("accept") => "accepted",
        Some("dismiss") => "dismissed",
        _ => return Err(bad("Unknown action.")),
    };
    let s = st.app.store.get_suggestion(id).ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "That suggestion no longer exists.".into()))?;
    st.app.store.update_suggestion(id, status, None, None)?;
    Ok(Json(json!({"prompt": s.prompt, "session_id": s.session_id})))
}

async fn get_audit(State(st): S) -> ApiResult {
    let (checked, altered) = st.app.store.verify_audit()?;
    Ok(Json(json!({"entries": st.app.store.audit_log(200)?, "checked": checked, "intact": altered.is_none()})))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::llm::{ChatRequest, ChatResponse, DeltaFn, Llm, Message, ScriptedLlm};
    use crate::store::Store;
    use std::collections::VecDeque;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream};

    /// Connections to the router over in-memory pipes. Every request goes through `router` and
    /// `axum::serve` as it does over TCP, without a port, so these tests also run where binding one
    /// is not allowed.
    struct Pipes(tokio::sync::mpsc::UnboundedReceiver<DuplexStream>);

    impl axum::serve::Listener for Pipes {
        type Io = DuplexStream;
        type Addr = std::net::SocketAddr;

        async fn accept(&mut self) -> (Self::Io, Self::Addr) {
            match self.0.recv().await {
                Some(io) => (io, ([127, 0, 0, 1], 0).into()),
                None => std::future::pending().await,
            }
        }

        fn local_addr(&self) -> std::io::Result<Self::Addr> {
            Ok(([127, 0, 0, 1], 0).into())
        }
    }

    struct Harness {
        dir: tempfile::TempDir,
        app: Arc<App>,
        connect: tokio::sync::mpsc::UnboundedSender<DuplexStream>,
    }

    /// The server over `router`, with the given model. `accept` accepts the risk notice first.
    async fn harness(llm: Arc<dyn Llm>, accept: bool, tweak: impl FnOnce(&mut Config)) -> Harness {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config { home: dir.path().join("home"), ..Default::default() };
        cfg.agent.auto_memory = false;
        tweak(&mut cfg);
        cfg.ensure_home().unwrap();
        let app = Arc::new(App::assemble(cfg, dir.path().join("ws"), Store::in_memory().unwrap(), llm).await.unwrap());
        if accept {
            crate::risk_notice::accept_current(&app.store).unwrap();
        }
        serve_app(dir, app)
    }

    /// The server over `router` for an app that is already built.
    fn serve_app(dir: tempfile::TempDir, app: Arc<App>) -> Harness {
        let (connect, incoming) = tokio::sync::mpsc::unbounded_channel();
        let served = router(app.clone(), "test-token".into());
        tokio::spawn(async move { axum::serve(Pipes(incoming), served).await });
        Harness { dir, app, connect }
    }

    fn scripted(answers: Vec<Message>) -> Arc<dyn Llm> {
        Arc::new(ScriptedLlm::new(answers))
    }

    fn write(path: &str, content: &str) -> Message {
        ScriptedLlm::tool_call("write_file", json!({"path": path, "content": content}))
    }

    /// A response: its status, its headers (names in lower case), and the body still to be read.
    struct Reply {
        status: u16,
        headers: Vec<(String, String)>,
        body: BufReader<DuplexStream>,
    }

    impl Reply {
        async fn json(mut self) -> Value {
            let mut text = String::new();
            tokio::time::timeout(Duration::from_secs(20), self.body.read_to_string(&mut text)).await.expect("the body stalled").unwrap();
            if text.trim().is_empty() { Value::Null } else { serde_json::from_str(&text).unwrap_or_else(|_| panic!("not JSON: {text}")) }
        }

        fn events(self) -> SseReader {
            assert_eq!(self.status, 200);
            SseReader { body: self.body, pending: VecDeque::new() }
        }
    }

    impl Harness {
        /// One HTTP/1.0 request to `/api{path}` with the token; the server closes the connection when
        /// the response ends, so the body runs to EOF.
        async fn send(&self, method: &str, path: &str, body: Value) -> Reply {
            let (mut client, server) = tokio::io::duplex(1 << 16);
            self.connect.send(server).unwrap();
            let body = if body.is_null() { String::new() } else { body.to_string() };
            let head = format!("{method} /api{path} HTTP/1.0\r\nHost: 127.0.0.1\r\nx-heron-token: test-token\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n", body.len());
            client.write_all(format!("{head}{body}").as_bytes()).await.unwrap();
            let mut reader = BufReader::new(client);
            let mut status_line = String::new();
            tokio::time::timeout(Duration::from_secs(20), reader.read_line(&mut status_line)).await.expect("no response").unwrap();
            let status = status_line.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or_else(|| panic!("bad status line {status_line:?}"));
            let mut headers = vec![];
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).await.unwrap();
                if line.trim().is_empty() {
                    break;
                }
                if let Some((name, value)) = line.split_once(':') {
                    headers.push((name.trim().to_lowercase(), value.trim().to_string()));
                }
            }
            Reply { status, headers, body: reader }
        }

        /// A request that must succeed, and its JSON body.
        async fn json(&self, method: &str, path: &str, body: Value) -> Value {
            let reply = self.send(method, path, body).await;
            let status = reply.status;
            let v = reply.json().await;
            assert!((200..300).contains(&status), "{method} {path}: {status} {v}");
            v
        }

        async fn chat(&self, body: Value) -> SseReader {
            self.send("POST", "/chat", body).await.events()
        }
    }

    /// Reads the `data:` lines of an SSE response as JSON.
    struct SseReader {
        body: BufReader<DuplexStream>,
        pending: VecDeque<Value>,
    }

    impl SseReader {
        /// The next event, or `None` when the stream has ended.
        async fn next(&mut self) -> Option<Value> {
            loop {
                if let Some(v) = self.pending.pop_front() {
                    return Some(v);
                }
                let mut line = String::new();
                let n = tokio::time::timeout(Duration::from_secs(20), self.body.read_line(&mut line)).await.expect("the stream stalled").ok()?;
                if n == 0 {
                    return None;
                }
                if let Some(data) = line.trim_end().strip_prefix("data:") {
                    self.pending.push_back(serde_json::from_str(data.trim()).unwrap());
                }
            }
        }

        /// Events up to and including the first of type `kind`.
        async fn until(&mut self, kind: &str) -> Vec<Value> {
            let mut out = vec![];
            while let Some(v) = self.next().await {
                let done = v["type"] == kind;
                out.push(v);
                if done {
                    return out;
                }
            }
            panic!("the stream ended before a {kind} event: {out:?}");
        }

        /// Every event until the stream ends.
        async fn all(&mut self) -> Vec<Value> {
            let mut out = vec![];
            while let Some(v) = self.next().await {
                out.push(v);
            }
            out
        }
    }

    fn types(events: &[Value]) -> Vec<&str> {
        events.iter().map(|e| e["type"].as_str().unwrap_or("")).collect()
    }

    fn find<'a>(events: &'a [Value], kind: &str) -> &'a Value {
        events.iter().find(|e| e["type"] == kind).unwrap_or_else(|| panic!("no {kind} in {events:?}"))
    }

    #[tokio::test]
    async fn chat_returns_428_before_the_notice_is_accepted() {
        let h = harness(scripted(vec![Message::assistant("Hello.")]), false, |_| {}).await;
        if crate::risk_notice::required() {
            let refused = h.send("POST", "/chat", json!({"message": "hi"})).await;
            assert_eq!(refused.status, 428);
            assert_eq!(refused.json().await, json!({"error": "risk_notice_not_accepted"}));
            assert!(h.app.store.list_sessions(10).unwrap().is_empty(), "no session row was created");
            assert_eq!(h.json("GET", "/state", Value::Null).await["risk_notice_accepted"], false);
            // The notice is accepted through the API, and only in its current version.
            let notice = h.json("GET", "/risk-notice", Value::Null).await;
            let version = notice["version"].as_u64().unwrap();
            assert_eq!(notice["accepted_version"], Value::Null);
            assert_eq!(notice["text"], crate::risk_notice::TEXT);
            assert_eq!(h.send("POST", "/risk-notice", json!({"version": version + 1})).await.status, 409);
            assert_eq!(h.send("POST", "/chat", json!({"message": "hi"})).await.status, 428);
            assert_eq!(h.send("POST", "/risk-notice", json!({"version": version})).await.status, 204);
            assert_eq!(h.json("GET", "/risk-notice", Value::Null).await["accepted_version"], version);
            assert_eq!(h.json("GET", "/state", Value::Null).await["risk_notice_accepted"], true);
        }
        // Accepted (or not needed on this platform): the chat starts.
        let events = h.chat(json!({"message": "hi"})).await.all().await;
        assert_eq!(events[0]["type"], "session");
        assert_eq!(find(&events, "done")["text"], "Hello.");
        assert_eq!(h.app.store.list_sessions(10).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn busy_session_returns_409_with_its_code() {
        // The first reply waits on an approval, so the conversation stays busy.
        let h = harness(scripted(vec![write("a.txt", "a"), Message::assistant("Done.")]), true, |_| {}).await;
        let mut first = h.chat(json!({"message": "write a"})).await;
        let sid = first.next().await.unwrap()["id"].as_str().unwrap().to_string();
        first.until("approval").await;
        let busy = h.send("POST", "/chat", json!({"session_id": sid, "message": "again"})).await;
        assert_eq!(busy.status, 409);
        assert_eq!(busy.json().await, json!({"error": "session_busy", "message": "Heron is still replying in this conversation. Stop it first, or wait."}));
        assert_eq!(h.json("GET", &format!("/sessions/{sid}"), Value::Null).await["busy"], true);
        // Once it is stopped, the conversation takes a new message.
        assert_eq!(h.json("POST", "/cancel", json!({"session_id": sid})).await["cancelled"], true);
        let mut again = h.chat(json!({"session_id": sid, "message": "again"})).await;
        assert_eq!(again.next().await.unwrap()["id"], sid.as_str());
    }

    /// A CLI whose installed version Heron has not been tested with.
    struct UntestedCli;

    #[async_trait]
    impl Llm for UntestedCli {
        async fn chat(&self, _req: ChatRequest, _on_delta: DeltaFn<'_>) -> Result<ChatResponse> {
            Err(crate::cli_llm::CliVersionUntested { id: "codex-cli".into(), found: "9.9.9".into() }.into())
        }
    }

    #[tokio::test]
    async fn untested_cli_version_arrives_as_an_sse_error_code() {
        let h = harness(Arc::new(UntestedCli), true, |_| {}).await;
        // The response has started, so the error cannot be an HTTP status: it is an event with a code.
        let events = h.chat(json!({"message": "hi"})).await.all().await;
        assert_eq!(types(&events), vec!["session", "error", "run_end"], "{events:?}");
        let error = find(&events, "error");
        assert_eq!((error["code"].as_str(), error["provider"].as_str(), error["version"].as_str()), (Some("cli_version_untested"), Some("codex-cli"), Some("9.9.9")));
        assert!(error["message"].as_str().unwrap().contains("9.9.9"), "{error}");
        let run_id = &events[0]["run_id"];
        assert!(events.iter().all(|e| e["run_id"] == *run_id));
    }

    #[tokio::test]
    async fn run_survives_dropped_chat_stream() {
        let h = harness(scripted(vec![write("kept.txt", "still here"), Message::assistant("Wrote it.")]), true, |_| {}).await;
        let mut chat = h.chat(json!({"message": "write kept.txt"})).await;
        let session = chat.next().await.unwrap();
        let (sid, run_id) = (session["id"].as_str().unwrap().to_string(), session["run_id"].as_str().unwrap().to_string());
        let approval = chat.until("approval").await.pop().unwrap();
        // The window closes in the middle of the turn.
        drop(chat);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(h.json("GET", &format!("/sessions/{sid}"), Value::Null).await["busy"], true, "the run is still going");
        // Another client answers the approval, and picks the run up again from the bus.
        assert_eq!(h.json("POST", "/approve", json!({"id": approval["id"], "decision": "once"})).await["ok"], true);
        let mut events = h.send("GET", &format!("/events?since={}", approval["seq"]), Value::Null).await.events();
        let rest = events.until("run_end").await;
        assert!(rest.iter().all(|e| e["run_id"] == run_id.as_str()), "{rest:?}");
        assert_eq!(find(&rest, "approval_resolved")["decision"], "once");
        assert_eq!(find(&rest, "done")["text"], "Wrote it.");
        assert_eq!(std::fs::read_to_string(h.app.workspace.join("kept.txt")).unwrap(), "still here");
        assert_eq!(h.json("GET", &format!("/sessions/{sid}"), Value::Null).await["busy"], false);
    }

    #[tokio::test]
    async fn cancel_reports_whether_a_run_was_stopped() {
        let h = harness(scripted(vec![write("never.txt", "x"), Message::assistant("Wrote it.")]), true, |_| {}).await;
        assert_eq!(h.json("POST", "/cancel", json!({"session_id": "nothing-running"})).await, json!({"ok": true, "cancelled": false}));
        let mut chat = h.chat(json!({"message": "write never.txt"})).await;
        let sid = chat.next().await.unwrap()["id"].as_str().unwrap().to_string();
        let approval = chat.until("approval").await.pop().unwrap();
        assert_eq!(h.json("POST", "/cancel", json!({"session_id": sid})).await, json!({"ok": true, "cancelled": true}));
        // The stream reports the stop, then ends.
        let rest = chat.all().await;
        assert_eq!(types(&rest), vec!["approval_resolved", "cancelled", "run_end"], "{rest:?}");
        assert_eq!((&rest[0]["id"], rest[0]["decision"].as_str()), (&approval["id"], Some("cancelled")));
        assert!(!h.app.workspace.join("never.txt").exists());
        assert_eq!(h.send("POST", "/approve", json!({"id": approval["id"], "decision": "once"})).await.status, 404);
        assert_eq!(h.json("GET", &format!("/sessions/{sid}"), Value::Null).await["busy"], false);
        assert_eq!(h.json("POST", "/cancel", json!({"session_id": sid})).await["cancelled"], false);
    }

    #[tokio::test]
    async fn approval_waits_on_the_bus_then_parks_on_timeout() {
        let h = harness(
            scripted(vec![write("answered.txt", "yes"), Message::assistant("First done."), write("parked.txt", "later"), Message::assistant("Carried on.")]),
            true,
            |c| c.server.approval_timeout_secs = 2,
        )
        .await;
        // Answered in time: the answer goes on the bus and the action is done.
        let mut chat = h.chat(json!({"message": "first"})).await;
        let sid = chat.next().await.unwrap()["id"].as_str().unwrap().to_string();
        let approval = chat.until("approval").await.pop().unwrap();
        assert_eq!(approval["tool"], "write_file");
        h.json("POST", "/approve", json!({"id": approval["id"], "decision": "once"})).await;
        let rest = chat.all().await;
        let resolved = find(&rest, "approval_resolved");
        assert_eq!((&resolved["id"], resolved["decision"].as_str()), (&approval["id"], Some("once")));
        assert_eq!(find(&rest, "done")["text"], "First done.");
        assert!(h.app.workspace.join("answered.txt").exists());

        // Nobody answers: after the timeout it is parked, and the turn carries on without it.
        let started = std::time::Instant::now();
        let mut chat = h.chat(json!({"session_id": sid, "message": "second"})).await;
        let live = chat.until("approval").await.pop().unwrap();
        let rest = chat.all().await;
        assert!(started.elapsed() >= Duration::from_secs(2), "it waited for the answer first");
        assert!(!types(&rest).contains(&"approval_resolved"), "{rest:?}");
        let parked = find(&rest, "approval_parked");
        assert_eq!(parked["id"], live["id"]);
        let parked_id = parked["parked_id"].as_i64().unwrap();
        assert_eq!(find(&rest, "done")["text"], "Carried on.");
        assert_eq!(types(&rest).last(), Some(&"run_end"));
        assert!(!h.app.workspace.join("parked.txt").exists());
        let waiting = h.app.store.pending_approvals().unwrap();
        assert_eq!(waiting.len(), 1);
        assert_eq!((waiting[0].id, waiting[0].tool.as_str(), waiting[0].origin.as_str()), (parked_id, "write_file", "user"));
        // The live id is never reused; the parked request is answered on its own route.
        assert_eq!(h.send("POST", "/approve", json!({"id": live["id"], "decision": "once"})).await.status, 404);
        h.json("POST", &format!("/approvals/{parked_id}"), json!({"action": "approve"})).await;
        assert_eq!(std::fs::read_to_string(h.app.workspace.join("parked.txt")).unwrap(), "later");
    }

    #[tokio::test]
    async fn mode_change_is_written_to_config() {
        let h = harness(scripted(vec![]), true, |_| {}).await;
        let saved = || toml::from_str::<Config>(&std::fs::read_to_string(h.dir.path().join("home/config.toml")).unwrap()).unwrap();
        assert_eq!(h.json("POST", "/mode", json!({"mode": "auto"})).await["mode"], "auto");
        assert_eq!(saved().permissions.mode, "auto");
        assert_eq!(h.app.policy.mode(), Mode::Auto);
        assert_eq!(h.app.cfg().permissions.mode, "auto");
        // A mode that does not exist changes nothing.
        assert_eq!(h.send("POST", "/mode", json!({"mode": "bogus"})).await.status, 400);
        assert_eq!(saved().permissions.mode, "auto");
        h.json("POST", "/mode", json!({"mode": "readonly"})).await;
        assert_eq!(saved().permissions.mode, "readonly");
        assert_eq!(h.app.policy.mode(), Mode::ReadOnly);
    }

    #[tokio::test]
    async fn grants_can_be_listed_and_revoked() {
        let h = harness(scripted(vec![]), true, |c| c.permissions.allow = vec!["task_add:*".into()]).await;
        let notes = h.dir.path().join("Notes");
        std::fs::create_dir_all(&notes).unwrap();
        h.app.policy.add_allow("shell:git *").unwrap();
        h.app.policy.add_allow(&format!("read:{}/**", notes.display())).unwrap();
        h.app.persist_rules();
        let read_rule = h.app.policy.allow_rules().into_iter().find(|r| r.starts_with("read:")).unwrap();
        let grants = h.json("GET", "/grants", Value::Null).await;
        let listed: Vec<(String, String)> = grants.as_array().unwrap().iter().map(|g| (g["rule"].as_str().unwrap().to_string(), g["kind"].as_str().unwrap().to_string())).collect();
        for want in [("task_add:*", "tool"), ("shell:git *", "tool"), (read_rule.as_str(), "read")] {
            assert!(listed.contains(&(want.0.to_string(), want.1.to_string())), "{want:?} in {listed:?}");
        }
        assert_eq!(listed.len(), 3);
        let saved_rules = || h.app.store.kv_get("allow_rules").unwrap_or_default();
        assert!(saved_rules().contains("shell:git *"));

        // A rule from an "always" answer goes from the policy and the saved answers.
        h.json("DELETE", "/grants", json!({"rule": "shell:git *"})).await;
        assert!(!h.app.policy.allow_rules().contains(&"shell:git *".to_string()));
        assert!(!saved_rules().contains("shell:git *"));
        h.json("DELETE", "/grants", json!({"rule": read_rule})).await;
        assert!(!saved_rules().contains(&read_rule));
        // A rule from the settings file goes from the file too.
        h.json("DELETE", "/grants", json!({"rule": "task_add:*"})).await;
        let saved = toml::from_str::<Config>(&std::fs::read_to_string(h.dir.path().join("home/config.toml")).unwrap()).unwrap();
        assert!(saved.permissions.allow.is_empty(), "{:?}", saved.permissions.allow);
        assert!(h.json("GET", "/grants", Value::Null).await.as_array().unwrap().is_empty());
        assert_eq!(h.send("DELETE", "/grants", json!({"rule": "shell:git *"})).await.status, 404);
        // Nothing comes back after a restart.
        let restarted = App::assemble(saved, h.dir.path().join("ws"), h.app.store.clone(), scripted(vec![])).await.unwrap();
        assert!(restarted.policy.allow_rules().is_empty(), "{:?}", restarted.policy.allow_rules());
    }

    /// A model that notes the standing rules of a policy each time it is called, then says "Hello.".
    struct NotesRules(Arc<std::sync::OnceLock<Arc<crate::permissions::Policy>>>, Arc<Mutex<Vec<Vec<String>>>>);

    #[async_trait]
    impl Llm for NotesRules {
        async fn chat(&self, _req: ChatRequest, _on_delta: DeltaFn<'_>) -> Result<ChatResponse> {
            self.1.lock().unwrap().push(self.0.get().map(|p| p.allow_rules()).unwrap_or_default());
            Ok(ChatResponse { message: Message::assistant("Hello."), usage: Default::default(), model: "notes".into() })
        }
    }

    #[tokio::test]
    async fn revoked_grant_stays_revoked_with_a_second_core() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config { home: dir.path().join("home"), ..Default::default() };
        cfg.agent.auto_memory = false;
        cfg.ensure_home().unwrap();
        let seed = Store::open(&cfg.db_path()).unwrap();
        seed.kv_set("allow_rules", &json!(["shell:git *", "shell:make *"]).to_string()).unwrap();
        crate::risk_notice::accept_current(&seed).unwrap();
        // Two cores on one home, each with its own connection (§3, "Another core already running").
        let core = |llm: Arc<dyn Llm>| {
            let (cfg, ws) = (cfg.clone(), dir.path().join("ws"));
            async move { Arc::new(App::assemble(cfg.clone(), ws, Store::open(&cfg.db_path()).unwrap(), llm).await.unwrap()) }
        };
        let (policy_of_b, seen_by_model) = (Arc::new(std::sync::OnceLock::new()), Arc::new(Mutex::new(vec![])));
        let a = core(scripted(vec![])).await;
        let b = core(Arc::new(NotesRules(policy_of_b.clone(), seen_by_model.clone()))).await;
        let _ = policy_of_b.set(b.policy.clone());
        for app in [&a, &b] {
            assert_eq!(app.policy.allow_rules(), vec!["shell:git *", "shell:make *"]);
        }
        let (ha, hb) = (serve_app(tempfile::tempdir().unwrap(), a.clone()), serve_app(tempfile::tempdir().unwrap(), b.clone()));
        let saved = || serde_json::from_str::<Vec<String>>(&seed.kv_get("allow_rules").unwrap_or_default()).unwrap_or_default();
        let has = |app: &App, rule: &str| app.policy.allow_rules().iter().any(|r| r == rule);
        // The other core got an "always" answer of its own that it has not saved yet.
        b.policy.add_allow("shell:ls *").unwrap();

        // A rule is revoked in one core. In the other, the next turn no longer has it by the time
        // anything is decided, and does not write it back; its own new rule is kept.
        ha.json("DELETE", "/grants", json!({"rule": "shell:git *"})).await;
        assert_eq!(saved(), vec!["shell:make *"]);
        let events = hb.chat(json!({"message": "hi"})).await.all().await;
        assert_eq!(find(&events, "done")["text"], "Hello.");
        let at_model = seen_by_model.lock().unwrap().clone();
        assert_eq!(at_model.len(), 1);
        assert!(!at_model[0].iter().any(|r| r == "shell:git *"), "the turn ran with the revoked rule: {at_model:?}");
        assert!(!has(&b, "shell:git *"));
        assert_eq!(saved(), vec!["shell:ls *", "shell:make *"]);

        // Another is revoked while a turn in the other core is going. When that turn ends and saves,
        // only what that core changed itself is written.
        ha.json("DELETE", "/grants", json!({"rule": "shell:make *"})).await;
        assert!(has(&b, "shell:make *"), "the other core has not looked yet");
        b.persist_rules();
        assert_eq!(saved(), vec!["shell:ls *"]);
        assert!(!has(&b, "shell:make *"), "{:?}", b.policy.allow_rules());

        // Neither comes back in a core started afterwards, nor in the first core.
        let fresh = core(scripted(vec![])).await;
        assert_eq!(fresh.policy.allow_rules(), vec!["shell:ls *"]);
        a.reload_rules();
        a.persist_rules();
        assert_eq!(a.policy.allow_rules(), vec!["shell:ls *"]);
        assert_eq!(saved(), vec!["shell:ls *"]);
    }

    /// Two cores on one home, each with its own connection, started from `config.toml` as saved.
    async fn two_cores_from_saved_settings(home: &std::path::Path, ws: &std::path::Path) -> (Arc<App>, Arc<App>) {
        let core = || async {
            let cfg = Config::read_saved(home).unwrap();
            let store = Store::open(&cfg.db_path()).unwrap();
            crate::risk_notice::accept_current(&store).unwrap();
            Arc::new(App::assemble(cfg, ws.to_path_buf(), store, scripted(vec![])).await.unwrap())
        };
        (core().await, core().await)
    }

    #[tokio::test]
    async fn revoked_config_rule_stays_revoked_across_cores() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let mut saved = Config { home: home.clone(), ..Default::default() };
        saved.agent.auto_memory = false;
        saved.permissions.allow = vec!["shell:git *".into(), "shell:make *".into()];
        saved.save().unwrap();
        let ws = dir.path().join("ws");
        let (a, b) = two_cores_from_saved_settings(&home, &ws).await;
        let (ha, hb) = (serve_app(tempfile::tempdir().unwrap(), a.clone()), serve_app(tempfile::tempdir().unwrap(), b.clone()));
        let has = |app: &App, rule: &str| app.policy.allow_rules().iter().any(|r| r == rule);
        let on_disk = || Config::read_saved(&home).unwrap().permissions.allow;
        for app in [&a, &b] {
            assert!(has(app, "shell:git *") && has(app, "shell:make *"), "{:?}", app.policy.allow_rules());
        }

        // A revokes one. B takes that in at its next reload: its policy, its settings, its grants
        // list and what its tool calls read no longer have it.
        ha.json("DELETE", "/grants", json!({"rule": "shell:git *"})).await;
        assert_eq!(on_disk(), vec!["shell:make *"]);
        assert!(has(&b, "shell:git *"), "B has not looked yet");
        b.reload_rules();
        assert!(!has(&b, "shell:git *"), "{:?}", b.policy.allow_rules());
        assert_eq!(b.cfg().permissions.allow, vec!["shell:make *"]);
        let listed = hb.json("GET", "/grants", json!(null)).await;
        assert_eq!(listed, json!([{"kind": "tool", "rule": "shell:make *"}]));
        let standing = crate::sandbox::standing_rules(&b.cfg(), &b.store);
        assert_eq!(standing, vec!["shell:make *"]);
        let verdict = crate::permissions::decide_for(&b.policy, "shell", "git push", crate::permissions::Effect::ProcessExec, false);
        assert_eq!(verdict, crate::permissions::Verdict::Ask, "B no longer runs it unasked");

        // B revokes the other. Only B's own removal is written, not its earlier copy of the list.
        hb.json("DELETE", "/grants", json!({"rule": "shell:make *"})).await;
        assert!(on_disk().is_empty(), "{:?}", on_disk());
        a.reload_rules();
        assert!(!has(&a, "shell:make *"), "{:?}", a.policy.allow_rules());

        // A rule one core adds to the file reaches the other's policy and settings at its reload.
        let mut more = (*a.cfg()).clone();
        more.permissions.allow.push("shell:ls *".into());
        a.update_config(more).unwrap();
        b.reload_rules();
        assert!(has(&b, "shell:ls *"), "{:?}", b.policy.allow_rules());
        assert_eq!(b.cfg().permissions.allow, vec!["shell:ls *"]);
        // Taken out of the file while a saved "always" answer still holds it, it stays granted.
        a.store.kv_set("allow_rules", &json!(["shell:ls *"]).to_string()).unwrap();
        b.reload_rules();
        let mut less = (*a.cfg()).clone();
        less.permissions.allow.clear();
        a.update_config(less).unwrap();
        b.reload_rules();
        assert!(b.cfg().permissions.allow.is_empty());
        assert!(has(&b, "shell:ls *"), "the saved answer still grants it: {:?}", b.policy.allow_rules());

        // A core started afterwards allows neither.
        let (fresh, _) = two_cores_from_saved_settings(&home, &ws).await;
        assert!(!has(&fresh, "shell:git *") && !has(&fresh, "shell:make *"), "{:?}", fresh.policy.allow_rules());
        assert!(fresh.cfg().permissions.allow.is_empty());
    }

    #[tokio::test]
    async fn explicit_mode_choice_is_saved_under_a_cli_override() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let mut saved = Config { home: home.clone(), ..Default::default() };
        saved.agent.auto_memory = false;
        saved.permissions.mode = "ask".into();
        saved.llm.model = "saved-model".into();
        saved.save().unwrap();
        // What `heron --mode yolo --model cli-model` starts with.
        let cfg = Config::read_saved(&home).unwrap().with_cli_overrides(Some("yolo"), Some("cli-model")).unwrap();
        let app = Arc::new(App::assemble(cfg, dir.path().join("ws"), Store::in_memory().unwrap(), scripted(vec![])).await.unwrap());
        let h = serve_app(dir, app.clone());
        let on_disk = || Config::read_saved(&home).unwrap();

        // An unrelated change still leaves the overrides out of the file.
        h.json("POST", "/settings", json!({"name": "Wren"})).await;
        assert_eq!((on_disk().permissions.mode.as_str(), on_disk().llm.model.as_str()), ("ask", "saved-model"));
        // Choosing the mode the override already set is written: it is the user's choice now.
        assert_eq!(h.json("POST", "/mode", json!({"mode": "yolo"})).await["mode"], "yolo");
        let disk = on_disk();
        assert_eq!((disk.permissions.mode.as_str(), disk.llm.model.as_str(), disk.agent.name.as_str()), ("yolo", "saved-model", "Wren"));
        assert_eq!(app.policy.mode(), Mode::Yolo);
    }

    #[tokio::test]
    async fn revoke_with_unparseable_config_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let mut saved = Config { home: home.clone(), ..Default::default() };
        saved.agent.auto_memory = false;
        saved.permissions.allow = vec!["shell:git *".into()];
        saved.save().unwrap();
        let cfg = Config::read_saved(&home).unwrap();
        let store = Store::open(&cfg.db_path()).unwrap();
        store.kv_set("allow_rules", &json!(["shell:make *"]).to_string()).unwrap();
        let app = Arc::new(App::assemble(cfg, dir.path().join("ws"), store, scripted(vec![])).await.unwrap());
        let h = serve_app(dir, app.clone());
        let config = home.join("config.toml");
        let broken = "[permissions\nallow = [\"shell:git *\"\n";
        std::fs::write(&config, broken).unwrap();
        let state = || (app.policy.allow_rules(), app.cfg().permissions.allow.clone(), app.store.kv_get("allow_rules"), std::fs::read_to_string(&config).unwrap());
        let before = state();
        assert_eq!(before.0, vec!["shell:git *", "shell:make *"]);

        for rule in ["shell:git *", "shell:make *"] {
            let refused = h.send("DELETE", "/grants", json!({"rule": rule})).await;
            assert!(!(200..300).contains(&refused.status), "{rule}: {}", refused.status);
            let message = refused.json().await["error"].as_str().unwrap_or_default().to_string();
            assert!(message.contains("config.toml"), "{rule}: the error says which file: {message}");
            assert_eq!(state(), before, "{rule}: nothing changed");
            assert!(app.revoke_grant(rule).is_err(), "{rule}");
            assert_eq!(state(), before, "{rule}: nothing changed");
        }
        // Once the file is repaired, the same revoke goes through.
        saved.save().unwrap();
        assert!(app.revoke_grant("shell:git *").unwrap());
        assert!(!app.policy.allow_rules().iter().any(|r| r == "shell:git *"));
        assert!(Config::read_saved(&home).unwrap().permissions.allow.is_empty());
    }

    /// A mail server on this computer with an empty inbox. Its first sign-in waits: `parked` fires
    /// when that sign-in arrives, and it is answered once `release` is sent.
    async fn mail_server_that_parks_a_sign_in() -> (u16, oneshot::Receiver<()>, oneshot::Sender<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (parked_tx, parked) = oneshot::channel();
        let (release, released) = oneshot::channel::<()>();
        tokio::spawn(async move {
            let mut gate = Some((parked_tx, released));
            while let Ok((sock, _)) = listener.accept().await {
                let mut s = BufReader::new(sock);
                if s.get_mut().write_all(b"* OK IMAP4rev1 ready\r\n").await.is_err() {
                    continue;
                }
                loop {
                    let mut line = String::new();
                    if s.read_line(&mut line).await.unwrap_or(0) == 0 {
                        break;
                    }
                    let Some((tag, cmd)) = line.trim_end().split_once(' ') else { break };
                    let (tag, cmd) = (tag.to_string(), cmd.to_string());
                    let reply = if cmd.starts_with("LOGIN") {
                        if let Some((parked, released)) = gate.take() {
                            let _ = parked.send(());
                            let _ = released.await;
                        }
                        format!("{tag} OK logged in\r\n")
                    } else if cmd.starts_with("EXAMINE") {
                        format!("* FLAGS (\\Seen)\r\n* 0 EXISTS\r\n* 0 RECENT\r\n{tag} OK [READ-ONLY] done\r\n")
                    } else {
                        format!("* BYE\r\n{tag} OK bye\r\n")
                    };
                    if s.get_mut().write_all(reply.as_bytes()).await.is_err() || cmd.starts_with("LOGOUT") {
                        break;
                    }
                }
            }
        });
        (port, parked, release)
    }

    /// `POST /api/sources/email` on `h`, with `meanwhile` run while the handler waits for the mail
    /// server to answer its sign-in check. Returns the response's status.
    async fn connect_mail_while(h: &Harness, meanwhile: impl std::future::Future<Output = ()>) -> u16 {
        let (port, parked, release) = mail_server_that_parks_a_sign_in().await;
        let request = h.send("POST", "/sources/email", json!({"host": "127.0.0.1", "port": port, "tls": false, "username": "me@example.com", "password": "app pass"}));
        tokio::pin!(request);
        tokio::select! {
            reply = &mut request => panic!("answered before the sign-in was checked: {}", reply.status),
            got = parked => got.unwrap(),
        }
        meanwhile.await;
        release.send(()).unwrap();
        request.await.status
    }

    #[tokio::test]
    async fn settings_save_does_not_bring_back_a_revoked_rule() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let mut saved = Config { home: home.clone(), ..Default::default() };
        saved.agent.auto_memory = false;
        saved.permissions.allow = vec!["shell:git *".into(), "shell:make *".into()];
        saved.save().unwrap();
        let ws = dir.path().join("ws");
        let (a, b) = two_cores_from_saved_settings(&home, &ws).await;
        let (ha, hb) = (serve_app(tempfile::tempdir().unwrap(), a.clone()), serve_app(tempfile::tempdir().unwrap(), b.clone()));
        let on_disk = || Config::read_saved(&home).unwrap();
        let has = |app: &App, rule: &str| app.policy.allow_rules().iter().any(|r| r == rule);

        // One core: a rule is revoked on A while A's settings handler waits for the mail login.
        let status = connect_mail_while(&ha, async {
            ha.json("DELETE", "/grants", json!({"rule": "shell:git *"})).await;
        })
        .await;
        assert_eq!(status, 200);
        let disk = on_disk();
        assert_eq!(disk.permissions.allow, vec!["shell:make *"], "the save brought the revoked rule back");
        assert_eq!(disk.sources.email.as_ref().map(|e| e.username.as_str()), Some("me@example.com"), "the change itself was saved");
        assert!(!has(&a, "shell:git *"), "{:?}", a.policy.allow_rules());
        assert_eq!(a.cfg().permissions.allow, vec!["shell:make *"]);

        // Two cores: B revokes the other while A's handler waits, and A takes that in (a turn of its
        // own starts) before the handler saves.
        let status = connect_mail_while(&ha, async {
            hb.json("DELETE", "/grants", json!({"rule": "shell:make *"})).await;
            a.reload_rules();
            assert!(!has(&a, "shell:make *"), "{:?}", a.policy.allow_rules());
        })
        .await;
        assert_eq!(status, 200);
        assert!(on_disk().permissions.allow.is_empty(), "the save brought the revoked rule back: {:?}", on_disk().permissions.allow);
        for app in [&a, &b] {
            app.reload_rules();
            assert!(!has(app, "shell:git *") && !has(app, "shell:make *"), "{:?}", app.policy.allow_rules());
            assert!(app.cfg().permissions.allow.is_empty(), "{:?}", app.cfg().permissions.allow);
        }
        let (fresh, _) = two_cores_from_saved_settings(&home, &ws).await;
        assert!(fresh.policy.allow_rules().is_empty(), "{:?}", fresh.policy.allow_rules());

        // A whole copy of the settings taken before a revoke is refused rather than saved over it.
        a.update_config(ConfigChange::edit(|c| c.permissions.allow.push("shell:ls *".into()))).unwrap();
        let mut copy = (*a.cfg()).clone();
        assert!(a.revoke_grant("shell:ls *").unwrap());
        copy.agent.name = "Wren".into();
        assert!(a.update_config(copy).is_err());
        let disk = on_disk();
        assert!(disk.permissions.allow.is_empty() && disk.agent.name != "Wren", "{:?} {}", disk.permissions.allow, disk.agent.name);
        a.cfg().store_secret("email_password", "").unwrap();
    }

    #[tokio::test]
    async fn new_core_drops_a_rule_revoked_while_it_started() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let mut saved = Config { home: home.clone(), ..Default::default() };
        saved.agent.auto_memory = false;
        saved.permissions.allow = vec!["shell:git *".into(), "shell:make *".into()];
        saved.save().unwrap();
        let ws = dir.path().join("ws");
        let (a, _) = two_cores_from_saved_settings(&home, &ws).await;
        let has = |app: &App, rule: &str| app.policy.allow_rules().iter().any(|r| r == rule);

        // A new core reads its settings as it starts. Before its startup ends (it waits on its
        // connected apps there), the running core revokes one of them.
        let cfg = Config::read_saved(&home).unwrap();
        assert!(a.revoke_grant("shell:git *").unwrap());
        assert_eq!(Config::read_saved(&home).unwrap().permissions.allow, vec!["shell:make *"]);
        let store = Store::open(&cfg.db_path()).unwrap();
        crate::risk_notice::accept_current(&store).unwrap();
        let b = App::assemble(cfg, ws, store, scripted(vec![])).await.unwrap();
        assert!(has(&b, "shell:git *"), "built from the settings it read: {:?}", b.policy.allow_rules());

        // Its first reload (the start of its first turn) takes the revoke in.
        b.reload_rules();
        assert!(!has(&b, "shell:git *"), "{:?}", b.policy.allow_rules());
        assert!(has(&b, "shell:make *"));
        assert_eq!(b.cfg().permissions.allow, vec!["shell:make *"]);
        let verdict = crate::permissions::decide_for(&b.policy, "shell", "git push", crate::permissions::Effect::ProcessExec, false);
        assert_eq!(verdict, crate::permissions::Verdict::Ask);
        // Nor does a save of its own write it back.
        b.update_config(ConfigChange::edit(|c| c.agent.name = "Wren".into())).unwrap();
        assert_eq!(Config::read_saved(&home).unwrap().permissions.allow, vec!["shell:make *"]);
    }

    #[tokio::test]
    async fn unreadable_config_drops_its_rules_on_reload() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let mut saved = Config { home: home.clone(), ..Default::default() };
        saved.agent.auto_memory = false;
        saved.permissions.allow = vec!["shell:git *".into(), "shell:make *".into()];
        saved.permissions.deny = vec!["web_fetch:*".into()];
        saved.save().unwrap();
        let cfg = Config::read_saved(&home).unwrap();
        let store = Store::open(&cfg.db_path()).unwrap();
        crate::risk_notice::accept_current(&store).unwrap();
        store.kv_set("allow_rules", &json!(["shell:ls *"]).to_string()).unwrap();
        let app = Arc::new(App::assemble(cfg, dir.path().join("ws"), store, scripted(vec![])).await.unwrap());
        let h = serve_app(dir, app.clone());
        let config = home.join("config.toml");
        let rules = || app.policy.allow_rules().into_iter().collect::<std::collections::BTreeSet<_>>();
        let set = |r: &[&str]| r.iter().map(|s| s.to_string()).collect::<std::collections::BTreeSet<_>>();
        let grants = || async {
            let reply = h.send("GET", "/grants", Value::Null).await;
            assert_eq!(reply.status, 200);
            let notice = reply.headers.iter().find(|(n, _)| n == "x-heron-notice").map(|(_, v)| v.clone());
            let listed: Vec<String> = reply.json().await.as_array().unwrap().iter().map(|g| g["rule"].as_str().unwrap().to_string()).collect();
            (listed, notice)
        };
        let fetch = |app: &App| crate::permissions::decide_for(&app.policy, "web_fetch", "https://example.com/", crate::permissions::Effect::NetworkRead, false);
        let git = |app: &App| crate::permissions::decide_for(&app.policy, "shell", "git push", crate::permissions::Effect::ProcessExec, false);
        assert_eq!(rules(), set(&["shell:git *", "shell:make *", "shell:ls *"]));
        assert_eq!(git(&app), crate::permissions::Verdict::Allow);
        assert_eq!(grants().await.1, None);

        // Unparseable: every rule that came from the file is dropped, and GET /api/grants says why.
        // The saved "always" answer stays, and so does the file's "never".
        std::fs::write(&config, "[permissions\nallow = [\"shell:git *\"\n").unwrap();
        let (listed, notice) = grants().await;
        assert_eq!(listed, vec!["shell:ls *"]);
        assert_eq!(notice.as_deref(), Some(crate::app::CONFIG_UNREADABLE_NOTICE));
        assert!(app.config_unreadable().unwrap().contains("config.toml"), "{:?}", app.config_unreadable());
        assert_eq!(rules(), set(&["shell:ls *"]));
        assert!(app.cfg().permissions.allow.is_empty(), "{:?}", app.cfg().permissions.allow);
        assert_eq!(git(&app), crate::permissions::Verdict::Ask);
        assert!(matches!(fetch(&app), crate::permissions::Verdict::Deny(_)), "{:?}", fetch(&app));
        // Still broken at the next reload: nothing comes back.
        app.reload_rules();
        assert_eq!(rules(), set(&["shell:ls *"]));

        // Parseable again: its rules apply again, and the note is gone.
        saved.save().unwrap();
        let (listed, notice) = grants().await;
        assert_eq!(listed.into_iter().collect::<std::collections::BTreeSet<_>>(), set(&["shell:git *", "shell:make *", "shell:ls *"]));
        assert_eq!(notice, None);
        assert_eq!(app.config_unreadable(), None);
        assert_eq!(app.cfg().permissions.allow, vec!["shell:git *", "shell:make *"]);
        assert_eq!(git(&app), crate::permissions::Verdict::Allow);

        // Missing: its rules are gone. The saved answer and the "never" stay.
        std::fs::remove_file(&config).unwrap();
        app.reload_rules();
        assert_eq!(rules(), set(&["shell:ls *"]));
        assert!(app.cfg().permissions.allow.is_empty(), "{:?}", app.cfg().permissions.allow);
        assert_eq!(git(&app), crate::permissions::Verdict::Ask);
        assert!(matches!(fetch(&app), crate::permissions::Verdict::Deny(_)), "{:?}", fetch(&app));
        assert_eq!(grants().await, (vec!["shell:ls *".to_string()], None));
    }

    #[tokio::test]
    async fn never_on_one_core_holds_on_another() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let mut saved = Config { home: home.clone(), ..Default::default() };
        saved.agent.auto_memory = false;
        saved.save().unwrap();
        let ws = dir.path().join("ws");
        let core = |llm: Arc<dyn Llm>| {
            let (home, ws) = (home.clone(), ws.clone());
            async move {
                let cfg = Config::read_saved(&home).unwrap();
                let store = Store::open(&cfg.db_path()).unwrap();
                crate::risk_notice::accept_current(&store).unwrap();
                Arc::new(App::assemble(cfg, ws, store, llm).await.unwrap())
            }
        };
        let fetch = ScriptedLlm::tool_call("web_fetch", json!({"url": "https://example.com/"}));
        let a = core(scripted(vec![])).await;
        let b = core(scripted(vec![fetch, Message::assistant("Done.")])).await;
        // B runs in Yolo mode: what it is not told to refuse, it runs unasked.
        b.policy.set_mode(Mode::Yolo);
        let decide = |app: &App| crate::permissions::decide_for(&app.policy, "web_fetch", "https://example.com/", crate::permissions::Effect::NetworkRead, false);
        assert_eq!(decide(&b), crate::permissions::Verdict::Allow);
        let (ha, hb) = (serve_app(tempfile::tempdir().unwrap(), a.clone()), serve_app(tempfile::tempdir().unwrap(), b.clone()));

        // "Never" is set on A. B's next turn refuses the tool before it runs.
        assert_eq!(ha.json("POST", "/tools/web_fetch", json!({"setting": "never"})).await["setting"], "never");
        assert_eq!(Config::read_saved(&home).unwrap().permissions.deny, vec!["web_fetch:*"]);
        let events = hb.chat(json!({"message": "fetch it"})).await.all().await;
        let end = find(&events, "tool_end");
        assert_eq!((end["name"].as_str(), end["ok"].as_bool()), (Some("web_fetch"), Some(false)), "{events:?}");
        assert!(end["preview"].as_str().unwrap().contains("switched this ability off"), "{end}");
        assert!(b.policy.deny_rules().contains(&"web_fetch:*".to_string()), "{:?}", b.policy.deny_rules());
        assert!(matches!(decide(&b), crate::permissions::Verdict::Deny(_)));
        assert_eq!(b.cfg().permissions.deny, vec!["web_fetch:*"]);
        assert_eq!(hb.json("GET", "/tools", Value::Null).await.as_array().unwrap().iter().find(|t| t["name"] == "web_fetch").unwrap()["setting"], "never");
        // An unattended run on B starts from the same rules.
        b.reload_rules();
        assert!(b.policy.deny_rules().contains(&"web_fetch:*".to_string()));

        // A save of B's own keeps A's choice in the file.
        hb.json("POST", "/tools/shell", json!({"setting": "never"})).await;
        assert_eq!(Config::read_saved(&home).unwrap().permissions.deny, vec!["web_fetch:*", "shell:*"]);
        a.reload_rules();
        assert!(a.policy.deny_rules().contains(&"shell:*".to_string()), "{:?}", a.policy.deny_rules());

        // Set back to "ask" on A, B follows at its next reload.
        ha.json("POST", "/tools/web_fetch", json!({"setting": "ask"})).await;
        b.reload_rules();
        assert!(!b.policy.deny_rules().contains(&"web_fetch:*".to_string()), "{:?}", b.policy.deny_rules());
        assert_eq!(decide(&b), crate::permissions::Verdict::Allow);
        assert_eq!(b.cfg().permissions.deny, vec!["shell:*"]);
    }

    #[tokio::test]
    async fn revoked_config_grant_closes_for_a_run_in_progress() {
        use crate::tools::shell::tests::{q, seatbelt, AskTierDir};
        // A folder outside the working folder that needs a grant, wherever the checkout lives.
        let bare = harness(scripted(vec![]), true, |_| {}).await;
        let outside = AskTierDir::new(&bare.app.cfg(), &bare.app.workspace);
        let granted = outside.path().join("granted");
        std::fs::create_dir_all(&granted).unwrap();
        let secret = granted.join("notes.txt");
        std::fs::write(&secret, "GRANTED-CONTENT").unwrap();
        let search = json!({"pattern": "GRANTED-CONTENT"});
        let cat = json!({"command": format!("cat {}", q(&secret))});
        let call = |app: Arc<App>, ctx: crate::tools::ToolCtx, tool: &'static str, args: Value| async move { app.tools.get(tool).unwrap().call(&args, &ctx).await.unwrap() };
        // With no grant, neither a walk through a link nor a command reads it.
        std::os::unix::fs::symlink(&secret, bare.app.workspace.join("granted-link.txt")).unwrap();
        let bare_ctx = bare.app.agent(Arc::new(crate::permissions::DenyAll)).tool_ctx("no-grant");
        let found = call(bare.app.clone(), bare_ctx.clone(), "search_files", search.clone()).await;
        assert!(!found.contains("granted-link.txt"), "the folder must need a grant: {found}");
        if seatbelt(&bare.app.cfg()) {
            let out = call(bare.app.clone(), bare_ctx, "shell", cat.clone()).await;
            assert!(!out.contains("GRANTED-CONTENT"), "the folder must need a grant: {out}");
        }

        let rule = format!("read:{}/**", granted.display());
        let h = harness(scripted(vec![]), true, |c| c.permissions.allow = vec![rule.clone()]).await;
        assert!(h.app.policy.allow_rules().contains(&rule), "{:?}", h.app.policy.rule_notices());
        // A run that started with the rule in its settings, and keeps that snapshot.
        let agent = h.app.agent(Arc::new(crate::permissions::DenyAll));
        let ctx = agent.tool_ctx("run-in-progress");
        // A walk of the working folder opens the granted file through a link to it.
        std::os::unix::fs::symlink(&secret, h.app.workspace.join("granted-link.txt")).unwrap();
        let sandboxed = seatbelt(&h.app.cfg());
        assert!(call(h.app.clone(), ctx.clone(), "search_files", search.clone()).await.contains("granted-link.txt"));
        if sandboxed {
            assert!(call(h.app.clone(), ctx.clone(), "shell", cat.clone()).await.contains("GRANTED-CONTENT"));
        }

        h.json("DELETE", "/grants", json!({"rule": rule})).await;
        assert!(ctx.cfg.permissions.allow.contains(&rule), "the run's settings snapshot still names it");
        // The run's next walk and next command no longer get it.
        let found = call(h.app.clone(), ctx.clone(), "search_files", search).await;
        assert!(!found.contains("granted-link.txt"), "{found}");
        if sandboxed {
            let out = call(h.app.clone(), ctx, "shell", cat).await;
            assert!(!out.contains("GRANTED-CONTENT"), "{out}");
        }
    }

    /// A model whose step does not yield: once it has begun, an abort cannot stop it before it ends.
    struct Stalls(Arc<std::sync::atomic::AtomicBool>);

    #[async_trait]
    impl Llm for Stalls {
        async fn chat(&self, _req: ChatRequest, on_delta: DeltaFn<'_>) -> Result<ChatResponse> {
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(600));
            on_delta("late words");
            Ok(ChatResponse { message: Message::assistant("late words"), usage: Default::default(), model: "stalls".into() })
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn no_event_follows_run_end_after_cancel() {
        let entered = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let h = harness(Arc::new(Stalls(entered.clone())), true, |_| {}).await;
        let mut bus = h.app.events.subscribe(None);
        let mut chat = h.chat(json!({"message": "hi"})).await;
        let session = chat.next().await.unwrap();
        let run_id = session["run_id"].as_str().unwrap().to_string();
        while !entered.load(std::sync::atomic::Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        // Stopped while the step is still going on another thread.
        assert_eq!(h.json("POST", "/cancel", json!({"session_id": session["id"]})).await["cancelled"], true);
        // Everything the run published, read for longer than the step takes.
        let mut kinds = vec![];
        while let Ok(Some(Item::Event(e))) = tokio::time::timeout(Duration::from_millis(1500), bus.next()).await {
            if e.run_id == run_id {
                kinds.push(e.kind().to_string());
            }
        }
        assert_eq!(kinds.first().map(String::as_str), Some("session"), "{kinds:?}");
        assert_eq!(&kinds[kinds.len() - 2..], ["cancelled", "run_end"], "nothing of the run follows run_end: {kinds:?}");
        assert!(kinds.iter().any(|k| k == "text"), "the step's own output came before the end: {kinds:?}");
        assert_eq!(kinds.iter().filter(|k| *k == "run_end").count(), 1, "{kinds:?}");
        assert!(!kinds.iter().any(|k| k == "done"), "a stopped run reports no outcome: {kinds:?}");
    }

    #[tokio::test]
    async fn parked_approval_waits_for_the_risk_notice() {
        let h = harness(scripted(vec![]), false, |_| {}).await;
        let sid = h.app.store.create_session("⏰ stretch", "schedule").unwrap();
        let args = json!({"path": "later.txt", "content": "carried out"});
        let id = h.app.store.park_approval(&sid, "write_file", &args.to_string(), "write later.txt", "write later.txt", crate::permissions::Effect::WorkspaceWrite.label(), false, "stretch").unwrap();
        let approve = format!("/approvals/{id}");
        if crate::risk_notice::required() {
            let refused = h.send("POST", &approve, json!({"action": "approve"})).await;
            assert_eq!(refused.status, 428);
            assert_eq!(refused.json().await, json!({"error": "risk_notice_not_accepted"}));
            let err = h.app.run_parked(id).await.expect_err("refused directly too");
            assert!(err.downcast_ref::<crate::risk_notice::RiskNoticeNotAccepted>().is_some(), "{err:#}");
            // Nothing was done, and the request is still waiting: it was not claimed.
            assert!(!h.app.workspace.join("later.txt").exists());
            let waiting = h.app.store.pending_approvals().unwrap();
            assert_eq!(waiting.iter().map(|a| (a.id, a.status.as_str())).collect::<Vec<_>>(), vec![(id, "new")]);
            crate::risk_notice::accept_current(&h.app.store).unwrap();
        }
        // Accepted (or not needed here): the same request is carried out, once.
        h.json("POST", &approve, json!({"action": "approve"})).await;
        assert_eq!(std::fs::read_to_string(h.app.workspace.join("later.txt")).unwrap(), "carried out");
        assert!(h.app.store.pending_approvals().unwrap().is_empty());
        assert_eq!(h.send("POST", &approve, json!({"action": "approve"})).await.status, 400, "and only once");
    }

    #[tokio::test]
    async fn print_endpoint_line_has_port_token_and_owner() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config { home: dir.path().join("home"), ..Default::default() };
        let app = Arc::new(App::assemble(cfg.clone(), dir.path().join("ws"), Store::in_memory().unwrap(), scripted(vec![])).await.unwrap());
        let started = start_endpoint(app, "127.0.0.1", 0).await.unwrap();
        let line = endpoint_line(&started);
        assert!(!line.contains('\n'), "one line: {line}");
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v, json!({"port": started.port, "token": started.token, "background_owner": true, "version": env!("CARGO_PKG_VERSION")}));
        assert_ne!(started.port, 0);
        assert_eq!(started.token.len(), 64);
        // The token works on the port the line names.
        let state = reqwest::Client::new().get(format!("http://127.0.0.1:{}/api/state", v["port"])).header("x-heron-token", v["token"].as_str().unwrap()).send().await.unwrap();
        assert_eq!(state.status(), 200);
        // A second core on the same home finds the background work already taken.
        let other = Arc::new(App::assemble(cfg, dir.path().join("ws"), Store::in_memory().unwrap(), scripted(vec![])).await.unwrap());
        let second = start_endpoint(other, "127.0.0.1", 0).await.unwrap();
        let v2: Value = serde_json::from_str(&endpoint_line(&second)).unwrap();
        assert_eq!(v2["background_owner"], false);
        assert_ne!(v2["port"], v["port"]);
        assert_ne!(v2["token"], v["token"]);
        started.task.abort();
        second.task.abort();
    }

    #[tokio::test]
    async fn stream_ends_after_run_end() {
        let h = harness(scripted(vec![Message::assistant("Hello there.")]), true, |_| {}).await;
        // Another run's events on the bus are not part of this response.
        h.app.events.publish("elsewhere", "another-run", json!({"type": "text", "text": "not mine"}));
        let events = h.chat(json!({"message": "hi"})).await.all().await;
        let session = &events[0];
        assert_eq!(session["type"], "session");
        assert_eq!(session["id"], session["session_id"]);
        let run_id = session["run_id"].as_str().unwrap();
        assert!(uuid::Uuid::parse_str(run_id).is_ok(), "{run_id}");
        assert!(events.iter().all(|e| e["run_id"] == run_id), "{events:?}");
        let seqs: Vec<u64> = events.iter().map(|e| e["seq"].as_u64().unwrap()).collect();
        assert!(seqs.windows(2).all(|w| w[0] < w[1]), "{seqs:?}");
        assert_eq!(find(&events, "done")["text"], "Hello there.");
        assert_eq!(types(&events).last(), Some(&"run_end"), "the stream ended right after run_end: {events:?}");
        assert_eq!(types(&events).iter().filter(|t| **t == "run_end").count(), 1);
    }

    #[tokio::test]
    async fn stalled_chat_reader_cannot_accumulate_unlimited_events() {
        let h = harness(scripted(vec![]), true, |_| {}).await;
        let run_id = crate::events::new_run_id();
        // A `/api/chat` reader that stops reading while a chatty run goes on.
        let mut stalled = Box::pin(run_view(h.app.events.subscribe(None), run_id.clone()));
        let tail_from = h.app.events.last_seq();
        let sink = h.app.sink_for("s", &run_id);
        for i in 0..crate::events::LIVE_CAPACITY + 10 {
            sink(crate::agent::Event::Text(i.to_string()));
        }
        h.app.publish_outcome("s", &run_id, Ok("all of it"));
        h.app.publish_run_end("s", &run_id);
        // Nothing beyond the bus's fixed capacity was kept for it: it is told to resync and dropped.
        let got: Vec<Item> = tokio::time::timeout(Duration::from_secs(5), stalled.as_mut().collect::<Vec<_>>()).await.unwrap();
        assert!(matches!(got.as_slice(), [Item::Resync { .. }]), "{got:?}");
        // The run itself finished: its end is on the bus for a client that comes back.
        let mut back = h.app.events.subscribe(Some(tail_from));
        let mut kinds = vec![];
        while let Ok(Some(Item::Event(e))) = tokio::time::timeout(Duration::from_millis(200), back.next()).await {
            kinds.push(e.kind().to_string());
        }
        assert_eq!(kinds.len(), crate::events::LIVE_CAPACITY + 12);
        assert_eq!(&kinds[kinds.len() - 2..], ["done", "run_end"]);
    }
}
