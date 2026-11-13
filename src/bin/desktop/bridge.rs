//! Rust bridge between the Slint UI and the local Heron server.
//!
//! The backend is unchanged: it runs in this same process as an Axum server on a
//! random loopback port, guarded by a per-launch token. The UI never talks to the
//! backend in-process. It talks HTTP, exactly like the old web UI did, through
//! [`HeronClient`]. That keeps one client contract for every view, and the server
//! stays testable over a socket.
//!
//! Threading rule, the one thing every lane must respect: Slint widgets live on
//! the UI thread only. Network work runs on the [`Bridge`] runtime (a background
//! tokio runtime owned by the desktop binary). A background task never touches a
//! widget directly. It hands results to the UI thread with
//! `Weak::upgrade_in_event_loop`, and the UI-thread closure mutates the window's
//! models. The chat stream follows the same rule: [`HeronClient::stream_chat`]
//! pushes [`ChatEvent`] values into a channel, and the binary's forwarder task
//! moves them onto the UI thread one by one.

use super::sse::SseParser;
use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use heron::llm::Message;
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc;

/// One JSON event from `POST /api/chat`, in stream order.
/// The shapes match `event_json` in `src/server.rs`; the `type` field tags the variant.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatEvent {
    /// First event of a turn: the conversation id (new or existing).
    Session {
        id: String,
    },
    /// A chunk of streamed assistant text. Append it to the current reply.
    Text {
        text: String,
    },
    ToolStart {
        id: String,
        name: String,
        summary: String,
        risk: String,
    },
    ToolEnd {
        id: String,
        name: String,
        ok: bool,
        preview: String,
    },
    Notice {
        text: String,
    },
    Usage {
        prompt: u64,
        completion: u64,
    },
    Model {
        name: String,
        why: String,
    },
    /// Heron asks before a consequential step. Answer with [`HeronClient::approve`].
    /// The UI must not let the turn hang: the server declines after 15 minutes.
    Approval {
        id: String,
        tool: String,
        summary: String,
        risk: String,
        #[serde(default)]
        detail: String,
        #[serde(default)]
        always_rule: Option<String>,
        #[serde(default)]
        can_always: bool,
        #[serde(default)]
        can_session: bool,
        #[serde(default)]
        tainted: bool,
    },
    /// The turn finished. `text` is the full reply; `title` is the session title.
    Done {
        text: String,
        title: String,
    },
    /// Facts Heron learned from this turn.
    Learned {
        items: Vec<Value>,
    },
    /// The turn failed. `message` is human-readable.
    Error {
        message: String,
    },
    /// The turn was cancelled with `POST /api/cancel`.
    Cancelled,
}

/// The subset of `GET /api/state` the shell shows. Unknown fields are ignored,
/// so the server can grow the payload without breaking the UI.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AppState {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub workspace: String,
    #[serde(default)]
    pub has_key: bool,
    #[serde(default)]
    pub demo: bool,
    #[serde(default)]
    pub memories: u64,
    #[serde(default)]
    pub unread: u64,
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub waiting: u64,
    #[serde(default)]
    pub onboarded: bool,
    #[serde(default)]
    pub suggestions: u64,
    #[serde(default)]
    pub tools: u64,
    #[serde(default)]
    pub os: String,
    #[serde(default)]
    pub sandbox: String,
}

/// One row of `GET /api/sessions`, as the server sends it. This mirrors
/// `SessionInfo` in `src/store.rs`, which only derives `Serialize`; the bridge
/// defines its own shape rather than touching the backend. Named
/// `SessionListRow`, not `SessionRow`: the plan's `SessionRow` is the
/// `.slint` view-model row (the Views lane declares it in `ui/views.slint`),
/// and the two types must never be confused.
#[derive(Debug, Clone, Deserialize)]
pub struct SessionListRow {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
    #[serde(default)]
    pub messages: i64,
}

/// One conversation, as returned by `GET /api/sessions/{id}`.
#[derive(Debug, Clone, Deserialize)]
pub struct SessionDetail {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub messages: Vec<Message>,
    #[serde(default)]
    pub busy: bool,
    #[serde(default)]
    pub interrupted: bool,
}

/// The HTTP client for the local server. Cheap to clone: the reqwest client and
/// the base URL are shared. Every request carries the per-launch token in the
/// `x-heron-token` header, exactly like the web UI did.
#[derive(Debug, Clone)]
pub struct HeronClient {
    http: reqwest::Client,
    base_url: String,
    token: String,
}

