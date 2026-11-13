//! Browser control: Heron drives a real browser that is already on the computer (Chrome, Edge,
//! Chromium or Brave) to read pages that need JavaScript and to click and fill in forms.
//!
//! How it is kept safe:
//! - The browser runs with its own profile in the Heron folder, never the user's own one, so it has
//!   none of their logins or cookies unless they sign in there themselves.
//! - Everything a page says is outside content, like a fetched page or an email.
//! - Opening an address and following a plain link are "reading" and follow the same rule as
//!   `web_fetch`: once outside content is in the conversation, only addresses that were already there
//!   may be opened without asking. Pressing a button or typing into a field acts on a website, so it is
//!   treated like running a command: asked about in "ask" mode, and always once outside content is present.
//! - Internal addresses are refused before navigating and again after any redirect; downloads are off.
//!
//! The browser is spoken to over the Chrome DevTools Protocol on a local WebSocket.

use crate::config::Config;
use crate::permissions::{check_url, url_host_obviously_local, Risk};
use crate::tools::{arg_str, opt_i64, schema, FnTool, ToolCtx};
use anyhow::{anyhow, bail, Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message as Frame;

/// Close the browser after this long without use.
const IDLE: Duration = Duration::from_secs(600);
const CALL_TIMEOUT: Duration = Duration::from_secs(25);
const MAX_ELEMENTS: usize = 80;

/// The browser program to use, if there is one. `agent.browser` (or HERON_BROWSER) may name a program,
/// or be "off" to switch browser control off.
pub fn find(cfg: &Config) -> Option<PathBuf> {
    let chosen = std::env::var("HERON_BROWSER").ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| cfg.agent.browser.clone());
    let chosen = chosen.trim();
    if chosen.eq_ignore_ascii_case("off") {
        return None;
    }
    if !chosen.is_empty() {
        let path = PathBuf::from(chosen);
        return if path.is_file() { Some(path) } else { on_path(chosen) };
    }
    let mut places: Vec<PathBuf> = vec![];
    if cfg!(target_os = "macos") {
        for app in ["Google Chrome", "Microsoft Edge", "Brave Browser", "Chromium"] {
            places.push(PathBuf::from(format!("/Applications/{app}.app/Contents/MacOS/{app}")));
            if let Some(home) = dirs::home_dir() {
                places.push(home.join(format!("Applications/{app}.app/Contents/MacOS/{app}")));
            }
        }
    } else if cfg!(windows) {
        for root in ["ProgramFiles", "ProgramFiles(x86)", "LocalAppData"].iter().filter_map(|v| std::env::var_os(v)) {
            for tail in [r"Google\Chrome\Application\chrome.exe", r"Microsoft\Edge\Application\msedge.exe", r"BraveSoftware\Brave-Browser\Application\brave.exe", r"Chromium\Application\chrome.exe"] {
                places.push(Path::new(&root).join(tail));
            }
        }
    } else {
        for name in ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser", "microsoft-edge", "microsoft-edge-stable", "brave-browser"] {
            if let Some(p) = on_path(name) {
                places.push(p);
            }
        }
    }
    places.into_iter().find(|p| p.is_file())
}

fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|dir| dir.join(name)).find(|p| p.is_file())
}

// ---------------- the connection ----------------

type Waiting = Arc<Mutex<HashMap<u64, oneshot::Sender<std::result::Result<Value, String>>>>>;

struct Conn {
    out: mpsc::UnboundedSender<String>,
    waiting: Waiting,
    next: AtomicU64,
    /// The page (tab) all commands go to, once attached.
    page: Mutex<String>,
}

