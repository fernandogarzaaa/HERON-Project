//! End-to-end tests over real sockets: the HTTP model client against an SSE server,
//! and the app's HTTP API (auth, chat stream, approvals, settings).

use axum::{routing::post, Json, Router};
use heron::app::App;
use heron::config::Config;
use heron::llm::{ChatRequest, HttpLlm, Llm, Message, ScriptedLlm};
use heron::store::Store;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

async fn spawn(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://{addr}")
}

fn sse(chunks: &[Value]) -> ([(&'static str, &'static str); 1], String) {
    let mut body: String = chunks.iter().map(|c| format!("data: {c}\n\n")).collect();
    body.push_str("data: [DONE]\n\n");
    ([("content-type", "text/event-stream")], body)
}

type SentCheck = Box<dyn Fn(&[(i64, String)]) -> bool + Send>;

fn cfg_for(base: &str, home: &std::path::Path) -> Config {
    let mut cfg = Config { home: home.to_path_buf(), ..Default::default() };
    cfg.llm.base_url = format!("{base}/v1");
    cfg.llm.api_key_env = "HERON_TEST_NO_SUCH_KEY".into();
    cfg.llm.max_retries = 2;
    cfg.agent.auto_memory = false;
    cfg
}

#[tokio::test]
async fn connector_control_plane_requires_local_auth_and_never_activates_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = cfg_for("demo", &dir.path().join("home"));
    cfg.llm.base_url = "demo".into();
    let app = Arc::new(App::build(cfg.clone(), dir.path().join("work")).await.unwrap());
    let base = spawn(heron::server::router(app.clone(), "connector-test-token".into())).await;
    let http = reqwest::Client::new();
    let endpoint = format!("{base}/api/connectors/custom");
    let spec = json!({"id":"test_remote", "name":"Test MCP", "category":"other",
        "transport":"remote_mcp", "endpoint":"https://example.org/mcp", "permission":"approval_required"});
    assert_eq!(http.post(&endpoint).json(&spec).send().await.unwrap().status(), 401);
    assert_eq!(http.post(&endpoint).bearer_auth("connector-test-token")
        .header("Origin", "https://evil.example.org").json(&spec).send().await.unwrap().status(), 403);
    assert_eq!(http.post(&endpoint).bearer_auth("connector-test-token")
        .header("Host", "evil.example.org").json(&spec).send().await.unwrap().status(), 403);
    assert!(!cfg.home.join("connectors.json").exists());

    let response = http.post(&endpoint).bearer_auth("connector-test-token")
        .json(&spec).send().await.unwrap();
    assert!(response.status().is_success());
    assert_eq!(response.json::<Value>().await.unwrap()["status"], "saved_not_active");
    let catalog: Value = http.get(format!("{base}/api/connectors"))
        .bearer_auth("connector-test-token").send().await.unwrap().json().await.unwrap();
    assert_eq!(catalog["custom_execution_enabled"], false);
    let card = catalog["connectors"].as_array().unwrap().iter()
        .find(|c| c["id"] == "test_remote").unwrap();
    assert_eq!(card["status"], "saved_not_active");
    assert_eq!(card["has_credential"], false);
    assert!(app.tools.names().iter().all(|name| !name.contains("test_remote")));

    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let response = http.post(format!("{endpoint}/test_remote/api-key"))
            .bearer_auth("connector-test-token").json(&json!({"api_key":"never-leak-this-test-key"}))
            .send().await.unwrap();
        assert!(!response.status().is_success());
        assert!(!response.text().await.unwrap().contains("never-leak-this-test-key"));
        assert!(!cfg.home.join("secrets.json").exists());
    }
    let audit = serde_json::to_string(&app.store.audit_log(100).unwrap()).unwrap();
    assert!(audit.contains("connector.register"));
    assert!(!audit.contains("never-leak-this-test-key"));
    // An unrelated damaged legacy file must not prevent emergency revocation.
    std::fs::write(cfg.home.join("secrets.json"), b"{corrupt").unwrap();
    assert!(http.delete(format!("{endpoint}/test_remote")).bearer_auth("connector-test-token")
        .send().await.unwrap().status().is_success());

    cfg.server.bind = "0.0.0.0".into();
    let remote = Arc::new(App::build(cfg, dir.path().join("other-work")).await.unwrap());
    let base = spawn(heron::server::router(remote, "remote-test-token".into())).await;
    assert_eq!(http.post(format!("{base}/api/connectors/custom"))
        .bearer_auth("remote-test-token").json(&spec).send().await.unwrap().status(), 403);
}

#[tokio::test]
async fn http_client_streams_retries_and_falls_back() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    let router = Router::new().route(
        "/v1/chat/completions",
        post(|Json(body): Json<Value>| async move {
            use axum::response::IntoResponse;
            let n = CALLS.fetch_add(1, Ordering::SeqCst);
            assert_eq!(body["stream"], true);
            if body["model"] == "broken" {
                return (axum::http::StatusCode::BAD_REQUEST, "model not found").into_response();
            }
            if n == 0 {
                return (axum::http::StatusCode::SERVICE_UNAVAILABLE, "overloaded").into_response(); // first attempt fails, retry succeeds
            }
            sse(&[
                json!({"choices": [{"delta": {"content": "Hel"}}]}),
                json!({"choices": [{"delta": {"content": "lo"}}]}),
                json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": "c1", "function": {"name": "list_dir", "arguments": "{\"path\""}}]}}]}),
                json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "function": {"arguments": ":\".\"}"}}]}}]}),
                json!({"choices": [], "usage": {"prompt_tokens": 11, "completion_tokens": 4}}),
            ])
            .into_response()
        }),
    );
    let base = spawn(router).await;
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = cfg_for(&base, dir.path());
    cfg.llm.model = "good".into();
    let llm = HttpLlm::from_config(&cfg).unwrap();
    let seen = std::sync::Mutex::new(String::new());
    let r = llm.chat(ChatRequest { messages: vec![Message::user("hi")], ..Default::default() }, &|t| seen.lock().unwrap().push_str(t)).await.unwrap();
    assert_eq!(*seen.lock().unwrap(), "Hello");
    assert_eq!(r.message.tool_calls[0].function.arguments, "{\"path\":\".\"}");
    assert_eq!(r.usage.prompt_tokens, 11);
    assert_eq!(CALLS.load(Ordering::SeqCst), 2, "one 503 then one success");

    // A model that fails with a non-retryable error falls through to the fallback model.
    cfg.llm.model = "broken".into();
    cfg.llm.fallback_models = vec!["good".into()];
    let llm = HttpLlm::from_config(&cfg).unwrap();
    let r = llm.chat(ChatRequest { messages: vec![Message::user("hi")], ..Default::default() }, &|_| {}).await.unwrap();
    assert_eq!(r.model, "good");

    // With no working model the error names the cause.
    cfg.llm.fallback_models.clear();
    let err = HttpLlm::from_config(&cfg).unwrap().chat(ChatRequest { messages: vec![Message::user("hi")], ..Default::default() }, &|_| {}).await.unwrap_err();
    assert!(err.to_string().contains("model not found"), "{err}");
}

#[tokio::test]
async fn auth_failures_stop_immediately_with_guidance() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    let router = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            CALLS.fetch_add(1, Ordering::SeqCst);
            (axum::http::StatusCode::UNAUTHORIZED, "bad key")
        }),
    );
    let base = spawn(router).await;
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = cfg_for(&base, dir.path());
    cfg.llm.fallback_models = vec!["other".into()];
    let err = HttpLlm::from_config(&cfg).unwrap().chat(ChatRequest { messages: vec![Message::user("hi")], ..Default::default() }, &|_| {}).await.unwrap_err();
    assert!(err.to_string().contains("API key"));
    assert_eq!(CALLS.load(Ordering::SeqCst), 1, "no retries or fallbacks on auth errors");
}

/// Read an SSE response into its JSON events, answering approval requests as they arrive.
async fn collect_events(client: &reqwest::Client, base: &str, token: &str, body: Value, approve_with: Option<&str>) -> Vec<Value> {
    use futures_util::StreamExt;
    let resp = client.post(format!("{base}/api/chat")).header("x-heron-token", token).json(&body).send().await.unwrap();
    assert!(resp.status().is_success(), "{}", resp.status());
    let mut events = vec![];
    let mut buf = String::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        buf.push_str(&String::from_utf8_lossy(&chunk.unwrap()));
        while let Some(i) = buf.find("\n\n") {
            let block: String = buf.drain(..i + 2).collect();
            for line in block.lines() {
                if let Some(data) = line.strip_prefix("data:") {
                    let ev: Value = serde_json::from_str(data.trim()).unwrap();
                    if ev["type"] == "approval" {
                        let decision = approve_with.expect("unexpected approval request");
                        let r = client.post(format!("{base}/api/approve")).header("x-heron-token", token).json(&json!({"id": ev["id"], "decision": decision})).send().await.unwrap();
                        assert!(r.status().is_success());
                    }
                    events.push(ev);
                }
            }
        }
    }
    events
}

