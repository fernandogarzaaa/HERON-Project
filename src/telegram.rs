//! Telegram gateway: talk to Heron from your phone.
//!
//! Safety model: the bot answers exactly one chat — the one that proved ownership by sending the
//! pairing code shown in the app. Everyone else is ignored. Approvals for file changes and commands
//! can be given by replying yes/no; risky actions, and anything requested after outside content
//! entered the conversation, can only be approved at the computer. `/stop` halts a running reply.

use crate::agent::{null_sink, Session};
use crate::app::App;
use crate::permissions::{Approval, ApprovalRequest, Approver};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;

const MAX_MESSAGE: usize = 3800;

#[derive(Clone)]
struct Bot {
    client: reqwest::Client,
    base: String,
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

    async fn send(&self, chat: i64, text: &str) {
        let chars: Vec<char> = text.chars().collect();
        for chunk in chars.chunks(MAX_MESSAGE) {
            let _ = self.call("sendMessage", json!({"chat_id": chat, "text": chunk.iter().collect::<String>()})).await;
        }
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
        bot.send(chat, &format!("{}\n\n{}", m.title, m.body)).await;
        let _ = app.store.kv_set("telegram_inbox_last", &m.id.to_string());
    }
}

/// Run the gateway until the task is dropped. `api_base` is normally https://api.telegram.org.
pub async fn run(app: Arc<App>, api_base: String, token: String) {
    let bot = Bot { client: reqwest::Client::builder().timeout(Duration::from_secs(40)).build().expect("http client"), base: format!("{}/bot{token}", api_base.trim_end_matches('/')) };
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
            offset = offset.max(u["update_id"].as_i64().unwrap_or(0) + 1);
            let _ = app.store.kv_set("telegram_offset", &offset.to_string());
            let Some(chat) = u["message"]["chat"]["id"].as_i64() else { continue };
            // Only one-to-one chats: a group would let other members speak as the owner.
            if u["message"]["chat"]["type"].as_str().unwrap_or("private") != "private" {
                continue;
            }
            let text = u["message"]["text"].as_str().map(str::to_string);
            match paired_chat(&app) {
                Some(owner) if owner == chat => match text {
                    Some(t) => handle_message(&app, &bot, &shared, chat, t).await,
                    None => bot.send(chat, "Heron can only read text messages for now.").await,
                },
                Some(_) => {} // someone else: say nothing, do nothing
                None => {
                    let sent = text.as_deref().map(str::trim).unwrap_or("");
                    if sent == "/start" {
                        bot.send(chat, "This Heron is not paired yet. If it is yours, send the pairing code shown in the Heron app under Settings.").await;
                    } else if pairing_locked(&app) {
                        // Guesses are not even looked at until the owner has seen the new code in the app.
                    } else if same_code(sent, &pairing_code(&app)) {
                        let _ = app.store.kv_set("telegram_wrong", "0");
                        let _ = app.store.kv_set("telegram_chat", &chat.to_string());
                        let last_inbox = app.store.inbox(false, 1).ok().and_then(|i| i.first().map(|m| m.id)).unwrap_or(0);
                        let _ = app.store.kv_set("telegram_inbox_last", &last_inbox.to_string());
                        bot.send(chat, "Paired. This chat now talks to your Heron, and nobody else's messages will be answered.").await;
                    } else if wrong_code(&app) {
                        bot.send(chat, "Too many wrong codes. Pairing is locked and the code has been replaced. If this Heron is yours, open it on your computer (Settings) to see the new code.").await;
                    }
                }
            }
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
