//! Telegram gateway: talk to Heron from your phone.
//!
//! Safety model: the bot answers exactly one chat — the one that proved ownership by sending the
//! pairing code shown in the app. Everyone else is ignored. Approvals for file changes and commands
//! can be given by replying yes/no; risky actions, and anything requested after outside content
//! entered the conversation, can only be approved at the computer. `/stop` halts a running reply.

use crate::agent::{null_sink, Session};
use crate::app::App;
use crate::channel::{ChannelIdentity, ChannelKind, ChannelStore, InboundMessage, RetryOutcome};
use crate::permissions::{Approval, ApprovalRequest, Approver};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;

const MAX_MESSAGE: usize = 3800;
/// Shown in the app only: forwarding it to the chat that just failed would fail again and
/// raise the next notice.
const UNDELIVERED_TITLE: &str = "A Telegram reply was not delivered";

#[derive(Debug, PartialEq)]
enum Delivery {
    Sent(Option<String>),
    /// Network trouble, a server error or a rate limit; `after` is Telegram's retry_after in seconds.
    Retry { after: Option<i64> },
    /// Telegram rejected the message itself (a 4xx other than 429); sending it again cannot help.
    Refused,
}

#[derive(Clone)]
struct Bot {
    client: reqwest::Client,
    base: String,
    app: Arc<App>,
    account_id: String,
    // Snapshot of the locally displayed pairing epoch. Revocation rotates it,
    // preventing an in-flight old agent from replying after re-pairing.
    pairing_epoch: String,
    spool: ChannelStore,
    dispatch_lock: Arc<tokio::sync::Mutex<()>>,
}

impl Bot {
    async fn call(&self, method: &str, body: Value) -> Result<Value> {
        let resp = self.client.post(format!("{}/{method}", self.base)).json(&body).send().await?;
        let v: Value = resp.json().await?;
        if v["ok"].as_bool() != Some(true) {
            return Err(anyhow!("Telegram refused {method}: {}", v["description"].as_str().unwrap_or("unknown error")));
        }
        Ok(v["result"].clone())
    }

    /// One sendMessage attempt, classified by what should happen to the queued row.
    async fn send_message(&self, chat: i64, text: &str) -> Delivery {
        let sent = self.client.post(format!("{}/sendMessage", self.base)).json(&json!({"chat_id": chat, "text": text})).send().await;
        let Ok(resp) = sent else { return Delivery::Retry { after: None } };
        let status = resp.status();
        let Ok(v) = resp.json::<Value>().await else { return Delivery::Retry { after: None } };
        if v["ok"].as_bool() == Some(true) {
            return Delivery::Sent(v["result"]["message_id"].as_i64().map(|id| id.to_string()));
        }
        let code = v["error_code"].as_u64().map(|c| c as u16).unwrap_or(status.as_u16());
        match code {
            429 => Delivery::Retry { after: v["parameters"]["retry_after"].as_i64() },
            400..=499 => Delivery::Refused,
            _ => Delivery::Retry { after: None },
        }
    }

    /// Each platform message must fit Telegram's UTF-16 length limit.
    fn chunks(text: &str) -> Vec<String> {
        let mut parts = Vec::new();
        let mut current = String::new();
        let mut units = 0;
        for ch in text.chars() {
            let n = ch.len_utf16();
            if units + n > MAX_MESSAGE {
                parts.push(std::mem::take(&mut current));
                units = 0;
            }
            current.push(ch);
            units += n;
        }
        if !current.is_empty() {
            parts.push(current);
        }
        parts
    }

    fn identity(&self, chat: i64) -> ChannelIdentity {
        ChannelIdentity {
            kind: ChannelKind::Telegram,
            account_id: self.account_id.clone(),
            chat_id: chat.to_string(),
            sender_id: chat.to_string(),
        }
    }

    /// Outbound rows are scoped to the pairing epoch as well as the bot token.
    /// A detached turn from an earlier pairing can never become sendable again
    /// merely because the same chat pairs with the bot a second time.
    fn outbox_account_id(&self) -> String {
        format!("{}:{}", self.account_id, self.pairing_epoch)
    }

    fn outbox_identity(&self, chat: i64) -> ChannelIdentity {
        let mut identity = self.identity(chat);
        identity.account_id = self.outbox_account_id();
        identity
    }

    /// Current pairing gate, including the specific generation captured by
    /// this Bot. Detached agent turns cannot send to a revoked owner.
    fn is_current_owner(&self, chat: i64) -> bool {
        paired_chat(&self.app) == Some(chat)
            && self.app.store.kv_get("telegram_pairing_epoch").as_deref()
                == Some(self.pairing_epoch.as_str())
    }

    /// Fail-closed on local persistence errors: do not send a message that
    /// cannot be recovered if HERON stops before delivery completes.
    async fn queue_with_key(&self, chat: i64, text: &str, key: &str) -> Result<()> {
        if !self.is_current_owner(chat) {
            return Err(anyhow!("The chat was unpaired or pairing was rotated; refusing to queue a reply"));
        }
        let identity = self.outbox_identity(chat);
        let parts = Self::chunks(text);
        self.spool.enqueue_parts(&identity, None, &parts, key, crate::util::now_ts())?;
        // Close the check/enqueue race with unpair or re-pair. If the epoch
        // changed while SQLite was committing, discard this old-epoch queue.
        if !self.is_current_owner(chat) {
            let _ = self.spool.purge_account(ChannelKind::Telegram, &identity.account_id);
            return Err(anyhow!("Telegram pairing changed while the reply was being queued."));
        }
        // Delivery is handled by the gateway's independent outbox worker.
        // Do not let a slow Telegram request block the caller or inbound polling.
        Ok(())
    }

    async fn queue(&self, chat: i64, text: &str) -> Result<()> {
        let key = uuid::Uuid::new_v4().simple().to_string();
        self.queue_with_key(chat, text, &key).await
    }

    async fn send(&self, chat: i64, text: &str) {
        if let Err(err) = self.queue(chat, text).await {
            eprintln!("HERON Telegram: could not persist a reply: {err}");
        }
    }

