//! Cross-platform, local-first messaging gateway primitives.
//!
//! A channel is how a person reaches HERON. A connector is how HERON reaches
//! external services. Neither a queued message nor an adapter can bypass the
//! existing agent/tool permission system.
//!
//! P0 provides an identity-scoped durable queue, not an enabled remote gateway.
//! Adapters must verify platform signatures/tokens and identity before calling
//! `enqueue_inbound`. Actual platform send/receive stays disabled until tested.
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::path::Path;
use std::sync::Mutex;

const MAX_MESSAGE_BYTES: usize = 16 * 1024;
const MAX_PENDING: i64 = 5000;
const MAX_ATTEMPTS: i64 = 5;
const MAX_CLAIM: usize = 32;
const MAX_ID_BYTES: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    Telegram,
    Discord,
    Slack,
    WhatsappCloud,
}

impl Channel {
    fn as_str(self) -> &'static str {
        match self {
            Self::Telegram => "telegram",
            Self::Discord => "discord",
            Self::Slack => "slack",
            Self::WhatsappCloud => "whatsapp_cloud",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Inbound,
    Outbound,
}

impl Direction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Inbound => "inbound",
            Self::Outbound => "outbound",
        }
    }
}

/// Untrusted messages are always tagged with the platform, principal, scope,
/// and provider event ID. The adapter MUST not treat message text as commands
/// from HERON's developer/system context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelEnvelope {
    pub channel: Channel,
    pub profile_id: String,
    pub account_id: String,
    pub chat_id: String,
    pub sender_id: String,
    pub event_id: String,
    pub thread_id: Option<String>,
    pub text: String,
    pub received_at: i64,
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ID_BYTES
        && !value.chars().any(|c| c.is_control())
}

impl ChannelEnvelope {
    pub fn validate(&self) -> Result<()> {
        for value in [
            &self.profile_id,
            &self.account_id,
            &self.chat_id,
            &self.sender_id,
            &self.event_id,
        ] {
            if !valid_id(value) {
                bail!("A messaging identity or event ID is invalid.");
            }
        }
        if self.thread_id.as_deref().is_some_and(|v| !valid_id(v)) {
            bail!("Invalid message thread ID.");
        }
        if self.text.trim().is_empty() || self.text.len() > MAX_MESSAGE_BYTES {
            bail!("Message is empty or exceeds the 16 KiB text limit.");
        }
        Ok(())
    }
}

/// A channel adapter must supply a binding taken from a separately verified
/// pairing/authorization process, never from message content. The queue checks
/// that the incoming account, profile, sender, and channel match the binding.
#[derive(Debug, Clone)]
pub struct VerifiedBinding {
    pub channel: Channel,
    pub profile_id: String,
    pub account_id: String,
    pub sender_id: String,
}

impl VerifiedBinding {
    pub fn accepts(&self, envelope: &ChannelEnvelope) -> bool {
        self.channel == envelope.channel
            && self.profile_id == envelope.profile_id
            && self.account_id == envelope.account_id
            && self.sender_id == envelope.sender_id
            && valid_id(&self.profile_id)
            && valid_id(&self.account_id)
            && valid_id(&self.sender_id)
    }
}

/// Adapters negotiate their supported behavior; the router must not assume
/// that a platform supports edits, threads, files, or native approvals.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub threads: bool,
    pub attachments: bool,
    pub progressive_edits: bool,
    pub remote_approvals: bool,
}

#[async_trait]
pub trait ChannelAdapter: Send + Sync {
    fn channel(&self) -> Channel;
    fn capabilities(&self) -> Capabilities;
    async fn send(&self, envelope: &ChannelEnvelope) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct LeasedMessage {
    pub id: i64,
    pub direction: Direction,
    pub envelope: ChannelEnvelope,
    pub attempts: i64,
}

/// On-disk SQLite spool. A local queue entry is *not* an authorization grant.
/// Delivery is at-least-once across crashes and MUST use a provider message
/// ID/idempotency key to avoid repeating high-impact actions on retry.
pub struct ChannelQueue {
    db: Mutex<Connection>,
}

impl ChannelQueue {
    pub fn open(home: &Path) -> Result<Self> {
        fs::create_dir_all(home).context("Cannot create channel data directory")?;
        let path = home.join("channel-gateway.sqlite3");
        // SQLite normally creates files using the process umask; force private
        // permissions even if the user has a permissive umask.
        let mut opts = OpenOptions::new();
        opts.create(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            opts.mode(0o600);
            let _file = opts.open(&path)?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }
        #[cfg(not(unix))]
        {
            let _file = opts.open(&path)?;
        }
        let db = Connection::open(path)?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        // The rollback journal is local and short-lived; no WAL sidecar should
        // persist message contents after completion.
        db.execute_batch(
            "PRAGMA journal_mode=DELETE;
             PRAGMA synchronous=FULL;
             CREATE TABLE IF NOT EXISTS channel_messages (
                 id INTEGER PRIMARY KEY,
                 profile TEXT NOT NULL,
                 channel TEXT NOT NULL,
                 account TEXT NOT NULL,
                 direction TEXT NOT NULL,
                 event_id TEXT NOT NULL,
                 payload TEXT NOT NULL,
                 status TEXT NOT NULL CHECK(status IN ('pending','leased','done','dead')),
                 attempts INTEGER NOT NULL DEFAULT 0,
                 next_at INTEGER NOT NULL DEFAULT 0,
                 lease_until INTEGER NOT NULL DEFAULT 0,
                 created_at INTEGER NOT NULL,
                 UNIQUE(profile, channel, account, direction, event_id)
             );
             CREATE INDEX IF NOT EXISTS idx_channel_ready
               ON channel_messages(status, next_at, lease_until, id);",
        )?;
        Ok(Self { db: Mutex::new(db) })
    }

