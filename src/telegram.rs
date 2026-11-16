//! Telegram gateway: talk to Heron from your phone.
//!
//! Safety model: the bot answers exactly one chat — the one that proved ownership by sending the
//! pairing code shown in the app. Everyone else is ignored. Approvals for file changes and commands
//! can be given by replying yes/no; risky actions, and anything requested after outside content
//! entered the conversation, can only be approved at the computer. `/stop` halts a running reply.

use crate::agent::{null_sink, Session};
use crate::app::App;
use crate::channel::{ChannelIdentity, ChannelKind, ChannelStore, InboundMessage};
use crate::permissions::{Approval, ApprovalRequest, Approver};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;

const MAX_MESSAGE: usize = 3800;

#[derive(Clone)]
struct Bot {
    client: reqwest::Client,
    base: String,
    account_id: String,
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

    /// Fail-closed on local persistence errors: do not send a message that
    /// cannot be recovered if HERON stops before delivery completes.
    async fn queue(&self, chat: i64, text: &str) -> Result<()> {
        let identity = self.identity(chat);
        for part in Self::chunks(text) {
            let key = uuid::Uuid::new_v4().simple().to_string();
            self.spool.enqueue(&identity, None, &part, &key, crate::util::now_ts())?;
        }
        // A provider timeout does not discard a queued message; it is retried.
        self.flush().await;
        Ok(())
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
        let due = match self.spool.due(
            ChannelKind::Telegram,
            &self.account_id,
            crate::util::now_ts(),
            40,
        ) {
            Ok(rows) => rows,
            Err(err) => {
                eprintln!("HERON Telegram: cannot load queued replies: {err}");
                return;
            }
        };
        for msg in due {
            let chat = match msg.identity.chat_id.parse::<i64>() {
                Ok(chat) => chat,
                Err(_) => {
                    let _ = self.spool.retry(msg.id, &msg.identity, crate::util::now_ts());
                    continue;
                }
            };
            match self.call("sendMessage", json!({"chat_id": chat, "text": msg.text})).await {
                Ok(response) => {
                    let remote_id = response["message_id"].as_i64().map(|id| id.to_string());
                    if let Err(err) = self.spool.delivered(
                        msg.id,
                        &msg.identity,
                        remote_id.as_deref(),
                    ) {
                        eprintln!("HERON Telegram: reply acknowledgement failed: {err}");
                        break;
                    }
                }
                Err(err) => {
                    let _ = self.spool.retry(msg.id, &msg.identity, crate::util::now_ts());
                    eprintln!("HERON Telegram: delivery failed; retry queued: {err}");
                    break;
                }
            }
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
    pending: Mutex<Option<oneshot::Sender<bool>>>,
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
        let (tx, rx) = oneshot::channel();
        *self.shared.pending.lock().unwrap() = Some(tx);
        self.bot.send(self.chat, &format!("Heron wants permission ({}):\n\n{}\n\nReply yes or no.", req.tool, crate::util::truncate_middle(&req.detail, 1200))).await;
        let answer = tokio::time::timeout(Duration::from_secs(600), rx).await;
        self.shared.pending.lock().unwrap().take();
        match answer {
            Ok(Ok(true)) => Approval::Once,
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
    let _ = app.store.kv_set("telegram_locked", "1");
    true
}

pub fn paired_chat(app: &App) -> Option<i64> {
    app.store.kv_get("telegram_chat").and_then(|c| c.parse().ok())
}

pub fn unpair(app: &App) {
    // Clear unsent replies and inbound identifiers when the trusted pairing
    // is revoked. A newly paired person must not inherit another chat's data.
    if let Some(token) = app.cfg().secret("telegram_token") {
        let account = format!("{:x}", Sha256::digest(token.as_bytes()));
        if let Ok(spool) = ChannelStore::open(&app.cfg().home.join("channels.db")) {
            let _ = spool.purge_account(ChannelKind::Telegram, &account);
        }
    }
    let _ = app.store.kv_set("telegram_chat", "");
    let _ = app.store.kv_set("telegram_code", &new_code());
    let _ = app.store.kv_set("telegram_wrong", "0");
    let _ = app.store.kv_set("telegram_locked", "");
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
    // A pending yes/no question takes the next message.
    if let Some(tx) = shared.pending.lock().unwrap().take() {
        let _ = tx.send(matches!(lower.as_str(), "yes" | "y" | "ok" | "allow"));
        return;
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
        if bot.queue(chat, &format!("{}\n\n{}", m.title, m.body)).await.is_ok() {
            // Inbox advancement means durably queued, not necessarily delivered.
            let _ = app.store.kv_set("telegram_inbox_last", &m.id.to_string());
        } else {
            break;
        }
    }
}

/// Run the gateway until the task is dropped. `api_base` is normally https://api.telegram.org.
pub async fn run(app: Arc<App>, api_base: String, token: String) {
    let spool = match ChannelStore::open(&app.cfg().home.join("channels.db")) {
        Ok(spool) => spool,
        Err(err) => {
            eprintln!("HERON Telegram: gateway disabled because its secure local queue cannot open: {err}");
            return;
        }
    };
    let bot = Bot {
        client: reqwest::Client::builder().timeout(Duration::from_secs(40)).build().expect("http client"),
        base: format!("{}/bot{token}", api_base.trim_end_matches('/')),
        account_id: format!("{:x}", Sha256::digest(token.as_bytes())),
        spool,
        dispatch_lock: Arc::new(tokio::sync::Mutex::new(())),
    };
    let shared: Arc<Shared> = Default::default();
    let mut offset: i64 = app.store.kv_get("telegram_offset").and_then(|v| v.parse().ok()).unwrap_or(0);
    let poll_secs = if api_base.contains("127.0.0.1") { 0 } else { 25 };
    loop {
        let updates = match bot.call("getUpdates", json!({"offset": offset, "timeout": poll_secs, "allowed_updates": ["message"]})).await {
            Ok(u) => u,
            Err(_) => {
                tokio::time::sleep(Duration::from_secs(if poll_secs == 0 { 1 } else { 10 })).await;
                continue;
            }
        };
        for u in updates.as_array().cloned().unwrap_or_default() {
            let update_id = u["update_id"].as_i64().unwrap_or(0);
            let next_offset = offset.max(update_id + 1);
            if let Some(chat) = u["message"]["chat"]["id"].as_i64() {
                // Never authorize groups: someone else could post a command.
                if u["message"]["chat"]["type"].as_str().unwrap_or("private") == "private" {
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
        bot.flush().await;
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

    #[tokio::test]
    async fn guessing_the_pairing_code_locks_pairing_until_the_owner_looks_at_the_app() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.home = dir.path().join("home");
        let app = App::assemble(cfg, dir.path().join("ws"), Store::in_memory().unwrap(), Arc::new(ScriptedLlm::new(vec![]))).await.unwrap();
        let code = pairing_code(&app);
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
        let replaced = pairing_code(&app);
        assert_ne!(replaced, code, "the code a guesser was closing in on is gone");
        // Only showing the code in the app (which needs the computer) opens pairing again.
        assert_eq!(show_pairing_code(&app), replaced);
        assert!(!pairing_locked(&app));
        assert!(!wrong_code(&app), "and the count starts again");
    }
}
