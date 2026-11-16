//! Local-first channel transport primitives.
//!
//! A channel is how a verified person talks to HERON. Connectors are how
//! HERON reads or writes a service. A provider adapter must authenticate the
//! sender *before* constructing an envelope. Data in an envelope is untrusted.
//!
//! SQLite stores accepted inbound message IDs and durable outbound retries.
//! The gateway is not an agent or a permission bypass; platform adapters must
//! route through the existing HERON Agent and Approver.

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::{Arc, Mutex};

const MAX_ID_LEN: usize = 160;
const MAX_TEXT_BYTES: usize = 32 * 1024;
const MAX_PENDING: i64 = 1000;
const MAX_ATTEMPTS: i64 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelKind {
    Telegram,
    Discord,
    Slack,
    WhatsAppCloud,
}

impl ChannelKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Telegram => "telegram",
            Self::Discord => "discord",
            Self::Slack => "slack",
            Self::WhatsAppCloud => "whatsapp_cloud",
        }
    }
}

/// Adapter-reported features must be negotiated, never assumed from a
/// provider name. Future message channels may support different media.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChannelCapabilities {
    pub threads: bool,
    pub attachments: bool,
    pub typing_indicator: bool,
    pub progressive_edits: bool,
    pub remote_approvals: bool,
}

/// A platform adapter must authenticate its incoming messages using the
/// platform's supported SDK/webhook signature and map them to InboundMessage.
/// This trait handles delivery only; it confers no agent tool privileges.
#[async_trait::async_trait]
pub trait ChannelAdapter: Send + Sync {
    fn kind(&self) -> ChannelKind;
    fn capabilities(&self) -> ChannelCapabilities;
    async fn deliver(&self, message: &OutboundMessage) -> Result<Option<String>>;
}

/// Provider-authenticated identity. Account, chat and user are independent:
/// never route a shared-channel event using chat ID alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelIdentity {
    pub kind: ChannelKind,
    pub account_id: String,
    pub chat_id: String,
    pub sender_id: String,
}

