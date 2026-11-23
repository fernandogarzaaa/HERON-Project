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
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
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
    /// Proof of the claim made by `due`. Only the holder of the current lease may acknowledge or
    /// reschedule the row, so a worker whose lease expired cannot undo a newer worker's ack.
    pub lease: String,
}

/// What became of a delivery that did not go through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryOutcome {
    Scheduled,
    /// The whole logical message was given up; nothing after it in the chat is held back by it.
    GaveUp,
    /// The caller's lease is no longer current; another worker owns the row now.
    NotHeld,
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

/// Run `attempt` again after a short wait while it fails as busy, until `budget` has passed on `now`.
/// The clock and the wait are given, so a test drives time itself; the real ones are
/// `Instant::now` and `thread::sleep`.
fn retry_while_busy<T>(
    budget: std::time::Duration,
    mut now: impl FnMut() -> std::time::Instant,
    mut sleep: impl FnMut(std::time::Duration),
    mut attempt: impl FnMut() -> Result<T>,
) -> Result<T> {
    let deadline = now() + budget;
    loop {
        match attempt() {
            Err(err) if is_busy(&err) && now() < deadline => {
                sleep(std::time::Duration::from_millis(25));
            }
            other => return other,
        }
    }
}

fn is_busy(err: &anyhow::Error) -> bool {
    matches!(
        err.downcast_ref::<rusqlite::Error>(),
        Some(rusqlite::Error::SqliteFailure(e, _)) if e.code == rusqlite::ErrorCode::DatabaseBusy
    )
}

#[cfg(unix)]
fn open_private_spool_file(path: &Path) -> Result<std::fs::File> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    // Open the final spool component without following a symlink. Keep the
    // handle open through SQLite initialization so permissions are applied
    // to this inode, not to a later pathname replacement.
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    anyhow::ensure!(metadata.is_file(), "The channel spool must be a regular file");
    anyhow::ensure!(metadata.nlink() == 1, "The channel spool must not be hard-linked");
    Ok(file)
}

#[cfg(unix)]
fn set_private_spool_permissions(file: &std::fs::File) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