impl Conn {
    async fn connect(ws_url: &str) -> Result<Arc<Conn>> {
        let (socket, _) = tokio::time::timeout(Duration::from_secs(10), tokio_tungstenite::connect_async(ws_url)).await.map_err(|_| anyhow!("timed out"))??;
        let (mut write, mut read) = socket.split();
        let (out, mut outbox) = mpsc::unbounded_channel::<String>();
        let waiting: Waiting = Default::default();
        tokio::spawn(async move {
            while let Some(text) = outbox.recv().await {
                if write.send(Frame::text(text)).await.is_err() {
                    break;
                }
            }
        });
        let (routes, replies) = (waiting.clone(), out.clone());
        tokio::spawn(async move {
            while let Some(Ok(frame)) = read.next().await {
                let Frame::Text(text) = frame else { continue };
                let Ok(v) = serde_json::from_str::<Value>(text.as_str()) else { continue };
                if let Some(id) = v.get("id").and_then(|i| i.as_u64()) {
                    if let Some(tx) = routes.lock().unwrap().remove(&id) {
                        let _ = tx.send(match v.get("error") {
                            Some(e) => Err(e.get("message").and_then(|m| m.as_str()).unwrap_or("browser error").to_string()),
                            None => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
                        });
                    }
                } else if v.get("method").and_then(|m| m.as_str()) == Some("Page.javascriptDialogOpening") {
                    // An alert or confirm box would freeze the page until answered. Decline it.
                    let mut dismiss = json!({"id": u64::MAX / 2, "method": "Page.handleJavaScriptDialog", "params": {"accept": false}});
                    if let Some(s) = v.get("sessionId") {
                        dismiss["sessionId"] = s.clone();
                    }
                    let _ = replies.send(dismiss.to_string());
                }
            }
            // The browser went away: nobody will answer what is still waiting.
            for (_, tx) in routes.lock().unwrap().drain() {
                let _ = tx.send(Err("the browser closed".into()));
            }
        });
        Ok(Arc::new(Conn { out, waiting, next: AtomicU64::new(1), page: Default::default() }))
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let mut msg = json!({"id": id, "method": method, "params": params});
        let page = self.page.lock().unwrap().clone();
        if !page.is_empty() && !method.starts_with("Target.") && !method.starts_with("Browser.") {
            msg["sessionId"] = Value::String(page);
        }
        let (tx, rx) = oneshot::channel();
        self.waiting.lock().unwrap().insert(id, tx);
        self.out.send(msg.to_string()).map_err(|_| anyhow!("the browser closed"))?;
        match tokio::time::timeout(CALL_TIMEOUT, rx).await {
            Ok(Ok(Ok(v))) => Ok(v),
            Ok(Ok(Err(e))) => bail!("{e}"),
            Ok(Err(_)) => bail!("the browser closed"),
            Err(_) => {
                self.waiting.lock().unwrap().remove(&id);
                bail!("the browser did not answer in time")
            }
        }
    }

    /// Run a script in the page and return its value.
    async fn eval(&self, script: &str) -> Result<Value> {
        let r = self.call("Runtime.evaluate", json!({"expression": script, "returnByValue": true, "awaitPromise": true})).await?;
        if let Some(problem) = r.get("exceptionDetails") {
            bail!("the page reported an error: {}", crate::util::preview(&problem["exception"]["description"].as_str().unwrap_or("script failed").to_string(), 200));
        }
        Ok(r["result"]["value"].clone())
    }
}

// ---------------- the running browser ----------------

#[derive(Clone, Debug)]
struct Element {
    id: i64,
    kind: String,
    label: String,
    href: String,
}

struct Live {
    conn: Arc<Conn>,
    /// The browser process, when this Heron started it (not when it found one already running).
    child: Option<tokio::process::Child>,
    used: Instant,
    opened: bool,
}

fn live() -> &'static tokio::sync::Mutex<Option<Live>> {
    static LIVE: OnceLock<tokio::sync::Mutex<Option<Live>>> = OnceLock::new();
    LIVE.get_or_init(Default::default)
}

/// What was on the page at the last look, for describing a click before it happens.
fn last_seen() -> &'static Mutex<Vec<Element>> {
    static SEEN: OnceLock<Mutex<Vec<Element>>> = OnceLock::new();
    SEEN.get_or_init(Default::default)
}

