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

fn cfg_for(base: &str, home: &std::path::Path) -> Config {
    let mut cfg = Config::default();
    cfg.home = home.to_path_buf();
    cfg.llm.base_url = format!("{base}/v1");
    cfg.llm.api_key_env = "HERON_TEST_NO_SUCH_KEY".into();
    cfg.llm.max_retries = 2;
    cfg.agent.auto_memory = false;
    cfg
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
    let mut cfg = Config::default();
    cfg.home = dir.path().join("home");
    cfg.agent.auto_memory = false;
    let llm = Arc::new(ScriptedLlm::new(vec![
        ScriptedLlm::tool_call("write_file", json!({"path": "a.txt", "content": "one"})),
        Message::assistant("Wrote a.txt."),
        ScriptedLlm::tool_call("write_file", json!({"path": "b.txt", "content": "two"})),
        Message::assistant("I was not allowed to write b.txt."),
    ]));
    let app = Arc::new(App::assemble(cfg, dir.path().join("ws"), Store::in_memory().unwrap(), llm).await.unwrap());
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

    // Undo restores the pre-write state.
    let undo: Value = client.post(format!("{base}/api/undo")).header("x-heron-token", &token).json(&json!({"session_id": sid})).send().await.unwrap().json().await.unwrap();
    assert!(undo["undone"].as_str().unwrap().contains("removed"));
    assert!(!ws.join("a.txt").exists());

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
    let mut cfg = Config::default();
    cfg.home = dir.path().join("client-home");
    let ctx = ToolCtx { workspace: dir.path().to_path_buf(), memory: heron::memory::Memory::new(store.clone(), None), store, session_id: "t".into(), cfg: Arc::new(cfg), tainted: Default::default(), seen_urls: Default::default() };
    let remember = registry.get("mcp__other-heron__remember").unwrap();
    assert_eq!(remember.risk(&json!({}), &ctx), heron::permissions::Risk::Exec, "an unknown app's tool needs approval");
    assert!(remember.untrusted_output());
    let saved = remember.call(&json!({"content": "The user's dog is called Bean"}), &ctx).await.unwrap();
    assert!(saved.contains("remembered"), "{saved}");
    let recalled = registry.get("mcp__other-heron__recall").unwrap().call(&json!({"query": "dog name"}), &ctx).await.unwrap();
    assert!(recalled.contains("Bean"), "{recalled}");
    let err = remember.call(&json!({}), &ctx).await.unwrap_err();
    assert!(err.to_string().contains("content"), "tool errors come back as errors: {err}");

    // Same definitions as pinned: unchanged. A different pin (the server changed its tools): flagged, always asks.
    let mut again = ToolRegistry::new();
    assert!(!heron::mcp::attach(&mut again, "other-heron", &server, Some(&attached.fingerprint)).await.unwrap().changed);
    let mut changed = ToolRegistry::new();
    assert!(heron::mcp::attach(&mut changed, "other-heron", &server, Some("0000000000000000")).await.unwrap().changed);
    assert_eq!(changed.get("mcp__other-heron__remember").unwrap().risk(&json!({}), &ctx), heron::permissions::Risk::Danger);
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
    let mut cfg = Config::default();
    cfg.home = dir.path().join("home");
    cfg.agent.auto_memory = false;
    cfg.agent.ambient_context = false;
    let llm = Arc::new(ScriptedLlm::new(vec![ScriptedLlm::tool_call("write_file", json!({"path": "phone.txt", "content": "from my phone"})), Message::assistant("Saved it."), ScriptedLlm::tool_call("write_file", json!({"path": "second.txt", "content": "never written"})), Message::assistant("This reply must never be sent.")]));
    let app = Arc::new(App::assemble(cfg, dir.path().join("ws"), Store::in_memory().unwrap(), llm.clone()).await.unwrap());
    let code = heron::telegram::pairing_code(&app);
    tokio::spawn(heron::telegram::run(app.clone(), base, "TESTTOKEN".into()));

    let say = |chat: i64, text: &str, kind: &str| {
        let id = mock.next.fetch_add(1, Ordering::SeqCst) as i64 + 1;
        mock.updates.lock().unwrap().push_back(json!({"update_id": id, "message": {"chat": {"id": chat, "type": kind}, "text": text}}));
    };
    let wait_for = |what: &'static str, cond: Box<dyn Fn(&[(i64, String)]) -> bool + Send>| {
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
    say(42, "delete everything", "group");
    // The owner asks for something that changes a file: Heron asks first, on the phone.
    say(42, "save a note", "private");
    wait_for("permission question", Box::new(|s| s.iter().any(|(_, t)| t.contains("Reply yes or no")))).await;
    assert!(!app.workspace.join("phone.txt").exists(), "nothing is written before the owner answers");
    say(42, "yes", "private");
    wait_for("final reply", Box::new(|s| s.iter().any(|(c, t)| *c == 42 && t == "Saved it."))).await;
    assert_eq!(std::fs::read_to_string(app.workspace.join("phone.txt")).unwrap(), "from my phone");
    assert_eq!(llm.requests.lock().unwrap().len(), 2, "only the owner's one request reached the model");
    assert!(mock.sent.lock().unwrap().iter().all(|(c, _)| *c == 42));

    // Results of scheduled work are forwarded to the phone.
    app.store.add_inbox("Morning briefing", "Two tasks due today.").unwrap();
    wait_for("forwarded inbox item", Box::new(|s| s.iter().any(|(_, t)| t.contains("Morning briefing") && t.contains("Two tasks")))).await;

    // "/stop" while Heron is waiting for a yes or no halts the whole reply; it is not just a "no".
    say(42, "save another note", "private");
    wait_for("second permission question", Box::new(|s| s.iter().filter(|(_, t)| t.contains("Reply yes or no")).count() == 2)).await;
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
    let mut cfg = Config::default();
    cfg.home = dir.path().join("home");
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
    // Following a link that was on the page is just reading.
    assert!(session.messages[6].text().contains("Second page here"), "{}", session.messages[6].text());
    assert!(session.messages[6].text().contains(&format!("Address: {base}/second")));

    // With the user's go-ahead, the form can be filled in and used.
    let ctx = agent.tool_ctx(&session.id);
    let run = |name: &'static str, args: Value| {
        let (tools, ctx) = (app.tools.clone(), ctx.clone());
        async move { tools.get(name).unwrap().call(&args, &ctx).await }
    };
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
    let mut cfg = Config::default();
    cfg.home = dir.path().join("home");
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
    let requests = llm.requests.lock().unwrap();
    let last = requests.last().unwrap();
    assert_eq!(last.model.as_deref(), Some("small"), "even real work goes to the cheaper model now");
    assert_eq!(last.max_tokens, Some(2048));
    assert!(last.messages[0].text().contains("token allowance for this period is used up"), "the model is told to be brief");
    assert!(events.lock().unwrap().iter().any(|l| l.contains("Your token allowance is used up")));
    let calls_so_far = requests.len();
    drop(requests);
    // Learning is switched on here, so it is the allowance (not the setting) that stops it.
    let mut learning = (*app.cfg()).clone();
    learning.agent.auto_memory = true;
    app.update_config(learning).unwrap();
    app.llm.swap(llm.clone());
    let agent = app.agent(Arc::new(heron::permissions::DenyAll));
    assert!(agent.learn(&session, "I always take the train to work", "Noted").await.is_empty());
    assert_eq!(llm.requests.lock().unwrap().len(), calls_so_far, "no extra model call for learning when the allowance is spent");
}
