//! Run records (F3): one record per run, kept in kv `run:<run_id>`, so the app can list the runs in
//! progress and the ones that finished. It isn't the scheduled-job history in the `runs` table.
//!
//! One writer keeps the records: the [`RunLog`], which the event bus calls synchronously inside
//! `publish`, before the event is broadcast. So a slow subscriber can lose no update, a run's record
//! is final before its `run_end` goes out, and nothing else writes the key.

use crate::events::BusEvent;
use crate::store::{RecordedCheckpoint, Store};
use crate::util::now_ts;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;

/// Records kept: the core deletes the older ones.
pub const KEEP: usize = 500;
const PREFIX: &str = "run:";

/// What `run:<run_id>` holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRecord {
    pub run_id: String,
    pub session_id: String,
    pub title: String,
    /// Where the run's session came from: `user`, `telegram`, `schedule`, `automation` or `proactive`.
    pub origin: String,
    /// `running`, `waiting` (for an approval), `done`, `error` or `cancelled`.
    pub status: String,
    pub started: i64,
    pub ended: Option<i64>,
    pub tool_calls: u64,
    /// The checkpoints the run's file tools saved.
    pub checkpoints: Vec<RecordedCheckpoint>,
    /// The audit rows the run's tool calls appended.
    pub audit_ids: Vec<i64>,
}

impl RunRecord {
    fn finished(&self) -> bool {
        self.ended.is_some()
    }

    /// The record without its two lists, as `GET /api/runs` lists it.
    pub fn summary(&self) -> Value {
        json!({
            "run_id": self.run_id, "session_id": self.session_id, "title": self.title, "origin": self.origin,
            "status": self.status, "started": self.started, "ended": self.ended, "tool_calls": self.tool_calls,
        })
    }
}

#[derive(Default)]
struct Cache {
    /// Oldest first; the newest [`KEEP`] records.
    records: VecDeque<RunRecord>,
    /// Approvals each unfinished run is waiting on, by approval id. Not saved: a run does not outlive
    /// its process.
    pending: HashMap<String, HashSet<String>>,
}

/// The keeper of run records. Cloning the bus shares it.
pub struct RunLog {
    store: Store,
    cache: Mutex<Cache>,
}

impl RunLog {
    /// Open the log over `store`: the records already saved are the cache, and any beyond the newest
    /// [`KEEP`] are deleted.
    pub fn open(store: Store) -> RunLog {
        let mut records: Vec<RunRecord> = store.kv_list_prefix(PREFIX).unwrap_or_default().into_iter().filter_map(|(_, v)| serde_json::from_str(&v).ok()).collect();
        records.sort_by(|a, b| (a.started, &a.run_id).cmp(&(b.started, &b.run_id)));
        let log = RunLog { store, cache: Mutex::new(Cache { records: records.into(), pending: HashMap::new() }) };
        log.prune(&mut log.cache.lock().unwrap_or_else(|e| e.into_inner()));
        log
    }

    /// Follow one published event. The bus calls this before it broadcasts the event.
    pub fn record(&self, ev: &BusEvent) {
        if ev.no_run || ev.run_id.is_empty() {
            return;
        }
        let kind = ev.kind();
        if !matches!(kind, "session" | "tool_end" | "approval" | "approval_resolved" | "approval_parked" | "done" | "error" | "cancelled") {
            return;
        }
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if kind == "session" {
            if cache.records.iter().any(|r| r.run_id == ev.run_id) {
                return;
            }
            let origin = self.store.session_origin(&ev.session_id).unwrap_or_else(|| "user".into());
            let title = self.store.session_title(&ev.session_id);
            let record = RunRecord { run_id: ev.run_id.clone(), session_id: ev.session_id.clone(), title, origin, status: "running".into(), started: now_ts(), ended: None, tool_calls: 0, checkpoints: vec![], audit_ids: vec![] };
            self.save(&record);
            cache.records.push_back(record);
            self.prune(&mut cache);
            return;
        }
        let Cache { records, pending } = &mut *cache;
        // An event for a run with no record (one the log never saw start, or one already pruned) is not
        // a run to follow.
        let Some(rec) = records.iter_mut().rev().find(|r| r.run_id == ev.run_id) else { return };
        if rec.finished() {
            return;
        }
        let approval_id = ev.json.get("id").and_then(Value::as_str).unwrap_or_default();
        match kind {
            "tool_end" => {
                rec.tool_calls += 1;
                if let Some(id) = ev.json.get("audit_id").and_then(Value::as_i64) {
                    rec.audit_ids.push(id);
                }
                if let Some(cp) = ev.json.get("checkpoint").and_then(|c| serde_json::from_value::<RecordedCheckpoint>(c.clone()).ok()) {
                    rec.checkpoints.push(cp);
                }
            }
            "approval" => {
                pending.entry(ev.run_id.clone()).or_default().insert(approval_id.to_string());
                rec.status = "waiting".into();
            }
            "approval_resolved" | "approval_parked" => {
                // Calls that only read may ask side by side: the run is running again once none is left.
                let left = pending.get_mut(&ev.run_id).map(|p| {
                    p.remove(approval_id);
                    p.len()
                });
                if left.unwrap_or(0) == 0 {
                    pending.remove(&ev.run_id);
                    rec.status = "running".into();
                }
            }
            _ => {
                pending.remove(&ev.run_id);
                rec.status = kind.into();
                rec.ended = Some(now_ts());
                if let Some(title) = ev.json.get("title").and_then(Value::as_str).filter(|t| !t.is_empty()) {
                    rec.title = title.to_string();
                }
            }
        }
        self.save(rec);
    }