#[tokio::test]
async fn http_api_chat_approval_and_settings() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config { home: dir.path().join("home"), ..Default::default() };
    cfg.llm.api_key_env = "HERON_TEST_NO_SUCH_KEY".into();
    cfg.agent.auto_memory = false;
    let llm = Arc::new(ScriptedLlm::new(vec![
        ScriptedLlm::tool_call("write_file", json!({"path": "a.txt", "content": "one"})),
        Message::assistant("Wrote a.txt."),
        ScriptedLlm::tool_call("write_file", json!({"path": "b.txt", "content": "two"})),
        Message::assistant("I was not allowed to write b.txt."),
    ]));
    let app = Arc::new(App::assemble(cfg, dir.path().join("ws"), Store::open(&dir.path().join("home/heron.db")).unwrap(), llm).await.unwrap());
    heron::risk_notice::accept_current(&app.store).unwrap();
    let ws = app.workspace.clone();
    let token = "test-token".to_string();
    let base = spawn(heron::server::router(app, token.clone())).await;
    let client = reqwest::Client::new();
    let get = |path: &str, tok: &str| client.get(format!("{base}{path}")).header("x-heron-token", tok.to_string()).send();

    // The page itself is public (it holds no data); every API route needs the token.
    let index = client.get(&base).send().await.unwrap();
    assert!(index.status().is_success());
    assert!(index.headers().contains_key("content-security-policy"));
    assert_eq!(get("/api/state", "").await.unwrap().status(), 401);
    assert_eq!(get("/api/state", "wrong").await.unwrap().status(), 401);
    assert_eq!(get("/api/memories", "").await.unwrap().status(), 401);
    let state: Value = get("/api/state", &token).await.unwrap().json().await.unwrap();
    assert_eq!(state["mode"], "ask");
    // Even with the right token, another website (or a rebound DNS name) is refused.
    let with = |name: &'static str, value: &'static str| client.get(format!("{base}/api/state")).header("x-heron-token", &token).header(name, value).send();
    assert_eq!(with("origin", "https://evil.example").await.unwrap().status(), 403);
    assert_eq!(with("host", "evil.example:7878").await.unwrap().status(), 403);
    let own_origin: &'static str = Box::leak(base.clone().into_boxed_str());
    assert_eq!(with("origin", own_origin).await.unwrap().status(), 200);

    // Turn 1: the write needs approval; we allow it.
    let events = collect_events(&client, &base, &token, json!({"message": "write a"}), Some("once")).await;
    let kinds: Vec<&str> = events.iter().map(|e| e["type"].as_str().unwrap()).collect();
    assert_eq!(kinds.first(), Some(&"session"));
    assert!(kinds.contains(&"approval") && kinds.contains(&"tool_start") && kinds.contains(&"tool_end"));
    assert_eq!(events.iter().find(|e| e["type"] == "done").unwrap()["text"], "Wrote a.txt.");
    assert_eq!(std::fs::read_to_string(ws.join("a.txt")).unwrap(), "one");
    let sid = events[0]["id"].as_str().unwrap().to_string();

    // Turn 2 in the same conversation: we decline; nothing is written.
    let events = collect_events(&client, &base, &token, json!({"message": "write b", "session_id": sid}), Some("deny")).await;
    assert!(events.iter().any(|e| e["type"] == "tool_end" && e["ok"] == false));
    assert!(!ws.join("b.txt").exists());

    // The conversation is stored and listed.
    let session: Value = get(&format!("/api/sessions/{sid}"), &token).await.unwrap().json().await.unwrap();
    assert_eq!(session["messages"].as_array().unwrap().len(), 8);
    assert_eq!(session["title"], "write a");

    // The file Heron wrote has been edited since: undo refuses, names the change it was about, and
    // "restore anyway" is accepted only for that change.
    std::fs::write(ws.join("a.txt"), "edited by hand afterwards").unwrap();
    let undo_with = |body: Value| client.post(format!("{base}/api/undo")).header("x-heron-token", &token).json(&body).send();
    let refused: Value = undo_with(json!({"session_id": sid})).await.unwrap().json().await.unwrap();
    assert!(refused["undone"].is_null() && refused["conflict"].as_str().unwrap().contains("changed since"), "{refused}");
    let asked_about = refused["checkpoint"].as_str().expect("the refusal names the change").to_string();
    assert_eq!(undo_with(json!({"session_id": sid, "force": true})).await.unwrap().status(), 400, "restoring anyway must say which change");
    let other: Value = undo_with(json!({"session_id": sid, "force": true, "checkpoint": "some-other-change"})).await.unwrap().json().await.unwrap();
    assert!(other["undone"].is_null() && other["stale"].is_string(), "{other}");
    assert_eq!(std::fs::read_to_string(ws.join("a.txt")).unwrap(), "edited by hand afterwards");
    // Confirmed for the right change: the file Heron created is removed, and the hand edit is kept beside it.
    let undo: Value = undo_with(json!({"session_id": sid, "force": true, "checkpoint": asked_about})).await.unwrap().json().await.unwrap();
    assert!(undo["undone"].as_str().unwrap().contains("removed"), "{undo}");
    assert!(!ws.join("a.txt").exists());
    let kept: Vec<_> = std::fs::read_dir(&ws).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().starts_with("a.txt.before-undo-")).collect();
    assert_eq!(kept.len(), 1);
    assert_eq!(std::fs::read_to_string(kept[0].path()).unwrap(), "edited by hand afterwards");

    // Settings: saving a key never echoes it back, and bad input is rejected with a clear message.
    let post = |path: &str, body: Value| client.post(format!("{base}{path}")).header("x-heron-token", &token).json(&body).send();
    let saved = post("/api/settings", json!({"model": "my-model", "api_key": "sk-secret-value-1234567890", "mode": "auto"})).await.unwrap();
    assert!(saved.status().is_success());
    let text = saved.text().await.unwrap();
    assert!(!text.contains("sk-secret"));
    let settings: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(settings["model"], "my-model");
    assert_eq!(settings["key_source"], "saved");
    assert_eq!(settings["mode"], "auto");
    assert!(std::fs::read_to_string(dir.path().join("home/config.toml")).unwrap().contains("my-model"));
    assert!(!std::fs::read_to_string(dir.path().join("home/config.toml")).unwrap().contains("sk-secret"));
    let bad = post("/api/settings", json!({"base_url": "not a url"})).await.unwrap();
    assert_eq!(bad.status(), 400);
    assert!(bad.json::<Value>().await.unwrap()["error"].as_str().unwrap().contains("not valid"));

    // Memory, tasks and reminders round-trip through the API.
    assert!(post("/api/memories", json!({"content": "The user plays the cello", "kind": "profile"})).await.unwrap().status().is_success());
    let found: Value = get("/api/memories?q=cello%20instrument", &token).await.unwrap().json().await.unwrap();
    assert_eq!(found.as_array().unwrap().len(), 1);
    // Undo and edit in the app send a forgotten row back with its source: an outside fact stays
    // unconfirmed instead of coming back as the user's own.
    assert!(post("/api/memories", json!({"content": "Invoices go to billing at northwind dot test", "kind": "fact", "source": "email"})).await.unwrap().status().is_success());
    let restored: Value = get("/api/memories?q=northwind%20invoices", &token).await.unwrap().json().await.unwrap();
    let row = restored.as_array().unwrap().iter().find(|m| m["content"].as_str().unwrap().contains("northwind")).expect("restored row");
    assert_eq!((row["source"].as_str(), row["confirmed"].as_bool()), (Some("email"), Some(false)), "{row}");
    assert_eq!(post("/api/memories", json!({"content": "Some odd source name", "source": "Not A Source!"})).await.unwrap().status(), 400);
    assert!(post("/api/tasks", json!({"title": "Tune the cello"})).await.unwrap().status().is_success());
    assert!(post("/api/schedules", json!({"prompt": "Practice reminder", "when": "daily 18:00"})).await.unwrap().status().is_success());
    let bad_when = post("/api/schedules", json!({"prompt": "x", "when": "sometime"})).await.unwrap();
    assert_eq!(bad_when.status(), 400);
    let schedules: Value = get("/api/schedules", &token).await.unwrap().json().await.unwrap();
    assert_eq!(schedules[0]["cron"], "0 18 * * *");
    let audit: Value = get("/api/audit", &token).await.unwrap().json().await.unwrap();
    assert_eq!(audit["entries"].as_array().unwrap().len(), 2);
    assert_eq!((audit["intact"].as_bool(), audit["checked"].as_u64()), (Some(true), Some(2)));

    // Personality: editable, validated, resettable.
    let persona: Value = get("/api/persona", &token).await.unwrap().json().await.unwrap();
    assert_eq!(persona["is_default"], true);
    let changed: Value = post("/api/persona", json!({"text": "# Who you are\n- Dry and brief. Call me Dana.", "name": "Juno"})).await.unwrap().json().await.unwrap();
    assert_eq!((changed["is_default"].as_bool(), changed["name"].as_str()), (Some(false), Some("Juno")));
    assert_eq!(post("/api/persona", json!({"text": "x"})).await.unwrap().status(), 400);
    let reset: Value = client.delete(format!("{base}/api/persona")).header("x-heron-token", &token).send().await.unwrap().json().await.unwrap();
    assert_eq!(reset["is_default"], true);

    // Per-tool settings: switching a tool off refuses it; the choice is saved in the settings file.
    let tools: Value = get("/api/tools", &token).await.unwrap().json().await.unwrap();
    assert!(tools.as_array().unwrap().iter().any(|t| t["name"] == "shell" && t["setting"] == "ask"));
    assert!(post("/api/tools/shell", json!({"setting": "never"})).await.unwrap().status().is_success());
    assert!(std::fs::read_to_string(dir.path().join("home/config.toml")).unwrap().contains("shell:*"));
    assert_eq!(post("/api/tools/shell", json!({"setting": "sometimes"})).await.unwrap().status(), 400);
    assert_eq!(post("/api/tools/nope", json!({"setting": "ask"})).await.unwrap().status(), 404);

    // Event-style reminders, the pause switch, and the Today summary.
    assert!(post("/api/schedules", json!({"prompt": "Note what changed", "when": "when notes/*.md changes", "quiet": true})).await.unwrap().status().is_success());
    let needs_mail = post("/api/schedules", json!({"prompt": "x", "when": "when an email arrives"})).await.unwrap();
    assert_eq!(needs_mail.status(), 400, "email triggers need a connected mailbox");
    let schedules: Value = get("/api/schedules", &token).await.unwrap().json().await.unwrap();
    assert!(schedules.as_array().unwrap().iter().any(|s| s["trigger"] == "When notes/*.md changes" && s["quiet"] == true));
    let paused: Value = post("/api/pause", json!({"paused": true})).await.unwrap().json().await.unwrap();
    assert_eq!(paused["paused"], true);
    let today: Value = get("/api/today", &token).await.unwrap().json().await.unwrap();
    assert_eq!((today["paused"].as_bool(), today["open_tasks"].as_u64(), today["waiting"].as_u64()), (Some(true), Some(1), Some(0)));

    // Saving a copy of the data while the app is running: a real, complete database lands in the backups folder.
    let saved: Value = post("/api/backup", json!({})).await.unwrap().json().await.unwrap();
    let copy = std::path::PathBuf::from(saved["path"].as_str().unwrap_or_else(|| panic!("{saved}")));
    assert!(copy.starts_with(dir.path().join("home/backups")), "{copy:?}");
    let restored_elsewhere = dir.path().join("elsewhere/heron.db");
    assert_eq!(heron::store::Store::restore(&restored_elsewhere, &copy).unwrap(), None);
    let from_copy = heron::store::Store::open(&restored_elsewhere).unwrap();
    assert_eq!(from_copy.list_schedules().unwrap().len(), schedules.as_array().unwrap().len(), "the copy holds what the app holds");
    assert!(from_copy.integrity().unwrap().is_empty());
    let no_token = client.post(format!("{base}/api/backup")).json(&json!({})).send().await.unwrap();
    assert!(!no_token.status().is_success(), "a copy cannot be requested without the app's token");
}

