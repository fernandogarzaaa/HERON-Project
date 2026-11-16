//! Consent-bound, provenance-preserving connector events for Pro Activity.
//!
//! This module does not poll connectors, read their secrets, execute tools,
//! send messages, or automatically accept proposals. It is the durable
//! evidence/control plane that future *qualified* connector adapters may use.
//! Provider content is untrusted; only opaque, hashed references are stored.

use anyhow::{bail, Result};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Mutex;

const DAY: i64 = 86_400;
const MAX_TTL: i64 = 30 * DAY;
const MAX_PENDING_PER_DAY: i64 = 5;
const MAX_EVENT_RECORDS: i64 = 20_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sensitivity {
    Normal,
    Sensitive,
    Restricted,
}

impl Sensitivity {
    fn name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Sensitive => "sensitive",
            Self::Restricted => "restricted",
        }
    }
}

/// Only opaque reference IDs are accepted; never raw event bodies, access
/// tokens, email text or model instructions.
#[derive(Clone, Debug)]
pub struct ConnectorEvent {
    pub connector_id: String,
    pub account_id: String,
    pub provider_event_id: String,
    pub evidence_ref: String,
    pub consent_version: String,
    pub observed_at: i64,
    pub ttl_seconds: i64,
    pub sensitivity: Sensitivity,
}

fn valid_id(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 160
        && !text.chars().any(char::is_control)
}

