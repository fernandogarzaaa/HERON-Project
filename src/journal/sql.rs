use anyhow::Result;
use rusqlite::Connection;

const EFFECTS: &str = r#"
CREATE TABLE IF NOT EXISTS effects(
  id INTEGER PRIMARY KEY, started_at INTEGER NOT NULL, finished_at INTEGER,
  proc TEXT NOT NULL, session_id TEXT NOT NULL, run_id TEXT NOT NULL, origin TEXT NOT NULL DEFAULT '',
  workspace TEXT NOT NULL, kind TEXT NOT NULL, name TEXT NOT NULL, label TEXT,
  args TEXT, args_hash TEXT NOT NULL, summary TEXT NOT NULL, risk TEXT NOT NULL,
  tainted INTEGER NOT NULL DEFAULT 0, child_pgid INTEGER,
  state TEXT NOT NULL DEFAULT 'intent', outcome TEXT, approval_id INTEGER);
CREATE INDEX IF NOT EXISTS idx_effects_state ON effects(state);
CREATE INDEX IF NOT EXISTS idx_effects_call ON effects(session_id, name, args_hash);
CREATE INDEX IF NOT EXISTS idx_effects_approval ON effects(approval_id);
"#;

/// Data format 4: the effect journal's table.
pub fn step(conn: &Connection) -> Result<()> {
    conn.execute_batch(EFFECTS)?;
    Ok(())
}