    /// Polling and concurrent agent completions can trigger this together.
    /// Serialize flushes to avoid sending the same unacknowledged row twice.
    async fn flush(&self) {
        let _lock = self.dispatch_lock.lock().await;
        // Claim immediately before the HTTP request. Leases last 60s while
        // this client's request times out after 40s. Never pre-lease a slow
        // batch that can expire while earlier messages are being delivered.
        for _ in 0..40 {
            let msg = match self.spool.due(
                ChannelKind::Telegram,
                &self.outbox_account_id(),
                crate::util::now_ts(),
                1,
            ) {
                Ok(batch) => match batch.into_iter().next() {
                    Some(msg) => msg,
                    None => break,
                },
                Err(err) => {
                    eprintln!("HERON Telegram: cannot load queued replies: {err}");
                    break;
                }
            };
            let chat = match msg.identity.chat_id.parse::<i64>() {
                Ok(chat) => chat,
                Err(_) => {
                    let _ = self.spool.fail(msg.id, &msg.identity, &msg.lease);
                    continue;
                }
            };
            if !self.is_current_owner(chat) {
                // Revocation must not leak a delayed response to a former owner.
                let _ = self.spool.purge_account(ChannelKind::Telegram, &self.outbox_account_id());
                break;
            }
            // Provider errors may echo the bot URL, which embeds its token: never log them.
            let outcome = match self.send_message(chat, &msg.text).await {
                Delivery::Sent(remote_id) => {
                    if let Err(err) = self.spool.delivered(msg.id, &msg.identity, &msg.lease, remote_id.as_deref()) {
                        eprintln!("HERON Telegram: reply acknowledgement failed: {err}");
                        break;
                    }
                    continue;
                }
                Delivery::Retry { after } => {
                    eprintln!("HERON Telegram: delivery failed; retry queued.");
                    // Telegram's retry_after counts from its answer, which can come many seconds after the request.
                    let now = crate::util::now_ts();
                    self.spool.retry(msg.id, &msg.identity, &msg.lease, now, after.map(|s| now.saturating_add(s)))
                }
                Delivery::Refused => {
                    eprintln!("HERON Telegram: Telegram refused a reply; it will not be retried.");
                    self.spool.fail(msg.id, &msg.identity, &msg.lease)
                }
            };
            if matches!(outcome, Ok(RetryOutcome::GaveUp)) {
                let _ = self.app.store.add_inbox(
                    UNDELIVERED_TITLE,
                    "Heron could not send a reply to your Telegram chat and stopped trying. Open the conversation in Heron to read it.",
                );
            }
            break;
        }
    }

    /// Do not persist responses to callers who have not completed pairing.
    async fn notice_unpaired(&self, chat: i64, text: &str) {
        let _ = self.call("sendMessage", json!({"chat_id": chat, "text": text})).await;
    }
}

#[derive(Default)]
struct Shared {
    /// A question waiting for the owner's yes/no.
    pending: Mutex<Option<(String, oneshot::Sender<bool>)>>,
    /// The reply in progress, with a number so a finished reply can only clear its own entry.
    running: Mutex<Option<(u64, tokio::task::AbortHandle)>>,
    next_run: std::sync::atomic::AtomicU64,
}

impl Shared {
    /// Halt the reply in progress, including a question it is waiting on. True if there was one.
    fn stop(&self) -> bool {
        self.pending.lock().unwrap().take();
        self.running.lock().unwrap().take().map(|(_, h)| h.abort()).is_some()
    }
}

// Settings can abort the gateway while separately spawned agent turns are
// still running. Dropping the gateway also aborts those child tasks.
struct AbortAgentsOnDrop(Arc<Shared>);
impl Drop for AbortAgentsOnDrop {
    fn drop(&mut self) {
        self.0.stop();
    }
}

/// Abort the gateway-owned delivery worker whenever the gateway exits.
struct AbortTaskOnDrop(tokio::task::JoinHandle<()>);
impl Drop for AbortTaskOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

struct TelegramApprover {
    bot: Bot,
    chat: i64,
    shared: Arc<Shared>,
}

#[async_trait]
impl Approver for TelegramApprover {
    async fn approve(&self, req: &ApprovalRequest) -> Approval {
        if req.risk.high() || req.tainted {
            let why = if req.tainted { "Heron has read outside content in this conversation" } else { "it is a risky action" };
            self.bot.send(self.chat, &format!("Heron wanted to use {} but did not: {why}, so it can only be approved at your computer.", req.tool)).await;
            return Approval::Deny(Some("this can only be approved at the user's computer, not from a phone".into()));
        }
        // Approval questions are deliberately NOT placed in the durable
        // outbound spool: an expired, stopped or rebooted request must never
        // surface as a live question after delivery recovers.
        if !self.bot.is_current_owner(self.chat) {
            return Approval::Deny(Some("Telegram pairing was revoked before approval.".into()));
        }
        let challenge = new_code().to_ascii_lowercase();
        let (tx, rx) = oneshot::channel();
        *self.shared.pending.lock().unwrap() = Some((challenge.clone(), tx));
        let prompt = format!(
            "Heron wants permission ({}):\n\n{}\n\nReply yes {challenge} or no {challenge}.",
            req.tool, crate::util::truncate_middle(&req.detail, 1200)
        );
        if !self.bot.is_current_owner(self.chat) {
            self.shared.pending.lock().unwrap().take();
            return Approval::Deny(Some("Telegram pairing changed before prompt delivery.".into()));
        }
        if self.bot.call("sendMessage", json!({"chat_id": self.chat, "text": prompt}))
            .await.is_err()
        {
            self.shared.pending.lock().unwrap().take();
            return Approval::Deny(Some("The permission prompt could not be delivered; no action was authorized.".into()));
        }
        let answer = tokio::time::timeout(Duration::from_secs(600), rx).await;
        self.shared.pending.lock().unwrap().take();
        match answer {
            Ok(Ok(true)) if self.bot.is_current_owner(self.chat) => Approval::Once,
            Ok(_) => Approval::Deny(None),
            Err(_) => Approval::Deny(Some("no answer within ten minutes".into())),
        }
    }
}

/// Wrong codes allowed before pairing locks and the code is replaced.
const MAX_WRONG_CODES: i64 = 5;

fn new_code() -> String {
    // Letters and digits that cannot be mistaken for one another; about 8.5e11 possibilities.
    const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
    let random = uuid::Uuid::new_v4();
    random.as_bytes().iter().take(8).map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char).collect()
}

