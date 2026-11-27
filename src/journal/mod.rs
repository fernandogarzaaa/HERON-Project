//! The effect journal (U1): a durable record of every mutating call, written before the call starts
//! and closed when it ends, so a call cut short by a crash or a quit is offered to the user again
//! ("… was interrupted. Run again?") instead of being silently repeated or forgotten.

pub mod sql;
mod lease;

pub use lease::{proc_id, sweep, Lease};

use crate::store::Store;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use rusqlite::TransactionBehavior;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

/// The id of an effects row.
pub type EntryId = i64;

/// The run id a call records when no run frame is set.
pub const NO_RUN: &str = "none";
/// The tool-name prefix of an approval that resumes an action.
pub const RESUME_PREFIX: &str = "resume:";
/// How long finished rows are kept.
pub const RETAIN_SECS: i64 = 30 * 24 * 60 * 60;
/// The outcome of a row whose tool panicked.
pub const PANIC_TEXT: &str = "the tool panicked";
/// The outcome of a row whose run the user stopped.
pub const STOPPED_TEXT: &str = "stopped by the user";
/// The result of a waiting ask-again that an identical call in the conversation replaced.
pub const SUPERSEDED_RESULT: &str = "Run from the conversation instead.";
/// The outcome of an unasked row that an identical call in the conversation replaced.
pub const RERUN_OUTCOME: &str = "run again from the conversation";

const OUTCOME_CHARS: usize = 300;
const ACTION_RISK: &str = "exec";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Tool,
    Action,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Tool => "tool",
            Kind::Action => "action",
        }
    }

    pub(crate) fn parse(s: &str) -> Kind {
        if s == "action" {
            Kind::Action
        } else {
            Kind::Tool
        }
    }
}

/// What is recorded before a mutating call starts.
#[derive(Debug, Clone)]
pub struct Intent {
    pub kind: Kind,
    /// The tool name, or the action's handler name.
    pub name: String,
    /// The full arguments JSON (tool), or the subject (action).
    pub args: String,
    pub args_hash: String,
    pub summary: String,
    pub risk: String,
    pub tainted: bool,
    pub session_id: String,
    pub run_id: String,
    pub origin: String,
    pub workspace: PathBuf,
    /// For an action, the text shown to the user, such as `Repair #12`.
    pub label: Option<String>,
}

/// An action another module records with [`begin_action`], resumed through its [`Resumer`].
#[derive(Debug, Clone)]
pub struct ActionIntent {
    pub handler: &'static str,
    pub subject: String,
    pub label: String,
    pub session_id: String,
    pub origin: String,
    pub tainted: bool,
    pub workspace: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Done,
    Failed(String),
}

#[derive(Debug)]
pub enum Guarded {
    /// The tool ran; `Err` for its error or the timeout.
    Ran(Result<String>),
    /// The journal couldn't record the intent, so the tool did not start.
    NotRun(String),
}

/// An earlier copy of a call that the gate on an identical call found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    /// An ask-again approval waits for the user.
    Waiting(i64),
    /// An ask-again approval is being run right now.
    Running(i64),
    /// Interrupted rows nobody has asked about yet.
    Unasked(Vec<i64>),
}

/// A row read for asking again.
#[derive(Debug, Clone)]
pub struct Interrupted {
    pub id: i64,
    pub session_id: String,
    pub run_id: String,
    pub origin: String,
    pub workspace: PathBuf,
    pub kind: Kind,
    pub name: String,
    pub label: Option<String>,
    pub args: Option<String>,
    pub args_hash: String,
    pub summary: String,
    pub risk: String,
    pub tainted: bool,
}

/// A row the gate on an identical call looks at.
#[derive(Debug, Clone)]
pub struct PendingRow {
    pub id: i64,
    pub state: String,
    pub proc: String,
    pub approval_id: Option<i64>,
    pub approval_status: Option<String>,
}

/// The approval an interrupted row becomes.
pub(crate) struct AskText {
    pub tool: String,
    pub args: String,
    pub summary: String,
    pub detail: String,
    pub risk: String,
    pub origin: String,
}

pub(crate) fn ask_text(row: &Interrupted) -> AskText {
    let origin = format!("interrupted: {}", row.origin);
    match row.kind {
        Kind::Tool => AskText {
            tool: row.name.clone(),
            args: row.args.clone().unwrap_or_else(|| "{}".into()),
            summary: format!("{} was interrupted. Run again?", row.summary),
            detail: format!("Heron stopped while doing this, in {}. It may or may not have finished. Check before running it again.", row.workspace.display()),
            risk: row.risk.clone(),
            origin,
        },
        Kind::Action => AskText {
            tool: resume_tool(&row.name),
            args: serde_json::json!({ "subject": row.args.clone().unwrap_or_default() }).to_string(),
            summary: format!("{} was interrupted. Run again?", row.label.as_deref().unwrap_or(&row.name)),
            detail: "Heron stopped while doing this. It may or may not have finished. Check before running it again.".into(),
            risk: ACTION_RISK.into(),
            origin,
        },
    }
}

/// The run id of an approval carried out later.
pub fn parked_run_id(approval_id: i64) -> String {
    format!("parked:{approval_id}")
}

pub(crate) fn parked_approval(run_id: &str) -> Option<i64> {
    run_id.strip_prefix("parked:")?.parse().ok()
}

/// The approval tool name that resumes `handler`.
pub fn resume_tool(handler: &str) -> String {
    format!("{RESUME_PREFIX}{handler}")
}

/// The handler of a `resume:<handler>` approval tool name.
pub fn resume_handler(tool: &str) -> Option<&str> {
    tool.strip_prefix(RESUME_PREFIX)
}

fn canonical(v: &serde_json::Value, out: &mut String) {
    match v {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::Value::String(k.clone()).to_string());
                out.push(':');
                canonical(&map[k], out);
            }
            out.push('}');
        }
        serde_json::Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canonical(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// SHA-256 of the canonical (key-sorted, compact) JSON of `args`.
pub fn args_hash(args: &serde_json::Value) -> String {
    let mut s = String::new();
    canonical(args, &mut s);
    crate::util::sha256_hex(s.as_bytes())
}

fn outcome_preview(text: &str) -> String {
    crate::util::preview(&crate::util::redact_secrets(text), OUTCOME_CHARS)
}

// ---------- the Stop flag ----------

fn cancels() -> MutexGuard<'static, HashSet<String>> {
    static SET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SET.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

/// Mark `run_id` as stopped by the user, before its task is aborted.
pub fn request_cancel(run_id: &str) {
    cancels().insert(run_id.to_string());
}

pub fn cancel_requested(run_id: &str) -> bool {
    cancels().contains(run_id)
}

pub fn clear_cancel(run_id: &str) {
    cancels().remove(run_id);
}

// ---------- guarded ----------

#[derive(Clone)]
struct Current {
    store: Store,
    id: i64,
}

tokio::task_local! {
    static CURRENT: Current;
}

fn open_row(store: &Store, heron_home: &Path, intent: &Intent) -> Result<i64> {
    Lease::acquire(heron_home)?;
    store.with_conn(|c| sql::insert_intent(c, intent, proc_id()))
}

struct RowGuard<'a> {
    store: &'a Store,
    id: i64,
    run_id: String,
    armed: bool,
}