fn element(args: &Value) -> Option<Element> {
    let id = args.get("id").and_then(|i| i.as_i64())?;
    last_seen().lock().unwrap().iter().find(|e| e.id == id).cloned()
}

/// If this click is on a plain link, the address it leads to. Following a link is reading a page.
pub fn link_target(args: &Value) -> Option<String> {
    element(args).filter(|e| e.kind == "link" && (e.href.starts_with("http://") || e.href.starts_with("https://"))).map(|e| e.href)
}

fn profile_dir(cfg: &Config) -> PathBuf {
    cfg.home.join("browser")
}

/// Where a browser already using Heron's profile is listening, if one is.
fn listening(profile: &Path) -> Option<String> {
    let text = std::fs::read_to_string(profile.join("DevToolsActivePort")).ok()?;
    let mut lines = text.lines();
    let port: u16 = lines.next()?.trim().parse().ok()?;
    let path = lines.next()?.trim();
    Some(format!("ws://127.0.0.1:{port}{path}"))
}

async fn start(cfg: &Config) -> Result<Live> {
    let program = find(cfg).ok_or_else(|| anyhow!("no browser was found on this computer. Install Chrome, Edge, Chromium or Brave, or set `browser` under [agent] in the config file to the program's path"))?;
    let profile = profile_dir(cfg);
    std::fs::create_dir_all(&profile)?;
    // A browser from an earlier run may still be open with this profile (it does not allow a second
    // one). Use it rather than fail; if it has gone, its leftover note is removed.
    if let Some(url) = listening(&profile) {
        if let Ok(conn) = Conn::connect(&url).await {
            return attach(conn, None).await;
        }
        let _ = std::fs::remove_file(profile.join("DevToolsActivePort"));
    }
    let mut cmd = tokio::process::Command::new(&program);
    cmd.arg("--remote-debugging-port=0")
        .arg(format!("--user-data-dir={}", profile.display()))
        .args(["--no-first-run", "--no-default-browser-check", "--disable-sync", "--mute-audio", "--window-size=1280,900", "--disable-features=Translate,MediaRouter"]);
    if !cfg.agent.browser_visible {
        cmd.arg("--headless=new");
    }
    #[cfg(unix)]
    if is_root() {
        cmd.arg("--no-sandbox"); // the browser refuses to start as root otherwise
    }
    cmd.arg("about:blank").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).kill_on_drop(true);
    let mut child = cmd.spawn().with_context(|| format!("could not start {}", program.display()))?;
    let began = Instant::now();
    let url = loop {
        if let Some(url) = listening(&profile) {
            break url;
        }
        if let Ok(Some(status)) = child.try_wait() {
            bail!("the browser ({}) closed straight away ({status})", program.display());
        }
        if began.elapsed() > Duration::from_secs(30) {
            let _ = child.kill().await;
            bail!("the browser ({}) did not start in time", program.display());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    let conn = Conn::connect(&url).await.context("could not connect to the browser")?;
    attach(conn, Some(child)).await
}

#[cfg(unix)]
fn is_root() -> bool {
    std::process::Command::new("id").arg("-u").output().map(|o| String::from_utf8_lossy(&o.stdout).trim() == "0").unwrap_or(false)
}

async fn attach(conn: Arc<Conn>, child: Option<tokio::process::Child>) -> Result<Live> {
    let target = conn.call("Target.createTarget", json!({"url": "about:blank"})).await?;
    let target_id = target["targetId"].as_str().ok_or_else(|| anyhow!("the browser did not open a page"))?;
    let attached = conn.call("Target.attachToTarget", json!({"targetId": target_id, "flatten": true})).await?;
    *conn.page.lock().unwrap() = attached["sessionId"].as_str().unwrap_or("").to_string();
    conn.call("Page.enable", json!({})).await?;
    conn.call("Runtime.enable", json!({})).await?;
    // Nothing is saved to disk on a page's say-so.
    let _ = conn.call("Browser.setDownloadBehavior", json!({"behavior": "deny"})).await;
    Ok(Live { conn, child, used: Instant::now(), opened: false })
}

/// Close the browser (when Heron exits, or after it has been idle).
pub async fn shutdown() {
    if let Some(mut b) = live().lock().await.take() {
        let _ = tokio::time::timeout(Duration::from_secs(3), b.conn.call("Browser.close", json!({}))).await;
        if let Some(child) = b.child.as_mut() {
            let _ = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
            let _ = child.kill().await;
        }
    }
    last_seen().lock().unwrap().clear();
}

/// Stop the browser at once, from code that is not async (the desktop window closing).
pub fn kill_now() {
    if let Ok(mut guard) = live().try_lock() {
        if let Some(child) = guard.as_mut().and_then(|b| b.child.as_mut()) {
            let _ = child.start_kill();
        }
        *guard = None;
    }
}

async fn with_browser<T, F>(ctx: &ToolCtx, work: F) -> Result<T>
where
    F: for<'a> FnOnce(&'a mut Live) -> futures_util::future::BoxFuture<'a, Result<T>>,
{
    let mut guard = live().lock().await;
    if guard.is_none() {
        *guard = Some(start(&ctx.cfg).await?);
        // Do not leave a browser running for ever once it is no longer being used.
        tokio::spawn(async {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                let idle = match live().lock().await.as_ref() {
                    Some(b) => b.used.elapsed() > IDLE,
                    None => return,
                };
                if idle {
                    shutdown().await;
                    return;
                }
            }
        });
    }
    let b = guard.as_mut().unwrap();
    b.used = Instant::now();
    let out = work(b).await;
    if out.as_ref().err().map(|e| e.to_string().contains("the browser closed")).unwrap_or(false) {
        *guard = None; // start a fresh one next time
    }
    out
}