fn same_code(sent: &str, code: &str) -> bool {
    let tidy = |s: &str| s.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect::<String>();
    !code.is_empty() && tidy(sent) == tidy(code)
}

/// The code a new chat must send to become the owner's chat. Created on first use.
pub fn pairing_code(app: &App) -> String {
    if let Some(code) = app.store.kv_get("telegram_code").filter(|c| !c.is_empty()) {
        return code;
    }
    let code = new_code();
    let _ = app.store.kv_set("telegram_code", &code);
    code
}

/// Stable, non-user-visible generation for queued replies. Unlike the short
/// pairing code, this UUID is not reused when a later owner pairs the same chat.
fn pairing_epoch(app: &App) -> String {
    if let Some(epoch) = app.store.kv_get("telegram_pairing_epoch").filter(|e| !e.is_empty()) {
        return epoch;
    }
    let epoch = uuid::Uuid::new_v4().simple().to_string();
    let _ = app.store.kv_set("telegram_pairing_epoch", &epoch);
    epoch
}

fn rotate_pairing_epoch(app: &App) -> String {
    let epoch = uuid::Uuid::new_v4().simple().to_string();
    let _ = app.store.kv_set("telegram_pairing_epoch", &epoch);
    epoch
}

/// The code, for showing in the app. Looking at it there is what re-opens pairing after too many
/// wrong guesses: only someone at the computer can do that.
pub fn show_pairing_code(app: &App) -> String {
    if pairing_locked(app) {
        let _ = app.store.kv_set("telegram_locked", "");
        let _ = app.store.kv_set("telegram_wrong", "0");
    }
    pairing_code(app)
}

pub fn pairing_locked(app: &App) -> bool {
    app.store.kv_get("telegram_locked").map(|v| v == "1").unwrap_or(false)
}

/// A chat sent something that was not the code. Anyone can find a bot and guess, so guesses are
/// counted: after a few, the code is replaced and no more are accepted until the owner looks at the
/// new one in the app. Returns true when this guess caused the lock.
fn wrong_code(app: &App) -> bool {
    let wrong = app.store.kv_get("telegram_wrong").and_then(|v| v.parse::<i64>().ok()).unwrap_or(0) + 1;
    let _ = app.store.kv_set("telegram_wrong", &wrong.to_string());
    if wrong < MAX_WRONG_CODES {
        return false;
    }
    let _ = app.store.kv_set("telegram_code", &new_code());
    rotate_pairing_epoch(app);
    let _ = app.store.kv_set("telegram_locked", "1");
    true
}

pub fn paired_chat(app: &App) -> Option<i64> {
    app.store.kv_get("telegram_chat").and_then(|c| c.parse().ok())
}

pub(crate) fn purge_token_spool(app: &App, token: &str) {
    let account = format!("{:x}", Sha256::digest(token.as_bytes()));
    if let Ok(spool) = ChannelStore::open(&app.cfg().home.join("channels.db")) {
        let _ = spool.purge_account(ChannelKind::Telegram, &account);
        let _ = spool.purge_outbox_account_prefix(ChannelKind::Telegram, &format!("{account}:"));
    }
}

pub fn unpair(app: &App) {
    let token = app.cfg().secret("telegram_token");
    // Revoke the pairing before purging. Detached turns check this state
    // after their durable enqueue, so no old-epoch write can race in behind
    // a purge that happened while the previous pairing was still valid.
    // A new pairing must never inherit a previous owner's messages.
    // Persist the ownership revocation and its new epoch in one SQLite
    // transaction. A crash must never leave an unpaired chat with the old
    // epoch, or an old queued reply could become valid after re-pairing.
    let epoch = uuid::Uuid::new_v4().simple().to_string();
    let code = new_code();
    if let Err(err) = app.store.kv_set_many(&[
        ("telegram_session", ""),
        // Preserve the update cursor for the *same bot*: replaying old owner
        // commands on re-pairing can re-run non-idempotent tool actions.
        ("telegram_inbox_last", "0"),
        ("telegram_chat", ""),
        ("telegram_code", code.as_str()),
        ("telegram_pairing_epoch", epoch.as_str()),
        ("telegram_wrong", "0"),
        ("telegram_locked", ""),
    ]) {
        eprintln!("HERON Telegram: could not commit pairing revocation: {err}");
        return;
    }
    if let Some(token) = token {
        purge_token_spool(app, &token);
    }
}

fn session_for(app: &App, fresh: bool) -> Result<Session> {
    if !fresh {
        if let Some(s) = app.store.kv_get("telegram_session").and_then(|id| Session::load(&app.store, &id).ok()) {
            return Ok(s);
        }
    }
    let s = Session::create(&app.store, "Phone conversation")?;
    app.store.kv_set("telegram_session", &s.id)?;
    Ok(s)
}