impl Drop for RowGuard<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let id = self.id;
        let written = if std::thread::panicking() {
            self.store.with_conn(|c| sql::write_outcome(c, id, "failed", Some(PANIC_TEXT)))
        } else if cancel_requested(&self.run_id) {
            self.store.with_conn(|c| sql::write_outcome(c, id, "cancelled", Some(STOPPED_TEXT)))
        } else {
            self.store.with_conn(|c| Ok(c.execute("UPDATE effects SET state='interrupted' WHERE id=? AND state='intent'", [id])? == 1))
        };
        if let Err(e) = written {
            eprintln!("heron: the journal could not close effects row {id}: {e:#}");
        }
    }
}

/// Run a mutating call under the journal: record the intent, run `fut` with the time limit `limit`,
/// record the outcome. When the intent cannot be recorded, `fut` is never started.
pub async fn guarded<F>(store: &Store, heron_home: &Path, intent: Intent, limit: Duration, fut: F) -> Guarded
where
    F: Future<Output = Result<String>>,
{
    let id = match open_row(store, heron_home, &intent) {
        Ok(id) => id,
        Err(e) => return Guarded::NotRun(format!("Heron couldn't record this action before running it, so it was not run: {e:#}")),
    };
    let mut guard = RowGuard { store, id, run_id: intent.run_id, armed: true };
    let out = CURRENT.scope(Current { store: store.clone(), id }, tokio::time::timeout(limit, fut)).await;
    guard.armed = false;
    let ran = out.unwrap_or_else(|_| Err(anyhow!("tool timed out after {}s", limit.as_secs())));
    let (state, outcome) = match &ran {
        Ok(text) => ("done", outcome_preview(text)),
        Err(e) => ("failed", outcome_preview(&format!("{e:#}"))),
    };
    if let Err(e) = store.with_conn(|c| sql::write_outcome(c, id, state, Some(&outcome))) {
        eprintln!("heron: the journal could not record the outcome of effects row {id}: {e:#}");
    }
    Guarded::Ran(ran)
}

/// Record the process group of the shell command the current guarded call started. Outside
/// [`guarded`] it does nothing.
pub fn note_child(pgid: i32) {
    let _ = CURRENT.try_with(|c| {
        if let Err(e) = c.store.with_conn(|conn| sql::set_child(conn, c.id, pgid as i64)) {
            eprintln!("heron: the journal could not record a process group for effects row {}: {e:#}", c.id);
        }
    });
}

// ---------- actions ----------

/// Record an action another module is about to carry out. Close it with [`finish`].
pub fn begin_action(store: &Store, heron_home: &Path, action: ActionIntent) -> Result<EntryId> {
    let intent = Intent {
        kind: Kind::Action,
        name: action.handler.to_string(),
        args_hash: args_hash(&serde_json::json!({ "subject": action.subject })),
        args: action.subject,
        summary: action.label.clone(),
        risk: ACTION_RISK.into(),
        tainted: action.tainted,
        session_id: action.session_id,
        run_id: NO_RUN.into(),
        origin: action.origin,
        workspace: action.workspace,
        label: Some(action.label),
    };
    open_row(store, heron_home, &intent)
}

pub fn finish(store: &Store, id: EntryId, outcome: Outcome) -> Result<()> {
    let (state, text) = match outcome {
        Outcome::Done => ("done", None),
        Outcome::Failed(e) => ("failed", Some(outcome_preview(&e))),
    };
    store.with_conn(|c| sql::write_outcome(c, id, state, text.as_deref()))?;
    Ok(())
}

/// Carries out an interrupted action again, for its handler.
#[async_trait]
pub trait Resumer: Send + Sync {
    /// **Run again**.
    async fn rerun(&self, app: &crate::app::App, subject: &str, tainted: bool) -> Result<String>;
    /// **Dismiss**.
    fn declined(&self, app: &crate::app::App, subject: &str);
}

