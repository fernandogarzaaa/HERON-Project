//! View layer for the native Heron UI: sidebar, Memory, Tasks, Reminders,
//! Awareness. This file is deliberately self-contained: it defines the data
//! each view needs, the text each view shows, and the operations each view
//! performs, without importing the rest of the crate. The Shell lane owns the
//! bridge that implements [`ViewsBridge`] (HTTP calls to the local server,
//! or direct calls into the app) and the Slint wiring in `views_slint.rs`.
//!
//! JSON shapes here match the server handlers in `src/server.rs` exactly.
//! See `docs/slint-views-map.md` for the full map, including the calls the
//! frontend makes today.

use async_trait::async_trait;
use chrono::{Local, TimeZone};
use serde::Deserialize;

// ---------- errors ----------

/// A bridge failure, shown to the user as-is. The frontend today surfaces
/// `data.error` from `{ "error": "..." }` responses, so plain text is the
/// right shape.
pub type BridgeResult<T> = Result<T, String>;

// ---------- view models: shapes the server returns ----------

/// `GET /sessions` item. The sidebar keeps only entries with `messages > 0`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub messages: i64,
}

/// The counts and flags the sidebar badges and footer read from `GET /state`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AppState {
    pub model: String,
    #[serde(default)]
    pub demo: bool,
    #[serde(default)]
    pub unread: u64,
    #[serde(default)]
    pub suggestions: u64,
    #[serde(default)]
    pub waiting: u64,
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub usage: UsageTokens,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct UsageTokens {
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
}

/// `GET /memories` item. Search results add `score`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct MemoryItem {
    pub id: i64,
    pub kind: String,
    pub content: String,
    #[serde(default)]
    pub tags: String,
    #[serde(default)]
    pub importance: f64,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub uses: i64,
    pub source: String,
    #[serde(default)]
    pub confirmed: bool,
    #[serde(default)]
    pub score: Option<f64>,
}

/// `GET /tasks` item.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TaskItem {
    pub id: i64,
    pub title: String,
    #[serde(default)]
    pub notes: String,
    pub status: String,
    #[serde(default)]
    pub priority: i64,
    #[serde(default)]
    pub due: Option<String>,
    pub created_at: i64,
    #[serde(default)]
    pub owner: String,
}

/// `GET /schedules` item, with the three computed fields the server adds.
/// The raw `workflow` string is stripped by the server.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ScheduleItem {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub cron: String,
    pub prompt: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub next_run: Option<i64>,
    #[serde(default)]
    pub last_run: Option<i64>,
    #[serde(default)]
    pub last_result: Option<String>,
    #[serde(default)]
    pub allow: String,
    #[serde(default)]
    pub quiet: bool,
    #[serde(default)]
    pub last_tokens: i64,
    #[serde(default)]
    pub trigger: Option<String>,
    #[serde(default)]
    pub outline: Option<String>,
    #[serde(default)]
    pub uses_model: Option<bool>,
}

/// One run of a schedule, from `GET /schedules/{id}/runs`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ScheduleRun {
    pub ts: i64,
    pub status: String,
    #[serde(default)]
    pub tokens: i64,
    #[serde(default)]
    pub record: ScheduleRunRecord,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ScheduleRunRecord {
    #[serde(default)]
    pub steps: Vec<RunStep>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RunStep {
    #[serde(default)]
    pub what: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub output: Option<String>,
}

/// `GET /inbox` item.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct InboxItem {
    pub id: i64,
    pub ts: i64,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub read: bool,
}

/// `GET /awareness` object.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Awareness {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub reflect: bool,
    #[serde(default)]
    pub proactive: String,
    #[serde(default)]
    pub budget_used: u64,
    #[serde(default)]
    pub budget_limit: u64,
    #[serde(default)]
    pub demo: bool,
    #[serde(default)]
    pub focus: Vec<(String, i64)>,
    #[serde(default)]
    pub active_hours: Option<(i64, i64)>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub journal: Vec<JournalEntry>,
    #[serde(default)]
    pub insights: Vec<MemoryItem>,
    #[serde(default)]
    pub workspace: String,
}

/// One journal entry inside `GET /awareness`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct JournalEntry {
    pub ts: i64,
    pub path: String,
}

/// `GET /suggestions` item.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Suggestion {
    pub id: i64,
    pub ts: i64,
    pub title: String,
    #[serde(default)]
    pub why: String,
    pub prompt: String,
    pub status: String,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
}