#[tokio::test]
async fn mcp_client_talks_to_a_real_server_and_detects_changed_tools() {
    use heron::config::McpServerConfig;
    use heron::tools::{ToolCtx, ToolRegistry};
    let dir = tempfile::tempdir().unwrap();
    let mut server = McpServerConfig { command: env!("CARGO_BIN_EXE_heron").to_string(), args: vec!["mcp-serve".into()], ..Default::default() };
    server.env.insert("HERON_HOME".into(), dir.path().join("server-home").to_string_lossy().to_string());
    server.env.insert("HERON_BASE_URL".into(), "demo".into());

    // First connection: tools are discovered and usable.
    let mut registry = ToolRegistry::new();
    let attached = heron::mcp::attach(&mut registry, "other-heron", &server, None).await.unwrap();
    assert_eq!(attached.tools, heron::mcp::SERVED_TOOLS.len());
    assert!(!attached.changed);
    let store = Store::in_memory().unwrap();
    let cfg = Config { home: dir.path().join("client-home"), ..Default::default() };
    let ctx = ToolCtx { workspace: dir.path().to_path_buf(), memory: heron::memory::Memory::new(store.clone(), None), store, session_id: "t".into(), cfg: Arc::new(cfg), tainted: Default::default(), seen_urls: Default::default() };
    let remember = registry.get("mcp__other-heron__remember").unwrap();
    assert_eq!(remember.risk(&json!({}), &ctx), heron::permissions::Effect::ExternalWrite, "an unknown app's tool is assumed able to change things elsewhere");
    assert!(remember.untrusted_output());
    // Q12: writes over MCP ask, and MCP cannot be asked, so `remember` is refused and nothing is stored.
    let refused = remember.call(&json!({"content": "The user's dog is called Bean"}), &ctx).await.unwrap_err();
    assert!(refused.to_string().contains("needs the user's approval"), "{refused}");
    let recalled = registry.get("mcp__other-heron__recall").unwrap().call(&json!({"query": "dog name"}), &ctx).await.unwrap();
    assert!(!recalled.contains("Bean"), "the refused write stored nothing: {recalled}");
    // A tool that is allowed still reports its own errors as errors (`remember` is refused before its arguments are read).
    let recall = registry.get("mcp__other-heron__recall").unwrap();
    let err = recall.call(&json!({}), &ctx).await.unwrap_err();
    assert!(err.to_string().contains("query"), "tool errors come back as errors: {err}");

    // Same definitions as pinned: unchanged. A different pin (the server changed its tools): flagged, always asks.
    let mut again = ToolRegistry::new();
    assert!(!heron::mcp::attach(&mut again, "other-heron", &server, Some(&attached.fingerprint)).await.unwrap().changed);
    let mut changed = ToolRegistry::new();
    assert!(heron::mcp::attach(&mut changed, "other-heron", &server, Some("0000000000000000")).await.unwrap().changed);
    assert_eq!(changed.get("mcp__other-heron__remember").unwrap().risk(&json!({}), &ctx), heron::permissions::Effect::Privileged);
}

#[tokio::test]
async fn a_rejected_bot_token_leaves_the_working_pairing_alone() {
    use axum::extract::State;
    use std::sync::Mutex;
    let checked = Arc::new(Mutex::new(Vec::<String>::new()));
    let telegram = Router::new()
        .fallback(|State(c): State<Arc<Mutex<Vec<String>>>>, uri: axum::http::Uri| async move {
            c.lock().unwrap().push(uri.path().to_string());
            (axum::http::StatusCode::UNAUTHORIZED, Json(json!({"ok": false, "error_code": 401, "description": "Unauthorized"})))
        })
        .with_state(checked.clone());
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config { home: dir.path().join("home"), ..Default::default() };
    cfg.llm.api_key_env = "HERON_TEST_NO_SUCH_KEY".into();
    cfg.telegram.api_base = spawn(telegram).await;
    let app = Arc::new(App::assemble(cfg, dir.path().join("ws"), Store::open(&dir.path().join("home/heron.db")).unwrap(), Arc::new(ScriptedLlm::new(vec![]))).await.unwrap());
    app.store.kv_set("telegram_chat", "42").unwrap();
    app.store.kv_set("telegram_offset", "7").unwrap();
    let token = "test-token".to_string();
    let base = spawn(heron::server::router(app.clone(), token.clone())).await;
    let bot_token = "123456789:AAEXAMPLEexampleEXAMPLEexample00";
    let resp = reqwest::Client::new().post(format!("{base}/api/telegram")).header("x-heron-token", &token).json(&json!({"token": bot_token})).send().await.unwrap();
    assert_eq!(resp.status(), 400);
    let body = resp.text().await.unwrap();
    assert!(body.contains("does not recognise"), "{body}");
    assert!(!body.contains("AAEXAMPLE"), "the token leaked into the reply: {body}");
    assert_eq!(checked.lock().unwrap().as_slice(), [format!("/bot{bot_token}/getMe")]);
    // Nothing changed: the phone stays paired, the stream position is kept, no token was stored.
    assert_eq!(app.store.kv_get("telegram_chat").as_deref(), Some("42"));
    assert_eq!(app.store.kv_get("telegram_offset").as_deref(), Some("7"));
    assert!(!app.cfg().vault_only_secret_present("telegram_token"));
}