impl HeronClient {
    /// `base_url` is the server origin without a trailing slash, e.g.
    /// `http://127.0.0.1:54321`. `token` is the per-launch token from the launch URL.
    pub fn new(base_url: &str, token: &str) -> Self {
        HeronClient {
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            token: token.to_string(),
        }
    }

    /// Split a launch URL of the form `http://127.0.0.1:PORT/?token=...` into the
    /// origin and the token. Returns `None` when the shape is not recognised.
    pub fn from_launch_url(url: &str) -> Option<Self> {
        let (origin, query) = url.split_once('?')?;
        let token = query
            .split('&')
            .find_map(|pair| pair.strip_prefix("token="))?;
        if origin.is_empty() || token.is_empty() {
            return None;
        }
        Some(HeronClient::new(origin, token))
    }

    /// Generic JSON call: `GET /api/state`, `POST /api/mode` with a body, and so on.
    /// The path starts with `/`. A non-2xx response becomes an error carrying the
    /// server's `{"error": "..."}` message when one is present.
    pub async fn api(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value> {
        let mut req = self
            .http
            .request(method, format!("{}{path}", self.base_url))
            .header("x-heron-token", &self.token);
        if let Some(body) = body {
            req = req.json(&body);
        }
        let resp = req
            .send()
            .await
            .with_context(|| format!("could not reach the Heron server at {}", self.base_url))?;
        let status = resp.status();
        let value: Value = resp
            .json()
            .await
            .context("the Heron server answered with unreadable JSON")?;
        if status.is_success() {
            return Ok(value);
        }
        let message = value
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("the Heron server refused the request")
            .to_string();
        Err(anyhow!("{message} (HTTP {})", status.as_u16()))
    }

    /// `GET /api/state`.
    pub async fn state(&self) -> Result<AppState> {
        let value = self.api(Method::GET, "/api/state", None).await?;
        serde_json::from_value(value).context("the /api/state payload did not match AppState")
    }

    /// `GET /api/sessions`.
    pub async fn list_sessions(&self) -> Result<Vec<SessionListRow>> {
        let value = self.api(Method::GET, "/api/sessions", None).await?;
        serde_json::from_value(value)
            .context("the /api/sessions payload did not match a session list")
    }

    /// `POST /api/sessions`. Returns the new conversation id.
    pub async fn new_session(&self) -> Result<String> {
        let value = self.api(Method::POST, "/api/sessions", None).await?;
        value
            .get("id")
            .and_then(|id| id.as_str())
            .map(str::to_string)
            .ok_or_else(|| anyhow!("the /api/sessions reply had no id"))
    }

    /// `GET /api/sessions/{id}`.
    pub async fn get_session(&self, id: &str) -> Result<SessionDetail> {
        let value = self
            .api(Method::GET, &format!("/api/sessions/{id}"), None)
            .await?;
        serde_json::from_value(value)
            .context("the /api/sessions/{id} payload did not match SessionDetail")
    }

    /// `DELETE /api/sessions/{id}`. Fails with 409 while a reply is running there.
    pub async fn delete_session(&self, id: &str) -> Result<()> {
        self.api(Method::DELETE, &format!("/api/sessions/{id}"), None)
            .await?;
        Ok(())
    }

    /// `POST /api/chat` with `{session_id, message}`, streaming SSE events into `tx`.
    /// The stream ends when the server closes it (after `done`, `error`, or
    /// `cancelled`); the sender is dropped on return either way. See `sse.rs`
    /// for what counts as an event and what is skipped.
    pub async fn stream_chat(
        &self,
        session_id: Option<&str>,
        message: &str,
        tx: mpsc::UnboundedSender<ChatEvent>,
    ) -> Result<()> {
        let body = serde_json::json!({"session_id": session_id.unwrap_or(""), "message": message});
        let resp = self
            .http
            .request(Method::POST, format!("{}/api/chat", self.base_url))
            .header("x-heron-token", &self.token)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("could not reach the Heron server at {}", self.base_url))?;
        if resp.status() == StatusCode::CONFLICT {
            return Err(anyhow!(
                "Heron is still replying in this conversation. Stop it first, or wait."
            ));
        }
        if !resp.status().is_success() {
            let value: Value = resp.json().await.unwrap_or(Value::Null);
            let message = value
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("the chat request failed")
                .to_string();
            return Err(anyhow!("{message}"));
        }
        let mut parser = SseParser::new();
        let mut stream = resp.bytes_stream();
        let mut saw_terminal_event = false;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("the chat stream broke")?;
            for event in parser.feed(&chunk) {
                // A clean HTTP EOF is not necessarily a completed turn. Without
                // a terminal event, the UI would keep its busy flag indefinitely.
                if matches!(
                    event,
                    ChatEvent::Done { .. } | ChatEvent::Error { .. } | ChatEvent::Cancelled
                ) {
                    saw_terminal_event = true;
                }
                if tx.send(event).is_err() {
                    return Ok(()); // the UI went away; stop reading
                }
            }
        }
        if !saw_terminal_event {
            return Err(anyhow!(
                "the chat connection closed before Heron finished replying"
            ));
        }
        Ok(())
    }

    /// `POST /api/approve` with `{id, decision}`. Decision is one of
    /// `once`, `always`, `session`, or anything else to deny.
    pub async fn approve(&self, id: &str, decision: &str) -> Result<()> {
        self.api(
            Method::POST,
            "/api/approve",
            Some(serde_json::json!({"id": id, "decision": decision})),
        )
        .await?;
        Ok(())
    }

    /// `POST /api/cancel` with `{session_id}`. Also drops any waiting approval.
    pub async fn cancel(&self, session_id: &str) -> Result<()> {
        self.api(
            Method::POST,
            "/api/cancel",
            Some(serde_json::json!({"session_id": session_id})),
        )
        .await?;
        Ok(())
    }

    /// `POST /api/mode` with `{mode}`.
    pub async fn set_mode(&self, mode: &str) -> Result<String> {
        let value = self
            .api(
                Method::POST,
                "/api/mode",
                Some(serde_json::json!({"mode": mode})),
            )
            .await?;
        Ok(value
            .get("mode")
            .and_then(|m| m.as_str())
            .unwrap_or(mode)
            .to_string())
    }

    /// `POST /api/undo` with `{session_id}`. Returns the undone description, if any.
    pub async fn undo(&self, session_id: &str) -> Result<Option<String>> {
        let value = self
            .api(
                Method::POST,
                "/api/undo",
                Some(serde_json::json!({"session_id": session_id})),
            )
            .await?;
        Ok(value
            .get("undone")
            .and_then(|u| u.as_str())
            .map(str::to_string))
    }
}

