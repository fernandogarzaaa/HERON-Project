//! SQLite persistence: sessions, messages, memories (FTS5 + vectors), tasks,
//! schedules, inbox, audit log and file checkpoints. One file, fully local.

use crate::llm::Message;
use crate::util::now_ts;
use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;
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

const SCHEMA: &str = r#"
PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;
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
CREATE TABLE IF NOT EXISTS job_runs(
  id INTEGER PRIMARY KEY, schedule_id INTEGER NOT NULL, name TEXT NOT NULL, due_at INTEGER NOT NULL,
  started_at INTEGER NOT NULL, finished_at INTEGER, status TEXT NOT NULL, note TEXT NOT NULL DEFAULT '');
CREATE INDEX IF NOT EXISTS idx_job_runs_status ON job_runs(status);
CREATE TABLE IF NOT EXISTS inbox(
  id INTEGER PRIMARY KEY, ts INTEGER NOT NULL, title TEXT NOT NULL, body TEXT NOT NULL, read INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS audit(
  id INTEGER PRIMARY KEY, ts INTEGER NOT NULL, session_id TEXT NOT NULL, tool TEXT NOT NULL,
  args TEXT NOT NULL, decision TEXT NOT NULL, result TEXT NOT NULL DEFAULT '', hash TEXT);
CREATE TABLE IF NOT EXISTS checkpoints(
  id INTEGER PRIMARY KEY, ts INTEGER NOT NULL, session_id TEXT NOT NULL, path TEXT NOT NULL,
  existed INTEGER NOT NULL, prev BLOB);
CREATE TABLE IF NOT EXISTS kv(k TEXT PRIMARY KEY, v TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS usage(day TEXT NOT NULL, model TEXT NOT NULL, prompt INTEGER NOT NULL DEFAULT 0, completion INTEGER NOT NULL DEFAULT 0, calls INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(day, model));
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
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn in_memory() -> Result<Store> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Store> {
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(SCHEMA)?;
        // Databases created by earlier versions.
        if conn.prepare("SELECT after_hash FROM checkpoints LIMIT 0").is_err() {
            conn.execute_batch("ALTER TABLE checkpoints ADD COLUMN after_hash TEXT;")?;
        }
        if conn.prepare("SELECT hash FROM audit LIMIT 0").is_err() {
            conn.execute_batch("ALTER TABLE audit ADD COLUMN hash TEXT;")?;
        }
        if conn.prepare("SELECT embedder FROM memories LIMIT 0").is_err() {
            conn.execute_batch("ALTER TABLE memories ADD COLUMN embedder TEXT NOT NULL DEFAULT 'hash-v1';")?;
        }
        // The activity log can only grow.
        conn.execute_batch(
            "CREATE TRIGGER IF NOT EXISTS audit_no_update BEFORE UPDATE ON audit BEGIN SELECT RAISE(ABORT, 'the activity log is append-only'); END;
             CREATE TRIGGER IF NOT EXISTS audit_no_delete BEFORE DELETE ON audit BEGIN SELECT RAISE(ABORT, 'the activity log is append-only'); END;",
        )?;
        if conn.prepare("SELECT quiet FROM schedules LIMIT 0").is_err() {
            conn.execute_batch("ALTER TABLE schedules ADD COLUMN quiet INTEGER NOT NULL DEFAULT 0; ALTER TABLE schedules ADD COLUMN last_tokens INTEGER NOT NULL DEFAULT 0;")?;
        }
        if conn.prepare("SELECT workflow FROM schedules LIMIT 0").is_err() {
            conn.execute_batch("ALTER TABLE schedules ADD COLUMN workflow TEXT NOT NULL DEFAULT '';")?;
        }
        if conn.prepare("SELECT owner FROM tasks LIMIT 0").is_err() {
            conn.execute_batch("ALTER TABLE tasks ADD COLUMN owner TEXT NOT NULL DEFAULT 'user';")?;
        }
        if conn.prepare("SELECT confirmed FROM memories LIMIT 0").is_err() {
            conn.execute_batch("ALTER TABLE memories ADD COLUMN confirmed INTEGER NOT NULL DEFAULT 1; ALTER TABLE memories ADD COLUMN invalid_at INTEGER; ALTER TABLE memories ADD COLUMN superseded_by INTEGER;")?;
        }
        if conn.prepare("SELECT allow FROM schedules LIMIT 0").is_err() {
            conn.execute_batch("ALTER TABLE schedules ADD COLUMN allow TEXT NOT NULL DEFAULT ''")?;
        }
        Ok(Store { conn: Arc::new(Mutex::new(conn)) })
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
        tx.execute("DELETE FROM job_runs WHERE id NOT IN (SELECT id FROM job_runs ORDER BY id DESC LIMIT 500)", [])?;
        tx.commit()?;
        Ok(Some(id))
    }

    pub fn finish_job_run(&self, run_id: i64, status: &str, note: &str) -> Result<()> {
        self.db().execute("UPDATE job_runs SET status=?, note=?, finished_at=? WHERE id=? AND status='running'", params![status, note, now_ts(), run_id])?;
        Ok(())
    }

    /// Runs that were under way when Heron last stopped. Whether they finished is not known, so they are
    /// marked that way and returned (schedule id, job name) for the user to be told. They are not repeated.
    pub fn recover_interrupted_jobs(&self) -> Result<Vec<(i64, String)>> {
        let db = self.db();
        let found: Vec<(i64, String)> = {
            let mut st = db.prepare("SELECT schedule_id, name FROM job_runs WHERE status='running' ORDER BY id")?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
            rows
        };
        db.execute("UPDATE job_runs SET status='unknown', finished_at=?, note='Heron stopped while this was running' WHERE status='running'", [now_ts()])?;
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

    pub fn save_checkpoint(&self, session_id: &str, path: &str, prev: Option<&[u8]>) -> Result<i64> {
        let db = self.db();
        db.execute("INSERT INTO checkpoints(ts, session_id, path, existed, prev) VALUES (?,?,?,?,?)", params![now_ts(), session_id, path, prev.is_some() as i64, prev])?;
        Ok(db.last_insert_rowid())
    }

    /// Record what the file looked like after Heron's change (a hash), so undo can tell later whether
    /// anyone has touched it since.
    pub fn set_checkpoint_after(&self, id: i64, after_hash: &str) -> Result<()> {
        self.db().execute("UPDATE checkpoints SET after_hash=? WHERE id=?", params![after_hash, id])?;
        Ok(())
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

        let first = s.save_checkpoint("s", "/a", Some(b"old")).unwrap();
        s.set_checkpoint_after(first, "abc").unwrap();
        assert_eq!(s.last_checkpoint("s").unwrap(), Some((first, "/a".into(), Some(b"old".to_vec()), Some("abc".into()))));
        s.save_checkpoint("s", "/b", None).unwrap();
        assert_eq!(s.pop_checkpoint("s").unwrap(), Some(("/b".into(), None)));
        assert_eq!(s.pop_checkpoint("s").unwrap(), Some(("/a".into(), Some(b"old".to_vec()))));
        assert_eq!(s.pop_checkpoint("s").unwrap(), None);
    }
}