#[tokio::test]
async fn telegram_gateway_pairs_one_chat_ignores_strangers_and_asks_before_acting() {
    use axum::extract::State;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    #[derive(Default)]
    struct Mock {
        updates: Mutex<VecDeque<Value>>,
        sent: Mutex<Vec<(i64, String)>>,
        next: AtomicUsize,
    }
    let mock = Arc::new(Mock::default());
    let router = Router::new()
        .route("/botTESTTOKEN/getUpdates", post(|State(m): State<Arc<Mock>>| async move { Json(json!({"ok": true, "result": m.updates.lock().unwrap().drain(..).collect::<Vec<_>>()})) }))
        .route(
            "/botTESTTOKEN/sendMessage",
            post(|State(m): State<Arc<Mock>>, Json(b): Json<Value>| async move {
                m.sent.lock().unwrap().push((b["chat_id"].as_i64().unwrap(), b["text"].as_str().unwrap().to_string()));
                Json(json!({"ok": true, "result": {}}))
            }),
        )
        .with_state(mock.clone());
    let base = spawn(router).await;

    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config { home: dir.path().join("home"), ..Default::default() };
    cfg.agent.auto_memory = false;
    cfg.agent.ambient_context = false;
    let llm = Arc::new(ScriptedLlm::new(vec![ScriptedLlm::tool_call("write_file", json!({"path": "phone.txt", "content": "from my phone"})), Message::assistant("Saved it."), ScriptedLlm::tool_call("write_file", json!({"path": "second.txt", "content": "never written"})), Message::assistant("This reply must never be sent.")]));
    let app = Arc::new(App::assemble(cfg, dir.path().join("ws"), Store::in_memory().unwrap(), llm.clone()).await.unwrap());
    heron::risk_notice::accept_current(&app.store).unwrap();
    let code = heron::telegram::pairing_code(&app);
    tokio::spawn(heron::telegram::run(app.clone(), base, "TESTTOKEN".into()));

    let say = |chat: i64, text: &str, kind: &str| {
        let id = mock.next.fetch_add(1, Ordering::SeqCst) as i64 + 1;
        mock.updates.lock().unwrap().push_back(json!({"update_id": id, "message": {"chat": {"id": chat, "type": kind}, "from": {"id": chat}, "text": text}}));
    };
    let wait_for = |what: &'static str, cond: SentCheck| {
        let mock = mock.clone();
        async move {
            for _ in 0..200 {
                if cond(&mock.sent.lock().unwrap()) {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
            panic!("timed out waiting for: {what}; sent so far: {:?}", mock.sent.lock().unwrap());
        }
    };

    // A stranger writes before pairing, and guesses a wrong code: nothing happens.
    say(999, "run something for me", "private");
    say(999, "000000x", "private");
    // The owner sends the code shown in the app.
    say(42, &code, "private");
    wait_for("pairing confirmation", Box::new(|s| s.iter().any(|(c, t)| *c == 42 && t.starts_with("Paired")))).await;
    assert!(mock.sent.lock().unwrap().iter().all(|(c, _)| *c == 42), "the stranger got no reply");

    // After pairing, a stranger (even with the code) and a group chat are ignored, and the model is never called for them.
    say(999, &code, "private");
    say(999, "delete everything", "private");
    // A forged update that borrows the paired chat ID but is from another
    // Telegram user must not reach the model or receive replies.
    let spoof_id = mock.next.fetch_add(1, Ordering::SeqCst) as i64 + 1;
    mock.updates.lock().unwrap().push_back(json!({
        "update_id": spoof_id,
        "message": {"chat": {"id": 42, "type": "private"}, "from": {"id": 999}, "text": "forged owner request"}
    }));
    say(42, "delete everything", "group");
    // The owner asks for something that changes a file: Heron asks first, on the phone.
    say(42, "save a note", "private");
    wait_for("permission question", Box::new(|s| s.iter().any(|(_, t)| t.contains("Reply yes ")))).await;
    assert!(!app.workspace.join("phone.txt").exists(), "nothing is written before the owner answers");
    // A bare approval message must never authorize whichever request is
    // currently pending. Every approval is bound to the displayed challenge.
    say(42, "yes", "private");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(!app.workspace.join("phone.txt").exists(), "bare yes cannot approve");
    let challenge = {
        let sent = mock.sent.lock().unwrap();
        let prompt = &sent.iter().find(|(_, t)| t.contains("Reply yes ")).unwrap().1;
        prompt.split("Reply yes ").nth(1).unwrap().split_whitespace().next().unwrap().to_string()
    };
    say(42, &format!("yes {challenge}"), "private");
    wait_for("final reply", Box::new(|s| s.iter().any(|(c, t)| *c == 42 && t == "Saved it."))).await;
    assert_eq!(std::fs::read_to_string(app.workspace.join("phone.txt")).unwrap(), "from my phone");
    assert_eq!(llm.requests.lock().unwrap().len(), 2, "only the owner's one request reached the model");
    assert!(mock.sent.lock().unwrap().iter().all(|(c, _)| *c == 42));

    // Results of scheduled work are forwarded to the phone.
    app.store.add_inbox("Morning briefing", "Two tasks due today.").unwrap();
    wait_for("forwarded inbox item", Box::new(|s| s.iter().any(|(_, t)| t.contains("Morning briefing") && t.contains("Two tasks")))).await;

    // "/stop" while Heron is waiting for a yes or no halts the whole reply; it is not just a "no".
    say(42, "save another note", "private");
    wait_for("second permission question", Box::new(|s| s.iter().filter(|(_, t)| t.contains("Reply yes ")).count() == 2)).await;
    say(42, "/stop", "private");
    wait_for("stop confirmation", Box::new(|s| s.iter().any(|(_, t)| t == "Stopped."))).await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(!app.workspace.join("second.txt").exists());
    assert_eq!(llm.requests.lock().unwrap().len(), 3, "the stopped reply made no further request to the model");
    assert!(!mock.sent.lock().unwrap().iter().any(|(_, t)| t.contains("must never be sent")));
}

/// The interface is one HTML file with inline script; a syntax error there breaks the whole app silently.
/// Where Node is available (development machines, CI), parse the script to catch that early.
#[test]
fn interface_script_parses() {
    let html = include_str!("../assets/index.html");
    let start = html.rfind("<script>").expect("script tag") + "<script>".len();
    let end = html.rfind("</script>").expect("closing script tag");
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("ui.js");
    std::fs::write(&file, &html[start..end]).unwrap();
    match std::process::Command::new("node").arg("--check").arg(&file).output() {
        Ok(out) => assert!(out.status.success(), "interface script has a syntax error:\n{}", String::from_utf8_lossy(&out.stderr)),
        Err(_) => eprintln!("node not found; skipping the interface syntax check"),
    }
}

/// Drives a real browser against a local site. Runs only when HERON_TEST_BROWSER is set: to "1" to use
/// whichever browser is installed, or to the path of one.
#[tokio::test(flavor = "multi_thread")]
async fn browser_reads_pages_follows_links_and_fills_in_forms() {
    let Ok(which) = std::env::var("HERON_TEST_BROWSER") else {
        eprintln!("HERON_TEST_BROWSER is not set; skipping the browser test");
        return;
    };
    if which != "1" {
        std::env::set_var("HERON_BROWSER", &which);
    }
    std::env::set_var("HERON_ALLOW_LOCAL_FETCH", "1"); // the test site is on this machine
    use axum::response::Html;
    use axum::routing::get;
    let first = r#"<!doctype html><title>Test site</title><h1>Welcome</h1><p id="js"></p>
        <a href="/second">Second page</a>
        <form onsubmit="return false"><input name="name" placeholder="Your name"><input type="password" name="pw" value="hunter2">
        <select name="when"><option>Morning</option><option>Evening</option></select>
        <button id="go" type="button">Greet</button></form><p id="out"></p>
        <input type="hidden" name="token" value="do-not-list"><button style="display:none">Hidden</button>
        <script>document.getElementById('js').textContent = 'Rendered by script';
        document.getElementById('go').onclick = () => { const f = document.forms[0]; document.getElementById('out').textContent = 'Good ' + f.when.value.toLowerCase() + ', ' + f.name.value; };</script>"#;
    let router = Router::new().route("/", get(move || async move { Html(first) })).route("/second", get(|| async { Html("<title>Second</title><p>Second page here</p><a href=\"/\">Back</a>") }));
    let base = spawn(router).await;

    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config { home: dir.path().join("home"), ..Default::default() };
    cfg.agent.auto_memory = false;
    cfg.agent.ambient_context = false;
    cfg.permissions.mode = "ask".into();
    assert!(heron::browser::find(&cfg).is_some(), "no browser found for the test");
    // The model's part: open the page, try to press a button, then follow a link.
    let llm = Arc::new(ScriptedLlm::new(vec![
        ScriptedLlm::tool_call("browser_open", json!({"url": format!("{base}/")})),
        ScriptedLlm::tool_call("browser_click", json!({"id": 5})),
        ScriptedLlm::tool_call("browser_click", json!({"id": 1})),
        Message::assistant("done"),
    ]));
    let app = Arc::new(App::assemble(cfg, dir.path().join("ws"), Store::in_memory().unwrap(), llm.clone()).await.unwrap());
    assert!(app.tools.names().iter().any(|n| n == "browser_open"), "browser tools are offered when a browser exists");
    // A turn does not start before the risk notice is accepted.
    heron::risk_notice::accept_current(&app.store).unwrap();

    // Nobody is there to approve anything.
    let agent = app.agent(Arc::new(heron::permissions::DenyAll));
    let mut session = heron::agent::Session::create(&app.store, "").unwrap();
    agent.run_turn(&mut session, &format!("look at {base}/"), &heron::agent::null_sink()).await.unwrap();
    let page = session.messages[2].text().to_string();
    assert!(page.contains("Rendered by script"), "scripts ran: {page}");
    assert!(page.contains(&format!("[1] link \"Second page\" -> {base}/second")), "{page}");
    assert!(page.contains("[2] field \"Your name\"") && page.contains("[3] password field") && page.contains("[4] choice") && page.contains("[5] button \"Greet\""), "{page}");
    assert!(!page.contains("hunter2") && !page.contains("do-not-list") && !page.contains("Hidden"), "passwords and hidden things are not shown: {page}");
    // Pressing a button acts on the site, and the page is outside content: not done without the user.
    assert!(session.messages[4].text().starts_with("Permission denied"), "{}", session.messages[4].text());
    // A link on the page leads where the page chose, not where the user asked to go, so following it
    // is not automatic either (the address the user typed was opened without asking).
    assert!(session.messages[6].text().starts_with("Permission denied"), "{}", session.messages[6].text());

    // With the user's go-ahead, the form can be filled in and used.
    let ctx = agent.tool_ctx(&session.id);
    let run = |name: &'static str, args: Value| {
        let (tools, ctx) = (app.tools.clone(), ctx.clone());
        async move { tools.get(name).unwrap().call(&args, &ctx).await }
    };
    run("browser_open", json!({"url": format!("{base}/")})).await.unwrap();
    // How each kind of click is classified, from the page as it stands.
    {
        use heron::permissions::Effect;
        let click = app.tools.get("browser_click").unwrap();
        assert_eq!(click.risk(&json!({"id": 1}), &ctx), Effect::NetworkEgress, "a link the page supplied");
        assert_eq!(click.risk(&json!({"id": 5}), &ctx), Effect::ExternalWrite, "a button");
        assert_eq!(app.tools.get("browser_type").unwrap().risk(&json!({"id": 2, "text": "x"}), &ctx), Effect::ExternalWrite);
        assert_eq!(app.tools.get("browser_open").unwrap().risk(&json!({"url": "https://made-up.example/x"}), &ctx), Effect::NetworkEgress, "an address the user never gave");
        assert_eq!(app.tools.get("web_search").unwrap().risk(&json!({"query": "x"}), &ctx), Effect::NetworkEgress);
    }
    let second = run("browser_click", json!({"id": 1})).await.unwrap();
    assert!(second.contains("Second page here") && second.contains(&format!("Address: {base}/second")), "{second}");
    run("browser_open", json!({"url": format!("{base}/")})).await.unwrap();
    let typed = run("browser_type", json!({"id": 2, "text": "Ada"})).await.unwrap();
    assert!(typed.contains("(now: Ada)"), "{typed}");
    let chosen = run("browser_type", json!({"id": 4, "text": "evening"})).await.unwrap();
    assert!(chosen.contains("[4] choice") && chosen.contains("(now: Evening)"), "{chosen}");
    assert!(run("browser_type", json!({"id": 4, "text": "Midnight"})).await.unwrap_err().to_string().contains("no such choice"));
    let greeted = run("browser_click", json!({"id": 5})).await.unwrap();
    assert!(greeted.contains("Good evening, Ada"), "{greeted}");
    let again = run("browser_read", json!({})).await.unwrap();
    assert!(again.contains("Good evening, Ada"));
    assert!(run("browser_click", json!({"id": 99})).await.is_err());
    // Addresses that are not web pages are refused.
    assert!(run("browser_open", json!({"url": "file:///etc/passwd"})).await.is_err());

    heron::browser::shutdown().await;
    assert!(dir.path().join("home/browser").is_dir(), "the browser used its own profile inside the Heron folder");
}

