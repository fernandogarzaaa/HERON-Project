use super::{Intent, Interrupted, Kind, PendingRow};
use anyhow::{bail, Result};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Transaction};

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

/// How long an `intent` row whose child process group is still alive waits before it is marked
/// interrupted anyway.
pub const CHILD_GRACE_SECS: i64 = 60 * 60;

pub fn insert_intent(conn: &Connection, intent: &Intent, proc: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO effects(started_at, proc, session_id, run_id, origin, workspace, kind, name, label, args, args_hash, summary, risk, tainted) \
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        params![
            crate::util::now_ts(),
            proc,
            intent.session_id,
            intent.run_id,
            intent.origin,
            intent.workspace.to_string_lossy(),
            intent.kind.as_str(),
            intent.name,
            intent.label,
            intent.args,
            intent.args_hash,
            intent.summary,
            intent.risk,
            intent.tainted as i64,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// End an open row: set its final state, outcome and `finished_at`, and clear its arguments.
pub fn write_outcome(conn: &Connection, id: i64, state: &str, outcome: Option<&str>) -> Result<bool> {
    Ok(conn.execute(
        "UPDATE effects SET state=?, outcome=?, finished_at=?, args=NULL WHERE id=? AND state='intent'",
        params![state, outcome, crate::util::now_ts(), id],
    )? == 1)
}

pub fn set_child(conn: &Connection, id: i64, pgid: i64) -> Result<()> {
    conn.execute("UPDATE effects SET child_pgid=? WHERE id=?", params![pgid, id])?;
    Ok(())
}

/// The processes that have `intent` rows.
pub fn open_procs(conn: &Connection) -> Result<Vec<String>> {
    let mut st = conn.prepare("SELECT DISTINCT proc FROM effects WHERE state='intent'")?;
    let v = st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    Ok(v)
}

/// Move the `intent` rows of the exited process `proc` to `interrupted`, except a row whose child
/// process group is still alive and younger than [`CHILD_GRACE_SECS`].
pub fn mark_interrupted(conn: &Connection, proc: &str, is_group_alive: impl Fn(i64) -> bool, now: i64) -> Result<usize> {
    let rows: Vec<(i64, Option<i64>, i64)> = {
        let mut st = conn.prepare("SELECT id, child_pgid, started_at FROM effects WHERE proc=? AND state='intent'")?;
        let v = st.query_map([proc], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
        v
    };
    let mut marked = 0;
    for (id, pgid, started_at) in rows {
        if let Some(pgid) = pgid {
            if now - started_at < CHILD_GRACE_SECS && is_group_alive(pgid) {
                continue;
            }
        }
        marked += conn.execute("UPDATE effects SET state='interrupted' WHERE id=? AND state='intent'", [id])?;
    }
    Ok(marked)
}

const INTERRUPTED_COLS: &str = "id, session_id, run_id, origin, workspace, kind, name, label, args, args_hash, summary, risk, tainted";

fn interrupted_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Interrupted> {
    let kind: String = r.get(5)?;
    Ok(Interrupted {
        id: r.get(0)?,
        session_id: r.get(1)?,
        run_id: r.get(2)?,
        origin: r.get(3)?,
        workspace: r.get::<_, String>(4)?.into(),
        kind: Kind::parse(&kind),
        name: r.get(6)?,
        label: r.get(7)?,
        args: r.get(8)?,
        args_hash: r.get(9)?,
        summary: r.get(10)?,
        risk: r.get(11)?,
        tainted: r.get::<_, i64>(12)? != 0,
    })
}

/// The `interrupted` rows that have no approval yet, oldest first.
pub fn unasked(conn: &Connection) -> Result<Vec<Interrupted>> {
    let mut st = conn.prepare(&format!("SELECT {INTERRUPTED_COLS} FROM effects WHERE state='interrupted' AND approval_id IS NULL ORDER BY id"))?;
    let v = st.query_map([], interrupted_row)?.collect::<rusqlite::Result<_>>()?;
    Ok(v)
}

/// The whole ask for one interrupted row: reopen its parked approval, or join a waiting approval
/// for the same call, or insert a new one; then set the row `asked`. Returns the approval id. The
/// caller commits `tx`; on an error it is dropped, so neither the approval nor the row changes.
pub fn ask_again(tx: &Transaction<'_>, row: &Interrupted) -> Result<i64> {
    let ask = super::ask_text(row);
    let reopened = match super::parked_approval(&row.run_id) {
        Some(id) => {
            let n = tx.execute("UPDATE approvals SET status='new', summary=?, detail=? WHERE id=? AND status IN ('running','unknown')", params![ask.summary, ask.detail, id])?;
            (n == 1).then_some(id)
        }
        None => None,
    };
    let approval_id = match reopened {
        Some(id) => id,
        None => {
            let target: Option<i64> = tx
                .query_row(
                    "SELECT e.approval_id FROM effects e JOIN approvals a ON a.id=e.approval_id \
                     WHERE e.state='asked' AND e.session_id=? AND e.name=? AND e.args_hash=? AND e.tainted=? AND e.workspace=? AND a.status='new' \
                     ORDER BY e.approval_id LIMIT 1",
                    params![row.session_id, row.name, row.args_hash, row.tainted as i64, row.workspace.to_string_lossy()],
                    |r| r.get(0),
                )
                .optional()?;
            match target {
                Some(id) => id,
                None => {
                    tx.execute(
                        "INSERT INTO approvals(ts, session_id, tool, args, summary, detail, risk, tainted, origin) VALUES (?,?,?,?,?,?,?,?,?)",
                        params![crate::util::now_ts(), row.session_id, ask.tool, ask.args, ask.summary, ask.detail, ask.risk, row.tainted as i64, ask.origin],
                    )?;
                    tx.last_insert_rowid()
                }
            }
        }
    };
    let n = tx.execute("UPDATE effects SET state='asked', approval_id=?, args=NULL WHERE id=? AND state='interrupted' AND approval_id IS NULL", params![approval_id, row.id])?;
    if n != 1 {
        bail!("effects row {} is no longer waiting to be asked about", row.id);
    }
    Ok(approval_id)
}

/// The rows the gate on an identical call looks at: open, interrupted or asked rows for this
/// session, tool and arguments.
pub fn pending(conn: &Connection, session_id: &str, name: &str, args_hash: &str) -> Result<Vec<PendingRow>> {
    let mut st = conn.prepare(
        "SELECT e.id, e.state, e.proc, e.approval_id, a.status FROM effects e LEFT JOIN approvals a ON a.id=e.approval_id \
         WHERE e.session_id=? AND e.name=? AND e.args_hash=? AND e.state IN ('intent','interrupted','asked') ORDER BY e.id",
    )?;
    let v = st
        .query_map(params![session_id, name, args_hash], |r| {
            Ok(PendingRow { id: r.get(0)?, state: r.get(1)?, proc: r.get(2)?, approval_id: r.get(3)?, approval_status: r.get(4)? })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(v)
}

/// Close open or interrupted rows as `cancelled`, so recovery never asks about them.
pub fn cancel_rows(conn: &Connection, ids: &[i64], outcome: &str) -> Result<usize> {
    if ids.is_empty() {
        return Ok(0);
    }
    let holes = vec!["?"; ids.len()].join(",");
    let sql = format!("UPDATE effects SET state='cancelled', outcome=?, finished_at=?, args=NULL WHERE state IN ('intent','interrupted') AND id IN ({holes})");
    let mut values: Vec<rusqlite::types::Value> = vec![outcome.to_string().into(), crate::util::now_ts().into()];
    values.extend(ids.iter().map(|&id| rusqlite::types::Value::from(id)));
    Ok(conn.execute(&sql, params_from_iter(values))?)
}

/// The approval id and `proc` of each `intent` row whose run is `parked:<id>`.
pub fn parked_intents(conn: &Connection) -> Result<Vec<(i64, String)>> {
    let mut st = conn.prepare("SELECT run_id, proc FROM effects WHERE state='intent' AND run_id LIKE 'parked:%' ORDER BY id")?;
    let rows: Vec<(String, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    Ok(rows.into_iter().filter_map(|(run, proc)| super::parked_approval(&run).map(|id| (id, proc))).collect())
}

/// The `workspace` of the approval's `asked` rows.
pub fn workspace_of(conn: &Connection, approval_id: i64) -> Result<Option<String>> {
    Ok(conn.query_row("SELECT workspace FROM effects WHERE approval_id=? AND state='asked' ORDER BY id LIMIT 1", [approval_id], |r| r.get(0)).optional()?)
}

/// Approvals that already wait as `new` and that asked rows point at.
pub fn waiting_approvals(conn: &Connection) -> Result<Vec<i64>> {
    let mut st = conn.prepare("SELECT DISTINCT e.approval_id FROM effects e JOIN approvals a ON a.id=e.approval_id WHERE e.state='asked' AND a.status='new'")?;
    let v = st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    Ok(v)
}

/// Delete rows that ended before `older_than` (unix seconds): `done`, `failed` and `cancelled`
/// rows, and `asked` rows whose approval no longer waits or runs. Open rows are kept.
pub fn prune(conn: &Connection, older_than: i64) -> Result<usize> {
    Ok(conn.execute(
        "DELETE FROM effects WHERE COALESCE(finished_at, started_at) < ? AND (\
           state IN ('done','failed','cancelled') \
           OR (state='asked' AND NOT EXISTS (SELECT 1 FROM approvals a WHERE a.id=effects.approval_id AND a.status IN ('new','running'))))",
        [older_than],
    )?)
}
