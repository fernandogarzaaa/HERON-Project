//! Slint glue for the Views lane: sidebar, Memory, Tasks, Reminders, Awareness.
//!
//! This file drives the MainWindow's forwarded API. It never touches the
//! view components directly. The Slint compiler generates Rust code only
//! for the last exported component of a file, so the five view components
//! have no Rust types of their own. The Shell's `ui/main.slint` instantiates
//! them and forwards every property and callback. The forwarded API is
//! specified in `docs/slint-views-bridge.md`. The harness test Window is
//! the executable form of that spec.
//!
//! Placement: this file belongs in the desktop binary crate
//! (`src/bin/desktop/`). It refers to the Window as `crate::MainWindow`.
//! `slint::include_modules!()` includes the generated code at the call
//! site, so the Window and row structs land directly at the crate root.
//! The binary crate's root must call it once. This file must NOT call it
//! itself: a second call would define the same types a second time in a
//! different module, and the two `MainWindow` types would not match.
//! The logic it drives lives in `super::views` (`src/ui/views.rs`). If
//! the Shell keeps that file in the library instead of the binary crate,
//! the one import line below changes to the library path. Nothing else
//! changes.
//!
//! Threading: the Slint event loop owns the main thread. Bridge calls run
//! on the backend tokio runtime via the `Handle` the Shell passes in. UI
//! updates come back through `slint::invoke_from_event_loop`. Nothing here
//! ever blocks the UI thread.
//!
//! Id casts: API ids are i64. Slint `int` is i32. The glue casts with `as`.
//! Heron ids fit in i32 in practice. A cast that overflows would corrupt
//! the row identity. This is an open item, not a silent assumption.
//!
//! Missing confirms: the web UI confirms before "take over" and before
//! "Forget everything observed". Slint has no native confirm dialog, so
//! this glue posts directly for now. The Shell must add a confirm step
//! before those two calls reach the bridge. See `docs/slint-views-bridge.md`.

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel, Weak};

use super::views as logic;
use crate::{
    AuditRow, InboxRow, MainWindow, MemoryRow, ModelRow, RunStepRow, ScheduleRow, SessionRow,
    SimpleRow, SuggestionRow, TaskRow, UsageDayRow,
};

/// How long the Memory search waits after the last keystroke before
/// reloading. The web UI reloads on every keystroke. The debounce keeps
/// fast typing from firing a request per letter.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(300);

/// Run-now poll: 60 polls of 1.5 seconds, the same 90-second window the
/// web UI uses before it gives up and reloads anyway.
const RUN_POLL_COUNT: usize = 60;
const RUN_POLL_INTERVAL: Duration = Duration::from_millis(1500);

/// Everything the five views need to talk to the bridge and to the Shell.
/// Held as `Arc` so spawned tokio tasks and Slint callbacks share it.
/// All fields are `Send + Sync`.
pub struct ViewsController {
    bridge: Arc<dyn logic::ViewsBridge>,
    state: Arc<Mutex<logic::ViewsState>>,
    rt: tokio::runtime::Handle,
    search_seq: AtomicU64,
    window: Mutex<Option<Weak<MainWindow>>>,
    /// Shell-provided: start a conversation with the accepted suggestion.
    /// Called with the prompt the server returned, and the session id when
    /// the server started one.
    pub on_accept_suggestion: Arc<dyn Fn(String, Option<String>) + Send + Sync>,
    /// Shell-provided: open the session by id.
    pub on_open_session: Arc<dyn Fn(String) + Send + Sync>,
    /// Shell-provided: switch the visible view ("chat", "memory", ...).
    pub on_navigate: Arc<dyn Fn(String) + Send + Sync>,
    /// Shell-provided: start a new conversation.
    pub on_new_conversation: Arc<dyn Fn() + Send + Sync>,
}

impl ViewsController {
    pub fn new(
        bridge: Arc<dyn logic::ViewsBridge>,
        rt: tokio::runtime::Handle,
        on_accept_suggestion: Arc<dyn Fn(String, Option<String>) + Send + Sync>,
        on_open_session: Arc<dyn Fn(String) + Send + Sync>,
        on_navigate: Arc<dyn Fn(String) + Send + Sync>,
        on_new_conversation: Arc<dyn Fn() + Send + Sync>,
    ) -> Arc<Self> {
        Arc::new(Self {
            bridge,
            state: Arc::new(Mutex::new(logic::ViewsState::new())),
            rt,
            search_seq: AtomicU64::new(0),
            window: Mutex::new(None),
            on_accept_suggestion,
            on_open_session,
            on_navigate,
            on_new_conversation,
        })
    }

    /// The Shell calls this when the Chat lane opens a session, so the
    /// sidebar highlights the right row.
    pub fn set_current_session(self: &Arc<Self>, id: Option<String>) {
        {
            let mut st = self.state.lock().unwrap();
            st.current_session = id;
        }
        self.refresh_sidebar_state();
    }

    /// Reload the sidebar badges, footer, and session list. The Shell
    /// calls this on its 30-second tick and after any action that changes
    /// state (memory add, suggestion answer, run-now, mark-read).
    pub fn refresh_sidebar_state(self: &Arc<Self>) {
        let ctrl = self.clone();
        let weak = self.window.lock().unwrap().clone();
        let Some(weak) = weak else {
            return;
        };
        let rt = self.rt.clone();
        rt.spawn(async move {
            let (state_res, sessions_res) =
                tokio::join!(ctrl.bridge.get_state(), ctrl.bridge.list_sessions());
            let Ok(state) = state_res else { return };
            let Ok(sessions) = sessions_res else { return };
            let (unread, suggestions, waiting) = {
                let mut st = ctrl.state.lock().unwrap();
                st.state = Some(state.clone());
                st.sessions = sessions.clone();
                st.badges()
            };
            let footer = logic::sidebar_footer(&state);
            let current = ctrl.state.lock().unwrap().current_session.clone();
            let rows: Vec<SessionRow> = sessions
                .iter()
                .filter(|s| s.messages > 0)
                .map(|s| session_row(s, current.as_deref()))
                .collect();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(window) = weak.upgrade() {
                    window.set_sidebar_unread_count(unread as i32);
                    window.set_sidebar_suggestions_count(suggestions as i32);
                    window.set_sidebar_waiting_count(waiting as i32);
                    window.set_sidebar_footer_text(footer.into());
                    window.set_sidebar_sessions(model_of(rows));
                }
            });
        });
    }
}

