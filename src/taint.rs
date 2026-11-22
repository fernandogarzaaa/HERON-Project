//! The taint source: one function that marks a conversation as carrying outside content.
//!
//! kv `tainted:<session>` says a conversation has taken in content from outside the owner's control.
//! Readers check only that the key is there, so a value written before this module (`"1"`) still
//! counts; its source then reads as `None`. A value written here is JSON:
//! `{"kind", "name", "detail", "ts"}`, where `kind` comes from the [`ContentEntry`], and `detail`
//! never holds content.

use crate::agent::{ContentEntry, Event, EventSink};
use crate::store::Store;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};

/// The kv key that marks `session_id` as tainted.
pub fn key(session_id: &str) -> String {
    format!("tainted:{session_id}")
}

/// Mark `session_id` as tainted by content from `entry`. Sets `flag` whatever else happens, so the
/// turn in progress fails closed; keeps the first source written (a later one never replaces it);
/// and publishes `tainted` on `sink` the first time. Returns true the first time.
///
/// An entry that never taints (trusted content, a system-prompt block) changes nothing.
pub fn mark(store: &Store, sink: &EventSink, flag: &AtomicBool, session_id: &str, entry: ContentEntry, name: &str, detail: &str) -> bool {
    let Some(kind) = entry.taint_kind() else { return false };
    flag.store(true, Ordering::SeqCst);
    let ts = crate::util::now_ts();
    let value = json!({"kind": kind, "name": name, "detail": detail, "ts": ts});
    let first = store.kv_insert_if_absent(&key(session_id), &value.to_string()).unwrap_or(false);
    if first {
        sink(Event::Tainted { kind: kind.into(), name: name.into(), detail: detail.into(), ts });
    }
    first
}

/// Whether `session_id` is tainted: the key is there, whatever its value.
pub fn is_tainted(store: &Store, session_id: &str) -> bool {
    store.kv_get(&key(session_id)).is_some()
}

/// What first tainted `session_id`, as written by [`mark`]; `None` when it is not tainted, or was
/// tainted before sources were recorded.
pub fn source(store: &Store, session_id: &str) -> Option<Value> {
    store.kv_get(&key(session_id)).and_then(|v| serde_json::from_str::<Value>(&v).ok()).filter(Value::is_object)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn recording() -> (EventSink, Arc<Mutex<Vec<Event>>>) {
        let seen = Arc::new(Mutex::new(vec![]));
        let s = seen.clone();
        (Arc::new(move |e| s.lock().unwrap().push(e)), seen)
    }

    #[test]
    fn taint_mark_keeps_the_first_source() {
        let store = Store::in_memory().unwrap();
        let (sink, seen) = recording();
        let flag = AtomicBool::new(false);
        assert!(!is_tainted(&store, "s1"));
        assert!(mark(&store, &sink, &flag, "s1", ContentEntry::ToolOutput { untrusted: true }, "web_fetch", "127.0.0.1"), "the first mark reports itself");
        assert!(flag.load(Ordering::SeqCst));
        let first = source(&store, "s1").unwrap();
        assert_eq!((first["kind"].as_str(), first["name"].as_str(), first["detail"].as_str()), (Some("tool"), Some("web_fetch"), Some("127.0.0.1")));
        assert!(first["ts"].as_i64().unwrap() > 0);
        // A later source, even of another kind, never replaces the first, and is not announced again.
        let again = AtomicBool::new(false);
        assert!(!mark(&store, &sink, &again, "s1", ContentEntry::ChatContext, "selection", "Mail"));
        assert!(again.load(Ordering::SeqCst), "the flag is set every time, so the turn in progress fails closed");
        assert_eq!(source(&store, "s1").unwrap(), first);
        let events = seen.lock().unwrap();
        assert_eq!(events.len(), 1, "{events:?}");
        assert!(matches!(&events[0], Event::Tainted { kind, name, detail, .. } if kind == "tool" && name == "web_fetch" && detail == "127.0.0.1"), "{events:?}");
        drop(events);
        // Each kind records the fields the design gives it; another session starts from nothing.
        for (i, (entry, name, detail, kind)) in [
            (ContentEntry::ChatContext, "selection", "Mail", "chat_context"),
            (ContentEntry::McpCall, "mcp", "conn-1", "mcp"),
            (ContentEntry::TelegramForward, "telegram", "", "telegram_forward"),
            (ContentEntry::TriggerEvent, "Invoices", "email", "trigger"),
            (ContentEntry::Suggestion, "suggestion", "", "suggestion"),
        ]
        .into_iter()
        .enumerate()
        {
            let sid = format!("k{i}");
            assert!(mark(&store, &sink, &AtomicBool::new(false), &sid, entry, name, detail));
            let src = source(&store, &sid).unwrap();
            assert_eq!((src["kind"].as_str(), src["name"].as_str(), src["detail"].as_str()), (Some(kind), Some(name), Some(detail)));
        }
        // Trusted content never taints.
        let clean = AtomicBool::new(false);
        for entry in [ContentEntry::UserMessage, ContentEntry::ScheduledPrompt, ContentEntry::ToolOutput { untrusted: false }] {
            assert!(!mark(&store, &sink, &clean, "clean", entry, "x", ""));
        }
        assert!(!clean.load(Ordering::SeqCst) && !is_tainted(&store, "clean"));
    }

    #[test]
    fn old_taint_value_still_counts() {
        let store = Store::in_memory().unwrap();
        let sid = store.create_session("", "user").unwrap();
        store.kv_set(&key(&sid), "1").unwrap();
        // Readers check presence only: a conversation tainted before sources were recorded stays tainted.
        assert!(is_tainted(&store, &sid));
        assert!(crate::agent::Session::load(&store, &sid).unwrap().tainted.load(Ordering::SeqCst));
        assert_eq!(source(&store, &sid), None, "its source reads as null");
        // A new mark neither rewrites the old value nor announces a first taint.
        let (sink, seen) = recording();
        let flag = AtomicBool::new(false);
        assert!(!mark(&store, &sink, &flag, &sid, ContentEntry::ToolOutput { untrusted: true }, "web_fetch", "example.com"));
        assert!(flag.load(Ordering::SeqCst));
        assert_eq!(store.kv_get(&key(&sid)).as_deref(), Some("1"));
        assert!(seen.lock().unwrap().is_empty());
    }
}
