-- The database schema exactly as Heron 0.1.0 created it (taken from src/store.rs at the v0.1.0 tag:
-- the SCHEMA text, then the two triggers its start-up code added). Used by the upgrade tests to build
-- a database "from the previous release". Do not edit: it records what shipped.
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
CREATE TRIGGER IF NOT EXISTS audit_no_update BEFORE UPDATE ON audit BEGIN SELECT RAISE(ABORT, 'the activity log is append-only'); END;
CREATE TRIGGER IF NOT EXISTS audit_no_delete BEFORE DELETE ON audit BEGIN SELECT RAISE(ABORT, 'the activity log is append-only'); END;