// ---------- row converters: view models to .slint structs ----------

fn session_row(s: &logic::SessionInfo, current: Option<&str>) -> SessionRow {
    SessionRow {
        id: s.id.clone().into(),
        title: s.title.clone().into(),
        updated: logic::fmt_time(s.updated_at).into(),
        messages: s.messages as i32,
        current: Some(s.id.as_str()) == current,
    }
}

fn memory_row(m: &logic::MemoryItem, now_ts: i64) -> MemoryRow {
    MemoryRow {
        id: m.id as i32,
        kind_label: logic::kind_label(&m.kind).into(),
        content: m.content.clone().into(),
        meta: logic::memory_meta(m, now_ts).into(),
        confirmed: m.confirmed,
    }
}

fn task_row(t: &logic::TaskItem) -> TaskRow {
    TaskRow {
        id: t.id as i32,
        title: t.title.clone().into(),
        meta: logic::task_meta(t).into(),
        done: t.status == "done",
    }
}

/// The `may` value the row dropdown shows: "read", "write", or "commands".
/// Mirrors the web UI's pre-selection from `allow`.
fn may_value(allow: &str) -> &'static str {
    if allow.is_empty() {
        "read"
    } else if allow.contains("shell") {
        "commands"
    } else {
        "write"
    }
}

fn schedule_row(s: &logic::ScheduleItem) -> ScheduleRow {
    ScheduleRow {
        id: s.id as i32,
        title: logic::schedule_title(s).into(),
        meta: logic::schedule_meta(s).into(),
        outline: s.outline.clone().unwrap_or_default().into(),
        has_outline: s.outline.is_some(),
        has_last_run: s.last_run.is_some(),
        uses_model: s.uses_model.unwrap_or(false),
        may: may_value(&s.allow).into(),
    }
}

fn inbox_row(m: &logic::InboxItem) -> InboxRow {
    InboxRow {
        title: m.title.clone().into(),
        body: slint::StyledText::from_markdown(&m.body).unwrap_or_default(),
        meta: logic::fmt_time(m.ts).into(),
    }
}

fn suggestion_row(s: &logic::Suggestion) -> SuggestionRow {
    SuggestionRow {
        id: s.id as i32,
        title: logic::suggestion_title(s).into(),
        why: s.why.clone().into(),
        result: s.result.clone().unwrap_or_default().into(),
        status: s.status.clone().into(),
        has_session: s.session_id.is_some(),
    }
}

/// A run step's label. The web UI maps tool names through its label table
/// first, then strips the `mcp__` prefix and turns `__` into ` → `.
/// This glue ports only the fallback. The shared label table belongs to
/// the Chat lane. See `docs/slint-views-bridge.md`.
fn step_label(what: &str) -> String {
    what.strip_prefix("mcp__")
        .unwrap_or(what)
        .replace("__", " → ")
}

/// A run's steps as rows, plus the title line above them. The web UI shows
/// no title, only the steps. The title here names the run so the section
/// is labeled. Step status labels mirror the web UI: ok is "done", failed
/// is "failed", waiting is "waiting for you". Any other status shows as it
/// is. A step that is not ok renders red, and a run error renders red.
fn run_step_rows(run: &logic::ScheduleRun) -> (String, Vec<RunStepRow>) {
    let title = format!("Last run: {}", logic::fmt_time(run.ts));
    let mut rows: Vec<RunStepRow> = run
        .record
        .steps
        .iter()
        .map(|st| {
            let status = match st.status.as_str() {
                "ok" => "done",
                "failed" => "failed",
                "waiting" => "waiting for you",
                other => other,
            };
            let mut text = format!("{} — {}", step_label(&st.what), status);
            if let Some(out) = &st.output {
                let first: String = out.chars().take(160).collect();
                text.push_str(&format!(": {first}"));
            }
            RunStepRow {
                text: text.into(),
                failed: st.status != "ok",
            }
        })
        .collect();
    if let Some(err) = &run.record.error {
        rows.push(RunStepRow {
            text: err.clone().into(),
            failed: true,
        });
    }
    (title, rows)
}

fn audit_row(a: &logic::AuditEntry) -> AuditRow {
    AuditRow {
        when: logic::fmt_time(a.ts).into(),
        action: step_label(&a.tool).into(),
        details: a.args.clone().into(),
        outcome: logic::decision_label(&a.decision).into(),
        denied: a.decision.starts_with("denied"),
    }
}

/// The 30-day chart rows, oldest first. The last row is today. Heights
/// follow the chart rule: at least 3 percent when the day had any use.
fn usage_day_rows(u: &logic::UsageReport) -> Vec<UsageDayRow> {
    let max = u.days.iter().map(|(_, n)| *n).max().unwrap_or(0);
    let len = u.days.len();
    u.days
        .iter()
        .enumerate()
        .map(|(i, (day, n))| UsageDayRow {
            label: logic::day_label(day).into(),
            tokens: logic::fmt_int(*n).into(),
            frac: logic::bar_frac(*n, max),
            today: i + 1 == len,
        })
        .collect()
}

fn model_row(m: &(String, u64, u64, u64)) -> ModelRow {
    ModelRow {
        model: m.0.clone().into(),
        sent: logic::fmt_int(m.1).into(),
        written: logic::fmt_int(m.2).into(),
        calls: logic::fmt_int(m.3).into(),
    }
}

