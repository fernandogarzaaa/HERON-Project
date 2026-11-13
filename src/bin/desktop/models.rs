//! UI-thread chat sync: the ChatView state machine and the window's message list.
//! Everything here runs on the UI thread only, inside a
//! `Weak::upgrade_in_event_loop` closure. Background tasks never call these.
//!
//! `ChatView` (`heron::ui::chat`) is the live Rust state machine. The
//! window's `messages` model is a render of its thread: one `ChatMsg` row
//! per `ThreadItem`. Each row conversion is cheap while a reply streams
//! (plain text) and parses markdown once, when the turn completes. The
//! sync is incremental: a streamed event re-renders only its own turn's
//! row. A full rebuild happens only when the thread itself is replaced
//! (open history, new conversation).
//!
//! The originating turn id is the identity. `start_send` returns a
//! monotonic id; every streamed event is applied with
//! `apply_event_for_turn(id, event)`, so a late `Learned` lands on the
//! turn that learned it even after `Done`, and events from a cleared
//! conversation are ignored instead of landing in the new one.

use super::bridge::{ChatEvent, SessionDetail};
use crate::{ApprovalRow, ChatMsg, MainWindow, ToolStep};
use heron::llm::Message as LlmMessage;
use heron::ui::chat as view;
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

/// The bridge's event carries `Learned` items as JSON values; the view's
/// state machine carries them as strings. A string item stays itself;
/// any other JSON value becomes its JSON text.
fn convert_event(event: ChatEvent) -> view::ChatEvent {
    match event {
        ChatEvent::Session { id } => view::ChatEvent::Session { id },
        ChatEvent::Text { text } => view::ChatEvent::Text { text },
        ChatEvent::ToolStart {
            id,
            name,
            summary,
            risk,
        } => view::ChatEvent::ToolStart {
            id,
            name,
            summary,
            risk,
        },
        ChatEvent::ToolEnd {
            id,
            name,
            ok,
            preview,
        } => view::ChatEvent::ToolEnd {
            id,
            name,
            ok,
            preview,
        },
        ChatEvent::Notice { text } => view::ChatEvent::Notice { text },
        ChatEvent::Usage { prompt, completion } => view::ChatEvent::Usage { prompt, completion },
        ChatEvent::Model { name, why } => view::ChatEvent::Model { name, why },
        ChatEvent::Approval {
            id,
            tool,
            summary,
            risk,
            detail,
            always_rule,
            can_always,
            can_session,
            tainted,
        } => view::ChatEvent::Approval {
            id,
            tool,
            summary,
            risk,
            detail,
            always_rule,
            can_always,
            can_session,
            tainted,
        },
        ChatEvent::Done { text, title } => view::ChatEvent::Done { text, title },
        ChatEvent::Learned { items } => view::ChatEvent::Learned {
            items: items
                .into_iter()
                .map(|v| match v {
                    serde_json::Value::String(s) => s,
                    other => other.to_string(),
                })
                .collect(),
        },
        ChatEvent::Error { message } => view::ChatEvent::Error { message },
        ChatEvent::Cancelled => view::ChatEvent::Cancelled,
    }
}