async fn handle_message(app: &Arc<App>, bot: &Bot, shared: &Arc<Shared>, chat: i64, text: String) {
    let lower = text.trim().to_lowercase();
    // Stopping comes first: "/stop" while a question is waiting must halt the whole reply, not merely
    // answer the question with "no" and let the work carry on.
    if lower == "/stop" {
        let stopped = shared.stop();
        bot.send(chat, if stopped { "Stopped." } else { "Nothing is running." }).await;
        return;
    }
    if lower == "/pause" {
        app.set_paused(true);
        shared.stop();
        bot.send(chat, "Paused. Heron will start nothing on its own (reminders, triggers, suggestions) until you send /resume.").await;
        return;
    }
    // A late response to a prior prompt must not approve whichever action
    // happens to be waiting now. Replies require the challenge displayed
    // in that precise approval request, and bare "yes" is not enough.
    let reply = {
        let mut pending = shared.pending.lock().unwrap();
        match pending.as_ref() {
            Some((challenge, _)) => {
                let words: Vec<_> = lower.split_whitespace().collect();
                let is_yes = words.first().is_some_and(|w| matches!(*w, "yes" | "y" | "ok" | "allow"));
                let is_no = words.first().is_some_and(|w| matches!(*w, "no" | "n" | "deny"));
                if words.len() == 2 && words[1] == challenge && (is_yes || is_no) {
                    let (_, tx) = pending.take().expect("approval pending was just checked");
                    Some(Ok((tx, is_yes)))
                } else {
                    Some(Err(()))
                }
            }
            None => None,
        }
    };
    match reply {
        Some(Ok((tx, allow))) => { let _ = tx.send(allow); return; }
        Some(Err(())) => {
            bot.send(chat, "An approval is pending. Reply using the exact yes/no code displayed in the permission question, or send /stop.").await;
            return;
        }
        None => {}
    }
    match lower.as_str() {
        "/new" => {
            let _ = session_for(app, true);
            bot.send(chat, "Started a new conversation.").await;
            return;
        }
        "/resume" => {
            app.set_paused(false);
            bot.send(chat, "Resumed.").await;
            return;
        }
        "/start" | "/help" => {
            bot.send(chat, "This is your Heron. Send a message to ask for something. /new starts a fresh conversation, /stop halts a running reply, /pause stops everything Heron does on its own, /resume turns it back on.").await;
            return;
        }
        _ => {}
    }
    {
        // The entry is written while the lock is held, so even a reply that finishes instantly finds
        // its own entry to clear instead of leaving a stale one behind.
        let mut running = shared.running.lock().unwrap();
        if running.is_none() {
            let run_id = shared.next_run.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let (app2, bot2, shared2) = (app.clone(), bot.clone(), shared.clone());
            let task = tokio::spawn(async move {
                let reply = async {
                    let mut session = session_for(&app2, false)?;
                    let agent = app2.agent(Arc::new(TelegramApprover { bot: bot2.clone(), chat, shared: shared2.clone() }));
                    let text_out = agent.run_turn(&mut session, &text, &null_sink()).await?;
                    app2.persist_rules();
                    agent.learn(&session, &text, &text_out).await;
                    Ok::<_, anyhow::Error>(text_out)
                }
                .await;
                {
                    let mut running = shared2.running.lock().unwrap();
                    if running.as_ref().map(|(id, _)| *id == run_id).unwrap_or(false) {
                        running.take();
                    }
                }
                match reply {
                    Ok(t) if t.trim().is_empty() => bot2.send(chat, "(done)").await,
                    Ok(t) => bot2.send(chat, &t).await,
                    Err(e) => bot2.send(chat, &format!("Heron could not finish: {e:#}")).await,
                }
            });
            *running = Some((run_id, task.abort_handle()));
            return;
        }
    }
    bot.send(chat, "Still working on your last message. Send /stop to halt it.").await;
}

/// Forward results of scheduled and self-started work to the paired chat.
async fn forward_inbox(app: &App, bot: &Bot, chat: i64) {
    let last: i64 = app.store.kv_get("telegram_inbox_last").and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut items = app.store.inbox(false, 20).unwrap_or_default();
    items.retain(|m| m.id > last);
    items.sort_by_key(|m| m.id);
    for m in items {
        if m.title == UNDELIVERED_TITLE {
            let _ = app.store.kv_set("telegram_inbox_last", &m.id.to_string());
            continue;
        }
        if bot.queue_with_key(chat, &format!("{}\n\n{}", m.title, m.body), &format!("inbox:{}", m.id)).await.is_ok() {
            // Inbox advancement means durably queued, not necessarily delivered.
            let _ = app.store.kv_set("telegram_inbox_last", &m.id.to_string());
        } else {
            break;
        }
    }
}

/// Run the gateway until the task is dropped. `api_base` is normally https://api.telegram.org.
/// Changes whenever the bot token is replaced or removed. Every Heron process that shares this
/// home reads it, so a gateway in another process notices that its token was revoked and stops.
const GENERATION_KEY: &str = "telegram_generation";

pub fn bump_generation(app: &App) {
    let _ = app.store.kv_set(GENERATION_KEY, &uuid::Uuid::new_v4().simple().to_string());
}

fn generation(app: &App) -> String {
    app.store.kv_get(GENERATION_KEY).unwrap_or_default()
}