/// `GET /approvals` item: work Heron parked while nobody was there to approve it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ParkedApproval {
    pub id: i64,
    pub ts: i64,
    pub session_id: String,
    pub tool: String,
    #[serde(default)]
    pub args: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub risk: String,
    #[serde(default)]
    pub tainted: bool,
    #[serde(default)]
    pub origin: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub result: Option<String>,
}

// ---------- formatting: ports of the frontend helpers ----------

/// Unix seconds to the frontend's format: "Nov 13, 6:46 PM".
pub fn fmt_time(ts: i64) -> String {
    match Local.timestamp_opt(ts, 0).single() {
        Some(dt) => dt.format("%b %-d, %-I:%M %p").to_string(),
        None => String::new(),
    }
}

/// A cron expression in words. Port of `cronText` in `assets/index.html`.
/// Anything unusual is shown as it is: `Repeats (0 8 * * *)`.
pub fn cron_text(cron: &str) -> String {
    let f: Vec<&str> = cron.trim().split_whitespace().collect();
    if f.len() != 5 {
        return format!("Repeats ({cron})");
    }
    let [m, h, dom, mon, dow] = [f[0], f[1], f[2], f[3], f[4]];
    let num = |x: &str| x.chars().all(|c| c.is_ascii_digit());
    let at = || {
        let hh: u32 = h.parse().unwrap_or(0);
        let mm: u32 = m.parse().unwrap_or(0);
        let hour12 = match hh % 12 {
            0 => 12,
            v => v,
        };
        let ampm = if hh < 12 { "AM" } else { "PM" };
        format!("{hour12}:{mm:02} {ampm}")
    };
    let days = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    if mon != "*" {
        return format!("Repeats ({cron})");
    }
    if num(m) && num(h) && dom == "*" {
        if dow == "*" {
            return format!("Every day at {}", at());
        }
        if dow == "1-5" {
            return format!("Every weekday at {}", at());
        }
        if dow == "0,6" || dow == "6,0" {
            return format!("Every Saturday and Sunday at {}", at());
        }
        if let Ok(d) = dow.parse::<usize>() {
            if let Some(day) = days.get(d % 7) {
                return format!("Every {day} at {}", at());
            }
        }
    }
    if num(m) && num(h) && num(dom) && dow == "*" {
        return format!("On day {dom} of every month at {}", at());
    }
    if dom == "*" && dow == "*" {
        if let Some(k) = m
            .strip_prefix("*/")
            .filter(|k| k.chars().all(|c| c.is_ascii_digit()))
        {
            if h == "*" {
                return format!("Every {k} minutes");
            }
        }
        if m == "0" {
            if let Some(k) = h
                .strip_prefix("*/")
                .filter(|k| k.chars().all(|c| c.is_ascii_digit()))
            {
                return format!("Every {k} hours");
            }
        }
        if num(m) && h == "*" {
            let mm: u32 = m.parse().unwrap_or(0);
            return format!("Every hour at {mm:02} minutes past");
        }
    }
    format!("Repeats ({cron})")
}

/// What a schedule may do, in words. Port of `mayText` in the frontend.
/// `allow` is the granted permission string, empty for read only.
pub fn may_text(allow: &str) -> &'static str {
    if allow.is_empty() {
        "read only"
    } else if allow.contains("shell") {
        "may edit files and run commands"
    } else {
        "may edit files"
    }
}

/// Token counts the way the Usage view shortens them: 1.2M, 45k, 1,240.
pub fn tok(n: u64) -> String {
    if n >= 1_000_000 {
        let v = n as f64 / 1_000_000.0;
        let s = format!("{v:.1}");
        format!("{}M", s.trim_end_matches(".0"))
    } else if n >= 10_000 {
        format!("{}k", n / 1_000)
    } else {
        format!("{n}")
    }
}

/// Memory kind labels, as the Memory view shows them.
pub fn kind_label(kind: &str) -> &'static str {
    match kind {
        "profile" => "About me",
        "preference" => "Preference",
        "workflow" => "How I like things done",
        "project" => "Project",
        "person" => "Person",
        _ => "Fact",
    }
}

/// Where a confirmed memory came from, as the Memory view shows it.
pub fn confirmed_source_label(source: &str) -> &'static str {
    match source {
        "user" => "You added this",
        "onboarding" => "You told Heron while getting set up",
        "agent" => "Heron saved this during a conversation",
        "auto" => "Heron picked this up from a conversation",
        _ => "Saved",
    }
}