impl ChannelStore {
    pub fn open(path: &Path) -> Result<Self> {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent)?;
        // SQLite's SQLITE_OPEN_NOFOLLOW also rejects symlinked ancestor
        // components. Resolve the parent once so ordinary macOS paths such as
        // /var/folders (a symlink to /private/var/folders) remain usable.
        // The final component is still opened with O_NOFOLLOW and checked
        // below, so a symlink swapped in at channels.db is rejected.
        let parent = std::fs::canonicalize(parent)?;
        let filename = path
            .file_name()
            .context("The channel spool path must include a file name")?;
        let path = parent.join(filename);
        // Never follow a pre-existing symlink to an arbitrary database or
        // chmod another file through a symlinked channels.db path.
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
                bail!("The channel spool path must be a regular file, not a symlink");
            }
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
        // Apply Unix permissions through a no-follow file handle, never by resolving
        // the pathname again. SQLite's NOFOLLOW flag closes the remaining final-
        // component symlink window between this preparation and its own open.
        #[cfg(unix)]
        let spool_file = open_private_spool_file(&path)?;
        #[cfg(unix)]
        set_private_spool_permissions(&spool_file)?;

        // On Windows, setting up the schema can fail with "database is locked" at once, without
        // waiting on busy_timeout, while another Heron process is committing. Retry it the way
        // busy_timeout would. NOFOLLOW also rejects a symlink at SQLite's open boundary.
        let result = retry_while_busy(
            std::time::Duration::from_secs(5),
            std::time::Instant::now,
            std::thread::sleep,
            || Self::init(Connection::open_with_flags(
                &path,
                OpenFlags::SQLITE_OPEN_READ_WRITE
                    | OpenFlags::SQLITE_OPEN_CREATE
                    | OpenFlags::SQLITE_OPEN_NOFOLLOW,
            )?),
        );
        #[cfg(unix)]
        drop(spool_file);
        result
    }

    #[cfg(test)]
    fn in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut db: Connection) -> Result<Self> {
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch(
            "PRAGMA journal_mode=DELETE;
             PRAGMA synchronous=FULL;
             PRAGMA secure_delete=ON;",
        )?;
        // One process sets up or upgrades the schema at a time. Another opening the spool waits,
        // then finds the work done; checking first and adding a column later would let both see
        // it missing, and the second ALTER would fail.
        let tx = Self::write_tx(&mut db)?;
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS channel_inbox(
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
               lease_token TEXT,
               message_key TEXT,
               UNIQUE(kind, account_id, idempotency_key)
             );
             CREATE INDEX IF NOT EXISTS idx_channel_outbox_due
               ON channel_outbox(kind, account_id, status, next_attempt, id);",
        )?;
        for column in ["lease_token", "message_key"] {
            let present: bool = tx.query_row(
                "SELECT COUNT(*) FROM pragma_table_info('channel_outbox') WHERE name=?1",
                [column], |r| r.get::<_, i64>(0),
            )? > 0;
            if !present {
                tx.execute_batch(&format!("ALTER TABLE channel_outbox ADD COLUMN {column} TEXT"))?;
            }
        }
        Self::backfill_message_keys(&tx)?;
        tx.commit()?;
        Ok(Self { db: Arc::new(Mutex::new(db)) })
    }

    /// Chunks queued before message_key existed carry it only inside their own key ("reply:0",
    /// "reply:1"). Without it, giving up a message would fail its head chunk and still send the tail.
    fn backfill_message_keys(tx: &rusqlite::Transaction<'_>) -> Result<()> {
        let rows: Vec<(i64, String)> = tx
            .prepare("SELECT id, idempotency_key FROM channel_outbox WHERE message_key IS NULL")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (id, key) in rows {
            if let Some((message, index)) = key.rsplit_once(':') {
                if !message.is_empty() && !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit()) {
                    tx.execute("UPDATE channel_outbox SET message_key=?1 WHERE id=?2", params![message, id])?;
                }
            }
        }
        Ok(())
    }

    /// Writers take the database lock up front. A deferred transaction that reads first and
    /// upgrades later fails at once with "database is locked" when another process got there
    /// between the two, without waiting for busy_timeout.
    fn write_tx(db: &mut Connection) -> rusqlite::Result<rusqlite::Transaction<'_>> {
        db.transaction_with_behavior(TransactionBehavior::Immediate)
    }

    /// Fails every remaining chunk of the logical message `row` belongs to.
    fn fail_message(tx: &rusqlite::Transaction<'_>, row: i64) -> rusqlite::Result<()> {
        tx.execute(
            "UPDATE channel_outbox SET status='failed',text='',lease_token=NULL
             WHERE status IN ('pending','leased') AND (kind,account_id,COALESCE(message_key,idempotency_key)) =
               (SELECT kind,account_id,COALESCE(message_key,idempotency_key) FROM channel_outbox WHERE id=?1)",
            params![row],
        )?;
        Ok(())
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

    /// A whole logical message is stored in a single transaction. A two-part
    /// reply cannot leave its prefix behind when the queue is almost full.
    /// Key is stable across caller retries (inbox ID), or random for one-off
    /// conversational responses. The suffix is deterministic per chunk.
    pub fn enqueue_parts(
        &self,
        identity: &ChannelIdentity,
        thread_id: Option<&str>,
        chunks: &[String],
        key: &str,
        now: i64,
    ) -> Result<Vec<i64>> {
        identity.validate()?;
        if let Some(thread) = thread_id { check_id(thread)?; }
        check_id(key)?;
        if key.len() > MAX_ID_LEN.saturating_sub(12) ||
            chunks.is_empty() || chunks.len() > 512 {
            bail!("Invalid logical outbound message");
        }
        for part in chunks { check_text(part)?; }
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = Self::write_tx(&mut db)?;
        let keys: Vec<String> = (0..chunks.len())
            .map(|i| format!("{key}:{i}")).collect();
        let mut missing = 0;
        for (index, chunk_key) in keys.iter().enumerate() {
            let prior: Option<(String, String, String)> = tx.query_row(
                "SELECT chat_id,sender_id,text FROM channel_outbox
                 WHERE kind=?1 AND account_id=?2 AND idempotency_key=?3",
                params![identity.kind.name(), identity.account_id, chunk_key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            ).optional()?;
            if let Some((chat,sender,text)) = prior {
                if chat != identity.chat_id || sender != identity.sender_id
                    || (!text.is_empty() && text != chunks[index]) {
                    bail!("Outbound key was reused for different message content or identity");
                }
            } else {
                missing += 1;
            }
        }
        let occupied: i64 = tx.query_row(
            "SELECT COUNT(*) FROM channel_outbox WHERE status IN ('pending','leased')",
            [], |r| r.get(0)
        )?;
        if occupied + missing > MAX_PENDING {
            bail!("Channel outbox lacks capacity for the complete reply");
        }
        let mut ids = Vec::with_capacity(chunks.len());
        for (text, chunk_key) in chunks.iter().zip(keys.iter()) {
            tx.execute(
                "INSERT OR IGNORE INTO channel_outbox
                 (kind,account_id,chat_id,sender_id,thread_id,text,idempotency_key,queued_at,next_attempt,message_key)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?8,?9)",
                params![identity.kind.name(), identity.account_id, identity.chat_id,
                        identity.sender_id, thread_id, text, chunk_key, now, key],
            )?;
            ids.push(tx.query_row(
                "SELECT id FROM channel_outbox
                 WHERE kind=?1 AND account_id=?2 AND idempotency_key=?3",
                params![identity.kind.name(), identity.account_id, chunk_key],
                |row| row.get(0),
            )?);
        }
        tx.commit()?;
        Ok(ids)
    }

    /// Compatibility API for one-part messages.
    pub fn enqueue(
        &self,
        identity: &ChannelIdentity,
        thread_id: Option<&str>,
        text: &str,
        key: &str,
        now: i64,
    ) -> Result<i64> {
        let parts = [text.to_owned()];
        Ok(self.enqueue_parts(identity, thread_id, &parts, key, now)?[0])
    }

    /// Lease messages atomically across processes. A crashed sender can retry
    /// only after its lease expires. Telegram does not offer idempotent sends,
    /// so a crash after provider acceptance but before ack can still repeat
    /// the message; every outbound action must remain non-high-impact.
    pub fn due(&self, kind: ChannelKind, account: &str, now: i64, limit: usize) -> Result<Vec<OutboundMessage>> {
        check_id(account)?;
        let lease = uuid::Uuid::new_v4().simple().to_string();
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = Self::write_tx(&mut db)?;
        let batch: Vec<OutboundMessage> = {
            let mut stmt = tx.prepare(
                "SELECT id,chat_id,sender_id,thread_id,text,attempts
                 FROM channel_outbox
                 WHERE kind=?1 AND account_id=?2
                   AND status IN ('pending','leased') AND next_attempt<=?3
                   AND NOT EXISTS (
                     SELECT 1 FROM channel_outbox earlier
                     WHERE earlier.kind=channel_outbox.kind
                       AND earlier.account_id=channel_outbox.account_id
                       AND earlier.chat_id=channel_outbox.chat_id
                       AND earlier.id<channel_outbox.id
                       AND earlier.status IN ('pending','leased')
                   )
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
                    lease: lease.clone(),
                })
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        for msg in &batch {
            tx.execute(
                "UPDATE channel_outbox SET status='leased',next_attempt=?2,lease_token=?3 WHERE id=?1",
                params![msg.id, now.saturating_add(60), lease],
            )?;
        }
        tx.commit()?;
        Ok(batch)
    }

    /// Acknowledge only the named outbox row for the expected account.
    pub fn delivered(&self, id: i64, identity: &ChannelIdentity, lease: &str, message_id: Option<&str>) -> Result<()> {
        identity.validate()?;
        if let Some(mid) = message_id {
            check_id(mid)?;
        }
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.execute(
            "UPDATE channel_outbox SET status='sent',text='',provider_message_id=?1,lease_token=NULL
             WHERE id=?2 AND kind=?3 AND account_id=?4 AND chat_id=?5 AND sender_id=?6
             AND status='leased' AND lease_token=?7",
            params![message_id, id, identity.kind.name(), identity.account_id,
                    identity.chat_id, identity.sender_id, lease],
        )?;
        Ok(())
    }

    /// Reschedule a delivery that failed for a reason that may pass (network, 5xx, 429).
    /// `not_before` carries the provider's own retry-after, when it gave one. After
    /// `MAX_ATTEMPTS` the whole logical message is given up together.
    pub fn retry(&self, id: i64, identity: &ChannelIdentity, lease: &str, now: i64, not_before: Option<i64>) -> Result<RetryOutcome> {
        identity.validate()?;
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = Self::write_tx(&mut db)?;
        let Some(previous) = Self::held_attempts(&tx, id, identity, lease)? else {
            return Ok(RetryOutcome::NotHeld);
        };
        let next = previous + 1;
        let outcome = if next >= MAX_ATTEMPTS {
            Self::fail_message(&tx, id)?;
            RetryOutcome::GaveUp
        } else {
            let backoff = now.saturating_add(2_i64.pow(next.min(8) as u32).min(300));
            tx.execute(
                "UPDATE channel_outbox SET attempts=?1,next_attempt=?2,status='pending',lease_token=NULL WHERE id=?3",
                params![next, backoff.max(not_before.unwrap_or(0)), id],
            )?;
            RetryOutcome::Scheduled
        };
        tx.commit()?;
        Ok(outcome)
    }

    /// Give up at once on a delivery the provider refused for good (a 4xx other than 429),
    /// together with the rest of its logical message.
    pub fn fail(&self, id: i64, identity: &ChannelIdentity, lease: &str) -> Result<RetryOutcome> {
        identity.validate()?;
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = Self::write_tx(&mut db)?;
        if Self::held_attempts(&tx, id, identity, lease)?.is_none() {
            return Ok(RetryOutcome::NotHeld);
        }
        Self::fail_message(&tx, id)?;
        tx.commit()?;
        Ok(RetryOutcome::GaveUp)
    }

    fn held_attempts(tx: &rusqlite::Transaction<'_>, id: i64, identity: &ChannelIdentity, lease: &str) -> rusqlite::Result<Option<i64>> {
        tx.query_row(
            "SELECT attempts FROM channel_outbox WHERE id=?1 AND kind=?2
             AND account_id=?3 AND chat_id=?4 AND sender_id=?5 AND status='leased' AND lease_token=?6",
            params![id, identity.kind.name(), identity.account_id, identity.chat_id,
                    identity.sender_id, lease],
            |row| row.get(0),
        ).optional()
    }

    /// One-time migration safety: old gateway builds put approval questions
    /// into the persistent spool. They are never valid after a process restart;
    /// discard them before forwarding any queued outgoing messages.
    pub fn discard_stale_approval_prompts(&self, kind: ChannelKind, account: &str) -> Result<usize> {
        check_id(account)?;
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.execute(
            "UPDATE channel_outbox SET status='failed',text=''
             WHERE kind=?1 AND account_id=?2 AND status IN ('pending','leased')
               AND text LIKE 'Heron wants permission (%'",
            params![kind.name(), account],
        )?)
    }

    /// Local account revocation discards cached messages and unsent responses.
    /// Adapters must also revoke credentials and cancel in-flight work.
    pub fn purge_account(&self, kind: ChannelKind, account: &str) -> Result<()> {
        check_id(account)?;
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = Self::write_tx(&mut db)?;
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

    #[test]
    fn a_worker_whose_lease_expired_cannot_undo_the_next_workers_ack() {
        let store = ChannelStore::in_memory().unwrap();
        let id = store.enqueue(&owner(), None, "hello", "one", 100).unwrap();
        let stale = store.due(ChannelKind::Telegram, "bot-1", 100, 1).unwrap().remove(0);
        let current = store.due(ChannelKind::Telegram, "bot-1", 161, 1).unwrap().remove(0);
        assert_eq!(store.retry(id, &owner(), &stale.lease, 161, None).unwrap(), RetryOutcome::NotHeld);
        assert_eq!(store.fail(id, &owner(), &stale.lease).unwrap(), RetryOutcome::NotHeld);
        store.delivered(id, &owner(), &current.lease, Some("tg-1")).unwrap();
        assert_eq!(store.count_pending().unwrap(), 0, "delivered once, not queued again");
        assert_eq!(store.count_failed().unwrap(), 0);
    }

    #[test]
    fn a_reply_that_keeps_failing_is_given_up_whole_and_never_sent_tail_first() {
        let store = ChannelStore::in_memory().unwrap();
        store.enqueue_parts(&owner(), None, &["head".into(), "tail".into()], "reply", 100).unwrap();
        let mut at = 100;
        let mut outcome = RetryOutcome::Scheduled;
        for _ in 0..MAX_ATTEMPTS {
            let claim = store.due(ChannelKind::Telegram, "bot-1", at, 10).unwrap();
            assert_eq!(claim.len(), 1, "only the head is ever offered");
            assert_eq!(claim[0].text, "head");
            outcome = store.retry(claim[0].id, &owner(), &claim[0].lease, at, None).unwrap();
            at += 1000;
        }
        assert_eq!(outcome, RetryOutcome::GaveUp);
        assert!(store.due(ChannelKind::Telegram, "bot-1", at, 10).unwrap().is_empty());
        assert_eq!(store.count_failed().unwrap(), 2);
    }

    #[test]
    fn a_refused_reply_is_given_up_at_once_and_a_rate_limit_is_waited_out() {
        let store = ChannelStore::in_memory().unwrap();
        store.enqueue_parts(&owner(), None, &["a".into(), "b".into()], "refused", 100).unwrap();
        let claim = store.due(ChannelKind::Telegram, "bot-1", 100, 1).unwrap().remove(0);
        assert_eq!(store.fail(claim.id, &owner(), &claim.lease).unwrap(), RetryOutcome::GaveUp);
        assert_eq!(store.count_failed().unwrap(), 2);

        store.enqueue(&owner(), None, "later", "limited", 200).unwrap();
        let claim = store.due(ChannelKind::Telegram, "bot-1", 200, 1).unwrap().remove(0);
        store.retry(claim.id, &owner(), &claim.lease, 200, Some(320)).unwrap();
        assert!(store.due(ChannelKind::Telegram, "bot-1", 319, 1).unwrap().is_empty());
        assert_eq!(store.due(ChannelKind::Telegram, "bot-1", 320, 1).unwrap().len(), 1);
    }

    #[test]
    fn two_processes_sharing_the_spool_wait_for_each_other_instead_of_failing_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("channels.db");
        ChannelStore::open(&path).unwrap();
        // Before the fix, about 35 of 1,200 operations failed after a millisecond. Under this much
        // contention a slow disk (Windows CI) can still starve a writer past busy_timeout; that is
        // the timeout working, so only a failure that did not wait it out counts.
        // Both processes start up first, as Heron's do, then work at the same time.
        let early = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let started = std::sync::Arc::new(std::sync::Barrier::new(2));
        let workers: Vec<_> = (0..2)
            .map(|w| {
                let (path, early, started) = (path.clone(), early.clone(), started.clone());
                std::thread::spawn(move || {
                    let start = std::time::Instant::now();
                    let opened = ChannelStore::open(&path);
                    started.wait();
                    let store = match opened {
                        Ok(store) => store,
                        Err(e) => {
                            early.lock().unwrap().push(format!("open: {e:#} after {:?}", start.elapsed()));
                            return;
                        }
                    };
                    let timed = |what: &str, op: &dyn Fn() -> Result<()>| {
                        let start = std::time::Instant::now();
                        if let Err(e) = op() {
                            if start.elapsed() < std::time::Duration::from_secs(4) {
                                early.lock().unwrap().push(format!("{what}: {e:#} after {:?}", start.elapsed()));
                            }
                        }
                    };
                    for i in 0..300 {
                        timed("enqueue", &|| store.enqueue(&owner(), None, "hi", &format!("w{w}-{i}"), 0).map(|_| ()));
                        timed("due", &|| store.due(ChannelKind::Telegram, "bot-1", 0, 1).map(|_| ()));
                    }
                })
            })
            .collect();
        for w in workers {
            w.join().unwrap();
        }
        let early = early.lock().unwrap();
        assert!(early.is_empty(), "{} operations failed without waiting, first: {:?}", early.len(), early.first());
    }

    fn sqlite_error(code: std::os::raw::c_int) -> anyhow::Error {
        rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(code), None).into()
    }

    #[test]
    fn only_a_busy_database_counts_as_busy() {
        assert!(is_busy(&sqlite_error(rusqlite::ffi::SQLITE_BUSY)));
        assert!(!is_busy(&sqlite_error(rusqlite::ffi::SQLITE_CONSTRAINT)));
        assert!(!is_busy(&anyhow::anyhow!("database is locked")));
    }

    #[test]
    fn busy_attempts_are_retried_within_the_budget_and_nothing_else_is() {
        // Time is the test's: a sleep moves a fake clock by exactly what it asked for, so how loaded
        // the machine is cannot change how many attempts fit in the budget.
        let base = std::time::Instant::now();
        let elapsed = std::cell::Cell::new(std::time::Duration::ZERO);
        let now = || base + elapsed.get();
        let sleep = |wait: std::time::Duration| elapsed.set(elapsed.get() + wait);
        let budget = std::time::Duration::from_millis(300);

        let mut calls = 0;
        let result = retry_while_busy(budget, now, sleep, || {
            calls += 1;
            if calls < 4 { Err(sqlite_error(rusqlite::ffi::SQLITE_BUSY)) } else { Ok(calls) }
        });
        assert_eq!(result.unwrap(), 4);
        assert_eq!(elapsed.get(), std::time::Duration::from_millis(75), "it waited between attempts only");

        elapsed.set(std::time::Duration::ZERO);
        let mut calls = 0;
        let other = retry_while_busy(budget, now, sleep, || -> Result<()> {
            calls += 1;
            Err(sqlite_error(rusqlite::ffi::SQLITE_CONSTRAINT))
        });
        assert!(other.is_err());
        assert_eq!(calls, 1, "a non-busy error was retried");
        assert_eq!(elapsed.get(), std::time::Duration::ZERO, "and was not waited for");

        elapsed.set(std::time::Duration::ZERO);
        let mut calls = 0;
        let stuck = retry_while_busy(budget, now, sleep, || -> Result<()> {
            calls += 1;
            Err(sqlite_error(rusqlite::ffi::SQLITE_BUSY))
        });
        assert!(stuck.is_err());
        // It gives up when the budget is spent, not before and not long after: attempts at 0, 25, ...
        // 300 ms.
        assert_eq!(elapsed.get(), budget);
        assert_eq!(calls, 13);
    }

    #[test]
    fn two_processes_upgrading_an_old_spool_at_once_both_open_it() {
        for _ in 0..20 {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("channels.db");
            ChannelStore::open(&path).unwrap();
            Connection::open(&path).unwrap()
                .execute_batch("ALTER TABLE channel_outbox DROP COLUMN lease_token; ALTER TABLE channel_outbox DROP COLUMN message_key;")
                .unwrap();
            let go = std::sync::Arc::new(std::sync::Barrier::new(2));
            let openers: Vec<_> = (0..2)
                .map(|_| {
                    let (path, go) = (path.clone(), go.clone());
                    std::thread::spawn(move || {
                        go.wait();
                        ChannelStore::open(&path).map(|_| ()).map_err(|e| format!("{e:#}"))
                    })
                })
                .collect();
            for opener in openers {
                opener.join().unwrap().expect("a second Heron process failed to open a spool being upgraded");
            }
        }
    }

    #[test]
    fn opening_the_spool_waits_while_another_process_is_writing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("channels.db");
        ChannelStore::open(&path).unwrap();
        // Another process in the middle of a commit: nobody may read or write until it finishes.
        let writer = Connection::open(&path).unwrap();
        writer.execute_batch("BEGIN EXCLUSIVE").unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            writer.execute_batch("COMMIT").unwrap();
        });
        let opened = ChannelStore::open(&path);
        release.join().unwrap();
        opened.expect("a second Heron process could not open the spool while the first was writing");
    }

    #[test]
    fn replies_queued_before_the_upgrade_are_still_given_up_whole() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("channels.db");
        ChannelStore::open(&path).unwrap().enqueue_parts(&owner(), None, &["head".into(), "tail".into()], "old-reply", 100).unwrap();
        // What an older build left behind: no lease or message-key columns, and keys of every shape.
        let old = Connection::open(&path).unwrap();
        old.execute_batch("ALTER TABLE channel_outbox DROP COLUMN lease_token; ALTER TABLE channel_outbox DROP COLUMN message_key;").unwrap();
        for key in ["note:x1", "note:", ":0", "plain"] {
            old.execute(
                "INSERT INTO channel_outbox(kind,account_id,chat_id,sender_id,text,idempotency_key,queued_at,next_attempt)
                 VALUES ('telegram','bot-2','42','42','x',?1,100,100)",
                [key],
            ).unwrap();
        }
        drop(old);
        let store = ChannelStore::open(&path).unwrap();
        let raw = Connection::open(&path).unwrap();
        let keys: Vec<(String, Option<String>)> = raw
            .prepare("SELECT idempotency_key, message_key FROM channel_outbox ORDER BY id").unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap()
            .collect::<rusqlite::Result<_>>().unwrap();
        let some = |k: &str| Some(k.to_string());
        assert_eq!(keys, [
            ("old-reply:0".into(), some("old-reply")), ("old-reply:1".into(), some("old-reply")),
            ("note:x1".into(), None), ("note:".into(), None), (":0".into(), None), ("plain".into(), None),
        ]);
        let head = store.due(ChannelKind::Telegram, "bot-1", 100, 1).unwrap().remove(0);
        assert_eq!(head.text, "head");
        assert_eq!(store.fail(head.id, &owner(), &head.lease).unwrap(), RetryOutcome::GaveUp);
        assert!(store.due(ChannelKind::Telegram, "bot-1", 100, 5).unwrap().is_empty(), "the tail was sent without its head");
        assert_eq!(store.count_failed().unwrap(), 2);
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
        let claim = store.due(ChannelKind::Telegram, "bot-1", 100, 10).unwrap();
        assert_eq!(claim.len(), 1);
        store.retry(id, &expected, &claim[0].lease, 100, None).unwrap();
        assert_eq!(store.due(ChannelKind::Telegram, "bot-1", 100, 10).unwrap().len(), 0);
        let again = store.due(ChannelKind::Telegram, "bot-1", 102, 10).unwrap();
        assert_eq!(again[0].attempts, 1);
        let mut wrong = expected.clone();
        wrong.account_id = "bot-2".into();
        store.delivered(id, &wrong, &again[0].lease, Some("remote-id")).unwrap();
        assert_eq!(store.count_pending().unwrap(), 1);
        store.delivered(id, &expected, &again[0].lease, Some("remote-id")).unwrap();
        assert_eq!(store.count_pending().unwrap(), 0);
        let second = store.enqueue(&expected, None, "Reply", "reply-2", 100).unwrap();
        for i in 0..MAX_ATTEMPTS {
            let at = 100 + i * 1000;
            let claimed = store.due(ChannelKind::Telegram, "bot-1", at, 10).unwrap();
            assert_eq!(claimed.len(), 1);
            assert_eq!(claimed[0].id, second);
            store.retry(second, &expected, &claimed[0].lease, at, None).unwrap();
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
        let reclaimed = second.due(ChannelKind::Telegram, "bot-1", 160, 3).unwrap();
        assert_eq!(reclaimed.len(), 1);
        first.delivered(id, &owner, &reclaimed[0].lease, Some("remote-1")).unwrap();
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
    fn partial_multi_chunk_queue_admission_rolls_back_completely() {
        let q = ChannelStore::in_memory().unwrap();
        let identity = owner();
        // Use direct insertion to reproduce the edge at max capacity without
        // allocating hundreds of large chunks.
        {
            let db = q.db.lock().unwrap();
            for i in 0..(MAX_PENDING - 1) {
                db.execute(
                    "INSERT INTO channel_outbox(kind,account_id,chat_id,sender_id,
                    thread_id,text,idempotency_key,queued_at,next_attempt)
                    VALUES ('telegram','bot-1','42','42',NULL,'p',?1,100,100)",
                    [format!("fill-{i}")],
                ).unwrap();
            }
        }
        assert!(q.enqueue_parts(&identity, None,
            &["one".into(), "two".into()], "logical", 100).is_err());
        let db = q.db.lock().unwrap();
        let count: i64 = db.query_row(
            "SELECT COUNT(*) FROM channel_outbox WHERE idempotency_key LIKE 'logical:%'",
            [], |r| r.get(0)
        ).unwrap();
        assert_eq!(count, 0, "no partial reply should be committed");
    }

    #[test]
    fn a_delayed_first_chunk_blocks_later_chunks_in_the_same_chat() {
        let q = ChannelStore::in_memory().unwrap();
        let identity = owner();
        let ids = q.enqueue_parts(&identity, None,
            &["first".into(), "second".into()], "reply", 100).unwrap();
        let head = q.due(ChannelKind::Telegram, "bot-1", 100, 40).unwrap();
        assert_eq!(head.len(), 1);
        q.retry(ids[0], &identity, &head[0].lease, 100, None).unwrap();
        assert!(q.due(ChannelKind::Telegram, "bot-1", 101, 40).unwrap().is_empty());
        let retried = q.due(ChannelKind::Telegram, "bot-1", 102, 40).unwrap();
        assert_eq!(retried.len(), 1);
        assert_eq!(retried[0].id, ids[0]);
        q.delivered(ids[0], &identity, &retried[0].lease, Some("tg-1")).unwrap();
        let second = q.due(ChannelKind::Telegram, "bot-1", 103, 40).unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].id, ids[1]);
    }

    #[test]
    fn stale_approval_prompts_are_discarded_before_reconnect() {
        let spool = ChannelStore::in_memory().unwrap();
        let owner = owner();
        let sent = spool.enqueue(&owner, None, "Heron wants permission (write_file):\\nReply yes or no.",
            "old-approval", 100).unwrap();
        let ordinary = spool.enqueue(&owner, None, "Ordinary notification", "safe-notice", 100).unwrap();
        assert_eq!(spool.discard_stale_approval_prompts(ChannelKind::Telegram, "bot-1").unwrap(), 1);
        let due = spool.due(ChannelKind::Telegram, "bot-1", 100, 4).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, ordinary);
        assert_ne!(due[0].id, sent);
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

    #[cfg(unix)]
    #[test]
    fn opens_spool_through_a_symlinked_parent_directory() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let real = temp.path().join("real");
        let alias = temp.path().join("alias");
        std::fs::create_dir(&real).unwrap();
        symlink(&real, &alias).unwrap();

        let _store = ChannelStore::open(&alias.join("channels.db")).unwrap();
        assert!(real.join("channels.db").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn spool_permissions_apply_to_the_open_inode_not_a_replaced_symlink() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("channels.db");
        let victim = temp.path().join("victim.txt");
        std::fs::write(&path, b"spool").unwrap();
        std::fs::write(&victim, b"private").unwrap();
        std::fs::set_permissions(&victim, std::fs::Permissions::from_mode(0o644)).unwrap();

        let file = open_private_spool_file(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        symlink(&victim, &path).unwrap();

        set_private_spool_permissions(&file).unwrap();
        assert_eq!(
            std::fs::metadata(&victim).unwrap().permissions().mode() & 0o777,
            0o644,
            "replacing the spool pathname must not chmod the symlink target"
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_spool_paths() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("unrelated.sqlite3");
        std::fs::write(&target, b"not a spool").unwrap();
        let alias = temp.path().join("channels.db");
        symlink(&target, &alias).unwrap();
        assert!(ChannelStore::open(&alias).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"not a spool");
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
