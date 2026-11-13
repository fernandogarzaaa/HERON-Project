//! Local HTTP server behind the desktop app and web UI.
//! Loopback-only by default; every API call requires the per-launch token.

use crate::agent::{Event, EventSink, Session};
use crate::app::App;
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
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::UnboundedReceiverStream;

const INDEX_HTML: &str = include_str!("../assets/index.html");

type Tx = mpsc::UnboundedSender<Value>;

pub struct ServerState {
    pub app: Arc<App>,
    pub token: String,
    /// True unless the user deliberately bound the server to a non-loopback address.
    pub loopback_only: bool,
    approvals: Mutex<HashMap<String, (String, oneshot::Sender<Approval>)>>,
    /// Replies in progress, by conversation. The number tells one reply from the next in the same
    /// conversation, so a reply that is finishing can never clear the entry of the one that followed it.
    running: Mutex<HashMap<String, (u64, tokio::task::AbortHandle, Tx)>>,
    next_run: std::sync::atomic::AtomicU64,
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

pub fn router(app: Arc<App>, token: String) -> Router {
    let bind = app.cfg().server.bind.clone();
    let loopback_only = matches!(bind.as_str(), "127.0.0.1" | "localhost" | "::1");
    let state = Arc::new(ServerState { app, token, loopback_only, approvals: Default::default(), running: Default::default(), next_run: Default::default() });
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
        .route("/persona", get(get_persona).post(set_persona).delete(reset_persona))
        .route("/pause", post(set_pause))
        .route("/approvals", get(list_approvals))
        .route("/approvals/{id}", post(answer_approval))
        .route("/tools", get(list_tools))
        .route("/tools/{name}", post(set_tool))
        .route("/today", get(get_today))
        .route("/telegram", get(get_telegram).post(set_telegram).delete(remove_telegram))
        .route("/usage", get(get_usage).post(set_usage))
        .route("/sources", get(get_sources))
        .route("/sources/{name}", post(connect_source).delete(disconnect_source))
        .route("/sources/{name}/learn", post(learn_source))
        .route("/onboarded", post(set_onboarded))
        .route("/suggestions", get(list_suggestions))
        .route("/suggestions/{id}", post(answer_suggestion))
        .route("/settings", get(get_settings).post(set_settings))
        .layer(middleware::from_fn_with_state(state.clone(), auth));
    Router::new()
        .route("/", get(|| async { ([(header::CACHE_CONTROL, "no-store"), (header::HeaderName::from_static("content-security-policy"), "default-src 'self'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'")], Html(INDEX_HTML)) }))
        .nest("/api", api)
        .with_state(state)
}

pub async fn serve(app: Arc<App>, listener: tokio::net::TcpListener, token: String) -> Result<()> {
    tokio::spawn(app.clone().scheduler_loop());
    tokio::spawn(app.clone().observer_loop());
    crate::telegram::spawn_if_configured(&app);
    axum::serve(listener, router(app, token)).await?;
    Ok(())
}

/// Bind and start serving in the background. Returns the launch URL (with token) and the server task.
pub async fn start(app: Arc<App>, bind: &str, port: u16) -> Result<(String, tokio::task::JoinHandle<Result<()>>)> {
    let listener = match tokio::net::TcpListener::bind((bind, port)).await {
        Ok(l) => l,
        // The preferred port is taken (perhaps another Heron window): fall back to any free port.
        Err(_) => tokio::net::TcpListener::bind((bind, 0)).await?,
    };
    let addr = listener.local_addr()?;
    let token = new_token();
    let host = if addr.ip().is_unspecified() || addr.ip().is_loopback() { "127.0.0.1".to_string() } else { addr.ip().to_string() };
    let url = format!("http://{host}:{}/?token={token}", addr.port());
    Ok((url, tokio::spawn(serve(app, listener, token))))
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
    Ok(Json(json!({
        "name": cfg.agent.name, "base_url": cfg.llm.base_url, "model": cfg.llm.model, "fast_model": cfg.llm.fast_model,
        "fallback_models": cfg.llm.fallback_models.join(", "), "key_source": key_source,
        "mode": st.app.policy.mode().name(), "daemon_mode": cfg.permissions.daemon_mode, "auto_memory": cfg.agent.auto_memory,
        "neural_memory": cfg.agent.neural_memory, "neural_available": crate::embed::neural_built_in(),
        "browser": crate::browser::find(&cfg).map(|p| p.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()), "browser_visible": cfg.agent.browser_visible,
        "workspace": st.app.workspace.display().to_string(), "home": cfg.home.display().to_string(),
        "allow_rules": st.app.policy.allow_rules(), "secret_storage": cfg.secret_storage(),
        "mcp": st.app.mcp_status, "mcp_changed": st.app.mcp_pending.keys().collect::<Vec<_>>(),
    })))
}

async fn set_settings(State(st): S, Json(body): Json<Value>) -> ApiResult {
    let mut cfg = (*st.app.cfg()).clone();
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
    if let Some(v) = text("daemon_mode") {
        Mode::parse(&v)?;
        cfg.permissions.daemon_mode = v;
    }
    if let Some(v) = text("mode") {
        st.app.policy.set_mode(Mode::parse(&v)?);
        cfg.permissions.mode = v;
    }
    if let Some(v) = body.get("auto_memory").and_then(|v| v.as_bool()) {
        cfg.agent.auto_memory = v;
    }
    if let Some(v) = body.get("browser_visible").and_then(|v| v.as_bool()) {
        if v != cfg.agent.browser_visible {
            cfg.agent.browser_visible = v;
            crate::browser::shutdown().await; // the next use starts it the new way
        }
    }
    if let Some(v) = body.get("neural_memory").and_then(|v| v.as_bool()) {
        cfg.agent.neural_memory = v && crate::embed::neural_built_in();
    }
    if let Some(v) = text("workspace").filter(|v| !v.is_empty()) {
        cfg.agent.workspace = v;
    }
    if let Some(key) = text("api_key").filter(|k| !k.is_empty()) {
        cfg.store_api_key(&key)?;
    }
    st.app.update_config(cfg)?;
    get_settings(State(st)).await
}

#[derive(Deserialize)]
struct ModeBody {
    mode: String,
}

async fn set_mode(State(st): S, Json(b): Json<ModeBody>) -> ApiResult {
    st.app.policy.set_mode(Mode::parse(&b.mode)?);
    Ok(Json(json!({"mode": st.app.policy.mode().name()})))
}

// ---------- sessions ----------

async fn list_sessions(State(st): S) -> ApiResult {
    Ok(Json(json!(st.app.store.list_sessions(100)?)))
}

async fn new_session(State(st): S) -> ApiResult {
    Ok(Json(json!({"id": st.app.store.create_session("")?})))
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
    tx: Tx,
}

#[async_trait]
impl Approver for WebApprover {
    async fn approve(&self, req: &ApprovalRequest) -> Approval {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let (tx, rx) = oneshot::channel();
        self.state.approvals.lock().unwrap().insert(id.clone(), (self.session_id.clone(), tx));
        let _ = self.tx.send(json!({"type": "approval", "id": id, "tool": req.tool, "summary": req.summary, "risk": req.risk.label(), "detail": req.detail,
            "always_rule": crate::permissions::always_rule(&req.tool, &req.summary), "can_always": !req.risk.high() && !req.tainted, "can_session": crate::agent::may_grant_for_conversation(&req.tool, req.risk, req.tainted), "tainted": req.tainted}));
        // If nobody answers (window closed), decline rather than hang forever.
        let answer = tokio::time::timeout(Duration::from_secs(900), rx).await;
        self.state.approvals.lock().unwrap().remove(&id);
        match answer {
            Ok(Ok(a)) => a,
            _ => Approval::Deny(Some("no answer to the approval request".into())),
        }
    }
}

fn event_json(e: Event) -> Value {
    match e {
        Event::Text(t) => json!({"type": "text", "text": t}),
        Event::ToolStart { id, name, summary, risk } => json!({"type": "tool_start", "id": id, "name": name, "summary": summary, "risk": risk.label()}),
        Event::ToolEnd { id, name, ok, preview } => json!({"type": "tool_end", "id": id, "name": name, "ok": ok, "preview": preview}),
        Event::Notice(t) => json!({"type": "notice", "text": t}),
        Event::Usage(u) => json!({"type": "usage", "prompt": u.prompt_tokens, "completion": u.completion_tokens}),
        Event::Model { name, why } => json!({"type": "model", "name": name, "why": why}),
    }
}

#[derive(Deserialize)]
struct ChatBody {
    session_id: Option<String>,
    message: String,
}

async fn chat(State(st): S, Json(body): Json<ChatBody>) -> std::result::Result<Response, ApiError> {
    let message = body.message.trim().to_string();
    if message.is_empty() {
        return Err(bad("Type a message first."));
    }
    let mut session = match body.session_id.as_deref().filter(|s| !s.is_empty()) {
        Some(id) => Session::load(&st.app.store, id).map_err(|e| ApiError(StatusCode::NOT_FOUND, e.to_string()))?,
        None => Session::create(&st.app.store, "")?,
    };
    let sid = session.id.clone();
    let (tx, rx) = mpsc::unbounded_channel::<Value>();
    {
        let mut running = st.running.lock().unwrap();
        if running.contains_key(&sid) {
            return Err(ApiError(StatusCode::CONFLICT, "Heron is still replying in this conversation. Stop it first, or wait.".into()));
        }
        let _ = tx.send(json!({"type": "session", "id": sid}));
        let run_id = st.next_run.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let finished = move |state: &ServerState, sid: &str| {
            let mut running = state.running.lock().unwrap();
            if running.get(sid).map(|(id, _, _)| *id == run_id).unwrap_or(false) {
                running.remove(sid);
            }
        };
        let state = st.clone();
        let task_tx = tx.clone();
        let task_sid = sid.clone();
        let handle = tokio::spawn(async move {
            let agent = state.app.agent(Arc::new(WebApprover { state: state.clone(), session_id: task_sid.clone(), tx: task_tx.clone() }));
            let sink_tx = task_tx.clone();
            let sink: EventSink = Arc::new(move |e| {
                let _ = sink_tx.send(event_json(e));
            });
            match agent.run_turn(&mut session, &message, &sink).await {
                Ok(text) => {
                    let _ = task_tx.send(json!({"type": "done", "text": text, "title": state.app.store.session_title(&task_sid)}));
                    state.app.persist_rules();
                    // Free the conversation before the (slower) background learning step.
                    finished(&state, &task_sid);
                    let learned = agent.learn(&session, &message, &text).await;
                    if !learned.is_empty() {
                        let _ = task_tx.send(json!({"type": "learned", "items": learned}));
                    }
                }
                Err(e) => {
                    let _ = task_tx.send(json!({"type": "error", "message": format!("{e:#}")}));
                }
            }
            finished(&state, &task_sid);
        });
        running.insert(sid, (run_id, handle.abort_handle(), tx));
    }
    let stream = UnboundedReceiverStream::new(rx).map(|v| Ok::<_, std::convert::Infallible>(SseEvent::default().data(v.to_string())));
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))).into_response())
}