// ---------------- pages ----------------

fn local_allowed() -> bool {
    std::env::var("HERON_ALLOW_LOCAL_FETCH").is_ok()
}

async fn goto(b: &mut Live, url: &url::Url) -> Result<()> {
    let r = b.conn.call("Page.navigate", json!({"url": url.as_str()})).await?;
    if let Some(problem) = r.get("errorText").and_then(|e| e.as_str()).filter(|e| !e.is_empty()) {
        bail!("the page could not be opened ({problem})");
    }
    b.opened = true;
    settle(b).await
}

/// Wait for the page to finish loading, then make sure it did not end up somewhere it must not be.
async fn settle(b: &mut Live) -> Result<()> {
    tokio::time::sleep(Duration::from_millis(250)).await;
    let began = Instant::now();
    while began.elapsed() < Duration::from_secs(15) {
        match b.conn.eval("document.readyState").await {
            Ok(Value::String(state)) if state == "complete" => break,
            _ => tokio::time::sleep(Duration::from_millis(200)).await,
        }
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    let here = b.conn.eval("location.href").await.ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
    if let Ok(url) = url::Url::parse(&here) {
        if matches!(url.scheme(), "http" | "https") && !local_allowed() && url_host_obviously_local(&url) {
            let _ = b.conn.call("Page.navigate", json!({"url": "about:blank"})).await;
            bail!("the page led to a local/internal address ({}), so it was closed", url.host_str().unwrap_or(""));
        }
    }
    Ok(())
}

/// Number what can be clicked or filled in, and collect the page's text.
const LOOK: &str = r#"(() => {
  const shown = el => { const r = el.getBoundingClientRect(); const s = getComputedStyle(el); return r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && s.display !== 'none'; };
  document.querySelectorAll('[data-heron-id]').forEach(e => e.removeAttribute('data-heron-id'));
  const out = []; let n = 0, more = 0;
  for (const el of document.querySelectorAll('a[href], button, input, select, textarea, [role=button], [role=link], [contenteditable=true], summary')) {
    if (!shown(el) || el.disabled || (el.tagName === 'INPUT' && el.type === 'hidden')) continue;
    if (n >= LIMIT) { more++; continue; }
    n++; el.setAttribute('data-heron-id', n);
    const tag = el.tagName.toLowerCase();
    const kind = tag === 'a' ? 'link' : tag === 'input' ? (['submit', 'button', 'reset', 'image'].includes(el.type) ? 'button' : el.type === 'checkbox' || el.type === 'radio' ? el.type : 'field') : tag === 'textarea' ? 'field' : tag === 'select' ? 'choice' : el.isContentEditable ? 'field' : el.getAttribute('role') === 'link' ? 'button' : 'button';
    const secret = el.type === 'password';
    const text = (el.innerText || (kind === 'button' ? el.value : '') || el.getAttribute('aria-label') || el.placeholder || el.name || el.title || el.alt || '').trim().replace(/\s+/g, ' ').slice(0, 80);
    const value = secret ? '' : kind === 'field' ? (el.value || '').slice(0, 60) : kind === 'choice' ? ((el.selectedOptions[0] || {}).text || '') : (kind === 'checkbox' || kind === 'radio') ? (el.checked ? 'on' : 'off') : '';
    out.push({ id: n, kind: secret ? 'password field' : kind, label: text, href: tag === 'a' ? el.href : '', value });
  }
  return JSON.stringify({ title: document.title, url: location.href, text: document.body ? document.body.innerText : '', elements: out, more });
})()"#;