/// One thread item as the one `ChatMsg` row the view renders for it.
/// A live turn renders plain text; a completed turn renders markdown
/// parsed once. An answered approval card is replaced by its decision
/// note, as the web UI does: the card leaves the row and the note joins
/// the turn's notes.
fn chat_msg_from_item(item: &view::ThreadItem) -> ChatMsg {
    match item {
        view::ThreadItem::User(text) => ChatMsg {
            role: SharedString::from("user"),
            text: SharedString::from(text.as_str()),
            body: StyledText::from_plain_text(text),
            steps: ModelRc::new(VecModel::<ToolStep>::default()),
            approvals: ModelRc::new(VecModel::<ApprovalRow>::default()),
            notes: ModelRc::new(VecModel::<SharedString>::default()),
            failure: SharedString::default(),
            complete: true,
            live: false,
        },
        view::ThreadItem::Note(text) => ChatMsg {
            role: SharedString::from("note"),
            text: SharedString::from(text.as_str()),
            body: StyledText::from_plain_text(text),
            steps: ModelRc::new(VecModel::<ToolStep>::default()),
            approvals: ModelRc::new(VecModel::<ApprovalRow>::default()),
            notes: ModelRc::new(VecModel::<SharedString>::default()),
            failure: SharedString::default(),
            complete: true,
            live: false,
        },
        view::ThreadItem::Turn(turn) => {
            let body = if turn.live {
                StyledText::from_plain_text(&turn.raw_text)
            } else {
                StyledText::from_markdown(&turn.raw_text)
                    .unwrap_or_else(|_| StyledText::from_plain_text(&turn.raw_text))
            };
            let steps: Vec<ToolStep> = turn
                .steps
                .iter()
                .map(|step| ToolStep {
                    id: SharedString::from(step.id.as_str()),
                    name: SharedString::from(step.name.as_str()),
                    label: SharedString::from(step.label.as_str()),
                    summary: SharedString::from(step.summary.as_str()),
                    risk: SharedString::from(step.risk.as_str()),
                    state: SharedString::from(step.status.clone().wire()),
                    preview: SharedString::from(step.preview.as_str()),
                    expanded: step.expanded,
                })
                .collect();
            let mut notes: Vec<SharedString> = turn
                .notes
                .iter()
                .map(|n| SharedString::from(n.as_str()))
                .collect();
            let approvals: Vec<ApprovalRow> = turn
                .approvals
                .iter()
                .filter_map(|card| match &card.decision_note {
                    Some(note) => {
                        notes.push(SharedString::from(note.as_str()));
                        None
                    }
                    None => Some(ApprovalRow {
                        id: SharedString::from(card.id.as_str()),
                        tool: SharedString::from(card.tool.as_str()),
                        title: SharedString::from(card.title.as_str()),
                        warning: SharedString::from(card.warning.as_str()),
                        detail: SharedString::from(card.detail.as_str()),
                        danger: card.danger,
                        can_session: card.can_session,
                        can_always: card.can_always,
                        session_label: SharedString::from(card.session_label.as_str()),
                        always_label: SharedString::from(card.always_label.as_str()),
                    }),
                })
                .collect();
            ChatMsg {
                role: SharedString::from("assistant"),
                text: SharedString::from(turn.raw_text.as_str()),
                body,
                steps: ModelRc::new(VecModel::from_slice(&steps)),
                approvals: ModelRc::new(VecModel::from_slice(&approvals)),
                notes: ModelRc::new(VecModel::from_slice(&notes)),
                failure: SharedString::from(turn.failure.as_deref().unwrap_or("")),
                complete: turn.complete,
                live: turn.live,
            }
        }
    }
}

/// The row index of the turn with this id, if the thread still holds it.
/// A cleared thread never holds it: that is how stale events are ignored.
fn turn_index(chat: &view::ChatView, turn_id: u64) -> Option<usize> {
    chat.thread.iter().position(|item| match item {
        view::ThreadItem::Turn(turn) => turn.turn_id == turn_id,
        _ => false,
    })
}

/// Re-render one turn's row. A no-op when the turn is gone.
fn sync_turn(window: &MainWindow, chat: &view::ChatView, turn_id: u64) {
    if let Some(index) = turn_index(chat, turn_id) {
        let messages = message_model(window);
        let model = vec_model(&messages);
        if index < model.row_count() {
            let row = chat_msg_from_item(&chat.thread[index]);
            model.set_row_data(index, row);
            bump_thread_version(window);
        }
    }
}

/// Rebuild the whole message list from the thread. Used only when the
/// thread itself is replaced: open history, new conversation.
fn sync_thread(window: &MainWindow, chat: &view::ChatView) {
    let rows: Vec<ChatMsg> = chat.thread.iter().map(chat_msg_from_item).collect();
    window.set_messages(ModelRc::new(VecModel::from_slice(&rows)));
    bump_thread_version(window);
}

/// The window properties the ChatView owns: session id, session title,
/// busy. Called after every mutation that can change them.
fn sync_window_state(window: &MainWindow, chat: &view::ChatView) {
    window.set_session_id(SharedString::from(chat.session_id.as_deref().unwrap_or("")));
    window.set_session_title(SharedString::from(chat.title.as_str()));
    window.set_busy(chat.busy);
}

/// The ChatView watches `thread-version` to follow-scroll: row mutations
/// do not fire a model's changed handler, so the counter is the trigger.
fn bump_thread_version(window: &MainWindow) {
    window.set_thread_version(window.get_thread_version() + 1);
}

/// Start a reply turn: the user bubble and the empty live turn. Returns
/// the monotonic turn id the forwarder must capture for every event.
pub fn begin_send(window: &MainWindow, chat: &mut view::ChatView, text: &str) -> u64 {
    let turn_id = chat.start_send(text);
    let messages = message_model(window);
    let model = vec_model(&messages);
    let len = chat.thread.len();
    for item in &chat.thread[len - 2..] {
        model.push(chat_msg_from_item(item));
    }
    sync_window_state(window, chat);
    bump_thread_version(window);
    turn_id
}