/// Token awareness end to end: calls are recorded, simple requests go to the cheaper model, work that
/// grows moves to the main model, and a nearly spent allowance changes how Heron behaves.
#[tokio::test]
async fn usage_is_recorded_and_shapes_model_choice_and_effort() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config { home: dir.path().join("home"), ..Default::default() };
    cfg.llm.model = "big".into();
    cfg.llm.fast_model = "small".into();
    cfg.agent.auto_memory = false;
    cfg.agent.ambient_context = false;
    cfg.permissions.mode = "auto".into();
    let llm = Arc::new(ScriptedLlm::new(vec![
        // 1: a greeting
        Message::assistant("Hello!"),
        // 2: looks simple, but turns into three steps of work
        ScriptedLlm::tool_call("task_list", json!({})),
        ScriptedLlm::tool_call("task_list", json!({})),
        ScriptedLlm::tool_call("task_list", json!({})),
        Message::assistant("You have nothing on your list."),
        // 3: plainly real work
        Message::assistant("Here is the plan."),
        // 4: after the allowance is spent
        Message::assistant("Short answer."),
    ]));
    let app = Arc::new(App::assemble(cfg, dir.path().join("ws"), Store::in_memory().unwrap(), llm.clone()).await.unwrap());
    heron::risk_notice::accept_current(&app.store).unwrap();
    let agent = app.agent(Arc::new(heron::permissions::DenyAll));
    let events = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let seen = events.clone();
    let sink: heron::agent::EventSink = Arc::new(move |e| match e {
        heron::agent::Event::Model { name, why } => seen.lock().unwrap().push(format!("model {name}: {why}")),
        heron::agent::Event::Notice(n) => seen.lock().unwrap().push(format!("notice {n}")),
        _ => {}
    });
    let mut session = heron::agent::Session::create(&app.store, "").unwrap();
    let models = |llm: &ScriptedLlm| llm.requests.lock().unwrap().iter().map(|r| r.model.clone().unwrap_or_else(|| "main".into())).collect::<Vec<_>>();

    agent.run_turn(&mut session, "hi there", &sink).await.unwrap();
    assert_eq!(models(&llm), vec!["small"]);
    agent.run_turn(&mut session, "what's on my list?", &sink).await.unwrap();
    assert_eq!(models(&llm)[1..], ["small", "small", "main", "main"], "two steps on the cheaper model, then the main one takes over");
    agent.run_turn(&mut session, "Research the best way to learn Rust in a month and write a plan", &sink).await.unwrap();
    assert_eq!(models(&llm).last().unwrap(), "main");
    {
        let e = events.lock().unwrap();
        assert!(e.iter().any(|l| l.starts_with("model small: a short, simple request")), "{e:?}");
        assert!(e.iter().any(|l| l.starts_with("model big: this turned out to need more work")), "{e:?}");
        assert!(!e.iter().any(|l| l.starts_with("notice")), "no economy notice while there is no limit: {e:?}");
    }

    // Every call was recorded (estimated here, since the stand-in model reports no usage).
    let report = heron::usage::report(&app.store, &app.cfg(), 0);
    assert!(report.budget.today.used > 0 && report.budget.month.used == report.budget.today.used);
    assert_eq!(report.models.iter().map(|m| m.3).sum::<u64>(), 6, "six model calls so far");
    assert_eq!(report.budget.level, heron::usage::Level::Normal);
    assert!(report.routing);

    // Set a daily limit below what has been used: Heron still answers, as cheaply as it can.
    let mut tight = (*app.cfg()).clone();
    tight.agent.daily_token_limit = 10;
    app.update_config(tight).unwrap();
    // update_config swaps in a client built from the settings; put the stand-in back for the test.
    app.llm.swap(llm.clone());
    let agent = app.agent(Arc::new(heron::permissions::DenyAll));
    agent.run_turn(&mut session, "Research the best way to learn Go and write a plan", &sink).await.unwrap();
    let calls_so_far = {
        let requests = llm.requests.lock().unwrap();
        let last = requests.last().unwrap();
        assert_eq!(last.model.as_deref(), Some("small"), "even real work goes to the cheaper model now");
        assert_eq!(last.max_tokens, Some(2048));
        assert!(last.messages[0].text().contains("token allowance for this period is used up"), "the model is told to be brief");
        requests.len()
    };
    assert!(events.lock().unwrap().iter().any(|l| l.contains("Your token allowance is used up")));
    // Learning is switched on here, so it is the allowance (not the setting) that stops it.
    let mut learning = (*app.cfg()).clone();
    learning.agent.auto_memory = true;
    app.update_config(learning).unwrap();
    app.llm.swap(llm.clone());
    let agent = app.agent(Arc::new(heron::permissions::DenyAll));
    assert!(agent.learn(&session, "I always take the train to work", "Noted").await.is_empty());
    assert_eq!(llm.requests.lock().unwrap().len(), calls_so_far, "no extra model call for learning when the allowance is spent");
}

// ---------- Ut: the event bus, runs that outlive their connection, and the real core ----------

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// A server-sent-event response, read one JSON event at a time.
struct Sse {
    resp: reqwest::Response,
    buf: Vec<u8>,
    ready: VecDeque<Value>,
}

impl Sse {
    async fn open(request: reqwest::RequestBuilder) -> Sse {
        let resp = request.send().await.unwrap();
        assert_eq!(resp.status(), 200, "the stream did not open");
        Sse { resp, buf: Vec::new(), ready: VecDeque::new() }
    }

    /// The next event within `wait` in all: `Err` on a timeout, `Ok(None)` once the stream has ended.
    /// Keep-alive comments arrive on their own schedule, so the limit is one deadline for the call.
    async fn next_within(&mut self, wait: Duration) -> Result<Option<Value>, tokio::time::error::Elapsed> {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            if let Some(v) = self.ready.pop_front() {
                return Ok(Some(v));
            }
            let Some(chunk) = tokio::time::timeout_at(deadline, self.resp.chunk()).await?.unwrap() else {
                return Ok(None);
            };
            self.buf.extend_from_slice(&chunk);
            while let Some(i) = self.buf.windows(2).position(|w| w == b"\n\n") {
                let block = String::from_utf8(self.buf.drain(..i + 2).collect()).unwrap();
                for line in block.lines() {
                    if let Some(data) = line.strip_prefix("data:") {
                        self.ready.push_back(serde_json::from_str(data.trim()).unwrap());
                    }
                }
            }
        }
    }

    async fn next(&mut self) -> Option<Value> {
        self.next_within(Duration::from_secs(20)).await.expect("the stream stalled for 20 s")
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

/// The core's HTTP API at one address, with its token.
struct Api {
    http: reqwest::Client,
    base: String,
    token: String,
}

impl Api {
    fn new(base: String, token: &str) -> Api {
        Api { http: reqwest::Client::new(), base, token: token.to_string() }
    }

    fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.http.get(format!("{}/api{path}", self.base)).header("x-heron-token", &self.token)
    }

    fn post(&self, path: &str) -> reqwest::RequestBuilder {
        self.http.post(format!("{}/api{path}", self.base)).header("x-heron-token", &self.token)
    }

    async fn json_of(response: reqwest::Response, what: &str) -> Value {
        let status = response.status();
        let text = response.text().await.unwrap();
        assert!(status.is_success(), "{what}: {status} {text}");
        serde_json::from_str(&text).unwrap_or_else(|_| panic!("{what}: not JSON: {text}"))
    }

    async fn get_json(&self, path: &str) -> Value {
        Api::json_of(self.get(path).send().await.unwrap(), &format!("GET {path}")).await
    }

    async fn post_json(&self, path: &str, body: Value) -> Value {
        Api::json_of(self.post(path).json(&body).send().await.unwrap(), &format!("POST {path}")).await
    }

    async fn status_of_post(&self, path: &str, body: Value) -> u16 {
        self.post(path).json(&body).send().await.unwrap().status().as_u16()
    }

    /// `POST /api/chat`: the stream of one run.
    async fn chat(&self, body: Value) -> Sse {
        Sse::open(self.post("/chat").json(&body)).await
    }

    /// `GET /api/events`: after `since` when given, new events only otherwise.
    async fn events(&self, since: Option<u64>) -> Sse {
        let path = match since {
            Some(s) => format!("/events?since={s}"),
            None => "/events".to_string(),
        };
        Sse::open(self.get(&path)).await
    }

    async fn approve(&self, id: &Value, decision: &str) {
        self.post_json("/approve", json!({"id": id, "decision": decision})).await;
    }
}

/// Serve an app on a real port with the token `test-token`.
async fn serve(app: Arc<App>) -> Api {
    let base = spawn(heron::server::router(app, "test-token".into())).await;
    Api::new(base, "test-token")
}

/// An app on a scripted model. The risk notice is not accepted: a test that chats accepts it itself.
async fn scripted_app(dir: &Path, script: Vec<Message>, tweak: impl FnOnce(&mut Config)) -> Arc<App> {
    let mut cfg = Config { home: dir.join("home"), ..Default::default() };
    cfg.llm.api_key_env = "HERON_TEST_NO_SUCH_KEY".into();
    cfg.agent.auto_memory = false;
    cfg.agent.ambient_context = false;
    tweak(&mut cfg);
    let store = Store::open(&dir.join("home/heron.db")).unwrap();
    Arc::new(App::assemble(cfg, dir.join("ws"), store, Arc::new(ScriptedLlm::new(script))).await.unwrap())
}

fn types(events: &[Value]) -> Vec<&str> {
    events.iter().map(|e| e["type"].as_str().unwrap_or("")).collect()
}

fn find<'a>(events: &'a [Value], kind: &str) -> &'a Value {
    events.iter().find(|e| e["type"] == kind).unwrap_or_else(|| panic!("no {kind} in {events:?}"))
}

fn seq_of(event: &Value) -> u64 {
    event["seq"].as_u64().unwrap_or_else(|| panic!("no seq in {event}"))
}

fn write_call(path: &str, content: &str) -> Message {
    ScriptedLlm::tool_call("write_file", json!({"path": path, "content": content}))
}

#[tokio::test]
async fn api_events_resume_from_sequence() {
    let dir = tempfile::tempdir().unwrap();
    let app = scripted_app(dir.path(), vec![Message::assistant("Hello there."), Message::assistant("Hello again.")], |_| {}).await;
    heron::risk_notice::accept_current(&app.store).unwrap();
    let api = serve(app).await;

    let run = api.chat(json!({"message": "hi"})).await.all().await;
    let seqs: Vec<u64> = run.iter().map(seq_of).collect();
    assert!(seqs.len() >= 3, "{run:?}");
    assert!(seqs.windows(2).all(|w| w[0] < w[1]), "seq rises along the run: {seqs:?}");
    assert_eq!((run[0]["type"].as_str(), types(&run).last().copied()), (Some("session"), Some("run_end")));

    // Resuming after the first event replays everything after it, in order, and nothing before it.
    let mut resumed = api.events(Some(seqs[0])).await;
    let tail = resumed.until("run_end").await;
    assert_eq!(tail.iter().map(seq_of).collect::<Vec<_>>(), seqs[1..], "{tail:?}");
    assert_eq!(types(&tail), types(&run)[1..]);
    assert!(tail.iter().all(|e| e["run_id"] == run[0]["run_id"] && e["session_id"] == run[0]["session_id"]), "{tail:?}");
    drop(resumed);

    // Resuming from the second to last event replays only the last one.
    let mut resumed = api.events(Some(seqs[seqs.len() - 2])).await;
    let last = resumed.next().await.unwrap();
    assert_eq!((last["type"].as_str(), seq_of(&last)), (Some("run_end"), *seqs.last().unwrap()));
    drop(resumed);

    // Resuming from the last event replays nothing; the next run's events then arrive on it, numbered on.
    let newest = *seqs.last().unwrap();
    let mut caught_up = api.events(Some(newest)).await;
    assert!(caught_up.next_within(Duration::from_millis(400)).await.is_err(), "nothing was replayed");
    let second = api.chat(json!({"message": "again"})).await.all().await;
    let first_live = caught_up.next().await.unwrap();
    assert_eq!((first_live["type"].as_str(), seq_of(&first_live)), (Some("session"), newest + 1));
    assert_eq!(seq_of(&second[0]), newest + 1);
    drop(caught_up);

    // Without `since` a client gets live events only, never the backlog.
    let mut live_only = api.events(None).await;
    assert!(live_only.next_within(Duration::from_millis(400)).await.is_err(), "no backlog without since");
    drop(live_only);

    // A `since` from a core that ran longer than this one, and one that is not a number.
    let mut ahead = api.events(Some(newest + 1000)).await;
    let told = ahead.all().await;
    assert_eq!(types(&told), ["resync"], "{told:?}");
    assert_eq!(api.get("/events?since=abc").send().await.unwrap().status(), 400);
}