/// Where an unconfirmed memory came from, as the Memory view shows it.
pub fn unconfirmed_origin_label(source: &str) -> &'static str {
    match source {
        "email" => "your email",
        "calendar" => "your calendar",
        "documents" => "your documents",
        "observed" => "watching your folder",
        "computer" => "looking around this computer",
        _ => "outside",
    }
}

/// The meta line under a memory row. Mirrors `loadMemory` exactly,
/// including the "not used in over a month" rule.
pub fn memory_meta(item: &MemoryItem, now_ts: i64) -> String {
    if item.confirmed {
        let mut meta = format!(
            "{}, {}",
            confirmed_source_label(&item.source),
            fmt_time(item.updated_at)
        );
        if item.uses == 0 && now_ts - item.updated_at > 30 * 86_400 {
            meta.push_str(". Not used in over a month");
        }
        meta
    } else {
        format!(
            "Picked up from {}, not confirmed by you. Heron treats it as possibly wrong.",
            unconfirmed_origin_label(&item.source)
        )
    }
}

/// The meta line under a task row: owner badge, due date, notes, joined
/// with " — ". Empty when there is nothing to say.
pub fn task_meta(item: &TaskItem) -> String {
    let mut parts: Vec<String> = Vec::new();
    if item.owner == "heron" {
        parts.push("Heron is handling this".to_string());
    }
    if let Some(due) = &item.due {
        if !due.is_empty() {
            parts.push(format!("Due {due}"));
        }
    }
    if !item.notes.trim().is_empty() {
        parts.push(item.notes.clone());
    }
    parts.join(" — ")
}

/// The meta line under a schedule row. Mirrors `loadReminders`.
pub fn schedule_meta(item: &ScheduleItem) -> String {
    let mut meta = if let Some(trigger) = &item.trigger {
        trigger.clone()
    } else if let Some(next) = item.next_run {
        let when = if item.cron.is_empty() {
            format!("Once, {}", fmt_time(next))
        } else {
            format!("{}. Next: {}", cron_text(&item.cron), fmt_time(next))
        };
        when
    } else if item.cron.is_empty() {
        "Once".to_string()
    } else {
        cron_text(&item.cron)
    };
    meta.push_str(&format!(" — {}", may_text(&item.allow)));
    if item.quiet {
        meta.push_str(", speaks up only when needed");
    }
    if let Some(last) = item.last_run {
        meta.push_str(&format!(". Last ran {}", fmt_time(last)));
        if item.last_tokens != 0 {
            meta.push_str(&format!(", used {} tokens", item.last_tokens));
        }
        if item.quiet {
            if let Some(result) = &item.last_result {
                meta.push_str(&format!(": {result}"));
            }
        }
    }
    meta
}

/// The title a schedule row shows: the name for automations, the prompt
/// for plain jobs.
pub fn schedule_title(item: &ScheduleItem) -> &str {
    if item.outline.is_some() {
        &item.name
    } else {
        &item.prompt
    }
}

/// The kind tag an automation row shows, if it is an automation.
pub fn schedule_kind_tag(item: &ScheduleItem) -> Option<&'static str> {
    if item.outline.is_some() {
        Some(if item.uses_model.unwrap_or(false) {
            "Automation, uses the AI model"
        } else {
            "Automation, no AI model needed"
        })
    } else {
        None
    }
}

/// The title prefix a suggestion shows, by status.
pub fn suggestion_title(item: &Suggestion) -> String {
    match item.status.as_str() {
        "done" => format!("Done for you: {}", item.title),
        "prepared" => format!("Prepared: {}", item.title),
        "failed" => format!("Could not finish: {}", item.title),
        _ => item.title.clone(),
    }
}

/// The footer line in the sidebar: model and session token count.
pub fn sidebar_footer(state: &AppState) -> String {
    let used = state.usage.prompt_tokens + state.usage.completion_tokens;
    let model = if state.demo {
        "Demo mode".to_string()
    } else {
        state.model.clone()
    };
    if used > 0 {
        format!("{model} — {} tokens this session", used)
    } else {
        model
    }
}

// ---------- the bridge: what the views need from the Shell lane ----------