async fn look(b: &mut Live, max_chars: usize) -> Result<String> {
    let raw = b.conn.eval(&LOOK.replace("LIMIT", &MAX_ELEMENTS.to_string())).await?;
    let page: Value = serde_json::from_str(raw.as_str().unwrap_or("{}")).unwrap_or_default();
    let mut seen = vec![];
    let mut lines = vec![];
    for e in page["elements"].as_array().cloned().unwrap_or_default() {
        let el = Element { id: e["id"].as_i64().unwrap_or(0), kind: e["kind"].as_str().unwrap_or("").to_string(), label: e["label"].as_str().unwrap_or("").to_string(), href: e["href"].as_str().unwrap_or("").to_string() };
        let mut line = format!("[{}] {} \"{}\"", el.id, el.kind, el.label);
        if el.kind == "link" && !el.href.is_empty() {
            line.push_str(&format!(" -> {}", el.href));
        }
        if let Some(v) = e["value"].as_str().filter(|v| !v.is_empty()) {
            line.push_str(&format!(" (now: {v})"));
        }
        lines.push(line);
        seen.push(el);
    }
    *last_seen().lock().unwrap() = seen;
    let more = page["more"].as_i64().unwrap_or(0);
    let text = page["text"].as_str().unwrap_or("").trim();
    let text: String = text.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()).collect::<Vec<_>>().join("\n");
    Ok(format!(
        "Page: {}\nAddress: {}\n\n{}\n\nOn this page you can use (by number, with browser_click or browser_type):\n{}{}",
        page["title"].as_str().unwrap_or(""),
        page["url"].as_str().unwrap_or(""),
        if text.is_empty() { "(the page shows no text)".to_string() } else { crate::util::truncate_middle(&text, max_chars) },
        if lines.is_empty() { "(nothing)".to_string() } else { lines.join("\n") },
        if more > 0 { format!("\n({more} more are not listed)") } else { String::new() },
    ))
}

fn max_chars(args: &Value) -> usize {
    opt_i64(args, "max_chars").unwrap_or(6000).clamp(500, 40_000) as usize
}

async fn open(args: &Value, ctx: &ToolCtx) -> Result<String> {
    let url = check_url(arg_str(args, "url")?).await?;
    let limit = max_chars(args);
    with_browser(ctx, |b| {
        Box::pin(async move {
            goto(b, &url).await?;
            look(b, limit).await
        })
    })
    .await
}