#[tokio::test]
async fn api_run_survives_client_disconnect() {
    let dir = tempfile::tempdir().unwrap();
    let app = scripted_app(dir.path(), vec![write_call("kept.txt", "still here"), Message::assistant("Wrote it.")], |_| {}).await;
    heron::risk_notice::accept_current(&app.store).unwrap();
    let ws = app.workspace.clone();
    let api = serve(app).await;

    let mut chat = api.chat(json!({"message": "write kept.txt"})).await;
    let sid = chat.next().await.unwrap()["id"].as_str().unwrap().to_string();
    let approval = chat.until("approval").await.pop().unwrap();
    // The window closes while the reply waits for its answer.
    drop(chat);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!ws.join("kept.txt").exists(), "nothing is done before the answer");
    assert_eq!(api.get_json(&format!("/sessions/{sid}")).await["busy"], true, "the run goes on without its connection");

    // Another connection answers, and reads the rest of the run from the bus.
    api.approve(&approval["id"], "once").await;
    let mut events = api.events(Some(seq_of(&approval))).await;
    let rest = events.until("run_end").await;
    assert!(rest.iter().all(|e| e["run_id"] == approval["run_id"]), "{rest:?}");
    let resolved = find(&rest, "approval_resolved");
    assert_eq!((&resolved["id"], resolved["decision"].as_str()), (&approval["id"], Some("once")));
    assert_eq!(find(&rest, "done")["text"], "Wrote it.");
    assert_eq!(std::fs::read_to_string(ws.join("kept.txt")).unwrap(), "still here");
    assert_eq!(api.get_json(&format!("/sessions/{sid}")).await["busy"], false);
}

#[tokio::test]
async fn api_approval_waits_on_the_bus_then_parks_on_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let script = vec![write_call("answered.txt", "yes"), Message::assistant("First done."), write_call("parked.txt", "later"), Message::assistant("Carried on.")];
    let app = scripted_app(dir.path(), script, |c| c.server.approval_timeout_secs = 2).await;
    heron::risk_notice::accept_current(&app.store).unwrap();
    let ws = app.workspace.clone();
    let api = serve(app).await;

    // Answered in time over `POST /api/approve`: the answer goes on the bus and the action is done.
    let mut chat = api.chat(json!({"message": "first"})).await;
    let sid = chat.next().await.unwrap()["id"].as_str().unwrap().to_string();
    let approval = chat.until("approval").await.pop().unwrap();
    assert_eq!(approval["tool"], "write_file");
    api.approve(&approval["id"], "once").await;
    let rest = chat.all().await;
    let resolved = find(&rest, "approval_resolved");
    assert_eq!((&resolved["id"], resolved["decision"].as_str()), (&approval["id"], Some("once")));
    assert_eq!(find(&rest, "done")["text"], "First done.");
    assert_eq!(std::fs::read_to_string(ws.join("answered.txt")).unwrap(), "yes");

    // Nobody answers: after `approval_timeout_secs` it is parked, and the turn carries on without it.
    let started = Instant::now();
    let mut chat = api.chat(json!({"session_id": sid, "message": "second"})).await;
    let live = chat.until("approval").await.pop().unwrap();
    let rest = chat.all().await;
    assert!(started.elapsed() >= Duration::from_secs(2), "it waited for an answer first");
    assert!(!types(&rest).contains(&"approval_resolved"), "{rest:?}");
    let parked = find(&rest, "approval_parked");
    assert_eq!(parked["id"], live["id"]);
    let parked_id = parked["parked_id"].as_i64().unwrap();
    assert_eq!(find(&rest, "done")["text"], "Carried on.");
    assert_eq!(types(&rest).last(), Some(&"run_end"));
    assert!(!ws.join("parked.txt").exists());
    // The model was told, in so many words, to carry on without it.
    let session = api.get_json(&format!("/sessions/{sid}")).await;
    assert!(session["messages"].to_string().contains(heron::permissions::PARKED_FOR_LATER), "{session}");

    // It waits in the list of approvals, and its live id is never answered again.
    let waiting = api.get_json("/approvals").await;
    let waiting = waiting.as_array().unwrap();
    assert_eq!(waiting.len(), 1, "{waiting:?}");
    assert_eq!((waiting[0]["id"].as_i64(), waiting[0]["tool"].as_str(), waiting[0]["origin"].as_str()), (Some(parked_id), Some("write_file"), Some("user")));
    assert_eq!(api.status_of_post("/approve", json!({"id": live["id"], "decision": "once"})).await, 404);

    // The parked request is answered on its own route, and then the action is done.
    api.post_json(&format!("/approvals/{parked_id}"), json!({"action": "approve"})).await;
    assert_eq!(std::fs::read_to_string(ws.join("parked.txt")).unwrap(), "later");
    assert!(api.get_json("/approvals").await.as_array().unwrap().is_empty());
}

/// The real `heron serve --print-endpoint`, as the Mac app starts it: a home of its own, the demo
/// model, and a pipe on stdin that the app holds open.
struct Core {
    child: std::process::Child,
    stdin: Option<std::process::ChildStdin>,
    line: Value,
}

impl Core {
    fn start(home: &Path, workspace: &Path) -> Core {
        use std::io::BufRead;
        std::fs::create_dir_all(workspace).unwrap();
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_heron"))
            .args(["serve", "--print-endpoint"])
            .current_dir(workspace)
            .env("HERON_HOME", home)
            .env("HERON_BASE_URL", "demo")
            .env_remove("HERON_MODEL")
            .env_remove("HERON_PROVIDER")
            .env_remove("HERON_CLI_COMMAND")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(stdout);
            let mut line = String::new();
            let _ = reader.read_line(&mut line);
            let _ = tx.send(line);
            // Keep the pipe open to its end, so a later write by the core cannot meet a closed pipe.
            let _ = std::io::copy(&mut reader, &mut std::io::sink());
        });
        // From here `Drop` stops the core, even when an assertion fails.
        let mut core = Core { child, stdin, line: Value::Null };
        let line = rx.recv_timeout(Duration::from_secs(30)).expect("the core printed no endpoint line within 30 s");
        core.line = serde_json::from_str(line.trim()).unwrap_or_else(|_| panic!("the first line is not JSON: {line:?}"));
        core
    }

    fn api(&self) -> Api {
        Api::new(format!("http://127.0.0.1:{}", self.line["port"]), self.line["token"].as_str().unwrap())
    }

    /// What the kernel does when the app dies: the pipe's write end closes.
    fn close_stdin(&mut self) {
        self.stdin = None;
    }

    async fn exit_within(&mut self, within: Duration) -> Option<std::process::ExitStatus> {
        let end = Instant::now() + within;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return Some(status);
            }
            if Instant::now() >= end {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Every file under `dir`, as bytes.
fn files_under(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = vec![];
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(files_under(&path));
        } else if let Ok(bytes) = std::fs::read(&path) {
            out.push((path, bytes));
        }
    }
    out
}

#[tokio::test]
async fn api_print_endpoint_prints_port_and_token() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let mut core = Core::start(&home, &dir.path().join("ws"));

    let line = core.line.clone();
    let keys: BTreeSet<&str> = line.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, BTreeSet::from(["background_owner", "port", "token", "version"]), "{line}");
    let port = line["port"].as_u64().unwrap();
    assert!((1..=65535).contains(&port), "{line}");
    let token = line["token"].as_str().unwrap();
    assert_eq!(token.len(), 64, "{line}");
    assert!(token.bytes().all(|b| b.is_ascii_hexdigit()), "{line}");
    assert_eq!(line["background_owner"], true);
    assert_eq!(line["version"], env!("CARGO_PKG_VERSION"));

    // The token opens the port the line names, and only the token does.
    let api = core.api();
    let state = api.get_json("/state").await;
    assert_eq!((state["demo"].as_bool(), state["version"].as_str()), (Some(true), Some(env!("CARGO_PKG_VERSION"))), "{state}");
    assert_eq!(api.http.get(format!("{}/api/state", api.base)).send().await.unwrap().status(), 401);
    assert_eq!(api.http.get(format!("{}/api/state", api.base)).header("x-heron-token", "wrong").send().await.unwrap().status(), 401);

    // The endpoint goes through the pipe and nowhere else: no file in the home holds the token.
    for (path, bytes) in files_under(&home) {
        assert!(!bytes.windows(token.len()).any(|w| w == token.as_bytes()), "{} holds the token", path.display());
    }

    core.close_stdin();
    assert!(core.exit_within(Duration::from_secs(5)).await.is_some(), "the core stops once its stdin is closed");
}

#[tokio::test]
async fn core_exits_when_app_dies() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = Core::start(&dir.path().join("home"), &dir.path().join("ws"));
    let api = core.api();
    assert_eq!(api.get("/state").send().await.unwrap().status(), 200, "the core is serving");
    assert!(core.exit_within(Duration::from_millis(500)).await.is_none(), "it keeps running while the app's pipe is open");

    // The app dies: the kernel closes its end of the pipe. The core must follow within 5 seconds.
    let died = Instant::now();
    core.close_stdin();
    let status = core.exit_within(Duration::from_secs(5)).await.unwrap_or_else(|| panic!("the core was still running {:?} after its stdin closed", died.elapsed()));
    assert!(status.success(), "{status:?}");
    // Nothing is left listening on its port.
    assert!(api.get("/state").send().await.is_err(), "the port is closed");
}