/// What each view sends and receives, as operations. The Shell lane
/// implements this against the local HTTP server (the frontend's calls,
/// mapped in `docs/slint-views-map.md`) or against the app directly.
/// The views never see HTTP, tokens, or URLs.
#[async_trait]
pub trait ViewsBridge: Send + Sync {
    // Sidebar
    async fn get_state(&self) -> BridgeResult<AppState>;
    async fn list_sessions(&self) -> BridgeResult<Vec<SessionInfo>>;
    async fn delete_session(&self, id: &str) -> BridgeResult<()>;

    // Memory
    async fn list_memories(&self, query: Option<&str>) -> BridgeResult<Vec<MemoryItem>>;
    async fn add_memory(&self, content: &str, kind: &str) -> BridgeResult<i64>;
    async fn delete_memory(&self, id: i64) -> BridgeResult<()>;
    async fn confirm_memory(&self, id: i64) -> BridgeResult<()>;

    // Tasks
    async fn list_tasks(&self) -> BridgeResult<Vec<TaskItem>>;
    async fn add_task(&self, title: &str) -> BridgeResult<i64>;
    async fn set_task_status(&self, id: i64, status: &str) -> BridgeResult<()>;
    async fn delete_task(&self, id: i64) -> BridgeResult<()>;

    // Reminders
    async fn list_schedules(&self) -> BridgeResult<Vec<ScheduleItem>>;
    async fn add_schedule(
        &self,
        prompt: &str,
        when: &str,
        may: &str,
        quiet: bool,
    ) -> BridgeResult<i64>;
    async fn set_schedule_may(&self, id: i64, may: &str) -> BridgeResult<()>;
    async fn run_schedule_now(&self, id: i64) -> BridgeResult<()>;
    async fn schedule_runs(&self, id: i64) -> BridgeResult<Vec<ScheduleRun>>;
    async fn delete_schedule(&self, id: i64) -> BridgeResult<()>;
    async fn get_inbox(&self) -> BridgeResult<Vec<InboxItem>>;
    async fn read_inbox(&self) -> BridgeResult<()>;

    // Awareness
    async fn get_awareness(&self) -> BridgeResult<Awareness>;
    async fn set_awareness_enabled(&self, enabled: bool) -> BridgeResult<Awareness>;
    async fn set_awareness_reflect(&self, reflect: bool) -> BridgeResult<Awareness>;
    async fn set_awareness_proactive(&self, level: &str) -> BridgeResult<Awareness>;
    async fn clear_awareness(&self) -> BridgeResult<i64>;
    async fn set_paused(&self, paused: bool) -> BridgeResult<bool>;
    async fn list_suggestions(&self) -> BridgeResult<Vec<Suggestion>>;
    async fn answer_suggestion(
        &self,
        id: i64,
        action: &str,
    ) -> BridgeResult<(String, Option<String>)>;
    async fn list_approvals(&self) -> BridgeResult<Vec<ParkedApproval>>;
    async fn answer_approval(&self, id: i64, action: &str) -> BridgeResult<Option<String>>;
}

// ---------- view state: what the components show ----------

/// Everything the five views display, loaded through the bridge.
/// The Shell refreshes each view on navigation and on the 30-second tick
/// for the sidebar state. `now_ts` is Unix seconds, used by `memory_meta`.
#[derive(Default)]
pub struct ViewsState {
    pub state: Option<AppState>,
    pub sessions: Vec<SessionInfo>,
    pub current_session: Option<String>,
    pub memories: Vec<MemoryItem>,
    pub memory_query: String,
    pub tasks: Vec<TaskItem>,
    /// Only enabled schedules, as the Reminders view shows them.
    pub schedules: Vec<ScheduleItem>,
    pub inbox: Vec<InboxItem>,
    pub awareness: Option<Awareness>,
    pub suggestions: Vec<Suggestion>,
    pub approvals: Vec<ParkedApproval>,
}

