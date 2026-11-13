//! SQLite persistence: sessions, messages, memories (FTS5 + vectors), tasks,
//! schedules, inbox, audit log and file checkpoints. One file, fully local.

use crate::llm::Message;
use crate::util::now_ts;
use anyhow::{anyhow, bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

fn audit_hash(prev: &str, ts: i64, session: &str, tool: &str, args: &str, decision: &str, result: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    // Length-prefix each field so that no two different entries can produce the same input.
    for field in [prev, &ts.to_string(), session, tool, args, decision, result] {
        h.update((field.len() as u64).to_le_bytes());
        h.update(field.as_bytes());
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
    /// Where the database file is; `None` for one that lives only in memory.
    path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub messages: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryItem {
    pub id: i64,
    pub kind: String,
    pub content: String,
    pub tags: String,
    pub importance: f64,
    pub created_at: i64,
    pub updated_at: i64,
    pub uses: i64,
    /// Where it came from: user, agent, auto, onboarding, email, calendar, documents, observed, computer.
    pub source: String,
    /// False for facts taken from outside material that the user has not vouched for yet.
    pub confirmed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub id: i64,
    pub title: String,
    pub notes: String,
    pub status: String,
    pub priority: i64,
    pub due: Option<String>,
    pub created_at: i64,
    /// Who is doing it: "user" or "heron".
    pub owner: String,
}

/// Something Heron wanted to do while nobody was there to approve it, kept for the user to decide.
#[derive(Debug, Clone, Serialize)]
pub struct ParkedApproval {
    pub id: i64,
    pub ts: i64,
    pub session_id: String,
    pub tool: String,
    pub args: String,
    pub summary: String,
    pub detail: String,
    pub risk: String,
    pub tainted: bool,
    /// What Heron was doing at the time (the job or suggestion title).
    pub origin: String,
    pub status: String,
    pub result: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Schedule {
    pub id: i64,
    pub name: String,
    /// Cron expression for recurring jobs; empty for one-shot jobs.
    pub cron: String,
    pub prompt: String,
    pub enabled: bool,
    pub next_run: Option<i64>,
    pub last_run: Option<i64>,
    pub last_result: Option<String>,
    /// Permission rules granted to this job by the user (one per line). Empty = default unattended mode.
    pub allow: String,
    /// Only tell the user when something needs their attention.
    pub quiet: bool,
    /// Tokens the most recent run used.
    pub last_tokens: i64,
    /// The steps of an automation (JSON), or empty when the job is a plain request to the agent.
    pub workflow: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InboxItem {
    pub id: i64,
    pub ts: i64,
    pub title: String,
    pub body: String,
    pub read: bool,
}

/// An item from a connected source (an email, a calendar event, a document), indexed locally for lookup.
#[derive(Debug, Clone, Serialize)]
pub struct KnowledgeItem {
    pub id: i64,
    pub source: String,
    pub reference: String,
    pub title: String,
    pub body: String,
    pub ts: i64,
}

/// Work Heron proposed on its own initiative.
#[derive(Debug, Clone, Serialize)]
pub struct Suggestion {
    pub id: i64,
    pub ts: i64,
    pub title: String,
    pub why: String,
    pub prompt: String,
    /// new | prepared | done | failed | accepted | dismissed
    pub status: String,
    pub result: Option<String>,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditEntry {
    pub ts: i64,
    pub session_id: String,
    pub tool: String,
    pub args: String,
    pub decision: String,
    pub result: String,
}

/// The data format this build reads and writes, kept in the database file as `PRAGMA user_version`.
/// It is the number of upgrade steps in [`STEPS`]. To change the schema: add a step, never edit an
/// old one. A test (`the_schema_cannot_change_without_a_new_version`) fails until both are done.
pub const SCHEMA_VERSION: i64 = STEPS.len() as i64;

type Step = fn(&Connection) -> Result<()>;

/// One entry per data format, in order. Step N takes a database from format N-1 to format N.
const STEPS: &[Step] = &[to_v1, to_v2];

/// Format 1: what Heron 0.1.0 shipped.
const SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS sessions(
  id TEXT PRIMARY KEY, title TEXT NOT NULL DEFAULT '', summary TEXT,
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS messages(
  id INTEGER PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  json TEXT NOT NULL, created_at INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, id);
CREATE TABLE IF NOT EXISTS memories(
  id INTEGER PRIMARY KEY, kind TEXT NOT NULL DEFAULT 'fact', content TEXT NOT NULL,
  tags TEXT NOT NULL DEFAULT '', importance REAL NOT NULL DEFAULT 0.5, source TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, last_used INTEGER, uses INTEGER NOT NULL DEFAULT 0,
  embedding BLOB, confirmed INTEGER NOT NULL DEFAULT 1, invalid_at INTEGER, superseded_by INTEGER,
  embedder TEXT NOT NULL DEFAULT 'hash-v1');
CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(content, tags, content='memories', content_rowid='id', tokenize='porter unicode61');
CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
  INSERT INTO memories_fts(rowid, content, tags) VALUES (new.id, new.content, new.tags); END;
CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
  INSERT INTO memories_fts(memories_fts, rowid, content, tags) VALUES('delete', old.id, old.content, old.tags); END;
CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE OF content, tags ON memories BEGIN
  INSERT INTO memories_fts(memories_fts, rowid, content, tags) VALUES('delete', old.id, old.content, old.tags);
  INSERT INTO memories_fts(rowid, content, tags) VALUES (new.id, new.content, new.tags); END;
CREATE TABLE IF NOT EXISTS tasks(
  id INTEGER PRIMARY KEY, title TEXT NOT NULL, notes TEXT NOT NULL DEFAULT '', status TEXT NOT NULL DEFAULT 'open',
  priority INTEGER NOT NULL DEFAULT 2, due TEXT, created_at INTEGER NOT NULL, done_at INTEGER, owner TEXT NOT NULL DEFAULT 'user');
CREATE TABLE IF NOT EXISTS approvals(
  id INTEGER PRIMARY KEY, ts INTEGER NOT NULL, session_id TEXT NOT NULL, tool TEXT NOT NULL, args TEXT NOT NULL,
  summary TEXT NOT NULL, detail TEXT NOT NULL, risk TEXT NOT NULL, tainted INTEGER NOT NULL DEFAULT 0,
  origin TEXT NOT NULL DEFAULT '', status TEXT NOT NULL DEFAULT 'new', result TEXT);
CREATE TABLE IF NOT EXISTS schedules(
  id INTEGER PRIMARY KEY, name TEXT NOT NULL, cron TEXT NOT NULL DEFAULT '', prompt TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1, next_run INTEGER, last_run INTEGER, last_result TEXT, created_at INTEGER NOT NULL,
  allow TEXT NOT NULL DEFAULT '', quiet INTEGER NOT NULL DEFAULT 0, last_tokens INTEGER NOT NULL DEFAULT 0,
  workflow TEXT NOT NULL DEFAULT '');
CREATE TABLE IF NOT EXISTS runs(
  id INTEGER PRIMARY KEY, schedule_id INTEGER NOT NULL, ts INTEGER NOT NULL, status TEXT NOT NULL, tokens INTEGER NOT NULL DEFAULT 0, record TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS idx_runs_schedule ON runs(schedule_id, id);
CREATE TABLE IF NOT EXISTS inbox(
  id INTEGER PRIMARY KEY, ts INTEGER NOT NULL, title TEXT NOT NULL, body TEXT NOT NULL, read INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS audit(
  id INTEGER PRIMARY KEY, ts INTEGER NOT NULL, session_id TEXT NOT NULL, tool TEXT NOT NULL,
  args TEXT NOT NULL, decision TEXT NOT NULL, result TEXT NOT NULL DEFAULT '', hash TEXT);
CREATE TABLE IF NOT EXISTS checkpoints(
  id INTEGER PRIMARY KEY, ts INTEGER NOT NULL, session_id TEXT NOT NULL, path TEXT NOT NULL,
  existed INTEGER NOT NULL, prev BLOB);
CREATE TABLE IF NOT EXISTS kv(k TEXT PRIMARY KEY, v TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS observations(
  id INTEGER PRIMARY KEY, ts INTEGER NOT NULL, kind TEXT NOT NULL, subject TEXT NOT NULL, bucket INTEGER NOT NULL,
  UNIQUE(kind, subject, bucket));
CREATE INDEX IF NOT EXISTS idx_observations_ts ON observations(ts);
CREATE TABLE IF NOT EXISTS knowledge(
  id INTEGER PRIMARY KEY, source TEXT NOT NULL, ref TEXT NOT NULL, title TEXT NOT NULL, body TEXT NOT NULL, ts INTEGER NOT NULL,
  digested INTEGER NOT NULL DEFAULT 0, UNIQUE(source, ref));
CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_fts USING fts5(title, body, content='knowledge', content_rowid='id', tokenize='porter unicode61');
CREATE TRIGGER IF NOT EXISTS knowledge_ai AFTER INSERT ON knowledge BEGIN
  INSERT INTO knowledge_fts(rowid, title, body) VALUES (new.id, new.title, new.body); END;
CREATE TRIGGER IF NOT EXISTS knowledge_ad AFTER DELETE ON knowledge BEGIN
  INSERT INTO knowledge_fts(knowledge_fts, rowid, title, body) VALUES('delete', old.id, old.title, old.body); END;
CREATE TRIGGER IF NOT EXISTS knowledge_au AFTER UPDATE OF title, body ON knowledge BEGIN
  INSERT INTO knowledge_fts(knowledge_fts, rowid, title, body) VALUES('delete', old.id, old.title, old.body);
  INSERT INTO knowledge_fts(rowid, title, body) VALUES (new.id, new.title, new.body); END;
CREATE TABLE IF NOT EXISTS suggestions(
  id INTEGER PRIMARY KEY, ts INTEGER NOT NULL, title TEXT NOT NULL, why TEXT NOT NULL DEFAULT '', prompt TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'new', result TEXT, session_id TEXT);
"#;

/// Settings applied to every connection. (The journal mode is stored in the file; the rest is per connection.)
const PRAGMAS: &str = "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;";

/// How many copies made before upgrades are kept. Copies made by hand or before a restore are never removed.
const KEEP_UPGRADE_SNAPSHOTS: usize = 3;

fn user_version(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
}

/// Is this a database Heron has used before (as opposed to a new, empty file)?
fn has_data(conn: &Connection) -> Result<bool> {
    Ok(conn.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name='sessions'", [], |r| r.get::<_, i64>(0))? > 0)
}

fn has_column(conn: &Connection, table: &str, column: &str) -> bool {
    conn.prepare(&format!("SELECT {column} FROM {table} LIMIT 0")).is_ok()
}

/// Physical damage: broken pages or indexes. A database with any is not upgraded, copied or restored from.
fn damage(conn: &Connection) -> Result<Vec<String>> {
    // A database can be too damaged for the check itself to run. That is a finding, not a failure to report.
    let run = || -> rusqlite::Result<Vec<String>> {
        let mut st = conn.prepare("PRAGMA quick_check")?;
        let lines = st.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(lines.into_iter().filter(|line| line != "ok").collect())
    };
    Ok(run().unwrap_or_else(|e| vec![e.to_string()]))
}

/// Damage, plus rows that point at rows which no longer exist. The second kind does not stop an
/// upgrade (the data is all there and the steps do not depend on it) but is worth reporting.
fn integrity_problems(conn: &Connection) -> Result<Vec<String>> {
    let mut problems = damage(conn)?;
    if !problems.is_empty() {
        return Ok(problems); // the second check cannot be trusted on a damaged file
    }
    let orphans = || -> rusqlite::Result<Vec<String>> {
        let mut st = conn.prepare("PRAGMA foreign_key_check")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?, r.get::<_, String>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows.into_iter().map(|(table, rowid, parent)| format!("{table} row {} refers to a missing row in {parent}", rowid.map(|r| r.to_string()).unwrap_or_else(|| "?".into()))).collect())
    };
    problems.extend(orphans().unwrap_or_else(|e| vec![e.to_string()]));
    Ok(problems)
}

/// The folder beside the database where copies of it are kept.
pub fn backup_dir(db: &Path) -> PathBuf {
    db.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new(".")).join("backups")
}

/// Save a complete copy of the database in its `backups` folder under a name nothing else has.
fn snapshot(conn: &Connection, db: &Path, label: &str) -> Result<PathBuf> {
    let dir = backup_dir(db);
    std::fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    for n in 1..=1000 {
        let to = dir.join(if n == 1 { format!("heron-{label}-{stamp}.db") } else { format!("heron-{label}-{stamp}-{n}.db") });
        match snapshot_to(conn, &to) {
            Ok(()) => return Ok(to),
            // The name was taken (perhaps this very moment, by another program): try the next.
            Err(e) if e.root_cause().downcast_ref::<std::io::Error>().is_some_and(|io| io.kind() == std::io::ErrorKind::AlreadyExists) => continue,
            Err(e) => return Err(e),
        }
    }
    bail!("cannot name a new copy in {}", dir.display())
}

/// Write a complete, consistent copy of the database to `to`, which must not exist yet. The copy is
/// made by SQLite itself, so it is correct even while the database is in use, and it is private to
/// the user from the moment it is created. It is checked before this returns.
fn snapshot_to(conn: &Connection, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let mut open = std::fs::OpenOptions::new();
    open.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        open.mode(0o600);
    }
    // Claim the name (and set who may read it) first; SQLite then fills the empty file.
    open.open(to).with_context(|| format!("cannot create {}", to.display()))?;
    let made = (|| -> Result<()> {
        conn.execute("VACUUM INTO ?1", [to.to_string_lossy().as_ref()])?;
        let copy = Connection::open_with_flags(to, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let problems = damage(&copy)?;
        if !problems.is_empty() {
            bail!("the copy did not pass its check ({})", problems.join("; "));
        }
        Ok(())
    })();
    if made.is_err() {
        let _ = std::fs::remove_file(to);
    }
    made.with_context(|| format!("cannot save a copy of the database to {}", to.display()))
}

/// Move a database that SQLite cannot read out of the way, exactly as it is, together with its log
/// files, into the `backups` folder. Nothing in it is changed, so whatever a repair tool could still
/// recover remains recoverable. Returns where the main file went.
fn set_aside_damaged(db: &Path) -> Result<PathBuf> {
    let dir = backup_dir(db);
    std::fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let with = |base: &Path, suffix: &str| PathBuf::from(format!("{}{suffix}", base.display()));
    for n in 1..=1000 {
        let to = dir.join(if n == 1 { format!("heron-damaged-{stamp}.db") } else { format!("heron-damaged-{stamp}-{n}.db") });
        // Claim the name first, so two restores cannot pick the same one and nothing is ever replaced.
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&to) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e).with_context(|| format!("cannot create {}", to.display())),
        }
        std::fs::rename(db, &to).with_context(|| format!("cannot move the damaged database to {}", to.display()))?;
        // The log files belong to it; keep them beside it under matching names.
        for suffix in ["-wal", "-shm"] {
            let part = with(db, suffix);
            if part.exists() {
                let _ = std::fs::rename(&part, with(&to, suffix));
            }
        }
        return Ok(to);
    }
    bail!("cannot name the damaged database's copy in {}", dir.display())
}

/// Keep the newest `keep` copies whose name starts with `heron-{prefix}` and remove the older ones
/// (but only ones that have been there a while).
fn prune_snapshots(db: &Path, prefix: &str, keep: usize) {
    let Ok(entries) = std::fs::read_dir(backup_dir(db)) else { return };
    let wanted = format!("heron-{prefix}");
    let mut copies: Vec<(std::time::SystemTime, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.starts_with(&wanted) && name.ends_with(".db")
        })
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    copies.sort();
    let extra = copies.len().saturating_sub(keep);
    // Never one made in the last few minutes: another program starting at the same moment may still
    // be writing or checking it.
    let settled = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
    for (made, path) in copies.into_iter().take(extra) {
        if made < settled {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Bring a database up to the latest format, as one transaction: every step that is still owed runs,
/// the version is recorded, and only then is anything kept. If a step fails, nothing is.
/// The version is read again inside the transaction, so two programs starting at once cannot both upgrade.
fn migrate(conn: &mut Connection, steps: &[Step]) -> Result<()> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let found = user_version(&tx)?;
    for (done, step) in steps.iter().enumerate().skip(found.max(0) as usize) {
        step(&tx).with_context(|| format!("step to data format {}", done + 1))?;
    }
    if found < steps.len() as i64 {
        tx.pragma_update(None, "user_version", steps.len() as i64)?;
    }
    tx.commit()?;
    Ok(())
}

/// To format 1. Databases from before formats were numbered are also "format 0", and some of those
/// were made by builds older than 0.1.0 that lacked a few columns, so this step adds whatever is missing.
fn to_v1(conn: &Connection) -> Result<()> {
    conn.execute_batch(SCHEMA_V1)?;
    if !has_column(conn, "audit", "hash") {
        conn.execute_batch("ALTER TABLE audit ADD COLUMN hash TEXT;")?;
    }
    if !has_column(conn, "memories", "embedder") {
        conn.execute_batch("ALTER TABLE memories ADD COLUMN embedder TEXT NOT NULL DEFAULT 'hash-v1';")?;
    }
    // The activity log can only grow.
    conn.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS audit_no_update BEFORE UPDATE ON audit BEGIN SELECT RAISE(ABORT, 'the activity log is append-only'); END;
         CREATE TRIGGER IF NOT EXISTS audit_no_delete BEFORE DELETE ON audit BEGIN SELECT RAISE(ABORT, 'the activity log is append-only'); END;",
    )?;
    if !has_column(conn, "schedules", "quiet") {
        conn.execute_batch("ALTER TABLE schedules ADD COLUMN quiet INTEGER NOT NULL DEFAULT 0; ALTER TABLE schedules ADD COLUMN last_tokens INTEGER NOT NULL DEFAULT 0;")?;
    }
    if !has_column(conn, "schedules", "workflow") {
        conn.execute_batch("ALTER TABLE schedules ADD COLUMN workflow TEXT NOT NULL DEFAULT '';")?;
    }
    if !has_column(conn, "tasks", "owner") {
        conn.execute_batch("ALTER TABLE tasks ADD COLUMN owner TEXT NOT NULL DEFAULT 'user';")?;
    }
    if !has_column(conn, "memories", "confirmed") {
        conn.execute_batch("ALTER TABLE memories ADD COLUMN confirmed INTEGER NOT NULL DEFAULT 1; ALTER TABLE memories ADD COLUMN invalid_at INTEGER; ALTER TABLE memories ADD COLUMN superseded_by INTEGER;")?;
    }
    if !has_column(conn, "schedules", "allow") {
        conn.execute_batch("ALTER TABLE schedules ADD COLUMN allow TEXT NOT NULL DEFAULT ''")?;
    }
    Ok(())
}

/// To format 2: token usage, the record of scheduled runs, and what a file looked like after Heron
/// changed it. Builds made between 0.1.0 and the first numbered format already have some of these
/// (still as "format 0"), so each part is added only if it is not there.
fn to_v2(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS job_runs(
           id INTEGER PRIMARY KEY, schedule_id INTEGER NOT NULL, name TEXT NOT NULL, due_at INTEGER NOT NULL,
           started_at INTEGER NOT NULL, finished_at INTEGER, status TEXT NOT NULL, note TEXT NOT NULL DEFAULT '');
         CREATE INDEX IF NOT EXISTS idx_job_runs_status ON job_runs(status);
         CREATE TABLE IF NOT EXISTS usage(day TEXT NOT NULL, model TEXT NOT NULL, prompt INTEGER NOT NULL DEFAULT 0, completion INTEGER NOT NULL DEFAULT 0, calls INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(day, model));",
    )?;
    if !has_column(conn, "checkpoints", "after_hash") {
        conn.execute_batch("ALTER TABLE checkpoints ADD COLUMN after_hash TEXT;")?;
    }
    Ok(())
}

fn row_to_memory(r: &rusqlite::Row) -> rusqlite::Result<MemoryItem> {
    Ok(MemoryItem {
        id: r.get(0)?,
        kind: r.get(1)?,
        content: r.get(2)?,
        tags: r.get(3)?,
        importance: r.get(4)?,
        created_at: r.get(5)?,
        updated_at: r.get(6)?,
        uses: r.get(7)?,
        source: r.get(8)?,
        confirmed: r.get::<_, i64>(9)? != 0,
        score: None,
    })
}
const MEM_COLS: &str = "id, kind, content, tags, importance, created_at, updated_at, uses, source, confirmed";

fn row_to_schedule(r: &rusqlite::Row) -> rusqlite::Result<Schedule> {
    Ok(Schedule {
        id: r.get(0)?,
        name: r.get(1)?,
        cron: r.get(2)?,
        prompt: r.get(3)?,
        enabled: r.get::<_, i64>(4)? != 0,
        next_run: r.get(5)?,
        last_run: r.get(6)?,
        last_result: r.get(7)?,
        allow: r.get(8)?,
        quiet: r.get::<_, i64>(9)? != 0,
        last_tokens: r.get(10)?,
        workflow: r.get(11)?,
    })
}
const SCHED_COLS: &str = "id, name, cron, prompt, enabled, next_run, last_run, last_result, allow, quiet, last_tokens, workflow";

impl Store {
    /// Open the database, bringing it up to this build's data format first if it is older.
    ///
    /// Before an existing database is upgraded it is checked for damage and a complete copy is saved in
    /// the `backups` folder beside it. The upgrade runs as one transaction: it either finishes or leaves
    /// the file exactly as it was. A database written by a newer Heron is refused and not touched.
    pub fn open(path: &Path) -> Result<Store> {
        Self::open_checked(path).map_err(|e| {
            // SQLite saying the file itself is broken: say what can be done about it.
            let broken = e.chain().filter_map(|c| c.downcast_ref::<rusqlite::Error>()).any(|c| matches!(c.sqlite_error_code(), Some(rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase)));
            if broken {
                e.context(format!("{} is damaged and cannot be read. Restore a copy from {} with `heron restore <file>`; the damaged file is kept when you do.", path.display(), backup_dir(path).display()))
            } else {
                e
            }
        })
    }

    fn open_checked(path: &Path) -> Result<Store> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let found = user_version(&conn)?;
        if found > SCHEMA_VERSION {
            bail!(
                "{} was last used by a newer version of Heron (data format {found}; this version understands up to {SCHEMA_VERSION}). Nothing was changed. Install the newer version, or restore an older copy from {} with `heron restore`.",
                path.display(),
                backup_dir(path).display()
            );
        }
        conn.execute_batch(PRAGMAS)?;
        if found < SCHEMA_VERSION {
            if has_data(&conn)? {
                let problems = damage(&conn).unwrap_or_else(|e| vec![e.to_string()]);
                if !problems.is_empty() {
                    bail!("{} is damaged and was not upgraded ({}). Restore a copy from {} with `heron restore`.", path.display(), problems.join("; "), backup_dir(path).display());
                }
                let copy = snapshot(&conn, path, &format!("before-upgrade-from-v{found}"))?;
                // Another program may have finished the upgrade while the copy was being made. Then the
                // copy is not "from before" anything and is not kept; the upgrade below finds nothing to do.
                if Connection::open_with_flags(&copy, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).and_then(|c| c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))).map(|v| v >= SCHEMA_VERSION).unwrap_or(false) {
                    let _ = std::fs::remove_file(&copy);
                }
                migrate(&mut conn, STEPS).with_context(|| format!("upgrading {} failed and it was left as it was; a copy from before the attempt is at {}", path.display(), copy.display()))?;
                prune_snapshots(path, "before-upgrade-", KEEP_UPGRADE_SNAPSHOTS);
            } else {
                migrate(&mut conn, STEPS)?;
            }
        }
        Ok(Store { conn: Arc::new(Mutex::new(conn)), path: Some(path.to_path_buf()) })
    }

    pub fn in_memory() -> Result<Store> {
        let mut conn = Connection::open_in_memory()?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(PRAGMAS)?;
        migrate(&mut conn, STEPS)?;
        Ok(Store { conn: Arc::new(Mutex::new(conn)), path: None })
    }

    /// The data format of the open database.
    pub fn schema_version(&self) -> Result<i64> {
        user_version(&self.db())
    }

    /// Look for damage: broken pages or indexes, and rows that point at rows which no longer exist.
    /// Returns what was found, in SQLite's words; an empty list means the database is sound.
    pub fn integrity(&self) -> Result<Vec<String>> {
        integrity_problems(&self.db())
    }

    /// Save a complete, consistent copy of the database in the `backups` folder beside it (or at `to`),
    /// while Heron keeps running. Returns where the copy is.
    pub fn backup(&self, to: Option<&Path>) -> Result<PathBuf> {
        let Some(path) = &self.path else { bail!("this database lives only in memory and cannot be backed up") };
        let db = self.db();
        match to {
            Some(to) => {
                snapshot_to(&db, to)?;
                Ok(to.to_path_buf())
            }
            None => snapshot(&db, path, "backup"),
        }
    }

    /// Replace the database at `path` with the copy in `from`.
    ///
    /// The copy is checked first (it must be an undamaged Heron database, no newer than this build
    /// understands), then the current database is itself saved to the `backups` folder, so a restore
    /// can be undone by restoring that. If the current database is damaged it cannot be copied through
    /// SQLite, so its files are moved to the `backups` folder exactly as they are instead. The content
    /// is replaced through SQLite, in one transaction, and then upgraded if it is from an older version. Returns where the previous database was saved
    /// (`None` if there was nothing there to save).
    pub fn restore(path: &Path, from: &Path) -> Result<Option<PathBuf>> {
        let source = Connection::open_with_flags(from, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).with_context(|| format!("cannot open {}", from.display()))?;
        let Ok(version) = user_version(&source) else { bail!("{} is not a database", from.display()) };
        if !has_data(&source).unwrap_or(false) {
            bail!("{} is not a Heron database", from.display());
        }
        if version > SCHEMA_VERSION {
            bail!("{} was made by a newer version of Heron (data format {version}; this version understands up to {SCHEMA_VERSION})", from.display());
        }
        let problems = damage(&source).unwrap_or_else(|e| vec![e.to_string()]);
        if !problems.is_empty() {
            bail!("{} is damaged and was not restored ({})", from.display(), problems.join("; "));
        }
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        // Save what is there now. A sound database is copied through SQLite. A damaged one cannot be
        // read that way (that is what damaged means), and it is exactly the case a restore is for: its
        // files are moved aside as they are, untouched, and the restore goes into a new file.
        let previous = match Connection::open(path) {
            Ok(live) => {
                let sound = damage(&live).map(|d| d.is_empty()).unwrap_or(false);
                match (sound, has_data(&live)) {
                    (true, Ok(true)) => Some(snapshot(&live, path, "before-restore")?),
                    (true, Ok(false)) => None,
                    _ => {
                        drop(live);
                        Some(set_aside_damaged(path)?)
                    }
                }
            }
            Err(_) => Some(set_aside_damaged(path)?),
        };
        let mut live = Connection::open(path)?;
        live.busy_timeout(std::time::Duration::from_secs(5))?;
        {
            let copy = rusqlite::backup::Backup::new(&source, &mut live)?;
            copy.run_to_completion(256, std::time::Duration::from_millis(0), None).with_context(|| match &previous {
                Some(p) => format!("restoring into {} failed; the previous database is at {}", path.display(), p.display()),
                None => format!("restoring into {} failed", path.display()),
            })?;
        }
        drop(live);
        drop(source);
        Store::open(path)?;
        Ok(previous)
    }

    /// Housekeeping for a long-running process: fold the write-ahead log back into the main file so it
    /// cannot grow without limit, and let SQLite refresh its query statistics.
    pub fn maintain(&self) -> Result<()> {
        let db = self.db();
        // Statistics first (that is itself a write), then the fold, so the log ends up empty.
        db.execute_batch("PRAGMA optimize;")?;
        db.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())).optional()?;
        Ok(())
    }

    fn db(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ---------- sessions ----------

    pub fn create_session(&self, title: &str) -> Result<String> {
        let id = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
        let now = now_ts();
        self.db().execute("INSERT INTO sessions(id, title, created_at, updated_at) VALUES (?,?,?,?)", params![id, title, now, now])?;
        Ok(id)
    }

    pub fn session_exists(&self, id: &str) -> bool {
        self.db().query_row("SELECT 1 FROM sessions WHERE id=?", [id], |_| Ok(())).optional().ok().flatten().is_some()
    }

    /// Resolve a full id or unique prefix.
    pub fn resolve_session(&self, prefix: &str) -> Result<String> {
        let db = self.db();
        let mut st = db.prepare("SELECT id FROM sessions WHERE id LIKE ? || '%' ORDER BY updated_at DESC LIMIT 2")?;
        let ids: Vec<String> = st.query_map([prefix], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        match ids.len() {
            0 => Err(anyhow!("no session matching '{prefix}'")),
            1 => Ok(ids[0].clone()),
            _ => Err(anyhow!("session prefix '{prefix}' is ambiguous")),
        }
    }

    pub fn append_message(&self, session_id: &str, msg: &Message) -> Result<()> {
        let now = now_ts();
        let db = self.db();
        db.execute("INSERT INTO messages(session_id, json, created_at) VALUES (?,?,?)", params![session_id, serde_json::to_string(msg)?, now])?;
        db.execute("UPDATE sessions SET updated_at=? WHERE id=?", params![now, session_id])?;
        Ok(())
    }

    pub fn load_messages(&self, session_id: &str) -> Result<Vec<Message>> {
        let db = self.db();
        let mut st = db.prepare("SELECT json FROM messages WHERE session_id=? ORDER BY id")?;
        let rows: Vec<String> = st.query_map([session_id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        Ok(rows.iter().filter_map(|j| serde_json::from_str(j).ok()).collect())
    }

    /// Replace the whole transcript (used after compaction).
    pub fn replace_messages(&self, session_id: &str, msgs: &[Message]) -> Result<()> {
        let mut db = self.db();
        let tx = db.transaction()?;
        tx.execute("DELETE FROM messages WHERE session_id=?", [session_id])?;
        for m in msgs {
            tx.execute("INSERT INTO messages(session_id, json, created_at) VALUES (?,?,?)", params![session_id, serde_json::to_string(m)?, now_ts()])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn list_sessions(&self, limit: usize) -> Result<Vec<SessionInfo>> {
        let db = self.db();
        let mut st = db.prepare(
            "SELECT s.id, s.title, s.created_at, s.updated_at, (SELECT COUNT(*) FROM messages m WHERE m.session_id=s.id)
             FROM sessions s ORDER BY s.updated_at DESC LIMIT ?",
        )?;
        let v = st
            .query_map([limit as i64], |r| Ok(SessionInfo { id: r.get(0)?, title: r.get(1)?, created_at: r.get(2)?, updated_at: r.get(3)?, messages: r.get(4)? }))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    pub fn set_session_title(&self, id: &str, title: &str) -> Result<()> {
        self.db().execute("UPDATE sessions SET title=? WHERE id=?", params![title, id])?;
        Ok(())
    }

    pub fn session_title(&self, id: &str) -> String {
        self.db().query_row("SELECT title FROM sessions WHERE id=?", [id], |r| r.get(0)).unwrap_or_default()
    }

    pub fn set_session_summary(&self, id: &str, summary: &str) -> Result<()> {
        self.db().execute("UPDATE sessions SET summary=? WHERE id=?", params![summary, id])?;
        Ok(())
    }

    pub fn session_summary(&self, id: &str) -> Option<String> {
        self.db().query_row("SELECT summary FROM sessions WHERE id=?", [id], |r| r.get(0)).ok().flatten()
    }

    pub fn delete_session(&self, id: &str) -> Result<()> {
        self.db().execute("DELETE FROM sessions WHERE id=?", [id])?;
        Ok(())
    }

    // ---------- memories ----------
    // A memory is never overwritten when a fact changes: the old row is marked invalid and points
    // at its replacement, so history is kept and only current facts are retrieved.

    #[allow(clippy::too_many_arguments)]
    pub fn insert_memory(&self, kind: &str, content: &str, tags: &str, importance: f64, source: &str, embedding: Option<&[u8]>, confirmed: bool, embedder: &str) -> Result<i64> {
        let now = now_ts();
        let db = self.db();
        db.execute(
            "INSERT INTO memories(kind, content, tags, importance, source, created_at, updated_at, embedding, confirmed, embedder) VALUES (?,?,?,?,?,?,?,?,?,?)",
            params![kind, content, tags, importance, source, now, now, embedding, confirmed as i64, embedder],
        )?;
        Ok(db.last_insert_rowid())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_memory(&self, id: i64, content: &str, tags: &str, importance: f64, embedding: Option<&[u8]>, confirmed: bool, embedder: &str) -> Result<()> {
        self.db().execute(
            "UPDATE memories SET content=?, tags=?, importance=?, embedding=?, confirmed=?, embedder=?, updated_at=? WHERE id=?",
            params![content, tags, importance, embedding, confirmed as i64, embedder, now_ts(), id],
        )?;
        Ok(())
    }

    /// Current facts whose vector was made by a different embedder than `embedder` (or is missing).
    pub fn memories_to_reindex(&self, embedder: &str, limit: usize) -> Result<Vec<(i64, String)>> {
        let db = self.db();
        let mut st = db.prepare("SELECT id, content FROM memories WHERE invalid_at IS NULL AND (embedding IS NULL OR embedder<>?) ORDER BY id LIMIT ?")?;
        let v = st.query_map(params![embedder, limit as i64], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    /// Flag a vector that no longer fits the current model, so it is made again.
    pub fn mark_embedding_stale(&self, id: i64) -> Result<()> {
        self.db().execute("UPDATE memories SET embedder='stale' WHERE id=?", [id])?;
        Ok(())
    }

    /// Replace only the vector of a memory (re-indexing leaves its text, date and standing untouched).
    pub fn set_memory_embedding(&self, id: i64, embedding: &[u8], embedder: &str) -> Result<()> {
        self.db().execute("UPDATE memories SET embedding=?, embedder=? WHERE id=?", params![embedding, embedder, id])?;
        Ok(())
    }

    /// Retire a fact in favour of a newer one.
    pub fn supersede_memory(&self, old_id: i64, new_id: i64) -> Result<()> {
        self.db().execute("UPDATE memories SET invalid_at=?, superseded_by=? WHERE id=? AND invalid_at IS NULL", params![now_ts(), new_id, old_id])?;
        Ok(())
    }

    /// Earlier versions of a fact, newest first.
    pub fn memory_history(&self, id: i64) -> Result<Vec<String>> {
        let db = self.db();
        let mut out = vec![];
        let mut cur = id;
        for _ in 0..20 {
            let prev: Option<(i64, String)> = db.query_row("SELECT id, content FROM memories WHERE superseded_by=? ORDER BY id DESC LIMIT 1", [cur], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
            match prev {
                Some((pid, content)) => {
                    out.push(content);
                    cur = pid;
                }
                None => break,
            }
        }
        Ok(out)
    }

    /// The user vouches for a fact that came from an outside source.
    pub fn confirm_memory(&self, id: i64) -> Result<bool> {
        Ok(self.db().execute("UPDATE memories SET confirmed=1, importance=MAX(importance, 0.7) WHERE id=?", [id])? > 0)
    }

    pub fn delete_memory(&self, id: i64) -> Result<bool> {
        Ok(self.db().execute("DELETE FROM memories WHERE id=?", [id])? > 0)
    }

    pub fn get_memory(&self, id: i64) -> Result<Option<MemoryItem>> {
        Ok(self.db().query_row(&format!("SELECT {MEM_COLS} FROM memories WHERE id=? AND invalid_at IS NULL"), [id], row_to_memory).optional()?)
    }

    pub fn list_memories(&self, kind: Option<&str>, limit: usize) -> Result<Vec<MemoryItem>> {
        let db = self.db();
        let sql = format!("SELECT {MEM_COLS} FROM memories WHERE invalid_at IS NULL AND (?1 IS NULL OR kind=?1) ORDER BY importance DESC, updated_at DESC LIMIT ?2");
        let mut st = db.prepare(&sql)?;
        let v = st.query_map(params![kind, limit as i64], row_to_memory)?.collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    /// The user's own (confirmed) facts of the given kinds, most important first across all of them.
    pub fn list_confirmed_memories(&self, kinds: &[&str], limit: usize) -> Result<Vec<MemoryItem>> {
        let db = self.db();
        let marks = vec!["?"; kinds.len()].join(",");
        let sql = format!("SELECT {MEM_COLS} FROM memories WHERE invalid_at IS NULL AND confirmed=1 AND kind IN ({marks}) ORDER BY importance DESC, updated_at DESC LIMIT {}", limit as i64);
        let mut st = db.prepare(&sql)?;
        let v = st.query_map(rusqlite::params_from_iter(kinds.iter()), row_to_memory)?.collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    /// Vectors of current facts made by one embedder. Vectors from different embedders live in
    /// different spaces and must never be compared with each other.
    pub fn memory_embeddings(&self, embedder: &str) -> Result<Vec<(i64, Vec<u8>)>> {
        let db = self.db();
        let mut st = db.prepare("SELECT id, embedding FROM memories WHERE embedding IS NOT NULL AND invalid_at IS NULL AND embedder=?")?;
        let v = st.query_map([embedder], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    /// Full-text search over current facts; returns ids ordered best-first.
    pub fn fts_memories(&self, match_expr: &str, limit: usize) -> Result<Vec<i64>> {
        let db = self.db();
        let mut st = db.prepare(
            "SELECT f.rowid FROM memories_fts f JOIN memories m ON m.id=f.rowid WHERE memories_fts MATCH ? AND m.invalid_at IS NULL ORDER BY bm25(memories_fts) LIMIT ?",
        )?;
        let v = st.query_map(params![match_expr, limit as i64], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    pub fn touch_memories(&self, ids: &[i64]) -> Result<()> {
        let db = self.db();
        for id in ids {
            db.execute("UPDATE memories SET uses=uses+1, last_used=? WHERE id=?", params![now_ts(), id])?;
        }
        Ok(())
    }

    pub fn memory_count(&self) -> i64 {
        self.db().query_row("SELECT COUNT(*) FROM memories WHERE invalid_at IS NULL", [], |r| r.get(0)).unwrap_or(0)
    }

    // ---------- tasks ----------

    pub fn add_task(&self, title: &str, notes: &str, priority: i64, due: Option<&str>) -> Result<i64> {
        let db = self.db();
        db.execute("INSERT INTO tasks(title, notes, priority, due, created_at) VALUES (?,?,?,?,?)", params![title, notes, priority, due, now_ts()])?;
        Ok(db.last_insert_rowid())
    }

    pub fn list_tasks(&self, status: Option<&str>) -> Result<Vec<Task>> {
        let db = self.db();
        let mut st = db.prepare(
            "SELECT id, title, notes, status, priority, due, created_at, owner FROM tasks WHERE (?1 IS NULL OR status=?1)
             ORDER BY status='done', priority, COALESCE(due,'9999'), id",
        )?;
        let v = st
            .query_map(params![status], |r| Ok(Task { id: r.get(0)?, title: r.get(1)?, notes: r.get(2)?, status: r.get(3)?, priority: r.get(4)?, due: r.get(5)?, created_at: r.get(6)?, owner: r.get(7)? }))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    pub fn update_task(&self, id: i64, title: Option<&str>, notes: Option<&str>, status: Option<&str>, priority: Option<i64>, due: Option<&str>) -> Result<bool> {
        let db = self.db();
        let n = db.execute(
            "UPDATE tasks SET title=COALESCE(?2,title), notes=COALESCE(?3,notes), status=COALESCE(?4,status),
             priority=COALESCE(?5,priority), due=COALESCE(?6,due),
             done_at=CASE WHEN ?4='done' THEN ?7 ELSE done_at END WHERE id=?1",
            params![id, title, notes, status, priority, due, now_ts()],
        )?;
        Ok(n > 0)
    }

    pub fn delete_task(&self, id: i64) -> Result<bool> {
        Ok(self.db().execute("DELETE FROM tasks WHERE id=?", [id])? > 0)
    }

    pub fn set_task_owner(&self, id: i64, owner: &str) -> Result<bool> {
        Ok(self.db().execute("UPDATE tasks SET owner=? WHERE id=?", params![if owner == "heron" { "heron" } else { "user" }, id])? > 0)
    }

    // ---------- approvals parked for later ----------

    #[allow(clippy::too_many_arguments)]
    pub fn park_approval(&self, session_id: &str, tool: &str, args: &str, summary: &str, detail: &str, risk: &str, tainted: bool, origin: &str) -> Result<i64> {
        let db = self.db();
        // The same request from the same piece of work is kept once.
        let existing: Option<i64> = db.query_row("SELECT id FROM approvals WHERE status='new' AND tool=? AND args=? AND origin=?", params![tool, args, origin], |r| r.get(0)).optional()?;
        if let Some(id) = existing {
            return Ok(id);
        }
        db.execute(
            "INSERT INTO approvals(ts, session_id, tool, args, summary, detail, risk, tainted, origin) VALUES (?,?,?,?,?,?,?,?,?)",
            params![now_ts(), session_id, tool, args, summary, detail, risk, tainted as i64, origin],
        )?;
        Ok(db.last_insert_rowid())
    }

    fn approval_rows(&self, sql: &str, args: &[&dyn rusqlite::ToSql]) -> Result<Vec<ParkedApproval>> {
        let db = self.db();
        let mut st = db.prepare(sql)?;
        let v = st
            .query_map(args, |r| {
                Ok(ParkedApproval {
                    id: r.get(0)?, ts: r.get(1)?, session_id: r.get(2)?, tool: r.get(3)?, args: r.get(4)?, summary: r.get(5)?, detail: r.get(6)?,
                    risk: r.get(7)?, tainted: r.get::<_, i64>(8)? != 0, origin: r.get(9)?, status: r.get(10)?, result: r.get(11)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    pub fn pending_approvals(&self) -> Result<Vec<ParkedApproval>> {
        self.approval_rows("SELECT id, ts, session_id, tool, args, summary, detail, risk, tainted, origin, status, result FROM approvals WHERE status='new' ORDER BY id DESC LIMIT 50", &[])
    }

    /// Take a waiting request for execution. Exactly one caller gets `true`, however many ask at the
    /// same moment: the change from "new" to "running" is a single statement that only succeeds while
    /// the request is still new.
    pub fn claim_approval(&self, id: i64) -> Result<bool> {
        Ok(self.db().execute("UPDATE approvals SET status='running' WHERE id=? AND status='new'", [id])? == 1)
    }

    /// Requests that were being carried out when Heron last stopped. Whether they finished is not
    /// known, so they are neither repeated nor reported as done: the user is told and decides.
    pub fn recover_interrupted_approvals(&self) -> Result<usize> {
        Ok(self.db().execute(
            "UPDATE approvals SET status='unknown', result='Heron stopped while doing this, so it is not known whether it finished. Check before asking for it again.' WHERE status='running'",
            [],
        )?)
    }

    pub fn get_approval(&self, id: i64) -> Option<ParkedApproval> {
        self.approval_rows("SELECT id, ts, session_id, tool, args, summary, detail, risk, tainted, origin, status, result FROM approvals WHERE id=?", &[&id]).ok()?.pop()
    }

    pub fn resolve_approval(&self, id: i64, status: &str, result: Option<&str>) -> Result<()> {
        self.db().execute("UPDATE approvals SET status=?, result=? WHERE id=?", params![status, result, id])?;
        Ok(())
    }

    // ---------- schedules ----------

    /// A job that runs when something happens (a file changes, an email arrives) rather than at a time.
    pub fn add_trigger(&self, name: &str, trigger: &str, prompt: &str, allow: &str) -> Result<i64> {
        let db = self.db();
        db.execute("INSERT INTO schedules(name, cron, prompt, next_run, created_at, allow) VALUES (?,?,?,NULL,?,?)", params![name, trigger, prompt, now_ts(), allow])?;
        Ok(db.last_insert_rowid())
    }

    pub fn triggers(&self) -> Result<Vec<Schedule>> {
        let db = self.db();
        let mut st = db.prepare(&format!("SELECT {SCHED_COLS} FROM schedules WHERE enabled=1 AND cron LIKE '@%' ORDER BY id"))?;
        let v = st.query_map([], row_to_schedule)?.collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    pub fn set_schedule_workflow(&self, id: i64, workflow_json: &str) -> Result<()> {
        self.db().execute("UPDATE schedules SET workflow=? WHERE id=?", params![workflow_json, id])?;
        Ok(())
    }

    pub fn set_schedule_allow(&self, id: i64, allow: &str) -> Result<bool> {
        Ok(self.db().execute("UPDATE schedules SET allow=? WHERE id=?", params![allow, id])? > 0)
    }

    pub fn get_schedule(&self, id: i64) -> Option<Schedule> {
        self.db().query_row(&format!("SELECT {SCHED_COLS} FROM schedules WHERE id=?"), [id], row_to_schedule).ok()
    }

    /// Keep the record of an automation run (the last 30 per automation).
    pub fn add_run(&self, schedule_id: i64, status: &str, tokens: u64, record_json: &str) -> Result<()> {
        let db = self.db();
        db.execute("INSERT INTO runs(schedule_id, ts, status, tokens, record) VALUES (?,?,?,?,?)", params![schedule_id, now_ts(), status, tokens as i64, record_json])?;
        db.execute("DELETE FROM runs WHERE schedule_id=?1 AND id NOT IN (SELECT id FROM runs WHERE schedule_id=?1 ORDER BY id DESC LIMIT 30)", [schedule_id])?;
        Ok(())
    }

    /// (when, status, tokens, record JSON), newest first.
    pub fn list_runs(&self, schedule_id: i64, limit: usize) -> Result<Vec<(i64, String, i64, String)>> {
        let db = self.db();
        let mut st = db.prepare("SELECT ts, status, tokens, record FROM runs WHERE schedule_id=? ORDER BY id DESC LIMIT ?")?;
        let v = st.query_map(params![schedule_id, limit as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    pub fn set_schedule_quiet(&self, id: i64, quiet: bool) -> Result<()> {
        self.db().execute("UPDATE schedules SET quiet=? WHERE id=?", params![quiet as i64, id])?;
        Ok(())
    }

    pub fn record_schedule_run(&self, id: i64, result: &str, tokens: u64) -> Result<()> {
        self.db().execute("UPDATE schedules SET last_result=?, last_tokens=?, last_run=? WHERE id=?", params![result, tokens as i64, now_ts(), id])?;
        Ok(())
    }

    pub fn knowledge_after(&self, source: &str, after_id: i64, limit: usize) -> Result<Vec<KnowledgeItem>> {
        self.knowledge_rows("SELECT id, source, ref, title, body, ts FROM knowledge WHERE source=?1 AND id>?2 ORDER BY id LIMIT ?3", &[&source, &after_id, &(limit as i64)])
    }

    pub fn max_knowledge_id(&self, source: &str) -> i64 {
        self.db().query_row("SELECT COALESCE(MAX(id),0) FROM knowledge WHERE source=?", [source], |r| r.get(0)).unwrap_or(0)
    }

    pub fn unconfirmed_memory_count(&self) -> i64 {
        self.db().query_row("SELECT COUNT(*) FROM memories WHERE confirmed=0 AND invalid_at IS NULL", [], |r| r.get(0)).unwrap_or(0)
    }


    pub fn add_schedule(&self, name: &str, cron: &str, prompt: &str, next_run: i64, allow: &str) -> Result<i64> {
        let db = self.db();
        db.execute("INSERT INTO schedules(name, cron, prompt, next_run, created_at, allow) VALUES (?,?,?,?,?,?)", params![name, cron, prompt, next_run, now_ts(), allow])?;
        Ok(db.last_insert_rowid())
    }

    pub fn list_schedules(&self) -> Result<Vec<Schedule>> {
        let db = self.db();
        let mut st = db.prepare(&format!("SELECT {SCHED_COLS} FROM schedules ORDER BY enabled DESC, next_run"))?;
        let v = st.query_map([], row_to_schedule)?.collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    pub fn due_schedules(&self, now: i64) -> Result<Vec<Schedule>> {
        let db = self.db();
        let mut st = db.prepare(&format!("SELECT {SCHED_COLS} FROM schedules WHERE enabled=1 AND next_run IS NOT NULL AND next_run<=? ORDER BY next_run"))?;
        let v = st.query_map([now], row_to_schedule)?.collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    /// Advance a schedule *before* running it so a crash can never cause a re-run loop.
    pub fn advance_schedule(&self, id: i64, next_run: Option<i64>) -> Result<()> {
        self.db().execute(
            "UPDATE schedules SET last_run=?, next_run=?, enabled=CASE WHEN ?2 IS NULL THEN 0 ELSE enabled END WHERE id=?",
            params![now_ts(), next_run, id],
        )?;
        Ok(())
    }

    /// Take a due job for running. In one transaction the schedule moves on to its next time and a
    /// "running" record is written, and only if the job is still due at the time that was seen. So
    /// two schedulers looking at the same moment cannot both run it, a crash cannot make it fire in a
    /// loop, and a run that is cut short leaves a record behind instead of vanishing.
    /// Returns the run's id, or None if someone else took the job first.
    pub fn claim_due_job(&self, job: &Schedule, next_run: Option<i64>) -> Result<Option<i64>> {
        let mut db = self.db();
        let tx = db.transaction()?;
        let taken = tx.execute(
            "UPDATE schedules SET last_run=?1, next_run=?2, enabled=CASE WHEN ?2 IS NULL THEN 0 ELSE enabled END WHERE id=?3 AND enabled=1 AND next_run IS ?4",
            params![now_ts(), next_run, job.id, job.next_run],
        )?;
        if taken != 1 {
            return Ok(None);
        }
        tx.execute("INSERT INTO job_runs(schedule_id, name, due_at, started_at, status) VALUES (?,?,?,?,'running')", params![job.id, job.name, job.next_run.unwrap_or(0), now_ts()])?;
        let id = tx.last_insert_rowid();
        // Keep the last 500 finished runs. A run still in progress is never removed, however old.
        tx.execute("DELETE FROM job_runs WHERE status != 'running' AND id NOT IN (SELECT id FROM job_runs WHERE status != 'running' ORDER BY id DESC LIMIT 500)", [])?;
        tx.commit()?;
        Ok(Some(id))
    }

    pub fn finish_job_run(&self, run_id: i64, status: &str, note: &str) -> Result<()> {
        self.db().execute("UPDATE job_runs SET status=?, note=?, finished_at=? WHERE id=? AND status='running'", params![status, note, now_ts(), run_id])?;
        Ok(())
    }

    /// Runs that were under way when Heron last stopped. Whether they finished is not known, so they are
    /// marked that way and returned (schedule id, job name) for the user to be told. They are not repeated.
    pub fn recover_interrupted_jobs(&self, notice: &str) -> Result<Vec<(i64, String)>> {
        let mut db = self.db();
        // One transaction: a run is never marked without its notice being written, so stopping
        // half-way cannot leave an interrupted run that nobody is told about.
        let tx = db.transaction()?;
        let found: Vec<(i64, String)> = {
            let mut st = tx.prepare("SELECT schedule_id, name FROM job_runs WHERE status='running' ORDER BY id")?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
            rows
        };
        let now = now_ts();
        tx.execute("UPDATE job_runs SET status='unknown', finished_at=?, note='Heron stopped while this was running' WHERE status='running'", [now])?;
        for (_, name) in &found {
            tx.execute("INSERT INTO inbox(ts, title, body) VALUES (?,?,?)", params![now, name, notice])?;
        }
        tx.commit()?;
        Ok(found)
    }

    /// Recent runs of scheduled jobs, newest first: (job name, due, started, finished, status, note).
    #[allow(clippy::type_complexity)]
    pub fn job_runs(&self, limit: usize) -> Result<Vec<(String, i64, i64, Option<i64>, String, String)>> {
        let db = self.db();
        let mut st = db.prepare("SELECT name, due_at, started_at, finished_at, status, note FROM job_runs ORDER BY id DESC LIMIT ?")?;
        let v = st.query_map([limit as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?.collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    pub fn set_schedule_result(&self, id: i64, result: &str) -> Result<()> {
        self.db().execute("UPDATE schedules SET last_result=? WHERE id=?", params![result, id])?;
        Ok(())
    }

    pub fn remove_schedule(&self, id: i64) -> Result<bool> {
        let _ = self.db().execute("DELETE FROM runs WHERE schedule_id=?", [id]);
        Ok(self.db().execute("DELETE FROM schedules WHERE id=?", [id])? > 0)
    }

    // ---------- inbox ----------

    pub fn add_inbox(&self, title: &str, body: &str) -> Result<()> {
        self.db().execute("INSERT INTO inbox(ts, title, body) VALUES (?,?,?)", params![now_ts(), title, body])?;
        Ok(())
    }

    pub fn inbox(&self, unread_only: bool, limit: usize) -> Result<Vec<InboxItem>> {
        let db = self.db();
        let mut st = db.prepare("SELECT id, ts, title, body, read FROM inbox WHERE (?1=0 OR read=0) ORDER BY id DESC LIMIT ?2")?;
        let v = st
            .query_map(params![unread_only as i64, limit as i64], |r| Ok(InboxItem { id: r.get(0)?, ts: r.get(1)?, title: r.get(2)?, body: r.get(3)?, read: r.get::<_, i64>(4)? != 0 }))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    pub fn mark_inbox_read(&self) -> Result<()> {
        self.db().execute("UPDATE inbox SET read=1 WHERE read=0", [])?;
        Ok(())
    }

    // ---------- audit ----------

    /// Append to the activity log. The log is append-only (the database refuses changes and deletions)
    /// and every entry carries a hash of itself and the entry before it, so tampering can be detected.
    pub fn audit(&self, session_id: &str, tool: &str, args: &str, decision: &str, result: &str) {
        let db = self.db();
        let ts = now_ts();
        let prev: String = db.query_row("SELECT COALESCE(hash,'') FROM audit ORDER BY id DESC LIMIT 1", [], |r| r.get(0)).unwrap_or_default();
        let hash = audit_hash(&prev, ts, session_id, tool, args, decision, result);
        let _ = db.execute(
            "INSERT INTO audit(ts, session_id, tool, args, decision, result, hash) VALUES (?,?,?,?,?,?,?)",
            params![ts, session_id, tool, args, decision, result, hash],
        );
    }

    /// Check the activity log's hash chain. Returns how many entries were checked and the id of the
    /// first entry that does not match (None = intact). Entries written before chaining existed are skipped.
    pub fn verify_audit(&self) -> Result<(usize, Option<i64>)> {
        let db = self.db();
        let mut st = db.prepare("SELECT id, ts, session_id, tool, args, decision, result, hash FROM audit ORDER BY id")?;
        let rows = st.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, String>(5)?, r.get::<_, String>(6)?, r.get::<_, Option<String>>(7)?))
        })?;
        let (mut prev, mut checked) = (String::new(), 0usize);
        for row in rows {
            let (id, ts, session, tool, args, decision, result, hash) = row?;
            let Some(hash) = hash else { continue };
            if hash != audit_hash(&prev, ts, &session, &tool, &args, &decision, &result) {
                return Ok((checked, Some(id)));
            }
            prev = hash;
            checked += 1;
        }
        Ok((checked, None))
    }

    pub fn audit_log(&self, limit: usize) -> Result<Vec<AuditEntry>> {
        let db = self.db();
        let mut st = db.prepare("SELECT ts, session_id, tool, args, decision, result FROM audit ORDER BY id DESC LIMIT ?")?;
        let v = st
            .query_map([limit as i64], |r| Ok(AuditEntry { ts: r.get(0)?, session_id: r.get(1)?, tool: r.get(2)?, args: r.get(3)?, decision: r.get(4)?, result: r.get(5)? }))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    // ---------- checkpoints (undo for file writes) ----------

    /// Note a file change that is about to be made: what was there (`prev`), and a hash of what the
    /// file will hold afterwards, so undo can tell later whether anyone has touched it since.
    pub fn save_checkpoint(&self, session_id: &str, path: &str, prev: Option<&[u8]>, after_hash: Option<&str>) -> Result<i64> {
        let db = self.db();
        db.execute("INSERT INTO checkpoints(ts, session_id, path, existed, prev, after_hash) VALUES (?,?,?,?,?,?)", params![now_ts(), session_id, path, prev.is_some() as i64, prev, after_hash])?;
        Ok(db.last_insert_rowid())
    }

    /// The change turned out not to happen (the write failed): there is nothing to undo.
    pub fn drop_checkpoint(&self, id: i64) -> Result<()> {
        self.db().execute("DELETE FROM checkpoints WHERE id=?", [id])?;
        Ok(())
    }

    /// The most recent checkpoint of a conversation, left in place:
    /// (id, path, previous content or None if the file did not exist, hash after Heron's change).
    #[allow(clippy::type_complexity)]
    pub fn last_checkpoint(&self, session_id: &str) -> Result<Option<(i64, String, Option<Vec<u8>>, Option<String>)>> {
        let row: Option<(i64, String, i64, Option<Vec<u8>>, Option<String>)> = self
            .db()
            .query_row("SELECT id, path, existed, prev, after_hash FROM checkpoints WHERE session_id=? ORDER BY id DESC LIMIT 1", [session_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .optional()?;
        Ok(row.map(|(id, path, existed, prev, after)| (id, path, if existed != 0 { Some(prev.unwrap_or_default()) } else { None }, after)))
    }

    /// When a checkpoint was made (seconds), or None if it is gone.
    pub fn checkpoint_made_at(&self, id: i64) -> Result<Option<i64>> {
        Ok(self.db().query_row("SELECT ts FROM checkpoints WHERE id=?", [id], |r| r.get(0)).optional()?)
    }

    /// Pop the most recent checkpoint for a session: (path, previous content or None if the file did not exist).
    pub fn pop_checkpoint(&self, session_id: &str) -> Result<Option<(String, Option<Vec<u8>>)>> {
        let db = self.db();
        let row: Option<(i64, String, i64, Option<Vec<u8>>)> = db
            .query_row("SELECT id, path, existed, prev FROM checkpoints WHERE session_id=? ORDER BY id DESC LIMIT 1", [session_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .optional()?;
        let Some((id, path, existed, prev)) = row else { return Ok(None) };
        db.execute("DELETE FROM checkpoints WHERE id=?", [id])?;
        Ok(Some((path, if existed != 0 { Some(prev.unwrap_or_default()) } else { None })))
    }

    // ---------- observations (background awareness journal) ----------

    /// Record that something was noticed. Repeats of the same thing within 30 minutes collapse into one row.
    pub fn observe(&self, kind: &str, subject: &str, ts: i64) -> bool {
        self.db().execute("INSERT OR IGNORE INTO observations(ts, kind, subject, bucket) VALUES (?,?,?,?)", params![ts, kind, subject, ts / 1800]).map(|n| n > 0).unwrap_or(false)
    }

    /// (ts, kind, subject), newest first.
    pub fn observations_since(&self, since: i64, limit: usize) -> Result<Vec<(i64, String, String)>> {
        let db = self.db();
        let mut st = db.prepare("SELECT ts, kind, subject FROM observations WHERE ts>=? ORDER BY ts DESC LIMIT ?")?;
        let v = st.query_map(params![since, limit as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    pub fn clear_observations(&self) -> Result<usize> {
        Ok(self.db().execute("DELETE FROM observations", [])?)
    }

    /// Keep the journal bounded: drop anything older than `keep_secs`.
    pub fn prune_observations(&self, keep_secs: i64) {
        let _ = self.db().execute("DELETE FROM observations WHERE ts<?", [now_ts() - keep_secs]);
    }

    pub fn memories_by_source(&self, source: &str, limit: usize) -> Result<Vec<MemoryItem>> {
        let db = self.db();
        let mut st = db.prepare(&format!("SELECT {MEM_COLS} FROM memories WHERE source=? AND invalid_at IS NULL ORDER BY updated_at DESC LIMIT ?"))?;
        let v = st.query_map(params![source, limit as i64], row_to_memory)?.collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    // ---------- knowledge (connected sources) ----------

    /// Insert or refresh an item. Returns true when it is new or its content changed.
    pub fn upsert_knowledge(&self, source: &str, reference: &str, title: &str, body: &str, ts: i64) -> Result<bool> {
        let db = self.db();
        let existing: Option<(i64, String, String)> =
            db.query_row("SELECT id, title, body FROM knowledge WHERE source=? AND ref=?", params![source, reference], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional()?;
        match existing {
            Some((id, t, b)) if t == title && b == body => {
                // Same text, but the date may have moved (a rescheduled event): keep it current
                // without sending the text to be read again.
                db.execute("UPDATE knowledge SET ts=? WHERE id=? AND ts<>?", params![ts, id, ts])?;
                Ok(false)
            }
            Some((id, _, _)) => {
                db.execute("UPDATE knowledge SET title=?, body=?, ts=?, digested=0 WHERE id=?", params![title, body, ts, id])?;
                Ok(true)
            }
            None => {
                db.execute("INSERT INTO knowledge(source, ref, title, body, ts) VALUES (?,?,?,?,?)", params![source, reference, title, body, ts])?;
                Ok(true)
            }
        }
    }

    fn knowledge_rows(&self, sql: &str, args: &[&dyn rusqlite::ToSql]) -> Result<Vec<KnowledgeItem>> {
        let db = self.db();
        let mut st = db.prepare(sql)?;
        let v = st
            .query_map(args, |r| Ok(KnowledgeItem { id: r.get(0)?, source: r.get(1)?, reference: r.get(2)?, title: r.get(3)?, body: r.get(4)?, ts: r.get(5)? }))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    pub fn search_knowledge(&self, match_expr: &str, source: Option<&str>, limit: usize) -> Result<Vec<KnowledgeItem>> {
        self.knowledge_rows(
            "SELECT k.id, k.source, k.ref, k.title, k.body, k.ts FROM knowledge_fts f JOIN knowledge k ON k.id=f.rowid
             WHERE knowledge_fts MATCH ?1 AND (?2 IS NULL OR k.source=?2) ORDER BY bm25(knowledge_fts) LIMIT ?3",
            &[&match_expr, &source, &(limit as i64)],
        )
    }

    /// Items of a source, newest first. `undigested_only` = not yet learned from.
    pub fn knowledge_of(&self, source: &str, undigested_only: bool, limit: usize) -> Result<Vec<KnowledgeItem>> {
        self.knowledge_rows(
            "SELECT id, source, ref, title, body, ts FROM knowledge WHERE source=?1 AND (?2=0 OR digested=0) ORDER BY ts DESC LIMIT ?3",
            &[&source, &(undigested_only as i64), &(limit as i64)],
        )
    }

    /// Items of a source whose time falls in a window, soonest first (calendar lookups).
    pub fn knowledge_between(&self, source: &str, from: i64, to: i64, limit: usize) -> Result<Vec<KnowledgeItem>> {
        self.knowledge_rows(
            "SELECT id, source, ref, title, body, ts FROM knowledge WHERE source=?1 AND ts>=?2 AND ts<=?3 ORDER BY ts LIMIT ?4",
            &[&source, &from, &to, &(limit as i64)],
        )
    }

    pub fn mark_digested(&self, ids: &[i64]) {
        let db = self.db();
        for id in ids {
            let _ = db.execute("UPDATE knowledge SET digested=1 WHERE id=?", [id]);
        }
    }

    pub fn knowledge_count(&self, source: &str) -> i64 {
        self.db().query_row("SELECT COUNT(*) FROM knowledge WHERE source=?", [source], |r| r.get(0)).unwrap_or(0)
    }

    pub fn clear_knowledge(&self, source: &str) -> Result<usize> {
        Ok(self.db().execute("DELETE FROM knowledge WHERE source=?", [source])?)
    }

    pub fn delete_memories_by_source(&self, source: &str) -> Result<usize> {
        Ok(self.db().execute("DELETE FROM memories WHERE source=?", [source])?)
    }

    // ---------- suggestions (proactive work) ----------

    pub fn add_suggestion(&self, title: &str, why: &str, prompt: &str) -> Result<i64> {
        let db = self.db();
        db.execute("INSERT INTO suggestions(ts, title, why, prompt) VALUES (?,?,?,?)", params![now_ts(), title, why, prompt])?;
        Ok(db.last_insert_rowid())
    }

    /// Newest first. `open_only` keeps what still deserves the user's attention.
    pub fn list_suggestions(&self, open_only: bool, limit: usize) -> Result<Vec<Suggestion>> {
        let db = self.db();
        let mut st = db.prepare(
            "SELECT id, ts, title, why, prompt, status, result, session_id FROM suggestions
             WHERE (?1=0 OR status IN ('new','prepared','done','failed')) ORDER BY id DESC LIMIT ?2",
        )?;
        let v = st
            .query_map(params![open_only as i64, limit as i64], |r| {
                Ok(Suggestion { id: r.get(0)?, ts: r.get(1)?, title: r.get(2)?, why: r.get(3)?, prompt: r.get(4)?, status: r.get(5)?, result: r.get(6)?, session_id: r.get(7)? })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    pub fn update_suggestion(&self, id: i64, status: &str, result: Option<&str>, session_id: Option<&str>) -> Result<bool> {
        let n = self.db().execute(
            "UPDATE suggestions SET status=?2, result=COALESCE(?3,result), session_id=COALESCE(?4,session_id) WHERE id=?1",
            params![id, status, result, session_id],
        )?;
        Ok(n > 0)
    }

    pub fn get_suggestion(&self, id: i64) -> Option<Suggestion> {
        self.db()
            .query_row("SELECT id, ts, title, why, prompt, status, result, session_id FROM suggestions WHERE id=?", [id], |r| {
                Ok(Suggestion { id: r.get(0)?, ts: r.get(1)?, title: r.get(2)?, why: r.get(3)?, prompt: r.get(4)?, status: r.get(5)?, result: r.get(6)?, session_id: r.get(7)? })
            })
            .ok()
    }

    /// The conversation the user last said something in (empty ones, opened and closed, do not count).
    pub fn last_conversation(&self) -> Option<String> {
        self.db().query_row("SELECT s.id FROM sessions s WHERE EXISTS (SELECT 1 FROM messages m WHERE m.session_id=s.id) ORDER BY s.updated_at DESC LIMIT 1", [], |r| r.get(0)).ok()
    }

    /// Files Heron itself changed since `since` (absolute paths).
    pub fn checkpoint_paths_since(&self, since: i64) -> Vec<String> {
        let db = self.db();
        let Ok(mut st) = db.prepare("SELECT DISTINCT path FROM checkpoints WHERE ts>=?") else { return vec![] };
        st.query_map([since], |r| r.get(0)).map(|rows| rows.flatten().collect()).unwrap_or_default()
    }

    // ---------- token usage ----------

    /// Add one model call to the record for that day and model.
    pub fn record_usage(&self, day: &str, model: &str, prompt: u64, completion: u64) -> Result<()> {
        self.db().execute(
            "INSERT INTO usage(day, model, prompt, completion, calls) VALUES (?1,?2,?3,?4,1)
             ON CONFLICT(day, model) DO UPDATE SET prompt=prompt+?3, completion=completion+?4, calls=calls+1",
            params![day, model, prompt as i64, completion as i64],
        )?;
        Ok(())
    }

    /// Total tokens per day from `since` (YYYY-MM-DD) on, oldest first.
    pub fn usage_per_day(&self, since: &str) -> Result<Vec<(String, u64)>> {
        let db = self.db();
        let mut st = db.prepare("SELECT day, SUM(prompt + completion) FROM usage WHERE day>=? GROUP BY day ORDER BY day")?;
        let v = st.query_map([since], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64)))?.collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    /// Per model from `since` on: (model, tokens in, tokens out, calls), heaviest first.
    pub fn usage_per_model(&self, since: &str) -> Result<Vec<(String, u64, u64, u64)>> {
        let db = self.db();
        let mut st = db.prepare("SELECT model, SUM(prompt), SUM(completion), SUM(calls) FROM usage WHERE day>=? GROUP BY model ORDER BY SUM(prompt + completion) DESC")?;
        let v = st.query_map([since], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64, r.get::<_, i64>(2)? as u64, r.get::<_, i64>(3)? as u64)))?.collect::<rusqlite::Result<_>>()?;
        Ok(v)
    }

    // ---------- kv ----------

    pub fn kv_get(&self, k: &str) -> Option<String> {
        self.db().query_row("SELECT v FROM kv WHERE k=?", [k], |r| r.get(0)).ok()
    }

    pub fn kv_set(&self, k: &str, v: &str) -> Result<()> {
        self.db().execute("INSERT INTO kv(k,v) VALUES (?,?) ON CONFLICT(k) DO UPDATE SET v=excluded.v", params![k, v])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_and_messages_roundtrip() {
        let s = Store::in_memory().unwrap();
        let id = s.create_session("t").unwrap();
        s.append_message(&id, &Message::user("hi")).unwrap();
        s.append_message(&id, &Message::assistant("hello")).unwrap();
        assert_eq!(s.load_messages(&id).unwrap().len(), 2);
        assert_eq!(s.resolve_session(&id[..4]).unwrap(), id);
        s.replace_messages(&id, &[Message::user("only")]).unwrap();
        assert_eq!(s.load_messages(&id).unwrap()[0].text(), "only");
        assert_eq!(s.list_sessions(10).unwrap()[0].messages, 1);
        s.delete_session(&id).unwrap();
        assert!(s.load_messages(&id).unwrap().is_empty());
    }

    #[test]
    fn fts_stays_in_sync_on_update_and_delete() {
        let s = Store::in_memory().unwrap();
        let id = s.insert_memory("fact", "user loves espresso", "coffee", 0.5, "", None, true, "hash-v1").unwrap();
        assert_eq!(s.fts_memories("\"espresso\"", 5).unwrap(), vec![id]);
        s.update_memory(id, "user loves green tea", "tea", 0.5, None, true, "hash-v1").unwrap();
        assert!(s.fts_memories("\"espresso\"", 5).unwrap().is_empty());
        assert_eq!(s.fts_memories("\"tea\"", 5).unwrap(), vec![id]);
        assert!(s.delete_memory(id).unwrap());
        assert!(s.fts_memories("\"tea\"", 5).unwrap().is_empty());
    }

    #[test]
    fn activity_log_is_append_only_and_tamper_evident() {
        let s = Store::in_memory().unwrap();
        s.audit("s1", "shell", "{\"command\":\"ls\"}", "allowed", "ok");
        s.audit("s1", "write_file", "{\"path\":\"a\"}", "approved", "wrote");
        s.audit("s2", "shell", "{\"command\":\"rm x\"}", "denied-by-user", "");
        assert_eq!(s.verify_audit().unwrap(), (3, None));
        // The database itself refuses to change or remove entries.
        assert!(s.db().execute("UPDATE audit SET decision='allowed' WHERE id=3", []).is_err());
        assert!(s.db().execute("DELETE FROM audit WHERE id=2", []).is_err());
        assert_eq!(s.verify_audit().unwrap(), (3, None));
        // Someone who edits the file directly (bypassing those guards) is still caught by the chain.
        s.db().execute_batch("DROP TRIGGER audit_no_update; UPDATE audit SET decision='allowed' WHERE id=3;").unwrap();
        assert_eq!(s.verify_audit().unwrap(), (2, Some(3)));
        s.db().execute_batch("UPDATE audit SET decision='denied-by-user' WHERE id=3; UPDATE audit SET result='edited' WHERE id=1;").unwrap();
        assert_eq!(s.verify_audit().unwrap(), (0, Some(1)));
    }

    #[test]
    fn a_run_in_progress_is_never_pruned_and_is_reported_with_its_notice_in_one_step() {
        let s = Store::in_memory().unwrap();
        let slow = s.add_schedule("slow job", "0 9 * * *", "p", 1, "").unwrap();
        let quick = s.add_schedule("quick job", "* * * * *", "p", 1, "").unwrap();
        let job = |id: i64| s.list_schedules().unwrap().into_iter().find(|j| j.id == id).unwrap();

        // One long run starts and stays in progress while another job runs many hundreds of times.
        assert!(s.claim_due_job(&job(slow), Some(i64::MAX - 1)).unwrap().is_some());
        for n in 0..520 {
            let run = s.claim_due_job(&job(quick), Some(1000 + n)).unwrap().expect("claimed");
            s.finish_job_run(run, "ok", "").unwrap();
        }
        let runs = s.job_runs(1000).unwrap();
        // (Pruning happens as a run starts, so the newest finished run can sit one above the cap.)
        assert!((500..=501).contains(&runs.iter().filter(|r| r.4 == "ok").count()), "finished history is capped");
        assert_eq!(runs.iter().filter(|r| r.0 == "slow job" && r.4 == "running").count(), 1, "the run in progress is still on record");

        // Heron stops. On the next start the run is marked and its notice written together.
        let found = s.recover_interrupted_jobs("it is not known whether it finished").unwrap();
        assert_eq!(found, vec![(slow, "slow job".to_string())]);
        let notices: Vec<_> = s.inbox(true, 10).unwrap().into_iter().filter(|m| m.title == "slow job").collect();
        assert_eq!(notices.len(), 1);
        assert!(notices[0].body.contains("not known whether it finished"));
        // Starting again finds nothing new and says nothing twice.
        assert!(s.recover_interrupted_jobs("x").unwrap().is_empty());
        assert_eq!(s.inbox(true, 10).unwrap().iter().filter(|m| m.title == "slow job").count(), 1);
    }

    #[test]
    fn tasks_schedules_checkpoints() {
        let s = Store::in_memory().unwrap();
        let t = s.add_task("write report", "", 1, Some("2026-12-01")).unwrap();
        assert!(s.update_task(t, None, None, Some("done"), None, None).unwrap());
        assert_eq!(s.list_tasks(Some("done")).unwrap().len(), 1);
        assert!(s.list_tasks(Some("open")).unwrap().is_empty());

        let sc = s.add_schedule("daily", "0 9 * * *", "brief me", 100, "").unwrap();
        assert_eq!(s.due_schedules(100).unwrap().len(), 1);
        s.advance_schedule(sc, None).unwrap();
        assert!(s.due_schedules(i64::MAX).unwrap().is_empty());
        assert!(!s.list_schedules().unwrap()[0].enabled);

        let first = s.save_checkpoint("s", "/a", Some(b"old"), Some("abc")).unwrap();
        assert_eq!(s.last_checkpoint("s").unwrap(), Some((first, "/a".into(), Some(b"old".to_vec()), Some("abc".into()))));
        s.save_checkpoint("s", "/b", None, None).unwrap();
        assert_eq!(s.pop_checkpoint("s").unwrap(), Some(("/b".into(), None)));
        assert_eq!(s.pop_checkpoint("s").unwrap(), Some(("/a".into(), Some(b"old".to_vec()))));
        assert_eq!(s.pop_checkpoint("s").unwrap(), None);
    }
}

/// Upgrades, copies and restores, tried against a database built exactly as Heron 0.1.0 built it.
#[cfg(test)]
mod upgrade_tests {
    use super::*;

    const RELEASED: &str = include_str!("../tests/fixtures/schema-v0.1.0.sql");

    /// A database as 0.1.0 would have left it: its schema, no format number, and a little of everything in it.
    fn released_database(path: &Path) {
        let conn = Connection::open(path).unwrap();
        // (Git on Windows may hand the file over with CRLF line endings; 0.1.0 itself never wrote those.)
        conn.execute_batch(&RELEASED.replace("\r\n", "\n")).unwrap();
        conn.execute_batch(
            "INSERT INTO sessions(id, title, created_at, updated_at) VALUES ('abc123', 'Planning the trip', 100, 200);
             INSERT INTO messages(session_id, json, created_at) VALUES ('abc123', '{\"role\":\"user\",\"content\":\"book the ferry\"}', 100);
             INSERT INTO memories(kind, content, tags, importance, source, created_at, updated_at) VALUES ('fact', 'Prefers the morning ferry', 'travel', 0.8, 'user', 100, 100);
             INSERT INTO tasks(title, created_at) VALUES ('Renew passport', 100);
             INSERT INTO schedules(name, cron, prompt, next_run, created_at) VALUES ('Morning brief', '0 9 * * 1-5', 'brief me', 4102444800, 100);
             INSERT INTO schedules(name, cron, prompt, next_run, created_at, workflow, allow, quiet) VALUES ('Inbox digest', '0 18 * * *', 'digest', 4102444800, 100, '{\"steps\":[]}', 'web_search', 1);
             INSERT INTO runs(schedule_id, ts, status, tokens, record) VALUES (2, 150, 'ok', 1200, '[]');
             INSERT INTO inbox(ts, title, body) VALUES (150, 'Digest', 'three new things');
             INSERT INTO checkpoints(ts, session_id, path, existed, prev) VALUES (160, 'abc123', '/tmp/notes.md', 1, x'6f6c64');
             INSERT INTO kv(k, v) VALUES ('onboarded', '1');
             INSERT INTO knowledge(source, ref, title, body, ts) VALUES ('notes', 'a.md', 'Ferry times', 'departs at seven', 100);
             INSERT INTO suggestions(ts, title, prompt) VALUES (170, 'Check ferry prices', 'look up ferry prices');",
        )
        .unwrap();
        // Two entries in the activity log, chained the way 0.1.0 chained them.
        let first = audit_hash("", 100, "abc123", "read_file", "notes.md", "allowed", "ok");
        let second = audit_hash(&first, 101, "abc123", "write_file", "notes.md", "approved", "ok");
        conn.execute("INSERT INTO audit(ts, session_id, tool, args, decision, result, hash) VALUES (100, 'abc123', 'read_file', 'notes.md', 'allowed', 'ok', ?)", [&first]).unwrap();
        conn.execute("INSERT INTO audit(ts, session_id, tool, args, decision, result, hash) VALUES (101, 'abc123', 'write_file', 'notes.md', 'approved', 'ok', ?)", [&second]).unwrap();
        assert_eq!(user_version(&conn).unwrap(), 0, "0.1.0 did not number its format");
    }

    fn copies(db: &Path, starting: &str) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = std::fs::read_dir(backup_dir(db))
            .map(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.file_name().unwrap().to_string_lossy().starts_with(starting)).collect())
            .unwrap_or_default();
        found.sort();
        found
    }

    /// Every table, index and trigger with its definition, in a fixed order (search-index internals left out).
    fn shape(conn: &Connection) -> Vec<String> {
        let mut st = conn.prepare("SELECT type || ' ' || name || ': ' || COALESCE(sql, '') FROM sqlite_master WHERE name NOT LIKE '%\\_fts\\_%' ESCAPE '\\' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' ORDER BY type, name").unwrap();
        let rows = st.query_map([], |r| r.get::<_, String>(0)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
        rows
    }

    fn file_hash(path: &Path) -> String {
        use sha2::{Digest, Sha256};
        Sha256::digest(std::fs::read(path).unwrap()).iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn a_database_from_the_released_version_is_upgraded_with_everything_in_it_and_a_copy_kept() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("heron.db");
        released_database(&db);

        let s = Store::open(&db).unwrap();
        assert_eq!(s.schema_version().unwrap(), SCHEMA_VERSION);
        assert!(s.integrity().unwrap().is_empty());

        // Everything that was there still is, and reads back through the ordinary functions.
        assert_eq!(s.load_messages("abc123").unwrap().len(), 1);
        let memories = s.list_memories(None, 10).unwrap();
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].content, "Prefers the morning ferry");
        assert_eq!(s.fts_memories("ferry", 10).unwrap(), vec![memories[0].id], "the search index came along");
        assert_eq!(s.list_tasks(None).unwrap()[0].title, "Renew passport");
        let jobs = s.list_schedules().unwrap();
        assert_eq!(jobs.len(), 2, "reminders and automations are preserved");
        let digest = jobs.iter().find(|j| j.name == "Inbox digest").unwrap();
        assert_eq!((digest.cron.as_str(), digest.workflow.as_str(), digest.allow.as_str(), digest.quiet, digest.next_run), ("0 18 * * *", "{\"steps\":[]}", "web_search", true, Some(4102444800)));
        assert_eq!(s.inbox(false, 10).unwrap().len(), 1);
        assert_eq!(s.verify_audit().unwrap(), (2, None), "the activity log's chain is unbroken");
        assert_eq!(s.last_checkpoint("abc123").unwrap().map(|c| (c.1, c.2, c.3)), Some(("/tmp/notes.md".into(), Some(b"old".to_vec()), None)), "an old checkpoint has no after-hash and still restores");

        // What the newer format adds works on the upgraded database.
        s.record_usage("2026-11-13", "some-model", 10, 5).unwrap();
        assert_eq!(s.usage_per_day("2026-11-01").unwrap(), vec![("2026-11-13".to_string(), 15)]);
        assert!(s.claim_due_job(&jobs[0], None).unwrap().is_some());

        // One copy was made first, it is the database as it was, and only the user can read it.
        let kept = copies(&db, "heron-before-upgrade-from-v0-");
        assert_eq!(kept.len(), 1, "{kept:?}");
        let old = Connection::open(&kept[0]).unwrap();
        assert_eq!(user_version(&old).unwrap(), 0);
        assert!(!has_column(&old, "usage", "day") && !has_column(&old, "checkpoints", "after_hash"), "the copy is from before the upgrade");
        assert_eq!(old.query_row("SELECT count(*) FROM schedules", [], |r| r.get::<_, i64>(0)).unwrap(), 2);
        assert_eq!(old.query_row("SELECT content FROM memories", [], |r| r.get::<_, String>(0)).unwrap(), "Prefers the morning ferry");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&kept[0]).unwrap().permissions().mode() & 0o777, 0o600);
            assert_eq!(std::fs::metadata(backup_dir(&db)).unwrap().permissions().mode() & 0o777, 0o700);
        }

        // Opening again is not an upgrade and makes no second copy.
        drop(s);
        Store::open(&db).unwrap();
        assert_eq!(copies(&db, "heron-before-upgrade-").len(), 1);

        // The copy can be opened by this build like any older database, which is what a restore does.
        let again = dir.path().join("restored-elsewhere.db");
        std::fs::copy(&kept[0], &again).unwrap();
        assert_eq!(Store::open(&again).unwrap().list_schedules().unwrap().len(), 2);
    }

    #[test]
    fn an_upgraded_database_and_a_new_one_are_the_same_shape() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old.db");
        released_database(&old);
        drop(Store::open(&old).unwrap());
        let elsewhere = tempfile::tempdir().unwrap();
        let new = elsewhere.path().join("new.db");
        drop(Store::open(&new).unwrap());
        assert!(copies(&new, "heron-").is_empty(), "a new, empty database has nothing to copy");
        assert_eq!(shape(&Connection::open(&old).unwrap()), shape(&Connection::open(&new).unwrap()));

        // Builds made between 0.1.0 and the first numbered format already had the newer tables,
        // unnumbered. They upgrade too, keeping what is in them.
        let between = dir.path().join("between.db");
        released_database(&between);
        {
            let conn = Connection::open(&between).unwrap();
            conn.execute_batch(
                "CREATE TABLE usage(day TEXT NOT NULL, model TEXT NOT NULL, prompt INTEGER NOT NULL DEFAULT 0, completion INTEGER NOT NULL DEFAULT 0, calls INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(day, model));
                 INSERT INTO usage VALUES ('2026-11-12', 'm', 100, 50, 3);
                 ALTER TABLE checkpoints ADD COLUMN after_hash TEXT;",
            )
            .unwrap();
        }
        let s = Store::open(&between).unwrap();
        assert_eq!(s.schema_version().unwrap(), SCHEMA_VERSION);
        assert_eq!(s.usage_per_day("2026-11-01").unwrap(), vec![("2026-11-12".to_string(), 150)]);
        assert!(s.job_runs(10).is_ok());
    }

    /// If this fails you changed the schema. Do not edit an existing step: add a new one to `STEPS`
    /// (which raises `SCHEMA_VERSION`), then update the two values below together.
    #[test]
    fn the_schema_cannot_change_without_a_new_version() {
        use sha2::{Digest, Sha256};
        let s = Store::in_memory().unwrap();
        let text = shape(&s.db()).join("\n");
        let fingerprint: String = Sha256::digest(text.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect();
        assert_eq!((SCHEMA_VERSION, fingerprint.as_str()), (2, "fde4f56b77ab5de5"), "the schema is now:\n{text}");
    }

    #[test]
    fn a_database_from_a_newer_heron_is_refused_and_not_touched() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("heron.db");
        released_database(&db);
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(&format!("CREATE TABLE something_new(x); PRAGMA user_version={};", SCHEMA_VERSION + 1)).unwrap();
        }
        let before = file_hash(&db);
        let err = Store::open(&db).err().expect("must not open").to_string();
        assert!(err.contains("newer version of Heron") && err.contains("Nothing was changed"), "{err}");
        assert_eq!(file_hash(&db), before);
        assert!(!backup_dir(&db).exists(), "nothing was written anywhere");
    }

    #[test]
    fn an_upgrade_that_fails_part_way_leaves_the_database_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("heron.db");
        released_database(&db);
        let mut conn = Connection::open(&db).unwrap();
        fn breaks(conn: &Connection) -> Result<()> {
            conn.execute_batch("CREATE TABLE half_done(x); DELETE FROM tasks; ALTER TABLE schedules ADD COLUMN extra TEXT;")?;
            bail!("the disk filled up")
        }
        let err = format!("{:#}", migrate(&mut conn, &[to_v1, to_v2, breaks]).unwrap_err());
        assert!(err.contains("step to data format 3") && err.contains("the disk filled up"), "{err}");
        // Not even the steps that succeeded were kept: it is still the released format, whole.
        assert_eq!(user_version(&conn).unwrap(), 0);
        assert!(!has_column(&conn, "half_done", "x") && !has_column(&conn, "usage", "day") && !has_column(&conn, "schedules", "extra"));
        assert_eq!(conn.query_row("SELECT count(*) FROM tasks", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
        drop(conn);
        // And the real upgrade still works on it afterwards.
        assert_eq!(Store::open(&db).unwrap().list_tasks(None).unwrap().len(), 1);
    }

    #[test]
    fn a_damaged_database_is_not_upgraded() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("heron.db");
        released_database(&db);
        {
            // Enough rows to spread over many pages, then closed so everything is in the main file.
            let conn = Connection::open(&db).unwrap();
            for i in 0..400 {
                conn.execute("INSERT INTO memories(kind, content, created_at, updated_at) VALUES ('fact', ?, 1, 1)", [format!("{i} {}", "padding ".repeat(60))]).unwrap();
            }
            conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())).unwrap();
        }
        // Scribble over the middle of the file, leaving the first page (which says what the file is) alone.
        let mut bytes = std::fs::read(&db).unwrap();
        let (from, to) = (bytes.len() / 3, bytes.len() / 3 * 2);
        assert!(from > 8192);
        for b in &mut bytes[from..to] {
            *b = 0xA5;
        }
        std::fs::write(&db, &bytes).unwrap();
        let before = file_hash(&db);

        let err = Store::open(&db).err().expect("must not open a damaged database").to_string();
        assert!(err.contains("damaged") && err.contains("was not upgraded"), "{err}");
        assert_eq!(file_hash(&db), before, "it was left exactly as found");
        assert!(copies(&db, "heron-").is_empty(), "and no copy of a damaged file is passed off as a backup");
    }

    #[test]
    fn two_programs_starting_at_once_upgrade_the_database_once() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("heron.db");
        released_database(&db);
        let threads: Vec<_> = (0..12)
            .map(|_| {
                let db = db.clone();
                std::thread::spawn(move || Store::open(&db).map(|s| (s.schema_version().unwrap(), s.list_schedules().unwrap().len())).map_err(|e| format!("{e:#}")))
            })
            .collect();
        for t in threads {
            assert_eq!(t.join().unwrap(), Ok((SCHEMA_VERSION, 2)));
        }
        let s = Store::open(&db).unwrap();
        assert!(s.integrity().unwrap().is_empty());
        assert_eq!(s.verify_audit().unwrap(), (2, None));
    }

    #[test]
    fn a_backup_can_be_restored_and_a_restore_can_be_undone() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("heron.db");
        let s = Store::open(&db).unwrap();
        s.add_task("Keep me", "", 2, None).unwrap();
        s.insert_memory("fact", "Lives by the harbour", "", 0.5, "user", None, true, "hash-v1").unwrap();
        s.add_schedule("Morning brief", "0 9 * * *", "brief me", 4102444800, "").unwrap();
        s.audit("x", "read_file", "a", "allowed", "ok");

        let saved = s.backup(None).unwrap();
        assert!(saved.starts_with(backup_dir(&db)) && saved.file_name().unwrap().to_string_lossy().starts_with("heron-backup-"));
        let elsewhere = dir.path().join("my-copy.db");
        assert_eq!(s.backup(Some(&elsewhere)).unwrap(), elsewhere);
        assert!(s.backup(Some(&elsewhere)).is_err(), "a copy is never written over an existing file");

        // Things change after the backup...
        let later = s.add_task("Added later", "", 2, None).unwrap();
        s.update_task(1, None, None, Some("done"), None, None).unwrap();
        drop(s);

        // ...and restoring brings back the state at the backup, whole.
        let previous = Store::restore(&db, &saved).unwrap().expect("there was data to save first");
        let s = Store::open(&db).unwrap();
        let tasks = s.list_tasks(None).unwrap();
        assert_eq!(tasks.iter().map(|t| (t.title.as_str(), t.status.as_str())).collect::<Vec<_>>(), [("Keep me", "open")]);
        assert_eq!(s.list_memories(None, 10).unwrap().len(), 1);
        assert_eq!(s.fts_memories("harbour", 10).unwrap().len(), 1);
        assert_eq!(s.list_schedules().unwrap().len(), 1);
        assert_eq!(s.verify_audit().unwrap(), (1, None));
        assert!(s.integrity().unwrap().is_empty());
        drop(s);

        // What was there before the restore was saved first, so the restore itself can be undone.
        assert!(previous.file_name().unwrap().to_string_lossy().starts_with("heron-before-restore-"));
        Store::restore(&db, &previous).unwrap();
        let s = Store::open(&db).unwrap();
        assert!(s.list_tasks(None).unwrap().iter().any(|t| t.id == later && t.title == "Added later"));
        drop(s);

        // Things that are not usable backups are refused, and the database is left alone.
        let before = Store::open(&db).unwrap().list_tasks(None).unwrap().len();
        let junk = dir.path().join("junk.db");
        std::fs::write(&junk, "this is not a database at all, just some text that is long enough").unwrap();
        assert!(Store::restore(&db, &junk).is_err());
        let empty = dir.path().join("empty.db");
        Connection::open(&empty).unwrap().execute_batch("CREATE TABLE unrelated(x);").unwrap();
        assert!(Store::restore(&db, &empty).unwrap_err().to_string().contains("not a Heron database"));
        let newer = dir.path().join("newer.db");
        std::fs::copy(&elsewhere, &newer).unwrap();
        Connection::open(&newer).unwrap().execute_batch(&format!("PRAGMA user_version={};", SCHEMA_VERSION + 5)).unwrap();
        assert!(Store::restore(&db, &newer).unwrap_err().to_string().contains("newer version"));
        assert!(Store::restore(&db, &dir.path().join("missing.db")).is_err());
        assert_eq!(Store::open(&db).unwrap().list_tasks(None).unwrap().len(), before);

        // A backup from the released version restores into this one and is upgraded on the way.
        let released = dir.path().join("released.db");
        released_database(&released);
        Store::restore(&db, &released).unwrap();
        let s = Store::open(&db).unwrap();
        assert_eq!((s.schema_version().unwrap(), s.list_schedules().unwrap().len()), (SCHEMA_VERSION, 2));
        drop(s);

        // Restoring onto a computer that has no Heron data yet works, and there is nothing to save first.
        let fresh = dir.path().join("new-computer").join("heron.db");
        assert_eq!(Store::restore(&fresh, &elsewhere).unwrap(), None);
        assert_eq!(Store::open(&fresh).unwrap().list_tasks(None).unwrap()[0].title, "Keep me");
    }

    #[test]
    fn a_good_backup_can_be_restored_over_a_damaged_database() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("heron.db");
        let s = Store::open(&db).unwrap();
        s.add_task("Keep me", "", 2, None).unwrap();
        for i in 0..400 {
            s.insert_memory("fact", &format!("{i} {}", "padding ".repeat(60)), "", 0.5, "user", None, true, "hash-v1").unwrap();
        }
        let saved = s.backup(None).unwrap();
        s.maintain().unwrap();
        drop(s);

        // The database is damaged after the backup was made: Heron will not open it...
        let mut bytes = std::fs::read(&db).unwrap();
        let (from, to) = (bytes.len() / 3, bytes.len() / 3 * 2);
        for b in &mut bytes[from..to] {
            *b = 0xA5;
        }
        std::fs::write(&db, &bytes).unwrap();
        let broken = file_hash(&db);
        match Store::open(&db) {
            // Depending on which pages were hit, Heron either cannot open it at all (and says what to do)...
            Err(e) => assert!(format!("{e:#}").contains("heron restore"), "{e:#}"),
            // ...or opens it and the check reports the damage.
            Ok(s) => assert!(!s.integrity().unwrap().is_empty(), "the damage is real"),
        }

        // ...and this is the moment a restore is for. It works, and the damaged file is kept exactly
        // as it was, in case something can still be recovered from it.
        let aside = Store::restore(&db, &saved).unwrap().expect("the damaged database is kept");
        assert!(aside.file_name().unwrap().to_string_lossy().starts_with("heron-damaged-"), "{aside:?}");
        assert_eq!(file_hash(&aside), broken);
        let s = Store::open(&db).unwrap();
        assert!(s.integrity().unwrap().is_empty());
        assert_eq!(s.list_tasks(None).unwrap()[0].title, "Keep me");
        assert_eq!(s.list_memories(None, 1000).unwrap().len(), 400);
        drop(s);

        // A file that is not a database at all, sitting where the database should be, is handled the same way.
        std::fs::write(&db, "not a database, just some text that happens to be where the database belongs").unwrap();
        for part in ["heron.db-wal", "heron.db-shm"] {
            let _ = std::fs::remove_file(dir.path().join(part));
        }
        let said = format!("{:#}", Store::open(&db).err().expect("text is not a database"));
        assert!(said.contains("damaged and cannot be read") && said.contains("heron restore"), "{said}");
        let aside = Store::restore(&db, &saved).unwrap().unwrap();
        assert!(std::fs::read_to_string(&aside).unwrap().starts_with("not a database"));
        assert_eq!(Store::open(&db).unwrap().list_tasks(None).unwrap().len(), 1);
    }

    #[test]
    fn housekeeping_keeps_the_log_file_small_and_only_the_newest_upgrade_copies() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("heron.db");
        let s = Store::open(&db).unwrap();
        for i in 0..200 {
            s.add_inbox(&format!("note {i}"), &"body ".repeat(200)).unwrap();
        }
        let log = dir.path().join("heron.db-wal");
        assert!(std::fs::metadata(&log).unwrap().len() > 0, "writes go to the log first");
        s.maintain().unwrap();
        assert_eq!(std::fs::metadata(&log).unwrap().len(), 0, "and are folded into the main file");
        assert_eq!(s.inbox(false, 500).unwrap().len(), 200);

        // Five upgrade copies and one made by hand: the three newest upgrade copies stay, and the hand-made one.
        std::fs::create_dir_all(backup_dir(&db)).unwrap();
        for n in 1..=5 {
            let p = backup_dir(&db).join(format!("heron-before-upgrade-from-v{n}-x.db"));
            std::fs::write(&p, "x").unwrap();
            let when = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000 + n);
            std::fs::File::options().write(true).open(&p).unwrap().set_modified(when).unwrap();
        }
        std::fs::write(backup_dir(&db).join("heron-backup-mine.db"), "x").unwrap();
        prune_snapshots(&db, "before-upgrade-", KEEP_UPGRADE_SNAPSHOTS);
        let left: Vec<String> = copies(&db, "heron-").iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect();
        assert_eq!(left, ["heron-backup-mine.db", "heron-before-upgrade-from-v3-x.db", "heron-before-upgrade-from-v4-x.db", "heron-before-upgrade-from-v5-x.db"]);
    }
}