#[tokio::test]
async fn mode_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let workspace = dir.path().join("ws");

    let mut first = Core::start(&home, &workspace);
    let api = first.api();
    assert_eq!(api.get_json("/state").await["mode"], "ask", "the default");
    assert_eq!(api.post_json("/mode", json!({"mode": "auto"})).await["mode"], "auto");
    assert_eq!(api.get_json("/state").await["mode"], "auto");
    first.close_stdin();
    assert!(first.exit_within(Duration::from_secs(5)).await.unwrap().success());

    // A new core on the same home starts in the mode the user chose.
    let mut second = Core::start(&home, &workspace);
    let api = second.api();
    assert_eq!(api.get_json("/state").await["mode"], "auto", "the mode survived the restart");
    // A mode that does not exist changes nothing, here or after another restart.
    assert_eq!(api.status_of_post("/mode", json!({"mode": "bogus"})).await, 400);
    assert_eq!(api.post_json("/mode", json!({"mode": "readonly"})).await["mode"], "readonly");
    second.close_stdin();
    assert!(second.exit_within(Duration::from_secs(5)).await.unwrap().success());
    let third = Core::start(&home, &workspace);
    assert_eq!(third.api().get_json("/state").await["mode"], "readonly");
}

#[tokio::test]
async fn risk_notice_blocks_chat_until_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let app = scripted_app(dir.path(), vec![Message::assistant("Hello.")], |_| {}).await;
    let store = app.store.clone();
    let api = serve(app).await;

    if heron::risk_notice::required() {
        // macOS (Q11): no turn runs, and no conversation is created, until the notice is accepted.
        let refused = api.post("/chat").json(&json!({"message": "hi"})).send().await.unwrap();
        assert_eq!(refused.status(), 428);
        assert_eq!(refused.json::<Value>().await.unwrap(), json!({"error": "risk_notice_not_accepted"}));
        assert!(api.get_json("/sessions").await.as_array().unwrap().is_empty(), "no session row was created");
        let state = api.get_json("/state").await;
        assert_eq!((state["risk_notice_required"].as_bool(), state["risk_notice_accepted"].as_bool()), (Some(true), Some(false)));

        // The notice is accepted over the API, and only in its current version.
        let notice = api.get_json("/risk-notice").await;
        let version = notice["version"].as_u64().unwrap();
        assert_eq!((notice["accepted_version"].clone(), notice["required"].as_bool()), (Value::Null, Some(true)));
        assert_eq!(notice["text"], heron::risk_notice::TEXT);
        assert_eq!(api.status_of_post("/risk-notice", json!({"version": version + 1})).await, 409);
        assert_eq!(api.status_of_post("/chat", json!({"message": "hi"})).await, 428, "a refused acceptance unblocks nothing");
        assert_eq!(api.status_of_post("/risk-notice", json!({"version": version})).await, 204);
        assert_eq!(api.get_json("/risk-notice").await["accepted_version"], version);
        assert_eq!(api.get_json("/state").await["risk_notice_accepted"], true);
        assert!(heron::risk_notice::accepted(&store), "the acceptance is stored");
    } else {
        // Elsewhere the notice is not required: chat runs without it.
        let state = api.get_json("/state").await;
        assert_eq!(state["risk_notice_required"], false);
        assert!(!heron::risk_notice::accepted(&store));
    }

    let events = api.chat(json!({"message": "hi"})).await.all().await;
    assert_eq!(events[0]["type"], "session");
    assert_eq!(find(&events, "done")["text"], "Hello.");
    assert_eq!(api.get_json("/sessions").await.as_array().unwrap().len(), 1);
}

// ---------- the recorded API fixtures (U4 decodes these) ----------

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/api");

/// The text with each temporary folder's path replaced, so a recording does not depend on the machine.
fn scrub(text: &str, folders: &[String]) -> String {
    let mut text = text.to_string();
    for folder in folders {
        text = text.replace(folder.as_str(), "/tmp/heron-fixture");
    }
    text
}

/// Every key path in `v`, arrays flattened, plus the paths whose value tells nothing about their
/// keys: `null` and an empty array.
fn key_paths(v: &Value, prefix: &str, all: &mut BTreeSet<String>, unknown: &mut BTreeSet<String>) {
    match v {
        Value::Object(map) => {
            for (key, value) in map {
                let path = format!("{prefix}.{key}");
                all.insert(path.clone());
                key_paths(value, &path, all, unknown);
            }
        }
        Value::Array(items) => {
            if items.is_empty() {
                unknown.insert(prefix.to_string());
            }
            for item in items {
                key_paths(item, &format!("{prefix}[]"), all, unknown);
            }
        }
        Value::Null => {
            unknown.insert(prefix.to_string());
        }
        _ => {}
    }
}

/// How the keys of `live` differ from those of `recorded`. Where either side has `null` or an empty
/// array, what lies below it is not compared. Values are never compared.
fn key_differences(recorded: &Value, live: &Value) -> Vec<String> {
    let (mut want, mut have, mut unknown) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    key_paths(recorded, "$", &mut want, &mut unknown);
    key_paths(live, "$", &mut have, &mut unknown);
    let below_unknown = |path: &String| unknown.iter().any(|u| path.len() > u.len() && path.starts_with(u.as_str()) && matches!(path.as_bytes()[u.len()], b'.' | b'['));
    let missing = want.difference(&have).filter(|p| !below_unknown(p)).map(|p| format!("the live core no longer has {p}"));
    let extra = have.difference(&want).filter(|p| !below_unknown(p)).map(|p| format!("the live core has {p}, which the recording lacks"));
    missing.chain(extra).collect()
}

fn fixture_path(name: &str) -> PathBuf {
    Path::new(FIXTURES).join(name)
}

/// Record `text` as the fixture when it is missing; otherwise hand back what was recorded.
fn recorded_or_write(name: &str, text: &str) -> Option<String> {
    let path = fixture_path(name);
    match std::fs::read_to_string(&path) {
        Ok(recorded) => Some(recorded),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, text).unwrap();
            None
        }
        Err(e) => panic!("cannot read {}: {e}", path.display()),
    }
}

/// One response the Mac app decodes: recorded when missing, otherwise checked for the same keys.
fn check_response_fixture(name: &str, live: &Value, folders: &[String]) {
    let live: Value = serde_json::from_str(&scrub(&live.to_string(), folders)).unwrap();
    let Some(recorded) = recorded_or_write(name, &format!("{}\n", serde_json::to_string_pretty(&live).unwrap())) else { return };
    let recorded: Value = serde_json::from_str(&recorded).unwrap_or_else(|e| panic!("{name} is not JSON: {e}"));
    let differences = key_differences(&recorded, &live);
    assert!(differences.is_empty(), "{name} no longer matches the live core (delete tests/fixtures/api/{name} to record it again, then update U4's models):\n{}", differences.join("\n"));
}

/// An event stream with `seq` and every id replaced by a fixed value, so the file is deterministic.
fn normalize_stream(events: &[Value], folders: &[String]) -> Vec<Value> {
    use std::collections::HashMap;
    let mut tool_ids: HashMap<String, String> = HashMap::new();
    let mut approval_ids: HashMap<String, String> = HashMap::new();
    let mut out = vec![];
    for (i, event) in events.iter().enumerate() {
        let mut event: Value = serde_json::from_str(&scrub(&event.to_string(), folders)).unwrap();
        let object = event.as_object_mut().unwrap();
        object.insert("seq".into(), json!(i + 1));
        object.insert("run_id".into(), json!("run-1"));
        object.insert("session_id".into(), json!("session-1"));
        let kind = object["type"].as_str().unwrap().to_string();
        let ids = match kind.as_str() {
            "session" => {
                object.insert("id".into(), json!("session-1"));
                None
            }
            "tool_start" | "tool_end" => Some((&mut tool_ids, "tool")),
            "approval" | "approval_resolved" | "approval_parked" => Some((&mut approval_ids, "approval")),
            _ => None,
        };
        if let Some((ids, prefix)) = ids {
            let old = object["id"].as_str().unwrap().to_string();
            let next = format!("{prefix}-{}", ids.len() + 1);
            let fixed = ids.entry(old).or_insert(next).clone();
            object.insert("id".into(), json!(fixed));
            if object.contains_key("parked_id") {
                object.insert("parked_id".into(), json!(1));
            }
        }
        out.push(event);
    }
    out
}

/// Each event type with the keys any event of that type has.
fn stream_shape(events: &[Value]) -> BTreeMap<String, BTreeSet<String>> {
    let mut shape: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for event in events {
        shape.entry(event["type"].as_str().unwrap().to_string()).or_default().extend(event.as_object().unwrap().keys().cloned());
    }
    shape
}

/// One recorded run: written as JSON lines when the file is missing, otherwise checked for the same
/// event types with the same keys. How many `text` events a reply streams in is not part of it.
fn check_stream_fixture(name: &str, live: &[Value]) {
    let lines: String = live.iter().map(|e| format!("{e}\n")).collect();
    let Some(recorded) = recorded_or_write(name, &lines) else { return };
    let recorded: Vec<Value> = recorded.lines().map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{name}: {l:?}: {e}"))).collect();
    assert!(!recorded.is_empty(), "{name} is empty");
    assert_eq!(stream_shape(&recorded), stream_shape(live), "{name} no longer matches the live core (delete tests/fixtures/api/{name} to record it again, then update U4's models)");
}

/// A model that fails its first call, which ends the turn with `error` and `run_end` (M16).
/// (`ScriptedLlm` has no failure mode, and nothing in the plan adds one, so the test brings its own.)
struct FailingLlm;

#[async_trait::async_trait]
impl Llm for FailingLlm {
    async fn chat(&self, _req: ChatRequest, _on_delta: heron::llm::DeltaFn<'_>) -> anyhow::Result<heron::llm::ChatResponse> {
        anyhow::bail!("the model is unavailable (scripted failure)")
    }
}

/// Start a run on `api` and read `/api/events` to its `run_end`, answering approvals with `once`.
async fn record_run(api: &Api, message: &str) -> Vec<Value> {
    let mut events = api.events(None).await;
    // Subscribed before the run starts; the chat response is not read, only kept open.
    let _chat = api.post("/chat").json(&json!({"message": message})).send().await.unwrap();
    let mut out = vec![];
    while let Some(event) = events.next().await {
        if event["type"] == "approval" {
            api.approve(&event["id"], "once").await;
        }
        let end = event["type"] == "run_end";
        out.push(event);
        if end {
            return out;
        }
    }
    panic!("the event stream ended before run_end: {out:?}");
}