/// The daily figures table, newest first: the same figures as the chart.
fn daily_rows(u: &logic::UsageReport) -> Vec<SimpleRow> {
    u.days
        .iter()
        .rev()
        .map(|(day, n)| SimpleRow {
            text: logic::day_label(day).into(),
            meta: logic::fmt_int(*n).into(),
        })
        .collect()
}

fn model_of<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
    std::rc::Rc::new(VecModel::from_slice(&rows)).into()
}

fn now_ts() -> i64 {
    chrono::Local::now().timestamp()
}

fn window_weak(ctrl: &Arc<ViewsController>) -> Option<Weak<MainWindow>> {
    ctrl.window.lock().unwrap().clone()
}

// ---------- sidebar ----------

/// Reload the sidebar badges, footer, and session list through the Window.
pub fn refresh_sidebar(ui: Weak<MainWindow>, ctrl: &Arc<ViewsController>) {
    let ctrl = ctrl.clone();
    let rt = ctrl.rt.clone();
    rt.spawn(async move {
        let (state_res, sessions_res) =
            tokio::join!(ctrl.bridge.get_state(), ctrl.bridge.list_sessions());
        let Ok(state) = state_res else { return };
        let Ok(sessions) = sessions_res else { return };
        let (unread, suggestions, waiting) = {
            let mut st = ctrl.state.lock().unwrap();
            st.state = Some(state.clone());
            st.sessions = sessions.clone();
            st.badges()
        };
        let footer = logic::sidebar_footer(&state);
        let current = ctrl.state.lock().unwrap().current_session.clone();
        let rows: Vec<SessionRow> = sessions
            .iter()
            .filter(|s| s.messages > 0)
            .map(|s| session_row(s, current.as_deref()))
            .collect();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(window) = ui.upgrade() {
                window.set_sidebar_unread_count(unread as i32);
                window.set_sidebar_suggestions_count(suggestions as i32);
                window.set_sidebar_waiting_count(waiting as i32);
                window.set_sidebar_footer_text(footer.into());
                window.set_sidebar_sessions(model_of(rows));
            }
        });
    });
}

// ---------- memory ----------

/// Reload the Memory list from the bridge and post it to the Window.
/// A bridge error leaves the old rows in place. There is no error
/// surface in the Memory view, so a failure is silent by design.
pub fn refresh_memory(ui: Weak<MainWindow>, ctrl: &Arc<ViewsController>) {
    let ctrl = ctrl.clone();
    let rt = ctrl.rt.clone();
    rt.spawn(async move {
        let query = {
            let st = ctrl.state.lock().unwrap();
            st.memory_query.clone()
        };
        let q = if query.trim().is_empty() {
            None
        } else {
            Some(query.as_str())
        };
        let Ok(items) = ctrl.bridge.list_memories(q).await else {
            return;
        };
        let now = now_ts();
        let rows: Vec<MemoryRow> = items.iter().map(|m| memory_row(m, now)).collect();
        let empty: SharedString = if items.is_empty() {
            if query.trim().is_empty() {
                "Nothing remembered yet. Tell Heron about yourself, or add something above.".into()
            } else {
                format!("Nothing in memory matches \"{query}\".").into()
            }
        } else {
            SharedString::default()
        };
        {
            let mut st = ctrl.state.lock().unwrap();
            st.memories = items;
        }
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(window) = ui.upgrade() {
                window.set_memory_rows(model_of(rows));
                window.set_memory_empty_text(empty);
            }
        });
    });
}

// ---------- tasks ----------

pub fn refresh_tasks(ui: Weak<MainWindow>, ctrl: &Arc<ViewsController>) {
    let ctrl = ctrl.clone();
    let rt = ctrl.rt.clone();
    rt.spawn(async move {
        let Ok(items) = ctrl.bridge.list_tasks().await else {
            return;
        };
        let rows: Vec<TaskRow> = items.iter().map(task_row).collect();
        let empty: SharedString = if items.is_empty() {
            "No tasks yet. Add one above, or ask Heron to.".into()
        } else {
            SharedString::default()
        };
        {
            let mut st = ctrl.state.lock().unwrap();
            st.tasks = items;
        }
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(window) = ui.upgrade() {
                window.set_tasks_rows(model_of(rows));
                window.set_tasks_empty_text(empty);
            }
        });
    });
}

// ---------- reminders ----------

/// Reload schedules and the delivered inbox, then mark unread delivered
/// items read and refresh the sidebar badges, as the web UI does on load.
pub fn refresh_reminders(ui: Weak<MainWindow>, ctrl: &Arc<ViewsController>) {
    let ctrl = ctrl.clone();
    let rt = ctrl.rt.clone();
    rt.spawn(async move {
        let (schedules_res, inbox_res) =
            tokio::join!(ctrl.bridge.list_schedules(), ctrl.bridge.get_inbox());
        let Ok(all) = schedules_res else { return };
        let Ok(inbox) = inbox_res else { return };
        let schedules: Vec<logic::ScheduleItem> = all.into_iter().filter(|s| s.enabled).collect();
        let rows: Vec<ScheduleRow> = schedules.iter().map(schedule_row).collect();
        let delivered: Vec<InboxRow> = inbox.iter().map(inbox_row).collect();
        let schedules_empty: SharedString = if schedules.is_empty() {
            "Nothing scheduled.".into()
        } else {
            SharedString::default()
        };
        let delivered_empty: SharedString = if inbox.is_empty() {
            "Results of scheduled runs appear here.".into()
        } else {
            SharedString::default()
        };
        let any_unread = inbox.iter().any(|m| !m.read);
        {
            let mut st = ctrl.state.lock().unwrap();
            st.schedules = schedules;
            st.inbox = inbox;
        }
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(window) = ui.upgrade() {
                window.set_reminders_schedules(model_of(rows));
                window.set_reminders_schedules_empty_text(schedules_empty);
                window.set_reminders_delivered(model_of(delivered));
                window.set_reminders_delivered_empty_text(delivered_empty);
            }
        });
        if any_unread {
            if ctrl.bridge.read_inbox().await.is_ok() {
                ctrl.refresh_sidebar_state();
            }
        }
    });
}