impl ViewsState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sidebar badge counts: (unread reminders, open suggestions, waiting approvals).
    pub fn badges(&self) -> (u64, u64, u64) {
        match &self.state {
            Some(s) => (s.unread, s.suggestions, s.waiting),
            None => (0, 0, 0),
        }
    }

    /// Sidebar footer text, or empty before the first state load.
    pub fn footer(&self) -> String {
        self.state.as_ref().map(sidebar_footer).unwrap_or_default()
    }

    /// Sessions with at least one message, newest first, as the sidebar shows them.
    pub fn visible_sessions(&self) -> Vec<&SessionInfo> {
        self.sessions.iter().filter(|s| s.messages > 0).collect()
    }

    pub async fn refresh_state(&mut self, bridge: &dyn ViewsBridge) -> BridgeResult<()> {
        self.state = Some(bridge.get_state().await?);
        Ok(())
    }

    pub async fn refresh_sessions(&mut self, bridge: &dyn ViewsBridge) -> BridgeResult<()> {
        self.sessions = bridge.list_sessions().await?;
        Ok(())
    }

    pub async fn refresh_memories(&mut self, bridge: &dyn ViewsBridge) -> BridgeResult<()> {
        let q = if self.memory_query.trim().is_empty() {
            None
        } else {
            Some(self.memory_query.as_str())
        };
        self.memories = bridge.list_memories(q).await?;
        Ok(())
    }

    pub async fn refresh_tasks(&mut self, bridge: &dyn ViewsBridge) -> BridgeResult<()> {
        self.tasks = bridge.list_tasks().await?;
        Ok(())
    }

    pub async fn refresh_reminders(&mut self, bridge: &dyn ViewsBridge) -> BridgeResult<()> {
        let all = bridge.list_schedules().await?;
        self.schedules = all.into_iter().filter(|s| s.enabled).collect();
        self.inbox = bridge.get_inbox().await?;
        Ok(())
    }

    pub async fn refresh_awareness(&mut self, bridge: &dyn ViewsBridge) -> BridgeResult<()> {
        self.awareness = Some(bridge.get_awareness().await?);
        self.suggestions = bridge.list_suggestions().await?;
        Ok(())
    }

    pub async fn refresh_approvals(&mut self, bridge: &dyn ViewsBridge) -> BridgeResult<()> {
        self.approvals = bridge.list_approvals().await?;
        Ok(())
    }

    /// Mark delivered results read, as the Reminders view does on load when
    /// any item is unread. Returns true when a mark-read call was made.
    pub async fn mark_inbox_read_if_needed(&self, bridge: &dyn ViewsBridge) -> BridgeResult<bool> {
        if self.inbox.iter().any(|m| !m.read) {
            bridge.read_inbox().await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// The "Right now" rows of the Awareness view, as (text, meta) pairs.
    pub fn right_now_rows(&self) -> Vec<(String, String)> {
        let Some(a) = &self.awareness else {
            return Vec::new();
        };
        if !a.enabled {
            return Vec::new();
        }
        let mut rows = Vec::new();
        if a.focus.is_empty() {
            rows.push((
                "Your focus this week".to_string(),
                "Nothing has changed in the folder yet.".to_string(),
            ));
        } else {
            let parts: Vec<String> = a
                .focus
                .iter()
                .map(|(area, n)| format!("{area} ({n} change{})", if *n == 1 { "" } else { "s" }))
                .collect();
            rows.push(("Your focus this week".to_string(), parts.join(", ")));
        }
        if let Some(branch) = &a.branch {
            if !branch.is_empty() {
                rows.push(("Git branch".to_string(), branch.clone()));
            }
        }
        let hours = match a.active_hours {
            Some((start, end)) => format!("Mostly active between {start}:00 and {end}:00"),
            None => "Not enough history yet. Heron needs a few days before it says anything about your habits.".to_string(),
        };
        rows.push(("Your usual hours".to_string(), hours));
        rows
    }

    /// The note under the proactivity radios. Mirrors `renderAwareness`.
    pub fn proactive_note(&self) -> String {
        let Some(a) = &self.awareness else {
            return String::new();
        };
        if a.demo && a.proactive != "off" {
            return "Demo mode cannot come up with suggestions. Connect a model in Settings."
                .to_string();
        }
        let mut note = "Suggesting, preparing and taking over send what Heron noticed (file names, your tasks and what it knows about you) to your AI model about every half hour while there is new activity, which uses tokens.".to_string();
        if a.budget_limit > 0 && a.proactive != "off" {
            note.push_str(&format!(
                " Today Heron has used {} of its {} token allowance for work it starts itself",
                a.budget_used, a.budget_limit
            ));
            if a.budget_used >= a.budget_limit {
                note.push_str(", so it is paused until tomorrow");
            }
            note.push('.');
        }
        note
    }
}

// ---------- tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    fn memory_json() -> &'static str {
        r#"{"id":7,"kind":"fact","content":"I am vegetarian","tags":"","importance":0.7,"created_at":1763000000,"updated_at":1763000000,"uses":3,"source":"user","confirmed":true}"#
    }

    #[test]
    fn memory_item_shape_matches_the_server() {
        let m: MemoryItem = serde_json::from_str(memory_json()).unwrap();
        assert_eq!(m.id, 7);
        assert_eq!(m.kind, "fact");
        assert_eq!(m.source, "user");
        assert!(m.confirmed);
        assert_eq!(m.score, None);
    }

    #[test]
    fn search_result_carries_a_score() {
        let m: MemoryItem =
            serde_json::from_str(r#"{"id":7,"kind":"fact","content":"x","created_at":1,"updated_at":1,"source":"email","confirmed":false,"score":0.93}"#)
                .unwrap();
        assert_eq!(m.score, Some(0.93));
        assert!(!m.confirmed);
    }

    #[test]
    fn session_shape_matches_the_server() {
        let s: SessionInfo = serde_json::from_str(
            r#"{"id":"01K","title":"Plan","created_at":1,"updated_at":2,"messages":12}"#,
        )
        .unwrap();
        assert_eq!(s.messages, 12);
    }

    #[test]
    fn schedule_shape_matches_the_server() {
        let s: ScheduleItem = serde_json::from_str(
            r#"{"id":5,"name":"Briefing","cron":"0 8 * * *","prompt":"Brief me","enabled":true,"next_run":1763085600,"last_run":1762999200,"last_result":"Done","allow":"","quiet":false,"last_tokens":1240,"trigger":null,"outline":null,"uses_model":null}"#,
        )
        .unwrap();
        assert_eq!(s.cron, "0 8 * * *");
        assert_eq!(s.next_run, Some(1763085600));
        assert_eq!(s.trigger, None);
    }

    #[test]
    fn trigger_schedule_has_no_cron() {
        let s: ScheduleItem = serde_json::from_str(
            r#"{"id":6,"name":"Watch","cron":"","prompt":"Watch mail","enabled":true,"allow":"","quiet":true,"last_tokens":0,"trigger":"When a new email arrives"}"#,
        )
        .unwrap();
        assert_eq!(s.trigger.as_deref(), Some("When a new email arrives"));
        assert!(s.cron.is_empty());
    }

    #[test]
    fn awareness_shape_matches_the_server() {
        let a: Awareness = serde_json::from_str(
            r#"{"enabled":true,"reflect":true,"proactive":"suggest","budget_used":1200,"budget_limit":5000,"demo":false,"focus":[["src",14]],"active_hours":[9,18],"branch":"main","journal":[{"ts":1,"path":"src/server.rs"}],"insights":[],"workspace":"/home/fernando/work"}"#,
        )
        .unwrap();
        assert_eq!(a.focus, vec![("src".to_string(), 14)]);
        assert_eq!(a.active_hours, Some((9, 18)));
        assert_eq!(a.journal.len(), 1);
    }

    #[test]
    fn cron_text_covers_the_common_shapes() {
        assert_eq!(cron_text("0 9 * * *"), "Every day at 9:00 AM");
        assert_eq!(cron_text("0 8 * * 1-5"), "Every weekday at 8:00 AM");
        assert_eq!(
            cron_text("0 10 * * 0,6"),
            "Every Saturday and Sunday at 10:00 AM"
        );
        assert_eq!(cron_text("0 9 * * 1"), "Every Monday at 9:00 AM");
        assert_eq!(
            cron_text("0 9 15 * *"),
            "On day 15 of every month at 9:00 AM"
        );
        assert_eq!(cron_text("*/30 * * * *"), "Every 30 minutes");
        assert_eq!(cron_text("0 */2 * * *"), "Every 2 hours");
        assert_eq!(cron_text("15 * * * *"), "Every hour at 15 minutes past");
        assert_eq!(cron_text("nonsense"), "Repeats (nonsense)");
        assert_eq!(cron_text("0 8 * 3 *"), "Repeats (0 8 * 3 *)");
    }

    #[test]
    fn may_text_matches_the_allow_strings() {
        assert_eq!(may_text(""), "read only");
        assert_eq!(may_text("write_file:*\nedit_file:*"), "may edit files");
        assert_eq!(
            may_text("write_file:*\nedit_file:*\nshell:*"),
            "may edit files and run commands"
        );
    }

    #[test]
    fn memory_meta_confirmed_and_stale() {
        let mut m: MemoryItem = serde_json::from_str(memory_json()).unwrap();
        m.updated_at = 1794536000;
        assert_eq!(
            memory_meta(&m, 1794536000),
            "You added this, Nov 13, 2:13 AM"
        );
        m.uses = 0;
        m.updated_at = 1794536000 - 31 * 86_400;
        assert!(memory_meta(&m, 1794536000).ends_with(". Not used in over a month"));
        m.confirmed = false;
        m.source = "email".to_string();
        assert!(memory_meta(&m, 1794536000).starts_with("Picked up from your email"));
    }

    #[test]
    fn task_meta_joins_the_parts() {
        let t = TaskItem {
            id: 1,
            title: "Call".into(),
            notes: "Ask for Maria".into(),
            status: "open".into(),
            priority: 2,
            due: Some("2026-11-14".into()),
            created_at: 1,
            owner: "heron".into(),
        };
        assert_eq!(
            task_meta(&t),
            "Heron is handling this — Due 2026-11-14 — Ask for Maria"
        );
        let plain = TaskItem {
            owner: "user".into(),
            notes: String::new(),
            due: None,
            ..t
        };
        assert_eq!(task_meta(&plain), "");
    }

    #[test]
    fn schedule_meta_for_a_daily_job() {
        let s = ScheduleItem {
            id: 5,
            name: "Briefing".into(),
            cron: "0 8 * * *".into(),
            prompt: "Brief me".into(),
            enabled: true,
            next_run: Some(1794643200),
            last_run: Some(1794556800),
            last_result: Some("Done".into()),
            allow: String::new(),
            quiet: false,
            last_tokens: 1240,
            trigger: None,
            outline: None,
            uses_model: None,
        };
        assert_eq!(
            schedule_meta(&s),
            "Every day at 8:00 AM. Next: Nov 14, 8:00 AM — read only. Last ran Nov 13, 8:00 AM, used 1240 tokens"
        );
        assert_eq!(schedule_title(&s), "Brief me");
        assert_eq!(schedule_kind_tag(&s), None);
    }

    #[test]
    fn schedule_meta_for_a_trigger_job() {
        let s = ScheduleItem {
            id: 6,
            name: "Watch".into(),
            cron: String::new(),
            prompt: "Watch mail".into(),
            enabled: true,
            next_run: None,
            last_run: None,
            last_result: None,
            allow: "write_file:*\nedit_file:*".into(),
            quiet: true,
            last_tokens: 0,
            trigger: Some("When a new email arrives".into()),
            outline: None,
            uses_model: None,
        };
        assert_eq!(
            schedule_meta(&s),
            "When a new email arrives — may edit files, speaks up only when needed"
        );
    }

    #[test]
    fn tok_shortens_like_the_usage_view() {
        assert_eq!(tok(1240), "1240");
        assert_eq!(tok(45_000), "45k");
        assert_eq!(tok(1_240_000), "1.2M");
        assert_eq!(tok(10_000_000), "10M");
    }

    #[test]
    fn sidebar_footer_matches_the_frontend() {
        let s = AppState {
            model: "claude-sonnet".into(),
            demo: false,
            unread: 0,
            suggestions: 0,
            waiting: 0,
            paused: false,
            usage: UsageTokens {
                prompt_tokens: 1200,
                completion_tokens: 800,
            },
        };
        assert_eq!(
            sidebar_footer(&s),
            "claude-sonnet — 2000 tokens this session"
        );
        let demo = AppState {
            model: "x".into(),
            demo: true,
            usage: UsageTokens::default(),
            ..s
        };
        assert_eq!(sidebar_footer(&demo), "Demo mode");
    }

    #[test]
    fn suggestion_titles_by_status() {
        let base = Suggestion {
            id: 1,
            ts: 1,
            title: "Draft it".into(),
            why: String::new(),
            prompt: "do it".into(),
            status: "new".into(),
            result: None,
            session_id: None,
        };
        assert_eq!(suggestion_title(&base), "Draft it");
        assert_eq!(
            suggestion_title(&Suggestion {
                status: "done".into(),
                ..base.clone()
            }),
            "Done for you: Draft it"
        );
        assert_eq!(
            suggestion_title(&Suggestion {
                status: "prepared".into(),
                ..base.clone()
            }),
            "Prepared: Draft it"
        );
        assert_eq!(
            suggestion_title(&Suggestion {
                status: "failed".into(),
                ..base
            }),
            "Could not finish: Draft it"
        );
    }
}