    /// Every record, newest first.
    pub fn list(&self) -> Vec<RunRecord> {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).records.iter().rev().cloned().collect()
    }

    /// `{"runs": [record, …]}` without the two lists, newest first: what `GET /api/runs` returns.
    pub fn summaries(&self) -> Value {
        json!({"runs": self.list().iter().map(RunRecord::summary).collect::<Vec<_>>()})
    }

    fn save(&self, rec: &RunRecord) {
        if let Ok(json) = serde_json::to_string(rec) {
            if let Err(e) = self.store.kv_set(&format!("{PREFIX}{}", rec.run_id), &json) {
                eprintln!("heron: could not save the record of run {}: {e:#}", rec.run_id);
            }
        }
    }

    /// Delete the records beyond the newest [`KEEP`].
    fn prune(&self, cache: &mut Cache) {
        while cache.records.len() > KEEP {
            if let Some(old) = cache.records.pop_front() {
                cache.pending.remove(&old.run_id);
                if let Err(e) = self.store.kv_delete(&format!("{PREFIX}{}", old.run_id)) {
                    eprintln!("heron: could not delete the record of run {}: {e:#}", old.run_id);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{EventBus, Item};

    fn bus_over(store: &Store) -> EventBus {
        EventBus::with_runs(store.clone())
    }

    fn record_of(store: &Store, run: &str) -> RunRecord {
        serde_json::from_str(&store.kv_get(&format!("run:{run}")).unwrap_or_else(|| panic!("no record for {run}"))).unwrap()
    }

    fn started(store: &Store, bus: &EventBus, origin: &str, title: &str) -> (String, String) {
        let sid = store.create_session(title, origin).unwrap();
        let run = crate::events::new_run_id();
        bus.publish(&sid, &run, json!({"type": "session", "id": sid, "run_id": run}));
        (sid, run)
    }

    fn checkpoint(id: i64) -> Value {
        json!({"id": id, "path": "/w/a.txt", "after_hash": "aa", "ts": 7, "prev_hash": "absent"})
    }

    #[test]
    fn run_record_follows_the_bus() {
        let store = Store::in_memory().unwrap();
        let bus = bus_over(&store);
        let (sid, run) = started(&store, &bus, "schedule", "Morning notes");
        let at_start = record_of(&store, &run);
        assert_eq!((at_start.session_id.as_str(), at_start.title.as_str(), at_start.origin.as_str(), at_start.status.as_str()), (sid.as_str(), "Morning notes", "schedule", "running"));
        assert_eq!((at_start.ended, at_start.tool_calls, at_start.checkpoints.len(), at_start.audit_ids.len()), (None, 0, 0, 0));
        assert!(at_start.started > 0);

        bus.publish(&sid, &run, json!({"type": "text", "text": "hello"}));
        bus.publish(&sid, &run, json!({"type": "tool_start", "id": "c1", "name": "write_file"}));
        bus.publish(&sid, &run, json!({"type": "tool_end", "id": "c1", "name": "write_file", "ok": true, "preview": "wrote", "audit_id": 11, "checkpoint": checkpoint(4)}));
        bus.publish(&sid, &run, json!({"type": "tool_end", "id": "c2", "name": "read_file", "ok": true, "preview": "x", "audit_id": 12}));
        // A call that left no audit row or checkpoint is still a call.
        bus.publish(&sid, &run, json!({"type": "tool_end", "id": "c3", "name": "delegate", "ok": true, "preview": "x"}));
        let mid = record_of(&store, &run);
        assert_eq!((mid.tool_calls, mid.audit_ids.clone(), mid.status.as_str()), (3, vec![11, 12], "running"));
        assert_eq!(mid.checkpoints, vec![RecordedCheckpoint { id: 4, path: "/w/a.txt".into(), after_hash: Some("aa".into()), ts: 7, prev_hash: "absent".into() }]);

        bus.publish(&sid, &run, json!({"type": "done", "text": "ok", "title": "Notes, tidied"}));
        let done = record_of(&store, &run);
        assert_eq!((done.status.as_str(), done.title.as_str(), done.tool_calls), ("done", "Notes, tidied", 3));
        assert!(done.ended.is_some_and(|e| e >= done.started));
        // Nothing after the end changes the record.
        bus.publish(&sid, &run, json!({"type": "tool_end", "id": "c4", "name": "x", "ok": true, "preview": "", "audit_id": 99}));
        bus.publish(&sid, &run, json!({"type": "approval", "id": "a9", "tool": "shell"}));
        bus.publish(&sid, &run, json!({"type": "run_end"}));
        assert_eq!(record_of(&store, &run), done);

        // `error` and `cancelled` end a run too, each with its own status.
        for (kind, status) in [("error", "error"), ("cancelled", "cancelled")] {
            let (sid, run) = started(&store, &bus, "user", "t");
            bus.publish(&sid, &run, json!({"type": kind, "message": "boom", "code": null}));
            let r = record_of(&store, &run);
            assert_eq!(r.status, status);
            assert!(r.ended.is_some());
        }
        // Events of another run, a notice, and one for a run never started touch nothing.
        let (sid2, run2) = started(&store, &bus, "user", "other");
        bus.publish_notice(Some(&sid2), None, json!({"type": "tool_end", "id": "n", "name": "x", "ok": true, "preview": "", "audit_id": 1}));
        bus.publish(&sid2, "ghost", json!({"type": "tool_end", "id": "g", "name": "x", "ok": true, "preview": "", "audit_id": 2}));
        assert!(store.kv_get("run:ghost").is_none());
        assert_eq!((record_of(&store, &run2).tool_calls, record_of(&store, &run2).status.as_str().to_string()), (0, "running".to_string()));
        // A repeated `session` does not start the run over.
        bus.publish(&sid2, &run2, json!({"type": "tool_end", "id": "c", "name": "x", "ok": true, "preview": "", "audit_id": 5}));
        bus.publish(&sid2, &run2, json!({"type": "session", "id": sid2, "run_id": run2}));
        assert_eq!(record_of(&store, &run2).tool_calls, 1);
    }

    #[test]
    fn waiting_returns_to_running() {
        let store = Store::in_memory().unwrap();
        let bus = bus_over(&store);
        let (sid, run) = started(&store, &bus, "user", "t");
        let status = |store: &Store| record_of(store, &run).status;
        bus.publish(&sid, &run, json!({"type": "approval", "id": "a1", "tool": "write_file"}));
        assert_eq!(status(&store), "waiting");
        bus.publish(&sid, &run, json!({"type": "approval_resolved", "id": "a1", "decision": "once"}));
        assert_eq!(status(&store), "running");
        // Parked: the approval timed out and the run carries on.
        bus.publish(&sid, &run, json!({"type": "approval", "id": "a2", "tool": "shell"}));
        assert_eq!(status(&store), "waiting");
        bus.publish(&sid, &run, json!({"type": "approval_parked", "id": "a2", "parked_id": 3}));
        assert_eq!(status(&store), "running");
        // Two approvals side by side: the run waits until both are answered.
        bus.publish(&sid, &run, json!({"type": "approval", "id": "a3", "tool": "read_file"}));
        bus.publish(&sid, &run, json!({"type": "approval", "id": "a4", "tool": "read_file"}));
        bus.publish(&sid, &run, json!({"type": "approval_resolved", "id": "a3", "decision": "once"}));
        assert_eq!(status(&store), "waiting");
        bus.publish(&sid, &run, json!({"type": "approval_resolved", "id": "a4", "decision": "deny"}));
        assert_eq!(status(&store), "running");
        // A cancel while waiting: the approval is cleared, then the run ends.
        bus.publish(&sid, &run, json!({"type": "approval", "id": "a5", "tool": "shell"}));
        bus.publish(&sid, &run, json!({"type": "approval_resolved", "id": "a5", "decision": "cancelled"}));
        bus.publish(&sid, &run, json!({"type": "cancelled"}));
        assert_eq!(status(&store), "cancelled");
        // An answer that arrives after the end does not reopen the run.
        bus.publish(&sid, &run, json!({"type": "approval_resolved", "id": "a5", "decision": "once"}));
        assert_eq!(status(&store), "cancelled");
        // `GET /api/runs` counts the waiting ones.
        let (sid2, run2) = started(&store, &bus, "user", "t2");
        bus.publish(&sid2, &run2, json!({"type": "approval", "id": "b1", "tool": "shell"}));
        let listed = bus.runs().unwrap().summaries();
        let waiting = listed["runs"].as_array().unwrap().iter().filter(|r| r["status"] == "waiting").count();
        assert_eq!(waiting, 1, "{listed}");
    }

    #[tokio::test]
    async fn run_record_is_written_before_run_end() {
        let store = Store::in_memory().unwrap();
        let bus = bus_over(&store);
        let (sid, run) = started(&store, &bus, "user", "t");
        let mut sub = bus.subscribe(None);
        bus.publish(&sid, &run, json!({"type": "tool_end", "id": "c", "name": "x", "ok": true, "preview": "", "audit_id": 8}));
        bus.publish(&sid, &run, json!({"type": "done", "text": "ok", "title": "t"}));
        // The record was final when `publish` returned, before the event reached anyone...
        assert_eq!(record_of(&store, &run).status, "done");
        bus.publish(&sid, &run, json!({"type": "run_end"}));
        // ...so whoever sees an event, `run_end` included, reads a record that already follows it.
        let mut seen = vec![];
        while seen.last().map(String::as_str) != Some("run_end") {
            let Ok(Some(Item::Event(e))) = tokio::time::timeout(std::time::Duration::from_secs(2), sub.next()).await else { panic!("the stream ended early: {seen:?}") };
            let rec = record_of(&store, &run);
            match e.kind() {
                "tool_end" => assert_eq!(rec.tool_calls, 1),
                "done" | "run_end" => assert_eq!((rec.status.as_str(), rec.ended.is_some(), rec.audit_ids.as_slice()), ("done", true, &[8][..])),
                other => panic!("unexpected {other}"),
            }
            seen.push(e.kind().to_string());
        }
        assert_eq!(seen, ["tool_end", "done", "run_end"]);
    }

    #[test]
    fn run_records_are_pruned_to_500() {
        let store = Store::in_memory().unwrap();
        let bus = bus_over(&store);
        let sid = store.create_session("t", "user").unwrap();
        let runs: Vec<String> = (0..KEEP + 3).map(|i| format!("run-{i:04}")).collect();
        for run in &runs {
            bus.publish(&sid, run, json!({"type": "session", "id": sid, "run_id": run}));
        }
        let keys: Vec<String> = store.kv_list_prefix("run:").unwrap().into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys.len(), KEEP, "the oldest three are deleted");
        assert!(keys.contains(&format!("run:{}", runs[3])) && keys.contains(&format!("run:{}", runs[KEEP + 2])));
        for old in &runs[..3] {
            assert!(store.kv_get(&format!("run:{old}")).is_none(), "{old} should be gone");
        }
        // The list is the newest 500, newest first, and an event for a pruned run starts nothing.
        let listed = bus.runs().unwrap().summaries();
        let ids: Vec<&str> = listed["runs"].as_array().unwrap().iter().map(|r| r["run_id"].as_str().unwrap()).collect();
        assert_eq!((ids.len(), ids[0], ids[KEEP - 1]), (KEEP, runs[KEEP + 2].as_str(), runs[3].as_str()));
        bus.publish(&sid, &runs[0], json!({"type": "done", "text": "late"}));
        assert!(store.kv_get(&format!("run:{}", runs[0])).is_none());
        // Other keys are never touched, nor ones that only look alike.
        store.kv_set("runner:x", "keep").unwrap();
        store.kv_set("run_count", "keep").unwrap();
        bus.publish(&sid, "run-extra", json!({"type": "session", "id": sid, "run_id": "run-extra"}));
        assert_eq!((store.kv_get("runner:x").as_deref(), store.kv_get("run_count").as_deref()), (Some("keep"), Some("keep")));
        assert_eq!(store.kv_list_prefix("run:").unwrap().len(), KEEP);
        // A restart trims what is saved: open over 505 records keeps the newest 500.
        let reopened = Store::in_memory().unwrap();
        for (i, run) in runs.iter().enumerate() {
            let rec = RunRecord { run_id: run.clone(), session_id: sid.clone(), title: "t".into(), origin: "user".into(), status: "done".into(), started: 1000 + i as i64, ended: Some(2000), tool_calls: 0, checkpoints: vec![], audit_ids: vec![] };
            reopened.kv_set(&format!("run:{run}"), &serde_json::to_string(&rec).unwrap()).unwrap();
        }
        let log = RunLog::open(reopened.clone());
        assert_eq!(reopened.kv_list_prefix("run:").unwrap().len(), KEEP);
        assert_eq!(log.list().first().map(|r| r.run_id.as_str()), Some(runs[KEEP + 2].as_str()));
        assert!(reopened.kv_get(&format!("run:{}", runs[0])).is_none() && reopened.kv_get(&format!("run:{}", runs[3])).is_some());
    }
}