async fn read(args: &Value, ctx: &ToolCtx) -> Result<String> {
    let limit = max_chars(args);
    with_browser(ctx, |b| {
        Box::pin(async move {
            if !b.opened {
                bail!("no page is open yet. Open one with browser_open first");
            }
            look(b, limit).await
        })
    })
    .await
}

fn find_script(id: i64, then: &str) -> String {
    format!("(() => {{ const el = document.querySelector('[data-heron-id=\"{id}\"]'); if (!el) return 'gone'; {then} }})()")
}

const GONE: &str = "that item is no longer on the page (the page has changed). Look at the page again with browser_read and use the new numbers";

async fn click(args: &Value, ctx: &ToolCtx) -> Result<String> {
    let id = args.get("id").and_then(|i| i.as_i64()).ok_or_else(|| anyhow!("'id' (the number shown next to the item) is required"))?;
    let limit = max_chars(args);
    // A plain link is followed by opening its address, so nothing the page attached to the click runs.
    let link = match link_target(args) {
        Some(href) => Some(check_url(&href).await?),
        None => None,
    };
    with_browser(ctx, |b| {
        Box::pin(async move {
            if !b.opened {
                bail!("no page is open yet. Open one with browser_open first");
            }
            if let Some(url) = link {
                goto(b, &url).await?;
                return look(b, limit).await;
            }
            let at = b.conn.eval(&find_script(id, "el.scrollIntoView({block: 'center', inline: 'center'}); const r = el.getBoundingClientRect(); return JSON.stringify({x: r.left + r.width / 2, y: r.top + r.height / 2});")).await?;
            let at = at.as_str().unwrap_or("gone");
            if at == "gone" {
                bail!("{GONE}");
            }
            let at: Value = serde_json::from_str(at)?;
            let (x, y) = (at["x"].as_f64().unwrap_or(0.0), at["y"].as_f64().unwrap_or(0.0));
            b.conn.call("Input.dispatchMouseEvent", json!({"type": "mouseMoved", "x": x, "y": y})).await?;
            b.conn.call("Input.dispatchMouseEvent", json!({"type": "mousePressed", "x": x, "y": y, "button": "left", "clickCount": 1})).await?;
            b.conn.call("Input.dispatchMouseEvent", json!({"type": "mouseReleased", "x": x, "y": y, "button": "left", "clickCount": 1})).await?;
            settle(b).await?;
            look(b, limit).await
        })
    })
    .await
}

async fn type_into(args: &Value, ctx: &ToolCtx) -> Result<String> {
    let id = args.get("id").and_then(|i| i.as_i64()).ok_or_else(|| anyhow!("'id' (the number shown next to the item) is required"))?;
    let text = arg_str(args, "text")?.to_string();
    let submit = crate::tools::opt_bool(args, "submit");
    let limit = max_chars(args);
    with_browser(ctx, |b| {
        Box::pin(async move {
            if !b.opened {
                bail!("no page is open yet. Open one with browser_open first");
            }
            let wanted = serde_json::to_string(&text)?;
            // A list of choices is set by picking the option with that text; anything else is typed into.
            let prepared = b
                .conn
                .eval(&find_script(
                    id,
                    &format!(
                        "el.scrollIntoView({{block: 'center'}}); if (el.tagName === 'SELECT') {{ const want = {wanted}.trim().toLowerCase(); const o = [...el.options].find(o => o.text.trim().toLowerCase() === want || o.value.toLowerCase() === want); if (!o) return 'no such choice: ' + [...el.options].map(o => o.text.trim()).join(', '); el.value = o.value; el.dispatchEvent(new Event('input', {{bubbles: true}})); el.dispatchEvent(new Event('change', {{bubbles: true}})); return 'chosen'; }} el.focus(); if (el.select) el.select(); return 'ready';"
                    ),
                ))
                .await?;
            match prepared.as_str().unwrap_or("gone") {
                "gone" => bail!("{GONE}"),
                "chosen" => {}
                "ready" => {
                    b.conn.call("Input.insertText", json!({"text": text})).await?;
                }
                other => bail!("{other}"),
            }
            if submit {
                let enter = json!({"key": "Enter", "code": "Enter", "windowsVirtualKeyCode": 13, "nativeVirtualKeyCode": 13});
                let mut down = enter.clone();
                down["type"] = "keyDown".into();
                down["text"] = "\r".into();
                b.conn.call("Input.dispatchKeyEvent", down).await?;
                let mut up = enter;
                up["type"] = "keyUp".into();
                b.conn.call("Input.dispatchKeyEvent", up).await?;
            }
            settle(b).await?;
            look(b, limit).await
        })
    })
    .await
}

