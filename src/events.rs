//! One event bus for every session (M15). Conversations, background runs and Telegram turns all
//! publish here, each event numbered with a `seq` that starts at 1 for each core process. A client
//! that reconnects resumes from the last `seq` it saw; one that asks for more than the ring holds, or
//! falls behind the live channel, gets `resync` and is disconnected.

use crate::agent::Event;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// Events kept for clients that reconnect: a 5-minute disconnect at 30 events per second.
pub const RING_CAPACITY: usize = 10_000;
/// Events a live subscriber may fall behind before it gets `resync` and is dropped.
pub const LIVE_CAPACITY: usize = 1_024;

/// One published event.
#[derive(Debug)]
pub struct BusEvent {
    pub seq: u64,
    pub session_id: String,
    pub run_id: String,
    /// `{"type": …, …fields}`, the shape `/api/chat` has always streamed.
    pub json: Value,
}

impl BusEvent {
    /// The wire form: the event's own fields plus `seq`, `session_id` and `run_id`.
    pub fn envelope(&self) -> Value {
        let mut v = self.json.clone();
        if let Some(o) = v.as_object_mut() {
            o.insert("seq".into(), json!(self.seq));
            o.insert("session_id".into(), json!(self.session_id));
            o.insert("run_id".into(), json!(self.run_id));
        }
        v
    }

    pub fn kind(&self) -> &str {
        self.json.get("type").and_then(Value::as_str).unwrap_or("")
    }
}

/// What a subscription yields.
#[derive(Debug, Clone)]
pub enum Item {
    Event(Arc<BusEvent>),
    /// The client must reload its state; the subscription ends after this.
    Resync { oldest: u64 },
}

impl Item {
    pub fn json(&self) -> Value {
        match self {
            Item::Event(e) => e.envelope(),
            Item::Resync { oldest } => json!({"type": "resync", "oldest": oldest}),
        }
    }
}

struct Shared {
    ring: Mutex<VecDeque<Arc<BusEvent>>>,
    tx: broadcast::Sender<Arc<BusEvent>>,
    next_seq: AtomicU64,
}

impl Shared {
    /// The oldest `seq` a client can still get: the ring's first event, or the next one when the
    /// ring is empty.
    fn oldest(&self, ring: &VecDeque<Arc<BusEvent>>) -> u64 {
        ring.front().map(|e| e.seq).unwrap_or_else(|| self.next_seq.load(Ordering::SeqCst))
    }
}

/// The bus. Cloning shares it.
#[derive(Clone)]
pub struct EventBus(Arc<Shared>);

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(LIVE_CAPACITY);
        EventBus(Arc::new(Shared { ring: Mutex::new(VecDeque::new()), tx, next_seq: AtomicU64::new(1) }))
    }

    /// Publish one event and return its `seq`.
    pub fn publish(&self, session_id: &str, run_id: &str, json: Value) -> u64 {
        // Numbering, the ring and the live channel change together under the ring lock, so every
        // subscriber sees events in `seq` order and none falls between its backlog and its live feed.
        let mut ring = self.0.ring.lock().unwrap_or_else(|e| e.into_inner());
        let seq = self.0.next_seq.fetch_add(1, Ordering::SeqCst);
        let event = Arc::new(BusEvent { seq, session_id: session_id.to_string(), run_id: run_id.to_string(), json });
        ring.push_back(event.clone());
        while ring.len() > RING_CAPACITY {
            ring.pop_front();
        }
        // No subscriber is not an error: the ring keeps the event for whoever comes back.
        let _ = self.0.tx.send(event);
        seq
    }

    /// The last `seq` issued, 0 before the first event.
    pub fn last_seq(&self) -> u64 {
        self.0.next_seq.load(Ordering::SeqCst) - 1
    }

    /// Events after `since` from the ring, then live ones. Without `since`, live events only.
    /// A `since` the ring can no longer serve, or one newer than the last `seq` issued (the client
    /// saw an earlier core process), yields `resync` at once.
    pub fn subscribe(&self, since: Option<u64>) -> Subscription {
        let ring = self.0.ring.lock().unwrap_or_else(|e| e.into_inner());
        let rx = self.0.tx.subscribe();
        let last = self.0.next_seq.load(Ordering::SeqCst) - 1;
        let oldest = self.0.oldest(&ring);
        let mut sub = Subscription { bus: self.clone(), backlog: VecDeque::new(), rx, pending_resync: None, last_yielded: last, done: false };
        if let Some(since) = since {
            if since > last || since.saturating_add(1) < oldest {
                sub.pending_resync = Some(oldest);
            } else {
                sub.backlog = ring.iter().filter(|e| e.seq > since).cloned().collect();
                sub.last_yielded = since;
            }
        }
        sub
    }
}

