//! Telegram gateway: talk to Heron from your phone.
//!
//! Safety model: the bot answers exactly one chat — the one that proved ownership by sending the
//! pairing code shown in the app. Everyone else is ignored. Approvals for file changes and commands
//! can be given by replying yes/no; risky actions, and anything requested after outside content
//! entered the conversation, can only be approved at the computer. `/stop` halts a running reply.

use crate::agent::{null_sink, Session};
use crate::app::App;
use crate::permissions::{Approval, ApprovalRequest, Approver, Risk};
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
    running: Mutex<Option<tokio::task::AbortHandle>>,
}

struct TelegramApprover {
    bot: Bot,
    chat: i64,
    shared: Arc<Shared>,
}

#[async_trait]
impl Approver for TelegramApprover {
    async fn approve(&self, req: &ApprovalRequest) -> Approval {
        if req.risk >= Risk::Danger || req.tainted {
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

/// The six-digit code a new chat must send to become the owner's chat. Created on first use.
pub fn pairing_code(app: &App) -> String {
    if let Some(code) = app.store.kv_get("telegram_code") {
        return code;
    }
    let code = format!("{:06}", uuid::Uuid::new_v4().as_u128() % 1_000_000);
    let _ = app.store.kv_set("telegram_code", &code);
    code
}

pub fn paired_chat(app: &App) -> Option<i64> {
    app.store.kv_get("telegram_chat").and_then(|c| c.parse().ok())
}

pub fn unpair(app: &App) {
    let _ = app.store.kv_set("telegram_chat", "");
    let _ = app.store.kv_set("telegram_code", &format!("{:06}", uuid::Uuid::new_v4().as_u128() % 1_000_000));
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
    // A pending yes/no question takes the next message.
    if let Some(tx) = shared.pending.lock().unwrap().take() {
        let _ = tx.send(matches!(lower.as_str(), "yes" | "y" | "ok" | "allow"));
        return;
    }
    match lower.as_str() {
        "/stop" => {
            let stopped = shared.running.lock().unwrap().take().map(|h| h.abort()).is_some();
            bot.send(chat, if stopped { "Stopped." } else { "Nothing is running." }).await;
            return;
        }
        "/new" => {
            let _ = session_for(app, true);
            bot.send(chat, "Started a new conversation.").await;
            return;
        }
        "/pause" => {
            app.set_paused(true);
            if let Some(h) = shared.running.lock().unwrap().take() {
                h.abort();
            }
            bot.send(chat, "Paused. Heron will start nothing on its own (reminders, triggers, suggestions) until you send /resume.").await;
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
    if shared.running.lock().unwrap().is_some() {
        bot.send(chat, "Still working on your last message. Send /stop to halt it.").await;
        return;
    }
    let (app2, bot2, shared2) = (app.clone(), bot.clone(), shared.clone());
    let task = tokio::spawn(async move {
        let reply = async {
            let mut session = session_for(&app2, false)?;
            let agent = app2.agent(Arc::new(TelegramApprover { bot: bot2.clone(), chat, shared: shared2.clone() }));
            let text_out = agent.run_turn(&mut session, &text, &null_sink()).await?;
            app2.persist_rules();
            agent.learn(&text, &text_out).await;
            Ok::<_, anyhow::Error>(text_out)
        }
        .await;
        shared2.running.lock().unwrap().take();
        match reply {
            Ok(t) if t.trim().is_empty() => bot2.send(chat, "(done)").await,
            Ok(t) => bot2.send(chat, &t).await,
            Err(e) => bot2.send(chat, &format!("Heron could not finish: {e:#}")).await,
        }
    });
    *shared.running.lock().unwrap() = Some(task.abort_handle());
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
                    if text.as_deref().map(str::trim) == Some(pairing_code(&app).as_str()) {
                        let _ = app.store.kv_set("telegram_chat", &chat.to_string());
                        let last_inbox = app.store.inbox(false, 1).ok().and_then(|i| i.first().map(|m| m.id)).unwrap_or(0);
                        let _ = app.store.kv_set("telegram_inbox_last", &last_inbox.to_string());
                        bot.send(chat, "Paired. This chat now talks to your Heron, and nobody else's messages will be answered.").await;
                    } else if text.as_deref() == Some("/start") {
                        bot.send(chat, "This Heron is not paired yet. If it is yours, send the six-digit pairing code shown in the Heron app under Settings.").await;
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

/// Start the gateway in the background if a bot token has been saved.
pub fn spawn_if_configured(app: &Arc<App>) -> bool {
    let cfg = app.cfg();
    match cfg.secret("telegram_token") {
        Some(token) => {
            tokio::spawn(run(app.clone(), cfg.telegram.api_base.clone(), token));
            true
        }
        None => false,
    }
}