/// The background runtime the UI uses for network work. One per desktop process,
/// created in the binary. `spawn` runs a future on it; the future must hand its
/// results to the UI thread with `Weak::upgrade_in_event_loop` before touching
/// any widget or model.
#[derive(Clone)]
pub struct Bridge {
    client: HeronClient,
    runtime: Arc<tokio::runtime::Runtime>,
}

impl Bridge {
    pub fn new(client: HeronClient) -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("heron-bridge")
            .enable_all()
            .build()
            .context("could not start the bridge runtime")?;
        Ok(Bridge {
            client,
            runtime: Arc::new(runtime),
        })
    }

    pub fn client(&self) -> &HeronClient {
        &self.client
    }

    pub fn spawn<F>(&self, fut: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.runtime.spawn(fut);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use heron::app::App;
    use heron::config::Config;
    use std::sync::Arc;

    fn demo_cfg(home: &std::path::Path) -> Config {
        let mut cfg = Config::default();
        cfg.home = home.to_path_buf();
        cfg.llm.base_url = "demo".into();
        cfg.llm.api_key_env = "HERON_TEST_NO_SUCH_KEY".into();
        cfg.agent.auto_memory = false;
        cfg
    }

    /// A real server on a random loopback port, backed by a temp home and the demo
    /// model, so chat turns complete with no provider and no network.
    struct DemoServer {
        _dir: tempfile::TempDir,
        origin: String,
        client: HeronClient,
        _handle: tokio::task::JoinHandle<anyhow::Result<()>>,
    }

    impl DemoServer {
        async fn start() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let cfg = demo_cfg(dir.path());
            let workspace = cfg.app_workspace();
            let app = Arc::new(App::build(cfg, workspace).await.unwrap());
            let (url, handle) = heron::server::start(app, "127.0.0.1", 0).await.unwrap();
            let origin = url
                .split('?')
                .next()
                .unwrap()
                .trim_end_matches('/')
                .to_string();
            let client =
                HeronClient::from_launch_url(&url).expect("the launch URL parses into a client");
            DemoServer {
                _dir: dir,
                origin,
                client,
                _handle: handle,
            }
        }
    }

    #[tokio::test]
    async fn incomplete_chat_stream_returns_error_instead_of_leaving_ui_busy() {
        // The HTTP transport can end cleanly without a Done/Error/Cancelled
        // event. That must be a stream failure, not a successful reply.
        let app = axum::Router::new().route(
            "/api/chat",
            axum::routing::post(|| async { concat!(r#"data: {"type":"text","text":"partial"}"#, "\n\n") }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test listener");
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("test server stopped");
        });

        let client = HeronClient::new(&origin, "test-token");
        let (tx, mut rx) = mpsc::unbounded_channel();
        let error = client.stream_chat(None, "hello", tx).await.unwrap_err();
        assert!(
            error.to_string().contains("closed before Heron finished"),
            "unexpected stream error: {error:#}"
        );
        assert!(
            matches!(rx.try_recv(), Ok(ChatEvent::Text { text }) if text == "partial"),
            "the UI still receives partial text before the truncated stream error"
        );
        server.abort();
    }

    #[test]
    fn launch_url_parsing() {
        assert!(
            HeronClient::from_launch_url("http://127.0.0.1:54321/?token=abc123").is_some(),
            "a well-formed launch URL parses"
        );
        assert!(
            HeronClient::from_launch_url("http://127.0.0.1:54321/").is_none(),
            "a URL with no token does not parse"
        );
        assert!(
            HeronClient::from_launch_url("http://127.0.0.1:54321/?token=").is_none(),
            "an empty token does not parse"
        );
        assert!(
            HeronClient::from_launch_url("not a url").is_none(),
            "a URL with no query does not parse"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn bridge_state_sessions_and_chat() {
        let server = DemoServer::start().await;
        let client = &server.client;

        // State: the server answers, and it knows it is in demo mode.
        let state = client.state().await.expect("GET /api/state works");
        assert!(!state.name.is_empty(), "the state names the agent");
        assert!(state.demo, "the temp server runs the demo model");
        assert_eq!(state.version, env!("CARGO_PKG_VERSION"));

        // A wrong token is refused, with the server's message, not a bare status.
        let bad_token = HeronClient::new(&server.origin, "wrong-token");
        assert!(bad_token.state().await.is_err(), "a bad token is refused");

        // Sessions: create, list, read, delete, in order.
        let id = client
            .new_session()
            .await
            .expect("POST /api/sessions works");
        assert_eq!(id.len(), 12, "session ids are 12 chars");
        let sessions = client
            .list_sessions()
            .await
            .expect("GET /api/sessions works");
        assert!(
            sessions.iter().any(|s| s.id == id),
            "the new session is listed"
        );
        let detail = client
            .get_session(&id)
            .await
            .expect("GET /api/sessions/{id} works");
        assert_eq!(detail.id, id);
        assert!(
            detail.messages.is_empty(),
            "a fresh session has no messages"
        );
        assert!(!detail.busy, "a fresh session is not busy");
        client
            .delete_session(&id)
            .await
            .expect("DELETE /api/sessions/{id} works");
        let sessions = client.list_sessions().await.unwrap();
        assert!(
            !sessions.iter().any(|s| s.id == id),
            "the deleted session is gone"
        );

        // Chat: a full turn streams events and ends with done.
        let (tx, mut rx) = mpsc::unbounded_channel::<ChatEvent>();
        let chat_client = client.clone();
        let stream = tokio::spawn(async move { chat_client.stream_chat(None, "hello", tx).await });
        let mut seen_session = false;
        let mut streamed = String::new();
        let mut done_text = String::new();
        let outcome = tokio::time::timeout(std::time::Duration::from_secs(60), async {
            while let Some(event) = rx.recv().await {
                match event {
                    ChatEvent::Session { .. } => seen_session = true,
                    ChatEvent::Text { text } => streamed.push_str(&text),
                    ChatEvent::Done { text, .. } => {
                        done_text = text;
                        break;
                    }
                    ChatEvent::Error { message } => panic!("the demo turn errored: {message}"),
                    ChatEvent::Cancelled => panic!("the demo turn was cancelled"),
                    _ => {}
                }
            }
        })
        .await;
        assert!(outcome.is_ok(), "the demo turn finished within 60s");
        stream.await.unwrap().expect("stream_chat returns Ok");
        assert!(seen_session, "the first event names the session");
        assert!(
            !done_text.is_empty(),
            "the turn ends with a non-empty reply"
        );
        if !streamed.is_empty() {
            assert_eq!(
                streamed, done_text,
                "streamed chunks add up to the final reply"
            );
        }

        // Cancel on an idle session is a no-op success, matching the web UI's stop button.
        assert!(
            client.cancel(&id).await.is_ok(),
            "cancel on an idle session succeeds"
        );
    }
}