#[derive(Deserialize)]
struct ApproveBody {
    id: String,
    decision: String,
}

async fn approve(State(st): S, Json(b): Json<ApproveBody>) -> ApiResult {
    let Some((_, tx)) = st.approvals.lock().unwrap().remove(&b.id) else {
        return Err(ApiError(StatusCode::NOT_FOUND, "That request is no longer waiting for an answer.".into()));
    };
    let _ = tx.send(match b.decision.as_str() {
        "once" => Approval::Once,
        "always" => Approval::Always,
        "session" => Approval::Session,
        _ => Approval::Deny(None),
    });
    Ok(Json(json!({"ok": true})))
}

#[derive(Deserialize)]
struct SessionBody {
    session_id: String,
}

async fn cancel(State(st): S, Json(b): Json<SessionBody>) -> ApiResult {
    let entry = st.running.lock().unwrap().remove(&b.session_id);
    st.approvals.lock().unwrap().retain(|_, (sid, _)| *sid != b.session_id);
    if let Some((_, handle, tx)) = entry {
        handle.abort();
        let _ = tx.send(json!({"type": "cancelled"}));
    }
    Ok(Json(json!({"ok": true})))
}

#[derive(Deserialize)]
struct UndoBody {
    session_id: String,
    /// Restore even though the file has changed since (the current version is kept beside it).
    #[serde(default)]
    force: bool,
    /// With `force`: the checkpoint the person was asked about, as named in the refusal. Restoring
    /// anyway applies to that one only.
    #[serde(default)]
    checkpoint: Option<i64>,
}