/// Load the newest run of a schedule and show its steps under the list.
/// The web UI takes the first run the server returns. A run error renders
/// as a red row after the steps.
pub fn show_last_run(ui: Weak<MainWindow>, ctrl: &Arc<ViewsController>, id: i64) {
    let ctrl = ctrl.clone();
    let rt = ctrl.rt.clone();
    rt.spawn(async move {
        let Ok(runs) = ctrl.bridge.schedule_runs(id).await else {
            return;
        };
        let Some(run) = runs.into_iter().next() else {
            return;
        };
        let (title, steps) = run_step_rows(&run);
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(window) = ui.upgrade() {
                window.set_reminders_last_run_title(title.into());
                window.set_reminders_last_run_steps(model_of(steps));
            }
        });
    });
}

/// Poll until the schedule's `last_run` advances, then reload and show the
/// run. Mirrors the web UI: 60 polls of 1.5 seconds, and a missing schedule
/// ends the poll early.
fn poll_schedule_run(ui: Weak<MainWindow>, ctrl: &Arc<ViewsController>, id: i64) {
    let before = {
        let st = ctrl.state.lock().unwrap();
        st.schedules
            .iter()
            .find(|s| s.id == id)
            .and_then(|s| s.last_run)
    };
    let ctrl = ctrl.clone();
    let rt = ctrl.rt.clone();
    rt.spawn(async move {
        let mut advanced = false;
        for _ in 0..RUN_POLL_COUNT {
            tokio::time::sleep(RUN_POLL_INTERVAL).await;
            let Ok(all) = ctrl.bridge.list_schedules().await else {
                return;
            };
            let Some(s) = all.iter().find(|s| s.id == id) else {
                break;
            };
            if s.last_run != before {
                advanced = true;
                break;
            }
        }
        refresh_reminders(ui.clone(), &ctrl);
        if advanced {
            show_last_run(ui, &ctrl, id);
        }
        ctrl.refresh_sidebar_state();
    });
}

// ---------- awareness ----------

/// Reload the awareness object and the suggestion list and post them to
/// the Window. The pause flag comes from `GET /state`, so state loads here
/// too. The proactive note is computed in `views.rs`, never here.
pub fn refresh_awareness(ui: Weak<MainWindow>, ctrl: &Arc<ViewsController>) {
    let ctrl = ctrl.clone();
    let rt = ctrl.rt.clone();
    rt.spawn(async move {
        let (state_res, awareness_res, suggestions_res) = tokio::join!(
            ctrl.bridge.get_state(),
            ctrl.bridge.get_awareness(),
            ctrl.bridge.list_suggestions()
        );
        let Ok(state) = state_res else { return };
        let Ok(awareness) = awareness_res else { return };
        let Ok(suggestions) = suggestions_res else { return };
        let suggestion_rows: Vec<SuggestionRow> =
            suggestions.iter().map(suggestion_row).collect();
        let suggestions_empty: SharedString = if suggestions.is_empty() {
            "No suggestions right now. When Heron sees something it could take off your hands, it appears here.".into()
        } else {
            SharedString::default()
        };
        let (right_now, right_now_empty, insights, insights_empty, journal, journal_empty) =
            {
                let mut st = ctrl.state.lock().unwrap();
                st.state = Some(state.clone());
                st.awareness = Some(awareness.clone());
                st.suggestions = suggestions;
                awareness_rows(&st)
            };
        let paused = state.paused;
        let watching = awareness.enabled;
        let reflecting = awareness.reflect;
        let workspace = awareness.workspace.clone();
        let proactive = awareness.proactive.clone();
        let note = {
            let st = ctrl.state.lock().unwrap();
            st.proactive_note()
        };
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(window) = ui.upgrade() {
                window.set_awareness_paused(paused);
                window.set_awareness_watching(watching);
                window.set_awareness_reflecting(reflecting);
                window.set_awareness_workspace(workspace.into());
                window.set_awareness_proactive(proactive.into());
                window.set_awareness_proactive_note(note.into());
                window.set_awareness_suggestions(model_of(suggestion_rows));
                window.set_awareness_suggestions_empty_text(suggestions_empty);
                window.set_awareness_right_now(model_of(right_now));
                window.set_awareness_right_now_empty_text(right_now_empty);
                window.set_awareness_insights(model_of(insights));
                window.set_awareness_insights_empty_text(insights_empty);
                window.set_awareness_journal(model_of(journal));
                window.set_awareness_journal_empty_text(journal_empty);
            }
        });
    });
}