/// The fixture comparison itself, which needs no core: what it catches and what it lets through.
#[test]
fn fixture_comparison_catches_key_drift_and_ignores_values() {
    let recorded = json!({"a": 1, "list": [{"x": 1}, {"y": 2}], "nested": {"k": "v"}, "maybe": null, "none": []});
    assert!(key_differences(&recorded, &recorded).is_empty());
    // Values, the order of items and which item has which key do not matter.
    let same_keys = json!({"a": "text", "list": [{"y": 9}, {"x": 8}], "nested": {"k": 3}, "maybe": null, "none": []});
    assert!(key_differences(&recorded, &same_keys).is_empty(), "{:?}", key_differences(&recorded, &same_keys));
    // Below a `null` or an empty array in the recording there is nothing to compare.
    let filled = json!({"a": 1, "list": [{"x": 1, "y": 1}], "nested": {"k": "v"}, "maybe": {"anything": true}, "none": [{"item": 1}]});
    assert!(key_differences(&recorded, &filled).is_empty(), "{:?}", key_differences(&recorded, &filled));
    // A key that went away, a key that appeared, and a key that moved into or out of an array's items.
    let removed = json!({"list": [{"x": 1}, {"y": 2}], "nested": {"k": "v"}, "maybe": null, "none": []});
    assert_eq!(key_differences(&recorded, &removed), ["the live core no longer has $.a"]);
    let added = json!({"a": 1, "extra": 1, "list": [{"x": 1}, {"y": 2}], "nested": {"k": "v"}, "maybe": null, "none": []});
    assert_eq!(key_differences(&recorded, &added), ["the live core has $.extra, which the recording lacks"]);
    let lost_item_key = json!({"a": 1, "list": [{"x": 1}], "nested": {"k": "v"}, "maybe": null, "none": []});
    assert_eq!(key_differences(&recorded, &lost_item_key), ["the live core no longer has $.list[].y"]);
    let renamed = json!({"a": 1, "list": [{"x": 1}, {"y": 2}], "nested": {"renamed": "v"}, "maybe": null, "none": []});
    assert_eq!(key_differences(&recorded, &renamed).len(), 2);
}

#[test]
fn recorded_streams_are_fixed_and_compared_by_event_type() {
    let tmp = vec!["/var/folders/x/T/tmp1".to_string()];
    let raw = vec![
        json!({"type": "session", "id": "real-sid", "run_id": "real-run", "session_id": "real-sid", "seq": 41}),
        json!({"type": "approval", "id": "uuid-a", "tool": "write_file", "summary": "write /var/folders/x/T/tmp1/ws/plan.md", "run_id": "real-run", "session_id": "real-sid", "seq": 42}),
        json!({"type": "approval_resolved", "id": "uuid-a", "decision": "once", "run_id": "real-run", "session_id": "real-sid", "seq": 43}),
        json!({"type": "tool_start", "id": "call_1", "name": "write_file", "run_id": "real-run", "session_id": "real-sid", "seq": 44}),
        json!({"type": "tool_end", "id": "call_1", "name": "write_file", "ok": true, "run_id": "real-run", "session_id": "real-sid", "seq": 45}),
        json!({"type": "run_end", "run_id": "real-run", "session_id": "real-sid", "seq": 46}),
    ];
    let fixed = normalize_stream(&raw, &tmp);
    assert_eq!(fixed.iter().map(seq_of).collect::<Vec<_>>(), [1, 2, 3, 4, 5, 6]);
    assert!(fixed.iter().all(|e| e["run_id"] == "run-1" && e["session_id"] == "session-1"), "{fixed:?}");
    assert_eq!(fixed[0]["id"], "session-1");
    assert_eq!((&fixed[1]["id"], &fixed[2]["id"]), (&json!("approval-1"), &json!("approval-1")), "one approval keeps one id");
    assert_eq!((&fixed[3]["id"], &fixed[4]["id"]), (&json!("tool-1"), &json!("tool-1")), "one tool call keeps one id");
    assert_eq!(fixed[1]["summary"], "write /tmp/heron-fixture/ws/plan.md", "the machine's folder is gone");
    // The same run again, with other ids, gives the same recording.
    let other: Vec<Value> = raw.iter().map(|e| serde_json::from_str(&e.to_string().replace("uuid-a", "uuid-b").replace("call_1", "call_2")).unwrap()).collect();
    assert_eq!(normalize_stream(&other, &tmp), fixed);
    // A stream is compared by its event types and their keys, so a differently chatty reply still matches.
    let mut chattier = fixed.clone();
    chattier.insert(1, json!({"type": "text", "text": "a", "seq": 2, "run_id": "run-1", "session_id": "session-1"}));
    chattier.insert(1, json!({"type": "text", "text": "b", "seq": 2, "run_id": "run-1", "session_id": "session-1"}));
    let mut with_text = fixed.clone();
    with_text.insert(1, json!({"type": "text", "text": "c", "seq": 2, "run_id": "run-1", "session_id": "session-1"}));
    assert_eq!(stream_shape(&chattier), stream_shape(&with_text));
    assert_ne!(stream_shape(&fixed), stream_shape(&with_text), "a new event type is a difference");
    let mut renamed_key = with_text.clone();
    renamed_key[1] = json!({"type": "text", "body": "c", "seq": 2, "run_id": "run-1", "session_id": "session-1"});
    assert_ne!(stream_shape(&renamed_key), stream_shape(&with_text), "a changed key is a difference");
}

#[tokio::test]
async fn api_fixtures_match_the_live_core() {
    let dir = tempfile::tempdir().unwrap();
    let mut folders = vec![dir.path().to_string_lossy().to_string(), dunce::canonicalize(dir.path()).unwrap().to_string_lossy().to_string()];
    folders.sort_by_key(|f| std::cmp::Reverse(f.len()));
    folders.dedup();
    let demo_config = |home: &str| {
        let mut cfg = Config { home: dir.path().join(home), ..Default::default() };
        cfg.llm.base_url = "demo".into();
        cfg.llm.api_key_env = "HERON_TEST_NO_SUCH_KEY".into();
        cfg.agent.auto_memory = false;
        cfg.agent.ambient_context = false;
        cfg
    };

    // A core on the demo model. Its demo tool call asks, and the turn is approved.
    let app = Arc::new(App::build(demo_config("home"), dir.path().join("ws")).await.unwrap());
    heron::risk_notice::accept_current(&app.store).unwrap();
    let api = serve(app.clone()).await;
    let approval_run = record_run(&api, "Save a plan as plan.md").await;
    let kinds = types(&approval_run);
    for wanted in ["session", "approval", "approval_resolved", "tool_start", "tool_end", "done", "run_end"] {
        assert!(kinds.contains(&wanted), "the demo turn has no {wanted}: {kinds:?}");
    }
    assert_eq!((kinds[0], *kinds.last().unwrap()), ("session", "run_end"));
    assert_eq!(find(&approval_run, "approval_resolved")["decision"], "once");
    let sid = approval_run[0]["id"].as_str().unwrap().to_string();

    // Something in every list the app shows: a waiting approval and a standing permission.
    app.store.park_approval(&sid, "write_file", r#"{"path":"later.txt","content":"later"}"#, "Write later.txt", "later.txt", heron::permissions::Effect::WorkspaceWrite.label(), false, "user").unwrap();
    app.policy.add_allow("shell:git *").unwrap();
    app.persist_rules();
    // And a "never" rule, on a tool the demo turn did not use.
    app.set_tool_setting("web_fetch", "never").unwrap();
    api.post_json("/memories", json!({"content": "The user plays the cello", "kind": "profile"})).await;

    let responses = [
        ("state.json", api.get_json("/state").await),
        ("sessions.json", api.get_json("/sessions").await),
        ("session.json", api.get_json(&format!("/sessions/{sid}")).await),
        ("approvals.json", api.get_json("/approvals").await),
        ("providers.json", api.get_json("/providers").await),
        ("grants.json", api.get_json("/grants").await),
        ("settings.json", api.get_json("/settings").await),
        ("risk-notice.json", api.get_json("/risk-notice").await),
        ("persona.json", api.get_json("/persona").await),
        ("today.json", api.get_json("/today").await),
        ("audit.json", api.get_json("/audit").await),
        ("nevers.json", api.get_json("/nevers").await),
    ];
    // A recording with an empty list would teach U4's decoder nothing about its items.
    for (name, list) in [("sessions.json", &responses[1].1), ("approvals.json", &responses[3].1), ("providers.json", &responses[4].1), ("grants.json", &responses[5].1), ("audit.json", &responses[10].1["entries"]), ("nevers.json", &responses[11].1)] {
        assert!(!list.as_array().unwrap().is_empty(), "{name} has no items to record");
    }
    // M9: the activity log says which model acted.
    let entry = &responses[10].1["entries"][0];
    assert!(entry.get("provider").is_some() && entry.get("cli_version").is_some(), "{entry}");
    for (name, live) in &responses {
        check_response_fixture(name, live, &folders);
    }
    check_stream_fixture("events-approval.jsonl", &normalize_stream(&approval_run, &folders));

    // A second core, whose model fails its first call: the turn ends with `error` and `run_end`.
    let failing_store = Store::open(&dir.path().join("failing-home/heron.db")).unwrap();
    let failing = Arc::new(App::assemble(demo_config("failing-home"), dir.path().join("failing-ws"), failing_store, Arc::new(FailingLlm)).await.unwrap());
    heron::risk_notice::accept_current(&failing.store).unwrap();
    let failing_api = serve(failing).await;
    let error_run = record_run(&failing_api, "Hello").await;
    let kinds = types(&error_run);
    assert_eq!((kinds[0], kinds[kinds.len() - 2], kinds[kinds.len() - 1]), ("session", "error", "run_end"), "{kinds:?}");
    let error = &error_run[error_run.len() - 2];
    assert!(error["message"].as_str().unwrap().contains("scripted failure"), "{error}");
    assert!(error.get("code").is_some(), "{error}");
    check_stream_fixture("events-error.jsonl", &normalize_stream(&error_run, &folders));
}