fn resumers() -> MutexGuard<'static, HashMap<&'static str, Arc<dyn Resumer>>> {
    static MAP: OnceLock<Mutex<HashMap<&'static str, Arc<dyn Resumer>>>> = OnceLock::new();
    MAP.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

pub fn register_resumer(handler: &'static str, resumer: Arc<dyn Resumer>) {
    resumers().insert(handler, resumer);
}

pub fn resumer(handler: &str) -> Option<Arc<dyn Resumer>> {
    resumers().get(handler).cloned()
}

// ---------- recovery ----------

#[cfg(unix)]
fn group_alive(pgid: i64) -> bool {
    let Ok(pgid) = libc::pid_t::try_from(pgid) else { return false };
    if pgid <= 1 {
        return false;
    }
    // SAFETY: signal 0 only checks that the group exists and may be signalled.
    let r = unsafe { libc::kill(-pgid, 0) };
    r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn group_alive(_pgid: i64) -> bool {
    false
}

/// Pass 1: move the `intent` rows of exited processes to `interrupted`.
fn mark_dead(store: &Store, heron_home: &Path) -> Result<usize> {
    let now = crate::util::now_ts();
    let mut marked = 0;
    for proc in store.with_conn(|c| sql::open_procs(c))? {
        if proc == proc_id() || !Lease::is_dead(heron_home, &proc) {
            continue;
        }
        marked += store.with_conn(|c| {
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let n = sql::mark_interrupted(&tx, &proc, group_alive, now)?;
            tx.commit()?;
            Ok(n)
        })?;
    }
    Ok(marked)
}

/// Pass 2: an approval for each `interrupted` row with none. Returns how many approvals were
/// created or reopened.
fn ask_pass(store: &Store) -> Result<usize> {
    let waiting: HashSet<i64> = store.with_conn(|c| sql::waiting_approvals(c))?.into_iter().collect();
    let mut shown = HashSet::new();
    for row in store.with_conn(|c| sql::unasked(c))? {
        let asked = store.with_conn(|c| {
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let id = sql::ask_again(&tx, &row)?;
            tx.commit()?;
            Ok(id)
        });
        match asked {
            Ok(id) if !waiting.contains(&id) => {
                shown.insert(id);
            }
            Ok(_) => {}
            Err(e) => eprintln!("heron: the journal could not ask again about effects row {}: {e:#}", row.id),
        }
    }
    Ok(shown.len())
}

/// Both passes of recovery, then the sweep of dead lease files. Only the background owner calls
/// it. Returns the number of approvals created or reopened.
pub fn recover(store: &Store, heron_home: &Path) -> Result<usize> {
    mark_dead(store, heron_home)?;
    let shown = ask_pass(store)?;
    sweep(heron_home, &[]);
    Ok(shown)
}

/// The approvals whose `parked:<id>` run has an `intent` row held by a live lease.
pub fn live_parked_approvals(store: &Store, heron_home: &Path) -> Vec<i64> {
    let rows = store.with_conn(|c| sql::parked_intents(c)).unwrap_or_default();
    let mut ids: Vec<i64> = rows.into_iter().filter(|(_, proc)| proc == proc_id() || !Lease::is_dead(heron_home, proc)).map(|(id, _)| id).collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

// ---------- the gate on an identical call ----------

/// An earlier copy of this call in this session that is interrupted, waiting to be asked about,
/// or asked about and not yet answered.
pub fn pending_ask_again(store: &Store, heron_home: &Path, session_id: &str, name: &str, args_hash: &str) -> Option<Gate> {
    let rows = store.with_conn(|c| sql::pending(c, session_id, name, args_hash)).ok()?;
    let (mut waiting, mut running, mut unasked) = (None, None, Vec::new());
    for row in rows {
        match row.state.as_str() {
            "asked" => match row.approval_status.as_deref() {
                Some("running") => running = running.or(row.approval_id),
                Some("new") => waiting = waiting.or(row.approval_id),
                _ => {}
            },
            "interrupted" if row.approval_id.is_none() => unasked.push(row.id),
            "intent" if row.proc != proc_id() && Lease::is_dead(heron_home, &row.proc) => unasked.push(row.id),
            _ => {}
        }
    }
    running.map(Gate::Running).or(waiting.map(Gate::Waiting)).or((!unasked.is_empty()).then_some(Gate::Unasked(unasked)))
}

/// Retire the earlier copy once the user approved the identical call in the conversation. `false`
/// when it could not be retired, because it is being run again from Approvals.
pub fn supersede(store: &Store, gate: &Gate) -> Result<bool> {
    match gate {
        Gate::Waiting(id) => {
            if !store.dismiss_approval(*id)? {
                return Ok(false);
            }
            store.resolve_approval(*id, "dismissed", Some(SUPERSEDED_RESULT))?;
            Ok(true)
        }
        Gate::Running(_) => Ok(false),
        Gate::Unasked(ids) => {
            store.with_conn(|c| sql::cancel_rows(c, ids, RERUN_OUTCOME))?;
            Ok(true)
        }
    }
}

/// The working folder of the approval's `asked` rows.
pub fn workspace_for(store: &Store, approval_id: i64) -> Option<PathBuf> {
    store.with_conn(|c| sql::workspace_of(c, approval_id)).ok().flatten().map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::lease::tests::{hold_lease, stale_lease};
    use super::*;
    use futures_util::FutureExt;
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct Row {
        state: String,
        args: Option<String>,
        args_hash: String,
        outcome: Option<String>,
        finished_at: Option<i64>,
        approval_id: Option<i64>,
        child_pgid: Option<i64>,
    }

    fn row(store: &Store, id: i64) -> Row {
        store
            .with_conn(|c| {
                Ok(c.query_row("SELECT state, args, args_hash, outcome, finished_at, approval_id, child_pgid FROM effects WHERE id=?", [id], |r| {
                    Ok(Row { state: r.get(0)?, args: r.get(1)?, args_hash: r.get(2)?, outcome: r.get(3)?, finished_at: r.get(4)?, approval_id: r.get(5)?, child_pgid: r.get(6)? })
                })?)
            })
            .unwrap()
    }

    fn row_count(store: &Store) -> i64 {
        store.with_conn(|c| Ok(c.query_row("SELECT COUNT(*) FROM effects", [], |r| r.get(0))?)).unwrap()
    }

    fn approval_count(store: &Store) -> i64 {
        store.with_conn(|c| Ok(c.query_row("SELECT COUNT(*) FROM approvals", [], |r| r.get(0))?)).unwrap()
    }

    fn sql_exec(store: &Store, sql: &str, p: impl rusqlite::Params) {
        store.with_conn(|c| Ok(c.execute(sql, p)?)).unwrap();
    }

    fn call(session: &str, name: &str, args: serde_json::Value, run_id: &str) -> Intent {
        Intent {
            kind: Kind::Tool,
            name: name.into(),
            args_hash: args_hash(&args),
            args: args.to_string(),
            summary: format!("{name} summary"),
            risk: "write".into(),
            tainted: false,
            session_id: session.into(),
            run_id: run_id.into(),
            origin: "chat".into(),
            workspace: PathBuf::from("/work/repo"),
            label: None,
        }
    }

    fn insert(store: &Store, intent: &Intent, proc: &str) -> i64 {
        store.with_conn(|c| sql::insert_intent(c, intent, proc)).unwrap()
    }

    fn interrupted(store: &Store, intent: &Intent) -> i64 {
        let id = insert(store, intent, "crashed-proc");
        sql_exec(store, "UPDATE effects SET state='interrupted' WHERE id=?", [id]);
        id
    }

    fn asked(store: &Store, intent: &Intent) -> (i64, i64) {
        let id = interrupted(store, intent);
        let approval = store.with_conn(|c| {
            let row = sql::unasked(c)?.into_iter().find(|r| r.id == id).unwrap();
            let tx = c.transaction()?;
            let a = sql::ask_again(&tx, &row)?;
            tx.commit()?;
            Ok(a)
        });
        (id, approval.unwrap())
    }

    fn setup() -> (Store, tempfile::TempDir) {
        (Store::in_memory().unwrap(), tempfile::tempdir().unwrap())
    }

    #[test]
    fn dead_process_intent_becomes_interrupted() {
        let (store, home) = setup();
        let gone = insert(&store, &call("s", "write_file", json!({"path": "a"}), "turn:1"), "no-lease-file");
        stale_lease(home.path(), "crashed");
        let crashed = insert(&store, &call("s", "write_file", json!({"path": "b"}), "turn:1"), "crashed");
        assert_eq!(mark_dead(&store, home.path()).unwrap(), 2);
        for id in [gone, crashed] {
            let r = row(&store, id);
            assert_eq!(r.state, "interrupted");
            assert!(r.args.is_some(), "a re-run needs the arguments");
            assert_eq!(r.approval_id, None);
        }
        assert_eq!(mark_dead(&store, home.path()).unwrap(), 0);
        assert_eq!(approval_count(&store), 0);
    }

    #[test]
    fn a_live_process_intent_is_left_alone() {
        let (store, home) = setup();
        let live = hold_lease(home.path(), "live-proc");
        let theirs = insert(&store, &call("s", "shell", json!({"command": "make"}), "turn:2"), "live-proc");
        let ours = insert(&store, &call("s", "shell", json!({"command": "ls"}), "turn:2"), proc_id());
        assert_eq!(recover(&store, home.path()).unwrap(), 0);
        assert_eq!(row(&store, theirs).state, "intent");
        assert_eq!(row(&store, ours).state, "intent");
        assert_eq!(approval_count(&store), 0);
        assert!(lease::lease_dir(home.path()).join("live-proc.lock").exists());
        drop(live);
        assert_eq!(recover(&store, home.path()).unwrap(), 1);
        assert_eq!(row(&store, theirs).state, "asked");
        assert_eq!(row(&store, ours).state, "intent");
    }

    #[tokio::test]
    async fn finished_rows_drop_their_arguments() {
        let (store, home) = setup();
        let intent = call("s", "write_file", json!({"path": "a", "content": "x"}), "turn:finish");
        let hash = intent.args_hash.clone();
        let Guarded::Ran(Ok(text)) = guarded(&store, home.path(), intent.clone(), Duration::from_secs(5), async { Ok("wrote a".to_string()) }).await else { panic!() };
        assert_eq!(text, "wrote a");
        let done = row(&store, 1);
        assert_eq!(done.state, "done");
        assert_eq!(done.args, None);
        assert_eq!(done.args_hash, hash);
        assert_eq!(done.outcome.as_deref(), Some("wrote a"));
        assert!(done.finished_at.is_some());

        let Guarded::Ran(Err(e)) = guarded(&store, home.path(), intent.clone(), Duration::from_secs(5), async { Err(anyhow!("disk full")) }).await else { panic!() };
        assert_eq!(e.to_string(), "disk full");
        let failed = row(&store, 2);
        assert_eq!(failed.state, "failed");
        assert_eq!(failed.args, None);
        assert_eq!(failed.outcome.as_deref(), Some("disk full"));

        let long = format!("token = abcdefghijklmnop {}", "y".repeat(1000));
        guarded(&store, home.path(), intent, Duration::from_secs(5), async move { Ok(long) }).await;
        let previewed = row(&store, 3).outcome.unwrap();
        assert_eq!(previewed.chars().count(), OUTCOME_CHARS + 1);
        assert!(previewed.starts_with("token = [REDACTED] yyy"), "{previewed}");
        Lease::release(home.path());
    }

    #[tokio::test]
    async fn drop_after_a_cancel_request_writes_cancelled() {
        let (store, home) = setup();
        let run = "turn:cancel-test";
        request_cancel(run);
        assert!(cancel_requested(run));
        let fut = guarded(&store, home.path(), call("s", "shell", json!({"command": "sleep 9"}), run), Duration::from_secs(60), std::future::pending());
        assert!(fut.now_or_never().is_none());
        let r = row(&store, 1);
        assert_eq!(r.state, "cancelled");
        assert_eq!(r.outcome.as_deref(), Some(STOPPED_TEXT));
        assert_eq!(r.args, None);
        clear_cancel(run);
        assert!(!cancel_requested(run));
        assert_eq!(recover(&store, home.path()).unwrap(), 0);
        assert_eq!(approval_count(&store), 0);
        Lease::release(home.path());
    }

    #[tokio::test]
    async fn drop_without_a_cancel_request_writes_interrupted() {
        let (store, home) = setup();
        request_cancel("turn:another-run");
        let fut = guarded(&store, home.path(), call("s", "shell", json!({"command": "sleep 9"}), "turn:quit"), Duration::from_secs(60), std::future::pending());
        assert!(fut.now_or_never().is_none());
        clear_cancel("turn:another-run");
        let r = row(&store, 1);
        assert_eq!(r.state, "interrupted");
        assert!(r.args.is_some());
        assert_eq!(recover(&store, home.path()).unwrap(), 1);
        assert_eq!(row(&store, 1).state, "asked");
        Lease::release(home.path());
    }

    #[tokio::test]
    async fn panic_inside_guarded_writes_failed() {
        let (store, home) = setup();
        let (s, h) = (store.clone(), home.path().to_path_buf());
        let task = tokio::spawn(async move {
            guarded(&s, &h, call("s", "shell", json!({"command": "x"}), "turn:panic"), Duration::from_secs(5), async {
                if std::hint::black_box(true) {
                    panic!("boom");
                }
                Ok(String::new())
            })
            .await
        });
        assert!(task.await.unwrap_err().is_panic());
        let r = row(&store, 1);
        assert_eq!(r.state, "failed");
        assert_eq!(r.outcome.as_deref(), Some(PANIC_TEXT));
        assert_eq!(r.args, None);
        Lease::release(home.path());
    }

    #[tokio::test]
    async fn timeout_inside_guarded_writes_failed() {
        let (store, home) = setup();
        let out = guarded(&store, home.path(), call("s", "shell", json!({"command": "sleep 30"}), "turn:slow"), Duration::from_secs(1), async {
            tokio::time::sleep(Duration::from_secs(30)).await;
            Ok("finished".to_string())
        })
        .await;
        let Guarded::Ran(Err(e)) = out else { panic!("{out:?}") };
        assert_eq!(e.to_string(), "tool timed out after 1s");
        let r = row(&store, 1);
        assert_eq!(r.state, "failed");
        assert_eq!(r.outcome.as_deref(), Some("tool timed out after 1s"));
        Lease::release(home.path());
    }

    #[tokio::test]
    async fn failed_intent_insert_does_not_run_the_tool() {
        let (store, home) = setup();
        store.execute_batch_for_test("DROP TABLE effects;");
        let ran = Arc::new(AtomicBool::new(false));
        let flag = ran.clone();
        let out = guarded(&store, home.path(), call("s", "shell", json!({"command": "rm x"}), "turn:x"), Duration::from_secs(5), async move {
            flag.store(true, Ordering::SeqCst);
            Ok("ran".to_string())
        })
        .await;
        let Guarded::NotRun(text) = out else { panic!("{out:?}") };
        assert!(text.starts_with("Heron couldn't record this action before running it, so it was not run: "), "{text}");
        assert!(text.contains("effects"), "{text}");
        assert!(!ran.load(Ordering::SeqCst));
        Lease::release(home.path());

        let store = Store::in_memory().unwrap();
        let not_a_folder = home.path().join("file");
        std::fs::write(&not_a_folder, b"").unwrap();
        let flag = ran.clone();
        let out = guarded(&store, &not_a_folder, call("s", "shell", json!({"command": "rm x"}), "turn:x"), Duration::from_secs(5), async move {
            flag.store(true, Ordering::SeqCst);
            Ok("ran".to_string())
        })
        .await;
        assert!(matches!(out, Guarded::NotRun(_)), "{out:?}");
        assert!(!ran.load(Ordering::SeqCst));
        assert_eq!(row_count(&store), 0);
    }

    #[test]
    fn interrupted_tool_call_becomes_one_ask_again_approval() {
        let (store, home) = setup();
        let mut intent = call("sess-1", "shell", json!({"command": "git commit -m wip"}), "turn:3");
        intent.summary = "git commit -m wip".into();
        intent.risk = "exec".into();
        let first = insert(&store, &intent, "crashed-a");
        assert_eq!(recover(&store, home.path()).unwrap(), 1);
        let r = row(&store, first);
        assert_eq!(r.state, "asked");
        assert_eq!(r.args, None);
        let id = r.approval_id.unwrap();
        let a = store.get_approval(id).unwrap();
        assert_eq!(a.session_id, "sess-1");
        assert_eq!(a.tool, "shell");
        assert_eq!(a.args, intent.args);
        assert_eq!(a.summary, "git commit -m wip was interrupted. Run again?");
        assert_eq!(a.detail, "Heron stopped while doing this, in /work/repo. It may or may not have finished. Check before running it again.");
        assert_eq!(a.risk, "exec");
        assert!(!a.tainted);
        assert_eq!(a.origin, "interrupted: chat");
        assert_eq!(a.status, "new");

        assert_eq!(recover(&store, home.path()).unwrap(), 0);
        assert_eq!(approval_count(&store), 1);

        let second = insert(&store, &intent, "crashed-b");
        assert_eq!(recover(&store, home.path()).unwrap(), 0);
        assert_eq!(approval_count(&store), 1);
        assert_eq!(row(&store, second).approval_id, Some(id));
        assert_eq!(workspace_for(&store, id), Some(PathBuf::from("/work/repo")));

        store.resolve_approval(id, "dismissed", None).unwrap();
        let third = insert(&store, &intent, "crashed-c");
        assert_eq!(recover(&store, home.path()).unwrap(), 1);
        assert_ne!(row(&store, third).approval_id, Some(id));
        assert_eq!(approval_count(&store), 2);
    }

    #[test]
    fn ask_pass_commits_the_approval_and_the_row_together() {
        let store = Store::in_memory().unwrap();
        let fresh = interrupted(&store, &call("s", "write_file", json!({"path": "a"}), "turn:4"));
        let parked = store.park_approval("s", "write_file", "{}", "Write b", "", "write", false, "chat").unwrap();
        assert!(store.claim_approval(parked).unwrap());
        let reopen = interrupted(&store, &call("s", "write_file", json!({"path": "b"}), &parked_run_id(parked)));
        let rows = store.with_conn(|c| sql::unasked(c)).unwrap();
        assert_eq!(rows.iter().map(|r| r.id).collect::<Vec<_>>(), vec![fresh, reopen]);

        store.with_conn(|c| sql::cancel_rows(c, &[fresh, reopen], "gone")).unwrap();
        for r in &rows {
            let failed = store.with_conn(|c| {
                let tx = c.transaction()?;
                let id = sql::ask_again(&tx, r)?;
                tx.commit()?;
                Ok(id)
            });
            assert!(failed.is_err());
        }
        assert_eq!(approval_count(&store), 1);
        assert_eq!(store.get_approval(parked).unwrap().status, "running");
        assert_eq!(store.get_approval(parked).unwrap().summary, "Write b");

        let other = interrupted(&store, &call("s", "write_file", json!({"path": "c"}), "turn:4"));
        let row_read = store.with_conn(|c| sql::unasked(c)).unwrap().pop().unwrap();
        store.with_conn(|c| {
            let tx = c.transaction()?;
            sql::ask_again(&tx, &row_read)?;
            Ok(())
        })
        .unwrap();
        assert_eq!(approval_count(&store), 1);
        assert_eq!(row(&store, other).state, "interrupted");

        let home = tempfile::tempdir().unwrap();
        assert_eq!(recover(&store, home.path()).unwrap(), 1);
        let r = row(&store, other);
        assert_eq!(r.state, "asked");
        assert!(store.get_approval(r.approval_id.unwrap()).is_some());
        assert_eq!(approval_count(&store), 2);
    }

    #[test]
    fn interrupted_calls_from_two_sessions_get_two_approvals() {
        let (store, home) = setup();
        let a = interrupted(&store, &call("sess-a", "shell", json!({"command": "make"}), "turn:5"));
        let b = interrupted(&store, &call("sess-b", "shell", json!({"command": "make"}), "turn:6"));
        assert_eq!(recover(&store, home.path()).unwrap(), 2);
        let (ra, rb) = (row(&store, a).approval_id.unwrap(), row(&store, b).approval_id.unwrap());
        assert_ne!(ra, rb);
        assert_eq!(store.get_approval(ra).unwrap().session_id, "sess-a");
        assert_eq!(store.get_approval(rb).unwrap().session_id, "sess-b");
    }

    #[test]
    fn interrupted_calls_with_different_taint_get_two_approvals() {
        let (store, home) = setup();
        let clean = call("s", "shell", json!({"command": "make"}), "turn:7");
        let mut dirty = clean.clone();
        dirty.tainted = true;
        let a = interrupted(&store, &clean);
        let b = interrupted(&store, &dirty);
        assert_eq!(recover(&store, home.path()).unwrap(), 2);
        let (ra, rb) = (row(&store, a).approval_id.unwrap(), row(&store, b).approval_id.unwrap());
        assert_ne!(ra, rb);
        assert!(!store.get_approval(ra).unwrap().tainted);
        assert!(store.get_approval(rb).unwrap().tainted);
    }

    #[test]
    fn interrupted_calls_in_two_workspaces_or_with_other_args_get_two_approvals() {
        let (store, home) = setup();
        let here = call("s", "shell", json!({"command": "make"}), "turn:8");
        let mut there = here.clone();
        there.workspace = PathBuf::from("/work/other");
        let a = interrupted(&store, &here);
        let b = interrupted(&store, &there);
        let c = interrupted(&store, &call("s", "shell", json!({"command": "make test"}), "turn:8"));
        let d = interrupted(&store, &call("s", "write_file", json!({"command": "make"}), "turn:8"));
        assert_eq!(recover(&store, home.path()).unwrap(), 4);
        let ids: HashSet<i64> = [a, b, c, d].iter().map(|&id| row(&store, id).approval_id.unwrap()).collect();
        assert_eq!(ids.len(), 4);
    }

    #[test]
    fn interrupted_parked_call_reopens_its_own_approval() {
        let (store, home) = setup();
        let running = store.park_approval("s", "write_file", r#"{"path":"a"}"#, "Write a", "old detail", "write", false, "job").unwrap();
        let lost = store.park_approval("s", "write_file", r#"{"path":"b"}"#, "Write b", "old detail", "write", false, "job").unwrap();
        assert!(store.claim_approval(running).unwrap());
        assert!(store.claim_approval(lost).unwrap());
        store.recover_interrupted_approvals(&[running]).unwrap();
        assert_eq!(store.get_approval(lost).unwrap().status, "unknown");
        let mut first = call("s", "write_file", json!({"path": "a"}), &parked_run_id(running));
        first.summary = "Write a".into();
        let mut second = call("s", "write_file", json!({"path": "b"}), &parked_run_id(lost));
        second.summary = "Write b".into();
        let r1 = insert(&store, &first, "crashed");
        let r2 = insert(&store, &second, "crashed");
        assert_eq!(recover(&store, home.path()).unwrap(), 2);
        assert_eq!(approval_count(&store), 2);
        for (id, approval, summary) in [(r1, running, "Write a was interrupted. Run again?"), (r2, lost, "Write b was interrupted. Run again?")] {
            assert_eq!(row(&store, id).approval_id, Some(approval));
            let a = store.get_approval(approval).unwrap();
            assert_eq!(a.status, "new");
            assert_eq!(a.summary, summary);
            assert_eq!(a.detail, "Heron stopped while doing this, in /work/repo. It may or may not have finished. Check before running it again.");
            assert_eq!(a.origin, "job");
        }
    }

    #[test]
    fn parked_call_whose_approval_cannot_reopen_gets_a_new_one() {
        let (store, home) = setup();
        let answered = store.park_approval("s", "write_file", r#"{"path":"a"}"#, "Write a", "", "write", false, "job").unwrap();
        store.resolve_approval(answered, "approved", Some("done")).unwrap();
        let mut gone = call("s", "write_file", json!({"path": "a"}), &parked_run_id(answered));
        gone.summary = "Write a".into();
        let r1 = insert(&store, &gone, "crashed");
        let r2 = insert(&store, &call("s2", "write_file", json!({"path": "z"}), &parked_run_id(9999)), "crashed");
        assert_eq!(recover(&store, home.path()).unwrap(), 2);
        assert_eq!(store.get_approval(answered).unwrap().status, "approved");
        assert_eq!(store.get_approval(answered).unwrap().summary, "Write a");
        let fresh = row(&store, r1).approval_id.unwrap();
        assert_ne!(fresh, answered);
        let a = store.get_approval(fresh).unwrap();
        assert_eq!(a.status, "new");
        assert_eq!(a.summary, "Write a was interrupted. Run again?");
        assert_eq!(a.origin, "interrupted: chat");
        let other = row(&store, r2).approval_id.unwrap();
        assert!(other != 9999 && other != fresh);
        assert_eq!(store.get_approval(other).unwrap().session_id, "s2");
    }

    #[test]
    fn live_child_process_group_delays_the_ask() {
        let store = Store::in_memory().unwrap();
        let now = crate::util::now_ts();
        let rows: Vec<i64> = (0..5).map(|i| insert(&store, &call("s", "shell", json!({ "n": i }), "turn:9"), "crashed")).collect();
        let cases = [(Some(41), 0), (Some(41), sql::CHILD_GRACE_SECS - 1), (Some(41), sql::CHILD_GRACE_SECS), (Some(42), 0), (None, 0)];
        for (&id, (pgid, age)) in rows.iter().zip(cases) {
            sql_exec(&store, "UPDATE effects SET child_pgid=?, started_at=? WHERE id=?", rusqlite::params![pgid, now - age, id]);
        }
        let probed = std::sync::Mutex::new(Vec::new());
        let marked = store
            .with_conn(|c| {
                sql::mark_interrupted(
                    c,
                    "crashed",
                    |g| {
                        probed.lock().unwrap().push(g);
                        g == 41
                    },
                    now,
                )
            })
            .unwrap();
        assert_eq!(marked, 3);
        let states: Vec<String> = rows.iter().map(|&id| row(&store, id).state).collect();
        assert_eq!(states, ["intent", "intent", "interrupted", "interrupted", "interrupted"]);
        assert!(probed.lock().unwrap().contains(&42));
        assert_eq!(store.with_conn(|c| sql::mark_interrupted(c, "someone-else", |_| false, now)).unwrap(), 0);

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            let (store, home) = setup();
            let mut child = std::process::Command::new("sleep").arg("30").process_group(0).spawn().unwrap();
            let pgid = child.id() as i64;
            assert!(group_alive(pgid));
            let id = insert(&store, &call("s", "shell", json!({"command": "sleep 30"}), "turn:10"), "crashed");
            store.with_conn(|c| sql::set_child(c, id, pgid)).unwrap();
            assert_eq!(recover(&store, home.path()).unwrap(), 0);
            assert_eq!(row(&store, id).state, "intent");
            child.kill().unwrap();
            child.wait().unwrap();
            assert!(!group_alive(pgid));
            assert_eq!(recover(&store, home.path()).unwrap(), 1);
            assert_eq!(row(&store, id).state, "asked");
            assert!(!group_alive(0) && !group_alive(1) && !group_alive(-5));
        }
    }

    #[tokio::test]
    async fn note_child_records_the_group_only_inside_guarded() {
        let (store, home) = setup();
        note_child(777);
        let out = guarded(&store, home.path(), call("s", "shell", json!({"command": "x"}), "turn:child"), Duration::from_secs(5), async {
            note_child(4321);
            Ok("ok".to_string())
        })
        .await;
        assert!(matches!(out, Guarded::Ran(Ok(_))));
        assert_eq!(row(&store, 1).child_pgid, Some(4321));
        assert_eq!(row_count(&store), 1);
        Lease::release(home.path());
    }

    #[test]
    fn interrupted_action_uses_its_label() {
        let (store, home) = setup();
        let action = ActionIntent {
            handler: "selfheal.repair",
            subject: "12".into(),
            label: "Repair #12".into(),
            session_id: "sess-r".into(),
            origin: "selfheal".into(),
            tainted: true,
            workspace: PathBuf::from("/work/repo"),
        };
        let id = begin_action(&store, home.path(), action.clone()).unwrap();
        let other = begin_action(&store, home.path(), action).unwrap();
        finish(&store, other, Outcome::Done).unwrap();
        sql_exec(&store, "UPDATE effects SET proc='crashed' WHERE id=?", [id]);
        Lease::release(home.path());
        assert_eq!(recover(&store, home.path()).unwrap(), 1);
        let a = store.get_approval(row(&store, id).approval_id.unwrap()).unwrap();
        assert_eq!(a.tool, "resume:selfheal.repair");
        assert_eq!(serde_json::from_str::<serde_json::Value>(&a.args).unwrap(), json!({"subject": "12"}));
        assert_eq!(a.summary, "Repair #12 was interrupted. Run again?");
        assert_eq!(a.detail, "Heron stopped while doing this. It may or may not have finished. Check before running it again.");
        assert_eq!(a.risk, "exec");
        assert!(a.tainted);
        assert_eq!(a.origin, "interrupted: selfheal");
        assert_eq!(a.session_id, "sess-r");
        assert_eq!(resume_handler(&a.tool), Some("selfheal.repair"));
        assert_eq!(row(&store, other).state, "done");
    }

    #[test]
    fn finished_action_records_its_outcome() {
        let (store, home) = setup();
        let action = ActionIntent {
            handler: "selfheal.repair",
            subject: "7".into(),
            label: "Repair #7".into(),
            session_id: "s".into(),
            origin: "selfheal".into(),
            tainted: false,
            workspace: PathBuf::from("/w"),
        };
        let id = begin_action(&store, home.path(), action).unwrap();
        let open = row(&store, id);
        assert_eq!(open.state, "intent");
        assert_eq!(open.args.as_deref(), Some("7"));
        assert_eq!(open.args_hash, args_hash(&json!({"subject": "7"})));
        finish(&store, id, Outcome::Failed("the repair broke".into())).unwrap();
        let r = row(&store, id);
        assert_eq!(r.state, "failed");
        assert_eq!(r.outcome.as_deref(), Some("the repair broke"));
        assert_eq!(r.args, None);
        Lease::release(home.path());
    }

    struct Recorder;

    #[async_trait]
    impl Resumer for Recorder {
        async fn rerun(&self, _app: &crate::app::App, subject: &str, _tainted: bool) -> Result<String> {
            Ok(subject.to_string())
        }
        fn declined(&self, _app: &crate::app::App, _subject: &str) {}
    }

    #[test]
    fn registered_resumer_is_found_by_its_handler() {
        assert!(resumer("test.recorder").is_none());
        register_resumer("test.recorder", Arc::new(Recorder));
        assert!(resumer("test.recorder").is_some());
        assert!(resumer("test.other").is_none());
        assert_eq!(resume_tool("test.recorder"), "resume:test.recorder");
        assert_eq!(resume_handler("shell"), None);
    }

    #[test]
    fn pending_ask_again_matches_session_tool_and_args() {
        let (store, home) = setup();
        let intent = call("s", "shell", json!({"command": "make"}), "turn:11");
        let (_, approval) = asked(&store, &intent);
        let gate = |session: &str, name: &str, hash: &str| pending_ask_again(&store, home.path(), session, name, hash);
        assert_eq!(gate("s", "shell", &intent.args_hash), Some(Gate::Waiting(approval)));
        assert_eq!(gate("other", "shell", &intent.args_hash), None);
        assert_eq!(gate("s", "write_file", &intent.args_hash), None);
        assert_eq!(gate("s", "shell", &args_hash(&json!({"command": "make test"}))), None);
        assert!(store.claim_approval(approval).unwrap());
        assert_eq!(gate("s", "shell", &intent.args_hash), Some(Gate::Running(approval)));
        store.resolve_approval(approval, "approved", None).unwrap();
        assert_eq!(gate("s", "shell", &intent.args_hash), None);
    }

    #[test]
    fn pending_ask_again_sees_unrecovered_rows() {
        let (store, home) = setup();
        let intent = call("s", "shell", json!({"command": "make"}), "turn:12");
        let gate = || pending_ask_again(&store, home.path(), "s", "shell", &intent.args_hash);
        let _live = hold_lease(home.path(), "live-proc");
        insert(&store, &intent, "live-proc");
        insert(&store, &intent, proc_id());
        let done = insert(&store, &intent, "crashed");
        assert!(store.with_conn(|c| sql::write_outcome(c, done, "done", None)).unwrap());
        assert!(!store.with_conn(|c| sql::write_outcome(c, done, "failed", Some("late"))).unwrap());
        assert_eq!(row(&store, done).state, "done");
        assert_eq!(gate(), None);
        let dead = insert(&store, &intent, "crashed");
        let cut = interrupted(&store, &intent);
        assert_eq!(gate(), Some(Gate::Unasked(vec![dead, cut])));

        let (_, waiting) = asked(&store, &call("s", "shell", json!({"command": "make"}), "turn:13"));
        assert_eq!(gate(), Some(Gate::Waiting(waiting)));
        store.resolve_approval(waiting, "dismissed", None).unwrap();

        assert!(supersede(&store, &Gate::Unasked(vec![dead, cut])).unwrap());
        for id in [dead, cut] {
            let r = row(&store, id);
            assert_eq!(r.state, "cancelled");
            assert_eq!(r.outcome.as_deref(), Some(RERUN_OUTCOME));
            assert_eq!(r.args, None);
        }
        assert_eq!(gate(), None);
        assert_eq!(recover(&store, home.path()).unwrap(), 0);
    }

    #[test]
    fn supersede_retires_a_waiting_ask_again_once() {
        let (store, _home) = setup();
        let (_, approval) = asked(&store, &call("s", "shell", json!({"command": "make"}), "turn:14"));
        assert!(supersede(&store, &Gate::Waiting(approval)).unwrap());
        let a = store.get_approval(approval).unwrap();
        assert_eq!(a.status, "dismissed");
        assert_eq!(a.result.as_deref(), Some(SUPERSEDED_RESULT));
        assert!(!supersede(&store, &Gate::Waiting(approval)).unwrap());
        assert!(!supersede(&store, &Gate::Running(approval)).unwrap());
    }

    #[test]
    fn live_parked_approvals_lists_only_live_leases() {
        let (store, home) = setup();
        let _live = hold_lease(home.path(), "live-proc");
        stale_lease(home.path(), "dead-proc");
        insert(&store, &call("s", "shell", json!({}), &parked_run_id(5)), "live-proc");
        insert(&store, &call("s", "shell", json!({"n": 1}), &parked_run_id(5)), "live-proc");
        insert(&store, &call("s", "shell", json!({}), &parked_run_id(6)), "dead-proc");
        insert(&store, &call("s", "shell", json!({}), &parked_run_id(7)), proc_id());
        insert(&store, &call("s", "shell", json!({}), "turn:15"), "live-proc");
        insert(&store, &call("s", "shell", json!({}), "parked:x"), "live-proc");
        let finished = insert(&store, &call("s", "shell", json!({}), &parked_run_id(8)), "live-proc");
        store.with_conn(|c| sql::write_outcome(c, finished, "done", None)).unwrap();
        assert_eq!(live_parked_approvals(&store, home.path()), vec![5, 7]);
    }

    #[test]
    fn dead_lease_files_are_swept() {
        let (store, home) = setup();
        let with_rows = stale_lease(home.path(), "crashed-with-rows");
        let without_rows = stale_lease(home.path(), "crashed-idle");
        let _live = hold_lease(home.path(), "live-proc");
        insert(&store, &call("s", "shell", json!({}), "turn:16"), "crashed-with-rows");
        Lease::acquire(home.path()).unwrap();
        recover(&store, home.path()).unwrap();
        assert!(!with_rows.exists());
        assert!(!without_rows.exists());
        let dir = lease::lease_dir(home.path());
        assert!(dir.join("live-proc.lock").exists());
        assert!(dir.join(format!("{}.lock", proc_id())).exists());
        Lease::release(home.path());
    }

    #[test]
    fn args_hash_ignores_key_order() {
        let a: serde_json::Value = serde_json::from_str(r#"{"b": 1, "a": {"y": [2, {"q": 1, "p": "x"}], "x": null}}"#).unwrap();
        let b: serde_json::Value = serde_json::from_str(r#"{"a": {"x": null, "y": [2, {"p": "x", "q": 1}]}, "b": 1}"#).unwrap();
        assert_eq!(args_hash(&a), args_hash(&b));
        assert_eq!(args_hash(&a), crate::util::sha256_hex(br#"{"a":{"x":null,"y":[2,{"p":"x","q":1}]},"b":1}"#));
        assert_ne!(args_hash(&a), args_hash(&json!({"a": {"x": null, "y": [{"p": "x", "q": 1}, 2]}, "b": 1})));
        assert_ne!(args_hash(&json!({"a": "1"})), args_hash(&json!({"a": 1})));
        assert_eq!(args_hash(&json!({"k\"ey": "v"})), crate::util::sha256_hex(br#"{"k\"ey":"v"}"#));
    }

    fn age(store: &Store, id: i64, secs: i64) {
        let at = crate::util::now_ts() - secs;
        sql_exec(store, "UPDATE effects SET started_at=?, finished_at=CASE WHEN finished_at IS NULL THEN NULL ELSE ? END WHERE id=?", rusqlite::params![at, at, id]);
    }

    fn cutoff() -> i64 {
        crate::util::now_ts() - RETAIN_SECS
    }

    #[test]
    fn prune_keeps_open_rows() {
        let store = Store::in_memory().unwrap();
        let intent = call("s", "shell", json!({}), "turn:17");
        let open = insert(&store, &intent, "p");
        let cut = interrupted(&store, &intent);
        let (waiting, _) = asked(&store, &call("s", "shell", json!({"n": 1}), "turn:17"));
        let (running, running_approval) = asked(&store, &call("s", "shell", json!({"n": 2}), "turn:17"));
        assert!(store.claim_approval(running_approval).unwrap());
        let old_done = insert(&store, &intent, "p");
        store.with_conn(|c| sql::write_outcome(c, old_done, "done", None)).unwrap();
        let recent_done = insert(&store, &intent, "p");
        store.with_conn(|c| sql::write_outcome(c, recent_done, "done", None)).unwrap();
        for id in [open, cut, waiting, running, old_done] {
            age(&store, id, RETAIN_SECS + 60);
        }
        age(&store, recent_done, RETAIN_SECS - 60);
        assert_eq!(store.with_conn(|c| sql::prune(c, cutoff())).unwrap(), 1);
        let left: Vec<i64> = store.with_conn(|c| Ok(c.prepare("SELECT id FROM effects ORDER BY id")?.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)).unwrap();
        assert_eq!(left, vec![open, cut, waiting, running, recent_done]);
    }

    #[test]
    fn asked_rows_prune_once_their_approval_is_answered() {
        let store = Store::in_memory().unwrap();
        let (answered, approval) = asked(&store, &call("s", "shell", json!({"n": 1}), "turn:18"));
        let (orphan, orphan_approval) = asked(&store, &call("s", "shell", json!({"n": 2}), "turn:18"));
        sql_exec(&store, "DELETE FROM approvals WHERE id=?", [orphan_approval]);
        let failed = insert(&store, &call("s", "shell", json!({"n": 3}), "turn:18"), "p");
        store.with_conn(|c| sql::write_outcome(c, failed, "failed", Some("x"))).unwrap();
        let cancelled = insert(&store, &call("s", "shell", json!({"n": 4}), "turn:18"), "p");
        store.with_conn(|c| sql::cancel_rows(c, &[cancelled], "x")).unwrap();
        let (recent, recent_approval) = asked(&store, &call("s", "shell", json!({"n": 5}), "turn:18"));
        store.resolve_approval(recent_approval, "approved", None).unwrap();
        for id in [answered, orphan, failed, cancelled] {
            age(&store, id, RETAIN_SECS + 60);
        }
        assert_eq!(store.with_conn(|c| sql::prune(c, cutoff())).unwrap(), 3);
        assert_eq!(row_count(&store), 2);
        store.resolve_approval(approval, "approved", None).unwrap();
        assert_eq!(store.with_conn(|c| sql::prune(c, cutoff())).unwrap(), 1);
        assert_eq!(row(&store, recent).state, "asked");
        assert_eq!(row_count(&store), 1);
    }
}