/// The Awareness rows that depend on the loaded state: right-now, insights,
/// and journal, each with its empty text. The strings come from the web UI.
/// Watching off shows one right-now line and the journal empty text
/// "Nothing is being recorded."
fn awareness_rows(
    st: &logic::ViewsState,
) -> (
    Vec<SimpleRow>,
    SharedString,
    Vec<MemoryRow>,
    SharedString,
    Vec<SimpleRow>,
    SharedString,
) {
    let Some(a) = &st.awareness else {
        return (
            Vec::new(),
            SharedString::default(),
            Vec::new(),
            SharedString::default(),
            Vec::new(),
            SharedString::default(),
        );
    };
    let now = now_ts();
    let right_now: Vec<SimpleRow> = if !a.enabled {
        vec![SimpleRow {
            text: "Background watching is off. Heron only knows what you tell it.".into(),
            meta: SharedString::default(),
        }]
    } else {
        st.right_now_rows()
            .into_iter()
            .map(|(text, meta)| SimpleRow {
                text: text.into(),
                meta: meta.into(),
            })
            .collect()
    };
    let insights: Vec<MemoryRow> = a.insights.iter().map(|m| memory_row(m, now)).collect();
    let insights_empty: SharedString = if insights.is_empty() {
        if !a.reflect {
            "Turn on 'Think about what it notices' to let Heron draw conclusions.".into()
        } else if a.demo {
            "Demo mode cannot reflect. Connect a model to get insights.".into()
        } else {
            "Nothing yet. Heron reflects after it has seen enough activity, at most every six hours.".into()
        }
    } else {
        SharedString::default()
    };
    let journal: Vec<SimpleRow> = a
        .journal
        .iter()
        .take(40)
        .map(|j| SimpleRow {
            text: j.path.clone().into(),
            meta: format!("Changed {}", logic::fmt_time(j.ts)).into(),
        })
        .collect();
    let journal_empty: SharedString = if journal.is_empty() {
        if a.enabled {
            "Nothing noticed yet. Changes in your folder appear here within a minute.".into()
        } else {
            "Nothing is being recorded.".into()
        }
    } else {
        SharedString::default()
    };
    (
        right_now,
        SharedString::default(),
        insights,
        insights_empty,
        journal,
        journal_empty,
    )
}

// ---------- usage ----------

/// Reload the Usage report and post every tile, the chart, the models
/// table, and the limits form to the Window. A bridge error leaves the
/// old figures in place. There is no error surface on load; the save
/// error text covers only failed saves.
pub fn refresh_usage(ui: Weak<MainWindow>, ctrl: &Arc<ViewsController>) {
    let ctrl = ctrl.clone();
    let rt = ctrl.rt.clone();
    rt.spawn(async move {
        let Ok(u) = ctrl.bridge.get_usage().await else {
            return;
        };
        let b = &u.budget;
        let level_note: SharedString = logic::level_note(u.demo, &b.level).into();
        let today = b.today.clone();
        let month = b.month.clone();
        let (today_big, today_small, today_frac, today_state) = (
            logic::tok(today.used).into(),
            logic::tile_small(&today).into(),
            logic::meter_frac(&today),
            logic::meter_state(&today).to_string(),
        );
        let (month_big, month_small, month_frac, month_state) = (
            logic::tok(month.used).into(),
            logic::tile_small(&month).into(),
            logic::meter_frac(&month),
            logic::meter_state(&month).to_string(),
        );
        let heading_big: SharedString = logic::tok(b.projected_month).into();
        let heading_small: SharedString =
            logic::heading_small(&month, &b.limit_reached_on).into();
        let (provider_visible, provider_title, provider_big, provider_small, provider_frac, provider_state) =
            match &u.provider {
                Some(p) => {
                    // Money tiles show dollars with two decimals, not tok().
                    // The meter uses the credit share, computed in f64 so
                    // truncation cannot flip the low boundary.
                    let big: SharedString = if p.credits_bought > 0.0 {
                        format!("${:.2} left", p.credits_left.max(0.0)).into()
                    } else {
                        "None".into()
                    };
                    let small: SharedString = if p.credits_bought > 0.0 {
                        format!("left of ${:.2} bought", p.credits_bought).into()
                    } else {
                        "No credit bought on this account, so only free models will answer.".into()
                    };
                    let (frac, state) = if p.credits_bought > 0.0 {
                        let left = p.credits_left.max(0.0);
                        let frac = ((p.credits_used / p.credits_bought) as f32).min(1.0).max(0.0);
                        let state = if left == 0.0 {
                            "out"
                        } else if left / p.credits_bought <= 0.25 {
                            "low"
                        } else {
                            "normal"
                        };
                        (frac, state.to_string())
                    } else {
                        (-1.0, "normal".to_string())
                    };
                    (true, format!("{} credit", p.name).into(), big, small, frac, state)
                }
                None => (false, SharedString::default(), SharedString::default(), SharedString::default(), -1.0, "normal".to_string()),
            };
        let days = usage_day_rows(&u);
        let pace: SharedString = logic::pace_note(b.daily_average).into();
        let models: Vec<ModelRow> = u.models.iter().map(model_row).collect();
        let models_empty: SharedString = if models.is_empty() {
            "No model calls this month.".into()
        } else {
            SharedString::default()
        };
        let models_note_visible = !models.is_empty();
        let daily = daily_rows(&u);
        let day_limit: SharedString = if b.today.limit == 0 {
            SharedString::default()
        } else {
            b.today.limit.to_string().into()
        };
        let month_limit: SharedString = if b.month.limit == 0 {
            SharedString::default()
        } else {
            b.month.limit.to_string().into()
        };
        let bg_limit: SharedString = if u.background_limit == 0 {
            SharedString::default()
        } else {
            u.background_limit.to_string().into()
        };
        let bg_note: SharedString = format!(
            "Suggestions, preparing ahead, taking over and reflection. {} used today. Reminders you set up yourself always run.",
            logic::fmt_int(u.background_today)
        )
        .into();
        let fast_model: SharedString = u.fast_model.clone().into();
        let fast_note: SharedString = format!("Your main model is {}. Name a cheaper one here to let Heron choose between them.", u.main_model).into();
        let main_model: SharedString = u.main_model.clone().into();
        let route_checked = u.routing;
        {
            let mut st = ctrl.state.lock().unwrap();
            st.usage = Some(u);
        }
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(window) = ui.upgrade() {
                window.set_usage_level_note(level_note);
                window.set_usage_today_big(today_big);
                window.set_usage_today_small(today_small);
                window.set_usage_today_frac(today_frac);
                window.set_usage_today_state(today_state.into());
                window.set_usage_month_big(month_big);
                window.set_usage_month_small(month_small);
                window.set_usage_month_frac(month_frac);
                window.set_usage_month_state(month_state.into());
                window.set_usage_heading_big(heading_big);
                window.set_usage_heading_small(heading_small);
                window.set_usage_provider_visible(provider_visible);
                window.set_usage_provider_title(provider_title);
                window.set_usage_provider_big(provider_big);
                window.set_usage_provider_small(provider_small);
                window.set_usage_provider_frac(provider_frac);
                window.set_usage_provider_state(provider_state.into());
                window.set_usage_days(model_of(days));
                window.set_usage_pace_note(pace);
                window.set_usage_models(model_of(models));
                window.set_usage_models_empty_text(models_empty);
                window.set_usage_models_note_visible(models_note_visible);
                window.set_usage_daily_rows(model_of(daily));
                window.set_usage_day_limit(day_limit);
                window.set_usage_month_limit(month_limit);
                window.set_usage_bg_limit(bg_limit);
                window.set_usage_bg_note(bg_note);
                window.set_usage_fast_model(fast_model);
                window.set_usage_fast_note(fast_note);
                window.set_usage_main_model(main_model);
                window.set_usage_route_checked(route_checked);
            }
        });
    });
}

