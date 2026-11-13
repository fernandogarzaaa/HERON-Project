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

/// Append streamed text to the current assistant reply (the last row), creating
/// one when the last row is not an assistant message.
pub fn append_reply_text(window: &MainWindow, delta: &str) {
    let messages = message_model(window);
    let model = vec_model(&messages);
    let count = model.row_count();
    if count > 0 {
        let mut last = model.row_data(count - 1).expect("last row exists");
        if last.role.as_str() == "assistant" {
            let text = format!("{}{delta}", last.text);
            last.body = StyledText::from_plain_text(&text);
            last.text = SharedString::from(text);
            model.set_row_data(count - 1, last);
            return;
        }
    }
    push_message(window, "assistant", delta);
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
            if count > 0 {
                let mut last = model.row_data(count - 1).expect("last row exists");
                if last.role.as_str() == "assistant" {
                    last.body = StyledText::from_markdown(&text)
                        .unwrap_or_else(|_| StyledText::from_plain_text(&text));
                    last.text = SharedString::from(&text);
                    model.set_row_data(count - 1, last);
                }
            }
            window.set_session_title(SharedString::from(title));
            window.set_busy(false);
        }
        ChatEvent::Learned { .. } => {}
        ChatEvent::Error { message } => {
            append_reply_text(window, &format!("\nError: {message}"));
            window.set_busy(false);
        }
        ChatEvent::Cancelled => window.set_busy(false),
    }
}
