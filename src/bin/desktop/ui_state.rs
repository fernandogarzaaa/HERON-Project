//! UI state that lives outside the window: the live server-state poll.
//! The current session id lives in the window itself (the `session-id`
//! property), because only the window crosses the thread boundary: a
//! background task reaches it through `Weak::upgrade_in_event_loop`.
//!
//! The poll also drives the rail badges and the chat properties (mode,
//! key, demo, paused, workspace). Badge values are strings, formatted
//! here: empty means no badge, so the `.slint` side never formats a number.
//! Mapping: waiting approvals onto Conversation, unread inbox onto Reminders,
//! unanswered suggestions onto Awareness. Totals that are not actions (the
//! memory count) get no badge.

use super::bridge::{AppState, Bridge};
use crate::MainWindow;
use heron::ui::chat::{workspace_short, ChatView, Mode};
use slint::{SharedString, Weak};
use std::sync::{Arc, Mutex};

/// One line for the status row: `Heron · model · mode`.
pub fn status_line(state: &AppState) -> String {
    format!("{} · {} · {}", state.name, state.model, state.mode)
}

fn badge(count: u64) -> String {
    if count > 0 {
        count.to_string()
    } else {
        String::new()
    }
}

/// Post one state snapshot to the UI thread: the status line, the rail
/// badges, and the chat properties. The ChatView's mode stays in sync
/// with the server's, so the state machine never lies about it.
fn post_state(window: &MainWindow, chat: &Arc<Mutex<ChatView>>, state: &AppState) {
    if let Some(mode) = Mode::parse(&state.mode) {
        chat.lock().unwrap().mode = mode;
    }
    window.set_backend_status(SharedString::from(status_line(state)));
    window.set_waiting_badge(SharedString::from(badge(state.waiting)));
    window.set_unread_badge(SharedString::from(badge(state.unread)));
    window.set_suggestions_badge(SharedString::from(badge(state.suggestions)));
    window.set_chat_mode(SharedString::from(state.mode.as_str()));
    window.set_has_key(state.has_key);
    window.set_demo(state.demo);
    window.set_paused(state.paused);
    window.set_workspace_full(SharedString::from(state.workspace.as_str()));
    window.set_workspace_short(SharedString::from(workspace_short(&state.workspace)));
}

/// Poll `GET /api/state` every 30 seconds (the web UI's cadence) and post
/// it to the UI thread. The first fetch happens at once. The loop ends
/// when the window is gone.
pub fn poll_state(bridge: &Bridge, weak: Weak<MainWindow>, chat: Arc<Mutex<ChatView>>) {
    let client = bridge.client().clone();
    bridge.spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            let state = match client.state().await {
                Ok(state) => state,
                Err(e) => {
                    let weak = weak.clone();
                    let posted = weak.upgrade_in_event_loop(move |window| {
                        window.set_backend_status(SharedString::from(format!(
                            "Connected, but the server state could not be read: {e:#}"
                        )));
                    });
                    if posted.is_err() {
                        break;
                    }
                    continue;
                }
            };
            let weak = weak.clone();
            let chat = chat.clone();
            let posted = weak.upgrade_in_event_loop(move |window| {
                post_state(&window, &chat, &state);
            });
            if posted.is_err() {
                break;
            }
        }
    });
}

/// Fetch state once and post it, now. Used after an action that changes
/// state (try demo, resume), so the view updates without waiting for the
/// next 30-second tick.
pub fn refresh_state_now(bridge: &Bridge, weak: Weak<MainWindow>, chat: Arc<Mutex<ChatView>>) {
    let client = bridge.client().clone();
    bridge.spawn(async move {
        let Ok(state) = client.state().await else {
            return;
        };
        let _ = weak.upgrade_in_event_loop(move |window| {
            post_state(&window, &chat, &state);
        });
    });
}