impl ChannelIdentity {
    fn validate(&self) -> Result<()> {
        for value in [&self.account_id, &self.chat_id, &self.sender_id] {
            check_id(value)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct InboundMessage {
    pub identity: ChannelIdentity,
    pub provider_message_id: String,
    pub thread_id: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundMessage {
    pub id: i64,
    pub identity: ChannelIdentity,
    pub thread_id: Option<String>,
    pub text: String,
    pub attempts: i64,
}

fn check_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > MAX_ID_LEN
        || value.chars().any(char::is_control)
    {
        bail!("Invalid channel identity or message identifier");
    }
    Ok(())
}

fn check_text(text: &str) -> Result<()> {
    if text.is_empty() || text.len() > MAX_TEXT_BYTES {
        bail!("Channel message must contain 1-32768 UTF-8 bytes");
    }
    Ok(())
}

/// A shared database handle lets several asynchronous adapters atomically
/// deduplicate event IDs and serialize delivery acknowledgements. SQLite is
/// intentionally separate from HERON's agent DB migration lifecycle.
#[derive(Clone)]
pub struct ChannelStore {
    db: Arc<Mutex<Connection>>,
}

impl ChannelStore {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Only the current OS user may inspect the channel spool.
        #[cfg(unix)]
        if !path.exists() {
            use std::os::unix::fs::OpenOptionsExt;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true).mode(0o600);
            match options.open(path) {
                Ok(_) => {}
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(err) => return Err(err.into()),
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        let db = Connection::open(path)?;
        Self::init(db)
    }

    #[cfg(test)]
    fn in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(db: Connection) -> Result<Self> {
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch(
            "PRAGMA journal_mode=DELETE;
             PRAGMA synchronous=FULL;
             PRAGMA secure_delete=ON;
             CREATE TABLE IF NOT EXISTS channel_inbox(
               kind TEXT NOT NULL, account_id TEXT NOT NULL, chat_id TEXT NOT NULL,
               sender_id TEXT NOT NULL, provider_message_id TEXT NOT NULL,
               thread_id TEXT, text TEXT NOT NULL, received_at INTEGER NOT NULL,
               status TEXT NOT NULL DEFAULT 'accepted',
               PRIMARY KEY(kind, account_id, provider_message_id)
             );
             CREATE INDEX IF NOT EXISTS idx_channel_inbox_received ON channel_inbox(received_at);
             CREATE TABLE IF NOT EXISTS channel_outbox(
               id INTEGER PRIMARY KEY,
               kind TEXT NOT NULL, account_id TEXT NOT NULL, chat_id TEXT NOT NULL,
               sender_id TEXT NOT NULL, thread_id TEXT, text TEXT NOT NULL,
               idempotency_key TEXT NOT NULL, queued_at INTEGER NOT NULL,
               next_attempt INTEGER NOT NULL, attempts INTEGER NOT NULL DEFAULT 0,
               status TEXT NOT NULL DEFAULT 'pending',
               provider_message_id TEXT,
               UNIQUE(kind, account_id, idempotency_key)
             );
             CREATE INDEX IF NOT EXISTS idx_channel_outbox_due
               ON channel_outbox(kind, account_id, status, next_attempt, id);",
        )?;
        Ok(Self { db: Arc::new(Mutex::new(db)) })
    }

    /// Refuse unpaired or spoofed senders before writing *any* content.
    /// Returns false for a duplicate provider event; duplicates are scoped
    /// by platform and bot account, not by a global message ID.
    pub fn accept_inbound(
        &self,
        expected_owner: &ChannelIdentity,
        event: &InboundMessage,
        now: i64,
    ) -> Result<bool> {
        expected_owner.validate()?;
        event.identity.validate()?;
        check_id(&event.provider_message_id)?;
        if let Some(thread) = &event.thread_id {
            check_id(thread)?;
        }
        check_text(&event.text)?;
        if expected_owner != &event.identity {
            bail!("Unpaired or mismatched channel sender");
        }
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let added = db.execute(
            "INSERT OR IGNORE INTO channel_inbox
             (kind,account_id,chat_id,sender_id,provider_message_id,thread_id,text,received_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                event.identity.kind.name(), event.identity.account_id,
                event.identity.chat_id, event.identity.sender_id,
                event.provider_message_id, event.thread_id, event.text, now
            ],
        )?;
        Ok(added == 1)
    }

    pub fn complete_inbound(
        &self,
        identity: &ChannelIdentity,
        provider_message_id: &str,
    ) -> Result<()> {
        identity.validate()?;
        check_id(provider_message_id)?;
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.execute(
            "UPDATE channel_inbox SET status='done',text=''
             WHERE kind=?1 AND account_id=?2 AND chat_id=?3 AND sender_id=?4
             AND provider_message_id=?5",
            params![identity.kind.name(), identity.account_id, identity.chat_id,
                    identity.sender_id, provider_message_id],
        )?;
        Ok(())
    }

    /// On restart an accepted-but-incomplete turn is uncertain: it may have
    /// already used tools. Suppress automatic replay and ask the owner to
    /// resend instead of silently executing potentially non-idempotent work.
    pub fn mark_interrupted(&self, kind: ChannelKind, account: &str) -> Result<usize> {
        check_id(account)?;
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.execute(
            "UPDATE channel_inbox SET status='interrupted',text=''
             WHERE kind=?1 AND account_id=?2 AND status='accepted'",
            params![kind.name(), account],
        )?)
    }

    /// Queues an at-least-once response. Callers supply an event-specific
    /// idempotency key; an existing key returns the original entry.
    /// Sending and ack happen separately so a crash does not silently lose
    /// an outbound reply. Provider APIs without idempotent sends can rarely
    /// display a duplicate if HERON crashes *after* sending but before ack.
    pub fn enqueue(
        &self,
        identity: &ChannelIdentity,
        thread_id: Option<&str>,
        text: &str,
        key: &str,
        now: i64,
    ) -> Result<i64> {
        identity.validate()?;
        if let Some(thread) = thread_id {
            check_id(thread)?;
        }
        check_id(key)?;
        check_text(text)?;
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = db.transaction()?;
        // A replay of an existing key must remain idempotent even when
        // the backlog has reached its admission limit.
        let prior: Option<i64> = tx.query_row(
            "SELECT id FROM channel_outbox WHERE kind=?1 AND account_id=?2 AND idempotency_key=?3",
            params![identity.kind.name(), identity.account_id, key],
            |row| row.get(0),
        ).optional()?;
        if let Some(id) = prior {
            tx.commit()?;
            return Ok(id);
        }
        let pending: i64 = tx.query_row(
            "SELECT COUNT(*) FROM channel_outbox WHERE status IN ('pending','leased')",
            [], |row| row.get(0)
        )?;
        if pending >= MAX_PENDING {
            bail!("Channel outbox is full; message not queued");
        }
        tx.execute(
            "INSERT OR IGNORE INTO channel_outbox
             (kind,account_id,chat_id,sender_id,thread_id,text,idempotency_key,queued_at,next_attempt)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?8)",
            params![identity.kind.name(), identity.account_id, identity.chat_id,
                    identity.sender_id, thread_id, text, key, now],
        )?;
        let id = tx.query_row(
            "SELECT id FROM channel_outbox WHERE kind=?1 AND account_id=?2 AND idempotency_key=?3",
            params![identity.kind.name(), identity.account_id, key],
            |row| row.get(0),
        )?;
        tx.commit()?;
        Ok(id)
    }

    /// Lease messages atomically across processes. A crashed sender can retry
    /// only after its lease expires. Telegram does not offer idempotent sends,
    /// so a crash after provider acceptance but before ack can still repeat
    /// the message; every outbound action must remain non-high-impact.
    pub fn due(&self, kind: ChannelKind, account: &str, now: i64, limit: usize) -> Result<Vec<OutboundMessage>> {
        check_id(account)?;
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = db.transaction()?;
        let batch: Vec<OutboundMessage> = {
            let mut stmt = tx.prepare(
                "SELECT id,chat_id,sender_id,thread_id,text,attempts
                 FROM channel_outbox
                 WHERE kind=?1 AND account_id=?2
                   AND status IN ('pending','leased') AND next_attempt<=?3
                 ORDER BY id LIMIT ?4"
            )?;
            let rows = stmt.query_map(params![kind.name(), account, now, limit.min(100) as i64], |row| {
                Ok(OutboundMessage {
                    id: row.get(0)?,
                    identity: ChannelIdentity {
                        kind,
                        account_id: account.to_string(),
                        chat_id: row.get(1)?,
                        sender_id: row.get(2)?,
                    },
                    thread_id: row.get(3)?,
                    text: row.get(4)?,
                    attempts: row.get(5)?,
                })
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        for msg in &batch {
            tx.execute(
                "UPDATE channel_outbox SET status='leased',next_attempt=?2 WHERE id=?1",
                params![msg.id, now.saturating_add(60)],
            )?;
        }
        tx.commit()?;
        Ok(batch)
    }

    /// Acknowledge only the named outbox row for the expected account.
    pub fn delivered(&self, id: i64, identity: &ChannelIdentity, message_id: Option<&str>) -> Result<()> {
        identity.validate()?;
        if let Some(mid) = message_id {
            check_id(mid)?;
        }
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.execute(
            "UPDATE channel_outbox SET status='sent',text='',provider_message_id=?1
             WHERE id=?2 AND kind=?3 AND account_id=?4 AND chat_id=?5 AND sender_id=?6
             AND status='leased'",
            params![message_id, id, identity.kind.name(), identity.account_id,
                    identity.chat_id, identity.sender_id],
        )?;
        Ok(())
    }

    pub fn retry(&self, id: i64, identity: &ChannelIdentity, now: i64) -> Result<()> {
        identity.validate()?;
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let attempts: Option<i64> = db.query_row(
            "SELECT attempts FROM channel_outbox WHERE id=?1 AND kind=?2
             AND account_id=?3 AND chat_id=?4 AND sender_id=?5 AND status='leased'",
            params![id, identity.kind.name(), identity.account_id, identity.chat_id,
                    identity.sender_id],
            |row| row.get(0),
        ).optional()?;
        if let Some(previous) = attempts {
            let next = previous + 1;
            let status = if next >= MAX_ATTEMPTS { "failed" } else { "pending" };
            let delay = 2_i64.pow(next.min(8) as u32).min(300);
            db.execute(
                "UPDATE channel_outbox SET attempts=?1,next_attempt=?2,status=?3,
                 text=CASE WHEN ?3='failed' THEN '' ELSE text END WHERE id=?4",
                params![next, now.saturating_add(delay), status, id],
            )?;
        }
        Ok(())
    }

    /// Local account revocation discards cached messages and unsent responses.
    /// Adapters must also revoke credentials and cancel in-flight work.
    pub fn purge_account(&self, kind: ChannelKind, account: &str) -> Result<()> {
        check_id(account)?;
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = db.transaction()?;
        tx.execute(
            "DELETE FROM channel_inbox WHERE kind=?1 AND account_id=?2",
            params![kind.name(), account],
        )?;
        tx.execute(
            "DELETE FROM channel_outbox WHERE kind=?1 AND account_id=?2",
            params![kind.name(), account],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn count_pending(&self) -> Result<i64> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.query_row("SELECT COUNT(*) FROM channel_outbox WHERE status IN ('pending','leased')", [], |r| r.get(0))
            .context("Cannot inspect channel outbox")
    }

    pub fn count_failed(&self) -> Result<i64> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.query_row("SELECT COUNT(*) FROM channel_outbox WHERE status='failed'", [], |r| r.get(0))
            .context("Cannot inspect failed channel deliveries")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner() -> ChannelIdentity {
        ChannelIdentity {
            kind: ChannelKind::Telegram,
            account_id: "bot-1".into(),
            chat_id: "42".into(),
            sender_id: "42".into(),
        }
    }

    fn inbound(message_id: &str) -> InboundMessage {
        InboundMessage {
            identity: owner(),
            provider_message_id: message_id.into(),
            thread_id: None,
            text: "hello".into(),
        }
    }

    #[test]
    fn pairing_and_dedup_are_scoped_to_account_and_sender() {
        let store = ChannelStore::in_memory().unwrap();
        let expected = owner();
        let event = inbound("update-1");
        assert!(store.accept_inbound(&expected, &event, 123).unwrap());
        assert!(!store.accept_inbound(&expected, &event, 123).unwrap());
        let mut spoofed = event.clone();
        spoofed.identity.sender_id = "attacker".into();
        assert!(store.accept_inbound(&expected, &spoofed, 124).is_err());
        let mut other_account = expected.clone();
        other_account.account_id = "bot-2".into();
        let mut other_event = event.clone();
        other_event.identity = other_account.clone();
        assert!(store.accept_inbound(&other_account, &other_event, 125).unwrap());
        store.complete_inbound(&expected, "update-1").unwrap();
    }

    #[test]
    fn durable_retries_ack_and_revoke_are_isolated() {
        let store = ChannelStore::in_memory().unwrap();
        let expected = owner();
        let id = store.enqueue(&expected, None, "Hello!", "reply-1", 100).unwrap();
        assert_eq!(store.enqueue(&expected, None, "Hello!", "reply-1", 100).unwrap(), id);
        assert_eq!(store.due(ChannelKind::Telegram, "bot-1", 100, 10).unwrap().len(), 1);
        store.retry(id, &expected, 100).unwrap();
        assert_eq!(store.due(ChannelKind::Telegram, "bot-1", 100, 10).unwrap().len(), 0);
        assert_eq!(store.due(ChannelKind::Telegram, "bot-1", 102, 10).unwrap()[0].attempts, 1);
        let mut wrong = expected.clone();
        wrong.account_id = "bot-2".into();
        store.delivered(id, &wrong, Some("remote-id")).unwrap();
        assert_eq!(store.count_pending().unwrap(), 1);
        store.delivered(id, &expected, Some("remote-id")).unwrap();
        assert_eq!(store.count_pending().unwrap(), 0);
        let second = store.enqueue(&expected, None, "Reply", "reply-2", 100).unwrap();
        for i in 0..MAX_ATTEMPTS {
            store.retry(second, &expected, i * 1000).unwrap();
        }
        assert_eq!(store.count_failed().unwrap(), 1);
        store.purge_account(ChannelKind::Telegram, "bot-1").unwrap();
        assert_eq!(store.count_failed().unwrap(), 0);
    }

    #[test]
    fn leased_outbox_cannot_be_claimed_twice_until_expiry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("channels.db");
        let first = ChannelStore::open(&path).unwrap();
        let second = ChannelStore::open(&path).unwrap();
        let owner = owner();
        let id = first.enqueue(&owner, None, "hello", "one", 100).unwrap();
        assert_eq!(first.due(ChannelKind::Telegram, "bot-1", 100, 3).unwrap().len(), 1);
        assert!(second.due(ChannelKind::Telegram, "bot-1", 100, 3).unwrap().is_empty());
        assert!(second.due(ChannelKind::Telegram, "bot-1", 159, 3).unwrap().is_empty());
        assert_eq!(second.due(ChannelKind::Telegram, "bot-1", 160, 3).unwrap().len(), 1);
        first.delivered(id, &owner, Some("remote-1")).unwrap();
        assert_eq!(first.count_pending().unwrap(), 0);
    }

    #[test]
    fn interrupted_requests_are_not_replayed_automatically() {
        let store = ChannelStore::in_memory().unwrap();
        let owner = owner();
        store.accept_inbound(&owner, &inbound("u1"), 100).unwrap();
        assert_eq!(store.mark_interrupted(ChannelKind::Telegram, "bot-1").unwrap(), 1);
        assert_eq!(store.mark_interrupted(ChannelKind::Telegram, "bot-1").unwrap(), 0);
        assert!(!store.accept_inbound(&owner, &inbound("u1"), 101).unwrap());
    }

    #[test]
    fn persistence_prevents_replaying_accepted_updates() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("channels.db");
        let expected = owner();
        {
            let store = ChannelStore::open(&db).unwrap();
            assert!(store.accept_inbound(&expected, &inbound("u9"), 100).unwrap());
            store.enqueue(&expected, None, "Hi", "response-u9", 100).unwrap();
        }
        let reopened = ChannelStore::open(&db).unwrap();
        assert!(!reopened.accept_inbound(&expected, &inbound("u9"), 101).unwrap());
        assert_eq!(reopened.count_pending().unwrap(), 1);
    }

    #[test]
    fn invalid_id_and_messages_are_rejected() {
        let store = ChannelStore::in_memory().unwrap();
        let mut bad = inbound("");
        assert!(store.accept_inbound(&owner(), &bad, 1).is_err());
        bad.provider_message_id = "one".into();
        bad.text = "x".repeat(MAX_TEXT_BYTES + 1);
        assert!(store.accept_inbound(&owner(), &bad, 1).is_err());
        assert!(store.enqueue(&owner(), None, "ok", "", 1).is_err());
    }
}