/// One gateway per Heron home, across processes: two readers of one bot split its updates and
/// race the spool. Waits for the current holder to stop, and gives up if the token changes first.
async fn take_gateway_lock(app: &App, started: &str) -> Option<std::fs::File> {
    let cfg = app.cfg();
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let file = match cfg.ensure_home().and_then(|_| Ok(opts.open(cfg.home.join("telegram.lock"))?)) {
        Ok(file) => file,
        Err(err) => {
            eprintln!("HERON Telegram: gateway disabled because its lock file cannot open: {err}");
            return None;
        }
    };
    let mut told = false;
    loop {
        match file.try_lock() {
            Ok(()) => return Some(file),
            Err(std::fs::TryLockError::WouldBlock) => {}
            Err(std::fs::TryLockError::Error(err)) => {
                eprintln!("HERON Telegram: gateway disabled because its lock cannot be taken: {err}");
                return None;
            }
        }
        if generation(app) != started {
            return None;
        }
        if !told {
            eprintln!("HERON Telegram: another Heron process is running the gateway; waiting for it to stop.");
            told = true;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// Asks Telegram whether a token belongs to a live bot, before it replaces the working one.
/// Messages never include the token: reqwest errors carry the URL, which contains it.
pub async fn verify_token(api_base: &str, token: &str) -> std::result::Result<(), String> {
    let url = format!("{}/bot{token}/getMe", api_base.trim_end_matches('/'));
    let unreachable = || "Heron could not reach Telegram to check the token. Check your connection and try again.".to_string();
    let client = reqwest::Client::builder().timeout(Duration::from_secs(15)).build().map_err(|_| unreachable())?;
    let resp = client.get(url).send().await.map_err(|_| unreachable())?;
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    if status.is_success() && body["ok"] == true && body["result"]["is_bot"] == true {
        return Ok(());
    }
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::NOT_FOUND {
        return Err("Telegram does not recognise that bot token. Copy it again from @BotFather.".into());
    }
    Err(format!("Telegram could not check the token right now (HTTP {}). Try again in a moment.", status.as_u16()))
}

pub async fn run(app: Arc<App>, api_base: String, token: String) {
    let started = generation(&app);
    let Some(_gateway_lock) = take_gateway_lock(&app, &started).await else { return };
    let spool = match ChannelStore::open(&app.cfg().home.join("channels.db")) {
        Ok(spool) => spool,
        Err(err) => {
            eprintln!("HERON Telegram: gateway disabled because its secure local queue cannot open: {err}");
            return;
        }
    };
    let mut bot = Bot {
        client: reqwest::Client::builder().timeout(Duration::from_secs(40)).build().expect("http client"),
        base: format!("{}/bot{token}", api_base.trim_end_matches('/')),
        app: app.clone(),
        account_id: format!("{:x}", Sha256::digest(token.as_bytes())),
        pairing_epoch: pairing_epoch(&app),
        spool,
        dispatch_lock: Arc::new(tokio::sync::Mutex::new(())),
    };
    // Old builds could persist approval questions. No prior process still
    // holds those pending approvals, so never deliver them after restart.
    if let Err(err) = bot.spool.discard_stale_approval_prompts(
        ChannelKind::Telegram, &bot.account_id
    ) {
        eprintln!("HERON Telegram: could not clear obsolete approval prompts: {err}");
        return;
    }
    // Pre-epoch builds keyed outbound rows only by bot token. They cannot be
    // safely attributed to the current pairing, so discard those legacy rows
    // rather than risk sending a previous owner's reply after an upgrade.
    match bot.spool.purge_outbox_account(ChannelKind::Telegram, &bot.account_id) {
        Ok(count) if count > 0 => eprintln!("HERON Telegram: discarded {count} legacy outbox row(s) without a pairing epoch."),
        Ok(_) => {}
        Err(err) => {
            eprintln!("HERON Telegram: could not discard legacy outbox rows: {err}");
            return;
        }
    }
    // An accepted turn left unfinished by a crash may have performed a tool
    // action before stopping. Do not re-execute it automatically.
    if let Ok(count) = bot.spool.mark_interrupted(ChannelKind::Telegram, &bot.account_id) {
        if count > 0 {
            if let Some(chat) = paired_chat(&app) {
                bot.send(chat, &format!("{count} prior request(s) may have been interrupted. Please resend anything still needed.")).await;
            }
        }
    }
    let shared: Arc<Shared> = Default::default();
    let _abort_children_on_shutdown = AbortAgentsOnDrop(shared.clone());
    // Outbound delivery must not depend on getUpdates succeeding, and draining
    // a backlog must not hold the polling loop hostage. Keep the worker's
    // pairing epoch current, while detached agent turns retain their snapshots.
    let mut outbox_bot = bot.clone();
    let _outbox_worker = AbortTaskOnDrop(tokio::spawn(async move {
        loop {
            let current_epoch = pairing_epoch(&outbox_bot.app);
            if current_epoch != outbox_bot.pairing_epoch {
                // Drop rows from the previous pairing. A late detached task
                // may still enqueue under that old epoch, but the new worker
                // will never query or deliver those rows.
                let old_account = outbox_bot.outbox_account_id();
                let _ = outbox_bot.spool.purge_account(ChannelKind::Telegram, &old_account);
                outbox_bot.pairing_epoch = current_epoch;
            }
            outbox_bot.flush().await;
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }));
    let mut offset: i64 = app.store.kv_get("telegram_offset").and_then(|v| v.parse().ok()).unwrap_or(0);
    let poll_secs = if api_base.contains("127.0.0.1") { 0 } else { 25 };
    loop {
        if generation(&app) != started {
            eprintln!("HERON Telegram: the bot token changed; this gateway stopped.");
            return;
        }
        let updates = match bot.call("getUpdates", json!({"offset": offset, "timeout": poll_secs, "allowed_updates": ["message"]})).await {
            Ok(u) => u,
            Err(_) => {
                tokio::time::sleep(Duration::from_secs(if poll_secs == 0 { 1 } else { 10 })).await;
                continue;
            }
        };
        // A long poll can return after the token was replaced: those messages were sent to the old bot.
        if generation(&app) != started {
            eprintln!("HERON Telegram: the bot token changed; this gateway stopped.");
            return;
        }
        for u in updates.as_array().cloned().unwrap_or_default() {
            let Some(update_id) = u["update_id"].as_i64().filter(|id| *id > 0) else {
                continue;
            };
            let next_offset = offset.max(update_id + 1);
            if let Some(chat) = u["message"]["chat"]["id"].as_i64() {
                // Never authorize groups: someone else could post a command.
                if u["message"]["chat"]["type"].as_str() == Some("private")
                    && u["message"]["from"]["id"].as_i64() == Some(chat) {
                    let text = u["message"]["text"].as_str().map(str::to_string);
                    match paired_chat(&app) {
                        Some(owner) if owner == chat => match text {
                            Some(t) => {
                                let identity = bot.identity(chat);
                                let event = InboundMessage {
                                    identity: identity.clone(),
                                    provider_message_id: update_id.to_string(),
                                    thread_id: None,
                                    text: t.clone(),
                                };
                                match bot.spool.accept_inbound(
                                    &identity,
                                    &event,
                                    crate::util::now_ts(),
                                ) {
                                    Ok(true) => {
                                        handle_message(&app, &bot, &shared, chat, t).await;
                                        let _ = bot.spool.complete_inbound(
                                            &identity,
                                            &event.provider_message_id,
                                        );
                                    }
                                    Ok(false) => {}
                                    Err(err) => {
                                        // Keep the Telegram offset unchanged so this
                                        // update can be inspected/retried rather than lost.
                                        eprintln!("HERON Telegram: cannot persist incoming message: {err}");
                                        break;
                                    }
                                }
                            }
                            None => bot.send(chat, "Heron can only read text messages for now.").await,
                        },
                        Some(_) => {} // A different person: no response, no action
                        None => {
                            let sent = text.as_deref().map(str::trim).unwrap_or("");
                            if sent == "/start" {
                                bot.notice_unpaired(chat, "This Heron is not paired yet. If it is yours, send the pairing code shown in the Heron app under Settings.").await;
                            } else if pairing_locked(&app) {
                                // Ignore guesses until local Settings unlocks pairing.
                            } else if same_code(sent, &pairing_code(&app)) {
                                let _ = app.store.kv_set("telegram_wrong", "0");
                                let _ = app.store.kv_set("telegram_chat", &chat.to_string());
                                // The running transport adopts the new pairing code;
                                // older cloned Bots retain their previous epoch and cannot
                                // send queued results after re-pairing.
                                bot.pairing_epoch = pairing_epoch(&app);
                                let last_inbox = app.store.inbox(false, 1).ok().and_then(|i| i.first().map(|m| m.id)).unwrap_or(0);
                                let _ = app.store.kv_set("telegram_inbox_last", &last_inbox.to_string());
                                bot.send(chat, "Paired. This chat now talks to your Heron, and nobody else's messages will be answered.").await;
                            } else if wrong_code(&app) {
                                bot.notice_unpaired(chat, "Too many wrong codes. Pairing is locked and the code has been replaced. If this Heron is yours, open it on your computer (Settings) to see the new code.").await;
                            }
                        }
                    }
                }
            }
            offset = next_offset;
            let _ = app.store.kv_set("telegram_offset", &offset.to_string());
        }
        if let Some(chat) = paired_chat(&app) {
            forward_inbox(&app, &bot, chat).await;
        }
        if poll_secs == 0 {
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    }
}

/// Start the gateway in the background if a bot token has been saved. Any gateway already running
/// is stopped first, so there is never more than one reader of the bot's messages and a replaced token
/// takes effect at once.
pub fn spawn_if_configured(app: &Arc<App>) -> bool {
    stop(app);
    let cfg = app.cfg();
    if !cfg.vault_only_secret_present("telegram_token") {
        // Legacy plaintext/file fallback is not an acceptable new messaging
        // credential source. User must reconnect through the verified vault.
        return false;
    }
    match cfg.secret("telegram_token") {
        Some(token) => {
            let task = tokio::spawn(run(app.clone(), cfg.telegram.api_base.clone(), token));
            *app.telegram_task.lock().unwrap() = Some(task.abort_handle());
            true
        }
        None => false,
    }
}

/// Stop the gateway, if it is running.
pub fn stop(app: &App) {
    if let Some(task) = app.telegram_task.lock().unwrap().take() {
        task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::llm::ScriptedLlm;
    use crate::store::Store;

    #[test]
    fn splits_telegram_replies_by_utf16_not_unicode_scalar_count() {
        let text = "😃".repeat(2400);
        let chunks = Bot::chunks(&text);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks.concat(), text);
        assert!(chunks.iter().all(|part| part.encode_utf16().count() <= MAX_MESSAGE));
    }

    async fn mock_telegram(reply: fn(&str) -> (u16, Value)) -> (String, Arc<Mutex<Vec<String>>>) {
        let paths = Arc::new(Mutex::new(Vec::<String>::new()));
        let router = axum::Router::new()
            .fallback(move |axum::extract::State(p): axum::extract::State<Arc<Mutex<Vec<String>>>>, uri: axum::http::Uri| async move {
                p.lock().unwrap().push(uri.path().to_string());
                let (status, body) = reply(uri.path());
                (axum::http::StatusCode::from_u16(status).unwrap(), axum::Json(body))
            })
            .with_state(paths.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        (base, paths)
    }

    /// A separate Heron process: its own App and connection, the same home and database.
    async fn heron_process(root: &std::path::Path) -> Arc<App> {
        let mut cfg = Config::default();
        cfg.home = root.join("home");
        cfg.ensure_home().unwrap();
        Arc::new(App::assemble(
            cfg, root.join("ws"),
            Store::open(&root.join("home/heron.db")).unwrap(),
            Arc::new(ScriptedLlm::new(vec![])),
        ).await.unwrap())
    }

    async fn until(what: &str, check: impl Fn() -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !check() {
            assert!(std::time::Instant::now() < deadline, "timed out waiting for: {what}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn mock_telegram_with_send_delay(
        delay: Duration,
        fail_poll: bool,
    ) -> (String, Arc<Mutex<Vec<String>>>) {
        let paths = Arc::new(Mutex::new(Vec::<String>::new()));
        let router = axum::Router::new()
            .fallback(move |axum::extract::State(p): axum::extract::State<Arc<Mutex<Vec<String>>>>, uri: axum::http::Uri| async move {
                let path = uri.path().to_string();
                p.lock().unwrap().push(path.clone());
                if path.ends_with("/sendMessage") {
                    tokio::time::sleep(delay).await;
                }
                let (status, body) = if path.ends_with("/getUpdates") && fail_poll {
                    (503, json!({"ok": false, "description": "poll unavailable"}))
                } else if path.ends_with("/getUpdates") {
                    (200, json!({"ok": true, "result": []}))
                } else if path.ends_with("/sendMessage") {
                    (200, json!({"ok": true, "result": {"message_id": 1}}))
                } else {
                    (404, json!({"ok": false}))
                };
                (axum::http::StatusCode::from_u16(status).unwrap(), axum::Json(body))
            })
            .with_state(paths.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        (base, paths)
    }

    async fn paired_heron(root: &std::path::Path) -> Arc<App> {
        let app = heron_process(root).await;
        let _ = pairing_code(&app);
        let _ = pairing_epoch(&app);
        app.store.kv_set("telegram_chat", "42").unwrap();
        app
    }

    fn queue_test_replies(app: &App, token: &str, count: usize) {
        let spool = ChannelStore::open(&app.cfg().home.join("channels.db")).unwrap();
        let token_account = format!("{:x}", Sha256::digest(token.as_bytes()));
        let epoch = pairing_epoch(&app);
        let identity = ChannelIdentity {
            kind: ChannelKind::Telegram,
            account_id: format!("{token_account}:{epoch}"),
            chat_id: "42".into(),
            sender_id: "42".into(),
        };
        for index in 0..count {
            let chunk = format!("queued reply {index}");
            spool.enqueue_parts(
                &identity,
                None,
                &[chunk],
                &format!("outbox-worker-test-{index}"),
                crate::util::now_ts(),
            ).unwrap();
        }
    }

    #[tokio::test]
    async fn legacy_unscoped_outbox_rows_are_discarded_on_gateway_start() {
        let (base, paths) = mock_telegram_with_send_delay(Duration::ZERO, false).await;
        let dir = tempfile::tempdir().unwrap();
        let app = paired_heron(dir.path()).await;
        let token = "LEGACY-OUTBOX";
        let spool = ChannelStore::open(&app.cfg().home.join("channels.db")).unwrap();
        let identity = ChannelIdentity {
            kind: ChannelKind::Telegram,
            account_id: format!("{:x}", Sha256::digest(token.as_bytes())),
            chat_id: "42".into(),
            sender_id: "42".into(),
        };
        spool.enqueue_parts(&identity, None, &["legacy reply".to_string()], "legacy-reply", crate::util::now_ts()).unwrap();

        let gateway = tokio::spawn(run(app, base, token.into()));
        until("legacy outbox purge", || spool.count_pending().unwrap() == 0).await;
        assert!(
            !paths.lock().unwrap().iter().any(|path| path.ends_with("/sendMessage")),
            "an unscoped legacy reply must not be delivered under the new pairing epoch"
        );
        gateway.abort();
    }

    #[tokio::test]
    async fn previous_pairing_epoch_outbox_is_never_delivered_after_repair() {
        let (base, paths) = mock_telegram_with_send_delay(Duration::ZERO, false).await;
        let dir = tempfile::tempdir().unwrap();
        let app = paired_heron(dir.path()).await;
        let token = "OUTBOX-OLD-EPOCH";
        queue_test_replies(&app, token, 1);
        let old_epoch = pairing_epoch(&app);
        let new_epoch = rotate_pairing_epoch(&app);

        let gateway = tokio::spawn(run(app.clone(), base, token.into()));
        tokio::time::sleep(Duration::from_millis(600)).await;
        assert!(
            !paths.lock().unwrap().iter().any(|path| path.ends_with("/sendMessage")),
            "a reply queued under the previous pairing epoch was delivered after re-pairing"
        );
        gateway.abort();

        let spool = ChannelStore::open(&app.cfg().home.join("channels.db")).unwrap();
        assert_eq!(spool.count_pending().unwrap(), 1, "the old row is not visible to the new epoch");
        purge_token_spool(&app, token);
        assert_eq!(spool.count_pending().unwrap(), 0, "token revocation purges every pairing-scoped outbox row");
        assert_ne!(old_epoch, new_epoch);
    }

    #[tokio::test]
    async fn outbox_retries_continue_when_inbound_polling_is_unavailable() {
        let (base, paths) = mock_telegram_with_send_delay(Duration::ZERO, true).await;
        let dir = tempfile::tempdir().unwrap();
        let app = paired_heron(dir.path()).await;
        queue_test_replies(&app, "OUTBOX-POLL-FAIL", 1);

        let gateway = tokio::spawn(run(app, base, "OUTBOX-POLL-FAIL".into()));
        until("queued reply delivery while getUpdates is failing", || {
            paths.lock().unwrap().iter().any(|path| path.ends_with("/sendMessage"))
        }).await;
        gateway.abort();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn draining_outbox_does_not_block_inbound_polling() {
        let (base, paths) = mock_telegram_with_send_delay(Duration::from_millis(400), false).await;
        let dir = tempfile::tempdir().unwrap();
        let app = paired_heron(dir.path()).await;
        queue_test_replies(&app, "OUTBOX-SLOW-SEND", 3);

        let gateway = tokio::spawn(run(app, base, "OUTBOX-SLOW-SEND".into()));
        let count = |suffix: &str| paths.lock().unwrap().iter()
            .filter(|path| path.ends_with(suffix)).count();
        until("first slow outbound send", || count("/sendMessage") > 0).await;
        let polls_before = count("/getUpdates");
        tokio::time::sleep(Duration::from_millis(120)).await;
        let polls_after = count("/getUpdates");
        assert!(
            polls_after > polls_before,
            "inbound polling stopped while sendMessage was in flight: before={polls_before}, after={polls_after}"
        );
        gateway.abort();
    }


    #[tokio::test]
    async fn telegram_answers_decide_whether_a_reply_is_sent_retried_or_given_up() {
        let (base, _) = mock_telegram(|path| match path {
            "/botOK/sendMessage" => (200, json!({"ok": true, "result": {"message_id": 7}})),
            "/botSLOW/sendMessage" => (429, json!({"ok": false, "error_code": 429, "parameters": {"retry_after": 30}})),
            "/botGONE/sendMessage" => (403, json!({"ok": false, "error_code": 403, "description": "Forbidden: bot was blocked by the user"})),
            "/botBAD/sendMessage" => (400, json!({"ok": false, "error_code": 400})),
            _ => (502, json!({"ok": false, "error_code": 502})),
        }).await;
        let dir = tempfile::tempdir().unwrap();
        let app = heron_process(dir.path()).await;
        let bot = |name: &str| Bot {
            client: reqwest::Client::new(),
            base: format!("{base}/bot{name}"),
            app: app.clone(),
            account_id: "bot".into(),
            pairing_epoch: String::new(),
            spool: ChannelStore::open(&app.cfg().home.join("channels.db")).unwrap(),
            dispatch_lock: Arc::new(tokio::sync::Mutex::new(())),
        };
        assert_eq!(bot("OK").send_message(42, "hi").await, Delivery::Sent(Some("7".into())));
        assert_eq!(bot("SLOW").send_message(42, "hi").await, Delivery::Retry { after: Some(30) });
        assert_eq!(bot("GONE").send_message(42, "hi").await, Delivery::Refused);
        assert_eq!(bot("BAD").send_message(42, "hi").await, Delivery::Refused);
        assert_eq!(bot("DOWN").send_message(42, "hi").await, Delivery::Retry { after: None });
        let offline = Bot { base: "http://127.0.0.1:1/botX".into(), ..bot("X") };
        assert_eq!(offline.send_message(42, "hi").await, Delivery::Retry { after: None });
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn one_gateway_per_home_and_a_replaced_token_stops_the_other_process() {
        let (base, paths) = mock_telegram(|_| (200, json!({"ok": true, "result": []}))).await;
        let polls = |bot: &str| {
            let prefix = format!("/bot{bot}/getUpdates");
            paths.lock().unwrap().iter().filter(|p| **p == prefix).count()
        };
        let dir = tempfile::tempdir().unwrap();
        let daemon = heron_process(dir.path()).await;
        let serve = heron_process(dir.path()).await;

        let old = tokio::spawn(run(daemon.clone(), base.clone(), "OLD".into()));
        until("the first gateway polls", || polls("OLD") > 0).await;
        let second = tokio::spawn(run(serve.clone(), base.clone(), "SECOND".into()));
        tokio::time::sleep(Duration::from_millis(600)).await;
        assert_eq!(polls("SECOND"), 0, "a second gateway polled while the first was running");
        second.abort();

        // What set_telegram does in the other process when the owner pastes a new token.
        bump_generation(&serve);
        let new = tokio::spawn(run(serve.clone(), base.clone(), "NEW".into()));
        until("the old gateway stops", || old.is_finished()).await;
        until("the new gateway takes over", || polls("NEW") > 0).await;
        let after = polls("OLD");
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(polls("OLD"), after, "the revoked bot is still being polled");
        new.abort();
    }

    #[tokio::test]
    async fn token_check_accepts_a_live_bot_and_never_echoes_the_token() {
        let (base, paths) = mock_telegram(|path| match path {
            "/botLIVE/getMe" => (200, json!({"ok": true, "result": {"id": 1, "is_bot": true}})),
            "/botBUSY/getMe" => (500, json!({"ok": false})),
            "/botUSER/getMe" => (200, json!({"ok": true, "result": {"id": 2, "is_bot": false}})),
            _ => (401, json!({"ok": false, "error_code": 401, "description": "Unauthorized"})),
        }).await;
        assert_eq!(verify_token(&format!("{base}/"), "LIVE").await, Ok(()));
        assert_eq!(paths.lock().unwrap().as_slice(), ["/botLIVE/getMe"]);
        let revoked = verify_token(&base, "REVOKED").await.unwrap_err();
        assert!(revoked.contains("does not recognise"), "{revoked}");
        let busy = verify_token(&base, "BUSY").await.unwrap_err();
        assert!(busy.contains("HTTP 500"), "{busy}");
        assert!(verify_token(&base, "USER").await.is_err(), "only a bot account may be connected");
        let offline = verify_token("http://127.0.0.1:1", "OFFLINE").await.unwrap_err();
        for msg in [&revoked, &busy, &offline] {
            assert!(!msg.contains("REVOKED") && !msg.contains("BUSY") && !msg.contains("OFFLINE"), "{msg}");
        }
    }

    #[tokio::test]
    async fn revoked_chat_cannot_receive_direct_approval_prompt() {
        use crate::permissions::Effect;
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.home = dir.path().join("home");
        let app = Arc::new(App::assemble(
            cfg, dir.path().join("ws"),
            Store::in_memory().unwrap(),
            Arc::new(ScriptedLlm::new(vec![])),
        ).await.unwrap());
        let _code = pairing_code(&app);
        let epoch = pairing_epoch(&app);
        app.store.kv_set("telegram_chat", "42").unwrap();
        let bot = Bot {
            client: reqwest::Client::new(),
            base: "http://127.0.0.1:1/botDISABLED".into(),
            app: app.clone(),
            account_id: "disposable-bot".into(),
            pairing_epoch: epoch,
            spool: ChannelStore::open(&app.cfg().home.join("channels.db")).unwrap(),
            dispatch_lock: Arc::new(tokio::sync::Mutex::new(())),
        };
        assert!(bot.is_current_owner(42));
        // Simulate disconnect/replacement after the turn started but before
        // the LLM requested tool permission.
        unpair(&app);
        assert!(!bot.is_current_owner(42));
        let shared = Arc::<Shared>::default();
        let approver = TelegramApprover { bot, chat: 42, shared: shared.clone() };
        let approval = approver.approve(&ApprovalRequest {
            tool: "write_file".into(),
            summary: "change a file".into(),
            risk: Effect::WorkspaceWrite,
            detail: "This content must never be delivered to the old owner".into(),
            tainted: false,
            args: json!({"path":"private.txt"}),
            session_id: "stale-turn".into(),
        }).await;
        assert!(matches!(approval, Approval::Deny(Some(reason)) if reason.contains("revoked")));
        assert!(shared.pending.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn stopping_gateway_aborts_detached_agent_turns() {
        let shared = Arc::<Shared>::default();
        let task = tokio::spawn(async {
            tokio::time::sleep(Duration::from_secs(120)).await;
        });
        *shared.running.lock().unwrap() = Some((1, task.abort_handle()));
        let guard = AbortAgentsOnDrop(shared.clone());
        drop(guard);
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(shared.running.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn unpair_does_not_replay_old_updates_for_the_same_bot() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.home = dir.path().join("home");
        let app = App::assemble(
            cfg,
            dir.path().join("ws"),
            Store::in_memory().unwrap(),
            Arc::new(ScriptedLlm::new(vec![])),
        ).await.unwrap();
        app.store.kv_set("telegram_offset", "982").unwrap();
        app.store.kv_set("telegram_chat", "42").unwrap();
        app.store.kv_set("telegram_session", "abc").unwrap();
        app.store.kv_set("telegram_pairing_epoch", "old-epoch").unwrap();
        unpair(&app);
        assert_eq!(app.store.kv_get("telegram_offset").as_deref(), Some("982"));
        assert_eq!(app.store.kv_get("telegram_session").as_deref(), Some(""));
        assert!(paired_chat(&app).is_none());
        assert_ne!(pairing_epoch(&app), "old-epoch", "unpair atomically revokes the prior pairing epoch");
    }

    #[tokio::test]
    async fn guessing_the_pairing_code_locks_pairing_until_the_owner_looks_at_the_app() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.home = dir.path().join("home");
        let app = App::assemble(cfg, dir.path().join("ws"), Store::in_memory().unwrap(), Arc::new(ScriptedLlm::new(vec![]))).await.unwrap();
        let code = pairing_code(&app);
        let initial_epoch = pairing_epoch(&app);
        assert_eq!(code.len(), 8);
        assert_eq!(pairing_code(&app), code, "the code is stable until it is used or replaced");
        assert!(same_code(&format!(" {}-{} ", code[..4].to_lowercase(), &code[4..]), &code), "spacing and case do not matter");
        assert!(!same_code("", &code) && !same_code(&code[..7], &code));

        for _ in 0..MAX_WRONG_CODES - 1 {
            assert!(!wrong_code(&app));
        }
        assert!(!pairing_locked(&app));
        assert!(wrong_code(&app), "the last allowed guess locks pairing");
        assert!(pairing_locked(&app));
        assert_ne!(pairing_epoch(&app), initial_epoch, "locking pairing also invalidates old queued replies");
        let replaced = pairing_code(&app);
        assert_ne!(replaced, code, "the code a guesser was closing in on is gone");
        // Only showing the code in the app (which needs the computer) opens pairing again.
        assert_eq!(show_pairing_code(&app), replaced);
        assert!(!pairing_locked(&app));
        assert!(!wrong_code(&app), "and the count starts again");
    }
}