/// One streamed chat event, applied on the UI thread to the originating
/// turn. A stale turn (cleared by New conversation) is ignored.
pub fn apply_chat_event(
    window: &MainWindow,
    chat: &mut view::ChatView,
    turn_id: u64,
    event: ChatEvent,
) {
    let event = convert_event(event);
    chat.apply_event_for_turn(turn_id, event);
    sync_turn(window, chat, turn_id);
    sync_window_state(window, chat);
}

/// The stream ended with no terminal event. Closes the live turn with the
/// connection note, as the ChatView specifies.
pub fn stream_closed_early(window: &MainWindow, chat: &mut view::ChatView) {
    let live = chat.thread.iter().rev().find_map(|item| match item {
        view::ThreadItem::Turn(turn) if turn.live => Some(turn.turn_id),
        _ => None,
    });
    chat.stream_closed_early();
    if let Some(turn_id) = live {
        sync_turn(window, chat, turn_id);
    }
    sync_window_state(window, chat);
}

/// A request failure before the first event. The failure block carries
/// the server's error text.
pub fn request_failed(window: &MainWindow, chat: &mut view::ChatView, message: &str) {
    let live = chat.thread.iter().rev().find_map(|item| match item {
        view::ThreadItem::Turn(turn) if turn.live => Some(turn.turn_id),
        _ => None,
    });
    chat.request_failed(message);
    if let Some(turn_id) = live {
        sync_turn(window, chat, turn_id);
    }
    sync_window_state(window, chat);
}

/// Replay a stored session into the thread, mirroring the web
/// `renderHistory`: user messages become user rows, assistant messages
/// become turns, tool messages fold into the turn's steps, system-notice
/// user messages are skipped, and an interrupted turn gains its note.
pub fn open_history(window: &MainWindow, chat: &mut view::ChatView, detail: &SessionDetail) {
    let messages: Vec<view::HistoryMessage> = detail
        .messages
        .iter()
        .map(|m: &LlmMessage| view::HistoryMessage {
            role: m.role.clone(),
            content: m.content.clone(),
            tool_calls: m
                .tool_calls
                .iter()
                .map(|c| view::HistoryToolCall {
                    id: c.id.clone(),
                    name: c.function.name.clone(),
                    arguments: c.function.arguments.clone(),
                })
                .collect(),
            tool_call_id: m.tool_call_id.clone(),
        })
        .collect();
    chat.open_history(messages, detail.interrupted);
    chat.session_id = Some(detail.id.clone());
    if !detail.title.is_empty() {
        chat.title = detail.title.clone();
    }
    // open_history leaves busy false: this client is not streaming the
    // session's run, so it does not claim to be. A send into a session
    // whose reply is still running fails with 409 and says so.
    sync_thread(window, chat);
    sync_window_state(window, chat);
}

/// Start a new conversation: clear the thread and the session id. The
/// server creates the next session lazily, on the first message sent.
pub fn start_new(window: &MainWindow, chat: &mut view::ChatView) {
    chat.start_new();
    sync_thread(window, chat);
    sync_window_state(window, chat);
}

/// Append a plain note row: undo outcomes and other Rust-side notes.
pub fn push_note(window: &MainWindow, chat: &mut view::ChatView, text: &str) {
    chat.thread.push(view::ThreadItem::Note(text.to_string()));
    let messages = message_model(window);
    vec_model(&messages).push(chat_msg_from_item(chat.thread.last().unwrap()));
    bump_thread_version(window);
}

/// Flip a tool result row open or closed. View state only, no server call.
pub fn toggle_step(window: &MainWindow, chat: &mut view::ChatView, id: &str) {
    chat.toggle_step(id);
    let found = chat.thread.iter().find_map(|item| match item {
        view::ThreadItem::Turn(turn) if turn.steps.iter().any(|step| step.id == id) => {
            Some(turn.turn_id)
        }
        _ => None,
    });
    if let Some(turn_id) = found {
        sync_turn(window, chat, turn_id);
    }
}

/// Record an approval answer. The card is replaced by its decision note
/// on the next sync of its turn. A failed approve call passes its error
/// as the note instead.
pub fn answer_approval(
    window: &MainWindow,
    chat: &mut view::ChatView,
    id: &str,
    decision: &str,
    err: Option<&str>,
) {
    let decision = match decision {
        "once" => view::ApprovalDecision::Once,
        "session" => view::ApprovalDecision::Session,
        "always" => view::ApprovalDecision::Always,
        _ => view::ApprovalDecision::Deny,
    };
    chat.answer_approval(id, decision, err);
    let found = chat.thread.iter().find_map(|item| match item {
        view::ThreadItem::Turn(turn) if turn.approvals.iter().any(|card| card.id == id) => {
            Some(turn.turn_id)
        }
        _ => None,
    });
    if let Some(turn_id) = found {
        sync_turn(window, chat, turn_id);
    }
}
