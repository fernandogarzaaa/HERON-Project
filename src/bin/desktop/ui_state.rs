//! UI state that lives outside the window: the live server-state poll.
//! The current session id lives in the window itself (the `session-id`
//! property), because only the window crosses the thread boundary: a
//! background task reaches it through `Weak::upgrade_in_event_loop`.
//!
//! The poll also drives the rail badges. Badge values are strings, formatted
//! here: empty means no badge, so the `.slint` side never formats a number.
//! Mapping: waiting approvals onto Conversation, unread inbox onto Reminders,
//! unanswered suggestions onto Awareness. Totals that are not actions (the
//! memory count) get no badge.

use super::bridge::{AppState, Bridge};
use crate::MainWindow;
use slint::{SharedString, Weak};

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

/// Poll `GET /api/state` every 30 seconds (the web UI's cadence) and post the
/// status line and the rail badges to the UI thread. The first fetch happens
/// at once. The loop ends when the window is gone.
pub fn poll_state(bridge: &Bridge, weak: Weak<MainWindow>) {
    let client = bridge.client().clone();
    bridge.spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            let (status, waiting, unread, suggestions) = match client.state().await {
                Ok(state) => (
                    status_line(&state),
                    badge(state.waiting),
                    badge(state.unread),
                    badge(state.suggestions),
                ),
                Err(e) => (
                    format!("Connected, but the server state could not be read: {e:#}"),
                    String::new(),
                    String::new(),
                    String::new(),
                ),
            };
            let weak = weak.clone();
            let posted = weak.upgrade_in_event_loop(move |window| {
                window.set_backend_status(SharedString::from(status));
                window.set_waiting_badge(SharedString::from(waiting));
                window.set_unread_badge(SharedString::from(unread));
                window.set_suggestions_badge(SharedString::from(suggestions));
            });
            if posted.is_err() {
                break;
            }
        }
    });
}