async fn undo(State(st): S, Json(b): Json<UndoBody>) -> ApiResult {
    use crate::tools::fs::Undo;
    let agent = st.app.agent(Arc::new(crate::permissions::DenyAll));
    let restore_anyway = match (b.force, b.checkpoint) {
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
    })))
}

async fn set_awareness(State(st): S, Json(b): Json<Value>) -> ApiResult {
    let mut cfg = (*st.app.cfg()).clone();
    if let Some(v) = b.get("enabled").and_then(|v| v.as_bool()) {
        cfg.agent.ambient_context = v;
    }
    if let Some(v) = b.get("reflect").and_then(|v| v.as_bool()) {
        cfg.agent.reflect = v;
    }
    if let Some(v) = b.get("proactive").and_then(|v| v.as_str()) {
        if !["off", "suggest", "prepare", "takeover"].contains(&v) {
            return Err(bad("Unknown proactivity level."));
        }
        cfg.agent.proactive = v.to_string();
    }
    st.app.update_config(cfg)?;
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

async fn get_persona(State(st): S) -> ApiResult {
    let cfg = st.app.cfg();
    Ok(Json(json!({"text": crate::persona::load(&cfg), "is_default": crate::persona::is_default(&cfg), "name": cfg.agent.name, "max": crate::persona::MAX_CHARS})))
}

async fn set_persona(State(st): S, Json(b): Json<Value>) -> ApiResult {
    let mut cfg = (*st.app.cfg()).clone();
    if let Some(text) = b.get("text").and_then(|t| t.as_str()) {
        crate::persona::save(&cfg, text)?;
    }
    if let Some(name) = b.get("name").and_then(|n| n.as_str()).map(str::trim).filter(|n| !n.is_empty() && n.chars().count() <= 40) {
        if name != cfg.agent.name {
            cfg.agent.name = name.to_string();
            st.app.update_config(cfg)?;
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

async fn list_approvals(State(st): S) -> ApiResult {
    Ok(Json(json!(st.app.store.pending_approvals()?)))
}

async fn answer_approval(State(st): S, Path(id): Path<i64>, Json(b): Json<Value>) -> ApiResult {
    match b.get("action").and_then(|a| a.as_str()) {
        Some("approve") => Ok(Json(json!({"result": st.app.run_parked(id).await?}))),
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
    let mut cfg = (*st.app.cfg()).clone();
    let rule = format!("{name}:*");
    cfg.permissions.allow.retain(|r| *r != rule);
    cfg.permissions.deny.retain(|r| *r != rule);
    match setting {
        "allow" => cfg.permissions.allow.push(rule),
        "never" => cfg.permissions.deny.push(rule),
        _ => {}
    }
    st.app.update_config(cfg)?;
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
    let mut cfg = (*st.app.cfg()).clone();
    let tokens = |v: &Value| v.as_u64().or_else(|| v.as_str().and_then(|s| s.replace([',', ' ', '_'], "").parse().ok()));
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
    st.app.update_config(cfg)?;
    get_usage(State(st)).await
}

async fn get_telegram(State(st): S) -> ApiResult {
    let configured = st.app.cfg().secret("telegram_token").is_some();
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
    st.app.cfg().store_secret("telegram_token", token)?;
    // Restarts the connection, so a replaced token is the one in use and there is only ever one reader.
    crate::telegram::spawn_if_configured(&st.app);
    get_telegram(State(st)).await
}

async fn remove_telegram(State(st): S) -> ApiResult {
    st.app.cfg().store_secret("telegram_token", "")?;
    crate::telegram::stop(&st.app);
    crate::telegram::unpair(&st.app);
    Ok(Json(json!({"configured": false, "paired": false})))
}

// ---------- onboarding & context sources ----------

async fn get_sources(State(st): S) -> ApiResult {
    let cfg = st.app.cfg();
    let store = &st.app.store;
    let learned = |source: &str| store.memories_by_source(source, 500).map(|m| m.len()).unwrap_or(0);
    let hosts: Vec<String> = crate::sources::calendar_links(&cfg).iter().filter_map(|l| url::Url::parse(&crate::sources::normalize_calendar_url(l)).ok()).filter_map(|u| u.host_str().map(String::from)).collect();
    Ok(Json(json!({
        "onboarded": store.kv_get("onboarded").is_some(),
        "demo": crate::llm::is_demo(&cfg),
        "has_model": cfg.api_key().is_some() || is_local(&cfg.llm.base_url),
        "model": cfg.llm.model,
        "about": {"saved": learned("onboarding"), "indexed": store.knowledge_count("about"), "learned": learned("about")},
        "computer": {"connected": store.knowledge_count("computer") > 0, "indexed": store.knowledge_count("computer"), "learned": learned("computer")},
        "documents": {"connected": !cfg.sources.folders.is_empty(), "folders": cfg.sources.folders, "indexed": store.knowledge_count("documents"), "learned": learned("documents")},
        "calendar": {"connected": !hosts.is_empty(), "hosts": hosts, "indexed": store.knowledge_count("calendar"), "learned": learned("calendar")},
        "email": {"connected": cfg.sources.email.is_some(), "address": cfg.sources.email.as_ref().map(|e| e.username.clone()), "indexed": store.knowledge_count("email"), "learned": learned("email")},
    })))
}

async fn connect_source(State(st): S, Path(name): Path<String>, Json(b): Json<Value>) -> ApiResult {
    let text = |k: &str| b.get(k).and_then(|v| v.as_str()).map(str::trim).filter(|v| !v.is_empty());
    let app = &st.app;
    let mut cfg = (*app.cfg()).clone();
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
            if crate::permissions::classify_path(&folder, &app.workspace, false) == crate::permissions::Effect::Privileged {
                return Err(bad("That folder holds keys or passwords, so Heron will not index it."));
            }
            let folder = folder.to_string_lossy().to_string();
            if !cfg.sources.folders.contains(&folder) {
                cfg.sources.folders.push(folder);
            }
            app.update_config(cfg)?;
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
            cfg.sources.email = Some(crate::config::EmailConfig { host: host.to_string(), port, username: username.to_string(), tls });
            cfg.store_secret("email_password", password)?;
            app.update_config(cfg)?;
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
    let mut cfg = (*st.app.cfg()).clone();
    match name.as_str() {
        "email" => {
            cfg.sources.email = None;
            cfg.store_secret("email_password", "")?;
        }
        "calendar" => {
            cfg.sources.calendars.clear();
            cfg.store_secret("calendar_links", "")?;
        }
        "documents" => cfg.sources.folders.clear(),
        _ => {}
    }
    st.app.update_config(cfg)?;
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