/// One client's view of the bus.
pub struct Subscription {
    bus: EventBus,
    backlog: VecDeque<Arc<BusEvent>>,
    rx: broadcast::Receiver<Arc<BusEvent>>,
    pending_resync: Option<u64>,
    last_yielded: u64,
    done: bool,
}

impl Subscription {
    /// The next item, or `None` once the subscription has ended (after `resync`).
    pub async fn next(&mut self) -> Option<Item> {
        if self.done {
            return None;
        }
        if let Some(oldest) = self.pending_resync.take() {
            self.done = true;
            return Some(Item::Resync { oldest });
        }
        if let Some(e) = self.backlog.pop_front() {
            self.last_yielded = e.seq;
            return Some(Item::Event(e));
        }
        loop {
            match self.rx.recv().await {
                Ok(e) if e.seq <= self.last_yielded => continue,
                Ok(e) => {
                    self.last_yielded = e.seq;
                    return Some(Item::Event(e));
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    self.done = true;
                    let ring = self.bus.0.ring.lock().unwrap_or_else(|e| e.into_inner());
                    return Some(Item::Resync { oldest: self.bus.0.oldest(&ring) });
                }
                Err(broadcast::error::RecvError::Closed) => {
                    self.done = true;
                    return None;
                }
            }
        }
    }

    /// The subscription as a stream, for SSE.
    pub fn into_stream(self) -> impl futures_util::Stream<Item = Item> + Send {
        futures_util::stream::unfold(self, |mut sub| async move { sub.next().await.map(|item| (item, sub)) })
    }
}

/// A run's event as JSON, in the shape `/api/chat` streams.
pub fn event_json(e: Event) -> Value {
    match e {
        Event::Text(t) => json!({"type": "text", "text": t}),
        Event::ToolStart { id, name, summary, risk } => json!({"type": "tool_start", "id": id, "name": name, "summary": summary, "risk": risk.label()}),
        Event::ToolEnd { id, name, ok, preview } => json!({"type": "tool_end", "id": id, "name": name, "ok": ok, "preview": preview}),
        Event::Notice(t) => json!({"type": "notice", "text": t}),
        Event::Usage(u) => json!({"type": "usage", "prompt": u.prompt_tokens, "completion": u.completion_tokens}),
        Event::Model { name, why } => json!({"type": "model", "name": name, "why": why}),
    }
}

/// The `error` event for a run that failed, with the `code` a client acts on.
pub fn error_json(e: &anyhow::Error) -> Value {
    let mut v = json!({"type": "error", "message": format!("{e:#}"), "code": Value::Null});
    for cause in e.chain() {
        if let Some(u) = cause.downcast_ref::<crate::cli_llm::CliVersionUntested>() {
            v["code"] = json!("cli_version_untested");
            v["provider"] = json!(u.id);
            v["version"] = json!(u.found);
            break;
        }
        if cause.downcast_ref::<crate::risk_notice::RiskNoticeNotAccepted>().is_some() {
            v["code"] = json!("risk_notice_not_accepted");
            break;
        }
        if cause.downcast_ref::<crate::permissions::WorkspaceRefused>().is_some() {
            v["code"] = json!("workspace_refused");
            break;
        }
    }
    v
}