// ---------- activity ----------

/// Reload the audit log and post the seal line and rows to the Window.
/// A bridge error leaves the old rows in place.
pub fn refresh_activity(ui: Weak<MainWindow>, ctrl: &Arc<ViewsController>) {
    let ctrl = ctrl.clone();
    let rt = ctrl.rt.clone();
    rt.spawn(async move {
        let Ok(log) = ctrl.bridge.get_audit().await else {
            return;
        };
        let rows: Vec<AuditRow> = log.entries.iter().map(audit_row).collect();
        let (seal_text, seal_danger, empty_text): (SharedString, bool, SharedString) =
            if rows.is_empty() {
                (
                    SharedString::default(),
                    false,
                    "Nothing yet. Actions appear here as Heron works.".into(),
                )
            } else {
                (
                    logic::seal_line(log.checked, log.intact).into(),
                    !log.intact,
                    SharedString::default(),
                )
            };
        {
            let mut st = ctrl.state.lock().unwrap();
            st.audit = Some(log);
        }
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(window) = ui.upgrade() {
                window.set_activity_rows(model_of(rows));
                window.set_activity_seal_text(seal_text);
                window.set_activity_seal_danger(seal_danger);
                window.set_activity_empty_text(empty_text);
            }
        });
    });
}

// ---------- wiring: one call binds the whole Window ----------