    /// Inbound admission requires an independently authenticated sender
    /// binding. Duplicates are ignored within the profile/account/channel,
    /// including after a completed row has had its plaintext payload erased.
    pub fn enqueue_inbound(
        &self,
        binding: &VerifiedBinding,
        envelope: &ChannelEnvelope,
    ) -> Result<bool> {
        if !binding.accepts(envelope) {
            bail!("Unpaired or mismatched sender is not authorized.");
        }
        self.enqueue(Direction::Inbound, envelope)
    }

    /// Outbound messages must already have passed HERON's existing tool and
    /// profile permission policy. This only saves the delivery attempt.
    pub fn enqueue_outbound(&self, envelope: &ChannelEnvelope) -> Result<bool> {
        self.enqueue(Direction::Outbound, envelope)
    }

    fn enqueue(&self, direction: Direction, envelope: &ChannelEnvelope) -> Result<bool> {
        envelope.validate()?;
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = db.transaction()?;
        // Existing duplicates must be no-ops even when the queue is full.
        let already: i64 = tx.query_row(
            "SELECT COUNT(*) FROM channel_messages WHERE profile=?1 AND channel=?2
             AND account=?3 AND direction=?4 AND event_id=?5",
            params![envelope.profile_id, envelope.channel.as_str(), envelope.account_id,
                    direction.as_str(), envelope.event_id],
            |r| r.get(0),
        )?;
        if already != 0 {
            tx.commit()?;
            return Ok(false);
        }
        let pending: i64 = tx.query_row(
            "SELECT COUNT(*) FROM channel_messages WHERE status IN ('pending','leased')",
            [],
            |r| r.get(0),
        )?;
        if pending >= MAX_PENDING {
            bail!("Channel queue full; refusing to silently drop a new message.");
        }
        let now = crate::util::now_ts();
        tx.execute(
            "INSERT INTO channel_messages
             (profile,channel,account,direction,event_id,payload,status,created_at,next_at)
             VALUES (?1,?2,?3,?4,?5,?6,'pending',?7,?7)",
            params![envelope.profile_id, envelope.channel.as_str(), envelope.account_id,
                    direction.as_str(), envelope.event_id, serde_json::to_string(envelope)?, now],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// Atomically lease a bounded batch. An expired lease is eligible again;
    /// a delivery with exhausted attempts is dead-lettered, not retried forever.
    pub fn claim(&self, now: i64, limit: usize, lease_seconds: i64) -> Result<Vec<LeasedMessage>> {
        if limit == 0 || limit > MAX_CLAIM || !(1..=300).contains(&lease_seconds) {
            bail!("Invalid channel lease size or duration.");
        }
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = db.transaction()?;
        tx.execute(
            "UPDATE channel_messages SET status='dead', payload='', lease_until=0
             WHERE status='leased' AND lease_until<=?1 AND attempts>=?2",
            params![now, MAX_ATTEMPTS],
        )?;
        let entries: Vec<(i64, String, String, i64)> = {
            let mut stmt = tx.prepare(
                "SELECT id,direction,payload,attempts FROM channel_messages
                 WHERE (status='pending' AND next_at<=?1)
                    OR (status='leased' AND lease_until<=?1 AND attempts<?2)
                 ORDER BY id LIMIT ?3",
            )?;
            let rows = stmt.query_map(params![now, MAX_ATTEMPTS, limit as i64], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut leased = Vec::with_capacity(entries.len());
        for (id, direction, json, attempts) in entries {
            let envelope: ChannelEnvelope = serde_json::from_str(&json)
                .context("Damaged queue entry; refusing to process")?;
            envelope.validate()?;
            tx.execute(
                "UPDATE channel_messages SET status='leased', attempts=attempts+1,
                 lease_until=?2 WHERE id=?1",
                params![id, now.saturating_add(lease_seconds)],
            )?;
            leased.push(LeasedMessage {
                id,
                direction: if direction == "inbound" { Direction::Inbound } else { Direction::Outbound },
                envelope,
                attempts: attempts + 1,
            });
        }
        tx.commit()?;
        Ok(leased)
    }

    /// Idempotently mark a lease complete and erase its local text while
    /// retaining the event identity as a replay/dedup tombstone.
    pub fn acknowledge(&self, id: i64) -> Result<bool> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.execute(
            "UPDATE channel_messages SET status='done', payload='', lease_until=0
             WHERE id=?1 AND status='leased'",
            params![id],
        )? == 1)
    }

    /// Retry with capped exponential backoff. Never persist raw provider error
    /// responses here: they may contain secrets or untrusted message content.
    pub fn retry(&self, id: i64, now: i64) -> Result<bool> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let attempts: Option<i64> = db
            .query_row(
                "SELECT attempts FROM channel_messages WHERE id=?1 AND status='leased'",
                params![id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(attempts) = attempts else { return Ok(false) };
        let dead = attempts >= MAX_ATTEMPTS;
        let delay = 1_i64 << attempts.min(8);
        Ok(db.execute(
            "UPDATE channel_messages SET status=?2, payload=CASE WHEN ?3 THEN '' ELSE payload END,
             lease_until=0, next_at=?4 WHERE id=?1 AND status='leased'",
            params![id, if dead { "dead" } else { "pending" }, dead, now.saturating_add(delay)],
        )? == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn envelope() -> ChannelEnvelope {
        ChannelEnvelope {
            channel: Channel::Telegram,
            profile_id: "default".into(),
            account_id: "personal-bot".into(),
            chat_id: "1001".into(),
            sender_id: "1001".into(),
            event_id: "update-3".into(),
            thread_id: None,
            text: "hello HERON".into(),
            received_at: 1_800_000_000,
        }
    }
    fn owner() -> VerifiedBinding {
        VerifiedBinding {
            channel: Channel::Telegram,
            profile_id: "default".into(),
            account_id: "personal-bot".into(),
            sender_id: "1001".into(),
        }
    }

    #[test]
    fn cannot_admit_unpaired_or_cross_profile_sender() {
        let tmp = tempdir().unwrap();
        let q = ChannelQueue::open(tmp.path()).unwrap();
        let mut e = envelope();
        e.sender_id = "attacker".into();
        assert!(q.enqueue_inbound(&owner(), &e).is_err());
        e = envelope();
        e.profile_id = "other".into();
        assert!(q.enqueue_inbound(&owner(), &e).is_err());
        e = envelope();
        e.text = "x".repeat(MAX_MESSAGE_BYTES + 1);
        assert!(q.enqueue_inbound(&owner(), &e).is_err());
        e = envelope();
        e.event_id = "abc\nforge".into();
        assert!(q.enqueue_inbound(&owner(), &e).is_err());
        assert!(q.enqueue_inbound(&owner(), &envelope()).unwrap());
    }

    #[test]
    fn replay_lease_recovery_dedup_and_payload_erasure() {
        let tmp = tempdir().unwrap();
        let now = crate::util::now_ts();
        let q = ChannelQueue::open(tmp.path()).unwrap();
        let e = envelope();
        assert!(q.enqueue_inbound(&owner(), &e).unwrap());
        assert!(!q.enqueue_inbound(&owner(), &e).unwrap());
        let first = q.claim(now, 2, 10).unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].attempts, 1);
        assert!(q.claim(now + 5, 2, 10).unwrap().is_empty());
        drop(q);
        let q = ChannelQueue::open(tmp.path()).unwrap();
        let retried = q.claim(now + 11, 1, 10).unwrap();
        assert_eq!(retried.len(), 1);
        assert_eq!(retried[0].id, first[0].id);
        assert_eq!(retried[0].attempts, 2);
        assert!(q.acknowledge(retried[0].id).unwrap());
        assert!(!q.acknowledge(retried[0].id).unwrap());
        assert!(!q.enqueue_inbound(&owner(), &e).unwrap());
        assert!(q.claim(now + 500, 2, 10).unwrap().is_empty());
        let db = q.db.lock().unwrap();
        let payload: String = db.query_row(
            "SELECT payload FROM channel_messages WHERE id=?1",
            params![first[0].id],
            |r| r.get(0),
        ).unwrap();
        assert!(payload.is_empty());
    }

    #[test]
    fn account_scoped_dedup_and_dead_letter_after_five_failures() {
        let tmp = tempdir().unwrap();
        let q = ChannelQueue::open(tmp.path()).unwrap();
        let now = crate::util::now_ts();
        let e = envelope();
        q.enqueue_inbound(&owner(), &e).unwrap();
        let mut other = e.clone();
        other.profile_id = "work".into();
        assert!(q.enqueue_outbound(&other).unwrap());
        assert!(q.enqueue_outbound(&e).unwrap());
        for i in 0..MAX_ATTEMPTS {
            let at = now + 200 * i;
            let a = q.claim(at, 1, 30).unwrap();
            assert_eq!(a.len(), 1);
            assert_eq!(a[0].direction, Direction::Inbound);
            assert!(q.retry(a[0].id, at).unwrap());
        }
        let mut db = q.db.lock().unwrap();
        let status: String = db.query_row(
            "SELECT status FROM channel_messages WHERE direction='inbound'",
            [],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(status, "dead");
        drop(db);
        assert_eq!(q.claim(now + 2000, 5, 30).unwrap().len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn spool_is_private_even_with_permissive_umask() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempdir().unwrap();
        ChannelQueue::open(tmp.path()).unwrap();
        let mode = fs::metadata(tmp.path().join("channel-gateway.sqlite3"))
            .unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