fn describe(args: &Value) -> String {
    match element(args) {
        Some(e) => format!("{} \"{}\"", e.kind, e.label),
        None => format!("item {}", args.get("id").map(|i| i.to_string()).unwrap_or_default()),
    }
}

pub fn tools() -> Vec<FnTool> {
    let limit = json!({"type": "integer", "description": "How much of the page's text to return. Default 6000"});
    vec![
        FnTool {
            name: "browser_open",
            description: "Open a web address in a real browser and return what the page shows: its text, and a numbered list of the links, buttons and fields on it. Use this instead of web_fetch when a page needs JavaScript, or when you need to click or fill something in. The browser has its own profile with no logins unless the user signed in there.",
            parameters: schema(json!({"url": {"type": "string"}, "max_chars": limit.clone()}), &["url"]),
            summary: |a| a.get("url").and_then(|u| u.as_str()).unwrap_or("").to_string(),
            risk: |_, _| Risk::Network,
            detail: None,
            untrusted: true,
            run: |a, c| Box::pin(open(a, c)),
        },
        FnTool {
            name: "browser_read",
            description: "Look again at the page that is open in the browser (for example after it has changed) and get fresh numbers for what is on it.",
            parameters: schema(json!({"max_chars": limit.clone()}), &[]),
            summary: |_| String::new(),
            risk: |_, _| Risk::Safe,
            detail: None,
            untrusted: true,
            run: |a, c| Box::pin(read(a, c)),
        },
        FnTool {
            name: "browser_click",
            description: "Click an item on the open page by its number from the latest browser_open or browser_read. Returns the page as it is afterwards.",
            parameters: schema(json!({"id": {"type": "integer"}, "max_chars": limit.clone()}), &["id"]),
            summary: |a| format!("click {}", describe(a)),
            // Following a link is reading. Pressing anything else may send, buy or change something.
            risk: |a, _| if link_target(a).is_some() { Risk::Network } else { Risk::Exec },
            detail: Some(|a, _| match link_target(a) {
                Some(href) => format!("Follow the link {} to {href}", describe(a)),
                None => format!("Click the {} on the page open in the browser", describe(a)),
            }),
            untrusted: true,
            run: |a, c| Box::pin(click(a, c)),
        },
        FnTool {
            name: "browser_type",
            description: "Type text into a field on the open page (or pick an option in a list of choices) by its number. Set submit to true to press Enter afterwards. Returns the page as it is afterwards.",
            parameters: schema(json!({"id": {"type": "integer"}, "text": {"type": "string"}, "submit": {"type": "boolean"}, "max_chars": limit}), &["id", "text"]),
            summary: |a| format!("type \"{}\" into {}{}", crate::util::preview(a.get("text").and_then(|t| t.as_str()).unwrap_or(""), 80), describe(a), if a.get("submit").and_then(|s| s.as_bool()).unwrap_or(false) { " and press Enter" } else { "" }),
            risk: |_, _| Risk::Exec,
            detail: Some(|a, _| format!("Type this into the {} on the page open in the browser{}:\n\n{}", describe(a), if a.get("submit").and_then(|s| s.as_bool()).unwrap_or(false) { ", then press Enter" } else { "" }, a.get("text").and_then(|t| t.as_str()).unwrap_or(""))),
            untrusted: true,
            run: |a, c| Box::pin(type_into(a, c)),
        },
    ]
}
