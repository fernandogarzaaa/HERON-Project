//! UI-thread model helpers: the window's message list and how streamed chat
//! events land in it. Everything here runs on the UI thread only, inside a
//! `Weak::upgrade_in_event_loop` closure. Background tasks never call these.
//!
//! Each row is a `ChatMsg` with both the plain source text and the rendered
//! body. While a reply streams, the body stays plain text; the markdown parse
//! happens once, at `done`. Re-parsing the whole reply on every delta would be
//! O(n) per chunk and visibly lags on long replies.

use super::bridge::ChatEvent;
use crate::{ChatMsg, MainWindow};
use slint::{Model, ModelRc, SharedString, StyledText, VecModel};

/// The window's message list. The model is installed from Rust at startup
/// (see `main`), so the downcast in `vec_model` always succeeds.
///
/// Returned by value on purpose: the caller holds the `ModelRc`, and the
/// downcast borrows from that local. The old shape returned a reference
/// borrowed from the temporary `ModelRc`, which the compiler rejected
/// (E0515: cannot return a value referencing a temporary).
pub fn message_model(window: &MainWindow) -> ModelRc<ChatMsg> {
    window.get_messages()
}

/// The message list as its concrete `VecModel`. The caller must hold the
/// `ModelRc` from `message_model` for as long as this reference is used.
fn vec_model(messages: &ModelRc<ChatMsg>) -> &VecModel<ChatMsg> {
    messages
        .as_any()
        .downcast_ref::<VecModel<ChatMsg>>()
        .expect("the messages model is a VecModel")
}

pub fn push_message(window: &MainWindow, role: &str, text: &str) {
    let messages = message_model(window);
    vec_model(&messages).push(ChatMsg {
        role: SharedString::from(role),
        text: SharedString::from(text),
        body: StyledText::from_plain_text(text),
    });
}

/// Push a new assistant row and record it as this turn's reply row. The
/// reply row is tracked by index, not by role: Notice and Approval rows
/// also use the "assistant" role, so finding the reply by role would land
/// streamed text on a notice row and lose it at Done.
fn push_reply_row(window: &MainWindow, text: &str) {
    push_message(window, "assistant", text);
    let messages = message_model(window);
    let model = vec_model(&messages);
    window.set_reply_row(model.row_count() as i32 - 1);
}

/// Start a reply turn: an empty assistant row that streamed deltas
/// accumulate on. Called by the send handler when a message goes out.
pub fn begin_reply(window: &MainWindow) {
    push_reply_row(window, "");
}

/// Append streamed text to this turn's reply row, creating one when there
/// is no live reply row (a stray event, or the first delta of a turn).
pub fn append_reply_text(window: &MainWindow, delta: &str) {
    let messages = message_model(window);
    let model = vec_model(&messages);
    let count = model.row_count();
    let reply = window.get_reply_row();
    if reply >= 0 && (reply as usize) < count {
        let mut row = model.row_data(reply as usize).expect("reply row exists");
        let text = format!("{}{delta}", row.text);
        row.body = StyledText::from_plain_text(&text);
        row.text = SharedString::from(text);
        model.set_row_data(reply as usize, row);
        return;
    }
    push_reply_row(window, delta);
}

/// One streamed chat event, applied on the UI thread. The Chat lane owns the
/// richer rendering (tool calls, approvals); the shell only keeps the reply
/// text, the session id, and the busy flag correct.
pub fn apply_chat_event(window: &MainWindow, event: ChatEvent) {
    match event {
        ChatEvent::Session { id } => window.set_session_id(SharedString::from(id)),
        ChatEvent::Text { text } => append_reply_text(window, &text),
        ChatEvent::ToolStart { .. }
        | ChatEvent::ToolEnd { .. }
        | ChatEvent::Usage { .. }
        | ChatEvent::Model { .. } => {}
        ChatEvent::Notice { text } => push_message(window, "assistant", &text),
        ChatEvent::Approval { summary, .. } => {
            push_message(
                window,
                "assistant",
                &format!("Heron needs approval: {summary}. Answer it from the Approvals view."),
            );
        }
        ChatEvent::Done { text, title } => {
            let messages = message_model(window);
            let model = vec_model(&messages);
            let count = model.row_count();
            let reply = window.get_reply_row();
            if reply >= 0 && (reply as usize) < count {
                let mut last = model.row_data(reply as usize).expect("reply row exists");
                last.body = StyledText::from_markdown(&text)
                    .unwrap_or_else(|_| StyledText::from_plain_text(&text));
                last.text = SharedString::from(&text);
                model.set_row_data(reply as usize, last);
            }
            window.set_reply_row(-1);
            window.set_session_title(SharedString::from(title));
            window.set_busy(false);
        }
        ChatEvent::Learned { .. } => {}
        ChatEvent::Error { message } => {
            append_reply_text(window, &format!("\nError: {message}"));
            window.set_reply_row(-1);
            window.set_busy(false);
        }
        ChatEvent::Cancelled => {
            window.set_reply_row(-1);
            window.set_busy(false);
        }
    }
}