/// A new run id: a UUID, unique across core restarts.
pub fn new_run_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(i: usize) -> Value {
        json!({"type": "text", "text": i.to_string()})
    }

    async fn drain(sub: &mut Subscription, n: usize) -> Vec<Item> {
        let mut out = vec![];
        for _ in 0..n {
            match tokio::time::timeout(std::time::Duration::from_secs(2), sub.next()).await {
                Ok(Some(item)) => out.push(item),
                _ => break,
            }
        }
        out
    }

    fn seqs(items: &[Item]) -> Vec<u64> {
        items.iter().filter_map(|i| if let Item::Event(e) = i { Some(e.seq) } else { None }).collect()
    }

    #[tokio::test]
    async fn bus_resumes_from_sequence() {
        let bus = EventBus::new();
        assert_eq!(bus.last_seq(), 0);
        for i in 0..5 {
            assert_eq!(bus.publish("s", "r", text(i)), i as u64 + 1, "seq starts at 1 and counts up");
        }
        // A client that saw up to 3 gets 4 and 5 from the ring, then live events, in order and once.
        let mut sub = bus.subscribe(Some(3));
        bus.publish("s2", "r2", json!({"type": "done", "text": "ok"}));
        let items = drain(&mut sub, 3).await;
        assert_eq!(seqs(&items), vec![4, 5, 6]);
        let Item::Event(last) = &items[2] else { panic!("{items:?}") };
        let wire = last.envelope();
        assert_eq!((wire["seq"].as_u64(), wire["session_id"].as_str(), wire["run_id"].as_str(), wire["type"].as_str()), (Some(6), Some("s2"), Some("r2"), Some("done")));
        // Nothing more until something is published.
        assert!(tokio::time::timeout(std::time::Duration::from_millis(50), sub.next()).await.is_err());
        // Without `since`, only new events.
        let mut live = bus.subscribe(None);
        bus.publish("s", "r", text(7));
        assert_eq!(seqs(&drain(&mut live, 1).await), vec![7]);
        // A client that has seen everything waits for the next one.
        let mut caught_up = bus.subscribe(Some(7));
        bus.publish("s", "r", text(8));
        assert_eq!(seqs(&drain(&mut caught_up, 1).await), vec![8]);
        // `since = 0` replays the whole ring.
        assert_eq!(seqs(&drain(&mut bus.subscribe(Some(0)), 8).await), (1..=8).collect::<Vec<_>>());
    }

    #[tokio::test]
    async fn bus_resyncs_when_since_is_too_old() {
        let bus = EventBus::new();
        for i in 0..RING_CAPACITY + 5 {
            bus.publish("s", "r", text(i));
        }
        // The ring holds the last 10,000: 6 ..= 10,005.
        let oldest = 6;
        assert_eq!(seqs(&drain(&mut bus.subscribe(Some(oldest - 1)), 1).await), vec![oldest], "the event right after `since` is still there");
        for since in [0, oldest - 2] {
            let mut sub = bus.subscribe(Some(since));
            let items = drain(&mut sub, 3).await;
            assert!(matches!(items.as_slice(), [Item::Resync { oldest: o }] if *o == oldest), "since {since}: {items:?}");
            assert_eq!(items[0].json(), json!({"type": "resync", "oldest": oldest}));
            assert!(sub.next().await.is_none(), "the subscription ends after resync");
        }
        // A `since` newer than any seq issued comes from an earlier core process.
        let mut restarted = bus.subscribe(Some(bus.last_seq() + 1));
        assert!(matches!(drain(&mut restarted, 3).await.as_slice(), [Item::Resync { .. }]));
        let fresh = EventBus::new();
        let mut after_restart = fresh.subscribe(Some(500));
        assert!(matches!(drain(&mut after_restart, 3).await.as_slice(), [Item::Resync { oldest: 1 }]));
    }

    #[tokio::test]
    async fn lagging_subscriber_gets_resync_and_is_dropped() {
        let bus = EventBus::new();
        let mut slow = bus.subscribe(None);
        let mut fast = bus.subscribe(None);
        for i in 0..LIVE_CAPACITY + 10 {
            bus.publish("s", "r", text(i));
            if let Some(Item::Event(e)) = fast.next().await {
                assert_eq!(e.seq, i as u64 + 1);
            } else {
                panic!("a reader that keeps up is never dropped");
            }
        }
        let items = drain(&mut slow, 5).await;
        assert!(matches!(items.as_slice(), [Item::Resync { oldest: 1 }]), "{items:?}");
        assert!(slow.next().await.is_none());
        // Publishing goes on; the dropped subscriber's lag cost nobody else anything.
        bus.publish("s", "r", text(0));
        assert!(matches!(fast.next().await, Some(Item::Event(e)) if e.seq == LIVE_CAPACITY as u64 + 11));
    }

    #[test]
    fn error_events_carry_a_code() {
        let untested: anyhow::Error = crate::cli_llm::CliVersionUntested { id: "codex-cli".into(), found: "9.9.9".into() }.into();
        let v = error_json(&untested.context("the model call failed"));
        assert_eq!((v["code"].as_str(), v["provider"].as_str(), v["version"].as_str()), (Some("cli_version_untested"), Some("codex-cli"), Some("9.9.9")));
        assert_eq!(error_json(&crate::risk_notice::RiskNoticeNotAccepted.into())["code"], "risk_notice_not_accepted");
        assert_eq!(error_json(&crate::permissions::WorkspaceRefused { path: "/x".into() }.into())["code"], "workspace_refused");
        let other = error_json(&anyhow::anyhow!("boom"));
        assert_eq!((other["code"].clone(), other["message"].as_str()), (Value::Null, Some("boom")));
    }
}