fn digest(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventAdmission {
    Accepted,
    Duplicate,
    ConsentRequired,
    Expired,
    CapacityReached,
}

/// Independent SQLite journal avoids silently changing HERON's core DB
/// schema. The operating system must protect its local files and backups.
pub struct ProactivityJournal {
    db: Mutex<Connection>,
}

impl ProactivityJournal {
    pub fn open(home: &Path) -> Result<Self> {
        std::fs::create_dir_all(home)?;
        let path = home.join("proactivity-events.db");
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
                bail!("Pro Activity database path must be a regular file")
            }
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        opts.open(&path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        Self::init(Connection::open(path)?)
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
             CREATE TABLE IF NOT EXISTS connector_consents(
               connector TEXT NOT NULL, account TEXT NOT NULL,
               version TEXT NOT NULL, valid_until INTEGER NOT NULL,
               active INTEGER NOT NULL DEFAULT 1,
               PRIMARY KEY(connector, account)
             );
             CREATE TABLE IF NOT EXISTS connector_events(
               id INTEGER PRIMARY KEY,
               connector TEXT NOT NULL, account TEXT NOT NULL,
               provider_event_hash TEXT NOT NULL, evidence_hash TEXT NOT NULL,
               consent_version TEXT NOT NULL, observed_at INTEGER NOT NULL,
               expires_at INTEGER NOT NULL, sensitivity TEXT NOT NULL,
               UNIQUE(connector, account, provider_event_hash)
             );
             CREATE INDEX IF NOT EXISTS idx_connector_events_expiry
               ON connector_events(expires_at);
             CREATE TABLE IF NOT EXISTS connector_suggestions(
               id INTEGER PRIMARY KEY,
               event_id INTEGER NOT NULL REFERENCES connector_events(id) ON DELETE CASCADE,
               connector TEXT NOT NULL, account TEXT NOT NULL,
               queued_at INTEGER NOT NULL,
               status TEXT NOT NULL DEFAULT 'pending',
               UNIQUE(event_id)
             );
             CREATE INDEX IF NOT EXISTS idx_connector_suggestions_budget
               ON connector_suggestions(connector, account, queued_at);
             -- A notification budget is audit/accounting state, independent of
             -- evidence rows that expire or are purged at consent rotation.
             CREATE TABLE IF NOT EXISTS connector_budget_ledger(
               connector TEXT NOT NULL,
               account TEXT NOT NULL,
               event_hash TEXT NOT NULL,
               queued_at INTEGER NOT NULL,
               PRIMARY KEY(connector, account, event_hash)
             );
             CREATE INDEX IF NOT EXISTS idx_connector_budget_window
               ON connector_budget_ledger(connector, account, queued_at);
             INSERT OR IGNORE INTO connector_budget_ledger(connector,account,event_hash,queued_at)
             SELECT s.connector,s.account,e.provider_event_hash,s.queued_at
             FROM connector_suggestions s JOIN connector_events e ON s.event_id=e.id;",
        )?;
        db.execute_batch("PRAGMA foreign_keys=ON;")?;
        Ok(Self { db: Mutex::new(db) })
    }

    /// Consent must come from the application's trusted local approval
    /// interface, never from remote event text.
    pub fn register_consent(
        &self,
        connector: &str,
        account: &str,
        version: &str,
        valid_until: i64,
        now: i64,
    ) -> Result<()> {
        if !valid_id(connector) || !valid_id(account) || !valid_id(version)
            || valid_until <= now
        {
            bail!("Invalid or expired connector consent");
        }
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.execute(
            "INSERT INTO connector_consents(connector, account, version, valid_until, active)
             VALUES (?1,?2,?3,?4,1)
             ON CONFLICT(connector,account) DO UPDATE SET
                version=excluded.version, valid_until=excluded.valid_until, active=1",
            params![connector, account, version, valid_until],
        )?;
        // A new consent version is not allowed to inherit old observations.
        db.execute(
            "DELETE FROM connector_events WHERE connector=?1 AND account=?2 AND consent_version!=?3",
            params![connector, account, version],
        )?;
        Ok(())
    }

    pub fn revoke(&self, connector: &str, account: &str) -> Result<()> {
        if !valid_id(connector) || !valid_id(account) {
            bail!("Invalid connector identity");
        }
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = db.transaction()?;
        tx.execute(
            "UPDATE connector_consents SET active=0 WHERE connector=?1 AND account=?2",
            params![connector, account],
        )?;
        tx.execute(
            "DELETE FROM connector_events WHERE connector=?1 AND account=?2",
            params![connector, account],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn admit(&self, event: &ConnectorEvent, now: i64) -> Result<EventAdmission> {
        for part in [
            &event.connector_id,
            &event.account_id,
            &event.provider_event_id,
            &event.evidence_ref,
            &event.consent_version,
        ] {
            if !valid_id(part) {
                bail!("Invalid connector event metadata");
            }
        }
        if event.ttl_seconds <= 0 || event.ttl_seconds > MAX_TTL
            || event.observed_at > now.saturating_add(300)
        {
            bail!("Invalid connector event lifetime or timestamp");
        }
        if event.observed_at.saturating_add(event.ttl_seconds) <= now {
            return Ok(EventAdmission::Expired);
        }
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = db.transaction()?;
        let consent: Option<(String, i64, i64)> = tx.query_row(
            "SELECT version, valid_until, active FROM connector_consents
             WHERE connector=?1 AND account=?2",
            params![event.connector_id, event.account_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional()?;
        let allowed = consent.is_some_and(|(version, until, active)|
            active == 1 && version == event.consent_version && until > now);
        if !allowed {
            return Ok(EventAdmission::ConsentRequired);
        }
        let event_hash = digest(&event.provider_event_id);
        let known: i64 = tx.query_row(
            "SELECT COUNT(*) FROM connector_events WHERE connector=?1 AND account=?2
             AND provider_event_hash=?3",
            params![event.connector_id, event.account_id, event_hash],
            |row| row.get(0),
        )?;
        if known > 0 {
            return Ok(EventAdmission::Duplicate);
        }
        let count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM connector_events", [], |row| row.get(0)
        )?;
        if count >= MAX_EVENT_RECORDS {
            return Ok(EventAdmission::CapacityReached);
        }
        tx.execute(
            "INSERT INTO connector_events
             (connector,account,provider_event_hash,evidence_hash,consent_version,
              observed_at,expires_at,sensitivity)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![event.connector_id, event.account_id, event_hash,
                    digest(&event.evidence_ref), event.consent_version,
                    event.observed_at, event.observed_at.saturating_add(event.ttl_seconds),
                    event.sensitivity.name()],
        )?;
        tx.commit()?;
        Ok(EventAdmission::Accepted)
    }

    /// Queue only a human-reviewable suggestion marker. No agent execution or
    /// external write is possible from this interface.
    pub fn propose(
        &self,
        connector: &str,
        account: &str,
        provider_event_id: &str,
        now: i64,
    ) -> Result<bool> {
        if !valid_id(connector) || !valid_id(account) || !valid_id(provider_event_id) {
            bail!("Invalid connector evidence identifier");
        }
        let mut db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        let tx = db.transaction()?;
        let eligible: Option<i64> = tx.query_row(
            "SELECT e.id FROM connector_events e
             JOIN connector_consents c ON e.connector=c.connector AND e.account=c.account
             WHERE e.connector=?1 AND e.account=?2 AND e.provider_event_hash=?3
               AND e.expires_at>?4 AND e.sensitivity='normal'
               AND c.active=1 AND c.valid_until>?4 AND c.version=e.consent_version",
            params![connector, account, digest(provider_event_id), now],
            |row| row.get(0),
        ).optional()?;
        let Some(event_id) = eligible else {
            return Ok(false);
        };
        let existing: i64 = tx.query_row(
            "SELECT COUNT(*) FROM connector_suggestions WHERE event_id=?1",
            [event_id],
            |row| row.get(0),
        )?;
        if existing > 0 {
            return Ok(false);
        }
        // Consent rotation and evidence retention must never reset this
        // rolling window. The independent ledger is pruned after the window.
        tx.execute(
            "DELETE FROM connector_budget_ledger WHERE queued_at<=?1",
            [now.saturating_sub(DAY)],
        )?;
        let count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM connector_budget_ledger
             WHERE connector=?1 AND account=?2 AND queued_at>?3",
            params![connector, account, now.saturating_sub(DAY)],
            |row| row.get(0),
        )?;
        if count >= MAX_PENDING_PER_DAY {
            return Ok(false);
        }
        // Ensure this provider event has not been counted already even after
        // its evidence was purged, then charge the budget atomically.
        let ledger_added = tx.execute(
            "INSERT OR IGNORE INTO connector_budget_ledger
             (connector,account,event_hash,queued_at) VALUES (?1,?2,?3,?4)",
            params![connector, account, digest(provider_event_id), now],
        )?;
        if ledger_added == 0 {
            return Ok(false);
        }
        tx.execute(
            "INSERT INTO connector_suggestions(event_id,connector,account,queued_at)
             VALUES (?1,?2,?3,?4)",
            params![event_id, connector, account, now],
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub fn prune_expired(&self, now: i64) -> Result<usize> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.execute("DELETE FROM connector_events WHERE expires_at<=?1", [now])?)
    }

    pub fn pending_count(&self) -> Result<i64> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        Ok(db.query_row(
            "SELECT COUNT(*) FROM connector_suggestions WHERE status='pending'",
            [],
            |row| row.get(0),
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(name: &str) -> ConnectorEvent {
        ConnectorEvent {
            connector_id: "gmail".into(),
            account_id: "personal".into(),
            provider_event_id: name.into(),
            evidence_ref: "opaque:message-id-123".into(),
            consent_version: "v1".into(),
            observed_at: 100,
            ttl_seconds: 1000,
            sensitivity: Sensitivity::Normal,
        }
    }

    #[test]
    fn consent_dedup_expiry_and_revocation_are_enforced() {
        let journal = ProactivityJournal::in_memory().unwrap();
        assert_eq!(journal.admit(&event("m1"), 100).unwrap(), EventAdmission::ConsentRequired);
        journal.register_consent("gmail", "personal", "v1", 500, 100).unwrap();
        assert_eq!(journal.admit(&event("m1"), 100).unwrap(), EventAdmission::Accepted);
        assert_eq!(journal.admit(&event("m1"), 100).unwrap(), EventAdmission::Duplicate);
        assert!(journal.propose("gmail", "personal", "m1", 101).unwrap());
        assert_eq!(journal.pending_count().unwrap(), 1);
        assert!(!journal.propose("gmail", "personal", "m1", 101).unwrap());
        journal.revoke("gmail", "personal").unwrap();
        assert_eq!(journal.pending_count().unwrap(), 0);
        assert_eq!(journal.admit(&event("m1"), 102).unwrap(), EventAdmission::ConsentRequired);
    }

    #[test]
    fn per_account_budgets_and_sensitive_sources_are_restricted() {
        let journal = ProactivityJournal::in_memory().unwrap();
        journal.register_consent("gmail", "personal", "v1", 10_000, 100).unwrap();
        journal.register_consent("gmail", "work", "v1", 10_000, 100).unwrap();
        for i in 0..6 {
            let e = event(&format!("m{i}"));
            assert_eq!(journal.admit(&e, 100).unwrap(), EventAdmission::Accepted);
            assert_eq!(journal.propose("gmail", "personal", &e.provider_event_id, 100).unwrap(), i < 5);
        }
        let mut work = event("work");
        work.account_id = "work".into();
        assert_eq!(journal.admit(&work, 100).unwrap(), EventAdmission::Accepted);
        assert!(journal.propose("gmail", "work", "work", 100).unwrap());
        let mut sensitive = event("private");
        sensitive.sensitivity = Sensitivity::Sensitive;
        assert_eq!(journal.admit(&sensitive, 100).unwrap(), EventAdmission::Accepted);
        assert!(!journal.propose("gmail", "personal", "private", 100).unwrap());
    }

    #[test]
    fn pruning_or_rotating_consent_does_not_reset_daily_budget() {
        let journal = ProactivityJournal::in_memory().unwrap();
        journal.register_consent("gmail", "personal", "v1", 10_000, 100).unwrap();
        for i in 0..5 {
            let mut e = event(&format!("first-{i}"));
            e.ttl_seconds = 1;
            assert_eq!(journal.admit(&e, 100).unwrap(), EventAdmission::Accepted);
            assert!(journal.propose("gmail", "personal", &e.provider_event_id, 100).unwrap());
        }
        assert_eq!(journal.prune_expired(102).unwrap(), 5);
        journal.register_consent("gmail", "personal", "v2", 10_000, 102).unwrap();
        let mut fresh = event("sixth");
        fresh.consent_version = "v2".into();
        fresh.observed_at = 102;
        assert_eq!(journal.admit(&fresh, 102).unwrap(), EventAdmission::Accepted);
        assert!(!journal.propose("gmail", "personal", "sixth", 102).unwrap(),
            "evidence deletion and new consent cannot replenish the budget");
        assert!(journal.propose("gmail", "personal", "sixth", 100 + DAY + 1).unwrap() == false,
            "expired evidence cannot be proposed after the budget period");
    }

    #[test]
    fn stale_consent_versions_and_expired_events_are_rejected() {
        let journal = ProactivityJournal::in_memory().unwrap();
        journal.register_consent("gmail", "personal", "v2", 400, 100).unwrap();
        assert_eq!(journal.admit(&event("old"), 100).unwrap(), EventAdmission::ConsentRequired);
        let mut fresh = event("m1");
        fresh.consent_version = "v2".into();
        assert_eq!(journal.admit(&fresh, 1101).unwrap(), EventAdmission::Expired);
        assert_eq!(journal.admit(&fresh, 100).unwrap(), EventAdmission::Accepted);
        assert!(!journal.propose("gmail", "personal", "m1", 401).unwrap());
        assert_eq!(journal.prune_expired(1100).unwrap(), 1);
    }
}