/// Wire every forwarded callback on the Window and load the first state.
/// The Shell calls this once, after creating the MainWindow. The Shell
/// also calls `refresh_sidebar_state` on its 30-second tick.
pub fn bind_window(window: &MainWindow, ctrl: &Arc<ViewsController>) {
    *ctrl.window.lock().unwrap() = Some(window.as_weak());

    let c = ctrl.clone();
    window.on_sidebar_navigate(move |view| (c.on_navigate)(view.to_string()));
    let c = ctrl.clone();
    window.on_sidebar_open_session(move |id| (c.on_open_session)(id.to_string()));
    let c = ctrl.clone();
    window.on_sidebar_new_conversation(move || (c.on_new_conversation)());

    let weak = window.as_weak();
    let c = ctrl.clone();
    window.on_memory_add_memory(move |content, kind| {
        let content = content.to_string();
        if content.trim().is_empty() {
            return;
        }
        let kind = kind.to_string();
        let ctrl = c.clone();
        let rt = ctrl.rt.clone();
        let weak = weak.clone();
        rt.spawn(async move {
            if ctrl.bridge.add_memory(&content, &kind).await.is_err() {
                return;
            }
            let weak_for_clear = weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(window) = weak_for_clear.upgrade() {
                    window.set_memory_new_content(SharedString::default());
                }
            });
            refresh_memory(weak, &ctrl);
            ctrl.refresh_sidebar_state();
        });
    });
    let weak = window.as_weak();
    let c = ctrl.clone();
    window.on_memory_delete_memory(move |id| {
        let ctrl = c.clone();
        let rt = ctrl.rt.clone();
        let weak = weak.clone();
        rt.spawn(async move {
            if ctrl.bridge.delete_memory(id as i64).await.is_err() {
                return;
            }
            refresh_memory(weak, &ctrl);
            ctrl.refresh_sidebar_state();
        });
    });
    let weak = window.as_weak();
    let c = ctrl.clone();
    window.on_memory_confirm_memory(move |id| {
        let ctrl = c.clone();
        let rt = ctrl.rt.clone();
        let weak = weak.clone();
        rt.spawn(async move {
            if ctrl.bridge.confirm_memory(id as i64).await.is_err() {
                return;
            }
            refresh_memory(weak, &ctrl);
            ctrl.refresh_sidebar_state();
        });
    });
    let weak = window.as_weak();
    let c = ctrl.clone();
    window.on_memory_search_changed(move |query| {
        let seq = c.search_seq.fetch_add(1, Ordering::SeqCst) + 1;
        {
            let mut st = c.state.lock().unwrap();
            st.memory_query = query.to_string();
        }
        let ctrl = c.clone();
        let rt = ctrl.rt.clone();
        let weak = weak.clone();
        rt.spawn(async move {
            tokio::time::sleep(SEARCH_DEBOUNCE).await;
            if ctrl.search_seq.load(Ordering::SeqCst) != seq {
                return;
            }
            refresh_memory(weak, &ctrl);
        });
    });

    let weak = window.as_weak();
    let c = ctrl.clone();
    window.on_tasks_add_task(move |title| {
        let title = title.to_string();
        if title.trim().is_empty() {
            return;
        }
        let ctrl = c.clone();
        let rt = ctrl.rt.clone();
        let weak = weak.clone();
        rt.spawn(async move {
            if ctrl.bridge.add_task(&title).await.is_err() {
                return;
            }
            let weak_for_clear = weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(window) = weak_for_clear.upgrade() {
                    window.set_tasks_new_title(SharedString::default());
                }
            });
            refresh_tasks(weak, &ctrl);
        });
    });
    let weak = window.as_weak();
    let c = ctrl.clone();
    window.on_tasks_set_task_done(move |id, done| {
        let status = if done { "done" } else { "open" };
        let ctrl = c.clone();
        let rt = ctrl.rt.clone();
        let weak = weak.clone();
        rt.spawn(async move {
            if ctrl
                .bridge
                .set_task_status(id as i64, status)
                .await
                .is_err()
            {
                return;
            }
            refresh_tasks(weak, &ctrl);
        });
    });
    let weak = window.as_weak();
    let c = ctrl.clone();
    window.on_tasks_delete_task(move |id| {
        let ctrl = c.clone();
        let rt = ctrl.rt.clone();
        let weak = weak.clone();
        rt.spawn(async move {
            if ctrl.bridge.delete_task(id as i64).await.is_err() {
                return;
            }
            refresh_tasks(weak, &ctrl);
        });
    });

    let weak = window.as_weak();
    let c = ctrl.clone();
    window.on_reminders_add_schedule(move |what, when, may, quiet| {
        let (what, when, may) = (what.to_string(), when.to_string(), may.to_string());
        if what.trim().is_empty() || when.trim().is_empty() {
            return;
        }
        let ctrl = c.clone();
        let rt = ctrl.rt.clone();
        let weak = weak.clone();
        rt.spawn(async move {
            match ctrl.bridge.add_schedule(&what, &when, &may, quiet).await {
                Ok(_) => {
                    let weak_for_clear = weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(window) = weak_for_clear.upgrade() {
                            window.set_reminders_new_what(SharedString::default());
                            window.set_reminders_new_when(SharedString::default());
                            window.set_reminders_new_may("read".into());
                            window.set_reminders_new_may_index(0);
                            window.set_reminders_new_quiet(false);
                            window.set_reminders_form_error(SharedString::default());
                        }
                    });
                    refresh_reminders(weak, &ctrl);
                }
                Err(err) => {
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(window) = weak.upgrade() {
                            window.set_reminders_form_error(err.into());
                        }
                    });
                }
            }
        });
    });
    let weak = window.as_weak();
    let c = ctrl.clone();
    window.on_reminders_cancel_schedule(move |id| {
        let ctrl = c.clone();
        let rt = ctrl.rt.clone();
        let weak = weak.clone();
        rt.spawn(async move {
            if ctrl.bridge.delete_schedule(id as i64).await.is_err() {
                return;
            }
            refresh_reminders(weak, &ctrl);
        });
    });
    let weak = window.as_weak();
    let c = ctrl.clone();
    window.on_reminders_run_schedule_now(move |id| {
        let ctrl = c.clone();
        let rt = ctrl.rt.clone();
        let weak = weak.clone();
        rt.spawn(async move {
            if ctrl.bridge.run_schedule_now(id as i64).await.is_err() {
                return;
            }
            poll_schedule_run(weak, &ctrl, id as i64);
        });
    });
    let c = ctrl.clone();
    window.on_reminders_set_schedule_may(move |id, may| {
        let may = may.to_string();
        let ctrl = c.clone();
        let rt = ctrl.rt.clone();
        rt.spawn(async move {
            let _ = ctrl.bridge.set_schedule_may(id as i64, &may).await;
        });
    });
    let weak = window.as_weak();
    let c = ctrl.clone();
    window.on_reminders_show_last_run(move |id| {
        show_last_run(weak.clone(), &c, id as i64);
    });
    let weak = window.as_weak();
    window.on_reminders_fill_daily_briefing(move || {
        if let Some(window) = weak.upgrade() {
            window.set_reminders_new_what("Give me a short morning briefing: my open tasks by priority, anything due today, and what I was working on most recently in my folder.".into());
            window.set_reminders_new_when("daily 08:00".into());
            window.set_reminders_new_may("read".into());
            window.set_reminders_new_may_index(0);
            window.set_reminders_new_quiet(false);
        }
    });
    let weak = window.as_weak();
    window.on_reminders_fill_inbox_watch(move || {
        if let Some(window) = weak.upgrade() {
            window.set_reminders_new_what("Look at the new email. If it needs a reply from me today, tell me who it is from, what they need and a suggested reply. Do not reply yourself.".into());
            window.set_reminders_new_when("when an email arrives".into());
            window.set_reminders_new_may("read".into());
            window.set_reminders_new_may_index(0);
            window.set_reminders_new_quiet(true);
        }
    });

    let weak = window.as_weak();
    let c = ctrl.clone();
    window.on_awareness_set_paused(move |paused| {
        let ctrl = c.clone();
        let rt = ctrl.rt.clone();
        let weak = weak.clone();
        rt.spawn(async move {
            if ctrl.bridge.set_paused(paused).await.is_err() {
                return;
            }
            refresh_awareness(weak, &ctrl);
            ctrl.refresh_sidebar_state();
        });
    });
    let weak = window.as_weak();
    let c = ctrl.clone();
    window.on_awareness_set_watching(move |enabled| {
        let ctrl = c.clone();
        let rt = ctrl.rt.clone();
        let weak = weak.clone();
        rt.spawn(async move {
            if ctrl.bridge.set_awareness_enabled(enabled).await.is_err() {
                return;
            }
            refresh_awareness(weak, &ctrl);
        });
    });
    let weak = window.as_weak();
    let c = ctrl.clone();
    window.on_awareness_set_reflecting(move |reflect| {
        let ctrl = c.clone();
        let rt = ctrl.rt.clone();
        let weak = weak.clone();
        rt.spawn(async move {
            if ctrl.bridge.set_awareness_reflect(reflect).await.is_err() {
                return;
            }
            refresh_awareness(weak, &ctrl);
        });
    });
    let weak = window.as_weak();
    let c = ctrl.clone();
    window.on_awareness_set_proactive(move |level| {
        let level = level.to_string();
        // OPEN: the web UI confirms before "take over". There is no
        // confirm dialog in Slint yet, so this posts directly. The Shell
        // must gate it until a confirm step exists.
        let ctrl = c.clone();
        let rt = ctrl.rt.clone();
        let weak = weak.clone();
        rt.spawn(async move {
            if ctrl.bridge.set_awareness_proactive(&level).await.is_err() {
                return;
            }
            refresh_awareness(weak, &ctrl);
        });
    });
    let weak = window.as_weak();
    let c = ctrl.clone();
    window.on_awareness_clear_observations(move || {
        // OPEN: the web UI confirms before this delete. Same missing
        // confirm dialog as "take over". Posts directly for now.
        let ctrl = c.clone();
        let rt = ctrl.rt.clone();
        let weak = weak.clone();
        rt.spawn(async move {
            if ctrl.bridge.clear_awareness().await.is_err() {
                return;
            }
            refresh_awareness(weak.clone(), &ctrl);
            refresh_memory(weak, &ctrl);
        });
    });
    let weak = window.as_weak();
    let c = ctrl.clone();
    window.on_awareness_forget_insight(move |id| {
        let ctrl = c.clone();
        let rt = ctrl.rt.clone();
        let weak = weak.clone();
        rt.spawn(async move {
            if ctrl.bridge.delete_memory(id as i64).await.is_err() {
                return;
            }
            refresh_awareness(weak, &ctrl);
        });
    });
    let weak = window.as_weak();
    let c = ctrl.clone();
    window.on_awareness_answer_suggestion(move |id, action| {
        let action = action.to_string();
        let ctrl = c.clone();
        let rt = ctrl.rt.clone();
        let weak = weak.clone();
        rt.spawn(async move {
            // The web UI answers "accept" for a done suggestion even on
            // its dismiss button ("Got it"). The .slint sends "dismiss",
            // so the glue maps it here from the stored status.
            let action = if action == "dismiss" {
                let st = ctrl.state.lock().unwrap();
                let is_done = st
                    .suggestions
                    .iter()
                    .find(|s| s.id == id as i64)
                    .map(|s| s.status == "done")
                    .unwrap_or(false);
                if is_done {
                    "accept".to_string()
                } else {
                    action
                }
            } else {
                action
            };
            match ctrl.bridge.answer_suggestion(id as i64, &action).await {
                Ok((prompt, session_id)) => {
                    if action == "accept" {
                        (ctrl.on_accept_suggestion)(prompt, session_id);
                    }
                    refresh_awareness(weak, &ctrl);
                    ctrl.refresh_sidebar_state();
                }
                Err(_) => {}
            }
        });
    });
    let weak = window.as_weak();
    let c = ctrl.clone();
    window.on_awareness_open_suggestion_session(move |id| {
        let ctrl = c.clone();
        let rt = ctrl.rt.clone();
        let weak = weak.clone();
        rt.spawn(async move {
            // The web UI answers "accept" first, then opens the session.
            let session_id = {
                let st = ctrl.state.lock().unwrap();
                st.suggestions
                    .iter()
                    .find(|s| s.id == id as i64)
                    .and_then(|s| s.session_id.clone())
            };
            let Some(session_id) = session_id else {
                return;
            };
            let _ = ctrl.bridge.answer_suggestion(id as i64, "accept").await;
            (ctrl.on_open_session)(session_id);
            refresh_awareness(weak, &ctrl);
            ctrl.refresh_sidebar_state();
        });
    });

    let weak = window.as_weak();
    let c = ctrl.clone();
    window.on_usage_save_usage(move || {
        // Read the form from the Window on the UI thread, then post from
        // the tokio side. The routing rule mirrors the web UI: no fast
        // model means no routing; a fast model equal to the main model
        // means routing on; otherwise the checkbox decides.
        let (day, month, bg, fast, route_checked, main_model) = match weak.upgrade() {
            Some(window) => (
                logic::parse_limit(&window.get_usage_day_limit()),
                logic::parse_limit(&window.get_usage_month_limit()),
                logic::parse_limit(&window.get_usage_bg_limit()),
                window.get_usage_fast_model().trim().to_string(),
                window.get_usage_route_checked(),
                window.get_usage_main_model().to_string(),
            ),
            None => return,
        };
        let routing = if fast.is_empty() {
            false
        } else if fast == main_model {
            true
        } else {
            route_checked
        };
        let limits = logic::UsageLimits {
            daily_token_limit: day,
            monthly_token_limit: month,
            background_tokens_per_day: bg,
            fast_model: fast,
            routing,
        };
        let ctrl = c.clone();
        let rt = ctrl.rt.clone();
        let weak = weak.clone();
        rt.spawn(async move {
            match ctrl.bridge.save_usage(&limits).await {
                Ok(_) => {
                    let weak_for_saved = weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(window) = weak_for_saved.upgrade() {
                            window.set_usage_saved_visible(true);
                            window.set_usage_save_error(SharedString::default());
                        }
                    });
                    // The web UI shows "Saved" for 1.8 seconds.
                    let weak_for_hide = weak.clone();
                    let rt2 = ctrl.rt.clone();
                    rt2.spawn(async move {
                        tokio::time::sleep(Duration::from_millis(1800)).await;
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(window) = weak_for_hide.upgrade() {
                                window.set_usage_saved_visible(false);
                            }
                        });
                    });
                    refresh_usage(weak, &ctrl);
                    ctrl.refresh_sidebar_state();
                }
                Err(err) => {
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(window) = weak.upgrade() {
                            window.set_usage_save_error(err.into());
                        }
                    });
                }
            }
        });
    });

    ctrl.refresh_sidebar_state();
    refresh_memory(window.as_weak(), ctrl);
    refresh_tasks(window.as_weak(), ctrl);
    refresh_reminders(window.as_weak(), ctrl);
    refresh_awareness(window.as_weak(), ctrl);
    refresh_usage(window.as_weak(), ctrl);
    refresh_activity(window.as_weak(), ctrl);
}
